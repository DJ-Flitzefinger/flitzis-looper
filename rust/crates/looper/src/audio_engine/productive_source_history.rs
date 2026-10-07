//! Fixed ownership of real source feed retained by the live native/adapter history.
//!
//! A neutral warmed reserve has no source history. The owning voice pins the immutable PCM;
//! addresses here are compared only and must be cleared before that voice releases its pin.

use super::constant_timing::AcceptedTimingProjection;
use super::source_playback::FractionalSourcePosition;
use super::source_reader::SourceLoopDomain;
use crate::messages::SampleBuffer;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ProductiveSourceBinding {
    pub(crate) source_address: usize,
    pub(crate) sample_count: usize,
    pub(crate) channels: usize,
    pub(crate) sample_rate_hz: u32,
    pub(crate) accepted: Option<AcceptedTimingProjection>,
}

impl ProductiveSourceBinding {
    pub(crate) fn new(
        sample: &SampleBuffer,
        sample_rate_hz: u32,
        accepted: Option<AcceptedTimingProjection>,
    ) -> Self {
        Self {
            source_address: sample.samples.as_ptr() as usize,
            sample_count: sample.samples.len(),
            channels: sample.channels,
            sample_rate_hz,
            accepted,
        }
    }

    fn same_source(self, other: Self) -> bool {
        self.source_address == other.source_address
            && self.sample_count == other.sample_count
            && self.channels == other.channels
            && self.sample_rate_hz == other.sample_rate_hz
    }
}

/// This value belongs to the actual used native handle and its pending adapter FIFOs.
/// A full timing refresh is coherent only when the next real feed continues the same source.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProductiveSourceHistory {
    pub(crate) binding: ProductiveSourceBinding,
    pub(crate) next_position: FractionalSourcePosition,
    pub(crate) domain: Option<SourceLoopDomain>,
    pub(crate) fed_output_frames: u64,
}

impl ProductiveSourceHistory {
    pub(crate) fn continues(
        self,
        binding: ProductiveSourceBinding,
        position: FractionalSourcePosition,
        domain: Option<SourceLoopDomain>,
    ) -> bool {
        if !self.binding.same_source(binding) {
            return false;
        }
        if same_position(self.next_position, position) {
            return true;
        }
        // A period-only accepted refresh changes the canonical modulo coordinate, not the
        // chronological native/filter owner. Allow only the exact new-domain representation
        // of the previously expected next phase, with the same physical geometry. Real seeks
        // clear history before feed, and source/marker changes cannot use this equivalence.
        self.binding.accepted != binding.accepted
            && self.domain.zip(domain).is_some_and(|(prior, current)| {
                prior.sample_frames == current.sample_frames
                    && prior.region == current.region
                    && prior != current
                    && current
                        .project_normal_position(self.next_position)
                        .is_some_and(|projected| same_position(projected, position))
            })
    }
}

fn same_position(left: FractionalSourcePosition, right: FractionalSourcePosition) -> bool {
    left.frame == right.frame
        && left.fraction.to_bits() == right.fraction.to_bits()
        && left.seek_mode == right.seek_mode
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::source_reader::{ExplicitSeekMode, FrameRange};
    use std::sync::Arc;

    #[test]
    fn period_projection_alias_requires_changed_accepted_owner_and_exact_physical_geometry() {
        let sample = SampleBuffer {
            channels: 1,
            samples: Arc::from(vec![0.0; 2000]),
        };
        let region = FrameRange {
            start: 19,
            end: 1519,
        };
        let prior_domain = SourceLoopDomain::musical(2000, region, 1500.25).unwrap();
        let next_domain = SourceLoopDomain::musical(2000, region, 1499.75).unwrap();
        let accepted = AcceptedTimingProjection {
            revision: [1; 32],
            period_seconds: 0.500_083_333_333_333_4,
            origin_seconds: -0.0,
            sample_rate_hz: 48_000,
            publication_epoch: 2,
        };
        let binding = ProductiveSourceBinding::new(&sample, 48_000, Some(accepted));
        let next_binding = ProductiveSourceBinding::new(
            &sample,
            48_000,
            Some(AcceptedTimingProjection {
                revision: [2; 32],
                period_seconds: 0.499_916_666_666_666_7,
                publication_epoch: 3,
                ..accepted
            }),
        );
        let position = FractionalSourcePosition {
            frame: 1519,
            fraction: 0.125,
            seek_mode: ExplicitSeekMode::Normal,
        };
        let projected = FractionalSourcePosition {
            frame: 19,
            fraction: 0.375,
            ..position
        };
        let history = ProductiveSourceHistory {
            binding,
            next_position: position,
            domain: Some(prior_domain),
            fed_output_frames: 777,
        };
        assert!(history.continues(next_binding, projected, Some(next_domain)));
        assert!(!history.continues(binding, projected, Some(next_domain)));
        assert!(!history.continues(next_binding, projected, None));
        assert!(!history.continues(
            next_binding,
            projected,
            Some(SourceLoopDomain {
                region: FrameRange {
                    start: 18,
                    end: 1518
                },
                ..next_domain
            })
        ));
        assert!(!history.continues(
            next_binding,
            FractionalSourcePosition {
                fraction: f64::from_bits(projected.fraction.to_bits() + 1),
                ..projected
            },
            Some(next_domain)
        ));
        assert!(!history.continues(
            next_binding,
            FractionalSourcePosition {
                seek_mode: ExplicitSeekMode::AfterLoop,
                ..projected
            },
            Some(next_domain)
        ));
        let replacement = SampleBuffer {
            channels: 1,
            samples: Arc::from(vec![0.0; 2000]),
        };
        assert!(!history.continues(
            ProductiveSourceBinding::new(&replacement, 48_000, next_binding.accepted),
            projected,
            Some(next_domain)
        ));
        // An in-range physical edit can still keep the exactly expected real next read.
        assert!(history.continues(binding, position, None));
    }
}
