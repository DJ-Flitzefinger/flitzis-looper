//! STFT implementation matching librosa (extracted from rosa).

use super::signal::Signal;
use realfft::RealFftPlanner;
use rustfft::num_complex::Complex;
use std::io;
use std::sync::Arc;

#[cfg(test)]
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

/// The same centered, windowed FFT as the dense STFT, retaining one frame.
/// Center padding is addressed virtually so it never copies the complete audio.
pub(crate) struct StftFrames<'a, 'b> {
    signal: &'a mut Signal<'b>,
    pad: usize,
    hop_length: usize,
    n_frames: usize,
    window: Vec<f64>,
    fft: Arc<dyn realfft::RealToComplex<f64>>,
    scratch: Vec<Complex<f64>>,
    input: Vec<f64>,
    output: Vec<Complex<f64>>,
}

impl<'a, 'b> StftFrames<'a, 'b> {
    pub(crate) fn new(signal: &'a mut Signal<'b>, params: &StftParams) -> Self {
        let win = match &params.window {
            Some(window) => window.clone(),
            None => super::windows::hann(params.win_length, true),
        };
        let window = super::dsp::pad_center(&win, params.n_fft);
        let pad = if params.center { params.n_fft / 2 } else { 0 };
        let signal_len = signal
            .len()
            .checked_add(pad.checked_mul(2).expect("STFT padding overflow"))
            .expect("STFT signal length overflow");
        let n_frames = if signal_len < params.n_fft {
            0
        } else {
            1 + (signal_len - params.n_fft) / params.hop_length
        };
        let mut planner = RealFftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(params.n_fft);
        let scratch = fft.make_scratch_vec();
        let input = fft.make_input_vec();
        let output = fft.make_output_vec();
        Self {
            signal,
            pad,
            hop_length: params.hop_length,
            n_frames,
            window,
            fft,
            scratch,
            input,
            output,
        }
    }

    pub(crate) fn frame_count(&self) -> usize {
        self.n_frames
    }

    pub(crate) fn process_frame(&mut self, index: usize) -> io::Result<&[Complex<f64>]> {
        assert!(index < self.n_frames, "STFT frame outside signal");
        let start = index * self.hop_length;
        self.input.fill(0.0);
        let destination_start = self.pad.saturating_sub(start).min(self.input.len());
        let source_start = start.saturating_sub(self.pad).min(self.signal.len());
        let count = (self.input.len() - destination_start).min(self.signal.len() - source_start);
        self.signal.read_range(
            source_start,
            &mut self.input[destination_start..destination_start + count],
        )?;
        for (sample, window) in self.input.iter_mut().zip(&self.window) {
            *sample *= window;
        }
        self.fft
            .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
            .expect("FFT failed");
        Ok(&self.output)
    }
}

/// Compute the complex STFT, returning real and imaginary parts as separate matrices.
///
/// Both matrices have shape `(1 + n_fft/2, n_frames)`.
#[cfg(test)]
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

#[cfg(test)]
mod streamed_tests {
    use super::*;

    #[test]
    fn streamed_frames_match_dense_fft_bits_at_centered_boundaries_and_tail() {
        for (n_fft, hop_length) in [(32768, 8820), (16384, 4410), (8192, 2205)] {
            for length in [1, hop_length - 1, hop_length, 2 * hop_length + 17] {
                let samples: Vec<f64> = (0..length)
                    .map(|index| {
                        (index as f64 * 0.173).sin() * 0.25
                            + if index % 37 == 0 { -0.75 } else { 0.0 }
                    })
                    .collect();
                let params = StftParams {
                    n_fft,
                    hop_length,
                    win_length: n_fft,
                    center: true,
                    window: None,
                };
                let (real, imaginary) = stft_as_real_imag(&samples, &params);
                let mut signal = Signal::Slice(&samples);
                let mut streamed = StftFrames::new(&mut signal, &params);
                assert_eq!(streamed.frame_count(), real.cols());
                let workspace = size_of::<f64>()
                    * (streamed.input.capacity() + streamed.window.capacity())
                    + size_of::<Complex<f64>>()
                        * (streamed.scratch.capacity() + streamed.output.capacity());
                assert!(workspace <= n_fft * 512);
                for frame in 0..streamed.frame_count() {
                    for (bin, spectrum) in streamed.process_frame(frame).unwrap().iter().enumerate()
                    {
                        assert_eq!(spectrum.re.to_bits(), real.get(bin, frame).to_bits());
                        assert_eq!(spectrum.im.to_bits(), imaginary.get(bin, frame).to_bits());
                    }
                }
            }
        }
    }

    #[test]
    fn streamed_frames_match_uncentered_and_custom_window_contracts() {
        let samples: Vec<f64> = (0..79).map(|index| (index as f64 * 0.39).cos()).collect();
        for center in [true, false] {
            let params = StftParams {
                n_fft: 32,
                hop_length: 7,
                win_length: 18,
                center,
                window: Some(super::super::windows::hann(18, true)),
            };
            let (real, imaginary) = stft_as_real_imag(&samples, &params);
            let mut signal = Signal::Slice(&samples);
            let mut streamed = StftFrames::new(&mut signal, &params);
            assert_eq!(streamed.frame_count(), real.cols());
            for frame in 0..streamed.frame_count() {
                for (bin, spectrum) in streamed.process_frame(frame).unwrap().iter().enumerate() {
                    assert_eq!(spectrum.re.to_bits(), real.get(bin, frame).to_bits());
                    assert_eq!(spectrum.im.to_bits(), imaginary.get(bin, frame).to_bits());
                }
            }
        }
    }
}
