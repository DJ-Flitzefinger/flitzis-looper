//! Downbeat detection: identify bar-start beats from beat positions.
//!
//! Implements the qm-dsp DownBeat spectral difference method.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use std::sync::Arc;

use crate::math_utils::adaptive_threshold;

/// Downbeat detector using spectral difference between beat segments.
pub struct DownBeat {
    /// Decimation factor.
    factor: usize,
    /// Detection function increment.
    increment: usize,
    /// Beat frame size (power of 2).
    beat_frame_size: usize,
    /// Complex buffer for FFT.
    fft_buf: Vec<Complex<f64>>,
    /// FFT plan.
    fft_plan: Arc<dyn rustfft::Fft<f64>>,
    /// Beats per bar (0 = auto-detect, default 4).
    beats_per_bar: usize,
}

impl DownBeat {
    /// Create a new DownBeat detector.
    pub fn new(sample_rate: f64, factor: usize, increment: usize) -> Self {
        let decimated_rate = sample_rate / factor as f64;
        let beat_frame_size = next_power_of_two((decimated_rate * 1.3) as usize).max(2);

        let mut planner = FftPlanner::<f64>::new();
        let fft_plan = planner.plan_fft_forward(beat_frame_size);

        Self {
            factor,
            increment,
            beat_frame_size,
            fft_buf: vec![Complex::new(0.0, 0.0); beat_frame_size],
            fft_plan,
            beats_per_bar: 0, // auto-detect (defaults to 4)
        }
    }

    /// Set the number of beats per bar (e.g., 4 for 4/4 time).
    pub fn set_beats_per_bar(&mut self, bpb: usize) {
        self.beats_per_bar = bpb;
    }

    /// Estimate which beats are downbeats.
    ///
    /// `audio` is the full audio buffer at the original sample rate.
    /// `audio_length` is the number of samples in the audio buffer.
    /// `beats` contains beat positions in frame indices (DF increment units).
    /// `downbeats` is filled with indices into the `beats` array.
    pub fn find_downbeats(
        &mut self,
        audio: &[f64],
        audio_length: usize,
        beats: &[f64],
        downbeats: &mut Vec<usize>,
    ) {
        if audio_length == 0 || beats.len() < 2 {
            return;
        }

        let newspec_size = self.beat_frame_size / 2;
        let mut newspec = vec![0.0; newspec_size];
        let mut oldspec = vec![0.0; newspec_size];
        let mut beatsd = Vec::new();

        // Downsample audio
        let decimated = self.downsample(audio);
        let dec_len = decimated.len();

        for i in 0..(beats.len() - 1) {
            // Calculate beat segment boundaries in downsampled audio
            let beat_start = ((beats[i] * self.increment as f64) / self.factor as f64) as usize;
            let mut beat_end =
                ((beats[i + 1] * self.increment as f64) / self.factor as f64) as usize;

            if beat_end >= dec_len {
                beat_end = dec_len.saturating_sub(1);
            }
            if beat_end < beat_start {
                beat_end = beat_start;
            }
            let beat_len = beat_end - beat_start;

            // Apply Hann window to beat frame and load into FFT buffer
            for j in 0..beat_len.min(self.beat_frame_size) {
                let mul =
                    0.5 * (1.0 - (2.0 * std::f64::consts::PI * j as f64 / beat_len as f64).cos());
                self.fft_buf[j] = Complex::new(decimated[beat_start + j] * mul, 0.0);
            }
            for j in beat_len..self.beat_frame_size {
                self.fft_buf[j] = Complex::new(0.0, 0.0);
            }

            // FFT
            self.fft_plan.process(&mut self.fft_buf);

            // Calculate magnitudes
            for (j, val) in newspec.iter_mut().enumerate().take(newspec_size) {
                *val = self.fft_buf[j].norm_sqr().sqrt();
            }

            // Preserve peaks by applying adaptive threshold
            adaptive_threshold(&mut newspec);

            // Calculate Jensen-Shannon divergence between new and old spectral frames
            if i > 0 {
                beatsd.push(self.measure_spec_diff(&mut oldspec, &newspec));
            }

            // Copy newspec to oldspec
            oldspec.copy_from_slice(&newspec);
        }

        // Find downbeat candidates
        let timesig = if self.beats_per_bar > 0 {
            self.beats_per_bar
        } else {
            4
        };

        let mut dbcand = vec![0.0; timesig];

        // Look for beat transition which leads to greatest spectral change
        #[allow(clippy::needless_range_loop)]
        for beat in 0..timesig {
            let mut count = 0;
            for example in (beat as isize - 1)..beatsd.len() as isize {
                if example < 0 {
                    continue;
                }
                if (example - (beat as isize - 1)) % timesig as isize == 0 {
                    dbcand[beat] += beatsd[example as usize] / timesig as f64;
                    count += 1;
                }
            }
            if count > 0 {
                dbcand[beat] /= count as f64;
            }
        }

        // First downbeat is at the index of maximum value of dbcand
        let dbind = dbcand
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        // Remaining downbeats are at timesig intervals from the first
        downbeats.clear();
        for i in (dbind..beats.len()).step_by(timesig) {
            downbeats.push(i);
        }
    }

    /// Measure Jensen-Shannon divergence between two spectral frames.
    fn measure_spec_diff(&self, oldspec: &mut [f64], newspec: &[f64]) -> f64 {
        const EPS: f64 = 2.2204e-016;

        let spec_size = 512.min(oldspec.len() / 4);
        if spec_size == 0 {
            return 0.0;
        }

        // Work on copies to avoid mutating oldspec permanently
        let mut local_old = oldspec[..spec_size].to_vec();
        let mut local_new = newspec[..spec_size].to_vec();

        let mut sum_new: f64 = 0.0;
        let mut sum_old: f64 = 0.0;

        // Add epsilon and compute sums
        for i in 0..spec_size {
            local_new[i] += EPS;
            local_old[i] += EPS;
            sum_new += local_new[i];
            sum_old += local_old[i];
        }

        // Normalize
        for i in 0..spec_size {
            local_new[i] /= sum_new;
            local_old[i] /= sum_old;

            if local_new[i] == 0.0 {
                local_new[i] = 1.0;
            }
            if local_old[i] == 0.0 {
                local_old[i] = 1.0;
            }
        }

        // Jensen-Shannon calculation
        let mut sd = 0.0;
        for i in 0..spec_size {
            let sd1 = 0.5 * local_old[i] + 0.5 * local_new[i];
            if sd1 > 0.0 {
                sd += -sd1 * sd1.ln()
                    + 0.5 * (local_old[i] * local_old[i].ln())
                    + 0.5 * (local_new[i] * local_new[i].ln());
            }
        }

        sd
    }

    /// Simple decimation by averaging blocks.
    fn downsample(&self, audio: &[f64]) -> Vec<f64> {
        if self.factor <= 1 {
            return audio.to_vec();
        }

        let out_len = audio.len() / self.factor;
        let mut output = vec![0.0; out_len];

        #[allow(clippy::needless_range_loop)]
        for i in 0..out_len {
            let start = i * self.factor;
            let end = (start + self.factor).min(audio.len());
            let sum: f64 = audio[start..end].iter().sum();
            output[i] = sum / (end - start) as f64;
        }

        output
    }
}

/// Return the next higher integer power of two from x.
fn next_power_of_two(x: usize) -> usize {
    if x <= 1 {
        return 1;
    }
    let mut n = 1;
    let mut v = x;
    while v > 1 {
        v >>= 1;
        n <<= 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn fake_beat_positions(count: usize, period_frames: f64) -> Vec<f64> {
        (0..count).map(|i| i as f64 * period_frames).collect()
    }

    #[test]
    fn downbeat_with_regular_beats() {
        let audio = click_track(44100, 120.0, 20);
        let beats = fake_beat_positions(20, 4.3); // ~120 BPM at 86 Hz frame rate

        let mut downbeat = DownBeat::new(44100.0, 16, 512);
        downbeat.set_beats_per_bar(4);

        let mut downbeats = Vec::new();
        downbeat.find_downbeats(&audio, audio.len(), &beats, &mut downbeats);

        // Should find at least one downbeat
        assert!(!downbeats.is_empty());
        // Downbeat indices should be valid
        for idx in &downbeats {
            assert!(*idx < beats.len());
        }
    }

    #[test]
    fn downbeat_empty_input() {
        let audio: Vec<f64> = vec![];
        let beats: Vec<f64> = vec![];

        let mut downbeat = DownBeat::new(44100.0, 16, 512);
        let mut downbeats = Vec::new();
        downbeat.find_downbeats(&audio, audio.len(), &beats, &mut downbeats);
        assert!(downbeats.is_empty());
    }

    #[test]
    fn downbeat_too_few_beats() {
        let audio = click_track(44100, 120.0, 20);
        let beats = vec![4.3]; // Only one beat

        let mut downbeat = DownBeat::new(44100.0, 16, 512);
        let mut downbeats = Vec::new();
        downbeat.find_downbeats(&audio, audio.len(), &beats, &mut downbeats);
        assert!(downbeats.is_empty());
    }
}
