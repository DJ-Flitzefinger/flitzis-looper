use crate::{
    audio_engine::channels::map_channels,
    messages::{BeatGridData, SampleAnalysis, SampleBuffer},
};
use stratum_dsp::{AnalysisConfig, analyze_audio};

/// Analyze audio using stratum-dsp.
pub fn analyze_sample(
    sample: &SampleBuffer,
    sample_rate_hz: u32,
) -> Result<SampleAnalysis, String> {
    let mono = map_channels(sample.samples.to_vec(), sample.channels, 1)
        .map_err(|err| format!("analysis failed: {err}"))?;

    let result = analyze_audio(&mono, sample_rate_hz, AnalysisConfig::default())
        .map_err(|err| format!("analysis failed: {err}"))?;

    Ok(SampleAnalysis {
        bpm: result.bpm,
        key: result.key.name(),
        beat_grid: BeatGridData {
            beats: result.beat_grid.beats,
            downbeats: result.beat_grid.downbeats,
        },
    })
}
