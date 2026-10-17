//! One selected Normal-start endpoint, separate from accepted action custody.
//!
//! Only the existing worker constructs/prepares/recycles Native. The callback moves
//! complete owners and compares exact source data; cache permission is never action permission.

use super::content_placement::ContentKey;
use super::mixer::RtMixer;
use super::native_history_permit::NativeHistoryContext;
use super::ordinary_start::OrdinaryStartWork;
use super::pitch_actions::{FrozenPitchRender, PitchActionPreparation, PitchActionWork};
use super::prepared_native_history::{
    NativeAdapterState, NativeHistoryRequest, PitchSourceSnapshot,
};
use super::productive_source_history::ProductiveSourceBinding;
use super::rubberband_backend::RubberBandLiveShifter;
use super::source_playback::SourcePlayback;
use super::source_reader::SourceReadPlan;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Comparison markers only. The actual work request retains every source/stem pin.
#[derive(Clone, Copy)]
pub(crate) struct EndpointDescriptor {
    id: usize,
    key: ContentKey,
    binding: ProductiveSourceBinding,
    source_generation: u64,
    authority: u64,
    source_incarnation: u64,
    playback: SourcePlayback,
    plan: SourceReadPlan,
    render: FrozenPitchRender,
    stem_owners: [usize; 6],
}

impl EndpointDescriptor {
    pub(crate) fn new(
        id: usize,
        key: ContentKey,
        snapshot: &PitchSourceSnapshot,
        source_generation: u64,
        authority: u64,
        source_incarnation: u64,
        render: FrozenPitchRender,
    ) -> Self {
        let mut stem_owners = [0; 6];
        if let Some(stems) = snapshot.stems.as_ref() {
            stem_owners[0] = Arc::as_ptr(&stems.complete_set_identity) as usize;
            stem_owners[1] = stems.reference_samples.as_ptr() as usize;
            for (index, stem) in stems.stems.iter().enumerate() {
                stem_owners[index + 2] = stem.samples.as_ptr() as usize;
            }
        }
        Self {
            id,
            key,
            binding: ProductiveSourceBinding::new(
                &snapshot.sample,
                snapshot.sample_rate_hz,
                snapshot.accepted,
            ),
            source_generation,
            authority,
            source_incarnation,
            playback: snapshot.playback,
            plan: snapshot.plan,
            render,
            stem_owners,
        }
    }

    fn matches(self, other: Self) -> bool {
        self.id == other.id
            && self.key == other.key
            && self.binding == other.binding
            && self.source_generation == other.source_generation
            && self.authority == other.authority
            && self.source_incarnation == other.source_incarnation
            && self.render == other.render
            && self.stem_owners == other.stem_owners
            && self.playback.matches_exact(&other.playback)
            && self.plan.matches_exact(other.plan)
    }
}

pub(crate) struct SelectedEndpointSnapshot {
    pub(crate) id: usize,
    pub(crate) descriptor: EndpointDescriptor,
    pub(crate) snapshot: PitchSourceSnapshot,
    pub(crate) render: FrozenPitchRender,
}

/// Exactly one Box exists, on callback or on its dedicated one-item worker lane.
pub(crate) struct EndpointReserveWork {
    pub(crate) candidate: Option<Box<NativeAdapterState>>,
    pub(crate) request: Option<NativeHistoryRequest>,
    pub(crate) retired_request: Option<NativeHistoryRequest>,
    format: Option<(u32, usize)>,
    generation: u64,
    attempted: bool,
    ready: bool,
}

impl EndpointReserveWork {
    fn new() -> Box<Self> {
        Box::new(Self {
            candidate: None,
            request: None,
            retired_request: None,
            format: None,
            generation: 1,
            attempted: true,
            ready: false,
        })
    }

    /// Move complete DSP state only after the caller proves its own action envelope.
    /// Retain the displaced cache request for the existing worker to destroy.
    pub(crate) fn transfer_to(
        &mut self,
        request: &mut Option<NativeHistoryRequest>,
        candidate: &mut Option<Box<NativeAdapterState>>,
        render_format: (u32, usize),
    ) -> bool {
        if !self.ready
            || candidate.is_some()
            || self.retired_request.is_some()
            || self.format != Some(render_format)
        {
            return false;
        }
        let Some(original) = request.as_ref() else {
            return false;
        };
        let Some(source) = self
            .candidate
            .as_ref()
            .and_then(|state| state.source.as_ref())
        else {
            return false;
        };
        if (original.binding.sample_rate_hz, original.plan.channels) != render_format
            || !source.same_endpoint_request(original)
        {
            return false;
        }
        let mut prepared = self.candidate.take().unwrap();
        self.retired_request = prepared.source.replace(request.take().unwrap());
        *candidate = Some(prepared);
        // The move consumes readiness even if the action is concurrently cancelled.
        self.ready = false;
        self.attempted = false;
        true
    }

    /// Worker only, including all failure/recycle drops and Native construction.
    pub(crate) fn prepare(&mut self) {
        self.ready = false;
        self.attempted = true;
        self.retired_request = None;
        let Some(request) = self.request.take() else {
            self.candidate = None;
            self.format = None;
            return;
        };
        if !request
            .permit
            .endpoint_source_current(&request.sample, request.binding.sample_rate_hz)
        {
            self.candidate = None;
            self.format = None;
            return;
        }
        let format = (request.binding.sample_rate_hz, request.plan.channels);
        if self.format != Some(format) || self.candidate.is_none() {
            self.candidate = None;
            let Ok(native) = RubberBandLiveShifter::new(format.0, format.1) else {
                return;
            };
            self.candidate = Some(Box::new(NativeAdapterState::new(Some(native), format.1)));
            self.format = Some(format);
        }
        let candidate = self.candidate.as_mut().unwrap();
        self.ready = candidate.prepare_source_at_endpoint(request).is_ok()
            && candidate.source.as_ref().is_some_and(|source| {
                source
                    .permit
                    .endpoint_source_current(&source.sample, source.binding.sample_rate_hz)
            });
        if !self.ready {
            self.candidate = None;
            self.format = None;
        }
    }
}

pub(crate) struct OrdinaryEndpointReserve {
    epoch: Arc<AtomicU64>,
    desired: Option<EndpointDescriptor>,
    work: Option<Box<EndpointReserveWork>>,
}

impl OrdinaryEndpointReserve {
    pub(crate) fn new() -> Self {
        Self {
            epoch: Arc::new(AtomicU64::new(1)),
            desired: None,
            work: Some(EndpointReserveWork::new()),
        }
    }

    pub(crate) fn poll(&mut self, preparation: &mut PitchActionPreparation) {
        if let Some(work) = preparation.poll_endpoint() {
            debug_assert!(self.work.is_none());
            self.work = Some(work);
        }
    }

    pub(crate) fn service(&mut self, mixer: &RtMixer, preparation: &mut PitchActionPreparation) {
        let capture = mixer.capture_selected_normal_endpoint();
        let next = capture.as_ref().map(|capture| capture.descriptor);
        let same = match (self.desired, next) {
            (Some(previous), Some(next)) => previous.matches(next),
            (None, None) => true,
            _ => false,
        };
        if !same {
            let epoch = self.epoch.load(Ordering::Acquire);
            let next_epoch = epoch.checked_add(1).unwrap_or(0);
            self.epoch.store(next_epoch, Ordering::Release);
            self.desired = next;
        }
        let generation = self.epoch.load(Ordering::Acquire);
        let Some(work) = self.work.as_mut() else {
            return;
        };
        if generation == 0 || work.generation == generation && work.attempted {
            return;
        }
        if !preparation.endpoint_available() {
            return;
        }
        let request = capture.and_then(|capture| {
            let permit = NativeHistoryContext {
                id: capture.id,
                ownership: mixer.input_ownership(),
                acknowledgements: mixer.timing_acknowledgements(),
                preparation_epoch: &self.epoch,
            }
            .capture(
                &capture.snapshot.sample,
                capture.snapshot.sample_rate_hz,
                capture.snapshot.accepted,
            )?;
            capture.snapshot.into_request(
                permit,
                capture.render.pitch,
                capture.render.key_lock,
                0,
                generation,
                generation,
            )
        });
        if self.desired.is_some() && request.is_none() {
            return;
        }
        debug_assert!(work.request.is_none());
        work.request = request;
        work.generation = generation;
        work.attempted = false;
        work.ready = false;
        preparation.publish_endpoint(&mut self.work);
    }

    fn ready_source(&self, mixer: &RtMixer) -> Option<&NativeHistoryRequest> {
        let current = mixer.capture_selected_normal_endpoint()?;
        if self
            .desired
            .is_none_or(|desired| !desired.matches(current.descriptor))
        {
            return None;
        }
        let work = self.work.as_ref().filter(|work| work.ready)?;
        if work.generation != self.epoch.load(Ordering::Acquire) {
            return None;
        }
        work.candidate
            .as_ref()
            .and_then(|candidate| candidate.source.as_ref())
    }

    pub(crate) fn ready_for(&self, mixer: &RtMixer, action: &OrdinaryStartWork) -> bool {
        self.ready_source(mixer)
            .is_some_and(|source| mixer.ordinary_endpoint_matches(action, source))
    }

    pub(crate) fn ready_for_pitch(
        &self,
        mixer: &RtMixer,
        action: &PitchActionWork,
        projection_frames: u64,
    ) -> bool {
        self.ready_source(mixer)
            .is_some_and(|source| mixer.pitch_endpoint_matches(action, source, projection_frames))
    }

    pub(crate) fn consume(&mut self, action: &mut OrdinaryStartWork) -> bool {
        let Some(work) = self.work.as_mut().filter(|work| work.ready) else {
            return false;
        };
        action.take_endpoint(work)
    }

    pub(crate) fn consume_pitch(&mut self, action: &mut PitchActionWork) -> bool {
        let Some(work) = self.work.as_mut().filter(|work| work.ready) else {
            return false;
        };
        action.take_endpoint(work)
    }

    #[cfg(test)]
    pub(crate) fn ready(&self) -> bool {
        self.work.as_ref().is_some_and(|work| work.ready)
    }
    #[cfg(test)]
    pub(crate) fn native_address(&self) -> usize {
        self.work
            .as_ref()
            .and_then(|work| work.candidate.as_ref())
            .and_then(|candidate| candidate.rubberband.as_ref())
            .map_or(0, RubberBandLiveShifter::state_address)
    }
}
