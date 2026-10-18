//! K13/K16-22: an actual -5 piano note retains its independently admitted -18
//! transposition through a public .5 -> 2 glide. Complete original mono PCM,
//! synthetic accepted timing labels, host waits and a virtual observer clock are
//! fixture seams; controllers/Learn/JSON reload, captures, actual feed history,
//! worker Native ownership, scheduler and rendered PCM use the productive path.

use super::*;
use crate::audio_engine::source_feed_history::SourceFeedSnapshot;

const LOWER_CHILD: &str = "FLITZI_LOWER_TOTAL_PIANO_GLIDE_CHILD";
const LOWER_TEST: &str = "audio_engine::pitch_runtime_tests::piano_mouse_tests::piano_rate_controller_tests::piano_rate_glide_tests::lower_total_rate_glide_tests::lower_total_piano_offset_retrigger_keeps_original_public_glide";
const LOWER_NOTE: i32 = -5;
const LOWER_EXTRA: i32 = -18;
const LOWER_TOTAL: i32 = -23;

const LOWER_HELPER: &str = r#"
from pathlib import Path
from tempfile import mkdtemp
from time import monotonic, sleep
from unittest.mock import patch

from flitzis_looper.input_mapping.controller import InputMappingController
from flitzis_looper.input_mapping.storage import load_midi_mapping_file, save_midi_mapping_file


def configure(probe):
    assert not probe.transport.pitch._owners
    assert probe.project.pad_key_intent[0].source_key.root == 0
    probe.project.selected_pad = 0
    probe.frame()
    item = next(item for item in probe.geometry if item.key.value == -5)
    assert len(probe.geometry) == 36
    assert item.semitone_index == 7
    assert item.key.value == item.semitone_index - (12 + 0)
    probe.ctx.audio.pads.set_extra_retrigger_intent(0, enabled=False)
    assert not probe.project.pad_key_intent[0].extra_retrigger
    root = Path(mkdtemp(prefix="lower-total-piano-glide-"))
    (root / "OWNER.md").write_text(
        "Owned isolated lower-total piano/glide mapping fixture; retain until child exit.\n"
    )
    probe.lower_mapping = root / "mapping.json"
    save_midi_mapping_file(probe.input_mapping._midi, probe.lower_mapping)
    probe.storage.enter_context(
        patch(
            "flitzis_looper.input_mapping.controller.load_midi_mapping_file",
            side_effect=lambda: load_midi_mapping_file(probe.lower_mapping),
        )
    )
    probe.storage.enter_context(
        patch(
            "flitzis_looper.input_mapping.controller.save_midi_mapping_file",
            side_effect=lambda data: save_midi_mapping_file(data, probe.lower_mapping),
        )
    )
    probe.input_mapping.toggle_learn()
    assert probe.session.input_learn_active


def finish_learn(probe):
    deadline = monotonic() + 2
    while probe.session.input_learn_pending_binding_key is None:
        probe.input_mapping._poll_rust_input_events()
        assert monotonic() < deadline, "actual lower-note MIDI Learn input did not arrive"
        sleep(0.001)
    assert probe.session.input_learn_pending_binding_key == "midi:note:1:60"
    probe.ctx.audio.pads.choose_piano_note(0, -5, retrigger=True)
    assert not probe.session.input_learn_active
    assert not probe.transport.pitch._owners
    intent = probe.project.pad_key_intent[0]
    assert (intent.base_shift, intent.extra_shift) == (0, 0)
    saved = load_midi_mapping_file(probe.lower_mapping)
    mapping = next(item for item in saved.mappings if item.input.key == "midi:note:1:60")
    assert mapping.action.key == "pitch.note:-5:set_retrigger"
    probe.lower_saved = probe.lower_mapping.read_bytes()
    probe.input_mapping = InputMappingController(probe)
    assert probe.input_mapping._midi == saved
    probe.input_mapping._sync_rust_mapping_snapshot()
    probe.input_mapping._sync_rust_learn_state()
    assert probe.input_mapping.refresh_pitch_runtime_state()
    assert probe.lower_mapping.read_bytes() == probe.lower_saved


def offset_step(probe, expected):
    assert not probe.transport.pitch._owners
    before = set(probe.transport.pitch._owners)
    probe.ctx.audio.pads.step_pitch(
        0,
        -1,
        piano=False,
        retrigger=False,
        received_at_ns=probe.ctx.input.capture_timestamp_ns(),
    )
    created = set(probe.transport.pitch._owners) - before
    assert len(created) == 1, probe.transport.pitch.status(0)
    owner = probe.transport.pitch._owners[created.pop()]
    capture = owner.capture
    assert owner.published
    assert owner.content_id == probe.project.pad_content[0].instance_id
    assert (capture.base_shift, capture.extra_shift, capture.retrigger) == (0, expected, False)
    intent = probe.project.pad_key_intent[0]
    assert (intent.base_shift, intent.extra_shift) == (0, expected)
    assert probe.session.input_mapping_error is None
    return capture


def settle_offset(probe, capture, expected):
    assert capture.effective
    assert capture.state == "applied"
    probe.transport.pitch.poll()
    assert not probe.transport.pitch._owners
    assert probe.transport.pitch.status(0) == ("applied", None)
    intent = probe.project.pad_key_intent[0]
    assert (intent.base_shift, intent.extra_shift, intent.extra_retrigger) == (0, expected, False)
    assert probe.project.pad_key_intent[1].extra_shift == 0


def poll_event(probe):
    deadline = monotonic() + 2
    while True:
        event = probe._audio.poll_input_events()
        if event is not None:
            return event
        assert monotonic() < deadline, "actual lower-note MIDI event did not arrive"
        sleep(0.001)


def selection(probe, slot):
    event = poll_event(probe)
    assert event.get("action_key") == f"ui.select_pad:{slot}"
    assert event.get("captured_pitch") is None
    probe.input_mapping._handle_rust_input_event(event)
    assert probe.project.selected_pad == slot


def midi_handoff(probe):
    events = [poll_event(probe) for _ in range(3)]
    assert [event.get("action_key") for event in events] == [
        "ui.select_pad:0",
        "pitch.note:-5:set_retrigger",
        "ui.select_pad:1",
    ]
    event = events[1]
    capture = event["captured_pitch"]
    assert event["direct"]
    assert not event["dispatched"]
    assert capture.received_at_ns == event["received_at_ns"]
    assert (capture.base_shift, capture.extra_shift, capture.retrigger) == (-5, -18, True)
    for event in events:
        probe.input_mapping._handle_rust_input_event(event)
    owner = probe.transport.pitch._owners[capture.accepted_sequence]
    assert owner.capture is capture
    assert owner.published
    assert owner.content_id == probe.project.pad_content[0].instance_id
    assert probe.project.selected_pad == 1
    assert capture.current_control_slot == 0
    assert capture.content_is_current
    assert probe.session.input_mapping_error is None
    return capture


def final_projection(probe):
    assert not probe.transport.pitch._owners
    assert probe.project.selected_pad == 1
    intent = probe.project.pad_key_intent[0]
    assert (intent.base_shift, intent.extra_shift) == (-5, -18)
    assert not intent.extra_retrigger
    peer = probe.project.pad_key_intent[1]
    assert peer.extra_shift == 0
    assert probe.lower_mapping.read_bytes() == probe.lower_saved
    assert probe.session.input_mapping_error is None
"#;

fn helper_call(route: &RateRoute, helper: &Py<PyModule>, name: &str) {
    Python::attach(|py| {
        helper
            .getattr(py, name)
            .unwrap()
            .call1(py, (route.route.probe.clone_ref(py),))
            .unwrap();
    });
}

fn inject(route: &RateRoute, numbers: &[u8]) {
    Python::attach(|py| {
        let engine = route.route.engine.borrow(py);
        let runtime = engine.input_runtime.as_ref().unwrap();
        for number in numbers {
            assert!(runtime.inject_midi_message(&[0x90, *number, 127]));
        }
    });
}

fn selection(route: &RateRoute, helper: &Py<PyModule>, slot: usize) {
    inject(route, &[40 + slot as u8]);
    Python::attach(|py| {
        helper
            .getattr(py, "selection")
            .unwrap()
            .call1(py, (route.route.probe.clone_ref(py), slot))
            .unwrap();
    });
}

fn mouse_capture(route: &RateRoute, note: i32) -> Py<CapturedPitchAction> {
    route.publish_clock();
    Python::attach(|py| {
        route
            .helper
            .getattr(py, "mouse")
            .unwrap()
            .call1(py, (route.route.probe.clone_ref(py), 0, note, LOWER_EXTRA))
            .unwrap()
            .extract(py)
            .unwrap()
    })
}

/// Query all independently declared public glide rates before choosing how much
/// genuine callback history to render. Neither the DUT journal nor its plan
/// determines this conservative maximum or any reference PCM/rate epoch.
fn maximum_public_bound(lock: bool) -> usize {
    let mut clock = ScalarClock::new(0, INITIAL_SPEED);
    let mut bounds = std::collections::BTreeMap::new();
    let pitch = scalar_pitch(LOWER_TOTAL, lock, INITIAL_SPEED);
    bounds.insert(pitch.to_bits(), RawRubberBand::history_bound(pitch));
    clock.target(GLIDE_SPEED);
    for _ in 0..=31 * 512 {
        clock.before_sample();
        let pitch = scalar_pitch(LOWER_TOTAL, lock, clock.rate);
        bounds
            .entry(pitch.to_bits())
            .or_insert_with(|| RawRubberBand::history_bound(pitch));
        clock.advance();
    }
    assert_eq!(clock.rate, GLIDE_SPEED);
    *bounds.values().max().unwrap()
}

fn lower_total(lock: bool, midi: bool) {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let mut route = RateRoute::new(lock);
    // Independently evaluate the declared original signal into separate complete
    // allocations. No live reader, prepared plan or DUT journal supplies taps.
    route.reference_sources = [complete_source(0), complete_source(1)];
    for id in 0..2 {
        assert_ne!(
            route.reference_sources[id].source_address(),
            route.sources[id].source_address()
        );
        assert_eq!(
            route.reference_sources[id].samples.as_ref(),
            route.sources[id].samples.as_ref()
        );
    }
    let helper = Python::attach(|py| {
        PyModule::from_code(
            py,
            &CString::new(LOWER_HELPER).unwrap(),
            c"lower_total_piano_glide_helpers.py",
            c"lower_total_piano_glide_helpers",
        )
        .unwrap()
        .unbind()
    });
    helper_call(&route, &helper, "configure");
    inject(&route, &[60]);
    helper_call(&route, &helper, "finish_learn");
    for step in 1..=18 {
        let extra = -step;
        route.publish_clock();
        let captured: Py<CapturedPitchAction> = Python::attach(|py| {
            helper
                .getattr(py, "offset_step")
                .unwrap()
                .call1(py, (route.route.probe.clone_ref(py), extra))
                .unwrap()
                .extract(py)
                .unwrap()
        });
        let action = Python::attach(|py| captured.borrow(py).clone());
        let original = (
            action.sequence(),
            action.content_key(),
            action.received_timestamp(),
            action.original_due_frame(),
        );
        assert_eq!(action.variant(), PitchActionVariant::Set);
        assert_eq!(action.render().pitch, PitchIntent::new(0, extra).unwrap());
        route.callback(0);
        route.assert_no_attacks();
        assert!(action.effective(), "{:?}", action.reason());
        assert_eq!(action.execution_target(), action.original_due_frame());
        assert_eq!(
            route.route.mixer.pad_pitch_intent(0),
            Some(PitchIntent::new(0, extra).unwrap())
        );
        assert!(!route.route.mixer.voices.iter().any(|voice| voice.active));
        assert_eq!(
            (
                action.sequence(),
                action.content_key(),
                action.received_timestamp(),
                action.original_due_frame()
            ),
            original,
        );
        Python::attach(|py| {
            helper
                .getattr(py, "settle_offset")
                .unwrap()
                .call1(py, (route.route.probe.clone_ref(py), captured, extra))
                .unwrap();
        });
    }
    public_speed(&route, INITIAL_SPEED);
    route.callback(0);
    route.assert_no_attacks();
    let initial_rate = route.rate();
    assert_eq!(initial_rate.state, "applied");
    assert_eq!(initial_rate.effective_speed, INITIAL_SPEED);
    assert_eq!(
        initial_rate.effective_period,
        Some(PERIODS[0] / INITIAL_SPEED)
    );
    route.observe_rate();
    // Preserve the existing wet initial TOTAL (+2 OFF/-5 ON), while earning the
    // separate -18 through stopped control actions and choosing a different key.
    let birth_note = if lock { 13 } else { 20 };
    assert_ne!(birth_note, LOWER_NOTE);
    let captured = mouse_capture(&route, birth_note);
    let admitted = route.admit_capture(0, birth_note, LOWER_EXTRA, captured);
    let originals = [
        initial_birth_admitted(&mut route, 0, INITIAL_SPEED, admitted),
        initial_birth(&mut route, 1, INITIAL_SPEED),
    ];
    route.wait_claims();
    let maximum_bound = maximum_public_bound(lock);
    let sufficient_frame = originals[0].e + maximum_bound as u64 + 1024;
    route.render_to(sufficient_frame.max(route.route.transport.output_frame()));
    let before_f = route.route.transport.output_frame();
    let mut observed_history = Box::new(SourceFeedSnapshot::new());
    assert!(
        route
            .voice(0)
            .source_feed_history
            .capture_into(&mut observed_history)
    );
    let actual_active = observed_history.active_frames() - observed_history.active_origin();
    let retained = observed_history.retained_range().unwrap();
    assert_eq!(actual_active, before_f - originals[0].e);
    assert!(actual_active >= maximum_bound as u64 + 1024);
    assert!(retained.end - retained.start >= maximum_bound as u64 + 1024);
    assert_eq!(retained.end, observed_history.active_frames());
    drop(observed_history);
    let accepted = begin_glide(&mut route);
    route.callback(73);
    route.assert_no_attacks();
    let frame = route.route.transport.output_frame();
    let timeline = original_timeline(originals[0].e, INITIAL_SPEED, before_f, frame + 8192);
    let (origin, bound, queried_pitches) = reset_bound(&timeline, LOWER_TOTAL, lock).unwrap();
    assert!(bound <= maximum_bound);
    assert!(bound > HISTORY);
    assert!(actual_active >= bound as u64);
    let endpoint = timeline.last().unwrap();
    assert!(endpoint.rate > INITIAL_SPEED && endpoint.rate < GLIDE_SPEED);
    assert_eq!(endpoint.target, GLIDE_SPEED);
    let h = 2.0_f64.powf(-23.0 / 12.0);
    let cold_pitch = scalar_pitch(LOWER_TOTAL, lock, timeline[origin + 511].rate);
    let endpoint_bound =
        RawRubberBand::history_bound(scalar_pitch(LOWER_TOTAL, lock, endpoint.rate));
    assert!(bound >= endpoint_bound);
    assert_eq!(
        scalar_pitch(LOWER_TOTAL, lock, endpoint.rate),
        if lock { h / endpoint.rate } else { h }
    );
    eprintln!(
        "lower_total_piano_glide_history {}",
        serde_json::json!({
            "key_lock":lock,"midi":midi,"birth_base":birth_note,"birth_extra":LOWER_EXTRA,
            "birth_total":birth_note+LOWER_EXTRA,"original_birth":originals[0].e,
            "public_rate_E":before_f,"actual_history_before_rate_E":actual_active,
            "independent_max_public_H":maximum_bound,"independent_reset_H":bound,
            "independent_endpoint_H":endpoint_bound,"independent_first_block_cold_p":cold_pitch,
            "independent_reset_profiles":queried_pitches,"independent_A_active":origin,
            "captured_base":LOWER_NOTE,"captured_extra":LOWER_EXTRA,"independent_total":LOWER_TOTAL,
            "h":h,"original_T":frame,"original_E":frame+8192,"independent_E_rate":endpoint.rate,
            "independent_E_p":if lock {h/endpoint.rate} else {h},
            "scope":"Complete FullMix 48k, actual stopped offset SET/receipts, distinct wet birth, genuine callback history; journal observed only as coverage evidence, never oracle input"
        })
    );
    let case = format!(
        "lower-total-23-{}-{}",
        if lock { "on" } else { "off" },
        if midi { "midi" } else { "gui" }
    );
    apply_wet_attack_from(
        &mut route,
        &originals,
        &accepted,
        INITIAL_SPEED,
        WetAttack {
            case: &case,
            pitch: PitchIntent::new(LOWER_NOTE, LOWER_EXTRA).unwrap(),
            cached: None,
        },
        |route| {
            route.publish_clock();
            let captured = if midi {
                inject(route, &[40, 60, 41]);
                Python::attach(|py| {
                    helper
                        .getattr(py, "midi_handoff")
                        .unwrap()
                        .call1(py, (route.route.probe.clone_ref(py),))
                        .unwrap()
                        .extract(py)
                        .unwrap()
                })
            } else {
                // Native selection barriers surround the real ImGui piano hit;
                // the limited PianoProbe projects their genuine MIDI receipts.
                selection(route, &helper, 0);
                let captured = mouse_capture(route, LOWER_NOTE);
                selection(route, &helper, 1);
                captured
            };
            route.admit_capture(0, LOWER_NOTE, LOWER_EXTRA, captured)
        },
    );
    helper_call(&route, &helper, "final_projection");
    assert_eq!(route.starts, [2, 1]);
    assert_eq!(
        route.route.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(LOWER_NOTE, LOWER_EXTRA).unwrap())
    );
    route.route.close();
}

#[test]
fn lower_total_piano_offset_retrigger_keeps_original_public_glide() {
    if let Ok(child) = std::env::var(LOWER_CHILD) {
        match child.as_str() {
            "gui-off" => lower_total(false, false),
            "gui-on" => lower_total(true, false),
            "midi-off" => lower_total(false, true),
            "midi-on" => lower_total(true, true),
            _ => panic!("unknown declared lower-total piano/glide child"),
        }
        return;
    }
    for child in ["gui-off", "gui-on", "midi-off", "midi-on"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", LOWER_TEST, "--nocapture", "--test-threads=1"])
            .env(LOWER_CHILD, child)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "isolated lower-total piano/glide {child} failed:\n{stdout}\n{stderr}"
        );
        assert!(stdout.contains("1 passed; 0 failed"));
        assert!(stderr.contains("lower_total_piano_glide_history"));
        assert!(stderr.contains("piano_rate_glide_first_sample"));
        print!("{stdout}");
        eprint!("{stderr}");
    }
}
