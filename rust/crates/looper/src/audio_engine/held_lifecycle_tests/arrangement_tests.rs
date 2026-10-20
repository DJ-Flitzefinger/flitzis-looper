//! R3 rendered pad IO -> original gesture binding -> actual Pair/Copy -> own ACK.
//! Every profile owns its input/config/cwd and the production device-free callback.

use super::*;
use crate::audio_engine::content_placement::{ContentKey, SlotSnapshot};

const ARRANGEMENT_ROOT: &str = "FLITZI_R3_ARRANGEMENT_ROOT";
const ARRANGEMENT_PROFILE: &str = "FLITZI_R3_ARRANGEMENT_PROFILE";
const PAIR_APP_PROFILE: &str = "FLITZI_REARRANGE_PAIR_APP_PROFILE";

#[derive(Debug, PartialEq)]
struct ArrangementLane {
    lane: usize,
    key: ContentKey,
    generation: u64,
    paused: bool,
    position: FractionalSourcePosition,
    pcm: usize,
    native: usize,
}

impl ArrangementLane {
    fn capture(fixture: &mut Fixture, slot: usize) -> Self {
        let (lane, voice) = fixture
            .state()
            .mixer
            .voices
            .iter()
            .enumerate()
            .find(|(_, voice)| voice.is_playing_sample(slot))
            .unwrap();
        Self {
            lane,
            key: voice
                .source_admission
                .as_ref()
                .unwrap()
                .content_key()
                .unwrap(),
            generation: voice.generation,
            paused: voice.paused,
            position: voice.source_playback.position(),
            pcm: voice.sample.as_ref().unwrap().samples.as_ptr() as usize,
            native: voice.stretch.native_state_address(),
        }
    }

    fn assert_at(&self, fixture: &mut Fixture, slot: usize) {
        assert_eq!(Self::capture(fixture, slot), *self);
    }
}

impl Fixture {
    fn arrangement_slots(&self) -> [SlotSnapshot; 3] {
        Python::attach(|py| {
            let engine = self.engine.borrow(py);
            std::array::from_fn(|slot| engine.input_runtime_ownership.slot_snapshot(slot).unwrap())
        })
    }

    fn arrangement_frame(&self, hover: i32, left: bool, right: bool, middle: bool) {
        let flag = |value| if value { "True" } else { "False" };
        self.script(&format!(
            "probe.render_arrangement_frame({hover}, left={}, right={}, middle={})",
            flag(left),
            flag(right),
            flag(middle)
        ));
    }

    fn arrangement_press(&self, copy: bool, hold_and_mute: bool) {
        self.arrangement_frame(0, !copy, copy || hold_and_mute, hold_and_mute);
        self.script("assert probe.render_binding is not None\nassert probe.render_binding.is_current()\nassert probe.join_context.ui.arrangement.source_id == 0\nassert probe.join_context.ui.arrangement.owns_pointer\nassert not any(probe.session.pressed_pads)");
    }

    fn arrangement_drop(&self, target: usize, operation: &str, hold_and_mute: bool) {
        let copy = operation.starts_with("Copy");
        self.arrangement_frame(target as i32, !copy, copy || hold_and_mute, hold_and_mute);
        Python::attach(|py| {
            self.probe
                .call_method1(py, "assert_arrangement_preview", (target, operation))
                .unwrap();
        });
        self.arrangement_frame(target as i32, false, hold_and_mute, hold_and_mute);
        self.script("assert probe.render_owner is not None\nassert probe.render_owner.source_binding is probe.render_binding\nassert probe.join_context.ui.arrangement.source_id is None\nassert not any(probe.session.pressed_pads)");
    }

    fn arrangement_publish(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let published = Python::attach(|py| {
                self.probe
                    .call_method0(py, "poll_arrangement_published")
                    .unwrap()
                    .extract::<bool>(py)
                    .unwrap()
            });
            if published {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "rendered arrangement never published its actual worker result"
            );
            thread::sleep(Duration::from_millis(1));
        }
        self.script("assert probe.render_owner.ticket.publication_status() == 'pending'\nassert probe.render_owner.receipt is None\nassert probe.persistence.config_path.read_bytes() == probe.render_config");
    }

    fn arrangement_accept(&mut self) {
        assert_eq!(
            self.drain(),
            1,
            "one actual rendered gesture must publish one complete Native command"
        );
        self.script("owner = probe.render_owner\nassert owner.ticket.publication_status() == 'accepted'\nassert owner.receipt is None\nassert not owner.native_finished\nassert probe.project == probe.render_project\nassert probe.persistence.config_path.read_bytes() == probe.render_config\nif probe.pairs._pair is not None:\n    assert owner.ticket.own_acknowledgement() is not None\n    assert owner.acknowledgement is None\nelse:\n    assert not owner.acknowledged");
        self.arrangement_finish();
    }

    fn arrangement_finish(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let finished = Python::attach(|py| {
                self.probe
                    .call_method0(py, "poll_arrangement_finished")
                    .unwrap()
                    .extract::<bool>(py)
                    .unwrap()
            });
            if finished {
                break;
            }
            self.drain();
            assert!(
                Instant::now() < deadline,
                "original rendered arrangement did not finish its own ACK/config/custody"
            );
            thread::sleep(Duration::from_millis(1));
        }
        self.script("assert probe.persistence._migration_owner is None\nassert probe.render_owner.retirement.remaining == 0\nassert not probe.render_owner.images\nassert not probe.pairs.active\nassert not probe.arrangement.active\nassert probe.saved_bytes == probe.persistence.config_path.read_bytes()");
        self.drain();
    }
}

fn arrangement_fixture(root: &Path) -> (Fixture, PathBuf) {
    let source = write_input(root);
    let peer = source.with_file_name("peer.wav");
    let mut bytes = fs::read(&source).unwrap();
    for sample in bytes[44..].chunks_exact_mut(2) {
        let value = i16::from_le_bytes([sample[0], sample[1]]) / 2;
        sample.copy_from_slice(&value.to_le_bytes());
    }
    fs::write(&peer, bytes).unwrap();
    let mut fixture = Fixture::new(root, false);
    fixture.load_join_slot(0, &source);
    fixture.load_join_slot(1, &peer);
    fixture.start_join_slot(0);
    fixture.start_join_slot(1);
    (fixture, source)
}

fn operation_profile(root: &Path, operation: &str, target: usize) {
    let (mut fixture, _) = arrangement_fixture(root);
    fixture.script("probe.transport.playback.pause_pad(1)");
    fixture.drain();
    fixture.call("setup_arrangement_ui");
    fixture.drain();
    let slots = fixture.arrangement_slots();
    let a = ArrangementLane::capture(&mut fixture, 0);
    let b = ArrangementLane::capture(&mut fixture, 1);
    assert!(!a.paused && b.paused);
    fixture.arrangement_press(operation.starts_with("Copy"), false);
    a.assert_at(&mut fixture, 0);
    b.assert_at(&mut fixture, 1);
    fixture.arrangement_drop(target, operation, false);
    fixture.arrangement_publish();
    a.assert_at(&mut fixture, 0);
    b.assert_at(&mut fixture, 1);
    fixture.arrangement_accept();
    let after = fixture.arrangement_slots();
    match operation {
        "Move" => {
            assert_eq!(after[0].content_key(), None);
            assert_eq!(after[2].content_key(), slots[0].content_key());
            assert_eq!(after[1].content_key(), slots[1].content_key());
            a.assert_at(&mut fixture, 2);
            b.assert_at(&mut fixture, 1);
            fixture.script("assert probe.project.pad_content[0] is None\nassert probe.project.pad_content[2] == probe.render_project.pad_content[0]\nassert probe.project.pad_content[1] == probe.render_project.pad_content[1]\nassert probe.session.active_sample_ids == {1, 2}\nassert probe.session.paused_sample_ids == {1}");
        }
        "Swap" => {
            assert_eq!(after[0].content_key(), slots[1].content_key());
            assert_eq!(after[1].content_key(), slots[0].content_key());
            a.assert_at(&mut fixture, 1);
            b.assert_at(&mut fixture, 0);
            fixture.script("assert probe.project.pad_content[0] == probe.render_project.pad_content[1]\nassert probe.project.pad_content[1] == probe.render_project.pad_content[0]\nassert probe.session.active_sample_ids == {0, 1}\nassert probe.session.paused_sample_ids == {0}");
        }
        "Copy" | "Copy overwrite" => {
            assert_eq!(after[0].content_key(), slots[0].content_key());
            assert!(after[target].content_key().is_some());
            assert_ne!(after[target].content_key(), slots[0].content_key());
            assert_ne!(after[target].content_key(), slots[1].content_key());
            a.assert_at(&mut fixture, 0);
            assert!(fixture.state().mixer.bank_for_measurement()[target].is_some());
            assert!(
                !fixture
                    .state()
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(target))
            );
            fixture.script(&format!("target = {target}\nassert probe.project.pad_content[0] == probe.render_project.pad_content[0]\nassert probe.project.pad_content[target].instance_id != probe.render_project.pad_content[0].instance_id\nassert probe._audio.loaded_source_digest(target) == probe._audio.loaded_source_digest(0)\nprobe.assert_stopped(target)"));
            if target == 2 {
                assert_eq!(after[1].content_key(), slots[1].content_key());
                b.assert_at(&mut fixture, 1);
                fixture.script("assert probe.session.active_sample_ids == {0, 1}\nassert probe.session.paused_sample_ids == {1}");
            } else {
                fixture.script("assert probe.project.pad_content[1].instance_id != probe.render_project.pad_content[1].instance_id\nassert probe.session.active_sample_ids == {0}\nassert not probe.session.paused_sample_ids");
            }
        }
        _ => unreachable!(),
    }
    let output = fixture.render(96);
    assert!(output.iter().all(|sample| sample.is_finite()));
    assert!(output.iter().any(|sample| sample.abs() > 1e-6));
    fixture.close_callback();
    fixture.call("close");
}

fn pair_app_multi_loop_after_ack_profile(root: &Path, target: usize) {
    let (mut fixture, _) = arrangement_fixture(root);
    fixture.script("probe.transport.playback.pause_pad(1)");
    fixture.drain();
    fixture.call("setup_arrangement_ui");
    fixture.drain();
    let a = ArrangementLane::capture(&mut fixture, 0);
    let b = ArrangementLane::capture(&mut fixture, 1);
    fixture.arrangement_press(false, false);
    fixture.arrangement_drop(target, if target == 1 { "Swap" } else { "Move" }, false);
    fixture.arrangement_publish();
    assert_eq!(
        fixture.drain(),
        1,
        "Pair must reach its actual callback ACK first"
    );

    let after = fixture.arrangement_slots();
    let revisions = Python::attach(|py| {
        let engine = fixture.engine.borrow(py);
        assert!(engine.input_runtime.is_some());
        [0, target].map(|slot| {
            assert_ne!(engine.input_runtime_ownership.pair_reservation(slot), 0);
            engine
                .input_runtime_ownership
                .runtime_for_cell(after[slot].cell())
                .load(Ordering::Acquire)
        })
    });
    // The real UiContext action and InputMapping frame callback run before App
    // observes the ACK, while Python still owns its original reserved rows.
    fixture.call("toggle_multi_loop_after_pair_ack");
    Python::attach(|py| {
        let engine = fixture.engine.borrow(py);
        for (slot, revision) in [0, target].into_iter().zip(revisions) {
            assert_eq!(
                engine
                    .input_runtime_ownership
                    .runtime_for_cell(after[slot].cell())
                    .load(Ordering::Acquire),
                revision + 1,
                "the actual global runtime publication must advance each content cell"
            );
            assert_ne!(engine.input_runtime_ownership.pair_reservation(slot), 0);
        }
    });

    fixture.arrangement_finish();
    a.assert_at(&mut fixture, target);
    b.assert_at(&mut fixture, if target == 1 { 0 } else { 1 });
    Python::attach(|py| {
        let engine = fixture.engine.borrow(py);
        for slot in [0, target] {
            assert_eq!(engine.input_runtime_ownership.pair_reservation(slot), 0);
        }
    });
    fixture.call("assert_pair_app_close_ready");
    fixture.close_callback();
    fixture.call("close");
}

fn stale_source_profile(root: &Path) {
    let (mut fixture, source) = arrangement_fixture(root);
    fixture.script("probe.transport.playback.pause_pad(1)");
    fixture.drain();
    fixture.call("setup_arrangement_ui");
    fixture.drain();
    let original = fixture.source();
    fixture.arrangement_press(false, false);
    fixture.load_join_slot(0, &source);
    let replacement = fixture.source();
    assert_eq!(replacement.reference, original.reference);
    assert_eq!(replacement.sha256, original.sha256);
    assert_ne!(replacement.content, original.content);
    assert_ne!(replacement.generation, original.generation);
    assert_ne!(replacement.lifetime, original.lifetime);
    let before = fixture.arrangement_slots();
    let request = fixture.request_image();
    let b = ArrangementLane::capture(&mut fixture, 1);
    fixture.script("probe.save()\nprobe._stale_drop_project = probe.project.model_copy(deep=True)\nprobe._stale_drop_session = probe.session.model_copy(deep=True)\nprobe._stale_drop_config = probe.saved_bytes\nprobe._stale_drop_revision = probe.persistence.revision\nassert not probe.render_binding.is_current()");
    fixture.arrangement_frame(2, true, false, false);
    fixture.script("assert probe.join_context.ui.arrangement.preview(2) is None");
    fixture.arrangement_frame(2, false, false, false);
    fixture.call("assert_cancelled_rendered_drop");
    assert_eq!(
        fixture.drain(),
        0,
        "stale rendered drag emitted a fresh content mutation"
    );
    assert_eq!(fixture.arrangement_slots(), before);
    assert_eq!(fixture.request_image(), request);
    b.assert_at(&mut fixture, 1);
    assert!(
        !fixture
            .state()
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0) || voice.is_playing_sample(2))
    );
    fixture.script("assert probe.project == probe._stale_drop_project\nassert probe.session == probe._stale_drop_session\nassert probe.persistence.revision == probe._stale_drop_revision\nassert probe.persistence.config_path.read_bytes() == probe._stale_drop_config\nprobe.assert_stopped(0)");
    fixture.close_callback();
    fixture.call("close");
}

fn mute_hold_profile(root: &Path) {
    let (mut fixture, _) = arrangement_fixture(root);
    fixture.call("setup_arrangement_ui");
    let hold = Python::attach(|py| {
        fixture
            .probe
            .call_method0(py, "press_arrangement_hold")
            .unwrap()
            .extract::<Py<HoldActionTicket>>(py)
            .unwrap()
    });
    assert_eq!(fixture.drain(), 1);
    fixture.call("observe_owned");
    let a = ArrangementLane::capture(&mut fixture, 0);
    let b = ArrangementLane::capture(&mut fixture, 1);
    assert!(a.paused && !b.paused);
    let effect = Python::attach(|py| hold.borrow(py).action.effect().unwrap());
    assert_eq!(effect.key, a.key);
    assert_eq!(effect.changed_mask, 1 << a.lane);
    fixture.arrangement_frame(-1, false, true, true);
    fixture.script("probe.join_context.audio.global_.set_momentary_output_mute(enabled=True)\nassert probe.session.global_stop_momentary_mute_active");
    fixture.drain();
    fixture.arrangement_press(false, true);
    fixture.arrangement_drop(2, "Move", true);
    fixture.arrangement_publish();
    fixture.arrangement_accept();
    // Pair finish releases the reservation after this App tick's Hold projection.
    // Observe its new slot at the next real global tick with both buttons still held.
    fixture.arrangement_frame(-1, false, true, true);
    a.assert_at(&mut fixture, 2);
    b.assert_at(&mut fixture, 1);
    fixture.script("assert probe.project.pad_content[0] is None\nassert probe.project.pad_content[2] == probe.render_project.pad_content[0]\nassert probe.session.active_sample_ids == {1, 2}\nassert probe.session.paused_sample_ids == set()\nassert probe.session.waveform_pause_hold_pad_id == 2\nassert probe.session.waveform_editor_pad_id == 2\nassert probe.owner.ticket is probe.hold\nassert probe.hold.publication_status() == 'paused_owned'\nassert not probe.owner.release_requested\nassert probe.transport.waveform._physical_hold_press == probe.owner.physical_press_id");
    for volume in [0.2, 1.7, -0.5, 0.43] {
        fixture.script(&format!("probe.join_context.audio.global_.set_volume({volume})\nassert probe.session.global_stop_momentary_mute_active"));
        fixture.drain();
        let output = fixture.render(64);
        assert!(
            output
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() < 1e-8),
            "volume edit escaped the actual global output mute"
        );
        fixture.call("poll_feedback");
        assert_eq!(ArrangementLane::capture(&mut fixture, 2), a);
        Python::attach(|py| {
            assert_eq!(hold.borrow(py).action.state(), HoldActionState::PausedOwned)
        });
    }
    fixture.script("assert probe.project.volume == 0.43\nassert probe.persistence._dirty\nprobe.save()\nassert not probe.persistence._dirty");
    fixture.arrangement_frame(-1, false, true, false);
    fixture.script("assert not probe.session.global_stop_momentary_mute_active\nassert probe.project.volume == 0.43\nassert probe.hold.publication_status() == 'paused_owned'\nassert not probe.owner.release_requested");
    fixture.drain();
    a.assert_at(&mut fixture, 2);
    let output = fixture.render(96);
    assert!(output.iter().all(|sample| sample.is_finite()));
    assert!(
        output.iter().any(|sample| sample.abs() > 1e-6),
        "Middle-UP did not restore the real continuing peer output"
    );
    assert_eq!(
        fixture
            .state()
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.is_playing_sample(1) && !voice.paused)
            .count(),
        1
    );
    fixture.arrangement_frame(-1, false, false, false);
    assert_eq!(
        fixture.drain(),
        1,
        "Right-UP must release exactly the original moved H"
    );
    let resumed = ArrangementLane::capture(&mut fixture, 2);
    assert!(!resumed.paused);
    assert_eq!(resumed.key, a.key);
    assert_eq!(resumed.lane, a.lane);
    assert_eq!(resumed.generation, a.generation);
    assert_eq!(resumed.native, a.native);
    assert_eq!(resumed.pcm, a.pcm);
    assert_eq!(resumed.position, a.position);
    Python::attach(|py| {
        let ticket = hold.borrow(py);
        assert_eq!(ticket.action.state(), HoldActionState::Resumed);
        assert_eq!(ticket.action.effect(), Some(effect));
    });
    fixture.arrangement_frame(-1, false, false, false);
    fixture.call("poll_terminal");
    fixture.script("assert probe.session.active_sample_ids == {1, 2}\nassert not probe.session.paused_sample_ids\nassert probe.session.waveform_pause_hold_pad_id is None");
    assert_eq!(
        fixture.drain(),
        0,
        "outside runtime tick duplicated a terminal release"
    );
    fixture.close_callback();
    fixture.call("close");
}

#[test]
fn real_pair_ack_multi_loop_settles_config_selection_and_app_close() {
    if let Some(profile) = std::env::var_os(PAIR_APP_PROFILE) {
        let target: usize = profile.to_str().unwrap().parse().unwrap();
        assert!([1, 2].contains(&target));
        let root = PathBuf::from(std::env::var_os(ARRANGEMENT_ROOT).unwrap());
        pair_app_multi_loop_after_ack_profile(&root, target);
        eprintln!(
            "Pair App after-ACK Multi Loop child target={target} PID={}",
            std::process::id()
        );
        return;
    }
    let parent = std::env::var_os(ARRANGEMENT_ROOT).map(PathBuf::from).unwrap_or_else(|| {
        let temporary = std::env::temp_dir().join("codex");
        fs::create_dir_all(&temporary).unwrap();
        let owned = tempfile::Builder::new()
            .prefix("rearrange-pair-app-")
            .tempdir_in(temporary)
            .unwrap()
            .keep();
        fs::write(
            owned.join("OWNER.md"),
            "Owner: device-free real Pair ACK/Multi Loop/Config/App-close proof. Reproducible synthetic outputs; preserve results before cleanup.\n",
        )
        .unwrap();
        owned
    });
    assert!(parent.is_dir() && parent.join("OWNER.md").is_file());
    let (_, module) = module_path!().split_once("::").unwrap();
    let test = format!("{module}::real_pair_ack_multi_loop_settles_config_selection_and_app_close");
    for target in [1, 2] {
        let root = tempfile::Builder::new()
            .prefix(&format!("pair-app-target-{target}-"))
            .tempdir_in(&parent)
            .unwrap()
            .keep();
        fs::write(
            root.join("OWNER.md"),
            "Owner: isolated real Pair/App child; synthetic input, no device or Human data.\n",
        )
        .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test, "--nocapture", "--test-threads=1"])
            .env(PAIR_APP_PROFILE, target.to_string())
            .env(ARRANGEMENT_ROOT, &root)
            .current_dir(&root)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprint!("{stderr}");
        assert!(
            output.status.success(),
            "Pair App target={target} failed:\n{stdout}\n{stderr}"
        );
        assert!(stdout.contains("1 passed; 0 failed"));
        assert!(stderr.contains(&format!(
            "Pair App after-ACK Multi Loop child target={target} PID="
        )));
    }
}

#[test]
fn rendered_pad_arrangement_uses_original_native_owners() {
    if let Some(profile) = std::env::var_os(ARRANGEMENT_PROFILE) {
        let profile = profile.to_str().unwrap();
        let root = PathBuf::from(std::env::var_os(ARRANGEMENT_ROOT).unwrap());
        match profile {
            "move" => operation_profile(&root, "Move", 2),
            "swap" => operation_profile(&root, "Swap", 1),
            "copy" => operation_profile(&root, "Copy", 2),
            "overwrite" => operation_profile(&root, "Copy overwrite", 1),
            "stale-source" => stale_source_profile(&root),
            "mute-hold" => mute_hold_profile(&root),
            _ => panic!("unknown R3 rendered arrangement profile: {profile}"),
        }
        eprintln!(
            "R3 rendered arrangement child profile={profile} PID={}",
            std::process::id()
        );
        return;
    }
    let parent = std::env::var_os(ARRANGEMENT_ROOT).map(PathBuf::from).unwrap_or_else(|| {
        let temporary = std::env::temp_dir().join("codex");
        fs::create_dir_all(&temporary).unwrap();
        let owned = tempfile::Builder::new().prefix("r3-arrange-ui-").tempdir_in(temporary).unwrap().keep();
        fs::write(owned.join("OWNER.md"), "Owner: device-free R3 actual rendered arrangement fixture. Reproducible outputs; preserve results before cleanup.\n").unwrap();
        owned
    });
    assert!(parent.is_dir() && parent.join("OWNER.md").is_file());
    let (_, module) = module_path!().split_once("::").unwrap();
    let test = format!("{module}::rendered_pad_arrangement_uses_original_native_owners");
    for profile in [
        "move",
        "swap",
        "copy",
        "overwrite",
        "stale-source",
        "mute-hold",
    ] {
        let root = tempfile::Builder::new()
            .prefix(&format!("arrange-{profile}-"))
            .tempdir_in(&parent)
            .unwrap()
            .keep();
        fs::write(
            root.join("OWNER.md"),
            "Owner: isolated R3 rendered UI/Native child; no App/device/private input.\n",
        )
        .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test, "--nocapture", "--test-threads=1"])
            .env(ARRANGEMENT_PROFILE, profile)
            .env(ARRANGEMENT_ROOT, &root)
            .current_dir(&root)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprint!("{stderr}");
        assert!(
            output.status.success(),
            "R3 rendered arrangement {profile} failed:\n{stdout}\n{stderr}"
        );
        assert!(stdout.contains("1 passed; 0 failed"));
        assert!(stderr.contains(&format!(
            "R3 rendered arrangement child profile={profile} PID="
        )));
    }
}
