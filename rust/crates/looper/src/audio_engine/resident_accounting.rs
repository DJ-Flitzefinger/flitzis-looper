//! Scoped resident admission, entirely outside the callback.
//!
//! The asset gate serializes pending costs and weak owner registration. A PCM
//! locator alone is not an owner: the matching scope-specific resident view must
//! also live. This ledger cannot supply playback buffers or file authority.

use super::project_assets::{PcmHistory, ProjectAssets};
use super::resident_host_memory::{HostAdmissionError, HostHeadroom, HostReservation};
use crate::messages::{PreparedStemSet, ResidentSourceView, SampleBuffer};
use std::collections::HashSet;
use std::io;
use std::sync::{Arc, Weak};

pub(super) const DEFAULT_BUDGET_MIB: usize = 0;
pub(super) const MIN_BUDGET_MIB: usize = 128;
pub(super) const MAX_BUDGET_MIB: usize = 16384;
const MIB: usize = 1024 * 1024;
const MAX_SCOPES: usize = 1024;
// At most four addressed incoming selections plus one assignment copy can
// coexist per one of the existing 32 admitted source jobs. These are allocation
// promises, not additional workers or job lanes.
const MAX_PENDING_RESERVATIONS: usize = 32 * 5;
// The existing 1024 reader records each permit at most 128 live source readers.
// This bounds accounting witnesses, not an additional PCM ownership pool.
const MAX_OWNER_WITNESSES: usize = 1024 * 128;

/// An opaque control/worker scope. It contains no PCM or callback destructor.
#[derive(Default)]
pub(super) struct ResidentAccount {
    #[cfg(test)]
    host_image: std::sync::Mutex<Option<Result<HostHeadroom, String>>>,
}

impl ResidentAccount {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn host_headroom(&self) -> Result<HostHeadroom, String> {
        #[cfg(test)]
        if let Some(image) = self
            .host_image
            .lock()
            .map_err(|_| "test host image poisoned")?
            .as_ref()
        {
            return image.clone();
        }
        super::resident_host_memory::host_headroom()
    }

    /// Complete legacy conversion scratch is separate from selected resident ownership.
    pub(super) fn reserve_transient(
        &self,
        bytes: usize,
    ) -> Result<HostReservation, ResidentAdmissionError> {
        HostReservation::reserve_with(bytes, || self.host_headroom())
            .map_err(ResidentAdmissionError::from)
    }

    #[cfg(test)]
    pub(super) fn set_host_memory_for_test(&self, image: Result<HostHeadroom, String>) {
        *self.host_image.lock().unwrap() = Some(image);
    }

    #[cfg(test)]
    pub(super) fn clear_host_memory_for_test(&self) {
        *self.host_image.lock().unwrap() = None;
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum ResidentAdmissionError {
    #[error(transparent)]
    Host(#[from] HostAdmissionError),
    #[error(
        "[resident_pcm_budget_exceeded] resident PCM budget exceeded: requested={requested_bytes} held={held_bytes} pending={pending_bytes} budget={budget_bytes} bytes"
    )]
    BudgetExceeded {
        requested_bytes: usize,
        held_bytes: usize,
        pending_bytes: usize,
        budget_bytes: usize,
    },
    #[error("resident PCM admission capacity exceeded: {0}")]
    Capacity(&'static str),
    #[error("invalid resident PCM admission: {0}")]
    Invalid(&'static str),
    #[error("resident PCM asset gate poisoned")]
    Poisoned,
}

impl ResidentAdmissionError {
    /// Keep resource pressure typed through the warm-cache IO boundary.
    pub(super) fn into_io_error(self) -> io::Error {
        let kind = if matches!(self, Self::BudgetExceeded { .. } | Self::Host(_)) {
            io::ErrorKind::OutOfMemory
        } else {
            io::ErrorKind::Other
        };
        io::Error::new(kind, self)
    }
}

struct ResidentOwner {
    pcm: Weak<[f32]>,
    view: Weak<ResidentSourceView>,
}

struct PendingReservation {
    id: u64,
    bytes: usize,
    owner_slots: usize,
    reader_slots: usize,
    history: Option<HistoryReservation>,
    cold_reader: Option<(u64, usize)>,
    pinned_backings: Vec<Arc<[f32]>>,
    host: HostReservation,
}

struct HistoryReservation {
    history: Weak<PcmHistory>,
    row: Arc<()>,
    slots: usize,
}

struct ResidentScope {
    account: Arc<ResidentAccount>,
    budget_bytes: usize,
    owners: Vec<ResidentOwner>,
    pending: Vec<PendingReservation>,
    next_reservation: u64,
}

impl ResidentScope {
    fn snapshot(&self) -> Result<ResidentSnapshot, ResidentAdmissionError> {
        self.snapshot_with_addresses().map(|(snapshot, _)| snapshot)
    }

    fn snapshot_with_addresses(
        &self,
    ) -> Result<(ResidentSnapshot, HashSet<usize>), ResidentAdmissionError> {
        let mut seen = HashSet::new();
        let mut held = 0usize;
        for owner in &self.owners {
            // Upgrade the actual scoped witness first. A foreign scope holding
            // the same PCM cannot make an ended witness live again.
            let Some(_view) = owner.view.upgrade() else {
                continue;
            };
            let Some(pcm) = owner.pcm.upgrade() else {
                continue;
            };
            if seen.insert(pcm.as_ptr() as usize) {
                held = held
                    .checked_add(pcm_bytes(&pcm)?)
                    .ok_or(ResidentAdmissionError::Invalid("held byte overflow"))?;
            }
        }
        for pcm in self
            .pending
            .iter()
            .flat_map(|record| &record.pinned_backings)
        {
            if seen.insert(pcm.as_ptr() as usize) {
                held = held
                    .checked_add(pcm_bytes(pcm)?)
                    .ok_or(ResidentAdmissionError::Invalid("pinned held byte overflow"))?;
            }
        }
        let pending = self.pending.iter().try_fold(0usize, |sum, record| {
            sum.checked_add(record.bytes)
                .ok_or(ResidentAdmissionError::Invalid("pending byte overflow"))
        })?;
        let total = held
            .checked_add(pending)
            .ok_or(ResidentAdmissionError::Invalid("admitted byte overflow"))?;
        let snapshot = ResidentSnapshot {
            automatic: self.budget_bytes == 0,
            budget_mib: self.budget_bytes / MIB,
            budget_bytes: self.budget_bytes,
            held_unique_pcm_bytes: held,
            pending_resident_bytes: pending,
            admitted_total_bytes: total,
            unique_backing_count: seen.len(),
            pending_reservation_count: self.pending.len(),
            available_bytes: self.budget_bytes.saturating_sub(total),
            over_budget: self.budget_bytes != 0 && total > self.budget_bytes,
        };
        Ok((snapshot, seen))
    }

    fn live_addresses(&self) -> HashSet<usize> {
        let mut seen: HashSet<_> = self
            .owners
            .iter()
            .filter(|owner| owner.view.strong_count() > 0)
            .filter_map(|owner| owner.pcm.upgrade())
            .map(|pcm| pcm.as_ptr() as usize)
            .collect();
        seen.extend(
            self.pending
                .iter()
                .flat_map(|record| &record.pinned_backings)
                .map(|pcm| pcm.as_ptr() as usize),
        );
        seen
    }
}

#[derive(Default)]
pub(super) struct ResidentLedger {
    scopes: Vec<ResidentScope>,
}

impl ResidentLedger {
    pub(super) fn prune(&mut self) {
        for scope in &mut self.scopes {
            scope
                .owners
                .retain(|owner| owner.view.strong_count() > 0 && owner.pcm.strong_count() > 0);
        }
        self.scopes.retain(|scope| {
            !scope.owners.is_empty()
                || !scope.pending.is_empty()
                || Arc::strong_count(&scope.account) > 1
        });
    }

    fn scope_index(
        &mut self,
        account: &Arc<ResidentAccount>,
    ) -> Result<usize, ResidentAdmissionError> {
        if let Some(index) = self
            .scopes
            .iter()
            .position(|scope| Arc::ptr_eq(&scope.account, account))
        {
            return Ok(index);
        }
        if self.scopes.len() >= MAX_SCOPES {
            return Err(ResidentAdmissionError::Capacity("engine scopes"));
        }
        self.scopes.push(ResidentScope {
            account: account.clone(),
            budget_bytes: DEFAULT_BUDGET_MIB * MIB,
            owners: Vec::new(),
            pending: Vec::new(),
            next_reservation: 0,
        });
        Ok(self.scopes.len() - 1)
    }

    pub(super) fn reserved_cold_reader_slots(&self, assignment: u64) -> usize {
        self.scopes
            .iter()
            .flat_map(|scope| &scope.pending)
            .filter_map(|record| record.cold_reader)
            .filter(|(id, _)| *id == assignment)
            .map(|(_, slots)| slots)
            .sum()
    }
    pub(super) fn reserved_reader_slots(&self) -> usize {
        self.scopes
            .iter()
            .flat_map(|scope| &scope.pending)
            .map(|record| record.reader_slots)
            .sum()
    }
}

pub(super) struct ResidentSnapshot {
    pub automatic: bool,
    pub budget_mib: usize,
    pub budget_bytes: usize,
    pub held_unique_pcm_bytes: usize,
    pub pending_resident_bytes: usize,
    pub admitted_total_bytes: usize,
    pub unique_backing_count: usize,
    pub pending_reservation_count: usize,
    pub available_bytes: usize,
    pub over_budget: bool,
}

/// Worker-only RAII promise. Actual readers retain their existing view Arcs.
pub(super) struct ResidentReservation {
    assets: Arc<ProjectAssets>,
    account: Arc<ResidentAccount>,
    id: u64,
}

fn pcm_bytes(pcm: &[f32]) -> Result<usize, ResidentAdmissionError> {
    pcm.len()
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or(ResidentAdmissionError::Invalid("PCM byte overflow"))
}

fn validate_sample(sample: &SampleBuffer) -> Result<(), ResidentAdmissionError> {
    let view = sample
        .residency
        .as_ref()
        .ok_or(ResidentAdmissionError::Invalid(
            "resident owner view missing",
        ))?;
    if !sample.valid_residency(view.source.sample_rate_hz, view.source.channels) {
        return Err(ResidentAdmissionError::Invalid("resident owner geometry"));
    }
    Ok(())
}

impl ProjectAssets {
    pub(super) fn set_resident_budget_mib(
        &self,
        account: &Arc<ResidentAccount>,
        mib: usize,
    ) -> Result<(), ResidentAdmissionError> {
        if mib != 0 && !(MIN_BUDGET_MIB..=MAX_BUDGET_MIB).contains(&mib) {
            return Err(ResidentAdmissionError::Invalid(
                "budget must be 0 (automatic) or 128..16384 MiB",
            ));
        }
        let bytes = mib
            .checked_mul(MIB)
            .ok_or(ResidentAdmissionError::Invalid("budget byte overflow"))?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| ResidentAdmissionError::Poisoned)?;
        Self::prune_pcm_locators(&mut state);
        let index = state.residency.scope_index(account)?;
        state.residency.scopes[index].budget_bytes = bytes;
        Ok(())
    }

    pub(super) fn resident_budget_status(
        &self,
        account: &Arc<ResidentAccount>,
    ) -> Result<ResidentSnapshot, ResidentAdmissionError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ResidentAdmissionError::Poisoned)?;
        Self::prune_pcm_locators(&mut state);
        let index = state.residency.scope_index(account)?;
        state.residency.scopes[index].snapshot()
    }

    /// Worker snapshot custody is charged even if its original resident view ends.
    /// These pins grant accounting ownership only, never source/publication authority.
    pub(super) fn pin_resident_backings(
        self: &Arc<Self>,
        account: &Arc<ResidentAccount>,
        backings: &[Arc<[f32]>],
    ) -> Result<ResidentReservation, ResidentAdmissionError> {
        let token = self.reserve_resident(account, 0, &[], backings.len())?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| ResidentAdmissionError::Poisoned)?;
        let index = state.residency.scope_index(account)?;
        let scope = &mut state.residency.scopes[index];
        let (snapshot, mut seen) = scope.snapshot_with_addresses()?;
        let mut added = 0usize;
        let mut pins = Vec::new();
        for pcm in backings {
            if !pins.iter().any(|pin| Arc::ptr_eq(pin, pcm)) {
                pins.push(pcm.clone());
            }
            if seen.insert(pcm.as_ptr() as usize) {
                added = added
                    .checked_add(pcm_bytes(pcm)?)
                    .ok_or(ResidentAdmissionError::Invalid("pinned byte overflow"))?;
            }
        }
        if scope.budget_bytes != 0
            && added > 0
            && snapshot
                .admitted_total_bytes
                .checked_add(added)
                .is_none_or(|total| total > scope.budget_bytes)
        {
            return Err(ResidentAdmissionError::BudgetExceeded {
                requested_bytes: added,
                held_bytes: snapshot.held_unique_pcm_bytes,
                pending_bytes: snapshot.pending_resident_bytes,
                budget_bytes: scope.budget_bytes,
            });
        }
        scope
            .pending
            .iter_mut()
            .find(|record| record.id == token.id)
            .ok_or(ResidentAdmissionError::Invalid("pin reservation ended"))?
            .pinned_backings = pins;
        drop(state);
        Ok(token)
    }

    /// Reserve prospective account ownership, not total disk extent or scratch.
    /// Reused PCM costs zero only when this same account already owns it.
    pub(super) fn reserve_resident(
        self: &Arc<Self>,
        account: &Arc<ResidentAccount>,
        new_bytes: usize,
        reused: &[&SampleBuffer],
        owner_slots: usize,
    ) -> Result<ResidentReservation, ResidentAdmissionError> {
        for sample in reused {
            validate_sample(sample)?;
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| ResidentAdmissionError::Poisoned)?;
        Self::prune_pcm_locators(&mut state);
        ResidentReservation::reject_foreign_views(&state.residency, account, reused)?;
        let index = state.residency.scope_index(account)?;
        let scope = &mut state.residency.scopes[index];
        let (snapshot, mut seen) = scope.snapshot_with_addresses()?;
        let mut requested = new_bytes;
        let mut reused_owners: Vec<ResidentOwner> = Vec::new();
        for sample in reused {
            if seen.insert(sample.samples.as_ptr() as usize) {
                requested = requested
                    .checked_add(pcm_bytes(&sample.samples)?)
                    .ok_or(ResidentAdmissionError::Invalid("reservation byte overflow"))?;
            }
            let pcm = Arc::downgrade(&sample.samples);
            let view = Arc::downgrade(sample.residency.as_ref().unwrap());
            if scope
                .owners
                .iter()
                .chain(&reused_owners)
                .any(|owner| owner.view.ptr_eq(&view) && !owner.pcm.ptr_eq(&pcm))
            {
                return Err(ResidentAdmissionError::Invalid(
                    "different PCM requires a distinct resident owner view",
                ));
            }
            if !scope
                .owners
                .iter()
                .chain(&reused_owners)
                .any(|owner| owner.pcm.ptr_eq(&pcm) && owner.view.ptr_eq(&view))
            {
                reused_owners.push(ResidentOwner { pcm, view });
            }
        }
        let pending_owner_slots =
            owner_slots
                .checked_sub(reused_owners.len())
                .ok_or(ResidentAdmissionError::Invalid(
                    "reused owners exceed reserved slots",
                ))?;
        if scope.budget_bytes != 0
            && requested > 0
            && snapshot
                .admitted_total_bytes
                .checked_add(requested)
                .is_none_or(|total| total > scope.budget_bytes)
        {
            return Err(ResidentAdmissionError::BudgetExceeded {
                requested_bytes: requested,
                held_bytes: snapshot.held_unique_pcm_bytes,
                pending_bytes: snapshot.pending_resident_bytes,
                budget_bytes: scope.budget_bytes,
            });
        }
        if scope.pending.len() >= MAX_PENDING_RESERVATIONS {
            return Err(ResidentAdmissionError::Capacity(
                "pending allocation reservations",
            ));
        }
        let reserved_owners = scope.pending.iter().try_fold(0usize, |sum, record| {
            sum.checked_add(record.owner_slots)
                .ok_or(ResidentAdmissionError::Invalid("owner slot overflow"))
        })?;
        if scope
            .owners
            .len()
            .checked_add(reserved_owners)
            .and_then(|count| count.checked_add(owner_slots))
            .is_none_or(|count| count > MAX_OWNER_WITNESSES)
        {
            return Err(ResidentAdmissionError::Capacity("scoped owner witnesses"));
        }
        let id = scope
            .next_reservation
            .checked_add(1)
            .ok_or(ResidentAdmissionError::Invalid("reservation id exhausted"))?;
        // OS memory already includes every reused backing, including backings
        // newly registered to this account. Promise only new allocation bytes.
        let host = HostReservation::reserve_with(new_bytes, || account.host_headroom())?;
        scope.next_reservation = id;
        // Real reused candidates already pin their local view and PCM. Register
        // them with admission, so retirement of an earlier same-scope owner
        // cannot create a zero-cost handover gap before the later commit.
        scope.owners.extend(reused_owners);
        scope.pending.push(PendingReservation {
            id,
            bytes: new_bytes,
            owner_slots: pending_owner_slots,
            reader_slots: 0,
            history: None,
            cold_reader: None,
            pinned_backings: Vec::new(),
            host,
        });
        Ok(ResidentReservation {
            assets: self.clone(),
            account: account.clone(),
            id,
        })
    }
}

impl ResidentReservation {
    /// Exact final backing still promised by this operation, separate from scratch.
    pub(super) fn remaining_bytes(&self) -> Result<usize, ResidentAdmissionError> {
        let state = self
            .assets
            .state
            .lock()
            .map_err(|_| ResidentAdmissionError::Poisoned)?;
        state
            .residency
            .scopes
            .iter()
            .find(|scope| Arc::ptr_eq(&scope.account, &self.account))
            .and_then(|scope| scope.pending.iter().find(|record| record.id == self.id))
            .map(|record| record.bytes)
            .ok_or(ResidentAdmissionError::Invalid("reservation ended"))
    }

    /// Reserve this pad's existing 128-reader history before selected allocation.
    /// All paths use asset -> history lock order; no caller holds that history lock.
    pub(super) fn reserve_history_slot(
        &mut self,
        history: &Arc<PcmHistory>,
        id: usize,
        reused: Option<&SampleBuffer>,
    ) -> Result<(), ResidentAdmissionError> {
        let mut state = self
            .assets
            .state
            .lock()
            .map_err(|_| ResidentAdmissionError::Poisoned)?;
        let weak_history = Arc::downgrade(history);
        let mut pads = history
            .lock()
            .map_err(|_| ResidentAdmissionError::Invalid("history gate poisoned"))?;
        let pad = pads
            .get_mut(id)
            .ok_or(ResidentAdmissionError::Invalid("history pad missing"))?;
        pad.retain(|pcm| pcm.strong_count() > 0);
        let reuses = reused.is_some_and(|sample| {
            pad.iter()
                .any(|pcm| pcm.ptr_eq(&Arc::downgrade(&sample.samples)))
        });
        let slots = usize::from(!reuses);
        let pending = state
            .residency
            .scopes
            .iter()
            .flat_map(|scope| &scope.pending)
            .filter_map(|record| record.history.as_ref())
            .filter(|record| {
                Arc::ptr_eq(&record.row, &pad.identity) && record.history.ptr_eq(&weak_history)
            })
            .try_fold(0usize, |sum, record| {
                sum.checked_add(record.slots)
                    .ok_or(ResidentAdmissionError::Invalid("history slot overflow"))
            })?;
        if pad
            .len()
            .checked_add(pending)
            .and_then(|count| count.checked_add(slots))
            .is_none_or(|count| count > 128)
        {
            return Err(ResidentAdmissionError::Capacity(
                "same-pad source history (128)",
            ));
        }
        let index = state.residency.scope_index(&self.account)?;
        let record = state.residency.scopes[index]
            .pending
            .iter_mut()
            .find(|record| record.id == self.id)
            .ok_or(ResidentAdmissionError::Invalid("reservation ended"))?;
        if record.history.is_some() {
            return Err(ResidentAdmissionError::Invalid(
                "history slot already reserved",
            ));
        }
        record.history = Some(HistoryReservation {
            history: weak_history,
            row: pad.identity.clone(),
            slots,
        });
        Ok(())
    }

    /// Atomically transfer source ownership and its reserved same-pad history slot.
    pub(super) fn commit_source_history(
        &mut self,
        sample: &SampleBuffer,
    ) -> Result<(), ResidentAdmissionError> {
        validate_sample(sample)?;
        let mut state = self
            .assets
            .state
            .lock()
            .map_err(|_| ResidentAdmissionError::Poisoned)?;
        let index = state.residency.scope_index(&self.account)?;
        let record_index = state.residency.scopes[index]
            .pending
            .iter()
            .position(|record| record.id == self.id)
            .ok_or(ResidentAdmissionError::Invalid("reservation ended"))?;
        let reserved = state.residency.scopes[index].pending[record_index]
            .history
            .as_ref()
            .ok_or(ResidentAdmissionError::Invalid("history slot not reserved"))?;
        let history = reserved
            .history
            .upgrade()
            .ok_or(ResidentAdmissionError::Invalid("history owner ended"))?;
        let row = reserved.row.clone();
        let slots = reserved.slots;
        let mut pads = history
            .lock()
            .map_err(|_| ResidentAdmissionError::Invalid("history gate poisoned"))?;
        let pad = pads
            .iter_mut()
            .find(|pad| Arc::ptr_eq(&pad.identity, &row))
            .ok_or(ResidentAdmissionError::Invalid(
                "reserved history custody ended",
            ))?;
        pad.retain(|pcm| pcm.strong_count() > 0);
        let pcm = Arc::downgrade(&sample.samples);
        let append = !pad.iter().any(|held| held.ptr_eq(&pcm));
        if append && (slots == 0 || pad.len() >= 128) {
            return Err(ResidentAdmissionError::Invalid(
                "actual history exceeds reserved slot",
            ));
        }
        Self::commit_owners(&mut state.residency, &self.account, self.id, &[sample])?;
        if append {
            pad.push(pcm);
        }
        state.residency.scopes[index].pending[record_index]
            .history
            .as_mut()
            .unwrap()
            .slots = 0;
        Ok(())
    }

    /// Promise one of the existing assignment's 128 PCM locators, not another assignment.
    pub(super) fn reserve_cold_reader_slot(
        &mut self,
        lease: &super::cold_store::CommittedColdLease,
        reused: Option<&SampleBuffer>,
    ) -> Result<(), ResidentAdmissionError> {
        let mut state = self
            .assets
            .state
            .lock()
            .map_err(|_| ResidentAdmissionError::Poisoned)?;
        let assignment = lease.assignment_id();
        let reader = state
            .readers
            .iter_mut()
            .find(|reader| {
                reader
                    .cold
                    .as_ref()
                    .is_some_and(|owned| owned.assignment_id() == assignment)
            })
            .ok_or(ResidentAdmissionError::Invalid(
                "cold reader assignment missing",
            ))?;
        reader.pcm.retain(|pcm| pcm.strong_count() > 0);
        let slots = usize::from(!reused.is_some_and(|sample| {
            reader
                .pcm
                .iter()
                .any(|pcm| pcm.ptr_eq(&Arc::downgrade(&sample.samples)))
        }));
        let held = reader.pcm.len();
        let pending = state.residency.reserved_cold_reader_slots(assignment);
        if held
            .checked_add(pending)
            .and_then(|n| n.checked_add(slots))
            .is_none_or(|n| n > 128)
        {
            return Err(ResidentAdmissionError::Capacity(
                "assignment live readers (128)",
            ));
        }
        let scope = state.residency.scope_index(&self.account)?;
        let record = state.residency.scopes[scope]
            .pending
            .iter_mut()
            .find(|record| record.id == self.id)
            .ok_or(ResidentAdmissionError::Invalid("reservation ended"))?;
        if record.cold_reader.is_some() {
            return Err(ResidentAdmissionError::Invalid(
                "cold reader already reserved",
            ));
        }
        record.cold_reader = Some((assignment, slots));
        Ok(())
    }
    pub(super) fn consume_cold_reader_slot(
        &mut self,
        assets: &ProjectAssets,
        ledger: &mut ResidentLedger,
        assignment: u64,
        append: bool,
    ) -> Result<(), ResidentAdmissionError> {
        if !std::ptr::eq(assets, self.assets.as_ref()) {
            return Err(ResidentAdmissionError::Invalid("different asset registry"));
        }
        let scope = ledger.scope_index(&self.account)?;
        let record = ledger.scopes[scope]
            .pending
            .iter_mut()
            .find(|record| record.id == self.id)
            .ok_or(ResidentAdmissionError::Invalid("reservation ended"))?;
        let (id, slots) = record
            .cold_reader
            .ok_or(ResidentAdmissionError::Invalid("cold reader not reserved"))?;
        if id != assignment || (append && slots == 0) {
            return Err(ResidentAdmissionError::Invalid(
                "cold reader exceeds promise",
            ));
        }
        record.cold_reader = Some((assignment, 0));
        Ok(())
    }

    pub(super) fn reserve_reader_slots(
        &mut self,
        count: usize,
    ) -> Result<(), ResidentAdmissionError> {
        let mut state = self
            .assets
            .state
            .lock()
            .map_err(|_| ResidentAdmissionError::Poisoned)?;
        if count > ProjectAssets::available_reader_slots(&state) {
            return Err(ResidentAdmissionError::Capacity("asset reader registry"));
        }
        let index = state.residency.scope_index(&self.account)?;
        let record = state.residency.scopes[index]
            .pending
            .iter_mut()
            .find(|record| record.id == self.id)
            .ok_or(ResidentAdmissionError::Invalid("reservation ended"))?;
        record.reader_slots = record
            .reader_slots
            .checked_add(count)
            .ok_or(ResidentAdmissionError::Invalid("reader slot overflow"))?;
        Ok(())
    }

    pub(super) fn consume_reader_slot(
        &mut self,
        assets: &ProjectAssets,
        ledger: &mut ResidentLedger,
    ) -> Result<(), ResidentAdmissionError> {
        if !std::ptr::eq(assets, self.assets.as_ref()) {
            return Err(ResidentAdmissionError::Invalid("different asset registry"));
        }
        let index = ledger.scope_index(&self.account)?;
        let record = ledger.scopes[index]
            .pending
            .iter_mut()
            .find(|record| record.id == self.id)
            .ok_or(ResidentAdmissionError::Invalid("reservation ended"))?;
        record.reader_slots = record
            .reader_slots
            .checked_sub(1)
            .ok_or(ResidentAdmissionError::Invalid("reader slot not reserved"))?;
        Ok(())
    }

    /// Transfer a subset of actual owner views while retaining the unspent promise.
    pub(super) fn commit(
        &mut self,
        samples: &[&SampleBuffer],
    ) -> Result<(), ResidentAdmissionError> {
        for sample in samples {
            validate_sample(sample)?;
        }
        let mut state = self
            .assets
            .state
            .lock()
            .map_err(|_| ResidentAdmissionError::Poisoned)?;
        Self::commit_owners(&mut state.residency, &self.account, self.id, samples)
    }

    fn commit_owners(
        ledger: &mut ResidentLedger,
        account: &Arc<ResidentAccount>,
        id: u64,
        samples: &[&SampleBuffer],
    ) -> Result<(), ResidentAdmissionError> {
        Self::reject_foreign_views(ledger, account, samples)?;
        let index = ledger.scope_index(account)?;
        let scope = &mut ledger.scopes[index];
        let record_index = scope
            .pending
            .iter()
            .position(|record| record.id == id)
            .ok_or(ResidentAdmissionError::Invalid("reservation ended"))?;
        let mut addresses = scope.live_addresses();
        let mut charged = 0usize;
        let mut added: Vec<ResidentOwner> = Vec::new();
        for sample in samples {
            let pcm = Arc::downgrade(&sample.samples);
            let view = Arc::downgrade(sample.residency.as_ref().unwrap());
            if scope
                .owners
                .iter()
                .chain(&added)
                .any(|owner| owner.view.ptr_eq(&view) && !owner.pcm.ptr_eq(&pcm))
            {
                return Err(ResidentAdmissionError::Invalid(
                    "different PCM requires a distinct resident owner view",
                ));
            }
            if scope
                .owners
                .iter()
                .chain(&added)
                .any(|owner| owner.pcm.ptr_eq(&pcm) && owner.view.ptr_eq(&view))
            {
                continue;
            }
            if addresses.insert(sample.samples.as_ptr() as usize) {
                charged = charged
                    .checked_add(pcm_bytes(&sample.samples)?)
                    .ok_or(ResidentAdmissionError::Invalid("transfer byte overflow"))?;
            }
            added.push(ResidentOwner { pcm, view });
        }
        let record = &mut scope.pending[record_index];
        let remaining_bytes =
            record
                .bytes
                .checked_sub(charged)
                .ok_or(ResidentAdmissionError::Invalid(
                    "actual ownership exceeds reserved bytes",
                ))?;
        let remaining_slots =
            record
                .owner_slots
                .checked_sub(added.len())
                .ok_or(ResidentAdmissionError::Invalid(
                    "actual owners exceed reserved slots",
                ))?;
        // All fallible validation precedes the atomic pending -> weak-owner transition.
        record.host.commit(charged)?;
        scope.owners.extend(added);
        record.bytes = remaining_bytes;
        record.owner_slots = remaining_slots;
        Ok(())
    }

    pub(super) fn commit_stems(
        &mut self,
        stems: &PreparedStemSet,
    ) -> Result<(), ResidentAdmissionError> {
        let view = stems
            .reference_residency
            .as_ref()
            .ok_or(ResidentAdmissionError::Invalid(
                "stem reference owner view missing",
            ))?;
        let reference = SampleBuffer {
            channels: stems.channels,
            samples: stems.reference_samples.clone(),
            residency: Some(view.clone()),
        };
        self.commit(&[
            &reference,
            &stems.stems[0],
            &stems.stems[1],
            &stems.stems[2],
            &stems.stems[3],
        ])
    }

    fn reject_foreign_views(
        ledger: &ResidentLedger,
        account: &Arc<ResidentAccount>,
        samples: &[&SampleBuffer],
    ) -> Result<(), ResidentAdmissionError> {
        for scope in &ledger.scopes {
            if Arc::ptr_eq(&scope.account, account) {
                continue;
            }
            for sample in samples {
                let view = Arc::downgrade(sample.residency.as_ref().unwrap());
                if scope.owners.iter().any(|owner| owner.view.ptr_eq(&view)) {
                    return Err(ResidentAdmissionError::Invalid(
                        "foreign scoped view must be rebound",
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Drop for ResidentReservation {
    fn drop(&mut self) {
        let Ok(mut state) = self.assets.state.lock() else {
            return;
        };
        if let Some(scope) = state
            .residency
            .scopes
            .iter_mut()
            .find(|scope| Arc::ptr_eq(&scope.account, &self.account))
        {
            scope.pending.retain(|record| record.id != self.id);
        }
        state.residency.prune();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn sample(value: f32) -> SampleBuffer {
        SampleBuffer {
            channels: 1,
            samples: Arc::from([value; 32]),
            residency: None,
        }
        .with_complete_source(48_000)
    }

    fn local_view(source: &SampleBuffer) -> SampleBuffer {
        SampleBuffer {
            channels: source.channels,
            samples: source.samples.clone(),
            residency: Some(Arc::new((**source.residency.as_ref().unwrap()).clone())),
        }
    }

    fn admit(assets: &Arc<ProjectAssets>, account: &Arc<ResidentAccount>, sample: &SampleBuffer) {
        let mut reservation = assets.reserve_resident(account, 0, &[sample], 1).unwrap();
        reservation.commit(&[sample]).unwrap();
    }

    #[test]
    fn worker_backing_pin_keeps_ended_views_charged_without_foreign_scope_resurrection() {
        let assets = ProjectAssets::isolated();
        let account = ResidentAccount::new();
        let foreign_account = ResidentAccount::new();
        assets.set_resident_budget_mib(&account, 128).unwrap();
        let old = sample(0.5);
        admit(&assets, &account, &old);
        let pins = assets
            .pin_resident_backings(&account, &[old.samples.clone()])
            .unwrap();
        let mut foreign = sample(0.75);
        foreign.samples = old.samples.clone();
        admit(&assets, &foreign_account, &foreign);
        drop(old);
        assert_eq!(
            assets
                .resident_budget_status(&account)
                .unwrap()
                .held_unique_pcm_bytes,
            128
        );
        let boundary = assets
            .reserve_resident(&account, 128 * MIB - 128, &[], 0)
            .unwrap();
        assert!(matches!(
            assets.reserve_resident(&account, 1, &[], 0),
            Err(ResidentAdmissionError::BudgetExceeded { .. })
        ));
        drop(pins);
        assert_eq!(
            assets
                .resident_budget_status(&account)
                .unwrap()
                .held_unique_pcm_bytes,
            0
        );
        assert_eq!(
            assets
                .resident_budget_status(&foreign_account)
                .unwrap()
                .held_unique_pcm_bytes,
            128
        );
        drop(assets.reserve_resident(&account, 128, &[], 0).unwrap());
        drop(boundary);
    }

    #[test]
    fn reserved_history_follows_moved_row_and_refuses_retired_custody() {
        for retire in [false, true] {
            let assets = ProjectAssets::isolated();
            let account = ResidentAccount::new();
            let old = sample(0.5);
            admit(&assets, &account, &old);
            let history = Arc::new(std::sync::Mutex::new(vec![
                vec![Arc::downgrade(&old.samples)].into(),
                super::super::project_assets::PcmHistoryRow::default(),
            ]));
            let mut token = assets.reserve_resident(&account, 128, &[], 1).unwrap();
            token.reserve_history_slot(&history, 0, None).unwrap();
            history.lock().unwrap().swap(0, 1);
            if retire {
                history.lock().unwrap()[1] = super::super::project_assets::PcmHistoryRow::default();
            }
            let next = sample(0.75);
            let result = token.commit_source_history(&next);
            if retire {
                assert!(result.unwrap_err().to_string().contains("custody ended"));
                assert!(history.lock().unwrap()[1].is_empty());
            } else {
                result.unwrap();
                assert!(history.lock().unwrap()[0].is_empty());
                assert!(
                    history.lock().unwrap()[1]
                        .iter()
                        .any(|pcm| pcm.ptr_eq(&Arc::downgrade(&next.samples)))
                );
            }
            drop(token);
            assert_eq!(
                assets
                    .resident_budget_status(&account)
                    .unwrap()
                    .pending_reservation_count,
                0
            );
        }
    }
    #[test]
    fn moved_history_pending_promises_keep_the_original_128_reader_bound() {
        let assets = ProjectAssets::isolated();
        let account = ResidentAccount::new();
        let held: Vec<_> = (0..127).map(|_| sample(0.5)).collect();
        let history = Arc::new(std::sync::Mutex::new(vec![
            held.iter()
                .map(|sample| Arc::downgrade(&sample.samples))
                .collect::<Vec<_>>()
                .into(),
            super::super::project_assets::PcmHistoryRow::default(),
        ]));
        let mut first = assets.reserve_resident(&account, 128, &[], 1).unwrap();
        first.reserve_history_slot(&history, 0, None).unwrap();
        history.lock().unwrap().swap(0, 1);
        let mut second = assets.reserve_resident(&account, 128, &[], 1).unwrap();
        assert!(matches!(
            second.reserve_history_slot(&history, 1, None),
            Err(ResidentAdmissionError::Capacity(_))
        ));
        second.reserve_history_slot(&history, 0, None).unwrap();
        drop(first);
        drop(second);
        assert_eq!(
            assets
                .resident_budget_status(&account)
                .unwrap()
                .pending_reservation_count,
            0
        );
    }

    #[test]
    fn same_scope_sharing_charges_once_until_the_actual_last_view_ends() {
        let assets = ProjectAssets::isolated();
        let account = ResidentAccount::new();
        let first = sample(0.25);
        let second = local_view(&first);
        admit(&assets, &account, &first);
        admit(&assets, &account, &second);
        let status = assets.resident_budget_status(&account).unwrap();
        assert_eq!(status.held_unique_pcm_bytes, 128);
        assert_eq!(status.unique_backing_count, 1);
        assert_eq!(status.pending_resident_bytes, 0);
        drop(first);
        assert_eq!(
            assets
                .resident_budget_status(&account)
                .unwrap()
                .held_unique_pcm_bytes,
            128
        );
        drop(second);
        let status = assets.resident_budget_status(&account).unwrap();
        assert_eq!(status.held_unique_pcm_bytes, 0);
        assert_eq!(status.unique_backing_count, 0);
    }

    #[test]
    fn first_foreign_scope_adoption_charges_its_local_owner_and_does_not_keep_old_scope_charged() {
        let assets = ProjectAssets::isolated();
        let first_account = ResidentAccount::new();
        let second_account = ResidentAccount::new();
        let first = sample(0.5);
        admit(&assets, &first_account, &first);
        assert!(matches!(
            assets.reserve_resident(&second_account, 0, &[&first], 1),
            Err(ResidentAdmissionError::Invalid(_))
        ));
        let second = local_view(&first);
        admit(&assets, &second_account, &second);
        assert!(Arc::ptr_eq(&first.samples, &second.samples));
        assert_eq!(
            assets
                .resident_budget_status(&first_account)
                .unwrap()
                .held_unique_pcm_bytes,
            128
        );
        assert_eq!(
            assets
                .resident_budget_status(&second_account)
                .unwrap()
                .held_unique_pcm_bytes,
            128
        );
        drop(first);
        assert_eq!(
            assets
                .resident_budget_status(&first_account)
                .unwrap()
                .held_unique_pcm_bytes,
            0
        );
        assert_eq!(
            assets
                .resident_budget_status(&second_account)
                .unwrap()
                .held_unique_pcm_bytes,
            128
        );
        drop(second);
        assert_eq!(
            assets
                .resident_budget_status(&second_account)
                .unwrap()
                .held_unique_pcm_bytes,
            0
        );
    }

    #[test]
    fn checked_pending_boundary_reduction_and_raii_preserve_held_readers() {
        let assets = ProjectAssets::isolated();
        let account = ResidentAccount::new();
        account.set_host_memory_for_test(Ok(HostHeadroom {
            available_physical: 64 * 1024 * 1024 * 1024,
            caller_available_commit: 64 * 1024 * 1024 * 1024,
            system_available_commit: 64 * 1024 * 1024 * 1024,
        }));
        assets.set_resident_budget_mib(&account, 256).unwrap();
        let held = sample(0.75);
        let held_address = held.samples.as_ptr();
        admit(&assets, &account, &held);
        let boundary = 256 * MIB - 128;
        let mut pending = assets.reserve_resident(&account, boundary, &[], 1).unwrap();
        assert_eq!(
            assets
                .resident_budget_status(&account)
                .unwrap()
                .admitted_total_bytes,
            256 * MIB
        );
        assert!(matches!(
            assets.reserve_resident(&account, 1, &[], 1),
            Err(ResidentAdmissionError::BudgetExceeded {
                requested_bytes: 1,
                ..
            })
        ));
        assets.set_resident_budget_mib(&account, 128).unwrap();
        let status = assets.resident_budget_status(&account).unwrap();
        assert!(status.over_budget);
        assert_eq!(status.available_bytes, 0);
        assert_eq!(status.held_unique_pcm_bytes, 128);
        assert!(matches!(
            assets.reserve_resident(&account, 1, &[], 1),
            Err(ResidentAdmissionError::BudgetExceeded { .. })
        ));
        // Existing zero-delta reuse remains valid, and the earlier promise can
        // transfer its actual small PCM after reduction without a second admission.
        drop(assets.reserve_resident(&account, 0, &[&held], 0).unwrap());
        let admitted = sample(1.0);
        pending.commit(&[&admitted]).unwrap();
        assert_eq!(held.samples.as_ptr(), held_address);
        assert!(assets.resident_budget_status(&account).unwrap().over_budget);
        drop(pending);
        let status = assets.resident_budget_status(&account).unwrap();
        assert_eq!(status.held_unique_pcm_bytes, 256);
        assert_eq!(status.pending_resident_bytes, 0);
        assert_eq!(status.pending_reservation_count, 0);
        assert!(!status.over_budget);
        assert!(held.samples.iter().all(|value| *value == 0.75));
    }

    #[test]
    fn automatic_default_manual_limit_and_zero_allocation_reuse_keep_distinct_authority() {
        let assets = ProjectAssets::isolated();
        let account = ResidentAccount::new();
        let held = sample(0.5);
        admit(&assets, &account, &held);
        let status = assets.resident_budget_status(&account).unwrap();
        assert!(status.automatic);
        assert_eq!(status.budget_mib, 0);
        assert_eq!(status.budget_bytes, 0);
        assert!(!status.over_budget);
        account.set_host_memory_for_test(Err("query failed".into()));
        let another_view = local_view(&held);
        admit(&assets, &account, &another_view);
        assert!(matches!(
            assets.reserve_resident(&account, 1, &[], 1),
            Err(ResidentAdmissionError::Host(_))
        ));
        assert_eq!(
            assets
                .resident_budget_status(&account)
                .unwrap()
                .held_unique_pcm_bytes,
            128
        );
        assets.set_resident_budget_mib(&account, 128).unwrap();
        let manual = assets.resident_budget_status(&account).unwrap();
        assert!(!manual.automatic);
        assert_eq!(manual.budget_mib, 128);
        assert!(matches!(
            assets.reserve_resident(&account, 128 * MIB, &[], 1),
            Err(ResidentAdmissionError::BudgetExceeded { .. })
        ));
        assets.set_resident_budget_mib(&account, 0).unwrap();
        assert!(assets.resident_budget_status(&account).unwrap().automatic);
        drop(held);
        assert_eq!(
            assets
                .resident_budget_status(&account)
                .unwrap()
                .held_unique_pcm_bytes,
            128
        );
        drop(another_view);
        assert_eq!(
            assets
                .resident_budget_status(&account)
                .unwrap()
                .held_unique_pcm_bytes,
            0
        );
        assert_eq!(
            assets
                .resident_budget_status(&account)
                .unwrap()
                .pending_reservation_count,
            0
        );
    }

    #[test]
    fn capacity_refusal_does_not_commit_prepared_current_authority() {
        let assets = ProjectAssets::isolated();
        let account = ResidentAccount::new();
        let mut current_request = 7;
        let epoch = AtomicU64::new(9);
        {
            let _advance =
                super::super::PadRequestAdvance::prepare(&mut current_request, &epoch).unwrap();
            assert!(matches!(
                assets.reserve_resident(&account, 1, &[], usize::MAX),
                Err(ResidentAdmissionError::Capacity(_))
            ));
        }
        assert_eq!(current_request, 7);
        assert_eq!(epoch.load(Ordering::Acquire), 9);
        let status = assets.resident_budget_status(&account).unwrap();
        assert_eq!(status.pending_resident_bytes, 0);
        assert_eq!(status.pending_reservation_count, 0);
    }

    #[test]
    fn small_216_unique_and_shared_owner_sets_reconcile_repeated_final_owners() {
        let assets = ProjectAssets::isolated();
        let account = ResidentAccount::new();
        assets.set_resident_budget_mib(&account, 128).unwrap();
        let unique: Vec<_> = (0..216).map(|id| sample(id as f32)).collect();
        for source in &unique {
            admit(&assets, &account, source);
        }
        let status = assets.resident_budget_status(&account).unwrap();
        assert_eq!(status.unique_backing_count, 216);
        assert_eq!(status.held_unique_pcm_bytes, 216 * 128);
        let shared: Vec<_> = (0..216).map(|_| local_view(&unique[0])).collect();
        for source in &shared {
            admit(&assets, &account, source);
        }
        assert_eq!(
            assets
                .resident_budget_status(&account)
                .unwrap()
                .unique_backing_count,
            216
        );
        drop(unique);
        let status = assets.resident_budget_status(&account).unwrap();
        assert_eq!(status.unique_backing_count, 1);
        assert_eq!(status.held_unique_pcm_bytes, 128);
        for round in 0..2 {
            let replacement: Vec<_> = (0..216)
                .map(|id| sample((round * 216 + id) as f32))
                .collect();
            for source in &replacement {
                admit(&assets, &account, source);
            }
            assert_eq!(
                assets
                    .resident_budget_status(&account)
                    .unwrap()
                    .unique_backing_count,
                217
            );
            drop(replacement);
            assert_eq!(
                assets
                    .resident_budget_status(&account)
                    .unwrap()
                    .unique_backing_count,
                1
            );
        }
        drop(shared);
        let status = assets.resident_budget_status(&account).unwrap();
        assert_eq!(status.held_unique_pcm_bytes, 0);
        assert_eq!(status.unique_backing_count, 0);
        assert_eq!(status.pending_resident_bytes, 0);
    }
}
