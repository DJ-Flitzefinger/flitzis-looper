//! Actual Cold Loader -> physical Hold -> new source/Close, without a device.
//! Each profile owns one child cwd beneath the caller's OWNER artifact block.

use super::*;
use crate::audio_engine::AudioEngine;
use crate::audio_engine::hold_action::{HoldActionState, HoldActionTicket};
use crate::audio_engine::input_mapping::InputRuntime;
use crate::audio_engine::source_playback::FractionalSourcePosition;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
const FRAMES: usize = 4096;
const ROOT: &str = "FLITZI_R2_HELD_LIFECYCLE_ROOT";
const PROFILE: &str = "FLITZI_R2_HELD_LIFECYCLE_PROFILE";
const PROBE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../src/tests/flitzis_looper/controller/held_lifecycle_probe.py"
));

fn write_input(root: &Path) -> PathBuf {
    let input = root.join("input");
    fs::create_dir(&input).unwrap();
    let path = input.join("held.wav");
    let size = (FRAMES * 4) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for frame in 0..FRAMES {
        for channel in 0..2 {
            let phase = frame as f64 * (0.037 + channel as f64 * 0.011);
            bytes.extend_from_slice(&((phase.sin() * 8192.0) as i16).to_le_bytes());
        }
    }
    fs::write(&path, bytes).unwrap();
    path
}

#[derive(Debug)]
struct SourceImage {
    owner: usize,
    generation: u64,
    lifetime: u64,
    content: String,
    reference: String,
    sha256: String,
}

#[derive(Debug, PartialEq)]
struct VoiceImage {
    lane: usize,
    generation: u64,
    paused: bool,
    position: FractionalSourcePosition,
    pcm: usize,
    native: usize,
}

#[derive(Debug, PartialEq)]
struct RequestImage {
    request: u64,
    prepared_epoch: u64,
    logical_request: u64,
    authority: u64,
    cache: (usize, usize, usize),
    digest: String,
    lease: (u64, bool, PathBuf, PathBuf),
    lease_generation: u64,
    cold_loading: u64,
    loading: bool,
}

struct DropProbe(mpsc::Sender<(ThreadId, Option<String>)>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        let current = thread::current();
        let _ = self
            .0
            .send((current.id(), current.name().map(str::to_owned)));
    }
}

struct ObservedCallback {
    callback: Box<AudioCallbackState>,
    // Report only after the complete production callback image has been destroyed.
    _drop_probe: DropProbe,
}

struct Fixture {
    engine: Py<AudioEngine>,
    probe: Py<PyAny>,
    callback: Option<CallbackCloseOwner<ObservedCallback>>,
    worker: Option<AudioBufferRetirementWorker>,
    destroyed: mpsc::Receiver<(ThreadId, Option<String>)>,
}

impl Fixture {
    fn new(root: &Path, restore: bool) -> Self {
        Python::initialize();
        let mut engine = AudioEngine::new().unwrap();
        let (producer, controls) = RingBuffer::new(8);
        let producer = Arc::new(Mutex::new(producer));
        let (parameters, parameter_rx) = RingBuffer::new(128);
        let (feedback, feedback_rx) = RingBuffer::new(512);
        engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(feedback_rx))));
        engine.test_parameter_producer = Some(Arc::new(Mutex::new(parameters)));
        engine.test_output_format = Some((RATE, 2));
        let clock = Arc::new(SharedOutputClock::new());
        let runtime = InputRuntime::new_with_ownership(
            producer,
            engine.input_clock,
            engine.input_runtime_ownership.clone(),
        );
        runtime.set_render_format(RATE, 2).unwrap();
        runtime.attach_pitch_clock(clock.clone()).unwrap();
        engine.input_runtime = Some(runtime);
        let mixer = create_stream_mixer(
            2,
            RATE,
            engine.current_timing_acknowledgements.clone(),
            engine.input_runtime_ownership.clone(),
            engine.prepared_source_epochs.clone(),
        )
        .unwrap();
        let acceptance = engine.loop_acceptance.clone();
        let (destroyed_tx, destroyed) = mpsc::channel();
        let (callback, worker) = create_audio_buffer_retirement_with_callback(|retirement| {
            Box::new(ObservedCallback {
                callback: boxed_stream_callback_state(
                    (controls, parameter_rx, feedback),
                    mixer,
                    RATE,
                    clock,
                    acceptance,
                    retirement,
                ),
                _drop_probe: DropProbe(destroyed_tx),
            })
        })
        .unwrap();
        let (engine, probe) = Python::attach(|py| {
            let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(3)
                .unwrap();
            py.import("sys")
                .unwrap()
                .getattr("path")
                .unwrap()
                .call_method1(
                    "insert",
                    (0, repository.join("src").to_string_lossy().as_ref()),
                )
                .unwrap();
            let engine = Py::new(py, engine).unwrap();
            let module = PyModule::from_code(
                py,
                &CString::new(PROBE).unwrap(),
                c"held_lifecycle_probe.py",
                c"held_lifecycle_probe",
            )
            .unwrap();
            let options = PyDict::new(py);
            options.set_item("restore", restore).unwrap();
            let probe = module
                .getattr("HeldLifecycleProbe")
                .unwrap()
                .call(
                    (engine.clone_ref(py), root.to_string_lossy().as_ref()),
                    Some(&options),
                )
                .unwrap()
                .unbind();
            (engine, probe)
        });
        Self {
            engine,
            probe,
            callback: Some(callback),
            worker: Some(worker),
            destroyed,
        }
    }

    fn state(&mut self) -> &mut AudioCallbackState {
        &mut self.callback.as_mut().unwrap().state_mut().callback
    }

    fn call(&self, name: &str) {
        Python::attach(|py| {
            self.probe.call_method0(py, name).unwrap();
        });
    }

    fn script(&self, source: &str) {
        Python::attach(|py| {
            let locals = PyDict::new(py);
            locals.set_item("probe", self.probe.bind(py)).unwrap();
            py.run(&CString::new(source).unwrap(), Some(&locals), None)
                .unwrap_or_else(|error| {
                    panic!("held lifecycle Python failed: {error:?}\n{source}")
                });
        });
    }

    fn ready_to_close(&self) -> bool {
        Python::attach(|py| {
            self.probe
                .call_method0(py, "prepare_close")
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn drain(&mut self) -> usize {
        let state = self.state();
        let frame = state.transport.output_frame();
        let count = drain_control_messages(
            &mut state.consumer_in,
            &mut state.scheduler,
            frame,
            &mut state.trigger_quantization,
            &mut state.transport,
            &mut state.mixer,
            &mut state.producer_out,
            &mut state.retired_buffers,
        );
        drain_parameter_messages(
            &mut state.parameter_consumer_in,
            &mut state.mixer,
            &mut state.transport,
        );
        count
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        let state = self.state();
        let frame = state.transport.output_frame();
        render_scheduled_audio_tracking_pads(
            &mut state.mixer,
            &mut state.scheduler,
            &mut output,
            &mut state.pad_peaks,
            &mut state.pad_activity,
            frame,
            2,
            &mut state.transport,
            &mut state.producer_out,
            &mut state.retired_buffers,
        );
        state.transport.advance_by_rendered_frames(frames);
        let end = state.transport.output_frame();
        publish_pad_telemetry(
            &mut state.producer_out,
            &state.mixer,
            &state.pad_peaks,
            &state.pad_activity,
            end,
            1,
            &mut state.last_pad_emit_frame,
        );
        output
    }

    fn wait_load(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.drain();
            let ready: bool = Python::attach(|py| {
                self.probe
                    .call_method1(py, "poll_load", (0,))
                    .unwrap()
                    .extract(py)
                    .unwrap()
            });
            if ready {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "real public Cold load lacked its own success ACK"
            );
            thread::sleep(Duration::from_millis(1));
        }
        self.drain();
        assert!(self.state().mixer.bank_for_measurement()[0].is_some());
        assert!(
            self.state()
                .mixer
                .voices
                .iter()
                .all(|voice| !voice.is_playing_sample(0))
        );
    }

    fn load(&mut self, path: &Path) {
        Python::attach(|py| {
            let request: u64 = self
                .probe
                .call_method1(py, "load", (0, path.to_string_lossy().as_ref()))
                .unwrap()
                .extract(py)
                .unwrap();
            assert!(request > 0);
        });
        self.wait_load();
    }

    fn source(&self) -> SourceImage {
        Python::attach(|py| {
            let history = self.probe.bind(py).getattr("source_history").unwrap();
            let source = history.get_item(0).unwrap().get_item(-1).unwrap();
            let engine = self.engine.borrow(py);
            SourceImage {
                owner: engine.input_runtime_ownership.owner_identity().diagnostic(),
                generation: source.getattr("request").unwrap().extract().unwrap(),
                lifetime: source.getattr("lifetime").unwrap().extract().unwrap(),
                content: source.getattr("content_id").unwrap().extract().unwrap(),
                reference: source.getattr("reference").unwrap().extract().unwrap(),
                sha256: source.getattr("sha256").unwrap().extract().unwrap(),
            }
        })
    }

    fn voice(&mut self) -> VoiceImage {
        let (lane, voice) = self
            .state()
            .mixer
            .voices
            .iter()
            .enumerate()
            .find(|(_, voice)| voice.is_playing_sample(0))
            .unwrap();
        VoiceImage {
            lane,
            generation: voice.generation,
            paused: voice.paused,
            position: voice.source_playback.position(),
            pcm: voice.sample.as_ref().unwrap().samples.as_ptr() as usize,
            native: voice.stretch.native_state_address(),
        }
    }

    fn request_image(&self) -> RequestImage {
        Python::attach(|py| {
            let engine = self.engine.borrow(py);
            let ownership = &engine.input_runtime_ownership;
            let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
            // Match the public unload's control-side lock order for a coherent image.
            let requests = engine.pad_request_ids.lock().unwrap();
            let cache = engine.sample_cache.lock().unwrap();
            let sample = cache[0].as_ref().unwrap();
            let digests = engine.loaded_source_digests.lock().unwrap();
            let leases = engine.cold_leases.lock().unwrap();
            let lease = leases[0].as_ref().unwrap();
            let loading = engine.loading_sample_ids.lock().unwrap();
            let assignment_retired = lease.assignment_retired();
            assert!(
                !assignment_retired,
                "current Native Cold assignment is retired"
            );
            RequestImage {
                request: requests[0],
                prepared_epoch: engine.prepared_source_epochs[0].load(Ordering::Acquire),
                logical_request: ownership.key_source_request_incarnation(key).unwrap(),
                authority: ownership.authority_revision(0),
                cache: (
                    sample.samples.as_ptr() as usize,
                    sample.samples.len(),
                    sample.channels,
                ),
                digest: digests[0].clone().unwrap(),
                lease: (
                    lease.assignment_id(),
                    assignment_retired,
                    lease.original_path.clone(),
                    lease.cache_path.clone(),
                ),
                lease_generation: engine.cold_lease_generations[0].load(Ordering::Acquire),
                cold_loading: engine.cold_loading[0].load(Ordering::Acquire),
                loading: loading.contains(&0),
            }
        })
    }

    fn start(&mut self) -> Vec<AudioMessage> {
        self.call("start");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.script("probe.transport.residency.poll()\nassert probe.transport.residency.status(0)[1] is None, probe.transport.residency.status(0)");
            self.drain();
            if self
                .state()
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(0))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "public start lacked its prepared Native adoption"
            );
            thread::sleep(Duration::from_millis(1));
        }
        self.script("probe.transport.residency.poll()\nassert probe.transport.residency.status(0) == (None, None), probe.transport.residency.status(0)");
        let output = self.render(37);
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(output.iter().any(|sample| sample.abs() > 1e-6));
        assert!(!self.voice().paused);
        let actual = self.take_feedback();
        // Retain unchanged captured START values for a later stale replay. Their
        // original packets still reach the real App and establish Session activity.
        let starts = actual
            .iter()
            .filter(|message| matches!(message, AudioMessage::ContentFeedback { kind: 1, .. }))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            starts.len(),
            1,
            "ordinary source start lacked its real tagged START"
        );
        for message in actual {
            assert!(self.state().producer_out.producer.push(message).is_ok());
        }
        self.call("poll_feedback");
        self.script("assert probe.session.active_sample_ids == {0}\nassert not probe.session.paused_sample_ids");
        starts
    }

    fn press(&self) -> Py<HoldActionTicket> {
        Python::attach(|py| {
            self.probe
                .call_method0(py, "press")
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn take_feedback(&self) -> Vec<AudioMessage> {
        Python::attach(|py| {
            let mut engine = self.engine.borrow_mut(py);
            let mut messages = Vec::new();
            while let Some(message) = engine.receive_msg().unwrap() {
                messages.push(message);
            }
            messages
        })
    }

    fn stale_feedback(&mut self, lifetime: u64) -> Vec<AudioMessage> {
        self.render(19);
        let messages = self
            .take_feedback()
            .into_iter()
            .filter(|message| {
                matches!(message,
            AudioMessage::ContentFeedback { lifetime: observed, .. } if *observed == lifetime)
            })
            .collect::<Vec<_>>();
        assert!(
            messages.iter().any(|message| matches!(
                message,
                AudioMessage::ContentFeedback { kind: 3 | 4, .. }
            ))
        );
        messages
    }

    fn late_release_and_feedback(&mut self, messages: Vec<AudioMessage>) {
        let before = self.voice();
        self.call("release");
        self.drain();
        self.call("poll_terminal");
        assert_eq!(
            self.voice(),
            before,
            "old Hold release changed the new source voice"
        );
        self.script("probe._late_feedback_before = probe.session.model_dump(mode='json')");
        for message in messages {
            assert!(self.state().producer_out.producer.push(message).is_ok());
        }
        self.call("poll_feedback");
        self.script("assert probe.session.model_dump(mode='json') == probe._late_feedback_before\nassert probe.session.active_sample_ids == {0}\nassert probe.session.waveform_pause_hold_pad_id is None");
        self.script(&format!(
            "assert probe.session.paused_sample_ids == {}",
            if before.paused { "{0}" } else { "set()" }
        ));
        assert_eq!(
            self.voice(),
            before,
            "old actual feedback changed the new source voice"
        );
    }

    fn close_callback(&mut self) {
        let caller = thread::current().id();
        drop(self.callback.take());
        drop(self.worker.take());
        let (thread, name) = self.destroyed.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_ne!(thread, caller);
        assert_eq!(name.as_deref(), Some("flitzis-audio-buffer-retirement"));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        Python::attach(|py| {
            let _ = self.probe.call_method0(py, "close");
            let _ = self.engine.borrow_mut(py).shut_down();
        });
        // Failure-safe order also applies to the deliberately failing pre-fix Close baseline.
        drop(self.callback.take());
        drop(self.worker.take());
    }
}

fn assignment_profile(root: &Path, direct: bool) {
    let path = write_input(root);
    let expected_sha = format!("{:x}", Sha256::digest(fs::read(&path).unwrap()));
    let mut fixture = Fixture::new(root, false);
    fixture.load(&path);
    let original = fixture.source();
    assert_eq!(original.sha256, expected_sha);
    let mut stale = fixture.start();
    stale.extend(fixture.stale_feedback(original.lifetime));
    assert!(stale.iter().all(|message| matches!(message,
        AudioMessage::ContentFeedback { lifetime, .. } if *lifetime == original.lifetime)));
    let hold = fixture.press();
    assert_eq!(fixture.drain(), 1);
    assert!(fixture.voice().paused);
    Python::attach(|py| assert_eq!(hold.borrow(py).action.state(), HoldActionState::PausedOwned));
    if direct {
        fixture.script("assert probe.owner.status == 'pause_pending'\nassert probe.session.waveform_pause_hold_pad_id is None");
    } else {
        fixture.call("observe_owned");
        fixture.call("save");
        let before = fixture.voice();
        Python::attach(|py| {
            let mut engine = fixture.engine.borrow_mut(py);
            while engine.ping().is_ok() {}
        });
        let requests = fixture.request_image();
        fixture.call("reject_full_unload");
        assert_eq!(
            fixture.request_image(),
            requests,
            "queue-full public unload changed Native request, cache or Cold lease authority"
        );
        assert_eq!(fixture.voice(), before);
        fixture.drain();
        fixture.call("unload");
        assert!(fixture.state().mixer.bank_for_measurement()[0].is_some());
        assert!(
            Path::new(&original.reference).is_file(),
            "callback reader lost its original before unload effect"
        );
        fixture.drain();
        assert!(fixture.state().mixer.bank_for_measurement()[0].is_none());
        assert!(
            fixture
                .state()
                .mixer
                .voices
                .iter()
                .all(|voice| !voice.is_playing_sample(0))
        );
    }
    fixture.load(&path);
    let current = fixture.source();
    assert_eq!(current.sha256, original.sha256, "same input bytes changed");
    assert_eq!(current.owner, original.owner);
    assert_ne!(current.generation, original.generation);
    assert_ne!(current.lifetime, original.lifetime);
    assert_ne!(current.content, original.content);
    fixture.start();
    if !direct {
        fixture.script("probe.transport.playback.pause_pad(0)");
        fixture.drain();
        assert!(
            fixture.voice().paused,
            "independent public pause of C did not take effect"
        );
    }
    fixture.late_release_and_feedback(stale);
    fixture.call("save");
    fixture.close_callback();
    fixture.call("close");
}

fn close_profile(root: &Path, observed: bool) {
    let path = write_input(root);
    let mut fixture = Fixture::new(root, false);
    fixture.load(&path);
    let original = fixture.source();
    // Pointer diagnostics are compared only while their actual owner stays alive.
    let original_owner =
        Python::attach(|py| fixture.engine.borrow(py).input_runtime_ownership.clone());
    fixture.start();
    let hold = fixture.press();
    if observed {
        fixture.drain();
        fixture.call("observe_owned");
    } else {
        Python::attach(|py| assert!(hold.borrow(py).action.effect().is_none()));
    }
    fixture.call("save");
    assert!(
        !fixture.ready_to_close(),
        "Close discarded an unsettled original Hold"
    );
    fixture.drain();
    Python::attach(|py| {
        let ticket = hold.borrow(py);
        let effect = ticket
            .action
            .effect()
            .expect("Close must settle the real owned pause effect");
        assert_eq!(effect.key.lifetime(), original.lifetime);
        assert_ne!(effect.changed_mask, 0);
        assert_eq!(ticket.action.state(), HoldActionState::Resumed);
    });
    assert!(fixture.ready_to_close());
    fixture.call("poll_terminal");
    // Intentionally reaches the actual stop_all route: its old stream-only lookup
    // is the regression baseline, not a substituted successful no-device Stop.
    fixture.call("finish_close");
    fixture.drain();
    assert!(
        fixture
            .state()
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.active)
    );
    fixture.close_callback();
    fixture.call("close");
    drop(hold);
    drop(fixture);
    assert!(
        Path::new(&original.reference).is_file(),
        "saved original was retired by Close"
    );

    let mut restored = Fixture::new(root, true);
    assert!(restored.state().mixer.bank_for_measurement()[0].is_none());
    restored.script("assert not probe.transport.waveform.has_pending_holds\nassert probe.session.waveform_pause_hold_pad_id is None\nassert not probe.session.active_sample_ids\nassert not probe.session.paused_sample_ids");
    restored.call("restore_sources");
    restored.wait_load();
    let fresh = restored.source();
    assert_ne!(fresh.owner, original.owner);
    assert_ne!(fresh.lifetime, original.lifetime);
    assert_eq!(fresh.content, original.content);
    assert_eq!(fresh.reference, original.reference);
    assert_eq!(fresh.sha256, original.sha256);
    restored.start();
    assert!(restored.ready_to_close());
    restored.call("finish_close");
    restored.drain();
    restored.close_callback();
    restored.call("close");
    drop(original_owner);
}

#[test]
fn public_cold_hold_source_and_close_lifetimes() {
    if let Some(profile) = std::env::var_os(PROFILE) {
        let profile = profile.to_str().unwrap();
        let root = PathBuf::from(std::env::var_os(ROOT).unwrap());
        match profile {
            "unload-refill" => assignment_profile(&root, false),
            "replace-unobserved" => assignment_profile(&root, true),
            "close-pending" => close_profile(&root, false),
            "close-owned" => close_profile(&root, true),
            _ => panic!("unknown held lifecycle profile: {profile}"),
        }
        eprintln!(
            "R2 held lifecycle child profile={profile} PID={}",
            std::process::id()
        );
        return;
    }
    let parent = std::env::var_os(ROOT).map(PathBuf::from).unwrap_or_else(|| {
        let temporary = std::env::temp_dir().join("codex");
        fs::create_dir_all(&temporary).unwrap();
        let owned = tempfile::Builder::new().prefix("r2-held-lifecycle-")
            .tempdir_in(temporary).unwrap().keep();
        fs::write(owned.join("OWNER.md"), "Owner: device-free R2 Cold/Hold lifecycle test block. Reproducible outputs; preserve results before cleanup.\n").unwrap();
        owned
    });
    assert!(parent.is_dir() && parent.join("OWNER.md").is_file());
    let (_, modules) = module_path!().split_once("::").unwrap();
    let test = format!("{modules}::public_cold_hold_source_and_close_lifetimes");
    for profile in [
        "unload-refill",
        "replace-unobserved",
        "close-pending",
        "close-owned",
    ] {
        let root = tempfile::Builder::new()
            .prefix(&format!("held-{profile}-"))
            .tempdir_in(&parent)
            .unwrap()
            .keep();
        fs::write(root.join("OWNER.md"), "Owner: device-free R2 real Cold/Hold lifecycle child. Reproducible outputs; preserve concise results before authorized cleanup.\n").unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test, "--nocapture", "--test-threads=1"])
            .env(PROFILE, profile)
            .env(ROOT, &root)
            .current_dir(&root)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprint!("{stderr}");
        assert!(
            output.status.success(),
            "held lifecycle {profile} failed:\n{stdout}\n{stderr}"
        );
        assert!(stdout.contains("1 passed; 0 failed"));
        assert!(stderr.contains(&format!("R2 held lifecycle child profile={profile} PID=")));
    }
}
