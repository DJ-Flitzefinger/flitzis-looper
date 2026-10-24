//! Dry policy conversion at one actual scheduler execution frame.
//! The original runtime snapshot, global lease and fixed content claim own the effect.

use super::global_rate::RateVoice;
use super::policy_retry::UnsupportedEvidence;
use super::*;
use crate::audio_engine::content_placement::OwnerIdentity;
use crate::audio_engine::global_rate_status::RateTuple;
use crate::audio_engine::input_mapping::{RuntimePolicyError as Error, RuntimePolicySnapshot};
use crate::audio_engine::input_runtime_binding::PairControlProjection;
use crate::audio_engine::mapped_source_clock::MappedClockError;
use crate::audio_engine::pad_start::PadStartCustody;
use crate::audio_engine::source_reader_period::SourceReaderPeriod;
use crate::audio_engine::source_sync_policy::{SourceSyncMode, SourceSyncRequest};
use crate::audio_engine::transport::TransportTimeline;
use std::cell::RefCell;

#[cfg(test)]
thread_local! { static BEFORE_CLAIM: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) }; }

#[cfg(test)]
thread_local! { static AFTER_GLOBAL_LEASE: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) }; }

#[cfg(test)]
thread_local! {
    static BEFORE_UNSUPPORTED_INPUTS: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    static AFTER_UNSUPPORTED_DECISION: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

struct ScalarLane {
    original: RateVoice,
    control: PairControlProjection,
    old_request: SourceSyncRequest,
    request: SourceSyncRequest,
    candidate: SourcePlayback,
    map_owner: Option<OwnerIdentity>,
    replace_map: bool,
}

struct ScalarCapture {
    lanes: [Option<ScalarLane>; MAX_VOICES],
    rate: RateTuple,
    bpm_lock: bool,
    timing_fence: u64,
    timeline: TransportTimeline,
}

impl RtMixer {
    #[cfg(test)]
    pub(crate) fn with_runtime_policy_after_global_lease_for_test<T>(
        hook: impl FnOnce() + 'static,
        action: impl FnOnce() -> T,
    ) -> T {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                AFTER_GLOBAL_LEASE.with(|value| *value.borrow_mut() = None);
            }
        }
        AFTER_GLOBAL_LEASE
            .with(|value| assert!(value.borrow_mut().replace(Box::new(hook)).is_none()));
        let _reset = Reset;
        let result = action();
        assert!(AFTER_GLOBAL_LEASE.with(|value| value.borrow().is_none()));
        result
    }

    #[cfg(test)]
    pub(crate) fn with_runtime_policy_unsupported_hooks_for_test<T>(
        before: Option<Box<dyn FnOnce()>>,
        after: Option<Box<dyn FnOnce()>>,
        action: impl FnOnce() -> T,
    ) -> T {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                BEFORE_UNSUPPORTED_INPUTS.with(|hook| *hook.borrow_mut() = None);
                AFTER_UNSUPPORTED_DECISION.with(|hook| *hook.borrow_mut() = None);
            }
        }
        BEFORE_UNSUPPORTED_INPUTS.with(|hook| *hook.borrow_mut() = before);
        AFTER_UNSUPPORTED_DECISION.with(|hook| *hook.borrow_mut() = after);
        let _reset = Reset;
        action()
    }

    #[cfg(test)]
    pub(crate) fn with_runtime_policy_before_claim_for_test<T>(
        hook: impl FnOnce() + 'static,
        action: impl FnOnce() -> T,
    ) -> T {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                BEFORE_CLAIM.with(|value| *value.borrow_mut() = None);
            }
        }
        BEFORE_CLAIM.with(|value| {
            assert!(value.borrow_mut().replace(Box::new(hook)).is_none());
        });
        let _reset = Reset;
        let result = action();
        assert!(
            BEFORE_CLAIM.with(|value| value.borrow().is_none()),
            "policy claim hook was not reached"
        );
        result
    }
    fn scalar_policy_available(&self, snapshot: &RuntimePolicySnapshot) -> Result<(), Error> {
        if !snapshot.belongs_to(&self.input_runtime_ownership) || !snapshot.current() {
            return Err(Error::Stale);
        }
        // These completed fields may be inspected during another publisher's
        // lease. Only our own later lease and final claim can authorize effects.
        if self.global_rate_pending()
            || !self.global_drains_claimed(self.global_rate_timing_revision())
        {
            return Err(Error::Busy);
        }
        Ok(())
    }

    fn scalar_policy_candidate(
        &self,
        index: usize,
        snapshot: &RuntimePolicySnapshot,
        timeline: &TransportTimeline,
        execution: u64,
        evidence: &mut Option<UnsupportedEvidence>,
    ) -> Result<(PitchSourceSnapshot, SourcePlayback, SourceSyncRequest), Error> {
        let voice = &self.voices[index];
        let id = voice.sample_id;
        let row = snapshot.row(id).ok_or(Error::SourceChanged)?;
        if !snapshot.row_current(id) || !self.admitted_source_binding_current(id, row.binding) {
            return Err(Error::SourceChanged);
        }
        #[cfg(test)]
        if let Some(hook) = BEFORE_UNSUPPORTED_INPUTS.with(|hook| hook.borrow_mut().take()) {
            hook();
        }
        // These exact once-read values decide the ORIGINAL guard and supply its
        // refusal evidence. A later re-read may not impersonate this decision.
        let early_inputs = self
            .scalar_policy_early_inputs(index, row)
            .ok_or(Error::SourceChanged)?;
        if early_inputs.blocked() {
            *evidence = Some(UnsupportedEvidence::Early {
                index,
                generation: voice.generation,
                inputs: early_inputs,
            });
            return Err(Error::Unsupported);
        }
        let raw = self
            .capture_voice_snapshot(index, SnapshotKind::RawScalarPolicy)
            .ok_or(Error::ReaderUnavailable)?;
        if raw.sample.source_address() != row.binding.source_address
            || raw.sample.source_sample_count() != row.binding.sample_count
            || raw.sample.channels != row.binding.channels
            || raw.sample.resident_binding() != row.binding.resident
            || raw.accepted != row.binding.accepted
        {
            return Err(Error::SourceChanged);
        }
        let intent = self.loop_intent_frames(id, row.loop_start_s, row.loop_end_s);
        let Some(intent) = intent else {
            *evidence = Some(UnsupportedEvidence::Loop {
                index,
                generation: voice.generation,
                intent,
            });
            return Err(Error::Unsupported);
        };
        let inputs = self.scalar_policy_late_inputs(index, &raw, intent, row.map_preparation);
        if inputs.blocked() {
            *evidence = Some(UnsupportedEvidence::Late {
                index,
                generation: voice.generation,
                inputs,
            });
            return Err(Error::Unsupported);
        }
        if matches!(
            row.request.mode,
            SourceSyncMode::Fixed | SourceSyncMode::Mapped
        ) && self.master_period_seconds.is_none()
        {
            return Err(Error::MissingMaster);
        }
        if row.request.mode == SourceSyncMode::Mapped {
            if !self.bpm_lock_enabled || !snapshot.map_current(id) {
                return Err(Error::SourceChanged);
            }
            let target = snapshot.prepared_map(id).ok_or(Error::Unsupported)?;
            if raw.playback.is_mapped() && raw.playback.mapped_output_frame() != Some(execution) {
                return Err(Error::PhaseChanged);
            }
            let same_map = voice
                .mapped_start
                .as_ref()
                .and_then(|owner| owner.effective_source())
                .is_some_and(|old| {
                    old.same_source(target)
                        && (old.geometry().has_compact_continuation()
                            || old.geometry().has_general_continuation())
                });
            let candidate = if raw.playback.is_compact_mapped()
                && same_map
                && raw
                    .playback
                    .mapped_epoch()
                    .is_some_and(|epoch| epoch.matches_timeline(timeline))
            {
                // New raw intent alone does not restart an already adopted trajectory.
                raw.playback
            } else {
                let scalar_domain = raw
                    .playback
                    .original_scalar_domain()
                    .ok_or(Error::Unsupported)?;
                let constructed = if target.geometry().has_general_continuation() {
                    SourcePlayback::mapped_general_continuation(
                        target.geometry(),
                        snapshot.revision(),
                        *timeline,
                        scalar_domain,
                        raw.playback.position(),
                        execution,
                    )
                } else if raw.playback.is_mapped() && scalar_domain != target.geometry().domain() {
                    SourcePlayback::mapped_compact_replacement(
                        target.geometry(),
                        snapshot.revision(),
                        *timeline,
                        &raw.playback,
                        execution,
                    )
                } else {
                    SourcePlayback::mapped_anchored_compact_continuation(
                        target.geometry(),
                        snapshot.revision(),
                        *timeline,
                        scalar_domain,
                        raw.playback.position(),
                        execution,
                    )
                };
                constructed.map_err(|error| {
                    // Only whole-map slope limits are cursor independent. Raw
                    // inverse, fraction, epoch and horizon failures may recover
                    // through ordinary playback and receive no stable evidence.
                    if error == MappedClockError::UnsupportedRate
                        && let Some(projection) = timeline.beat_projection()
                    {
                        *evidence = Some(UnsupportedEvidence::MappedRate {
                            index,
                            generation: voice.generation,
                            inputs: early_inputs,
                            projection,
                        });
                    }
                    Error::Unsupported
                })?
            };
            // Cursor/E/first-PCM eligibility can recover while playback advances.
            // These failures deliberately carry no stable Unsupported evidence.
            self.scalar_policy_endpoint(&raw, &candidate)?;
            return Ok((raw, candidate, row.request));
        }
        let target = self
            .tempo_ratio_for_request_with_rate(
                row.request,
                self.timing_for_voice(voice).period_seconds(),
                self.speed,
                self.master_period_seconds,
            )
            .filter(|ratio| ratio.is_finite() && (SPEED_MIN..=SPEED_MAX).contains(ratio))
            .ok_or(Error::InvalidTarget)?;
        let mut candidate = if raw.playback.is_mapped() {
            if raw.playback.mapped_output_frame() != Some(execution) {
                return Err(Error::PhaseChanged);
            }
            let original_domain = raw
                .playback
                .original_scalar_domain()
                .ok_or(Error::Unsupported)?;
            let construct = match original_domain.musical_period {
                SourceReaderPeriod::Physical => SourcePlayback::scalar_continuation,
                SourceReaderPeriod::ScalarMusical(_) => SourcePlayback::scalar_musical_continuation,
                SourceReaderPeriod::MappedRational(_)
                | SourceReaderPeriod::MappedPhysicalQuarters(_) => return Err(Error::Unsupported),
            };
            construct(
                raw.playback.position(),
                original_domain,
                raw.playback.tempo_ratio(),
                raw.playback.tempo_ratio(),
            )
            .ok_or(Error::Unsupported)?
        } else {
            raw.playback
        };
        if target.to_bits() != candidate.rate_target().to_bits() {
            if !candidate.set_target(target) {
                return Err(Error::InvalidTarget);
            }
            // Exactly the existing renderer's first new-target step, on a copy.
            // Installing this copy leaves its normal 512-active-frame timer intact.
            candidate.chunk(DEFAULT_BLOCK_SAMPLES);
        }
        self.scalar_policy_endpoint(&raw, &candidate)?;
        Ok((raw, candidate, row.request))
    }

    fn scalar_policy_lane(
        &self,
        index: usize,
        snapshot: &RuntimePolicySnapshot,
        timeline: &TransportTimeline,
        execution: u64,
        evidence: &mut Option<UnsupportedEvidence>,
    ) -> Result<ScalarLane, Error> {
        let voice = &self.voices[index];
        let row = snapshot.row(voice.sample_id).ok_or(Error::SourceChanged)?;
        let (raw, candidate, request) =
            self.scalar_policy_candidate(index, snapshot, timeline, execution, evidence)?;
        let original = self
            .capture_rate_participant(index, &raw)
            .map_err(|_| Error::SourceChanged)?;
        let control = self
            .input_runtime_ownership
            .pair_control_projection(original.placement, Some(row.binding))
            .map_err(|_| Error::SourceChanged)?;
        Ok(ScalarLane {
            original,
            control,
            old_request: voice.sync_request,
            request,
            candidate,
            map_owner: voice
                .mapped_start
                .as_ref()
                .and_then(|owner| owner.effective_source())
                .map(OwnerIdentity::of),
            replace_map: request.mode == SourceSyncMode::Mapped
                && snapshot
                    .prepared_map(voice.sample_id)
                    .is_some_and(|target| {
                        voice
                            .mapped_start
                            .as_ref()
                            .and_then(|owner| owner.effective_source())
                            .is_none_or(|old| {
                                !old.same_source(target)
                                    || old.sync_witness() != target.sync_witness()
                                    || !(old.geometry().has_compact_continuation()
                                        || old.geometry().has_general_continuation())
                            })
                    }),
        })
    }

    fn scalar_policy_endpoint(
        &self,
        raw: &PitchSourceSnapshot,
        candidate: &SourcePlayback,
    ) -> Result<(), Error> {
        let before = raw.playback.position();
        let after = candidate.position();
        if before.frame != after.frame
            || before.fraction.to_bits() != after.fraction.to_bits()
            || before.seek_mode != after.seek_mode
        {
            return Err(Error::PhaseChanged);
        }
        let domain = candidate.effective_domain().ok_or(Error::PhaseChanged)?;
        let candidate_plan = raw
            .plan
            .with_effective_domain(domain)
            .ok_or(Error::PhaseChanged)?;
        let readers = raw.readers().ok_or(Error::ReaderUnavailable)?;
        if !normal_loop_feed_available_with_readers(
            &raw.sample,
            readers,
            raw.sample_rate_hz,
            raw.accepted,
            raw.plan,
            &raw.playback,
        ) || !normal_loop_feed_available_with_readers(
            &raw.sample,
            readers,
            raw.sample_rate_hz,
            raw.accepted,
            candidate_plan,
            candidate,
        ) {
            return Err(Error::ReaderUnavailable);
        }
        let candidate_plan = SourceReadPlan {
            frame_pos: after.frame,
            seek_mode: after.seek_mode,
            ..candidate_plan
        };
        let a = raw
            .plan
            .try_fractional_taps(before.fraction)
            .ok_or(Error::PhaseChanged)?;
        let b = candidate_plan
            .try_fractional_taps(after.fraction)
            .ok_or(Error::PhaseChanged)?;
        if a.left != b.left
            || a.right != b.right
            || a.right_gain.to_bits() != b.right_gain.to_bits()
        {
            return Err(Error::PhaseChanged);
        }
        for channel in 0..raw.plan.channels {
            if !raw
                .plan
                .tap_available_with_readers(&raw.sample, readers, a.left, channel)
                || a.right.is_some_and(|right| {
                    !raw.plan
                        .tap_available_with_readers(&raw.sample, readers, right, channel)
                })
                || raw
                    .plan
                    .try_sample_fractional_with_readers(
                        &raw.sample,
                        readers,
                        before.fraction,
                        0.0,
                        channel,
                    )
                    .ok_or(Error::ReaderUnavailable)?
                    .to_bits()
                    != candidate_plan
                        .try_sample_fractional_with_readers(
                            &raw.sample,
                            readers,
                            after.fraction,
                            0.0,
                            channel,
                        )
                        .ok_or(Error::ReaderUnavailable)?
                        .to_bits()
            {
                return Err(Error::ReaderUnavailable);
            }
        }
        Ok(())
    }

    fn scalar_policy_capture_current(
        &self,
        snapshot: &RuntimePolicySnapshot,
        capture: &ScalarCapture,
        timeline: &TransportTimeline,
        execution: u64,
    ) -> bool {
        if !snapshot.belongs_to(&self.input_runtime_ownership)
            || !snapshot.current()
            || self.global_rate_pending()
            || !self.global_drains_claimed(capture.timing_fence)
            || self.effective_global_rate() != capture.rate
            || self.bpm_lock_enabled != capture.bpm_lock
            || self.global_rate_timing_revision() != capture.timing_fence
            || !timeline.matches_clock_epoch(&capture.timeline)
        {
            return false;
        }
        for (index, expected) in capture.lanes.iter().enumerate() {
            let Some(expected) = expected else {
                if self.voices[index].active {
                    return false;
                }
                continue;
            };
            if self
                .rate_participant_current(index, &expected.original)
                .is_err()
                || !self
                    .input_runtime_ownership
                    .pair_control_projection_current(&expected.control, 0)
                || self.voices[index].sync_request != expected.old_request
                || self.voices[index]
                    .mapped_start
                    .as_ref()
                    .and_then(|owner| owner.effective_source())
                    .map(OwnerIdentity::of)
                    != expected.map_owner
            {
                return false;
            }
            let Ok((raw, candidate, request)) =
                self.scalar_policy_candidate(index, snapshot, timeline, execution, &mut None)
            else {
                return false;
            };
            if request != expected.request
                || !candidate.matches_exact(&expected.candidate)
                || !raw.playback.matches_exact(&expected.original.playback)
                || !raw.plan.matches_exact(expected.original.plan)
                || !raw
                    .source_feed_context
                    .same_source(expected.original.context)
            {
                return false;
            }
        }
        snapshot.current()
            && self.global_rate_timing_revision() == capture.timing_fence
            && self.global_drains_claimed(capture.timing_fence)
    }

    pub(crate) fn adopt_runtime_policy_observed(
        &mut self,
        snapshot: &RuntimePolicySnapshot,
        timeline: &TransportTimeline,
        execution: u64,
        retirement: &mut dyn AudioBufferRetirement,
    ) -> Option<(
        Result<(), Error>,
        Option<super::super::runtime_policy_retry::RetryToken>,
    )> {
        self.prepare_runtime_policy_retry(snapshot);
        let before = self.observe_runtime_policy_retry(timeline);
        let mut evidence = None;
        let result = match self.adopt_runtime_policy(
            snapshot,
            timeline,
            execution,
            retirement,
            &mut evidence,
        ) {
            Ok(None) => return None,
            Ok(Some(())) => Ok(()),
            Err(error) => Err(error),
        };
        #[cfg(test)]
        if result == Err(Error::Unsupported)
            && let Some(hook) = AFTER_UNSUPPORTED_DECISION.with(|hook| hook.borrow_mut().take())
        {
            hook();
        }
        let matches_before = self.policy_retry_evidence_matches(evidence);
        let after = self.observe_runtime_policy_retry(timeline);
        let retry = if result == Err(Error::Unsupported)
            && before.is_some()
            && before == after
            && matches_before
            && self.policy_retry_evidence_matches(evidence)
            && snapshot.retry_fences_current()
        {
            before
        } else {
            None
        };
        Some((result, retry))
    }

    /// Work plus the greatest owner displacement possible for this original snapshot.
    /// This is capacity accounting only; the final claim repeats all source checks.
    pub(crate) fn runtime_policy_retirement_slots(
        &self,
        snapshot: &RuntimePolicySnapshot,
    ) -> usize {
        1 + self
            .voices
            .iter()
            .filter(|voice| voice.active)
            .filter(|voice| {
                snapshot
                    .prepared_map(voice.sample_id)
                    .is_some_and(|target| {
                        voice
                            .mapped_start
                            .as_ref()
                            .and_then(|owner| owner.effective_source())
                            .is_some_and(|old| {
                                !old.same_source(target)
                                    || old.sync_witness() != target.sync_witness()
                                    || !(old.geometry().has_compact_continuation()
                                        || old.geometry().has_general_continuation())
                            })
                    })
            })
            .count()
    }

    fn adopt_runtime_policy(
        &mut self,
        snapshot: &RuntimePolicySnapshot,
        timeline: &TransportTimeline,
        execution: u64,
        retirement: &mut dyn AudioBufferRetirement,
        evidence: &mut Option<UnsupportedEvidence>,
    ) -> Result<Option<()>, Error> {
        self.scalar_policy_available(snapshot)?;
        if !self.voices.iter().any(|voice| voice.active) {
            return Err(Error::NoActiveVoices);
        }
        let master_dependent = self
            .voices
            .iter()
            .filter(|voice| voice.active)
            .any(|voice| {
                snapshot.row(voice.sample_id).is_none_or(|row| {
                    row.request.mode != SourceSyncMode::Off
                        && (row.request.mode != SourceSyncMode::Legacy
                            || row.request.mapped_matching_enabled(self.bpm_lock_enabled))
                })
            });
        // Off and matching-disabled Legacy consume speed only. Leave both
        // independent master records intact, including their absent states.
        if master_dependent
            && timeline.master_period_seconds().map(f64::to_bits)
                != self.master_period_seconds.map(f64::to_bits)
        {
            return Err(Error::MissingMaster);
        }
        let mut capture = ScalarCapture {
            lanes: std::array::from_fn(|_| None),
            rate: self.effective_global_rate(),
            bpm_lock: self.bpm_lock_enabled,
            timing_fence: self.global_rate_timing_revision(),
            timeline: *timeline,
        };
        for (index, lane) in capture.lanes.iter_mut().enumerate() {
            if self.voices[index].active {
                *lane =
                    Some(self.scalar_policy_lane(index, snapshot, timeline, execution, evidence)?);
            }
        }
        let anchor = capture
            .lanes
            .iter()
            .flatten()
            .next()
            .ok_or(Error::NoActiveVoices)?
            .original
            .placement;
        let ownership = self.input_runtime_ownership.clone();
        let slots = 1 + capture
            .lanes
            .iter()
            .flatten()
            .filter(|lane| lane.replace_map && lane.map_owner.is_some())
            .count();
        if retirement.available_retirement_slots() < slots {
            return Err(Error::Capacity);
        }
        #[cfg(test)]
        if let Some(hook) = BEFORE_CLAIM.with(|value| value.borrow_mut().take()) {
            hook();
        }
        // Observe retry outside this lease. Global publishers and this complete
        // effect share the established global-before-placement order.
        let Some(_lease) =
            crate::audio_engine::GlobalTimingLease::acquire(&ownership.global_timing_publishing)
        else {
            return Ok(None);
        };
        #[cfg(test)]
        if let Some(hook) = AFTER_GLOBAL_LEASE.with(|value| value.borrow_mut().take()) {
            hook();
        }
        let write = ownership
            .prepare_slot_write_fixed(anchor, anchor.lifetime())
            .map_err(|_| Error::SourceChanged)?;
        let mixer = RefCell::new(self);
        ownership
            .commit_slot_write_fixed(
                &write,
                || {
                    mixer
                        .borrow()
                        .scalar_policy_capture_current(snapshot, &capture, timeline, execution)
                },
                |_| {
                    let mut mixer = mixer.borrow_mut();
                    for (index, lane) in capture.lanes.iter().enumerate() {
                        if let Some(lane) = lane {
                            if lane.replace_map {
                                let target = snapshot
                                    .prepared_map(lane.original.id)
                                    .expect("complete claim retains the original prepared map");
                                if let Some(owner) = &mut mixer.voices[index].mapped_start {
                                    if let Some(displaced) = owner.replace_effective_map(target) {
                                        retirement.retire_pad_start_target(displaced);
                                    }
                                } else {
                                    mixer.voices[index].mapped_start =
                                        Some(PadStartCustody::for_runtime_map(target));
                                }
                            }
                            mixer.voices[index].sync_request = lane.request;
                            mixer.voices[index].source_playback = lane.candidate;
                        }
                    }
                },
            )
            .map_err(|_| Error::Stale)?;
        for lane in capture.lanes.iter().flatten() {
            mixer.borrow().publish_source_sync_status(lane.original.id);
        }
        Ok(Some(()))
    }
}
