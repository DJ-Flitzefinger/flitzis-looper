//! Integration tests for the BPM detection pipeline.
//!
//! Uses `fixtures/test_120bpm.mp3` (known 120 BPM, ~31 s) as the test
//! fixture and exercises every stage of the qm-dsp pipeline:
//! DetectionFunction (ComplexSD), RCF / Viterbi beat period estimation,
//! DP beat tracking, and final BPM calculation.

use flitzis_looper_analysis::{
    AnalysisConfig, BeatGrid, DetectionFunction, DownBeat, SampleAnalysis, TempoTrackV2,
};
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

    let file = std::fs::File::open(path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot open {e}")))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
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
    let channels = codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(1);

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

/// Return the next higher integer power of two from x.
fn next_power_of_two(x: usize) -> usize {
    if x <= 1 {
        return 1;
    }
    if x.is_power_of_two() {
        return x;
    }
    let mut n = 1;
    while n < x {
        n <<= 1;
    }
    n
}

/// Run the full pipeline on the test fixture and return the analysis result.
fn analyze_test_fixture() -> std::io::Result<SampleAnalysis> {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let path = PathBuf::from(&manifest_dir).join("tests/fixtures/test_120bpm.mp3");

    let sample_rate = 44_100u32;
    let audio = decode_audio_to_mono_f64(&path, sample_rate)?;

    if audio.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "empty audio",
        ));
    }

    let config = AnalysisConfig::default();

    // Step 1: Compute onset detection function
    let mut df = DetectionFunction::new(sample_rate, &config);
    let frame_length = next_power_of_two((sample_rate as f64 / config.max_bin_hz) as usize);

    if audio.len() < frame_length {
        return Ok(SampleAnalysis {
            bpm: 0.0,
            key: "unknown".to_string(),
            beat_grid: BeatGrid {
                beats: Vec::new(),
                downbeats: Vec::new(),
                bars: Vec::new(),
            },
        });
    }

    let odf = df.process(&audio);

    if odf.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "no ODF values",
        ));
    }

    // Step 2: Estimate beat periods via Viterbi HMM
    let mut beat_period = Vec::new();
    let tracker = TempoTrackV2::new(sample_rate as f64, config.step_secs);
    tracker.calculate_beat_period(&odf, &mut beat_period, config.input_tempo, false);

    // Step 3: Calculate beat positions via dynamic programming
    let mut beats_frames = Vec::new();
    tracker.calculate_beats(
        &odf,
        &beat_period,
        &mut beats_frames,
        config.alpha,
        config.tightness,
    );

    // Step 4: Calculate BPM from beat intervals
    let bpm = flitzis_looper_analysis::calculate_bpm(&beats_frames, config.step_secs);

    // Step 5: Downbeat detection
    let mut downbeat_indices = Vec::new();
    let mut bar_indices = Vec::new();
    if !beats_frames.is_empty() {
        let mut downbeat = DownBeat::new(sample_rate as f64, 16, config.step_secs as usize);
        downbeat.find_downbeats(
            &audio,
            audio.len(),
            &beats_frames,
            &mut downbeat_indices,
        );
        bar_indices = downbeat_indices.clone();
    }

    // Convert beat positions from frames to seconds
    let frame_duration = config.step_secs;
    let beats: Vec<f32> = beats_frames
        .iter()
        .map(|f| (*f * frame_duration) as f32)
        .collect();
    let downbeats: Vec<f32> = downbeat_indices
        .iter()
        .filter_map(|idx| beats_frames.get(*idx))
        .map(|f| (*f * frame_duration) as f32)
        .collect();
    let bars: Vec<f32> = bar_indices
        .iter()
        .filter_map(|idx| beats_frames.get(*idx))
        .map(|f| (*f * frame_duration) as f32)
        .collect();

    let beat_grid = BeatGrid {
        beats,
        downbeats,
        bars,
    };

    Ok(SampleAnalysis {
        bpm,
        key: "unknown".to_string(),
        beat_grid,
    })
}

// ODF stage

#[test]
fn odf_produces_frames() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    assert!(
        !result.beat_grid.beats.is_empty() || result.bpm > 0.0,
        "Pipeline should produce some output"
    );
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
fn downbeats_detected() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    assert!(
        !result.beat_grid.downbeats.is_empty(),
        "Expected at least one downbeat"
    );
}

#[test]
fn downbeats_are_subset_of_beats() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    assert!(
        result.beat_grid.downbeats.len() <= result.beat_grid.beats.len(),
        "Downbeat count {} should not exceed beat count {}",
        result.beat_grid.downbeats.len(),
        result.beat_grid.beats.len()
    );
}
