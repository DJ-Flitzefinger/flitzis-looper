//! Actual ImGui mouse -> production UI actions/controllers -> public Native pitch route.
//!
//! Sources, captures, publication, preparation, scheduler and PCM are real. Only the
//! Python project envelope and mapping-file storage are synthetic. The virtual clock
//! waits for workers; this proves no CPAL device, listening or performer deadline.

use super::*;
use crate::audio_engine::pitch_actions::PitchActionVariant;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use std::ffi::CString;

static MOUSE_CONTEXT: Mutex<()> = Mutex::new(());
#[path = "fixed_offset_learn_tests.rs"]
mod fixed_offset_learn_tests;
#[path = "hold_pitch_tests.rs"]
mod hold_pitch_tests;
#[path = "late_piano_eq_catchup_tests.rs"]
mod late_piano_eq_catchup_tests;
#[path = "past_piano_mouse_tests.rs"]
mod past_piano_mouse_tests;
#[path = "piano_rate_controller_tests.rs"]
mod piano_rate_controller_tests;
#[path = "productive_custody_workload_tests.rs"]
mod productive_custody_workload_tests;
#[path = "rapid_birth_history_tests.rs"]
mod rapid_birth_history_tests;

const PROBE: &str = r###"
from contextlib import ExitStack
from pathlib import Path
from time import monotonic, sleep
from unittest.mock import patch

from imgui_bundle import imgui

from flitzis_looper.controller.transport import TransportController
from flitzis_looper.input_mapping.bindings import KeyboardBinding
from flitzis_looper.input_mapping.actions import LooperAction
from flitzis_looper.input_mapping.controller import InputMappingController
from flitzis_looper.input_mapping.storage import (
    KeyboardMappingFile, MidiMappingFile, load_midi_mapping_file, save_midi_mapping_file)
from flitzis_looper.key_intent import MusicalKey, PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import sidebar_left
from flitzis_looper.ui.render.pitch_keyboard import piano_geometry, render_piano_keyboard


class PianoProbe:
    def __init__(self, audio, lock, mapping_path=None):
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
        if mapping_path is None:
            self.storage.enter_context(patch(
                "flitzis_looper.input_mapping.controller.load_midi_mapping_file",
                return_value=MidiMappingFile()))
        else:
            self.mapping_path = Path(mapping_path)
            self.storage.enter_context(patch(
                "flitzis_looper.input_mapping.controller.load_midi_mapping_file",
                side_effect=lambda: load_midi_mapping_file(self.mapping_path)))
            self.storage.enter_context(patch(
                "flitzis_looper.input_mapping.controller.save_midi_mapping_file",
                side_effect=lambda data: save_midi_mapping_file(data, self.mapping_path)))
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

    def offset_frame(self):
        # Forward the real widget results; retain only their rendered hit boxes.
        # No forced-open tree, fabricated click result or replacement GUI action.
        self.offset_items = {}

        def observe(native):
            def rendered(label, *args, **kwargs):
                result = native(label, *args, **kwargs)
                lo, hi = imgui.get_item_rect_min(), imgui.get_item_rect_max()
                self.offset_items[label.split("##")[-1]] = (
                    (lo.x + hi.x) / 2,
                    (lo.y + hi.y) / 2,
                    label,
                )
                return result

            return rendered

        imgui.new_frame()
        imgui.set_next_window_pos((0, 0))
        imgui.set_next_window_size((1000, 300))
        imgui.begin("Real Native fixed offset", flags=imgui.WindowFlags_.no_saved_settings)
        slot = self.project.selected_pad
        info = sidebar_left._SidebarPadInfo(
            pad_id=slot,
            avail_x=imgui.get_content_region_avail().x,
            is_loaded=True,
            is_loading=False,
        )
        with (
            patch.object(imgui, "button", observe(imgui.button)),
            patch.object(imgui, "small_button", observe(imgui.small_button)),
            patch.object(imgui, "checkbox", observe(imgui.checkbox)),
            patch.object(imgui, "tree_node", observe(imgui.tree_node)),
        ):
            sidebar_left._render_transposition(self.ctx, info, self.ctx.state.pads.key_intent(slot))
        imgui.end()
        imgui.render()
        assert imgui.get_draw_data().total_vtx_count > 0

    def offset_click(self, item_id):
        x, y, _ = self.offset_items[item_id]
        io = imgui.get_io()
        io.add_mouse_pos_event(x, y)
        pressed, released = True, False
        io.add_mouse_button_event(0, pressed)
        self.offset_frame()
        io.add_mouse_button_event(0, released)
        self.offset_frame()

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

    def select_pad(self, slot, *, native_selection_already_applied=False,
                   expected_native_selection_revision=None):
        assert native_selection_already_applied
        if self._audio.input_selected_projection() == (expected_native_selection_revision, slot):
            self.project.selected_pad = slot

    def begin_encoder_learn(self):
        # Exercise Learn without opening an actual MIDI port or audio device.
        self.project.input_mapping_enabled = True
        self._audio.set_input_mapping_enabled(True)
        self.input_mapping.toggle_learn()
        assert self.session.input_learn_active

    def finish_encoder_learn(self, number, mode, piano, retrigger, nrpn=False):
        deadline = monotonic() + 2
        while self.session.input_learn_pending_binding_key is None:
            self.input_mapping._poll_rust_input_events()
            assert monotonic() < deadline, "actual CC Learn input did not arrive"
            sleep(0.001)
        kind = "nrpn" if nrpn else "cc"
        assert self.session.input_learn_pending_binding_key == f"midi:{kind}:1:{number}"
        self.ctx.ui.set_pitch_cc_mode(mode)
        self.ctx.audio.pads.step_pitch(
            0, 1, piano=piano, retrigger=retrigger,
            cc_mode=self.ctx.state.pitch_menus.midi_cc_mode)
        assert not self.session.input_learn_active
        assert not self.transport.pitch._owners

    def finish_nrpn_button_learn(self, number, original):
        deadline = monotonic() + 2
        while self.session.input_learn_pending_binding_key is None:
            self.input_mapping._poll_rust_input_events()
            assert monotonic() < deadline, "actual NRPN button Learn input did not arrive"
            sleep(0.001)
        assert self.session.input_learn_pending_binding_key == f"midi:nrpn:1:{number}"
        if original:
            self.ctx.audio.pads.original_key(0, retrigger=True)
        else:
            self.ctx.audio.pads.choose_piano_note(0, 12, retrigger=True)
        assert not self.session.input_learn_active
        assert not self.transport.pitch._owners

    def restore_encoder_learn(self, expected_count=10):
        for slot in (0, 1):
            # Both GUI preferences deliberately disagree with their stored lane:
            # piano learned SET, offset learned SET_RETRIGGER.
            self.ctx.audio.pads.set_pitch_retrigger_intent(slot, enabled=True)
            self.ctx.audio.pads.set_extra_retrigger_intent(slot, enabled=False)
        self.ctx.ui.set_pitch_cc_mode("button")
        for number, slot in ((40, 0), (41, 1)):
            self.input_mapping.save_mapping(
                "midi", f"midi:note:1:{number}", LooperAction(key=f"ui.select_pad:{slot}"))
        self.saved = self.mapping_path.read_bytes()
        # New production controller, real JSON/Pydantic load, no retained mapping object.
        self.input_mapping = InputMappingController(self)
        assert len(self.input_mapping._midi.mappings) == expected_count
        assert self.input_mapping._midi == load_midi_mapping_file(self.mapping_path)
        self.input_mapping._sync_rust_mapping_snapshot()
        self.input_mapping._sync_rust_learn_state()
        self.input_mapping.refresh_pitch_runtime_state()
        assert self.mapping_path.read_bytes() == self.saved
        assert not self.transport.pitch._owners

    def configure_encoder_timing(self, quantize, sync):
        self.project.trigger_quantization_enabled = quantize
        self.project.trigger_quantization_step = "1_16"
        self.project.bpm_lock = sync
        self.input_mapping.refresh_pitch_runtime_state()

    def encoder_handoff(self, expected_key):
        events = []
        deadline = monotonic() + 2
        while not events or events[-1].get("action_key") != "ui.select_pad:1":
            event = self._audio.poll_input_events()
            if event is not None:
                events.append(event)
            else:
                assert monotonic() < deadline, "actual CC dispatcher barrier did not arrive"
                sleep(0.001)
        keys = [event["action_key"] for event in events]
        expected = ["ui.select_pad:0"]
        if expected_key is not None:
            expected.append(expected_key)
        expected.append("ui.select_pad:1")
        assert keys == expected, (keys, expected)
        captures = [event for event in events if event.get("captured_pitch") is not None]
        for event in events:
            self.input_mapping._handle_rust_input_event(event)
        assert self.project.selected_pad == 1
        assert self.session.input_mapping_error is None
        assert self.mapping_path.read_bytes() == self.saved
        if expected_key is None:
            assert not captures and not self.transport.pitch._owners
            return None
        assert len(captures) == 1
        event = captures[0]
        capture = event["captured_pitch"]
        assert event["direct"] and not event["dispatched"]
        assert capture.received_at_ns == event["received_at_ns"]
        assert capture.current_control_slot == 0 and capture.content_is_current
        owner = self.transport.pitch._owners[capture.accepted_sequence]
        assert owner.capture is capture and owner.published
        assert owner.content_id == self.project.pad_content[0].instance_id
        return capture

    def encoder_applied(self, capture):
        assert capture.effective and capture.state == "applied"
        self.transport.pitch.poll()
        assert not self.transport.pitch._owners
        assert self.transport.pitch.status(0) == ("applied", None)
        intent = self.project.pad_key_intent[0]
        assert (intent.base_shift, intent.extra_shift) == (capture.base_shift, capture.extra_shift)
        other = self.project.pad_key_intent[1]
        assert (other.base_shift, other.extra_shift) == (0, 0)
        assert self.project.selected_pad == 1
        # Render the same central piano after switching back; source and selected
        # intent are production projections. Exact out-of-range rendering has its
        # separate ImGui vertex/text regression.
        self.project.selected_pad = 0
        self.frame()
        self.project.selected_pad = 1
        self.frame()
        assert self.mapping_path.read_bytes() == self.saved
        assert self.session.input_mapping_error is None

    def close(self):
        if self.imgui_context is None:
            return
        imgui.destroy_context(self.imgui_context)
        self.imgui_context = None
        if self.previous_context is not None:
            imgui.set_current_context(self.previous_context)
        self.storage.close()
"###;

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
        Self::new_with_storage(lock, None)
    }

    fn new_with_storage(lock: bool, mapping_path: Option<&std::path::Path>) -> Self {
        Self::from_fixture(Fixture::new(lock), lock, mapping_path)
    }

    fn from_fixture(fixture: Fixture, lock: bool, mapping_path: Option<&std::path::Path>) -> Self {
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
                .call1((
                    engine.clone_ref(py),
                    lock,
                    mapping_path.map(|path| path.to_string_lossy().into_owned()),
                ))
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

    fn starts(&self, id: usize) -> usize {
        self.feedback
            .iter()
            .filter(|message| {
                matches!(message,
                    AudioMessage::SampleStarted { id: actual }
                    | AudioMessage::ContentFeedback { id: actual, kind: 1, .. } if *actual == id
                )
            })
            .count()
    }

    fn encoder_message(
        &self,
        number: u16,
        values: &[u8],
        key: Option<&str>,
        nrpn: bool,
    ) -> Option<Py<CapturedPitchAction>> {
        self.refresh_clock();
        Python::attach(|py| {
            let engine = self.engine.borrow(py);
            let runtime = engine.input_runtime.as_ref().unwrap();
            assert!(runtime.inject_midi_message(&[0x90, 40, 127]));
            inject_control_values(runtime, number, values, nrpn);
            assert!(runtime.inject_midi_message(&[0x90, 41, 127]));
            drop(engine);
            self.probe
                .call_method1(py, "encoder_handoff", (key,))
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn apply_encoder(
        &mut self,
        captured: &Py<CapturedPitchAction>,
        pitch: PitchIntent,
        lock: bool,
        sync: bool,
        retrigger: bool,
    ) {
        let action = Python::attach(|py| captured.borrow(py).clone());
        let original = (
            action.sequence(),
            action.content_key(),
            action.received_timestamp(),
            action.original_due_frame(),
        );
        assert_eq!(
            action.variant(),
            if retrigger {
                PitchActionVariant::SetRetrigger
            } else {
                PitchActionVariant::Set
            }
        );
        let starts = self.starts(0);
        let generation = self
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap()
            .generation;
        assert_eq!(action.render().pitch, pitch);
        assert_eq!(action.render().key_lock, lock);
        assert_eq!(action.sync(), sync);
        assert_eq!(action.current_control_slot(), Some(0));
        self.drain();
        assert!(action.accepted(), "{:?}", action.reason());
        let execution = action.execution_target();
        let frame = self.transport.output_frame();
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
                "restored encoder worker did not prepare"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        #[cfg(windows)]
        let prepared_native = if retrigger {
            self.mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .ready_pitch_native_address(action.sequence())
        } else {
            None
        };
        #[cfg(windows)]
        if retrigger && pitch.total_semitones() != 0 {
            assert!(
                prepared_native.is_some(),
                "nonneutral retrigger lost its own Native"
            );
        }
        assert_eq!(self.transport.output_frame(), frame);
        assert_eq!(action.execution_target(), execution);
        while self.transport.output_frame() < execution {
            self.render((execution - self.transport.output_frame()).min(512) as usize);
            assert!(!action.effective());
        }
        let before_effect = self
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap()
            .source_playback
            .position();
        self.render(1);
        assert!(action.effective(), "{:?}", action.reason());
        assert_eq!(self.starts(0), starts + usize::from(retrigger));
        let voice = self
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap();
        if retrigger {
            #[cfg(windows)]
            if let Some(address) = prepared_native {
                assert_ne!(address, 0);
                assert_eq!(voice.stretch.native_state_address(), address);
            }
            assert_eq!(voice.stretch.adopted_request_id(), Some(action.sequence()));
            assert_eq!(
                voice.stretch.native_origin_tuple(),
                Some((pitch, lock, action.sequence()))
            );
        } else {
            assert_eq!(voice.generation, generation);
            assert_source_phase(
                voice.source_playback.position(),
                before_effect.frame as f64 + before_effect.fraction,
                1,
                1.0,
            );
        }
        assert_eq!(voice.source_playback.tempo_ratio(), 1.0);
        assert!(voice.stretch.owns_pitch_contract(pitch, lock));
        assert!(
            !self
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(1))
        );
        assert_eq!(self.starts(1), 0);
        assert_eq!(self.mixer.pad_pitch_intent(1), Some(PitchIntent::default()));
        let mut pcm = Vec::new();
        for _ in 0..96 {
            pcm.extend(self.render(512));
        }
        assert_tone(
            &pcm[12_000..],
            pitch.base_shift(),
            pitch.extra_shift(),
            1.0,
            lock,
        );
        assert_eq!(
            (
                action.sequence(),
                action.content_key(),
                action.received_timestamp(),
                action.original_due_frame()
            ),
            original
        );
        assert_eq!(action.execution_target(), execution);
        Python::attach(|py| {
            assert_eq!(
                captured
                    .getattr(py, "effective_output_frame")
                    .unwrap()
                    .extract::<u64>(py)
                    .unwrap(),
                execution
            );
            self.probe
                .call_method1(py, "encoder_applied", (captured.clone_ref(py),))
                .unwrap();
        });
        assert!(self.scheduler.is_empty());
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

fn learned_restored_encoder_cases(nrpn: bool) {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let parent = std::env::var_os("FLITZI_PIANO_MIDI_PARENT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("codex"));
    std::fs::create_dir_all(&parent).unwrap();
    for lock in [false, true] {
        let root = tempfile::Builder::new()
            .prefix("encoder-")
            .tempdir_in(&parent)
            .unwrap()
            .keep();
        let mut route = MouseRoute::new_with_storage(lock, Some(&root.join("midi.json")));
        let modes = ["button", "absolute", "twos_complement", "offset_binary"];
        for (index, mode) in modes.iter().enumerate() {
            for retrigger in [false, true] {
                let number =
                    if nrpn { 130 } else { 70 } + (2 * index) as u16 + u16::from(retrigger);
                Python::attach(|py| {
                    route.probe.call_method0(py, "begin_encoder_learn").unwrap();
                    inject_control_values(
                        route.engine.borrow(py).input_runtime.as_ref().unwrap(),
                        number,
                        &[127],
                        nrpn,
                    );
                    route
                        .probe
                        .call_method1(
                            py,
                            "finish_encoder_learn",
                            (number, *mode, !retrigger, retrigger, nrpn),
                        )
                        .unwrap();
                });
            }
        }
        Python::attach(|py| {
            route
                .probe
                .call_method0(py, "restore_encoder_learn")
                .unwrap()
        });
        let initial = route.capture("click", Some(0));
        route.apply(&initial, 0, lock);
        let mut base = 0;
        let mut extra = 0;
        let mut previous = Python::attach(|py| initial.borrow(py).sequence());
        for (index, mode) in modes.iter().enumerate() {
            // All four Quantize/SYNC pairs occur across the four explicit modes;
            // this is not their full Cartesian acoustic/actual-r(n) matrix.
            let quantize = index & 1 != 0;
            let sync = index & 2 != 0;
            Python::attach(|py| {
                route
                    .probe
                    .call_method1(py, "configure_encoder_timing", (quantize, sync))
                    .unwrap()
            });
            for retrigger in [false, true] {
                let number =
                    if nrpn { 130 } else { 70 } + (2 * index) as u16 + u16::from(retrigger);
                let axis = if retrigger { "extra" } else { "note" };
                let variant = if retrigger { "set_retrigger" } else { "set" };
                let key = if *mode == "button" {
                    format!("pitch.{axis}.step:1:{variant}")
                } else {
                    format!("pitch.{axis}.encoder:1:{mode}:{variant}")
                };
                if *mode != "button" {
                    let neutral: &[u8] = if *mode == "absolute" {
                        &[64, 64]
                    } else {
                        &[0, 64, 64]
                    };
                    assert!(route.encoder_message(number, neutral, None, nrpn).is_none());
                }
                let inputs: Vec<(Vec<u8>, i32)> = match *mode {
                    "button" => vec![(vec![127, 127, 0], 1), (vec![127, 0], 1)],
                    "absolute" => vec![(vec![65, 65], 1), (vec![64, 64], -1)],
                    "twos_complement" => {
                        vec![(vec![1], 1), (vec![1], 1), (vec![127], -1), (vec![127], -1)]
                    }
                    "offset_binary" => {
                        vec![(vec![65], 1), (vec![65], 1), (vec![63], -1), (vec![63], -1)]
                    }
                    _ => unreachable!(),
                };
                for (values, direction) in inputs {
                    if quantize {
                        let before_grid = (route.transport.output_frame() / 6000 + 1) * 6000 - 1000;
                        while route.transport.output_frame() < before_grid {
                            route
                                .render((before_grid - route.transport.output_frame()).min(512)
                                    as usize);
                        }
                    }
                    let frame = route.transport.output_frame();
                    let captured = route
                        .encoder_message(number, &values, Some(&key), nrpn)
                        .unwrap();
                    let sequence = Python::attach(|py| {
                        let action = captured.borrow(py);
                        assert_eq!(
                            action.original_due_frame(),
                            if quantize {
                                (frame / 6000 + 1) * 6000
                            } else {
                                frame
                            }
                        );
                        assert_eq!(
                            captured
                                .getattr(py, "quantize_step_64ths")
                                .unwrap()
                                .extract::<Option<u32>>(py)
                                .unwrap(),
                            quantize.then_some(4)
                        );
                        action.sequence()
                    });
                    assert!(sequence > previous);
                    previous = sequence;
                    if retrigger {
                        extra += direction;
                    } else {
                        base += direction;
                    }
                    route.apply_encoder(
                        &captured,
                        PitchIntent::new(base, extra).unwrap(),
                        lock,
                        sync,
                        retrigger,
                    );
                }
            }
        }
        route.close();
    }
}

fn inject_control_values(runtime: &InputRuntime, number: u16, values: &[u8], nrpn: bool) {
    if nrpn {
        assert!(!runtime.inject_midi_message(&[0xb0, 99, (number >> 7) as u8]));
        assert!(!runtime.inject_midi_message(&[0xb0, 98, (number & 127) as u8]));
    }
    for value in values {
        let controller = if nrpn {
            6
        } else {
            u8::try_from(number).unwrap()
        };
        assert!(runtime.inject_midi_message(&[0xb0, controller, *value]));
        if nrpn {
            // Nonzero low bytes must not appear as a second action or clear the
            // parameter before the next held value, reversal or release.
            assert!(!runtime.inject_midi_message(&[0xb0, 38, 91]));
        }
    }
}

#[test]
fn real_learned_restored_encoders_keep_variants_content_and_productive_pcm() {
    learned_restored_encoder_cases(false);
}

#[test]
fn real_nrpn_data_entry_learn_restores_parameter_encodings_and_productive_pcm() {
    learned_restored_encoder_cases(true);
}

#[test]
fn real_nrpn_piano_and_original_buttons_ignore_low_bytes_releases_and_held_values() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let parent = std::env::var_os("FLITZI_PIANO_MIDI_PARENT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("codex"));
    std::fs::create_dir_all(&parent).unwrap();
    for lock in [false, true] {
        let root = tempfile::Builder::new()
            .prefix("nrpn-buttons-")
            .tempdir_in(&parent)
            .unwrap()
            .keep();
        let mut route = MouseRoute::new_with_storage(lock, Some(&root.join("midi.json")));
        for (number, original) in [(130, false), (131, true)] {
            Python::attach(|py| {
                route.probe.call_method0(py, "begin_encoder_learn").unwrap();
                inject_control_values(
                    route.engine.borrow(py).input_runtime.as_ref().unwrap(),
                    number,
                    &[127],
                    true,
                );
                route
                    .probe
                    .call_method1(py, "finish_nrpn_button_learn", (number, original))
                    .unwrap();
            });
        }
        Python::attach(|py| {
            route
                .probe
                .call_method1(py, "restore_encoder_learn", (4,))
                .unwrap()
        });
        let initial = route.capture("click", Some(0));
        route.apply(&initial, 0, lock);
        let mut previous = Python::attach(|py| initial.borrow(py).sequence());
        for (index, (number, key, shift, original)) in [
            (130, "pitch.note:12:set_retrigger", 12, false),
            (130, "pitch.note:12:set_retrigger", 12, false),
            (131, "pitch.original:set_retrigger", 0, true),
            (131, "pitch.original:set_retrigger", 0, true),
        ]
        .into_iter()
        .enumerate()
        {
            let captured = route
                .encoder_message(number, &[127, 127, 0], Some(key), true)
                .unwrap();
            Python::attach(|py| {
                let action = captured.borrow(py);
                assert_eq!(action.variant(), PitchActionVariant::SetRetrigger);
                assert_eq!(action.resets_key_correction(), original);
                assert!(action.sequence() > previous);
                previous = action.sequence();
            });
            route.apply(&captured, shift, lock);
            assert_eq!(route.starts(0), index + 2);
            assert_eq!(route.starts(1), 0);
            assert_eq!(
                route.mixer.pad_pitch_intent(1),
                Some(PitchIntent::default())
            );
            Python::attach(|py| {
                route
                    .probe
                    .call_method1(py, "encoder_applied", (captured.clone_ref(py),))
                    .unwrap()
            });
        }
        route.close();
    }
}
