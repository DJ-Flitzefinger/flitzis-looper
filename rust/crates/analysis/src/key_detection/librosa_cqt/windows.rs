//! Window functions matching scipy.signal.get_window (extracted from rosa).
//!
//! The `periodic` parameter corresponds to `fftbins=True` in scipy/librosa.

use std::f64::consts::PI;

/// Generalized cosine window.
///
/// `w[k] = sum(coeffs[i] * (-1)^i * cos(2*pi*i*k / N))`
///
/// When `periodic=true`, computes a symmetric window of size `n+1` and drops
/// the last sample — matching `scipy.signal.get_window(name, n, fftbins=True)`.
pub fn general_cosine(n: usize, coeffs: &[f64], periodic: bool) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![1.0];
    }

    let m = if periodic { n + 1 } else { n };

    let mut w: Vec<f64> = (0..m)
        .map(|k| {
            let frac = (k as f64) / ((m - 1) as f64);
            coeffs
                .iter()
                .enumerate()
                .map(|(i, &c)| {
                    let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
                    sign * c * (2.0 * PI * (i as f64) * frac).cos()
                })
                .sum()
        })
        .collect();

    w.truncate(n);
    w
}

/// Hann (raised cosine) window.
///
/// Coefficients: [0.5, 0.5]
pub fn hann(n: usize, periodic: bool) -> Vec<f64> {
    general_cosine(n, &[0.5, 0.5], periodic)
}
