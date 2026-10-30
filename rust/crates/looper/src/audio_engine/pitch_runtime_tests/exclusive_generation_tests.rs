//! Public pitch capture/publication at the exclusive retrigger generation boundary.
//! Only the counter is placed near exhaustion; sources, admission and execution are real.

use super::*;
use crate::audio_engine::dsp::PerPadDspChain;
use crate::audio_engine::ordinary_endpoint_reserve_tests::{
    adopt_current_sources, refresh_actual_runtime_bindings,
};
use pyo3::prelude::*;
use pyo3::types::PyInt;

fn capture_at_current_frame(fixture: &Fixture, id: usize) -> CapturedPitchAction {
    Python::initialize();
    Python::attach(|py| {
        fixture.refresh_clock();
        let timestamp = PyInt::new(py, fixture.clock.read().unwrap().audible_at_ns);
        let action = fixture
            .control
            .engine
            .capture_pad_pitch_action(id, "pitch.extra:0:set_retrigger", Some(timestamp.as_any()))
            .unwrap();
        assert_eq!(
            action.original_due_frame(),
            fixture.transport.output_frame()
        );
        action
    })
}

fn publish_at_current_frame(fixture: &mut Fixture, action: &CapturedPitchAction) {
    fixture.publish(action);
    assert_eq!(action.execution_target(), action.original_due_frame());
    assert_eq!(action.execution_target(), fixture.transport.output_frame());
}

fn active_fixture(exclusive: bool) -> Fixture {
    let mut fixture = Fixture::new(false);
    adopt_current_sources(&mut fixture, false);
    refresh_actual_runtime_bindings(&fixture);
    // Settle the real ON->OFF collapse while silent. These independent ordinary
    // setup births explicitly request nonexclusive intent; the Piano action
    // under test then captures the already settled runtime policy.
    set_multi_loop(&fixture, !exclusive);
    fixture.drain();
    fixture.render(0);
    assert!(fixture.scheduler.is_empty());
    assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));
    for id in [0, 1] {
        fixture.refresh_clock();
        let action = fixture
            .control
            .engine
            .capture_pad_start(
                id,
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap();
        fixture.drain();
        assert!(!action.exclusive());
        assert!(
            fixture
                .control
                .engine
                .publish_pad_start(&action, None)
                .unwrap()
        );
        fixture.drain();
        fixture.render(1);
        assert_eq!(action.state(), "applied", "reason={:?}", action.reason());
    }
    assert!(fixture.mixer.voices[0].active);
    assert_eq!(fixture.mixer.voices[0].sample_id, 0);
    assert!(fixture.mixer.voices[1].active);
    assert_eq!(fixture.mixer.voices[1].sample_id, 1);
    fixture.mixer.set_pad_eq(0, -18.0, 4.0, -6.0);
    fixture.mixer.set_pad_eq(1, 3.0, -12.0, 2.0);
    fixture.pcm(1_024);
    fixture
}

fn set_multi_loop(fixture: &Fixture, multi_loop: bool) {
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    for id in [0, 1] {
        loaded[id] = true;
        starts[id] = LOOP_START as f64 / f64::from(RATE);
        ends[id] = Some(LOOP_END as f64 / f64::from(RATE));
        bindings[id] = input_runtime_binding::capture(&fixture.control.engine, id).unwrap();
    }
    fixture
        .control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state(
            multi_loop,
            loaded,
            starts,
            ends,
            bindings.iter().map(Option::as_ref).collect(),
        )
        .unwrap();
}

fn set_selected_generation(fixture: &mut Fixture, generation: u64) {
    let voice = &mut fixture.mixer.voices[0];
    assert!(voice.active && voice.sample_id == 0);
    voice.generation = generation;
    voice.history_lifetime.store(generation, Ordering::Release);
}

fn filter_probe(mut chain: PerPadDspChain) -> Vec<f32> {
    (0..73)
        .map(|frame| {
            chain.begin_frame();
            chain.process_sample(0, if frame == 0 { 0.37 } else { 0.0 })
        })
        .collect()
}

fn assert_unchanged_audio_state(actual: &Fixture, baseline: &Fixture) {
    for id in [0, 1] {
        let voice = actual.voice(id);
        let expected = baseline.voice(id);
        assert_eq!(voice.generation, expected.generation);
        assert_eq!(
            voice.history_lifetime.load(Ordering::Acquire),
            expected.history_lifetime.load(Ordering::Acquire),
        );
        assert!(
            voice
                .source_playback
                .matches_exact(&expected.source_playback)
        );
        assert_eq!(voice.paused, expected.paused);
        assert_eq!(voice.volume, expected.volume);
        assert_eq!(
            actual.mixer.loop_region_frames(id),
            baseline.mixer.loop_region_frames(id)
        );
        assert_eq!(
            actual.mixer.pad_pitch_intent(id),
            baseline.mixer.pad_pitch_intent(id)
        );
        let chain = actual.mixer.dsp_chain_snapshot_for_test(id).unwrap();
        let expected_chain = baseline.mixer.dsp_chain_snapshot_for_test(id).unwrap();
        assert_eq!(
            chain.loop_acceptance_parameters(),
            expected_chain.loop_acceptance_parameters(),
        );
        assert_eq!(filter_probe(chain), filter_probe(expected_chain));
        assert_eq!(actual.starts(id), baseline.starts(id));
    }
    assert_eq!(actual.feedback.len(), baseline.feedback.len());
}

#[test]
fn exclusive_retrigger_at_max_minus_one_rejects_before_any_stop_start_or_eq_reset() {
    let mut hit = active_fixture(true);
    let mut baseline = active_fixture(true);
    for fixture in [&mut hit, &mut baseline] {
        set_selected_generation(fixture, u64::MAX - 1);
    }
    let other_generation = hit.voice(1).generation;
    let action = capture_at_current_frame(&hit, 0);
    assert!(action.exclusive());
    publish_at_current_frame(&mut hit, &action);
    let actual_first = hit.render(1);
    let expected_first = baseline.render(1);

    assert_eq!(action.reason(), Some("sequence_exhausted"));
    assert_eq!(action.state(), "rejected");
    assert!(!action.effective());
    assert!(hit.scheduler.is_empty());
    assert_eq!(hit.voice(0).generation, u64::MAX - 1);
    assert_eq!(hit.voice(1).generation, other_generation);
    assert_eq!(actual_first, expected_first);
    assert_unchanged_audio_state(&hit, &baseline);
    assert_eq!(hit.pcm(2_048), baseline.pcm(2_048));
    assert_unchanged_audio_state(&hit, &baseline);
}

#[test]
fn exclusive_retrigger_rejects_when_another_active_pad_cannot_be_stopped() {
    let mut hit = active_fixture(true);
    let mut baseline = active_fixture(true);
    for fixture in [&mut hit, &mut baseline] {
        let peer = &mut fixture.mixer.voices[1];
        assert!(peer.active && peer.sample_id == 1);
        peer.generation = u64::MAX;
        peer.history_lifetime.store(u64::MAX, Ordering::Release);
    }
    let action = capture_at_current_frame(&hit, 0);
    assert!(action.exclusive());
    publish_at_current_frame(&mut hit, &action);
    assert_eq!(hit.render(1), baseline.render(1));
    assert_eq!(action.reason(), Some("sequence_exhausted"));
    assert_eq!(action.state(), "rejected");
    assert!(!action.effective());
    assert!(hit.scheduler.is_empty());
    assert_unchanged_audio_state(&hit, &baseline);
    assert_eq!(hit.pcm(2_048), baseline.pcm(2_048));
    assert_unchanged_audio_state(&hit, &baseline);
}

#[test]
fn exclusive_retrigger_at_max_minus_two_can_consume_both_generations_once() {
    let mut hit = active_fixture(true);
    let mut baseline = active_fixture(true);
    set_selected_generation(&mut hit, u64::MAX - 2);
    let other_generation = hit.voice(1).generation;
    let action = capture_at_current_frame(&hit, 0);
    let expected = capture_at_current_frame(&baseline, 0);
    assert!(action.exclusive() && expected.exclusive());
    publish_at_current_frame(&mut hit, &action);
    publish_at_current_frame(&mut baseline, &expected);
    assert_eq!(hit.render(1), baseline.render(1));

    assert!(action.effective(), "reason={:?}", action.reason());
    assert!(expected.effective(), "reason={:?}", expected.reason());
    assert_eq!(hit.voice(0).generation, u64::MAX);
    assert_eq!(
        hit.voice(0).history_lifetime.load(Ordering::Acquire),
        u64::MAX
    );
    assert!(!hit.mixer.voices[1].active);
    assert_eq!(hit.mixer.voices[1].generation, other_generation + 1);
    assert_eq!(hit.starts(0), 2);
    assert_eq!(hit.starts(1), 1);
    assert!(hit.scheduler.is_empty());
    let pcm = hit.pcm(2_048);
    assert!(pcm.iter().any(|sample| sample.abs() > 1.0e-4));
    assert_eq!(pcm, baseline.pcm(2_048));
}

#[test]
fn nonexclusive_retrigger_at_max_minus_one_consumes_only_one_and_keeps_other_pad() {
    let mut hit = active_fixture(false);
    let mut baseline = active_fixture(false);
    set_selected_generation(&mut hit, u64::MAX - 1);
    let other_generation = hit.voice(1).generation;
    let action = capture_at_current_frame(&hit, 0);
    let expected = capture_at_current_frame(&baseline, 0);
    assert!(!action.exclusive() && !expected.exclusive());
    publish_at_current_frame(&mut hit, &action);
    publish_at_current_frame(&mut baseline, &expected);
    assert_eq!(hit.render(1), baseline.render(1));

    assert!(action.effective(), "reason={:?}", action.reason());
    assert!(expected.effective(), "reason={:?}", expected.reason());
    assert_eq!(hit.voice(0).generation, u64::MAX);
    assert_eq!(
        hit.voice(0).history_lifetime.load(Ordering::Acquire),
        u64::MAX
    );
    assert_eq!(hit.voice(1).generation, other_generation);
    assert!(
        hit.voice(1)
            .source_playback
            .matches_exact(&baseline.voice(1).source_playback)
    );
    assert_eq!(hit.starts(0), 2);
    assert_eq!(hit.starts(1), 1);
    assert!(hit.scheduler.is_empty());
    assert_eq!(hit.pcm(2_048), baseline.pcm(2_048));
    // Isolate the unaffected pad's actual PCM after the successful new attack.
    for fixture in [&mut hit, &mut baseline] {
        fixture
            .mixer
            .pause_sample_at_output_frame(0, fixture.transport.output_frame());
    }
    assert_eq!(hit.pcm(2_048), baseline.pcm(2_048));
    assert_eq!(hit.voice(1).generation, other_generation);
}
