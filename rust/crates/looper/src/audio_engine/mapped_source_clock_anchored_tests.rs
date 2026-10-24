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
    assert_eq!(geometry.domain().musical_period.scalar_period(), Some(2.25));
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

#[test]
fn descriptive_extended_profiles_keep_edge_shapes_separate() {
    use std::hint::black_box;
    use std::time::Instant;

    let virtual_geometry = qualified_geometry(
        &[(0.0, 0.0), (9.0 / 32768.0, 2.0_f64.powi(-13))],
        8192,
        3,
        FrameRange { start: 0, end: 2 },
        2.0_f64.powi(-13),
    );
    for (name, geometry, position, first) in [
        ("near_one", variable(), pair(0, 0x3fef_ffff_ffff_ffff), 0),
        (
            "negative_zero",
            variable(),
            pair(0, (-0.0_f64).to_bits()),
            0,
        ),
        (
            "virtual",
            virtual_geometry,
            pair(2, 0x3fc0_0000_0000_0000),
            0,
        ),
        ("large_epoch", variable(), pair(17, 1), (1_u64 << 53) + 7),
    ] {
        let epoch = new_epoch(&geometry, timeline(16_000.0, first, 0.0), position, first);
        let span = geometry.refresh_span(epoch, first).unwrap();
        let width = span.end_frame().checked_sub(first).unwrap();
        assert!(width > 0);
        let before = Instant::now();
        let (ok, allocations) = observe_current_thread(|| {
            (0..128).all(|offset| {
                black_box(span.position_at(black_box(first + offset % width))).is_some()
            })
        });
        let queries = before.elapsed();
        assert!(ok);
        assert_eq!(allocations, ThreadAllocations::default());
        let before = Instant::now();
        for offset in 0..16 {
            black_box(
                geometry
                    .refresh_span(epoch, black_box(first + offset * 1000))
                    .unwrap(),
            );
        }
        let refreshes = before.elapsed();
        eprintln!(
            "anchored_extended_profile: {name} 128_queries={queries:?} 16_refreshes={refreshes:?}; queries observed only, descriptive shapes, no worstcase/stack/callback permission"
        );
    }
    let one = SourceInteger::from_integer(1).unwrap();
    for (name, left, right) in [
        (
            "sparse_general",
            one.checked_shl(3100)
                .unwrap()
                .checked_add(SourceInteger::from_integer(3).unwrap())
                .unwrap(),
            one.checked_shl(3200)
                .unwrap()
                .checked_add(SourceInteger::from_integer(7).unwrap())
                .unwrap(),
        ),
        (
            "dense_general",
            one.checked_shl(3199).unwrap().checked_sub(one).unwrap(),
            one.checked_shl(3199).unwrap().checked_sub(one).unwrap(),
        ),
    ] {
        let before = Instant::now();
        let (ok, allocations) = observe_current_thread(|| {
            (0..128).all(|_| black_box(black_box(left).checked_mul(black_box(right))).is_some())
        });
        let elapsed = before.elapsed();
        assert!(ok);
        assert_eq!(allocations, ThreadAllocations::default());
        eprintln!(
            "anchored_extended_profile: {name} 128_products={elapsed:?}; observed Rust thread requests only; no worstcase/RT permission"
        );
    }
}

fn work_cases() -> [(&'static str, MappedLoopGeometry, MappedClockEpoch, u64); 5] {
    let virtual_geometry = qualified_geometry(
        &[(0.0, 0.0), (9.0 / 32768.0, 2.0_f64.powi(-13))],
        8192,
        3,
        FrameRange { start: 0, end: 2 },
        2.0_f64.powi(-13),
    );
    [
        ("minsub", variable(), pair(0, 1), 0),
        ("near_one", variable(), pair(0, 0x3fef_ffff_ffff_ffff), 0),
        (
            "negative_zero",
            variable(),
            pair(0, (-0.0_f64).to_bits()),
            0,
        ),
        (
            "virtual",
            virtual_geometry,
            pair(2, 0x3fc0_0000_0000_0000),
            0,
        ),
        ("large_epoch", variable(), pair(17, 1), (1_u64 << 53) + 7),
    ]
    .map(|(name, geometry, position, first)| {
        let epoch = new_epoch(&geometry, timeline(16_000.0, first, 0.0), position, first);
        (name, geometry, epoch, first)
    })
}

struct WorkQuery {
    frame: u64,
    span: MappedSourceSpan,
    phase: Phase,
}

fn work_walk(
    geometry: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    first: u64,
) -> (Vec<WorkQuery>, Vec<u64>, u64) {
    let mut span = geometry.refresh_span(epoch, first).unwrap();
    let initial_width = span.end_frame().checked_sub(first).unwrap();
    assert!(initial_width > 0);
    let mut refresh_requests = vec![first];
    let mut queries = Vec::with_capacity(512);
    for frame in first..first.checked_add(512).unwrap() {
        if frame == span.end_frame() {
            span = geometry.refresh_span(epoch, frame).unwrap();
            refresh_requests.push(frame);
        }
        assert!(frame >= span.start_frame() && frame < span.end_frame());
        let phase = phase_at(
            epoch,
            span.legacy_program().unwrap().beat_start,
            span.legacy_program().unwrap().quarters,
            span.loaded_rate_hz,
            frame,
        )
        .unwrap();
        queries.push(WorkQuery { frame, span, phase });
    }
    assert_eq!(queries.len(), 512);
    (queries, refresh_requests, initial_width)
}

#[test]
fn descriptive_work_components_use_the_same_consuming_span_queries() {
    use std::hint::black_box;
    use std::time::Instant;

    for (name, geometry, epoch, first) in work_cases() {
        // Prepared wide phases and their storage belong only to this test, outside timing.
        let (queries, refresh_requests, initial_width) = work_walk(&geometry, epoch, first);
        for query in &queries {
            let terms = anchor_terms(
                epoch,
                query.span.legacy_program().unwrap().beat_start,
                query.span.legacy_program().unwrap().quarters,
                query.span.loaded_rate_hz,
            )
            .unwrap();
            assert_eq!(terms.source_delta, query.phase.terms.source_delta);
            assert_eq!(terms.initial, query.phase.terms.initial);
            assert_eq!(terms.period, query.phase.terms.period);
            assert_eq!(
                unfolded_at(epoch, &query.phase.terms, query.frame),
                Some(query.phase.unfolded)
            );
            assert_eq!(
                query.phase.unfolded.rem_euclid(query.phase.terms.period),
                Some(query.phase.remainder)
            );
            let actual = query.span.position_at(query.frame).unwrap();
            let composed = position_on_segment(
                epoch,
                &query.phase,
                query.span.segment,
                query.span.legacy_program().unwrap().beat_start,
                query.span.loaded_rate_hz,
                query.span.source_start,
                query.span.legacy_program().unwrap().source_period,
            )
            .unwrap();
            assert_pair(composed, actual.frame, actual.fraction.to_bits());
            let direct = geometry.position_at(epoch, query.frame).unwrap();
            assert_pair(actual, direct.frame, direct.fraction.to_bits());
        }
        let original = anchor(epoch).unwrap();
        assert_pair(
            queries[0].span.position_at(first).unwrap(),
            usize::try_from(original.frame).unwrap(),
            original.fraction.to_bits(),
        );
        for component in [
            "anchor_terms",
            "unfolded_at",
            "phase_at",
            "position_on_segment",
            "full_span_query",
        ] {
            let before = Instant::now();
            let (ok, allocations) = observe_current_thread(|| {
                queries.iter().all(|query| match component {
                    "anchor_terms" => black_box(anchor_terms(
                        black_box(epoch),
                        black_box(query.span.legacy_program().unwrap().beat_start),
                        black_box(query.span.legacy_program().unwrap().quarters),
                        black_box(query.span.loaded_rate_hz),
                    ))
                    .is_some(),
                    "unfolded_at" => black_box(unfolded_at(
                        black_box(epoch),
                        black_box(&query.phase.terms),
                        black_box(query.frame),
                    ))
                    .is_some(),
                    "phase_at" => black_box(phase_at(
                        black_box(epoch),
                        black_box(query.span.legacy_program().unwrap().beat_start),
                        black_box(query.span.legacy_program().unwrap().quarters),
                        black_box(query.span.loaded_rate_hz),
                        black_box(query.frame),
                    ))
                    .is_some(),
                    "position_on_segment" => black_box(position_on_segment(
                        black_box(epoch),
                        black_box(&query.phase),
                        black_box(query.span.segment),
                        black_box(query.span.legacy_program().unwrap().beat_start),
                        black_box(query.span.loaded_rate_hz),
                        black_box(query.span.source_start),
                        black_box(query.span.legacy_program().unwrap().source_period),
                    ))
                    .is_some(),
                    "full_span_query" => {
                        black_box(black_box(query.span).position_at(black_box(query.frame)))
                            .is_some()
                    }
                    _ => unreachable!("fixed component list"),
                })
            });
            let elapsed = before.elapsed();
            assert!(ok);
            assert_eq!(allocations, ThreadAllocations::default());
            eprintln!(
                "anchored_work_component: shape={name} component={component} queries={} first={first} last={} initial_span_width={initial_width} refresh_requests_including_initial={} elapsed_ms={:.4}; actual consuming512-frame walk, no modulo sampling; prepared storage and comparisons outside clock, no timed output capture; zero observed thread Rust requests only; component times include call/copy/cache effects, not additive or exclusive; no reader/mixer/DSP/device/peakstack/RT permission; old isolated virtual modulo-width profile unchanged",
                queries.len(),
                queries.last().unwrap().frame,
                refresh_requests.len(),
                elapsed.as_secs_f64() * 1000.0,
            );
        }
    }
}

#[test]
fn descriptive_work_boundary_replay_matches_actual_refreshes() {
    #[derive(Default)]
    struct Counts {
        left: usize,
        max_probe: usize,
        right: usize,
        terminal: bool,
    }

    fn replay(geometry: &MappedLoopGeometry, epoch: MappedClockEpoch, request: u64) -> Counts {
        let actual = geometry.refresh_span(epoch, request).unwrap();
        let phase = phase_at(
            epoch,
            geometry.beat_start,
            geometry.quarters,
            geometry.loaded_rate_hz,
            request,
        )
        .unwrap();
        let segment = forward_segment(geometry, &phase).unwrap();
        let cycle_base = phase.unfolded.checked_sub(phase.remainder).unwrap();
        let loop_end = geometry.beat_start + geometry.quarters;
        let left = segment.beat_start.max(geometry.beat_start);
        let right = segment.beat_end.min(loop_end);
        let left_target = cycle_base
            .checked_add(beat_offset(left, geometry.beat_start, phase.terms.source_delta).unwrap())
            .unwrap();
        let right_offset = if right == loop_end {
            phase.terms.period
        } else {
            beat_offset(right, geometry.beat_start, phase.terms.source_delta).unwrap()
        };
        let right_target = cycle_base.checked_add(right_offset).unwrap();
        assert!(phase.unfolded >= left_target && phase.unfolded < right_target);
        let mut counts = Counts::default();
        let start = first_true_near_high(epoch.first_output_frame, request, |frame| {
            counts.left += 1;
            Some(unfolded_at(epoch, &phase.terms, frame)? >= left_target)
        })
        .unwrap();
        counts.max_probe += 1;
        let terminal = unfolded_at(epoch, &phase.terms, u64::MAX).unwrap() < right_target;
        let end = if terminal {
            counts.terminal = true;
            u64::MAX
        } else {
            first_true_near_low(request.checked_add(1).unwrap(), u64::MAX, |frame| {
                counts.right += 1;
                Some(unfolded_at(epoch, &phase.terms, frame)? >= right_target)
            })
            .unwrap()
        };
        assert!(counts.left <= 65 && counts.right <= 66);
        assert_eq!(counts.max_probe, 1);
        assert_eq!(start, actual.start_frame());
        assert_eq!(end, actual.end_frame());
        if terminal {
            assert_eq!(counts.right, 0);
        }
        counts
    }

    let aligned_geometry = variable();
    let aligned_epoch = new_epoch(
        &aligned_geometry,
        timeline(16_000.0, 0, 0.0),
        pair(23_999, 1.0_f64.next_down().to_bits()),
        0,
    );
    for (name, geometry, epoch, first) in work_cases().into_iter().chain(std::iter::once((
        "aligned_knot",
        aligned_geometry,
        aligned_epoch,
        0,
    ))) {
        let (_, requests, _) = work_walk(&geometry, epoch, first);
        let mut total = Counts::default();
        let mut maximum_left = 0;
        let mut maximum_right = 0;
        let mut terminal_requests = 0;
        for &request in &requests {
            let counts = replay(&geometry, epoch, request);
            total.left += counts.left;
            total.max_probe += counts.max_probe;
            total.right += counts.right;
            maximum_left = maximum_left.max(counts.left);
            maximum_right = maximum_right.max(counts.right);
            terminal_requests += usize::from(counts.terminal);
        }
        if name == "virtual" || name == "aligned_knot" {
            assert!(requests.len() > 1);
        }
        eprintln!(
            "anchored_work_search_replay: shape={name} walk_frames=512 refresh_requests_including_initial={} left_predicates={} max_probes={} right_predicates={} maximum_left={maximum_left} maximum_right={maximum_right} terminal_requests={terminal_requests}; qualified near-probe-inclusive replay matched actual refreshed start/end, search bounds left65/right66; counts exclude phase setup/validation/endpoints and are not an instrumented production hotcall trace; no throughput/peakstack/RT permission",
            requests.len(),
            total.left,
            total.max_probe,
            total.right,
        );
    }
    // A separate terminal request exercises the actual plateau branch without a 512-frame
    // walk near MAX or any attempt to construct a renderable span at MAX itself.
    let geometry = variable();
    let epoch = new_epoch(&geometry, timeline(16_000.0, 0, 0.0), pair(0, 1), 0);
    let counts = replay(&geometry, epoch, u64::MAX - 1);
    assert!(counts.terminal);
    assert_eq!(counts.right, 0);
    eprintln!(
        "anchored_work_search_replay: shape=terminal_request_only walk_frames=0 refresh_requests=1 left_predicates={} max_probes={} right_predicates={} terminal_requests=1; qualified near-probe-inclusive MAX-1 replay matched actual span, one explicit MAX probe, no right search or refresh at MAX; no production trace/peakstack/RT claim",
        counts.left, counts.max_probe, counts.right,
    );
}

#[test]
fn qualified_near_searches_match_original_for_total_monotone_predicates() {
    fn compare(low: u64, high: u64, predicate: impl Fn(u64) -> bool) -> (usize, usize) {
        assert!(low <= high && predicate(high));
        let original = first_true(low, high, |frame| Some(predicate(frame)));
        let mut upper_calls = 0;
        let upper = first_true_near_high(low, high, |frame| {
            upper_calls += 1;
            Some(predicate(frame))
        });
        let mut lower_calls = 0;
        let lower = first_true_near_low(low, high, |frame| {
            lower_calls += 1;
            Some(predicate(frame))
        });
        assert_eq!(upper, original);
        assert_eq!(lower, original);
        assert!(upper_calls <= 65 && lower_calls <= 66);
        (upper_calls, lower_calls)
    }

    for low in 0..=16 {
        for high in low..=16 {
            for stride in [1, 2, 3, 8] {
                for threshold in 0..=high / stride {
                    compare(low, high, |frame| frame / stride >= threshold);
                }
            }
        }
    }
    let mut maximum_upper = 0;
    let mut maximum_lower = 0;
    for (low, high) in [
        (0, u64::MAX),
        (0, u64::MAX - 1),
        (1, u64::MAX),
        ((1_u64 << 53) - 3, (1_u64 << 53) + 1025),
        (u64::MAX - 1024, u64::MAX),
        (u64::MAX - 2, u64::MAX),
        (u64::MAX - 1, u64::MAX),
        (u64::MAX, u64::MAX),
    ] {
        for stride in [1, 2, 3, 64, 1024, 1_u64 << 53, u64::MAX] {
            for threshold in [
                0,
                low / stride,
                (low / stride).saturating_add(1),
                (low / stride).saturating_add(2),
                high / stride,
                (high / stride).saturating_sub(1),
            ] {
                if threshold <= high / stride {
                    let (upper, lower) = compare(low, high, |frame| frame / stride >= threshold);
                    maximum_upper = maximum_upper.max(upper);
                    maximum_lower = maximum_lower.max(lower);
                }
            }
        }
    }
    assert_eq!(maximum_upper, 65);
    assert_eq!(maximum_lower, 66);
}

#[test]
fn qualified_near_search_guards_and_probe_failures_do_not_change_generic_search() {
    for (low, high) in [(1, 0), (u64::MAX, u64::MAX - 1)] {
        assert!(first_true_near_high(low, high, |_| panic!("invalid interval probed")).is_none());
        assert!(first_true_near_low(low, high, |_| panic!("invalid interval probed")).is_none());
    }
    for frame in [0, 1, u64::MAX - 1, u64::MAX] {
        assert_eq!(first_true(frame, frame, |_| None), Some(frame));
        assert_eq!(first_true_near_high(frame, frame, |_| None), Some(frame));
        assert_eq!(first_true_near_low(frame, frame, |_| None), Some(frame));
    }
    let mut adjacent_calls = 0;
    assert_eq!(
        first_true_near_low(u64::MAX - 1, u64::MAX, |frame| {
            adjacent_calls += 1;
            Some(frame == u64::MAX)
        }),
        Some(u64::MAX)
    );
    assert_eq!(adjacent_calls, 1);

    // This non-total predicate deliberately distinguishes the unchanged generic search:
    // its original probes never visit 7. Qualified helpers do not promise equivalence here.
    let fallible = |frame| if frame == 7 { None } else { Some(true) };
    assert_eq!(first_true(0, 8, fallible), Some(0));
    assert!(first_true_near_high(0, 8, fallible).is_none());
    assert!(
        first_true_near_low(0, 8, |frame| { if frame == 0 { None } else { Some(true) } }).is_none()
    );
    assert!(
        first_true_near_low(0, 8, |frame| {
            match frame {
                0 => Some(false),
                1 => None,
                _ => Some(true),
            }
        })
        .is_none()
    );
    // None also propagates from the original binary search after successful local probes.
    assert!(
        first_true_near_high(0, 8, |frame| { if frame == 3 { None } else { Some(true) } })
            .is_none()
    );
    assert!(
        first_true_near_low(0, 8, |frame| {
            if frame == 5 { None } else { Some(frame >= 4) }
        })
        .is_none()
    );
}
