//! Actual MIDI capture -> Python controllers -> opaque Native publication -> render.
//! Only the project envelope, mapping-file storage and incoming-association flag are
//! synthetic. No pitch capture/publication/worker/effect result is replaced. The
//! virtual clock waits for preparation; no device or performer deadline is proved.

use super::*;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use std::ffi::CString;

const TARGET: u64 = 24_000;
const INITIAL_PCM: usize = 48_000;
const PROBE: &str = r#"
from time import monotonic, sleep
from unittest.mock import patch

from flitzis_looper.controller.transport import TransportController
from flitzis_looper.input_mapping.controller import InputMappingController
from flitzis_looper.input_mapping.storage import KeyboardMappingFile, MidiMappingFile
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState


class Probe:
    def __init__(self, audio, pending):
        self._audio = audio
        self.project = ProjectState()
        self.project.input_mapping_enabled = True
        self.project.multi_loop = True
        self.project.bpm_lock = False
        self.project.key_lock = True
        self.project.selected_pad = 1
        self.session = SessionState()
        # These are synthetic Python project associations, not Native source permits.
        for slot in (0, 1):
            self.project.sample_paths[slot] = f"synthetic-controller-{slot}.wav"
            self.project.pad_content[slot] = PadContentIdentity(instance_id=f"{slot + 1:032x}")
        self.transport = TransportController(self.project, self.session, audio)
        self.pending = pending
        # Exercise the production incoming-receipt association gate. The flag models
        # only delayed Python projection; the Native source is genuinely current.
        self.transport.bind_current_pad_edit(
            lambda *_args: None,
            incoming_request=lambda slot: 991 if self.pending and slot == 0 else None,
        )
        # Avoid reading or creating Human mapping files. Both controller constructors
        # and all subsequent input/pitch methods are production implementations.
        with patch("flitzis_looper.input_mapping.controller.load_midi_mapping_file",
                   return_value=MidiMappingFile()), patch(
                   "flitzis_looper.input_mapping.controller.load_keyboard_mapping_file",
                   return_value=KeyboardMappingFile()):
            self.input_mapping = InputMappingController(self)
        self.capture = None
        self.event = None

    def select_pad(self, slot, *, native_selection_already_applied=False,
                   expected_native_selection_revision=None):
        # UI-only projection: stale select-A receipts cannot overwrite the already
        # dispatched select-B receipt. No Native selection/capture is reconstructed.
        assert native_selection_already_applied
        if self._audio.input_selected_projection() == (expected_native_selection_revision, slot):
            self.project.selected_pad = slot

    def handoff(self):
        events = []
        deadline = monotonic() + 2
        while len(events) < 3:
            event = self._audio.poll_input_events()
            if event is not None:
                events.append(event)
            else:
                assert monotonic() < deadline, "actual dispatcher events unavailable"
                sleep(0.001)
        assert [event["action_key"] for event in events] == [
            "ui.select_pad:0", "pitch.extra:7:set_retrigger", "ui.select_pad:1"]
        assert self._audio.input_selected_projection()[1] == 1
        self.event = events[1]
        assert self.event["direct"] is True and self.event["dispatched"] is False
        self.capture = self.event["captured_pitch"]
        assert self.capture.state == "captured" and not self.capture.accepted
        assert self.capture.received_at_ns == self.event["received_at_ns"]
        for event in events:
            self.input_mapping._handle_rust_input_event(event)
        # A repeated delivery before callback admission must reuse this original
        # Controller owner and Native publication, not construct a second Work.
        self.input_mapping._handle_rust_input_event(self.event)
        assert self.session.input_mapping_error is None
        assert self.project.selected_pad == 1
        assert self.capture.state == "captured" and not self.capture.accepted
        assert len(self.transport.pitch._owners) == 1
        owner = self.transport.pitch._owners[self.capture.accepted_sequence]
        assert owner.capture is self.capture and owner.published
        assert (owner.content_id is None) is self.pending
        assert self.project.pad_key_intent[0].extra_shift == (0 if self.pending else 7)
        assert self.project.pad_key_intent[1].extra_shift == 0
        return self.capture

    def finish_after_actual_effect(self):
        assert self.capture.state == "applied" and self.capture.effective
        assert self.capture.effective_output_frame == 24000
        # Crucial order: effect AND productive PCM precede these polls; the pending
        # association stays closed while Native already reports its real effect.
        for _ in range(3):
            self.transport.pitch.poll()
        owners_while_pending = len(self.transport.pitch._owners)
        if self.pending:
            assert self.project.pad_key_intent[0].extra_shift == 0
            assert self.transport.pitch.status(0) == (None, None)
        self.pending = False
        for _ in range(3):
            self.transport.pitch.poll()
        assert self.project.pad_key_intent[0].extra_shift == 7, (
            "original applied A owner lost before pending project association resolved",
            self.capture.accepted_sequence, self.capture.state, owners_while_pending,
            self.project.pad_key_intent[0].extra_shift)
        assert self.project.pad_key_intent[1].extra_shift == 0
        assert self.project.selected_pad == 1
        assert self.transport.pitch.status(0) == ("applied", None)
        projection = self.transport.pitch._projections[self.project.pad_content[0].instance_id]
        assert projection.queued_sequence == self.capture.accepted_sequence
        assert projection.effective_sequence == self.capture.accepted_sequence
        assert projection.effective_pitch == (0, 7) and projection.effective_frame == 24000
        assert not self.transport.pitch._owners
        assert self.session.input_mapping_error is None
"#;

/// Own the original Fixture's exact callback components after moving its engine
/// into a real PyO3 object. No second scheduler or alternate executor is installed.
struct ControllerRoute {
    engine: Py<AudioEngine>,
    probe: Py<PyAny>,
    mixer: Box<RtMixer>,
    consumer: rtrb::Consumer<ControlMessage>,
    scheduler: FixedCapacityScheduler<16>,
    transport: TransportTimeline,
    clock: Arc<SharedOutputClock>,
    feedback: Vec<AudioMessage>,
    retirement: RtAudioBufferRetirement,
    _retirement_worker: AudioBufferRetirementWorker,
}

fn prepared_fixture() -> Fixture {
    let mut fixture = Fixture::new(true);
    let background = fixture.capture(1, "pitch.extra:0:set_retrigger");
    fixture.publish(&background);
    fixture.apply(&background);
    fixture.render_to(17_000);
    fixture.rows(PitchPadProjection {
        source_root: Some(0),
        source_mode: Some(PitchKeyMode::Major),
        key_lock: true,
        sync: false,
        quantize_step_64ths: Some(16),
        ..PitchPadProjection::default()
    });
    fixture
}

impl ControllerRoute {
    fn new(fixture: Fixture, pending: bool) -> Self {
        let repository = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
                c"pitch_controller_route_probe.py",
                c"pitch_controller_route_probe",
            )
            .unwrap();
            let probe = module
                .getattr("Probe")
                .unwrap()
                .call1((engine.clone_ref(py), pending))
                .unwrap()
                .unbind();
            (engine, probe)
        });
        Self {
            engine,
            probe,
            mixer: fixture.mixer,
            consumer: fixture.control.consumer,
            scheduler: fixture.scheduler,
            transport: fixture.transport,
            clock: fixture.clock,
            feedback: fixture.feedback,
            retirement: fixture.retirement,
            _retirement_worker: fixture._retirement_worker,
        }
    }

    fn handoff(&self) -> CapturedPitchAction {
        Python::attach(|py| {
            let engine = self.engine.borrow(py);
            let frame = self.transport.output_frame();
            let now = engine.input_clock.capture_ns();
            self.clock.publish(OutputClockSnapshot {
                valid: true,
                observed_at_ns: now,
                audible_at_ns: now,
                output_frame: frame,
                sample_rate_hz: RATE,
                master_period_seconds: Some(0.5),
                downbeat_frame: 0,
                master_beat: frame as f64 / (f64::from(RATE) * 0.5),
                freshness_ns: 2_000_000_000,
            });
            let runtime = engine.input_runtime.as_ref().unwrap();
            runtime.replace_mappings(vec![
                ("midi:note:1:40".into(), "ui.select_pad:0".into()),
                (
                    "midi:note:2:60".into(),
                    "pitch.extra:7:set_retrigger".into(),
                ),
                ("midi:note:1:41".into(), "ui.select_pad:1".into()),
            ]);
            runtime.set_enabled(true);
            assert!(runtime.inject_midi_message(&[0x90, 40, 127]));
            assert!(runtime.inject_midi_message(&[0x91, 60, 127]));
            assert!(runtime.inject_midi_message(&[0x90, 41, 127]));
            drop(engine);
            let action = self
                .probe
                .call_method0(py, "handoff")
                .unwrap()
                .extract::<Py<CapturedPitchAction>>(py)
                .unwrap();
            action.borrow(py).clone()
        })
    }

    fn allocations(&self) -> (usize, usize) {
        Python::attach(|py| self.engine.borrow(py).pitch_work_capacity.allocations())
    }

    fn drain(&mut self) {
        drain_control_messages(
            &mut self.consumer,
            &mut self.scheduler,
            self.transport.output_frame(),
            &mut TriggerQuantization::Immediate,
            &mut self.transport,
            &mut self.mixer,
            &mut self.feedback,
            &mut self.retirement,
        );
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames];
        render_scheduled_audio(
            &mut self.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            self.transport.output_frame(),
            1,
            &mut self.transport,
            &mut self.feedback,
            &mut self.retirement,
        );
        self.transport.advance_by_rendered_frames(frames);
        output
    }

    fn render_to(&mut self, target: u64) -> Vec<f32> {
        assert!(target >= self.transport.output_frame());
        let mut output = Vec::new();
        while self.transport.output_frame() < target {
            output.extend(self.render((target - self.transport.output_frame()).min(512) as usize));
        }
        output
    }

    fn voice(&self, id: usize) -> &VoiceSlot {
        self.mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == id)
            .unwrap()
    }

    fn starts(&self, id: usize) -> usize {
        // The production Vec sink records diagnostic SampleStarted messages;
        // actual content ownership is checked independently on the Native below.
        self.feedback
            .iter()
            .filter(|message| matches!(message, AudioMessage::SampleStarted { id: actual } if *actual == id))
            .count()
    }
}

fn controller_route_profile(pending: bool) {
    let mut direct = prepared_fixture();
    let direct_action = direct.capture(0, "pitch.extra:7:set_retrigger");
    direct.publish(&direct_action);
    direct.wait_worker(&[&direct_action]);
    assert_eq!(direct_action.execution_target(), TARGET);
    let expected_before = direct.render_to(TARGET);
    let expected = direct.pcm(INITIAL_PCM);
    assert!(direct_action.effective());

    let mut actual = ControllerRoute::new(prepared_fixture(), pending);
    let allocations = actual.allocations();
    let action = actual.handoff();
    let original = (
        action.content_key(),
        action.sequence(),
        action.received_timestamp(),
        action.render(),
    );
    assert_eq!(action.control_guard(None).unwrap().id, 0);
    assert_eq!(action.original_due_frame(), TARGET);
    assert_eq!(action.render().pitch, PitchIntent::new(0, 7).unwrap());
    assert_eq!(
        (action.render().key_lock, action.sync(), action.exclusive()),
        (true, false, false),
        "actual dispatcher captured unexpected original settings"
    );
    assert_eq!(actual.allocations().0 - allocations.0, 1);
    actual.drain();
    assert!(action.accepted() && !action.effective());
    assert_eq!(actual.scheduler.len(), 1);
    assert_eq!(action.execution_target(), TARGET);
    let deadline = Instant::now() + WORKER_WAIT;
    loop {
        actual.mixer.pitch_runtime.as_mut().unwrap().poll();
        if actual
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
            "original Controller worker did not prepare"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(actual.allocations().1 - allocations.1, 1);
    assert_eq!(actual.transport.output_frame(), 17_000);
    let background_native = actual.voice(1).stretch.native_state_address();
    let background_generation = actual.voice(1).generation;
    assert_eq!(actual.render_to(TARGET), expected_before);
    assert!(!action.effective());
    let mut pcm = Vec::new();
    while pcm.len() < INITIAL_PCM {
        pcm.extend(actual.render((INITIAL_PCM - pcm.len()).min(512)));
    }
    assert!(action.effective());
    assert!(actual.scheduler.is_empty());
    assert_eq!((actual.starts(0), actual.starts(1)), (1, 1));
    assert_eq!(pcm, expected);
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    let a = actual.voice(0);
    assert_eq!(a.stretch.adopted_request_id(), Some(action.sequence()));
    assert_ne!(a.stretch.native_state_address(), 0);
    assert!(a.stretch.owns_pitch_contract(action.render().pitch, true));
    let sample = a.sample.as_ref().unwrap();
    let history = a.stretch.productive_history().unwrap();
    assert_eq!(history.binding.source_address, sample.source_address());
    assert_eq!(history.binding.resident, sample.resident_binding());
    assert_eq!(
        a.source_playback.position(),
        direct.voice(0).source_playback.position()
    );
    assert_eq!(
        a.stretch.pending_fifo_frames(),
        direct.voice(0).stretch.pending_fifo_frames()
    );
    let b = actual.voice(1);
    assert_eq!(b.generation, background_generation);
    assert_eq!(b.stretch.native_state_address(), background_native);
    assert!(
        b.source_playback
            .matches_exact(&direct.voice(1).source_playback)
    );
    assert_eq!(
        (
            action.content_key(),
            action.sequence(),
            action.received_timestamp(),
            action.render(),
        ),
        original
    );
    // The regression's Python association polls intentionally occur only after
    // the original action's real Native effect and complete initial PCM above.
    Python::attach(|py| {
        actual
            .probe
            .call_method0(py, "finish_after_actual_effect")
            .unwrap()
    });
    actual.drain();
    assert!(actual.scheduler.is_empty());
    assert_eq!((actual.starts(0), actual.starts(1)), (1, 1));
    assert_eq!(actual.allocations().0 - allocations.0, 1);
    assert_eq!(actual.allocations().1 - allocations.1, 1);
}

#[test]
fn actual_controller_publishes_original_routed_capture_once_after_later_selection() {
    controller_route_profile(false);
}

#[test]
fn actual_controller_retains_applied_original_until_pending_project_association() {
    controller_route_profile(true);
}

fn opaque_content_liveness(action: &Py<CapturedPitchAction>) -> (bool, Option<usize>) {
    Python::attach(|py| {
        let action = action.bind(py);
        (
            action
                .getattr("content_is_current")
                .unwrap()
                .extract()
                .unwrap(),
            action
                .getattr("current_control_slot")
                .unwrap()
                .extract()
                .unwrap(),
        )
    })
}

#[test]
fn actual_applied_capture_liveness_survives_pair_mirror_gap_then_retires_on_unload() {
    use crate::audio_engine::pad_pair::control as pair_control;
    use crate::audio_engine::pitch_actions::PitchActionReject;

    let mut fixture = prepared_fixture();
    let action = fixture.capture(0, "pitch.extra:7:set_retrigger");
    let original = (
        action.content_key(),
        action.sequence(),
        action.received_timestamp(),
    );
    fixture.publish(&action);
    fixture.apply(&action);
    assert_native_source(&fixture, 0, &action);
    let native = fixture.voice(0).stretch.native_state_address();
    let generation = fixture.voice(0).generation;
    let position = fixture.voice(0).source_playback.position();
    let fifo = fixture.voice(0).stretch.pending_fifo_frames();
    let opaque = Python::attach(|py| Py::new(py, action.clone()).unwrap());
    assert_eq!(opaque_content_liveness(&opaque), (true, Some(0)));

    // Same actual capture/preparation/publication/callback path as remap_tests,
    // using its explicit mono-format seam rather than the stereo device default.
    let captured = fixture.control.engine._capture_pad_pair(0, 2).unwrap();
    let ticket = pair_control::prepare(&fixture.control.engine, &captured, RATE, 1).unwrap();
    fixture.control.engine._publish_pad_pair(&ticket).unwrap();
    fixture.drain();
    let native_ack = ticket.native_acknowledgement().unwrap();
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(native_ack.target_after.lifetime, original.0.lifetime());
    assert!(matches!(
        action.control_guard(None),
        Err(PitchActionReject::PlacementPending)
    ));
    assert_eq!(action.callback_guard(None).unwrap().id, 2);
    // A genuine Native Move is complete while the Python/control mirror remains
    // unprojected. Missing control placement is not a retired content lifetime.
    assert_eq!(opaque_content_liveness(&opaque), (true, None));
    assert!(action.effective());
    assert_eq!(fixture.voice(2).stretch.native_state_address(), native);
    assert_eq!(fixture.voice(2).generation, generation);
    assert_eq!(fixture.voice(2).source_playback.position(), position);
    assert_eq!(fixture.voice(2).stretch.pending_fifo_frames(), fifo);

    let acknowledgement = ticket.own_acknowledgement().unwrap();
    assert!(fixture.control.engine._reconcile_pad_pair(&ticket).unwrap());
    assert!(
        fixture
            .control
            .engine
            ._validate_pad_pair_acknowledgement(&ticket, &acknowledgement)
            .unwrap()
    );
    fixture
        .control
        .engine
        ._finish_pad_pair(&ticket, &acknowledgement)
        .unwrap();
    assert_eq!(opaque_content_liveness(&opaque), (true, Some(2)));
    assert_eq!(
        (
            action.content_key(),
            action.sequence(),
            action.received_timestamp()
        ),
        original
    );

    // Exercise actual public Unload and its normal callback consumption. The
    // retained applied receipt must not make the retired original lifetime live.
    fixture.control.engine.unload_sample(2).unwrap();
    fixture.drain();
    assert!(fixture.mixer.bank_for_measurement()[2].is_none());
    assert!(
        !fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 2)
    );
    assert_eq!(opaque_content_liveness(&opaque), (false, None));
    assert!(action.effective());
    assert_eq!(
        (
            action.content_key(),
            action.sequence(),
            action.received_timestamp()
        ),
        original
    );
}
