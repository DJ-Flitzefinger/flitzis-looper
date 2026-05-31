//! Onset Detection Function (ODF) computation.
//!
//! Implements Complex Spectral Difference (ComplexSD) method from qm-dsp.
//! Uses Hann-windowed RFFT per frame, extracts magnitude and phase, and
//! computes phase deviation (second-order phase derivative).

use crate::math_utils::principal_arg;
use crate::phase_vocoder::PhaseVocoder;
use crate::window::{apply_window, hann_window};

use crate::AnalysisConfig;

/// Onset Detection Function processor.
pub struct DetectionFunction {
    /// Phase vocoder for FFT + phase tracking.
    phase_vocoder: PhaseVocoder,
    /// Previous magnitude history.
    prev_magnitude: Vec<f64>,
    /// Previous instantaneous phase.
    prev_phase: Vec<f64>,
    /// Previous-prev instantaneous phase.
    prev_prev_phase: Vec<f64>,
    /// Hann window.
    window: Vec<f64>,
    /// Frame length in samples.
    frame_length: usize,
    /// Hop size in samples.
    step_size: usize,
    /// Number of frequency bins (frame_length/2 + 1).
    half_length: usize,
}

impl DetectionFunction {
    /// Create a new DetectionFunction with the given sample rate and config.
    pub fn new(sample_rate_hz: u32, config: &AnalysisConfig) -> Self {
        // Frame length: next power of 2 of sampleRate / maxBinHz
        let frame_length = next_power_of_two((sample_rate_hz as f64 / config.max_bin_hz) as usize);
        // Step size: sampleRate * stepSecs
        let step_size = (sample_rate_hz as f64 * config.step_secs) as usize;
        let half_length = frame_length / 2 + 1;

        Self {
            phase_vocoder: PhaseVocoder::new(frame_length, step_size),
            prev_magnitude: vec![0.0; half_length],
            prev_phase: vec![0.0; half_length],
            prev_prev_phase: vec![0.0; half_length],
            window: hann_window(frame_length),
            frame_length,
            step_size,
            half_length,
        }
    }

    /// Process the entire audio buffer and return the ODF values.
    pub fn process(&mut self, audio: &[f64]) -> Vec<f64> {
        let total_frames = self.num_frames(audio.len());
        let mut odf = Vec::with_capacity(total_frames);

        for frame_idx in 0..total_frames {
            let start = (frame_idx * self.step_size).min(audio.len());
            let end = (start + self.frame_length).min(audio.len());

            // Extract frame and zero-pad if needed
            let mut frame = vec![0.0; self.frame_length];
            let copy_len = end - start;
            if copy_len > 0 {
                frame[..copy_len].copy_from_slice(&audio[start..end]);
            }

            // Apply Hann window
            apply_window(&mut frame, &self.window);

            // Process through phase vocoder
            let (magnitude, phase, _unwrapped) = self.phase_vocoder.process_time_domain(&frame);

            // Compute Complex Spectral Difference
            let df_value = self.complex_sd(&magnitude, &phase);
            odf.push(df_value);
        }

        odf
    }

    /// Compute Complex Spectral Difference for a single frame.
    fn complex_sd(&mut self, magnitude: &[f64], phase: &[f64]) -> f64 {
        let mut val = 0.0;

        for i in 0..self.half_length {
            // Phase deviation (second-order phase derivative)
            let tmp_phase = phase[i] - 2.0 * self.prev_phase[i] + self.prev_prev_phase[i];
            let dev = principal_arg(tmp_phase);

            // Complex spectral difference:
            // meas = prev_magnitude[i] - magnitude[i] * exp(j * dev)
            let cos_dev = dev.cos();
            let sin_dev = dev.sin();
            let real_part = self.prev_magnitude[i] - magnitude[i] * cos_dev;
            let imag_part = -magnitude[i] * sin_dev;

            val += (real_part * real_part + imag_part * imag_part).sqrt();

            // Update history
            self.prev_prev_phase[i] = self.prev_phase[i];
            self.prev_phase[i] = phase[i];
            self.prev_magnitude[i] = magnitude[i];
        }

        val
    }

    /// Calculate the number of frames for the given audio length.
    fn num_frames(&self, audio_len: usize) -> usize {
        if audio_len < self.frame_length {
            return 1;
        }
        ((audio_len - self.frame_length) as f64 / self.step_size as f64).ceil() as usize + 1
    }
}

/// Return the next higher integer power of two from x (or x if already a power of two).
fn next_power_of_two(x: usize) -> usize {
    if x <= 1 {
        return 1;
    }
    if x.is_power_of_two() {
        return x;
    }
    let mut n = 1;
    while n < x {
        n <<= 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn silent_audio(sample_rate: u32, duration_secs: usize) -> Vec<f64> {
        vec![0.0; (sample_rate as usize) * duration_secs]
    }

    fn click_track(sample_rate: u32, bpm: f64, beats: usize) -> Vec<f64> {
        let interval_s = 60.0 / bpm;
        let duration_s = interval_s * beats as f64 + 1.0;
        let mut audio = vec![0.0; (duration_s * sample_rate as f64).ceil() as usize];
        for beat in 0..beats {
            let start = (interval_s * beat as f64 * sample_rate as f64).round() as usize;
            for offset in 0..64 {
                let idx = start + offset;
                if idx < audio.len() {
                    audio[idx] = 1.0 - offset as f64 / 64.0;
                }
            }
        }
        audio
    }

    #[test]
    fn odf_silent_input_produces_near_zero() {
        let config = AnalysisConfig::default();
        let audio = silent_audio(44100, 2);
        let mut df = DetectionFunction::new(44100, &config);
        let odf = df.process(&audio);

        assert!(!odf.is_empty());
        for v in &odf {
            assert!(
                *v < 0.01,
                "ODF value {} should be near zero for silent audio",
                v
            );
        }
    }

    #[test]
    fn odf_click_track_produces_onsets() {
        let config = AnalysisConfig::default();
        let audio = click_track(44100, 120.0, 20);
        let mut df = DetectionFunction::new(44100, &config);
        let odf = df.process(&audio);

        assert!(!odf.is_empty());
        // Should have some non-zero values (onsets from clicks)
        let non_zero = odf.iter().filter(|v| **v > 0.01).count();
        assert!(non_zero > 0, "Expected onsets in click track ODF");
    }

    #[test]
    fn next_power_of_two_correct() {
        assert_eq!(next_power_of_two(1), 1);
        assert_eq!(next_power_of_two(2), 2);
        assert_eq!(next_power_of_two(3), 4);
        assert_eq!(next_power_of_two(1300), 2048);
        assert_eq!(next_power_of_two(2048), 2048);
    }
}
