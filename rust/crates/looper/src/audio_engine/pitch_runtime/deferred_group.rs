//! A later retry of the same due event, with every Work already in group custody.

use super::*;
use crate::audio_engine::ordinary_start::OrdinaryCompletionPlan;

#[cfg(all(test, windows))]
#[path = "deferred_group_tests.rs"]
mod tests;

impl PitchRuntime {
    fn deferred_group_metadata_current(
        &self,
        index: usize,
        group: &OrdinaryStartGroup,
        expected: u64,
        execution: u64,
    ) -> bool {
        group.slots[0] == index
            && self.slots[index].planned == expected
            && group.slots[..group.len]
                .iter()
                .enumerate()
                .all(|(member, index)| {
                    let slot = &self.slots[*index];
                    let Some(work) = group.work[member].as_ref() else {
                        return false;
                    };
                    slot.work.is_none()
                        && slot.independent_birth
                        && slot.sequence == work.sequence()
                        && slot.original == work.origin().target_frame().unwrap_or(u64::MAX)
                        && slot.content == Some(work.origin().content_key())
                        && slot
                            .epoch
                            .as_ref()
                            .is_some_and(|epoch| Arc::ptr_eq(epoch, &work.epoch))
                        && matches!(&slot.owner, Some(OriginalActionOwner::Ordinary(owner))
                        if owner.same_owner(work.origin()))
                        && slot.planned == expected
                        && work.execution_frame() == expected
                        && slot.replans < MAX_LATE_REPLANS
                        && execution
                            .checked_sub(slot.original)
                            .is_some_and(|delay| delay <= MAX_LAUNCH_DELAY)
                        && (slot.replans == 0 || work.late_completion_prepared_for(expected))
                })
    }

    /// Plans may refuse and leave the exact legacy path intact. Once binding
    /// begins, any lost member CAS rejects the entire group before audio stores.
    pub(super) fn bind_deferred_group(
        &mut self,
        mixer: &mut RtMixer,
        index: usize,
        group: &mut OrdinaryStartGroup,
        expected: u64,
        execution: u64,
    ) -> Result<bool, PitchActionReject> {
        if !self.deferred_group_metadata_current(index, group, expected, execution) {
            return Ok(false);
        }
        let key = OrdinaryGroupKey {
            slot: index,
            action_sequence: self.slots[index].sequence,
        };
        if !mixer.group_capture_current(&group.batch, key, execution) {
            return Ok(false);
        }
        let mut plans: [Option<OrdinaryCompletionPlan>; super::super::constants::MAX_VOICES] =
            std::array::from_fn(|_| None);
        for member in 0..group.len {
            let work = group.work[member].as_ref().unwrap();
            #[cfg(all(test, windows))]
            super::super::ordinary_start_tests::preparation_deadline_tests::before_group_late_plan(
                work,
            );
            let Ok(plan) = mixer.plan_ordinary_deferred_completion(work, execution, execution)
            else {
                return Ok(false);
            };
            plans[member] = Some(plan);
        }
        if !mixer.group_capture_current(&group.batch, key, execution) {
            return Ok(false);
        }
        for (member, plan) in plans.iter_mut().enumerate().take(group.len) {
            #[cfg(all(test, windows))]
            tests::before_bind(member);
            group.work[member]
                .as_mut()
                .unwrap()
                .bind_deferred_completion(plan.take().unwrap())?;
        }
        // This is the exact already-popped deferred event. Do not reinsert it,
        // grant a queued-completion receipt or release any reserved capacity.
        mixer.replan_group_capture(&group.batch, key, execution);
        for index in &group.slots[..group.len] {
            self.slots[*index].planned = execution;
            self.slots[*index].replans += 1;
        }
        Ok(true)
    }
}
