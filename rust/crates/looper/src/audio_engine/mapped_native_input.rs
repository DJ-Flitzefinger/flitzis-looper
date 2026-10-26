//! Original-owned mapped input for a declared internal Native stream experiment.
//!
//! This capability retains immutable map/artifact pins, not an attack's mutable Work.
//! It grants canonical input projection only; a declared raw cut is no output-alignment proof.

use super::{NativeHistoryRequest, RubberBandError, SourceReadCheckpoint};
use crate::audio_engine::content_placement::OwnerIdentity;
use crate::audio_engine::mapped_source_clock::MappedClockEpoch;
use crate::audio_engine::native_history_permit::{
    EffectiveNativeVoice, NativeHistoryPermitIdentity, NativeSourceAdmission,
};
use crate::audio_engine::ordinary_start::OrdinaryStartWork;
use crate::audio_engine::pad_start::{MappedStartPendingGuard, MappedStartSource};
use crate::audio_engine::pitch_intent::PitchIntent;
use std::cell::Cell;
use std::sync::Arc;

#[derive(Clone, Copy)]
enum ProjectionRole {
    Birth,
    Current,
}

#[derive(Default)]
struct ProjectionMemoSlot {
    // Read the small key first: a miss never copies the full numeric checkpoint.
    frame: Cell<Option<u64>>,
    result: Cell<Option<SourceReadCheckpoint>>,
}

#[derive(Default)]
struct ProjectionMemo {
    // The role's origin stays in slot zero; slot one retains its most recent other
    // endpoint. Look-ahead eviction is only a performance miss, never new authority.
    slots: [ProjectionMemoSlot; 2],
}

impl ProjectionMemo {
    fn get(&self, frame: u64, origin: u64) -> Option<SourceReadCheckpoint> {
        let slot = &self.slots[usize::from(frame != origin)];
        (slot.frame.get() == Some(frame))
            .then(|| slot.result.get())
            .flatten()
    }

    fn insert(&self, frame: u64, origin: u64, result: SourceReadCheckpoint) {
        let slot = &self.slots[usize::from(frame != origin)];
        slot.result.set(Some(result));
        slot.frame.set(Some(frame));
    }

    fn clear(&self) {
        for slot in &self.slots {
            slot.frame.set(None);
        }
    }
}

pub(crate) struct MappedNativeInput {
    source: Arc<MappedStartSource>,
    pending: MappedStartPendingGuard,
    clock: MappedClockEpoch,
    origin: SourceReadCheckpoint,
    // A claimed live continuation never rewrites the original request or birth proof.
    current_origin: Option<SourceReadCheckpoint>,
    // Numeric Copy results only. Geometry and role origins belong to this unique Box;
    // no source/permit/status result is memoized and the Box is not shared via Sync.
    birth_projection: ProjectionMemo,
    current_projection: ProjectionMemo,
    permit: NativeHistoryPermitIdentity,
    target: u64,
    request_id: u64,
    epoch: u64,
    pitch_intent: PitchIntent,
    key_lock: bool,
    effective_voice: Option<PreparedMappedVoice>,
}

/// Prequalified destination and exact candidate, moved only after the complete birth claim.
pub(crate) struct PreparedMappedVoice {
    voice: EffectiveNativeVoice,
    owner: MappedNativePitchOwner,
    stamp: MappedNativePitchStamp,
}

impl PreparedMappedVoice {
    pub(crate) fn claim_current(
        &self,
        admission: &NativeSourceAdmission,
        lifetime: &Arc<std::sync::atomic::AtomicU64>,
        generation: u64,
        active: bool,
    ) -> bool {
        self.voice
            .claim_current(admission, lifetime, generation, active)
    }
}

/// Comparison only: the worker request or adopted adapter retains all actual owners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MappedNativePitchOwner {
    source: OwnerIdentity,
    permit: NativeHistoryPermitIdentity,
    target: u64,
    request_id: u64,
    epoch: u64,
}

/// Freshness of this one retained adapter, not an independently usable source clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MappedNativePitchStamp {
    pub(super) adapter_id: u64,
    pub(super) native: usize,
    pub(super) incarnation: u64,
    pub(super) input_serial: u64,
    pub(super) actual_frame: u64,
    pub(super) continuation_revision: u64,
}

impl MappedNativePitchStamp {
    pub(super) fn checked_next(self, frames: usize, frozen_birth: bool) -> Option<Self> {
        let count = u64::try_from(frames).ok()?;
        Some(Self {
            input_serial: self.input_serial.checked_add(count)?,
            actual_frame: if frozen_birth {
                self.actual_frame
            } else {
                self.actual_frame.checked_add(count)?
            },
            ..self
        })
    }
}

/// A single readable original span, or explicit silence before that source was born.
/// Private fields prevent a numeric rate or a copied playback from granting this capability.
pub(crate) struct MappedNativePitchChunk {
    owner: MappedNativePitchOwner,
    checkpoint: SourceReadCheckpoint,
    frames: usize,
    pitch: f64,
    frozen_birth: bool,
    intent: PitchIntent,
    key_lock: bool,
    stamp: MappedNativePitchStamp,
    next_stamp: MappedNativePitchStamp,
}

impl MappedNativePitchChunk {
    pub(super) fn owner(&self) -> MappedNativePitchOwner {
        self.owner
    }

    pub(super) fn frames(&self) -> usize {
        self.frames
    }

    pub(super) fn pitch(&self) -> f64 {
        self.pitch
    }

    pub(super) fn is_readable(&self) -> bool {
        self.frozen_birth || self.checkpoint.playback.can_read_frames(self.frames)
    }

    pub(super) fn matches_tuple(&self, intent: PitchIntent, key_lock: bool) -> bool {
        self.intent == intent && self.key_lock == key_lock
    }

    pub(super) fn next_stamp(
        &self,
        current: MappedNativePitchStamp,
    ) -> Option<MappedNativePitchStamp> {
        (self.stamp == current
            && self.checkpoint.playback.mapped_output_frame() == Some(current.actual_frame)
            && current.checked_next(self.frames, self.frozen_birth) == Some(self.next_stamp))
        .then_some(self.next_stamp)
    }
}

impl MappedNativeInput {
    /// Worker/control-only capture from the original frozen ordinary action.
    /// A copied checkpoint or an arbitrary equal-domain geometry cannot construct its owner.
    pub(crate) fn capture(
        work: &OrdinaryStartWork,
        request: &NativeHistoryRequest,
    ) -> Result<Self, RubberBandError> {
        let unavailable = || RubberBandError::UnavailableSourceHistory;
        work.origin()
            .preparation_guard()
            .map_err(|_| unavailable())?;
        let clock = request.playback.mapped_epoch().ok_or_else(unavailable)?;
        let target = work.execution_frame();
        let source = work
            .origin()
            .mapped_source_owner()
            .ok_or_else(unavailable)?;
        let pending = work
            .origin()
            .mapped_pending_guard()
            .map_err(|_| unavailable())?;
        let origin = SourceReadCheckpoint {
            playback: request.playback,
            plan: request.plan,
        };
        if request.mapped_input.is_some()
            || origin.plan.transition.is_active()
            || work.execution_frame() != target
            || work
                .origin()
                .target_frame()
                .is_none_or(|original| original > target)
            || request.target_output_frame != target
            || request.request_id != work.sequence()
            || request.epoch != work.epoch()
            || work
                .late_reprepare_snapshot()
                .is_none_or(|checkpoint| !checkpoint.matches_exact(origin))
            || !request.actual_feed_checkpoint().matches_exact(origin)
            || !request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
            || !request.permit.matches_action_owner(
                work.origin().ownership(),
                work.origin().acknowledgements(),
                work.origin().content_key(),
                &work.epoch,
            )
        {
            return Err(unavailable());
        }
        let input = Self {
            source,
            pending,
            clock,
            origin,
            current_origin: None,
            birth_projection: ProjectionMemo::default(),
            current_projection: ProjectionMemo::default(),
            permit: request.permit.scalar_identity(),
            target,
            request_id: request.request_id,
            epoch: request.epoch,
            pitch_intent: request.pitch_intent,
            key_lock: request.key_lock,
            effective_voice: None,
        };
        // Do not normalize a copied beginning into apparent permission.
        let canonical = input.checkpoint_at(target)?;
        if !origin.matches_exact(canonical) || !origin.is_consistent() {
            return Err(unavailable());
        }
        Ok(input)
    }

    pub(crate) fn capture_pitch(
        work: &crate::audio_engine::pitch_actions::PitchActionWork,
        request: &NativeHistoryRequest,
    ) -> Result<Self, RubberBandError> {
        let unavailable = || RubberBandError::UnavailableSourceHistory;
        let action = &work.action;
        let origin = SourceReadCheckpoint {
            playback: request.playback,
            plan: request.plan,
        };
        if action.variant() != crate::audio_engine::pitch_actions::PitchActionVariant::SetRetrigger
            || work.current_guard().is_err()
            || request.mapped_input.is_some()
            || request.target_output_frame != action.execution_target()
            || request.target_output_frame < action.original_due_frame()
            || request.request_id != action.sequence()
            || request.epoch != action.epoch()
            || !action.matches_history_owner(request)
            || work
                .normal_launch_origin
                .is_none_or(|expected| !expected.matches_exact(origin))
            || !request.actual_feed_checkpoint().matches_exact(origin)
            || !request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
        {
            return Err(unavailable());
        }
        let input = Self {
            source: action.mapped_source_owner().ok_or_else(unavailable)?,
            pending: action.mapped_pending_guard().map_err(|_| unavailable())?,
            clock: request.playback.mapped_epoch().ok_or_else(unavailable)?,
            origin,
            current_origin: None,
            birth_projection: ProjectionMemo::default(),
            current_projection: ProjectionMemo::default(),
            permit: request.permit.scalar_identity(),
            target: request.target_output_frame,
            request_id: request.request_id,
            epoch: request.epoch,
            pitch_intent: request.pitch_intent,
            key_lock: request.key_lock,
            effective_voice: None,
        };
        if !origin.is_consistent() || !input.checkpoint_at(input.target)?.matches_exact(origin) {
            return Err(unavailable());
        }
        Ok(input)
    }

    pub(crate) fn matches_request(&self, request: &NativeHistoryRequest) -> bool {
        request.target_output_frame == self.target
            && request.request_id == self.request_id
            && request.epoch == self.epoch
            && request.permit.scalar_identity() == self.permit
            && request.pitch_intent == self.pitch_intent
            && request.key_lock == self.key_lock
            && self.origin.matches_exact(SourceReadCheckpoint {
                playback: request.playback,
                plan: request.plan,
            })
    }

    /// Pending preparation/adoption checks freshness. An installed birth retains its
    /// separately claimed voice lifetime; it cannot become a new pending candidate.
    pub(crate) fn pending_current(&self) -> bool {
        self.effective_voice.is_none() && self.pending.current(&self.source)
    }

    pub(super) fn stage_effective_voice(
        &self,
        request: &NativeHistoryRequest,
        stamp: MappedNativePitchStamp,
        lifetime: &Arc<std::sync::atomic::AtomicU64>,
        generation: u64,
        active: bool,
        increments: u64,
    ) -> Option<PreparedMappedVoice> {
        if !self.matches_request(request) || !self.pending_current() {
            return None;
        }
        Some(PreparedMappedVoice {
            voice: request.permit.stage_effective_voice(
                &request.sample,
                request.binding.sample_rate_hz,
                lifetime,
                generation,
                active,
                increments,
            )?,
            owner: self.pitch_owner(),
            stamp,
        })
    }

    pub(super) fn staged_voice_matches(
        &self,
        staged: &PreparedMappedVoice,
        stamp: MappedNativePitchStamp,
    ) -> bool {
        self.effective_voice.is_none()
            && self.pitch_owner() == staged.owner
            && stamp == staged.stamp
    }

    pub(super) fn install_effective_voice(&mut self, staged: PreparedMappedVoice) {
        debug_assert!(self.effective_voice.is_none());
        debug_assert_eq!(self.pitch_owner(), staged.owner);
        self.effective_voice = Some(staged);
    }

    pub(super) fn effective_adapter_current(&self, stamp: Option<MappedNativePitchStamp>) -> bool {
        self.effective_voice.as_ref().is_none_or(|installed| {
            stamp.is_some_and(|stamp| {
                stamp.adapter_id == installed.stamp.adapter_id
                    && stamp.native == installed.stamp.native
                    && stamp.incarnation == installed.stamp.incarnation
            })
        })
    }

    pub(super) fn has_effective_voice(&self) -> bool {
        self.effective_voice.is_some()
    }

    pub(super) fn effective_source_current(&self, request: &NativeHistoryRequest) -> bool {
        match &self.effective_voice {
            None => request
                .permit
                .current_effective_source(&request.sample, request.binding.sample_rate_hz),
            Some(installed) => {
                installed.owner == self.pitch_owner()
                    && request.permit.scalar_identity() == self.permit
                    && installed.voice.current(
                        &request.sample,
                        request.binding.sample_rate_hz,
                        request.binding.accepted,
                    )
            }
        }
    }

    pub(super) fn pitch_owner(&self) -> MappedNativePitchOwner {
        MappedNativePitchOwner {
            source: OwnerIdentity::of(&self.source),
            permit: self.permit,
            target: self.target,
            request_id: self.request_id,
            epoch: self.epoch,
        }
    }

    fn owns_request(&self, request: &NativeHistoryRequest, stamp: MappedNativePitchStamp) -> bool {
        request.target_output_frame == self.target
            && request.request_id == self.request_id
            && request.epoch == self.epoch
            && request.permit.scalar_identity() == self.permit
            && request.pitch_intent == self.pitch_intent
            && request.key_lock == self.key_lock
            && request
                .mapped_input
                .as_ref()
                .is_some_and(|input| input.pitch_owner() == self.pitch_owner())
            && self.effective_adapter_current(Some(stamp))
            && self.effective_source_current(request)
    }

    /// Effective reads deliberately do not consult the pending map-job revision.
    /// The caller additionally validates the adapter's actual endpoint and effective KL mode.
    pub(super) fn pitch_chunk(
        &self,
        request: &NativeHistoryRequest,
        checkpoint: SourceReadCheckpoint,
        frames: usize,
        intent: PitchIntent,
        key_lock: bool,
        stamp: MappedNativePitchStamp,
    ) -> Result<MappedNativePitchChunk, RubberBandError> {
        let canonical = self.canonicalize(checkpoint)?;
        let mut bounded = canonical.playback;
        if !self.owns_request(request, stamp)
            || intent != request.pitch_intent
            || canonical.playback.mapped_output_frame() != Some(stamp.actual_frame)
            || !canonical.matches_exact(checkpoint)
            || !canonical.playback.can_read_frames(frames)
            || (frames != 0 && bounded.chunk(frames).0 != frames)
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        let pitch = intent
            .native_pitch_scale(canonical.playback.tempo_ratio(), key_lock)
            .map_err(|_| RubberBandError::InvalidPitchScale)?;
        let next_stamp = stamp
            .checked_next(frames, false)
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        Ok(MappedNativePitchChunk {
            owner: self.pitch_owner(),
            checkpoint: canonical,
            frames,
            pitch,
            frozen_birth: false,
            intent,
            key_lock,
            stamp,
            next_stamp,
        })
    }

    /// No source frame exists in this prefix, so neither map address nor rate advances.
    pub(super) fn frozen_birth_pitch_chunk(
        &self,
        request: &NativeHistoryRequest,
        frames: usize,
        stamp: MappedNativePitchStamp,
    ) -> Result<MappedNativePitchChunk, RubberBandError> {
        if self.effective_voice.is_some()
            || !self.owns_request(request, stamp)
            || self.origin.playback.mapped_output_frame() != Some(stamp.actual_frame)
            || !self.origin.matches_exact(SourceReadCheckpoint {
                playback: request.playback,
                plan: request.plan,
            })
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        let pitch = request
            .pitch_intent
            .native_pitch_scale(self.origin.playback.tempo_ratio(), request.key_lock)
            .map_err(|_| RubberBandError::InvalidPitchScale)?;
        let next_stamp = stamp
            .checked_next(frames, true)
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        Ok(MappedNativePitchChunk {
            owner: self.pitch_owner(),
            checkpoint: self.origin,
            frames,
            pitch,
            frozen_birth: true,
            intent: request.pitch_intent,
            key_lock: request.key_lock,
            stamp,
            next_stamp,
        })
    }

    fn role_origin(&self, role: ProjectionRole) -> SourceReadCheckpoint {
        match role {
            ProjectionRole::Birth => self.origin,
            ProjectionRole::Current => self.current_origin.unwrap_or(self.origin),
        }
    }

    fn project_origin(
        &self,
        role: ProjectionRole,
        frame: u64,
    ) -> Result<SourceReadCheckpoint, RubberBandError> {
        #[cfg(test)]
        let _probe = super::mapped_rate_probe::scope(super::mapped_rate_probe::Stage::Projection);
        let origin = self.role_origin(role);
        let start = origin
            .playback
            .mapped_output_frame()
            .filter(|start| frame >= *start)
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        let memo = match role {
            ProjectionRole::Birth => &self.birth_projection,
            ProjectionRole::Current => &self.current_projection,
        };
        if let Some(result) = memo.get(frame, start) {
            #[cfg(test)]
            super::mapped_rate_probe::record_projection_memo(true);
            return Ok(result);
        }
        #[cfg(test)]
        super::mapped_rate_probe::record_projection_memo(false);
        let result = self.project_origin_uncached(origin, frame)?;
        memo.insert(frame, start, result);
        Ok(result)
    }

    // Keep the original strict factory intact. In particular, even an origin-frame
    // miss must reconstruct the canonical span and validate its original geometry.
    fn project_origin_uncached(
        &self,
        origin: SourceReadCheckpoint,
        frame: u64,
    ) -> Result<SourceReadCheckpoint, RubberBandError> {
        if origin
            .playback
            .mapped_output_frame()
            .is_none_or(|start| frame < start)
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        let mut playback = origin.playback;
        playback
            .project_mapped_compact_to(self.source.geometry(), frame)
            .map_err(|_| RubberBandError::UnavailableSourceHistory)?;
        let position = playback.position();
        Ok(SourceReadCheckpoint {
            playback,
            plan: crate::audio_engine::source_reader::SourceReadPlan {
                frame_pos: position.frame,
                seek_mode: position.seek_mode,
                loop_period: playback.reader_period(),
                ..origin.plan
            },
        })
    }

    fn checkpoint_at(&self, frame: u64) -> Result<SourceReadCheckpoint, RubberBandError> {
        self.project_origin(ProjectionRole::Current, frame)
    }

    /// Historical raw preparation is always evaluated through the immutable birth clock.
    pub(super) fn project_birth(
        &self,
        checkpoint: SourceReadCheckpoint,
        frames: u64,
    ) -> Option<SourceReadCheckpoint> {
        let address = checkpoint.playback.mapped_output_frame()?;
        let original = self.project_origin(ProjectionRole::Birth, address).ok()?;
        if !checkpoint.matches_exact(original) {
            return None;
        }
        self.project_origin(ProjectionRole::Birth, address.checked_add(frames)?)
            .ok()
    }

    pub(super) fn continuation_matches(
        &self,
        previous: SourceReadCheckpoint,
        next: SourceReadCheckpoint,
    ) -> bool {
        let Ok(previous) = self.canonicalize(previous) else {
            return false;
        };
        let Some(epoch) = next.playback.mapped_epoch() else {
            return false;
        };
        let mut expected = previous.playback;
        next.is_consistent()
            && !next.playback.has_phase_program()
            && !next.plan.transition.is_active()
            && previous.plan.matches_source_contract(next.plan)
            && previous.playback.mapped_output_frame() == next.playback.mapped_output_frame()
            && expected
                .replace_mapped_epoch(self.source.geometry(), epoch)
                .is_ok()
            && expected.matches_exact(&next.playback)
    }

    pub(super) fn install_continuation_claimed(&mut self, origin: SourceReadCheckpoint) {
        self.current_projection.clear();
        self.current_origin = Some(origin);
    }

    #[cfg(test)]
    pub(crate) fn projection_memo_clear_for_test(&self) {
        self.birth_projection.clear();
        self.current_projection.clear();
    }

    #[cfg(test)]
    pub(crate) fn projection_memo_keys_for_test(&self) -> [[Option<u64>; 2]; 2] {
        [&self.birth_projection, &self.current_projection]
            .map(|memo| std::array::from_fn(|index| memo.slots[index].frame.get()))
    }

    #[cfg(test)]
    pub(crate) fn projection_pair_for_test(
        &self,
        frame: u64,
        cached: bool,
    ) -> [Result<SourceReadCheckpoint, RubberBandError>; 2] {
        [ProjectionRole::Birth, ProjectionRole::Current].map(|role| {
            if cached {
                self.project_origin(role, frame)
            } else {
                self.project_origin_uncached(self.role_origin(role), frame)
            }
        })
    }

    #[cfg(test)]
    pub(crate) fn projection_memo_layout_for_test() -> (usize, usize) {
        (
            std::mem::size_of::<ProjectionMemoSlot>(),
            std::mem::size_of::<ProjectionMemo>(),
        )
    }

    /// Normalize only a genuine exclusive endpoint from the preceding original span.
    /// Interior impostor spans and changed source selections are never rewritten.
    pub(super) fn canonicalize(
        &self,
        checkpoint: SourceReadCheckpoint,
    ) -> Result<SourceReadCheckpoint, RubberBandError> {
        #[cfg(test)]
        let _probe = super::mapped_rate_probe::scope(super::mapped_rate_probe::Stage::Canonicalize);
        let unavailable = || RubberBandError::UnavailableSourceHistory;
        let address = checkpoint
            .playback
            .mapped_output_frame()
            .ok_or_else(unavailable)?;
        if !checkpoint.is_consistent()
            || checkpoint.plan.transition.is_active()
            || !checkpoint.plan.matches_source_contract(self.origin.plan)
            || checkpoint.playback.mapped_epoch().is_none_or(|clock| {
                !clock.matches_exact(
                    self.current_origin
                        .and_then(|origin| origin.playback.mapped_epoch())
                        .unwrap_or(self.clock),
                )
            })
        {
            return Err(unavailable());
        }
        let canonical = self.checkpoint_at(address)?;
        if checkpoint.matches_exact(canonical) {
            return Ok(canonical);
        }
        let mut endpoint = checkpoint;
        if endpoint.playback.can_read_frames(1)
            || endpoint
                .playback
                .refresh_mapped_span(self.source.geometry())
                .is_err()
            || !endpoint.matches_exact(canonical)
        {
            return Err(unavailable());
        }
        Ok(canonical)
    }

    /// Coordinates only. The caller must separately complete real reads before advancing receipts.
    pub(crate) fn project(
        &self,
        checkpoint: &mut SourceReadCheckpoint,
        frames: u64,
    ) -> Result<(), RubberBandError> {
        let canonical = self.canonicalize(*checkpoint)?;
        let target = canonical
            .playback
            .mapped_output_frame()
            .and_then(|frame| frame.checked_add(frames))
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        let replacement = self.checkpoint_at(target)?;
        *checkpoint = replacement;
        Ok(())
    }

    pub(super) fn same_checkpoint(
        &self,
        left: SourceReadCheckpoint,
        right: SourceReadCheckpoint,
    ) -> bool {
        match (self.canonicalize(left), self.canonicalize(right)) {
            (Ok(left), Ok(right)) => left.matches_exact(right),
            _ => false,
        }
    }
}
