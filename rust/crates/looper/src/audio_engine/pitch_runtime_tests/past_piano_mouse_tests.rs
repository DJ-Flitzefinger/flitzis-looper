//! Actual ImGui/UiContext piano capture before the next Native callback control drain.
#![cfg(windows)]

use super::*;

const TARGET: u64 = 24_000;
const PREPARATION_LEAD: u64 = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PriorPlayback {
    Running,
    Paused,
    Stopped,
}

fn render_to(route: &mut MouseRoute, frame: u64) {
    while route.transport.output_frame() < frame {
        let count = (frame - route.transport.output_frame()).min(512) as usize;
        route.render(count);
    }
    assert_eq!(route.transport.output_frame(), frame);
}

fn voice(route: &MouseRoute, slot: usize) -> &crate::audio_engine::voice_slot::VoiceSlot {
    route
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == slot)
        .unwrap()
}

fn configured(rate: f64, policy: &str, prior: PriorPlayback) -> MouseRoute {
    let mut fixture = Fixture::new(false);
    fixture.mixer.set_speed(rate);
    fixture.drain();
    assert_eq!(fixture.render(1), vec![0.0]);
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior(policy)
            .unwrap()
    );
    // The UI requests Key Lock ON; the actual old voices keep their dry source tuple
    // until this original captured piano action has its prepared effect.
    let mut route = MouseRoute::from_fixture(fixture, true, None);
    if prior != PriorPlayback::Stopped {
        assert!(route.mixer.play_sample(0, 1.0));
    }
    // A second loaded and audible pad proves that the selected attack remains local.
    assert!(route.mixer.play_sample(1, 0.3));
    if prior == PriorPlayback::Paused {
        render_to(&mut route, TARGET - 512);
        route.mixer.pause_sample_at_output_frame(0, TARGET - 512);
        assert!(voice(&route, 0).paused);
    }
    render_to(&mut route, TARGET);
    route
}

fn late_mouse_case(rate: f64, policy: &str, prior: PriorPlayback) {
    let mut route = configured(rate, policy, prior);
    let other_generation = voice(&route, 1).generation;
    let mut other_playback = voice(&route, 1).source_playback;
    let predecessor = (prior != PriorPlayback::Stopped).then(|| {
        let old = voice(&route, 0);
        (
            old.generation,
            old.stretch.native_state_address(),
            old.paused,
        )
    });
    let captured = route.capture("click", Some(7));
    let action = Python::attach(|py| captured.borrow(py).clone());
    let original = (
        action.sequence(),
        action.content_key(),
        action.received_timestamp(),
        action.original_due_frame(),
    );
    assert_eq!(action.original_due_frame(), TARGET);
    assert_eq!(action.variant(), PitchActionVariant::SetRetrigger);
    assert_eq!(action.render().pitch, PitchIntent::new(7, 0).unwrap());
    assert!(!action.accepted());
    route.render(1);
    assert_eq!(route.transport.output_frame(), TARGET + 1);
    route.drain();
    assert!(
        action.accepted(),
        "actual late GUI piano must remain usable: prior={prior:?} rate={rate} policy={policy} state={} reason={:?}",
        action.state(),
        action.reason()
    );
    let execution = TARGET + 1 + PREPARATION_LEAD;
    assert_eq!(action.execution_target(), execution);
    let waiting_frame = route.transport.output_frame();
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
            "late GUI Native worker host wait exceeded bound"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(route.transport.output_frame(), waiting_frame);
    let prepared_native = route
        .mixer
        .pitch_runtime
        .as_ref()
        .unwrap()
        .ready_pitch_native_address(action.sequence())
        .unwrap();
    render_to(&mut route, execution);
    assert!(!action.effective());
    assert_eq!(route.starts(0), 0);
    if let Some((generation, native, paused)) = predecessor {
        let old = voice(&route, 0);
        assert_eq!(
            (
                old.generation,
                old.stretch.native_state_address(),
                old.paused
            ),
            (generation, native, paused)
        );
    }
    let mut initial_pcm = route.render(1);
    initial_pcm.extend(route.render(511));
    assert!(action.effective(), "{:?}", action.reason());
    assert_eq!(action.state(), "applied");
    assert_eq!(route.starts(0), 1);
    let new = voice(&route, 0);
    assert!(!new.paused);
    assert_eq!(new.stretch.native_state_address(), prepared_native);
    assert_eq!(new.stretch.adopted_request_id(), Some(action.sequence()));
    assert!(new.stretch.owns_pitch_contract(action.render().pitch, true));
    assert_eq!(new.source_playback.tempo_ratio(), rate);
    assert_eq!(new.source_playback.rate_target(), rate);
    let source_frames = 512.0 * rate;
    let expected_source = LOOP_START as f64 + source_frames;
    let position = new.source_playback.position();
    assert_eq!(position.frame, expected_source.floor() as usize);
    assert!((position.fraction - expected_source.fract()).abs() < 1.0e-8);
    assert!(initial_pcm.iter().all(|sample| sample.is_finite()));
    assert!(other_playback.advance((execution - TARGET) as usize + 512));
    assert!(other_playback.matches_exact(&voice(&route, 1).source_playback));
    assert_eq!(voice(&route, 1).generation, other_generation);
    assert_eq!(route.starts(1), 0);
    assert_eq!(
        route.mixer.pad_pitch_intent(1),
        Some(PitchIntent::default())
    );
    assert_eq!(
        (
            action.sequence(),
            action.content_key(),
            action.received_timestamp(),
            action.original_due_frame(),
        ),
        original
    );
    assert_eq!(action.execution_target(), execution);
    Python::attach(|py| {
        route
            .probe
            .call_method1(py, "applied", (captured.clone_ref(py),))
            .unwrap();
        let other = route
            .probe
            .bind(py)
            .getattr("project")
            .unwrap()
            .getattr("pad_key_intent")
            .unwrap()
            .get_item(1)
            .unwrap();
        assert_eq!(
            other
                .getattr("base_shift")
                .unwrap()
                .extract::<i32>()
                .unwrap(),
            0
        );
        assert_eq!(
            other
                .getattr("extra_shift")
                .unwrap()
                .extract::<i32>()
                .unwrap(),
            0
        );
    });
    assert!(route.scheduler.is_empty());
}

fn playback_matrix(prior: PriorPlayback) {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for rate in [0.73, 1.0, 1.37] {
        for policy in ["first_sound_at_grid", "attack_at_grid"] {
            late_mouse_case(rate, policy, prior);
        }
    }
}

#[test]
fn actual_late_gui_piano_running_preserves_original_t_native_and_other_pad() {
    playback_matrix(PriorPlayback::Running);
}

#[test]
fn actual_late_gui_piano_paused_preserves_original_t_native_and_other_pad() {
    playback_matrix(PriorPlayback::Paused);
}

#[test]
fn actual_late_gui_piano_stopped_preserves_original_t_native_and_other_pad() {
    playback_matrix(PriorPlayback::Stopped);
}

#[test]
fn actual_late_gui_piano_at_first_paused_boundary_cannot_invent_prior_control_order() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for rate in [0.73, 1.0, 1.37] {
        for policy in ["first_sound_at_grid", "attack_at_grid"] {
            let mut route = configured(rate, policy, PriorPlayback::Running);
            route.mixer.pause_sample_at_output_frame(0, TARGET);
            let generation = voice(&route, 0).generation;
            let captured = route.capture("click", Some(7));
            let action = Python::attach(|py| captured.borrow(py).clone());
            assert_eq!(action.original_due_frame(), TARGET);
            route.render(1);
            route.drain();
            assert!(!action.accepted());
            assert_eq!(action.reason().as_deref(), Some("preparation_failed"));
            assert!(!action.effective());
            assert_eq!(voice(&route, 0).generation, generation);
            assert!(voice(&route, 0).paused);
            assert_eq!(route.starts(0), 0);
            assert_eq!(route.starts(1), 0);
        }
    }
}
