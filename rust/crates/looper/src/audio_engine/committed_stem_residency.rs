//! Sealed same-set residency and independent new-set candidate transactions.
//! Component I/O and every final PCM/file-owner drop use existing workers.
mod copy_binding;
pub(super) use copy_binding::{
    CapturedCopySelection, CopySelectionTarget, PreparedCopySelection, RecoveredSelectionCustody,
    capture_copy_selection, capture_copy_selection_fences, capture_copy_source_selection_fences,
    capture_pair_selection_fences, capture_recovered_empty_selection, capture_recovered_selection,
    prepare_copy_replacement, prepare_copy_selection, reconcile_copy_selection,
};

use super::AudioEngine;
use super::constant_timing::AcceptedTimingProjection;
use super::constant_timing::CurrentTimingAcknowledgements;
use super::content_placement::{PlacementError, PlacementObservation, SlotSnapshot};
use super::input_runtime_binding::{
    self, InputPadBinding, InputRuntimeOwnership, InputRuntimePadBinding,
};
use super::native_history_permit::NativeHistoryPermit;
use super::prepared_source::{
    PreparedSourcePermit, PreparedSourceTicket, next_epoch, validate_prepared_ticket,
};
use super::resident_relocation::{StemOwner, StemPairOwner};
use super::source_playback::SourcePlayback;
use super::source_reader::SourceReadPlan;
use super::stem_pair_preparation::PreparedStemPair;
use crate::messages::{
    ControlMessage, PreparedStemSet, STEM_COMPONENT_MASK, SampleBuffer, StemMixMode,
};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::{Producer, PushError};
use std::cell::UnsafeCell;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(30);

fn placement_admission_error(error: PlacementError) -> PyErr {
    if error == PlacementError::Pending {
        PyRuntimeError::new_err(
            "[resident_stem_placement_pending] content placement publication is in progress",
        )
    } else {
        PyRuntimeError::new_err(error.to_string())
    }
}

impl AudioEngine {
    /// Bracket only preflight observation, including existing idempotent cache
    /// maintenance. Never wrap proposal mutation, admission or queue publication:
    /// an already admitted operation must return its own original receipt.
    pub(super) fn stable_committed_placement_read<T>(
        &self,
        read: impl FnOnce() -> PyResult<T>,
    ) -> PyResult<T> {
        let before = self
            .input_runtime_ownership
            .placement_read_token_result()
            .map_err(placement_admission_error)?;
        let result = read();
        let after = self
            .input_runtime_ownership
            .placement_read_token_result()
            .map_err(placement_admission_error)?;
        if after != before {
            return Err(placement_admission_error(PlacementError::Pending));
        }
        result
    }
}

pub(super) struct SelectionOwner {
    pub(super) id: usize,
    pub(super) binding: InputPadBinding,
    pub(super) source_generation: u64,
    pub(super) source_version: String,
    pub(super) identity: Arc<[u8; 32]>,
    pub(super) pair: StemPairOwner,
    pub(super) generation_path: PathBuf,
    pub(super) epoch: Arc<AtomicU64>,
    pub(super) expected: u64,
    pub(super) revoked: Arc<AtomicBool>,
    pub(super) source_epoch: Arc<AtomicU64>,
    pub(super) ownership: Arc<InputRuntimeOwnership>,
}

impl SelectionOwner {
    fn source_current(&self) -> bool {
        self.ownership
            .binding_source_generation(self.id, self.binding)
            == Some(self.source_generation)
    }
    fn current(&self) -> bool {
        self.source_current()
            && !self.revoked.load(Ordering::Acquire)
            && self.epoch.load(Ordering::Acquire) == self.expected
    }
    pub(super) fn rt_permit(&self) -> SelectionPermit {
        SelectionPermit {
            identity: *self.identity,
            source_generation: self.source_generation,
            epoch: self.epoch.clone(),
            expected: self.expected,
            revoked: self.revoked.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SelectionPermit {
    pub(crate) identity: [u8; 32],
    pub(crate) source_generation: u64,
    pub(crate) epoch: Arc<AtomicU64>,
    pub(crate) expected: u64,
    pub(crate) revoked: Arc<AtomicBool>,
}
impl SelectionPermit {
    pub(crate) fn current(&self) -> bool {
        !self.revoked.load(Ordering::Acquire) && self.epoch.load(Ordering::Acquire) == self.expected
    }
}

/// Future selection authority. None of these fences mutates the audible predecessor.
#[derive(Debug)]
pub(crate) struct CandidateAuthority {
    epoch: Arc<AtomicU64>,
    expected: u64,
    predecessor_intent: Arc<AtomicU64>,
    predecessor_intent_expected: u64,
    predecessor_selection: Arc<AtomicU64>,
    pub(crate) predecessor_selection_expected: u64,
    adopted: AtomicBool,
}
impl CandidateAuthority {
    pub(crate) fn current(&self) -> bool {
        self.adopted.load(Ordering::Acquire)
            || (self.epoch.load(Ordering::Acquire) == self.expected
                && self.predecessor_intent.load(Ordering::Acquire)
                    == self.predecessor_intent_expected
                && self.predecessor_selection.load(Ordering::Acquire)
                    == self.predecessor_selection_expected)
    }
    pub(crate) fn adopted(&self) -> bool {
        self.adopted.load(Ordering::Acquire)
    }
    /// An accepted selection remains reusable, while each operation's receipt
    /// loses effective authority when a later ordinary mode/window intent wins.
    pub(crate) fn receipt_current(&self) -> bool {
        self.predecessor_intent.load(Ordering::Acquire) == self.predecessor_intent_expected
    }
    pub(crate) fn mark_adopted(&self) {
        self.adopted.store(true, Ordering::Release);
    }
}

#[pyclass(frozen)]
#[derive(Clone)]
pub struct CommittedStemSetTicket {
    owner: Arc<SelectionOwner>,
    publication: PreparedSourcePermit,
    candidate: Option<Arc<CandidateAuthority>>,
}
#[pymethods]
impl CommittedStemSetTicket {
    pub fn publication_status(&self) -> &'static str {
        match self.publication.status() {
            "adopting" => "pending",
            status => status,
        }
    }
    pub fn error(&self) -> Option<String> {
        (self.publication.status() == "rejected")
            .then(|| "Committed stem registration rejected or cancelled".into())
    }
    pub fn is_current(&self) -> bool {
        self.owner.source_current()
            && !self.owner.revoked.load(Ordering::Acquire)
            && if let Some(candidate) = &self.candidate
                && !candidate.adopted()
            {
                self.publication.current() && candidate.current()
            } else if self.publication.status() == "accepted" {
                self.owner.current()
            } else {
                self.publication.current()
            }
    }
    /// Revocation changes future admission only. Actual current audio keeps its readers.
    pub fn cancel(&self) -> bool {
        let revoked = !self.owner.revoked.swap(true, Ordering::AcqRel);
        let cancelled = self.publication.cancel_unclaimed();
        revoked || cancelled
    }
}

pub(crate) struct ResidencyStatus {
    pub(crate) publication: PreparedSourcePermit,
    pub(crate) effective: AtomicU8,
    /// A real source-Native failure remains distinguishable from retired receipt authority.
    pub(crate) effective_failure: AtomicBool,
    error: Mutex<Option<String>>,
    proposal: Option<super::resident_proposal::ResidentProposal>,
    pub(crate) policy_epoch: Option<(Arc<AtomicU64>, u64)>,
    ownership: Arc<InputRuntimeOwnership>,
    id: usize,
    binding: InputPadBinding,
    resident_control: bool,
    control_published: AtomicBool,
    preparation_clock: super::resident_relocation::ResidentPreparationClock,
    #[cfg(test)]
    control_wait_observed: AtomicBool,
    failure_pending: AtomicBool,
    failure_settled: AtomicBool,
    failed_settlement: Mutex<bool>,
}
impl std::fmt::Debug for ResidencyStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResidencyStatus")
            .field("publication", &self.publication)
            .finish()
    }
}
impl ResidencyStatus {
    fn preparation_expired_at(&self, admitted: Instant, now: Instant) -> Result<bool, String> {
        let pending = !self.control_published.load(Ordering::Acquire)
            && self.preparation_current()
            && self
                .binding
                .placement
                .and_then(|placement| placement.content_key())
                .is_some_and(|key| {
                    matches!(
                        self.ownership.observe_control_key(key),
                        PlacementObservation::Pending
                    )
                });
        self.preparation_clock
            .expired_at(admitted, pending, DEADLINE, now)
    }
    fn publication_status(&self) -> &'static str {
        if self.failure_pending.load(Ordering::Acquire)
            && !self.failure_settled.load(Ordering::Acquire)
        {
            return "pending";
        }
        match self.publication.status() {
            "adopting" => "pending",
            status => status,
        }
    }
    pub(crate) fn preparation_current(&self) -> bool {
        // Arming remains private preparation until the complete control image is published.
        // The original proposal and cancellation CAS still fence genuinely superseded work.
        if let Some(proposal) = self.proposal.as_ref()
            && !self.control_published.load(Ordering::Acquire)
        {
            self.publication.status() != "rejected" && proposal.current()
        } else {
            self.publication.current()
        }
    }
    pub(crate) fn reject_rt(&self) {
        self.finish_control_rt();
        self.publication.mark_rejected();
        self.effective.store(3, Ordering::Release);
    }
    pub(crate) fn reject_retired_rt(&self) {
        self.publication.mark_rejected();
        self.effective.store(3, Ordering::Release);
    }
    /// Only callback applications already holding the shared mutation marker use this tail.
    pub(crate) fn finish_control_rt(&self) {
        if self.resident_control && self.control_published.load(Ordering::Acquire) {
            self.ownership.finish_resident_control_for_binding(
                self.id,
                self.binding,
                self.publication.expected,
            );
        }
    }
    fn settlement_pending(&self) -> bool {
        self.failed_settlement.lock().is_ok_and(|pending| *pending)
    }
    fn publish_control(&self, publish: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        // Off RT, follow Producer -> Owners -> settlement -> nonblocking mutation claim.
        // Failure cannot retire an unpublished request after its final guards have passed.
        let _settlement = self
            .failed_settlement
            .lock()
            .map_err(|_| "committed settlement lock poisoned")?;
        publish()
    }
    fn retry_failure(&self) {
        let Ok(mut pending) = self.failed_settlement.lock() else {
            return;
        };
        if !*pending {
            return;
        }
        if self.resident_control
            && self.control_published.load(Ordering::Acquire)
            && !self.ownership.try_finish_resident_control_for_binding(
                self.id,
                self.binding,
                self.publication.expected,
            )
        {
            return;
        }
        // A Pending marker retains this settlement before any terminal rejection is visible.
        self.effective.store(3, Ordering::Release);
        self.failure_settled.store(true, Ordering::Release);
        *pending = false;
    }
    fn fail(&self, error: String) -> bool {
        let Ok(mut pending) = self.failed_settlement.lock() else {
            return false;
        };
        if self.failure_pending.load(Ordering::Acquire) {
            return false;
        }
        // Reserve cancellation with the existing publication CAS. The public receipt remains
        // Pending until its captured Native control is settled; an actual callback claim wins.
        self.failure_pending.store(true, Ordering::Release);
        if !self.publication.cancel_unclaimed() {
            self.failure_pending.store(false, Ordering::Release);
            return false;
        }
        if let Ok(mut stored) = self.error.lock() {
            *stored = Some(error);
        }
        *pending = true;
        drop(pending);
        self.retry_failure();
        true
    }
}

#[pyclass(frozen)]
pub struct ResidentStemTicket {
    status: Arc<ResidencyStatus>,
    owner: Arc<SelectionOwner>,
    full_mix: bool,
    binding: InputPadBinding,
    launch_revision: u64,
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
    candidate: Option<Arc<CandidateAuthority>>,
    policy_only: bool,
}
#[pymethods]
impl ResidentStemTicket {
    pub fn residency_status(&self) -> &'static str {
        self.status.publication_status()
    }
    pub fn effective_mode_status(&self) -> &'static str {
        if self.policy_only {
            return "not_requested";
        }
        if self.status.failure_pending.load(Ordering::Acquire)
            && !self.status.failure_settled.load(Ordering::Acquire)
        {
            return "waiting";
        }
        if !self.is_current() {
            return "rejected";
        }
        match self.status.effective.load(Ordering::Acquire) {
            1 => "transitioning",
            2 => "effective",
            3 => "rejected",
            _ => "waiting",
        }
    }
    pub fn effective_failure_reason(&self) -> Option<&'static str> {
        self.status
            .effective_failure
            .load(Ordering::Acquire)
            .then_some("native-preparation-failed")
    }
    pub fn error(&self) -> Option<String> {
        if self.effective_failure_reason().is_some() {
            return Some("Committed stem effective mode Native preparation failed".into());
        }
        self.status
            .error
            .lock()
            .ok()
            .and_then(|error| error.clone())
            .or_else(|| {
                (self.status.publication.status() == "rejected" || !self.is_current())
                    .then(|| "Committed stem residency or mode ownership changed".into())
            })
    }
    pub fn is_current(&self) -> bool {
        (if self.policy_only && self.status.publication.status() == "accepted" {
            self.status
                .policy_epoch
                .as_ref()
                .is_some_and(|(epoch, expected)| epoch.load(Ordering::Acquire) == *expected)
        } else {
            self.status.preparation_current()
        }) && self
            .candidate
            .as_ref()
            .is_none_or(|candidate| candidate.receipt_current())
            && self.owner.source_current()
            && (self.full_mix
                || self.owner.current()
                || self.candidate.as_ref().is_some_and(|candidate| {
                    !candidate.adopted()
                        && candidate.current()
                        && !self.owner.revoked.load(Ordering::Acquire)
                }))
            && self
                .owner
                .ownership
                .authority_current(self.owner.id, self.binding)
            && self
                .owner
                .ownership
                .binding_source_current(self.owner.id, self.binding)
            && self.acknowledgements.current_for_binding(
                &self.owner.ownership,
                self.owner.id,
                self.binding,
            ) == self
                .binding
                .accepted
                .map_or(0, |accepted| accepted.publication_epoch)
            && (self.policy_only
                || self.owner.ownership.binding_launch_current(
                    self.owner.id,
                    self.binding,
                    self.launch_revision,
                ))
    }
    pub fn cancel(&self) -> bool {
        self.withdraw()
    }
    /// True only when this caller actually won cancellation before callback claim.
    pub fn withdraw(&self) -> bool {
        if let Some(proposal) = self.status.proposal.as_ref()
            && !proposal.armed()
        {
            proposal.cancel();
        }
        self.status
            .fail("Committed stem residency or mode ownership changed".into())
    }
    /// Claim remains observable after acceptance, including paused/stopped waiting.
    pub fn claimed(&self) -> bool {
        matches!(self.status.publication.status(), "adopting" | "accepted")
    }
}

struct PendingRegistration {
    owner: Arc<SelectionOwner>,
    publication: PreparedSourcePermit,
}
#[derive(Clone)]
struct PendingResidency {
    owner: Arc<SelectionOwner>,
    status: Arc<ResidencyStatus>,
    stems: Option<PreparedStemSet>,
    full_mix: bool,
    admitted: Instant,
    candidate: Option<Arc<CandidateAuthority>>,
    policy_only: bool,
    retain_components: bool,
    preload: bool,
}

#[derive(Default)]
pub(super) struct CommittedStemState {
    accepted: Option<Arc<SelectionOwner>>,
    registration: Option<PendingRegistration>,
    registration_epoch: Arc<AtomicU64>,
    selection_epoch: Arc<AtomicU64>,
    candidate_epoch: Arc<AtomicU64>,
    pending: Option<PendingResidency>,
    proposal: Option<PendingResidency>,
    policy_pending: Option<PendingResidency>,
    policy_proposal: Option<PendingResidency>,
    policy_operation_epoch: Arc<AtomicU64>,
    policy_interest: bool,
    explicit_interest: bool,
    policy_epoch: Arc<AtomicU64>,
    /// The real admission calculation is exercised without allocating a GiB fixture.
    #[cfg(test)]
    pub(super) pcm_limit_for_test: Option<usize>,
}
impl CommittedStemState {
    fn proposal_slot(&mut self, policy: bool) -> &mut Option<PendingResidency> {
        if policy {
            &mut self.policy_proposal
        } else {
            &mut self.proposal
        }
    }
    fn pending_slot(&mut self, policy: bool) -> &mut Option<PendingResidency> {
        if policy {
            &mut self.policy_pending
        } else {
            &mut self.pending
        }
    }
    fn explicit_busy(&self) -> bool {
        self.proposal
            .as_ref()
            .is_some_and(|proposal| proposal.status.preparation_current())
            || self.pending.as_ref().is_some_and(|pending| {
                matches!(
                    pending.status.publication.status(),
                    "captured" | "pending" | "adopting"
                ) || (pending.status.publication.status() == "accepted"
                    && pending.status.effective.load(Ordering::Acquire) == 1)
            })
    }
    pub(super) fn reconcile(
        &mut self,
        accepted: &mut Option<StemOwner>,
        sample: Option<&SampleBuffer>,
    ) {
        if let Some(proposal) = self.proposal.as_ref() {
            proposal.status.retry_failure();
            match proposal
                .status
                .preparation_expired_at(proposal.admitted, Instant::now())
            {
                Ok(true) => {
                    proposal
                        .status
                        .fail("Committed stem proposal deadline expired".into());
                }
                Err(error) => {
                    proposal.status.fail(error);
                }
                Ok(false) => {}
            }
            if (!proposal.status.preparation_current() || !proposal.owner.source_current())
                && !proposal.status.settlement_pending()
            {
                self.proposal = None;
            }
        }
        if let Some(proposal) = self.policy_proposal.as_ref() {
            proposal.status.retry_failure();
            match proposal
                .status
                .preparation_expired_at(proposal.admitted, Instant::now())
            {
                Ok(true) => {
                    proposal
                        .status
                        .fail("Committed policy proposal deadline expired".into());
                }
                Err(error) => {
                    proposal.status.fail(error);
                }
                Ok(false) => {}
            }
            if (!proposal.status.preparation_current() || !proposal.owner.source_current())
                && !proposal.status.settlement_pending()
            {
                self.policy_proposal = None;
            }
        }
        if let Some(registration) = &self.registration {
            match registration.publication.status() {
                "accepted" if registration.owner.current() => {
                    self.accepted = Some(registration.owner.clone())
                }
                "captured" | "pending" | "adopting" => {}
                _ => {}
            }
            if !matches!(
                registration.publication.status(),
                "captured" | "pending" | "adopting"
            ) {
                self.registration = None;
            }
        }
        if self.accepted.as_ref().is_some_and(|owner| !owner.current()) {
            self.accepted = None;
            self.policy_interest = false;
            self.explicit_interest = false;
        }
        // Accepted means that this request once installed readers; it does not grant
        // authority to re-promote them after a newer ordered release or cancellation.
        if let Some(pending) = self.pending.as_ref() {
            pending.status.retry_failure();
        }
        if self.pending.as_ref().is_some_and(|pending| {
            !pending.status.publication.current()
                || pending
                    .candidate
                    .as_ref()
                    .is_some_and(|candidate| !candidate.receipt_current())
                || !pending.owner.source_current()
                || (!pending.full_mix
                    && !pending.owner.current()
                    && !pending.candidate.as_ref().is_some_and(|candidate| {
                        !candidate.adopted()
                            && candidate.current()
                            && !pending.owner.revoked.load(Ordering::Acquire)
                    }))
        }) {
            if let Some(pending) = self.pending.as_ref() {
                pending
                    .status
                    .fail("Committed stem residency or mode ownership changed".into());
            }
            if self
                .pending
                .as_ref()
                .is_some_and(|pending| !pending.status.settlement_pending())
            {
                self.pending = None;
            }
        }
        if let Some(pending) = &mut self.pending {
            if pending.admitted.elapsed() >= DEADLINE {
                pending
                    .status
                    .fail("Committed stem residency deadline expired".into());
            }
            if !pending.status.publication.current() {
                pending
                    .status
                    .fail("Committed stem residency or mode ownership changed".into());
            }
            if pending.status.publication.status() == "accepted" {
                if pending.policy_only {
                    self.policy_interest = pending.preload;
                } else {
                    self.explicit_interest = !pending.full_mix;
                }
                if pending
                    .candidate
                    .as_ref()
                    .is_some_and(|candidate| candidate.adopted())
                    && pending.owner.current()
                {
                    self.accepted = Some(pending.owner.clone());
                }
                if pending.full_mix && !pending.retain_components {
                    if matches!(pending.status.effective.load(Ordering::Acquire), 2 | 4) {
                        *accepted = None;
                        pending.stems = None;
                    }
                } else if let Some(set) = pending.stems.as_ref()
                    && sample.is_some_and(|sample| {
                        super::source_reader::prepared_stem_set_matches_sample(
                            set,
                            sample,
                            sample.channels,
                            pending.owner.binding.sample_rate_hz as f32,
                            sample.frame_count(),
                        )
                    })
                {
                    *accepted = Some(StemOwner {
                        set: set.clone(),
                        source_version: pending.owner.source_version.clone(),
                        cache_dir: pending.owner.generation_path.clone(),
                        generation_path: pending.owner.generation_path.clone(),
                        pair: Some(pending.owner.pair.clone()),
                    });
                }
            }
            pending.status.retry_failure();
            if (pending.status.publication.status() == "rejected"
                || !pending.status.publication.current())
                && !pending.status.settlement_pending()
            {
                self.pending = None;
            }
        }
        if let Some(pending) = self.policy_pending.as_ref() {
            pending.status.retry_failure();
            if pending.admitted.elapsed() >= DEADLINE {
                pending
                    .status
                    .fail("Committed policy residency deadline expired".into());
            }
            if !pending.status.publication.current() || !pending.owner.current() {
                pending
                    .status
                    .fail("Committed stem residency or mode ownership changed".into());
                self.policy_pending = None;
            } else if pending.status.publication.status() == "accepted" {
                self.policy_interest = pending.preload;
                if pending.full_mix && !pending.retain_components {
                    *accepted = None;
                    // A settled explicit FullMix receipt owns no audible stem
                    // interest. Its bookkeeping must not keep policy PCM alive.
                    if let Some(explicit) = self.pending.as_mut()
                        && explicit.full_mix
                        && matches!(explicit.status.effective.load(Ordering::Acquire), 2 | 4)
                    {
                        explicit.stems = None;
                        explicit.retain_components = false;
                    }
                } else if let Some(set) = pending.stems.as_ref()
                    && sample.is_some_and(|sample| {
                        super::source_reader::prepared_stem_set_matches_sample(
                            set,
                            sample,
                            sample.channels,
                            pending.owner.binding.sample_rate_hz as f32,
                            sample.frame_count(),
                        )
                    })
                {
                    *accepted = Some(StemOwner {
                        set: set.clone(),
                        source_version: pending.owner.source_version.clone(),
                        cache_dir: pending.owner.generation_path.clone(),
                        generation_path: pending.owner.generation_path.clone(),
                        pair: Some(pending.owner.pair.clone()),
                    });
                }
                // The acknowledged bank/accepted StemOwner retains actual
                // readers. Policy receipt status alone owns no PCM.
                self.policy_pending = None;
            } else if pending.status.publication.status() == "rejected" {
                self.policy_pending = None;
            }
        }
    }
    pub(super) fn cancel_pending_shared(&self) {
        if let Some(proposal) = self.proposal.as_ref() {
            proposal
                .status
                .fail("Committed stem proposal cancelled by shutdown".into());
        }
        if let Some(pending) = &self.pending {
            pending
                .status
                .fail("Committed stem request cancelled by shutdown".into());
        }
        for pending in [self.policy_proposal.as_ref(), self.policy_pending.as_ref()]
            .into_iter()
            .flatten()
        {
            pending
                .status
                .fail("Committed policy request cancelled by shutdown".into());
        }
        if let Some(registration) = &self.registration {
            registration.publication.cancel_unclaimed();
        }
    }
    pub(super) fn claimed(&self) -> bool {
        [self.pending.as_ref(), self.policy_pending.as_ref()]
            .into_iter()
            .flatten()
            .any(|pending| pending.status.publication.status() == "adopting")
    }
    fn handover_pending(&self) -> bool {
        self.pending.as_ref().is_some_and(|pending| {
            pending.status.publication.status() == "adopting"
                || (pending.status.publication.status() == "accepted"
                    && pending.status.effective.load(Ordering::Acquire) == 1)
        })
    }
}

#[derive(Clone)]
pub(crate) struct StemVoicePin {
    pub(crate) sample: SampleBuffer,
    pub(crate) stems: Option<PreparedStemSet>,
    pub(crate) voice_index: Option<usize>,
    pub(crate) generation: u64,
    pub(crate) key_lock: bool,
    pub(crate) history_epoch: u64,
    pub(crate) permit: NativeHistoryPermit,
    pub(crate) accepted: Option<AcceptedTimingProjection>,
    pub(crate) plan: SourceReadPlan,
    pub(crate) playback: SourcePlayback,
    pub(crate) predecessor: Option<SelectedStemIdentity>,
}
impl std::fmt::Debug for StemVoicePin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StemVoicePin")
            .field("voice_index", &self.voice_index)
            .field("generation", &self.generation)
            .finish()
    }
}

pub(crate) struct StemVoiceCapture {
    pub(crate) id: usize,
    pub(crate) binding: InputPadBinding,
    pub(crate) selection: SelectionPermit,
    pub(crate) status: Arc<ResidencyStatus>,
    pub(crate) launch_revision: u64,
    pub(crate) target: StemMixMode,
    pub(crate) candidate: Option<Arc<CandidateAuthority>>,
    sender: UnsafeCell<Producer<StemVoicePin>>,
    used: AtomicBool,
}
unsafe impl Sync for StemVoiceCapture {}
impl std::fmt::Debug for StemVoiceCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StemVoiceCapture")
            .field("id", &self.id)
            .finish()
    }
}
impl StemVoiceCapture {
    pub(crate) fn publish(&self, pin: StemVoicePin) -> Option<StemVoicePin> {
        if self
            .used
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Some(pin);
        }
        // Exactly one callback winner owns this preallocated producer.
        match unsafe { &mut *self.sender.get() }.push(pin) {
            Ok(()) => None,
            Err(PushError::Full(pin)) => Some(pin),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StemResidencyTransaction {
    pub(crate) id: usize,
    pub(crate) binding: InputPadBinding,
    pub(crate) selection: SelectionPermit,
    pub(crate) status: Arc<ResidencyStatus>,
    pub(crate) launch_revision: u64,
    pub(crate) target: StemMixMode,
    pub(crate) mask: u8,
    pub(crate) candidate: Option<Arc<CandidateAuthority>>,
    pub(crate) pin: StemVoicePin,
    pub(crate) stems: Option<PreparedStemSet>,
    pub(crate) retired_stems: Option<PreparedStemSet>,
    pub(crate) retired_status: Option<StemModeReceipt>,
    pub(crate) policy_only: bool,
    pub(crate) retain_components: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelectedStemIdentity {
    pub(crate) identity: [u8; 32],
    pub(crate) source_generation: u64,
    pub(crate) epoch: u64,
}
#[derive(Debug, Clone)]
pub(crate) struct StemModeReceipt {
    pub(crate) status: Arc<ResidencyStatus>,
    pub(crate) binding: InputPadBinding,
    pub(crate) selection: SelectionPermit,
    pub(crate) target: StemMixMode,
    pub(crate) mask: u8,
    pub(crate) launch_revision: u64,
    pub(crate) candidate: Option<Arc<CandidateAuthority>>,
}
#[derive(Debug, Clone)]
pub(crate) enum CommittedStemRtWork {
    Register {
        id: usize,
        binding: InputPadBinding,
        selection: SelectionPermit,
        publication: PreparedSourcePermit,
    },
    Capture(Arc<StemVoiceCapture>),
    Adopt(StemResidencyTransaction),
    // Held intact for final PCM and preallocated capture-ring destruction off realtime.
    RetiredPin {
        _pin: StemVoicePin,
        _capture: Arc<StemVoiceCapture>,
    },
}

impl AudioEngine {
    pub(super) fn register_committed_stem_pair_with_producer(
        &self,
        prepared: &PreparedStemPair,
        ticket: &PreparedSourceTicket,
        producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ) -> PyResult<CommittedStemSetTicket> {
        super::resident_relocation::reconcile(self)?;
        let id = match ticket
            .publication
            .control_placement(self)
            .map_err(PyValueError::new_err)?
        {
            PlacementObservation::Current(current) => current.slot(),
            PlacementObservation::Pending => {
                return Err(PyRuntimeError::new_err(
                    "committed source control placement is pending",
                ));
            }
            _ => {
                return Err(PyValueError::new_err(
                    "committed source content owner is stale",
                ));
            }
        };
        let binding = ticket.admission.clone();
        if !binding.available() || !binding.admitted_current() {
            return Err(PyValueError::new_err(
                "stale or foreign prepared source ticket",
            ));
        }
        let requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        if self.input_runtime_ownership.copy_reservation(id) != 0 {
            return Err(PyRuntimeError::new_err("pad content copy is pending"));
        }
        let source_epoch = self
            .prepared_source_epoch(id)
            .map_err(PyRuntimeError::new_err)?;
        let cache = self
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source cache lock poisoned"))?;
        let sample = cache
            .get(id)
            .and_then(Option::as_ref)
            .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
        validate_prepared_ticket(
            ticket,
            id,
            &prepared.source_version,
            requests[id],
            source_epoch,
            sample,
        )
        .map_err(PyValueError::new_err)?;
        if !sample.same_window(&prepared.reference) || prepared.discarded.load(Ordering::Acquire) {
            return Err(PyValueError::new_err(
                "committed pair source/window is stale",
            ));
        }
        let mut producer = producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
        let mut owners = self
            .resident_stem_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident ownership lock poisoned"))?;
        let state = &mut owners[id].committed;
        if let Some(previous) = &state.accepted
            && previous.current()
            && previous.source_generation == binding.source_generation
            && *previous.identity == *prepared._identity
            && previous.source_version == prepared.source_version
            && previous.generation_path == prepared.wav_path
            && previous.pair.root == prepared.root
            && previous.pair.descriptor_reference == prepared.descriptor_reference
        {
            return Ok(CommittedStemSetTicket {
                owner: previous.clone(),
                publication: PreparedSourcePermit::accepted_fixed(),
                candidate: None,
            });
        }
        if producer.is_full() {
            return Err(PyRuntimeError::new_err(
                "committed registration queue is full",
            ));
        }
        let expected = next_epoch(&state.registration_epoch).map_err(PyRuntimeError::new_err)?;
        let source_expected = source_epoch.load(Ordering::Acquire);
        let owner = Arc::new(SelectionOwner {
            id: id,
            binding: binding.binding,
            source_generation: binding.source_generation,
            source_version: prepared.source_version.clone(),
            identity: prepared._identity.clone(),
            pair: StemPairOwner {
                root: prepared.root.clone(),
                descriptor_reference: prepared.descriptor_reference.clone(),
                source_lease: prepared.source_lease.clone(),
                reader: prepared.pair_reader.clone(),
            },
            generation_path: prepared.wav_path.clone(),
            epoch: state.selection_epoch.clone(),
            expected,
            revoked: Arc::new(AtomicBool::new(false)),
            source_epoch: source_epoch.clone(),
            ownership: self.input_runtime_ownership.clone(),
        });
        let publication = PreparedSourcePermit::new(state.registration_epoch.clone(), expected)
            .with_source_epoch(owner.source_epoch.clone(), source_expected);
        publication
            .mark_pending()
            .map_err(PyRuntimeError::new_err)?;
        state.registration_epoch.store(expected, Ordering::Release);
        producer
            .push(ControlMessage::CommittedStemWork(Box::new(
                CommittedStemRtWork::Register {
                    id: id,
                    binding: binding.binding,
                    selection: owner.rt_permit(),
                    publication: publication.clone(),
                },
            )))
            .map_err(|_| PyRuntimeError::new_err("reserved registration capacity lost"))?;
        state.registration = Some(PendingRegistration {
            owner: owner.clone(),
            publication: publication.clone(),
        });
        Ok(CommittedStemSetTicket {
            owner,
            publication,
            candidate: None,
        })
    }

    /// Seal a future complete set without changing the installed selection or intent.
    pub(super) fn register_pending_committed_stem_pair_with_producer(
        &self,
        prepared: &PreparedStemPair,
        ticket: &PreparedSourceTicket,
        _producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ) -> PyResult<CommittedStemSetTicket> {
        super::resident_relocation::reconcile(self)?;
        let id = match ticket
            .publication
            .control_placement(self)
            .map_err(PyValueError::new_err)?
        {
            PlacementObservation::Current(current) => current.slot(),
            PlacementObservation::Pending => {
                return Err(PyRuntimeError::new_err(
                    "committed source control placement is pending",
                ));
            }
            _ => {
                return Err(PyValueError::new_err(
                    "committed source content owner is stale",
                ));
            }
        };
        let binding = ticket.admission.clone();
        if !binding.available() || !binding.admitted_current() {
            return Err(PyValueError::new_err(
                "stale or foreign prepared source ticket",
            ));
        }
        let requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        if self.input_runtime_ownership.copy_reservation(id) != 0 {
            return Err(PyRuntimeError::new_err("pad content copy is pending"));
        }
        let source_epoch = self
            .prepared_source_epoch(id)
            .map_err(PyRuntimeError::new_err)?;
        let cache = self
            .sample_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("source cache lock poisoned"))?;
        let sample = cache
            .get(id)
            .and_then(Option::as_ref)
            .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
        validate_prepared_ticket(
            ticket,
            id,
            &prepared.source_version,
            requests[id],
            source_epoch,
            sample,
        )
        .map_err(PyValueError::new_err)?;
        if !sample.same_window(&prepared.reference) || prepared.discarded.load(Ordering::Acquire) {
            return Err(PyValueError::new_err(
                "committed candidate source/window is stale",
            ));
        }
        let mut owners = self
            .resident_stem_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident ownership lock poisoned"))?;
        let resident = &mut owners[id];
        if resident.ordinary_publication_pending()
            || resident.committed.handover_pending()
            || resident
                .committed
                .registration
                .as_ref()
                .is_some_and(|registration| {
                    matches!(
                        registration.publication.status(),
                        "captured" | "pending" | "adopting"
                    )
                })
        {
            return Err(PyRuntimeError::new_err("resident handover is in progress"));
        }
        let state = &mut resident.committed;
        let expected = next_epoch(&state.registration_epoch).map_err(PyRuntimeError::new_err)?;
        let candidate_expected =
            next_epoch(&state.candidate_epoch).map_err(PyRuntimeError::new_err)?;
        let owner = Arc::new(SelectionOwner {
            id: id,
            binding: binding.binding,
            source_generation: binding.source_generation,
            source_version: prepared.source_version.clone(),
            identity: prepared._identity.clone(),
            pair: StemPairOwner {
                root: prepared.root.clone(),
                descriptor_reference: prepared.descriptor_reference.clone(),
                source_lease: prepared.source_lease.clone(),
                reader: prepared.pair_reader.clone(),
            },
            generation_path: prepared.wav_path.clone(),
            epoch: state.selection_epoch.clone(),
            expected,
            revoked: Arc::new(AtomicBool::new(false)),
            source_epoch: source_epoch.clone(),
            ownership: self.input_runtime_ownership.clone(),
        });
        let candidate = Arc::new(CandidateAuthority {
            epoch: state.candidate_epoch.clone(),
            expected: candidate_expected,
            predecessor_intent: resident.intent_epoch.clone(),
            predecessor_intent_expected: resident.intent_epoch.load(Ordering::Acquire),
            predecessor_selection: state.selection_epoch.clone(),
            predecessor_selection_expected: state.selection_epoch.load(Ordering::Acquire),
            adopted: AtomicBool::new(false),
        });
        // registration_epoch is only a unique selection-token allocator here. Its
        // change cannot revoke V1's selection_epoch or any resident/native permit.
        state.registration_epoch.store(expected, Ordering::Release);
        state
            .candidate_epoch
            .store(candidate_expected, Ordering::Release);
        Ok(CommittedStemSetTicket {
            owner,
            publication: PreparedSourcePermit::accepted_fixed(),
            candidate: Some(candidate),
        })
    }

    #[cfg(test)]
    pub(super) fn request_committed_stem_residency_with_producer(
        &self,
        selection: &CommittedStemSetTicket,
        mode: &str,
        enabled_mask: u8,
        producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ) -> PyResult<ResidentStemTicket> {
        self.request_committed_stem_residency_with_interest_and_producer(
            selection,
            mode,
            enabled_mask,
            "explicit",
            producer,
        )
    }

    pub(super) fn request_committed_stem_residency_with_interest_and_producer(
        &self,
        selection: &CommittedStemSetTicket,
        mode: &str,
        enabled_mask: u8,
        interest: &str,
        producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ) -> PyResult<ResidentStemTicket> {
        let policy_only = match interest {
            "explicit" => false,
            "preload" | "withdraw_preload" if mode == "full_mix" => true,
            _ => {
                return Err(PyValueError::new_err(
                    "invalid committed resident interest or policy mode",
                ));
            }
        };
        let preload = interest == "preload";
        let target = match mode {
            "full_mix" if preload => StemMixMode::AllStems,
            "full_mix" => StemMixMode::FullMix,
            "all_stems" => StemMixMode::AllStems,
            _ => return Err(PyValueError::new_err("invalid committed stem mode")),
        };
        if enabled_mask & !STEM_COMPONENT_MASK != 0 {
            return Err(PyValueError::new_err("invalid committed stem mask"));
        }
        if selection.publication.status() != "accepted" || !selection.is_current() {
            return Err(PyValueError::new_err(
                "committed disk set lacks its current registration ACK",
            ));
        }
        let candidate = selection
            .candidate
            .as_ref()
            .filter(|candidate| !candidate.adopted())
            .cloned();
        if policy_only && candidate.is_some() {
            return Err(PyValueError::new_err(
                "unadopted candidate requires explicit all_stems intent",
            ));
        }
        if candidate.is_some() && target != StemMixMode::AllStems {
            return Err(PyValueError::new_err(
                "unadopted candidate requires all_stems intent",
            ));
        }
        super::resident_relocation::reconcile(self)?;
        let original_key = selection
            .owner
            .binding
            .placement
            .and_then(|placement| placement.content_key())
            .ok_or_else(|| {
                PyValueError::new_err("committed selection lacks its original content key")
            })?;
        let id = match self
            .input_runtime_ownership
            .observe_control_key(original_key)
        {
            PlacementObservation::Current(resolved) => resolved.slot(),
            PlacementObservation::Pending => {
                return Err(placement_admission_error(PlacementError::Pending));
            }
            _ => {
                return Err(PyValueError::new_err(
                    "committed selection content owner changed",
                ));
            }
        };
        let binding = self.stable_committed_placement_read(|| {
            input_runtime_binding::capture(self, id)?
                .filter(|binding| binding.current() && binding.available())
                .ok_or_else(|| PyValueError::new_err("current source/timing unavailable"))
        })?;
        if binding.source_generation != selection.owner.source_generation
            || binding
                .binding
                .placement
                .and_then(|placement| placement.content_key())
                != Some(original_key)
        {
            return Err(PyValueError::new_err("committed source generation changed"));
        }
        let reservation = self.cold_jobs.reserve().map_err(PyRuntimeError::new_err)?;
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        self.input_runtime_ownership
            .ensure_no_copy_reservation(id)?;
        let source_epoch = self.stable_committed_placement_read(|| {
            self.prepared_source_epoch(id)
                .map_err(PyRuntimeError::new_err)
        })?;
        let mut producer_guard = producer
            .lock()
            .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
        if producer_guard.is_full() {
            return Err(PyRuntimeError::new_err("committed residency queue is full"));
        }
        let mut owners = self
            .resident_stem_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident ownership lock poisoned"))?;
        let owner = &mut owners[id];
        if policy_only && owner.committed.explicit_busy() {
            return Err(PyRuntimeError::new_err(
                "[resident_stem_interest_busy] explicit stem intent is preparing or awaiting effective acknowledgement",
            ));
        }
        if owner.ordinary_publication_pending()
            || owner.committed.claimed()
            || (candidate.is_some() && owner.committed.handover_pending())
        {
            return Err(PyRuntimeError::new_err("resident adoption is in progress"));
        }
        #[cfg(test)]
        let pcm_limit = owner
            .committed
            .pcm_limit_for_test
            .unwrap_or(super::cold_jobs::PCM_LIMIT_BYTES)
            .min(super::cold_jobs::PCM_LIMIT_BYTES);
        let base_intent = owner.intent_epoch.load(Ordering::Acquire);
        let (epoch, expected) = if policy_only {
            let expected = next_epoch(&owner.committed.policy_operation_epoch)
                .map_err(PyRuntimeError::new_err)?;
            (owner.committed.policy_operation_epoch.clone(), expected)
        } else if candidate.is_some() {
            // An independent permit is essential: a candidate number is not an
            // ordinary resident-control token and cannot invalidate V1 history.
            (Arc::new(AtomicU64::new(1)), 1)
        } else {
            let expected = next_epoch(&owner.intent_epoch).map_err(PyRuntimeError::new_err)?;
            (owner.intent_epoch.clone(), expected)
        };
        let publication = PreparedSourcePermit::new(epoch, expected)
            .with_source_epoch(source_epoch.clone(), source_epoch.load(Ordering::Acquire));
        let control = self.stable_committed_placement_read(|| {
            CommittedControlWrite::capture(
                &binding,
                base_intent,
                (candidate.is_none() && !policy_only).then_some(expected),
            )
            .map_err(PyRuntimeError::new_err)
        })?;
        // Every Pending-returning preflight precedes proposal epoch mutation.
        // A deferred request cannot supersede a still-current prior proposal.
        let proposal = if policy_only {
            self.resident_proposals.begin_policy_for_cell(
                binding
                    .binding
                    .placement
                    .expect("captured content placement")
                    .cell(),
            )
        } else {
            self.resident_proposals.begin_stems_for_cell(
                binding
                    .binding
                    .placement
                    .expect("captured content placement")
                    .cell(),
            )
        }
        .map_err(PyRuntimeError::new_err)?
        .with_source_epoch(source_epoch.clone(), source_epoch.load(Ordering::Acquire));
        let status = Arc::new(ResidencyStatus {
            publication: publication.clone(),
            effective: AtomicU8::new(0),
            effective_failure: AtomicBool::new(false),
            error: Mutex::new(None),
            proposal: Some(proposal.clone()),
            policy_epoch: if policy_only {
                Some((
                    owner.committed.policy_epoch.clone(),
                    next_epoch(&owner.committed.policy_epoch).map_err(PyRuntimeError::new_err)?,
                ))
            } else {
                None
            },
            ownership: self.input_runtime_ownership.clone(),
            id,
            binding: binding.binding,
            resident_control: candidate.is_none() && !policy_only,
            control_published: AtomicBool::new(false),
            preparation_clock: super::resident_relocation::ResidentPreparationClock::default(),
            #[cfg(test)]
            control_wait_observed: AtomicBool::new(false),
            failure_pending: AtomicBool::new(false),
            failure_settled: AtomicBool::new(false),
            failed_settlement: Mutex::new(false),
        });
        let (sender, receiver) = rtrb::RingBuffer::new(1);
        let launch_revision = self.input_runtime_ownership.launch_revision(id);
        let capture = Arc::new(StemVoiceCapture {
            id,
            binding: binding.binding,
            selection: selection.owner.rt_permit(),
            status: status.clone(),
            launch_revision,
            target,
            candidate: candidate.clone(),
            sender: UnsafeCell::new(sender),
            used: AtomicBool::new(false),
        });
        *owner.committed.proposal_slot(policy_only) = Some(PendingResidency {
            owner: selection.owner.clone(),
            status: status.clone(),
            stems: None,
            full_mix: target == StemMixMode::FullMix,
            admitted: Instant::now(),
            candidate: candidate.clone(),
            policy_only,
            retain_components: false,
            preload,
        });
        producer_guard
            .push(ControlMessage::CommittedStemWork(Box::new(
                CommittedStemRtWork::Capture(capture),
            )))
            .map_err(|_| PyRuntimeError::new_err("reserved capture capacity lost"))?;
        let acknowledgements = binding.acknowledgements.clone();
        let ticket_binding = binding.binding;
        let work = ResidencyWork {
            id,
            binding,
            control,
            selection: selection.owner.clone(),
            status: status.clone(),
            receiver,
            target,
            mask: enabled_mask,
            launch_revision,
            candidate: candidate.clone(),
            assets: self.project_assets.clone(),
            history: self.cold_pcm_history.clone(),
            owners: self.resident_stem_cache.clone(),
            producer: producer.clone(),
            cancelled: self.cold_cancelled.clone(),
            proposal,
            intent_epoch: owner.intent_epoch.clone(),
            base_intent,
            resident: super::resident_proposal::ResidentReadContext::reader(
                self.project_assets.clone(),
                self.resident_account.clone(),
            ),
            policy_only,
            preload,
            #[cfg(test)]
            pcm_limit,
        };
        drop(owners);
        drop(producer_guard);
        drop(_requests);
        let guard = ResidencyJobGuard {
            status: status.clone(),
            enqueued: false,
        };
        self.cold_jobs
            .submit(reservation, move || {
                let mut guard = guard;
                match work.prepare() {
                    Ok(()) => guard.enqueued = true,
                    Err(error) => {
                        guard.status.fail(error);
                    }
                }
            })
            .map_err(PyRuntimeError::new_err)?;
        Ok(ResidentStemTicket {
            status,
            owner: selection.owner.clone(),
            full_mix: target == StemMixMode::FullMix,
            binding: ticket_binding,
            launch_revision,
            acknowledgements,
            candidate,
            policy_only,
        })
    }
}

struct ResidencyJobGuard {
    status: Arc<ResidencyStatus>,
    enqueued: bool,
}
impl Drop for ResidencyJobGuard {
    fn drop(&mut self) {
        if !self.enqueued {
            self.status
                .fail("Committed stem worker ended before publication".into());
            self.status.retry_failure();
        }
    }
}
/// The original complete control image, captured before worker preparation starts.
struct CommittedControlWrite {
    placement: SlotSnapshot,
    original_intent: u64,
    original_control: u64,
    original_request: u64,
    next_intent: Option<u64>,
}
impl CommittedControlWrite {
    fn capture(
        binding: &InputRuntimePadBinding,
        intent: u64,
        next_intent: Option<u64>,
    ) -> Result<Self, String> {
        let placement = binding
            .binding
            .placement
            .ok_or("committed control source has no captured content placement")?;
        if !binding.ownership.slot_snapshot_current(placement) {
            return Err("committed control content placement changed or is pending".into());
        }
        Ok(Self {
            placement,
            original_intent: intent,
            original_control: binding
                .ownership
                .resident_control_for_cell(placement.cell())
                .load(Ordering::Acquire),
            original_request: binding
                .ownership
                .key_lock_request_for_cell(placement.cell())
                .load(Ordering::Acquire),
            next_intent,
        })
    }
    #[cfg(test)]
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
                    && ownership
                        .resident_control_for_cell(self.placement.cell())
                        .load(Ordering::Acquire)
                        == self.original_control
                    && ownership
                        .key_lock_request_for_cell(self.placement.cell())
                        .load(Ordering::Acquire)
                        == self.original_request
                    && claim()
            },
            |cell| {
                if let Some(next) = self.next_intent {
                    intent_epoch.store(next, Ordering::Release);
                    ownership
                        .resident_control_for_cell(cell)
                        .store(next, Ordering::Release);
                }
                apply();
            },
        )?;
        Ok(())
    }
}

struct ResidencyWork {
    id: usize,
    binding: InputRuntimePadBinding,
    control: CommittedControlWrite,
    selection: Arc<SelectionOwner>,
    status: Arc<ResidencyStatus>,
    receiver: rtrb::Consumer<StemVoicePin>,
    target: StemMixMode,
    mask: u8,
    launch_revision: u64,
    candidate: Option<Arc<CandidateAuthority>>,
    assets: Arc<super::project_assets::ProjectAssets>,
    history: Arc<super::project_assets::PcmHistory>,
    owners: Arc<Mutex<Vec<super::resident_relocation::ResidentStemCache>>>,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    cancelled: Arc<AtomicBool>,
    proposal: super::resident_proposal::ResidentProposal,
    intent_epoch: Arc<AtomicU64>,
    base_intent: u64,
    resident: super::resident_proposal::ResidentReadContext,
    policy_only: bool,
    preload: bool,
    #[cfg(test)]
    pcm_limit: usize,
}
impl ResidencyWork {
    fn current(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && self.status.preparation_current()
            && self.proposal.current()
            && self.binding.admitted_current()
            && (self.proposal.armed()
                || self.intent_epoch.load(Ordering::Acquire) == self.base_intent)
            && self.selection.source_current()
            && (self.target == StemMixMode::FullMix
                || self.selection.current()
                || self.candidate.as_ref().is_some_and(|candidate| {
                    !candidate.adopted()
                        && candidate.current()
                        && !self.selection.revoked.load(Ordering::Acquire)
                }))
            && self.selection.ownership.binding_launch_current(
                self.id,
                self.binding.binding,
                self.launch_revision,
            )
    }
    fn observe_control(&self) -> PlacementObservation<SlotSnapshot> {
        self.control
            .placement
            .content_key()
            .map_or(PlacementObservation::Stale, |key| {
                self.binding.ownership.observe_control_key(key)
            })
    }
    fn current_owners(
        &self,
    ) -> Result<
        (
            std::sync::MutexGuard<'_, Vec<super::resident_relocation::ResidentStemCache>>,
            SlotSnapshot,
        ),
        String,
    > {
        loop {
            if !self.current() {
                return Err("Committed stem preparation superseded".into());
            }
            let owners = self
                .owners
                .lock()
                .map_err(|_| "resident ownership lock poisoned")?;
            match self.observe_control() {
                PlacementObservation::Current(resolved) => {
                    self.status
                        .preparation_clock
                        .observe_at(false, Instant::now())?;
                    return Ok((owners, resolved));
                }
                PlacementObservation::Pending => {
                    self.status
                        .preparation_clock
                        .observe_at(true, Instant::now())?;
                    #[cfg(test)]
                    self.status
                        .control_wait_observed
                        .store(true, Ordering::Release);
                    drop(owners);
                }
                _ => return Err("Committed stem content owner was retired".into()),
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn prepare(mut self) -> Result<(), String> {
        let deadline = Instant::now() + DEADLINE;
        let pin = loop {
            if !self.current() {
                return Err("Committed stem capture superseded".into());
            }
            if let Ok(pin) = self.receiver.pop() {
                break pin;
            }
            if Instant::now() >= deadline {
                return Err("Committed stem voice capture deadline expired".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let stems = if self.target == StemMixMode::FullMix {
            pin.stems.clone()
        } else {
            // Serialize admission and allocation against every existing same-pad resident reader.
            let (_owners, preparation_slot) = self.current_owners()?;
            if !self.current() {
                return Err("Committed stem preparation superseded".into());
            }
            let readers = self
                .assets
                .held_same_pad_reader_backings(
                    &self.history,
                    preparation_slot.slot(),
                    &self.selection.pair.source_lease,
                )
                .map_err(|error| error.to_string())?;
            let mut held = vec![pin.sample.samples.clone()];
            for samples in readers {
                if !held.iter().any(|old| Arc::ptr_eq(old, &samples)) {
                    held.push(samples);
                }
            }
            if let Some(set) = &pin.stems {
                for component in &set.stems {
                    if !held.iter().any(|old| Arc::ptr_eq(old, &component.samples)) {
                        held.push(component.samples.clone());
                    }
                }
            }
            let held_bytes = held
                .iter()
                .try_fold(0usize, |sum, samples| {
                    samples
                        .len()
                        .checked_mul(4)
                        .and_then(|bytes| sum.checked_add(bytes))
                })
                .ok_or("committed stem held PCM overflow")?;
            let new_bytes = if pin.stems.is_some() && self.candidate.is_none() {
                0
            } else {
                pin.sample
                    .samples
                    .len()
                    .checked_mul(16)
                    .ok_or("committed stem component PCM overflow")?
            };
            #[cfg(test)]
            let pcm_limit = self.pcm_limit;
            #[cfg(not(test))]
            let pcm_limit = super::cold_jobs::PCM_LIMIT_BYTES;
            if held_bytes
                .checked_add(new_bytes)
                .and_then(|bytes| bytes.checked_add(64 * 1024))
                .is_none_or(|peak| peak > pcm_limit)
            {
                return Err("committed stem residency exceeds 1-GiB transient admission".into());
            }
            let shared = if pin.stems.is_none() && self.candidate.is_none() {
                self.assets
                    .shared_stems(
                        &self.selection.generation_path,
                        &pin.sample,
                        &self.selection.source_version,
                        self.binding.binding.sample_rate_hz,
                    )
                    .map_err(|error| error.to_string())?
                    .filter(|set| *set.complete_set_identity == *self.selection.identity)
            } else {
                None
            };
            let existing = if self.candidate.is_none() {
                pin.stems.as_ref().or(shared.as_ref())
            } else {
                None
            };
            let mut reused = vec![&pin.sample];
            if let Some(set) = existing {
                reused.extend(set.stems.iter());
            }
            let bytes = if existing.is_some() {
                0
            } else {
                pin.sample
                    .samples
                    .len()
                    .checked_mul(16)
                    .ok_or("resident component byte overflow")?
            };
            let mut resident = self
                .resident
                .reserve(bytes, &reused, 5)
                .map_err(|error| error.to_string())?;
            resident
                .reserve_reader_slots(1)
                .map_err(|error| error.to_string())?;
            let mut set = if let Some(set) = existing
                && self.candidate.is_none()
            {
                if *set.complete_set_identity != *self.selection.identity {
                    return Err("effective stem set differs from registered selection".into());
                }
                let mut set = set.clone();
                if pin.stems.is_none() {
                    // Shared cache reconstruction has not been admitted for this
                    // subscriber, so its placeholder takes this operation's stamp.
                    set.publication = self.status.publication.clone();
                }
                set
            } else {
                let components = self
                    .selection
                    .pair
                    .reader
                    .prepare_component_views_cancellable(&pin.sample, &|| !self.current())?;
                PreparedStemSet {
                    complete_set_identity: self.selection.identity.clone(),
                    accepted_timing: pin.accepted,
                    reference_samples: pin.sample.samples.clone(),
                    reference_residency: pin.sample.residency.clone(),
                    publication: self.status.publication.clone(),
                    source_version_hash: super::stem_cache::source_version_hash(
                        &self.selection.source_version,
                    ),
                    sample_rate_hz: self.binding.binding.sample_rate_hz,
                    channels: pin.sample.channels,
                    frame_count: pin.sample.frame_count(),
                    available_mask: STEM_COMPONENT_MASK,
                    stems: components,
                }
            };
            // Admitted readers keep their actual complete-set/history owner even when
            // an independent disk-selection receipt seals equal content at new refs.
            // New components and shared reconstructions take this operation's stamp;
            // its receipt must not restamp an admitted set or its productive Native.
            set.accepted_timing = pin.accepted;
            set.reference_samples = pin.sample.samples.clone();
            set.reference_residency = pin.sample.residency.clone();
            resident
                .commit_stems(&set)
                .map_err(|error| error.to_string())?;
            self.assets
                .retain_stem_pair_reserved(
                    self.selection.generation_path.clone(),
                    super::project_assets::stem_readers::StemPairReaders {
                        reference: &pin.sample,
                        version: &self.selection.source_version,
                        identity: &set.complete_set_identity,
                        stems: Some(&set),
                    },
                    self.selection.pair.reader.clone_pins(
                        self.selection
                            .pair
                            .root
                            .parent()
                            .ok_or("pair project root missing")?,
                    )?,
                    self.selection.pair.source_lease.clone(),
                    &mut resident,
                )
                .map_err(|error| error.to_string())?;
            Some(set)
        };
        self.publish_prepared(pin, stems)
    }

    fn publish_prepared(
        &self,
        pin: StemVoicePin,
        stems: Option<PreparedStemSet>,
    ) -> Result<(), String> {
        let pin_requires_stems = pin.plan.selection.required_stem_hash().is_some();
        let mut pin = Some(pin);
        let mut stems = Some(stems);
        let mut pending = None;
        let mut message = None;
        let mut window_predecessor = None;
        let mut queue_attempts = 0;
        loop {
            if !self.current() {
                return Err("Committed stem adoption superseded".into());
            }
            let mut producer = self.producer.lock().map_err(|_| "producer lock poisoned")?;
            if !producer.is_full() {
                let mut owners = self
                    .owners
                    .lock()
                    .map_err(|_| "resident ownership lock poisoned")?;
                let resolved = match self.observe_control() {
                    PlacementObservation::Current(resolved) => {
                        self.status
                            .preparation_clock
                            .observe_at(false, Instant::now())?;
                        resolved
                    }
                    PlacementObservation::Pending => {
                        self.status
                            .preparation_clock
                            .observe_at(true, Instant::now())?;
                        #[cfg(test)]
                        self.status
                            .control_wait_observed
                            .store(true, Ordering::Release);
                        drop(owners);
                        drop(producer);
                        std::thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    _ => return Err("Committed stem content owner was retired".into()),
                };
                let slot = resolved.slot();
                if !self.proposal.current()
                    || self.intent_epoch.load(Ordering::Acquire) != self.base_intent
                    || owners[slot].ordinary_publication_pending()
                    || owners[slot].committed.claimed()
                {
                    return Err("committed proposal authority changed before arming".into());
                }
                if self.policy_only && owners[slot].committed.explicit_busy() {
                    return Err("[resident_stem_interest_busy] explicit stem intent is preparing or awaiting effective acknowledgement".into());
                }
                let proposed = owners[slot]
                    .committed
                    .proposal_slot(self.policy_only)
                    .as_ref()
                    .filter(|pending| Arc::ptr_eq(&pending.status, &self.status))
                    .cloned()
                    .ok_or("committed preparation proposal changed")?;
                if !proposed.owner.source_current()
                    || self.status.publication.status() == "rejected"
                {
                    return Err("committed proposal source changed before arming".into());
                }
                let retain_components = if self.policy_only {
                    self.preload || owners[slot].committed.explicit_interest || pin_requires_stems
                } else {
                    self.target == StemMixMode::AllStems || owners[slot].committed.policy_interest
                };
                if let Some((epoch, expected)) = self.status.policy_epoch.as_ref()
                    && next_epoch(epoch)? != *expected
                {
                    return Err("committed policy interest changed before arming".into());
                }
                if pending.is_none() {
                    window_predecessor = if self.candidate.is_none() && !self.policy_only {
                        owners[slot]
                            .prepare_window_supersession()
                            .map_err(|error| error.to_string())?
                    } else {
                        None
                    };
                    pending = Some(PendingResidency {
                        owner: proposed.owner.clone(),
                        status: proposed.status.clone(),
                        stems: stems.as_ref().expect("held component payload").clone(),
                        full_mix: proposed.full_mix,
                        admitted: proposed.admitted,
                        candidate: proposed.candidate.clone(),
                        policy_only: proposed.policy_only,
                        retain_components,
                        preload: proposed.preload,
                    });
                    // All payload allocation and owner cloning precede the shared mutation claim.
                    message = Some(ControlMessage::CommittedStemWork(Box::new(
                        CommittedStemRtWork::Adopt(StemResidencyTransaction {
                            id: self.id,
                            binding: self.binding.binding,
                            selection: self.selection.rt_permit(),
                            status: self.status.clone(),
                            launch_revision: self.launch_revision,
                            target: self.target,
                            mask: self.mask,
                            candidate: self.candidate.clone(),
                            pin: pin.take().expect("one captured voice pin"),
                            stems: stems.take().expect("one prepared component payload"),
                            retired_stems: None,
                            retired_status: None,
                            policy_only: self.policy_only,
                            retain_components,
                        }),
                    )));
                }
                let mut displaced = None;
                let mut proposal_owner = None;
                let mut outcome = None;
                pending.as_mut().expect("held residency payload").admitted = Instant::now();
                self.status.publish_control(|| {
                    if !self.proposal.armed() {
                        self.status.publication.mark_pending()?;
                        self.proposal.arm()?;
                    }
                    outcome = Some(self.control.commit_at(
                        &self.binding,
                        &self.intent_epoch,
                        resolved,
                        || {
                            !self.cancelled.load(Ordering::Acquire)
                                && !self.status.failure_pending.load(Ordering::Acquire)
                                && self.status.publication.status() == "pending"
                                && self.proposal.current()
                                && self.status.policy_epoch.as_ref().is_none_or(
                                    |(epoch, expected)| {
                                        epoch.load(Ordering::Acquire) == expected.saturating_sub(1)
                                    },
                                )
                                && (!self.policy_only
                                    || self.status.publication.epoch.load(Ordering::Acquire)
                                        == self.status.publication.expected.saturating_sub(1))
                                && self.selection.source_current()
                                && self.selection.ownership.binding_launch_current(
                                    self.id,
                                    self.binding.binding,
                                    self.launch_revision,
                                )
                                && (self.target == StemMixMode::FullMix
                                    || self.selection.current()
                                    || self.candidate.as_ref().is_some_and(|candidate| {
                                        !candidate.adopted()
                                            && candidate.current()
                                            && !self.selection.revoked.load(Ordering::Acquire)
                                    }))
                                && window_predecessor
                                    .as_ref()
                                    .is_none_or(|previous| previous.claim())
                        },
                        || {
                            if self.policy_only {
                                self.status
                                    .publication
                                    .epoch
                                    .store(self.status.publication.expected, Ordering::Release);
                            }
                            // Move every displaced owner out; none is destroyed under the marker.
                            displaced = owners[slot]
                                .committed
                                .pending_slot(self.policy_only)
                                .replace(pending.take().expect("held residency payload"));
                            proposal_owner = owners[slot]
                                .committed
                                .proposal_slot(self.policy_only)
                                .take();
                            self.status.control_published.store(true, Ordering::Release);
                            producer
                                .push(message.take().expect("held committed stem message"))
                                .expect("reserved single-producer committed adoption capacity");
                        },
                    ));
                    Ok(())
                })?;
                drop(owners);
                drop(producer);
                if let Some(previous) = window_predecessor.as_ref() {
                    if outcome.as_ref().is_some_and(|outcome| outcome.is_ok()) {
                        previous.finish(&self.selection.ownership, slot);
                    }
                }
                if let Some(previous) = displaced.as_ref() {
                    previous
                        .status
                        .fail("Committed stem residency or mode ownership changed".into());
                }
                drop(proposal_owner);
                drop(displaced);
                match outcome.expect("one publication attempt") {
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
                continue;
            }
            drop(producer);
            queue_attempts += 1;
            if queue_attempts >= 8 {
                return Err("committed stem command queue is full".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(test)]
#[path = "committed_control_publication_tests.rs"]
mod control_publication_tests;
