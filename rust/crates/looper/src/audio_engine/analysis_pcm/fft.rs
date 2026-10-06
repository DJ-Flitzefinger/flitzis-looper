//! Shared dimensions and finite tail padding for the pinned Rubato FFT setup.

use super::PcmError;

pub(super) const RESAMPLE_CHUNK_FRAMES: usize = 1024;

/// FixedSync::Input, one subchunk and one channel. An FFT input unit is a
/// multiple of the reduced input rate and at least one complete input chunk,
/// so a process call emits at most one FFT output unit.
pub(super) fn fft_dimensions(src_rate: u32, target_rate: u32) -> (usize, usize) {
    let mut left = src_rate as usize;
    let mut right = target_rate as usize;
    while right != 0 {
        (left, right) = (right, left % right);
    }
    let reduced_input = src_rate as usize / left;
    let fft_units = RESAMPLE_CHUNK_FRAMES.div_ceil(reduced_input);
    (
        fft_units * reduced_input,
        fft_units * (target_rate as usize / left),
    )
}

/// Every ceil(FFT input size / input chunk) padding calls emit at least one
/// FFT output unit, regardless of saved phase. Zero output can advance that
/// phase and is valid within this finite budget. Callers validate both rates.
pub(super) fn tail_call_budget(
    missing_frames: usize,
    src_rate: u32,
    target_rate: u32,
) -> Result<usize, PcmError> {
    let (fft_input, fft_output) = fft_dimensions(src_rate, target_rate);
    missing_frames
        .div_ceil(fft_output)
        .checked_mul(fft_input.div_ceil(RESAMPLE_CHUNK_FRAMES))
        .ok_or(PcmError::Limit("resampler tail call budget overflow"))
}
