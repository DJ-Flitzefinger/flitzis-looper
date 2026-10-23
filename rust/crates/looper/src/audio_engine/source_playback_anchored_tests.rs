//! Bounded reference-coordinate integration, without source/adoption or RT permission.

use super::*;
use crate::audio_engine::mapped_source_clock::tests::map_with_source;
use crate::audio_engine::source_reader::{
    SourceReadPlan, StemReadSets, StemRenderSelection, StemTransition,
};
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::SampleBuffer;

const RATE: u32 = 48_000;
const FRAMES: usize = 96_000;
const MARKER: u64 = 37;

fn position(frame: usize, fraction: f64) -> FractionalSourcePosition {
    FractionalSourcePosition {
        frame,
        fraction,
        seek_mode: ExplicitSeekMode::Normal,
    }
}

fn assert_position(actual: FractionalSourcePosition, expected: FractionalSourcePosition) {
    assert_eq!(actual.frame, expected.frame);
    assert_eq!(actual.fraction.to_bits(), expected.fraction.to_bits());
    assert_eq!(actual.seek_mode, expected.seek_mode);
}

fn certified_geometry(
    anchors: &[(f64, f64)],
    rate: u32,
    frames: usize,
    region: FrameRange,
    quarters: f64,
) -> MappedLoopGeometry {
    let coverage = vec!["trusted"; anchors.len() - 1];
    MappedLoopGeometry::new(
        map_with_source(anchors, &coverage, false, rate, frames),
        frames,
        rate,
        region,
        quarters,
        false,
    )
    .unwrap()
    .qualify_exact_continuation()
    .unwrap()
}

fn variable_geometry() -> MappedLoopGeometry {
    certified_geometry(
        &[(0.0, 0.0), (0.5, 1.0), (1.0, 4.0)],
        RATE,
        FRAMES,
        FrameRange {
            start: 0,
            end: 48_000,
        },
        4.0,
    )
}

fn timeline() -> TransportTimeline {
    let mut timeline = TransportTimeline::new(RATE);
    assert!(timeline.set_master_period(16_000.0 / f64::from(RATE)));
    assert_eq!(timeline.frames_per_beat(), Some(16_000.0));
    timeline
}

fn large_frame_timeline() -> TransportTimeline {
    let mut timeline = timeline();
    assert!(timeline.anchor_beat_position_at_frame(3.125, (1_u64 << 53) + 1));
    timeline
}

fn anchored(
    geometry: &MappedLoopGeometry,
    timeline: TransportTimeline,
    raw: FractionalSourcePosition,
    frame: u64,
) -> SourcePlayback {
    SourcePlayback::mapped_anchored_continuation(
        geometry,
        MARKER,
        timeline,
        geometry.domain(),
        raw,
        frame,
    )
    .unwrap()
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

fn compare_original_reader(sample: &SampleBuffer, mapped: SourcePlayback) {
    let raw = mapped.position();
    let scalar = SourcePlayback::scalar_musical_continuation(
        raw,
        mapped.domain.unwrap(),
        mapped.tempo_ratio(),
        mapped.tempo_ratio(),
    )
    .unwrap();
    assert_position(scalar.position(), raw);
    let mapped_plan = read_plan(mapped);
    let scalar_plan = read_plan(scalar);
    assert!(mapped_plan.matches_exact(scalar_plan));
    let mapped_taps = mapped_plan.fractional_taps(raw.fraction);
    let scalar_taps = scalar_plan.fractional_taps(raw.fraction);
    assert_eq!(mapped_taps.left, scalar_taps.left);
    assert_eq!(mapped_taps.right, scalar_taps.right);
    assert_eq!(
        mapped_taps.right_gain.to_bits(),
        scalar_taps.right_gain.to_bits()
    );
    let mut mapped_output = [vec![-99.0; 1]];
    let mut scalar_output = [vec![-98.0; 1]];
    mapped_plan
        .fill_fractional_buffers_checked_with_readers(
            sample,
            StemReadSets::default(),
            &mapped,
            &mut mapped_output,
            1,
        )
        .unwrap();
    scalar_plan
        .fill_fractional_buffers_checked_with_readers(
            sample,
            StemReadSets::default(),
            &scalar,
            &mut scalar_output,
            1,
        )
        .unwrap();
    assert_eq!(mapped_output[0][0].to_bits(), scalar_output[0][0].to_bits());
}

#[test]
fn anchored_copies_and_unequal_partitions_retain_global_knots_and_wraps() {
    let geometry = variable_geometry();
    let original = anchored(
        &geometry,
        timeline(),
        position(23_999, 1.0_f64.next_down()),
        0,
    );
    let epoch = original.mapped_epoch().unwrap();
    let run = |partitions: &[usize]| {
        let mut playback = original;
        let mut total = 0;
        let mut partition = 0;
        while total < 64_002 {
            let requested = partitions[partition % partitions.len()].min(64_002 - total);
            let mut remaining = requested;
            while remaining > 0 {
                let (frames, _) = playback.chunk(remaining);
                if frames == 0 {
                    playback.refresh_mapped_span(&geometry).unwrap();
                    continue;
                }
                let start = playback.mapped_output_frame().unwrap();
                for offset in [0, frames / 2, frames] {
                    assert_position(
                        playback.position_at(offset).unwrap(),
                        geometry.position_at(epoch, start + offset as u64).unwrap(),
                    );
                }
                let mut copied = playback;
                assert!(copied.advance(frames));
                assert!(playback.advance(frames));
                assert!(playback.matches_exact(&copied));
                total += frames;
                remaining -= frames;
            }
            partition += 1;
        }
        playback.refresh_mapped_span(&geometry).unwrap();
        playback
    };
    let reference = run(&[20_003, 43_999]);
    let partitioned = run(&[1, 15_999, 16_000, 16_000, 16_002]);
    assert!(reference.matches_exact(&partitioned));
    assert!(reference.matches_rate_target(&original));
    assert_eq!(original.mapped_output_frame(), Some(0));
    assert_position(original.position(), position(23_999, 1.0_f64.next_down()));
}

#[test]
fn anchored_raw_pairs_keep_the_unchanged_reader_taps_gain_and_first_pcm() {
    let geometry = variable_geometry();
    let sample = SampleBuffer {
        samples: (0..FRAMES)
            .map(|frame| [0.25, -0.75, 0.5][frame % 3])
            .collect::<Vec<_>>()
            .into(),
        channels: 1,
        residency: None,
    };
    for raw in [
        position(0, -0.0),
        position(12_000, f64::from_bits(1)),
        position(23_999, 1.0_f64.next_down()),
    ] {
        let playback = anchored(&geometry, timeline(), raw, 17);
        assert_position(playback.position(), raw);
        compare_original_reader(&sample, playback);
    }
}

#[test]
fn anchored_virtual_cursor_revisit_retains_original_period_and_reader_pcm() {
    let geometry = certified_geometry(
        &[(0.0, 0.0), (9.0 / 32_768.0, 2.0_f64.powi(-13))],
        8192,
        3,
        FrameRange { start: 0, end: 2 },
        2.0_f64.powi(-13),
    );
    let sample = SampleBuffer {
        samples: vec![0.25, -0.75, 0.5].into(),
        channels: 1,
        residency: None,
    };
    let raw = position(2, 0.125);
    let mut playback = anchored(&geometry, timeline(), raw, 0);
    assert_eq!(playback.loop_period(), Some(2.25));
    let taps = read_plan(playback).fractional_taps(raw.fraction);
    assert_eq!(taps.left, 1);
    assert_eq!(taps.right, Some(0));
    assert_eq!(taps.right_gain.to_bits(), 0.9_f32.to_bits());
    compare_original_reader(&sample, playback);
    playback.project_mapped_to(&geometry, 125).unwrap();
    assert_position(playback.position(), raw);
    assert_eq!(playback.loop_period(), Some(2.25));
    compare_original_reader(&sample, playback);
}

#[test]
fn anchored_pause_translation_repeats_the_original_address_without_reanchoring() {
    let geometry = variable_geometry();
    let timeline = large_frame_timeline();
    let e = 1_u64 << 53;
    let raw = position(23_999, 1.0_f64.next_down());
    let original = anchored(&geometry, timeline, raw, e);
    let epoch = original.mapped_epoch().unwrap();
    let mut playback = original;
    playback.project_mapped_to(&geometry, e + 2).unwrap();
    let paused = playback.position();
    assert_position(
        paused,
        position(24_001, f64::from_bits(0x3d75_7fd5_5555_5555)),
    );
    playback
        .resume_mapped_at(&geometry, &timeline, e + 2 + 12_345)
        .unwrap();
    assert_position(playback.position(), paused);
    playback
        .project_mapped_to(&geometry, e + 3 + 12_345)
        .unwrap();
    assert_position(
        playback.position(),
        geometry.position_at(epoch, e + 3).unwrap(),
    );
    let before = playback;
    assert!(playback.project_mapped_to(&geometry, e + 12_344).is_err());
    assert!(playback.matches_exact(&before));
    assert!(playback.resume_mapped_at(&geometry, &timeline, e).is_err());
    assert!(playback.matches_exact(&before));
    let mut stale = timeline;
    assert!(stale.set_master_period(0.25));
    assert!(
        playback
            .resume_mapped_at(&geometry, &stale, e + 20_000)
            .is_err()
    );
    assert!(playback.matches_exact(&before));
}

#[test]
fn anchored_master_replacement_retains_frozen_m0_and_advances_history_floor() {
    let geometry = variable_geometry();
    let timeline = large_frame_timeline();
    let e = 1_u64 << 53;
    let c = e + 32_000;
    let mut playback = anchored(
        &geometry,
        timeline,
        position(23_999, 1.0_f64.next_down()),
        e,
    );
    playback.project_mapped_to(&geometry, c).unwrap();
    let old_epoch = playback.mapped_epoch().unwrap();
    assert_position(playback.position(), position(40_000, 0.0));
    let mut replacement_timeline = timeline;
    assert!(replacement_timeline.set_master_period_preserving_beat_position_at_frame(0.25, c));
    assert_eq!(replacement_timeline.frames_per_beat(), Some(12_000.0));
    let replacement_epoch = old_epoch.with_timeline_at(replacement_timeline, c).unwrap();
    playback
        .replace_mapped_epoch(&geometry, replacement_epoch)
        .unwrap();
    assert_position(playback.position(), position(40_000, 0.0));
    assert_eq!(playback.tempo_ratio().to_bits(), (2.0_f64 / 3.0).to_bits());
    // Recomputing original M0 under this projection would instead produce
    // (45333, f64::from_bits(0x3fd5_5555_5555_01ff)); exact continuity keeps40000.
    let before = playback;
    assert!(playback.project_mapped_to(&geometry, c - 1).is_err());
    assert!(playback.matches_exact(&before));
    assert!(playback.replace_mapped_epoch(&geometry, old_epoch).is_err());
    assert!(playback.matches_exact(&before));
    let mut discontinuous = replacement_timeline;
    assert!(discontinuous.anchor_beat_position_at_frame(9.125, c));
    assert!(
        replacement_epoch
            .with_timeline_at(discontinuous, c)
            .is_none()
    );
    assert!(playback.matches_exact(&before));
}

#[test]
fn anchored_replacement_refuses_new_anchor_marker_and_legacy_mode_at_equal_cursor() {
    let geometry = variable_geometry();
    let timeline = timeline();
    let original = anchored(&geometry, timeline, position(0, 0.0), 0);
    let mut playback = original;
    playback.project_mapped_to(&geometry, 4_000).unwrap();
    let before = playback;
    let reanchored = MappedClockEpoch::anchored(
        MARKER,
        timeline,
        &geometry,
        geometry.domain(),
        playback.position(),
        4_000,
    )
    .unwrap();
    assert_position(
        geometry.position_at(reanchored, 4_000).unwrap(),
        playback.position(),
    );
    assert!(
        playback
            .replace_mapped_epoch(&geometry, reanchored)
            .is_err()
    );
    assert!(playback.matches_exact(&before));
    let changed_marker = MappedClockEpoch::anchored(
        MARKER + 1,
        timeline,
        &geometry,
        geometry.domain(),
        position(0, 0.0),
        0,
    )
    .unwrap();
    assert!(
        playback
            .replace_mapped_epoch(&geometry, changed_marker)
            .is_err()
    );
    assert!(playback.matches_exact(&before));
    let legacy_epoch = MappedClockEpoch::new(MARKER, timeline, 0.0).unwrap();
    let mut legacy = SourcePlayback::mapped(&geometry, legacy_epoch, 0).unwrap();
    assert_position(legacy.position(), original.position());
    assert!(!legacy.matches_exact(&original));
    assert!(!legacy.matches_rate_target(&original));
    let legacy_before = legacy;
    assert!(
        legacy
            .replace_mapped_epoch(&geometry, original.mapped_epoch().unwrap())
            .is_err()
    );
    assert!(legacy.matches_exact(&legacy_before));
    assert!(
        playback
            .replace_mapped_epoch(&geometry, legacy_epoch)
            .is_err()
    );
    assert!(playback.matches_exact(&before));
    legacy.project_mapped_to(&geometry, 16_000).unwrap();
    assert_position(legacy.position(), position(24_000, 0.0));
}

#[test]
fn anchored_history_and_terminal_u64_failures_preserve_the_complete_copy() {
    let geometry = variable_geometry();
    let mut playback = anchored(&geometry, timeline(), position(17, 0.25), 17);
    let before = playback;
    assert!(playback.history_before_constant(1).is_none());
    assert!(playback.shifted_normal_history(1.0).is_none());
    assert!(!playback.set_target(1.0));
    assert!(!playback.seek(0, ExplicitSeekMode::Normal));
    assert!(
        !playback.configure_domain(SourceLoopDomain::physical(FRAMES, geometry.domain().region))
    );
    playback.clear_explicit_seek();
    assert!(playback.project_mapped_to(&geometry, 16).is_err());
    assert!(playback.matches_exact(&before));
    assert!(
        SourcePlayback::mapped_anchored_continuation(
            &geometry,
            MARKER,
            timeline(),
            geometry.domain(),
            position(17, 0.25),
            u64::MAX,
        )
        .is_err()
    );
    let mut terminal = anchored(&geometry, timeline(), position(17, 0.25), u64::MAX - 1);
    let before = terminal;
    assert!(!terminal.try_advance(2));
    assert!(terminal.matches_exact(&before));
    assert_eq!(terminal.chunk(2).0, 1);
    assert!(terminal.advance(1));
    assert_eq!(terminal.mapped_output_frame(), Some(u64::MAX));
    assert_position(terminal.position(), position(17, 0.25));
    assert_eq!(terminal.position_at(1), None);
    assert!(!terminal.can_read_frames(1));
    let before = terminal;
    assert!(!terminal.advance(1));
    assert!(terminal.refresh_mapped_span(&geometry).is_err());
    assert!(terminal.matches_exact(&before));
}

#[test]
fn anchored_reverse_coordinates_use_original_pairmusical_domain_and_existing_glide_once() {
    let geometry = variable_geometry();
    let mapped = anchored(
        &geometry,
        timeline(),
        position(12_000, f64::from_bits(1)),
        0,
    );
    let raw = mapped.position();
    let applied = mapped.tempo_ratio();
    assert_eq!(applied, 1.5);
    let mut scalar =
        SourcePlayback::scalar_musical_continuation(raw, geometry.domain(), applied, 0.5).unwrap();
    assert_position(scalar.position(), raw);
    assert_eq!(scalar.loop_period(), mapped.loop_period());
    assert!(scalar.history_before_constant(1).is_none());
    assert!(!scalar.matches_exact(&mapped));
    let mut copied = scalar;
    let (frames, ratio) = scalar.chunk(512);
    assert_eq!(frames, 512);
    assert_eq!(ratio.to_bits(), (applied - 0.05).to_bits());
    assert_position(scalar.position(), raw);
    assert_eq!(copied.chunk(17).1.to_bits(), ratio.to_bits());
    assert!(copied.advance(17));
    assert!(copied.advance(495));
    assert!(scalar.advance(512));
    assert!(scalar.matches_exact(&copied));
    let before_next_step = scalar.position();
    assert_eq!(scalar.chunk(512).1.to_bits(), (ratio - 0.05).to_bits());
    assert_position(scalar.position(), before_next_step);
    assert_eq!(mapped.mapped_output_frame(), Some(0));
    assert_position(mapped.position(), raw);
}
