//! Independent bounded Boris evidence, never used by product code.
//!
//! Alignment is transcribed from the published 9a4b64d stem_cache.rs legacy
//! full-array algorithm (same at e9da7e1), including f32 arithmetic, global
//! onset threshold, coarse/refine search and strict ascending-lag tie rule.
//! RIFF44 PCM16 parsing/conversion here does not call the productive WAV
//! parser, streamed converter or alignment helpers. The Boris artifacts have
//! this independently observed canonical small-RIFF geometry. Large RF64
//! productive interoperability is covered by the separate long fixtures.

use crate::audio_engine::cold_store::sealed_reader;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io;
use std::path::Path;

const BLOCK_SAMPLES: usize = 16 * 1024;
const STEM_NAMES: [&str; 4] = ["vocals", "melody", "bass", "drums"];
const MAX_SHIFT_SECONDS: f32 = 0.25;
const ANALYSIS_SECONDS: f32 = 4.0;
const PRE_ONSET_SECONDS: f32 = 0.5;
const MIN_SCORE: f32 = 0.35;
const MIN_IMPROVEMENT: f32 = 0.08;
const THRESHOLD_RATIO: f32 = 0.05;

fn read_some(file: &File, bytes: &mut [u8], offset: u64) -> io::Result<usize> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        file.seek_read(bytes, offset)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_at(bytes, offset)
    }
}

fn read_exact_at(file: &File, mut bytes: &mut [u8], mut offset: u64) {
    while !bytes.is_empty() {
        let count = read_some(file, bytes, offset).unwrap();
        assert!(count != 0, "independent oracle unexpected EOF at {offset}");
        offset = offset.checked_add(count as u64).unwrap();
        bytes = &mut bytes[count..];
    }
}

fn exact_eof(file: &File, extent: u64) {
    assert_eq!(file.metadata().unwrap().len(), extent);
    assert_eq!(read_some(file, &mut [0], extent).unwrap(), 0);
}

struct Riff16 {
    file: File,
    channels: usize,
    rate: u32,
    frames: usize,
}

impl Riff16 {
    fn open(path: &Path, expected_frames: usize) -> Self {
        let file = sealed_reader(path).unwrap();
        let mut header = [0; 44];
        read_exact_at(&file, &mut header, 0);
        assert_eq!(&header[..4], b"RIFF");
        assert_eq!(&header[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(header[16..20].try_into().unwrap()), 16);
        assert_eq!(u16::from_le_bytes(header[20..22].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(header[34..36].try_into().unwrap()), 16);
        assert_eq!(&header[36..40], b"data");
        let channels = usize::from(u16::from_le_bytes(header[22..24].try_into().unwrap()));
        let rate = u32::from_le_bytes(header[24..28].try_into().unwrap());
        assert!((1..=32).contains(&channels) && rate > 0);
        assert_eq!(
            u16::from_le_bytes(header[32..34].try_into().unwrap()) as usize,
            channels * 2
        );
        assert_eq!(
            u32::from_le_bytes(header[28..32].try_into().unwrap()),
            rate * channels as u32 * 2
        );
        let data = u64::from(u32::from_le_bytes(header[40..44].try_into().unwrap()));
        assert_eq!(data, expected_frames as u64 * channels as u64 * 2);
        assert_eq!(
            u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap())) + 8,
            data + 44
        );
        exact_eof(&file, data + 44);
        Self {
            file,
            channels,
            rate,
            frames: expected_frames,
        }
    }

    fn read_frames(&self, first: usize, values: &mut [f32]) {
        assert!(values.len().is_multiple_of(self.channels));
        assert!(first + values.len() / self.channels <= self.frames);
        let mut bytes = [0_u8; BLOCK_SAMPLES * 2];
        assert!(values.len() * 2 <= bytes.len());
        read_exact_at(
            &self.file,
            &mut bytes[..values.len() * 2],
            44 + first as u64 * self.channels as u64 * 2,
        );
        for (value, pair) in values.iter_mut().zip(bytes.chunks_exact(2)) {
            let word = i16::from_le_bytes(pair.try_into().unwrap());
            *value = if word == i16::MIN {
                -1.0
            } else {
                f32::from(word) / 32767.0_f32
            };
        }
    }
}

fn read_f32_frames(file: &File, first: usize, channels: usize, values: &mut [f32]) {
    let mut bytes = [0_u8; BLOCK_SAMPLES * 4];
    assert!(values.len().is_multiple_of(channels) && values.len() * 4 <= bytes.len());
    read_exact_at(
        file,
        &mut bytes[..values.len() * 4],
        first as u64 * channels as u64 * 4,
    );
    for (value, word) in values.iter_mut().zip(bytes.chunks_exact(4)) {
        *value = f32::from_le_bytes(word.try_into().unwrap());
        assert!(value.is_finite(), "nonfinite independent complete PCM");
    }
}

fn abs_mean(frame: &[f32]) -> f32 {
    frame.iter().map(|sample| sample.abs()).sum::<f32>() / frame.len() as f32
}

fn visit_onsets(
    source: &File,
    stems: &[Riff16],
    region: std::ops::Range<usize>,
    mut visit: impl FnMut(usize, f32, f32),
) {
    let channels = stems[0].channels;
    let mut reference = vec![0.0_f32; BLOCK_SAMPLES];
    let mut mixed = vec![0.0_f32; BLOCK_SAMPLES];
    let mut component = vec![0.0_f32; BLOCK_SAMPLES];
    let mut previous_reference = 0.0_f32;
    let mut previous_candidate = 0.0_f32;
    let chunk_frames = BLOCK_SAMPLES / channels;
    for first in (region.start..region.end).step_by(chunk_frames) {
        let samples = (region.end - first).min(chunk_frames) * channels;
        read_f32_frames(source, first, channels, &mut reference[..samples]);
        mixed[..samples].fill(0.0);
        // The frozen algorithm accumulates components in this exact order,
        // rounding each f32 addition; it never averages them or uses f64 mixing.
        for stem in stems {
            stem.read_frames(first, &mut component[..samples]);
            for (sum, value) in mixed[..samples].iter_mut().zip(&component[..samples]) {
                *sum += *value;
            }
        }
        for (local, (source_frame, stem_frame)) in reference[..samples]
            .chunks_exact(channels)
            .zip(mixed[..samples].chunks_exact(channels))
            .enumerate()
        {
            let reference_envelope = abs_mean(source_frame);
            let candidate_envelope = abs_mean(stem_frame);
            let reference_onset = (reference_envelope - previous_reference).max(0.0);
            let candidate_onset = (candidate_envelope - previous_candidate).max(0.0);
            previous_reference = reference_envelope;
            previous_candidate = candidate_envelope;
            visit(first + local, reference_onset, candidate_onset);
        }
    }
}

fn first_crossing(source: &File, channels: usize, frames: usize, threshold: f32) -> usize {
    let mut values = vec![0.0_f32; BLOCK_SAMPLES];
    let mut previous = 0.0_f32;
    let chunk_frames = BLOCK_SAMPLES / channels;
    for first in (0..frames).step_by(chunk_frames) {
        let samples = (frames - first).min(chunk_frames) * channels;
        read_f32_frames(source, first, channels, &mut values[..samples]);
        for (local, frame) in values[..samples].chunks_exact(channels).enumerate() {
            let envelope = abs_mean(frame);
            let onset = (envelope - previous).max(0.0);
            previous = envelope;
            if onset >= threshold {
                return first + local;
            }
        }
    }
    0
}

#[derive(Clone, Copy)]
struct LagScore {
    lag: isize,
    score: f32,
}

fn correlation(reference: &[f32], candidate: &[f32], lag: isize) -> f32 {
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
        let a = reference[reference_start + offset];
        let b = candidate[candidate_start + offset];
        dot += a * b;
        reference_energy += a * a;
        candidate_energy += b * b;
    }
    let denominator = (reference_energy * candidate_energy).sqrt();
    if denominator <= f32::EPSILON {
        return 0.0;
    }
    dot / denominator
}

fn best_lag(reference: &[f32], candidate: &[f32], lower: isize, upper: isize) -> LagScore {
    let mut best = LagScore {
        lag: 0,
        score: f32::NEG_INFINITY,
    };
    for lag in lower..=upper {
        let score = correlation(reference, candidate, lag);
        if score > best.score {
            best = LagScore { lag, score };
        }
    }
    best
}

fn window_offset(reference: &[f32], candidate: &[f32], rate: u32) -> isize {
    let maximum =
        ((rate as f32 * MAX_SHIFT_SECONDS).round() as usize).min(reference.len().saturating_sub(1));
    if maximum == 0 {
        return 0;
    }
    let hop = (rate as usize / 4000).clamp(1, 64);
    let downsample = |values: &[f32]| {
        values
            .chunks(hop)
            .map(|block| block.iter().copied().fold(0.0_f32, f32::max))
            .collect::<Vec<_>>()
    };
    let a = downsample(reference);
    let b = downsample(candidate);
    let coarse = best_lag(
        &a,
        &b,
        -((maximum / hop) as isize),
        (maximum / hop) as isize,
    )
    .lag;
    let coarse_frames = coarse * hop as isize;
    let radius = (hop as isize).saturating_mul(2).max(1);
    let lower = (coarse_frames - radius).max(-(maximum as isize));
    let upper = (coarse_frames + radius).min(maximum as isize);
    let best = best_lag(reference, candidate, lower, upper);
    let zero = correlation(reference, candidate, 0);
    if best.lag == 0 || best.score < MIN_SCORE || best.score < zero + MIN_IMPROVEMENT {
        0
    } else {
        best.lag
    }
}

pub(super) fn independent_boris_offset(
    source_playback: &Path,
    wav_directory: &Path,
    frames: usize,
) -> isize {
    let source = sealed_reader(source_playback).unwrap();
    let stems = STEM_NAMES
        .iter()
        .map(|name| Riff16::open(&wav_directory.join(format!("{name}.wav")), frames))
        .collect::<Vec<_>>();
    assert!(
        stems
            .iter()
            .all(|stem| stem.channels == 2 && stem.rate == 48000)
    );
    exact_eof(&source, frames as u64 * 2 * 4);
    let mut maximum_reference = 0.0_f32;
    let mut maximum_candidate = 0.0_f32;
    visit_onsets(&source, &stems, 0..frames, |_, reference, candidate| {
        maximum_reference = maximum_reference.max(reference);
        maximum_candidate = maximum_candidate.max(candidate);
    });
    // The complete first pass above consumed actual contents through these EOFs.
    exact_eof(&source, frames as u64 * 2 * 4);
    for stem in &stems {
        exact_eof(&stem.file, 44 + frames as u64 * 2 * 2);
    }
    if maximum_reference <= f32::EPSILON || maximum_candidate <= f32::EPSILON {
        return 0;
    }
    let onset = first_crossing(&source, 2, frames, maximum_reference * THRESHOLD_RATIO);
    let start = onset.saturating_sub((48000_f32 * PRE_ONSET_SECONDS).round() as usize);
    let end = start
        .saturating_add((48000_f32 * ANALYSIS_SECONDS).round() as usize)
        .min(frames);
    if end - start < 4 {
        return 0;
    }
    let mut reference = vec![0.0; end - start];
    let mut candidate = vec![0.0; end - start];
    // The previous envelope is reconstructed from exactly the preceding frame.
    visit_onsets(
        &source,
        &stems,
        start.saturating_sub(1)..end,
        |frame, a, b| {
            if frame >= start {
                reference[frame - start] = a;
                candidate[frame - start] = b;
            }
        },
    );
    window_offset(&reference, &candidate, 48000)
}

pub(super) fn aligned_pcm_hash(wav: &Path, frames: usize, offset: isize) -> String {
    let source = Riff16::open(wav, frames);
    let mut values = vec![0.0_f32; BLOCK_SAMPLES];
    let mut encoded = [0_u8; BLOCK_SAMPLES * 4];
    let mut digest = Sha256::new();
    let chunk_frames = BLOCK_SAMPLES / source.channels;
    for first in (0..frames).step_by(chunk_frames) {
        let count = (frames - first).min(chunk_frames);
        let length = count * source.channels;
        values[..length].fill(0.0);
        // Independent interval intersection of the legacy signed per-frame
        // address rule: source_frame = loaded_frame + offset. Avoid importing
        // the productive converter's block skip/tail implementation.
        let target_first = first as i128;
        let target_end = (first + count) as i128;
        let shift = offset as i128;
        let valid_first = target_first.max(-shift);
        let valid_end = target_end.min(frames as i128 - shift);
        if valid_first < valid_end {
            let local_first =
                usize::try_from(valid_first - target_first).unwrap() * source.channels;
            let local_end = usize::try_from(valid_end - target_first).unwrap() * source.channels;
            let source_first = usize::try_from(valid_first + shift).unwrap();
            source.read_frames(source_first, &mut values[local_first..local_end]);
        }
        for (value, word) in values[..length].iter().zip(encoded.chunks_exact_mut(4)) {
            word.copy_from_slice(&value.to_bits().to_le_bytes());
        }
        digest.update(&encoded[..length * 4]);
    }
    exact_eof(
        &source.file,
        44 + frames as u64 * source.channels as u64 * 2,
    );
    format!("{:x}", digest.finalize())
}

pub(super) fn mono_hash(playback: &Path, channels: usize) -> String {
    assert!((1..=32).contains(&channels));
    let source = sealed_reader(playback).unwrap();
    let extent = source.metadata().unwrap().len();
    assert_eq!(extent % (channels as u64 * 4), 0);
    let frames = usize::try_from(extent / (channels as u64 * 4)).unwrap();
    let mut values = vec![0.0_f32; BLOCK_SAMPLES];
    let mut encoded = [0_u8; BLOCK_SAMPLES * 4];
    let mut digest = Sha256::new();
    let chunk_frames = BLOCK_SAMPLES / channels;
    for first in (0..frames).step_by(chunk_frames) {
        let count = (frames - first).min(chunk_frames);
        read_f32_frames(&source, first, channels, &mut values[..count * channels]);
        for (frame, word) in values[..count * channels]
            .chunks_exact(channels)
            .zip(encoded.chunks_exact_mut(4))
        {
            // Deliberately signed f64 channel mean, distinct from abs f32 onset.
            let mean =
                (frame.iter().map(|&value| f64::from(value)).sum::<f64>() / channels as f64) as f32;
            word.copy_from_slice(&mean.to_bits().to_le_bytes());
        }
        digest.update(&encoded[..count * 4]);
    }
    exact_eof(&source, extent);
    format!("{:x}", digest.finalize())
}
