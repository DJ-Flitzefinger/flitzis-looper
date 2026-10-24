//! Dry scalar-to-scalar request changes at one actual scheduler execution frame.
//! No owning work, projection conversion or pending rate authority lives here.

use super::global_rate::RateVoice;
use super::policy_retry::UnsupportedEvidence;
use super::*;
use crate::audio_engine::global_rate_status::RateTuple;
use crate::audio_engine::input_mapping::{RuntimePolicyError as Error, RuntimePolicySnapshot};
use crate::audio_engine::input_runtime_binding::PairControlProjection;
use crate::audio_engine::source_sync_policy::{SourceSyncMode, SourceSyncRequest};
use crate::audio_engine::transport::TransportTimeline;
use std::cell::RefCell;

#[cfg(test)]
thread_local! { static BEFORE_CLAIM: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) }; }

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
        if self.global_rate_pending() || !self.global_rate_drains_settled() {
            return Err(Error::Busy);
        }
        Ok(())
    }

    fn scalar_policy_candidate(
        &self,
        index: usize,
        snapshot: &RuntimePolicySnapshot,
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
        let inputs = self
            .scalar_policy_early_inputs(index, row)
            .ok_or(Error::SourceChanged)?;
        if inputs.blocked() {
            *evidence = Some(UnsupportedEvidence::Early {
                index,
                generation: voice.generation,
                inputs,
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
        let inputs = self.scalar_policy_late_inputs(index, &raw, intent);
        if inputs.blocked() {
            *evidence = Some(UnsupportedEvidence::Late {
                index,
                generation: voice.generation,
                inputs,
            });
            return Err(Error::Unsupported);
        }
        if row.request.mode == SourceSyncMode::Fixed && self.master_period_seconds.is_none() {
            return Err(Error::MissingMaster);
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
        let mut candidate = raw.playback;
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
        evidence: &mut Option<UnsupportedEvidence>,
    ) -> Result<ScalarLane, Error> {
        let voice = &self.voices[index];
        let row = snapshot.row(voice.sample_id).ok_or(Error::SourceChanged)?;
        let (raw, candidate, request) = self.scalar_policy_candidate(index, snapshot, evidence)?;
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
            || !candidate.matches_domain(raw.plan.domain())
        {
            return Err(Error::PhaseChanged);
        }
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
            raw.plan,
            candidate,
        ) {
            return Err(Error::ReaderUnavailable);
        }
        let candidate_plan = SourceReadPlan {
            frame_pos: after.frame,
            seek_mode: after.seek_mode,
            ..raw.plan
        };
        let a = raw.plan.fractional_taps(before.fraction);
        let b = candidate_plan.fractional_taps(after.fraction);
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
                    .sample_fractional_with_readers(
                        &raw.sample,
                        readers,
                        before.fraction,
                        0.0,
                        channel,
                    )
                    .to_bits()
                    != candidate_plan
                        .sample_fractional_with_readers(
                            &raw.sample,
                            readers,
                            after.fraction,
                            0.0,
                            channel,
                        )
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
    ) -> bool {
        if self.scalar_policy_available(snapshot).is_err()
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
            {
                return false;
            }
            let Ok((raw, candidate, request)) =
                self.scalar_policy_candidate(index, snapshot, &mut None)
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
            && self.global_rate_drains_settled()
    }

    pub(crate) fn adopt_runtime_policy_observed(
        &mut self,
        snapshot: &RuntimePolicySnapshot,
        timeline: &TransportTimeline,
        execution: u64,
    ) -> (
        Result<(), Error>,
        Option<super::super::runtime_policy_retry::RetryToken>,
    ) {
        self.prepare_runtime_policy_retry(snapshot);
        let before = self.observe_runtime_policy_retry(timeline);
        let mut evidence = None;
        let result = self.adopt_runtime_policy(snapshot, timeline, execution, &mut evidence);
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
        (result, retry)
    }

    fn adopt_runtime_policy(
        &mut self,
        snapshot: &RuntimePolicySnapshot,
        timeline: &TransportTimeline,
        _execution: u64,
        evidence: &mut Option<UnsupportedEvidence>,
    ) -> Result<(), Error> {
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
                *lane = Some(self.scalar_policy_lane(index, snapshot, evidence)?);
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
        let write = ownership
            .prepare_slot_write_fixed(anchor, anchor.lifetime())
            .map_err(|_| Error::SourceChanged)?;
        #[cfg(test)]
        if let Some(hook) = BEFORE_CLAIM.with(|value| value.borrow_mut().take()) {
            hook();
        }
        let mixer = RefCell::new(self);
        ownership
            .commit_slot_write_fixed(
                &write,
                || {
                    mixer
                        .borrow()
                        .scalar_policy_capture_current(snapshot, &capture, timeline)
                },
                |_| {
                    let mut mixer = mixer.borrow_mut();
                    for (index, lane) in capture.lanes.iter().enumerate() {
                        if let Some(lane) = lane {
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
        Ok(())
    }
}
