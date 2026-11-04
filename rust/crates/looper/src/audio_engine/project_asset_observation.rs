//! Scalar test diagnostics without pruning or extending any asset lifetime.
use super::*;
use std::sync::TryLockError;

#[derive(Debug, Serialize)]
pub(in crate::audio_engine) struct RegistryObservation {
    reader_limit: usize,
    reader_records: usize,
    reserved_reader_slots: usize,
    available_reader_slots: usize,
    native_live: usize,
    pending_without_native: usize,
    saved_cold_file_only: usize,
    other_without_native_or_pending: usize,
    pending_assignments: usize,
    saved_claims: usize,
    cold_descriptors: usize,
    live_engine_witnesses: usize,
    owner_records: usize,
    live_owner_witnesses: usize,
    history_books: usize,
    live_history_witnesses: usize,
    retirements: usize,
    residency: super::super::resident_accounting::ResidentLedgerObservation,
}

impl ProjectAssets {
    /// Observe existing records only. A busy or poisoned gate is not an empty
    /// registry. In particular, do not call the pruning budget-status route.
    pub(in crate::audio_engine) fn observe_registry_for_test(
        &self,
    ) -> Result<RegistryObservation, &'static str> {
        let state = self.state.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => "asset gate busy",
            TryLockError::Poisoned(_) => "asset gate poisoned",
        })?;
        let mut observation = RegistryObservation {
            reader_limit: MAX_READER_RECORDS,
            reader_records: state.readers.len(),
            reserved_reader_slots: state.residency.reserved_reader_slots(),
            available_reader_slots: Self::available_reader_slots(&state),
            native_live: 0,
            pending_without_native: 0,
            saved_cold_file_only: 0,
            other_without_native_or_pending: 0,
            pending_assignments: 0,
            saved_claims: 0,
            cold_descriptors: 0,
            live_engine_witnesses: 0,
            owner_records: state.owners.len(),
            live_owner_witnesses: state
                .owners
                .iter()
                .filter(|owner| owner.strong_count() > 0)
                .count(),
            history_books: state.histories.len(),
            live_history_witnesses: state
                .histories
                .iter()
                .filter(|history| history.strong_count() > 0)
                .count(),
            retirements: state.retiring.len(),
            residency: state.residency.observe_for_test(),
        };
        for reader in &state.readers {
            // These four categories are disjoint. The last category is not
            // "dead": a real owner can still protect shared paired files.
            if reader.pcm.iter().any(|pcm| pcm.strong_count() > 0) || reader.shared_native_live() {
                observation.native_live += 1;
            } else if reader.pending_assignment {
                observation.pending_without_native += 1;
            } else if reader.saved_claim && reader.cold.is_some() {
                observation.saved_cold_file_only += 1;
            } else {
                observation.other_without_native_or_pending += 1;
            }
            observation.pending_assignments += usize::from(reader.pending_assignment);
            observation.saved_claims += usize::from(reader.saved_claim);
            observation.cold_descriptors += usize::from(reader.cold.is_some());
            observation.live_engine_witnesses += usize::from(reader.engine.strong_count() > 0);
        }
        // No Weak upgrade, Arc clone, collection, pruning, filesystem access or
        // logging occurs under the gate. Only scalar values leave this scope.
        Ok(observation)
    }
}
