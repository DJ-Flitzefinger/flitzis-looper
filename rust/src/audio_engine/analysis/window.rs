//! Window functions for frame conditioning.

/// Generate a Hann window of the given size.
///
/// The Hann window is periodic by design (equivalent to a symmetric window
/// of size N+1 with the final element missing).
pub fn hann_window(size: usize) -> Vec<f64> {
    if size <= 1 {
        return vec![1.0; size];
    }

    (0..size)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / size as f64).cos())
        .collect()
}

/// Apply a pre-computed Hann window to the source buffer in-place.
pub fn apply_window(src: &mut [f64], window: &[f64]) {
    let len = src.len().min(window.len());
    for (s, w) in src.iter_mut().zip(window.iter()).take(len) {
        *s *= w;
    }
    // Zero out any remaining samples if src is longer than window
    src[len..].fill(0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_window_correct_size() {
        let w = hann_window(1024);
        assert_eq!(w.len(), 1024);
    }

    #[test]
    fn hann_window_ends_near_zero() {
        let w = hann_window(1024);
        assert!(w[0].abs() < 1e-10);
        // Periodic Hann window: last sample is very close to zero but not exactly
        assert!(w[w.len() - 1].abs() < 1e-5);
    }

    #[test]
    fn hann_window_center_is_one() {
        let w = hann_window(1024);
        assert!((w[512] - 1.0).abs() < 1e-10);
    }

    #[test]
    fn apply_window_zeros_input() {
        let mut src = vec![1.0; 10];
        let window = vec![0.0; 10];
        apply_window(&mut src, &window);
        for s in &src {
            assert!((*s).abs() < 1e-10);
        }
    }
}
