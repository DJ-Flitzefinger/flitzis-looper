//! Fixed ownership of real source feed retained by the live native/adapter history.
//!
//! A neutral warmed reserve has no source history. The owning voice pins the immutable PCM;
//! addresses here are compared only and must be cleared before that voice releases its pin.

use super::constant_timing::AcceptedTimingProjection;
use super::source_playback::FractionalSourcePosition;
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
    pub(crate) fed_output_frames: u64,
}

impl ProductiveSourceHistory {
    pub(crate) fn continues(
        self,
        binding: ProductiveSourceBinding,
        position: FractionalSourcePosition,
    ) -> bool {
        self.binding.same_source(binding)
            && self.next_position.frame == position.frame
            && self.next_position.fraction.to_bits() == position.fraction.to_bits()
            && self.next_position.seek_mode == position.seek_mode
    }
}
