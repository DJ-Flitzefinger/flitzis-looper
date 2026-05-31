//! Unit tests for librosa-compatible CQT (adapted from rosa).

use super::convert::cqt_frequencies;
use super::windows::hann;

/// Compute Hann window equivalent noise bandwidth (ENBW).
fn window_bandwidth(n: usize) -> f64 {
    let w = hann(n, true);
    let sum_sq: f64 = w.iter().map(|x| x * x).sum();
    let sum_w: f64 = w.iter().sum();
    n as f64 * sum_sq / (sum_w * sum_w)
}

// ============================================================================
// CQT Frequencies — adapted from rosa tests/cqt_test.rs
// ============================================================================

#[test]
fn test_cqt_frequencies_84() {
    // librosa defaults: 84 bins, C1 (32.703…), 12 bpo, tuning=0
    let freqs = cqt_frequencies(84, 32.703_195_662_574_83, 12, 0.0);
    assert_eq!(freqs.len(), 84);

    // Spot-check known values (from librosa):
    // bin 0  = C1  = 32.703…
    // bin 12 = C2  = 65.406…
    // bin 72 = C7  = 2093.004…
    assert!((freqs[0] - 32.703_195_662_574_83).abs() < 1e-10);
    assert!((freqs[12] - 65.406_391_325_149_66).abs() < 1e-10);
    assert!((freqs[72] - 2093.004_522_404_789).abs() < 1e-3);
}

#[test]
fn test_cqt_frequencies_84_tuned() {
    // Tuning = 0.5 (half a semitone sharp)
    let freqs = cqt_frequencies(84, 32.703_195_662_574_83, 12, 0.5);
    assert_eq!(freqs.len(), 84);

    // bin 0 should be 32.703 * 2^(0.5/12) ≈ 33.911971910
    let expected_c1_sharp = 32.703_195_662_574_83 * 2.0f64.powf(0.5 / 12.0);
    assert!(
        (freqs[0] - expected_c1_sharp).abs() < 1e-10,
        "got {}, expected {}",
        freqs[0],
        expected_c1_sharp
    );
}

#[test]
fn test_cqt_frequencies_our_params() {
    // Our exact parameters: 105 bins, fmin=65, 24 bpo
    let freqs = cqt_frequencies(105, 65.0, 24, 0.0);
    assert_eq!(freqs.len(), 105);
    assert!((freqs[0] - 65.0).abs() < 1e-10);

    // Verify monotonic increase
    for i in 1..freqs.len() {
        assert!(
            freqs[i] > freqs[i - 1],
            "freqs[{}] = {} <= freqs[{}] = {}",
            i,
            freqs[i],
            i - 1,
            freqs[i - 1]
        );
    }

    // librosa uses fmin * 2^((n-1)/bpo) for last bin, not fmin * 2^(n/bpo)
    let expected_last = 65.0 * 2.0f64.powf(104.0 / 24.0);
    assert!(
        (freqs[104] - expected_last).abs() < 0.01,
        "last freq {} vs expected {}",
        freqs[104],
        expected_last
    );
}

// ============================================================================
// Window bandwidth — adapted from rosa tests/cqt_test.rs
// ============================================================================

#[test]
fn test_window_bandwidth_hann() {
    // librosa uses periodic Hann (sym=False in scipy) with ENBW ≈ 1.5
    let bw = window_bandwidth(1000);
    assert!(
        (bw - 1.5).abs() < 1e-10,
        "window_bandwidth(1000) = {}, expected ≈ 1.5",
        bw
    );
}

// ============================================================================
// Relative bandwidth — adapted from rosa tests/cqt_test.rs
// ============================================================================

#[test]
fn test_relative_bandwidth_basic() {
    // Equal-tempered 12 bpo → middle bins have same alpha
    let freqs = cqt_frequencies(84, 32.703_195_662_574_83, 12, 0.0);
    let alpha = super::cqt::relative_bandwidth(&freqs);
    assert_eq!(alpha.len(), freqs.len());

    // Middle bins should all have the same alpha (within tolerance)
    let mid_alpha = alpha[42];
    for i in 2..freqs.len() - 2 {
        assert!(
            (alpha[i] - mid_alpha).abs() < 1e-10,
            "alpha[{}] = {} vs mid = {}",
            i,
            alpha[i],
            mid_alpha
        );
    }

    // For 12 bpo, alpha ≈ 0.057698109799852 (not 1/12!)
    // alpha = (2^(2/bpo) - 1) / (2^(2/bpo) + 1) where bpo = bins_per_octave
    let r2 = 2.0_f64.powf(2.0 / 12.0);
    let expected_alpha = (r2 - 1.0) / (r2 + 1.0);
    assert!(
        (mid_alpha - expected_alpha).abs() < 1e-8,
        "mid alpha = {}, expected ≈ {}",
        mid_alpha,
        expected_alpha
    );
}

// ============================================================================
// Wavelet lengths — adapted from rosa tests/cqt_test.rs
// ============================================================================

#[test]
fn test_wavelet_lengths_cqt() {
    let freqs = cqt_frequencies(84, 32.703_195_662_574_83, 12, 0.0);
    let (lengths, cutoff) = super::cqt::wavelet_lengths(&freqs, 22050.0, 1.0, 0.0); // gamma=0 for CQT

    assert_eq!(lengths.len(), 84);

    // Lengths should decrease with frequency (higher freq → shorter filter)
    for i in 1..lengths.len() {
        assert!(
            lengths[i] < lengths[i - 1],
            "lengths[{}] = {} >= lengths[{}] = {}",
            i,
            lengths[i],
            i - 1,
            lengths[i - 1]
        );
    }

    // Cutoff should be above the highest frequency
    assert!(
        cutoff > freqs[83],
        "cutoff {} <= max freq {}",
        cutoff,
        freqs[83]
    );

    // Reference from librosa: lengths[0] ≈ 11685.756 for C1 @ 22050 Hz
    assert!(
        (lengths[0] - 11685.756).abs() < 1.0,
        "lengths[0] = {}, expected ≈ 11685.756",
        lengths[0]
    );
}

#[test]
fn test_wavelet_lengths_our_params() {
    let freqs = cqt_frequencies(105, 65.0, 24, 0.0);
    let (lengths, cutoff) = super::cqt::wavelet_lengths(&freqs, 44100.0, 1.0, 0.0);

    assert_eq!(lengths.len(), 105);

    // All lengths should be positive and finite
    for (i, &l) in lengths.iter().enumerate() {
        assert!(l.is_finite() && l > 0.0, "lengths[{}] = {}", i, l);
    }

    // Lengths decrease monotonically
    for i in 1..lengths.len() {
        assert!(
            lengths[i] < lengths[i - 1],
            "lengths[{}] = {} >= lengths[{}] = {}",
            i,
            lengths[i],
            i - 1,
            lengths[i - 1]
        );
    }

    // Cutoff should be above max frequency
    assert!(cutoff > freqs[104]);
}

// ============================================================================
// Hann window — adapted from rosa tests
// ============================================================================

#[test]
fn test_hann_window_properties() {
    let w = hann(100, true);
    assert_eq!(w.len(), 100);

    // Periodic Hann: starts at 0, ends near 0 (but not exactly 0 for even N)
    assert!(w[0].abs() < 1e-15);
    // For periodic Hann with even N, last sample is sin²(π/(2N)) ≈ 0 but not 0
    assert!(w[99].abs() < 0.001);

    // Peak should be 1.0 near center
    let peak = w.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    assert!((peak - 1.0).abs() < 1e-15);

    // All values should be in [0, 1]
    for (i, &v) in w.iter().enumerate() {
        assert!(v >= 0.0 && v <= 1.0, "hann[{}] = {}", i, v);
    }
}

#[test]
fn test_hann_window_symmetric_odd() {
    // Odd-length periodic Hann: w[k] = sin²(π * k / (N-1))
    // For periodic Hann, first sample is 0, last is sin²(π/(N-1)) ≈ 0
    let w = hann(99, true);
    assert!(w[0].abs() < 1e-15);
    assert!(w[98].abs() < 0.002); // sin²(π/98) ≈ 0.001
    // Check center is near 1.0 (not exactly 1 for odd N periodic)
    assert!((w[49] - 1.0).abs() < 0.001);
}

// ============================================================================
// Matrix operations — adapted from rosa tests
// ============================================================================

#[test]
fn test_matrix_zeros() {
    let m = super::matrix::Matrix::zeros(3, 4);
    assert_eq!(m.rows(), 3);
    assert_eq!(m.cols(), 4);
    for r in 0..3 {
        for c in 0..4 {
            assert!((m.get(r, c) - 0.0).abs() < 1e-15);
        }
    }
}

#[test]
fn test_matrix_matmul_identity() {
    // Identity matrix
    let mut id_data = vec![0.0; 3 * 3];
    for i in 0..3 {
        id_data[i * 3 + i] = 1.0;
    }
    let id = super::matrix::Matrix::from_vec(id_data, 3, 3);

    let mut a_data = vec![0.0; 3 * 2];
    for i in 0..6 {
        a_data[i] = i as f64;
    }
    let a = super::matrix::Matrix::from_vec(a_data, 3, 2);

    let result = id.matmul(&a);
    for r in 0..3 {
        for c in 0..2 {
            assert!(
                (result.get(r, c) - a.get(r, c)).abs() < 1e-12,
                "I @ A mismatch at [{}, {}]",
                r,
                c
            );
        }
    }
}

#[test]
fn test_matrix_map_and_zip() {
    let data = vec![1.0, 2.0, 3.0, 4.0];
    let m = super::matrix::Matrix::from_vec(data, 2, 2);

    let doubled = m.map(|v| v * 2.0);
    assert!((doubled.get(0, 0) - 2.0).abs() < 1e-15);
    assert!((doubled.get(1, 1) - 8.0).abs() < 1e-15);

    let summed = m.zip_map(&doubled, |a, b| a + b);
    assert!((summed.get(0, 0) - 3.0).abs() < 1e-15); // 1 + 2
    assert!((summed.get(1, 1) - 12.0).abs() < 1e-15); // 4 + 8
}

// ============================================================================
// STFT — adapted from rosa tests/stft.rs
// ============================================================================

#[test]
fn test_stft_sine_wave() {
    // 440 Hz sine at 44100 Hz, 256 samples
    let sr = 44100.0;
    let freq = 440.0;
    let n = 256;
    let y: Vec<f64> = (0..n)
        .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / sr).sin())
        .collect();

    let params = super::stft::StftParams {
        n_fft: 256,
        hop_length: 64,
        win_length: 256,
        center: false,
        window: None, // default Hann
    };

    let (re, _im) = super::stft::stft_as_real_imag(&y, &params);
    assert_eq!(re.rows(), 129); // n_fft/2 + 1
    assert!(re.cols() > 0);

    // All values should be finite
    for v in re.as_slice() {
        assert!(v.is_finite());
    }
}

#[test]
fn test_stft_centered_padding() {
    let y = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];

    let params = super::stft::StftParams {
        n_fft: 4,
        hop_length: 2,
        win_length: 4,
        center: true,
        window: None,
    };

    let (re, _im) = super::stft::stft_as_real_imag(&y, &params);
    // With center=true, librosa pads n_fft/2 on each side
    assert!(re.cols() > 0);
    for v in re.as_slice() {
        assert!(v.is_finite());
    }
}

// ============================================================================
// Full CQT integration tests — adapted from rosa tests/cqt_test.rs
// ============================================================================

#[test]
fn test_cqt_sine_wave_energy() {
    // 440 Hz sine at 44100 Hz, 1 second
    let sr = 44100.0;
    let freq = 440.0;
    let n = sr as usize;
    let y: Vec<f64> = (0..n)
        .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / sr).sin())
        .collect();

    let config = super::cqt::CqtConfig {
        sr,
        hop_length: 512,
        fmin: 65.0,
        n_bins: 84,
        bins_per_octave: 12,
        tuning: 0.0,
        filter_scale: 1.0,
        norm: Some(1.0),
        scale: true,
    };

    let mag = super::cqt::cqt(&y, &config);
    let n_frames = mag.len() / 84;
    assert!(n_frames > 0, "CQT should produce frames for 1s audio");

    // Find peak bin
    let mut max_bin = 0;
    let mut max_mean = f64::NEG_INFINITY;
    for bin in 0..84 {
        let mean: f64 =
            (0..n_frames).map(|t| mag[bin * n_frames + t]).sum::<f64>() / n_frames as f64;
        if mean > max_mean {
            max_mean = mean;
            max_bin = bin;
        }
    }

    let freqs = cqt_frequencies(84, 65.0, 12, 0.0);
    let peak_freq = freqs[max_bin];
    assert!(
        (peak_freq - 440.0).abs() < 50.0,
        "peak bin {} at {:.1} Hz should be near 440 Hz",
        max_bin,
        peak_freq
    );
}

/// Debug test: output raw CQT magnitude as JSON for comparison with librosa.
/// Run with: `cargo test -- test_cqt_real_audio_numerical --nocapture`
#[test]
fn test_cqt_real_audio_numerical() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let samples = decode_mp3_to_f64(&fixture_dir.join("test_120bpm.mp3")).expect("load mp3");

    let config = super::cqt::CqtConfig {
        sr: 44100.0,
        hop_length: 8820,
        fmin: 65.0,
        n_bins: 105,
        bins_per_octave: 24,
        tuning: 0.0,
        filter_scale: 1.0,
        norm: Some(1.0),
        scale: false,
    };

    let mag = super::cqt::cqt(&samples, &config);
    let n_frames = mag.len() / 105;

    // Output as JSON to stderr for external comparison
    let json = format!("{{\"n_frames\": {}, \"magnitude\": {:?}}}", n_frames, mag);
    eprintln!("{}", json);
}

fn decode_mp3_to_f64(path: &std::path::Path) -> std::io::Result<Vec<f64>> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;
    use symphonia::default::get_codecs;
    use symphonia::default::get_probe;

    let file = std::fs::File::open(path).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot open {e}"))
    })?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("probe: {e}")))?;
    let mut format = probed.format;

    let track = format
        .default_track()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no default track"))?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    let mut decoder = get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("decoder: {e}"))
        })?;

    let channels = codec_params.channels.map(|c| c.count()).unwrap_or(1);
    let mut mono_samples: Vec<f64> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("packet: {e}"),
                ));
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        let audio_buf = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(_) => continue,
        };

        let spec = *audio_buf.spec();
        let duration: u64 = audio_buf.capacity() as u64;
        let mut sample_buf = SampleBuffer::<f32>::new(duration, spec);
        sample_buf.copy_interleaved_ref(audio_buf);

        let samples = sample_buf.samples();
        let samples_per_frame = channels;
        let frames = samples.len() / samples_per_frame;
        for frame in 0..frames {
            let mut sum = 0.0f64;
            for ch in 0..samples_per_frame {
                sum += samples[frame * samples_per_frame + ch] as f64;
            }
            mono_samples.push(sum / samples_per_frame as f64);
        }
    }

    Ok(mono_samples)
}

/// Debug: trace per-octave CQT energy to find where high-freq bins go wrong.
#[test]
fn test_cqt_debug_octave_energy() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let samples = decode_mp3_to_f64(&fixture_dir.join("test_120bpm.mp3")).expect("load mp3");

    // Manually trace through octaves
    let config = super::cqt::CqtConfig {
        sr: 44100.0,
        hop_length: 8820,
        fmin: 65.0,
        n_bins: 105,
        bins_per_octave: 24,
        tuning: 0.0,
        filter_scale: 1.0,
        norm: Some(1.0),
        scale: false,
    };

    let freqs = cqt_frequencies(
        config.n_bins,
        config.fmin,
        config.bins_per_octave,
        config.tuning,
    );
    let gamma = 0.0;
    let (_, f_cutoff) = super::cqt::wavelet_lengths(&freqs, config.sr, config.filter_scale, gamma);

    let nyquist = config.sr / 2.0;
    let num_twos = config.hop_length.trailing_zeros() as usize;
    let n_octaves = (config.n_bins as f64 / config.bins_per_octave as f64).ceil() as usize;

    let downsample_count = if f_cutoff > 0.0 && nyquist > f_cutoff {
        let max_ds = ((nyquist / f_cutoff).log2().floor() as isize - 1 - (n_octaves as isize - 1))
            .max(0) as usize;
        max_ds.min(num_twos)
    } else {
        0
    };

    eprintln!(
        "n_octaves={}, f_cutoff={:.0}, downsample_count={}",
        n_octaves, f_cutoff, downsample_count
    );

    let mut my_y = samples.clone();
    let mut my_sr = config.sr;
    let mut my_hop = config.hop_length;

    if downsample_count > 0 {
        let factor = 1u32 << downsample_count;
        let new_sr = (config.sr as u32) / factor;
        my_y =
            super::cqt::resample_by_factor(&samples, config.sr as u32, new_sr).expect("resample");
        my_sr = new_sr as f64;
        my_hop = config.hop_length >> downsample_count;
    }

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
        let (basis_re, basis_im, n_fft, _) = super::cqt::vqt_filter_fft(
            my_sr,
            freqs_oct,
            config.filter_scale,
            config.norm,
            my_hop,
            gamma,
        );

        let sr_scale = (config.sr / my_sr).sqrt();
        let basis_re = basis_re.map(|v| v * sr_scale);
        let basis_im = basis_im.map(|v| v * sr_scale);

        let (resp_re, resp_im) =
            super::cqt::cqt_response(&my_y, n_fft, my_hop, &basis_re, &basis_im);

        let n_frames = resp_re.cols();
        let n_bins_resp = resp_re.rows();

        // Compute per-bin mean magnitude
        let mut max_bin_mean = 0.0f64;
        for b in 0..n_bins_resp {
            let mean: f64 = (0..n_frames)
                .map(|f| {
                    let r = resp_re.get(b, f);
                    let i = resp_im.get(b, f);
                    (r * r + i * i).sqrt()
                })
                .sum::<f64>()
                / n_frames as f64;
            if mean > max_bin_mean {
                max_bin_mean = mean;
            }
        }

        eprintln!(
            "Oct {}: bins {}-{} ({} bins), sr={}, hop={}, n_fft={}, n_frames={}, resp_rows={}, max_bin_mean={:.4}",
            oct,
            bin_start,
            bin_end - 1,
            n_bins_oct,
            my_sr as u32,
            my_hop,
            n_fft,
            n_frames,
            n_bins_resp,
            max_bin_mean
        );

        if oct < n_octaves - 1 && my_hop.is_multiple_of(2) {
            my_y = super::cqt::resample_half(&my_y);
            my_sr /= 2.0;
            my_hop /= 2;
        }
    }
}

/// Debug: check STFT energy for octave 0 parameters.
#[test]
fn test_stft_debug_octave0() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let samples = decode_mp3_to_f64(&fixture_dir.join("test_120bpm.mp3")).expect("load mp3");

    // Octave 0 params: n_fft=32768, hop=8820, boxcar window
    let params = super::stft::StftParams {
        n_fft: 32768,
        hop_length: 8820,
        win_length: 32768,
        center: true,
        window: Some(vec![1.0; 32768]),
    };

    let (re, im) = super::stft::stft_as_real_imag(&samples, &params);
    let n_bins = re.rows();
    let n_frames = re.cols();
    eprintln!("STFT (32768): {}x{}", n_bins, n_frames);

    // Per-bin mean magnitude
    let mut max_bin = 0usize;
    let mut max_mean = 0.0f64;
    for b in 0..n_bins {
        let mean: f64 = (0..n_frames)
            .map(|f| {
                let r = re.get(b, f);
                let i = im.get(b, f);
                (r * r + i * i).sqrt()
            })
            .sum::<f64>()
            / n_frames as f64;
        if mean > max_mean {
            max_mean = mean;
            max_bin = b;
        }
    }
    eprintln!("  max_bin={}, max_mean={:.4}", max_bin, max_mean);

    // Check a few specific bins
    for &b in &[0, 100, 500, 1000, 5000, 10000, 16000] {
        if b < n_bins {
            let mean: f64 = (0..n_frames)
                .map(|f| {
                    let r = re.get(b, f);
                    let i = im.get(b, f);
                    (r * r + i * i).sqrt()
                })
                .sum::<f64>()
                / n_frames as f64;
            eprintln!("  bin {:>5}: mean={:.4}", b, mean);
        }
    }

    // Compare with n_fft=16384 (octave 1)
    let params2 = super::stft::StftParams {
        n_fft: 16384,
        hop_length: 4410,
        win_length: 16384,
        center: true,
        window: Some(vec![1.0; 16384]),
    };

    let (re2, im2) = super::stft::stft_as_real_imag(&samples, &params2);
    let n_bins2 = re2.rows();
    let n_frames2 = re2.cols();
    eprintln!("\nSTFT (16384): {}x{}", n_bins2, n_frames2);

    let mut max_mean2 = 0.0f64;
    for b in 0..n_bins2 {
        let mean: f64 = (0..n_frames2)
            .map(|f| {
                let r = re2.get(b, f);
                let i = im2.get(b, f);
                (r * r + i * i).sqrt()
            })
            .sum::<f64>()
            / n_frames2 as f64;
        if mean > max_mean2 {
            max_mean2 = mean;
        }
    }
    eprintln!("  max_mean={:.4}", max_mean2);
}

/// Debug: check filter basis for octave 0.
#[test]
fn test_basis_debug_octave0() {
    let freqs_oct = cqt_frequencies(105, 65.0, 24, 0.0)[81..105].to_vec();
    eprintln!("Octave 0 freqs: {} to {} Hz", freqs_oct[0], freqs_oct[23]);

    let (basis_re, basis_im, n_fft, lengths) =
        super::cqt::vqt_filter_fft(44100.0, &freqs_oct, 1.0, Some(1.0), 8820, 0.0);

    eprintln!(
        "Basis: {}x{}, n_fft={}",
        basis_re.rows(),
        basis_re.cols(),
        n_fft
    );
    eprintln!("Filter lengths: {} to {}", lengths[0], lengths[23]);

    // Per-filter mean magnitude
    let n_bins_fft = basis_re.cols();
    for f in 0..basis_re.rows() {
        let mean: f64 = (0..n_bins_fft)
            .map(|b| {
                let r = basis_re.get(f, b);
                let i = basis_im.get(f, b);
                (r * r + i * i).sqrt()
            })
            .sum::<f64>()
            / n_bins_fft as f64;
        if f % 6 == 0 || f == 23 {
            eprintln!(
                "  filter {:>2} (freq={:.0}): mean_mag={:.6}",
                f, freqs_oct[f], mean
            );
        }
    }

    // Check STFT for same params
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let samples = decode_mp3_to_f64(&fixture_dir.join("test_120bpm.mp3")).expect("load mp3");

    let params = super::stft::StftParams {
        n_fft,
        hop_length: 8820,
        win_length: n_fft,
        center: true,
        window: Some(vec![1.0; n_fft]),
    };
    let (stft_re, stft_im) = super::stft::stft_as_real_imag(&samples, &params);
    eprintln!("\nSTFT: {}x{}", stft_re.rows(), stft_re.cols());

    // Manual magnitude-only matmul for first filter, first frame
    let mut mag_val = 0.0f64;
    for k in 0..n_bins_fft {
        let basis_mag = (basis_re.get(0, k).powi(2) + basis_im.get(0, k).powi(2)).sqrt();
        let stft_mag = (stft_re.get(k, 0).powi(2) + stft_im.get(k, 0).powi(2)).sqrt();
        mag_val += basis_mag * stft_mag;
    }
    eprintln!(
        "Magnitude-only matmul filter 0, frame 0: mag={:.6}",
        mag_val
    );
}

/// Debug: check peak values in filter basis.
#[test]
fn test_basis_peak_debug() {
    let freqs_oct = cqt_frequencies(105, 65.0, 24, 0.0)[81..105].to_vec();

    let (basis_re, basis_im, n_fft, _) =
        super::cqt::vqt_filter_fft(44100.0, &freqs_oct, 1.0, Some(1.0), 8820, 0.0);

    let n_bins_fft = basis_re.cols();
    eprintln!("n_fft={}, n_bins_fft={}", n_fft, n_bins_fft);

    // For filter 0 (674 Hz), find peak bin
    let center_freq = freqs_oct[0];
    let expected_bin = (center_freq * n_fft as f64 / 44100.0) as usize;
    eprintln!(
        "Filter 0: center_freq={:.0} Hz, expected_bin={}",
        center_freq, expected_bin
    );

    let mut peak_bin = 0usize;
    let mut peak_mag = 0.0f64;
    for b in 0..n_bins_fft {
        let r = basis_re.get(0, b);
        let i = basis_im.get(0, b);
        let mag = (r * r + i * i).sqrt();
        if mag > peak_mag {
            peak_mag = mag;
            peak_bin = b;
        }
    }
    eprintln!(
        "  peak_bin={}, peak_mag={:.6}, freq={:.0} Hz",
        peak_bin,
        peak_mag,
        peak_bin as f64 * 44100.0 / n_fft as f64
    );

    // Check values around peak
    for b in (peak_bin.saturating_sub(3)..peak_bin + 4) {
        if b < n_bins_fft {
            let r = basis_re.get(0, b);
            let i = basis_im.get(0, b);
            let mag = (r * r + i * i).sqrt();
            eprintln!("    bin {:>5}: mag={:.6}", b, mag);
        }
    }

    // Compare with octave 1
    let freqs_oct1 = cqt_frequencies(105, 65.0, 24, 0.0)[57..81].to_vec();
    let (basis_re1, basis_im1, n_fft1, _) =
        super::cqt::vqt_filter_fft(22050.0, &freqs_oct1, 1.0, Some(1.0), 4410, 0.0);

    let n_bins_fft1 = basis_re1.cols();
    eprintln!("\nOctave 1: n_fft={}, n_bins_fft={}", n_fft1, n_bins_fft1);

    let center_freq1 = freqs_oct1[0];
    let expected_bin1 = (center_freq1 * n_fft1 as f64 / 22050.0) as usize;
    eprintln!(
        "Filter 0: center_freq={:.0} Hz, expected_bin={}",
        center_freq1, expected_bin1
    );

    let mut peak_mag1 = 0.0f64;
    for b in 0..n_bins_fft1 {
        let r = basis_re1.get(0, b);
        let i = basis_im1.get(0, b);
        let mag = (r * r + i * i).sqrt();
        if mag > peak_mag1 {
            peak_mag1 = mag;
        }
    }
    eprintln!("  peak_mag={:.6}", peak_mag1);
}

/// Debug: test with n_fft=8192 (hop.next_pow2 instead of 2*hop.next_pow2).
#[test]
fn test_cqt_debug_nfft8192() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let samples = decode_mp3_to_f64(&fixture_dir.join("test_120bpm.mp3")).expect("load mp3");

    let freqs_oct = cqt_frequencies(105, 65.0, 24, 0.0)[81..105].to_vec();

    // Build basis with n_fft=8192 (half of current)
    let (basis_re, basis_im, _lengths) =
        build_basis_with_nfft(44100.0, &freqs_oct, 1.0, Some(1.0), 8820, 0.0, 8192);

    let params = super::stft::StftParams {
        n_fft: 8192,
        hop_length: 8820,
        win_length: 8192,
        center: true,
        window: Some(vec![1.0; 8192]),
    };
    let (stft_re, stft_im) = super::stft::stft_as_real_imag(&samples, &params);

    // Magnitude-only matmul
    let n_freqs = basis_re.rows();
    let n_bins_fft = basis_re.cols();
    let n_frames = stft_re.cols();
    let mut resp_data = vec![0.0; n_freqs * n_frames];
    for f in 0..n_frames {
        for b in 0..n_freqs {
            let mut sum = 0.0f64;
            for k in 0..n_bins_fft {
                let basis_mag = (basis_re.get(b, k).powi(2) + basis_im.get(b, k).powi(2)).sqrt();
                let stft_mag = (stft_re.get(k, f).powi(2) + stft_im.get(k, f).powi(2)).sqrt();
                sum += basis_mag * stft_mag;
            }
            resp_data[b * n_frames + f] = sum;
        }
    }

    // Per-bin mean magnitude
    for &b in &[0, 11, 23] {
        let mean: f64 = (0..n_frames)
            .map(|f| resp_data[b * n_frames + f])
            .sum::<f64>()
            / n_frames as f64;
        eprintln!("bin {}: mean_mag={:.4}", 81 + b, mean);
    }
}

fn build_basis_with_nfft(
    sr: f64,
    freqs: &[f64],
    filter_scale: f64,
    norm: Option<f64>,
    hop_length: usize,
    gamma: f64,
    n_fft: usize,
) -> (super::matrix::Matrix, super::matrix::Matrix, Vec<f64>) {
    use super::cqt::build_wavelet_filters;
    use realfft::RealFftPlanner;

    let (filters_re, filters_im, lengths) =
        build_wavelet_filters(freqs, sr, filter_scale, norm, gamma);

    let n_freqs = freqs.len();
    let n_bins_fft = n_fft / 2 + 1;

    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n_fft);
    let mut input_buf = fft.make_input_vec();
    let mut output_buf = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();

    let filter_cols = filters_re.cols();
    let pad_start = (n_fft - filter_cols) / 2;
    let copy_len = filter_cols.min(n_fft);

    let mut basis_re_data = vec![0.0; n_freqs * n_bins_fft];
    let mut basis_im_data = vec![0.0; n_freqs * n_bins_fft];

    for k in 0..n_freqs {
        let scale = lengths[k] / n_fft as f64;

        // FFT real part
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

        // FFT imaginary part
        input_buf.fill(0.0);
        for j in 0..copy_len {
            input_buf[pad_start + j] = filters_im.get(k, j) * scale;
        }
        fft.process_with_scratch(&mut input_buf, &mut output_buf, &mut scratch)
            .expect("FFT failed");

        for j in 0..n_bins_fft {
            let fft_im_re = output_buf[j].re;
            let fft_im_im = output_buf[j].im;
            basis_re_data[k * n_bins_fft + j] = fft_re_re[j] - fft_im_im;
            basis_im_data[k * n_bins_fft + j] = fft_re_im[j] + fft_im_re;
        }
    }

    (
        super::matrix::Matrix::from_vec(basis_re_data, n_freqs, n_bins_fft),
        super::matrix::Matrix::from_vec(basis_im_data, n_freqs, n_bins_fft),
        lengths,
    )
}

/// Debug: check STFT values at basis peak bins.
#[test]
fn test_stft_at_basis_peak() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let samples = decode_mp3_to_f64(&fixture_dir.join("test_120bpm.mp3")).expect("load mp3");

    let freqs_oct = cqt_frequencies(105, 65.0, 24, 0.0)[81..105].to_vec();
    let (basis_re, basis_im, n_fft, _) =
        super::cqt::vqt_filter_fft(44100.0, &freqs_oct, 1.0, Some(1.0), 8820, 0.0);

    let params = super::stft::StftParams {
        n_fft,
        hop_length: 8820,
        win_length: n_fft,
        center: true,
        window: Some(vec![1.0; n_fft]),
    };
    let (stft_re, stft_im) = super::stft::stft_as_real_imag(&samples, &params);

    // For filter 0 (674 Hz), find basis peak bin and STFT value there
    let mut basis_peak = 0usize;
    let mut basis_peak_mag = 0.0f64;
    for b in 0..basis_re.cols() {
        let mag = (basis_re.get(0, b).powi(2) + basis_im.get(0, b).powi(2)).sqrt();
        if mag > basis_peak_mag {
            basis_peak_mag = mag;
            basis_peak = b;
        }
    }

    // STFT at basis peak bin
    let stft_peak_mag =
        (stft_re.get(basis_peak, 0).powi(2) + stft_im.get(basis_peak, 0).powi(2)).sqrt();
    eprintln!(
        "Filter 0 (674 Hz): basis_peak_bin={}, basis_peak_mag={:.6}, stft_mag={:.6}",
        basis_peak, basis_peak_mag, stft_peak_mag
    );

    // Find STFT peak
    let mut stft_peak_bin = 0usize;
    let mut stft_peak_val = 0.0f64;
    for b in 0..stft_re.rows() {
        let mag = (stft_re.get(b, 0).powi(2) + stft_im.get(b, 0).powi(2)).sqrt();
        if mag > stft_peak_val {
            stft_peak_val = mag;
            stft_peak_bin = b;
        }
    }
    eprintln!(
        "STFT peak: bin={}, mag={:.6}, freq={:.0} Hz",
        stft_peak_bin,
        stft_peak_val,
        stft_peak_bin as f64 * 44100.0 / n_fft as f64
    );

    // Basis value at STFT peak
    let basis_at_stft_peak =
        (basis_re.get(0, stft_peak_bin).powi(2) + basis_im.get(0, stft_peak_bin).powi(2)).sqrt();
    eprintln!("Basis at STFT peak bin: mag={:.6}", basis_at_stft_peak);

    // Manual matmul: sum over all bins
    let n_bins = basis_re.cols();
    let mut re_sum = 0.0f64;
    let mut im_sum = 0.0f64;
    for k in 0..n_bins {
        re_sum += basis_re.get(0, k) * stft_re.get(k, 0) - basis_im.get(0, k) * stft_im.get(k, 0);
        im_sum += basis_re.get(0, k) * stft_im.get(k, 0) + basis_im.get(0, k) * stft_re.get(k, 0);
    }
    let matmul_mag = (re_sum * re_sum + im_sum * im_sum).sqrt();
    eprintln!("Manual matmul mag: {:.6}", matmul_mag);

    // Expected: basis_peak * stft_at_peak (if perfectly aligned)
    eprintln!(
        "Expected (if aligned): {:.6}",
        basis_peak_mag * stft_peak_val
    );
}
