//! Source-specific native history and its complete fixed adapter ownership.
//!
//! Preparation runs on the existing Key Lock worker. Rendering and worker catch-up use the same
//! adapter, while every source pin travels with the native/FIFO owner until worker retirement.

use super::native_history_permit::NativeHistoryPermit;
use super::native_source_coverage::{
    NormalLoopCoverage, complete_view_available, normal_loop_feed_available,
    normal_loop_feed_available_with_readers,
};
use super::pitch_intent::PitchIntent;
use super::productive_source_history::ProductiveSourceBinding;
#[cfg(test)]
use super::rubberband_backend::pitch_scale_for_tempo_ratio;
use super::rubberband_backend::{RubberBandError, RubberBandLiveShifter};
use super::source_feed_history::SourceFeedContext;
use super::source_playback::SourcePlayback;
use super::source_reader::{SourceReadPlan, StemReadSets, StemTransition};
use super::stretch_processor::{DEFAULT_BLOCK_SAMPLES, FixedFifo, ProductiveSourceFeed};
use crate::messages::{PreparedStemSet, SampleBuffer};
use std::sync::Arc;

#[path = "retained_native_start.rs"]
mod retained_start;
use retained_start::RetainedNativeStartTimeline;

pub(crate) const PREPARED_HISTORY_FRAMES: usize = 4096;
// A near-unity request remains a real request. Only actual native p=1 permits dry bypass.
pub(crate) const PITCH_SCALE_EPSILON: f64 = 0.0;

/// Keep the original constant-rate Native origin when a launch executes after its target.
/// The handle must already have its exact pitch/reset prepared. This is a preparation
/// bound, never an acoustic-delay or principal-attack estimate.
pub(crate) fn launch_history_output_frames(
    native: &RubberBandLiveShifter,
    request: &NativeHistoryRequest,
    catchup_frames: u64,
) -> Result<usize, RubberBandError> {
    let pitch = request
        .pitch_intent
        .native_pitch_scale(request.playback.tempo_ratio(), request.key_lock)
        .map_err(|_| RubberBandError::InvalidPitchScale)?;
    let catchup =
        usize::try_from(catchup_frames).map_err(|_| RubberBandError::UnavailableSourceHistory)?;
    native
        .source_history_frames(pitch)?
        .checked_add(catchup)
        .filter(|history| *history <= super::source_feed_history::SOURCE_FEED_HISTORY_MAX_FRAMES)
        .ok_or(RubberBandError::UnavailableSourceHistory)
}

/// Preserve the canonical T epoch after the rewound preparation prefix. Algebraically
/// equal fractional positions can address different f32 interpolation taps without it.
pub(crate) fn launch_source_rebase(
    request: &NativeHistoryRequest,
    history_frames: usize,
    catchup_frames: u64,
) -> Result<Option<(usize, SourceReadCheckpoint)>, RubberBandError> {
    if catchup_frames == 0 {
        return Ok(None);
    }
    let unavailable = || RubberBandError::UnavailableSourceHistory;
    let catchup = usize::try_from(catchup_frames).map_err(|_| unavailable())?;
    let boundary = history_frames
        .checked_sub(catchup)
        .ok_or_else(unavailable)?;
    let (playback, silence) = request
        .playback
        .history_before_constant(catchup)
        .ok_or_else(unavailable)?;
    if silence != 0 || boundary == 0 {
        return Err(unavailable());
    }
    let position = playback.position();
    let checkpoint = SourceReadCheckpoint {
        playback,
        plan: SourceReadPlan {
            frame_pos: position.frame,
            seek_mode: position.seek_mode,
            ..request.plan
        },
    };
    let mut projected = checkpoint;
    if !checkpoint.is_consistent()
        || !projected.project_output_frames(catchup_frames)
        || !projected.matches_exact(SourceReadCheckpoint {
            playback: request.playback,
            plan: request.plan,
        })
    {
        return Err(unavailable());
    }
    Ok(Some((boundary, checkpoint)))
}

/// Explicit raw stream coordinates; declaring these does not accept a musical anchor.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RetainedNativeStartCut {
    pub(crate) history_output_frames: usize,
    pub(crate) discarded_raw_output_frames: usize,
}

/// A new physical birth has no previously played input. Genuine continuations
/// retain the existing source-origin proof and must never use this silent context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeStartHistoryContext {
    SourceHistory,
    PhysicalBirth,
}

/// Source/history admission failures belong to one request. Adapter/backend faults still
/// terminate the worker; in particular, its FIFO InvalidBlockSize is not a coverage rejection.
#[derive(Debug)]
pub(crate) enum NativeSourcePreparationError {
    Unavailable(RubberBandError),
    Backend(RubberBandError),
}

impl NativeSourcePreparationError {
    fn from_history(error: RubberBandError) -> Self {
        match error {
            RubberBandError::UnavailableSourceHistory => Self::Unavailable(error),
            _ => Self::Backend(error),
        }
    }

    fn from_coverage(error: RubberBandError) -> Self {
        match error {
            RubberBandError::InvalidBlockSize | RubberBandError::UnavailableSourceHistory => {
                Self::Unavailable(error)
            }
            _ => Self::Backend(error),
        }
    }

    fn into_backend(self) -> RubberBandError {
        match self {
            Self::Unavailable(error) | Self::Backend(error) => error,
        }
    }
}

/// One source-read trajectory, independent of its logical or Native-input role.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SourceReadCheckpoint {
    pub(crate) playback: SourcePlayback,
    pub(crate) plan: SourceReadPlan,
}

impl SourceReadCheckpoint {
    pub(crate) fn matches_exact(self, other: Self) -> bool {
        self.playback.matches_exact(&other.playback) && self.plan.matches_exact(other.plan)
    }

    pub(crate) fn is_consistent(self) -> bool {
        let position = self.playback.position();
        self.playback.matches_domain(self.plan.domain())
            && self.plan.frame_pos == position.frame
            && self.plan.seek_mode == position.seek_mode
    }

    /// Advance each finite smoothing interval at its own rate, then normalize the next read.
    /// Work is bounded by the supported rate range, including a terminal constant interval.
    /// Failure does not partially update this checkpoint.
    pub(crate) fn project_output_frames(&mut self, output_frames: u64) -> bool {
        let mut projected = *self;
        let mut remaining = output_frames;
        for _ in 0..SourcePlayback::MAX_PROJECTION_SEGMENTS {
            if remaining == 0 {
                break;
            }
            let max_frames = usize::try_from(remaining).unwrap_or(usize::MAX);
            let (frames, ratio) = projected.playback.chunk(max_frames);
            if frames == 0 {
                return false;
            }
            if !projected.playback.advance(frames) {
                return false;
            }
            projected
                .plan
                .transition
                .advance_output_frames(frames, ratio);
            remaining -= frames as u64;
        }
        if remaining != 0 {
            return false;
        }
        projected.playback.chunk(DEFAULT_BLOCK_SAMPLES);
        let position = projected.playback.position();
        projected.plan.frame_pos = position.frame;
        projected.plan.seek_mode = position.seek_mode;
        *self = projected;
        true
    }
}

/// Frozen source/render checkpoint owned by one accepted action, without mutable DSP state.
pub(crate) struct PitchSourceSnapshot {
    pub(crate) sample: SampleBuffer,
    pub(crate) stems: Option<PreparedStemSet>,
    pub(crate) accepted: Option<super::constant_timing::AcceptedTimingProjection>,
    pub(crate) sample_rate_hz: u32,
    pub(crate) paused: bool,
    pub(crate) playback: SourcePlayback,
    pub(crate) plan: SourceReadPlan,
    /// None preserves the legacy identity between logical and actual input trajectories.
    pub(crate) actual_feed: Option<SourceReadCheckpoint>,
    pub(crate) source_feed_context: SourceFeedContext,
}

impl PitchSourceSnapshot {
    pub(crate) fn actual_feed_checkpoint(&self) -> SourceReadCheckpoint {
        self.actual_feed.unwrap_or(SourceReadCheckpoint {
            playback: self.playback,
            plan: self.plan,
        })
    }

    /// Resolve the actual captured reader slots against the retained physical pin.
    pub(crate) fn readers(&self) -> Option<StemReadSets<'_>> {
        self.source_feed_context
            .resolve_readers(self.plan, self.stems.as_ref())
    }

    /// Project a frozen SET checkpoint from capture F to its declared execution E.
    /// This changes only the snapshot. It has a fixed bound because accepted source-rate
    /// smoothing has finitely many .05 steps; the terminal constant interval advances at once.
    pub(crate) fn project_output_frames(&mut self, output_frames: u64) -> bool {
        if self.paused || output_frames == 0 {
            return true;
        }
        let mut logical = SourceReadCheckpoint {
            playback: self.playback,
            plan: self.plan,
        };
        let mut actual = self.actual_feed;
        if !logical.project_output_frames(output_frames)
            || actual
                .as_mut()
                .is_some_and(|actual| !actual.project_output_frames(output_frames))
        {
            return false;
        }
        self.playback = logical.playback;
        self.plan = logical.plan;
        self.actual_feed = actual;
        self.source_feed_context = self
            .source_feed_context
            .for_plan(self.actual_feed_checkpoint().plan);
        true
    }

    pub(crate) fn into_request(
        self,
        permit: NativeHistoryPermit,
        pitch_intent: PitchIntent,
        key_lock: bool,
        target_output_frame: u64,
        request_id: u64,
        epoch: u64,
    ) -> Option<NativeHistoryRequest> {
        if self.plan.transition.is_active() {
            return None;
        }
        self.into_history_request(
            permit,
            pitch_intent,
            key_lock,
            target_output_frame,
            request_id,
            epoch,
        )
    }

    /// Only genuine source-feed replay may own a still-active endpoint transition.
    pub(crate) fn into_history_request(
        self,
        permit: NativeHistoryPermit,
        pitch_intent: PitchIntent,
        key_lock: bool,
        target_output_frame: u64,
        request_id: u64,
        epoch: u64,
    ) -> Option<NativeHistoryRequest> {
        if !permit.current(&self.sample, self.sample_rate_hz)
            || !permit.matches_projection(self.accepted)
            || !self.actual_feed_checkpoint().is_consistent()
            || pitch_intent
                .native_pitch_scale(self.playback.tempo_ratio(), key_lock)
                .is_err()
        {
            return None;
        }
        let readers = self.readers()?;
        let coverage = if complete_view_available(&self.sample) {
            None
        } else {
            Some(NormalLoopCoverage::capture_with_readers(
                &self.sample,
                readers,
                self.sample_rate_hz,
                self.accepted,
                self.plan,
                &self.playback,
            )?)
        };
        Some(NativeHistoryRequest {
            pitch_intent,
            key_lock,
            binding: ProductiveSourceBinding::new(&self.sample, self.sample_rate_hz, self.accepted),
            sample: self.sample,
            stems: self.stems,
            permit,
            coverage,
            playback: self.playback,
            plan: self.plan,
            actual_feed: self.actual_feed,
            target_output_frame,
            request_id,
            epoch,
        })
    }
}

pub(crate) struct NativeHistoryRequest {
    pub(crate) pitch_intent: PitchIntent,
    pub(crate) key_lock: bool,
    pub(crate) sample: SampleBuffer,
    pub(crate) stems: Option<PreparedStemSet>,
    pub(crate) permit: NativeHistoryPermit,
    pub(crate) coverage: Option<NormalLoopCoverage>,
    pub(crate) binding: ProductiveSourceBinding,
    pub(crate) playback: SourcePlayback,
    pub(crate) plan: SourceReadPlan,
    /// Logical adoption remains authoritative; this names only the actual next Native input.
    pub(crate) actual_feed: Option<SourceReadCheckpoint>,
    pub(crate) target_output_frame: u64,
    pub(crate) request_id: u64,
    pub(crate) epoch: u64,
}

impl NativeHistoryRequest {
    fn effective_source_current(&self, sample: &SampleBuffer, rate: u32) -> bool {
        self.permit.current_effective_source(sample, rate)
    }

    pub(crate) fn actual_feed_checkpoint(&self) -> SourceReadCheckpoint {
        self.actual_feed.unwrap_or(SourceReadCheckpoint {
            playback: self.playback,
            plan: self.plan,
        })
    }

    /// Endpoint DSP data excludes the absolute output target and action envelope.
    /// Both real source owners remain current; callers must capture a fresh action permit.
    pub(crate) fn matches_endpoint(
        &self,
        snapshot: &PitchSourceSnapshot,
        render: super::pitch_actions::FrozenPitchRender,
    ) -> bool {
        let Some(readers) = snapshot.readers() else {
            return false;
        };
        let feed = ProductiveSourceFeed {
            sample: &snapshot.sample,
            stems: readers.current,
            outgoing_stems: readers.outgoing,
            sample_rate_hz: snapshot.sample_rate_hz,
            accepted: snapshot.accepted,
            plan: snapshot.plan,
            playback: &snapshot.playback,
            permit: None,
            output_frame: None,
            pitch_intent: render.pitch,
            key_lock: render.key_lock,
        };
        self.endpoint_data_matches(&feed)
            && self
                .actual_feed_checkpoint()
                .matches_exact(snapshot.actual_feed_checkpoint())
    }

    /// A silent physical-birth reserve has no performer launch coordinates yet.
    /// Compare a scalar copy in its phase namespace; the fresh action request and
    /// all general source/history/adoption comparisons remain unchanged.
    pub(crate) fn matches_reserved_endpoint(
        &self,
        snapshot: &PitchSourceSnapshot,
        render: super::pitch_actions::FrozenPitchRender,
    ) -> bool {
        if self.actual_feed.is_some() || snapshot.actual_feed.is_some() {
            return false;
        }
        let Some(readers) = snapshot.readers() else {
            return false;
        };
        let mut playback = snapshot.playback;
        playback.carry_launch_reference_from(&self.playback);
        let feed = ProductiveSourceFeed {
            sample: &snapshot.sample,
            stems: readers.current,
            outgoing_stems: readers.outgoing,
            sample_rate_hz: snapshot.sample_rate_hz,
            accepted: snapshot.accepted,
            plan: snapshot.plan,
            playback: &playback,
            permit: None,
            output_frame: None,
            pitch_intent: render.pitch,
            key_lock: render.key_lock,
        };
        self.endpoint_data_matches(&feed)
    }

    pub(crate) fn same_reserved_endpoint_request(&self, other: &Self) -> bool {
        if self.actual_feed.is_some() || other.actual_feed.is_some() {
            return false;
        }
        let mut playback = other.playback;
        playback.carry_launch_reference_from(&self.playback);
        let feed = ProductiveSourceFeed {
            sample: &other.sample,
            stems: other.stems.as_ref(),
            outgoing_stems: None,
            sample_rate_hz: other.binding.sample_rate_hz,
            accepted: other.binding.accepted,
            plan: other.plan,
            playback: &playback,
            permit: None,
            output_frame: None,
            pitch_intent: other.pitch_intent,
            key_lock: other.key_lock,
        };
        self.endpoint_data_matches(&feed)
            && other
                .permit
                .current(&other.sample, other.binding.sample_rate_hz)
            && self.permit.same_endpoint_authority(&other.permit)
    }

    fn endpoint_data_matches(&self, feed: &ProductiveSourceFeed<'_>) -> bool {
        self.pitch_intent == feed.pitch_intent
            && self.key_lock == feed.key_lock
            && self.coverage_matches(feed, true)
            && self.binding
                == ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted)
            && self
                .permit
                .endpoint_source_current(feed.sample, feed.sample_rate_hz)
            && self.permit.matches_projection(feed.accepted)
            && self.playback.matches_exact(feed.playback)
            && self.plan.matches_exact(feed.plan)
            && self.required_stems_match(feed, true)
    }

    fn required_stems_match(&self, feed: &ProductiveSourceFeed<'_>, exact_window: bool) -> bool {
        let original = StemReadSets::new(self.stems.as_ref(), None);
        let actual = StemReadSets::new(feed.stems, feed.outgoing_stems);
        std::iter::once(feed.plan.selection)
            .chain(feed.plan.transition.required_from_selection())
            .all(|selection| {
                let old = original.for_selection(selection);
                let next = actual.for_selection(selection);
                if selection.required_stem_hash().is_some() && (old.is_none() || next.is_none()) {
                    return false;
                }
                if exact_window {
                    same_stem_owners(old, next)
                } else {
                    match (old, next) {
                        (None, None) => true,
                        (Some(old), Some(next)) => {
                            Arc::ptr_eq(&old.complete_set_identity, &next.complete_set_identity)
                                && *old.complete_set_identity == *next.complete_set_identity
                                && Arc::ptr_eq(&old.publication.epoch, &next.publication.epoch)
                                && old.publication.expected == next.publication.expected
                                && old.accepted_timing == next.accepted_timing
                        }
                        _ => false,
                    }
                }
            })
    }

    fn coverage_matches(&self, feed: &ProductiveSourceFeed<'_>, exact_window: bool) -> bool {
        self.coverage_matches_transition(feed, exact_window, None, false)
    }

    fn coverage_matches_transition(
        &self,
        feed: &ProductiveSourceFeed<'_>,
        exact_window: bool,
        expected_transition: Option<StemTransition>,
        productive_continuity: bool,
    ) -> bool {
        let readers = feed
            .plan
            .required_read_sets(StemReadSets::new(feed.stems, feed.outgoing_stems));
        let required_available = |plan: SourceReadPlan, readers: StemReadSets<'_>| {
            std::iter::once(plan.selection)
                .chain(plan.transition.required_from_selection())
                .all(|selection| {
                    selection.required_stem_hash().is_none()
                        || readers.for_selection(selection).is_some()
                })
        };
        if !required_available(self.plan, StemReadSets::new(self.stems.as_ref(), None))
            || !required_available(feed.plan, readers)
        {
            return false;
        }
        if let Some(coverage) = &self.coverage {
            if exact_window {
                coverage.matches_with_readers(
                    feed.sample,
                    readers,
                    feed.sample_rate_hz,
                    feed.accepted,
                    feed.plan,
                    feed.playback,
                    true,
                )
            } else if productive_continuity {
                coverage.matches_productive_with_readers(feed, expected_transition)
            } else {
                coverage.matches_effective_with_readers(
                    feed.sample,
                    readers,
                    feed.sample_rate_hz,
                    feed.accepted,
                    feed.plan,
                    feed.playback,
                    expected_transition,
                )
            }
        } else {
            // A missing finite proof never becomes a successful native source request.
            // An already adopted complete request retains its original source/history pins
            // while a storage-only ACK may install any proved finite current context. Intro
            // and Tail return to Normal through the same copied clock, with no native reset.
            complete_view_available(&self.sample)
                && (complete_view_available(feed.sample)
                    || (!exact_window
                        && super::native_source_coverage::normal_loop_feed_available_with_readers(
                            feed.sample,
                            readers,
                            feed.sample_rate_hz,
                            feed.accepted,
                            feed.plan,
                            feed.playback,
                        )))
        }
    }
    pub(crate) fn matches_effective_contract(
        &self,
        feed: &ProductiveSourceFeed<'_>,
        _epoch: u64,
    ) -> bool {
        self.matches_effective_contract_with_transition(feed, _epoch, None, false, self.key_lock)
    }

    fn matches_effective_contract_with_transition(
        &self,
        feed: &ProductiveSourceFeed<'_>,
        _epoch: u64,
        expected_transition: Option<StemTransition>,
        productive_continuity: bool,
        effective_key_lock: bool,
    ) -> bool {
        if self.plan.transition.is_active()
            && expected_transition
                .is_none_or(|expected| !expected.matches_exact(feed.plan.transition))
        {
            return false;
        }
        let binding = ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted);
        // Once adopted, this frozen action's own permit owns its lifetime. A later lane request
        // epoch is not authority to stale the already accepted attack or its productive history.
        self.pitch_intent == feed.pitch_intent
            && effective_key_lock == feed.key_lock
            && self.coverage_matches_transition(
                feed,
                false,
                expected_transition,
                productive_continuity,
            )
            && self.binding.same_source(binding)
            && self.binding.accepted == binding.accepted
            && self.effective_source_current(feed.sample, feed.sample_rate_hz)
            && self.permit.matches_projection(feed.accepted)
            && self.plan.matches_source_contract(feed.plan)
            // Pending candidates still require their frozen exact target below. Once adopted,
            // the same real Native/FIFO owner may follow a new rate only while its next source
            // phase continues exactly; selection, source, permit and timing remain guarded.
            && (self.playback.matches_rate_target(feed.playback) || productive_continuity)
            && self.required_stems_match(feed, false)
    }

    /// A claimed live selection may replace reader roles, never the original source authority.
    fn matches_effective_stem_source(
        &self,
        feed: &ProductiveSourceFeed<'_>,
        effective_key_lock: bool,
    ) -> bool {
        let binding = ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted);
        self.pitch_intent == feed.pitch_intent
            && effective_key_lock == feed.key_lock
            && self.binding.same_source(binding)
            && self.binding.accepted == binding.accepted
            && self.effective_source_current(feed.sample, feed.sample_rate_hz)
            && self.permit.matches_projection(feed.accepted)
            && SourceReadPlan {
                selection: feed.plan.selection,
                ..self.plan
            }
            .matches_source_contract(feed.plan)
    }
    pub(crate) fn matches_contract(&self, feed: &ProductiveSourceFeed<'_>, epoch: u64) -> bool {
        self.epoch == epoch
            && self.pitch_intent == feed.pitch_intent
            && self.key_lock == feed.key_lock
            && self.coverage_matches(feed, true)
            && self.binding
                == ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted)
            && self.permit.current(feed.sample, feed.sample_rate_hz)
            && self.permit.matches_projection(feed.accepted)
            && self.plan.matches_source_contract(feed.plan)
            && self.playback.matches_rate_target(feed.playback)
            && self.required_stems_match(feed, true)
    }

    pub(crate) fn matches_adoption(
        &self,
        feed: &ProductiveSourceFeed<'_>,
        epoch: u64,
        request_id: u64,
    ) -> bool {
        self.request_id == request_id
            && self.matches_contract(feed, epoch)
            && feed.output_frame == Some(self.target_output_frame)
            && self.playback.matches_exact(feed.playback)
            && self.plan.matches_exact(feed.plan)
    }

    pub(crate) fn endpoint_stems(&self) -> Option<&PreparedStemSet> {
        StemReadSets::new(self.stems.as_ref(), None).for_selection(self.plan.selection)
    }
}

fn same_stem_owners(left: Option<&PreparedStemSet>, right: Option<&PreparedStemSet>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            Arc::ptr_eq(&left.complete_set_identity, &right.complete_set_identity)
                && *left.complete_set_identity == *right.complete_set_identity
                && Arc::ptr_eq(&left.publication.epoch, &right.publication.epoch)
                && left.publication.expected == right.publication.expected
                && left.source_version_hash == right.source_version_hash
                && left.sample_rate_hz == right.sample_rate_hz
                && left.channels == right.channels
                && left.frame_count == right.frame_count
                && left.available_mask == right.available_mask
                && left.accepted_timing == right.accepted_timing
                && Arc::ptr_eq(&left.reference_samples, &right.reference_samples)
                && left.stems.iter().zip(&right.stems).all(|(left, right)| {
                    left.channels == right.channels && Arc::ptr_eq(&left.samples, &right.samples)
                })
        }
        _ => false,
    }
}

/// Scalar evidence for one already accepted live read context. PCM and publication owners
/// remain in the mixer's current/outgoing or frozen reader slots and the original request.
#[derive(Clone, Copy, Debug)]
struct EffectiveStemReadContext {
    plan: SourceReadPlan,
    readers: SourceFeedContext,
}

impl EffectiveStemReadContext {
    fn matches(self, feed: &ProductiveSourceFeed<'_>, transition: Option<StemTransition>) -> bool {
        let readers = feed
            .plan
            .required_read_sets(StemReadSets::new(feed.stems, feed.outgoing_stems));
        self.plan.matches_source_contract(feed.plan)
            && transition.is_some_and(|expected| expected.matches_exact(feed.plan.transition))
            && self.readers.binding.accepted == feed.accepted
            && self
                .readers
                .for_plan(feed.plan)
                .same_source(SourceFeedContext::new(
                    ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted),
                    readers,
                ))
            && normal_loop_feed_available_with_readers(
                feed.sample,
                readers,
                feed.sample_rate_hz,
                feed.accepted,
                feed.plan,
                feed.playback,
            )
    }
}

pub(crate) struct NativeAdapterState {
    pub(crate) rubberband: Option<RubberBandLiveShifter>,
    pub(crate) block_size: usize,
    pub(crate) input: Vec<Vec<f32>>,
    pub(crate) output: Vec<Vec<f32>>,
    pub(crate) input_fifo: Vec<FixedFifo>,
    pub(crate) output_fifo: Vec<FixedFifo>,
    pub(crate) active: bool,
    pub(crate) pitch_scale: f64,
    pub(crate) dirty: bool,
    pub(crate) used: bool,
    pub(crate) source: Option<NativeHistoryRequest>,
    /// Effective scalar association after a claimed identical-pitch mode change.
    /// The originating request and its historical mode remain immutable.
    effective_mode: Option<bool>,
    /// Only a same-source live selection claim may install this scalar reader association.
    effective_stem_read_context: Option<EffectiveStemReadContext>,
    /// Only genuine active-E preparation seeds this. Some(inactive) fences later restarts.
    pub(crate) source_transition: Option<StemTransition>,
    pub(crate) primed_output_frames: usize,
    input_history_context: Option<NativeStartHistoryContext>,
    retained_start_timeline: Option<RetainedNativeStartTimeline>,
}

impl NativeAdapterState {
    pub(crate) fn new(rubberband: Option<RubberBandLiveShifter>, channels: usize) -> Self {
        let block_size = rubberband
            .as_ref()
            .map_or(DEFAULT_BLOCK_SAMPLES, RubberBandLiveShifter::block_size);
        Self {
            rubberband,
            block_size,
            input: (0..channels).map(|_| vec![0.0; block_size]).collect(),
            output: (0..channels).map(|_| vec![0.0; block_size]).collect(),
            input_fifo: (0..channels)
                .map(|_| FixedFifo::new(block_size + DEFAULT_BLOCK_SAMPLES))
                .collect(),
            output_fifo: (0..channels)
                .map(|_| FixedFifo::new(block_size * 2 + DEFAULT_BLOCK_SAMPLES))
                .collect(),
            active: false,
            pitch_scale: 1.0,
            dirty: false,
            used: false,
            source: None,
            effective_mode: None,
            effective_stem_read_context: None,
            source_transition: None,
            primed_output_frames: 0,
            input_history_context: None,
            retained_start_timeline: None,
        }
    }

    /// Clears only fixed adapter storage. Source/native owners remain pinned until worker recycling.
    pub(crate) fn reset_adapter(&mut self) {
        self.dirty |= self.used;
        self.input.iter_mut().for_each(|channel| channel.fill(0.0));
        self.output.iter_mut().for_each(|channel| channel.fill(0.0));
        self.input_fifo.iter_mut().for_each(FixedFifo::reset);
        self.output_fifo.iter_mut().for_each(FixedFifo::reset);
        self.active = false;
        self.pitch_scale = 1.0;
        self.primed_output_frames = 0;
        self.source_transition = None;
        self.effective_mode = None;
        self.effective_stem_read_context = None;
        self.input_history_context = None;
        self.retained_start_timeline = None;
    }

    pub(crate) fn has_physical_birth_context(&self) -> bool {
        self.input_history_context == Some(NativeStartHistoryContext::PhysicalBirth)
    }

    pub(super) fn record_input_history_context(&mut self, context: NativeStartHistoryContext) {
        debug_assert!(self.source.is_some());
        self.input_history_context = Some(context);
    }

    pub(crate) fn effective_key_lock(&self) -> Option<bool> {
        self.source
            .as_ref()
            .map(|source| self.effective_mode.unwrap_or(source.key_lock))
    }

    /// Called only inside the mixer's complete same-pitch/source claim.
    pub(crate) fn retain_effective_mode_claimed(&mut self, enabled: bool) {
        self.effective_mode = Some(enabled);
    }

    /// Called after the complete mixer selection claim and exact retained endpoint update.
    pub(crate) fn retain_stem_read_context_claimed(&mut self, feed: &ProductiveSourceFeed<'_>) {
        let readers = feed
            .plan
            .required_read_sets(StemReadSets::new(feed.stems, feed.outgoing_stems));
        self.effective_stem_read_context = Some(EffectiveStemReadContext {
            plan: feed.plan,
            readers: SourceFeedContext::new(
                ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted),
                readers,
            ),
        });
        // Keep the completed transition as an exact fence against an unclaimed restart.
        self.source_transition = Some(feed.plan.transition);
    }

    pub(crate) fn has_productive_continuity(
        &self,
        feed: &ProductiveSourceFeed<'_>,
        history: Option<super::productive_source_history::ProductiveSourceHistory>,
    ) -> bool {
        self.active
            && self.used
            && !self.dirty
            && history.is_some_and(|history| {
                history.continues(
                    ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted),
                    feed.playback.position(),
                    Some(feed.plan.domain()),
                )
            })
    }

    pub(crate) fn matches_effective_source(
        &self,
        feed: &ProductiveSourceFeed<'_>,
        epoch: u64,
        history: Option<super::productive_source_history::ProductiveSourceHistory>,
    ) -> bool {
        let productive_continuity = self.has_productive_continuity(feed, history);
        if self.effective_mode.is_some() && !productive_continuity {
            return false;
        }
        if let Some(context) = self.effective_stem_read_context {
            return productive_continuity
                && context.matches(feed, self.source_transition)
                && self.source.as_ref().is_some_and(|source| {
                    source.matches_effective_stem_source(
                        feed,
                        self.effective_mode.unwrap_or(source.key_lock),
                    )
                });
        }
        self.source.as_ref().is_some_and(|source| {
            if self.source_transition.is_some() || productive_continuity {
                source.matches_effective_contract_with_transition(
                    feed,
                    epoch,
                    self.source_transition,
                    productive_continuity,
                    self.effective_mode.unwrap_or(source.key_lock),
                )
            } else {
                source.matches_effective_contract(feed, epoch)
            }
        })
    }

    /// Native reset, initial pitch setup, source catch-up and old pin destruction are worker-only.
    #[cfg(test)]
    pub(crate) fn prepare_source(
        &mut self,
        request: NativeHistoryRequest,
    ) -> Result<(), RubberBandError> {
        self.prepare_source_with_silence(request, 0, PREPARED_HISTORY_FRAMES, false, None, None)
            .map_err(NativeSourcePreparationError::into_backend)
    }

    /// Catch up to the existing F+4096 checkpoint without changing its declared endpoint.
    /// Legacy pitch profiles retain their exact forward-history path. Expanded profiles need
    /// more real history and may rewind only an already constant captured source trajectory.
    /// This method is worker-only; no source/native preparation is added to rendering.
    pub(crate) fn prepare_live_catchup(
        &mut self,
        mut request: NativeHistoryRequest,
    ) -> Result<(), NativeSourcePreparationError> {
        let pitch = request
            .pitch_intent
            .native_pitch_scale(request.playback.tempo_ratio(), request.key_lock)
            .map_err(|_| {
                NativeSourcePreparationError::Unavailable(RubberBandError::InvalidPitchScale)
            })?;
        if (0.5..=2.0).contains(&pitch) {
            return self.prepare_source_with_silence(
                request,
                0,
                PREPARED_HISTORY_FRAMES,
                false,
                None,
                None,
            );
        }
        if request.playback.tempo_ratio().to_bits() != request.playback.rate_target().to_bits() {
            return Err(NativeSourcePreparationError::Unavailable(
                RubberBandError::UnavailableSourceHistory,
            ));
        }
        if !request.playback.advance(PREPARED_HISTORY_FRAMES) {
            return Err(NativeSourcePreparationError::Unavailable(
                RubberBandError::UnavailableSourceHistory,
            ));
        }
        request.playback.chunk(DEFAULT_BLOCK_SAMPLES);
        let position = request.playback.position();
        request.plan.frame_pos = position.frame;
        request.plan.seek_mode = position.seek_mode;
        self.prepare_source_at_endpoint_checked(request)
    }

    /// Prime actual past source history while keeping an intentional launch endpoint unchanged.
    /// This is worker-only. Pending rate smoothing remains an explicit unavailable trajectory,
    /// rather than being replaced with a fabricated constant-rate prediction.
    pub(crate) fn prepare_source_at_endpoint(
        &mut self,
        request: NativeHistoryRequest,
    ) -> Result<(), RubberBandError> {
        self.prepare_source_at_endpoint_checked(request)
            .map_err(NativeSourcePreparationError::into_backend)
    }

    /// Warm one fresh physical source with silence before its selected origin.
    /// Logical progress stays at that exact endpoint; all work remains off realtime.
    pub(crate) fn prepare_source_at_physical_birth(
        &mut self,
        request: NativeHistoryRequest,
    ) -> Result<(), RubberBandError> {
        self.prepare_retained_source_start_context_checked(
            request,
            None,
            0,
            None,
            NativeStartHistoryContext::PhysicalBirth,
        )
        .map_err(NativeSourcePreparationError::into_backend)
    }

    pub(crate) fn prepare_source_at_endpoint_with_launch_catchup_and_dsp(
        &mut self,
        request: NativeHistoryRequest,
        catchup_frames: u64,
        dsp: Option<&mut super::late_start_dsp::LateStartDspWork>,
    ) -> Result<(), RubberBandError> {
        self.prepare_retained_source_start_with_catchup_and_dsp_checked(
            request,
            None,
            catchup_frames,
            dsp,
        )
        .map_err(NativeSourcePreparationError::into_backend)
    }

    fn prepare_source_at_endpoint_checked(
        &mut self,
        request: NativeHistoryRequest,
    ) -> Result<(), NativeSourcePreparationError> {
        self.prepare_retained_source_start_checked(request, None)
    }

    fn prepare_source_with_silence(
        &mut self,
        mut request: NativeHistoryRequest,
        mut silent_prefix: usize,
        history_frames: usize,
        native_prepared: bool,
        source_rebase: Option<(usize, SourceReadCheckpoint)>,
        mut dsp: Option<(usize, &mut super::late_start_dsp::LateStartDspWork)>,
    ) -> Result<(), NativeSourcePreparationError> {
        if !request
            .actual_feed_checkpoint()
            .matches_exact(SourceReadCheckpoint {
                playback: request.playback,
                plan: request.plan,
            })
        {
            // This legacy forward path has no actual-history replay proof. A differing
            // input trajectory must use genuine replay, never read the logical substitute.
            return Err(NativeSourcePreparationError::Unavailable(
                RubberBandError::UnavailableSourceHistory,
            ));
        }
        request.actual_feed = None;
        if !complete_view_available(&request.sample)
            && !request.coverage.as_ref().is_some_and(|coverage| {
                coverage.matches(
                    &request.sample,
                    request.endpoint_stems(),
                    request.binding.sample_rate_hz,
                    request.binding.accepted,
                    request.plan,
                    &request.playback,
                    true,
                )
            })
        {
            return Err(NativeSourcePreparationError::Unavailable(
                RubberBandError::InvalidBlockSize,
            ));
        }
        self.source = None;
        self.reset_adapter();
        let pitch = request
            .pitch_intent
            .native_pitch_scale(request.playback.tempo_ratio(), request.key_lock)
            .map_err(|_| {
                NativeSourcePreparationError::Unavailable(RubberBandError::InvalidPitchScale)
            })?;
        if !native_prepared {
            self.rubberband
                .as_mut()
                .expect("prepared adapter owns native state")
                .prepare_exact_pitch(pitch)
                .map_err(NativeSourcePreparationError::Backend)?;
        }
        if !native_prepared
            && self
                .rubberband
                .as_ref()
                .expect("prepared adapter owns native state")
                .source_history_frames(pitch)
                .map_err(NativeSourcePreparationError::from_history)?
                > history_frames
        {
            // ForwardHistory owns exactly its declared 4096-frame checkpoint interval.
            // Larger-pitch readiness cannot silently move that endpoint or claim warmed output.
            return Err(NativeSourcePreparationError::Unavailable(
                RubberBandError::UnavailableSourceHistory,
            ));
        }
        self.pitch_scale = pitch;
        self.dirty = false;
        self.used = false;
        let channels = self.input.len();
        let mut feed = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect::<Vec<_>>();
        let mut output = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect::<Vec<_>>();
        let mut remaining = history_frames;
        #[cfg(test)]
        super::source_reader::reset_tap_observation_for_test();
        while remaining > 0 {
            let consumed = history_frames - remaining;
            if let Some((boundary, checkpoint)) = source_rebase
                && consumed == boundary
            {
                if silent_prefix != 0 {
                    return Err(NativeSourcePreparationError::Unavailable(
                        RubberBandError::UnavailableSourceHistory,
                    ));
                }
                request.playback = checkpoint.playback;
                request.plan = checkpoint.plan;
            }
            let until_rebase = source_rebase.map_or(remaining, |(boundary, _)| {
                if boundary > consumed {
                    boundary - consumed
                } else {
                    remaining
                }
            });
            let (frames, ratio) = request
                .playback
                .chunk(remaining.min(DEFAULT_BLOCK_SAMPLES).min(until_rebase));
            if frames == 0 {
                return Err(NativeSourcePreparationError::Unavailable(
                    RubberBandError::UnavailableSourceHistory,
                ));
            }
            let mut next_playback = request.playback;
            if silent_prefix == 0 && !next_playback.advance(frames) {
                return Err(NativeSourcePreparationError::Unavailable(
                    RubberBandError::UnavailableSourceHistory,
                ));
            }
            let pitch = request
                .pitch_intent
                .native_pitch_scale(ratio, request.key_lock)
                .map_err(|_| {
                    NativeSourcePreparationError::Unavailable(RubberBandError::InvalidPitchScale)
                })?;
            if !native_prepared
                && self
                    .rubberband
                    .as_ref()
                    .expect("prepared adapter owns native state")
                    .source_history_frames(pitch)
                    .map_err(NativeSourcePreparationError::from_history)?
                    > history_frames
            {
                // The actual forward trajectory may enter an expanded pitch profile after
                // its initial checkpoint. Its fixed interval must not claim unavailable history.
                return Err(NativeSourcePreparationError::Unavailable(
                    RubberBandError::UnavailableSourceHistory,
                ));
            }
            if silent_prefix != 0 {
                let frames = frames.min(silent_prefix);
                feed.iter_mut()
                    .for_each(|channel| channel[..frames].fill(0.0));
                self.render(&feed, &mut output, frames, pitch)
                    .map_err(NativeSourcePreparationError::Backend)?;
                silent_prefix -= frames;
                remaining -= frames;
                continue;
            }
            // Preparation retains the same Native stream even at unity.
            self.render_source_history_chunk(&request, &mut feed, &mut output, frames, pitch)?;
            if let Some((start, dsp)) = &mut dsp
                && consumed >= *start
            {
                dsp.consume(&request, &output, frames)
                    .map_err(NativeSourcePreparationError::from_history)?;
            }
            request.playback = next_playback;
            request.plan.transition.advance_output_frames(frames, ratio);
            let position = request.playback.position();
            request.plan.frame_pos = position.frame;
            request.plan.seek_mode = position.seek_mode;
            remaining -= frames;
        }
        request.playback.chunk(DEFAULT_BLOCK_SAMPLES);
        #[cfg(test)]
        if let Some(coverage) = &mut request.coverage {
            coverage.prepared_taps = super::source_reader::tap_observation_for_test();
        }
        self.source = Some(request);
        self.primed_output_frames = history_frames;
        self.input_history_context = Some(NativeStartHistoryContext::SourceHistory);
        Ok(())
    }

    /// Existing source reader and adapter processing shared by cold priming and
    /// continuation of the same original launch. Worker-only; never resets Native.
    pub(super) fn render_source_history_chunk(
        &mut self,
        request: &NativeHistoryRequest,
        feed: &mut [Vec<f32>],
        output: &mut [Vec<f32>],
        frames: usize,
        pitch: f64,
    ) -> Result<(), NativeSourcePreparationError> {
        if !request.playback.can_read_frames(frames) {
            return Err(NativeSourcePreparationError::Unavailable(
                RubberBandError::UnavailableSourceHistory,
            ));
        }
        if request.coverage.is_some() {
            if !normal_loop_feed_available(
                &request.sample,
                request.stems.as_ref(),
                request.binding.sample_rate_hz,
                request.binding.accepted,
                request.plan,
                &request.playback,
            ) {
                return Err(NativeSourcePreparationError::Unavailable(
                    RubberBandError::InvalidBlockSize,
                ));
            }
            if feed.len() < request.plan.channels {
                return Err(NativeSourcePreparationError::Backend(
                    RubberBandError::InvalidBlockSize,
                ));
            }
            request
                .plan
                .fill_fractional_buffers_checked(
                    &request.sample,
                    request.stems.as_ref(),
                    &request.playback,
                    feed,
                    frames,
                )
                .map_err(NativeSourcePreparationError::from_coverage)?;
        } else if complete_view_available(&request.sample) {
            request
                .plan
                .fill_fractional_buffers(
                    &request.sample,
                    request.stems.as_ref(),
                    &request.playback,
                    feed,
                    frames,
                )
                .map_err(NativeSourcePreparationError::from_coverage)?;
        } else {
            return Err(NativeSourcePreparationError::Unavailable(
                RubberBandError::InvalidBlockSize,
            ));
        }
        self.render(feed, output, frames, pitch)
            .map_err(NativeSourcePreparationError::Backend)
    }

    /// One common adapter implementation for worker catch-up and bounded callback rendering.
    pub(crate) fn render(
        &mut self,
        feed: &[Vec<f32>],
        output: &mut [Vec<f32>],
        frames: usize,
        pitch: f64,
    ) -> Result<(), RubberBandError> {
        if !self.active {
            for fifo in &mut self.output_fifo {
                fifo.push_silence(self.block_size.saturating_sub(1));
            }
            self.active = true;
        }
        if (pitch - self.pitch_scale).abs() > PITCH_SCALE_EPSILON {
            self.rubberband
                .as_mut()
                .expect("active adapter owns native state")
                .set_pitch_scale(pitch)?;
            self.pitch_scale = pitch;
        }
        for (fifo, channel) in self.input_fifo.iter_mut().zip(feed) {
            if fifo.push_slice(&channel[..frames]) != frames {
                return Err(RubberBandError::InvalidBlockSize);
            }
        }
        let max_blocks = (DEFAULT_BLOCK_SAMPLES / self.block_size).saturating_add(2);
        for _ in 0..max_blocks {
            if !self
                .input_fifo
                .iter()
                .all(|fifo| fifo.len() >= self.block_size)
            {
                break;
            }
            for (fifo, channel) in self.input_fifo.iter_mut().zip(&mut self.input) {
                if fifo.pop_into(&mut channel[..self.block_size]) != self.block_size {
                    return Err(RubberBandError::InvalidBlockSize);
                }
            }
            self.rubberband
                .as_mut()
                .expect("active adapter owns native state")
                .shift(&self.input, &mut self.output)?;
            self.used = true;
            for (fifo, channel) in self.output_fifo.iter_mut().zip(&self.output) {
                if fifo.push_slice(&channel[..self.block_size]) != self.block_size {
                    return Err(RubberBandError::InvalidBlockSize);
                }
            }
        }
        for (fifo, channel) in self.output_fifo.iter_mut().zip(output) {
            let read = fifo.pop_into(&mut channel[..frames]);
            channel[read..frames].fill(0.0);
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
    use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
    use crate::audio_engine::native_history_permit::NativeHistoryContext;
    use crate::audio_engine::prepared_source::PreparedSourcePermit;
    use crate::audio_engine::source_reader::{
        ExplicitSeekMode, FrameRange, SourceLoopDomain, StemRenderSelection, StemTransition,
        full_stem_available_mask,
    };
    use crate::messages::StemMixMode;
    use std::sync::atomic::AtomicU64;

    const SOURCE_FRAMES: usize = 24_001;
    const REGION: FrameRange = FrameRange {
        start: 100,
        end: 23_999,
    };

    pub(crate) fn fixture(
        channels: usize,
        rate: u32,
        ratio: f64,
        mode: ExplicitSeekMode,
        stem_mask: Option<u8>,
    ) -> NativeHistoryRequest {
        let pcm = (0..SOURCE_FRAMES)
            .flat_map(|frame| {
                let base = (frame as f64 * 0.057).sin() as f32 * 0.3
                    + (frame as f64 * 0.0037).cos() as f32 * 0.1;
                (0..channels).map(move |channel| if channel == 0 { base } else { base * -0.73 })
            })
            .collect::<Vec<_>>();
        let sample = SampleBuffer {
            residency: None,
            channels,
            samples: Arc::from(pcm),
        };
        let stems = stem_mask.map(|_| PreparedStemSet {
            complete_set_identity: std::sync::Arc::new([0; 32]),
            accepted_timing: None,
            reference_samples: sample.samples.clone(),
            reference_residency: sample.residency.clone(),
            publication: PreparedSourcePermit::unrestricted(),
            source_version_hash: 17,
            sample_rate_hz: rate,
            channels,
            frame_count: SOURCE_FRAMES,
            available_mask: full_stem_available_mask(),
            stems: [0.1, 0.2, 0.3, 0.4].map(|gain| SampleBuffer {
                residency: None,
                channels,
                samples: sample
                    .samples
                    .iter()
                    .map(|value| *value * gain)
                    .collect::<Vec<_>>()
                    .into(),
            }),
        });
        let frame = match mode {
            ExplicitSeekMode::Normal => 171,
            ExplicitSeekMode::BeforeLoop => 23,
            ExplicitSeekMode::AfterLoop => 24_000,
        };
        let mut playback = SourcePlayback::new(frame, mode, ratio);
        assert!(playback.configure(SOURCE_FRAMES, REGION));
        assert!(playback.advance(1));
        let position = playback.position();
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        ownership.publish_test_source(0, &sample, rate, 13);
        let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
        let epoch = Arc::new(AtomicU64::new(7));
        let permit = NativeHistoryContext {
            id: 0,
            ownership: &ownership,
            acknowledgements: &acknowledgements,
            preparation_epoch: &epoch,
        }
        .capture(&sample, rate, None)
        .unwrap();
        let plan = SourceReadPlan {
            channels,
            sample_frames: SOURCE_FRAMES,
            frame_pos: position.frame,
            loop_region: REGION,
            loop_period: crate::audio_engine::source_reader::SourceReaderPeriod::Physical,
            seek_mode: position.seek_mode,
            selection: if let Some(mask) = stem_mask {
                StemRenderSelection::from_state(StemMixMode::AllStems, 17, mask)
            } else {
                StemRenderSelection::full_mix()
            },
            transition: StemTransition::default(),
        };
        NativeHistoryRequest {
            pitch_intent: PitchIntent::default(),
            key_lock: true,
            binding: ProductiveSourceBinding::new(&sample, rate, None),
            sample,
            stems,
            permit,
            coverage: None,
            playback,
            plan,
            actual_feed: None,
            target_output_frame: PREPARED_HISTORY_FRAMES as u64,
            request_id: 1,
            epoch: 3,
        }
    }

    // Independent algebraic source addressing/interpolation; this oracle does not invoke the
    // production source reader, cursor advancement or adapter FIFO implementation.
    fn oracle_feed(
        request: &NativeHistoryRequest,
        start: usize,
        frames: usize,
        initial_mode: ExplicitSeekMode,
    ) -> Vec<Vec<f32>> {
        let origin_frame = match initial_mode {
            ExplicitSeekMode::Normal => 171,
            ExplicitSeekMode::BeforeLoop => 23,
            ExplicitSeekMode::AfterLoop => 24_000,
        };
        let ratio = request.playback.tempo_ratio();
        let address = |offset: usize| {
            let absolute = origin_frame + offset;
            match initial_mode {
                ExplicitSeekMode::Normal => {
                    REGION.start + (origin_frame - REGION.start + offset) % REGION.len()
                }
                ExplicitSeekMode::BeforeLoop if absolute < REGION.start => absolute,
                ExplicitSeekMode::BeforeLoop => {
                    REGION.start + (absolute - REGION.start) % REGION.len()
                }
                ExplicitSeekMode::AfterLoop if absolute < SOURCE_FRAMES => absolute,
                ExplicitSeekMode::AfterLoop => {
                    REGION.start + (absolute - SOURCE_FRAMES) % REGION.len()
                }
            }
        };
        let read = |frame: usize, channel: usize| {
            if let Some(stems) = &request.stems {
                stems
                    .stems
                    .iter()
                    .enumerate()
                    .take(4)
                    .filter(|(index, _)| {
                        request.plan.selection
                            == StemRenderSelection::from_state(StemMixMode::AllStems, 17, 0b1111)
                            || [1, 3].contains(index)
                    })
                    .map(|(_, stem)| stem.samples[frame * request.sample.channels + channel])
                    .sum()
            } else {
                request.sample.samples[frame * request.sample.channels + channel]
            }
        };
        (0..request.sample.channels)
            .map(|channel| {
                (start..start + frames)
                    .map(|frame| {
                        let distance = (frame + 1) as f64 * ratio;
                        let offset = distance.floor() as usize;
                        let left = read(address(offset), channel);
                        let right = read(address(offset + 1), channel);
                        left + (right - left) * distance.fract() as f32
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn real_prepared_native_fifo_suffix_matches_independent_raw_source_native_oracle() {
        let patterns = [&[512][..], &[1, 127, 384, 96, 257, 512, 31][..]];
        let continuation = 20_017;
        let mut cases = 0;
        for rate in [44_100, 48_000, 96_000] {
            for ratio in [0.73, 1.371_234_567_890_123, 2.0] {
                for mode in [
                    ExplicitSeekMode::Normal,
                    ExplicitSeekMode::BeforeLoop,
                    ExplicitSeekMode::AfterLoop,
                ] {
                    for (channels, stem_mask) in [1, 2].into_iter().flat_map(|channels| {
                        [None, Some(0b1010), Some(0b1111)].map(|mask| (channels, mask))
                    }) {
                        let reference_request = fixture(channels, rate, ratio, mode, stem_mask);
                        let mut raw = RubberBandLiveShifter::new(rate, channels).unwrap();
                        raw.prepare_exact_pitch(pitch_scale_for_tempo_ratio(ratio))
                            .unwrap();
                        let block = raw.block_size();
                        let total =
                            (PREPARED_HISTORY_FRAMES + continuation).div_ceil(block) * block;
                        let mut expected = (0..channels)
                            .map(|_| vec![0.0; block - 1])
                            .collect::<Vec<_>>();
                        let mut raw_output =
                            (0..channels).map(|_| vec![0.0; block]).collect::<Vec<_>>();
                        for start in (0..total).step_by(block) {
                            raw.shift(
                                &oracle_feed(&reference_request, start, block, mode),
                                &mut raw_output,
                            )
                            .unwrap();
                            for channel in 0..channels {
                                expected[channel].extend_from_slice(&raw_output[channel]);
                            }
                        }
                        for channel in &expected {
                            assert!(
                                channel[PREPARED_HISTORY_FRAMES
                                    ..PREPARED_HISTORY_FRAMES + continuation]
                                    .iter()
                                    .any(|sample| sample.abs() > 0.02),
                                "oracle suffix must contain genuine shifted audio at rate={rate} ratio={ratio}"
                            );
                        }
                        for pattern in patterns {
                            let request = fixture(channels, rate, ratio, mode, stem_mask);
                            let mut state = NativeAdapterState::new(
                                Some(RubberBandLiveShifter::new(rate, channels).unwrap()),
                                channels,
                            );
                            let address = state.rubberband.as_ref().unwrap().state_address();
                            state.prepare_source(request).unwrap();
                            assert_eq!(state.rubberband.as_ref().unwrap().state_address(), address);
                            assert!(state.active && state.used && !state.dirty);
                            assert_eq!(
                                state.input_fifo[0].len() + state.output_fifo[0].len(),
                                block - 1
                            );
                            assert_eq!(state.source.as_ref().unwrap().request_id, 1);
                            let mut playback = state.source.as_ref().unwrap().playback;
                            let mut plan = state.source.as_ref().unwrap().plan;
                            let mut feed = (0..channels)
                                .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
                                .collect::<Vec<_>>();
                            let mut output = (0..channels)
                                .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
                                .collect::<Vec<_>>();
                            let mut elapsed = 0;
                            let mut step = 0;
                            while elapsed < continuation {
                                let frames =
                                    pattern[step % pattern.len()].min(continuation - elapsed);
                                let source = state.source.as_ref().unwrap();
                                plan.fill_fractional_buffers(
                                    &source.sample,
                                    source.stems.as_ref(),
                                    &playback,
                                    &mut feed,
                                    frames,
                                )
                                .unwrap();
                                state
                                    .render(
                                        &feed,
                                        &mut output,
                                        frames,
                                        pitch_scale_for_tempo_ratio(ratio),
                                    )
                                    .unwrap();
                                for channel in 0..channels {
                                    assert_eq!(
                                        &output[channel][..frames],
                                        &expected[channel][PREPARED_HISTORY_FRAMES + elapsed
                                            ..PREPARED_HISTORY_FRAMES + elapsed + frames],
                                        "rate={rate} ratio={ratio} mode={mode:?} channels={channels} stems={stem_mask:?} pattern={pattern:?}"
                                    );
                                }
                                assert!(playback.advance(frames));
                                let position = playback.position();
                                plan.frame_pos = position.frame;
                                plan.seek_mode = position.seek_mode;
                                elapsed += frames;
                                step += 1;
                                assert_eq!(
                                    state.input_fifo[0].len() + state.output_fifo[0].len(),
                                    block - 1
                                );
                            }
                            cases += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 324);
    }

    #[test]
    fn prepared_owner_pins_real_source_until_worker_style_retirement() {
        let request = fixture(2, 48_000, 1.37, ExplicitSeekMode::Normal, Some(0b1010));
        let weak = Arc::downgrade(&request.sample.samples);
        let mut state =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(48_000, 2).unwrap()), 2);
        state.prepare_source(request).unwrap();
        assert!(weak.upgrade().is_some());
        state.reset_adapter();
        assert!(
            weak.upgrade().is_some(),
            "adapter reset must retain source/native owner for worker retirement"
        );
        drop(state.source.take());
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn actual_adoption_requires_exact_outstanding_request_identity() {
        let request = fixture(1, 48_000, 1.37, ExplicitSeekMode::Normal, None);
        let mut state =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(48_000, 1).unwrap()), 1);
        state.prepare_source(request).unwrap();
        let source = state.source.as_ref().unwrap();
        let feed = ProductiveSourceFeed {
            pitch_intent: super::super::pitch_intent::PitchIntent::default(),
            key_lock: true,
            sample: &source.sample,
            outgoing_stems: None,
            stems: source.stems.as_ref(),
            sample_rate_hz: 48_000,
            accepted: None,
            plan: source.plan,
            playback: &source.playback,
            permit: Some(&source.permit),
            output_frame: Some(source.target_output_frame),
        };
        assert!(source.matches_adoption(&feed, 3, 1));
        assert!(!source.matches_adoption(&feed, 3, 2));
    }

    #[test]
    fn smoothed_preparation_and_continuation_match_piecewise_algebraic_raw_native() {
        let continuation = 20_017;
        let mut cases = 0;
        for rate in [44_100, 48_000, 96_000] {
            for channels in [1, 2] {
                for mode in [
                    ExplicitSeekMode::Normal,
                    ExplicitSeekMode::BeforeLoop,
                    ExplicitSeekMode::AfterLoop,
                ] {
                    for stem_mask in [None, Some(0b1010), Some(0b1111)] {
                        let mut request = fixture(channels, rate, 0.73, mode, stem_mask);
                        assert!(request.playback.set_target(1.37));
                        request.playback.chunk(DEFAULT_BLOCK_SAMPLES);
                        let mut raw = RubberBandLiveShifter::new(rate, channels).unwrap();
                        let block = raw.block_size();
                        assert_eq!(block, 512);
                        let mut ratio = 0.73 + 0.05;
                        raw.prepare_exact_pitch(pitch_scale_for_tempo_ratio(ratio))
                            .unwrap();
                        let mut frame = match mode {
                            ExplicitSeekMode::Normal => 171,
                            ExplicitSeekMode::BeforeLoop => 23,
                            ExplicitSeekMode::AfterLoop => 24_000,
                        };
                        let mut fraction = 0.73;
                        let mut seek_mode = mode;
                        let mut stable_elapsed = 0_usize;
                        let total =
                            (PREPARED_HISTORY_FRAMES + continuation).div_ceil(block) * block;
                        let mut expected = (0..channels)
                            .map(|_| vec![0.0; block - 1])
                            .collect::<Vec<_>>();
                        let mut feed = (0..channels).map(|_| vec![0.0; block]).collect::<Vec<_>>();
                        let mut shifted =
                            (0..channels).map(|_| vec![0.0; block]).collect::<Vec<_>>();
                        for _ in (0..total).step_by(block) {
                            let address = |offset: usize| {
                                let absolute = frame + offset;
                                match seek_mode {
                                    ExplicitSeekMode::Normal => {
                                        REGION.start
                                            + (frame - REGION.start + offset) % REGION.len()
                                    }
                                    ExplicitSeekMode::BeforeLoop if absolute < REGION.start => {
                                        absolute
                                    }
                                    ExplicitSeekMode::BeforeLoop => {
                                        REGION.start + (absolute - REGION.start) % REGION.len()
                                    }
                                    ExplicitSeekMode::AfterLoop if absolute < SOURCE_FRAMES => {
                                        absolute
                                    }
                                    ExplicitSeekMode::AfterLoop => {
                                        REGION.start + (absolute - SOURCE_FRAMES) % REGION.len()
                                    }
                                }
                            };
                            let read = |frame: usize, channel: usize| {
                                if let (Some(stems), Some(mask)) = (&request.stems, stem_mask) {
                                    stems
                                        .stems
                                        .iter()
                                        .enumerate()
                                        .take(4)
                                        .filter(|(index, _)| mask & (1 << index) != 0)
                                        .map(|(_, stem)| stem.samples[frame * channels + channel])
                                        .sum::<f32>()
                                } else {
                                    request.sample.samples[frame * channels + channel]
                                }
                            };
                            for (channel, output) in feed.iter_mut().enumerate() {
                                for (offset, value) in output.iter_mut().enumerate() {
                                    let distance: f64 =
                                        fraction + (stable_elapsed + offset) as f64 * ratio;
                                    let whole = distance.floor() as usize;
                                    let left = read(address(whole), channel);
                                    *value = left
                                        + (read(address(whole + 1), channel) - left)
                                            * distance.fract() as f32;
                                }
                            }
                            raw.set_pitch_scale(pitch_scale_for_tempo_ratio(ratio))
                                .unwrap();
                            raw.shift(&feed, &mut shifted).unwrap();
                            for channel in 0..channels {
                                expected[channel].extend_from_slice(&shifted[channel]);
                            }
                            let distance = fraction + (stable_elapsed + block) as f64 * ratio;
                            let whole = distance.floor() as usize;
                            let next_frame = address(whole);
                            seek_mode = match seek_mode {
                                ExplicitSeekMode::BeforeLoop if frame + whole >= REGION.start => {
                                    ExplicitSeekMode::Normal
                                }
                                ExplicitSeekMode::AfterLoop if frame + whole >= SOURCE_FRAMES => {
                                    ExplicitSeekMode::Normal
                                }
                                mode => mode,
                            };
                            if ratio == 1.37 {
                                stable_elapsed += block;
                            } else {
                                frame = next_frame;
                                fraction = distance.fract();
                            }
                            ratio = if (1.37_f64 - ratio).abs() <= 0.05 {
                                1.37
                            } else {
                                ratio + 0.05
                            };
                        }
                        assert!(
                            expected[0]
                                [PREPARED_HISTORY_FRAMES..PREPARED_HISTORY_FRAMES + continuation]
                                .iter()
                                .any(|sample| sample.abs() > 0.02)
                        );
                        let mut state = NativeAdapterState::new(
                            Some(RubberBandLiveShifter::new(rate, channels).unwrap()),
                            channels,
                        );
                        state.prepare_source(request).unwrap();
                        let mut playback = state.source.as_ref().unwrap().playback;
                        let mut plan = state.source.as_ref().unwrap().plan;
                        let mut feed = (0..channels)
                            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
                            .collect::<Vec<_>>();
                        let mut output = (0..channels)
                            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
                            .collect::<Vec<_>>();
                        let mut elapsed = 0;
                        for requested in [1, 127, 384, 96, 257, 512, 31].into_iter().cycle() {
                            if elapsed == continuation {
                                break;
                            }
                            let (frames, ratio) =
                                playback.chunk(requested.min(continuation - elapsed));
                            let source = state.source.as_ref().unwrap();
                            plan.fill_fractional_buffers(
                                &source.sample,
                                source.stems.as_ref(),
                                &playback,
                                &mut feed,
                                frames,
                            )
                            .unwrap();
                            state
                                .render(
                                    &feed,
                                    &mut output,
                                    frames,
                                    pitch_scale_for_tempo_ratio(ratio),
                                )
                                .unwrap();
                            for channel in 0..channels {
                                assert_eq!(
                                    &output[channel][..frames],
                                    &expected[channel][PREPARED_HISTORY_FRAMES + elapsed
                                        ..PREPARED_HISTORY_FRAMES + elapsed + frames],
                                    "smoothed raw-native mismatch rate={rate} channels={channels} mode={mode:?} stems={stem_mask:?} n={elapsed}"
                                );
                            }
                            assert!(playback.advance(frames));
                            let position = playback.position();
                            plan.frame_pos = position.frame;
                            plan.seek_mode = position.seek_mode;
                            elapsed += frames;
                        }
                        cases += 1;
                    }
                }
            }
        }
        assert_eq!(cases, 54);
    }

    const MUSICAL_REGION: FrameRange = FrameRange {
        start: 13,
        end: 140,
    };

    fn musical_fixture(
        rate: u32,
        ratio: f64,
        period: f64,
        mode: ExplicitSeekMode,
        mask: Option<u8>,
    ) -> NativeHistoryRequest {
        let mut request = fixture(2, rate, ratio, mode, mask);
        let origin = match mode {
            ExplicitSeekMode::Normal => MUSICAL_REGION.start,
            ExplicitSeekMode::BeforeLoop => 7,
            ExplicitSeekMode::AfterLoop => SOURCE_FRAMES - 5,
        };
        let mut playback = SourcePlayback::new(origin, mode, ratio);
        assert!(playback.configure_domain(
            SourceLoopDomain::musical(SOURCE_FRAMES, MUSICAL_REGION, period).unwrap(),
        ));
        request.playback = playback;
        request.plan.frame_pos = origin;
        request.plan.seek_mode = mode;
        request.plan.loop_region = MUSICAL_REGION;
        request.plan.loop_period =
            crate::audio_engine::source_reader::SourceReaderPeriod::from_scalar_period(Some(
                period,
            ));
        request
    }

    /// Independent continuous musical phase and physical-knot oracle. Neither production
    /// addressing nor a SourceReadPlan/SourcePlayback supplies the reference feed. Intro/tail
    /// distances are source frames; native blocks and the adapter prefix are output frames.
    fn musical_oracle_feed(
        request: &NativeHistoryRequest,
        initial_mode: ExplicitSeekMode,
        output_start: usize,
        output_frames: usize,
    ) -> Vec<Vec<f32>> {
        let period = request.plan.loop_period.scalar_period().unwrap();
        let ratio = request.playback.tempo_ratio();
        let read = |frame: usize, channel: usize| {
            if let Some(stems) = &request.stems {
                [1, 3]
                    .into_iter()
                    .map(|index| stems.stems[index].samples[frame * 2 + channel])
                    .sum::<f32>()
            } else {
                request.sample.samples[frame * 2 + channel]
            }
        };
        (0..2)
            .map(|channel| {
                (output_start..output_start + output_frames)
                    .map(|output_frame| {
                        let distance = output_frame as f64 * ratio;
                        let before = match initial_mode {
                            ExplicitSeekMode::Normal => 0.0,
                            ExplicitSeekMode::BeforeLoop => 6.0,
                            ExplicitSeekMode::AfterLoop => 5.0,
                        };
                        let (left_frame, right_frame, fraction) = if distance < before {
                            let origin = match initial_mode {
                                ExplicitSeekMode::BeforeLoop => 7,
                                ExplicitSeekMode::AfterLoop => SOURCE_FRAMES - 5,
                                ExplicitSeekMode::Normal => unreachable!(),
                            };
                            let left_frame = origin + distance.floor() as usize;
                            let right_frame = if left_frame + 1 == SOURCE_FRAMES {
                                MUSICAL_REGION.start
                            } else {
                                left_frame + 1
                            };
                            (left_frame, right_frame, distance.fract())
                        } else {
                            let phase = (distance - before).rem_euclid(period);
                            let last_knot = (period.ceil() as usize - 1)
                                .min(MUSICAL_REGION.end - MUSICAL_REGION.start - 1);
                            if phase >= last_knot as f64 {
                                (
                                    MUSICAL_REGION.start + last_knot,
                                    MUSICAL_REGION.start,
                                    (phase - last_knot as f64) / (period - last_knot as f64),
                                )
                            } else {
                                let left_frame = MUSICAL_REGION.start + phase.floor() as usize;
                                (left_frame, left_frame + 1, phase.fract())
                            }
                        };
                        assert!(left_frame < SOURCE_FRAMES && right_frame < SOURCE_FRAMES);
                        let left = read(left_frame, channel);
                        let right = read(right_frame, channel);
                        left + (right - left) * fraction as f32
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn fractional_musical_prepared_native_fifo_matches_independent_pcm_through_1000_cycles() {
        let patterns = [&[512][..], &[1, 127, 384, 96, 257, 512, 31][..]];
        let mut cases = 0;
        for rate in [44_100, 48_000, 96_000] {
            for ratio in [0.73_f64, 1.371_234_567_890_123] {
                for period in [126.75_f64, 127.25] {
                    // All 1000 continuous musical cycles, including the productive adapter's
                    // real prefix. The 75-cycle checkpoint is part of the same retained PCM.
                    let total_frames = ((1000.0 * period + 6.0) / ratio).ceil() as usize + 17;
                    let continuation = total_frames - PREPARED_HISTORY_FRAMES;
                    let checkpoint_75 = ((75.0 * period + 6.0) / ratio).ceil() as usize;
                    assert!(checkpoint_75 > PREPARED_HISTORY_FRAMES);
                    for mode in [
                        ExplicitSeekMode::Normal,
                        ExplicitSeekMode::BeforeLoop,
                        ExplicitSeekMode::AfterLoop,
                    ] {
                        for mask in [None, Some(0b1010)] {
                            let reference = musical_fixture(rate, ratio, period, mode, mask);
                            let mut raw = RubberBandLiveShifter::new(rate, 2).unwrap();
                            raw.prepare_exact_pitch(pitch_scale_for_tempo_ratio(ratio))
                                .unwrap();
                            let block = raw.block_size();
                            let mut expected = vec![vec![0.0; block - 1]; 2];
                            let mut shifted = vec![vec![0.0; block]; 2];
                            for start in (0..total_frames.div_ceil(block) * block).step_by(block) {
                                raw.shift(
                                    &musical_oracle_feed(&reference, mode, start, block),
                                    &mut shifted,
                                )
                                .unwrap();
                                for channel in 0..2 {
                                    expected[channel].extend_from_slice(&shifted[channel]);
                                }
                            }
                            assert!(
                                expected[0][checkpoint_75..total_frames]
                                    .iter()
                                    .any(|sample| sample.abs() > 0.01)
                            );
                            for pattern in patterns {
                                let request = musical_fixture(rate, ratio, period, mode, mask);
                                let mut state = NativeAdapterState::new(
                                    Some(RubberBandLiveShifter::new(rate, 2).unwrap()),
                                    2,
                                );
                                state.prepare_source(request).unwrap();
                                let mut playback = state.source.as_ref().unwrap().playback;
                                let mut plan = state.source.as_ref().unwrap().plan;
                                assert_eq!(playback.loop_period(), Some(period));
                                assert_eq!(plan.loop_period.scalar_period(), Some(period));
                                let mut feed = vec![vec![0.0; DEFAULT_BLOCK_SAMPLES]; 2];
                                let mut output = vec![vec![0.0; DEFAULT_BLOCK_SAMPLES]; 2];
                                let mut elapsed = 0;
                                let mut step = 0;
                                while elapsed < continuation {
                                    let frames =
                                        pattern[step % pattern.len()].min(continuation - elapsed);
                                    let source = state.source.as_ref().unwrap();
                                    plan.fill_fractional_buffers(
                                        &source.sample,
                                        source.stems.as_ref(),
                                        &playback,
                                        &mut feed,
                                        frames,
                                    )
                                    .unwrap();
                                    state
                                        .render(
                                            &feed,
                                            &mut output,
                                            frames,
                                            pitch_scale_for_tempo_ratio(ratio),
                                        )
                                        .unwrap();
                                    for channel in 0..2 {
                                        assert_eq!(
                                            &output[channel][..frames],
                                            &expected[channel][PREPARED_HISTORY_FRAMES + elapsed
                                                ..PREPARED_HISTORY_FRAMES + elapsed + frames],
                                            "fractional native FIFO rate={rate} ratio={ratio} P={period} mode={mode:?} mask={mask:?} n={elapsed}"
                                        );
                                    }
                                    assert!(playback.advance(frames));
                                    let position = playback.position();
                                    plan.frame_pos = position.frame;
                                    plan.seek_mode = position.seek_mode;
                                    assert_eq!(
                                        state.input_fifo[0].len() + state.output_fifo[0].len(),
                                        block - 1
                                    );
                                    elapsed += frames;
                                    step += 1;
                                }
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 144);
    }

    #[test]
    fn same_phase_rate_and_integer_bounds_do_not_authorize_different_musical_worker_domain() {
        let request = musical_fixture(48_000, 0.73, 127.25, ExplicitSeekMode::Normal, None);
        let mut changed_clock = request.playback;
        assert!(changed_clock.configure_domain(
            SourceLoopDomain::musical(SOURCE_FRAMES, MUSICAL_REGION, 126.75).unwrap(),
        ));
        assert_eq!(request.playback.position(), changed_clock.position());
        assert_eq!(request.playback.tempo_ratio(), changed_clock.tempo_ratio());
        assert!(!request.playback.matches_exact(&changed_clock));
        let changed_plan = SourceReadPlan {
            loop_period: crate::audio_engine::source_reader::SourceReaderPeriod::from_scalar_period(
                Some(126.75),
            ),
            ..request.plan
        };
        assert!(!request.plan.matches_exact(changed_plan));
        assert!(!request.plan.matches_source_contract(changed_plan));
        let feed = ProductiveSourceFeed {
            pitch_intent: super::super::pitch_intent::PitchIntent::default(),
            key_lock: true,
            sample: &request.sample,
            outgoing_stems: None,
            stems: None,
            sample_rate_hz: 48_000,
            accepted: None,
            plan: changed_plan,
            playback: &changed_clock,
            permit: Some(&request.permit),
            output_frame: Some(request.target_output_frame),
        };
        assert!(!request.matches_contract(&feed, request.epoch));
        assert!(!request.matches_adoption(&feed, request.epoch, request.request_id));
        // One binary64 change is still a distinct worker/source contract.
        let changed_plan = SourceReadPlan {
            loop_period: crate::audio_engine::source_reader::SourceReaderPeriod::from_scalar_period(
                Some(f64::from_bits(127.25_f64.to_bits() + 1)),
            ),
            ..request.plan
        };
        assert!(!request.plan.matches_source_contract(changed_plan));
    }
}
