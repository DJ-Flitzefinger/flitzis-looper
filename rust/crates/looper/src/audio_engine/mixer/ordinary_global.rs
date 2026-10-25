//! All prepared ordinary participants share the existing global placement transaction.

use super::pitch_action::{captured_feedback, placement_rejection};
use super::*;
use crate::audio_engine::audio_stream::{AudioMessageSink, global_playback_entry_slot};
use crate::audio_engine::global_playback_batch::GlobalPlaybackBatch;
use crate::audio_engine::ordinary_start::{OrdinaryGroupKey, OrdinaryStartWork};
use crate::audio_engine::pad_start::PadStartGuard;
use crate::audio_engine::pitch_actions::{FrozenPitchRender, PitchActionReject};
use crate::messages::AudioMessage;
use std::cell::RefCell;

#[cfg(test)]
thread_local! {
    static CAPTURE_CLAIM_HOOK: RefCell<Option<(bool, Box<dyn FnOnce()>)>> = RefCell::new(None);
}

#[cfg(test)]
fn capture_claim_hook(after_claim: bool) {
    let hook = CAPTURE_CLAIM_HOOK.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|(after, _)| *after == after_claim)
        {
            slot.take().map(|(_, hook)| hook)
        } else {
            None
        }
    });
    if let Some(hook) = hook {
        hook();
    }
}

impl RtMixer {
    #[cfg(test)]
    pub(crate) fn with_group_capture_claim_hook_for_test<T>(
        after_claim: bool,
        hook: impl FnOnce() + 'static,
        action: impl FnOnce() -> T,
    ) -> T {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                CAPTURE_CLAIM_HOOK.with(|slot| *slot.borrow_mut() = None);
            }
        }
        CAPTURE_CLAIM_HOOK.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some((after_claim, Box::new(hook)));
        });
        let _reset = Reset;
        let result = action();
        assert!(
            CAPTURE_CLAIM_HOOK.with(|slot| slot.borrow().is_none()),
            "original capture claim boundary was not reached"
        );
        result
    }
    pub(crate) fn execute_ordinary_global_start(
        &mut self,
        works: &mut [Option<Box<OrdinaryStartWork>>],
        batch: &GlobalPlaybackBatch,
        group_key: OrdinaryGroupKey,
        output_frame: u64,
        audio_messages: &mut impl AudioMessageSink,
        retirement: &mut impl AudioBufferRetirement,
    ) -> Result<(), PitchActionReject> {
        let count = batch.entries.len();
        if !batch.start || count == 0 || count > MAX_VOICES || works.len() != count {
            return Err(PitchActionReject::InvalidAction);
        }
        // Reserve the whole transaction before cloning source pins or claiming any owner.
        if retirement.available_retirement_slots()
            < 3 * MAX_VOICES
                + self.mapped_owner_retirement_slots()
                + 1
                + usize::from(batch.capture_publication.is_some())
            || audio_messages.available_audio_message_slots() < count
        {
            return Err(PitchActionReject::Capacity);
        }
        let mut guards: [Option<PadStartGuard>; MAX_VOICES] = [None; MAX_VOICES];
        let mut renders: [Option<FrozenPitchRender>; MAX_VOICES] = [None; MAX_VOICES];
        let mut snapshots: [Option<PitchSourceSnapshot>; MAX_VOICES] =
            std::array::from_fn(|_| None);
        let mut configs: [Option<VoiceStartConfig>; MAX_VOICES] = std::array::from_fn(|_| None);
        let mut staged_voices: [Option<super::super::prepared_native_history::PreparedMappedVoice>;
            MAX_VOICES] = std::array::from_fn(|_| None);
        let mut voice_indices = [usize::MAX; MAX_VOICES];
        let mut loop_intents = [(0, None); MAX_VOICES];
        let mut seen = [false; NUM_SAMPLES];
        let mut reserved_voices = [false; MAX_VOICES];
        for (position, entry) in batch.entries.iter().enumerate() {
            let work = works[position]
                .as_ref()
                .ok_or(PitchActionReject::PreparedMismatch)?;
            if work.execution_frame() != output_frame
                || work.origin().exclusive()
                || work.origin().loop_region() != (entry.start_s, entry.end_s)
            {
                return Err(PitchActionReject::PreparedMismatch);
            }
            work.rt_ready(output_frame)?;
            let guard = self.ordinary_start_guard(work)?;
            let id = guard.id;
            if !self.ordinary_start_dsp_current(work, id) {
                return Err(PitchActionReject::PreparedMismatch);
            }
            if global_playback_entry_slot(self, entry, true) != Some(id)
                || entry
                    .binding
                    .placement
                    .and_then(|placement| placement.content_key())
                    != Some(work.origin().content_key())
            {
                return Err(PitchActionReject::StaleContent);
            }
            if seen[id] {
                return Err(PitchActionReject::InvalidAction);
            }
            seen[id] = true;
            let render = work.render.ok_or(PitchActionReject::PreparedMismatch)?;
            if self.ordinary_start_render(id) != render {
                return Err(PitchActionReject::PreparedMismatch);
            }
            let mut active_index = None;
            for (index, voice) in self.voices.iter().enumerate() {
                if voice.active && voice.sample_id == id {
                    if active_index.is_some() {
                        return Err(PitchActionReject::PreparedMismatch);
                    }
                    active_index = Some(index);
                }
            }
            let index = active_index
                .or_else(|| {
                    self.voices
                        .iter()
                        .enumerate()
                        .position(|(index, voice)| !voice.active && !reserved_voices[index])
                })
                .ok_or(PitchActionReject::Capacity)?;
            if reserved_voices[index] {
                return Err(PitchActionReject::PreparedMismatch);
            }
            if self.voices[index].generation == u64::MAX {
                return Err(PitchActionReject::SequenceExhausted);
            }
            reserved_voices[index] = true;
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
            if !self.admitted_trigger_loop_context_available(
                id,
                entry.start_s,
                entry.end_s,
                guard.binding,
            ) {
                return Err(PitchActionReject::WindowPending);
            }
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
            if work.native_required() && snapshot.playback.is_mapped() {
                let voice = &self.voices[index];
                staged_voices[position] = Some(
                    work.candidate
                        .as_ref()
                        .ok_or(PitchActionReject::PreparedMismatch)?
                        .stage_mapped_voice(
                            &voice.history_lifetime,
                            voice.generation,
                            voice.active,
                            1,
                        )
                        .ok_or(PitchActionReject::PreparedMismatch)?,
                );
            }
            loop_intents[position] = self
                .loop_intent_frames(id, entry.start_s, entry.end_s)
                .ok_or(PitchActionReject::InvalidAction)?;
            guards[position] = Some(guard);
            renders[position] = Some(render);
            snapshots[position] = Some(snapshot);
            configs[position] = Some(config);
            voice_indices[position] = index;
        }
        let ownership = self.input_runtime_ownership.clone();
        let anchor = guards[0]
            .unwrap()
            .binding
            .placement
            .ok_or(PitchActionReject::StaleContent)?;
        let write = ownership
            .prepare_slot_write_fixed(anchor, anchor.lifetime())
            .map_err(placement_rejection)?;
        if !self.group_capture_current(batch, group_key, output_frame) {
            return Err(PitchActionReject::PreparationFailed);
        }
        // The token exclusively borrows the SAME original Box. Temporarily move
        // custody out of the mixer so the established infallible voice apply may
        // borrow its disjoint mixer state. Every normal result restores custody.
        let attached_capture = batch.capture_publication.is_some();
        let mut capture_owner = if attached_capture {
            self.mix_capture.take()
        } else {
            None
        };
        let capture_work = RefCell::new(capture_owner.as_deref_mut());
        let capture_claim = RefCell::new(None);
        let configs = RefCell::new(configs);
        let staged_voices = RefCell::new(staged_voices);
        let mixer = RefCell::new(self);
        let works = RefCell::new(works);
        let mut feedback: [Option<AudioMessage>; MAX_VOICES] = std::array::from_fn(|_| None);
        let committed = ownership.commit_slot_write_fixed(
            &write,
            || {
                let mixer = mixer.borrow();
                let works = works.borrow();
                let mut configs = configs.borrow_mut();
                // Every original batch/source/tuple/feed proof precedes the first CAS.
                // Global publication already admitted all launches under one marker.
                for (position, entry) in batch.entries.iter().enumerate() {
                    let work = works[position].as_ref().unwrap();
                    let guard = guards[position].unwrap();
                    if global_playback_entry_slot(&mixer, entry, true) != Some(guard.id)
                        || !work.origin().claim_current(guard)
                        || !mixer.admitted_source_binding_current(guard.id, guard.binding)
                        || !mixer.ordinary_start_continuity_current(work)
                        || mixer.ordinary_start_render(guard.id) != renders[position].unwrap()
                        || !mixer.ordinary_start_key_lock_request_current(work, guard.id)
                        || !mixer.ordinary_start_dsp_current(work, guard.id)
                        || !mixer.admit_prepared_source_birth(
                            configs[position].as_mut().unwrap(),
                            guard.binding,
                            false,
                        )
                        || (work.preparation_required()
                            && !mixer.ordinary_start_candidate_matches(
                                work,
                                snapshots[position].as_ref().unwrap(),
                                voice_indices[position],
                                output_frame,
                            ))
                        || staged_voices.borrow()[position]
                            .as_ref()
                            .is_some_and(|staged| {
                                !mixer.mapped_birth_witness_current(
                                    voice_indices[position],
                                    work.candidate.as_ref().unwrap(),
                                    configs[position].as_ref().unwrap(),
                                    staged,
                                )
                            })
                    {
                        return false;
                    }
                }
                for position in 0..count {
                    if !works[position].as_ref().unwrap().claim_effect() {
                        // A later participant can be cancelled after an earlier CAS.
                        // No mixer state has changed; terminally release those claims.
                        for previous in works[..position].iter().flatten() {
                            previous
                                .origin()
                                .abort_claim(PitchActionReject::StaleContent);
                        }
                        return false;
                    }
                }
                if let Some(work) = capture_work.borrow_mut().take() {
                    #[cfg(test)]
                    capture_claim_hook(false);
                    let session_id = work.session_id();
                    let Some(claim) = work.try_claim_start(
                        session_id,
                        output_frame,
                        mixer.sample_rate_hz as u32,
                        mixer.channels,
                    ) else {
                        for previous in works.iter().flatten() {
                            previous
                                .origin()
                                .abort_claim(PitchActionReject::PreparationFailed);
                        }
                        return false;
                    };
                    *capture_claim.borrow_mut() = Some(claim);
                    #[cfg(test)]
                    capture_claim_hook(true);
                }
                true
            },
            |_| {
                let mut mixer = mixer.borrow_mut();
                let mut works = works.borrow_mut();
                let mut configs = configs.borrow_mut();
                for position in 0..count {
                    let work = works[position].as_mut().unwrap();
                    let id = guards[position].unwrap().id;
                    let index = voice_indices[position];
                    let render = renders[position].unwrap();
                    let snapshot = snapshots[position].as_ref().unwrap();
                    let (start, end) = loop_intents[position];
                    mixer.pad_loop_start_frame[id] = start;
                    mixer.pad_loop_end_frame[id] = end;
                    mixer.stem_transitions[id].clear();
                    mixer.pad_dsp_chains[id].reset();
                    let config = configs[position].take().unwrap();
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
                    voice.phase_launch_qualified = work.phase_launch_qualified;
                    voice.phase_birth_unadvanced = true;
                    voice.sync_request = work.origin().sync_request();
                    voice.mapped_birth = snapshot.playback.is_mapped().then(|| {
                        work.origin().birth_observation(
                            output_frame,
                            work.phase_launch_qualified,
                            snapshot.plan.loop_region.start,
                            work.native_required(),
                            work.key_lock_request.unwrap_or(0),
                        )
                    });
                    voice.mapped_start = snapshot.playback.is_mapped().then(|| {
                        super::super::pad_start::PadStartCustody::for_mapped_voice(work.origin())
                    });
                    if work.native_required() {
                        if let Some(staged) = staged_voices.borrow_mut()[position].take() {
                            work.candidate
                                .as_mut()
                                .unwrap()
                                .install_mapped_voice_claimed(staged);
                        }
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
                    // Each claimed birth owns fresh first-live-wet feedback under its
                    // already proved actual tuple; retain its clean prepared Native/FIFO.
                    mixer.rearm_key_lock_after_voice_start(id, index);
                    ownership.publish_ordinary_lifecycle(
                        work.origin().content_key(),
                        crate::audio_engine::ordinary_transport::STARTED,
                        true,
                    );
                    feedback[position] = captured_feedback(&ownership, id, true);
                    work.origin().mark_effective(output_frame);
                }
                if let Some(claim) = capture_claim.borrow_mut().take() {
                    // Permission was linearized after all member claims.
                    // Effective ACK follows every infallible audio store.
                    mixer.mix_capture_pending_start = None;
                    mixer.mix_capture_pending_group = None;
                    claim.publish();
                }
            },
        );
        drop(capture_claim);
        drop(capture_work);
        if attached_capture {
            // Custody restoration is not another audio effect or ACK.
            mixer.borrow_mut().mix_capture = capture_owner;
        }
        committed.map_err(placement_rejection)?;
        #[cfg(test)]
        crate::audio_engine::audio_stream::after_pad_feedback_effect_for_test();
        for position in 0..count {
            audio_messages.push_captured_pad_feedback(
                feedback[position].take(),
                AudioMessage::SampleStarted {
                    id: guards[position].unwrap().id,
                },
            );
        }
        Ok(())
    }
}
