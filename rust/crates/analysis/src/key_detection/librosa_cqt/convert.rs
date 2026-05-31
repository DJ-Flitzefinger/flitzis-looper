//! Frequency conversion utilities matching librosa.core.convert (extracted from rosa).

/// Compute center frequencies for CQT bins.
///
/// `f_k = fmin * 2^(tuning / bpo) * 2^(k / bpo)` for k in 0..n_bins.
/// Matches `librosa.cqt_frequencies(n_bins, fmin=fmin, bins_per_octave=bpo, tuning=tuning)`.
pub fn cqt_frequencies(n_bins: usize, fmin: f64, bins_per_octave: usize, tuning: f64) -> Vec<f64> {
    let correction = 2.0_f64.powf(tuning / bins_per_octave as f64);
    (0..n_bins)
        .map(|k| correction * fmin * 2.0_f64.powf(k as f64 / bins_per_octave as f64))
        .collect()
}
