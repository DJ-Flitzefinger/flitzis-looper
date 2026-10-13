//! Executed complete legacy analysis preparation and allocation admission.

use super::{PcmError, fft, resample_mono_cancellable, validate_rate};
use crate::messages::SampleBuffer;
use flitzis_looper_analysis as analysis;

/// Both pipelines borrow this one signal. Preserve legacy f32 channel arithmetic
/// before widening, including frame zero and the complete resampled tail.
pub(in crate::audio_engine) fn prepare_default_mono_f64(
    sample: &SampleBuffer,
    rate_hz: u32,
) -> Result<Vec<f64>, PcmError> {
    let channels = sample.channels;
    if !matches!(channels, 1 | 2)
        || sample.samples.is_empty()
        || !sample.samples.len().is_multiple_of(channels)
    {
        return Err(PcmError::InvalidInput("default analysis source geometry"));
    }
    let frames = sample.samples.len() / channels;
    default_analysis_peak(
        frames,
        channels,
        rate_hz,
        0,
        super::super::constant_timing::MAX_PCM_BYTES,
    )?;
    let mean = |frame: &[f32]| {
        if channels == 1 {
            frame[0]
        } else {
            (frame[0] + frame[1]) * 0.5
        }
    };
    if rate_hz == 44_100 {
        return Ok(sample
            .samples
            .chunks_exact(channels)
            .map(|frame| f64::from(mean(frame)))
            .collect());
    }
    let mono: Vec<f32> = sample.samples.chunks_exact(channels).map(mean).collect();
    let converted = resample_mono_cancellable(
        mono,
        rate_hz,
        44_100,
        super::super::constant_timing::MAX_PCM_BYTES,
        &|| false,
    )?;
    // The f32 conversion allocation retires on return, before either pipeline
    // starts. It is still counted alongside the widening allocation in admission.
    Ok(converted.iter().map(|value| f64::from(*value)).collect())
}

/// Sum all simultaneous storage within each proven separate lifetime stage.
/// The source and finite retained view remain live in every stage. Model/ORT
/// allocations are outside this bound and reported separately as process peaks.
pub(in crate::audio_engine) fn default_analysis_peak(
    frames: usize,
    channels: usize,
    rate_hz: u32,
    extra_held_bytes: usize,
    maximum: usize,
) -> Result<usize, PcmError> {
    validate_rate(rate_hz)?;
    if frames == 0 || !matches!(channels, 1 | 2) {
        return Err(PcmError::InvalidInput("default analysis source geometry"));
    }
    let converted = (frames as u128 * 44_100).div_ceil(u128::from(rate_hz));
    let converted_usize = usize::try_from(converted)
        .map_err(|_| PcmError::Limit("default analysis frame count overflow"))?;
    let base = frames as u128 * channels as u128 * 4 + extra_held_bytes as u128;
    let prepare = if rate_hz == 44_100 {
        converted * 8
    } else {
        let (input, output) = fft::fft_dimensions(rate_hz, 44_100);
        // Rubato input/output/scratch/plan allowance from pinned FFT dimensions;
        // delayed output retains capacity after its in-place trim.
        let fft_workspace = (input as u128 + output as u128) * 64 * 4;
        let output_capacity = (converted + output as u128 * 2) * 4;
        let resample_stage = frames as u128 * 4 + output_capacity + fft_workspace;
        let widening_stage = output_capacity + converted * 8;
        resample_stage.max(widening_stage)
    };
    let cqt = analysis::key_detection::librosa_cqt::cqt::default_cqt_working_bytes(converted_usize)
        .ok_or(PcmError::Limit("default CQT workspace overflow"))?;
    let bpm = analysis::default_bpm_working_bytes(converted_usize)
        .ok_or(PcmError::Limit("default BPM workspace overflow"))?;
    // Concurrent means both independent workspaces are charged together.
    let inference = converted * 8 + cqt as u128 + bpm as u128;
    let peak = base + prepare.max(inference);
    if peak > maximum as u128 {
        return Err(PcmError::Limit(
            "complete automatic analysis transient bytes",
        ));
    }
    usize::try_from(peak).map_err(|_| PcmError::Limit("default analysis reservation overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_default_mono_matches_complete_legacy_channel_and_rate_oracle() {
        for channels in [1, 2] {
            for rate in [44_100, 48_000, 96_000] {
                let samples: Vec<f32> = (0..rate as usize * 8 + 37)
                    .flat_map(|frame| {
                        (0..channels).map(move |channel| {
                            if frame < 51 {
                                0.0
                            } else {
                                ((frame * 17 + channel * 31) as f32 * 0.001).sin() * 0.7
                            }
                        })
                    })
                    .collect();
                let mono =
                    crate::audio_engine::channels::map_channels(samples.clone(), channels, 1)
                        .unwrap();
                let oracle =
                    resample_mono_cancellable(mono, rate, 44_100, usize::MAX, &|| false).unwrap();
                let sample = SampleBuffer {
                    residency: None,
                    channels,
                    samples: samples.into(),
                };
                let owners = std::sync::Arc::strong_count(&sample.samples);
                let shared = prepare_default_mono_f64(&sample, rate).unwrap();
                assert_eq!(owners, std::sync::Arc::strong_count(&sample.samples));
                assert_eq!(shared.len(), oracle.len());
                assert!(
                    shared.iter().zip(&oracle).all(
                        |(actual, expected)| actual.to_bits() == f64::from(*expected).to_bits()
                    )
                );
                let actual =
                    analysis::analyze_bpm(&shared, 44_100, &analysis::AnalysisConfig::default())
                        .unwrap();
                let expected = analysis::analyze_bpm(
                    &oracle
                        .iter()
                        .map(|value| f64::from(*value))
                        .collect::<Vec<_>>(),
                    44_100,
                    &analysis::AnalysisConfig::default(),
                )
                .unwrap();
                assert_eq!(actual.0, expected.0);
                assert_eq!(actual.1.beats, expected.1.beats);
                assert_eq!(actual.1.downbeats, expected.1.downbeats);
                assert_eq!(actual.1.bars, expected.1.bars);
            }
        }
    }
}
