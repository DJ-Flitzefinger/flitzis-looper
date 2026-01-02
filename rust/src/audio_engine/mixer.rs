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
use crate::audio_engine::stretch_processor::{DEFAULT_BLOCK_SAMPLES, StretchProcessor};
use crate::messages::SampleBuffer;
use cpal::Sample;

fn transpose_semitones_for_tempo_ratio(tempo_ratio: f32) -> f32 {
    if !tempo_ratio.is_finite() || tempo_ratio <= 0.0 {
        return 0.0;
    }

    -12.0 * tempo_ratio.log2()
}

struct VoiceSlot {
    active: bool,
    sample_id: usize,
    sample: Option<SampleBuffer>,
    frame_pos: usize,
    volume: f32,
    tempo_ratio_smoothed: f32,
    stretch: StretchProcessor,
}

impl VoiceSlot {
    fn new(channels: usize) -> Self {
        Self {
            active: false,
            sample_id: 0,
            sample: None,
            frame_pos: 0,
            volume: 0.0,
            tempo_ratio_smoothed: 1.0,
            stretch: StretchProcessor::new(channels),
        }
    }

    fn start(
        &mut self,
        sample_id: usize,
        sample: SampleBuffer,
        volume: f32,
        initial_tempo_ratio: f32,
    ) {
        self.active = true;
        self.sample_id = sample_id;
        self.sample = Some(sample);
        self.frame_pos = 0;
        self.volume = volume;
        self.tempo_ratio_smoothed = initial_tempo_ratio;
    }

    fn stop(&mut self) {
        self.active = false;
        self.sample = None;
        self.frame_pos = 0;
        self.volume = 0.0;
        self.tempo_ratio_smoothed = 1.0;
    }

    fn smooth_tempo_ratio(&mut self, target: f32) -> f32 {
        if !target.is_finite() {
            return self.tempo_ratio_smoothed;
        }

        let mut target = target.clamp(SPEED_MIN, SPEED_MAX);
        if !self.tempo_ratio_smoothed.is_finite() {
            self.tempo_ratio_smoothed = target;
            return self.tempo_ratio_smoothed;
        }

        let max_step = 0.05;
        let delta = (target - self.tempo_ratio_smoothed).clamp(-max_step, max_step);
        self.tempo_ratio_smoothed = (self.tempo_ratio_smoothed + delta).clamp(SPEED_MIN, SPEED_MAX);
        target = self.tempo_ratio_smoothed;

        target
    }

    fn is_playing_sample(&self, sample_id: usize) -> bool {
        self.active && self.sample_id == sample_id
    }
}

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

    /// Enable BPM lock (tempo matching).
    bpm_lock_enabled: bool,

    /// Enable key lock (preserve pitch when tempo changes).
    key_lock_enabled: bool,

    /// Current master BPM when BPM lock is enabled.
    master_bpm: Option<f32>,

    /// Effective pad BPM metadata (manual override or analysis).
    pad_bpm: [Option<f32>; NUM_SAMPLES],

    /// Sample storage with NUM_SAMPLES slots.
    sample_bank: [Option<SampleBuffer>; NUM_SAMPLES],

    /// Active voices with MAX_VOICES slots.
    voices: [VoiceSlot; MAX_VOICES],
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
            bpm_lock_enabled: false,
            key_lock_enabled: false,
            master_bpm: None,
            pad_bpm: std::array::from_fn(|_| None),
            sample_bank: std::array::from_fn(|_| None),
            voices: std::array::from_fn(|_| VoiceSlot::new(channels)),
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

        let tempo_ratio = self.tempo_ratio_for_sample_id(id);
        for voice_slot in &mut self.voices {
            if !voice_slot.active {
                voice_slot.start(id, sample.clone(), velocity, tempo_ratio);
                return;
            }
        }

        // No free voice slot: drop deterministically.
    }

    /// Stops all active voices.
    pub fn stop_all(&mut self) {
        for voice in &mut self.voices {
            voice.stop();
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

    pub fn set_bpm_lock(&mut self, enabled: bool) {
        self.bpm_lock_enabled = enabled;
        if !enabled {
            self.master_bpm = None;
        }
    }

    pub fn set_key_lock(&mut self, enabled: bool) {
        self.key_lock_enabled = enabled;
    }

    pub fn set_master_bpm(&mut self, bpm: f32) {
        if !bpm.is_finite() || bpm <= 0.0 {
            return;
        }

        self.master_bpm = Some(bpm);
    }

    pub fn set_pad_bpm(&mut self, id: usize, bpm: Option<f32>) {
        if id >= NUM_SAMPLES {
            return;
        }

        let bpm = bpm.and_then(|value| {
            if !value.is_finite() || value <= 0.0 {
                None
            } else {
                Some(value)
            }
        });

        self.pad_bpm[id] = bpm;
    }

    fn tempo_ratio_for_sample_id(&self, sample_id: usize) -> f32 {
        let mut ratio = self.speed;

        if self.bpm_lock_enabled {
            if let (Some(master_bpm), Some(pad_bpm)) = (self.master_bpm, self.pad_bpm[sample_id]) {
                ratio = master_bpm / pad_bpm;
            }
        }

        if !ratio.is_finite() {
            ratio = 1.0;
        }

        ratio.clamp(SPEED_MIN, SPEED_MAX)
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
            if voice_slot.is_playing_sample(id) {
                voice_slot.stop();
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
        if frames == 0 {
            return;
        }

        let speed = self.speed;
        let bpm_lock_enabled = self.bpm_lock_enabled;
        let key_lock_enabled = self.key_lock_enabled;
        let master_bpm = self.master_bpm;
        let pad_bpm = self.pad_bpm;

        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }

            let Some(sample) = voice.sample.clone() else {
                voice.stop();
                continue;
            };

            let sample_frames = sample.samples.len() / self.channels;
            if sample_frames == 0 {
                voice.stop();
                continue;
            }

            let mut target_tempo_ratio = speed;
            if bpm_lock_enabled {
                if let (Some(master_bpm), Some(pad_bpm)) = (master_bpm, pad_bpm[voice.sample_id]) {
                    target_tempo_ratio = master_bpm / pad_bpm;
                }
            }

            if !target_tempo_ratio.is_finite() {
                target_tempo_ratio = 1.0;
            }
            target_tempo_ratio = target_tempo_ratio.clamp(SPEED_MIN, SPEED_MAX);

            let tempo_ratio = voice.smooth_tempo_ratio(target_tempo_ratio);
            let transpose_semitones = if key_lock_enabled {
                transpose_semitones_for_tempo_ratio(tempo_ratio)
            } else {
                0.0
            };

            let mut input_frames = ((frames as f32) * tempo_ratio).round() as usize;
            input_frames = input_frames.clamp(1, DEFAULT_BLOCK_SAMPLES);

            let input_buffers = voice.stretch.input_buffers_mut(input_frames);
            for channel in 0..self.channels {
                let buf = &mut input_buffers[channel];
                for i in 0..input_frames {
                    let frame = (voice.frame_pos + i) % sample_frames;
                    buf[i] = sample.samples[frame * self.channels + channel];
                }
            }

            voice.stretch.set_transpose_semitones(transpose_semitones);
            voice.stretch.process(input_frames, frames);

            let output_buffers = voice.stretch.output_buffers();
            for frame in 0..frames {
                let out_base = frame * self.channels;
                for channel in 0..self.channels {
                    output[out_base + channel] +=
                        output_buffers[channel][frame] * voice.volume * self.volume;
                }
            }

            voice.frame_pos = (voice.frame_pos + input_frames) % sample_frames;
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
    fn test_transpose_semitones_for_tempo_ratio() {
        assert!((transpose_semitones_for_tempo_ratio(1.0) - 0.0).abs() < 1e-6);
        assert!((transpose_semitones_for_tempo_ratio(2.0) - (-12.0)).abs() < 1e-6);
        assert!((transpose_semitones_for_tempo_ratio(0.5) - 12.0).abs() < 1e-6);
    }

    #[test]
    fn test_tempo_ratio_for_sample_id_speed_only() {
        let mut mixer = RtMixer::new(2);
        mixer.set_speed(1.25);

        let ratio = mixer.tempo_ratio_for_sample_id(0);
        assert!((ratio - 1.25).abs() < 1e-6);
    }

    #[test]
    fn test_tempo_ratio_for_sample_id_bpm_lock() {
        let mut mixer = RtMixer::new(2);
        mixer.set_speed(1.0);
        mixer.set_bpm_lock(true);
        mixer.set_master_bpm(120.0);
        mixer.set_pad_bpm(0, Some(90.0));

        let ratio = mixer.tempo_ratio_for_sample_id(0);
        assert!((ratio - (120.0 / 90.0)).abs() < 1e-6);

        mixer.set_pad_bpm(0, None);
        let ratio = mixer.tempo_ratio_for_sample_id(0);
        assert!((ratio - 1.0).abs() < 1e-6);
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
        assert!(mixer.voices.iter().any(|v| v.active));
    }

    #[test]
    fn test_play_sample_not_loaded() {
        let mut mixer = RtMixer::new(2);

        // Try to play sample that wasn't loaded
        mixer.play_sample(0, 0.8);

        // No voice should be created
        assert!(mixer.voices.iter().all(|v| !v.active));
    }

    #[test]
    fn test_stop_all() {
        let mut mixer = RtMixer::new(2);
        let sample = create_test_sample(2, 100, 0.5);
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 0.8);

        // Should have active voice
        assert!(mixer.voices.iter().any(|v| v.active));

        mixer.stop_all();

        // All voices should be stopped
        assert!(mixer.voices.iter().all(|v| !v.active));
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
        assert_eq!(mixer.voices.iter().filter(|v| v.active).count(), 2);

        mixer.stop_sample(0);

        // Only sample 1 should be stopped, sample 2 should still play
        assert!(mixer.voices.iter().any(|v| v.active && v.sample_id == 1));
        assert!(mixer.voices.iter().all(|v| !v.active || v.sample_id != 0));
    }

    #[test]
    fn test_unload_sample() {
        let mut mixer = RtMixer::new(2);
        let sample = create_test_sample(2, 100, 0.5);
        mixer.load_sample(0, sample);
        mixer.play_sample(0, 0.8);

        // Should have loaded sample and active voice
        assert!(mixer.sample_bank[0].is_some());
        assert!(mixer.voices.iter().any(|v| v.active));

        mixer.unload_sample(0);

        // Sample should be unloaded and voice stopped
        assert!(mixer.sample_bank[0].is_none());
        assert!(mixer.voices.iter().all(|v| !v.active));
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
    fn test_speed_changes_affect_render_output() {
        let samples: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let sample = SampleBuffer {
            channels: 1,
            samples: Arc::from(samples.into_boxed_slice()),
        };

        let mut mixer = RtMixer::new(1);
        mixer.load_sample(0, sample.clone());

        mixer.set_speed(1.0);
        mixer.play_sample(0, 1.0);
        let mut output_1x = vec![0.0; 20];
        mixer.render(&mut output_1x);

        mixer.stop_all();
        mixer.set_speed(2.0);
        mixer.play_sample(0, 1.0);
        let mut output_2x = vec![0.0; 20];
        mixer.render(&mut output_2x);

        assert!(
            output_1x
                .iter()
                .zip(&output_2x)
                .any(|(a, b)| (*a - *b).abs() > 1e-6)
        );
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
        assert_eq!(mixer.voices.iter().filter(|v| v.active).count(), MAX_VOICES);
    }
}
