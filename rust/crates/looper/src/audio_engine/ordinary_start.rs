//! One prepared ordinary attack. The mixer freezes its actual applied tuple at admission.

use super::native_history_permit::{NativeHistoryContext, NativeHistoryPermit};
use super::pad_start::CapturedPadStart;
use super::pitch_actions::{FrozenPitchRender, PitchActionReject};
use super::prepared_native_history::{NativeAdapterState, NativeHistoryRequest};
use super::rubberband_backend::{RubberBandError, RubberBandLiveShifter};
use crate::messages::SampleBuffer;
use std::cell::UnsafeCell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub(crate) struct OrdinaryStartWork {
    origin: Option<CapturedPadStart>,
    sequence: u64,
    pub(crate) candidate: Option<Box<NativeAdapterState>>,
    render_format: (u32, usize),
    request: Option<NativeHistoryRequest>,
    rejected_request: Option<NativeHistoryRequest>,
    pub(crate) render: Option<FrozenPitchRender>,
    pub(crate) epoch: Arc<AtomicU64>,
    frames: Option<(u64, u64)>,
    preparation_required: bool,
    inherit_loop: bool,
    captured_loop: Option<(f64, Option<f64>)>,
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
            rejected_request: None,
            render: None,
            epoch: Arc::new(AtomicU64::new(1)),
            frames: None,
            preparation_required: true,
            inherit_loop: false,
            captured_loop: None,
        }))
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
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
        self.origin().freeze_frames(original, execution)?;
        self.frames = Some((original, execution));
        Ok(())
    }
    pub(crate) fn execution_frame(&self) -> u64 {
        self.frames.expect("ordinary target frozen").1
    }
    pub(crate) fn preparation_required(&self) -> bool {
        self.preparation_required
    }
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    pub(crate) fn is_ready(&self) -> bool {
        !self.preparation_required
            || self
                .candidate
                .as_ref()
                .is_some_and(|candidate| candidate.source.is_some())
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
        if self.candidate.is_some() || reserve.retired_request.is_some() {
            return false;
        }
        let Some(request) = self.request.as_ref() else {
            return false;
        };
        let Some(source) = reserve
            .candidate
            .as_ref()
            .and_then(|candidate| candidate.source.as_ref())
        else {
            return false;
        };
        if !source.same_endpoint_request(request)
            || !request.permit.matches_action_owner(
                self.origin().ownership(),
                self.origin().acknowledgements(),
                self.origin().content_key(),
                &self.epoch,
            )
        {
            return false;
        }
        let mut candidate = reserve.candidate.take().unwrap();
        reserve.retired_request = candidate.source.replace(self.request.take().unwrap());
        self.candidate = Some(candidate);
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
        self.preparation_required = self.request.is_some();
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
        self.origin().callback_guard()?;
        if !self.preparation_required {
            return Ok(());
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
        if self.candidate.is_none() {
            let (rate, channels) = self.render_format;
            let native = RubberBandLiveShifter::new(rate, channels)
                .map_err(|_| PitchActionReject::PreparationFailed)?;
            self.candidate = Some(Box::new(NativeAdapterState::new(Some(native), channels)));
        }
        self.candidate
            .as_mut()
            .unwrap()
            .prepare_source_at_endpoint(request)
            .map_err(|_| PitchActionReject::PreparationFailed)?;
        self.origin().callback_guard()?;
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
        if self.preparation_required {
            let source = self
                .candidate
                .as_ref()
                .and_then(|candidate| candidate.source.as_ref())
                .ok_or(PitchActionReject::PreparedMismatch)?;
            let render = self.render.ok_or(PitchActionReject::PreparedMismatch)?;
            if source.request_id != self.sequence
                || source.epoch != self.epoch()
                || source.target_output_frame != frame
                || source.pitch_intent != render.pitch
                || source.key_lock != render.key_lock
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
