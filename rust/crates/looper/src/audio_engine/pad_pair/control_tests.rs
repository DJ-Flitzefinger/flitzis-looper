use super::*;
use crate::audio_engine::buffer_retirement::AudioBufferRetirement;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, PreparedStemSet, TriggerQuantization};
use rtrb::RingBuffer;
use std::sync::atomic::AtomicBool;

fn fixture(capacity: usize) -> LaunchFixture {
    let mut fixture = LaunchFixture::new(capacity);
    let (_, feedback) = RingBuffer::<AudioMessage>::new(8);
    fixture.engine.test_control_channels =
        Some((fixture.producer.clone(), Arc::new(Mutex::new(feedback))));
    fixture
}

struct PairRetirement {
    received: Option<Box<PadPairRtWork>>,
}
impl AudioBufferRetirement for PairRetirement {
    fn retire_pad_pair(&mut self, work: Box<PadPairRtWork>) {
        // The work retains the complete guard and placement. No temporary callback Arc may
        // become their final owner if the actual worker drains the work immediately.
        assert_eq!(Arc::strong_count(&work.guard), 1);
        assert_eq!(Arc::strong_count(&work.prepared), 1);
        assert!(
            work.guard
                .participants
                .iter()
                .all(|participant| participant.selection_fences.len() == 10)
        );
        self.received = Some(work);
    }
    fn retire_resident_capture(
        &mut self,
        _: Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
    }
    fn retire_resident_cancellation(&mut self, _: Arc<AtomicBool>) {}
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
    fn retire_cold_adoption(&mut self, _: Arc<AtomicU8>) {}
    fn retire_sample(&mut self, _: SampleBuffer) {}
    fn retire_prepared_stems(&mut self, _: PreparedStemSet) {}
    fn retire_constant_timing(
        &mut self,
        _: crate::audio_engine::constant_timing::PreparedConstantTiming,
    ) {
    }
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
    }
    fn available_retirement_slots(&mut self) -> usize {
        usize::MAX
    }
}

#[test]
fn actual_internal_capture_prepare_publish_ack_retains_full_native_custody_and_no_callback_pins() {
    let mut fixture = fixture(4);
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let captured = capture(&fixture.engine, 0, 1).unwrap();
    let ticket = prepare(&fixture.engine, &captured, 48_000, 1).unwrap();
    assert!(ticket.native_acknowledgement().is_none());
    publish(&fixture.engine, &ticket).unwrap();
    let message = fixture.consumer.pop().unwrap();
    assert!(matches!(&message, ControlMessage::PadPair(_)));
    let mut retirement = PairRetirement { received: None };
    crate::audio_engine::audio_stream::process_control_message(
        message,
        &mut FixedCapacityScheduler::<8>::new(),
        0,
        &mut TriggerQuantization::Immediate,
        &mut TransportTimeline::new(48_000),
        &mut mixer,
        &mut Vec::<AudioMessage>::new(),
        &mut retirement,
    );
    let receipt = ticket.native_acknowledgement().unwrap();
    assert_eq!(
        receipt.source_after.lifetime,
        captured.state.snapshots()[1].lifetime()
    );
    assert_eq!(
        receipt.target_after.lifetime,
        captured.state.snapshots()[0].lifetime()
    );
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    assert!(!cancel(&fixture.engine, &captured).unwrap());
    assert!(fixture.engine.input_runtime_ownership.pair_reservation(0) != 0);
    let envelope: serde_json::Value =
        serde_json::from_str(&ticket.snapshot_envelope_json()).unwrap();
    assert_eq!(envelope["own_native_ack"], true);
    assert_eq!(envelope["control_mirrored"], false);
    assert_eq!(envelope["durable_finished"], false);
    assert!(retirement.received.is_some());
}

#[test]
fn internal_publication_pressure_retains_same_prepared_work_and_cancel_releases_both() {
    let mut fixture = fixture(1);
    let captured = capture(&fixture.engine, 0, 1).unwrap();
    let ticket = prepare(&fixture.engine, &captured, 48_000, 1).unwrap();
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    assert!(publish(&fixture.engine, &ticket).is_err());
    assert_eq!(ticket.state.stage.load(Ordering::Acquire), 2);
    assert!(ticket.state.work.lock().unwrap().is_some());
    assert!(ticket.native_acknowledgement().is_none());
    assert!(cancel(&fixture.engine, &captured).unwrap());
    assert_eq!(
        fixture.engine.input_runtime_ownership.pair_reservation(0),
        0
    );
    assert_eq!(
        fixture.engine.input_runtime_ownership.pair_reservation(1),
        0
    );
    assert!(matches!(
        fixture.consumer.pop().unwrap(),
        ControlMessage::Ping()
    ));
    assert!(publish(&fixture.engine, &ticket).is_err());
    assert!(fixture.consumer.is_empty());
}

fn actual_pair_ack(
    fixture: &mut LaunchFixture,
    mixer: &mut crate::audio_engine::mixer::RtMixer,
) -> PadPairTicket {
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let captured = fixture.engine._capture_pad_pair(0, 1).unwrap();
    let ticket = prepare(&fixture.engine, &captured, 48_000, 1).unwrap();
    fixture.engine._publish_pad_pair(&ticket).unwrap();
    crate::audio_engine::audio_stream::process_control_message(
        fixture.consumer.pop().unwrap(),
        &mut FixedCapacityScheduler::<8>::new(),
        0,
        &mut TriggerQuantization::Immediate,
        &mut TransportTimeline::new(48_000),
        mixer,
        &mut Vec::<AudioMessage>::new(),
        &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement,
    );
    assert!(ticket.own_acknowledgement().is_some());
    ticket
}

#[test]
fn exact_opaque_ack_finish_requires_complete_mirror_and_releases_only_original_reservation() {
    let mut fixture = fixture(4);
    let mut mixer = fixture.mixer();
    let ticket = actual_pair_ack(&mut fixture, &mut mixer);
    let ack = ticket.own_acknowledgement().unwrap();
    let reservation = ticket.state.guard.reservation;
    assert!(
        !fixture
            .engine
            ._validate_pad_pair_acknowledgement(&ticket, &ack)
            .unwrap()
    );
    assert!(fixture.engine._finish_pad_pair(&ticket, &ack).is_err());
    for slot in [0, 1] {
        assert_eq!(
            fixture
                .engine
                .input_runtime_ownership
                .pair_reservation(slot),
            reservation
        );
    }
    assert!(fixture.engine._reconcile_pad_pair(&ticket).unwrap());
    assert!(
        fixture
            .engine
            ._validate_pad_pair_acknowledgement(&ticket, &ack)
            .unwrap()
    );
    let foreign_fixture = self::fixture(4);
    let mut foreign_mixer = foreign_fixture.mixer();
    let mut foreign_fixture = foreign_fixture;
    let foreign = actual_pair_ack(&mut foreign_fixture, &mut foreign_mixer);
    let foreign_ack = foreign.own_acknowledgement().unwrap();
    assert!(
        !fixture
            .engine
            ._validate_pad_pair_acknowledgement(&ticket, &foreign_ack)
            .unwrap()
    );
    assert!(
        fixture
            .engine
            ._finish_pad_pair(&ticket, &foreign_ack)
            .is_err()
    );
    assert!(
        fixture
            .engine
            ._finish_pad_pair(&foreign, &foreign_ack)
            .is_err()
    );
    assert!(
        foreign_fixture
            .engine
            ._finish_pad_pair(&ticket, &ack)
            .is_err()
    );
    let substituted = PadPairAcknowledgement {
        state: ticket.state.clone(),
        observed: super::super::PairAcknowledgementSnapshot {
            layout_revision: ack.observed.layout_revision + 2,
            ..ack.observed
        },
    };
    assert!(
        !fixture
            .engine
            ._validate_pad_pair_acknowledgement(&ticket, &substituted)
            .unwrap()
    );
    assert!(
        fixture
            .engine
            ._finish_pad_pair(&ticket, &substituted)
            .is_err()
    );
    for slot in [0, 1] {
        assert_eq!(
            fixture
                .engine
                .input_runtime_ownership
                .pair_reservation(slot),
            reservation
        );
    }
    fixture.engine._finish_pad_pair(&ticket, &ack).unwrap();
    fixture.engine._finish_pad_pair(&ticket, &ack).unwrap();
    for slot in [0, 1] {
        assert_eq!(
            fixture
                .engine
                .input_runtime_ownership
                .pair_reservation(slot),
            0
        );
    }
    assert!(fixture.consumer.is_empty());
    assert!(
        foreign_fixture
            .engine
            ._reconcile_pad_pair(&foreign)
            .unwrap()
    );
    foreign_fixture
        .engine
        ._finish_pad_pair(&foreign, &foreign_ack)
        .unwrap();
}

#[test]
fn finished_own_receipt_is_idempotent_after_later_pair_without_releasing_successor() {
    let mut fixture = fixture(4);
    let mut mixer = fixture.mixer();
    let first = actual_pair_ack(&mut fixture, &mut mixer);
    let first_ack = first.own_acknowledgement().unwrap();
    assert!(fixture.engine._reconcile_pad_pair(&first).unwrap());
    fixture.engine._finish_pad_pair(&first, &first_ack).unwrap();
    let first_after = [
        fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap(),
        fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(1)
            .unwrap(),
    ];
    let second = actual_pair_ack(&mut fixture, &mut mixer);
    let second_ack = second.own_acknowledgement().unwrap();
    assert!(fixture.engine._reconcile_pad_pair(&second).unwrap());
    let before_retry = [
        fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap(),
        fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(1)
            .unwrap(),
    ];
    assert!(
        first_after
            .iter()
            .zip(before_retry)
            .all(|(before, after)| before.content_key() != after.content_key())
    );
    assert!(
        !fixture
            .engine
            ._validate_pad_pair_acknowledgement(&first, &first_ack)
            .unwrap()
    );
    fixture.engine._finish_pad_pair(&first, &first_ack).unwrap();
    assert!(
        fixture
            .engine
            ._finish_pad_pair(&first, &second_ack)
            .is_err()
    );
    assert!(
        fixture
            .engine
            ._finish_pad_pair(&second, &first_ack)
            .is_err()
    );
    for (slot, before) in before_retry.into_iter().enumerate() {
        assert_eq!(
            fixture
                .engine
                .input_runtime_ownership
                .pair_reservation(slot),
            second.state.guard.reservation
        );
        assert!(
            fixture
                .engine
                .input_runtime_ownership
                .slot_snapshot(slot)
                .unwrap()
                .same_projection(before)
        );
    }
    assert!(
        fixture
            .engine
            ._validate_pad_pair_acknowledgement(&second, &second_ack)
            .unwrap()
    );
    fixture
        .engine
        ._finish_pad_pair(&second, &second_ack)
        .unwrap();
    assert_eq!(
        fixture.engine.input_runtime_ownership.pair_reservation(0),
        0
    );
    assert_eq!(
        fixture.engine.input_runtime_ownership.pair_reservation(1),
        0
    );
    assert!(fixture.consumer.is_empty());
}
