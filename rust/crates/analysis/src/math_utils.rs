//! Mathematical utility functions for the qm-dsp pipeline.

/// Map the phase angle `ang` into the range [-π, π).
pub fn principal_arg(ang: f64) -> f64 {
    
    mod_f64(ang + std::f64::consts::PI, -2.0 * std::f64::consts::PI) + std::f64::consts::PI
}

/// Floating-point division modulus: return x % y.
fn mod_f64(x: f64, y: f64) -> f64 {
    let a = (x / y).floor();
    x - (y * a)
}

/// Return the mean of the given slice.
pub fn mean(src: &[f64]) -> f64 {
    if src.is_empty() {
        return 0.0;
    }
    src.iter().sum::<f64>() / src.len() as f64
}

/// Normalize a vector in-place by dividing through by its maximum absolute value.
pub fn normalize(data: &mut [f64]) {
    let max = data.iter().map(|v| v.abs()).fold(0.0_f64, f64::max);
    if max > 0.0 {
        for v in data.iter_mut() {
            *v /= max;
        }
    }
}

/// Adaptive threshold: subtract a moving-mean average filter from the data.
/// Values below the threshold are set to zero.
pub fn adaptive_threshold(data: &mut [f64]) {
    let sz = data.len();
    if sz == 0 {
        return;
    }

    let p_pre = 8;
    let p_post = 7;

    // Compute smoothed values (moving mean)
    let mut smoothed = vec![0.0; sz];
    #[allow(clippy::needless_range_loop)]
    for i in 0..sz {
        let first = i.saturating_sub(p_pre);
        let last = (i + p_post).min(sz - 1);
        let count = (last - first + 1) as f64;
        let sum: f64 = data[first..=last].iter().sum();
        smoothed[i] = sum / count;
    }

    // Subtract smoothed and clip to zero
    for i in 0..sz {
        data[i] -= smoothed[i];
        if data[i] < 0.0 {
            data[i] = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_arg_maps_to_range() {
        assert!((principal_arg(0.0) - 0.0).abs() < 1e-10);
        assert!(
            (principal_arg(std::f64::consts::PI) - std::f64::consts::PI).abs() < 1e-10
                || (principal_arg(std::f64::consts::PI) - (-std::f64::consts::PI)).abs() < 1e-10
        );
        assert!(principal_arg(3.0 * std::f64::consts::PI).abs() <= std::f64::consts::PI);
    }

    #[test]
    fn mean_returns_correct_average() {
        let data = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((mean(&data) - 3.0).abs() < 1e-10);
    }

    #[test]
    fn mean_empty_returns_zero() {
        let data: [f64; 0] = [];
        assert_eq!(mean(&data), 0.0);
    }

    #[test]
    fn normalize_unit_max() {
        let mut data = vec![1.0, 2.0, 3.0, -4.0, 2.0];
        normalize(&mut data);
        assert!((data[3].abs() - 1.0).abs() < 1e-10); // -4/4 = -1
        assert!((data[2] - 0.75).abs() < 1e-10); // 3/4 = 0.75
    }

    #[test]
    fn adaptive_threshold_removes_dc() {
        let mut data = vec![5.0; 20];
        data[10] = 10.0;
        adaptive_threshold(&mut data);
        // DC components should be zeroed
        assert!(data[0] < 0.1);
        // The spike should remain (partially)
        assert!(data[10] > 0.0);
    }
}
