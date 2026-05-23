//! Offline stem-cache artifact helpers.
//!
//! These helpers run only on background/control-plane threads. They must never be called from the
//! real-time audio callback.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use crate::messages::SampleBuffer;

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
}
