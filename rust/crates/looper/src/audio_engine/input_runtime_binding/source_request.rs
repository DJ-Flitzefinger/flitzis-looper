//! Stable request intent, independent of physical slot counters and resident windows.

use super::{
    ContentKey, InputPadBinding, InputRuntimeOwnership, PlacementError, PlacementObservation,
    SlotSnapshot, SourcePublicationError,
};
use std::sync::atomic::{AtomicU64, Ordering};

/// Prepared under the existing request mutex before any accepted intent is published.
/// Dropping a rejected admission changes nothing. The same mutex serializes commit.
pub(crate) struct LogicalSourceRequestAdvance<'a> {
    current: &'a AtomicU64,
    next: u64,
}

impl LogicalSourceRequestAdvance<'_> {
    pub(crate) fn commit(self) {
        self.current.store(self.next, Ordering::Release);
    }
}

impl InputRuntimeOwnership {
    /// Retain the caller's original fixed-slot admission through publication contention.
    pub(crate) fn prepare_source_request_advance_for_placement(
        &self,
        placement: SlotSnapshot,
    ) -> Result<LogicalSourceRequestAdvance<'_>, SourcePublicationError> {
        self.ensure_no_copy_reservation(placement.slot())
            .map_err(|error| error.to_string())?;
        match self.observe_slot_snapshot(placement) {
            PlacementObservation::Current(_) => {}
            PlacementObservation::Pending => return Err(PlacementError::Pending.into()),
            PlacementObservation::Stale => return Err(PlacementError::Stale.into()),
            PlacementObservation::Retired => return Err(PlacementError::Retired.into()),
        }
        let current = &self.source_requests[placement.cell().index()];
        let next = current
            .load(Ordering::Acquire)
            .checked_add(1)
            .ok_or("logical source request incarnation exhausted")?;
        Ok(LogicalSourceRequestAdvance { current, next })
    }

    pub(crate) fn prepare_source_request_advance(
        &self,
        id: usize,
    ) -> Result<LogicalSourceRequestAdvance<'_>, String> {
        self.ensure_no_copy_reservation(id)
            .map_err(|error| error.to_string())?;
        let placement = self
            .slot_snapshot(id)
            .ok_or("source request placement is pending or unavailable")?;
        let current = &self.source_requests[placement.cell().index()];
        let next = current
            .load(Ordering::Acquire)
            .checked_add(1)
            .ok_or("logical source request incarnation exhausted")?;
        Ok(LogicalSourceRequestAdvance { current, next })
    }

    pub(crate) fn binding_source_request_incarnation(
        &self,
        id: usize,
        binding: InputPadBinding,
    ) -> Option<u64> {
        let cell = self.binding_cell(id, binding)?;
        Some(self.source_requests[cell.index()].load(Ordering::Acquire))
    }

    pub(crate) fn key_source_request_incarnation(&self, key: ContentKey) -> Option<u64> {
        self.content_key_current(key)
            .then(|| self.source_requests[key.cell().index()].load(Ordering::Acquire))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_and_exhausted_request_admissions_do_not_mutate_incarnation() {
        let ownership = InputRuntimeOwnership::default();
        let before = ownership.source_requests[0].load(Ordering::Acquire);
        drop(ownership.prepare_source_request_advance(0).unwrap());
        assert_eq!(ownership.source_requests[0].load(Ordering::Acquire), before);
        ownership
            .prepare_source_request_advance(0)
            .unwrap()
            .commit();
        assert_eq!(
            ownership.source_requests[0].load(Ordering::Acquire),
            before + 1
        );
        ownership.source_requests[0].store(u64::MAX, Ordering::Release);
        assert!(ownership.prepare_source_request_advance(0).is_err());
        assert_eq!(
            ownership.source_requests[0].load(Ordering::Acquire),
            u64::MAX
        );
    }
}
