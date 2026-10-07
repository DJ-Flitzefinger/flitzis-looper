//! Non-live source-preparation proof fixture, compiled only in native tests.
//!
//! This establishes exact-ratio source/FIFO accounting before asynchronous live adoption. It
//! owns coherent native state and fixed FIFOs, borrows immutable accepted sources, and keeps the
//! logical source cursor separate from future DSP feed. An explicit discard index is an experiment,
//! not a certified audible compensation rule. No mixer, scheduler or callback uses this fixture.
//! Nonzero history declares a prior source origin and an exact output-frame distance H to the
//! requested logical phase. Native input starts at that origin; raw discard D and feed length P
//! are measured from it independently of H. History accepts only a fixed source selection: an
//! active transition lacks its prior gains and cannot be reconstructed by reversing this plan.

use super::constants::{SPEED_MAX, SPEED_MIN};
use super::rubberband_backend::{
    RubberBandError, RubberBandLiveShifter, pitch_scale_for_tempo_ratio,
};
use super::source_playback::{FractionalSourcePosition, SourcePlayback};
use super::source_reader::SourceReadPlan;
use super::stretch_processor::{DEFAULT_BLOCK_SAMPLES, FixedFifo};
use crate::messages::{PreparedStemSet, SampleBuffer};

const MAX_PREPARATION_OUTPUT_FRAMES: usize = 131_072;
const MAX_HISTORY_OUTPUT_FRAMES: usize = 131_072;

#[derive(Debug, thiserror::Error)]
pub(super) enum PreparationError {
    #[error("source/native channel layout or loop bounds are invalid")]
    InvalidSource,
    #[error("source preparation requires an exact finite supported tempo ratio")]
    InvalidRatio,
    #[error("explicit preparation discard exceeds the bounded proof fixture")]
    InvalidDiscard,
    #[error("source history is unbounded, invalid or does not reach the requested logical phase")]
    InvalidHistory,
    #[error("render exceeds fixed proof-fixture capacity")]
    InvalidRenderFrames,
    #[error("prepared FIFO accounting failed; state cannot be reused")]
    FifoAccounting,
    #[error("native source preparation failed: {0}")]
    Backend(#[from] RubberBandError),
}

#[derive(Clone, Copy)]
pub(super) struct SourceHistory {
    /// Explicit prior source epoch; preparation never guesses a reverse loop/seek traversal.
    pub(super) origin: SourcePlayback,
    /// Output-domain frames at the requested exact ratio from origin to logical playback.
    pub(super) output_frames: usize,
}

#[derive(Clone, Copy)]
pub(super) struct SourcePreparation {
    pub(super) plan: SourceReadPlan,
    pub(super) logical: SourcePlayback,
    pub(super) tempo_ratio: f64,
    pub(super) discard_output_frames: usize,
    pub(super) source_history: Option<SourceHistory>,
}

pub(super) struct PreparedSourceStream<'a> {
    sample: &'a SampleBuffer,
    stems: Option<&'a PreparedStemSet>,
    plan: SourceReadPlan,
    logical: SourcePlayback,
    feed: SourcePlayback,
    native: RubberBandLiveShifter,
    input: Vec<Vec<f32>>,
    native_input: Vec<Vec<f32>>,
    native_output: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
    input_fifo: Vec<FixedFifo>,
    output_fifo: Vec<FixedFifo>,
    discard: usize,
    prepared_feed: usize,
    retained: usize,
    history_output_frames: usize,
    history_origin: FractionalSourcePosition,
    valid: bool,
}

impl<'a> PreparedSourceStream<'a> {
    /// Allocating, bounded preparation performed exclusively by this non-realtime proof fixture.
    pub(super) fn prepare(
        sample: &'a SampleBuffer,
        stems: Option<&'a PreparedStemSet>,
        request: SourcePreparation,
        mut native: RubberBandLiveShifter,
    ) -> Result<Self, PreparationError> {
        let plan = request.plan;
        let channels = plan.channels;
        let block = native.block_size();
        if channels == 0
            || native.channel_count() != channels
            || block == 0
            || block > DEFAULT_BLOCK_SAMPLES
            || plan.sample_frames.checked_mul(channels) != Some(sample.samples.len())
            || sample.channels != channels
            || plan.loop_region.start >= plan.loop_region.end
            || plan.loop_region.end > plan.sample_frames
            || plan.loop_period.is_some_and(|period| {
                !period.is_finite()
                    || period <= 0.0
                    || (period - plan.loop_region.len() as f64).abs() > 1.0
            })
            || stems.is_some_and(|stems| {
                stems.channels != channels
                    || stems.frame_count != plan.sample_frames
                    || stems.stems.iter().any(|stem| {
                        stem.channels != channels || stem.samples.len() != sample.samples.len()
                    })
            })
        {
            return Err(PreparationError::InvalidSource);
        }
        if !request.tempo_ratio.is_finite()
            || !(SPEED_MIN..=SPEED_MAX).contains(&request.tempo_ratio)
        {
            return Err(PreparationError::InvalidRatio);
        }
        let discard = request.discard_output_frames;
        let prepared_feed = discard
            .checked_add(block - 1)
            .and_then(|frames| frames.div_ceil(block).checked_mul(block))
            .filter(|frames| *frames <= MAX_PREPARATION_OUTPUT_FRAMES)
            .ok_or(PreparationError::InvalidDiscard)?;
        let retained = prepared_feed - discard;

        let mut logical = request.logical.at_constant_ratio(request.tempo_ratio);
        logical.configure_domain(plan.domain());
        let history_output_frames = request
            .source_history
            .map_or(0, |history| history.output_frames);
        let feed = if history_output_frames == 0 {
            // Zero history is the existing exact-phase experiment, including active transitions.
            logical
        } else {
            if history_output_frames > MAX_HISTORY_OUTPUT_FRAMES || plan.transition.is_active() {
                return Err(PreparationError::InvalidHistory);
            }
            let history = request.source_history.unwrap();
            let requested_position = request.logical.position();
            let origin_position = history.origin.position();
            if !valid_history_position(requested_position, plan)
                || !valid_history_position(origin_position, plan)
                || logical.position() != requested_position
            {
                return Err(PreparationError::InvalidHistory);
            }
            let mut origin = history.origin.at_constant_ratio(request.tempo_ratio);
            origin.configure_domain(plan.domain());
            if origin.position() != origin_position {
                return Err(PreparationError::InvalidHistory);
            }
            let source_distance =
                origin_position.fraction + history_output_frames as f64 * request.tempo_ratio;
            if !source_distance.is_finite()
                || origin_position
                    .frame
                    .checked_add(source_distance.floor() as usize)
                    .is_none()
            {
                return Err(PreparationError::InvalidHistory);
            }
            let mut anchor = origin;
            anchor.advance(history_output_frames);
            if anchor.position() != requested_position {
                return Err(PreparationError::InvalidHistory);
            }
            origin
        };
        let history_origin = feed.position();

        // Set pitch before reset: native reset initializes its previous output hop at that pitch.
        // First shift processes actual source content; no neutral silent warming precedes it.
        native.prepare_exact_pitch(pitch_scale_for_tempo_ratio(request.tempo_ratio))?;
        let mut stream = Self {
            sample,
            stems,
            plan,
            logical,
            feed,
            native,
            input: vec![vec![0.0; DEFAULT_BLOCK_SAMPLES]; channels],
            native_input: vec![vec![0.0; block]; channels],
            native_output: vec![vec![0.0; block]; channels],
            output: vec![vec![0.0; DEFAULT_BLOCK_SAMPLES]; channels],
            input_fifo: (0..channels)
                .map(|_| FixedFifo::new(block + DEFAULT_BLOCK_SAMPLES))
                .collect(),
            output_fifo: (0..channels)
                .map(|_| FixedFifo::new(retained + DEFAULT_BLOCK_SAMPLES + block))
                .collect(),
            discard,
            prepared_feed,
            retained,
            history_output_frames,
            history_origin,
            valid: true,
        };
        for start in (0..prepared_feed).step_by(block) {
            stream.fill_feed(block);
            for channel in 0..channels {
                stream.native_input[channel].copy_from_slice(&stream.input[channel][..block]);
            }
            stream
                .native
                .shift(&stream.native_input, &mut stream.native_output)?;
            let keep_start = discard.saturating_sub(start).min(block);
            for channel in 0..channels {
                let keep = &stream.native_output[channel][keep_start..block];
                if stream.output_fifo[channel].push_slice(keep) != keep.len() {
                    return Err(PreparationError::FifoAccounting);
                }
            }
        }
        Ok(stream)
    }

    /// Continue the exact prepared native stream with bounded staging and no inserted silence.
    pub(super) fn render(&mut self, frames: usize) -> Result<&[Vec<f32>], PreparationError> {
        if frames > DEFAULT_BLOCK_SAMPLES {
            return Err(PreparationError::InvalidRenderFrames);
        }
        if !self.valid {
            return Err(PreparationError::FifoAccounting);
        }
        let result = self.render_inner(frames);
        if result.is_err() {
            self.valid = false;
        }
        result?;
        self.logical.advance(frames);
        Ok(&self.output)
    }

    fn render_inner(&mut self, frames: usize) -> Result<(), PreparationError> {
        self.fill_feed(frames);
        for channel in 0..self.plan.channels {
            if self.input_fifo[channel].push_slice(&self.input[channel][..frames]) != frames {
                return Err(PreparationError::FifoAccounting);
            }
        }
        let block = self.native.block_size();
        // Input backlog is at most B-1 plus one fixed-capacity segment.
        for _ in 0..(DEFAULT_BLOCK_SAMPLES / block + 1) {
            if self.input_fifo[0].len() < block {
                break;
            }
            for channel in 0..self.plan.channels {
                if self.input_fifo[channel].pop_into(&mut self.native_input[channel]) != block {
                    return Err(PreparationError::FifoAccounting);
                }
            }
            self.native
                .shift(&self.native_input, &mut self.native_output)?;
            for channel in 0..self.plan.channels {
                if self.output_fifo[channel].push_slice(&self.native_output[channel]) != block {
                    return Err(PreparationError::FifoAccounting);
                }
            }
        }
        for channel in 0..self.plan.channels {
            if self.output_fifo[channel].pop_into(&mut self.output[channel][..frames]) != frames {
                return Err(PreparationError::FifoAccounting);
            }
        }
        Ok(())
    }

    fn fill_feed(&mut self, frames: usize) {
        self.plan.fill_fractional_buffers(
            self.sample,
            self.stems,
            &self.feed,
            &mut self.input,
            frames,
        );
        self.plan
            .transition
            .advance_fractional(frames as f64 * self.feed.tempo_ratio());
        self.feed.advance(frames);
    }

    pub(super) fn logical_position(&self) -> FractionalSourcePosition {
        self.logical.position()
    }

    pub(super) fn feed_position(&self) -> FractionalSourcePosition {
        self.feed.position()
    }

    pub(super) fn native_delay(&self) -> usize {
        self.native.start_delay()
    }

    pub(super) fn discard_frames(&self) -> usize {
        self.discard
    }

    pub(super) fn retained_frames(&self) -> usize {
        self.retained
    }

    pub(super) fn prepared_feed_frames(&self) -> usize {
        self.prepared_feed
    }

    pub(super) fn history_output_frames(&self) -> usize {
        self.history_output_frames
    }

    pub(super) fn history_origin_position(&self) -> FractionalSourcePosition {
        self.history_origin
    }

    pub(super) fn fifo_occupancy(&self) -> usize {
        self.output_fifo[0].len()
    }

    pub(super) fn pending_input_frames(&self) -> usize {
        self.input_fifo[0].len()
    }
}

fn valid_history_position(position: FractionalSourcePosition, plan: SourceReadPlan) -> bool {
    use super::source_reader::ExplicitSeekMode;

    position.fraction.is_finite()
        && (0.0..1.0).contains(&position.fraction)
        && match position.seek_mode {
            ExplicitSeekMode::Normal => {
                position.frame >= plan.loop_region.start
                    && (position.frame - plan.loop_region.start) as f64 + position.fraction
                        < plan.loop_period.unwrap_or(plan.loop_region.len() as f64)
            }
            ExplicitSeekMode::BeforeLoop => position.frame < plan.loop_region.start,
            ExplicitSeekMode::AfterLoop => {
                (plan.loop_region.end..plan.sample_frames).contains(&position.frame)
            }
        }
}

#[path = "key_lock_source_preparation_tests.rs"]
mod tests;

#[path = "key_lock_source_history_tests.rs"]
mod history_tests;

#[cfg(test)]
mod bounds {
    use super::*;
    use crate::audio_engine::source_reader::{
        ExplicitSeekMode, FrameRange, StemRenderSelection, StemTransition,
    };
    use std::sync::Arc;

    fn fixture() -> (SampleBuffer, SourcePreparation) {
        let sample = SampleBuffer {
            channels: 2,
            samples: Arc::from(vec![0.25; 128]),
        };
        let plan = SourceReadPlan {
            channels: 2,
            sample_frames: 64,
            frame_pos: 7,
            loop_region: FrameRange { start: 3, end: 59 },
            loop_period: None,
            seek_mode: ExplicitSeekMode::Normal,
            selection: StemRenderSelection::full_mix(),
            transition: StemTransition::default(),
        };
        let mut logical = SourcePlayback::new(7, plan.seek_mode, 0.73);
        logical.configure(plan.sample_frames, plan.loop_region);
        logical.advance(13);
        (
            sample,
            SourcePreparation {
                plan,
                logical,
                tempo_ratio: 0.73,
                discard_output_frames: 1031,
                source_history: None,
            },
        )
    }

    #[test]
    fn invalid_source_ratio_and_extreme_discard_fail_before_reading() {
        let (sample, request) = fixture();
        for ratio in [f64::NAN, f64::INFINITY, 0.0, -1.0, 0.49, 2.01] {
            let mut invalid = request;
            invalid.tempo_ratio = ratio;
            assert!(matches!(
                PreparedSourceStream::prepare(
                    &sample,
                    None,
                    invalid,
                    RubberBandLiveShifter::new(48_000, 2).unwrap()
                ),
                Err(PreparationError::InvalidRatio)
            ));
        }
        for discard in [usize::MAX, usize::MAX - 511, MAX_PREPARATION_OUTPUT_FRAMES] {
            let mut invalid = request;
            invalid.discard_output_frames = discard;
            assert!(matches!(
                PreparedSourceStream::prepare(
                    &sample,
                    None,
                    invalid,
                    RubberBandLiveShifter::new(48_000, 2).unwrap()
                ),
                Err(PreparationError::InvalidDiscard)
            ));
        }
        let mut invalid = request;
        invalid.plan.loop_region.end = 65;
        assert!(matches!(
            PreparedSourceStream::prepare(
                &sample,
                None,
                invalid,
                RubberBandLiveShifter::new(48_000, 2).unwrap()
            ),
            Err(PreparationError::InvalidSource)
        ));
        assert!(matches!(
            PreparedSourceStream::prepare(
                &sample,
                None,
                request,
                RubberBandLiveShifter::new(48_000, 1).unwrap()
            ),
            Err(PreparationError::InvalidSource)
        ));
    }

    #[test]
    fn rejected_render_and_zero_frames_leave_prepared_state_unchanged() {
        let (sample, request) = fixture();
        let original = request.logical.position();
        let mut stream = PreparedSourceStream::prepare(
            &sample,
            None,
            request,
            RubberBandLiveShifter::new(48_000, 2).unwrap(),
        )
        .unwrap();
        assert_eq!(stream.logical_position(), original);
        assert_eq!(request.logical.position(), original);
        let before = (
            stream.logical_position(),
            stream.feed_position(),
            stream.fifo_occupancy(),
        );
        assert!(matches!(
            stream.render(DEFAULT_BLOCK_SAMPLES + 1),
            Err(PreparationError::InvalidRenderFrames)
        ));
        stream.render(0).unwrap();
        assert_eq!(
            (
                stream.logical_position(),
                stream.feed_position(),
                stream.fifo_occupancy()
            ),
            before
        );
        stream.render(DEFAULT_BLOCK_SAMPLES).unwrap();
        assert_ne!(stream.logical_position(), original);
        assert_eq!(request.logical.position(), original);
    }
}
