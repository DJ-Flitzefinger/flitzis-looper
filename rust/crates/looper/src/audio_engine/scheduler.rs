//! Fixed-capacity absolute output-frame scheduler.
//!
//! The scheduler owns bounded storage so later audio-callback integration can
//! accept or reject quantized events without heap allocation, blocking, or
//! eviction of previously accepted events.

use crate::audio_engine::constants::MAX_SCHEDULED_EVENTS;
use crate::audio_engine::input_runtime_binding::InputPadBinding;

pub(crate) type TransportScheduler = FixedCapacityScheduler<MAX_SCHEDULED_EVENTS>;

#[derive(Debug, Clone, PartialEq)]
// Compact inline launch guards measure 264 bytes/command on Windows x64; the
// 1024-slot pool (280 bytes/slot) is allocated once during control-side startup.
// Exact source/timing bindings must stay inline when RT schedules or rejects an
// event. Boxing them there would allocate/free on RT; this exception is limited
// to the generic variant-size heuristic, with fixed storage and bounded copies.
#[allow(clippy::large_enum_variant)]
pub(crate) enum ScheduledCommand {
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
    },
    StopAllThenPlaySample {
        id: usize,
        volume: f32,
        received_at_ns: Option<u64>,
    },
    StopSample {
        id: usize,
    },
    StopAll,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScheduledEvent {
    pub(crate) target_frame: u64,
    pub(crate) sequence: u64,
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
}

impl<const CAPACITY: usize> FixedCapacityScheduler<CAPACITY> {
    pub(crate) fn new() -> Self {
        Self {
            events: vec![None; CAPACITY].into_boxed_slice(),
            len: 0,
            next_sequence: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        CAPACITY
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    #[cfg(test)]
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

fn event_sorts_before(left: &ScheduledEvent, right: &ScheduledEvent) -> bool {
    left.target_frame < right.target_frame
        || (left.target_frame == right.target_frame && left.sequence < right.sequence)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(id: usize) -> ScheduledCommand {
        ScheduledCommand::PlaySample {
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
