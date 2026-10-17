//! Successful original GUI/MIDI attacks through productive 64-owner custody cycles.
//!
//! The output clock is deliberately held while the real worker prepares. This is
//! engineering pressure, not a performer deadline. Process observations precede
//! construction of every independent Raw Native oracle and include Python, the
//! harness and retained PCM. No App, device, installed extension or private input.

#![cfg(windows)]

use super::*;
use crate::audio_engine::c3_process_metrics;
use crate::audio_engine::constants::MAX_SCHEDULED_EVENTS;
use crate::audio_engine::dsp::PerPadDspChain;
use crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_source_ids;
use crate::audio_engine::pitch_runtime_tests::source_feed_tests::RawRubberBand;
use crate::test_memory::{ThreadAllocations, observe_current_thread};
use rtrb::{Consumer, Producer};
use serde_json::{Value, json};

const TEST: &str = "audio_engine::pitch_runtime_tests::piano_mouse_tests::productive_custody_workload_tests::productive_gui_midi_custody_workload";
const CHILD: &str = "FLITZI_PRODUCTIVE_PITCH_CUSTODY_ROW";
const QUEUE: usize = 1024;
const CLAIMS: usize = 64;
const CYCLES: usize = 3;
const CADENCE: usize = 32;
const HOST_WAIT: Duration = Duration::from_secs(15);

const HELPERS: &str = r#"
from time import monotonic, sleep
from unittest.mock import patch
from imgui_bundle import imgui
from flitzis_looper.input_mapping.actions import LooperAction
from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import PadContentIdentity


def configure(probe, count):
    # The existing project and mapping-load boundary is synthetic. Prevent every
    # mapping save from touching the Human's actual configuration as well.
    probe.storage.enter_context(patch(
        "flitzis_looper.input_mapping.controller.save_midi_mapping_file"))
    for slot in range(6):
        if slot < count:
            probe.project.sample_paths[slot] = f"synthetic-custody-{slot}.wav"
            probe.project.sample_durations[slot] = 3.0
            probe.project.pad_content[slot] = PadContentIdentity(instance_id=f"{slot + 1:032x}")
            probe.project.pad_key_intent[slot] = PadKeyIntent(source=SourceKeyVersion(raw_key="C"))
            probe.project.manual_bpm[slot] = 120.0
            probe.project.pad_loop_auto[slot] = False
            probe.project.pad_loop_start_s[slot] = 0.1
            probe.project.pad_loop_end_s[slot] = 2.1
        else:
            probe.project.sample_paths[slot] = None
            probe.project.sample_durations[slot] = None
            probe.project.pad_content[slot] = None
    probe.project.input_mapping_enabled = True
    probe._audio.set_input_mapping_enabled(True)
    for slot in range(6):
        probe.input_mapping.save_mapping(
            "midi", f"midi:note:1:{40 + slot}", LooperAction(key=f"ui.select_pad:{slot}"))
    for number, note in ((60, 7), (61, -7), (62, 2)):
        probe.input_mapping.save_mapping(
            "midi", f"midi:note:2:{number}",
            LooperAction(key=f"pitch.note:{note}:set_retrigger"))
    assert probe.input_mapping.refresh_pitch_runtime_state()
    assert probe.session.input_mapping_error is None


def mouse(probe, slot, note):
    probe.project.selected_pad = slot
    probe.frame()
    before = set(probe.transport.pitch._owners)
    item = next(item for item in probe.geometry if item.key.value == note)
    io = imgui.get_io()
    io.add_mouse_pos_event(probe.origin[0] + item.center_x,
                           probe.origin[1] + item.height * (0.45 if item.key.black else 0.85))
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
    assert capture.current_control_slot == slot and capture.content_is_current
    assert (capture.base_shift, capture.extra_shift, capture.retrigger) == (note, 0, True)
    assert probe.session.input_mapping_error is None
    return capture


def midi_handoff(probe, slot, barrier, note, refuse):
    expected = [f"ui.select_pad:{slot}", f"pitch.note:{note}:set_retrigger",
                f"ui.select_pad:{barrier}"]
    events = []
    deadline = monotonic() + 2
    while len(events) < 3:
        event = probe._audio.poll_input_events()
        if event is None:
            assert monotonic() < deadline, "actual MIDI dispatcher did not reach barrier"
            sleep(0.001)
        else:
            events.append(event)
    assert [event.get("action_key") for event in events] == expected
    event = events[1]
    capture = event["captured_pitch"]
    assert event["direct"] and not event["dispatched"]
    assert capture.received_at_ns == event["received_at_ns"]
    assert capture.current_control_slot == slot and capture.content_is_current
    assert (capture.base_shift, capture.extra_shift, capture.retrigger) == (note, 0, True)
    for event in events:
        probe.input_mapping._handle_rust_input_event(event)
    assert probe.project.selected_pad == barrier, (probe.project.selected_pad, barrier)
    if refuse:
        assert capture.state == "rejected" and capture.reason == "capacity"
        assert capture.accepted_sequence not in probe.transport.pitch._owners
        assert probe.session.input_mapping_error is not None
    else:
        owner = probe.transport.pitch._owners[capture.accepted_sequence]
        assert owner.capture is capture and owner.published
        assert owner.content_id == probe.project.pad_content[slot].instance_id
        assert probe.session.input_mapping_error is None
    return capture


def settle(probe):
    probe.transport.pitch.poll()
    assert not probe.transport.pitch._owners
    assert probe.session.input_mapping_error is None
"#;

#[derive(Clone, Copy)]
struct Profile {
    count: usize,
    shared: bool,
    lock: bool,
}

impl Profile {
    fn from_row(row: usize) -> Self {
        assert!(row < 16);
        Self {
            count: [1, 2, 4, 6][row / 4],
            shared: row & 2 == 0,
            lock: row & 1 != 0,
        }
    }
}

struct Original {
    captured: Py<CapturedPitchAction>,
    action: CapturedPitchAction,
    sequence: u64,
    timestamp: u64,
    content: crate::audio_engine::content_placement::ContentKey,
    original_t: u64,
    execution: u64,
    slot: usize,
    note: i32,
    prepared_native: usize,
}

impl Original {
    fn assert_original(&self) {
        assert_eq!(self.action.sequence(), self.sequence);
        assert_eq!(self.action.received_timestamp(), self.timestamp);
        assert_eq!(self.action.content_key(), self.content);
        assert_eq!(self.action.original_due_frame(), self.original_t);
        assert_eq!(self.action.execution_target(), self.execution);
        assert_eq!(
            self.action.render().pitch,
            PitchIntent::new(self.note, 0).unwrap()
        );
        assert!(!self.action.exclusive());
    }
}

struct VoiceCut {
    id: usize,
    native: usize,
    generation: u64,
    source: usize,
    frame: usize,
    fraction: f64,
    fifo: (usize, usize),
    volume: f32,
}

struct Cut {
    start: u64,
    pcm_start: usize,
    frames: usize,
    // Native voice-array order is retained solely to reproduce f32 summation.
    voices: Vec<VoiceCut>,
}

struct Effect {
    frame: u64,
    id: usize,
    note: i32,
    native: usize,
    generation: u64,
}

struct Workload {
    route: MouseRoute,
    helper: Py<PyModule>,
    scheduler: FixedCapacityScheduler<MAX_SCHEDULED_EVENTS>,
    feedback: Producer<AudioMessage>,
    messages: Arc<Mutex<Consumer<AudioMessage>>>,
    sources: Vec<SampleBuffer>,
    pcm: Vec<f32>,
    cuts: Vec<Cut>,
    effects: Vec<Effect>,
    callbacks: usize,
    callback_ns: u128,
    profile: Profile,
}

fn source(seed: usize, voices: usize) -> SampleBuffer {
    // Scale immutable input before capture, retaining audible contributions from
    // every voice without changing voice gains or muting peers.
    let scale = 0.18_f32 / voices as f32;
    SampleBuffer {
        samples: (0..3 * RATE as usize)
            .map(|frame| {
                let phase = frame as f64 / f64::from(RATE);
                (std::f64::consts::TAU * (173.0 + seed as f64 * 41.0) * phase + 0.19).sin() as f32
                    * scale
            })
            .collect::<Vec<_>>()
            .into(),
        channels: 1,
        residency: None,
    }
    .with_complete_source(RATE)
    .window(LOOP_START, LOOP_END, 2, ResidentContext::KeyLockFiniteLoop)
    .unwrap()
}

impl Workload {
    fn new(profile: Profile) -> Self {
        let mut fixture = Fixture::new(profile.lock);
        // The actual stream uses 1024 incoming/feedback cells. Preserve the
        // original producer Arc already retained by InputRuntime/AudioEngine.
        let (producer, consumer) = RingBuffer::new(QUEUE);
        *fixture.control.producer.lock().unwrap() = producer;
        fixture.control.consumer = consumer;
        let (feedback, messages) = RingBuffer::new(QUEUE);
        let messages = Arc::new(Mutex::new(messages));
        fixture.control.engine.test_control_channels =
            Some((fixture.control.producer.clone(), messages.clone()));
        let common = source(0, profile.count);
        let sources = (0..profile.count)
            .map(|id| {
                if profile.shared {
                    common.clone()
                } else {
                    source(id, profile.count)
                }
            })
            .collect::<Vec<_>>();
        for (id, sample) in sources.iter().enumerate() {
            publish_tone(&fixture.control, id, sample.clone(), true);
            fixture.mixer.load_sample(id, sample.clone());
            fixture.mixer.set_pad_loop_region(
                id,
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
            );
            fixture.mixer.set_pad_key_lock(id, profile.lock);
        }
        let ids = (0..profile.count).collect::<Vec<_>>();
        adopt_current_source_ids(&mut fixture, profile.lock, &ids);
        for id in &ids {
            let ownership = &fixture.control.engine.input_runtime_ownership;
            assert_eq!(
                ownership.callback_feedback_key(*id),
                ownership.slot_snapshot(*id).unwrap().content_key()
            );
        }
        for left in 0..profile.count {
            for right in 0..left {
                assert_eq!(
                    Arc::ptr_eq(&sources[left].samples, &sources[right].samples),
                    profile.shared
                );
                let ownership = &fixture.control.engine.input_runtime_ownership;
                assert_ne!(
                    ownership.slot_snapshot(left).unwrap().content_key(),
                    ownership.slot_snapshot(right).unwrap().content_key()
                );
            }
        }
        let route = MouseRoute::from_fixture(fixture, profile.lock, None);
        let helper = Python::attach(|py| {
            let helper = PyModule::from_code(
                py,
                &CString::new(HELPERS).unwrap(),
                c"productive_custody_helpers.py",
                c"productive_custody_helpers",
            )
            .unwrap();
            helper
                .getattr("configure")
                .unwrap()
                .call1((route.probe.clone_ref(py), profile.count))
                .unwrap();
            helper.unbind()
        });
        Self {
            route,
            helper,
            scheduler: FixedCapacityScheduler::new(),
            feedback,
            messages,
            sources,
            pcm: Vec::new(),
            cuts: Vec::new(),
            effects: Vec::new(),
            callbacks: 0,
            callback_ns: 0,
            profile,
        }
    }

    fn callback(&mut self, frames: usize) {
        let frame = self.route.transport.output_frame();
        let mut output = vec![0.0; frames]; // Harness storage precedes RT observation.
        let then = Instant::now();
        let ((), allocated) = observe_current_thread(|| {
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
        });
        self.callback_ns += then.elapsed().as_nanos();
        self.callbacks += 1;
        assert_eq!(
            allocated,
            ThreadAllocations::default(),
            "productive pitch callback"
        );
        self.route.transport.advance_by_rendered_frames(frames);
        assert!(
            self.route.scheduler.is_empty(),
            "the parent 16-cell scheduler is unused"
        );
        if frames > 0 {
            let voices = self
                .route
                .mixer
                .voices
                .iter()
                .filter(|voice| voice.active)
                .map(|voice| {
                    assert!(!voice.paused);
                    let position = voice.source_playback.position();
                    assert_eq!(voice.source_playback.tempo_ratio(), 1.0);
                    VoiceCut {
                        id: voice.sample_id,
                        native: voice.stretch.native_state_address(),
                        generation: voice.generation,
                        source: voice.sample.as_ref().unwrap().source_address(),
                        frame: position.frame,
                        fraction: position.fraction,
                        fifo: voice.stretch.pending_fifo_frames(),
                        volume: voice.volume,
                    }
                })
                .collect();
            self.cuts.push(Cut {
                start: frame,
                pcm_start: self.pcm.len(),
                frames,
                voices,
            });
            self.pcm.extend(output);
        }
    }

    fn discard_nonattack_feedback(&self) {
        let mut messages = self.messages.lock().unwrap();
        while let Ok(message) = messages.pop() {
            assert!(
                !matches!(
                    message,
                    AudioMessage::ContentFeedback { kind: 1, .. }
                        | AudioMessage::SampleStarted { .. }
                ),
                "unexpected attack outside original E"
            );
        }
    }

    fn render_to(&mut self, target: u64) {
        assert!(target >= self.route.transport.output_frame());
        while self.route.transport.output_frame() < target {
            self.callback((target - self.route.transport.output_frame()).min(512) as usize);
            self.discard_nonattack_feedback();
        }
    }

    fn capture(
        &mut self,
        slot: usize,
        note: i32,
        midi: bool,
        refuse: bool,
    ) -> Py<CapturedPitchAction> {
        self.route.refresh_clock();
        Python::attach(|py| {
            if midi {
                // A one-voice row has no second loaded target. Its barrier
                // selects that same living slot; multi-voice rows select another.
                let barrier = (slot + 1) % self.profile.count;
                let number = match note {
                    7 => 60,
                    -7 => 61,
                    2 => 62,
                    _ => unreachable!(),
                };
                let engine = self.route.engine.borrow(py);
                let runtime = engine.input_runtime.as_ref().unwrap();
                assert!(runtime.inject_midi_message(&[0x90, 40 + slot as u8, 127]));
                assert!(runtime.inject_midi_message(&[0x91, number, 127]));
                assert!(runtime.inject_midi_message(&[0x90, 40 + barrier as u8, 127]));
                drop(engine);
                self.helper
                    .getattr(py, "midi_handoff")
                    .unwrap()
                    .call1(
                        py,
                        (self.route.probe.clone_ref(py), slot, barrier, note, refuse),
                    )
                    .unwrap()
                    .extract(py)
                    .unwrap()
            } else {
                assert!(!refuse);
                self.helper
                    .getattr(py, "mouse")
                    .unwrap()
                    .call1(py, (self.route.probe.clone_ref(py), slot, note))
                    .unwrap()
                    .extract(py)
                    .unwrap()
            }
        })
    }

    fn admit(&mut self, slot: usize, note: i32, midi: bool) -> Original {
        let frame = self.route.transport.output_frame();
        // The selected reserve remains enabled. These declared pressure targets
        // differ from the actually applied tuple, so they require their own work.
        assert_ne!(
            self.route.mixer.pad_pitch_intent(slot).unwrap(),
            PitchIntent::new(note, 0).unwrap()
        );
        let captured = self.capture(slot, note, midi, false);
        let action = Python::attach(|py| captured.borrow(py).clone());
        assert_eq!(action.original_due_frame(), frame);
        self.callback(0); // Same actual frame freezes the original E.
        self.discard_nonattack_feedback();
        assert!(
            action.accepted() && !action.effective(),
            "{:?}",
            action.reason()
        );
        assert_eq!(action.execution_target(), frame + 8192);
        Original {
            sequence: action.sequence(),
            timestamp: action.received_timestamp(),
            content: action.content_key(),
            original_t: action.original_due_frame(),
            execution: action.execution_target(),
            slot,
            note,
            captured,
            action,
            prepared_native: 0,
        }
    }

    fn usage(&self) -> (usize, (usize, usize)) {
        Python::attach(|py| {
            let engine = self.route.engine.borrow(py);
            (
                engine.pitch_work_capacity.used(),
                engine.pitch_work_capacity.allocations(),
            )
        })
    }

    fn wait_ready(&mut self, originals: &mut [Original]) {
        let frame = self.route.transport.output_frame();
        let deadline = Instant::now() + HOST_WAIT;
        loop {
            self.callback(0);
            self.discard_nonattack_feedback();
            let runtime = self.route.mixer.pitch_runtime.as_ref().unwrap();
            if originals
                .iter()
                .all(|original| runtime.has_ready_action(original.sequence))
            {
                break;
            }
            for original in originals.iter() {
                original.assert_original();
                assert_ne!(
                    original.action.state(),
                    "rejected",
                    "{:?}",
                    original.action.reason()
                );
            }
            assert!(
                Instant::now() < deadline,
                "controlled host worker wait exceeded 15 s"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(self.route.transport.output_frame(), frame);
        let runtime = self.route.mixer.pitch_runtime.as_ref().unwrap();
        for original in originals.iter_mut() {
            original.prepared_native = runtime
                .ready_pitch_native_address(original.sequence)
                .unwrap();
            assert_ne!(original.prepared_native, 0);
        }
        for (index, original) in originals.iter().enumerate() {
            assert!(
                !originals[..index]
                    .iter()
                    .any(|other| other.prepared_native == original.prepared_native),
                "simultaneously held original candidates must be distinct Native owners"
            );
        }
    }

    fn effect(&mut self, original: &Original) {
        self.render_to(original.execution);
        original.assert_original();
        assert!(!original.action.effective());
        self.callback(1);
        assert!(
            original.action.effective(),
            "{:?}",
            original.action.reason()
        );
        original.assert_original();
        Python::attach(|py| {
            assert_eq!(
                original
                    .captured
                    .getattr(py, "effective_output_frame")
                    .unwrap()
                    .extract::<u64>(py)
                    .unwrap(),
                original.execution
            );
        });
        let voice = self
            .route
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == original.slot)
            .unwrap();
        assert_eq!(
            voice.stretch.native_state_address(),
            original.prepared_native
        );
        assert!(voice.stretch.owns_pitch_contract(
            PitchIntent::new(original.note, 0).unwrap(),
            self.profile.lock
        ));
        assert_eq!(
            voice.sample.as_ref().unwrap().source_address(),
            self.sources[original.slot].source_address()
        );
        let generation = voice.generation;
        let previous = self
            .effects
            .iter()
            .rev()
            .find(|effect| effect.id == original.slot);
        if let Some(previous) = previous {
            assert_eq!(generation, previous.generation + 1);
        }
        self.effects.push(Effect {
            frame: original.execution,
            id: original.slot,
            note: original.note,
            native: original.prepared_native,
            generation,
        });
        let mut messages = self.messages.lock().unwrap();
        let mut attacks = 0;
        while let Ok(message) = messages.pop() {
            if let AudioMessage::ContentFeedback {
                id,
                cell,
                lifetime,
                kind: 1,
                ..
            } = message
            {
                attacks += 1;
                assert_eq!(id, original.slot);
                assert_eq!(cell, original.content.cell().index());
                assert_eq!(lifetime, original.content.lifetime());
            } else {
                assert!(
                    !matches!(message, AudioMessage::SampleStarted { .. }),
                    "tracked sources require actual content-tagged attack feedback"
                );
            }
        }
        assert_eq!(
            attacks, 1,
            "each distinct E must have its own attack feedback"
        );
    }

    fn wait_retired(&mut self) {
        let frame = self.route.transport.output_frame();
        let deadline = Instant::now() + HOST_WAIT;
        while self.usage().0 != 0 {
            self.callback(0);
            self.discard_nonattack_feedback();
            assert!(
                Instant::now() < deadline,
                "actual off-callback custody retirement did not finish"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(self.route.transport.output_frame(), frame);
    }

    fn settle_controller(&self) {
        Python::attach(|py| {
            self.helper
                .getattr(py, "settle")
                .unwrap()
                .call1(py, (self.route.probe.clone_ref(py),))
                .unwrap()
        });
    }
}

struct Oracle {
    raw: RawRubberBand,
    filter: PerPadDspChain,
    age: u64,
    pitch: f64,
    native: usize,
    generation: u64,
}

fn source_sample(source: &SampleBuffer, age: i64) -> f32 {
    let frame = LOOP_START as i64 + age.rem_euclid((LOOP_END - LOOP_START) as i64);
    source.samples[frame as usize - source.resident_start()]
}

fn verify_after_measurement(workload: &Workload) -> f32 {
    // This is the FIRST construction/query/processing of any reference Native.
    // No reference helper runs before the product-phase process snapshots.
    let mut oracles: Vec<Option<Oracle>> = (0..workload.profile.count).map(|_| None).collect();
    let mut next_effect = 0;
    let mut maximum = 0.0_f32;
    let mut energy = 0.0_f64;
    for cut in &workload.cuts {
        while next_effect < workload.effects.len()
            && workload.effects[next_effect].frame == cut.start
        {
            let effect = &workload.effects[next_effect];
            let pitch = 2.0_f64.powf(f64::from(effect.note) / 12.0);
            assert!((0.5..=2.0).contains(&pitch));
            assert_eq!(RawRubberBand::history_bound(pitch), 4096);
            let mut raw = RawRubberBand::new(pitch);
            for age in -4096..0 {
                raw.sample(source_sample(&workload.sources[effect.id], age), pitch);
            }
            oracles[effect.id] = Some(Oracle {
                raw,
                filter: PerPadDspChain::new(effect.id, RATE as f32, 1),
                age: 0,
                pitch,
                native: effect.native,
                generation: effect.generation,
            });
            next_effect += 1;
        }
        assert!(
            next_effect == workload.effects.len()
                || workload.effects[next_effect].frame >= cut.start + cut.frames as u64,
            "recorded render cuts must retain every original E boundary"
        );
        // Declared attacks establish lifetime membership independently of the
        // renderer's observation. These looping voices never stop in this run.
        // A temporarily missing peer must not disappear from both PCM sums.
        let expected_ids: Vec<_> = oracles
            .iter()
            .enumerate()
            .filter_map(|(id, oracle)| oracle.as_ref().map(|_| id))
            .collect();
        let mut actual_ids: Vec<_> = cut.voices.iter().map(|voice| voice.id).collect();
        actual_ids.sort_unstable();
        assert_eq!(
            actual_ids, expected_ids,
            "every cut must contain exactly the independently born voices at frame {}",
            cut.start
        );
        for offset in 0..cut.frames {
            let mut expected = 0.0_f32;
            for voice in &cut.voices {
                let oracle = oracles[voice.id].as_mut().unwrap();
                let sample = oracle.raw.sample(
                    source_sample(&workload.sources[voice.id], oracle.age as i64),
                    oracle.pitch,
                );
                oracle.filter.begin_frame();
                // Neutral trim and master are both exactly one in this profile.
                let contribution = oracle.filter.process_sample(0, sample * 1.0) * voice.volume;
                expected += contribution * 1.0;
                oracle.age += 1;
            }
            let actual = workload.pcm[cut.pcm_start + offset];
            assert!(actual.is_finite() && expected.is_finite());
            let error = (actual - expected).abs();
            maximum = maximum.max(error);
            assert!(
                error < 2.0e-6,
                "frame={} actual={actual} independent_sum={expected} error={error}",
                cut.start + offset as u64
            );
            energy += f64::from(actual).powi(2);
        }
        for voice in &cut.voices {
            let oracle = oracles[voice.id].as_ref().unwrap();
            assert_eq!(voice.native, oracle.native);
            assert_eq!(voice.generation, oracle.generation);
            assert_eq!(voice.source, workload.sources[voice.id].source_address());
            assert_eq!(
                voice.frame,
                LOOP_START + oracle.age as usize % (LOOP_END - LOOP_START)
            );
            assert_eq!(voice.fraction, 0.0);
            assert_eq!(voice.fifo, oracle.raw.fifo());
            assert_eq!(voice.volume, 1.0);
        }
    }
    assert_eq!(next_effect, workload.effects.len());
    assert!(
        energy > 1.0,
        "all declared voices must contribute actual nonzero PCM"
    );
    maximum
}

fn run_row(row: usize) {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let profile = Profile::from_row(row);
    let mut workload = Workload::new(profile);
    let setup = c3_process_metrics::snapshot();
    let mut retained = Vec::new();
    let mut rejected = Vec::new();
    // Actual +2 GUI births make every voice productive before the load baseline.
    for id in 0..profile.count {
        let mut originals = [workload.admit(id, 2, false)];
        workload.wait_ready(&mut originals);
        workload.effect(&originals[0]);
        workload.wait_retired();
        retained.extend(originals);
        workload.settle_controller();
    }
    workload.callback(733);
    workload.discard_nonattack_feedback();
    let baseline = c3_process_metrics::snapshot();
    let mut phases: Vec<Value> = Vec::new();
    for cycle in 0..CYCLES {
        let note = if cycle == 1 { -7 } else { 7 };
        let before = workload.usage();
        assert_eq!(before.0, 0);
        let mut originals = Vec::with_capacity(CLAIMS);
        for index in 0..CLAIMS {
            let original = workload.admit(index % profile.count, note, index % 2 != 0);
            if let Some(previous) = originals.last() {
                let previous: &Original = previous;
                assert!(
                    original.sequence > previous.sequence
                        && original.timestamp > previous.timestamp
                );
                assert_eq!(original.execution, previous.execution + CADENCE as u64);
            }
            originals.push(original);
            workload.callback(CADENCE);
            workload.discard_nonattack_feedback();
        }
        assert!(workload.route.transport.output_frame() < originals[0].execution);
        assert_eq!(workload.usage().0, CLAIMS);
        workload.wait_ready(&mut originals);
        assert_eq!(workload.usage().0, CLAIMS);
        let prepared = workload.usage();
        assert_eq!(prepared.1.0, before.1.0 + CLAIMS);
        let overflow = workload.capture(0, note, true, true);
        Python::attach(|py| {
            let overflow = overflow.borrow(py);
            assert_eq!(overflow.reason(), Some("capacity"));
            assert!(!overflow.accepted() && !overflow.effective());
        });
        assert_eq!(
            workload.usage(),
            prepared,
            "65th refusal must allocate no work or Native"
        );
        phases.push(json!({"cycle":cycle,"phase":"64_genuinely_ready", "process":c3_process_metrics::snapshot(),
            "claims":prepared.0,"work_allocations":prepared.1.0,"native_constructions":prepared.1.1,
            "first_E":originals[0].execution,"last_E":originals[CLAIMS-1].execution}));
        for original in &originals {
            workload.effect(original);
        }
        workload.wait_retired();
        assert!(originals.iter().all(|original| original.action.effective()));
        retained.extend(originals); // Receipts remain live across every later cycle.
        assert_eq!(workload.usage().0, 0);
        assert_eq!(
            workload
                .route
                .mixer
                .voices
                .iter()
                .filter(|voice| voice.active)
                .count(),
            profile.count
        );
        // Fresh real recovery clears the controller's capacity error and proves
        // storage reuse while all successful and rejected receipts are retained.
        let mut recovery = [workload.admit(0, 2, true)];
        workload.wait_ready(&mut recovery);
        workload.effect(&recovery[0]);
        workload.wait_retired();
        retained.extend(recovery);
        workload.settle_controller();
        workload.callback(257);
        workload.discard_nonattack_feedback();
        phases.push(json!({"cycle":cycle,"phase":"actual_retired_and_fresh_recovery", "claims":workload.usage().0,
            "retained_success_receipts":retained.len(),"process":c3_process_metrics::snapshot()}));
        // Keep the failed original capture too; it must not retain heavy custody.
        Python::attach(|py| assert_eq!(overflow.borrow(py).reason(), Some("capacity")));
        rejected.push(overflow);
    }
    assert_eq!(
        workload.effects.len(),
        profile.count + CYCLES * (CLAIMS + 1)
    );
    workload.callback(4097);
    workload.discard_nonattack_feedback();
    workload.wait_retired();
    for original in &retained {
        original.assert_original();
        assert!(original.action.effective());
    }
    Python::attach(|py| {
        for capture in &rejected {
            assert_eq!(capture.borrow(py).reason(), Some("capacity"));
            assert!(!capture.borrow(py).effective());
        }
    });
    assert!(workload.scheduler.is_empty());
    let measured = c3_process_metrics::snapshot();
    let retained_pcm_bytes = workload.pcm.len() * size_of::<f32>();
    let verification_start = Instant::now();
    let maximum = verify_after_measurement(&workload);
    let after_verification = c3_process_metrics::snapshot();
    println!(
        "{}",
        json!({
            "measurement":"productive_pitch_custody_workload", "row":row,"voices":profile.count,
            "topology":if profile.shared {"shared_immutable_pcm"} else {"distinct_immutable_pcm"},
            "count_one_sharing_is_degenerate":profile.count == 1,"key_lock":profile.lock,
            "count_one_midi_barrier_is_same_loaded_slot":profile.count == 1,
            "sample_rate_hz":RATE,"channels":1,"actual_r":1,"cycles":CYCLES,
            "successful_pressure_actions":CYCLES*CLAIMS,"capacity_refusals":CYCLES,
            "successful_recovery_actions":CYCLES,"successful_births":profile.count,
            "control_feedback_capacity":QUEUE,"scheduler_capacity":MAX_SCHEDULED_EVENTS,
            "actual_claims_after_retirement":workload.usage().0,
            "retained_success_receipts":retained.len(),"actual_work_native_allocations":workload.usage().1,
            "retained_capacity_refusals":rejected.len(),
            "retained_pcm_bytes":retained_pcm_bytes,"rendered_frames":workload.pcm.len(),
            "callbacks_including_zero_frame_service":workload.callbacks,"callback_wall_ns":workload.callback_ns,
            "setup_process":setup,"productive_baseline":baseline,"phases":phases,
            "before_any_reference_native":measured,"post_reference_process":after_verification,
            "independent_pcm_max_error":maximum,"verification_wall_ns":verification_start.elapsed().as_nanos(),
            "scope":"isolated productive software path plus Python/harness/retained PCM; process lifetime peaks are cumulative, not reset cycle peaks; all Raw reference Native construction and execution follows the preverification snapshots; controlled host worker waits provide no performer or CPAL deadline; no C++ allocation, acoustic, continuous-O, variable-r, P6/216, device or Human acceptance"
        })
    );
    workload.route.close();
}

#[test]
fn productive_gui_midi_custody_workload() {
    if let Some(row) = std::env::var_os(CHILD) {
        run_row(row.to_str().unwrap().parse().unwrap());
        return;
    }
    for row in 0..16 {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(CHILD, row.to_string())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "productive custody row {row} failed:\n{stdout}\n{stderr}"
        );
        assert!(stdout.contains("1 passed; 0 failed"));
        assert!(stdout.contains("productive_pitch_custody_workload"));
        print!("{stdout}");
        if !stderr.is_empty() {
            eprint!("{stderr}");
        }
    }
}
