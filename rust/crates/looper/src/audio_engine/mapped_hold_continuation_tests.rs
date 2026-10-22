// Genuine original-map launches and public own-ACK Hold actions through the callback.
// Synthetic PCM proves exact addressing/control, never device or musical acceptance.
use super::*;
use crate::audio_engine::hold_action::{HoldActionState, HoldActionTicket};
use crate::audio_engine::source_playback::FractionalSourcePosition;
use crate::messages::{ControlParameterMessage, TriggerQuantization};

mod refusal_tests {
    include!("mapped_hold_refusal_tests.rs");
}

mod public_route_tests {
    include!("mapped_hold_public_route_tests.rs");
}

mod ordinary_transport_tests {
    include!("ordinary_mapped_transport_tests.rs");
}

fn qualified() -> (LaunchPath, OfflineAnalysisJob, CapturedPadStart) {
    let mut path = mapped_fixed_pitch_birth_tests::configured(0);
    let (job, delivery) = variable_original_delivery(&path);
    publish_original(&path, &delivery, 1);
    let captured = path.start();
    (path, job, captured)
}

fn lane(path: &LaunchPath) -> usize {
    path.source
        .mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(0))
        .unwrap()
}

fn pause(path: &LaunchPath, press: u64) -> HoldActionTicket {
    Python::attach(|py| {
        let values = PyDict::new(py);
        values.set_item("press", press).unwrap();
        let value = values.get_item("press").unwrap().unwrap();
        // The same public PyO3 input parser and producer used by waveform control.
        path.source
            .engine
            .pause_waveform_hold(0, &value, None)
            .unwrap()
    })
}

fn release(path: &LaunchPath, ticket: &HoldActionTicket) {
    assert!(path.source.engine.release_waveform_hold(ticket).unwrap());
}

fn exact(left: FractionalSourcePosition, right: FractionalSourcePosition) {
    assert_eq!(left.frame, right.frame);
    assert_eq!(left.fraction.to_bits(), right.fraction.to_bits());
    assert_eq!(left.seek_mode, right.seek_mode);
}

fn snapshot(path: &LaunchPath) -> SourceFeedSnapshot {
    let mut result = SourceFeedSnapshot::new();
    assert!(path.voice().source_feed_history.capture_into(&mut result));
    result
}

fn same_spans(left: &SourceFeedSnapshot, right: &SourceFeedSnapshot) {
    assert_eq!(left.active_frames(), right.active_frames());
    assert_eq!(left.spans().len(), right.spans().len());
    for (left, right) in left.spans().zip(right.spans()) {
        assert_eq!(
            (left.active_start, left.frames),
            (right.active_start, right.frames)
        );
        assert!(left.playback.matches_exact(&right.playback));
        assert!(left.plan.matches_exact(right.plan));
        assert_eq!(left.context, right.context);
    }
}

fn render_partitioned(path: &mut LaunchPath, count: usize, partitions: &[usize]) -> Vec<f32> {
    let mut output = Vec::new();
    let mut remaining = count;
    let mut index = 0;
    while remaining > 0 {
        let frames = remaining.min(partitions[index % partitions.len()]);
        output.extend(path.render(frames));
        remaining -= frames;
        index += 1;
    }
    output
}

fn pcm_at(source: &crate::messages::SampleBuffer, position: f64) -> [f32; 2] {
    let left = 32 + (position.floor() as usize - 32) % 32;
    let right = 32 + (left + 1 - 32) % 32;
    let fraction = position.fract() as f32;
    std::array::from_fn(|channel| {
        let a = source.sample_at_absolute(left, channel);
        let b = source.sample_at_absolute(right, channel);
        a + (b - a) * fraction
    })
}

fn coordinate(total: f64) -> f64 {
    let (beginning, quarters) = mapped_fixed_pitch_birth_tests::independent_extent();
    let beat = beginning + total.rem_euclid(quarters);
    let seconds = if beat < 0.6 {
        (beat / 0.6) * (48.0 / f64::from(RATE))
    } else {
        (48.0 / f64::from(RATE)) + ((beat - 0.6) / 0.4) * (16.0 / f64::from(RATE))
    };
    seconds * f64::from(RATE)
}

fn assert_pcm(output: &[f32], expected: impl Iterator<Item = [f32; 2]>) {
    let expected: Vec<f32> = expected.flatten().collect();
    assert_eq!(output.len(), expected.len());
    let error = output
        .iter()
        .zip(expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(error < 3e-6, "independent dry PCM error={error}");
}

fn repeated_holds(partitions: &[usize]) -> Vec<f32> {
    let (mut path, job, captured) = qualified();
    let source = path.voice().sample.as_ref().unwrap().clone();
    let original_epoch = path.voice().source_playback.mapped_epoch().unwrap();
    let original_master = path.timeline;
    let mut all = Vec::new();
    let mut active = 0;
    // Boundaries and durations are fixed before any result is observed.
    for (press, running, silent) in [(1, 7, 113), (2, 29, 67), (3, 71, 193)] {
        let output = render_partitioned(&mut path, running, partitions);
        assert_pcm(
            &output,
            (active..active + running)
                .map(|frame| pcm_at(&source, coordinate(frame as f64 / 64.0))),
        );
        active += running;
        all.extend(output);
        let saved = path.voice().source_playback;
        let revision = path.voice().source_feed_history.continuity_revision();
        let history = snapshot(&path);
        let pause_frame = path.timeline.output_frame();
        let ticket = pause(&path, press);
        assert_eq!(ticket.publication_status(), "pause_pending");
        assert!(ticket.action.effect().is_none());
        path.drain();
        assert_eq!(ticket.action.state(), HoldActionState::PausedOwned);
        assert_eq!(
            ticket.action.effect().unwrap().paused_at_output_frame,
            pause_frame
        );
        assert!(path.voice().paused);
        assert!(path.voice().source_playback.matches_exact(&saved));
        assert_eq!(
            path.voice().source_feed_history.continuity_revision(),
            revision + 1
        );
        assert!(path.timeline.matches_clock_epoch(&original_master));
        let silence = render_partitioned(&mut path, silent, partitions);
        assert!(silence.iter().all(|sample| *sample == 0.0));
        all.extend(silence);
        same_spans(&history, &snapshot(&path));
        assert!(path.voice().source_playback.matches_exact(&saved));
        release(&path, &ticket);
        path.drain();
        assert_eq!(ticket.action.state(), HoldActionState::Resumed);
        assert!(!path.voice().paused);
        exact(saved.position(), path.voice().source_playback.position());
        assert_eq!(
            path.voice().source_playback.mapped_output_frame(),
            Some(path.timeline.output_frame())
        );
        assert_eq!(
            path.voice().source_feed_history.continuity_revision(),
            revision + 2
        );
        same_spans(&history, &snapshot(&path));
        let after = path.voice().source_playback;
        let revision = path.voice().source_feed_history.continuity_revision();
        release(&path, &ticket);
        path.drain();
        assert!(path.voice().source_playback.matches_exact(&after));
        assert_eq!(
            path.voice().source_feed_history.continuity_revision(),
            revision
        );
        assert!(
            path.voice()
                .mapped_start
                .as_ref()
                .unwrap()
                .same_owner(&captured)
        );
        // Original selected-map changes and cancellation cannot rewrite an effective owner.
        if press == 1 {
            job.cancel();
        }
    }
    let output = render_partitioned(&mut path, 257, partitions);
    assert_pcm(
        &output,
        (active..active + 257).map(|frame| pcm_at(&source, coordinate(frame as f64 / 64.0))),
    );
    all.extend(output);
    let history = snapshot(&path);
    assert_eq!(history.retained_range(), Some(0..(active + 257) as u64));
    let mut expected_active = 0;
    let mut saw_translated = false;
    for span in history.spans() {
        assert_eq!(span.active_start, expected_active);
        if !span
            .playback
            .mapped_epoch()
            .unwrap()
            .matches_exact(original_epoch)
        {
            saw_translated = true;
        }
        for offset in 0..span.frames {
            let position = span.playback.position_at(offset).unwrap();
            exact(
                position,
                captured
                    .mapped_geometry()
                    .unwrap()
                    .position_at(original_epoch, expected_active + offset as u64)
                    .unwrap(),
            );
        }
        expected_active += span.frames as u64;
    }
    assert!(saw_translated);
    assert_eq!(expected_active, (active + 257) as u64);
    all
}

#[test]
fn actual_qualified_hold_pcm_saved_fraction_knots_wrap_history_and_partitions() {
    assert_eq!(repeated_holds(&[1]), repeated_holds(&[17, 1, 53, 3]));
}

#[test]
fn actual_qualified_hold_release_before_ack_and_prepaused_idempotency() {
    let (mut path, _job, _captured) = qualified();
    path.render(7);
    let saved = path.voice().source_playback;
    let revision = path.voice().source_feed_history.continuity_revision();
    let ticket = pause(&path, 1);
    release(&path, &ticket);
    path.drain();
    assert_eq!(ticket.action.state(), HoldActionState::Resumed);
    exact(path.voice().source_playback.position(), saved.position());
    assert_eq!(
        path.voice().source_feed_history.continuity_revision(),
        revision + 2
    );
    path.render(3);
    let ticket = pause(&path, 2);
    path.drain();
    path.render(113);
    let saved = path.voice().source_playback;
    let revision = path.voice().source_feed_history.continuity_revision();
    let already = pause(&path, 3);
    path.drain();
    assert_eq!(already.action.state(), HoldActionState::NoEffect);
    assert_eq!(ticket.action.state(), HoldActionState::Superseded);
    release(&path, &already);
    release(&path, &ticket);
    path.drain();
    assert!(path.voice().paused);
    assert!(path.voice().source_playback.matches_exact(&saved));
    assert_eq!(
        path.voice().source_feed_history.continuity_revision(),
        revision
    );
}

#[test]
fn actual_qualified_hold_then_global_master_claim_and_second_hold_keep_pcm() {
    let (mut path, _job, _captured) = qualified();
    let (producer, mut consumer) = rtrb::RingBuffer::<ControlParameterMessage>::new(16);
    path.source.engine.test_parameter_producer = Some(Arc::new(std::sync::Mutex::new(producer)));
    let source = path.voice().sample.as_ref().unwrap().clone();
    path.render(7);
    let ticket = pause(&path, 1);
    path.drain();
    path.render(113);
    release(&path, &ticket);
    path.drain();
    path.render(11);
    let execution = path.timeline.output_frame();
    let saved = path.voice().source_playback.position();
    let old_history = snapshot(&path);
    let beat = path.timeline.beat_position_at_frame(execution).unwrap();
    let rate = 1.16;
    path.source
        .engine
        .set_speed_and_master_period(rate, PERIOD / rate)
        .unwrap();
    path.drain();
    drain_parameter_messages(&mut consumer, &mut path.source.mixer, &mut path.timeline);
    path.render(0);
    let effect = path
        .source
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap()
        .effect;
    assert_eq!(
        effect.state,
        crate::audio_engine::global_rate_status::RateState::Applied,
        "{:?}",
        effect.error
    );
    exact(saved, path.voice().source_playback.position());
    assert_eq!(
        path.timeline
            .beat_position_at_frame(execution)
            .unwrap()
            .to_bits(),
        beat.to_bits()
    );
    same_spans(&old_history, &snapshot(&path));
    let master = path.timeline;
    let output = path.render(137);
    assert_pcm(
        &output,
        (execution..execution + 137).map(|frame| {
            let total = 18.0 / 64.0
                + (master.beat_position_at_frame(frame).unwrap()
                    - master.beat_position_at_frame(execution).unwrap());
            pcm_at(&source, coordinate(total))
        }),
    );
    let saved = path.voice().source_playback.position();
    let ticket = pause(&path, 2);
    path.drain();
    path.render(89);
    release(&path, &ticket);
    path.drain();
    assert_eq!(ticket.action.state(), HoldActionState::Resumed);
    exact(saved, path.voice().source_playback.position());
    let resumed = path.timeline.output_frame();
    let output = path.render(97);
    assert_pcm(
        &output,
        (resumed..resumed + 97).map(|frame| {
            let addressed = frame - 89;
            pcm_at(
                &source,
                coordinate(
                    18.0 / 64.0
                        + (master.beat_position_at_frame(addressed).unwrap()
                            - master.beat_position_at_frame(execution).unwrap()),
                ),
            )
        }),
    );
}
