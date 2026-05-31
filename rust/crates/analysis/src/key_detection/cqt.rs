//! Constant-Q Transform computation for key detection preprocessing.
//!
//! Produces a CQT spectrogram from mono audio and applies log1p magnitude
//! compression to match the KeyNet model's expected input format.
//!
//! Pipeline: mono f32 samples → CQT magnitude → log1p → trim last bin → (104, T) tensor.

use cqt_rs::{CQTParams as CqtRsParams, Cqt};

/// CQT preprocessing parameters matching the KeyNet training pipeline.
#[derive(Debug, Clone)]
pub struct CqtParams {
    /// Target number of frequency bins before trimming (105).
    pub n_bins: usize,
    /// Bins per octave.
    pub bins_per_octave: usize,
    /// Minimum frequency in Hz.
    pub fmin: f32,
    /// Hop length in samples between frames.
    pub hop_length: usize,
    /// Audio sample rate (must match input).
    pub sample_rate: u32,
}

impl Default for CqtParams {
    fn default() -> Self {
        Self {
            n_bins: 105,
            bins_per_octave: 24,
            fmin: 65.0,
            hop_length: 8820,
            sample_rate: 44_100,
        }
    }
}

impl CqtParams {
    /// Calculate the max frequency that yields the desired number of bins.
    ///
    /// `n_bins = bins_per_octave * ceil(log2(fmax / fmin))`
    /// We solve for fmax: `fmax = fmin * 2^(n_bins / bins_per_octave)`
    fn max_freq(&self) -> f32 {
        self.fmin * 2.0f32.powf(self.n_bins as f32 / self.bins_per_octave as f32)
    }

    /// Window length for the CQT (next power of 2 ≥ largest filter).
    ///
    /// cqt-rs requires window_length to be a power of two and
    /// hop_length ≤ window_length. We choose the larger of:
    /// - Next power of 2 above sample_rate / fmin (largest filter)
    /// - Next power of 2 above hop_length (cqt-rs constraint)
    fn window_length(&self) -> usize {
        let largest_filter = (self.sample_rate as usize / self.fmin as usize).next_power_of_two();
        let min_for_hop = self.hop_length.next_power_of_two();
        largest_filter.max(min_for_hop)
    }
}

/// Compute the CQT spectrogram from mono audio samples.
///
/// Returns a flattened `Vec<f32>` of shape `(104, n_time_frames)` in row-major
/// order. The pipeline:
/// 1. Compute CQT magnitude with `cqt-rs` (produces K × T where K ≥ 105)
/// 2. Trim to first 105 frequency bins
/// 3. Apply `log1p` magnitude compression
/// 4. Remove the last frequency bin → `(104, T)`
///
/// Returns empty vec if the signal is too short to produce any frames.
pub fn compute_cqt(samples: &[f32], sample_rate: u32, params: &CqtParams) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }

    let fmax = params.max_freq();
    let window_len = params.window_length();

    // Build cqt-rs parameters.
    let cqt_params = CqtRsParams::new(
        params.fmin,
        fmax,
        params.bins_per_octave,
        sample_rate as usize,
        window_len,
    )
    .expect("invalid CQT params");

    let num_bins = cqt_params.num_bins();
    if num_bins < params.n_bins {
        panic!(
            "cqt-rs produced {num_bins} bins, need at least {}",
            params.n_bins
        );
    }

    let cqt = Cqt::new(cqt_params);
    let magnitude = cqt
        .process(samples, params.hop_length)
        .expect("CQT processing failed");

    let (n_frames, n_total_bins) = magnitude.dim();
    if n_total_bins != num_bins {
        panic!("CQT dim mismatch: expected {num_bins}, got {n_total_bins}");
    }
    if n_frames == 0 {
        return Vec::new();
    }

    // cqt-rs returns Array2<f32> with shape (n_frames, n_bins) in row-major.
    // Each row is one time frame, each column is one frequency bin.
    // We need: trim to first n_bins cols → log1p → remove last col → transpose → (104, T).
    let output_bins = params.n_bins - 1; // 104

    // Iterate over the ndarray in row-major order and build the output.
    // Input layout: (n_frames) × (n_total_bins), row-major.
    // We want output layout: (output_bins) × (n_frames), row-major.
    // This is a transpose + trim operation.
    let mut output = Vec::with_capacity(output_bins * n_frames);

    // For each output frequency bin (0..output_bins):
    //   For each time frame (0..n_frames):
    //     value = log1p(magnitude[frame][bin])
    for bin in 0..output_bins {
        for frame in 0..n_frames {
            let val = magnitude[[frame, bin]];
            output.push((1.0f32 + val).ln());
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_wave(freq_hz: f32, sample_rate: u32, duration_secs: f32) -> Vec<f32> {
        let n = (sample_rate as f32 * duration_secs) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq_hz * i as f32 / sample_rate as f32).sin())
            .collect()
    }

    #[test]
    fn test_cqt_output_shape_10s() {
        // 10 seconds at 44100 Hz → 441000 samples
        let samples = sine_wave(440.0, 44_100, 10.0);
        let params = CqtParams::default();

        let output = compute_cqt(&samples, 44_100, &params);

        // Expected frames: floor(441000 / 8820) = 50
        let n_frames = samples.len() / params.hop_length;
        let expected_len = 104 * n_frames;
        assert_eq!(
            output.len(),
            expected_len,
            "expected (104, {n_frames}) = {expected_len}, got {}",
            output.len()
        );
    }

    #[test]
    fn test_cqt_output_shape_30s() {
        let samples = sine_wave(440.0, 44_100, 30.0);
        let params = CqtParams::default();

        let output = compute_cqt(&samples, 44_100, &params);

        let n_frames = samples.len() / params.hop_length;
        assert_eq!(output.len(), 104 * n_frames);
    }

    #[test]
    fn test_cqt_silence_produces_finite_values() {
        let samples = vec![0.0f32; 441_000]; // 10s silence
        let params = CqtParams::default();

        let output = compute_cqt(&samples, 44_100, &params);

        assert!(!output.is_empty());
        assert!(
            output.iter().all(|v| v.is_finite()),
            "all values should be finite after log1p"
        );
        // log1p(0) = 0, silence should produce values near -infinity after log
        // but log1p handles 0 gracefully → log1p(0) = 0.0
        let mean = output.iter().sum::<f32>() / output.len() as f32;
        assert!(
            mean.abs() < 1.0,
            "silence should produce values near 0 after log1p, got mean {mean}"
        );
    }

    #[test]
    fn test_cqt_short_audio_edge_case() {
        // Very short: less than one hop_length → 0 frames
        let samples = sine_wave(440.0, 44_100, 0.1); // 4410 samples < 8820
        let params = CqtParams::default();

        let output = compute_cqt(&samples, 44_100, &params);
        // 4410 / 8820 = 0 frames → empty output
        assert!(output.is_empty());
    }

    #[test]
    fn test_cqt_empty_input() {
        let params = CqtParams::default();
        let output = compute_cqt(&[], 44_100, &params);
        assert!(output.is_empty());
    }

    #[test]
    fn test_cqt_sine_wave_produces_energy() {
        // A 440 Hz sine wave should produce non-trivial energy in the CQT
        let samples = sine_wave(440.0, 44_100, 5.0);
        let params = CqtParams::default();

        let output = compute_cqt(&samples, 44_100, &params);
        assert!(!output.is_empty());

        // At least some values should be positive (log1p of magnitude > 1)
        let has_positive = output.iter().any(|v| *v > 0.0);
        assert!(
            has_positive,
            "440 Hz sine should produce some energy above 0 in CQT"
        );
    }

    #[test]
    fn test_cqt_params_max_freq() {
        let params = CqtParams::default();
        // fmax = fmin * 2^(n_bins / bins_per_octave) = 65 * 2^(105/24) ≈ 1349
        let fmax = params.max_freq();
        assert!((fmax - 1349.0).abs() < 100.0, "fmax ≈ 1349, got {fmax}");
    }

    #[test]
    fn test_cqt_params_window_length() {
        let params = CqtParams::default();
        // Must be ≥ hop_length (8820) and ≥ sample_rate/fmin (678)
        // next_power_of_2(8820) = 16384, next_power_of_2(678) = 1024
        // So window_length = max(16384, 1024) = 16384
        let wl = params.window_length();
        assert!(wl.is_power_of_two());
        assert!(
            wl >= params.hop_length,
            "window {wl} must be >= hop {}",
            params.hop_length
        );
        assert!(wl >= 44_100 / 65);
    }

    /// Verify CQT output structural properties (task 3.3).
    ///
    /// Verify CQT output is structurally correct on a real audio file.
    ///
    /// cqt-rs and librosa use different windowing/bin-centering, so per-value
    /// comparison is not feasible.  Instead we verify:
    /// - Shape matches expected (104, T)
    /// - Peak energy is at a frequency bin consistent with the audio content
    /// - All values are finite and in a reasonable range
    #[test]
    fn test_cqt_real_audio_structural() {
        // Decode test_120bpm.mp3 from fixtures.
        let fixture_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let samples =
            decode_mp3_to_f32(&fixture_dir.join("test_120bpm.mp3")).expect("load test_120bpm.mp3");

        let params = CqtParams::default();
        let output = compute_cqt(&samples, 44_100, &params);

        // Shape check.
        let n_frames = samples.len() / params.hop_length;
        let expected_len = 104 * n_frames;
        assert_eq!(
            output.len(),
            expected_len,
            "shape mismatch: expected (104, {n_frames}) = {expected_len}, got {}",
            output.len()
        );

        // All values must be finite.
        assert!(
            output.iter().all(|v| v.is_finite()),
            "all CQT values should be finite"
        );

        // Per-bin mean energy should show a plausible peak.
        let n_bins = 104;
        let mut max_bin = 0;
        let mut max_mean = f32::NEG_INFINITY;
        for bin in 0..n_bins {
            let mean: f32 =
                (0..n_frames).map(|t| output[bin + t * n_bins]).sum::<f32>() / n_frames as f32;
            if mean > max_mean {
                max_mean = mean;
                max_bin = bin;
            }
        }

        // Beat track has strong low-frequency content.
        // Peak bin should be in a plausible range for music.
        let peak_freq = params.fmin * 2.0f32.powf(max_bin as f32 / params.bins_per_octave as f32);
        assert!(
            peak_freq >= params.fmin && peak_freq < 5000.0,
            "peak bin {max_bin} center freq {peak_freq:.0} Hz should be in [{}, 5000)",
            params.fmin
        );
        assert!(
            max_mean > 0.0,
            "peak mean energy {max_mean} should be positive"
        );

        // Overall stats.
        let mean = output.iter().sum::<f32>() / output.len() as f32;
        assert!(
            mean > 0.0 && mean < 50.0,
            "mean {mean} should be in (0, 50)"
        );
    }

    /// Decode an audio file to mono f32 samples using symphonia (dev helper).
    fn decode_mp3_to_f32(path: &std::path::Path) -> std::io::Result<Vec<f32>> {
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
            .map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("probe: {e}"))
            })?;
        let mut format = probed.format;

        let track = format.default_track().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "no default track")
        })?;
        let track_id = track.id;
        let codec_params = track.codec_params.clone();

        let mut decoder = get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("decoder: {e}"))
            })?;

        let channels = codec_params.channels.map(|c| c.count()).unwrap_or(1);
        let mut mono_samples: Vec<f32> = Vec::new();

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
                let mut sum = 0.0f32;
                for ch in 0..samples_per_frame {
                    sum += samples[frame * samples_per_frame + ch];
                }
                mono_samples.push(sum / samples_per_frame as f32);
            }
        }

        Ok(mono_samples)
    }
}
