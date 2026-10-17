//! A real GUI/NRPN pitch attack prepared from generated V1 cannot authorize generated V2.
//! Device-free clocks and declared inference WAVs are fixture inputs; capture, worker,
//! selection publication, scheduler, Native adoption, retirement and PCM are production.

use super::*;
use crate::audio_engine::content_placement::ContentKey;
use crate::audio_engine::pitch_actions::{
    CapturedPitchAction, FrozenPitchRender, PitchActionVariant,
};
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::timing::{OutputClockSnapshot, SharedOutputClock};

const CHILD: &str = "FLITZI_PUBLIC_GENERATE_PITCH_ACTION_CHILD";
const TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::public_generate_adoption_tests::pitch_action_tests::original_v1_piano_attack_fences_replacement_then_fresh_v2_attack_owns_pcm";
const MASTER_PERIOD: f64 = 4.0;
const GRID_FRAMES: u64 = 48_000;
const CAPTURE_PHASE: u64 = 26_400;

#[test]
fn original_v1_piano_attack_fences_replacement_then_fresh_v2_attack_owns_pcm() {
    if let Ok(lock) = std::env::var(CHILD) {
        let (key_lock, followup): (bool, complete::PublicCompleteFollowup) = match lock.as_str() {
            "gui-off" | "nrpn-off" => (false, history_off),
            "gui-on" | "nrpn-on" => (true, history_on),
            _ => panic!("unknown declared attack lock profile"),
        };
        complete::history_with_piano_followup(key_lock, followup);
        eprintln!(
            "public Generate pitch attack child {lock} PID={}",
            std::process::id()
        );
        return;
    }
    for lock in ["gui-off", "gui-on", "nrpn-off", "nrpn-on"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(CHILD, lock)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "public Generate pitch attack {lock} child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains(&format!("public Generate pitch attack child {lock} PID="))
        );
    }
}

fn history_off(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    _: &[Arc<InputRuntimeOwnership>; 2],
) {
    history(playback, consumer, locals, engine, false);
}

fn history_on(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    _: &[Arc<InputRuntimeOwnership>; 2],
) {
    history(playback, consumer, locals, engine, true);
}

/// Publish current control rows and a fresh observed clock, then let the real
/// PitchController capture and publish this choice. The nearest 1/16 grid
/// is independently known from the declared 15 BPM clock and counted frame.
fn capture_attack(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &AudioEngine,
    clock: &SharedOutputClock,
    name: &str,
) -> CapturedPitchAction {
    assert_eq!(playback.frame % GRID_FRAMES, CAPTURE_PHASE);
    assert_eq!(playback.callback.transport.output_frame(), playback.frame);
    assert_eq!(
        playback.callback.transport.master_period_seconds(),
        Some(MASTER_PERIOD)
    );
    let expected_due = playback.frame - CAPTURE_PHASE + GRID_FRAMES;
    let frame_before = playback.frame;
    let beat_before = playback.callback.transport.beat_position();
    let now = engine.input_clock.capture_ns();
    clock.publish(OutputClockSnapshot {
        valid: true,
        observed_at_ns: now,
        audible_at_ns: now,
        output_frame: playback.frame,
        sample_rate_hz: RATE,
        master_period_seconds: playback.callback.transport.master_period_seconds(),
        downbeat_frame: playback.callback.transport.downbeat_frame(),
        master_beat: playback.callback.transport.beat_position().unwrap(),
        freshness_ns: 2_000_000_000,
    });
    Python::attach(|py| {
        locals.bind(py).set_item("attack_timestamp", now).unwrap();
    });
    controller_python(
        locals,
        r#"
transport.pitch.poll()
project.multi_loop = True
project.bpm_lock = False
project.trigger_quantization_enabled = True
project.trigger_quantization_step = '1_16'
for slot in (0, 215):
    runtime_loaded[slot] = True
    runtime_starts[slot] = project.pad_loop_start_s[slot]
    runtime_ends[slot] = project.pad_loop_end_s[slot]
    runtime_bindings[slot] = playback_audio.current_input_runtime_pad_binding(slot)
    assert runtime_bindings[slot] is not None
    intent = project.pad_key_intent[slot]
    runtime_pitch_rows[slot].update(base_shift=intent.base_shift,
        extra_shift=intent.extra_shift, key_lock=project.pad_key_lock[slot],
        quantize_step_64ths=4, sync=False,
        request_sequence=transport.pitch.projection_sequence(slot))
playback_audio.publish_piano_runtime(runtime_loaded, runtime_starts,
    runtime_ends, runtime_bindings, pitch_rows=runtime_pitch_rows)
assert not transport.pitch._owners
"#,
    );
    let midi = python_bool(locals, "attack_midi");
    if midi {
        controller_python(
            locals,
            r#"
if 'attack_mapping' not in globals():
    from unittest.mock import patch
    from flitzis_looper.input_mapping.actions import LooperAction
    from flitzis_looper.input_mapping.bindings import MidiBinding
    from flitzis_looper.input_mapping.controller import InputMappingController
    from flitzis_looper.input_mapping.storage import KeyboardMappingFile, MidiMappingFile, MidiMappingEntry
    # Frozen mapping input only: no mapping-file storage, Learn or MIDI port is used here.
    frozen_mapping = MidiMappingFile(mappings=[MidiMappingEntry(
        input=MidiBinding(kind='nrpn', channel=1, number=130),
        action=LooperAction.from_key('pitch.note:7:set_retrigger'))])
    with patch('flitzis_looper.input_mapping.controller.load_midi_mapping_file',
               return_value=frozen_mapping), patch(
               'flitzis_looper.input_mapping.controller.load_keyboard_mapping_file',
               return_value=KeyboardMappingFile()):
        attack_mapping = InputMappingController(SimpleNamespace(
            project=project, session=session, _audio=playback_audio, transport=transport))
    project.input_mapping_enabled = True
    attack_mapping._sync_rust_mapping_snapshot()
    playback_audio.set_input_mapping_enabled(True)
"#,
        );
        engine.select_input_pad(0).unwrap();
        assert!(
            !engine
                .inject_midi_input_for_test(vec![0xb0, 99, 1])
                .unwrap()
        );
        assert!(
            !engine
                .inject_midi_input_for_test(vec![0xb0, 98, 2])
                .unwrap()
        );
        assert!(
            engine
                .inject_midi_input_for_test(vec![0xb0, 6, 127])
                .unwrap()
        );
        controller_python(
            locals,
            r#"
from time import monotonic, sleep
attack_event = None
attack_event_deadline = monotonic() + 2
while attack_event is None:
    attack_event = playback_audio.poll_input_events()
    if attack_event is None:
        assert monotonic() < attack_event_deadline, 'actual NRPN pitch event unavailable'
        sleep(.001)
assert attack_event['action_key'] == 'pitch.note:7:set_retrigger'
assert attack_event['direct'] is True and attack_event['dispatched'] is False
assert attack_event.get('pitch_rejection') is None, attack_event
attack_capture = attack_event['captured_pitch']
assert attack_capture.state == 'captured' and not attack_capture.accepted
assert attack_capture.received_at_ns == attack_event['received_at_ns']
attack_timestamp = attack_capture.received_at_ns
attack_mapping._handle_rust_input_event(attack_event)
assert transport.pitch._owners[attack_capture.accepted_sequence].capture is attack_capture
"#,
        );
        assert!(engine.inject_midi_input_for_test(vec![0xb0, 6, 0]).unwrap());
    } else {
        controller_python(
            locals,
            r#"
transport.pitch.choose_piano_note(0, 7, retrigger=True, received_at_ns=attack_timestamp)
attack_capture = next(iter(transport.pitch._owners.values())).capture
"#,
        );
    }
    controller_python(
        locals,
        r#"
assert session.input_mapping_error is None, session.input_mapping_error
assert len(transport.pitch._owners) == 1, transport.pitch.status(0)
attack_owner = next(iter(transport.pitch._owners.values()))
assert attack_capture is attack_owner.capture
assert attack_owner.published and attack_owner.content_id == '9' * 32
assert attack_capture.base_shift == 7 and attack_capture.extra_shift == 0
assert attack_capture.retrigger and attack_capture.key_lock is key_lock
assert not attack_capture.sync_enabled and attack_capture.quantize_step_64ths == 4
assert attack_capture.received_at_ns == attack_timestamp
assert project.pad_key_intent[0].base_shift == 7
assert project.pad_key_lock[0] is key_lock and not project.bpm_lock
assert session.active_sample_ids == {0, 215} and not session.paused_sample_ids
"#,
    );
    let action = Python::attach(|py| {
        let capture = locals.bind(py).get_item("attack_capture").unwrap().unwrap();
        locals.bind(py).set_item(name, &capture).unwrap();
        capture
            .extract::<PyRef<CapturedPitchAction>>()
            .unwrap()
            .clone()
    });
    assert_eq!(action.original_due_frame(), expected_due);
    assert_eq!(action.render().pitch, PitchIntent::new(7, 0).unwrap());
    assert_eq!(action.variant(), PitchActionVariant::SetRetrigger);
    assert!(!action.sync() && !action.exclusive());
    if midi {
        assert!(action.received_timestamp() >= now);
        assert!(action.received_timestamp() <= engine.input_clock.capture_ns());
    } else {
        assert_eq!(action.received_timestamp(), now);
    }
    assert_eq!(action.control_guard(None).unwrap().id, 0);
    assert_eq!(
        playback.drain(consumer),
        1,
        "one original controller pitch Work"
    );
    assert!(consumer.is_empty());
    wait_until(|| {
        let runtime = playback.callback.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        assert_ne!(action.state(), "rejected", "{:?}", action.reason());
        runtime.has_ready_action(action.sequence())
    });
    assert!(action.accepted() && !action.effective());
    assert_eq!(
        action.execution_target(),
        expected_due,
        "future T remains E"
    );
    assert!(expected_due - playback.frame > 12_000);
    assert_eq!(
        playback.frame, frame_before,
        "host preparation wait consumes no audio"
    );
    assert_eq!(playback.callback.transport.output_frame(), frame_before);
    assert_eq!(playback.callback.transport.beat_position(), beat_before);
    assert_eq!(
        playback.callback.transport.master_period_seconds(),
        Some(MASTER_PERIOD)
    );
    action
}

fn render_until(playback: &mut ControllerPlayback, peer: &Peer, target: u64) {
    assert!(target >= playback.frame);
    let mut counts = [1_usize, 127, 777, 96, 257, 512, 31].into_iter().cycle();
    while playback.frame < target {
        let frames = counts
            .next()
            .unwrap()
            .min((target - playback.frame) as usize);
        render(playback, peer, frames);
    }
}

fn align_capture(playback: &mut ControllerPlayback, peer: &Peer) {
    let current = playback.frame % GRID_FRAMES;
    let delta = (CAPTURE_PHASE + GRID_FRAMES - current) % GRID_FRAMES;
    let target = playback.frame + delta;
    render_until(playback, peer, target);
}

#[derive(Debug, PartialEq)]
struct ActionIdentity {
    content: ContentKey,
    sequence: u64,
    received: u64,
    due: u64,
    execution: u64,
    render: FrozenPitchRender,
    variant: PitchActionVariant,
    sync: bool,
    exclusive: bool,
    loop_region: (f64, Option<f64>),
}

fn action_identity(action: &CapturedPitchAction) -> ActionIdentity {
    ActionIdentity {
        content: action.content_key(),
        sequence: action.sequence(),
        received: action.received_timestamp(),
        due: action.original_due_frame(),
        execution: action.execution_target(),
        render: action.render(),
        variant: action.variant(),
        sync: action.sync(),
        exclusive: action.exclusive(),
        loop_region: action.loop_region(),
    }
}

/// Independent V2 selected source: serialized 1010 is canonical components 1001.
/// The explicit 4096-frame loop and r=1 provide the pre-Normal history at zero.
fn selected_v2_feed(components: &[SampleBuffer; 4], start: u64, frames: usize) -> [Vec<f32>; 2] {
    std::array::from_fn(|channel| {
        (start..start + frames as u64)
            .map(|frame| {
                let index = (frame as usize % 4096) * 2 + channel;
                components[0].samples[index] + components[3].samples[index]
            })
            .collect()
    })
}

/// After fresh E the old reference mixers retain the preceding uninterrupted
/// performance. This bounded suffix instead compares actual scheduled audio to
/// a new independent raw Native and reset EQ for the newly requested attack.
fn fresh_suffix(
    playback: &mut ControllerPlayback,
    peer: &Peer,
    current: &FinalReaderProbe,
    components: &[SampleBuffer; 4],
    action: &CapturedPitchAction,
    old_native: usize,
) {
    let pitch = 2.0_f64.powf(7.0 / 12.0);
    let mut raw = ControllerRawNative::new_with_pitch(pitch, true);
    raw.render(&selected_v2_feed(components, 0, 4096));
    let mut filter = controller_filter();
    let start = playback.frame;
    assert_eq!(start, action.execution_target());
    let generation = voice(&playback.callback.mixer).generation;
    let mut consumed = 0_u64;
    for frames in [1_usize, 31, 137, 511, 777, 96, 512, 1, 257, 4096] {
        let expected_raw = raw.render(&selected_v2_feed(components, consumed, frames));
        let mut output = vec![0.0; frames * 2];
        let callback = playback.callback.as_mut();
        crate::audio_engine::audio_stream::render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            playback.frame,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        callback.transport.advance_by_rendered_frames(frames);
        for (actual, raw) in output.chunks_exact(2).zip(expected_raw.chunks_exact(2)) {
            filter.begin_frame();
            for channel in 0..2 {
                assert_eq!(
                    actual[channel],
                    filter.process_sample(channel, raw[channel]),
                    "fresh V2 selected Native + reset EQ at {}",
                    playback.frame
                );
                assert!(actual[channel].is_finite());
            }
        }
        consumed += frames as u64;
        playback.frame += frames as u64;
        playback.source_frame += frames as u64;
        assert_eq!(playback.callback.transport.output_frame(), playback.frame);
        assert_eq!(
            playback.callback.transport.master_period_seconds(),
            Some(MASTER_PERIOD)
        );
        let actual = voice(&playback.callback.mixer);
        assert_eq!(
            actual.source_playback.position().frame,
            consumed as usize % 4096
        );
        assert_eq!(
            actual.source_playback.position().fraction.to_bits(),
            0.0_f64.to_bits()
        );
        assert_eq!(actual.source_playback.tempo_ratio(), 1.0);
        assert_eq!(actual.generation, generation + 1);
        assert_ne!(actual.stretch.native_state_address(), old_native);
        assert_eq!(actual.stretch.adopted_request_id(), Some(action.sequence()));
        assert_eq!(actual.stretch.native_primed_output_frames(), 4096);
        assert_eq!(
            actual.stretch.pending_fifo_frames(),
            (raw.input[0].len(), raw.output[0].len())
        );
        assert!(
            actual
                .stretch
                .owns_pitch_contract(PitchIntent::new(7, 0).unwrap(), peer.profile.key_lock)
        );
        assert_eq!(
            playback.callback.mixer.pad_pitch_intent(0),
            Some(PitchIntent::new(7, 0).unwrap())
        );
        assert_eq!(
            playback.callback.mixer.key_lock_for_measurement(0),
            peer.profile.key_lock
        );
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(0).0,
            StemMixMode::AllStems
        );
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(0).1,
            0b1010
        );
        let set = playback.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(
            current
                .identity
                .ptr_eq(&Arc::downgrade(&set.complete_set_identity))
        );
        for (expected, stem) in current.pcm.iter().zip(&set.stems) {
            assert!(expected.ptr_eq(&Arc::downgrade(&stem.samples)));
        }
        current.assert_retained();
        peer.assert_current(playback);
    }
    assert!(
        action.effective(),
        "{} {:?}",
        action.state(),
        action.reason()
    );
    assert!(playback.callback.scheduler.is_empty());
    assert!(
        voice(&playback.callback.mixer)
            .stretch
            .wet_current_source_output_ready()
    );
    assert!(
        consumed > 4096,
        "productive suffix crosses its own complete selected loop"
    );
}

/// MIDI selection also owns the ordinary selected-endpoint cache. Real callbacks
/// service that independent worker lane even while stopped; loader polling alone
/// cannot return a completed endpoint owner to the lane for off-thread disposal.
fn retire_selected_endpoint_readers(
    playback: &mut ControllerPlayback,
    locals: &Py<PyDict>,
    engine: &AudioEngine,
    probe: &FinalReaderProbe,
    stage: &str,
) {
    let frame = playback.frame;
    probe.wait_for(stage, || {
        let callback = playback.callback.as_mut();
        crate::audio_engine::audio_stream::render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut [],
            &mut [0.0; NUM_SAMPLES],
            frame,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        Python::attach(|py| {
            engine.poll_loader_events(py).unwrap();
        });
        controller_python(locals, "controller.on_frame_render()");
        probe.identity.strong_count() == 0 && probe.pcm.iter().all(|pcm| pcm.strong_count() == 0)
    });
    assert_eq!(playback.frame, frame);
    assert_eq!(playback.callback.transport.output_frame(), frame);
}

fn history(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    key_lock: bool,
) {
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item(
                "attack_midi",
                std::env::var(CHILD).unwrap().starts_with("nrpn-"),
            )
            .unwrap();
    });
    let profile = ProcessingProfile {
        ratio: 1.0,
        key_lock,
        piano_shift: 12,
        native_pitch: 2.0,
    };
    let peer = Peer::new(playback, profile);
    let DeclaredOutputs {
        first,
        second,
        components,
    } = declared_wavs();
    let (actual, worker) = crate::audio_engine::buffer_retirement::create_audio_buffer_retirement();
    playback.callback.retirement.retired.clear();
    playback.callback.retirement.actual = Some(actual);
    let GenerationFixture {
        root,
        mut parameters,
        release,
    } = setup(playback, locals, engine, &first, &second, profile);
    let hash = crate::audio_engine::stem_cache::source_version_hash(&Python::attach(|py| {
        locals
            .bind(py)
            .get_item("generate_source_version")
            .unwrap()
            .unwrap()
            .extract::<String>()
            .unwrap()
    }));
    let source_window = key_lock_identity(engine);
    generate(playback, consumer, locals, &peer, false);
    let ack1 = adopt(playback, consumer, locals, &peer, "generated1");
    assert!(playback.reference.set_stem_enabled_mask(0, 0b1010, hash));
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::AllStems, hash)
    );
    playback.select(Some(0b1010));
    let old = probe(
        &root,
        playback,
        locals,
        "generated_disk1",
        &components,
        [0, 1, 2, 3],
    );
    settle(playback, locals, &peer, "generated1", ack1, None);

    // Previous non-scheduled reference rendering already consumed this exact delta.
    let delta = playback
        .frame
        .checked_sub(playback.callback.transport.output_frame())
        .unwrap();
    playback
        .callback
        .transport
        .advance_by_rendered_frames(delta as usize);
    assert_eq!(playback.callback.transport.output_frame(), playback.frame);
    assert!(playback.callback.transport.set_master_period(MASTER_PERIOD));
    assert_eq!(playback.callback.transport.downbeat_frame(), 0);
    assert_eq!(
        playback.callback.transport.beat_position(),
        Some(playback.frame as f64 / (f64::from(RATE) * MASTER_PERIOD))
    );
    playback.scheduled = true;
    let clock = Arc::new(SharedOutputClock::new());
    engine
        .input_runtime
        .as_ref()
        .unwrap()
        .attach_pitch_clock(clock.clone())
        .unwrap();
    align_capture(playback, &peer);
    controller_python(locals, "assert project.pad_key_intent[0].base_shift == 12");
    let old_attack = capture_attack(playback, consumer, locals, engine, &clock, "old_attack");
    let original = action_identity(&old_attack);
    controller_python(
        locals,
        r#"
old_attack_snapshot = (old_attack.accepted_sequence, old_attack.received_at_ns,
    old_attack.content_lifetime, old_attack.due_frame, old_attack.execution_frame,
    old_attack.base_shift, old_attack.extra_shift, old_attack.key_lock,
    old_attack.retrigger, old_attack.sync_enabled, old_attack.quantize_step_64ths)
"#,
    );
    let original_generation = voice(&playback.callback.mixer).generation;
    old.assert_retained();
    assert!(playback.frame < old_attack.execution_target());

    generate(playback, consumer, locals, &peer, true);
    let ack2 = adopt(playback, consumer, locals, &peer, "generated2");
    assert!(playback.reference.set_stem_enabled_mask(0, 0b1001, hash));
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::AllStems, hash)
    );
    playback.select(Some(0b1001));
    let current = probe(
        &root,
        playback,
        locals,
        "generated_disk2",
        &components,
        [1, 0, 2, 3],
    );
    controller_python(
        locals,
        "del generated1, generated_live1, generate_job1, generated1_reopened\ncontroller.on_frame_render()",
    );
    playback.callback.retirement.slots = 0;
    settle(
        playback,
        locals,
        &peer,
        "generated2",
        ack2,
        Some((engine, &old)),
    );
    assert_eq!(action_identity(&old_attack), original);
    assert!(
        playback.frame < original.execution,
        "V2 is genuinely settled before frozen E"
    );
    fifo(
        playback,
        &old,
        "V1 ordinary Native retired while original attack retains its own pins",
    );
    if old_attack.state() != "rejected" {
        old.assert_retained();
    }
    let v2_native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let v2_adoption = voice(&playback.callback.mixer).stretch.adopted_request_id();
    let demand = playback.callback.mixer.stem_demand_for_measurement(0);
    let mut counts = [1_usize, 127, 777, 96, 257, 512, 31].into_iter().cycle();
    while playback.frame < original.execution {
        if old_attack.state() != "rejected" {
            old.assert_retained();
        }
        let frames = counts
            .next()
            .unwrap()
            .min((original.execution - playback.frame) as usize);
        render(playback, &peer, frames);
        assert_eq!(action_identity(&old_attack), original);
        current.assert_retained();
    }
    // The real scheduler owns terminal rejection, including the output-frame boundary.
    render(playback, &peer, 1);
    assert_eq!(old_attack.state(), "rejected");
    assert!(matches!(
        old_attack.reason(),
        Some("prepared_mismatch" | "source_changed")
    ));
    assert!(!old_attack.effective());
    assert_eq!(action_identity(&old_attack), original);
    assert_eq!(
        voice(&playback.callback.mixer).generation,
        original_generation
    );
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        v2_native
    );
    assert_eq!(
        voice(&playback.callback.mixer).stretch.adopted_request_id(),
        v2_adoption
    );
    assert_eq!(
        playback.callback.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(12, 0).unwrap())
    );
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0),
        demand
    );
    assert_eq!(key_lock_identity(engine), source_window);
    complete::feedback(locals, &mut playback.callback);
    controller_python(
        locals,
        r#"
assert old_attack.state == 'rejected' and not old_attack.effective
assert old_attack.effective_output_frame is None
assert (old_attack.accepted_sequence, old_attack.received_at_ns,
    old_attack.content_lifetime, old_attack.due_frame, old_attack.execution_frame,
    old_attack.base_shift, old_attack.extra_shift, old_attack.key_lock,
    old_attack.retrigger, old_attack.sync_enabled, old_attack.quantize_step_64ths) == old_attack_snapshot
assert old_attack.accepted_sequence not in transport.pitch._owners
assert transport_events == [('started', 215), ('started', 0)]
assert controller.stem_residency_status(0).effective_stem_set_identity == generated_disk2.stem_set_identity
assert generated2.effective_mode_status() == 'effective'
assert project.stem_cache[0].pair == generated_disk2
assert len(captured_start_calls) == 1
"#,
    );
    fifo(
        playback,
        &old,
        "terminal old pitch candidate retirement FIFO",
    );
    retire_selected_endpoint_readers(
        playback,
        locals,
        engine,
        &old,
        "V1 terminal attack and independently selected endpoint final readers",
    );
    cleanup(engine, locals, &old);
    current.assert_retained();

    align_capture(playback, &peer);
    let fresh = capture_attack(playback, consumer, locals, engine, &clock, "fresh_attack");
    assert_ne!(fresh.sequence(), old_attack.sequence());
    assert_ne!(fresh.received_timestamp(), old_attack.received_timestamp());
    assert!(fresh.original_due_frame() > old_attack.original_due_frame());
    assert_eq!(fresh.content_key(), old_attack.content_key());
    render_until(playback, &peer, fresh.execution_target());
    fresh_suffix(playback, &peer, &current, &components, &fresh, v2_native);
    complete::feedback(locals, &mut playback.callback);
    controller_python(
        locals,
        r#"
assert fresh_attack.state == 'applied' and fresh_attack.effective
assert fresh_attack.effective_output_frame == fresh_attack.execution_frame == fresh_attack.due_frame
assert fresh_attack.accepted_sequence not in transport.pitch._owners
assert transport.pitch.status(0) == ('applied', None)
assert transport_events == [('started', 215), ('started', 0), ('started', 0)]
assert len(captured_start_calls) == 1
assert project.pad_key_intent[0].base_shift == 7 and project.pad_key_intent[0].extra_shift == 0
assert project.pad_key_lock[0] is key_lock and not project.bpm_lock
assert project.stem_cache[0].pair == generated_disk2
assert len(generate_demucs.calls) == len(generate_bs.calls) == 1
"#,
    );

    prepare_public_unload_loader(locals);
    playback.callback.retirement.slots = 0;
    controller_python(
        locals,
        r#"
loader.unload_sample(0)
assert project.sample_paths[0] is None and project.stem_cache[0] is None
assert project.pad_stem_mix_mode[0] == 'full_mix'
del generated2, generated_live2, generate_job2, generated2_reopened
"#,
    );
    current.assert_retained();
    assert_eq!(
        playback.drain(consumer),
        0,
        "public Unload awaits retirement admission"
    );
    assert!(voice(&playback.callback.mixer).is_playing_sample(0));
    playback.callback.retirement.slots = usize::MAX;
    let before = playback.callback.feedback.events.len();
    let stopped = ExpectedStoppedFeedback::source_command(&playback.callback.mixer, consumer, 0);
    crate::audio_engine::audio_stream::drain_parameter_messages(
        &mut parameters,
        &mut playback.callback.mixer,
        &mut playback.callback.transport,
    );
    assert!(parameters.is_empty());
    assert_eq!(playback.drain(consumer), 4, "public Unload/defaults ACK");
    assert!(consumer.is_empty());
    assert!(playback.callback.mixer.bank_for_measurement()[0].is_none());
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(playback.callback.feedback.events.len(), before + 1);
    stopped.assert_message(&playback.callback.feedback.events[before]);
    complete::feedback(locals, &mut playback.callback);
    for frames in [1_usize, 31, 137] {
        let mut silence = vec![0.0; frames * 2];
        let callback = playback.callback.as_mut();
        crate::audio_engine::audio_stream::render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut silence,
            &mut [0.0; NUM_SAMPLES],
            playback.frame,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        callback.transport.advance_by_rendered_frames(frames);
        assert!(silence.iter().all(|sample| *sample == 0.0));
        playback.frame += frames as u64;
        assert_eq!(playback.callback.transport.output_frame(), playback.frame);
        peer.assert_current(playback);
    }
    fifo(
        playback,
        &current,
        "fresh V2 attack public Unload production FIFO",
    );
    retire_selected_endpoint_readers(
        playback,
        locals,
        engine,
        &current,
        "V2 public Unload and independently selected endpoint final readers",
    );
    cleanup(engine, locals, &current);
    controller_python(
        locals,
        r#"
assert session.active_sample_ids == {215}
assert not session.stem_generation_errors and not session.sample_load_errors
assert len(generate_demucs.calls) == len(generate_bs.calls) == 1
(generate_module.separator_model_cache_dir, generate_module.resolved_demucs_encoder,
 generate_module.encoder_contract, generate_module.bs_input_contract,
 generate_module.capture_processing_files, generate_module.processing_source_roots) = generate_saved_functions
"#,
    );
    playback.callback.retirement.actual.take();
    drop(worker);
    drop(release);
}
