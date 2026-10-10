//! Native production publication/guard/grid paths with independently constructed fixture units.

use super::super::{buffer_retirement::ImmediateAudioBufferRetirement, mixer::RtMixer};
use super::*;
use flitzis_looper_analysis::tempo_evidence::{
    BeatThisModelIdentity, BeatThisRawEvidence, BeatThisRequestIdentity,
};
use serde_json::json;

pub(super) const RATE: u32 = 8_000;
const COUNT: usize = 64;
pub(super) const PERIOD: f64 = 60.0 / 119.999;

#[path = "input_runtime_binding_tests.rs"]
mod input_runtime_binding_tests;

#[path = "accepted_timing_refresh_tests.rs"]
mod accepted_timing_refresh_tests;

#[path = "global_playback_batch_tests.rs"]
mod global_playback_batch_tests;

#[path = "prepared_source_timing_tests.rs"]
mod prepared_source_timing_tests;

#[path = "loop_acceptance_tests.rs"]
mod loop_acceptance_tests;

pub(super) fn source() -> SampleBuffer {
    let mut samples = vec![0.0_f32; (COUNT as f64 * PERIOD * f64::from(RATE)).ceil() as usize];
    for index in 0..COUNT {
        let frame = (index as f64 * PERIOD * f64::from(RATE)).round() as usize;
        samples[frame..frame + 4].copy_from_slice(&[1.0, -0.5, 0.25, -0.125]);
    }
    SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from(samples),
    }
}

pub(super) fn test_engine() -> AudioEngine {
    Python::initialize();
    let engine = AudioEngine::new().unwrap();
    let loaded = source();
    engine.sample_cache.lock().unwrap()[0] = Some(loaded.clone());
    engine.pad_request_ids.lock().unwrap()[0] = 7;
    engine.loaded_source_generations.lock().unwrap()[0] = (7, RATE);
    engine.loaded_source_digests.lock().unwrap()[0] = Some("a".repeat(64));
    engine.timing_intents.lock().unwrap()[0] = TimingIntent::Automatic;
    engine
        .input_runtime_ownership
        .set_timing_intent(0, TimingIntent::Automatic);
    engine
        .input_runtime_ownership
        .publish_source(0, &loaded, RATE, 7);
    engine
}

/// Synthetic retained backend fixtures enter only inside tests, never through the public API.
pub(super) fn synthetic_ticket(engine: &AudioEngine) -> ConstantTimingTicket {
    let sample = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let request_id = engine.pad_request_ids.lock().unwrap()[0];
    let binding = PcmBinding::verify(
        &sample.samples,
        PcmBindingMetadata {
            job: JobIdentity {
                pad_id: 0,
                request_id,
                source_id: "loaded-0-7".into(),
                source_generation: 7,
            },
            source_sha256: "a".repeat(64),
            source_provenance: SOURCE_PROVENANCE.into(),
            pcm_sha256: f32_pcm_sha256(&sample.samples),
            sample_rate_hz: RATE,
            frame_count: sample.samples.len() as u64,
            origin_seconds: 0.0,
            mono_revision: MONO_REVISION.into(),
        },
    )
    .unwrap();
    let model = BeatThisModelIdentity {
        sha256: "b".repeat(64),
        frontend_id: "synthetic-native-fixture-no-inference".into(),
        environment_id: "independently-generated-fixture".into(),
        package_version: "1.1.0".into(),
        checkpoint: "final0".into(),
        postprocessor: "minimal".into(),
        device: "cpu".into(),
        precision: "float32".into(),
    };
    let expected = BeatThisRequestIdentity {
        job: binding.metadata().job.clone(),
        pcm_sha256: binding.metadata().pcm_sha256.clone(),
        pcm_path: "synthetic-fixture.f32le".into(),
        sample_rate_hz: RATE,
        frame_count: sample.samples.len() as u64,
        origin_seconds: 0.0,
        dtype: "float32-le".into(),
        channels: 1,
        schema_version: 1,
        model,
    };
    let raw = BeatThisRawEvidence {
        response_job: expected.job.clone(),
        response_model: expected.model.clone(),
        response_schema_version: 1,
        expected_request: expected,
        beat_seconds: (0..COUNT).map(|i| i as f64 * PERIOD).collect(),
        downbeat_seconds: vec![0.0],
        beat_logits: vec![0.0],
        downbeat_logits: vec![-0.0],
    };
    let evidence = BoundTempoEvidence::from_beat_this(
        &binding,
        raw,
        TimingBound {
            halfwidth_seconds: 0.001,
            provenance: "declared synthetic fixture error".into(),
        },
        0.0,
    )
    .unwrap();
    let mut guard = TimingAdoptionGuard::new(&binding, TimingIntent::Automatic).unwrap();
    let adoption_ticket = guard.issue_ticket().unwrap();
    ConstantTimingTicket {
        id: 0,
        request_id,
        source_generation: 7,
        sample_rate_hz: RATE,
        source_digest: "a".repeat(64),
        sample: sample.clone(),
        epoch: engine.prepared_source_epochs[0].clone(),
        captured_epoch: engine.prepared_source_epochs[0].load(Ordering::Acquire),
        binding: binding.metadata().clone(),
        evidence,
        guard: Mutex::new(guard),
        adoption_ticket,
        publication: Mutex::new(None),
        pcm_budget: PcmBudget::default(),
    }
}

pub(super) fn hypotheses() -> String {
    json!([{"id":"independent-generated-quarters", "provenance":"each independently generated fixture pulse is explicitly a quarter", "verification":"verified", "quarter_note_denominator":1, "quarter_counts":(0..COUNT).map(|i| Some(i as i64)).collect::<Vec<_>>() }]).to_string()
}

pub(super) fn origin() -> IndependentTimingOrigin {
    IndependentTimingOrigin {
        seconds: -0.125_012_3,
        provenance: "independently chosen fractional signed fixture origin".into(),
    }
}
pub(super) fn decision() -> TimingAcceptanceDecision {
    TimingAcceptanceDecision {
        policy_version: "explicit-native-fixture-acceptance-v1".into(),
        provenance: "independent generated quarter truth; no musical/default acceptance".into(),
    }
}

pub(super) fn queue(
    capacity: usize,
) -> (
    Arc<Mutex<Producer<ControlMessage>>>,
    rtrb::Consumer<ControlMessage>,
) {
    let (producer, consumer) = rtrb::RingBuffer::new(capacity);
    (Arc::new(Mutex::new(producer)), consumer)
}

pub(super) fn accept_message(mixer: &mut RtMixer, message: ControlMessage) -> bool {
    let ControlMessage::PublishConstantTiming { id, timing } = message else {
        panic!("timing publication");
    };
    mixer.publish_constant_timing_rt(id, timing, &mut ImmediateAudioBufferRetirement)
}

pub(super) fn acknowledged_mixer(engine: &AudioEngine) -> RtMixer {
    let mut mixer = RtMixer::new(1, RATE as f32);
    mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    mixer.load_sample(0, engine.sample_cache.lock().unwrap()[0].clone().unwrap());
    mixer
}

fn current_record(engine: &AudioEngine) -> Option<(String, f64, u64)> {
    Python::attach(|py| {
        current_metadata(engine, py, 0).unwrap().map(|value| {
            let dict = value.bind(py).cast::<PyDict>().unwrap();
            let get = |key| dict.get_item(key).unwrap().unwrap();
            assert_eq!(get("source_id").extract::<String>().unwrap(), "loaded-0-7");
            assert_eq!(get("source_generation").extract::<u64>().unwrap(), 7);
            assert_eq!(get("sample_rate_hz").extract::<u32>().unwrap(), RATE);
            assert_eq!(
                get("source_sha256").extract::<String>().unwrap(),
                "a".repeat(64)
            );
            assert_eq!(get("source_zero_seconds").extract::<f64>().unwrap(), 0.0);
            assert_eq!(
                get("origin_seconds").extract::<f64>().unwrap(),
                origin().seconds
            );
            assert_eq!(
                get("mono_revision").extract::<String>().unwrap(),
                MONO_REVISION
            );
            assert_eq!(get("accepted_request_id").extract::<u64>().unwrap(), 7);
            (
                get("revision").extract::<String>().unwrap(),
                get("period_seconds_per_quarter").extract::<f64>().unwrap(),
                get("publication_epoch").extract::<u64>().unwrap(),
            )
        })
    })
}

#[test]
fn current_resolver_follows_callback_revision_and_retains_old_acceptance_while_replacement_pending()
{
    let engine = test_engine();
    let first = synthetic_ticket(&engine);
    let (producer, mut consumer) = queue(2);
    let mut mixer = acknowledged_mixer(&engine);
    publish(
        &engine,
        &producer,
        &first,
        &hypotheses(),
        origin(),
        decision(),
    )
    .unwrap();
    assert!(current_record(&engine).is_none());
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    let original = current_record(&engine).unwrap();
    assert_eq!(
        original.0,
        first.guard.lock().unwrap().accepted().unwrap().revision()
    );

    // Equal-valued projections still have different full evidence/decision identity.
    let replacement = synthetic_ticket(&engine);
    let mut changed_decision = decision();
    changed_decision
        .provenance
        .push_str("; distinct explicit decision");
    producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    assert!(
        publish(
            &engine,
            &producer,
            &replacement,
            &hypotheses(),
            origin(),
            changed_decision.clone()
        )
        .is_err()
    );
    assert_eq!(replacement.publication_status().unwrap(), "captured");
    assert_eq!(current_record(&engine).unwrap(), original);
    consumer.pop().unwrap();
    consumer.pop().unwrap();
    publish(
        &engine,
        &producer,
        &replacement,
        &hypotheses(),
        origin(),
        changed_decision,
    )
    .unwrap();
    assert_eq!(replacement.publication_status().unwrap(), "pending");
    assert_eq!(current_record(&engine).unwrap(), original);
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    let current = current_record(&engine).unwrap();
    assert_ne!(current.0, original.0);
    assert_eq!(current.1, original.1);
    assert!(current.2 > original.2);
    assert_eq!(first.publication_status().unwrap(), "accepted");
    assert_eq!(engine.current_constant_timing.lock().unwrap()[0].len(), 1);

    // A new analysis request invalidates pending work, not acknowledged live timing.
    let mut requests = engine.pad_request_ids.lock().unwrap();
    PadRequestAdvance::prepare(&mut requests[0], &engine.prepared_source_epochs[0])
        .unwrap()
        .commit();
    drop(requests);
    assert_eq!(current_record(&engine).unwrap(), current);
    mixer.clear_constant_timing(0, original.2);
    assert_eq!(current_record(&engine).unwrap(), current);
    mixer.clear_constant_timing(0, current.2);
    assert!(current_record(&engine).is_none());
    Python::attach(|py| assert!(replacement.accepted_metadata(py).unwrap().is_some()));
}

#[test]
fn current_resolver_never_promotes_rejected_replacement_or_matching_stale_source_metadata() {
    for source_change in 0..6 {
        let engine = test_engine();
        let first = synthetic_ticket(&engine);
        let (producer, mut consumer) = queue(2);
        let mut mixer = acknowledged_mixer(&engine);
        publish(
            &engine,
            &producer,
            &first,
            &hypotheses(),
            origin(),
            decision(),
        )
        .unwrap();
        assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
        let original = current_record(&engine).unwrap();
        let pending = synthetic_ticket(&engine);
        publish(
            &engine,
            &producer,
            &pending,
            &hypotheses(),
            origin(),
            decision(),
        )
        .unwrap();
        // Callback rejection is observed without replacing the current accepted record.
        engine.prepared_source_epochs[0].fetch_add(1, Ordering::AcqRel);
        assert!(!accept_message(&mut mixer, consumer.pop().unwrap()));
        assert_eq!(current_record(&engine).unwrap(), original);
        match source_change {
            0 => engine.sample_cache.lock().unwrap()[0] = Some(source()),
            1 => engine.loaded_source_generations.lock().unwrap()[0].0 += 1,
            2 => engine.loaded_source_generations.lock().unwrap()[0].1 = 48_000,
            3 => engine.loaded_source_digests.lock().unwrap()[0] = Some("c".repeat(64)),
            4 => engine.sample_cache.lock().unwrap()[0] = None,
            _ => mixer.unload_sample(0),
        }
        assert!(
            current_record(&engine).is_none(),
            "source change {source_change}"
        );
        assert_eq!(first.publication_status().unwrap(), "accepted");
    }
}

#[test]
fn current_registry_cannot_pin_unloaded_pcm_and_shutdown_retires_pending_record_metadata() {
    let mut engine = test_engine();
    let first = synthetic_ticket(&engine);
    let (producer, mut consumer) = queue(2);
    let mut mixer = acknowledged_mixer(&engine);
    publish(
        &engine,
        &producer,
        &first,
        &hypotheses(),
        origin(),
        decision(),
    )
    .unwrap();
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    drop(first);
    let current_source = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    // Neither strong nor weak array owners may linger in the metadata registry:
    // Weak<[f32]> would retain the entire Arc allocation after the last strong drop.
    assert_eq!(Arc::strong_count(&current_source.samples), 3); // cache, mixer, this pin
    assert_eq!(Arc::weak_count(&current_source.samples), 0);
    drop(current_source);
    engine.sample_cache.lock().unwrap()[0] = None;
    mixer.unload_sample(0);
    assert!(current_record(&engine).is_none());

    engine.sample_cache.lock().unwrap()[0] = Some(source());
    let pending = synthetic_ticket(&engine);
    publish(
        &engine,
        &producer,
        &pending,
        &hypotheses(),
        origin(),
        decision(),
    )
    .unwrap();
    assert_eq!(engine.current_constant_timing.lock().unwrap()[0].len(), 1);
    engine.shut_down().unwrap();
    assert!(engine.current_constant_timing.lock().unwrap()[0].is_empty());
    assert_eq!(engine.current_timing_acknowledgements.current_epoch(0), 0);
}

#[test]
fn current_resolver_preserves_failed_edit_and_never_revives_manual_tap_legacy_authority() {
    for intent in [
        TimingIntent::Manual,
        TimingIntent::Tap,
        TimingIntent::Legacy,
        TimingIntent::Automatic,
    ] {
        let engine = test_engine();
        let first = synthetic_ticket(&engine);
        let (producer, mut consumer) = queue(2);
        let mut mixer = acknowledged_mixer(&engine);
        publish(
            &engine,
            &producer,
            &first,
            &hypotheses(),
            origin(),
            decision(),
        )
        .unwrap();
        assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
        let original = current_record(&engine).unwrap();
        producer
            .lock()
            .unwrap()
            .push(ControlMessage::Ping())
            .unwrap();
        producer
            .lock()
            .unwrap()
            .push(ControlMessage::Ping())
            .unwrap();
        assert!(set_intent(&engine, &producer, 0, intent).is_err());
        assert_eq!(current_record(&engine).unwrap(), original);
        consumer.pop().unwrap();
        consumer.pop().unwrap();
        set_intent(&engine, &producer, 0, intent).unwrap();
        assert!(current_record(&engine).is_none());
        set_intent(&engine, &producer, 0, TimingIntent::Automatic).unwrap();
        assert!(current_record(&engine).is_none()); // callback clears remain queued
        assert_eq!(first.publication_status().unwrap(), "accepted");
    }
    for bpm_edit in [false, true] {
        let engine = test_engine();
        let first = synthetic_ticket(&engine);
        let (producer, mut consumer) = queue(2);
        let mut mixer = acknowledged_mixer(&engine);
        publish(
            &engine,
            &producer,
            &first,
            &hypotheses(),
            origin(),
            decision(),
        )
        .unwrap();
        assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
        if bpm_edit {
            let (parameter, _) = rtrb::RingBuffer::new(1);
            publish_legacy_bpm(&engine, &Arc::new(Mutex::new(parameter)), 0, Some(119.999))
                .unwrap();
        } else {
            publish_legacy_origin(&engine, &producer, 0, origin().seconds).unwrap();
        }
        assert!(current_record(&engine).is_none());
        set_intent(&engine, &producer, 0, TimingIntent::Automatic).unwrap();
        assert!(current_record(&engine).is_none());
    }
}

#[test]
fn real_native_qm_preparation_binds_complete_actual_loaded_pcm_and_request() {
    let engine = test_engine();
    let before_epoch = engine.prepared_source_epochs[0].load(Ordering::Acquire);
    let ticket = prepare(
        &engine,
        0,
        TimingBound {
            halfwidth_seconds: 0.05,
            provenance: "explicit fixture matching bound, not musical calibration".into(),
        },
    )
    .unwrap();
    assert_eq!(ticket.request_id, 8);
    assert_eq!(ticket.captured_epoch, before_epoch + 1);
    assert_eq!(
        ticket.evidence.binding().pcm_sha256,
        f32_pcm_sha256(&ticket.sample.samples)
    );
    assert_eq!(ticket.evidence.binding().sample_rate_hz, RATE);
    assert_eq!(
        ticket.evidence.binding().frame_count,
        ticket.sample.samples.len() as u64
    );
    let BackendEvidence::Qm { raw, input } = ticket.evidence.backend() else {
        panic!("actual native QM");
    };
    assert_eq!(raw.input_sample_rate_hz(), 44_100);
    assert_eq!(
        input.frame_count,
        (ticket.sample.samples.len() as u64 * 44_100).div_ceil(u64::from(RATE))
    );
    assert!(raw.beat_frames().len() >= 24);
    assert!(matches!(
        input.transform,
        QmInputTransform::Rubato44100 { .. }
    ));
    let counts: Vec<Option<i64>> = ticket
        .evidence
        .beat_seconds()
        .iter()
        .map(|seconds| Some((seconds / PERIOD).round() as i64))
        .collect();
    let explicit = json!([{"id":"independently-generated-fixture-quarter-association","provenance":"mapping to independently generated pulse/quarter coordinates, not ordinal detector inference","verification":"verified","quarter_note_denominator":1,"quarter_counts":counts}]).to_string();
    let (producer, mut consumer) = queue(1);
    publish(&engine, &producer, &ticket, &explicit, origin(), decision()).unwrap();
    let mut mixer = RtMixer::new(1, RATE as f32);
    mixer.load_sample(0, ticket.sample.clone());
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
}

#[test]
fn actual_publication_uses_guard_acknowledgement_and_binary64_source_grid() {
    let engine = test_engine();
    let ticket = synthetic_ticket(&engine);
    let (producer, mut consumer) = queue(1);
    publish(
        &engine,
        &producer,
        &ticket,
        &hypotheses(),
        origin(),
        decision(),
    )
    .unwrap();
    assert_eq!(ticket.publication_status().unwrap(), "pending");
    Python::attach(|py| assert!(ticket.accepted_metadata(py).unwrap().is_none()));
    let mut mixer = RtMixer::new(1, RATE as f32);
    mixer.load_sample(0, ticket.sample.clone());
    mixer.set_pad_bpm(0, Some(119.999));
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    assert!(mixer.play_sample(0, 1.0));
    let guard = ticket.guard.lock().unwrap();
    let accepted = guard.accepted().unwrap();
    let expected = -origin().seconds / accepted.period_seconds_per_quarter();
    assert_eq!(mixer.active_pad_beat_position(0), Some(expected));
    let lossy_period = 60.0 / f64::from(119.999_f32);
    assert_ne!(expected, -origin().seconds / lossy_period);
    drop(guard);
    Python::attach(|py| assert!(ticket.accepted_metadata(py).unwrap().is_some()));
    assert!(
        publish(
            &engine,
            &producer,
            &ticket,
            &hypotheses(),
            origin(),
            decision()
        )
        .is_err()
    );
}

#[test]
fn current_owner_rejects_matching_stale_or_foreign_snapshots() {
    for change in 0..7 {
        let engine = test_engine();
        let ticket = synthetic_ticket(&engine);
        match change {
            0 => engine.sample_cache.lock().unwrap()[0] = Some(source()),
            1 => engine.loaded_source_digests.lock().unwrap()[0] = Some("c".repeat(64)),
            2 => engine.loaded_source_generations.lock().unwrap()[0].0 += 1,
            3 => engine.loaded_source_generations.lock().unwrap()[0].1 = 48_000,
            4 => engine.pad_request_ids.lock().unwrap()[0] += 1,
            5 => engine.sample_cache.lock().unwrap()[0] = None,
            _ => engine.timing_intents.lock().unwrap()[0] = TimingIntent::Manual,
        }
        let (producer, mut consumer) = queue(1);
        assert!(
            publish(
                &engine,
                &producer,
                &ticket,
                &hypotheses(),
                origin(),
                decision()
            )
            .is_err(),
            "change {change}"
        );
        assert!(consumer.pop().is_err());
        assert!(ticket.guard.lock().unwrap().accepted().is_none());
    }
    let first = test_engine();
    let other = test_engine();
    let ticket = synthetic_ticket(&first);
    let (producer, _) = queue(1);
    assert!(
        publish(
            &other,
            &producer,
            &ticket,
            &hypotheses(),
            origin(),
            decision()
        )
        .is_err()
    );
}

#[test]
fn full_queue_and_exhaustion_preserve_ticket_guard_epoch_and_intent() {
    let engine = test_engine();
    let ticket = synthetic_ticket(&engine);
    let (producer, mut consumer) = queue(1);
    producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    assert!(
        publish(
            &engine,
            &producer,
            &ticket,
            &hypotheses(),
            origin(),
            decision()
        )
        .is_err()
    );
    assert_eq!(ticket.publication_status().unwrap(), "captured");
    assert!(ticket.guard.lock().unwrap().accepted().is_none());
    assert_eq!(
        engine.prepared_source_epochs[0].load(Ordering::Acquire),
        ticket.captured_epoch
    );
    assert!(set_intent(&engine, &producer, 0, TimingIntent::Manual).is_err());
    assert_eq!(
        engine.timing_intents.lock().unwrap()[0],
        TimingIntent::Automatic
    );
    consumer.pop().unwrap();
    publish(
        &engine,
        &producer,
        &ticket,
        &hypotheses(),
        origin(),
        decision(),
    )
    .unwrap();

    let engine = test_engine();
    engine.prepared_source_epochs[0].store(u64::MAX, Ordering::Release);
    let ticket = synthetic_ticket(&engine);
    let (producer, mut consumer) = queue(2);
    assert!(
        publish(
            &engine,
            &producer,
            &ticket,
            &hypotheses(),
            origin(),
            decision()
        )
        .is_err()
    );
    assert!(set_intent(&engine, &producer, 0, TimingIntent::Tap).is_err());
    assert!(consumer.pop().is_err());
    assert!(ticket.guard.lock().unwrap().accepted().is_none());
    assert_eq!(
        engine.timing_intents.lock().unwrap()[0],
        TimingIntent::Automatic
    );
}

#[test]
fn explicit_intent_roundtrip_equal_legacy_edits_and_actual_request_admission_revoke() {
    for intent in [
        TimingIntent::Manual,
        TimingIntent::Tap,
        TimingIntent::Legacy,
        TimingIntent::Automatic,
    ] {
        let engine = test_engine();
        let ticket = synthetic_ticket(&engine);
        let (producer, mut consumer) = queue(2);
        set_intent(&engine, &producer, 0, intent).unwrap();
        consumer.pop().unwrap();
        set_intent(&engine, &producer, 0, TimingIntent::Automatic).unwrap();
        consumer.pop().unwrap();
        assert!(
            publish(
                &engine,
                &producer,
                &ticket,
                &hypotheses(),
                origin(),
                decision()
            )
            .is_err()
        );
    }
    for use_bpm in [false, true] {
        let engine = test_engine();
        let ticket = synthetic_ticket(&engine);
        let (producer, _) = queue(2);
        let (parameter, _) = rtrb::RingBuffer::new(1);
        if use_bpm {
            publish_legacy_bpm(&engine, &Arc::new(Mutex::new(parameter)), 0, Some(120.0)).unwrap();
        } else {
            publish_legacy_origin(&engine, &producer, 0, 0.0).unwrap();
        }
        assert_eq!(
            engine.timing_intents.lock().unwrap()[0],
            TimingIntent::Legacy
        );
        assert!(
            publish(
                &engine,
                &producer,
                &ticket,
                &hypotheses(),
                origin(),
                decision()
            )
            .is_err()
        );
    }
    let engine = test_engine();
    let ticket = synthetic_ticket(&engine);
    let job = engine
        .offline_jobs
        .begin(
            0,
            ticket.sample.clone(),
            RATE,
            7,
            super::super::analysis_jobs::OfflineRequestOwner {
                request_ids: engine.pad_request_ids.clone(),
                prepared_epoch: engine.prepared_source_epochs[0].clone(),
            },
            engine.loader_tx.clone(),
        )
        .unwrap();
    let (producer, _) = queue(1);
    assert!(
        publish(
            &engine,
            &producer,
            &ticket,
            &hypotheses(),
            origin(),
            decision()
        )
        .is_err()
    );
    job.cancel();
}

#[test]
fn callback_rejects_retired_intent_or_changed_source_and_preserves_previous_grid() {
    for change_source in [false, true] {
        let engine = test_engine();
        let ticket = synthetic_ticket(&engine);
        let (producer, mut consumer) = queue(2);
        publish(
            &engine,
            &producer,
            &ticket,
            &hypotheses(),
            origin(),
            decision(),
        )
        .unwrap();
        let mut mixer = RtMixer::new(1, RATE as f32);
        mixer.load_sample(0, ticket.sample.clone());
        mixer.set_pad_bpm(0, Some(120.0));
        if change_source {
            mixer.load_sample(0, source());
        } else {
            set_intent(&engine, &producer, 0, TimingIntent::Manual).unwrap();
        }
        assert!(!accept_message(&mut mixer, consumer.pop().unwrap()));
        assert_eq!(ticket.publication_status().unwrap(), "rejected");
        Python::attach(|py| assert!(ticket.accepted_metadata(py).unwrap().is_none()));
        assert!(mixer.play_sample(0, 1.0));
        assert_eq!(mixer.active_pad_beat_position(0), Some(0.0));
    }
}

#[test]
fn old_cross_ring_clear_or_bpm_cannot_erase_newer_precise_adoption() {
    let engine = test_engine();
    let ticket = synthetic_ticket(&engine);
    let (producer, mut consumer) = queue(1);
    publish(
        &engine,
        &producer,
        &ticket,
        &hypotheses(),
        origin(),
        decision(),
    )
    .unwrap();
    let mut mixer = RtMixer::new(1, RATE as f32);
    mixer.load_sample(0, ticket.sample.clone());
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    mixer.set_pad_bpm(0, Some(60.0)); // older parameter ring can drain after the command ring
    mixer.clear_constant_timing(0, ticket.captured_epoch);
    assert!(mixer.play_sample(0, 1.0));
    let expected = -origin().seconds
        / ticket
            .guard
            .lock()
            .unwrap()
            .accepted()
            .unwrap()
            .period_seconds_per_quarter();
    assert_eq!(mixer.active_pad_beat_position(0), Some(expected));
    mixer.clear_constant_timing(0, ticket.captured_epoch + 1);
    assert_eq!(mixer.active_pad_beat_position(0), Some(0.0));
}

#[test]
fn unsupported_or_unverified_assertions_cannot_publish_or_consume_ticket() {
    let engine = test_engine();
    let ticket = synthetic_ticket(&engine);
    let (producer, mut consumer) = queue(1);
    let unverified = hypotheses().replace("\"verified\"", "\"unverified\"");
    assert!(
        publish(
            &engine,
            &producer,
            &ticket,
            &unverified,
            origin(),
            decision()
        )
        .is_err()
    );
    assert!(publish(&engine, &producer, &ticket, "[]", origin(), decision()).is_err());
    let mut bad_origin = origin();
    bad_origin.seconds = f64::NAN;
    assert!(
        publish(
            &engine,
            &producer,
            &ticket,
            &hypotheses(),
            bad_origin,
            decision()
        )
        .is_err()
    );
    assert_eq!(ticket.publication_status().unwrap(), "captured");
    assert!(consumer.pop().is_err());
    publish(
        &engine,
        &producer,
        &ticket,
        &hypotheses(),
        origin(),
        decision(),
    )
    .unwrap();
}

#[test]
fn constant_timing_prepare_preserves_owners_on_busy_invalid_or_exhausted_admission() {
    for failure in 0..5 {
        let engine = test_engine();
        match failure {
            0 => engine.constant_timing_busy.store(true, Ordering::Release),
            1 => engine.timing_intents.lock().unwrap()[0] = TimingIntent::Tap,
            2 => engine.pad_request_ids.lock().unwrap()[0] = u64::MAX,
            3 => engine.prepared_source_epochs[0].store(u64::MAX, Ordering::Release),
            _ => engine
                .loading_sample_ids
                .lock()
                .unwrap()
                .insert(0)
                .then_some(())
                .unwrap(),
        }
        let before_request = engine.pad_request_ids.lock().unwrap()[0];
        let before_epoch = engine.prepared_source_epochs[0].load(Ordering::Acquire);
        assert!(
            prepare(
                &engine,
                0,
                TimingBound {
                    halfwidth_seconds: 0.05,
                    provenance: "explicit fixture".into()
                }
            )
            .is_err()
        );
        assert_eq!(engine.pad_request_ids.lock().unwrap()[0], before_request);
        assert_eq!(
            engine.prepared_source_epochs[0].load(Ordering::Acquire),
            before_epoch
        );
        assert_eq!(
            engine.constant_timing_busy.load(Ordering::Acquire),
            failure == 0
        );
    }
}

#[test]
fn constant_timing_prepare_rechecks_real_manual_intent_while_native_qm_is_running() {
    let engine = Arc::new(test_engine());
    let worker_engine = engine.clone();
    let worker = std::thread::spawn(move || {
        prepare(
            &worker_engine,
            0,
            TimingBound {
                halfwidth_seconds: 0.05,
                provenance: "explicit fixture".into(),
            },
        )
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while engine.pad_request_ids.lock().unwrap()[0] == 7 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(engine.pad_request_ids.lock().unwrap()[0], 8);
    let (producer, _) = queue(1);
    set_intent(&engine, &producer, 0, TimingIntent::Manual).unwrap();
    assert!(worker.join().unwrap().is_err());
    assert!(!engine.constant_timing_busy.load(Ordering::Acquire));
    assert_eq!(
        engine.timing_intents.lock().unwrap()[0],
        TimingIntent::Manual
    );
}

#[test]
fn constant_timing_normal_analysis_admission_rejects_pending_and_queued_and_checks_exhaustion() {
    for queued in [false, true] {
        let engine = test_engine();
        let ticket = synthetic_ticket(&engine);
        let (producer, mut consumer) = queue(1);
        if queued {
            publish(
                &engine,
                &producer,
                &ticket,
                &hypotheses(),
                origin(),
                decision(),
            )
            .unwrap();
        }
        let (reader, request, rate) = super::super::admit_sample_analysis(&engine, 0).unwrap();
        let sample = reader
            .materialize(super::super::cold_jobs::PCM_LIMIT_BYTES, &|| false)
            .unwrap();
        assert_eq!(request, ticket.request_id + 1);
        assert_eq!(rate, RATE);
        assert!(Arc::ptr_eq(&sample.samples, &ticket.sample.samples));
        if queued {
            let mut mixer = RtMixer::new(1, RATE as f32);
            mixer.load_sample(0, sample);
            assert!(!accept_message(&mut mixer, consumer.pop().unwrap()));
            assert_eq!(ticket.publication_status().unwrap(), "rejected");
        } else {
            assert!(
                publish(
                    &engine,
                    &producer,
                    &ticket,
                    &hypotheses(),
                    origin(),
                    decision()
                )
                .is_err()
            );
        }
        assert!(
            engine
                .active_tasks
                .lock()
                .unwrap()
                .contains(&(0, crate::messages::BackgroundTaskKind::Analysis))
        );
    }
    for exhaust_request in [false, true] {
        let engine = test_engine();
        if exhaust_request {
            engine.pad_request_ids.lock().unwrap()[0] = u64::MAX;
        } else {
            engine.prepared_source_epochs[0].store(u64::MAX, Ordering::Release);
        }
        let request = engine.pad_request_ids.lock().unwrap()[0];
        let epoch = engine.prepared_source_epochs[0].load(Ordering::Acquire);
        assert!(super::super::admit_sample_analysis(&engine, 0).is_err());
        assert!(engine.active_tasks.lock().unwrap().is_empty());
        assert_eq!(engine.pad_request_ids.lock().unwrap()[0], request);
        assert_eq!(
            engine.prepared_source_epochs[0].load(Ordering::Acquire),
            epoch
        );
    }
}

#[test]
fn constant_timing_loader_delivery_strips_retired_timing_preserves_source_bookkeeping_and_skips_stale_progress()
 {
    let engine = test_engine();
    let analysis = crate::messages::SampleAnalysis {
        bpm: 120.0,
        key: "Am".into(),
        beat_grid: flitzis_looper_analysis::BeatGrid {
            beats: vec![0.0, 0.5],
            downbeats: vec![0.0],
            bars: vec![0.0],
        },
    };
    engine
        .loader_tx
        .send(crate::messages::LoaderEvent::Progress {
            id: 0,
            request_id: 7,
            percent: 0.5,
            stage: "old".into(),
        })
        .unwrap();
    engine
        .loader_tx
        .send(crate::messages::LoaderEvent::OfflineAnalysisCompleted {
            id: 0,
            request_id: 7,
            result_json: "{}".into(),
        })
        .unwrap();
    engine
        .loader_tx
        .send(crate::messages::LoaderEvent::Success {
            original_lease: None,
            timing_epoch: None,
            id: 0,
            request_id: 7,
            duration_s: 32.0,
            detected_loop_start_s: Some(0.125),
            cached_path: "samples/current.wav".into(),
            analysis: Some(analysis.clone()),
        })
        .unwrap();
    engine
        .loader_tx
        .send(crate::messages::LoaderEvent::TaskSuccess {
            id: 0,
            request_id: 7,
            task: crate::messages::BackgroundTaskKind::Analysis,
            analysis: Some(analysis),
        })
        .unwrap();
    super::super::next_pad_request_id(
        &engine.pad_request_ids,
        0,
        &engine.prepared_source_epochs[0],
    )
    .unwrap();
    Python::attach(|py| {
        let event = engine.poll_loader_events(py).unwrap().unwrap();
        let event = event.bind(py).cast::<PyDict>().unwrap();
        assert_eq!(
            event
                .get_item("type")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "success"
        );
        assert_eq!(
            event
                .get_item("cached_path")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "samples/current.wav"
        );
        assert!(
            event
                .get_item("timing_stale")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(event.get_item("analysis").unwrap().is_none());
        let task = engine.poll_loader_events(py).unwrap().unwrap();
        let task = task.bind(py).cast::<PyDict>().unwrap();
        assert_eq!(
            task.get_item("type")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "task_success"
        );
        assert!(
            task.get_item("timing_stale")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(task.get_item("analysis").unwrap().is_none());
        assert!(engine.poll_loader_events(py).unwrap().is_none());
    });
}

#[test]
fn constant_timing_loader_delivery_discards_old_success_after_source_clear_and_continues_drain() {
    for replacement_loading in [false, true] {
        let engine = test_engine();
        engine
            .loader_tx
            .send(crate::messages::LoaderEvent::Success {
                original_lease: None,
                timing_epoch: None,
                id: 0,
                request_id: 7,
                duration_s: 32.0,
                detected_loop_start_s: Some(0.125),
                cached_path: "samples/retired.wav".into(),
                analysis: None,
            })
            .unwrap();
        {
            // Unload and replacement admission clear the current source under this owner,
            // while its last generation remains until another source is published.
            let mut requests = engine.pad_request_ids.lock().unwrap();
            let advance =
                PadRequestAdvance::prepare(&mut requests[0], &engine.prepared_source_epochs[0])
                    .unwrap();
            engine.sample_cache.lock().unwrap()[0] = None;
            engine.loaded_source_digests.lock().unwrap()[0] = None;
            advance.commit();
        }
        if replacement_loading {
            engine.loading_sample_ids.lock().unwrap().insert(0);
        }
        engine
            .loader_tx
            .send(crate::messages::LoaderEvent::Started {
                id: 0,
                request_id: 8,
            })
            .unwrap();
        Python::attach(|py| {
            let event = engine.poll_loader_events(py).unwrap().unwrap();
            let event = event.bind(py).cast::<PyDict>().unwrap();
            assert_eq!(
                event
                    .get_item("type")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "started"
            );
            assert!(event.get_item("timing_stale").unwrap().is_none());
            assert!(engine.poll_loader_events(py).unwrap().is_none());
        });
    }
}
