//! Worker-only raw suffix preparation with separate logical and actual-feed clocks.
//!
//! A raw discard is a declared stream coordinate, never a selected musical attack anchor.
//! The adapter contributes B-1 frames before raw Native output. Feeding and discarding
//! D+B-1 adapter frames therefore leaves raw Native frame D next in the same FIFO owner.

use super::{
    MappedNativeInput, MappedNativePitchChunk, NativeAdapterState, NativeHistoryRequest,
    NativeSourcePreparationError, NativeStartHistoryContext, RetainedNativeStartCut,
    RubberBandError, SourceReadCheckpoint,
};
use crate::audio_engine::source_feed_history::SOURCE_FEED_HISTORY_MAX_FRAMES;
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::source_reader::SourceReadPlan;
use crate::messages::PreparedStemSet;
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub(super) struct RetainedNativeStartTimeline {
    logical: SourceReadCheckpoint,
    actual: SourceReadCheckpoint,
    provenance: RetainedFeedProvenance,
    valid: bool,
}

/// A raw preparation receipt remains attached even if a later claimed control change
/// establishes a new continuation epoch. Replay never invents these raw coordinates.
#[derive(Clone, Copy, Debug)]
struct RawStartProvenance {
    cut: RetainedNativeStartCut,
    input_origin: SourceReadCheckpoint,
    silent_prefix: usize,
    emitted_output_frames: usize,
    endpoint: SourceReadCheckpoint,
}

#[derive(Clone, Copy, Debug)]
struct RetainedFeedProvenance {
    raw: Option<RawStartProvenance>,
    continuation: Option<RetainedContinuation>,
}

#[derive(Clone, Copy, Debug)]
struct RetainedContinuation {
    logical_origin: SourceReadCheckpoint,
    actual_origin: SourceReadCheckpoint,
    emitted_output_frames: u64,
}

struct RetainedRateUpdate {
    previous_logical: SourceReadCheckpoint,
    previous_actual: SourceReadCheckpoint,
    logical: SourceReadCheckpoint,
    actual: SourceReadCheckpoint,
}

/// Declared stream coordinates only; these never certify intended-output alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RetainedMappedOutputCoordinates {
    pub(crate) logical_output_frame: u64,
    pub(crate) raw_output_frame: u64,
    pub(crate) actual_input_frame: u64,
}

fn project_checkpoint(
    checkpoint: &mut SourceReadCheckpoint,
    frames: u64,
    mapped: Option<&MappedNativeInput>,
) -> bool {
    if let Some(mapped) = mapped {
        mapped.project(checkpoint, frames).is_ok()
    } else {
        checkpoint.project_output_frames(frames)
    }
}

fn same_checkpoint(
    left: SourceReadCheckpoint,
    right: SourceReadCheckpoint,
    mapped: Option<&MappedNativeInput>,
) -> bool {
    mapped.map_or_else(
        || left.matches_exact(right),
        |mapped| mapped.same_checkpoint(left, right),
    )
}

impl RawStartProvenance {
    fn proves_endpoint(
        self,
        block_size: usize,
        primed_output_frames: usize,
        mapped: Option<&MappedNativeInput>,
    ) -> bool {
        let Some(actual_frames) = self
            .cut
            .discarded_raw_output_frames
            .checked_add(block_size.saturating_sub(1))
            .filter(|frames| *frames == primed_output_frames)
            .and_then(|frames| frames.checked_sub(self.silent_prefix))
            .and_then(|frames| frames.checked_add(self.emitted_output_frames))
        else {
            return false;
        };
        let mut expected = self.input_origin;
        project_checkpoint(&mut expected, actual_frames as u64, mapped)
            && same_checkpoint(expected, self.endpoint, mapped)
    }
}

impl RetainedNativeStartTimeline {
    fn proves_endpoints(
        self,
        block_size: usize,
        primed_output_frames: usize,
        mapped: Option<&MappedNativeInput>,
    ) -> bool {
        if !self.valid {
            return false;
        }
        if !self
            .provenance
            .raw
            .is_none_or(|raw| raw.proves_endpoint(block_size, primed_output_frames, mapped))
        {
            return false;
        }
        if let Some(mapped) = mapped {
            let Some(raw) = self.provenance.raw else {
                return false;
            };
            let mut expected_logical = raw.input_origin;
            if !project_checkpoint(
                &mut expected_logical,
                raw.emitted_output_frames as u64,
                Some(mapped),
            ) || !same_checkpoint(expected_logical, self.logical, Some(mapped))
            {
                return false;
            }
        }
        match self.provenance.continuation {
            None => self
                .provenance
                .raw
                .is_some_and(|raw| same_checkpoint(raw.endpoint, self.actual, mapped)),
            Some(RetainedContinuation {
                mut logical_origin,
                mut actual_origin,
                emitted_output_frames,
            }) => {
                project_checkpoint(&mut logical_origin, emitted_output_frames, mapped)
                    && project_checkpoint(&mut actual_origin, emitted_output_frames, mapped)
                    && same_checkpoint(logical_origin, self.logical, mapped)
                    && same_checkpoint(actual_origin, self.actual, mapped)
            }
        }
    }
}

fn unavailable() -> NativeSourcePreparationError {
    NativeSourcePreparationError::Unavailable(RubberBandError::UnavailableSourceHistory)
}

impl NativeAdapterState {
    fn mapped_input(&self) -> Option<&MappedNativeInput> {
        self.source
            .as_ref()
            .and_then(|source| source.mapped_input.as_deref())
    }

    pub(crate) fn retained_mapped_output_coordinates(
        &self,
    ) -> Result<Option<RetainedMappedOutputCoordinates>, RubberBandError> {
        let Some(mapped) = self.mapped_input() else {
            return Ok(None);
        };
        let timeline = self
            .retained_start_timeline
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        if !timeline.proves_endpoints(self.block_size, self.primed_output_frames, Some(mapped)) {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        let raw = timeline
            .provenance
            .raw
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        let logical_output_frame = timeline
            .logical
            .playback
            .mapped_output_frame()
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        let actual_input_frame = timeline
            .actual
            .playback
            .mapped_output_frame()
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        let raw_output_frame = u64::try_from(raw.cut.discarded_raw_output_frames)
            .ok()
            .and_then(|frame| frame.checked_add(raw.emitted_output_frames as u64))
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        Ok(Some(RetainedMappedOutputCoordinates {
            logical_output_frame,
            raw_output_frame,
            actual_input_frame,
        }))
    }

    /// The read receipt names actual input; the caller still emits at the distinct logical n.
    pub(crate) fn owned_mapped_read_ready(
        &self,
        logical: &SourcePlayback,
        plan: SourceReadPlan,
        output_frame: Option<u64>,
        frames: usize,
    ) -> bool {
        let Some(mapped) = self.mapped_input() else {
            return true;
        };
        let Some(timeline) = self.retained_start_timeline else {
            return false;
        };
        let Ok(Some(coordinates)) = self.retained_mapped_output_coordinates() else {
            return false;
        };
        let mut next_logical = SourceReadCheckpoint {
            playback: *logical,
            plan,
        };
        let mut next_actual = timeline.actual;
        output_frame == Some(coordinates.logical_output_frame)
            && coordinates
                .logical_output_frame
                .checked_add(frames as u64)
                .is_some()
            && coordinates
                .actual_input_frame
                .checked_add(frames as u64)
                .is_some()
            && coordinates
                .raw_output_frame
                .checked_add(frames as u64)
                .is_some()
            && self
                .retained_start_feed_chunk_limit(logical, plan, frames)
                .is_ok_and(|bound| bound == frames)
            // An exclusive span endpoint alone does not prove the next canonical
            // read state is representable. Fail before touching Native or FIFOs.
            && mapped.project(&mut next_logical, frames as u64).is_ok()
            && mapped.project(&mut next_actual, frames as u64).is_ok()
    }

    pub(crate) fn has_owned_mapped_input(&self) -> bool {
        self.mapped_input().is_some()
    }

    /// Called after the existing complete live source contract check and before rendering.
    /// This describes the retained AHEAD input, never the independently advancing logical n.
    pub(crate) fn live_mapped_pitch_chunk(
        &self,
        actual: SourceReadCheckpoint,
        frames: usize,
        intent: crate::audio_engine::pitch_intent::PitchIntent,
        key_lock: bool,
    ) -> Result<Option<MappedNativePitchChunk>, RubberBandError> {
        let Some(mapped) = self.mapped_input() else {
            return Ok(None);
        };
        let source = self
            .source
            .as_ref()
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        let timeline = self
            .retained_start_timeline
            .ok_or(RubberBandError::UnavailableSourceHistory)?;
        if self.effective_key_lock() != Some(key_lock)
            || source.pitch_intent != intent
            || !mapped.canonicalize(timeline.actual)?.matches_exact(actual)
            || !timeline.proves_endpoints(self.block_size, self.primed_output_frames, Some(mapped))
            || self
                .mapped_pitch
                .as_ref()
                .is_none_or(|state| state.owner != mapped.pitch_owner())
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        mapped
            .pitch_chunk(
                source,
                actual,
                frames,
                intent,
                key_lock,
                self.mapped_pitch_stamp()
                    .ok_or(RubberBandError::UnavailableSourceHistory)?,
            )
            .map(Some)
    }

    pub(crate) fn has_retained_feed(&self) -> bool {
        self.retained_start_timeline.is_some()
    }

    /// Reader retirement follows the actual endpoint, including a paused or lagging ramp.
    /// An invalid mapping cannot grant permission to release any potentially used set.
    pub(crate) fn retained_stem_reader_required(&self, stems: &PreparedStemSet) -> bool {
        let Some(timeline) = self.retained_start_timeline else {
            return false;
        };
        if !timeline.proves_endpoints(
            self.block_size,
            self.primed_output_frames,
            self.mapped_input(),
        ) || self.effective_stem_read_context.is_some_and(|context| {
            !context.plan.matches_source_contract(timeline.actual.plan)
                || self.source_transition.is_none_or(|transition| {
                    !transition.matches_exact(timeline.actual.plan.transition)
                })
        }) {
            return true;
        }
        let owner = Arc::as_ptr(&stems.complete_set_identity) as usize;
        std::iter::once(timeline.actual.plan.selection)
            .chain(timeline.actual.plan.transition.required_from_selection())
            .any(|selection| selection.matches_stem_descriptor(owner, stems.source_version_hash))
    }

    fn retained_rate_update(
        &self,
        target: f64,
    ) -> Result<Option<RetainedRateUpdate>, RubberBandError> {
        let Some(timeline) = self.retained_start_timeline else {
            return Ok(None);
        };
        if !target.is_finite()
            || target <= 0.0
            || !self.active
            || !self.used
            || self.dirty
            || !timeline.proves_endpoints(
                self.block_size,
                self.primed_output_frames,
                self.mapped_input(),
            )
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        let mut logical = timeline.logical;
        let mut actual = timeline.actual;
        if !logical.playback.set_target(target)
            || !actual.playback.set_target(target)
            || !logical.project_output_frames(0)
            || !actual.project_output_frames(0)
            || !logical.is_consistent()
            || !actual.is_consistent()
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        Ok(Some(RetainedRateUpdate {
            previous_logical: timeline.logical,
            previous_actual: timeline.actual,
            logical,
            actual,
        }))
    }

    pub(crate) fn can_retarget_retained_rate(&self, target: f64) -> bool {
        self.retained_rate_update(target).is_ok()
    }

    /// A bounded global preflight has validated every participant before any rate changes.
    /// Recompute from unchanged callback-owned state; there are no source/permit lookups here.
    pub(crate) fn retarget_retained_rate_claimed(&mut self, target: f64) {
        let Some(update) = self
            .retained_rate_update(target)
            .expect("preflighted callback-owned retained rate")
        else {
            return;
        };
        self.reconfigure_retained_feed_endpoint_claimed(
            update.previous_logical,
            update.previous_actual,
            update.logical,
            update.actual,
        )
        .expect("unchanged preflighted retained endpoints");
    }

    /// Prepare a declared raw suffix off realtime, preserving its exact Native/FIFO owner.
    /// None is the previous endpoint contract: H input frames and no source-feed offset.
    pub(crate) fn prepare_retained_source_start_checked(
        &mut self,
        request: NativeHistoryRequest,
        declared_cut: Option<RetainedNativeStartCut>,
    ) -> Result<(), NativeSourcePreparationError> {
        self.prepare_retained_source_start_with_catchup_checked(request, declared_cut, 0)
    }

    pub(crate) fn prepare_retained_source_start_with_catchup_checked(
        &mut self,
        request: NativeHistoryRequest,
        declared_cut: Option<RetainedNativeStartCut>,
        catchup_frames: u64,
    ) -> Result<(), NativeSourcePreparationError> {
        self.prepare_retained_source_start_with_catchup_and_dsp_checked(
            request,
            declared_cut,
            catchup_frames,
            None,
        )
    }

    pub(crate) fn prepare_retained_source_start_with_catchup_and_dsp_checked(
        &mut self,
        request: NativeHistoryRequest,
        declared_cut: Option<RetainedNativeStartCut>,
        catchup_frames: u64,
        dsp: Option<&mut crate::audio_engine::late_start_dsp::LateStartDspWork>,
    ) -> Result<(), NativeSourcePreparationError> {
        self.prepare_retained_source_start_context_checked(
            request,
            declared_cut,
            catchup_frames,
            dsp,
            NativeStartHistoryContext::SourceHistory,
        )
    }

    pub(super) fn prepare_retained_source_start_context_checked(
        &mut self,
        request: NativeHistoryRequest,
        declared_cut: Option<RetainedNativeStartCut>,
        catchup_frames: u64,
        dsp: Option<&mut crate::audio_engine::late_start_dsp::LateStartDspWork>,
        context: NativeStartHistoryContext,
    ) -> Result<(), NativeSourcePreparationError> {
        self.prepare_retained_source_start_context_inner(
            request,
            declared_cut,
            catchup_frames,
            dsp,
            context,
            false,
        )
    }

    /// Internal declared-stream experiment only. This supplies no productive output policy.
    /// The request's immutable original-map capability retires with its existing source pins.
    pub(crate) fn prepare_owned_mapped_birth_future(
        &mut self,
        request: NativeHistoryRequest,
        cut: RetainedNativeStartCut,
    ) -> Result<(), NativeSourcePreparationError> {
        self.prepare_retained_source_start_context_inner(
            request,
            Some(cut),
            0,
            None,
            NativeStartHistoryContext::PhysicalBirth,
            true,
        )
    }

    fn prepare_retained_source_start_context_inner(
        &mut self,
        mut request: NativeHistoryRequest,
        declared_cut: Option<RetainedNativeStartCut>,
        catchup_frames: u64,
        dsp: Option<&mut crate::audio_engine::late_start_dsp::LateStartDspWork>,
        context: NativeStartHistoryContext,
        allow_owned_mapped_future: bool,
    ) -> Result<(), NativeSourcePreparationError> {
        if request.plan.loop_period.is_mapped()
            || request.playback.has_phase_program()
            || request.playback.is_general_mapped()
            || request.actual_feed.is_some_and(|checkpoint| {
                checkpoint.plan.loop_period.is_mapped()
                    || checkpoint.playback.is_general_mapped()
                    || checkpoint.playback.has_phase_program()
            })
        {
            return Err(unavailable());
        }
        let mapped = request.playback.is_mapped()
            || request
                .actual_feed
                .is_some_and(|checkpoint| checkpoint.playback.is_mapped());
        let owned_future = allow_owned_mapped_future
            && mapped
            && request
                .mapped_input
                .as_ref()
                .is_some_and(|input| input.matches_request(&request) && input.pending_current())
            && request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz);
        if allow_owned_mapped_future != owned_future
            || (request.mapped_input.is_some() && !owned_future)
            || request.plan.transition.is_active() && owned_future
        {
            return Err(unavailable());
        }
        if mapped
            && (context != NativeStartHistoryContext::PhysicalBirth
                || (!owned_future
                    && (request.key_lock || request.pitch_intent.total_semitones() == 0))
                || (declared_cut.is_some() && !owned_future)
                || catchup_frames != 0
                || dsp.is_some()
                || request.actual_feed.is_some())
        {
            return Err(unavailable());
        }
        if declared_cut.is_some() && catchup_frames != 0 {
            return Err(unavailable());
        }
        if context == NativeStartHistoryContext::PhysicalBirth
            && ((declared_cut.is_some() && !owned_future) || catchup_frames != 0 || dsp.is_some())
        {
            return Err(unavailable());
        }
        let endpoint = request.playback;
        let endpoint_plan = request.plan;
        if endpoint.tempo_ratio().to_bits() != endpoint.rate_target().to_bits()
            || (declared_cut.is_some() && endpoint_plan.transition.is_active())
        {
            return Err(unavailable());
        }
        let pitch = request
            .pitch_intent
            .native_pitch_scale(endpoint.tempo_ratio(), request.key_lock)
            .map_err(|_| {
                NativeSourcePreparationError::Unavailable(RubberBandError::InvalidPitchScale)
            })?;
        if owned_future {
            self.begin_mapped_preparation()?;
        }
        let native = self
            .rubberband
            .as_mut()
            .ok_or(NativeSourcePreparationError::Backend(
                RubberBandError::ConstructionFailed,
            ))?;
        native
            .prepare_exact_pitch(pitch)
            .map_err(NativeSourcePreparationError::Backend)?;
        let minimum_history = native
            .source_history_frames(pitch)
            .map_err(NativeSourcePreparationError::from_history)?;
        let adapter_lead = self.block_size.saturating_sub(1);
        let cut = if let Some(cut) = declared_cut {
            cut
        } else {
            // source_history_frames includes the actual pitch/reset resampler delay.
            // Derive the extended origin only after that exact preparation above.
            let history = super::launch_history_output_frames(native, &request, catchup_frames)
                .map_err(NativeSourcePreparationError::from_history)?;
            RetainedNativeStartCut {
                history_output_frames: history,
                discarded_raw_output_frames: history
                    .checked_sub(adapter_lead)
                    .ok_or_else(unavailable)?,
            }
        };
        let feed_frames = cut
            .discarded_raw_output_frames
            .checked_add(adapter_lead)
            .filter(|frames| *frames <= SOURCE_FEED_HISTORY_MAX_FRAMES)
            .ok_or_else(unavailable)?;
        if cut.history_output_frames < minimum_history
            || cut.history_output_frames > SOURCE_FEED_HISTORY_MAX_FRAMES
            || feed_frames < cut.history_output_frames
        {
            return Err(unavailable());
        }
        if let Some(input) = &request.mapped_input {
            let mut future = SourceReadCheckpoint {
                playback: endpoint,
                plan: endpoint_plan,
            };
            input
                .project(
                    &mut future,
                    (feed_frames - cut.history_output_frames) as u64,
                )
                .map_err(NativeSourcePreparationError::from_history)?;
        }
        let (source_rebase, history, silent_prefix) = match context {
            NativeStartHistoryContext::SourceHistory => {
                let rebase = super::launch_source_rebase(
                    &request,
                    cut.history_output_frames,
                    catchup_frames,
                )
                .map_err(NativeSourcePreparationError::from_history)?;
                let (history, silent) = endpoint
                    .history_before_constant(cut.history_output_frames)
                    .ok_or_else(unavailable)?;
                (rebase, history, silent)
            }
            NativeStartHistoryContext::PhysicalBirth => {
                // Before this attack there was no input from this new physical source.
                // The existing silence path consumes Native frames without moving its
                // source/rate checkpoint; no virtual loop tail enters the new owner.
                (None, endpoint, cut.history_output_frames)
            }
        };
        request.playback = history;
        let position = history.position();
        request.plan.frame_pos = position.frame;
        request.plan.seek_mode = position.seek_mode;
        // This owner receives its own declared source or silent-birth context.
        // A prior owner's offset cannot describe its new input chronology.
        request.actual_feed = None;
        let input_origin = SourceReadCheckpoint {
            playback: request.playback,
            plan: request.plan,
        };
        // The existing renderer consumes its one adapter lead here; its live continuation
        // inherits the remaining real raw suffix instead of constructing another prefix.
        let dsp = if let Some(dsp) = dsp {
            let catchup = usize::try_from(catchup_frames).map_err(|_| unavailable())?;
            Some((
                cut.history_output_frames
                    .checked_sub(catchup)
                    .ok_or_else(unavailable)?,
                dsp,
            ))
        } else {
            None
        };
        self.prepare_source_with_silence(
            request,
            silent_prefix,
            feed_frames,
            true,
            source_rebase,
            dsp,
        )?;
        let source = self
            .source
            .as_mut()
            .expect("prepared raw suffix retains its original source owner");
        let actual = SourceReadCheckpoint {
            playback: source.playback,
            plan: source.plan,
        };
        let logical = SourceReadCheckpoint {
            playback: endpoint,
            plan: endpoint_plan,
        };
        source.playback = endpoint;
        source.plan = endpoint_plan;
        source.actual_feed = (feed_frames != cut.history_output_frames).then_some(actual);
        self.retained_start_timeline = (feed_frames != cut.history_output_frames || owned_future)
            .then_some(RetainedNativeStartTimeline {
                logical,
                actual,
                provenance: RetainedFeedProvenance {
                    raw: Some(RawStartProvenance {
                        cut,
                        input_origin,
                        silent_prefix,
                        emitted_output_frames: 0,
                        endpoint: actual,
                    }),
                    continuation: None,
                },
                valid: true,
            });
        self.record_input_history_context(context);
        Ok(())
    }

    /// Observe the actual next read only for the same unchanged logical/control trajectory.
    /// New controls over already processed future material require a fresh prepared owner.
    pub(crate) fn retained_start_feed(
        &self,
        logical: &SourcePlayback,
        logical_plan: SourceReadPlan,
    ) -> Result<Option<(SourcePlayback, SourceReadPlan)>, RubberBandError> {
        let Some(timeline) = self.retained_start_timeline else {
            return Ok(None);
        };
        let mapped = self.mapped_input();
        if !same_checkpoint(
            timeline.logical,
            SourceReadCheckpoint {
                playback: *logical,
                plan: logical_plan,
            },
            mapped,
        ) || !timeline.proves_endpoints(self.block_size, self.primed_output_frames, mapped)
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        let actual = if let Some(mapped) = mapped {
            mapped.canonicalize(timeline.actual)?
        } else {
            timeline.actual
        };
        Ok(Some((actual.playback, actual.plan)))
    }

    /// Bound one actual read before either copied smoothing clock crosses a rate step.
    /// The mixer still owns its ordinary logical chunk; this adds the Native-input bound.
    pub(crate) fn retained_start_feed_chunk_limit(
        &self,
        logical: &SourcePlayback,
        logical_plan: SourceReadPlan,
        max_frames: usize,
    ) -> Result<usize, RubberBandError> {
        let Some((mut actual, _)) = self.retained_start_feed(logical, logical_plan)? else {
            return Ok(max_frames);
        };
        let mut logical = SourceReadCheckpoint {
            playback: *logical,
            plan: logical_plan,
        };
        if let Some(mapped) = self.mapped_input() {
            logical = mapped.canonicalize(logical)?;
        }
        let logical_frames = logical.playback.chunk(max_frames).0;
        Ok(actual.chunk(logical_frames).0)
    }

    /// Worker-only: install the two endpoints after genuine source replay succeeded.
    /// The request retains logical adoption authority; a differing actual input endpoint
    /// requires this mapping instead of relabeling the prepared FIFO as logical input.
    pub(crate) fn install_replayed_feed_endpoint(
        &mut self,
        logical: SourceReadCheckpoint,
        actual: SourceReadCheckpoint,
    ) -> Result<(), RubberBandError> {
        if logical.playback.is_mapped()
            || actual.playback.is_mapped()
            || logical.playback.has_phase_program()
            || actual.playback.has_phase_program()
            || !logical.is_consistent()
            || !actual.is_consistent()
            || !self.active
            || !self.used
            || self.dirty
            || self.source.as_ref().is_none_or(|source| {
                !logical.matches_exact(SourceReadCheckpoint {
                    playback: source.playback,
                    plan: source.plan,
                }) || !actual.matches_exact(source.actual_feed_checkpoint())
            })
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        self.source_transition = actual
            .plan
            .transition
            .is_active()
            .then_some(actual.plan.transition);
        self.retained_start_timeline =
            (!logical.matches_exact(actual)).then_some(RetainedNativeStartTimeline {
                logical,
                actual,
                provenance: RetainedFeedProvenance {
                    raw: None,
                    continuation: Some(RetainedContinuation {
                        logical_origin: logical,
                        actual_origin: actual,
                        emitted_output_frames: 0,
                    }),
                },
                valid: true,
            });
        Ok(())
    }

    /// Called only after the mixer's complete claimed source/rate reconfiguration.
    /// Both old endpoints must still be exact. The caller supplies independently configured
    /// new clocks at their respective phases; ordinary controls cannot overwrite this map.
    /// Their next smoothing chunk is normalized here, exactly as before the next live read.
    /// Processed Native/FIFO data and any original raw preparation receipt stay owned here.
    pub(crate) fn reconfigure_retained_feed_endpoint_claimed(
        &mut self,
        previous_logical: SourceReadCheckpoint,
        previous_actual: SourceReadCheckpoint,
        mut logical: SourceReadCheckpoint,
        mut actual: SourceReadCheckpoint,
    ) -> Result<(), RubberBandError> {
        if logical.playback.is_mapped()
            || actual.playback.is_mapped()
            || logical.playback.has_phase_program()
            || actual.playback.has_phase_program()
            || !logical.project_output_frames(0)
            || !actual.project_output_frames(0)
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        let Some(timeline) = self.retained_start_timeline else {
            return if previous_logical.matches_exact(previous_actual)
                && logical.matches_exact(actual)
            {
                Ok(())
            } else {
                Err(RubberBandError::UnavailableSourceHistory)
            };
        };
        if !timeline.logical.matches_exact(previous_logical)
            || !timeline.actual.matches_exact(previous_actual)
            || !timeline.proves_endpoints(
                self.block_size,
                self.primed_output_frames,
                self.mapped_input(),
            )
            || !logical.is_consistent()
            || !actual.is_consistent()
        {
            return Err(RubberBandError::UnavailableSourceHistory);
        }
        let raw = timeline.provenance.raw;
        self.retained_start_timeline = Some(RetainedNativeStartTimeline {
            logical,
            actual,
            provenance: RetainedFeedProvenance {
                raw,
                continuation: Some(RetainedContinuation {
                    logical_origin: logical,
                    actual_origin: actual,
                    emitted_output_frames: 0,
                }),
            },
            valid: true,
        });
        self.source_transition = (self.effective_stem_read_context.is_some()
            || actual.plan.transition.is_active())
        .then_some(actual.plan.transition);
        Ok(())
    }

    /// Commit only after the actual read and Native/FIFO render succeeded.
    pub(crate) fn advance_retained_start_feed(&mut self, frames: usize) {
        let mapped = self
            .source
            .as_ref()
            .and_then(|source| source.mapped_input.as_deref());
        let Some(timeline) = &mut self.retained_start_timeline else {
            return;
        };
        let mut logical = timeline.logical;
        let mut actual = timeline.actual;
        let counters_fit = match timeline.provenance.continuation {
            Some(continuation) => continuation
                .emitted_output_frames
                .checked_add(frames as u64)
                .is_some(),
            None => timeline
                .provenance
                .raw
                .is_some_and(|raw| raw.emitted_output_frames.checked_add(frames).is_some()),
        };
        if !counters_fit
            || !project_checkpoint(&mut logical, frames as u64, mapped)
            || !project_checkpoint(&mut actual, frames as u64, mapped)
        {
            timeline.valid = false;
            return;
        }
        timeline.logical = logical;
        timeline.actual = actual;
        match (
            &mut timeline.provenance.continuation,
            &mut timeline.provenance.raw,
        ) {
            (None, Some(raw)) => {
                if let Some(emitted) = raw.emitted_output_frames.checked_add(frames) {
                    raw.emitted_output_frames = emitted;
                    raw.endpoint = timeline.actual;
                } else {
                    timeline.valid = false;
                }
            }
            (
                Some(RetainedContinuation {
                    emitted_output_frames,
                    ..
                }),
                _,
            ) => {
                if let Some(emitted) = emitted_output_frames.checked_add(frames as u64) {
                    *emitted_output_frames = emitted;
                } else {
                    timeline.valid = false;
                }
            }
            (None, None) => timeline.valid = false,
        }
        if let Some(context) = &mut self.effective_stem_read_context {
            context.plan = timeline.actual.plan;
            context.readers = context.readers.for_plan(timeline.actual.plan);
        }
    }
}

#[cfg(test)]
#[path = "retained_native_start_tests.rs"]
mod tests;
