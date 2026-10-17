//! Current-bank or admitted retained-voice authority for worker-owned source history.
//!
//! This retains only shared atomic owners and a fixed complete projection. Actual PCM/stem pins
//! belong to the worker request/bundle and retire there, never through a dropped callback ticket.

use super::constant_timing::{AcceptedTimingProjection, CurrentTimingAcknowledgements};
use super::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
use super::source_feed_history::SourceFeedGuard;
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
    ActionCurrent {
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
    source_feed_guard: Option<SourceFeedGuard>,
}

impl NativeHistoryContext<'_> {
    /// Retain the caller's actual captured key, source and timing while its outer marker is owned.
    /// Runtime rows are MIDI intent; source-only callers take their current scalar for fresh history.
    pub(crate) fn capture_admitted(
        &self,
        sample: &SampleBuffer,
        rate: u32,
        binding: InputPadBinding,
    ) -> Option<NativeHistoryPermit> {
        let cell = self.ownership.binding_cell(self.id, binding)?;
        if binding.placement.is_some_and(|placement| {
            placement
                .content_key()
                .is_none_or(|key| self.ownership.callback_key_slot(key) != Some(self.id))
        }) {
            return None;
        }
        let source_generation = self.ownership.binding_source_generation(self.id, binding)?;
        let permit = NativeHistoryPermit {
            id: self.id,
            binding: InputPadBinding {
                runtime_revision: self
                    .ownership
                    .runtime_for_cell(cell)
                    .load(Ordering::Acquire),
                ..binding
            },
            source_generation,
            preparation_epoch: self.preparation_epoch.clone(),
            expected_epoch: self.preparation_epoch.load(Ordering::Acquire),
            authority: NativeHistoryAuthority::Current {
                ownership: self.ownership.clone(),
                acknowledgements: self.acknowledgements.clone(),
            },
            source_feed_guard: None,
        };
        permit.current(sample, rate).then_some(permit)
    }

    pub(crate) fn capture_admitted_source(
        &self,
        sample: &SampleBuffer,
        rate: u32,
        binding: InputPadBinding,
        strict_runtime: bool,
    ) -> Option<NativeSourceAdmission> {
        self.ownership.binding_cell(self.id, binding)?;
        if binding.placement.is_some_and(|placement| {
            placement
                .content_key()
                .is_none_or(|key| self.ownership.callback_key_slot(key) != Some(self.id))
        }) {
            return None;
        }
        let admission = NativeSourceAdmission {
            id: self.id,
            binding,
            source_generation: self.ownership.binding_source_generation(self.id, binding)?,
        };
        admission
            .current_for_birth(
                self.ownership,
                self.acknowledgements,
                sample,
                rate,
                strict_runtime,
            )
            .then_some(admission)
    }

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
        let placement = self.ownership.slot_snapshot(self.id);
        if self.ownership.is_source_tracked()
            && placement
                .and_then(|snapshot| snapshot.content_key())
                .is_none()
        {
            return None;
        }
        let permit = NativeHistoryPermit {
            id: self.id,
            binding: InputPadBinding {
                placement: self
                    .ownership
                    .is_source_tracked()
                    .then_some(placement)
                    .flatten(),
                resident: sample.resident_binding(),
                source_address: sample.source_address(),
                sample_count: sample.source_sample_count(),
                channels: sample.channels,
                sample_rate_hz: rate,
                authority_revision: self.ownership.authority_revision(self.id),
                runtime_revision: self.ownership.runtime_revision(self.id),
                accepted,
            },
            source_generation,
            preparation_epoch: self.preparation_epoch.clone(),
            expected_epoch: self.preparation_epoch.load(Ordering::Acquire),
            authority: NativeHistoryAuthority::Current {
                ownership: self.ownership.clone(),
                acknowledgements: self.acknowledgements.clone(),
            },
            source_feed_guard: None,
        };
        permit.current(sample, rate).then_some(permit)
    }
}

impl NativeSourceAdmission {
    pub(crate) fn content_key(&self) -> Option<super::content_placement::ContentKey> {
        self.binding
            .placement
            .and_then(|placement| placement.content_key())
    }
    pub(crate) fn current_for_birth(
        &self,
        ownership: &InputRuntimeOwnership,
        acknowledgements: &CurrentTimingAcknowledgements,
        sample: &SampleBuffer,
        rate: u32,
        strict_runtime: bool,
    ) -> bool {
        sample.source_address() == self.binding.source_address
            && sample.source_sample_count() == self.binding.sample_count
            && sample.channels == self.binding.channels
            && sample.resident_binding() == self.binding.resident
            && rate == self.binding.sample_rate_hz
            && ownership.authority_current(self.id, self.binding)
            && ownership.binding_source_generation(self.id, self.binding)
                == Some(self.source_generation)
            && ownership.binding_window_current(self.id, self.binding)
            && (!strict_runtime || ownership.current(self.id, self.binding))
            && acknowledgements.current_for_binding(ownership, self.id, self.binding)
                == self
                    .binding
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
            && ownership.source_timing_available_for_binding(
                self.id,
                self.binding,
                acknowledgements,
            )
    }

    #[cfg(test)]
    pub(crate) fn content_key_for_test(&self) -> Option<super::content_placement::ContentKey> {
        self.binding
            .placement
            .and_then(|placement| placement.content_key())
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
            source_feed_guard: None,
        };
        permit.current(sample, rate).then_some(permit)
    }
}

impl NativeHistoryPermit {
    /// Narrow a freshly captured strong permit once with its genuine source-feed/voice fence.
    /// An existing guard is never replaced or refreshed. A repeated call must be worker/control
    /// work because its incoming duplicate drops here. The attached guard follows this permit's
    /// existing off-realtime source-owner retirement on every outcome.
    pub(crate) fn with_source_feed_guard(mut self, guard: SourceFeedGuard) -> Self {
        if self.source_feed_guard.is_none() {
            self.source_feed_guard = Some(guard);
        }
        self
    }

    /// Compare real source authority without transferring either permission/epoch.
    /// The caller must retain and validate a fresh original action permit separately.
    pub(crate) fn same_endpoint_authority(&self, other: &Self) -> bool {
        let owners = |authority: &NativeHistoryAuthority| match authority {
            NativeHistoryAuthority::Current {
                ownership,
                acknowledgements,
            }
            | NativeHistoryAuthority::ActionCurrent {
                ownership,
                acknowledgements,
            } => Some((Arc::as_ptr(ownership), Arc::as_ptr(acknowledgements))),
            NativeHistoryAuthority::RetainedVoice { .. } => None,
        };
        owners(&self.authority).is_some()
            && owners(&self.authority) == owners(&other.authority)
            && self.id == other.id
            && self.source_generation == other.source_generation
            && self
                .binding
                .placement
                .and_then(|placement| placement.content_key())
                == other
                    .binding
                    .placement
                    .and_then(|placement| placement.content_key())
            && self.binding.authority_revision == other.binding.authority_revision
            && self.binding.resident == other.binding.resident
            && self.binding.accepted == other.binding.accepted
            && self.binding.source_address == other.binding.source_address
            && self.binding.sample_count == other.binding.sample_count
            && self.binding.channels == other.binding.channels
            && self.binding.sample_rate_hz == other.binding.sample_rate_hz
    }

    /// A separately admitted action retains its own epoch and actual Native owners.
    /// Equal PCM or scalar counters cannot relabel another content's permission.
    pub(crate) fn matches_action_owner(
        &self,
        ownership: &Arc<InputRuntimeOwnership>,
        acknowledgements: &Arc<CurrentTimingAcknowledgements>,
        key: super::content_placement::ContentKey,
        preparation_epoch: &Arc<AtomicU64>,
    ) -> bool {
        let own_authority = matches!(
            &self.authority,
            NativeHistoryAuthority::Current {
                ownership: actual_ownership,
                acknowledgements: actual_acknowledgements,
            } | NativeHistoryAuthority::ActionCurrent {
                ownership: actual_ownership,
                acknowledgements: actual_acknowledgements,
            } if Arc::ptr_eq(actual_ownership, ownership)
                && Arc::ptr_eq(actual_acknowledgements, acknowledgements)
        );
        own_authority
            && self
                .binding
                .placement
                .and_then(|placement| placement.content_key())
                == Some(key)
            && Arc::ptr_eq(&self.preparation_epoch, preparation_epoch)
            && self.expected_epoch != 0
            && self.expected_epoch == preparation_epoch.load(Ordering::Acquire)
    }
    /// Only a completely matched action owner can opt out of the mutable MIDI row's
    /// revision. Source, lifetime, window, ACK and its own epoch remain authoritative.
    pub(crate) fn for_action(
        mut self,
        ownership: &Arc<InputRuntimeOwnership>,
        acknowledgements: &Arc<CurrentTimingAcknowledgements>,
        key: super::content_placement::ContentKey,
        epoch: &Arc<AtomicU64>,
    ) -> Option<Self> {
        if !self.matches_action_owner(ownership, acknowledgements, key, epoch) {
            return None;
        }
        self.authority = NativeHistoryAuthority::ActionCurrent {
            ownership: ownership.clone(),
            acknowledgements: acknowledgements.clone(),
        };
        Some(self)
    }
    /// Already adopted history owns chronological native/FIFO/filter state. A
    /// storage-only window ACK may fence pending preparation without invalidating
    /// that same source's established trajectory or timing owner.
    pub(crate) fn current_effective_source(&self, sample: &SampleBuffer, rate: u32) -> bool {
        let current = || self.current_inner(sample, rate, false, false);
        current() && current()
    }
    /// Preparation/request and current source/authority are rechecked at ACTUAL native adoption.
    /// The double check is bounded and has no spin, owner allocation or address dereference.
    pub(crate) fn current(&self, sample: &SampleBuffer, rate: u32) -> bool {
        let current = || self.current_inner(sample, rate, true, true);
        let source_feed_current = || {
            self.source_feed_guard
                .as_ref()
                .is_none_or(SourceFeedGuard::is_current)
        };
        source_feed_current() && current() && current() && source_feed_current()
    }

    /// Physical endpoint data observation only, never action/adoption permission.
    /// A same-window resident control may advance RuntimeProjection without changing
    /// any prepared sample. Consumers still require their own fresh original guard/permit.
    pub(crate) fn endpoint_source_current(&self, sample: &SampleBuffer, rate: u32) -> bool {
        let current = || self.current_inner(sample, rate, true, false);
        current() && current()
    }

    fn current_inner(
        &self,
        sample: &SampleBuffer,
        rate: u32,
        exact_window: bool,
        strict_runtime: bool,
    ) -> bool {
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
            }
            | NativeHistoryAuthority::ActionCurrent {
                ownership,
                acknowledgements,
            } => {
                (if exact_window {
                    if !strict_runtime
                        || matches!(
                            &self.authority,
                            NativeHistoryAuthority::ActionCurrent { .. }
                        )
                    {
                        ownership.authority_current(self.id, self.binding)
                            && ownership.binding_window_current(self.id, self.binding)
                    } else {
                        ownership.current(self.id, self.binding)
                    }
                } else {
                    ownership.authority_current(self.id, self.binding)
                }) && ownership.binding_source_generation(self.id, self.binding)
                    == Some(self.source_generation)
                    && acknowledgements.current_for_binding(ownership, self.id, self.binding)
                        == self
                            .binding
                            .accepted
                            .map_or(0, |value| value.publication_epoch)
                    && ownership.source_timing_available_for_binding(
                        self.id,
                        self.binding,
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
    use crate::audio_engine::source_feed_history::{SourceFeedHistory, SourceFeedSnapshot};
    use crate::messages::{CompleteSourceIdentity, ResidentContext, ResidentSourceView};
    use flitzis_looper_analysis::tempo_acceptance::TimingIntent;

    fn capture_source_feed_guard(
        lifetime: &Arc<AtomicU64>,
        generation: u64,
    ) -> (SourceFeedHistory, SourceFeedSnapshot, SourceFeedGuard) {
        let history = SourceFeedHistory::new();
        let mut snapshot = SourceFeedSnapshot::new();
        assert!(history.capture_into(&mut snapshot));
        // Empty history is deliberately enough for a fence, never for replay coverage or
        // Native authority. Those independent obligations cannot come from this helper.
        let guard = snapshot.guard(lifetime, generation).unwrap();
        (history, snapshot, guard)
    }

    #[test]
    fn recycled_source_snapshot_keeps_adoption_fences_without_revoking_productive_history() {
        let sample = SampleBuffer {
            residency: None,
            samples: Arc::from([0.25; 32]),
            channels: 1,
        };
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        let acknowledgements = Arc::default();
        let epoch = Arc::new(AtomicU64::new(7));
        ownership.publish_test_source(0, &sample, 48_000, 3);
        let context = NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: &epoch,
        };
        let lifetime = Arc::new(AtomicU64::new(11));
        let (history, mut snapshot, guard) = capture_source_feed_guard(&lifetime, 11);
        let permit = context
            .capture(&sample, 48_000, None)
            .unwrap()
            .with_source_feed_guard(guard);
        assert!(permit.current(&sample, 48_000));
        snapshot.clear_for_reuse();
        assert!(history.capture_into(&mut snapshot));
        assert!(permit.current(&sample, 48_000));

        history.invalidate_captures();
        assert!(!permit.current(&sample, 48_000));
        assert!(permit.current_effective_source(&sample, 48_000));
        assert!(permit.endpoint_source_current(&sample, 48_000));
        snapshot.clear_for_reuse();
        assert!(history.capture_into(&mut snapshot));
        let fresh_guard = snapshot.guard(&lifetime, 11).unwrap();
        let still_original = permit.clone().with_source_feed_guard(fresh_guard.clone());
        assert!(fresh_guard.is_current());
        assert!(
            !still_original.current(&sample, 48_000),
            "an attached original guard cannot be replaced by a later SOURCE epoch"
        );

        let next_permit = context
            .capture(&sample, 48_000, None)
            .unwrap()
            .with_source_feed_guard(fresh_guard);
        snapshot.clear_for_reuse();
        assert!(next_permit.current(&sample, 48_000));
        lifetime.store(0, Ordering::Release);
        assert!(!next_permit.current(&sample, 48_000));
        lifetime.store(12, Ordering::Release);
        assert!(!next_permit.current(&sample, 48_000));

        let publication = super::super::prepared_source::PreparedSourcePermit::unrestricted();
        assert!(history.capture_into(&mut snapshot));
        let publication_guard = snapshot
            .guard(&lifetime, 12)
            .unwrap()
            .with_stem_publication(&publication)
            .unwrap();
        assert!(
            publication_guard
                .clone()
                .with_stem_publication(&publication)
                .is_none(),
            "an exact publication fence cannot be refreshed or replaced"
        );
        let selected_permit = context
            .capture(&sample, 48_000, None)
            .unwrap()
            .with_source_feed_guard(publication_guard);
        assert!(selected_permit.current(&sample, 48_000));
        publication.epoch.fetch_add(1, Ordering::AcqRel);
        assert!(!selected_permit.current(&sample, 48_000));
        assert!(selected_permit.current_effective_source(&sample, 48_000));
        assert!(selected_permit.endpoint_source_current(&sample, 48_000));
    }

    #[test]
    fn current_source_feed_guard_cannot_authorize_foreign_or_expired_native_context() {
        let sample = SampleBuffer {
            residency: None,
            samples: Arc::from([0.25; 32]),
            channels: 1,
        };
        let foreign_sample = SampleBuffer {
            samples: Arc::from([0.5; 32]),
            ..sample.clone()
        };
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
        let epoch = Arc::new(AtomicU64::new(7));
        ownership.publish_test_source(0, &sample, 48_000, 3);
        let accepted = AcceptedTimingProjection {
            revision: [7; 32],
            period_seconds: 0.5,
            origin_seconds: 0.0,
            sample_rate_hz: 48_000,
            publication_epoch: 3,
        };
        let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
        assert!(acknowledgements.acknowledge_for_key(&ownership, key, 3));
        let context = NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: &epoch,
        };
        let lifetime = Arc::new(AtomicU64::new(11));
        let (_history, mut snapshot, guard) = capture_source_feed_guard(&lifetime, 11);
        let permit = context
            .capture(&sample, 48_000, Some(accepted))
            .unwrap()
            .with_source_feed_guard(guard.clone());
        snapshot.clear_for_reuse();
        assert!(permit.current(&sample, 48_000));
        assert!(!permit.current(&foreign_sample, 48_000));
        assert!(!permit.current(&sample, 44_100));

        let foreign_ownership = Arc::new(InputRuntimeOwnership::tracked());
        foreign_ownership.publish_test_source(0, &sample, 48_000, 3);
        let foreign_permit = NativeHistoryPermit {
            binding: InputPadBinding {
                placement: foreign_ownership.binding_placement(0),
                ..permit.binding
            },
            ..permit.clone()
        };
        assert!(!foreign_permit.current(&sample, 48_000));
        assert!(acknowledgements.clear_for_key(&ownership, key));
        assert!(!permit.current(&sample, 48_000));
        assert!(guard.is_current());
        assert!(acknowledgements.acknowledge_for_key(&ownership, key, 3));
        assert!(permit.current(&sample, 48_000));
        epoch.fetch_add(1, Ordering::AcqRel);
        assert!(!permit.current(&sample, 48_000));
        assert!(guard.is_current());
    }

    #[test]
    fn own_action_history_ignores_only_mutable_runtime_rows_and_keeps_strong_source_fences() {
        let sample = SampleBuffer {
            samples: Arc::from([0.25; 32]),
            channels: 1,
            residency: None,
        };
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        ownership.publish_test_source(0, &sample, 48_000, 3);
        let acknowledgements = Arc::default();
        let epoch = Arc::new(AtomicU64::new(1));
        let context = NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: &epoch,
        };
        let background = context.capture(&sample, 48_000, None).unwrap();
        let key = background.binding.placement.unwrap().content_key().unwrap();
        let action = background
            .clone()
            .for_action(&ownership, &acknowledgements, key, &epoch)
            .unwrap();
        ownership
            .runtime_for_cell(key.cell())
            .fetch_add(1, Ordering::AcqRel);
        assert!(!background.current(&sample, 48_000));
        assert!(action.current(&sample, 48_000));
        let foreign_epoch = Arc::new(AtomicU64::new(1));
        assert!(
            action
                .clone()
                .for_action(&ownership, &acknowledgements, key, &foreign_epoch)
                .is_none()
        );
        ownership
            .authority_for_cell(key.cell())
            .fetch_add(1, Ordering::AcqRel);
        assert!(!action.current(&sample, 48_000));
        assert!(!action.current_effective_source(&sample, 48_000));
    }

    #[test]
    fn admitted_history_follows_its_content_cell_while_old_slot_admission_stays_stale_after_aba() {
        let sample = SampleBuffer {
            residency: None,
            samples: Arc::from([0.25; 32]),
            channels: 1,
        };
        let other = SampleBuffer {
            samples: Arc::from([0.5; 32]),
            ..sample.clone()
        };
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        ownership.publish_test_source(0, &sample, 48_000, 3);
        ownership.publish_test_source(36, &other, 48_000, 7);
        let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
        let epoch = Arc::new(AtomicU64::new(2));
        let projection = AcceptedTimingProjection {
            revision: [7; 32],
            period_seconds: 0.5,
            origin_seconds: 0.0,
            sample_rate_hz: 48_000,
            publication_epoch: 11,
        };
        let original = ownership.slot_snapshot(0).unwrap();
        let other_slot = ownership.slot_snapshot(36).unwrap();
        let unrelated = ownership.slot_snapshot(215).unwrap();
        assert!(acknowledgements.acknowledge_for_key(
            &ownership,
            original.content_key().unwrap(),
            11
        ));
        let context = NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: &epoch,
        };
        let permit = context.capture(&sample, 48_000, Some(projection)).unwrap();
        assert!(ownership.admission_current(0, permit.binding));
        let prepared = ownership
            .prepare_placement_pair(original, other_slot)
            .unwrap();
        // Preparing and even the callback's short publication marker leave established effects
        // on their stable cell. New slot admissions must wait for the complete publication.
        assert!(permit.current(&sample, 48_000));
        ownership
            .commit_placement_pair(
                &prepared,
                true,
                || {
                    assert!(permit.current(&sample, 48_000));
                    assert!(!ownership.admission_current(0, permit.binding));
                    true
                },
                || {
                    assert!(permit.current_effective_source(&sample, 48_000));
                },
            )
            .unwrap();
        assert!(permit.current(&sample, 48_000));
        assert!(!permit.current(&other, 48_000));
        assert!(!ownership.admission_current(0, permit.binding));
        assert!(ownership.slot_snapshot_current(unrelated));
        assert_eq!(
            ownership.slot_snapshot(36).unwrap().content_key(),
            original.content_key()
        );
        assert!(context.capture(&sample, 48_000, Some(projection)).is_none());

        let returned = ownership
            .prepare_placement_pair(
                ownership.slot_snapshot(36).unwrap(),
                ownership.slot_snapshot(0).unwrap(),
            )
            .unwrap();
        ownership
            .commit_placement_pair(&returned, true, || true, || {})
            .unwrap();
        assert!(permit.current(&sample, 48_000));
        assert!(!ownership.admission_current(0, permit.binding));
        let fresh = context.capture(&sample, 48_000, Some(projection)).unwrap();
        assert!(ownership.admission_current(0, fresh.binding));
        assert_ne!(
            fresh.binding.placement.unwrap().binding_epoch(),
            original.binding_epoch()
        );
        // A real lifetime retirement revokes both historical candidates; placement alone did not.
        ownership.publish_content_lifetime(0, super::super::content_lifetime::reserve().unwrap());
        assert!(!permit.current_effective_source(&sample, 48_000));
        assert!(!fresh.current(&sample, 48_000));
    }

    #[test]
    fn tracked_history_cannot_gain_content_authority_from_zero_lifetime_or_foreign_owner() {
        let sample = SampleBuffer {
            residency: None,
            samples: Arc::from([0.25; 32]),
            channels: 1,
        };
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        ownership.publish_source(0, &sample, 48_000, 3);
        let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
        let epoch = Arc::new(AtomicU64::new(2));
        let context = NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: &epoch,
        };
        assert!(context.capture(&sample, 48_000, None).is_none());
        ownership.publish_test_source(0, &sample, 48_000, 3);
        let permit = context.capture(&sample, 48_000, None).unwrap();
        let foreign = Arc::new(InputRuntimeOwnership::tracked());
        foreign.publish_test_source(0, &sample, 48_000, 3);
        let foreign_binding = InputPadBinding {
            placement: foreign.binding_placement(0),
            ..permit.binding
        };
        assert!(!ownership.authority_current(0, foreign_binding));
        assert!(!ownership.binding_source_current(0, foreign_binding));
        assert!(permit.current(&sample, 48_000));
    }

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
        ownership.publish_test_source(0, &sample, 44_100, 3);
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
        ownership.publish_test_source(0, &sample, 44_100, 4);
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
        ownership.publish_test_source(0, &original, 48_000, 3);
        acknowledgements.acknowledge_for_key(
            &ownership,
            ownership.slot_snapshot(0).unwrap().content_key().unwrap(),
            accepted.publication_epoch,
        );
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
        ownership.publish_test_source(0, &replacement, 48_000, 4);
        ownership
            .authority_for_cell(ownership.slot_snapshot(0).unwrap().cell())
            .fetch_add(1, Ordering::AcqRel);
        bank_epoch.fetch_add(1, Ordering::AcqRel);
        acknowledgements.acknowledge_for_key(
            &ownership,
            ownership.slot_snapshot(0).unwrap().content_key().unwrap(),
            9,
        );
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
        ownership.publish_test_source(0, &original, 48_000, 3);
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
        let (_history, mut snapshot, guard) = capture_source_feed_guard(&lifetime, 11);
        let pending = retained
            .capture(&original, 48_000)
            .unwrap()
            .with_source_feed_guard(guard.clone());
        snapshot.clear_for_reuse();
        assert!(guard.is_current());
        assert!(pending.current(&original, 48_000));
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
        ownership
            .authority_for_cell(ownership.slot_snapshot(0).unwrap().cell())
            .fetch_add(1, Ordering::AcqRel);
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
