//! Whole-set cancellation, ordered STOP and external completion use actual publication claims.
use super::super::content_placement::PlacementError;
use super::super::resident_relocation::control_publication_tests::PendingWriter;
use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

thread_local! {
    static PENDING_OBSERVER: RefCell<Option<Box<dyn FnMut()>>> = RefCell::new(None);
}

pub(super) fn observe_pending() {
    PENDING_OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow_mut().as_mut() {
            observer();
        }
    });
}

fn with_pending_observer<T>(observer: impl FnMut() + 'static, action: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            PENDING_OBSERVER.with(|slot| *slot.borrow_mut() = None);
        }
    }
    PENDING_OBSERVER.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(observer));
    });
    let _reset = Reset;
    action()
}

fn loaded(owner: &Arc<InputRuntimeOwnership>, id: usize) -> (SampleBuffer, SourcePublication) {
    let sample = SampleBuffer {
        channels: 1,
        samples: Arc::from([0.25; 64]),
        residency: None,
    };
    let before = owner.slot_snapshot(id).unwrap();
    let write = owner
        .prepare_slot_write(before, super::super::content_lifetime::reserve().unwrap())
        .unwrap();
    let publication = owner
        .publish_source_write(
            &write,
            &sample,
            48_000,
            7,
            2,
            Some(TimingIntent::Legacy),
            true,
            || true,
        )
        .unwrap();
    (sample, publication)
}

#[test]
fn bulk_cancel_preserves_admitted_targets_and_terminal_revision_never_wraps() {
    pyo3::Python::initialize();
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let (_, first) = loaded(&owner, 1);
    let (_, last) = loaded(&owner, NUM_SAMPLES - 1);
    owner.mark_launch_admitted(1);
    owner.mark_launch_admitted(NUM_SAMPLES - 1);
    owner
        .launch_revision_for_cell(last.placement.cell())
        .store(u64::MAX, Ordering::Release);
    assert_eq!(
        cancel_launches_with_producer(&owner, None, None).unwrap(),
        vec![1, NUM_SAMPLES - 1]
    );
    assert_eq!(owner.launch_revision(1), 1);
    assert_eq!(owner.launch_revision(NUM_SAMPLES - 1), u64::MAX);
    assert_eq!(owner.admitted_launch_ids(), vec![1, NUM_SAMPLES - 1]);
    assert!(owner.content_key_current(first.key().unwrap()));
    assert!(
        owner
            .prepare_launch_write(
                owner.slot_snapshot(NUM_SAMPLES - 1).unwrap(),
                LaunchWriteKind::Admit
            )
            .is_err()
    );
}

#[test]
fn bulk_cancel_pending_has_no_partial_scalars_or_successful_target_report() {
    pyo3::Python::initialize();
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let (_, first) = loaded(&owner, 1);
    owner.mark_launch_admitted(1);
    let held = PendingWriter::hold(owner.clone());
    let retries = Rc::new(Cell::new(0));
    let observed_retries = retries.clone();
    assert!(
        with_pending_observer(
            move || observed_retries.set(observed_retries.get() + 1),
            || cancel_launches_with_producer(&owner, None, None),
        )
        .is_err()
    );
    assert_eq!(retries.get(), CONTROL_LAUNCH_PUBLICATION_ATTEMPTS - 1);
    assert_eq!(
        owner
            .launch_revision_for_cell(first.placement.cell())
            .load(Ordering::Acquire),
        0
    );
    assert!(
        owner
            .launch_admitted_for_cell(first.placement.cell())
            .load(Ordering::Acquire)
    );
    held.complete();
    assert_eq!(
        cancel_launches_with_producer(&owner, None, None).unwrap(),
        vec![1]
    );
    assert_eq!(owner.launch_revision(1), 1);
}

#[test]
fn bulk_cancel_retries_pending_publication_under_original_producer_exclusion() {
    pyo3::Python::initialize();
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let (first_sample, first) = loaded(&owner, 1);
    let (second_sample, second) = loaded(&owner, 2);
    owner.set_timing_intent_for_cell(first.placement.cell(), TimingIntent::Manual);
    owner.set_timing_intent_for_cell(second.placement.cell(), TimingIntent::Tap);
    owner.mark_launch_admitted(1);
    owner.mark_launch_admitted(2);
    let first_binding = first.binding(&first_sample, 48_000);
    let second_binding = second.binding(&second_sample, 48_000);
    let revisions = (owner.launch_revision(1), owner.launch_revision(2));
    assert!(owner.binding_launch_current(1, first_binding, revisions.0));
    assert!(owner.binding_launch_current(2, second_binding, revisions.1));
    let (producer, _consumer) = rtrb::RingBuffer::new(1);
    let producer = Arc::new(Mutex::new(producer));
    let same_producer = producer.clone();
    let observed = Rc::new(Cell::new(0));
    let observed_hook = observed.clone();
    let observed_owner = owner.clone();
    let mut held = Some(PendingWriter::hold(owner.clone()));
    let targets = with_pending_observer(
        move || {
            observed_hook.set(observed_hook.get() + 1);
            assert!(matches!(
                same_producer.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ));
            assert_eq!(
                observed_owner
                    .launch_revision_for_cell(first.placement.cell())
                    .load(Ordering::Acquire),
                revisions.0
            );
            assert_eq!(
                observed_owner
                    .launch_revision_for_cell(second.placement.cell())
                    .load(Ordering::Acquire),
                revisions.1
            );
            held.take().unwrap().complete();
        },
        || cancel_launches_with_producer(&owner, Some(&producer), None),
    )
    .unwrap();
    assert_eq!(observed.get(), 1);
    assert_eq!(targets, vec![1, 2]);
    assert_eq!(owner.admitted_launch_ids(), targets);
    assert!(!owner.binding_launch_current(1, first_binding, revisions.0));
    assert!(!owner.binding_launch_current(2, second_binding, revisions.1));
    assert!(owner.binding_launch_current(1, first_binding, revisions.0 + 1));
    assert!(owner.binding_launch_current(2, second_binding, revisions.1 + 1));
    assert!(first.current(&first_sample, 48_000));
    assert!(second.current(&second_sample, 48_000));
    assert_eq!(
        (owner.runtime_revision(1), owner.runtime_revision(2)),
        (0, 0)
    );
    assert_eq!(
        (
            owner.timing_intent[first.placement.cell().index()].load(Ordering::Acquire),
            owner.timing_intent[second.placement.cell().index()].load(Ordering::Acquire),
        ),
        (2, 3)
    );
    assert_eq!(
        (owner.cold_status(1, 7), owner.cold_status(2, 7)),
        (Some(0), Some(0))
    );
    assert!(producer.try_lock().is_ok());
}

#[test]
fn changed_last_bulk_participant_rejects_every_earlier_write() {
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let ids: Vec<_> = (0..NUM_SAMPLES).collect();
    let prepared = owner
        .prepare_launch_set(&ids, LaunchWriteKind::Stop)
        .unwrap();
    owner.launch_revisions[NUM_SAMPLES - 1].store(11, Ordering::Release);
    let before = owner.slot_snapshot(0).unwrap();
    assert!(
        owner
            .commit_launch_set(
                &prepared,
                || true,
                || panic!("partial STOP must not enqueue")
            )
            .is_err()
    );
    assert_eq!(owner.slot_snapshot(0), Some(before));
    assert!(
        owner.launch_revisions[..NUM_SAMPLES - 1]
            .iter()
            .all(|revision| revision.load(Ordering::Acquire) == 0)
    );
    assert_eq!(owner.launch_revision(NUM_SAMPLES - 1), 11);
}

#[test]
fn control_retry_does_not_readmit_retired_or_changed_participants() {
    for retire in [false, true] {
        let owner = Arc::new(InputRuntimeOwnership::tracked());
        let (_, first) = loaded(&owner, 1);
        loaded(&owner, 2);
        let prepared = owner
            .prepare_control_launch_set(&[1, 2], LaunchWriteKind::Cancel)
            .unwrap();
        if retire {
            loaded(&owner, 1);
            assert!(!owner.content_key_current(first.key().unwrap()));
        } else {
            owner.launch_revisions[2].store(11, Ordering::Release);
        }
        let attempts = Cell::new(0);
        let result = retry_control_launch_publication(|| {
            attempts.set(attempts.get() + 1);
            owner.commit_control_launch_set(
                &prepared,
                || true,
                || panic!("terminal failure must not apply a partial cancellation"),
            )
        });
        assert_eq!(attempts.get(), 1);
        assert_eq!(
            result,
            Err(ControlLaunchSetError::Publication(if retire {
                PlacementError::Retired
            } else {
                PlacementError::ClaimRejected
            }))
        );
        assert_eq!(owner.launch_revision(1), 0);
        assert_eq!(owner.launch_revision(2), if retire { 0 } else { 11 });
    }
}

#[test]
fn ordered_stop_preflights_capacity_and_pending_then_publishes_captured_payload() {
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let (_, publication) = loaded(&owner, 4);
    owner.mark_launch_admitted(4);
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
    producer
        .push(crate::messages::ControlMessage::Ping())
        .unwrap();
    assert!(!enqueue_stop_with_producer(&owner, &mut producer, Some(4)));
    assert_eq!(owner.launch_revision(4), 0);
    assert!(owner.launch_admitted(4));
    consumer.pop().unwrap();
    let held = PendingWriter::hold(owner.clone());
    assert!(!enqueue_stop_with_producer(&owner, &mut producer, None));
    assert!(consumer.is_empty());
    assert_eq!(
        owner
            .launch_revision_for_cell(publication.placement.cell())
            .load(Ordering::Acquire),
        0
    );
    held.complete();
    assert!(enqueue_stop_with_producer(&owner, &mut producer, Some(4)));
    match consumer.pop().unwrap() {
        crate::messages::ControlMessage::StopSampleForPlacement { placement } => {
            assert_eq!(placement.content_key(), publication.key())
        }
        _ => panic!("STOP must retain its captured placement"),
    }
    assert_eq!(owner.launch_revision(4), 1);
    assert!(!owner.launch_admitted(4));
}

#[test]
fn ordered_stop_retries_pending_publication_without_releasing_producer() {
    for all in [false, true] {
        let owner = Arc::new(InputRuntimeOwnership::tracked());
        let (sample, publication) = loaded(&owner, 4);
        owner.mark_launch_admitted(4);
        let binding = publication.binding(&sample, 48_000);
        let revision = owner.launch_revision(4);
        let (producer, mut consumer) = rtrb::RingBuffer::new(1);
        let producer = Arc::new(Mutex::new(producer));
        let same_producer = producer.clone();
        let observed = Rc::new(Cell::new(0));
        let observed_hook = observed.clone();
        let mut held = Some(PendingWriter::hold(owner.clone()));
        let mut guard = producer.lock().unwrap();
        assert!(with_pending_observer(
            move || {
                observed_hook.set(observed_hook.get() + 1);
                assert!(matches!(
                    same_producer.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                ));
                held.take().unwrap().complete();
            },
            || enqueue_stop_with_producer(&owner, &mut guard, if all { None } else { Some(4) }),
        ));
        assert_eq!(observed.get(), 1);
        match consumer.pop().unwrap() {
            crate::messages::ControlMessage::StopAll() if all => {}
            crate::messages::ControlMessage::StopSampleForPlacement { placement } if !all => {
                assert_eq!(placement.content_key(), publication.key());
            }
            _ => panic!("STOP must retain its original admitted scope"),
        }
        assert!(consumer.is_empty());
        assert!(!owner.binding_launch_current(4, binding, revision));
        assert!(!owner.launch_admitted(4));
        assert!(publication.current(&sample, 48_000));
        assert_eq!(owner.cold_status(4, 7), Some(0));
    }
}

#[test]
fn fresh_lifetime_restores_launch_permission_after_exhausted_predecessor() {
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let (sample, first) = loaded(&owner, 4);
    owner
        .launch_revision_for_cell(first.placement.cell())
        .store(u64::MAX, Ordering::Release);
    owner.mark_launch_admitted(4);
    owner
        .resident_control_for_cell(first.placement.cell())
        .store(5, Ordering::Release);
    let before = owner.slot_snapshot(4).unwrap();
    let write = owner
        .prepare_slot_write(before, super::super::content_lifetime::reserve().unwrap())
        .unwrap();
    owner
        .commit_slot_write_fixed(
            &write,
            || true,
            |cell| owner.apply_source_for_cell(cell, &sample, 48_000, 8, 3, TimingIntent::Legacy),
        )
        .unwrap();
    assert!(!owner.content_key_current(first.key().unwrap()));
    assert_eq!(owner.launch_revision(4), 0);
    assert!(!owner.launch_admitted(4));
    assert!(!owner.resident_control_pending(4));
    assert!(
        owner
            .prepare_launch_write(owner.slot_snapshot(4).unwrap(), LaunchWriteKind::Admit)
            .is_ok()
    );
    owner
        .resident_control_for_cell(first.placement.cell())
        .store(5, Ordering::Release);
    assert!(owner.try_finish_resident_control_for_binding(4, first.binding(&sample, 48_000), 5));
    assert_eq!(
        owner
            .resident_control_for_cell(first.placement.cell())
            .load(Ordering::Acquire),
        5
    );
}

#[test]
fn migration_bulk_hold_and_release_preserve_flags_and_reject_partial_owner_sets() {
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    loaded(&owner, 1);
    loaded(&owner, 2);
    owner.mark_launch_admitted(1);
    owner.commit_migration_holds(&[1, 2], 19, true).unwrap();
    assert_eq!((owner.migration_hold(1), owner.migration_hold(2)), (19, 19));
    assert_eq!((owner.launch_revision(1), owner.launch_revision(2)), (1, 1));
    assert!(owner.launch_admitted(1));
    assert!(owner.commit_migration_holds(&[1, 3], 19, false).is_err());
    assert_eq!(owner.migration_hold(1), 19);
    assert_eq!(owner.launch_revision(1), 1);
    let held = PendingWriter::hold(owner.clone());
    assert!(owner.commit_migration_holds(&[1, 2], 19, false).is_err());
    held.complete();
    owner.commit_migration_holds(&[1, 2], 19, false).unwrap();
    assert_eq!((owner.migration_hold(1), owner.migration_hold(2)), (0, 0));
    assert_eq!((owner.launch_revision(1), owner.launch_revision(2)), (2, 2));
    assert!(owner.launch_admitted(1));
}

#[test]
fn external_resident_finish_retries_pending_and_never_clears_successor() {
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let (sample, publication) = loaded(&owner, 4);
    let binding = publication.binding(&sample, 48_000);
    owner
        .resident_control_for_cell(publication.placement.cell())
        .store(5, Ordering::Release);
    let held = PendingWriter::hold(owner.clone());
    assert!(!owner.try_finish_resident_control_for_binding(4, binding, 5));
    assert_eq!(
        owner
            .resident_control_for_cell(publication.placement.cell())
            .load(Ordering::Acquire),
        5
    );
    held.complete();
    owner
        .resident_control_for_cell(publication.placement.cell())
        .store(6, Ordering::Release);
    assert!(owner.try_finish_resident_control_for_binding(4, binding, 5));
    assert_eq!(
        owner
            .resident_control_for_cell(publication.placement.cell())
            .load(Ordering::Acquire),
        6
    );
    assert!(owner.try_finish_resident_control_for_binding(4, binding, 6));
    assert!(!owner.resident_control_pending(4));
}

#[test]
fn cold_cancellation_pending_preserves_both_permissions_and_native_claim_wins() {
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let (_, publication) = loaded(&owner, 4);
    let adoption = AtomicU8::new(0);
    let held = PendingWriter::hold(owner.clone());
    assert_eq!(
        owner.cancel_cold_with_adoption_fixed(&publication, &adoption),
        Err(PlacementError::Pending)
    );
    assert_eq!(adoption.load(Ordering::Acquire), 0);
    assert_eq!(
        owner.cold_adoptions[publication.placement.cell().index()].load(Ordering::Acquire),
        28
    );
    held.complete();
    let snapshot = owner.slot_snapshot(4).unwrap();
    let write = owner
        .prepare_slot_write_fixed(snapshot, snapshot.lifetime())
        .unwrap();
    owner
        .commit_slot_write_fixed(
            &write,
            || {
                adoption.store(1, Ordering::Release);
                owner.claim_cold_publication(&publication)
            },
            |_| {},
        )
        .unwrap();
    assert_eq!(
        owner.cancel_cold_with_adoption_fixed(&publication, &adoption),
        Ok(false)
    );
    assert_eq!(adoption.load(Ordering::Acquire), 1);
    assert_eq!(
        owner.cold_adoptions[publication.placement.cell().index()].load(Ordering::Acquire),
        29
    );
}

#[test]
fn shutdown_retries_actual_pending_writer_before_revoking_whole_set() {
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    loaded(&owner, 4);
    owner.mark_launch_admitted(4);
    let held = PendingWriter::hold(owner.clone());
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(10));
        held.complete();
    });
    cancel_launches_before_shutdown(&owner, None).unwrap();
    release.join().unwrap();
    assert!(
        owner
            .launch_revisions
            .iter()
            .all(|revision| revision.load(Ordering::Acquire) == 1)
    );
    assert_eq!(owner.admitted_launch_ids(), vec![4]);
}
