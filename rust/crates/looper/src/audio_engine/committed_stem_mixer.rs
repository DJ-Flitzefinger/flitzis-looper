//! Callback half of sealed residency and new-set candidate transactions.
use super::*;
use crate::audio_engine::committed_stem_residency::{
    CommittedStemRtWork, SelectedStemIdentity, SelectionPermit, StemModeReceipt,
    StemResidencyTransaction, StemVoiceCapture, StemVoicePin,
};
use crate::audio_engine::native_source_coverage::{
    complete_view_available, normal_loop_feed_available, normal_loop_feed_available_with_readers,
};
use crate::audio_engine::source_reader::StemReadSets;
use std::sync::atomic::Ordering;

fn candidate_residency_available(sample: &SampleBuffer) -> bool {
    sample
        .residency
        .as_ref()
        .is_some_and(|view| match view.context {
            crate::messages::ResidentContext::FiniteLoop
            | crate::messages::ResidentContext::KeyLockFiniteLoop => true,
            crate::messages::ResidentContext::FullTrack
            | crate::messages::ResidentContext::KeyLockFullTrack => complete_view_available(sample),
        })
}

fn committed_stem_mode_available(
    mode: ExplicitSeekMode,
    candidate: bool,
    paused: bool,
    key_lock: bool,
    accepted: Option<crate::audio_engine::constant_timing::AcceptedTimingProjection>,
    sample: &SampleBuffer,
) -> bool {
    mode == ExplicitSeekMode::Normal
        || (candidate
            && accepted.is_none()
            && (mode == ExplicitSeekMode::AfterLoop
                || (mode == ExplicitSeekMode::BeforeLoop && !paused))
            && sample.residency.as_ref().is_some_and(|view| {
                if key_lock {
                    view.context == crate::messages::ResidentContext::KeyLockFiniteLoop
                } else {
                    view.context == crate::messages::ResidentContext::FullTrack
                        && complete_view_available(sample)
                }
            }))
}

impl RtMixer {
    fn committed_selection_matches(&self, id: usize, selection: &SelectionPermit) -> bool {
        id < NUM_SAMPLES
            && selection.current()
            && self.committed_stem_selection[id].is_some_and(|accepted| {
                accepted.identity == selection.identity
                    && accepted.source_generation == selection.source_generation
                    && accepted.epoch == selection.expected
            })
    }

    fn capture_committed_stem_voice(&self, capture: &StemVoiceCapture) -> Option<StemVoicePin> {
        let id = capture.id;
        if id >= NUM_SAMPLES
            || !capture.status.publication.current()
            || !self.source_binding_current(id, capture.binding)
            || !self
                .input_runtime_ownership
                .launch_current(id, capture.launch_revision)
            || (capture.target != StemMixMode::FullMix
                && !if let Some(candidate) = &capture.candidate {
                    candidate.current()
                        && !candidate.adopted()
                        && !capture.selection.revoked.load(Ordering::Acquire)
                        && self.committed_stem_selection[id].map_or(0, |selection| selection.epoch)
                            == candidate.predecessor_selection_expected
                } else {
                    self.committed_selection_matches(id, &capture.selection)
                })
        {
            return None;
        }
        let bank = self.sample_bank[id].as_ref()?;
        if capture.candidate.is_some() && !candidate_residency_available(bank) {
            return None;
        }
        let selection = self.stem_render_selection(id);
        let transition = self.stem_transitions[id];
        let stems = self.prepared_stems[id].as_ref();
        if capture.candidate.is_some()
            && (transition.is_active() || self.outgoing_stems[id].is_some())
        {
            return None;
        }
        if capture.candidate.is_none()
            && stems.is_some_and(|stems| *stems.complete_set_identity != capture.selection.identity)
        {
            return None;
        }
        if capture.target == StemMixMode::AllStems
            && stems.is_none()
            && (selection != StemRenderSelection::full_mix() || transition.is_active())
        {
            return None;
        }
        let mut active = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| voice.active && voice.sample_id == id);
        let voice = active.next();
        if active.next().is_some() {
            return None;
        }
        let paused = voice.is_some_and(|(_, voice)| voice.paused);
        let (sample, index, generation, timing, mut playback, region, history_epoch) =
            if let Some((index, voice)) = voice {
                let sample = voice.sample.as_ref()?;
                if !sample.same_window(bank)
                    || voice.frozen_stems.is_some()
                    || !committed_stem_mode_available(
                        voice.explicit_seek_mode,
                        capture.candidate.is_some(),
                        voice.paused,
                        self.pad_key_lock_enabled[id],
                        self.timing_for_voice(voice).accepted,
                        sample,
                    )
                {
                    return None;
                }
                let region = voice
                    .source_loop_region
                    .or_else(|| self.effective_loop_region(id, sample.frame_count()))?;
                (
                    sample,
                    Some(index),
                    voice.generation,
                    self.timing_for_voice(voice),
                    voice.source_playback,
                    region,
                    voice.stretch.source_epoch_owner().load(Ordering::Acquire),
                )
            } else {
                let region = self.effective_loop_region(id, bank.frame_count())?;
                let timing = VoiceSourceTiming {
                    accepted: self.pad_accepted_timing[id],
                    ..VoiceSourceTiming::default()
                };
                (
                    bank,
                    None,
                    0,
                    timing,
                    SourcePlayback::new(region.start, ExplicitSeekMode::Normal, 1.0),
                    region,
                    0,
                )
            };
        playback.configure_domain(timing.loop_domain(sample.frame_count(), region));
        let position = playback.position();
        if !committed_stem_mode_available(
            position.seek_mode,
            capture.candidate.is_some(),
            paused,
            self.pad_key_lock_enabled[id],
            timing.accepted,
            sample,
        ) {
            return None;
        }
        let plan = SourceReadPlan {
            channels: self.channels,
            sample_frames: sample.frame_count(),
            frame_pos: position.frame,
            loop_region: region,
            loop_period: playback.loop_period(),
            seek_mode: position.seek_mode,
            selection,
            transition,
        };
        if !normal_loop_feed_available(
            sample,
            stems,
            self.sample_rate_hz as u32,
            timing.accepted,
            plan,
            &playback,
        ) {
            return None;
        }
        let context = NativeHistoryContext {
            id,
            ownership: &self.input_runtime_ownership,
            acknowledgements: &self.current_timing_acknowledgements,
            preparation_epoch: &self.prepared_source_epochs[id],
        };
        let permit = context.capture(sample, self.sample_rate_hz as u32, timing.accepted)?;
        Some(StemVoicePin {
            sample: sample.clone(),
            stems: stems.cloned(),
            voice_index: index,
            generation,
            key_lock: self.pad_key_lock_enabled[id],
            history_epoch,
            permit,
            accepted: timing.accepted,
            plan,
            playback,
            predecessor: self.committed_stem_selection[id],
        })
    }

    fn committed_stem_adoption_current(&self, transaction: &StemResidencyTransaction) -> bool {
        let id = transaction.id;
        let pin = &transaction.pin;
        if id >= NUM_SAMPLES
            || !transaction.status.publication.current()
            || !self.source_binding_current(id, transaction.binding)
            || !self
                .input_runtime_ownership
                .launch_current(id, transaction.launch_revision)
            || (transaction.target != StemMixMode::FullMix
                && !if let Some(candidate) = &transaction.candidate {
                    candidate.current()
                        && !candidate.adopted()
                        && !transaction.selection.revoked.load(Ordering::Acquire)
                        && self.committed_stem_selection[id] == pin.predecessor
                        && self.committed_stem_selection[id].map_or(0, |selection| selection.epoch)
                            == candidate.predecessor_selection_expected
                        && !self.stem_transitions[id].is_active()
                        && self.outgoing_stems[id].is_none()
                        && self.sample_bank[id]
                            .as_ref()
                            .is_some_and(candidate_residency_available)
                } else {
                    self.committed_selection_matches(id, &transaction.selection)
                })
            || !pin.permit.current(&pin.sample, self.sample_rate_hz as u32)
            || self.pad_key_lock_enabled[id] != pin.key_lock
            || self.pad_accepted_timing[id] != pin.accepted
            || !self.sample_bank[id]
                .as_ref()
                .is_some_and(|bank| bank.same_window(&pin.sample))
            || self.stem_render_selection(id) != pin.plan.selection
            || !self.stem_transitions[id].matches_source_contract(pin.plan.transition)
        {
            return false;
        }
        if !match (pin.stems.as_ref(), self.prepared_stems[id].as_ref()) {
            (None, None) => true,
            (Some(captured), Some(current)) => Arc::ptr_eq(
                &captured.complete_set_identity,
                &current.complete_set_identity,
            ),
            _ => false,
        } {
            return false;
        }
        let target = if transaction.target == StemMixMode::AllStems {
            let Some(stems) = transaction.stems.as_ref() else {
                return false;
            };
            StemRenderSelection::for_stems(stems, transaction.mask)
        } else {
            StemRenderSelection::full_mix()
        };
        let ramp = if target == pin.plan.selection {
            pin.plan.transition
        } else {
            StemTransition::start(pin.plan.selection, STEM_TRANSITION_RAMP_FRAMES)
        };
        if transaction.target == StemMixMode::AllStems
            && !transaction.stems.as_ref().is_some_and(|stems| {
                *stems.complete_set_identity == transaction.selection.identity
                    && stems.accepted_timing == pin.accepted
                    && prepared_stem_set_matches_sample(
                        stems,
                        &pin.sample,
                        self.channels,
                        self.sample_rate_hz,
                        pin.sample.frame_count(),
                    )
            })
        {
            return false;
        }
        let mut active = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| voice.active && voice.sample_id == id);
        match (pin.voice_index, active.next()) {
            (None, None) => {
                let plan = SourceReadPlan {
                    selection: target,
                    transition: ramp,
                    ..pin.plan
                };
                normal_loop_feed_available_with_readers(
                    &pin.sample,
                    StemReadSets::new(transaction.stems.as_ref(), pin.stems.as_ref()),
                    self.sample_rate_hz as u32,
                    pin.accepted,
                    plan,
                    &pin.playback,
                )
            }
            (Some(index), Some((current_index, voice))) => {
                if active.next().is_some()
                    || current_index != index
                    || voice.generation != pin.generation
                    || voice.frozen_stems.is_some()
                    || !committed_stem_mode_available(
                        voice.explicit_seek_mode,
                        transaction.candidate.is_some(),
                        voice.paused,
                        pin.key_lock,
                        pin.accepted,
                        &pin.sample,
                    )
                    || voice.stretch.source_epoch_owner().load(Ordering::Acquire)
                        != pin.history_epoch
                    || !voice
                        .sample
                        .as_ref()
                        .is_some_and(|sample| sample.same_window(&pin.sample))
                {
                    return false;
                }
                let Some(region) = voice
                    .source_loop_region
                    .or_else(|| self.effective_loop_region(id, pin.sample.frame_count()))
                else {
                    return false;
                };
                let mut playback = voice.source_playback;
                playback.configure_domain(
                    self.timing_for_voice(voice)
                        .loop_domain(pin.sample.frame_count(), region),
                );
                let position = playback.position();
                let actual = SourceReadPlan {
                    frame_pos: position.frame,
                    seek_mode: position.seek_mode,
                    loop_region: region,
                    loop_period: playback.loop_period(),
                    ..pin.plan
                };
                // Addressing/rate/owners stay current; the live cursor is allowed to advance.
                actual.matches_source_contract(pin.plan)
                    && playback.matches_rate_target(&pin.playback)
                    && committed_stem_mode_available(
                        position.seek_mode,
                        transaction.candidate.is_some(),
                        voice.paused,
                        pin.key_lock,
                        pin.accepted,
                        &pin.sample,
                    )
                    && normal_loop_feed_available_with_readers(
                        &pin.sample,
                        StemReadSets::new(transaction.stems.as_ref(), pin.stems.as_ref()),
                        self.sample_rate_hz as u32,
                        pin.accepted,
                        SourceReadPlan {
                            selection: target,
                            transition: ramp,
                            ..actual
                        },
                        &playback,
                    )
            }
            _ => false,
        }
    }

    pub(crate) fn apply_committed_stem_work(
        &mut self,
        mut work: Box<CommittedStemRtWork>,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        match work.as_mut() {
            CommittedStemRtWork::Register {
                id,
                binding,
                selection,
                publication,
            } => {
                let current = *id < NUM_SAMPLES
                    && publication.current()
                    && !selection.revoked.load(Ordering::Acquire)
                    && self.source_binding_current(*id, *binding)
                    && self
                        .input_runtime_ownership
                        .binding_source_generation(*id, *binding)
                        == Some(selection.source_generation)
                    && (!self.sample_is_active(*id)
                        || self.committed_stem_selection[*id].is_some_and(|old| {
                            old.identity == selection.identity
                                && old.source_generation == selection.source_generation
                        })
                        || self.prepared_stems[*id].as_ref().is_some_and(|stems| {
                            *stems.complete_set_identity == selection.identity
                                && stems.publication.status() == "accepted"
                                && self.sample_bank[*id].as_ref().is_some_and(|bank| {
                                    prepared_stem_set_matches_sample(
                                        stems,
                                        bank,
                                        self.channels,
                                        self.sample_rate_hz,
                                        bank.frame_count(),
                                    )
                                })
                        }));
                if current
                    && publication.claim_resident()
                    && publication.current()
                    && !selection.revoked.load(Ordering::Acquire)
                    && self.source_binding_current(*id, *binding)
                {
                    self.committed_stem_selection[*id] = Some(SelectedStemIdentity {
                        identity: selection.identity,
                        source_generation: selection.source_generation,
                        epoch: selection.expected,
                    });
                    selection.epoch.store(selection.expected, Ordering::Release);
                    publication.mark_accepted();
                } else {
                    publication.mark_rejected();
                }
            }
            CommittedStemRtWork::Capture(capture) => {
                if let Some(pin) = self.capture_committed_stem_voice(capture) {
                    if let Some(pin) = capture.publish(pin) {
                        capture.status.reject_rt();
                        *work = CommittedStemRtWork::RetiredPin {
                            _pin: pin,
                            _capture: capture.clone(),
                        };
                    }
                } else {
                    capture.status.reject_rt();
                    if capture.candidate.is_none() {
                        self.input_runtime_ownership.finish_resident_control(
                            capture.id,
                            capture.status.publication.expected,
                        );
                    }
                }
            }
            CommittedStemRtWork::Adopt(transaction) => {
                let id = transaction.id;
                let valid = retirement.available_retirement_slots() >= 2
                    && self.committed_stem_adoption_current(transaction)
                    && transaction.status.publication.claim_resident()
                    && self.committed_stem_adoption_current(transaction);
                if valid {
                    let previous = self.stem_render_selection(id);
                    if transaction.target == StemMixMode::AllStems {
                        let old = self.prepared_stems[id].take();
                        if transaction.candidate.is_some()
                            && self.sample_is_active(id)
                            && previous.required_stem_hash().is_some()
                        {
                            // Candidate admission proved this fixed slot empty. The
                            // final owner remains until the actual ramp/lifecycle ends.
                            self.outgoing_stems[id] = old;
                        } else {
                            transaction.retired_stems = old;
                        }
                        self.prepared_stems[id] = transaction.stems.take();
                        self.release_pair_components[id] = false;
                    }
                    // The same source PCM/window, voice, Native, FIFO and EQ owners are untouched.
                    self.stem_mix_mode[id] = transaction.target;
                    self.stem_mix_source_version_hash[id] =
                        if transaction.target == StemMixMode::AllStems {
                            self.prepared_stems[id]
                                .as_ref()
                                .expect("validated components")
                                .source_version_hash
                        } else {
                            0
                        };
                    self.stem_enabled_mask[id] = transaction.mask;
                    let next = self.stem_render_selection(id);
                    self.arm_stem_transition(id, previous, next);
                    if let Some(candidate) = &transaction.candidate {
                        self.committed_stem_selection[id] = Some(SelectedStemIdentity {
                            identity: transaction.selection.identity,
                            source_generation: transaction.selection.source_generation,
                            epoch: transaction.selection.expected,
                        });
                        transaction
                            .selection
                            .epoch
                            .store(transaction.selection.expected, Ordering::Release);
                        candidate.mark_adopted();
                    }
                    transaction
                        .status
                        .publication
                        .mark_accepted_preserved_window();
                    if transaction.target == StemMixMode::FullMix {
                        self.release_pair_components[id] = true;
                    }
                    transaction.status.effective.store(
                        // Stopped readiness installs readers, but proves no actual output.
                        if self.sample_is_active(id) { 1 } else { 4 },
                        Ordering::Release,
                    );
                    transaction.retired_status =
                        self.committed_stem_receipts[id].replace(StemModeReceipt {
                            status: transaction.status.clone(),
                            binding: transaction.binding,
                            selection: transaction.selection.clone(),
                            target: transaction.target,
                            mask: transaction.mask,
                            launch_revision: transaction.launch_revision,
                            candidate: transaction.candidate.clone(),
                        });
                    // This transaction owns one pre-reserved retired payload. Leave unrelated
                    // release requests to the ordinary render pass so they cannot consume it.
                    if transaction.target == StemMixMode::FullMix
                        && !self.stem_transitions[id].is_active()
                    {
                        transaction.retired_stems = self.prepared_stems[id].take();
                    }
                } else {
                    transaction.status.reject_rt();
                }
                if id < NUM_SAMPLES && transaction.candidate.is_none() {
                    self.input_runtime_ownership
                        .finish_resident_control(id, transaction.status.publication.expected);
                }
            }
            CommittedStemRtWork::RetiredPin { .. } => {}
        }
        retirement.retire_committed_stem_work(work);
    }

    pub(crate) fn finish_committed_stem_mode(&mut self, id: usize) {
        let Some(receipt) = self.committed_stem_receipts[id].as_ref() else {
            return;
        };
        if receipt.status.effective.load(Ordering::Acquire) == 3 {
            return;
        }
        let current = receipt.status.publication.current()
            && receipt
                .candidate
                .as_ref()
                .is_none_or(|candidate| candidate.receipt_current())
            && self.source_binding_current(id, receipt.binding)
            && self
                .input_runtime_ownership
                .launch_current(id, receipt.launch_revision)
            && (receipt.target == StemMixMode::FullMix
                || self.committed_selection_matches(id, &receipt.selection));
        if !current {
            receipt.status.effective.store(3, Ordering::Release);
            return;
        }
        let matches = self.stem_mix_mode[id] == receipt.target
            && self.stem_enabled_mask[id] == receipt.mask
            && !self.stem_transitions[id].is_active();
        let mut failed = false;
        let mut ready = true;
        let mut active = false;
        for voice in self
            .voices
            .iter()
            .filter(|voice| voice.active && voice.sample_id == id)
        {
            active = true;
            let wet = self.pad_key_lock_enabled[id]
                && (crate::audio_engine::rubberband_backend::pitch_scale_for_tempo_ratio(
                    voice.source_playback.tempo_ratio(),
                ) - 1.0)
                    .abs()
                    > crate::audio_engine::prepared_native_history::PITCH_SCALE_EPSILON;
            if wet
                && voice.stretch.wet_mode_failed()
                && !voice.stretch.wet_current_source_output_ready()
            {
                failed = true;
            }
            ready &= !voice.paused && (!wet || voice.stretch.wet_current_source_output_ready());
        }
        if failed {
            // Component residency remains accepted and current audio retains all its
            // real owners. Only this operation's unproved effective-mode receipt fails.
            receipt
                .status
                .effective_failure
                .store(true, Ordering::Release);
            receipt.status.effective.store(3, Ordering::Release);
            return;
        }
        if matches && active && ready {
            receipt.status.effective.store(2, Ordering::Release);
        }
    }
}
