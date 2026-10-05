//! Unique Rubber Band state preparation and bounded realtime ownership exchange.
//!
//! Every voice has an initial warmed handle and one ready reserve. The callback only moves
//! ownership through SPSC lanes; construction, reset, warming, and destruction happen outside it.

use crate::audio_engine::constants::MAX_VOICES;
use crate::audio_engine::rubberband_backend::{RubberBandError, RubberBandLiveShifter};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const LANE_CAPACITY: usize = 1;
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Debug, thiserror::Error)]
pub(crate) enum KeyLockPreparationError {
    #[error("Key Lock preparation requires between 1 and {MAX_VOICES} voices")]
    InvalidVoiceCount,
    #[error("Key Lock backend preparation failed: {0}")]
    Backend(#[from] RubberBandError),
    #[error("failed to start Key Lock preparation worker: {0}")]
    WorkerSpawn(#[source] std::io::Error),
}

/// Generic ownership lane keeps queue saturation testable without native DSP work.
struct PreparedStateLane<T> {
    initial: Option<T>,
    ready: Consumer<T>,
    recycle: Producer<T>,
    pending_return: Option<T>,
}

impl<T> PreparedStateLane<T> {
    fn take_initial(&mut self) -> Option<T> {
        self.initial.take()
    }

    /// No native calls, waits, allocations, or resource drops occur in this exchange.
    fn exchange(&mut self, current: &mut Option<T>) -> bool {
        if let Some(pending) = self.pending_return.take()
            && let Err(PushError::Full(pending)) = self.recycle.push(pending)
        {
            self.pending_return = Some(pending);
            return false;
        }

        // Reserve retirement capacity before taking the ready handle. The producer has only one
        // writer and the worker can only increase capacity, so this admission remains valid.
        if current.is_some() && self.recycle.slots() == 0 {
            return false;
        }
        let Ok(prepared) = self.ready.pop() else {
            return false;
        };

        if let Some(old) = current.replace(prepared)
            && let Err(PushError::Full(old)) = self.recycle.push(old)
        {
            // Defensive bounded ownership retention, even if the admission invariant regresses.
            self.pending_return = Some(old);
        }
        true
    }
}

pub(crate) struct KeyLockPreparationLane {
    state: PreparedStateLane<RubberBandLiveShifter>,
    worker_failed: Arc<AtomicBool>,
}

impl KeyLockPreparationLane {
    pub(crate) fn take_initial(&mut self) -> Option<RubberBandLiveShifter> {
        self.state.take_initial()
    }

    /// Exchange a used handle only when a warmed reserve and recycle capacity are both available.
    pub(crate) fn exchange(&mut self, current: &mut Option<RubberBandLiveShifter>) -> bool {
        if self.worker_failed.load(Ordering::Acquire) {
            return false;
        }
        self.state.exchange(current)
    }
}

struct WorkerLane {
    ready: Producer<RubberBandLiveShifter>,
    recycle: Consumer<RubberBandLiveShifter>,
    pending_ready: Option<RubberBandLiveShifter>,
}

impl WorkerLane {
    /// At most one recycled handle is warmed per scan, so another voice cannot be starved by a
    /// callback repeatedly invalidating one lane.
    fn replenish(&mut self) -> Result<bool, RubberBandError> {
        if let Some(prepared) = self.pending_ready.take() {
            return match self.ready.push(prepared) {
                Ok(()) => Ok(true),
                Err(PushError::Full(prepared)) => {
                    self.pending_ready = Some(prepared);
                    Ok(false)
                }
            };
        }

        let Ok(mut recycled) = self.recycle.pop() else {
            return Ok(false);
        };
        recycled.prepare_for_reuse()?;
        if let Err(PushError::Full(prepared)) = self.ready.push(recycled) {
            self.pending_ready = Some(prepared);
        }
        Ok(true)
    }
}

/// Owned by stream/control state. It must be dropped only after callback rendering has stopped.
pub(crate) struct KeyLockPreparationWorker {
    running: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl KeyLockPreparationWorker {
    fn spawn(
        mut lanes: Vec<WorkerLane>,
        worker_failed: Arc<AtomicBool>,
    ) -> Result<Self, KeyLockPreparationError> {
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let join_handle = thread::Builder::new()
            .name("flitzis-key-lock-preparation".to_owned())
            .spawn(move || {
                while thread_running.load(Ordering::Acquire) {
                    let mut worked = false;
                    for lane in &mut lanes {
                        match lane.replenish() {
                            Ok(replenished) => worked |= replenished,
                            Err(_) => {
                                // The callback keeps its current state or deterministic fallback;
                                // failed preparation never publishes a partially warmed handle.
                                worker_failed.store(true, Ordering::Release);
                                return;
                            }
                        }
                    }
                    if !worked {
                        thread::sleep(IDLE_POLL_INTERVAL);
                    }
                }
                // Recycled/pending handles are destroyed on this worker when its lanes drop.
            })
            .map_err(KeyLockPreparationError::WorkerSpawn)?;

        Ok(Self {
            running,
            join_handle: Some(join_handle),
        })
    }
}

impl Drop for KeyLockPreparationWorker {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

/// Construct two warmed unique backend handles per voice before realtime rendering begins.
pub(crate) fn create_key_lock_preparation(
    channels: usize,
    sample_rate_hz: u32,
    voice_count: usize,
) -> Result<(Vec<KeyLockPreparationLane>, KeyLockPreparationWorker), KeyLockPreparationError> {
    if voice_count == 0 || voice_count > MAX_VOICES {
        return Err(KeyLockPreparationError::InvalidVoiceCount);
    }

    let worker_failed = Arc::new(AtomicBool::new(false));
    let mut callback_lanes = Vec::with_capacity(voice_count);
    let mut worker_lanes = Vec::with_capacity(voice_count);

    for _ in 0..voice_count {
        let mut initial = RubberBandLiveShifter::new(sample_rate_hz, channels)?;
        initial.prepare_for_reuse()?;
        let mut reserve = RubberBandLiveShifter::new(sample_rate_hz, channels)?;
        reserve.prepare_for_reuse()?;

        let (mut ready_producer, ready_consumer) = RingBuffer::new(LANE_CAPACITY);
        let (recycle_producer, recycle_consumer) = RingBuffer::new(LANE_CAPACITY);
        // The brand-new bounded queue is empty and no consumer exists yet.
        if let Err(PushError::Full(reserve)) = ready_producer.push(reserve) {
            drop(reserve);
            unreachable!("new Key Lock reserve queue must accept its first handle");
        }

        callback_lanes.push(KeyLockPreparationLane {
            state: PreparedStateLane {
                initial: Some(initial),
                ready: ready_consumer,
                recycle: recycle_producer,
                pending_return: None,
            },
            worker_failed: worker_failed.clone(),
        });
        worker_lanes.push(WorkerLane {
            ready: ready_producer,
            recycle: recycle_consumer,
            pending_ready: None,
        });
    }

    let worker = KeyLockPreparationWorker::spawn(worker_lanes, worker_failed)?;
    Ok((callback_lanes, worker))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct DropTracked {
        id: usize,
        dropped: Arc<AtomicUsize>,
    }

    impl Drop for DropTracked {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn tracked(id: usize, dropped: &Arc<AtomicUsize>) -> DropTracked {
        DropTracked {
            id,
            dropped: dropped.clone(),
        }
    }

    fn test_lane<T>(
        initial: T,
        ready_capacity: usize,
        recycle_capacity: usize,
    ) -> (PreparedStateLane<T>, Producer<T>, Consumer<T>) {
        let (ready_producer, ready_consumer) = RingBuffer::new(ready_capacity);
        let (recycle_producer, recycle_consumer) = RingBuffer::new(recycle_capacity);
        (
            PreparedStateLane {
                initial: Some(initial),
                ready: ready_consumer,
                recycle: recycle_producer,
                pending_return: None,
            },
            ready_producer,
            recycle_consumer,
        )
    }

    #[test]
    fn exchange_transfers_old_ownership_without_dropping_it() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (mut lane, mut ready, mut recycle) = test_lane(tracked(1, &dropped), 1, 1);
        assert!(ready.push(tracked(2, &dropped)).is_ok());
        let mut current = lane.take_initial();

        assert!(lane.exchange(&mut current));
        assert_eq!(current.as_ref().unwrap().id, 2);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        let old = recycle.pop().unwrap();
        assert_eq!(old.id, 1);
        drop(old);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn empty_ready_queue_keeps_current_ownership() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (mut lane, _ready, recycle) = test_lane(tracked(1, &dropped), 1, 1);
        let mut current = lane.take_initial();

        assert!(!lane.exchange(&mut current));
        assert_eq!(current.as_ref().unwrap().id, 1);
        assert!(recycle.is_empty());
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn full_recycle_queue_preserves_ready_reserve_and_current_state() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (mut lane, mut ready, mut recycle) = test_lane(tracked(1, &dropped), 1, 1);
        assert!(ready.push(tracked(2, &dropped)).is_ok());
        assert!(lane.recycle.push(tracked(3, &dropped)).is_ok());
        let mut current = lane.take_initial();

        assert!(!lane.exchange(&mut current));
        assert_eq!(current.as_ref().unwrap().id, 1);
        assert_eq!(lane.ready.peek().unwrap().id, 2);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);

        drop(recycle.pop().unwrap());
        assert!(lane.exchange(&mut current));
        assert_eq!(current.as_ref().unwrap().id, 2);
        assert_eq!(recycle.pop().unwrap().id, 1);
    }

    #[test]
    fn retained_return_prevents_exchange_until_it_can_be_retired() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (mut lane, mut ready, _recycle) = test_lane(tracked(1, &dropped), 1, 0);
        lane.pending_return = Some(tracked(3, &dropped));
        assert!(ready.push(tracked(2, &dropped)).is_ok());
        let mut current = lane.take_initial();

        assert!(!lane.exchange(&mut current));
        assert_eq!(current.as_ref().unwrap().id, 1);
        assert_eq!(lane.pending_return.as_ref().unwrap().id, 3);
        assert_eq!(lane.ready.peek().unwrap().id, 2);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn native_worker_replenishes_unique_warmed_reserve() {
        let (mut lanes, worker) = create_key_lock_preparation(1, 48_000, 1).unwrap();
        let lane = &mut lanes[0];
        let mut current = lane.take_initial();
        assert!(lane.exchange(&mut current));

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while lane.state.ready.is_empty() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(lane.exchange(&mut current));
        assert_eq!(current.as_ref().unwrap().pitch_scale(), 1.0);
        assert!(!lane.worker_failed.load(Ordering::Acquire));
        drop(current);
        drop(lanes);
        drop(worker);
    }

    #[test]
    fn invalid_pool_dimensions_fail_before_starting_worker() {
        assert!(matches!(
            create_key_lock_preparation(1, 48_000, 0),
            Err(KeyLockPreparationError::InvalidVoiceCount)
        ));
        assert!(matches!(
            create_key_lock_preparation(1, 48_000, MAX_VOICES + 1),
            Err(KeyLockPreparationError::InvalidVoiceCount)
        ));
        assert!(matches!(
            create_key_lock_preparation(0, 48_000, 1),
            Err(KeyLockPreparationError::Backend(
                RubberBandError::InvalidChannelCount
            ))
        ));
    }
}
