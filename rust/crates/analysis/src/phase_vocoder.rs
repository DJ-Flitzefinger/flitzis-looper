//! Phase vocoder: FFT + phase tracking for the DetectionFunction.
//!
//! Uses `rustfft` for FFT. Provides magnitude, instantaneous
//! phase, and unwrapped phase extraction.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use std::sync::Arc;

use crate::math_utils::principal_arg;

/// Phase vocoder for frame-by-frame FFT and phase tracking.
pub struct PhaseVocoder {
    /// Frame size (must be power of 2).
    n: usize,
    /// Hop size (frame increment in samples).
    hop: usize,
    /// FFT plan for forward transform.
    fft_plan: Arc<dyn rustfft::Fft<f64>>,
    /// Complex buffer for FFT input/output.
    complex_buf: Vec<Complex<f64>>,
    /// Previous instantaneous phase per bin.
    prev_phase: Vec<f64>,
    /// Previous unwrapped phase per bin.
    prev_unwrapped: Vec<f64>,
    /// Number of frequency bins (n/2 + 1).
    half_n: usize,
}

impl PhaseVocoder {
    /// Create a new PhaseVocoder with the given frame size and hop size.
    pub fn new(n: usize, hop: usize) -> Self {
        let half_n = n / 2 + 1;

        let mut planner = FftPlanner::<f64>::new();
        let fft_plan = planner.plan_fft_forward(n);

        let mut prev_phase = vec![0.0; half_n];
        let mut prev_unwrapped = vec![0.0; half_n];

        // Initialize to one step behind so that a signal with initial phase
        // at zero matches the expected values.
        for i in 0..half_n {
            let omega = (2.0 * std::f64::consts::PI * hop as f64 * i as f64) / n as f64;
            prev_phase[i] = -omega;
            prev_unwrapped[i] = -omega;
        }

        Self {
            n,
            hop,
            fft_plan,
            complex_buf: vec![Complex::new(0.0, 0.0); n],
            prev_phase,
            prev_unwrapped,
            half_n,
        }
    }

    /// Process a time-domain frame (already windowed by the caller).
    ///
    /// Returns `(magnitude, phase, unwrapped)` each of length `n/2 + 1`.
    pub fn process_time_domain(&mut self, src: &[f64]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        // Apply FFT shift: swap first and second halves (matching qm-dsp behavior)
        let half = self.n / 2;
        for i in 0..self.n {
            let shifted_idx = if i < half { i + half } else { i - half };
            let real_val = if shifted_idx < src.len() {
                src[shifted_idx]
            } else {
                0.0
            };
            self.complex_buf[i] = Complex::new(real_val, 0.0);
        }

        // Perform FFT
        self.fft_plan.process(&mut self.complex_buf);

        // Extract magnitudes and phases from first half_n bins
        let mag = self.get_magnitudes();
        let phase = self.get_phases();
        let unwrapped = self.unwrap_phases(&phase);

        (mag, phase, unwrapped)
    }

    /// Extract magnitudes from FFT output.
    fn get_magnitudes(&self) -> Vec<f64> {
        (0..self.half_n)
            .map(|i| self.complex_buf[i].norm_sqr().sqrt())
            .collect()
    }

    /// Extract instantaneous phases from FFT output.
    fn get_phases(&self) -> Vec<f64> {
        (0..self.half_n)
            .map(|i| self.complex_buf[i].arg())
            .collect()
    }

    /// Unwrap phases using previous phase history.
    fn unwrap_phases(&mut self, theta: &[f64]) -> Vec<f64> {
        let mut unwrapped = Vec::with_capacity(self.half_n);

        #[allow(clippy::needless_range_loop)]
        for i in 0..self.half_n {
            let omega = (2.0 * std::f64::consts::PI * self.hop as f64 * i as f64) / self.n as f64;
            let expected = self.prev_phase[i] + omega;
            let error = principal_arg(theta[i] - expected);

            let unwrapped_val = self.prev_unwrapped[i] + omega + error;

            self.prev_phase[i] = theta[i];
            self.prev_unwrapped[i] = unwrapped_val;

            unwrapped.push(unwrapped_val);
        }

        unwrapped
    }

    /// Reset stored phases to initial values.
    #[cfg(test)]
    pub fn reset(&mut self) {
        for i in 0..self.half_n {
            let omega = (2.0 * std::f64::consts::PI * self.hop as f64 * i as f64) / self.n as f64;
            self.prev_phase[i] = -omega;
            self.prev_unwrapped[i] = -omega;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_sine(freq: f64, sample_rate: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / sample_rate as f64).sin())
            .collect()
    }

    #[test]
    fn phase_vocoder_produces_magnitudes() {
        let sample_rate = 44100.0;
        let frame_size = 1024;
        let hop = 256;
        let mut pv = PhaseVocoder::new(frame_size, hop);

        let signal = synthetic_sine(440.0, sample_rate, frame_size);
        let (mag, _phase, _unwrapped) = pv.process_time_domain(&signal);

        assert_eq!(mag.len(), frame_size / 2 + 1);
        // Should have non-zero magnitudes
        let non_zero = mag.iter().filter(|m| **m > 0.01).count();
        assert!(non_zero > 0);
    }

    #[test]
    fn phase_vocoder_phases_in_range() {
        let sample_rate = 44100.0;
        let frame_size = 1024;
        let hop = 256;
        let mut pv = PhaseVocoder::new(frame_size, hop);

        let signal = synthetic_sine(440.0, sample_rate, frame_size);
        let (_mag, phase, _unwrapped) = pv.process_time_domain(&signal);

        for p in &phase {
            assert!(*p >= -std::f64::consts::PI - 0.01 && *p <= std::f64::consts::PI + 0.01);
        }
    }

    #[test]
    fn phase_vocoder_reset() {
        let mut pv = PhaseVocoder::new(1024, 256);
        let signal = vec![0.0; 1024];
        pv.process_time_domain(&signal);
        pv.reset();
        // Should not panic
    }
}
