use super::*;
use crate::audio_engine::mapped_source_clock::tests::map_with_source;
use crate::audio_engine::source_reader::FrameRange;
use crate::test_memory::{ThreadAllocations, observe_current_thread};

const VARIABLE: &[(f64, f64)] = &[(0.0, 0.0), (0.5, 1.0), (1.0, 4.0)];

fn qualified_geometry(
    anchors: &[(f64, f64)],
    rate: u32,
    frames: usize,
    region: FrameRange,
    quarters: f64,
) -> MappedLoopGeometry {
    MappedLoopGeometry::new(
        map_with_source(
            anchors,
            &vec!["trusted"; anchors.len() - 1],
            false,
            rate,
            frames,
        ),
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

fn variable() -> MappedLoopGeometry {
    qualified_geometry(
        VARIABLE,
        48_000,
        96_000,
        FrameRange {
            start: 0,
            end: 48_000,
        },
        4.0,
    )
}

fn timeline(fpb: f64, frame: u64, beat: f64) -> TransportTimeline {
    let mut timeline = TransportTimeline::new(1);
    assert!(timeline.set_master_period_and_anchor_beat_position_at_frame(fpb, beat, frame));
    timeline
}

fn pair(frame: usize, fraction_bits: u64) -> FractionalSourcePosition {
    FractionalSourcePosition {
        frame,
        fraction: f64::from_bits(fraction_bits),
        seek_mode: ExplicitSeekMode::Normal,
    }
}

fn new_epoch(
    geometry: &MappedLoopGeometry,
    timeline: TransportTimeline,
    position: FractionalSourcePosition,
    frame: u64,
) -> MappedClockEpoch {
    MappedClockEpoch::anchored(7, timeline, geometry, geometry.domain(), position, frame).unwrap()
}

fn assert_pair(actual: FractionalSourcePosition, whole: usize, fraction_bits: u64) {
    assert_eq!(actual.frame, whole);
    assert_eq!(actual.fraction.to_bits(), fraction_bits);
    assert_eq!(actual.seek_mode, ExplicitSeekMode::Normal);
}

#[test]
fn independent_variable_oracle_retains_actual_large_frame_plateaus_and_fraction_carries() {
    let geometry = variable();
    let e = 1_u64 << 53;
    let epoch = new_epoch(
        &geometry,
        timeline(16_000.0, e + 1, 3.125),
        pair(23_999, 0x3fef_ffff_ffff_ffff),
        e,
    );
    for (offset, master_bits, whole, fraction_bits) in [
        (0, 0x4009_0000_0000_0000, 23_999, 0x3fef_ffff_ffff_ffff),
        (1, 0x4009_0000_0000_0000, 23_999, 0x3fef_ffff_ffff_ffff),
        (2, 0x4009_0041_8937_4bc7, 24_001, 0x3d75_7fd5_5555_5555),
        (3, 0x4009_0083_126e_978d, 24_001, 0x3fef_ffff_ffff_d900),
        (31_999, 0x4014_8000_0000_0000, 40_000, 0),
        (32_000, 0x4014_8000_0000_0000, 40_000, 0),
        (64_000, 0x401c_8000_0000_0000, 23_999, 0x3fef_ffff_ffff_ffff),
        (u64::MAX - e, 0x4310_6041_8937_4bd4, 39_000, 0),
    ] {
        let frame = e + offset;
        assert_eq!(
            epoch.addressed_master_at(frame).unwrap().to_bits(),
            master_bits
        );
        assert_pair(
            geometry.position_at(epoch, frame).unwrap(),
            whole,
            fraction_bits,
        );
        if frame != u64::MAX {
            let span = geometry.refresh_span(epoch, frame).unwrap();
            assert_pair(span.position_at(frame).unwrap(), whole, fraction_bits);
            assert_eq!(
                span.position_at(span.end_frame()),
                Some(geometry.position_at(epoch, span.end_frame()).unwrap())
            );
        }
    }
}

#[test]
fn independent_ordinary_master_differences_are_not_an_elapsed_clock() {
    let geometry = variable();
    for (e, raw, rows) in [
        (
            17,
            pair(0, 1_u64 << 63),
            vec![
                (17, 0, 1_u64 << 63),
                (8017, 11_999, 0x3fef_ffff_ffff_e370),
                (64_017, 47_999, 0x3fef_ffff_ffff_b7fa),
            ],
        ),
        (
            96_007,
            pair(12_000, 1),
            vec![
                (96_007, 12_000, 1),
                (96_008, 12_001, 0x3fdf_ffff_fffc_1c00),
                (160_007, 12_000, 1),
            ],
        ),
    ] {
        let epoch = new_epoch(&geometry, timeline(16_000.0, 0, 0.0), raw, e);
        for (frame, whole, fraction) in rows {
            assert_pair(geometry.position_at(epoch, frame).unwrap(), whole, fraction);
        }
        assert!(geometry.position_at(epoch, e - 1).is_err());
    }
}

#[test]
fn generic_fraction_identity_covers_integer_displacements_but_not_subnormal_knot_scaling() {
    let geometry = variable();
    for raw_bits in [1_u64 << 63, 1] {
        let epoch = new_epoch(&geometry, timeline(16_000.0, 0, 0.0), pair(0, raw_bits), 0);
        for (frame, whole, fraction) in [
            (4000, 6000, raw_bits),
            (16_000, 24_000, if raw_bits == 1 { 0 } else { raw_bits }),
            (64_000, 0, raw_bits),
        ] {
            assert_pair(geometry.position_at(epoch, frame).unwrap(), whole, fraction);
        }
        let first = geometry.refresh_span(epoch, 0).unwrap();
        assert_eq!(first.end_frame(), 16_000);
        let second = geometry.refresh_span(epoch, 16_000).unwrap();
        assert_eq!(second.segment.beat_start, 1.0);
        assert_eq!(second.start_frame(), 16_000);
    }
}

#[test]
fn virtual_domain_wraps_original_period_and_exact_rounded_endpoint_only() {
    let geometry = qualified_geometry(
        &[(0.0, 0.0), (9.0 / 32768.0, 2.0_f64.powi(-13))],
        8192,
        3,
        FrameRange { start: 0, end: 2 },
        2.0_f64.powi(-13),
    );
    assert_eq!(geometry.domain().musical_period, Some(2.25));
    let epoch = new_epoch(
        &geometry,
        timeline(16_000.0, 0, 0.0),
        pair(2, 0x3fc0_0000_0000_0000),
        0,
    );
    for (frame, whole, fraction) in [
        (0, 2, 0x3fc0_0000_0000_0000),
        (1, 1, 0x3f9b_a5e3_53f7_cee0),
        (2, 2, 0x3fc6_e978_d4fd_f3b8),
        (125, 2, 0x3fc0_0000_0000_0000),
    ] {
        assert_pair(geometry.position_at(epoch, frame).unwrap(), whole, fraction);
    }
    let identity = qualified_geometry(
        &[(0.0, 0.0), (1.0, 1.0)],
        1,
        2,
        FrameRange { start: 0, end: 1 },
        1.0,
    );
    let epoch = new_epoch(
        &identity,
        timeline(2.0, 0, 0.0),
        pair(0, 0x3fdf_ffff_ffff_ffff),
        0,
    );
    assert_pair(identity.position_at(epoch, 1).unwrap(), 0, 0);
    assert!(
        normalize(
            SourceCoordinate {
                whole: 1,
                fraction: f64::from_bits(1)
            },
            0,
            1.0
        )
        .is_none()
    );
    assert_pair(
        normalize(
            SourceCoordinate {
                whole: 1_u128 << 64,
                fraction: 0.0,
            },
            0,
            2.0_f64.powi(64),
        )
        .unwrap(),
        0,
        0,
    );
}

#[test]
fn separate_exact_endpoint_normalization_precedes_usize_cast_and_never_clamps() {
    let period = 2.0_f64.powi(64);
    assert_pair(
        normalize(
            SourceCoordinate {
                whole: 1_u128 << 64,
                fraction: 0.0,
            },
            0,
            period,
        )
        .unwrap(),
        0,
        0,
    );
    assert_pair(
        normalize(
            SourceCoordinate {
                whole: u128::from(u64::MAX),
                fraction: 0.5,
            },
            0,
            period,
        )
        .unwrap(),
        usize::MAX,
        0.5_f64.to_bits(),
    );
    assert!(
        normalize(
            SourceCoordinate {
                whole: 1_u128 << 64,
                fraction: f64::from_bits(1)
            },
            0,
            period
        )
        .is_none()
    );
    let start = (1_u64 << 53) + 7;
    assert_pair(
        normalize(
            SourceCoordinate {
                whole: u128::from(start),
                fraction: 0.5,
            },
            start,
            0.75,
        )
        .unwrap(),
        start as usize,
        0.5_f64.to_bits(),
    );
    assert_pair(
        normalize(
            SourceCoordinate {
                whole: u128::from(start),
                fraction: 0.75,
            },
            start,
            0.75,
        )
        .unwrap(),
        start as usize,
        0,
    );
    assert!(
        normalize(
            SourceCoordinate {
                whole: u128::from(start),
                fraction: f64::from_bits(0.75_f64.to_bits() + 1)
            },
            start,
            0.75
        )
        .is_none()
    );
}

#[test]
fn valid_master_change_retains_original_m0_and_refuses_out_of_rate_projection() {
    let geometry = variable();
    let e = 1_u64 << 53;
    let original = new_epoch(
        &geometry,
        timeline(16_000.0, e + 1, 3.125),
        pair(23_999, 0x3fef_ffff_ffff_ffff),
        e,
    );
    let current = e + 32_000;
    let current_master = original.addressed_master_at(current).unwrap();
    let changed = timeline(12_000.0, current, current_master);
    let replacement = original.with_timeline_at(changed, current).unwrap();
    assert_eq!(
        anchor(replacement).unwrap().original_master.to_bits(),
        0x4009_0000_0000_0000
    );
    assert_eq!(
        replacement.projection.beat_position_at_frame(e).to_bits(),
        0x4003_aaaa_aaaa_aaab
    );
    assert_pair(
        geometry.position_at(replacement, current).unwrap(),
        40_000,
        0,
    );
    assert!(geometry.position_at(replacement, current - 1).is_err());
    assert!(original.replacement_anchor_matches(replacement, current));
    assert!(original.with_timeline(changed).is_none());
    assert!(
        replacement
            .with_timeline_at(changed, current)
            .unwrap()
            .matches_exact(replacement)
    );
    // Intentionally corrupt only the private cached-M0 field: independent oracle's
    // wrong-recomputation counterexample, not an admitted constructor/transition.
    let mut wrong = replacement;
    let mut raw = anchor(wrong).unwrap();
    raw.original_master = replacement.projection.beat_position_at_frame(e);
    wrong.mode = MappedClockMode::Anchored(raw);
    assert_pair(
        geometry.position_at(wrong, current).unwrap(),
        45_333,
        0x3fd5_5555_5555_01ff,
    );
    assert!(!replacement.replacement_anchor_matches(wrong, current));
    let too_slow = original
        .with_timeline_at(timeline(32_000.0, current, current_master), current)
        .unwrap();
    assert_eq!(
        geometry.position_at(too_slow, current),
        Err(MappedClockError::UnsupportedRate)
    );
}

#[test]
fn changed_projection_replacement_cannot_reopen_unobserved_preclaim_history() {
    let geometry = variable();
    let e = 1_u64 << 53;
    let original = new_epoch(
        &geometry,
        timeline(16_000.0, e + 1, 3.125),
        pair(23_999, 0x3fef_ffff_ffff_ffff),
        e,
    );
    let current = e + 32_000;
    let changed = timeline(
        12_000.0,
        current,
        original.addressed_master_at(current).unwrap(),
    );
    let replacement = original.with_timeline_at(changed, current).unwrap();
    let mut playback = SourcePlayback::mapped(&geometry, original, current).unwrap();
    let before = playback;

    // The API validates this restriction itself, without trusting that a
    // caller retained the history floor returned by with_timeline_at.
    let mut reopened = replacement;
    reopened.first_output_frame = original.first_output_frame;
    assert_pair(geometry.position_at(reopened, current).unwrap(), 40_000, 0);
    assert!(!original.replacement_anchor_matches(reopened, current));
    assert!(playback.replace_mapped_epoch(&geometry, reopened).is_err());
    assert!(playback.matches_exact(&before));

    playback.replace_mapped_epoch(&geometry, original).unwrap();
    assert!(playback.matches_exact(&before));
    playback
        .replace_mapped_epoch(&geometry, replacement)
        .unwrap();
    assert_pair(playback.position(), 40_000, 0);
    let after = playback;
    assert!(playback.project_mapped_to(&geometry, current - 1).is_err());
    assert!(playback.matches_exact(&after));
}

#[test]
fn wide_and_tiny_maps_keep_subnormal_anchor_and_cycles_wider_than_u64() {
    for (beta, quarters, fpb, master_at_one, master_at_max) in [
        (
            0x7e70_0000_0000_0000,
            0x7b70_0000_0000_0000,
            0x0470_0000_0000_0000,
            0x7e70_0000_0000_0010,
            0x7f70_0010_0000_0000,
        ),
        (
            0,
            0x0030_0000_0000_0000,
            0x7fb0_0000_0000_0000,
            0x0030_0000_0000_0000,
            0x0430_0000_0000_0000,
        ),
    ] {
        let beta = f64::from_bits(beta);
        let quarters = f64::from_bits(quarters);
        let geometry = qualified_geometry(
            &[(0.0, beta), (1.0, beta + quarters)],
            1,
            2,
            FrameRange { start: 0, end: 1 },
            quarters,
        );
        let epoch = new_epoch(
            &geometry,
            timeline(f64::from_bits(fpb), 0, beta),
            pair(0, 1),
            0,
        );
        for (frame, master) in [
            (0, beta.to_bits()),
            (1, master_at_one),
            (u64::MAX, master_at_max),
        ] {
            assert_eq!(epoch.addressed_master_at(frame).unwrap().to_bits(), master);
            assert_pair(geometry.position_at(epoch, frame).unwrap(), 0, 1);
        }
        let phase = phase_at(epoch, geometry.beat_start, quarters, 1, u64::MAX).unwrap();
        let (cycles, _) = phase.unfolded.div_rem_euclid(phase.terms.period).unwrap();
        assert_eq!(cycles, SourceInteger::from_integer(1_u128 << 64).unwrap());
    }
}

#[test]
fn partial_offset_loop_uses_original_negative_boundary_offsets_and_unused_knots() {
    let geometry = qualified_geometry(
        VARIABLE,
        48_000,
        96_000,
        FrameRange {
            start: 12_000,
            end: 40_000,
        },
        2.5,
    );
    assert_eq!(geometry.start_beat(), 0.5);
    let epoch = new_epoch(&geometry, timeline(16_000.0, 0, 0.0), pair(12_000, 0), 0);
    let span = geometry.refresh_span(epoch, 0).unwrap();
    assert_eq!(span.segment.beat_start, 0.0);
    assert_eq!(span.end_frame(), 8000);
    assert_pair(geometry.position_at(epoch, 4000).unwrap(), 18_000, 0);
    assert_pair(geometry.position_at(epoch, 8000).unwrap(), 24_000, 0);
    assert_pair(geometry.position_at(epoch, 40_000).unwrap(), 12_000, 0);
    let map = map_with_source(
        &[(0.1, 0.0), (0.5, 1.0), (1.0, 4.0), (1.1, 5.0)],
        &["uncertain", "trusted", "uncertain"],
        false,
        48_000,
        96_000,
    );
    let geometry = MappedLoopGeometry::new(
        map,
        96_000,
        48_000,
        FrameRange {
            start: 24_000,
            end: 48_000,
        },
        3.0,
        false,
    )
    .unwrap()
    .qualify_exact_continuation()
    .unwrap();
    let epoch = new_epoch(&geometry, timeline(16_000.0, 0, 0.0), pair(24_000, 1), 0);
    assert_pair(geometry.position_at(epoch, 0).unwrap(), 24_000, 1);
}

#[test]
fn fallible_original_axis_search_preserves_boundaries_and_propagates_failure() {
    let map = map_with_source(VARIABLE, &["trusted", "trusted"], false, 48_000, 96_000);
    for (target, expected_start) in [(0.0, 0.0), (0.5, 0.5), (1.0, 0.5)] {
        let mut comparisons = 0;
        let segment = map
            .canonical_segment_by_source_ordering(|seconds| {
                comparisons += 1;
                seconds.partial_cmp(&target)
            })
            .unwrap();
        assert_eq!(segment.source_start, expected_start);
        assert!(comparisons <= 18);
    }
    assert!(
        map.canonical_segment_by_source_ordering(|seconds| seconds.partial_cmp(&-1.0))
            .is_none()
    );
    assert!(
        map.canonical_segment_by_beat_ordering(|beat| beat.partial_cmp(&5.0))
            .is_none()
    );
    let mut calls = 0;
    assert!(
        map.canonical_segment_by_beat_ordering(|_| {
            calls += 1;
            None
        })
        .is_none()
    );
    assert_eq!(calls, 1);
    assert!(first_true(0, u64::MAX, |_| None).is_none());
    let mut calls = 0;
    assert_eq!(
        first_true(0, u64::MAX, |frame| {
            calls += 1;
            Some(frame >= 123)
        }),
        Some(123)
    );
    assert!(calls <= 64);
}

#[test]
fn constructor_refuses_uncertified_domains_raw_pairs_and_nonfinite_future_master() {
    let geometry = variable();
    let actual = timeline(16_000.0, 0, 0.0);
    let raw = pair(0, 0);
    for domain in [
        SourceLoopDomain::physical(96_000, geometry.domain().region),
        SourceLoopDomain::musical(96_000, geometry.domain().region, 48_000.25).unwrap(),
    ] {
        assert!(MappedClockEpoch::anchored(7, actual, &geometry, domain, raw, 0).is_err());
    }
    for invalid in [
        pair(48_000, 0),
        pair(0, 1.0_f64.to_bits()),
        pair(0, f64::NAN.to_bits()),
        FractionalSourcePosition {
            seek_mode: ExplicitSeekMode::BeforeLoop,
            ..raw
        },
    ] {
        assert!(
            MappedClockEpoch::anchored(7, actual, &geometry, geometry.domain(), invalid, 0)
                .is_err()
        );
    }
    assert!(MappedClockEpoch::anchored(0, actual, &geometry, geometry.domain(), raw, 0).is_err());
    assert!(
        MappedClockEpoch::anchored(7, actual, &geometry, geometry.domain(), raw, u64::MAX).is_err()
    );
    let unqualified = MappedLoopGeometry::new(
        map_with_source(VARIABLE, &["trusted", "trusted"], false, 48_000, 96_000),
        96_000,
        48_000,
        geometry.domain().region,
        4.0,
        false,
    )
    .unwrap();
    assert!(
        MappedClockEpoch::anchored(7, actual, &unqualified, unqualified.domain(), raw, 0).is_err()
    );
    let overflow = qualified_geometry(
        &[(0.0, 0.0), (2.0_f64.powi(-13), 2.0_f64.powi(1000))],
        8192,
        2,
        FrameRange { start: 0, end: 1 },
        2.0_f64.powi(1000),
    );
    assert!(
        MappedClockEpoch::anchored(
            7,
            timeline(2.0_f64.powi(-1000), 0, 0.0),
            &overflow,
            overflow.domain(),
            raw,
            0
        )
        .is_err()
    );
}

#[test]
fn terminal_span_observes_max_without_inventing_a_future_boundary() {
    let geometry = qualified_geometry(
        &[(0.0, 0.0), (1.0, 1.0)],
        1,
        2,
        FrameRange { start: 0, end: 1 },
        1.0,
    );
    let e = u64::MAX - 10;
    // An ordinary valid transport anchor keeps the existing downbeat guard.
    // Near MAX the unchanged u64 cast makes every remaining M(n) equal.
    let epoch = new_epoch(&geometry, timeline(1.0, 0, 0.0), pair(0, 1), e);
    let span = geometry.refresh_span(epoch, e).unwrap();
    assert_eq!(span.end_frame(), u64::MAX);
    assert_pair(span.position_at(u64::MAX).unwrap(), 0, 1);
    assert!(geometry.refresh_span(epoch, u64::MAX).is_err());
}

#[test]
fn descriptive_headless_query_and_refresh_profile_observes_only_thread_requests() {
    use std::hint::black_box;
    use std::time::Instant;
    let geometry = variable();
    let epoch = new_epoch(&geometry, timeline(16_000.0, 0, 0.0), pair(0, 1), 0);
    let span = geometry.refresh_span(epoch, 0).unwrap();
    let before = Instant::now();
    let (query_ok, allocations) = observe_current_thread(|| {
        (0..128).all(|frame| black_box(span.position_at(black_box(frame))).is_some())
    });
    let query_elapsed = before.elapsed();
    assert!(query_ok);
    assert_eq!(allocations, ThreadAllocations::default());
    let before = Instant::now();
    for frame in 0..16 {
        black_box(
            geometry
                .refresh_span(epoch, black_box(frame * 1000))
                .unwrap(),
        );
    }
    let refresh_elapsed = before.elapsed();
    eprintln!(
        "anchored_reference_profile: 128_span_queries={query_elapsed:?} 16_refreshes={refresh_elapsed:?}; descriptive only, stack/callback throughput OPEN; observer counts thread Rust requests, not frees/Native/global allocation"
    );
}
