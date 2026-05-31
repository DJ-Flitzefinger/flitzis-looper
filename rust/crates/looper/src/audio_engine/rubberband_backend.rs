#![cfg_attr(not(test), allow(dead_code))]

use std::os::raw::{c_double, c_float, c_int, c_uint};
use std::ptr::NonNull;

pub(crate) const RUBBERBAND_API_MAJOR_VERSION: u32 = 3;
pub(crate) const RUBBERBAND_API_MINOR_VERSION: u32 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RubberBandApiVersion {
    pub major: u32,
    pub minor: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RubberBandLiveOptions(c_int);

impl RubberBandLiveOptions {
    pub const WINDOW_SHORT: Self = Self(0x00000000);
    pub const WINDOW_MEDIUM: Self = Self(0x00100000);
    pub const FORMANT_SHIFTED: Self = Self(0x00000000);
    pub const FORMANT_PRESERVED: Self = Self(0x01000000);
    pub const CHANNELS_APART: Self = Self(0x00000000);
    pub const CHANNELS_TOGETHER: Self = Self(0x10000000);

    pub const fn default_realtime() -> Self {
        Self(Self::WINDOW_SHORT.0 | Self::CHANNELS_TOGETHER.0)
    }

    pub const fn bits(self) -> c_int {
        self.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl Default for RubberBandLiveOptions {
    fn default() -> Self {
        Self::default_realtime()
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RubberBandError {
    #[error("sample_rate_hz must be greater than zero")]
    InvalidSampleRate,
    #[error("channels must be greater than zero and fit into the Rubber Band C API")]
    InvalidChannelCount,
    #[error("Rubber Band LiveShifter construction failed")]
    ConstructionFailed,
    #[error("Rubber Band reported {reported} channels for a {requested}-channel shifter")]
    ChannelCountMismatch { requested: usize, reported: usize },
    #[error("Rubber Band reported an invalid fixed block size")]
    InvalidBlockSize,
    #[error("pitch scale must be finite and greater than zero")]
    InvalidPitchScale,
    #[error("expected {expected} channels but got {actual}")]
    BufferChannelCount { expected: usize, actual: usize },
    #[error("channel {channel} has {actual} frames but Rubber Band requires at least {required}")]
    BufferTooSmall {
        channel: usize,
        required: usize,
        actual: usize,
    },
}

pub(crate) struct RubberBandLiveShifter {
    handle: NonNull<RubberBandLiveStateOpaque>,
    channels: usize,
    block_size: usize,
    start_delay: usize,
    input_ptrs: Vec<*const c_float>,
    output_ptrs: Vec<*mut c_float>,
}

// Rubber Band documents that separate instances may be used on separate threads. This wrapper
// requires `&mut self` for processing, so one handle cannot be shifted concurrently through it.
unsafe impl Send for RubberBandLiveShifter {}

impl RubberBandLiveShifter {
    pub(crate) fn new(sample_rate_hz: u32, channels: usize) -> Result<Self, RubberBandError> {
        Self::with_options(sample_rate_hz, channels, RubberBandLiveOptions::default())
    }

    pub(crate) fn with_options(
        sample_rate_hz: u32,
        channels: usize,
        options: RubberBandLiveOptions,
    ) -> Result<Self, RubberBandError> {
        if sample_rate_hz == 0 {
            return Err(RubberBandError::InvalidSampleRate);
        }
        let channels_c = c_uint::try_from(channels)
            .ok()
            .filter(|value| *value > 0)
            .ok_or(RubberBandError::InvalidChannelCount)?;

        let sample_rate_c =
            c_uint::try_from(sample_rate_hz).map_err(|_| RubberBandError::InvalidSampleRate)?;

        let raw_handle = unsafe {
            rubberband_live_set_default_debug_level(0);
            rubberband_live_new(sample_rate_c, channels_c, options.bits())
        };
        let handle = NonNull::new(raw_handle).ok_or(RubberBandError::ConstructionFailed)?;

        let mut shifter = Self {
            handle,
            channels,
            block_size: 0,
            start_delay: 0,
            input_ptrs: vec![std::ptr::null(); channels],
            output_ptrs: vec![std::ptr::null_mut(); channels],
        };
        shifter.set_debug_level(0);

        let reported_channels = shifter.channel_count();
        if reported_channels != channels {
            return Err(RubberBandError::ChannelCountMismatch {
                requested: channels,
                reported: reported_channels,
            });
        }

        shifter.block_size = shifter.query_block_size();
        if shifter.block_size == 0 {
            return Err(RubberBandError::InvalidBlockSize);
        }
        shifter.start_delay = shifter.query_start_delay();

        Ok(shifter)
    }

    pub(crate) const fn api_version() -> RubberBandApiVersion {
        RubberBandApiVersion {
            major: RUBBERBAND_API_MAJOR_VERSION,
            minor: RUBBERBAND_API_MINOR_VERSION,
        }
    }

    pub(crate) fn channel_count(&self) -> usize {
        unsafe { rubberband_live_get_channel_count(self.handle.as_ptr()) as usize }
    }

    pub(crate) fn block_size(&self) -> usize {
        self.block_size
    }

    pub(crate) fn start_delay(&self) -> usize {
        self.start_delay
    }

    pub(crate) fn pitch_scale(&self) -> f64 {
        unsafe { rubberband_live_get_pitch_scale(self.handle.as_ptr()) as f64 }
    }

    pub(crate) fn set_pitch_scale(&mut self, scale: f64) -> Result<(), RubberBandError> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(RubberBandError::InvalidPitchScale);
        }

        unsafe {
            rubberband_live_set_pitch_scale(self.handle.as_ptr(), scale as c_double);
        }
        self.start_delay = self.query_start_delay();
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        unsafe {
            rubberband_live_reset(self.handle.as_ptr());
        }
    }

    pub(crate) fn shift(
        &mut self,
        input: &[Vec<f32>],
        output: &mut [Vec<f32>],
    ) -> Result<(), RubberBandError> {
        self.prepare_channel_ptrs(input, output)?;

        unsafe {
            rubberband_live_shift(
                self.handle.as_ptr(),
                self.input_ptrs.as_ptr(),
                self.output_ptrs.as_ptr(),
            );
        }
        Ok(())
    }

    fn prepare_channel_ptrs(
        &mut self,
        input: &[Vec<f32>],
        output: &mut [Vec<f32>],
    ) -> Result<(), RubberBandError> {
        validate_channel_count(self.channels, input.len())?;
        validate_channel_count(self.channels, output.len())?;

        for channel in 0..self.channels {
            let input_len = input[channel].len();
            if input_len < self.block_size {
                return Err(RubberBandError::BufferTooSmall {
                    channel,
                    required: self.block_size,
                    actual: input_len,
                });
            }

            let output_len = output[channel].len();
            if output_len < self.block_size {
                return Err(RubberBandError::BufferTooSmall {
                    channel,
                    required: self.block_size,
                    actual: output_len,
                });
            }

            self.input_ptrs[channel] = input[channel].as_ptr();
            self.output_ptrs[channel] = output[channel].as_mut_ptr();
        }

        Ok(())
    }

    fn query_block_size(&self) -> usize {
        unsafe { rubberband_live_get_block_size(self.handle.as_ptr()) as usize }
    }

    fn query_start_delay(&self) -> usize {
        unsafe { rubberband_live_get_start_delay(self.handle.as_ptr()) as usize }
    }

    fn set_debug_level(&mut self, level: c_int) {
        unsafe {
            rubberband_live_set_debug_level(self.handle.as_ptr(), level);
        }
    }
}

impl Drop for RubberBandLiveShifter {
    fn drop(&mut self) {
        unsafe {
            rubberband_live_delete(self.handle.as_ptr());
        }
    }
}

fn validate_channel_count(expected: usize, actual: usize) -> Result<(), RubberBandError> {
    if actual == expected {
        Ok(())
    } else {
        Err(RubberBandError::BufferChannelCount { expected, actual })
    }
}

#[repr(C)]
struct RubberBandLiveStateOpaque {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn rubberband_live_new(
        sample_rate: c_uint,
        channels: c_uint,
        options: c_int,
    ) -> *mut RubberBandLiveStateOpaque;
    fn rubberband_live_delete(state: *mut RubberBandLiveStateOpaque);
    fn rubberband_live_reset(state: *mut RubberBandLiveStateOpaque);
    fn rubberband_live_set_pitch_scale(state: *mut RubberBandLiveStateOpaque, scale: c_double);
    fn rubberband_live_get_pitch_scale(state: *mut RubberBandLiveStateOpaque) -> c_double;
    fn rubberband_live_get_start_delay(state: *mut RubberBandLiveStateOpaque) -> c_uint;
    fn rubberband_live_get_block_size(state: *mut RubberBandLiveStateOpaque) -> c_uint;
    fn rubberband_live_shift(
        state: *mut RubberBandLiveStateOpaque,
        input: *const *const c_float,
        output: *const *mut c_float,
    );
    fn rubberband_live_get_channel_count(state: *mut RubberBandLiveStateOpaque) -> c_uint;
    fn rubberband_live_set_debug_level(state: *mut RubberBandLiveStateOpaque, level: c_int);
    fn rubberband_live_set_default_debug_level(level: c_int);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_block(
        channels: usize,
        block_size: usize,
        block_index: usize,
        sample_rate_hz: f32,
    ) -> Vec<Vec<f32>> {
        let mut input = (0..channels)
            .map(|_| vec![0.0; block_size])
            .collect::<Vec<_>>();

        for (channel, channel_input) in input.iter_mut().enumerate() {
            let frequency_hz = 220.0 + channel as f32 * 110.0;
            for (index, sample) in channel_input.iter_mut().enumerate() {
                let frame = block_index * block_size + index;
                let phase = frame as f32 * frequency_hz * std::f32::consts::TAU / sample_rate_hz;
                *sample = phase.sin() * 0.25;
            }
        }

        input
    }

    #[test]
    fn live_option_bits_match_c_api() {
        assert_eq!(RubberBandLiveOptions::WINDOW_SHORT.bits(), 0x00000000);
        assert_eq!(RubberBandLiveOptions::WINDOW_MEDIUM.bits(), 0x00100000);
        assert_eq!(RubberBandLiveOptions::FORMANT_SHIFTED.bits(), 0x00000000);
        assert_eq!(RubberBandLiveOptions::FORMANT_PRESERVED.bits(), 0x01000000);
        assert_eq!(RubberBandLiveOptions::CHANNELS_APART.bits(), 0x00000000);
        assert_eq!(RubberBandLiveOptions::CHANNELS_TOGETHER.bits(), 0x10000000);
        assert_eq!(
            RubberBandLiveOptions::WINDOW_SHORT
                .union(RubberBandLiveOptions::CHANNELS_TOGETHER)
                .bits(),
            RubberBandLiveOptions::default().bits()
        );
    }

    #[test]
    fn constructs_and_reports_fixed_live_properties() {
        let mut shifter = RubberBandLiveShifter::new(48_000, 2).unwrap();

        assert_eq!(
            RubberBandLiveShifter::api_version(),
            RubberBandApiVersion { major: 3, minor: 0 }
        );
        assert_eq!(shifter.channel_count(), 2);
        assert!(shifter.block_size() > 0);
        assert!(shifter.block_size() <= 8192);
        assert!(shifter.start_delay() > 0);
        assert!((shifter.pitch_scale() - 1.0).abs() < f64::EPSILON);

        shifter.set_pitch_scale(0.5).unwrap();
        assert!((shifter.pitch_scale() - 0.5).abs() < 1.0e-12);
        assert!(shifter.start_delay() > 0);
        shifter.reset();
    }

    #[test]
    fn rejects_invalid_construction_and_pitch_values() {
        assert!(matches!(
            RubberBandLiveShifter::new(0, 2),
            Err(RubberBandError::InvalidSampleRate)
        ));
        assert!(matches!(
            RubberBandLiveShifter::new(48_000, 0),
            Err(RubberBandError::InvalidChannelCount)
        ));

        let mut shifter = RubberBandLiveShifter::new(48_000, 1).unwrap();
        assert!(matches!(
            shifter.set_pitch_scale(0.0),
            Err(RubberBandError::InvalidPitchScale)
        ));
        assert!(matches!(
            shifter.set_pitch_scale(f64::NAN),
            Err(RubberBandError::InvalidPitchScale)
        ));
    }

    #[test]
    fn shift_requires_preallocated_matching_buffers() {
        let mut shifter = RubberBandLiveShifter::new(48_000, 2).unwrap();
        let block_size = shifter.block_size();
        let input = vec![vec![0.0; block_size]; 1];
        let mut output = vec![vec![0.0; block_size]; 2];

        assert!(matches!(
            shifter.shift(&input, &mut output),
            Err(RubberBandError::BufferChannelCount {
                expected: 2,
                actual: 1
            })
        ));

        let input = vec![vec![0.0; block_size - 1], vec![0.0; block_size]];
        let mut output = vec![vec![0.0; block_size]; 2];
        assert!(matches!(
            shifter.shift(&input, &mut output),
            Err(RubberBandError::BufferTooSmall { channel: 0, .. })
        ));
    }

    #[test]
    fn shift_produces_finite_fixed_blocks() {
        let mut shifter = RubberBandLiveShifter::new(48_000, 2).unwrap();
        shifter.set_pitch_scale(0.5).unwrap();
        let block_size = shifter.block_size();
        let blocks_to_prime = shifter.start_delay() / block_size + 4;
        let mut output = vec![vec![0.0; block_size]; 2];
        let mut energy_after_delay = 0.0;

        for block_index in 0..blocks_to_prime {
            let input = sine_block(2, block_size, block_index, 48_000.0);
            shifter.shift(&input, &mut output).unwrap();

            for channel in &output {
                assert!(channel.iter().all(|sample| sample.is_finite()));
            }

            if block_index + 1 >= blocks_to_prime {
                energy_after_delay += output
                    .iter()
                    .flat_map(|channel| channel.iter())
                    .map(|sample| sample.abs())
                    .sum::<f32>();
            }
        }

        assert!(
            energy_after_delay > 0.0,
            "Rubber Band returned only silence after its reported start delay"
        );
    }
}
