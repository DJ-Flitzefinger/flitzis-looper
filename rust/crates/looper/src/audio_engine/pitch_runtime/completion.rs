//! Optional early completion, with no unfinished worker in the deferred-due cell.

use super::*;
use crate::audio_engine::ordinary_start::OrdinaryCompletionPlan;

impl PitchRuntime {
    pub(super) fn accelerate_completed_starts<const CAPACITY: usize>(
        &mut self,
        mixer: &mut RtMixer,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        callback: u64,
    ) {
        for index in 0..self.slots.len() {
            let slot = &self.slots[index];
            let execution = slot.original.max(callback);
            if slot.sequence == 0
                || slot.replans != 0
                || execution >= slot.planned
                || !scheduler.has_queued_pitch_action(index, slot.sequence, slot.planned)
            {
                continue;
            }
            if slot.group.is_some() {
                self.accelerate_group(mixer, scheduler, index, execution, callback);
                continue;
            }
            // Later originals wait for their predecessor to actually finish.
            // A group member has no separate scheduler event and never enters here.
            if self.preceding_endpoint(index).is_some() {
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
        }
    }

    fn accelerate_group<const CAPACITY: usize>(
        &mut self,
        mixer: &mut RtMixer,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        index: usize,
        execution: u64,
        callback: u64,
    ) {
        let group = self.slots[index].group.as_ref().unwrap();
        // Recording owns a separately admitted S/stop reservation. Its existing
        // exact path is unchanged until a separate complete transaction is proved.
        if group.batch.capture_publication.is_some() {
            return;
        }
        let mut plans: [Option<OrdinaryCompletionPlan>; super::super::constants::MAX_VOICES] =
            std::array::from_fn(|_| None);
        for (member, slot_index) in group.slots[..group.len].iter().copied().enumerate() {
            if self.preceding_endpoint(slot_index).is_some() {
                return;
            }
            let Some(PreparedActionWork::Ordinary(work)) = self.slots[slot_index].work.as_ref()
            else {
                return;
            };
            let Ok(plan) = mixer.plan_ordinary_early_completion(work, execution, callback) else {
                return;
            };
            plans[member] = Some(plan);
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
            for slot_index in &group.slots[..group.len] {
                self.slots[*slot_index].planned = execution;
            }
        }
        self.slots[index].group = Some(group);
    }
}
