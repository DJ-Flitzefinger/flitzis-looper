//! Declared stream-coordinate proofs; these make no musical attack acceptance claim.

use super::*;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::prepared_native_history::tests::fixture;
use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
use crate::audio_engine::source_reader::ExplicitSeekMode;
use crate::audio_engine::stretch_processor::DEFAULT_BLOCK_SAMPLES;

/// Independent direct Native blocks, without the product adapter, FIFO or cut implementation.
fn raw_reference(
    source: &NativeHistoryRequest,
    native: &mut RubberBandLiveShifter,
    history: usize,
    raw_frames: usize,
) -> Vec<Vec<f32>> {
    let channels = source.plan.channels;
    let block = native.block_size();
    let (mut playback, mut silent) = source.playback.history_before_constant(history).unwrap();
    let mut plan = source.plan;
    let mut input = vec![vec![0.0; block]; channels];
    let mut output = vec![vec![0.0; block]; channels];
    let mut result = vec![Vec::new(); channels];
    for _ in 0..raw_frames.div_ceil(block) {
        let prefix = silent.min(block);
        let frames = block - prefix;
        if frames != 0 {
            let position = playback.position();
            plan.frame_pos = position.frame;
            plan.seek_mode = position.seek_mode;
            plan.fill_fractional_buffers(
                &source.sample,
                source.stems.as_ref(),
                &playback,
                &mut input,
                frames,
            );
            playback.advance(frames);
        }
        for channel in &mut input {
            channel.copy_within(0..frames, prefix);
            channel[..prefix].fill(0.0);
        }
        silent -= prefix;
        native.shift(&input, &mut output).unwrap();
        for (result, output) in result.iter_mut().zip(&output) {
            result.extend_from_slice(output);
        }
    }
    result
}

#[test]
fn declared_raw_suffix_keeps_logical_endpoint_and_matches_direct_native_under_unequal_partitions() {
    const CONTINUATION: usize = 4099;
    let mut cases = 0;
    for (rate, ratio, offset) in [(44_100, 0.73, 0), (48_000, 1.37, 0), (96_000, 0.5, 12)] {
        for mode in [
            ExplicitSeekMode::Normal,
            ExplicitSeekMode::BeforeLoop,
            ExplicitSeekMode::AfterLoop,
        ] {
            for mask in [None, Some(0b1010)] {
                let mut reference = fixture(2, rate, ratio, mode, mask);
                reference.pitch_intent = PitchIntent::new(0, offset).unwrap();
                let pitch = reference
                    .pitch_intent
                    .native_pitch_scale(ratio, true)
                    .unwrap();
                let mut raw = RubberBandLiveShifter::new(rate, 2).unwrap();
                raw.prepare_exact_pitch(pitch).unwrap();
                let history = raw.source_history_frames(pitch).unwrap();
                // Fixed arbitrary raw coordinate declared before rendering, not a fitted C.
                let discard = history + 137;
                let expected = raw_reference(&reference, &mut raw, history, discard + CONTINUATION);
                for pattern in [&[512][..], &[1, 127, 384, 96, 257, 512, 31][..]] {
                    let mut request = fixture(2, rate, ratio, mode, mask);
                    request.pitch_intent = PitchIntent::new(0, offset).unwrap();
                    let mut logical = request.playback;
                    let mut logical_plan = request.plan;
                    let mut state = NativeAdapterState::new(
                        Some(RubberBandLiveShifter::new(rate, 2).unwrap()),
                        2,
                    );
                    let address = state.rubberband.as_ref().unwrap().state_address();
                    state
                        .prepare_retained_source_start_checked(
                            request,
                            Some(RetainedNativeStartCut {
                                history_output_frames: history,
                                discarded_raw_output_frames: discard,
                            }),
                        )
                        .unwrap();
                    assert!(
                        state
                            .source
                            .as_ref()
                            .unwrap()
                            .playback
                            .matches_exact(&logical)
                    );
                    assert!(
                        state
                            .source
                            .as_ref()
                            .unwrap()
                            .plan
                            .matches_exact(logical_plan)
                    );
                    assert_eq!(state.rubberband.as_ref().unwrap().state_address(), address);
                    let (initial_feed, _) = state
                        .retained_start_feed(&logical, logical_plan)
                        .unwrap()
                        .unwrap();
                    assert_ne!(initial_feed.position(), logical.position());
                    assert!(
                        expected[0][discard..discard + 32]
                            .iter()
                            .any(|value| *value != 0.0)
                    );
                    let mut feed = vec![vec![0.0; DEFAULT_BLOCK_SAMPLES]; 2];
                    let mut output = vec![vec![0.0; DEFAULT_BLOCK_SAMPLES]; 2];
                    let mut elapsed = 0;
                    let mut step = 0;
                    while elapsed < CONTINUATION {
                        let frames = pattern[step % pattern.len()].min(CONTINUATION - elapsed);
                        let (actual, plan) = state
                            .retained_start_feed(&logical, logical_plan)
                            .unwrap()
                            .unwrap();
                        let source = state.source.as_ref().unwrap();
                        plan.fill_fractional_buffers(
                            &source.sample,
                            source.stems.as_ref(),
                            &actual,
                            &mut feed,
                            frames,
                        );
                        state.render(&feed, &mut output, frames, pitch).unwrap();
                        for channel in 0..2 {
                            assert_eq!(
                                &output[channel][..frames],
                                &expected[channel][discard + elapsed..discard + elapsed + frames],
                                "rate={rate} ratio={ratio} mode={mode:?} mask={mask:?} pattern={pattern:?}"
                            );
                        }
                        state.advance_retained_start_feed(frames);
                        logical.advance(frames);
                        let position = logical.position();
                        logical_plan.frame_pos = position.frame;
                        logical_plan.seek_mode = position.seek_mode;
                        assert_eq!(state.input_fifo[0].len() + state.output_fifo[0].len(), 511);
                        elapsed += frames;
                        step += 1;
                    }
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 36);
}

#[test]
fn retained_future_feed_cannot_follow_changed_logical_position_or_control_trajectory() {
    let request = fixture(2, 48_000, 1.37, ExplicitSeekMode::Normal, None);
    let logical = request.playback;
    let plan = request.plan;
    let mut state =
        NativeAdapterState::new(Some(RubberBandLiveShifter::new(48_000, 2).unwrap()), 2);
    state
        .prepare_retained_source_start_checked(
            request,
            Some(RetainedNativeStartCut {
                history_output_frames: 4096,
                discarded_raw_output_frames: 4096 + 137,
            }),
        )
        .unwrap();
    let mut changed = logical;
    changed.advance(1);
    assert!(state.retained_start_feed(&changed, plan).is_err());
    let mut changed = logical;
    changed.set_target(1.5);
    assert!(state.retained_start_feed(&changed, plan).is_err());
    let mut changed_plan = plan;
    changed_plan.loop_region.end -= 1;
    assert!(state.retained_start_feed(&logical, changed_plan).is_err());
    assert!(state.retained_start_feed(&logical, plan).unwrap().is_some());
    state.reset_adapter();
    assert!(state.retained_start_feed(&logical, plan).unwrap().is_none());
}

#[test]
fn raw_discard_bounds_and_unavailable_history_are_refused_without_clipping() {
    for cut in [
        RetainedNativeStartCut {
            history_output_frames: 4096,
            discarded_raw_output_frames: usize::MAX,
        },
        RetainedNativeStartCut {
            history_output_frames: SOURCE_FEED_HISTORY_MAX_FRAMES + 1,
            discarded_raw_output_frames: SOURCE_FEED_HISTORY_MAX_FRAMES,
        },
        RetainedNativeStartCut {
            history_output_frames: 4095,
            discarded_raw_output_frames: 4096,
        },
        RetainedNativeStartCut {
            history_output_frames: 4096,
            discarded_raw_output_frames: 0,
        },
    ] {
        let mut state =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(48_000, 2).unwrap()), 2);
        let request = fixture(2, 48_000, 1.37, ExplicitSeekMode::Normal, None);
        assert!(matches!(
            state.prepare_retained_source_start_checked(request, Some(cut)),
            Err(NativeSourcePreparationError::Unavailable(
                RubberBandError::UnavailableSourceHistory
            ))
        ));
        assert!(state.source.is_none());
    }
}
