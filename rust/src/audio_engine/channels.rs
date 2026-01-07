use crate::audio_engine::errors::SampleLoadError;

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
    use super::*;

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
}
