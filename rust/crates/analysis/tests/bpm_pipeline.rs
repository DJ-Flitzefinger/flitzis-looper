//! Integration tests for the BPM detection pipeline.
//!
//! Uses `fixtures/test_120bpm.mp3` (known 120 BPM, ~31 s) as the test
//! fixture and exercises every stage of the qm-dsp pipeline:
//! DetectionFunction (ComplexSD), RCF / Viterbi beat period estimation,
//! DP beat tracking, and final BPM calculation.

use flitzis_looper_analysis::{AnalysisConfig, DetectionFunction, SampleAnalysis, analyze_bpm};
use std::path::PathBuf;

const KNOWN_BPM: f32 = 120.0;

/// Decode an audio file to mono f64 samples at a fixed sample rate using symphonia.
fn decode_audio_to_mono_f64(
    path: &std::path::Path,
    target_sample_rate: u32,
) -> std::io::Result<Vec<f64>> {
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

    let src_rate = codec_params.sample_rate.unwrap_or(44100);
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

        // Copy into f32 SampleBuffer (handles all sample formats)
        let spec = *audio_buf.spec();
        let duration: u64 = audio_buf.capacity() as u64;
        let mut sample_buf = SampleBuffer::<f32>::new(duration, spec);
        sample_buf.copy_interleaved_ref(audio_buf);

        // Downmix to mono
        let samples = sample_buf.samples();
        let samples_per_frame = channels;
        let frames = samples.len() / samples_per_frame;
        for frame in 0..frames {
            let mut sum = 0.0f32;
            for ch in 0..samples_per_frame {
                sum += samples[frame * samples_per_frame + ch];
            }
            mono_samples.push((sum / samples_per_frame as f32) as f64);
        }
    }

    // Resample if needed (simple nearest-neighbor)
    if src_rate != target_sample_rate {
        let ratio = src_rate as f64 / target_sample_rate as f64;
        let resampled_len = (mono_samples.len() as f64 / ratio) as usize;
        let mut resampled = vec![0.0; resampled_len];
        for i in 0..resampled_len {
            let src_idx = (i as f64 * ratio) as usize;
            if src_idx < mono_samples.len() {
                resampled[i] = mono_samples[src_idx];
            }
        }
        Ok(resampled)
    } else {
        Ok(mono_samples)
    }
}

/// Run the full pipeline on the test fixture and return the analysis result.
fn analyze_test_fixture() -> std::io::Result<SampleAnalysis> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let path = PathBuf::from(&manifest_dir).join("tests/fixtures/test_120bpm.mp3");

    let sample_rate = 44_100u32;
    let audio = decode_audio_to_mono_f64(&path, sample_rate)?;

    if audio.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "empty audio",
        ));
    }

    let (bpm, beat_grid) = analyze_bpm(&audio, sample_rate, &AnalysisConfig::default())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;

    Ok(SampleAnalysis {
        bpm,
        key: "unknown".to_string(),
        beat_grid,
    })
}

// Final BPM calculation

#[test]
fn bpm_within_tolerance() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    let diff = (result.bpm - KNOWN_BPM).abs();
    assert!(
        diff <= 5.0,
        "BPM {:.2} is outside ±5 BPM of expected {KNOWN_BPM} (diff={diff:.2})",
        result.bpm
    );
}

#[test]
fn bpm_is_reasonable() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    assert!(
        result.bpm > 0.0 && result.bpm < 300.0,
        "BPM {:.2} is outside reasonable range (0, 300)",
        result.bpm
    );
}

// Beat tracking

#[test]
fn beats_are_strictly_increasing() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    assert!(result.beat_grid.beats.len() > 40);
    for i in 1..result.beat_grid.beats.len() {
        assert!(
            result.beat_grid.beats[i] > result.beat_grid.beats[i - 1],
            "Beats must be strictly increasing: beat[{i}]={:.3} <= beat[{}]={:.3}",
            result.beat_grid.beats[i],
            i - 1,
            result.beat_grid.beats[i - 1]
        );
    }
}

#[test]
fn beats_span_the_audio() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    let last_beat = result.beat_grid.beats.last().copied().unwrap_or(0.0);
    assert!(
        last_beat > 20.0,
        "Last beat at {:.1}s — should span most of the ~31 s fixture",
        last_beat
    );
}

// Downbeat detection

#[test]
fn downbeats_and_bars_retain_the_detected_beat_indices() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    let grid = result.beat_grid;
    assert!(grid.downbeats.len() > 10);
    assert_eq!(grid.bars, grid.downbeats);
    let indices: Vec<usize> = grid
        .downbeats
        .iter()
        .map(|downbeat| {
            grid.beats
                .iter()
                .position(|beat| beat == downbeat)
                .expect("each downbeat must be an actual detected beat")
        })
        .collect();
    assert!(indices[0] < 4);
    assert!(indices.windows(2).all(|pair| pair[1] - pair[0] == 4));
}

#[test]
fn complete_pipeline_keeps_original_sample_coordinates_at_multiple_rates() {
    let config = AnalysisConfig::default();
    for sample_rate_hz in [44_100, 48_000, 96_000] {
        let hop_samples = DetectionFunction::new(sample_rate_hz, &config).step_size_samples();
        let period_samples = 43 * hop_samples;
        let offset_samples = 3 * hop_samples;
        let mut audio = vec![0.0; offset_samples + 32 * period_samples];
        for beat in 0..32 {
            let start = offset_samples + beat * period_samples;
            for offset in 0..64 {
                audio[start + offset] = 1.0 - offset as f64 / 64.0;
            }
        }

        // This entry point is also called by the production engine wrapper.
        let (bpm, grid) = analyze_bpm(&audio, sample_rate_hz, &config).unwrap();
        assert!(grid.beats.len() >= 28, "rate={sample_rate_hz}");
        assert!(
            grid.beats[0] < 0.5,
            "startup must remain in original coordinates"
        );
        assert!(grid.beats.last().unwrap() > &14.0);
        let frame_indices: Vec<f64> = grid
            .beats
            .iter()
            .map(|seconds| {
                let source_sample = *seconds as f64 * sample_rate_hz as f64;
                let frame = (source_sample / hop_samples as f64).round();
                assert!(
                    (source_sample - frame * hop_samples as f64).abs() < 0.25,
                    "beat {seconds} at rate {sample_rate_hz} must use actual hop {hop_samples}"
                );
                frame
            })
            .collect();
        let average_period =
            (frame_indices.last().unwrap() - frame_indices[0]) / (frame_indices.len() - 1) as f64;
        let expected_bpm =
            (60.0 * sample_rate_hz as f64 / (average_period * hop_samples as f64)) as f32;
        assert_eq!(bpm, expected_bpm);
        assert!(
            frame_indices
                .windows(2)
                .all(|pair| (pair[1] - pair[0] - 43.0).abs() <= 1.0)
        );
        assert_eq!(grid.bars, grid.downbeats);
        assert!(grid.downbeats.iter().all(|time| grid.beats.contains(time)));
    }
}
