//! Current native source/request/authority permit for worker-owned source history.
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

#[derive(Clone)]
pub(crate) struct NativeHistoryPermit {
    id: usize,
    binding: InputPadBinding,
    source_generation: u64,
    preparation_epoch: Arc<AtomicU64>,
    expected_epoch: u64,
    ownership: Arc<InputRuntimeOwnership>,
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
}

impl NativeHistoryContext<'_> {
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
            ownership: self.ownership.clone(),
            acknowledgements: self.acknowledgements.clone(),
        };
        permit.current(sample, rate).then_some(permit)
    }
}

impl NativeHistoryPermit {
    /// Already adopted history owns chronological native/FIFO/filter state. A
    /// storage-only window ACK may fence pending preparation without invalidating
    /// that same source's established trajectory or timing owner.
    pub(crate) fn current_effective_source(&self, sample: &SampleBuffer, rate: u32) -> bool {
        let current = || {
            self.preparation_epoch.load(Ordering::Acquire) == self.expected_epoch
                && self.ownership.authority_current(self.id, self.binding)
                && self.ownership.source_generation(self.id, sample, rate)
                    == Some(self.source_generation)
                && sample.source_address() == self.binding.source_address
                && sample.source_sample_count() == self.binding.sample_count
                && sample.channels == self.binding.channels
                && rate == self.binding.sample_rate_hz
                && self.acknowledgements.current_epoch(self.id)
                    == self
                        .binding
                        .accepted
                        .map_or(0, |value| value.publication_epoch)
                && self.ownership.source_timing_available(
                    self.id,
                    self.binding.accepted,
                    &self.acknowledgements,
                )
        };
        current() && current()
    }
    /// Preparation/request and current source/authority are rechecked at ACTUAL native adoption.
    /// The double check is bounded and has no spin, owner allocation or address dereference.
    pub(crate) fn current(&self, sample: &SampleBuffer, rate: u32) -> bool {
        let current = || {
            self.preparation_epoch.load(Ordering::Acquire) == self.expected_epoch
                && self.ownership.current(self.id, self.binding)
                && self.ownership.source_generation(self.id, sample, rate)
                    == Some(self.source_generation)
                && sample.source_address() == self.binding.source_address
                && sample.source_sample_count() == self.binding.sample_count
                && sample.channels == self.binding.channels
                && rate == self.binding.sample_rate_hz
                && self.acknowledgements.current_epoch(self.id)
                    == self
                        .binding
                        .accepted
                        .map_or(0, |value| value.publication_epoch)
                && self.ownership.source_timing_available(
                    self.id,
                    self.binding.accepted,
                    &self.acknowledgements,
                )
        };
        current() && current()
    }

    pub(crate) fn matches_projection(&self, accepted: Option<AcceptedTimingProjection>) -> bool {
        self.binding.accepted == accepted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
