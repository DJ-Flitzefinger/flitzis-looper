//! Scaling is per retained bin/frame, after octave responses have been trimmed.
//!
//! Register as a test-only child of `librosa_cqt::cqt`. This uses the independent
//! scale=false output and the analytic constant-Q filter length relation; it
//! does not duplicate octave traversal or the production scaling loop.

use super::{CqtConfig, cqt};

fn pinned_config(scale: bool) -> CqtConfig {
    CqtConfig {
        sr: 44_100.0,
        hop_length: 8820,
        fmin: 65.0,
        n_bins: 105,
        bins_per_octave: 24,
        tuning: 0.0,
        filter_scale: 1.0,
        norm: Some(1.0),
        scale,
    }
}

fn assert_complete_matrix_scaling(input_frames: usize) {
    let samples: Vec<f64> = (0..input_frames)
        .map(|frame| {
            let time = frame as f64 / 44_100.0;
            0.37 * (std::f64::consts::TAU * 196.0 * time).sin()
                + 0.23 * (std::f64::consts::TAU * 523.25 * time).cos()
                + if frame % 113 == 0 { 0.11 } else { 0.0 }
        })
        .collect();
    let unscaled = cqt(&samples, &pinned_config(false));
    let scaled = cqt(&samples, &pinned_config(true));
    let columns = 1 + input_frames / 8820;
    assert_eq!(unscaled.len(), 105 * columns, "unscaled geometry");
    assert_eq!(scaled.len(), 105 * columns, "scaled geometry");
    assert!(
        unscaled
            .iter()
            .chain(&scaled)
            .all(|value| value.is_finite())
    );

    // For logarithmically spaced frequencies, Q=1/alpha and L=Q*sr/f.
    // scale=false multiplies raw response by sqrt(L/n_fft), whereas
    // scale=true divides by sqrt(n_fft). Their ratio is therefore 1/sqrt(L),
    // independently of octave FFT sizes, source padding and matrix traversal.
    let frequency_ratio_squared = 2.0_f64.powf(2.0 / 24.0);
    let alpha = (frequency_ratio_squared - 1.0) / (frequency_ratio_squared + 1.0);
    let mut maximum_error = 0.0_f64;
    let mut maximum_first_column_error = 0.0_f64;
    for (index, (&raw, &actual)) in unscaled.iter().zip(&scaled).enumerate() {
        let row = index / columns;
        let column = index % columns;
        let frequency = 65.0 * 2.0_f64.powf(row as f64 / 24.0);
        let length = 44_100.0 / (alpha * frequency);
        let expected = raw / length.sqrt();
        let error = (actual - expected).abs();
        maximum_error = maximum_error.max(error);
        if column == 0 {
            maximum_first_column_error = maximum_first_column_error.max(error);
        }
    }
    assert!(
        maximum_first_column_error < 1e-10,
        "N={input_frames}: first-column scaling error {maximum_first_column_error}"
    );
    assert!(
        maximum_error < 1e-10,
        "N={input_frames}: complete-matrix scaling error {maximum_error}"
    );
}

#[test]
fn pinned_cqt_scales_each_retained_cell_once_when_octaves_have_extra_frames() {
    // The original-rate octave has k columns. Ceil-half/quarter source lengths
    // have k+1 columns, so trim_stack discards their final column.
    for input_frames in [17_639, 26_459] {
        assert_complete_matrix_scaling(input_frames);
    }
}

#[test]
fn pinned_cqt_scaling_preserves_normal_equal_octave_frame_counts() {
    for input_frames in [17_640, 26_460] {
        assert_complete_matrix_scaling(input_frames);
    }
}
