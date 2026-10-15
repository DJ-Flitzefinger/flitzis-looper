//! Worker-only replay of the voice's genuine bounded source chronology.
//!
//! A fresh reset on the declared active-feed lattice proves this finite replay,
//! not equality to a Native handle that has processed arbitrary earlier history.

use super::native_source_coverage::normal_loop_feed_available_with_readers;
use super::prepared_native_history::{NativeAdapterState, NativeHistoryRequest};
use super::rubberband_backend::RubberBandError;
use super::source_feed_history::{
    SOURCE_FEED_HISTORY_MAX_FRAMES, SourceFeedContext, SourceFeedSnapshot, SourceFeedSpan,
};
use super::source_playback::SourcePlayback;
use super::source_reader::{SourceReadPlan, StemReadSets};
use super::stretch_processor::DEFAULT_BLOCK_SAMPLES;

fn unavailable() -> RubberBandError {
    RubberBandError::UnavailableSourceHistory
}

fn constant_chronology_matches(
    request: &NativeHistoryRequest,
    history: &SourceFeedSnapshot,
    spans: &[SourceFeedSpan],
    end: u64,
    bound: u64,
) -> bool {
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
        let readers = span
            .plan
            .required_read_sets(StemReadSets::new(request.stems.as_ref(), None));
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

impl NativeAdapterState {
    /// Keep the established exact constant endpoint path. A non-reconstructible
    /// moving endpoint instead needs the original read spans plus frozen F-to-E.
    pub(crate) fn prepare_source_with_feed_history(
        &mut self,
        request: NativeHistoryRequest,
        history: &SourceFeedSnapshot,
        future: (SourcePlayback, SourceReadPlan, u64),
    ) -> Result<(), RubberBandError> {
        if !history.is_current()
            || !request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
        {
            return Err(unavailable());
        }
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
        let (mut playback, mut plan, mut remaining) = future;
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
            let readers = plan.required_read_sets(StemReadSets::new(request.stems.as_ref(), None));
            spans.push(SourceFeedSpan {
                active_start: active,
                frames,
                playback,
                plan,
                context: SourceFeedContext::new(request.binding, readers),
            });
            playback.advance(frames);
            plan.transition.advance_fractional(frames as f64 * ratio);
            active = active.checked_add(frames as u64).ok_or_else(unavailable)?;
            remaining -= frames as u64;
        }
        playback.chunk(DEFAULT_BLOCK_SAMPLES);
        let position = playback.position();
        plan.frame_pos = position.frame;
        plan.seek_mode = position.seek_mode;
        if remaining != 0
            || !playback.matches_exact(&request.playback)
            || !plan.matches_exact(request.plan)
        {
            return Err(unavailable());
        }
        if request
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
            let lattice = history.active_origin();
            origin = end
                .checked_sub(bound)
                .and_then(|start| start.checked_sub(lattice))
                .ok_or_else(unavailable)?
                / block
                * block
                + lattice;
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
                .plan
                .required_read_sets(StemReadSets::new(request.stems.as_ref(), None));
            let actual = SourceFeedContext::new(request.binding, readers);
            if !part.context.same_source(actual)
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
                part.plan.fill_fractional_buffers_checked(
                    &request.sample,
                    request.stems.as_ref(),
                    &part.playback,
                    &mut feed,
                    frames,
                )?;
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
        self.source = Some(request);
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
    }

    impl Fixture {
        fn new(gap: bool, missing_context: bool) -> Self {
            Self::with_controls(gap, missing_context, true)
        }

        fn with_controls(gap: bool, missing_context: bool, rate_edit: bool) -> Self {
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
                let stems = (missing_context && active < 32_017).then_some(&old_stems);
                plan.selection = stems.map_or_else(StemRenderSelection::full_mix, |set| {
                    StemRenderSelection::for_stems(set, 1)
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
            }
        }

        fn request(&self, sequence: u64) -> NativeHistoryRequest {
            let mut endpoint = PitchSourceSnapshot {
                sample: self.sample.clone(),
                stems: None,
                accepted: None,
                sample_rate_hz: RATE,
                paused: false,
                playback: self.playback,
                plan: self.plan,
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
                    (missing.playback, missing.plan, FUTURE),
                ),
                Err(RubberBandError::UnavailableSourceHistory)
            ));
            let valid = Fixture::new(false, false);
            candidate
                .prepare_source_with_feed_history(
                    valid.request(2),
                    &valid.snapshot(),
                    (valid.playback, valid.plan, FUTURE),
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
                    (missing.playback, missing.plan, FUTURE),
                ),
                Err(RubberBandError::UnavailableSourceHistory)
            ));
        }
        let valid = Fixture::with_controls(false, false, false);
        candidate
            .prepare_source_with_feed_history(
                valid.request(4),
                &valid.snapshot(),
                (valid.playback, valid.plan, FUTURE),
            )
            .unwrap();
        assert_eq!(candidate.source.as_ref().unwrap().request_id, 4);
        assert!(candidate.used && candidate.active && !candidate.dirty);
    }
}
