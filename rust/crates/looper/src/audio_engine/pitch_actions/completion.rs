//! One-shot-per-bundle completion of a constant independent Piano physical birth.

use super::*;

pub(crate) struct PitchCompletionPlan {
    pub(crate) request: NativeHistoryRequest,
    pub(crate) intent: crate::audio_engine::late_start_dsp::DspInputIntent,
}

impl PitchActionWork {
    pub(crate) fn late_completion_prepared_for(&self, execution: u64) -> bool {
        self.late_reprepared_execution == Some(execution)
            && self.action.execution_target() == execution
            && self.early_completion_source().is_some()
    }

    pub(crate) fn early_completion_source(&self) -> Option<&NativeHistoryRequest> {
        if !self.is_ready()
            || self.rejected_request.is_some()
            || self.late_replan.is_some()
            || self.late_reprepare_snapshot().is_none()
            || self.action.start_behavior() != Some(KeyLockStartBehavior::FirstSoundAtGrid)
        {
            return None;
        }
        let source = if self.native_required {
            let candidate = self.candidate.as_ref()?;
            if !candidate.has_physical_birth_context() || candidate.has_retained_feed() {
                return None;
            }
            candidate.source.as_ref()?
        } else {
            self.request.as_ref()?
        };
        (source.actual_feed.is_none()
            && source.request_id == self.action.sequence()
            && source.epoch == self.action.epoch()
            && source.target_output_frame == self.action.execution_target())
        .then_some(source)
    }

    pub(crate) fn completion_request_matches(&self, request: &NativeHistoryRequest) -> bool {
        self.early_completion_source().is_some_and(|source| {
            request.request_id == self.action.sequence()
                && request.epoch == self.action.epoch()
                && request.actual_feed.is_none()
                && source.playback.matches_exact(&request.playback)
                && source.same_reserved_endpoint_request(request)
                && self.action.matches_history_owner(request)
        })
    }

    /// Bind a complete fresh envelope while retaining both old and failed pins.
    /// No Native reset, consumption or final source-owner drop occurs here.
    pub(crate) fn bind_early_completion(
        &mut self,
        plan: PitchCompletionPlan,
    ) -> Result<(), PitchActionReject> {
        let execution = plan.request.target_output_frame;
        let expected = self.action.execution_target();
        let current = self.completion_request_matches(&plan.request);
        debug_assert!(self.rejected_request.is_none());
        self.rejected_request = Some(plan.request);
        if !current || execution < self.action.original_due_frame() || execution >= expected {
            return Err(PitchActionReject::PreparedMismatch);
        }
        self.action.preparation_guard(self.own_window)?;
        let status = self.action.owner.status.load(Ordering::Acquire);
        if !matches!(status, PREPARED | SCHEDULED) {
            return Err(PitchActionReject::StaleContent);
        }
        self.action
            .owner
            .status
            .compare_exchange(status, PREPARED, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PitchActionReject::StaleContent)?;
        self.action
            .owner
            .execution_frame
            .compare_exchange(expected, execution, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PitchActionReject::PreparedMismatch)?;
        let fresh = self.rejected_request.take().unwrap();
        self.rejected_request = if self.native_required {
            self.candidate.as_mut().unwrap().source.replace(fresh)
        } else {
            self.request.replace(fresh)
        };
        self.launch_delay_frames = execution - self.action.original_due_frame();
        self.late_reprepared_execution = None;
        self.dsp_intent = Some(plan.intent);
        Ok(())
    }
}
