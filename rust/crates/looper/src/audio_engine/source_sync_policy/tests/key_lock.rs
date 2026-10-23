//! Actual prepared Native birth and retained rate edits under scalar global policy.
//! Device-free engineering proof; no mapped Key Lock or musical-quality claim.

use super::*;
use crate::audio_engine::audio_stream::drain_parameter_messages;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::global_rate_status::{RateError, RateState};
use crate::audio_engine::input_mapping::launch_admission_tests::RATE;
use crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_sources;
use crate::audio_engine::ordinary_start_tests::{complete_attack, wait_ready};
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::pitch_runtime_tests::{Fixture, LOOP_END, LOOP_START};
use crate::audio_engine::source_feed_history::SourceFeedSnapshot;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;
use std::sync::{Arc, Mutex};

const MASTER: f64 = 0.5;
const SOURCE: f64 = 0.625;
const POLICY_REVISION: u64 = 33;

fn publish(fixture: &Fixture, sync: bool) {
    Python::attach(|py| {
        let bindings = (0..NUM_SAMPLES)
            .map(|id| {
                (id < 2).then(|| {
                    Py::new(
                        py,
                        fixture
                            .control
                            .engine
                            .current_input_runtime_pad_binding(id)
                            .unwrap()
                            .unwrap(),
                    )
                    .unwrap()
                })
            })
            .collect();
        let rows = (0..NUM_SAMPLES)
            .map(|id| {
                let row = PyDict::new(py);
                row.set_item("sync_requested", id < 2).unwrap();
                row.set_item("intent_revision", 7_u64).unwrap();
                row.set_item("delivery", py.None()).unwrap();
                row.set_item("exact_quarters", py.None()).unwrap();
                row.unbind()
            })
            .collect();
        fixture
            .control
            .engine
            .set_input_runtime_state(
                py,
                true,
                (0..NUM_SAMPLES).map(|id| id < 2).collect(),
                vec![LOOP_START as f64 / f64::from(RATE); NUM_SAMPLES],
                vec![Some(LOOP_END as f64 / f64::from(RATE)); NUM_SAMPLES],
                Some(bindings),
                None, // Preserve the fixture's independently published Key Lock intent.
                Some(rows),
                Some(
                    serde_json::json!({
                        "schema_version":1, "sync":sync, "realtime_bpm":false,
                        "revision":POLICY_REVISION,
                    })
                    .to_string(),
                ),
            )
            .unwrap();
    });
}

fn feedback(fixture: &Fixture) -> Value {
    Python::attach(|py| {
        let status = fixture
            .control
            .engine
            .pad_source_sync_status(py, 0)
            .unwrap()
            .unwrap();
        let text: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (status,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&text).unwrap()
    })
}

fn assert_feedback(fixture: &Fixture, sync: bool, settled: bool) {
    let observed = feedback(fixture);
    assert_eq!(observed["active"], true);
    assert_eq!(
        observed["policy_mode"],
        if sync { "fixed" } else { "legacy" }
    );
    assert_eq!(observed["policy_revision"], POLICY_REVISION);
    assert_eq!(observed["policy_sync"], sync);
    assert_eq!(observed["policy_realtime_bpm"], false);
    assert_eq!(observed["intent_revision"], 7);
    assert_eq!(observed["settled"], settled);
    assert_eq!(observed["effective"], sync && settled);
    assert!(observed["map_revision"].is_null());
    assert_eq!(
        observed["source_identity"],
        fixture.voice(0).sample.as_ref().unwrap().source_address()
    );
    assert!(observed["source_generation"].as_u64().unwrap() > 0);
    Python::attach(|py| {
        let status = fixture
            .control
            .engine
            .pad_key_lock_status(py, 0)
            .unwrap()
            .unwrap();
        let status = status.bind(py);
        assert!(
            status
                .get_item("effective")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(
            status
                .get_item("ready")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(status.get_item("error").unwrap().unwrap().is_none());
    });
}

fn assert_pcm(pcm: &[f32]) {
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
}

fn assert_actual_read_history(fixture: &Fixture, settled_ratio: f64) {
    let voice = fixture.voice(0);
    let productive = voice.stretch.productive_history().unwrap();
    let sample = voice.sample.as_ref().unwrap();
    assert_eq!(productive.binding.source_address, sample.source_address());
    assert_eq!(productive.binding.resident, sample.resident_binding());
    assert_eq!(productive.binding.sample_rate_hz, RATE);
    let mut journal = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut journal));
    assert_eq!(journal.retained_range(), Some(0..journal.active_frames()));
    assert!(productive.fed_output_frames >= journal.active_frames());
    let spans = journal.spans().collect::<Vec<_>>();
    assert!(!spans.is_empty());
    for span in &spans {
        assert!(span.context.binding.same_source(productive.binding));
        let start = span.playback.position();
        let end = span.playback.position_at(span.frames).unwrap();
        let expected = LOOP_START as f64
            + (start.frame as f64 + start.fraction - LOOP_START as f64
                + span.frames as f64 * span.playback.tempo_ratio())
            .rem_euclid((LOOP_END - LOOP_START) as f64);
        assert!((end.frame as f64 + end.fraction - expected).abs() < 1e-8);
    }
    let last = spans.last().unwrap();
    assert!((last.playback.tempo_ratio() - settled_ratio).abs() < 1e-12);
    assert_eq!(
        last.playback.position_at(last.frames).unwrap(),
        productive.next_position
    );
    let taps = voice.stretch.native_tap_observation().unwrap();
    // This observation belongs to preparation; productive history and the actual
    // callback receipts above separately prove the subsequent positive source reads.
    assert_eq!(taps.missing_reads, 0);
}

#[test]
fn fixed_and_off_key_lock_keep_prepared_native_owner_through_public_rate_tuple() {
    Python::initialize();
    for sync in [true, false] {
        let mut fixture = Fixture::new(true);
        // The shared fixture installs its bank directly. Complete the existing
        // production LoadSample claim and callback SourceACK before requesting feedback.
        adopt_current_sources(&mut fixture, true);
        fixture.mixer.set_bpm_lock(sync);
        fixture.mixer.set_master_period(MASTER);
        fixture.mixer.set_pad_bpm(0, Some(60.0 / SOURCE));
        fixture.mixer.set_speed(1.5);
        assert!(fixture.transport.set_master_period(MASTER));
        let original_transport = fixture.transport;
        publish(&fixture, sync);
        let original = fixture
            .control
            .engine
            .capture_pad_start(
                0,
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap();
        fixture.drain();
        assert_eq!(original.original_frame(), Some(0));
        assert!(
            fixture
                .control
                .engine
                .publish_pad_start(&original, None)
                .unwrap()
        );
        fixture.drain();
        assert!(original.accepted(), "{:?}", original.reason());
        wait_ready(&mut fixture, &[original.sequence()]);
        let execution = original.execution_frame().unwrap();
        let pcm = complete_attack(&mut fixture, execution);
        assert_pcm(&pcm);
        assert_eq!(original.state(), "applied", "{:?}", original.reason());
        assert_eq!(original.effective_frame(), Some(execution));
        assert_eq!(fixture.starts(0), 1);
        let mode = if sync {
            SourceSyncMode::Fixed
        } else {
            SourceSyncMode::Legacy
        };
        let ratio = if sync { SOURCE / MASTER } else { 1.5 };
        let voice = fixture.voice(0);
        assert_eq!(
            voice.sync_request,
            SourceSyncRequest {
                mode,
                global_revision: Some(POLICY_REVISION),
                intent_revision: 7,
                policy_sync: Some(sync),
                policy_realtime_bpm: Some(false),
            }
        );
        assert_eq!(voice.sync_request, original.sync_request());
        assert!(voice.mapped_start.is_none());
        assert_eq!(
            voice.stretch.adopted_request_id(),
            Some(original.sequence())
        );
        let native = voice.stretch.native_state_address();
        assert_ne!(native, 0);
        assert!(
            voice
                .stretch
                .owns_pitch_contract(PitchIntent::default(), true)
        );
        assert!((voice.source_playback.tempo_ratio() - ratio).abs() < 1e-12);
        let position = voice.source_playback.position();
        assert!(
            (position.frame as f64 + position.fraction
                - LOOP_START as f64
                - pcm.len() as f64 * ratio)
                .abs()
                < 1e-8
        );
        assert_actual_read_history(&fixture, ratio);
        assert_feedback(&fixture, sync, true);
        assert!(fixture.transport.matches_clock_epoch(&original_transport));

        let generation = fixture.voice(0).generation;
        let origin = fixture.voice(0).stretch.native_origin_tuple();
        assert_eq!(
            origin,
            Some((PitchIntent::default(), true, original.sequence()))
        );
        let requested_period = 0.4;
        let requested_speed = 0.75;
        let target = if sync {
            SOURCE / requested_period
        } else {
            requested_speed
        };
        let (producer, mut consumer) = rtrb::RingBuffer::new(8);
        fixture.control.engine.test_parameter_producer = Some(Arc::new(Mutex::new(producer)));
        let capture_frame = fixture.transport.output_frame();
        let before_rate = fixture.voice(0).source_playback;
        fixture
            .control
            .engine
            .set_speed_and_master_period(requested_speed, requested_period)
            .unwrap();
        drain_parameter_messages(&mut consumer, &mut fixture.mixer, &mut fixture.transport);
        fixture.drain();
        fixture.render(0);
        assert!(consumer.is_empty());
        let effect = fixture
            .control
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap();
        assert_eq!(effect.effect.state, RateState::Applied);
        assert_eq!(effect.effect.error, RateError::None);
        assert_eq!(effect.effect.revision, effect.request_revision);
        assert_eq!(effect.effect.execution_frame, Some(capture_frame));
        assert_eq!(effect.requested.speed, requested_speed);
        assert_eq!(effect.requested.period, Some(requested_period));
        assert_eq!(effect.effect.effective.speed, requested_speed);
        assert_eq!(effect.effect.effective.period, Some(requested_period));
        assert_eq!(
            fixture.transport.master_period_seconds(),
            Some(requested_period)
        );
        assert_eq!(fixture.transport.output_frame(), capture_frame);
        assert!(fixture.voice(0).source_playback.matches_exact(&before_rate));
        let mut changed_pcm = fixture.render(1);
        assert_feedback(&fixture, sync, false);
        changed_pcm.extend(fixture.pcm(8999));
        assert_pcm(&changed_pcm);
        let voice = fixture.voice(0);
        assert_eq!(voice.generation, generation);
        assert_eq!(voice.stretch.native_state_address(), native);
        assert_eq!(voice.stretch.native_origin_tuple(), origin);
        assert_eq!(
            voice.stretch.adopted_request_id(),
            Some(original.sequence())
        );
        assert_eq!(voice.sync_request, original.sync_request());
        assert!(
            voice
                .stretch
                .owns_pitch_contract(PitchIntent::default(), true)
        );
        assert!((voice.source_playback.tempo_ratio() - target).abs() < 1e-12);
        assert_eq!(fixture.starts(0), 1);
        assert_actual_read_history(&fixture, target);
        assert_feedback(&fixture, sync, true);
        assert_eq!(
            fixture.transport.master_period_seconds(),
            Some(requested_period)
        );
    }
}
