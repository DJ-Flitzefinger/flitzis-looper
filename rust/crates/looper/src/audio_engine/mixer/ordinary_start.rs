//! Ordinary attacks freeze the applied mixer tuple and adopt their own prepared source.

use super::pitch_action::{captured_feedback, placement_rejection};
use super::*;
use crate::audio_engine::audio_stream::AudioMessageSink;
use crate::audio_engine::input_runtime_binding::LaunchWriteKind;
use crate::audio_engine::ordinary_start::OrdinaryStartWork;
use crate::audio_engine::pad_start::PadStartGuard;
use crate::audio_engine::pitch_actions::{FrozenPitchRender, PitchActionReject};
use crate::audio_engine::prepared_native_history::SourceReadCheckpoint;
use crate::messages::AudioMessage;
use std::cell::RefCell;

impl RtMixer {
    pub(super) fn fresh_start_dsp_current(
        &self,
        intent: Option<super::super::late_start_dsp::DspInputIntent>,
        original: u64,
        id: usize,
    ) -> bool {
        intent.is_none_or(|intent| {
            self.input_runtime_ownership.callback_key_slot(intent.key) == Some(id)
                && self.pad_gain_smoothers[id]
                    .constant()
                    .is_some_and(|gain| gain.to_bits() == intent.gain.to_bits())
                && self.pad_dsp_chains[id].matches_late_start_intent(intent, original)
        })
    }

    pub(super) fn ordinary_start_dsp_current(&self, work: &OrdinaryStartWork, id: usize) -> bool {
        work.dsp_intent.is_none_or(|intent| {
            work.origin()
                .target_frame()
                .is_some_and(|original| self.fresh_start_dsp_current(Some(intent), original, id))
        })
    }

    pub(super) fn capture_fresh_start_dsp_intent(
        &mut self,
        key: super::super::content_placement::ContentKey,
        original: u64,
        id: usize,
        callback: u64,
        previous: Option<super::super::late_start_dsp::DspInputIntent>,
    ) -> Result<super::super::late_start_dsp::DspInputIntent, PitchActionReject> {
        self.pad_dsp_chains[id].observe_input_intent(
            key,
            callback,
            self.pad_gain_smoothers[id].constant(),
        );
        self.pad_dsp_chains[id]
            .late_start_intent(key, original)
            .filter(|_| self.fresh_start_dsp_current(previous, original, id))
            .ok_or(PitchActionReject::PreparationFailed)
    }

    fn capture_ordinary_start_dsp_intent(
        &mut self,
        work: &OrdinaryStartWork,
        id: usize,
        callback: u64,
    ) -> Result<super::super::late_start_dsp::DspInputIntent, PitchActionReject> {
        self.capture_fresh_start_dsp_intent(
            work.origin().content_key(),
            work.origin()
                .target_frame()
                .ok_or(PitchActionReject::TimingUnavailable)?,
            id,
            callback,
            work.dsp_intent,
        )
    }

    pub(super) fn ordinary_start_continuity_current(&self, work: &OrdinaryStartWork) -> bool {
        work.launch_delay_frames() == 0 || self.ordinary_start_source_controls_current(work)
    }

    fn ordinary_start_source_controls_current(&self, work: &OrdinaryStartWork) -> bool {
        self.source_control_revision(work.origin().content_key())
            .is_ok_and(|current| work.origin().accepts_source_control_revision(current))
    }

    fn ordinary_loop_region(&self, work: &OrdinaryStartWork, id: usize) -> (f64, Option<f64>) {
        if work.inherits_loop() {
            let rate = f64::from(self.sample_rate_hz);
            (
                self.pad_loop_start_frame[id] as f64 / rate,
                self.pad_loop_end_frame[id].map(|end| end as f64 / rate),
            )
        } else {
            work.loop_region()
        }
    }
    pub(super) fn ordinary_start_guard(
        &self,
        work: &OrdinaryStartWork,
    ) -> Result<PadStartGuard, PitchActionReject> {
        if !work.origin().belongs_to(&self.input_runtime_ownership) {
            return Err(PitchActionReject::StaleContent);
        }
        let guard = work.origin().callback_guard()?;
        if !self.admitted_source_binding_current(guard.id, guard.binding) {
            return Err(PitchActionReject::SourceChanged);
        }
        if !self.ordinary_start_continuity_current(work) {
            return Err(PitchActionReject::TimingChanged);
        }
        Ok(guard)
    }

    pub(super) fn ordinary_start_render(&self, id: usize) -> FrozenPitchRender {
        FrozenPitchRender {
            pitch: self.pad_pitch_intent[id],
            key_lock: self.pad_key_lock_enabled[id],
        }
    }

    pub(super) fn ordinary_start_snapshot(
        &self,
        work: &OrdinaryStartWork,
        id: usize,
    ) -> Result<PitchSourceSnapshot, PitchActionReject> {
        let (start_s, end_s) = self.ordinary_loop_region(work, id);
        let mut snapshot = self.normal_start_snapshot(id, start_s, end_s)?;
        if let Some(geometry) = work.origin().mapped_geometry() {
            let (epoch, original) = work
                .mapped_clock()
                .ok_or(PitchActionReject::MappedTimingUnavailable)?;
            let render = self.ordinary_start_render(id);
            // Native-input chronology alone does not prove alignment to the
            // common output timeline. Keep public activation gated until that
            // per-path contract (including AttackAtGrid) is actually integrated.
            if render.key_lock
                || render.pitch.total_semitones() != 0
                || !self.bpm_lock_enabled
                || epoch.master_period_seconds().map(f64::to_bits)
                    != self.master_period_seconds.map(f64::to_bits)
                || snapshot.plan.loop_region != geometry.domain().region
            {
                return Err(PitchActionReject::MappedTimingUnavailable);
            }
            snapshot.playback = SourcePlayback::mapped(geometry, epoch, original)
                .map_err(|_| PitchActionReject::MappedTimingUnavailable)?;
            let position = snapshot.playback.position();
            snapshot.plan.frame_pos = position.frame;
            snapshot.plan.seek_mode = position.seek_mode;
            snapshot.plan.loop_period = snapshot.playback.loop_period();
        }
        if let Some(origin) = work.late_reprepare_snapshot() {
            if !origin.matches_exact(
                super::super::prepared_native_history::SourceReadCheckpoint {
                    playback: snapshot.playback,
                    plan: snapshot.plan,
                },
            ) {
                return Err(PitchActionReject::PreparedMismatch);
            }
        }
        Ok(snapshot)
    }

    pub(super) fn normal_start_snapshot(
        &self,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
    ) -> Result<PitchSourceSnapshot, PitchActionReject> {
        let (start, end) = self
            .loop_intent_frames(id, start_s, end_s)
            .ok_or(PitchActionReject::InvalidAction)?;
        let sample = self.sample_bank[id]
            .as_ref()
            .ok_or(PitchActionReject::SourceChanged)?;
        let region = effective_loop_region(start, end, sample.frame_count())
            .ok_or(PitchActionReject::InvalidAction)?;
        // Capture the existing source/stem/accepted timing owners once. An ordinary attack
        // declares a physical beginning. Unheard wall-clock delay never advances it.
        let mut snapshot = self
            .capture_pitch_start_snapshot(id, region.start)
            .ok_or(PitchActionReject::TimingUnavailable)?;
        snapshot.playback = SourcePlayback::new(
            region.start,
            ExplicitSeekMode::Normal,
            self.tempo_ratio_for_sample_id(id),
        );
        if !snapshot.playback.configure_domain(
            self.timing_for_sample_id(id)
                .loop_domain(sample.frame_count(), region),
        ) {
            return Err(PitchActionReject::PreparedMismatch);
        }
        snapshot.plan.frame_pos = region.start;
        snapshot.plan.loop_region = region;
        snapshot.plan.loop_period = snapshot.playback.loop_period();
        snapshot.plan.seek_mode = ExplicitSeekMode::Normal;
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
        if snapshot
            .plan
            .required_read_sets(StemReadSets::new(snapshot.stems.as_ref(), None))
            .current
            .is_none()
        {
            snapshot.stems = None;
        }
        Ok(snapshot)
    }

    /// Component tests borrow the real ordinary source snapshot; this grants no
    /// launch admission, mapped permission, state mutation or output alignment.
    #[cfg(test)]
    pub(crate) fn normal_start_snapshot_for_test(
        &self,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
    ) -> Result<PitchSourceSnapshot, PitchActionReject> {
        self.normal_start_snapshot(id, start_s, end_s)
    }

    pub(crate) fn requires_ordinary_start_preparation(
        &self,
        work: &OrdinaryStartWork,
    ) -> Result<bool, PitchActionReject> {
        if !work.validates_render_format(self.sample_rate_hz as u32, self.channels) {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let guard = self.ordinary_start_guard(work)?;
        let render = self.ordinary_start_render(guard.id);
        let snapshot = self.ordinary_start_snapshot(work, guard.id)?;
        Ok(render
            .pitch
            .native_pitch_scale(snapshot.playback.tempo_ratio(), render.key_lock)
            .map_err(|_| PitchActionReject::UnsupportedPitch)?
            != 1.0)
    }

    pub(crate) fn capture_ordinary_start_request(
        &mut self,
        work: &mut OrdinaryStartWork,
        callback_frame: u64,
        execution_frame: u64,
    ) -> Result<(), PitchActionReject> {
        if execution_frame != work.execution_frame() || work.render.is_some() {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let guard = self.ordinary_start_guard(work)?;
        let render = self.ordinary_start_render(guard.id);
        work.freeze_loop(self.ordinary_loop_region(work, guard.id));
        let snapshot = self.ordinary_start_snapshot(work, guard.id)?;
        let launch_origin = super::super::prepared_native_history::SourceReadCheckpoint {
            playback: snapshot.playback,
            plan: snapshot.plan,
        };
        work.freeze_launch_origin(launch_origin)?;
        if work.launch_delay_frames() != 0 {
            work.dsp_intent =
                Some(self.capture_ordinary_start_dsp_intent(work, guard.id, callback_frame)?);
        }
        render
            .pitch
            .native_pitch_scale(snapshot.playback.tempo_ratio(), render.key_lock)
            .map_err(|_| PitchActionReject::UnsupportedPitch)?;
        let request = {
            let permit = work
                .capture_history_permit(&snapshot.sample, snapshot.sample_rate_hz)
                .ok_or(PitchActionReject::SourceChanged)?;
            Some(
                snapshot
                    .into_request(
                        permit,
                        render.pitch,
                        render.key_lock,
                        execution_frame,
                        work.sequence(),
                        work.epoch(),
                    )
                    .ok_or(PitchActionReject::PreparationFailed)?,
            )
        };
        work.capture(request, render)
    }

    /// Validate the current source/control tuple without recapturing its preparation permit.
    /// The new endpoint is projected only from this original's retained T clock.
    pub(crate) fn prepare_ordinary_late_replan(
        &mut self,
        work: &mut OrdinaryStartWork,
        execution: u64,
        callback: u64,
    ) -> Result<(), PitchActionReject> {
        let (endpoint, intent) = self.plan_ordinary_late_reprepare(work, execution, callback)?;
        work.plan_late_reprepare(endpoint, intent, execution)
    }

    /// Validate the retained physical beginning before changing any original E/status.
    pub(crate) fn plan_ordinary_late_reprepare(
        &mut self,
        work: &OrdinaryStartWork,
        execution: u64,
        callback: u64,
    ) -> Result<
        (
            SourceReadCheckpoint,
            super::super::late_start_dsp::DspInputIntent,
        ),
        PitchActionReject,
    > {
        let guard = self.ordinary_start_guard(work)?;
        // A delayed mapped birth needs an owning canonical replay trajectory.
        // Refuse before the scalar endpoint comparison below can relabel it.
        if work.mapped_clock().is_some() {
            return Err(PitchActionReject::MappedTimingUnavailable);
        }
        if work.render != Some(self.ordinary_start_render(guard.id))
            || execution <= work.execution_frame()
        {
            return Err(PitchActionReject::PreparedMismatch);
        }
        if !self.ordinary_start_source_controls_current(work) {
            return Err(PitchActionReject::TimingChanged);
        }
        let origin = work
            .late_reprepare_snapshot()
            .ok_or(PitchActionReject::PreparedMismatch)?;
        let (start_s, end_s) = work.loop_region();
        let snapshot = self.normal_start_snapshot(guard.id, start_s, end_s)?;
        if !origin.matches_exact(
            super::super::prepared_native_history::SourceReadCheckpoint {
                playback: snapshot.playback,
                plan: snapshot.plan,
            },
        ) {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let intent = self.capture_ordinary_start_dsp_intent(work, guard.id, callback)?;
        Ok((origin, intent))
    }

    pub(super) fn ordinary_start_candidate_matches(
        &self,
        work: &OrdinaryStartWork,
        snapshot: &PitchSourceSnapshot,
        index: usize,
        output_frame: u64,
    ) -> bool {
        if !work.native_required() {
            return work.dry_source_matches(snapshot);
        }
        let Some(render) = work.render else {
            return false;
        };
        let feed = ProductiveSourceFeed {
            sample: &snapshot.sample,
            stems: snapshot.stems.as_ref(),
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
        let Some(candidate) = work.candidate.as_ref() else {
            return false;
        };
        self.voices[index].stretch.prepared_pitch_matches(
            candidate,
            &feed,
            work.epoch(),
            work.sequence(),
        )
    }

    pub(crate) fn execute_ordinary_start(
        &mut self,
        work: &mut OrdinaryStartWork,
        output_frame: u64,
        audio_messages: &mut impl AudioMessageSink,
        retirement: &mut impl AudioBufferRetirement,
    ) -> Result<(), PitchActionReject> {
        if output_frame != work.execution_frame() {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let guard = self.ordinary_start_guard(work)?;
        work.rt_ready(output_frame)?;
        let id = guard.id;
        if !self.ordinary_start_dsp_current(work, id) {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let render = work.render.ok_or(PitchActionReject::PreparedMismatch)?;
        if self.ordinary_start_render(id) != render {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let exclusive = work.origin().exclusive();
        let mut active_index = None;
        for (index, voice) in self.voices.iter().enumerate() {
            if voice.active && voice.sample_id == id {
                if active_index.is_some() {
                    return Err(PitchActionReject::PreparedMismatch);
                }
                active_index = Some(index);
            }
        }
        let index = if exclusive {
            0
        } else {
            active_index
                .or_else(|| self.voices.iter().position(|voice| !voice.active))
                .ok_or(PitchActionReject::Capacity)?
        };
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
        let snapshot = self.ordinary_start_snapshot(work, id)?;
        if work.preparation_required() {
            if !self.ordinary_start_candidate_matches(work, &snapshot, index, output_frame) {
                return Err(PitchActionReject::PreparedMismatch);
            }
        } else if render
            .pitch
            .native_pitch_scale(snapshot.playback.tempo_ratio(), render.key_lock)
            .map_err(|_| PitchActionReject::UnsupportedPitch)?
            != 1.0
        {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let retirement_needed =
            1 + if exclusive { 3 * MAX_VOICES } else { 3 } + self.mapped_owner_retirement_slots();
        let feedback_needed = 1 + if exclusive {
            self.voices.iter().filter(|voice| voice.active).count()
        } else {
            0
        };
        if retirement.available_retirement_slots() < retirement_needed
            || audio_messages.available_audio_message_slots() < feedback_needed
        {
            return Err(PitchActionReject::Capacity);
        }
        let (start_s, end_s) = work.loop_region();
        if !self.admitted_trigger_loop_context_available(id, start_s, end_s, guard.binding) {
            return Err(PitchActionReject::WindowPending);
        }
        let (start, end) = self
            .loop_intent_frames(id, start_s, end_s)
            .ok_or(PitchActionReject::InvalidAction)?;
        let mut config = self
            .prepare_admitted_play_sample_rt(
                id,
                work.origin().volume(),
                output_frame,
                guard.binding,
                retirement,
            )
            .ok_or(PitchActionReject::SourceChanged)?;
        config.initial_frame_pos = snapshot.plan.frame_pos;
        let ownership = self.input_runtime_ownership.clone();
        let placement = guard
            .binding
            .placement
            .ok_or(PitchActionReject::StaleContent)?;
        let write = ownership
            .prepare_slot_write_fixed(placement, placement.lifetime())
            .map_err(placement_rejection)?;
        let launch = ownership
            .prepare_launch_write_fixed(placement, LaunchWriteKind::Admit)
            .map_err(placement_rejection)?;
        let config = RefCell::new(Some(config));
        let mixer = RefCell::new(self);
        let work = RefCell::new(work);
        let mut feedback: [Option<(Option<AudioMessage>, usize, bool)>; MAX_VOICES + 1] =
            std::array::from_fn(|_| None);
        let mut feedback_count = 0;
        ownership
            .commit_slot_write_fixed(
                &write,
                || {
                    let mixer = mixer.borrow();
                    let work = work.borrow();
                    work.origin().claim_current(guard)
                        && mixer.admitted_source_binding_current(id, guard.binding)
                        && mixer.ordinary_start_continuity_current(&work)
                        && mixer.ordinary_start_render(id) == render
                        && mixer.ordinary_start_dsp_current(&work, id)
                        && ownership.launch_write_current(launch)
                        && mixer.admit_prepared_source_birth(
                            config.borrow_mut().as_mut().unwrap(),
                            guard.binding,
                            false,
                        )
                        && (!work.preparation_required()
                            || mixer.ordinary_start_candidate_matches(
                                &work,
                                &snapshot,
                                index,
                                output_frame,
                            ))
                        && work.claim_effect()
                },
                |_| {
                    let mut mixer = mixer.borrow_mut();
                    let mut work = work.borrow_mut();
                    ownership.apply_launch_write(launch);
                    let continued_controls = active_index.and_then(|previous_index| {
                        mixer.continued_independent_birth_controls(
                            previous_index,
                            work.origin().content_key(),
                            &snapshot,
                            output_frame,
                        )
                    });
                    if exclusive {
                        for voice_index in 0..MAX_VOICES {
                            if mixer.voices[voice_index].active {
                                let stopped_id = mixer.voices[voice_index].sample_id;
                                feedback[feedback_count] = Some((
                                    captured_feedback(&ownership, stopped_id, false),
                                    stopped_id,
                                    false,
                                ));
                                feedback_count += 1;
                                mixer.pad_dsp_chains[stopped_id].reset();
                                mixer.voices[voice_index].stop_rt(retirement);
                                mixer.publish_key_lock_status(stopped_id);
                            }
                        }
                    }
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
                    voice.mapped_start =
                        snapshot.playback.is_mapped().then(|| work.origin().clone());
                    if let Some(continued_controls) = continued_controls {
                        voice.source_control_epoch = continued_controls;
                    }
                    if work.native_required() {
                        let feed = ProductiveSourceFeed {
                            sample: &snapshot.sample,
                            stems: snapshot.stems.as_ref(),
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
                        let sequence = work.sequence();
                        voice.stretch.adopt_prepared_pitch_claimed(
                            work.candidate
                                .as_mut()
                                .expect("prepared candidate checked before claim"),
                            &feed,
                            sequence,
                        );
                    }
                    // The complete claim proved the frozen actual tuple. Rearm only this
                    // birth's existing first-live-wet guard, without adopting later intent.
                    // A clean prepared owner remains in place: reserve_wet_mode cannot
                    // exchange or reset it merely to rearm feedback for the new voice.
                    mixer.rearm_key_lock_after_voice_start(id, index);
                    feedback[feedback_count] =
                        Some((captured_feedback(&ownership, id, true), id, true));
                    feedback_count += 1;
                    work.origin().mark_effective(output_frame);
                },
            )
            .map_err(placement_rejection)?;
        for (captured, id, started) in feedback.into_iter().take(feedback_count).flatten() {
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
