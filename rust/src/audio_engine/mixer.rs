//! Real-time audio mixer implementation.
//!
//! This module provides the [`RtMixer`] struct which handles real-time mixing
//! of multiple audio voices with sample loading, playback, and mixing capabilities.
//!
//! The mixer manages a collection of [`Voice`](crate::audio_engine::voice::Voice) instances
//! and operates on [`SampleBuffer`](crate::messages::SampleBuffer) data loaded via
//! [`decode_audio_file_to_sample_buffer`](crate::audio_engine::sample_loader::decode_audio_file_to_sample_buffer).

use crate::audio_engine::constants::{
    MAX_VOICES, NUM_SAMPLES, PAD_EQ_DB_MAX, PAD_EQ_DB_MIN, PAD_GAIN_MAX, PAD_GAIN_MIN, SPEED_MAX,
    SPEED_MIN, VOLUME_MAX, VOLUME_MIN,
};
use crate::audio_engine::stretch_processor::{DEFAULT_BLOCK_SAMPLES, StretchProcessor};
use crate::messages::SampleBuffer;
use cpal::Sample;
use std::f32::consts::PI;

fn transpose_semitones_for_tempo_ratio(tempo_ratio: f32) -> f32 {
    if !tempo_ratio.is_finite() || tempo_ratio <= 0.0 {
        return 0.0;
    }

    -12.0 * tempo_ratio.log2()
}

#[derive(Clone, Copy)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl BiquadCoeffs {
    fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

fn biquad_process(coeffs: BiquadCoeffs, state: &mut BiquadState, x: f32) -> f32 {
    let y = coeffs.b0 * x + state.z1;
    state.z1 = coeffs.b1 * x - coeffs.a1 * y + state.z2;
    state.z2 = coeffs.b2 * x - coeffs.a2 * y;
    y
}

#[derive(Clone, Copy)]
struct Eq3Coeffs {
    low: BiquadCoeffs,
    mid: BiquadCoeffs,
    high: BiquadCoeffs,
}

impl Eq3Coeffs {
    fn identity() -> Self {
        Self {
            low: BiquadCoeffs::identity(),
            mid: BiquadCoeffs::identity(),
            high: BiquadCoeffs::identity(),
        }
    }

    fn process(&self, state: &mut Eq3State, mut x: f32) -> f32 {
        x = biquad_process(self.low, &mut state.low, x);
        x = biquad_process(self.mid, &mut state.mid, x);
        x = biquad_process(self.high, &mut state.high, x);
        x
    }
}

#[derive(Clone, Copy, Default)]
struct Eq3State {
    low: BiquadState,
    mid: BiquadState,
    high: BiquadState,
}

impl Eq3State {
    fn reset(&mut self) {
        *self = Self::default();
    }
}

fn db_to_a(db: f32) -> f32 {
    if !db.is_finite() {
        return 1.0;
    }
    10.0_f32.powf(db / 40.0)
}

fn clamp_freq_hz(fs_hz: f32, freq_hz: f32) -> f32 {
    if !fs_hz.is_finite() || fs_hz <= 0.0 {
        return freq_hz.max(1.0);
    }

    let nyquist = fs_hz * 0.5;
    let max_hz = (nyquist * 0.9).max(1.0);
    freq_hz.clamp(1.0, max_hz)
}

fn normalize_biquad(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> BiquadCoeffs {
    if !a0.is_finite() || a0.abs() < 1e-12 {
        return BiquadCoeffs::identity();
    }

    let inv_a0 = 1.0 / a0;
    let coeffs = BiquadCoeffs {
        b0: b0 * inv_a0,
        b1: b1 * inv_a0,
        b2: b2 * inv_a0,
        a1: a1 * inv_a0,
        a2: a2 * inv_a0,
    };

    if [coeffs.b0, coeffs.b1, coeffs.b2, coeffs.a1, coeffs.a2]
        .iter()
        .all(|v| v.is_finite())
    {
        coeffs
    } else {
        BiquadCoeffs::identity()
    }
}

fn biquad_low_shelf(fs_hz: f32, freq_hz: f32, db_gain: f32) -> BiquadCoeffs {
    let freq_hz = clamp_freq_hz(fs_hz, freq_hz);
    let a = db_to_a(db_gain);
    let w0 = 2.0 * PI * freq_hz / fs_hz;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / 2.0 * 2.0_f32.sqrt();

    let sqrt_a = a.sqrt();

    let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha);
    let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
    let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha);
    let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
    let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
    let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha;

    normalize_biquad(b0, b1, b2, a0, a1, a2)
}

fn biquad_high_shelf(fs_hz: f32, freq_hz: f32, db_gain: f32) -> BiquadCoeffs {
    let freq_hz = clamp_freq_hz(fs_hz, freq_hz);
    let a = db_to_a(db_gain);
    let w0 = 2.0 * PI * freq_hz / fs_hz;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / 2.0 * 2.0_f32.sqrt();

    let sqrt_a = a.sqrt();

    let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha);
    let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
    let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha);
    let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
    let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
    let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha;

    normalize_biquad(b0, b1, b2, a0, a1, a2)
}

fn biquad_peaking(fs_hz: f32, freq_hz: f32, q: f32, db_gain: f32) -> BiquadCoeffs {
    let freq_hz = clamp_freq_hz(fs_hz, freq_hz);
    let a = db_to_a(db_gain);
    let q = if q.is_finite() && q > 0.0 { q } else { 0.707 };

    let w0 = 2.0 * PI * freq_hz / fs_hz;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * q);

    let b0 = 1.0 + alpha * a;
    let b1 = -2.0 * cos_w0;
    let b2 = 1.0 - alpha * a;
    let a0 = 1.0 + alpha / a;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha / a;

    normalize_biquad(b0, b1, b2, a0, a1, a2)
}

fn coeffs_for_eq3(fs_hz: f32, low_db: f32, mid_db: f32, high_db: f32) -> Eq3Coeffs {
    if !fs_hz.is_finite() || fs_hz <= 0.0 {
        return Eq3Coeffs::identity();
    }

    Eq3Coeffs {
        low: biquad_low_shelf(fs_hz, 250.0, low_db),
        mid: biquad_peaking(fs_hz, 1_000.0, 0.5, mid_db),
        high: biquad_high_shelf(fs_hz, 3_000.0, high_db),
    }
}

struct VoiceSlot {
    active: bool,
    sample_id: usize,
    sample: Option<SampleBuffer>,
    frame_pos: usize,
    volume: f32,
    tempo_ratio_smoothed: f32,
    stretch: StretchProcessor,
    eq_state: Vec<Eq3State>,
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
            eq_state: vec![Eq3State::default(); channels],
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
        for state in &mut self.eq_state {
            state.reset();
        }
    }

    fn stop(&mut self) {
        self.active = false;
        self.sample = None;
        self.frame_pos = 0;
        self.volume = 0.0;
        self.tempo_ratio_smoothed = 1.0;
        for state in &mut self.eq_state {
            state.reset();
        }
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

    /// Output sample rate in Hz.
    sample_rate_hz: f32,

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

    /// Per-pad gain scalar (linear, 0.0..=1.0).
    pad_gain: [f32; NUM_SAMPLES],

    /// Per-pad EQ coefficients (low/mid/high).
    pad_eq: [Eq3Coeffs; NUM_SAMPLES],

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
    pub fn new(channels: usize, sample_rate_hz: f32) -> Self {
        let sample_rate_hz = if sample_rate_hz.is_finite() && sample_rate_hz > 0.0 {
            sample_rate_hz
        } else {
            44_100.0
        };

        Self {
            channels,
            sample_rate_hz,
            volume: VOLUME_MAX,
            speed: 1.0,
            bpm_lock_enabled: false,
            key_lock_enabled: false,
            master_bpm: None,
            pad_bpm: std::array::from_fn(|_| None),
            pad_gain: std::array::from_fn(|_| 1.0),
            pad_eq: std::array::from_fn(|_| Eq3Coeffs::identity()),
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

    pub fn set_pad_gain(&mut self, id: usize, gain: f32) {
        if id >= NUM_SAMPLES {
            return;
        }

        if !gain.is_finite() || !(PAD_GAIN_MIN..=PAD_GAIN_MAX).contains(&gain) {
            return;
        }

        self.pad_gain[id] = gain;
    }

    pub fn set_pad_eq(&mut self, id: usize, low_db: f32, mid_db: f32, high_db: f32) {
        if id >= NUM_SAMPLES {
            return;
        }

        let all = [low_db, mid_db, high_db];
        if all
            .iter()
            .any(|v| !v.is_finite() || !(PAD_EQ_DB_MIN..=PAD_EQ_DB_MAX).contains(v))
        {
            return;
        }

        self.pad_eq[id] = coeffs_for_eq3(self.sample_rate_hz, low_db, mid_db, high_db);
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
    fn render_inner(&mut self, output: &mut [f32], mut pad_peaks: Option<&mut [f32; NUM_SAMPLES]>) {
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
        let pad_bpm = &self.pad_bpm;
        let pad_gain = &self.pad_gain;
        let pad_eq = &self.pad_eq;

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

            let eq = pad_eq[voice.sample_id];
            let pad_gain = pad_gain[voice.sample_id];

            let output_buffers = voice.stretch.output_buffers();
            for frame in 0..frames {
                let out_base = frame * self.channels;
                for channel in 0..self.channels {
                    let mut sample = output_buffers[channel][frame];
                    if let Some(state) = voice.eq_state.get_mut(channel) {
                        sample = eq.process(state, sample);
                    }
                    let mixed = sample * voice.volume * self.volume * pad_gain;
                    output[out_base + channel] += mixed;
                    if let Some(peaks) = pad_peaks.as_deref_mut() {
                        let peak = mixed.abs();
                        if peak > peaks[voice.sample_id] {
                            peaks[voice.sample_id] = peak;
                        }
                    }
                }
            }

            voice.frame_pos = (voice.frame_pos + input_frames) % sample_frames;
        }
    }

    pub fn render(&mut self, output: &mut [f32]) {
        self.render_inner(output, None);
    }

    pub fn render_with_peaks(&mut self, output: &mut [f32], pad_peaks: &mut [f32; NUM_SAMPLES]) {
        pad_peaks.fill(0.0);
        self.render_inner(output, Some(pad_peaks));
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
        let mixer = RtMixer::new(2, 44_100.0);
        assert_eq!(mixer.channels(), 2);
    }

    #[test]
    fn test_transpose_semitones_for_tempo_ratio() {
        assert!((transpose_semitones_for_tempo_ratio(1.0) - 0.0).abs() < 1e-6);
        assert!((transpose_semitones_for_tempo_ratio(2.0) - (-12.0)).abs() < 1e-6);
        assert!((transpose_semitones_for_tempo_ratio(0.5) - 12.0).abs() < 1e-6);
    }

    #[test]
    fn test_eq3_coeffs_finite() {
        let eq = coeffs_for_eq3(44_100.0, 6.0, -3.0, 0.0);
        for v in [
            eq.low.b0, eq.low.b1, eq.low.b2, eq.low.a1, eq.low.a2, eq.mid.b0, eq.mid.b1, eq.mid.b2,
            eq.mid.a1, eq.mid.a2, eq.high.b0, eq.high.b1, eq.high.b2, eq.high.a1, eq.high.a2,
        ] {
            assert!(v.is_finite());
        }
    }

    #[test]
    fn test_eq3_processing_stable_on_impulse() {
        let eq = coeffs_for_eq3(44_100.0, 0.0, 6.0, 0.0);
        let mut state = Eq3State::default();
        let mut max_abs: f32 = 0.0;
        for i in 0..512 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let y = eq.process(&mut state, x);
            assert!(y.is_finite());
            max_abs = max_abs.max(y.abs());
        }
        assert!(max_abs > 0.0);
    }

    #[test]
    fn test_tempo_ratio_for_sample_id_speed_only() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        mixer.set_speed(1.25);

        let ratio = mixer.tempo_ratio_for_sample_id(0);
        assert!((ratio - 1.25).abs() < 1e-6);
    }

    #[test]
    fn test_tempo_ratio_for_sample_id_bpm_lock() {
        let mut mixer = RtMixer::new(2, 44_100.0);
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
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample = create_test_sample(2, 100, 0.5);

        mixer.load_sample(0, sample.clone());

        // Sample should be loaded
        assert!(mixer.sample_bank[0].is_some());
    }

    #[test]
    fn test_load_sample_invalid_id() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample = create_test_sample(2, 100, 0.5);

        // Try to load at invalid ID
        mixer.load_sample(NUM_SAMPLES + 100, sample.clone());

        // Should not panic, but sample should not be loaded
        assert!(mixer.sample_bank[NUM_SAMPLES - 1].is_none());
    }

    #[test]
    fn test_load_sample_wrong_channels() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample = create_test_sample(1, 100, 0.5);

        mixer.load_sample(0, sample);

        // Sample should not be loaded due to channel mismatch
        assert!(mixer.sample_bank[0].is_none());
    }

    #[test]
    fn test_play_sample() {
        let mut mixer = RtMixer::new(2, 44_100.0);
        let sample = create_test_sample(2, 100, 0.5);
        mixer.load_sample(0, sample);

        mixer.play_sample(0, 0.8);

        // One voice should be active
        assert!(mixer.voices.iter().any(|v| v.active));
    }

    #[test]
    fn test_play_sample_not_loaded() {
        let mut mixer = RtMixer::new(2, 44_100.0);

        // Try to play sample that wasn't loaded
        mixer.play_sample(0, 0.8);

        // No voice should be created
        assert!(mixer.voices.iter().all(|v| !v.active));
    }

    #[test]
    fn test_stop_all() {
        let mut mixer = RtMixer::new(2, 44_100.0);
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
        let mut mixer = RtMixer::new(2, 44_100.0);
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
        let mut mixer = RtMixer::new(2, 44_100.0);
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
        let mut mixer = RtMixer::new(2, 44_100.0);
        let mut output = vec![0.0; 200]; // 100 frames of stereo

        mixer.render(&mut output);

        // Output should be silence (all zeros)
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_render_with_voice() {
        let mut mixer = RtMixer::new(2, 44_100.0);
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

        let mut mixer = RtMixer::new(1, 44_100.0);
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
        let mut mixer = RtMixer::new(1, 44_100.0);
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
        let mut mixer = RtMixer::new(2, 44_100.0);
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
    fn test_pad_gain_applies_to_render() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        let sample = create_test_sample(1, 5, 0.8);
        mixer.load_sample(0, sample);
        mixer.set_pad_gain(0, 0.25);
        mixer.play_sample(0, 1.0);

        let mut output = vec![0.0; 20]; // 20 frames of mono
        mixer.render(&mut output);

        let expected = 0.8 * 0.25;
        assert!(output.iter().all(|&s| (s - expected).abs() < 1e-6));
    }

    #[test]
    fn test_voice_limit() {
        let mut mixer = RtMixer::new(1, 44_100.0);

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
