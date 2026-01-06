//! Audio file loading and decoding functionality.
//!
//! This module provides functions for loading and decoding audio files into sample buffers
//! that can be used by the real-time mixer.

use audioadapter::AdapterMut;
use audioadapter_buffers::owned::InterleavedOwned;
use rubato::{Fft, FixedSync, Indexing, Resampler};
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::audio_engine::errors::SampleLoadError;
use crate::messages::SampleBuffer;
use symphonia::core::{
    audio::SampleBuffer as SymphoniaSampleBuffer, codecs::DecoderOptions,
    errors::Error as SymphoniaError, formats::FormatOptions, io::MediaSourceStream,
    meta::MetadataOptions, probe::Hint,
};
use symphonia::default::{get_codecs, get_probe};

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
///
/// # Arguments
///
/// * `samples` - Interleaved audio samples to resample
/// * `channels` - Number of channels in the audio data
/// * `from_rate` - Current sample rate in Hz
/// * `to_rate` - Target sample rate in Hz
///
/// # Returns
///
/// - `Ok(Vec<f32>)`: Resampled audio samples
/// - `Err(SampleLoadError)`: Resampling error
fn resample_audio<F>(
    samples: Vec<f32>,
    channels: usize,
    from_rate: u32,
    to_rate: u32,
    mut progress: F,
) -> Result<Vec<f32>, SampleLoadError>
where
    F: FnMut(f32),
{
    if from_rate == to_rate {
        progress(1.0);
        return Ok(samples);
    }

    let input_frames = samples.len() / channels;
    if input_frames == 0 {
        progress(1.0);
        return Ok(Vec::new());
    }

    // Create resampler with fixed I/O size.
    let mut resampler = Fft::<f32>::new(
        from_rate as usize,
        to_rate as usize,
        1024, // chunk_size
        1,    // sub_chunks
        channels,
        FixedSync::Input,
    )?;

    // Create input buffer adapter.
    let input_buffer = InterleavedOwned::new_from(samples, channels, input_frames).unwrap();

    // Manual version of rubato's `process_all_into_buffer()` loop, so we can emit progress.
    let expected_output_len = (resampler.resample_ratio() * input_frames as f64).ceil() as usize;

    // Allocate with `output_frames_max()` (not `output_frames_next()`), because for some
    // resampler configurations `output_frames_next()` can change after processing starts.
    // This matches rubato's `process_all_needed_output_len()` logic but uses the maximum.
    let output_frames = resampler
        .output_delay()
        .saturating_add(resampler.output_frames_max())
        .saturating_add(expected_output_len);

    let mut output_buffer =
        InterleavedOwned::new_from(vec![0.0; output_frames * channels], channels, output_frames)
            .unwrap();

    let mut indexing = Indexing {
        input_offset: 0,
        output_offset: 0,
        partial_len: None,
        active_channels_mask: None,
    };

    let mut frames_left = input_frames;
    let mut output_len: usize = 0;
    let mut frames_to_trim = resampler.output_delay();

    progress(0.0);

    let next_nbr_input_frames = resampler.input_frames_next();
    while frames_left > next_nbr_input_frames {
        let (nbr_in, nbr_out) =
            resampler.process_into_buffer(&input_buffer, &mut output_buffer, Some(&indexing))?;

        frames_left = frames_left.saturating_sub(nbr_in);
        output_len = output_len.saturating_add(nbr_out);
        indexing.input_offset = indexing.input_offset.saturating_add(nbr_in);
        indexing.output_offset = indexing.output_offset.saturating_add(nbr_out);

        if frames_to_trim > 0 && output_len > frames_to_trim {
            output_buffer.copy_frames_within(frames_to_trim, 0, frames_to_trim);
            output_len -= frames_to_trim;
            indexing.output_offset -= frames_to_trim;
            frames_to_trim = 0;
        }

        let consumed = input_frames.saturating_sub(frames_left);
        let percent = consumed as f32 / input_frames as f32;
        progress(clamp_progress(percent));
    }

    if frames_left > 0 {
        indexing.partial_len = Some(frames_left);
        let (_nbr_in, nbr_out) =
            resampler.process_into_buffer(&input_buffer, &mut output_buffer, Some(&indexing))?;

        output_len = output_len.saturating_add(nbr_out);
        indexing.output_offset = indexing.output_offset.saturating_add(nbr_out);

        progress(1.0);
    }

    indexing.partial_len = Some(0);
    while output_len < expected_output_len {
        let (_nbr_in, nbr_out) =
            resampler.process_into_buffer(&input_buffer, &mut output_buffer, Some(&indexing))?;

        output_len = output_len.saturating_add(nbr_out);
        indexing.output_offset = indexing.output_offset.saturating_add(nbr_out);

        progress(1.0);
    }

    let trimmed_buf = output_buffer.take_data();
    let trimmed_buf = trimmed_buf[..expected_output_len * channels].to_vec();

    Ok(trimmed_buf)
}

/// Maps audio samples from one channel configuration to another.
///
/// Currently supports:
/// - Mono (1 channel) → Stereo (2 channels): duplicates mono signal to both channels
/// - Stereo (2 channels) → Mono (1 channel): averages both channels
/// - Same channel count: no conversion needed
///
/// # Parameters
///
/// - `samples`: Interleaved audio samples to convert
/// - `file_channels`: Number of channels in the source audio
/// - `output_channels`: Number of channels for the output
///
/// # Returns
///
/// - `Ok(Vec<f32>)`: Samples with converted channel layout
/// - `Err(SampleLoadError)`: Unsupported channel mapping
pub fn map_channels(
    samples: Vec<f32>,
    file_channels: usize,
    output_channels: usize,
) -> Result<Vec<f32>, SampleLoadError> {
    if file_channels == output_channels {
        return Ok(samples);
    }

    match (file_channels, output_channels) {
        // Mono → Stereo: duplicate each sample
        (1, 2) => {
            let mut out = Vec::with_capacity(samples.len() * 2);
            for s in samples {
                out.push(s);
                out.push(s);
            }
            Ok(out)
        }
        // Stereo → Mono: average each frame
        (2, 1) => {
            let mut out = Vec::with_capacity(samples.len() / 2);
            for frame in samples.chunks_exact(2) {
                out.push((frame[0] + frame[1]) * 0.5);
            }
            Ok(out)
        }
        // Unsupported mapping
        _ => Err(SampleLoadError::UnsupportedChannels {
            file_channels,
            output_channels,
        }),
    }
}

/// Decodes an audio file into a sample buffer with the specified output configuration.
///
/// This function loads an audio file from disk, decodes it using the Symphonia library,
/// resamples it to the target sample rate (if needed), and converts it to a
/// floating-point sample buffer with the requested channel count.
///
/// # Parameters
///
/// - `path`: Path to the audio file to load
/// - `output_channels`: Number of output channels (1 for mono, 2 for stereo)
/// - `output_rate_hz`: Output sample rate in Hz
/// - `progress`: Progress callback
///
/// # Returns
///
/// - `Ok(SampleBuffer)`: Successfully decoded audio buffer
/// - `Err(SampleLoadError)`: Error encountered during loading or decoding
///
/// # Errors
///
/// This function may return errors for various conditions:
/// - File not found or cannot be opened
/// - Audio format not recognized or corrupted
/// - Unsupported channel count
/// - Resampling errors
/// - Invalid or corrupt audio data
#[allow(unused_imports)] // Temporary until we clean up imports
pub fn decode_audio_file_to_sample_buffer<F>(
    path: &Path,
    output_channels: usize,
    output_rate_hz: u32,
    mut progress: F,
) -> Result<SampleBuffer, SampleLoadError>
where
    F: FnMut(SampleLoadProgress),
{
    use std::io::Write;

    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;

    let track = format
        .default_track()
        .ok_or(SampleLoadError::NoDefaultTrack)?;
    let file_rate_hz = track
        .codec_params
        .sample_rate
        .ok_or(SampleLoadError::MissingSampleRate)?;
    let file_channels = track
        .codec_params
        .channels
        .ok_or(SampleLoadError::MissingChannels)?
        .count();

    let mut decoder = get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let resampling_required = file_rate_hz != output_rate_hz;

    let total_frames = track.codec_params.n_frames;
    let mut decoded_frames: u64 = 0;
    progress(SampleLoadProgress {
        subtask: SampleLoadSubtask::Decoding,
        resampling_required,
        percent: 0.0,
    });

    let mut decoded: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(err) => return Err(SampleLoadError::Decode(err)),
        };

        let audio_buf = decoder.decode(&packet)?;
        let spec = *audio_buf.spec();
        let duration = audio_buf.capacity() as u64;
        let packet_frames = audio_buf.frames() as u64;

        let mut sample_buf = SymphoniaSampleBuffer::<f32>::new(duration, spec);
        sample_buf.copy_interleaved_ref(audio_buf);
        decoded.extend_from_slice(sample_buf.samples());

        decoded_frames = decoded_frames.saturating_add(packet_frames);
        if let Some(total_frames) = total_frames {
            let percent = (decoded_frames as f32 / total_frames as f32).min(1.0);
            progress(SampleLoadProgress {
                subtask: SampleLoadSubtask::Decoding,
                resampling_required,
                percent: clamp_progress(percent),
            });
        }
    }

    progress(SampleLoadProgress {
        subtask: SampleLoadSubtask::Decoding,
        resampling_required,
        percent: 1.0,
    });

    let resampled = if !resampling_required {
        decoded
    } else {
        progress(SampleLoadProgress {
            subtask: SampleLoadSubtask::Resampling,
            resampling_required,
            percent: 0.0,
        });
        resample_audio(
            decoded,
            file_channels,
            file_rate_hz,
            output_rate_hz,
            |percent| {
                progress(SampleLoadProgress {
                    subtask: SampleLoadSubtask::Resampling,
                    resampling_required,
                    percent,
                });
            },
        )?
    };

    progress(SampleLoadProgress {
        subtask: SampleLoadSubtask::ChannelMapping,
        resampling_required,
        percent: 0.0,
    });
    let mapped = map_channels(resampled, file_channels, output_channels)?;
    progress(SampleLoadProgress {
        subtask: SampleLoadSubtask::ChannelMapping,
        resampling_required,
        percent: 1.0,
    });

    Ok(SampleBuffer {
        channels: output_channels,
        samples: Arc::from(mapped.into_boxed_slice()),
    })
}

/// Generates a unique filename for caching an audio file, handling collisions
/// by appending numeric suffixes (_0, _1, etc.).
fn find_unique_cache_path(
    project_samples_dir: &Path,
    stem: &str,
    extension: &str,
) -> std::io::Result<PathBuf> {
    let base = project_samples_dir.join(format!("{stem}{extension}"));

    // Check if any file with this stem exists (regardless of extension)
    let has_collision = base.exists()
        || std::fs::read_dir(project_samples_dir)?.any(|entry| {
            let entry = entry.unwrap();
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();
            file_name_str.starts_with(&format!("{stem}_"))
                || file_name_str == format!("{stem}{}", extension)
                || file_name_str.starts_with(&format!("{stem}."))
        });

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
    use std::io::Write;

    use super::*;

    /// Helper function to create a PCM16 WAV file for testing.
    fn write_pcm16_wav(
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
    fn test_map_channels_mono_to_stereo() {
        let input = vec![0.5, -0.3, 0.8];
        let output = map_channels(input, 1, 2).unwrap();

        assert_eq!(output.len(), 6); // 3 frames × 2 channels
        assert_eq!(output, vec![0.5, 0.5, -0.3, -0.3, 0.8, 0.8]);
    }

    #[test]
    fn test_map_channels_stereo_to_mono() {
        let input = vec![0.5, 0.3, -0.2, 0.4, 0.8, 0.6];
        let output = map_channels(input, 2, 1).unwrap();

        assert_eq!(output.len(), 3); // 3 frames × 1 channel
        assert!((output[0] - 0.4).abs() < 1e-6); // (0.5 + 0.3) / 2
        assert!((output[1] - 0.1).abs() < 1e-6); // (-0.2 + 0.4) / 2
        assert!((output[2] - 0.7).abs() < 1e-6); // (0.8 + 0.6) / 2
    }

    #[test]
    fn test_map_channels_same_channels() {
        let input = vec![0.5, -0.3, 0.8, 0.2];
        let output = map_channels(input.clone(), 2, 2).unwrap();

        assert_eq!(output, input); // Should return unchanged
    }

    #[test]
    fn test_map_channels_unsupported() {
        let input = vec![0.5, -0.3, 0.8, 0.2];
        let result = map_channels(input, 2, 4);

        assert!(matches!(
            result,
            Err(SampleLoadError::UnsupportedChannels { .. })
        ));
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
