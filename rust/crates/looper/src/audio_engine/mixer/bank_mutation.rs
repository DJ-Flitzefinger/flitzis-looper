//! Whole-bank fixed image application using the existing placement and retirement authorities.
use super::*;
use crate::audio_engine::content_placement::PlacementError;
use crate::audio_engine::hold_action::HoldActionState;
use crate::audio_engine::pad_copy::bank::recovery::{RecoveredBankOccupied, RecoveredBankProbe};
use crate::audio_engine::pad_copy::bank::{BankEmptyRtImage, BankMutationRtWork, BankRtImage};
use crate::audio_engine::pad_pair::PairParticipantGuard;
use crate::messages::ControlMessage;
use std::sync::atomic::Ordering;

impl RtMixer {
    #[cfg(test)]
    pub(crate) fn recovered_gain_state_for_test(&self, id: usize) -> (f32, f32) {
        (
            self.pad_gain_smoothers[id].current,
            self.pad_gain_smoothers[id].target,
        )
    }
    /// One read-only complete callback observation. Runtime mismatch retires this
    /// probe instead of occupying the deferred FIFO ahead of required restores.
    pub(crate) fn verify_recovered_bank_rt(
        &mut self,
        probe: Box<RecoveredBankProbe>,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        if self.placement_pending() || retirement.available_retirement_slots() < 1 {
            self.defer_control(ControlMessage::VerifyRecoveredBank(probe));
            return;
        }
        let state = &probe.state;
        if !Arc::ptr_eq(&state.guard.ownership, &self.input_runtime_ownership)
            || !state.begin_probe(probe.nonce)
        {
            retirement.retire_recovered_bank_probe(probe);
            return;
        }
        let ownership = self.input_runtime_ownership.clone();
        let Some(anchor) = ownership.slot_snapshot(state.guard.targets[0].slot()) else {
            state.defer_probe(probe.nonce);
            self.defer_control(ControlMessage::VerifyRecoveredBank(probe));
            return;
        };
        let Ok(write) = ownership.prepare_slot_write_fixed(anchor, anchor.lifetime()) else {
            state.defer_probe(probe.nonce);
            self.defer_control(ControlMessage::VerifyRecoveredBank(probe));
            return;
        };
        let stale = std::cell::Cell::new(false);
        let result = ownership.commit_slot_write_fixed(
            &write,
            || {
                if !state.checking(probe.nonce) || !state.guard.current(true) {
                    stale.set(true);
                    return false;
                }
                state
                    .guard
                    .participants
                    .iter()
                    .enumerate()
                    .all(|(index, participant)| {
                        let slot = participant.placement().slot();
                        !self
                            .voices
                            .iter()
                            .any(|voice| voice.sample_id == slot && (voice.active || voice.paused))
                            && self.bank_participant_source_current(participant)
                            && match &state.images[index] {
                                Some(image) => self.recovered_bank_controls_current(slot, image),
                                None => {
                                    self.prepared_stems[slot].is_none()
                                        && self.outgoing_stems[slot].is_none()
                                        && self.committed_stem_receipts[slot].as_ref().is_none_or(
                                            |receipt| {
                                                !self.admitted_source_binding_current(
                                                    slot,
                                                    receipt.binding,
                                                )
                                            },
                                        )
                                }
                            }
                    })
            },
            |_| state.accept(probe.nonce, output_frame),
        );
        if matches!(result, Err(PlacementError::Pending)) {
            state.defer_probe(probe.nonce);
            self.defer_control(ControlMessage::VerifyRecoveredBank(probe));
            return;
        }
        if result.is_err() {
            state.finish_probe(probe.nonce, stale.get());
        }
        drop(ownership);
        retirement.retire_recovered_bank_probe(probe);
    }

    fn recovered_bank_controls_current(&self, id: usize, image: &RecoveredBankOccupied) -> bool {
        let intent = image.intent;
        let same_optional = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        };
        if (self.pad_accepted_timing[id].is_none()
            && (!same_optional(self.pad_period_seconds[id], intent.period_seconds)
                || self.pad_phase_anchor_frame[id].to_bits() != intent.origin_frame.to_bits()))
            || self.pad_loop_start_frame[id] != intent.loop_start_frame
            || self.pad_loop_end_frame[id] != intent.loop_end_frame
            || self.pad_pitch_intent[id] != intent.pitch_intent
            || self.pad_key_lock_enabled[id] != intent.key_lock
            || self.pad_key_lock_pending_on[id]
            || self.stem_mix_mode[id] != intent.stem_mix_mode
            || self.stem_mix_source_version_hash[id] != intent.source_version_hash
            || (intent.stem_mix_mode == StemMixMode::AllStems
                && self.stem_enabled_mask[id] != intent.enabled_mask)
            || self.pad_gain_db[id].to_bits() != intent.gain_db.to_bits()
            || self.pad_dsp_chains[id]
                .loop_acceptance_parameters()
                .1
                .iter()
                .zip([intent.eq_low_db, intent.eq_mid_db, intent.eq_high_db])
                .any(|(actual, expected)| {
                    actual.to_bits() != pad_eq_db_to_normalized(expected).to_bits()
                })
        {
            return false;
        }
        match (
            image.selection,
            image.selection_permit.as_ref(),
            self.committed_stem_selection[id],
        ) {
            (None, None, actual)
                if actual.is_none_or(|actual| {
                    self.sample_bank[id].as_ref().is_none_or(|sample| {
                        self.input_runtime_ownership.callback_source_generation(
                            id,
                            sample,
                            self.sample_rate_hz as u32,
                        ) != Some(actual.source_generation)
                    })
                }) => {}
            (Some(expected), Some(permit), Some(actual))
                if permit.current() && expected == actual => {}
            _ => return false,
        }
        match (&image.stems, &self.prepared_stems[id]) {
            (None, None) => {}
            (Some(expected), Some(actual))
                if expected.publication.status() == "accepted"
                    && actual.publication.status() == "accepted"
                    && actual.publication.current()
                    && Arc::ptr_eq(
                        &expected.complete_set_identity,
                        &actual.complete_set_identity,
                    )
                    && expected.source_version_hash == actual.source_version_hash
                    && expected.accepted_timing == actual.accepted_timing
                    && self.sample_bank[id].as_ref().is_some_and(|sample| {
                        prepared_stem_set_matches_sample(
                            actual,
                            sample,
                            self.channels,
                            self.sample_rate_hz,
                            sample.frame_count(),
                        )
                    })
                    && expected
                        .stems
                        .iter()
                        .zip(&actual.stems)
                        .all(|(expected, actual)| {
                            expected.same_source(actual)
                                && Arc::ptr_eq(&expected.samples, &actual.samples)
                                && expected.resident_binding() == actual.resident_binding()
                        }) => {}
            _ => return false,
        }
        let readers = crate::audio_engine::source_reader::StemReadSets::new(
            self.prepared_stems[id].as_ref(),
            self.outgoing_stems[id].as_ref(),
        );
        if let Some(from) = self.stem_transitions[id]
            .required_from_selection()
            .and_then(|selection| readers.for_selection(selection))
            && (image
                .selection
                .is_none_or(|selection| selection.identity != *from.complete_set_identity)
                || from.accepted_timing != self.pad_accepted_timing[id]
                || self.sample_bank[id].as_ref().is_none_or(|sample| {
                    !prepared_stem_set_matches_sample(
                        from,
                        sample,
                        self.channels,
                        self.sample_rate_hz,
                        sample.frame_count(),
                    )
                }))
        {
            return false;
        }
        // Settled FullMix may keep its accepted receipt and release policy. The
        // selected owner and actual readers above, not incidental flags, certify it.
        true
    }

    fn bank_participant_source_current(&self, participant: &PairParticipantGuard) -> bool {
        let slot = participant.placement().slot();
        match participant.control.binding() {
            Some(binding) => {
                self.admitted_source_binding_current(slot, binding)
                    && self.sample_bank[slot].as_ref().is_some_and(|sample| {
                        self.input_runtime_ownership.callback_source_generation(
                            slot,
                            sample,
                            self.sample_rate_hz as u32,
                        ) == Some(participant.control.source_generation())
                    })
            }
            None => {
                participant.placement().lifetime() == 0
                    && self
                        .input_runtime_ownership
                        .source_cell_empty(participant.placement().cell())
                    && self.sample_bank[slot].is_none()
                    && !self
                        .voices
                        .iter()
                        .any(|voice| voice.sample_id == slot && (voice.active || voice.paused))
            }
        }
    }
    pub(crate) fn mutate_bank_rt(
        &mut self,
        mut work: Box<BankMutationRtWork>,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        if self.placement_pending()
            || retirement.available_retirement_slots() < work.retirement_slots_needed()
        {
            self.defer_bank_mutation(work);
            return false;
        }
        if !self.bank_mutation_preflight(&work) {
            work.guard.reject();
            retirement.retire_bank_mutation(work);
            return false;
        }
        let ownership = self.input_runtime_ownership.clone();
        let guard = work.guard.clone();
        let committed = ownership.commit_slot_set_write(
            &work.writes,
            || guard.claim(),
            || {
                for (index, image) in work.images.iter_mut().enumerate() {
                    let before = guard.targets[index];
                    if let Some(key) = before.content_key() {
                        ownership.publish_ordinary_lifecycle(
                            key,
                            crate::audio_engine::ordinary_transport::STOPPED,
                            false,
                        );
                        self.retire_content_actions_rt(
                            key,
                            HoldActionState::Retired,
                            output_frame,
                            retirement,
                        );
                        self.stop_sample_rt(before.slot(), retirement);
                    }
                    match image {
                        BankRtImage::Copy(copy) => {
                            self.install_copy_image_claimed(copy, before.cell())
                        }
                        BankRtImage::Empty(empty) => {
                            self.install_bank_empty_claimed(empty, before.cell())
                        }
                    }
                }
            },
        );
        if let Ok(after) = committed {
            for (index, image) in work.images.iter().enumerate() {
                let target = after[index].slot();
                if let BankRtImage::Copy(copy) = image {
                    ownership.acknowledge_callback_source(copy.content_key);
                    if let Some(projection) = copy.projection {
                        self.current_timing_acknowledgements.acknowledge_for_key(
                            &ownership,
                            copy.content_key,
                            projection.publication_epoch,
                        );
                    } else {
                        self.current_timing_acknowledgements
                            .clear_for_key(&ownership, copy.content_key);
                    }
                    if let Some(stems) = &self.prepared_stems[target] {
                        stems.publication.mark_accepted();
                    }
                    copy.publication.mark_accepted();
                    self.publish_key_lock_status(target);
                }
            }
            guard
                .revision
                .store(after[0].layout_revision(), Ordering::Release);
            guard.phase.store(2, Ordering::Release);
            drop(guard);
            drop(ownership);
            retirement.retire_bank_mutation(work);
            true
        } else {
            // No true claim reaches an error. Marker contention is a bounded pending state.
            let pending = matches!(committed, Err(PlacementError::Pending));
            if !pending {
                guard.reject();
            }
            drop(guard);
            drop(ownership);
            if pending {
                self.defer_bank_mutation(work);
            } else {
                retirement.retire_bank_mutation(work);
            }
            false
        }
    }

    fn defer_bank_mutation(&mut self, mut work: Box<BankMutationRtWork>) {
        let publication = work
            .publication
            .take()
            .expect("published bank retains its publication");
        publication.restore_unclaimed(work);
        self.defer_control(ControlMessage::BankMutation(publication));
    }

    fn bank_mutation_preflight(&self, work: &BankMutationRtWork) -> bool {
        if !Arc::ptr_eq(&work.guard.ownership, &self.input_runtime_ownership)
            || work.guard.phase.load(Ordering::Acquire) != 0
            || !work.guard.current(true)
        {
            return false;
        }
        for participant in &work.guard.participants {
            if !self.bank_participant_source_current(participant) {
                return false;
            }
        }
        for (index, image) in work.images.iter().enumerate() {
            if !work.writes.writes()[index]
                .before()
                .same_projection(work.guard.targets[index])
            {
                return false;
            }
            match image {
                BankRtImage::Copy(copy) if self.copy_pad_preflight(copy) => {}
                BankRtImage::Empty(empty)
                    if empty.target == work.guard.targets[index].slot()
                        && empty.empty.sample_rate_hz == self.sample_rate_hz as u32
                        && empty.empty.channels == self.channels
                        && empty.retired_sample.is_none()
                        && empty.retired_stems.is_none()
                        && empty.retired_outgoing.is_none()
                        && empty.retired_receipt.is_none() => {}
                _ => return false,
            }
        }
        true
    }

    fn install_bank_empty_claimed(
        &mut self,
        image: &mut BankEmptyRtImage,
        cell: crate::audio_engine::content_placement::ContentCell,
    ) {
        let target = image.target;
        self.input_runtime_ownership
            .apply_empty_source_for_cell(cell, image.authority);
        image.epoch_owner.store(image.epoch, Ordering::Release);
        self.input_runtime_ownership
            .publish_copy_parameter_epoch_for_cell(cell, image.epoch);
        self.clear_timing_ack_for_slot(target);
        image.retired_sample = self.sample_bank[target].take();
        image.retired_stems = self.prepared_stems[target].take();
        image.retired_outgoing = self.outgoing_stems[target].take();
        image.retired_receipt = self.committed_stem_receipts[target].take();
        std::mem::swap(&mut self.pad_dsp_chains[target], &mut image.empty.dsp_chain);
        self.committed_stem_selection[target] = None;
        self.pad_key_lock_enabled[target] = false;
        self.pad_pitch_intent[target] = PitchIntent::default();
        self.pad_key_lock_request_id[target] = 0;
        self.pad_key_lock_rejected[target] = false;
        self.pad_key_lock_pending_on[target] = false;
        self.pad_key_lock_ack_source[target] = None;
        self.pad_key_lock_status[target] = None;
        self.pad_period_seconds[target] = None;
        self.pad_phase_anchor_frame[target] = 0.0;
        self.pad_accepted_timing[target] = None;
        self.pad_gain_db[target] = PAD_GAIN_DB_DEFAULT;
        self.pad_gain_smoothers[target] = SmoothedGain::default();
        self.pad_loop_start_frame[target] = 0;
        self.pad_loop_end_frame[target] = None;
        self.pad_playhead_frame[target] = None;
        self.stem_mix_mode[target] = StemMixMode::FullMix;
        self.stem_mix_source_version_hash[target] = 0;
        self.stem_enabled_mask[target] = STEM_COMPONENT_MASK;
        self.stem_transitions[target] = StemTransition::default();
        self.release_pair_components[target] = false;
    }
}
