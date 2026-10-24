//! Fixed observation of retry conditions, never an audio admission authority.

use super::*;
use crate::audio_engine::hold_action::HoldActionState;
use crate::audio_engine::input_mapping::{
    RuntimePolicyRetryRow, RuntimePolicyRow, RuntimePolicySnapshot,
};
use crate::audio_engine::native_history_permit::NativeSourceAdmissionIdentity;
use crate::audio_engine::runtime_policy_retry::{RetryObservationSlot, RetryToken};
use crate::audio_engine::source_reader::SourceLoopDomain;
use crate::audio_engine::source_sync_policy::{SourceSyncMode, SourceSyncRequest};
use crate::audio_engine::stretch_processor::SourceLaneRetryState;
use crate::audio_engine::transport::{TransportBeatProjection, TransportTimeline};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EarlyUnsupportedInputs {
    mapped: bool,
    target_mapped: bool,
    paused: bool,
    pitch: PitchIntent,
    key_lock: bool,
    requested_pitch: PitchIntent,
    requested_key_lock: bool,
    resident: u64,
    native: bool,
    retained: bool,
    lane: SourceLaneRetryState,
    pending_key_lock: bool,
    queued_key_lock: bool,
    hold: Option<(u64, HoldActionState)>,
    pending_pitch: bool,
}

impl EarlyUnsupportedInputs {
    pub(super) fn blocked(self) -> bool {
        self.mapped
            || self.target_mapped
            || self.paused
            || self.key_lock
            || self.pitch.total_semitones() != 0
            || self.requested_key_lock
            || self.requested_pitch.total_semitones() != 0
            || self.resident != 0
            || self.native
            || self.retained
            || self.lane.busy()
            || self.pending_key_lock
            || self.queued_key_lock
            || self.hold.is_some_and(|(_, state)| !state.terminal())
            || self.pending_pitch
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LateUnsupportedInputs {
    region_matches: bool,
    raw_domain_matches: bool,
    timing_domain_matches: bool,
    transition: bool,
    actual_feed: bool,
}

impl LateUnsupportedInputs {
    pub(super) fn blocked(self) -> bool {
        !self.region_matches
            || !self.raw_domain_matches
            || !self.timing_domain_matches
            || self.transition
            || self.actual_feed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DomainIdentity {
    frames: usize,
    region: FrameRange,
    period: Option<u64>,
}

impl From<SourceLoopDomain> for DomainIdentity {
    fn from(value: SourceLoopDomain) -> Self {
        Self {
            frames: value.sample_frames,
            region: value.region,
            period: value.musical_period.map(f64::to_bits),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RetryVoiceShape {
    id: usize,
    generation: u64,
    admission: NativeSourceAdmissionIdentity,
    source: ProductiveSourceBinding,
    request: SourceSyncRequest,
    map: Option<super::super::input_runtime_binding::SourceSyncWitness>,
    configured: Option<DomainIdentity>,
    region: FrameRange,
    timing_period: Option<u64>,
    selection: StemRenderSelection,
    early: EarlyUnsupportedInputs,
    loop_intent: Option<(usize, Option<usize>)>,
    late: Option<LateUnsupportedInputs>,
}

#[derive(Clone, Copy)]
struct RetryGlobalShape {
    rate: u32,
    channels: usize,
    speed: u64,
    master: Option<u64>,
    bpm_lock: bool,
    timing_revision: u64,
    drained: bool,
    pending: bool,
    projection: Option<TransportBeatProjection>,
}

impl PartialEq for RetryGlobalShape {
    fn eq(&self, other: &Self) -> bool {
        self.rate == other.rate
            && self.channels == other.channels
            && self.speed == other.speed
            && self.master == other.master
            && self.bpm_lock == other.bpm_lock
            && self.timing_revision == other.timing_revision
            && self.drained == other.drained
            && self.pending == other.pending
            && match (self.projection, other.projection) {
                (Some(a), Some(b)) => a.matches_exact(b),
                (None, None) => true,
                _ => false,
            }
    }
}

/// The actual once-read decision, not a later reconstruction of its reason.
#[derive(Clone, Copy)]
pub(super) enum UnsupportedEvidence {
    Early {
        index: usize,
        generation: u64,
        inputs: EarlyUnsupportedInputs,
    },
    Loop {
        index: usize,
        generation: u64,
        intent: Option<(usize, Option<usize>)>,
    },
    Late {
        index: usize,
        generation: u64,
        inputs: LateUnsupportedInputs,
    },
}

pub(super) struct PolicyRetryPublisher {
    slot: Arc<RetryObservationSlot>,
    rows: Box<[Option<RuntimePolicyRetryRow>]>,
    shapes: Box<[Option<RetryVoiceShape>]>,
    revision: u64,
    global: Option<RetryGlobalShape>,
    token: Option<RetryToken>,
}

impl Drop for PolicyRetryPublisher {
    fn drop(&mut self) {
        // The containing mixer already follows CallbackOwned's off-RT teardown.
        // A closed/rebound callback cannot leave its cached refusal qualified.
        // Superseded publishers invalidate only their private displaced slots.
        self.slot.invalidate();
    }
}

impl PolicyRetryPublisher {
    fn new(slot: Arc<RetryObservationSlot>) -> Box<Self> {
        Box::new(Self {
            slot,
            rows: vec![None; NUM_SAMPLES].into_boxed_slice(),
            shapes: vec![None; MAX_VOICES].into_boxed_slice(),
            revision: 0,
            global: None,
            token: None,
        })
    }

    fn prepare(&mut self, snapshot: &RuntimePolicySnapshot) {
        if self.revision == snapshot.revision() {
            return;
        }
        self.slot.invalidate();
        self.token = None;
        self.revision = snapshot.revision();
        for (id, row) in self.rows.iter_mut().enumerate() {
            *row = snapshot.retry_row(id);
        }
    }

    fn evidence_matches(&self, evidence: UnsupportedEvidence) -> bool {
        let (index, generation) = match evidence {
            UnsupportedEvidence::Early {
                index, generation, ..
            }
            | UnsupportedEvidence::Loop {
                index, generation, ..
            }
            | UnsupportedEvidence::Late {
                index, generation, ..
            } => (index, generation),
        };
        self.shapes
            .get(index)
            .copied()
            .flatten()
            .is_some_and(|shape| {
                shape.generation == generation
                    && match evidence {
                        UnsupportedEvidence::Early { inputs, .. } => shape.early == inputs,
                        UnsupportedEvidence::Loop { intent, .. } => shape.loop_intent == intent,
                        UnsupportedEvidence::Late { inputs, .. } => shape.late == Some(inputs),
                    }
            })
    }

    fn fences_current(&self, mixer: &RtMixer) -> bool {
        mixer.input_runtime_ownership.runtime_policy_revision() == Some(self.revision)
            && self.rows.iter().enumerate().all(|(id, row)| {
                row.is_none_or(|row| {
                    row.current(
                        &mixer.input_runtime_ownership,
                        &mixer.current_timing_acknowledgements,
                        id,
                    )
                })
            })
    }

    fn observe(&mut self, mixer: &RtMixer, timeline: &TransportTimeline) -> Option<RetryToken> {
        if self.revision == 0
            || !mixer
                .input_runtime_ownership
                .runtime_policy_retry
                .available()
            || !self.fences_current(mixer)
        {
            self.slot.invalidate();
            self.token = None;
            return None;
        }
        let global = RetryGlobalShape {
            rate: mixer.sample_rate_hz.to_bits(),
            channels: mixer.channels,
            speed: mixer.speed.to_bits(),
            master: mixer.master_period_seconds.map(f64::to_bits),
            bpm_lock: mixer.bpm_lock_enabled,
            timing_revision: mixer.global_rate_timing_revision(),
            drained: mixer.global_rate_drains_settled(),
            pending: mixer.global_rate_pending(),
            projection: timeline.beat_projection(),
        };
        let mut changed = self.global != Some(global);
        self.global = Some(global);
        for index in 0..MAX_VOICES {
            let shape = if mixer.voices[index].active {
                let id = mixer.voices[index].sample_id;
                let Some(row) = self.rows.get(id).copied().flatten() else {
                    self.slot.invalidate();
                    self.token = None;
                    return None;
                };
                let Some(shape) = mixer.policy_retry_voice_shape(index, row.row) else {
                    self.slot.invalidate();
                    self.token = None;
                    return None;
                };
                Some(shape)
            } else {
                None
            };
            changed |= self.shapes[index] != shape;
            self.shapes[index] = shape;
        }
        if !self.fences_current(mixer)
            || mixer.global_rate_timing_revision() != global.timing_revision
            || mixer
                .input_runtime_ownership
                .global_timing_publishing
                .load(Ordering::Acquire)
        {
            self.slot.invalidate();
            self.token = None;
            return None;
        }
        let condition = if !changed && let Some(token) = self.token {
            // An unchanged checkpoint is already a complete observation. Avoid
            // creating artificial unreadable intervals on every ordinary render.
            if self.slot.matches(token) {
                return Some(token);
            }
            token.condition
        } else {
            let Some(condition) = mixer
                .input_runtime_ownership
                .runtime_policy_retry
                .next_condition()
            else {
                // Keep the shape uncommitted: equal values on the next callback
                // must still try to acquire a new valid observation token.
                self.slot.invalidate();
                self.token = None;
                return None;
            };
            condition
        };
        let token = self
            .slot
            .publish(self.revision, global.timing_revision, condition);
        self.token = Some(token);
        Some(token)
    }
}

impl RtMixer {
    /// Control only, once the entire boxed constructor or explicit rebind succeeds.
    pub(crate) fn register_runtime_policy_retry_publisher(&mut self) {
        self.runtime_policy_retry = self
            .input_runtime_ownership
            .runtime_policy_retry
            .register()
            .map(PolicyRetryPublisher::new);
    }

    pub(crate) fn observe_runtime_policy_retry(
        &mut self,
        timeline: &TransportTimeline,
    ) -> Option<RetryToken> {
        let mut publisher = self.runtime_policy_retry.take()?;
        let token = publisher.observe(self, timeline);
        self.runtime_policy_retry = Some(publisher);
        token
    }

    pub(super) fn prepare_runtime_policy_retry(&mut self, snapshot: &RuntimePolicySnapshot) {
        if !snapshot.belongs_to(&self.input_runtime_ownership) || !snapshot.current() {
            return;
        }
        if let Some(publisher) = self.runtime_policy_retry.as_mut() {
            publisher.prepare(snapshot);
        }
    }

    pub(super) fn policy_retry_evidence_matches(
        &self,
        evidence: Option<UnsupportedEvidence>,
    ) -> bool {
        evidence.is_some_and(|evidence| {
            self.runtime_policy_retry
                .as_ref()
                .is_some_and(|publisher| publisher.evidence_matches(evidence))
        })
    }

    pub(super) fn scalar_policy_early_inputs(
        &self,
        index: usize,
        row: RuntimePolicyRow,
    ) -> Option<EarlyUnsupportedInputs> {
        let voice = &self.voices[index];
        let id = voice.sample_id;
        let key = row.binding.placement?.content_key()?;
        let (pitch, key_lock) = self.effective_rate_voice_tuple(index);
        Some(EarlyUnsupportedInputs {
            mapped: voice.source_playback.is_mapped(),
            target_mapped: row.request.mode == SourceSyncMode::Mapped,
            paused: voice.paused,
            pitch,
            key_lock,
            requested_pitch: row.pitch.intent,
            requested_key_lock: row.pitch.key_lock,
            resident: self
                .input_runtime_ownership
                .resident_control_for_cell(key.cell())
                .load(Ordering::Acquire),
            native: voice.stretch.has_active_native_ownership(),
            retained: voice.stretch.has_retained_source_input(),
            lane: voice.stretch.source_lane_retry_state(),
            pending_key_lock: self.pending_key_lock_tuples[id].is_some(),
            queued_key_lock: self.queued_key_lock_tuples[id].is_some(),
            hold: self.hold_actions[id]
                .as_ref()
                .map(|work| (work.action.transport_revision(), work.action.state())),
            pending_pitch: self
                .pitch_runtime
                .as_ref()
                .is_some_and(|runtime| runtime.has_pending_content(key)),
        })
    }

    pub(super) fn scalar_policy_late_inputs(
        &self,
        index: usize,
        raw: &PitchSourceSnapshot,
        intent: (usize, Option<usize>),
    ) -> LateUnsupportedInputs {
        self.policy_retry_late_inputs(
            index,
            &raw.playback,
            raw.plan,
            raw.actual_feed.is_some(),
            intent,
        )
    }

    fn policy_retry_late_inputs(
        &self,
        index: usize,
        playback: &SourcePlayback,
        plan: SourceReadPlan,
        actual_feed: bool,
        (start, end): (usize, Option<usize>),
    ) -> LateUnsupportedInputs {
        LateUnsupportedInputs {
            region_matches: effective_loop_region(start, end, plan.sample_frames)
                == Some(plan.loop_region),
            raw_domain_matches: playback.matches_domain(plan.domain()),
            timing_domain_matches: playback.matches_domain(
                self.timing_for_voice(&self.voices[index])
                    .loop_domain(plan.sample_frames, plan.loop_region),
            ),
            transition: plan.transition.is_active(),
            actual_feed,
        }
    }

    fn policy_retry_voice_shape(
        &self,
        index: usize,
        row: RuntimePolicyRow,
    ) -> Option<RetryVoiceShape> {
        let voice = &self.voices[index];
        if voice.generation == 0 || voice.generation == u64::MAX {
            return None;
        }
        let sample = voice.sample.as_ref()?;
        let admission = voice.source_admission.as_ref()?;
        let identity = admission.retry_identity();
        let region = voice
            .source_loop_region
            .or_else(|| self.effective_loop_region(voice.sample_id, sample.frame_count()))?;
        let timing = self.timing_for_voice(voice);
        let frozen = voice.frozen_stems.as_ref().filter(|_| {
            !self.sample_bank[voice.sample_id]
                .as_ref()
                .is_some_and(|bank| bank.same_source(sample))
        });
        let selection = frozen.map_or_else(
            || self.stem_render_selection(voice.sample_id),
            |view| view.selection,
        );
        let transition = frozen.map_or(self.stem_transitions[voice.sample_id], |view| {
            view.transition
        });
        let loop_intent =
            self.loop_intent_frames(voice.sample_id, row.loop_start_s, row.loop_end_s);
        let position = voice.source_playback.position();
        let plan = SourceReadPlan {
            channels: self.channels,
            sample_frames: sample.frame_count(),
            frame_pos: position.frame,
            loop_region: region,
            loop_period: voice.source_playback.loop_period(),
            seek_mode: position.seek_mode,
            selection,
            transition,
        };
        let actual_feed = voice
            .stretch
            .actual_source_checkpoint(&voice.source_playback, plan)
            .ok()?
            .is_some();
        Some(RetryVoiceShape {
            id: voice.sample_id,
            generation: voice.generation,
            admission: identity,
            source: ProductiveSourceBinding::new(
                sample,
                self.sample_rate_hz as u32,
                timing.accepted,
            ),
            request: voice.sync_request,
            map: voice.mapped_start.as_ref().and_then(|owner| {
                owner.source_sync_witness(admission.content_key()?, identity.source_generation)
            }),
            configured: voice.source_playback.retry_domain().map(Into::into),
            region,
            timing_period: timing.period_seconds().map(f64::to_bits),
            selection,
            early: self.scalar_policy_early_inputs(index, row)?,
            loop_intent,
            late: loop_intent.map(|intent| {
                self.policy_retry_late_inputs(
                    index,
                    &voice.source_playback,
                    plan,
                    actual_feed,
                    intent,
                )
            }),
        })
    }
}
