//! Existing public controls must preserve an actually retained Native input offset.
//! Device-free chronology and PCM regressions; no musical-anchor or hearing claim.

#[path = "retained_stem_tests.rs"]
mod retained_stem_tests;

use super::*;

fn retained_fixture() -> (Fixture, SourcePlayback, SourcePlayback, usize) {
    retained_fixture_with_setup(|_| {})
}

fn retained_fixture_with_setup(
    setup: impl FnOnce(&mut Fixture),
) -> (Fixture, SourcePlayback, SourcePlayback, usize) {
    let mut fixture = Fixture::new(true);
    setup(&mut fixture);
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
    let epoch_owner = fixture
        .control
        .engine
        .prepared_source_epoch(0)
        .unwrap()
        .clone();
    let epoch = fixture.mixer.voices[lane].stretch.preparation_epoch();
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
    assert!(request.coverage.is_some(), "the real source is finite");
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
    let (actual, _) = candidate
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
    assert_ne!(actual.position(), logical.position());
    (fixture, logical, actual, address)
}

fn assert_retained_coordinates(
    fixture: &Fixture,
    logical: &SourcePlayback,
    actual: &SourcePlayback,
    elapsed: usize,
) {
    let voice = fixture.voice(0);
    assert_eq!(
        voice.source_playback.position(),
        logical.position_at(elapsed)
    );
    let productive = voice.stretch.productive_history().unwrap();
    assert_eq!(productive.next_position, actual.position_at(elapsed));
    assert_ne!(productive.next_position, voice.source_playback.position());
    let sample = voice.sample.as_ref().unwrap();
    assert_eq!(productive.binding.source_address, sample.source_address());
    assert_eq!(productive.binding.resident, sample.resident_binding());
    let taps = voice.stretch.native_tap_observation().unwrap();
    assert!(taps.left_reads > 0);
    assert_eq!(taps.missing_reads, 0);
    let mut journal = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut journal));
    let span = journal.spans().last().unwrap();
    assert_eq!(span.context.binding.source_address, sample.source_address());
    assert_eq!(
        span.playback.position_at(span.frames),
        productive.next_position
    );
}

fn assert_nonzero_pcm(pcm: &[f32]) {
    assert!(pcm.iter().all(|value| value.is_finite()));
    assert!(
        pcm.iter().any(|value| *value != 0.0),
        "lost retained output"
    );
}

#[test]
fn public_captured_set_preserves_retained_actual_feed_and_logical_phase_under_unequal_partitions() {
    let (mut fixture, logical, actual, old_native) = retained_fixture();
    let (mut reference, reference_logical, reference_actual, reference_native) = retained_fixture();
    assert_ne!(old_native, reference_native);
    let generation = fixture.voice(0).generation;
    let starts = fixture.starts(0);
    let mut elapsed = 0;
    let mut before = Vec::new();
    for frames in [17, 31, 1, 511] {
        before.extend(fixture.render(frames));
        elapsed += frames;
        assert_retained_coordinates(&fixture, &logical, &actual, elapsed);
    }
    assert_nonzero_pcm(&before);
    assert_eq!(before, reference.render(elapsed));
    assert_retained_coordinates(&reference, &reference_logical, &reference_actual, elapsed);

    let capture = fixture.transport.output_frame();
    let action = fixture.capture(0, "pitch.extra:7:set");
    let reference_action = reference.capture(0, "pitch.extra:7:set");
    fixture.publish(&action);
    reference.publish(&reference_action);
    let execution = capture + 8192;
    assert_eq!(action.original_due_frame(), capture);
    assert_eq!(reference_action.original_due_frame(), capture);
    assert_eq!(action.execution_target(), execution);
    assert_eq!(reference_action.execution_target(), execution);
    fixture.wait_worker(&[&action]);
    reference.wait_worker(&[&reference_action]);
    assert_eq!(fixture.transport.output_frame(), capture);
    assert_retained_coordinates(&fixture, &logical, &actual, elapsed);

    let before_effect = fixture.render_to(execution);
    assert_nonzero_pcm(&before_effect);
    assert_eq!(before_effect, reference.render_to(execution));
    elapsed += (execution - capture) as usize;
    assert!(!action.effective());
    assert_eq!(fixture.voice(0).stretch.native_state_address(), old_native);
    assert_retained_coordinates(&fixture, &logical, &actual, elapsed);
    fixture.render(0);
    reference.render(0);
    assert!(
        action.effective(),
        "{} {:?}",
        action.state(),
        action.reason()
    );
    assert!(reference_action.effective());
    assert_eq!(action.execution_target(), execution);
    assert_eq!(fixture.transport.output_frame(), execution);
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), starts);
    let native = fixture.voice(0).stretch.native_state_address();
    assert_ne!(native, old_native);
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(action.sequence())
    );
    assert!(
        fixture
            .voice(0)
            .stretch
            .owns_pitch_contract(PitchIntent::new(0, 7).unwrap(), true)
    );
    assert_retained_coordinates(&fixture, &logical, &actual, elapsed);

    let mut continued = Vec::new();
    let continuation_frames = [1, 17, 31, 511, 257, 1, 512, 73];
    for frames in continuation_frames {
        let pcm = fixture.render(frames);
        assert_nonzero_pcm(&pcm);
        continued.extend(pcm);
        elapsed += frames;
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.starts(0), starts);
        assert_retained_coordinates(&fixture, &logical, &actual, elapsed);
    }
    let expected =
        reference.render_to(execution + continuation_frames.iter().sum::<usize>() as u64);
    assert_nonzero_pcm(&expected);
    assert_eq!(
        continued, expected,
        "unequal callback partitions lost or changed PCM"
    );
    assert_retained_coordinates(&reference, &reference_logical, &reference_actual, elapsed);
}

fn retained_fifo_samples(fixture: &Fixture) -> (Vec<f32>, Vec<f32>) {
    let native = &fixture.voice(0).stretch.native;
    let samples = |fifo: &FixedFifo| {
        (0..fifo.len)
            .map(|offset| fifo.buffer[(fifo.read_pos + offset) % fifo.buffer.len()])
            .collect()
    };
    (
        samples(&native.input_fifo[0]),
        samples(&native.output_fifo[0]),
    )
}

fn drain_retained_rate(
    fixture: &mut Fixture,
    consumer: &mut rtrb::Consumer<crate::messages::ControlParameterMessage>,
) {
    crate::audio_engine::audio_stream::drain_parameter_messages(
        consumer,
        &mut fixture.mixer,
        &mut fixture.transport,
    );
    fixture.drain();
    fixture.render(0);
    assert!(consumer.is_empty());
}

// Independent scalar .05/512 schedule. Neither the source cursor nor receipt replay
// supplies the expected elapsed source distance after the public rate commit.
fn retained_rate_distance(old: f64, target: f64, frames: usize) -> f64 {
    let mut remaining = frames;
    let mut ratio = old;
    let mut distance = 0.0;
    while remaining != 0 {
        ratio += (target - ratio).clamp(-0.05, 0.05);
        let count = remaining.min(512);
        distance += count as f64 * ratio;
        remaining -= count;
    }
    distance
}

fn assert_retained_rate_phase(
    observed: crate::audio_engine::source_playback::FractionalSourcePosition,
    origin: crate::audio_engine::source_playback::FractionalSourcePosition,
    distance: f64,
) {
    // This finite fixture uses its actual physical [4800,100800) Normal loop.
    let expected =
        4800.0 + (origin.frame as f64 + origin.fraction - 4800.0 + distance).rem_euclid(96_000.0);
    let actual = observed.frame as f64 + observed.fraction;
    assert!(
        (actual - expected).abs() < 1.0e-8,
        "phase={actual} expected={expected}"
    );
    assert_eq!(observed.seek_mode, ExplicitSeekMode::Normal);
}

#[test]
fn public_rate_commit_preserves_retained_native_fifo_history_and_both_moving_source_clocks() {
    use crate::audio_engine::global_rate_status::{RateError, RateState};
    use crate::messages::ControlParameterMessage;

    let (mut fixture, _, _, native) = retained_fixture();
    let (mut reference, _, _, reference_native) = retained_fixture();
    assert_ne!(native, reference_native);
    let mut past = Vec::new();
    for frames in [17, 31, 1, 511] {
        past.extend(fixture.render(frames));
    }
    assert_nonzero_pcm(&past);
    assert_eq!(past, reference.render_to(fixture.transport.output_frame()));
    let (parameters, mut consumer) = rtrb::RingBuffer::<ControlParameterMessage>::new(8);
    let (reference_parameters, mut reference_consumer) =
        rtrb::RingBuffer::<ControlParameterMessage>::new(8);
    fixture.control.engine.test_parameter_producer =
        Some(Arc::new(std::sync::Mutex::new(parameters)));
    reference.control.engine.test_parameter_producer =
        Some(Arc::new(std::sync::Mutex::new(reference_parameters)));
    // The retained fixture set its initial mixer speed directly. Establish the same
    // public effect observation without changing that productive history.
    for (current, parameter_consumer) in [
        (&mut fixture, &mut consumer),
        (&mut reference, &mut reference_consumer),
    ] {
        current.control.engine.set_speed(1.37).unwrap();
        drain_retained_rate(current, parameter_consumer);
        let status = current
            .control
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap();
        assert_eq!(status.effect.state, RateState::Applied);
        assert_eq!(status.effect.effective.speed, 1.37);
        assert_eq!(status.effect.effective.period, None);
    }
    let capture = fixture.transport.output_frame();
    assert_eq!(reference.transport.output_frame(), capture);
    let logical = fixture.voice(0).source_playback;
    let actual = fixture.voice(0).stretch.productive_history().unwrap();
    let origin = fixture.voice(0).stretch.native_origin_tuple();
    let generation = fixture.voice(0).generation;
    let starts = fixture.starts(0);
    let fifo = retained_fifo_samples(&fixture);
    let old_transport_period = fixture.transport.master_period_seconds();
    assert_eq!(logical.tempo_ratio(), 1.37);
    assert_ne!(logical.position(), actual.next_position);
    assert_eq!(fifo, retained_fifo_samples(&reference));
    assert!(
        fixture
            .voice(0)
            .stretch
            .owns_pitch_contract(PitchIntent::default(), true)
    );

    fixture.control.engine.set_speed(1.5).unwrap();
    reference.control.engine.set_speed(1.5).unwrap();
    // Queueing the real parameter request alone cannot change any effective tuple,
    // either source chronology, or the unique Native and its pending samples.
    for current in [&mut fixture, &mut reference] {
        current.render(0);
        let queued = current
            .control
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap();
        assert!(queued.request_revision > queued.effect.revision);
        assert_eq!(queued.requested.speed, 1.5);
        assert_eq!(queued.effect.effective.speed, 1.37);
        assert_eq!(queued.effect.effective.period, None);
        assert_eq!(current.transport.output_frame(), capture);
        assert_eq!(
            current.transport.master_period_seconds(),
            old_transport_period
        );
        assert!(current.voice(0).source_playback.matches_exact(&logical));
        let productive = current.voice(0).stretch.productive_history().unwrap();
        assert_eq!(productive.next_position, actual.next_position);
        assert_eq!(productive.fed_output_frames, actual.fed_output_frames);
        assert_eq!(retained_fifo_samples(current), fifo);
    }
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(
        reference.voice(0).stretch.native_state_address(),
        reference_native
    );

    drain_retained_rate(&mut fixture, &mut consumer);
    drain_retained_rate(&mut reference, &mut reference_consumer);
    for current in [&fixture, &reference] {
        let applied = current
            .control
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap();
        assert_eq!(applied.effect.state, RateState::Applied);
        assert_eq!(applied.effect.error, RateError::None);
        assert_eq!(applied.effect.revision, applied.request_revision);
        assert_eq!(applied.effect.execution_frame, Some(capture));
        assert_eq!(applied.effect.effective.speed, 1.5);
        assert_eq!(applied.effect.effective.period, None);
        assert_eq!(current.transport.output_frame(), capture);
        assert_eq!(
            current.transport.master_period_seconds(),
            old_transport_period
        );
        assert_eq!(
            current.voice(0).source_playback.position(),
            logical.position()
        );
        let productive = current.voice(0).stretch.productive_history().unwrap();
        assert_eq!(productive.next_position, actual.next_position);
        assert_eq!(productive.fed_output_frames, actual.fed_output_frames);
        assert_eq!(retained_fifo_samples(current), fifo);
        assert_eq!(current.voice(0).stretch.native_origin_tuple(), origin);
        let voice = current.voice(0);
        let source = voice.stretch.native.source.as_ref().unwrap();
        assert!(
            source
                .permit
                .current_effective_source(voice.sample.as_ref().unwrap(), 48_000),
            "retained fixture's source permit was invalidated by a rate-only edit"
        );
    }
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(
        reference.voice(0).stretch.native_state_address(),
        reference_native
    );

    let mut elapsed = 0;
    for frames in [1, 17, 31, 511, 257, 1, 512, 73, 511, 1024] {
        let pcm = fixture.render(frames);
        // A different callback division crosses the same active-frame rate steps.
        let mut expected = Vec::new();
        let mut remaining = frames;
        while remaining != 0 {
            let count = remaining.min(113);
            expected.extend(reference.render(count));
            remaining -= count;
        }
        assert_nonzero_pcm(&pcm);
        assert_eq!(
            pcm, expected,
            "rate smoothing changed retained PCM with callback division"
        );
        elapsed += frames;
        let distance = retained_rate_distance(logical.tempo_ratio(), 1.5, elapsed);
        let voice = fixture.voice(0);
        assert_retained_rate_phase(
            voice.source_playback.position(),
            logical.position(),
            distance,
        );
        let productive = voice.stretch.productive_history().unwrap();
        assert_retained_rate_phase(productive.next_position, actual.next_position, distance);
        assert_ne!(voice.source_playback.position(), productive.next_position);
        assert_eq!(
            productive.fed_output_frames,
            actual.fed_output_frames + elapsed as u64
        );
        assert_eq!(productive.binding, actual.binding);
        assert_eq!(voice.stretch.native_state_address(), native);
        assert_eq!(voice.stretch.native_origin_tuple(), origin);
        assert_eq!(voice.generation, generation);
        assert_eq!(fixture.starts(0), starts);
        assert!(
            voice
                .source_playback
                .matches_exact(&reference.voice(0).source_playback)
        );
        assert_eq!(
            retained_fifo_samples(&fixture),
            retained_fifo_samples(&reference)
        );
        let taps = voice.stretch.native_tap_observation().unwrap();
        assert!(taps.left_reads > 0);
        assert_eq!(taps.missing_reads, 0);
        let mut journal = SourceFeedSnapshot::new();
        assert!(voice.source_feed_history.capture_into(&mut journal));
        assert_eq!(journal.retained_range(), Some(0..capture + elapsed as u64));
        let span = journal.spans().last().unwrap();
        assert_eq!(span.context.binding, productive.binding);
        assert_eq!(
            span.playback.position_at(span.frames),
            productive.next_position
        );
    }
    assert_eq!(fixture.voice(0).source_playback.tempo_ratio(), 1.5);
    assert_eq!(fixture.voice(0).source_playback.rate_target(), 1.5);
    assert_eq!(
        reference.voice(0).stretch.native_state_address(),
        reference_native
    );
}
