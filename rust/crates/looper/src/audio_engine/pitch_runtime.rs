//! Fixed preparation custody connected to the one transport scheduler.

mod completion;
#[cfg(all(test, windows))]
mod group_peer_tests;

use super::audio_stream::{AudioMessageSink, ScheduledExecution};
use super::buffer_retirement::AudioBufferRetirement;
use super::global_playback_batch::GlobalPlaybackBatch;
use super::mixer::RtMixer;
use super::ordinary_endpoint_reserve::OrdinaryEndpointReserve;
use super::ordinary_start::{OrdinaryGroupKey, OrdinaryStartGroup, OrdinaryStartWork};
use super::pitch_actions::{
    PitchActionPreparation, PitchActionPreparationWorker, PitchActionReject, PitchActionWork,
    PreparedActionWork,
};
use super::scheduler::{FixedCapacityScheduler, ScheduledCommand};
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub(crate) const MAX_PITCH_ACTIONS: usize = 64;
// Explicit preparation lead, never another quantize calculation. T remains original.
const PREPARATION_LEAD_FRAMES: u64 = 8192;
const MAX_LATE_REPLANS: u8 = 8;
const MAX_LAUNCH_DELAY: u64 = super::source_feed_history::SOURCE_FEED_HISTORY_MAX_FRAMES as u64;

enum OriginalActionOwner {
    Pitch(super::pitch_actions::CapturedPitchAction),
    Ordinary(super::pad_start::CapturedPadStart),
}

impl OriginalActionOwner {
    fn rejected(&self) -> bool {
        match self {
            Self::Pitch(owner) => owner.state() == "rejected",
            Self::Ordinary(owner) => owner.state() == "rejected",
        }
    }
    fn reject(&self, reason: PitchActionReject) {
        match self {
            Self::Pitch(owner) => {
                owner.reject(reason);
            }
            Self::Ordinary(owner) => {
                owner.reject(reason);
            }
        }
    }
}

struct ActionSlot {
    sequence: u64,
    original: u64,
    planned: u64,
    content: Option<super::content_placement::ContentKey>,
    epoch: Option<Arc<std::sync::atomic::AtomicU64>>,
    owner: Option<OriginalActionOwner>,
    independent_birth: bool,
    replans: u8,
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
    /// Existing fixed custody only. Terminal originals and retired content never
    /// fence an unrelated adoption; worker-held work retains its original epoch.
    pub(crate) fn has_pending_content(&self, key: super::content_placement::ContentKey) -> bool {
        self.slots.iter().any(|slot| {
            slot.content == Some(key)
                && slot
                    .epoch
                    .as_ref()
                    .is_some_and(|epoch| epoch.load(Ordering::Acquire) != 0)
                && slot.owner.as_ref().is_some_and(|owner| match owner {
                    OriginalActionOwner::Pitch(owner) => {
                        owner.content_is_current()
                            && !matches!(owner.state(), "applied" | "rejected")
                    }
                    OriginalActionOwner::Ordinary(owner) => {
                        owner.content_key() == key
                            && !matches!(owner.state(), "applied" | "rejected")
                    }
                })
        })
    }

    pub(crate) fn capture_group_pending(&self, key: OrdinaryGroupKey, session_id: u64) -> bool {
        self.slots
            .get(key.slot)
            .filter(|slot| slot.sequence == key.action_sequence)
            .and_then(|slot| slot.group.as_ref())
            .is_some_and(|group| {
                group
                    .batch
                    .capture_publication
                    .as_ref()
                    .is_some_and(|publication| publication.session_id() == session_id)
                    && group.slots[..group.len].iter().all(|index| {
                        self.slots[*index]
                            .owner
                            .as_ref()
                            .is_some_and(|owner| !owner.rejected())
                    })
            })
    }
    /// Only a still-uncommitted exact group/session can be withdrawn by its own
    /// capture. Heavy Work remains in this same runtime/worker custody.
    pub(crate) fn reject_capture_group(&mut self, key: OrdinaryGroupKey, session_id: u64) {
        let Some(slot) = self
            .slots
            .get(key.slot)
            .filter(|slot| slot.sequence == key.action_sequence)
        else {
            return;
        };
        let Some(group) = slot.group.as_ref().filter(|group| {
            group
                .batch
                .capture_publication
                .as_ref()
                .is_some_and(|publication| publication.session_id() == session_id)
        }) else {
            return;
        };
        group.batch.finish(false);
        for index in &group.slots[..group.len] {
            if let Some(owner) = &self.slots[*index].owner {
                owner.reject(PitchActionReject::PreparationFailed);
            }
            if let Some(epoch) = &self.slots[*index].epoch {
                epoch.store(0, Ordering::Release);
            }
        }
    }
    fn clear_slot_metadata(slot: &mut ActionSlot) {
        // The caller still holds the genuine Work/Group, so these scalar Arc
        // observations cannot become final owners on the callback.
        slot.owner = None;
        slot.epoch = None;
        slot.content = None;
        slot.independent_birth = false;
        slot.original = 0;
        slot.planned = 0;
        slot.replans = 0;
        slot.sequence = 0;
    }

    fn set_slot_metadata(slot: &mut ActionSlot, work: &PreparedActionWork, execution: u64) {
        let (original, content, epoch) = match work {
            PreparedActionWork::Pitch(work) => (
                work.action.original_due_frame(),
                work.action.content_key(),
                work.action.preparation_epoch().clone(),
            ),
            PreparedActionWork::Ordinary(work) => (
                work.origin()
                    .target_frame()
                    .expect("captured original target"),
                work.origin().content_key(),
                work.epoch.clone(),
            ),
        };
        slot.original = original;
        slot.planned = execution;
        slot.content = Some(content);
        slot.epoch = Some(epoch);
        // A published original contains scalar admission; its Work already left
        // control custody. This fixed receipt owns no Native or PCM chain.
        slot.owner = Some(match work {
            PreparedActionWork::Pitch(work) => OriginalActionOwner::Pitch(work.action.clone()),
            PreparedActionWork::Ordinary(work) => {
                OriginalActionOwner::Ordinary(work.origin().clone())
            }
        });
        slot.independent_birth = match work {
            PreparedActionWork::Ordinary(_) => true,
            PreparedActionWork::Pitch(work) => work.late_reprepare_snapshot().is_some(),
        };
        slot.replans = 0;
    }

    fn preceding_endpoint(&self, index: usize) -> Option<u64> {
        let current = &self.slots[index];
        if !current.independent_birth {
            return None;
        }
        self.slots
            .iter()
            .filter(|slot| {
                slot.sequence != 0
                    && slot.independent_birth
                    && slot.content == current.content
                    && (slot.original, slot.sequence) < (current.original, current.sequence)
                    // Terminal rejection cannot become an effect. Keep its
                    // slot/event/worker custody until normal cleanup, but no
                    // longer make live originals wait for that cleanup deadline.
                    && slot.owner.as_ref().is_none_or(|owner| !owner.rejected())
            })
            .map(|slot| slot.planned)
            .max()
    }

    fn next_replan(slot: &ActionSlot, frame: u64) -> Result<u64, PitchActionReject> {
        let execution = frame
            .checked_add(PREPARATION_LEAD_FRAMES)
            .ok_or(PitchActionReject::SequenceExhausted)?;
        if slot.replans >= MAX_LATE_REPLANS
            || execution
                .checked_sub(slot.original)
                .is_none_or(|frames| frames > MAX_LAUNCH_DELAY)
        {
            return Err(PitchActionReject::PreparationFailed);
        }
        Ok(execution)
    }

    fn prepare_late_work(
        mixer: &mut RtMixer,
        work: &mut PreparedActionWork,
        execution: u64,
        callback: u64,
        timeline: Option<super::transport::TransportTimeline>,
    ) -> Result<(), PitchActionReject> {
        match work {
            PreparedActionWork::Ordinary(work) => {
                mixer.prepare_ordinary_late_replan(work, execution, callback, timeline)
            }
            PreparedActionWork::Pitch(work) => {
                mixer.prepare_pitch_late_replan(work, execution, callback, timeline)
            }
        }
    }

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
    #[cfg(all(test, windows))]
    pub(super) fn ordinary_group_originals(&self) -> Vec<super::pad_start::CapturedPadStart> {
        let group = self
            .slots
            .iter()
            .find_map(|slot| slot.group.as_ref())
            .expect("test observes its actual admitted ordinary group");
        group.slots[..group.len]
            .iter()
            .map(|index| match self.slots[*index].owner.as_ref().unwrap() {
                OriginalActionOwner::Ordinary(original) => original.clone(),
                OriginalActionOwner::Pitch(_) => unreachable!("ordinary group owner"),
            })
            .collect()
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
                original: 0,
                planned: 0,
                content: None,
                epoch: None,
                owner: None,
                independent_birth: false,
                replans: 0,
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
        self.reject_impossible_group_peers();
    }

    fn reject_impossible_group_peers(&self) {
        for group in self.slots.iter().filter_map(|slot| slot.group.as_ref()) {
            let members = &group.slots[..group.len];
            // The retained group owns these exact reserved slots until cleanup.
            // Missing metadata, epochs and deadlines cannot prove impossibility.
            if !members.iter().all(|index| {
                self.slots[*index].sequence != 0
                    && matches!(
                        self.slots[*index].owner,
                        Some(OriginalActionOwner::Ordinary(_))
                    )
            }) || !members
                .iter()
                .any(|index| self.slots[*index].owner.as_ref().unwrap().rejected())
            {
                continue;
            }
            // Every member must claim before any group audio store. A terminal
            // rejected member makes that effect impossible, even with Work on
            // the worker. Each peer still needs its own successful rejection CAS;
            // a lost CAS remains an ordering barrier and is retried next poll.
            for index in members {
                self.slots[*index]
                    .owner
                    .as_ref()
                    .unwrap()
                    .reject(PitchActionReject::PreparationFailed);
            }
            group.batch.finish(false);
            // No epoch, slot, event, Work or capture custody is released here.
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
        Self::set_slot_metadata(&mut self.slots[index], &work, execution);
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
        mixer: &mut RtMixer,
        mut work: Box<PitchActionWork>,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        callback: u64,
        timeline: Option<super::transport::TransportTimeline>,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        if work.action.variant() == super::pitch_actions::PitchActionVariant::SetRetrigger {
            work.launch_qualification =
                super::loop_phase_reference::LaunchQualification::capture(timeline, callback);
            work.launch_reference = super::loop_phase_reference::LoopPhaseReference::capture(
                timeline,
                work.action.original_due_frame(),
            );
        }
        let index = self.slots.iter().position(|slot| slot.sequence == 0);
        if index.is_none() || scheduler.is_full() {
            work.action.reject(PitchActionReject::Capacity);
            retirement.retire_pitch_action(work);
            return;
        }
        // Classify at the original possible effect, then keep the existing one-time
        // preparation lead. Neither classification nor a cache hit may use the bank
        // target in place of an active voice's still changing actual rate.
        let provisional = execution_frame(work.action.original_due_frame(), callback, false);
        let projection_frames = provisional - callback;
        let required = match mixer.requires_pitch_action_preparation(&work, projection_frames) {
            Ok(required) => required,
            Err(reason) => {
                work.action.reject(reason);
                retirement.retire_pitch_action(work);
                return;
            }
        };
        let hit = required
            && work.action.original_due_frame() >= callback
            && work.action.start_behavior()
                != Some(super::key_lock_start_policy::KeyLockStartBehavior::AttackAtGrid)
            && self
                .endpoint
                .ready_for_pitch(mixer, &work, projection_frames);
        let needs_worker = required && !hit;
        if needs_worker && !self.preparation.preparation_available() {
            work.action.reject(PitchActionReject::Capacity);
            retirement.retire_pitch_action(work);
            return;
        }
        let execution = execution_frame(work.action.original_due_frame(), callback, needs_worker);
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
        mixer: &mut RtMixer,
        mut work: Box<OrdinaryStartWork>,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        callback: u64,
        timeline: Option<super::transport::TransportTimeline>,
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
            work.freeze_launch_clock(timeline, original, callback)?;
            let required = mixer.requires_ordinary_start_preparation(&work)?;
            let hit = required
                && work.origin().start_behavior()
                    != super::key_lock_start_policy::KeyLockStartBehavior::AttackAtGrid
                && self.endpoint.ready_for(mixer, &work);
            let needs_worker = required && !hit;
            if needs_worker && !self.preparation.preparation_available() {
                return Err(PitchActionReject::Capacity);
            }
            let execution = execution_frame(original, callback, needs_worker);
            work.freeze_frames(original, execution)?;
            mixer.capture_ordinary_start_request(&mut work, callback, execution)?;
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
        mixer: &mut RtMixer,
        batch: Arc<GlobalPlaybackBatch>,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        callback: u64,
        original: u64,
        timeline: Option<super::transport::TransportTimeline>,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        let Some(work) = batch.take_prepared_starts() else {
            // A transferred duplicate has no authority over the original active
            // group, receipt or capture. Retire only this redundant Arc.
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
            if group
                .batch
                .captured_start
                .as_ref()
                .is_some_and(|captured| captured.cancelled())
            {
                return Err(PitchActionReject::LaunchCancelled);
            }
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
            let mut required = 0;
            let mut reserve_member = None;
            for work in group.work.iter_mut().flatten() {
                work.freeze_launch_clock(timeline, original, callback)?;
            }
            for (member, work) in group.work.iter().enumerate() {
                let work = work.as_ref().ok_or(PitchActionReject::PreparedMismatch)?;
                if mixer.requires_ordinary_start_preparation(work)? {
                    required += 1;
                    if work.origin().start_behavior()
                        != super::key_lock_start_policy::KeyLockStartBehavior::AttackAtGrid
                        && self.endpoint.ready_for(mixer, work)
                    {
                        reserve_member = Some(member);
                    }
                }
            }
            // One reserve owns one independent Native/FIFO. It can make the
            // complete group ready only when every other member is already dry.
            let reserve_member = reserve_member.filter(|_| required == 1);
            let execution = execution_frame(
                original,
                callback,
                required != 0 && reserve_member.is_none(),
            );
            for work in group.work.iter_mut().flatten() {
                match work.origin().target_frame() {
                    None => work.origin().capture_target(
                        original,
                        mixer.source_control_revision(work.origin().content_key())?,
                    )?,
                    Some(captured) if captured == original => {}
                    Some(_) => return Err(PitchActionReject::PreparedMismatch),
                }
                work.freeze_frames(original, execution)?;
                mixer.capture_ordinary_start_request(work, callback, execution)?;
            }
            if let Some(member) = reserve_member {
                let work = group.work[member].as_mut().unwrap();
                if !self.endpoint.consume(work) {
                    return Err(PitchActionReject::PreparedMismatch);
                }
                work.origin().callback_guard()?;
            }
            Ok(execution)
        })();
        let execution = match result {
            Ok(execution) => execution,
            Err(reason) => {
                group.reject(reason);
                mixer.reject_unadmitted_group_capture(&group.batch, retirement);
                retirement.retire_ordinary_start_group(group);
                return;
            }
        };
        let first = group.slots[0];
        let sequence = group.work[0].as_ref().unwrap().sequence();
        let key = OrdinaryGroupKey {
            slot: first,
            action_sequence: sequence,
        };
        let scheduled = if let Some(publication) = &group.batch.capture_publication {
            mixer.accept_group_capture(publication, key, execution, scheduler, retirement)
        } else {
            scheduler
                .schedule(
                    execution,
                    ScheduledCommand::PitchAction {
                        slot: first,
                        action_sequence: sequence,
                    },
                )
                .map(|_| ())
                .map_err(|_| PitchActionReject::Capacity)
        };
        if let Err(reason) = scheduled {
            group.reject(reason);
            retirement.retire_ordinary_start_group(group);
            return;
        }
        for (member, index) in group.slots[..group.len].iter().copied().enumerate() {
            let work = group.work[member].take().unwrap();
            self.slots[index].sequence = work.sequence();
            work.origin().mark_scheduled();
            let work = PreparedActionWork::Ordinary(work);
            Self::set_slot_metadata(&mut self.slots[index], &work, execution);
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
        self.slots[first].group = Some(group);
    }

    fn execute_group(
        &mut self,
        mixer: &mut RtMixer,
        index: usize,
        frame: u64,
        timeline: Option<super::transport::TransportTimeline>,
        messages: &mut impl AudioMessageSink,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        let group = self.slots[index].group.as_ref().unwrap();
        let key = OrdinaryGroupKey {
            slot: index,
            action_sequence: self.slots[index].sequence,
        };
        if !mixer.group_capture_current(&group.batch, key, frame.max(self.slots[index].planned)) {
            mixer.reject_pending_group_capture(&group.batch, key);
            let session = group
                .batch
                .capture_publication
                .as_ref()
                .map_or(0, |publication| publication.session_id());
            self.reject_capture_group(key, session);
        }
        let group = self.slots[index].group.as_ref().unwrap();
        let expired = frame
            .checked_sub(self.slots[index].original)
            .is_some_and(|frames| frames > MAX_LAUNCH_DELAY);
        if expired {
            for slot_index in &group.slots[..group.len] {
                if let Some(owner) = &self.slots[*slot_index].owner {
                    owner.reject(PitchActionReject::PreparationFailed);
                }
                if let Some(epoch) = &self.slots[*slot_index].epoch {
                    epoch.store(0, Ordering::Release);
                }
            }
        }
        let preceding = (!expired)
            .then(|| {
                group.slots[..group.len]
                    .iter()
                    .filter_map(|slot_index| self.preceding_endpoint(*slot_index))
                    .max()
            })
            .flatten();
        let retirement_units = if group.batch.capture_publication.is_some() {
            2
        } else {
            1
        };
        if retirement.available_retirement_slots() < retirement_units
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
        let expected = group.work[0].as_ref().unwrap().execution_frame();
        if !mixer.group_capture_current(&group.batch, key, expected) {
            group.reject(PitchActionReject::PreparationFailed);
            mixer.reject_pending_group_capture(&group.batch, key);
            for slot_index in &group.slots[..group.len] {
                Self::clear_slot_metadata(&mut self.slots[*slot_index]);
            }
            retirement.retire_ordinary_start_group(group);
            return true;
        }
        if frame < expected && preceding.is_none_or(|previous| previous <= expected) {
            self.slots[index].planned = expected;
            self.slots[index].group = Some(group);
            return false;
        }
        if frame > expected || preceding.is_some() {
            // Plans hold only the retained original physical source checkpoint.
            // No original E/status changes until all member plans have succeeded.
            let mut plans: [Option<(
                super::prepared_native_history::SourceReadCheckpoint,
                super::late_start_dsp::DspInputIntent,
            )>; super::constants::MAX_VOICES] = std::array::from_fn(|_| None);
            let next = (|| {
                if self.preparation.preparation_slots() < group.len {
                    return Err(PitchActionReject::Capacity);
                }
                let execution =
                    Self::next_replan(&self.slots[index], frame)?.max(preceding.unwrap_or(0));
                // Keep the original admitted S. Equality preserves the original
                // action order; crossing it rejects all before any plan changes.
                if !mixer.group_capture_current(&group.batch, key, execution) {
                    return Err(PitchActionReject::PreparationFailed);
                }
                for (member, slot_index) in group.slots[..group.len].iter().copied().enumerate() {
                    Self::next_replan(&self.slots[slot_index], frame)?;
                    if execution
                        .checked_sub(self.slots[slot_index].original)
                        .is_none_or(|frames| frames > MAX_LAUNCH_DELAY)
                    {
                        return Err(PitchActionReject::PreparationFailed);
                    }
                    #[cfg(all(test, windows))]
                    super::ordinary_start_tests::preparation_deadline_tests::before_group_late_plan(
                        group.work[member].as_ref().unwrap(),
                    );
                    plans[member] = Some(mixer.plan_ordinary_late_reprepare(
                        group.work[member].as_ref().unwrap(),
                        execution,
                        frame,
                        timeline,
                    )?);
                }
                Ok(execution)
            })();
            let execution = match next {
                Ok(execution) => execution,
                Err(PitchActionReject::PlacementPending | PitchActionReject::WindowPending) => {
                    // Every original still owns its old E/status and exact heavy
                    // owners. The existing callback deferral retries the whole plan.
                    self.slots[index].group = Some(group);
                    return false;
                }
                Err(reason) => {
                    group.reject(reason);
                    mixer.reject_pending_group_capture(&group.batch, key);
                    for slot_index in &group.slots[..group.len] {
                        Self::clear_slot_metadata(&mut self.slots[*slot_index]);
                    }
                    retirement.retire_ordinary_start_group(group);
                    return true;
                }
            };
            let commit = (|| {
                for (member, work) in group.work.iter_mut().enumerate().take(group.len) {
                    let (endpoint, intent) = plans[member].take().unwrap();
                    work.as_mut()
                        .unwrap()
                        .plan_late_reprepare(endpoint, intent, execution)?;
                }
                Ok(())
            })();
            if let Err(reason) = commit {
                // A genuine source/cancellation/CAS failure after a validated
                // prefix refuses the entire group; no partial plan is retried.
                group.reject(reason);
                mixer.reject_pending_group_capture(&group.batch, key);
                for slot_index in &group.slots[..group.len] {
                    Self::clear_slot_metadata(&mut self.slots[*slot_index]);
                }
                retirement.retire_ordinary_start_group(group);
                return true;
            }
            mixer.replan_group_capture(&group.batch, key, execution);
            for (member, slot_index) in group.slots[..group.len].iter().copied().enumerate() {
                self.slots[slot_index].planned = execution;
                self.slots[slot_index].replans += 1;
                self.slots[slot_index].work = Some(PreparedActionWork::Ordinary(
                    group.work[member].take().unwrap(),
                ));
                let published = self
                    .preparation
                    .publish_prepared(&mut self.slots[slot_index].work);
                debug_assert!(published.is_ok());
            }
            self.slots[index].group = Some(group);
            return false;
        }
        for work in group.work.iter_mut().flatten() {
            work.qualify_phase_launch_at(timeline, frame);
        }
        let result = mixer.execute_ordinary_global_start(
            &mut group.work,
            &group.batch,
            key,
            frame,
            messages,
            retirement,
        );
        if matches!(
            result,
            Err(PitchActionReject::PlacementPending | PitchActionReject::WindowPending)
        ) {
            self.slots[index].group = Some(group);
            return false;
        }
        // The header reserved a slot for this complete original group. Feedback,
        // voice or whole-effect retirement capacity failure is terminal; keeping
        // it pending would change the existing all-or-none global contract.
        match result {
            Ok(()) => group.batch.finish(true),
            Err(reason) => {
                group.reject(reason);
                mixer.reject_pending_group_capture(&group.batch, key);
            }
        }
        for index in &group.slots[..group.len] {
            Self::clear_slot_metadata(&mut self.slots[*index]);
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
        timeline: Option<super::transport::TransportTimeline>,
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
            return if self.execute_group(mixer, index, frame, timeline, messages, retirement) {
                ScheduledExecution::Complete
            } else if self.slots[index].planned > frame {
                ScheduledExecution::Replanned {
                    command,
                    frame: self.slots[index].planned,
                }
            } else {
                ScheduledExecution::Pending(command)
            };
        }
        let expired = frame
            .checked_sub(self.slots[index].original)
            .is_some_and(|frames| frames > MAX_LAUNCH_DELAY);
        let preceding = (!expired).then(|| self.preceding_endpoint(index)).flatten();
        let slot = &mut self.slots[index];
        if slot.work.is_none() || retirement.available_retirement_slots() == 0 {
            if frame
                .checked_sub(slot.original)
                .is_some_and(|frames| frames > MAX_LAUNCH_DELAY)
                && let Some(epoch) = &slot.epoch
            {
                // The worker owns every heavy pin until it observes cancellation and returns.
                if let Some(owner) = &slot.owner {
                    owner.reject(PitchActionReject::PreparationFailed);
                }
                epoch.store(0, Ordering::Release);
            }
            return ScheduledExecution::Pending(command);
        }
        let expected = match slot.work.as_ref().unwrap() {
            PreparedActionWork::Ordinary(work) => work.execution_frame(),
            PreparedActionWork::Pitch(work) => work.action.execution_target(),
        };
        if frame < expected && preceding.is_none_or(|previous| previous <= expected) {
            return ScheduledExecution::Replanned {
                command,
                frame: expected,
            };
        }
        if (frame > expected && slot.independent_birth) || preceding.is_some() {
            let next = Self::next_replan(slot, frame).and_then(|execution| {
                if !self.preparation.preparation_available() {
                    return Err(PitchActionReject::Capacity);
                }
                let execution = execution.max(preceding.unwrap_or(0));
                if execution
                    .checked_sub(slot.original)
                    .is_none_or(|frames| frames > MAX_LAUNCH_DELAY)
                {
                    return Err(PitchActionReject::PreparationFailed);
                }
                Self::prepare_late_work(
                    mixer,
                    slot.work.as_mut().unwrap(),
                    execution,
                    frame,
                    timeline,
                )?;
                Ok(execution)
            });
            match next {
                Ok(execution) => {
                    slot.planned = execution;
                    slot.replans += 1;
                    let published = self.preparation.publish_prepared(&mut slot.work);
                    debug_assert!(published.is_ok());
                    return ScheduledExecution::Replanned {
                        command,
                        frame: execution,
                    };
                }
                Err(PitchActionReject::PlacementPending | PitchActionReject::WindowPending) => {
                    // Transient validation made no Work/E/status mutation. Keep
                    // this same original for the existing bounded callback retry.
                    return ScheduledExecution::Pending(command);
                }
                Err(reason) => {
                    slot.work.as_ref().unwrap().reject(reason);
                    let work = slot.work.take().unwrap();
                    Self::clear_slot_metadata(slot);
                    retire(work, retirement);
                    return ScheduledExecution::Complete;
                }
            }
        }
        let result = match slot.work.as_mut().unwrap() {
            PreparedActionWork::Pitch(work) => {
                if work.action.state() == "rejected" {
                    Err(PitchActionReject::PreparationFailed)
                } else {
                    work.launch_qualified =
                        work.launch_qualification
                            .at_effect(work.launch_reference, timeline, frame);
                    mixer.execute_pitch_action(work, frame, messages, retirement)
                }
            }
            PreparedActionWork::Ordinary(work) => {
                if work.origin().state() == "rejected" {
                    Err(PitchActionReject::PreparationFailed)
                } else {
                    work.qualify_phase_launch_at(timeline, frame);
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
        let work = slot.work.take().unwrap();
        Self::clear_slot_metadata(slot);
        retire(work, retirement);
        ScheduledExecution::Complete
    }
}

pub(super) fn service_endpoint_reserve<const CAPACITY: usize>(
    mixer: &mut RtMixer,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback: u64,
) {
    let mut runtime = mixer
        .pitch_runtime
        .take()
        .expect("one callback owns preparation lane");
    runtime.poll();
    runtime.accelerate_completed_starts(mixer, scheduler, callback);
    runtime.endpoint.service(mixer, &mut runtime.preparation);
    mixer.pitch_runtime = Some(runtime);
}

pub(super) fn accept<const CAPACITY: usize>(
    mixer: &mut RtMixer,
    work: Box<PitchActionWork>,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback: u64,
    timeline: Option<super::transport::TransportTimeline>,
    retirement: &mut impl AudioBufferRetirement,
) {
    let mut runtime = mixer
        .pitch_runtime
        .take()
        .expect("one callback owns preparation lane");
    runtime.poll();
    runtime.accept(mixer, work, scheduler, callback, timeline, retirement);
    mixer.pitch_runtime = Some(runtime);
}

pub(super) fn accept_ordinary<const CAPACITY: usize>(
    mixer: &mut RtMixer,
    work: Box<OrdinaryStartWork>,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback: u64,
    timeline: Option<super::transport::TransportTimeline>,
    retirement: &mut impl AudioBufferRetirement,
) {
    let mut runtime = mixer
        .pitch_runtime
        .take()
        .expect("one callback owns preparation lane");
    runtime.poll();
    runtime.accept_ordinary(mixer, work, scheduler, callback, timeline, retirement);
    mixer.pitch_runtime = Some(runtime);
}

pub(super) fn accept_group<const CAPACITY: usize>(
    mixer: &mut RtMixer,
    batch: Arc<GlobalPlaybackBatch>,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback: u64,
    original: u64,
    timeline: Option<super::transport::TransportTimeline>,
    retirement: &mut impl AudioBufferRetirement,
) {
    let mut runtime = mixer
        .pitch_runtime
        .take()
        .expect("one callback owns preparation lane");
    runtime.poll();
    runtime.accept_group(
        mixer, batch, scheduler, callback, original, timeline, retirement,
    );
    mixer.pitch_runtime = Some(runtime);
}

pub(super) fn execute(
    mixer: &mut RtMixer,
    slot: usize,
    sequence: u64,
    frame: u64,
    timeline: Option<super::transport::TransportTimeline>,
    messages: &mut impl AudioMessageSink,
    retirement: &mut impl AudioBufferRetirement,
) -> ScheduledExecution {
    let mut runtime = mixer
        .pitch_runtime
        .take()
        .expect("one callback owns preparation lane");
    let result = runtime.execute(mixer, slot, sequence, frame, timeline, messages, retirement);
    mixer.pitch_runtime = Some(runtime);
    result
}
