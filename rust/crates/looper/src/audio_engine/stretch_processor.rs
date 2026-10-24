use super::constant_timing::AcceptedTimingProjection;
use super::native_history_permit::NativeHistoryPermit;
use super::pitch_intent::PitchIntent;
use super::prepared_native_history::{
    NativeAdapterState, PITCH_SCALE_EPSILON, SourceReadCheckpoint,
};
use super::productive_source_history::{ProductiveSourceBinding, ProductiveSourceHistory};
use super::source_feed_history::{SourceFeedContext, SourceReadDomain, SourceReadReceipt};
use super::source_playback::SourcePlayback;
use super::source_reader::{
    STEM_TRANSITION_RAMP_FRAMES, SourceReadPlan, StemReadSets, StemTransition,
};
#[cfg(test)]
use crate::audio_engine::key_lock_preparation::create_key_lock_preparation;
use crate::audio_engine::key_lock_preparation::{
    KeyLockPreparationLane, KeyLockPreparationWorker, ProspectiveSourceHistory, SourceExchange,
};
#[cfg(test)]
use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
#[cfg(test)]
use crate::audio_engine::rubberband_backend::pitch_scale_for_tempo_ratio;
use crate::messages::{PreparedStemSet, SampleBuffer};
use std::sync::atomic::Ordering;

#[cfg(test)]
#[path = "stretch_processor/coordinate_units_tests.rs"]
mod coordinate_units_tests;

#[cfg(test)]
#[path = "stretch_processor/source_read_receipt_tests.rs"]
mod source_read_receipt_tests;

pub(crate) struct ProductiveSourceFeed<'a> {
    pub(crate) pitch_intent: PitchIntent,
    pub(crate) key_lock: bool,
    pub(crate) sample: &'a SampleBuffer,
    pub(crate) stems: Option<&'a PreparedStemSet>,
    pub(crate) outgoing_stems: Option<&'a PreparedStemSet>,
    pub(crate) sample_rate_hz: u32,
    pub(crate) accepted: Option<AcceptedTimingProjection>,
    pub(crate) plan: SourceReadPlan,
    pub(crate) playback: &'a SourcePlayback,
    pub(crate) permit: Option<&'a NativeHistoryPermit>,
    pub(crate) output_frame: Option<u64>,
}

/// Default maximum block size handled by the per-voice DSP wrapper.
///
/// Source playback fills these fixed output-domain buffers before Key Lock processing. The bound
/// covers the 512-frame CPAL callback and bounded internal segments without resizing.
pub const DEFAULT_BLOCK_SAMPLES: usize = 1024;

#[cfg(test)]
const DEFAULT_SAMPLE_RATE_HZ: f32 = 48_000.0;
#[cfg(test)]
const RUBBERBAND_MIN_SAMPLE_RATE_HZ: f32 = 8_000.0;

pub struct StretchProcessor {
    channels: usize,
    varispeed: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
    native: Box<NativeAdapterState>,
    preparation_epoch: u64,
    request_id: u64,
    request_pending: bool,
    prospective_request: Option<u64>,
    adopted_request_id: Option<u64>,
    preparation: KeyLockPreparationLane,
    productive_history: Option<ProductiveSourceHistory>,
    wet_rendered: bool,
    wet_render_failed: bool,
    wet_output_emitted: bool,
    wet_source_rendered: bool,
    wet_source_output_emitted: bool,
    // Standalone processors retain their worker. Mixer processors share an engine worker.
    _preparation_worker: Option<KeyLockPreparationWorker>,
}

unsafe impl Send for StretchProcessor {}

/// Comparison-only input to the original source-lane busy predicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceLaneRetryState {
    request: u64,
    epoch: u64,
    source_epoch: u64,
    pending: bool,
    prospective: Option<u64>,
    result_pending: bool,
}

impl SourceLaneRetryState {
    pub(crate) fn busy(self) -> bool {
        self.pending || self.prospective.is_some() || self.result_pending
    }
}

impl StretchProcessor {
    /// A missing mapping is the established identity; a refused mapping is never a fallback.
    pub(crate) fn actual_source_checkpoint(
        &self,
        playback: &SourcePlayback,
        plan: SourceReadPlan,
    ) -> Result<Option<SourceReadCheckpoint>, super::rubberband_backend::RubberBandError> {
        self.native
            .retained_start_feed(playback, plan)
            .map(|actual| actual.map(|(playback, plan)| SourceReadCheckpoint { playback, plan }))
    }

    pub(crate) fn retained_source_chunk_limit(
        &self,
        playback: &SourcePlayback,
        plan: SourceReadPlan,
        max_frames: usize,
    ) -> usize {
        // An invalid map still consumes a bounded ordinary chunk as an explicit gap.
        // Its renderer refuses rather than quietly selecting logical source input.
        self.native
            .retained_start_feed_chunk_limit(playback, plan, max_frames)
            .unwrap_or(max_frames)
    }

    pub(crate) fn has_retained_source_input(&self) -> bool {
        self.native.has_retained_feed()
    }

    /// A productive Native can need no new history while still owning audible/FIFO state.
    pub(crate) fn has_active_native_ownership(&self) -> bool {
        self.native.active
    }

    #[cfg(test)]
    pub(crate) fn retained_mapped_output_coordinates(
        &self,
    ) -> Result<
        Option<super::prepared_native_history::RetainedMappedOutputCoordinates>,
        super::rubberband_backend::RubberBandError,
    > {
        self.native.retained_mapped_output_coordinates()
    }

    pub(crate) fn retained_rate_change_ready(
        &self,
        feed: &ProductiveSourceFeed<'_>,
        target: f64,
    ) -> bool {
        let Ok(Some(actual)) = self.actual_source_checkpoint(feed.playback, feed.plan) else {
            return !self.has_retained_source_input();
        };
        let actual_feed = ProductiveSourceFeed {
            playback: &actual.playback,
            plan: actual.plan,
            ..*feed
        };
        !self.wet_mode_failed()
            && self
                .native
                .has_productive_continuity(&actual_feed, self.productive_history)
            && self.native.matches_effective_source(
                &actual_feed,
                self.preparation_epoch,
                self.productive_history,
            )
            && self.native.can_retarget_retained_rate(target)
    }

    pub(crate) fn retarget_retained_rate_claimed(&mut self, target: f64) {
        self.native.retarget_retained_rate_claimed(target);
    }

    /// All current-bank participants are checked before the mixer's first selection store.
    pub(crate) fn retained_stem_change_ready(
        &self,
        previous: &ProductiveSourceFeed<'_>,
        next: &ProductiveSourceFeed<'_>,
    ) -> bool {
        if self.native.has_owned_mapped_input() {
            return false;
        }
        if !self.has_retained_source_input() {
            return true;
        }
        let Ok(Some(actual)) = self.actual_source_checkpoint(previous.playback, previous.plan)
        else {
            return false;
        };
        let same_trajectory = previous.playback.matches_exact(next.playback)
            && SourceReadPlan {
                selection: next.plan.selection,
                transition: next.plan.transition,
                ..previous.plan
            }
            .matches_exact(next.plan);
        let transition = if previous.plan.selection == next.plan.selection {
            previous.plan.transition
        } else {
            StemTransition::start(previous.plan.selection, STEM_TRANSITION_RAMP_FRAMES)
        };
        if !same_trajectory
            || previous.pitch_intent != next.pitch_intent
            || previous.key_lock != next.key_lock
            || previous.accepted != next.accepted
            || ProductiveSourceBinding::new(
                previous.sample,
                previous.sample_rate_hz,
                previous.accepted,
            ) != ProductiveSourceBinding::new(next.sample, next.sample_rate_hz, next.accepted)
            || !transition.matches_exact(next.plan.transition)
        {
            return false;
        }
        let previous_actual = ProductiveSourceFeed {
            playback: &actual.playback,
            plan: actual.plan,
            ..*previous
        };
        let next_actual = ProductiveSourceFeed {
            playback: &actual.playback,
            plan: SourceReadPlan {
                selection: next.plan.selection,
                transition: if previous.plan.selection == next.plan.selection {
                    actual.plan.transition
                } else {
                    next.plan.transition
                },
                ..actual.plan
            },
            ..*next
        };
        !self.wet_mode_failed()
            && self
                .native
                .has_productive_continuity(&previous_actual, self.productive_history)
            && self.native.matches_effective_source(
                &previous_actual,
                self.preparation_epoch,
                self.productive_history,
            )
            && super::native_source_coverage::normal_loop_feed_available_with_readers(
                next_actual.sample,
                next_actual.plan.required_read_sets(StemReadSets::new(
                    next_actual.stems,
                    next_actual.outgoing_stems,
                )),
                next_actual.sample_rate_hz,
                next_actual.accepted,
                next_actual.plan,
                next_actual.playback,
            )
    }

    /// Recompute only callback-owned endpoints after every participant passed preflight.
    pub(crate) fn retain_stem_change_claimed(
        &mut self,
        previous: &ProductiveSourceFeed<'_>,
        next: &ProductiveSourceFeed<'_>,
    ) {
        if !self.has_retained_source_input() {
            return;
        }
        let actual = self
            .actual_source_checkpoint(previous.playback, previous.plan)
            .expect("preflighted retained stem mapping")
            .expect("preflighted retained stem endpoint");
        let logical = SourceReadCheckpoint {
            playback: *next.playback,
            plan: next.plan,
        };
        let next_actual = SourceReadCheckpoint {
            plan: SourceReadPlan {
                selection: next.plan.selection,
                transition: if previous.plan.selection == next.plan.selection {
                    actual.plan.transition
                } else {
                    next.plan.transition
                },
                ..actual.plan
            },
            ..actual
        };
        self.native
            .reconfigure_retained_feed_endpoint_claimed(
                SourceReadCheckpoint {
                    playback: *previous.playback,
                    plan: previous.plan,
                },
                actual,
                logical,
                next_actual,
            )
            .expect("unchanged preflighted retained stem endpoints");
        self.native
            .retain_stem_read_context_claimed(&ProductiveSourceFeed {
                playback: &next_actual.playback,
                plan: next_actual.plan,
                ..*next
            });
        self.wet_rendered = false;
        self.wet_output_emitted = false;
        self.wet_source_rendered = false;
        self.wet_source_output_emitted = false;
    }

    pub(crate) fn retained_stem_reader_required(&self, stems: &PreparedStemSet) -> bool {
        self.native.retained_stem_reader_required(stems)
    }

    pub(crate) fn preparation_epoch(&self) -> u64 {
        self.preparation_epoch
    }

    /// Scalar preflight for an event-owned candidate, before any mixer setting changes.
    pub(crate) fn prepared_pitch_matches(
        &self,
        candidate: &NativeAdapterState,
        feed: &ProductiveSourceFeed<'_>,
        epoch: u64,
        request_id: u64,
    ) -> bool {
        !self.wet_mode_failed()
            && candidate.active
            && candidate.used
            && !candidate.dirty
            && candidate
                .source
                .as_ref()
                .is_some_and(|source| source.matches_adoption(feed, epoch, request_id))
            && candidate
                .retained_start_feed(feed.playback, feed.plan)
                .is_ok()
    }

    /// Transfer an accepted event's exact source/pitch/FIFO state without allocation or drops.
    /// The caller retains the displaced owner in its existing off-thread retirement work.
    pub(crate) fn adopt_prepared_pitch(
        &mut self,
        candidate: &mut Box<NativeAdapterState>,
        feed: &ProductiveSourceFeed<'_>,
        epoch: u64,
        request_id: u64,
    ) -> bool {
        if !self.prepared_pitch_matches(candidate, feed, epoch, request_id) {
            return false;
        }
        self.adopt_prepared_pitch_claimed(candidate, feed, request_id);
        true
    }

    /// Only a caller that already owns the complete same-content claim may use this.
    /// Post-claim control revocation cannot partially undo the accepted frozen effect.
    pub(crate) fn adopt_prepared_pitch_claimed(
        &mut self,
        candidate: &mut Box<NativeAdapterState>,
        feed: &ProductiveSourceFeed<'_>,
        request_id: u64,
    ) {
        std::mem::swap(&mut self.native, candidate);
        self.request_pending = false;
        self.adopted_request_id = Some(request_id);
        self.productive_history = self.prepared_input_history(feed);
        self.wet_rendered = false;
        self.wet_render_failed = false;
        self.wet_output_emitted = false;
        self.wet_source_rendered = false;
        self.wet_source_output_emitted = false;
    }

    pub(crate) fn owns_pitch_contract(&self, pitch_intent: PitchIntent, key_lock: bool) -> bool {
        self.native.active
            && self.native.used
            && !self.native.dirty
            && self.native.source.as_ref().is_some_and(|source| {
                source.pitch_intent == pitch_intent
                    && self.native.effective_key_lock() == Some(key_lock)
            })
    }

    /// Preparation may have consumed actual input ahead of the logical adoption endpoint.
    fn prepared_input_history(
        &self,
        feed: &ProductiveSourceFeed<'_>,
    ) -> Option<ProductiveSourceHistory> {
        let (playback, plan) = self
            .native
            .retained_start_feed(feed.playback, feed.plan)
            .ok()?
            .unwrap_or((*feed.playback, feed.plan));
        Some(ProductiveSourceHistory {
            binding: ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted),
            next_position: playback.position(),
            domain: Some(plan.domain()),
            fed_output_frames: self.native.primed_output_frames as u64,
        })
    }

    fn retain_aligned_unity_history(&self, pitch_intent: PitchIntent, key_lock: bool) -> bool {
        self.native.active
            && self.native.source.as_ref().is_some_and(|source| {
                source.pitch_intent == pitch_intent
                    && self.native.effective_key_lock() == Some(key_lock)
            })
    }

    /// Classify an already validated Native scale without losing aligned unity history.
    pub(crate) fn wet_processing_required(
        &self,
        pitch_scale: f64,
        pitch_intent: PitchIntent,
        key_lock: bool,
    ) -> bool {
        (pitch_scale - 1.0).abs() > PITCH_SCALE_EPSILON
            || self.retain_aligned_unity_history(pitch_intent, key_lock)
    }

    pub(crate) fn native_identity(&self) -> usize {
        self.native
            .rubberband
            .as_ref()
            .map_or(0, |native| native.state_address())
    }

    /// Numeric equality alone cannot authorize keeping a source-specific DSP owner.
    pub(crate) fn can_retain_mode_at_unit_rate(&self, feed: &ProductiveSourceFeed<'_>) -> bool {
        if feed.playback.is_static_phase() {
            return false;
        }
        let Ok(actual) = self.actual_source_checkpoint(feed.playback, feed.plan) else {
            return false;
        };
        let actual = actual.unwrap_or(SourceReadCheckpoint {
            playback: *feed.playback,
            plan: feed.plan,
        });
        if actual.playback.is_static_phase() {
            return false;
        }
        let actual_feed = ProductiveSourceFeed {
            playback: &actual.playback,
            plan: actual.plan,
            ..*feed
        };
        !self.wet_mode_failed()
            && !self.source_lane_busy()
            && feed.pitch_intent.total_semitones() != 0
            && feed.playback.tempo_ratio().to_bits() == 1.0_f64.to_bits()
            && feed.playback.rate_target().to_bits() == 1.0_f64.to_bits()
            && actual.playback.tempo_ratio().to_bits() == 1.0_f64.to_bits()
            && actual.playback.rate_target().to_bits() == 1.0_f64.to_bits()
            && !feed.plan.transition.is_active()
            && !actual.plan.transition.is_active()
            && feed.plan.selection == super::source_reader::StemRenderSelection::full_mix()
            && super::native_source_coverage::complete_view_available(feed.sample)
            && feed.permit.is_some_and(|permit| {
                permit.current(feed.sample, feed.sample_rate_hz)
                    && permit.matches_projection(feed.accepted)
            })
            && self.native.pitch_scale.to_bits() == feed.pitch_intent.frequency_ratio().to_bits()
            && self
                .native
                .has_productive_continuity(&actual_feed, self.productive_history)
            && self.native.matches_effective_source(
                &actual_feed,
                self.preparation_epoch,
                self.productive_history,
            )
    }

    /// Preserve all native, FIFO, origin, source and request data after the complete mixer claim.
    pub(crate) fn retain_unit_rate_mode_claimed(&mut self, enabled: bool) {
        self.native.retain_effective_mode_claimed(enabled);
    }

    pub(crate) fn wet_mode_failed(&self) -> bool {
        self.wet_render_failed || self.preparation.worker_failed()
    }

    pub(crate) fn wet_render_ready(&self) -> bool {
        self.wet_rendered && self.native.active && self.native.used
    }

    pub(crate) fn wet_output_ready(&self) -> bool {
        self.wet_output_emitted
    }

    /// The chosen output belongs to the current source-specific Native/FIFO owner.
    /// A predecessor's generic wet-ready flag cannot acknowledge a new stem selection.
    pub(crate) fn wet_current_source_output_ready(&self) -> bool {
        self.wet_source_output_emitted
    }

    pub(crate) fn pending_mode_needs_dry(&self) -> bool {
        !self.native.used
    }

    pub(crate) fn chunk_until_mode_readiness(&self, max_frames: usize) -> usize {
        if self.native.used {
            max_frames
        } else {
            max_frames.min(
                self.native
                    .block_size
                    .saturating_sub(self.native.input_fifo.first().map_or(0, FixedFifo::len))
                    .max(1),
            )
        }
    }

    pub(crate) fn wet_mode_armed(&self) -> bool {
        (self.native.rubberband.is_some() && !self.native.dirty)
            || self.preparation.warmed_reserve_available()
    }

    /// Claim the lane's already warmed handle before admitting a new mode. The exchange only
    /// moves unique owners through fixed queues; Native preparation remains worker-owned.
    pub(crate) fn reserve_wet_mode(&mut self) -> bool {
        if !self.wet_mode_available() {
            return false;
        }
        self.reserve_wet_mode_claimed();
        true
    }

    pub(crate) fn wet_mode_available(&self) -> bool {
        !self.wet_mode_failed()
            && ((self.native.rubberband.is_some() && !self.native.dirty)
                || self.preparation.warmed_reserve_available())
    }

    /// The callback's complete effect claim owns the previously checked reserve.
    pub(crate) fn reserve_wet_mode_claimed(&mut self) {
        if self.native.rubberband.is_some() && !self.native.dirty {
            return;
        }
        self.preparation
            .exchange_claimed(&mut self.native.rubberband);
        self.native.dirty = false;
        self.native.used = false;
        self.native.pitch_scale = 1.0;
    }

    /// Admission for an intentional source cut must precede clearing usable old history.
    pub(crate) fn source_cut_available(&self) -> bool {
        (!self.native.used && !self.native.dirty && self.native.rubberband.is_some())
            || self.preparation.warmed_reserve_available()
    }

    /// Only the callback consumes this preflighted warmed owner and recycle capacity.
    pub(crate) fn reserve_source_cut_claimed(&mut self) {
        if !self.native.used && !self.native.dirty && self.native.rubberband.is_some() {
            return;
        }
        self.preparation
            .exchange_claimed(&mut self.native.rubberband);
        self.native.used = false;
        self.native.dirty = false;
        self.native.pitch_scale = 1.0;
    }

    /// A pending ON warms Native while the previously effective dry feed reaches the EQ.
    pub(crate) fn retain_dry_output(&mut self, frames: usize) {
        self.copy_varispeed_output(frames.min(DEFAULT_BLOCK_SAMPLES));
    }

    pub(crate) fn record_wet_output(&mut self) {
        // Called only after the render owner chose successful wet output. A worker failure
        // observed later cannot relabel audio that already reached this callback as dry.
        self.wet_output_emitted |= self.wet_render_ready();
        self.wet_source_output_emitted = self.wet_source_rendered && self.wet_render_ready();
    }

    /// Retire mode-specific Native/FIFO ownership without touching the source clock or EQ.
    pub(crate) fn mode_changed(&mut self, enabled: bool) {
        self.wet_rendered = false;
        self.wet_render_failed = false;
        self.wet_output_emitted = false;
        self.wet_source_rendered = false;
        self.wet_source_output_emitted = false;
        if !enabled {
            self.reset_rubberband_state();
        }
    }
    /// Scalar state already owned by the render path; no native processing or query.
    pub(crate) fn loop_acceptance_state(&self) -> (bool, f64, usize, usize, usize) {
        (
            self.native.active,
            self.native.pitch_scale,
            self.native.input_fifo.first().map_or(0, FixedFifo::len),
            self.native.output_fifo.first().map_or(0, FixedFifo::len),
            self.native.block_size,
        )
    }
    #[cfg(test)]
    pub fn new(channels: usize) -> Self {
        Self::with_sample_rate(channels, DEFAULT_SAMPLE_RATE_HZ)
    }

    #[cfg(test)]
    pub fn with_sample_rate(channels: usize, sample_rate_hz: f32) -> Self {
        let (mut lanes, worker) =
            create_key_lock_preparation(channels, sample_rate_to_u32(sample_rate_hz), 1)
                .expect("failed to prepare standalone Key Lock processor");
        let mut processor = Self::with_preparation_lane(channels, lanes.remove(0));
        processor._preparation_worker = Some(worker);
        processor
    }

    pub(crate) fn with_preparation_lane(
        channels: usize,
        mut preparation: KeyLockPreparationLane,
    ) -> Self {
        let varispeed = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect();
        let output = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect();

        let rubberband = preparation.take_initial();
        Self {
            channels,
            varispeed,
            output,
            native: Box::new(NativeAdapterState::new(rubberband, channels)),
            preparation_epoch: 0,
            request_id: 0,
            request_pending: false,
            prospective_request: None,
            adopted_request_id: None,
            preparation,
            productive_history: None,
            wet_rendered: false,
            wet_render_failed: false,
            wet_output_emitted: false,
            wet_source_rendered: false,
            wet_source_output_emitted: false,
            _preparation_worker: None,
        }
    }

    pub fn reset(&mut self) {
        self.wet_rendered = false;
        self.wet_render_failed = false;
        self.wet_output_emitted = false;
        self.wet_source_rendered = false;
        self.wet_source_output_emitted = false;
        self.invalidate_prepared();
        self.productive_history = None;
        for channel in &mut self.varispeed {
            channel.fill(0.0);
        }
        for channel in &mut self.output {
            channel.fill(0.0);
        }
        self.reset_rubberband_state();
    }

    /// Fence a pending source-specific candidate without touching current audio/native history.
    pub(crate) fn invalidate_prepared(&mut self) {
        self.prospective_request = None;
        self.preparation_epoch = self.preparation_epoch.saturating_add(1);
        self.preparation.invalidate_source(self.preparation_epoch);
    }

    pub(crate) fn hold_prospective_tuple(&mut self) {
        self.invalidate_prepared();
        self.request_pending = false;
        self.prospective_request = Some(0);
    }

    /// Poll only the bounded recycler before the mixer holds a prospective request.
    pub(crate) fn source_history_work_available(&mut self) -> bool {
        self.preparation.source_history_work_available()
    }

    /// A different accepted mode/source request retains its lane until retirement.
    pub(crate) fn source_lane_busy(&self) -> bool {
        self.request_pending
            || self.prospective_request.is_some()
            || self.preparation.source_result_pending()
    }

    pub(crate) fn source_lane_retry_state(&self) -> SourceLaneRetryState {
        SourceLaneRetryState {
            request: self.request_id,
            epoch: self.preparation_epoch,
            source_epoch: self
                .preparation
                .source_epoch_owner()
                .load(Ordering::Acquire),
            pending: self.request_pending,
            prospective: self.prospective_request,
            result_pending: self.preparation.source_result_pending(),
        }
    }

    /// Refusal may revoke only the exact source request this effect accepted.
    pub(crate) fn cancel_source_request_if_owned(&mut self, epoch: u64, request_id: u64) {
        if self.request_id != request_id {
            return;
        }
        if self.preparation_epoch == epoch && self.prospective_request == Some(request_id) {
            self.invalidate_prepared();
            self.request_pending = false;
        } else if self.preparation_epoch > epoch && self.prospective_request.is_none() {
            // Pause/source invalidation may already have revoked this same accepted request.
            // Its unique id still proves ownership; a later request always has a new id.
            self.preparation.retire_prepared();
            self.request_pending = false;
        }
    }

    pub(crate) fn request_prospective_tuple_with_history(
        &mut self,
        feed: &ProductiveSourceFeed<'_>,
        history: ProspectiveSourceHistory<'_>,
    ) -> Option<(u64, u64)> {
        let request_id = self.request_id.checked_add(1)?;
        if self.preparation_epoch == u64::MAX
            || !self.preparation.request_source_with_history(
                feed,
                history,
                self.preparation_epoch,
                request_id,
            )
        {
            return None;
        }
        self.request_id = request_id;
        self.request_pending = true;
        self.prospective_request = Some(request_id);
        Some((self.preparation_epoch, request_id))
    }

    pub(crate) fn prospective_tuple_matches(
        &mut self,
        feed: &ProductiveSourceFeed<'_>,
        epoch: u64,
        request_id: u64,
    ) -> bool {
        self.preparation_epoch == epoch
            && self.prospective_request == Some(request_id)
            && self.preparation.source_matches(feed, epoch, request_id)
    }

    pub(crate) fn prospective_rate_change_ready(
        &mut self,
        feed: &ProductiveSourceFeed<'_>,
        epoch: u64,
        request_id: u64,
        target: f64,
    ) -> bool {
        self.preparation_epoch == epoch
            && self.prospective_request == Some(request_id)
            && self
                .preparation
                .source_rate_change_ready(feed, epoch, request_id, target)
    }

    #[cfg(test)]
    pub(crate) fn prospective_work_ready(&mut self) -> bool {
        self.preparation.prepared_state().is_some()
    }

    #[cfg(test)]
    pub(crate) fn prospective_work_finished(&self) -> bool {
        self.preparation.source_work_finished(self.request_id)
    }

    #[cfg(test)]
    pub(crate) fn source_preparation_pending(&self) -> bool {
        self.request_pending
    }

    pub(crate) fn adopt_prospective_tuple_claimed(
        &mut self,
        feed: &ProductiveSourceFeed<'_>,
        request_id: u64,
    ) {
        self.preparation.exchange_source_claimed(&mut self.native);
        self.prospective_request = None;
        self.request_pending = false;
        self.adopted_request_id = Some(request_id);
        self.productive_history = self.prepared_input_history(feed);
        self.wet_render_failed = false;
        self.wet_rendered = false;
        self.wet_output_emitted = false;
        self.wet_source_rendered = false;
        self.wet_source_output_emitted = false;
    }

    pub(crate) fn source_epoch_owner(&self) -> &std::sync::Arc<std::sync::atomic::AtomicU64> {
        self.preparation.source_epoch_owner()
    }

    pub(crate) fn retire_prepared(&mut self) {
        if self.prospective_request.is_some() {
            self.preparation.poll_source_recycling();
        } else {
            self.preparation.retire_prepared();
        }
    }

    pub(crate) fn chunk_until_prepared_adoption(
        &mut self,
        output_frame: u64,
        max_frames: usize,
    ) -> usize {
        self.preparation
            .chunk_until_prepared_adoption(output_frame, max_frames)
    }

    /// Read the actual feed coordinates before feeding live native/adapter state.
    /// Projection refreshes retain chronological history only across exact source continuity.
    /// A receipt describes only a successful reader and render pass, including dry bypass.
    /// A refused read/render remains a gap even when the independent logical clock advances.
    pub(crate) fn process_source(
        &mut self,
        feed: ProductiveSourceFeed<'_>,
        output_frames: usize,
        _preserve_pitch: bool,
    ) -> Option<SourceReadReceipt> {
        self.wet_rendered = false;
        self.wet_source_rendered = false;
        self.wet_source_output_emitted = false;
        let frames = output_frames.min(DEFAULT_BLOCK_SAMPLES);
        if frames == 0 || self.channels == 0 {
            return None;
        }
        if !self
            .native
            .owned_mapped_read_ready(feed.playback, feed.plan, feed.output_frame, frames)
        {
            self.silence_output(frames);
            self.wet_render_failed = true;
            return None;
        }
        let retained_feed = match self.native.retained_start_feed(feed.playback, feed.plan) {
            Ok(actual) => actual,
            Err(_) => {
                self.silence_output(frames);
                self.wet_render_failed = true;
                return None;
            }
        };
        let feed = ProductiveSourceFeed {
            playback: retained_feed
                .as_ref()
                .map_or(feed.playback, |(actual, _)| actual),
            plan: retained_feed.as_ref().map_or(feed.plan, |(_, plan)| *plan),
            ..feed
        };
        let readers = feed
            .plan
            .required_read_sets(StemReadSets::new(feed.stems, feed.outgoing_stems));
        let feed = ProductiveSourceFeed {
            stems: readers.current,
            outgoing_stems: readers.outgoing,
            ..feed
        };
        if !feed.playback.can_read_frames(frames) {
            self.silence_output(frames);
            self.wet_render_failed = true;
            return None;
        }
        let Some(next_position) = feed.playback.position_at(frames) else {
            self.silence_output(frames);
            self.wet_render_failed = true;
            return None;
        };
        if self
            .native
            .source_transition
            .is_some_and(|expected| !expected.matches_exact(feed.plan.transition))
        {
            self.native.source_transition = None;
        }
        let Ok(pitch_scale) = feed
            .pitch_intent
            .native_pitch_scale(feed.playback.tempo_ratio(), feed.key_lock)
        else {
            self.silence_output(frames);
            self.wet_render_failed = true;
            return None;
        };
        let wet = self.wet_processing_required(pitch_scale, feed.pitch_intent, feed.key_lock);
        // Static correction has no Native/FIFO variable-history integration yet.
        if feed.playback.is_static_phase() && wet {
            self.silence_output(frames);
            self.wet_render_failed = true;
            return None;
        }
        if retained_feed.is_some()
            && (!wet
                || !self.native.matches_effective_source(
                    &feed,
                    self.preparation_epoch,
                    self.productive_history,
                ))
        {
            self.silence_output(frames);
            self.wet_render_failed = true;
            return None;
        }
        let finite = !super::native_source_coverage::complete_view_available(feed.sample);
        if finite
            && ((wet
                && !super::native_source_coverage::normal_loop_feed_available_with_readers(
                    feed.sample,
                    readers,
                    feed.sample_rate_hz,
                    feed.accepted,
                    feed.plan,
                    feed.playback,
                ))
                || feed
                    .plan
                    .fill_fractional_buffers_checked_with_readers(
                        feed.sample,
                        readers,
                        feed.playback,
                        self.resampled_buffers_mut(frames),
                        frames,
                    )
                    .is_err())
        {
            // Never consume missing-tap zeros or touch productive native/FIFO/history state.
            self.silence_output(frames);
            self.wet_render_failed = wet;
            return None;
        }
        if self.prospective_request.is_none() && retained_feed.is_none() {
            if !wet && self.request_pending {
                self.invalidate_prepared();
            }
            if self.request_pending && self.preparation.source_work_finished(self.request_id) {
                self.request_pending = false;
            }
            match self.preparation.exchange_source(
                &mut self.native,
                &feed,
                self.preparation_epoch,
                self.request_id,
            ) {
                SourceExchange::Adopted(request_id) => {
                    self.request_pending = false;
                    self.adopted_request_id = Some(request_id);
                    self.productive_history = self.prepared_input_history(&feed);
                }
                SourceExchange::Rejected(_request_id) => self.request_pending = false,
                SourceExchange::Pending => {}
            }
            if wet
                && !self.request_pending
                && !self.native.matches_effective_source(
                    &feed,
                    self.preparation_epoch,
                    self.productive_history,
                )
                && let Some(request_id) = self.request_id.checked_add(1)
                && self.preparation_epoch != u64::MAX
                && self
                    .preparation
                    .request_source(&feed, self.preparation_epoch, request_id)
            {
                self.request_id = request_id;
                self.request_pending = true;
            }
        }
        let binding = ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted);
        let position = feed.playback.position();
        if self
            .productive_history
            .is_some_and(|history| !history.continues(binding, position, Some(feed.plan.domain())))
        {
            // Discard only fixed adapter storage. Used native state stays uniquely owned until
            // the existing worker lane can exchange and reset it off the callback.
            self.reset_rubberband_state();
        }
        let prior_frames = self
            .productive_history
            .map_or(0, |history| history.fed_output_frames);
        if !finite
            && feed
                .plan
                .fill_fractional_buffers_with_readers(
                    feed.sample,
                    readers,
                    feed.playback,
                    self.resampled_buffers_mut(frames),
                    frames,
                )
                .is_err()
        {
            self.silence_output(frames);
            self.wet_render_failed = wet;
            return None;
        }
        self.process_resampled_with_pitch(
            frames,
            feed.playback.tempo_ratio(),
            feed.key_lock,
            feed.pitch_intent,
        );
        if wet && !self.wet_rendered {
            // The source reader may have completed, but reserve starvation or Native failure
            // did not complete the render pass. Do not publish a successful history receipt.
            return None;
        }
        let receipt = SourceReadReceipt {
            playback: *feed.playback,
            plan: feed.plan,
            context: SourceFeedContext::new(binding, readers),
            output_frames: frames,
            domain: if wet {
                SourceReadDomain::NativeInput
            } else {
                SourceReadDomain::DryOutput
            },
        };
        self.wet_source_rendered = self.wet_render_ready()
            && self.native.matches_effective_source(
                &feed,
                self.preparation_epoch,
                self.productive_history,
            );
        if self.wet_source_rendered
            && let Some(expected) = &mut self.native.source_transition
        {
            expected.advance_output_frames(frames, feed.playback.tempo_ratio());
        }
        if receipt.domain == SourceReadDomain::NativeInput {
            self.productive_history = Some(ProductiveSourceHistory {
                binding: receipt.context.binding,
                next_position,
                domain: Some(receipt.plan.domain()),
                fed_output_frames: prior_frames.saturating_add(receipt.output_frames as u64),
            });
            self.native
                .advance_retained_start_feed(receipt.output_frames);
        } else {
            // Dry bypass and failed reserve admission own no productive wet history.
            self.productive_history = None;
        }
        Some(receipt)
    }

    /// Returns fixed feed storage whose first `output_frames` samples the source reader fills.
    pub fn resampled_buffers_mut(&mut self, output_frames: usize) -> &mut [Vec<f32>] {
        debug_assert!(output_frames <= DEFAULT_BLOCK_SAMPLES);
        &mut self.varispeed
    }

    /// Applies Key Lock or dry bypass to the already resampled output-domain feed.
    pub fn process_resampled(
        &mut self,
        output_frames: usize,
        tempo_ratio: f64,
        preserve_pitch: bool,
    ) {
        self.process_resampled_with_pitch(
            output_frames,
            tempo_ratio,
            preserve_pitch,
            PitchIntent::default(),
        );
    }

    /// Render one native pitch pass. The immutable source reader owns all duration/rate changes.
    pub(crate) fn process_resampled_with_pitch(
        &mut self,
        output_frames: usize,
        tempo_ratio: f64,
        key_lock: bool,
        pitch_intent: PitchIntent,
    ) {
        if self.channels == 0 {
            return;
        }

        let output_samples = output_frames.min(DEFAULT_BLOCK_SAMPLES);
        let Ok(pitch_scale) = pitch_intent.native_pitch_scale(tempo_ratio, key_lock) else {
            self.silence_output(output_samples);
            self.wet_render_failed = true;
            return;
        };

        if self.wet_processing_required(pitch_scale, pitch_intent, key_lock) {
            if self.native.rubberband.is_some() {
                self.process_rubberband(output_samples, pitch_scale);
            } else {
                self.silence_output(output_samples);
                self.wet_render_failed = true;
            }
        } else {
            self.deactivate_rubberband_if_needed();
            self.copy_varispeed_output(output_samples);
        }
    }

    pub fn output_buffers(&self) -> &[Vec<f32>] {
        &self.output
    }

    fn process_rubberband(&mut self, output_samples: usize, pitch_scale: f64) {
        if !self.native.active && self.native.dirty {
            if !self.preparation.exchange(&mut self.native.rubberband) {
                self.silence_output(output_samples);
                return;
            }
            self.native.dirty = false;
            self.native.used = false;
            self.native.pitch_scale = 1.0;
        }
        if self
            .native
            .render(
                &self.varispeed,
                &mut self.output,
                output_samples,
                pitch_scale,
            )
            .is_err()
        {
            self.reset_rubberband_state();
            self.silence_output(output_samples);
            self.wet_render_failed = true;
        } else {
            self.wet_rendered = true;
            self.wet_render_failed = false;
        }
    }

    fn copy_varispeed_output(&mut self, output_samples: usize) {
        for channel in 0..self.channels {
            self.output[channel][..output_samples]
                .copy_from_slice(&self.varispeed[channel][..output_samples]);
        }
    }

    fn silence_output(&mut self, output_samples: usize) {
        for channel in &mut self.output {
            channel[..output_samples].fill(0.0);
        }
    }

    fn deactivate_rubberband_if_needed(&mut self) {
        if self.native.active {
            self.reset_rubberband_state();
        }
    }

    fn reset_rubberband_state(&mut self) {
        self.productive_history = None;
        self.preparation
            .retire_source_owner(&mut self.native.source);
        self.native.reset_adapter();
    }

    #[cfg(test)]
    pub(crate) fn processing_capacity(&self) -> usize {
        self.varispeed.first().map_or(0, Vec::len)
    }

    #[cfg(test)]
    pub(crate) fn varispeed_buffers(&self) -> &[Vec<f32>] {
        &self.varispeed
    }

    #[cfg(test)]
    pub(crate) fn rubberband_block_size(&self) -> usize {
        self.native.block_size
    }

    #[cfg(test)]
    pub(crate) fn rubberband_start_delay(&self) -> usize {
        self.native
            .rubberband
            .as_ref()
            .map_or(0, RubberBandLiveShifter::start_delay)
    }

    #[cfg(test)]
    pub(crate) fn adapter_delay_frames(&self) -> usize {
        self.native.block_size.saturating_sub(1)
    }

    #[cfg(test)]
    pub(crate) fn rubberband_input_fifo_capacity(&self) -> usize {
        self.native
            .input_fifo
            .first()
            .map_or(0, FixedFifo::capacity)
    }

    #[cfg(test)]
    pub(crate) fn rubberband_output_fifo_capacity(&self) -> usize {
        self.native
            .output_fifo
            .first()
            .map_or(0, FixedFifo::capacity)
    }

    #[cfg(test)]
    pub(crate) fn productive_history(&self) -> Option<ProductiveSourceHistory> {
        self.productive_history
    }

    #[cfg(test)]
    pub(crate) fn native_state_address(&self) -> usize {
        self.native
            .rubberband
            .as_ref()
            .map_or(0, RubberBandLiveShifter::state_address)
    }

    #[cfg(all(test, windows))]
    pub(crate) fn native_pitch_scale_observation(&self) -> Option<f64> {
        self.native
            .rubberband
            .as_ref()
            .map(RubberBandLiveShifter::pitch_scale)
    }

    #[cfg(test)]
    pub(crate) fn native_origin_tuple(&self) -> Option<(PitchIntent, bool, u64)> {
        self.native
            .source
            .as_ref()
            .map(|source| (source.pitch_intent, source.key_lock, source.request_id))
    }

    #[cfg(test)]
    pub(crate) fn pending_fifo_frames(&self) -> (usize, usize) {
        (
            self.native.input_fifo[0].len(),
            self.native.output_fifo[0].len(),
        )
    }

    #[cfg(test)]
    pub(crate) fn fail_preparation_worker(&self) {
        self.preparation.fail_worker();
    }

    #[cfg(test)]
    pub(crate) fn exhaust_warmed_reserve(&mut self) {
        self.native.dirty = true;
        self.preparation.withhold_warmed_reserve();
    }

    #[cfg(test)]
    pub(crate) fn prepared_target_output_frame(&mut self) -> Option<u64> {
        self.preparation
            .prepared_state()?
            .source
            .as_ref()
            .map(|source| source.target_output_frame)
    }

    #[cfg(test)]
    pub(crate) fn source_preparation_ready(&mut self) -> bool {
        self.preparation.prepared_state().is_some()
    }

    #[cfg(test)]
    pub(crate) fn prepared_native_address(&mut self) -> usize {
        self.preparation
            .prepared_state()
            .and_then(|state| state.rubberband.as_ref())
            .map_or(0, RubberBandLiveShifter::state_address)
    }

    #[cfg(test)]
    pub(crate) fn prepared_fifo_frames(&mut self) -> Option<(usize, usize)> {
        self.preparation
            .prepared_state()
            .map(|state| (state.input_fifo[0].len(), state.output_fifo[0].len()))
    }

    #[cfg(test)]
    pub(crate) fn prepared_tap_observation(
        &mut self,
    ) -> Option<super::source_reader::TapReadObservation> {
        self.preparation
            .prepared_state()?
            .source
            .as_ref()?
            .coverage
            .as_ref()
            .map(|coverage| coverage.prepared_taps)
    }

    #[cfg(test)]
    pub(crate) fn native_primed_output_frames(&self) -> usize {
        self.native.primed_output_frames
    }

    #[cfg(test)]
    pub(crate) fn native_has_physical_birth_context_for_test(&self) -> bool {
        self.native.has_physical_birth_context()
    }

    #[cfg(test)]
    pub(crate) fn native_tap_observation(
        &self,
    ) -> Option<super::source_reader::TapReadObservation> {
        self.native
            .source
            .as_ref()?
            .coverage
            .as_ref()
            .map(|coverage| coverage.prepared_taps)
    }

    #[cfg(test)]
    pub(crate) fn adopted_request_id(&self) -> Option<u64> {
        self.adopted_request_id
    }
}

#[cfg(test)]
fn sample_rate_to_u32(sample_rate_hz: f32) -> u32 {
    if !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
        return DEFAULT_SAMPLE_RATE_HZ as u32;
    }

    sample_rate_hz
        .round()
        .clamp(RUBBERBAND_MIN_SAMPLE_RATE_HZ, u32::MAX as f32) as u32
}

pub(crate) struct FixedFifo {
    buffer: Vec<f32>,
    read_pos: usize,
    write_pos: usize,
    len: usize,
}

#[cfg(test)]
#[path = "pitch_dsp_tests.rs"]
mod pitch_dsp_tests;

impl FixedFifo {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity.max(1)],
            read_pos: 0,
            write_pos: 0,
            len: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.read_pos = 0;
        self.write_pos = 0;
        self.len = 0;
    }

    #[cfg(test)]
    fn capacity(&self) -> usize {
        self.buffer.len()
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn push_slice(&mut self, input: &[f32]) -> usize {
        let mut written = 0;
        for sample in input {
            if self.len == self.buffer.len() {
                break;
            }
            self.buffer[self.write_pos] = *sample;
            self.write_pos = (self.write_pos + 1) % self.buffer.len();
            self.len += 1;
            written += 1;
        }
        written
    }

    pub(super) fn push_silence(&mut self, frames: usize) {
        debug_assert!(self.len + frames <= self.buffer.len());
        for _ in 0..frames.min(self.buffer.len().saturating_sub(self.len)) {
            self.buffer[self.write_pos] = 0.0;
            self.write_pos = (self.write_pos + 1) % self.buffer.len();
            self.len += 1;
        }
    }

    pub(super) fn pop_into(&mut self, output: &mut [f32]) -> usize {
        let mut read = 0;
        for sample in output {
            if self.len == 0 {
                break;
            }
            *sample = self.buffer[self.read_pos];
            self.read_pos = (self.read_pos + 1) % self.buffer.len();
            self.len -= 1;
            read += 1;
        }
        read
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estimate_frequency(samples: &[f32], sample_rate_hz: f32) -> f32 {
        let mut crossings = Vec::new();
        for index in 1..samples.len() {
            let prev = samples[index - 1];
            let current = samples[index];
            if prev <= 0.0 && current > 0.0 {
                let denom = current - prev;
                let frac = if denom.abs() > f32::EPSILON {
                    -prev / denom
                } else {
                    0.0
                };
                crossings.push(index as f32 - 1.0 + frac);
            }
        }

        if crossings.len() < 2 {
            return 0.0;
        }

        let span = crossings[crossings.len() - 1] - crossings[0];
        if span <= 0.0 {
            return 0.0;
        }

        (crossings.len() - 1) as f32 * sample_rate_hz / span
    }

    #[test]
    fn buffers_are_preallocated_for_callback_bounds() {
        let processor = StretchProcessor::new(2);

        assert_eq!(processor.processing_capacity(), DEFAULT_BLOCK_SAMPLES);
        assert!(processor.rubberband_block_size() > 0);
        assert!(processor.rubberband_start_delay() > 0);
        assert!(processor.rubberband_input_fifo_capacity() >= DEFAULT_BLOCK_SAMPLES);
        assert!(processor.rubberband_output_fifo_capacity() >= DEFAULT_BLOCK_SAMPLES);
    }

    #[test]
    fn refused_history_capture_preserves_prospective_request_state_and_later_id() {
        use super::super::prepared_native_history::tests::fixture;
        use super::super::source_feed_history::SourceFeedHistory;
        use super::super::source_reader::ExplicitSeekMode;
        use std::sync::{Arc, atomic::AtomicU64};
        use std::time::{Duration, Instant};

        let mut processor = StretchProcessor::new(1);
        let source = fixture(1, 48_000, 1.0, ExplicitSeekMode::Normal, None);
        let feed = ProductiveSourceFeed {
            pitch_intent: PitchIntent::new(3, 0).unwrap(),
            key_lock: false,
            sample: &source.sample,
            stems: None,
            outgoing_stems: None,
            sample_rate_hz: 48_000,
            accepted: None,
            plan: source.plan,
            playback: &source.playback,
            permit: Some(&source.permit),
            output_frame: Some(100),
        };
        let history = SourceFeedHistory::new();
        let lifetime = Arc::new(AtomicU64::new(1));
        assert!(processor.source_history_work_available());
        processor.hold_prospective_tuple();
        let epoch = processor.preparation_epoch;
        assert!(
            processor
                .request_prospective_tuple_with_history(
                    &feed,
                    ProspectiveSourceHistory {
                        history: &history,
                        voice_lifetime: &lifetime,
                        voice_generation: 2,
                        execution_frame: 8292,
                        paused: false,
                        actual_feed: None,
                        source_feed_context:
                            super::super::source_feed_history::SourceFeedContext::new(
                                ProductiveSourceBinding::new(
                                    feed.sample,
                                    feed.sample_rate_hz,
                                    feed.accepted
                                ),
                                feed.plan.required_read_sets(StemReadSets::new(
                                    feed.stems,
                                    feed.outgoing_stems
                                )),
                            ),
                    },
                )
                .is_none()
        );
        assert_eq!(processor.preparation_epoch, epoch);
        assert_eq!(processor.request_id, 0);
        assert!(!processor.request_pending);
        assert_eq!(processor.prospective_request, Some(0));

        let deadline = Instant::now() + Duration::from_secs(2);
        while !processor.source_history_work_available() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(processor.source_history_work_available());
        assert_eq!(
            processor.request_prospective_tuple_with_history(
                &feed,
                ProspectiveSourceHistory {
                    history: &history,
                    voice_lifetime: &lifetime,
                    voice_generation: 1,
                    execution_frame: 8292,
                    paused: false,
                    actual_feed: None,
                    source_feed_context: super::super::source_feed_history::SourceFeedContext::new(
                        ProductiveSourceBinding::new(
                            feed.sample,
                            feed.sample_rate_hz,
                            feed.accepted
                        ),
                        feed.plan
                            .required_read_sets(StemReadSets::new(feed.stems, feed.outgoing_stems)),
                    ),
                },
            ),
            Some((epoch, 1))
        );
        assert!(processor.request_pending);
        assert_eq!(processor.prospective_request, Some(1));
    }

    #[test]
    fn paused_prospective_candidate_survives_idle_poll_and_cancel_allows_fresh_work() {
        use super::super::prepared_native_history::tests::fixture;
        use super::super::source_feed_history::{SourceFeedContext, SourceFeedHistory};
        use super::super::source_reader::ExplicitSeekMode;
        use std::sync::{Arc, atomic::AtomicU64};
        use std::time::{Duration, Instant};

        let mut processor = StretchProcessor::new(1);
        let mut source = fixture(1, 48_000, 1.0, ExplicitSeekMode::Normal, None);
        let mut history = SourceFeedHistory::new();
        const ACTIVE_FRAMES: usize = 32_769;
        history.record_success(
            source.playback,
            source.plan,
            SourceFeedContext::new(
                source.binding,
                source
                    .plan
                    .required_read_sets(StemReadSets::new(source.stems.as_ref(), None)),
            ),
            ACTIVE_FRAMES,
        );
        assert!(source.playback.advance(ACTIVE_FRAMES));
        source.playback.chunk(DEFAULT_BLOCK_SAMPLES);
        let position = source.playback.position();
        source.plan.frame_pos = position.frame;
        source.plan.seek_mode = position.seek_mode;
        let feed = ProductiveSourceFeed {
            pitch_intent: PitchIntent::new(3, 0).unwrap(),
            key_lock: false,
            sample: &source.sample,
            stems: None,
            outgoing_stems: None,
            sample_rate_hz: 48_000,
            accepted: None,
            plan: source.plan,
            playback: &source.playback,
            permit: Some(&source.permit),
            output_frame: Some(100),
        };
        let lifetime = Arc::new(AtomicU64::new(1));
        processor.hold_prospective_tuple();
        let first_epoch = processor.preparation_epoch;
        assert_eq!(
            processor.request_prospective_tuple_with_history(
                &feed,
                ProspectiveSourceHistory {
                    history: &history,
                    voice_lifetime: &lifetime,
                    voice_generation: 1,
                    execution_frame: 8292,
                    paused: true,
                    actual_feed: None,
                    source_feed_context: super::super::source_feed_history::SourceFeedContext::new(
                        ProductiveSourceBinding::new(
                            feed.sample,
                            feed.sample_rate_hz,
                            feed.accepted
                        ),
                        feed.plan
                            .required_read_sets(StemReadSets::new(feed.stems, feed.outgoing_stems)),
                    ),
                },
            ),
            Some((first_epoch, 1))
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while !processor.prospective_work_ready() && Instant::now() < deadline {
            processor.retire_prepared();
            std::thread::sleep(Duration::from_millis(1));
        }
        let candidate = processor.preparation.prepared_state().unwrap();
        let address = candidate.rubberband.as_ref().unwrap().state_address();
        let fifo = (
            candidate.input_fifo[0].len(),
            candidate.output_fifo[0].len(),
        );
        assert_eq!(candidate.source.as_ref().unwrap().target_output_frame, 8292);
        assert!(
            candidate
                .source
                .as_ref()
                .unwrap()
                .playback
                .matches_exact(feed.playback)
        );
        for _ in 0..8 {
            processor.retire_prepared();
            let held = processor.preparation.prepared_state().unwrap();
            assert_eq!(held.rubberband.as_ref().unwrap().state_address(), address);
            assert_eq!((held.input_fifo[0].len(), held.output_fifo[0].len()), fifo);
        }
        assert!(processor.source_history_work_available());

        processor.invalidate_prepared();
        assert!(processor.prospective_request.is_none());
        assert!(processor.preparation.prepared_state().is_none());
        processor.hold_prospective_tuple();
        let retry_epoch = processor.preparation_epoch;
        assert_eq!(
            processor.request_prospective_tuple_with_history(
                &feed,
                ProspectiveSourceHistory {
                    history: &history,
                    voice_lifetime: &lifetime,
                    voice_generation: 1,
                    execution_frame: 12_388,
                    paused: true,
                    actual_feed: None,
                    source_feed_context: super::super::source_feed_history::SourceFeedContext::new(
                        ProductiveSourceBinding::new(
                            feed.sample,
                            feed.sample_rate_hz,
                            feed.accepted
                        ),
                        feed.plan
                            .required_read_sets(StemReadSets::new(feed.stems, feed.outgoing_stems)),
                    ),
                },
            ),
            Some((retry_epoch, 2))
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while !processor.prospective_work_ready() && Instant::now() < deadline {
            processor.retire_prepared();
            std::thread::sleep(Duration::from_millis(1));
        }
        let retry = processor.preparation.prepared_state().unwrap();
        let retried_source = retry.source.as_ref().unwrap();
        assert_eq!(retried_source.request_id, 2);
        assert_eq!(retried_source.target_output_frame, 12_388);
        assert!(retried_source.playback.matches_exact(feed.playback));
        processor.invalidate_prepared();
    }

    #[test]
    fn pitch_scale_tracks_inverse_tempo_ratio() {
        assert!((pitch_scale_for_tempo_ratio(2.0) - 0.5).abs() < f64::EPSILON);
        assert!((pitch_scale_for_tempo_ratio(0.5) - 2.0).abs() < f64::EPSILON);
        assert!((pitch_scale_for_tempo_ratio(1.0) - 1.0).abs() < f64::EPSILON);
        assert!((pitch_scale_for_tempo_ratio(f64::NAN) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn live_processor_applies_binary64_inverse_rate_to_native_pitch() {
        let mut processor = StretchProcessor::new(1);
        let frames = processor.rubberband_block_size().min(DEFAULT_BLOCK_SAMPLES);
        for ratio in [1.371234567890123_f64, 1.3976543210987654_f64] {
            let expected = 1.0 / ratio;
            assert_ne!(expected, f64::from(1.0_f32 / ratio as f32));
            processor.resampled_buffers_mut(frames)[0][..frames].fill(0.0);
            processor.process_resampled(frames, ratio, true);
            assert_eq!(processor.native.pitch_scale, expected);
            assert_eq!(
                processor.native.rubberband.as_ref().unwrap().pitch_scale(),
                expected
            );
            assert!(
                processor.output_buffers()[0][..frames]
                    .iter()
                    .all(|value| value.is_finite())
            );
        }
    }

    #[test]
    fn neutral_key_lock_is_transparent() {
        let mut processor = StretchProcessor::new(1);
        let input = processor.resampled_buffers_mut(256);
        for (index, sample) in input[0].iter_mut().take(256).enumerate() {
            *sample = (index as f32 * 0.01).sin();
        }

        processor.process_resampled(256, 1.0, true);
        let output = &processor.output_buffers()[0][..256];

        for (index, sample) in output.iter().enumerate() {
            let expected = (index as f32 * 0.01).sin();
            assert!((*sample - expected).abs() < 1.0e-6);
        }
    }

    #[test]
    fn key_lock_off_copies_already_resampled_input() {
        let mut processor = StretchProcessor::new(1);
        let input = processor.resampled_buffers_mut(512);
        for (index, sample) in input[0].iter_mut().take(512).enumerate() {
            *sample = index as f32 * 2.0;
        }

        processor.process_resampled(512, 2.0, false);

        assert_eq!(
            &processor.output_buffers()[0][..512],
            &processor.varispeed_buffers()[0][..512]
        );
        assert_eq!(processor.output_buffers()[0][511], 1022.0);
    }

    #[test]
    fn unavailable_rubberband_output_uses_silence_fallback() {
        let mut processor = StretchProcessor::new(1);
        let output_samples = processor
            .rubberband_block_size()
            .saturating_sub(1)
            .clamp(1, DEFAULT_BLOCK_SAMPLES);
        let input = processor.resampled_buffers_mut(output_samples);
        for sample in input[0].iter_mut().take(output_samples) {
            *sample = 0.5;
        }

        processor.process_resampled(output_samples, 2.0, true);

        assert!(
            processor.output_buffers()[0][..output_samples]
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }

    #[test]
    fn reset_clears_pending_rubberband_output() {
        let mut processor = StretchProcessor::new(1);
        let block_size = processor.rubberband_block_size().min(DEFAULT_BLOCK_SAMPLES);
        for chunk in 0..4 {
            let input = processor.resampled_buffers_mut(block_size);
            for (index, sample) in input[0].iter_mut().take(block_size).enumerate() {
                *sample = ((chunk * block_size + index) as f32 * 0.031).sin();
            }
            processor.process_resampled(block_size, 2.0, true);
        }

        processor.reset();

        let output_samples = block_size.saturating_sub(1).max(1);
        let input = processor.resampled_buffers_mut(output_samples);
        for sample in input[0].iter_mut().take(output_samples) {
            *sample = 0.5;
        }
        processor.process_resampled(output_samples, 2.0, true);

        assert!(
            processor.output_buffers()[0][..output_samples]
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }

    #[test]
    fn rubberband_key_lock_reduces_varispeed_pitch_shift() {
        let sample_rate_hz = 48_000.0;
        let input_hz = 440.0;
        let mut processor = StretchProcessor::new(1);
        let mut varispeed = Vec::new();
        let mut locked = Vec::new();

        // The source reader has already applied a 2x source rate to this immutable feed.
        let feed = (0..48 * 512)
            .map(|output_frame| {
                let source_frame = output_frame as f64 * 2.0;
                (source_frame * f64::from(input_hz) * std::f64::consts::TAU
                    / f64::from(sample_rate_hz))
                .sin() as f32
            })
            .collect::<Vec<_>>();
        for input in feed.chunks(512) {
            processor.resampled_buffers_mut(input.len())[0][..input.len()].copy_from_slice(input);
            processor.process_resampled(input.len(), 2.0, false);
            varispeed.extend_from_slice(&processor.output_buffers()[0][..input.len()]);
        }

        processor.reset();
        for input in feed.chunks(512) {
            processor.resampled_buffers_mut(input.len())[0][..input.len()].copy_from_slice(input);
            processor.process_resampled(input.len(), 2.0, true);
            locked.extend_from_slice(&processor.output_buffers()[0][..input.len()]);
        }

        let skip = processor.rubberband_start_delay() + processor.rubberband_block_size() * 2;
        let varispeed_hz = estimate_frequency(&varispeed[skip..], sample_rate_hz);
        let locked_hz = estimate_frequency(&locked[skip..], sample_rate_hz);

        assert!(varispeed_hz > 800.0, "varispeed_hz={varispeed_hz}");
        assert!((360.0..560.0).contains(&locked_hz), "locked_hz={locked_hz}");
    }

    #[test]
    fn rubberband_key_lock_renders_finite_output() {
        let mut processor = StretchProcessor::new(1);
        for chunk in 0..8 {
            let input = processor.resampled_buffers_mut(512);
            for (index, sample) in input[0].iter_mut().take(512).enumerate() {
                let phase = (chunk * 512 + index) as f32 * 1.5 * 0.031;
                *sample = phase.sin() * 0.5;
            }

            processor.process_resampled(512, 1.5, true);

            assert!(
                processor.output_buffers()[0][..512]
                    .iter()
                    .all(|sample| sample.is_finite())
            );
        }
    }

    fn shifted_sequence(partitions: &[usize], total_frames: usize) -> Vec<f32> {
        let mut processor = StretchProcessor::new(1);
        assert_eq!(
            processor.adapter_delay_frames(),
            processor.rubberband_block_size() - 1
        );
        let feed = (0..total_frames)
            .map(|position| {
                let signal = (position as f32 * 0.057).sin() * 0.2;
                if position == 2000 {
                    signal + 0.7
                } else {
                    signal
                }
            })
            .collect::<Vec<_>>();
        let mut output = Vec::with_capacity(total_frames);
        let mut cursor = 0;
        let mut partition = 0;
        while cursor < total_frames {
            let frames = partitions[partition % partitions.len()].min(total_frames - cursor);
            processor.resampled_buffers_mut(frames)[0][..frames]
                .copy_from_slice(&feed[cursor..cursor + frames]);
            processor.process_resampled(frames, 2.0, true);
            output.extend_from_slice(&processor.output_buffers()[0][..frames]);
            // Output always drains completely; no segment can insert extra silence/latency.
            let pending_input = processor.native.input_fifo[0].len();
            let pending_output = processor.native.output_fifo[0].len();
            assert_eq!(
                pending_input + pending_output,
                processor.adapter_delay_frames()
            );
            cursor += frames;
            partition += 1;
        }
        output
    }

    #[test]
    fn fixed_adapter_delay_preserves_output_under_irregular_partitions() {
        let regular = shifted_sequence(&[512], 24_000);
        assert!(regular.iter().any(|sample| sample.abs() > 0.05));
        for partitions in [&[64][..], &[128][..], &[1, 127, 384, 96, 257, 512, 31][..]] {
            let irregular = shifted_sequence(partitions, regular.len());
            assert_eq!(
                regular, irregular,
                "adapter changed output for {partitions:?}"
            );
        }
    }

    #[test]
    fn exhausted_preparation_retains_dirty_state_and_silences_only_shifted_audio() {
        let mut processor = StretchProcessor::new(1);
        // Stop the owned test worker outside rendering. Exactly one startup reserve remains.
        drop(processor._preparation_worker.take());
        processor.resampled_buffers_mut(512)[0][..512].fill(0.5);
        processor.process_resampled(512, 2.0, true);
        processor.reset();
        processor.resampled_buffers_mut(512)[0][..512].fill(0.5);
        processor.process_resampled(512, 2.0, true);
        assert!(!processor.native.dirty);
        assert!(processor.native.used);

        processor.reset();
        for _ in 0..4 {
            processor.resampled_buffers_mut(512)[0][..512].fill(0.5);
            processor.process_resampled(512, 2.0, true);
            assert!(processor.native.dirty);
            assert!(!processor.native.active);
            assert!(processor.native.rubberband.is_some());
            assert!(
                processor.output_buffers()[0][..512]
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
        }
        processor.process_resampled(512, 2.0, false);
        assert!(
            processor.output_buffers()[0][..512]
                .iter()
                .all(|sample| *sample == 0.5)
        );
        assert!(processor.native.dirty);
    }

    #[test]
    fn unused_warm_state_survives_repeated_start_invalidations() {
        let mut processor = StretchProcessor::new(1);
        for _ in 0..10 {
            processor.reset();
            assert!(!processor.native.dirty);
        }
        processor.resampled_buffers_mut(512)[0][..512].fill(0.2);
        processor.process_resampled(512, 2.0, true);
        assert!(processor.native.used);
        assert!(!processor.native.dirty);
    }
}
