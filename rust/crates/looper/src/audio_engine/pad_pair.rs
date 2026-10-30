//! Internal complete Move/Swap admission, retained guards and its own callback receipt.
//!
//! This module does not expose a public arrangement API or certify control/durable settlement.
//! Stable content owners survive placement; the separate reservation fences new slot admission.

use super::committed_stem_residency::StemModeReceipt;
use super::constant_timing::CurrentTimingAcknowledgements;
use super::constants::{MAX_VOICES, NUM_SAMPLES};
use super::content_placement::{
    ContentKey, PairPlacementKind, PlacementCommit, PlacementError, PreparedPlacement, SlotSnapshot,
};
use super::dsp::PerPadDspChain;
use super::hold_action::{HoldActionState, NativeHoldProjection, NativePauseEffect};
use super::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership, PairControlProjection};
use crate::messages::PreparedStemSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

pub(crate) const MAX_PAIR_SELECTION_FENCES: usize = 16;

/// Original participant evidence. No lookup through a later occupant is permitted at claim.
#[derive(Clone, Debug)]
pub(crate) struct PairParticipantGuard {
    pub(crate) control: PairControlProjection,
    pub(crate) preparation_epoch: Arc<AtomicU64>,
    pub(crate) captured_preparation_epoch: u64,
    pub(crate) selection_fences: Vec<(Arc<AtomicU64>, u64)>,
    pub(crate) timing_acknowledgement: u64,
}

impl PairParticipantGuard {
    pub(crate) fn capture(
        ownership: &InputRuntimeOwnership,
        acknowledgements: &CurrentTimingAcknowledgements,
        placement: SlotSnapshot,
        binding: Option<InputPadBinding>,
        preparation_epoch: Arc<AtomicU64>,
        selection_fences: Vec<(Arc<AtomicU64>, u64)>,
    ) -> Result<Self, PlacementError> {
        if selection_fences.len() > MAX_PAIR_SELECTION_FENCES {
            return Err(PlacementError::CapacityUnavailable);
        }
        let control = ownership.pair_control_projection(placement, binding)?;
        let timing_acknowledgement = placement
            .content_key()
            .map_or(0, |key| acknowledgements.current_for_key(ownership, key));
        if timing_acknowledgement
            != binding
                .and_then(|binding| binding.accepted)
                .map_or(0, |timing| timing.publication_epoch)
        {
            return Err(PlacementError::Stale);
        }
        let captured_preparation_epoch = preparation_epoch.load(Ordering::Acquire);
        if selection_fences
            .iter()
            .any(|(epoch, expected)| epoch.load(Ordering::Acquire) != *expected)
        {
            return Err(PlacementError::Stale);
        }
        Ok(Self {
            control,
            preparation_epoch,
            captured_preparation_epoch,
            selection_fences,
            timing_acknowledgement,
        })
    }

    pub(crate) fn placement(&self) -> SlotSnapshot {
        self.control.placement()
    }

    pub(crate) fn current(
        &self,
        ownership: &InputRuntimeOwnership,
        acknowledgements: &CurrentTimingAcknowledgements,
        reservation: u64,
    ) -> bool {
        ownership.pair_control_projection_current(&self.control, reservation)
            && self.preparation_and_selection_current(ownership, acknowledgements)
    }

    pub(crate) fn current_for_copy(
        &self,
        ownership: &InputRuntimeOwnership,
        acknowledgements: &CurrentTimingAcknowledgements,
        reservation: u64,
    ) -> bool {
        ownership.recovered_copy_control_projection_current(&self.control, reservation)
            && self.preparation_and_selection_current(ownership, acknowledgements)
    }

    fn preparation_and_selection_current(
        &self,
        ownership: &InputRuntimeOwnership,
        acknowledgements: &CurrentTimingAcknowledgements,
    ) -> bool {
        self.preparation_epoch.load(Ordering::Acquire) == self.captured_preparation_epoch
            && self.selection_fences.len() <= MAX_PAIR_SELECTION_FENCES
            && self
                .selection_fences
                .iter()
                .all(|(epoch, expected)| epoch.load(Ordering::Acquire) == *expected)
            && self
                .placement()
                .content_key()
                .map_or(0, |key| acknowledgements.current_for_key(ownership, key))
                == self.timing_acknowledgement
    }
}

/// Complete committed slot projection. Its engine owner is retained by the owning Pair ticket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PairSlotProjection {
    pub(crate) slot: usize,
    pub(crate) cell: usize,
    pub(crate) binding_epoch: u64,
    pub(crate) lifetime: u64,
}

impl PairSlotProjection {
    fn from_snapshot(snapshot: SlotSnapshot) -> Self {
        Self {
            slot: snapshot.slot(),
            cell: snapshot.cell().index(),
            binding_epoch: snapshot.binding_epoch(),
            lifetime: snapshot.lifetime(),
        }
    }

    pub(crate) fn matches(self, snapshot: SlotSnapshot) -> bool {
        self == Self::from_snapshot(snapshot)
    }
}

/// Own complete Native receipt; historical observation never creates new content permission.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PairAcknowledgementSnapshot {
    pub(crate) kind: PairPlacementKind,
    pub(crate) source_after: PairSlotProjection,
    pub(crate) target_after: PairSlotProjection,
    pub(crate) layout_revision: u64,
    pub(crate) participants: [PairControlProjection; 2],
    pub(crate) preparation_epochs: [u64; 2],
    pub(crate) timing_acknowledgements: [u64; 2],
    pub(crate) holds: [Option<NativeHoldProjection>; 2],
}

/// Preallocated, single-write fixed receipt storage. Phase Release/Acquire publishes all fields.
struct PairHoldAcknowledgement {
    present: AtomicBool,
    action_id: AtomicU64,
    transport_revision: AtomicU64,
    physical_press_id: AtomicU64,
    state: AtomicU8,
    release_requested: AtomicBool,
    release_admitted: AtomicBool,
    effect_current: AtomicBool,
    effect_present: AtomicBool,
    effect_id: AtomicU64,
    effect_transport_revision: AtomicU64,
    cohort_mask: AtomicU32,
    changed_mask: AtomicU32,
    generations: [AtomicU64; MAX_VOICES],
    paused_at_output_frame: AtomicU64,
    settled_at_output_frame: AtomicU64,
}

impl Default for PairHoldAcknowledgement {
    fn default() -> Self {
        Self {
            present: AtomicBool::new(false),
            action_id: AtomicU64::new(0),
            transport_revision: AtomicU64::new(0),
            physical_press_id: AtomicU64::new(0),
            state: AtomicU8::new(0),
            release_requested: AtomicBool::new(false),
            release_admitted: AtomicBool::new(false),
            effect_current: AtomicBool::new(false),
            effect_present: AtomicBool::new(false),
            effect_id: AtomicU64::new(0),
            effect_transport_revision: AtomicU64::new(0),
            cohort_mask: AtomicU32::new(0),
            changed_mask: AtomicU32::new(0),
            generations: std::array::from_fn(|_| AtomicU64::new(0)),
            paused_at_output_frame: AtomicU64::new(0),
            settled_at_output_frame: AtomicU64::new(0),
        }
    }
}

impl PairHoldAcknowledgement {
    fn publish(&self, projection: Option<NativeHoldProjection>) {
        let Some(projection) = projection else {
            return;
        };
        self.action_id
            .store(projection.action_id, Ordering::Relaxed);
        self.transport_revision
            .store(projection.transport_revision, Ordering::Relaxed);
        self.physical_press_id
            .store(projection.physical_press_id, Ordering::Relaxed);
        self.state.store(projection.state as u8, Ordering::Relaxed);
        self.release_requested
            .store(projection.release_requested, Ordering::Relaxed);
        self.release_admitted
            .store(projection.release_admitted, Ordering::Relaxed);
        self.effect_current
            .store(projection.effect_current, Ordering::Relaxed);
        self.settled_at_output_frame
            .store(projection.settled_at_output_frame, Ordering::Relaxed);
        if let Some(effect) = projection.effect {
            self.effect_id.store(effect.effect_id, Ordering::Relaxed);
            self.effect_transport_revision
                .store(effect.transport_revision, Ordering::Relaxed);
            self.cohort_mask
                .store(effect.cohort_mask, Ordering::Relaxed);
            self.changed_mask
                .store(effect.changed_mask, Ordering::Relaxed);
            for (stored, generation) in self.generations.iter().zip(effect.generations) {
                stored.store(generation, Ordering::Relaxed);
            }
            self.paused_at_output_frame
                .store(effect.paused_at_output_frame, Ordering::Relaxed);
            self.effect_present.store(true, Ordering::Relaxed);
        }
        self.present.store(true, Ordering::Relaxed);
    }

    fn snapshot(&self, key: Option<ContentKey>) -> Option<NativeHoldProjection> {
        let key = key?;
        if !self.present.load(Ordering::Relaxed) {
            return None;
        }
        let action_id = self.action_id.load(Ordering::Relaxed);
        let state = match self.state.load(Ordering::Relaxed) {
            0 => HoldActionState::Prepared,
            1 => HoldActionState::Admitted,
            2 => HoldActionState::PausedOwned,
            3 => HoldActionState::NoEffect,
            4 => HoldActionState::Resumed,
            5 => HoldActionState::Retired,
            6 => HoldActionState::Superseded,
            7 => HoldActionState::Exhausted,
            _ => HoldActionState::Rejected,
        };
        Some(NativeHoldProjection {
            key,
            action_id,
            transport_revision: self.transport_revision.load(Ordering::Relaxed),
            physical_press_id: self.physical_press_id.load(Ordering::Relaxed),
            state,
            release_requested: self.release_requested.load(Ordering::Relaxed),
            release_admitted: self.release_admitted.load(Ordering::Relaxed),
            effect_current: self.effect_current.load(Ordering::Relaxed),
            effect: self
                .effect_present
                .load(Ordering::Relaxed)
                .then(|| NativePauseEffect {
                    key,
                    action_id,
                    effect_id: self.effect_id.load(Ordering::Relaxed),
                    transport_revision: self.effect_transport_revision.load(Ordering::Relaxed),
                    cohort_mask: self.cohort_mask.load(Ordering::Relaxed),
                    changed_mask: self.changed_mask.load(Ordering::Relaxed),
                    generations: std::array::from_fn(|index| {
                        self.generations[index].load(Ordering::Relaxed)
                    }),
                    paused_at_output_frame: self.paused_at_output_frame.load(Ordering::Relaxed),
                }),
            settled_at_output_frame: self.settled_at_output_frame.load(Ordering::Relaxed),
        })
    }
}

#[derive(Default)]
struct PairAcknowledgement {
    source_binding_epoch: AtomicU64,
    target_binding_epoch: AtomicU64,
    layout_revision: AtomicU64,
    holds: [PairHoldAcknowledgement; 2],
}

/// Both original guards and every actual owner remain pinned until outer settlement.
#[derive(Clone)]
pub(crate) struct PairRtGuard {
    pub(crate) participants: [PairParticipantGuard; 2],
    pub(crate) ownership: Arc<InputRuntimeOwnership>,
    pub(crate) acknowledgements: Arc<CurrentTimingAcknowledgements>,
    pub(crate) reservation: u64,
    phase: Arc<AtomicU8>,
    receipt: Arc<PairAcknowledgement>,
}

impl std::fmt::Debug for PairRtGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PairRtGuard")
            .field("participants", &self.participants)
            .field("reservation", &self.reservation)
            .field("phase", &self.phase.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl PairRtGuard {
    pub(crate) fn new(
        participants: [PairParticipantGuard; 2],
        ownership: Arc<InputRuntimeOwnership>,
        acknowledgements: Arc<CurrentTimingAcknowledgements>,
        reservation: u64,
    ) -> Result<Self, PlacementError> {
        let [source, target] = participants
            .each_ref()
            .map(|participant| participant.placement());
        if source.slot() >= NUM_SAMPLES || target.slot() >= NUM_SAMPLES || reservation == 0 {
            return Err(PlacementError::InvalidSlot);
        }
        if source.slot() == target.slot() {
            return Err(PlacementError::SameSlot);
        }
        if source.lifetime() == 0 {
            return Err(PlacementError::EmptySource);
        }
        Ok(Self {
            participants,
            ownership,
            acknowledgements,
            reservation,
            phase: Arc::new(AtomicU8::new(0)),
            receipt: Arc::new(PairAcknowledgement::default()),
        })
    }

    pub(crate) fn phase(&self) -> u8 {
        self.phase.load(Ordering::Acquire)
    }

    /// The caller owns the publication marker; only original direct cell fields are compared.
    pub(crate) fn stable_current(&self) -> bool {
        self.reservation != 0
            && self.participants.iter().all(|participant| {
                participant.current(&self.ownership, &self.acknowledgements, self.reservation)
            })
    }

    pub(crate) fn claim_stable(&self) -> bool {
        // Shared publication ownership prevents further control writers after this final check.
        // The callback is the sole audio/Hold writer. Nothing fallible follows a successful CAS.
        self.stable_current()
            && self
                .phase
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    pub(crate) fn reject(&self) -> bool {
        self.phase
            .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn acknowledge(
        &self,
        committed: PlacementCommit,
        holds: [Option<NativeHoldProjection>; 2],
    ) {
        self.receipt
            .source_binding_epoch
            .store(committed.source_after.binding_epoch(), Ordering::Relaxed);
        self.receipt
            .target_binding_epoch
            .store(committed.target_after.binding_epoch(), Ordering::Relaxed);
        self.receipt
            .layout_revision
            .store(committed.layout_revision, Ordering::Relaxed);
        for (receipt, hold) in self.receipt.holds.iter().zip(holds) {
            receipt.publish(hold);
        }
        self.phase.store(2, Ordering::Release);
    }

    pub(crate) fn native_acknowledgement(&self) -> Option<PairAcknowledgementSnapshot> {
        if self.phase() != 2 {
            return None;
        }
        let [source, target] = self
            .participants
            .each_ref()
            .map(|participant| participant.placement());
        Some(PairAcknowledgementSnapshot {
            kind: if target.lifetime() == 0 {
                PairPlacementKind::Move
            } else {
                PairPlacementKind::Swap
            },
            source_after: PairSlotProjection {
                slot: source.slot(),
                cell: target.cell().index(),
                binding_epoch: self.receipt.source_binding_epoch.load(Ordering::Relaxed),
                lifetime: target.lifetime(),
            },
            target_after: PairSlotProjection {
                slot: target.slot(),
                cell: source.cell().index(),
                binding_epoch: self.receipt.target_binding_epoch.load(Ordering::Relaxed),
                lifetime: source.lifetime(),
            },
            layout_revision: self.receipt.layout_revision.load(Ordering::Relaxed),
            participants: self
                .participants
                .each_ref()
                .map(|participant| participant.control),
            preparation_epochs: self
                .participants
                .each_ref()
                .map(|participant| participant.captured_preparation_epoch),
            timing_acknowledgements: self
                .participants
                .each_ref()
                .map(|participant| participant.timing_acknowledgement),
            holds: std::array::from_fn(|index| {
                self.receipt.holds[index]
                    .snapshot(self.participants[index].placement().content_key())
            }),
        })
    }
}

/// Constructed off RT. Swapping this chain installs independent canonical empty history/ramps.
#[derive(Clone, Debug)]
pub(crate) struct PreparedPairEmptyImage {
    pub(crate) sample_rate_hz: u32,
    pub(crate) channels: usize,
    pub(crate) dsp_chain: PerPadDspChain,
}

impl PreparedPairEmptyImage {
    pub(crate) fn new(
        source: usize,
        sample_rate_hz: u32,
        channels: usize,
    ) -> Result<Self, PlacementError> {
        if source >= NUM_SAMPLES || sample_rate_hz == 0 || !(1..=2).contains(&channels) {
            return Err(PlacementError::InvalidSlot);
        }
        Ok(Self {
            sample_rate_hz,
            channels,
            dsp_chain: PerPadDspChain::new(source, sample_rate_hz as f32, channels),
        })
    }
}

/// One queue/retirement owner. All displaced empty-slot owners leave the callback with it.
#[derive(Clone, Debug)]
pub(crate) struct PadPairRtWork {
    pub(crate) prepared: Arc<PreparedPlacement>,
    pub(crate) guard: Arc<PairRtGuard>,
    pub(crate) empty: PreparedPairEmptyImage,
    pub(crate) retired_stems: Option<PreparedStemSet>,
    pub(crate) retired_outgoing: Option<PreparedStemSet>,
    pub(crate) retired_receipt: Option<StemModeReceipt>,
}

impl PadPairRtWork {
    pub(crate) fn new(
        guard: PairRtGuard,
        sample_rate_hz: u32,
        channels: usize,
    ) -> Result<Box<Self>, PlacementError> {
        let snapshots = guard
            .participants
            .each_ref()
            .map(|participant| participant.placement());
        if !guard.stable_current() {
            return Err(PlacementError::Stale);
        }
        let current = snapshots.map(|snapshot| {
            guard
                .ownership
                .slot_snapshot(snapshot.slot())
                .filter(|current| current.same_projection(snapshot))
        });
        let [Some(source), Some(target)] = current else {
            return Err(PlacementError::Pending);
        };
        let prepared = guard.ownership.prepare_placement_pair(source, target)?;
        let empty = PreparedPairEmptyImage::new(source.slot(), sample_rate_hz, channels)?;
        Ok(Box::new(Self {
            prepared: Arc::new(prepared),
            guard: Arc::new(guard),
            empty,
            retired_stems: None,
            retired_outgoing: None,
            retired_receipt: None,
        }))
    }

    #[cfg(test)]
    pub(crate) fn kind(&self) -> PairPlacementKind {
        self.prepared.kind()
    }
}

pub(crate) mod control;
pub(crate) mod recovery;
pub use control::{CapturedPadPair, PadPairAcknowledgement, PadPairTicket};
#[cfg(test)]
pub(crate) use control::{cancel, capture, prepare, publish, reconcile};

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "pad_pair/control_hold_tests.rs"]
mod control_hold_tests;
