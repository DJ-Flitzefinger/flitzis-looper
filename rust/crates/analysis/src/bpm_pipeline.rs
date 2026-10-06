//! Shared non-realtime qm-dsp pipeline and its input-sample timebase.

use crate::{AnalysisConfig, BeatGrid, DetectionFunction, DownBeat, TempoTrackV2, calculate_bpm};

// Generous offline dimensional bound, checked before any FFT or history allocation.
const MAX_ANALYSIS_WINDOW_SAMPLES: usize = 1 << 20;

/// Complete immutable qm-dsp detections before binary32 publication.
///
/// Frame coordinates and downbeat indices retain the detector's original raw
/// associations. The input timebase describes the buffer supplied to this API;
/// it does not attest loaded-PCM/source hashes, quarter-note counts or calibrated
/// timing uncertainty. Config fields are retained even where the legacy tracker
/// currently uses hardcoded defaults. All access and projection is non-realtime.
#[derive(Debug, Clone)]
pub struct QmRawAnalysis {
    beat_frames: Vec<f64>,
    downbeat_raw_indices: Vec<usize>,
    input_sample_rate_hz: u32,
    input_frame_count: usize,
    odf_hop_samples: usize,
    configuration: AnalysisConfig,
}

impl QmRawAnalysis {
    /// Complete detector coordinates in ODF frames, without seconds conversion.
    pub fn beat_frames(&self) -> &[f64] {
        &self.beat_frames
    }

    /// Original raw beat indices selected by the unchanged downbeat detector.
    pub fn downbeat_raw_indices(&self) -> &[usize] {
        &self.downbeat_raw_indices
    }

    /// Sample rate of the complete mono input, before any caller-side mapping.
    pub fn input_sample_rate_hz(&self) -> u32 {
        self.input_sample_rate_hz
    }

    /// Complete input extent, including leading silence and the final partial hop.
    pub fn input_frame_count(&self) -> usize {
        self.input_frame_count
    }

    /// Actual truncated ODF hop in input samples, rather than the requested step.
    pub fn odf_hop_samples(&self) -> usize {
        self.odf_hop_samples
    }

    /// Unmodified configuration supplied to this detector run.
    pub fn configuration(&self) -> &AnalysisConfig {
        &self.configuration
    }

    fn frame_duration_seconds(&self) -> f64 {
        self.odf_hop_samples as f64 / self.input_sample_rate_hz as f64
    }

    /// Iterate every original source-relative binary64 second without allocating.
    ///
    /// Frame zero is input sample zero. No binary32 widening, cropping, fitted
    /// origin correction or independent count interpretation is involved.
    pub fn beat_seconds(&self) -> impl ExactSizeIterator<Item = f64> + '_ {
        let frame_duration = self.frame_duration_seconds();
        self.beat_frames
            .iter()
            .map(move |frame| *frame * frame_duration)
    }

    /// Iterate source seconds by the retained downbeat raw-index associations.
    pub fn downbeat_seconds(&self) -> impl ExactSizeIterator<Item = f64> + '_ {
        let frame_duration = self.frame_duration_seconds();
        self.downbeat_raw_indices
            .iter()
            .map(move |index| self.beat_frames[*index] * frame_duration)
    }

    /// Project the unchanged legacy binary32 BPM/grid without rerunning analysis.
    ///
    /// This compatibility projection is not an accepted constant-tempo result.
    /// It retains the existing allocation behavior without a complete binary64
    /// seconds buffer.
    pub fn legacy_result(&self) -> (f32, BeatGrid) {
        let bpm = calculate_bpm(&self.beat_frames, self.frame_duration_seconds());
        let beats: Vec<f32> = self.beat_seconds().map(|seconds| seconds as f32).collect();
        let downbeats: Vec<f32> = self
            .downbeat_raw_indices
            .iter()
            .map(|index| beats[*index])
            .collect();
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
}

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
    Ok(analyze_bpm_raw(audio, sample_rate_hz, config)?.legacy_result())
}

/// Capture complete qm-dsp evidence before the legacy binary32 conversion.
///
/// The ODF, tracker and downbeat detector run once using the existing pipeline.
/// Input sample zero is preserved, including silence and the final partial hop;
/// the result stores exact detector frames and derives seconds with the actual
/// integer sample hop. Short-input and invalid-timebase behavior matches
/// [`analyze_bpm`]. This capture neither fits a period nor publishes live timing.
pub fn analyze_bpm_raw(
    audio: &[f64],
    sample_rate_hz: u32,
    config: &AnalysisConfig,
) -> Result<QmRawAnalysis, String> {
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
        return Ok(QmRawAnalysis {
            beat_frames: Vec::new(),
            downbeat_raw_indices: Vec::new(),
            input_sample_rate_hz: sample_rate_hz,
            input_frame_count: audio.len(),
            odf_hop_samples: increment_samples,
            configuration: config.clone(),
        });
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

    Ok(raw_analysis_from_frames(
        audio,
        sample_rate_hz,
        increment_samples,
        config,
        beats_frames,
    ))
}

fn raw_analysis_from_frames(
    audio: &[f64],
    sample_rate_hz: u32,
    increment_samples: usize,
    config: &AnalysisConfig,
    beat_frames: Vec<f64>,
) -> QmRawAnalysis {
    let mut downbeat_indices = Vec::new();
    if !beat_frames.is_empty() {
        let mut downbeat = DownBeat::new(sample_rate_hz as f64, 16, increment_samples);
        downbeat.find_downbeats(audio, audio.len(), &beat_frames, &mut downbeat_indices);
    }

    QmRawAnalysis {
        beat_frames,
        downbeat_raw_indices: downbeat_indices,
        input_sample_rate_hz: sample_rate_hz,
        input_frame_count: audio.len(),
        odf_hop_samples: increment_samples,
        configuration: config.clone(),
    }
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
                let raw = raw_analysis_from_frames(
                    &audio,
                    sample_rate_hz,
                    increment_samples,
                    &config,
                    beats_frames.clone(),
                );
                let (bpm, grid) = raw.legacy_result();
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
                assert_eq!(raw.beat_frames(), beats_frames);
                assert_eq!(
                    raw.downbeat_raw_indices(),
                    (1..beats_frames.len()).step_by(4).collect::<Vec<_>>()
                );
                let raw_seconds: Vec<f64> = raw.beat_seconds().collect();
                let raw_downbeats: Vec<f64> = raw.downbeat_seconds().collect();
                assert_eq!(
                    raw_seconds,
                    beats_frames
                        .iter()
                        .map(|frame| frame * frame_duration_secs)
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    raw_downbeats,
                    (1..beats_frames.len())
                        .step_by(4)
                        .map(|index| raw_seconds[index])
                        .collect::<Vec<_>>()
                );
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
                let audio = vec![0.0; length];
                let raw = analyze_bpm_raw(&audio, sample_rate_hz, &config).unwrap();
                assert!(raw.beat_frames().is_empty());
                assert!(raw.downbeat_raw_indices().is_empty());
                assert_eq!(raw.beat_seconds().len(), 0);
                assert_eq!(raw.downbeat_seconds().len(), 0);
                assert_eq!(raw.input_sample_rate_hz(), sample_rate_hz);
                assert_eq!(raw.input_frame_count(), length);
                assert_eq!(
                    raw.odf_hop_samples(),
                    (sample_rate_hz as f64 * config.step_secs) as usize
                );
                assert_eq!(raw.configuration(), &config);
                let (bpm, grid) = raw.legacy_result();
                assert_eq!(bpm, 0.0);
                assert!(grid.beats.is_empty());
                assert!(grid.downbeats.is_empty());
                assert!(grid.bars.is_empty());
                let (legacy_bpm, legacy_grid) =
                    analyze_bpm(&audio, sample_rate_hz, &config).unwrap();
                assert_eq!(legacy_bpm, bpm);
                assert_eq!(legacy_grid.beats, grid.beats);
                assert_eq!(legacy_grid.downbeats, grid.downbeats);
                assert_eq!(legacy_grid.bars, grid.bars);
            }
        }
    }

    #[test]
    fn long_raw_projection_retains_binary64_precision_and_legacy_rounding() {
        // Trusted sparse detector coordinates exercise only capture projection;
        // this is not an analyzer/PCM/count/constant-period acceptance fixture.
        for sample_rate_hz in [44_100, 48_000, 96_000] {
            let config = AnalysisConfig::default();
            let hop = (sample_rate_hz as f64 * config.step_secs) as usize;
            let frames = vec![
                0.0,
                1.0,
                (599.5 * sample_rate_hz as f64 / hop as f64).round(),
                (1800.0 * sample_rate_hz as f64 / hop as f64).floor() - 2.0,
            ];
            let raw = QmRawAnalysis {
                beat_frames: frames.clone(),
                downbeat_raw_indices: vec![0, 2],
                input_sample_rate_hz: sample_rate_hz,
                input_frame_count: sample_rate_hz as usize * 1801,
                odf_hop_samples: hop,
                configuration: config,
            };
            let seconds: Vec<f64> = raw.beat_seconds().collect();
            let (bpm, legacy) = raw.legacy_result();
            let frame_duration = hop as f64 / sample_rate_hz as f64;
            let average_frames = (frames[3] - frames[0]) / 3.0;
            assert_eq!(bpm, (60.0 / (average_frames * frame_duration)) as f32);
            for (index, frame) in frames.iter().enumerate() {
                let expected = *frame * frame_duration;
                let sample_oracle = (*frame * hop as f64) / sample_rate_hz as f64;
                assert_eq!(seconds[index].to_bits(), expected.to_bits());
                assert!((seconds[index] - sample_oracle).abs() * (sample_rate_hz as f64) < 1e-6);
                assert_eq!(legacy.beats[index].to_bits(), (expected as f32).to_bits());
            }
            assert!(seconds.iter().zip(&legacy.beats).any(|(precise, rounded)| {
                (*precise - f64::from(*rounded)).abs() * sample_rate_hz as f64 > 0.5
            }));
            assert_eq!(
                raw.downbeat_seconds().collect::<Vec<_>>(),
                vec![seconds[0], seconds[2]]
            );
            assert_eq!(legacy.downbeats, vec![legacy.beats[0], legacy.beats[2]]);
            assert_eq!(legacy.bars, legacy.downbeats);
            assert_eq!(raw.beat_frames(), frames);
        }
    }

    fn assert_timebase_rejected(sample_rate_hz: u32, config: &AnalysisConfig) {
        let raw_error = analyze_bpm_raw(&[], sample_rate_hz, config).unwrap_err();
        let legacy_error = analyze_bpm(&[], sample_rate_hz, config).unwrap_err();
        assert_eq!(raw_error, legacy_error);
    }

    #[test]
    fn pipeline_rejects_invalid_or_unbounded_timebase_parameters_before_allocation() {
        let config = AnalysisConfig::default();
        assert_timebase_rejected(0, &config);
        for invalid in [0.0, -0.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let step_config = AnalysisConfig {
                step_secs: invalid,
                ..config.clone()
            };
            assert_timebase_rejected(44_100, &step_config);
            let window_config = AnalysisConfig {
                max_bin_hz: invalid,
                ..config.clone()
            };
            assert_timebase_rejected(44_100, &window_config);
        }
        for step_secs in [0.5 / 44_100.0, f64::MIN_POSITIVE, 1e10, f64::MAX] {
            let step_config = AnalysisConfig {
                step_secs,
                ..config.clone()
            };
            assert_timebase_rejected(44_100, &step_config);
        }
        for max_bin_hz in [f64::MIN_POSITIVE, 1e-100, 1e10, f64::MAX] {
            let window_config = AnalysisConfig {
                max_bin_hz,
                ..config.clone()
            };
            assert_timebase_rejected(44_100, &window_config);
        }
        // The ODF dimensions alone would be safe, but the downbeat FFT would not.
        let extreme_rate = u32::MAX;
        let config = AnalysisConfig {
            step_secs: 512.0 / extreme_rate as f64,
            max_bin_hz: extreme_rate as f64 / 1024.0,
            ..config
        };
        assert_timebase_rejected(extreme_rate, &config);
    }
}
