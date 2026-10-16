//! Unique Rubber Band state preparation and bounded realtime ownership exchange.
//!
//! Every voice has an initial warmed handle, a warmed reserve, and a source-only native/adapter
//! reserve. Callback exchanges use bounded SPSC lanes; construction, reset, source catch-up,
//! warming and owner destruction happen outside rendering.

use super::prepared_native_history::{
    NativeAdapterState, NativeHistoryRequest, NativeSourcePreparationError,
    PREPARED_HISTORY_FRAMES, PitchSourceSnapshot,
};
use super::productive_source_history::ProductiveSourceBinding;
use super::source_feed_history::{SourceFeedContext, SourceFeedHistory, SourceFeedSnapshot};
use super::source_playback::SourcePlayback;
use super::source_reader::SourceReadPlan;
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

/// The owning voice supplies its genuine chronology and the original execution coordinate.
pub(crate) struct ProspectiveSourceHistory<'a> {
    pub(crate) history: &'a SourceFeedHistory,
    pub(crate) voice_lifetime: &'a Arc<AtomicU64>,
    pub(crate) voice_generation: u64,
    pub(crate) execution_frame: u64,
    pub(crate) paused: bool,
    pub(crate) source_feed_context: SourceFeedContext,
}

/// One reusable owner per source lane, allocated before rendering. Only the worker clears a
/// filled snapshot; the callback merely moves this owner through bounded queues.
struct SourceHistoryWork {
    snapshot: SourceFeedSnapshot,
    future: Option<(SourcePlayback, SourceReadPlan, u64, SourceFeedContext)>,
}

impl SourceHistoryWork {
    fn new() -> Self {
        Self {
            snapshot: SourceFeedSnapshot::new(),
            future: None,
        }
    }

    fn clear_for_reuse(&mut self) {
        self.snapshot.clear_for_reuse();
        self.future = None;
    }
}

enum SourcePreparationRequest {
    Ordinary(NativeHistoryRequest),
    WithHistory {
        request: Option<NativeHistoryRequest>,
        history: Box<SourceHistoryWork>,
        // Failed capture/queue admission still sends its owners to the worker, without
        // publishing an unaccepted candidate or advancing the callback's request state.
        prepare: bool,
    },
}

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
    requests: Producer<SourcePreparationRequest>,
    source_ready: Consumer<Box<NativeAdapterState>>,
    source_recycle: Producer<Box<NativeAdapterState>>,
    source_owner_recycle: Producer<NativeHistoryRequest>,
    pending_request: Box<Option<SourcePreparationRequest>>,
    pending_source_return: Option<Box<NativeAdapterState>>,
    source_history: Option<Box<SourceHistoryWork>>,
    source_history_return: Consumer<Box<SourceHistoryWork>>,
    source_epoch: Arc<AtomicU64>,
    completed_request: Arc<AtomicU64>,
    worker_failed: Arc<AtomicBool>,
    #[cfg(test)]
    reserve_withheld: bool,
}

pub(crate) enum SourceExchange {
    Pending,
    Adopted(u64),
    Rejected(u64),
}

impl KeyLockPreparationLane {
    pub(crate) fn source_result_pending(&self) -> bool {
        !self.source_ready.is_empty()
            || self.pending_source_return.is_some()
            || self.pending_request.is_some()
    }

    pub(crate) fn worker_failed(&self) -> bool {
        self.worker_failed.load(Ordering::Acquire)
    }

    pub(crate) fn warmed_reserve_available(&self) -> bool {
        #[cfg(test)]
        if self.reserve_withheld {
            return false;
        }
        !self.worker_failed.load(Ordering::Acquire)
            && self.state.pending_return.is_none()
            && !self.state.ready.is_empty()
            && self.state.recycle.slots() != 0
    }

    pub(crate) fn source_epoch_owner(&self) -> &Arc<AtomicU64> {
        &self.source_epoch
    }

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
        if !self.poll_source_recycling() {
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

    /// Service bounded returned work without consuming a held prospective candidate.
    pub(crate) fn poll_source_recycling(&mut self) -> bool {
        self.flush_pending_request();
        self.reclaim_source_history();
        if let Some(pending) = self.pending_source_return.take()
            && let Err(PushError::Full(pending)) = self.source_recycle.push(pending)
        {
            self.pending_source_return = Some(pending);
            return false;
        }
        self.source_recycle.slots() != 0
    }

    fn flush_pending_request(&mut self) {
        if let Some(request) = self.pending_request.take()
            && let Err(PushError::Full(request)) = self.requests.push(request)
        {
            *self.pending_request = Some(request);
        }
    }

    fn reclaim_source_history(&mut self) {
        if self.source_history.is_none() {
            self.source_history = self.source_history_return.pop().ok();
        }
    }

    /// Acquire the already allocated empty owner before a mixer starts holding a tuple.
    pub(crate) fn source_history_work_available(&mut self) -> bool {
        self.flush_pending_request();
        self.reclaim_source_history();
        !self.worker_failed()
            && self.pending_request.is_none()
            && self.requests.slots() != 0
            && self.source_history.is_some()
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
            // Keep the existing deferred preparation policy. The productive native adapter
            // consumes both finite selection sides until the actual source-frame ramp ends;
            // a later request catches up the settled selection without resetting that adapter.
            || feed.plan.transition.is_active()
        {
            return false;
        }
        let coverage = if super::native_source_coverage::complete_view_available(feed.sample) {
            None
        } else {
            let Some(coverage) = super::native_source_coverage::NormalLoopCoverage::capture(
                feed.sample,
                feed.stems,
                feed.sample_rate_hz,
                feed.accepted,
                feed.plan,
                feed.playback,
            ) else {
                return false;
            };
            Some(coverage)
        };
        let request = NativeHistoryRequest {
            pitch_intent: feed.pitch_intent,
            key_lock: feed.key_lock,
            sample: feed.sample.clone(),
            stems: feed.stems.cloned(),
            permit: permit.clone(),
            coverage,
            binding: ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted),
            playback: *feed.playback,
            plan: feed.plan,
            target_output_frame,
            request_id,
            epoch,
        };
        if let Err(PushError::Full(request)) = self
            .requests
            .push(SourcePreparationRequest::Ordinary(request))
        {
            *self.pending_request = Some(request);
        }
        true
    }

    /// Capture the actual pre-F history and project the same frozen F-to-original-E once.
    /// Every failure after taking the reusable owner sends it intact to worker recycling.
    pub(crate) fn request_source_with_history(
        &mut self,
        feed: &ProductiveSourceFeed<'_>,
        context: ProspectiveSourceHistory<'_>,
        epoch: u64,
        request_id: u64,
    ) -> bool {
        if !self.source_history_work_available() {
            return false;
        }
        let (Some(permit), Some(capture_frame)) = (feed.permit, feed.output_frame) else {
            return false;
        };
        let Some(delta) = context.execution_frame.checked_sub(capture_frame) else {
            return false;
        };
        let active_delta = if context.paused { 0 } else { delta };
        if !permit.current(feed.sample, feed.sample_rate_hz)
            || !permit.matches_projection(feed.accepted)
        {
            return false;
        }

        let mut history = self
            .source_history
            .take()
            .expect("preflighted reusable source-history owner");
        let request = if context.history.capture_into(&mut history.snapshot) {
            history.future = Some((
                *feed.playback,
                feed.plan,
                active_delta,
                context.source_feed_context,
            ));
            history
                .snapshot
                .guard(context.voice_lifetime, context.voice_generation)
                .and_then(|guard| {
                    let mut endpoint = PitchSourceSnapshot {
                        sample: feed.sample.clone(),
                        stems: feed.stems.or(feed.outgoing_stems).cloned(),
                        accepted: feed.accepted,
                        sample_rate_hz: feed.sample_rate_hz,
                        paused: context.paused,
                        playback: *feed.playback,
                        plan: feed.plan,
                        source_feed_context: context.source_feed_context,
                    };
                    if !endpoint.project_output_frames(delta) {
                        return None;
                    }
                    endpoint.into_history_request(
                        permit.clone().with_source_feed_guard(guard),
                        feed.pitch_intent,
                        feed.key_lock,
                        context.execution_frame,
                        request_id,
                        epoch,
                    )
                })
        } else {
            None
        };
        let accepted = request.is_some();
        let queued = SourcePreparationRequest::WithHistory {
            request,
            history,
            prepare: accepted,
        };
        match self.requests.push(queued) {
            Ok(()) => accepted,
            Err(PushError::Full(mut queued)) => {
                if let SourcePreparationRequest::WithHistory { prepare, .. } = &mut queued {
                    *prepare = false;
                }
                *self.pending_request = Some(queued);
                false
            }
        }
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
        self.flush_pending_request();
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

    /// Reserve the exact ready owner and its retirement slot before a multi-voice claim.
    pub(crate) fn source_matches(
        &mut self,
        feed: &ProductiveSourceFeed<'_>,
        epoch: u64,
        request_id: u64,
    ) -> bool {
        if let Some(pending) = self.pending_source_return.take()
            && let Err(PushError::Full(pending)) = self.source_recycle.push(pending)
        {
            self.pending_source_return = Some(pending);
            return false;
        }
        !self.worker_failed.load(Ordering::Acquire)
            && self.source_recycle.slots() != 0
            && self.source_ready.peek().is_ok_and(|candidate| {
                candidate.active
                    && candidate.used
                    && !candidate.dirty
                    && candidate
                        .source
                        .as_ref()
                        .is_some_and(|source| source.matches_adoption(feed, epoch, request_id))
            })
    }

    /// The callback has preflighted every participant and owns the enclosing effect claim.
    /// Only this consumer can remove ready owners; the worker can only add retirement capacity.
    pub(crate) fn exchange_source_claimed(&mut self, current: &mut Box<NativeAdapterState>) {
        let mut candidate = self.source_ready.pop().expect("preflighted source owner");
        std::mem::swap(current, &mut candidate);
        if let Err(PushError::Full(candidate)) = self.source_recycle.push(candidate) {
            self.pending_source_return = Some(candidate);
        }
    }
    #[cfg(test)]
    pub(crate) fn fail_worker(&self) {
        self.worker_failed.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn withhold_warmed_reserve(&mut self) {
        // Hold the actual ready owner instead of recycling it, so the worker cannot replenish
        // the lane. Test setup and final destruction run outside callback rendering.
        self.state.pending_return = self.state.ready.pop().ok();
        self.reserve_withheld = true;
    }

    pub(crate) fn take_initial(&mut self) -> Option<RubberBandLiveShifter> {
        self.state.take_initial()
    }

    /// Exchange a used handle only when a warmed reserve and recycle capacity are both available.
    pub(crate) fn exchange(&mut self, current: &mut Option<RubberBandLiveShifter>) -> bool {
        #[cfg(test)]
        if self.reserve_withheld {
            return false;
        }
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
    requests: Consumer<SourcePreparationRequest>,
    source_ready: Producer<Box<NativeAdapterState>>,
    source_recycle: Consumer<Box<NativeAdapterState>>,
    source_reserve: Option<Box<NativeAdapterState>>,
    source_owner_recycle: Consumer<NativeHistoryRequest>,
    source_history_return: Producer<Box<SourceHistoryWork>>,
    pending_history_return: Option<Box<SourceHistoryWork>>,
    source_epoch: Arc<AtomicU64>,
    completed_request: Arc<AtomicU64>,
}

impl WorkerLane {
    fn flush_history_return(&mut self) -> bool {
        if let Some(history) = self.pending_history_return.take()
            && let Err(PushError::Full(history)) = self.source_history_return.push(history)
        {
            self.pending_history_return = Some(history);
            return false;
        }
        true
    }

    fn return_source_history(&mut self, mut history: Box<SourceHistoryWork>) {
        history.clear_for_reuse();
        // Each lane has exactly one real history owner. Retain it if bounded return capacity
        // is unavailable rather than replacing or destroying it on the callback.
        debug_assert!(self.pending_history_return.is_none());
        if let Err(PushError::Full(history)) = self.source_history_return.push(history) {
            self.pending_history_return = Some(history);
        }
    }

    fn retire_request(&mut self, queued: SourcePreparationRequest) {
        let (request, history, accepted) = match queued {
            SourcePreparationRequest::Ordinary(request) => (Some(request), None, true),
            SourcePreparationRequest::WithHistory {
                request,
                history,
                prepare,
            } => (request, Some(history), prepare),
        };
        if let Some(request) = request {
            if accepted {
                self.completed_request
                    .store(request.request_id, Ordering::Release);
            }
            drop(request);
        }
        if let Some(history) = history {
            self.return_source_history(history);
        }
    }

    /// Preparation failure disables publication, but the worker remains an off-RT retirement
    /// owner until stream teardown. Source/native pins must still retire after stop or unload.
    fn drain_failed(&mut self) -> bool {
        let mut worked = false;
        let can_return_history = self.flush_history_return();
        if let Ok(owner) = self.source_owner_recycle.pop() {
            drop(owner);
            worked = true;
        }
        if let Ok(state) = self.source_recycle.pop() {
            drop(state);
            worked = true;
        }
        if can_return_history && let Ok(request) = self.requests.pop() {
            self.retire_request(request);
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
        if !self.flush_history_return() {
            return Ok(retired);
        }
        // Even a refused capture must return its filled owner while the Native adapter is
        // still in flight or its ready queue is occupied.
        if self.requests.peek().is_ok_and(|request| {
            matches!(
                request,
                SourcePreparationRequest::WithHistory { prepare: false, .. }
            )
        }) {
            let request = self.requests.pop().expect("peeked source cleanup request");
            self.retire_request(request);
            return Ok(true);
        }
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
        let Ok(queued) = self.requests.pop() else {
            return Ok(retired);
        };
        let (request, history) = match queued {
            SourcePreparationRequest::Ordinary(request) => (request, None),
            SourcePreparationRequest::WithHistory {
                request: Some(request),
                history,
                prepare: true,
            } => (request, Some(history)),
            queued => {
                self.retire_request(queued);
                return Ok(true);
            }
        };
        let request_id = request.request_id;
        let mut state = self
            .source_reserve
            .take()
            .expect("reserved source adapter exists");
        let result = if request.epoch == self.source_epoch.load(Ordering::Acquire)
            && request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
            && request.permit.matches_projection(request.binding.accepted)
        {
            if let Some(history) = &history {
                state
                    .prepare_source_with_feed_history(
                        request,
                        &history.snapshot,
                        history.future.expect("captured original F-to-E projection"),
                    )
                    .map_err(|error| match error {
                        RubberBandError::UnavailableSourceHistory => {
                            NativeSourcePreparationError::Unavailable(error)
                        }
                        _ => NativeSourcePreparationError::Backend(error),
                    })
                    .map(|()| true)
            } else {
                state.prepare_live_catchup(request).map(|()| true)
            }
        } else {
            // No stale owner reaches the callback. All pins are released on this worker.
            drop(request);
            Ok(false)
        };
        if let Some(history) = history {
            self.return_source_history(history);
        }
        match result {
            Ok(false) => {
                self.source_reserve = Some(state);
                self.completed_request.store(request_id, Ordering::Release);
                return Ok(true);
            }
            Ok(true) => {}
            Err(NativeSourcePreparationError::Unavailable(_)) => {
                // A missing source/history proof rejects only this request. Retain the
                // unique adapter for another request; no half-prepared owner is published.
                self.completed_request.store(request_id, Ordering::Release);
                state.source = None;
                state.reset_adapter();
                self.source_reserve = Some(state);
                return Ok(true);
            }
            Err(NativeSourcePreparationError::Backend(error)) => {
                // Backend/adapter faults still terminate the shared worker. The callback
                // must observe completion even though no ready owner can be published.
                self.completed_request.store(request_id, Ordering::Release);
                return Err(error);
            }
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

#[cfg(test)]
#[path = "pitch_catchup_tests.rs"]
mod pitch_catchup_tests;

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
        let (source_history_producer, source_history_consumer) = RingBuffer::new(LANE_CAPACITY);
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
            source_history: Some(Box::new(SourceHistoryWork::new())),
            source_history_return: source_history_consumer,
            source_epoch: source_epoch.clone(),
            completed_request: completed_request.clone(),
            worker_failed: worker_failed.clone(),
            #[cfg(test)]
            reserve_withheld: false,
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
            source_history_return: source_history_producer,
            pending_history_return: None,
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

    fn source_history_lanes(return_capacity: usize) -> (KeyLockPreparationLane, WorkerLane) {
        let (ready, ready_consumer) = RingBuffer::new(1);
        let (recycle_producer, recycle) = RingBuffer::new(1);
        let (requests, request_consumer) = RingBuffer::new(1);
        let (source_ready, source_ready_consumer) = RingBuffer::new(1);
        let (source_recycle_producer, source_recycle) = RingBuffer::new(1);
        let (owner_producer, source_owner_recycle) = RingBuffer::new(2);
        let (source_history_return, history_consumer) = RingBuffer::new(return_capacity);
        let source_epoch = Arc::new(AtomicU64::new(0));
        let completed_request = Arc::new(AtomicU64::new(0));
        (
            KeyLockPreparationLane {
                state: PreparedStateLane {
                    initial: None,
                    ready: ready_consumer,
                    recycle: recycle_producer,
                    pending_return: None,
                },
                requests,
                source_ready: source_ready_consumer,
                source_recycle: source_recycle_producer,
                source_owner_recycle: owner_producer,
                pending_request: Box::new(None),
                pending_source_return: None,
                source_history: Some(Box::new(SourceHistoryWork::new())),
                source_history_return: history_consumer,
                source_epoch: source_epoch.clone(),
                completed_request: completed_request.clone(),
                worker_failed: Arc::new(AtomicBool::new(false)),
                reserve_withheld: false,
            },
            WorkerLane {
                ready,
                recycle,
                pending_ready: None,
                requests: request_consumer,
                source_ready,
                source_recycle,
                source_reserve: Some(Box::new(NativeAdapterState::new(
                    Some(RubberBandLiveShifter::new(48_000, 1).unwrap()),
                    1,
                ))),
                source_owner_recycle,
                source_history_return,
                pending_history_return: None,
                source_epoch,
                completed_request,
            },
        )
    }

    fn prospective_feed(request: &NativeHistoryRequest) -> ProductiveSourceFeed<'_> {
        ProductiveSourceFeed {
            pitch_intent: super::super::pitch_intent::PitchIntent::new(3, 0).unwrap(),
            key_lock: false,
            sample: &request.sample,
            stems: request.stems.as_ref(),
            outgoing_stems: None,
            sample_rate_hz: 48_000,
            accepted: None,
            plan: request.plan,
            playback: &request.playback,
            permit: Some(&request.permit),
            output_frame: Some(100),
        }
    }

    fn history_context<'a>(
        history: &'a SourceFeedHistory,
        lifetime: &'a Arc<AtomicU64>,
        feed: &ProductiveSourceFeed<'_>,
        delta: u64,
    ) -> ProspectiveSourceHistory<'a> {
        ProspectiveSourceHistory {
            history,
            voice_lifetime: lifetime,
            voice_generation: 1,
            execution_frame: 100 + delta,
            paused: false,
            source_feed_context: SourceFeedContext::new(
                ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted),
                feed.plan
                    .required_read_sets(super::super::source_reader::StemReadSets::new(
                        feed.stems,
                        feed.outgoing_stems,
                    )),
            ),
        }
    }

    fn history_address(lane: &KeyLockPreparationLane) -> usize {
        lane.source_history.as_deref().unwrap() as *const SourceHistoryWork as usize
    }

    /// Opt-in full-voice scalar storage/capture characterization. It creates no Native state
    /// and supplies no PCM authority; its storage figures describe these fixed owner counts.
    #[test]
    #[ignore = "host-specific 32-voice mode snapshot resource profile; run explicitly with --nocapture"]
    fn prospective_mode_source_history_resource_profile() {
        use super::super::source_feed_history::{
            SOURCE_FEED_HISTORY_MAX_FRAMES, SOURCE_FEED_HISTORY_MAX_SPANS, SourceFeedContext,
            SourceFeedGuard, SourceFeedSpan,
        };
        use super::super::source_reader::{
            FrameRange, StemReadSets, StemRenderSelection, StemTransition,
        };
        use std::hint::black_box;
        use std::mem::size_of;
        use std::time::Instant;

        const SET_WORKS: usize = 64;
        const WARMUP_BATCHES: usize = 8;
        const MEASURED_BATCHES: usize = 128;
        let mut histories: Vec<_> = (0..MAX_VOICES).map(|_| SourceFeedHistory::new()).collect();
        let mut works: Vec<_> = (0..MAX_VOICES)
            .map(|_| Box::new(SourceHistoryWork::new()))
            .collect();
        let mut frozen = Vec::with_capacity(MAX_VOICES);
        let mut addresses = Vec::with_capacity(MAX_VOICES);
        let lifetime = Arc::new(AtomicU64::new(1));
        let mut copied = [false; MAX_VOICES];
        let mut durations_ns = [0_u128; MEASURED_BATCHES];

        // Every allocation and journal fill precedes all capture timing. Distinct scalar
        // address markers prevent coalescing but never stand for retained PCM ownership.
        for (voice_index, (history, work)) in histories.iter_mut().zip(&works).enumerate() {
            let mut playback = SourcePlayback::new(19, ExplicitSeekMode::Normal, 0.73);
            let region = FrameRange {
                start: 19,
                end: 1519,
            };
            playback.configure(2003, region);
            let mut transition = StemTransition::start(StemRenderSelection::full_mix(), 80_000);
            for span_index in 0..SOURCE_FEED_HISTORY_MAX_SPANS {
                playback.set_target(0.53 + ((span_index * 13 + voice_index) % 83) as f64 * 0.017);
                let (frames, ratio) = playback.chunk(1 + (span_index + voice_index) % 29);
                let position = playback.position();
                let plan = SourceReadPlan {
                    channels: 2,
                    sample_frames: 2003,
                    frame_pos: position.frame,
                    loop_region: region,
                    loop_period: None,
                    seek_mode: position.seek_mode,
                    selection: StemRenderSelection::full_mix(),
                    transition,
                };
                let binding = ProductiveSourceBinding {
                    resident: None,
                    source_address: 17 + voice_index * SOURCE_FEED_HISTORY_MAX_SPANS + span_index,
                    sample_count: 4006,
                    channels: 2,
                    sample_rate_hz: 48_000,
                    accepted: None,
                };
                history.record_success(
                    playback,
                    plan,
                    SourceFeedContext::new(binding, StemReadSets::default()),
                    frames,
                );
                playback.advance(frames);
                transition.advance_output_frames(frames, ratio);
            }
            playback.chunk(1024);
            let position = playback.position();
            frozen.push((
                playback,
                SourceReadPlan {
                    channels: 2,
                    sample_frames: 2003,
                    frame_pos: position.frame,
                    loop_region: region,
                    loop_period: None,
                    seek_mode: position.seek_mode,
                    selection: StemRenderSelection::full_mix(),
                    transition,
                },
                4096,
                SourceFeedContext::new(
                    ProductiveSourceBinding {
                        resident: None,
                        source_address: 17 + voice_index,
                        sample_count: 4006,
                        channels: 2,
                        sample_rate_hz: 48_000,
                        accepted: None,
                    },
                    StemReadSets::default(),
                ),
            ));
            let address = work.as_ref() as *const SourceHistoryWork as usize;
            assert!(!addresses.contains(&address));
            addresses.push(address);
        }

        for batch in 0..WARMUP_BATCHES + MEASURED_BATCHES {
            // Worker/control recycling, including Arc retirement, stays outside measurement.
            for work in &mut works {
                work.clear_for_reuse();
            }
            let started = Instant::now();
            for voice_index in 0..MAX_VOICES {
                let work = black_box(&mut works[voice_index]);
                copied[voice_index] =
                    black_box(&histories[voice_index]).capture_into(black_box(&mut work.snapshot));
                work.future = Some(black_box(frozen[voice_index]));
            }
            let elapsed_ns = started.elapsed().as_nanos();

            for (voice_index, work) in works.iter().enumerate() {
                assert!(copied[voice_index]);
                assert_eq!(
                    work.as_ref() as *const SourceHistoryWork as usize,
                    addresses[voice_index]
                );
                assert_eq!(work.snapshot.spans().len(), SOURCE_FEED_HISTORY_MAX_SPANS);
                assert!(work.snapshot.guard(&lifetime, 1).unwrap().is_current());
                black_box(work.snapshot.spans().fold(0_u64, |checksum, span| {
                    checksum
                        .wrapping_add(span.active_start)
                        .wrapping_add(span.frames as u64)
                        .wrapping_add(span.plan.frame_pos as u64)
                        .wrapping_add(span.playback.tempo_ratio().to_bits())
                        .wrapping_add(span.context.binding.source_address as u64)
                }));
            }
            if batch >= WARMUP_BATCHES {
                durations_ns[batch - WARMUP_BATCHES] = elapsed_ns;
            }
        }
        for work in &mut works {
            work.clear_for_reuse();
        }

        let span_box_bytes = SOURCE_FEED_HISTORY_MAX_SPANS * size_of::<Option<SourceFeedSpan>>();
        let mode_span_bytes = MAX_VOICES * span_box_bytes;
        let existing_span_bytes = (MAX_VOICES + SET_WORKS) * span_box_bytes;
        let combined_span_bytes = existing_span_bytes + mode_span_bytes;
        let mode_work_outer_bytes = MAX_VOICES * size_of::<SourceHistoryWork>();
        let mode_work_pointer_bytes = MAX_VOICES * size_of::<Box<SourceHistoryWork>>();
        let mode_guard_inline_bytes = MAX_VOICES * size_of::<SourceFeedGuard>();
        let return_queue_payload_bytes =
            MAX_VOICES * LANE_CAPACITY * size_of::<Box<SourceHistoryWork>>();
        let return_queue_handle_bytes = MAX_VOICES
            * (size_of::<Producer<Box<SourceHistoryWork>>>()
                + size_of::<Consumer<Box<SourceHistoryWork>>>());
        let retained_history_pointer_bytes =
            2 * MAX_VOICES * size_of::<Option<Box<SourceHistoryWork>>>();
        let request_enum_extra_bytes = size_of::<SourcePreparationRequest>()
            .checked_sub(size_of::<NativeHistoryRequest>())
            .unwrap();
        durations_ns.sort_unstable();
        eprintln!(
            "prospective mode scalar pool: profile={} voice_count={} journal_count={} \
             set_snapshot_count={} mode_work_count={} spans_per_owner={} retained_frame_bound={} \
             span_bytes={} option_span_bytes={} span_box_bytes={} mode_span_bytes={} \
             allocated_journal_mode_span_bytes={} \
             derived_existing_journal_set_span_bytes={} derived_combined_span_bytes={}",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            MAX_VOICES,
            MAX_VOICES,
            SET_WORKS,
            MAX_VOICES,
            SOURCE_FEED_HISTORY_MAX_SPANS,
            SOURCE_FEED_HISTORY_MAX_FRAMES,
            size_of::<SourceFeedSpan>(),
            size_of::<Option<SourceFeedSpan>>(),
            span_box_bytes,
            mode_span_bytes,
            2 * mode_span_bytes,
            existing_span_bytes,
            combined_span_bytes,
        );
        eprintln!(
            "prospective mode separate overhead: work_outer_bytes_each={} \
             mode_work_outer_bytes={} mode_work_pointer_bytes={} guard_inline_bytes_each={} \
             mode_guard_inline_bytes={} recycle_queue_capacity_each={} \
             recycle_queue_logical_payload_bytes={} recycle_queue_handle_bytes={} \
             callback_worker_retained_history_pointer_bytes={} native_request_bytes={} \
             request_enum_bytes={} enum_extra_bytes_each={} \
             request_queue_enum_extra_bytes={} pending_request_enum_extra_bytes={} \
             (logical payload/inline layout only; excludes allocator, Arc control blocks, \
             ring metadata/alignment, Native/PCM and other work storage; guard clones allocate no owners)",
            size_of::<SourceHistoryWork>(),
            mode_work_outer_bytes,
            mode_work_pointer_bytes,
            size_of::<SourceFeedGuard>(),
            mode_guard_inline_bytes,
            LANE_CAPACITY,
            return_queue_payload_bytes,
            return_queue_handle_bytes,
            retained_history_pointer_bytes,
            size_of::<NativeHistoryRequest>(),
            size_of::<SourcePreparationRequest>(),
            request_enum_extra_bytes,
            MAX_VOICES * LANE_CAPACITY * request_enum_extra_bytes,
            MAX_VOICES * request_enum_extra_bytes,
        );
        eprintln!(
            "prospective mode 32-voice capture-batch host timing: measured_batches={} \
             warmup_batches={} captures_per_batch={} copied_spans_per_batch={} \
             min_ns={} median_ns={} p99_ns={} max_ns={} \
             (capture plus fixed future assignment only; clear/guard/projection/queues/replay \
             excluded; no product RAM, callback deadline, RSS, Native, P6 or device acceptance claim)",
            MEASURED_BATCHES,
            WARMUP_BATCHES,
            MAX_VOICES,
            MAX_VOICES * SOURCE_FEED_HISTORY_MAX_SPANS,
            durations_ns[0],
            durations_ns[MEASURED_BATCHES / 2],
            durations_ns[(MEASURED_BATCHES * 99).div_ceil(100) - 1],
            durations_ns[MEASURED_BATCHES - 1],
        );
    }

    #[test]
    fn failed_history_capture_recycles_same_filled_owner_without_a_native_reserve() {
        let (mut callback, mut worker) = source_history_lanes(1);
        let address = history_address(&callback);
        let request = fixture(1, 48_000, 1.0, ExplicitSeekMode::Normal, None);
        let feed = prospective_feed(&request);
        let history = SourceFeedHistory::new();
        let lifetime = Arc::new(AtomicU64::new(2));
        assert!(!callback.request_source_with_history(
            &feed,
            history_context(&history, &lifetime, &feed, 8192),
            0,
            1,
        ));
        assert!(callback.source_history.is_none());
        let queued = worker.requests.peek().unwrap();
        let SourcePreparationRequest::WithHistory {
            history, prepare, ..
        } = queued
        else {
            panic!("capture refusal retains its snapshot owner");
        };
        assert!(!*prepare);
        assert!(history.snapshot.is_current());
        assert_eq!(
            history.as_ref() as *const SourceHistoryWork as usize,
            address
        );
        worker.source_reserve = None;
        assert!(worker.prepare_source().unwrap());
        assert!(callback.source_history_work_available());
        assert_eq!(history_address(&callback), address);
        assert_eq!(
            callback
                .source_history
                .as_ref()
                .unwrap()
                .snapshot
                .captured_epoch(),
            0
        );
        assert!(!callback.source_work_finished(1));
    }

    #[test]
    fn paused_history_capture_keeps_original_global_e_without_projecting_active_frames() {
        let (mut callback, mut worker) = source_history_lanes(1);
        let mut request = fixture(1, 48_000, 1.37, ExplicitSeekMode::Normal, None);
        request.playback.set_target(0.73);
        request
            .playback
            .chunk(super::super::stretch_processor::DEFAULT_BLOCK_SAMPLES);
        let feed = prospective_feed(&request);
        let history = SourceFeedHistory::new();
        let lifetime = Arc::new(AtomicU64::new(1));
        let mut context = history_context(&history, &lifetime, &feed, 8192);
        context.paused = true;
        assert!(callback.request_source_with_history(&feed, context, 0, 1));
        let SourcePreparationRequest::WithHistory {
            request: Some(queued),
            history: captured,
            ..
        } = worker.requests.peek().unwrap()
        else {
            panic!("the original paused request owns its captured work");
        };
        assert_eq!(queued.target_output_frame, 8292);
        assert!(queued.playback.matches_exact(feed.playback));
        assert!(queued.plan.matches_exact(feed.plan));
        let (future_playback, future_plan, active_delta, _) = captured.future.as_ref().unwrap();
        assert_eq!(*active_delta, 0);
        assert!(future_playback.matches_exact(feed.playback));
        assert!(future_plan.matches_exact(feed.plan));

        // An empty paused past cannot borrow the global waiting interval as source history.
        assert!(worker.prepare_source().unwrap());
        assert!(callback.source_work_finished(1));
        assert!(callback.prepared_state().is_none());
        assert!(callback.source_history_work_available());
        assert!(!callback.worker_failed());
    }

    #[test]
    fn source_history_saturation_preserves_owner_original_e_and_recycled_adoption_guard() {
        let (mut callback, mut worker) = source_history_lanes(1);
        let address = history_address(&callback);
        let request = fixture(1, 48_000, 1.0, ExplicitSeekMode::Normal, None);
        let feed = prospective_feed(&request);
        let history = SourceFeedHistory::new();
        let lifetime = Arc::new(AtomicU64::new(1));
        assert!(
            callback
                .requests
                .push(SourcePreparationRequest::Ordinary(fixture(
                    1,
                    48_000,
                    1.0,
                    ExplicitSeekMode::Normal,
                    None
                )))
                .is_ok()
        );
        assert!(!callback.request_source_with_history(
            &feed,
            history_context(&history, &lifetime, &feed, 8192),
            0,
            1,
        ));
        assert_eq!(history_address(&callback), address);
        drop(worker.requests.pop().unwrap());
        assert!(callback.request_source_with_history(
            &feed,
            history_context(&history, &lifetime, &feed, 8192),
            0,
            1,
        ));
        assert!(!callback.request_source_with_history(
            &feed,
            history_context(&history, &lifetime, &feed, 12_288),
            0,
            2,
        ));
        let SourcePreparationRequest::WithHistory {
            request: Some(queued),
            history: captured,
            ..
        } = worker.requests.peek().unwrap()
        else {
            panic!("one original history request remains queued");
        };
        assert_eq!(queued.target_output_frame, 8292);
        assert_eq!(captured.future.as_ref().unwrap().2, 8192);
        assert_eq!(
            captured.as_ref() as *const SourceHistoryWork as usize,
            address
        );
        assert!(worker.prepare_source().unwrap());
        assert!(callback.source_history_work_available());
        assert_eq!(history_address(&callback), address);
        let candidate = callback.prepared_state().unwrap();
        let source = candidate.source.as_ref().unwrap();
        assert_eq!(source.target_output_frame, 8292);
        assert!(source.permit.current(&source.sample, 48_000));
        history.invalidate_captures();
        assert!(!source.permit.current(&source.sample, 48_000));
        assert!(
            source
                .permit
                .current_effective_source(&source.sample, 48_000)
        );
    }

    #[test]
    fn unavailable_source_history_returns_storage_and_keeps_later_request_usable() {
        let (mut callback, mut worker) = source_history_lanes(1);
        let address = history_address(&callback);
        let request = fixture(1, 48_000, 1.0, ExplicitSeekMode::Normal, None);
        let feed = prospective_feed(&request);
        let mut history = SourceFeedHistory::new();
        history.record_gap(1);
        let lifetime = Arc::new(AtomicU64::new(1));
        assert!(callback.request_source_with_history(
            &feed,
            history_context(&history, &lifetime, &feed, 17),
            0,
            1,
        ));
        assert!(worker.prepare_source().unwrap());
        assert!(callback.source_work_finished(1));
        assert!(callback.prepared_state().is_none());
        assert!(callback.source_history_work_available());
        assert_eq!(history_address(&callback), address);
        assert!(!callback.worker_failed());
        let valid = SourceFeedHistory::new();
        assert!(callback.request_source_with_history(
            &feed,
            history_context(&valid, &lifetime, &feed, 8192),
            0,
            2,
        ));
        assert!(worker.prepare_source().unwrap());
        assert!(callback.prepared_state().is_some());
        assert!(callback.source_history_work_available());
        assert_eq!(history_address(&callback), address);
    }

    #[test]
    fn full_history_recycler_retains_same_cleared_box_until_callback_can_reclaim() {
        let (mut callback, mut worker) = source_history_lanes(0);
        let address = history_address(&callback);
        let request = fixture(1, 48_000, 1.0, ExplicitSeekMode::Normal, None);
        let feed = prospective_feed(&request);
        let history = SourceFeedHistory::new();
        let lifetime = Arc::new(AtomicU64::new(2));
        assert!(!callback.request_source_with_history(
            &feed,
            history_context(&history, &lifetime, &feed, 8192),
            0,
            1,
        ));
        assert!(worker.prepare_source().unwrap());
        assert!(!callback.source_history_work_available());
        let pending = worker.pending_history_return.as_deref().unwrap();
        assert_eq!(pending as *const SourceHistoryWork as usize, address);
        assert_eq!(pending.snapshot.captured_epoch(), 0);
        assert!(pending.future.is_none());
        assert!(!worker.prepare_source().unwrap());
        assert_eq!(
            worker.pending_history_return.as_deref().unwrap() as *const SourceHistoryWork as usize,
            address
        );
        let (producer, consumer) = RingBuffer::new(1);
        worker.source_history_return = producer;
        callback.source_history_return = consumer;
        assert!(!worker.prepare_source().unwrap());
        assert!(callback.source_history_work_available());
        assert_eq!(history_address(&callback), address);
        assert!(worker.pending_history_return.is_none());
    }

    #[test]
    fn stale_and_failed_worker_requests_return_the_same_history_owner() {
        for failed in [false, true] {
            let (mut callback, mut worker) = source_history_lanes(1);
            let address = history_address(&callback);
            let request = fixture(1, 48_000, 1.0, ExplicitSeekMode::Normal, None);
            let feed = prospective_feed(&request);
            let history = SourceFeedHistory::new();
            let lifetime = Arc::new(AtomicU64::new(1));
            assert!(callback.request_source_with_history(
                &feed,
                history_context(&history, &lifetime, &feed, 8192),
                0,
                1,
            ));
            if failed {
                assert!(worker.drain_failed());
            } else {
                worker.source_epoch.store(1, Ordering::Release);
                assert!(worker.prepare_source().unwrap());
            }
            assert!(callback.source_work_finished(1));
            assert!(callback.prepared_state().is_none());
            assert!(callback.source_history_work_available());
            assert_eq!(history_address(&callback), address);
            assert_eq!(
                callback
                    .source_history
                    .as_ref()
                    .unwrap()
                    .snapshot
                    .captured_epoch(),
                0
            );
        }
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
            pitch_intent: super::super::pitch_intent::PitchIntent::default(),
            key_lock: true,
            sample: &sample,
            outgoing_stems: None,
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
                pitch_intent: super::super::pitch_intent::PitchIntent::default(),
                key_lock: true,
                sample: &request.sample,
                outgoing_stems: None,
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
        let (source_history_return, _source_history_consumer) = RingBuffer::new(1);
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
            source_history_return,
            pending_history_return: None,
            source_epoch: Arc::new(AtomicU64::new(0)),
            completed_request: completed_request.clone(),
        };
        let mut request = fixture(2, 48_000, 1.37, ExplicitSeekMode::Normal, None);
        request.epoch = 0;
        request.request_id = 17;
        let weak = Arc::downgrade(&request.sample.samples);
        assert!(
            requests
                .push(SourcePreparationRequest::Ordinary(request))
                .is_ok()
        );
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
