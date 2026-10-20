//! Worker-only replay of genuine bounded source chronology and declared new-start history.
//!
//! A fresh reset on the declared active-feed lattice proves this finite replay,
//! not equality to a Native handle that has processed arbitrary earlier history.

use super::native_source_coverage::{
    normal_loop_feed_available_with_readers, reset_source_feed_available,
};
use super::prepared_native_history::{
    NativeAdapterState, NativeHistoryRequest, NativeStartHistoryContext, SourceReadCheckpoint,
};
use super::productive_source_history::ProductiveSourceHistory;
use super::rubberband_backend::RubberBandError;
use super::source_feed_history::{
    SOURCE_FEED_HISTORY_MAX_FRAMES, SourceFeedContext, SourceFeedSnapshot, SourceFeedSpan,
};
use super::source_playback::SourcePlayback;
use super::source_reader::StemReadSets;
use super::source_reader::{ExplicitSeekMode, SourceReadPlan};
use super::stretch_processor::DEFAULT_BLOCK_SAMPLES;

fn unavailable() -> RubberBandError {
    RubberBandError::UnavailableSourceHistory
}

/// Original, unshifted E checkpoint for a separately declared fresh Normal start.
/// The actual journal remains untouched; only worker-local input reads are derived.
#[derive(Clone, Copy)]
pub(crate) struct EndpointResetHistory {
    pub(crate) playback: SourcePlayback,
    pub(crate) plan: SourceReadPlan,
}

impl EndpointResetHistory {
    fn phase_shift(self, request: &NativeHistoryRequest) -> Result<f64, RubberBandError> {
        let readers = StemReadSets::new(request.stems.as_ref(), None);
        if !reset_source_feed_available(
            &request.sample,
            readers,
            request.binding.sample_rate_hz,
            request.binding.accepted,
            request.plan,
            &request.playback,
        ) || request.plan.seek_mode != ExplicitSeekMode::Normal
            || self.plan.seek_mode != ExplicitSeekMode::Normal
            || request.plan.transition.is_active()
            || self.plan.transition.is_active()
            || !self.plan.matches_source_contract(request.plan)
            || !self.playback.matches_domain(self.plan.domain())
        {
            return Err(unavailable());
        }
        let mut reset = self.playback;
        reset.seek(request.plan.loop_region.start, ExplicitSeekMode::Normal);
        reset.configure_domain(request.plan.domain());
        let mut plan = self.plan;
        plan.frame_pos = request.plan.loop_region.start;
        plan.seek_mode = ExplicitSeekMode::Normal;
        plan.transition.clear();
        if !reset.matches_exact(&request.playback) || !plan.matches_exact(request.plan) {
            return Err(unavailable());
        }
        let position = self.playback.position();
        if position.seek_mode != ExplicitSeekMode::Normal
            || position.frame < request.plan.loop_region.start
            || !position.fraction.is_finite()
            || !(0.0..1.0).contains(&position.fraction)
        {
            return Err(unavailable());
        }
        Ok(-((position.frame - request.plan.loop_region.start) as f64 + position.fraction))
    }
}

fn constant_chronology_matches(
    request: &NativeHistoryRequest,
    history: &SourceFeedSnapshot,
    spans: &[SourceFeedSpan],
    end: u64,
    bound: u64,
) -> bool {
    if request.plan.transition.is_active() {
        return false;
    }
    // Intentional constant starts retain their established pre-origin priming semantics.
    // Every actual read since that origin must still prove the same selection and data;
    // the source clock alone says nothing about masks, outgoing readers or refused reads.
    let start = end.saturating_sub(bound).max(history.active_origin());
    let mut next = start;
    for span in spans {
        let begin = span.active_start.max(start);
        let stop = span.active_end().min(end);
        if stop <= begin {
            continue;
        }
        if begin != next
            || span.plan.transition.is_active()
            || !span.plan.matches_source_contract(request.plan)
            || span.playback.tempo_ratio().to_bits() != request.playback.tempo_ratio().to_bits()
        {
            return false;
        }
        let Some(readers) = span
            .context
            .resolve_readers(span.plan, request.stems.as_ref())
        else {
            return false;
        };
        if !span
            .context
            .same_source(SourceFeedContext::new(request.binding, readers))
            || !normal_loop_feed_available_with_readers(
                &request.sample,
                readers,
                request.binding.sample_rate_hz,
                span.context.binding.accepted,
                span.plan,
                &span.playback,
            )
        {
            return false;
        }
        next = stop;
    }
    next == end
}

fn future_continues_actual_history(
    history: &SourceFeedSnapshot,
    future: (SourcePlayback, SourceReadPlan, u64, SourceFeedContext),
) -> bool {
    // Only the capture seam joins actual reads to planned input. Earlier recorded
    // seeks are real chronology; do not rewrite or reject them as discontinuities.
    // An empty journal has no endpoint proof: the replay coverage checks below
    // still distinguish a new lifecycle origin from missing required input.
    history.spans().next_back().is_none_or(|last| {
        last.active_end() == history.active_frames()
            && ProductiveSourceHistory {
                binding: last.context.binding,
                next_position: last.playback.position_at(last.frames),
                domain: Some(last.plan.domain()),
                fed_output_frames: last.frames as u64,
            }
            .continues(
                future.3.binding,
                future.0.position(),
                Some(future.1.domain()),
            )
    })
}

impl NativeAdapterState {
    /// Keep the established exact constant endpoint path. A non-reconstructible
    /// moving endpoint instead needs the original read spans plus frozen F-to-E.
    pub(crate) fn prepare_source_with_feed_history(
        &mut self,
        request: NativeHistoryRequest,
        history: &SourceFeedSnapshot,
        future: (SourcePlayback, SourceReadPlan, u64, SourceFeedContext),
    ) -> Result<(), RubberBandError> {
        self.prepare_source_feed_history(request, history, future, None)
    }

    /// Prepare a fresh Normal birth from proven actual rate epochs. Its pre-origin
    /// PCM is silence; shifted source phases still validate the existing coverage
    /// and endpoint proof without pretending that predecessor PCM was played by it.
    pub(crate) fn prepare_source_with_endpoint_reset_history(
        &mut self,
        request: NativeHistoryRequest,
        history: &SourceFeedSnapshot,
        future: (SourcePlayback, SourceReadPlan, u64, SourceFeedContext),
        endpoint: EndpointResetHistory,
    ) -> Result<(), RubberBandError> {
        self.prepare_source_feed_history(request, history, future, Some(endpoint))
    }

    /// Continue the already prepared original-T owner. Pre-T Native warm-up is
    /// excluded from the fresh filter; every real T-to-E output feeds that filter.
    pub(crate) fn continue_prepared_launch(
        &mut self,
        endpoint: SourceReadCheckpoint,
        frames: u64,
        dsp: &mut super::late_start_dsp::LateStartDspWork,
    ) -> Result<(), RubberBandError> {
        let count = usize::try_from(frames).map_err(|_| unavailable())?;
        let primed = self
            .primed_output_frames
            .checked_add(count)
            .filter(|frames| *frames <= SOURCE_FEED_HISTORY_MAX_FRAMES)
            .ok_or_else(unavailable)?;
        let mut request = self.source.take().ok_or_else(unavailable)?;
        let mut checkpoint = SourceReadCheckpoint {
            playback: request.playback,
            plan: request.plan,
        };
        let mut projected = checkpoint;
        if !projected.project_output_frames(frames)
            || !projected.matches_exact(endpoint)
            || self
                .retained_start_feed(&request.playback, request.plan)?
                .is_some()
        {
            return Err(unavailable());
        }
        let mut feed = vec![vec![0.0; DEFAULT_BLOCK_SAMPLES]; request.plan.channels];
        let mut output = vec![vec![0.0; DEFAULT_BLOCK_SAMPLES]; request.plan.channels];
        let mut remaining = frames;
        while remaining > 0 {
            if !request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
            {
                return Err(unavailable());
            }
            let (count, ratio) = request
                .playback
                .chunk(remaining.min(DEFAULT_BLOCK_SAMPLES as u64) as usize);
            if count == 0 {
                return Err(unavailable());
            }
            let pitch = request
                .pitch_intent
                .native_pitch_scale(ratio, request.key_lock)
                .map_err(|_| unavailable())?;
            self.render_source_history_chunk(&request, &mut feed, &mut output, count, pitch)
                .map_err(|_| unavailable())?;
            dsp.consume(&request, &output, count)?;
            checkpoint = SourceReadCheckpoint {
                playback: request.playback,
                plan: request.plan,
            };
            if !checkpoint.project_output_frames(count as u64) {
                return Err(unavailable());
            }
            request.playback = checkpoint.playback;
            request.plan = checkpoint.plan;
            remaining -= count as u64;
        }
        if !checkpoint.matches_exact(endpoint)
            || !dsp.ready()
            || !request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
        {
            return Err(unavailable());
        }
        #[cfg(test)]
        if let Some(coverage) = &mut request.coverage {
            coverage.prepared_taps = super::source_reader::tap_observation_for_test();
        }
        self.primed_output_frames = primed;
        self.source = Some(request);
        self.install_replayed_feed_endpoint(endpoint, endpoint)
    }

    fn prepare_source_feed_history(
        &mut self,
        request: NativeHistoryRequest,
        history: &SourceFeedSnapshot,
        future: (SourcePlayback, SourceReadPlan, u64, SourceFeedContext),
        endpoint_reset: Option<EndpointResetHistory>,
    ) -> Result<(), RubberBandError> {
        if !history.is_current()
            || !request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
            || !future_continues_actual_history(history, future)
        {
            return Err(unavailable());
        }
        let reset_shift = endpoint_reset
            .map(|endpoint| endpoint.phase_shift(&request))
            .transpose()?;
        let endpoint_pitch = request
            .pitch_intent
            .native_pitch_scale(request.playback.tempo_ratio(), request.key_lock)
            .map_err(|_| unavailable())?;
        let native = self
            .rubberband
            .as_mut()
            .ok_or(RubberBandError::ConstructionFailed)?;
        native.prepare_exact_pitch(endpoint_pitch)?;
        let mut bound = native.source_history_frames(endpoint_pitch)? as u64;
        let mut spans: Vec<SourceFeedSpan> = history.spans().copied().collect();
        let (mut playback, mut plan, mut remaining, future_context) = future;
        let end = history
            .active_frames()
            .checked_add(remaining)
            .ok_or_else(unavailable)?;
        let mut active = history.active_frames();
        for _ in 0..SourcePlayback::MAX_PROJECTION_SEGMENTS {
            if remaining == 0 {
                break;
            }
            let (frames, ratio) =
                playback.chunk(usize::try_from(remaining).map_err(|_| unavailable())?);
            if frames == 0 {
                return Err(unavailable());
            }
            let position = playback.position();
            plan.frame_pos = position.frame;
            plan.seek_mode = position.seek_mode;
            spans.push(SourceFeedSpan {
                active_start: active,
                frames,
                playback,
                plan,
                context: future_context.for_plan(plan),
            });
            playback.advance(frames);
            plan.transition.advance_output_frames(frames, ratio);
            active = active.checked_add(frames as u64).ok_or_else(unavailable)?;
            remaining -= frames as u64;
        }
        playback.chunk(DEFAULT_BLOCK_SAMPLES);
        let position = playback.position();
        plan.frame_pos = position.frame;
        plan.seek_mode = position.seek_mode;
        let expected = request.actual_feed_checkpoint();
        let (expected_playback, expected_plan) = endpoint_reset
            .map(|endpoint| (endpoint.playback, endpoint.plan))
            .unwrap_or((expected.playback, expected.plan));
        if remaining != 0
            || !playback.matches_exact(&expected_playback)
            || !plan.matches_exact(expected_plan)
        {
            return Err(unavailable());
        }
        if endpoint_reset.is_none()
            && expected.matches_exact(SourceReadCheckpoint {
                playback: request.playback,
                plan: request.plan,
            })
            && request
                .playback
                .history_before_constant(bound as usize)
                .is_some()
            && constant_chronology_matches(&request, history, &spans, end, bound)
        {
            return self.prepare_source_at_endpoint(request);
        }

        // H is reset-profile dependent. Expand monotonically as newly included
        // spans introduce larger bounds; no realtime work or reset between replay spans.
        let block = self.block_size as u64;
        if block == 0 {
            return Err(RubberBandError::InvalidBlockSize);
        }
        let mut origin = 0;
        let mut settled = false;
        let mut known_bounds = vec![(endpoint_pitch.to_bits(), bound)];
        let retained_start = history
            .retained_range()
            .map_or(history.active_frames(), |range| range.start);
        for _ in 0..=spans.len() {
            origin = if endpoint_reset.is_some() {
                // A new owner completes whole input blocks exactly at its own E.
                let frames = bound
                    .div_ceil(block)
                    .checked_mul(block)
                    .ok_or_else(unavailable)?;
                end.checked_sub(frames).ok_or_else(unavailable)?
            } else {
                let lattice = history.active_origin();
                end.checked_sub(bound)
                    .and_then(|start| start.checked_sub(lattice))
                    .ok_or_else(unavailable)?
                    / block
                    * block
                    + lattice
            };
            if end - origin > SOURCE_FEED_HISTORY_MAX_FRAMES as u64 || origin < retained_start {
                return Err(unavailable());
            }
            let mut required = bound;
            for span in spans
                .iter()
                .filter(|span| span.active_end() > origin && span.active_start < end)
            {
                let pitch = request
                    .pitch_intent
                    .native_pitch_scale(span.playback.tempo_ratio(), request.key_lock)
                    .map_err(|_| unavailable())?;
                let pitch_bound = if let Some((_, frames)) = known_bounds
                    .iter()
                    .find(|(bits, _)| *bits == pitch.to_bits())
                {
                    *frames
                } else {
                    native.prepare_exact_pitch(pitch)?;
                    let frames = native.source_history_frames(pitch)? as u64;
                    known_bounds.push((pitch.to_bits(), frames));
                    frames
                };
                required = required.max(pitch_bound);
            }
            if required == bound {
                settled = true;
                break;
            }
            bound = required;
        }
        if !settled {
            return Err(unavailable());
        }
        let mut replay = Vec::new();
        let mut next = origin;
        for span in &spans {
            let start = span.active_start.max(origin);
            let stop = span.active_end().min(end);
            if stop <= start {
                continue;
            }
            if start != next {
                return Err(unavailable());
            }
            let part = span
                .slice(
                    (start - span.active_start) as usize,
                    (stop - start) as usize,
                )
                .ok_or_else(unavailable)?;
            let readers = part
                .context
                .resolve_readers(part.plan, request.stems.as_ref())
                .ok_or_else(unavailable)?;
            let actual = SourceFeedContext::new(request.binding, readers);
            if !part.context.same_source(actual)
                || (endpoint_reset.is_some()
                    && (part.context.binding != request.binding
                        || part.plan.seek_mode != ExplicitSeekMode::Normal
                        || part.playback.position().seek_mode != ExplicitSeekMode::Normal
                        || !part.playback.matches_domain(request.plan.domain())
                        || part.plan.transition.is_active()
                        || !part.plan.matches_source_contract(request.plan)))
                || !normal_loop_feed_available_with_readers(
                    &request.sample,
                    readers,
                    request.binding.sample_rate_hz,
                    part.context.binding.accepted,
                    part.plan,
                    &part.playback,
                )
            {
                return Err(unavailable());
            }
            replay.push(part);
            next = stop;
        }
        if next != end || replay.is_empty() || end - origin < block {
            return Err(unavailable());
        }
        // Pitch belongs to the span completing a mixed-rate first input block.
        // Configure that cold profile before any real input (including >2 safety).
        let first_completion = origin + block;
        let first = replay
            .iter()
            .find(|span| span.active_end() >= first_completion)
            .ok_or_else(unavailable)?;
        let first_pitch = request
            .pitch_intent
            .native_pitch_scale(first.playback.tempo_ratio(), request.key_lock)
            .map_err(|_| unavailable())?;
        native.prepare_exact_pitch(first_pitch)?;
        self.source = None;
        self.reset_adapter();
        self.pitch_scale = first_pitch;
        self.dirty = false;
        self.used = false;
        let channels = self.input.len();
        let mut feed = vec![vec![0.0; DEFAULT_BLOCK_SAMPLES]; channels];
        let mut output = vec![vec![0.0; DEFAULT_BLOCK_SAMPLES]; channels];
        #[cfg(test)]
        super::source_reader::reset_tap_observation_for_test();
        for span in &replay {
            let mut offset = 0;
            while offset < span.frames {
                let frames = (span.frames - offset).min(DEFAULT_BLOCK_SAMPLES);
                let part = span.slice(offset, frames).ok_or_else(unavailable)?;
                let readers = part
                    .context
                    .resolve_readers(part.plan, request.stems.as_ref())
                    .ok_or_else(unavailable)?;
                if !part
                    .context
                    .same_source(SourceFeedContext::new(request.binding, readers))
                    || !normal_loop_feed_available_with_readers(
                        &request.sample,
                        readers,
                        request.binding.sample_rate_hz,
                        part.context.binding.accepted,
                        part.plan,
                        &part.playback,
                    )
                {
                    return Err(unavailable());
                }
                let (read_playback, read_plan) = if let Some(shift) = reset_shift {
                    let playback = part
                        .playback
                        .shifted_normal_history(shift)
                        .ok_or_else(unavailable)?;
                    let position = playback.position();
                    let plan = SourceReadPlan {
                        frame_pos: position.frame,
                        seek_mode: position.seek_mode,
                        ..part.plan
                    };
                    if !normal_loop_feed_available_with_readers(
                        &request.sample,
                        readers,
                        request.binding.sample_rate_hz,
                        request.binding.accepted,
                        plan,
                        &playback,
                    ) {
                        return Err(unavailable());
                    }
                    (playback, plan)
                } else {
                    (part.playback, part.plan)
                };
                if endpoint_reset.is_some() {
                    // Keep every real journal/seam/reader/rate/pitch proof above.
                    // This new physical source has no PCM before its birth, while
                    // the unchanged spans still advance the historical pitch clock.
                    for channel in &mut feed {
                        channel[..frames].fill(0.0);
                    }
                } else {
                    read_plan.fill_fractional_buffers_checked_with_readers(
                        &request.sample,
                        readers,
                        &read_playback,
                        &mut feed,
                        frames,
                    )?;
                }
                let pitch = request
                    .pitch_intent
                    .native_pitch_scale(part.playback.tempo_ratio(), request.key_lock)
                    .map_err(|_| unavailable())?;
                self.render(&feed, &mut output, frames, pitch)?;
                offset += frames;
            }
        }
        if !history.is_current()
            || !request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
            || (endpoint_reset.is_some()
                && !reset_source_feed_available(
                    &request.sample,
                    StemReadSets::new(request.stems.as_ref(), None),
                    request.binding.sample_rate_hz,
                    request.binding.accepted,
                    request.plan,
                    &request.playback,
                ))
        {
            return Err(unavailable());
        }
        #[cfg(test)]
        let mut request = request;
        #[cfg(test)]
        if let Some(coverage) = &mut request.coverage {
            coverage.prepared_taps = super::source_reader::tap_observation_for_test();
        }
        self.primed_output_frames = (end - origin) as usize;
        // For EndpointReset, the request deliberately retains the exact seek-at-E
        // representation after derived pre-E input. Binary64 translated phase need
        // not accumulate to that representation. Adoption remains strictly exact.
        // The frozen request retains exact E. Callback consumption owns subsequent
        // scalar progress, including a genuine continuation's retained transition.
        let logical = SourceReadCheckpoint {
            playback: request.playback,
            plan: request.plan,
        };
        let actual = request.actual_feed_checkpoint();
        self.source = Some(request);
        self.install_replayed_feed_endpoint(logical, actual)?;
        self.record_input_history_context(if endpoint_reset.is_some() {
            NativeStartHistoryContext::PhysicalBirth
        } else {
            NativeStartHistoryContext::SourceHistory
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
    use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
    use crate::audio_engine::native_history_permit::NativeHistoryContext;
    use crate::audio_engine::pitch_intent::PitchIntent;
    use crate::audio_engine::prepared_native_history::PitchSourceSnapshot;
    use crate::audio_engine::prepared_source::PreparedSourcePermit;
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
    use crate::audio_engine::source_feed_history::SourceFeedHistory;
    use crate::audio_engine::source_reader::{
        ExplicitSeekMode, FrameRange, StemRenderSelection, StemTransition,
    };
    use crate::messages::{PreparedStemSet, SampleBuffer};
    use std::sync::{Arc, atomic::AtomicU64};

    const RATE: u32 = 48_000;
    const CAPTURE: usize = 36_077;
    const FUTURE: u64 = 4096;

    struct Fixture {
        sample: SampleBuffer,
        ownership: Arc<InputRuntimeOwnership>,
        acks: Arc<CurrentTimingAcknowledgements>,
        epoch: Arc<AtomicU64>,
        history: SourceFeedHistory,
        playback: SourcePlayback,
        plan: SourceReadPlan,
        stems: Option<PreparedStemSet>,
    }

    impl Fixture {
        fn new(gap: bool, missing_context: bool) -> Self {
            Self::with_controls(gap, missing_context, true)
        }

        fn with_controls(gap: bool, missing_context: bool, rate_edit: bool) -> Self {
            Self::with_selection_controls(gap, missing_context, rate_edit, false)
        }

        fn with_selection_controls(
            gap: bool,
            missing_context: bool,
            rate_edit: bool,
            selected: bool,
        ) -> Self {
            let sample = SampleBuffer {
                channels: 1,
                residency: None,
                samples: (0..12_000)
                    .map(|n| ((n as f64 * 0.017).sin() * 0.25) as f32)
                    .collect::<Vec<_>>()
                    .into(),
            }
            .with_complete_source(RATE);
            let ownership = Arc::new(InputRuntimeOwnership::tracked());
            ownership.publish_test_source(0, &sample, RATE, 7);
            let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
            assert!(ownership.acknowledge_callback_source(key));
            let mut playback = SourcePlayback::new(117, ExplicitSeekMode::Normal, 1.2);
            playback.configure(
                12_000,
                FrameRange {
                    start: 100,
                    end: 3100,
                },
            );
            let mut history = SourceFeedHistory::new();
            let old_component = SampleBuffer {
                channels: 1,
                residency: sample.residency.clone(),
                samples: sample
                    .samples
                    .iter()
                    .map(|value| value * 0.6)
                    .collect::<Vec<_>>()
                    .into(),
            };
            let old_stems = PreparedStemSet {
                complete_set_identity: Arc::new([3; 32]),
                accepted_timing: None,
                reference_samples: sample.samples.clone(),
                reference_residency: sample.residency.clone(),
                publication: PreparedSourcePermit::unrestricted(),
                source_version_hash: 42,
                sample_rate_hz: RATE,
                channels: 1,
                frame_count: sample.frame_count(),
                available_mask: 15,
                stems: std::array::from_fn(|_| old_component.clone()),
            };
            let binding = super::super::productive_source_history::ProductiveSourceBinding::new(
                &sample, RATE, None,
            );
            let mut active = 0;
            let mut plan = SourceReadPlan {
                channels: 1,
                sample_frames: 12_000,
                frame_pos: 117,
                loop_region: FrameRange {
                    start: 100,
                    end: 3100,
                },
                loop_period: None,
                seek_mode: ExplicitSeekMode::Normal,
                selection: StemRenderSelection::full_mix(),
                transition: StemTransition::default(),
            };
            let mut read = vec![vec![0.0; 512]];
            while active < CAPTURE {
                if rate_edit && active == 32_017 {
                    playback.set_target(0.55);
                }
                let boundary = if active < 32_017 {
                    32_017
                } else if gap && active < 35_000 {
                    35_000
                } else {
                    CAPTURE
                };
                let (frames, _) = playback.chunk((boundary - active).min(233));
                let position = playback.position();
                plan.frame_pos = position.frame;
                plan.seek_mode = position.seek_mode;
                let stems =
                    (selected || (missing_context && active < 32_017)).then_some(&old_stems);
                plan.selection = stems.map_or_else(StemRenderSelection::full_mix, |set| {
                    let mask =
                        if selected && !(missing_context && (32_017..34_017).contains(&active)) {
                            5
                        } else {
                            1
                        };
                    StemRenderSelection::for_stems(set, mask)
                });
                plan.fill_fractional_buffers_checked(&sample, stems, &playback, &mut read, frames)
                    .unwrap();
                let context = SourceFeedContext::new(binding, StemReadSets::new(stems, None));
                if gap && active == 35_000 {
                    history.record_gap(frames);
                } else {
                    history.record_success(playback, plan, context, frames);
                }
                playback.advance(frames);
                active += frames;
            }
            playback.chunk(512);
            let position = playback.position();
            plan.frame_pos = position.frame;
            plan.seek_mode = position.seek_mode;
            Self {
                sample,
                ownership,
                acks: Arc::default(),
                epoch: Arc::new(AtomicU64::new(1)),
                history,
                playback,
                plan,
                stems: selected.then_some(old_stems),
            }
        }

        fn request(&self, sequence: u64) -> NativeHistoryRequest {
            let mut endpoint = PitchSourceSnapshot {
                sample: self.sample.clone(),
                stems: self.stems.clone(),
                accepted: None,
                sample_rate_hz: RATE,
                paused: false,
                playback: self.playback,
                plan: self.plan,
                actual_feed: None,
                source_feed_context: self.feed_context(),
            };
            assert!(endpoint.project_output_frames(FUTURE));
            let permit = NativeHistoryContext {
                id: 0,
                ownership: &self.ownership,
                acknowledgements: &self.acks,
                preparation_epoch: &self.epoch,
            }
            .capture(&self.sample, RATE, None)
            .unwrap();
            endpoint
                .into_request(
                    permit,
                    PitchIntent::new(6, 18).unwrap(),
                    true,
                    CAPTURE as u64 + FUTURE,
                    sequence,
                    1,
                )
                .unwrap()
        }

        fn snapshot(&self) -> SourceFeedSnapshot {
            let mut snapshot = SourceFeedSnapshot::new();
            assert!(self.history.capture_into(&mut snapshot));
            snapshot
        }

        fn feed_context(&self) -> SourceFeedContext {
            SourceFeedContext::new(
                super::super::productive_source_history::ProductiveSourceBinding::new(
                    &self.sample,
                    RATE,
                    None,
                ),
                StemReadSets::new(self.stems.as_ref(), None),
            )
        }

        fn reset_request(&self, sequence: u64) -> (NativeHistoryRequest, EndpointResetHistory) {
            let mut request = self.request(sequence);
            let endpoint = EndpointResetHistory {
                playback: request.playback,
                plan: request.plan,
            };
            request
                .playback
                .seek(request.plan.loop_region.start, ExplicitSeekMode::Normal);
            request.playback.configure_domain(request.plan.domain());
            request.plan.frame_pos = request.plan.loop_region.start;
            request.plan.seek_mode = ExplicitSeekMode::Normal;
            request.plan.transition.clear();
            (request, endpoint)
        }
    }

    #[test]
    fn planned_future_cannot_relabel_the_actual_historical_endpoint_and_same_owner_retries() {
        let mut candidate =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 1).unwrap()), 1);
        let address = candidate.rubberband.as_ref().unwrap().state_address();
        for (moving, reset) in [(false, false), (true, false), (false, true), (true, true)] {
            let valid = Fixture::with_controls(false, false, moving);
            candidate
                .prepare_source_with_feed_history(
                    valid.request(30),
                    &valid.snapshot(),
                    (valid.playback, valid.plan, FUTURE, valid.feed_context()),
                )
                .unwrap();
            let before = (
                candidate.rubberband.as_ref().unwrap().pitch_scale(),
                candidate.primed_output_frames,
                candidate.input_fifo[0].len(),
                candidate.output_fifo[0].len(),
            );
            let mut displaced = Fixture::with_controls(false, false, moving);
            // Same PCM, frame labels and tuple, but future begins at a different source
            // position. This models actual ahead-of-logical input joined to logical future.
            displaced.playback.advance(17);
            let position = displaced.playback.position();
            displaced.plan.frame_pos = position.frame;
            displaced.plan.seek_mode = position.seek_mode;
            let future = (
                displaced.playback,
                displaced.plan,
                FUTURE,
                displaced.feed_context(),
            );
            let prepared = if reset {
                let (mut request, endpoint) = displaced.reset_request(31);
                request.pitch_intent = PitchIntent::default();
                candidate.prepare_source_with_endpoint_reset_history(
                    request,
                    &displaced.snapshot(),
                    future,
                    endpoint,
                )
            } else {
                let mut request = displaced.request(31);
                request.pitch_intent = PitchIntent::default();
                candidate.prepare_source_with_feed_history(request, &displaced.snapshot(), future)
            };
            assert!(
                matches!(prepared, Err(RubberBandError::UnavailableSourceHistory)),
                "unproved historical-to-future position jump accepted: moving={moving} reset={reset}"
            );
            assert_eq!(
                candidate.rubberband.as_ref().unwrap().state_address(),
                address
            );
            assert_eq!(candidate.source.as_ref().unwrap().request_id, 30);
            assert_eq!(
                (
                    candidate.rubberband.as_ref().unwrap().pitch_scale(),
                    candidate.primed_output_frames,
                    candidate.input_fifo[0].len(),
                    candidate.output_fifo[0].len(),
                ),
                before,
                "refusal changed prepared Native/FIFO state"
            );
            candidate
                .prepare_source_with_feed_history(
                    valid.request(32),
                    &valid.snapshot(),
                    (valid.playback, valid.plan, FUTURE, valid.feed_context()),
                )
                .unwrap();
            assert_eq!(candidate.source.as_ref().unwrap().request_id, 32);
            assert_eq!(
                candidate.rubberband.as_ref().unwrap().state_address(),
                address
            );
        }
    }

    #[test]
    fn capture_seam_accepts_recorded_seeks_but_not_an_unread_future_jump() {
        let fixture = Fixture::with_controls(false, false, false);
        let mut history = SourceFeedHistory::new();
        let mut playback = fixture.playback;
        let mut plan = fixture.plan;
        let mut buffers = vec![vec![0.0; 17]];
        for frame in [317, 1703] {
            playback.seek(frame, ExplicitSeekMode::Normal);
            playback.configure_domain(plan.domain());
            plan.frame_pos = frame;
            plan.seek_mode = ExplicitSeekMode::Normal;
            plan.fill_fractional_buffers_checked(
                &fixture.sample,
                None,
                &playback,
                &mut buffers,
                17,
            )
            .unwrap();
            history.record_success(playback, plan, fixture.feed_context(), 17);
            playback.advance(17);
        }
        let position = playback.position();
        plan.frame_pos = position.frame;
        let mut snapshot = SourceFeedSnapshot::new();
        assert!(history.capture_into(&mut snapshot));
        assert_eq!(snapshot.spans().len(), 2);
        assert!(future_continues_actual_history(
            &snapshot,
            (playback, plan, 0, fixture.feed_context())
        ));
        playback.advance(1);
        plan.frame_pos = playback.position().frame;
        assert!(!future_continues_actual_history(
            &snapshot,
            (playback, plan, 0, fixture.feed_context())
        ));
    }

    #[test]
    fn capture_seam_preserves_accepted_period_alias_without_authorizing_a_seek() {
        use crate::audio_engine::constant_timing::AcceptedTimingProjection;
        use crate::audio_engine::source_playback::FractionalSourcePosition;
        use crate::audio_engine::source_reader::SourceLoopDomain;

        let fixture = Fixture::with_controls(false, false, false);
        let region = FrameRange {
            start: 19,
            end: 1519,
        };
        let prior_domain = SourceLoopDomain::musical(12_000, region, 1500.25).unwrap();
        let next_domain = SourceLoopDomain::musical(12_000, region, 1499.75).unwrap();
        let accepted = AcceptedTimingProjection {
            revision: [1; 32],
            period_seconds: 0.500_083_333_333_333_4,
            origin_seconds: -0.0,
            sample_rate_hz: RATE,
            publication_epoch: 2,
        };
        let mut prior_context = fixture.feed_context();
        prior_context.binding.accepted = Some(accepted);
        let mut plan = fixture.plan;
        plan.loop_region = region;
        plan.loop_period = prior_domain.musical_period;
        plan.frame_pos = 1518;
        let mut playback = SourcePlayback::new(1518, ExplicitSeekMode::Normal, 1.125);
        playback.configure_domain(prior_domain);
        let mut buffers = vec![vec![0.0; 1]];
        plan.fill_fractional_buffers_checked(&fixture.sample, None, &playback, &mut buffers, 1)
            .unwrap();
        let mut history = SourceFeedHistory::new();
        history.record_success(playback, plan, prior_context, 1);
        let mut snapshot = SourceFeedSnapshot::new();
        assert!(history.capture_into(&mut snapshot));
        assert_eq!(
            snapshot
                .spans()
                .next_back()
                .unwrap()
                .playback
                .position_at(1),
            FractionalSourcePosition {
                frame: 1519,
                fraction: 0.125,
                seek_mode: ExplicitSeekMode::Normal,
            }
        );
        let mut future = SourcePlayback::new(18, ExplicitSeekMode::Normal, 1.375);
        future.advance(1);
        future.configure_domain(next_domain);
        assert_eq!(
            future.position(),
            FractionalSourcePosition {
                frame: 19,
                fraction: 0.375,
                seek_mode: ExplicitSeekMode::Normal,
            }
        );
        let mut future_plan = plan;
        future_plan.frame_pos = future.position().frame;
        future_plan.loop_period = next_domain.musical_period;
        let mut next_context = prior_context;
        next_context.binding.accepted = Some(AcceptedTimingProjection {
            revision: [2; 32],
            period_seconds: 0.499_916_666_666_666_7,
            publication_epoch: 3,
            ..accepted
        });
        assert!(future_continues_actual_history(
            &snapshot,
            (future, future_plan, 0, next_context)
        ));
        assert!(!future_continues_actual_history(
            &snapshot,
            (future, future_plan, 0, prior_context)
        ));
    }

    #[test]
    fn lifecycle_origin_and_real_post_gap_suffix_keep_their_existing_preparation_paths() {
        let mut candidate =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 1).unwrap()), 1);
        let mut fresh = Fixture::with_controls(false, false, false);
        fresh.history.reset();
        let snapshot = fresh.snapshot();
        assert_eq!(snapshot.spans().len(), 0);
        assert_eq!(snapshot.active_origin(), CAPTURE as u64);
        assert_eq!(snapshot.active_frames(), CAPTURE as u64);
        candidate
            .prepare_source_with_feed_history(
                fresh.request(40),
                &snapshot,
                (fresh.playback, fresh.plan, FUTURE, fresh.feed_context()),
            )
            .unwrap();

        for reset in [false, true] {
            let mut fixture = Fixture::new(false, false);
            let original = fixture.snapshot();
            let mut history = SourceFeedHistory::new();
            history.record_gap(73);
            if reset {
                history.reset();
            }
            for span in original.spans() {
                let mut buffers = vec![vec![0.0; span.frames]];
                span.plan
                    .fill_fractional_buffers_checked(
                        &fixture.sample,
                        None,
                        &span.playback,
                        &mut buffers,
                        span.frames,
                    )
                    .unwrap();
                history.record_success(span.playback, span.plan, span.context, span.frames);
            }
            fixture.history = history;
            let snapshot = fixture.snapshot();
            assert_eq!(snapshot.active_origin(), if reset { 73 } else { 0 });
            assert_eq!(snapshot.active_frames(), CAPTURE as u64 + 73);
            candidate
                .prepare_source_with_feed_history(
                    fixture.request(41),
                    &snapshot,
                    (
                        fixture.playback,
                        fixture.plan,
                        FUTURE,
                        fixture.feed_context(),
                    ),
                )
                .unwrap();
            assert_eq!(candidate.source.as_ref().unwrap().request_id, 41);
        }
    }

    #[test]
    fn empty_post_gap_journal_does_not_invent_a_required_historical_prefix() {
        let mut fixture = Fixture::new(false, false);
        fixture.history.record_gap(37);
        fixture.playback.advance(37);
        fixture.playback.chunk(DEFAULT_BLOCK_SAMPLES);
        let position = fixture.playback.position();
        fixture.plan.frame_pos = position.frame;
        fixture.plan.seek_mode = position.seek_mode;
        let snapshot = fixture.snapshot();
        assert_eq!(snapshot.spans().len(), 0);
        assert_eq!(snapshot.active_origin(), 0);
        let future = (fixture.playback, fixture.plan, 0, fixture.feed_context());
        assert!(future_continues_actual_history(&snapshot, future));
        let mut request = fixture.request(42);
        request.playback = fixture.playback;
        request.plan = fixture.plan;
        let mut candidate =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 1).unwrap()), 1);
        assert!(matches!(
            candidate.prepare_source_with_feed_history(request, &snapshot, future),
            Err(RubberBandError::UnavailableSourceHistory)
        ));
    }

    #[test]
    fn moving_source_gap_and_unheld_context_refuse_without_poisoning_candidate() {
        let mut candidate =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 1).unwrap()), 1);
        for (gap, context) in [(true, false), (false, true)] {
            let missing = Fixture::new(gap, context);
            assert!(matches!(
                candidate.prepare_source_with_feed_history(
                    missing.request(1),
                    &missing.snapshot(),
                    (
                        missing.playback,
                        missing.plan,
                        FUTURE,
                        missing.feed_context()
                    ),
                ),
                Err(RubberBandError::UnavailableSourceHistory)
            ));
            let valid = Fixture::new(false, false);
            candidate
                .prepare_source_with_feed_history(
                    valid.request(2),
                    &valid.snapshot(),
                    (valid.playback, valid.plan, FUTURE, valid.feed_context()),
                )
                .unwrap();
            assert!(candidate.used && candidate.active && !candidate.dirty);
            assert!(candidate.primed_output_frames > FUTURE as usize);
            assert_eq!(candidate.source.as_ref().unwrap().request_id, 2);
        }
    }
    #[test]
    fn constant_endpoint_clock_cannot_skip_real_gap_or_retired_stem_selection() {
        let mut candidate =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 1).unwrap()), 1);
        for (gap, context) in [(true, false), (false, true)] {
            let missing = Fixture::with_controls(gap, context, false);
            let request = missing.request(3);
            assert!(request.playback.history_before_constant(4096).is_some());
            assert!(matches!(
                candidate.prepare_source_with_feed_history(
                    request,
                    &missing.snapshot(),
                    (
                        missing.playback,
                        missing.plan,
                        FUTURE,
                        missing.feed_context()
                    ),
                ),
                Err(RubberBandError::UnavailableSourceHistory)
            ));
        }
        let valid = Fixture::with_controls(false, false, false);
        candidate
            .prepare_source_with_feed_history(
                valid.request(4),
                &valid.snapshot(),
                (valid.playback, valid.plan, FUTURE, valid.feed_context()),
            )
            .unwrap();
        assert_eq!(candidate.source.as_ref().unwrap().request_id, 4);
        assert!(candidate.used && candidate.active && !candidate.dirty);
    }

    #[test]
    fn selected_reset_history_requires_exact_complete_pin_and_unchanged_actual_selection() {
        let mut candidate =
            NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 1).unwrap()), 1);
        for corruption in [
            "missing",
            "foreign_set",
            "foreign_pcm",
            "incomplete",
            "expired",
            "selection_aba",
        ] {
            // Every journal entry below follows a real checked input read; the ABA
            // case actually reads the other mask before returning to the endpoint mask.
            let invalid =
                Fixture::with_selection_controls(false, corruption == "selection_aba", true, true);
            let (mut request, endpoint) = invalid.reset_request(5);
            match corruption {
                "missing" => request.stems = None,
                "foreign_set" => {
                    request.stems.as_mut().unwrap().complete_set_identity = Arc::new([3; 32]);
                }
                "foreign_pcm" => {
                    let component = &mut request.stems.as_mut().unwrap().stems[0];
                    component.samples = component
                        .samples
                        .iter()
                        .map(|value| value + 0.01)
                        .collect::<Vec<_>>()
                        .into();
                }
                "incomplete" => {
                    let component = &mut request.stems.as_mut().unwrap().stems[3];
                    component.samples = component.samples[0..1000].to_vec().into();
                }
                "expired" => {
                    request
                        .stems
                        .as_ref()
                        .unwrap()
                        .publication
                        .epoch
                        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                }
                "selection_aba" => {}
                _ => unreachable!(),
            }
            assert!(
                matches!(
                    candidate.prepare_source_with_endpoint_reset_history(
                        request,
                        &invalid.snapshot(),
                        (
                            invalid.playback,
                            invalid.plan,
                            FUTURE,
                            invalid.feed_context()
                        ),
                        endpoint,
                    ),
                    Err(RubberBandError::UnavailableSourceHistory)
                ),
                "{corruption}"
            );
            // The same preparation owner can recover with a genuinely fresh current
            // request. Never restore an old publication epoch or relabel its readers.
            let valid = Fixture::with_selection_controls(false, false, true, true);
            let (request, endpoint) = valid.reset_request(6);
            candidate
                .prepare_source_with_endpoint_reset_history(
                    request,
                    &valid.snapshot(),
                    (valid.playback, valid.plan, FUTURE, valid.feed_context()),
                    endpoint,
                )
                .unwrap();
            assert!(candidate.used && candidate.active && !candidate.dirty);
            assert_eq!(candidate.source.as_ref().unwrap().request_id, 6);
        }
    }
}
