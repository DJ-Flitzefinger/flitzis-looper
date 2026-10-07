//! Unique Rubber Band state preparation and bounded realtime ownership exchange.
//!
//! Every voice has an initial warmed handle, a warmed reserve, and a source-only native/adapter
//! reserve. Callback exchanges use bounded SPSC lanes; construction, reset, source catch-up,
//! warming and owner destruction happen outside rendering.

use super::prepared_native_history::{
    NativeAdapterState, NativeHistoryRequest, PREPARED_HISTORY_FRAMES,
};
use super::productive_source_history::ProductiveSourceBinding;
use super::stretch_processor::ProductiveSourceFeed;
use crate::audio_engine::constants::MAX_VOICES;
use crate::audio_engine::rubberband_backend::{RubberBandError, RubberBandLiveShifter};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
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
    requests: Producer<NativeHistoryRequest>,
    source_ready: Consumer<Box<NativeAdapterState>>,
    source_recycle: Producer<Box<NativeAdapterState>>,
    source_owner_recycle: Producer<NativeHistoryRequest>,
    pending_request: Box<Option<NativeHistoryRequest>>,
    pending_source_return: Option<Box<NativeAdapterState>>,
    source_epoch: Arc<AtomicU64>,
    completed_request: Arc<AtomicU64>,
    worker_failed: Arc<AtomicBool>,
}

pub(crate) enum SourceExchange {
    Pending,
    Adopted(u64),
    Rejected(u64),
}

impl KeyLockPreparationLane {
    pub(crate) fn invalidate_source(&mut self, epoch: u64) {
        self.source_epoch.store(epoch, Ordering::Release);
        self.retire_prepared();
    }

    pub(crate) fn source_work_finished(&self, request_id: u64) -> bool {
        self.completed_request.load(Ordering::Acquire) >= request_id
    }

    /// Also polled for inactive/paused voices by the callback owner. A worker result published
    /// immediately after cancellation is then retired without requiring another source render.
    pub(crate) fn retire_prepared(&mut self) {
        if let Some(pending) = self.pending_source_return.take()
            && let Err(PushError::Full(pending)) = self.source_recycle.push(pending)
        {
            self.pending_source_return = Some(pending);
            return;
        }
        if self.source_recycle.slots() == 0 {
            return;
        }
        if let Ok(state) = self.source_ready.pop() {
            if let Some(source) = &state.source {
                self.completed_request
                    .store(source.request_id, Ordering::Release);
            }
            if let Err(PushError::Full(state)) = self.source_recycle.push(state) {
                self.pending_source_return = Some(state);
            }
        }
    }
    /// A reset may release only the source pins while retaining dirty native state. Two slots cover
    /// the active and ready source owners; preparing another source requires worker progression.
    pub(crate) fn retire_source_owner(&mut self, source: &mut Option<NativeHistoryRequest>) {
        if self.source_owner_recycle.slots() == 0 {
            return;
        }
        if let Some(owner) = source.take()
            && let Err(PushError::Full(owner)) = self.source_owner_recycle.push(owner)
        {
            *source = Some(owner);
        }
    }
    /// Reserve request capacity before cloning source/permit pins. Every constructed owner either
    /// enters the worker queue or remains retained here for off-callback teardown.
    pub(crate) fn request_source(
        &mut self,
        feed: &ProductiveSourceFeed<'_>,
        epoch: u64,
        request_id: u64,
    ) -> bool {
        if self.worker_failed.load(Ordering::Acquire)
            || self.pending_request.is_some()
            || self.requests.slots() == 0
        {
            return false;
        }
        let (Some(permit), Some(output_frame)) = (feed.permit, feed.output_frame) else {
            return false;
        };
        let Some(target_output_frame) = output_frame.checked_add(PREPARED_HISTORY_FRAMES as u64)
        else {
            return false;
        };
        if !permit.current(feed.sample, feed.sample_rate_hz)
            || !permit.matches_projection(feed.accepted)
            || feed.plan.transition.is_active()
        {
            return false;
        }
        let request = NativeHistoryRequest {
            sample: feed.sample.clone(),
            stems: feed.stems.cloned(),
            permit: permit.clone(),
            binding: ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted),
            playback: *feed.playback,
            plan: feed.plan,
            target_output_frame,
            request_id,
            epoch,
        };
        if let Err(PushError::Full(request)) = self.requests.push(request) {
            *self.pending_request = Some(request);
        }
        true
    }

    pub(crate) fn chunk_until_prepared_adoption(
        &mut self,
        output_frame: u64,
        max_frames: usize,
    ) -> usize {
        if let Ok(ready) = self.source_ready.peek()
            && let Some(source) = &ready.source
            && source.target_output_frame > output_frame
        {
            return max_frames
                .min((source.target_output_frame - output_frame).min(usize::MAX as u64) as usize);
        }
        max_frames
    }

    #[cfg(test)]
    pub(crate) fn prepared_state(&mut self) -> Option<&NativeAdapterState> {
        self.source_ready.peek().ok().map(Box::as_ref)
    }

    /// Retain all ownership when retirement capacity is unavailable. Late or mismatched results
    /// move intact to the worker; they never clear or replace the productive current adapter.
    pub(crate) fn exchange_source(
        &mut self,
        current: &mut Box<NativeAdapterState>,
        feed: &ProductiveSourceFeed<'_>,
        epoch: u64,
        expected_request_id: u64,
    ) -> SourceExchange {
        if let Some(request) = self.pending_request.take()
            && let Err(PushError::Full(request)) = self.requests.push(request)
        {
            *self.pending_request = Some(request);
        }
        if let Some(pending) = self.pending_source_return.take()
            && let Err(PushError::Full(pending)) = self.source_recycle.push(pending)
        {
            self.pending_source_return = Some(pending);
            return SourceExchange::Pending;
        }
        let Ok(ready) = self.source_ready.peek() else {
            return SourceExchange::Pending;
        };
        let Some(source) = &ready.source else {
            return SourceExchange::Pending;
        };
        let valid_contract = source.matches_contract(feed, epoch);
        if valid_contract
            && feed
                .output_frame
                .is_some_and(|frame| frame < source.target_output_frame)
        {
            return SourceExchange::Pending;
        }
        if self.source_recycle.slots() == 0 {
            return SourceExchange::Pending;
        }
        let request_id = source.request_id;
        let adopt = !self.worker_failed.load(Ordering::Acquire)
            && ready.active
            && ready.used
            && !ready.dirty
            && source.matches_adoption(feed, epoch, expected_request_id);
        let mut candidate = self
            .source_ready
            .pop()
            .expect("peeked source candidate remains in single-consumer queue");
        if adopt {
            std::mem::swap(current, &mut candidate);
        }
        if let Err(PushError::Full(candidate)) = self.source_recycle.push(candidate) {
            self.pending_source_return = Some(candidate);
        }
        if adopt {
            SourceExchange::Adopted(request_id)
        } else {
            SourceExchange::Rejected(request_id)
        }
    }
    #[cfg(test)]
    pub(crate) fn fail_worker(&self) {
        self.worker_failed.store(true, Ordering::Release);
    }

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
    requests: Consumer<NativeHistoryRequest>,
    source_ready: Producer<Box<NativeAdapterState>>,
    source_recycle: Consumer<Box<NativeAdapterState>>,
    source_reserve: Option<Box<NativeAdapterState>>,
    source_owner_recycle: Consumer<NativeHistoryRequest>,
    source_epoch: Arc<AtomicU64>,
    completed_request: Arc<AtomicU64>,
}

impl WorkerLane {
    /// Preparation failure disables publication, but the worker remains an off-RT retirement
    /// owner until stream teardown. Source/native pins must still retire after stop or unload.
    fn drain_failed(&mut self) -> bool {
        let mut worked = false;
        if let Ok(owner) = self.source_owner_recycle.pop() {
            drop(owner);
            worked = true;
        }
        if let Ok(state) = self.source_recycle.pop() {
            drop(state);
            worked = true;
        }
        if let Ok(request) = self.requests.pop() {
            self.completed_request
                .store(request.request_id, Ordering::Release);
            drop(request);
            worked = true;
        }
        if let Ok(native) = self.recycle.pop() {
            drop(native);
            worked = true;
        }
        if self.pending_ready.is_some() || self.source_reserve.is_some() {
            drop(self.pending_ready.take());
            drop(self.source_reserve.take());
            worked = true;
        }
        worked
    }
    fn prepare_source(&mut self) -> Result<bool, RubberBandError> {
        let retired_owner = self.source_owner_recycle.pop().ok();
        let retired = retired_owner.is_some();
        drop(retired_owner);
        if self.source_reserve.is_none() {
            self.source_reserve = self.source_recycle.pop().ok();
            if let Some(state) = &mut self.source_reserve {
                // The callback transferred these pins intact; reclaim them only on this worker.
                state.source = None;
            }
        }
        if self.source_reserve.is_none() || self.source_ready.slots() == 0 {
            return Ok(retired);
        }
        let Ok(request) = self.requests.pop() else {
            return Ok(retired);
        };
        let request_id = request.request_id;
        let mut state = self
            .source_reserve
            .take()
            .expect("reserved source adapter exists");
        if request.epoch == self.source_epoch.load(Ordering::Acquire)
            && request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
            && request.permit.matches_projection(request.binding.accepted)
        {
            if let Err(error) = state.prepare_source(request) {
                // This consumed request will never appear in the ready lane. Publish completion
                // before worker-owned state/pins retire so the callback cannot retain its pending
                // request forever while failure-only retirement continues.
                self.completed_request.store(request_id, Ordering::Release);
                return Err(error);
            }
            if state.source.as_ref().is_some_and(|source| {
                source.epoch != self.source_epoch.load(Ordering::Acquire)
                    || !source
                        .permit
                        .current(&source.sample, source.binding.sample_rate_hz)
                    || !source.permit.matches_projection(source.binding.accepted)
            }) {
                state.source = None;
                self.source_reserve = Some(state);
                self.completed_request.store(request_id, Ordering::Release);
                return Ok(true);
            }
        } else {
            // No stale owner reaches the callback. All pins are released on this worker.
            drop(request);
            self.source_reserve = Some(state);
            self.completed_request.store(request_id, Ordering::Release);
            return Ok(true);
        }
        if let Err(PushError::Full(state)) = self.source_ready.push(state) {
            self.source_reserve = Some(state);
        }
        Ok(true)
    }
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
                        if worker_failed.load(Ordering::Acquire) {
                            worked |= lane.drain_failed();
                            continue;
                        }
                        match lane.prepare_source() {
                            Ok(prepared) => worked |= prepared,
                            Err(_) => {
                                worker_failed.store(true, Ordering::Release);
                                worked |= lane.drain_failed();
                                continue;
                            }
                        }
                        match lane.replenish() {
                            Ok(replenished) => worked |= replenished,
                            Err(_) => {
                                // The callback keeps its current state or deterministic fallback;
                                // failed preparation never publishes a partially warmed handle.
                                worker_failed.store(true, Ordering::Release);
                                worked |= lane.drain_failed();
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

/// Construct two warmed unique handles and one source-only native adapter per voice before
/// realtime rendering begins. The third handle's first shifts contain its requested real source.
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
        let (request_producer, request_consumer) = RingBuffer::new(LANE_CAPACITY);
        let (source_ready_producer, source_ready_consumer) = RingBuffer::new(LANE_CAPACITY);
        let (source_recycle_producer, source_recycle_consumer) = RingBuffer::new(LANE_CAPACITY);
        let (source_owner_recycle_producer, source_owner_recycle_consumer) = RingBuffer::new(2);
        let source_epoch = Arc::new(AtomicU64::new(0));
        let completed_request = Arc::new(AtomicU64::new(0));
        let source_reserve = Box::new(NativeAdapterState::new(
            Some(RubberBandLiveShifter::new(sample_rate_hz, channels)?),
            channels,
        ));
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
            requests: request_producer,
            source_ready: source_ready_consumer,
            source_recycle: source_recycle_producer,
            source_owner_recycle: source_owner_recycle_producer,
            pending_request: Box::new(None),
            pending_source_return: None,
            source_epoch: source_epoch.clone(),
            completed_request: completed_request.clone(),
            worker_failed: worker_failed.clone(),
        });
        worker_lanes.push(WorkerLane {
            ready: ready_producer,
            recycle: recycle_consumer,
            pending_ready: None,
            requests: request_consumer,
            source_ready: source_ready_producer,
            source_recycle: source_recycle_consumer,
            source_reserve: Some(source_reserve),
            source_owner_recycle: source_owner_recycle_consumer,
            source_epoch,
            completed_request,
        });
    }

    let worker = KeyLockPreparationWorker::spawn(worker_lanes, worker_failed)?;
    Ok((callback_lanes, worker))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::prepared_native_history::tests::fixture;
    use crate::audio_engine::source_reader::ExplicitSeekMode;
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

    #[test]
    fn full_native_bundle_recycle_lane_retains_current_candidate_fifo_and_source_pins() {
        let (mut lanes, worker) = create_key_lock_preparation(1, 48_000, 1).unwrap();
        drop(worker);
        let lane = &mut lanes[0];
        let mut current = Box::new(NativeAdapterState::new(lane.take_initial(), 1));
        let current_address = current.rubberband.as_ref().unwrap().state_address();
        let (mut ready, ready_consumer) = RingBuffer::new(1);
        lane.source_ready = ready_consumer;
        let (recycle_producer, mut recycle) = RingBuffer::new(1);
        lane.source_recycle = recycle_producer;
        let request = fixture(1, 48_000, 1.37, ExplicitSeekMode::Normal, None);
        let mut candidate =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(48_000, 1).unwrap()), 1);
        candidate.prepare_source(request).unwrap();
        let candidate_address = candidate.rubberband.as_ref().unwrap().state_address();
        let source = candidate.source.as_ref().unwrap();
        let sample = source.sample.clone();
        let permit = source.permit.clone();
        let plan = source.plan;
        let playback = source.playback;
        let fifo = (
            candidate.input_fifo[0].len(),
            candidate.output_fifo[0].len(),
        );
        let pin_count = Arc::strong_count(&sample.samples);
        assert!(ready.push(Box::new(candidate)).is_ok());
        assert!(
            lane.source_recycle
                .push(Box::new(NativeAdapterState::new(
                    Some(RubberBandLiveShifter::new(48_000, 1).unwrap()),
                    1
                )))
                .is_ok()
        );
        let feed = ProductiveSourceFeed {
            sample: &sample,
            stems: None,
            sample_rate_hz: 48_000,
            accepted: None,
            plan,
            playback: &playback,
            permit: Some(&permit),
            output_frame: Some(PREPARED_HISTORY_FRAMES as u64),
        };
        assert!(matches!(
            lane.exchange_source(&mut current, &feed, 3, 1),
            SourceExchange::Pending
        ));
        assert_eq!(
            current.rubberband.as_ref().unwrap().state_address(),
            current_address
        );
        let retained = lane.prepared_state().unwrap();
        assert_eq!(
            retained.rubberband.as_ref().unwrap().state_address(),
            candidate_address
        );
        assert_eq!(
            (retained.input_fifo[0].len(), retained.output_fifo[0].len()),
            fifo
        );
        assert_eq!(Arc::strong_count(&sample.samples), pin_count);
        drop(recycle.pop().unwrap());
        assert!(matches!(
            lane.exchange_source(&mut current, &feed, 3, 1),
            SourceExchange::Adopted(1)
        ));
        assert_eq!(
            current.rubberband.as_ref().unwrap().state_address(),
            candidate_address
        );
        assert_eq!(
            recycle
                .pop()
                .unwrap()
                .rubberband
                .as_ref()
                .unwrap()
                .state_address(),
            current_address
        );
    }

    #[test]
    fn cancelled_ready_and_inflight_source_pins_retire_without_another_source_render() {
        for (wait_until_ready, fail_worker) in [(false, false), (true, false), (true, true)] {
            let (mut lanes, worker) = create_key_lock_preparation(1, 48_000, 1).unwrap();
            let lane = &mut lanes[0];
            let mut request = fixture(1, 48_000, 1.37, ExplicitSeekMode::Normal, None);
            request.epoch = 0;
            let weak = Arc::downgrade(&request.sample.samples);
            let feed = ProductiveSourceFeed {
                sample: &request.sample,
                stems: None,
                sample_rate_hz: 48_000,
                accepted: None,
                plan: request.plan,
                playback: &request.playback,
                permit: Some(&request.permit),
                output_frame: Some(0),
            };
            assert!(lane.request_source(&feed, 0, 1));
            drop(request);
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            if wait_until_ready {
                while lane.prepared_state().is_none() && std::time::Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(1));
                }
                assert!(lane.prepared_state().is_some());
            }
            if fail_worker {
                lane.fail_worker();
            }
            lane.invalidate_source(1);
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            // Pin destruction and request completion are separate worker effects. Observing
            // the last Arc disappear cannot synchronize the subsequent completion store.
            while (weak.upgrade().is_some() || !lane.source_work_finished(1))
                && std::time::Instant::now() < deadline
            {
                // Equivalent to the mixer polling inactive lanes. Never invokes process_source.
                lane.retire_prepared();
                thread::sleep(Duration::from_millis(1));
            }
            assert!(
                weak.upgrade().is_none(),
                "cancelled source pin must be dropped by worker"
            );
            assert!(lane.source_work_finished(1));
            drop(lanes);
            drop(worker);
        }

        // Exercise an actual backend error after consuming the request, independently of the
        // failure latch. A two-channel adapter with a one-channel native handle rejects its first
        // real source-content shift through the backend's ordinary buffer validation.
        let (ready, _ready_consumer) = RingBuffer::new(1);
        let (_recycle_producer, recycle) = RingBuffer::new(1);
        let (mut requests, request_consumer) = RingBuffer::new(1);
        let (source_ready, _source_ready_consumer) = RingBuffer::new(1);
        let (_source_recycle_producer, source_recycle) = RingBuffer::new(1);
        let (_owner_producer, source_owner_recycle) = RingBuffer::new(2);
        let completed_request = Arc::new(AtomicU64::new(0));
        let mut lane = WorkerLane {
            ready,
            recycle,
            pending_ready: None,
            requests: request_consumer,
            source_ready,
            source_recycle,
            source_reserve: Some(Box::new(NativeAdapterState::new(
                Some(RubberBandLiveShifter::new(48_000, 1).unwrap()),
                2,
            ))),
            source_owner_recycle,
            source_epoch: Arc::new(AtomicU64::new(0)),
            completed_request: completed_request.clone(),
        };
        let mut request = fixture(2, 48_000, 1.37, ExplicitSeekMode::Normal, None);
        request.epoch = 0;
        request.request_id = 17;
        let weak = Arc::downgrade(&request.sample.samples);
        assert!(requests.push(request).is_ok());
        assert!(matches!(
            lane.prepare_source(),
            Err(RubberBandError::BufferChannelCount {
                expected: 1,
                actual: 2
            })
        ));
        assert_eq!(completed_request.load(Ordering::Acquire), 17);
        assert!(weak.upgrade().is_none());
        assert!(lane.source_reserve.is_none());
    }
}
