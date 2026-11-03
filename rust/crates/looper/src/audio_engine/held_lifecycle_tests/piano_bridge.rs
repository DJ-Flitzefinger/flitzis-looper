//! Complete Loader sources through public Piano actions, without an audio device.

use super::controller_overlap::{script_value, settle, start};
use super::*;
use crate::audio_engine::pitch_actions::{CapturedPitchAction, PitchActionVariant};
use crate::audio_engine::productive_source_history::ProductiveSourceHistory;
use crate::audio_engine::source_playback::SourcePlayback;

fn source_image(f: &mut Fixture, id: usize) -> (usize, String) {
    let source = f.state().mixer.bank_for_measurement()[id].as_ref().unwrap();
    assert_eq!(source.frame_count(), 3 * RATE as usize);
    assert_eq!(
        (source.resident_start(), source.resident_end()),
        (0, source.frame_count())
    );
    assert_eq!(source.samples.len(), 2 * source.frame_count());
    let mut hash = Sha256::new();
    for sample in source.samples.iter() {
        hash.update(sample.to_le_bytes());
    }
    (
        source.samples.as_ptr() as usize,
        format!("{:x}", hash.finalize()),
    )
}

fn voice_image(f: &mut Fixture, id: usize) -> (u64, usize, usize, SourcePlayback) {
    let voice = f
        .state()
        .mixer
        .voices
        .iter()
        .find(|v| v.is_playing_sample(id))
        .unwrap();
    assert!(!voice.paused && voice.volume > 0.0);
    (
        voice.generation,
        voice.sample.as_ref().unwrap().samples.as_ptr() as usize,
        voice.stretch.native_state_address(),
        voice.source_playback,
    )
}

fn assert_adopted(f: &mut Fixture, action: &CapturedPitchAction) {
    let voice = f
        .state()
        .mixer
        .voices
        .iter()
        .find(|v| v.is_playing_sample(0))
        .unwrap();
    let render = action.render();
    assert_eq!(voice.stretch.adopted_request_id(), Some(action.sequence()));
    assert_eq!(
        voice.stretch.native_origin_tuple(),
        Some((render.pitch, render.key_lock, action.sequence()))
    );
    assert!(
        voice
            .stretch
            .owns_pitch_contract(render.pitch, render.key_lock)
    );
    assert!(!voice.stretch.source_preparation_pending());
}

fn render_with_peer(f: &mut Fixture, frames: usize) -> Vec<f32> {
    let (generation, source, native, mut playback) = voice_image(f, 1);
    assert!(playback.advance(frames));
    let pcm = f.render(frames);
    let after = voice_image(f, 1);
    assert_eq!((after.0, after.1, after.2), (generation, source, native));
    assert_eq!(after.3.position(), playback.position());
    assert_eq!(after.3.tempo_ratio(), 1.0);
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    if frames >= 128 {
        // This is the actual post-DSP peer contribution, before mixing the two PADs.
        assert!(f.state().pad_peaks[1] > 1e-6);
        assert!(pcm.iter().any(|sample| sample.abs() > 1e-6));
    }
    pcm
}

fn capture(f: &mut Fixture, retrigger: bool) -> CapturedPitchAction {
    let timestamp = Python::attach(|py| f.engine.borrow(py).input_clock.capture_ns());
    let state = f.state();
    // Deterministic software callback clock; this is not a device timestamp witness.
    state.callback_clock.publish(OutputClockSnapshot {
        valid: true,
        observed_at_ns: timestamp,
        audible_at_ns: timestamp,
        output_frame: state.transport.output_frame(),
        sample_rate_hz: RATE,
        master_period_seconds: state.transport.master_period_seconds(),
        downbeat_frame: state.transport.downbeat_frame(),
        master_beat: state.transport.beat_position().unwrap(),
        freshness_ns: 2_000_000_000,
    });
    f.script(&format!(
        "probe.piano_old_owners = set(probe.transport.pitch._owners)\nprobe.transport.pitch.choose_piano_note(0, 7, retrigger={}, received_at_ns={timestamp})\nprobe.piano_new_owners = set(probe.transport.pitch._owners) - probe.piano_old_owners\nassert len(probe.piano_new_owners) == 1, probe.transport.pitch.status(0)\nprobe.piano_capture = probe.transport.pitch._owners[probe.piano_new_owners.pop()].capture\nassert probe.piano_capture.content_is_current\nassert probe.piano_capture.received_at_ns == {timestamp}\nassert probe.piano_capture.base_shift == 7 and probe.piano_capture.extra_shift == 0\nassert probe.project.selected_pad == 1\nassert probe._audio.input_selected_projection()[1] == 1",
        if retrigger { "True" } else { "False" }
    ));
    Python::attach(|py| {
        f.probe
            .bind(py)
            .getattr("piano_capture")
            .unwrap()
            .extract::<PyRef<CapturedPitchAction>>()
            .unwrap()
            .clone()
    })
}

#[test]
#[ignore = "explicit full-source Piano controller bridge; no device or latency claim"]
fn actual_loader_piano_set_and_repeated_retrigger_preserve_peer() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut f = Fixture::new(root, false);
    Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/controller_overlap_probe.py"
            )))
            .unwrap(),
            c"controller_overlap_probe.py",
            c"controller_overlap_probe",
        )
        .unwrap();
        module
            .call_method1(
                "write_source",
                (root.join("source.wav").to_string_lossy().as_ref(), RATE, 3),
            )
            .unwrap();
    });
    let path = root.join("source.wav");
    f.load(&path);
    f.script(&format!("probe.load(1, {:?})", path.to_string_lossy()));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !script_value(&f, "probe.poll_load(1)") {
        f.drain();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    f.drain();
    let sources = [0, 1].map(|id| source_image(&mut f, id));
    f.script("probe.piano_sources = tuple((probe._audio.waveform_source_identity(i), probe._audio.loaded_source_digest(i), probe.source_history[i][-1]) for i in range(2))");
    start(&mut f, 0, true);
    settle(&mut f, &[0]);
    start(&mut f, 1, true);
    settle(&mut f, &[0, 1]);
    f.script("probe.select_pad(1)\nprobe.input_mapping.on_frame_render()\nassert probe.session.active_sample_ids == {0, 1}");
    render_with_peer(&mut f, 4096);
    let peer_birth = voice_image(&mut f, 1);
    let mut rows = Vec::new();
    let mut previous_sequence = 0;
    let mut pcm_hash = Sha256::new();
    for retrigger in [false, true, true] {
        let before = voice_image(&mut f, 0);
        let old_pitch = f.state().mixer.pad_pitch_intent(0);
        let action = capture(&mut f, retrigger);
        let original = (
            action.sequence(),
            action.content_key(),
            action.received_timestamp(),
            action.original_due_frame(),
        );
        assert!(original.0 > previous_sequence);
        previous_sequence = original.0;
        assert_eq!(
            action.variant(),
            if retrigger {
                PitchActionVariant::SetRetrigger
            } else {
                PitchActionVariant::Set
            }
        );
        assert!(!action.exclusive() && !action.sync());
        assert_eq!(action.loop_region().0, 0.0);
        f.drain();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let runtime = f.state().mixer.pitch_runtime.as_mut().unwrap();
            runtime.poll();
            assert_ne!(action.state(), "rejected", "{:?}", action.reason());
            if runtime.has_ready_action(action.sequence()) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "original Piano action never prepared"
            );
            thread::sleep(Duration::from_millis(1));
        }
        assert!(action.accepted() && !action.effective());
        render_with_peer(&mut f, 0);
        assert_eq!(f.state().mixer.pad_pitch_intent(0), old_pitch);
        let execution = action.execution_target();
        while f.state().transport.output_frame() < execution {
            let frame = f.state().transport.output_frame();
            let frames = (execution - frame).min(512) as usize;
            let mut expected = voice_image(&mut f, 0).3;
            assert!(expected.advance(frames));
            render_with_peer(&mut f, frames);
            let unchanged = voice_image(&mut f, 0);
            assert_eq!(
                (unchanged.0, unchanged.1, unchanged.2),
                (before.0, before.1, before.2)
            );
            assert_eq!(unchanged.3.position(), expected.position());
            assert_eq!(f.state().mixer.pad_pitch_intent(0), old_pitch);
            assert!(!action.effective(), "no effect before the action's own E");
        }
        let mut continued = voice_image(&mut f, 0).3;
        assert!(continued.advance(1));
        render_with_peer(&mut f, 1);
        assert!(action.effective(), "{:?}", action.reason());
        assert_adopted(&mut f, &action);
        let after = voice_image(&mut f, 0);
        if retrigger {
            assert_ne!(after.0, before.0);
            let position = after.3.position();
            assert_eq!((position.frame, position.fraction), (1, 0.0));
            assert_eq!(after.3.diagnostic_launch_reference().unwrap().1, original.3);
        } else {
            assert_eq!(after.0, before.0);
            assert_eq!(after.3.position(), continued.position());
        }
        assert_eq!(after.1, before.1);
        assert_eq!(after.3.tempo_ratio(), 1.0);
        assert_eq!(
            f.state().mixer.pad_pitch_intent(0),
            Some(action.render().pitch)
        );
        assert_eq!(action.execution_target(), execution);
        assert_eq!(
            (
                action.sequence(),
                action.content_key(),
                action.received_timestamp(),
                action.original_due_frame()
            ),
            original
        );
        f.script(&format!("assert probe.piano_capture.effective_output_frame == {execution}\nassert probe.piano_capture.execution_frame == {execution}"));
        let mut target_contributed = false;
        for _ in 0..16 {
            for sample in render_with_peer(&mut f, 512) {
                pcm_hash.update(sample.to_le_bytes());
            }
            target_contributed |= f.state().pad_peaks[0] > 1e-6;
        }
        assert!(
            target_contributed,
            "the pitched target must also contribute actual PCM"
        );
        assert_adopted(&mut f, &action);
        f.call("poll_feedback");
        f.script("probe.transport.on_frame_render()\nprobe.input_mapping.on_frame_render()\nassert probe.transport.pitch.status(0) == ('applied', None)\nassert probe.session.active_sample_ids == {0, 1}\nassert not probe.session.paused_sample_ids\nassert probe.project.selected_pad == 1\nassert probe._audio.input_selected_projection()[1] == 1\nassert tuple((probe._audio.waveform_source_identity(i), probe._audio.loaded_source_digest(i), probe.source_history[i][-1]) for i in range(2)) == probe.piano_sources\nassert all(probe._audio.current_input_runtime_pad_binding(i).is_current() for i in range(2))");
        rows.push(serde_json::json!({"retrigger": retrigger, "sequence": original.0,
            "received_ns": original.2, "T": original.3, "E": execution,
            "old_generation": before.0, "generation": after.0,
            "position_after_effect": [after.3.position().frame as f64, after.3.position().fraction]}));
    }
    let peer_end = voice_image(&mut f, 1);
    let delta =
        f.state().transport.output_frame() - peer_birth.3.diagnostic_launch_reference().unwrap().1;
    assert_eq!(
        (peer_end.0, peer_end.1, peer_end.2),
        (peer_birth.0, peer_birth.1, peer_birth.2)
    );
    assert!(delta > 4096);
    for (id, before) in sources.iter().enumerate() {
        assert_eq!(&source_image(&mut f, id), before);
    }
    f.script("probe.transport.playback.stop_all_pads()");
    f.drain();
    f.render(512);
    f.call("poll_feedback");
    f.script("assert not probe.session.active_sample_ids");
    let joined_counts = Python::attach(|py| {
        let mut engine = f.engine.borrow_mut(py);
        engine.shut_down().unwrap();
        engine.cold_jobs.counts_for_test()
    });
    assert_eq!(joined_counts, (0, 0, 0));
    f.close_callback();
    eprintln!(
        "piano_bridge_raw {}",
        serde_json::json!({"rate": RATE, "channels": 2,
        "frames_per_source": 3 * RATE, "sources": sources, "actions": rows,
        "peer_generation": peer_birth.0, "peer_post_dsp_contribution": true,
        "mixed_pcm_sha256": format!("{:x}", pcm_hash.finalize()),
        "joined_counts": joined_counts, "callback_retired": true,
        "limits": "One full-source controller bridge; no device, hearing, latency, NN, quality or full-matrix acceptance"})
    );
}

struct HeldTargetImage {
    lane: usize,
    generation: u64,
    pcm: usize,
    native: usize,
    paused: bool,
    playback: SourcePlayback,
    fifo: (usize, usize),
    pitch: Option<ProductiveSourceHistory>,
    dsp: Option<ProductiveSourceHistory>,
}

fn held_target(f: &mut Fixture) -> HeldTargetImage {
    let state = f.state();
    let (lane, voice) = state
        .mixer
        .voices
        .iter()
        .enumerate()
        .find(|(_, voice)| voice.is_playing_sample(0))
        .unwrap();
    HeldTargetImage {
        lane,
        generation: voice.generation,
        pcm: voice.sample.as_ref().unwrap().samples.as_ptr() as usize,
        native: voice.stretch.native_state_address(),
        paused: voice.paused,
        playback: voice.source_playback,
        fifo: voice.stretch.pending_fifo_frames(),
        pitch: voice.stretch.productive_history(),
        dsp: state.mixer.dsp_source_history_for_test(0),
    }
}

fn unchanged_target(before: &HeldTargetImage, after: &HeldTargetImage) {
    assert_eq!(after.lane, before.lane);
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.pcm, before.pcm);
    assert_eq!(after.native, before.native);
    assert_eq!(after.paused, before.paused);
    assert!(after.playback.matches_exact(&before.playback));
    assert_eq!(after.fifo, before.fifo);
    for (before, after) in [(before.pitch, after.pitch), (before.dsp, after.dsp)] {
        match (before, after) {
            (Some(before), Some(after)) => {
                assert_eq!(after.binding, before.binding);
                assert_eq!(after.next_position, before.next_position);
                assert_eq!(after.domain, before.domain);
                assert_eq!(after.fed_output_frames, before.fed_output_frames);
            }
            (None, None) => {}
            _ => panic!("Hold or old release changed productive history"),
        }
    }
}

#[test]
#[ignore = "explicit loaded physical Hold/Piano/original release bridge; no device claim"]
fn actual_loader_hold_retrigger_and_original_release_preserve_peer() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut f = Fixture::new(root, false);
    Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/controller_overlap_probe.py"
            )))
            .unwrap(),
            c"controller_overlap_probe.py",
            c"controller_overlap_probe",
        )
        .unwrap();
        module
            .call_method1(
                "write_source",
                (root.join("source.wav").to_string_lossy().as_ref(), RATE, 3),
            )
            .unwrap();
    });
    let path = root.join("source.wav");
    f.load(&path);
    f.script(&format!("probe.load(1, {:?})", path.to_string_lossy()));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !script_value(&f, "probe.poll_load(1)") {
        f.drain();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    f.drain();
    let sources = [0, 1].map(|id| source_image(&mut f, id));
    let markers = [0, 1].map(|id| f.state().mixer.loop_region_frames(id));
    f.script("probe.piano_sources = tuple((probe._audio.waveform_source_identity(i), probe._audio.loaded_source_digest(i), probe.source_history[i][-1]) for i in range(2))");
    start(&mut f, 0, true);
    settle(&mut f, &[0]);
    start(&mut f, 1, true);
    settle(&mut f, &[0, 1]);
    f.script("probe.select_pad(1)\nprobe.input_mapping.on_frame_render()");
    render_with_peer(&mut f, 4096);
    let ticket = f.press();
    let projection = || Python::attach(|py| ticket.borrow(py).action.projection());
    let admitted = projection();
    assert_eq!(admitted.state, HoldActionState::Admitted);
    assert!(admitted.effect.is_none());
    assert!(!held_target(&mut f).paused);
    let pause_frame = f.state().transport.output_frame();
    f.drain();
    let hold = projection();
    assert_eq!(hold.state, HoldActionState::PausedOwned);
    assert!(hold.effect_current);
    assert_eq!(hold.effect.unwrap().paused_at_output_frame, pause_frame);
    f.call("observe_owned");
    f.script("assert probe.project.selected_pad == 1\nassert probe._audio.input_selected_projection()[1] == 1");
    let frozen = held_target(&mut f);
    assert!(frozen.paused);
    for frames in [128, 385, 512] {
        render_with_peer(&mut f, frames);
        unchanged_target(&frozen, &held_target(&mut f));
        assert_eq!(f.state().pad_peaks[0], 0.0);
        assert_eq!(projection(), hold);
    }
    let old_pitch = f.state().mixer.pad_pitch_intent(0);
    let action = capture(&mut f, true);
    assert_eq!(action.variant(), PitchActionVariant::SetRetrigger);
    assert!(!action.exclusive() && !action.sync());
    let original = (
        action.sequence(),
        action.content_key(),
        action.received_timestamp(),
        action.original_due_frame(),
    );
    f.drain();
    let deadline = Instant::now() + Duration::from_secs(10);
    let prepared_native = loop {
        let runtime = f.state().mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        assert_ne!(action.state(), "rejected", "{:?}", action.reason());
        if let Some(native) = runtime.ready_pitch_native_address(action.sequence()) {
            break native;
        }
        assert!(
            Instant::now() < deadline,
            "original retrigger never prepared"
        );
        thread::sleep(Duration::from_millis(1));
    };
    assert!(action.accepted() && !action.effective());
    render_with_peer(&mut f, 0);
    let execution = action.execution_target();
    while f.state().transport.output_frame() < execution {
        let frames = (execution - f.state().transport.output_frame()).min(512) as usize;
        render_with_peer(&mut f, frames);
        unchanged_target(&frozen, &held_target(&mut f));
        assert_eq!(projection(), hold);
        assert_eq!(f.state().mixer.pad_pitch_intent(0), old_pitch);
        assert!(!action.effective(), "no effect before own E");
    }
    render_with_peer(&mut f, 1);
    assert!(action.effective(), "{:?}", action.reason());
    assert_adopted(&mut f, &action);
    let attack = held_target(&mut f);
    assert!(!attack.paused);
    assert_eq!(attack.generation, frozen.generation + 1);
    assert_eq!(attack.lane, frozen.lane);
    assert_eq!(attack.pcm, frozen.pcm);
    assert_eq!(attack.native, prepared_native);
    assert_ne!(attack.native, frozen.native);
    let position = attack.playback.position();
    assert_eq!((position.frame, position.fraction), (1, 0.0));
    assert_eq!(
        attack.playback.diagnostic_launch_reference().unwrap().1,
        original.3
    );
    let superseded = projection();
    assert_eq!(superseded.state, HoldActionState::Superseded);
    assert_eq!(superseded.settled_at_output_frame, execution);
    assert_eq!(superseded.effect, hold.effect);
    assert!(!superseded.effect_current);
    // The original physical owner is still registered: Up precedes terminal polling.
    let peer_before_release = voice_image(&mut f, 1);
    f.call("release");
    f.drain();
    f.call("poll_terminal");
    unchanged_target(&attack, &held_target(&mut f));
    assert_eq!(projection(), superseded);
    let peer_after_release = voice_image(&mut f, 1);
    assert_eq!(peer_after_release.0, peer_before_release.0);
    assert_eq!(peer_after_release.1, peer_before_release.1);
    assert_eq!(peer_after_release.2, peer_before_release.2);
    assert!(peer_after_release.3.matches_exact(&peer_before_release.3));
    let mut pcm_hash = Sha256::new();
    let mut target_contributed = false;
    for frames in [73, 511, 127, 512]
        .into_iter()
        .chain(std::iter::repeat_n(512, 16))
    {
        let mut expected = held_target(&mut f).playback;
        assert!(expected.advance(frames));
        for sample in render_with_peer(&mut f, frames) {
            pcm_hash.update(sample.to_le_bytes());
        }
        let after = held_target(&mut f);
        assert!(!after.paused);
        assert_eq!(after.generation, attack.generation);
        assert_eq!(after.native, prepared_native);
        assert!(after.playback.matches_exact(&expected));
        assert_adopted(&mut f, &action);
        target_contributed |= f.state().pad_peaks[0] > 1e-6;
    }
    assert!(target_contributed);
    assert_eq!(action.execution_target(), execution);
    assert_eq!(
        (
            action.sequence(),
            action.content_key(),
            action.received_timestamp(),
            action.original_due_frame()
        ),
        original
    );
    f.call("poll_feedback");
    f.script(&format!("probe.transport.on_frame_render()\nprobe.input_mapping.on_frame_render()\nassert probe.piano_capture.effective_output_frame == {execution}\nassert probe.piano_capture.execution_frame == {execution}\nassert probe.transport.pitch.status(0) == ('applied', None)\nassert probe.session.active_sample_ids == {{0, 1}}\nassert not probe.session.paused_sample_ids\nassert probe.project.selected_pad == 1\nassert probe._audio.input_selected_projection()[1] == 1\nassert tuple((probe._audio.waveform_source_identity(i), probe._audio.loaded_source_digest(i), probe.source_history[i][-1]) for i in range(2)) == probe.piano_sources\nassert all(probe._audio.current_input_runtime_pad_binding(i).is_current() for i in range(2))"));
    for (id, before) in sources.iter().enumerate() {
        assert_eq!(&source_image(&mut f, id), before);
        assert_eq!(f.state().mixer.loop_region_frames(id), markers[id]);
    }
    f.script("probe.transport.playback.stop_all_pads()");
    f.drain();
    f.render(512);
    f.call("poll_feedback");
    f.script("assert not probe.session.active_sample_ids");
    let joined_counts = Python::attach(|py| {
        let mut engine = f.engine.borrow_mut(py);
        engine.shut_down().unwrap();
        engine.cold_jobs.counts_for_test()
    });
    assert_eq!(joined_counts, (0, 0, 0));
    f.close_callback();
    eprintln!(
        "hold_retrigger_bridge_raw {}",
        serde_json::json!({
            "rate": RATE, "channels": 2, "frames_per_source": 3 * RATE,
            "sources": sources, "hold_action": hold.action_id,
            "hold_physical_press": hold.physical_press_id, "pause_frame": pause_frame,
            "sequence": original.0, "received_ns": original.2, "T": original.3, "E": execution,
            "old_generation": frozen.generation, "new_generation": attack.generation,
            "position_after_effect": [position.frame as f64, position.fraction],
            "exact_native_adoption": true, "old_release_preserved_attack": true,
            "target_actual_pcm": target_contributed, "peer_actual_post_dsp": true,
            "mixed_pcm_sha256": format!("{:x}", pcm_hash.finalize()),
            "joined_counts": joined_counts, "callback_retired": true,
            "limits": "One loaded Hold/retrigger/release bridge; no device, hearing, latency, NN, quality or full-matrix acceptance"
        })
    );
}
