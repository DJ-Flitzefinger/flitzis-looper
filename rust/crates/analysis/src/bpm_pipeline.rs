//! Shared non-realtime qm-dsp pipeline and its input-sample timebase.

use crate::{AnalysisConfig, BeatGrid, DetectionFunction, DownBeat, TempoTrackV2, calculate_bpm};

// Generous offline dimensional bound, checked before any FFT or history allocation.
const MAX_ANALYSIS_WINDOW_SAMPLES: usize = 1 << 20;

/// Analyze complete mono audio using the legacy qm-dsp backend.
///
/// ODF frame zero retains input sample zero. Beat times and BPM use the actual
/// truncated ODF sample hop divided by `sample_rate_hz`, with no fitted offset.
/// Audio shorter than one full analysis window returns an empty grid and zero BPM.
/// Invalid/nonfinite timebase parameters and windows/hops above 2^20 samples fail
/// before allocation. This limit also applies to the decimated downbeat window.
pub fn analyze_bpm(
    audio: &[f64],
    sample_rate_hz: u32,
    config: &AnalysisConfig,
) -> Result<(f32, BeatGrid), String> {
    if sample_rate_hz == 0 {
        return Err("BPM pipeline: sample rate must be positive".to_string());
    }
    if !config.step_secs.is_finite() || config.step_secs <= 0.0 {
        return Err("BPM pipeline: frame step must be finite and positive".to_string());
    }
    if !config.max_bin_hz.is_finite() || config.max_bin_hz <= 0.0 {
        return Err("BPM pipeline: bin frequency must be finite and positive".to_string());
    }
    let dimension_limit = MAX_ANALYSIS_WINDOW_SAMPLES as f64;
    let requested_window_samples = sample_rate_hz as f64 / config.max_bin_hz;
    let requested_hop_samples = sample_rate_hz as f64 * config.step_secs;
    let downbeat_window_samples = sample_rate_hz as f64 / 16.0 * 1.3;
    if !(1.0..=dimension_limit).contains(&requested_window_samples)
        || !(1.0..=dimension_limit).contains(&requested_hop_samples)
        || !(1.0..=dimension_limit).contains(&downbeat_window_samples)
    {
        return Err(
            "BPM pipeline: analysis windows and hop must be within 1..=2^20 samples".to_string(),
        );
    }

    let mut df = DetectionFunction::new(sample_rate_hz, config);
    let increment_samples = df.step_size_samples();
    if audio.len() < df.frame_length_samples() {
        return Ok((
            0.0,
            BeatGrid {
                beats: Vec::new(),
                downbeats: Vec::new(),
                bars: Vec::new(),
            },
        ));
    }

    let odf = df.process(audio);
    if odf.is_empty() {
        return Err("BPM pipeline: no ODF values produced".to_string());
    }

    let frame_duration_secs = increment_samples as f64 / sample_rate_hz as f64;
    let tracker = TempoTrackV2::new(sample_rate_hz as f64, frame_duration_secs);
    let mut beat_period = Vec::new();
    tracker.calculate_beat_period(&odf, &mut beat_period, config.input_tempo, false);

    let mut beats_frames = Vec::new();
    tracker.calculate_beats(
        &odf,
        &beat_period,
        &mut beats_frames,
        config.alpha,
        config.tightness,
    );

    Ok(beat_grid_from_frames(
        audio,
        sample_rate_hz,
        increment_samples,
        &beats_frames,
    ))
}

fn beat_grid_from_frames(
    audio: &[f64],
    sample_rate_hz: u32,
    increment_samples: usize,
    beats_frames: &[f64],
) -> (f32, BeatGrid) {
    let frame_duration_secs = increment_samples as f64 / sample_rate_hz as f64;
    let bpm = calculate_bpm(beats_frames, frame_duration_secs);

    let mut downbeat_indices = Vec::new();
    if !beats_frames.is_empty() {
        let mut downbeat = DownBeat::new(sample_rate_hz as f64, 16, increment_samples);
        downbeat.find_downbeats(audio, audio.len(), beats_frames, &mut downbeat_indices);
    }

    let beats: Vec<f32> = beats_frames
        .iter()
        .map(|frame| (*frame * frame_duration_secs) as f32)
        .collect();
    let downbeats: Vec<f32> = downbeat_indices.iter().map(|index| beats[*index]).collect();
    let bars = downbeats.clone();
    (
        bpm,
        BeatGrid {
            beats,
            downbeats,
            bars,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A large A-to-B spectral transition at beat 1, then gradual return to A.
    /// Its intended bar phase is 1, deliberately unlike the zero-hop tie at 3.
    fn spectral_bars(
        sample_rate_hz: u32,
        increment_samples: usize,
        offset_frames: usize,
    ) -> (Vec<f64>, Vec<f64>) {
        const PERIOD_FRAMES: usize = 43;
        const BEATS: usize = 32;
        let offset_samples = offset_frames * increment_samples;
        let period_samples = PERIOD_FRAMES * increment_samples;
        let mut audio = vec![0.0; offset_samples + BEATS * period_samples];
        for beat in 0..BEATS {
            let (a, b) = match beat % 4 {
                0 => (1.0, 0.0),
                1 => (0.0, 1.0),
                2 => (0.4, 0.8),
                _ => (0.8, 0.4),
            };
            let start = offset_samples + beat * period_samples;
            for sample in 0..period_samples {
                let t = sample as f64 / sample_rate_hz as f64;
                audio[start + sample] = a * (std::f64::consts::TAU * 48.0 * t).sin()
                    + b * (std::f64::consts::TAU * 112.0 * t).sin();
            }
        }
        let beats = (0..BEATS)
            .map(|beat| (offset_frames + beat * PERIOD_FRAMES) as f64)
            .collect();
        (audio, beats)
    }

    #[test]
    fn actual_sample_hop_recovers_spectral_bar_phase_instead_of_zero_hop_tie() {
        for sample_rate_hz in [44_100, 48_000, 96_000] {
            let config = AnalysisConfig::default();
            let increment_samples =
                DetectionFunction::new(sample_rate_hz, &config).step_size_samples();
            for offset_frames in [0, 3] {
                let (audio, beats_frames) =
                    spectral_bars(sample_rate_hz, increment_samples, offset_frames);
                let (bpm, grid) =
                    beat_grid_from_frames(&audio, sample_rate_hz, increment_samples, &beats_frames);
                let frame_duration_secs = increment_samples as f64 / sample_rate_hz as f64;
                let expected_beats: Vec<f32> = beats_frames
                    .iter()
                    .map(|frame| (*frame * frame_duration_secs) as f32)
                    .collect();
                let expected_downbeats: Vec<f32> = (1..beats_frames.len())
                    .step_by(4)
                    .map(|index| expected_beats[index])
                    .collect();
                assert_eq!(grid.beats, expected_beats);
                assert_eq!(grid.downbeats, expected_downbeats, "rate={sample_rate_hz}");
                assert_eq!(grid.bars, expected_downbeats);
                assert_eq!(bpm, (60.0 / (43.0 * frame_duration_secs)) as f32);
                // No window-center shift, decimation shift or fitted origin correction.
                assert_eq!(
                    grid.beats[0],
                    (offset_frames as f64 * frame_duration_secs) as f32
                );

                let mut historical = DownBeat::new(sample_rate_hz as f64, 16, 0);
                let mut historical_indices = Vec::new();
                historical.find_downbeats(
                    &audio,
                    audio.len(),
                    &beats_frames,
                    &mut historical_indices,
                );
                assert_eq!(
                    historical_indices,
                    (3..beats_frames.len()).step_by(4).collect::<Vec<_>>()
                );
                assert_ne!(grid.downbeats[0], grid.beats[historical_indices[0]]);
            }
        }
    }

    #[test]
    fn short_audio_keeps_the_empty_grid_contract() {
        let config = AnalysisConfig::default();
        for sample_rate_hz in [44_100, 48_000, 96_000] {
            let frame_length =
                DetectionFunction::new(sample_rate_hz, &config).frame_length_samples();
            for length in [0, frame_length - 1] {
                let (bpm, grid) = analyze_bpm(&vec![0.0; length], sample_rate_hz, &config).unwrap();
                assert_eq!(bpm, 0.0);
                assert!(grid.beats.is_empty());
                assert!(grid.downbeats.is_empty());
                assert!(grid.bars.is_empty());
            }
        }
    }

    #[test]
    fn pipeline_rejects_invalid_or_unbounded_timebase_parameters_before_allocation() {
        let config = AnalysisConfig::default();
        assert!(analyze_bpm(&[], 0, &config).is_err());
        for invalid in [0.0, -0.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let step_config = AnalysisConfig {
                step_secs: invalid,
                ..config.clone()
            };
            assert!(analyze_bpm(&[], 44_100, &step_config).is_err());
            let window_config = AnalysisConfig {
                max_bin_hz: invalid,
                ..config.clone()
            };
            assert!(analyze_bpm(&[], 44_100, &window_config).is_err());
        }
        for step_secs in [0.5 / 44_100.0, f64::MIN_POSITIVE, 1e10, f64::MAX] {
            let step_config = AnalysisConfig {
                step_secs,
                ..config.clone()
            };
            assert!(analyze_bpm(&[], 44_100, &step_config).is_err());
        }
        for max_bin_hz in [f64::MIN_POSITIVE, 1e-100, 1e10, f64::MAX] {
            let window_config = AnalysisConfig {
                max_bin_hz,
                ..config.clone()
            };
            assert!(analyze_bpm(&[], 44_100, &window_config).is_err());
        }
        // The ODF dimensions alone would be safe, but the downbeat FFT would not.
        let extreme_rate = u32::MAX;
        let config = AnalysisConfig {
            step_secs: 512.0 / extreme_rate as f64,
            max_bin_hz: extreme_rate as f64 / 1024.0,
            ..config
        };
        assert!(analyze_bpm(&[], extreme_rate, &config).is_err());
    }
}
