//! Finished QM evidence survives placement contention without gaining publication authority.

use super::*;
use crate::audio_engine::audio_stream::{create_stream_mixer, process_control_message};
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::pad_pair;
use crate::audio_engine::resident_relocation::control_publication_tests::PendingWriter;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, TriggerQuantization};
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

thread_local! {
    static BEFORE_PUBLICATION_CLAIM: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

pub(super) fn before_publication_claim() {
    let hook = BEFORE_PUBLICATION_CLAIM.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn with_before_publication_claim<T>(
    hook: impl FnOnce() + 'static,
    action: impl FnOnce() -> T,
) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            BEFORE_PUBLICATION_CLAIM.with(|slot| *slot.borrow_mut() = None);
        }
    }
    BEFORE_PUBLICATION_CLAIM.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    let _reset = Reset;
    action()
}

fn prepared_qm(engine: &AudioEngine) -> PreparedTimingEvidence {
    ConstantTimingPreparationWork::capture(
        engine,
        0,
        TimingBound {
            halfwidth_seconds: 0.05,
            provenance: "explicit generated-pulse fixture bound, not musical calibration".into(),
        },
    )
    .unwrap()
    .prepare()
    .unwrap()
}

fn assessment(ticket: &ConstantTimingTicket) -> AcceptedConstantTiming {
    let counts: Vec<_> = ticket
        .evidence
        .beat_seconds()
        .iter()
        .map(|seconds| Some((seconds / super::tests::PERIOD).round() as i64))
        .collect();
    let hypotheses = json!([{
        "id": "independently-generated-fixture-quarter-association",
        "provenance": "original QM positions mapped to independently generated pulse quarters",
        "verification": "verified",
        "quarter_note_denominator": 1,
        "quarter_counts": counts,
    }])
    .to_string();
    prepare_publication(
        ticket,
        &hypotheses,
        super::tests::origin(),
        super::tests::decision(),
    )
    .unwrap()
}

fn mixer(engine: &AudioEngine) -> Box<RtMixer> {
    let mut mixer = create_stream_mixer(
        1,
        super::tests::RATE,
        engine.current_timing_acknowledgements.clone(),
        engine.input_runtime_ownership.clone(),
        engine.prepared_source_epochs.clone(),
    )
    .unwrap();
    for (id, sample) in engine.sample_cache.lock().unwrap().iter().enumerate() {
        if let Some(sample) = sample {
            mixer.load_sample(id, sample.clone());
        }
    }
    mixer
}

fn refill_same_source(engine: &AudioEngine, source: &SampleBuffer) {
    let ownership = &engine.input_runtime_ownership;
    let placement = ownership.slot_snapshot(0).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(
            placement,
            crate::audio_engine::content_lifetime::reserve().unwrap(),
        )
        .unwrap();
    ownership
        .publish_source_write(
            &write,
            source,
            super::tests::RATE,
            7,
            ownership.next_authority(0).unwrap(),
            Some(TimingIntent::Automatic),
            false,
            || true,
        )
        .unwrap();
}

fn assert_unpublished(engine: &AudioEngine, ticket: &ConstantTimingTicket, acknowledged: u64) {
    assert_eq!(ticket.publication_status().unwrap(), "captured");
    assert!(ticket.guard.lock().unwrap().accepted().is_none());
    assert_eq!(ticket.epoch.load(Ordering::Acquire), ticket.captured_epoch);
    assert!(
        engine
            .current_constant_timing
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .all(|record| {
                record.content_key != ticket.placement.unwrap().content_key()
                    || record.binding.job.request_id != ticket.request_id
            })
    );
    assert_eq!(
        engine.current_timing_acknowledgements.current_for_key(
            &ticket.ownership,
            ticket.placement.unwrap().content_key().unwrap(),
        ),
        acknowledged,
    );
}

#[test]
fn completed_qm_during_unrelated_pending_returns_original_evidence_without_early_publication() {
    let engine = super::tests::test_engine();
    let preparations = Rc::new(Cell::new(0));
    let observed = preparations.clone();
    struct ResetObserver;
    impl Drop for ResetObserver {
        fn drop(&mut self) {
            crate::audio_engine::native_owned_work::set_observer(None);
        }
    }
    let _reset = ResetObserver;
    crate::audio_engine::native_owned_work::set_observer(Some(Box::new(move |point| {
        if point == crate::audio_engine::native_owned_work::WorkPoint::TimingPcm {
            observed.set(observed.get() + 1);
        }
    })));
    let prepared = prepared_qm(&engine);
    crate::audio_engine::native_owned_work::set_observer(None);
    assert_eq!(preparations.get(), 1);
    let source = prepared.ticket.sample.clone();
    let epoch = prepared.ticket.epoch.clone();
    let key = prepared.ticket.placement.unwrap().content_key().unwrap();
    let request = prepared.ticket.request_incarnation;
    let raw_revision = prepared
        .ticket
        .evidence
        .source_identity()
        .raw_revision
        .clone();
    let beats = prepared.ticket.evidence.beat_seconds().to_vec();
    let held = PendingWriter::hold(engine.input_runtime_ownership.clone());
    assert!(engine._content_placement_pending());
    let ticket = prepared.finish(&engine).unwrap();
    assert!(ticket.placement_pending());
    assert_eq!(ticket.current_control_slot(), None);
    assert!(ticket.sample.same_source(&source));
    assert!(Arc::ptr_eq(&ticket.epoch, &epoch));
    assert_eq!(ticket.placement.unwrap().content_key(), Some(key));
    assert_eq!(ticket.request_incarnation, request);
    assert_eq!(ticket.evidence.source_identity().raw_revision, raw_revision);
    assert_eq!(ticket.evidence.beat_seconds(), beats);
    assert_eq!(preparations.get(), 1);
    assert!(!engine.constant_timing_busy.load(Ordering::Acquire));
    assert_unpublished(&engine, &ticket, 0);
    let timing = assessment(&ticket);
    let (producer, mut consumer) = super::tests::queue(1);
    let engine = super::tests::with_control_producer(engine, &producer);
    let error = publish_record(&engine, &producer, &ticket, timing.clone(), false).unwrap_err();
    Python::attach(|py| assert!(error.is_instance_of::<PyBlockingIOError>(py)));
    assert_unpublished(&engine, &ticket, 0);
    assert!(consumer.pop().is_err());
    held.complete();
    assert!(!engine._content_placement_pending());
    assert!(!ticket.placement_pending());
    publish_record(&engine, &producer, &ticket, timing, false).unwrap();
    let mut mixer = mixer(&engine);
    assert!(super::tests::accept_message(
        &mut mixer,
        consumer.pop().unwrap()
    ));
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    assert!(consumer.pop().is_err());
    let binding = engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap();
    let refresh = crate::audio_engine::accepted_timing_refresh::enqueue(
        &engine,
        &producer,
        &binding,
        0.0,
        Some(1.0),
        None,
    )
    .unwrap();
    let held = PendingWriter::hold(engine.input_runtime_ownership.clone());
    assert!(refresh.placement_pending());
    assert_eq!(refresh.publication_status(), "pending");
    held.complete();
    assert!(!refresh.placement_pending());
    // Original lifetime retirement never becomes a frontend Pending wait,
    // even when another writer is currently holding the shared marker.
    refill_same_source(&engine, &source);
    let held = PendingWriter::hold(engine.input_runtime_ownership.clone());
    assert!(!ticket.placement_pending());
    assert!(!refresh.placement_pending());
    held.complete();
}

#[test]
fn pending_after_completed_assessment_before_claim_does_not_consume_original_adoption() {
    let engine = super::tests::test_engine();
    let ticket = prepared_qm(&engine).finish(&engine).unwrap();
    let timing = assessment(&ticket);
    let revision = timing.revision().to_owned();
    let held = Rc::new(RefCell::new(None));
    let pending = held.clone();
    let ownership = engine.input_runtime_ownership.clone();
    let (producer, mut consumer) = super::tests::queue(1);
    let engine = super::tests::with_control_producer(engine, &producer);
    let error = with_before_publication_claim(
        move || *pending.borrow_mut() = Some(PendingWriter::hold(ownership)),
        || publish_record(&engine, &producer, &ticket, timing.clone(), false),
    )
    .unwrap_err();
    Python::attach(|py| assert!(error.is_instance_of::<PyBlockingIOError>(py)));
    assert_unpublished(&engine, &ticket, 0);
    assert!(consumer.pop().is_err());
    held.borrow_mut().take().unwrap().complete();
    publish_record(&engine, &producer, &ticket, timing.clone(), false).unwrap();
    assert_eq!(
        ticket.guard.lock().unwrap().accepted().unwrap().revision(),
        revision
    );
    assert_eq!(ticket.publication_status().unwrap(), "pending");
    assert!(publish_record(&engine, &producer, &ticket, timing, false).is_err());
    let mut mixer = mixer(&engine);
    assert!(super::tests::accept_message(
        &mut mixer,
        consumer.pop().unwrap()
    ));
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    assert!(consumer.pop().is_err());
}

#[test]
fn finished_original_qm_crosses_actual_move_and_swap_mirror_with_same_ticket() {
    for occupied in [false, true] {
        let mut engine = super::tests::test_engine();
        if occupied {
            let mut other = super::tests::source();
            other.samples = Arc::from(
                other
                    .samples
                    .iter()
                    .map(|sample| -*sample)
                    .collect::<Vec<_>>(),
            );
            engine.sample_cache.lock().unwrap()[36] = Some(other.clone());
            engine.pad_request_ids.lock().unwrap()[36] = 19;
            engine.loaded_source_generations.lock().unwrap()[36] = (19, super::tests::RATE);
            engine.loaded_source_digests.lock().unwrap()[36] = Some("b".repeat(64));
            engine.timing_intents.lock().unwrap()[36] = TimingIntent::Manual;
            engine
                .input_runtime_ownership
                .publish_test_source(36, &other, super::tests::RATE, 19);
            engine
                .input_runtime_ownership
                .set_timing_intent(36, TimingIntent::Manual);
        }
        let (producer, mut consumer) = super::tests::queue(4);
        let (_, feedback) = rtrb::RingBuffer::<AudioMessage>::new(8);
        engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(feedback))));
        let mut mixer = mixer(&engine);
        let baseline = super::tests::synthetic_ticket(&engine);
        publish(
            &engine,
            &producer,
            &baseline,
            &super::tests::hypotheses(),
            super::tests::origin(),
            super::tests::decision(),
        )
        .unwrap();
        assert!(super::tests::accept_message(
            &mut mixer,
            consumer.pop().unwrap()
        ));
        let acknowledged = engine
            .current_timing_acknowledgements
            .current_for_slot(&engine.input_runtime_ownership, 0);
        assert_ne!(acknowledged, 0);
        let prepared = prepared_qm(&engine);
        let original_key = prepared.ticket.placement.unwrap().content_key().unwrap();
        let epoch = prepared.ticket.epoch.clone();
        let request = prepared.ticket.request_incarnation;
        let raw_revision = prepared
            .ticket
            .evidence
            .source_identity()
            .raw_revision
            .clone();
        let pair_capture = pad_pair::capture(&engine, 0, 36).unwrap();
        let pair = pad_pair::prepare(&engine, &pair_capture, super::tests::RATE, 1).unwrap();
        pad_pair::publish(&engine, &pair).unwrap();
        process_control_message(
            consumer.pop().unwrap(),
            &mut FixedCapacityScheduler::<8>::new(),
            0,
            &mut TriggerQuantization::Immediate,
            &mut TransportTimeline::new(super::tests::RATE),
            &mut mixer,
            &mut Vec::new(),
            &mut ImmediateAudioBufferRetirement,
        );
        assert!(pair.native_acknowledgement().is_some());
        assert!(matches!(
            engine
                .input_runtime_ownership
                .observe_control_key(original_key),
            PlacementObservation::Pending
        ));
        let ticket = prepared.finish(&engine).unwrap();
        assert!(ticket.placement_pending());
        assert_unpublished(&engine, &ticket, acknowledged);
        let timing = assessment(&ticket);
        let error = publish_record(&engine, &producer, &ticket, timing.clone(), false).unwrap_err();
        Python::attach(|py| assert!(error.is_instance_of::<PyBlockingIOError>(py)));
        assert!(consumer.pop().is_err());
        assert!(pad_pair::reconcile(&engine, &pair).unwrap());
        assert!(!ticket.placement_pending());
        assert_eq!(ticket.current_control_slot(), Some(36));
        assert_eq!(ticket.placement.unwrap().content_key(), Some(original_key));
        assert_eq!(ticket.request_incarnation, request);
        assert!(Arc::ptr_eq(&ticket.epoch, &epoch));
        assert_eq!(ticket.evidence.source_identity().raw_revision, raw_revision);
        publish_record(&engine, &producer, &ticket, timing, false).unwrap();
        let message = consumer.pop().unwrap();
        assert!(matches!(
            &message,
            ControlMessage::PublishConstantTiming { id: 36, .. }
        ));
        assert!(super::tests::accept_message(&mut mixer, message));
        assert_eq!(ticket.publication_status().unwrap(), "accepted");
        assert_eq!(
            engine
                .current_timing_acknowledgements
                .current_for_slot(&engine.input_runtime_ownership, 0),
            0
        );
        assert_ne!(
            engine
                .current_timing_acknowledgements
                .current_for_slot(&engine.input_runtime_ownership, 36),
            0
        );
        assert!(consumer.pop().is_err());
    }
}

#[test]
fn completed_qm_rejects_retired_request_source_epoch_and_intent_even_while_pending() {
    for case in 0..8 {
        let mut engine = super::tests::test_engine();
        let prepared = prepared_qm(&engine);
        let placement = prepared.ticket.placement.unwrap();
        match case {
            0 => engine
                .input_runtime_ownership
                .prepare_source_request_advance(0)
                .unwrap()
                .commit(),
            1 => {
                let same_pcm = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
                refill_same_source(&engine, &same_pcm);
            }
            2 => prepared
                .ticket
                .epoch
                .store(prepared.ticket.captured_epoch + 1, Ordering::Release),
            3 => {
                engine.prepared_source_epochs[placement.cell().index()] =
                    Arc::new(AtomicU64::new(prepared.ticket.captured_epoch))
            }
            4 => engine
                .input_runtime_ownership
                .set_timing_intent_for_cell(placement.cell(), TimingIntent::Manual),
            5 => engine.sample_cache.lock().unwrap()[0] = Some(super::tests::source()),
            6 => engine.loaded_source_generations.lock().unwrap()[0].0 += 1,
            _ => engine.loaded_source_digests.lock().unwrap()[0] = Some("b".repeat(64)),
        }
        let held = PendingWriter::hold(engine.input_runtime_ownership.clone());
        let error = prepared
            .finish(&engine)
            .err()
            .expect("stale finished evidence must reject");
        assert_ne!(
            error, "constant timing content placement pending",
            "case {case}"
        );
        assert!(!engine.constant_timing_busy.load(Ordering::Acquire));
        assert!(
            engine
                .current_constant_timing
                .lock()
                .unwrap()
                .iter()
                .all(Vec::is_empty)
        );
        held.complete();
    }
}

#[test]
fn stale_dispatched_publication_is_value_error_during_pending_and_never_retry_authority() {
    let engine = super::tests::test_engine();
    let ticket = prepared_qm(&engine).finish(&engine).unwrap();
    let timing = assessment(&ticket);
    engine
        .input_runtime_ownership
        .prepare_source_request_advance(0)
        .unwrap()
        .commit();
    let held = PendingWriter::hold(engine.input_runtime_ownership.clone());
    let (producer, mut consumer) = super::tests::queue(1);
    let engine = super::tests::with_control_producer(engine, &producer);
    let error = publish_record(&engine, &producer, &ticket, timing, false).unwrap_err();
    Python::attach(|py| {
        assert!(error.is_instance_of::<PyValueError>(py));
        assert!(!error.is_instance_of::<PyBlockingIOError>(py));
    });
    assert_unpublished(&engine, &ticket, 0);
    assert!(consumer.pop().is_err());
    held.complete();
}

#[test]
fn completed_qm_cannot_finish_on_a_foreign_engine_with_pending_placement() {
    let engine = super::tests::test_engine();
    let prepared = prepared_qm(&engine);
    let foreign = super::tests::test_engine();
    let held = PendingWriter::hold(foreign.input_runtime_ownership.clone());
    assert!(prepared.finish(&foreign).is_err());
    assert!(!engine.constant_timing_busy.load(Ordering::Acquire));
    assert!(
        foreign
            .current_constant_timing
            .lock()
            .unwrap()
            .iter()
            .all(Vec::is_empty)
    );
    held.complete();
}

#[test]
fn engine_read_token_ffi_observes_completed_rejected_marker_after_boolean_pending_closes() {
    let engine = super::tests::test_engine();
    Python::attach(|py| {
        let engine = Py::new(py, engine).unwrap();
        let bound = engine.bind(py);
        let ownership = bound.borrow().input_runtime_ownership.clone();
        let read = || {
            bound
                .call_method0("_content_placement_read_token")
                .unwrap()
                .extract::<Option<u64>>()
                .unwrap()
        };
        let token = read().unwrap();
        assert_eq!(read(), Some(token));
        assert_eq!(ownership.placement_read_token(), Some(token));
        let original = ownership.slot_snapshot(0).unwrap();
        let other = ownership.slot_snapshot(215).unwrap();
        let write = ownership
            .prepare_slot_write_fixed(other, other.lifetime())
            .unwrap();
        assert_eq!(
            ownership.commit_slot_write_fixed(
                &write,
                || {
                    assert_eq!(read(), None);
                    assert!(
                        bound
                            .call_method0("_content_placement_pending")
                            .unwrap()
                            .extract::<bool>()
                            .unwrap()
                    );
                    false
                },
                |_| panic!("rejected unrelated marker must not apply"),
            ),
            Err(PlacementError::ClaimRejected)
        );
        assert_eq!(ownership.slot_snapshot(0).unwrap(), original);
        assert!(
            !bound
                .call_method0("_content_placement_pending")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        let crossed = read().unwrap();
        assert_ne!(crossed, token);
        assert_eq!(read(), Some(crossed));
    });
}
