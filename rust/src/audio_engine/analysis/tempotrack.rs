//! Tempo tracking: Viterbi HMM beat period estimation + DP beat tracking.
//!
//! Implements the qm-dsp TempoTrackV2 algorithm with configurable parameters.

use super::math_utils::adaptive_threshold;

/// Tempo tracker using Viterbi HMM for beat period estimation and
/// dynamic programming for beat position tracking.
pub struct TempoTrackV2 {
    /// Sample rate.
    rate: f64,
    /// Detection function step in seconds.
    step_secs: f64,
}

impl TempoTrackV2 {
    /// Create a new TempoTrackV2.
    pub fn new(rate: f64, step_secs: f64) -> Self {
        Self { rate, step_secs }
    }

    /// Estimate beat period sequence using Viterbi HMM over RCF bank.
    ///
    /// `beat_period` is filled with the estimated beat period (in frames) for each window.
    pub fn calculate_beat_period(
        &self,
        df: &[f64],
        beat_period: &mut Vec<usize>,
        input_tempo: f64,
        _constrain_tempo: bool,
    ) {
        const WV_LEN: usize = 128;
        const WINLEN: usize = 512;
        const HOPSIZE: usize = 128;

        // Rayleigh parameter
        // Magic number from qm-dsp: (60 * 44100 / 512.0) / inputtempo
        // This converts BPM to "frames at 86.1 Hz"
        let _step_secs = self.step_secs;
        let rayparam = (60.0 * 44100.0 / 512.0) / input_tempo;

        // Generate Rayleigh weighting curve
        let mut wv = vec![0.0; WV_LEN];
        #[allow(clippy::needless_range_loop)]
        for i in 0..WV_LEN {
            let x = i as f64;
            let rp2 = rayparam * rayparam;
            wv[i] = (x / rp2) * ((-(x * x)) / (2.0 * rp2)).exp();
        }

        let df_len = df.len();
        let mut rcfmat = Vec::new();
        let mut dfframe = vec![0.0; WINLEN];
        let mut rcf = vec![0.0; WV_LEN];

        // Loop over the ODF with half-window padding
        let start_i = -(WINLEN as isize / 2);
        let end_i = df_len as isize - WINLEN as isize / 2;
        for i in (start_i..end_i).step_by(HOPSIZE) {
            let mut k = 0;
            let mut l = WINLEN;

            if i < 0 {
                k = (-i) as usize;
                dfframe[..k].fill(0.0);
            }

            // i + k is always >= 0, so safe to cast
            let i_usize = (i + k as isize) as usize;
            if i_usize + l > df_len {
                l = df_len.saturating_sub(i_usize);
                dfframe[l..WINLEN].fill(0.0);
            }

            let copy_len = l.saturating_sub(k);
            let src_start = i_usize;
            dfframe[k..k + copy_len].copy_from_slice(&df[src_start..src_start + copy_len]);

            // Apply RCF bank
            self.get_rcf(&dfframe, &wv, &mut rcf);

            // Append to matrix
            rcfmat.push(rcf.clone());
        }

        // Viterbi decode
        self.viterbi_decode(&rcfmat, &wv, beat_period);
    }

    /// Compute the Resonator Comb Filter (RCF) bank for a single frame.
    fn get_rcf(&self, dfframe_in: &[f64], wv: &[f64], rcf: &mut [f64]) {
        const EPS: f64 = 0.0000008;
        const NUM_ELEM: usize = 4;

        let mut dfframe = dfframe_in.to_vec();
        adaptive_threshold(&mut dfframe);

        let dfframe_len = dfframe.len();
        let rcf_len = rcf.len();

        // Compute autocorrelation function
        let mut acf = vec![0.0; dfframe_len];
        for lag in 0..dfframe_len {
            let mut sum = 0.0;
            for n in 0..(dfframe_len - lag) {
                sum += dfframe[n] * dfframe[n + lag];
            }
            acf[lag] = sum / (dfframe_len - lag) as f64;
        }

        // Apply comb filtering
        for i in 2..rcf_len {
            for a in 1..=NUM_ELEM {
                let a_isize = a as isize;
                for b in (1_isize - a_isize)..a_isize {
                    let idx = a_isize * i as isize + b - 1;
                    if idx >= 0 && (idx as usize) < acf.len() {
                        rcf[i - 1] += (acf[idx as usize] * wv[i - 1]) / (2.0 * a as f64 - 1.0);
                    }
                }
            }
        }

        // Apply adaptive threshold to RCF
        adaptive_threshold(rcf);

        // Normalize RCF to sum to unity
        let mut rcfsum = 0.0;
        for val in rcf.iter_mut().take(rcf_len) {
            *val += EPS;
            rcfsum += *val;
        }

        for val in rcf.iter_mut().take(rcf_len) {
            *val /= rcfsum + EPS;
        }
    }

    /// Viterbi decoding over the RCF probability matrix.
    fn viterbi_decode(&self, rcfmat: &[Vec<f64>], wv: &[f64], beat_period: &mut Vec<usize>) {
        const EPS: f64 = 0.0000008;
        const SIGMA: f64 = 8.0;

        if rcfmat.len() < 2 {
            return;
        }

        let t = rcfmat.len(); // time steps
        let q = rcfmat[0].len(); // states (beat periods)

        // Build transition matrix (diagonal Gaussian)
        let mut tmat = vec![vec![0.0; q]; q];
        #[allow(clippy::needless_range_loop)]
        for i in 20..q.saturating_sub(20) {
            #[allow(clippy::needless_range_loop)]
            for j in 20..q.saturating_sub(20) {
                let mu = i as f64;
                tmat[i][j] = ((-(j as f64 - mu).powi(2)) / (2.0 * SIGMA * SIGMA)).exp();
            }
        }

        // Delta and psi matrices
        let mut delta = vec![vec![0.0; q]; t];
        let mut psi = vec![vec![0usize; q]; t];

        // Initialize first column
        for (j, val) in delta[0].iter_mut().enumerate().take(q) {
            *val = wv[j] * rcfmat[0][j];
        }

        // Normalize first column
        let mut deltasum: f64 = delta[0].iter().sum();
        for val in delta[0].iter_mut().take(q) {
            *val /= deltasum + EPS;
        }

        // Forward pass
        let mut tmp_vec = vec![0.0; q];
        for time_step in 1..t {
            #[allow(clippy::needless_range_loop)]
            for j in 0..q {
                #[allow(clippy::needless_range_loop)]
                for i in 0..q {
                    tmp_vec[i] = delta[time_step - 1][i] * tmat[j][i];
                }

                // Find max
                let (max_val, max_idx) = tmp_vec
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.total_cmp(b))
                    .map(|(idx, val)| (*val, idx))
                    .unwrap_or((0.0, 0));

                delta[time_step][j] = max_val * rcfmat[time_step][j];
                psi[time_step][j] = max_idx;
            }

            // Normalize
            deltasum = delta[time_step].iter().sum();
            for val in delta[time_step].iter_mut().take(q) {
                *val /= deltasum + EPS;
            }
        }

        // Backtrace
        let max_idx = delta[t - 1]
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        beat_period.clear();
        beat_period.resize(t, 0);
        beat_period[t - 1] = max_idx;

        for time_step in (1..t).rev() {
            let next_idx = beat_period[time_step].min(q - 1);
            beat_period[time_step - 1] = psi[time_step][next_idx];
        }
    }

    /// Calculate beat positions using dynamic programming.
    ///
    /// `beats` is filled with beat positions in frame indices.
    pub fn calculate_beats(
        &self,
        df: &[f64],
        beat_period: &[usize],
        beats: &mut Vec<f64>,
        alpha: f64,
        tightness: f64,
    ) {
        if df.is_empty() || beat_period.is_empty() {
            return;
        }

        let df_len = df.len();
        let mut cumscore = vec![0.0; df_len];
        let mut backlink = vec![0usize; df_len];
        let localscore = df; // detection function values

        let mut old_period = 0;
        let mut txwt_len = 0;
        let mut txwt = Vec::new();

        for i in 0..df_len {
            // Get beat period for current window (beat_period has one entry per window)
            let window_idx = (i / 128).min(beat_period.len() - 1);
            let period = beat_period[window_idx].max(1);

            let prange_min = -(period as isize * 2);
            if period != old_period {
                old_period = period;
                let prange_max = -(period as isize / 2);

                txwt_len = (prange_max as isize - prange_min) as usize + 1;
                txwt.clear();
                txwt.reserve(txwt_len);

                #[allow(clippy::needless_range_loop)]
                for j in 0..txwt_len {
                    let mu = period as f64;
                    let round_2mu = (2.0 * mu).round();
                    let log_arg = ((round_2mu - j as f64) / mu).ln();
                    txwt.push((-0.5 * (tightness * log_arg).powi(2)).exp());
                }
            }

            // Find best previous beat within range
            let mut vv = 0.0;
            let mut xx = 0usize;

            #[allow(clippy::needless_range_loop)]
            for j in 0..txwt_len {
                let cscore_ind = (i as isize + prange_min + j as isize) as usize;
                if cscore_ind < df_len {
                    let scorecands = txwt[j] * cumscore[cscore_ind];
                    if scorecands > vv {
                        vv = scorecands;
                        xx = cscore_ind;
                    }
                }
            }

            cumscore[i] = alpha * vv + (1.0 - alpha) * localscore[i];
            backlink[i] = xx;
        }

        // Find starting point (last beat) - pick strongest point in last beat period
        let last_period = beat_period.last().copied().unwrap_or(1).max(1);
        let start_search = if df_len.saturating_sub(last_period) > 0 {
            df_len - last_period
        } else {
            0
        };

        let tmp_vec = cumscore[start_search..df_len].to_vec();

        let max_idx = tmp_vec
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        let startpoint = (start_search + max_idx).min(backlink.len() - 1);

        // Backtrack
        let mut ibeats = Vec::new();
        ibeats.push(startpoint);

        loop {
            let last = *ibeats.last().unwrap();
            let bl = backlink[last];
            if bl == 0 || bl == last {
                break;
            }
            ibeats.push(bl);
        }

        // Reverse and store as beats
        beats.clear();
        for i in 0..ibeats.len() {
            beats.push(ibeats[ibeats.len() - i - 1] as f64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_rhythmic_odf(bpm: f64, frames: usize) -> Vec<f64> {
        let frames_per_beat = 60.0 / bpm * 86.1; // ~86 Hz frame rate
        let mut odf = vec![0.0; frames];
        let mut pos = 0.0;
        while pos < frames as f64 {
            let idx = pos.round() as usize;
            if idx < frames {
                odf[idx] = 1.0;
            }
            pos += frames_per_beat;
        }
        odf
    }

    #[test]
    fn beat_period_estimation_120_bpm() {
        let tracker = TempoTrackV2::new(44100.0, 0.01161);
        // Use enough frames to produce multiple windows (512 frame windows, 128 hop)
        let odf = synthetic_rhythmic_odf(120.0, 4096);

        let mut beat_period = Vec::new();
        tracker.calculate_beat_period(&odf, &mut beat_period, 120.0, false);

        assert!(!beat_period.is_empty());
        // At 120 BPM with ~86 Hz frame rate, beat period ≈ 4.3 frames
        // The Viterbi should find periods near this value
        let avg_period = beat_period.iter().sum::<usize>() as f64 / beat_period.len() as f64;
        assert!(
            avg_period > 1.0 && avg_period < 100.0,
            "avg period: {}",
            avg_period
        );
    }

    #[test]
    fn beat_tracking_produces_beats() {
        let tracker = TempoTrackV2::new(44100.0, 0.01161);
        let odf = synthetic_rhythmic_odf(120.0, 4096);

        let mut beat_period = Vec::new();
        tracker.calculate_beat_period(&odf, &mut beat_period, 120.0, false);

        let mut beats = Vec::new();
        tracker.calculate_beats(&odf, &beat_period, &mut beats, 0.9, 4.0);

        assert!(!beats.is_empty());
        // Beats should be in increasing order
        for i in 1..beats.len() {
            assert!(beats[i] >= beats[i - 1]);
        }
    }

    #[test]
    fn empty_input_returns_empty() {
        let tracker = TempoTrackV2::new(44100.0, 0.01161);
        let odf: Vec<f64> = vec![];

        let mut beat_period = Vec::new();
        tracker.calculate_beat_period(&odf, &mut beat_period, 120.0, false);
        assert!(beat_period.is_empty());
    }
}
