//! Immutable loaded-audio analysis inputs. All work here is non-realtime.
//!
//! PCM stays in the loaded playback-buffer domain: frame zero is preserved,
//! channel mixing never trims silence, and exported samples are complete f32 LE
//! mono at the actual loaded rate. The Beat This frontend owns its independent
//! 22050-Hz conversion; the existing key frontend derives 44100 Hz directly here.

use crate::messages::SampleBuffer;
#[cfg(test)]
use std::io::Write;
#[cfg(test)]
use std::sync::Arc;

mod fft;
mod streamed;
use fft::{RESAMPLE_CHUNK_FRAMES, tail_call_budget};
pub(crate) use streamed::{PcmStagingPlan, key_input_from_f32_le};

pub(crate) const MONO_RULE: &str = "arithmetic-channel-mean-f64-v1";
pub(crate) const KEY_PREPROCESSING: &str = "rubato-fft-1.0-44100-delay-trim-tail-flush-v1";
const CHUNK_FRAMES: usize = 4096;
const MAX_CHANNELS: usize = 32;
const MIN_RATE_HZ: u32 = 8000;
const MAX_RATE_HZ: u32 = 384_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PcmIdentity {
    pub pad_id: usize,
    pub request_id: u64,
    pub source_id: String,
    pub source_generation: u64,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PcmError {
    #[error("analysis PCM preparation cancelled")]
    Cancelled,
    #[error("invalid analysis PCM: {0}")]
    InvalidInput(&'static str),
    #[error("analysis PCM limit exceeded: {0}")]
    Limit(&'static str),
    #[error("analysis PCM export failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("analysis resampling failed: {0}")]
    Resample(String),
}

/// Pins the exact immutable loaded source, without copying or decoding it.
pub(crate) struct LoadedPcmSnapshot {
    sample: SampleBuffer,
    rate_hz: u32,
    #[cfg(test)]
    identity: PcmIdentity,
    frame_count: usize,
    retained_bytes: usize,
}

impl LoadedPcmSnapshot {
    pub(crate) fn new(
        sample: SampleBuffer,
        rate_hz: u32,
        identity: PcmIdentity,
        max_pcm_bytes: usize,
    ) -> Result<Self, PcmError> {
        validate_rate(rate_hz)?;
        if sample.channels == 0 || sample.channels > MAX_CHANNELS {
            return Err(PcmError::InvalidInput("channel count must be in 1..=32"));
        }
        if sample.samples.is_empty() || !sample.samples.len().is_multiple_of(sample.channels) {
            return Err(PcmError::InvalidInput(
                "empty or incomplete interleaved frames",
            ));
        }
        if identity.request_id == 0
            || identity.source_generation == 0
            || identity.source_id.is_empty()
            || identity.source_id.len() > 1024
        {
            return Err(PcmError::InvalidInput(
                "missing or oversized source/request identity",
            ));
        }
        let retained_bytes = pcm_bytes(sample.samples.len())?;
        check_limit(retained_bytes, max_pcm_bytes, "loaded source bytes")?;
        let frame_count = sample.samples.len() / sample.channels;
        Ok(Self {
            sample,
            rate_hz,
            #[cfg(test)]
            identity,
            frame_count,
            retained_bytes,
        })
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub(crate) fn source_channels(&self) -> usize {
        self.sample.channels
    }

    /// Peak of separate source-export and file-to-key stages, without FFT setup.
    #[cfg(test)]
    pub(crate) fn working_pcm_bytes(&self) -> Result<usize, PcmError> {
        let plan = self.staging_plan()?;
        Ok(plan.export_peak_bytes.max(plan.key_peak_bytes))
    }

    #[cfg(test)]
    pub(crate) fn prepare_mono(
        &self,
        max_pcm_bytes: usize,
        cancelled: &impl Fn() -> bool,
    ) -> Result<SharedMono, PcmError> {
        let mono = self.prepare_complete_mono(max_pcm_bytes, cancelled)?;
        Ok(SharedMono {
            samples: Arc::new(mono),
            identity: self.identity.clone(),
            rate_hz: self.rate_hz,
            frame_count: self.frame_count,
            source_channels: self.sample.channels,
            origin_seconds: 0.0,
        })
    }

    /// Complete loaded-rate channel means for explicit non-realtime timing acceptance.
    pub(crate) fn prepare_complete_mono(
        &self,
        max_pcm_bytes: usize,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Vec<f32>, PcmError> {
        check_cancelled(cancelled)?;
        check_limit(pcm_bytes(self.frame_count)?, max_pcm_bytes, "mono bytes")?;
        let mut mono = Vec::with_capacity(self.frame_count);
        for chunk in self
            .sample
            .samples
            .chunks(CHUNK_FRAMES * self.sample.channels)
        {
            check_cancelled(cancelled)?;
            for frame in chunk.chunks_exact(self.sample.channels) {
                let mut sum = 0.0_f64;
                for value in frame {
                    if !value.is_finite() {
                        return Err(PcmError::InvalidInput("non-finite loaded sample"));
                    }
                    sum += f64::from(*value);
                }
                // A wider sum cannot overflow for any supported finite f32 frame.
                mono.push((sum / self.sample.channels as f64) as f32);
            }
        }
        check_cancelled(cancelled)?;
        Ok(mono)
    }
}

/// Full-buffer test oracle retained for streamed preparation parity checks.
#[derive(Clone)]
#[cfg(test)]
pub(crate) struct SharedMono {
    // Arc<Vec<_>> avoids a second full-track allocation when preparing the Arc.
    samples: Arc<Vec<f32>>,
    pub identity: PcmIdentity,
    pub rate_hz: u32,
    pub frame_count: usize,
    pub source_channels: usize,
    pub origin_seconds: f64,
}

#[cfg(test)]
impl SharedMono {
    pub(crate) fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub(crate) fn byte_len(&self) -> usize {
        self.samples.len() * size_of::<f32>()
    }

    fn validate_metadata(&self) -> Result<(), PcmError> {
        validate_rate(self.rate_hz)?;
        if self.frame_count != self.samples().len()
            || self.frame_count == 0
            || !(1..=MAX_CHANNELS).contains(&self.source_channels)
            || self.origin_seconds != 0.0
        {
            return Err(PcmError::InvalidInput("inconsistent shared mono metadata"));
        }
        Ok(())
    }

    /// Write bounded chunks; the owner retires any incomplete temporary file.
    pub(crate) fn write_f32_le(
        &self,
        output: &mut impl Write,
        max_bytes: usize,
        cancelled: &impl Fn() -> bool,
    ) -> Result<usize, PcmError> {
        check_cancelled(cancelled)?;
        self.validate_metadata()?;
        check_limit(self.byte_len(), max_bytes, "export bytes")?;
        let mut bytes = [0_u8; CHUNK_FRAMES * size_of::<f32>()];
        for chunk in self.samples.chunks(CHUNK_FRAMES) {
            check_cancelled(cancelled)?;
            for (sample, destination) in chunk.iter().zip(bytes.chunks_exact_mut(4)) {
                destination.copy_from_slice(&sample.to_le_bytes());
            }
            output.write_all(&bytes[..std::mem::size_of_val(chunk)])?;
        }
        check_cancelled(cancelled)?;
        output.flush()?;
        check_cancelled(cancelled)?;
        Ok(self.byte_len())
    }

    /// Derive KeyNet PCM directly from the loaded-rate shared mono, not beat PCM.
    ///
    /// The limit includes shared mono plus the owned input/output PCM buffers.
    /// Rubato's bounded FFT scratch and later KeyNet/ORT workspace are separate.
    pub(crate) fn key_input(
        &self,
        max_working_bytes: usize,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Vec<f32>, PcmError> {
        check_cancelled(cancelled)?;
        self.validate_metadata()?;
        let available = max_working_bytes
            .checked_sub(self.byte_len())
            .ok_or(PcmError::Limit("shared mono and key PCM working bytes"))?;
        check_limit(self.byte_len(), available, "key input copy bytes")?;
        let mut input = Vec::with_capacity(self.samples.len());
        for chunk in self.samples.chunks(CHUNK_FRAMES) {
            check_cancelled(cancelled)?;
            input.extend_from_slice(chunk);
        }
        resample_mono_cancellable(input, self.rate_hz, 44_100, available, cancelled)
    }
}

fn validate_rate(rate_hz: u32) -> Result<(), PcmError> {
    if !(MIN_RATE_HZ..=MAX_RATE_HZ).contains(&rate_hz) {
        return Err(PcmError::InvalidInput(
            "sample rate must be in 8000..=384000 Hz",
        ));
    }
    Ok(())
}

fn pcm_bytes(frames: usize) -> Result<usize, PcmError> {
    frames
        .checked_mul(size_of::<f32>())
        .ok_or(PcmError::Limit("PCM byte count overflow"))
}

fn check_limit(bytes: usize, maximum: usize, name: &'static str) -> Result<(), PcmError> {
    if bytes > maximum {
        return Err(PcmError::Limit(name));
    }
    Ok(())
}

fn check_cancelled(cancelled: &impl Fn() -> bool) -> Result<(), PcmError> {
    if cancelled() {
        return Err(PcmError::Cancelled);
    }
    Ok(())
}

/// The existing Rubato FFT conversion, with bounded PCM and cooperative checks.
///
/// Zero padding flushes the complete tail. Leading algorithmic delay is removed
/// exactly once, then output has ceil(input_frames * target_rate / source_rate)
/// frames. The returned Vec reuses the output allocation after trimming delay.
pub(crate) fn resample_mono_cancellable(
    mono: Vec<f32>,
    src_rate: u32,
    target_rate: u32,
    max_working_bytes: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<f32>, PcmError> {
    use audioadapter_buffers::owned::InterleavedOwned;
    use rubato::{Fft, FixedSync, Indexing, Resampler};

    check_cancelled(cancelled)?;
    validate_rate(src_rate)?;
    validate_rate(target_rate)?;
    let input_frames = mono.len();
    let input_bytes = pcm_bytes(mono.capacity())?;
    check_limit(input_bytes, max_working_bytes, "resampler input bytes")?;
    if src_rate == target_rate || mono.is_empty() {
        return Ok(mono);
    }
    let expected_output_len =
        (input_frames as u128 * u128::from(target_rate)).div_ceil(u128::from(src_rate));
    let expected_output_len = usize::try_from(expected_output_len)
        .map_err(|_| PcmError::Limit("resampler output frame count overflow"))?;
    let minimum_bytes = input_bytes
        .checked_add(pcm_bytes(expected_output_len)?)
        .ok_or(PcmError::Limit("resampler working bytes overflow"))?;
    check_limit(minimum_bytes, max_working_bytes, "resampler working bytes")?;

    let mut resampler = Fft::<f32>::new(
        src_rate as usize,
        target_rate as usize,
        RESAMPLE_CHUNK_FRAMES,
        1,
        1,
        FixedSync::Input,
    )
    .map_err(|error| PcmError::Resample(error.to_string()))?;
    check_cancelled(cancelled)?;
    let delay_frames = resampler.output_delay();
    let required_output_len = delay_frames
        .checked_add(expected_output_len)
        .ok_or(PcmError::Limit("resampler delayed output overflow"))?;
    let output_frames = required_output_len
        .checked_add(resampler.output_frames_max())
        .ok_or(PcmError::Limit("resampler padded output overflow"))?;
    let total_bytes = input_bytes
        .checked_add(pcm_bytes(output_frames)?)
        .ok_or(PcmError::Limit("resampler total PCM bytes overflow"))?;
    check_limit(total_bytes, max_working_bytes, "resampler total PCM bytes")?;
    let input = InterleavedOwned::new_from(mono, 1, input_frames)
        .map_err(|error| PcmError::Resample(error.to_string()))?;
    let mut output = InterleavedOwned::new_from(vec![0.0_f32; output_frames], 1, output_frames)
        .map_err(|error| PcmError::Resample(error.to_string()))?;
    let mut indexing = Indexing {
        input_offset: 0,
        output_offset: 0,
        partial_len: None,
        active_channels_mask: None,
    };
    let mut frames_left = input_frames;
    let mut output_len = 0;
    while frames_left >= resampler.input_frames_next() {
        check_cancelled(cancelled)?;
        let (consumed, produced) = resampler
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .map_err(|error| PcmError::Resample(error.to_string()))?;
        if consumed == 0 {
            return Err(PcmError::Resample(
                "input conversion made no progress".into(),
            ));
        }
        frames_left -= consumed;
        output_len += produced;
        indexing.input_offset += consumed;
        indexing.output_offset += produced;
    }
    if frames_left > 0 {
        check_cancelled(cancelled)?;
        indexing.partial_len = Some(frames_left);
        let (_, produced) = resampler
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .map_err(|error| PcmError::Resample(error.to_string()))?;
        output_len += produced;
        indexing.output_offset += produced;
    }
    indexing.partial_len = Some(0);
    let mut tail_calls_left = tail_call_budget(
        required_output_len.saturating_sub(output_len),
        src_rate,
        target_rate,
    )?;
    while output_len < required_output_len {
        check_cancelled(cancelled)?;
        if tail_calls_left == 0 {
            return Err(PcmError::Resample("tail exceeded bounded padding".into()));
        }
        tail_calls_left -= 1;
        let (_, produced) = resampler
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .map_err(|error| PcmError::Resample(error.to_string()))?;
        output_len += produced;
        indexing.output_offset += produced;
    }
    check_cancelled(cancelled)?;
    let mut data = output.take_data();
    // Chunked in-place shift also bounds cancellation latency during the final
    // full-track copy without retaining a second output allocation.
    for start in (0..expected_output_len).step_by(CHUNK_FRAMES) {
        check_cancelled(cancelled)?;
        let end = (start + CHUNK_FRAMES).min(expected_output_len);
        data.copy_within(delay_frames + start..delay_frames + end, start);
    }
    data.truncate(expected_output_len);
    check_cancelled(cancelled)?;
    Ok(data)
}

#[cfg(test)]
mod tempo_gate;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn snapshot(samples: Vec<f32>, channels: usize, rate_hz: u32) -> LoadedPcmSnapshot {
        LoadedPcmSnapshot::new(
            SampleBuffer {
                channels,
                samples: samples.into(),
            },
            rate_hz,
            PcmIdentity {
                pad_id: 3,
                request_id: 7,
                source_id: "loaded-3-2".into(),
                source_generation: 2,
            },
            usize::MAX,
        )
        .unwrap()
    }

    #[test]
    fn snapshot_pins_source_and_mono_preserves_origin_full_tail_and_identity() {
        let loaded = snapshot(vec![1.0, 0.0, 0.0, 0.0, -0.5, 0.5, 0.5, 1.0], 2, 48_000);
        assert_eq!(loaded.retained_bytes(), 32);
        let source_owner = loaded.sample.samples.clone();
        let mono = loaded.prepare_mono(16, &|| false).unwrap();
        drop(loaded);
        assert_eq!(source_owner.len(), 8);
        assert_eq!(mono.samples(), &[0.5, 0.0, 0.0, 0.75]);
        assert_eq!(mono.frame_count, 4);
        assert_eq!(mono.rate_hz, 48_000);
        assert_eq!(mono.source_channels, 2);
        assert_eq!(mono.origin_seconds, 0.0);
        assert_eq!(mono.identity.source_generation, 2);
        assert_eq!(mono.identity.request_id, 7);
        assert_eq!(mono.identity.pad_id, 3);
        let mut bytes = Vec::new();
        assert_eq!(mono.write_f32_le(&mut bytes, 16, &|| false).unwrap(), 16);
        assert_eq!(
            bytes,
            [0.5_f32, 0.0, 0.0, 0.75]
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn mono_rule_uses_all_channels_and_avoids_f32_sum_overflow() {
        let mono = snapshot(
            vec![1.0, 0.0, -1.0, 0.5, f32::MAX, f32::MAX, f32::MAX, f32::MAX],
            4,
            44_100,
        )
        .prepare_mono(8, &|| false)
        .unwrap();
        assert_eq!(mono.samples(), &[0.125, f32::MAX]);
    }

    #[test]
    fn malformed_layout_rate_and_nonfinite_samples_are_rejected() {
        for (samples, channels, rate) in [
            (vec![], 1, 44_100),
            (vec![0.0], 0, 44_100),
            (vec![0.0], 2, 44_100),
            (vec![0.0], 1, 0),
        ] {
            assert!(
                LoadedPcmSnapshot::new(
                    SampleBuffer {
                        channels,
                        samples: samples.into()
                    },
                    rate,
                    PcmIdentity {
                        pad_id: 0,
                        request_id: 1,
                        source_id: "source".into(),
                        source_generation: 1
                    },
                    usize::MAX
                )
                .is_err()
            );
        }
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(matches!(
                snapshot(vec![value], 1, 44_100).prepare_mono(4, &|| false),
                Err(PcmError::InvalidInput(_))
            ));
        }
    }

    #[test]
    fn byte_limits_reject_whole_input_without_writing_a_truncated_export() {
        let loaded = snapshot(vec![0.0; 16], 2, 48_000);
        assert!(matches!(
            loaded.prepare_mono(31, &|| false),
            Err(PcmError::Limit(_))
        ));
        let mono = loaded.prepare_mono(32, &|| false).unwrap();
        let mut bytes = Vec::new();
        assert!(matches!(
            mono.write_f32_le(&mut bytes, 31, &|| false),
            Err(PcmError::Limit(_))
        ));
        assert!(bytes.is_empty());
        assert!(matches!(
            mono.key_input(63, &|| false),
            Err(PcmError::Limit(_))
        ));
        assert!(matches!(
            resample_mono_cancellable(vec![0.0; 10], 48_000, 44_100, 40, &|| false),
            Err(PcmError::Limit(_))
        ));
    }

    #[test]
    fn admission_counts_separate_source_export_and_key_stages() {
        let loaded = snapshot(vec![0.0; 96_000], 2, 48_000);
        assert_eq!(loaded.source_channels(), 2);
        assert_eq!(
            loaded.working_pcm_bytes().unwrap(),
            96_000 * 4 + CHUNK_FRAMES * 4
        );
        let insufficient = LoadedPcmSnapshot::new(
            loaded.sample.clone(),
            loaded.rate_hz,
            loaded.identity.clone(),
            loaded.retained_bytes() - 1,
        );
        assert!(matches!(insufficient, Err(PcmError::Limit(_))));
    }

    #[test]
    fn preparation_and_export_cancel_between_bounded_chunks() {
        let loaded = snapshot(vec![0.5; CHUNK_FRAMES * 3], 1, 44_100);
        let calls = Cell::new(0);
        let cancelled = || {
            calls.set(calls.get() + 1);
            calls.get() >= 3
        };
        assert!(matches!(
            loaded.prepare_mono(usize::MAX, &cancelled),
            Err(PcmError::Cancelled)
        ));
        let mono = loaded.prepare_mono(usize::MAX, &|| false).unwrap();
        calls.set(0);
        let mut bytes = Vec::new();
        assert!(matches!(
            mono.write_f32_le(&mut bytes, usize::MAX, &cancelled),
            Err(PcmError::Cancelled)
        ));
        assert_eq!(bytes.len(), CHUNK_FRAMES * 4);
        assert!(matches!(
            mono.key_input(usize::MAX, &|| true),
            Err(PcmError::Cancelled)
        ));
    }

    #[test]
    fn key_branch_rate_duration_and_fractional_markers_preserve_loaded_origin() {
        for rate in [22_050_u32, 44_100, 48_000] {
            let frames = rate as usize + 17;
            for marker in [0, 137, frames - 1] {
                let mut samples = vec![0.0; frames];
                samples[marker] = 0.75;
                let mono = snapshot(samples, 1, rate)
                    .prepare_mono(usize::MAX, &|| false)
                    .unwrap();
                let output = mono.key_input(usize::MAX, &|| false).unwrap();
                let expected_len = (frames as u64 * 44_100).div_ceil(u64::from(rate)) as usize;
                assert_eq!(output.len(), expected_len);
                let expected_marker = (marker as f64 * 44_100.0 / f64::from(rate)).round() as usize;
                let peak = output
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
                    .unwrap();
                assert!(
                    peak.0.abs_diff(expected_marker) <= 1,
                    "rate={rate}, marker={marker}, peak={peak:?}"
                );
                assert!(
                    *peak.1 > 0.25,
                    "tail marker lost: rate={rate}, marker={marker}, peak={peak:?}"
                );
                assert_eq!(mono.samples()[marker], 0.75);
                assert_eq!(mono.rate_hz, rate);
            }
        }
    }

    #[test]
    fn key_same_rate_is_exact_silence_keeps_duration_and_conversion_cancels() {
        let mono = snapshot(vec![0.0, -0.25, 0.75, 0.0], 1, 44_100)
            .prepare_mono(16, &|| false)
            .unwrap();
        assert_eq!(mono.key_input(32, &|| false).unwrap(), mono.samples());
        let silence =
            resample_mono_cancellable(vec![0.0; 48_017], 48_000, 44_100, usize::MAX, &|| false)
                .unwrap();
        assert!(silence.iter().all(|value| *value == 0.0));
        assert_eq!(
            silence.len(),
            (48_017_u64 * 44_100).div_ceil(48_000) as usize
        );
        let calls = Cell::new(0);
        assert!(matches!(
            resample_mono_cancellable(vec![0.0; 48_017], 48_000, 44_100, usize::MAX, &|| {
                calls.set(calls.get() + 1);
                calls.get() >= 4
            }),
            Err(PcmError::Cancelled)
        ));
    }
}
