//! A mode-only claim for an unchanged productive pitch at settled unit source rate.
//!
//! Capture retains only scalar checkpoints. Original Native/history owners and their
//! frozen request provenance remain in the voice until the common tuple claim.

use super::*;

impl RtMixer {
    pub(super) fn capture_continuous_key_lock_tuple(
        &mut self,
        id: usize,
        pending: &mut PendingKeyLockTuple,
        frame: u64,
        execution: u64,
    ) -> bool {
        let Some(delta) = execution.checked_sub(frame) else {
            return false;
        };
        let request = pending.request;
        if id >= NUM_SAMPLES
            || request.pitch.total_semitones() == 0
            || request.previous_mode == request.enabled
        {
            return false;
        }
        let mut captured = [None; MAX_VOICES];
        let mut checkpoints = [None; MAX_VOICES];
        let mut saw_voice = false;
        for (index, captured_voice) in captured.iter_mut().enumerate() {
            let voice = &self.voices[index];
            if !voice.active || voice.sample_id != id {
                continue;
            }
            if voice.paused
                || self.effective_voice_tuple(index) != (request.pitch, request.previous_mode)
                || voice.stretch.source_lane_busy()
            {
                return false;
            }
            let Some(mut snapshot) = self.capture_pitch_voice_snapshot(index) else {
                return false;
            };
            if snapshot.paused
                || !self.sample_bank[id]
                    .as_ref()
                    .is_some_and(|bank| bank.same_source(&snapshot.sample))
            {
                return false;
            }
            let Some(permit) = self.capture_voice_history_permit(index, &snapshot) else {
                return false;
            };
            let old = feed(
                &snapshot,
                request.pitch,
                request.previous_mode,
                frame,
                Some(&permit),
            );
            if !voice.stretch.can_retain_mode_at_unit_rate(&old) {
                return false;
            }
            let native = voice.stretch.native_identity();
            let continuity = voice.source_feed_history.continuity_revision();
            if native == 0 || continuity == 0 || !snapshot.project_output_frames(delta) {
                return false;
            }
            *captured_voice = Some(TupleVoice {
                generation: voice.generation,
                paused: false,
                epoch: voice.stretch.preparation_epoch(),
                request_id: 0,
                pitch: request.pitch,
                previous_mode: request.previous_mode,
                continuous: true,
            });
            checkpoints[index] = Some(ContinuousTupleVoice {
                pad_id: id,
                request_id: request.request_id,
                native,
                continuity,
                playback: snapshot.playback,
                plan: snapshot.plan,
            });
            saw_voice = true;
        }
        if !saw_voice {
            return false;
        }
        // A failed participant leaves the original tuple untouched for its normal
        // preparation path; only a complete scalar capture publishes this choice.
        pending.voices = captured;
        pending.execution = Some(execution);
        // Each active voice belongs to one pad. Preserve other pads' pending
        // checkpoints; storage is bounded by voice count, not pads x voices.
        for (index, checkpoint) in checkpoints.into_iter().enumerate() {
            if checkpoint.is_some() {
                self.continuous_key_lock_voices[index] = checkpoint;
            }
        }
        true
    }

    pub(super) fn continuous_key_lock_voice_ready(
        &self,
        index: usize,
        request_id: u64,
        expected: TupleVoice,
        frame: u64,
        permit: &crate::audio_engine::native_history_permit::NativeHistoryPermit,
    ) -> bool {
        let Some(continuous) = self.continuous_key_lock_voices[index] else {
            return false;
        };
        let voice = &self.voices[index];
        if !voice.active
            || voice.paused
            || expected.paused
            || !expected.continuous
            || expected.request_id != 0
            || continuous.request_id != request_id
            || continuous.pad_id != voice.sample_id
            || voice.generation != expected.generation
            || voice.stretch.preparation_epoch() != expected.epoch
            || continuous.native == 0
            || voice.stretch.native_identity() != continuous.native
            || continuous.continuity == 0
            || voice.source_feed_history.continuity_revision() != continuous.continuity
            || voice.stretch.source_lane_busy()
            || self.effective_voice_tuple(index) != (expected.pitch, expected.previous_mode)
        {
            return false;
        }
        let Some(snapshot) = self.capture_pitch_voice_snapshot(index) else {
            return false;
        };
        if snapshot.paused
            || !snapshot.playback.matches_exact(&continuous.playback)
            || !snapshot.plan.matches_exact(continuous.plan)
            || !self.sample_bank[voice.sample_id]
                .as_ref()
                .is_some_and(|bank| bank.same_source(&snapshot.sample))
        {
            return false;
        }
        let old = feed(
            &snapshot,
            expected.pitch,
            expected.previous_mode,
            frame,
            Some(permit),
        );
        voice.stretch.can_retain_mode_at_unit_rate(&old)
    }

    pub(super) fn clear_continuous_key_lock_tuple(
        &mut self,
        id: usize,
        pending: &PendingKeyLockTuple,
    ) {
        for checkpoint in self.continuous_key_lock_voices.iter_mut() {
            if checkpoint.is_some_and(|captured| {
                captured.pad_id == id && captured.request_id == pending.request.request_id
            }) {
                *checkpoint = None;
            }
        }
    }
}
