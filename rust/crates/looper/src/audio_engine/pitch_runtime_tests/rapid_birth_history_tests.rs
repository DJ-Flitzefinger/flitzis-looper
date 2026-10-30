//! A genuine earlier birth must not erase a later delivered independent piano launch.
//! The reference really emits from T with its own GUI capture, source, Native and EQ.
#![cfg(windows)]

use super::*;
use crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::WorkerGate;

const RATE_RATIO: f64 = 1.43;
const ORIGINAL_T: u64 = 36_000;
const EARLY_CAPTURE: u64 = ORIGINAL_T - 2_048;
const PREPARATION_LEAD: u64 = 8_192;
const EMITTED_FRAMES: usize = 12_289;
const FIRST_NOTE: i32 = 7;
const EQ_DB: [f32; 3] = [-12.0, -3.0, 4.0];

fn render_to(route: &mut MouseRoute, frame: u64) {
    while route.transport.output_frame() < frame {
        route.render((frame - route.transport.output_frame()).min(512) as usize);
    }
    assert_eq!(route.transport.output_frame(), frame);
}

fn render_partitioned(route: &mut MouseRoute, frames: usize, chunks: &[usize]) -> Vec<f32> {
    let mut pcm = Vec::with_capacity(frames);
    let mut callback = 0;
    while pcm.len() < frames {
        pcm.extend(route.render(chunks[callback % chunks.len()].min(frames - pcm.len())));
        callback += 1;
    }
    pcm
}

fn voice(route: &MouseRoute, id: usize) -> &crate::audio_engine::voice_slot::VoiceSlot {
    route
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == id)
        .unwrap()
}

fn configured(policy: &str) -> MouseRoute {
    configured_with_peer_volume(policy, 0.3)
}

fn configured_with_peer_volume(policy: &str, peer_volume: f32) -> MouseRoute {
    let mut fixture = Fixture::new(false);
    fixture.mixer.set_speed(RATE_RATIO);
    fixture.mixer.set_pad_gain(0, -4.5);
    fixture.mixer.set_pad_eq(0, EQ_DB[0], EQ_DB[1], EQ_DB[2]);
    fixture.drain();
    assert_eq!(fixture.render(1), vec![0.0]);
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior(policy)
            .unwrap()
    );
    // The original GUI actions request KEY LOCK ON. Their separate frozen tuples
    // own its complete Native effect; the predecessor still plays until then.
    let mut route = MouseRoute::from_fixture(fixture, true, None);
    assert!(route.mixer.play_sample(0, 1.0));
    assert!(route.mixer.play_sample(1, peer_volume));
    render_to(&mut route, 24_000);
    assert_eq!(voice(&route, 0).source_playback.tempo_ratio(), RATE_RATIO);
    assert_eq!(voice(&route, 0).source_playback.rate_target(), RATE_RATIO);
    route
}

// The independent primary reference emits at its own on-time T. Its audible
// peer must instead retain the actual absolute output coordinate E, so render
// that separate source owner to E and sum in the production voice order.
fn independent_peer_pcm(execution: u64) -> Vec<f32> {
    let mut fixture = Fixture::new(false);
    fixture.mixer.set_speed(RATE_RATIO);
    assert_eq!(fixture.render(1), vec![0.0]);
    assert!(fixture.mixer.play_sample(1, 0.3));
    fixture.render_to(execution);
    let pcm = fixture.pcm(EMITTED_FRAMES);
    assert!(pcm.iter().any(|sample| sample.abs() > 0.001));
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    pcm
}

fn wait_ready(route: &mut MouseRoute, action: &CapturedPitchAction) {
    let frame = route.transport.output_frame();
    let deadline = Instant::now() + WORKER_WAIT;
    loop {
        route.mixer.pitch_runtime.as_mut().unwrap().poll();
        if route
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_ready_action(action.sequence())
        {
            break;
        }
        assert_ne!(action.state(), "rejected", "{:?}", action.reason());
        assert!(
            Instant::now() < deadline,
            "independent burst preparation exceeded host bound"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(route.transport.output_frame(), frame);
}

fn birth_between_capture_and_delivery(policy: &str, birth_after_t: u64, second_note: i32) {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(birth_after_t <= 1);

    // An independent on-time launch supplies its complete emitted beginning.
    // Neither its phase nor its Native comes from the delayed birth.
    let mut reference = configured_with_peer_volume(policy, 0.0);
    render_to(&mut reference, EARLY_CAPTURE);
    Python::attach(|py| {
        let project = reference.probe.bind(py).getattr("project").unwrap();
        project
            .setattr("trigger_quantization_enabled", true)
            .unwrap();
        project
            .setattr("trigger_quantization_step", "1_16")
            .unwrap();
    });
    let reference_capture = reference.capture("click", Some(second_note));
    let reference_action = Python::attach(|py| reference_capture.borrow(py).clone());
    assert_eq!(reference_action.original_due_frame(), ORIGINAL_T);
    reference.drain();
    assert!(
        reference_action.accepted(),
        "{:?}",
        reference_action.reason()
    );
    assert_eq!(reference_action.execution_target(), ORIGINAL_T);
    wait_ready(&mut reference, &reference_action);
    render_to(&mut reference, ORIGINAL_T);

    let mut burst = configured(policy);
    let first_effect = ORIGINAL_T + birth_after_t;
    let first_target = first_effect - PREPARATION_LEAD - 1;
    render_to(&mut burst, first_target);
    let first_capture = burst.capture("click", Some(FIRST_NOTE));
    let first = Python::attach(|py| first_capture.borrow(py).clone());
    assert_eq!(first.original_due_frame(), first_target);
    burst.render(1);
    burst.drain();
    assert!(first.accepted(), "{:?}", first.reason());
    assert_eq!(first.execution_target(), first_effect);
    wait_ready(&mut burst, &first);
    render_to(&mut burst, ORIGINAL_T);
    assert!(!first.effective());

    let second_capture = burst.capture("click", Some(second_note));
    let second = Python::attach(|py| second_capture.borrow(py).clone());
    let original = (
        second.sequence(),
        second.content_key(),
        second.received_timestamp(),
        second.original_due_frame(),
    );
    assert_eq!(second.original_due_frame(), ORIGINAL_T);
    assert_eq!(
        second.render().pitch,
        PitchIntent::new(second_note, 0).unwrap()
    );
    assert!(second.render().key_lock);
    assert!(second.sequence() > first.sequence());
    // Actual Native/scheduler work starts A while B's original GUI message remains
    // queued. This crosses a birth, with no rate/loop/source/EQ/control change.
    burst.render((first_effect - ORIGINAL_T + 1) as usize);
    assert!(first.effective(), "{:?}", first.reason());
    assert_eq!(burst.starts(0), 1);
    assert_eq!(burst.mixer.pad_pitch_intent(0), Some(first.render().pitch));
    let first_native = voice(&burst, 0).stretch.native_state_address();
    let first_generation = voice(&burst, 0).generation;
    let peer_generation = voice(&burst, 1).generation;
    let mut peer_playback = voice(&burst, 1).source_playback;
    let delivery = burst.transport.output_frame();
    burst.drain();
    assert!(
        second.accepted(),
        "an independent birth erased B's proved past: policy={policy} A.E={first_effect} B.T={ORIGINAL_T} B.F={delivery} notes={FIRST_NOTE}/{second_note} state={} reason={:?}",
        second.state(),
        second.reason()
    );
    let execution = delivery + PREPARATION_LEAD;
    assert_eq!(second.execution_target(), execution);
    wait_ready(&mut burst, &second);
    let second_native = burst
        .mixer
        .pitch_runtime
        .as_ref()
        .unwrap()
        .ready_pitch_native_address(second.sequence())
        .unwrap();
    assert_ne!(second_native, first_native);
    render_to(&mut burst, execution);
    assert!(!second.effective());
    assert_eq!(voice(&burst, 0).generation, first_generation);
    assert_eq!(
        voice(&burst, 0).stretch.native_state_address(),
        first_native
    );
    assert_eq!(burst.mixer.pad_pitch_intent(0), Some(first.render().pitch));

    let primary_expected =
        render_partitioned(&mut reference, EMITTED_FRAMES, &[127, 1, 509, 31, 256]);
    assert!(primary_expected.iter().any(|sample| sample.abs() > 0.001));
    let peer_expected = independent_peer_pcm(execution);
    assert_eq!(primary_expected.len(), peer_expected.len());
    let expected: Vec<_> = primary_expected
        .iter()
        .zip(&peer_expected)
        .map(|(primary, peer)| primary + peer)
        .collect();
    let actual = render_partitioned(&mut burst, EMITTED_FRAMES, &[257, 17, 1, 383, 96]);
    let expected = expected.as_slice();
    assert!(expected.iter().any(|sample| sample.abs() > 0.001));
    assert!(actual.iter().all(|sample| sample.is_finite()));
    let max_error = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert_eq!(
        actual.iter().zip(expected).position(|(a, b)| a != b),
        None,
        "independent complete Native/EQ beginning differs: policy={policy} boundary={birth_after_t} note={second_note} max={max_error}"
    );
    assert!(second.effective(), "{:?}", second.reason());
    assert!(
        reference_action.effective(),
        "{:?}",
        reference_action.reason()
    );
    assert_eq!(burst.starts(0), 2);
    assert_eq!(reference.starts(0), 1);
    assert_eq!(burst.starts(1), 0);
    assert_eq!(reference.starts(1), 0);
    let selected = voice(&burst, 0);
    assert_eq!(selected.generation, first_generation + 1);
    assert_eq!(selected.stretch.native_state_address(), second_native);
    assert_eq!(
        selected.stretch.adopted_request_id(),
        Some(second.sequence())
    );
    assert!(
        selected
            .stretch
            .owns_pitch_contract(second.render().pitch, true)
    );
    assert_eq!(selected.source_playback.tempo_ratio(), RATE_RATIO);
    assert_eq!(selected.source_playback.rate_target(), RATE_RATIO);
    assert_eq!(
        selected.stretch.native_primed_output_frames(),
        voice(&reference, 0).stretch.native_primed_output_frames()
    );
    assert_eq!(
        selected.stretch.pending_fifo_frames(),
        voice(&reference, 0).stretch.pending_fifo_frames()
    );
    let phase = LOOP_START as f64 + EMITTED_FRAMES as f64 * RATE_RATIO;
    let actual_phase = selected.source_playback.position();
    assert_eq!(actual_phase.frame, phase.floor() as usize);
    assert!((actual_phase.fraction - phase.fract()).abs() < 1.0e-8);
    assert!(peer_playback.advance((execution - delivery) as usize + EMITTED_FRAMES));
    assert!(peer_playback.matches_exact(&voice(&burst, 1).source_playback));
    assert_eq!(voice(&burst, 1).generation, peer_generation);
    assert_eq!(
        (
            second.sequence(),
            second.content_key(),
            second.received_timestamp(),
            second.original_due_frame()
        ),
        original
    );
    assert_eq!(second.execution_target(), execution);
    assert!(burst.scheduler.is_empty());
    assert!(reference.scheduler.is_empty());
    burst.close();
    reference.close();
}

#[test]
fn rapid_birth_first_sound_before_different_note_retains_original_pcm() {
    birth_between_capture_and_delivery("first_sound_at_grid", 1, -5);
}

#[test]
fn rapid_birth_first_sound_at_different_note_retains_original_pcm() {
    birth_between_capture_and_delivery("first_sound_at_grid", 0, -5);
}

#[test]
fn rapid_birth_first_sound_before_equal_note_retains_both_attacks() {
    birth_between_capture_and_delivery("first_sound_at_grid", 1, FIRST_NOTE);
}

#[test]
fn rapid_birth_first_sound_at_equal_note_retains_both_attacks() {
    birth_between_capture_and_delivery("first_sound_at_grid", 0, FIRST_NOTE);
}

#[test]
fn rapid_birth_attack_before_different_note_retains_original_pcm() {
    birth_between_capture_and_delivery("attack_at_grid", 1, -5);
}

#[test]
fn rapid_birth_attack_at_different_note_retains_original_pcm() {
    birth_between_capture_and_delivery("attack_at_grid", 0, -5);
}

#[test]
fn rapid_birth_attack_before_equal_note_retains_both_attacks() {
    birth_between_capture_and_delivery("attack_at_grid", 1, FIRST_NOTE);
}

#[test]
fn rapid_birth_attack_at_equal_note_retains_both_attacks() {
    birth_between_capture_and_delivery("attack_at_grid", 0, FIRST_NOTE);
}

fn ordinary_birth_between_piano_capture_and_delivery(policy: &str, birth_after_t: u64) {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    const PIANO_T: u64 = 48_000;
    let mut route = configured(policy);
    // Establish the effective KEY LOCK/pitch tuple using an original GUI action;
    // the intervening ordinary start must inherit that confirmed native tuple.
    let setup_capture = route.capture("click", Some(FIRST_NOTE));
    let setup = Python::attach(|py| setup_capture.borrow(py).clone());
    route.drain();
    assert!(setup.accepted(), "{:?}", setup.reason());
    wait_ready(&mut route, &setup);
    render_to(&mut route, setup.execution_target());
    route.render(1);
    assert!(setup.effective(), "{:?}", setup.reason());
    assert_eq!(route.starts(0), 1);

    let ordinary_effect = PIANO_T + birth_after_t;
    let ordinary_target = ordinary_effect - PREPARATION_LEAD - 1;
    render_to(&mut route, ordinary_target - 1);
    route.refresh_clock();
    let ordinary = Python::attach(|py| {
        route
            .engine
            .borrow(py)
            .capture_pad_start(
                0,
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap()
    });
    let gate = WorkerGate::for_ordinary(&ordinary);
    route.render(1);
    route.drain();
    // Ordinary target capture freezes the callback's original coordinate here;
    // unlike the piano capture, its ControlMessage has not frozen T earlier.
    assert_eq!(ordinary.original_frame(), Some(ordinary_target));
    // Keep publication genuinely late relative to that already frozen T. This
    // prevents an endpoint hit while preserving the declared A.E and B.T cases.
    route.render(1);
    assert!(Python::attach(|py| {
        route
            .engine
            .borrow(py)
            .publish_pad_start(&ordinary, None)
            .unwrap()
    }));
    route.drain();
    assert!(ordinary.accepted(), "{:?}", ordinary.reason());
    assert_eq!(ordinary.execution_frame(), Some(ordinary_effect));
    gate.wait_for(false);
    // Keep the declared A.E==B.T / B.T+1 race real despite completion-driven
    // scheduling: the ordinary worker cannot finish before that boundary.
    render_to(&mut route, PIANO_T);
    let captured = route.capture("click", Some(-5));
    let piano = Python::attach(|py| captured.borrow(py).clone());
    let original = (
        piano.sequence(),
        piano.received_timestamp(),
        piano.original_due_frame(),
    );
    assert_eq!(piano.original_due_frame(), PIANO_T);
    if birth_after_t != 0 {
        route.render(birth_after_t as usize);
    }
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    let deadline = Instant::now() + WORKER_WAIT;
    loop {
        route.mixer.pitch_runtime.as_mut().unwrap().poll();
        if route
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_ready_action(ordinary.sequence())
        {
            break;
        }
        assert_ne!(ordinary.state(), "rejected", "{:?}", ordinary.reason());
        assert!(
            Instant::now() < deadline,
            "ordinary mixed-birth preparation exceeded host bound"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    route.render(1);
    assert_eq!(ordinary.state(), "applied", "{:?}", ordinary.reason());
    assert_eq!(ordinary.effective_frame(), Some(ordinary_effect));
    assert_eq!(route.starts(0), 2);
    let ordinary_native = voice(&route, 0).stretch.native_state_address();
    let generation = voice(&route, 0).generation;
    let peer_generation = voice(&route, 1).generation;
    route.drain();
    assert!(
        piano.accepted(),
        "ordinary birth erased original piano: {policy} boundary={birth_after_t} {:?}",
        piano.reason()
    );
    let execution = route.transport.output_frame() + PREPARATION_LEAD;
    assert_eq!(piano.execution_target(), execution);
    wait_ready(&mut route, &piano);
    let piano_native = route
        .mixer
        .pitch_runtime
        .as_ref()
        .unwrap()
        .ready_pitch_native_address(piano.sequence())
        .unwrap();
    assert_ne!(piano_native, ordinary_native);
    render_to(&mut route, execution);
    assert_eq!(voice(&route, 0).generation, generation);
    assert_eq!(
        voice(&route, 0).stretch.native_state_address(),
        ordinary_native
    );
    let pcm = render_partitioned(&mut route, EMITTED_FRAMES, &[1, 127, 509, 31, 257]);
    assert!(piano.effective(), "{:?}", piano.reason());
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(route.starts(0), 3);
    assert_eq!(route.starts(1), 0);
    assert_eq!(voice(&route, 0).generation, generation + 1);
    assert_eq!(
        voice(&route, 0).stretch.native_state_address(),
        piano_native
    );
    assert_eq!(
        voice(&route, 0).stretch.adopted_request_id(),
        Some(piano.sequence())
    );
    assert!(
        voice(&route, 0)
            .stretch
            .owns_pitch_contract(PitchIntent::new(-5, 0).unwrap(), true)
    );
    assert_eq!(voice(&route, 0).source_playback.tempo_ratio(), RATE_RATIO);
    assert_eq!(voice(&route, 1).generation, peer_generation);
    assert_eq!(
        (
            piano.sequence(),
            piano.received_timestamp(),
            piano.original_due_frame()
        ),
        original
    );
    assert_eq!(piano.execution_target(), execution);
    assert!(route.scheduler.is_empty());
    route.close();
}

#[test]
fn rapid_birth_ordinary_first_sound_retains_late_piano_original() {
    for boundary in [0, 1] {
        ordinary_birth_between_piano_capture_and_delivery("first_sound_at_grid", boundary);
    }
}

#[test]
fn rapid_birth_ordinary_attack_retains_late_piano_original() {
    for boundary in [0, 1] {
        ordinary_birth_between_piano_capture_and_delivery("attack_at_grid", boundary);
    }
}
