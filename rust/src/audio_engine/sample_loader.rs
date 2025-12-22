//! Audio file loading and decoding functionality.
//!
//! This module provides functions for loading and decoding audio files into sample buffers
//! that can be used by the real-time mixer.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use symphonia::core::{
    audio::SampleBuffer as SymphoniaSampleBuffer, codecs::DecoderOptions,
    errors::Error as SymphoniaError, formats::FormatOptions, io::MediaSourceStream,
    meta::MetadataOptions, probe::Hint,
};
use symphonia::default::{get_codecs, get_probe};

use crate::audio_engine::errors::SampleLoadError;
use crate::messages::SampleBuffer;

/// Decodes an audio file into a sample buffer with the specified output configuration.
///
/// This function loads an audio file from disk, decodes it using the Symphonia library,
/// and converts it to a floating-point sample buffer with the requested channel count
/// and sample rate.
///
/// # Parameters
///
/// - `path`: Path to the audio file to load
/// - `output_channels`: Number of output channels (1 for mono, 2 for stereo)
/// - `output_rate_hz`: Output sample rate in Hz
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
/// - Sample rate mismatch
/// - Invalid or corrupt audio data
pub fn decode_audio_file_to_sample_buffer(
    path: &Path,
    output_channels: usize,
    output_rate_hz: u32,
) -> Result<SampleBuffer, SampleLoadError> {
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

    if file_rate_hz != output_rate_hz {
        return Err(SampleLoadError::SampleRateMismatch {
            file_rate: file_rate_hz,
            output_rate: output_rate_hz,
        });
    }

    let mut decoder = get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

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

        let mut sample_buf = SymphoniaSampleBuffer::<f32>::new(duration, spec);
        sample_buf.copy_interleaved_ref(audio_buf);
        decoded.extend_from_slice(sample_buf.samples());
    }

    let mapped = map_channels(decoded, file_channels, output_channels)?;

    Ok(SampleBuffer {
        channels: output_channels,
        samples: Arc::from(mapped.into_boxed_slice()),
    })
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
    fn test_decode_wav_to_f32_buffer() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.wav");

        let samples = [0i16, 16_384i16, -16_384i16, 32_767i16];
        write_pcm16_wav(&path, 1, 44_100, &samples).unwrap();

        let decoded = decode_audio_file_to_sample_buffer(&path, 1, 44_100).unwrap();
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

        let decoded = decode_audio_file_to_sample_buffer(&path, 2, 44_100).unwrap();
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
    fn test_decode_invalid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nonexistent.wav");

        let result = decode_audio_file_to_sample_buffer(&path, 1, 44_100);
        assert!(result.is_err());
    }
}
