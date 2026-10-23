//! One coalesced global-rate effect on the existing source-history lanes.
//!
//! The previous global tuple remains effective until endangered unity-born voices
//! own prepared history at one old-trajectory endpoint. Only then does normal rate
//! smoothing start. Candidates and their displaced owners stay on existing lanes.

use super::*;
use crate::audio_engine::content_placement::{ContentKey, SlotSnapshot};
use crate::audio_engine::global_rate_status::{RateEffect, RateError, RateState, RateTuple};
use crate::audio_engine::key_lock_preparation::ProspectiveSourceHistory;
use crate::audio_engine::native_history_permit::NativeHistoryPermit;
use crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES;
use crate::audio_engine::source_feed_history::SourceFeedContext;
use crate::audio_engine::transport::TransportTimeline;
use std::cell::RefCell;

#[derive(Clone, Copy)]
struct RateWork {
    epoch: u64,
    request_id: u64,
}

pub(super) struct RateVoice {
    pub(super) id: usize,
    pub(super) placement: SlotSnapshot,
    pub(super) key: ContentKey,
    pub(super) incarnation: u64,
    pub(super) authority: u64,
    pub(super) generation: u64,
    pub(super) paused: bool,
    pub(super) continuity: u64,
    pub(super) pitch: PitchIntent,
    pub(super) key_lock: bool,
    pub(super) binding: ProductiveSourceBinding,
    pub(super) permit: NativeHistoryPermit,
    pub(super) playback: SourcePlayback,
    pub(super) plan: SourceReadPlan,
    pub(super) context: SourceFeedContext,
    needs_history: bool,
    work: Option<RateWork>,
}

struct RateCapture {
    execution: u64,
    timing_fence: u64,
    voices: [Option<RateVoice>; MAX_VOICES],
}

struct PendingRate {
    revision: u64,
    requested: RateTuple,
    previous: RateTuple,
    bpm_lock: bool,
    capture: Option<RateCapture>,
    dry_mapped: bool,
}

#[derive(Default)]
pub(super) struct GlobalRateCoordinator {
    // Sole callback-owned coalescing authority. Refusing an earlier drained
    // prefix does not erase its requested field before the next original edit.
    requested: RateTuple,
    pending: Option<PendingRate>,
}

fn feed<'a>(
    snapshot: &'a PitchSourceSnapshot,
    pitch: PitchIntent,
    key_lock: bool,
    frame: u64,
    permit: Option<&'a NativeHistoryPermit>,
) -> ProductiveSourceFeed<'a> {
    let readers = snapshot.readers().unwrap_or_default();
    ProductiveSourceFeed {
        pitch_intent: pitch,
        key_lock,
        sample: &snapshot.sample,
        stems: readers.current.or(snapshot.stems.as_ref()),
        outgoing_stems: readers.outgoing,
        sample_rate_hz: snapshot.sample_rate_hz,
        accepted: snapshot.accepted,
        playback: &snapshot.playback,
        plan: snapshot.plan,
        permit,
        output_frame: Some(frame),
    }
}

impl RtMixer {
    pub(super) fn effective_global_rate(&self) -> RateTuple {
        RateTuple {
            speed: self.speed,
            period: self.master_period_seconds,
        }
    }

    fn publish_global_rate_effect(
        &self,
        pending: &PendingRate,
        state: RateState,
        error: RateError,
        execution: Option<u64>,
    ) {
        self.input_runtime_ownership
            .global_rate_status
            .publish_effect(RateEffect {
                revision: pending.revision,
                requested: pending.requested,
                effective: self.effective_global_rate(),
                state,
                execution_frame: execution,
                error,
            });
    }

    /// Called once for the parameter drain's final coalesced tuple. No requested
    /// observation is read back as authority for its values.
    pub(crate) fn stage_global_rate(
        &mut self,
        revision: u64,
        speed: Option<f64>,
        period: Option<f64>,
        transport: &mut TransportTimeline,
    ) {
        let Some(mut coordinator) = self.global_rate.take() else {
            return;
        };
        if coordinator
            .pending
            .as_ref()
            .is_some_and(|pending| pending.revision > revision)
        {
            self.global_rate = Some(coordinator);
            return;
        }
        let base = coordinator.requested;
        coordinator.requested = RateTuple {
            speed: speed.unwrap_or(base.speed),
            period: period.or(base.period),
        };
        if let Some(previous) = coordinator.pending.take() {
            self.finish_global_rate(previous, RateError::Superseded);
        }
        let mut pending = PendingRate {
            revision,
            requested: coordinator.requested,
            previous: self.effective_global_rate(),
            bpm_lock: self.bpm_lock_enabled,
            capture: None,
            dry_mapped: self.has_mapped_voice()
                && coordinator.requested != self.effective_global_rate(),
        };
        let valid = pending.requested.speed.is_finite()
            && (SPEED_MIN..=SPEED_MAX).contains(&pending.requested.speed)
            && pending.requested.period.is_none_or(|period| {
                period.is_finite()
                    && period > 0.0
                    && (period * f64::from(self.sample_rate_hz) * 4.0).is_finite()
            });
        if !valid {
            self.finish_global_rate(pending, RateError::InvalidTransport);
        } else if pending.dry_mapped {
            let result = self
                .dry_mapped_rate_cohort_current(pending.requested)
                .and_then(|()| {
                    if !self.global_rate_drains_settled() {
                        return Ok(false);
                    }
                    let frame = transport.output_frame();
                    self.capture_global_rate(&mut pending, frame, frame)?;
                    self.commit_global_rate(&pending, frame, transport)?;
                    Ok(true)
                });
            match result {
                Ok(true) => self.publish_global_rate_effect(
                    &pending,
                    RateState::Applied,
                    RateError::None,
                    Some(transport.output_frame()),
                ),
                Ok(false) => {
                    self.publish_global_rate_effect(
                        &pending,
                        RateState::Preparing,
                        RateError::None,
                        None,
                    );
                    coordinator.pending = Some(pending);
                }
                Err(error) => self.finish_global_rate(pending, error),
            }
        } else {
            match self.global_rate_needs_history(pending.requested) {
                Ok(false) => {
                    let frame = transport.output_frame();
                    match self.apply_global_rate_tuple(&pending, transport, frame) {
                        Ok(()) => self.publish_global_rate_effect(
                            &pending,
                            RateState::Applied,
                            RateError::None,
                            Some(frame),
                        ),
                        Err(error) => self.finish_global_rate(pending, error),
                    }
                }
                Ok(true) => {
                    self.publish_global_rate_effect(
                        &pending,
                        RateState::Preparing,
                        RateError::None,
                        None,
                    );
                    coordinator.pending = Some(pending);
                }
                Err(error) => self.finish_global_rate(pending, error),
            }
        }
        self.global_rate = Some(coordinator);
    }

    pub(super) fn effective_rate_voice_tuple(&self, index: usize) -> (PitchIntent, bool) {
        self.effective_voice_tuple(index)
    }

    fn rate_voice_needs_history(
        &self,
        index: usize,
        requested: RateTuple,
    ) -> Result<bool, RateError> {
        let (pitch, key_lock) = self.effective_rate_voice_tuple(index);
        let target = self
            .tempo_ratio_for_request_with_rate(
                self.voices[index].sync_request,
                self.timing_for_voice(&self.voices[index]).period_seconds(),
                requested.speed,
                requested.period,
            )
            .ok_or(RateError::HistoryUnavailable)?;
        if self.voices[index]
            .stretch
            .owns_pitch_contract(pitch, key_lock)
        {
            return Ok(false);
        }
        pitch
            .native_pitch_scale(target, key_lock)
            .map(|pitch| pitch != 1.0)
            .map_err(|_| RateError::HistoryUnavailable)
    }

    #[cfg(test)]
    pub(crate) fn rate_voice_needs_history_for_test(
        &self,
        index: usize,
        requested: RateTuple,
    ) -> Result<bool, RateError> {
        self.rate_voice_needs_history(index, requested)
    }

    fn global_rate_needs_history(&self, requested: RateTuple) -> Result<bool, RateError> {
        if requested != self.effective_global_rate()
            && self
                .voices
                .iter()
                .any(|voice| voice.active && voice.source_playback.is_mapped())
        {
            // A new master epoch needs the original map's prepared endpoint and
            // every participant's atomic continuity proof; scalar history is insufficient.
            return Err(RateError::HistoryUnavailable);
        }
        for index in 0..MAX_VOICES {
            if self.voices[index].active && self.rate_voice_needs_history(index, requested)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn dry_mapped_rate_cohort_current(&self, requested: RateTuple) -> Result<(), RateError> {
        if self.voices.iter().any(|voice| {
            voice.active
                && voice.source_playback.is_mapped()
                && !voice
                    .sync_request
                    .mapped_matching_enabled(self.bpm_lock_enabled)
        }) || self.master_period_seconds.is_none()
            || requested.period.is_none()
        {
            return Err(RateError::MappedTimingUnavailable);
        }
        for index in 0..MAX_VOICES {
            let voice = &self.voices[index];
            if !voice.active {
                continue;
            }
            let (pitch, key_lock) = self.effective_rate_voice_tuple(index);
            if voice.stretch.has_active_native_ownership()
                || voice.stretch.has_retained_source_input()
                || self.rate_voice_needs_history(index, requested)?
                || (voice.source_playback.is_mapped()
                    && (voice.paused || key_lock || pitch.total_semitones() != 0))
            {
                return Err(RateError::MappedTimingUnavailable);
            }
            let snapshot = self
                .capture_rate_voice_snapshot(index)
                .ok_or(RateError::SourceChanged)?;
            if snapshot.plan.transition.is_active()
                || snapshot.actual_feed.is_some()
                || pitch
                    .native_pitch_scale(snapshot.playback.tempo_ratio(), key_lock)
                    .map_err(|_| RateError::MappedTimingUnavailable)?
                    != 1.0
            {
                return Err(RateError::MappedTimingUnavailable);
            }
            let readers = snapshot
                .readers()
                .ok_or(RateError::MappedTimingUnavailable)?;
            if !super::super::native_source_coverage::normal_loop_feed_available_with_readers(
                &snapshot.sample,
                readers,
                snapshot.sample_rate_hz,
                snapshot.accepted,
                snapshot.plan,
                &snapshot.playback,
            ) {
                return Err(RateError::MappedTimingUnavailable);
            }
        }
        Ok(())
    }

    fn global_rate_snapshot(
        &self,
        index: usize,
        pending: &PendingRate,
    ) -> Option<PitchSourceSnapshot> {
        if pending.dry_mapped {
            self.capture_rate_voice_snapshot(index)
        } else {
            self.capture_pitch_voice_snapshot(index)
        }
    }

    pub(super) fn capture_voice_history_permit(
        &self,
        index: usize,
        snapshot: &PitchSourceSnapshot,
    ) -> Option<NativeHistoryPermit> {
        let voice = &self.voices[index];
        let id = voice.sample_id;
        if self.sample_bank[id]
            .as_ref()
            .is_some_and(|bank| bank.same_source(&snapshot.sample))
        {
            let placement = self.input_runtime_ownership.slot_snapshot(id)?;
            NativeHistoryContext {
                id,
                ownership: &self.input_runtime_ownership,
                acknowledgements: &self.current_timing_acknowledgements,
                preparation_epoch: &self.prepared_source_epochs[placement.cell().index()],
            }
            .capture(&snapshot.sample, snapshot.sample_rate_hz, snapshot.accepted)
        } else {
            NativeVoiceHistoryContext {
                admission: voice.source_admission.as_ref()?,
                lifetime: &voice.history_lifetime,
                generation: voice.generation,
                preparation_epoch: &voice.history_lifetime,
                accepted: snapshot.accepted,
            }
            .capture(&snapshot.sample, snapshot.sample_rate_hz)
        }
    }

    /// A stopped bank reserve needs its real SourceACK. This request instead
    /// belongs to an already living voice: its actual admitted birth can also
    /// prove the same source while all original source/window/timing fences hold.
    /// Keeping PCM, a slot observation or a generic warmed Native cannot do so.
    pub(super) fn rate_voice_source_admitted(
        &self,
        index: usize,
        key: ContentKey,
        snapshot: &PitchSourceSnapshot,
    ) -> bool {
        let voice = &self.voices[index];
        self.input_runtime_ownership
            .callback_feedback_key(voice.sample_id)
            == Some(key)
            || voice.source_admission.as_ref().is_some_and(|admission| {
                admission.content_key() == Some(key)
                    && admission.current_for_birth(
                        &self.input_runtime_ownership,
                        &self.current_timing_acknowledgements,
                        &snapshot.sample,
                        snapshot.sample_rate_hz,
                        false,
                    )
            })
    }

    pub(super) fn global_rate_pending(&self) -> bool {
        self.global_rate
            .as_ref()
            .is_none_or(|coordinator| coordinator.pending.is_some())
    }

    /// Shared fixed participant capture. This does not request work or change a
    /// source clock; callers retain their own existing projection/eligibility rules.
    pub(super) fn capture_rate_participant(
        &self,
        index: usize,
        snapshot: &PitchSourceSnapshot,
    ) -> Result<RateVoice, RateError> {
        let voice = &self.voices[index];
        let id = voice.sample_id;
        let placement = self
            .input_runtime_ownership
            .slot_snapshot(id)
            .ok_or(RateError::SourceChanged)?;
        let key = placement.content_key().ok_or(RateError::SourceChanged)?;
        if !self.rate_voice_source_admitted(index, key, snapshot) {
            return Err(RateError::SourceChanged);
        }
        let permit = self
            .capture_voice_history_permit(index, snapshot)
            .ok_or(RateError::SourceChanged)?;
        let (pitch, key_lock) = self.effective_rate_voice_tuple(index);
        Ok(RateVoice {
            id,
            placement,
            key,
            incarnation: self
                .input_runtime_ownership
                .key_source_request_incarnation(key)
                .ok_or(RateError::SourceChanged)?,
            authority: self
                .input_runtime_ownership
                .authority_for_cell(key.cell())
                .load(Ordering::Acquire),
            generation: voice.generation,
            paused: voice.paused,
            continuity: voice.source_feed_history.continuity_revision(),
            pitch,
            key_lock,
            binding: ProductiveSourceBinding::new(
                &snapshot.sample,
                snapshot.sample_rate_hz,
                snapshot.accepted,
            ),
            permit,
            playback: snapshot.playback,
            plan: snapshot.plan,
            context: snapshot.source_feed_context,
            needs_history: false,
            work: None,
        })
    }

    pub(super) fn rate_participant_current(
        &self,
        index: usize,
        expected: &RateVoice,
    ) -> Result<(), RateError> {
        let voice = &self.voices[index];
        if !voice.active
            || voice.sample_id != expected.id
            || voice.generation != expected.generation
            || voice.paused != expected.paused
            || voice.source_feed_history.continuity_revision() != expected.continuity
            || self.effective_rate_voice_tuple(index) != (expected.pitch, expected.key_lock)
            || self.pending_key_lock_tuples[expected.id].is_some()
            || self.queued_key_lock_tuples[expected.id].is_some()
        {
            return Err(RateError::ParticipantChanged);
        }
        let ownership = &self.input_runtime_ownership;
        if !ownership.slot_snapshot_fields_current(expected.placement)
            || ownership.callback_key_slot(expected.key) != Some(expected.id)
            || ownership.key_source_request_incarnation(expected.key) != Some(expected.incarnation)
            || ownership
                .authority_for_cell(expected.key.cell())
                .load(Ordering::Acquire)
                != expected.authority
        {
            return Err(RateError::SourceChanged);
        }
        let sample = voice.sample.as_ref().ok_or(RateError::SourceChanged)?;
        if ProductiveSourceBinding::new(
            sample,
            self.sample_rate_hz as u32,
            self.timing_for_voice(voice).accepted,
        ) != expected.binding
            || !expected.permit.current(sample, self.sample_rate_hz as u32)
        {
            return Err(RateError::SourceChanged);
        }
        Ok(())
    }

    fn capture_global_rate(
        &mut self,
        pending: &mut PendingRate,
        frame: u64,
        execution: u64,
    ) -> Result<(), RateError> {
        if execution < frame || (pending.dry_mapped && execution != frame) {
            return Err(RateError::MissedBoundary);
        }
        let timing_fence = self
            .input_runtime_ownership
            .global_timing_revision
            .load(Ordering::Acquire);
        let mut capture = RateCapture {
            execution,
            timing_fence,
            voices: std::array::from_fn(|_| None),
        };
        for index in 0..MAX_VOICES {
            let voice = &self.voices[index];
            if !voice.active {
                continue;
            }
            let id = voice.sample_id;
            if self.pending_key_lock_tuples[id].is_some()
                || self.queued_key_lock_tuples[id].is_some()
            {
                return Err(RateError::Capacity);
            }
            let mut snapshot = self
                .global_rate_snapshot(index, pending)
                .ok_or(RateError::SourceChanged)?;
            let mut participant = self.capture_rate_participant(index, &snapshot)?;
            if !snapshot.project_output_frames(execution - frame) {
                return Err(RateError::HistoryUnavailable);
            }
            participant.playback = snapshot.playback;
            participant.plan = snapshot.plan;
            participant.context = participant.context.for_plan(snapshot.plan);
            participant.needs_history = self.rate_voice_needs_history(index, pending.requested)?;
            capture.voices[index] = Some(participant);
        }
        // All foreign owners and reusable storage are checked before the first
        // lane changes epoch. A partial capture retires only our successful work.
        for index in 0..MAX_VOICES {
            if capture.voices[index]
                .as_ref()
                .is_some_and(|voice| voice.needs_history)
                && (self.voices[index].stretch.source_lane_busy()
                    || !self.voices[index].stretch.source_history_work_available())
            {
                return Err(RateError::Capacity);
            }
        }
        pending.capture = Some(capture);
        for index in 0..MAX_VOICES {
            let captured = pending.capture.as_mut().unwrap().voices[index].as_mut();
            let Some(captured) = captured.filter(|voice| voice.needs_history) else {
                continue;
            };
            let snapshot = self
                .capture_pitch_voice_snapshot(index)
                .ok_or(RateError::SourceChanged)?;
            let prospective = feed(
                &snapshot,
                captured.pitch,
                captured.key_lock,
                frame,
                Some(&captured.permit),
            );
            let voice = &mut self.voices[index];
            voice.stretch.invalidate_prepared();
            let Some((epoch, request_id)) = voice.stretch.request_prospective_tuple_with_history(
                &prospective,
                ProspectiveSourceHistory {
                    history: &voice.source_feed_history,
                    voice_lifetime: &voice.history_lifetime,
                    voice_generation: voice.generation,
                    execution_frame: execution,
                    paused: snapshot.paused,
                    actual_feed: snapshot.actual_feed,
                    source_feed_context: snapshot.source_feed_context,
                },
            ) else {
                return Err(RateError::HistoryUnavailable);
            };
            captured.work = Some(RateWork { epoch, request_id });
        }
        self.global_rate_current(pending)
    }

    fn global_rate_current(&self, pending: &PendingRate) -> Result<(), RateError> {
        if pending.dry_mapped {
            self.dry_mapped_rate_cohort_current(pending.requested)?;
        } else if pending.requested != self.effective_global_rate()
            && self
                .voices
                .iter()
                .any(|voice| voice.active && voice.source_playback.is_mapped())
        {
            return Err(RateError::HistoryUnavailable);
        }
        if self
            .input_runtime_ownership
            .global_rate_status
            .requested_revision()
            > pending.revision
        {
            return Err(RateError::Superseded);
        }
        if self.effective_global_rate() != pending.previous
            || self.bpm_lock_enabled != pending.bpm_lock
        {
            return Err(RateError::TimingChanged);
        }
        let Some(capture) = pending.capture.as_ref() else {
            return Ok(());
        };
        if !self.global_rate_drains_settled()
            || self
                .input_runtime_ownership
                .global_timing_revision
                .load(Ordering::Acquire)
                != capture.timing_fence
        {
            return Err(RateError::TimingChanged);
        }
        for index in 0..MAX_VOICES {
            let voice = &self.voices[index];
            let Some(expected) = capture.voices[index].as_ref() else {
                if voice.active {
                    return Err(RateError::ParticipantChanged);
                }
                continue;
            };
            self.rate_participant_current(index, expected)?;
        }
        if self
            .input_runtime_ownership
            .global_timing_revision
            .load(Ordering::Acquire)
            != capture.timing_fence
            || self
                .input_runtime_ownership
                .global_rate_status
                .requested_revision()
                > pending.revision
            || !self.global_rate_drains_settled()
        {
            return Err(RateError::TimingChanged);
        }
        Ok(())
    }

    fn global_rate_ready(&mut self, pending: &PendingRate, frame: u64) -> bool {
        let Some(capture) = pending
            .capture
            .as_ref()
            .filter(|capture| capture.execution == frame)
        else {
            return false;
        };
        if self.global_rate_current(pending).is_err() {
            return false;
        }
        for index in 0..MAX_VOICES {
            let Some(expected) = capture.voices[index].as_ref() else {
                continue;
            };
            let Some(snapshot) = self.global_rate_snapshot(index, pending) else {
                return false;
            };
            if !snapshot.playback.matches_exact(&expected.playback)
                || !snapshot.plan.matches_exact(expected.plan)
                || !snapshot.source_feed_context.same_source(expected.context)
            {
                return false;
            }
            if let Some(work) = expected.work {
                let prospective = feed(&snapshot, expected.pitch, expected.key_lock, frame, None);
                let Some(target) = self.tempo_ratio_for_request_with_rate(
                    self.voices[index].sync_request,
                    self.timing_for_voice(&self.voices[index]).period_seconds(),
                    pending.requested.speed,
                    pending.requested.period,
                ) else {
                    return false;
                };
                if !self.voices[index].stretch.prospective_rate_change_ready(
                    &prospective,
                    work.epoch,
                    work.request_id,
                    target,
                ) {
                    return false;
                }
            } else if self.voices[index].stretch.has_retained_source_input() {
                let prospective = feed(&snapshot, expected.pitch, expected.key_lock, frame, None);
                let Some(target) = self.tempo_ratio_for_request_with_rate(
                    self.voices[index].sync_request,
                    self.timing_for_voice(&self.voices[index]).period_seconds(),
                    pending.requested.speed,
                    pending.requested.period,
                ) else {
                    return false;
                };
                if !self.voices[index]
                    .stretch
                    .retained_rate_change_ready(&prospective, target)
                {
                    return false;
                }
            }
        }
        true
    }

    fn apply_global_rate_tuple(
        &mut self,
        pending: &PendingRate,
        transport: &mut TransportTimeline,
        frame: u64,
    ) -> Result<(), RateError> {
        let tuple = pending.requested;
        let mut candidate = *transport;
        if let Some(period) = tuple.period
            && candidate.master_period_seconds().map(f64::to_bits) != Some(period.to_bits())
            && !candidate.set_master_period_preserving_beat_position_at_frame(period, frame)
        {
            return Err(RateError::InvalidTransport);
        }
        let mut targets = [None; MAX_VOICES];
        for (index, target) in targets.iter_mut().enumerate() {
            let voice = &self.voices[index];
            if !voice.active || !voice.stretch.has_retained_source_input() {
                continue;
            }
            let snapshot = self
                .capture_pitch_voice_snapshot(index)
                .ok_or(RateError::SourceChanged)?;
            let requested = self
                .tempo_ratio_for_request_with_rate(
                    voice.sync_request,
                    self.timing_for_voice(voice).period_seconds(),
                    tuple.speed,
                    tuple.period,
                )
                .ok_or(RateError::HistoryUnavailable)?;
            let (pitch, key_lock) = self.effective_rate_voice_tuple(index);
            let current = feed(&snapshot, pitch, key_lock, frame, None);
            if !voice
                .stretch
                .retained_rate_change_ready(&current, requested)
            {
                return Err(RateError::HistoryUnavailable);
            }
            *target = Some(requested);
        }
        self.global_rate_current(pending)?;
        // Every fallible lookup and participant validation precedes the first store.
        // Only unchanged callback-owned clocks are touched after this boundary.
        for (index, target) in targets.into_iter().enumerate() {
            if let Some(target) = target {
                self.voices[index]
                    .stretch
                    .retarget_retained_rate_claimed(target);
            }
        }
        if self.effective_global_rate() != tuple {
            self.invalidate_source_for_all();
        }
        self.speed = tuple.speed;
        self.master_period_seconds = tuple.period;
        *transport = candidate;
        Ok(())
    }

    fn commit_global_rate(
        &mut self,
        pending: &PendingRate,
        frame: u64,
        transport: &mut TransportTimeline,
    ) -> Result<(), RateError> {
        let mut candidate = *transport;
        if let Some(period) = pending.requested.period
            && candidate.master_period_seconds().map(f64::to_bits) != Some(period.to_bits())
            && !candidate.set_master_period_preserving_beat_position_at_frame(period, frame)
        {
            return Err(RateError::InvalidTransport);
        }
        if !self.global_rate_ready(pending, frame) {
            return Err(RateError::MissedBoundary);
        }
        let mut mapped_replacements = [None; MAX_VOICES];
        if pending.dry_mapped {
            if transport.master_period_seconds().map(f64::to_bits)
                != self.master_period_seconds.map(f64::to_bits)
                || transport.beat_position_at_frame(frame).map(f64::to_bits)
                    != candidate.beat_position_at_frame(frame).map(f64::to_bits)
            {
                return Err(RateError::MappedTimingUnavailable);
            }
            for (index, replacement) in mapped_replacements.iter_mut().enumerate() {
                let voice = &self.voices[index];
                if !voice.active || !voice.source_playback.is_mapped() {
                    continue;
                }
                let geometry = voice
                    .mapped_start
                    .as_ref()
                    .and_then(|owner| owner.mapped_geometry())
                    .ok_or(RateError::MappedTimingUnavailable)?;
                let old_epoch = voice
                    .source_playback
                    .mapped_epoch()
                    .ok_or(RateError::MappedTimingUnavailable)?;
                if voice.source_playback.mapped_output_frame() != Some(frame)
                    || !old_epoch.matches_timeline(transport)
                {
                    return Err(RateError::MappedTimingUnavailable);
                }
                if candidate.matches_clock_epoch(transport) {
                    continue;
                }
                let epoch = old_epoch
                    .with_timeline_at(candidate, frame)
                    .ok_or(RateError::MappedTimingUnavailable)?;
                let mut playback = voice.source_playback;
                playback
                    .replace_mapped_epoch(geometry, epoch)
                    .map_err(|_| RateError::MappedTimingUnavailable)?;
                *replacement = Some(playback);
            }
        }
        let capture = pending.capture.as_ref().unwrap();
        let anchor = capture
            .voices
            .iter()
            .flatten()
            .next()
            .ok_or(RateError::ParticipantChanged)?
            .placement;
        let ownership = self.input_runtime_ownership.clone();
        let write = ownership
            .prepare_slot_write_fixed(anchor, anchor.lifetime())
            .map_err(|_| RateError::SourceChanged)?;
        let mixer = RefCell::new(self);
        ownership
            .commit_slot_write_fixed(
                &write,
                || mixer.borrow_mut().global_rate_ready(pending, frame),
                |_| {
                    let mut mixer = mixer.borrow_mut();
                    for index in 0..MAX_VOICES {
                        let Some(expected) = capture.voices[index].as_ref() else {
                            continue;
                        };
                        let Some(work) = expected.work else {
                            continue;
                        };
                        let snapshot = mixer
                            .capture_pitch_voice_snapshot(index)
                            .expect("claimed old rate endpoint");
                        let prospective =
                            feed(&snapshot, expected.pitch, expected.key_lock, frame, None);
                        mixer.voices[index]
                            .stretch
                            .adopt_prospective_tuple_claimed(&prospective, work.request_id);
                    }
                    for index in 0..MAX_VOICES {
                        let voice = &mixer.voices[index];
                        if !voice.active || !voice.stretch.has_retained_source_input() {
                            continue;
                        }
                        let target = mixer
                            .tempo_ratio_for_request_with_rate(
                                voice.sync_request,
                                mixer.timing_for_voice(voice).period_seconds(),
                                pending.requested.speed,
                                pending.requested.period,
                            )
                            .expect("complete rate claim validated every scalar policy ratio");
                        mixer.voices[index]
                            .stretch
                            .retarget_retained_rate_claimed(target);
                    }
                    // Actual source controls change only after every endangered owner is adopted.
                    // Invalidation cannot revoke those now-productive Native/FIFO trajectories.
                    mixer.invalidate_source_for_all();
                    for (index, replacement) in mapped_replacements.into_iter().enumerate() {
                        if let Some(replacement) = replacement {
                            mixer.voices[index].source_playback = replacement;
                        }
                    }
                    mixer.speed = pending.requested.speed;
                    mixer.master_period_seconds = pending.requested.period;
                    *transport = candidate;
                },
            )
            .map(|_| ())
            .map_err(|_| RateError::SourceChanged)
    }

    fn finish_global_rate(&mut self, pending: PendingRate, error: RateError) {
        if let Some(capture) = pending.capture.as_ref() {
            for (index, expected) in capture.voices.iter().enumerate() {
                if let Some(expected) = expected
                    && self.voices[index].generation == expected.generation
                    && let Some(work) = expected.work
                {
                    self.voices[index]
                        .stretch
                        .cancel_source_request_if_owned(work.epoch, work.request_id);
                }
            }
        }
        self.publish_global_rate_effect(
            &pending,
            RateState::Rejected,
            error,
            pending.capture.as_ref().map(|capture| capture.execution),
        );
    }

    /// Services only the existing exact render boundary; it creates no scheduled event.
    pub(crate) fn service_global_rate(
        &mut self,
        frame: u64,
        transport: &mut TransportTimeline,
        _retirement: &mut impl AudioBufferRetirement,
    ) {
        let Some(mut coordinator) = self.global_rate.take() else {
            return;
        };
        let Some(mut pending) = coordinator.pending.take() else {
            self.global_rate = Some(coordinator);
            return;
        };
        let result = self.global_rate_current(&pending).and_then(|()| {
            if pending.capture.is_none() {
                if !self.global_rate_drains_settled() {
                    return Ok(false);
                }
                let execution = if pending.dry_mapped {
                    frame
                } else {
                    frame
                        .checked_add(PREPARED_HISTORY_FRAMES as u64)
                        .ok_or(RateError::MissedBoundary)?
                };
                self.capture_global_rate(&mut pending, frame, execution)?;
                self.publish_global_rate_effect(
                    &pending,
                    RateState::Preparing,
                    RateError::None,
                    pending.capture.as_ref().map(|capture| capture.execution),
                );
            }
            let execution = pending.capture.as_ref().unwrap().execution;
            if frame < execution {
                return Ok(false);
            }
            if frame != execution {
                return Err(RateError::MissedBoundary);
            }
            self.commit_global_rate(&pending, frame, transport)?;
            Ok(true)
        });
        match result {
            Ok(true) => self.publish_global_rate_effect(
                &pending,
                RateState::Applied,
                RateError::None,
                Some(frame),
            ),
            Ok(false) => coordinator.pending = Some(pending),
            Err(error) => self.finish_global_rate(pending, error),
        }
        self.global_rate = Some(coordinator);
    }

    pub(crate) fn next_global_rate_frame(&self) -> Option<u64> {
        self.global_rate
            .as_ref()?
            .pending
            .as_ref()?
            .capture
            .as_ref()
            .map(|capture| capture.execution)
    }

    pub(crate) fn has_pending_global_rate(&self) -> bool {
        self.global_rate
            .as_ref()
            .is_some_and(|coordinator| coordinator.pending.is_some())
    }

    /// Legacy effects have no held rate request. Their actual callback state is
    /// the next coalescing base, including an explicitly cleared master period.
    pub(super) fn refresh_global_rate_intent(&mut self) {
        let effective = self.effective_global_rate();
        if let Some(coordinator) = self.global_rate.as_mut()
            && coordinator.pending.is_none()
        {
            coordinator.requested = effective;
        }
    }

    /// Legacy global edits have no separately held complete rate tuple. Refuse both
    /// pending effects instead of refreshing an old captured fence or applying half.
    pub(crate) fn refuse_legacy_global_rate_if_pending(&mut self) -> bool {
        let Some(mut coordinator) = self.global_rate.take() else {
            return false;
        };
        let refused = if let Some(pending) = coordinator.pending.take() {
            self.finish_global_rate(pending, RateError::OtherRatePending);
            true
        } else {
            false
        };
        self.global_rate = Some(coordinator);
        refused
    }
}
