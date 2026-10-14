use super::*;
use crate::audio_engine::input_mapping::launch_admission_tests::{LaunchFixture, dispatch};
use crate::audio_engine::resident_relocation::control_publication_tests::PendingWriter;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, ResidentContext, TriggerQuantization};
use rtrb::RingBuffer;

fn fixture() -> LaunchFixture {
    let mut fixture = LaunchFixture::new(4);
    let (_, feedback) = RingBuffer::<AudioMessage>::new(8);
    fixture.engine.test_control_channels =
        Some((fixture.producer.clone(), Arc::new(Mutex::new(feedback))));
    fixture
}

fn acknowledged(fixture: &mut LaunchFixture, target: usize) -> PadPairTicket {
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let captured = capture(&fixture.engine, 0, target).unwrap();
    let ticket = prepare(&fixture.engine, &captured, 48_000, 1).unwrap();
    assert!(!reconcile(&fixture.engine, &ticket).unwrap());
    publish(&fixture.engine, &ticket).unwrap();
    let message = fixture.consumer.pop().unwrap();
    dispatch(
        &mut mixer,
        message,
        &mut FixedCapacityScheduler::<8>::new(),
        &mut TransportTimeline::new(48_000),
        &mut TriggerQuantization::Immediate,
        &mut Vec::new(),
    );
    assert!(ticket.native_acknowledgement().is_some());
    ticket
}

#[test]
fn actual_pair_ack_mirrors_exact_cache_history_owners_once_without_finishing() {
    let mut fixture = fixture();
    let samples = fixture.engine.sample_cache.lock().unwrap().clone();
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let epochs = fixture.engine.prepared_source_epochs.clone();
    let keys = [0, 1].map(|id| {
        fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(id)
            .unwrap()
            .content_key()
            .unwrap()
    });
    let resident_epochs = fixture
        .engine
        .resident_stem_cache
        .lock()
        .unwrap()
        .iter()
        .take(2)
        .map(|owner| owner.intent_epoch.clone())
        .collect::<Vec<_>>();
    for id in [0, 1] {
        fixture.engine.cold_pcm_history.lock().unwrap()[id]
            .push(Arc::downgrade(&samples[id].as_ref().unwrap().samples));
    }
    let ticket = acknowledged(&mut fixture, 1);
    assert!(matches!(
        fixture
            .engine
            .input_runtime_ownership
            .observe_control_key(keys[0]),
        crate::audio_engine::content_placement::PlacementObservation::Pending
    ));
    assert!(reconcile(&fixture.engine, &ticket).unwrap());
    assert!(reconcile(&fixture.engine, &ticket).unwrap());
    let cache = fixture.engine.sample_cache.lock().unwrap();
    assert!(
        cache[0]
            .as_ref()
            .unwrap()
            .same_window(samples[1].as_ref().unwrap())
    );
    assert!(
        cache[1]
            .as_ref()
            .unwrap()
            .same_window(samples[0].as_ref().unwrap())
    );
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    assert!(
        epochs
            .iter()
            .zip(&fixture.engine.prepared_source_epochs)
            .all(|(before, after)| Arc::ptr_eq(before, after))
    );
    let resident = fixture.engine.resident_stem_cache.lock().unwrap();
    assert!(Arc::ptr_eq(&resident[1].intent_epoch, &resident_epochs[0]));
    assert!(Arc::ptr_eq(&resident[0].intent_epoch, &resident_epochs[1]));
    let history = fixture.engine.cold_pcm_history.lock().unwrap();
    assert!(history[1][0].ptr_eq(&Arc::downgrade(&samples[0].as_ref().unwrap().samples)));
    assert_ne!(
        fixture.engine.input_runtime_ownership.pair_reservation(0),
        0
    );
    let envelope: serde_json::Value =
        serde_json::from_str(&ticket.snapshot_envelope_json()).unwrap();
    assert_eq!(envelope["control_mirrored"], true);
    assert_eq!(envelope["durable_finished"], false);
}

#[test]
fn actual_move_installs_independent_empty_defaults_and_preserves_zero_target_request() {
    let mut fixture = fixture();
    let empty_epoch = fixture.engine.resident_stem_cache.lock().unwrap()[2]
        .intent_epoch
        .clone();
    fixture.engine.timing_intents.lock().unwrap()[2] = TimingIntent::Tap;
    let empty = fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(2)
        .unwrap();
    fixture
        .engine
        .input_runtime_ownership
        .set_timing_intent_for_cell(empty.cell(), TimingIntent::Tap);
    let sample = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let ticket = acknowledged(&mut fixture, 2);
    assert!(reconcile(&fixture.engine, &ticket).unwrap());
    assert!(fixture.engine.sample_cache.lock().unwrap()[0].is_none());
    assert!(
        fixture.engine.sample_cache.lock().unwrap()[2]
            .as_ref()
            .unwrap()
            .same_window(&sample)
    );
    assert_eq!(
        fixture.engine.loaded_source_generations.lock().unwrap()[0],
        (0, 0)
    );
    assert_eq!(
        fixture.engine.loaded_source_generations.lock().unwrap()[2],
        (7, 48_000)
    );
    assert_eq!(fixture.engine.pad_request_ids.lock().unwrap()[2], 0);
    assert_eq!(
        fixture.engine.timing_intents.lock().unwrap()[0],
        TimingIntent::Legacy
    );
    assert!(!Arc::ptr_eq(
        &fixture.engine.resident_stem_cache.lock().unwrap()[0].intent_epoch,
        &empty_epoch
    ));
}

#[test]
fn native_move_source_capture_uses_logical_generation_after_private_reservation_release() {
    let mut fixture = fixture();
    let ticket = acknowledged(&mut fixture, 2);
    assert!(reconcile(&fixture.engine, &ticket).unwrap());
    let version = format!("samples/a.wav|sha256-v1:{:064x}", 7);
    assert!(
        fixture
            .engine
            .capture_prepared_source(2, version.clone())
            .is_err()
    );
    // Exercise admission after the exact private reservation is released. This
    // deliberately grants no durable/public Pair completion or controller rights.
    fixture
        .engine
        .input_runtime_ownership
        .release_pair(&ticket.state.snapshots(), ticket.state.guard.reservation)
        .unwrap();
    let prepared = fixture.engine.capture_prepared_source(2, version).unwrap();
    assert_eq!(prepared.request_id, 0);
    assert!(prepared.publication.current());
    assert_eq!(
        fixture.engine.loaded_source_generations.lock().unwrap()[2],
        (7, 48_000)
    );
    assert_eq!(fixture.engine.pad_request_ids.lock().unwrap()[2], 0);
}

#[test]
fn marker_contention_and_foreign_source_leave_mirror_fenced_then_retry_identical_owner() {
    let mut fixture = fixture();
    let ticket = acknowledged(&mut fixture, 1);
    let held = PendingWriter::hold(fixture.engine.input_runtime_ownership.clone());
    assert!(!reconcile(&fixture.engine, &ticket).unwrap());
    held.complete();
    let original = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let mut foreign = original.clone();
    foreign.residency = None;
    foreign.samples = Arc::from([0.25_f32; 128]);
    fixture.engine.sample_cache.lock().unwrap()[0] = Some(foreign);
    assert!(reconcile(&fixture.engine, &ticket).is_err());
    assert!(!ticket.state.mirrored.load(Ordering::Acquire));
    fixture.engine.sample_cache.lock().unwrap()[0] = Some(original);
    assert!(reconcile(&fixture.engine, &ticket).unwrap());
}

#[test]
fn post_ack_same_content_window_and_declared_intent_are_preserved() {
    let mut fixture = fixture();
    let ticket = acknowledged(&mut fixture, 1);
    let original = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let window = original
        .window(16, 96, 17, ResidentContext::FiniteLoop)
        .unwrap();
    let ownership = &fixture.engine.input_runtime_ownership;
    let current = ownership.slot_snapshot(1).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(current, current.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |cell| {
                ownership.publish_window_for_cell(cell, &window);
                ownership.set_timing_intent_for_cell(cell, TimingIntent::Tap);
            },
        )
        .unwrap();
    fixture.engine.sample_cache.lock().unwrap()[0] = Some(window.clone());
    fixture.engine.timing_intents.lock().unwrap()[0] = TimingIntent::Tap;
    assert!(reconcile(&fixture.engine, &ticket).unwrap());
    assert!(
        fixture.engine.sample_cache.lock().unwrap()[1]
            .as_ref()
            .unwrap()
            .same_window(&window)
    );
    assert_eq!(
        fixture.engine.timing_intents.lock().unwrap()[1],
        TimingIntent::Tap
    );
}
