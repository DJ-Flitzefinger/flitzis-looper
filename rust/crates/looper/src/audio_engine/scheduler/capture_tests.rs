//! Actual fixed scheduler storage, logical stop capacity and original event ownership.
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
        len: scheduler.len(),
        next_sequence: scheduler.next_sequence,
        stop_owner: scheduler.capture_stop_owner,
        deferred: scheduler.deferred_due().cloned(),
        deferred_at_callback: scheduler.deferred_at_callback,
    }
}

fn commands<const CAPACITY: usize>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
) -> Vec<ScheduledCommand> {
    let mut commands = Vec::new();
    while let Some(event) = scheduler.pop_due_through(0, u64::MAX) {
        commands.push(event.command);
    }
    commands
}

#[test]
fn start_needs_both_real_start_capacity_and_one_future_stop_slot() {
    let mut zero = FixedCapacityScheduler::<0>::new();
    let before = image(&zero);
    assert_eq!(
        zero.schedule_capture_start(11, 100),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&zero), before);

    let mut one = FixedCapacityScheduler::<1>::new();
    let before = image(&one);
    assert_eq!(
        one.schedule_capture_start(11, 100),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&one), before);
    one.schedule(3, play(7)).unwrap();
    assert_eq!(commands(&mut one), vec![play(7)]);

    let mut two = FixedCapacityScheduler::<2>::new();
    two.schedule(3, play(7)).unwrap();
    let before = image(&two);
    assert_eq!(
        two.schedule_capture_start(11, 100),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&two), before);
    assert_eq!(commands(&mut two), vec![play(7)]);
    two.schedule_capture_start(11, 100).unwrap();
    assert_eq!(two.len(), 1);
    assert_eq!(two.capture_stop_owner, Some(11));
    assert!(two.is_full());
}

#[test]
fn active_stop_reservation_is_not_a_future_event_or_nonempty_sentinel() {
    let mut scheduler = FixedCapacityScheduler::<3>::new();
    scheduler.schedule_capture_start(11, 9).unwrap();
    let start = scheduler.pop_due_at_callback_start(9).unwrap();
    assert_eq!(
        start.command,
        ScheduledCommand::CaptureStart { session_id: 11 }
    );
    assert_eq!(scheduler.capture_stop_owner, Some(11));
    assert_eq!(scheduler.len(), 0);
    assert!(scheduler.is_empty());
    assert_eq!(scheduler.peek_next_target_frame(), None);
    assert!(scheduler.deferred_due().is_none());
    assert!(!scheduler.is_full());
    let before = image(&scheduler);
    assert_eq!(
        scheduler.schedule_capture_start(12, 10),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&scheduler), before);
    assert_eq!(scheduler.cancel_capture_session(11), 0);
    assert_eq!(scheduler.capture_stop_owner, None);
    assert!(scheduler.is_empty());
    scheduler.schedule_capture_start(12, 10).unwrap();
}

#[test]
fn ordinary_full_cannot_spend_reserved_stop_and_matching_stop_uses_it_once() {
    let mut scheduler = FixedCapacityScheduler::<4>::new();
    scheduler.schedule_capture_start(11, 1).unwrap();
    scheduler.pop_due_at_callback_start(1).unwrap();
    for id in 0..3 {
        scheduler.schedule(20 + id as u64, play(id)).unwrap();
    }
    assert!(scheduler.is_full());
    assert_eq!(scheduler.len(), 3);
    let before = image(&scheduler);
    assert_eq!(scheduler.schedule(25, play(4)), Err(ScheduleError::Full));
    assert_eq!(
        scheduler.schedule_capture_stop(12, 30),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&scheduler), before);

    let stop = scheduler.schedule_capture_stop(11, 30).unwrap();
    assert_eq!(scheduler.capture_stop_owner, None);
    assert_eq!(scheduler.len(), 4);
    assert!(scheduler.is_full());
    let accepted = image(&scheduler);
    assert_eq!(scheduler.schedule_capture_stop(11, 30), Ok(stop));
    assert_eq!(image(&scheduler), accepted);
    assert_eq!(
        scheduler.schedule_capture_stop(11, 31),
        Err(ScheduleError::Full)
    );
    assert_eq!(
        scheduler.schedule_capture_start(12, 31),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&scheduler), accepted);
    assert_eq!(
        commands(&mut scheduler),
        vec![
            play(0),
            play(1),
            play(2),
            ScheduledCommand::CaptureStop { session_id: 11 }
        ]
    );
    assert!(scheduler.is_empty());
}

#[test]
fn cancellation_preserves_playback_stop_all_and_foreign_session_reservation() {
    let mut scheduler = FixedCapacityScheduler::<6>::new();
    let playback_stop = scheduler.schedule(7, ScheduledCommand::StopAll).unwrap();
    scheduler.schedule_capture_start(11, 10).unwrap();
    let playback_start = scheduler.schedule(12, play(3)).unwrap();
    let before = image(&scheduler);
    assert_eq!(scheduler.cancel_capture_session(12), 0);
    assert_eq!(image(&scheduler), before);
    assert_eq!(scheduler.cancel_capture_session(0), 0);
    assert_eq!(image(&scheduler), before);

    let stopped = scheduler.pop_due_at_callback_start(7).unwrap();
    assert_eq!(stopped.command, ScheduledCommand::StopAll);
    assert_eq!(stopped.target_frame, playback_stop.target_frame);
    assert_eq!(scheduler.capture_stop_owner, Some(11));
    assert_eq!(scheduler.cancel_capture_session(11), 1);
    assert_eq!(scheduler.capture_stop_owner, None);
    assert_eq!(scheduler.events[0], Some(playback_start));

    scheduler.schedule_capture_start(12, 20).unwrap();
    let current = image(&scheduler);
    assert_eq!(scheduler.cancel_capture_session(11), 0);
    assert_eq!(image(&scheduler), current);
    assert_eq!(scheduler.capture_stop_owner, Some(12));
    assert_eq!(scheduler.cancel_capture_session(12), 1);
    assert_eq!(commands(&mut scheduler), vec![play(3)]);
}

#[test]
fn cancelling_queued_start_and_stop_removes_only_that_session_in_original_order() {
    let mut scheduler = FixedCapacityScheduler::<6>::new();
    let first = scheduler.schedule(7, play(1)).unwrap();
    scheduler.schedule_capture_start(11, 10).unwrap();
    let middle = scheduler.schedule(11, ScheduledCommand::StopAll).unwrap();
    scheduler.schedule_capture_stop(11, 20).unwrap();
    let last = scheduler.schedule(30, play(2)).unwrap();
    let next_sequence = scheduler.next_sequence;
    assert_eq!(scheduler.cancel_capture_session(11), 2);
    assert_eq!(scheduler.capture_stop_owner, None);
    assert_eq!(scheduler.len(), 3);
    assert_eq!(scheduler.next_sequence, next_sequence);
    assert_eq!(scheduler.events[0], Some(first));
    assert_eq!(scheduler.events[1], Some(middle));
    assert_eq!(scheduler.events[2], Some(last));
    assert!(scheduler.events[3..].iter().all(Option::is_none));
    assert_eq!(scheduler.cancel_capture_session(11), 0);
    assert_eq!(
        commands(&mut scheduler),
        vec![play(1), ScheduledCommand::StopAll, play(2)]
    );
}

#[test]
fn deferred_foreign_command_blocks_stop_without_losing_its_reserved_slot_or_retry_gate() {
    let mut scheduler = FixedCapacityScheduler::<4>::new();
    scheduler.schedule_capture_start(11, 1).unwrap();
    scheduler.pop_due_at_callback_start(1).unwrap();
    scheduler.schedule(5, play(3)).unwrap();
    let foreign = scheduler.pop_due_at_callback_start(5).unwrap();
    scheduler.defer_due(foreign.clone(), 5);
    let before = image(&scheduler);
    assert_eq!(
        scheduler.schedule_capture_stop(11, 6),
        Err(ScheduleError::Full)
    );
    assert_eq!(
        scheduler.schedule_capture_start(12, 6),
        Err(ScheduleError::Full)
    );
    assert_eq!(scheduler.cancel_capture_session(12), 0);
    assert_eq!(image(&scheduler), before);
    assert_eq!(scheduler.cancel_capture_session(11), 0);
    assert_eq!(scheduler.capture_stop_owner, None);
    assert_eq!(scheduler.deferred_due(), Some(&foreign));
    assert_eq!(scheduler.pop_due_through(5, 10), None);
    let retried = scheduler.pop_due_through(6, 10).unwrap();
    assert_eq!(retried.command, play(3));
    assert_eq!(retried.target_frame, 5);
    assert_eq!(retried.execution_frame, 6);
    assert!(retried.was_late);
}

#[test]
fn cancellation_removes_own_deferred_capture_without_a_ghost_or_foreign_loss() {
    let mut scheduler = FixedCapacityScheduler::<4>::new();
    scheduler.schedule_capture_start(11, 5).unwrap();
    let playback = scheduler.schedule(9, play(2)).unwrap();
    let capture = scheduler.pop_due_at_callback_start(5).unwrap();
    scheduler.defer_due(capture, 5);
    assert_eq!(scheduler.cancel_capture_session(12), 0);
    assert_eq!(scheduler.cancel_capture_session(11), 1);
    assert_eq!(scheduler.capture_stop_owner, None);
    assert!(scheduler.deferred_due().is_none());
    assert_eq!(scheduler.len(), 1);
    assert_eq!(scheduler.events[0], Some(playback));
    assert_eq!(scheduler.peek_next_target_frame(), Some(9));
    assert_eq!(commands(&mut scheduler), vec![play(2)]);
    assert!(scheduler.is_empty());
    scheduler.schedule_capture_start(12, 10).unwrap();
}

#[test]
fn deferred_stop_duplicate_cannot_reschedule_or_replace_its_original_execution() {
    let mut scheduler = FixedCapacityScheduler::<3>::new();
    scheduler.schedule_capture_start(11, 1).unwrap();
    scheduler.pop_due_at_callback_start(1).unwrap();
    scheduler.schedule_capture_stop(11, 5).unwrap();
    let stop = scheduler.pop_due_at_callback_start(5).unwrap();
    scheduler.defer_due(stop.clone(), 5);
    let before = image(&scheduler);
    assert_eq!(
        scheduler.schedule_capture_stop(11, 5),
        Err(ScheduleError::Full)
    );
    assert_eq!(scheduler.cancel_capture_session(12), 0);
    assert_eq!(image(&scheduler), before);
    assert_eq!(scheduler.pop_due_at_callback_start(5), None);
    let retried = scheduler.pop_due_at_callback_start(6).unwrap();
    assert_eq!(retried.command, stop.command);
    assert_eq!(retried.target_frame, 5);
    assert_eq!(retried.execution_frame, 6);
    assert!(retried.was_late);
    assert!(scheduler.is_empty());
}

#[test]
fn ordering_allocator_failure_leaves_start_and_stop_admission_atomic() {
    let mut scheduler = FixedCapacityScheduler::<4>::new();
    scheduler.schedule(30, play(3)).unwrap();
    let before = image(&scheduler);
    assert_eq!(
        scheduler.schedule_capture_start_with_order(11, 5, || None),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&scheduler), before);
    scheduler.schedule_capture_start(11, 5).unwrap();
    scheduler.pop_due_at_callback_start(5).unwrap();
    scheduler.schedule(31, play(4)).unwrap();
    scheduler.schedule(32, play(5)).unwrap();
    assert!(scheduler.is_full());
    let before = image(&scheduler);
    assert_eq!(
        scheduler.schedule_capture_stop_with_order(11, 40, || None),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&scheduler), before);
    scheduler.schedule_capture_stop(11, 40).unwrap();
    assert_eq!(scheduler.capture_stop_owner, None);
    assert_eq!(scheduler.len(), 4);
}

#[test]
fn refused_preflight_does_not_consume_an_order_and_stop_cannot_precede_pending_start() {
    let mut scheduler = FixedCapacityScheduler::<3>::new();
    assert_eq!(
        scheduler.schedule_capture_start_with_order(0, 10, || panic!(
            "invalid session allocated an order"
        )),
        Err(ScheduleError::Full)
    );
    scheduler.schedule_capture_start(11, 10).unwrap();
    let before = image(&scheduler);
    assert_eq!(
        scheduler.schedule_capture_start_with_order(12, 11, || panic!(
            "busy session allocated an order"
        )),
        Err(ScheduleError::Full)
    );
    assert_eq!(
        scheduler
            .schedule_capture_stop_with_order(11, 9, || panic!("early stop allocated an order")),
        Err(ScheduleError::Full)
    );
    assert_eq!(
        scheduler
            .schedule_capture_stop_with_order(12, 10, || panic!("foreign stop allocated an order")),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&scheduler), before);
    scheduler.schedule_capture_stop(11, 10).unwrap();
    assert_eq!(
        commands(&mut scheduler),
        vec![
            ScheduledCommand::CaptureStart { session_id: 11 },
            ScheduledCommand::CaptureStop { session_id: 11 }
        ]
    );
}

#[test]
fn generic_schedule_cannot_bypass_capture_reservation_or_foreign_stop_checks() {
    let mut one = FixedCapacityScheduler::<1>::new();
    assert_eq!(
        one.schedule(10, ScheduledCommand::CaptureStart { session_id: 11 }),
        Err(ScheduleError::Full)
    );
    let mut scheduler = FixedCapacityScheduler::<3>::new();
    scheduler
        .schedule(10, ScheduledCommand::CaptureStart { session_id: 11 })
        .unwrap();
    assert_eq!(scheduler.capture_stop_owner, Some(11));
    let before = image(&scheduler);
    assert_eq!(
        scheduler.schedule(11, ScheduledCommand::CaptureStop { session_id: 12 }),
        Err(ScheduleError::Full)
    );
    assert_eq!(image(&scheduler), before);
    scheduler
        .schedule(11, ScheduledCommand::CaptureStop { session_id: 11 })
        .unwrap();
    assert_eq!(scheduler.capture_stop_owner, None);
    assert_eq!(scheduler.len(), 2);
    assert_eq!(scheduler.cancel_capture_session(11), 2);
    assert!(scheduler.is_empty());
}

#[test]
fn capture_admission_preserves_existing_local_sequence_saturation_and_total_order() {
    let mut scheduler = FixedCapacityScheduler::<3>::new();
    scheduler.next_sequence = u64::MAX;
    let start = scheduler.schedule_capture_start(11, 10).unwrap();
    let stop = scheduler.schedule_capture_stop(11, 10).unwrap();
    assert_eq!(start.sequence, u64::MAX);
    assert_eq!(stop.sequence, u64::MAX);
    assert_eq!(scheduler.next_sequence, u64::MAX);
    assert!(start.order_sequence < stop.order_sequence);
    assert_eq!(commands(&mut scheduler), vec![start.command, stop.command]);
}

#[test]
fn stop_endpoint_observation_requires_an_actual_own_queued_or_deferred_event() {
    let mut scheduler = FixedCapacityScheduler::<3>::new();
    assert_eq!(scheduler.capture_stop_target_frame(0), None);
    scheduler.schedule_capture_start(11, 10).unwrap();
    assert_eq!(scheduler.capture_stop_target_frame(11), None);
    scheduler.schedule_capture_stop(11, 19).unwrap();
    assert_eq!(scheduler.capture_stop_target_frame(12), None);
    assert_eq!(scheduler.capture_stop_target_frame(11), Some(19));
    scheduler.pop_due_at_callback_start(10).unwrap();
    let stop = scheduler.pop_due_at_callback_start(20).unwrap();
    assert_eq!(scheduler.capture_stop_target_frame(11), None);
    scheduler.defer_due(stop, 20);
    assert_eq!(scheduler.capture_stop_target_frame(11), Some(19));
    assert_eq!(scheduler.capture_stop_target_frame(12), None);
    assert_eq!(scheduler.cancel_capture_session(11), 1);
    assert_eq!(scheduler.capture_stop_target_frame(11), None);
}
