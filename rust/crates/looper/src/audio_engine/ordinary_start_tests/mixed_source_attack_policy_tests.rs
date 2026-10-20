//! Real mixed-rate WAV loading, accepted fractional timing and original quantized attacks.
//! This synthetic device-free probe does not reproduce or accept the Human's H2 report.
#![cfg(windows)]

use super::*;
use crate::audio_engine::cold_load::admit_for_format_selected;
use crate::audio_engine::cold_residency::ResidentLoadHint;
use crate::audio_engine::constant_timing;
use crate::audio_engine::input_mapping::PitchPadProjection;
use crate::audio_engine::input_runtime_binding;
use crate::audio_engine::key_lock_start_policy::KeyLockStartBehavior;
use crate::audio_engine::pitch_actions::{CapturedPitchAction, PitchKeyMode};
use crate::audio_engine::resident_relocation::{self, ResidentWindowTicket};
use crate::audio_engine::{LoaderEvent, NUM_SAMPLES};
use flitzis_looper_analysis::tempo_acceptance::{
    IndependentTimingOrigin, TimingAcceptanceDecision,
};
use flitzis_looper_analysis::tempo_evidence::TimingBound;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyInt};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

const WAIT: Duration = Duration::from_secs(5);
const LOAD_WAIT: Duration = Duration::from_secs(10);
const SOURCE_PERIOD: f64 = 0.500_012_3;
const SOURCE_PULSES: usize = 64;
const ACCEPTED_ORIGIN: f64 = -0.125_012_3;
const GRID_PERIOD: f64 = 0.500_123_456_789;
const GRID_BEAT_AT_CAPTURE: f64 = 0.125_02;
const CAPTURE_FRAME: u64 = 1_000;
const PCM: usize = 24_000;

fn write_source(path: &Path, input_rate: u32, hz: f64) {
    let frames =
        ((SOURCE_PULSES as f64 * SOURCE_PERIOD + 0.25) * f64::from(input_rate)).ceil() as usize;
    let mut samples = Vec::with_capacity(frames);
    for frame in 0..frames {
        let seconds = frame as f64 / f64::from(input_rate);
        let pulse_age = seconds.rem_euclid(SOURCE_PERIOD);
        let pulse = if pulse_age < 0.008 {
            0.70 * (-pulse_age * 900.0).exp() * (pulse_age * 1_800.0 * std::f64::consts::TAU).cos()
        } else {
            0.0
        };
        let tone = 0.12 * (seconds * hz * std::f64::consts::TAU).sin();
        samples.push(((tone + pulse) * 32_000.0).round() as i16);
    }
    let data_bytes = u32::try_from(samples.len() * 2).unwrap();
    let mut bytes = Vec::with_capacity(data_bytes as usize + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&input_rate.to_le_bytes());
    bytes.extend_from_slice(&(input_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, &bytes).unwrap();
    assert_eq!(
        u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
        input_rate
    );
}

fn load_actual_source(fixture: &mut Fixture, directory: &Path, id: usize, input_rate: u32) {
    let path = directory.join(format!("source-{id}-{input_rate}.wav"));
    write_source(&path, input_rate, if id == 0 { 160.0 } else { 233.0 });
    assert!(Arc::ptr_eq(
        fixture.control.engine.control_producer().unwrap(),
        &fixture.control.producer,
    ));
    let request = admit_for_format_selected(
        &fixture.control.engine,
        id,
        path.to_string_lossy().into_owned(),
        (
            false,
            false,
            true,
            Some(ResidentLoadHint {
                start_s: LOOP_START as f64 / f64::from(RATE),
                end_s: LOOP_END as f64 / f64::from(RATE),
                key_lock: true,
            }),
        ),
        fixture.control.producer.clone(),
        (1, RATE, directory.join("samples")),
    )
    .unwrap();
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + LOAD_WAIT;
    let mut success = false;
    loop {
        fixture.drain();
        while let Ok(event) = fixture.control.engine.loader_rx.lock().unwrap().try_recv() {
            match event.into_test_payload() {
                LoaderEvent::Success {
                    id: actual,
                    request_id,
                    ..
                } => {
                    assert_eq!((actual, request_id), (id, request));
                    success = true;
                }
                LoaderEvent::Error { .. } => panic!("actual mixed-rate load failed"),
                _ => {}
            }
        }
        if success && fixture.control.engine.cold_loading[id].load(Ordering::Acquire) == 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "mixed-rate load {id}/{input_rate} timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), frame);
    let engine = &fixture.control.engine;
    assert_eq!(engine.loaded_source_generations.lock().unwrap()[id].1, RATE);
    let sample = engine.sample_cache.lock().unwrap()[id].clone().unwrap();
    assert_eq!(sample.channels, 1);
    assert!(sample.resident_start() > 0);
    assert!(sample.resident_end() < sample.frame_count());
    assert!(sample.samples.len() < sample.source_sample_count());
    engine.cold_leases.lock().unwrap()[id]
        .as_ref()
        .unwrap()
        .verify_reference(&sample)
        .unwrap();
    let binding = input_runtime_binding::capture(engine, id).unwrap().unwrap();
    assert!(binding.current());
    assert_eq!(binding.binding.sample_rate_hz, RATE);
    assert_eq!(
        engine.input_runtime_ownership.callback_feedback_key(id),
        binding.binding.placement.and_then(|p| p.content_key())
    );
}

fn publish_actual_fractional_acceptance(fixture: &mut Fixture) {
    fixture
        .control
        .engine
        .set_pad_timing_intent(0, "automatic")
        .unwrap();
    fixture.drain();
    let ticket = constant_timing::prepare(
        &fixture.control.engine,
        0,
        TimingBound {
            halfwidth_seconds: 0.05,
            provenance: "explicit error envelope for generated regular-pulse QM probe".into(),
        },
    )
    .unwrap();
    let beat_count = Python::attach(|py| {
        let metadata = ticket.metadata(py).unwrap();
        let metadata = metadata.bind(py).cast::<PyDict>().unwrap();
        assert_eq!(
            metadata
                .get_item("sample_rate_hz")
                .unwrap()
                .unwrap()
                .extract::<u32>()
                .unwrap(),
            RATE
        );
        metadata
            .get_item("beat_seconds")
            .unwrap()
            .unwrap()
            .extract::<Vec<f64>>()
            .unwrap()
            .len()
    });
    assert!(
        beat_count >= 24,
        "regular source did not yield the required real QM support"
    );
    let hypotheses = json!([{
        "id": "generated-quarter-pulses",
        "provenance": "independent generator places one declared quarter pulse every SOURCE_PERIOD",
        "verification": "verified",
        "quarter_note_denominator": 1,
        "quarter_counts": (0..beat_count).map(|index| Some(index as i64)).collect::<Vec<_>>(),
    }])
    .to_string();
    constant_timing::publish(
        &fixture.control.engine,
        &fixture.control.producer,
        &ticket,
        &hypotheses,
        IndependentTimingOrigin {
            seconds: ACCEPTED_ORIGIN,
            provenance: "independently selected signed fractional origin for the generated source".into(),
        },
        TimingAcceptanceDecision {
            policy_version: "generated-mixed-rate-attack-probe-v1".into(),
            provenance: "explicit engineering acceptance of generated pulse-quarter timing; no musical or Human acceptance".into(),
        },
    )
    .unwrap();
    fixture.drain();
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    let binding = input_runtime_binding::capture(&fixture.control.engine, 0)
        .unwrap()
        .unwrap();
    let accepted = binding.binding.accepted.unwrap();
    assert!(binding.current());
    assert_eq!(accepted.sample_rate_hz, RATE);
    assert_eq!(accepted.origin_seconds.to_bits(), ACCEPTED_ORIGIN.to_bits());
    assert!((0.45..0.55).contains(&accepted.period_seconds));
    assert_ne!((accepted.origin_seconds * f64::from(RATE)).fract(), 0.0);
}

fn refresh_rows(fixture: &Fixture) {
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    for id in [0, 1] {
        loaded[id] = true;
        starts[id] = LOOP_START as f64 / f64::from(RATE);
        ends[id] = Some(LOOP_END as f64 / f64::from(RATE));
        bindings[id] = input_runtime_binding::capture(&fixture.control.engine, id).unwrap();
        rows[id] = PitchPadProjection {
            intent: fixture.mixer.pad_pitch_intent(id).unwrap(),
            source_root: Some(0),
            source_mode: Some(PitchKeyMode::Major),
            key_lock: true,
            request_sequence: runtime.pitch_request_sequence(id).unwrap(),
            ..PitchPadProjection::default()
        };
    }
    runtime
        .set_runtime_state_with_pitch(
            true,
            loaded,
            starts,
            ends,
            bindings.iter().map(Option::as_ref).collect(),
            rows,
        )
        .unwrap();
}

fn public_set(fixture: &mut Fixture, id: usize, shift: i32) -> CapturedPitchAction {
    let action = Python::attach(|py| {
        fixture.refresh_clock();
        let timestamp = PyInt::new(py, fixture.clock.read().unwrap().audible_at_ns);
        fixture
            .control
            .engine
            .capture_pad_pitch_action(
                id,
                &format!("pitch.extra:{shift}:set"),
                Some(timestamp.as_any()),
            )
            .unwrap()
    });
    assert_eq!(
        action.original_due_frame(),
        fixture.transport.output_frame()
    );
    assert!(
        fixture
            .control
            .engine
            .publish_pitch_action(&action)
            .unwrap()
    );
    fixture.drain();
    fixture.render(1);
    assert!(
        action.effective(),
        "public SET failed: {:?}",
        action.reason()
    );
    assert_eq!(
        fixture.mixer.pad_pitch_intent(id),
        Some(PitchIntent::new(0, shift).unwrap())
    );
    action
}

struct Probe {
    fixture: Fixture,
    _sources: tempfile::TempDir,
}

impl Probe {
    fn new() -> Self {
        let mut fixture = Fixture::new(false);
        let sources = tempfile::tempdir().unwrap();
        load_actual_source(&mut fixture, sources.path(), 0, 44_100);
        load_actual_source(&mut fixture, sources.path(), 1, RATE);
        publish_actual_fractional_acceptance(&mut fixture);
        for id in [0, 1] {
            let generation = fixture
                .control
                .engine
                .loaded_source_generations
                .lock()
                .unwrap()[id]
                .0;
            fixture
                .control
                .engine
                .set_pad_key_lock_with_source(id, true, Some(generation))
                .unwrap();
        }
        fixture.drain();
        for id in [0, 1] {
            assert!(fixture.mixer.key_lock_for_measurement(id));
        }
        refresh_rows(&fixture);
        public_set(&mut fixture, 0, 7);
        public_set(&mut fixture, 1, -5);
        assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));
        assert!(fixture.control.engine.can_change_key_lock_start_behavior());
        assert!(
            fixture
                .control
                .engine
                .set_key_lock_start_behavior("attack_at_grid")
                .unwrap()
        );
        fixture.render_to(CAPTURE_FRAME);
        assert!(fixture.transport.set_master_period(GRID_PERIOD));
        assert!(
            fixture
                .transport
                .anchor_beat_position_at_frame(GRID_BEAT_AT_CAPTURE, CAPTURE_FRAME)
        );
        Self {
            fixture,
            _sources: sources,
        }
    }

    fn drain_grid(&mut self) {
        drain_with_mode(
            &mut self.fixture,
            TriggerQuantization::Grid { step_64ths: 16 },
        );
    }

    fn wait_window(&mut self, ticket: &ResidentWindowTicket) {
        let frame = self.fixture.transport.output_frame();
        let deadline = Instant::now() + WAIT;
        loop {
            self.drain_grid();
            resident_relocation::reconcile(&self.fixture.control.engine).unwrap();
            if ticket.publication_status() == "accepted" {
                assert!(ticket.is_current());
                break;
            }
            assert!(
                !matches!(
                    ticket.publication_status(),
                    "failed" | "rejected" | "cancelled"
                ),
                "window failed: {:?}",
                ticket.error()
            );
            assert!(
                Instant::now() < deadline,
                "mixed-rate finite Window ACK timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(self.fixture.transport.output_frame(), frame);
    }

    fn starts(&mut self, ids: &[usize]) -> Vec<CapturedPadStart> {
        let starts = ids
            .iter()
            .map(|&id| {
                self.fixture
                    .control
                    .engine
                    .capture_pad_start(
                        id,
                        LOOP_START as f64 / f64::from(RATE),
                        Some(LOOP_END as f64 / f64::from(RATE)),
                        1.0,
                        false,
                        None,
                        None,
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        self.drain_grid();
        for start in &starts {
            assert_eq!(start.start_behavior(), KeyLockStartBehavior::AttackAtGrid);
            let ticket = self
                .fixture
                .control
                .engine
                .prepare_captured_pad_start(start)
                .unwrap();
            self.wait_window(&ticket);
            assert!(
                self.fixture
                    .control
                    .engine
                    .publish_pad_start(start, Some(&ticket))
                    .unwrap()
            );
        }
        self.drain_grid();
        let frame = self.fixture.transport.output_frame();
        let deadline = Instant::now() + WAIT;
        loop {
            let runtime = self.fixture.mixer.pitch_runtime.as_mut().unwrap();
            runtime.poll();
            if starts
                .iter()
                .all(|start| runtime.has_ready_action(start.sequence()))
            {
                break;
            }
            for start in &starts {
                assert_ne!(
                    start.state(),
                    "rejected",
                    "original preparation: {:?}",
                    start.reason()
                );
            }
            assert!(
                Instant::now() < deadline,
                "mixed-rate original workers timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(self.fixture.transport.output_frame(), frame);
        for start in &starts {
            assert!(start.accepted());
            assert!(start.original_frame().unwrap() > frame);
            assert_eq!(start.execution_frame(), start.original_frame());
        }
        starts
    }
}

fn original_identity(
    start: &CapturedPadStart,
) -> (
    u64,
    crate::audio_engine::content_placement::ContentKey,
    u64,
    Option<u64>,
    Option<u64>,
) {
    (
        start.sequence(),
        start.content_key(),
        start.received_timestamp(),
        start.original_frame(),
        start.execution_frame(),
    )
}

fn assert_native_attack(fixture: &Fixture, start: &CapturedPadStart, id: usize, shift: i32) {
    assert_eq!(start.state(), "applied", "PAD {id}: {:?}", start.reason());
    assert_eq!(start.effective_frame(), start.execution_frame());
    assert_eq!(fixture.starts(id), 1);
    assert_eq!(
        fixture.mixer.pad_pitch_intent(id),
        Some(PitchIntent::new(0, shift).unwrap())
    );
    let voice = fixture.voice(id);
    assert_ne!(voice.stretch.native_state_address(), 0);
    assert_eq!(voice.stretch.adopted_request_id(), Some(start.sequence()));
    assert!(voice.stretch.wet_current_source_output_ready());
}

#[test]
fn mixed_actual_input_rates_fractional_acceptance_and_future_grid_keep_both_original_attacks() {
    let mut probe = Probe::new();
    let starts = probe.starts(&[0, 1]);
    let identities = starts.iter().map(original_identity).collect::<Vec<_>>();
    let expected = CAPTURE_FRAME
        + ((1.0 - GRID_BEAT_AT_CAPTURE) * GRID_PERIOD * f64::from(RATE)).ceil() as u64;
    assert!(expected > CAPTURE_FRAME + 8_192);
    assert!(
        starts
            .iter()
            .all(|start| start.original_frame() == Some(expected))
    );
    assert!(
        probe
            .fixture
            .render_to(expected)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let mut pcm = probe.fixture.render(1);
    pcm.extend(probe.fixture.pcm(PCM));
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| sample.abs() > 0.001));
    assert!(probe.fixture.scheduler.is_empty());
    for (id, shift) in [(0, 7), (1, -5)] {
        assert_eq!(original_identity(&starts[id]), identities[id]);
        assert_native_attack(&probe.fixture, &starts[id], id, shift);
    }
}

#[test]
fn public_post_preparation_set_rejects_only_its_original_start_and_fresh_retry_works() {
    let mut probe = Probe::new();
    let starts = probe.starts(&[0, 1]);
    let identities = starts.iter().map(original_identity).collect::<Vec<_>>();
    let target = starts[0].execution_frame().unwrap();
    assert_eq!(starts[1].execution_frame(), Some(target));
    // A real later non-retriggering SET changes only A's tuple while both original
    // ordinary candidates remain owned by their original future scheduler entries.
    let changed = public_set(&mut probe.fixture, 0, 3);
    assert!(changed.effective());
    assert!(probe.fixture.mixer.voices.iter().all(|voice| !voice.active));
    probe.fixture.render_to(target);
    let mut other_pcm = probe.fixture.render(1);
    other_pcm.extend(probe.fixture.pcm(4_096));
    assert_eq!(starts[0].state(), "rejected");
    assert_eq!(starts[0].reason(), Some("prepared_mismatch"));
    assert_eq!(starts[0].effective_frame(), None);
    assert_eq!(probe.fixture.starts(0), 0);
    assert!(
        !probe
            .fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    assert_native_attack(&probe.fixture, &starts[1], 1, -5);
    assert!(other_pcm.iter().all(|sample| sample.is_finite()));
    assert!(other_pcm.iter().any(|sample| sample.abs() > 0.001));
    for id in [0, 1] {
        assert_eq!(original_identity(&starts[id]), identities[id]);
    }
    let other_generation = probe.fixture.voice(1).generation;
    let fresh = probe.starts(&[0]).pop().unwrap();
    assert!(fresh.sequence() > starts[0].sequence());
    let fresh_frame = fresh.execution_frame().unwrap();
    probe.fixture.render_to(fresh_frame);
    probe.fixture.render(1);
    assert_eq!(probe.fixture.voice(1).generation, other_generation);
    assert_eq!(probe.fixture.starts(1), 1);
    assert_eq!(original_identity(&starts[0]), identities[0]);
    assert_eq!(starts[0].reason(), Some("prepared_mismatch"));
    // Isolate the new A attack's actual PCM only after proving B kept its lifetime.
    probe
        .fixture
        .mixer
        .pause_sample_at_output_frame(1, probe.fixture.transport.output_frame());
    let fresh_pcm = probe.fixture.pcm(PCM);
    assert!(fresh_pcm.iter().all(|sample| sample.is_finite()));
    assert!(fresh_pcm.iter().any(|sample| sample.abs() > 0.001));
    assert_native_attack(&probe.fixture, &fresh, 0, 3);
    assert!(probe.fixture.scheduler.is_empty());
}
