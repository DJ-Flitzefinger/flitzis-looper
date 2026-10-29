//! One callback-owned capture slot on the existing mixer and output scheduler.

use super::RtMixer;
use crate::audio_engine::buffer_retirement::AudioBufferRetirement;
use crate::audio_engine::global_playback_batch::GlobalPlaybackBatch;
use crate::audio_engine::mix_capture::publication::CapturePublication;
use crate::audio_engine::mix_capture::{CaptureConfig, CaptureFailure};
use crate::audio_engine::ordinary_start::OrdinaryGroupKey;
use crate::audio_engine::pitch_actions::PitchActionReject;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::messages::{ControlMessage, MixCaptureTarget};
use std::sync::Arc;

fn output_geometry_matches(rate_hz: f32, channels: usize, config: CaptureConfig) -> bool {
    channels == config.channels
        && rate_hz.is_finite()
        && rate_hz > 0.0
        && rate_hz.fract() == 0.0
        && f64::from(rate_hz) <= f64::from(u32::MAX)
        && rate_hz as u32 == config.sample_rate_hz
}

/// Identity of the one still-uncommitted group. `stop_frame` is an immutable
/// receipt of its successfully admitted scheduler event, never a second clock.
pub(super) struct PendingGroupCapture {
    pub(super) key: OrdinaryGroupKey,
    stop_frame: Option<u64>,
}

impl RtMixer {
    pub(crate) fn reject_unadmitted_group_capture(
        &mut self,
        batch: &GlobalPlaybackBatch,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        if let Some(publication) = &batch.capture_publication {
            if let Some(captured) = &batch.captured_start {
                captured.reject(PitchActionReject::PreparationFailed);
            }
            if let Some(mut work) = publication.claim() {
                work.fail(work.session_id(), CaptureFailure::Rejected);
                retirement.retire_mix_capture(work);
            }
        }
    }

    pub(crate) fn accept_group_capture<const CAPACITY: usize>(
        &mut self,
        publication: &CapturePublication,
        key: OrdinaryGroupKey,
        execution: u64,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        retirement: &mut impl AudioBufferRetirement,
    ) -> Result<(), PitchActionReject> {
        let Some(mut work) = publication.claim() else {
            return Err(PitchActionReject::PreparedMismatch);
        };
        let session = work.session_id();
        let rejection = if self.mix_capture.is_some() || work.observe_failure() {
            Some(CaptureFailure::Rejected)
        } else if !output_geometry_matches(self.sample_rate_hz, self.channels, work.config()) {
            Some(CaptureFailure::Geometry)
        } else if scheduler
            .schedule_capture_group_start(session, execution, key.slot, key.action_sequence)
            .is_err()
        {
            Some(CaptureFailure::Rejected)
        } else {
            None
        };
        if let Some(reason) = rejection {
            work.fail(session, reason);
            retirement.retire_mix_capture(work);
            return Err(PitchActionReject::PreparedMismatch);
        }
        self.mix_capture_pending_start = Some(execution);
        self.mix_capture_pending_group = Some(PendingGroupCapture {
            key,
            stop_frame: None,
        });
        self.mix_capture_scheduler_cleared = false;
        self.mix_capture = Some(work);
        Ok(())
    }

    pub(crate) fn group_capture_current(
        &mut self,
        batch: &GlobalPlaybackBatch,
        key: OrdinaryGroupKey,
        execution: u64,
    ) -> bool {
        if batch
            .captured_start
            .as_ref()
            .is_some_and(|captured| captured.cancelled())
        {
            return false;
        }
        let Some(publication) = batch.capture_publication.as_ref() else {
            return true;
        };
        let Some(pending) = self
            .mix_capture_pending_group
            .as_ref()
            .filter(|pending| pending.key == key)
        else {
            return false;
        };
        if pending.stop_frame.is_some_and(|stop| stop < execution) {
            return false;
        }
        self.mix_capture.as_mut().is_some_and(|work| {
            work.session_id() == publication.session_id()
                && !work.observe_failure()
                && !work.terminal()
                && output_geometry_matches(self.sample_rate_hz, self.channels, work.config())
        })
    }

    pub(crate) fn replan_group_capture(
        &mut self,
        batch: &GlobalPlaybackBatch,
        key: OrdinaryGroupKey,
        execution: u64,
    ) {
        if batch.capture_publication.is_some()
            && self
                .mix_capture_pending_group
                .as_ref()
                .is_some_and(|pending| pending.key == key)
        {
            self.mix_capture_pending_start = Some(execution);
        }
    }

    pub(crate) fn reject_pending_group_capture(
        &mut self,
        batch: &GlobalPlaybackBatch,
        key: OrdinaryGroupKey,
    ) {
        if let Some(publication) = batch.capture_publication.as_ref()
            && self
                .mix_capture_pending_group
                .as_ref()
                .is_some_and(|pending| pending.key == key)
            && let Some(work) = self
                .mix_capture
                .as_mut()
                .filter(|work| work.session_id() == publication.session_id())
        {
            work.fail(work.session_id(), CaptureFailure::Rejected);
        }
    }

    /// Called after due work at this same real scheduler/render boundary. An
    /// unready deferred group must not hide its own immutable S behind Pending.
    /// At S==E a ready group has already committed and cleared the association.
    pub(crate) fn settle_group_capture_stop_through(&mut self, frame: u64) {
        if self
            .mix_capture_pending_group
            .as_ref()
            .is_some_and(|pending| pending.stop_frame.is_some_and(|stop| stop <= frame))
            && let Some(work) = self.mix_capture.as_mut()
        {
            work.fail(work.session_id(), CaptureFailure::Cancelled);
        }
    }
    /// Admission consumes only the new publication. Busy/stale rejection cannot
    /// alter the old slot, and neither complete owner is ever destroyed here.
    pub(crate) fn accept_mix_capture<const CAPACITY: usize>(
        &mut self,
        publication: Arc<CapturePublication>,
        target_frame: MixCaptureTarget,
        callback_frame: u64,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        self.service_mix_capture(scheduler, retirement);
        // Publication + rejected work are both possible last heavy owners.
        if retirement.available_retirement_slots() < 2 {
            self.defer_control(ControlMessage::StartMixCapture {
                publication,
                target_frame,
            });
            return;
        }
        let work = publication.claim();
        retirement.retire_capture_publication(publication);
        let Some(mut work) = work else { return };
        let session_id = work.session_id();
        let config = work.config();
        // Intent survives placement/deferred-message and capture-retirement
        // gates unchanged. Only this actual admission fixes scalar F once.
        let target_frame = target_frame.resolve(callback_frame);
        let geometry_matches = output_geometry_matches(self.sample_rate_hz, self.channels, config);
        if self.mix_capture.is_some() || target_frame < callback_frame || work.observe_failure() {
            work.fail(session_id, CaptureFailure::Rejected);
            retirement.retire_mix_capture(work);
            return;
        }
        if !geometry_matches {
            work.fail(session_id, CaptureFailure::Geometry);
            retirement.retire_mix_capture(work);
            return;
        }
        if scheduler
            .schedule_capture_start(session_id, target_frame)
            .is_err()
        {
            work.fail(session_id, CaptureFailure::Rejected);
            retirement.retire_mix_capture(work);
            return;
        }
        // Scheduling owns the start + reserved stop lane before installing work.
        self.mix_capture_pending_start = Some(target_frame);
        self.mix_capture_pending_group = None;
        self.mix_capture_scheduler_cleared = false;
        self.mix_capture = Some(work);
    }

    pub(crate) fn schedule_mix_capture_stop<const CAPACITY: usize>(
        &mut self,
        session_id: u64,
        target_frame: MixCaptureTarget,
        callback_frame: u64,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    ) {
        let Some(work) = self
            .mix_capture
            .as_mut()
            .filter(|work| work.session_id() == session_id)
        else {
            return;
        };
        if work.terminal() {
            return;
        }
        if scheduler.capture_stop_target_frame(session_id).is_some() {
            // A previously admitted original stop wins even if a later duplicate
            // asks for a different endpoint before the still-pending start.
            return;
        }
        let target_frame = target_frame.resolve(callback_frame);
        if self
            .mix_capture_pending_start
            .is_some_and(|start| target_frame < start)
        {
            // A stop preceding the future original start withdraws that same
            // session. No positive first/end extent is manufactured.
            scheduler.cancel_capture_session(session_id);
            self.mix_capture_scheduler_cleared = true;
            self.mix_capture_pending_start = None;
            work.fail(session_id, CaptureFailure::Cancelled);
            return;
        }
        // Do not check is_full(): the original start reserved this exact lane.
        // Duplicate or stale-session rejection leaves the original event intact.
        let admitted_stop = target_frame.max(callback_frame);
        if scheduler
            .schedule_capture_stop(session_id, admitted_stop)
            .is_err()
        {
            // Even a reserved physical slot cannot override exhausted global
            // input ordering. A failed first stop must not look accepted while
            // recording continues forever. Existing accepted stops stay intact.
            work.fail(session_id, CaptureFailure::Rejected);
        } else if let Some(pending) = self.mix_capture_pending_group.as_mut() {
            pending.stop_frame = Some(admitted_stop);
        }
    }

    pub(crate) fn execute_mix_capture_start(&mut self, session_id: u64, frame: u64) {
        let Some(work) = self
            .mix_capture
            .as_mut()
            .filter(|work| work.session_id() == session_id)
        else {
            return;
        };
        if self.mix_capture_pending_start.is_none() {
            return;
        }
        self.mix_capture_pending_start = None;
        // Revalidate actual productive output geometry at the effective frame E.
        if !output_geometry_matches(self.sample_rate_hz, self.channels, work.config()) {
            work.fail(session_id, CaptureFailure::Geometry);
            return;
        }
        work.start(session_id, frame, self.sample_rate_hz as u32, self.channels);
    }

    pub(crate) fn execute_mix_capture_stop(&mut self, session_id: u64, frame: u64) {
        let Some(work) = self
            .mix_capture
            .as_mut()
            .filter(|work| work.session_id() == session_id)
        else {
            return;
        };
        if self.mix_capture_pending_group.is_some() {
            // An own stop may run while a worker/placement gate still retains
            // the original group. It cancels that empty original as one unit.
            work.fail(session_id, CaptureFailure::Cancelled);
        } else {
            work.stop(session_id, frame);
        }
    }

    /// Only the real final-mix segment calls this; there is no second render or clock.
    pub(crate) fn append_mix_capture(&mut self, first_frame: u64, final_mix: &[f32]) {
        if self.mix_capture_pending_start.is_some() {
            return;
        }
        if let Some(work) = self.mix_capture.as_mut() {
            work.append_segment(first_frame, final_mix);
        }
    }

    pub(crate) fn fail_mix_capture_stream(&mut self) {
        if let Some(work) = self.mix_capture.as_mut() {
            work.fail(work.session_id(), CaptureFailure::Stream);
        }
    }

    /// Service even empty callbacks. Sealing never waits for the retirement lane;
    /// a terminal owner remains in this same slot until its full Box can transfer.
    pub(crate) fn service_mix_capture<const CAPACITY: usize>(
        &mut self,
        scheduler: &mut FixedCapacityScheduler<CAPACITY>,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        let Some(work) = self.mix_capture.as_mut() else {
            return;
        };
        if let Some(pending) = self.mix_capture_pending_group.as_ref()
            && self.pitch_runtime.as_ref().is_some_and(|runtime| {
                !runtime.capture_group_pending(pending.key, work.session_id())
            })
        {
            work.fail(work.session_id(), CaptureFailure::Cancelled);
        }
        work.observe_failure();
        if !work.terminal() {
            return;
        }
        if let Some(pending) = self.mix_capture_pending_group.take()
            && let Some(runtime) = self.pitch_runtime.as_mut()
        {
            runtime.reject_capture_group(pending.key, work.session_id());
        }
        if !self.mix_capture_scheduler_cleared {
            scheduler.cancel_capture_session(work.session_id());
            self.mix_capture_scheduler_cleared = true;
        }
        self.mix_capture_pending_start = None;
        if retirement.available_retirement_slots() != 0 {
            let work = self
                .mix_capture
                .take()
                .expect("terminal capture owner retained");
            retirement.retire_mix_capture(work);
        }
    }
}

#[cfg(test)]
#[path = "capture/callback_tests.rs"]
mod tests;
