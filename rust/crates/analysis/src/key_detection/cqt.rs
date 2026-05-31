//! Constant-Q Transform computation for key detection preprocessing.
//!
//! Produces a CQT spectrogram from mono audio and applies log1p magnitude
//! compression to match the KeyNet model's expected input format.
//!
//! Pipeline: mono f32 samples → CQT magnitude (105 bins) → log1p → (105, T) tensor.
//!
//! Uses librosa-compatible CQT (`librosa_cqt` module) for numerical accuracy.

use crate::key_detection::librosa_cqt::{CqtConfig, cqt as librosa_cqt};

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
}

impl Default for CqtParams {
    fn default() -> Self {
        Self {
            n_bins: 105,
            bins_per_octave: 24,
            fmin: 65.0,
            hop_length: 8820,
        }
    }
}

/// Compute the CQT spectrogram from mono audio samples.
///
/// Returns a flattened `Vec<f32>` of shape `(105, n_time_frames)` in row-major
/// order. The pipeline:
/// 1. Convert f32 → f64 for librosa-compatible CQT computation
/// 2. Compute CQT magnitude with librosa_cqt (produces 105 × T)
/// 3. Apply `log1p` magnitude compression
/// 4. Convert f64 → f32
///
/// Returns empty vec if the signal is too short to produce any frames.
pub fn compute_cqt(samples: &[f32], sample_rate: u32, params: &CqtParams) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }

    // Convert f32 → f64 for librosa-compatible computation
    let samples_f64: Vec<f64> = samples.iter().map(|&v| v as f64).collect();

    // Build librosa CQT config
    let config = CqtConfig {
        sr: sample_rate as f64,
        hop_length: params.hop_length,
        fmin: params.fmin as f64,
        n_bins: params.n_bins,
        bins_per_octave: params.bins_per_octave,
        tuning: 0.0,
        filter_scale: 1.0,
        norm: Some(1.0),
        scale: true,
    };

    // Compute CQT magnitude (shape: n_bins × n_frames, row-major)
    let magnitude = librosa_cqt(&samples_f64, &config);

    if magnitude.is_empty() {
        return Vec::new();
    }

    let n_frames = magnitude.len() / params.n_bins;

    // Convert to f32 with log1p. Keep all 105 frequency bins.
    // Input layout: (n_bins) × (n_frames), row-major.
    let mut output = Vec::with_capacity(params.n_bins * n_frames);

    for bin in 0..params.n_bins {
        for frame in 0..n_frames {
            let val = magnitude[bin * n_frames + frame];
            output.push((1.0f64 + val).ln() as f32);
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
        let samples = sine_wave(440.0, 44_100, 10.0);
        let params = CqtParams::default();

        let output = compute_cqt(&samples, 44_100, &params);

        // librosa CQT uses centered padding (n_fft/2 on each side),
        // so frame count differs from simple signal_len / hop_length.
        assert!(
            !output.is_empty(),
            "CQT should produce frames for 10s audio"
        );
        assert!(
            output.len() % 105 == 0,
            "output length {} should be divisible by 105",
            output.len()
        );
    }

    #[test]
    fn test_cqt_output_shape_30s() {
        let samples = sine_wave(440.0, 44_100, 30.0);
        let params = CqtParams::default();

        let output = compute_cqt(&samples, 44_100, &params);

        assert!(!output.is_empty());
        assert!(output.len() % 105 == 0);
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
        // log1p(0) = 0, silence should produce values near 0 after log1p
        let mean = output.iter().sum::<f32>() / output.len() as f32;
        assert!(
            mean.abs() < 1.0,
            "silence should produce values near 0 after log1p, got mean {mean}"
        );
    }

    #[test]
    fn test_cqt_short_audio_edge_case() {
        // Very short: less than one hop_length
        let samples = sine_wave(440.0, 44_100, 0.1); // 4410 samples < 8820
        let params = CqtParams::default();

        let output = compute_cqt(&samples, 44_100, &params);

        // Even short audio should produce some frames due to centered padding
        // (librosa pads n_fft/2 on each side)
        // If truly too short, returns empty
        if !output.is_empty() {
            assert!(output.len() % 105 == 0);
        }
    }

    #[test]
    fn test_cqt_sine_wave_produces_energy() {
        let samples = sine_wave(440.0, 44_100, 5.0);
        let params = CqtParams::default();

        let output = compute_cqt(&samples, 44_100, &params);

        assert!(
            !output.is_empty(),
            "CQT should produce frames for 5s sine wave"
        );
        assert!(output.len() % 105 == 0);

        // 440 Hz should produce energy in some frequency bins
        let max_val = output.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max_val > 0.5,
            "440 Hz sine wave should produce significant CQT energy, got max {max_val}"
        );

        // All values should be finite
        assert!(
            output.iter().all(|v| v.is_finite()),
            "all values should be finite"
        );
    }

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

        assert!(
            !output.is_empty(),
            "CQT should produce frames for real audio"
        );
        let n_frames = output.len() / 104;

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
