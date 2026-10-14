//! Content copies: sealed original participants and a distinct callback receipt.
//! Disk/evidence/assignment owners never enter the realtime payload.

use super::committed_stem_residency::{SelectedStemIdentity, StemModeReceipt};
use super::constant_timing::{AcceptedTimingProjection, CurrentTimingAcknowledgements};
use super::content_placement::{ContentKey, SlotSnapshot};
use super::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
use super::prepared_source::PreparedSourcePermit;
use crate::messages::{PreparedStemSet, SampleBuffer, StemMixMode};
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU64, Ordering},
};

/// A complete stopped image, already validated and converted off-thread.
#[derive(Debug, Clone, Copy)]
pub(super) struct CopyRtIntent {
    pub(super) period_seconds: Option<f64>,
    pub(super) origin_frame: f64,
    pub(super) loop_start_frame: usize,
    pub(super) loop_end_frame: Option<usize>,
    pub(super) key_lock: bool,
    pub(super) timing_intent: TimingIntent,
    pub(super) stem_mix_mode: StemMixMode,
    pub(super) source_version_hash: u64,
    pub(super) enabled_mask: u8,
    pub(super) gain_db: f32,
    pub(super) eq_low_db: f32,
    pub(super) eq_mid_db: f32,
    pub(super) eq_high_db: f32,
}

/// Pending reservations change no source, authority, request or launch epoch.
#[derive(Clone)]
pub(super) struct CopyRtGuard {
    pub(super) source: usize,
    pub(super) target: usize,
    pub(super) source_binding: InputPadBinding,
    pub(super) source_generation: u64,
    pub(super) source_epoch: Arc<AtomicU64>,
    pub(super) captured_source_epoch: u64,
    pub(super) source_selection_fences: Option<[(Arc<AtomicU64>, u64); 2]>,
    pub(super) target_epoch: Arc<AtomicU64>,
    pub(super) captured_target_epoch: u64,
    pub(super) target_authority: u64,
    pub(super) target_runtime: u64,
    pub(super) target_placement: SlotSnapshot,
    pub(super) target_binding: Option<InputPadBinding>,
    pub(super) target_source_generation: u64,
    pub(super) target_selection_fences:
        [Option<(Arc<AtomicU64>, u64)>; super::pad_pair::MAX_PAIR_SELECTION_FENCES],
    pub(super) reservation: u64,
    pub(super) phase: Arc<AtomicU8>,
    pub(super) ownership: Arc<InputRuntimeOwnership>,
    pub(super) acknowledgements: Arc<CurrentTimingAcknowledgements>,
}

impl std::fmt::Debug for CopyRtGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CopyRtGuard")
            .field("source", &self.source)
            .field("target", &self.target)
            .field("reservation", &self.reservation)
            .field("phase", &self.phase.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl CopyRtGuard {
    fn target_fields_current(&self) -> bool {
        self.target_selection_fences.iter().all(|fence| {
            fence
                .as_ref()
                .is_none_or(|(epoch, expected)| epoch.load(Ordering::Acquire) == *expected)
        }) && match self.target_binding {
            Some(binding) => {
                self.ownership.current(self.target, binding)
                    && self
                        .ownership
                        .binding_source_generation(self.target, binding)
                        == Some(self.target_source_generation)
                    && self.acknowledgements.current_for_binding(
                        &self.ownership,
                        self.target,
                        binding,
                    ) == binding
                        .accepted
                        .map_or(0, |projection| projection.publication_epoch)
            }
            None => {
                self.target_placement.lifetime() == 0
                    && self
                        .ownership
                        .source_cell_empty(self.target_placement.cell())
            }
        }
    }

    pub(super) fn stable_current(&self) -> bool {
        let cell = self.target_placement.cell();
        self.ownership
            .slot_snapshot_fields_current(self.target_placement)
            && self
                .source_binding
                .placement
                .is_some_and(|snapshot| self.ownership.slot_snapshot_fields_current(snapshot))
            && self.ownership.copy_reservation(self.target) == self.reservation
            && self.target_fields_current()
            && self
                .ownership
                .authority_for_cell(cell)
                .load(Ordering::Acquire)
                == self.target_authority
            && self
                .ownership
                .runtime_for_cell(cell)
                .load(Ordering::Acquire)
                == self.target_runtime
            && self.target_epoch.load(Ordering::Acquire) == self.captured_target_epoch
            && self.source_epoch.load(Ordering::Acquire) == self.captured_source_epoch
            && self.source_selection_fences.as_ref().is_none_or(|fences| {
                fences
                    .iter()
                    .all(|(epoch, expected)| epoch.load(Ordering::Acquire) == *expected)
            })
            && self.ownership.current(self.source, self.source_binding)
            && self
                .ownership
                .binding_source_generation(self.source, self.source_binding)
                == Some(self.source_generation)
            && self.acknowledgements.current_for_binding(
                &self.ownership,
                self.source,
                self.source_binding,
            ) == self
                .source_binding
                .accepted
                .map_or(0, |projection| projection.publication_epoch)
    }
    pub(super) fn claim_stable(&self) -> bool {
        if !self.stable_current()
            || self
                .phase
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return false;
        }
        if !self.stable_current() {
            self.phase.store(3, Ordering::Release);
            return false;
        }
        true
    }
    pub(super) fn current(&self) -> bool {
        self.ownership.copy_reservation(self.target) == self.reservation
            && self.ownership.slot_snapshot_current(self.target_placement)
            && self.target_fields_current()
            && self.ownership.authority_revision(self.target) == self.target_authority
            && self.ownership.runtime_revision(self.target) == self.target_runtime
            && self.target_epoch.load(Ordering::Acquire) == self.captured_target_epoch
            && self.source_epoch.load(Ordering::Acquire) == self.captured_source_epoch
            && self.source_selection_fences.as_ref().is_none_or(|fences| {
                fences
                    .iter()
                    .all(|(epoch, expected)| epoch.load(Ordering::Acquire) == *expected)
            })
            && self
                .ownership
                .admission_current(self.source, self.source_binding)
            && self
                .ownership
                .binding_source_generation(self.source, self.source_binding)
                == Some(self.source_generation)
            && self.acknowledgements.current_for_binding(
                &self.ownership,
                self.source,
                self.source_binding,
            ) == self
                .source_binding
                .accepted
                .map_or(0, |projection| projection.publication_epoch)
    }

    #[cfg(test)]
    pub(super) fn claim(&self) -> bool {
        if !self.current()
            || self
                .phase
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return false;
        }
        // A terminal callback rejection proves no installation occurred. A mere
        // timeout while phase 1 remains visible never grants retirement authority.
        if !self.current() {
            self.phase.store(3, Ordering::Release);
            return false;
        }
        true
    }

    pub(super) fn reject(&self) {
        let _ = self
            .phase
            .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire);
    }

    pub(super) fn acknowledge(&self) {
        self.phase.store(2, Ordering::Release);
    }
}

#[derive(Debug, Clone)]
pub(super) struct CopySelectionFences {
    pub(super) selection: Arc<AtomicU64>,
    pub(super) registration: Arc<AtomicU64>,
    pub(super) expected: u64,
    pub(super) publication: PreparedSourcePermit,
}

#[derive(Debug, Clone)]
pub(crate) struct PadCopyRtWork {
    pub(super) content_key: ContentKey,
    pub(super) target: usize,
    pub(super) sample: SampleBuffer,
    pub(super) publication: PreparedSourcePermit,
    pub(super) stems: Option<PreparedStemSet>,
    pub(super) selection: Option<SelectedStemIdentity>,
    pub(super) selection_fences: Option<CopySelectionFences>,
    pub(super) projection: Option<AcceptedTimingProjection>,
    pub(super) source_generation: u64,
    pub(super) content_lifetime: u64,
    pub(super) prepared_epoch: u64,
    pub(super) authority_revision: u64,
    pub(super) guard: CopyRtGuard,
    pub(super) intent: CopyRtIntent,
    // Displaced bank and stem readers retire with this whole payload.
    pub(super) retired_sample: Option<SampleBuffer>,
    pub(super) retired_receipt: Option<StemModeReceipt>,
    pub(super) retired_stems: Option<PreparedStemSet>,
    pub(super) retired_outgoing: Option<PreparedStemSet>,
}

impl PadCopyRtWork {
    pub(crate) fn retirement_slots_needed(&self) -> usize {
        if self.guard.target_binding.is_some() {
            // Each old voice owns input plus up to two frozen stem readers;
            // the old Hold and this aggregate each need their own retirement entry.
            3 * super::constants::MAX_VOICES + 2
        } else {
            1
        }
    }
}

mod control;
mod intent;
pub use control::{CapturedPadCopy, PadCopyTicket};
pub(super) use control::{
    PadCopyState, cancel, capture, finish, prepare, prepare_detached, publish, reconcile,
};

#[cfg(all(test, windows))]
mod public_complete_tests;

#[cfg(all(test, windows))]
mod qm_complete_tests;
