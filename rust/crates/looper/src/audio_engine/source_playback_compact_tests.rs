//! Compact coordinates and consuming original-reader checks, without adoption or RT permission.

use super::*;
use crate::audio_engine::constants::MAX_VOICES;
use crate::audio_engine::mapped_source_clock::tests::map_with_source;
use crate::audio_engine::source_reader::{
    SourceReadPlan, StemReadSets, StemRenderSelection, StemTransition,
};
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::SampleBuffer;
use crate::test_memory::{ThreadAllocations, observe_current_thread};
use std::hint::black_box;
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
const FRAMES: usize = 96_000;
const BLOCK: usize = 512;
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

fn assert_complete_copy(actual: &SourcePlayback, expected: &SourcePlayback) {
    assert!(actual.matches_exact(expected));
    assert_position(actual.origin, expected.origin);
    assert_eq!(actual.domain, expected.domain);
    assert_eq!(actual.elapsed_output_frames, expected.elapsed_output_frames);
    assert_eq!(actual.ratio.to_bits(), expected.ratio.to_bits());
    assert_eq!(actual.target.to_bits(), expected.target.to_bits());
    assert_eq!(actual.frames_until_step, expected.frames_until_step);
    assert_eq!(
        actual.history_reconstructible,
        expected.history_reconstructible
    );
    assert_eq!(actual.scalar_projection, expected.scalar_projection);
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

fn virtual_geometry() -> MappedLoopGeometry {
    certified_geometry(
        &[(0.0, 0.0), (9.0 / 32_768.0, 2.0_f64.powi(-13))],
        8192,
        FRAMES,
        FrameRange { start: 0, end: 2 },
        2.0_f64.powi(-13),
    )
}

fn timeline(large: bool) -> TransportTimeline {
    let mut timeline = TransportTimeline::new(RATE);
    assert!(timeline.set_master_period(16_000.0 / f64::from(RATE)));
    assert_eq!(timeline.frames_per_beat(), Some(16_000.0));
    if large {
        assert!(timeline.anchor_beat_position_at_frame(3.125, (1_u64 << 53) + 1));
    }
    timeline
}

// The original wide constructor is deliberately confined to off-observer setup.
fn epoch(
    geometry: &MappedLoopGeometry,
    timeline: TransportTimeline,
    raw: FractionalSourcePosition,
    first: u64,
) -> MappedClockEpoch {
    MappedClockEpoch::anchored(MARKER, timeline, geometry, geometry.domain(), raw, first).unwrap()
}

fn plan(playback: SourcePlayback) -> SourceReadPlan {
    let domain = playback.domain.unwrap();
    let position = playback.position();
    SourceReadPlan {
        channels: 2,
        sample_frames: domain.sample_frames,
        frame_pos: position.frame,
        loop_region: domain.region,
        loop_period: domain.musical_period,
        seek_mode: position.seek_mode,
        selection: StemRenderSelection::full_mix(),
        transition: StemTransition::default(),
    }
}

fn sample() -> SampleBuffer {
    SampleBuffer {
        samples: (0..FRAMES)
            .flat_map(|frame| {
                let value = [0.25, -0.75, 0.5][frame % 3];
                [value, -value * 0.5]
            })
            .collect::<Vec<_>>()
            .into(),
        channels: 2,
        residency: None,
    }
}

fn buffers(lanes: usize) -> Vec<[Vec<f32>; 2]> {
    (0..lanes)
        .map(|_| [vec![f32::NAN; BLOCK], vec![f32::NAN; BLOCK]])
        .collect()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    chunks: usize,
    refreshes: usize,
    reader_coordinates: usize,
    endpoint_coordinates: usize,
}

impl Work {
    fn add(&mut self, other: Self) {
        self.chunks += other.chunks;
        self.refreshes += other.refreshes;
        self.reader_coordinates += other.reader_coordinates;
        self.endpoint_coordinates += other.endpoint_coordinates;
    }
}

#[derive(Clone, Copy)]
enum Reader {
    Full,
    Checked,
}

impl Reader {
    fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Checked => "checked_two_pass",
        }
    }

    fn passes(self) -> usize {
        match self {
            Self::Full => 1,
            Self::Checked => 2,
        }
    }
}

// These are explicit harness requests, not an instrumented callback call trace.
// Every iteration consumes a positive chunk, including after a knot/wrap refresh.
fn render_compact(
    geometry: &MappedLoopGeometry,
    sample: &SampleBuffer,
    playbacks: &mut [SourcePlayback],
    scratch: &mut [[Vec<f32>; 2]],
    output: &mut [[Vec<f32>; 2]],
    work: &mut [Work],
    reader: Reader,
) {
    for (lane, playback) in playbacks.iter_mut().enumerate() {
        let mut rendered = 0;
        work[lane] = Work::default();
        while rendered < BLOCK {
            let remaining = BLOCK - rendered;
            let (mut frames, _) = playback.chunk(remaining);
            if frames == 0 {
                playback.refresh_mapped_span_compact(geometry).unwrap();
                work[lane].refreshes += 1;
                frames = playback.chunk(remaining).0;
            }
            assert!(frames > 0 && frames <= remaining);
            let read_plan = plan(*playback);
            black_box(playback.position_at_compact(geometry, black_box(frames))).unwrap();
            let mut next = *playback;
            assert!(black_box(next.advance_compact(geometry, black_box(frames))));
            match reader {
                Reader::Full => read_plan.fill_fractional_buffers_compact_with_readers(
                    sample,
                    StemReadSets::default(),
                    playback,
                    geometry,
                    &mut scratch[lane],
                    frames,
                ),
                Reader::Checked => read_plan.fill_fractional_buffers_checked_compact_with_readers(
                    sample,
                    StemReadSets::default(),
                    playback,
                    geometry,
                    &mut scratch[lane],
                    frames,
                ),
            }
            .unwrap();
            for (destination, source) in output[lane].iter_mut().zip(&scratch[lane]) {
                destination[rendered..rendered + frames].copy_from_slice(&source[..frames]);
            }
            *playback = next;
            rendered += frames;
            work[lane].chunks += 1;
            work[lane].reader_coordinates += frames * reader.passes();
            work[lane].endpoint_coordinates += 2;
        }
        assert_eq!(rendered, BLOCK);
        black_box(playback.position());
        black_box(&output[lane]);
    }
}

// Unchanged wide public APIs provide a separate reference outside compact observations.
fn render_reference(
    geometry: &MappedLoopGeometry,
    sample: &SampleBuffer,
    playbacks: &mut [SourcePlayback],
    scratch: &mut [[Vec<f32>; 2]],
    output: &mut [[Vec<f32>; 2]],
    work: &mut [Work],
    reader: Reader,
) {
    for (lane, playback) in playbacks.iter_mut().enumerate() {
        let mut rendered = 0;
        work[lane] = Work::default();
        while rendered < BLOCK {
            let remaining = BLOCK - rendered;
            let (mut frames, _) = playback.chunk(remaining);
            if frames == 0 {
                playback.refresh_mapped_span(geometry).unwrap();
                work[lane].refreshes += 1;
                frames = playback.chunk(remaining).0;
            }
            assert!(frames > 0 && frames <= remaining);
            let read_plan = plan(*playback);
            playback.position_at(frames).unwrap();
            let mut next = *playback;
            assert!(next.advance(frames));
            match reader {
                Reader::Full => read_plan.fill_fractional_buffers_with_readers(
                    sample,
                    StemReadSets::default(),
                    playback,
                    &mut scratch[lane],
                    frames,
                ),
                Reader::Checked => read_plan.fill_fractional_buffers_checked_with_readers(
                    sample,
                    StemReadSets::default(),
                    playback,
                    &mut scratch[lane],
                    frames,
                ),
            }
            .unwrap();
            for (destination, source) in output[lane].iter_mut().zip(&scratch[lane]) {
                destination[rendered..rendered + frames].copy_from_slice(&source[..frames]);
            }
            *playback = next;
            rendered += frames;
            work[lane].chunks += 1;
            work[lane].reader_coordinates += frames * reader.passes();
            work[lane].endpoint_coordinates += 2;
        }
    }
}

fn assert_pcm(actual: &[[Vec<f32>; 2]], expected: &[[Vec<f32>; 2]]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        for (actual, expected) in actual.iter().zip(expected) {
            for (actual, expected) in actual.iter().zip(expected) {
                assert!(actual.is_finite());
                assert_eq!(actual.to_bits(), expected.to_bits());
            }
        }
    }
}

fn assert_first_pcm(sample: &SampleBuffer, initial: SourcePlayback, actual: &[Vec<f32>; 2]) {
    let raw = initial.position();
    let scalar = SourcePlayback::scalar_musical_continuation(
        raw,
        initial.domain.unwrap(),
        initial.tempo_ratio(),
        initial.tempo_ratio(),
    )
    .unwrap();
    let mapped_taps = plan(initial).fractional_taps(raw.fraction);
    let scalar_taps = plan(scalar).fractional_taps(raw.fraction);
    assert_eq!(mapped_taps.left, scalar_taps.left);
    assert_eq!(mapped_taps.right, scalar_taps.right);
    assert_eq!(
        mapped_taps.right_gain.to_bits(),
        scalar_taps.right_gain.to_bits()
    );
    let mut expected = [vec![f32::NAN; 1], vec![f32::NAN; 1]];
    plan(scalar)
        .fill_fractional_buffers_checked_with_readers(
            sample,
            StemReadSets::default(),
            &scalar,
            &mut expected,
            1,
        )
        .unwrap();
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual[0].to_bits(), expected[0].to_bits());
    }
}

#[test]
fn compact_exact_pairs_match_independent_knots_wraps_and_large_epoch_fixtures() {
    let geometry = variable_geometry();
    for first in [17, 1_u64 << 53] {
        let timeline = timeline(first > 17);
        for raw in [
            position(0, -0.0),
            position(17, 0.25),
            position(23_999, 1.0_f64.next_down()),
        ] {
            let epoch = epoch(&geometry, timeline, raw, first);
            let mut compact = SourcePlayback::mapped_compact(&geometry, epoch, first).unwrap();
            assert_position(compact.position(), raw);
            for delta in [0, 1, 2, 15_999, 16_000, 32_000, 64_001] {
                compact
                    .project_mapped_compact_to(&geometry, first + delta)
                    .unwrap();
                assert_position(
                    compact.position(),
                    geometry.position_at(epoch, first + delta).unwrap(),
                );
                assert_position(
                    compact.position_at_compact(&geometry, 0).unwrap(),
                    compact.position(),
                );
            }
        }
    }
    // These exact pairs are independent recorded fixtures from the anchored contract.
    let first = 1_u64 << 53;
    let epoch = epoch(
        &geometry,
        timeline(true),
        position(23_999, 1.0_f64.next_down()),
        first,
    );
    let mut compact = SourcePlayback::mapped_compact(&geometry, epoch, first).unwrap();
    compact
        .project_mapped_compact_to(&geometry, first + 2)
        .unwrap();
    assert_position(
        compact.position(),
        position(24_001, f64::from_bits(0x3d75_7fd5_5555_5555)),
    );
    compact
        .project_mapped_compact_to(&geometry, first + 32_000)
        .unwrap();
    assert_position(compact.position(), position(40_000, 0.0));
}

#[test]
fn compact_unequal_partitions_keep_exact_copies_and_borrowed_view_snapshots() {
    let geometry = variable_geometry();
    let first = 17;
    let epoch = epoch(
        &geometry,
        timeline(false),
        position(23_999, 1.0_f64.next_down()),
        first,
    );
    let initial = SourcePlayback::mapped_compact(&geometry, epoch, first).unwrap();
    let run = |partitions: &[usize]| {
        let mut playback = initial;
        let mut consumed = 0;
        let mut partition = 0;
        while consumed < 64_002 {
            let requested = partitions[partition % partitions.len()].min(64_002 - consumed);
            let mut remaining = requested;
            while remaining > 0 {
                let (mut frames, _) = playback.chunk(remaining);
                if frames == 0 {
                    playback.refresh_mapped_span_compact(&geometry).unwrap();
                    frames = playback.chunk(remaining).0;
                }
                assert!(frames > 0 && frames <= remaining);
                let snapshot = playback.compact_view(&geometry).unwrap();
                for offset in [0, frames / 2, frames] {
                    assert_position(
                        snapshot.position_at(offset).unwrap(),
                        geometry
                            .position_at(epoch, first + (consumed + offset) as u64)
                            .unwrap(),
                    );
                }
                let mut copy = playback;
                assert!(copy.advance_compact(&geometry, frames));
                assert!(playback.advance_compact(&geometry, frames));
                assert!(playback.matches_exact(&copy));
                // A borrowed view is a snapshot; advancing another copy does not mutate it.
                assert_position(
                    snapshot.position_at(0).unwrap(),
                    geometry
                        .position_at(epoch, first + consumed as u64)
                        .unwrap(),
                );
                remaining -= frames;
                consumed += frames;
            }
            partition += 1;
        }
        playback.refresh_mapped_span_compact(&geometry).unwrap();
        playback
    };
    let reference = run(&[20_003, 43_999]);
    let partitioned = run(&[1, 15_999, 16_000, 16_000, 16_002]);
    assert!(reference.matches_exact(&partitioned));
    let mut wide = SourcePlayback::mapped(&geometry, epoch, first).unwrap();
    assert!(initial.matches_exact(&wide));
    let mut remaining = 64_002;
    while remaining > 0 {
        let (mut frames, _) = wide.chunk(remaining);
        if frames == 0 {
            wide.refresh_mapped_span(&geometry).unwrap();
            frames = wide.chunk(remaining).0;
        }
        assert!(frames > 0 && frames <= remaining);
        assert!(wide.advance(frames));
        remaining -= frames;
    }
    wide.refresh_mapped_span(&geometry).unwrap();
    assert!(reference.matches_exact(&wide));
    assert_position(reference.position(), wide.position());
    assert!(reference.matches_rate_target(&initial));
    assert_eq!(initial.mapped_output_frame(), Some(first));
}

#[test]
fn compact_virtual_pair_keeps_original_domain_taps_gain_and_revisited_pcm() {
    let geometry = virtual_geometry();
    let sample = sample();
    let raw = position(2, 0.125);
    let epoch = epoch(&geometry, timeline(false), raw, 0);
    let mut playback = SourcePlayback::mapped_compact(&geometry, epoch, 0).unwrap();
    assert_eq!(playback.loop_period(), Some(2.25));
    assert_eq!(
        playback.domain.unwrap().region,
        FrameRange { start: 0, end: 2 }
    );
    let taps = plan(playback).fractional_taps(raw.fraction);
    assert_eq!(taps.left, 1);
    assert_eq!(taps.right, Some(0));
    assert_eq!(taps.right_gain.to_bits(), 0.9_f32.to_bits());
    for frame in [0, 125] {
        playback
            .project_mapped_compact_to(&geometry, frame)
            .unwrap();
        assert_position(playback.position(), raw);
        let mut output = [vec![f32::NAN; 1], vec![f32::NAN; 1]];
        plan(playback)
            .fill_fractional_buffers_checked_compact_with_readers(
                &sample,
                StemReadSets::default(),
                &playback,
                &geometry,
                &mut output,
                1,
            )
            .unwrap();
        assert_first_pcm(&sample, playback, &output);
    }
}

#[test]
fn compact_refusals_preserve_complete_copy_and_checked_output() {
    let geometry = variable_geometry();
    let wrong_geometry = certified_geometry(
        &[(0.0, 0.0), (0.5, 2.0), (1.0, 4.0)],
        RATE,
        FRAMES,
        FrameRange {
            start: 0,
            end: 48_000,
        },
        4.0,
    );
    let sample = sample();
    let epoch = epoch(&geometry, timeline(false), position(17, 0.25), 17);
    let mut playback = SourcePlayback::mapped_compact(&geometry, epoch, 17).unwrap();
    let before = playback;
    let mut output = [vec![-91.0; BLOCK], vec![-92.0; BLOCK]];
    let unchanged = output.clone();
    assert!(playback.compact_view(&wrong_geometry).is_err());
    assert!(playback.position_at_compact(&wrong_geometry, 0).is_none());
    assert!(!playback.advance_compact(&wrong_geometry, 1));
    assert!(
        playback
            .refresh_mapped_span_compact(&wrong_geometry)
            .is_err()
    );
    assert!(
        playback
            .project_mapped_compact_to(&wrong_geometry, 18)
            .is_err()
    );
    assert!(playback.project_mapped_compact_to(&geometry, 16).is_err());
    assert!(!playback.advance_compact(&geometry, usize::MAX));
    assert_complete_copy(&playback, &before);
    assert_position(playback.position(), before.position());
    assert!(
        plan(playback)
            .fill_fractional_buffers_checked_compact_with_readers(
                &sample,
                StemReadSets::default(),
                &playback,
                &wrong_geometry,
                &mut output,
                1,
            )
            .is_err()
    );
    assert_eq!(output, unchanged);

    let mut stale = playback;
    stale.origin.fraction = 0.5;
    assert!(stale.compact_view(&geometry).is_err());
    assert!(
        plan(stale)
            .fill_fractional_buffers_checked_compact_with_readers(
                &sample,
                StemReadSets::default(),
                &stale,
                &geometry,
                &mut output,
                1,
            )
            .is_err()
    );
    assert_eq!(output, unchanged);
    let mut wrong_domain = playback;
    wrong_domain.domain = Some(SourceLoopDomain::physical(FRAMES, geometry.domain().region));
    assert!(wrong_domain.compact_view(&geometry).is_err());
    assert!(!wrong_domain.advance_compact(&geometry, 1));
    assert!(
        plan(wrong_domain)
            .fill_fractional_buffers_checked_compact_with_readers(
                &sample,
                StemReadSets::default(),
                &wrong_domain,
                &geometry,
                &mut output,
                1,
            )
            .is_err()
    );
    assert_eq!(output, unchanged);

    let span_frames = playback.chunk(usize::MAX).0;
    assert!(span_frames > 0 && span_frames < usize::MAX);
    assert!(!playback.advance_compact(&geometry, span_frames + 1));
    assert_complete_copy(&playback, &before);
    let mut exhausted = playback;
    assert!(exhausted.advance_compact(&geometry, span_frames));
    let exhausted_before = exhausted;
    assert!(
        plan(exhausted)
            .fill_fractional_buffers_checked_compact_with_readers(
                &sample,
                StemReadSets::default(),
                &exhausted,
                &geometry,
                &mut output,
                1,
            )
            .is_err()
    );
    assert_complete_copy(&exhausted, &exhausted_before);
    assert_eq!(output, unchanged);
    // A short destination is a separate original-reader contract failure.
    assert!(
        plan(playback)
            .fill_fractional_buffers_checked_compact_with_readers(
                &sample,
                StemReadSets::default(),
                &playback,
                &geometry,
                &mut output,
                BLOCK + 1,
            )
            .is_err()
    );
    assert_eq!(output, unchanged);
}

#[test]
fn compact_unrepresentable_raw_class_keeps_the_explicit_general_reference_available() {
    let geometry = variable_geometry();
    let sample = sample();
    let raw = position(12_000, f64::from_bits(1));
    let epoch = epoch(&geometry, timeline(false), raw, 17);
    let wide = SourcePlayback::mapped(&geometry, epoch, 17).unwrap();
    assert_position(wide.position(), raw);
    assert!(SourcePlayback::mapped_compact(&geometry, epoch, 17).is_err());
    let mut output = [vec![-91.0; 1], vec![-92.0; 1]];
    let unchanged = output.clone();
    assert!(
        plan(wide)
            .fill_fractional_buffers_checked_compact_with_readers(
                &sample,
                StemReadSets::default(),
                &wide,
                &geometry,
                &mut output,
                1,
            )
            .is_err()
    );
    assert_eq!(output, unchanged);
    plan(wide)
        .fill_fractional_buffers_checked_with_readers(
            &sample,
            StemReadSets::default(),
            &wide,
            &mut output,
            1,
        )
        .unwrap();
    assert_first_pcm(&sample, wide, &output);
}

#[test]
fn compact_whole_master_grid_refusal_preserves_explicit_wide_reader_access() {
    let geometry = certified_geometry(
        &[(0.0, 0.0), (2.0, 2.0_f64.powi(-100))],
        1,
        FRAMES,
        FrameRange { start: 0, end: 2 },
        2.0_f64.powi(-100),
    );
    let mut timeline = TransportTimeline::new(1);
    let frames_per_beat = 3.0 * 2.0_f64.powi(99);
    assert!(timeline.set_master_period(frames_per_beat));
    assert_eq!(timeline.frames_per_beat(), Some(frames_per_beat));
    // Both horizon endpoints fit a coarse grid. The conservative quotient grid
    // must still account for the finer actual division at interior frame one.
    let first_master = timeline.beat_position_at_frame(0).unwrap();
    let last_master = timeline.beat_position_at_frame(u64::MAX).unwrap();
    assert_eq!(first_master, 0.0);
    assert!(last_master.is_finite());
    assert_ne!(timeline.beat_position_at_frame(1).unwrap(), 0.0);
    let epoch = epoch(&geometry, timeline, position(0, -0.0), 0);
    let wide = SourcePlayback::mapped(&geometry, epoch, 0).unwrap();
    assert!(wide.position_at(1).is_some());
    assert!(SourcePlayback::mapped_compact(&geometry, epoch, 0).is_err());
    let sample = sample();
    let mut output = [vec![-91.0; 1], vec![-92.0; 1]];
    let unchanged = output.clone();
    assert!(
        plan(wide)
            .fill_fractional_buffers_checked_compact_with_readers(
                &sample,
                StemReadSets::default(),
                &wide,
                &geometry,
                &mut output,
                1,
            )
            .is_err()
    );
    assert_eq!(output, unchanged);
    plan(wide)
        .fill_fractional_buffers_checked_with_readers(
            &sample,
            StemReadSets::default(),
            &wide,
            &mut output,
            1,
        )
        .unwrap();
    assert_first_pcm(&sample, wide, &output);
}

#[test]
fn compact_pause_and_master_replacement_keep_original_m0_and_recertify_the_view() {
    let geometry = variable_geometry();
    let timeline = timeline(true);
    let first = 1_u64 << 53;
    let epoch = epoch(
        &geometry,
        timeline,
        position(23_999, 1.0_f64.next_down()),
        first,
    );
    let mut compact = SourcePlayback::mapped_compact(&geometry, epoch, first).unwrap();
    let mut wide = SourcePlayback::mapped(&geometry, epoch, first).unwrap();
    compact
        .project_mapped_compact_to(&geometry, first + 2)
        .unwrap();
    wide.project_mapped_to(&geometry, first + 2).unwrap();
    let paused = compact.position();
    compact
        .resume_mapped_compact_at(&geometry, &timeline, first + 12_347)
        .unwrap();
    wide.resume_mapped_at(&geometry, &timeline, first + 12_347)
        .unwrap();
    assert_position(compact.position(), paused);
    assert!(compact.matches_exact(&wide));
    compact
        .project_mapped_compact_to(&geometry, first + 12_348)
        .unwrap();
    wide.project_mapped_to(&geometry, first + 12_348).unwrap();
    assert!(compact.matches_exact(&wide));
    assert_position(
        compact.position(),
        geometry.position_at(epoch, first + 3).unwrap(),
    );
    let before = compact;
    let mut stale = timeline;
    assert!(stale.set_master_period(0.25));
    assert!(
        compact
            .resume_mapped_compact_at(&geometry, &stale, first + 30_000)
            .is_err()
    );
    assert!(
        compact
            .resume_mapped_compact_at(&geometry, &timeline, first)
            .is_err()
    );
    assert_complete_copy(&compact, &before);

    // A separate unpaused branch exercises the original cached M0 replacement fixture.
    let frame = first + 32_000;
    let mut compact = SourcePlayback::mapped_compact(&geometry, epoch, first).unwrap();
    let mut wide = SourcePlayback::mapped(&geometry, epoch, first).unwrap();
    compact.project_mapped_compact_to(&geometry, frame).unwrap();
    wide.project_mapped_to(&geometry, frame).unwrap();
    let mut replacement = timeline;
    assert!(replacement.set_master_period_preserving_beat_position_at_frame(0.25, frame));
    let replacement_epoch = epoch.with_timeline_at(replacement, frame).unwrap();
    compact
        .replace_mapped_epoch_compact(&geometry, replacement_epoch)
        .unwrap();
    wide.replace_mapped_epoch(&geometry, replacement_epoch)
        .unwrap();
    assert!(compact.matches_exact(&wide));
    assert_position(compact.position(), position(40_000, 0.0));
    assert_eq!(compact.tempo_ratio().to_bits(), (2.0_f64 / 3.0).to_bits());
    compact.compact_view(&geometry).unwrap();
    let before = compact;
    assert!(
        compact
            .project_mapped_compact_to(&geometry, frame - 1)
            .is_err()
    );
    assert!(
        compact
            .replace_mapped_epoch_compact(&geometry, epoch)
            .is_err()
    );
    assert_complete_copy(&compact, &before);
    compact
        .project_mapped_compact_to(&geometry, frame + 513)
        .unwrap();
    wide.project_mapped_to(&geometry, frame + 513).unwrap();
    assert!(compact.matches_exact(&wide));
}

#[test]
fn compact_terminal_u64_and_history_failures_are_atomic() {
    let geometry = variable_geometry();
    let epoch = epoch(&geometry, timeline(false), position(17, 0.25), u64::MAX - 1);
    let mut playback = SourcePlayback::mapped_compact(&geometry, epoch, u64::MAX - 1).unwrap();
    let before = playback;
    assert!(playback.history_before_constant(1).is_none());
    assert!(playback.shifted_normal_history(1.0).is_none());
    assert!(!playback.set_target(1.0));
    assert!(!playback.seek(0, ExplicitSeekMode::Normal));
    assert!(!playback.advance_compact(&geometry, 2));
    assert_complete_copy(&playback, &before);
    assert_eq!(playback.chunk(2).0, 1);
    assert!(playback.advance_compact(&geometry, 1));
    assert_eq!(playback.mapped_output_frame(), Some(u64::MAX));
    assert_position(playback.position(), position(17, 0.25));
    let before = playback;
    assert_eq!(playback.position_at_compact(&geometry, 1), None);
    assert!(!playback.advance_compact(&geometry, 1));
    assert!(playback.refresh_mapped_span_compact(&geometry).is_err());
    assert!(
        playback
            .project_mapped_compact_to(&geometry, u64::MAX)
            .is_err()
    );
    assert_complete_copy(&playback, &before);
    let sample = sample();
    let mut output = [vec![-91.0; 1], vec![-92.0; 1]];
    let unchanged = output.clone();
    assert!(
        plan(playback)
            .fill_fractional_buffers_checked_compact_with_readers(
                &sample,
                StemReadSets::default(),
                &playback,
                &geometry,
                &mut output,
                1,
            )
            .is_err()
    );
    assert_eq!(output, unchanged);
    assert_complete_copy(&playback, &before);
}

#[derive(Clone, Copy)]
enum Shape {
    NegativeZero,
    NearOne,
    LargeEpoch,
    KnotAligned,
    KnotStaggered,
    VirtualAligned,
    VirtualStaggered,
}

impl Shape {
    fn name(self) -> &'static str {
        match self {
            Self::NegativeZero => "negative_zero",
            Self::NearOne => "near_one",
            Self::LargeEpoch => "large_epoch",
            Self::KnotAligned => "knot_aligned",
            Self::KnotStaggered => "knot_staggered",
            Self::VirtualAligned => "extreme_virtual_aligned",
            Self::VirtualStaggered => "extreme_virtual_staggered",
        }
    }

    fn is_virtual(self) -> bool {
        matches!(self, Self::VirtualAligned | Self::VirtualStaggered)
    }
}

fn prepared_lanes(
    geometry: &MappedLoopGeometry,
    shape: Shape,
    active: usize,
) -> [SourcePlayback; MAX_VOICES] {
    let large = matches!(shape, Shape::LargeEpoch);
    let first = if large { (1_u64 << 53) + 7 } else { 17 };
    let timeline = timeline(large);
    let prepare = |lane| {
        let raw = match shape {
            Shape::NegativeZero => position(17 + lane * 7, -0.0),
            Shape::NearOne => position(17 + lane * 7, 1.0_f64.next_down()),
            Shape::LargeEpoch => position(17 + lane * 7, 0.25),
            Shape::KnotAligned => position(23_999, 1.0_f64.next_down()),
            Shape::KnotStaggered => position(23_999 - lane * 13, 1.0_f64.next_down()),
            Shape::VirtualAligned => position(2, 0.125),
            Shape::VirtualStaggered => {
                let phase = lane as f64 * 9.0 / 128.0;
                position(phase.floor() as usize, phase.fract())
            }
        };
        let epoch = epoch(geometry, timeline, raw, first);
        let playback = SourcePlayback::mapped_compact(geometry, epoch, first).unwrap();
        assert_position(playback.position(), raw);
        playback
    };
    assert!(active > 0 && active <= MAX_VOICES);
    let mut lanes = [prepare(0); MAX_VOICES];
    for (lane, playback) in lanes.iter_mut().enumerate().take(active).skip(1) {
        *playback = prepare(lane);
    }
    lanes
}

fn reference_lanes(
    geometry: &MappedLoopGeometry,
    compact: [SourcePlayback; MAX_VOICES],
    active: usize,
) -> [SourcePlayback; MAX_VOICES] {
    let mut reference = compact;
    for (actual, reference) in compact.iter().zip(&mut reference).take(active) {
        *reference = SourcePlayback::mapped(
            geometry,
            actual.mapped_epoch().unwrap(),
            actual.mapped_output_frame().unwrap(),
        )
        .unwrap();
        assert!(actual.matches_exact(reference));
        assert_position(actual.position(), reference.position());
    }
    reference
}

#[test]
fn compact_consuming_full_checked_and_reference_readers_keep_pcm_and_final_copies() {
    let ordinary = variable_geometry();
    let virtual_loop = virtual_geometry();
    let sample = sample();
    for shape in [
        Shape::NegativeZero,
        Shape::NearOne,
        Shape::LargeEpoch,
        Shape::KnotAligned,
        Shape::VirtualStaggered,
    ] {
        let geometry = if shape.is_virtual() {
            &virtual_loop
        } else {
            &ordinary
        };
        let original = prepared_lanes(geometry, shape, 2);
        let mut reference = reference_lanes(geometry, original, 2);
        let mut reference_scratch = buffers(2);
        let mut reference_output = buffers(2);
        let mut reference_work = [Work::default(); 2];
        render_reference(
            geometry,
            &sample,
            &mut reference[..2],
            &mut reference_scratch,
            &mut reference_output,
            &mut reference_work,
            Reader::Full,
        );
        for reader in [Reader::Full, Reader::Checked] {
            let mut compact = original;
            let mut scratch = buffers(2);
            let mut output = buffers(2);
            let mut work = [Work::default(); 2];
            let (_, observed) = observe_current_thread(|| {
                render_compact(
                    geometry,
                    &sample,
                    &mut compact[..2],
                    &mut scratch,
                    &mut output,
                    &mut work,
                    reader,
                );
            });
            assert_eq!(observed, ThreadAllocations::default());
            assert_pcm(&output, &reference_output);
            for lane in 0..2 {
                assert!(compact[lane].matches_exact(&reference[lane]));
                assert_position(compact[lane].position(), reference[lane].position());
                assert_eq!(work[lane].chunks, reference_work[lane].chunks);
                assert_eq!(work[lane].refreshes, reference_work[lane].refreshes);
                assert_eq!(
                    work[lane].endpoint_coordinates,
                    reference_work[lane].endpoint_coordinates
                );
                assert_eq!(work[lane].reader_coordinates, BLOCK * reader.passes());
                assert_first_pcm(&sample, original[lane], &output[lane]);
            }
        }
    }
}

#[test]
fn compact_consuming_workload_runs_on_explicit_two_mib_stack_without_rust_requests() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<MappedSourceSpan>();
    assert_copy::<SourcePlayback>();
    assert!(!std::mem::needs_drop::<MappedSourceSpan>());
    assert!(!std::mem::needs_drop::<SourcePlayback>());
    assert!(std::mem::size_of::<MappedSourceSpan>() <= 240);
    assert!(std::mem::size_of::<[SourcePlayback; MAX_VOICES]>() <= 16 * 1024);

    // All wide qualification/epoch setup, owners and heap buffers precede thread creation.
    let geometry = virtual_geometry();
    let sample = sample();
    let original = prepared_lanes(&geometry, Shape::VirtualStaggered, 6);
    let mut reference = reference_lanes(&geometry, original, 6);
    let mut reference_scratch = buffers(6);
    let mut reference_output = buffers(6);
    let mut reference_work = [Work::default(); 6];
    render_reference(
        &geometry,
        &sample,
        &mut reference[..6],
        &mut reference_scratch,
        &mut reference_output,
        &mut reference_work,
        Reader::Checked,
    );
    // Keep the original owners outside the child; no last owner is retired in the scope.
    let child_geometry = geometry.clone();
    let child_sample = sample.clone();
    let mut scratch = buffers(6);
    let mut output = buffers(6);
    let mut playback = original;
    let child = std::thread::Builder::new()
        .name("compact-two-mib-reader".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let mut work = [Work::default(); 6];
            let (_, observed) = observe_current_thread(|| {
                render_compact(
                    &child_geometry,
                    &child_sample,
                    &mut playback[..6],
                    &mut scratch,
                    &mut output,
                    &mut work,
                    Reader::Checked,
                );
            });
            (playback, output, work, observed)
        })
        .unwrap();
    let (playback, output, work, observed) = child.join().unwrap();
    assert_eq!(observed, ThreadAllocations::default());
    assert_pcm(&output, &reference_output);
    for lane in 0..6 {
        assert!(playback[lane].matches_exact(&reference[lane]));
        assert_position(playback[lane].position(), reference[lane].position());
        assert_eq!(work[lane], reference_work[lane]);
    }
}

#[test]
#[ignore = "descriptive consuming Release profile; run explicitly with --release --ignored --nocapture"]
fn descriptive_compact_consuming_cohorts_report_median_max_and_actual_work() {
    const REPEATS: usize = 3;
    let ordinary = variable_geometry();
    let virtual_loop = virtual_geometry();
    let sample = sample();
    let interval = BLOCK as f64 / f64::from(RATE);
    for shape in [
        Shape::NegativeZero,
        Shape::NearOne,
        Shape::LargeEpoch,
        Shape::KnotAligned,
        Shape::KnotStaggered,
        Shape::VirtualAligned,
        Shape::VirtualStaggered,
    ] {
        let geometry = if shape.is_virtual() {
            &virtual_loop
        } else {
            &ordinary
        };
        let original = prepared_lanes(geometry, shape, 20);
        for reader in [Reader::Full, Reader::Checked] {
            // One unchanged reference cohort is reused for each matching prefix, outside timing.
            let mut reference = reference_lanes(geometry, original, 20);
            let mut reference_scratch = buffers(20);
            let mut reference_output = buffers(20);
            let mut reference_work = [Work::default(); 20];
            render_reference(
                geometry,
                &sample,
                &mut reference[..20],
                &mut reference_scratch,
                &mut reference_output,
                &mut reference_work,
                reader,
            );
            // Ten is measured unconditionally to retain a performance fallback diagnostic.
            for active in [2, 4, 6, 10, 20] {
                let mut scratch = buffers(active);
                let mut output = buffers(active);
                let mut work = [Work::default(); MAX_VOICES];
                let mut times = [Duration::ZERO; REPEATS];
                let mut actual_work = Work::default();
                for repetition in 0..=REPEATS {
                    let mut playback = original;
                    let before = Instant::now();
                    let (_, observed) = observe_current_thread(|| {
                        render_compact(
                            geometry,
                            &sample,
                            &mut playback[..active],
                            &mut scratch,
                            &mut output,
                            &mut work[..active],
                            reader,
                        );
                    });
                    let elapsed = before.elapsed();
                    assert_eq!(observed, ThreadAllocations::default());
                    assert_pcm(&output, &reference_output[..active]);
                    actual_work = Work::default();
                    for lane in 0..active {
                        assert!(playback[lane].matches_exact(&reference[lane]));
                        assert_position(playback[lane].position(), reference[lane].position());
                        assert_eq!(work[lane], reference_work[lane]);
                        actual_work.add(work[lane]);
                        assert_first_pcm(&sample, original[lane], &output[lane]);
                    }
                    if repetition > 0 {
                        times[repetition - 1] = elapsed;
                    }
                }
                times.sort_unstable();
                let median = times[REPEATS / 2].as_secs_f64();
                let maximum = times[REPEATS - 1].as_secs_f64();
                let cohort = match active {
                    2 | 4 | 6 => "primary",
                    20 => "stress",
                    _ => "unconditional_performance_fallback_diagnostic",
                };
                assert_eq!(
                    actual_work.reader_coordinates,
                    active * BLOCK * reader.passes()
                );
                assert_eq!(actual_work.endpoint_coordinates, actual_work.chunks * 2);
                eprintln!(
                    "compact_consuming_profile: shape={} reader={} cohort={} active={} fixed_capacity={} frames_per_lane={} warmups=1 repeats={} median_ms={:.4} max_ms={:.4} nominal_512_at_48k_ms={:.7} median_interval_ratio={:.3} max_interval_ratio={:.3} chunks={} refreshes={} reader_coordinate_requests={} separate_endpoint_requests={}; includes refresh, copied advance, actual reader fill and output copy; refresh/view-internal queries are uncounted; observed Rust allocation/zeroed/reallocation requests=0 (no deallocation or Native observation); no callback/DSP/device/music/worstcase/peakstack/custody/RT permission",
                    shape.name(),
                    reader.name(),
                    cohort,
                    active,
                    MAX_VOICES,
                    BLOCK,
                    REPEATS,
                    median * 1000.0,
                    maximum * 1000.0,
                    interval * 1000.0,
                    median / interval,
                    maximum / interval,
                    actual_work.chunks,
                    actual_work.refreshes,
                    actual_work.reader_coordinates,
                    actual_work.endpoint_coordinates,
                );
            }
        }
    }
}

fn tagged_playback(
    geometry: &MappedLoopGeometry,
    actual: TransportTimeline,
    raw: FractionalSourcePosition,
    first: u64,
) -> SourcePlayback {
    SourcePlayback::mapped_anchored_compact_continuation(
        geometry,
        MARKER,
        actual,
        geometry.domain(),
        raw,
        first,
    )
    .unwrap()
}

#[test]
fn tagged_generic_full_and_checked_readers_prepare_once_and_never_enter_wide_queries() {
    use crate::audio_engine::mapped_source_clock::observe_compact_queries_for_test;
    let geometry = variable_geometry();
    let sample = sample();
    let actual = timeline(false);
    let raw = position(17, 0.25);
    let compact = tagged_playback(&geometry, actual, raw, 17);
    let wide_epoch = epoch(&geometry, actual, raw, 17);
    let wide = SourcePlayback::mapped(&geometry, wide_epoch, 17).unwrap();
    assert!(compact.is_compact_mapped());
    assert!(!wide.is_compact_mapped());
    assert!(!compact.matches_exact(&wide));
    for reader in [Reader::Full, Reader::Checked] {
        let mut expected = [vec![f32::NAN; BLOCK], vec![f32::NAN; BLOCK]];
        let mut observed = [vec![f32::NAN; BLOCK], vec![f32::NAN; BLOCK]];
        plan(wide)
            .fill_fractional_buffers_checked_with_readers(
                &sample,
                StemReadSets::default(),
                &wide,
                &mut expected,
                BLOCK,
            )
            .unwrap();
        let (result, counts) = observe_compact_queries_for_test(|| match reader {
            Reader::Full => plan(compact).fill_fractional_buffers_with_readers(
                &sample,
                StemReadSets::default(),
                &compact,
                &mut observed,
                BLOCK,
            ),
            Reader::Checked => plan(compact).fill_fractional_buffers_checked_with_readers(
                &sample,
                StemReadSets::default(),
                &compact,
                &mut observed,
                BLOCK,
            ),
        });
        result.unwrap();
        assert_eq!(counts, (1, 0));
        for (observed, expected) in observed.iter().flatten().zip(expected.iter().flatten()) {
            assert_eq!(observed.to_bits(), expected.to_bits());
        }
        assert_first_pcm(&sample, compact, &observed);
    }
}

#[test]
fn tagged_generic_partitioned_advance_refresh_resume_and_projection_preserve_original_trajectory() {
    use crate::audio_engine::mapped_source_clock::observe_compact_queries_for_test;
    let geometry = variable_geometry();
    let actual = timeline(true);
    let first = 1_u64 << 53;
    let raw = position(23_999, 1.0_f64.next_down());
    let initial = tagged_playback(&geometry, actual, raw, first);
    let wide_epoch = epoch(&geometry, actual, raw, first);
    let mut expected = SourcePlayback::mapped(&geometry, wide_epoch, first).unwrap();
    expected
        .project_mapped_to(&geometry, first + 32_000)
        .unwrap();
    for partition in [&[512][..], &[73, 511, 7, 127, 3][..]] {
        let (mut compact, (_, wide_entries)) = observe_compact_queries_for_test(|| {
            let mut compact = initial;
            let mut rendered = 0;
            let mut chunk = 0;
            while rendered < 32_000 {
                let remaining = partition[chunk % partition.len()].min(32_000 - rendered);
                let (frames, _) = compact.chunk(remaining);
                if frames == 0 {
                    compact.refresh_mapped_span(&geometry).unwrap();
                    continue;
                }
                let next = compact.position_at(frames).unwrap();
                assert!(compact.advance(frames));
                assert_position(compact.position(), next);
                rendered += frames;
                chunk += 1;
            }
            compact.refresh_mapped_span(&geometry).unwrap();
            compact
        });
        assert_eq!(wide_entries, 0);
        assert_position(compact.position(), expected.position());
        assert_eq!(
            compact.mapped_output_frame(),
            expected.mapped_output_frame()
        );
        let saved = compact;
        assert!(compact.replace_mapped_epoch(&geometry, wide_epoch).is_err());
        assert_complete_copy(&compact, &saved);
        let paused = compact.position();
        compact
            .resume_mapped_at(&geometry, &actual, first + 32_031)
            .unwrap();
        assert_position(compact.position(), paused);
        assert!(compact.is_compact_mapped());
        let mut resumed_wide = expected;
        resumed_wide
            .resume_mapped_at(&geometry, &actual, first + 32_031)
            .unwrap();
        compact
            .project_mapped_to(&geometry, first + 32_032)
            .unwrap();
        resumed_wide
            .project_mapped_to(&geometry, first + 32_032)
            .unwrap();
        assert_position(compact.position(), resumed_wide.position());
    }
    let frame = first + 32_000;
    let mut compact = initial;
    compact.project_mapped_to(&geometry, frame).unwrap();
    let mut changed = actual;
    assert!(changed.set_master_period_preserving_beat_position_at_frame(0.25, frame));
    let replacement = compact
        .mapped_epoch()
        .unwrap()
        .with_timeline_at(changed, frame)
        .unwrap();
    compact
        .replace_mapped_epoch(&geometry, replacement)
        .unwrap();
    let wide_replacement = wide_epoch.with_timeline_at(changed, frame).unwrap();
    expected
        .replace_mapped_epoch(&geometry, wide_replacement)
        .unwrap();
    assert_position(compact.position(), expected.position());
    assert_eq!(
        compact.tempo_ratio().to_bits(),
        expected.tempo_ratio().to_bits()
    );
    compact.project_mapped_to(&geometry, frame + 513).unwrap();
    expected.project_mapped_to(&geometry, frame + 513).unwrap();
    assert_position(compact.position(), expected.position());
}

#[test]
fn tagged_journal_slice_merge_and_trim_keep_exact_coordinates_without_wide_fallback() {
    use crate::audio_engine::mapped_source_clock::observe_compact_queries_for_test;
    use crate::audio_engine::productive_source_history::ProductiveSourceBinding;
    use crate::audio_engine::source_feed_history::{
        SOURCE_FEED_HISTORY_MAX_FRAMES, SourceFeedContext, SourceFeedHistory, SourceFeedSnapshot,
        SourceFeedSpan,
    };
    let geometry = variable_geometry();
    let sample = sample();
    let actual = timeline(false);
    let raw = position(17, 0.25);
    let mut compact = tagged_playback(&geometry, actual, raw, 17);
    let wide_epoch = epoch(&geometry, actual, raw, 17);
    let context = SourceFeedContext::new(
        ProductiveSourceBinding::new(&sample, RATE, None),
        StemReadSets::default(),
    );
    let first_span = SourceFeedSpan {
        active_start: 0,
        frames: 512,
        playback: compact,
        plan: plan(compact),
        context,
    };
    let suffix = first_span.slice(73, 439).unwrap();
    assert_position(suffix.playback.position(), compact.position_at(73).unwrap());
    let mut journal = SourceFeedHistory::new();
    let mut snapshot = SourceFeedSnapshot::new();
    let total = SOURCE_FEED_HISTORY_MAX_FRAMES + 777;
    // Journal bookkeeping is isolated generated-coordinate evidence. The separate
    // generic-reader test proves actual PCM; these entries do not grant source permits.
    let (_, (_, wide_entries)) = observe_compact_queries_for_test(|| {
        let mut rendered = 0;
        while rendered < total {
            let (frames, _) = compact.chunk((total - rendered).min(512));
            if frames == 0 {
                compact.refresh_mapped_span(&geometry).unwrap();
                continue;
            }
            journal.record_success(compact, plan(compact), context, frames);
            assert!(compact.advance(frames));
            rendered += frames;
        }
        assert!(journal.capture_into(&mut snapshot));
    });
    assert_eq!(wide_entries, 0);
    assert_eq!(snapshot.retained_range(), Some(777..total as u64));
    assert!(snapshot.spans().len() < total / 512);
    for span in snapshot.spans() {
        let expected = geometry
            .position_at(wide_epoch, 17 + span.active_start)
            .unwrap();
        assert_position(span.playback.position(), expected);
        let offset = span.frames / 2;
        let slice = span.slice(offset, span.frames - offset).unwrap();
        let expected = geometry
            .position_at(wide_epoch, 17 + span.active_start + offset as u64)
            .unwrap();
        assert_position(slice.playback.position(), expected);
    }
    let mut checkpoint = crate::audio_engine::prepared_native_history::SourceReadCheckpoint {
        playback: compact,
        plan: plan(compact),
    };
    let before = checkpoint;
    assert!(!checkpoint.project_output_frames(1));
    assert!(checkpoint.matches_exact(before));
}

#[test]
fn actual_e_constructor_and_generic_consuming_reader_fit_two_mib_without_rust_allocations() {
    use crate::audio_engine::mapped_source_clock::observe_compact_queries_for_test;
    fn assert_copy<T: Copy>() {}
    assert_copy::<MappedSourceSpan>();
    assert_copy::<SourcePlayback>();
    assert!(!std::mem::needs_drop::<MappedSourceSpan>());
    assert!(!std::mem::needs_drop::<SourcePlayback>());
    assert!(std::mem::size_of::<MappedSourceSpan>() <= 240);
    assert!(std::mem::size_of::<[SourcePlayback; MAX_VOICES]>() <= 16 * 1024);
    let geometry = variable_geometry();
    let sample = sample();
    let actual = timeline(false);
    let initial = prepared_lanes(&geometry, Shape::NegativeZero, 6);
    let mut reference = reference_lanes(&geometry, initial, 6);
    let mut reference_scratch = buffers(6);
    let mut reference_output = buffers(6);
    let mut reference_work = [Work::default(); 6];
    render_reference(
        &geometry,
        &sample,
        &mut reference[..6],
        &mut reference_scratch,
        &mut reference_output,
        &mut reference_work,
        Reader::Checked,
    );
    let child_geometry = geometry.clone();
    let child_sample = sample.clone();
    let mut scratch = buffers(6);
    let mut output = buffers(6);
    let child = std::thread::Builder::new()
        .name("actual-e-compact-two-mib".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let mut work = [Work::default(); 6];
            let ((playback, counts), allocations) = observe_current_thread(|| {
                observe_compact_queries_for_test(|| {
                    // The bounded actual-E constructor is INSIDE the observed child.
                    let first = tagged_playback(&child_geometry, actual, position(17, -0.0), 17);
                    let mut playback = [first; MAX_VOICES];
                    for (lane, slot) in playback.iter_mut().enumerate().take(6).skip(1) {
                        *slot = tagged_playback(
                            &child_geometry,
                            actual,
                            position(17 + lane * 7, -0.0),
                            17,
                        );
                    }
                    render_reference(
                        &child_geometry,
                        &child_sample,
                        &mut playback[..6],
                        &mut scratch,
                        &mut output,
                        &mut work,
                        Reader::Checked,
                    );
                    playback
                })
            });
            (playback, output, work, allocations, counts)
        })
        .unwrap();
    let (playback, output, work, allocations, counts) = child.join().unwrap();
    assert_eq!(allocations, ThreadAllocations::default());
    assert_eq!(counts.1, 0);
    assert!(counts.0 >= 6);
    assert_pcm(&output, &reference_output);
    for lane in 0..6 {
        assert!(playback[lane].is_compact_mapped());
        assert!(!playback[lane].matches_exact(&reference[lane]));
        assert_position(playback[lane].position(), reference[lane].position());
        assert_eq!(
            playback[lane].mapped_output_frame(),
            reference[lane].mapped_output_frame()
        );
        assert_eq!(work[lane], reference_work[lane]);
    }
}

#[test]
fn tagged_terminal_queries_advance_and_checked_output_keep_atomic_refusals() {
    use crate::audio_engine::mapped_source_clock::observe_compact_queries_for_test;
    let geometry = variable_geometry();
    let sample = sample();
    let first = u64::MAX - 1;
    let mut compact = tagged_playback(&geometry, timeline(false), position(17, 0.25), first);
    let before = compact;
    let mut output = [vec![-91.0; 2], vec![-92.0; 2]];
    let unchanged = output.clone();
    let (_, (_, wide_entries)) = observe_compact_queries_for_test(|| {
        assert!(!compact.advance(2));
        assert_complete_copy(&compact, &before);
        assert!(
            plan(compact)
                .fill_fractional_buffers_checked_with_readers(
                    &sample,
                    StemReadSets::default(),
                    &compact,
                    &mut output,
                    2,
                )
                .is_err()
        );
        assert_eq!(output, unchanged);
        assert!(compact.advance(1));
        assert_eq!(compact.mapped_output_frame(), Some(u64::MAX));
        assert_position(compact.position(), before.position());
        let terminal = compact;
        assert!(compact.position_at(1).is_none());
        assert!(!compact.advance(1));
        assert!(compact.refresh_mapped_span(&geometry).is_err());
        assert_complete_copy(&compact, &terminal);
    });
    assert_eq!(wide_entries, 0);
}
