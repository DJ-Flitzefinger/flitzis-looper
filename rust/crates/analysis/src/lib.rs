//! Audio analysis: BPM/beat-grid detection and musical key detection.
//!
//! **BPM pipeline** (qm-dsp): DetectionFunction → TempoTrackV2 → DownBeat
//! → { bpm, beats, downbeats, bars }.
//!
//! **Key detection** (KeyNet CNN): mono audio → CQT spectrogram → ONNX inference
//! → Camelot key string.

mod bpm_pipeline;
mod detection_function;
mod downbeat;
pub mod key_detection;
mod phase_vocoder;
pub mod tempo_evidence;
pub mod tempo_refinement;
pub mod tempo_summary;
mod tempotrack;

// Re-export public API surface
pub use bpm_pipeline::{QmRawAnalysis, analyze_bpm, analyze_bpm_raw};
pub use detection_function::DetectionFunction;
pub use downbeat::DownBeat;
pub use key_detection::{KeyError, KeyResult, camelot_index_to_key, detect_key};
pub use tempotrack::TempoTrackV2;

pub mod math_utils;
pub mod window;

/// Configuration for the analysis pipeline with Mixxx-matching defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisConfig {
    /// Requested frame step in seconds (≈86 Hz). Truncated to an integer sample hop.
    /// Default: 0.01161.
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

/// Beat grid with beat positions, downbeat positions, and bar start positions.
#[derive(Debug, Clone)]
pub struct BeatGrid {
    /// Beat positions in seconds.
    pub beats: Vec<f32>,
    /// Downbeat positions in seconds (first beat of each bar).
    pub downbeats: Vec<f32>,
    /// Bar start positions in seconds.
    pub bars: Vec<f32>,
}

/// Result of analyzing a sample buffer.
#[derive(Debug, Clone)]
pub struct SampleAnalysis {
    pub bpm: f32,
    pub key: String,
    pub beat_grid: BeatGrid,
}

/// Calculate BPM from beat positions (in frames).
pub fn calculate_bpm(beats_frames: &[f64], step_secs: f64) -> f32 {
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
