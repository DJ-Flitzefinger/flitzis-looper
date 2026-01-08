use crate::{
    audio_engine::channels::map_channels,
    messages::{SampleAnalysis, SampleBuffer},
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

    println!("Analysis grid_stability={}", result.grid_stability);

    Ok(SampleAnalysis {
        bpm: result.bpm,
        key: result.key.name(),
        beat_grid: result.beat_grid,
    })
}
