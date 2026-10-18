//! Actual Python Hold/Pair owners through Native callback, Config, mirrors and finish.
//! Each bounded stereo FullMix profile owns a child process and synthetic files.
//! This closes the named integration seam, not full R2/resource/music/Human gates.

use super::*;
use crate::messages::ControlMessage;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use std::ffi::CString;
use std::path::{Path, PathBuf};

const PROFILE: &str = "FLITZI_R2_CONTROLLER_HOLD_PROFILE";
const ROOT: &str = "FLITZI_R2_CONTROLLER_HOLD_ROOT";
const TEST: &str = "audio_engine::pad_pair::control_hold_tests::controller_native_tests::real_controllers_hold_pair_durable_finish_both_release_orders";
const PROBE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../src/tests/flitzis_looper/controller/pair_native_probe.py"
));

struct ControllerRoute {
    engine: Py<crate::audio_engine::AudioEngine>,
    probe: Py<PyAny>,
    consumer: rtrb::Consumer<ControlMessage>,
    mixer: Box<RtMixer>,
    scheduler: FixedCapacityScheduler<8>,
    transport: TransportTimeline,
    quantization: TriggerQuantization,
    feedback: Vec<AudioMessage>,
    retirement: RetirementGate,
    _worker: AudioBufferRetirementWorker,
}

impl ControllerRoute {
    fn new(root: &Path, target: usize) -> Self {
        // Public _prepare_pad_pair has the real no-device stereo default. Keep
        // that method in the Controller route instead of a mono preparation shim.
        let fixture = CallbackFixture::new_with_channels(8, 2);
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let (engine, probe) = Python::attach(|py| {
            py.import("sys")
                .unwrap()
                .getattr("path")
                .unwrap()
                .call_method1(
                    "insert",
                    (0, repository.join("src").to_string_lossy().as_ref()),
                )
                .unwrap();
            let engine = Py::new(py, fixture.control.engine).unwrap();
            let module = PyModule::from_code(
                py,
                &CString::new(PROBE).unwrap(),
                c"pair_native_probe.py",
                c"pair_native_probe",
            )
            .unwrap();
            let probe = module
                .getattr("ControllerProbe")
                .unwrap()
                .call1((
                    engine.clone_ref(py),
                    root.to_string_lossy().as_ref(),
                    target,
                ))
                .unwrap()
                .unbind();
            (engine, probe)
        });
        Self {
            engine,
            probe,
            consumer: fixture.control.consumer,
            mixer: fixture.mixer,
            scheduler: fixture.scheduler,
            transport: fixture.transport,
            quantization: fixture.quantization,
            feedback: fixture.feedback,
            retirement: fixture.retirement,
            _worker: fixture._worker,
        }
    }

    fn call(&self, method: &str) {
        Python::attach(|py| {
            self.probe.call_method0(py, method).unwrap();
        });
    }

    fn ticket<T: pyo3::PyClass>(&self, method: &str) -> Py<T> {
        Python::attach(|py| {
            self.probe
                .call_method0(py, method)
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn drain(&mut self) -> usize {
        drain_control_messages(
            &mut self.consumer,
            &mut self.scheduler,
            self.transport.output_frame(),
            &mut self.quantization,
            &mut self.transport,
            &mut self.mixer,
            &mut self.feedback,
            &mut self.retirement,
        )
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        let frame = self.transport.output_frame();
        render_scheduled_audio(
            &mut self.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; crate::audio_engine::constants::NUM_SAMPLES],
            frame,
            2,
            &mut self.transport,
            &mut self.feedback,
            &mut self.retirement,
        );
        self.transport.advance_by_rendered_frames(frames);
        output
    }
}

fn run_profile(root: &Path, target: usize, early: bool, release_first: bool) {
    let mut route = ControllerRoute::new(root, target);
    let mut continuing = CallbackFixture::new_with_channels(8, 2);
    let hold: Py<HoldActionTicket> = route.ticket("press");
    Python::attach(|py| {
        assert_eq!(hold.borrow(py).publication_status(), "pause_pending");
        assert!(hold.borrow(py).action.effect().is_none());
    });
    if !early {
        assert_eq!(route.drain(), 1);
        route.call("observe_owned");
    }
    let source = LaneEvidence::capture(&route.mixer, SOURCE);
    let other = LaneEvidence::capture(&route.mixer, OTHER);
    assert!(route.mixer.voices[other.lane].paused);
    route.call("begin");
    if release_first {
        route.call("release");
    }
    let ticket: Py<PadPairTicket> = route.ticket("publish");
    if !release_first {
        route.call("release");
    }
    Python::attach(|py| {
        assert!(hold.borrow(py).release_requested());
        assert!(hold.borrow(py).release_admitted());
        assert!(ticket.borrow(py).own_acknowledgement().is_none());
    });
    assert_eq!(route.drain(), if early { 3 } else { 2 });
    let receipt = Python::attach(|py| {
        let hold = hold.borrow(py);
        assert_eq!(hold.action.state(), HoldActionState::Resumed);
        let effect = hold.action.effect().unwrap();
        assert_eq!(effect.key, source.key);
        assert_eq!(effect.cohort_mask, 1 << source.lane);
        assert_eq!(effect.changed_mask, 1 << source.lane);
        assert_eq!(effect.generations[source.lane], source.generation);
        let receipt = ticket.borrow(py).native_acknowledgement().unwrap();
        assert_eq!(
            receipt.kind,
            if target == EMPTY {
                PairPlacementKind::Move
            } else {
                PairPlacementKind::Swap
            }
        );
        assert_eq!(receipt.target_after.lifetime, source.key.lifetime());
        if release_first {
            assert!(receipt.holds[0].is_none());
        } else {
            let carried = receipt.holds[0].unwrap();
            assert_eq!(carried.action_id, hold.action.action_id());
            assert_eq!(carried.effect, Some(effect));
            assert!(carried.effect_current);
            assert_eq!(carried.state, HoldActionState::PausedOwned);
        }
        assert!(receipt.holds[1].is_none());
        receipt
    });
    source.assert_retained(&route.mixer, target, if early { 2 } else { 1 });
    let other_after = if target == EMPTY { OTHER } else { SOURCE };
    other.assert_retained(&route.mixer, other_after, 0);
    assert!(!route.mixer.voices[source.lane].paused);
    assert!(route.mixer.voices[other.lane].paused);

    // Real Python settlement owns reconciliation, prepared mirrors, actual file
    // receipts/journal, Native asset leases and finish. No direct finish shortcut.
    route.call("settle");
    route.call("poll_terminal");
    Python::attach(|py| {
        let engine = route.engine.borrow(py);
        for slot in [SOURCE, target] {
            assert_eq!(engine.input_runtime_ownership.pair_reservation(slot), 0);
        }
        assert_eq!(hold.borrow(py).current_control_slot(), None);
        assert_eq!(ticket.borrow(py).native_acknowledgement(), Some(receipt));
    });
    source.assert_retained(&route.mixer, target, if early { 2 } else { 1 });
    other.assert_retained(&route.mixer, other_after, 0);
    assert_eq!(route.drain(), 0);
    let pcm = route.render(96);
    let expected = continuing.render(96);
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(
        pcm, expected,
        "layout/release replaced continuing PCM history"
    );
    route.call("close");
    eprintln!(
        "R2 actual controllers target={target} release_before_pause_ack={early} release_first={release_first}: own ACK, Config/journal/leases/mirrors/finish and 96-frame stereo PCM equal"
    );
}

#[test]
fn real_controllers_hold_pair_durable_finish_both_release_orders() {
    if let Some(profile) = std::env::var_os(PROFILE) {
        let profile = profile.to_str().unwrap();
        let parts: Vec<_> = profile.split(',').collect();
        assert_eq!(parts.len(), 3);
        let root = PathBuf::from(std::env::var_os(ROOT).unwrap());
        run_profile(
            &root,
            parts[0].parse().unwrap(),
            parts[1] == "true",
            parts[2] == "true",
        );
        return;
    }
    let parent = std::env::var_os("FLITZI_R2_HOLD_PARENT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("codex"));
    std::fs::create_dir_all(&parent).unwrap();
    for target in [OTHER, EMPTY] {
        for early in [false, true] {
            for release_first in [false, true] {
                let root = tempfile::Builder::new()
                    .prefix("r2-controller-")
                    .tempdir_in(&parent)
                    .unwrap()
                    .keep();
                std::fs::write(root.join("OWNER.md"), "Owner: device-free R2 Controller Native Hold/Pair fixture. Reproducible owned outputs; clean after terminal delivery.\n").unwrap();
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
                    .env(PROFILE, format!("{target},{early},{release_first}"))
                    .env(ROOT, root)
                    .output()
                    .unwrap();
                eprint!("{}", String::from_utf8_lossy(&output.stderr));
                assert!(
                    output.status.success(),
                    "child failed: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
                assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
            }
        }
    }
}
