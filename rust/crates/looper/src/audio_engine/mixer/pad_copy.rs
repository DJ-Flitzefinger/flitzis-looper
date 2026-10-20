//! One prepared copy, with atomic target retirement and a distinct stopped image/ACK.

use super::*;
use crate::audio_engine::pad_copy::PadCopyRtWork;
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use std::sync::atomic::Ordering;

fn same_copy_timing(
    source: Option<AcceptedTimingProjection>,
    target: Option<AcceptedTimingProjection>,
) -> bool {
    match (source, target) {
        (None, None) => true,
        (Some(source), Some(target)) => {
            source.revision == target.revision
                && source.period_seconds.to_bits() == target.period_seconds.to_bits()
                && source.origin_seconds.to_bits() == target.origin_seconds.to_bits()
                && source.sample_rate_hz == target.sample_rate_hz
                && target.publication_epoch != 0
        }
        _ => false,
    }
}

fn same_copy_window(source: &SampleBuffer, target: &SampleBuffer) -> bool {
    source.same_source(target)
        && Arc::ptr_eq(&source.samples, &target.samples)
        && source.resident_start() == target.resident_start()
        && source.resident_end() == target.resident_end()
        && source.residency.as_ref().map(|view| view.context)
            == target.residency.as_ref().map(|view| view.context)
}

fn copy_stem_reference_matches(stems: &PreparedStemSet, sample: &SampleBuffer) -> bool {
    match (&stems.reference_residency, &sample.residency) {
        (None, None) => true,
        (Some(reference), Some(sample)) => {
            Arc::ptr_eq(&reference.source, &sample.source)
                && reference.start_frame == sample.start_frame
                && reference.window_revision == sample.window_revision
                && reference.context == sample.context
        }
        _ => false,
    }
}

impl RtMixer {
    /// Parameter edits retain the actual admitted copy epoch across the two queues.
    pub(crate) fn pad_parameter_epoch_current(&self, id: usize, expected: u64) -> bool {
        id < NUM_SAMPLES
            && self
                .input_runtime_ownership
                .callback_copy_parameter_epoch(id)
                == Some(expected)
    }

    /// Legacy timing's own admission epoch is newer than every preceding accepted copy.
    pub(crate) fn legacy_parameter_after_copy(&self, id: usize, through_epoch: u64) -> bool {
        id < NUM_SAMPLES
            && self
                .input_runtime_ownership
                .callback_copy_parameter_epoch(id)
                .is_some_and(|epoch| through_epoch >= epoch)
    }

    /// Validate, claim and install one complete stopped target without touching its source.
    ///
    /// The callback only moves or clones already-owned handles and writes fixed scalar state.
    /// One aggregate retires displaced bank/stem owners; prechecked separate entries carry
    /// each old voice reader and the removed content's exact retained Hold.
    /// Target control admissions stay fenced by the reservation until control reconciles this ACK.
    #[cfg(test)]
    pub(crate) fn copy_pad_rt(
        &mut self,
        work: Box<PadCopyRtWork>,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        self.copy_pad_at_output_frame_rt(work, 0, retirement)
    }

    pub(crate) fn copy_pad_at_output_frame_rt(
        &mut self,
        mut work: Box<PadCopyRtWork>,
        output_frame: u64,
        retirement: &mut impl AudioBufferRetirement,
    ) -> bool {
        if self.placement_pending() {
            self.defer_control(crate::messages::ControlMessage::PadCopy(work));
            return false;
        }
        if !self.copy_pad_preflight(&work) {
            if self.placement_pending() || retirement.available_retirement_slots() == 0 {
                self.defer_control(crate::messages::ControlMessage::PadCopy(work));
                return false;
            }
            work.guard.reject();
            retirement.retire_pad_copy(work);
            return false;
        }
        if retirement.available_retirement_slots() < work.retirement_slots_needed() {
            self.defer_control(crate::messages::ControlMessage::PadCopy(work));
            return false;
        }

        let ownership = self.input_runtime_ownership.clone();
        let guard = work.guard.clone();
        let write = match ownership
            .prepare_slot_write_fixed(guard.target_placement, work.content_lifetime)
        {
            Ok(write) => write,
            Err(super::super::content_placement::PlacementError::Pending) => {
                self.defer_control(crate::messages::ControlMessage::PadCopy(work));
                return false;
            }
            Err(_) => {
                guard.reject();
                retirement.retire_pad_copy(work);
                return false;
            }
        };
        let target = work.target;
        let committed = ownership.commit_slot_write_fixed(
            &write,
            || guard.claim_stable(),
            |cell| {
                if let Some(old_key) = guard.target_placement.content_key() {
                    self.retire_content_actions_rt(
                        old_key,
                        crate::audio_engine::hold_action::HoldActionState::Retired,
                        output_frame,
                        retirement,
                    );
                    self.stop_sample_rt(target, retirement);
                }
                self.install_copy_image_claimed(work.as_mut(), cell);
            },
        );
        match committed {
            Ok(_) => {}
            Err(super::super::content_placement::PlacementError::Pending) => {
                self.defer_control(crate::messages::ControlMessage::PadCopy(work));
                return false;
            }
            Err(_) => {
                guard.reject();
                retirement.retire_pad_copy(work);
                return false;
            }
        }
        ownership.acknowledge_callback_source(work.content_key);
        if let Some(projection) = work.projection {
            self.current_timing_acknowledgements.acknowledge_for_key(
                &self.input_runtime_ownership,
                work.content_key,
                projection.publication_epoch,
            );
        } else {
            self.current_timing_acknowledgements
                .clear_for_key(&self.input_runtime_ownership, work.content_key);
        }
        if let Some(stems) = &self.prepared_stems[target] {
            stems.publication.mark_accepted();
        }
        self.publish_key_lock_status(target);
        work.publication.mark_accepted();
        work.guard.acknowledge();
        retirement.retire_pad_copy(work);
        true
    }

    pub(super) fn install_copy_image_claimed(
        &mut self,
        work: &mut PadCopyRtWork,
        cell: crate::audio_engine::content_placement::ContentCell,
    ) {
        let ownership = self.input_runtime_ownership.clone();
        let target = work.target;
        let intent = work.intent;
        ownership.apply_source_for_cell(
            cell,
            &work.sample,
            self.sample_rate_hz as u32,
            work.source_generation,
            work.authority_revision,
            intent.timing_intent,
        );
        work.guard
            .target_epoch
            .store(work.prepared_epoch, Ordering::Release);
        ownership.publish_copy_parameter_epoch_for_cell(cell, work.prepared_epoch);
        work.retired_sample = self.sample_bank[target].replace(work.sample.clone());
        work.retired_stems =
            std::mem::replace(&mut self.prepared_stems[target], work.stems.clone());
        work.retired_outgoing = self.outgoing_stems[target].take();
        work.retired_receipt = self.committed_stem_receipts[target].take();
        self.committed_stem_selection[target] = work.selection;
        if let Some(fences) = &work.selection_fences {
            fences.selection.store(fences.expected, Ordering::Release);
            fences
                .registration
                .store(fences.expected, Ordering::Release);
            fences.publication.mark_accepted();
        }
        self.release_pair_components[target] = false;
        self.pad_loop_start_frame[target] = intent.loop_start_frame;
        self.pad_loop_end_frame[target] = intent.loop_end_frame;
        self.pad_period_seconds[target] = intent.period_seconds;
        self.pad_phase_anchor_frame[target] = intent.origin_frame;
        self.pad_accepted_timing[target] = work.projection;
        self.stem_mix_mode[target] = intent.stem_mix_mode;
        self.stem_mix_source_version_hash[target] = intent.source_version_hash;
        self.stem_enabled_mask[target] = intent.enabled_mask;
        self.stem_transitions[target].clear();
        self.pad_playhead_frame[target] = None;
        self.pad_key_lock_enabled[target] = intent.key_lock;
        self.pad_pitch_intent[target] = intent.pitch_intent;
        self.pad_key_lock_request_id[target] = 0;
        self.pad_key_lock_rejected[target] = false;
        self.pad_key_lock_pending_on[target] = false;
        self.pad_key_lock_ack_source[target] =
            Some((work.source_generation, work.sample.source_address()));
        self.pad_key_lock_status[target] = None;

        // Both smoothing and filter history belong to this stopped target. Reserved future
        // parameters return to their neutral value rather than retaining the previous content.
        self.set_pad_dsp_parameter(target, DspParameterSlot::Slot3, 0.5);
        self.set_pad_gain(target, intent.gain_db);
        self.set_pad_eq(
            target,
            intent.eq_low_db,
            intent.eq_mid_db,
            intent.eq_high_db,
        );
        self.pad_dsp_chains[target].reset();
    }

    pub(super) fn copy_pad_preflight(&self, work: &PadCopyRtWork) -> bool {
        let guard = &work.guard;
        let target = work.target;
        let source = guard.source;
        let Some(source_placement) = self.input_runtime_ownership.slot_snapshot(source) else {
            return false;
        };
        let Some(target_placement) = self.input_runtime_ownership.slot_snapshot(target) else {
            return false;
        };
        if target >= NUM_SAMPLES
            || source >= NUM_SAMPLES
            || source == target
            || guard.target != target
            || work.source_generation == 0
            || work.content_lifetime == 0
            || guard
                .target_placement
                .for_lifetime(work.content_lifetime)
                .and_then(|snapshot| snapshot.content_key())
                != Some(work.content_key)
            || work.prepared_epoch <= guard.captured_target_epoch
            || work.authority_revision <= guard.target_authority
            || work.retired_stems.is_some()
            || work.retired_sample.is_some()
            || work.retired_outgoing.is_some()
            || work.retired_receipt.is_some()
            || !Arc::ptr_eq(&guard.ownership, &self.input_runtime_ownership)
            || !Arc::ptr_eq(
                &guard.acknowledgements,
                &self.current_timing_acknowledgements,
            )
            || !Arc::ptr_eq(
                &guard.source_epoch,
                &self.prepared_source_epochs[source_placement.cell().index()],
            )
            || !Arc::ptr_eq(
                &guard.target_epoch,
                &self.prepared_source_epochs[target_placement.cell().index()],
            )
            || !Arc::ptr_eq(&work.publication.epoch, &guard.target_epoch)
            || work.publication.expected != work.prepared_epoch
            || !guard.current()
            || !(if guard.source_reservation == 0 {
                self.source_binding_current(source, guard.source_binding)
            } else {
                self.admitted_source_binding_current(source, guard.source_binding)
            })
        {
            return false;
        }
        match guard.target_binding {
            Some(binding)
                if self.admitted_source_binding_current(target, binding)
                    && self.sample_bank[target].as_ref().is_some_and(|sample| {
                        self.input_runtime_ownership.callback_source_generation(
                            target,
                            sample,
                            self.sample_rate_hz as u32,
                        ) == Some(guard.target_source_generation)
                    }) => {}
            None if self.sample_bank[target].is_none()
                && !self
                    .voices
                    .iter()
                    .any(|voice| voice.sample_id == target && (voice.active || voice.paused)) => {}
            _ => return false,
        }
        let Some(source_sample) = self.sample_bank[source].as_ref() else {
            return false;
        };
        same_copy_window(source_sample, &work.sample)
            && self.copy_pad_geometry_valid(work)
            && same_copy_timing(guard.source_binding.accepted, work.projection)
            && self.copy_pad_stems_valid(work)
    }

    fn copy_pad_geometry_valid(&self, work: &PadCopyRtWork) -> bool {
        let intent = work.intent;
        let rate = self.sample_rate_hz as u32;
        if !work.sample.valid_residency(rate, self.channels)
            || intent.enabled_mask & !STEM_COMPONENT_MASK != 0
            || !intent.origin_frame.is_finite()
            || !intent.gain_db.is_finite()
            || !(PAD_GAIN_DB_MIN..=PAD_GAIN_DB_MAX).contains(&intent.gain_db)
            || [intent.eq_low_db, intent.eq_mid_db, intent.eq_high_db]
                .into_iter()
                .any(|value| {
                    !value.is_finite() || !(PAD_EQ_DB_MIN..=PAD_EQ_DB_MAX).contains(&value)
                })
            || intent.period_seconds.is_some_and(|period| {
                let bpm = 60.0 / period;
                SourceGrid::from_period(period * f64::from(rate), intent.origin_frame).is_none()
                    || !bpm.is_finite()
                    || bpm <= 0.0
            })
            || ((intent.key_lock || intent.pitch_intent.total_semitones() != 0)
                && !Self::initial_key_lock_geometry_available(&work.sample))
        {
            return false;
        }
        let Some(region) = effective_loop_region(
            intent.loop_start_frame,
            intent.loop_end_frame,
            work.sample.frame_count(),
        ) else {
            return false;
        };
        if intent.loop_start_frame != region.start
            || intent.loop_end_frame.is_some_and(|end| end != region.end)
            || !resident_read_context_available(
                &work.sample,
                region,
                ExplicitSeekMode::Normal,
                intent.key_lock,
            )
        {
            return false;
        }
        match work.projection {
            Some(projection) => {
                intent.timing_intent == TimingIntent::Automatic
                    && projection.sample_rate_hz == rate
                    && projection.publication_epoch != 0
                    && SourceGrid::from_period(
                        projection.period_seconds * f64::from(rate),
                        projection.origin_seconds * f64::from(rate),
                    )
                    .is_some()
            }
            None => intent.timing_intent != TimingIntent::Automatic,
        }
    }

    fn copy_pad_stems_valid(&self, work: &PadCopyRtWork) -> bool {
        let source = work.guard.source;
        match (work.selection, &work.selection_fences) {
            (None, None) => {}
            (selection, Some(fences))
                if selection.is_none_or(|selection| selection.epoch == fences.expected)
                    && fences.expected != 0
                    && Arc::ptr_eq(&fences.publication.epoch, &fences.registration)
                    && fences.publication.expected == fences.expected
                    && fences.selection.load(Ordering::Acquire) < fences.expected
                    && fences.registration.load(Ordering::Acquire) < fences.expected => {}
            _ => return false,
        }
        match (self.committed_stem_selection[source], work.selection) {
            (None, None) => {}
            // A sealed desired disk pair can be independently verified for Copy
            // while the source is FullMix and has no installed registration.
            (None, Some(target))
                if target.source_generation == work.source_generation && target.epoch != 0 => {}
            (Some(source), Some(target))
                if source.identity == target.identity
                    && target.source_generation == work.source_generation
                    && target.epoch != 0 => {}
            _ => return false,
        }
        if let Some(stems) = &work.stems {
            let source_stems = self.prepared_stems[source].as_ref();
            let Some(source_sample) = self.sample_bank[source].as_ref() else {
                return false;
            };
            if work
                .selection
                .is_none_or(|selection| selection.identity != *stems.complete_set_identity)
                || source_stems.is_some_and(|source_stems| {
                    *source_stems.complete_set_identity != *stems.complete_set_identity
                        || source_stems.source_version_hash != stems.source_version_hash
                        || source_stems.accepted_timing != work.guard.source_binding.accepted
                        || !copy_stem_reference_matches(source_stems, source_sample)
                })
                || stems.accepted_timing != work.projection
                || !copy_stem_reference_matches(stems, &work.sample)
                || source_stems.is_some_and(|source_stems| {
                    !prepared_stem_set_matches_sample(
                        source_stems,
                        source_sample,
                        self.channels,
                        self.sample_rate_hz,
                        source_sample.frame_count(),
                    )
                })
                || !prepared_stem_set_matches_sample(
                    stems,
                    &work.sample,
                    self.channels,
                    self.sample_rate_hz,
                    work.sample.frame_count(),
                )
                || !Arc::ptr_eq(&stems.publication.epoch, &work.guard.target_epoch)
                || stems.publication.expected != work.prepared_epoch
                || source_stems.is_some_and(|source_stems| {
                    source_stems
                        .stems
                        .iter()
                        .zip(&stems.stems)
                        .any(|(source, target)| !same_copy_window(source, target))
                })
            {
                return false;
            }
        }
        match work.intent.stem_mix_mode {
            StemMixMode::FullMix => work.intent.source_version_hash == 0,
            StemMixMode::AllStems => work.stems.as_ref().is_some_and(|stems| {
                work.intent.source_version_hash != 0
                    && work.intent.source_version_hash == stems.source_version_hash
            }),
        }
    }
}

#[cfg(test)]
#[path = "pad_copy_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "pad_copy_overwrite_tests.rs"]
mod overwrite_tests;
