//! Real-time audio mixer implementation.
//!
//! This module provides the [`RtMixer`] struct which handles real-time mixing
//! of multiple audio voices with sample loading, playback, and mixing capabilities.
//!
//! The mixer manages a collection of [`Voice`](crate::audio_engine::voice::Voice) instances
//! and operates on [`SampleBuffer`](crate::messages::SampleBuffer) data loaded via
//! [`decode_audio_file_to_sample_buffer`](crate::audio_engine::sample_loader::decode_audio_file_to_sample_buffer).

use crate::audio_engine::constants::{
    MAX_VOICES, NUM_SAMPLES, SPEED_MAX, SPEED_MIN, VOLUME_MAX, VOLUME_MIN,
};
use crate::audio_engine::voice::Voice;
use crate::messages::SampleBuffer;
use cpal::Sample;

/// Real-time mixer that handles sample loading and voice management.
///
/// The mixer maintains a sample bank with preloaded audio samples and manages
/// multiple concurrent voices for playback. All operations are designed to be
/// lock-free and real-time safe.
pub struct RtMixer {
    /// Number of output channels (1 for mono, 2 for stereo).
    channels: usize,

    /// Global volume multiplier.
    volume: f32,

    /// Global speed multiplier.
    speed: f32,

    /// Sample storage with NUM_SAMPLES slots.
    sample_bank: [Option<SampleBuffer>; NUM_SAMPLES],

    /// Active voices with MAX_VOICES slots.
    voices: [Option<Voice>; MAX_VOICES],
}

impl RtMixer {
    /// Creates a new RtMixer with the specified number of channels.
    ///
    /// # Parameters
    ///
    /// - `channels`: Number of output channels (1 for mono, 2 for stereo)
    ///
    /// # Returns
    ///
    /// A new `RtMixer` instance with empty sample bank and no active voices.
    pub fn new(channels: usize) -> Self {
        Self {
            channels,
            volume: VOLUME_MAX,
            speed: 1.0,
            sample_bank: std::array::from_fn(|_| None),
            voices: std::array::from_fn(|_| None),
        }
    }

    /// Loads a sample into the sample bank at the specified slot.
    ///
    /// # Parameters
    ///
    /// - `id`: Sample slot ID (0 to NUM_SAMPLES-1)
    /// - `sample`: Sample buffer to load
    ///
    /// # Safety
    ///
    /// The sample must have the same number of channels as the mixer.
    /// Invalid IDs are silently ignored.
    pub fn load_sample(&mut self, id: usize, sample: SampleBuffer) {
        if id >= NUM_SAMPLES {
            return;
        }

        if sample.channels != self.channels {
            return;
        }

        self.sample_bank[id] = Some(sample);
    }

    /// Starts playback of a loaded sample.
    ///
    /// # Parameters
    ///
    /// - `id`: Sample slot ID to play
    /// - `velocity`: Playback volume (0.0 to 1.0)
    ///
    /// If no free voice slot is available, the playback request is silently dropped.
    pub fn play_sample(&mut self, id: usize, velocity: f32) {
        if id >= NUM_SAMPLES {
            return;
        }

        if !velocity.is_finite() || !(VOLUME_MIN..=VOLUME_MAX).contains(&velocity) {
            return;
        }

        let Some(sample) = self.sample_bank[id].as_ref() else {
            return;
        };
        let sample = sample.clone();

        for voice_slot in &mut self.voices {
            if voice_slot.is_none() {
                *voice_slot = Some(Voice::new(id, sample, velocity));
                return;
            }
        }

        // No free voice slot: drop deterministically.
    }

    /// Stops all active voices.
    pub fn stop_all(&mut self) {
        for voice in &mut self.voices {
            *voice = None;
        }
    }

    /// Sets the global volume multiplier.
    ///
    /// # Parameters
    ///
    /// - `volume`: Volume multiplier (0.0 to 1.0)
    ///
    /// Invalid values (NaN, infinite, or out of range) are silently ignored.
    pub fn set_volume(&mut self, volume: f32) {
        if !volume.is_finite() || !(VOLUME_MIN..=VOLUME_MAX).contains(&volume) {
            return;
        }

        self.volume = volume;
    }

    /// Sets the global speed multiplier.
    ///
    /// # Parameters
    ///
    /// - `speed`: Speed multiplier
    ///
    /// Invalid values (NaN, infinite, or out of range) are silently ignored.
    pub fn set_speed(&mut self, speed: f32) {
        if !speed.is_finite() || !(SPEED_MIN..=SPEED_MAX).contains(&speed) {
            return;
        }

        self.speed = speed;
    }

    /// Stops all voices playing a specific sample.
    ///
    /// # Parameters
    ///
    /// - `id`: Sample slot ID to stop
    pub fn stop_sample(&mut self, id: usize) {
        if id >= NUM_SAMPLES {
            return;
        }

        for voice_slot in &mut self.voices {
            let should_stop = voice_slot
                .as_ref()
                .is_some_and(|voice| voice.sample_id == id);
            if should_stop {
                *voice_slot = None;
            }
        }
    }

    /// Unloads a sample from the sample bank.
    ///
    /// This stops all voices playing the sample and removes it from the bank.
    ///
    /// # Parameters
    ///
    /// - `id`: Sample slot ID to unload
    pub fn unload_sample(&mut self, id: usize) {
        if id >= NUM_SAMPLES {
            return;
        }

        self.stop_sample(id);
        self.sample_bank[id] = None;
    }

    /// Renders audio frames to the output buffer.
    ///
    /// Mixes all active voices into the output buffer. The output buffer must
    /// contain interleaved audio samples with `channels` per frame.
    ///
    /// # Parameters
    ///
    /// - `output`: Output buffer to fill with mixed audio samples
    pub fn render(&mut self, output: &mut [f32]) {
        output.fill(Sample::EQUILIBRIUM);

        if self.channels == 0 {
            return;
        }

        let frames = output.len() / self.channels;

        for frame_idx in 0..frames {
            let frame_base = frame_idx * self.channels;

            for voice_slot in &mut self.voices {
                let Some(voice) = voice_slot else {
                    continue;
                };

                let sample_frames = voice.sample.samples.len() / self.channels;
                if sample_frames == 0 {
                    *voice_slot = None;
                    continue;
                }

                if voice.frame_pos >= sample_frames {
                    voice.frame_pos = 0;
                }

                let sample_base = voice.frame_pos * self.channels;

                for channel in 0..self.channels {
                    output[frame_base + channel] +=
                        voice.sample.samples[sample_base + channel] * voice.volume * self.volume;
                }

                voice.frame_pos += 1;
                if voice.frame_pos >= sample_frames {
                    voice.frame_pos = 0;
                }
            }
        }
    }

    /// Gets the number of channels configured for this mixer.
    pub fn channels(&self) -> usize {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn create_test_sample(channels: usize, frames: usize, value: f32) -> SampleBuffer {
        let samples = vec![value; channels * frames];
        SampleBuffer {
            channels,
            samples: Arc::from(samples.into_boxed_slice()),
        }
    }

    #[test]
    fn test_mixer_creation() {
        let mixer = RtMixer::new(2);
        assert_eq!(mixer.channels(), 2);
    }

    #[test]
    fn test_load_sample() {
        let mut mixer = RtMixer::new(2);
        let sample = create_test_sample(2, 100, 0.5);

        mixer.load_sample(0, sample.clone());

        // Sample should be loaded
        assert!(mixer.sample_bank[0].is_some());
    }

    #[test]
    fn test_load_sample_invalid_id() {
        let mut mixer = RtMixer::new(2);
        let sample = create_test_sample(2, 100, 0.5);

        // Try to load at invalid ID
        mixer.load_sample(NUM_SAMPLES + 100, sample.clone());

        // Should not panic, but sample should not be loaded
        assert!(mixer.sample_bank[NUM_SAMPLES - 1].is_none());
    }

    #[test]
    fn test_load_sample_wrong_channels() {
        let mut mixer = RtMixer::new(2);
        let sample = create_test_sample(1, 100, 0.5);

        mixer.load_sample(0, sample);

        // Sample should not be loaded due to channel mismatch
        assert!(mixer.sample_bank[0].is_none());
    }

    #[test]
    fn test_play_sample() {
        let mut mixer = RtMixer::new(2);
        let sample = create_test_sample(2, 100, 0.5);
        mixer.load_sample(0, sample);

        mixer.play_sample(0, 0.8);

        // One voice should be active
        assert!(mixer.voices.iter().any(|v| v.is_some()));
    }

    #[test]
    fn test_play_sample_not_loaded() {
        let mut mixer = RtMixer::new(2);

        // Try to play sample that wasn't loaded
        mixer.play_sample(0, 0.8);

        // No voice should be created
        assert!(mixer.voices.iter().all(|v| v.is_none()));
    }

    #[test]
    fn test_play_sample_invalid_velocity() {
        let mut mixer = RtMixer::new(2);
        let sample = create_test_sample(2, 100, 0.5);
        mixer.load_sample(0, sample);

        // Try to play with invalid velocity
        mixer.play_sample(0, f32::NAN);
        mixer.play_sample(0, -1.0);
        mixer.play_sample(0, 2.0);

        // No voice should be created
        assert!(mixer.voices.iter().all(|v| v.is_none()));
    }

    #[test]
    fn test_stop_all() {
        let mut mixer = RtMixer::new(2);
        let sample = create_test_sample(2, 100, 0.5);
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 0.8);

        // Should have active voice
        assert!(mixer.voices.iter().any(|v| v.is_some()));

        mixer.stop_all();

        // All voices should be stopped
        assert!(mixer.voices.iter().all(|v| v.is_none()));
    }

    #[test]
    fn test_stop_sample() {
        let mut mixer = RtMixer::new(2);
        let sample1 = create_test_sample(2, 100, 0.5);
        let sample2 = create_test_sample(2, 100, 0.3);
        mixer.load_sample(0, sample1);
        mixer.load_sample(1, sample2);

        mixer.play_sample(0, 0.8);
        mixer.play_sample(1, 0.6);

        // Should have 2 active voices
        assert_eq!(mixer.voices.iter().filter(|v| v.is_some()).count(), 2);

        mixer.stop_sample(0);

        // Only sample 1 should be stopped, sample 2 should still play
        assert!(
            mixer
                .voices
                .iter()
                .any(|v| { v.as_ref().map_or(false, |voice| voice.sample_id == 1) })
        );
        assert!(
            mixer
                .voices
                .iter()
                .all(|v| { v.as_ref().map_or(true, |voice| voice.sample_id != 0) })
        );
    }

    #[test]
    fn test_unload_sample() {
        let mut mixer = RtMixer::new(2);
        let sample = create_test_sample(2, 100, 0.5);
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 0.8);

        // Should have loaded sample and active voice
        assert!(mixer.sample_bank[0].is_some());
        assert!(mixer.voices.iter().any(|v| v.is_some()));

        mixer.unload_sample(0);

        // Sample should be unloaded and voice stopped
        assert!(mixer.sample_bank[0].is_none());
        assert!(mixer.voices.iter().all(|v| v.is_none()));
    }

    #[test]
    fn test_render_silence() {
        let mut mixer = RtMixer::new(2);
        let mut output = vec![0.0; 200]; // 100 frames of stereo

        mixer.render(&mut output);

        // Output should be silence (all zeros)
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_render_with_voice() {
        let mut mixer = RtMixer::new(2);
        let sample = create_test_sample(2, 10, 0.5);
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 1.0);

        let mut output = vec![0.0; 20]; // 10 frames of stereo

        mixer.render(&mut output);

        // Output should contain sample data
        assert!(output.iter().any(|&s| s != 0.0));
    }

    #[test]
    fn test_render_loop_sample() {
        let mut mixer = RtMixer::new(1);
        let sample = create_test_sample(1, 5, 0.5);
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 1.0);

        // Render more frames than the sample contains
        let mut output = vec![0.0; 20]; // 20 frames of mono

        mixer.render(&mut output);

        // Sample should loop and all frames should have data
        assert!(output.iter().all(|&s| s == 0.5));
    }

    #[test]
    fn test_multiple_voices_mixing() {
        let mut mixer = RtMixer::new(2);
        let sample1 = create_test_sample(2, 10, 0.3);
        let sample2 = create_test_sample(2, 10, 0.2);
        mixer.load_sample(0, sample1);
        mixer.load_sample(1, sample2);

        mixer.play_sample(0, 1.0);
        mixer.play_sample(1, 1.0);

        let mut output = vec![0.0; 20]; // 10 frames of stereo

        mixer.render(&mut output);

        // Output should contain mixed samples (0.3 + 0.2 = 0.5 per channel)
        assert!(output.iter().all(|&s| (s - 0.5).abs() < f32::EPSILON));
    }

    #[test]
    fn test_voice_limit() {
        let mut mixer = RtMixer::new(1);

        // Create MAX_VOICES + 5 samples
        for i in 0..(MAX_VOICES + 5) {
            let sample = create_test_sample(1, 10, 0.5);
            mixer.load_sample(i, sample);
            mixer.play_sample(i, 1.0);
        }

        // Only MAX_VOICES voices should be active
        assert_eq!(
            mixer.voices.iter().filter(|v| v.is_some()).count(),
            MAX_VOICES
        );
    }
}
