//! Actual reader/adapter receipts and the callback journal, without a device or timing anchor.

#[path = "retained_control_tests.rs"]
mod retained_control_tests;

use super::*;
use crate::audio_engine::native_history_permit::NativeHistoryContext;
use crate::audio_engine::pitch_runtime_tests::Fixture;
use crate::audio_engine::prepared_native_history::RetainedNativeStartCut;
use crate::audio_engine::source_feed_history::{SourceFeedHistory, SourceFeedSnapshot};
use crate::audio_engine::source_reader::{
    ExplicitSeekMode, FrameRange, StemRenderSelection, StemTransition,
    reset_tap_observation_for_test, tap_observation_for_test,
};
use crate::messages::ResidentContext;
use std::sync::{Arc, atomic::Ordering};

const REGION: FrameRange = FrameRange { start: 8, end: 32 };
const SOURCE_FRAMES: usize = 48;
const RATIO: f64 = 0.75;

fn source(channels: usize) -> SampleBuffer {
    SampleBuffer {
        residency: None,
        channels,
        samples: Arc::from(
            (0..SOURCE_FRAMES)
                .flat_map(|frame| {
                    (0..channels).map(move |channel| {
                        ((frame * frame + 7 * channel) % 53) as f32 / 64.0 - 0.4
                    })
                })
                .collect::<Vec<_>>(),
        ),
    }
}

fn playback(start: usize, mode: ExplicitSeekMode) -> SourcePlayback {
    let mut playback = SourcePlayback::new(start, mode, RATIO);
    playback.configure(SOURCE_FRAMES, REGION);
    // A real fractional epoch, rather than a receipt assembled from an integer playhead.
    playback.advance(1);
    playback
}

fn plan(sample: &SampleBuffer, playback: &SourcePlayback) -> SourceReadPlan {
    let position = playback.position();
    SourceReadPlan {
        channels: sample.channels,
        sample_frames: SOURCE_FRAMES,
        frame_pos: position.frame,
        loop_region: REGION,
        loop_period: None,
        seek_mode: position.seek_mode,
        selection: StemRenderSelection::full_mix(),
        transition: StemTransition::default(),
    }
}

fn feed<'a>(
    sample: &'a SampleBuffer,
    playback: &'a SourcePlayback,
    key_lock: bool,
) -> ProductiveSourceFeed<'a> {
    ProductiveSourceFeed {
        pitch_intent: PitchIntent::default(),
        key_lock,
        sample,
        stems: None,
        outgoing_stems: None,
        sample_rate_hz: 48_000,
        accepted: None,
        plan: plan(sample, playback),
        playback,
        permit: None,
        output_frame: None,
    }
}

// Independent integer addressing from the declared source epoch, without the production
// cursor, fractional-tap helper, receipt slicing or journal replay.
fn physical(raw: usize, mode: ExplicitSeekMode) -> usize {
    match mode {
        ExplicitSeekMode::BeforeLoop if raw < REGION.start => raw,
        ExplicitSeekMode::AfterLoop if raw < SOURCE_FRAMES => raw,
        ExplicitSeekMode::AfterLoop => REGION.start + (raw - SOURCE_FRAMES) % REGION.len(),
        _ => REGION.start + (raw - REGION.start) % REGION.len(),
    }
}

#[test]
fn actual_read_receipts_preserve_fractional_intro_normal_tail_under_partitions() {
    let sample = source(2);
    let mut processor = StretchProcessor::new(2);
    for (start, mode) in [
        (1, ExplicitSeekMode::BeforeLoop),
        (31, ExplicitSeekMode::Normal),
        (46, ExplicitSeekMode::AfterLoop),
    ] {
        let mut actual = playback(start, mode);
        let origin = actual;
        let mut history = SourceFeedHistory::new();
        let mut elapsed = 0;
        for frames in [17, 1, 23, 30] {
            let before = actual;
            let expected_plan = plan(&sample, &actual);
            let receipt = processor
                .process_source(feed(&sample, &actual, false), frames, false)
                .expect("complete dry read succeeds");
            assert_eq!(receipt.domain, SourceReadDomain::DryOutput);
            assert_eq!(receipt.output_frames, frames);
            assert!(receipt.playback.matches_exact(&before));
            assert!(receipt.plan.matches_exact(expected_plan));
            assert!(
                actual.matches_exact(&before),
                "reading cannot advance its owner"
            );
            assert!(processor.productive_history().is_none());
            for offset in 0..frames {
                let coordinate = start as f64 + (1 + elapsed + offset) as f64 * RATIO;
                let whole = coordinate.floor() as usize;
                let fraction = (coordinate - whole as f64) as f32;
                let left = physical(whole, mode);
                let right = physical(whole + 1, mode);
                for channel in 0..sample.channels {
                    let a = sample.samples[left * sample.channels + channel];
                    let b = sample.samples[right * sample.channels + channel];
                    let expected = a + (b - a) * fraction;
                    assert_eq!(processor.output_buffers()[channel][offset], expected);
                }
            }
            history.record_read(receipt);
            actual.advance(frames);
            elapsed += frames;
        }
        let mut snapshot = SourceFeedSnapshot::new();
        assert!(history.capture_into(&mut snapshot));
        assert_eq!(snapshot.retained_range(), Some(0..71));
        assert_eq!(snapshot.spans().len(), 1);
        let span = snapshot.spans().next().unwrap();
        assert!(span.playback.matches_exact(&origin));
        assert_eq!(span.playback.position_at(71), actual.position());
    }
}

#[test]
fn native_receipt_counts_accepted_fifo_prefix_before_first_shift_and_tracks_actual_feed() {
    let sample = source(1);
    let mut actual = playback(31, ExplicitSeekMode::Normal);
    let mut processor = StretchProcessor::new(1);
    // Isolate the ordinary live adapter; no asynchronous prepared owner is needed for this
    // receipt test. The Native handle is still the real warmed production backend.
    processor.prospective_request = Some(0);
    let logical = playback(13, ExplicitSeekMode::Normal);
    let logical_before = logical;
    let receipt = processor
        .process_source(feed(&sample, &actual, true), 1, true)
        .unwrap();
    assert_eq!(receipt.domain, SourceReadDomain::NativeInput);
    assert!(
        !processor.native.used,
        "one frame is a pending real FIFO input"
    );
    assert!(!processor.wet_render_ready());
    assert!(processor.wet_rendered);
    let history = processor.productive_history().unwrap();
    assert_eq!(history.fed_output_frames, 1);
    assert_eq!(history.next_position, actual.position_at(1));
    assert_ne!(history.next_position, logical.position_at(1));
    assert!(logical.matches_exact(&logical_before));
    let mut journal = SourceFeedHistory::new();
    journal.record_read(receipt);

    actual.advance(1);
    let frames = processor.rubberband_block_size();
    let receipt = processor
        .process_source(feed(&sample, &actual, true), frames, true)
        .unwrap();
    assert_eq!(receipt.domain, SourceReadDomain::NativeInput);
    assert!(processor.native.used);
    let history = processor.productive_history().unwrap();
    assert_eq!(history.fed_output_frames, (1 + frames) as u64);
    assert_eq!(history.next_position, receipt.playback.position_at(frames));
    journal.record_read(receipt);
    let mut snapshot = SourceFeedSnapshot::new();
    assert!(journal.capture_into(&mut snapshot));
    assert_eq!(snapshot.retained_range(), Some(0..(1 + frames) as u64));
    let span = snapshot.spans().next().unwrap();
    assert_eq!(
        span.playback.position(),
        playback(31, ExplicitSeekMode::Normal).position()
    );
    assert_ne!(span.playback.position(), logical.position());
}

#[test]
fn unavailable_native_pass_cannot_publish_a_successful_source_receipt() {
    let sample = source(1);
    let actual = playback(13, ExplicitSeekMode::Normal);
    let mut processor = StretchProcessor::new(1);
    processor.prospective_request = Some(0);
    processor.native.rubberband = None;
    reset_tap_observation_for_test();
    assert!(
        processor
            .process_source(feed(&sample, &actual, true), 17, true)
            .is_none()
    );
    assert!(
        tap_observation_for_test().left_reads > 0,
        "the real reader ran"
    );
    assert!(processor.wet_mode_failed());
    assert!(processor.productive_history().is_none());
    assert!(
        processor.output_buffers()[0][..17]
            .iter()
            .all(|sample| *sample == 0.0)
    );
}

#[test]
fn actual_adapter_fifo_error_cannot_publish_a_successful_source_receipt() {
    let sample = source(1);
    let actual = playback(13, ExplicitSeekMode::Normal);
    let mut processor = StretchProcessor::new(1);
    processor.prospective_request = Some(0);
    // Inject a fixed-capacity adapter fault. Native remains real; its failed render must
    // invalidate fixed history/FIFOs without reporting the completed source read as success.
    let capacity = processor.native.input_fifo[0].capacity();
    processor.native.input_fifo[0].push_silence(capacity);
    reset_tap_observation_for_test();
    assert!(
        processor
            .process_source(feed(&sample, &actual, true), 17, true)
            .is_none()
    );
    assert!(tap_observation_for_test().left_reads > 0);
    assert!(processor.wet_mode_failed());
    assert!(!processor.native.active);
    assert!(processor.productive_history().is_none());
    assert_eq!(processor.native.input_fifo[0].len(), 0);
    assert!(
        processor.output_buffers()[0][..17]
            .iter()
            .all(|sample| *sample == 0.0)
    );
}

#[test]
fn finite_missing_fractional_tap_returns_no_receipt_and_fences_journal_history() {
    let complete = source(1).with_complete_source(48_000);
    let sample = complete
        .window(8, 31, 2, ResidentContext::KeyLockFiniteLoop)
        .unwrap();
    let mut processor = StretchProcessor::new(1);
    let mut history = SourceFeedHistory::new();
    let valid = playback(8, ExplicitSeekMode::Normal);
    let receipt = processor
        .process_source(feed(&sample, &valid, false), 3, false)
        .unwrap();
    history.record_read(receipt);
    let mut before_gap = SourceFeedSnapshot::new();
    assert!(history.capture_into(&mut before_gap));
    let native_before = processor.native_state_address();
    let missing = playback(30, ExplicitSeekMode::Normal); // left30 exists, right31 does not.
    reset_tap_observation_for_test();
    let receipt = processor.process_source(feed(&sample, &missing, false), 1, false);
    assert!(receipt.is_none());
    assert!(tap_observation_for_test().missing_reads > 0);
    assert_eq!(processor.native_state_address(), native_before);
    assert!(processor.productive_history().is_none());
    assert_eq!(processor.output_buffers()[0][0], 0.0);
    history.record_gap(1);
    assert!(!before_gap.is_current());
    let mut after_gap = SourceFeedSnapshot::new();
    assert!(history.capture_into(&mut after_gap));
    assert_eq!(after_gap.active_frames(), 4);
    assert_eq!(after_gap.active_origin(), 0);
    assert_eq!(after_gap.retained_range(), None);
}

#[test]
fn real_callback_journals_fractional_source_receipts_and_leaves_logical_cursor_owned_by_voice() {
    let mut fixture = Fixture::new(false);
    fixture.mixer.set_speed(RATIO);
    assert!(fixture.mixer.play_sample(0, 1.0));
    let before = fixture.voice(0).source_playback;
    for frames in [17, 31, 1] {
        fixture.render(frames);
    }
    let voice = fixture.voice(0);
    let mut snapshot = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut snapshot));
    assert_eq!(snapshot.retained_range(), Some(0..49));
    assert_eq!(snapshot.spans().len(), 1);
    let span = snapshot.spans().next().unwrap();
    // Voice birth precedes its first loop-domain configuration in the callback.
    assert_eq!(span.playback.position(), before.position());
    assert_eq!(span.playback.tempo_ratio(), before.tempo_ratio());
    assert_eq!(span.plan.frame_pos, 4_800);
    assert_eq!(span.plan.seek_mode, ExplicitSeekMode::Normal);
    assert_eq!(
        span.context.binding.source_address,
        voice.sample.as_ref().unwrap().source_address()
    );
    assert_eq!(voice.source_playback.position().frame, 4_836);
    assert_eq!(voice.source_playback.position().fraction, 0.75);
    assert_eq!(
        span.playback.position_at(49),
        voice.source_playback.position()
    );
    assert!(
        voice.stretch.productive_history().is_none(),
        "dry journal is no Native proof"
    );
}

#[test]
fn retained_native_continuation_reads_and_journals_future_input_without_moving_logical_clock() {
    let request = crate::audio_engine::prepared_native_history::tests::fixture(
        1,
        48_000,
        1.37,
        ExplicitSeekMode::Normal,
        None,
    );
    let sample = request.sample.clone();
    let permit = request.permit.clone();
    let mut logical = request.playback;
    let mut plan = request.plan;
    let target = request.target_output_frame;
    let epoch = request.epoch;
    let sequence = request.request_id;
    let mut candidate = Box::new(NativeAdapterState::new(
        Some(RubberBandLiveShifter::new(48_000, 1).unwrap()),
        1,
    ));
    candidate
        .prepare_retained_source_start_checked(
            request,
            Some(RetainedNativeStartCut {
                history_output_frames: 4096,
                discarded_raw_output_frames: 4096 + 137,
            }),
        )
        .unwrap();
    let (actual_origin, _) = candidate
        .retained_start_feed(&logical, plan)
        .unwrap()
        .unwrap();
    let address = candidate.rubberband.as_ref().unwrap().state_address();
    let mut processor = StretchProcessor::new(1);
    assert!(processor.adopt_prepared_pitch(
        &mut candidate,
        &retained_feed(&sample, &permit, &logical, plan, target),
        epoch,
        sequence,
    ));
    assert_eq!(
        processor.productive_history().unwrap().next_position,
        actual_origin.position()
    );
    let mut journal = SourceFeedHistory::new();
    let logical_origin = logical;
    let mut elapsed = 0;
    for frames in [17, 511, 1, 257] {
        let before = logical;
        let receipt = processor
            .process_source(
                retained_feed(&sample, &permit, &logical, plan, target + elapsed as u64),
                frames,
                true,
            )
            .unwrap();
        assert!(logical.matches_exact(&before));
        assert_eq!(receipt.domain, SourceReadDomain::NativeInput);
        assert_eq!(
            receipt.playback.position(),
            actual_origin.position_at(elapsed)
        );
        assert_ne!(receipt.playback.position(), logical.position());
        assert_eq!(processor.native_state_address(), address);
        assert!(processor.wet_render_ready());
        assert!(processor.wet_source_rendered);
        assert_eq!(
            processor.productive_history().unwrap().next_position,
            actual_origin.position_at(elapsed + frames)
        );
        journal.record_read(receipt);
        logical.advance(frames);
        let position = logical.position();
        plan.frame_pos = position.frame;
        plan.seek_mode = position.seek_mode;
        elapsed += frames;
    }
    let mut snapshot = SourceFeedSnapshot::new();
    assert!(journal.capture_into(&mut snapshot));
    assert_eq!(snapshot.retained_range(), Some(0..elapsed as u64));
    assert_eq!(snapshot.spans().len(), 1);
    let span = snapshot.spans().next().unwrap();
    assert_eq!(span.playback.position(), actual_origin.position());
    assert_ne!(span.playback.position(), logical_origin.position());
    let pending_input = processor.native.input_fifo[0].len();
    let retained_output = processor.native.output_fifo[0].len();
    logical.set_target(1.5);
    assert!(
        processor
            .process_source(
                retained_feed(&sample, &permit, &logical, plan, target + elapsed as u64),
                1,
                true,
            )
            .is_none()
    );
    assert_eq!(processor.native_state_address(), address);
    assert_eq!(processor.native.input_fifo[0].len(), pending_input);
    assert_eq!(processor.native.output_fifo[0].len(), retained_output);
    assert_eq!(processor.output_buffers()[0][0], 0.0);
    journal.record_gap(1);
    assert!(!snapshot.is_current());
}

fn retained_feed<'a>(
    sample: &'a SampleBuffer,
    permit: &'a NativeHistoryPermit,
    playback: &'a SourcePlayback,
    plan: SourceReadPlan,
    output_frame: u64,
) -> ProductiveSourceFeed<'a> {
    ProductiveSourceFeed {
        sample,
        stems: None,
        outgoing_stems: None,
        sample_rate_hz: 48_000,
        accepted: None,
        playback,
        plan,
        pitch_intent: PitchIntent::default(),
        key_lock: true,
        permit: Some(permit),
        output_frame: Some(output_frame),
    }
}

#[test]
fn actual_finite_callback_keeps_retained_native_feed_ahead_of_unchanged_logical_playhead() {
    let mut fixture = Fixture::new(true);
    fixture.mixer.set_speed(1.37);
    assert!(fixture.mixer.play_sample(0, 1.0));
    fixture.render(1);
    let lane = fixture
        .mixer
        .voices
        .iter()
        .position(|voice| voice.active)
        .unwrap();
    let snapshot = fixture.mixer.capture_pitch_voice_snapshot(lane).unwrap();
    let logical = snapshot.playback;
    let plan = snapshot.plan;
    let sample = snapshot.sample.clone();
    let epoch_owner = fixture.mixer.voices[lane]
        .stretch
        .source_epoch_owner()
        .clone();
    let epoch = epoch_owner.load(Ordering::Acquire);
    let permit = NativeHistoryContext {
        id: 0,
        ownership: &fixture.control.engine.input_runtime_ownership,
        acknowledgements: &fixture.control.engine.current_timing_acknowledgements,
        preparation_epoch: &epoch_owner,
    }
    .capture(&sample, 48_000, snapshot.accepted)
    .unwrap();
    let target = fixture.transport.output_frame();
    let request = snapshot
        .into_request(
            permit.clone(),
            PitchIntent::default(),
            true,
            target,
            99,
            epoch,
        )
        .unwrap();
    assert!(
        request.coverage.is_some(),
        "the actual callback source is finite"
    );
    let mut candidate = Box::new(NativeAdapterState::new(
        Some(RubberBandLiveShifter::new(48_000, 1).unwrap()),
        1,
    ));
    candidate
        .prepare_retained_source_start_checked(
            request,
            Some(RetainedNativeStartCut {
                history_output_frames: 4096,
                discarded_raw_output_frames: 4096 + 137,
            }),
        )
        .unwrap();
    let (actual_origin, _) = candidate
        .retained_start_feed(&logical, plan)
        .unwrap()
        .unwrap();
    let address = candidate.rubberband.as_ref().unwrap().state_address();
    assert!(fixture.mixer.voices[lane].stretch.adopt_prepared_pitch(
        &mut candidate,
        &retained_feed(&sample, &permit, &logical, plan, target),
        epoch,
        99,
    ));
    let mut elapsed = 0;
    for frames in [17, 31, 1, 511] {
        let pcm = fixture.render(frames);
        elapsed += frames;
        let voice = fixture.voice(0);
        assert!(pcm.iter().any(|value| *value != 0.0));
        assert_eq!(voice.stretch.native_state_address(), address);
        assert_eq!(
            voice.source_playback.position(),
            logical.position_at(elapsed)
        );
        assert_eq!(
            voice.stretch.productive_history().unwrap().next_position,
            actual_origin.position_at(elapsed)
        );
        let mut journal = SourceFeedSnapshot::new();
        assert!(voice.source_feed_history.capture_into(&mut journal));
        let actual_span = journal.spans().last().unwrap();
        assert_eq!(actual_span.playback.position(), actual_origin.position());
        assert_ne!(actual_span.playback.position(), logical.position());
        assert_eq!(
            actual_span.playback.position_at(elapsed),
            actual_origin.position_at(elapsed)
        );
        assert_eq!(
            actual_span.context.binding.source_address,
            sample.source_address()
        );
    }
}
