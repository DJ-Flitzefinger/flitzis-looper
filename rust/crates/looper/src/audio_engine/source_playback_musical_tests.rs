//! Coordinates-only musical continuation. The original source reader remains authoritative.

use super::*;
use crate::audio_engine::prepared_native_history::SourceReadCheckpoint;
use crate::audio_engine::productive_source_history::ProductiveSourceBinding;
use crate::audio_engine::source_feed_history::{SourceFeedContext, SourceFeedSpan};
use crate::audio_engine::source_reader::{
    SourceReadPlan, StemReadSets, StemRenderSelection, StemTransition,
};
use crate::messages::SampleBuffer;
use std::sync::Arc;

fn position(frame: usize, fraction: f64) -> FractionalSourcePosition {
    FractionalSourcePosition {
        frame,
        fraction,
        seek_mode: ExplicitSeekMode::Normal,
    }
}

fn domain(start: usize, length: usize, sample_frames: usize, period: f64) -> SourceLoopDomain {
    SourceLoopDomain {
        sample_frames,
        region: FrameRange {
            start,
            end: start + length,
        },
        musical_period: crate::audio_engine::source_reader::SourceReaderPeriod::from_scalar_period(
            Some(period),
        ),
    }
}

fn exact(actual: FractionalSourcePosition, expected: FractionalSourcePosition) {
    assert_eq!(actual.frame, expected.frame);
    assert_eq!(actual.fraction.to_bits(), expected.fraction.to_bits());
    assert_eq!(actual.seek_mode, expected.seek_mode);
}

fn read_plan(playback: SourcePlayback) -> SourceReadPlan {
    let domain = playback.domain.unwrap();
    let position = playback.position();
    SourceReadPlan {
        channels: 1,
        sample_frames: domain.sample_frames,
        frame_pos: position.frame,
        loop_region: domain.region,
        loop_period: domain.musical_period,
        seek_mode: position.seek_mode,
        selection: StemRenderSelection::full_mix(),
        transition: StemTransition::default(),
    }
}

#[test]
fn musical_constructor_rejects_bad_domain_pair_mode_and_rate_without_normalizing() {
    let valid = domain(7, 17, 31, 17.25);
    let anchor = position(10, 0.25);
    for rate in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.5,
        0.0,
        f64::from_bits(0.5_f64.to_bits() - 1),
        f64::from_bits(2.0_f64.to_bits() + 1),
    ] {
        assert!(SourcePlayback::scalar_musical_continuation(anchor, valid, rate, 1.0).is_none());
        assert!(SourcePlayback::scalar_musical_continuation(anchor, valid, 1.0, rate).is_none());
    }
    for fraction in [f64::NAN, f64::INFINITY, -f64::from_bits(1), -0.25, 1.0] {
        assert!(
            SourcePlayback::scalar_musical_continuation(position(10, fraction), valid, 1.0, 1.0,)
                .is_none()
        );
    }
    for frame in [0, 6, 25, usize::MAX] {
        assert!(
            SourcePlayback::scalar_musical_continuation(position(frame, 0.0), valid, 1.0, 1.0)
                .is_none()
        );
    }
    for seek_mode in [ExplicitSeekMode::BeforeLoop, ExplicitSeekMode::AfterLoop] {
        assert!(
            SourcePlayback::scalar_musical_continuation(
                FractionalSourcePosition {
                    seek_mode,
                    ..anchor
                },
                valid,
                1.0,
                1.0,
            )
            .is_none()
        );
    }
    for invalid in [
        SourceLoopDomain {
            musical_period: crate::audio_engine::source_reader::SourceReaderPeriod::Physical,
            ..valid
        },
        domain(7, 0, 31, 0.5),
        domain(7, 17, 23, 17.25),
        SourceLoopDomain {
            region: FrameRange { start: 24, end: 7 },
            ..valid
        },
        domain(7, 17, 31, 15.75),
        domain(7, 17, 31, 18.25),
        domain(7, 17, 31, 0.0),
        domain(7, 17, 31, -0.5),
        domain(7, 17, 31, f64::NAN),
        domain(7, 17, 31, f64::INFINITY),
        domain(0, 1, 1, f64::from_bits(0.5_f64.to_bits() - 1)),
        domain(
            0,
            1,
            1,
            f64::from_bits(MUSICAL_PAIR_MAX_PERIOD.to_bits() + 1),
        ),
    ] {
        assert!(SourcePlayback::scalar_musical_continuation(anchor, invalid, 1.0, 1.0).is_none());
    }
    for rate in [0.5, 2.0] {
        let playback =
            SourcePlayback::scalar_musical_continuation(anchor, valid, rate, rate).unwrap();
        exact(playback.position(), anchor);
        assert_eq!(
            playback.loop_period().unwrap().to_bits(),
            17.25_f64.to_bits()
        );
        assert!(playback.matches_domain(valid));
        assert_eq!(playback.scalar_projection, ScalarProjection::PairMusical);
    }
    // Even an equal integer musical period is never a physical-domain permit.
    assert!(
        SourcePlayback::scalar_continuation(anchor, domain(7, 17, 31, 17.0), 1.0, 1.0).is_none()
    );
}

#[test]
fn canonical_validation_compares_the_separate_pair_and_allows_the_virtual_end() {
    let narrow = domain(7, 2, 10, 1.0 + 2.0_f64.powi(-52));
    let tiny = 2.0_f64.powi(-54);
    assert_eq!(1.0 + tiny, 1.0);
    let accepted =
        SourcePlayback::scalar_musical_continuation(position(8, tiny), narrow, 1.0, 1.0).unwrap();
    exact(accepted.position(), position(8, tiny));
    for fraction in [2.0_f64.powi(-52), 0.25] {
        assert!(
            SourcePlayback::scalar_musical_continuation(position(8, fraction), narrow, 1.0, 1.0,)
                .is_none()
        );
    }
    let shorter = domain(7, 3, 11, 2.75);
    assert!(
        SourcePlayback::scalar_musical_continuation(position(9, 0.75), shorter, 1.0, 1.0).is_none()
    );
    let longer = domain(7, 3, 11, 3.25);
    let virtual_end = position(longer.region.end, 0.125);
    let mut accepted =
        SourcePlayback::scalar_musical_continuation(virtual_end, longer, 0.5, 0.5).unwrap();
    exact(accepted.position(), virtual_end);
    assert!(accepted.advance(1));
    exact(accepted.position(), position(7, 0.375));
    assert!(
        SourcePlayback::scalar_musical_continuation(
            position(longer.region.end, 0.25),
            longer,
            1.0,
            1.0,
        )
        .is_none()
    );
    let subframe = domain(7, 1, 8, 0.5);
    assert!(
        SourcePlayback::scalar_musical_continuation(
            position(7, 0.0),
            domain(7, 1, 8, f64::from_bits(0.5_f64.to_bits() - 1)),
            1.0,
            1.0,
        )
        .is_none()
    );
    assert!(
        SourcePlayback::scalar_musical_continuation(position(7, 0.5), subframe, 1.0, 1.0).is_none()
    );
    assert!(
        SourcePlayback::scalar_musical_continuation(
            position(7, f64::from_bits(0.5_f64.to_bits() - 1)),
            subframe,
            1.0,
            1.0,
        )
        .is_some()
    );
}

#[test]
fn generic_integer_displacement_and_exact_period_visits_retain_raw_fraction_bits() {
    for (length, period, rate, visit) in [
        (17, 16.75, 0.5, 67),
        (17, 17.0, 1.0, 17),
        (17, 17.25, 0.75, 23),
        (1, 0.5, 0.5, 1),
    ] {
        let source = domain(7, length, 31, period);
        for fraction in [-0.0, 0.0, f64::from_bits(1), 2.0_f64.powi(-70), 0.25] {
            let anchor = position(7, fraction);
            let initial =
                SourcePlayback::scalar_musical_continuation(anchor, source, rate, rate).unwrap();
            for elapsed in [0, visit, 2 * visit, 3 * visit] {
                exact(initial.position_at(elapsed).unwrap(), anchor);
                let mut copied = initial;
                assert!(copied.advance(elapsed));
                exact(copied.position(), anchor);
            }
        }
    }
    let source = domain(7, 17, 31, 17.25);
    for fraction in [-0.0, f64::from_bits(1), 2.0_f64.powi(-70), 0.25] {
        let initial =
            SourcePlayback::scalar_musical_continuation(position(10, fraction), source, 1.0, 1.0)
                .unwrap();
        // An ordinary integer move is also the generic fractional identity.
        exact(initial.position_at(1).unwrap(), position(11, fraction));
        exact(initial.position_at(2).unwrap(), position(12, fraction));
    }
}

#[test]
fn exact_carry_and_rounding_carry_are_distinct_before_canonical_period_normalization() {
    let source = domain(7, 3, 11, 3.25);
    let exact_carry = SourcePlayback::scalar_musical_continuation(
        position(7, 0.5 + 2.0_f64.powi(-53)),
        source,
        1.5,
        1.5,
    )
    .unwrap();
    exact(
        exact_carry.position_at(1).unwrap(),
        position(9, 2.0_f64.powi(-53)),
    );
    let rounded_carry = SourcePlayback::scalar_musical_continuation(
        position(8, 0.5 - 2.0_f64.powi(-54)),
        source,
        1.5,
        1.5,
    )
    .unwrap();
    exact(rounded_carry.position_at(1).unwrap(), position(10, 0.0));
    let wrap_from_fraction = SourcePlayback::scalar_musical_continuation(
        position(8, 0.5),
        domain(7, 2, 11, 1.75),
        0.5,
        0.5,
    )
    .unwrap();
    exact(
        wrap_from_fraction.position_at(1).unwrap(),
        position(7, 0.25),
    );
    let rounded_period = SourcePlayback::scalar_musical_continuation(
        position(7, f64::from_bits(0.25_f64.to_bits() - 1)),
        domain(7, 2, 11, 1.75),
        1.5,
        1.5,
    )
    .unwrap();
    exact(rounded_period.position_at(1).unwrap(), position(7, 0.0));
}

#[cfg(target_pointer_width = "64")]
#[test]
fn independent_fraction_oracle_covers_full_u64_nontrivial_periods_and_large_source_offsets() {
    // Root derived these constants independently using Python Fraction directly on
    // (q-start)+f+n*rate modulo P, then one correctly rounded fractional float.
    // No grid/TwoSum/source implementation was imported. The negative-zero case
    // additionally follows the generic exact integer-displacement identity contract.
    let max = u64::MAX as usize;
    let large = 9_007_199_254_740_999usize;
    let vectors = [
        (
            0,
            1,
            0x3fe0000000000000,
            0,
            1,
            0x4000000000000000,
            max,
            0,
            1,
        ),
        (
            0,
            1,
            0x3ff0000000000001,
            1,
            0x3c90000000000000,
            0x3fe0000000000000,
            max,
            0,
            0x3fe0000000001000,
        ),
        (
            0,
            1,
            0x3fe8000000000000,
            0,
            0x3fe0000000000000,
            0x3fe0000000000000,
            1,
            0,
            0x3fd0000000000000,
        ),
        (
            0,
            1,
            0x3ff0000000000000,
            0,
            0x8000000000000000,
            0x3ff0000000000000,
            max,
            0,
            0x8000000000000000,
        ),
        (
            0,
            1024,
            0x4090010000000000,
            1024,
            0x3fc0000000000000,
            0x3ff28f5c28f5c28f,
            max,
            390,
            0x3fcb851eb851eb88,
        ),
        (
            0,
            1024,
            0x408ffe0000000000,
            1023,
            0x3fe0000000000000,
            0x3ff28f5c28f5c28f,
            max,
            794,
            0x3feae147ae147ae2,
        ),
        (
            0,
            1024,
            0x4090000000000000,
            1023,
            0x3fefffffffffffff,
            0x3fe0000000000000,
            1,
            0,
            0x3fdffffffffffffe,
        ),
        (
            0,
            2,
            0x3ff8000000000000,
            1,
            0x3fdfffffffffffff,
            0x3ff0000000000000,
            1,
            1,
            0,
        ),
        (
            0,
            1,
            0x3fe8000000000000,
            0,
            0x3fcfffffffffffff,
            0x3fe0000000000000,
            1,
            0,
            0,
        ),
        (
            0,
            max,
            0x43f0000000000000,
            max,
            0x3fe0000000000000,
            0x3fe0000000000000,
            1,
            0,
            0,
        ),
        (
            0,
            max,
            0x43f0000000000000,
            max,
            1,
            0x4000000000000000,
            max,
            max - 2,
            1,
        ),
        (
            large,
            1024,
            0x4090010000000000,
            large + 1024,
            0x3fc0000000000000,
            0x3ff28f5c28f5c28f,
            9_007_199_254_740_993,
            large + 242,
            0x3fd23d70a3d70a3c,
        ),
    ];
    for (
        start,
        length,
        period,
        frame,
        fraction,
        rate,
        elapsed,
        expected_frame,
        expected_fraction,
    ) in vectors
    {
        let period = f64::from_bits(period);
        let rate = f64::from_bits(rate);
        let initial = SourcePlayback::scalar_musical_continuation(
            position(frame, f64::from_bits(fraction)),
            domain(start, length, start + length, period),
            rate,
            rate,
        )
        .unwrap();
        let expected = position(expected_frame, f64::from_bits(expected_fraction));
        exact(initial.position_at(elapsed).unwrap(), expected);
        let mut one = initial;
        assert!(one.advance(elapsed));
        exact(one.position(), expected);
        let mut split = initial;
        let prefix = elapsed / 3;
        assert!(split.advance(prefix));
        assert!(split.advance(elapsed - prefix));
        assert!(split.matches_exact(&one));
    }
}

#[cfg(target_pointer_width = "64")]
#[test]
fn compatibility_is_exact_against_integer_length_including_two_to_64() {
    let power = 1usize << 54;
    assert_eq!((power + 2) as f64, power as f64);
    assert!(
        SourcePlayback::scalar_musical_continuation(
            position(0, 0.0),
            domain(0, power + 1, power + 1, power as f64),
            1.0,
            1.0,
        )
        .is_some()
    );
    assert!(
        SourcePlayback::scalar_musical_continuation(
            position(0, 0.0),
            domain(0, power + 2, power + 2, power as f64),
            1.0,
            1.0,
        )
        .is_none()
    );
    let maximum = domain(0, usize::MAX, usize::MAX, MUSICAL_PAIR_MAX_PERIOD);
    // Both integer lengths cast to 2^64 in binary64, but only the first is
    // within the exact one-frame contract. Never narrow L before comparison.
    assert_eq!((usize::MAX - 1) as f64, MUSICAL_PAIR_MAX_PERIOD);
    assert!(
        SourcePlayback::scalar_musical_continuation(
            position(0, 0.0),
            domain(0, usize::MAX - 1, usize::MAX, MUSICAL_PAIR_MAX_PERIOD),
            1.0,
            1.0,
        )
        .is_none()
    );
    let anchor = position(usize::MAX, 0.75);
    let initial = SourcePlayback::scalar_musical_continuation(anchor, maximum, 0.5, 0.5).unwrap();
    exact(initial.position(), anchor);
    exact(initial.position_at(1).unwrap(), position(0, 0.25));
    assert!(
        SourcePlayback::scalar_musical_continuation(
            position(0, 0.0),
            SourceLoopDomain {
                musical_period:
                    crate::audio_engine::source_reader::SourceReaderPeriod::from_scalar_period(
                        Some(f64::from_bits(MUSICAL_PAIR_MAX_PERIOD.to_bits() - 1))
                    ),
                ..maximum
            },
            1.0,
            1.0,
        )
        .is_none()
    );
}

#[test]
fn unchanged_musical_reader_executes_identical_taps_gains_and_pcm_at_anchor_and_revisits() {
    let sample = SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from(vec![0.0, 0.0, -0.25, 0.75, 0.5, 0.0, 0.0, 0.0]),
    };
    let narrow = 1.0 + 2.0_f64.powi(-52);
    for (source, anchor, rate, visit, left, right, gain) in [
        (
            domain(2, 2, 8, narrow),
            position(3, 2.0_f64.powi(-54)),
            narrow,
            1,
            3,
            2,
            0.0f32,
        ),
        (
            domain(2, 3, 8, 2.75),
            position(4, 0.5),
            1.375,
            2,
            4,
            2,
            (2.0 / 3.0) as f32,
        ),
        (
            domain(2, 3, 8, 3.0),
            position(3, 2.0_f64.powi(-54)),
            1.0,
            3,
            3,
            4,
            2.0_f64.powi(-54) as f32,
        ),
        (
            domain(2, 3, 8, 3.25),
            position(5, 0.125),
            0.8125,
            4,
            4,
            2,
            0.9f32,
        ),
        (
            domain(2, 3, 8, 3.25),
            position(5, -0.0),
            0.8125,
            4,
            4,
            2,
            0.8f32,
        ),
        (
            domain(2, 3, 8, 3.25),
            position(5, f64::from_bits(1)),
            0.8125,
            4,
            4,
            2,
            0.8f32,
        ),
    ] {
        let initial =
            SourcePlayback::scalar_musical_continuation(anchor, source, rate, rate).unwrap();
        let expected = sample.samples[left] + (sample.samples[right] - sample.samples[left]) * gain;
        for elapsed in [0, visit, 2 * visit, 19 * visit] {
            let mut copied = initial;
            assert!(copied.advance(elapsed));
            exact(copied.position(), anchor);
            let plan = read_plan(copied);
            assert_eq!(
                plan.loop_period.scalar_period().map(f64::to_bits),
                source.musical_period.scalar_period().map(f64::to_bits)
            );
            let taps = plan.fractional_taps(copied.position().fraction);
            assert_eq!(taps.left, left);
            assert_eq!(taps.right, Some(right));
            assert_eq!(taps.right_gain.to_bits(), gain.to_bits());
            let mut buffers = [vec![-99.0; 1]];
            plan.fill_fractional_buffers(&sample, None, &copied, &mut buffers, 1)
                .unwrap();
            assert_eq!(buffers[0][0].to_bits(), expected.to_bits());
        }
    }
    // The existing recombination loses this tiny offset before the narrow seam:
    // its executed alpha stays zero, rather than a new pair-aware alpha of 1/4.
    let initial = SourcePlayback::scalar_musical_continuation(
        position(3, 2.0_f64.powi(-54)),
        domain(2, 2, 8, narrow),
        narrow,
        narrow,
    )
    .unwrap();
    assert_eq!(
        read_plan(initial)
            .fractional_taps(initial.position().fraction)
            .right_gain,
        0.0
    );
    assert_eq!((2.0_f64.powi(-54) / (narrow - 1.0)) as f32, 0.25);
}

#[test]
fn musical_mode_guards_history_domains_and_cross_mode_equivalence() {
    let source = domain(7, 17, 31, 17.0);
    let anchor = position(10, -0.0);
    let mut pair = SourcePlayback::scalar_musical_continuation(anchor, source, 1.0, 1.0).unwrap();
    assert!(pair.history_before_constant(1).is_none());
    assert!(pair.shifted_normal_history(0.0).is_none());
    let mut legacy = SourcePlayback::new(anchor.frame, ExplicitSeekMode::Normal, 1.0);
    assert!(legacy.configure_domain(source));
    legacy.origin.fraction = -0.0;
    legacy.history_reconstructible = false;
    let physical = SourcePlayback::scalar_continuation(
        anchor,
        SourceLoopDomain {
            musical_period: crate::audio_engine::source_reader::SourceReaderPeriod::Physical,
            ..source
        },
        1.0,
        1.0,
    )
    .unwrap();
    for other in [legacy, physical] {
        assert!(!pair.matches_exact(&other));
        assert!(!other.matches_exact(&pair));
        assert!(!pair.matches_rate_target(&other));
        assert!(!other.matches_rate_target(&pair));
    }
    assert!(pair.advance(17));
    let before = pair;
    pair.clear_explicit_seek();
    assert!(pair.configure_domain(source));
    assert!(pair.matches_exact(&before));
    for changed in [
        domain(7, 17, 31, f64::from_bits(17.0_f64.to_bits() + 1)),
        domain(8, 17, 31, 17.0),
        domain(7, 17, 32, 17.0),
        SourceLoopDomain {
            musical_period: crate::audio_engine::source_reader::SourceReaderPeriod::Physical,
            ..source
        },
    ] {
        assert!(!pair.configure_domain(changed));
        assert!(pair.matches_exact(&before));
    }
    let (mut history, silence) = pair.history_before_constant(13).unwrap();
    assert_eq!(silence, 0);
    assert!(history.advance(13));
    assert!(history.matches_exact(&pair));
    assert!(pair.history_before_constant(18).is_none());
    let prepared = pair.at_constant_ratio(0.75);
    exact(prepared.position(), pair.position());
    assert_eq!(prepared.scalar_projection, ScalarProjection::PairMusical);
    assert!(prepared.history_before_constant(1).is_none());
    assert!(prepared.shifted_normal_history(1.0).is_none());
    assert!(pair.seek(0, ExplicitSeekMode::BeforeLoop));
    assert_eq!(pair.scalar_projection, ScalarProjection::Legacy);
    assert!(pair.configure_domain(source));
    let mut intentional = SourcePlayback::new(0, ExplicitSeekMode::BeforeLoop, 1.0);
    assert!(intentional.configure_domain(source));
    assert!(pair.matches_exact(&intentional));
}

#[test]
fn equal_target_keeps_pending_glide_and_changed_target_rebases_once_at_the_same_pair() {
    let source = domain(7, 17, 31, 17.25);
    let mut playback =
        SourcePlayback::scalar_musical_continuation(position(10, 0.25), source, 1.0, 1.2).unwrap();
    assert_eq!(playback.chunk(200), (200, 1.05));
    assert!(playback.advance(200));
    let before = playback;
    assert!(playback.set_target(1.2));
    assert!(playback.matches_exact(&before));
    assert_eq!(playback.chunk(512), (312, 1.05));
    let pair = playback.position();
    assert!(playback.set_target(0.8));
    exact(playback.position(), pair);
    assert_eq!(playback.chunk(512), (512, 1.0));
    exact(playback.position(), pair);
    assert_eq!(playback.elapsed_output_frames, 0);
    assert_eq!(playback.scalar_projection, ScalarProjection::PairMusical);
    assert!(playback.history_before_constant(1).is_none());
    let after = playback;
    assert_eq!(playback.chunk(31), (31, 1.0));
    assert!(playback.matches_exact(&after));
}

#[test]
fn musical_glide_partitions_real_read_spans_and_checkpoint_copies_have_identical_epochs() {
    for period in [16.75, 17.0, 17.25] {
        let initial = SourcePlayback::scalar_musical_continuation(
            position(8, 2.0_f64.powi(-54)),
            domain(5, 17, 24, period),
            1.0,
            1.2,
        )
        .unwrap();
        let sample = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from(
                (0..24)
                    .map(|frame| (frame as f32 - 12.0) / 16.0)
                    .collect::<Vec<_>>(),
            ),
        };
        let binding = ProductiveSourceBinding::new(&sample, 48_000, None);
        let context = SourceFeedContext::new(binding, StemReadSets::new(None, None));
        let run = |partitions: &[usize]| {
            let mut playback = initial;
            let mut trace = Vec::new();
            let mut elapsed = 0;
            let mut packet = 0;
            while elapsed < 1733 {
                let mut remaining = partitions[packet % partitions.len()].min(1733 - elapsed);
                while remaining != 0 {
                    let (frames, ratio) = playback.chunk(remaining);
                    let plan = read_plan(playback);
                    let mut buffers = [vec![0.0; frames]];
                    plan.fill_fractional_buffers(&sample, None, &playback, &mut buffers, frames)
                        .unwrap();
                    let span = SourceFeedSpan {
                        active_start: elapsed as u64,
                        frames,
                        playback,
                        plan,
                        context,
                    };
                    let offset = frames / 2;
                    let suffix = span.slice(offset, frames - offset).unwrap();
                    let mut copied = playback;
                    assert!(copied.advance(offset));
                    assert!(suffix.playback.matches_exact(&copied));
                    for (frame, sample) in buffers[0].iter().enumerate() {
                        let pair = playback.position_at(frame).unwrap();
                        trace.push((
                            ratio.to_bits(),
                            pair.frame,
                            pair.fraction.to_bits(),
                            sample.to_bits(),
                        ));
                    }
                    assert!(playback.advance(frames));
                    remaining -= frames;
                    elapsed += frames;
                }
                packet += 1;
            }
            (trace, playback)
        };
        let (reference, final_clock) = run(&[1]);
        for (frame, expected_ratio) in [
            (0, 1.05),
            (511, 1.05),
            (512, 1.1),
            (1023, 1.1),
            (1024, 1.15),
            (1535, 1.15),
            (1536, 1.2),
        ] {
            assert!((f64::from_bits(reference[frame].0) - expected_ratio).abs() < 1.0e-14);
        }
        for partition in [&[512][..], &[31, 777, 1, 96, 257][..]] {
            let (trace, copied) = run(partition);
            assert_eq!(trace, reference);
            assert!(copied.matches_exact(&final_clock));
        }
        let mut checkpoint = SourceReadCheckpoint {
            playback: initial,
            plan: read_plan(initial),
        };
        assert!(checkpoint.project_output_frames(1733));
        assert!(checkpoint.is_consistent());
        assert!(checkpoint.playback.matches_exact(&final_clock));
        assert!(final_clock.history_before_constant(198).is_none());
        let (mut history, silence) = final_clock.history_before_constant(197).unwrap();
        assert_eq!(silence, 0);
        exact(
            history.position(),
            position(reference[1536].1, f64::from_bits(reference[1536].2)),
        );
        let mut buffers = [vec![0.0; 197]];
        read_plan(history)
            .fill_fractional_buffers(&sample, None, &history, &mut buffers, 197)
            .unwrap();
        for (actual, expected) in buffers[0].iter().zip(&reference[1536..]) {
            assert_eq!(actual.to_bits(), expected.3);
        }
        assert!(history.advance(197));
        assert!(history.matches_exact(&final_clock));
    }
}

#[cfg(target_pointer_width = "64")]
#[test]
fn musical_epoch_overflow_refuses_before_clock_checkpoint_or_output_mutation() {
    let mut playback = SourcePlayback::scalar_musical_continuation(
        position(2, -0.0),
        domain(2, 3, 8, 3.25),
        1.0,
        1.0,
    )
    .unwrap();
    assert!(playback.advance(u64::MAX as usize));
    let before = playback;
    assert!(!playback.advance(1));
    assert!(playback.matches_exact(&before));
    assert!(playback.position_at(1).is_none());
    assert!(playback.position_at(0).is_some());
    assert!(!playback.can_read_frames(1));
    let mut checkpoint = SourceReadCheckpoint {
        playback,
        plan: read_plan(playback),
    };
    let original = checkpoint;
    assert!(!checkpoint.project_output_frames(1));
    assert!(checkpoint.matches_exact(original));
    let sample = SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from(vec![0.0; 8]),
    };
    let mut buffers = [vec![-1.0; 1]];
    assert!(
        read_plan(playback)
            .fill_fractional_buffers(&sample, None, &playback, &mut buffers, 1)
            .is_err()
    );
    assert_eq!(buffers[0], vec![-1.0]);
}

#[test]
fn musical_pair_copy_remains_nonowning_and_within_the_existing_fixed_lane_bound() {
    fn copy<T: Copy>() {}
    copy::<SourcePlayback>();
    assert!(!std::mem::needs_drop::<SourcePlayback>());
    assert!(std::mem::size_of::<MappedSourceSpan>() <= 264);
    assert!(
        std::mem::size_of::<[Option<SourcePlayback>; crate::audio_engine::constants::MAX_VOICES]>()
            <= 16 * 1024
    );
}
