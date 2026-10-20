//! Same-source control edits refresh selection authority without inventing selection.

use super::*;
use crate::audio_engine::audio_stream::drain_parameter_messages;
use crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_sources;
use crate::audio_engine::pitch_actions::PitchKeyMode;
use crate::audio_engine::pitch_runtime_tests::{Fixture, LOOP_END, LOOP_START, publish_tone};
use crate::messages::{ControlParameterMessage, ResidentContext, SampleBuffer};
use rtrb::RingBuffer;

const ACTION: &str = "pitch.extra:2:set_retrigger";

#[derive(Clone, Copy, Debug)]
enum Edit {
    Loop,
    Bpm,
    GridOrigin,
}

fn refresh_rows(fixture: &Fixture, edit: Option<Edit>) {
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    for id in [0, 1] {
        loaded[id] = true;
        starts[id] = if id == 0 && matches!(edit, Some(Edit::Loop)) {
            0.2
        } else {
            0.1
        };
        ends[id] = Some(2.1);
        bindings[id] = fixture
            .control
            .engine
            .current_input_runtime_pad_binding(id)
            .unwrap();
        assert!(bindings[id].as_ref().unwrap().current());
        rows[id] = PitchPadProjection {
            source_root: Some(0),
            source_mode: Some(PitchKeyMode::Major),
            ..PitchPadProjection::default()
        };
    }
    fixture
        .control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_pitch(
            true,
            loaded,
            starts,
            ends,
            bindings.iter().map(Option::as_ref).collect(),
            rows,
        )
        .unwrap();
}

fn fixture() -> Fixture {
    let mut fixture = Fixture::new(true);
    adopt_current_sources(&mut fixture, true);
    refresh_rows(&fixture, None);
    fixture.control.engine.select_input_pad(0).unwrap();
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    runtime.replace_mappings(vec![("midi:note:1:60".into(), ACTION.into())]);
    runtime.set_enabled(true);
    fixture.refresh_clock();
    fixture
}

fn mutate(fixture: &mut Fixture, edit: Edit) {
    match edit {
        Edit::Loop => fixture
            .control
            .engine
            .set_pad_loop_region(0, 0.2, Some(2.1))
            .unwrap(),
        Edit::Bpm => {
            let (producer, mut consumer) = RingBuffer::<ControlParameterMessage>::new(8);
            fixture.control.engine.test_parameter_producer = Some(Arc::new(Mutex::new(producer)));
            fixture.control.engine.set_pad_bpm(0, Some(97.0)).unwrap();
            drain_parameter_messages(&mut consumer, &mut fixture.mixer, &mut fixture.transport);
            assert!(consumer.is_empty());
            assert!((fixture.mixer.output_bpm_for_sample_id(0).unwrap() - 97.0).abs() < 1.0e-9);
        }
        Edit::GridOrigin => fixture
            .control
            .engine
            .set_pad_timing_metadata(0, 0.125)
            .unwrap(),
    }
    fixture.drain();
    refresh_rows(fixture, Some(edit));
    fixture.refresh_clock();
}

fn next_event(fixture: &Fixture) -> InputRuntimeEvent {
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(event) = runtime.poll_event() {
            return event;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "real selected MIDI event timed out"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn same_source_control_edit_keeps_selected_capture(edit: Edit, midi: bool) {
    let mut fixture = fixture();
    let selection = fixture.control.engine.input_selected_projection().unwrap();
    assert_eq!(selection.1, Some(0));
    let original = fixture
        .control
        .engine
        .capture_selected_pitch_action(ACTION, None)
        .unwrap();
    let old = original.control_guard(None).unwrap().binding;
    let key = original.content_key();
    mutate(&mut fixture, edit);

    let explicit = fixture
        .control
        .engine
        .capture_pad_pitch_action(0, ACTION, None)
        .unwrap();
    let current = explicit.control_guard(None).unwrap().binding;
    assert_eq!(explicit.content_key(), key);
    assert_eq!(current.source_address, old.source_address);
    assert_eq!(current.sample_count, old.sample_count);
    assert_eq!(current.channels, old.channels);
    assert_eq!(current.sample_rate_hz, old.sample_rate_hz);
    assert!(current.authority_revision > old.authority_revision);
    assert!(
        original.control_guard(None).is_err(),
        "a previous capture must retain its old authority"
    );
    assert!(!original.accepted() && !original.effective());

    let selected = if midi {
        let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
        assert!(runtime.inject_midi_message(&[0x90, 60, 100]));
        let event = next_event(&fixture);
        assert_eq!(event.action_key.as_deref(), Some(ACTION));
        assert!(event.direct && !event.dispatched);
        assert!(
            event.pitch_rejection.is_none(),
            "same-source {edit:?} left the real MIDI selection stale: {:?}",
            event.pitch_rejection
        );
        let captured = event.captured_pitch.unwrap();
        assert_eq!(captured.received_timestamp(), event.received_at_ns);
        captured
    } else {
        // This public Native entry is the existing selected keyboard pitch route.
        fixture
            .control
            .engine
            .capture_selected_pitch_action(ACTION, None)
            .unwrap_or_else(|reason| {
                panic!("explicit pad works but selected keyboard fails after {edit:?}: {reason}")
            })
    };
    assert_eq!(selected.content_key(), key);
    let guard = selected.control_guard(None).unwrap();
    assert_eq!(guard.id, 0);
    assert_eq!(guard.binding.authority_revision, current.authority_revision);
    assert_eq!(guard.binding.source_address, old.source_address);
    assert_eq!(
        fixture.control.engine.input_selected_projection().unwrap(),
        selection
    );
    assert_eq!(
        fixture
            .control
            .engine
            .input_runtime_ownership
            .endpoint_selection(),
        Some((selection.0, Some(key)))
    );
}

#[test]
fn selected_keyboard_pitch_keeps_source_after_loop_edit() {
    same_source_control_edit_keeps_selected_capture(Edit::Loop, false);
}

#[test]
fn selected_midi_pitch_keeps_source_after_loop_edit() {
    same_source_control_edit_keeps_selected_capture(Edit::Loop, true);
}

#[test]
fn selected_keyboard_pitch_keeps_source_after_bpm_edit() {
    same_source_control_edit_keeps_selected_capture(Edit::Bpm, false);
}

#[test]
fn selected_midi_pitch_keeps_source_after_bpm_edit() {
    same_source_control_edit_keeps_selected_capture(Edit::Bpm, true);
}

#[test]
fn selected_keyboard_pitch_keeps_source_after_grid_origin_edit() {
    same_source_control_edit_keeps_selected_capture(Edit::GridOrigin, false);
}

#[test]
fn selected_midi_pitch_keeps_source_after_grid_origin_edit() {
    same_source_control_edit_keeps_selected_capture(Edit::GridOrigin, true);
}

fn replacement() -> SampleBuffer {
    SampleBuffer {
        channels: 1,
        samples: (0..144_000)
            .map(|frame| {
                (0.2 * (frame as f64 * 257.0 * std::f64::consts::TAU / 48_000.0).sin()) as f32
            })
            .collect::<Vec<_>>()
            .into(),
        residency: None,
    }
    .with_complete_source(48_000)
    .window(LOOP_START, LOOP_END, 2, ResidentContext::KeyLockFiniteLoop)
    .unwrap()
}

fn adopt_replacement_source(fixture: &mut Fixture) {
    let engine = &fixture.control.engine;
    let ownership = engine.input_runtime_ownership.clone();
    let placement = ownership.slot_snapshot(0).unwrap();
    let sample = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let (generation, rate) = engine.loaded_source_generations.lock().unwrap()[0];
    let write = ownership
        .prepare_slot_write_fixed(placement, placement.lifetime())
        .unwrap();
    let publication = ownership
        .publish_source_write(
            &write,
            &sample,
            rate,
            generation,
            ownership.authority_revision(0),
            None,
            false,
            || true,
        )
        .unwrap();
    fixture
        .control
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::LoadSample {
            id: 0,
            sample: sample.clone(),
            publication: Some(publication),
        })
        .unwrap();
    fixture.drain();
    fixture.mixer.set_pad_loop_region(0, 0.1, Some(2.1));
    fixture.mixer.set_pad_key_lock(0, true);
    // Same-key overwrites still require a new complete source adoption. A matching
    // historical ContentKey SourceACK alone cannot prove replacement PCM custody.
    let adopted = fixture
        .mixer
        .capture_pitch_start_snapshot(0, LOOP_START)
        .unwrap();
    assert!(adopted.sample.same_source(&sample));
    assert_eq!(ownership.callback_feedback_key(0), placement.content_key());
}

#[test]
fn selected_source_never_follows_same_cell_overwrite_or_new_content_without_selection() {
    for (refill, reuse_pcm) in [(false, false), (true, false), (false, true)] {
        let mut fixture = fixture();
        let selection = fixture.control.engine.input_selected_projection().unwrap();
        let original = fixture
            .control
            .engine
            .capture_selected_pitch_action(ACTION, None)
            .unwrap();
        let old_key = original.content_key();
        let old_source = original.control_guard(None).unwrap().binding.source_address;
        let old_generation = fixture
            .control
            .engine
            .loaded_source_generations
            .lock()
            .unwrap()[0]
            .0;
        let sample = if reuse_pcm {
            fixture.control.engine.sample_cache.lock().unwrap()[0]
                .clone()
                .unwrap()
        } else {
            replacement()
        };
        publish_tone(&fixture.control, 0, sample, refill);
        adopt_replacement_source(&mut fixture);
        refresh_rows(&fixture, None);
        fixture.refresh_clock();

        let explicit = fixture
            .control
            .engine
            .capture_pad_pitch_action(0, ACTION, None)
            .unwrap();
        assert_eq!(
            explicit.control_guard(None).unwrap().binding.source_address == old_source,
            reuse_pcm
        );
        assert!(
            fixture
                .control
                .engine
                .loaded_source_generations
                .lock()
                .unwrap()[0]
                .0
                > old_generation
        );
        assert_eq!(explicit.content_key() == old_key, !refill);
        assert!(original.control_guard(None).is_err());
        assert!(
            fixture
                .control
                .engine
                .capture_selected_pitch_action(ACTION, None)
                .is_err()
        );
        assert_eq!(
            fixture.control.engine.input_selected_projection().unwrap(),
            (selection.0, None)
        );
        let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
        assert!(runtime.inject_midi_message(&[0x90, 60, 100]));
        let event = next_event(&fixture);
        assert!(event.captured_pitch.is_none());
        assert_eq!(event.pitch_rejection, Some(PitchActionReject::StaleContent));

        let revision = fixture.control.engine.select_input_pad(0).unwrap();
        assert!(revision > selection.0);
        let selected = fixture
            .control
            .engine
            .capture_selected_pitch_action(ACTION, None)
            .unwrap();
        assert_eq!(selected.content_key(), explicit.content_key());
        assert_eq!(
            selected.control_guard(None).unwrap().binding.source_address,
            explicit.control_guard(None).unwrap().binding.source_address
        );
    }
}

#[test]
fn disabled_and_learn_midi_do_not_mutate_selection_or_recapture_retired_owners() {
    for learn in [false, true] {
        let fixture = fixture();
        let selection = fixture.control.engine.input_selected_projection().unwrap();
        let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
        assert!(runtime.inject_midi_message(&[0x90, 60, 100]));
        let captured = next_event(&fixture).captured_pitch.unwrap();
        assert!(captured.control_guard(None).is_ok());
        if learn {
            runtime.set_learn_capture_active(true);
        } else {
            runtime.set_enabled(false);
        }
        assert!(captured.control_guard(None).is_err());
        assert!(runtime.inject_midi_message(&[0x90, 60, 100]));
        let event = next_event(&fixture);
        assert!(event.action_key.is_none());
        assert!(event.captured_pitch.is_none());
        assert!(event.selection_revision.is_none());
        assert_eq!(
            fixture.control.engine.input_selected_projection().unwrap(),
            selection
        );
        assert_eq!(
            fixture
                .control
                .engine
                .input_runtime_ownership
                .endpoint_selection(),
            Some((selection.0, Some(captured.content_key())))
        );
    }
}
