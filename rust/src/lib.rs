use pyo3::pymodule;

mod audio_engine;
mod messages;

/// The Python module implemented in Rust.
#[pymodule]
mod flitzis_looper_audio {
    #[pymodule_export]
    use super::audio_engine::AudioEngine;

    #[pymodule_export]
    use super::messages::AudioMessage;
}

/// Results from running the full BPM analysis pipeline on an audio file.
///
/// Exposed as `pub` under `#[cfg(feature = "test-helpers")]` for integration tests.
#[cfg(feature = "test-helpers")]
#[derive(Debug)]
pub struct AnalysisResult {
    /// Estimated BPM.
    pub bpm: f32,
    /// Number of beats detected.
    pub beat_count: usize,
    /// Beat positions in seconds.
    pub beat_positions: Vec<f32>,
    /// Number of ODF frames produced.
    pub odf_frame_count: usize,
    /// Average estimated beat period in ODF frames.
    pub avg_beat_period_frames: f64,
    /// Number of downbeats detected.
    pub downbeat_count: usize,
}

#[cfg(feature = "test-helpers")]
pub use messages::SampleBuffer;

#[cfg(feature = "test-helpers")]
/// Decode an audio file and run the full BPM analysis pipeline.
///
/// Returns an [`AnalysisResult`] with all pipeline stage outputs.
pub fn analyze_audio_file(path: &std::path::Path) -> std::io::Result<AnalysisResult> {
    use std::io::{Error, ErrorKind};

    // Decode audio file to mono at 44100 Hz
    let sample = audio_engine::sample_loader::decode_audio_file_to_sample_buffer(
        path,
        1, // mono
        44_100,
        |_| {},
    )
    .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;

    let sample_rate = 44_100u32;

    // Run detection function to get ODF
    let mono_f64: Vec<f64> = sample.samples.iter().map(|s| *s as f64).collect();

    let config = audio_engine::analysis::AnalysisConfig::default();

    let mut df = audio_engine::analysis::DetectionFunction::new(sample_rate, &config);
    let odf = df.process(&mono_f64);
    let odf_frame_count = odf.len();

    // Run Viterbi beat period estimation
    let mut beat_period = Vec::new();
    let tracker =
        audio_engine::analysis::TempoTrackV2::new(sample_rate as f64, config.step_secs);
    tracker.calculate_beat_period(&odf, &mut beat_period, config.input_tempo, false);

    let avg_beat_period_frames = if beat_period.is_empty() {
        0.0
    } else {
        beat_period.iter().sum::<usize>() as f64 / beat_period.len() as f64
    };

    // Run beat tracking
    let mut beats_frames = Vec::new();
    tracker.calculate_beats(
        &odf,
        &beat_period,
        &mut beats_frames,
        config.alpha,
        config.tightness,
    );

    // Calculate BPM
    let bpm = audio_engine::analysis::calculate_bpm(&beats_frames, config.step_secs);

    // Downbeat detection
    let mut downbeat_indices = Vec::new();
    if !beats_frames.is_empty() {
        let mut downbeat =
            audio_engine::analysis::DownBeat::new(sample_rate as f64, 16, config.step_secs as usize);
        downbeat.find_downbeats(
            &mono_f64,
            mono_f64.len(),
            &beats_frames,
            &mut downbeat_indices,
        );
    }

    // Convert beat positions from frames to seconds
    let frame_duration = config.step_secs;
    let beat_positions: Vec<f32> = beats_frames
        .iter()
        .map(|f| (*f * frame_duration) as f32)
        .collect();

    Ok(AnalysisResult {
        bpm,
        beat_count: beats_frames.len(),
        beat_positions,
        odf_frame_count,
        avg_beat_period_frames,
        downbeat_count: downbeat_indices.len(),
    })
}

