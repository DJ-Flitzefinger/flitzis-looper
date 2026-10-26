//! Infallible occupied-pair audio image mutation inside a claimed placement transaction.

use super::*;
use crate::audio_engine::content_placement::{PairPlacementKind, PlacementError};
use crate::audio_engine::pad_pair::PadPairRtWork;
use crate::messages::ControlMessage;

impl RtMixer {
    /// Claim both complete original participants, transfer their living images and emit own ACK.
    /// One pre-reserved retirement entry carries the full work and displaced Move-empty owners.
    pub(crate) fn pair_pad_rt(
        &mut self,
        mut work: Box<PadPairRtWork>,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        if self.placement_pending() || retirement.available_retirement_slots() == 0 {
            self.defer_control(ControlMessage::PadPair(work));
            return false;
        }
        if !self.pair_pad_preflight(&work) {
            if self.placement_pending() {
                self.defer_control(ControlMessage::PadPair(work));
                return false;
            }
            work.guard.reject();
            retirement.retire_pad_pair(work);
            return false;
        }
        let ownership = work.guard.ownership.clone();
        let guard = work.guard.clone();
        let prepared = work.prepared.clone();
        let source = prepared.source().slot();
        let target = prepared.target().slot();
        let kind = prepared.kind();
        let mut holds = [None; 2];
        let committed = ownership.commit_placement_pair(
            &prepared,
            true,
            || guard.claim_stable(),
            || {
                // The callback is the sole cohort/effect writer. These fixed diagnostic reads
                // happen under the shared marker at the actual Pair linearization point; they
                // cannot fail or undo claim, and no historical Hold ACK grants new permission.
                holds = guard.participants.each_ref().map(|participant| {
                    participant
                        .placement()
                        .content_key()
                        .and_then(|key| self.capture_hold_projection_rt(key))
                });
                self.swap_occupied_pad_images_rt(source, target);
                if kind == PairPlacementKind::Move {
                    self.install_pair_empty_image_rt(source, &mut work);
                }
            },
        );
        match &committed {
            Ok(committed) => guard.acknowledge(*committed, holds),
            Err(PlacementError::Pending) => {}
            Err(_) => {
                guard.reject();
            }
        }
        // Work still pins every temporary owner. An immediately draining retirement worker
        // must never make one of these Arc drops the final allocation owner on the callback.
        drop(guard);
        drop(prepared);
        drop(ownership);
        match committed {
            Ok(_) => {
                retirement.retire_pad_pair(work);
                true
            }
            Err(PlacementError::Pending) => {
                self.defer_control(ControlMessage::PadPair(work));
                false
            }
            Err(_) => {
                // Every error is preclaim. An interrupted true claim leaves Pending and never
                // enters a rollback or terminal-rejection branch.
                retirement.retire_pad_pair(work);
                false
            }
        }
    }

    fn pair_pad_preflight(&self, work: &PadPairRtWork) -> bool {
        let guard = &work.guard;
        let source = work.prepared.source();
        let target = work.prepared.target();
        if source.slot() >= NUM_SAMPLES
            || target.slot() >= NUM_SAMPLES
            || source.slot() == target.slot()
            || source.lifetime() == 0
            || !source.same_projection(guard.participants[0].placement())
            || !target.same_projection(guard.participants[1].placement())
            || !Arc::ptr_eq(&guard.ownership, &self.input_runtime_ownership)
            || !Arc::ptr_eq(
                &guard.acknowledgements,
                &self.current_timing_acknowledgements,
            )
            || work.empty.sample_rate_hz as f32 != self.sample_rate_hz
            || work.empty.channels != self.channels
            || work.retired_stems.is_some()
            || work.retired_outgoing.is_some()
            || work.retired_receipt.is_some()
            || guard.phase() != 0
            || !guard.stable_current()
        {
            return false;
        }
        for participant in &guard.participants {
            let snapshot = participant.placement();
            let slot = snapshot.slot();
            if !Arc::ptr_eq(
                &participant.preparation_epoch,
                &self.prepared_source_epochs[snapshot.cell().index()],
            ) {
                return false;
            }
            match participant.control.binding() {
                Some(binding) => {
                    if snapshot.lifetime() == 0
                        || !self.admitted_source_binding_current(slot, binding)
                        || !self.sample_bank[slot].as_ref().is_some_and(|sample| {
                            self.input_runtime_ownership.callback_source_generation(
                                slot,
                                sample,
                                self.sample_rate_hz as u32,
                            ) == Some(participant.control.source_generation())
                        })
                    {
                        return false;
                    }
                }
                None => {
                    if snapshot.lifetime() != 0
                        || self.sample_bank[slot].is_some()
                        || self
                            .voices
                            .iter()
                            .any(|voice| voice.active && voice.sample_id == slot)
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Called only after occupied-image exchange left the captured empty image at source.
    /// Every possibly final owner is moved to the work aggregate before canonical scalar stores.
    fn install_pair_empty_image_rt(&mut self, source: usize, work: &mut PadPairRtWork) {
        self.input_runtime_ownership
            .apply_pair_empty_defaults_for_cell(work.prepared.target().cell());
        std::mem::swap(&mut self.pad_dsp_chains[source], &mut work.empty.dsp_chain);
        work.retired_stems = self.prepared_stems[source].take();
        work.retired_outgoing = self.outgoing_stems[source].take();
        work.retired_receipt = self.committed_stem_receipts[source].take();
        self.committed_stem_selection[source] = None;
        self.pad_key_lock_enabled[source] = false;
        self.pad_pitch_intent[source] = PitchIntent::default();
        self.pad_key_lock_request_id[source] = 0;
        self.pad_key_lock_rejected[source] = false;
        self.pad_key_lock_pending_on[source] = false;
        self.pad_key_lock_ack_source[source] = None;
        self.pad_key_lock_status[source] = None;
        self.pad_period_seconds[source] = None;
        self.pad_phase_anchor_frame[source] = 0.0;
        self.pad_accepted_timing[source] = None;
        self.pad_gain_db[source] = PAD_GAIN_DB_DEFAULT;
        self.pad_gain_smoothers[source] = SmoothedGain::default();
        self.pad_loop_start_frame[source] = 0;
        self.pad_loop_end_frame[source] = None;
        self.pad_playhead_frame[source] = None;
        self.stem_mix_mode[source] = StemMixMode::FullMix;
        self.stem_mix_source_version_hash[source] = 0;
        self.stem_enabled_mask[source] = STEM_COMPONENT_MASK;
        self.stem_transitions[source] = StemTransition::default();
        self.release_pair_components[source] = false;
    }

    /// Exchange two complete occupied images without restarting their admitted voices.
    ///
    /// The parent must validate distinct, in-range, occupied slots and every transaction guard
    /// before claim. It calls this only inside the placement publication's infallible apply
    /// closure. This does not publish placement, acknowledge a transaction or release admission.
    /// Move's independent empty defaults and retirement are installed by `pair_pad_rt` afterward.
    ///
    /// Authority, timing ACKs and preparation epochs belong to stable content cells. They are
    /// deliberately untouched. Voice lanes retain their native/FIFO/history/preparation owners;
    /// only the physical pad used to select their image changes.
    pub(super) fn swap_occupied_pad_images_rt(&mut self, source: usize, target: usize) {
        self.pad_key_lock_enabled.swap(source, target);
        self.pad_pitch_intent.swap(source, target);
        self.pad_key_lock_request_id.swap(source, target);
        self.pad_key_lock_rejected.swap(source, target);
        self.pad_key_lock_pending_on.swap(source, target);
        self.pad_key_lock_ack_source.swap(source, target);
        self.pad_key_lock_status.swap(source, target);
        self.pad_period_seconds.swap(source, target);
        self.pad_phase_anchor_frame.swap(source, target);
        self.pad_accepted_timing.swap(source, target);
        self.pad_gain_db.swap(source, target);
        self.pad_gain_smoothers.swap(source, target);
        self.pad_dsp_chains.swap(source, target);
        self.pad_loop_start_frame.swap(source, target);
        self.pad_loop_end_frame.swap(source, target);
        self.pad_playhead_frame.swap(source, target);
        self.sample_bank.swap(source, target);
        self.prepared_stems.swap(source, target);
        self.outgoing_stems.swap(source, target);
        self.committed_stem_selection.swap(source, target);
        self.committed_stem_receipts.swap(source, target);
        self.stem_mix_mode.swap(source, target);
        self.stem_mix_source_version_hash.swap(source, target);
        self.stem_enabled_mask.swap(source, target);
        self.stem_transitions.swap(source, target);
        self.release_pair_components.swap(source, target);

        self.pad_dsp_chains[source].retarget_pad_id(source as u16);
        self.pad_dsp_chains[target].retarget_pad_id(target as u16);
        for voice in &mut self.voices {
            if voice.active {
                if voice.sample_id == source {
                    voice.phase_read = None;
                    voice.sample_id = target;
                } else if voice.sample_id == target {
                    voice.phase_read = None;
                    voice.sample_id = source;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "pad_pair_tests.rs"]
mod tests;
