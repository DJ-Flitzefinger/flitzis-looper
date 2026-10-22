//! A pending ordinary origin preserves exact caused-Hold custody, never audio authority.
use super::pair_control::PreparedTransportIntent;
use super::{ContentCell, ContentKey, InputRuntimeOwnership, PlacementError, SlotSnapshot};
use std::sync::atomic::Ordering;

impl InputRuntimeOwnership {
    pub(crate) fn commit_ordinary_transport_intent(
        &self,
        prepared: PreparedTransportIntent,
        claim: impl FnOnce() -> bool,
        enqueue: impl FnOnce() -> bool,
    ) -> Result<SlotSnapshot, PlacementError> {
        let placement = prepared.placement();
        let write = self.prepare_slot_write_fixed(placement, placement.lifetime())?;
        self.commit_slot_write_fixed(
            &write,
            || self.transport_intent_current(prepared) && claim(),
            |_| {
                let pending = &self.pending_ordinary_origins[placement.cell().index()];
                let previous = pending.load(Ordering::Acquire);
                // Publish before the release fence: an earlier queued release or render
                // tail can already see why its exact caused effect must be retained.
                pending.store(prepared.revision(), Ordering::Release);
                self.apply_transport_intent(prepared);
                if !enqueue() {
                    // The request lock reserved producer capacity. Preserve previous
                    // custody even if that reservation unexpectedly fails.
                    let _ = pending.compare_exchange(
                        prepared.revision(),
                        previous,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                }
            },
        )
    }

    pub(crate) fn pending_ordinary_origin(&self, cell: ContentCell) -> u64 {
        self.pending_ordinary_origins[cell.index()].load(Ordering::Acquire)
    }

    /// Only inside the same full-key callback claim as the original disposition.
    /// A newer admission retains custody; lifetime replacement resets the word.
    pub(crate) fn finish_pending_ordinary_origin(&self, key: ContentKey, revision: u64) {
        if self.callback_key_slot(key).is_some() {
            let _ = self.pending_ordinary_origins[key.cell().index()].compare_exchange(
                revision,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
}
