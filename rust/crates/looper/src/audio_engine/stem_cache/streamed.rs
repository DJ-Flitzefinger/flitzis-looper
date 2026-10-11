//! Complete alignment and conversion with duration-independent scratch.
use super::wav::Pcm16Wav;
use super::{
    STEM_ALIGNMENT_ANALYSIS_SECONDS, STEM_ALIGNMENT_ONSET_THRESHOLD_RATIO,
    STEM_ALIGNMENT_PRE_ONSET_SECONDS, alignment_offset_in_window,
};
use crate::audio_engine::complete_context::CompleteSourceReader;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Write};

const BLOCK_SAMPLES: usize = 4096;
const FIXED_SCRATCH: usize = 512 * 1024;

/// Checked upper bound for simultaneously retained temporary PCM/encoded blocks
/// and onset/coarse windows. This is not resident backing or measured process RSS.
pub(crate) fn alignment_scratch_upper_bound(rate: u32, frames: usize) -> Result<usize, String> {
    let analysis_frames = (rate as f32 * STEM_ALIGNMENT_ANALYSIS_SECONDS).round() as usize;
    analysis_frames
        .min(frames)
        .checked_mul(16)
        .and_then(|n| n.checked_add(FIXED_SCRATCH))
        .ok_or_else(|| "stem alignment scratch overflow".into())
}

struct BufferedPcmHash {
    hash: Sha256,
    bytes: [u8; 64 * 1024],
    filled: usize,
}
impl BufferedPcmHash {
    fn new() -> Self {
        Self {
            hash: Sha256::new(),
            bytes: [0; 64 * 1024],
            filled: 0,
        }
    }
    fn word(&mut self, value: f32) {
        self.bytes[self.filled..self.filled + 4].copy_from_slice(&value.to_bits().to_le_bytes());
        self.filled += 4;
        if self.filled == self.bytes.len() {
            self.hash.update(&self.bytes);
            self.filled = 0;
        }
    }
    fn finalize(mut self) -> [u8; 32] {
        self.hash.update(&self.bytes[..self.filled]);
        self.hash.finalize().into()
    }
}

#[cfg(test)]
thread_local! {
    static ALLOCATION_FAULT: std::cell::Cell<(Option<usize>, usize)> = const { std::cell::Cell::new((None, 0)) };
}
#[cfg(test)]
pub(crate) fn set_allocation_failure_for_test(attempt: Option<usize>) {
    ALLOCATION_FAULT.with(|value| value.set((attempt, 0)));
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}

fn floats(length: usize) -> io::Result<Vec<f32>> {
    #[cfg(test)]
    if ALLOCATION_FAULT.with(|value| {
        let (failure, count) = value.get();
        value.set((failure, count + 1));
        failure == Some(count + 1)
    }) {
        return Err(invalid(
            "injected bounded PCM allocation failure before try_reserve_exact",
        ));
    }
    let mut values = Vec::new();
    values.try_reserve_exact(length).map_err(invalid)?;
    values.resize(length, 0.0);
    Ok(values)
}

fn check(cancelled: &dyn Fn() -> bool) -> io::Result<()> {
    if cancelled() {
        Err(invalid("complete stem conversion cancelled"))
    } else {
        Ok(())
    }
}

pub(crate) struct AlignedStemStream<'a> {
    files: &'a [File],
    wavs: [Pcm16Wav; 5],
    frames: usize,
    channels: usize,
    block_samples: usize,
    pub(crate) offset_frames: isize,
}

impl<'a> AlignedStemStream<'a> {
    /// The first pass establishes both global onset maxima and full source/mono
    /// integrity. The second finds the globally thresholded first attack. The
    /// third retains only the unchanged four-second correlation window.
    pub(crate) fn prepare(
        reader: &CompleteSourceReader,
        files: &'a [File],
        rate: u32,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, String> {
        Self::prepare_blocks(reader, files, rate, cancelled, BLOCK_SAMPLES)
    }

    #[cfg(test)]
    pub(crate) fn prepare_partitioned(
        reader: &CompleteSourceReader,
        files: &'a [File],
        rate: u32,
        cancelled: &dyn Fn() -> bool,
        block_frames: usize,
    ) -> Result<Self, String> {
        let samples = block_frames
            .checked_mul(reader.reference.channels)
            .ok_or("test block sample overflow")?;
        if samples == 0 || samples > BLOCK_SAMPLES {
            return Err("test block outside bounded streaming partition".into());
        }
        Self::prepare_blocks(reader, files, rate, cancelled, samples)
    }

    fn prepare_blocks(
        reader: &CompleteSourceReader,
        files: &'a [File],
        rate: u32,
        cancelled: &dyn Fn() -> bool,
        block_samples: usize,
    ) -> Result<Self, String> {
        let frames = reader.reference.frame_count();
        let channels = reader.reference.channels;
        if files.len() != 5 || frames == 0 || rate == 0 || channels == 0 || channels > 32 {
            return Err("invalid complete streaming stem geometry".into());
        }
        let mut layouts = Vec::with_capacity(5);
        for file in files {
            let layout = Pcm16Wav::inspect(file, cancelled).map_err(|e| e.to_string())?;
            layout.validate_geometry(rate, channels, frames)?;
            layouts.push(layout);
        }
        let wavs = layouts.try_into().map_err(|_| "incomplete stem set")?;
        let analysis_frames = (rate as f32 * STEM_ALIGNMENT_ANALYSIS_SECONDS).round() as usize;
        // Two onset windows plus their two worst-case coarse windows. Neither
        // complete disk extent nor already-owned live backing is scratch here.
        let peak = alignment_scratch_upper_bound(rate, frames)?;
        if peak > crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES {
            return Err("stem alignment analysis scratch exceeds transient PCM admission".into());
        }
        let mut stream = Self {
            files,
            wavs,
            frames,
            channels,
            block_samples,
            offset_frames: 0,
        };
        let mut source_hash = BufferedPcmHash::new();
        let mut mono_hash = BufferedPcmHash::new();
        let mut maxima = (0.0_f32, 0.0_f32);
        stream
            .visit_onsets(
                reader,
                0..frames,
                cancelled,
                |_, reference, candidate, values| {
                    maxima.0 = maxima.0.max(reference);
                    maxima.1 = maxima.1.max(candidate);
                    for value in values {
                        source_hash.word(*value);
                    }
                    let mean = (values.iter().map(|&value| f64::from(value)).sum::<f64>()
                        / channels as f64) as f32;
                    mono_hash.word(mean);
                    Ok(())
                },
            )
            .map_err(|e| e.to_string())?;
        if let Some(view) = &reader.reference.residency {
            let actual_source = source_hash.finalize();
            let actual_mono = mono_hash.finalize();
            if actual_source != view.source.playback_sha256
                || actual_mono != view.source.mono_sha256
            {
                return Err("complete alignment reference PCM/mono digest mismatch".into());
            }
        }
        if maxima.0 <= f32::EPSILON || maxima.1 <= f32::EPSILON {
            return Ok(stream);
        }
        let threshold = maxima.0 * STEM_ALIGNMENT_ONSET_THRESHOLD_RATIO;
        let mut onset = None;
        stream
            .visit_onsets(reader, 0..frames, cancelled, |frame, reference, _, _| {
                if onset.is_none() && reference >= threshold {
                    onset = Some(frame);
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        let pre_onset = (rate as f32 * STEM_ALIGNMENT_PRE_ONSET_SECONDS).round() as usize;
        let start = onset.unwrap_or(0).saturating_sub(pre_onset);
        let end = start.saturating_add(analysis_frames).min(frames);
        if end - start < 4 {
            return Ok(stream);
        }
        let mut reference = floats(end - start).map_err(|e| e.to_string())?;
        let mut candidate = floats(end - start).map_err(|e| e.to_string())?;
        stream
            .visit_onsets(
                reader,
                start.saturating_sub(1)..end,
                cancelled,
                |frame, a, b, _| {
                    if frame >= start {
                        reference[frame - start] = a;
                        candidate[frame - start] = b;
                    }
                    Ok(())
                },
            )
            .map_err(|e| e.to_string())?;
        stream.offset_frames = alignment_offset_in_window(&reference, &candidate, rate);
        check(cancelled).map_err(|e| e.to_string())?;
        Ok(stream)
    }

    fn visit_onsets(
        &self,
        reader: &CompleteSourceReader,
        region: std::ops::Range<usize>,
        cancelled: &dyn Fn() -> bool,
        mut visit: impl FnMut(usize, f32, f32, &[f32]) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut mixed = floats(self.block_samples)?;
        let mut component = floats(self.block_samples)?;
        let mut previous_reference = 0.0_f32;
        let mut previous_candidate = 0.0_f32;
        reader.visit_region(region, cancelled, |first, source| {
            check(cancelled)?;
            #[cfg(test)]
            crate::audio_engine::stem_pair::fail_at(
                crate::audio_engine::stem_pair::StemPairFault::AlignmentScan,
            )
            .map_err(invalid)?;
            let partition_samples = (self.block_samples / self.channels) * self.channels;
            for (part, source) in source.chunks(partition_samples).enumerate() {
                let first = first + part * (partition_samples / self.channels);
                mixed[..source.len()].fill(0.0);
                // Preserve f32 component accumulation in vocals, melody, bass,
                // drums order, including its rounding after each addition.
                for index in 0..4 {
                    self.wavs[index].read_frames(
                        self.files
                            .get(index)
                            .ok_or_else(|| invalid("missing stem"))?,
                        first,
                        &mut component[..source.len()],
                        cancelled,
                    )?;
                    for (target, value) in mixed[..source.len()]
                        .iter_mut()
                        .zip(&component[..source.len()])
                    {
                        *target += *value;
                    }
                }
                for (local, (reference, candidate)) in source
                    .chunks_exact(self.channels)
                    .zip(mixed[..source.len()].chunks_exact(self.channels))
                    .enumerate()
                {
                    if reference.iter().any(|n| !n.is_finite())
                        || candidate.iter().any(|n| !n.is_finite())
                    {
                        return Err(invalid("non-finite complete alignment PCM"));
                    }
                    let reference_envelope =
                        reference.iter().map(|n| n.abs()).sum::<f32>() / self.channels as f32;
                    let candidate_envelope =
                        candidate.iter().map(|n| n.abs()).sum::<f32>() / self.channels as f32;
                    let reference_onset = (reference_envelope - previous_reference).max(0.0);
                    let candidate_onset = (candidate_envelope - previous_candidate).max(0.0);
                    previous_reference = reference_envelope;
                    previous_candidate = candidate_envelope;
                    visit(first + local, reference_onset, candidate_onset, reference)?;
                }
            }
            Ok(())
        })
    }

    /// Full aligned conversion includes every zero-padded edge and exact tail.
    /// The same pass is used for prepublication identity and subsequent write.
    pub(crate) fn convert(
        &self,
        index: usize,
        output: &mut impl Write,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<(String, u64)> {
        let layout = self
            .wavs
            .get(index)
            .ok_or_else(|| invalid("stem index out of range"))?;
        let mut values = floats(BLOCK_SAMPLES)?;
        let mut encoded = [0_u8; BLOCK_SAMPLES * 4];
        let frames_per_block = self.block_samples / self.channels;
        let mut digest = Sha256::new();
        let mut bytes = 0_u64;
        for first in (0..self.frames).step_by(frames_per_block) {
            check(cancelled)?;
            let count = (self.frames - first).min(frames_per_block);
            let length = count * self.channels;
            values[..length].fill(0.0);
            let skip = if self.offset_frames < 0 {
                self.offset_frames
                    .unsigned_abs()
                    .saturating_sub(first)
                    .min(count)
            } else {
                0
            };
            if let Some(source_first) = (first + skip).checked_add_signed(self.offset_frames) {
                let available = self.frames.saturating_sub(source_first).min(count - skip);
                if available != 0 {
                    layout.read_frames(
                        &self.files[index],
                        source_first,
                        &mut values[skip * self.channels..(skip + available) * self.channels],
                        cancelled,
                    )?;
                }
            }
            for (value, word) in values[..length].iter().zip(encoded.chunks_exact_mut(4)) {
                word.copy_from_slice(&value.to_bits().to_le_bytes());
            }
            let block = &encoded[..length * 4];
            digest.update(block);
            #[cfg(test)]
            crate::audio_engine::stem_pair::fail_at(
                crate::audio_engine::stem_pair::StemPairFault::PcmConvert,
            )
            .map_err(invalid)?;
            output.write_all(block)?;
            bytes = bytes
                .checked_add(block.len() as u64)
                .ok_or_else(|| invalid("stem PCM extent overflow"))?;
            check(cancelled)?;
        }
        Ok((format!("{:x}", digest.finalize()), bytes))
    }
}
