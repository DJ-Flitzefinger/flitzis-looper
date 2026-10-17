//! Fixed preparation custody connected to the one transport scheduler.

use super::audio_stream::{AudioMessageSink, ScheduledExecution};
use super::buffer_retirement::AudioBufferRetirement;
use super::global_playback_batch::GlobalPlaybackBatch;
use super::mixer::RtMixer;
use super::ordinary_endpoint_reserve::OrdinaryEndpointReserve;
use super::ordinary_start::{OrdinaryStartGroup, OrdinaryStartWork};
use super::pitch_actions::{
    PitchActionPreparation, PitchActionPreparationWorker, PitchActionReject, PitchActionWork,
    PreparedActionWork,
};
use super::scheduler::{FixedCapacityScheduler, ScheduledCommand};
use std::sync::Arc;

pub(crate) const MAX_PITCH_ACTIONS: usize = 64;
// Explicit preparation lead, never another quantize calculation. T remains original.
const PREPARATION_LEAD_FRAMES: u64 = 8192;

struct ActionSlot {
    sequence: u64,
    work: Option<PreparedActionWork>,
    group: Option<OrdinaryStartGroup>,
}

pub(crate) struct PitchRuntime {
    slots: Box<[ActionSlot]>,
    preparation: PitchActionPreparation,
    endpoint: OrdinaryEndpointReserve,
}

fn retire(work: PreparedActionWork, retirement: &mut impl AudioBufferRetirement) {
    match work {
        PreparedActionWork::Pitch(work) => retirement.retire_pitch_action(work),
        PreparedActionWork::Ordinary(work) => retirement.retire_ordinary_start(work),
    }
}

fn execution_frame(original: u64, callback: u64, needs_preparation: bool) -> u64 {
    if !needs_preparation || original > callback {
        original.max(callback)
    } else {
        callback.saturating_add(PREPARATION_LEAD_FRAMES)
    }
}

impl PitchRuntime {
    #[cfg(test)]
    pub(super) fn selected_endpoint_ready(&self) -> bool {
        self.endpoint.ready()
    }
    #[cfg(test)]
    pub(super) fn selected_endpoint_native_address(&self) -> usize {
        self.endpoint.native_address()
    }
    #[cfg(test)]
    pub(super) fn unused_action_capacity(&self) -> (usize, usize) {
        (
            self.slots.iter().filter(|slot| slot.sequence == 0).count(),
            self.preparation.preparation_slots(),
        )
    }
    #[cfg(test)]
    pub(super) fn ordinary_group_ready(&self, count: usize) -> bool {
        self.slots
            .iter()
            .filter_map(|slot| slot.group.as_ref())
            .any(|group| {
                group.len == count
                    && group.slots[..group.len]
                        .iter()
                        .enumerate()
                        .all(|(member, index)| {
                            group.work[member]
                                .as_ref()
                                .is_some_and(|work| work.is_ready())
                                || self.slots[*index]
                                    .work
                                    .as_ref()
                                    .is_some_and(PreparedActionWork::is_ready)
                        })
            })
    }
    #[cfg(test)]
    pub(super) fn has_ready_action(&self, sequence: u64) -> bool {
        self.slots.iter().any(|slot| {
            slot.sequence == sequence
                && slot.work.as_ref().is_some_and(PreparedActionWork::is_ready)
        })
    }
    #[cfg(all(test, windows))]
    pub(super) fn ready_pitch_native_address(&self, sequence: u64) -> Option<usize> {
        self.slots.iter().find_map(|slot| {
            if slot.sequence != sequence {
                return None;
            }
            let PreparedActionWork::Pitch(work) = slot.work.as_ref()? else {
                return None;
            };
            if !work.is_ready() {
                return None;
            }
            work.candidate
                .as_ref()?
                .rubberband
                .as_ref()
                .map(|native| native.state_address())
        })
    }
    pub(crate) fn new() -> std::io::Result<(Self, PitchActionPreparationWorker)> {
        let (preparation, worker) = PitchActionPreparation::new(MAX_PITCH_ACTIONS)?;
        let slots = (0..MAX_PITCH_ACTIONS)
            .map(|_| ActionSlot {
                sequence: 0,
                work: None,
                group: None,
            })
            .collect();
        Ok((
            Self {
                slots,
                preparation,
                endpoint: OrdinaryEndpointReserve::new(),
            },
            worker,
        ))
    }

    pub(crate) fn poll(&mut self) {
        self.endpoint.poll(&mut self.preparation);
        for _ in 0..MAX_PITCH_ACTIONS {
            let Some(work) = self.preparation.poll_prepared() else {
                break;
            };
            let slot = self
                .slots
                .iter_mut()
                .find(|slot| slot.sequence == work.sequence())
                .expect("returned preparation retains its reserved action slot");
            debug_assert!(slot.work.is_none());
            slot.work = Some(work);
        }
    }

    fn install<const CAPACITY: usize>(
        &mut self,
        work: PreparedActionWork,
        index: usize,
        execution: u64,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    ) {
        let sequence = work.sequence();
        let scheduled = scheduler.schedule(
            execution,
            ScheduledCommand::PitchAction {
                slot: index,
                action_sequence: sequence,
            },
        );
        debug_assert!(scheduled.is_ok());
        self.slots[index].sequence = sequence;
        match &work {
            PreparedActionWork::Pitch(work) => {
                work.action.mark_scheduled();
            }
            PreparedActionWork::Ordinary(work) => {
                work.origin().mark_scheduled();
            }
        }
        self.slots[index].work = Some(work);
        if self.slots[index]
            .work
            .as_ref()
            .unwrap()
            .preparation_required()
            && !self.slots[index].work.as_ref().unwrap().is_ready()
        {
            let published = self
                .preparation
                .publish_prepared(&mut self.slots[index].work);
            debug_assert!(published.is_ok());
        }
    }

    fn accept<const CAPACITY: usize>(
        &mut self,
        mixer: &RtMixer,
        mut work: Box<PitchActionWork>,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        callback: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        let index = self.slots.iter().position(|slot| slot.sequence == 0);
        if index.is_none() || scheduler.is_full() {
            work.action.reject(PitchActionReject::Capacity);
            retirement.retire_pitch_action(work);
            return;
        }
        let required = match mixer.requires_pitch_action_preparation(&work) {
            Ok(required) => required,
            Err(reason) => {
                work.action.reject(reason);
                retirement.retire_pitch_action(work);
                return;
            }
        };
        let hit = required && self.endpoint.ready_for_pitch(mixer, &work);
        if !hit && !self.preparation.preparation_available() {
            work.action.reject(PitchActionReject::Capacity);
            retirement.retire_pitch_action(work);
            return;
        }
        let execution =
            execution_frame(work.action.original_due_frame(), callback, required && !hit);
        let capture = work
            .freeze_execution_frame(execution)
            .and_then(|()| mixer.capture_pitch_action_request(&mut work, callback, execution));
        let result = match capture {
            Ok(Some(request)) => work.rt_capture(request).map_err(|request| {
                let retained = work.retain_rejected_capture(request);
                debug_assert!(retained.is_ok());
                if let Err(request) = retained {
                    std::mem::forget(request);
                }
                PitchActionReject::PreparedMismatch
            }),
            Ok(None) => work.complete_without_preparation(),
            Err(reason) => Err(reason),
        };
        let result = result.and_then(|()| {
            if hit {
                if !self.endpoint.consume_pitch(&mut work) {
                    return Err(PitchActionReject::PreparedMismatch);
                }
                work.current_guard()?;
            }
            Ok(())
        });
        if let Err(reason) = result {
            work.action.reject(reason);
            retirement.retire_pitch_action(work);
            return;
        }
        self.install(
            PreparedActionWork::Pitch(work),
            index.unwrap(),
            execution,
            scheduler,
        );
    }

    fn accept_ordinary<const CAPACITY: usize>(
        &mut self,
        mixer: &RtMixer,
        mut work: Box<OrdinaryStartWork>,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        callback: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        let result = (|| {
            let index = self
                .slots
                .iter()
                .position(|slot| slot.sequence == 0)
                .ok_or(PitchActionReject::Capacity)?;
            if scheduler.is_full() {
                return Err(PitchActionReject::Capacity);
            }
            let original = work
                .origin()
                .target_frame()
                .ok_or(PitchActionReject::TimingUnavailable)?;
            let required = mixer.requires_ordinary_start_preparation(&work)?;
            let hit = required && self.endpoint.ready_for(mixer, &work);
            let needs_worker = required && !hit;
            if needs_worker && !self.preparation.preparation_available() {
                return Err(PitchActionReject::Capacity);
            }
            let execution = execution_frame(original, callback, needs_worker);
            work.freeze_frames(original, execution)?;
            mixer.capture_ordinary_start_request(&mut work, execution)?;
            if hit {
                if !self.endpoint.consume(&mut work) {
                    return Err(PitchActionReject::PreparedMismatch);
                }
                work.origin().callback_guard()?;
            }
            Ok((index, execution))
        })();
        match result {
            Ok((index, execution)) => self.install(
                PreparedActionWork::Ordinary(work),
                index,
                execution,
                scheduler,
            ),
            Err(reason) => {
                work.reject(reason);
                retirement.retire_ordinary_start(work);
            }
        }
    }

    fn accept_group<const CAPACITY: usize>(
        &mut self,
        mixer: &RtMixer,
        batch: Arc<GlobalPlaybackBatch>,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        callback: u64,
        original: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        let Some(work) = batch.take_prepared_starts() else {
            batch.finish(false);
            retirement.retire_global_playback_batch(batch);
            return;
        };
        let mut group = OrdinaryStartGroup {
            len: work.len(),
            work,
            batch,
            slots: [usize::MAX; super::constants::MAX_VOICES],
        };
        let result = (|| {
            if group.len == 0
                || group.len > super::constants::MAX_VOICES
                || scheduler.is_full()
                || self.preparation.preparation_slots() < group.len
            {
                return Err(PitchActionReject::Capacity);
            }
            let mut count = 0;
            for (index, slot) in self.slots.iter().enumerate() {
                if slot.sequence == 0 {
                    group.slots[count] = index;
                    count += 1;
                    if count == group.len {
                        break;
                    }
                }
            }
            if count != group.len {
                return Err(PitchActionReject::Capacity);
            }
            let mut required = false;
            for work in &group.work {
                required |= mixer.requires_ordinary_start_preparation(
                    work.as_ref().ok_or(PitchActionReject::PreparedMismatch)?,
                )?;
            }
            let execution = execution_frame(original, callback, required);
            for work in group.work.iter_mut().flatten() {
                match work.origin().target_frame() {
                    None => work.origin().capture_target(original)?,
                    Some(captured) if captured == original => {}
                    Some(_) => return Err(PitchActionReject::PreparedMismatch),
                }
                work.freeze_frames(original, execution)?;
                mixer.capture_ordinary_start_request(work, execution)?;
            }
            Ok(execution)
        })();
        let execution = match result {
            Ok(execution) => execution,
            Err(reason) => {
                group.reject(reason);
                retirement.retire_ordinary_start_group(group);
                return;
            }
        };
        let first = group.slots[0];
        let sequence = group.work[0].as_ref().unwrap().sequence();
        let scheduled = scheduler.schedule(
            execution,
            ScheduledCommand::PitchAction {
                slot: first,
                action_sequence: sequence,
            },
        );
        debug_assert!(scheduled.is_ok());
        for (member, index) in group.slots[..group.len].iter().copied().enumerate() {
            let work = group.work[member].take().unwrap();
            self.slots[index].sequence = work.sequence();
            work.origin().mark_scheduled();
            self.slots[index].work = Some(PreparedActionWork::Ordinary(work));
            if self.slots[index]
                .work
                .as_ref()
                .unwrap()
                .preparation_required()
                && !self.slots[index].work.as_ref().unwrap().is_ready()
            {
                let published = self
                    .preparation
                    .publish_prepared(&mut self.slots[index].work);
                debug_assert!(published.is_ok());
            }
        }
        self.slots[first].group = Some(group);
    }

    fn execute_group(
        &mut self,
        mixer: &mut RtMixer,
        index: usize,
        frame: u64,
        messages: &mut impl AudioMessageSink,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        let group = self.slots[index].group.as_ref().unwrap();
        if retirement.available_retirement_slots() == 0
            || group.slots[..group.len]
                .iter()
                .enumerate()
                .any(|(member, index)| {
                    group.work[member].is_none() && self.slots[*index].work.is_none()
                })
        {
            return false;
        }
        let mut group = self.slots[index].group.take().unwrap();
        for (member, index) in group.slots[..group.len].iter().copied().enumerate() {
            if group.work[member].is_none() {
                let Some(PreparedActionWork::Ordinary(work)) = self.slots[index].work.take() else {
                    unreachable!("ordinary group retains ordinary participants");
                };
                group.work[member] = Some(work);
            }
        }
        let result = mixer.execute_ordinary_global_start(
            &mut group.work,
            &group.batch,
            frame,
            messages,
            retirement,
        );
        if matches!(
            result,
            Err(PitchActionReject::Capacity
                | PitchActionReject::PlacementPending
                | PitchActionReject::WindowPending)
        ) {
            self.slots[index].group = Some(group);
            return false;
        }
        match result {
            Ok(()) => group.batch.finish(true),
            Err(reason) => group.reject(reason),
        }
        for index in &group.slots[..group.len] {
            self.slots[*index].sequence = 0;
        }
        retirement.retire_ordinary_start_group(group);
        true
    }

    fn execute(
        &mut self,
        mixer: &mut RtMixer,
        index: usize,
        sequence: u64,
        frame: u64,
        messages: &mut impl AudioMessageSink,
        retirement: &mut impl AudioBufferRetirement,
    ) -> ScheduledExecution {
        self.poll();
        let command = ScheduledCommand::PitchAction {
            slot: index,
            action_sequence: sequence,
        };
        let Some(slot) = self
            .slots
            .get(index)
            .filter(|slot| slot.sequence == sequence)
        else {
            return ScheduledExecution::Complete;
        };
        if slot.group.is_some() {
            return if self.execute_group(mixer, index, frame, messages, retirement) {
                ScheduledExecution::Complete
            } else {
                ScheduledExecution::Pending(command)
            };
        }
        let slot = &mut self.slots[index];
        if slot.work.is_none() || retirement.available_retirement_slots() == 0 {
            return ScheduledExecution::Pending(command);
        }
        let result = match slot.work.as_mut().unwrap() {
            PreparedActionWork::Pitch(work) => {
                if work.action.state() == "rejected" {
                    Err(PitchActionReject::PreparationFailed)
                } else {
                    mixer.execute_pitch_action(work, frame, messages, retirement)
                }
            }
            PreparedActionWork::Ordinary(work) => {
                if work.origin().state() == "rejected" {
                    Err(PitchActionReject::PreparationFailed)
                } else {
                    mixer.execute_ordinary_start(work, frame, messages, retirement)
                }
            }
        };
        if matches!(
            result,
            Err(PitchActionReject::Capacity
                | PitchActionReject::PlacementPending
                | PitchActionReject::WindowPending)
        ) {
            return ScheduledExecution::Pending(command);
        }
        if let Err(reason) = result {
            slot.work.as_ref().unwrap().reject(reason);
        }
        retire(slot.work.take().unwrap(), retirement);
        slot.sequence = 0;
        ScheduledExecution::Complete
    }
}

/// Shared render entry, after control/parameter drains, including stopped callbacks.
pub(super) fn service_endpoint_reserve(mixer: &mut RtMixer) {
    let mut runtime = mixer
        .pitch_runtime
        .take()
        .expect("one callback owns preparation lane");
    runtime.poll();
    runtime.endpoint.service(mixer, &mut runtime.preparation);
    mixer.pitch_runtime = Some(runtime);
}

pub(super) fn accept<const CAPACITY: usize>(
    mixer: &mut RtMixer,
    work: Box<PitchActionWork>,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback: u64,
    retirement: &mut impl AudioBufferRetirement,
) {
    let mut runtime = mixer
        .pitch_runtime
        .take()
        .expect("one callback owns preparation lane");
    runtime.poll();
    runtime.accept(mixer, work, scheduler, callback, retirement);
    mixer.pitch_runtime = Some(runtime);
}

pub(super) fn accept_ordinary<const CAPACITY: usize>(
    mixer: &mut RtMixer,
    work: Box<OrdinaryStartWork>,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback: u64,
    retirement: &mut impl AudioBufferRetirement,
) {
    let mut runtime = mixer
        .pitch_runtime
        .take()
        .expect("one callback owns preparation lane");
    runtime.poll();
    runtime.accept_ordinary(mixer, work, scheduler, callback, retirement);
    mixer.pitch_runtime = Some(runtime);
}

pub(super) fn accept_group<const CAPACITY: usize>(
    mixer: &mut RtMixer,
    batch: Arc<GlobalPlaybackBatch>,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback: u64,
    original: u64,
    retirement: &mut impl AudioBufferRetirement,
) {
    let mut runtime = mixer
        .pitch_runtime
        .take()
        .expect("one callback owns preparation lane");
    runtime.poll();
    runtime.accept_group(mixer, batch, scheduler, callback, original, retirement);
    mixer.pitch_runtime = Some(runtime);
}

pub(super) fn execute(
    mixer: &mut RtMixer,
    slot: usize,
    sequence: u64,
    frame: u64,
    messages: &mut impl AudioMessageSink,
    retirement: &mut impl AudioBufferRetirement,
) -> ScheduledExecution {
    let mut runtime = mixer
        .pitch_runtime
        .take()
        .expect("one callback owns preparation lane");
    let result = runtime.execute(mixer, slot, sequence, frame, messages, retirement);
    mixer.pitch_runtime = Some(runtime);
    result
}
