//! Offline stem-cache artifact helpers.
//!
//! These helpers run only on background/control-plane threads. They must never be called from the
//! real-time audio callback.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use crate::messages::{PreparedStemSet, STEM_BUFFER_COUNT, SampleBuffer};

pub(crate) const STEM_FILE_NAMES: [&str; 5] = ["vocals", "melody", "bass", "drums", "instrumental"];

pub(crate) fn project_stem_cache_dir(cache_dir: &str) -> Result<PathBuf, String> {
    let path = Path::new(cache_dir);
    if path.is_absolute() {
        return Err("stem cache directory must be project-relative".to_string());
    }

    let parts: Vec<String> = path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value
                .to_str()
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .ok_or_else(|| "stem cache directory contains invalid UTF-8".to_string()),
            _ => Err(
                "stem cache directory must not contain root, current, or parent components"
                    .to_string(),
            ),
        })
        .collect::<Result<_, _>>()?;

    if parts.len() != 3 || parts[0] != "samples" || parts[1] != "stems" {
        return Err(
            "stem cache directory must be under samples/stems/<source-version>".to_string(),
        );
    }

    Ok(parts.iter().collect())
}

pub(crate) fn write_deterministic_stem_artifacts<F>(
    sample: &SampleBuffer,
    output_sample_rate: u32,
    cache_dir: &str,
    mut progress: F,
) -> Result<(), String>
where
    F: FnMut(f32, &'static str),
{
    write_deterministic_stem_artifacts_at_project_root(
        sample,
        output_sample_rate,
        cache_dir,
        Path::new("."),
        &mut progress,
    )
}

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

fn prepare_stem_buffers_from_cache_at_project_root(
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
    let expected_frames = reference.samples.len() / reference.channels;
    if expected_frames == 0 {
        return Err("reference sample must contain at least one frame".to_string());
    }

    let cache_dir = project_root.join(project_stem_cache_dir(cache_dir)?);
    let mut buffers = Vec::with_capacity(STEM_BUFFER_COUNT);

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

    let stems: [SampleBuffer; STEM_BUFFER_COUNT] = buffers
        .try_into()
        .map_err(|_| "stem set is incomplete".to_string())?;

    Ok(PreparedStemSet {
        source_version_hash: source_version_hash(source_version),
        sample_rate_hz: output_sample_rate,
        channels: reference.channels,
        frame_count: expected_frames,
        available_mask: ((1_u16 << STEM_BUFFER_COUNT) - 1) as u8,
        stems,
    })
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
    if sample.samples.len() % sample.channels != 0 {
        return Err("sample buffer length must align to channel count".to_string());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct WavFormat {
    channels: usize,
    sample_rate_hz: u32,
    block_align: usize,
}

fn read_aligned_pcm16_wav(
    path: &Path,
    expected_sample_rate_hz: u32,
    expected_channels: usize,
    expected_frames: usize,
) -> Result<SampleBuffer, String> {
    let bytes = fs::read(path).map_err(|err| format!("Failed to read WAV file: {err}"))?;
    let (format, data) = parse_pcm16_wav(&bytes)?;

    if format.sample_rate_hz != expected_sample_rate_hz {
        return Err(format!(
            "sample rate mismatch: expected {expected_sample_rate_hz}, got {}",
            format.sample_rate_hz
        ));
    }
    if format.channels != expected_channels {
        return Err(format!(
            "channel count mismatch: expected {expected_channels}, got {}",
            format.channels
        ));
    }
    if data.len() % format.block_align != 0 {
        return Err("data length does not align to WAV block size".to_string());
    }

    let frames = data.len() / format.block_align;
    if frames == 0 {
        return Err("stem artifact must contain at least one frame".to_string());
    }
    if frames != expected_frames {
        return Err(format!(
            "frame count mismatch: expected {expected_frames}, got {frames}"
        ));
    }

    let mut samples = Vec::with_capacity(data.len() / 2);
    for chunk in data.chunks_exact(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
        samples.push(pcm16_to_float(sample));
    }

    Ok(SampleBuffer {
        channels: format.channels,
        samples: samples.into_boxed_slice().into(),
    })
}

fn parse_pcm16_wav(bytes: &[u8]) -> Result<(WavFormat, &[u8]), String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("expected RIFF/WAVE header".to_string());
    }

    let mut offset: usize = 12;
    let mut format = None;
    let mut data = None;

    while offset.saturating_add(8) <= bytes.len() {
        let chunk_id = &bytes[offset..offset + 4];
        let chunk_size = parse_le_u32(bytes, offset + 4)
            .ok_or_else(|| "invalid WAV chunk size".to_string())? as usize;
        let chunk_start = offset + 8;
        let chunk_end = chunk_start
            .checked_add(chunk_size)
            .ok_or_else(|| "WAV chunk size overflow".to_string())?;
        if chunk_end > bytes.len() {
            return Err("WAV chunk extends past end of file".to_string());
        }

        match chunk_id {
            b"fmt " => {
                format = Some(parse_pcm16_wav_format(&bytes[chunk_start..chunk_end])?);
            }
            b"data" => {
                data = Some(&bytes[chunk_start..chunk_end]);
            }
            _ => {}
        }

        offset = chunk_end + (chunk_size % 2);
    }

    let format = format.ok_or_else(|| "missing fmt chunk".to_string())?;
    let data = data.ok_or_else(|| "missing data chunk".to_string())?;
    Ok((format, data))
}

fn parse_pcm16_wav_format(bytes: &[u8]) -> Result<WavFormat, String> {
    if bytes.len() < 16 {
        return Err("fmt chunk is too short".to_string());
    }

    let audio_format = parse_le_u16(bytes, 0).ok_or_else(|| "missing audio format".to_string())?;
    if audio_format != 1 {
        return Err("only PCM WAV stem artifacts are supported".to_string());
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
        assert_eq!(prepared.available_mask, 0b1_1111);
        assert!(prepared.source_version_hash != 0);
        for stem in prepared.stems {
            assert_eq!(stem.channels, 2);
            assert_eq!(stem.samples.len(), 4);
        }
    }

    #[test]
    fn prepare_stem_buffers_rejects_sample_rate_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let sample = SampleBuffer {
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
