//! Fixed native content placement, separate from content/source authority.
//!
//! The caller owns the sole lifetime table and retains its engine ownership `Arc` for every
//! captured key. A placement transaction changes neither that table nor any source, timing,
//! preparation, selection or voice guard. Empty slots also own a cell: all slots and cells form
//! one complete permutation, so moving into an empty slot needs no allocation or free-cell scan.

use super::constants::NUM_SAMPLES;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};

const _: () = assert!(NUM_SAMPLES <= u16::MAX as usize);

/// Native-only comparison marker. It never dereferences a saved pointer.
///
/// A key/permit using this marker must retain the actual ownership `Arc` separately. Persisted
/// numbers and a marker without its retained owner cannot create current native authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OwnerIdentity(usize);

impl OwnerIdentity {
    pub(crate) fn diagnostic(self) -> usize {
        self.0
    }

    pub(crate) fn of<T>(owner: &Arc<T>) -> Self {
        Self::of_ref(owner.as_ref())
    }

    /// Match `of(&Arc<T>)` from an existing borrowed native ownership method.
    ///
    /// The containing engine or captured permit must still retain that same ownership `Arc`.
    pub(crate) fn of_ref<T>(owner: &T) -> Self {
        Self(std::ptr::from_ref(owner) as usize)
    }
}

/// Stable index within one engine's preallocated authority cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ContentCell(u16);

impl ContentCell {
    pub(crate) fn from_index(index: usize) -> Option<Self> {
        (index < NUM_SAMPLES).then_some(Self(index as u16))
    }

    pub(crate) fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// Owner-scoped content incarnation. Placement changes preserve this exact value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ContentKey {
    owner: OwnerIdentity,
    cell: ContentCell,
    lifetime: u64,
}

impl ContentKey {
    /// Construct a comparison key; actual authority still requires the caller's live table.
    pub(crate) fn new(owner: OwnerIdentity, cell: ContentCell, lifetime: u64) -> Option<Self> {
        (lifetime != 0).then_some(Self {
            owner,
            cell,
            lifetime,
        })
    }

    pub(crate) fn cell(self) -> ContentCell {
        self.cell
    }

    pub(crate) fn lifetime(self) -> u64 {
        self.lifetime
    }

    pub(crate) fn belongs_to(self, owner: OwnerIdentity) -> bool {
        self.owner == owner
    }
}

/// Fixed-slot admission snapshot, distinct from an already admitted content key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotSnapshot {
    owner: OwnerIdentity,
    slot: usize,
    cell: ContentCell,
    binding_epoch: u64,
    lifetime: u64,
    layout_revision: u64,
}

impl SlotSnapshot {
    pub(crate) fn owner(self) -> OwnerIdentity {
        self.owner
    }
    /// Compare the same fixed-slot admission without an unrelated global publication counter.
    /// Full equality still includes the exact layout revision for committed transaction evidence.
    pub(crate) fn same_projection(self, other: Self) -> bool {
        self.owner == other.owner
            && self.slot == other.slot
            && self.cell == other.cell
            && self.binding_epoch == other.binding_epoch
            && self.lifetime == other.lifetime
    }

    pub(crate) fn slot(self) -> usize {
        self.slot
    }

    pub(crate) fn cell(self) -> ContentCell {
        self.cell
    }

    pub(crate) fn binding_epoch(self) -> u64 {
        self.binding_epoch
    }

    pub(crate) fn lifetime(self) -> u64 {
        self.lifetime
    }

    pub(crate) fn layout_revision(self) -> u64 {
        self.layout_revision
    }

    pub(crate) fn content_key(self) -> Option<ContentKey> {
        ContentKey::new(self.owner, self.cell, self.lifetime)
    }

    /// Describe a reserved future incarnation without publishing any authority.
    ///
    /// This is for an existing guarded native publisher preparing a new assignment. The key
    /// remains noncurrent until that publisher installs exactly this nonzero lifetime. A new
    /// slot admission must still validate the actual slot binding/epoch, not trust this value.
    pub(crate) fn for_lifetime(self, lifetime: u64) -> Option<Self> {
        (lifetime != 0).then_some(Self { lifetime, ..self })
    }
}

/// Pending publication is neither proof of stale source authority nor retirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlacementObservation<T> {
    Current(T),
    Pending,
    Stale,
    Retired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlacementError {
    InvalidSlot,
    ForeignOwner,
    SameSlot,
    EmptySource,
    Pending,
    Stale,
    Retired,
    SlotEpochExhausted,
    LayoutRevisionExhausted,
    ObservationGenerationExhausted,
    CapacityUnavailable,
    ClaimRejected,
}

impl fmt::Display for PlacementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidSlot => "content placement slot is out of range",
            Self::ForeignOwner => "content placement belongs to another native owner",
            Self::SameSlot => "content placement requires two distinct slots",
            Self::EmptySource => "content placement source is empty",
            Self::Pending => "content placement publication is pending",
            Self::Stale => "content placement snapshot is stale",
            Self::Retired => "content placement lifetime has retired",
            Self::SlotEpochExhausted => "content placement slot epoch exhausted",
            Self::LayoutRevisionExhausted => "content placement revision exhausted",
            Self::ObservationGenerationExhausted => {
                "content placement observation generation exhausted"
            }
            Self::CapacityUnavailable => "content placement capacity is unavailable",
            Self::ClaimRejected => "content placement native claim was rejected",
        })
    }
}

impl std::error::Error for PlacementError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PairPlacementKind {
    Move,
    Swap,
}

/// Pure off-thread preparation. Creating a proposal reserves no live authority.
#[derive(Debug)]
pub(crate) struct PreparedPlacement {
    source: SlotSnapshot,
    target: SlotSnapshot,
    source_next_epoch: u64,
    target_next_epoch: u64,
}

impl PreparedPlacement {
    pub(crate) fn source(&self) -> SlotSnapshot {
        self.source
    }

    pub(crate) fn target(&self) -> SlotSnapshot {
        self.target
    }

    pub(crate) fn kind(&self) -> PairPlacementKind {
        if self.target.lifetime == 0 {
            PairPlacementKind::Move
        } else {
            PairPlacementKind::Swap
        }
    }
}

/// The complete projection after a successful placement publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlacementCommit {
    pub(crate) source_after: SlotSnapshot,
    pub(crate) target_after: SlotSnapshot,
    pub(crate) layout_revision: u64,
}

/// Off-thread scalar preparation for one source/incarnation write. No live authority is reserved.
pub(crate) struct PreparedSlotWrite {
    before: SlotSnapshot,
    after: SlotSnapshot,
}

/// One fixed complete assignment image; every participant is checked before its sole claim.
pub(crate) struct PreparedSlotSetWrite<const N: usize> {
    writes: [PreparedSlotWrite; N],
}

impl<const N: usize> PreparedSlotSetWrite<N> {
    pub(crate) fn writes(&self) -> &[PreparedSlotWrite; N] {
        &self.writes
    }
}

impl PreparedSlotWrite {
    pub(crate) fn before(&self) -> SlotSnapshot {
        self.before
    }
    pub(crate) fn after(&self) -> SlotSnapshot {
        self.after
    }
}

/// Preallocated complete slot/cell permutation. Stable cells own no duplicated lifetime table.
pub(crate) struct ContentPlacement {
    slot_to_cell: [AtomicU16; NUM_SAMPLES],
    cell_to_slot: [AtomicU16; NUM_SAMPLES],
    slot_epochs: [AtomicU64; NUM_SAMPLES],
    // Even is complete; odd is a single callback writer's in-progress publication.
    revision: AtomicU64,
    // Every acquired marker is observable even when a false claim restores the
    // original layout revision. MAX is permanently unavailable to read tokens.
    observation_generation: AtomicU64,
}

impl Default for ContentPlacement {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentPlacement {
    pub(crate) fn prepare_slot_set_write<const N: usize>(
        &self,
        owner: OwnerIdentity,
        lifetimes: &[AtomicU64; NUM_SAMPLES],
        captured: [SlotSnapshot; N],
        next_lifetimes: [u64; N],
    ) -> Result<PreparedSlotSetWrite<N>, PlacementError> {
        if N == 0 || N > NUM_SAMPLES {
            return Err(PlacementError::InvalidSlot);
        }
        let token = self.read_token().ok_or(PlacementError::Pending)?;
        let mut seen = [false; NUM_SAMPLES];
        let mut writes = Vec::with_capacity(N);
        for (snapshot, lifetime) in captured.into_iter().zip(next_lifetimes) {
            if snapshot.slot >= NUM_SAMPLES || seen[snapshot.slot] {
                return Err(PlacementError::InvalidSlot);
            }
            seen[snapshot.slot] = true;
            writes.push(self.prepare_slot_write(owner, lifetimes, snapshot, lifetime)?);
        }
        if self.read_token() != Some(token) {
            return Err(PlacementError::Pending);
        }
        Ok(PreparedSlotSetWrite {
            writes: writes.try_into().map_err(|_| PlacementError::InvalidSlot)?,
        })
    }

    /// No allocation, nested marker or fallible operation follows the true whole-set claim.
    pub(crate) fn commit_slot_set_write<const N: usize>(
        &self,
        owner: OwnerIdentity,
        lifetimes: &[AtomicU64; NUM_SAMPLES],
        prepared: &PreparedSlotSetWrite<N>,
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(),
    ) -> Result<[SlotSnapshot; N], PlacementError> {
        let revision = self.revision.load(Ordering::SeqCst);
        if revision & 1 != 0 {
            return Err(PlacementError::Pending);
        }
        if !prepared
            .writes
            .iter()
            .all(|write| self.snapshot_fields_current(owner, lifetimes, write.before))
        {
            return Err(PlacementError::Stale);
        }
        let next_revision = self.enter_marker(revision)?;
        if !prepared
            .writes
            .iter()
            .all(|write| self.snapshot_fields_current(owner, lifetimes, write.before))
            || !claim()
        {
            self.revision.store(revision, Ordering::SeqCst);
            return Err(PlacementError::ClaimRejected);
        }
        apply();
        for write in &prepared.writes {
            lifetimes[write.before.cell.index()].store(write.after.lifetime, Ordering::SeqCst);
            self.slot_epochs[write.before.slot].store(write.after.binding_epoch, Ordering::SeqCst);
        }
        self.revision.store(next_revision, Ordering::SeqCst);
        Ok(std::array::from_fn(|index| SlotSnapshot {
            layout_revision: next_revision,
            ..prepared.writes[index].after
        }))
    }
    pub(crate) fn pending(&self) -> bool {
        self.revision.load(Ordering::SeqCst) & 1 != 0
    }

    /// Bracket control reads without granting content or source authority. A
    /// marker crossing changes this token even when its claim rolls back with
    /// unchanged layout, slot epochs and lifetimes. Reads are bounded and never retry.
    pub(crate) fn read_token(&self) -> Option<u64> {
        self.read_token_result().ok()
    }

    /// Read only inside the caller's already held mutation marker. This observation
    /// grants no claim; it binds a preflight token to this marker's single increment.
    pub(crate) fn claimed_observation_token(&self) -> Option<u64> {
        let revision = self.revision.load(Ordering::SeqCst);
        let token = self.observation_generation.load(Ordering::SeqCst);
        (revision & 1 != 0 && token != u64::MAX).then_some(token)
    }

    /// Preserve permanent exhaustion when a control owner needs to defer only
    /// actual publication contention. The optional read API retains its contract.
    pub(crate) fn read_token_result(&self) -> Result<u64, PlacementError> {
        let revision = self.revision.load(Ordering::SeqCst);
        if revision & 1 != 0 {
            return Err(PlacementError::Pending);
        }
        let generation = self.observation_generation.load(Ordering::SeqCst);
        if generation == u64::MAX {
            return Err(PlacementError::ObservationGenerationExhausted);
        }
        if self.revision.load(Ordering::SeqCst) != revision
            || self.observation_generation.load(Ordering::SeqCst) != generation
        {
            return Err(PlacementError::Pending);
        }
        Ok(generation)
    }

    /// One nonblocking entry for all source and pair writers. Generation is
    /// loaded again only after this writer owns the odd marker: a competing
    /// rejected claim may have restored the same even revision since capture.
    fn enter_marker(&self, revision: u64) -> Result<u64, PlacementError> {
        let next_revision = revision
            .checked_add(2)
            .ok_or(PlacementError::LayoutRevisionExhausted)?;
        if self.observation_generation.load(Ordering::SeqCst) == u64::MAX {
            return Err(PlacementError::ObservationGenerationExhausted);
        }
        #[cfg(test)]
        tests::before_marker_compare_exchange();
        self.revision
            .compare_exchange(
                revision,
                next_revision - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .map_err(|revision| {
                if revision & 1 != 0 {
                    PlacementError::Pending
                } else {
                    PlacementError::Stale
                }
            })?;
        let Some(next_generation) = self
            .observation_generation
            .load(Ordering::SeqCst)
            .checked_add(1)
        else {
            // Another rejected claim exhausted the counter after the first
            // capacity check. MAX already prevents every stable read token;
            // restore only this unchanged marker without invoking claim/apply.
            self.revision.store(revision, Ordering::SeqCst);
            return Err(PlacementError::ObservationGenerationExhausted);
        };
        self.observation_generation
            .store(next_generation, Ordering::SeqCst);
        Ok(next_revision)
    }

    /// Only the callback may mutate placement. A concurrent control source write cannot change
    /// this permutation, so its unrelated odd marker does not turn a captured scalar epoch to 0.
    pub(crate) fn callback_cell(&self, slot: usize) -> Option<ContentCell> {
        self.slot_to_cell
            .get(slot)
            .and_then(|cell| ContentCell::from_index(usize::from(cell.load(Ordering::SeqCst))))
    }
    pub(crate) fn callback_key_slot(
        &self,
        owner: OwnerIdentity,
        lifetimes: &[AtomicU64; NUM_SAMPLES],
        key: ContentKey,
    ) -> Option<usize> {
        if !self.key_current(owner, lifetimes, key) {
            return None;
        }
        let slot = usize::from(self.cell_to_slot[key.cell.index()].load(Ordering::SeqCst));
        (self.callback_cell(slot) == Some(key.cell) && self.key_current(owner, lifetimes, key))
            .then_some(slot)
    }
    pub(crate) fn snapshot_fields_current(
        &self,
        owner: OwnerIdentity,
        lifetimes: &[AtomicU64; NUM_SAMPLES],
        snapshot: SlotSnapshot,
    ) -> bool {
        snapshot.owner == owner
            && snapshot.slot < NUM_SAMPLES
            && self.callback_cell(snapshot.slot) == Some(snapshot.cell)
            && self.slot_epochs[snapshot.slot].load(Ordering::SeqCst) == snapshot.binding_epoch
            && lifetimes[snapshot.cell.index()].load(Ordering::SeqCst) == snapshot.lifetime
            && self.slot_epochs[snapshot.slot].load(Ordering::SeqCst) == snapshot.binding_epoch
    }
    pub(crate) fn new() -> Self {
        Self {
            slot_to_cell: std::array::from_fn(|index| AtomicU16::new(index as u16)),
            cell_to_slot: std::array::from_fn(|index| AtomicU16::new(index as u16)),
            slot_epochs: std::array::from_fn(|_| AtomicU64::new(1)),
            revision: AtomicU64::new(2),
            observation_generation: AtomicU64::new(1),
        }
    }

    pub(crate) fn prepare_slot_write(
        &self,
        owner: OwnerIdentity,
        lifetimes: &[AtomicU64; NUM_SAMPLES],
        captured: SlotSnapshot,
        lifetime: u64,
    ) -> Result<PreparedSlotWrite, PlacementError> {
        let before = current(self.observe_snapshot(owner, lifetimes, captured))?;
        let binding_epoch = if lifetime != before.lifetime {
            before
                .binding_epoch
                .checked_add(1)
                .ok_or(PlacementError::SlotEpochExhausted)?
        } else {
            before.binding_epoch
        };
        let next_revision = before
            .layout_revision
            .checked_add(2)
            .ok_or(PlacementError::LayoutRevisionExhausted)?;
        Ok(PreparedSlotWrite {
            before,
            after: SlotSnapshot {
                lifetime,
                binding_epoch,
                layout_revision: next_revision,
                ..before
            },
        })
    }

    /// Source and Pair writers share this single nonblocking publication boundary. Preparation,
    /// locks, capacity and overflow checks precede entry. A true claim is followed only by fixed
    /// infallible stores. Interrupted claimed work deliberately leaves the layout Pending.
    pub(crate) fn commit_slot_write(
        &self,
        owner: OwnerIdentity,
        lifetimes: &[AtomicU64; NUM_SAMPLES],
        prepared: &PreparedSlotWrite,
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(ContentCell),
    ) -> Result<SlotSnapshot, PlacementError> {
        let observed = current(self.observe_snapshot(owner, lifetimes, prepared.before))?;
        let next_revision = self.enter_marker(observed.layout_revision)?;
        if !claim() {
            self.revision
                .store(observed.layout_revision, Ordering::SeqCst);
            return Err(PlacementError::ClaimRejected);
        }
        apply(prepared.before.cell);
        lifetimes[prepared.before.cell.index()].store(prepared.after.lifetime, Ordering::SeqCst);
        self.slot_epochs[prepared.before.slot]
            .store(prepared.after.binding_epoch, Ordering::SeqCst);
        self.revision.store(next_revision, Ordering::SeqCst);
        Ok(SlotSnapshot {
            layout_revision: next_revision,
            ..prepared.after
        })
    }

    /// Existing source/effect guards use this independently of a pending layout publication.
    pub(crate) fn key_current(
        &self,
        owner: OwnerIdentity,
        lifetimes: &[AtomicU64; NUM_SAMPLES],
        key: ContentKey,
    ) -> bool {
        key.belongs_to(owner)
            && key.lifetime != 0
            && lifetimes[key.cell.index()].load(Ordering::SeqCst) == key.lifetime
            && lifetimes[key.cell.index()].load(Ordering::SeqCst) == key.lifetime
    }

    pub(crate) fn capture_slot(
        &self,
        owner: OwnerIdentity,
        lifetimes: &[AtomicU64; NUM_SAMPLES],
        slot: usize,
    ) -> PlacementObservation<SlotSnapshot> {
        if slot >= NUM_SAMPLES {
            return PlacementObservation::Stale;
        }
        let revision = self.revision.load(Ordering::SeqCst);
        if revision & 1 != 0 {
            return PlacementObservation::Pending;
        }
        let Some(cell) =
            ContentCell::from_index(usize::from(self.slot_to_cell[slot].load(Ordering::SeqCst)))
        else {
            return PlacementObservation::Stale;
        };
        let lifetime = lifetimes[cell.index()].load(Ordering::SeqCst);
        let binding_epoch = self.slot_epochs[slot].load(Ordering::SeqCst);
        let inverse = usize::from(self.cell_to_slot[cell.index()].load(Ordering::SeqCst));
        let lifetime_after = lifetimes[cell.index()].load(Ordering::SeqCst);
        if self.revision.load(Ordering::SeqCst) != revision || lifetime_after != lifetime {
            return PlacementObservation::Pending;
        }
        if inverse != slot || binding_epoch == 0 {
            return PlacementObservation::Stale;
        }
        PlacementObservation::Current(SlotSnapshot {
            owner,
            slot,
            cell,
            binding_epoch,
            lifetime,
            layout_revision: revision,
        })
    }

    pub(crate) fn observe_key(
        &self,
        owner: OwnerIdentity,
        lifetimes: &[AtomicU64; NUM_SAMPLES],
        key: ContentKey,
    ) -> PlacementObservation<SlotSnapshot> {
        if !key.belongs_to(owner) {
            return PlacementObservation::Stale;
        }
        let revision = self.revision.load(Ordering::SeqCst);
        if revision & 1 != 0 {
            return PlacementObservation::Pending;
        }
        let lifetime = lifetimes[key.cell.index()].load(Ordering::SeqCst);
        let slot = usize::from(self.cell_to_slot[key.cell.index()].load(Ordering::SeqCst));
        if slot >= NUM_SAMPLES {
            return if self.revision.load(Ordering::SeqCst) != revision {
                PlacementObservation::Pending
            } else {
                PlacementObservation::Stale
            };
        }
        let snapshot = match self.capture_slot(owner, lifetimes, slot) {
            PlacementObservation::Current(snapshot) => snapshot,
            PlacementObservation::Pending => return PlacementObservation::Pending,
            PlacementObservation::Stale | PlacementObservation::Retired => {
                return PlacementObservation::Stale;
            }
        };
        if snapshot.layout_revision != revision
            || self.revision.load(Ordering::SeqCst) != revision
            || lifetimes[key.cell.index()].load(Ordering::SeqCst) != lifetime
        {
            return PlacementObservation::Pending;
        }
        if lifetime != key.lifetime {
            return PlacementObservation::Retired;
        }
        if snapshot.cell != key.cell || snapshot.lifetime != key.lifetime {
            return PlacementObservation::Stale;
        }
        PlacementObservation::Current(snapshot)
    }

    pub(crate) fn observe_snapshot(
        &self,
        owner: OwnerIdentity,
        lifetimes: &[AtomicU64; NUM_SAMPLES],
        snapshot: SlotSnapshot,
    ) -> PlacementObservation<SlotSnapshot> {
        if snapshot.owner != owner {
            return PlacementObservation::Stale;
        }
        let observed = self.capture_slot(owner, lifetimes, snapshot.slot);
        let PlacementObservation::Current(current) = observed else {
            return observed;
        };
        if snapshot.lifetime != 0
            && lifetimes[snapshot.cell.index()].load(Ordering::SeqCst) != snapshot.lifetime
        {
            return PlacementObservation::Retired;
        }
        if current.cell != snapshot.cell
            || current.binding_epoch != snapshot.binding_epoch
            || current.lifetime != snapshot.lifetime
        {
            return PlacementObservation::Stale;
        }
        PlacementObservation::Current(current)
    }

    pub(crate) fn prepare_pair(
        &self,
        owner: OwnerIdentity,
        lifetimes: &[AtomicU64; NUM_SAMPLES],
        source: SlotSnapshot,
        target: SlotSnapshot,
    ) -> Result<PreparedPlacement, PlacementError> {
        if source.owner != owner || target.owner != owner {
            return Err(PlacementError::ForeignOwner);
        }
        if source.slot >= NUM_SAMPLES || target.slot >= NUM_SAMPLES {
            return Err(PlacementError::InvalidSlot);
        }
        if source.slot == target.slot {
            return Err(PlacementError::SameSlot);
        }
        if source.lifetime == 0 {
            return Err(PlacementError::EmptySource);
        }
        let source = current(self.observe_snapshot(owner, lifetimes, source))?;
        let target = current(self.observe_snapshot(owner, lifetimes, target))?;
        if source.layout_revision != target.layout_revision {
            return Err(PlacementError::Pending);
        }
        let source_next_epoch = source
            .binding_epoch
            .checked_add(1)
            .ok_or(PlacementError::SlotEpochExhausted)?;
        let target_next_epoch = target
            .binding_epoch
            .checked_add(1)
            .ok_or(PlacementError::SlotEpochExhausted)?;
        source
            .layout_revision
            .checked_add(2)
            .ok_or(PlacementError::LayoutRevisionExhausted)?;
        Ok(PreparedPlacement {
            source,
            target,
            source_next_epoch,
            target_next_epoch,
        })
    }

    /// Claim and publish one prepared pair in bounded callback work.
    ///
    /// `claim` must validate the caller's complete native/source/action guards and return true
    /// only after its irreversible claim. `apply` then performs the complete infallible native
    /// content-image/routing mutation without changing either captured lifetime. It must not
    /// allocate, block, panic or destroy final owners. No guard or fallible step follows a true
    /// claim. The caller ACKs its complete product transaction only after this method succeeds.
    ///
    /// A false claim restores only an unchanged publication marker. A panic or interrupted true
    /// claim deliberately leaves Pending; this primitive never guesses rollback after mutation.
    pub(crate) fn commit_with(
        &self,
        owner: OwnerIdentity,
        lifetimes: &[AtomicU64; NUM_SAMPLES],
        prepared: &PreparedPlacement,
        capacity_available: bool,
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(),
    ) -> Result<PlacementCommit, PlacementError> {
        if prepared.source.owner != owner || prepared.target.owner != owner {
            return Err(PlacementError::ForeignOwner);
        }
        if !capacity_available {
            return Err(PlacementError::CapacityUnavailable);
        }
        let source = current(self.observe_snapshot(owner, lifetimes, prepared.source))?;
        let target = current(self.observe_snapshot(owner, lifetimes, prepared.target))?;
        if source.layout_revision != target.layout_revision {
            return Err(PlacementError::Pending);
        }
        // Unrelated same-content writes and already-admitted Hold release may advance
        // the exclusion marker. Original cells/lifetimes/physical epochs were validated
        // above; they grant no new authority. Claim that latest even marker once.
        let next_revision = self.enter_marker(source.layout_revision)?;
        if !claim() {
            self.revision
                .store(source.layout_revision, Ordering::SeqCst);
            return Err(PlacementError::ClaimRejected);
        }

        apply();
        self.slot_to_cell[source.slot].store(target.cell.0, Ordering::SeqCst);
        self.slot_to_cell[target.slot].store(source.cell.0, Ordering::SeqCst);
        self.cell_to_slot[source.cell.index()].store(target.slot as u16, Ordering::SeqCst);
        self.cell_to_slot[target.cell.index()].store(source.slot as u16, Ordering::SeqCst);
        self.slot_epochs[source.slot].store(prepared.source_next_epoch, Ordering::SeqCst);
        self.slot_epochs[target.slot].store(prepared.target_next_epoch, Ordering::SeqCst);
        self.revision.store(next_revision, Ordering::SeqCst);

        Ok(PlacementCommit {
            source_after: SlotSnapshot {
                owner,
                slot: source.slot,
                cell: target.cell,
                binding_epoch: prepared.source_next_epoch,
                lifetime: target.lifetime,
                layout_revision: next_revision,
            },
            target_after: SlotSnapshot {
                owner,
                slot: target.slot,
                cell: source.cell,
                binding_epoch: prepared.target_next_epoch,
                lifetime: source.lifetime,
                layout_revision: next_revision,
            },
            layout_revision: next_revision,
        })
    }
}

fn current<T>(observed: PlacementObservation<T>) -> Result<T, PlacementError> {
    match observed {
        PlacementObservation::Current(value) => Ok(value),
        PlacementObservation::Pending => Err(PlacementError::Pending),
        PlacementObservation::Stale => Err(PlacementError::Stale),
        PlacementObservation::Retired => Err(PlacementError::Retired),
    }
}

#[cfg(test)]
#[path = "content_placement_tests.rs"]
pub(crate) mod tests;
