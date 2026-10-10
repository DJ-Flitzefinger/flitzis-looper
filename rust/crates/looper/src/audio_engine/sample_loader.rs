//! Audio file loading and decoding functionality.
//!
//! This module provides functions for loading and decoding audio files into sample buffers
//! that can be used by the real-time mixer.

#[cfg(test)]
use std::fs::{self, File};
#[cfg(test)]
use std::path::{Path, PathBuf};

use crate::audio_engine::errors::SampleLoadError;
#[cfg(test)]
use crate::messages::SampleBuffer;

mod cold;
pub(crate) use cold::validate_decoder_cache_provenance;
pub(crate) use cold::{decode_audio_snapshot, prepare_playback};
pub(crate) use cold::{decoder_cache_selector, playback_cache_transform};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleLoadSubtask {
    Decoding,
    Resampling,
    ChannelMapping,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleLoadProgress {
    pub subtask: SampleLoadSubtask,
    pub resampling_required: bool,
    /// Best-effort local subtask progress (0.0..=1.0).
    pub percent: f32,
}

fn clamp_progress(percent: f32) -> f32 {
    if percent.is_finite() {
        percent.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn append_decode_error_silence(
    decoded: &mut Vec<f32>,
    channels: Option<usize>,
    packet_frames: u64,
) -> Option<u64> {
    let channels = channels.filter(|channels| *channels > 0)?;
    if packet_frames == 0 {
        return None;
    }

    let frames = usize::try_from(packet_frames).ok()?;
    let silence_samples = frames.checked_mul(channels)?;
    let new_len = decoded.len().checked_add(silence_samples)?;
    decoded.resize(new_len, 0.0);
    Some(packet_frames)
}

fn update_decoded_stream_config(
    sample_rate_hz: &mut Option<u32>,
    channels: &mut Option<usize>,
    decoded_rate_hz: u32,
    decoded_channels: usize,
) -> Result<(), SampleLoadError> {
    if decoded_channels == 0 {
        return Err(SampleLoadError::MissingChannels);
    }

    match *sample_rate_hz {
        Some(initial_rate_hz) if initial_rate_hz != decoded_rate_hz => {
            Err(SampleLoadError::InconsistentSampleRate {
                initial_rate_hz,
                new_rate_hz: decoded_rate_hz,
            })
        }
        Some(_) => Ok(()),
        None => {
            *sample_rate_hz = Some(decoded_rate_hz);
            Ok(())
        }
    }?;

    match *channels {
        Some(initial_channels) if initial_channels != decoded_channels => {
            Err(SampleLoadError::InconsistentChannels {
                initial_channels,
                new_channels: decoded_channels,
            })
        }
        Some(_) => Ok(()),
        None => {
            *channels = Some(decoded_channels);
            Ok(())
        }
    }
}

/// Decode a path for legacy fixture consumers. Productive cold loading supplies
/// a stable snapshot handle to `decode_audio_snapshot` instead.
#[cfg(test)]
pub fn decode_audio_file_to_sample_buffer<F>(
    path: &Path,
    output_channels: usize,
    output_rate_hz: u32,
    mut progress: F,
) -> Result<SampleBuffer, SampleLoadError>
where
    F: FnMut(SampleLoadProgress),
{
    let decoded = decode_audio_snapshot(
        File::open(path)?,
        path,
        output_rate_hz,
        usize::MAX,
        &|| false,
        &mut progress,
    )?;
    let (sample, _) = prepare_playback(
        &decoded,
        output_channels,
        output_rate_hz,
        usize::MAX,
        &|| false,
        progress,
    )?;
    Ok(sample)
}

/// Generates a unique filename for caching an audio file, handling collisions
/// by appending numeric suffixes (_0, _1, etc.).
#[cfg(test)]
fn find_unique_cache_path(
    project_samples_dir: &Path,
    stem: &str,
    extension: &str,
) -> std::io::Result<PathBuf> {
    let base = project_samples_dir.join(format!("{stem}{extension}"));

    let has_collision = if base.exists() {
        true
    } else {
        let directory_file_names = std::fs::read_dir(project_samples_dir)?
            .map(|entry| entry.map(|entry| entry.file_name()));
        has_cache_filename_collision(false, stem, extension, directory_file_names)?
    };

    if !has_collision {
        return Ok(base);
    }

    // Try with numeric suffixes
    for index in 0..=999 {
        let candidate = project_samples_dir.join(format!("{stem}_{index}{extension}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "too many filename collisions",
    ))
}

#[cfg(test)]
fn has_cache_filename_collision<I, N>(
    base_exists: bool,
    stem: &str,
    extension: &str,
    file_names: I,
) -> std::io::Result<bool>
where
    I: IntoIterator<Item = std::io::Result<N>>,
    N: AsRef<std::ffi::OsStr>,
{
    if base_exists {
        return Ok(true);
    }

    let suffixed_stem = format!("{stem}_");
    let exact_name = format!("{stem}{extension}");
    let extension_collision_prefix = format!("{stem}.");

    for file_name in file_names {
        let file_name = file_name?;
        let file_name_str = file_name.as_ref().to_string_lossy();
        if file_name_str.starts_with(&suffixed_stem)
            || file_name_str == exact_name
            || file_name_str.starts_with(&extension_collision_prefix)
        {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Copies an audio file to the project samples directory with collision handling.
///
/// # Arguments
///
/// * `project_samples_dir` - Directory to copy the file into
/// * `source_path` - Path to the original audio file
///
/// # Returns
///
/// - `Ok(PathBuf)`: Path to the cached file (either existing or newly copied)
/// - `Err(std::io::Error)`: I/O error during copy operation
#[cfg(test)]
pub fn cache_audio_file_for_project(
    project_samples_dir: &Path,
    source_path: &Path,
) -> std::io::Result<PathBuf> {
    fs::create_dir_all(project_samples_dir)?;

    // Extract file stem and extension from source path
    let stem = source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("sample");

    let extension = source_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_else(|| ".unknown".to_string());

    let cache_path = find_unique_cache_path(project_samples_dir, stem, &extension)?;

    // Copy the original file to the cache location
    fs::copy(source_path, &cache_path)?;

    Ok(cache_path)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::io::Write;

    use super::*;

    /// Helper function to create a PCM16 WAV file for testing.
    pub(super) fn write_pcm16_wav(
        path: &Path,
        channels: u16,
        sample_rate_hz: u32,
        samples: &[i16],
    ) -> std::io::Result<()> {
        let bits_per_sample = 16u16;
        let block_align = channels * (bits_per_sample / 8);
        let byte_rate = sample_rate_hz * u32::from(block_align);
        let data_len_bytes = u32::try_from(samples.len() * 2).expect("sample data too large");
        let chunk_size = 36 + data_len_bytes;

        let mut file = File::create(path)?;
        file.write_all(b"RIFF")?;
        file.write_all(&chunk_size.to_le_bytes())?;
        file.write_all(b"WAVE")?;

        file.write_all(b"fmt ")?;
        file.write_all(&16u32.to_le_bytes())?;
        file.write_all(&1u16.to_le_bytes())?; // PCM
        file.write_all(&channels.to_le_bytes())?;
        file.write_all(&sample_rate_hz.to_le_bytes())?;
        file.write_all(&byte_rate.to_le_bytes())?;
        file.write_all(&block_align.to_le_bytes())?;
        file.write_all(&bits_per_sample.to_le_bytes())?;

        file.write_all(b"data")?;
        file.write_all(&data_len_bytes.to_le_bytes())?;
        for sample in samples {
            file.write_all(&sample.to_le_bytes())?;
        }

        Ok(())
    }

    #[test]
    fn test_decoded_stream_config_accepts_missing_container_metadata() {
        let mut sample_rate_hz = None;
        let mut channels = None;

        update_decoded_stream_config(&mut sample_rate_hz, &mut channels, 44_100, 2).unwrap();

        assert_eq!(sample_rate_hz, Some(44_100));
        assert_eq!(channels, Some(2));
    }

    #[test]
    fn test_decoded_stream_config_rejects_midstream_sample_rate_change() {
        let mut sample_rate_hz = Some(44_100);
        let mut channels = Some(2);

        let result = update_decoded_stream_config(&mut sample_rate_hz, &mut channels, 48_000, 2);

        assert!(matches!(
            result,
            Err(SampleLoadError::InconsistentSampleRate {
                initial_rate_hz: 44_100,
                new_rate_hz: 48_000,
            })
        ));
    }

    #[test]
    fn test_decoded_stream_config_rejects_midstream_channel_change() {
        let mut sample_rate_hz = Some(44_100);
        let mut channels = Some(2);

        let result = update_decoded_stream_config(&mut sample_rate_hz, &mut channels, 44_100, 1);

        assert!(matches!(
            result,
            Err(SampleLoadError::InconsistentChannels {
                initial_channels: 2,
                new_channels: 1,
            })
        ));
    }

    #[test]
    fn test_decoded_stream_config_rejects_zero_channels() {
        let mut sample_rate_hz = None;
        let mut channels = None;

        let result = update_decoded_stream_config(&mut sample_rate_hz, &mut channels, 44_100, 0);

        assert!(matches!(result, Err(SampleLoadError::MissingChannels)));
    }

    #[test]
    fn test_decode_error_silence_preserves_packet_duration() {
        let mut decoded = vec![1.0, -1.0];

        let frames = append_decode_error_silence(&mut decoded, Some(2), 3).unwrap();

        assert_eq!(frames, 3);
        assert_eq!(decoded, vec![1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_decode_error_silence_requires_known_channels() {
        let mut decoded = vec![1.0, -1.0];

        assert_eq!(append_decode_error_silence(&mut decoded, None, 3), None);
        assert_eq!(append_decode_error_silence(&mut decoded, Some(2), 0), None);
        assert_eq!(decoded, vec![1.0, -1.0]);
    }

    #[test]
    fn test_cache_audio_file_copies_original() {
        let tmp = tempfile::tempdir().unwrap();
        let samples_dir = tmp.path().join("samples");
        let source_dir = tmp.path().join("source");
        fs::create_dir_all(&source_dir).unwrap();

        // Create a source WAV file
        let source_path = source_dir.join("loop.flac");
        let samples = [0i16, 16_384i16, -16_384i16, 32_767i16];
        write_pcm16_wav(&source_path, 1, 44_100, &samples).unwrap();

        let cached = cache_audio_file_for_project(&samples_dir, &source_path).unwrap();

        assert!(cached.is_file());
        assert!(cached.ends_with(Path::new("samples/loop.flac")));

        // Verify the file contents are identical
        let source_content = std::fs::read(&source_path).unwrap();
        let cached_content = std::fs::read(&cached).unwrap();
        assert_eq!(source_content, cached_content);
    }

    #[test]
    fn test_cache_audio_file_handles_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let samples_dir = tmp.path().join("samples");
        let source_dir = tmp.path().join("source");
        fs::create_dir_all(&source_dir).unwrap();

        // Create first source file
        let source_path_a = source_dir.join("loop.mp3");
        let samples_a = [0i16, 16_384i16, -16_384i16];
        write_pcm16_wav(&source_path_a, 1, 44_100, &samples_a).unwrap();

        let path_a = cache_audio_file_for_project(&samples_dir, &source_path_a).unwrap();
        assert!(path_a.ends_with(Path::new("samples/loop.mp3")));

        // Create second source file with same stem but different extension
        let source_path_b = source_dir.join("loop.wav");
        let samples_b = [0i16, 32_767i16, -32_767i16];
        write_pcm16_wav(&source_path_b, 1, 44_100, &samples_b).unwrap();

        let path_b = cache_audio_file_for_project(&samples_dir, &source_path_b).unwrap();

        assert_ne!(path_a, path_b);
        assert!(path_a.is_file());
        assert!(path_b.is_file());
        assert!(path_b.ends_with(Path::new("samples/loop_0.wav")));

        // Verify both files have correct content
        let source_content_a = std::fs::read(&source_path_a).unwrap();
        let cached_content_a = std::fs::read(&path_a).unwrap();
        assert_eq!(source_content_a, cached_content_a);

        let source_content_b = std::fs::read(&source_path_b).unwrap();
        let cached_content_b = std::fs::read(&path_b).unwrap();
        assert_eq!(source_content_b, cached_content_b);
    }

    #[test]
    fn test_cache_audio_file_handles_multiple_collisions() {
        let tmp = tempfile::tempdir().unwrap();
        let samples_dir = tmp.path().join("samples");
        let source_dir = tmp.path().join("source");
        fs::create_dir_all(&source_dir).unwrap();

        // Create multiple source files with same stem
        let paths: Vec<PathBuf> = (0..5)
            .map(|i| {
                let source_path = source_dir.join(format!("loop_{}.mp3", i));
                let samples = [0i16, 1i16, 2i16, 3i16];
                write_pcm16_wav(&source_path, 1, 44_100, &samples).unwrap();
                source_path
            })
            .collect();

        // Cache them all (they should all have stem "loop")
        let cached_paths: Vec<PathBuf> = paths
            .iter()
            .map(|source_path| {
                // Rename to have same stem for collision testing
                let temp_path = source_dir.join("loop.mp3");
                fs::copy(source_path, &temp_path).unwrap();
                let cached = cache_audio_file_for_project(&samples_dir, &temp_path).unwrap();
                fs::remove_file(&temp_path).unwrap();
                cached
            })
            .collect();

        // All should be cached successfully
        assert_eq!(cached_paths.len(), 5);
        for path in &cached_paths {
            assert!(path.is_file());
        }

        // Check that they have different names
        let mut cached_names = cached_paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect::<Vec<String>>();
        cached_names.sort();
        assert_eq!(
            cached_names,
            vec![
                "loop.mp3",
                "loop_0.mp3",
                "loop_1.mp3",
                "loop_2.mp3",
                "loop_3.mp3",
            ]
        );
    }

    #[test]
    fn test_cache_audio_file_handles_too_many_collisions() {
        let tmp = tempfile::tempdir().unwrap();
        let samples_dir = tmp.path().join("samples");
        let source_dir = tmp.path().join("source");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&samples_dir).unwrap();

        println!("samples_dir exists: {}", samples_dir.exists());

        // Create a source file
        let source_path = source_dir.join("loop.mp3");
        let samples = [0i16, 16_384i16];
        write_pcm16_wav(&source_path, 1, 44_100, &samples).unwrap();

        // Create 1000 collision files
        for i in 0..=999 {
            let collision_path = samples_dir.join(format!("loop_{}.mp3", i));
            write_pcm16_wav(&collision_path, 1, 44_100, &samples).unwrap();
        }

        // The next one should fail
        let result = cache_audio_file_for_project(&samples_dir, &source_path);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn test_cache_filename_collision_propagates_directory_entry_error() {
        let file_names: Vec<std::io::Result<OsString>> = vec![Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "entry failed",
        ))];

        let result = has_cache_filename_collision(false, "loop", ".wav", file_names);

        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn test_decode_wav_to_f32_buffer() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.wav");

        let samples = [0i16, 16_384i16, -16_384i16, 32_767i16];
        write_pcm16_wav(&path, 1, 44_100, &samples).unwrap();

        let decoded = decode_audio_file_to_sample_buffer(&path, 1, 44_100, |_| {}).unwrap();
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.samples.len(), samples.len());
        assert!(decoded.samples.iter().all(|s| (-1.0..=1.0).contains(s)));
    }

    #[test]
    fn test_decode_channel_mapping_mono_to_stereo() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.wav");

        let samples = [0i16, 16_384i16, -16_384i16];
        write_pcm16_wav(&path, 1, 44_100, &samples).unwrap();

        let decoded = decode_audio_file_to_sample_buffer(&path, 2, 44_100, |_| {}).unwrap();
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.samples.len(), samples.len() * 2);

        // Verify that mono samples are duplicated to both stereo channels
        for frame in decoded.samples.chunks_exact(2) {
            assert!((frame[0] - frame[1]).abs() < 1e-6);
        }
    }

    #[test]
    fn test_resample_same_rate() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.wav");

        let samples = [0i16, 16_384i16, -16_384i16, 32_767i16];
        write_pcm16_wav(&path, 1, 44_100, &samples).unwrap();

        // Decode at same sample rate (no resampling needed)
        let decoded = decode_audio_file_to_sample_buffer(&path, 1, 44_100, |_| {}).unwrap();
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.samples.len(), samples.len());
    }

    #[test]
    fn test_resample_48000_to_44100() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.wav");

        // Create a 48kHz test file with more samples to ensure resampling changes the count
        // Using 1024 frames (1024 samples) to be larger than the resampler's chunk size
        let samples = vec![0i16; 1024];
        let sample_count = samples.len();
        write_pcm16_wav(&path, 1, 48_000, &samples).unwrap();

        // Decode at 44.1kHz (requires resampling)
        let decoded = decode_audio_file_to_sample_buffer(&path, 1, 44_100, |_| {}).unwrap();
        assert_eq!(decoded.channels, 1);
        // For 48kHz->44.1kHz, we expect fewer output samples (44100/48000 = 0.91875)
        // With 1024 input samples (1024 frames), we expect ~945.35 output frames = ~945 output samples
        assert!(
            decoded.samples.len() < sample_count,
            "Expected less than {} samples, got {}",
            sample_count,
            decoded.samples.len()
        );
        assert!(decoded.samples.iter().all(|s| (-1.0..=1.0).contains(s)));
    }
}
