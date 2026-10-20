//! One prospective mode tuple on the existing source-preparation lanes.
//!
//! Resident storage can be claimed first, but its original ticket remains adopting until
//! all exact Native/FIFO owners and the same mode request commit at one output boundary.

use super::*;
use crate::audio_engine::input_runtime_binding::InputPadBinding;
use crate::audio_engine::key_lock_preparation::ProspectiveSourceHistory;
use crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES;
use std::cell::RefCell;

#[path = "key_lock_continuous.rs"]
mod continuous;

#[derive(Clone, Copy)]
pub(super) struct KeyLockTupleRequest {
    pub(super) enabled: bool,
    pub(super) request_id: u64,
    pub(super) binding: InputPadBinding,
    pub(super) source_generation: u64,
    pub(super) pitch: PitchIntent,
    pub(super) previous_mode: bool,
}

#[cfg(test)]
mod tests {
    include!("key_lock_tuple_tests.rs");
}

#[derive(Clone, Copy)]
pub(super) struct ContinuousTupleVoice {
    pad_id: usize,
    request_id: u64,
    native: usize,
    continuity: u64,
    playback: SourcePlayback,
    plan: SourceReadPlan,
}

#[derive(Clone, Copy)]
struct TupleVoice {
    generation: u64,
    paused: bool,
    epoch: u64,
    request_id: u64,
    pitch: PitchIntent,
    previous_mode: bool,
    continuous: bool,
}

pub(super) struct PendingKeyLockTuple {
    request: KeyLockTupleRequest,
    execution: Option<u64>,
    voices: [Option<TupleVoice>; MAX_VOICES],
    resident: Option<Box<crate::messages::ResidentTransaction>>,
}

impl PendingKeyLockTuple {
    fn scalar(request: KeyLockTupleRequest) -> Self {
        Self {
            request,
            execution: None,
            voices: [None; MAX_VOICES],
            resident: None,
        }
    }
}

fn feed<'a>(
    snapshot: &'a PitchSourceSnapshot,
    pitch: PitchIntent,
    enabled: bool,
    output_frame: u64,
    permit: Option<&'a crate::audio_engine::native_history_permit::NativeHistoryPermit>,
) -> ProductiveSourceFeed<'a> {
    let readers = snapshot.readers().unwrap_or_default();
    ProductiveSourceFeed {
        pitch_intent: pitch,
        key_lock: enabled,
        sample: &snapshot.sample,
        // Keep a FullMix history pin reachable by the source worker even though
        // the plan correctly excludes it from endpoint reader comparisons.
        stems: readers.current.or(snapshot.stems.as_ref()),
        outgoing_stems: readers.outgoing,
        sample_rate_hz: snapshot.sample_rate_hz,
        accepted: snapshot.accepted,
        playback: &snapshot.playback,
        plan: snapshot.plan,
        permit,
        output_frame: Some(output_frame),
    }
}

impl RtMixer {
    pub(super) fn owns_resident_key_lock_tuple(&self, id: usize) -> bool {
        self.pending_key_lock_tuples.get(id).is_some_and(|pending| {
            pending
                .as_ref()
                .is_some_and(|pending| pending.resident.is_some())
        })
    }

    pub(super) fn key_lock_tuple_needs_preparation(&self, id: usize, enabled: bool) -> bool {
        id < NUM_SAMPLES
            && self.pad_key_lock_enabled[id] != enabled
            && self.pad_pitch_intent[id].total_semitones() != 0
            && self
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == id)
    }

    pub(super) fn effective_voice_tuple(&self, index: usize) -> (PitchIntent, bool) {
        let voice = &self.voices[index];
        if voice.frozen_stems.is_some()
            && !self.sample_bank[voice.sample_id]
                .as_ref()
                .is_some_and(|bank| {
                    voice
                        .sample
                        .as_ref()
                        .is_some_and(|sample| bank.same_source(sample))
                })
        {
            (self.voice_pitch_intent[index], self.voice_key_lock[index])
        } else {
            (
                self.pad_pitch_intent[voice.sample_id],
                self.pad_key_lock_enabled[voice.sample_id],
            )
        }
    }

    pub(super) fn retain_resident_key_lock_tuple(
        &mut self,
        id: usize,
        transaction: Box<crate::messages::ResidentTransaction>,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        if let Some(previous) = self.pending_key_lock_tuples[id].take() {
            self.finish_key_lock_tuple(id, previous, false, retirement);
        }
        self.queued_key_lock_tuples[id] = None;
        let sample = self.sample_bank[id]
            .as_ref()
            .expect("claimed resident bank");
        // This is a fresh observation of the actual installed window, not a future-window permit.
        let binding = InputPadBinding {
            resident: sample.resident_binding(),
            runtime_revision: self.input_runtime_ownership.runtime_revision(id),
            ..transaction.binding
        };
        let source_generation = self
            .input_runtime_ownership
            .binding_source_generation(id, binding)
            .expect("claimed source generation");
        let request = KeyLockTupleRequest {
            enabled: transaction.intent.key_lock.expect("mode-only transaction"),
            request_id: transaction
                .intent
                .key_lock_request_id
                .expect("owned mode request"),
            binding,
            source_generation,
            pitch: self.pad_pitch_intent[id],
            previous_mode: self.pad_key_lock_enabled[id],
        };
        let mut pending = PendingKeyLockTuple::scalar(request);
        pending.resident = Some(transaction);
        self.pending_key_lock_tuples[id] = Some(pending);
        self.pad_key_lock_rejected[id] = false;
        self.publish_key_lock_status(id);
    }

    fn key_lock_tuple_current(&self, id: usize, pending: &PendingKeyLockTuple) -> bool {
        let request = pending.request;
        self.pad_pitch_intent[id] == request.pitch
            && self.pad_key_lock_enabled[id] == request.previous_mode
            && self.pad_key_lock_request_id[id] == request.request_id
            && self
                .input_runtime_ownership
                .callback_key_lock_request_current(id, request.request_id)
            && self.key_lock_request_binding_current(id, request.binding)
            && self
                .input_runtime_ownership
                .binding_source_generation(id, request.binding)
                == Some(request.source_generation)
            && pending
                .resident
                .as_ref()
                .is_none_or(|work| work.publication.current())
    }

    fn capture_key_lock_tuple(
        &mut self,
        id: usize,
        pending: &mut PendingKeyLockTuple,
        frame: u64,
    ) -> bool {
        let Some(execution) = frame.checked_add(PREPARED_HISTORY_FRAMES as u64) else {
            return false;
        };
        if self.capture_continuous_key_lock_tuple(id, pending, frame, execution) {
            return true;
        }
        let mut saw_voice = false;
        for index in 0..MAX_VOICES {
            if !self.voices[index].active || self.voices[index].sample_id != id {
                continue;
            }
            saw_voice = true;
            let Some(snapshot) = self.capture_pitch_voice_snapshot(index) else {
                return false;
            };
            // The lane projects to the original E and refuses if its ramp is still
            // active there. Pause contributes zero active frames to that projection.
            // Reserve reusable callback storage before changing this voice's DSP request.
            // A filled snapshot returns through the worker; no replacement is allocated here.
            if !self.voices[index].stretch.source_history_work_available() {
                return false;
            }
            let (pitch, previous_mode) = self.effective_voice_tuple(index);
            self.voices[index].stretch.hold_prospective_tuple();
            pending.voices[index] = Some(TupleVoice {
                generation: self.voices[index].generation,
                paused: snapshot.paused,
                epoch: self.voices[index].stretch.preparation_epoch(),
                request_id: 0,
                pitch,
                previous_mode,
                continuous: false,
            });
            let Some(permit) = self.capture_voice_history_permit(index, &snapshot) else {
                return false;
            };
            let prospective = feed(
                &snapshot,
                pitch,
                pending.request.enabled,
                frame,
                Some(&permit),
            );
            let voice = &mut self.voices[index];
            let Some((epoch, request_id)) = voice.stretch.request_prospective_tuple_with_history(
                &prospective,
                ProspectiveSourceHistory {
                    history: &voice.source_feed_history,
                    voice_lifetime: &voice.history_lifetime,
                    voice_generation: voice.generation,
                    execution_frame: execution,
                    paused: snapshot.paused,
                    actual_feed: snapshot.actual_feed,
                    source_feed_context: snapshot.source_feed_context,
                },
            ) else {
                return false;
            };
            pending.voices[index] = Some(TupleVoice {
                generation: self.voices[index].generation,
                paused: snapshot.paused,
                epoch,
                request_id,
                pitch,
                previous_mode,
                continuous: false,
            });
        }
        pending.execution = Some(execution);
        saw_voice
    }

    fn key_lock_tuple_ready(
        &mut self,
        id: usize,
        pending: &PendingKeyLockTuple,
        frame: u64,
        continuous_permits: &[Option<crate::audio_engine::native_history_permit::NativeHistoryPermit>;
             MAX_VOICES],
    ) -> bool {
        if pending.execution != Some(frame) || !self.key_lock_tuple_current(id, pending) {
            return false;
        }
        for (index, permit) in continuous_permits.iter().enumerate() {
            let voice = &self.voices[index];
            let participates = voice.active && voice.sample_id == id;
            let Some(expected) = pending.voices[index] else {
                if participates {
                    return false;
                }
                continue;
            };
            if !participates
                || voice.paused != expected.paused
                || voice.generation != expected.generation
                || self.effective_voice_tuple(index) != (expected.pitch, expected.previous_mode)
            {
                return false;
            }
            if expected.continuous {
                let Some(permit) = permit.as_ref() else {
                    return false;
                };
                if !self.continuous_key_lock_voice_ready(
                    index,
                    pending.request.request_id,
                    expected,
                    frame,
                    permit,
                ) {
                    return false;
                }
                continue;
            }
            let Some(snapshot) = self.capture_pitch_voice_snapshot(index) else {
                return false;
            };
            let prospective = feed(
                &snapshot,
                expected.pitch,
                pending.request.enabled,
                frame,
                None,
            );
            if !self.voices[index].stretch.prospective_tuple_matches(
                &prospective,
                expected.epoch,
                expected.request_id,
            ) {
                return false;
            }
        }
        true
    }

    fn commit_key_lock_tuple(
        &mut self,
        id: usize,
        pending: &PendingKeyLockTuple,
        frame: u64,
    ) -> bool {
        // Capture current permits before entering our layout marker; obtaining a
        // new slot observation inside that marker correctly refuses. The strong
        // permits themselves are revalidated by the complete claim below.
        let mut continuous_permits = std::array::from_fn(|_| None);
        for (index, permit) in continuous_permits.iter_mut().enumerate() {
            if pending.voices[index].is_some_and(|expected| expected.continuous) {
                let Some(snapshot) = self.capture_pitch_voice_snapshot(index) else {
                    return false;
                };
                let Some(current) = self.capture_voice_history_permit(index, &snapshot) else {
                    return false;
                };
                *permit = Some(current);
            }
        }
        let ownership = self.input_runtime_ownership.clone();
        let Some(slot) = ownership.slot_snapshot(id) else {
            return false;
        };
        let Ok(write) = ownership.prepare_slot_write_fixed(slot, slot.lifetime()) else {
            return false;
        };
        let mixer = RefCell::new(self);
        ownership
            .commit_slot_write_fixed(
                &write,
                || {
                    mixer
                        .borrow_mut()
                        .key_lock_tuple_ready(id, pending, frame, &continuous_permits)
                },
                |_| {
                    let mut mixer = mixer.borrow_mut();
                    for index in 0..MAX_VOICES {
                        let Some(expected) = pending.voices[index] else {
                            continue;
                        };
                        if expected.continuous {
                            mixer.voices[index]
                                .stretch
                                .retain_unit_rate_mode_claimed(pending.request.enabled);
                            mixer.voice_key_lock[index] = pending.request.enabled;
                            continue;
                        }
                        let snapshot = mixer
                            .capture_pitch_voice_snapshot(index)
                            .expect("claimed voice snapshot");
                        let prospective = feed(
                            &snapshot,
                            expected.pitch,
                            pending.request.enabled,
                            frame,
                            None,
                        );
                        mixer.voices[index]
                            .stretch
                            .adopt_prospective_tuple_claimed(&prospective, expected.request_id);
                        mixer.voice_key_lock[index] = pending.request.enabled;
                    }
                    mixer.pad_key_lock_enabled[id] = pending.request.enabled;
                    mixer.pad_key_lock_pending_on[id] = false;
                    mixer.pad_key_lock_rejected[id] = false;
                },
            )
            .is_ok()
    }

    fn finish_key_lock_tuple(
        &mut self,
        id: usize,
        mut pending: PendingKeyLockTuple,
        accepted: bool,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        self.clear_continuous_key_lock_tuple(id, &pending);
        if !accepted {
            for (index, participant) in pending.voices.iter().enumerate() {
                if let Some(expected) = participant
                    && !expected.continuous
                    && self.voices[index].generation == expected.generation
                    && self.voices[index].stretch.preparation_epoch() == expected.epoch
                {
                    self.voices[index].stretch.invalidate_prepared();
                }
            }
            if self.pad_key_lock_request_id[id] == pending.request.request_id
                && self
                    .input_runtime_ownership
                    .callback_key_lock_request_current(id, pending.request.request_id)
            {
                self.pad_key_lock_rejected[id] = true;
            }
        }
        if let Some(work) = pending.resident.take() {
            if accepted {
                work.publication.mark_accepted();
            } else {
                work.publication.mark_rejected();
            }
            self.input_runtime_ownership
                .finish_resident_control_for_binding(id, work.binding, work.publication.expected);
            retirement.retire_resident_transaction(work);
        }
        self.publish_key_lock_status(id);
    }

    pub(super) fn poll_key_lock_tuples(
        &mut self,
        frame: Option<u64>,
        retirement: &mut impl AudioBufferRetirement,
    ) {
        for id in 0..NUM_SAMPLES {
            // Render polling has no control-message retirement reservation. Keep the entire
            // original owner, status and any queued supersession until its one retirement
            // slot is available. A missed exact E will reject when capacity returns.
            if self.owns_resident_key_lock_tuple(id) && retirement.available_retirement_slots() == 0
            {
                continue;
            }
            if let Some(request) = self.queued_key_lock_tuples[id].take() {
                if let Some(previous) = self.pending_key_lock_tuples[id].take() {
                    self.finish_key_lock_tuple(id, previous, false, retirement);
                }
                self.pending_key_lock_tuples[id] = Some(PendingKeyLockTuple::scalar(request));
            }
            let Some(mut pending) = self.pending_key_lock_tuples[id].take() else {
                continue;
            };
            let current = self.key_lock_tuple_current(id, &pending);
            let captured = if pending.execution.is_none() {
                current
                    && frame
                        .is_some_and(|frame| self.capture_key_lock_tuple(id, &mut pending, frame))
            } else {
                current
            };
            if !captured {
                self.finish_key_lock_tuple(id, pending, false, retirement);
            } else if frame
                .is_some_and(|frame| pending.execution.is_some_and(|target| frame >= target))
            {
                let accepted =
                    frame.is_some_and(|frame| self.commit_key_lock_tuple(id, &pending, frame));
                self.finish_key_lock_tuple(id, pending, accepted, retirement);
            } else {
                self.pending_key_lock_tuples[id] = Some(pending);
                self.publish_key_lock_status(id);
            }
        }
    }

    pub(super) fn chunk_until_key_lock_tuple(
        &self,
        frame: Option<u64>,
        max_frames: usize,
    ) -> usize {
        let Some(frame) = frame else {
            return max_frames;
        };
        self.pending_key_lock_tuples
            .iter()
            .filter_map(|pending| pending.as_ref()?.execution)
            .filter(|target| *target > frame)
            .fold(max_frames, |frames, target| {
                frames.min((target - frame).min(usize::MAX as u64) as usize)
            })
    }
}
