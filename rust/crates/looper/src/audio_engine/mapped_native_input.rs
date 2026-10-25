//! Original-owned mapped input for a declared internal Native stream experiment.
//!
//! This capability retains immutable map/artifact pins, not an attack's mutable Work.
//! It grants canonical input projection only; a declared raw cut is no output-alignment proof.

use super::{NativeHistoryRequest, RubberBandError, SourceReadCheckpoint};
use crate::audio_engine::content_placement::OwnerIdentity;
use crate::audio_engine::mapped_source_clock::MappedClockEpoch;
use crate::audio_engine::native_history_permit::NativeHistoryPermitIdentity;
use crate::audio_engine::ordinary_start::OrdinaryStartWork;
use crate::audio_engine::pad_start::{MappedStartPendingGuard, MappedStartSource};
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::source_playback::SourcePlayback;
use std::sync::Arc;

pub(crate) struct MappedNativeInput {
    source: Arc<MappedStartSource>,
    pending: MappedStartPendingGuard,
    clock: MappedClockEpoch,
    origin: SourceReadCheckpoint,
    permit: NativeHistoryPermitIdentity,
    target: u64,
    request_id: u64,
    epoch: u64,
    pitch_intent: PitchIntent,
    key_lock: bool,
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
        let (clock, target) = work.mapped_clock().ok_or_else(unavailable)?;
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
            || work.origin().target_frame() != Some(target)
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
            permit: request.permit.scalar_identity(),
            target,
            request_id: request.request_id,
            epoch: request.epoch,
            pitch_intent: request.pitch_intent,
            key_lock: request.key_lock,
        };
        // Do not normalize a copied beginning into apparent permission.
        let canonical = input.checkpoint_at(target)?;
        if !origin.matches_exact(canonical) || !origin.is_consistent() {
            return Err(unavailable());
        }
        Ok(input)
    }

    pub(super) fn matches_request(&self, request: &NativeHistoryRequest) -> bool {
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

    /// Pending preparation/adoption checks freshness. Effective reads retain immutable pins
    /// and use the existing NativeHistoryPermit source/OwnACK contract instead.
    pub(super) fn pending_current(&self) -> bool {
        self.pending.current(&self.source)
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

    fn owns_request(&self, request: &NativeHistoryRequest) -> bool {
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
            && request
                .permit
                .current_effective_source(&request.sample, request.binding.sample_rate_hz)
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
        if !self.owns_request(request)
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
        if !self.owns_request(request)
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

    fn checkpoint_at(&self, frame: u64) -> Result<SourceReadCheckpoint, RubberBandError> {
        let playback = SourcePlayback::mapped(self.source.geometry(), self.clock, frame)
            .map_err(|_| RubberBandError::UnavailableSourceHistory)?;
        let position = playback.position();
        Ok(SourceReadCheckpoint {
            playback,
            plan: crate::audio_engine::source_reader::SourceReadPlan {
                frame_pos: position.frame,
                seek_mode: position.seek_mode,
                loop_period: playback.reader_period(),
                ..self.origin.plan
            },
        })
    }

    /// Normalize only a genuine exclusive endpoint from the preceding original span.
    /// Interior impostor spans and changed source selections are never rewritten.
    pub(super) fn canonicalize(
        &self,
        checkpoint: SourceReadCheckpoint,
    ) -> Result<SourceReadCheckpoint, RubberBandError> {
        let unavailable = || RubberBandError::UnavailableSourceHistory;
        let address = checkpoint
            .playback
            .mapped_output_frame()
            .ok_or_else(unavailable)?;
        if !checkpoint.is_consistent()
            || checkpoint.plan.transition.is_active()
            || !checkpoint.plan.matches_source_contract(self.origin.plan)
            || checkpoint
                .playback
                .mapped_epoch()
                .is_none_or(|clock| !clock.matches_exact(self.clock))
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
