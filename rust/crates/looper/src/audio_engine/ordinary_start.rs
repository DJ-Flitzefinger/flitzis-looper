//! One prepared ordinary attack. The mixer freezes its actual applied tuple at admission.

use super::key_lock_attack_measurement::{
    PreparedAttackMeasurement, measure_attack_reference_for_birth,
};
use super::key_lock_start_policy::KeyLockStartBehavior;
use super::native_history_permit::{NativeHistoryContext, NativeHistoryPermit};
use super::pad_start::CapturedPadStart;
use super::pitch_actions::{FrozenPitchRender, PitchActionReject};
use super::prepared_native_history::{
    NativeAdapterState, NativeHistoryRequest, SourceReadCheckpoint,
};
use super::rubberband_backend::{RubberBandError, RubberBandLiveShifter};
use crate::messages::SampleBuffer;
use std::cell::UnsafeCell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

struct PendingLateReprepare {
    endpoint: SourceReadCheckpoint,
    previous_execution: u64,
}

pub(crate) struct OrdinaryStartWork {
    origin: Option<CapturedPadStart>,
    sequence: u64,
    pub(crate) candidate: Option<Box<NativeAdapterState>>,
    render_format: (u32, usize),
    request: Option<NativeHistoryRequest>,
    pub(crate) attack_measurement: Option<PreparedAttackMeasurement>,
    rejected_request: Option<NativeHistoryRequest>,
    pub(crate) render: Option<FrozenPitchRender>,
    pub(crate) epoch: Arc<AtomicU64>,
    frames: Option<(u64, u64)>,
    preparation_required: bool,
    native_required: bool,
    pub(crate) dsp_intent: Option<super::late_start_dsp::DspInputIntent>,
    launch_origin: Option<SourceReadCheckpoint>,
    pending_late_reprepare: Option<PendingLateReprepare>,
    inherit_loop: bool,
    captured_loop: Option<(f64, Option<f64>)>,
    mapped_clock: Option<super::mapped_source_clock::MappedClockEpoch>,
    mapped_origin_frame: Option<u64>,
    launch_reference: super::loop_phase_reference::LoopPhaseReference,
    launch_qualification: super::loop_phase_reference::LaunchQualification,
    pub(crate) phase_launch_qualified: bool,
}

impl OrdinaryStartWork {
    /// Control/dispatcher-only construction; never called by the audio callback.
    pub(crate) fn new(rate: u32, channels: usize) -> Result<Box<Self>, RubberBandError> {
        // Keep the exact existing C-ABI format validation even for unity starts.
        if rate == 0 {
            return Err(RubberBandError::InvalidSampleRate);
        }
        if u32::try_from(channels)
            .ok()
            .is_none_or(|channels| channels == 0)
        {
            return Err(RubberBandError::InvalidChannelCount);
        }
        let sequence = super::scheduler::reserve_input_order()
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        Ok(Box::new(Self {
            origin: None,
            sequence,
            candidate: None,
            render_format: (rate, channels),
            request: None,
            attack_measurement: None,
            rejected_request: None,
            render: None,
            epoch: Arc::new(AtomicU64::new(1)),
            frames: None,
            preparation_required: true,
            native_required: false,
            dsp_intent: None,
            launch_origin: None,
            pending_late_reprepare: None,
            inherit_loop: false,
            captured_loop: None,
            mapped_clock: None,
            mapped_origin_frame: None,
            launch_reference: super::loop_phase_reference::LoopPhaseReference::Unavailable,
            launch_qualification: super::loop_phase_reference::LaunchQualification::default(),
            phase_launch_qualified: false,
        }))
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
    }

    /// The existing scheduler supplies its actual immutable master projection.
    /// No map access or ownership allocation occurs on the callback.
    pub(crate) fn freeze_mapped_clock(
        &mut self,
        timeline: Option<super::transport::TransportTimeline>,
        original: u64,
        admission: u64,
    ) -> Result<(), PitchActionReject> {
        self.launch_reference =
            super::loop_phase_reference::LoopPhaseReference::capture(timeline, original);
        self.launch_qualification =
            super::loop_phase_reference::LaunchQualification::capture(timeline, admission);
        let Some(geometry) = self.origin().mapped_geometry() else {
            return Ok(());
        };
        if self.mapped_clock.is_some() {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let timeline = timeline.ok_or(PitchActionReject::MappedTimingUnavailable)?;
        let phase = -timeline
            .beat_position_at_frame(original)
            .ok_or(PitchActionReject::TimingUnavailable)?;
        let epoch =
            super::mapped_source_clock::MappedClockEpoch::new(self.sequence, timeline, phase)
                .ok_or(PitchActionReject::TimingUnavailable)?;
        geometry
            .validate_epoch(epoch)
            .map_err(|_| PitchActionReject::MappedTimingUnavailable)?;
        self.mapped_clock = Some(epoch);
        self.mapped_origin_frame = Some(original);
        Ok(())
    }

    pub(crate) fn mapped_clock(
        &self,
    ) -> Option<(super::mapped_source_clock::MappedClockEpoch, u64)> {
        self.mapped_clock.zip(self.mapped_origin_frame)
    }
    pub(crate) fn launch_reference(&self) -> super::loop_phase_reference::LoopPhaseReference {
        self.launch_reference
    }

    pub(crate) fn qualify_phase_launch_at(
        &mut self,
        timeline: Option<super::transport::TransportTimeline>,
        execution: u64,
    ) {
        self.phase_launch_qualified =
            self.launch_qualification
                .at_effect(self.launch_reference, timeline, execution);
    }
    pub(crate) fn validate_mapped_timeline(
        &self,
        timeline: Option<super::transport::TransportTimeline>,
    ) -> Result<(), PitchActionReject> {
        if self
            .mapped_clock
            .is_some_and(|epoch| timeline.is_none_or(|actual| !epoch.matches_timeline(&actual)))
        {
            return Err(PitchActionReject::MappedTimingUnavailable);
        }
        Ok(())
    }
    pub(crate) fn use_current_loop(&mut self) {
        self.inherit_loop = true;
    }
    pub(crate) fn inherits_loop(&self) -> bool {
        self.inherit_loop
    }
    pub(crate) fn loop_region(&self) -> (f64, Option<f64>) {
        self.captured_loop
            .unwrap_or_else(|| self.origin().loop_region())
    }
    pub(crate) fn freeze_loop(&mut self, region: (f64, Option<f64>)) {
        self.captured_loop = Some(region);
        self.inherit_loop = false;
    }
    pub(crate) fn origin(&self) -> &CapturedPadStart {
        self.origin
            .as_ref()
            .expect("ordinary work is bound before publication")
    }
    pub(crate) fn bind_origin(
        &mut self,
        origin: CapturedPadStart,
    ) -> Result<(), PitchActionReject> {
        if origin.sequence() != self.sequence {
            return Err(PitchActionReject::PreparedMismatch);
        }
        if let Some(current) = &self.origin {
            return if current.same_owner(&origin) {
                Ok(())
            } else {
                Err(PitchActionReject::PreparedMismatch)
            };
        }
        self.origin = Some(origin);
        Ok(())
    }

    /// Failed control admission returns the same unused candidate to its owner.
    /// Retention must break the temporary Work -> owner Arc to avoid a cycle.
    pub(crate) fn detach_unpublished_origin(
        &mut self,
        origin: &CapturedPadStart,
    ) -> Result<(), PitchActionReject> {
        if self.sequence != origin.sequence()
            || self
                .origin
                .as_ref()
                .is_none_or(|current| !current.same_owner(origin))
            || self.frames.is_some()
            || self.render.is_some()
            || self.request.is_some()
            || self.rejected_request.is_some()
            || self.candidate.is_some()
        {
            return Err(PitchActionReject::PreparedMismatch);
        }
        self.origin = None;
        Ok(())
    }
    pub(crate) fn freeze_frames(
        &mut self,
        original: u64,
        execution: u64,
    ) -> Result<(), PitchActionReject> {
        if self.frames.is_some() || execution < original {
            return Err(PitchActionReject::PreparedMismatch);
        }
        // Native/FIFO mapped preparation and delayed attack reconstruction are
        // separate admission gates. Never skip the first attack to catch up.
        if self.mapped_clock.is_some() && execution != original {
            return Err(PitchActionReject::MappedTimingUnavailable);
        }
        self.origin().freeze_frames(original, execution)?;
        self.frames = Some((original, execution));
        Ok(())
    }
    pub(crate) fn execution_frame(&self) -> u64 {
        self.frames.expect("ordinary target frozen").1
    }
    pub(crate) fn freeze_launch_origin(
        &mut self,
        origin: SourceReadCheckpoint,
    ) -> Result<(), PitchActionReject> {
        if self.launch_origin.is_some()
            || !origin.is_consistent()
            || origin.playback.has_phase_program()
            || origin.plan.seek_mode != super::source_reader::ExplicitSeekMode::Normal
            || origin.plan.transition.is_active()
            || origin.playback.tempo_ratio().to_bits() != origin.playback.rate_target().to_bits()
        {
            return Err(PitchActionReject::PreparedMismatch);
        }
        self.launch_origin = Some(origin);
        Ok(())
    }
    /// Copy this original's physical beginning, never a later mutable bank phase.
    pub(crate) fn late_reprepare_snapshot(&self) -> Option<SourceReadCheckpoint> {
        self.launch_origin
    }
    /// Callback-only scalar planning. Existing Native, source pins and DSP retire on the worker.
    pub(crate) fn plan_late_reprepare(
        &mut self,
        endpoint: SourceReadCheckpoint,
        dsp_intent: super::late_start_dsp::DspInputIntent,
        execution: u64,
    ) -> Result<(), PitchActionReject> {
        if self.mapped_clock.is_some() {
            return Err(PitchActionReject::MappedTimingUnavailable);
        }
        let (original, previous_execution) =
            self.frames.ok_or(PitchActionReject::PreparedMismatch)?;
        let origin = self
            .launch_origin
            .ok_or(PitchActionReject::PreparedMismatch)?;
        let source = self
            .request
            .as_ref()
            .or_else(|| {
                self.candidate
                    .as_ref()
                    .and_then(|candidate| candidate.source.as_ref())
            })
            .ok_or(PitchActionReject::PreparedMismatch)?;
        if self.pending_late_reprepare.is_some()
            || execution <= previous_execution
            || execution - original
                > super::source_feed_history::SOURCE_FEED_HISTORY_MAX_FRAMES as u64
            || !origin.matches_exact(endpoint)
            || source.request_id != self.sequence
            || source.epoch != self.epoch()
            || source.target_output_frame != previous_execution
            || self.render.is_none_or(|render| {
                source.pitch_intent != render.pitch || source.key_lock != render.key_lock
            })
            || !source
                .permit
                .current(&source.sample, source.binding.sample_rate_hz)
            || !source.permit.matches_action_owner(
                self.origin().ownership(),
                self.origin().acknowledgements(),
                self.origin().content_key(),
                &self.epoch,
            )
        {
            return Err(PitchActionReject::PreparedMismatch);
        }
        self.origin()
            .replan_execution_frame(previous_execution, execution)?;
        self.dsp_intent = Some(dsp_intent);
        self.pending_late_reprepare = Some(PendingLateReprepare {
            endpoint,
            previous_execution,
        });
        self.frames = Some((original, execution));
        self.preparation_required = true;
        Ok(())
    }
    /// Elapsed wall-clock delay remains relevant to source/control fences, not source skipping.
    pub(crate) fn launch_delay_frames(&self) -> u64 {
        self.frames
            .map_or(0, |(original, execution)| execution - original)
    }
    pub(crate) fn preparation_required(&self) -> bool {
        self.preparation_required
    }
    pub(crate) fn native_required(&self) -> bool {
        self.native_required
    }
    pub(crate) fn dry_source_matches(
        &self,
        snapshot: &super::prepared_native_history::PitchSourceSnapshot,
    ) -> bool {
        !self.native_required
            && self
                .request
                .as_ref()
                .zip(self.render)
                .is_some_and(|(request, render)| {
                    request.matches_endpoint(snapshot, render)
                        && request
                            .permit
                            .current(&request.sample, request.binding.sample_rate_hz)
                })
    }
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    pub(crate) fn is_ready(&self) -> bool {
        self.render.is_some()
            && self.pending_late_reprepare.is_none()
            && (!self.preparation_required
                || !self.native_required
                || self
                    .candidate
                    .as_ref()
                    .is_some_and(|candidate| candidate.source.is_some()))
    }
    pub(crate) fn validates_render_format(&self, rate: u32, channels: usize) -> bool {
        self.render_format == (rate, channels)
    }

    /// Move one exact physical endpoint. Its cache permit is retained for worker retirement;
    /// only this original action's fresh request/permit travels to strict adoption.
    pub(crate) fn take_endpoint(
        &mut self,
        reserve: &mut super::ordinary_endpoint_reserve::EndpointReserveWork,
    ) -> bool {
        // A neutral cached endpoint has no source-event measurement for this action.
        // The original worker retains custody and produces its own proposal instead.
        if self.origin().start_behavior() == KeyLockStartBehavior::AttackAtGrid {
            return false;
        }
        let Some(request) = self.request.as_ref() else {
            return false;
        };
        if request.request_id != self.sequence()
            || request.epoch != self.epoch()
            || request.target_output_frame != self.execution_frame()
            || self.render.is_none_or(|render| {
                request.pitch_intent != render.pitch || request.key_lock != render.key_lock
            })
            || !request.permit.matches_action_owner(
                self.origin().ownership(),
                self.origin().acknowledgements(),
                self.origin().content_key(),
                &self.epoch,
            )
        {
            return false;
        }
        if !reserve.transfer_to(&mut self.request, &mut self.candidate, self.render_format) {
            return false;
        }
        self.origin().mark_prepared();
        true
    }
    pub(crate) fn reject(&self, reason: PitchActionReject) {
        self.origin().reject(reason);
        self.epoch.store(0, Ordering::Release);
    }
    pub(crate) fn capture_history_permit(
        &self,
        sample: &SampleBuffer,
        rate: u32,
    ) -> Option<NativeHistoryPermit> {
        let origin = self.origin();
        let guard = origin.callback_guard().ok()?;
        NativeHistoryContext {
            id: guard.id,
            ownership: origin.ownership(),
            acknowledgements: origin.acknowledgements(),
            preparation_epoch: &self.epoch,
        }
        .capture_admitted(sample, rate, guard.binding)?
        .for_action(
            origin.ownership(),
            origin.acknowledgements(),
            origin.content_key(),
            &self.epoch,
        )
    }

    /// Retain all request pins even on rejection, for intact off-callback retirement.
    pub(crate) fn capture(
        &mut self,
        request: Option<NativeHistoryRequest>,
        render: FrozenPitchRender,
    ) -> Result<(), PitchActionReject> {
        if self.request.is_some() || self.render.is_some() {
            debug_assert!(self.rejected_request.is_none());
            self.rejected_request = request;
            return Err(PitchActionReject::PreparedMismatch);
        }
        self.request = request;
        self.render = Some(render);
        self.native_required = self.request.as_ref().is_some_and(|request| {
            render
                .pitch
                .native_pitch_scale(request.playback.tempo_ratio(), render.key_lock)
                .is_ok_and(|scale| scale != 1.0)
        });
        self.preparation_required = self.request.is_some() && self.native_required;
        self.origin().callback_guard()?;
        let valid = self.request.as_ref().is_none_or(|request| {
            request.request_id == self.sequence
                && request.epoch == self.epoch()
                && request.target_output_frame == self.execution_frame()
                && request.pitch_intent == render.pitch
                && request.key_lock == render.key_lock
                && request.permit.matches_action_owner(
                    self.origin().ownership(),
                    self.origin().acknowledgements(),
                    self.origin().content_key(),
                    &self.epoch,
                )
        });
        if !valid {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let advanced = if self.preparation_required {
            self.origin().mark_preparing()
        } else {
            self.origin().mark_prepared()
        };
        if !advanced {
            return Err(PitchActionReject::StaleContent);
        }
        Ok(())
    }

    pub(crate) fn prepare(&mut self) -> Result<(), PitchActionReject> {
        self.origin().preparation_guard()?;
        if !self.preparation_required {
            return Ok(());
        }
        if let Some(pending) = self.pending_late_reprepare.take() {
            let mut request = self
                .request
                .take()
                .or_else(|| {
                    self.candidate
                        .as_mut()
                        .and_then(|candidate| candidate.source.take())
                })
                .ok_or(PitchActionReject::PreparedMismatch)?;
            if request.target_output_frame != pending.previous_execution
                || !request.plan.matches_source_contract(pending.endpoint.plan)
            {
                return Err(PitchActionReject::PreparedMismatch);
            }
            request.target_output_frame = self.execution_frame();
            request.playback = pending.endpoint.playback;
            request.plan = pending.endpoint.plan;
            request.actual_feed = None;
            self.request = Some(request);
            self.attack_measurement = None;
        }
        let request = self
            .request
            .take()
            .ok_or(PitchActionReject::PreparedMismatch)?;
        if request.epoch != self.epoch()
            || !request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
            || !request.permit.matches_action_owner(
                self.origin().ownership(),
                self.origin().acknowledgements(),
                self.origin().content_key(),
                &self.epoch,
            )
        {
            return Err(PitchActionReject::SourceChanged);
        }
        if self.native_required && self.candidate.is_none() {
            let (rate, channels) = self.render_format;
            let native = RubberBandLiveShifter::new(rate, channels)
                .map_err(|_| PitchActionReject::PreparationFailed)?;
            self.candidate = Some(Box::new(NativeAdapterState::new(Some(native), channels)));
        }
        if self.native_required
            && self.origin().start_behavior() == KeyLockStartBehavior::AttackAtGrid
        {
            self.attack_measurement = Some(measure_attack_reference_for_birth(&request));
            self.origin().preparation_guard()?;
        }
        if self.native_required {
            self.candidate
                .as_mut()
                .unwrap()
                .prepare_source_at_physical_birth(request)
                .map_err(|_| PitchActionReject::PreparationFailed)?;
        } else {
            // The dry worker retains the original source pins and permit for strict
            // adoption and off-callback retirement, without constructing Native.
            self.request = Some(request);
        }
        self.origin().preparation_guard()?;
        if !self.origin().mark_prepared() {
            return Err(PitchActionReject::StaleContent);
        }
        Ok(())
    }

    pub(crate) fn rt_ready(&self, frame: u64) -> Result<(), PitchActionReject> {
        self.origin().callback_guard()?;
        if frame != self.execution_frame() || !self.is_ready() {
            return Err(PitchActionReject::PreparedMismatch);
        }
        if self.native_required
            && self
                .candidate
                .as_ref()
                .is_none_or(|candidate| !candidate.has_physical_birth_context())
        {
            return Err(PitchActionReject::PreparedMismatch);
        }
        if self.preparation_required {
            let source = if self.native_required {
                self.candidate
                    .as_ref()
                    .and_then(|candidate| candidate.source.as_ref())
            } else {
                self.request.as_ref()
            }
            .ok_or(PitchActionReject::PreparedMismatch)?;
            let render = self.render.ok_or(PitchActionReject::PreparedMismatch)?;
            if source.request_id != self.sequence
                || source.epoch != self.epoch()
                || source.target_output_frame != frame
                || source.pitch_intent != render.pitch
                || source.key_lock != render.key_lock
                || (self.native_required
                    && self.origin().start_behavior() == KeyLockStartBehavior::AttackAtGrid
                    && self
                        .attack_measurement
                        .as_ref()
                        .is_none_or(|measurement| !measurement.matches_birth_request(source)))
                || !source.permit.matches_action_owner(
                    self.origin().ownership(),
                    self.origin().acknowledgements(),
                    self.origin().content_key(),
                    &self.epoch,
                )
            {
                return Err(PitchActionReject::PreparedMismatch);
            }
        }
        Ok(())
    }
    pub(crate) fn claim_effect(&self) -> bool {
        self.origin().claim_effect()
    }
}

/// The original control-allocated group container also retires off the callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OrdinaryGroupKey {
    pub(crate) slot: usize,
    pub(crate) action_sequence: u64,
}

pub(crate) struct OrdinaryStartGroup {
    pub(crate) batch: Arc<super::global_playback_batch::GlobalPlaybackBatch>,
    pub(crate) work: Box<[Option<Box<OrdinaryStartWork>>]>,
    pub(crate) slots: [usize; super::constants::MAX_VOICES],
    pub(crate) len: usize,
}

impl OrdinaryStartGroup {
    pub(crate) fn reject(&self, reason: PitchActionReject) {
        for work in self.work.iter().flatten() {
            work.reject(reason);
        }
        self.batch.finish(false);
    }
}

/// Cloneable scalar publication, with exactly one owner transfer of the prepared-capable work.
pub(crate) struct OrdinaryStartPublication {
    claimed: AtomicBool,
    work: UnsafeCell<Option<Box<OrdinaryStartWork>>>,
}
// Producer initializes before queue publication. Only the successful atomic claimant accesses
// the cell; all losing clones are empty scalar publications and retire off the callback.
unsafe impl Send for OrdinaryStartPublication {}
unsafe impl Sync for OrdinaryStartPublication {}
impl OrdinaryStartPublication {
    pub(crate) fn new(work: Box<OrdinaryStartWork>) -> Self {
        Self {
            claimed: AtomicBool::new(false),
            work: UnsafeCell::new(Some(work)),
        }
    }
    pub(crate) fn take(&self) -> Option<Box<OrdinaryStartWork>> {
        self.claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        unsafe { (&mut *self.work.get()).take() }
    }
}
impl std::fmt::Debug for OrdinaryStartPublication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrdinaryStartPublication")
            .field("claimed", &self.claimed.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}
