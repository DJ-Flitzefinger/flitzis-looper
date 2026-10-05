//! Independent fractional-origin source and raw-native oracle for explicit history anchors.
//!
//! The oracle only uses immutable PCM and algebraic half-open loop/intro/tail addressing. It
//! deliberately does not use SourcePlayback, SourceReadPlan or the preparation stream for input.

use super::*;
use crate::audio_engine::source_reader::{
    ExplicitSeekMode, FrameRange, StemRenderSelection, StemTransition,
};
use crate::messages::StemMixMode;
use std::sync::Arc;

const RATES: [u32; 3] = [44_100, 48_000, 96_000];
const RATIOS: [f32; 5] = [0.5, 0.73, 1.0, 1.37, 2.0];
const PATTERNS: [&[usize]; 4] = [&[1], &[64], &[512], &[64, 96, 257, 512, 31, 1]];
const SOURCE_FRAMES: usize = 1939;
const LOOP: FrameRange = FrameRange {
    start: 113,
    end: 1278,
};

struct Fixture {
    sample: SampleBuffer,
    stems: PreparedStemSet,
    origin: SourcePlayback,
    phase: FractionalSourcePosition,
    ratio: f32,
    rate: u32,
    mask: Option<u8>,
}

impl Fixture {
    fn new(rate: u32, ratio: f32, mode: ExplicitSeekMode, mask: Option<u8>) -> Self {
        let stems = std::array::from_fn(|component| SampleBuffer {
            channels: 2,
            samples: Arc::from(
                (0..SOURCE_FRAMES)
                    .flat_map(|frame| {
                        (0..2).map(move |channel| {
                            let coordinate = frame * 17 + channel * 131 + component * 977;
                            ((coordinate as f32 * 0.019).sin() + (coordinate as f32 * 0.057).cos())
                                * 0.035
                        })
                    })
                    .collect::<Vec<_>>(),
            ),
        });
        let sample = SampleBuffer {
            channels: 2,
            samples: Arc::from(
                (0..SOURCE_FRAMES * 2)
                    .map(|index| stems[..4].iter().map(|stem| stem.samples[index]).sum())
                    .collect::<Vec<_>>(),
            ),
        };
        let start = match mode {
            ExplicitSeekMode::Normal => LOOP.end - 3,
            ExplicitSeekMode::BeforeLoop => 7,
            ExplicitSeekMode::AfterLoop => 1453,
        };
        // All tested ratios, including 1 and 2, begin with an independently fractional phase.
        let initial_frames = if mode == ExplicitSeekMode::Normal {
            1
        } else {
            17
        };
        let distance = initial_frames as f64 * f64::from(0.73_f32);
        let phase = FractionalSourcePosition {
            frame: start + distance.floor() as usize,
            fraction: distance.fract(),
            seek_mode: mode,
        };
        let mut origin = SourcePlayback::new(start, mode, 0.73);
        origin.configure(SOURCE_FRAMES, LOOP);
        origin.advance(initial_frames);
        assert_eq!(origin.position(), phase);
        Self {
            sample,
            stems: PreparedStemSet {
                source_version_hash: 17,
                sample_rate_hz: rate,
                channels: 2,
                frame_count: SOURCE_FRAMES,
                available_mask: 0x1f,
                stems,
            },
            origin,
            phase,
            ratio,
            rate,
            mask,
        }
    }

    fn request(&self, history: usize, discard: usize) -> SourcePreparation {
        let mut logical = self.origin.at_constant_ratio(self.ratio);
        logical.advance(history);
        let position = logical.position();
        assert_eq!(position, self.reference_position(history));
        SourcePreparation {
            plan: SourceReadPlan {
                channels: 2,
                sample_frames: SOURCE_FRAMES,
                frame_pos: position.frame,
                loop_region: LOOP,
                seek_mode: position.seek_mode,
                selection: self
                    .mask
                    .map_or_else(StemRenderSelection::full_mix, |mask| {
                        StemRenderSelection::from_state(StemMixMode::AllStems, 17, mask)
                    }),
                transition: StemTransition::default(),
            },
            logical,
            tempo_ratio: self.ratio,
            discard_output_frames: discard,
            source_history: Some(SourceHistory {
                origin: self.origin,
                output_frames: history,
            }),
        }
    }

    fn integer_position(&self, offset: usize) -> (usize, ExplicitSeekMode) {
        let absolute = self.phase.frame + offset;
        let loop_length = LOOP.end - LOOP.start;
        match self.phase.seek_mode {
            ExplicitSeekMode::Normal => (
                LOOP.start + (absolute - LOOP.start) % loop_length,
                ExplicitSeekMode::Normal,
            ),
            ExplicitSeekMode::BeforeLoop if absolute < LOOP.start => {
                (absolute, ExplicitSeekMode::BeforeLoop)
            }
            ExplicitSeekMode::BeforeLoop => (
                LOOP.start + (absolute - LOOP.start) % loop_length,
                ExplicitSeekMode::Normal,
            ),
            ExplicitSeekMode::AfterLoop if absolute < SOURCE_FRAMES => {
                (absolute, ExplicitSeekMode::AfterLoop)
            }
            ExplicitSeekMode::AfterLoop => (
                LOOP.start + (absolute - SOURCE_FRAMES) % loop_length,
                ExplicitSeekMode::Normal,
            ),
        }
    }

    fn reference_position(&self, offset: usize) -> FractionalSourcePosition {
        let distance = self.phase.fraction + offset as f64 * f64::from(self.ratio);
        let (frame, seek_mode) = self.integer_position(distance.floor() as usize);
        FractionalSourcePosition {
            frame,
            fraction: distance.fract(),
            seek_mode,
        }
    }

    fn reference_sample(&self, output_frame: usize, channel: usize) -> f32 {
        let distance = self.phase.fraction + output_frame as f64 * f64::from(self.ratio);
        let whole = distance.floor() as usize;
        let fraction = distance.fract() as f32;
        let left = self.tap(whole, channel);
        if fraction == 0.0 {
            return left;
        }
        let right = self.tap(whole + 1, channel);
        left + (right - left) * fraction
    }

    fn tap(&self, offset: usize, channel: usize) -> f32 {
        let index = self.integer_position(offset).0 * 2 + channel;
        if let Some(mask) = self.mask {
            self.stems.stems[..4]
                .iter()
                .enumerate()
                .filter(|(component, _)| mask & (1 << component) != 0)
                .map(|(_, stem)| stem.samples[index])
                .sum()
        } else {
            self.sample.samples[index]
        }
    }

    fn native_reference(&self, output_frames: usize) -> (usize, usize, Vec<Vec<f32>>) {
        let mut native = RubberBandLiveShifter::new(self.rate, 2).unwrap();
        native
            .set_pitch_scale(f64::from(1.0_f32 / self.ratio))
            .unwrap();
        native.reset_for_preparation();
        let delay = native.start_delay();
        let block = native.block_size();
        let total = output_frames.div_ceil(block) * block;
        let mut input = vec![vec![0.0; block]; 2];
        let mut shifted = vec![vec![0.0; block]; 2];
        let mut output = (0..2)
            .map(|_| Vec::with_capacity(total))
            .collect::<Vec<_>>();
        for offset in (0..total).step_by(block) {
            for (channel, input) in input.iter_mut().enumerate() {
                for (frame, sample) in input.iter_mut().enumerate() {
                    *sample = self.reference_sample(offset + frame, channel);
                }
            }
            native.shift(&input, &mut shifted).unwrap();
            for channel in 0..2 {
                assert!(shifted[channel].iter().all(|sample| sample.is_finite()));
                output[channel].extend_from_slice(&shifted[channel]);
            }
        }
        (delay, block, output)
    }

    fn prove(
        &self,
        history: usize,
        discard: usize,
        pattern: &[usize],
        block: usize,
        reference: &[Vec<f32>],
        output_frames: usize,
    ) -> Vec<Vec<f32>> {
        let request = self.request(history, discard);
        let unchanged = request.logical.position();
        let mut stream = PreparedSourceStream::prepare(
            &self.sample,
            Some(&self.stems),
            request,
            RubberBandLiveShifter::new(self.rate, 2).unwrap(),
        )
        .unwrap();
        let feed = (discard + block - 1).div_ceil(block) * block;
        let retained = feed - discard;
        assert_eq!(stream.history_output_frames(), history);
        assert_eq!(stream.history_origin_position(), self.phase);
        assert_eq!(stream.discard_frames(), discard);
        assert_eq!(stream.prepared_feed_frames(), feed);
        assert_eq!(stream.retained_frames(), retained);
        assert_eq!(stream.logical_position(), unchanged);
        assert_eq!(stream.feed_position(), self.reference_position(feed));
        assert_eq!(stream.pending_input_frames(), 0);
        let mut output = (0..2)
            .map(|_| Vec::with_capacity(output_frames))
            .collect::<Vec<_>>();
        let mut elapsed = 0;
        let mut partition = 0;
        while elapsed < output_frames {
            let frames = pattern[partition % pattern.len()].min(output_frames - elapsed);
            let chunk = stream.render(frames).unwrap();
            for channel in 0..2 {
                assert_eq!(
                    &chunk[channel][..frames],
                    &reference[channel][discard + elapsed..discard + elapsed + frames],
                    "history/reference mismatch: rate={}, ratio={}, origin={:?}, mask={:?}, H={}, D={}, pattern={:?}, n={}",
                    self.rate,
                    self.ratio,
                    self.phase,
                    self.mask,
                    history,
                    discard,
                    pattern,
                    elapsed,
                );
                output[channel].extend_from_slice(&chunk[channel][..frames]);
            }
            elapsed += frames;
            partition += 1;
            assert_eq!(
                stream.logical_position(),
                self.reference_position(history + elapsed)
            );
            assert_eq!(
                stream.feed_position(),
                self.reference_position(feed + elapsed)
            );
            assert_eq!(stream.pending_input_frames(), elapsed % block);
            assert_eq!(stream.fifo_occupancy(), retained - elapsed % block);
            assert_eq!(request.logical.position(), unchanged);
            assert_eq!(self.origin.position(), self.phase);
        }
        output
    }
}

#[test]
fn source_history_matches_independent_fractional_source_and_raw_native_across_partitions() {
    let output_frames = 1537;
    let mut cases = 0;
    for rate in RATES {
        for ratio in RATIOS {
            for mode in [
                ExplicitSeekMode::Normal,
                ExplicitSeekMode::BeforeLoop,
                ExplicitSeekMode::AfterLoop,
            ] {
                for mask in [None, Some(0b1010), Some(0b1111)] {
                    let fixture = Fixture::new(rate, ratio, mode, mask);
                    let (delay, block, reference) = fixture.native_reference(12_288);
                    for history in [1, 73, 2049] {
                        let mut canonical = None;
                        for pattern in PATTERNS {
                            let output = fixture.prove(
                                history,
                                delay + 17,
                                pattern,
                                block,
                                &reference,
                                output_frames,
                            );
                            if let Some(expected) = &canonical {
                                assert_eq!(&output, expected);
                            } else {
                                canonical = Some(output);
                            }
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 1620);
}

#[test]
fn source_history_all_stems_match_full_mix_and_raw_discard_is_independent_of_history() {
    let full_mix = Fixture::new(48_000, 1.37, ExplicitSeekMode::AfterLoop, None);
    let all_stems = Fixture::new(48_000, 1.37, ExplicitSeekMode::AfterLoop, Some(0b1111));
    let (_, block, reference) = full_mix.native_reference(8192);
    for history in [1, 1025, MAX_HISTORY_OUTPUT_FRAMES] {
        for discard in [0, 1, block - 1, block, block + 1, 3671] {
            let output = all_stems.prove(history, discard, PATTERNS[3], block, &reference, 1537);
            assert_eq!(&output[0], &reference[0][discard..discard + 1537]);
        }
    }
}

fn assert_invalid_history(fixture: &Fixture, request: SourcePreparation) {
    assert!(matches!(
        PreparedSourceStream::prepare(
            &fixture.sample,
            Some(&fixture.stems),
            request,
            RubberBandLiveShifter::new(fixture.rate, 2).unwrap(),
        ),
        Err(PreparationError::InvalidHistory)
    ));
}

#[test]
fn source_history_rejects_bounds_phase_domain_and_active_transition_mismatches() {
    let fixture = Fixture::new(48_000, 0.73, ExplicitSeekMode::Normal, Some(0b1010));
    let request = fixture.request(2049, 1234);
    for count in [MAX_HISTORY_OUTPUT_FRAMES + 1, usize::MAX] {
        let mut invalid = request;
        invalid.source_history.as_mut().unwrap().output_frames = count;
        assert_invalid_history(&fixture, invalid);
    }
    let mut invalid = request;
    invalid.logical.advance(1);
    assert_invalid_history(&fixture, invalid);
    let mut invalid = request;
    let position = request.logical.position();
    invalid.logical = SourcePlayback::new(position.frame, position.seek_mode, 0.5);
    invalid.logical.configure(SOURCE_FRAMES, LOOP);
    invalid.logical.advance(1);
    assert_eq!(invalid.logical.position().frame, position.frame);
    assert_ne!(invalid.logical.position().fraction, position.fraction);
    assert_invalid_history(&fixture, invalid);
    let mut invalid = request;
    invalid.source_history.as_mut().unwrap().origin.advance(1);
    assert_invalid_history(&fixture, invalid);
    for (frame, mode) in [
        (0, ExplicitSeekMode::Normal),
        (SOURCE_FRAMES, ExplicitSeekMode::AfterLoop),
        (usize::MAX, ExplicitSeekMode::AfterLoop),
        (LOOP.start, ExplicitSeekMode::BeforeLoop),
        (LOOP.start, ExplicitSeekMode::AfterLoop),
    ] {
        let mut invalid = request;
        invalid.source_history.as_mut().unwrap().origin = SourcePlayback::new(frame, mode, 0.73);
        assert_invalid_history(&fixture, invalid);
        let mut invalid = request;
        invalid.logical = SourcePlayback::new(frame, mode, 0.73);
        assert_invalid_history(&fixture, invalid);
    }
    let mut invalid = request;
    invalid.plan.transition = StemTransition::start(StemRenderSelection::full_mix(), 128);
    assert_invalid_history(&fixture, invalid);
}

#[test]
fn source_history_zero_keeps_the_previous_logical_phase_and_transition_experiment() {
    let fixture = Fixture::new(48_000, 1.37, ExplicitSeekMode::BeforeLoop, Some(0b1010));
    let mut plain = fixture.request(73, 1531);
    plain.source_history = None;
    plain.plan.transition = StemTransition::start(StemRenderSelection::full_mix(), 128);
    let mut zero = plain;
    zero.source_history = Some(SourceHistory {
        origin: SourcePlayback::new(usize::MAX, ExplicitSeekMode::AfterLoop, 0.5),
        output_frames: 0,
    });
    let mut left = PreparedSourceStream::prepare(
        &fixture.sample,
        Some(&fixture.stems),
        plain,
        RubberBandLiveShifter::new(fixture.rate, 2).unwrap(),
    )
    .unwrap();
    let mut right = PreparedSourceStream::prepare(
        &fixture.sample,
        Some(&fixture.stems),
        zero,
        RubberBandLiveShifter::new(fixture.rate, 2).unwrap(),
    )
    .unwrap();
    assert_eq!(left.history_output_frames(), 0);
    assert_eq!(right.history_output_frames(), 0);
    assert_eq!(left.history_origin_position(), plain.logical.position());
    assert_eq!(right.history_origin_position(), plain.logical.position());
    for frames in PATTERNS[3].iter().copied().cycle().take(30) {
        assert_eq!(left.render(frames).unwrap(), right.render(frames).unwrap());
        assert_eq!(left.logical_position(), right.logical_position());
        assert_eq!(left.feed_position(), right.feed_position());
        assert_eq!(left.fifo_occupancy(), right.fifo_occupancy());
    }
}
