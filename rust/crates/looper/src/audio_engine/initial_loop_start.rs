//! Initial physical loop-start suggestion from immutable loaded PCM, off-thread.
//!
//! This detects amplitude activity, not a beat/downbeat or musical grid origin.
//! The candidate is the loaded frame immediately before the first finite sample
//! outside a fixed symmetric +/-0.01 full-scale tolerance, clamped at source
//! frame zero. This testable heuristic can skip low-level content and the start
//! of fades; tracks entirely inside the band have no suggestion. Isolated noise
//! above the band can still produce an unsuitable musical start. Nonfinite values
//! are ignored, so the candidate does not certify silence in malformed PCM. No
//! waveform averaging, allocation, PCM edits or analysis job is needed.

use crate::messages::SampleBuffer;

const NEAR_ZERO_TOLERANCE: f32 = 0.01;

/// Return the loaded frame preceding the first across-channel tolerance crossing.
///
/// One borrowed pass inspects finite amplitudes without cancellation between
/// opposite-polarity channels. Both fixed tolerance boundaries count as near-zero;
/// activity requires a strictly greater absolute amplitude. Silence, audio entirely
/// inside the band and invalid shapes/rates have no suggestion; callers retain the
/// zero-start fallback for new assignment. This does not identify a beat or downbeat.
pub(super) fn detect_initial_loop_start(sample: &SampleBuffer, sample_rate_hz: u32) -> Option<f64> {
    if sample_rate_hz == 0
        || sample.channels == 0
        || sample.samples.is_empty()
        || !sample.samples.len().is_multiple_of(sample.channels)
    {
        return None;
    }

    let activity_frame = sample
        .samples
        .chunks_exact(sample.channels)
        .position(|frame| {
            frame
                .iter()
                .any(|value| value.is_finite() && value.abs() > NEAR_ZERO_TOLERANCE)
        })?;
    let start_frame = activity_frame.saturating_sub(1);
    Some(start_frame as f64 / f64::from(sample_rate_hz))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn sample(samples: Vec<f32>, channels: usize) -> SampleBuffer {
        SampleBuffer {
            residency: None,
            samples: Arc::from(samples),
            channels,
        }
    }

    #[test]
    fn silence_and_audio_within_absolute_tolerance_have_no_activity_suggestion() {
        for amplitude in [
            0.0,
            0.005,
            -0.005,
            NEAR_ZERO_TOLERANCE,
            -NEAR_ZERO_TOLERANCE,
        ] {
            assert_eq!(
                detect_initial_loop_start(&sample(vec![amplitude; 1000], 1), 48_000),
                None
            );
        }
    }

    #[test]
    fn last_near_zero_frame_is_used_at_every_loaded_sample_rate() {
        let mut audio = vec![0.0; 6000];
        audio[5000] = -0.5;
        let audio = sample(audio, 1);
        for rate in [1, 1000, 44_100, 48_000, 96_000, 192_000, u32::MAX] {
            let frame = 4999;
            let seconds = detect_initial_loop_start(&audio, rate).unwrap();
            assert_eq!(seconds, f64::from(frame) / f64::from(rate));
            assert_eq!((seconds * f64::from(rate)).round(), f64::from(frame));
        }
    }

    #[test]
    fn entirely_quiet_track_has_no_candidate_after_leading_silence() {
        let mut audio = vec![0.0; 800];
        audio[400..].fill(0.005);
        assert_eq!(detect_initial_loop_start(&sample(audio, 1), 48_000), None);
    }

    #[test]
    fn fixed_tolerance_ignores_signed_residue_before_either_polarity_attack() {
        for polarity in [1.0, -1.0] {
            let mut audio: Vec<f32> = (0..1000)
                .map(|frame| if frame % 2 == 0 { 0.005 } else { -0.005 })
                .collect();
            audio[900] = polarity * 0.0101;
            assert_eq!(
                detect_initial_loop_start(&sample(audio, 1), 48_000),
                Some(899.0 / 48_000.0)
            );
        }
    }

    #[test]
    fn later_peak_does_not_change_the_fixed_tolerance_boundary() {
        for later_peak in [0.1, 0.5, 1.0] {
            let audio = sample(vec![0.0, 0.005, -0.005, 0.0101, later_peak], 1);
            assert_eq!(
                detect_initial_loop_start(&audio, 48_000),
                Some(2.0 / 48_000.0)
            );
        }
    }

    #[test]
    fn fade_uses_the_last_frame_inside_tolerance_without_fixed_time_pre_roll() {
        let mut audio = vec![0.0; 1500];
        for step in 0..=1024 {
            audio[400 + step] = step as f32 / 1024.0;
        }
        // Fade frame 410 is 10/1024 and remains inside the fixed .01 band;
        // frame 411 exceeds it, so the quietest fade content precedes the marker.
        assert_eq!(
            detect_initial_loop_start(&sample(audio, 1), 1000),
            Some(410.0 / 1000.0)
        );
    }

    #[test]
    fn opposite_polarity_and_single_channel_stereo_activity_are_retained() {
        for frame in [[0.5, -0.5], [0.0, -0.5], [0.5, 0.0]] {
            let mut audio = vec![0.0; 1600];
            audio[1000..1002].copy_from_slice(&frame);
            assert_eq!(
                detect_initial_loop_start(&sample(audio, 2), 48_000),
                Some(499.0 / 48_000.0)
            );
        }
    }

    #[test]
    fn isolated_last_frame_activity_is_kept_and_first_frame_activity_clamps_to_zero() {
        let mut audio = vec![0.0; 17];
        audio[16] = 0.5;
        let audio = sample(audio, 1);
        assert_eq!(detect_initial_loop_start(&audio, 1000), Some(0.015));
        assert_eq!(
            detect_initial_loop_start(&audio, 48_000),
            Some(15.0 / 48_000.0)
        );
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
        // One quiet frame before activity selects source frame zero at any rate.
        assert_eq!(
            detect_initial_loop_start(&sample(vec![0.0, 1.0], 1), u32::MAX),
            Some(0.0)
        );
    }

    #[test]
    fn nonfinite_values_do_not_define_first_activity() {
        let invalid = vec![f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        assert_eq!(detect_initial_loop_start(&sample(invalid, 1), 1000), None);
        let mut audio = vec![0.0; 20];
        audio[2] = f32::NAN;
        audio[3] = f32::INFINITY;
        audio[12] = -0.5;
        assert_eq!(
            detect_initial_loop_start(&sample(audio, 1), 1000),
            Some(0.011)
        );
    }

    #[test]
    fn both_fixed_tolerance_boundaries_are_inside_and_the_next_float_is_outside() {
        for polarity in [1.0, -1.0] {
            let just_outside = f32::from_bits(NEAR_ZERO_TOLERANCE.to_bits() + 1);
            let audio = sample(
                vec![
                    0.0,
                    NEAR_ZERO_TOLERANCE,
                    -NEAR_ZERO_TOLERANCE,
                    polarity * just_outside,
                    0.5,
                ],
                1,
            );
            assert_eq!(
                detect_initial_loop_start(&audio, 48_000),
                Some(2.0 / 48_000.0)
            );
        }
    }

    #[test]
    fn any_finite_channel_can_cross_tolerance_despite_nonfinite_other_channels() {
        let audio = sample(
            vec![
                f32::NAN,
                0.0,
                f32::INFINITY,
                -NEAR_ZERO_TOLERANCE,
                f32::NEG_INFINITY,
                2.0 * NEAR_ZERO_TOLERANCE,
            ],
            2,
        );
        assert_eq!(
            detect_initial_loop_start(&audio, 48_000),
            Some(1.0 / 48_000.0)
        );
    }
}
