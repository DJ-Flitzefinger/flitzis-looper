//! STFT implementation matching librosa (extracted from rosa).

use realfft::RealFftPlanner;

use super::matrix::Matrix;

/// STFT parameters with librosa-compatible defaults.
pub struct StftParams {
    /// Number of FFT points.
    pub n_fft: usize,
    /// Number of samples between successive frames.
    pub hop_length: usize,
    /// Length of the windowed signal before zero-padding.
    pub win_length: usize,
    /// If true, pad the signal on both sides so frames are centered.
    pub center: bool,
    /// Custom window. None = default periodic hann window.
    pub window: Option<Vec<f64>>,
}

/// Compute the complex STFT, returning real and imaginary parts as separate matrices.
///
/// Both matrices have shape `(1 + n_fft/2, n_frames)`.
pub fn stft_as_real_imag(y: &[f64], params: &StftParams) -> (Matrix, Matrix) {
    let n_fft = params.n_fft;
    let hop_length = params.hop_length;

    // Build window: use custom window if provided, otherwise periodic hann
    let win = match &params.window {
        Some(w) => w.clone(),
        None => super::windows::hann(params.win_length, true),
    };
    let win = super::dsp::pad_center(&win, n_fft);

    // Pad signal if center=true
    let padded: Vec<f64>;
    let signal = if params.center {
        let pad = n_fft / 2;
        padded = std::iter::repeat_n(0.0, pad)
            .chain(y.iter().copied())
            .chain(std::iter::repeat_n(0.0, pad))
            .collect();
        &padded
    } else {
        y
    };

    // Compute frame count
    if signal.len() < n_fft {
        return (
            Matrix::zeros(1 + n_fft / 2, 0),
            Matrix::zeros(1 + n_fft / 2, 0),
        );
    }
    let n_frames = 1 + (signal.len() - n_fft) / hop_length;
    let n_bins = 1 + n_fft / 2;

    // Setup FFT
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n_fft);
    let mut scratch = fft.make_scratch_vec();
    let mut input_buf = fft.make_input_vec();
    let mut output_buf = fft.make_output_vec();

    let mut real_data = vec![0.0; n_bins * n_frames];
    let mut imag_data = vec![0.0; n_bins * n_frames];

    for i in 0..n_frames {
        let start = i * hop_length;
        let frame = &signal[start..start + n_fft];

        // Apply window
        for (j, sample) in frame.iter().enumerate() {
            input_buf[j] = sample * win[j];
        }

        // FFT
        fft.process_with_scratch(&mut input_buf, &mut output_buf, &mut scratch)
            .expect("FFT failed");

        // Copy output
        for (bin_idx, &c) in output_buf[..n_bins].iter().enumerate() {
            let idx = bin_idx * n_frames + i;
            real_data[idx] = c.re;
            imag_data[idx] = c.im;
        }
    }

    (
        Matrix::from_vec(real_data, n_bins, n_frames),
        Matrix::from_vec(imag_data, n_bins, n_frames),
    )
}
