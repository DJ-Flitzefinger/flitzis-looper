//! Actual dispatcher order and publication outcomes; no MIDI port or audio device is opened.

use super::*;
use crate::audio_engine::input_mapping::InputRuntimeEvent;

fn routed_fixture(multi_loop: bool) -> Fixture {
    let fixture = Fixture::new(false);
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    for id in [0, 1] {
        loaded[id] = true;
        starts[id] = LOOP_START as f64 / f64::from(RATE);
        ends[id] = Some(LOOP_END as f64 / f64::from(RATE));
        bindings[id] = input_runtime_binding::capture(&fixture.control.engine, id).unwrap();
        rows[id] = PitchPadProjection {
            source_root: Some(0),
            source_mode: Some(PitchKeyMode::Major),
            quantize_step_64ths: Some(16),
            ..PitchPadProjection::default()
        };
    }
    runtime
        .set_runtime_state_with_pitch(
            multi_loop,
            loaded,
            starts,
            ends,
            bindings.iter().map(Option::as_ref).collect(),
            rows,
        )
        .unwrap();
    runtime.replace_mappings(vec![
        ("midi:note:1:40".into(), "ui.select_pad:0".into()),
        ("midi:note:1:41".into(), "ui.select_pad:1".into()),
        (
            "midi:note:2:60".into(),
            "pitch.extra:7:set_retrigger".into(),
        ),
        (
            "midi:note:2:61".into(),
            "pitch.extra:0:set_retrigger".into(),
        ),
    ]);
    runtime.set_enabled(true);
    fixture
}

fn event(fixture: &Fixture) -> InputRuntimeEvent {
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    let deadline = Instant::now() + WORKER_WAIT;
    loop {
        if let Some(event) = runtime.poll_event() {
            return event;
        }
        assert!(
            Instant::now() < deadline,
            "routed MIDI event did not arrive"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn select_a_pitch_then_b(fixture: &Fixture) -> CapturedPitchAction {
    fixture.refresh_clock();
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    // The production dispatcher receives the whole ordered sequence before controller polling.
    assert!(runtime.inject_midi_message(&[0x90, 40, 127]));
    assert!(runtime.inject_midi_message(&[0x91, 60, 127]));
    assert!(runtime.inject_midi_message(&[0x90, 41, 127]));
    assert!(!runtime.inject_midi_message(&[0x81, 60, 0]));
    assert!(!runtime.inject_midi_message(&[0x91, 60, 0]));

    let select_a = event(fixture);
    assert_eq!(select_a.action_key.as_deref(), Some("ui.select_pad:0"));
    assert!(select_a.dispatched && !select_a.direct);
    assert!(select_a.selection_revision.is_some() && select_a.captured_pitch.is_none());
    let pitch = event(fixture);
    assert_eq!(pitch.binding_key, "midi:note:2:60");
    assert_eq!(
        pitch.action_key.as_deref(),
        Some("pitch.extra:7:set_retrigger")
    );
    assert!(pitch.direct && !pitch.dispatched && pitch.pitch_rejection.is_none());
    let action = pitch.captured_pitch.unwrap();
    assert_eq!(action.received_timestamp(), pitch.received_at_ns);
    assert_eq!(action.control_guard(None).unwrap().id, 0);
    let select_b = event(fixture);
    assert_eq!(select_b.action_key.as_deref(), Some("ui.select_pad:1"));
    assert!(select_b.dispatched && !select_b.direct);
    assert!(select_b.selection_revision > select_a.selection_revision);
    assert_eq!(runtime.selected_projection().unwrap().1, Some(1));
    assert!(runtime.poll_event().is_none());
    assert!(!action.accepted() && !action.effective());
    action
}

fn start_b(fixture: &mut Fixture) {
    let action = fixture.capture(1, "pitch.extra:0:set_retrigger");
    fixture.publish(&action);
    fixture.apply(&action);
    assert_eq!(fixture.starts(1), 1);
}

#[test]
fn routed_selection_order_keeps_a_through_pending_preparation_and_actual_multiloop_effect() {
    for multi_loop in [false, true] {
        let mut fixture = routed_fixture(multi_loop);
        let mut background = routed_fixture(multi_loop);
        start_b(&mut fixture);
        start_b(&mut background);
        assert_eq!(fixture.render_to(17_000), background.render_to(17_000));
        let old_b_native = fixture.voice(1).stretch.native_state_address();
        let old_b_generation = fixture.voice(1).generation;
        let action = select_a_pitch_then_b(&fixture);
        let content = action.content_key();
        let timestamp = action.received_timestamp();
        assert_eq!(action.original_due_frame(), 24_000);
        assert_eq!(action.exclusive(), !multi_loop);
        assert_eq!(action.render().pitch, PitchIntent::new(0, 7).unwrap());
        assert_eq!(
            content,
            fixture
                .control
                .engine
                .input_runtime_ownership
                .slot_snapshot(0)
                .unwrap()
                .content_key()
                .unwrap()
        );

        fixture.publish(&action);
        assert!(!action.effective());
        let before = fixture.apply(&action);
        assert_eq!(action.execution_target(), 24_000);
        assert_eq!(before, background.render_to(action.execution_target()));
        background.render(1);
        assert_native_source(&fixture, 0, &action);
        assert_eq!(action.content_key(), content);
        assert_eq!(action.received_timestamp(), timestamp);
        assert_eq!(fixture.starts(0), 1);
        assert_eq!(fixture.starts(1), 1);
        assert_eq!(
            fixture.mixer.pad_pitch_intent(1),
            Some(PitchIntent::default())
        );
        assert_source_phase(
            fixture.voice(0).source_playback.position(),
            LOOP_START as f64,
            1,
            1.0,
        );
        let output = fixture.pcm(48_000);
        let old_b_pcm = background.pcm(48_000);
        let a_pcm = if multi_loop {
            assert_eq!(fixture.voice(1).generation, old_b_generation);
            assert_eq!(
                fixture.voice(1).stretch.native_state_address(),
                old_b_native
            );
            assert!(
                fixture
                    .voice(1)
                    .source_playback
                    .matches_exact(&background.voice(1).source_playback)
            );
            output
                .iter()
                .zip(old_b_pcm)
                .map(|(mixed, b)| mixed - b)
                .collect::<Vec<_>>()
        } else {
            assert!(
                !fixture
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.active && voice.sample_id == 1)
            );
            output
        };
        assert_tone(&a_pcm[12_000..], 0, 7, 1.0, false);
        assert_source_phase(
            fixture.voice(0).source_playback.position(),
            LOOP_START as f64,
            48_001,
            1.0,
        );
        assert_eq!(
            fixture
                .control
                .engine
                .input_selected_projection()
                .unwrap()
                .1,
            Some(1)
        );
    }
}

#[test]
fn routed_capacity_refusal_never_replays_a_against_b_and_fresh_b_is_independent() {
    for multi_loop in [false, true] {
        let mut fixture = routed_fixture(multi_loop);
        let mut explicit_b = routed_fixture(multi_loop);
        start_b(&mut fixture);
        start_b(&mut explicit_b);
        assert_eq!(fixture.render_to(17_000), explicit_b.render_to(17_000));
        let action = select_a_pitch_then_b(&fixture);
        let content = action.content_key();
        let timestamp = action.received_timestamp();
        {
            let mut producer = fixture.control.producer.lock().unwrap();
            while producer.slots() != 0 {
                producer.push(ControlMessage::StopAll()).unwrap();
            }
        }
        assert!(
            fixture
                .control
                .engine
                .publish_pitch_action(&action)
                .is_err()
        );
        assert_eq!(action.reason(), Some("capacity"));
        assert!(!action.accepted() && !action.effective());
        // Retire only this test's unexecuted scalar pressure messages, preserving real playback.
        while let Ok(message) = fixture.control.consumer.pop() {
            assert!(matches!(message, ControlMessage::StopAll()));
        }
        assert_eq!(fixture.starts(0), 0);
        assert_eq!(
            fixture.mixer.pad_pitch_intent(0),
            Some(PitchIntent::default())
        );
        fixture.refresh_clock();
        let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
        assert!(runtime.inject_midi_message(&[0x91, 61, 127]));
        let fresh_event = event(&fixture);
        let fresh = fresh_event.captured_pitch.unwrap();
        assert_eq!(fresh.received_timestamp(), fresh_event.received_at_ns);
        assert_eq!(fresh.control_guard(None).unwrap().id, 1);
        assert_ne!(fresh.content_key(), content);
        assert!(fresh.sequence() > action.sequence());
        assert_eq!(fresh.original_due_frame(), 24_000);
        let reference = explicit_b.capture(1, "pitch.extra:0:set_retrigger");
        fixture.publish(&fresh);
        explicit_b.publish(&reference);
        assert_eq!(fixture.apply(&fresh), explicit_b.apply(&reference));
        assert_eq!(fixture.pcm(4096), explicit_b.pcm(4096));
        assert_eq!(fixture.starts(0), 0);
        assert_eq!(fixture.starts(1), 2);
        assert_eq!(action.content_key(), content);
        assert_eq!(action.received_timestamp(), timestamp);
        assert_eq!(action.original_due_frame(), 24_000);
        assert_eq!(action.reason(), Some("capacity"));
        assert!(!action.effective());
    }
}
