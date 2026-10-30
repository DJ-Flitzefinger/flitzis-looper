//! Bounded loaded-source export and file-backed KeyNet preprocessing.
//!
//! The caller keeps original playback PCM alive; admission includes that owner.
//! A retained native file handle supplies the exact exported mono to both branches.

use super::fft::{RESAMPLE_CHUNK_FRAMES, fft_dimensions, tail_call_budget};
use super::{
    CHUNK_FRAMES, LoadedPcmSnapshot, PcmError, check_cancelled, check_limit, pcm_bytes,
    validate_rate,
};
use audioadapter_buffers::owned::InterleavedOwned;
use rubato::{Fft, FixedSync, Indexing, Resampler};
use std::io::{Read, Write};

const KEY_RATE_HZ: u32 = 44_100;
const IO_CHUNK_BYTES: usize = CHUNK_FRAMES * size_of::<f32>();

/// PCM-only reservations; existing internal FFT/CQT/ORT scratch is excluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PcmStagingPlan {
    pub export_peak_bytes: usize,
    pub key_peak_bytes: usize,
    pub export_bytes: usize,
}

pub(crate) struct PreparedKeyPcm {
    pub samples: Vec<f32>,
    /// Actual owned PCM capacities plus the bounded read-byte buffer.
    pub peak_bytes: usize,
}

impl LoadedPcmSnapshot {
    pub(crate) fn exported_bytes(&self) -> Result<usize, PcmError> {
        pcm_bytes(self.frame_count)
    }

    /// Arithmetic only: safe to run before admission without constructing an FFT.
    pub(crate) fn staging_plan(&self) -> Result<PcmStagingPlan, PcmError> {
        staging_plan(self.retained_bytes, self.frame_count, self.rate_hz)
    }

    /// Export every channel mean directly; no full-track mono allocation exists.
    /// The caller uses an unbuffered file and retires any incomplete export.
    pub(crate) fn stream_f32_le(
        &self,
        output: &mut impl Write,
        max_working_bytes: usize,
        max_export_bytes: usize,
        cancelled: &impl Fn() -> bool,
    ) -> Result<usize, PcmError> {
        check_cancelled(cancelled)?;
        let export_bytes = self.exported_bytes()?;
        check_limit(export_bytes, max_export_bytes, "export bytes")?;
        let export_peak = add_bytes(self.retained_bytes, IO_CHUNK_BYTES)?;
        check_limit(
            export_peak,
            max_working_bytes,
            "source and export chunk bytes",
        )?;
        let mut bytes = [0_u8; IO_CHUNK_BYTES];
        for chunk in self
            .sample
            .samples
            .chunks(CHUNK_FRAMES * self.sample.channels)
        {
            check_cancelled(cancelled)?;
            let frames = chunk.len() / self.sample.channels;
            for (frame, destination) in chunk
                .chunks_exact(self.sample.channels)
                .zip(bytes.chunks_exact_mut(size_of::<f32>()))
            {
                let mut sum = 0.0_f64;
                for value in frame {
                    if !value.is_finite() {
                        return Err(PcmError::InvalidInput("non-finite loaded sample"));
                    }
                    sum += f64::from(*value);
                }
                let mono = (sum / self.sample.channels as f64) as f32;
                destination.copy_from_slice(&mono.to_le_bytes());
            }
            output.write_all(&bytes[..frames * size_of::<f32>()])?;
        }
        check_cancelled(cancelled)?;
        output.flush()?;
        check_cancelled(cancelled)?;
        Ok(export_bytes)
    }
}

fn add_bytes(left: usize, right: usize) -> Result<usize, PcmError> {
    left.checked_add(right)
        .ok_or(PcmError::Limit("analysis PCM reservation overflow"))
}

fn output_frames(frame_count: usize, rate_hz: u32) -> Result<usize, PcmError> {
    validate_rate(rate_hz)?;
    if frame_count == 0 {
        return Err(PcmError::InvalidInput("empty staged mono input"));
    }
    usize::try_from((frame_count as u128 * u128::from(KEY_RATE_HZ)).div_ceil(u128::from(rate_hz)))
        .map_err(|_| PcmError::Limit("key output frame count overflow"))
}

/// Exact external adapter dimensions for the pinned Rubato 1.0 configuration:
/// FixedSync::Input, 1024 input frames, one subchunk, one channel. A FFT input
/// unit is a multiple of the reduced input rate and at least 1024 frames;
/// consequently at most one FFT unit can be emitted by any process call.
/// Runtime checks and tests compare this cheap arithmetic against Rubato itself.
fn converter_dimensions(rate_hz: u32) -> (usize, usize) {
    (
        RESAMPLE_CHUNK_FRAMES,
        fft_dimensions(rate_hz, KEY_RATE_HZ).1,
    )
}

pub(crate) fn key_stream_working_bytes(
    frame_count: usize,
    rate_hz: u32,
) -> Result<usize, PcmError> {
    output_frames(frame_count, rate_hz)?;
    // Read bytes, output encoding bytes, and one source chunk for the direct path.
    let mut peak = IO_CHUNK_BYTES * 2;
    if rate_hz == KEY_RATE_HZ {
        peak = add_bytes(peak, pcm_bytes(CHUNK_FRAMES)?)?;
    }
    if rate_hz != KEY_RATE_HZ {
        let (input, output) = converter_dimensions(rate_hz);
        peak = add_bytes(peak, pcm_bytes(input)?)?;
        peak = add_bytes(peak, pcm_bytes(output)?)?;
    }
    Ok(peak)
}

pub(crate) fn staging_plan(
    retained_bytes: usize,
    frame_count: usize,
    rate_hz: u32,
) -> Result<PcmStagingPlan, PcmError> {
    Ok(PcmStagingPlan {
        export_peak_bytes: add_bytes(retained_bytes, IO_CHUNK_BYTES)?,
        key_peak_bytes: key_peak_bytes(frame_count, rate_hz)?,
        export_bytes: pcm_bytes(frame_count)?,
    })
}

fn read_samples(
    input: &mut impl Read,
    samples: &mut [f32],
    cancelled: &impl Fn() -> bool,
) -> Result<(), PcmError> {
    let mut bytes = [0_u8; IO_CHUNK_BYTES];
    for chunk in samples.chunks_mut(CHUNK_FRAMES) {
        check_cancelled(cancelled)?;
        input.read_exact(&mut bytes[..std::mem::size_of_val(chunk)])?;
        check_cancelled(cancelled)?;
        for (sample, data) in chunk.iter_mut().zip(bytes.chunks_exact(size_of::<f32>())) {
            let value = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            if !value.is_finite() {
                return Err(PcmError::InvalidInput("non-finite staged mono sample"));
            }
            *sample = value;
        }
    }
    Ok(())
}

fn check_end(input: &mut impl Read, cancelled: &impl Fn() -> bool) -> Result<(), PcmError> {
    check_cancelled(cancelled)?;
    let mut extra = [0_u8; 1];
    match input.read_exact(&mut extra) {
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {}
        Err(error) => return Err(error.into()),
        Ok(()) => return Err(PcmError::InvalidInput("staged mono has trailing bytes")),
    }
    check_cancelled(cancelled)
}

fn zeroed_pcm(frames: usize, cancelled: &impl Fn() -> bool) -> Result<Vec<f32>, PcmError> {
    check_cancelled(cancelled)?;
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(frames)
        .map_err(|_| PcmError::Limit("PCM allocation unavailable"))?;
    for start in (0..frames).step_by(CHUNK_FRAMES) {
        check_cancelled(cancelled)?;
        samples.resize((start + CHUNK_FRAMES).min(frames), 0.0);
    }
    check_cancelled(cancelled)?;
    Ok(samples)
}

/// Stream the unchanged complete 44100-Hz input through one conversion core.
fn stream_key_input(
    input: &mut impl Read,
    frame_count: usize,
    rate_hz: u32,
    max_working_bytes: usize,
    cancelled: &impl Fn() -> bool,
    emit: &mut impl FnMut(&[f32]) -> Result<(), PcmError>,
) -> Result<usize, PcmError> {
    check_cancelled(cancelled)?;
    pcm_bytes(frame_count)?;
    let expected_frames = output_frames(frame_count, rate_hz)?;
    let reservation = key_stream_working_bytes(frame_count, rate_hz)?;
    check_limit(reservation, max_working_bytes, "staged key PCM bytes")?;
    if rate_hz == KEY_RATE_HZ {
        let mut chunk = [0.0; CHUNK_FRAMES];
        for start in (0..frame_count).step_by(CHUNK_FRAMES) {
            let count = CHUNK_FRAMES.min(frame_count - start);
            read_samples(input, &mut chunk[..count], cancelled)?;
            emit(&chunk[..count])?;
        }
        check_end(input, cancelled)?;
        return Ok(reservation);
    }

    let mut converter = Fft::<f32>::new(
        rate_hz as usize,
        KEY_RATE_HZ as usize,
        RESAMPLE_CHUNK_FRAMES,
        1,
        1,
        FixedSync::Input,
    )
    .map_err(|error| PcmError::Resample(error.to_string()))?;
    check_cancelled(cancelled)?;
    let dimensions = (converter.input_frames_max(), converter.output_frames_max());
    if dimensions != converter_dimensions(rate_hz) {
        return Err(PcmError::Resample(
            "converter dimensions differ from PCM reservation".into(),
        ));
    }
    let mut source_chunk = zeroed_pcm(dimensions.0, cancelled)?;
    let mut converted_chunk = zeroed_pcm(dimensions.1, cancelled)?;
    let peak_bytes = [source_chunk.capacity(), converted_chunk.capacity()]
        .into_iter()
        .try_fold(IO_CHUNK_BYTES * 2, |total, capacity| {
            add_bytes(total, pcm_bytes(capacity)?)
        })?;
    check_limit(peak_bytes, max_working_bytes, "actual staged key PCM bytes")?;

    let delay_frames = converter.output_delay();
    let required_frames = delay_frames
        .checked_add(expected_frames)
        .ok_or(PcmError::Limit("resampler delayed output overflow"))?;
    let mut frames_left = frame_count;
    let mut produced_total = 0_usize;
    let mut written = 0_usize;
    let mut tail_calls_left = None;
    while frames_left > 0 || produced_total < required_frames {
        check_cancelled(cancelled)?;
        let input_frames = converter.input_frames_next();
        let available = frames_left.min(input_frames);
        if available == 0 {
            let remaining = match tail_calls_left {
                Some(remaining) => remaining,
                None => tail_call_budget(required_frames - produced_total, rate_hz, KEY_RATE_HZ)?,
            };
            if remaining == 0 {
                return Err(PcmError::Resample(
                    "staged tail exceeded bounded padding".into(),
                ));
            }
            tail_calls_left = Some(remaining - 1);
        }
        read_samples(input, &mut source_chunk[..available], cancelled)?;
        let indexing = Indexing {
            input_offset: 0,
            output_offset: 0,
            partial_len: (available < input_frames).then_some(available),
            active_channels_mask: None,
        };
        let source = InterleavedOwned::new_from(source_chunk, 1, dimensions.0)
            .map_err(|error| PcmError::Resample(error.to_string()))?;
        let mut converted = InterleavedOwned::new_from(converted_chunk, 1, dimensions.1)
            .map_err(|error| PcmError::Resample(error.to_string()))?;
        let (consumed, produced) = converter
            .process_into_buffer(&source, &mut converted, Some(&indexing))
            .map_err(|error| PcmError::Resample(error.to_string()))?;
        source_chunk = source.take_data();
        converted_chunk = converted.take_data();
        if available == input_frames && consumed == 0 {
            return Err(PcmError::Resample(
                "staged conversion made no progress".into(),
            ));
        }
        if available > 0 {
            frames_left -= available;
            if frames_left == 0 {
                check_end(input, cancelled)?;
            }
        }
        let start = delay_frames.saturating_sub(produced_total).min(produced);
        let count = (produced - start).min(expected_frames - written);
        for chunk in converted_chunk[start..start + count].chunks(CHUNK_FRAMES) {
            check_cancelled(cancelled)?;
            emit(chunk)?;
            written += chunk.len();
        }
        produced_total = produced_total
            .checked_add(produced)
            .ok_or(PcmError::Limit("resampler output frame count overflow"))?;
    }
    check_cancelled(cancelled)?;
    if written != expected_frames {
        return Err(PcmError::Resample("staged output length mismatch".into()));
    }
    Ok(peak_bytes)
}

/// File output keeps whole-track PCM off the admitted heap. Partial files belong to the job.
pub(crate) fn key_input_to_f32_le(
    input: &mut impl Read,
    output: &mut impl Write,
    frame_count: usize,
    rate_hz: u32,
    max_working_bytes: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<usize, PcmError> {
    let peak = stream_key_input(
        input,
        frame_count,
        rate_hz,
        max_working_bytes,
        cancelled,
        &mut |samples| {
            check_cancelled(cancelled)?;
            let mut bytes = [0_u8; IO_CHUNK_BYTES];
            for (sample, encoded) in samples.iter().zip(bytes.chunks_exact_mut(4)) {
                encoded.copy_from_slice(&sample.to_le_bytes());
            }
            output.write_all(&bytes[..samples.len() * 4])?;
            Ok(())
        },
    )?;
    output.flush()?;
    check_cancelled(cancelled)?;
    Ok(peak)
}

fn key_peak_bytes(frame_count: usize, rate_hz: u32) -> Result<usize, PcmError> {
    add_bytes(
        pcm_bytes(output_frames(frame_count, rate_hz)?)?,
        key_stream_working_bytes(frame_count, rate_hz)?,
    )
}

/// Dense adapter for the existing offline API and independent PCM test oracles.
pub(crate) fn key_input_from_f32_le(
    input: &mut impl Read,
    frame_count: usize,
    rate_hz: u32,
    max_working_bytes: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<PreparedKeyPcm, PcmError> {
    check_limit(
        key_peak_bytes(frame_count, rate_hz)?,
        max_working_bytes,
        "dense key PCM bytes",
    )?;
    let mut samples = zeroed_pcm(output_frames(frame_count, rate_hz)?, cancelled)?;
    let owned = pcm_bytes(samples.capacity())?;
    let available = max_working_bytes
        .checked_sub(owned)
        .ok_or(PcmError::Limit("dense key PCM bytes"))?;
    let mut written = 0;
    let peak = stream_key_input(
        input,
        frame_count,
        rate_hz,
        available,
        cancelled,
        &mut |chunk| {
            samples[written..written + chunk.len()].copy_from_slice(chunk);
            written += chunk.len();
            Ok(())
        },
    )?;
    Ok(PreparedKeyPcm {
        samples,
        peak_bytes: add_bytes(owned, peak)?,
    })
}

#[cfg(test)]
#[path = "streamed_tests.rs"]
mod tests;
