//! Native source/timing admission for MIDI snapshots; no PCM/evidence owner enters RT.

use super::AudioEngine;
use super::constant_timing::{
    self, AcceptedTimingProjection, CurrentConstantTimingRecord, CurrentTimingAcknowledgements,
};
use super::constants::NUM_SAMPLES;
use super::content_placement::{
    ContentCell, ContentKey, ContentPlacement, OwnerIdentity, PlacementError, PlacementObservation,
    SlotSnapshot,
};
use super::prepared_source::next_epoch;
use crate::messages::SampleBuffer;
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering},
};

#[path = "key_lock_status.rs"]
mod key_lock_status;
pub(crate) use key_lock_status::KeyLockStatus;
use key_lock_status::KeyLockStatusSlot;
#[path = "input_runtime_binding/pair_control.rs"]
mod pair_control;
pub(crate) use pair_control::PairControlProjection;
mod source_request;

/// One prepared runtime store retains the original slot/cell and old counter. It grants
/// no new source authority and may only be applied inside the complete runtime claim.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RuntimeRevisionWrite {
    placement: SlotSnapshot,
    previous: u64,
    next: u64,
}

impl RuntimeRevisionWrite {
    pub(crate) fn placement(self) -> SlotSnapshot {
        self.placement
    }
    pub(crate) fn revision(self) -> u64 {
        self.next
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaunchWriteKind {
    Admit,
    Cancel,
    Stop,
}

/// Control-only error context preserves existing diagnostics without parsing them for retries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ControlLaunchSetError {
    InvalidParticipants,
    EmptyParticipants,
    SlotObservation(PlacementError),
    Publication(PlacementError),
}

impl ControlLaunchSetError {
    fn pending(self) -> bool {
        matches!(
            self,
            Self::SlotObservation(PlacementError::Pending)
                | Self::Publication(PlacementError::Pending)
        )
    }
}

impl std::fmt::Display for ControlLaunchSetError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidParticipants => formatter.write_str("invalid launch participant set"),
            Self::EmptyParticipants => formatter.write_str("empty launch participant set"),
            Self::SlotObservation(PlacementError::Pending) => {
                formatter.write_str("content placement is pending")
            }
            Self::SlotObservation(error) | Self::Publication(error) => {
                std::fmt::Display::fmt(error, formatter)
            }
        }
    }
}

/// Original participant fields for one complete launch-control publication.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PreparedLaunchWrite {
    pub(crate) placement: SlotSnapshot,
    pub(crate) previous_revision: u64,
    pub(crate) previous_admitted: bool,
    pub(crate) next_revision: u64,
    next_admitted: bool,
    original_hold: u64,
    original_resident: u64,
    original_transport: u64,
    next_transport: u64,
    kind: LaunchWriteKind,
}

/// Actual source control publication. Retaining its Native owner is mandatory even when a
/// foreign/stale message is rejected; the complete value retires through the existing worker.
#[derive(Clone)]
pub(crate) struct SourcePublication {
    pub(crate) placement: SlotSnapshot,
    pub(crate) source_generation: u64,
    pub(crate) authority_revision: u64,
    pub(crate) ownership: Arc<InputRuntimeOwnership>,
    pub(crate) retired_feedback: Option<(ContentKey, u64)>,
}

impl std::fmt::Debug for SourcePublication {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SourcePublication")
            .field("placement", &self.placement)
            .field("source_generation", &self.source_generation)
            .finish_non_exhaustive()
    }
}

impl SourcePublication {
    pub(crate) fn binding(&self, sample: &SampleBuffer, rate: u32) -> InputPadBinding {
        InputPadBinding {
            placement: Some(self.placement),
            resident: sample.resident_binding(),
            source_address: sample.source_address(),
            sample_count: sample.source_sample_count(),
            channels: sample.channels,
            sample_rate_hz: rate,
            authority_revision: self.authority_revision,
            runtime_revision: 0,
            accepted: None,
        }
    }
    pub(crate) fn key(&self) -> Option<ContentKey> {
        self.placement.content_key()
    }
    pub(crate) fn current(&self, sample: &SampleBuffer, rate: u32) -> bool {
        let Some(key) = self.key() else {
            return false;
        };
        self.ownership.content_key_current(key)
            && self
                .ownership
                .authority_for_cell(key.cell())
                .load(Ordering::Acquire)
                == self.authority_revision
            && self
                .ownership
                .source_generation_for_cell(key.cell(), sample, rate)
                == Some(self.source_generation)
            && self
                .ownership
                .binding_window_current(self.placement.slot(), self.binding(sample, rate))
            && self.ownership.content_key_current(key)
    }
}

/// One control-owned current PCM publication. Zero generation fences all new starts.
/// Readers perform one bounded check and recheck; they never spin or dereference its address.
struct CurrentSourceFence {
    generation: AtomicU64,
    address: AtomicUsize,
    count: AtomicUsize,
    channels: AtomicUsize,
    rate: AtomicU32,
    resident_address: AtomicUsize,
    resident_start: AtomicUsize,
    resident_end: AtomicUsize,
    window_revision: AtomicU64,
    resident_context: AtomicU8,
}

impl Default for CurrentSourceFence {
    fn default() -> Self {
        Self {
            generation: AtomicU64::new(0),
            address: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
            channels: AtomicUsize::new(0),
            rate: AtomicU32::new(0),
            resident_address: AtomicUsize::new(0),
            resident_start: AtomicUsize::new(0),
            resident_end: AtomicUsize::new(0),
            window_revision: AtomicU64::new(0),
            resident_context: AtomicU8::new(0),
        }
    }
}

/// Control publishes revocation before queued callback work. Preparation requests
/// deliberately do not change these source/authority epochs.
pub(crate) struct InputRuntimeOwnership {
    // One original control revocation fence, shared with the callback's rate capture.
    pub(crate) global_timing_revision: Arc<AtomicU64>,
    pub(crate) global_timing_publishing: AtomicBool,
    pub(crate) global_rate_status: super::global_rate_status::GlobalRateStatusSlot,
    authority: [AtomicU64; NUM_SAMPLES],
    runtime: [AtomicU64; NUM_SAMPLES],
    timing_intent: [AtomicU8; NUM_SAMPLES],
    source_tracking: bool,
    sources: [CurrentSourceFence; NUM_SAMPLES],
    cold_adoptions: [AtomicU64; NUM_SAMPLES],
    // Actual callback PCM custody, including Copy and legacy source adoption.
    callback_source_lifetimes: [AtomicU64; NUM_SAMPLES],
    resident_controls: [AtomicU64; NUM_SAMPLES],
    launch_revisions: [AtomicU64; NUM_SAMPLES],
    admitted_launches: [AtomicBool; NUM_SAMPLES],
    migration_holds: [AtomicU64; NUM_SAMPLES],
    copy_reservations: [AtomicU64; NUM_SAMPLES],
    pair_reservations: [AtomicU64; NUM_SAMPLES],
    transport_intents: [AtomicU64; NUM_SAMPLES],
    parameter_intents: [AtomicU64; NUM_SAMPLES],
    // Source/request intent belongs to stable content cells. Window preparation and
    // physical placement counters do not change this incarnation.
    source_requests: [AtomicU64; NUM_SAMPLES],
    // Control custody follows Native placement only after its complete own mirror claim.
    control_cell_to_slot: [AtomicUsize; NUM_SAMPLES],
    content_lifetimes: [AtomicU64; NUM_SAMPLES],
    copy_parameter_epochs: [AtomicU64; NUM_SAMPLES],
    key_lock: [KeyLockStatusSlot; NUM_SAMPLES],
    endpoint_selection_revision: AtomicU64,
    endpoint_selection_cell: AtomicUsize,
    endpoint_selection_lifetime: AtomicU64,
    placement: ContentPlacement,
}

impl Default for InputRuntimeOwnership {
    fn default() -> Self {
        Self {
            global_timing_revision: Arc::new(AtomicU64::new(1)),
            global_timing_publishing: AtomicBool::new(false),
            global_rate_status: super::global_rate_status::GlobalRateStatusSlot::default(),
            authority: std::array::from_fn(|_| AtomicU64::new(1)),
            runtime: std::array::from_fn(|_| AtomicU64::new(0)),
            timing_intent: std::array::from_fn(|_| AtomicU8::new(0)),
            source_tracking: false,
            sources: std::array::from_fn(|_| CurrentSourceFence::default()),
            cold_adoptions: std::array::from_fn(|_| AtomicU64::new(0)),
            callback_source_lifetimes: std::array::from_fn(|_| AtomicU64::new(0)),
            resident_controls: std::array::from_fn(|_| AtomicU64::new(0)),
            launch_revisions: std::array::from_fn(|_| AtomicU64::new(0)),
            admitted_launches: std::array::from_fn(|_| AtomicBool::new(false)),
            migration_holds: std::array::from_fn(|_| AtomicU64::new(0)),
            copy_reservations: std::array::from_fn(|_| AtomicU64::new(0)),
            pair_reservations: std::array::from_fn(|_| AtomicU64::new(0)),
            transport_intents: std::array::from_fn(|_| AtomicU64::new(0)),
            parameter_intents: std::array::from_fn(|_| AtomicU64::new(0)),
            source_requests: std::array::from_fn(|_| AtomicU64::new(1)),
            control_cell_to_slot: std::array::from_fn(|slot| AtomicUsize::new(slot)),
            content_lifetimes: std::array::from_fn(|_| AtomicU64::new(0)),
            copy_parameter_epochs: std::array::from_fn(|_| AtomicU64::new(0)),
            key_lock: std::array::from_fn(|_| KeyLockStatusSlot::default()),
            endpoint_selection_revision: AtomicU64::new(0),
            endpoint_selection_cell: AtomicUsize::new(NUM_SAMPLES),
            endpoint_selection_lifetime: AtomicU64::new(0),
            placement: ContentPlacement::new(),
        }
    }
}

impl InputRuntimeOwnership {
    /// Existing serialized selection writers publish preparation interest only.
    /// Zero marks an in-progress projection; the callback never waits for it.
    pub(crate) fn publish_endpoint_selection(&self, revision: u64, key: Option<ContentKey>) {
        self.endpoint_selection_revision.store(0, Ordering::SeqCst);
        self.endpoint_selection_cell.store(
            key.map_or(NUM_SAMPLES, |key| key.cell().index()),
            Ordering::SeqCst,
        );
        self.endpoint_selection_lifetime
            .store(key.map_or(0, ContentKey::lifetime), Ordering::SeqCst);
        self.endpoint_selection_revision
            .store(revision, Ordering::SeqCst);
    }

    /// Owner identity is implicit in this live table. Scalar equality grants no permission.
    pub(crate) fn endpoint_selection(&self) -> Option<(u64, Option<ContentKey>)> {
        let revision = self.endpoint_selection_revision.load(Ordering::SeqCst);
        if revision == 0 {
            return None;
        }
        let cell = self.endpoint_selection_cell.load(Ordering::SeqCst);
        let lifetime = self.endpoint_selection_lifetime.load(Ordering::SeqCst);
        let key = ContentCell::from_index(cell)
            .and_then(|cell| ContentKey::new(self.owner_identity(), cell, lifetime));
        (self.endpoint_selection_revision.load(Ordering::SeqCst) == revision)
            .then_some((revision, key))
    }

    pub(crate) fn callback_content_key(&self, id: usize) -> Option<ContentKey> {
        let cell = self.callback_cell(id)?;
        ContentKey::new(
            self.owner_identity(),
            cell,
            self.content_lifetimes[cell.index()].load(Ordering::Acquire),
        )
    }
    /// Callback feedback follows its actual placement even inside its own marker.
    /// A control-armed successor has no adopted PCM and cannot inherit old telemetry.
    pub(crate) fn callback_feedback_key(&self, id: usize) -> Option<ContentKey> {
        let key = self.callback_content_key(id)?;
        if !self.source_tracking {
            return Some(key);
        }
        (self.callback_source_lifetimes[key.cell().index()].load(Ordering::Acquire)
            == key.lifetime()
            && self.content_key_current(key))
        .then_some(key)
    }

    /// Called only by successful callback source-image adoption, never control preparation.
    pub(crate) fn acknowledge_callback_source(&self, key: ContentKey) -> bool {
        if !self.content_key_current(key) {
            return false;
        }
        self.callback_source_lifetimes[key.cell().index()].store(key.lifetime(), Ordering::Release);
        self.content_key_current(key)
    }
    /// Source ownership survives a later timing-intent edit. This does not admit any new
    /// timing action: the captured slot incarnation and complete source/window remain exact.
    pub(crate) fn source_publication_identity_binding_current(
        &self,
        publication: &SourcePublication,
        binding: InputPadBinding,
    ) -> bool {
        std::ptr::eq(self, publication.ownership.as_ref())
            && binding.placement == Some(publication.placement)
            && self.slot_snapshot_fields_current(publication.placement)
            && self.binding_window_current(publication.placement.slot(), binding)
            && self.binding_source_generation(publication.placement.slot(), binding)
                == Some(publication.source_generation)
            && self.slot_snapshot_fields_current(publication.placement)
    }

    pub(crate) fn source_publication_binding_current(
        &self,
        publication: &SourcePublication,
        binding: InputPadBinding,
    ) -> bool {
        self.source_publication_identity_binding_current(publication, binding)
            && self.authority_current(publication.placement.slot(), binding)
    }

    pub(crate) fn source_publication_identity_current(
        &self,
        publication: &SourcePublication,
        sample: &SampleBuffer,
        rate: u32,
    ) -> bool {
        self.source_publication_identity_binding_current(
            publication,
            publication.binding(sample, rate),
        )
    }
    pub(crate) fn callback_key_slot(&self, key: ContentKey) -> Option<usize> {
        self.placement
            .callback_key_slot(self.owner_identity(), &self.content_lifetimes, key)
    }
    pub(crate) fn callback_key_lock_request_current(&self, id: usize, request: u64) -> bool {
        self.callback_cell(id).is_some_and(|cell| {
            self.key_lock[cell.index()].request.load(Ordering::Acquire) == request
        })
    }
    pub(crate) fn finish_resident_control_for_binding(
        &self,
        id: usize,
        binding: InputPadBinding,
        intent: u64,
    ) {
        if let Some(cell) = self.binding_cell(id, binding) {
            let _ = self.resident_controls[cell.index()].compare_exchange(
                intent,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
    /// External completion owns the publication marker. Callback tails use the direct
    /// helper above only while their enclosing source/control transaction owns it.
    pub(crate) fn try_finish_resident_control_for_binding(
        &self,
        id: usize,
        binding: InputPadBinding,
        intent: u64,
    ) -> bool {
        let observed = match binding.placement {
            Some(captured) => match captured.content_key() {
                Some(key) => self.observe_control_key(key),
                None if self.slot_snapshot_current(captured) => {
                    PlacementObservation::Current(captured)
                }
                None if self.placement_pending() => PlacementObservation::Pending,
                None => PlacementObservation::Stale,
            },
            None => match self.slot_snapshot(id) {
                Some(snapshot) => PlacementObservation::Current(snapshot),
                None if self.placement_pending() => PlacementObservation::Pending,
                None => PlacementObservation::Stale,
            },
        };
        let current = match observed {
            PlacementObservation::Current(current) => current,
            PlacementObservation::Pending => return false,
            PlacementObservation::Stale | PlacementObservation::Retired => return true,
        };
        let prepared = match self.prepare_slot_write_fixed(current, current.lifetime()) {
            Ok(prepared) => prepared,
            Err(
                super::content_placement::PlacementError::Stale
                | super::content_placement::PlacementError::Retired,
            ) => return false,
            Err(_) => return false,
        };
        self.commit_slot_write_fixed(
            &prepared,
            || true,
            |cell| {
                let _ = self.resident_control_for_cell(cell).compare_exchange(
                    intent,
                    0,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
            },
        )
        .is_ok()
    }
    pub(crate) fn callback_source_generation(
        &self,
        id: usize,
        sample: &SampleBuffer,
        rate: u32,
    ) -> Option<u64> {
        if !self.source_tracking {
            Some(1)
        } else {
            self.source_generation_for_cell(self.callback_cell(id)?, sample, rate)
        }
    }
    pub(crate) fn publish_callback_key_lock_status(&self, id: usize, status: KeyLockStatus) {
        let Some(cell) = self.callback_cell(id) else {
            return;
        };
        let source = &self.sources[cell.index()];
        if (status.request_id == 0
            || self.key_lock[cell.index()].request.load(Ordering::Acquire) == status.request_id)
            && (!self.source_tracking
                || (source.generation.load(Ordering::SeqCst) == status.source_generation
                    && source.address.load(Ordering::SeqCst) == status.source_address
                    && source.window_revision.load(Ordering::SeqCst) == status.window_revision))
        {
            self.key_lock[cell.index()].publish(status);
        }
    }
    pub(crate) fn slot_snapshot_fields_current(&self, snapshot: SlotSnapshot) -> bool {
        self.placement.snapshot_fields_current(
            self.owner_identity(),
            &self.content_lifetimes,
            snapshot,
        )
    }
    pub(crate) fn source_cell_empty(&self, cell: ContentCell) -> bool {
        self.sources[cell.index()].generation.load(Ordering::SeqCst) == 0
    }
    pub(crate) fn apply_source_for_cell(
        &self,
        cell: ContentCell,
        sample: &SampleBuffer,
        rate: u32,
        generation: u64,
        authority: u64,
        intent: TimingIntent,
    ) {
        self.authority_for_cell(cell)
            .store(authority, Ordering::Release);
        self.publish_source_for_cell(cell, sample, rate, generation);
        self.set_timing_intent_for_cell(cell, intent);
    }
    pub(crate) fn publish_copy_parameter_epoch_for_cell(&self, cell: ContentCell, epoch: u64) {
        self.copy_parameter_epochs[cell.index()].store(epoch, Ordering::Release);
    }
    pub(crate) fn placement_pending(&self) -> bool {
        self.placement.pending()
    }
    pub(crate) fn placement_read_token(&self) -> Option<u64> {
        self.placement.read_token()
    }
    pub(crate) fn callback_cell(&self, id: usize) -> Option<ContentCell> {
        self.placement.callback_cell(id)
    }
    pub(crate) fn callback_copy_parameter_epoch(&self, id: usize) -> Option<u64> {
        self.callback_cell(id)
            .map(|cell| self.copy_parameter_epochs[cell.index()].load(Ordering::Acquire))
    }
    pub(crate) fn prepare_slot_write_fixed(
        &self,
        captured: SlotSnapshot,
        lifetime: u64,
    ) -> Result<super::content_placement::PreparedSlotWrite, super::content_placement::PlacementError>
    {
        self.placement.prepare_slot_write(
            self.owner_identity(),
            &self.content_lifetimes,
            captured,
            lifetime,
        )
    }
    pub(crate) fn commit_slot_write_fixed(
        &self,
        prepared: &super::content_placement::PreparedSlotWrite,
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(ContentCell),
    ) -> Result<SlotSnapshot, super::content_placement::PlacementError> {
        self.placement.commit_slot_write(
            self.owner_identity(),
            &self.content_lifetimes,
            prepared,
            claim,
            |cell| {
                // A source birth owns a fresh launch fence even when its reused cell's
                // predecessor reached the terminal revision. Moves preserve the lifetime.
                if prepared.before().lifetime() != prepared.after().lifetime() {
                    self.launch_revisions[cell.index()].store(0, Ordering::Release);
                    self.admitted_launches[cell.index()].store(false, Ordering::Release);
                    self.resident_controls[cell.index()].store(0, Ordering::Release);
                    self.transport_intents[cell.index()].store(0, Ordering::Release);
                    self.parameter_intents[cell.index()].store(0, Ordering::Release);
                }
                apply(cell);
            },
        )
    }
    pub(crate) fn prepare_slot_write(
        &self,
        captured: SlotSnapshot,
        lifetime: u64,
    ) -> Result<super::content_placement::PreparedSlotWrite, String> {
        self.placement
            .prepare_slot_write(
                self.owner_identity(),
                &self.content_lifetimes,
                captured,
                lifetime,
            )
            .map_err(|error| error.to_string())
    }

    pub(crate) fn commit_slot_write(
        &self,
        prepared: &super::content_placement::PreparedSlotWrite,
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(ContentCell),
    ) -> Result<SlotSnapshot, String> {
        self.commit_slot_write_fixed(prepared, claim, apply)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn publish_source_write(
        self: &Arc<Self>,
        prepared: &super::content_placement::PreparedSlotWrite,
        sample: &SampleBuffer,
        rate: u32,
        generation: u64,
        authority: u64,
        intent: Option<TimingIntent>,
        cold: bool,
        claim: impl FnOnce() -> bool,
    ) -> Result<SourcePublication, String> {
        if generation == 0
            || prepared.after().lifetime() == 0
            || (cold && generation > u64::MAX / 4)
        {
            return Err("source publication identity is empty or exhausted".into());
        }
        let placement = self.commit_slot_write(prepared, claim, |cell| {
            self.authority_for_cell(cell)
                .store(authority, Ordering::Release);
            if let Some(intent) = intent {
                self.set_timing_intent_for_cell(cell, intent);
            }
            self.publish_source_for_cell(cell, sample, rate, generation);
            if cold {
                self.cold_adoptions[cell.index()].store(generation * 4, Ordering::Release);
            }
        })?;
        Ok(SourcePublication {
            placement,
            source_generation: generation,
            authority_revision: authority,
            ownership: self.clone(),
            retired_feedback: None,
        })
    }

    pub(crate) fn source_publication_current(
        &self,
        publication: &SourcePublication,
        sample: &SampleBuffer,
        rate: u32,
    ) -> bool {
        publication
            .placement
            .content_key()
            .is_some_and(|key| key.belongs_to(self.owner_identity()))
            && publication.current(sample, rate)
    }

    pub(crate) fn observe_source_publication(
        &self,
        publication: &SourcePublication,
    ) -> PlacementObservation<SlotSnapshot> {
        if let Some(key) = publication.key() {
            self.observe_content_key(key)
        } else {
            self.placement.observe_snapshot(
                self.owner_identity(),
                &self.content_lifetimes,
                publication.placement,
            )
        }
    }

    pub(crate) fn retire_source_write(
        self: &Arc<Self>,
        prepared: &super::content_placement::PreparedSlotWrite,
        authority: u64,
        claim: impl FnOnce() -> bool,
    ) -> Result<SourcePublication, String> {
        if prepared.after().lifetime() != 0 {
            return Err("source retirement must publish an empty incarnation".into());
        }
        let retired_feedback = prepared.before().content_key().map(|key| {
            (
                key,
                self.transport_intent_for_cell(key.cell())
                    .load(Ordering::Acquire),
            )
        });
        let placement = self.commit_slot_write(prepared, claim, |cell| {
            self.authority_for_cell(cell)
                .store(authority, Ordering::Release);
            self.sources[cell.index()]
                .generation
                .store(0, Ordering::SeqCst);
            self.resident_controls[cell.index()].store(0, Ordering::Release);
            self.key_lock[cell.index()].clear();
            self.admitted_launches[cell.index()].store(false, Ordering::Release);
        })?;
        Ok(SourcePublication {
            placement,
            source_generation: 0,
            authority_revision: authority,
            ownership: self.clone(),
            retired_feedback,
        })
    }

    pub(crate) fn source_retirement_fields_current(&self, publication: &SourcePublication) -> bool {
        publication.placement.lifetime() == 0
            && self.slot_snapshot_fields_current(publication.placement)
            && std::ptr::eq(self, publication.ownership.as_ref())
            && self.sources[publication.placement.cell().index()]
                .generation
                .load(Ordering::SeqCst)
                == 0
    }

    pub(crate) fn rollback_source_write(
        &self,
        publication: &SourcePublication,
        sample: &SampleBuffer,
        rate: u32,
        predecessor: SlotSnapshot,
        previous: Option<(&SampleBuffer, u32, u64)>,
        intent: Option<TimingIntent>,
    ) -> Result<bool, String> {
        if !self.source_publication_identity_current(publication, sample, rate) {
            return Ok(false);
        }
        let current = match self.observe_content_key(publication.key().ok_or("source key missing")?)
        {
            PlacementObservation::Current(current) => current,
            PlacementObservation::Pending => return Err("content placement is pending".into()),
            _ => return Ok(false),
        };
        if current.cell() != predecessor.cell() {
            return Ok(false);
        }
        let prepared = self.prepare_slot_write(current, predecessor.lifetime())?;
        self.commit_slot_write(
            &prepared,
            || self.source_publication_identity_current(publication, sample, rate),
            |cell| {
                if let Some(intent) = intent {
                    self.set_timing_intent_for_cell(cell, intent);
                }
                if let Some((sample, rate, generation)) = previous {
                    self.publish_source_for_cell(cell, sample, rate, generation);
                } else {
                    self.sources[cell.index()]
                        .generation
                        .store(0, Ordering::SeqCst);
                }
            },
        )?;
        Ok(true)
    }

    /// Cancel only this captured incarnation under the same exclusion used by source/Pair writes.
    pub(crate) fn cancel_cold_publication_fixed(
        &self,
        publication: &SourcePublication,
    ) -> Result<bool, super::content_placement::PlacementError> {
        let snapshot = match self.observe_source_publication(publication) {
            PlacementObservation::Current(snapshot) => snapshot,
            PlacementObservation::Pending => {
                return Err(super::content_placement::PlacementError::Pending);
            }
            PlacementObservation::Stale | PlacementObservation::Retired => return Ok(false),
        };
        let write = self.prepare_slot_write_fixed(snapshot, snapshot.lifetime())?;
        let cancelled = std::cell::Cell::new(false);
        self.commit_slot_write_fixed(
            &write,
            || {
                publication
                    .key()
                    .is_some_and(|key| self.content_key_current(key))
                    && self.slot_snapshot_fields_current(snapshot)
                    && self.sources[snapshot.cell().index()]
                        .generation
                        .load(Ordering::SeqCst)
                        == publication.source_generation
            },
            |_| cancelled.set(self.finish_cold_publication(publication, false, false)),
        )?;
        Ok(cancelled.get())
    }

    /// Migration cancellation reserves its private permission and settles the exact
    /// native cold row together. Pending retains the caller's publication for retry.
    pub(crate) fn cancel_cold_with_adoption_fixed(
        &self,
        publication: &SourcePublication,
        adoption: &AtomicU8,
    ) -> Result<bool, super::content_placement::PlacementError> {
        use super::content_placement::PlacementError;
        if !std::ptr::eq(self, publication.ownership.as_ref()) {
            return Err(PlacementError::ForeignOwner);
        }
        if publication.source_generation == 0 || publication.source_generation > u64::MAX / 4 {
            return Err(PlacementError::ClaimRejected);
        }
        let settle_retired = || {
            adoption.load(Ordering::Acquire) == 3
                || adoption
                    .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
        };
        let snapshot = match self.observe_source_publication(publication) {
            PlacementObservation::Current(snapshot) => snapshot,
            PlacementObservation::Pending => return Err(PlacementError::Pending),
            PlacementObservation::Stale | PlacementObservation::Retired => {
                return Ok(settle_retired());
            }
        };
        let cell = snapshot.cell().index();
        if self.sources[cell].generation.load(Ordering::SeqCst) != publication.source_generation {
            return Ok(settle_retired());
        }
        let base = publication.source_generation * 4;
        let native = self.cold_adoptions[cell].load(Ordering::Acquire);
        if !matches!(adoption.load(Ordering::Acquire), 0 | 3)
            || (native != base && native != base + 3)
        {
            return Ok(false);
        }
        let write = self.prepare_slot_write_fixed(snapshot, snapshot.lifetime())?;
        let result = self.commit_slot_write_fixed(&write, || {
            self.slot_snapshot_fields_current(snapshot)
                && publication.key().is_some_and(|key| self.content_key_current(key))
                && self.sources[cell].generation.load(Ordering::SeqCst) == publication.source_generation
                && self.cold_adoptions[cell].load(Ordering::Acquire) == native
                // Every native guard precedes the irreversible private cancellation.
                && (adoption.load(Ordering::Acquire) == 3
                    || adoption.compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire).is_ok())
        }, |cell| {
            self.cold_adoptions[cell.index()].store(base + 3, Ordering::Release);
        });
        match result {
            Ok(_) => Ok(true),
            Err(PlacementError::ClaimRejected)
                if matches!(adoption.load(Ordering::Acquire), 1 | 2) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn claim_cold_publication(&self, publication: &SourcePublication) -> bool {
        if publication.source_generation == 0 || publication.source_generation > u64::MAX / 4 {
            return false;
        }
        let Some(key) = publication
            .key()
            .filter(|key| self.content_key_current(*key))
        else {
            return false;
        };
        self.cold_adoptions[key.cell().index()]
            .compare_exchange(
                publication.source_generation * 4,
                publication.source_generation * 4 + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub(crate) fn finish_cold_publication(
        &self,
        publication: &SourcePublication,
        claimed: bool,
        accepted: bool,
    ) -> bool {
        if publication.source_generation == 0 || publication.source_generation > u64::MAX / 4 {
            return false;
        }
        let Some(key) = publication
            .key()
            .filter(|key| self.content_key_current(*key))
        else {
            return false;
        };
        let finished = self.cold_adoptions[key.cell().index()]
            .compare_exchange(
                publication.source_generation * 4 + u64::from(claimed),
                publication.source_generation * 4 + if accepted { 2 } else { 3 },
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok();
        if finished && accepted {
            self.acknowledge_callback_source(key);
        }
        finished
    }
    pub(crate) fn is_source_tracked(&self) -> bool {
        self.source_tracking
    }
    pub(crate) fn owner_identity(&self) -> OwnerIdentity {
        OwnerIdentity::of_ref(self)
    }

    pub(crate) fn slot_snapshot(&self, id: usize) -> Option<SlotSnapshot> {
        match self
            .placement
            .capture_slot(self.owner_identity(), &self.content_lifetimes, id)
        {
            PlacementObservation::Current(snapshot) => Some(snapshot),
            _ => None,
        }
    }

    pub(crate) fn content_key_current(&self, key: ContentKey) -> bool {
        self.placement
            .key_current(self.owner_identity(), &self.content_lifetimes, key)
    }

    pub(crate) fn slot_snapshot_current(&self, snapshot: SlotSnapshot) -> bool {
        matches!(
            self.placement.observe_snapshot(
                self.owner_identity(),
                &self.content_lifetimes,
                snapshot
            ),
            PlacementObservation::Current(_)
        )
    }

    pub(crate) fn binding_placement(&self, id: usize) -> Option<SlotSnapshot> {
        self.source_tracking
            .then(|| self.slot_snapshot(id))
            .flatten()
            .filter(|snapshot| snapshot.content_key().is_some())
    }

    pub(crate) fn observe_content_key(
        &self,
        key: ContentKey,
    ) -> PlacementObservation<SlotSnapshot> {
        self.placement
            .observe_key(self.owner_identity(), &self.content_lifetimes, key)
    }

    pub(super) fn prepare_placement_pair(
        &self,
        source: SlotSnapshot,
        target: SlotSnapshot,
    ) -> Result<super::content_placement::PreparedPlacement, super::content_placement::PlacementError>
    {
        self.placement.prepare_pair(
            self.owner_identity(),
            &self.content_lifetimes,
            source,
            target,
        )
    }

    pub(super) fn commit_placement_pair(
        &self,
        prepared: &super::content_placement::PreparedPlacement,
        capacity: bool,
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(),
    ) -> Result<super::content_placement::PlacementCommit, super::content_placement::PlacementError>
    {
        self.placement.commit_with(
            self.owner_identity(),
            &self.content_lifetimes,
            prepared,
            capacity,
            claim,
            apply,
        )
    }

    pub(crate) fn binding_cell(&self, id: usize, binding: InputPadBinding) -> Option<ContentCell> {
        match binding.placement {
            Some(snapshot) => snapshot
                .content_key()
                .filter(|key| self.content_key_current(*key))
                .map(ContentKey::cell),
            None if !self.source_tracking => ContentCell::from_index(id),
            None => None,
        }
    }

    pub(crate) fn authority_for_cell(&self, cell: ContentCell) -> &AtomicU64 {
        &self.authority[cell.index()]
    }

    pub(crate) fn runtime_for_cell(&self, cell: ContentCell) -> &AtomicU64 {
        &self.runtime[cell.index()]
    }

    pub(crate) fn key_lock_request_for_cell(&self, cell: ContentCell) -> &AtomicU64 {
        &self.key_lock[cell.index()].request
    }

    pub(crate) fn resident_control_for_cell(&self, cell: ContentCell) -> &AtomicU64 {
        &self.resident_controls[cell.index()]
    }

    pub(crate) fn launch_revision_for_cell(&self, cell: ContentCell) -> &AtomicU64 {
        &self.launch_revisions[cell.index()]
    }

    #[cfg(test)]
    pub(crate) fn launch_admitted_for_cell(&self, cell: ContentCell) -> &AtomicBool {
        &self.admitted_launches[cell.index()]
    }

    pub(crate) fn prepare_launch_write(
        &self,
        placement: SlotSnapshot,
        kind: LaunchWriteKind,
    ) -> Result<PreparedLaunchWrite, String> {
        self.prepare_launch_write_fixed(placement, kind)
            .map_err(|error| error.to_string())
    }

    /// Callback counterpart: errors carry only fixed enum values and never allocate.
    pub(crate) fn prepare_launch_write_fixed(
        &self,
        placement: SlotSnapshot,
        kind: LaunchWriteKind,
    ) -> Result<PreparedLaunchWrite, super::content_placement::PlacementError> {
        use super::content_placement::PlacementError;
        if !self.slot_snapshot_current(placement) {
            return Err(PlacementError::Pending);
        }
        self.prepare_launch_write_fields(placement, kind)
    }

    /// Captured fields grant no authority: the enclosing commit still checks the exact
    /// original placement and every scalar under its marker before applying any write.
    fn prepare_launch_write_fields(
        &self,
        placement: SlotSnapshot,
        kind: LaunchWriteKind,
    ) -> Result<PreparedLaunchWrite, PlacementError> {
        let cell = placement.cell().index();
        let previous_revision = self.launch_revisions[cell].load(Ordering::Acquire);
        let previous_admitted = self.admitted_launches[cell].load(Ordering::Acquire);
        let original_hold = self.migration_holds[cell].load(Ordering::Acquire);
        let original_resident = self.resident_controls[cell].load(Ordering::Acquire);
        let original_transport = self.transport_intents[cell].load(Ordering::Acquire);
        let next_transport = original_transport
            .checked_add(1)
            .ok_or(PlacementError::ObservationGenerationExhausted)?;
        if kind == LaunchWriteKind::Admit
            && (previous_revision == u64::MAX
                || original_hold != 0
                || original_resident != 0
                || self.copy_reservation(placement.slot()) != 0
                || self.pair_reservation(placement.slot()) != 0)
        {
            return Err(PlacementError::CapacityUnavailable);
        }
        Ok(PreparedLaunchWrite {
            placement,
            previous_revision,
            previous_admitted,
            next_revision: if kind == LaunchWriteKind::Admit {
                previous_revision
            } else {
                previous_revision.saturating_add(1)
            },
            next_admitted: match kind {
                LaunchWriteKind::Admit => true,
                LaunchWriteKind::Cancel => previous_admitted,
                LaunchWriteKind::Stop => false,
            },
            original_hold,
            original_resident,
            original_transport,
            next_transport,
            kind,
        })
    }

    /// The caller already owns the marker; only original captured fields are read.
    pub(crate) fn launch_write_current(&self, prepared: PreparedLaunchWrite) -> bool {
        let cell = prepared.placement.cell().index();
        self.slot_snapshot_fields_current(prepared.placement)
            && self.launch_revisions[cell].load(Ordering::Acquire) == prepared.previous_revision
            && self.admitted_launches[cell].load(Ordering::Acquire) == prepared.previous_admitted
            && self.migration_holds[cell].load(Ordering::Acquire) == prepared.original_hold
            && self.resident_controls[cell].load(Ordering::Acquire) == prepared.original_resident
            && self.transport_intents[cell].load(Ordering::Acquire) == prepared.original_transport
            && (prepared.kind != LaunchWriteKind::Admit
                || self.copy_reservation(prepared.placement.slot()) == 0)
    }

    pub(crate) fn apply_launch_write(&self, prepared: PreparedLaunchWrite) {
        let cell = prepared.placement.cell().index();
        self.launch_revisions[cell].store(prepared.next_revision, Ordering::Release);
        self.admitted_launches[cell].store(prepared.next_admitted, Ordering::Release);
        self.transport_intents[cell].store(prepared.next_transport, Ordering::Release);
    }

    pub(crate) fn commit_launch_write(
        &self,
        prepared: PreparedLaunchWrite,
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(),
    ) -> Result<SlotSnapshot, String> {
        let write = self.prepare_slot_write(prepared.placement, prepared.placement.lifetime())?;
        self.commit_slot_write(
            &write,
            || self.launch_write_current(prepared) && claim(),
            |_| {
                self.apply_launch_write(prepared);
                apply();
            },
        )
    }

    fn prepare_launch_set(
        &self,
        ids: &[usize],
        kind: LaunchWriteKind,
    ) -> Result<Vec<PreparedLaunchWrite>, String> {
        self.prepare_control_launch_set(ids, kind)
            .map_err(|error| error.to_string())
    }

    /// Control-side only: the typed error does not make this allocating Vec preflight RT-safe.
    fn prepare_control_launch_set(
        &self,
        ids: &[usize],
        kind: LaunchWriteKind,
    ) -> Result<Vec<PreparedLaunchWrite>, ControlLaunchSetError> {
        if ids.is_empty() || ids.len() > NUM_SAMPLES {
            return Err(ControlLaunchSetError::InvalidParticipants);
        }
        let mut seen = [false; NUM_SAMPLES];
        let mut prepared = Vec::with_capacity(ids.len());
        for &id in ids {
            if id >= NUM_SAMPLES || seen[id] {
                return Err(ControlLaunchSetError::InvalidParticipants);
            }
            seen[id] = true;
            let placement = match self.placement.capture_slot(
                self.owner_identity(),
                &self.content_lifetimes,
                id,
            ) {
                PlacementObservation::Current(placement) => placement,
                PlacementObservation::Pending => {
                    return Err(ControlLaunchSetError::SlotObservation(
                        PlacementError::Pending,
                    ));
                }
                PlacementObservation::Stale => {
                    return Err(ControlLaunchSetError::SlotObservation(
                        PlacementError::Stale,
                    ));
                }
                PlacementObservation::Retired => {
                    return Err(ControlLaunchSetError::SlotObservation(
                        PlacementError::Retired,
                    ));
                }
            };
            prepared.push(
                self.prepare_launch_write_fields(placement, kind)
                    .map_err(ControlLaunchSetError::Publication)?,
            );
        }
        Ok(prepared)
    }

    /// One exclusion covers the complete captured set; no participant changes if a
    /// later participant's placement or original scalar changed during preflight.
    fn commit_launch_set(
        &self,
        prepared: &[PreparedLaunchWrite],
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(),
    ) -> Result<(), String> {
        self.commit_control_launch_set(prepared, claim, apply)
            .map_err(|error| error.to_string())
    }

    /// Control-side whole-set claim, paired with the allocating preflight above.
    fn commit_control_launch_set(
        &self,
        prepared: &[PreparedLaunchWrite],
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(),
    ) -> Result<(), ControlLaunchSetError> {
        let anchor = prepared
            .first()
            .ok_or(ControlLaunchSetError::EmptyParticipants)?
            .placement;
        let write = self
            .prepare_slot_write_fixed(anchor, anchor.lifetime())
            .map_err(ControlLaunchSetError::Publication)?;
        self.commit_slot_write_fixed(
            &write,
            || {
                prepared
                    .iter()
                    .all(|entry| self.launch_write_current(*entry))
                    && claim()
            },
            |_| {
                for &entry in prepared {
                    self.apply_launch_write(entry);
                }
                apply();
            },
        )
        .map(|_| ())
        .map_err(ControlLaunchSetError::Publication)
    }

    pub(super) fn commit_migration_holds(
        &self,
        ids: &[usize],
        owner: u64,
        acquire: bool,
    ) -> Result<(), String> {
        if owner == 0 {
            return Err("empty migration hold owner".into());
        }
        let prepared = self.prepare_launch_set(ids, LaunchWriteKind::Cancel)?;
        let previous = if acquire { 0 } else { owner };
        if prepared.iter().any(|entry| entry.original_hold != previous) {
            return Err("migration hold owner changed".into());
        }
        self.commit_launch_set(
            &prepared,
            || true,
            || {
                for entry in &prepared {
                    self.migration_holds[entry.placement.cell().index()]
                        .store(if acquire { owner } else { 0 }, Ordering::Release);
                }
            },
        )
    }

    pub(crate) fn authority_revision(&self, id: usize) -> u64 {
        self.slot_snapshot(id).map_or(0, |snapshot| {
            self.authority_for_cell(snapshot.cell())
                .load(Ordering::Acquire)
        })
    }

    pub(crate) fn runtime_revision(&self, id: usize) -> u64 {
        self.slot_snapshot(id).map_or(0, |snapshot| {
            self.runtime_for_cell(snapshot.cell())
                .load(Ordering::Acquire)
        })
    }

    pub(crate) fn prepare_runtime_revision_write(
        &self,
        placement: SlotSnapshot,
        changed: bool,
    ) -> Result<RuntimeRevisionWrite, String> {
        if !self.slot_snapshot_current(placement) {
            return Err("runtime content placement changed or is pending".into());
        }
        let previous = self
            .runtime_for_cell(placement.cell())
            .load(Ordering::Acquire);
        let next = if changed {
            previous
                .checked_add(1)
                .ok_or("prepared source publication epoch exhausted")?
        } else {
            previous
        };
        Ok(RuntimeRevisionWrite {
            placement,
            previous,
            next,
        })
    }

    /// The caller already owns the global marker. Do not perform a new slot lookup.
    pub(crate) fn runtime_revision_write_current(&self, prepared: RuntimeRevisionWrite) -> bool {
        self.slot_snapshot_fields_current(prepared.placement)
            && self
                .runtime_for_cell(prepared.placement.cell())
                .load(Ordering::Acquire)
                == prepared.previous
    }

    pub(crate) fn apply_runtime_revision_write(&self, prepared: RuntimeRevisionWrite) {
        self.runtime_for_cell(prepared.placement.cell())
            .store(prepared.next, Ordering::Release);
    }

    /// Slot admission rejects a changed binding even when its original content survives elsewhere.
    /// Already admitted effects use `current`/`authority_current` and keep the stable content key.
    pub(crate) fn admission_source_current(&self, id: usize, binding: InputPadBinding) -> bool {
        let placement_current = match binding.placement {
            Some(snapshot) => {
                snapshot.slot() == id
                    && snapshot.content_key().is_some()
                    && matches!(
                        self.placement.observe_snapshot(
                            self.owner_identity(),
                            &self.content_lifetimes,
                            snapshot
                        ),
                        PlacementObservation::Current(_)
                    )
            }
            None => !self.source_tracking,
        };
        placement_current
            && self.copy_reservation(id) == 0
            && self.pair_reservation(id) == 0
            && self.authority_current(id, binding)
            && self.binding_source_current(id, binding)
    }

    pub(crate) fn admission_current(&self, id: usize, binding: InputPadBinding) -> bool {
        self.admission_source_current(id, binding) && self.current(id, binding)
    }

    /// Fixed-slot admission guard usable inside the already acquired publication marker.
    pub(crate) fn admission_fields_current(&self, id: usize, binding: InputPadBinding) -> bool {
        id < NUM_SAMPLES
            && binding.placement.map_or(!self.source_tracking, |snapshot| {
                snapshot.slot() == id
                    && snapshot.content_key().is_some()
                    && self.slot_snapshot_fields_current(snapshot)
            })
            && self.copy_reservation(id) == 0
            && self.pair_reservation(id) == 0
            && self.current(id, binding)
            && self.binding_source_current(id, binding)
            && self.binding_cell(id, binding).is_some_and(|cell| {
                self.resident_controls[cell.index()].load(Ordering::Acquire) == 0
                    && self.migration_holds[cell.index()].load(Ordering::Acquire) == 0
            })
    }

    pub(super) fn copy_reservation(&self, id: usize) -> u64 {
        self.copy_reservations[id].load(Ordering::Acquire)
    }

    pub(super) fn ensure_no_copy_reservation(&self, id: usize) -> PyResult<()> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        if self.copy_reservation(id) != 0 {
            return Err(PyRuntimeError::new_err("pad content copy is pending"));
        }
        if self.pair_reservation(id) != 0 {
            return Err(PyRuntimeError::new_err("pad content pair is pending"));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn reserve_copy(&self, id: usize, owner: u64) -> PyResult<()> {
        if owner == 0 || !self.copy_target_empty(id) {
            return Err(PyValueError::new_err("copy requires an empty target"));
        }
        let captured = self
            .slot_snapshot(id)
            .ok_or_else(|| PyRuntimeError::new_err("copy target placement is pending"))?;
        self.reserve_copy_target(captured, owner)
    }

    pub(super) fn reserve_copy_target(&self, captured: SlotSnapshot, owner: u64) -> PyResult<()> {
        let id = captured.slot();
        self.ensure_no_copy_reservation(id)?;
        if owner == 0 || !self.slot_snapshot_current(captured) {
            return Err(PyValueError::new_err("copy target placement changed"));
        }
        self.copy_reservations[id]
            .compare_exchange(0, owner, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PyRuntimeError::new_err("pad content copy is pending"))?;
        Ok(())
    }

    pub(super) fn release_copy(&self, id: usize, owner: u64) -> PyResult<()> {
        self.copy_reservations[id]
            .compare_exchange(owner, 0, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PyRuntimeError::new_err("pad content copy owner changed"))?;
        Ok(())
    }

    pub(super) fn copy_target_empty(&self, id: usize) -> bool {
        self.slot_snapshot(id).is_some_and(|snapshot| {
            self.sources[snapshot.cell().index()]
                .generation
                .load(Ordering::SeqCst)
                == 0
        })
    }

    pub(super) fn copy_parameter_epoch(&self, id: usize) -> u64 {
        self.slot_snapshot(id).map_or(0, |snapshot| {
            self.copy_parameter_epochs[snapshot.cell().index()].load(Ordering::Acquire)
        })
    }

    pub(crate) fn content_lifetime(&self, id: usize) -> u64 {
        self.slot_snapshot(id)
            .map_or(0, |snapshot| snapshot.lifetime())
    }

    #[cfg(test)]
    pub(super) fn publish_content_lifetime(&self, id: usize, lifetime: u64) {
        if let Some(snapshot) = self.slot_snapshot(id) {
            self.content_lifetimes[snapshot.cell().index()].store(lifetime, Ordering::Release);
        }
    }

    #[cfg(test)]
    pub(crate) fn next_key_lock_request(&self, id: usize) -> PyResult<u64> {
        let request = self.reserve_key_lock_request(id)?;
        self.publish_key_lock_request(id, request);
        Ok(request)
    }

    #[cfg(test)]
    pub(crate) fn reserve_key_lock_request(&self, id: usize) -> PyResult<u64> {
        if id >= NUM_SAMPLES {
            return Err(PyRuntimeError::new_err("invalid Key Lock pad"));
        }
        let cell = self
            .slot_snapshot(id)
            .ok_or_else(|| PyRuntimeError::new_err("content placement is pending"))?
            .cell();
        next_epoch(&self.key_lock[cell.index()].request).map_err(PyRuntimeError::new_err)
    }

    #[cfg(test)]
    pub(crate) fn publish_key_lock_request(&self, id: usize, request: u64) {
        if let Some(snapshot) = self.slot_snapshot(id) {
            self.key_lock[snapshot.cell().index()]
                .request
                .store(request, Ordering::Release);
        }
    }

    pub(crate) fn key_lock_status(&self, id: usize) -> Option<KeyLockStatus> {
        let cell = self.slot_snapshot(id)?.cell();
        let source = &self.sources[cell.index()];
        let current = || {
            let status = self.key_lock[cell.index()].read()?;
            (source.generation.load(Ordering::SeqCst) == status.source_generation
                && source.address.load(Ordering::SeqCst) == status.source_address
                && source.window_revision.load(Ordering::SeqCst) == status.window_revision)
                .then_some(status)
        };
        let status = current()?;
        let again = current()?;
        (again.request_id == status.request_id
            && again.source_generation == status.source_generation
            && again.window_revision == status.window_revision)
            .then_some(again)
    }
    #[cfg(test)]
    pub(crate) fn mark_launch_admitted(&self, id: usize) {
        if let Some(snapshot) = self.slot_snapshot(id) {
            self.admitted_launches[snapshot.cell().index()].store(true, Ordering::Release);
        }
    }

    pub(super) fn admitted_launch_ids(&self) -> Vec<usize> {
        (0..NUM_SAMPLES)
            .filter(|&id| self.launch_admitted(id))
            .collect()
    }

    pub(crate) fn launch_revision(&self, id: usize) -> u64 {
        self.slot_snapshot(id).map_or(u64::MAX, |snapshot| {
            self.launch_revisions[snapshot.cell().index()].load(Ordering::Acquire)
        })
    }

    fn launch_admitted(&self, id: usize) -> bool {
        self.slot_snapshot(id).is_some_and(|snapshot| {
            self.admitted_launches[snapshot.cell().index()].load(Ordering::Acquire)
        })
    }

    pub(crate) fn binding_launch_current(
        &self,
        id: usize,
        binding: InputPadBinding,
        revision: u64,
    ) -> bool {
        self.binding_cell(id, binding).is_some_and(|cell| {
            revision != u64::MAX
                && self.migration_holds[cell.index()].load(Ordering::Acquire) == 0
                && self.launch_revisions[cell.index()].load(Ordering::Acquire) == revision
        })
    }

    #[cfg(test)]
    pub(crate) fn launch_current(&self, id: usize, revision: u64) -> bool {
        id < NUM_SAMPLES
            && revision != u64::MAX
            && self.migration_hold(id) == 0
            && self.copy_reservation(id) == 0
            && self.launch_revision(id) == revision
    }

    pub(super) fn migration_hold(&self, id: usize) -> u64 {
        self.slot_snapshot(id).map_or(u64::MAX, |snapshot| {
            self.migration_holds[snapshot.cell().index()].load(Ordering::Acquire)
        })
    }

    pub(super) fn migration_hold_for_cell(&self, cell: ContentCell) -> u64 {
        self.migration_holds[cell.index()].load(Ordering::Acquire)
    }

    /// Settlement releases the original hold cells even after source replacement.
    /// The marker serializes the complete exact-owner release without numeric re-admission.
    pub(super) fn release_migration_hold_cells(
        &self,
        cells: &[ContentCell],
        owner: u64,
    ) -> Result<(), String> {
        if owner == 0 || cells.is_empty() || cells.len() > NUM_SAMPLES {
            return Err("empty migration hold owner or participant set".into());
        }
        struct PreparedHoldRelease {
            cell: ContentCell,
            previous_revision: u64,
            previous_admitted: bool,
            original_resident: u64,
            original_transport: u64,
            next_revision: u64,
            next_transport: u64,
        }
        let mut seen = [false; NUM_SAMPLES];
        let mut prepared = Vec::with_capacity(cells.len());
        for &cell in cells {
            let index = cell.index();
            if seen[index] || self.migration_hold_for_cell(cell) != owner {
                return Err("migration hold participant or owner changed".into());
            }
            seen[index] = true;
            let previous_revision = self.launch_revisions[index].load(Ordering::Acquire);
            let original_transport = self.transport_intents[index].load(Ordering::Acquire);
            prepared.push(PreparedHoldRelease {
                cell,
                previous_revision,
                previous_admitted: self.admitted_launches[index].load(Ordering::Acquire),
                original_resident: self.resident_controls[index].load(Ordering::Acquire),
                original_transport,
                next_revision: previous_revision
                    .checked_add(1)
                    .ok_or("launch revision exhausted")?,
                next_transport: original_transport
                    .checked_add(1)
                    .ok_or("transport intent exhausted")?,
            });
        }
        let anchor = self
            .slot_snapshot(self.control_slot_for_cell(cells[0]))
            .filter(|snapshot| snapshot.cell() == cells[0])
            .ok_or("migration hold placement is pending")?;
        let write = self
            .prepare_slot_write_fixed(anchor, anchor.lifetime())
            .map_err(|error| error.to_string())?;
        self.commit_slot_write_fixed(
            &write,
            || {
                prepared.iter().all(|entry| {
                    let index = entry.cell.index();
                    self.migration_hold_for_cell(entry.cell) == owner
                        && self.launch_revisions[index].load(Ordering::Acquire)
                            == entry.previous_revision
                        && self.admitted_launches[index].load(Ordering::Acquire)
                            == entry.previous_admitted
                        && self.resident_controls[index].load(Ordering::Acquire)
                            == entry.original_resident
                        && self.transport_intents[index].load(Ordering::Acquire)
                            == entry.original_transport
                })
            },
            |_| {
                for entry in &prepared {
                    let index = entry.cell.index();
                    self.launch_revisions[index].store(entry.next_revision, Ordering::Release);
                    self.admitted_launches[index].store(entry.previous_admitted, Ordering::Release);
                    self.transport_intents[index].store(entry.next_transport, Ordering::Release);
                    self.migration_holds[index].store(0, Ordering::Release);
                }
            },
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    /// A stop revokes earlier starts without changing adopted source/window/timing.
    #[cfg(test)]
    pub(super) fn cancel_launches(&self, id: usize) {
        let prepared = self
            .prepare_launch_set(&[id], LaunchWriteKind::Cancel)
            .unwrap();
        self.commit_launch_set(&prepared, || true, || {}).unwrap();
    }

    #[cfg(test)]
    pub(super) fn begin_resident_control(&self, id: usize, intent: u64) {
        if let Some(snapshot) = self.slot_snapshot(id) {
            self.resident_controls[snapshot.cell().index()].store(intent, Ordering::Release);
        }
    }

    #[cfg(test)]
    pub(crate) fn finish_resident_control(&self, id: usize, intent: u64) {
        let Some(snapshot) = self.slot_snapshot(id) else {
            return;
        };
        let _ = self.resident_controls[snapshot.cell().index()].compare_exchange(
            intent,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub(crate) fn resident_control_pending(&self, id: usize) -> bool {
        self.slot_snapshot(id).is_some_and(|snapshot| {
            self.resident_controls[snapshot.cell().index()].load(Ordering::Acquire) != 0
        })
    }

    #[cfg(test)]
    pub(super) fn cancel_cold(&self, id: usize, generation: u64) -> bool {
        if id >= NUM_SAMPLES || generation == 0 || generation > u64::MAX / 4 {
            return false;
        }
        let Some(snapshot) = self.slot_snapshot(id) else {
            return false;
        };
        let Ok(write) = self.prepare_slot_write_fixed(snapshot, snapshot.lifetime()) else {
            return false;
        };
        self.commit_slot_write_fixed(
            &write,
            || {
                self.cold_adoptions[snapshot.cell().index()].load(Ordering::Acquire)
                    == generation * 4
            },
            |cell| self.cold_adoptions[cell.index()].store(generation * 4 + 3, Ordering::Release),
        )
        .is_ok()
    }

    #[cfg(test)]
    pub(super) fn cold_status(&self, id: usize, generation: u64) -> Option<u8> {
        let cell = self.slot_snapshot(id)?.cell();
        let status = self.cold_adoptions[cell.index()].load(Ordering::Acquire);
        (status / 4 == generation).then_some((status % 4) as u8)
    }

    /// AudioEngine tracks actual native control-cache ownership; standalone mixers are untracked.
    pub(super) fn tracked() -> Self {
        Self {
            source_tracking: true,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn publish_test_source(
        &self,
        id: usize,
        sample: &SampleBuffer,
        rate: u32,
        generation: u64,
    ) {
        if self.content_lifetime(id) == 0 || !self.source_current(id, sample, rate) {
            self.publish_content_lifetime(id, super::content_lifetime::reserve().unwrap());
        }
        self.publish_source(id, sample, rate, generation);
    }

    pub(super) fn revoke_source(&self, id: usize) {
        let Some(snapshot) = self.slot_snapshot(id) else {
            return;
        };
        let cell = snapshot.cell().index();
        self.sources[cell].generation.store(0, Ordering::SeqCst);
        self.resident_controls[cell].store(0, Ordering::Release);
        self.key_lock[cell].clear();
    }

    pub(super) fn revoke_all_sources(&self) {
        for id in 0..NUM_SAMPLES {
            self.revoke_source(id);
        }
    }

    /// Called only after successful queue/cache/generation/digest publication under request owner.
    #[cfg(test)]
    pub(super) fn publish_source(
        &self,
        id: usize,
        sample: &SampleBuffer,
        rate: u32,
        generation: u64,
    ) {
        let Some(snapshot) = self.slot_snapshot(id) else {
            return;
        };
        self.publish_source_for_cell(snapshot.cell(), sample, rate, generation);
    }

    fn publish_source_for_cell(
        &self,
        cell: ContentCell,
        sample: &SampleBuffer,
        rate: u32,
        generation: u64,
    ) {
        let cell = cell.index();
        let source = &self.sources[cell];
        source.generation.store(0, Ordering::SeqCst);
        self.key_lock[cell].clear();
        source
            .address
            .store(sample.source_address(), Ordering::SeqCst);
        source
            .count
            .store(sample.source_sample_count(), Ordering::SeqCst);
        source.channels.store(sample.channels, Ordering::SeqCst);
        source.rate.store(rate, Ordering::SeqCst);
        self.publish_window_for_cell(
            ContentCell::from_index(cell).expect("fixed content cell"),
            sample,
        );
        source.generation.store(generation, Ordering::SeqCst);
    }

    /// Actual callback adoption is the window linearization point. Complete source
    /// generation/authority stays unchanged; stale MIDI/preparation views are fenced.
    #[cfg(test)]
    pub(crate) fn publish_window(&self, id: usize, sample: &SampleBuffer) {
        let Some(snapshot) = self.slot_snapshot(id) else {
            return;
        };
        self.publish_window_for_cell(snapshot.cell(), sample);
    }

    pub(crate) fn publish_window_for_cell(&self, cell: ContentCell, sample: &SampleBuffer) {
        let source = &self.sources[cell.index()];
        source.window_revision.store(0, Ordering::SeqCst);
        source
            .resident_address
            .store(sample.samples.as_ptr() as usize, Ordering::SeqCst);
        source
            .resident_start
            .store(sample.resident_start(), Ordering::SeqCst);
        source
            .resident_end
            .store(sample.resident_end(), Ordering::SeqCst);
        source.resident_context.store(
            sample
                .residency
                .as_ref()
                .map_or(0, |view| match view.context {
                    crate::messages::ResidentContext::FiniteLoop => 1,
                    crate::messages::ResidentContext::FullTrack => 2,
                    crate::messages::ResidentContext::KeyLockFullTrack => 3,
                    crate::messages::ResidentContext::KeyLockFiniteLoop => 4,
                }),
            Ordering::SeqCst,
        );
        source
            .window_revision
            .store(sample.window_revision(), Ordering::SeqCst);
    }

    pub(crate) fn binding_window_current(&self, id: usize, binding: InputPadBinding) -> bool {
        if !self.source_tracking {
            return true;
        }
        let Some(cell) = self.binding_cell(id, binding) else {
            return false;
        };
        let source = &self.sources[cell.index()];
        let Some(resident) = binding.resident else {
            return source.window_revision.load(Ordering::SeqCst) == 0;
        };
        let context = match resident.context {
            crate::messages::ResidentContext::FiniteLoop => 1,
            crate::messages::ResidentContext::FullTrack => 2,
            crate::messages::ResidentContext::KeyLockFullTrack => 3,
            crate::messages::ResidentContext::KeyLockFiniteLoop => 4,
        };
        source.window_revision.load(Ordering::SeqCst) == resident.revision
            && source.resident_address.load(Ordering::SeqCst) == resident.address
            && source.resident_start.load(Ordering::SeqCst) == resident.start
            && source.resident_end.load(Ordering::SeqCst) == resident.end
            && source.resident_context.load(Ordering::SeqCst) == context
            && source.window_revision.load(Ordering::SeqCst) == resident.revision
    }

    pub(crate) fn source_current(&self, id: usize, sample: &SampleBuffer, rate: u32) -> bool {
        self.source_generation(id, sample, rate).is_some()
    }

    /// Capture the actual loaded request generation with one bounded source check/recheck.
    /// Standalone mixers do not have control-cache tracking; their immutable pin is authoritative.
    pub(crate) fn source_generation(
        &self,
        id: usize,
        sample: &SampleBuffer,
        rate: u32,
    ) -> Option<u64> {
        if !self.source_tracking {
            return Some(1);
        }
        let cell = self.slot_snapshot(id)?.cell();
        self.source_generation_for_cell(cell, sample, rate)
    }

    fn source_generation_for_cell(
        &self,
        cell: ContentCell,
        sample: &SampleBuffer,
        rate: u32,
    ) -> Option<u64> {
        let source = &self.sources[cell.index()];
        let generation = source.generation.load(Ordering::SeqCst);
        (generation != 0
            && source.address.load(Ordering::SeqCst) == sample.source_address()
            && source.count.load(Ordering::SeqCst) == sample.source_sample_count()
            && source.channels.load(Ordering::SeqCst) == sample.channels
            && source.rate.load(Ordering::SeqCst) == rate
            && source.generation.load(Ordering::SeqCst) == generation)
            .then_some(generation)
    }

    /// Successful control admission publishes declared authority before its callback clear.
    #[cfg(test)]
    pub(super) fn set_timing_intent(&self, id: usize, intent: TimingIntent) {
        if let Some(snapshot) = self.slot_snapshot(id) {
            self.set_timing_intent_for_cell(snapshot.cell(), intent);
        }
    }

    pub(crate) fn set_timing_intent_for_cell(&self, cell: ContentCell, intent: TimingIntent) {
        let value = match intent {
            TimingIntent::Legacy => 0,
            TimingIntent::Automatic => 1,
            TimingIntent::Manual => 2,
            TimingIntent::Tap => 3,
        };
        self.timing_intent[cell.index()].store(value, Ordering::Release);
    }

    /// New voice admission cannot reinterpret unavailable Automatic as Legacy fallback.
    /// Existing effective source/history keeps rendering while a successful clear is pending.
    pub(crate) fn source_timing_available(
        &self,
        id: usize,
        accepted: Option<AcceptedTimingProjection>,
        acknowledgements: &CurrentTimingAcknowledgements,
    ) -> bool {
        self.slot_snapshot(id).is_some_and(|snapshot| {
            self.source_timing_available_for_cell(snapshot.cell(), accepted, acknowledgements)
        })
    }

    fn source_timing_available_for_cell(
        &self,
        cell: ContentCell,
        accepted: Option<AcceptedTimingProjection>,
        acknowledgements: &CurrentTimingAcknowledgements,
    ) -> bool {
        if let Some(accepted) = accepted {
            ContentKey::new(
                self.owner_identity(),
                cell,
                self.content_lifetimes[cell.index()].load(Ordering::Acquire),
            )
            .is_some_and(|key| {
                acknowledgements.current_for_key(self, key) == accepted.publication_epoch
            }) || (!self.source_tracking
                && acknowledgements.current_epoch(cell.index()) == accepted.publication_epoch)
        } else {
            self.timing_intent[cell.index()].load(Ordering::Acquire) != 1
        }
    }

    pub(crate) fn source_timing_available_for_binding(
        &self,
        id: usize,
        binding: InputPadBinding,
        acknowledgements: &CurrentTimingAcknowledgements,
    ) -> bool {
        self.binding_cell(id, binding).is_some_and(|cell| {
            self.source_timing_available_for_cell(cell, binding.accepted, acknowledgements)
        })
    }

    pub(super) fn next_authority(&self, id: usize) -> PyResult<u64> {
        self.ensure_no_copy_reservation(id)?;
        let cell = self
            .slot_snapshot(id)
            .ok_or_else(|| PyRuntimeError::new_err("content placement is pending"))?
            .cell();
        next_epoch(self.authority_for_cell(cell)).map_err(PyRuntimeError::new_err)
    }

    pub(super) fn revoke(&self, id: usize, next: u64) {
        if let Some(snapshot) = self.slot_snapshot(id) {
            self.authority_for_cell(snapshot.cell())
                .store(next, Ordering::Release);
        }
    }

    pub(crate) fn current(&self, id: usize, binding: InputPadBinding) -> bool {
        self.authority_current(id, binding)
            && self.binding_window_current(id, binding)
            && self.binding_cell(id, binding).is_some_and(|cell| {
                self.runtime_for_cell(cell).load(Ordering::Acquire) == binding.runtime_revision
            })
    }

    pub(crate) fn binding_timing_intent(&self, id: usize, binding: InputPadBinding) -> Option<u8> {
        self.binding_cell(id, binding)
            .map(|cell| self.timing_intent[cell.index()].load(Ordering::Acquire))
    }

    pub(crate) fn authority_current(&self, id: usize, binding: InputPadBinding) -> bool {
        id < NUM_SAMPLES
            && self.binding_cell(id, binding).is_some_and(|cell| {
                self.authority_for_cell(cell).load(Ordering::Acquire) == binding.authority_revision
            })
    }

    pub(crate) fn binding_source_current(&self, id: usize, binding: InputPadBinding) -> bool {
        self.binding_source_generation(id, binding).is_some()
            && self.binding_window_current(id, binding)
    }

    /// Actual callback source adoption, rather than a populated control cache.
    /// Normal restored sources use the cold publication lane; its rejected/pending
    /// and merely control-published states confer no fresh recovery acceptance.
    pub(crate) fn binding_source_adoption_accepted(
        &self,
        id: usize,
        binding: InputPadBinding,
    ) -> bool {
        if !self.source_tracking {
            return false;
        }
        let Some(generation) = self.binding_source_generation(id, binding) else {
            return false;
        };
        let Some(cell) = self.binding_cell(id, binding) else {
            return false;
        };
        let Some(accepted) = generation
            .checked_mul(4)
            .and_then(|base| base.checked_add(2))
        else {
            return false;
        };
        self.cold_adoptions[cell.index()].load(Ordering::Acquire) == accepted
            && self.binding_window_current(id, binding)
            && self.binding_source_generation(id, binding) == Some(generation)
            && self.cold_adoptions[cell.index()].load(Ordering::Acquire) == accepted
    }

    /// Read-only fixed source fence, including its checked generation for evidence consumers.
    pub(crate) fn binding_source_generation(
        &self,
        id: usize,
        binding: InputPadBinding,
    ) -> Option<u64> {
        if id >= NUM_SAMPLES {
            return None;
        }
        if !self.source_tracking {
            return Some(1);
        }
        let cell = self.binding_cell(id, binding)?;
        let source = &self.sources[cell.index()];
        let generation = source.generation.load(Ordering::SeqCst);
        (generation != 0
            && source.address.load(Ordering::SeqCst) == binding.source_address
            && source.count.load(Ordering::SeqCst) == binding.sample_count
            && source.channels.load(Ordering::SeqCst) == binding.channels
            && source.rate.load(Ordering::SeqCst) == binding.sample_rate_hz
            && source.generation.load(Ordering::SeqCst) == generation)
            .then_some(generation)
    }
}

const CONTROL_LAUNCH_PUBLICATION_ATTEMPTS: usize = 64;

/// Off-realtime only. The caller keeps its original producer guard across every fresh
/// whole-set preflight/claim. Brief Pending contention yields; other failures never retry.
fn retry_control_launch_publication<T>(
    mut attempt: impl FnMut() -> Result<T, ControlLaunchSetError>,
) -> Result<T, ControlLaunchSetError> {
    let mut result = attempt();
    for _ in 1..CONTROL_LAUNCH_PUBLICATION_ATTEMPTS {
        if !result.as_ref().is_err_and(|error| error.pending()) {
            return result;
        }
        #[cfg(test)]
        launch_control_publication_tests::observe_pending();
        std::thread::yield_now();
        result = attempt();
    }
    result
}

/// Capture every earlier admitted target and revoke its old launch revision at one
/// admission boundary. A live caller supplies the same producer mutex used by
/// UI, MIDI and global starts. None is only for an engine without a stream.
/// Pure cancellation keeps the flags set: a start may have passed its final RT
/// guard already and still needs an ordered STOP before feedback reaches Python.
pub(super) fn cancel_launches_with_producer(
    ownership: &InputRuntimeOwnership,
    producer: Option<&Arc<Mutex<rtrb::Producer<crate::messages::ControlMessage>>>>,
    id: Option<usize>,
) -> PyResult<Vec<usize>> {
    if id.is_some_and(|id| id >= NUM_SAMPLES) {
        return Err(pyo3::exceptions::PyValueError::new_err("id out of range"));
    }
    let _producer = producer
        .map(|producer| {
            producer
                .lock()
                .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))
        })
        .transpose()?;
    let ids: Vec<_> = id.map_or_else(|| (0..NUM_SAMPLES).collect(), |id| vec![id]);
    retry_control_launch_publication(|| {
        let prepared = ownership.prepare_control_launch_set(&ids, LaunchWriteKind::Cancel)?;
        let targets = prepared
            .iter()
            .filter(|entry| entry.previous_admitted)
            .map(|entry| entry.placement.slot())
            .collect();
        ownership.commit_control_launch_set(&prepared, || true, || {})?;
        Ok(targets)
    })
    .map_err(|error| PyRuntimeError::new_err(error.to_string()))
}

/// Runtime admission is disabled before shutdown calls this control-only fence.
/// Recover a poisoned producer solely to revoke queued starts before worker joins;
/// public admission/cancellation still reports a poisoned producer as an error.
pub(super) fn cancel_launches_before_shutdown(
    ownership: &InputRuntimeOwnership,
    producer: Option<&Arc<Mutex<rtrb::Producer<crate::messages::ControlMessage>>>>,
) -> Result<(), String> {
    let _producer = producer.map(|producer| {
        producer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    });
    let ids: Vec<_> = (0..NUM_SAMPLES).collect();
    // This is off realtime, after input admission is disabled. Retry a bounded
    // transient publication; a permanently interrupted marker is reported instead
    // of claiming that shutdown revoked permissions it could not settle.
    for _ in 0..128 {
        let result = ownership
            .prepare_launch_set(&ids, LaunchWriteKind::Cancel)
            .and_then(|prepared| ownership.commit_launch_set(&prepared, || true, || {}));
        match result {
            Ok(()) => return Ok(()),
            Err(error)
                if ownership.placement_pending()
                    || error.contains("pending")
                    || error.contains("changed")
                    || error.contains("stale")
                    || error.contains("rejected") =>
            {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
    Err("launch shutdown settlement remains pending".into())
}

#[cfg(test)]
#[path = "launch_control_publication_tests.rs"]
mod launch_control_publication_tests;

/// Producer capacity is reserved before changing launch ownership. This helper is
/// shared by public stops and direct MIDI stops; callers hold the producer lock.
pub(super) fn enqueue_stop_with_producer(
    ownership: &InputRuntimeOwnership,
    producer: &mut rtrb::Producer<crate::messages::ControlMessage>,
    id: Option<usize>,
) -> bool {
    if producer.is_full() || id.is_some_and(|id| id >= NUM_SAMPLES) {
        return false;
    }
    let ids: Vec<_> = id.map_or_else(|| (0..NUM_SAMPLES).collect(), |id| vec![id]);
    retry_control_launch_publication(|| {
        let prepared = ownership.prepare_control_launch_set(&ids, LaunchWriteKind::Stop)?;
        let message = if id.is_some() {
            crate::messages::ControlMessage::StopSampleForPlacement {
                placement: prepared[0].placement,
            }
        } else {
            crate::messages::ControlMessage::StopAll()
        };
        // The sole producer owns an available slot; the consumer can only free more.
        ownership.commit_control_launch_set(
            &prepared,
            || true,
            || {
                producer
                    .push(message)
                    .expect("reserved single-producer STOP capacity");
            },
        )
    })
    .is_ok()
}

/// Fixed scheduler value. Pointer is compared only; monotonic authority fences
/// reject source replacement/unload before the old callback bank is retired.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct InputPadBinding {
    pub(crate) placement: Option<SlotSnapshot>,
    pub(crate) resident: Option<crate::messages::ResidentBinding>,
    pub(crate) source_address: usize,
    pub(crate) sample_count: usize,
    pub(crate) channels: usize,
    pub(crate) sample_rate_hz: u32,
    pub(crate) authority_revision: u64,
    pub(crate) runtime_revision: u64,
    pub(crate) accepted: Option<AcceptedTimingProjection>,
}

impl InputPadBinding {
    /// The same complete source/runtime projection, independent of unrelated layout writes.
    /// This compares captured values only; current authority still requires its native guards.
    pub(crate) fn same_projection(&self, other: &Self) -> bool {
        let placement_matches = match (self.placement, other.placement) {
            (Some(left), Some(right)) => left.same_projection(right),
            (None, None) => true,
            _ => false,
        };
        placement_matches
            && self.resident == other.resident
            && self.source_address == other.source_address
            && self.sample_count == other.sample_count
            && self.channels == other.channels
            && self.sample_rate_hz == other.sample_rate_hz
            && self.authority_revision == other.authority_revision
            && self.runtime_revision == other.runtime_revision
            && self.accepted == other.accepted
    }
}

#[pyclass(frozen)]
#[derive(Clone)]
pub struct InputRuntimePadBinding {
    pub(super) id: usize,
    pub(super) source_generation: u64,
    content_lifetime: u64,
    source_digest: String,
    sample_rate_hz: u32,
    intent: TimingIntent,
    pub(super) binding: InputPadBinding,
    pub(super) ownership: Arc<InputRuntimeOwnership>,
    pub(super) acknowledgements: Arc<CurrentTimingAcknowledgements>,
    accepted: Option<CurrentConstantTimingRecord>,
}

impl InputRuntimePadBinding {
    /// Already admitted work keeps its original source and timing owners through placement.
    /// Pair reservations fence new admission; they do not revoke this captured permission.
    pub(super) fn admitted_current(&self) -> bool {
        let current = || {
            let Some(cell) = self.ownership.binding_cell(self.id, self.binding) else {
                return false;
            };
            let intent = match self.intent {
                TimingIntent::Legacy => 0,
                TimingIntent::Automatic => 1,
                TimingIntent::Manual => 2,
                TimingIntent::Tap => 3,
            };
            self.ownership.authority_current(self.id, self.binding)
                && self
                    .ownership
                    .binding_source_generation(self.id, self.binding)
                    == Some(self.source_generation)
                && self.ownership.binding_window_current(self.id, self.binding)
                && self.ownership.timing_intent[cell.index()].load(Ordering::Acquire) == intent
                && self
                    .acknowledgements
                    .current_for_binding(&self.ownership, self.id, self.binding)
                    == self
                        .binding
                        .accepted
                        .map_or(0, |accepted| accepted.publication_epoch)
        };
        current() && current()
    }

    /// Final arm uses the resolved control placement inside its own mutation marker.
    /// No fresh fixed-slot admission or replacement binding grants permission here.
    pub(super) fn admitted_publication_current(&self, resolved: SlotSnapshot) -> bool {
        self.binding.placement.is_some_and(|original| {
            original.content_key().is_some()
                && original.content_key() == resolved.content_key()
                && self.ownership.slot_snapshot_fields_current(resolved)
                && self.ownership.control_slot_for_cell(original.cell()) == resolved.slot()
                && self.ownership.copy_reservation(resolved.slot()) == 0
                && self.admitted_current()
                && self.ownership.slot_snapshot_fields_current(resolved)
        })
    }

    pub(super) fn copy_timing_context(
        &self,
    ) -> (TimingIntent, Option<CurrentConstantTimingRecord>) {
        (self.intent, self.accepted.clone())
    }

    #[cfg(test)]
    pub(super) fn for_test(id: usize, ownership: Arc<InputRuntimeOwnership>) -> Self {
        if ownership.content_lifetime(id) == 0 {
            ownership.publish_content_lifetime(id, super::content_lifetime::reserve().unwrap());
        }
        Self {
            id,
            source_generation: 1,
            content_lifetime: ownership.content_lifetime(id),
            source_digest: "a".repeat(64),
            sample_rate_hz: 48_000,
            intent: TimingIntent::Legacy,
            binding: InputPadBinding {
                placement: ownership.slot_snapshot(id),
                resident: None,
                source_address: 0,
                sample_count: 1,
                channels: 1,
                sample_rate_hz: 48_000,
                authority_revision: ownership.authority_revision(id),
                runtime_revision: 0,
                accepted: None,
            },
            ownership,
            acknowledgements: Arc::default(),
            accepted: None,
        }
    }
    pub(super) fn current(&self) -> bool {
        self.ownership
            .admission_source_current(self.id, self.binding)
            && self.ownership.binding_source_current(self.id, self.binding)
            && self
                .acknowledgements
                .current_for_binding(&self.ownership, self.id, self.binding)
                == self
                    .binding
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
            && self
                .ownership
                .admission_source_current(self.id, self.binding)
    }

    /// Original loaded-source guard for the complete runtime publication claim. The
    /// caller holds the global marker, so fixed-slot fields replace Pending-sensitive
    /// admission reads. The future RuntimePadState contains a different target revision.
    pub(super) fn runtime_publication_current(&self) -> bool {
        self.available() && self.control_publication_current()
    }

    /// Full captured control admission under an already owned publication marker.
    /// Pending-sensitive slot capture must not revoke this unchanged source projection.
    pub(super) fn control_publication_current(&self) -> bool {
        let current = || {
            let placement_current = match self.binding.placement {
                Some(placement) => {
                    placement.slot() == self.id
                        && placement.content_key().is_some()
                        && self.ownership.slot_snapshot_fields_current(placement)
                }
                None => !self.ownership.is_source_tracked(),
            };
            let Some(cell) = self.ownership.binding_cell(self.id, self.binding) else {
                return false;
            };
            let intent = match self.intent {
                TimingIntent::Legacy => 0,
                TimingIntent::Automatic => 1,
                TimingIntent::Manual => 2,
                TimingIntent::Tap => 3,
            };
            placement_current
                && self.ownership.copy_reservation(self.id) == 0
                && self.ownership.authority_current(self.id, self.binding)
                && self
                    .ownership
                    .binding_source_generation(self.id, self.binding)
                    == Some(self.source_generation)
                && self.ownership.binding_window_current(self.id, self.binding)
                && self.ownership.timing_intent[cell.index()].load(Ordering::Acquire) == intent
                && self
                    .acknowledgements
                    .current_for_binding(&self.ownership, self.id, self.binding)
                    == self
                        .binding
                        .accepted
                        .map_or(0, |accepted| accepted.publication_epoch)
        };
        current() && current()
    }

    pub(super) fn available(&self) -> bool {
        self.intent != TimingIntent::Automatic || self.binding.accepted.is_some()
    }
}

#[pymethods]
impl InputRuntimePadBinding {
    /// Confirm the actual native source/authority before capturing live intent.
    pub fn is_current(&self) -> bool {
        self.current()
    }

    /// Fresh copies of complete native metadata; historical bindings confer no authority.
    pub fn metadata(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let metadata = PyDict::new(py);
        metadata.set_item("pad_id", self.id)?;
        metadata.set_item(
            "source_id",
            format!("loaded-{}-{}", self.id, self.source_generation),
        )?;
        metadata.set_item("source_generation", self.source_generation)?;
        metadata.set_item("content_lifetime", self.content_lifetime)?;
        metadata.set_item("source_sha256", &self.source_digest)?;
        metadata.set_item("sample_rate_hz", self.sample_rate_hz)?;
        metadata.set_item(
            "frame_count",
            self.binding.sample_count / self.binding.channels,
        )?;
        metadata.set_item("channels", self.binding.channels)?;
        metadata.set_item(
            "window_revision",
            self.binding.resident.map_or(0, |view| view.revision),
        )?;
        metadata.set_item(
            "resident_start_frame",
            self.binding.resident.map_or(0, |view| view.start),
        )?;
        metadata.set_item(
            "resident_end_frame",
            self.binding
                .resident
                .map_or(self.binding.sample_count / self.binding.channels, |view| {
                    view.end
                }),
        )?;
        metadata.set_item(
            "resident_pcm_identity",
            self.binding
                .resident
                .map_or(self.binding.source_address, |view| view.address),
        )?;
        metadata.set_item(
            "resident_context",
            self.binding
                .resident
                .map_or("legacy-full-track", |view| match view.context {
                    crate::messages::ResidentContext::FiniteLoop => "finite-loop",
                    crate::messages::ResidentContext::FullTrack => "full-track",
                    crate::messages::ResidentContext::KeyLockFullTrack => "key-lock-full-track",
                    crate::messages::ResidentContext::KeyLockFiniteLoop => "key-lock-finite-loop",
                }),
        )?;
        metadata.set_item("authority_revision", self.binding.authority_revision)?;
        metadata.set_item(
            "intent",
            match self.intent {
                TimingIntent::Automatic => "automatic",
                TimingIntent::Manual => "manual",
                TimingIntent::Tap => "tap",
                TimingIntent::Legacy => "legacy",
            },
        )?;
        metadata.set_item(
            "accepted_timing",
            self.accepted
                .as_ref()
                .map(|record| record.metadata_for_slot(py, self.id))
                .transpose()?,
        )?;
        Ok(metadata.into_any().unbind())
    }
}

/// Same lock order and actual current-record predicate as current_constant_timing.
pub(super) fn capture(engine: &AudioEngine, id: usize) -> PyResult<Option<InputRuntimePadBinding>> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    capture_under_request_lock(engine, id)
}

/// Optional live controls must still name the actual current source. Saved and
/// legacy controls omit the guard and retain their existing admission behavior.
pub(super) fn validate_expected_source_generation(
    binding: Option<&InputRuntimePadBinding>,
    expected_source_generation: Option<u64>,
) -> PyResult<()> {
    if let Some(expected) = expected_source_generation
        && !binding
            .is_some_and(|binding| binding.source_generation == expected && binding.current())
    {
        return Err(PyValueError::new_err("current complete source changed"));
    }
    Ok(())
}

/// The caller holds pad_requests before this check and through its mutation.
pub(super) fn validate_expected_source_generation_under_request_lock(
    engine: &AudioEngine,
    id: usize,
    expected_source_generation: Option<u64>,
) -> PyResult<()> {
    engine
        .input_runtime_ownership
        .ensure_no_copy_reservation(id)?;
    if expected_source_generation.is_none() {
        return Ok(());
    }
    let binding = capture_under_request_lock(engine, id)?;
    validate_expected_source_generation(binding.as_ref(), expected_source_generation)
}

/// Shared current-source resolver for preparation and MIDI. The caller retains
/// the pad request lock through its own source/admission transaction.
pub(super) fn capture_under_request_lock(
    engine: &AudioEngine,
    id: usize,
) -> PyResult<Option<InputRuntimePadBinding>> {
    engine
        .input_runtime_ownership
        .ensure_no_copy_reservation(id)?;
    super::resident_relocation::reconcile_under_request_lock(engine)?;
    let intents = engine
        .timing_intents
        .lock()
        .map_err(|_| PyRuntimeError::new_err("timing intent lock poisoned"))?;
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let generations = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
    let digests = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
    let mut all_records = engine
        .current_constant_timing
        .lock()
        .map_err(|_| PyRuntimeError::new_err("current timing lock poisoned"))?;
    let Some(source) = &cache[id] else {
        return Ok(None);
    };
    let (generation, rate) = generations[id];
    let Some(digest) = &digests[id] else {
        return Ok(None);
    };
    if generation == 0 || rate == 0 || source.channels == 0 {
        return Ok(None);
    }
    let Some(placement) = engine
        .input_runtime_ownership
        .slot_snapshot(id)
        .filter(|snapshot| snapshot.content_key().is_some())
    else {
        return Ok(None);
    };
    let records = &mut all_records[id];
    constant_timing::retire_old_current_records(engine, records);
    let acknowledged = engine
        .current_timing_acknowledgements
        .current_for_slot(&engine.input_runtime_ownership, id);
    let accepted = if intents[id] == TimingIntent::Automatic {
        constant_timing::current_record_for_source(
            engine,
            id,
            source,
            generations[id],
            Some(digest),
            records,
        )
        .cloned()
    } else {
        None
    };
    if engine
        .current_timing_acknowledgements
        .current_for_slot(&engine.input_runtime_ownership, id)
        != acknowledged
    {
        return Ok(None);
    }
    Ok(Some(InputRuntimePadBinding {
        id,
        source_generation: generation,
        content_lifetime: engine.input_runtime_ownership.content_lifetime(id),
        source_digest: digest.clone(),
        sample_rate_hz: rate,
        intent: intents[id],
        binding: InputPadBinding {
            placement: Some(placement),
            resident: source.resident_binding(),
            source_address: source.source_address(),
            sample_count: source.source_sample_count(),
            channels: source.channels,
            sample_rate_hz: rate,
            authority_revision: engine.input_runtime_ownership.authority_revision(id),
            runtime_revision: 0,
            accepted: accepted.as_ref().map(|record| record.projection),
        },
        ownership: engine.input_runtime_ownership.clone(),
        acknowledgements: engine.current_timing_acknowledgements.clone(),
        accepted,
    }))
}

#[cfg(test)]
#[path = "source_publication_tests.rs"]
mod source_publication_tests;
