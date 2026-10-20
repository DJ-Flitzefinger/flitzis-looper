//! Group capture shares one real action and the existing logical stop reservation.
use super::*;

fn play(id: usize) -> ScheduledCommand {
    ScheduledCommand::PlaySample {
        id,
        volume: 1.0,
        received_at_ns: None,
        expected_source_generation: None,
    }
}

#[derive(Debug, PartialEq)]
struct Image {
    events: Vec<Option<ScheduledEvent>>,
    len: usize,
    next_sequence: u64,
    stop_owner: Option<u64>,
    deferred: Option<DueEvent>,
    deferred_at_callback: u64,
}

fn image<const CAPACITY: usize>(scheduler: &FixedCapacityScheduler<CAPACITY>) -> Image {
    Image {
        events: scheduler.events.to_vec(),
        len: scheduler.len,
        next_sequence: scheduler.next_sequence,
        stop_owner: scheduler.capture_stop_owner,
        deferred: scheduler.deferred_due().cloned(),
        deferred_at_callback: scheduler.deferred_at_callback,
    }
}

#[test]
fn group_admission_needs_two_slots_without_partial_event_or_reservation() {
    let mut zero = FixedCapacityScheduler::<0>::new();
    let before = image(&zero);
    assert_eq!(
        zero.schedule_capture_group_start(11, 100, 3, 41),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&zero), before);

    let mut one = FixedCapacityScheduler::<1>::new();
    let before = image(&one);
    assert_eq!(
        one.schedule_capture_group_start(11, 100, 3, 41),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&one), before);

    let mut two = FixedCapacityScheduler::<2>::new();
    two.schedule(3, play(7)).unwrap();
    let before = image(&two);
    assert_eq!(
        two.schedule_capture_group_start(11, 100, 3, 41),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&two), before);
    assert_eq!(two.pop_due_at_callback_start(3).unwrap().command, play(7));

    let group = two.schedule_capture_group_start(11, 100, 3, 41).unwrap();
    assert_eq!(
        group.command,
        ScheduledCommand::PitchAction {
            slot: 3,
            action_sequence: 41,
        }
    );
    assert_eq!(group.order_sequence, 41);
    assert_eq!(two.len(), 1);
    assert_eq!(two.capture_stop_owner, Some(11));
    assert!(two.is_full());
    assert_eq!(
        two.pop_due_at_callback_start(100).unwrap().command,
        group.command
    );
    assert!(two.is_empty());
    assert_eq!(two.peek_next_target_frame(), None);
    assert!(two.deferred_due().is_none());
    assert_eq!(two.capture_stop_owner, Some(11));
}

#[test]
fn group_admission_reuses_invalid_busy_and_queued_capture_rollback_guards() {
    let mut scheduler = FixedCapacityScheduler::<6>::new();
    scheduler.schedule(5, play(1)).unwrap();
    let before = image(&scheduler);
    assert_eq!(
        scheduler.schedule_capture_group_start(0, 10, 3, 41),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&scheduler), before);

    scheduler
        .schedule_capture_group_start(11, 10, 3, 41)
        .unwrap();
    let before = image(&scheduler);
    assert_eq!(
        scheduler.schedule_capture_group_start(12, 20, 4, 42),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&scheduler), before);
    scheduler.schedule_capture_stop(11, 30).unwrap();
    let before = image(&scheduler);
    assert_eq!(
        scheduler.schedule_capture_group_start(12, 20, 4, 42),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&scheduler), before);

    let mut deferred = FixedCapacityScheduler::<4>::new();
    deferred.schedule(5, play(2)).unwrap();
    let event = deferred.pop_due_at_callback_start(5).unwrap();
    deferred.defer_due(event, 5);
    let before = image(&deferred);
    assert_eq!(
        deferred.schedule_capture_group_start(11, 10, 3, 41),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&deferred), before);
}

#[test]
fn ordinary_events_and_foreign_stop_cannot_spend_group_stop_reservation() {
    let mut scheduler = FixedCapacityScheduler::<4>::new();
    let group = scheduler
        .schedule_capture_group_start(11, 10, 3, 41)
        .unwrap();
    scheduler.schedule(20, play(1)).unwrap();
    scheduler.schedule(21, play(2)).unwrap();
    assert_eq!(scheduler.len(), 3);
    assert!(scheduler.is_full());
    let before = image(&scheduler);
    assert_eq!(scheduler.schedule(22, play(3)), Err(ScheduleError::Full));
    assert_eq!(
        scheduler.schedule_capture_stop(12, 30),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&scheduler), before);

    let stop = scheduler.schedule_capture_stop(11, 30).unwrap();
    assert_eq!(scheduler.len(), 4);
    assert_eq!(scheduler.capture_stop_owner, None);
    assert!(scheduler.is_full());
    let before = image(&scheduler);
    assert_eq!(scheduler.schedule_capture_stop(11, 30), Ok(stop.clone()));
    assert_eq!(image(&scheduler), before);
    assert_eq!(
        scheduler.pop_due_at_callback_start(10).unwrap().command,
        group.command
    );
    assert_eq!(
        scheduler.pop_due_at_callback_start(20).unwrap().command,
        play(1)
    );
    assert_eq!(
        scheduler.pop_due_at_callback_start(21).unwrap().command,
        play(2)
    );
    assert_eq!(
        scheduler.pop_due_at_callback_start(30).unwrap().command,
        stop.command
    );
    assert!(scheduler.is_empty());
}

#[test]
fn replanned_group_keeps_original_action_order_before_its_own_equal_frame_stop() {
    let mut scheduler = FixedCapacityScheduler::<4>::new();
    let action_sequence = reserve_input_order().unwrap();
    let original = scheduler
        .schedule_capture_group_start(11, 10, 3, action_sequence)
        .unwrap();
    let stop = scheduler.schedule_capture_stop(11, 100).unwrap();
    assert!(original.order_sequence < stop.order_sequence);
    let due = scheduler.pop_due_at_callback_start(10).unwrap();
    assert_eq!(due.command, original.command);

    // A replan reinserts the same existing action after the later stop was queued.
    let replanned = scheduler.schedule(100, due.command).unwrap();
    assert!(replanned.sequence > stop.sequence);
    assert_eq!(replanned.order_sequence, original.order_sequence);
    assert_eq!(scheduler.capture_stop_target_frame(11), Some(100));
    assert_eq!(scheduler.events[0], Some(replanned.clone()));
    assert_eq!(scheduler.events[1], Some(stop.clone()));
    assert_eq!(
        scheduler.pop_due_at_callback_start(100).unwrap().command,
        replanned.command
    );
    assert_eq!(
        scheduler.pop_due_at_callback_start(100).unwrap().command,
        stop.command
    );
    assert!(scheduler.is_empty());
}

#[test]
fn capture_cancellation_keeps_group_action_and_foreign_stop_or_reservation() {
    let mut scheduler = FixedCapacityScheduler::<6>::new();
    let group = scheduler
        .schedule_capture_group_start(11, 10, 3, 41)
        .unwrap();
    let before = image(&scheduler);
    assert_eq!(scheduler.cancel_capture_session(12), 0);
    assert_eq!(image(&scheduler), before);
    assert_eq!(scheduler.cancel_capture_session(11), 0);
    assert_eq!(scheduler.capture_stop_owner, None);
    assert_eq!(scheduler.events[0], Some(group.clone()));

    scheduler.schedule_capture_start(12, 20).unwrap();
    let before = image(&scheduler);
    assert_eq!(scheduler.cancel_capture_session(11), 0);
    assert_eq!(image(&scheduler), before);
    assert_eq!(scheduler.capture_stop_owner, Some(12));
    scheduler.schedule_capture_stop(12, 30).unwrap();
    let before = image(&scheduler);
    assert_eq!(scheduler.cancel_capture_session(11), 0);
    assert_eq!(image(&scheduler), before);
    assert_eq!(scheduler.capture_stop_target_frame(12), Some(30));
    assert_eq!(scheduler.cancel_capture_session(12), 2);
    assert_eq!(scheduler.events[0], Some(group));
    assert_eq!(scheduler.len(), 1);
    assert!(scheduler.events[1..].iter().all(Option::is_none));
}

#[test]
fn own_capture_stop_cancellation_preserves_group_and_unrelated_playback() {
    let mut scheduler = FixedCapacityScheduler::<4>::new();
    let group = scheduler
        .schedule_capture_group_start(11, 10, 3, 41)
        .unwrap();
    scheduler.schedule_capture_stop(11, 30).unwrap();
    let playback = scheduler.schedule(20, ScheduledCommand::StopAll).unwrap();
    let next_sequence = scheduler.next_sequence;
    assert_eq!(scheduler.cancel_capture_session(11), 1);
    assert_eq!(scheduler.capture_stop_owner, None);
    assert_eq!(scheduler.next_sequence, next_sequence);
    assert_eq!(scheduler.events[0], Some(group));
    assert_eq!(scheduler.events[1], Some(playback));
    assert_eq!(scheduler.len(), 2);
    assert!(scheduler.events[2..].iter().all(Option::is_none));
}

#[test]
fn capture_cancellation_never_removes_deferred_group_pitch_action() {
    let mut scheduler = FixedCapacityScheduler::<3>::new();
    scheduler
        .schedule_capture_group_start(11, 10, 3, 41)
        .unwrap();
    let group = scheduler.pop_due_at_callback_start(10).unwrap();
    scheduler.defer_due(group.clone(), 10);
    let before = image(&scheduler);
    assert_eq!(scheduler.cancel_capture_session(12), 0);
    assert_eq!(image(&scheduler), before);
    assert_eq!(scheduler.cancel_capture_session(11), 0);
    assert_eq!(scheduler.capture_stop_owner, None);
    assert_eq!(scheduler.deferred_due(), Some(&group));
    assert_eq!(scheduler.deferred_at_callback, 10);
    assert_eq!(scheduler.pop_due_at_callback_start(10), None);
    assert_eq!(
        scheduler.pop_due_at_callback_start(11).unwrap().command,
        group.command
    );
    assert!(scheduler.is_empty());
}
