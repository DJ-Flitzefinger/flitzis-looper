//! One original Native runtime publication and its bounded callback attempt.
//! This owns requested rows and their optional immutable map derivative, never a prepared phase,
//! source clock or scheduler.

use super::{InputRuntime, RuntimePadState};
use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
use crate::audio_engine::content_placement::OwnerIdentity;
use crate::audio_engine::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
use crate::audio_engine::pad_start::MappedStartSource;
use crate::audio_engine::runtime_policy_retry::RetryToken;
use crate::audio_engine::source_map_admission::RuntimeMapPreparation;
use crate::audio_engine::source_sync_policy::{SourceSyncMode, SourceSyncRequest};
use crate::messages::ControlMessage;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
};

const IDLE: u8 = 0;
const PENDING: u8 = 1;
const APPLIED: u8 = 2;
const REFUSED: u8 = 3;
const UPDATING: u8 = 4;

#[cfg(test)]
thread_local! {
    static FINAL_DROP_BEFORE_PENDING_CHECK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum RuntimePolicyError {
    Busy = 1,
    Stale,
    Unsupported,
    SourceChanged,
    MissingMaster,
    InvalidTarget,
    PhaseChanged,
    ReaderUnavailable,
    Capacity,
    SchedulerFull,
    SequenceExhausted,
    NoActiveVoices,
}

impl RuntimePolicyError {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::Stale => "stale",
            Self::Unsupported => "unsupported",
            Self::SourceChanged => "source_changed",
            Self::MissingMaster => "missing_master",
            Self::InvalidTarget => "invalid_target",
            Self::PhaseChanged => "phase_changed",
            Self::ReaderUnavailable => "reader_unavailable",
            Self::Capacity => "capacity",
            Self::SchedulerFull => "scheduler_full",
            Self::SequenceExhausted => "sequence_exhausted",
            Self::NoActiveVoices => "no_active_voices",
        }
    }

    fn from_code(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Busy,
            2 => Self::Stale,
            3 => Self::Unsupported,
            4 => Self::SourceChanged,
            5 => Self::MissingMaster,
            6 => Self::InvalidTarget,
            7 => Self::PhaseChanged,
            8 => Self::ReaderUnavailable,
            9 => Self::Capacity,
            10 => Self::SchedulerFull,
            11 => Self::SequenceExhausted,
            12 => Self::NoActiveVoices,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SourceFence {
    generation: u64,
    incarnation: u64,
    timing_intent: u8,
    acknowledgement: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct RuntimePolicyRow {
    pub(crate) binding: InputPadBinding,
    pub(crate) request: SourceSyncRequest,
    pub(crate) pitch: super::PitchPadProjection,
    pub(crate) loop_start_s: f64,
    pub(crate) loop_end_s: Option<f64>,
    pub(crate) map_preparation: RuntimeMapPreparation,
}

struct PreparedRuntimeMap {
    preparation: RuntimeMapPreparation,
    source: Option<Arc<MappedStartSource>>,
    available: bool,
}

impl PreparedRuntimeMap {
    fn available(row: &RuntimePadState, ownership: &Arc<InputRuntimeOwnership>) -> bool {
        row.binding
            .filter(|_| row.loaded)
            .is_some_and(|binding| row.source_map.runtime_map(ownership, binding).is_some())
    }

    /// Called once for the exact rows captured by the existing control-owned snapshot.
    fn capture(row: &RuntimePadState, ownership: &Arc<InputRuntimeOwnership>) -> Self {
        let unavailable = || Self {
            preparation: RuntimeMapPreparation::unavailable(row.source_map.selected_sync_witness()),
            source: None,
            available: false,
        };
        let Some(binding) = row.binding.filter(|_| row.loaded) else {
            return unavailable();
        };
        let Some(qualified) = row.source_map.runtime_map(ownership, binding) else {
            return unavailable();
        };
        let (preparation, source) = MappedStartSource::prepare_runtime_map(
            qualified,
            binding,
            row.loop_start_s,
            row.loop_end_s,
        );
        Self {
            preparation,
            source,
            available: true,
        }
    }
}

/// Scalar observation only. Original immutable rows retain all owning custody.
#[derive(Clone, Copy)]
pub(crate) struct RuntimePolicyRetryRow {
    pub(crate) row: RuntimePolicyRow,
    source: SourceFence,
    acknowledgements: OwnerIdentity,
}

impl RuntimePolicyRetryRow {
    pub(crate) fn current(
        &self,
        ownership: &InputRuntimeOwnership,
        acknowledgements: &Arc<CurrentTimingAcknowledgements>,
        id: usize,
    ) -> bool {
        let binding = self.row.binding;
        self.acknowledgements == OwnerIdentity::of(acknowledgements)
            && ownership.current(id, binding)
            && ownership.binding_source_generation(id, binding) == Some(self.source.generation)
            && ownership.binding_source_request_incarnation(id, binding)
                == Some(self.source.incarnation)
            && ownership.binding_timing_intent(id, binding) == Some(self.source.timing_intent)
            && acknowledgements.current_for_binding(ownership, id, binding)
                == self.source.acknowledgement
            && ownership.source_timing_available_for_binding(id, binding, acknowledgements)
            && ownership.copy_reservation(id) == 0
            && ownership.pair_reservation(id) == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimePolicyAttempt {
    pub(crate) attempt: u64,
    pub(crate) state: &'static str,
    pub(crate) reason: Option<RuntimePolicyError>,
    pub(crate) execution_frame: Option<u64>,
}

pub(crate) struct RuntimePolicySnapshot {
    revision: u64,
    // Exact clones of the ORIGINAL rows, including ACK and selected-map custody.
    // An unavailable/mismatched source-map proxy must never stand in for these.
    rows: Box<[RuntimePadState]>,
    // Derivative geometry belongs to these same original rows and is prepared only once off RT.
    maps: Box<[PreparedRuntimeMap]>,
    sources: Box<[Option<SourceFence>]>,
    ownership: Arc<InputRuntimeOwnership>,
    state: AtomicU8,
    attempt: AtomicU64,
    reason: AtomicU8,
    execution: AtomicU64,
    execution_present: AtomicBool,
    retry_birth: AtomicU64,
    retry_condition: AtomicU64,
    retry_timing_revision: AtomicU64,
    // Idle is zero. An attempt counts each Work plus one cleanup reservation;
    // only its last Work releases that reservation after terminal cleanup.
    dispatch_owners: AtomicUsize,
}

/// One attempt's queue/scheduler/retirement custody. Cached requested rows never
/// retain this lease, so closing an unexecuted callback can release Pending.
pub(crate) struct RuntimePolicyWork {
    snapshot: Arc<RuntimePolicySnapshot>,
    attempt: u64,
}

impl std::ops::Deref for RuntimePolicyWork {
    type Target = RuntimePolicySnapshot;
    fn deref(&self) -> &Self::Target {
        &self.snapshot
    }
}

impl Clone for RuntimePolicyWork {
    fn clone(&self) -> Self {
        self.snapshot.dispatch_owners.fetch_add(1, Ordering::AcqRel);
        Self {
            snapshot: self.snapshot.clone(),
            attempt: self.attempt,
        }
    }
}

impl std::fmt::Debug for RuntimePolicyWork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimePolicyWork")
            .field("snapshot", &self.snapshot)
            .field("attempt", &self.attempt)
            .finish()
    }
}

impl PartialEq for RuntimePolicyWork {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.snapshot, &other.snapshot) && self.attempt == other.attempt
    }
}

impl Drop for RuntimePolicyWork {
    fn drop(&mut self) {
        // Every terminal RT owner is first transferred to the existing worker.
        // A scheduler's failed clone has its original guard still alive.
        let previous = self.snapshot.dispatch_owners.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous >= 2);
        if previous != 2 {
            return;
        }
        // One remains reserved throughout cleanup. The control cache therefore
        // cannot begin a new attempt that an old destructor could overwrite.
        if self.snapshot.attempt.load(Ordering::Acquire) == self.attempt {
            #[cfg(test)]
            if let Some(hook) =
                FINAL_DROP_BEFORE_PENDING_CHECK.with(|hook| hook.borrow_mut().take())
            {
                hook();
            }
            if self.snapshot.state.load(Ordering::Acquire) == PENDING {
                self.snapshot.finish(Err(RuntimePolicyError::Busy), None);
            }
        }
        self.snapshot.dispatch_owners.store(0, Ordering::Release);
        // No attempt state is read or written after publishing idle.
    }
}

impl std::fmt::Debug for RuntimePolicySnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimePolicySnapshot")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl PartialEq for RuntimePolicySnapshot {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl RuntimePolicySnapshot {
    fn source_fence(
        ownership: &InputRuntimeOwnership,
        id: usize,
        row: &RuntimePadState,
    ) -> Option<SourceFence> {
        let binding = row.binding.filter(|_| row.loaded)?;
        let acknowledgements = row.acknowledgements.as_ref()?;
        Some(SourceFence {
            generation: ownership.binding_source_generation(id, binding)?,
            incarnation: ownership.binding_source_request_incarnation(id, binding)?,
            timing_intent: ownership.binding_timing_intent(id, binding)?,
            acknowledgement: acknowledgements.current_for_binding(ownership, id, binding),
        })
    }

    /// Independent callback ACK/source fences can change without a row publish.
    /// Compare the existing capture in place before allocating any replacement.
    fn source_fences_current(&self) -> bool {
        let sources_current = self
            .rows
            .iter()
            .enumerate()
            .all(|(id, row)| self.sources[id] == Self::source_fence(&self.ownership, id, row));
        // Source/ACK revocation always wins. Job cancellation after genuine adoption
        // does not revoke effective immutable custody or restart a current trajectory.
        sources_current
            && (self.actual_current()
                || self.rows.iter().enumerate().all(|(id, row)| {
                    self.maps[id].available == PreparedRuntimeMap::available(row, &self.ownership)
                }))
    }

    fn capture(runtime: &InputRuntime, state: &super::RuntimeState, revision: u64) -> Self {
        let rows = state.pads.clone().into_boxed_slice();
        let sources = rows
            .iter()
            .enumerate()
            .map(|(id, row)| Self::source_fence(&runtime.ownership, id, row))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let maps = rows
            .iter()
            .map(|row| PreparedRuntimeMap::capture(row, &runtime.ownership))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            revision,
            rows,
            maps,
            sources,
            ownership: runtime.ownership.clone(),
            state: AtomicU8::new(IDLE),
            attempt: AtomicU64::new(0),
            reason: AtomicU8::new(0),
            execution: AtomicU64::new(u64::MAX),
            execution_present: AtomicBool::new(false),
            retry_birth: AtomicU64::new(0),
            retry_condition: AtomicU64::new(0),
            retry_timing_revision: AtomicU64::new(0),
            dispatch_owners: AtomicUsize::new(0),
        }
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn belongs_to(&self, ownership: &Arc<InputRuntimeOwnership>) -> bool {
        Arc::ptr_eq(&self.ownership, ownership)
    }

    pub(crate) fn current(&self) -> bool {
        self.ownership.runtime_policy_revision() == Some(self.revision)
    }

    pub(crate) fn row(&self, id: usize) -> Option<RuntimePolicyRow> {
        let row = self.rows.get(id).filter(|row| row.loaded)?;
        Some(RuntimePolicyRow {
            binding: row.binding?,
            request: row.source_map.request(),
            pitch: row.pitch,
            loop_start_s: row.loop_start_s,
            loop_end_s: row.loop_end_s,
            map_preparation: self.map_preparation(id),
        })
    }

    /// Borrow prepared custody only; the full actual-E claim separately repeats `map_current`.
    pub(crate) fn prepared_map(&self, id: usize) -> Option<&Arc<MappedStartSource>> {
        self.maps.get(id)?.source.as_ref()
    }

    pub(crate) fn map_preparation(&self, id: usize) -> RuntimeMapPreparation {
        self.maps.get(id).map_or_else(
            || RuntimeMapPreparation::unavailable(None),
            |map| map.preparation,
        )
    }

    /// This final successful source check is the pending map's admission decision. A later
    /// overlapping data-job cancellation cannot revoke an already effective immutable pin.
    pub(crate) fn map_current(&self, id: usize) -> bool {
        let Some(row) = self.rows.get(id).filter(|row| row.loaded) else {
            return false;
        };
        let Some(binding) = row.binding else {
            return false;
        };
        self.row_current(id)
            && row
                .source_map
                .runtime_map(&self.ownership, binding)
                .is_some()
            && self
                .prepared_map(id)
                .is_none_or(|source| source.current_for(&self.ownership, binding))
    }

    pub(crate) fn retry_row(&self, id: usize) -> Option<RuntimePolicyRetryRow> {
        let row = self.rows.get(id).filter(|row| row.loaded)?;
        Some(RuntimePolicyRetryRow {
            row: self.row(id)?,
            source: self.sources.get(id).copied().flatten()?,
            acknowledgements: OwnerIdentity::of(row.acknowledgements.as_ref()?),
        })
    }

    pub(crate) fn retry_fences_current(&self) -> bool {
        self.current()
            && self
                .rows
                .iter()
                .enumerate()
                .all(|(id, row)| !row.loaded || self.row_current(id))
    }

    /// Reads only captured rows and existing fixed Native fences, including inside
    /// the original callback claim. No runtime mutex or fresh control capture.
    pub(crate) fn row_current(&self, id: usize) -> bool {
        let Some(row) = self.rows.get(id).filter(|row| row.loaded) else {
            return false;
        };
        let Some(binding) = row.binding else {
            return false;
        };
        let Some(source) = self.sources.get(id).copied().flatten() else {
            return false;
        };
        let Some(acknowledgements) = &row.acknowledgements else {
            return false;
        };
        self.current()
            && self.ownership.current(id, binding)
            && self.ownership.binding_source_generation(id, binding) == Some(source.generation)
            && self
                .ownership
                .binding_source_request_incarnation(id, binding)
                == Some(source.incarnation)
            && self.ownership.binding_timing_intent(id, binding) == Some(source.timing_intent)
            && acknowledgements.current_for_binding(&self.ownership, id, binding)
                == source.acknowledgement
            && self
                .ownership
                .source_timing_available_for_binding(id, binding, acknowledgements)
            && self.ownership.copy_reservation(id) == 0
            && self.ownership.pair_reservation(id) == 0
            && self.current()
    }

    fn actual_current(&self) -> bool {
        let mut active = false;
        for (id, row) in self.rows.iter().enumerate() {
            let Some(status) = self.ownership.source_sync_status(id) else {
                continue;
            };
            if !status.active {
                continue;
            }
            active = true;
            if !row.loaded
                || !self.row_current(id)
                || row.binding.is_none_or(|binding| {
                    status.content_lifetime != binding.placement.map_or(0, |p| p.lifetime())
                        || status.source_address != binding.source_address
                        || status.window_revision != binding.resident.map_or(0, |r| r.revision)
                        || self.sources[id]
                            .is_none_or(|source| source.generation != status.source_generation)
                })
                || status.policy.is_none_or(|policy| {
                    policy.request != row.source_map.request()
                        || !policy.target_current
                        || (policy.request.requires_common_phase()
                            && (!policy.phase_program_current
                                || policy.phase_program_revision != Some(self.revision)))
                })
            {
                return false;
            }
        }
        active && self.current()
    }

    /// Only complete current callback evidence can keep an idle refusal quiet.
    /// Missing loaded-row observations permit retry; every slot is still checked
    /// for an active participant outside the captured loaded cohort.
    fn actual_inactive(&self) -> bool {
        for (id, row) in self.rows.iter().enumerate() {
            let status = self.ownership.source_sync_status(id);
            if status.is_some_and(|status| status.active) {
                return false;
            }
            if !row.loaded {
                continue;
            }
            let Some(status) = status else {
                return false;
            };
            if !self.row_current(id)
                || row.binding.is_none_or(|binding| {
                    binding
                        .placement
                        .is_none_or(|placement| status.content_lifetime != placement.lifetime())
                        || status.source_address != binding.source_address
                        || status.window_revision != binding.resident.map_or(0, |r| r.revision)
                        || self.sources[id]
                            .is_none_or(|source| source.generation != status.source_generation)
                })
                || self.ownership.source_sync_status(id) != Some(status)
            {
                return false;
            }
        }
        self.current()
    }

    #[cfg(test)]
    fn begin(self: &Arc<Self>) -> Option<RuntimePolicyWork> {
        self.begin_with_service(false)
    }

    fn begin_with_service(self: &Arc<Self>, service: bool) -> Option<RuntimePolicyWork> {
        if self.dispatch_owners.load(Ordering::Acquire) != 0 {
            return None;
        }
        let state = self.state.load(Ordering::Acquire);
        if state == PENDING || state == UPDATING {
            return None;
        }
        if (state == APPLIED || state == REFUSED)
            && (self.actual_current()
                || (state == REFUSED
                    && RuntimePolicyError::from_code(self.reason.load(Ordering::Acquire))
                        == Some(RuntimePolicyError::NoActiveVoices)
                    && self.actual_inactive()))
        {
            return None;
        }
        if service
            && state == REFUSED
            && RuntimePolicyError::from_code(self.reason.load(Ordering::Acquire))
                == Some(RuntimePolicyError::Unsupported)
            && self.retry_fences_current()
        {
            let condition = self.retry_condition.load(Ordering::Acquire);
            let token = RetryToken {
                birth: self.retry_birth.load(Ordering::Acquire),
                condition,
                revision: self.revision,
                timing_revision: self.retry_timing_revision.load(Ordering::Acquire),
            };
            if condition != 0
                && self.ownership.runtime_policy_retry.current() == Some(token)
                && self
                    .ownership
                    .global_timing_revision
                    .load(Ordering::Acquire)
                    == token.timing_revision
                && !self
                    .ownership
                    .global_timing_publishing
                    .load(Ordering::Acquire)
                && self.retry_fences_current()
            {
                return None;
            }
        }
        if self
            .state
            .compare_exchange(state, UPDATING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return None;
        }
        let Some(attempt) = self.attempt.load(Ordering::Relaxed).checked_add(1) else {
            self.reason.store(
                RuntimePolicyError::SequenceExhausted as u8,
                Ordering::Relaxed,
            );
            self.execution_present.store(false, Ordering::Relaxed);
            self.state.store(REFUSED, Ordering::Release);
            return None;
        };
        self.attempt.store(attempt, Ordering::Relaxed);
        self.reason.store(0, Ordering::Relaxed);
        self.retry_condition.store(0, Ordering::Relaxed);
        self.execution.store(u64::MAX, Ordering::Relaxed);
        self.execution_present.store(false, Ordering::Relaxed);
        // The only production clones are the scheduler argument and its stored
        // event copy: at most three Work owners, or counter value four here.
        self.dispatch_owners.store(2, Ordering::Release);
        self.state.store(PENDING, Ordering::Release);
        Some(RuntimePolicyWork {
            snapshot: self.clone(),
            attempt,
        })
    }

    pub(crate) fn finish(&self, result: Result<(), RuntimePolicyError>, frame: Option<u64>) {
        self.finish_with_retry(result, frame, None);
    }

    pub(crate) fn finish_with_retry(
        &self,
        result: Result<(), RuntimePolicyError>,
        frame: Option<u64>,
        retry: Option<RetryToken>,
    ) {
        let retry = retry.filter(|token| {
            result == Err(RuntimePolicyError::Unsupported) && token.revision == self.revision
        });
        self.retry_birth
            .store(retry.map_or(0, |token| token.birth), Ordering::Relaxed);
        self.retry_timing_revision.store(
            retry.map_or(0, |token| token.timing_revision),
            Ordering::Relaxed,
        );
        self.retry_condition
            .store(retry.map_or(0, |token| token.condition), Ordering::Relaxed);
        self.reason.store(
            result.err().map_or(0, |error| error as u8),
            Ordering::Relaxed,
        );
        self.execution
            .store(frame.unwrap_or(u64::MAX), Ordering::Relaxed);
        self.execution_present
            .store(frame.is_some(), Ordering::Relaxed);
        self.state.store(
            if result.is_ok() { APPLIED } else { REFUSED },
            Ordering::Release,
        );
    }

    pub(crate) fn observation(&self) -> Option<RuntimePolicyAttempt> {
        let state = self.state.load(Ordering::Acquire);
        let attempt = self.attempt.load(Ordering::Acquire);
        let value = RuntimePolicyAttempt {
            attempt,
            state: match state {
                PENDING => "pending",
                APPLIED => "applied",
                REFUSED => "refused",
                _ => return None,
            },
            reason: if state == PENDING {
                None
            } else {
                RuntimePolicyError::from_code(self.reason.load(Ordering::Acquire))
            },
            execution_frame: if state == PENDING {
                None
            } else {
                let frame = self.execution.load(Ordering::Acquire);
                self.execution_present
                    .load(Ordering::Acquire)
                    .then_some(frame)
            },
        };
        (self.state.load(Ordering::Acquire) == state
            && self.attempt.load(Ordering::Acquire) == attempt)
            .then_some(value)
    }
}

impl InputRuntime {
    /// Caller holds the engine's request mutex before this original Native mutex.
    #[cfg(test)]
    pub(crate) fn request_runtime_policy_adoption(&self) -> Result<(), String> {
        self.request_runtime_policy_adoption_with_service(false)
    }

    pub(crate) fn request_runtime_policy_adoption_with_service(
        &self,
        service: bool,
    ) -> Result<(), String> {
        let mut state = self
            .runtime_state
            .lock()
            .map_err(|_| "input runtime lock poisoned")?;
        let Some(mut revision) = self.ownership.runtime_policy_revision() else {
            return Ok(());
        };
        if state.policy_snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.revision == revision && !snapshot.source_fences_current()
        }) {
            // The observed independent values have already changed. Commit after
            // those writes under the same row mutex; never bump for prospective rows.
            self.ownership.publish_runtime_policy_revision();
            let Some(current) = self.ownership.runtime_policy_revision() else {
                return Ok(());
            };
            revision = current;
        }
        if state
            .policy_snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.revision != revision)
        {
            state.policy_snapshot = Some(Arc::new(RuntimePolicySnapshot::capture(
                self, &state, revision,
            )));
        }
        let snapshot = state
            .policy_snapshot
            .as_ref()
            .expect("captured runtime policy");
        let mut producer = self
            .audio_producer
            .lock()
            .map_err(|_| "audio producer lock poisoned")?;
        let Some(work) = snapshot.begin_with_service(service) else {
            return Ok(());
        };
        if let Err(rtrb::PushError::Full(ControlMessage::AdoptRuntimePolicy(owner))) =
            producer.push(ControlMessage::AdoptRuntimePolicy(work))
        {
            // Control owns both references and may release a failed enqueue here.
            owner.finish(Err(RuntimePolicyError::Capacity), None);
        }
        Ok(())
    }

    pub(crate) fn runtime_policy_snapshot(
        &self,
    ) -> Result<Option<Arc<RuntimePolicySnapshot>>, String> {
        let state = self
            .runtime_state
            .lock()
            .map_err(|_| "input runtime lock poisoned")?;
        Ok(state
            .policy_snapshot
            .as_ref()
            .filter(|snapshot| snapshot.current())
            .cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::timing::InputClock;
    use std::sync::Mutex;

    #[test]
    fn scheduler_three_work_owners_keep_cleanup_reserved_until_final_drop_and_exact_retry() {
        use crate::audio_engine::scheduler::{FixedCapacityScheduler, ScheduledCommand};
        let (producer, mut consumer) = rtrb::RingBuffer::new(2);
        let runtime = InputRuntime::new_with_ownership(
            Arc::new(Mutex::new(producer)),
            InputClock::new(),
            Arc::new(InputRuntimeOwnership::default()),
        );
        runtime.request_runtime_policy_adoption().unwrap();
        let ControlMessage::AdoptRuntimePolicy(original) = consumer.pop().unwrap() else {
            panic!("original request")
        };
        let snapshot = runtime.runtime_policy_snapshot().unwrap().unwrap();
        let mut scheduler = FixedCapacityScheduler::<1>::new();
        let returned = scheduler
            .schedule(9, ScheduledCommand::AdoptRuntimePolicy(original.clone()))
            .unwrap();
        // Actual original + returned event + stored event, and the cleanup guard.
        assert_eq!(snapshot.dispatch_owners.load(Ordering::Acquire), 4);
        let pending = snapshot.observation();
        drop(original);
        runtime.request_runtime_policy_adoption().unwrap();
        assert_eq!(snapshot.dispatch_owners.load(Ordering::Acquire), 3);
        assert_eq!(snapshot.observation(), pending);
        assert!(consumer.is_empty());
        drop(returned);
        runtime.request_runtime_policy_adoption().unwrap();
        assert_eq!(snapshot.dispatch_owners.load(Ordering::Acquire), 2);
        assert_eq!(snapshot.observation(), pending);
        assert!(consumer.is_empty());
        let due = scheduler.pop_due_at_callback_start(9).unwrap();
        assert_eq!(snapshot.dispatch_owners.load(Ordering::Acquire), 2);
        drop(due);
        assert_eq!(snapshot.dispatch_owners.load(Ordering::Acquire), 0);
        assert_eq!(
            snapshot.observation(),
            Some(RuntimePolicyAttempt {
                attempt: 1,
                state: "refused",
                reason: Some(RuntimePolicyError::Busy),
                execution_frame: None,
            })
        );
        runtime.request_runtime_policy_adoption().unwrap();
        let ControlMessage::AdoptRuntimePolicy(next) = consumer.pop().unwrap() else {
            panic!("same cached retry")
        };
        assert!(Arc::ptr_eq(&next.snapshot, &snapshot));
        assert_eq!(
            snapshot.observation(),
            Some(RuntimePolicyAttempt {
                attempt: 2,
                state: "pending",
                reason: None,
                execution_frame: None,
            })
        );
        drop(next);
    }

    #[test]
    fn final_dispatch_owner_keeps_cached_attempt_closed_until_cleanup_before_exact_retry() {
        use std::sync::mpsc;
        use std::time::Duration;
        for terminal in [Ok(()), Err(RuntimePolicyError::Unsupported)] {
            let (producer, mut consumer) = rtrb::RingBuffer::new(2);
            let ownership = Arc::new(InputRuntimeOwnership::default());
            let runtime = InputRuntime::new_with_ownership(
                Arc::new(Mutex::new(producer)),
                InputClock::new(),
                ownership,
            );
            runtime.request_runtime_policy_adoption().unwrap();
            let ControlMessage::AdoptRuntimePolicy(work) = consumer.pop().unwrap() else {
                panic!("original request")
            };
            work.finish(terminal, Some(u64::MAX));
            let snapshot = runtime.runtime_policy_snapshot().unwrap().unwrap();
            let original = snapshot.observation().unwrap();
            assert_eq!(original.attempt, 1);
            let (entered, waiting) = mpsc::channel();
            let (release, released) = mpsc::channel();
            let retiring = std::thread::spawn(move || {
                FINAL_DROP_BEFORE_PENDING_CHECK.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(move || {
                        let _ = entered.send(());
                        let _ = released.recv();
                    }));
                });
                drop(work);
            });
            let reached = waiting.recv_timeout(Duration::from_secs(3)).is_ok();
            // Keep the genuine control cache strong. A cached terminal attempt
            // with no active rows is retryable only after final owner cleanup.
            let during_result = runtime.request_runtime_policy_adoption();
            let during = snapshot.observation();
            let prematurely_queued = consumer.pop().ok();
            let was_queued = prematurely_queued.is_some();
            // Every assertion follows release/join so RED cannot strand a thread.
            let _ = release.send(());
            let joined = retiring.join();
            let after_cleanup = snapshot.observation();
            drop(prematurely_queued);
            let retry_result = runtime.request_runtime_policy_adoption();
            let retry_work = consumer.pop().ok();
            let retry = snapshot.observation();
            let retry_sequence = retry_work.as_ref().and_then(|command| match command {
                ControlMessage::AdoptRuntimePolicy(work) => Some(work.attempt),
                _ => None,
            });
            drop(retry_work);
            assert!(reached && joined.is_ok());
            assert!(during_result.is_ok() && retry_result.is_ok());
            assert!(
                !was_queued,
                "old finalizer exposed a new queued attempt before cleanup"
            );
            assert_eq!(during, Some(original));
            assert_eq!(after_cleanup, Some(original));
            assert_eq!(retry_sequence, Some(2));
            assert_eq!(
                retry,
                Some(RuntimePolicyAttempt {
                    attempt: 2,
                    state: "pending",
                    reason: None,
                    execution_frame: None,
                })
            );
        }
    }

    #[test]
    fn attempt_exhaustion_reports_reason_without_reusing_previous_applied_e() {
        let (producer, mut consumer) = rtrb::RingBuffer::new(2);
        let ownership = Arc::new(InputRuntimeOwnership::default());
        let runtime = InputRuntime::new_with_ownership(
            Arc::new(Mutex::new(producer)),
            InputClock::new(),
            ownership.clone(),
        );
        runtime.request_runtime_policy_adoption().unwrap();
        let ControlMessage::AdoptRuntimePolicy(work) = consumer.pop().unwrap() else {
            panic!("original request")
        };
        work.finish(Ok(()), Some(u64::MAX));
        let snapshot = runtime.runtime_policy_snapshot().unwrap().unwrap();
        drop(work);
        snapshot.attempt.store(u64::MAX, Ordering::Release);
        assert!(snapshot.begin().is_none());
        let observation = snapshot.observation().unwrap();
        assert_eq!(
            observation.reason,
            Some(RuntimePolicyError::SequenceExhausted)
        );
        assert_eq!(observation.execution_frame, None);
        assert_eq!(observation.attempt, u64::MAX);
        assert!(consumer.is_empty());
    }
}
