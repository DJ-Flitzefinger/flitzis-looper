//! Actual piano/MIDI and Python global-rate controls share confirmed Native time.
//!
//! Complete small PCM and declared synthetic timing labels are fixture inputs.
//! Source/timing ACKs, controllers, parameter/control drains, preparation, the sole
//! scheduler, effects and PCM are real. Host waits and a virtual observer clock
//! establish no device deadline, finite-window, continuous-O or acoustic acceptance.

#![cfg(windows)]

#[path = "piano_rate_glide_tests.rs"]
mod piano_rate_glide_tests;

use super::*;
use crate::audio_engine::audio_stream::drain_parameter_messages;
use crate::audio_engine::constant_timing::{self, l3_constant_timing_fixture};
use crate::audio_engine::constants::MAX_SCHEDULED_EVENTS;
use crate::audio_engine::dsp::PerPadDspChain;
use crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_sources;
use crate::audio_engine::pitch_runtime_tests::source_feed_tests::RawRubberBand;
use crate::messages::ControlParameterMessage;
use rtrb::{Consumer, Producer};

const CHILD: &str = "FLITZI_PIANO_GLOBAL_RATE_CHILD";
const TEST: &str = "audio_engine::pitch_runtime_tests::piano_mouse_tests::piano_rate_controller_tests::public_rate_controls_fence_old_pitch_then_fresh_controller_attack_owns_pcm";
const CAPACITY: usize = 1024;
const WAIT: Duration = Duration::from_secs(15);
const PERIODS: [f64; 2] = [0.5, 0.375];
const ENDS: [usize; 2] = [100_800, 76_800];
const NEW_SPEED: f64 = 1.8;
const HISTORY: usize = 4096;
const CHUNKS: [usize; 7] = [73, 509, 7, 127, 3, 512, 31];

const HELPER: &str = r#"
from time import monotonic, sleep
from unittest.mock import patch
from imgui_bundle import imgui
from flitzis_looper.input_mapping.actions import LooperAction


def configure(probe):
    probe.storage.enter_context(patch(
        "flitzis_looper.input_mapping.controller.save_midi_mapping_file"))
    for slot, period, end in ((0, .5, 2.1), (1, .375, 1.6)):
        # PianoProbe's legacy manual120 would bypass accepted-anchor publication.
        probe.project.manual_bpm[slot] = None
        probe.project.pad_timing_intent[slot] = "automatic"
        probe.project.pad_loop_auto[slot] = False
        probe.project.pad_loop_start_s[slot] = .1
        probe.project.pad_loop_end_s[slot] = end
        timing = probe.transport.bpm.current_timing(slot)
        metadata = probe._audio.current_constant_timing(slot)
        assert timing is not None and timing.accepted_revision
        assert timing.accepted_identity is not None
        assert timing.period_seconds == period
        assert metadata["revision"] == timing.accepted_revision
        assert metadata["source_generation"] == timing.accepted_identity.source_generation
        binding = probe._audio.current_input_runtime_pad_binding(slot)
        assert binding is not None and binding.is_current()
    probe.project.selected_pad = 0
    probe.project.input_mapping_enabled = True
    probe._audio.set_input_mapping_enabled(True)
    for number, key in ((40, "ui.select_pad:0"), (41, "ui.select_pad:1"),
                        (60, "pitch.note:7:set_retrigger")):
        probe.input_mapping.save_mapping(
            "midi", f"midi:note:1:{number}", LooperAction(key=key))
    assert probe.input_mapping.refresh_pitch_runtime_state()
    probe.ctx.audio.global_.toggle_bpm_lock()
    assert probe.project.bpm_lock
    assert probe.session.bpm_lock_anchor_pad_id == 0
    assert probe.session.bpm_lock_anchor_revision == probe.transport.bpm.current_timing(0).accepted_revision
    probe.ctx.audio.global_.set_speed(2.0)
    assert probe.project.speed == 2.0
    assert probe.session.master_period_seconds == .25


def mouse(probe, slot, note):
    probe.project.selected_pad = slot
    probe.frame()
    before = set(probe.transport.pitch._owners)
    item = next(item for item in probe.geometry if item.key.value == note)
    io = imgui.get_io()
    io.add_mouse_pos_event(probe.origin[0] + item.center_x,
        probe.origin[1] + item.height * (.45 if item.key.black else .85))
    io.add_mouse_button_event(0, True)
    probe.frame()
    io.add_mouse_button_event(0, False)
    probe.frame()
    created = set(probe.transport.pitch._owners) - before
    assert len(created) == 1, (slot, note, probe.session.input_mapping_error,
                               probe.transport.pitch.status(slot))
    owner = probe.transport.pitch._owners[created.pop()]
    assert owner.published and owner.content_id == probe.project.pad_content[slot].instance_id
    capture = owner.capture
    assert (capture.base_shift, capture.extra_shift, capture.retrigger) == (note, 0, True)
    assert capture.current_control_slot == slot and capture.content_is_current
    assert probe.session.input_mapping_error is None
    return capture


def midi_handoff(probe):
    events = []
    deadline = monotonic() + 2
    while len(events) < 3:
        event = probe._audio.poll_input_events()
        if event is None:
            assert monotonic() < deadline, "actual MIDI barrier did not arrive"
            sleep(.001)
        else:
            events.append(event)
    assert [event.get("action_key") for event in events] == [
        "ui.select_pad:0", "pitch.note:7:set_retrigger", "ui.select_pad:1"]
    event = events[1]
    capture = event["captured_pitch"]
    assert event["direct"] and not event["dispatched"]
    assert capture.received_at_ns == event["received_at_ns"]
    for event in events:
        probe.input_mapping._handle_rust_input_event(event)
    owner = probe.transport.pitch._owners[capture.accepted_sequence]
    assert owner.capture is capture and owner.published
    assert owner.content_id == probe.project.pad_content[0].instance_id
    assert probe.project.selected_pad == 1
    assert probe.session.input_mapping_error is None
    return capture


def request_rate(probe):
    assert probe.session.bpm_lock_anchor_pad_id == 0
    timing = probe.transport.bpm.current_timing(0)
    assert timing is not None and timing.accepted_revision
    assert timing.accepted_identity is not None and timing.period_seconds == .5
    assert probe.project.manual_bpm[0] is None and probe.project.manual_bpm[1] is None
    probe.ctx.audio.global_.set_speed(1.8)
    assert probe.project.speed == 1.8
    assert probe.session.master_period_seconds == .5 / 1.8
    # Requested Python intent is not Native effective confirmation.
    status = probe.transport.global_params.global_rate_status()
    assert status.state == "queued" and status.requested_speed == 1.8
    assert status.effective_speed == 2.0 and status.effective_master_period == .25


def observe_rate(probe):
    probe.transport.global_params._poll_global_rate_status()
    status = probe.ctx.state.global_.global_rate_status()
    actual = probe._audio.global_rate_status()
    assert actual is not None
    for field in ("state", "request_revision", "effect_revision", "requested_speed",
                  "effective_speed", "requested_master_period", "effective_master_period",
                  "execution_frame", "error"):
        assert getattr(status, field) == actual[field], (field, status, actual)


def settle(probe, capture, rejected):
    probe.transport.pitch.poll()
    assert capture.accepted_sequence not in probe.transport.pitch._owners
    if rejected:
        assert capture.state == "rejected" and not capture.effective
        assert probe.transport.pitch.status(0) == (
            "error", "Audio changed during pitch preparation; try again.")
    else:
        assert capture.state == "applied" and capture.effective
        assert probe.transport.pitch.status(capture.current_control_slot) == ("applied", None)
    assert probe.session.input_mapping_error is None
"#;

#[derive(Clone, Debug)]
struct RateReceipt {
    request: u64,
    effect: u64,
    state: String,
    requested_speed: f64,
    effective_speed: f64,
    requested_period: Option<f64>,
    effective_period: Option<f64>,
    execution: Option<u64>,
    error: Option<String>,
}

struct Original {
    py: Py<CapturedPitchAction>,
    action: CapturedPitchAction,
    sequence: u64,
    received: u64,
    content: crate::audio_engine::content_placement::ContentKey,
    t: u64,
    e: u64,
    id: usize,
    note: i32,
    prepared_native: Option<usize>,
}

impl Original {
    fn unchanged(&self) {
        assert_eq!(self.action.sequence(), self.sequence);
        assert_eq!(self.action.received_timestamp(), self.received);
        assert_eq!(self.action.content_key(), self.content);
        assert_eq!(self.action.original_due_frame(), self.t);
        assert_eq!(self.action.execution_target(), self.e);
        assert_eq!(
            self.action.render().pitch,
            PitchIntent::new(self.note, 0).unwrap()
        );
        assert!(!self.action.exclusive());
        assert!(self.action.sync());
    }
}

/// Independent active-frame scalar epochs; no productive SourcePlayback or reader.
#[derive(Clone)]
struct ScalarClock {
    origin: f64,
    elapsed: u64,
    rate: f64,
    target: f64,
    until_step: usize,
    end: usize,
}

impl ScalarClock {
    fn new(id: usize, rate: f64) -> Self {
        Self {
            origin: LOOP_START as f64,
            elapsed: 0,
            rate,
            target: rate,
            until_step: 0,
            end: ENDS[id],
        }
    }

    fn phase(&self) -> f64 {
        LOOP_START as f64
            + (self.origin - LOOP_START as f64 + self.elapsed as f64 * self.rate)
                .rem_euclid((self.end - LOOP_START) as f64)
    }

    fn target(&mut self, target: f64) {
        if self.target != target {
            self.target = target;
            self.until_step = 0;
        }
    }

    fn before_sample(&mut self) {
        if self.until_step == 0 && self.rate != self.target {
            self.origin = self.phase();
            self.elapsed = 0;
            self.rate = if (self.target - self.rate).abs() <= 0.05 {
                self.target
            } else {
                self.rate + (self.target - self.rate).clamp(-0.05, 0.05)
            };
            self.until_step = 512;
        }
    }

    fn advance(&mut self) {
        self.elapsed += 1;
        self.until_step = self.until_step.saturating_sub(1);
    }
}

fn scalar_pcm(source: &SampleBuffer, phase: f64, end: usize) -> f32 {
    let frame = phase.floor() as usize;
    let fraction = (phase - frame as f64) as f32;
    let right = if frame + 1 == end {
        LOOP_START
    } else {
        frame + 1
    };
    let left = source.samples[frame];
    left + (source.samples[right] - left) * fraction
}

struct Reference {
    clock: ScalarClock,
    raw: Option<RawRubberBand>,
    filter: PerPadDspChain,
    note: i32,
    lock: bool,
    native: usize,
    generation: u64,
}

impl Reference {
    fn pitch(&self) -> f64 {
        let h = 2.0_f64.powf(f64::from(self.note) / 12.0);
        if self.lock { h / self.clock.rate } else { h }
    }

    fn birth(
        source: &SampleBuffer,
        id: usize,
        note: i32,
        rate: f64,
        lock: bool,
        native: usize,
        generation: u64,
    ) -> Self {
        let h = 2.0_f64.powf(f64::from(note) / 12.0);
        let pitch = if lock { h / rate } else { h };
        let raw = if pitch == 1.0 {
            None
        } else {
            assert_eq!(RawRubberBand::history_bound(pitch), HISTORY);
            let mut raw = RawRubberBand::new(pitch);
            for age in -(HISTORY as i64)..0 {
                let phase = LOOP_START as f64
                    + (age as f64 * rate).rem_euclid((ENDS[id] - LOOP_START) as f64);
                raw.sample(scalar_pcm(source, phase, ENDS[id]), pitch);
            }
            Some(raw)
        };
        Self {
            clock: ScalarClock::new(id, rate),
            raw,
            filter: PerPadDspChain::new(id, RATE as f32, 1),
            note,
            lock,
            native,
            generation,
        }
    }

    fn sample(&mut self, source: &SampleBuffer) -> f32 {
        self.clock.before_sample();
        let input = scalar_pcm(source, self.clock.phase(), self.clock.end);
        let pitch = self.pitch();
        let wet = self
            .raw
            .as_mut()
            .map_or(input, |raw| raw.sample(input, pitch));
        self.filter.begin_frame();
        let output = self.filter.process_sample(0, wet);
        self.clock.advance();
        output
    }
}

struct MasterClock {
    frame: u64,
    beat: f64,
    period: f64,
}

impl MasterClock {
    fn beat_at(&self, frame: u64) -> f64 {
        self.beat + (frame - self.frame) as f64 / (f64::from(RATE) * self.period)
    }

    fn change(&mut self, frame: u64, period: f64) {
        self.beat = self.beat_at(frame);
        self.frame = frame;
        self.period = period;
    }
}

struct RateRoute {
    route: MouseRoute,
    helper: Py<PyModule>,
    parameters: Consumer<ControlParameterMessage>,
    scheduler: FixedCapacityScheduler<MAX_SCHEDULED_EVENTS>,
    feedback: Producer<AudioMessage>,
    messages: Arc<Mutex<Consumer<AudioMessage>>>,
    sources: [SampleBuffer; 2],
    references: [Option<Reference>; 2],
    master: Option<MasterClock>,
    origin: f64,
    lock: bool,
    maximum: f32,
    compared: usize,
    last_callback_first: Option<(u64, f32, f32)>,
    starts: [usize; 2],
}

fn complete_source(id: usize) -> SampleBuffer {
    SampleBuffer {
        samples: (0..3 * RATE as usize)
            .map(|frame| {
                let time = frame as f64 / f64::from(RATE);
                (0.11 * (std::f64::consts::TAU * (173.0 + 94.0 * id as f64) * time + 0.17).sin()
                    + 0.035
                        * (std::f64::consts::TAU * (613.0 + 113.0 * id as f64) * time + 0.43).cos())
                    as f32
            })
            .collect::<Vec<_>>()
            .into(),
        channels: 1,
        residency: None,
    }
    .with_complete_source(RATE)
}

impl RateRoute {
    fn new(lock: bool) -> Self {
        let mut fixture = Fixture::new(lock);
        let (producer, consumer) = RingBuffer::new(CAPACITY);
        *fixture.control.producer.lock().unwrap() = producer;
        fixture.control.consumer = consumer;
        let (feedback, messages) = RingBuffer::new(CAPACITY);
        let messages = Arc::new(Mutex::new(messages));
        fixture.control.engine.test_control_channels =
            Some((fixture.control.producer.clone(), messages.clone()));
        let (parameters, parameter_consumer) = RingBuffer::new(CAPACITY);
        fixture.control.engine.test_parameter_producer = Some(Arc::new(Mutex::new(parameters)));
        let sources = [complete_source(0), complete_source(1)];
        for (id, source) in sources.iter().enumerate() {
            publish_tone(&fixture.control, id, source.clone(), true);
            fixture.mixer.load_sample(id, source.clone());
        }
        adopt_current_sources(&mut fixture, lock);
        // Real source/request capture, verified complete mono, acceptance and own
        // callback ACK; labels below are explicitly synthetic, never inference.
        for (id, period) in PERIODS.into_iter().enumerate() {
            constant_timing::set_intent(
                &fixture.control.engine,
                &fixture.control.producer,
                id,
                TimingIntent::Automatic,
            )
            .unwrap();
            fixture.drain();
            let ticket = l3_constant_timing_fixture::publish_synthetic_loaded_timing(
                &fixture.control.engine,
                &fixture.control.producer,
                id,
                period,
            );
            assert_eq!(ticket.publication_status().unwrap(), "pending");
            fixture.drain();
            assert_eq!(ticket.publication_status().unwrap(), "accepted");
            let binding = fixture
                .control
                .engine
                .current_input_runtime_pad_binding(id)
                .unwrap()
                .unwrap();
            assert!(binding.current());
            assert_eq!(binding.binding.accepted.unwrap().period_seconds, period);
            fixture
                .mixer
                .set_pad_loop_region(id, 0.1, Some(ENDS[id] as f64 / f64::from(RATE)));
        }
        let origin = fixture
            .control
            .engine
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .unwrap()
            .binding
            .accepted
            .unwrap()
            .origin_seconds;
        let route = MouseRoute::from_fixture(fixture, lock, None);
        let helper = Python::attach(|py| {
            let helper = PyModule::from_code(
                py,
                &CString::new(HELPER).unwrap(),
                c"piano_rate_controller_helpers.py",
                c"piano_rate_controller_helpers",
            )
            .unwrap();
            helper
                .getattr("configure")
                .unwrap()
                .call1((route.probe.clone_ref(py),))
                .unwrap();
            helper.unbind()
        });
        let mut result = Self {
            route,
            helper,
            parameters: parameter_consumer,
            scheduler: FixedCapacityScheduler::new(),
            feedback,
            messages,
            sources,
            references: [None, None],
            master: None,
            origin,
            lock,
            maximum: 0.0,
            compared: 0,
            last_callback_first: None,
            starts: [0, 0],
        };
        result.callback(0);
        result.assert_no_attacks();
        let initial = result.rate();
        assert_eq!(initial.state, "applied");
        assert_eq!(initial.effective_speed, 2.0);
        assert_eq!(initial.effective_period, Some(0.25));
        assert_eq!(result.route.transport.master_period_seconds(), Some(0.25));
        result.observe_rate();
        result
    }

    fn publish_clock(&self) {
        Python::attach(|py| {
            let now = self.route.engine.borrow(py).input_clock.capture_ns();
            let frame = self.route.transport.output_frame();
            self.route.clock.publish(OutputClockSnapshot {
                valid: true,
                observed_at_ns: now,
                audible_at_ns: now,
                output_frame: frame,
                sample_rate_hz: RATE,
                master_period_seconds: self.route.transport.master_period_seconds(),
                downbeat_frame: self.route.transport.downbeat_frame(),
                master_beat: self.route.transport.beat_position_at_frame(frame).unwrap(),
                freshness_ns: 2_000_000_000,
            });
        });
    }

    fn callback(&mut self, frames: usize) {
        let frame = self.route.transport.output_frame();
        let mut output = vec![0.0; frames];
        drain_control_messages(
            &mut self.route.consumer,
            &mut self.scheduler,
            frame,
            &mut TriggerQuantization::Immediate,
            &mut self.route.transport,
            &mut self.route.mixer,
            &mut self.feedback,
            &mut self.route.retirement,
        );
        drain_parameter_messages(
            &mut self.parameters,
            &mut self.route.mixer,
            &mut self.route.transport,
        );
        render_scheduled_audio(
            &mut self.route.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            frame,
            1,
            &mut self.route.transport,
            &mut self.feedback,
            &mut self.route.retirement,
        );
        self.route.transport.advance_by_rendered_frames(frames);
        assert!(
            self.route.scheduler.is_empty(),
            "the parent 16-cell scheduler remains unused"
        );
        if frames > 0 {
            let order = self
                .route
                .mixer
                .voices
                .iter()
                .filter(|voice| voice.active)
                .map(|voice| voice.sample_id)
                .collect::<Vec<_>>();
            let expected_ids = self
                .references
                .iter()
                .enumerate()
                .filter_map(|(id, reference)| reference.as_ref().map(|_| id))
                .collect::<Vec<_>>();
            let mut actual_ids = order.clone();
            actual_ids.sort_unstable();
            assert_eq!(actual_ids, expected_ids, "independent live voice set");
            for (offset, actual) in output.into_iter().enumerate() {
                let mut expected = 0.0_f32;
                for id in &order {
                    expected += self.references[*id]
                        .as_mut()
                        .unwrap()
                        .sample(&self.sources[*id]);
                }
                let difference = (actual - expected).abs();
                if offset == 0 {
                    self.last_callback_first = Some((frame, actual, expected));
                }
                self.maximum = self.maximum.max(difference);
                assert!(actual.is_finite() && expected.is_finite());
                assert!(
                    difference < 2e-5,
                    "frame={} actual={actual} independent_raw_sum={expected} error={difference}",
                    frame + offset as u64
                );
                self.compared += 1;
            }
            self.check_voices();
        }
        // Observer-only virtual clock: all musical fields come from the just
        // serviced Rust transport, never Python's earlier requested period.
        self.publish_clock();
    }

    fn check_voices(&self) {
        if let Some(master) = &self.master {
            let frame = self.route.transport.output_frame();
            assert_eq!(
                self.route.transport.master_period_seconds(),
                Some(master.period)
            );
            assert!(
                (self.route.transport.beat_position_at_frame(frame).unwrap()
                    - master.beat_at(frame))
                .abs()
                    < 1e-9
            );
        }
        for (id, reference) in self.references.iter().enumerate() {
            let Some(reference) = reference else {
                continue;
            };
            let voice = self.voice(id);
            let actual = voice.source_playback.position();
            let phase = actual.frame as f64 + actual.fraction;
            assert!(
                (phase - reference.clock.phase()).abs() < 2e-7,
                "id={id} actual_phase={phase} independent_phase={}",
                reference.clock.phase()
            );
            assert!((voice.source_playback.tempo_ratio() - reference.clock.rate).abs() < 1e-14);
            assert_eq!(voice.source_playback.rate_target(), reference.clock.target);
            assert_eq!(voice.generation, reference.generation);
            assert_eq!(voice.stretch.native_state_address(), reference.native);
            assert_eq!(
                voice.sample.as_ref().unwrap().source_address(),
                self.sources[id].source_address()
            );
            assert_eq!(voice.volume, 1.0);
            assert!(!voice.paused);
            if let Some(raw) = &reference.raw {
                assert_eq!(voice.stretch.pending_fifo_frames(), raw.fifo());
                assert!(
                    voice.stretch.owns_pitch_contract(
                        PitchIntent::new(reference.note, 0).unwrap(),
                        self.lock
                    )
                );
                let actual_pitch = voice.stretch.native_pitch_scale_observation().unwrap();
                assert!((actual_pitch - reference.pitch()).abs() < 1e-12);
            }
        }
    }

    fn voice(&self, id: usize) -> &VoiceSlot {
        self.route
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == id)
            .unwrap()
    }

    fn take_attacks(&mut self) -> Vec<(usize, usize, u64)> {
        let mut result = Vec::new();
        let mut messages = self.messages.lock().unwrap();
        while let Ok(message) = messages.pop() {
            match message {
                AudioMessage::ContentFeedback {
                    id,
                    cell,
                    lifetime,
                    kind: 1,
                    ..
                } => {
                    self.starts[id] += 1;
                    result.push((id, cell, lifetime));
                }
                AudioMessage::SampleStarted { .. } => {
                    panic!("tracked source lacks own feedback tags")
                }
                _ => {}
            }
        }
        result
    }

    fn assert_no_attacks(&mut self) {
        assert!(self.take_attacks().is_empty());
    }

    fn render_to(&mut self, target: u64) {
        let mut index = 0;
        assert!(target >= self.route.transport.output_frame());
        while self.route.transport.output_frame() < target {
            let frames = CHUNKS[index % CHUNKS.len()]
                .min((target - self.route.transport.output_frame()) as usize);
            self.callback(frames);
            self.assert_no_attacks();
            index += 1;
        }
    }

    fn capture(&self, id: usize, note: i32, midi: bool) -> Py<CapturedPitchAction> {
        self.publish_clock();
        Python::attach(|py| {
            if midi {
                assert_eq!((id, note), (0, 7));
                let engine = self.route.engine.borrow(py);
                let runtime = engine.input_runtime.as_ref().unwrap();
                for bytes in [[0x90, 40, 127], [0x90, 60, 127], [0x90, 41, 127]] {
                    assert!(runtime.inject_midi_message(&bytes));
                }
                drop(engine);
                self.helper
                    .getattr(py, "midi_handoff")
                    .unwrap()
                    .call1(py, (self.route.probe.clone_ref(py),))
                    .unwrap()
                    .extract(py)
                    .unwrap()
            } else {
                self.helper
                    .getattr(py, "mouse")
                    .unwrap()
                    .call1(py, (self.route.probe.clone_ref(py), id, note))
                    .unwrap()
                    .extract(py)
                    .unwrap()
            }
        })
    }

    fn admit(&mut self, id: usize, note: i32, midi: bool) -> Original {
        let frame = self.route.transport.output_frame();
        let py = self.capture(id, note, midi);
        let action = Python::attach(|py_context| py.borrow(py_context).clone());
        assert_eq!(action.original_due_frame(), frame);
        self.callback(0);
        let original = Original {
            sequence: action.sequence(),
            received: action.received_timestamp(),
            content: action.content_key(),
            t: action.original_due_frame(),
            e: action.execution_target(),
            id,
            note,
            py,
            action,
            prepared_native: None,
        };
        original.unchanged();
        original
    }

    fn wait_action(&mut self, original: &mut Original) {
        let frame = self.route.transport.output_frame();
        let deadline = Instant::now() + WAIT;
        loop {
            let runtime = self.route.mixer.pitch_runtime.as_ref().unwrap();
            if runtime.has_ready_action(original.sequence) {
                original.prepared_native = runtime.ready_pitch_native_address(original.sequence);
                assert!(original.prepared_native.is_some_and(|address| address != 0));
                break;
            }
            assert!(!original.action.effective());
            assert_ne!(
                original.action.state(),
                "rejected",
                "{:?}",
                original.action.reason()
            );
            assert!(
                Instant::now() < deadline,
                "host pitch preparation exceeded 15 s"
            );
            self.callback(0);
            self.assert_no_attacks();
            std::thread::sleep(Duration::from_millis(1));
        }
        original.unchanged();
        assert_eq!(self.route.transport.output_frame(), frame);
    }

    fn install_birth_reference(&mut self, original: &Original) {
        let voice = self.voice(original.id);
        if let Some(address) = original.prepared_native {
            assert_eq!(voice.stretch.native_state_address(), address);
        }
        let native = voice.stretch.native_state_address();
        let generation = voice.generation;
        let rate = PERIODS[original.id] / self.route.transport.master_period_seconds().unwrap();
        assert_eq!(voice.source_playback.tempo_ratio(), rate);
        let index = self
            .route
            .mixer
            .voices
            .iter()
            .position(|voice| voice.active && voice.sample_id == original.id)
            .unwrap();
        let snapshot = self
            .route
            .mixer
            .capture_pitch_voice_snapshot(index)
            .unwrap();
        assert_eq!(
            snapshot.plan.loop_period,
            Some((ENDS[original.id] - LOOP_START) as f64)
        );
        assert_eq!(
            voice.source_timing.accepted.unwrap().period_seconds,
            PERIODS[original.id]
        );
        if original.id == 0 && self.master.is_none() {
            self.master = Some(MasterClock {
                frame: original.e,
                beat: (0.1 - self.origin) / PERIODS[0],
                period: 0.25,
            });
        }
        self.references[original.id] = Some(Reference::birth(
            &self.sources[original.id],
            original.id,
            original.note,
            rate,
            self.lock,
            native,
            generation,
        ));
        let attacks = self.take_attacks();
        assert_eq!(
            attacks,
            vec![(
                original.id,
                original.content.cell().index(),
                original.content.lifetime()
            )]
        );
        Python::attach(|py| {
            assert_eq!(
                original
                    .py
                    .getattr(py, "effective_output_frame")
                    .unwrap()
                    .extract::<u64>(py)
                    .unwrap(),
                original.e
            )
        });
        original.unchanged();
    }

    fn start(&mut self, id: usize, note: i32, midi: bool) -> Original {
        let mut original = self.admit(id, note, midi);
        if !original.action.effective() {
            self.assert_no_attacks();
            self.wait_action(&mut original);
            self.render_to(original.e);
            self.callback(0);
        }
        assert!(
            original.action.effective(),
            "{:?}",
            original.action.reason()
        );
        self.install_birth_reference(&original);
        self.callback(1);
        self.assert_no_attacks();
        self.settle(&original, false);
        original
    }

    fn settle(&self, original: &Original, rejected: bool) {
        Python::attach(|py| {
            self.helper
                .getattr(py, "settle")
                .unwrap()
                .call1(
                    py,
                    (
                        self.route.probe.clone_ref(py),
                        original.py.clone_ref(py),
                        rejected,
                    ),
                )
                .unwrap()
        });
    }

    fn rate(&self) -> RateReceipt {
        Python::attach(|py| {
            let status = self
                .route
                .engine
                .borrow(py)
                .global_rate_status(py)
                .unwrap()
                .unwrap();
            let status = status.bind(py);
            let value = |key| status.get_item(key).unwrap().unwrap();
            RateReceipt {
                request: value("request_revision").extract().unwrap(),
                effect: value("effect_revision").extract().unwrap(),
                state: value("state").extract().unwrap(),
                requested_speed: value("requested_speed").extract().unwrap(),
                effective_speed: value("effective_speed").extract().unwrap(),
                requested_period: value("requested_master_period").extract().unwrap(),
                effective_period: value("effective_master_period").extract().unwrap(),
                execution: value("execution_frame").extract().unwrap(),
                error: value("error").extract().unwrap(),
            }
        })
    }

    fn observe_rate(&self) {
        Python::attach(|py| {
            self.helper
                .getattr(py, "observe_rate")
                .unwrap()
                .call1(py, (self.route.probe.clone_ref(py),))
                .unwrap()
        });
    }

    fn wait_rate(&mut self, execution: u64) -> usize {
        let frame = self.route.transport.output_frame();
        let deadline = Instant::now() + WAIT;
        loop {
            self.callback(0);
            self.assert_no_attacks();
            let voice = self
                .route
                .mixer
                .voices
                .iter_mut()
                .find(|voice| voice.active && voice.sample_id == 0)
                .unwrap();
            if voice.stretch.source_preparation_ready()
                && voice.stretch.prepared_target_output_frame() == Some(execution)
            {
                let native = voice.stretch.prepared_native_address();
                assert_ne!(native, 0);
                return native;
            }
            assert_eq!(self.route.transport.output_frame(), frame);
            assert!(
                Instant::now() < deadline,
                "host rate preparation exceeded 15 s"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn wait_claims(&mut self) {
        let deadline = Instant::now() + WAIT;
        while Python::attach(|py| self.route.engine.borrow(py).pitch_work_capacity.used()) != 0 {
            self.callback(0);
            self.assert_no_attacks();
            assert!(
                Instant::now() < deadline,
                "pitch custody did not actually retire"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn history(lock: bool, midi: bool) {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let mut route = RateRoute::new(lock);
    let first = route.start(0, 12, false);
    let peer = route.start(1, 7, false);
    route.wait_claims();
    route.callback(733);
    route.assert_no_attacks();
    assert_eq!(route.voice(0).source_playback.tempo_ratio(), 2.0);
    assert_eq!(route.voice(1).source_playback.tempo_ratio(), 1.5);
    if lock {
        assert!(route.references[0].as_ref().unwrap().raw.is_none());
        assert!(route.voice(0).stretch.productive_history().is_none());
        assert_eq!(route.voice(0).stretch.native_primed_output_frames(), 0);
    }
    let mut old = route.admit(0, 7, midi);
    route.assert_no_attacks();
    assert_eq!(old.e, old.t + 8192);
    route.wait_action(&mut old);
    let old_prepared = old.prepared_native.unwrap();
    let peer_native = route.voice(1).stretch.native_state_address();
    let peer_generation = route.voice(1).generation;
    let original_generation = route.voice(0).generation;
    let frame = route.route.transport.output_frame();
    let before_rate = route.rate();
    Python::attach(|py| {
        route
            .helper
            .getattr(py, "request_rate")
            .unwrap()
            .call1(py, (route.route.probe.clone_ref(py),))
            .unwrap()
    });
    route.observe_rate(); // Real queued Native receipt must not look applied.
    route.callback(0);
    route.assert_no_attacks();
    let request = route.rate();
    assert!(request.request > before_rate.request);
    assert_eq!(request.requested_speed, NEW_SPEED);
    assert_eq!(request.requested_period, Some(PERIODS[0] / NEW_SPEED));
    assert!(request.error.is_none());
    route.observe_rate();
    let mut prepared_raw = None;
    let rate_e = request.execution.unwrap();
    if lock {
        assert_eq!(request.state, "preparing");
        assert_eq!(request.effective_speed, 2.0);
        assert_eq!(request.effective_period, Some(0.25));
        assert_eq!(rate_e, frame + HISTORY as u64);
        let prepared_address = route.wait_rate(rate_e);
        assert_ne!(prepared_address, old_prepared);
        // A-reset native history starts at original RateF, with actual old r2.
        // A's already warm EQ remains untouched by the later Native adoption.
        let mut raw = RawRubberBand::new(1.0);
        let mut clock = route.references[0].as_ref().unwrap().clock.clone();
        for _ in 0..HISTORY {
            clock.before_sample();
            assert_eq!(clock.rate, 2.0);
            raw.sample(scalar_pcm(&route.sources[0], clock.phase(), ENDS[0]), 1.0);
            clock.advance();
        }
        prepared_raw = Some((raw, prepared_address));
        route.render_to(rate_e);
        assert_eq!(route.rate().state, "preparing");
        route.callback(0);
        route.assert_no_attacks();
    } else {
        assert_eq!(request.state, "applied");
        assert_eq!(rate_e, frame);
    }
    let applied = route.rate();
    assert_eq!(applied.state, "applied");
    assert_eq!(applied.request, request.request);
    assert_eq!(applied.effect, request.request);
    assert_eq!(applied.execution, Some(rate_e));
    assert_eq!(applied.effective_speed, NEW_SPEED);
    assert_eq!(applied.effective_period, Some(PERIODS[0] / NEW_SPEED));
    assert!(applied.error.is_none());
    if let Some((raw, address)) = prepared_raw {
        assert_eq!(route.voice(0).stretch.native_state_address(), address);
        let reference = route.references[0].as_mut().unwrap();
        reference.raw = Some(raw);
        reference.native = address;
    }
    route
        .master
        .as_mut()
        .unwrap()
        .change(rate_e, PERIODS[0] / NEW_SPEED);
    for (id, reference) in route.references.iter_mut().enumerate() {
        reference
            .as_mut()
            .unwrap()
            .clock
            .target(PERIODS[id] / (PERIODS[0] / NEW_SPEED));
    }
    route.publish_clock();
    route.observe_rate();
    assert!(rate_e < old.e);
    assert_eq!(route.voice(0).generation, original_generation);
    assert_eq!(route.voice(1).generation, peer_generation);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer_native);
    // Every sample crosses the original action's E: refusal must not restore the
    // old global tuple, restart A, reset B, or emit any attack feedback.
    route.render_to(old.e);
    route.callback(1);
    route.assert_no_attacks();
    old.unchanged();
    assert_eq!(old.action.state(), "rejected");
    assert!(!old.action.effective());
    assert_eq!(old.action.reason(), Some("prepared_mismatch"));
    assert_eq!(route.voice(0).generation, original_generation);
    assert_eq!(
        route.route.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(12, 0).unwrap())
    );
    assert_eq!(route.rate().request, applied.request);
    assert_eq!(route.rate().effective_period, applied.effective_period);
    route.settle(&old, true);
    route.wait_claims();
    route.callback(HISTORY + 73);
    route.assert_no_attacks();
    assert_eq!(
        route.voice(0).source_playback.tempo_ratio(),
        PERIODS[0] / (PERIODS[0] / NEW_SPEED)
    );
    assert_eq!(
        route.voice(1).source_playback.tempo_ratio(),
        PERIODS[1] / (PERIODS[0] / NEW_SPEED)
    );
    let fresh = route.start(0, 7, midi);
    assert!(fresh.sequence > old.sequence);
    assert_eq!(route.voice(0).generation, original_generation + 1);
    route.wait_claims();
    for frames in CHUNKS.into_iter().chain([HISTORY + 37]) {
        route.callback(frames);
        route.assert_no_attacks();
    }
    assert_eq!(route.starts, [2, 1]);
    assert_eq!(route.voice(1).generation, peer_generation);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer_native);
    assert_eq!(
        route.route.transport.master_period_seconds(),
        applied.effective_period
    );
    assert!(route.scheduler.is_empty());
    for original in [&first, &peer, &old, &fresh] {
        original.unchanged();
    }
    route.observe_rate();
    eprintln!(
        "piano_global_rate lock={lock} origin={} old_sequence={} old_T={} old_E={} old_refusal={:?} rate_request={} rate_E={rate_e} period_before=.25 period_after={:?} source_rates={:?} fresh_sequence={} fresh_T={} fresh_E={} raw_pcm_max={} compared_frames={} starts={:?}; complete-resident declared synthetic timing; A-reset/constant-history software only; no device/deadline/continuous-O/acoustic/P6 acceptance",
        if midi { "raw_midi" } else { "actual_gui" },
        old.sequence,
        old.t,
        old.e,
        old.action.reason(),
        applied.request,
        applied.effective_period,
        [
            route.voice(0).source_playback.tempo_ratio(),
            route.voice(1).source_playback.tempo_ratio()
        ],
        fresh.sequence,
        fresh.t,
        fresh.e,
        route.maximum,
        route.compared,
        route.starts
    );
    route.route.close();
}

#[test]
fn public_rate_controls_fence_old_pitch_then_fresh_controller_attack_owns_pcm() {
    if let Ok(child) = std::env::var(CHILD) {
        let (lock, midi) = match child.as_str() {
            "gui-off" => (false, false),
            "gui-on" => (true, false),
            "midi-off" => (false, true),
            "midi-on" => (true, true),
            _ => panic!("unknown declared piano/rate child"),
        };
        history(lock, midi);
        return;
    }
    for child in ["gui-off", "gui-on", "midi-off", "midi-on"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(CHILD, child)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "piano/rate {child} failed:\n{stdout}\n{stderr}"
        );
        assert!(stdout.contains("1 passed; 0 failed"));
        assert!(stderr.contains("piano_global_rate"));
        print!("{stdout}");
        if !stderr.is_empty() {
            eprint!("{stderr}");
        }
    }
}
