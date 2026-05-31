//! Array utilities matching librosa.util (extracted from rosa).

/// Smallest positive normal f64 value.
pub fn tiny() -> f64 {
    f64::MIN_POSITIVE
}

/// Center-pad `data` with zeros to `size`.
///
/// Matches `librosa.util.pad_center`. Panics if `size < data.len()`.
pub fn pad_center(data: &[f64], size: usize) -> Vec<f64> {
    let n = data.len();
    assert!(
        size >= n,
        "pad_center: target size ({size}) must be >= data length ({n})"
    );
    if size == n {
        return data.to_vec();
    }
    let lpad = (size - n) / 2;
    let rpad = size - n - lpad;
    let mut result = vec![0.0; lpad];
    result.extend_from_slice(data);
    result.extend(std::iter::repeat_n(0.0, rpad));
    result
}
