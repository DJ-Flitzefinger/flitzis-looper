//! Original control participants for a placement claim and accepted transport intent.
//! Reads inside a claim never recapture a slot or acquire another publication marker.

use super::{
    ContentCell, ContentKey, InputPadBinding, InputRuntimeOwnership, PlacementObservation,
    SlotSnapshot,
};
use crate::audio_engine::content_placement::PlacementError;
use crate::messages::SampleBuffer;
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceProjection {
    generation: u64,
    address: usize,
    count: usize,
    channels: usize,
    rate: u32,
    resident_address: usize,
    resident_start: usize,
    resident_end: usize,
    window_revision: u64,
    resident_context: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PairControlProjection {
    placement: SlotSnapshot,
    binding: Option<InputPadBinding>,
    source: SourceProjection,
    authority: u64,
    runtime: u64,
    timing_intent: u8,
    key_request: u64,
    cold_adoption: u64,
    resident_control: u64,
    launch_revision: u64,
    launch_admitted: bool,
    migration_hold: u64,
    transport_intent: u64,
    parameter_intent: u64,
    parameter_epoch: u64,
    copy_reservation: u64,
}

impl PairControlProjection {
    pub(crate) fn authority_revision(self) -> u64 {
        self.authority
    }
    pub(crate) fn runtime_revision(self) -> u64 {
        self.runtime
    }
    pub(crate) fn placement(self) -> SlotSnapshot {
        self.placement
    }
    pub(crate) fn binding(self) -> Option<InputPadBinding> {
        self.binding
    }
    pub(crate) fn source_generation(self) -> u64 {
        self.source.generation
    }
    pub(crate) fn transport_intent(self) -> u64 {
        self.transport_intent
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PreparedTransportIntent {
    placement: SlotSnapshot,
    previous: u64,
    next: u64,
}

impl PreparedTransportIntent {
    pub(crate) fn placement(self) -> SlotSnapshot {
        self.placement
    }
    pub(crate) fn revision(self) -> u64 {
        self.next
    }
}

impl InputRuntimeOwnership {
    /// Existing admitted work keeps its original key. A Native/control layout gap is
    /// a bounded wait, never proof of stale material or authority from another occupant.
    pub(crate) fn observe_control_key(
        &self,
        key: ContentKey,
    ) -> PlacementObservation<SlotSnapshot> {
        match self.observe_content_key(key) {
            PlacementObservation::Current(snapshot) => {
                if self.control_slot_for_cell(key.cell()) == snapshot.slot() {
                    PlacementObservation::Current(snapshot)
                } else {
                    PlacementObservation::Pending
                }
            }
            other => other,
        }
    }

    pub(crate) fn control_slot_for_cell(&self, cell: ContentCell) -> usize {
        self.control_cell_to_slot[cell.index()].load(Ordering::Acquire)
    }

    /// Fixed stores inside the complete Native control mirror, after all cache exchanges.
    pub(crate) fn publish_pair_control_slots(&self, after: &[SlotSnapshot; 2]) {
        for snapshot in after {
            self.control_cell_to_slot[snapshot.cell().index()]
                .store(snapshot.slot(), Ordering::Release);
        }
    }

    /// Validate the currently held control image against its original stable owner.
    /// The caller owns the shared marker. Newer same-content windows/intent are
    /// compared as they actually stand; captured historical scalars are not restored.
    pub(crate) fn pair_mirror_source_current(
        &self,
        original: SlotSnapshot,
        after: SlotSnapshot,
        sample: Option<&SampleBuffer>,
        generation_and_rate: (u64, u32),
        intent: TimingIntent,
        reservation: u64,
    ) -> bool {
        if original.owner() != self.owner_identity()
            || after.owner() != self.owner_identity()
            || original.cell() != after.cell()
            || original.lifetime() != after.lifetime()
            || !self.slot_snapshot_fields_current(after)
            || self.control_slot_for_cell(original.cell()) != original.slot()
            || self.pair_reservation(original.slot()) != reservation
            || self.pair_reservation(after.slot()) != reservation
            || self.copy_reservation(original.slot()) != 0
            || self.copy_reservation(after.slot()) != 0
        {
            return false;
        }
        let cell = original.cell();
        if original.lifetime() == 0 {
            return sample.is_none() && self.source_cell_empty(cell);
        }
        let Some(sample) = sample else {
            return false;
        };
        let (generation, rate) = generation_and_rate;
        let binding = InputPadBinding {
            placement: Some(original),
            resident: sample.resident_binding(),
            source_address: sample.source_address(),
            sample_count: sample.source_sample_count(),
            channels: sample.channels,
            sample_rate_hz: rate,
            authority_revision: self.authority[cell.index()].load(Ordering::Acquire),
            runtime_revision: self.runtime[cell.index()].load(Ordering::Acquire),
            accepted: None,
        };
        let declared = match intent {
            TimingIntent::Legacy => 0,
            TimingIntent::Automatic => 1,
            TimingIntent::Manual => 2,
            TimingIntent::Tap => 3,
        };
        generation != 0
            && self.source_generation_for_cell(cell, sample, rate) == Some(generation)
            && self.binding_window_current(original.slot(), binding)
            && self.timing_intent[cell.index()].load(Ordering::Acquire) == declared
    }
    fn pair_source_projection(&self, cell: ContentCell) -> SourceProjection {
        let source = &self.sources[cell.index()];
        SourceProjection {
            generation: source.generation.load(Ordering::SeqCst),
            address: source.address.load(Ordering::SeqCst),
            count: source.count.load(Ordering::SeqCst),
            channels: source.channels.load(Ordering::SeqCst),
            rate: source.rate.load(Ordering::SeqCst),
            resident_address: source.resident_address.load(Ordering::SeqCst),
            resident_start: source.resident_start.load(Ordering::SeqCst),
            resident_end: source.resident_end.load(Ordering::SeqCst),
            window_revision: source.window_revision.load(Ordering::SeqCst),
            resident_context: source.resident_context.load(Ordering::SeqCst),
        }
    }

    pub(crate) fn pair_control_projection(
        &self,
        placement: SlotSnapshot,
        binding: Option<InputPadBinding>,
    ) -> Result<PairControlProjection, PlacementError> {
        if !self.slot_snapshot_current(placement) {
            return Err(PlacementError::Stale);
        }
        let cell = placement.cell().index();
        let projection = PairControlProjection {
            placement,
            binding,
            source: self.pair_source_projection(placement.cell()),
            authority: self.authority[cell].load(Ordering::Acquire),
            runtime: self.runtime[cell].load(Ordering::Acquire),
            timing_intent: self.timing_intent[cell].load(Ordering::Acquire),
            key_request: self.key_lock[cell].request.load(Ordering::Acquire),
            cold_adoption: self.cold_adoptions[cell].load(Ordering::Acquire),
            resident_control: self.resident_controls[cell].load(Ordering::Acquire),
            launch_revision: self.launch_revisions[cell].load(Ordering::Acquire),
            launch_admitted: self.admitted_launches[cell].load(Ordering::Acquire),
            migration_hold: self.migration_holds[cell].load(Ordering::Acquire),
            transport_intent: self.transport_intents[cell].load(Ordering::Acquire),
            parameter_intent: self.parameter_intents[cell].load(Ordering::Acquire),
            parameter_epoch: self.copy_parameter_epochs[cell].load(Ordering::Acquire),
            copy_reservation: self.copy_reservations[placement.slot()].load(Ordering::Acquire),
        };
        let valid_source = match binding {
            Some(binding) => {
                placement.content_key().is_some()
                    && binding
                        .placement
                        .is_some_and(|captured| captured.same_projection(placement))
                    && binding.source_address == projection.source.address
                    && binding.sample_count == projection.source.count
                    && binding.channels == projection.source.channels
                    && binding.sample_rate_hz == projection.source.rate
                    && binding.authority_revision == projection.authority
                    && binding.runtime_revision == projection.runtime
                    && self.binding_window_current(placement.slot(), binding)
                    && projection.source.generation != 0
            }
            None => placement.lifetime() == 0 && projection.source.generation == 0,
        };
        if !valid_source
            || projection.copy_reservation != 0
            || self.pair_reservation(placement.slot()) != 0
            || !self.pair_control_projection_current(&projection, 0)
            || !self.slot_snapshot_current(placement)
        {
            return Err(PlacementError::Stale);
        }
        Ok(projection)
    }

    /// The marker is already held by the caller; original fields remain the only authority.
    pub(crate) fn pair_control_projection_current(
        &self,
        projection: &PairControlProjection,
        reservation: u64,
    ) -> bool {
        self.participant_control_fields_current(projection)
            && self.copy_reservations[projection.placement.slot()].load(Ordering::Acquire)
                == projection.copy_reservation
            && self.pair_reservation(projection.placement.slot()) == reservation
    }

    /// The same exact participant fields, with only its own single-target Copy fence.
    pub(crate) fn recovered_copy_control_projection_current(
        &self,
        projection: &PairControlProjection,
        reservation: u64,
    ) -> bool {
        reservation != 0
            && projection.copy_reservation == 0
            && self.participant_control_fields_current(projection)
            && self.copy_reservation(projection.placement.slot()) == reservation
            && self.pair_reservation(projection.placement.slot()) == 0
    }

    fn participant_control_fields_current(&self, projection: &PairControlProjection) -> bool {
        let placement = projection.placement;
        let cell = placement.cell().index();
        self.slot_snapshot_fields_current(placement)
            && self.pair_source_projection(placement.cell()) == projection.source
            && self.authority[cell].load(Ordering::Acquire) == projection.authority
            && self.runtime[cell].load(Ordering::Acquire) == projection.runtime
            && self.timing_intent[cell].load(Ordering::Acquire) == projection.timing_intent
            && self.key_lock[cell].request.load(Ordering::Acquire) == projection.key_request
            && self.cold_adoptions[cell].load(Ordering::Acquire) == projection.cold_adoption
            && self.resident_controls[cell].load(Ordering::Acquire) == projection.resident_control
            && self.launch_revisions[cell].load(Ordering::Acquire) == projection.launch_revision
            && self.admitted_launches[cell].load(Ordering::Acquire) == projection.launch_admitted
            && self.migration_holds[cell].load(Ordering::Acquire) == projection.migration_hold
            && self.transport_intents[cell].load(Ordering::Acquire) == projection.transport_intent
            && self.parameter_intents[cell].load(Ordering::Acquire) == projection.parameter_intent
            && self.copy_parameter_epochs[cell].load(Ordering::Acquire)
                == projection.parameter_epoch
    }

    pub(crate) fn pair_reservation(&self, slot: usize) -> u64 {
        self.pair_reservations
            .get(slot)
            .map_or(u64::MAX, |value| value.load(Ordering::Acquire))
    }

    pub(crate) fn reserve_pair(
        &self,
        captured: &[SlotSnapshot; 2],
        owner: u64,
    ) -> Result<(), PlacementError> {
        if owner == 0 {
            return Err(PlacementError::ClaimRejected);
        }
        if captured[0].slot() == captured[1].slot() {
            return Err(PlacementError::SameSlot);
        }
        let write = self.prepare_slot_write_fixed(captured[0], captured[0].lifetime())?;
        self.commit_slot_write_fixed(
            &write,
            || {
                captured.iter().all(|snapshot| {
                    self.slot_snapshot_fields_current(*snapshot)
                        && self.pair_reservation(snapshot.slot()) == 0
                        && self.copy_reservation(snapshot.slot()) == 0
                })
            },
            |_| {
                for snapshot in captured {
                    self.pair_reservations[snapshot.slot()].store(owner, Ordering::Release);
                }
            },
        )
        .map(|_| ())
    }

    /// Reservation belongs to the physical participants, including their post-ACK mappings.
    pub(crate) fn release_pair(
        &self,
        captured: &[SlotSnapshot; 2],
        owner: u64,
    ) -> Result<(), PlacementError> {
        if owner == 0 {
            return Err(PlacementError::ClaimRejected);
        }
        if captured[0].slot() == captured[1].slot() {
            return Err(PlacementError::SameSlot);
        }
        let anchor = self
            .slot_snapshot(captured[0].slot())
            .ok_or(PlacementError::Pending)?;
        let write = self.prepare_slot_write_fixed(anchor, anchor.lifetime())?;
        self.commit_slot_write_fixed(
            &write,
            || {
                captured.iter().all(|snapshot| {
                    snapshot.owner() == self.owner_identity()
                        && self.pair_reservation(snapshot.slot()) == owner
                })
            },
            |_| {
                for snapshot in captured {
                    self.pair_reservations[snapshot.slot()].store(0, Ordering::Release);
                }
            },
        )
        .map(|_| ())
    }

    pub(crate) fn apply_pair_empty_defaults_for_cell(&self, cell: ContentCell) {
        self.timing_intent[cell.index()].store(0, Ordering::Release);
        self.key_lock[cell.index()].clear();
    }

    pub(crate) fn transport_intent_for_cell(&self, cell: ContentCell) -> &AtomicU64 {
        &self.transport_intents[cell.index()]
    }

    /// One fresh parameter admission fences a previously captured Pair, without changing
    /// the existing Copy installation epoch or invalidating older admitted content edits.
    pub(crate) fn commit_parameter_intent(
        &self,
        placement: SlotSnapshot,
        epoch: u64,
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(),
    ) -> Result<SlotSnapshot, PlacementError> {
        let cell = placement.cell().index();
        let previous = self.parameter_intents[cell].load(Ordering::Acquire);
        let next = previous.checked_add(1).ok_or(PlacementError::Retired)?;
        let write = self.prepare_slot_write_fixed(placement, placement.lifetime())?;
        self.commit_slot_write_fixed(
            &write,
            || {
                self.slot_snapshot_fields_current(placement)
                    && self.parameter_intents[cell].load(Ordering::Acquire) == previous
                    && self.copy_parameter_epochs[cell].load(Ordering::Acquire) == epoch
                    && self.copy_reservation(placement.slot()) == 0
                    && self.pair_reservation(placement.slot()) == 0
                    && claim()
            },
            |_| {
                self.parameter_intents[cell].store(next, Ordering::Release);
                apply();
            },
        )
    }

    pub(crate) fn prepare_transport_intent(
        &self,
        placement: SlotSnapshot,
    ) -> Result<PreparedTransportIntent, PlacementError> {
        if !self.slot_snapshot_current(placement) {
            return Err(PlacementError::Stale);
        }
        let previous = self.transport_intents[placement.cell().index()].load(Ordering::Acquire);
        let next = previous.checked_add(1).ok_or(PlacementError::Retired)?;
        if !self.slot_snapshot_current(placement) {
            return Err(PlacementError::Stale);
        }
        Ok(PreparedTransportIntent {
            placement,
            previous,
            next,
        })
    }

    pub(crate) fn transport_intent_current(&self, prepared: PreparedTransportIntent) -> bool {
        self.slot_snapshot_fields_current(prepared.placement)
            && self.transport_intents[prepared.placement.cell().index()].load(Ordering::Acquire)
                == prepared.previous
    }

    pub(crate) fn apply_transport_intent(&self, prepared: PreparedTransportIntent) {
        self.transport_intents[prepared.placement.cell().index()]
            .store(prepared.next, Ordering::Release);
    }

    pub(crate) fn commit_transport_intent(
        &self,
        prepared: PreparedTransportIntent,
        claim: impl FnOnce() -> bool,
        apply: impl FnOnce(),
    ) -> Result<SlotSnapshot, PlacementError> {
        let write =
            self.prepare_slot_write_fixed(prepared.placement, prepared.placement.lifetime())?;
        self.commit_slot_write_fixed(
            &write,
            || self.transport_intent_current(prepared) && claim(),
            |_| {
                self.apply_transport_intent(prepared);
                apply();
            },
        )
    }
}
