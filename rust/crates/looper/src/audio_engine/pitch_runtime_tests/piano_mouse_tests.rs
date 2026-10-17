//! Actual ImGui mouse -> production UI actions/controllers -> public Native pitch route.
//!
//! Sources, captures, publication, preparation, scheduler and PCM are real. Only the
//! Python project envelope and mapping-file storage are synthetic. The virtual clock
//! waits for workers; this proves no CPAL device, listening or performer deadline.

use super::*;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use std::ffi::CString;

static MOUSE_CONTEXT: Mutex<()> = Mutex::new(());
const PROBE: &str = r#"
from contextlib import ExitStack
from unittest.mock import patch

from imgui_bundle import imgui

from flitzis_looper.controller.transport import TransportController
from flitzis_looper.input_mapping.bindings import KeyboardBinding
from flitzis_looper.input_mapping.controller import InputMappingController
from flitzis_looper.input_mapping.storage import KeyboardMappingFile, MidiMappingFile
from flitzis_looper.key_intent import MusicalKey, PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render.pitch_keyboard import piano_geometry, render_piano_keyboard


class PianoProbe:
    def __init__(self, audio, lock):
        self._audio = audio
        self.project = ProjectState()
        self.project.input_mapping_enabled = False
        self.project.multi_loop = True
        self.project.bpm_lock = False
        self.project.key_lock = lock
        self.project.trigger_quantization_enabled = False
        self.project.selected_pad = 0
        self.session = SessionState()
        self.persistence = None
        for slot in (0, 1):
            self.project.sample_paths[slot] = f"synthetic-piano-{slot}.wav"
            self.project.sample_durations[slot] = 3.0
            self.project.pad_content[slot] = PadContentIdentity(instance_id=f"{slot + 1:032x}")
            self.project.pad_key_intent[slot] = PadKeyIntent(source=SourceKeyVersion(raw_key="C"))
            self.project.manual_bpm[slot] = 120.0
            self.project.pad_loop_auto[slot] = False
            self.project.pad_loop_start_s[slot] = 0.1
            self.project.pad_loop_end_s[slot] = 2.1
        self.transport = TransportController(self.project, self.session, audio)
        self.storage = ExitStack()
        self.storage.enter_context(patch(
            "flitzis_looper.input_mapping.controller.load_midi_mapping_file",
            return_value=MidiMappingFile()))
        self.storage.enter_context(patch(
            "flitzis_looper.input_mapping.controller.load_keyboard_mapping_file",
            return_value=KeyboardMappingFile()))
        self.storage.enter_context(patch(
            "flitzis_looper.input_mapping.controller.save_keyboard_mapping_file"))
        self.input_mapping = InputMappingController(self)
        self.ctx = UiContext(self)
        audio.set_input_mapping_enabled(False)
        self.previous_context = imgui.get_current_context()
        self.imgui_context = imgui.create_context()
        imgui.set_current_context(self.imgui_context)
        io = imgui.get_io()
        io.set_ini_filename(None)
        io.display_size = imgui.ImVec2(1100, 400)
        io.delta_time = 1 / 60
        io.backend_flags |= imgui.BackendFlags_.renderer_has_textures
        io.add_mouse_pos_event(1050, 350)
        self.origin = None
        self.geometry = ()
        # No on_frame_render/runtime sync: the first mouse action must refresh the
        # genuine source binding itself, just after a source assignment is visible.
        self.frame()
        self.frame()

    def frame(self):
        imgui.new_frame()
        imgui.set_next_window_pos((0, 0))
        imgui.set_next_window_size((1000, 300))
        imgui.begin("Real Native piano mouse", flags=imgui.WindowFlags_.no_saved_settings)
        intent = self.project.pad_key_intent[self.project.selected_pad]
        source = intent.source_key
        root = source.root if source is not None else None
        origin = imgui.get_cursor_screen_pos()
        self.origin = (origin.x, origin.y)
        self.geometry = piano_geometry(root, width=imgui.get_content_region_avail().x, height=150)
        chosen = render_piano_keyboard(
            "native_piano_mouse", source_root=root, selected_shift=intent.base_shift, height=150)
        if chosen is not None:
            self.ctx.audio.pads.choose_piano_note(
                self.project.selected_pad, chosen, retrigger=True,
                received_at_ns=self.ctx.input.capture_timestamp_ns())
        imgui.end()
        imgui.render()
        assert imgui.get_draw_data().total_vtx_count > 0

    def click(self, shift, *, expect_capture=True):
        before = set(self.transport.pitch._owners)
        item = next(item for item in self.geometry if item.key.value == shift)
        # Lower white-key area avoids the raised black neighbors. Black-key
        # centers intentionally overlap underlying white geometry.
        x = self.origin[0] + item.center_x
        y = self.origin[1] + item.height * (0.45 if item.key.black else 0.85)
        io = imgui.get_io()
        io.add_mouse_pos_event(x, y)
        io.add_mouse_button_event(0, True)
        self.frame()
        io.add_mouse_button_event(0, False)
        self.frame()
        created = set(self.transport.pitch._owners) - before
        if not expect_capture:
            assert not created, "Learn generated an unexpected audible pitch owner"
            return None
        assert len(created) == 1, (
            "mouse pitch did not reach real Native admission", shift,
            self.session.input_mapping_error, self.transport.pitch.status(0))
        owner = self.transport.pitch._owners[created.pop()]
        assert owner.published and owner.content_id == self.project.pad_content[0].instance_id
        capture = owner.capture
        assert capture.base_shift == shift and capture.extra_shift == 0
        assert capture.retrigger and capture.content_is_current
        assert self.project.pad_key_intent[0].base_shift == shift
        assert self.session.input_mapping_error is None
        return capture

    def applied(self, capture):
        assert capture.state == "applied" and capture.effective
        self.transport.pitch.poll()
        assert self.transport.pitch.status(0) == ("applied", None)
        assert not self.transport.pitch._owners
        assert self.project.pad_key_intent[0].base_shift == capture.base_shift

    def correct_and_choose_legacy_base(self):
        # Change correction after the last frame sync, then immediately invoke
        # the retained production GUI action. Stale C/major Native rows reject
        # D# minor; a refreshed D minor source resolves exactly +1 semitone.
        self.transport.pad.set_manual_key(0, "Dm")
        before = set(self.transport.pitch._owners)
        self.ctx.audio.pads.choose_base_key(
            0, MusicalKey(root=3, mode="minor"), retrigger=True,
            received_at_ns=self.ctx.input.capture_timestamp_ns())
        created = set(self.transport.pitch._owners) - before
        assert len(created) == 1, self.session.input_mapping_error
        capture = self.transport.pitch._owners[created.pop()].capture
        assert capture.base_shift == 1 and capture.extra_shift == 0
        assert self.project.pad_key_intent[0].correction == "Dm"
        return capture

    def learn_without_attack(self):
        self.project.input_mapping_enabled = True
        self._audio.set_input_mapping_enabled(True)
        self.input_mapping.toggle_learn()
        assert self.session.input_learn_active
        binding = KeyboardBinding(key_name="A")
        assert self.input_mapping.capture_keyboard_input(
            binding, text_input_focused=False,
            received_at_ns=self.ctx.input.capture_timestamp_ns())
        assert self.session.input_learn_pending_binding_key == binding.key
        self.frame()
        self.click(2, expect_capture=False)
        assert not self.session.input_learn_active
        mapping = self.input_mapping._keyboard.mappings[-1]
        assert mapping.input == binding
        assert mapping.action.key.endswith(":set_retrigger")
        assert self.project.pad_key_intent[0].base_shift == 1
        assert not self.transport.pitch._owners

    def close(self):
        if self.imgui_context is None:
            return
        imgui.destroy_context(self.imgui_context)
        self.imgui_context = None
        if self.previous_context is not None:
            imgui.set_current_context(self.previous_context)
        self.storage.close()
"#;

struct MouseRoute {
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

impl MouseRoute {
    fn new(lock: bool) -> Self {
        let fixture = Fixture::new(lock);
        // Source custody is genuinely ready, but the once-per-frame control rows
        // have not observed it. This is the pre-fix first-action admission gap.
        fixture
            .control
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_runtime_state(
                true,
                vec![false; NUM_SAMPLES],
                vec![0.0; NUM_SAMPLES],
                vec![None; NUM_SAMPLES],
                vec![None; NUM_SAMPLES],
            )
            .unwrap();
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
                c"piano_mouse_probe.py",
                c"piano_mouse_probe",
            )
            .unwrap();
            let probe = module
                .getattr("PianoProbe")
                .unwrap()
                .call1((engine.clone_ref(py), lock))
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

    fn refresh_clock(&self) {
        Python::attach(|py| {
            let now = self.engine.borrow(py).input_clock.capture_ns();
            let frame = self.transport.output_frame();
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
        });
    }

    fn capture(&self, method: &str, shift: Option<i32>) -> Py<CapturedPitchAction> {
        self.refresh_clock();
        Python::attach(|py| {
            let value = match shift {
                Some(shift) => self.probe.call_method1(py, method, (shift,)),
                None => self.probe.call_method0(py, method),
            }
            .unwrap();
            value.extract(py).unwrap()
        })
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

    fn apply(&mut self, captured: &Py<CapturedPitchAction>, shift: i32, lock: bool) {
        let action = Python::attach(|py| captured.borrow(py).clone());
        assert_eq!(action.render().pitch, PitchIntent::new(shift, 0).unwrap());
        let frame = self.transport.output_frame();
        assert_eq!(action.original_due_frame(), frame);
        self.drain();
        assert!(action.accepted() && !action.effective());
        let execution_frame = action.execution_target();
        assert!(execution_frame >= frame);
        let deadline = Instant::now() + WORKER_WAIT;
        while !self
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_ready_action(action.sequence())
        {
            self.mixer.pitch_runtime.as_mut().unwrap().poll();
            assert_ne!(action.state(), "rejected", "{:?}", action.reason());
            assert!(
                Instant::now() < deadline,
                "mouse pitch worker did not prepare"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        while self.transport.output_frame() < execution_frame {
            self.render((execution_frame - self.transport.output_frame()).min(512) as usize);
            assert!(
                !action.effective(),
                "pitch applied before its original execution frame"
            );
        }
        self.render(1);
        assert!(action.effective(), "{:?}", action.reason());
        assert_eq!(
            Python::attach(|py| {
                captured
                    .getattr(py, "effective_output_frame")
                    .unwrap()
                    .extract::<u64>(py)
                    .unwrap()
            }),
            execution_frame
        );
        assert!(self.scheduler.is_empty());
        let mut pcm = Vec::new();
        for _ in 0..96 {
            pcm.extend(self.render(512));
        }
        assert_tone(&pcm[12_000..], shift, 0, 1.0, lock);
        let voice = self
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == 0)
            .unwrap();
        if shift != 0 {
            assert!(
                voice
                    .stretch
                    .owns_pitch_contract(action.render().pitch, lock)
            );
        }
        Python::attach(|py| {
            self.probe
                .call_method1(py, "applied", (captured.clone_ref(py),))
                .unwrap();
        });
    }

    fn close(&self) {
        Python::attach(|py| self.probe.call_method0(py, "close").unwrap());
    }
}

impl Drop for MouseRoute {
    fn drop(&mut self) {
        self.close();
    }
}

#[test]
fn real_mouse_piano_refreshes_new_source_with_routing_disabled_and_repeats_attacks() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Include white, black, an identical repeated black, a new white, and both
    // octave directions. Identical requests must retain distinct action owners.
    for lock in [false, true] {
        let mut route = MouseRoute::new(lock);
        let mut previous_sequence = 0;
        for (index, shift) in [0, 1, 1, 2, -12, 12].into_iter().enumerate() {
            let captured = route.capture("click", Some(shift));
            let sequence = Python::attach(|py| captured.borrow(py).sequence());
            assert!(sequence > previous_sequence);
            previous_sequence = sequence;
            route.apply(&captured, shift, lock);
            let starts = route
                .feedback
                .iter()
                .filter(|message| {
                    matches!(
                        message,
                        AudioMessage::SampleStarted { id: 0 }
                            | AudioMessage::ContentFeedback { id: 0, kind: 1, .. }
                    )
                })
                .count();
            assert_eq!(
                starts,
                index + 1,
                "mouse press lost or duplicated an attack"
            );
        }
        route.close();
    }
}

#[test]
fn real_gui_refreshes_same_frame_correction_and_learn_keeps_pitch_inaudible() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut route = MouseRoute::new(true);
    let captured = route.capture("correct_and_choose_legacy_base", None);
    route.apply(&captured, 1, true);
    let before = Python::attach(|py| {
        route
            .engine
            .borrow(py)
            .input_runtime
            .as_ref()
            .unwrap()
            .pitch_request_sequence(0)
            .unwrap()
    });
    route.refresh_clock();
    Python::attach(|py| {
        route
            .probe
            .call_method0(py, "learn_without_attack")
            .unwrap()
    });
    route.drain();
    assert!(route.scheduler.is_empty());
    let after = Python::attach(|py| {
        route
            .engine
            .borrow(py)
            .input_runtime
            .as_ref()
            .unwrap()
            .pitch_request_sequence(0)
            .unwrap()
    });
    assert_eq!(after, before, "Learn captured an audible Native action");
    route.close();
}
