//! CQT/VQT implementation matching librosa (extracted from rosa).
//!
//! Combines wavelet construction and VQT/CQT transform into a single module.

use realfft::RealFftPlanner;
use std::borrow::Cow;

use super::convert::cqt_frequencies;
use super::dsp::tiny;
use super::matrix::Matrix;
use super::stft::{StftFrames, StftParams};
use super::windows::hann;

/// Checked allocation reservation for the pinned 44100-Hz KeyNet frontend.
/// This is working storage in addition to the caller's shared complete f64 mono;
/// the separately cached ONNX model and its inference arena are not PCM/CQT.
///
/// The default 105 bins / 24 bins per octave / 65-Hz fmin / 8820 hop has no
/// early downsampling. Its first octave borrows mono; two subsequent even hops
/// retain at most ceil(N/2) + ceil(N/4) f64 samples during replacement (6N
/// bytes, with rounding covered below). The later 2205 hop is odd. Each Sinc
/// result has exactly the requested capacity, including zero-filled tail.
///
/// 52 bytes per bin/frame cover all retained octave re/im responses (16),
/// stacked re/im (16), magnitude (8), and f32 frontend output (4), with another
/// eight bytes for conservative overlap. Ceil(N/8820)+1 also covers the two
/// rounded half lengths. No FFT-bin-by-time matrix is retained.
///
/// The maximum default FFT is 32768. The fixed reservation is derived from
/// three simultaneous-storage envelopes: 1024 bytes/FFT point for the original
/// and scaled 24-row bases, magnitude basis, padded wavelet filters and their
/// construction vectors; 512 bytes/point for the FFT plans, scratch, window,
/// real input and complex output; 512 bytes/point for the pinned 256x256 Sinc
/// tables, construction scratch, input/result chunks, vector headers and ceil
/// rounding. These sum to 64 MiB and dominate the smaller later octave plans.
pub fn default_cqt_working_bytes(input_frames: usize) -> Option<usize> {
    let time_frames = input_frames.div_ceil(8820).checked_add(1)?;
    let resampling = input_frames.checked_mul(6)?;
    let responses = time_frames.checked_mul(105)?.checked_mul(52)?;
    let fixed_workspace = 32768_usize.checked_mul(1024 + 512 + 512)?;
    resampling
        .checked_add(responses)?
        .checked_add(fixed_workspace)
}

// ============================================================================
// Wavelet construction
// ============================================================================

/// Equivalent noise bandwidth (ENBW) of a window function.
pub fn window_bandwidth(n: usize) -> f64 {
    let w = hann(n, true);
    let sum_sq: f64 = w.iter().map(|x| x * x).sum();
    let sum_w: f64 = w.iter().sum();
    n as f64 * sum_sq / (sum_w * sum_w)
}

/// Compute relative bandwidth alpha from frequency array.
///
/// Matches `librosa.filters._relative_bandwidth(freqs=freqs)`.
pub fn relative_bandwidth(freqs: &[f64]) -> Vec<f64> {
    let n = freqs.len();
    assert!(n >= 2, "relative_bandwidth requires at least 2 frequencies");

    let logf: Vec<f64> = freqs.iter().map(|f| f.log2()).collect();
    let mut bpo = vec![0.0; n];

    bpo[0] = 1.0 / (logf[1] - logf[0]);
    bpo[n - 1] = 1.0 / (logf[n - 1] - logf[n - 2]);

    for i in 1..n - 1 {
        bpo[i] = 2.0 / (logf[i + 1] - logf[i - 1]);
    }

    bpo.iter()
        .map(|&b| {
            let r2 = 2.0_f64.powf(2.0 / b);
            (r2 - 1.0) / (r2 + 1.0)
        })
        .collect()
}

/// Compute filter lengths for CQT/VQT wavelet basis.
///
/// Returns (lengths, cutoff_frequency).
pub fn wavelet_lengths(freqs: &[f64], sr: f64, filter_scale: f64, gamma: f64) -> (Vec<f64>, f64) {
    let alpha = relative_bandwidth(freqs);
    let n = freqs.len();

    // Q factor per bin
    let q: Vec<f64> = alpha.iter().map(|&a| filter_scale / a).collect();

    // Filter lengths: lengths[k] = Q * sr / (freqs[k] + gamma / alpha[k])
    let lengths: Vec<f64> = (0..n)
        .map(|i| q[i] * sr / (freqs[i] + gamma / alpha[i]))
        .collect();

    // Cutoff frequency
    let win_bw = window_bandwidth(1000);
    let f_cutoff = (0..n)
        .map(|i| freqs[i] * (1.0 + 0.5 * win_bw / q[i]) + 0.5 * gamma)
        .fold(0.0_f64, f64::max);

    (lengths, f_cutoff)
}

/// Build complex wavelet filters for CQT/VQT.
///
/// Returns (filters_re, filters_im, lengths).
pub(crate) fn build_wavelet_filters(
    freqs: &[f64],
    sr: f64,
    filter_scale: f64,
    norm: Option<f64>,
    gamma: f64,
) -> (Matrix, Matrix, Vec<f64>) {
    let (lengths, _f_cutoff) = wavelet_lengths(freqs, sr, filter_scale, gamma);
    let n_freqs = freqs.len();

    let mut filters_re: Vec<Vec<f64>> = Vec::with_capacity(n_freqs);
    let mut filters_im: Vec<Vec<f64>> = Vec::with_capacity(n_freqs);

    for (k, &freq) in freqs.iter().enumerate() {
        let ilen = lengths[k];
        // Match librosa: arange(-ilen//2, ilen//2)
        let half = (ilen / 2.0).floor() as i64;
        let n_k = (half + half) as usize; // 2 * floor(ilen/2)
        let start = -half;

        // Generate time axis and complex exponential
        let mut re = Vec::with_capacity(n_k);
        let mut im = Vec::with_capacity(n_k);
        for j in 0..n_k {
            let t = (start + j as i64) as f64;
            let phase = 2.0 * std::f64::consts::PI * freq * t / sr;
            re.push(phase.cos());
            im.push(phase.sin());
        }

        // Apply Hann window
        let win = hann(n_k, true);
        for j in 0..n_k {
            re[j] *= win[j];
            im[j] *= win[j];
        }

        // Normalize complex signal (L1 norm by default)
        if let Some(p) = norm {
            let norm_val = if p == 1.0 {
                // L1: sum of |z|
                re.iter()
                    .zip(im.iter())
                    .map(|(&r, &i)| (r * r + i * i).sqrt())
                    .sum::<f64>()
            } else if p == 2.0 {
                // L2: sqrt(sum(|z|^2))
                re.iter()
                    .zip(im.iter())
                    .map(|(&r, &i)| r * r + i * i)
                    .sum::<f64>()
                    .sqrt()
            } else if p.is_infinite() {
                // Linf: max(|z|)
                re.iter()
                    .zip(im.iter())
                    .map(|(&r, &i)| (r * r + i * i).sqrt())
                    .fold(0.0_f64, f64::max)
            } else {
                // Lp norm
                re.iter()
                    .zip(im.iter())
                    .map(|(&r, &i)| (r * r + i * i).sqrt().powf(p))
                    .sum::<f64>()
                    .powf(1.0 / p)
            };

            if norm_val > tiny() {
                let inv = 1.0 / norm_val;
                for j in 0..n_k {
                    re[j] *= inv;
                    im[j] *= inv;
                }
            }
        }

        filters_re.push(re);
        filters_im.push(im);
    }

    // Determine max_len (next power of 2)
    let raw_max = filters_re.iter().map(|f| f.len()).max().unwrap_or(0);
    let max_len = raw_max.next_power_of_two();

    // Center-pad all filters to max_len
    let mut re_data = vec![0.0; n_freqs * max_len];
    let mut im_data = vec![0.0; n_freqs * max_len];

    for k in 0..n_freqs {
        let flen = filters_re[k].len();
        let lpad = (max_len - flen) / 2;
        for j in 0..flen {
            re_data[k * max_len + lpad + j] = filters_re[k][j];
            im_data[k * max_len + lpad + j] = filters_im[k][j];
        }
    }

    (
        Matrix::from_vec(re_data, n_freqs, max_len),
        Matrix::from_vec(im_data, n_freqs, max_len),
        lengths,
    )
}

// ============================================================================
// CQT/VQT Transform
// ============================================================================

/// Complex matrix multiply: (A_re + j*A_im) @ (B_re + j*B_im)
/// Returns (C_re, C_im).
/// Build FFT-domain filter basis for one octave.
/// Returns (basis_re, basis_im, n_fft, lengths).
pub(crate) fn vqt_filter_fft(
    sr: f64,
    freqs: &[f64],
    filter_scale: f64,
    norm: Option<f64>,
    hop_length: usize,
    gamma: f64,
) -> (Matrix, Matrix, usize, Vec<f64>) {
    // Build time-domain wavelet filters
    let (filters_re, filters_im, lengths) =
        build_wavelet_filters(freqs, sr, filter_scale, norm, gamma);

    // Determine n_fft: at least 2 * hop_length, rounded to power of 2
    let min_fft = (2 * hop_length).next_power_of_two();
    let n_fft = filters_re.cols().max(min_fft);
    let n_fft = n_fft.next_power_of_two();

    let n_freqs = freqs.len();
    let n_bins_fft = n_fft / 2 + 1;

    // FFT each filter row (complex FFT via separate real+imag FFTs)
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n_fft);
    let mut input_buf = fft.make_input_vec();
    let mut output_buf = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();

    let mut basis_re_data = vec![0.0; n_freqs * n_bins_fft];
    let mut basis_im_data = vec![0.0; n_freqs * n_bins_fft];

    for k in 0..n_freqs {
        // Renormalize: basis[k, :] *= lengths[k] / n_fft
        let scale = lengths[k] / n_fft as f64;

        // Center the filter in n_fft
        let filter_cols = filters_re.cols();
        let pad_start = if n_fft > filter_cols {
            (n_fft - filter_cols) / 2
        } else {
            0
        };
        let copy_len = filter_cols.min(n_fft);

        // FFT the real part
        input_buf.fill(0.0);
        for j in 0..copy_len {
            input_buf[pad_start + j] = filters_re.get(k, j) * scale;
        }
        fft.process_with_scratch(&mut input_buf, &mut output_buf, &mut scratch)
            .expect("FFT failed");

        let mut fft_re_re = vec![0.0; n_bins_fft];
        let mut fft_re_im = vec![0.0; n_bins_fft];
        for j in 0..n_bins_fft {
            fft_re_re[j] = output_buf[j].re;
            fft_re_im[j] = output_buf[j].im;
        }

        // FFT the imaginary part
        input_buf.fill(0.0);
        for j in 0..copy_len {
            input_buf[pad_start + j] = filters_im.get(k, j) * scale;
        }
        fft.process_with_scratch(&mut input_buf, &mut output_buf, &mut scratch)
            .expect("FFT failed");

        // F{complex_filter} = F{re} + j * F{im}
        // = (fft_re_re + j*fft_re_im) + j*(fft_im_re + j*fft_im_im)
        // = (fft_re_re - fft_im_im) + j*(fft_re_im + fft_im_re)
        for j in 0..n_bins_fft {
            let fft_im_re = output_buf[j].re;
            let fft_im_im = output_buf[j].im;
            basis_re_data[k * n_bins_fft + j] = fft_re_re[j] - fft_im_im;
            basis_im_data[k * n_bins_fft + j] = fft_re_im[j] + fft_im_re;
        }
    }

    (
        Matrix::from_vec(basis_re_data, n_freqs, n_bins_fft),
        Matrix::from_vec(basis_im_data, n_freqs, n_bins_fft),
        n_fft,
        lengths,
    )
}

/// Compute CQT response for one octave.
///
/// Matches librosa's magnitude-only CQT: `|basis| @ |STFT|`.
/// Librosa takes `np.abs(fft_basis)` and uses `phase=False` (magnitude STFT),
/// avoiding complex phase cancellation.
pub(crate) fn cqt_response(
    y: &[f64],
    n_fft: usize,
    hop_length: usize,
    basis_re: &Matrix,
    basis_im: &Matrix,
) -> (Matrix, Matrix) {
    // STFT with Hann window (librosa default for CQT)
    let stft_params = StftParams {
        n_fft,
        hop_length,
        win_length: n_fft,
        center: true,
        window: None, // default Hann
    };
    let mut stft = StftFrames::new(y, &stft_params);

    // Magnitude-only matmul: |basis| @ |STFT|
    let n_freqs = basis_re.rows();
    let n_bins_fft = basis_re.cols();
    let n_frames = stft.frame_count();

    // Precompute |basis| and |STFT| magnitudes
    let basis_mag: Vec<Vec<f64>> = (0..n_freqs)
        .map(|b| {
            (0..n_bins_fft)
                .map(|k| (basis_re.get(b, k).powi(2) + basis_im.get(b, k).powi(2)).sqrt())
                .collect()
        })
        .collect();

    let mut stft_mag = vec![0.0; n_bins_fft];

    // Real matmul: resp[b, f] = sum_k basis_mag[b, k] * stft_mag[k, f]
    let response_len = n_freqs
        .checked_mul(n_frames)
        .expect("CQT response dimensions overflow");
    let mut resp_data = vec![0.0; response_len];
    for f in 0..n_frames {
        let spectrum = stft.process_frame(f);
        for k in 0..n_bins_fft {
            // Preserve the dense reference's operations and summation order.
            stft_mag[k] = (spectrum[k].re.powi(2) + spectrum[k].im.powi(2)).sqrt();
        }
        for b in 0..n_freqs {
            let mut sum = 0.0f64;
            for k in 0..n_bins_fft {
                sum += basis_mag[b][k] * stft_mag[k];
            }
            resp_data[b * n_frames + f] = sum;
        }
    }

    (
        Matrix::from_vec(resp_data, n_freqs, n_frames),
        Matrix::zeros(n_freqs, n_frames),
    )
}

/// Stack octave responses, trimming to minimum frame count.
fn trim_stack(responses: &[(Matrix, Matrix, usize, usize)], n_bins: usize) -> (Matrix, Matrix) {
    if responses.is_empty() {
        return (Matrix::zeros(n_bins, 0), Matrix::zeros(n_bins, 0));
    }

    let min_frames = responses
        .iter()
        .map(|(re, _, _, _)| re.cols())
        .min()
        .unwrap_or(0);

    let mut out_re = vec![0.0; n_bins * min_frames];
    let mut out_im = vec![0.0; n_bins * min_frames];

    // Stack from lowest octave (last in list) to highest (first)
    let mut row_offset = 0;
    for (resp_re, resp_im, n_bins_oct, _) in responses.iter().rev() {
        let skip = if resp_re.rows() > *n_bins_oct {
            resp_re.rows() - *n_bins_oct
        } else {
            0
        };
        let bins_to_copy = (*n_bins_oct).min(n_bins - row_offset);

        for b in 0..bins_to_copy {
            let src_row = skip + b;
            let dst_row = row_offset + b;
            for f in 0..min_frames {
                out_re[dst_row * min_frames + f] = resp_re.get(src_row, f);
                out_im[dst_row * min_frames + f] = resp_im.get(src_row, f);
            }
        }
        row_offset += bins_to_copy;
    }

    (
        Matrix::from_vec(out_re, n_bins, min_frames),
        Matrix::from_vec(out_im, n_bins, min_frames),
    )
}

/// Resample audio using rubato (simplified 2:1 downsampler for octave decimation).
pub(crate) fn resample_half(y: &[f64]) -> Vec<f64> {
    use rubato::{
        Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
    };

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };

    if y.is_empty() {
        return Vec::new();
    }
    let expected_len = y.len().div_ceil(2);
    let mut output = bounded_resample_output(expected_len).expect("resample output allocation");
    let chunk_size = 1024.min(y.len());
    let mut resampler =
        SincFixedIn::<f64>::new(0.5, 2.0, params, chunk_size, 1).expect("resampler");

    let mut pos = 0;
    while pos + chunk_size <= y.len() {
        let chunk = vec![y[pos..pos + chunk_size].to_vec()];
        let result = resampler.process(&chunk, None).expect("resample");
        append_resampled_prefix(&mut output, &result[0], expected_len);
        pos += chunk_size;
    }

    if pos < y.len() {
        let remaining = &y[pos..];
        let chunk = vec![remaining.to_vec()];
        let result = resampler
            .process_partial(Some(&chunk), None)
            .expect("resample");
        append_resampled_prefix(&mut output, &result[0], expected_len);
    } else {
        let result = resampler
            .process_partial::<Vec<f64>>(None, None)
            .expect("resample");
        append_resampled_prefix(&mut output, &result[0], expected_len);
    }

    output.resize(expected_len, 0.0);
    output
}

// ============================================================================
// Public API
// ============================================================================

/// Parameters for librosa-compatible CQT.
#[derive(Debug, Clone)]
pub struct CqtConfig {
    /// Sample rate in Hz.
    pub sr: f64,
    /// Hop length in samples.
    pub hop_length: usize,
    /// Minimum frequency in Hz.
    pub fmin: f64,
    /// Number of frequency bins.
    pub n_bins: usize,
    /// Bins per octave.
    pub bins_per_octave: usize,
    /// Tuning offset (default 0.0).
    pub tuning: f64,
    /// Filter scale (default 1.0).
    pub filter_scale: f64,
    /// Normalization: None, Some(1.0) = L1, Some(2.0) = L2.
    pub norm: Option<f64>,
    /// Scale output by sqrt(filter_length).
    pub scale: bool,
}

impl Default for CqtConfig {
    fn default() -> Self {
        Self {
            sr: 22050.0,
            hop_length: 512,
            fmin: 32.703_195_662_574_83, // C1
            n_bins: 84,
            bins_per_octave: 12,
            tuning: 0.0,
            filter_scale: 1.0,
            norm: Some(1.0),
            scale: true,
        }
    }
}

/// Compute the Constant-Q Transform matching `librosa.cqt`.
///
/// Returns the magnitude spectrogram as `Vec<f64>` of shape `(n_bins, n_frames)`
/// in row-major order.
pub fn cqt(y: &[f64], config: &CqtConfig) -> Vec<f64> {
    let (re, im) = cqt_complex(y, config);
    let n_bins = re.rows();
    let n_frames = re.cols();

    // Compute magnitude: sqrt(re^2 + im^2)
    let mut mag = Vec::with_capacity(n_bins * n_frames);
    for i in 0..n_bins * n_frames {
        let r = re.as_slice()[i];
        let i_val = im.as_slice()[i];
        mag.push((r * r + i_val * i_val).sqrt());
    }
    mag
}

/// Compute the Complex Constant-Q Transform matching `librosa.cqt`.
///
/// Returns (re, im) matrices of shape `(n_bins, n_frames)`.
pub fn cqt_complex(y: &[f64], config: &CqtConfig) -> (Matrix, Matrix) {
    let n_octaves = (config.n_bins as f64 / config.bins_per_octave as f64).ceil() as usize;

    // Generate all target frequencies with tuning correction
    let freqs = cqt_frequencies(
        config.n_bins,
        config.fmin,
        config.bins_per_octave,
        config.tuning,
    );

    // Compute all filter lengths to determine f_cutoff and early downsampling
    let gamma = 0.0; // CQT: gamma = 0
    let (all_lengths, f_cutoff) = wavelet_lengths(&freqs, config.sr, config.filter_scale, gamma);

    // Early downsampling
    let nyquist = config.sr / 2.0;
    let num_twos = config.hop_length.trailing_zeros() as usize;

    let downsample_count = if f_cutoff > 0.0 && nyquist > f_cutoff {
        let max_ds = ((nyquist / f_cutoff).log2().floor() as isize - 1 - (n_octaves as isize - 1))
            .max(0) as usize;
        max_ds.min(num_twos)
    } else {
        0
    };

    let mut my_y: Cow<'_, [f64]>;
    let mut my_sr: f64;
    let mut my_hop: usize;

    if downsample_count > 0 {
        let factor = 1u32 << downsample_count;
        let new_sr = (config.sr as u32) / factor;
        my_y = Cow::Owned(resample_by_factor(y, config.sr as u32, new_sr).expect("resample"));
        my_sr = new_sr as f64;
        my_hop = config.hop_length >> downsample_count;
    } else {
        my_y = Cow::Borrowed(y);
        my_sr = config.sr;
        my_hop = config.hop_length;
    }

    let original_sr = config.sr;
    // Store (response_re, response_im, n_bins_oct, n_fft) per octave
    let mut responses: Vec<(Matrix, Matrix, usize, usize)> = Vec::with_capacity(n_octaves);

    // Process octaves from highest frequency to lowest
    for oct in 0..n_octaves {
        let bin_start = config
            .n_bins
            .saturating_sub((oct + 1) * config.bins_per_octave);
        let bin_end = config.n_bins.saturating_sub(oct * config.bins_per_octave);
        let n_bins_oct = bin_end - bin_start;

        if n_bins_oct == 0 {
            continue;
        }

        let freqs_oct = &freqs[bin_start..bin_end];

        // Build filter basis for this octave
        let (basis_re, basis_im, n_fft, _lengths_oct) = vqt_filter_fft(
            my_sr,
            freqs_oct,
            config.filter_scale,
            config.norm,
            my_hop,
            gamma,
        );

        // Scale basis by sqrt(original_sr / my_sr) for resampling energy compensation
        let sr_scale = (original_sr / my_sr).sqrt();
        let basis_re = basis_re.map(|v| v * sr_scale);
        let basis_im = basis_im.map(|v| v * sr_scale);

        // Compute response for this octave
        let (resp_re, resp_im) = cqt_response(&my_y, n_fft, my_hop, &basis_re, &basis_im);

        responses.push((resp_re, resp_im, n_bins_oct, n_fft));

        // Downsample for next octave (if hop_length is even)
        if oct < n_octaves - 1 && my_hop.is_multiple_of(2) {
            my_y = Cow::Owned(resample_half(&my_y));
            my_sr /= 2.0;
            my_hop /= 2;
        }
    }

    // Stack all octave responses
    let (mut out_re, mut out_im) = trim_stack(&responses, config.n_bins);

    // Apply scaling to match librosa:
    // scale=True:  C /= sqrt(n_fft)  (per octave)
    // scale=False: C *= sqrt(lengths / n_fft) (per bin)
    if config.scale {
        let mut row_offset = 0usize;
        for (_, _, n_bins_oct, n_fft) in responses.iter().rev() {
            let n_bins_oct = *n_bins_oct;
            let n_fft = *n_fft;
            let bins_to_copy = n_bins_oct.min(config.n_bins - row_offset);
            let scale = 1.0 / (n_fft as f64).sqrt();
            for b in 0..bins_to_copy {
                for f in 0..out_re.cols() {
                    *out_re.get_mut(row_offset + b, f) *= scale;
                }
            }
            row_offset += bins_to_copy;
        }
    } else {
        for (k, &length) in all_lengths.iter().enumerate().take(config.n_bins) {
            let n_fft = find_octave_nfft(k, &responses);
            let scale = (length / n_fft as f64).sqrt();
            for f in 0..out_re.cols() {
                *out_re.get_mut(k, f) *= scale;
                *out_im.get_mut(k, f) *= scale;
            }
        }
    }

    (out_re, out_im)
}

/// Find the n_fft for the octave containing a given bin.
fn find_octave_nfft(bin: usize, responses: &[(Matrix, Matrix, usize, usize)]) -> usize {
    let mut row_offset = 0usize;
    for (_, _, n_bins_oct, n_fft) in responses.iter().rev() {
        let n_bins_oct = *n_bins_oct;
        let n_fft = *n_fft;
        if bin < row_offset + n_bins_oct {
            return n_fft;
        }
        row_offset += n_bins_oct;
    }
    responses.last().map(|r| r.3).unwrap_or(1)
}

pub(crate) fn resample_by_factor(y: &[f64], orig_sr: u32, target_sr: u32) -> Option<Vec<f64>> {
    if orig_sr == target_sr {
        let mut output = bounded_resample_output(y.len())?;
        output.extend_from_slice(y);
        return Some(output);
    }

    let ratio = target_sr as f64 / orig_sr as f64;
    let expected_len = (y.len() as f64 * ratio).ceil() as usize;
    let mut output = bounded_resample_output(expected_len)?;
    if y.is_empty() {
        return Some(output);
    }

    use rubato::{
        Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
    };

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };

    let chunk_size = 1024.min(y.len());
    let mut resampler = SincFixedIn::<f64>::new(ratio, 2.0, params, chunk_size, 1).ok()?;

    let mut pos = 0;
    while pos + chunk_size <= y.len() {
        let chunk = vec![y[pos..pos + chunk_size].to_vec()];
        let result = resampler.process(&chunk, None).ok()?;
        append_resampled_prefix(&mut output, &result[0], expected_len);
        pos += chunk_size;
    }

    if pos < y.len() {
        let remaining = &y[pos..];
        let chunk = vec![remaining.to_vec()];
        let result = resampler.process_partial(Some(&chunk), None).ok()?;
        append_resampled_prefix(&mut output, &result[0], expected_len);
    } else {
        let result = resampler.process_partial::<Vec<f64>>(None, None).ok()?;
        append_resampled_prefix(&mut output, &result[0], expected_len);
    }

    output.resize(expected_len, 0.0);
    Some(output)
}

/// Reserve once before constructing a resampler, with no geometric Vec growth.
fn bounded_resample_output(expected_len: usize) -> Option<Vec<f64>> {
    expected_len.checked_mul(size_of::<f64>())?;
    let mut output = Vec::new();
    output.try_reserve_exact(expected_len).ok()?;
    // Admission accounts for capacities, rather than only final lengths.
    if output.capacity() != expected_len {
        return None;
    }
    Some(output)
}

fn append_resampled_prefix(output: &mut Vec<f64>, result: &[f64], expected_len: usize) {
    let remaining = expected_len - output.len();
    output.extend_from_slice(&result[..result.len().min(remaining)]);
}

#[cfg(test)]
mod bounded_tests {
    use super::super::stft::stft_as_real_imag;
    use super::*;

    #[test]
    fn pinned_default_octave_dimensions_fit_the_working_reservation() {
        let freqs = cqt_frequencies(105, 65.0, 24, 0.0);
        let (_, cutoff) = wavelet_lengths(&freqs, 44100.0, 1.0, 0.0);
        let maximum_early_halves = ((22050.0 / cutoff).log2().floor() as isize - 1 - 4).max(0);
        assert_eq!(maximum_early_halves, 0);
        let mut rate = 44100.0;
        let mut hop = 8820_usize;
        for (octave, expected_fft) in [32768, 16384, 8192, 8192, 8192].into_iter().enumerate() {
            let start = 105_usize.saturating_sub((octave + 1) * 24);
            let end = 105_usize.saturating_sub(octave * 24);
            let (real, imaginary, n_fft, _) =
                vqt_filter_fft(rate, &freqs[start..end], 1.0, Some(1.0), hop, 0.0);
            assert_eq!(n_fft, expected_fft);
            // Two original bases, two mapped bases and the magnitude basis are
            // the largest simultaneous filter data during a response.
            let basis_overlap =
                (real.as_slice().len() + imaginary.as_slice().len()) * size_of::<f64>() * 5 / 2;
            assert!(basis_overlap < 32768 * 1024);
            if hop.is_multiple_of(2) {
                rate /= 2.0;
                hop /= 2;
            }
        }
    }

    /// Dense pre-streaming response, independent of StftFrames and its indexing.
    fn dense_response(
        samples: &[f64],
        n_fft: usize,
        hop_length: usize,
        basis_re: &Matrix,
        basis_im: &Matrix,
    ) -> Vec<f64> {
        let (real, imaginary) = stft_as_real_imag(
            samples,
            &StftParams {
                n_fft,
                hop_length,
                win_length: n_fft,
                center: true,
                window: None,
            },
        );
        let n_bins_fft = basis_re.cols();
        let basis_mag: Vec<Vec<f64>> = (0..basis_re.rows())
            .map(|bin| {
                (0..n_bins_fft)
                    .map(|index| {
                        (basis_re.get(bin, index).powi(2) + basis_im.get(bin, index).powi(2)).sqrt()
                    })
                    .collect()
            })
            .collect();
        let magnitude: Vec<Vec<f64>> = (0..real.cols())
            .map(|frame| {
                (0..n_bins_fft)
                    .map(|bin| {
                        (real.get(bin, frame).powi(2) + imaginary.get(bin, frame).powi(2)).sqrt()
                    })
                    .collect()
            })
            .collect();
        let mut response = vec![0.0; basis_re.rows() * real.cols()];
        for frame in 0..real.cols() {
            for bin in 0..basis_re.rows() {
                let mut sum = 0.0;
                for index in 0..n_bins_fft {
                    sum += basis_mag[bin][index] * magnitude[frame][index];
                }
                response[bin * real.cols() + frame] = sum;
            }
        }
        response
    }

    #[test]
    fn streamed_cqt_response_preserves_dense_accumulation_bits() {
        for (n_fft, hop_length) in [(32768, 8820), (16384, 4410), (8192, 2205)] {
            let samples: Vec<f64> = (0..2 * hop_length + 29)
                .map(|index| (index as f64 * 0.053).sin() + (index as f64 * 0.071).cos())
                .collect();
            let n_bins_fft = n_fft / 2 + 1;
            let basis_re = Matrix::from_vec(
                (0..3 * n_bins_fft)
                    .map(|index| (index as f64 * 0.37).sin() * 0.01)
                    .collect(),
                3,
                n_bins_fft,
            );
            let basis_im = Matrix::from_vec(
                (0..3 * n_bins_fft)
                    .map(|index| (index as f64 * 0.17).cos() * 0.02)
                    .collect(),
                3,
                n_bins_fft,
            );
            let expected = dense_response(&samples, n_fft, hop_length, &basis_re, &basis_im);
            let (actual, imaginary) =
                cqt_response(&samples, n_fft, hop_length, &basis_re, &basis_im);
            assert_eq!(actual.as_slice().len(), expected.len());
            for (actual, expected) in actual.as_slice().iter().zip(expected) {
                assert_eq!(actual.to_bits(), expected.to_bits());
            }
            assert!(
                imaginary
                    .as_slice()
                    .iter()
                    .all(|value| value.to_bits() == 0)
            );
        }
    }

    /// Original growing/truncating Sinc adapter, including the same final flush.
    fn growing_resample_reference(samples: &[f64], ratio: f64) -> Vec<f64> {
        use rubato::{
            Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType,
            WindowFunction,
        };
        let chunk_size = 1024.min(samples.len());
        let mut resampler = SincFixedIn::<f64>::new(
            ratio,
            2.0,
            SincInterpolationParameters {
                sinc_len: 256,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 256,
                window: WindowFunction::BlackmanHarris2,
            },
            chunk_size,
            1,
        )
        .unwrap();
        // The default downsampler's process allocations fit the fixed envelope.
        assert!(resampler.output_frames_max() <= 2048);
        let mut output = Vec::new();
        let mut position = 0;
        while position + chunk_size <= samples.len() {
            let result = resampler
                .process(&[samples[position..position + chunk_size].to_vec()], None)
                .unwrap();
            output.extend_from_slice(&result[0]);
            position += chunk_size;
        }
        let result = if position < samples.len() {
            resampler
                .process_partial(Some(&[samples[position..].to_vec()]), None)
                .unwrap()
        } else {
            resampler.process_partial::<Vec<f64>>(None, None).unwrap()
        };
        output.extend_from_slice(&result[0]);
        let expected_len = (samples.len() as f64 * ratio).ceil() as usize;
        output.truncate(expected_len);
        output.resize(expected_len, 0.0);
        output
    }

    #[test]
    fn exact_capacity_sinc_outputs_preserve_complete_reference_bits() {
        for length in [1, 1023, 1024, 1025, 8193] {
            let samples: Vec<f64> = (0..length)
                .map(|index| (index as f64 * 0.071).sin())
                .collect();
            for target_rate in [11025, 22050] {
                let expected = growing_resample_reference(&samples, target_rate as f64 / 44100.0);
                let actual = resample_by_factor(&samples, 44100, target_rate).unwrap();
                assert_eq!(actual.capacity(), actual.len());
                assert_eq!(actual.len(), expected.len());
                for (actual, expected) in actual.iter().zip(&expected) {
                    assert_eq!(actual.to_bits(), expected.to_bits());
                }
                if target_rate == 22050 {
                    let half = resample_half(&samples);
                    assert_eq!(half.capacity(), half.len());
                    for (actual, expected) in half.iter().zip(expected) {
                        assert_eq!(actual.to_bits(), expected.to_bits());
                    }
                }
            }
        }
        assert!(bounded_resample_output(usize::MAX).is_none());
        assert!(default_cqt_working_bytes(usize::MAX).is_none());
    }
}
#[cfg(test)]
#[path = "scaling_review_tests.rs"]
mod scaling_review_tests;
