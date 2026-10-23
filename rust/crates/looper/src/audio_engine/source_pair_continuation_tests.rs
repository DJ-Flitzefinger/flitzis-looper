//! Coordinates-only physical continuation proof. No live admission or source permit is built.

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

fn physical(start: usize, end: usize, sample_frames: usize) -> SourceLoopDomain {
    SourceLoopDomain {
        sample_frames,
        region: FrameRange { start, end },
        musical_period: None,
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
fn physical_constructor_rejects_invalid_rates_pairs_and_every_musical_domain() {
    let domain = physical(7, 24, 31);
    let anchor = position(10, 0.25);
    for invalid in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.5,
        0.0,
        0.499,
        2.001,
    ] {
        assert!(SourcePlayback::scalar_continuation(anchor, domain, invalid, 1.0).is_none());
        assert!(SourcePlayback::scalar_continuation(anchor, domain, 1.0, invalid).is_none());
    }
    for fraction in [f64::NAN, f64::INFINITY, -0.25, 1.0, 1.25] {
        assert!(
            SourcePlayback::scalar_continuation(position(10, fraction), domain, 1.0, 1.0).is_none()
        );
    }
    for frame in [0, 6, 24, usize::MAX] {
        assert!(
            SourcePlayback::scalar_continuation(position(frame, 0.25), domain, 1.0, 1.0).is_none()
        );
    }
    for seek_mode in [ExplicitSeekMode::BeforeLoop, ExplicitSeekMode::AfterLoop] {
        assert!(
            SourcePlayback::scalar_continuation(
                FractionalSourcePosition {
                    seek_mode,
                    ..anchor
                },
                domain,
                1.0,
                1.0
            )
            .is_none()
        );
    }
    for invalid in [
        physical(7, 7, 31),
        physical(24, 7, 31),
        physical(7, 32, 31),
        physical(0, 1, 0),
        SourceLoopDomain {
            musical_period: Some(17.0),
            ..domain
        },
        SourceLoopDomain {
            musical_period: Some(17.25),
            ..domain
        },
        SourceLoopDomain {
            musical_period: Some(f64::NAN),
            ..domain
        },
    ] {
        assert!(SourcePlayback::scalar_continuation(anchor, invalid, 1.0, 1.0).is_none());
    }
    for ratio in [0.5, 2.0] {
        let playback = SourcePlayback::scalar_continuation(anchor, domain, ratio, ratio).unwrap();
        exact(playback.position(), anchor);
        assert_eq!(playback.tempo_ratio().to_bits(), ratio.to_bits());
        assert_eq!(playback.rate_target().to_bits(), ratio.to_bits());
    }
}

#[test]
fn fractional_identity_preserves_signed_zero_and_tiny_bits_at_every_integer_revisit() {
    let domain = physical(7, 24, 31);
    for fraction in [-0.0, 0.0, f64::from_bits(1), 2.0_f64.powi(-70), 0.25] {
        let anchor = position(10, fraction);
        let initial = SourcePlayback::scalar_continuation(anchor, domain, 1.0, 1.0).unwrap();
        for (elapsed, expected_frame) in [(0, 10), (1, 11), (17, 10), (34, 10), (51, 10)] {
            exact(
                initial.position_at(elapsed).unwrap(),
                position(expected_frame, fraction),
            );
            let mut copied = initial;
            assert!(copied.advance(elapsed));
            exact(copied.position(), position(expected_frame, fraction));
        }
    }
}

#[test]
fn dyadic_physical_motion_carries_and_wraps_without_an_absolute_float_coordinate() {
    let domain = physical(7, 12, 20);
    let initial =
        SourcePlayback::scalar_continuation(position(11, 0.75), domain, 0.5, 0.5).unwrap();
    for (elapsed, frame, fraction) in [
        (0, 11, 0.75),
        (1, 7, 0.25),
        (2, 7, 0.75),
        (7, 10, 0.25),
        (10, 11, 0.75),
    ] {
        exact(
            initial.position_at(elapsed).unwrap(),
            position(frame, fraction),
        );
    }
    // The exact fraction is 1 - 2^-54, halfway between the last fraction below
    // one and one. Round-to-nearest-even carries to the loop start. Its circular
    // error is 2^-54 frame; ordinary absolute source-coordinate error is not small.
    let rounded = SourcePlayback::scalar_continuation(
        position(11, 0.5 - 2.0_f64.powi(-54)),
        domain,
        0.5,
        0.5,
    )
    .unwrap();
    exact(rounded.position_at(1).unwrap(), position(7, 0.0));
}

#[cfg(target_pointer_width = "64")]
#[test]
fn complete_u64_epoch_and_giant_source_offset_keep_separate_pair_coordinates() {
    let start = (1usize << 54) + 13;
    let domain = physical(start, start + 17, start + 29);
    let fraction = f64::from_bits(1);
    let anchor = position(start + 3, fraction);
    let initial = SourcePlayback::scalar_continuation(anchor, domain, 2.0, 2.0).unwrap();
    // 2^64 - 1 is divisible by 17; twice that displacement revisits the anchor.
    exact(initial.position_at(u64::MAX as usize).unwrap(), anchor);
    let mut copied = initial;
    assert!(copied.advance(u64::MAX as usize));
    exact(copied.position(), anchor);
    let fractional =
        SourcePlayback::scalar_continuation(position(start + 3, 0.25), domain, 0.5, 0.5).unwrap();
    // floor((2^64 - 1)/2) == 2^63 - 1 == 8 mod 17, with fraction 1/2.
    exact(
        fractional.position_at(u64::MAX as usize).unwrap(),
        position(start + 11, 0.75),
    );
    let faster =
        SourcePlayback::scalar_continuation(position(start + 3, 0.75), domain, 1.5, 1.5).unwrap();
    exact(
        faster.position_at(u64::MAX as usize).unwrap(),
        position(start + 12, 0.25),
    );
    let fine = 1.0 + 2.0_f64.powi(-52);
    let fine =
        SourcePlayback::scalar_continuation(position(start + 3, 0.0), domain, fine, fine).unwrap();
    // n*(1+2^-52) == n+4095+(1-2^-52). Since n==0 mod17 and
    // 4095==15 mod17, the separate integer anchor 3 wraps to relative 1.
    exact(
        fine.position_at(u64::MAX as usize).unwrap(),
        position(start + 1, 1.0 - 2.0_f64.powi(-52)),
    );
}

#[test]
fn actual_physical_reader_taps_and_pcm_keep_tiny_fraction_at_anchor_and_revisit() {
    let sample = SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]),
    };
    let tiny = 2.0_f64.powi(-54);
    let mut playback =
        SourcePlayback::scalar_continuation(position(2, tiny), physical(2, 5, 8), 1.0, 1.0)
            .unwrap();
    let original = playback;
    for elapsed in [0, 3, 6] {
        playback = original;
        assert!(playback.advance(elapsed));
        let plan = read_plan(playback);
        let taps = plan.fractional_taps(playback.position().fraction);
        assert_eq!(taps.left, 2);
        assert_eq!(taps.right, Some(3));
        assert_eq!(taps.right_gain.to_bits(), (tiny as f32).to_bits());
        let mut buffers = [vec![-1.0; 3]];
        plan.fill_fractional_buffers(&sample, None, &playback, &mut buffers, 3)
            .unwrap();
        assert_eq!(buffers[0], vec![tiny as f32, 1.0, 0.0]);
        exact(playback.position(), position(2, tiny));
    }
    #[cfg(target_pointer_width = "64")]
    {
        playback = original;
        // 2^54+2 is a multiple of three, beyond binary64 integer precision.
        assert!(playback.advance((1usize << 54) + 2));
        let mut buffers = [vec![-1.0; 3]];
        read_plan(playback)
            .fill_fractional_buffers(&sample, None, &playback, &mut buffers, 3)
            .unwrap();
        assert_eq!(buffers[0], vec![tiny as f32, 1.0, 0.0]);
    }
}

#[test]
fn pair_mode_guards_history_and_does_not_equal_a_legacy_epoch_with_the_same_cursor() {
    let domain = physical(7, 24, 31);
    let mut pair =
        SourcePlayback::scalar_continuation(position(10, 0.0), domain, 1.0, 1.0).unwrap();
    let mut legacy = SourcePlayback::new(10, ExplicitSeekMode::Normal, 1.0);
    assert!(legacy.configure_domain(domain));
    // Isolate the mode comparison from the independent history permission bit.
    legacy.history_reconstructible = false;
    exact(pair.position(), legacy.position());
    assert!(!pair.matches_exact(&legacy));
    assert!(!pair.matches_rate_target(&legacy));
    assert!(!legacy.matches_exact(&pair));
    assert!(!legacy.matches_rate_target(&pair));
    assert!(pair.history_before_constant(1).is_none());
    assert!(pair.shifted_normal_history(0.0).is_none());
    assert!(pair.advance(17));
    let before_clear = pair;
    pair.clear_explicit_seek();
    assert!(pair.matches_exact(&before_clear));
    let (mut history, silence) = pair.history_before_constant(13).unwrap();
    assert_eq!(silence, 0);
    assert!(history.advance(13));
    assert!(history.matches_exact(&pair));
    assert!(pair.history_before_constant(18).is_none());
    let prepared = pair.at_constant_ratio(0.75);
    exact(prepared.position(), pair.position());
    assert!(prepared.history_before_constant(1).is_none());
    assert!(prepared.shifted_normal_history(1.0).is_none());
}

#[test]
fn unsupported_pair_domain_change_refuses_before_mutation_and_explicit_seek_is_legacy() {
    let domain = physical(2, 10, 12);
    let mut pair =
        SourcePlayback::scalar_continuation(position(4, 2.0_f64.powi(-60)), domain, 1.0, 1.0)
            .unwrap();
    assert!(pair.advance(1));
    let before = pair;
    assert!(pair.configure_domain(domain));
    assert!(pair.matches_exact(&before));
    for invalid in [
        physical(3, 9, 12),
        physical(6, 10, 12),
        physical(2, 13, 12),
        physical(8, 2, 12),
        SourceLoopDomain {
            musical_period: Some(8.0),
            ..domain
        },
    ] {
        assert!(!pair.configure_domain(invalid));
        assert!(pair.matches_exact(&before));
    }
    assert_eq!(pair.scalar_projection, ScalarProjection::PairPhysical);
    assert!(pair.history_before_constant(2).is_none());
    assert!(pair.seek(0, ExplicitSeekMode::BeforeLoop));
    assert_eq!(pair.scalar_projection, ScalarProjection::Legacy);
    assert!(pair.configure_domain(domain));
    let mut legacy = SourcePlayback::new(0, ExplicitSeekMode::BeforeLoop, 1.0);
    assert!(legacy.configure_domain(domain));
    assert!(pair.matches_exact(&legacy));
}

#[test]
fn glide_copies_real_reader_spans_and_checkpoints_keep_the_same_512_frame_epochs() {
    let domain = physical(5, 22, 24);
    let initial =
        SourcePlayback::scalar_continuation(position(8, 2.0_f64.powi(-54)), domain, 1.0, 1.2)
            .unwrap();
    let sample = SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from((0..24).map(|frame| frame as f32).collect::<Vec<_>>()),
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
                    trace.push((
                        ratio.to_bits(),
                        playback.position_at(frame).unwrap(),
                        sample.to_bits(),
                    ));
                }
                for frame in 0..suffix.frames {
                    exact(
                        suffix.playback.position_at(frame).unwrap(),
                        playback.position_at(offset + frame).unwrap(),
                    );
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
    exact(reference[0].1, initial.position());
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
        let (actual, clock) = run(partition);
        for (actual, expected) in actual.iter().zip(&reference) {
            assert_eq!(actual.0, expected.0);
            exact(actual.1, expected.1);
            assert_eq!(actual.2, expected.2);
        }
        assert!(clock.matches_exact(&final_clock));
    }
    let mut checkpoint = SourceReadCheckpoint {
        playback: initial,
        plan: read_plan(initial),
    };
    assert!(checkpoint.project_output_frames(1733));
    assert!(checkpoint.is_consistent());
    assert!(checkpoint.playback.matches_exact(&final_clock));
    assert!(final_clock.history_before_constant(198).is_none());
    let (mut history, _) = final_clock.history_before_constant(197).unwrap();
    exact(history.position(), reference[1536].1);
    let mut history_buffers = [vec![0.0; 197]];
    read_plan(history)
        .fill_fractional_buffers(&sample, None, &history, &mut history_buffers, 197)
        .unwrap();
    for (actual, expected) in history_buffers[0].iter().zip(&reference[1536..]) {
        assert_eq!(actual.to_bits(), expected.2);
    }
    assert!(history.advance(197));
    assert!(history.matches_exact(&final_clock));
}

#[cfg(target_pointer_width = "64")]
#[test]
fn pair_epoch_exhaustion_refuses_before_advance_read_or_checkpoint_mutation() {
    let mut playback =
        SourcePlayback::scalar_continuation(position(2, -0.0), physical(2, 5, 8), 1.0, 1.0)
            .unwrap();
    assert!(playback.advance(u64::MAX as usize));
    let before = playback;
    assert!(!playback.advance(1));
    assert!(playback.matches_exact(&before));
    assert_eq!(playback.position_at(1), None);
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
    let mut legacy = SourcePlayback::new(0, ExplicitSeekMode::Normal, 1.0);
    assert!(legacy.advance(u64::MAX as usize));
    assert!(legacy.advance(1));
    assert_eq!(legacy.elapsed_output_frames, u64::MAX);
}

#[test]
fn physical_pair_mode_remains_a_bounded_nonowning_copy() {
    fn copy<T: Copy>() {}
    copy::<SourcePlayback>();
    assert!(!std::mem::needs_drop::<SourcePlayback>());
    assert!(
        std::mem::size_of::<[Option<SourcePlayback>; crate::audio_engine::constants::MAX_VOICES]>()
            <= 16 * 1024
    );
}
