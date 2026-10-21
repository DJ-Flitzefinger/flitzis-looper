//! Frozen pitch application inside the existing same-content placement claim.

use super::*;
use crate::audio_engine::audio_stream::AudioMessageSink;
use crate::audio_engine::content_placement::PlacementError;
use crate::audio_engine::input_runtime_binding::{
    InputPadBinding, LaunchWriteKind, PreparedLaunchWrite,
};
use crate::audio_engine::pitch_actions::{
    FrozenPitchRender, PitchActionGuard, PitchActionPreparationMode, PitchActionReject,
    PitchActionVariant, PitchActionWork,
};
use crate::audio_engine::prepared_native_history::{NativeHistoryRequest, PREPARED_HISTORY_FRAMES};
use crate::audio_engine::prepared_source_feed::EndpointResetHistory;
use crate::audio_engine::source_control_epoch::{SourceControlBirth, SourceControlEpoch};
use crate::messages::AudioMessage;
use std::cell::RefCell;

fn retrigger_needs_native(
    render: FrozenPitchRender,
    snapshot: &PitchSourceSnapshot,
) -> Result<bool, PitchActionReject> {
    if snapshot.playback.is_mapped() {
        return Err(PitchActionReject::MappedTimingUnavailable);
    }
    let pitch = render
        .pitch
        .native_pitch_scale(snapshot.playback.tempo_ratio(), render.key_lock)
        .map_err(|_| PitchActionReject::UnsupportedPitch)?;
    // A momentary ON unity point during smoothing is not a dry future trajectory.
    // A fresh endpoint must prove its own rate-epoch and reset-phase history.
    Ok(pitch != 1.0
        || (render.key_lock
            && snapshot.playback.tempo_ratio().to_bits()
                != snapshot.playback.rate_target().to_bits()))
}

pub(super) fn placement_rejection(error: PlacementError) -> PitchActionReject {
    match error {
        PlacementError::Pending => PitchActionReject::PlacementPending,
        PlacementError::CapacityUnavailable => PitchActionReject::Capacity,
        PlacementError::SlotEpochExhausted
        | PlacementError::LayoutRevisionExhausted
        | PlacementError::ObservationGenerationExhausted => PitchActionReject::SequenceExhausted,
        _ => PitchActionReject::StaleContent,
    }
}

pub(super) fn captured_feedback(
    ownership: &super::super::input_runtime_binding::InputRuntimeOwnership,
    id: usize,
    started: bool,
) -> Option<AudioMessage> {
    let diagnostic = if started {
        AudioMessage::SampleStarted { id }
    } else {
        AudioMessage::SampleStopped { id }
    };
    if !ownership.is_source_tracked() {
        return Some(diagnostic);
    }
    let key = ownership.callback_feedback_key(id)?;
    let intent = ownership.transport_intent_for_cell(key.cell());
    let revision = intent.load(Ordering::Acquire);
    (ownership.callback_feedback_key(id) == Some(key) && intent.load(Ordering::Acquire) == revision)
        .then_some(AudioMessage::ContentFeedback {
            id,
            owner: ownership.owner_identity().diagnostic(),
            cell: key.cell().index(),
            lifetime: key.lifetime(),
            transport_revision: revision,
            kind: if started { 1 } else { 2 },
            value: 0.0,
        })
}

impl RtMixer {
    /// Reprepare only the same proved constant independent birth; no live SET/glide replay.
    pub(crate) fn prepare_pitch_late_replan(
        &mut self,
        work: &mut PitchActionWork,
        execution: u64,
        callback: u64,
    ) -> Result<(), PitchActionReject> {
        let guard = self.pitch_action_guard(work)?;
        let origin = work
            .late_reprepare_snapshot()
            .ok_or(PitchActionReject::PreparationFailed)?;
        let endpoint = origin;
        // This current snapshot validates the retained data contract; it never supplies
        // a new source, request, permit or reset origin to the original worker.
        let (start, end) = work.action.loop_region();
        let mut snapshot = self.normal_start_snapshot(guard.id, start, end)?;
        if !snapshot.plan.matches_source_contract(origin.plan)
            || snapshot.playback.tempo_ratio().to_bits() != origin.playback.tempo_ratio().to_bits()
            || snapshot.playback.rate_target().to_bits() != origin.playback.rate_target().to_bits()
        {
            return Err(PitchActionReject::TimingChanged);
        }
        snapshot.playback = endpoint.playback;
        snapshot.plan = endpoint.plan;
        let dsp = self.capture_fresh_start_dsp_intent(
            work.action.content_key(),
            work.action.original_due_frame(),
            guard.id,
            callback,
            work.dsp_intent,
        )?;
        work.plan_late_reprepare(endpoint, dsp, execution)
    }

    /// Admission can keep an already usable scalar/dry/owned tuple on its original dueframe.
    pub(crate) fn requires_pitch_action_preparation(
        &self,
        work: &PitchActionWork,
        projection_frames: u64,
    ) -> Result<bool, PitchActionReject> {
        if !work.validates_render_format(self.sample_rate_hz as u32, self.channels) {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let guard = self.pitch_action_guard(work)?;
        let render = work.action.render();
        match work.action.variant() {
            PitchActionVariant::Set => {
                let Some(index) = self.pitch_action_voice(guard.id, guard.binding)? else {
                    return Ok(false);
                };
                let snapshot = self
                    .capture_pitch_voice_snapshot(index)
                    .ok_or(PitchActionReject::TimingUnavailable)?;
                Ok(!self.pitch_action_scalar_ready(work, &snapshot, index)?)
            }
            PitchActionVariant::SetRetrigger => {
                let snapshot =
                    self.pitch_action_start_snapshot(work, guard.id, projection_frames)?;
                retrigger_needs_native(render, &snapshot)
            }
        }
    }

    fn pitch_action_scalar_ready(
        &self,
        work: &PitchActionWork,
        snapshot: &PitchSourceSnapshot,
        index: usize,
    ) -> Result<bool, PitchActionReject> {
        if snapshot.playback.is_mapped() {
            return Err(PitchActionReject::MappedTimingUnavailable);
        }
        let render = work.action.render();
        let id = self.voices[index].sample_id;
        let same_owned = self.pad_pitch_intent[id] == render.pitch
            && self.pad_key_lock_enabled[id] == render.key_lock
            && self.voices[index]
                .stretch
                .owns_pitch_contract(render.pitch, render.key_lock);
        let dry_unity = render
            .pitch
            .native_pitch_scale(snapshot.playback.tempo_ratio(), render.key_lock)
            .map_err(|_| PitchActionReject::UnsupportedPitch)?
            == 1.0
            && !self.voices[index].stretch.loop_acceptance_state().0;
        Ok(same_owned || dry_unity)
    }

    pub(super) fn pitch_action_guard(
        &self,
        work: &PitchActionWork,
    ) -> Result<PitchActionGuard, PitchActionReject> {
        if !work.action.belongs_to(&self.input_runtime_ownership) {
            return Err(PitchActionReject::StaleContent);
        }
        let guard = work.current_guard()?;
        if !self.mapped_pitch_action_available(guard.id) {
            return Err(PitchActionReject::MappedTimingUnavailable);
        }
        if !self.admitted_source_binding_current(guard.id, guard.binding) {
            return Err(PitchActionReject::SourceChanged);
        }
        if work.source_control_revision.is_some_and(|revision| {
            !self.source_control_revision_matches(work.action.content_key(), revision)
        }) {
            return Err(PitchActionReject::TimingChanged);
        }
        Ok(guard)
    }

    fn mapped_pitch_action_available(&self, id: usize) -> bool {
        !self
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == id && voice.source_playback.is_mapped())
    }

    fn pitch_action_voice(
        &self,
        id: usize,
        binding: InputPadBinding,
    ) -> Result<Option<usize>, PitchActionReject> {
        let mut current = None;
        for (index, voice) in self.voices.iter().enumerate() {
            if !voice.active || voice.sample_id != id {
                continue;
            }
            if current.is_some() {
                return Err(PitchActionReject::PreparedMismatch);
            }
            if !voice.sample.as_ref().is_some_and(|sample| {
                sample.source_address() == binding.source_address
                    && sample.source_sample_count() == binding.sample_count
                    && sample.channels == binding.channels
                    && sample.resident_binding() == binding.resident
            }) || self.timing_for_voice(voice).accepted != binding.accepted
            {
                return Err(PitchActionReject::SourceChanged);
            }
            current = Some(index);
        }
        Ok(current)
    }

    /// A completed independent birth may continue its observed control program,
    /// while every PCM/Native/source-feed lifetime still starts independently.
    pub(super) fn continued_independent_birth_controls(
        &self,
        previous_index: usize,
        key: super::super::content_placement::ContentKey,
        next: &PitchSourceSnapshot,
        output_frame: u64,
    ) -> Option<SourceControlEpoch> {
        let voice = &self.voices[previous_index];
        if !voice.active || voice.paused || next.paused {
            return None;
        }
        // Physical PCM identity alone cannot distinguish two content lifetimes
        // using the same source. Retain this actual voice's admitted content key.
        let previous_key = voice.source_admission.as_ref()?.content_key()?;
        if previous_key != key {
            return None;
        }
        let previous = self.capture_pitch_voice_snapshot(previous_index)?;
        let revision = self.source_control_revision(key).ok()?;
        voice.source_control_epoch.continue_independent_birth(
            output_frame,
            SourceControlBirth {
                key: previous_key,
                checkpoint: super::super::prepared_native_history::SourceReadCheckpoint {
                    playback: previous.playback,
                    plan: previous.plan,
                },
                context: previous.source_feed_context,
                revision,
            },
            SourceControlBirth {
                key,
                checkpoint: super::super::prepared_native_history::SourceReadCheckpoint {
                    playback: next.playback,
                    plan: next.plan,
                },
                context: next.source_feed_context,
                revision,
            },
        )
    }

    pub(super) fn pitch_action_start_snapshot(
        &self,
        work: &PitchActionWork,
        id: usize,
        projection_frames: u64,
    ) -> Result<PitchSourceSnapshot, PitchActionReject> {
        let (start_s, end_s) = work.action.loop_region();
        let mut snapshot = self.normal_start_snapshot(id, start_s, end_s)?;
        let guard = self.pitch_action_guard(work)?;
        let active = match self.pitch_action_voice(id, guard.binding) {
            Ok(active) => active,
            // A fresh source can replace a still audible old source at this slot.
            // Its new launch retains ordinary bank-source start semantics.
            Err(PitchActionReject::SourceChanged) => None,
            Err(reason) => return Err(reason),
        };
        if let Some(index) = active {
            let mut current = self
                .capture_pitch_voice_snapshot(index)
                .ok_or(PitchActionReject::TimingUnavailable)?;
            if !current.project_output_frames(projection_frames) {
                return Err(PitchActionReject::PreparationFailed);
            }
            // A piano attack resets phase, not the accepted active-frame rate clock.
            // Native history remains subject to its existing exact endpoint guard.
            let mut playback = current.playback;
            if !playback.seek(snapshot.plan.loop_region.start, ExplicitSeekMode::Normal)
                || !playback.configure_domain(
                    self.timing_for_sample_id(id)
                        .loop_domain(snapshot.sample.frame_count(), snapshot.plan.loop_region),
                )
            {
                return Err(PitchActionReject::MappedTimingUnavailable);
            }
            snapshot.playback = playback;
            snapshot.plan.loop_period = playback.loop_period();
        }
        if let Some(origin) = work.normal_launch_origin {
            // A constant birth retains its physical start while the live clock
            // validates the accepted rate. Lateness never consumes source PCM.
            if !origin.playback.matches_exact(&snapshot.playback)
                || !origin.plan.matches_source_contract(snapshot.plan)
            {
                return Err(PitchActionReject::PreparedMismatch);
            }
            snapshot.playback = origin.playback;
            snapshot.plan = origin.plan;
        }
        Ok(snapshot)
    }

    pub(crate) fn capture_pitch_action_request(
        &mut self,
        work: &mut PitchActionWork,
        callback_frame: u64,
        execution_frame: u64,
    ) -> Result<Option<NativeHistoryRequest>, PitchActionReject> {
        if !work.capture_available()
            || execution_frame != work.action.execution_target()
            || execution_frame < callback_frame
        {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let guard = self.pitch_action_guard(work)?;
        let render = work.action.render();
        let snapshot = match work.action.variant() {
            PitchActionVariant::Set => {
                let Some(index) = self.pitch_action_voice(guard.id, guard.binding)? else {
                    return Ok(None);
                };
                let mut snapshot = self
                    .capture_pitch_voice_snapshot(index)
                    .ok_or(PitchActionReject::TimingUnavailable)?;
                // Admission already kept scalar/dry/owned SET at its original frame.
                // Capture must make the same decision; execution rechecks it before claim.
                if self.pitch_action_scalar_ready(work, &snapshot, index)? {
                    return Ok(None);
                }
                if work.preparation_mode() == PitchActionPreparationMode::Endpoint {
                    let voice = &self.voices[index];
                    if !voice
                        .source_feed_history
                        .capture_into(&mut work.source_feed)
                    {
                        return Err(PitchActionReject::PreparationFailed);
                    }
                    let delta = execution_frame
                        .checked_sub(callback_frame)
                        .ok_or(PitchActionReject::PreparedMismatch)?;
                    let active_delta = if snapshot.paused { 0 } else { delta };
                    let actual = snapshot.actual_feed_checkpoint();
                    work.source_feed_future = Some((
                        actual.playback,
                        actual.plan,
                        active_delta,
                        snapshot.source_feed_context,
                    ));
                    work.source_feed_voice =
                        Some((voice.history_lifetime.clone(), voice.generation));
                }
                let project_to = match work.preparation_mode() {
                    PitchActionPreparationMode::Endpoint => execution_frame,
                    PitchActionPreparationMode::ForwardHistory => {
                        if snapshot.paused {
                            return Err(PitchActionReject::PreparationFailed);
                        }
                        execution_frame
                            .checked_sub(PREPARED_HISTORY_FRAMES as u64)
                            .ok_or(PitchActionReject::PreparedMismatch)?
                    }
                };
                let delta = project_to
                    .checked_sub(callback_frame)
                    .ok_or(PitchActionReject::PreparedMismatch)?;
                if !snapshot.project_output_frames(delta) {
                    return Err(PitchActionReject::PreparationFailed);
                }
                snapshot
            }
            PitchActionVariant::SetRetrigger => {
                // This admission fence proves no intervening control changes. It does
                // not reconstruct rate history preceding this callback.
                work.source_control_revision =
                    Some(self.source_control_revision(work.action.content_key())?);
                if work.preparation_mode() != PitchActionPreparationMode::Endpoint {
                    return Err(PitchActionReject::PreparedMismatch);
                }
                let delta = execution_frame
                    .checked_sub(callback_frame)
                    .ok_or(PitchActionReject::PreparedMismatch)?;
                let active = match self.pitch_action_voice(guard.id, guard.binding) {
                    Ok(active) => active,
                    // A different old source cannot supply this new source's clock/history.
                    Err(PitchActionReject::SourceChanged) => None,
                    Err(reason) => return Err(reason),
                };
                work.launch_delay_frames = execution_frame
                    .checked_sub(work.action.original_due_frame())
                    .ok_or(PitchActionReject::PreparedMismatch)?;
                if active.is_none() {
                    let (start, end) = work.action.loop_region();
                    let original = self.normal_start_snapshot(guard.id, start, end)?;
                    if original.playback.tempo_ratio().to_bits()
                        == original.playback.rate_target().to_bits()
                    {
                        work.normal_launch_origin = Some(
                            super::super::prepared_native_history::SourceReadCheckpoint {
                                playback: original.playback,
                                plan: original.plan,
                            },
                        );
                    }
                }
                let snapshot = self.pitch_action_start_snapshot(work, guard.id, delta)?;
                if let Some(index) = active {
                    let mut current = self
                        .capture_pitch_voice_snapshot(index)
                        .ok_or(PitchActionReject::TimingUnavailable)?;
                    let gliding = current.playback.tempo_ratio().to_bits()
                        != current.playback.rate_target().to_bits()
                        || snapshot.playback.tempo_ratio().to_bits()
                            != snapshot.playback.rate_target().to_bits();
                    if gliding && work.action.original_due_frame() < callback_frame {
                        // No retained proof for an unobserved past glide is invented
                        // merely because this is a new physical source beginning.
                        return Err(PitchActionReject::PreparationFailed);
                    }
                    if gliding && retrigger_needs_native(render, &snapshot)? {
                        if current.paused
                            || current.readers().is_none_or(|readers| {
                                !super::super::native_source_coverage::reset_source_feed_available(
                                    &current.sample,
                                    readers,
                                    current.sample_rate_hz,
                                    current.accepted,
                                    current.plan,
                                    &current.playback,
                                )
                            })
                            || snapshot.readers().is_none_or(|readers| {
                                !super::super::native_source_coverage::reset_source_feed_available(
                                    &snapshot.sample,
                                    readers,
                                    snapshot.sample_rate_hz,
                                    snapshot.accepted,
                                    snapshot.plan,
                                    &snapshot.playback,
                                )
                            })
                            || current.plan.seek_mode != ExplicitSeekMode::Normal
                            || current.playback.position().seek_mode != ExplicitSeekMode::Normal
                            || current.plan.transition.is_active()
                            || !current.plan.matches_source_contract(snapshot.plan)
                            || !current.playback.matches_domain(snapshot.plan.domain())
                            || current.source_feed_context.binding
                                != snapshot.source_feed_context.binding
                            || !current
                                .source_feed_context
                                .same_source(snapshot.source_feed_context)
                            || current.accepted != guard.binding.accepted
                        {
                            return Err(PitchActionReject::PreparationFailed);
                        }
                        let voice = &self.voices[index];
                        if !voice
                            .source_feed_history
                            .capture_into(&mut work.source_feed)
                        {
                            return Err(PitchActionReject::PreparationFailed);
                        }
                        let actual = current.actual_feed_checkpoint();
                        work.source_feed_future = Some((
                            actual.playback,
                            actual.plan,
                            delta,
                            current.source_feed_context,
                        ));
                        work.source_feed_voice =
                            Some((voice.history_lifetime.clone(), voice.generation));
                        if !current.project_output_frames(delta) {
                            return Err(PitchActionReject::PreparationFailed);
                        }
                        // The journal proves the accepted rate clock at E. The
                        // independent birth's physical phase is already reset at E.
                        let actual = current.actual_feed_checkpoint();
                        work.endpoint_reset_history = Some(EndpointResetHistory {
                            playback: actual.playback,
                            plan: actual.plan,
                        });
                    } else if !gliding {
                        // This is a new physical Normal launch, not the old voice's PCM
                        // history. Its Current action permit and content-control fence
                        // survive independent accepted births, including another at E.
                        if work.action.original_due_frame() < callback_frame {
                            // An endpoint alone says nothing about controls preceding F.
                            // Actual render or separately observed stationary paused controls
                            // certify this past. Neither grants an old-voice lifetime fence.
                            let revision = work
                                .source_control_revision
                                .ok_or(PitchActionReject::PreparationFailed)?;
                            let epoch = &self.voices[index].source_control_epoch;
                            let checkpoint =
                                super::super::prepared_native_history::SourceReadCheckpoint {
                                    playback: current.playback,
                                    plan: current.plan,
                                };
                            let covered = if current.paused {
                                epoch.covers_paused(
                                    work.action.original_due_frame(),
                                    callback_frame,
                                    checkpoint,
                                    current.source_feed_context,
                                    revision,
                                )
                            } else {
                                epoch.covers(
                                    work.action.original_due_frame(),
                                    callback_frame,
                                    checkpoint,
                                    current.source_feed_context,
                                    revision,
                                )
                            };
                            if !covered {
                                return Err(PitchActionReject::PreparationFailed);
                            }
                        }
                        work.normal_launch_origin = Some(
                            super::super::prepared_native_history::SourceReadCheckpoint {
                                playback: snapshot.playback,
                                plan: snapshot.plan,
                            },
                        );
                    }
                }
                snapshot
            }
        };
        render
            .pitch
            .native_pitch_scale(snapshot.playback.tempo_ratio(), render.key_lock)
            .map_err(|_| PitchActionReject::UnsupportedPitch)?;
        if work.action.variant() == PitchActionVariant::SetRetrigger
            && execution_frame != work.action.original_due_frame()
        {
            work.dsp_intent = Some(self.capture_fresh_start_dsp_intent(
                work.action.content_key(),
                work.action.original_due_frame(),
                guard.id,
                callback_frame,
                work.dsp_intent,
            )?);
        }
        // Permits use the admitted action's own epoch and current resolved storage window.
        let permit = work
            .capture_history_permit(&snapshot.sample, snapshot.sample_rate_hz)
            .ok_or(PitchActionReject::SourceChanged)?;
        if work.source_feed_future.is_some() {
            snapshot.into_history_request(
                permit,
                render.pitch,
                render.key_lock,
                execution_frame,
                work.action.sequence(),
                work.action.epoch(),
            )
        } else {
            snapshot.into_request(
                permit,
                render.pitch,
                render.key_lock,
                execution_frame,
                work.action.sequence(),
                work.action.epoch(),
            )
        }
        .map(Some)
        .ok_or(PitchActionReject::PreparationFailed)
    }

    fn pitch_action_candidate_matches(
        &self,
        work: &PitchActionWork,
        snapshot: &PitchSourceSnapshot,
        index: usize,
        output_frame: u64,
    ) -> bool {
        if snapshot.playback.is_mapped()
            || !self.mapped_pitch_action_available(self.voices[index].sample_id)
        {
            return false;
        }
        if !work.native_required() {
            return work.dry_source_matches(snapshot);
        }
        let render = work.action.render();
        let Some(readers) = snapshot.readers() else {
            return false;
        };
        let feed = ProductiveSourceFeed {
            sample: &snapshot.sample,
            stems: readers.current,
            outgoing_stems: readers.outgoing,
            sample_rate_hz: snapshot.sample_rate_hz,
            accepted: snapshot.accepted,
            plan: snapshot.plan,
            playback: &snapshot.playback,
            permit: None,
            output_frame: Some(output_frame),
            pitch_intent: render.pitch,
            key_lock: render.key_lock,
        };
        let Some(candidate) = work.candidate.as_ref() else {
            return false;
        };
        self.voices[index].stretch.prepared_pitch_matches(
            candidate,
            &feed,
            work.action.epoch(),
            work.action.sequence(),
        )
    }

    fn pitch_action_adopt(
        &mut self,
        work: &mut PitchActionWork,
        snapshot: &PitchSourceSnapshot,
        index: usize,
        output_frame: u64,
    ) {
        let render = work.action.render();
        // The pre-claim exact candidate match has already proved these same fixed pins.
        let readers = snapshot
            .readers()
            .expect("claimed pitch retains both reader roles");
        let feed = ProductiveSourceFeed {
            sample: &snapshot.sample,
            stems: readers.current,
            outgoing_stems: readers.outgoing,
            sample_rate_hz: snapshot.sample_rate_hz,
            accepted: snapshot.accepted,
            plan: snapshot.plan,
            playback: &snapshot.playback,
            permit: None,
            output_frame: Some(output_frame),
            pitch_intent: render.pitch,
            key_lock: render.key_lock,
        };
        self.voices[index].stretch.adopt_prepared_pitch_claimed(
            work.candidate
                .as_mut()
                .expect("ready pitch retains its own candidate"),
            &feed,
            work.action.sequence(),
        );
    }

    pub(crate) fn execute_pitch_action(
        &mut self,
        work: &mut PitchActionWork,
        output_frame: u64,
        audio_messages: &mut impl AudioMessageSink,
        retirement: &mut impl AudioBufferRetirement,
    ) -> Result<(), PitchActionReject> {
        if output_frame != work.action.execution_target() {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let guard = self.pitch_action_guard(work)?;
        work.rt_ready(output_frame)?;
        let id = guard.id;
        if !self.fresh_start_dsp_current(work.dsp_intent, work.action.original_due_frame(), id) {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let render = work.action.render();
        let retrigger = work.action.variant() == PitchActionVariant::SetRetrigger;
        if retrigger
            && self
                .voices
                .iter()
                .filter(|voice| voice.active && voice.sample_id == id)
                .count()
                > 1
        {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let active_index = if retrigger {
            self.voices
                .iter()
                .position(|voice| voice.active && voice.sample_id == id)
        } else {
            self.pitch_action_voice(id, guard.binding)?
        };
        let index = if retrigger && work.action.exclusive() {
            Some(0)
        } else {
            active_index.or_else(|| {
                retrigger
                    .then(|| self.voices.iter().position(|voice| !voice.active))
                    .flatten()
            })
        };
        if retrigger && index.is_none() {
            return Err(PitchActionReject::Capacity);
        }
        let snapshot = if retrigger {
            Some(self.pitch_action_start_snapshot(work, id, 0)?)
        } else {
            active_index.and_then(|index| self.capture_pitch_voice_snapshot(index))
        };
        if let (Some(index), Some(snapshot)) = (index, snapshot.as_ref()) {
            if work.preparation_required() {
                if !self.pitch_action_candidate_matches(work, snapshot, index, output_frame) {
                    return Err(PitchActionReject::PreparedMismatch);
                }
            } else {
                let scalar_ready = if retrigger {
                    !retrigger_needs_native(render, snapshot)?
                } else {
                    self.pitch_action_scalar_ready(work, snapshot, index)?
                };
                if !scalar_ready {
                    return Err(PitchActionReject::PreparedMismatch);
                }
            }
        } else if !retrigger && active_index.is_some() {
            return Err(PitchActionReject::SourceChanged);
        }
        if !retrigger && render.pitch.total_semitones() != 0 && !self.key_lock_context_available(id)
        {
            return Err(PitchActionReject::WindowPending);
        }
        let retirement_needed = 1 + if retrigger {
            if work.action.exclusive() {
                3 * MAX_VOICES + self.mapped_owner_retirement_slots()
            } else {
                3
            }
        } else {
            0
        };
        let feedback_needed = if retrigger {
            1 + if work.action.exclusive() {
                self.voices.iter().filter(|voice| voice.active).count()
            } else {
                0
            }
        } else {
            0
        };
        if retirement.available_retirement_slots() < retirement_needed
            || audio_messages.available_audio_message_slots() < feedback_needed
        {
            return Err(PitchActionReject::Capacity);
        }
        let ownership = self.input_runtime_ownership.clone();
        let placement = guard
            .binding
            .placement
            .ok_or(PitchActionReject::StaleContent)?;
        let write = ownership
            .prepare_slot_write_fixed(placement, placement.lifetime())
            .map_err(placement_rejection)?;
        let launch: Option<PreparedLaunchWrite> = if retrigger {
            Some(
                ownership
                    .prepare_launch_write_fixed(placement, LaunchWriteKind::Admit)
                    .map_err(placement_rejection)?,
            )
        } else {
            None
        };
        let loop_intent = if retrigger {
            let (start_s, end_s) = work.action.loop_region();
            Some(
                self.loop_intent_frames(id, start_s, end_s)
                    .ok_or(PitchActionReject::InvalidAction)?,
            )
        } else {
            None
        };
        let config = if retrigger {
            let snapshot = snapshot.as_ref().unwrap();
            let index = index.unwrap();
            let exclusive = work.action.exclusive();
            let selected_increments = 1 + u64::from(exclusive && self.voices[index].active);
            if self.voices[index]
                .generation
                .checked_add(selected_increments)
                .is_none()
                || (exclusive
                    && self
                        .voices
                        .iter()
                        .any(|voice| voice.active && voice.generation == u64::MAX))
            {
                return Err(PitchActionReject::SequenceExhausted);
            }
            if !self.can_adopt_sample(id, retirement) {
                return Err(PitchActionReject::Capacity);
            }
            Some(VoiceStartConfig {
                sample_id: id,
                sample: snapshot.sample.clone(),
                initial_frame_pos: snapshot.plan.frame_pos,
                volume: 1.0,
                initial_tempo_ratio: snapshot.playback.tempo_ratio(),
                start_output_frame: Some(output_frame),
                source_timing: self.timing_for_sample_id(id),
                source_admission: None,
            })
        } else {
            None
        };
        let config = RefCell::new(config);
        let mixer = RefCell::new(self);
        let work = RefCell::new(work);
        let mut feedback: [Option<(Option<AudioMessage>, usize, bool)>; MAX_VOICES + 1] =
            std::array::from_fn(|_| None);
        let mut feedback_count = 0;
        let committed = ownership
            .commit_slot_write_fixed(
                &write,
                || {
                    let mixer = mixer.borrow();
                    let work = work.borrow();
                    work.claim_current(guard)
                        && mixer.mapped_pitch_action_available(id)
                        && mixer.admitted_source_binding_current(id, guard.binding)
                        && work.source_control_revision.is_none_or(|revision| {
                            mixer.source_control_revision_matches(
                                work.action.content_key(),
                                revision,
                            )
                        })
                        && launch.is_none_or(|launch| ownership.launch_write_current(launch))
                        && mixer.fresh_start_dsp_current(
                            work.dsp_intent,
                            work.action.original_due_frame(),
                            id,
                        )
                        && (!retrigger
                            || mixer.admit_prepared_source_birth(
                                config.borrow_mut().as_mut().unwrap(),
                                guard.binding,
                                false,
                            ))
                        && snapshot.as_ref().is_none_or(|snapshot| {
                            !work.preparation_required()
                                || mixer.pitch_action_candidate_matches(
                                    &work,
                                    snapshot,
                                    index.unwrap(),
                                    output_frame,
                                )
                        })
                        && work.claim_effect()
                },
                |_| {
                    let mut mixer = mixer.borrow_mut();
                    let mut work = work.borrow_mut();
                    if let Some(launch) = launch {
                        ownership.apply_launch_write(launch);
                    }
                    if retrigger {
                        // Only the complete accepted effect can carry this scalar
                        // proof. Missing/paused/changed source history keeps the
                        // ordinary new-birth reset below, without a fallback guess.
                        let continued_controls = active_index.and_then(|previous_index| {
                            mixer.continued_independent_birth_controls(
                                previous_index,
                                work.action.content_key(),
                                snapshot.as_ref().unwrap(),
                                output_frame,
                            )
                        });
                        if work.action.exclusive() {
                            for voice_index in 0..MAX_VOICES {
                                if mixer.voices[voice_index].active {
                                    let stopped_id = mixer.voices[voice_index].sample_id;
                                    feedback[feedback_count] = Some((
                                        captured_feedback(&ownership, stopped_id, false),
                                        stopped_id,
                                        false,
                                    ));
                                    feedback_count += 1;
                                    mixer.stop_sample_rt(stopped_id, retirement);
                                }
                            }
                        }
                        let snapshot = snapshot.as_ref().unwrap();
                        let index = index.unwrap();
                        let (start, end) = loop_intent.unwrap();
                        mixer.pad_loop_start_frame[id] = start;
                        mixer.pad_loop_end_frame[id] = end;
                        mixer.stem_transitions[id].clear();
                        mixer.pad_dsp_chains[id].reset();
                        let config = config.borrow_mut().take().unwrap();
                        let voice = &mut mixer.voices[index];
                        if voice.active
                            && voice.sample_id == id
                            && voice
                                .sample
                                .as_ref()
                                .is_some_and(|sample| sample.same_source(&config.sample))
                        {
                            voice.source_timing = config.source_timing;
                            voice.source_admission = config.source_admission;
                            voice.restart(
                                config.initial_frame_pos,
                                config.volume,
                                config.initial_tempo_ratio,
                                config.start_output_frame,
                                retirement,
                            );
                        } else {
                            voice.start_rt(config, retirement);
                        }
                        voice.source_playback = snapshot.playback;
                        if let Some(continued_controls) = continued_controls {
                            voice.source_control_epoch = continued_controls;
                        }
                    }
                    if let (Some(index), Some(snapshot)) = (index, snapshot.as_ref())
                        && work.native_required()
                    {
                        mixer.pitch_action_adopt(&mut work, snapshot, index, output_frame);
                    }
                    // All source/native/context/capacity checks preceded the true claim. These fixed
                    // stores cannot reject half an accepted pitch+attack after a concurrent revocation.
                    mixer.pad_pitch_intent[id] = render.pitch;
                    mixer.pad_key_lock_enabled[id] = render.key_lock;
                    mixer.pad_key_lock_pending_on[id] = false;
                    // A later prospective mode still owns its independent receipt. Its
                    // captured-pitch guard will reject a now-stale tuple on the next poll;
                    // this earlier SET must not erase that request's eventual status.
                    if mixer.pending_key_lock_tuples[id].is_none()
                        && mixer.queued_key_lock_tuples[id].is_none()
                    {
                        mixer.pad_key_lock_request_id[id] = 0;
                        mixer.pad_key_lock_rejected[id] = false;
                    }
                    if let Some(index) = index {
                        mixer.voice_pitch_intent[index] = render.pitch;
                        mixer.voice_key_lock[index] = render.key_lock;
                    }
                    mixer.publish_key_lock_status(id);
                    if retrigger {
                        feedback[feedback_count] =
                            Some((captured_feedback(&ownership, id, true), id, true));
                        feedback_count += 1;
                    }
                    work.action.mark_effective(output_frame);
                },
            )
            .map_err(placement_rejection);
        committed?;
        for entry in feedback.into_iter().take(feedback_count).flatten() {
            let (captured, id, started) = entry;
            audio_messages.push_captured_pad_feedback(
                captured,
                if started {
                    AudioMessage::SampleStarted { id }
                } else {
                    AudioMessage::SampleStopped { id }
                },
            );
        }
        Ok(())
    }
}
