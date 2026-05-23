//! Fixed-capacity absolute output-frame scheduler.
//!
//! The scheduler owns bounded storage so later audio-callback integration can
//! accept or reject quantized events without heap allocation, blocking, or
//! eviction of previously accepted events.

#![allow(dead_code)]

use crate::audio_engine::constants::MAX_SCHEDULED_EVENTS;

pub(crate) type TransportScheduler = FixedCapacityScheduler<MAX_SCHEDULED_EVENTS>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ScheduledCommand {
    PlaySample { id: usize, volume: f32 },
    StopSample { id: usize },
    StopAll,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScheduledEvent {
    pub(crate) target_frame: u64,
    pub(crate) sequence: u64,
    pub(crate) command: ScheduledCommand,
}

#[derive(Debug, Clone, Copy, PartialEq)]
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
    events: [Option<ScheduledEvent>; CAPACITY],
    len: usize,
    next_sequence: u64,
}

impl<const CAPACITY: usize> FixedCapacityScheduler<CAPACITY> {
    pub(crate) fn new() -> Self {
        Self {
            events: [None; CAPACITY],
            len: 0,
            next_sequence: 0,
        }
    }

    pub(crate) fn capacity(&self) -> usize {
        CAPACITY
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn is_full(&self) -> bool {
        self.len == CAPACITY
    }

    pub(crate) fn schedule(
        &mut self,
        target_frame: u64,
        command: ScheduledCommand,
    ) -> Result<ScheduledEvent, ScheduleError> {
        if self.is_full() {
            return Err(ScheduleError::Full);
        }

        let event = ScheduledEvent {
            target_frame,
            sequence: self.next_sequence,
            command,
        };
        let insert_at = self.insertion_index(event);

        let mut index = self.len;
        while index > insert_at {
            self.events[index] = self.events[index - 1];
            index -= 1;
        }

        self.events[insert_at] = Some(event);
        self.len += 1;
        self.next_sequence = self.next_sequence.saturating_add(1);

        Ok(event)
    }

    pub(crate) fn peek_next_target_frame(&self) -> Option<u64> {
        self.events.first().and_then(|event| event.map(|event| event.target_frame))
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
        let event = self.events.first().copied().flatten()?;
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

    fn insertion_index(&self, new_event: ScheduledEvent) -> usize {
        let mut index = 0;
        while index < self.len {
            let Some(existing_event) = self.events[index] else {
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

        let event = self.events[0]?;

        let mut index = 1;
        while index < self.len {
            self.events[index - 1] = self.events[index];
            index += 1;
        }

        self.len -= 1;
        self.events[self.len] = None;

        Some(event)
    }
}

fn event_sorts_before(left: ScheduledEvent, right: ScheduledEvent) -> bool {
    left.target_frame < right.target_frame
        || (left.target_frame == right.target_frame && left.sequence < right.sequence)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(id: usize) -> ScheduledCommand {
        ScheduledCommand::PlaySample { id, volume: 1.0 }
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

        assert_eq!(scheduler.capacity(), MAX_SCHEDULED_EVENTS);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn events_are_drained_by_target_frame_order() {
        let mut scheduler = FixedCapacityScheduler::<8>::new();

        scheduler.schedule(30, play(3)).unwrap();
        scheduler.schedule(10, play(1)).unwrap();
        scheduler.schedule(20, play(2)).unwrap();

        assert_eq!(drain_commands(&mut scheduler, 30), vec![play(1), play(2), play(3)]);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn same_frame_events_are_stable_by_insertion_order() {
        let mut scheduler = FixedCapacityScheduler::<8>::new();

        scheduler.schedule(10, play(1)).unwrap();
        scheduler.schedule(10, ScheduledCommand::StopSample { id: 2 }).unwrap();
        scheduler.schedule(10, ScheduledCommand::StopAll).unwrap();

        assert_eq!(
            drain_commands(&mut scheduler, 10),
            vec![
                play(1),
                ScheduledCommand::StopSample { id: 2 },
                ScheduledCommand::StopAll,
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
