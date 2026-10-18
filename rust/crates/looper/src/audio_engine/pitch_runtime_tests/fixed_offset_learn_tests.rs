//! Rendered fixed-offset Learn -> actual CC capture -> own JSON -> fresh controller.
//!
//! Signed targets, immutable SET/RETRIGGER and both Key Locks use the existing
//! control/worker/scheduler/PCM route. The Python project envelope, source labels,
//! virtual clock and host worker waits are declared fixture seams. No MIDI port,
//! CPAL device, listening, performer deadline or full-App acceptance is claimed.

use super::*;

const BASE: i32 = -5;
const NUMBER: u16 = 70;
const HELPER: &str = r#"
from time import monotonic, sleep

from flitzis_looper.input_mapping.storage import load_midi_mapping_file, save_midi_mapping_file
from flitzis_looper.ui.render.pitch_keyboard import format_extra_shift


def master(probe):
    project = probe.project
    return (
        probe.session.master_bpm,
        probe.session.master_period_seconds,
        project.speed,
        project.multi_loop,
        project.bpm_lock,
        project.key_lock,
        tuple(project.manual_bpm),
        tuple(project.pad_loop_start_s),
        tuple(project.pad_loop_end_s),
    )


def choose_offset(probe, target):
    # Real public setup action, not seeded Native/project pitch state. Keep the
    # peer selected while explicitly addressing content 0, as replay will do.
    if not hasattr(probe, "saved"):
        save_midi_mapping_file(probe.input_mapping._midi, probe.mapping_path)
        probe.saved = probe.mapping_path.read_bytes()
    probe.project.selected_pad = 1
    before = set(probe.transport.pitch._owners)
    probe.ctx.audio.pads.choose_extra_shift(
        0, target, retrigger=False, received_at_ns=probe.ctx.input.capture_timestamp_ns()
    )
    created = set(probe.transport.pitch._owners) - before
    assert len(created) == 1, (probe.session.input_mapping_error, probe.transport.pitch.status(0))
    owner = probe.transport.pitch._owners[created.pop()]
    capture = owner.capture
    assert owner.published
    assert owner.content_id == probe.project.pad_content[0].instance_id
    assert (capture.base_shift, capture.extra_shift, capture.retrigger) == (-5, target, False)
    assert capture.current_control_slot == 0
    assert capture.content_is_current
    return capture


def finish_fixed_learn(probe, number, target, retrigger):
    deadline = monotonic() + 2
    while probe.session.input_learn_pending_binding_key is None:
        probe.input_mapping._poll_rust_input_events()
        assert monotonic() < deadline, "actual fixed-offset CC Learn input did not arrive"
        sleep(0.001)
    binding = f"midi:cc:1:{number}"
    assert probe.session.input_learn_pending_binding_key == binding
    probe.project.selected_pad = 0
    # Deliberately disagree between lanes: the fixed target must use offset's
    # own preference. Encoder-mode choice must not turn this into a delta.
    probe.ctx.audio.pads.set_pitch_retrigger_intent(0, enabled=not retrigger)
    probe.ctx.audio.pads.set_extra_retrigger_intent(0, enabled=retrigger)
    probe.ctx.ui.set_pitch_cc_mode("offset_binary")
    before = probe.project.pad_key_intent[0]
    assert (before.base_shift, before.extra_shift) == (-5, target)
    assert not probe.transport.pitch._owners
    probe.fixed_master = master(probe)
    probe.offset_frame()
    probe.offset_frame()
    assert "pitch_offset_fixed" not in probe.offset_items, "foldout did not start closed"
    probe.offset_click("pitch_steps")
    assert "pitch_offset_fixed" in probe.offset_items, "rendered fixed-offset Learn row missing"
    assert probe.offset_items["pitch_offset_fixed"][2] == (
        f"Fixed offset {format_extra_shift(target)}##pitch_offset_fixed"
    )
    probe.offset_click("pitch_offset_fixed")
    assert not probe.session.input_learn_active
    assert probe.session.input_learn_pending_binding_key is None
    assert not probe.transport.pitch._owners, "fixed-offset Learn admitted audible work"
    assert probe.project.pad_key_intent[0] == before, "fixed-offset Learn changed intent"
    assert master(probe) == probe.fixed_master
    variant = "set_retrigger" if retrigger else "set"
    expected = f"pitch.extra:{target}:{variant}"
    mapping = probe.input_mapping._midi.mappings[-1]
    assert mapping.input.key == binding
    assert mapping.action.key == expected
    disk = load_midi_mapping_file(probe.mapping_path)
    assert disk == probe.input_mapping._midi
    assert disk.mappings[-1].action.key == expected
    assert probe.session.input_mapping_error is None


def restore_fixed_learn(probe, target, retrigger):
    # Existing helper instantiates a fresh production controller and deserializes
    # the real own JSON. Two selection barriers plus one learned fixed target.
    probe.restore_encoder_learn(3)
    for slot in (0, 1):
        probe.ctx.audio.pads.set_pitch_retrigger_intent(slot, enabled=not retrigger)
        probe.ctx.audio.pads.set_extra_retrigger_intent(slot, enabled=not retrigger)
        intent = probe.project.pad_key_intent[slot]
        assert (intent.retrigger, intent.extra_retrigger) == (not retrigger, not retrigger)
    probe.input_mapping.refresh_pitch_runtime_state()
    variant = "set_retrigger" if retrigger else "set"
    learned = next(
        mapping
        for mapping in probe.input_mapping._midi.mappings
        if mapping.input.key == "midi:cc:1:70"
    )
    assert learned.action.key == f"pitch.extra:{target}:{variant}"
    assert master(probe) == probe.fixed_master


def assert_replayed(probe, target, retrigger):
    intent = probe.project.pad_key_intent[0]
    assert (intent.base_shift, intent.extra_shift) == (-5, target)
    assert (intent.retrigger, intent.extra_retrigger) == (not retrigger, not retrigger)
    assert probe.project.selected_pad == 1
    assert master(probe) == probe.fixed_master
    assert probe.mapping_path.read_bytes() == probe.saved
    assert probe.session.input_mapping_error is None
"#;

fn request_sequence(route: &MouseRoute) -> u64 {
    Python::attach(|py| {
        route
            .engine
            .borrow(py)
            .input_runtime
            .as_ref()
            .unwrap()
            .pitch_request_sequence(0)
            .unwrap()
    })
}

fn choose_offset(
    route: &MouseRoute,
    helper: &Py<PyModule>,
    target: i32,
) -> Py<CapturedPitchAction> {
    route.refresh_clock();
    Python::attach(|py| {
        helper
            .getattr(py, "choose_offset")
            .unwrap()
            .call1(py, (route.probe.clone_ref(py), target))
            .unwrap()
            .extract(py)
            .unwrap()
    })
}

#[test]
fn real_rendered_fixed_offsets_learn_restore_and_replay_absolute_variants_with_pcm() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // The owning run points the established fixture parent/TMP at its disposable
    // block. Four Native representatives complement the Python 37-target test.
    let parent = std::env::var_os("FLITZI_PIANO_MIDI_PARENT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("codex"));
    std::fs::create_dir_all(&parent).unwrap();
    for lock in [false, true] {
        for target in [-18, 0, 7, 18] {
            for retrigger in [false, true] {
                let root = tempfile::Builder::new()
                    .prefix("native-fixed-")
                    .tempdir_in(&parent)
                    .unwrap()
                    .keep();
                let mut route = MouseRoute::new_with_storage(lock, Some(&root.join("midi.json")));
                let helper = Python::attach(|py| {
                    PyModule::from_code(
                        py,
                        &CString::new(HELPER).unwrap(),
                        c"fixed_offset_learn_probe.py",
                        c"fixed_offset_learn_probe",
                    )
                    .unwrap()
                    .unbind()
                });
                let initial = route.capture("click", Some(BASE));
                route.apply(&initial, BASE, lock);
                let master = (
                    route.transport.master_period_seconds(),
                    route.transport.downbeat_frame(),
                );
                let setup = choose_offset(&route, &helper, target);
                route.apply_encoder(
                    &setup,
                    PitchIntent::new(BASE, target).unwrap(),
                    lock,
                    false,
                    false,
                );
                let starts = route.starts(0);
                let sequence = request_sequence(&route);
                let phase = route
                    .mixer
                    .voices
                    .iter()
                    .find(|voice| voice.is_playing_sample(0))
                    .unwrap()
                    .source_playback
                    .position();
                let frame = route.transport.output_frame();
                route.refresh_clock();
                Python::attach(|py| {
                    route.probe.call_method0(py, "begin_encoder_learn").unwrap();
                    inject_control_values(
                        route.engine.borrow(py).input_runtime.as_ref().unwrap(),
                        NUMBER,
                        &[127],
                        false,
                    );
                    helper
                        .getattr(py, "finish_fixed_learn")
                        .unwrap()
                        .call1(py, (route.probe.clone_ref(py), NUMBER, target, retrigger))
                        .unwrap();
                });
                route.drain();
                assert!(route.scheduler.is_empty());
                assert_eq!(request_sequence(&route), sequence, "Learn captured audio");
                assert_eq!(route.starts(0), starts);
                assert_eq!(route.transport.output_frame(), frame);
                assert_eq!(
                    route
                        .mixer
                        .voices
                        .iter()
                        .find(|voice| voice.is_playing_sample(0))
                        .unwrap()
                        .source_playback
                        .position(),
                    phase
                );
                Python::attach(|py| {
                    helper
                        .getattr(py, "restore_fixed_learn")
                        .unwrap()
                        .call1(py, (route.probe.clone_ref(py), target, retrigger))
                        .unwrap();
                });
                // Audible state now differs from the learned target. Replaying
                // must restore that absolute target, not add it to this offset.
                let divergent = if target == -18 { -17 } else { target - 1 };
                let changed = choose_offset(&route, &helper, divergent);
                route.apply_encoder(
                    &changed,
                    PitchIntent::new(BASE, divergent).unwrap(),
                    lock,
                    false,
                    false,
                );
                let variant = if retrigger { "set_retrigger" } else { "set" };
                let key = format!("pitch.extra:{target}:{variant}");
                let mut previous = request_sequence(&route);
                // Fixed extra CC is a legacy per-value action. Equal high
                // values and zero each retain their own original admission;
                // only the newer piano/Original and relative buttons use edges.
                for (repetition, value) in [127, 127, 0].into_iter().enumerate() {
                    let before_native = route
                        .mixer
                        .voices
                        .iter()
                        .find(|voice| voice.is_playing_sample(0))
                        .unwrap()
                        .stretch
                        .native_state_address();
                    let captured = route
                        .encoder_message(NUMBER, &[value], Some(&key), false)
                        .unwrap();
                    Python::attach(|py| {
                        let action = captured.borrow(py);
                        assert!(action.sequence() > previous);
                        previous = action.sequence();
                        assert_eq!(action.original_due_frame(), route.transport.output_frame());
                        assert_eq!(action.content_key(), initial.borrow(py).content_key());
                    });
                    route.apply_encoder(
                        &captured,
                        PitchIntent::new(BASE, target).unwrap(),
                        lock,
                        false,
                        retrigger,
                    );
                    if !retrigger && repetition > 0 {
                        assert_eq!(
                            route
                                .mixer
                                .voices
                                .iter()
                                .find(|voice| voice.is_playing_sample(0))
                                .unwrap()
                                .stretch
                                .native_state_address(),
                            before_native,
                            "equal-target SET replaced its already-owned Native"
                        );
                    }
                    Python::attach(|py| {
                        helper
                            .getattr(py, "assert_replayed")
                            .unwrap()
                            .call1(py, (route.probe.clone_ref(py), target, retrigger))
                            .unwrap();
                    });
                    assert_eq!(
                        (
                            route.transport.master_period_seconds(),
                            route.transport.downbeat_frame(),
                        ),
                        master
                    );
                    Python::attach(|py| {
                        let action = captured.borrow(py);
                        eprintln!(
                            "fixed_offset_learn_replay {}",
                            serde_json::json!({
                                "key_lock":lock,"base":BASE,"extra":target,
                                "retrigger":retrigger,"repetition":repetition,"cc_value":value,
                                "sequence":action.sequence(),
                                "original_T":action.original_due_frame(),
                                "execution_E":action.execution_target(),
                                "timestamp":action.received_timestamp(),
                                "scope":"real rendered Learn, own JSON reload, routed MIDI, existing worker/scheduler and settled PCM tone predicate; synthetic source/project/host waits, no device or acoustic acceptance"
                            })
                        );
                    });
                }
                route.drain();
                assert!(route.scheduler.is_empty());
                assert_eq!(request_sequence(&route), previous);
                assert_eq!(route.starts(0), starts + 3 * usize::from(retrigger));
                assert_eq!(route.starts(1), 0);
                route.close();
            }
        }
    }
}
