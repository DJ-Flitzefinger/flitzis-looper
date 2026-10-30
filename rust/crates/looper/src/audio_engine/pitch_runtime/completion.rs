//! Optional early completion, with no unfinished worker in the deferred-due cell.

use super::*;
use crate::audio_engine::ordinary_start::OrdinaryCompletionPlan;

#[derive(Clone, Copy)]
struct CompletionReceipt {
    header: usize,
    sequence: u64,
    execution: u64,
}

#[cfg(all(test, windows))]
#[path = "completion_tests.rs"]
mod tests;

impl PitchRuntime {
    fn completion_bundle_current(slot: &ActionSlot) -> bool {
        slot.replans == 0
            || match slot.work.as_ref() {
                Some(PreparedActionWork::Ordinary(work)) => {
                    work.late_completion_prepared_for(slot.planned)
                }
                Some(PreparedActionWork::Pitch(work)) => {
                    work.late_completion_prepared_for(slot.planned)
                }
                None => false,
            }
    }

    pub(super) fn accelerate_completed_starts<const CAPACITY: usize>(
        &mut self,
        mixer: &mut RtMixer,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        callback: u64,
    ) {
        // Slot reuse need not follow original input order. Visit each reserved
        // slot once in chronological order, using only this pass's fresh binds.
        let mut visited = [false; MAX_PITCH_ACTIONS];
        let mut receipts = [None; MAX_PITCH_ACTIONS];
        for _ in 0..self.slots.len() {
            let Some(index) = (0..self.slots.len())
                .filter(|index| !visited[*index] && self.slots[*index].sequence != 0)
                .min_by_key(|index| (self.slots[*index].original, self.slots[*index].sequence))
            else {
                break;
            };
            visited[index] = true;
            let slot = &self.slots[index];
            let mut execution = slot.original.max(callback);
            if slot.owner.is_none()
                || !Self::completion_bundle_current(slot)
                || execution >= slot.planned
                || !scheduler.has_queued_pitch_action(index, slot.sequence, slot.planned)
            {
                continue;
            }
            if slot.group.is_some() {
                let group = slot.group.as_ref().unwrap();
                let mut eligible = true;
                for member in &group.slots[..group.len] {
                    match self.completion_execution(*member, execution, &receipts) {
                        Some(frame) => execution = frame,
                        None => {
                            eligible = false;
                            break;
                        }
                    }
                }
                if !eligible
                    || execution >= slot.planned
                    || !group.slots[..group.len].iter().all(|member| {
                        self.completion_predecessors_queued(
                            *member, index, execution, &receipts, scheduler,
                        )
                    })
                {
                    continue;
                }
                if self.accelerate_group(mixer, scheduler, index, execution, callback) {
                    let group = self.slots[index].group.as_ref().unwrap();
                    for member in &group.slots[..group.len] {
                        receipts[*member] = Some(CompletionReceipt {
                            header: index,
                            sequence: self.slots[index].sequence,
                            execution,
                        });
                    }
                }
                continue;
            }
            // A live predecessor is eligible only after its exact fresh bind in
            // this pass. Retain captured spacing rather than collapse originals.
            let Some(execution) = self.completion_execution(index, execution, &receipts) else {
                continue;
            };
            if execution >= slot.planned
                || !self
                    .completion_predecessors_queued(index, index, execution, &receipts, scheduler)
            {
                continue;
            }
            let Some(work) = self.slots[index].work.as_mut() else {
                continue;
            };
            let result = match work {
                PreparedActionWork::Ordinary(work) => {
                    let Ok(plan) = mixer.plan_ordinary_early_completion(work, execution, callback)
                    else {
                        continue;
                    };
                    work.bind_early_completion(plan)
                }
                PreparedActionWork::Pitch(work) => {
                    let Ok(plan) = mixer.plan_pitch_early_completion(work, execution, callback)
                    else {
                        continue;
                    };
                    work.bind_early_completion(plan)
                }
            };
            if let Err(reason) = result {
                work.reject(reason);
                continue;
            }
            let slot = &mut self.slots[index];
            let moved = scheduler.retarget_ready_pitch_action(
                index,
                slot.sequence,
                slot.planned,
                execution,
            );
            assert!(moved, "single callback retains the exact queued original");
            slot.planned = execution;
            receipts[index] = Some(CompletionReceipt {
                header: index,
                sequence: slot.sequence,
                execution,
            });
        }
    }

    fn completion_execution(
        &self,
        index: usize,
        mut execution: u64,
        receipts: &[Option<CompletionReceipt>; MAX_PITCH_ACTIONS],
    ) -> Option<u64> {
        let current = &self.slots[index];
        if !current.independent_birth {
            return Some(execution);
        }
        for (previous_index, previous) in self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, previous)| previous.precedes_endpoint(current))
        {
            let receipt = receipts[previous_index]?;
            if previous.owner.is_none()
                || previous.planned != receipt.execution
                || self.slots[receipt.header].sequence != receipt.sequence
                || self.slots[receipt.header].planned != receipt.execution
            {
                return None;
            }
            execution = execution.max(
                receipt
                    .execution
                    .checked_add(current.original.checked_sub(previous.original)?)?,
            );
        }
        Some(execution)
    }

    fn completion_predecessors_queued<const CAPACITY: usize>(
        &self,
        index: usize,
        header: usize,
        execution: u64,
        receipts: &[Option<CompletionReceipt>; MAX_PITCH_ACTIONS],
        scheduler: &FixedCapacityScheduler<CAPACITY>,
    ) -> bool {
        let current = &self.slots[index];
        !current.independent_birth
            || self
                .slots
                .iter()
                .enumerate()
                .filter(|(_, previous)| previous.precedes_endpoint(current))
                .all(|(previous, _)| {
                    receipts[previous].is_some_and(|receipt| {
                        scheduler.queued_pitch_action_precedes(
                            (receipt.header, receipt.sequence, receipt.execution),
                            (
                                header,
                                self.slots[header].sequence,
                                self.slots[header].planned,
                            ),
                            execution,
                        )
                    })
                })
    }

    fn accelerate_group<const CAPACITY: usize>(
        &mut self,
        mixer: &mut RtMixer,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        index: usize,
        execution: u64,
        callback: u64,
    ) -> bool {
        let group = self.slots[index].group.as_ref().unwrap();
        let key = OrdinaryGroupKey {
            slot: index,
            action_sequence: self.slots[index].sequence,
        };
        // The original capture, including its immutable admitted STOP, must
        // still own this group before any member request or E can change.
        if !mixer.group_capture_current(&group.batch, key, execution) {
            return false;
        }
        let mut plans: [Option<OrdinaryCompletionPlan>; super::super::constants::MAX_VOICES] =
            std::array::from_fn(|_| None);
        for (member, slot_index) in group.slots[..group.len].iter().copied().enumerate() {
            if self.slots[slot_index].owner.is_none()
                || self.slots[slot_index].planned != self.slots[index].planned
                || !Self::completion_bundle_current(&self.slots[slot_index])
            {
                return false;
            }
            let Some(PreparedActionWork::Ordinary(work)) = self.slots[slot_index].work.as_ref()
            else {
                return false;
            };
            let Ok(plan) = mixer.plan_ordinary_early_completion(work, execution, callback) else {
                return false;
            };
            plans[member] = Some(plan);
        }
        if !mixer.group_capture_current(&group.batch, key, execution) {
            return false;
        }
        // Every plan has passed before any Work, receipt or E changes. Temporary
        // failed-plan clones above share pins with the bank and unchanged Work.
        let group = self.slots[index].group.take().unwrap();
        let mut failure = None;
        for (member, slot_index) in group.slots[..group.len].iter().copied().enumerate() {
            let Some(PreparedActionWork::Ordinary(work)) = self.slots[slot_index].work.as_mut()
            else {
                unreachable!("validated group retains every member");
            };
            if let Err(reason) = work.bind_early_completion(plans[member].take().unwrap()) {
                failure = Some(reason);
                break;
            }
        }
        if let Some(reason) = failure {
            // Cancellation may win a member CAS; none of the group has effected.
            for slot_index in &group.slots[..group.len] {
                self.slots[*slot_index]
                    .work
                    .as_ref()
                    .unwrap()
                    .reject(reason);
            }
        } else {
            let header = &self.slots[index];
            let moved = scheduler.retarget_ready_pitch_action(
                index,
                header.sequence,
                header.planned,
                execution,
            );
            assert!(moved, "single callback retains the exact queued group");
            // The same event keeps OriginalOrder and the reserved STOP lane.
            // Later STOP admission must observe the new common capture S=E.
            mixer.replan_group_capture(&group.batch, key, execution);
            for slot_index in &group.slots[..group.len] {
                self.slots[*slot_index].planned = execution;
            }
        }
        self.slots[index].group = Some(group);
        failure.is_none()
    }
}
