//! Voice management for real-time audio mixing.
//!
//! This module provides the [`Voice`] struct which represents a single playing
//! audio sample with its current playback state.
//!
//! Voices are managed by the [`RtMixer`](crate::audio_engine::mixer::RtMixer) and represent
//! individual instances of playing samples with independent playback positions and volumes.

use crate::messages::SampleBuffer;

/// A single voice in the mixer, representing a playing audio sample.
#[derive(Debug)]
pub struct Voice {
    /// ID of the sample being played.
    pub sample_id: usize,

    /// The sample buffer being played.
    pub sample: SampleBuffer,

    /// Current playback position in frames.
    pub frame_pos: usize,

    /// Volume multiplier for this voice (0.0 to 1.0).
    pub volume: f32,
}

impl Voice {
    /// Creates a new voice for playing a sample.
    ///
    /// # Parameters
    ///
    /// - `sample_id`: ID of the sample to play
    /// - `sample`: The sample buffer to play
    /// - `volume`: Volume multiplier (0.0 to 1.0)
    ///
    /// # Returns
    ///
    /// A new `Voice` instance with playback position set to 0.
    pub fn new(sample_id: usize, sample: SampleBuffer, volume: f32) -> Self {
        Self {
            sample_id,
            sample,
            frame_pos: 0,
            volume,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn test_voice_creation() {
        let sample = SampleBuffer {
            channels: 2,
            samples: Arc::from(vec![0.0, 0.0, 0.0, 0.0].into_boxed_slice()),
        };

        let voice = Voice::new(42, sample.clone(), 0.75);

        assert_eq!(voice.sample_id, 42);
        assert_eq!(voice.frame_pos, 0);
        assert!((voice.volume - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn test_voice_with_minimal_sample() {
        let sample = SampleBuffer {
            channels: 1,
            samples: Arc::from(vec![0.5].into_boxed_slice()),
        };

        let voice = Voice::new(0, sample, 1.0);

        assert_eq!(voice.sample_id, 0);
        assert_eq!(voice.frame_pos, 0);
        assert!((voice.volume - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_voice_with_multiple_channels() {
        let sample = SampleBuffer {
            channels: 4,
            samples: Arc::from(vec![0.1, 0.2, 0.3, 0.4].into_boxed_slice()),
        };

        let voice = Voice::new(10, sample, 0.5);

        assert_eq!(voice.sample_id, 10);
        assert_eq!(voice.frame_pos, 0);
        assert!((voice.volume - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_voice_zero_volume() {
        let sample = SampleBuffer {
            channels: 2,
            samples: Arc::from(vec![0.5, -0.5].into_boxed_slice()),
        };

        let voice = Voice::new(5, sample, 0.0);

        assert_eq!(voice.sample_id, 5);
        assert_eq!(voice.frame_pos, 0);
        assert!((voice.volume - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_multiple_voices_with_same_sample() {
        let sample = SampleBuffer {
            channels: 2,
            samples: Arc::from(vec![0.1, -0.1].into_boxed_slice()),
        };

        let voice1 = Voice::new(0, sample.clone(), 1.0);
        let voice2 = Voice::new(0, sample.clone(), 0.5);
        let voice3 = Voice::new(0, sample, 0.25);

        assert_eq!(voice1.sample_id, 0);
        assert_eq!(voice2.sample_id, 0);
        assert_eq!(voice3.sample_id, 0);
        assert!((voice1.volume - 1.0).abs() < f32::EPSILON);
        assert!((voice2.volume - 0.5).abs() < f32::EPSILON);
        assert!((voice3.volume - 0.25).abs() < f32::EPSILON);
    }
}
