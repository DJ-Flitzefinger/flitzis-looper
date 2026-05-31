//! qm-dsp BPM/beat-grid detection pipeline.
#![allow(dead_code, unused_variables, unused_mut)]
//!
//! Ports the Queen Mary University tempo tracking algorithm (qm-dsp) to pure Rust.
//! Pipeline: DetectionFunction → TempoTrackV2 → DownBeat → { bpm, beats, downbeats, bars }

pub(crate) mod detection_function;
pub(crate) mod downbeat;
pub(crate) mod math_utils;
pub(crate) mod phase_vocoder;
pub(crate) mod tempotrack;
pub(crate) mod window;

use crate::audio_engine::channels::map_channels;
use crate::messages::{BeatGrid, SampleAnalysis, SampleBuffer};

use self::detection_function::DetectionFunction;
use self::downbeat::DownBeat;
use self::tempotrack::TempoTrackV2;

/// Configuration for the analysis pipeline with Mixxx-matching defaults.
#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    /// Frame step in seconds (≈86 Hz frame rate). Default: 0.01161.
    pub step_secs: f64,
    /// Maximum bin frequency in Hz for frame size calculation. Default: 50.0.
    pub max_bin_hz: f64,
    /// Input tempo for Rayleigh weighting (BPM). Default: 120.0.
    pub input_tempo: f64,
    /// Beat tracking alpha blend. Default: 0.9.
    pub alpha: f64,
    /// Beat tracking Gaussian tightness. Default: 4.0.
    pub tightness: f64,
    /// Viterbi transition smoothness (sigma). Default: 8.0.
    pub viterbi_sigma: f64,
    /// Beat tracking window length in frames. Default: 512.
    pub window_length: usize,
    /// Beat tracking hop size in frames. Default: 128.
    pub hop_size: usize,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            step_secs: 0.01161,
            max_bin_hz: 50.0,
            input_tempo: 120.0,
            alpha: 0.9,
            tightness: 4.0,
            viterbi_sigma: 8.0,
            window_length: 512,
            hop_size: 128,
        }
    }
}

/// Return the next higher integer power of two from x (or x if already a power of two).
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

/// Analyze a sample buffer for BPM, beat grid, and key.
///
/// Runs the full qm-dsp pipeline: DetectionFunction → TempoTrackV2 → DownBeat.
pub fn analyze_sample(
    sample: &SampleBuffer,
    sample_rate_hz: u32,
) -> Result<SampleAnalysis, String> {
    let mono = map_channels(sample.samples.to_vec(), sample.channels, 1)
        .map_err(|err| format!("analysis failed: {err}"))?;

    if mono.is_empty() || sample_rate_hz == 0 {
        return Err("analysis failed: empty sample or zero sample rate".to_string());
    }

    let config = AnalysisConfig::default();

    // Convert to f64 for internal processing
    let mono_f64: Vec<f64> = mono.iter().map(|s| *s as f64).collect();

    // Step 1: Compute onset detection function
    let mut df = DetectionFunction::new(sample_rate_hz, &config);

    // Guard: need at least one full frame of audio
    let frame_length = next_power_of_two((sample_rate_hz as f64 / config.max_bin_hz) as usize);
    if mono_f64.len() < frame_length {
        // Return default result for very short audio
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
    let odf = df.process(&mono_f64);

    if odf.is_empty() {
        return Err("analysis failed: no ODF values produced".to_string());
    }

    // Step 2: Estimate beat periods via Viterbi HMM
    let mut beat_period = Vec::new();
    let tracker = TempoTrackV2::new(sample_rate_hz as f64, config.step_secs);
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
    let bpm = calculate_bpm(&beats_frames, config.step_secs);

    // Step 5: Downbeat detection
    let mut downbeat_indices = Vec::new();
    let mut bar_indices = Vec::new();
    if !beats_frames.is_empty() {
        let mut downbeat = DownBeat::new(sample_rate_hz as f64, 16, config.step_secs as usize);
        downbeat.find_downbeats(
            &mono_f64,
            mono_f64.len(),
            &beats_frames,
            &mut downbeat_indices,
        );

        // Group downbeats into bars
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

    // Key detection is not yet ported; return placeholder
    Ok(SampleAnalysis {
        bpm,
        key: "unknown".to_string(),
        beat_grid,
    })
}

/// Calculate BPM from beat positions (in frames).
fn calculate_bpm(beats_frames: &[f64], step_secs: f64) -> f32 {
    if beats_frames.len() < 2 {
        return 0.0;
    }

    let mut intervals = Vec::with_capacity(beats_frames.len() - 1);
    for i in 1..beats_frames.len() {
        intervals.push(beats_frames[i] - beats_frames[i - 1]);
    }

    if intervals.is_empty() {
        return 0.0;
    }

    let avg_interval_frames = intervals.iter().sum::<f64>() / intervals.len() as f64;
    let avg_interval_secs = avg_interval_frames * step_secs;

    if avg_interval_secs <= 0.0 {
        return 0.0;
    }

    (60.0 / avg_interval_secs) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sample(samples: Vec<f32>, channels: usize) -> SampleBuffer {
        SampleBuffer {
            channels,
            samples: std::sync::Arc::from(samples),
        }
    }

    #[test]
    fn analyze_sample_silent_audio() {
        let sample = make_sample(vec![0.0; 44100 * 5], 1); // 5 seconds of silence
        let result = analyze_sample(&sample, 44100);
        // Should succeed (BPM may be 0 or some default)
        assert!(result.is_ok());
    }

    #[test]
    fn analyze_sample_single_sample() {
        let sample = make_sample(vec![0.5], 1);
        let result = analyze_sample(&sample, 44100);
        // Should handle gracefully (may return error or empty beat grid)
        // The key thing is it doesn't panic
        match result {
            Ok(analysis) => {
                // BPM might be 0 for a single sample
                assert!(analysis.bpm >= 0.0);
            }
            Err(_) => {
                // Error is also acceptable for extremely short input
            }
        }
    }

    #[test]
    fn analyze_sample_stereo_downmix() {
        // Create stereo audio with clicks
        let total_samples = 44100 * 10;
        let mut samples = vec![0.0f32; total_samples];
        for beat in 0..18 {
            let pos = (beat as f64 * 0.5 * 44100.0) as usize;
            if pos < total_samples / 2 {
                samples[pos * 2] = 0.8; // left
                samples[pos * 2 + 1] = 0.8; // right
            }
        }
        let sample = make_sample(samples, 2);
        let result = analyze_sample(&sample, 44100);
        assert!(result.is_ok());
    }

    #[test]
    fn analyze_sample_zero_sample_rate() {
        let sample = make_sample(vec![0.5; 1000], 1);
        let result = analyze_sample(&sample, 0);
        assert!(result.is_err());
    }

    #[test]
    fn analyze_sample_empty() {
        let sample = make_sample(vec![], 1);
        let result = analyze_sample(&sample, 44100);
        assert!(result.is_err());
    }

    #[test]
    fn calculate_bpm_empty() {
        assert_eq!(calculate_bpm(&[], 0.01161), 0.0);
    }

    #[test]
    fn calculate_bpm_single_beat() {
        assert_eq!(calculate_bpm(&[10.0], 0.01161), 0.0);
    }

    #[test]
    fn calculate_bpm_known_tempo() {
        // 120 BPM = 0.5s per beat, at step_secs=0.01161, period ≈ 43 frames
        let beats = (0..20).map(|i| i as f64 * 43.0).collect::<Vec<_>>();
        let bpm = calculate_bpm(&beats, 0.01161);
        assert!((bpm - 120.0).abs() < 1.0, "expected ~120 BPM, got {}", bpm);
    }
}
