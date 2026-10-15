//! Frozen pitch application inside the existing same-content placement claim.

use super::*;
use crate::audio_engine::audio_stream::AudioMessageSink;
use crate::audio_engine::content_placement::PlacementError;
use crate::audio_engine::input_runtime_binding::{
    InputPadBinding, LaunchWriteKind, PreparedLaunchWrite,
};
use crate::audio_engine::pitch_actions::{
    PitchActionGuard, PitchActionPreparationMode, PitchActionReject, PitchActionVariant,
    PitchActionWork,
};
use crate::audio_engine::prepared_native_history::{NativeHistoryRequest, PREPARED_HISTORY_FRAMES};
use crate::messages::AudioMessage;
use std::cell::RefCell;

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
    /// Admission can keep an already usable scalar/dry/owned tuple on its original dueframe.
    pub(crate) fn requires_pitch_action_preparation(
        &self,
        work: &PitchActionWork,
    ) -> Result<bool, PitchActionReject> {
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
                let snapshot = self.pitch_action_start_snapshot(work, guard.id)?;
                Ok(render
                    .pitch
                    .native_pitch_scale(snapshot.playback.tempo_ratio(), render.key_lock)
                    .map_err(|_| PitchActionReject::UnsupportedPitch)?
                    != 1.0)
            }
        }
    }

    fn pitch_action_scalar_ready(
        &self,
        work: &PitchActionWork,
        snapshot: &PitchSourceSnapshot,
        index: usize,
    ) -> Result<bool, PitchActionReject> {
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

    fn pitch_action_guard(
        &self,
        work: &PitchActionWork,
    ) -> Result<PitchActionGuard, PitchActionReject> {
        if !work.action.belongs_to(&self.input_runtime_ownership) {
            return Err(PitchActionReject::StaleContent);
        }
        let guard = work.current_guard()?;
        if !self.admitted_source_binding_current(guard.id, guard.binding) {
            return Err(PitchActionReject::SourceChanged);
        }
        Ok(guard)
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

    fn pitch_action_start_snapshot(
        &self,
        work: &PitchActionWork,
        id: usize,
    ) -> Result<PitchSourceSnapshot, PitchActionReject> {
        let (start_s, end_s) = work.action.loop_region();
        let (start, end) = self
            .loop_intent_frames(id, start_s, end_s)
            .ok_or(PitchActionReject::InvalidAction)?;
        let sample = self.sample_bank[id]
            .as_ref()
            .ok_or(PitchActionReject::SourceChanged)?;
        let region = effective_loop_region(start, end, sample.frame_count())
            .ok_or(PitchActionReject::InvalidAction)?;
        let mut snapshot = self
            .capture_pitch_start_snapshot(id, region.start)
            .ok_or(PitchActionReject::TimingUnavailable)?;
        snapshot.playback = SourcePlayback::new(
            region.start,
            ExplicitSeekMode::Normal,
            self.tempo_ratio_for_sample_id(id),
        );
        snapshot.playback.configure_domain(
            self.timing_for_sample_id(id)
                .loop_domain(sample.frame_count(), region),
        );
        snapshot.plan.frame_pos = region.start;
        snapshot.plan.loop_region = region;
        snapshot.plan.loop_period = snapshot.playback.loop_period();
        snapshot.plan.seek_mode = ExplicitSeekMode::Normal;
        // The existing normal retrigger ends the source selection ramp at its own restart.
        snapshot.plan.transition.clear();
        if !super::super::native_source_coverage::normal_loop_feed_available(
            &snapshot.sample,
            snapshot.stems.as_ref(),
            snapshot.sample_rate_hz,
            snapshot.accepted,
            snapshot.plan,
            &snapshot.playback,
        ) {
            return Err(PitchActionReject::WindowPending);
        }
        Ok(snapshot)
    }

    pub(crate) fn capture_pitch_action_request(
        &self,
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
        let mut snapshot = match work.action.variant() {
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
                    work.source_feed_future =
                        Some((snapshot.playback, snapshot.plan, active_delta));
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
                if work.preparation_mode() != PitchActionPreparationMode::Endpoint {
                    return Err(PitchActionReject::PreparedMismatch);
                }
                self.pitch_action_start_snapshot(work, guard.id)?
            }
        };
        let pitch = render
            .pitch
            .native_pitch_scale(snapshot.playback.tempo_ratio(), render.key_lock)
            .map_err(|_| PitchActionReject::UnsupportedPitch)?;
        if pitch == 1.0 && work.action.variant() == PitchActionVariant::SetRetrigger {
            return Ok(None);
        }
        if snapshot
            .plan
            .required_read_sets(StemReadSets::new(snapshot.stems.as_ref(), None))
            .current
            .is_none()
        {
            snapshot.stems = None;
        }
        // Permits use the admitted action's own epoch and current resolved storage window.
        let permit = work
            .capture_history_permit(&snapshot.sample, snapshot.sample_rate_hz)
            .ok_or(PitchActionReject::SourceChanged)?;
        snapshot
            .into_request(
                permit,
                render.pitch,
                render.key_lock,
                execution_frame,
                work.action.sequence(),
                work.action.epoch(),
            )
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
        let render = work.action.render();
        let stems = snapshot
            .plan
            .required_read_sets(StemReadSets::new(snapshot.stems.as_ref(), None))
            .current;
        let feed = ProductiveSourceFeed {
            sample: &snapshot.sample,
            stems,
            outgoing_stems: None,
            sample_rate_hz: snapshot.sample_rate_hz,
            accepted: snapshot.accepted,
            plan: snapshot.plan,
            playback: &snapshot.playback,
            permit: None,
            output_frame: Some(output_frame),
            pitch_intent: render.pitch,
            key_lock: render.key_lock,
        };
        self.voices[index].stretch.prepared_pitch_matches(
            &work.candidate,
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
        let stems = snapshot
            .plan
            .required_read_sets(StemReadSets::new(snapshot.stems.as_ref(), None))
            .current;
        let feed = ProductiveSourceFeed {
            sample: &snapshot.sample,
            stems,
            outgoing_stems: None,
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
            &mut work.candidate,
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
            Some(self.pitch_action_start_snapshot(work, id)?)
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
                    render
                        .pitch
                        .native_pitch_scale(snapshot.playback.tempo_ratio(), render.key_lock)
                        .map_err(|_| PitchActionReject::UnsupportedPitch)?
                        == 1.0
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
                3 * MAX_VOICES
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
            if self.voices[index.unwrap()].generation == u64::MAX {
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
                        && mixer.admitted_source_binding_current(id, guard.binding)
                        && launch.is_none_or(|launch| ownership.launch_write_current(launch))
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
                    }
                    if let (Some(index), Some(snapshot)) = (index, snapshot.as_ref())
                        && work.preparation_required()
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
