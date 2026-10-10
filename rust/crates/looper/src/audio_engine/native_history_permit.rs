//! Current-bank or admitted retained-voice authority for worker-owned source history.
//!
//! This retains only shared atomic owners and a fixed complete projection. Actual PCM/stem pins
//! belong to the worker request/bundle and retire there, never through a dropped callback ticket.

use super::constant_timing::{AcceptedTimingProjection, CurrentTimingAcknowledgements};
use super::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
use crate::messages::SampleBuffer;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub(crate) struct NativeHistoryContext<'a> {
    pub(crate) id: usize,
    pub(crate) ownership: &'a Arc<InputRuntimeOwnership>,
    pub(crate) acknowledgements: &'a Arc<CurrentTimingAcknowledgements>,
    pub(crate) preparation_epoch: &'a Arc<AtomicU64>,
}

/// Source authority captured at an actual admitted voice birth while this source is current.
/// Keeping PCM alone cannot construct this value, and later bank publication cannot relabel it.
#[derive(Clone, Debug)]
pub(crate) struct NativeSourceAdmission {
    id: usize,
    binding: InputPadBinding,
    source_generation: u64,
}

/// Fresh callback capture of a retained voice's actual source and effective timing. The lifetime
/// is preallocated with the voice, while the local preparation epoch is the existing worker fence.
/// Selection, stems, coverage and the exact playback checkpoint are captured by the request.
pub(crate) struct NativeVoiceHistoryContext<'a> {
    pub(crate) admission: &'a NativeSourceAdmission,
    pub(crate) lifetime: &'a Arc<AtomicU64>,
    pub(crate) generation: u64,
    pub(crate) preparation_epoch: &'a Arc<AtomicU64>,
    pub(crate) accepted: Option<AcceptedTimingProjection>,
}

#[derive(Clone)]
enum NativeHistoryAuthority {
    Current {
        ownership: Arc<InputRuntimeOwnership>,
        acknowledgements: Arc<CurrentTimingAcknowledgements>,
    },
    RetainedVoice {
        lifetime: Arc<AtomicU64>,
        generation: u64,
    },
}

#[derive(Clone)]
pub(crate) struct NativeHistoryPermit {
    id: usize,
    binding: InputPadBinding,
    source_generation: u64,
    preparation_epoch: Arc<AtomicU64>,
    expected_epoch: u64,
    authority: NativeHistoryAuthority,
}

impl NativeHistoryContext<'_> {
    pub(crate) fn capture_source_admission(
        &self,
        sample: &SampleBuffer,
        rate: u32,
        accepted: Option<AcceptedTimingProjection>,
    ) -> Option<NativeSourceAdmission> {
        let permit = self.capture(sample, rate, accepted)?;
        Some(NativeSourceAdmission {
            id: permit.id,
            binding: permit.binding,
            source_generation: permit.source_generation,
        })
    }

    pub(crate) fn capture(
        &self,
        sample: &SampleBuffer,
        rate: u32,
        accepted: Option<AcceptedTimingProjection>,
    ) -> Option<NativeHistoryPermit> {
        let source_generation = self.ownership.source_generation(self.id, sample, rate)?;
        let permit = NativeHistoryPermit {
            id: self.id,
            binding: InputPadBinding {
                resident: sample.resident_binding(),
                source_address: sample.source_address(),
                sample_count: sample.source_sample_count(),
                channels: sample.channels,
                sample_rate_hz: rate,
                authority_revision: self.ownership.authority[self.id].load(Ordering::Acquire),
                runtime_revision: self.ownership.runtime[self.id].load(Ordering::Acquire),
                accepted,
            },
            source_generation,
            preparation_epoch: self.preparation_epoch.clone(),
            expected_epoch: self.preparation_epoch.load(Ordering::Acquire),
            authority: NativeHistoryAuthority::Current {
                ownership: self.ownership.clone(),
                acknowledgements: self.acknowledgements.clone(),
            },
        };
        permit.current(sample, rate).then_some(permit)
    }
}

impl NativeVoiceHistoryContext<'_> {
    pub(crate) fn capture(&self, sample: &SampleBuffer, rate: u32) -> Option<NativeHistoryPermit> {
        let admission = self.admission;
        let permit = NativeHistoryPermit {
            id: admission.id,
            binding: InputPadBinding {
                resident: sample.resident_binding(),
                accepted: self.accepted,
                ..admission.binding
            },
            source_generation: admission.source_generation,
            preparation_epoch: self.preparation_epoch.clone(),
            expected_epoch: self.preparation_epoch.load(Ordering::Acquire),
            authority: NativeHistoryAuthority::RetainedVoice {
                lifetime: self.lifetime.clone(),
                generation: self.generation,
            },
        };
        permit.current(sample, rate).then_some(permit)
    }
}

impl NativeHistoryPermit {
    /// Already adopted history owns chronological native/FIFO/filter state. A
    /// storage-only window ACK may fence pending preparation without invalidating
    /// that same source's established trajectory or timing owner.
    pub(crate) fn current_effective_source(&self, sample: &SampleBuffer, rate: u32) -> bool {
        let current = || self.current_inner(sample, rate, false);
        current() && current()
    }
    /// Preparation/request and current source/authority are rechecked at ACTUAL native adoption.
    /// The double check is bounded and has no spin, owner allocation or address dereference.
    pub(crate) fn current(&self, sample: &SampleBuffer, rate: u32) -> bool {
        let current = || self.current_inner(sample, rate, true);
        current() && current()
    }

    fn current_inner(&self, sample: &SampleBuffer, rate: u32, exact_window: bool) -> bool {
        if self.preparation_epoch.load(Ordering::Acquire) != self.expected_epoch
            || sample.source_address() != self.binding.source_address
            || sample.source_sample_count() != self.binding.sample_count
            || sample.channels != self.binding.channels
            || rate != self.binding.sample_rate_hz
        {
            return false;
        }
        match &self.authority {
            NativeHistoryAuthority::Current {
                ownership,
                acknowledgements,
            } => {
                (if exact_window {
                    ownership.current(self.id, self.binding)
                } else {
                    ownership.authority_current(self.id, self.binding)
                }) && ownership.source_generation(self.id, sample, rate)
                    == Some(self.source_generation)
                    && acknowledgements.current_epoch(self.id)
                        == self
                            .binding
                            .accepted
                            .map_or(0, |value| value.publication_epoch)
                    && ownership.source_timing_available(
                        self.id,
                        self.binding.accepted,
                        acknowledgements,
                    )
            }
            NativeHistoryAuthority::RetainedVoice {
                lifetime,
                generation,
            } => {
                // This authority is born only from a separately admitted source and a NEW
                // capture of the actual retained voice. An earlier Current permit never takes
                // this branch, even when that source remains pinned by the voice.
                *generation != 0
                    && *generation != u64::MAX
                    && self.source_generation != 0
                    && self.binding.authority_revision != 0
                    && lifetime.load(Ordering::Acquire) == *generation
                    && (!exact_window || sample.resident_binding() == self.binding.resident)
            }
        }
    }

    pub(crate) fn matches_projection(&self, accepted: Option<AcceptedTimingProjection>) -> bool {
        self.binding.accepted == accepted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{CompleteSourceIdentity, ResidentContext, ResidentSourceView};
    use flitzis_looper_analysis::tempo_acceptance::TimingIntent;

    #[test]
    fn retained_pin_cannot_authorize_replaced_generation_or_new_request() {
        let sample = SampleBuffer {
            residency: None,
            samples: Arc::from([0.25; 32]),
            channels: 1,
        };
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        let acknowledgements = Arc::default();
        let epoch = Arc::new(AtomicU64::new(7));
        ownership.publish_source(0, &sample, 44_100, 3);
        let context = NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: &epoch,
        };
        let permit = context.capture(&sample, 44_100, None).unwrap();
        assert!(permit.current(&sample, 44_100));
        epoch.store(8, Ordering::Release);
        assert!(!permit.current(&sample, 44_100));
        let request = context.capture(&sample, 44_100, None).unwrap();
        ownership.publish_source(0, &sample, 44_100, 4);
        assert!(!request.current(&sample, 44_100));
        ownership.revoke_source(0);
        assert!(context.capture(&sample, 44_100, None).is_none());
    }

    #[test]
    fn fresh_admitted_voice_capture_does_not_reauthorize_an_old_current_candidate() {
        let original = SampleBuffer {
            residency: None,
            samples: Arc::from([0.25; 32]),
            channels: 1,
        };
        let replacement = SampleBuffer {
            samples: Arc::from([0.5; 32]),
            ..original.clone()
        };
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
        let bank_epoch = Arc::new(AtomicU64::new(7));
        let local_epoch = Arc::new(AtomicU64::new(2));
        let lifetime = Arc::new(AtomicU64::new(11));
        let accepted = AcceptedTimingProjection {
            revision: [7; 32],
            period_seconds: 0.50000000000001,
            origin_seconds: -0.0,
            sample_rate_hz: 48_000,
            publication_epoch: 3,
        };
        ownership.publish_source(0, &original, 48_000, 3);
        acknowledgements.acknowledge(0, accepted.publication_epoch);
        let current = NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: &bank_epoch,
        };
        let admission = current
            .capture_source_admission(&original, 48_000, Some(accepted))
            .unwrap();
        let old_candidate = current.capture(&original, 48_000, Some(accepted)).unwrap();

        // Control has already published B before its callback load. A's actual admitted birth,
        // effective timing and local voice lifetime remain distinct from B's new authority.
        ownership.publish_source(0, &replacement, 48_000, 4);
        ownership.authority[0].fetch_add(1, Ordering::AcqRel);
        bank_epoch.fetch_add(1, Ordering::AcqRel);
        acknowledgements.acknowledge(0, 9);
        let retained = NativeVoiceHistoryContext {
            admission: &admission,
            lifetime: &lifetime,
            generation: 11,
            preparation_epoch: &local_epoch,
            accepted: Some(accepted),
        };
        assert!(!old_candidate.current(&original, 48_000));
        assert!(!old_candidate.current_effective_source(&original, 48_000));
        assert!(current.capture(&original, 48_000, Some(accepted)).is_none());
        let fresh = retained.capture(&original, 48_000).unwrap();
        assert!(fresh.current(&original, 48_000));
        assert!(fresh.matches_projection(Some(accepted)));
        assert!(!fresh.matches_projection(Some(AcceptedTimingProjection {
            publication_epoch: 9,
            ..accepted
        })));
        assert!(retained.capture(&replacement, 48_000).is_none());
        assert!(retained.capture(&original, 44_100).is_none());

        // Pause/resume or another preparation fence retires only that captured local work.
        local_epoch.fetch_add(1, Ordering::AcqRel);
        assert!(!fresh.current(&original, 48_000));
        let resumed = retained.capture(&original, 48_000).unwrap();
        assert!(resumed.current(&original, 48_000));
        lifetime.store(0, Ordering::Release);
        assert!(!resumed.current(&original, 48_000));
        assert!(retained.capture(&original, 48_000).is_none());
        lifetime.store(12, Ordering::Release);
        assert!(retained.capture(&original, 48_000).is_none());
        assert!(!old_candidate.current(&original, 48_000));
    }

    #[test]
    fn retained_preparation_is_exact_window_bound_and_effective_history_keeps_its_source() {
        let source = Arc::new(CompleteSourceIdentity {
            frame_count: 64,
            channels: 1,
            sample_rate_hz: 48_000,
            original_sha256: [1; 32],
            playback_sha256: [2; 32],
            mono_sha256: [3; 32],
            transform_sha256: [4; 32],
            source_zero_frame: 0,
        });
        let original = SampleBuffer {
            residency: Some(Arc::new(ResidentSourceView {
                source: source.clone(),
                start_frame: 16,
                window_revision: 1,
                context: ResidentContext::KeyLockFiniteLoop,
            })),
            samples: Arc::from([0.25; 32]),
            channels: 1,
        };
        let next_window = SampleBuffer {
            residency: Some(Arc::new(ResidentSourceView {
                window_revision: 2,
                ..original.residency.as_ref().unwrap().as_ref().clone()
            })),
            samples: Arc::from([0.25; 32]),
            ..original.clone()
        };
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        let acknowledgements = Arc::default();
        let bank_epoch = Arc::new(AtomicU64::new(1));
        ownership.publish_source(0, &original, 48_000, 3);
        let admission = NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: &bank_epoch,
        }
        .capture_source_admission(&original, 48_000, None)
        .unwrap();
        let local_epoch = Arc::new(AtomicU64::new(2));
        let lifetime = Arc::new(AtomicU64::new(11));
        let retained = NativeVoiceHistoryContext {
            admission: &admission,
            lifetime: &lifetime,
            generation: 11,
            preparation_epoch: &local_epoch,
            accepted: None,
        };
        let pending = retained.capture(&original, 48_000).unwrap();
        assert!(!pending.current(&next_window, 48_000));
        assert!(pending.current_effective_source(&next_window, 48_000));
        let fresh = retained.capture(&next_window, 48_000).unwrap();
        assert!(fresh.current(&next_window, 48_000));
        assert!(!fresh.current(&original, 48_000));
        let foreign = SampleBuffer {
            residency: Some(Arc::new(ResidentSourceView {
                source: Arc::new(CompleteSourceIdentity {
                    frame_count: source.frame_count,
                    channels: source.channels,
                    sample_rate_hz: source.sample_rate_hz,
                    original_sha256: [5; 32],
                    playback_sha256: source.playback_sha256,
                    mono_sha256: source.mono_sha256,
                    transform_sha256: source.transform_sha256,
                    source_zero_frame: source.source_zero_frame,
                }),
                ..next_window.residency.as_ref().unwrap().as_ref().clone()
            })),
            ..next_window.clone()
        };
        assert!(!pending.current_effective_source(&foreign, 48_000));
        assert!(retained.capture(&foreign, 48_000).is_none());
    }

    #[test]
    fn full_accepted_projection_and_current_authority_are_required() {
        let sample = SampleBuffer {
            residency: None,
            samples: Arc::from([0.25; 32]),
            channels: 1,
        };
        let ownership = Arc::default();
        let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
        let epoch = Arc::new(AtomicU64::new(1));
        let accepted = AcceptedTimingProjection {
            revision: [7; 32],
            period_seconds: 0.50000000000001,
            origin_seconds: -0.0,
            sample_rate_hz: 48_000,
            publication_epoch: 3,
        };
        let context = NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: &epoch,
        };
        assert!(context.capture(&sample, 48_000, Some(accepted)).is_none());
        acknowledgements.acknowledge(0, 3);
        let permit = context.capture(&sample, 48_000, Some(accepted)).unwrap();
        assert!(permit.matches_projection(Some(accepted)));
        assert!(!permit.matches_projection(Some(AcceptedTimingProjection {
            origin_seconds: 0.0,
            ..accepted
        })));
        assert!(!permit.matches_projection(Some(AcceptedTimingProjection {
            revision: [8; 32],
            ..accepted
        })));
        assert!(!permit.current(&sample, 44_100));
        ownership.authority[0].fetch_add(1, Ordering::AcqRel);
        assert!(!permit.current(&sample, 48_000));
        acknowledgements.clear(0);
        ownership.set_timing_intent(0, TimingIntent::Automatic);
        assert!(context.capture(&sample, 48_000, None).is_none());
        for intent in [
            TimingIntent::Manual,
            TimingIntent::Tap,
            TimingIntent::Legacy,
        ] {
            ownership.set_timing_intent(0, intent);
            let permit = context.capture(&sample, 48_000, None).unwrap();
            assert!(permit.matches_projection(None));
            assert!(permit.current(&sample, 48_000));
        }
    }
}
