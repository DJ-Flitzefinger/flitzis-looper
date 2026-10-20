//! Production waveform physical-press owners composed with actual GUI/MIDI pitch actions.
//! FullMix, 48 kHz mono, constant r=1, both Key Locks. The muted peer proves phase
//! ownership only. The independent bounded-reset PCM oracle does not certify
//! continuous-O, performer deadlines, a physical device, listening or the full App.

use super::*;
use crate::audio_engine::content_placement::ContentKey;
use crate::audio_engine::dsp::{DspNodeSlot, DspParameterId, DspParameterSlot, PerPadDspChain};
use crate::audio_engine::hold_action::{HoldActionState, HoldActionTicket, NativeHoldProjection};
use crate::audio_engine::pitch_runtime_tests::source_feed_tests::RawRubberBand;
use crate::audio_engine::source_feed_history::SourceFeedSnapshot;
use crate::audio_engine::source_playback::SourcePlayback;

const NOTE: i32 = 7;
const PARTITIONS: [usize; 9] = [1, 73, 511, 127, 512, 379, 512, 512, 446];
const HOLD_PROBE: &str = r#"
from time import monotonic, sleep
from unittest.mock import patch
from flitzis_looper.input_mapping.actions import LooperAction


def press(probe):
    waveform = probe.transport.waveform
    assert not waveform.has_pending_holds
    waveform.press_pause_hold(0, received_at_ns=probe.ctx.input.capture_timestamp_ns())
    assert waveform._physical_hold_press is not None
    assert len(waveform._hold_owners) == 1, waveform.hold_status()
    owner = next(iter(waveform._hold_owners.values()))
    assert owner.physical_press_id == waveform._physical_hold_press
    return owner.ticket


def observe_owned(probe):
    waveform = probe.transport.waveform
    waveform.poll_holds()
    assert waveform.hold_status() == ("paused_owned", None)
    assert len(waveform._hold_owners) == 1
    assert waveform._physical_hold_press is not None
    assert probe.session.waveform_pause_hold_pad_id == 0


def choose_set(probe, note):
    before = set(probe.transport.pitch._owners)
    probe.ctx.audio.pads.choose_piano_note(
        0, note, retrigger=False, received_at_ns=probe.ctx.input.capture_timestamp_ns())
    created = set(probe.transport.pitch._owners) - before
    assert len(created) == 1, (probe.session.input_mapping_error, probe.transport.pitch.status(0))
    owner = probe.transport.pitch._owners[created.pop()]
    assert owner.published and owner.content_id == probe.project.pad_content[0].instance_id
    capture = owner.capture
    assert not capture.retrigger and capture.base_shift == note and capture.extra_shift == 0
    assert capture.current_control_slot == 0 and capture.content_is_current
    return capture


def configure_midi(probe):
    # This fixture proves the real mapping Controller and dispatcher, not storage.
    # PianoProbe already owns the matching synthetic load boundary in this stack.
    probe.storage.enter_context(patch(
        "flitzis_looper.input_mapping.controller.save_midi_mapping_file"))
    probe.project.input_mapping_enabled = True
    probe._audio.set_input_mapping_enabled(True)
    for number, key in ((40, "ui.select_pad:0"), (41, "ui.select_pad:1"),
                        (60, "pitch.note:7:set_retrigger")):
        probe.input_mapping.save_mapping(
            "midi", f"midi:note:1:{number}", LooperAction(key=key))
    assert probe.input_mapping.refresh_pitch_runtime_state()
    assert probe.session.input_mapping_error is None


def midi_handoff(probe, expected_pitch):
    events = []
    deadline = monotonic() + 2
    while not events or events[-1].get("action_key") != "ui.select_pad:1":
        event = probe._audio.poll_input_events()
        if event is not None:
            events.append(event)
        else:
            assert monotonic() < deadline, "actual MIDI dispatcher barrier did not arrive"
            sleep(0.001)
    keys = [event.get("action_key") for event in events]
    expected = ["ui.select_pad:0"]
    if expected_pitch:
        expected.append("pitch.note:7:set_retrigger")
    expected.append("ui.select_pad:1")
    assert keys == expected, (keys, expected)
    captures = [event for event in events if event.get("captured_pitch") is not None]
    for event in events:
        probe.input_mapping._handle_rust_input_event(event)
    assert probe.project.selected_pad == 1
    assert probe.session.input_mapping_error is None
    if not expected_pitch:
        assert not captures
        return None
    assert len(captures) == 1
    event = captures[0]
    capture = event["captured_pitch"]
    assert event["direct"] and not event["dispatched"]
    assert capture.received_at_ns == event["received_at_ns"]
    assert capture.current_control_slot == 0 and capture.content_is_current
    owner = probe.transport.pitch._owners[capture.accepted_sequence]
    assert owner.capture is capture and owner.published
    return capture


def assert_intent(probe, note, physical):
    intent = probe.project.pad_key_intent[0]
    assert (intent.base_shift, intent.extra_shift) == (note, 0)
    assert (probe.transport.waveform._physical_hold_press is not None) == physical
    other = probe.project.pad_key_intent[1]
    assert (other.base_shift, other.extra_shift) == (0, 0)
    # This checks the chosen intent used by the central piano projection. Detailed
    # rendered highlight geometry remains in its separate ImGui regressions.
    assert probe.session.input_mapping_error is None


def release(probe):
    waveform = probe.transport.waveform
    waveform.request_hold_release("button_up")
    waveform.poll_holds()
    assert waveform._physical_hold_press is None


def observe_terminal(probe):
    waveform = probe.transport.waveform
    waveform.poll_holds()
    assert not waveform.has_pending_holds
    assert waveform.hold_status() == (None, None)
    assert probe.session.waveform_pause_hold_pad_id is None


def applied(probe, capture):
    assert capture.effective and capture.state == "applied"
    probe.transport.pitch.poll()
    assert not probe.transport.pitch._owners
    assert probe.transport.pitch.status(0) == ("applied", None)
    assert_intent(probe, capture.base_shift, probe.transport.waveform._physical_hold_press is not None)


def rejected(probe, capture):
    assert capture.state == "rejected" and not capture.effective
    assert capture.reason == "preparation_failed" and capture.effective_output_frame is None
    probe.transport.pitch.poll()
    assert not probe.transport.pitch._owners
    status, error = probe.transport.pitch.status(0)
    assert status == "error" and error
    assert_intent(probe, capture.base_shift, False)
"#;

struct HoldRoute {
    route: MouseRoute,
    helper: Py<PyModule>,
}

impl HoldRoute {
    fn new(lock: bool) -> Self {
        let mut route = MouseRoute::new(lock);
        let helper = Python::attach(|py| {
            PyModule::from_code(
                py,
                &CString::new(HOLD_PROBE).unwrap(),
                c"hold_pitch_probe.py",
                c"hold_pitch_probe",
            )
            .unwrap()
            .unbind()
        });
        // Start through the existing real mouse route; its actual wet history and
        // physical source reads precede the later waveform Hold and new pitch.
        let initial = route.capture("click", Some(2));
        let initial_t = Python::attach(|py| initial.borrow(py).original_due_frame());
        route.apply(&initial, 2, lock);
        let initial_receipt = Python::attach(|py| Receipt::capture(&initial.borrow(py)));
        assert_eq!(initial_receipt.original_t, initial_t);
        assert_eq!(initial_receipt.preparation_delay(), 8_192);
        let mut result = Self { route, helper };
        result.start_phase_peer();
        result
    }

    fn call(&self, name: &str) {
        Python::attach(|py| {
            self.helper
                .getattr(py, name)
                .unwrap()
                .call1(py, (self.route.probe.clone_ref(py),))
                .unwrap();
        });
    }

    fn assert_intent(&self, physical: bool) {
        Python::attach(|py| {
            self.helper
                .getattr(py, "assert_intent")
                .unwrap()
                .call1(py, (self.route.probe.clone_ref(py), NOTE, physical))
                .unwrap();
        });
    }

    fn voice(&self, id: usize) -> &VoiceSlot {
        self.route
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(id))
            .unwrap()
    }

    fn start_phase_peer(&mut self) {
        self.route.refresh_clock();
        let frame = self.route.transport.output_frame();
        let start = Python::attach(|py| {
            let engine = self.route.engine.borrow(py);
            let start = engine
                .capture_pad_start(
                    1,
                    LOOP_START as f64 / f64::from(RATE),
                    Some(LOOP_END as f64 / f64::from(RATE)),
                    0.0,
                    false,
                    None,
                    None,
                )
                .unwrap();
            assert!(engine.publish_pad_start(&start, None).unwrap());
            start
        });
        self.route.drain();
        let deadline = Instant::now() + WORKER_WAIT;
        while start.effective_frame().is_none() {
            assert_ne!(start.state(), "rejected", "{:?}", start.reason());
            assert!(Instant::now() < deadline, "peer start did not settle");
            self.route.mixer.pitch_runtime.as_mut().unwrap().poll();
            self.route.drain();
            assert_eq!(self.route.transport.output_frame(), frame);
            std::thread::sleep(Duration::from_millis(1));
        }
        self.route.render(1);
        assert_eq!(start.effective_frame(), Some(frame));
        assert_eq!(self.voice(1).volume, 0.0);
        assert!(!self.voice(1).paused);
    }

    fn press(&mut self) -> Py<HoldActionTicket> {
        self.route.refresh_clock();
        let ticket = Python::attach(|py| {
            self.helper
                .getattr(py, "press")
                .unwrap()
                .call1(py, (self.route.probe.clone_ref(py),))
                .unwrap()
                .extract(py)
                .unwrap()
        });
        self.route.drain();
        self.call("observe_owned");
        let projection = hold_projection(&ticket);
        assert_eq!(projection.state, HoldActionState::PausedOwned);
        assert!(projection.effect_current);
        assert_ne!(projection.effect.unwrap().changed_mask, 0);
        assert!(self.voice(0).paused);
        ticket
    }

    fn capture_set(&self) -> Py<CapturedPitchAction> {
        self.route.refresh_clock();
        Python::attach(|py| {
            self.helper
                .getattr(py, "choose_set")
                .unwrap()
                .call1(py, (self.route.probe.clone_ref(py), NOTE))
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn midi(&self, pitch: bool, releases: bool) -> Option<Py<CapturedPitchAction>> {
        self.route.refresh_clock();
        Python::attach(|py| {
            let engine = self.route.engine.borrow(py);
            let runtime = engine.input_runtime.as_ref().unwrap();
            assert!(runtime.inject_midi_message(&[0x90, 40, 127]));
            if pitch {
                assert!(runtime.inject_midi_message(&[0x90, 60, 127]));
            }
            if releases {
                assert!(!runtime.inject_midi_message(&[0x80, 60, 0]));
                assert!(!runtime.inject_midi_message(&[0x90, 60, 0]));
            }
            assert!(runtime.inject_midi_message(&[0x90, 41, 127]));
            drop(engine);
            self.helper
                .getattr(py, "midi_handoff")
                .unwrap()
                .call1(py, (self.route.probe.clone_ref(py), pitch))
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn prepare(&mut self, captured: &Py<CapturedPitchAction>) -> (CapturedPitchAction, Receipt) {
        let action = Python::attach(|py| captured.borrow(py).clone());
        let original = (
            action.sequence(),
            action.content_key(),
            action.received_timestamp(),
            action.original_due_frame(),
        );
        self.route.drain();
        assert!(action.accepted(), "{:?}", action.reason());
        assert!(!action.effective());
        assert_eq!(action.render().pitch, PitchIntent::new(NOTE, 0).unwrap());
        let receipt = Receipt::capture(&action);
        assert_eq!(receipt.execution, receipt.original_t + 8_192);
        let frame = self.route.transport.output_frame();
        let deadline = Instant::now() + WORKER_WAIT;
        while !self
            .route
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_ready_action(action.sequence())
        {
            self.route.mixer.pitch_runtime.as_mut().unwrap().poll();
            assert_ne!(action.state(), "rejected", "{:?}", action.reason());
            assert!(
                Instant::now() < deadline,
                "held pitch worker did not prepare"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(self.route.transport.output_frame(), frame);
        assert_eq!(
            (
                action.sequence(),
                action.content_key(),
                action.received_timestamp(),
                action.original_due_frame(),
            ),
            original
        );
        receipt.assert_unchanged(&action);
        (action, receipt)
    }

    fn applied(&self, captured: &Py<CapturedPitchAction>, receipt: &Receipt) {
        Python::attach(|py| {
            assert_eq!(
                captured
                    .getattr(py, "effective_output_frame")
                    .unwrap()
                    .extract::<u64>(py)
                    .unwrap(),
                receipt.execution
            );
            self.helper
                .getattr(py, "applied")
                .unwrap()
                .call1(py, (self.route.probe.clone_ref(py), captured.clone_ref(py)))
                .unwrap();
        });
        assert!(self.route.scheduler.is_empty());
    }

    fn rejected(&self, captured: &Py<CapturedPitchAction>) {
        Python::attach(|py| {
            self.helper
                .getattr(py, "rejected")
                .unwrap()
                .call1(py, (self.route.probe.clone_ref(py), captured.clone_ref(py)))
                .unwrap();
        });
        assert!(self.route.scheduler.is_empty());
    }
}

fn hold_projection(ticket: &Py<HoldActionTicket>) -> NativeHoldProjection {
    Python::attach(|py| ticket.borrow(py).action.projection())
}

struct Receipt {
    sequence: u64,
    content: ContentKey,
    timestamp: u64,
    original_t: u64,
    execution: u64,
}

impl Receipt {
    fn capture(action: &CapturedPitchAction) -> Self {
        Self {
            sequence: action.sequence(),
            content: action.content_key(),
            timestamp: action.received_timestamp(),
            original_t: action.original_due_frame(),
            execution: action.execution_target(),
        }
    }

    fn assert_unchanged(&self, action: &CapturedPitchAction) {
        assert_eq!(action.sequence(), self.sequence);
        assert_eq!(action.content_key(), self.content);
        assert_eq!(action.received_timestamp(), self.timestamp);
        assert_eq!(action.original_due_frame(), self.original_t);
        assert_eq!(action.execution_target(), self.execution);
    }

    fn preparation_delay(&self) -> u64 {
        self.execution.checked_sub(self.original_t).unwrap()
    }
}

fn active_clock(route: &HoldRoute, id: usize) -> (u64, u64) {
    let mut snapshot = SourceFeedSnapshot::new();
    assert!(
        route
            .voice(id)
            .source_feed_history
            .capture_into(&mut snapshot)
    );
    (snapshot.active_origin(), snapshot.active_frames())
}

fn filter_probe(route: &HoldRoute) -> Vec<f32> {
    filter_probe_chain(route.route.mixer.dsp_chain_snapshot_for_test(0).unwrap())
}

fn filter_probe_chain(mut chain: PerPadDspChain) -> Vec<f32> {
    let low = DspParameterId::per_pad(0, DspNodeSlot::Slot0, DspParameterSlot::Slot0).unwrap();
    assert!(chain.set_parameter(low, 0.0));
    (0..73)
        .map(|frame| {
            chain.begin_frame();
            chain.process_sample(0, if frame == 0 { 0.37 } else { 0.0 })
        })
        .collect()
}

struct Frozen {
    playback: SourcePlayback,
    history: (u64, u64),
    filter: Vec<f32>,
    sample: SampleBuffer,
    generation: u64,
    starts: usize,
    native: usize,
    fifo: (usize, usize),
}

impl Frozen {
    fn capture(route: &HoldRoute) -> Self {
        let voice = route.voice(0);
        Self {
            playback: voice.source_playback,
            history: active_clock(route, 0),
            filter: filter_probe(route),
            sample: voice.sample.as_ref().unwrap().clone(),
            generation: voice.generation,
            starts: route.route.starts(0),
            native: voice.stretch.native_state_address(),
            fifo: voice.stretch.pending_fifo_frames(),
        }
    }

    fn assert_physical_hold(&self, route: &HoldRoute) {
        let voice = route.voice(0);
        assert!(voice.paused);
        assert!(voice.source_playback.matches_exact(&self.playback));
        assert_eq!(active_clock(route, 0), self.history);
        assert_eq!(filter_probe(route), self.filter);
        assert_eq!(voice.generation, self.generation);
        assert!(voice.sample.as_ref().unwrap().same_window(&self.sample));
        assert_eq!(route.route.starts(0), self.starts);
    }

    fn assert_before_effect(&self, route: &HoldRoute) {
        self.assert_physical_hold(route);
        assert_eq!(route.voice(0).stretch.native_state_address(), self.native);
        assert_eq!(route.voice(0).stretch.pending_fifo_frames(), self.fifo);
    }

    fn assert_resumed_unchanged(&self, route: &HoldRoute) {
        let voice = route.voice(0);
        assert!(!voice.paused);
        assert!(voice.source_playback.matches_exact(&self.playback));
        assert_eq!(active_clock(route, 0), self.history);
        assert_eq!(filter_probe(route), self.filter);
        assert_eq!(voice.generation, self.generation);
        assert!(voice.sample.as_ref().unwrap().same_window(&self.sample));
        assert_eq!(route.route.starts(0), self.starts);
        assert_eq!(voice.stretch.native_state_address(), self.native);
        assert_eq!(voice.stretch.pending_fifo_frames(), self.fifo);
    }
}

struct Peer {
    output_frame: u64,
    active_frame: u64,
    phase: f64,
    generation: u64,
    native: usize,
    source: usize,
    starts: usize,
}

impl Peer {
    fn capture(route: &HoldRoute) -> Self {
        let voice = route.voice(1);
        let position = voice.source_playback.position();
        Self {
            output_frame: route.route.transport.output_frame(),
            active_frame: active_clock(route, 1).1,
            phase: position.frame as f64 + position.fraction,
            generation: voice.generation,
            native: voice.stretch.native_state_address(),
            source: voice.sample.as_ref().unwrap().source_address(),
            starts: route.route.starts(1),
        }
    }

    fn assert_continues(&self, route: &HoldRoute) {
        let delta = route.route.transport.output_frame() - self.output_frame;
        let voice = route.voice(1);
        assert!(!voice.paused);
        assert_eq!(voice.volume, 0.0);
        assert_eq!(voice.source_playback.tempo_ratio(), 1.0);
        assert_eq!(active_clock(route, 1).1 - self.active_frame, delta);
        assert_source_phase(voice.source_playback.position(), self.phase, delta, 1.0);
        assert_eq!(voice.generation, self.generation);
        assert_eq!(voice.stretch.native_state_address(), self.native);
        assert_eq!(voice.sample.as_ref().unwrap().source_address(), self.source);
        assert_eq!(route.route.starts(1), self.starts);
        assert_eq!(
            route.route.mixer.pad_pitch_intent(1),
            Some(PitchIntent::default())
        );
    }
}

/// Independent modulo source chronology plus the existing direct C ABI oracle.
/// No production reader, SourcePlayback, Native preparation or feed-journal replay
/// supplies the reference PCM. Actual sample ownership supplies only immutable PCM.
struct EndpointOracle {
    raw: RawRubberBand,
    source: SampleBuffer,
    source_frames: u64,
    pitch: f64,
    maximum: f32,
    filter: Option<PerPadDspChain>,
}

impl EndpointOracle {
    fn held_endpoint(source: &SampleBuffer, source_frames: u64) -> Self {
        let pitch = 2.0_f64.powf(f64::from(NOTE) / 12.0);
        let bound = RawRubberBand::history_bound(pitch) as u64;
        assert_eq!(bound, 4_096);
        assert!(source_frames > bound);
        // SET retains exact endpoint priming: H samples ending at the held F,
        // addressed only by the old voice's genuinely emitted source chronology.
        Self::from_origin(source, source_frames, NOTE, (source_frames - bound) as i64)
    }

    fn restart(source: &SampleBuffer, receipt: &Receipt) -> Self {
        let pitch = 2.0_f64.powf(f64::from(NOTE) / 12.0);
        let bound = RawRubberBand::history_bound(pitch) as u64;
        assert_eq!(bound, 4_096);
        assert!(receipt.execution >= receipt.original_t);
        // This independent new source begins at E with the existing H warmup.
        // Its immutable requested T never consumes an unheard source suffix.
        Self::from_origin(source, 0, NOTE, -(bound as i64))
    }

    /// Retain the original old +2 Native chronology, including its birth preroll.
    /// Reinitializing only a suffix at current F would certify a different owner.
    fn continuous_old(source: &SampleBuffer, source_frames: u64) -> Self {
        let pitch = 2.0_f64.powf(2.0 / 12.0);
        let bound = RawRubberBand::history_bound(pitch);
        assert_eq!(bound, 4_096);
        Self::from_origin(source, source_frames, 2, -(bound as i64))
    }

    fn from_origin(source: &SampleBuffer, source_frames: u64, note: i32, origin: i64) -> Self {
        let pitch = 2.0_f64.powf(f64::from(note) / 12.0);
        let mut result = Self {
            raw: RawRubberBand::new(pitch),
            source: source.clone(),
            source_frames,
            pitch,
            maximum: 0.0,
            filter: None,
        };
        for frame in origin..result.source_frames as i64 {
            // Both restart and the continuously retained original owner began
            // physically: negative birth coordinates are silence. The held SET
            // endpoint's strictly positive genuine history is unchanged.
            let input = if frame < 0 {
                0.0
            } else {
                result.source_sample(frame)
            };
            result.raw.sample(input, pitch);
        }
        result
    }

    fn source_sample(&self, active: i64) -> f32 {
        let frame = LOOP_START as i64 + active.rem_euclid((LOOP_END - LOOP_START) as i64);
        assert!(frame as usize >= self.source.resident_start());
        self.source.samples[frame as usize - self.source.resident_start()]
    }

    fn compare(&mut self, route: &HoldRoute, pcm: &[f32]) {
        for sample in pcm {
            let native = self
                .raw
                .sample(self.source_sample(self.source_frames as i64), self.pitch);
            let expected = match self.filter.as_mut() {
                Some(chain) => {
                    chain.begin_frame();
                    chain.process_sample(0, native)
                }
                None => native,
            };
            let error = (*sample - expected).abs();
            self.maximum = self.maximum.max(error);
            assert!(
                error < 2.0e-6,
                "source_frames={} actual={sample} independent_A_reset={expected} error={error}",
                self.source_frames
            );
            self.source_frames += 1;
        }
        let voice = route.voice(0);
        assert_source_phase(
            voice.source_playback.position(),
            LOOP_START as f64,
            self.source_frames,
            1.0,
        );
        assert_eq!(voice.source_playback.tempo_ratio(), 1.0);
        assert_eq!(voice.stretch.pending_fifo_frames(), self.raw.fifo());
        if let Some(chain) = &self.filter {
            assert_eq!(filter_probe(route), filter_probe_chain(chain.clone()));
        }
    }
}

fn render_held_to(
    route: &mut HoldRoute,
    frozen: &Frozen,
    ticket: &Py<HoldActionTicket>,
    original_hold: NativeHoldProjection,
    action: &CapturedPitchAction,
    receipt: &Receipt,
    peer: &Peer,
) {
    while route.route.transport.output_frame() < receipt.execution {
        let frames = (receipt.execution - route.route.transport.output_frame()).min(127) as usize;
        assert!(
            route
                .route
                .render(frames)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        frozen.assert_before_effect(route);
        assert_eq!(hold_projection(ticket), original_hold);
        assert!(!action.effective());
        receipt.assert_unchanged(action);
        peer.assert_continues(route);
    }
    assert_eq!(route.route.transport.output_frame(), receipt.execution);
}

#[test]
fn real_waveform_hold_gui_set_adopts_prepared_pitch_and_matching_release_resumes_same_voice() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for lock in [false, true] {
        let mut route = HoldRoute::new(lock);
        let ticket = route.press();
        let hold = hold_projection(&ticket);
        let frozen = Frozen::capture(&route);
        let peer = Peer::capture(&route);
        assert_eq!(frozen.history.0, 0, "initial voice's active-feed origin");
        let source_frames = frozen.history.1;
        assert_source_phase(
            route.voice(0).source_playback.position(),
            LOOP_START as f64,
            source_frames,
            1.0,
        );
        let mut oracle = EndpointOracle::held_endpoint(&frozen.sample, source_frames);
        let captured = route.capture_set();
        let (action, receipt) = route.prepare(&captured);
        assert_eq!(action.variant(), PitchActionVariant::Set);
        assert_eq!(action.render().key_lock, lock);
        route.assert_intent(true);
        frozen.assert_before_effect(&route);
        assert_eq!(hold_projection(&ticket), hold);
        render_held_to(&mut route, &frozen, &ticket, hold, &action, &receipt, &peer);
        assert_eq!(route.route.render(1), vec![0.0]);
        assert!(action.effective(), "{:?}", action.reason());
        frozen.assert_physical_hold(&route);
        assert_eq!(hold_projection(&ticket), hold);
        let native = route.voice(0).stretch.native_state_address();
        assert_ne!(native, frozen.native);
        assert_eq!(
            route.voice(0).stretch.pending_fifo_frames(),
            oracle.raw.fifo()
        );
        assert_eq!(
            route.voice(0).stretch.native_origin_tuple(),
            Some((action.render().pitch, lock, receipt.sequence))
        );
        route.call("observe_owned");
        for frames in [73, 511, 127, 512] {
            assert!(
                route
                    .route
                    .render(frames)
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
            frozen.assert_physical_hold(&route);
            assert_eq!(hold_projection(&ticket), hold);
            assert_eq!(route.voice(0).stretch.native_state_address(), native);
            assert_eq!(
                route.voice(0).stretch.pending_fifo_frames(),
                oracle.raw.fifo()
            );
            peer.assert_continues(&route);
        }
        route.call("release");
        route.route.drain();
        route.call("observe_terminal");
        let resumed = hold_projection(&ticket);
        assert_eq!(resumed.state, HoldActionState::Resumed);
        assert_eq!(resumed.effect, hold.effect);
        assert!(!route.voice(0).paused);
        assert!(
            route
                .voice(0)
                .source_playback
                .matches_exact(&frozen.playback)
        );
        assert_eq!(route.voice(0).stretch.native_state_address(), native);
        assert_eq!(
            route.voice(0).stretch.pending_fifo_frames(),
            oracle.raw.fifo()
        );
        assert_eq!(filter_probe(&route), frozen.filter);
        route.assert_intent(false);
        let mut pcm = Vec::new();
        for frames in PARTITIONS.into_iter().chain(std::iter::repeat_n(512, 96)) {
            let block = route.route.render(frames);
            oracle.compare(&route, &block);
            pcm.extend(block);
            assert_eq!(route.voice(0).generation, frozen.generation);
            assert_eq!(route.route.starts(0), frozen.starts);
            assert_eq!(route.voice(0).stretch.native_state_address(), native);
            assert_eq!(
                route.voice(0).stretch.adopted_request_id(),
                Some(receipt.sequence)
            );
            assert!(!route.voice(0).stretch.source_preparation_pending());
            receipt.assert_unchanged(&action);
            peer.assert_continues(&route);
        }
        assert_tone(&pcm[12_000..], NOTE, 0, 1.0, lock);
        route.applied(&captured, &receipt);
        println!(
            "waveform_hold_gui_SET lock={lock} original_T={} E={} bounded_reset_max_abs={} same_generation={} matched_physical_release; muted_peer_phase_only",
            receipt.original_t, receipt.execution, oracle.maximum, frozen.generation
        );
    }
}

#[test]
fn real_waveform_hold_midi_set_retrigger_supersedes_only_at_e_and_all_releases_leave_new_attack() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for lock in [false, true] {
        let mut route = HoldRoute::new(lock);
        route.call("configure_midi");
        let ticket = route.press();
        let hold = hold_projection(&ticket);
        let frozen = Frozen::capture(&route);
        let peer = Peer::capture(&route);
        let captured = route.midi(true, false).unwrap();
        let (action, receipt) = route.prepare(&captured);
        assert_eq!(action.variant(), PitchActionVariant::SetRetrigger);
        assert_eq!(action.render().key_lock, lock);
        assert!(!action.exclusive());
        assert!(route.midi(false, true).is_none());
        route.assert_intent(true);
        route.call("observe_owned");
        route.route.drain();
        frozen.assert_before_effect(&route);
        assert_eq!(hold_projection(&ticket), hold);
        receipt.assert_unchanged(&action);
        render_held_to(&mut route, &frozen, &ticket, hold, &action, &receipt, &peer);
        let mut oracle = EndpointOracle::restart(&frozen.sample, &receipt);
        let first = route.route.render(1);
        assert!(action.effective(), "{:?}", action.reason());
        oracle.compare(&route, &first);
        let superseded = hold_projection(&ticket);
        assert_eq!(superseded.state, HoldActionState::Superseded);
        assert_eq!(superseded.settled_at_output_frame, receipt.execution);
        assert_eq!(superseded.effect, hold.effect);
        assert!(!superseded.effect_current);
        assert!(!route.voice(0).paused);
        assert_eq!(route.voice(0).generation, frozen.generation + 1);
        assert_eq!(route.route.starts(0), frozen.starts + 1);
        assert_eq!(
            route.voice(0).stretch.native_origin_tuple(),
            Some((action.render().pitch, lock, receipt.sequence))
        );
        let native = route.voice(0).stretch.native_state_address();
        assert_ne!(native, frozen.native);
        route.call("observe_terminal");
        route.assert_intent(true);
        let playing = route.voice(0).source_playback;
        let fifo = route.voice(0).stretch.pending_fifo_frames();
        assert!(route.midi(false, true).is_none());
        route.route.drain();
        route.assert_intent(true);
        assert_eq!(hold_projection(&ticket), superseded);
        assert!(route.voice(0).source_playback.matches_exact(&playing));
        assert_eq!(route.voice(0).stretch.native_state_address(), native);
        assert_eq!(route.voice(0).stretch.pending_fifo_frames(), fifo);
        route.call("release");
        route.route.drain();
        route.call("observe_terminal");
        route.assert_intent(false);
        assert_eq!(hold_projection(&ticket), superseded);
        assert!(route.voice(0).source_playback.matches_exact(&playing));
        assert_eq!(route.voice(0).stretch.pending_fifo_frames(), fifo);
        let mut pcm = first;
        for frames in PARTITIONS.into_iter().chain(std::iter::repeat_n(512, 96)) {
            let block = route.route.render(frames);
            oracle.compare(&route, &block);
            pcm.extend(block);
            assert!(!route.voice(0).paused);
            assert_eq!(route.voice(0).generation, frozen.generation + 1);
            assert_eq!(route.route.starts(0), frozen.starts + 1);
            assert_eq!(route.voice(0).stretch.native_state_address(), native);
            assert_eq!(
                route.voice(0).stretch.adopted_request_id(),
                Some(receipt.sequence)
            );
            assert!(!route.voice(0).stretch.source_preparation_pending());
            receipt.assert_unchanged(&action);
            peer.assert_continues(&route);
        }
        assert_tone(&pcm[12_000..], NOTE, 0, 1.0, lock);
        route.applied(&captured, &receipt);
        println!(
            "waveform_hold_MIDI_SET_RETRIGGER lock={lock} original_T={} E={} bounded_reset_max_abs={} one_new_generation={} NoteOff_velocity0_and_old_physical_release_preserve_attack; muted_peer_phase_only",
            receipt.original_t,
            receipt.execution,
            oracle.maximum,
            frozen.generation + 1
        );
    }
}

fn assert_old_playing(
    route: &HoldRoute,
    frozen: &Frozen,
    origin: Option<(PitchIntent, bool, u64)>,
) {
    let voice = route.voice(0);
    assert!(!voice.paused);
    assert_eq!(voice.generation, frozen.generation);
    assert_eq!(route.route.starts(0), frozen.starts);
    assert!(voice.sample.as_ref().unwrap().same_window(&frozen.sample));
    assert_eq!(voice.stretch.native_state_address(), frozen.native);
    assert_eq!(voice.stretch.native_origin_tuple(), origin);
    assert_eq!(
        route.route.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(2, 0).unwrap())
    );
    assert!(!voice.stretch.source_preparation_pending());
}

#[test]
fn real_waveform_release_before_e_resumes_old_pcm_and_fences_set_but_preserves_normal_retrigger() {
    let _context = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for retrigger in [false, true] {
        for lock in [false, true] {
            let mut route = HoldRoute::new(lock);
            route.call("configure_midi");
            let ticket = route.press();
            let hold = hold_projection(&ticket);
            let frozen = Frozen::capture(&route);
            let peer = Peer::capture(&route);
            assert_eq!(frozen.history.0, 0, "initial voice's active-feed origin");
            let source_frames = frozen.history.1;
            assert_source_phase(
                route.voice(0).source_playback.position(),
                LOOP_START as f64,
                source_frames,
                1.0,
            );
            let old_origin = route.voice(0).stretch.native_origin_tuple();
            assert_eq!(old_origin.unwrap().0, PitchIntent::new(2, 0).unwrap());
            assert_eq!(old_origin.unwrap().1, lock);
            // Retain the initial +2 physical birth at E and every genuinely
            // emitted read since that birth, rather than resetting at held F.
            let mut old = EndpointOracle::continuous_old(&frozen.sample, source_frames);
            assert_eq!(old.raw.fifo(), frozen.fifo);
            old.filter = Some(route.route.mixer.dsp_chain_snapshot_for_test(0).unwrap());
            let captured = if retrigger {
                route.midi(true, false).unwrap()
            } else {
                route.capture_set()
            };
            let (action, receipt) = route.prepare(&captured);
            assert_eq!(
                action.variant(),
                if retrigger {
                    PitchActionVariant::SetRetrigger
                } else {
                    PitchActionVariant::Set
                }
            );
            assert_eq!(action.render().key_lock, lock);
            assert_eq!(route.route.transport.output_frame(), receipt.original_t);
            frozen.assert_before_effect(&route);
            assert_eq!(hold_projection(&ticket), hold);
            // prepare() has observed the genuine candidate ready before this
            // matching physical release. No candidate or clock is reconstructed.
            route.call("release");
            route.route.drain();
            route.call("observe_terminal");
            let resumed = hold_projection(&ticket);
            assert_eq!(resumed.state, HoldActionState::Resumed);
            assert_eq!(resumed.effect, hold.effect);
            assert_eq!(resumed.settled_at_output_frame, receipt.original_t);
            assert!(resumed.release_requested && resumed.release_admitted);
            frozen.assert_resumed_unchanged(&route);
            assert_old_playing(&route, &frozen, old_origin);
            route.assert_intent(false);
            receipt.assert_unchanged(&action);
            // Real NoteOff and velocity-zero ingress and repeated physical Up
            // leave the resumed old owner alone even while its new pitch waits.
            assert!(route.midi(false, true).is_none());
            route.call("release");
            route.route.drain();
            frozen.assert_resumed_unchanged(&route);
            assert_eq!(hold_projection(&ticket), resumed);
            assert_old_playing(&route, &frozen, old_origin);
            route.assert_intent(false);
            let mut before_pcm = Vec::new();
            while route.route.transport.output_frame() < receipt.execution {
                let frames =
                    (receipt.execution - route.route.transport.output_frame()).min(127) as usize;
                let block = route.route.render(frames);
                old.compare(&route, &block);
                before_pcm.extend(block);
                assert_old_playing(&route, &frozen, old_origin);
                assert!(!action.effective());
                assert_eq!(hold_projection(&ticket), resumed);
                receipt.assert_unchanged(&action);
                peer.assert_continues(&route);
            }
            assert_eq!(before_pcm.len(), 8_192);
            assert_tone(&before_pcm, 2, 0, 1.0, lock);
            // The scheduler now reaches the original E. SET's paused read
            // projection is stale; a Normal restart has no such read dependency.
            let mut fresh = EndpointOracle::restart(&frozen.sample, &receipt);
            let first = route.route.render(1);
            if retrigger {
                assert!(action.effective(), "{:?}", action.reason());
                fresh.compare(&route, &first);
                assert_eq!(route.voice(0).generation, frozen.generation + 1);
                assert_eq!(route.route.starts(0), frozen.starts + 1);
                assert_ne!(route.voice(0).stretch.native_state_address(), frozen.native);
                assert_eq!(
                    route.voice(0).stretch.native_origin_tuple(),
                    Some((PitchIntent::new(NOTE, 0).unwrap(), lock, receipt.sequence))
                );
            } else {
                assert_eq!(action.state(), "rejected");
                assert_eq!(action.reason(), Some("preparation_failed"));
                assert!(!action.effective());
                old.compare(&route, &first);
                assert_old_playing(&route, &frozen, old_origin);
            }
            assert_eq!(hold_projection(&ticket), resumed);
            receipt.assert_unchanged(&action);
            let after = Frozen::capture(&route);
            assert!(route.midi(false, true).is_none());
            route.call("release");
            route.route.drain();
            route.call("observe_terminal");
            after.assert_resumed_unchanged(&route);
            route.assert_intent(false);
            assert_eq!(hold_projection(&ticket), resumed);
            let native = route.voice(0).stretch.native_state_address();
            let mut after_pcm = first;
            for frames in PARTITIONS.into_iter().chain(std::iter::repeat_n(512, 96)) {
                let block = route.route.render(frames);
                if retrigger {
                    fresh.compare(&route, &block);
                    assert!(!route.voice(0).paused);
                    assert_eq!(route.voice(0).generation, frozen.generation + 1);
                    assert_eq!(route.route.starts(0), frozen.starts + 1);
                    assert_eq!(route.voice(0).stretch.native_state_address(), native);
                    assert_eq!(
                        route.voice(0).stretch.adopted_request_id(),
                        Some(receipt.sequence)
                    );
                    assert!(!route.voice(0).stretch.source_preparation_pending());
                } else {
                    old.compare(&route, &block);
                    assert_old_playing(&route, &frozen, old_origin);
                    assert_eq!(action.state(), "rejected");
                    assert!(!action.effective());
                }
                after_pcm.extend(block);
                assert_eq!(hold_projection(&ticket), resumed);
                receipt.assert_unchanged(&action);
                peer.assert_continues(&route);
            }
            assert_tone(
                &after_pcm[12_000..],
                if retrigger { NOTE } else { 2 },
                0,
                1.0,
                lock,
            );
            if retrigger {
                route.applied(&captured, &receipt);
            } else {
                route.rejected(&captured);
            }
            println!(
                "waveform_hold_release_before_E retrigger={retrigger} lock={lock} original_T={} E={} old_birth_continuous_max_abs={} new_bounded_reset_max_abs={} old_release_and_MIDI_releases_preserve_owners; muted_peer_phase_only",
                receipt.original_t, receipt.execution, old.maximum, fresh.maximum
            );
        }
    }
}
