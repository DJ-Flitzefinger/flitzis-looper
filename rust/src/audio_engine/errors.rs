//! Audio-specific error types.

use thiserror::Error;

/// Errors that can occur while loading audio files.
#[derive(Debug, Error)]
pub enum SampleLoadError {
    /// Failed to open the audio file.
    #[error("failed to open file: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to decode the audio file.
    #[error("failed to decode audio file: {0}")]
    Decode(#[from] symphonia::core::errors::Error),

    /// Failed to create resampler.
    #[error("failed to create resampler: {0}")]
    ResamplerConstruction(#[from] rubato::ResamplerConstructionError),

    /// Failed to resample audio.
    #[error("failed to resample audio: {0}")]
    Resample(#[from] rubato::ResampleError),

    /// Audio file has no default track.
    #[error("audio file has no default track")]
    NoDefaultTrack,

    /// Audio file is missing sample rate information.
    #[error("audio file is missing a sample rate")]
    MissingSampleRate,

    /// Audio file is missing channel information.
    #[error("audio file is missing channel information")]
    MissingChannels,

    /// Unsupported channel mapping configuration.
    #[error(
        "unsupported channel mapping: file has {file_channels} channels, output has {output_channels} channels (only mono↔stereo supported)"
    )]
    UnsupportedChannels {
        /// Number of channels in the source file.
        file_channels: usize,
        /// Number of channels expected for output.
        output_channels: usize,
    },
}
