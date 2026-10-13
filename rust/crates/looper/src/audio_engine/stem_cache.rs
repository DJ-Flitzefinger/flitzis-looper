//! Offline stem-cache artifact helpers.
//!
//! These helpers run only on background/control-plane threads. They must never be called from the
//! real-time audio callback.

use sha2::{Digest, Sha256};
#[cfg(test)]
use std::fs;
use std::fs::File;
#[cfg(test)]
use std::io::{self, Write};
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub(crate) mod streamed;
#[cfg(test)]
mod streamed_tests;
pub(crate) mod wav;

use crate::messages::{PreparedStemSet, STEM_BUFFER_COUNT, SampleBuffer};

pub(crate) const STEM_FILE_NAMES: [&str; 5] = ["vocals", "melody", "bass", "drums", "instrumental"];
#[cfg(test)]
const COMPONENT_STEM_COUNT: usize = 4;
const STEM_ALIGNMENT_MAX_SHIFT_SECONDS: f32 = 0.25;
const STEM_ALIGNMENT_ANALYSIS_SECONDS: f32 = 4.0;
const STEM_ALIGNMENT_PRE_ONSET_SECONDS: f32 = 0.5;
const STEM_ALIGNMENT_MIN_SCORE: f32 = 0.35;
const STEM_ALIGNMENT_MIN_IMPROVEMENT: f32 = 0.08;
const STEM_ALIGNMENT_ONSET_THRESHOLD_RATIO: f32 = 0.05;

/// Maximum simultaneous PCM includes complete reference, five retained stems,
/// WAV-to-f32/Arc overlap, shifted Vec-to-Arc overlap, two mono onset arrays and
/// bounded coarse analysis scratch. Existing pad/voice readers are separate owners.
pub(super) fn admitted_stem_pcm_bytes(frames: usize, channels: usize) -> Result<usize, String> {
    frames
        .checked_mul(channels)
        .and_then(|n| n.checked_mul(4))
        .and_then(|bytes| bytes.checked_mul(8))
        .and_then(|bytes| {
            frames
                .checked_mul(8)
                .and_then(|mono| bytes.checked_add(mono))
        })
        .and_then(|bytes| bytes.checked_add(4 * 1024 * 1024))
        .ok_or_else(|| "complete stem transient PCM geometry overflow".into())
}

pub(crate) fn project_stem_cache_dir(cache_dir: &str) -> Result<PathBuf, String> {
    let path = Path::new(cache_dir);
    if path.is_absolute() {
        return Err("stem cache directory must be project-relative".to_string());
    }

    let kind = super::material_paths::classify(path)
        .map_err(|error| format!("invalid stem cache directory: {error}"))?;
    if !matches!(kind, super::material_paths::AssetKind::StemDirectory { .. })
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".generation-"))
    {
        return Err(
            "stem cache directory must name a legacy set or an immutable ready generation".into(),
        );
    }
    Ok(path.to_owned())
}

#[cfg(test)]
fn write_deterministic_stem_artifacts_at_project_root<F>(
    sample: &SampleBuffer,
    output_sample_rate: u32,
    cache_dir: &str,
    project_root: &Path,
    progress: &mut F,
) -> Result<(), String>
where
    F: FnMut(f32, &'static str),
{
    let cache_dir = project_root.join(project_stem_cache_dir(cache_dir)?);
    validate_sample_buffer(sample, output_sample_rate)?;

    fs::create_dir_all(&cache_dir)
        .map_err(|err| format!("Failed to create stem cache directory: {err}"))?;

    for (index, stem_name) in STEM_FILE_NAMES.iter().enumerate() {
        let percent = index as f32 / STEM_FILE_NAMES.len() as f32;
        progress(percent, "Writing stem cache");

        let target_path = cache_dir.join(format!("{stem_name}.wav"));
        let temp_path = cache_dir.join(format!("{stem_name}.wav.tmp"));
        let silent = *stem_name != "instrumental";

        write_pcm16_wav(
            &temp_path,
            sample.channels,
            output_sample_rate,
            &sample.samples,
            silent,
        )
        .map_err(|err| format!("Failed to write {stem_name} stem artifact: {err}"))?;

        if target_path.exists() {
            fs::remove_file(&target_path)
                .map_err(|err| format!("Failed to replace {stem_name} stem artifact: {err}"))?;
        }
        fs::rename(&temp_path, &target_path)
            .map_err(|err| format!("Failed to finalize {stem_name} stem artifact: {err}"))?;
    }

    progress(1.0, "Stem cache ready");
    Ok(())
}

pub(crate) fn prepare_stem_buffers_from_cache(
    source_version: &str,
    reference: &SampleBuffer,
    output_sample_rate: u32,
    cache_dir: &str,
) -> Result<PreparedStemSet, String> {
    prepare_stem_buffers_from_cache_at_project_root(
        source_version,
        reference,
        output_sample_rate,
        cache_dir,
        Path::new("."),
    )
}

pub(super) fn prepare_stem_buffers_from_cache_at_project_root(
    source_version: &str,
    reference: &SampleBuffer,
    output_sample_rate: u32,
    cache_dir: &str,
    project_root: &Path,
) -> Result<PreparedStemSet, String> {
    if source_version.trim().is_empty() {
        return Err("source_version must not be empty".to_string());
    }

    validate_sample_buffer(reference, output_sample_rate)?;
    let expected_frames = reference.frame_count();
    // This legacy API explicitly requests complete resident component views.
    // Complete immutable pair preparation uses a separate streaming reader and
    // never makes this allocation/admission dependent on disk source extent.
    if admitted_stem_pcm_bytes(expected_frames, reference.channels)?
        > super::cold_jobs::PCM_LIMIT_BYTES
    {
        return Err("complete resident stem views exceed transient PCM admission".into());
    }
    let reader = super::complete_context::CompleteSourceReader::new(reference.clone(), None)?;
    let path = project_root.join(project_stem_cache_dir(cache_dir)?);
    let files = STEM_FILE_NAMES
        .iter()
        .map(|name| {
            super::cold_store::sealed_reader(&path.join(format!("{name}.wav")))
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let complete =
        streamed::AlignedStemStream::prepare(&reader, &files, output_sample_rate, &|| false)?;
    let mut identity = Sha256::new();
    identity.update(b"aligned-complete-stem-set-v1");
    identity.update(source_version.as_bytes());
    identity.update(output_sample_rate.to_le_bytes());
    identity.update((expected_frames as u64).to_le_bytes());
    struct IdentityWriter<'a>(&'a mut Sha256);
    impl std::io::Write for IdentityWriter<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    for index in 0..5 {
        complete
            .convert(index, &mut IdentityWriter(&mut identity), &|| false)
            .map_err(|e| e.to_string())?;
    }
    struct SamplesWriter(Vec<f32>);
    impl std::io::Write for SamplesWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if !bytes.len().is_multiple_of(4) {
                return Err(std::io::Error::other("unaligned converted PCM block"));
            }
            self.0.extend(
                bytes
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes(b.try_into().unwrap())),
            );
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let length = expected_frames
        .checked_mul(reference.channels)
        .ok_or("resident stem extent overflow")?;
    let mut buffers = Vec::with_capacity(4);
    for index in 0..4 {
        let mut values = Vec::new();
        values
            .try_reserve_exact(length)
            .map_err(|e| e.to_string())?;
        let mut output = SamplesWriter(values);
        complete
            .convert(index, &mut output, &|| false)
            .map_err(|e| e.to_string())?;
        buffers.push(SampleBuffer {
            residency: None,
            channels: reference.channels,
            samples: output.0.into_boxed_slice().into(),
        });
    }
    let stems = buffers.try_into().map_err(|_| "component set incomplete")?;
    Ok(PreparedStemSet {
        complete_set_identity: std::sync::Arc::new(identity.finalize().into()),
        accepted_timing: None,
        reference_samples: reference.samples.clone(),
        reference_residency: reference.residency.clone(),
        publication: super::prepared_source::PreparedSourcePermit::unbound(),
        source_version_hash: source_version_hash(source_version),
        sample_rate_hz: output_sample_rate,
        channels: reference.channels,
        frame_count: expected_frames,
        available_mask: ((1_u16 << STEM_BUFFER_COUNT) - 1) as u8,
        stems,
    })
}

/// Complete five-artifact conversion result; offset uses loaded/output-rate frames.
/// Temporary instrumental PCM belongs here rather than in an offline-only descriptor.
#[cfg(test)]
pub(super) struct CompleteAlignedStems {
    pub stems: [SampleBuffer; 5],
    pub offset_frames: isize,
}

/// Execute the existing exact-geometry PCM16 conversion and one shared alignment.
#[cfg(test)]
pub(super) fn prepare_complete_stems_at_project_root(
    reference: &SampleBuffer,
    output_sample_rate: u32,
    cache_dir: &str,
    project_root: &Path,
) -> Result<CompleteAlignedStems, String> {
    validate_sample_buffer(reference, output_sample_rate)?;
    let expected_frames = reference.frame_count();
    if admitted_stem_pcm_bytes(expected_frames, reference.channels)?
        > super::cold_jobs::PCM_LIMIT_BYTES
    {
        return Err("complete stem preparation exceeds the 1-GiB transient PCM admission".into());
    }
    if reference.resident_start() != 0 || reference.resident_end() != expected_frames {
        return Err("complete reference PCM is required for stem alignment".into());
    }
    if expected_frames == 0 {
        return Err("reference sample must contain at least one frame".to_string());
    }

    let cache_dir = project_root.join(project_stem_cache_dir(cache_dir)?);
    let mut buffers = Vec::with_capacity(STEM_FILE_NAMES.len());

    for stem_name in STEM_FILE_NAMES {
        let path = cache_dir.join(format!("{stem_name}.wav"));
        let buffer = read_aligned_pcm16_wav(
            &path,
            output_sample_rate,
            reference.channels,
            expected_frames,
        )
        .map_err(|err| format!("Invalid {stem_name} stem artifact: {err}"))?;
        buffers.push(buffer);
    }

    let offset_frames = align_component_stems_to_reference(
        reference,
        &mut buffers,
        output_sample_rate,
        expected_frames,
    );

    let stems: [SampleBuffer; 5] = buffers
        .try_into()
        .map_err(|_| "stem set is incomplete".to_string())?;

    Ok(CompleteAlignedStems {
        stems,
        offset_frames,
    })
}

#[cfg(test)]
fn align_component_stems_to_reference(
    reference: &SampleBuffer,
    buffers: &mut [SampleBuffer],
    output_sample_rate: u32,
    expected_frames: usize,
) -> isize {
    if buffers.len() != STEM_FILE_NAMES.len() || reference.channels == 0 || expected_frames == 0 {
        return 0;
    }

    let frame_offset =
        detected_stem_alignment_offset(reference, buffers, output_sample_rate, expected_frames);
    if frame_offset == 0 {
        return 0;
    }

    for buffer in buffers {
        shift_sample_buffer(buffer, expected_frames, frame_offset);
    }
    frame_offset
}

#[cfg(test)]
fn detected_stem_alignment_offset(
    reference: &SampleBuffer,
    buffers: &[SampleBuffer],
    output_sample_rate: u32,
    expected_frames: usize,
) -> isize {
    let channels = reference.channels;
    if channels == 0 || buffers.len() < COMPONENT_STEM_COUNT {
        return 0;
    }

    let reference_onset = onset_signal(reference.samples.as_ref(), channels, expected_frames);
    let component_mix = mixed_component_stem_samples(buffers, channels, expected_frames);
    let component_onset = onset_signal(&component_mix, channels, expected_frames);
    best_alignment_offset(
        &reference_onset,
        &component_onset,
        output_sample_rate,
        expected_frames,
    )
}

#[cfg(test)]
fn mixed_component_stem_samples(
    buffers: &[SampleBuffer],
    channels: usize,
    expected_frames: usize,
) -> Vec<f32> {
    let expected_samples = expected_frames.saturating_mul(channels);
    let mut mixed = vec![0.0; expected_samples];
    for buffer in buffers.iter().take(COMPONENT_STEM_COUNT) {
        if buffer.channels != channels || buffer.samples.len() != expected_samples {
            continue;
        }
        for (mixed_sample, stem_sample) in mixed.iter_mut().zip(buffer.samples.iter()) {
            *mixed_sample += *stem_sample;
        }
    }
    mixed
}

#[cfg(test)]
fn onset_signal(samples: &[f32], channels: usize, frames: usize) -> Vec<f32> {
    let mut signal = vec![0.0; frames];
    if channels == 0 {
        return signal;
    }

    let usable_frames = frames.min(samples.len() / channels);
    let mut previous = 0.0_f32;
    for (frame, signal_value) in signal.iter_mut().enumerate().take(usable_frames) {
        let start = frame * channels;
        let stop = start + channels;
        let envelope = samples[start..stop]
            .iter()
            .map(|sample| sample.abs())
            .sum::<f32>()
            / channels as f32;
        *signal_value = (envelope - previous).max(0.0);
        previous = envelope;
    }
    signal
}

#[cfg(test)]
fn best_alignment_offset(
    reference: &[f32],
    candidate: &[f32],
    output_sample_rate: u32,
    expected_frames: usize,
) -> isize {
    let max_reference = reference.iter().copied().fold(0.0_f32, f32::max);
    let max_candidate = candidate.iter().copied().fold(0.0_f32, f32::max);
    if max_reference <= f32::EPSILON || max_candidate <= f32::EPSILON {
        return 0;
    }

    let analysis = alignment_analysis_range(reference, output_sample_rate, expected_frames);
    if analysis.len() < 4 {
        return 0;
    }

    let max_shift_frames = ((output_sample_rate as f32 * STEM_ALIGNMENT_MAX_SHIFT_SECONDS).round()
        as usize)
        .min(analysis.len().saturating_sub(1));
    if max_shift_frames == 0 {
        return 0;
    }

    alignment_offset_in_window(
        &reference[analysis.start..analysis.end],
        &candidate[analysis.start..analysis.end],
        output_sample_rate,
    )
}

/// Shared legacy correlation oracle, now also used by the bounded global scan.
fn alignment_offset_in_window(
    reference_window: &[f32],
    candidate_window: &[f32],
    output_sample_rate: u32,
) -> isize {
    let max_shift_frames = ((output_sample_rate as f32 * STEM_ALIGNMENT_MAX_SHIFT_SECONDS).round()
        as usize)
        .min(reference_window.len().saturating_sub(1));
    if max_shift_frames == 0 {
        return 0;
    }
    let coarse_hop = (output_sample_rate as usize / 4_000).clamp(1, 64);
    let coarse_reference = downsample_max(reference_window, coarse_hop);
    let coarse_candidate = downsample_max(candidate_window, coarse_hop);
    let coarse_lag = best_lag_in_range(
        &coarse_reference,
        &coarse_candidate,
        -((max_shift_frames / coarse_hop) as isize),
        (max_shift_frames / coarse_hop) as isize,
    )
    .lag;

    let coarse_frame_lag = coarse_lag * coarse_hop as isize;
    let refine_radius = (coarse_hop as isize).saturating_mul(2).max(1);
    let lower = (coarse_frame_lag - refine_radius).max(-(max_shift_frames as isize));
    let upper = (coarse_frame_lag + refine_radius).min(max_shift_frames as isize);
    let best = best_lag_in_range(reference_window, candidate_window, lower, upper);
    let zero_score = correlation_for_lag(reference_window, candidate_window, 0);

    if best.lag == 0
        || best.score < STEM_ALIGNMENT_MIN_SCORE
        || best.score < zero_score + STEM_ALIGNMENT_MIN_IMPROVEMENT
    {
        return 0;
    }

    best.lag
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
struct AlignmentAnalysisRange {
    start: usize,
    end: usize,
}

#[cfg(test)]
impl AlignmentAnalysisRange {
    fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }
}

#[cfg(test)]
fn alignment_analysis_range(
    reference: &[f32],
    output_sample_rate: u32,
    expected_frames: usize,
) -> AlignmentAnalysisRange {
    let max_reference = reference.iter().copied().fold(0.0_f32, f32::max);
    let threshold = max_reference * STEM_ALIGNMENT_ONSET_THRESHOLD_RATIO;
    let onset_frame = reference
        .iter()
        .position(|value| *value >= threshold)
        .unwrap_or(0);
    let pre_onset_frames =
        (output_sample_rate as f32 * STEM_ALIGNMENT_PRE_ONSET_SECONDS).round() as usize;
    let analysis_frames =
        (output_sample_rate as f32 * STEM_ALIGNMENT_ANALYSIS_SECONDS).round() as usize;
    let start = onset_frame.saturating_sub(pre_onset_frames);
    let end = start.saturating_add(analysis_frames).min(expected_frames);
    AlignmentAnalysisRange { start, end }
}

#[derive(Debug, Clone, Copy)]
struct LagScore {
    lag: isize,
    score: f32,
}

fn best_lag_in_range(reference: &[f32], candidate: &[f32], lower: isize, upper: isize) -> LagScore {
    let mut best = LagScore {
        lag: 0,
        score: f32::NEG_INFINITY,
    };
    for lag in lower..=upper {
        let score = correlation_for_lag(reference, candidate, lag);
        if score > best.score {
            best = LagScore { lag, score };
        }
    }
    best
}

fn correlation_for_lag(reference: &[f32], candidate: &[f32], lag: isize) -> f32 {
    let len = reference.len().min(candidate.len());
    if len == 0 || lag.unsigned_abs() >= len {
        return 0.0;
    }

    let (reference_start, candidate_start, count) = if lag >= 0 {
        (0, lag as usize, len - lag as usize)
    } else {
        ((-lag) as usize, 0, len - (-lag) as usize)
    };
    if count == 0 {
        return 0.0;
    }

    let mut dot = 0.0_f32;
    let mut reference_energy = 0.0_f32;
    let mut candidate_energy = 0.0_f32;
    for offset in 0..count {
        let reference_value = reference[reference_start + offset];
        let candidate_value = candidate[candidate_start + offset];
        dot += reference_value * candidate_value;
        reference_energy += reference_value * reference_value;
        candidate_energy += candidate_value * candidate_value;
    }

    let denominator = (reference_energy * candidate_energy).sqrt();
    if denominator <= f32::EPSILON {
        return 0.0;
    }
    dot / denominator
}

fn downsample_max(signal: &[f32], hop: usize) -> Vec<f32> {
    let hop = hop.max(1);
    signal
        .chunks(hop)
        .map(|chunk| chunk.iter().copied().fold(0.0_f32, f32::max))
        .collect()
}

#[cfg(test)]
fn shift_sample_buffer(buffer: &mut SampleBuffer, expected_frames: usize, frame_offset: isize) {
    let channels = buffer.channels;
    if channels == 0 || buffer.samples.len() != expected_frames.saturating_mul(channels) {
        return;
    }

    let mut shifted = vec![0.0; buffer.samples.len()];
    for frame in 0..expected_frames {
        let source_frame = frame as isize + frame_offset;
        if !(0..expected_frames as isize).contains(&source_frame) {
            continue;
        }

        let source_start = source_frame as usize * channels;
        let target_start = frame * channels;
        shifted[target_start..target_start + channels]
            .copy_from_slice(&buffer.samples[source_start..source_start + channels]);
    }
    buffer.samples = shifted.into_boxed_slice().into();
}

pub(crate) fn source_version_hash(source_version: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in source_version.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn validate_sample_buffer(sample: &SampleBuffer, output_sample_rate: u32) -> Result<(), String> {
    if output_sample_rate == 0 {
        return Err("output sample rate must be non-zero".to_string());
    }
    if sample.channels == 0 {
        return Err("sample channel count must be non-zero".to_string());
    }
    if sample.samples.is_empty() {
        return Err("sample buffer must not be empty".to_string());
    }
    if !sample.samples.len().is_multiple_of(sample.channels) {
        return Err("sample buffer length must align to channel count".to_string());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WavFormat {
    pub(crate) channels: usize,
    pub(crate) sample_rate_hz: u32,
    pub(crate) block_align: usize,
}

#[cfg(test)]
fn read_aligned_pcm16_wav(
    path: &Path,
    expected_sample_rate_hz: u32,
    expected_channels: usize,
    expected_frames: usize,
) -> Result<SampleBuffer, String> {
    let reader = File::open(path).map_err(|err| format!("Failed to open WAV file: {err}"))?;
    let layout = wav::Pcm16Wav::inspect(&reader, &|| false).map_err(|e| e.to_string())?;
    layout.validate_geometry(expected_sample_rate_hz, expected_channels, expected_frames)?;
    let length = expected_frames
        .checked_mul(expected_channels)
        .ok_or("stem sample overflow")?;
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(length)
        .map_err(|e| e.to_string())?;
    samples.resize(length, 0.0);
    layout
        .read_frames(&reader, 0, &mut samples, &|| false)
        .map_err(|e| e.to_string())?;
    Ok(SampleBuffer {
        residency: None,
        channels: expected_channels,
        samples: samples.into_boxed_slice().into(),
    })
}

/// Check the same decoder geometry on a held sealed reader without conversion/alignment.
pub(super) fn verify_pcm16_wav_geometry_cancellable(
    reader: &mut File,
    sample_rate_hz: u32,
    channels: usize,
    frames: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<(), String> {
    let layout = wav::Pcm16Wav::inspect(reader, cancelled).map_err(|e| e.to_string())?;
    layout.validate_geometry(sample_rate_hz, channels, frames)?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn parse_pcm16_wav_format(bytes: &[u8]) -> Result<WavFormat, String> {
    if bytes.len() < 16 {
        return Err("fmt chunk is too short".to_string());
    }

    let audio_format = parse_le_u16(bytes, 0).ok_or_else(|| "missing audio format".to_string())?;
    if audio_format != 1 && audio_format != 0xfffe {
        return Err("only PCM WAV stem artifacts are supported".to_string());
    }
    if audio_format == 0xfffe {
        let pcm_guid = [
            1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71,
        ];
        if bytes.len() < 40
            || parse_le_u16(bytes, 16).is_none_or(|length| length < 22)
            || parse_le_u16(bytes, 18) != Some(16)
            || bytes[24..40] != pcm_guid
        {
            return Err("only 16-bit extensible PCM WAV stem artifacts are supported".into());
        }
    }

    let channels = parse_le_u16(bytes, 2).ok_or_else(|| "missing channel count".to_string())?;
    let sample_rate_hz = parse_le_u32(bytes, 4).ok_or_else(|| "missing sample rate".to_string())?;
    let block_align = parse_le_u16(bytes, 12).ok_or_else(|| "missing block align".to_string())?;
    let bits_per_sample =
        parse_le_u16(bytes, 14).ok_or_else(|| "missing bits per sample".to_string())?;

    if channels == 0 {
        return Err("channel count must be non-zero".to_string());
    }
    if sample_rate_hz == 0 {
        return Err("sample rate must be non-zero".to_string());
    }
    if bits_per_sample != 16 {
        return Err("only 16-bit PCM WAV stem artifacts are supported".to_string());
    }

    let expected_block_align = channels
        .checked_mul(bits_per_sample / 8)
        .ok_or_else(|| "invalid block align".to_string())?;
    if block_align != expected_block_align {
        return Err("block align does not match channel layout".to_string());
    }

    Ok(WavFormat {
        channels: usize::from(channels),
        sample_rate_hz,
        block_align: usize::from(block_align),
    })
}

fn parse_le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn parse_le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[cfg(test)]
fn write_pcm16_wav(
    path: &Path,
    channels: usize,
    sample_rate_hz: u32,
    samples: &[f32],
    silent: bool,
) -> io::Result<()> {
    let channels = u16::try_from(channels)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many channels"))?;
    let bits_per_sample = 16u16;
    let bytes_per_sample = bits_per_sample / 8;
    let block_align = channels
        .checked_mul(bytes_per_sample)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid block align"))?;
    let byte_rate = sample_rate_hz
        .checked_mul(u32::from(block_align))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid byte rate"))?;
    let data_len_bytes = u32::try_from(samples.len().saturating_mul(usize::from(bytes_per_sample)))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "stem artifact too large"))?;
    let chunk_size = 36u32
        .checked_add(data_len_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "stem artifact too large"))?;

    let mut file = File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&chunk_size.to_le_bytes())?;
    file.write_all(b"WAVE")?;
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&channels.to_le_bytes())?;
    file.write_all(&sample_rate_hz.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&bits_per_sample.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_len_bytes.to_le_bytes())?;

    for sample in samples {
        let value = if silent { 0 } else { float_to_pcm16(*sample) };
        file.write_all(&value.to_le_bytes())?;
    }

    Ok(())
}

fn pcm16_to_float(sample: i16) -> f32 {
    if sample == i16::MIN {
        -1.0
    } else {
        f32::from(sample) / f32::from(i16::MAX)
    }
}

#[cfg(test)]
fn float_to_pcm16(sample: f32) -> i16 {
    let sample = if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    };

    if sample >= 1.0 {
        i16::MAX
    } else if sample <= -1.0 {
        i16::MIN
    } else {
        (sample * f32::from(i16::MAX)).round() as i16
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn read_le_u16(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
    }

    fn read_le_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ])
    }

    #[test]
    fn project_stem_cache_dir_accepts_project_local_stem_path() {
        let path = project_stem_cache_dir("samples/stems/abcdef0123456789").unwrap();

        assert!(path.ends_with(Path::new("samples/stems/abcdef0123456789")));
        assert_eq!(
            project_stem_cache_dir("samples/stems/#1").unwrap(),
            Path::new("samples/stems/#1")
        );
    }

    #[test]
    fn project_stem_cache_dir_accepts_only_published_pad_generations() {
        for pad in [1, 216] {
            let cache_dir = format!("samples/stems/#{pad}/.ready-0123456789abcdef0123456789abcdef");
            assert_eq!(
                project_stem_cache_dir(&cache_dir).unwrap(),
                Path::new(&cache_dir)
            );
        }
        for cache_dir in [
            "samples/stems/#1/.generation-0123456789abcdef0123456789abcdef",
            "samples/stems/cache/.ready-0123456789abcdef0123456789abcdef",
            "samples/stems/#0/.ready-0123456789abcdef0123456789abcdef",
            "samples/stems/#217/.ready-0123456789abcdef0123456789abcdef",
            "samples/stems/#1/.ready-0123456789abcdef0123456789abcde",
            "samples/stems/#1/.ready-0123456789abcdef0123456789abcdeg",
            "samples/stems/#1/.ready-0123456789ABCDEF0123456789ABCDEF",
            "samples/stems/#1/.ready-0123456789abcdef0123456789abcdef/extra",
            "samples/stems/#1/../.ready-0123456789abcdef0123456789abcdef",
        ] {
            assert!(project_stem_cache_dir(cache_dir).is_err(), "{cache_dir}");
        }
    }

    #[test]
    fn project_stem_cache_dir_rejects_paths_outside_project_stems() {
        assert!(project_stem_cache_dir("../samples/stems/cache").is_err());
        assert!(project_stem_cache_dir("samples/../stems/cache").is_err());
        assert!(project_stem_cache_dir("samples/cache").is_err());
        assert!(project_stem_cache_dir("samples/stems").is_err());

        let absolute = std::env::temp_dir()
            .join("samples")
            .join("stems")
            .join("cache");
        assert!(project_stem_cache_dir(&absolute.to_string_lossy()).is_err());
    }

    #[test]
    fn write_deterministic_stem_artifacts_creates_aligned_wav_files() {
        let tmp = tempfile::tempdir().unwrap();
        let sample = SampleBuffer {
            residency: None,
            channels: 2,
            samples: Arc::from([0.5_f32, -0.5, 1.5, -1.5].as_slice()),
        };
        let mut progress = Vec::new();
        let mut record_progress = |percent, stage| progress.push((percent, stage));

        write_deterministic_stem_artifacts_at_project_root(
            &sample,
            48_000,
            "samples/stems/cache",
            tmp.path(),
            &mut record_progress,
        )
        .unwrap();

        assert_eq!(progress.last(), Some(&(1.0, "Stem cache ready")));

        for stem_name in STEM_FILE_NAMES {
            let path = tmp
                .path()
                .join("samples")
                .join("stems")
                .join("cache")
                .join(format!("{stem_name}.wav"));
            let bytes = fs::read(path).unwrap();

            assert_eq!(&bytes[0..4], b"RIFF");
            assert_eq!(&bytes[8..12], b"WAVE");
            assert_eq!(read_le_u16(&bytes, 22), 2);
            assert_eq!(read_le_u32(&bytes, 24), 48_000);
            assert_eq!(read_le_u32(&bytes, 40), 8);

            let samples: Vec<i16> = bytes[44..]
                .chunks_exact(2)
                .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            if stem_name == "instrumental" {
                assert_eq!(samples, vec![16_384, -16_384, i16::MAX, i16::MIN]);
            } else {
                assert_eq!(samples, vec![0, 0, 0, 0]);
            }
        }
    }

    #[test]
    fn prepare_stem_buffers_from_cache_validates_and_loads_aligned_wavs() {
        let tmp = tempfile::tempdir().unwrap();
        let sample = SampleBuffer {
            residency: None,
            channels: 2,
            samples: Arc::from([0.5_f32, -0.5, 0.25, -0.25].as_slice()),
        };

        write_deterministic_stem_artifacts_at_project_root(
            &sample,
            48_000,
            "samples/stems/cache",
            tmp.path(),
            &mut |_, _| {},
        )
        .unwrap();

        let prepared = prepare_stem_buffers_from_cache_at_project_root(
            "samples/loop.wav|4|10",
            &sample,
            48_000,
            "samples/stems/cache",
            tmp.path(),
        )
        .unwrap();

        assert_eq!(prepared.sample_rate_hz, 48_000);
        assert_eq!(prepared.channels, 2);
        assert_eq!(prepared.frame_count, 2);
        assert_eq!(prepared.available_mask, 0b1111);
        assert!(prepared.source_version_hash != 0);
        for stem in prepared.stems {
            assert_eq!(stem.channels, 2);
            assert_eq!(stem.samples.len(), 4);
        }
    }

    #[test]
    fn prepare_stem_buffers_aligns_delayed_stems_to_reference_origin() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("samples").join("stems").join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let mut reference_samples = vec![0.0_f32; 64];
        reference_samples[8] = 1.0;
        reference_samples[24] = 0.5;
        let sample = SampleBuffer {
            residency: None,
            channels: 1,
            samples: reference_samples.into_boxed_slice().into(),
        };

        let mut delayed_drums = vec![0.0_f32; 64];
        delayed_drums[13] = 1.0;
        delayed_drums[29] = 0.5;
        for stem_name in ["vocals", "melody", "bass"] {
            write_pcm16_wav(
                &cache_dir.join(format!("{stem_name}.wav")),
                1,
                48_000,
                &[0.0; 64],
                false,
            )
            .unwrap();
        }
        write_pcm16_wav(
            &cache_dir.join("drums.wav"),
            1,
            48_000,
            &delayed_drums,
            false,
        )
        .unwrap();
        write_pcm16_wav(
            &cache_dir.join("instrumental.wav"),
            1,
            48_000,
            &delayed_drums,
            false,
        )
        .unwrap();

        let prepared = prepare_complete_stems_at_project_root(
            &sample,
            48_000,
            "samples/stems/cache",
            tmp.path(),
        )
        .unwrap();

        let drums = &prepared.stems[3].samples;
        assert_eq!(drums[8], 1.0);
        assert_eq!(drums[13], 0.0);
        assert!((drums[24] - 0.5).abs() < 0.0001);
        let instrumental = &prepared.stems[4].samples;
        assert_eq!(instrumental[8], 1.0);
        assert_eq!(instrumental[13], 0.0);
    }

    #[test]
    fn prepare_stem_buffers_rejects_sample_rate_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let sample = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from([0.5_f32, -0.5].as_slice()),
        };

        write_deterministic_stem_artifacts_at_project_root(
            &sample,
            48_000,
            "samples/stems/cache",
            tmp.path(),
            &mut |_, _| {},
        )
        .unwrap();
        let vocals = tmp
            .path()
            .join("samples")
            .join("stems")
            .join("cache")
            .join("vocals.wav");
        write_pcm16_wav(&vocals, 1, 44_100, &sample.samples, true).unwrap();

        let error = prepare_stem_buffers_from_cache_at_project_root(
            "samples/loop.wav|2|10",
            &sample,
            48_000,
            "samples/stems/cache",
            tmp.path(),
        )
        .unwrap_err();

        assert!(error.contains("sample rate mismatch"));
    }

    #[test]
    fn prepare_stem_buffers_rejects_frame_count_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let sample = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from([0.5_f32, -0.5, 0.25].as_slice()),
        };

        write_deterministic_stem_artifacts_at_project_root(
            &sample,
            48_000,
            "samples/stems/cache",
            tmp.path(),
            &mut |_, _| {},
        )
        .unwrap();
        let vocals = tmp
            .path()
            .join("samples")
            .join("stems")
            .join("cache")
            .join("vocals.wav");
        write_pcm16_wav(&vocals, 1, 48_000, &[0.0_f32, 0.0], true).unwrap();

        let error = prepare_stem_buffers_from_cache_at_project_root(
            "samples/loop.wav|3|10",
            &sample,
            48_000,
            "samples/stems/cache",
            tmp.path(),
        )
        .unwrap_err();

        assert!(error.contains("frame count mismatch"));
    }
}
