//! Finite source-window preparation shares the two-worker, 32-job cold lane.
//! Complete file readers and all large allocations/destruction remain off callback.
use super::AudioEngine;
use super::cold_store::CommittedColdLease;
use super::constant_timing::CurrentTimingAcknowledgements;
use super::constants::NUM_SAMPLES;
use super::content_placement::{PlacementError, PlacementObservation, SlotSnapshot};
use super::input_mapping::CapturedInputPadTrigger;
use super::input_runtime_binding::{
    self, InputPadBinding, InputRuntimeOwnership, InputRuntimePadBinding,
};
use super::ordinary_start::OrdinaryStartPublication;
use super::pad_start::{CapturedPadStart, PadStartReadinessSeal};
use super::pitch_actions::PitchActionReject;
use super::prepared_source::{PreparedSourcePermit, ResidentLaunchPermit};
use crate::messages::{
    ControlMessage, PreparedStemSet, ResidentContext, ResidentControlIntent, SampleBuffer,
};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex, OnceLock, Weak,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

const ADOPTION_DEADLINE: Duration = Duration::from_secs(30);

#[cfg(test)]
mod resident_loop_transition_tests;

/// Preparation owns its result through a control mirror gap. This off-thread
/// clock excludes only time waiting on the original key's Pending placement.
#[derive(Default)]
pub(super) struct ResidentPreparationClock {
    paused: Mutex<(Option<Instant>, Duration)>,
}
impl ResidentPreparationClock {
    pub(super) fn observe_at(&self, pending: bool, now: Instant) -> Result<(), String> {
        let mut clock = self
            .paused
            .lock()
            .map_err(|_| "resident preparation clock poisoned")?;
        if pending {
            if clock.0.is_none() {
                clock.0 = Some(now);
            }
        } else if let Some(started) = clock.0.take() {
            clock.1 = clock
                .1
                .saturating_add(now.saturating_duration_since(started));
        }
        Ok(())
    }
    pub(super) fn expired_at(
        &self,
        admitted: Instant,
        pending: bool,
        deadline: Duration,
        now: Instant,
    ) -> Result<bool, String> {
        self.observe_at(pending, now)?;
        let clock = self
            .paused
            .lock()
            .map_err(|_| "resident preparation clock poisoned")?;
        let paused = clock
            .1
            .saturating_add(clock.0.map_or(Duration::ZERO, |started| {
                now.saturating_duration_since(started)
            }));
        Ok(now
            .saturating_duration_since(admitted)
            .saturating_sub(paused)
            >= deadline)
    }
}

#[derive(Clone)]
pub(super) struct StemPairOwner {
    pub root: PathBuf,
    pub descriptor_reference: String,
    pub source_lease: CommittedColdLease,
    pub reader: Arc<super::stem_pair::VerifiedStemPair>,
}

#[derive(Clone)]
pub(super) struct StemOwner {
    pub(super) set: PreparedStemSet,
    pub(super) source_version: String,
    pub(super) cache_dir: PathBuf,
    pub(super) generation_path: PathBuf,
    pub(super) pair: Option<StemPairOwner>,
}

struct StemDescriptor {
    identity: std::sync::Weak<[u8; 32]>,
    source_version: String,
    cache_dir: PathBuf,
    generation_path: PathBuf,
    pair: Option<StemPairOwner>,
}

impl StemOwner {
    fn prepare_window(
        &self,
        complete: &SampleBuffer,
        window: &SampleBuffer,
        rate: u32,
        resident: &super::resident_proposal::ResidentReadContext,
        cancelled: &impl Fn() -> bool,
        preallocated: Option<super::resident_accounting::ResidentReservation>,
    ) -> Result<PreparedStemSet, String> {
        let assets = &resident.assets;
        if let Some(owner) = &self.pair {
            owner
                .source_lease
                .verify_reference(window)
                .map_err(|error| error.to_string())?;
            let pair = &owner.reader;
            if pair.descriptor_reference != owner.descriptor_reference {
                return Err("retained pair descriptor differs from owner".into());
            }
            if pair.descriptor.stem_set_identity
                != self
                    .set
                    .complete_set_identity
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            {
                return Err("complete accepted stem pair changed".into());
            }
            let same_range = self.set.stems.iter().all(|stem| {
                stem.same_source(window)
                    && stem.resident_start() == window.resident_start()
                    && stem.resident_end() == window.resident_end()
            });
            let mut reused = vec![window];
            if same_range {
                reused.extend(self.set.stems.iter());
            }
            let bytes = if same_range {
                0
            } else {
                window
                    .samples
                    .len()
                    .checked_mul(16)
                    .ok_or("resident component window byte overflow")?
            };
            let mut reservation = if let Some(token) = preallocated {
                token
            } else {
                let mut token = resident
                    .reserve(bytes, &reused, 5)
                    .map_err(|error| error.to_string())?;
                token
                    .reserve_reader_slots(1)
                    .map_err(|error| error.to_string())?;
                token
            };
            let stems = if same_range {
                self.set
                    .stems
                    .clone()
                    .map(|stem| {
                        stem.window(
                            window.resident_start(),
                            window.resident_end(),
                            window.window_revision(),
                            window.residency.as_ref().unwrap().context,
                        )
                    })
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>()?
                    .try_into()
                    .map_err(|_| "retained component count differs".to_string())?
            } else {
                pair.prepare_component_views_cancellable(window, cancelled, &reservation)?
            };
            let set = PreparedStemSet {
                complete_set_identity: self.set.complete_set_identity.clone(),
                accepted_timing: self.set.accepted_timing,
                reference_samples: window.samples.clone(),
                reference_residency: window.residency.clone(),
                publication: self.set.publication.clone(),
                source_version_hash: self.set.source_version_hash,
                sample_rate_hz: rate,
                channels: window.channels,
                frame_count: window.frame_count(),
                available_mask: self.set.available_mask,
                stems,
            };
            let mut views = vec![window];
            views.extend(set.stems.iter());
            reservation
                .commit(&views)
                .map_err(|error| error.to_string())?;
            assets
                .retain_stem_pair_reserved(
                    self.generation_path.clone(),
                    super::project_assets::stem_readers::StemPairReaders {
                        reference: window,
                        version: &self.source_version,
                        identity: &set.complete_set_identity,
                        stems: Some(&set),
                    },
                    pair.clone_pins(owner.root.parent().ok_or("pair project root missing")?)?,
                    owner.source_lease.clone(),
                    &mut reservation,
                )
                .map_err(|error| error.to_string())?;
            return Ok(set);
        }
        let path = self.cache_dir.to_str().ok_or("invalid stem cache path")?;
        let bytes = window
            .samples
            .len()
            .checked_mul(16)
            .ok_or("resident legacy component window byte overflow")?;
        let mut reservation = if let Some(token) = preallocated {
            token
        } else {
            let mut token = resident
                .reserve(bytes, &[window], 5)
                .map_err(|error| error.to_string())?;
            token
                .reserve_reader_slots(1)
                .map_err(|error| error.to_string())?;
            token
        };
        let mut set = super::stem_cache::prepare_stem_buffers_from_cache(
            &self.source_version,
            complete,
            rate,
            path,
        )?;
        if *set.complete_set_identity != *self.set.complete_set_identity {
            return Err("complete accepted stem set changed".into());
        }
        set.complete_set_identity = self.set.complete_set_identity.clone();
        let set = set.window_for(window)?;
        let mut views = vec![window];
        views.extend(set.stems.iter());
        reservation
            .commit(&views)
            .map_err(|error| error.to_string())?;
        assets
            .retain_stems_reserved(self.generation_path.clone(), &set, &mut reservation)
            .map_err(|error| error.to_string())?;
        Ok(set)
    }
}

struct PendingWindow {
    sample: Option<SampleBuffer>,
    stems: Option<PreparedStemSet>,
    publication: PreparedSourcePermit,
    expected_revision: u64,
    token: Arc<()>,
    state: Arc<WindowState>,
    admitted: Instant,
}

#[derive(Default)]
pub(super) struct ResidentStemCache {
    pub(super) accepted: Option<StemOwner>,
    pending: Option<StemOwner>,
    relocation: Option<PendingWindow>,
    proposal: Option<PendingWindow>,
    start_readiness: Option<Weak<WindowState>>,
    pub(super) intent_epoch: Arc<AtomicU64>,
    history: Vec<StemDescriptor>,
    pub(super) committed: super::committed_stem_residency::CommittedStemState,
}

/// Small cancellation handle staged before a control claim. Large window owners
/// stay in the cache and are settled only after the shared marker is released.
pub(super) struct WindowPredecessor {
    publication: PreparedSourcePermit,
    state: Arc<WindowState>,
}

impl WindowPredecessor {
    pub(super) fn claim(&self) -> bool {
        self.publication.status() == "rejected" || self.publication.cancel_unclaimed()
    }

    pub(super) fn finish(&self, ownership: &InputRuntimeOwnership, id: usize) {
        self.state.fail(
            ownership,
            id,
            self.publication.expected,
            "resident preparation superseded by committed stem intent".into(),
            true,
        );
    }
}

impl ResidentStemCache {
    pub(super) fn prepare_window_supersession(&self) -> PyResult<Option<WindowPredecessor>> {
        let Some(previous) = self.relocation.as_ref() else {
            return Ok(None);
        };
        if matches!(previous.publication.status(), "adopting" | "accepted") {
            return Err(PyRuntimeError::new_err(
                "resident native adoption is in progress",
            ));
        }
        Ok(Some(WindowPredecessor {
            publication: previous.publication.clone(),
            state: previous.state.clone(),
        }))
    }
    pub(super) fn ordinary_publication_pending(&self) -> bool {
        self.pending.is_some()
    }
    /// Fresh recovery must not fence an original window collector before its
    /// actual control settlement. Call after the existing reconcile path.
    pub(super) fn recovered_custody_settled(&self) -> bool {
        self.pending.is_none() && self.relocation.is_none() && self.proposal.is_none()
    }
    fn remember(&mut self, owner: &StemOwner) -> PyResult<()> {
        self.history
            .retain(|entry| entry.identity.strong_count() > 0);
        let identity = Arc::downgrade(&owner.set.complete_set_identity);
        if self
            .history
            .iter()
            .any(|entry| entry.identity.ptr_eq(&identity))
        {
            return Ok(());
        }
        if self.history.len() >= 128 {
            return Err(PyRuntimeError::new_err(
                "retained stem descriptor bound exceeded",
            ));
        }
        self.history.push(StemDescriptor {
            identity,
            source_version: owner.source_version.clone(),
            cache_dir: owner.cache_dir.clone(),
            generation_path: owner.generation_path.clone(),
            pair: owner.pair.clone(),
        });
        Ok(())
    }

    fn owner_for(&self, set: &PreparedStemSet) -> Option<StemOwner> {
        if let Some(owner) = &self.accepted
            && Arc::ptr_eq(&owner.set.complete_set_identity, &set.complete_set_identity)
        {
            let mut owner = owner.clone();
            owner.set = set.clone();
            return Some(owner);
        }
        let identity = Arc::downgrade(&set.complete_set_identity);
        let descriptor = self
            .history
            .iter()
            .find(|entry| entry.identity.ptr_eq(&identity))?;
        Some(StemOwner {
            set: set.clone(),
            source_version: descriptor.source_version.clone(),
            cache_dir: descriptor.cache_dir.clone(),
            generation_path: descriptor.generation_path.clone(),
            pair: descriptor.pair.clone(),
        })
    }
}

#[derive(Default)]
struct WindowState {
    placement: Option<SlotSnapshot>,
    source_generation: u64,
    preparation_clock: ResidentPreparationClock,
    control_published: AtomicBool,
    terminal: AtomicU8,
    failed_settlement: Mutex<Option<(usize, u64, u8)>>,
    error: Mutex<Option<String>>,
    adopted_binding: Mutex<Option<InputPadBinding>>,
    launch_cancelled: Arc<AtomicBool>,
    start_readiness: OnceLock<SharedStartReadiness>,
    proposal: Mutex<Option<super::resident_proposal::ResidentProposal>>,
    #[cfg(test)]
    terminal_observation: Mutex<Option<terminal_order_tests::TerminalObservation>>,
    #[cfg(test)]
    range_observation: Mutex<Option<WindowReadObservation>>,
    #[cfg(test)]
    control_wait_observed: AtomicBool,
}

/// Actual physical request metadata. WindowState owns this record; it contains
/// no WindowState/attack/Work reference and therefore introduces no Arc cycle.
struct SharedStartReadiness {
    seal: PadStartReadinessSeal,
    publication: PreparedSourcePermit,
    binding: InputPadBinding,
    source_generation: u64,
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
    loop_region: Option<(f64, Option<f64>)>,
    start_frame: usize,
    end_frame: usize,
    full_frame_count: usize,
    window_revision: u64,
    previous_window_revision: u64,
    key_lock_request_id: Option<u64>,
    independent_owner: bool,
    members: Mutex<Vec<Weak<AtomicBool>>>,
}

impl SharedStartReadiness {
    fn register_member(&self) -> PyResult<Arc<AtomicBool>> {
        let mut members = self
            .members
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident member lock poisoned"))?;
        members.retain(|member| member.strong_count() != 0);
        if members.len() >= 256 {
            return Err(PyRuntimeError::new_err(
                "resident start membership capacity exhausted",
            ));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        members.push(Arc::downgrade(&cancelled));
        Ok(cancelled)
    }

    fn has_other_live_member(&self) -> bool {
        self.members.lock().map_or(true, |members| {
            members
                .iter()
                .filter_map(Weak::upgrade)
                .any(|member| !member.load(Ordering::Acquire))
        })
    }
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub(super) struct WindowReadObservation {
    pub source: super::cold_store::WindowReadObservation,
    pub stems: super::stem_pair::PairReadObservation,
    pub admitted_peak_bytes: usize,
}

impl WindowState {
    fn for_placement(placement: SlotSnapshot, source_generation: u64) -> Self {
        Self {
            placement: Some(placement),
            source_generation,
            ..Self::default()
        }
    }

    /// External settlement shares the source/pair marker. Callback tails use their
    /// existing direct-cell completion instead. Pending retains this state for polling.
    fn finish_control(&self, ownership: &InputRuntimeOwnership, _id: usize, expected: u64) -> bool {
        if self.placement.is_some() && !self.control_published.load(Ordering::Acquire) {
            return true;
        }
        let placement = self.placement;
        #[cfg(test)]
        let placement = placement.or_else(|| ownership.slot_snapshot(_id));
        let Some(captured) = placement else {
            return true;
        };
        let observed = if let Some(key) = captured.content_key() {
            ownership.observe_control_key(key)
        } else if ownership.slot_snapshot_current(captured) {
            PlacementObservation::Current(captured)
        } else if ownership.placement_pending() {
            PlacementObservation::Pending
        } else {
            PlacementObservation::Stale
        };
        let current = match observed {
            PlacementObservation::Current(current) => current,
            PlacementObservation::Pending => return false,
            PlacementObservation::Stale | PlacementObservation::Retired => return true,
        };
        let prepared = match ownership.prepare_slot_write_fixed(current, current.lifetime()) {
            Ok(prepared) => prepared,
            Err(_) => return false,
        };
        match ownership.commit_slot_write_fixed(
            &prepared,
            || true,
            |cell| {
                let _ = ownership.resident_control_for_cell(cell).compare_exchange(
                    expected,
                    0,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
            },
        ) {
            Ok(_) => true,
            Err(PlacementError::Retired | PlacementError::Stale) => captured
                .content_key()
                .is_some_and(|key| !ownership.content_key_current(key)),
            Err(_) => false,
        }
    }

    fn retry_failure(&self, ownership: &InputRuntimeOwnership) {
        let Ok(mut settlement) = self.failed_settlement.lock() else {
            return;
        };
        let Some((id, expected, terminal)) = *settlement else {
            return;
        };
        if !self.finish_control(ownership, id, expected) {
            return;
        }
        *settlement = None;
        self.terminal.store(terminal, Ordering::Release);
        drop(settlement);
        #[cfg(test)]
        if let Some(observation) = self.terminal_observation.lock().unwrap().take() {
            observation.observe();
        }
    }

    /// Settle this exact control intent before publishing terminal failure.
    /// This scalar CAS neither clears a replacement intent nor retires job/read pins.
    fn fail(
        &self,
        ownership: &InputRuntimeOwnership,
        id: usize,
        expected: u64,
        error: String,
        cancelled: bool,
    ) {
        if let Ok(mut message) = self.error.lock() {
            *message = Some(error);
        }
        if self
            .proposal
            .lock()
            .is_ok_and(|proposal| proposal.as_ref().is_none_or(|proposal| proposal.armed()))
        {
            if let Ok(mut settlement) = self.failed_settlement.lock() {
                *settlement = Some((id, expected, if cancelled { 2 } else { 1 }));
            }
        } else {
            self.terminal
                .store(if cancelled { 2 } else { 1 }, Ordering::Release);
        }
        self.retry_failure(ownership);
    }
}

/// Destroying an unrun queued job (including shutdown) settles its public state.
struct JobGuard {
    publication: PreparedSourcePermit,
    state: Arc<WindowState>,
    id: usize,
    ownership: Arc<InputRuntimeOwnership>,
    enqueued: bool,
}

impl JobGuard {
    fn mark_enqueued(&mut self) {
        self.enqueued = true;
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        if !self.enqueued
            && self.publication.cancel_unclaimed()
            && self.state.terminal.load(Ordering::Acquire) == 0
        {
            self.state.fail(
                &self.ownership,
                self.id,
                self.publication.expected,
                "resident preparation cancelled before native enqueue".into(),
                true,
            );
        }
    }
}

/// Native ACK is authoritative; polling cannot cause callback adoption.
#[derive(Clone, Debug)]
pub(crate) struct ResidentLaunchGuard {
    // Resident permits contain only fixed atomic Arcs, never PCM,
    // file/evidence leases or collections. Scheduling clones allocate nothing;
    // rejection/execution drops only this bounded scalar metadata.
    publication: ResidentLaunchPermit,
    cancelled: Arc<AtomicBool>,
    member_cancelled: Option<Arc<AtomicBool>>,
}

impl PartialEq for ResidentLaunchGuard {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.publication.epoch, &other.publication.epoch)
            && self.publication.expected == other.publication.expected
            && Arc::ptr_eq(&self.cancelled, &other.cancelled)
            && match (&self.member_cancelled, &other.member_cancelled) {
                (None, None) => true,
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                _ => false,
            }
    }
}

impl ResidentLaunchGuard {
    pub(crate) fn current(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && self
                .member_cancelled
                .as_ref()
                .is_none_or(|member| !member.load(Ordering::Acquire))
            && self.publication.current_accepted()
    }

    /// Only this retained member/root's actual accepted loop commit may bridge
    /// its original target revision. Later source-control changes still refuse.
    pub(crate) fn owns_source_control_transition(&self, original: u64, current: u64) -> bool {
        self.current()
            && self
                .publication
                .owns_source_control_transition(original, current)
    }
}

#[pyclass(frozen)]
pub struct ResidentWindowTicket {
    input_trigger: Option<CapturedInputPadTrigger>,
    pad_start: Option<CapturedPadStart>,
    member_cancelled: Option<Arc<AtomicBool>>,
    publication: PreparedSourcePermit,
    state: Arc<WindowState>,
    #[pyo3(get)]
    window_revision: u64,
    #[pyo3(get)]
    previous_window_revision: u64,
    #[pyo3(get)]
    start_frame: usize,
    #[pyo3(get)]
    end_frame: usize,
    #[pyo3(get)]
    full_frame_count: usize,
    id: usize,
    source_generation: u64,
    binding: InputPadBinding,
    ownership: Arc<InputRuntimeOwnership>,
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
    loop_region: Option<(f64, Option<f64>)>,
    #[pyo3(get)]
    key_lock_request_id: Option<u64>,
}

/// A retained, genuine resident request dependency. Capturing it grants no new
/// window owner and never joins/replaces the singleton resident proposal.
#[derive(Clone)]
pub(crate) struct ResidentStartDependency {
    publication: PreparedSourcePermit,
    state: Arc<WindowState>,
    binding: InputPadBinding,
    ownership: Arc<InputRuntimeOwnership>,
    member_cancelled: Option<Arc<AtomicBool>>,
}

impl ResidentStartDependency {
    pub(crate) fn resolve(
        &self,
        start: &CapturedPadStart,
    ) -> PyResult<Option<(InputPadBinding, ResidentLaunchGuard)>> {
        self.state.retry_failure(&self.ownership);
        if !start.belongs_to(&self.ownership)
            || self.state.terminal.load(Ordering::Acquire) != 0
            || self.state.launch_cancelled.load(Ordering::Acquire)
            || self
                .member_cancelled
                .as_ref()
                .is_some_and(|member| member.load(Ordering::Acquire))
            || self.publication.status() == "rejected"
        {
            return Err(PyValueError::new_err(
                "original global resident dependency rejected",
            ));
        }
        let proposal_current = self
            .state
            .proposal
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident proposal lock poisoned"))?
            .as_ref()
            .is_some_and(|proposal| {
                !self.state.control_published.load(Ordering::Acquire) && proposal.current()
            });
        if !self.publication.current() && !proposal_current {
            return Err(PyValueError::new_err(
                "original global resident dependency superseded",
            ));
        }
        if self.publication.status() != "accepted" {
            return Ok(None);
        }
        let binding = if self.publication.preserved_window() {
            self.binding
        } else {
            let Some(binding) = *self
                .state
                .adopted_binding
                .lock()
                .map_err(|_| PyRuntimeError::new_err("resident binding lock poisoned"))?
            else {
                return Ok(None);
            };
            binding
        };
        let publication = self
            .publication
            .resident_launch_permit()
            .ok_or_else(|| PyValueError::new_err("global resident ACK has no launch permit"))?;
        let guard = ResidentLaunchGuard {
            publication,
            cancelled: self.state.launch_cancelled.clone(),
            member_cancelled: self.member_cancelled.clone(),
        };
        let resolved = match start.preflight_work_with_binding(binding, Some(&guard)) {
            Ok(resolved) => resolved,
            Err(
                super::pitch_actions::PitchActionReject::PlacementPending
                | super::pitch_actions::PitchActionReject::WindowPending,
            ) => return Ok(None),
            Err(reason) => return Err(PyValueError::new_err(reason.as_str())),
        };
        Ok(Some((resolved.binding, guard)))
    }
}

impl ResidentWindowTicket {
    /// Retain this exact already-admitted request at GLOBAL gesture capture.
    pub(crate) fn capture_start_dependency(
        &self,
        start: &CapturedPadStart,
    ) -> PyResult<ResidentStartDependency> {
        if !start.belongs_to(&self.ownership)
            || self.state.terminal.load(Ordering::Acquire) != 0
            || self.state.launch_cancelled.load(Ordering::Acquire)
            || !self.is_current()
            || self
                .loop_region
                .is_some_and(|region| region != start.loop_region())
        {
            return Err(PyValueError::new_err(
                "resident ticket does not own the original global intent",
            ));
        }
        let binding =
            if self.publication.status() == "accepted" && !self.publication.preserved_window() {
                self.state
                    .adopted_binding
                    .lock()
                    .map_err(|_| PyRuntimeError::new_err("resident binding lock poisoned"))?
                    .ok_or_else(|| PyValueError::new_err("resident ACK binding unavailable"))?
            } else {
                self.binding
            };
        let original = start
            .control_guard()
            .map_err(|reason| PyValueError::new_err(reason.as_str()))?;
        if !start.validates_preparation_binding(original.id, binding) {
            return Err(PyValueError::new_err(
                "resident ticket belongs to a different original source",
            ));
        }
        // Key Lock/seek-only tickets have no loop intent. Their actual finite
        // window must cover the complete immutable attack region instead.
        if self.loop_region.is_none() {
            let (from, to) = start.loop_region();
            let rate = f64::from(binding.sample_rate_hz);
            let from = (from * rate).round().max(0.0) as usize;
            let to = to
                .map_or(self.full_frame_count, |end| {
                    (end * rate).round().max(0.0) as usize
                })
                .min(self.full_frame_count);
            if from < self.start_frame || to > self.end_frame || from >= to {
                return Err(PyValueError::new_err(
                    "resident dependency does not cover original global loop",
                ));
            }
        }
        Ok(ResidentStartDependency {
            publication: self.publication.clone(),
            state: self.state.clone(),
            binding: self.binding,
            ownership: self.ownership.clone(),
            member_cancelled: self.member_cancelled.clone(),
        })
    }

    /// Only the original captured start can use this ticket's actual WindowACK.
    pub(super) fn capture_pad_start_launch_guard(
        &self,
        start: &CapturedPadStart,
    ) -> PyResult<Option<(InputPadBinding, ResidentLaunchGuard)>> {
        if self
            .pad_start
            .as_ref()
            .is_none_or(|original| !original.same_owner(start))
            || self.publication.status() != "accepted"
            || !self.is_current()
            || self.state.launch_cancelled.load(Ordering::Acquire)
            || self
                .member_cancelled
                .as_ref()
                .is_some_and(|member| member.load(Ordering::Acquire))
            || self.loop_region != Some(start.loop_region())
        {
            return Ok(None);
        }
        let binding = if self.publication.preserved_window() {
            self.binding
        } else {
            let Some(binding) = *self
                .state
                .adopted_binding
                .lock()
                .map_err(|_| PyRuntimeError::new_err("resident binding lock poisoned"))?
            else {
                return Ok(None);
            };
            binding
        };
        let Some(publication) = self.publication.resident_launch_permit() else {
            return Ok(None);
        };
        Ok(Some((
            binding,
            ResidentLaunchGuard {
                publication,
                cancelled: self.state.launch_cancelled.clone(),
                member_cancelled: self.member_cancelled.clone(),
            },
        )))
    }

    #[cfg(test)]
    pub(super) fn control_published_for_test(&self) -> bool {
        self.state.control_published.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(super) fn control_wait_observed_for_test(&self) -> bool {
        self.state.control_wait_observed.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(super) fn key_lock_request_id_for_test(&self) -> Option<u64> {
        self.key_lock_request_id
    }

    #[cfg(test)]
    pub(super) fn read_observation_for_test(&self) -> Option<WindowReadObservation> {
        self.state.range_observation.lock().unwrap().clone()
    }
    /// Legacy launch metadata only; callers convert it to prepared work off realtime.
    /// This scalar TriggerInputPad value must never be published to the audio queue.
    pub(super) fn launch_message(
        &self,
        exclusive: bool,
        received_at_ns: u64,
    ) -> Option<(ControlMessage, input_runtime_binding::PreparedLaunchWrite)> {
        if self.input_trigger.is_some()
            || self.pad_start.is_some()
            || self.publication.status() != "accepted"
            || !self.is_current()
            || self.state.launch_cancelled.load(Ordering::Acquire)
        {
            return None;
        }
        let (start_s, end_s) = self.loop_region?;
        let binding = if self.publication.preserved_window() {
            self.binding
        } else {
            (*self.state.adopted_binding.lock().ok()?)?
        };
        let placement = binding
            .placement
            .or_else(|| self.ownership.slot_snapshot(self.id))?;
        let prepared = self
            .ownership
            .prepare_launch_write(placement, input_runtime_binding::LaunchWriteKind::Admit)
            .ok()?;
        Some((
            ControlMessage::TriggerInputPad {
                id: self.id,
                start_s,
                end_s,
                exclusive,
                binding,
                received_at_ns,
                resident_control: Some(ResidentLaunchGuard {
                    publication: self.publication.resident_launch_permit()?,
                    cancelled: self.state.launch_cancelled.clone(),
                    member_cancelled: self.member_cancelled.clone(),
                }),
                launch_revision: prepared.previous_revision,
            },
            prepared,
        ))
    }

    /// All locks and payload preparation precede entry; this uses only original scalar owners.
    fn launch_binding_current(&self, binding: InputPadBinding) -> bool {
        let ownership = &self.ownership;
        let placement_current =
            binding
                .placement
                .map_or(!ownership.is_source_tracked(), |placement| {
                    placement.slot() == self.id
                        && placement.content_key().is_some()
                        && ownership.slot_snapshot_fields_current(placement)
                });
        placement_current
            && self.publication.status() == "accepted"
            && self.publication.current()
            && !self.state.launch_cancelled.load(Ordering::Acquire)
            && ownership.copy_reservation(self.id) == 0
            && ownership.authority_current(self.id, binding)
            && ownership.binding_source_generation(self.id, binding) == Some(self.source_generation)
            && ownership.binding_window_current(self.id, binding)
            && self
                .acknowledgements
                .current_for_binding(ownership, self.id, binding)
                == binding
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
    }
}

pub(super) fn launch_with_producer(
    engine: &AudioEngine,
    ticket: &ResidentWindowTicket,
    exclusive: bool,
    received_at_ns: u64,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
) -> PyResult<bool> {
    launch_with_producer_before_commit(engine, ticket, exclusive, received_at_ns, producer, || {})
}

/// Original MIDI fallback uses the same launch publication, with its own actual WindowACK.
pub(super) fn launch_captured_with_producer(
    engine: &AudioEngine,
    trigger: &CapturedInputPadTrigger,
    ticket: &ResidentWindowTicket,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
) -> PyResult<bool> {
    if !Arc::ptr_eq(&ticket.ownership, &engine.input_runtime_ownership)
        || !trigger.belongs_to(&engine.input_runtime_ownership)
        || ticket
            .input_trigger
            .as_ref()
            .is_none_or(|original| !original.same_owner(trigger))
        || ticket.publication.status() != "accepted"
        || !ticket.is_current()
        || ticket.state.launch_cancelled.load(Ordering::Acquire)
        || ticket.loop_region != Some(trigger.loop_region())
    {
        return Ok(false);
    }
    let Some((window_binding, resident_guard)) =
        ticket.capture_pad_start_launch_guard(trigger.pad_start())?
    else {
        return Ok(false);
    };
    let Some(guard) = trigger.launch_guard(Some(window_binding))? else {
        return Ok(false);
    };
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err("resident command queue is full"));
    }
    let Some(placement) = guard.binding.placement else {
        return Ok(false);
    };
    let prepared = ticket
        .ownership
        .prepare_launch_write(placement, input_runtime_binding::LaunchWriteKind::Admit)
        .map_err(PyRuntimeError::new_err)?;
    let Some(work) = trigger
        .pad_start()
        .take_work_with_binding(guard.binding, Some(resident_guard))
        .map_err(|reason| PyRuntimeError::new_err(reason.as_str()))?
    else {
        return Ok(false);
    };
    let publication = Arc::new(OrdinaryStartPublication::new(work));
    let message = ControlMessage::PreparedStart(publication.clone());
    #[cfg(test)]
    control_publication_tests::before_commit();
    let committed = ticket
        .ownership
        .commit_launch_write(
            prepared,
            || {
                guard.current()
                    && ticket.publication.status() == "accepted"
                    && ticket.publication.current()
                    && !ticket.state.launch_cancelled.load(Ordering::Acquire)
                    && ticket.ownership.slot_snapshot_fields_current(placement)
                    && ticket.ownership.copy_reservation(guard.id) == 0
                    && ticket.ownership.authority_current(guard.id, guard.binding)
                    && ticket
                        .ownership
                        .binding_source_generation(guard.id, guard.binding)
                        == Some(ticket.source_generation)
                    && ticket
                        .ownership
                        .binding_window_current(guard.id, guard.binding)
                    && ticket.acknowledgements.current_for_binding(
                        &ticket.ownership,
                        guard.id,
                        guard.binding,
                    ) == guard
                        .binding
                        .accepted
                        .map_or(0, |accepted| accepted.publication_epoch)
                    && trigger.pad_start().mark_published_claimed()
            },
            || {
                guard.mark_admitted();
                producer
                    .push(message)
                    .expect("reserved MIDI resident capacity");
            },
        )
        .is_ok();
    if !committed
        && let Some(work) = publication.take()
        && let Err(work) = trigger.pad_start().restore_unpublished_work(work)
    {
        trigger
            .pad_start()
            .reject(PitchActionReject::PreparedMismatch);
        drop(work);
    }
    Ok(committed)
}

fn launch_with_producer_before_commit(
    engine: &AudioEngine,
    ticket: &ResidentWindowTicket,
    exclusive: bool,
    received_at_ns: u64,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    before_commit: impl FnOnce(),
) -> PyResult<bool> {
    if !Arc::ptr_eq(&ticket.ownership, &engine.input_runtime_ownership) {
        return Ok(false);
    }
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    if producer.slots() < 2 {
        return Err(PyRuntimeError::new_err(
            "resident command queue requires timing and prepared start capacity",
        ));
    }
    let Some((message, prepared)) = ticket.launch_message(exclusive, received_at_ns) else {
        return Ok(false);
    };
    let ControlMessage::TriggerInputPad {
        binding,
        start_s,
        end_s,
        resident_control: Some(resident_guard),
        ..
    } = message
    else {
        unreachable!("resident launch preparation retains its exact native guard");
    };
    let start = CapturedPadStart::capture_parts(
        binding,
        ticket.ownership.clone(),
        ticket.acknowledgements.clone(),
        start_s,
        end_s,
        1.0,
        exclusive,
        received_at_ns,
        engine.ordinary_start_render_format()?,
        None,
    )
    .map_err(|reason| PyRuntimeError::new_err(reason.as_str()))?;
    let Some(work) = start
        .take_work_with_binding(binding, Some(resident_guard))
        .map_err(|reason| PyRuntimeError::new_err(reason.as_str()))?
    else {
        return Ok(false);
    };
    let target = ControlMessage::CapturePadStartTarget(start.clone());
    let publication = Arc::new(OrdinaryStartPublication::new(work));
    let message = ControlMessage::PreparedStart(publication.clone());
    before_commit();
    let committed = engine
        .input_runtime_ownership
        .commit_launch_write(
            prepared,
            || ticket.launch_binding_current(binding) && start.mark_published_claimed(),
            || {
                producer
                    .push(target)
                    .expect("reserved legacy resident target capacity");
                producer
                    .push(message)
                    .expect("reserved legacy resident prepared start capacity");
            },
        )
        .is_ok();
    if !committed
        && let Some(work) = publication.take()
        && let Err(work) = start.restore_unpublished_work(work)
    {
        start.reject(PitchActionReject::PreparedMismatch);
        drop(work);
    }
    Ok(committed)
}

#[cfg(test)]
#[path = "resident_launch_publication_tests.rs"]
mod resident_launch_publication_tests;

#[pymethods]
impl ResidentWindowTicket {
    /// Revoke this ticket's starts while preserving its acknowledged window.
    /// The flag belongs only to this ticket, so stale cancellation cannot affect a newer owner.
    pub fn cancel_launch(&self) -> bool {
        !self
            .member_cancelled
            .as_ref()
            .unwrap_or(&self.state.launch_cancelled)
            .swap(true, Ordering::AcqRel)
    }

    /// The adopted target comes from the actual voice's complete native extent.
    #[getter]
    pub fn effective_seek_seconds(&self) -> Option<f64> {
        (self.publication.status() == "accepted")
            .then(|| self.publication.resident_seek_seconds())
            .flatten()
    }

    pub fn publication_status(&self) -> &'static str {
        if self
            .member_cancelled
            .as_ref()
            .is_some_and(|member| member.load(Ordering::Acquire))
        {
            return "cancelled";
        }
        self.state.retry_failure(&self.ownership);
        if self
            .state
            .failed_settlement
            .lock()
            .is_ok_and(|pending| pending.is_some())
        {
            return "pending";
        }
        match self.state.terminal.load(Ordering::Acquire) {
            1 => "failed",
            2 => "cancelled",
            _ => match self.publication.status() {
                "captured" => "preparing",
                status => status,
            },
        }
    }

    pub fn error(&self) -> PyResult<Option<String>> {
        self.state
            .error
            .lock()
            .map(|error| error.clone())
            .map_err(|_| PyRuntimeError::new_err("resident error lock poisoned"))
    }

    /// An ACK alone cannot promote a ticket after unload, replacement or a newer intent.
    pub fn is_current(&self) -> bool {
        if self
            .member_cancelled
            .as_ref()
            .is_some_and(|member| member.load(Ordering::Acquire))
        {
            return false;
        }
        let binding =
            if self.publication.status() == "accepted" && !self.publication.preserved_window() {
                let Ok(adopted) = self.state.adopted_binding.lock() else {
                    return false;
                };
                let Some(binding) = *adopted else {
                    return false;
                };
                binding
            } else {
                self.binding
            };
        self.publication.status() != "rejected"
            && (self.publication.current()
                || self.state.proposal.lock().is_ok_and(|proposal| {
                    proposal.as_ref().is_some_and(|proposal| {
                        !self.state.control_published.load(Ordering::Acquire) && proposal.current()
                    })
                }))
            && self.ownership.authority_current(self.id, binding)
            && self.ownership.binding_source_generation(self.id, binding)
                == Some(self.source_generation)
            && self.ownership.binding_window_current(self.id, binding)
            && self
                .acknowledgements
                .current_for_binding(&self.ownership, self.id, binding)
                == binding
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
    }

    /// A claimed callback tail cannot be cancelled or rolled back by metadata.
    pub fn cancel(&self) -> bool {
        if let Some(member) = &self.member_cancelled {
            let changed = !member.swap(true, Ordering::AcqRel);
            if self.state.start_readiness.get().is_some_and(|readiness| {
                readiness.independent_owner || readiness.has_other_live_member()
            }) {
                return changed;
            }
        }
        if let Ok(proposal) = self.state.proposal.lock()
            && let Some(proposal) = proposal.as_ref()
            && !proposal.armed()
        {
            proposal.cancel();
        }
        let cancelled = self.publication.cancel_unclaimed();
        if cancelled {
            self.state.fail(
                &self.ownership,
                self.id,
                self.publication.expected,
                "resident preparation cancelled".into(),
                true,
            );
        }
        cancelled
    }
}

/// Called only after source/ticket validation and reserved producer capacity.
/// Register the pending generation and retire old resident intent before the
/// publication can be observed. Only fixed epoch guards enter queued launches.
pub(super) fn admit_stem_publication(
    engine: &AudioEngine,
    id: usize,
    set: &PreparedStemSet,
    source_version: &str,
    registration: Option<(PathBuf, PathBuf, Option<StemPairOwner>)>,
) -> PyResult<bool> {
    let ownership = &engine.input_runtime_ownership;
    let resolved = match set
        .publication
        .control_placement(engine)
        .map_err(PyValueError::new_err)?
    {
        PlacementObservation::Current(resolved) => resolved,
        PlacementObservation::Pending => return Ok(false),
        _ => {
            return Err(PyValueError::new_err(
                "admitted stem publication content owner changed",
            ));
        }
    };
    if id >= NUM_SAMPLES
        || resolved.slot() != id
        || !set.publication.current()
        || ownership.copy_reservation(id) != 0
    {
        return Err(PyValueError::new_err(
            "admitted stem publication source or copy owner changed",
        ));
    }
    let mut owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident stem ownership lock poisoned"))?;
    let owner = &mut owners[id];
    let intent_epoch = owner.intent_epoch.clone();
    let original_intent = owner.intent_epoch.load(Ordering::Acquire);
    let next_intent =
        super::prepared_source::next_epoch(&owner.intent_epoch).map_err(PyRuntimeError::new_err)?;
    let predecessor = owner.prepare_window_supersession()?;
    let mut prepared_owner = registration.map(|(cache_dir, generation_path, pair)| StemOwner {
        set: set.clone(),
        source_version: source_version.to_owned(),
        cache_dir,
        generation_path,
        pair,
    });
    let write = match ownership.prepare_slot_write_fixed(resolved, resolved.lifetime()) {
        Ok(write) => write,
        Err(PlacementError::Pending | PlacementError::Stale) => return Ok(false),
        Err(error) => return Err(PyValueError::new_err(error.to_string())),
    };
    let mut retired_owner = None;
    let outcome = ownership.commit_slot_write_fixed(
        &write,
        || {
            ownership.slot_snapshot_fields_current(resolved)
                && ownership.control_slot_for_cell(resolved.cell()) == id
                && resolved.content_key() == set.publication.content_key()
                && set.publication.current()
                && set.publication.status() == "captured"
                && ownership.copy_reservation(id) == 0
                && intent_epoch.load(Ordering::Acquire) == original_intent
                && predecessor.as_ref().is_none_or(|previous| {
                    matches!(
                        previous.publication.status(),
                        "captured" | "pending" | "rejected"
                    )
                })
                && set.publication.mark_pending().is_ok()
        },
        |_| {
            if let Some(previous) = predecessor.as_ref() {
                previous.publication.cancel_unclaimed();
            }
            intent_epoch.store(next_intent, Ordering::Release);
            if let Some(next) = prepared_owner.take() {
                retired_owner = owner.pending.replace(next);
            }
        },
    );
    drop(owners);
    drop(retired_owner);
    match outcome {
        Ok(_) => {
            if let Some(previous) = predecessor {
                previous.finish(ownership, id);
            }
            Ok(true)
        }
        Err(PlacementError::Pending | PlacementError::Stale) => Ok(false),
        Err(error) => Err(PyValueError::new_err(error.to_string())),
    }
}

pub(super) fn set_stem_pair_full_mix_with_producer(
    engine: &AudioEngine,
    id: usize,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
) -> PyResult<()> {
    if id >= NUM_SAMPLES {
        return Err(PyValueError::new_err("id out of range"));
    }
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    engine
        .input_runtime_ownership
        .ensure_no_copy_reservation(id)?;
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("control queue lock poisoned"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err("stem full-mix queue is full"));
    }
    let mut owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident stem ownership lock poisoned"))?;
    let owner = &mut owners[id];
    let next_intent =
        super::prepared_source::next_epoch(&owner.intent_epoch).map_err(PyRuntimeError::new_err)?;
    if let Some(previous) = owner.accepted.clone() {
        owner.remember(&previous)?;
    }
    let retired = [
        owner.accepted.as_ref().map(|value| value.set.clone()),
        owner.pending.as_ref().map(|value| value.set.clone()),
    ];
    producer
        .push(ControlMessage::SetStemPairFullMix { id, retired })
        .map_err(|_| PyRuntimeError::new_err("stem full-mix queue is full"))?;
    owner.intent_epoch.store(next_intent, Ordering::Release);
    // The queued fixed handles, callback bank and actual jobs retain the PCM.
    // No potentially final large-buffer destruction occurs in this control path.
    owner.accepted = None;
    owner.pending = None;
    Ok(())
}

pub(super) fn reconcile(engine: &AudioEngine) -> PyResult<()> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    reconcile_context_under_request_lock(
        &super::resident_control_context::ResidentControlContext::capture(engine),
    )
}

#[cfg(test)]
pub(super) fn history_count_for_test(engine: &AudioEngine, id: usize) -> usize {
    engine.resident_stem_cache.lock().unwrap()[id].history.len()
}

pub(super) fn cancel_all(engine: &AudioEngine) -> PyResult<()> {
    let owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident ownership lock poisoned"))?;
    for (id, owner) in owners.iter().enumerate() {
        owner.committed.cancel_pending_shared();
        if let Some(proposal) = &owner.proposal
            && proposal.publication.cancel_unclaimed()
        {
            if let Ok(token) = proposal.state.proposal.lock()
                && let Some(token) = token.as_ref()
            {
                token.cancel();
            }
            proposal.state.fail(
                &engine.input_runtime_ownership,
                id,
                proposal.publication.expected,
                "resident proposal cancelled by shutdown".into(),
                true,
            );
        }
        if let Some(pending) = &owner.relocation
            && pending.publication.cancel_unclaimed()
        {
            pending.state.fail(
                &engine.input_runtime_ownership,
                id,
                pending.publication.expected,
                "resident preparation cancelled by shutdown".into(),
                true,
            );
        }
    }
    Ok(())
}

/// A real Window ACK can arrive after Native Pair placement but before the complete
/// control mirror. Settle only that accepted window into its still-owning custody row.
/// The original key and every adopted source/timing field remain the authority;
/// no fresh ticket, slot occupant or PCM equality can authorize this catch-up.
fn catch_up_pair_window_custody(
    engine: &super::resident_control_context::ResidentControlContext,
    id: usize,
    pending: &PendingWindow,
    cache: &mut Option<SampleBuffer>,
    accepted: &mut Option<StemOwner>,
) -> PyResult<bool> {
    let ownership = &engine.input_runtime_ownership;
    let Some(key) = pending
        .state
        .placement
        .and_then(|placement| placement.content_key())
    else {
        return Ok(false);
    };
    if !matches!(
        ownership.observe_control_key(key),
        PlacementObservation::Pending
    ) || ownership.control_slot_for_cell(key.cell()) != id
    {
        return Ok(false);
    }
    let PlacementObservation::Current(native) = ownership.observe_content_key(key) else {
        return Ok(false);
    };
    if native.slot() == id || pending.publication.preserved_window() {
        return Ok(false);
    }
    let Some(next) = pending.sample.as_ref() else {
        return Ok(false);
    };
    let adopted = *pending
        .state
        .adopted_binding
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident binding lock poisoned"))?;
    let Some(binding) = adopted else {
        return Ok(false);
    };
    let owns_cache = cache.as_ref().is_some_and(|sample| {
        sample.same_source(next) && sample.window_revision() == pending.expected_revision
    });
    let owns_binding = binding
        .placement
        .and_then(|placement| placement.content_key())
        == Some(key)
        && binding.resident == next.resident_binding()
        && binding.source_address == next.source_address()
        && binding.sample_count == next.source_sample_count()
        && binding.channels == next.channels;
    if !owns_cache || !owns_binding || pending.state.source_generation == 0 {
        return Ok(false);
    }
    let write = match ownership.prepare_slot_write_fixed(native, native.lifetime()) {
        Ok(write) => write,
        Err(PlacementError::Pending | PlacementError::Stale | PlacementError::Retired) => {
            return Ok(false);
        }
        Err(error) => return Err(PyRuntimeError::new_err(error.to_string())),
    };
    let replacement = next.clone();
    let stems = pending.stems.clone();
    let mut retired_sample = None;
    let mut retired_stems = None;
    let outcome = ownership.commit_slot_write_fixed(
        &write,
        || {
            owns_cache
                && owns_binding
                && ownership.slot_snapshot_fields_current(native)
                && ownership.control_slot_for_cell(key.cell()) == id
                && pending.state.control_published.load(Ordering::Acquire)
                && pending.publication.status() == "accepted"
                && pending.publication.current()
                && ownership.authority_current(id, binding)
                && ownership.binding_source_generation(id, binding)
                    == Some(pending.state.source_generation)
                && ownership.binding_window_current(id, binding)
                && engine
                    .current_timing_acknowledgements
                    .current_for_binding(ownership, id, binding)
                    == binding
                        .accepted
                        .map_or(0, |accepted| accepted.publication_epoch)
        },
        |_| {
            retired_sample = cache.replace(replacement);
            if let Some(owner) = accepted.as_mut()
                && let Some(stems) = stems
                && Arc::ptr_eq(
                    &owner.set.complete_set_identity,
                    &stems.complete_set_identity,
                )
            {
                retired_stems = Some(std::mem::replace(&mut owner.set, stems));
            }
        },
    );
    drop(retired_sample);
    drop(retired_stems);
    match outcome {
        Ok(_) => Ok(true),
        Err(
            PlacementError::Pending
            | PlacementError::Stale
            | PlacementError::Retired
            | PlacementError::ClaimRejected,
        ) => Ok(false),
        Err(error) => Err(PyRuntimeError::new_err(error.to_string())),
    }
}

/// Mirror only this original transaction's observed storage effect. Mode refusal is
/// independent: the rejected ticket remains rejected and cannot authorize any launch.
/// False means a publication/mirror marker is temporarily unavailable; retain all owners.
fn reconcile_claimed_resident_storage(
    engine: &super::resident_control_context::ResidentControlContext,
    id: usize,
    pending: &PendingWindow,
    cache: &mut Option<SampleBuffer>,
    accepted: &mut Option<StemOwner>,
) -> PyResult<bool> {
    let ownership = &engine.input_runtime_ownership;
    let Some(key) = pending
        .state
        .placement
        .and_then(|placement| placement.content_key())
    else {
        return Ok(true);
    };
    let native = match ownership.observe_content_key(key) {
        PlacementObservation::Current(native) => native,
        PlacementObservation::Pending => return Ok(false),
        _ => return Ok(true),
    };
    if ownership.control_slot_for_cell(key.cell()) != id {
        return Ok(false);
    }
    let Some(next) = pending.sample.as_ref() else {
        return Ok(true);
    };
    let Some(binding) = *pending
        .state
        .adopted_binding
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident binding lock poisoned"))?
    else {
        return Ok(false);
    };
    let owns_binding = binding
        .placement
        .and_then(|placement| placement.content_key())
        == Some(key)
        && binding.resident == next.resident_binding()
        && binding.source_address == next.source_address()
        && binding.sample_count == next.source_sample_count()
        && binding.channels == next.channels;
    let owns_cache = cache.as_ref().is_some_and(|sample| {
        sample.same_source(next)
            && (sample.window_revision() == pending.expected_revision
                || (sample.same_window(next) && Arc::ptr_eq(&sample.samples, &next.samples)))
    });
    let source_current = || {
        owns_binding
            && owns_cache
            && pending.state.source_generation != 0
            && pending.publication.resident_storage_adopted()
            && pending.state.control_published.load(Ordering::Acquire)
            && ownership.slot_snapshot_fields_current(native)
            && ownership.control_slot_for_cell(key.cell()) == id
            // A later BPM/grid edit revokes launch/timing authority, not this
            // receipt's already observed physical storage transfer. Requiring
            // its old authority here can orphan the cache at the predecessor
            // window forever. Exact content/source/window custody still fences
            // both this preflight and the mirror's fixed publication claim.
            && ownership.binding_source_generation(id, binding)
                == Some(pending.state.source_generation)
            && ownership.binding_window_current(id, binding)
    };
    if !source_current() {
        // A replacement no longer owns this exact physical window. Never install an
        // old receipt over it, even when source bytes or numerical revisions agree.
        return Ok(true);
    }
    let write = match ownership.prepare_slot_write_fixed(native, native.lifetime()) {
        Ok(write) => write,
        Err(PlacementError::Pending | PlacementError::Stale) => return Ok(false),
        Err(PlacementError::Retired) => return Ok(true),
        Err(error) => return Err(PyRuntimeError::new_err(error.to_string())),
    };
    let replacement = next.clone();
    let stems = pending.stems.clone();
    let mut old_sample = None;
    let mut old_stems = None;
    let result = ownership.commit_slot_write_fixed(&write, source_current, |_| {
        old_sample = cache.replace(replacement);
        if let Some(owner) = accepted.as_mut()
            && let Some(stems) = stems
            && Arc::ptr_eq(
                &owner.set.complete_set_identity,
                &stems.complete_set_identity,
            )
        {
            old_stems = Some(std::mem::replace(&mut owner.set, stems));
        }
    });
    // All destruction is on the control thread under its existing request/cache locks.
    drop(old_sample);
    drop(old_stems);
    match result {
        Ok(_) => Ok(true),
        Err(PlacementError::Pending | PlacementError::Stale | PlacementError::ClaimRejected) => {
            Ok(false)
        }
        Err(PlacementError::Retired) => Ok(true),
        Err(error) => Err(PyRuntimeError::new_err(error.to_string())),
    }
}

pub(super) fn reconcile_context(
    engine: &super::resident_control_context::ResidentControlContext,
) -> PyResult<()> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    reconcile_context_under_request_lock(engine)
}
pub(super) fn reconcile_context_under_request_lock(
    engine: &super::resident_control_context::ResidentControlContext,
) -> PyResult<()> {
    let mut cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let mut owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident stem ownership lock poisoned"))?;
    for id in 0..NUM_SAMPLES {
        let owner = &mut owners[id];
        if let Some(proposal) = owner.proposal.as_ref() {
            let mirror_pending = !proposal.state.control_published.load(Ordering::Acquire)
                && proposal
                    .state
                    .placement
                    .and_then(|placement| placement.content_key())
                    .is_some_and(|key| {
                        matches!(
                            engine.input_runtime_ownership.observe_control_key(key),
                            PlacementObservation::Pending
                        )
                    });
            if proposal
                .state
                .preparation_clock
                .expired_at(
                    proposal.admitted,
                    mirror_pending,
                    ADOPTION_DEADLINE,
                    Instant::now(),
                )
                .map_err(PyRuntimeError::new_err)?
                && proposal.publication.cancel_unclaimed()
            {
                proposal.state.fail(
                    &engine.input_runtime_ownership,
                    id,
                    proposal.publication.expected,
                    "resident proposal deadline expired".into(),
                    true,
                );
            }
            let current =
                proposal.state.proposal.lock().is_ok_and(|proposal| {
                    proposal.as_ref().is_some_and(|proposal| proposal.current())
                });
            if !current || proposal.publication.status() == "rejected" {
                owner.proposal = None;
            }
        }
        owner
            .committed
            .reconcile(&mut owner.accepted, cache[id].as_ref());
        // Historical file leases are justified only by a real logical set
        // still held by a voice, queued transaction or job.
        owner
            .history
            .retain(|entry| entry.identity.strong_count() > 0);
        if let Some(pending) = owner.pending.as_ref() {
            match pending.set.publication.status() {
                "accepted" => {
                    if let Some(old) = owner.accepted.clone() {
                        owner.remember(&old)?;
                    }
                    owner.accepted = owner.pending.take();
                }
                "rejected" => {
                    owner.pending = None;
                }
                _ => {}
            }
        }
        if let Some(pending) = owner.relocation.as_ref() {
            if pending.admitted.elapsed() >= ADOPTION_DEADLINE
                && pending.publication.cancel_unclaimed()
            {
                pending.state.fail(
                    &engine.input_runtime_ownership,
                    id,
                    pending.publication.expected,
                    "resident native adoption deadline expired".into(),
                    true,
                );
            }
            let status = pending.publication.status();
            if matches!(status, "accepted" | "rejected")
                && pending.publication.resident_storage_adopted()
            {
                if !reconcile_claimed_resident_storage(
                    engine,
                    id,
                    pending,
                    &mut cache[id],
                    &mut owner.accepted,
                )? || !pending.state.finish_control(
                    &engine.input_runtime_ownership,
                    id,
                    pending.publication.expected,
                ) {
                    continue;
                }
                owner.relocation = None;
                continue;
            }
            if status == "accepted" {
                if catch_up_pair_window_custody(
                    engine,
                    id,
                    pending,
                    &mut cache[id],
                    &mut owner.accepted,
                )? {
                    continue;
                }
                if !pending.state.finish_control(
                    &engine.input_runtime_ownership,
                    id,
                    pending.publication.expected,
                ) {
                    continue;
                }
                let current = pending.sample.as_ref().is_some_and(|next| {
                    cache[id].as_ref().is_some_and(|sample| {
                        sample.same_source(next)
                            && sample.window_revision() == pending.expected_revision
                    })
                });
                let pending = owner.relocation.take().expect("checked pending window");
                if current && !pending.publication.preserved_window() {
                    cache[id] = pending.sample;
                    if let Some(accepted) = &mut owner.accepted
                        && let Some(stems) = pending.stems
                        && Arc::ptr_eq(
                            &accepted.set.complete_set_identity,
                            &stems.complete_set_identity,
                        )
                    {
                        accepted.set = stems;
                    }
                }
            } else if status == "rejected" {
                pending.state.retry_failure(&engine.input_runtime_ownership);
                if !pending.state.finish_control(
                    &engine.input_runtime_ownership,
                    id,
                    pending.publication.expected,
                ) {
                    continue;
                }
                owner.relocation = None;
            }
        }
        if owner.accepted.as_ref().is_some_and(|accepted| {
            !cache[id].as_ref().is_some_and(|sample| {
                accepted
                    .set
                    .stems
                    .first()
                    .is_some_and(|stem| sample.same_source(stem))
            })
        }) {
            if let Some(old) = owner.accepted.clone() {
                owner.remember(&old)?;
            }
            owner.accepted = None;
        }
    }
    Ok(())
}

/// Prepared control publication keeps original scalars in their captured cells.
struct ResidentControlWrite {
    placement: SlotSnapshot,
    original_intent: u64,
    original_control: u64,
    original_request: u64,
    next_intent: u64,
    next_request: Option<u64>,
    predecessor: Option<PreparedSourcePermit>,
}

impl ResidentControlWrite {
    fn capture(
        binding: &InputRuntimePadBinding,
        intent: u64,
        next_intent: u64,
        key_lock: bool,
    ) -> Result<Self, String> {
        let placement = binding
            .binding
            .placement
            .ok_or("resident source has no captured content placement")?;
        let original_request = binding
            .ownership
            .key_lock_request_for_cell(placement.cell())
            .load(Ordering::Acquire);
        let next_request = if key_lock {
            Some(
                original_request
                    .checked_add(1)
                    .ok_or("Key Lock request exhausted")?,
            )
        } else {
            None
        };
        Ok(Self {
            placement,
            original_intent: intent,
            original_control: binding
                .ownership
                .resident_control_for_cell(placement.cell())
                .load(Ordering::Acquire),
            original_request,
            next_intent,
            next_request,
            predecessor: None,
        })
    }

    fn with_predecessor(mut self, predecessor: Option<PreparedSourcePermit>) -> Self {
        self.predecessor = predecessor;
        self
    }

    fn original_control_current(&self, ownership: &InputRuntimeOwnership) -> bool {
        let actual = ownership
            .resident_control_for_cell(self.placement.cell())
            .load(Ordering::Acquire);
        actual == self.original_control
            || (actual == 0
                && self.original_control != 0
                && self.predecessor.as_ref().is_some_and(|previous| {
                    previous.expected == self.original_control
                        && matches!(previous.status(), "accepted" | "rejected")
                }))
    }

    /// Only the captured predecessor may cancel. A completed callback keeps its ACK;
    /// it may have cleared its own control counter just before this mutation marker.
    fn claim_predecessor(&self, cancelled: &mut bool) -> bool {
        let Some(previous) = self.predecessor.as_ref() else {
            return true;
        };
        if previous.cancel_unclaimed() {
            *cancelled = true;
            return true;
        }
        matches!(previous.status(), "accepted" | "rejected")
    }

    fn commit(
        &self,
        binding: &InputRuntimePadBinding,
        intent_epoch: &AtomicU64,
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(),
    ) -> Result<(), PlacementError> {
        let ownership = &binding.ownership;
        let key = self.placement.content_key().ok_or(PlacementError::Stale)?;
        let resolved = match ownership.observe_control_key(key) {
            PlacementObservation::Current(resolved) => resolved,
            PlacementObservation::Pending => return Err(PlacementError::Pending),
            PlacementObservation::Stale => return Err(PlacementError::Stale),
            PlacementObservation::Retired => return Err(PlacementError::Retired),
        };
        self.commit_at(binding, intent_epoch, resolved, claim, apply)
    }

    fn commit_at(
        &self,
        binding: &InputRuntimePadBinding,
        intent_epoch: &AtomicU64,
        resolved: SlotSnapshot,
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(),
    ) -> Result<(), PlacementError> {
        let ownership = &binding.ownership;
        if resolved.content_key() != self.placement.content_key() {
            return Err(PlacementError::Stale);
        }
        let prepared = ownership.prepare_slot_write_fixed(resolved, resolved.lifetime())?;
        #[cfg(test)]
        control_publication_tests::before_commit();
        ownership.commit_slot_write_fixed(
            &prepared,
            || {
                binding.admitted_publication_current(resolved)
                    && intent_epoch.load(Ordering::Acquire) == self.original_intent
                    && self.original_control_current(ownership)
                    && ownership
                        .key_lock_request_for_cell(self.placement.cell())
                        .load(Ordering::Acquire)
                        == self.original_request
                    && claim()
            },
            |cell| {
                intent_epoch.store(self.next_intent, Ordering::Release);
                ownership
                    .resident_control_for_cell(cell)
                    .store(self.next_intent, Ordering::Release);
                if let Some(request) = self.next_request {
                    ownership
                        .key_lock_request_for_cell(cell)
                        .store(request, Ordering::Release);
                }
                apply();
            },
        )?;
        Ok(())
    }
}

struct WindowWork {
    id: usize,
    old: SampleBuffer,
    bank: SampleBuffer,
    seek_receiver: Option<rtrb::Consumer<super::resident_seek::ResidentSeekPin>>,
    seek_pin: Option<super::resident_seek::ResidentSeekPin>,
    start: usize,
    end: usize,
    revision: u64,
    context: ResidentContext,
    intent: ResidentControlIntent,
    _original_request: u64,
    binding: InputRuntimePadBinding,
    control: ResidentControlWrite,
    source_lease: CommittedColdLease,
    stem_owner: Option<StemOwner>,
    requests: Arc<Mutex<Vec<u64>>>,
    cache: Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    owners: Arc<Mutex<Vec<ResidentStemCache>>>,
    preparation_gate: Arc<Mutex<()>>,
    assets: Arc<super::project_assets::ProjectAssets>,
    pcm_history: Arc<super::project_assets::PcmHistory>,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    publication: PreparedSourcePermit,
    state: Arc<WindowState>,
    token: Arc<()>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    proposal: super::resident_proposal::ResidentProposal,
    intent_epoch: Arc<AtomicU64>,
    base_intent: u64,
    resident: super::resident_proposal::ResidentReadContext,
}

#[cfg(test)]
fn admitted_window_peak(
    old: &SampleBuffer,
    start: usize,
    end: usize,
    stems: Option<&StemOwner>,
) -> Result<usize, String> {
    admitted_window_peak_with_readers(old, start, end, stems, &[])
}

fn admitted_window_peak_with_readers(
    old: &SampleBuffer,
    start: usize,
    end: usize,
    stems: Option<&StemOwner>,
    readers: &[Arc<[f32]>],
) -> Result<usize, String> {
    let frames = old.frame_count();
    let window_bytes = end
        .checked_sub(start)
        .filter(|frames| *frames > 0)
        .ok_or("invalid resident range")?
        .checked_mul(old.channels)
        .and_then(|n| n.checked_mul(4))
        .ok_or("resident PCM geometry overflow")?;
    if start >= end || end > frames {
        return Err("resident range exceeds complete source".into());
    }
    let mut backing = vec![old.samples.clone()];
    for samples in readers {
        if !backing.iter().any(|held| Arc::ptr_eq(held, samples)) {
            backing.push(samples.clone());
        }
    }
    if let Some(owner) = stems {
        for samples in std::iter::once(&owner.set.reference_samples)
            .chain(owner.set.stems.iter().map(|stem| &stem.samples))
        {
            if !backing.iter().any(|held| Arc::ptr_eq(held, samples)) {
                backing.push(samples.clone());
            }
        }
    }
    let held = backing.iter().try_fold(0_usize, |sum, samples| {
        samples
            .len()
            .checked_mul(4)
            .and_then(|bytes| sum.checked_add(bytes))
            .ok_or("resident held PCM overlap overflow")
    })?;
    let peak = if stems.is_some_and(|owner| owner.pair.is_none()) {
        // Legacy conversion still owns its old set while constructing complete
        // derivatives and the new crops. Those real old backings are additional
        // to the converter's reference/decode/alignment peak.
        super::stem_cache::admitted_stem_pcm_bytes(frames, old.channels)?
            .checked_add(held)
            .and_then(|bytes| {
                window_bytes
                    .checked_mul(5)
                    .and_then(|windows| bytes.checked_add(windows))
            })
            .and_then(|bytes| bytes.checked_add(64 * 1024))
            .ok_or("resident PCM overlap overflow")?
    } else {
        let source_new = if old.resident_start() == start && old.resident_end() == end {
            0
        } else {
            window_bytes
        };
        let stems_new = if let Some(owner) = stems {
            if owner.set.stems.iter().all(|stem| {
                stem.same_source(old)
                    && stem.resident_start() == start
                    && stem.resident_end() == end
            }) {
                0
            } else {
                window_bytes
                    .checked_mul(4)
                    .ok_or("resident component overlap overflow")?
            }
        } else {
            0
        };
        held.checked_add(source_new)
            .and_then(|bytes| bytes.checked_add(stems_new))
            .and_then(|bytes| {
                bytes.checked_add(if source_new != 0 || stems_new != 0 {
                    64 * 1024
                } else {
                    0
                })
            })
            .ok_or("resident PCM overlap overflow")?
    };
    if stems.is_some_and(|owner| owner.pair.is_none()) && peak > super::cold_jobs::PCM_LIMIT_BYTES {
        return Err("resident preparation exceeds the 1-GiB transient PCM admission".into());
    }
    Ok(peak)
}

fn ensure_window_history_capacity(
    history: &mut Vec<std::sync::Weak<[f32]>>,
    old: &SampleBuffer,
    start: usize,
    end: usize,
) -> Result<(), String> {
    history.retain(|reader| reader.strong_count() > 0);
    let reuses_registered_backing = start == old.resident_start()
        && end == old.resident_end()
        && history
            .iter()
            .any(|reader| reader.ptr_eq(&Arc::downgrade(&old.samples)));
    if !reuses_registered_backing && history.len() >= 128 {
        return Err("same-pad source reader history full (128 live assignments)".into());
    }
    Ok(())
}

impl WindowWork {
    fn current(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && self.proposal.current()
            && self.publication.status() != "rejected"
            && (if self.state.control_published.load(Ordering::Acquire) {
                self.publication.current()
            } else {
                self.intent_epoch.load(Ordering::Acquire) == self.base_intent
            })
            && self.binding.admitted_current()
    }

    fn observe_control(&self) -> PlacementObservation<SlotSnapshot> {
        self.control
            .placement
            .content_key()
            .map_or(PlacementObservation::Stale, |key| {
                self.binding.ownership.observe_control_key(key)
            })
    }

    /// The cache lock pins the current custody row; Pending never holds it while waiting.
    fn current_owners(
        &self,
    ) -> Result<
        (
            std::sync::MutexGuard<'_, Vec<ResidentStemCache>>,
            SlotSnapshot,
        ),
        String,
    > {
        loop {
            if !self.current() {
                return Err("resident source/window was superseded".into());
            }
            let owners = self
                .owners
                .lock()
                .map_err(|_| "resident ownership lock poisoned")?;
            match self.observe_control() {
                PlacementObservation::Current(resolved) => {
                    let proposed = owners[resolved.slot()]
                        .proposal
                        .as_ref()
                        .filter(|proposal| {
                            Arc::ptr_eq(&proposal.token, &self.token)
                                && Arc::ptr_eq(&proposal.state, &self.state)
                                && proposal.expected_revision == self.bank.window_revision()
                        })
                        .ok_or("resident proposal/source/window ownership changed")?;
                    if !self.current() || proposed.publication.status() == "rejected" {
                        return Err("resident source/window was superseded".into());
                    }
                    self.state
                        .preparation_clock
                        .observe_at(false, Instant::now())?;
                    return Ok((owners, resolved));
                }
                PlacementObservation::Pending => {
                    self.state
                        .preparation_clock
                        .observe_at(true, Instant::now())?;
                    #[cfg(test)]
                    self.state
                        .control_wait_observed
                        .store(true, Ordering::Release);
                    drop(owners);
                }
                PlacementObservation::Stale | PlacementObservation::Retired => {
                    return Err("resident content owner was retired".into());
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn prepare(&mut self) -> Result<(), String> {
        if let Some(mut receiver) = self.seek_receiver.take() {
            let deadline = Instant::now() + ADOPTION_DEADLINE;
            let pin = loop {
                if self.publication.status() == "accepted" {
                    return Ok(());
                }
                if !self.current() {
                    return Err("resident seek capture was cancelled or superseded".into());
                }
                if let Ok(pin) = receiver.pop() {
                    break pin;
                }
                if Instant::now() >= deadline {
                    return Err("resident seek capture deadline expired".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            if !pin.sample.same_source(&self.bank) {
                self.source_lease = self
                    .assets
                    .cold_lease_for_reader(&pin.sample)
                    .map_err(|error| error.to_string())?;
                self.old = pin.sample.clone();
                self.stem_owner = pin
                    .stems
                    .as_ref()
                    .map(|set| {
                        let (owners, resolved) = self.current_owners()?;
                        owners[resolved.slot()].owner_for(set).ok_or_else(|| {
                            "captured stem set has no retained immutable descriptor".to_string()
                        })
                    })
                    .transpose()?;
                self.revision = self
                    .old
                    .window_revision()
                    .checked_add(1)
                    .ok_or("voice window revision exhausted")?;
            }
            // Capture owns the actual voice view even when its complete source
            // equals a newer bank window. Coverage and reuse follow that view.
            self.old = pin.sample.clone();
            self.seek_pin = Some(pin.clone());
            if pin.stopped {
                self.start = self.old.resident_start();
                self.end = self.old.resident_end();
                self.revision = self.old.window_revision();
                self.context = self
                    .old
                    .residency
                    .as_ref()
                    .ok_or("stopped source view missing")?
                    .context;
            } else if super::source_reader::resident_read_context_available(
                &pin.sample,
                pin.loop_region,
                super::source_reader::explicit_seek_mode_for_frame(
                    ((self.intent.seek_position_s.expect("captured seek")
                        * f64::from(self.binding.binding.sample_rate_hz))
                    .round() as usize)
                        .min(pin.sample.frame_count()),
                    pin.loop_region,
                    pin.sample.frame_count(),
                ),
                pin.key_lock,
            ) {
                self.start = pin.sample.resident_start();
                self.end = pin.sample.resident_end();
                self.revision = pin.sample.window_revision();
                self.context = pin
                    .sample
                    .residency
                    .as_ref()
                    .ok_or("captured source view missing")?
                    .context;
            } else
            // A finite KEY LOCK voice owns its actual physical loop, including after bank
            // replacement. Admit its prefix/loop or loop/tail union, never a guessed halo.
            // Complete-backed and non-KEY seeks retain the explicit complete-track exception.
            if pin.key_lock
                && pin
                    .sample
                    .residency
                    .as_ref()
                    .is_some_and(|view| view.context.permits_finite_range())
            {
                let target = (self.intent.seek_position_s.expect("captured seek")
                    * f64::from(self.binding.binding.sample_rate_hz))
                .round() as usize;
                let mode = super::source_reader::explicit_seek_mode_for_frame(
                    target.min(pin.sample.frame_count()),
                    pin.loop_region,
                    pin.sample.frame_count(),
                );
                let range = super::native_source_coverage::source_context_range(
                    pin.sample.frame_count(),
                    pin.loop_region,
                    mode,
                )
                .ok_or("invalid captured seek context")?;
                (self.start, self.end) = (range.start, range.end);
                self.context = ResidentContext::KeyLockFiniteLoop;
            } else {
                self.start = 0;
                self.end = self.old.frame_count();
                self.context = if pin.key_lock {
                    ResidentContext::KeyLockFullTrack
                } else {
                    ResidentContext::FullTrack
                };
            }
        }
        if !self.current() {
            return Err("resident source/window was superseded".into());
        }
        // Committed components and window peaks retain their existing cross-worker serialization.
        // Always acquire the worker-only gate BEFORE owners; controls never acquire it.
        let gate = self.preparation_gate.clone();
        let preparation = super::resident_proposal::lock_preparation(&gate, || self.current())?;
        // Pin the original custody row and all promises before the first allocation.
        let (preparation_owners, preparation_slot) = self.current_owners()?;
        let resident = self
            .resident
            .clone()
            .with_history(self.pcm_history.clone(), preparation_slot.slot());
        let held_readers = self
            .assets
            .held_same_pad_reader_backings(
                &self.pcm_history,
                preparation_slot.slot(),
                &self.source_lease,
            )
            .map_err(|error| error.to_string())?;
        let peak = admitted_window_peak_with_readers(
            &self.old,
            self.start,
            self.end,
            self.stem_owner.as_ref(),
            &held_readers,
        )?;
        let reader_pins = self
            .assets
            .pin_resident_backings(&self.resident.account, &held_readers)
            .map_err(|error| error.to_string())?;
        // Only the accounting token retains this historical snapshot from here on.
        drop(held_readers);
        let window_bytes = (self.end - self.start)
            .checked_mul(self.old.channels)
            .and_then(|n| n.checked_mul(4))
            .ok_or("resident window byte overflow")?;
        let source_reused =
            self.start == self.old.resident_start() && self.end == self.old.resident_end();
        // Register the immutable original witness first; final views get their own slots.
        let original = self
            .resident
            .reserve(0, &[&self.old], 1)
            .map_err(|error| error.to_string())?;
        let mut source_reservation = resident
            .reserve(
                if source_reused { 0 } else { window_bytes },
                &if source_reused {
                    vec![&self.old]
                } else {
                    Vec::new()
                },
                if source_reused { 2 } else { 1 },
            )
            .map_err(|error| error.to_string())?;
        source_reservation
            .reserve_cold_reader_slot(&self.source_lease, source_reused.then_some(&self.old))
            .map_err(|error| error.to_string())?;
        let component_reservation = self
            .stem_owner
            .as_ref()
            .map(|owner| {
                let same = owner.pair.is_some()
                    && owner.set.stems.iter().all(|stem| {
                        stem.same_source(&self.old)
                            && stem.resident_start() == self.start
                            && stem.resident_end() == self.end
                    });
                let reused: Vec<_> = if same {
                    owner.set.stems.iter().collect()
                } else {
                    Vec::new()
                };
                let bytes = if same {
                    0
                } else {
                    window_bytes
                        .checked_mul(4)
                        .ok_or("resident component byte overflow")?
                };
                // Components do not allocate a source history entry. Its source token already owns it.
                let mut token = self
                    .resident
                    .reserve(bytes, &reused, 9)
                    .map_err(|error| error.to_string())?;
                token
                    .reserve_reader_slots(1)
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>(token)
            })
            .transpose()?;
        // Legacy conversion additionally owns a complete reference and complete
        // converter/alignment buffers before cropping. Promise that whole envelope
        // before even the selected source read; the existing legacy cap stays above.
        let transient = self
            .stem_owner
            .as_ref()
            .filter(|owner| owner.pair.is_none())
            .map(|_| {
                self.resident
                    .account
                    .reserve_transient(super::stem_cache::admitted_stem_pcm_bytes(
                        self.old.frame_count(),
                        self.old.channels,
                    )?)
                    .map_err(|error| error.to_string())
            })
            .transpose()?;
        if !self.current() {
            return Err("resident source/window superseded before allocation".into());
        }
        drop(preparation_owners);
        #[cfg(test)]
        {
            super::cold_store::reset_window_read_observation_for_test();
            super::stem_pair::reset_range_observation_for_test();
            super::native_owned_work::observe_preparation(
                super::native_owned_work::WorkPoint::WindowSourceRead,
            );
        }
        let sample = self
            .source_lease
            .read_window_preallocated(
                &self.old,
                self.start,
                self.end,
                self.revision,
                self.context,
                0,
                &|| !self.current(),
                Some(&resident),
                Some(&mut source_reservation),
            )
            .map_err(|error| error.to_string())?;
        drop(self.current_owners()?);
        self.assets
            .retain_cold_window_reserved(&self.source_lease, &sample, &mut source_reservation)
            .map_err(|error| error.to_string())?;
        drop(self.current_owners()?);
        let stems = self
            .stem_owner
            .as_ref()
            .map(|owner| -> Result<PreparedStemSet, String> {
                // Only an unconverted legacy set still needs its complete WAV
                // converter. Accepted paired sets use their held PCM reader.
                let legacy_complete = if owner.pair.is_none() {
                    Some(
                        self.source_lease
                            .read_complete_cancellable(
                                &self.old,
                                super::cold_jobs::PCM_LIMIT_BYTES,
                                &|| !self.current(),
                            )
                            .map_err(|error| error.to_string())?,
                    )
                } else {
                    None
                };
                let mut set = owner.prepare_window(
                    legacy_complete.as_ref().unwrap_or(&sample),
                    &sample,
                    self.binding.binding.sample_rate_hz,
                    &resident,
                    &|| !self.current(),
                    component_reservation,
                )?;
                set.complete_set_identity = owner.set.complete_set_identity.clone();
                set.accepted_timing = self
                    .seek_pin
                    .as_ref()
                    .map_or(self.binding.binding.accepted, |pin| pin.timing.accepted);
                set.publication = owner.set.publication.clone();
                // Paired preparation already registered these four PCM owners,
                // their source lease and the paired WAV/PCM/common files.
                Ok(set)
            })
            .transpose()?;
        #[cfg(test)]
        {
            *self.state.range_observation.lock().unwrap() = Some(WindowReadObservation {
                source: super::cold_store::window_read_observation_for_test(),
                stems: super::stem_pair::range_observation_for_test(),
                admitted_peak_bytes: peak,
            });
        }
        #[cfg(not(test))]
        let _ = peak;
        drop(self.current_owners()?);
        drop(transient);
        drop(source_reservation);
        drop(original);
        drop(reader_pins);
        drop(preparation);
        self.publish_prepared(sample, stems)
    }

    fn publish_prepared(
        &self,
        sample: SampleBuffer,
        stems: Option<PreparedStemSet>,
    ) -> Result<(), String> {
        let mut adopted_binding = self.binding.binding;
        adopted_binding.resident = sample.resident_binding();
        let mut prepared_views = Some((sample.clone(), stems.clone()));
        let mut message = Some(ControlMessage::RelocateResident(Box::new(
            crate::messages::ResidentTransaction {
                id: self.id,
                sample,
                stems,
                binding: self.binding.binding,
                publication: self.publication.clone(),
                expected_window_revision: self.bank.window_revision(),
                intent: self.intent,
                seek_pin: self.seek_pin.clone(),
            },
        )));
        let mut pending = None;
        let mut queue_attempts = 0;
        loop {
            if !self.current() {
                return Err("resident preparation superseded before publication".into());
            }
            // Physical request rows stay physical. The original source epoch/generation
            // fences freshness; this lock only serializes cache/mirror publication.
            let requests = self.requests.lock().map_err(|_| "request lock poisoned")?;
            let cache = self
                .cache
                .lock()
                .map_err(|_| "sample cache lock poisoned")?;
            let mut producer = self.producer.lock().map_err(|_| "producer lock poisoned")?;
            let mut owners = self
                .owners
                .lock()
                .map_err(|_| "resident ownership lock poisoned")?;
            let resolved = match self.observe_control() {
                PlacementObservation::Current(resolved) => {
                    self.state
                        .preparation_clock
                        .observe_at(false, Instant::now())?;
                    resolved
                }
                PlacementObservation::Pending => {
                    self.state
                        .preparation_clock
                        .observe_at(true, Instant::now())?;
                    #[cfg(test)]
                    self.state
                        .control_wait_observed
                        .store(true, Ordering::Release);
                    drop(owners);
                    drop(producer);
                    drop(cache);
                    drop(requests);
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                _ => return Err("resident content owner was retired".into()),
            };
            let slot = resolved.slot();
            if !self.current()
                || !cache[slot]
                    .as_ref()
                    .is_some_and(|current| current.same_window(&self.bank))
            {
                return Err("stale resident source/window preparation".into());
            }
            if producer.is_full() {
                queue_attempts += 1;
                if queue_attempts >= 8 {
                    return Err("resident command queue is full".into());
                }
                drop(owners);
                drop(producer);
                drop(cache);
                drop(requests);
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            if self.intent_epoch.load(Ordering::Acquire) != self.base_intent
                || !self.proposal.current()
                || owners[slot].pending.is_some()
                || owners[slot].committed.claimed()
                || owners[slot]
                    .relocation
                    .as_ref()
                    .is_some_and(|pending| pending.publication.status() == "adopting")
            {
                return Err("resident proposal authority changed before arming".into());
            }
            let proposed = owners[slot]
                .proposal
                .as_ref()
                .filter(|pending| Arc::ptr_eq(&pending.token, &self.token))
                .ok_or("resident proposal no longer owns preparation")?;
            if proposed.publication.status() == "rejected" {
                return Err("resident proposal cancelled before arming".into());
            }
            if pending.is_none() {
                let (sample, stems) = prepared_views.take().expect("one prepared window payload");
                pending = Some(PendingWindow {
                    sample: Some(sample),
                    stems,
                    publication: proposed.publication.clone(),
                    expected_revision: proposed.expected_revision,
                    token: proposed.token.clone(),
                    state: proposed.state.clone(),
                    admitted: proposed.admitted,
                });
                self.publication.mark_pending()?;
                self.proposal.arm()?;
            }
            let mut adopted = self
                .state
                .adopted_binding
                .lock()
                .map_err(|_| "resident binding lock poisoned")?;
            let mut displaced = None;
            let mut proposal_owner = None;
            let mut predecessor_cancelled = false;
            pending.as_mut().expect("held window payload").admitted = Instant::now();
            let outcome = self.control.commit_at(
                &self.binding,
                &self.intent_epoch,
                resolved,
                || {
                    !self.cancelled.load(Ordering::Acquire)
                        && self.proposal.current()
                        && self.publication.status() == "pending"
                        && self.control.claim_predecessor(&mut predecessor_cancelled)
                },
                || {
                    displaced = owners[slot]
                        .relocation
                        .replace(pending.take().expect("held window payload"));
                    proposal_owner = owners[slot].proposal.take();
                    *adopted = Some(adopted_binding);
                    self.state.control_published.store(true, Ordering::Release);
                    producer
                        .push(message.take().expect("held resident message"))
                        .expect("reserved single-producer capacity");
                },
            );
            drop(adopted);
            drop(owners);
            drop(producer);
            drop(cache);
            drop(requests);
            if predecessor_cancelled && let Some(previous) = displaced.as_ref() {
                previous.state.fail(
                    &self.binding.ownership,
                    slot,
                    previous.publication.expected,
                    "resident preparation superseded by admitted newer intent".into(),
                    true,
                );
            }
            // Final owners and failed payloads are released outside the marker.
            drop(proposal_owner);
            drop(displaced);
            match outcome {
                Ok(()) => return Ok(()),
                Err(PlacementError::Pending) => std::thread::sleep(Duration::from_millis(1)),
                Err(PlacementError::Stale)
                    if self.current()
                        && !self.binding.ownership.slot_snapshot_current(resolved) =>
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    }
}

/// Prepare only storage which covers effective active geometry. New geometry and
/// nonresident seeks use the separate finite-readiness control stage.
pub(super) fn relocate(
    engine: &AudioEngine,
    id: usize,
    start_s: f64,
    end_s: f64,
) -> PyResult<ResidentWindowTicket> {
    let handle = engine
        .stream_handle
        .as_ref()
        .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
    relocate_with_producer(engine, id, start_s, end_s, handle.producer.clone())
}

pub(super) fn relocate_with_producer(
    engine: &AudioEngine,
    id: usize,
    start_s: f64,
    end_s: f64,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
) -> PyResult<ResidentWindowTicket> {
    prepare_window_with_producer(
        engine,
        id,
        WindowRequest {
            storage_range: Some((start_s, end_s)),
            ..WindowRequest::default()
        },
        producer,
    )
}

#[derive(Default, Clone, Copy)]
pub(super) struct WindowRequest {
    pub(super) storage_range: Option<(f64, f64)>,
    pub(super) loop_region: Option<(f64, Option<f64>)>,
    pub(super) seek_position_s: Option<f64>,
    pub(super) key_lock: Option<bool>,
    pub(super) expected_source_generation: Option<u64>,
}

pub(super) fn prepare_control(
    engine: &AudioEngine,
    id: usize,
    request: WindowRequest,
) -> PyResult<ResidentWindowTicket> {
    prepare_window_with_producer(engine, id, request, engine.control_producer()?.clone())
}

pub(super) fn prepare_window_with_producer(
    engine: &AudioEngine,
    id: usize,
    request: WindowRequest,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
) -> PyResult<ResidentWindowTicket> {
    prepare_window_with_producer_and_trigger(engine, id, request, producer, None, None)
}

pub(super) fn prepare_captured_control(
    engine: &AudioEngine,
    trigger: &CapturedInputPadTrigger,
) -> PyResult<ResidentWindowTicket> {
    if let Some(ticket) = join_captured_readiness(engine, trigger.pad_start(), Some(trigger), None)?
    {
        trigger.note_window_prepared();
        return Ok(ticket);
    }
    let id = trigger.preparation_slot(engine)?;
    let ticket = prepare_window_with_producer_and_trigger(
        engine,
        id,
        WindowRequest {
            loop_region: Some(trigger.loop_region()),
            ..WindowRequest::default()
        },
        engine.control_producer()?.clone(),
        Some(trigger),
        Some(trigger.pad_start()),
    )?;
    trigger.note_window_prepared();
    Ok(ticket)
}

pub(super) fn prepare_captured_pad_start_with_dependency(
    engine: &AudioEngine,
    start: &CapturedPadStart,
    dependency: Option<&ResidentWindowTicket>,
) -> PyResult<ResidentWindowTicket> {
    if let Some(ticket) = join_captured_readiness(engine, start, None, dependency)? {
        start.note_window_prepared();
        return Ok(ticket);
    }
    let id = start.preparation_slot(engine)?;
    let ticket = prepare_window_with_producer_and_trigger(
        engine,
        id,
        WindowRequest {
            loop_region: Some(start.loop_region()),
            ..WindowRequest::default()
        },
        engine.control_producer()?.clone(),
        None,
        Some(start),
    )?;
    start.note_window_prepared();
    Ok(ticket)
}

/// Register a distinct original attack with a retained physical request BEFORE
/// strict old-window admission or reconciliation can retire its discovery row.
fn join_captured_readiness(
    engine: &AudioEngine,
    start: &CapturedPadStart,
    input_trigger: Option<&CapturedInputPadTrigger>,
    dependency: Option<&ResidentWindowTicket>,
) -> PyResult<Option<ResidentWindowTicket>> {
    if !start.belongs_to(&engine.input_runtime_ownership) {
        return Err(PyValueError::new_err("foreign captured pad start"));
    }
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let id = start
        .diagnostic_slot()
        .ok_or_else(|| PyValueError::new_err("captured start placement unavailable"))?;
    let state = match dependency {
        Some(ticket) => {
            if !Arc::ptr_eq(&ticket.ownership, &engine.input_runtime_ownership)
                || ticket
                    .member_cancelled
                    .as_ref()
                    .is_some_and(|member| member.load(Ordering::Acquire))
            {
                return Err(PyValueError::new_err(
                    "original resident dependency unavailable",
                ));
            }
            Some(ticket.state.clone())
        }
        None => engine
            .resident_stem_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident ownership lock poisoned"))?[id]
            .start_readiness
            .as_ref()
            .and_then(Weak::upgrade),
    };
    let Some(state) = state else {
        if dependency.is_some() {
            return Err(PyValueError::new_err(
                "original resident dependency unavailable",
            ));
        }
        return Ok(None);
    };
    let Some(readiness) = state.start_readiness.get() else {
        if dependency.is_some() {
            return Err(PyValueError::new_err(
                "original resident dependency has no Native readiness owner",
            ));
        }
        return Ok(None);
    };
    // Independent controller edits can only be joined via the exact ticket
    // retained at gesture capture; a later matching edit is never discovered.
    if dependency.is_none() && readiness.independent_owner {
        return Ok(None);
    }
    let proposal_current = state
        .proposal
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident proposal lock poisoned"))?
        .as_ref()
        .is_some_and(|proposal| {
            !state.control_published.load(Ordering::Acquire) && proposal.current()
        });
    let current = state.terminal.load(Ordering::Acquire) == 0
        && !state.launch_cancelled.load(Ordering::Acquire)
        && readiness.publication.status() != "rejected"
        && (readiness.publication.current() || proposal_current);
    let adopted = if readiness.publication.status() == "accepted" {
        *state
            .adopted_binding
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident binding lock poisoned"))?
    } else {
        None
    };
    let (from, to) = start.loop_region();
    let rate = f64::from(readiness.binding.sample_rate_hz);
    let from_frame = (from * rate).round().max(0.0) as usize;
    let to_frame = to
        .map_or(readiness.full_frame_count, |end| {
            (end * rate).round().max(0.0) as usize
        })
        .min(readiness.full_frame_count);
    let same_context = readiness.loop_region.map_or(
        from_frame >= readiness.start_frame
            && to_frame <= readiness.end_frame
            && from_frame < to_frame,
        |region| region == start.loop_region(),
    );
    if !current || !same_context || !start.matches_readiness_seal(&readiness.seal, adopted) {
        if dependency.is_some() {
            return Err(PyValueError::new_err(
                "original resident dependency source, timing or loop changed",
            ));
        }
        return Ok(None);
    }
    let member_cancelled = readiness.register_member()?;
    Ok(Some(ResidentWindowTicket {
        input_trigger: input_trigger.cloned(),
        pad_start: Some(start.clone()),
        member_cancelled: Some(member_cancelled),
        publication: readiness.publication.clone(),
        state: state.clone(),
        window_revision: readiness.window_revision,
        previous_window_revision: readiness.previous_window_revision,
        start_frame: readiness.start_frame,
        end_frame: readiness.end_frame,
        full_frame_count: readiness.full_frame_count,
        id,
        source_generation: readiness.source_generation,
        binding: readiness.binding,
        ownership: engine.input_runtime_ownership.clone(),
        acknowledgements: readiness.acknowledgements.clone(),
        loop_region: Some(start.loop_region()),
        key_lock_request_id: readiness.key_lock_request_id,
    }))
}

fn prepare_window_with_producer_and_trigger(
    engine: &AudioEngine,
    id: usize,
    request: WindowRequest,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    input_trigger: Option<&CapturedInputPadTrigger>,
    pad_start: Option<&CapturedPadStart>,
) -> PyResult<ResidentWindowTicket> {
    if id >= NUM_SAMPLES
        || request.storage_range.is_some_and(|(start, end)| {
            !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start
        })
        || request.loop_region.is_some_and(|(start, end)| {
            !start.is_finite()
                || start < 0.0
                || end.is_some_and(|end| !end.is_finite() || end < 0.0)
        })
        || request
            .seek_position_s
            .is_some_and(|position| !position.is_finite() || position < 0.0)
    {
        return Err(PyValueError::new_err("invalid resident source range"));
    }
    reconcile(engine)?;
    let seek_only = request.seek_position_s.is_some()
        && request.loop_region.is_none()
        && request.key_lock.is_none()
        && request.storage_range.is_none();
    if producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?
        .is_full()
    {
        return Err(PyRuntimeError::new_err("resident command queue is full"));
    }
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let id = if let Some(trigger) = input_trigger {
        trigger.preparation_slot(engine)?
    } else if let Some(start) = pad_start {
        start.preparation_slot(engine)?
    } else {
        id
    };
    engine
        .input_runtime_ownership
        .ensure_no_copy_reservation(id)?;
    let source_epoch = engine
        .prepared_source_epoch(id)
        .map_err(PyRuntimeError::new_err)?;
    let binding = input_runtime_binding::capture_under_request_lock(engine, id)?;
    input_runtime_binding::validate_expected_source_generation(
        binding.as_ref(),
        request.expected_source_generation,
    )?;
    let binding = binding
        .filter(|current| (seek_only || current.available()) && current.current())
        .ok_or_else(|| PyValueError::new_err("current complete source/timing unavailable"))?;
    if input_trigger
        .is_some_and(|trigger| !trigger.validates_preparation_binding(id, binding.binding))
        || pad_start.is_some_and(|start| !start.validates_preparation_binding(id, binding.binding))
    {
        return Err(PyValueError::new_err(
            "captured MIDI preparation source or intent changed",
        ));
    }
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let old = cache[id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
    let view = old
        .residency
        .as_ref()
        .ok_or_else(|| PyValueError::new_err("complete cache residency is unavailable"))?;
    let rate = binding.binding.sample_rate_hz;
    let mut intent = ResidentControlIntent {
        loop_region: request.loop_region.map(|(start, end)| {
            let start = (start * f64::from(rate)).round() as usize;
            let end = end
                .map(|end| ((end * f64::from(rate)).round() as usize).max(start.saturating_add(1)));
            (start, end)
        }),
        loop_is_launch_readiness: pad_start.is_some(),
        seek_position_s: request.seek_position_s,
        key_lock: request.key_lock,
        key_lock_request_id: None,
    };
    let key_lock = request.key_lock.unwrap_or(matches!(
        view.context,
        ResidentContext::KeyLockFullTrack | ResidentContext::KeyLockFiniteLoop
    ));
    let (start, end, context) = if request.seek_position_s.is_some() {
        (
            0,
            old.frame_count(),
            if key_lock {
                ResidentContext::KeyLockFullTrack
            } else {
                ResidentContext::FullTrack
            },
        )
    } else if let Some((start, end)) = intent.loop_region {
        let region = super::source_reader::effective_loop_region(start, end, old.frame_count())
            .ok_or_else(|| PyValueError::new_err("invalid complete source loop"))?;
        (
            region.start,
            region.end,
            if key_lock {
                super::cold_residency::key_lock_context_for_range(
                    region.start,
                    region.end,
                    old.frame_count(),
                )
            } else if region.start == 0 && region.end == old.frame_count() {
                ResidentContext::FullTrack
            } else {
                ResidentContext::FiniteLoop
            },
        )
    } else if let Some((start_s, end_s)) = request.storage_range {
        let start = (start_s * f64::from(rate)).round() as usize;
        let end = (end_s * f64::from(rate)).round() as usize;
        (
            start,
            end,
            if key_lock {
                super::cold_residency::key_lock_context_for_range(start, end, old.frame_count())
            } else if start == 0 && end == old.frame_count() {
                ResidentContext::FullTrack
            } else {
                ResidentContext::FiniteLoop
            },
        )
    } else if request.key_lock.is_some() {
        // A mode change prepares the actual installed physical extent. It is
        // neither an implicit loop edit nor permission to widen finite readers.
        let context = if key_lock {
            super::cold_residency::key_lock_context_for_range(
                old.resident_start(),
                old.resident_end(),
                old.frame_count(),
            )
        } else if view.context.permits_finite_range() {
            ResidentContext::FiniteLoop
        } else {
            ResidentContext::FullTrack
        };
        (old.resident_start(), old.resident_end(), context)
    } else if key_lock && !view.context.permits_finite_range() {
        (0, old.frame_count(), ResidentContext::KeyLockFullTrack)
    } else {
        (old.resident_start(), old.resident_end(), view.context)
    };
    if start >= end || end > old.frame_count() {
        return Err(PyValueError::new_err("resident range exceeds full source"));
    }
    let source_lease = engine
        .cold_leases
        .lock()
        .map_err(|_| PyRuntimeError::new_err("cold lease lock poisoned"))?[id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("complete source lease unavailable"))?;
    // Admission and the immediate voice-capture command share exclusive producer
    // capacity before a previous intent can be superseded.
    let mut admission_producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    if admission_producer.is_full() {
        return Err(PyRuntimeError::new_err("resident command queue is full"));
    }
    let mut owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident ownership lock poisoned"))?;
    if owners[id].pending.is_some() {
        return Err(PyRuntimeError::new_err(
            "resident stem publication is pending",
        ));
    }
    // Identical storage reuses already owned immutable PCM. It still queues the
    // existing native transaction and requires its real callback ACK before launch.
    // No seek capture or changed context can enter this worker-free branch.
    let reuse_window = request.seek_position_s.is_none()
        && start == old.resident_start()
        && end == old.resident_end()
        && context == view.context
        && binding.current()
        && old.resident_binding() == binding.binding.resident;
    let ready_stems = if reuse_window {
        owners[id]
            .accepted
            .as_ref()
            .map(|owner| {
                let mut set = owner.set.clone();
                // Accepted source PCM follows the actual acknowledged projection;
                // native timing adoption may have refreshed it since owner capture.
                set.accepted_timing = binding.binding.accepted;
                if !super::source_reader::prepared_stem_set_matches_sample(
                    &set,
                    &old,
                    old.channels,
                    rate as f32,
                    old.frame_count(),
                ) {
                    return Err(PyValueError::new_err(
                        "accepted resident stem/source window differs",
                    ));
                }
                Ok(set)
            })
            .transpose()?
    } else {
        None
    };
    // Required heavy preparation retains its original capacity-before-supersession
    // rule. A valid ready start never consumes or waits for a cold-lane slot.
    let reservation = if reuse_window {
        engine
            .cold_jobs
            .ensure_open()
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        None
    } else {
        let reservation = engine
            .cold_jobs
            .reserve()
            .map_err(PyRuntimeError::new_err)?;
        if !seek_only {
            {
                let mut history = engine
                    .cold_pcm_history
                    .lock()
                    .map_err(|_| PyRuntimeError::new_err("PCM history lock poisoned"))?;
                let pad = history
                    .get_mut(id)
                    .ok_or_else(|| PyRuntimeError::new_err("same-pad PCM history missing"))?;
                ensure_window_history_capacity(pad, &old, start, end)
                    .map_err(PyRuntimeError::new_err)?;
            }
            let held_readers = engine
                .project_assets
                .held_same_pad_reader_backings(&engine.cold_pcm_history, id, &source_lease)
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
            admitted_window_peak_with_readers(
                &old,
                start,
                end,
                owners[id].accepted.as_ref(),
                &held_readers,
            )
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        }
        Some(reservation)
    };
    let revision = if reuse_window {
        old.window_revision()
    } else {
        old.window_revision()
            .checked_add(1)
            .ok_or_else(|| PyValueError::new_err("window revision exhausted"))?
    };
    if owners[id].committed.claimed()
        || owners[id]
            .relocation
            .as_ref()
            .is_some_and(|pending| pending.publication.status() == "adopting")
    {
        return Err(PyRuntimeError::new_err(
            "resident native adoption is in progress",
        ));
    }
    let next_intent = super::prepared_source::next_epoch(&owners[id].intent_epoch)
        .map_err(PyRuntimeError::new_err)?;
    // All scalar exhaustion checks precede cancellation/publication effects.
    // Producer capacity is still exclusively held, and an unclaimed predecessor
    // must actually cancel before this reservation becomes current.
    let base_intent = owners[id].intent_epoch.load(Ordering::Acquire);
    let control = ResidentControlWrite::capture(
        &binding,
        base_intent,
        next_intent,
        request.key_lock.is_some(),
    )
    .map_err(PyRuntimeError::new_err)?
    .with_predecessor(
        owners[id]
            .relocation
            .as_ref()
            .map(|previous| previous.publication.clone()),
    );
    let key_lock_request_id = control.next_request;
    let proposal = if reuse_window {
        None
    } else {
        Some(
            engine
                .resident_proposals
                .begin_for_cell(control.placement.cell())
                .map_err(PyRuntimeError::new_err)?
                .with_source_epoch(source_epoch.clone(), source_epoch.load(Ordering::Acquire)),
        )
    };
    if reuse_window {
        let mut reused = vec![&old];
        if let Some(set) = ready_stems.as_ref() {
            reused.extend(set.stems.iter());
        }
        let mut resident = engine
            .project_assets
            .reserve_resident(&engine.resident_account, 0, &reused, reused.len())
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        resident
            .commit(&reused)
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    }
    let publication = PreparedSourcePermit::new(owners[id].intent_epoch.clone(), next_intent)
        .with_source_epoch(source_epoch.clone(), source_epoch.load(Ordering::Acquire));
    let state = Arc::new(WindowState::for_placement(
        control.placement,
        binding.source_generation,
    ));
    state
        .start_readiness
        .set(SharedStartReadiness {
            seal: PadStartReadinessSeal::capture(&binding)
                .map_err(|reason| PyValueError::new_err(reason.as_str()))?,
            publication: publication.clone(),
            binding: binding.binding,
            source_generation: binding.source_generation,
            acknowledgements: binding.acknowledgements.clone(),
            loop_region: request.loop_region,
            start_frame: start,
            end_frame: end,
            full_frame_count: old.frame_count(),
            window_revision: revision,
            previous_window_revision: old.window_revision(),
            key_lock_request_id,
            independent_owner: pad_start.is_none(),
            members: Mutex::new(Vec::new()),
        })
        .map_err(|_| PyRuntimeError::new_err("resident readiness already installed"))?;
    let member_cancelled = pad_start
        .map(|_| {
            state
                .start_readiness
                .get()
                .expect("new readiness record")
                .register_member()
        })
        .transpose()?;
    *state
        .proposal
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident proposal lock poisoned"))? =
        proposal.clone();
    intent.key_lock_request_id = key_lock_request_id;
    let token = Arc::new(());
    let ticket = ResidentWindowTicket {
        input_trigger: input_trigger.cloned(),
        pad_start: pad_start.cloned(),
        member_cancelled,
        publication: publication.clone(),
        state: state.clone(),
        window_revision: revision,
        previous_window_revision: old.window_revision(),
        start_frame: start,
        end_frame: end,
        full_frame_count: old.frame_count(),
        id,
        source_generation: binding.source_generation,
        binding: binding.binding,
        ownership: binding.ownership.clone(),
        acknowledgements: binding.acknowledgements.clone(),
        loop_region: request.loop_region,
        key_lock_request_id: intent.key_lock_request_id,
    };
    let stem_owner = owners[id].accepted.clone();
    let mut pending = PendingWindow {
        sample: None,
        stems: None,
        publication: publication.clone(),
        expected_revision: old.window_revision(),
        token: token.clone(),
        state: state.clone(),
        admitted: Instant::now(),
    };
    if reuse_window {
        pending.sample = Some(old.clone());
        pending.stems = ready_stems.clone();
        *state
            .adopted_binding
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident binding lock poisoned"))? =
            Some(binding.binding);
        publication
            .mark_pending()
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        let message =
            ControlMessage::RelocateResident(Box::new(crate::messages::ResidentTransaction {
                id,
                sample: old,
                stems: ready_stems,
                binding: binding.binding,
                publication,
                expected_window_revision: revision,
                intent,
                seek_pin: None,
            }));
        let intent_epoch = owners[id].intent_epoch.clone();
        let mut displaced = None;
        let mut predecessor_cancelled = false;
        control
            .commit(
                &binding,
                &intent_epoch,
                || control.claim_predecessor(&mut predecessor_cancelled),
                || {
                    displaced = owners[id].relocation.replace(pending);
                    owners[id].start_readiness = Some(Arc::downgrade(&state));
                    state.control_published.store(true, Ordering::Release);
                    admission_producer
                        .push(message)
                        .expect("reserved single-producer capacity");
                },
            )
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        if predecessor_cancelled && let Some(previous) = displaced.as_ref() {
            previous.state.fail(
                &engine.input_runtime_ownership,
                id,
                previous.publication.expected,
                "resident preparation superseded by newer intent".into(),
                true,
            );
        }
        drop(displaced);
        return Ok(ticket);
    }
    owners[id].proposal = Some(pending);
    owners[id].start_readiness = Some(Arc::downgrade(&state));
    let (capture, seek_receiver) = if seek_only {
        let (sender, receiver) = rtrb::RingBuffer::new(1);
        (
            Some(Arc::new(
                super::resident_seek::ResidentSeekCapture::new(
                    id,
                    binding.binding,
                    publication.clone(),
                    request
                        .seek_position_s
                        .expect("validated seek-only request"),
                    sender,
                )
                .proposal(proposal.clone().expect("worker proposal")),
            )),
            Some(receiver),
        )
    } else {
        (None, None)
    };
    let mut work = WindowWork {
        id,
        old: old.clone(),
        bank: old,
        seek_receiver,
        seek_pin: None,
        start,
        end,
        revision,
        context,
        intent,
        _original_request: requests[id],
        binding,
        control,
        source_lease,
        stem_owner,
        requests: engine.pad_request_ids.clone(),
        cache: engine.sample_cache.clone(),
        owners: engine.resident_stem_cache.clone(),
        preparation_gate: engine.resident_preparation_gate.clone(),
        assets: engine.project_assets.clone(),
        pcm_history: engine.cold_pcm_history.clone(),
        producer: producer.clone(),
        publication: publication.clone(),
        state: state.clone(),
        token,
        cancelled: engine.cold_cancelled.clone(),
        proposal: proposal.expect("worker proposal"),
        intent_epoch: owners[id].intent_epoch.clone(),
        base_intent,
        resident: super::resident_proposal::ResidentReadContext::reader(
            engine.project_assets.clone(),
            engine.resident_account.clone(),
        ),
    };
    drop(owners);
    drop(cache);
    drop(requests);
    if let Some(capture) = capture
        && admission_producer
            .push(ControlMessage::CaptureResidentSeek(capture))
            .is_err()
    {
        publication.cancel_unclaimed();
        state.fail(
            &engine.input_runtime_ownership,
            id,
            publication.expected,
            "resident command queue is full".into(),
            false,
        );
        return Err(PyRuntimeError::new_err("resident command queue is full"));
    }
    drop(admission_producer);
    let mut guard = JobGuard {
        publication: publication.clone(),
        state: state.clone(),
        id,
        ownership: engine.input_runtime_ownership.clone(),
        enqueued: false,
    };
    #[cfg(test)]
    let observer = super::native_owned_work::take_preparation_observer();
    engine
        .cold_jobs
        .submit(
            reservation.expect("new storage reserved cold capacity"),
            move || {
                #[cfg(test)]
                let _observation =
                    super::native_owned_work::PreparationObservationScope::install(observer);
                match work.prepare() {
                    Ok(()) => guard.mark_enqueued(),
                    Err(error) => {
                        let cancelled = !work.current();
                        work.publication.cancel_unclaimed();
                        work.state.fail(
                            &work.binding.ownership,
                            work.id,
                            work.publication.expected,
                            error,
                            cancelled,
                        );
                    }
                }
            },
        )
        .map_err(PyRuntimeError::new_err)?;
    Ok(ticket)
}

#[cfg(test)]
#[path = "resident_control_publication_tests.rs"]
pub(super) mod control_publication_tests;

#[cfg(test)]
#[path = "resident_terminal_order_tests.rs"]
mod terminal_order_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_pad_window_history_capacity_rejects_new_backing_and_reuses_registered_backing() {
        let old = source();
        let mut owners: Vec<Arc<[f32]>> = (0..127).map(|_| Arc::from([0.5_f32; 1])).collect();
        let mut history = vec![Arc::downgrade(&old.samples)];
        history.extend(owners.iter().map(Arc::downgrade));
        let before = history.clone();
        assert!(ensure_window_history_capacity(&mut history, &old, 200, 400).is_err());
        assert_eq!(history.len(), before.len());
        assert!(
            history
                .iter()
                .zip(&before)
                .all(|(now, prior)| now.ptr_eq(prior))
        );
        assert!(ensure_window_history_capacity(&mut history, &old, 0, 1000).is_ok());
        drop(owners.pop());
        assert!(ensure_window_history_capacity(&mut history, &old, 200, 400).is_ok());
        assert_eq!(history.len(), 127);
    }

    #[test]
    fn sealed_live_window_geometry_above_one_gib_keeps_actual_overlap_without_fixed_cutoff() {
        // Geometry-only admission regression for the reported relocation gate.
        // These actual tiny backings do not prove a large file, read, ACK or
        // render; host/resident admission and productive reads have separate tests.
        let frames = super::super::cold_jobs::PCM_LIMIT_BYTES / 4 + 4096;
        let mut old = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.25; 64]),
            residency: Some(Arc::new(crate::messages::ResidentSourceView {
                source: Arc::new(crate::messages::CompleteSourceIdentity {
                    frame_count: frames,
                    channels: 1,
                    sample_rate_hz: 48_000,
                    original_sha256: [0; 32],
                    playback_sha256: [0; 32],
                    mono_sha256: [0; 32],
                    transform_sha256: [0; 32],
                    source_zero_frame: 0,
                }),
                start_frame: 0,
                window_revision: 1,
                context: ResidentContext::FiniteLoop,
            })),
        };
        let retained: Arc<[f32]> = Arc::from([0.5; 32]);
        let held = [old.samples.clone(), retained.clone(), retained];
        let peak = admitted_window_peak_with_readers(&old, 0, frames, None, &held).unwrap();
        assert_eq!(peak, frames * 4 + (64 + 32) * 4 + 64 * 1024);
        assert!(peak > super::super::cold_jobs::PCM_LIMIT_BYTES);
        assert_eq!(old.samples.len(), 64);
        assert!(admitted_window_peak_with_readers(&old, 0, frames + 1, None, &held).is_err());
        let view = Arc::make_mut(old.residency.as_mut().unwrap());
        let identity = Arc::get_mut(&mut view.source).unwrap();
        identity.frame_count = usize::MAX;
        assert!(admitted_window_peak_with_readers(&old, 0, usize::MAX, None, &held).is_err());
    }

    #[test]
    fn legacy_peak_charges_distinct_live_old_component_backing_before_conversion() {
        // This is an admission-only geometry fixture, not a source/ACK oracle.
        // Tiny real arrays expose a boundary without allocating complete PCM.
        let held = 64 * 4;
        let crop = 4;
        let fixed = 4 * 1024 * 1024 + 64 * 1024 + crop * 5 + held;
        let frames = (super::super::cold_jobs::PCM_LIMIT_BYTES - fixed) / 40;
        let old = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.0; 64]),
            residency: Some(Arc::new(crate::messages::ResidentSourceView {
                source: Arc::new(crate::messages::CompleteSourceIdentity {
                    frame_count: frames,
                    channels: 1,
                    sample_rate_hz: 48_000,
                    original_sha256: [0; 32],
                    playback_sha256: [0; 32],
                    mono_sha256: [0; 32],
                    transform_sha256: [0; 32],
                    source_zero_frame: 0,
                }),
                start_frame: 0,
                window_revision: 1,
                context: ResidentContext::FiniteLoop,
            })),
        };
        let mut owner = StemOwner {
            set: PreparedStemSet {
                complete_set_identity: Arc::new([0; 32]),
                accepted_timing: None,
                reference_samples: old.samples.clone(),
                reference_residency: old.residency.clone(),
                publication: PreparedSourcePermit::unbound(),
                source_version_hash: 0,
                sample_rate_hz: 48_000,
                channels: 1,
                frame_count: frames,
                available_mask: 0b1111,
                stems: std::array::from_fn(|_| old.clone()),
            },
            source_version: "admission-only".into(),
            cache_dir: PathBuf::new(),
            generation_path: PathBuf::new(),
            pair: None,
        };
        let shared = admitted_window_peak(&old, 1, 2, Some(&owner)).unwrap();
        for stem in &mut owner.set.stems {
            stem.samples = Arc::from([0.0; 64]);
        }
        assert!(admitted_window_peak(&old, 1, 2, Some(&owner)).is_err());
        let smaller = admitted_window_peak(&old, 1, 2, None).unwrap();
        assert!(smaller < shared);
        assert_eq!(old.samples.len(), 64);
    }
    use std::sync::{Condvar, mpsc};

    fn source() -> SampleBuffer {
        SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from(vec![0.25; 1000]),
        }
        .with_complete_source(48_000)
    }

    #[test]
    fn claimed_callback_state_is_nonterminal_and_cannot_be_cancelled_or_reconciled_early() {
        let engine = AudioEngine::new().unwrap();
        let full = source();
        let old = full
            .window(200, 400, 1, ResidentContext::FiniteLoop)
            .unwrap();
        let next = full
            .window(190, 410, 2, ResidentContext::FiniteLoop)
            .unwrap();
        engine.sample_cache.lock().unwrap()[0] = Some(old.clone());
        let publication = PreparedSourcePermit::unrestricted();
        publication.mark_pending().unwrap();
        let state = Arc::new(WindowState::default());
        engine.resident_stem_cache.lock().unwrap()[0].relocation = Some(PendingWindow {
            sample: Some(next.clone()),
            stems: None,
            publication: publication.clone(),
            expected_revision: 1,
            token: Arc::new(()),
            state: state.clone(),
            admitted: Instant::now(),
        });
        assert!(publication.claim_resident());
        assert_eq!(publication.status(), "adopting");
        assert!(!publication.cancel_unclaimed());
        reconcile(&engine).unwrap();
        assert!(
            engine.resident_stem_cache.lock().unwrap()[0]
                .relocation
                .is_some()
        );
        assert!(
            engine.sample_cache.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .same_window(&old)
        );
        publication.mark_accepted();
        reconcile(&engine).unwrap();
        assert!(
            engine.sample_cache.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .same_window(&next)
        );
        assert!(
            engine.resident_stem_cache.lock().unwrap()[0]
                .relocation
                .is_none()
        );
        assert_eq!(state.terminal.load(Ordering::Acquire), 0);
    }

    #[test]
    fn timeout_and_shutdown_revoke_unclaimed_pending_and_preserve_previous_effective_source() {
        for shutdown in [false, true] {
            let engine = AudioEngine::new().unwrap();
            let full = source();
            let old = full
                .window(200, 400, 1, ResidentContext::FiniteLoop)
                .unwrap();
            let next = full
                .window(190, 410, 2, ResidentContext::FiniteLoop)
                .unwrap();
            engine.sample_cache.lock().unwrap()[0] = Some(old.clone());
            let publication = PreparedSourcePermit::unrestricted();
            publication.mark_pending().unwrap();
            let state = Arc::new(WindowState::default());
            engine.resident_stem_cache.lock().unwrap()[0].relocation = Some(PendingWindow {
                sample: Some(next),
                stems: None,
                publication: publication.clone(),
                expected_revision: 1,
                token: Arc::new(()),
                state: state.clone(),
                admitted: if shutdown {
                    Instant::now()
                } else {
                    Instant::now() - ADOPTION_DEADLINE - Duration::from_secs(1)
                },
            });
            if shutdown {
                cancel_all(&engine).unwrap();
            }
            reconcile(&engine).unwrap();
            assert_eq!(publication.status(), "rejected");
            assert_eq!(state.terminal.load(Ordering::Acquire), 2);
            assert!(
                engine.sample_cache.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .same_window(&old)
            );
            assert!(
                engine.resident_stem_cache.lock().unwrap()[0]
                    .relocation
                    .is_none()
            );
        }
    }

    #[test]
    fn actual_cold_lane_shutdown_drops_queued_job_guard_and_settles_ticket_without_running_it() {
        let mut lane = super::super::cold_jobs::ColdJobs::new().unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (tx, rx) = mpsc::channel();
        for _ in 0..2 {
            let gate = gate.clone();
            let tx = tx.clone();
            lane.submit(lane.reserve().unwrap(), move || {
                tx.send(()).unwrap();
                let mut ready = gate.0.lock().unwrap();
                while !*ready {
                    ready = gate.1.wait(ready).unwrap();
                }
            })
            .unwrap();
        }
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let publication = PreparedSourcePermit::unrestricted();
        let state = Arc::new(WindowState::default());
        let ownership = Arc::new(InputRuntimeOwnership::default());
        ownership.begin_resident_control(0, publication.expected);
        let mut guard = JobGuard {
            publication: publication.clone(),
            state: state.clone(),
            id: 0,
            ownership: ownership.clone(),
            enqueued: false,
        };
        lane.submit(lane.reserve().unwrap(), move || guard.mark_enqueued())
            .unwrap();
        let shutdown = std::thread::spawn(move || lane.shutdown());
        let deadline = Instant::now() + Duration::from_secs(3);
        while state.terminal.load(Ordering::Acquire) == 0 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(state.terminal.load(Ordering::Acquire), 2);
        assert_eq!(publication.status(), "rejected");
        assert!(!ownership.resident_control_pending(0));
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        shutdown.join().unwrap();
    }

    #[test]
    fn actual_cold_worker_panic_finishes_only_its_exact_pending_input_owner() {
        for newer_intent in [false, true] {
            let mut lane = super::super::cold_jobs::ColdJobs::new().unwrap();
            let publication = PreparedSourcePermit::unrestricted();
            let state = Arc::new(WindowState::default());
            let ownership = Arc::new(InputRuntimeOwnership::default());
            ownership.begin_resident_control(0, publication.expected);
            let guard = JobGuard {
                publication: publication.clone(),
                state: state.clone(),
                id: 0,
                ownership: ownership.clone(),
                enqueued: false,
            };
            if newer_intent {
                ownership.begin_resident_control(0, publication.expected + 1);
            }
            lane.submit(lane.reserve().unwrap(), move || {
                let _guard = guard;
                panic!("injected resident preparation failure");
            })
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            while state.terminal.load(Ordering::Acquire) == 0 && Instant::now() < deadline {
                std::thread::yield_now();
            }
            assert_eq!(state.terminal.load(Ordering::Acquire), 2);
            lane.shutdown();
            assert_eq!(publication.status(), "rejected");
            assert_eq!(
                ownership.resident_control_pending(0),
                newer_intent,
                "failed old job cleared a newer native readiness owner"
            );
        }
    }

    #[test]
    fn same_source_ack_cannot_install_after_a_new_assignment_supersedes_the_control_owner() {
        let engine = AudioEngine::new().unwrap();
        let full = source();
        let next = full
            .window(190, 410, 2, ResidentContext::FiniteLoop)
            .unwrap();
        let replacement = source()
            .window(200, 400, 1, ResidentContext::FiniteLoop)
            .unwrap();
        engine.sample_cache.lock().unwrap()[0] = Some(replacement.clone());
        let publication = PreparedSourcePermit::unrestricted();
        publication.mark_pending().unwrap();
        publication.mark_accepted();
        engine.resident_stem_cache.lock().unwrap()[0].relocation = Some(PendingWindow {
            sample: Some(next),
            stems: None,
            publication,
            expected_revision: 1,
            token: Arc::new(()),
            state: Arc::new(WindowState::default()),
            admitted: Instant::now(),
        });
        reconcile(&engine).unwrap();
        assert!(
            engine.sample_cache.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .same_window(&replacement)
        );
    }
}
