//! Fixed-capacity absolute output-frame scheduler.
//!
//! The scheduler owns bounded storage so later audio-callback integration can
//! accept or reject quantized events without heap allocation, blocking, or
//! eviction of previously accepted events.

use crate::audio_engine::constants::MAX_SCHEDULED_EVENTS;
use crate::audio_engine::input_runtime_binding::InputPadBinding;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static INPUT_ORDER: AtomicU64 = AtomicU64::new(1);
static INPUT_ORDER_EXHAUSTED: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
thread_local! { static REFUSE_NEXT_ORDER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

#[cfg(test)]
pub(crate) fn with_input_order_exhaustion_for_test<T>(action: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            REFUSE_NEXT_ORDER.with(|value| value.set(false));
        }
    }
    REFUSE_NEXT_ORDER.with(|value| {
        assert!(!value.replace(true));
    });
    let _reset = Reset;
    action()
}

/// A single total ordering domain covers input captures and ordinary queued effects.
/// The reserved exhaustion margin exceeds all bounded producers; RT uses one atomic add.
pub(crate) fn reserve_input_order() -> Option<u64> {
    #[cfg(test)]
    if REFUSE_NEXT_ORDER.with(|value| value.replace(false)) {
        return None;
    }
    if INPUT_ORDER_EXHAUSTED.load(Ordering::Acquire) {
        return None;
    }
    let sequence = INPUT_ORDER.fetch_add(1, Ordering::AcqRel);
    if sequence >= u64::MAX - MAX_SCHEDULED_EVENTS as u64 {
        INPUT_ORDER_EXHAUSTED.store(true, Ordering::Release);
        return None;
    }
    Some(sequence)
}

pub(crate) type TransportScheduler = FixedCapacityScheduler<MAX_SCHEDULED_EVENTS>;

#[derive(Debug, Clone, PartialEq)]
// Compact inline launch guards measure 264 bytes/command on Windows x64; the
// 1024-slot pool (280 bytes/slot) is allocated once during control-side startup.
// Exact source/timing bindings must stay inline when RT schedules or rejects an
// event. Boxing them there would allocate/free on RT; this exception is limited
// to the generic variant-size heuristic, with fixed storage and bounded copies.
#[allow(clippy::large_enum_variant)]
pub(crate) enum ScheduledCommand {
    CaptureStart {
        session_id: u64,
    },
    CaptureStop {
        session_id: u64,
    },
    PitchAction {
        slot: usize,
        action_sequence: u64,
    },
    RefreshAcceptedTiming(std::sync::Arc<super::accepted_timing_refresh::AcceptedTimingRefresh>),
    GlobalPlaybackBatch(std::sync::Arc<super::global_playback_batch::GlobalPlaybackBatch>),
    TriggerInputPad {
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
        exclusive: bool,
        binding: InputPadBinding,
        received_at_ns: u64,
        resident_control: Option<super::resident_relocation::ResidentLaunchGuard>,
        launch_revision: u64,
    },
    PlaySample {
        id: usize,
        volume: f32,
        received_at_ns: Option<u64>,
        expected_source_generation: Option<u64>,
    },
    StopAllThenPlaySample {
        id: usize,
        volume: f32,
        received_at_ns: Option<u64>,
        expected_source_generation: Option<u64>,
    },
    #[cfg(test)]
    StopSample {
        id: usize,
    },
    StopSampleForPlacement {
        placement: super::content_placement::SlotSnapshot,
    },
    StopAll,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScheduledEvent {
    pub(crate) target_frame: u64,
    pub(crate) sequence: u64,
    order_sequence: u64,
    pub(crate) command: ScheduledCommand,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DueEvent {
    pub(crate) target_frame: u64,
    pub(crate) execution_frame: u64,
    pub(crate) was_late: bool,
    pub(crate) command: ScheduledCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScheduleError {
    Full,
}

pub(crate) struct FixedCapacityScheduler<const CAPACITY: usize> {
    // Allocate bounded storage once on the control/startup thread. Larger guarded
    // events must not put several copies of this array on Python's debug stack.
    events: Box<[Option<ScheduledEvent>]>,
    len: usize,
    next_sequence: u64,
    // Allocate this single bounded slot at startup too, keeping the scheduler handle
    // small on the Windows stack. RT only moves the already popped event into it.
    deferred_due: Box<Option<DueEvent>>,
    deferred_at_callback: u64,
    // Capacity for this live session's later stop, without a fictitious event.
    // It never contributes to event length, ordering or the scheduler-idle view.
    capture_stop_owner: Option<u64>,
}

impl<const CAPACITY: usize> FixedCapacityScheduler<CAPACITY> {
    pub(crate) fn new() -> Self {
        Self {
            events: vec![None; CAPACITY].into_boxed_slice(),
            len: 0,
            next_sequence: 0,
            deferred_due: Box::new(None),
            deferred_at_callback: 0,
            capture_stop_owner: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        CAPACITY
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.len + usize::from(self.deferred_due.is_some())
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0 && self.deferred_due.is_none()
    }

    pub(crate) fn is_full(&self) -> bool {
        self.len
            .saturating_add(usize::from(self.capture_stop_owner.is_some()))
            >= CAPACITY
            || self.deferred_due.is_some()
    }

    pub(crate) fn deferred_due(&self) -> Option<&DueEvent> {
        self.deferred_due.as_ref().as_ref()
    }

    pub(crate) fn defer_due(&mut self, event: DueEvent, callback_frame: u64) {
        debug_assert!(self.deferred_due.is_none());
        *self.deferred_due = Some(event);
        self.deferred_at_callback = callback_frame;
    }

    pub(crate) fn schedule(
        &mut self,
        target_frame: u64,
        command: ScheduledCommand,
    ) -> Result<ScheduledEvent, ScheduleError> {
        match command {
            ScheduledCommand::CaptureStart { session_id } => {
                return self.schedule_capture_start(session_id, target_frame);
            }
            ScheduledCommand::CaptureStop { session_id } => {
                return self.schedule_capture_stop(session_id, target_frame);
            }
            _ => {}
        }
        if self.is_full() {
            return Err(ScheduleError::Full);
        }

        self.insert_event(target_frame, command, reserve_input_order)
    }

    /// Admit one real start and reserve its same-session stop atomically.
    /// The reservation alone never makes an otherwise empty scheduler non-idle.
    pub(crate) fn schedule_capture_start(
        &mut self,
        session_id: u64,
        target_frame: u64,
    ) -> Result<ScheduledEvent, ScheduleError> {
        self.schedule_capture_start_with_order(session_id, target_frame, reserve_input_order)
    }

    /// Admit the existing group action and reserve its same-session stop atomically.
    /// The action retains its original input order, including after a later replan.
    pub(crate) fn schedule_capture_group_start(
        &mut self,
        session_id: u64,
        target_frame: u64,
        slot: usize,
        action_sequence: u64,
    ) -> Result<ScheduledEvent, ScheduleError> {
        self.schedule_reserved_capture_start(
            session_id,
            target_frame,
            ScheduledCommand::PitchAction {
                slot,
                action_sequence,
            },
            reserve_input_order,
        )
    }

    fn schedule_capture_start_with_order(
        &mut self,
        session_id: u64,
        target_frame: u64,
        reserve_order: impl FnOnce() -> Option<u64>,
    ) -> Result<ScheduledEvent, ScheduleError> {
        self.schedule_reserved_capture_start(
            session_id,
            target_frame,
            ScheduledCommand::CaptureStart { session_id },
            reserve_order,
        )
    }

    fn schedule_reserved_capture_start(
        &mut self,
        session_id: u64,
        target_frame: u64,
        command: ScheduledCommand,
        reserve_order: impl FnOnce() -> Option<u64>,
    ) -> Result<ScheduledEvent, ScheduleError> {
        if session_id == 0
            || self.capture_stop_owner.is_some()
            || self.deferred_due.is_some()
            || CAPACITY.saturating_sub(self.len) < 2
            || self.events[..self.len]
                .iter()
                .flatten()
                .any(|event| capture_session(&event.command).is_some())
        {
            return Err(ScheduleError::Full);
        }
        let event = self.insert_event(target_frame, command, reserve_order)?;
        self.capture_stop_owner = Some(session_id);
        Ok(event)
    }

    /// Consume only this session's reserved physical slot after insertion succeeds.
    /// An identical queued stop retains its original order/sequence and endpoint.
    pub(crate) fn schedule_capture_stop(
        &mut self,
        session_id: u64,
        target_frame: u64,
    ) -> Result<ScheduledEvent, ScheduleError> {
        self.schedule_capture_stop_with_order(session_id, target_frame, reserve_input_order)
    }

    /// Observe only an actually admitted stop, never its logical reservation.
    pub(crate) fn capture_stop_target_frame(&self, session_id: u64) -> Option<u64> {
        if session_id == 0 {
            return None;
        }
        if let Some(event) = self.deferred_due()
            && matches!(&event.command, ScheduledCommand::CaptureStop { session_id: owner } if *owner == session_id)
        {
            return Some(event.target_frame);
        }
        self.events[..self.len].iter().flatten().find_map(|event| {
            matches!(&event.command, ScheduledCommand::CaptureStop { session_id: owner } if *owner == session_id)
                .then_some(event.target_frame)
        })
    }

    fn schedule_capture_stop_with_order(
        &mut self,
        session_id: u64,
        target_frame: u64,
        reserve_order: impl FnOnce() -> Option<u64>,
    ) -> Result<ScheduledEvent, ScheduleError> {
        if session_id == 0 {
            return Err(ScheduleError::Full);
        }
        if let Some(event) = self.events[..self.len].iter().flatten().find(|event| {
            matches!(&event.command, ScheduledCommand::CaptureStop { session_id: owner } if *owner == session_id)
        }) {
            return if event.target_frame == target_frame {
                Ok(event.clone())
            } else {
                Err(ScheduleError::Full)
            };
        }
        if self.capture_stop_owner != Some(session_id)
            || self.deferred_due.is_some()
            || self.len >= CAPACITY
            || self.events[..self.len].iter().flatten().any(|event| {
                matches!(&event.command, ScheduledCommand::CaptureStart { session_id: owner } if *owner == session_id)
                    && target_frame < event.target_frame
            })
        {
            return Err(ScheduleError::Full);
        }
        let event = self.insert_event(
            target_frame,
            ScheduledCommand::CaptureStop { session_id },
            reserve_order,
        )?;
        self.capture_stop_owner = None;
        Ok(event)
    }

    /// Remove only this session's scalar capture commands and logical reservation.
    /// Returns removed queued/deferred event count; an active reservation may be
    /// released even when no event remains. Other payloads are moved, never dropped.
    pub(crate) fn cancel_capture_session(&mut self, session_id: u64) -> usize {
        if session_id == 0 {
            return 0;
        }
        let mut removed = 0;
        let mut kept = 0;
        for index in 0..self.len {
            let event = self.events[index]
                .take()
                .expect("scheduled storage is dense");
            if capture_session(&event.command) == Some(session_id) {
                removed += 1;
            } else {
                self.events[kept] = Some(event);
                kept += 1;
            }
        }
        self.len = kept;
        if self
            .deferred_due()
            .is_some_and(|event| capture_session(&event.command) == Some(session_id))
        {
            drop(self.deferred_due.take());
            self.deferred_at_callback = 0;
            removed += 1;
        }
        if self.capture_stop_owner == Some(session_id) {
            self.capture_stop_owner = None;
        }
        removed
    }

    fn insert_event(
        &mut self,
        target_frame: u64,
        command: ScheduledCommand,
        reserve_order: impl FnOnce() -> Option<u64>,
    ) -> Result<ScheduledEvent, ScheduleError> {
        debug_assert!(self.len < CAPACITY);
        let event = ScheduledEvent {
            target_frame,
            sequence: self.next_sequence,
            order_sequence: match &command {
                ScheduledCommand::PitchAction {
                    action_sequence, ..
                } => *action_sequence,
                _ => reserve_order().ok_or(ScheduleError::Full)?,
            },
            command,
        };
        let insert_at = self.insertion_index(&event);

        let mut index = self.len;
        while index > insert_at {
            self.events[index] = self.events[index - 1].take();
            index -= 1;
        }

        self.events[insert_at] = Some(event.clone());
        self.len += 1;
        self.next_sequence = self.next_sequence.saturating_add(1);

        Ok(event)
    }

    pub(crate) fn peek_next_target_frame(&self) -> Option<u64> {
        if let Some(event) = self.deferred_due.as_ref() {
            return Some(event.target_frame);
        }
        self.events
            .first()
            .and_then(|event| event.as_ref().map(|event| event.target_frame))
    }

    pub(crate) fn pop_due_at_callback_start(
        &mut self,
        callback_start_frame: u64,
    ) -> Option<DueEvent> {
        self.pop_due_through(callback_start_frame, callback_start_frame)
    }

    pub(crate) fn pop_due_through(
        &mut self,
        callback_start_frame: u64,
        latest_frame: u64,
    ) -> Option<DueEvent> {
        if let Some(event) = self.deferred_due.as_ref() {
            if callback_start_frame <= self.deferred_at_callback
                || event.target_frame > latest_frame
            {
                return None;
            }
            let mut event = self.deferred_due.take()?;
            event.execution_frame = event.target_frame.max(callback_start_frame);
            event.was_late = event.target_frame < callback_start_frame;
            return Some(event);
        }
        let event = self.events.first()?.as_ref()?;
        if event.target_frame > latest_frame {
            return None;
        }

        self.pop_front().map(|event| DueEvent {
            target_frame: event.target_frame,
            execution_frame: event.target_frame.max(callback_start_frame),
            was_late: event.target_frame < callback_start_frame,
            command: event.command,
        })
    }

    fn insertion_index(&self, new_event: &ScheduledEvent) -> usize {
        let mut index = 0;
        while index < self.len {
            let Some(existing_event) = self.events[index].as_ref() else {
                break;
            };

            if event_sorts_before(new_event, existing_event) {
                break;
            }

            index += 1;
        }
        index
    }

    fn pop_front(&mut self) -> Option<ScheduledEvent> {
        if self.len == 0 {
            return None;
        }

        let event = self.events[0].take()?;

        let mut index = 1;
        while index < self.len {
            self.events[index - 1] = self.events[index].take();
            index += 1;
        }

        self.len -= 1;
        self.events[self.len] = None;

        Some(event)
    }
}

fn capture_session(command: &ScheduledCommand) -> Option<u64> {
    match command {
        ScheduledCommand::CaptureStart { session_id }
        | ScheduledCommand::CaptureStop { session_id } => Some(*session_id),
        _ => None,
    }
}

fn event_sorts_before(left: &ScheduledEvent, right: &ScheduledEvent) -> bool {
    left.target_frame < right.target_frame
        || (left.target_frame == right.target_frame && left.order_sequence < right.order_sequence)
}

#[cfg(test)]
#[path = "scheduler/atomic_capture_group_tests.rs"]
mod atomic_capture_group_tests;
#[cfg(test)]
#[path = "scheduler/capture_tests.rs"]
mod capture_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn play(id: usize) -> ScheduledCommand {
        ScheduledCommand::PlaySample {
            expected_source_generation: None,
            id,
            volume: 1.0,
            received_at_ns: None,
        }
    }

    fn drain_commands<const CAPACITY: usize>(
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        latest_frame: u64,
    ) -> Vec<ScheduledCommand> {
        let mut commands = Vec::new();
        while let Some(event) = scheduler.pop_due_through(0, latest_frame) {
            commands.push(event.command);
        }
        commands
    }

    #[test]
    fn scheduler_uses_named_capacity() {
        let scheduler = TransportScheduler::new();

        eprintln!(
            "scheduler layout: command={} slot={} capacity={} startup_bytes={}",
            std::mem::size_of::<ScheduledCommand>(),
            std::mem::size_of::<Option<ScheduledEvent>>(),
            MAX_SCHEDULED_EVENTS,
            std::mem::size_of::<Option<ScheduledEvent>>() * MAX_SCHEDULED_EVENTS,
        );

        assert_eq!(scheduler.capacity(), MAX_SCHEDULED_EVENTS);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn guarded_production_scheduler_initializes_on_normal_windows_python_stack() {
        // Native debug startup previously copied the larger fixed event array
        // several times on Python's 1 MiB thread stack. Storage belongs off RT.
        assert!(std::mem::size_of::<TransportScheduler>() < 128);
        std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(|| {
                let mut scheduler = TransportScheduler::new();
                let storage = scheduler.events.as_ptr();
                for frame in 0..MAX_SCHEDULED_EVENTS {
                    scheduler.schedule(frame as u64, play(0)).unwrap();
                }
                assert_eq!(scheduler.schedule(0, play(0)), Err(ScheduleError::Full));
                for frame in 0..MAX_SCHEDULED_EVENTS {
                    assert_eq!(
                        scheduler.pop_due_through(0, u64::MAX).unwrap().target_frame,
                        frame as u64
                    );
                }
                assert_eq!(scheduler.events.as_ptr(), storage);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn events_are_drained_by_target_frame_order() {
        let mut scheduler = FixedCapacityScheduler::<8>::new();

        scheduler.schedule(30, play(3)).unwrap();
        scheduler.schedule(10, play(1)).unwrap();
        scheduler.schedule(20, play(2)).unwrap();

        assert_eq!(
            drain_commands(&mut scheduler, 30),
            vec![play(1), play(2), play(3)]
        );
        assert!(scheduler.is_empty());
    }

    #[test]
    fn equal_frame_pitch_and_stop_use_one_transitive_capture_order() {
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let first = reserve_input_order().unwrap();
        scheduler.schedule(10, ScheduledCommand::StopAll).unwrap();
        let second = reserve_input_order().unwrap();
        let first_pitch = ScheduledCommand::PitchAction {
            slot: 0,
            action_sequence: first,
        };
        let second_pitch = ScheduledCommand::PitchAction {
            slot: 1,
            action_sequence: second,
        };
        scheduler.schedule(10, second_pitch.clone()).unwrap();
        scheduler.schedule(10, first_pitch.clone()).unwrap();
        assert_eq!(
            drain_commands(&mut scheduler, 10),
            vec![first_pitch, ScheduledCommand::StopAll, second_pitch]
        );
    }

    #[test]
    fn same_frame_events_are_stable_by_insertion_order() {
        let mut scheduler = FixedCapacityScheduler::<8>::new();

        scheduler.schedule(10, play(1)).unwrap();
        scheduler
            .schedule(10, ScheduledCommand::StopSample { id: 2 })
            .unwrap();
        scheduler.schedule(10, ScheduledCommand::StopAll).unwrap();
        scheduler
            .schedule(
                10,
                ScheduledCommand::StopAllThenPlaySample {
                    expected_source_generation: None,
                    id: 3,
                    volume: 1.0,
                    received_at_ns: None,
                },
            )
            .unwrap();

        assert_eq!(
            drain_commands(&mut scheduler, 10),
            vec![
                play(1),
                ScheduledCommand::StopSample { id: 2 },
                ScheduledCommand::StopAll,
                ScheduledCommand::StopAllThenPlaySample {
                    expected_source_generation: None,
                    id: 3,
                    volume: 1.0,
                    received_at_ns: None
                },
            ]
        );
    }

    #[test]
    fn future_events_remain_scheduled_until_due() {
        let mut scheduler = FixedCapacityScheduler::<4>::new();

        scheduler.schedule(100, play(1)).unwrap();

        assert_eq!(scheduler.pop_due_through(0, 99), None);
        assert_eq!(scheduler.len(), 1);
        assert_eq!(scheduler.peek_next_target_frame(), Some(100));
    }

    #[test]
    fn callback_start_drains_events_due_at_or_before_start() {
        let mut scheduler = FixedCapacityScheduler::<4>::new();

        scheduler.schedule(90, play(1)).unwrap();
        scheduler.schedule(100, play(2)).unwrap();
        scheduler.schedule(101, play(3)).unwrap();

        let late = scheduler.pop_due_at_callback_start(100).unwrap();
        assert_eq!(late.target_frame, 90);
        assert_eq!(late.execution_frame, 100);
        assert!(late.was_late);

        let on_time = scheduler.pop_due_at_callback_start(100).unwrap();
        assert_eq!(on_time.target_frame, 100);
        assert_eq!(on_time.execution_frame, 100);
        assert!(!on_time.was_late);

        assert_eq!(scheduler.pop_due_at_callback_start(100), None);
        assert_eq!(scheduler.peek_next_target_frame(), Some(101));
    }

    #[test]
    fn event_inside_buffer_executes_at_target_frame() {
        let mut scheduler = FixedCapacityScheduler::<4>::new();

        scheduler.schedule(128, play(1)).unwrap();

        let due = scheduler.pop_due_through(100, 200).unwrap();

        assert_eq!(due.target_frame, 128);
        assert_eq!(due.execution_frame, 128);
        assert!(!due.was_late);
    }

    #[test]
    fn full_scheduler_rejects_new_event_without_eviction() {
        let mut scheduler = FixedCapacityScheduler::<2>::new();

        let first = scheduler.schedule(10, play(1)).unwrap();
        let second = scheduler.schedule(20, play(2)).unwrap();
        let rejected = scheduler.schedule(15, play(3));

        assert_eq!(rejected, Err(ScheduleError::Full));
        assert!(scheduler.is_full());
        assert_eq!(scheduler.len(), 2);

        let first_due = scheduler.pop_due_through(0, 20).unwrap();
        let second_due = scheduler.pop_due_through(0, 20).unwrap();

        assert_eq!(first_due.command, first.command);
        assert_eq!(second_due.command, second.command);
        assert_eq!(scheduler.pop_due_through(0, 20), None);
    }

    #[test]
    fn zero_capacity_scheduler_rejects_without_panic() {
        let mut scheduler = FixedCapacityScheduler::<0>::new();

        assert_eq!(scheduler.schedule(10, play(1)), Err(ScheduleError::Full));
        assert_eq!(scheduler.pop_due_through(0, 10), None);
    }
}
