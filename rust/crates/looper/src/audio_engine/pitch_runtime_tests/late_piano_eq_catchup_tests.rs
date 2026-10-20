//! Real mouse Piano gestures retain the original source, trim and EQ chronology.
//! Register as a child of piano_mouse_tests; no device or listening acceptance.
#![cfg(windows)]

use super::*;

const ORIGINAL_T: u64 = 24_000;
const EARLY_CAPTURE: u64 = ORIGINAL_T - 2_048;
const PAUSE_FRAME: u64 = ORIGINAL_T - 4_096;
const PREPARATION_LEAD: u64 = 8_192;
const EMITTED_FRAMES: usize = 12_289;
const TRIM_DB: f32 = -4.5;
const EQ_DB: [f32; 3] = [-12.0, -3.0, 4.0];

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

fn render_partitioned(route: &mut MouseRoute, frames: usize, partitions: &[usize]) -> Vec<f32> {
    let mut pcm = Vec::with_capacity(frames);
    let mut rendered = 0;
    let mut callback = 0;
    while rendered < frames {
        let count = partitions[callback % partitions.len()].min(frames - rendered);
        assert!(count > 0);
        pcm.extend(route.render(count));
        rendered += count;
        callback += 1;
    }
    pcm
}

fn selected_voice(route: &MouseRoute) -> &crate::audio_engine::voice_slot::VoiceSlot {
    route
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 0)
        .unwrap()
}

fn native_constructions(route: &MouseRoute) -> usize {
    Python::attach(|py| route.engine.borrow(py).pitch_work_capacity.allocations().1)
}

fn configured(lock: bool, policy: &str, prior: PriorPlayback) -> MouseRoute {
    let mut fixture = Fixture::new(lock);
    // Use the real idle setters. No selected or unrelated active voice is used
    // to settle trim before the stopped case's first gesture.
    fixture.mixer.set_pad_gain(0, TRIM_DB);
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
    let mut route = MouseRoute::from_fixture(fixture, lock, None);
    if prior != PriorPlayback::Stopped {
        assert!(route.mixer.play_sample(0, 1.0));
    }
    if prior == PriorPlayback::Paused {
        render_to(&mut route, PAUSE_FRAME);
        route.mixer.pause_sample_at_output_frame(0, PAUSE_FRAME);
        assert!(selected_voice(&route).paused);
    }
    render_to(&mut route, EARLY_CAPTURE);
    if prior == PriorPlayback::Stopped {
        assert!(!route.mixer.voices.iter().any(|voice| voice.active));
        assert_eq!(route.starts(0), 0);
    }
    route
}

fn enable_reference_quantization(route: &MouseRoute) {
    Python::attach(|py| {
        let project = route.probe.bind(py).getattr("project").unwrap();
        project
            .setattr("trigger_quantization_enabled", true)
            .unwrap();
        project
            .setattr("trigger_quantization_step", "1_16")
            .unwrap();
    });
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
            "Piano DSP host preparation exceeded its bound"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(route.transport.output_frame(), frame);
}

fn compare_late_mouse_beginning(lock: bool, policy: &str, prior: PriorPlayback, shift: i32) {
    // The independent reference really starts at T. It has its own public GUI
    // capture, work, source, Native/FIFO, trim and EQ; no late candidate supplies it.
    let mut early = configured(lock, policy, prior);
    enable_reference_quantization(&early);
    let early_captured = early.capture("click", Some(shift));
    let early_action = Python::attach(|py| early_captured.borrow(py).clone());
    assert_eq!(early_action.original_due_frame(), ORIGINAL_T);
    assert_eq!(
        early_action.render().pitch,
        PitchIntent::new(shift, 0).unwrap()
    );
    assert_eq!(early_action.render().key_lock, lock);
    let early_native_before = native_constructions(&early);
    early.drain();
    assert!(early_action.accepted(), "{:?}", early_action.reason());
    assert_eq!(early_action.execution_target(), ORIGINAL_T);
    wait_ready(&mut early, &early_action);

    // Construct the second Python context after the reference mouse gesture.
    // Both independent Native/source owners remain alive during all comparisons.
    let mut late = configured(lock, policy, prior);
    render_to(&mut late, ORIGINAL_T);
    let predecessor = (prior != PriorPlayback::Stopped).then(|| {
        let voice = selected_voice(&late);
        (
            voice.generation,
            voice.stretch.native_state_address(),
            voice.paused,
        )
    });
    let late_native_before = native_constructions(&late);
    let late_captured = late.capture("click", Some(shift));
    let late_action = Python::attach(|py| late_captured.borrow(py).clone());
    let original = (
        late_action.sequence(),
        late_action.content_key(),
        late_action.received_timestamp(),
        late_action.original_due_frame(),
    );
    assert_eq!(late_action.original_due_frame(), ORIGINAL_T);
    assert_eq!(late_action.render(), early_action.render());
    assert!(!late_action.accepted());
    late.render(1); // The actual callback sees the original gesture only after T.
    assert_eq!(late.transport.output_frame(), ORIGINAL_T + 1);
    late.drain();
    assert!(
        late_action.accepted(),
        "state={} reason={:?}",
        late_action.state(),
        late_action.reason()
    );
    let execution = ORIGINAL_T + 1 + if shift == 0 { 0 } else { PREPARATION_LEAD };
    assert_eq!(late_action.execution_target(), execution);
    if shift == 0 {
        // Execute the already-ready scalar event at this same F. No sample is
        // emitted or elapsed; the first real sample still participates below.
        assert!(late.render(0).is_empty());
        assert!(late_action.effective());
        assert_eq!(
            Python::attach(|py| late_captured
                .bind(py)
                .getattr("effective_output_frame")
                .unwrap()
                .extract::<Option<u64>>()
                .unwrap()),
            Some(execution)
        );
    } else {
        wait_ready(&mut late, &late_action);
    }

    render_to(&mut early, ORIGINAL_T);
    render_to(&mut late, execution);
    assert!(!early_action.effective());
    assert_eq!(late_action.effective(), shift == 0);
    assert_eq!(early.starts(0), 0);
    assert_eq!(late.starts(0), usize::from(shift == 0));
    if let Some(expected) = predecessor {
        let old = selected_voice(&late);
        assert_eq!(
            (
                old.generation,
                old.stretch.native_state_address(),
                old.paused
            ),
            expected
        );
    }

    let early_pcm = render_partitioned(&mut early, EMITTED_FRAMES, &[127, 1, 509, 31, 256]);
    // Every actual sample from E participates, including the first emitted sample.
    // The independent on-time reference supplies its complete emitted beginning.
    let late_pcm = render_partitioned(&mut late, EMITTED_FRAMES, &[257, 17, 1, 383, 96]);
    let expected = early_pcm.as_slice();
    assert_eq!(late_pcm.len(), EMITTED_FRAMES);
    assert_eq!(expected.len(), late_pcm.len());
    assert!(expected.iter().any(|sample| sample.abs() > 0.001));
    assert!(late_pcm.iter().all(|sample| sample.is_finite()));
    let maximum_error = late_pcm
        .iter()
        .zip(expected)
        .map(|(actual, expected)| (*actual - *expected).abs())
        .fold(0.0_f32, f32::max);
    let first_difference = late_pcm
        .iter()
        .zip(expected)
        .position(|(actual, expected)| actual != expected);
    assert_eq!(
        first_difference, None,
        "full late Piano EQ/trim beginning differs: lock={lock} policy={policy} prior={prior:?} shift={shift} max={maximum_error}"
    );
    assert_eq!(
        early_action.state(),
        "applied",
        "{:?}",
        early_action.reason()
    );
    assert_eq!(late_action.state(), "applied", "{:?}", late_action.reason());
    assert_eq!(early_action.execution_target(), ORIGINAL_T);
    assert_eq!(late_action.execution_target(), execution);
    assert_eq!(early.starts(0), 1);
    assert_eq!(late.starts(0), 1);
    assert_eq!(early.starts(1), 0);
    assert_eq!(late.starts(1), 0);
    assert!(early.scheduler.is_empty());
    assert!(late.scheduler.is_empty());
    assert_eq!(
        (
            late_action.sequence(),
            late_action.content_key(),
            late_action.received_timestamp(),
            late_action.original_due_frame(),
        ),
        original
    );
    let early_voice = selected_voice(&early);
    let late_voice = selected_voice(&late);
    let expected_source = LOOP_START + EMITTED_FRAMES;
    assert!(expected_source < LOOP_END);
    for voice in [early_voice, late_voice] {
        assert!(!voice.paused);
        assert_eq!(voice.source_playback.tempo_ratio(), 1.0);
        assert_eq!(voice.source_playback.position().frame, expected_source);
        assert_eq!(voice.source_playback.position().fraction, 0.0);
    }
    if shift == 0 {
        assert_eq!(native_constructions(&early), early_native_before);
        assert_eq!(native_constructions(&late), late_native_before);
        for voice in [early_voice, late_voice] {
            assert_eq!(voice.stretch.adopted_request_id(), None);
            assert_eq!(voice.stretch.native_primed_output_frames(), 0);
        }
    } else {
        assert_eq!(native_constructions(&early), early_native_before + 1);
        assert_eq!(native_constructions(&late), late_native_before + 1);
        assert_eq!(
            early_voice.stretch.adopted_request_id(),
            Some(early_action.sequence())
        );
        assert_eq!(
            late_voice.stretch.adopted_request_id(),
            Some(late_action.sequence())
        );
        assert_ne!(early_voice.stretch.native_state_address(), 0);
        assert_ne!(late_voice.stretch.native_state_address(), 0);
        assert_ne!(
            early_voice.stretch.native_state_address(),
            late_voice.stretch.native_state_address()
        );
        assert_eq!(
            late_voice.stretch.native_primed_output_frames(),
            early_voice.stretch.native_primed_output_frames()
        );
        assert_eq!(
            late_voice.stretch.pending_fifo_frames(),
            early_voice.stretch.pending_fifo_frames()
        );
    }
}

#[test]
fn actual_stopped_late_piano_wet_eq_trim_matches_early_first_sound_beginning() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    compare_late_mouse_beginning(true, "first_sound_at_grid", PriorPlayback::Stopped, 7);
}

#[test]
fn actual_stopped_late_piano_wet_eq_trim_matches_early_attack_beginning() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    compare_late_mouse_beginning(true, "attack_at_grid", PriorPlayback::Stopped, 7);
}

#[test]
fn actual_running_late_piano_wet_eq_trim_matches_early_beginning() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    compare_late_mouse_beginning(true, "first_sound_at_grid", PriorPlayback::Running, 7);
}

#[test]
fn actual_paused_late_piano_wet_eq_trim_matches_early_beginning() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    compare_late_mouse_beginning(true, "attack_at_grid", PriorPlayback::Paused, 7);
}

#[test]
fn actual_stopped_late_piano_dry_lock_on_eq_trim_has_no_new_native() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    compare_late_mouse_beginning(true, "first_sound_at_grid", PriorPlayback::Stopped, 0);
}

#[test]
fn actual_stopped_late_piano_dry_lock_off_eq_trim_has_no_new_native() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    compare_late_mouse_beginning(false, "attack_at_grid", PriorPlayback::Stopped, 0);
}

#[derive(Clone, Copy, Debug)]
enum Invalidation {
    EqAba,
    GainAba,
    GainRamp,
}

fn edit_after_capture(route: &mut MouseRoute, invalidation: Invalidation) {
    match invalidation {
        Invalidation::EqAba => {
            route.mixer.set_pad_eq(0, 3.0, -9.0, -6.0);
            route.mixer.set_pad_eq(0, EQ_DB[0], EQ_DB[1], EQ_DB[2]);
        }
        Invalidation::GainAba => {
            route.mixer.set_pad_gain(0, -9.0);
            route.mixer.set_pad_gain(0, TRIM_DB);
        }
        Invalidation::GainRamp => route.mixer.set_pad_gain(0, -9.0),
    }
}

#[test]
fn actual_late_piano_refuses_unproved_eq_gain_history_preserves_voice_and_fresh_mouse_recovers() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for invalidation in [
        Invalidation::EqAba,
        Invalidation::GainAba,
        Invalidation::GainRamp,
    ] {
        let mut hit = configured(true, "first_sound_at_grid", PriorPlayback::Running);
        render_to(&mut hit, ORIGINAL_T);
        let captured = hit.capture("click", Some(7));
        let action = Python::attach(|py| captured.borrow(py).clone());
        assert_eq!(action.original_due_frame(), ORIGINAL_T);
        let generation = selected_voice(&hit).generation;
        let native = selected_voice(&hit).stretch.native_state_address();

        let mut baseline = configured(true, "first_sound_at_grid", PriorPlayback::Running);
        render_to(&mut baseline, ORIGINAL_T);
        assert_eq!(hit.render(1), baseline.render(1));
        // The old T has actually elapsed before the control edit/ABA. No timed
        // render can certify the replacement DSP epoch back through that T.
        for route in [&mut hit, &mut baseline] {
            edit_after_capture(route, invalidation);
        }
        hit.drain();
        assert_eq!(
            action.state(),
            "rejected",
            "{invalidation:?} {:?}",
            action.reason()
        );
        assert!(action.reason().is_some());
        assert!(!action.effective());
        assert_eq!(hit.starts(0), 0);
        assert_eq!(hit.mixer.pad_pitch_intent(0), Some(PitchIntent::default()));
        assert_eq!(selected_voice(&hit).generation, generation);
        assert_eq!(selected_voice(&hit).stretch.native_state_address(), native);
        assert!(!selected_voice(&hit).paused);
        assert!(hit.scheduler.is_empty());
        assert_eq!(
            render_partitioned(&mut hit, 4_097, &[1, 127, 509, 31]),
            render_partitioned(&mut baseline, 4_097, &[257, 17, 383, 96])
        );
        assert!(
            selected_voice(&hit)
                .source_playback
                .matches_exact(&selected_voice(&baseline).source_playback)
        );
        assert_eq!(selected_voice(&hit).generation, generation);
        assert_eq!(hit.starts(0), 0);
        // Destroy the later-created baseline context before the next real mouse
        // gesture, restoring the hit probe's original ImGui context normally.
        drop(baseline);
        Python::attach(|py| {
            hit.probe
                .bind(py)
                .getattr("transport")
                .unwrap()
                .getattr("pitch")
                .unwrap()
                .call_method0("poll")
                .unwrap();
        });
        let fresh_captured = hit.capture("click", Some(7));
        let fresh = Python::attach(|py| fresh_captured.borrow(py).clone());
        assert_ne!(fresh.sequence(), action.sequence());
        assert_eq!(fresh.original_due_frame(), hit.transport.output_frame());
        hit.drain();
        assert!(fresh.accepted(), "{invalidation:?} {:?}", fresh.reason());
        wait_ready(&mut hit, &fresh);
        render_to(&mut hit, fresh.execution_target());
        assert_eq!(hit.starts(0), 0);
        let initial = hit.render(1);
        assert!(initial.iter().all(|sample| sample.is_finite()));
        assert!(fresh.effective(), "{invalidation:?} {:?}", fresh.reason());
        assert_eq!(hit.starts(0), 1);
        assert_eq!(hit.starts(1), 0);
        assert!(hit.scheduler.is_empty());
    }
}
