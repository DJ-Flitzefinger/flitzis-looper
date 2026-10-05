//! Initial physical loop-start suggestion from immutable loaded PCM, off-thread.
//!
//! This detects amplitude activity, not a beat/downbeat or musical grid origin.
//! The low threshold and five-millisecond pre-roll preserve quiet attacks where
//! possible; earlier content below the threshold and isolated noise remain an
//! explicit heuristic limitation. No waveform averaging, allocation or analysis
//! job is needed, and no PCM or timing metadata is changed.

use crate::messages::SampleBuffer;

const MIN_ACTIVITY_AMPLITUDE: f32 = 1e-5;
const RELATIVE_ACTIVITY_AMPLITUDE: f32 = 0.001;
const PRE_ROLL_MILLIS: u64 = 5;

/// Return the earliest across-channel activity with conservative loaded-frame pre-roll.
///
/// Two borrowed passes inspect finite amplitudes without cancellation between
/// opposite-polarity channels. Silence, sub-floor audio and invalid shapes/rates
/// have no suggestion; callers retain the zero-start fallback for new assignment.
pub(super) fn detect_initial_loop_start(sample: &SampleBuffer, sample_rate_hz: u32) -> Option<f64> {
    if sample_rate_hz == 0
        || sample.channels == 0
        || sample.samples.is_empty()
        || !sample.samples.len().is_multiple_of(sample.channels)
    {
        return None;
    }

    let peak = sample
        .samples
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(f32::abs)
        .fold(0.0_f32, f32::max);
    if peak < MIN_ACTIVITY_AMPLITUDE {
        return None;
    }
    let threshold = MIN_ACTIVITY_AMPLITUDE.max(peak * RELATIVE_ACTIVITY_AMPLITUDE);
    let activity_frame = sample
        .samples
        .chunks_exact(sample.channels)
        .position(|frame| {
            frame
                .iter()
                .any(|value| value.is_finite() && value.abs() >= threshold)
        })?;
    let pre_roll_frames = (u64::from(sample_rate_hz) * PRE_ROLL_MILLIS / 1000) as usize;
    let start_frame = activity_frame.saturating_sub(pre_roll_frames);
    Some(start_frame as f64 / f64::from(sample_rate_hz))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn sample(samples: Vec<f32>, channels: usize) -> SampleBuffer {
        SampleBuffer {
            samples: Arc::from(samples),
            channels,
        }
    }

    #[test]
    fn silent_and_sub_floor_audio_have_no_activity_suggestion() {
        for amplitude in [0.0, 1e-6, -1e-6] {
            assert_eq!(
                detect_initial_loop_start(&sample(vec![amplitude; 1000], 1), 48_000),
                None
            );
        }
    }

    #[test]
    fn leading_blank_is_skipped_in_exact_loaded_frames_at_each_rate() {
        let mut audio = vec![0.0; 6000];
        audio[5000] = -0.5;
        let audio = sample(audio, 1);
        for (rate, pre_roll_frames) in [(44_100, 220), (48_000, 240), (96_000, 480), (192_000, 960)]
        {
            let frame = 5000 - pre_roll_frames;
            let seconds = detect_initial_loop_start(&audio, rate).unwrap();
            assert_eq!(seconds, f64::from(frame) / f64::from(rate));
            assert_eq!((seconds * f64::from(rate)).round(), f64::from(frame));
        }
    }

    #[test]
    fn quiet_audio_uses_absolute_floor_instead_of_a_fixed_loudness_gate() {
        let mut audio = vec![0.0; 800];
        audio[400..].fill(2e-5);
        assert_eq!(
            detect_initial_loop_start(&sample(audio, 1), 48_000),
            Some(160.0 / 48_000.0)
        );
    }

    #[test]
    fn relative_threshold_ignores_lower_level_noise_before_louder_activity() {
        let mut audio = vec![0.0; 1000];
        audio[300] = 2e-5;
        audio[900] = 0.5;
        assert_eq!(
            detect_initial_loop_start(&sample(audio, 1), 48_000),
            Some(660.0 / 48_000.0)
        );
    }

    #[test]
    fn low_threshold_and_pre_roll_retain_a_soft_fade_attack() {
        let mut audio = vec![0.0; 1500];
        for step in 0..=1024 {
            audio[400 + step] = step as f32 / 1024.0;
        }
        // Peak 1.0 gives threshold .001. The second fade frame crosses it;
        // five frames of pre-roll place the suggestion before the fade begins.
        assert_eq!(
            detect_initial_loop_start(&sample(audio, 1), 1000),
            Some(397.0 / 1000.0)
        );
    }

    #[test]
    fn opposite_polarity_and_single_channel_stereo_activity_are_retained() {
        for frame in [[0.5, -0.5], [0.0, -0.5], [0.5, 0.0]] {
            let mut audio = vec![0.0; 1600];
            audio[1000..1002].copy_from_slice(&frame);
            assert_eq!(
                detect_initial_loop_start(&sample(audio, 2), 48_000),
                Some(260.0 / 48_000.0)
            );
        }
    }

    #[test]
    fn isolated_last_frame_activity_is_kept_and_short_pre_roll_clamps_to_zero() {
        let mut audio = vec![0.0; 17];
        audio[16] = 0.5;
        let audio = sample(audio, 1);
        assert_eq!(detect_initial_loop_start(&audio, 1000), Some(0.011));
        assert_eq!(detect_initial_loop_start(&audio, 48_000), Some(0.0));
        assert_eq!(
            detect_initial_loop_start(&sample(vec![-0.5], 1), 48_000),
            Some(0.0)
        );
    }

    #[test]
    fn invalid_shape_and_zero_rate_have_no_suggestion() {
        for audio in [
            sample(Vec::new(), 1),
            sample(vec![1.0], 0),
            sample(vec![1.0; 3], 2),
        ] {
            assert_eq!(detect_initial_loop_start(&audio, 48_000), None);
        }
        assert_eq!(detect_initial_loop_start(&sample(vec![1.0], 1), 0), None);
        // The integer pre-roll calculation remains bounded even for unusual rates.
        assert_eq!(
            detect_initial_loop_start(&sample(vec![0.0, 1.0], 1), u32::MAX),
            Some(0.0)
        );
    }

    #[test]
    fn nonfinite_values_do_not_define_the_peak_or_first_activity() {
        let invalid = vec![f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        assert_eq!(detect_initial_loop_start(&sample(invalid, 1), 1000), None);
        let mut audio = vec![0.0; 20];
        audio[2] = f32::NAN;
        audio[3] = f32::INFINITY;
        audio[12] = -0.5;
        assert_eq!(
            detect_initial_loop_start(&sample(audio, 1), 1000),
            Some(0.007)
        );
    }
}
