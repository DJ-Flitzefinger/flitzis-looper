//! Ordinary attacks freeze the applied mixer tuple and adopt their own prepared source.

use super::pitch_action::{captured_feedback, placement_rejection};
use super::*;
use crate::audio_engine::audio_stream::AudioMessageSink;
use crate::audio_engine::input_runtime_binding::LaunchWriteKind;
use crate::audio_engine::ordinary_start::OrdinaryStartWork;
use crate::audio_engine::pad_start::PadStartGuard;
use crate::audio_engine::pitch_actions::{FrozenPitchRender, PitchActionReject};
use crate::messages::AudioMessage;
use std::cell::RefCell;

impl RtMixer {
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
        self.normal_start_snapshot(id, start_s, end_s)
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
        // starts at its physical loop start at E; preparation time never advances its clock.
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
        &self,
        work: &mut OrdinaryStartWork,
        execution_frame: u64,
    ) -> Result<(), PitchActionReject> {
        if execution_frame != work.execution_frame() || work.render.is_some() {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let guard = self.ordinary_start_guard(work)?;
        let render = self.ordinary_start_render(guard.id);
        work.freeze_loop(self.ordinary_loop_region(work, guard.id));
        let snapshot = self.ordinary_start_snapshot(work, guard.id)?;
        let native_pitch = render
            .pitch
            .native_pitch_scale(snapshot.playback.tempo_ratio(), render.key_lock)
            .map_err(|_| PitchActionReject::UnsupportedPitch)?;
        let request = if native_pitch == 1.0 {
            None
        } else {
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

    pub(super) fn ordinary_start_candidate_matches(
        &self,
        work: &OrdinaryStartWork,
        snapshot: &PitchSourceSnapshot,
        index: usize,
        output_frame: u64,
    ) -> bool {
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
        let retirement_needed = 1 + if exclusive { 3 * MAX_VOICES } else { 3 };
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
                        && mixer.ordinary_start_render(id) == render
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
                    if work.preparation_required() {
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
                    // This attack inherits the actual tuple. It never changes the bank image
                    // or rearms a later requested tuple over the candidate it just adopted.
                    mixer.voice_pitch_intent[index] = render.pitch;
                    mixer.voice_key_lock[index] = render.key_lock;
                    mixer.publish_key_lock_status(id);
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
