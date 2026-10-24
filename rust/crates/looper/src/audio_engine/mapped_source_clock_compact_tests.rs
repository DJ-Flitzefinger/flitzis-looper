use super::*;
use crate::audio_engine::mapped_source_clock::tests::map_with_source;
use crate::audio_engine::source_reader::FrameRange;
use crate::audio_engine::transport::TransportTimeline;

fn make_geometry(
    anchors: &[(f64, f64)],
    coverage: &[&str],
    rate: u32,
    frames: usize,
    region: FrameRange,
    quarters: f64,
) -> MappedLoopGeometry {
    MappedLoopGeometry::new(
        map_with_source(anchors, coverage, false, rate, frames),
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
    make_geometry(
        &[(0.0, 0.0), (0.5, 1.0), (1.0, 4.0)],
        &["trusted"; 2],
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

fn compare_span(
    geometry: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
) -> MappedSourceSpan {
    let compact = geometry.refresh_span_compact(epoch, frame).unwrap();
    let reference = geometry.refresh_span(epoch, frame).unwrap();
    assert!(compact.matches_exact(reference));
    let borrowed = compact.compact_view(geometry).unwrap();
    for query in [
        frame,
        compact.start_frame,
        compact.end_frame.saturating_sub(1),
        compact.end_frame,
    ] {
        if query >= compact.start_frame && query <= compact.end_frame {
            let observed = borrowed.position_at(query).unwrap();
            let expected = geometry.position_at(epoch, query).unwrap();
            assert_pair(observed, expected.frame, expected.fraction.to_bits());
        }
    }
    assert!(
        borrowed
            .position_at(compact.start_frame.saturating_sub(1))
            .is_none()
            || compact.start_frame == 0
    );
    compact
}

#[test]
fn compact_independent_fixture_retains_original_master_plateaus_fraction_carries_and_spans() {
    let geometry = variable();
    let e = 1_u64 << 53;
    let epoch = new_epoch(
        &geometry,
        timeline(16_000.0, e + 1, 3.125),
        pair(23_999, 0x3fef_ffff_ffff_ffff),
        e,
    );
    // Independent rational-fixture binary64 results, including the cast plateau at E.
    for (offset, whole, fraction_bits) in [
        (0, 23_999, 0x3fef_ffff_ffff_ffff),
        (1, 23_999, 0x3fef_ffff_ffff_ffff),
        (2, 24_001, 0x3d75_7fd5_5555_5555),
        (3, 24_001, 0x3fef_ffff_ffff_d900),
        (31_999, 40_000, 0),
        (32_000, 40_000, 0),
        (64_000, 23_999, 0x3fef_ffff_ffff_ffff),
        (u64::MAX - e, 39_000, 0),
    ] {
        let frame = e + offset;
        assert_pair(
            geometry.position_at_compact(epoch, frame).unwrap(),
            whole,
            fraction_bits,
        );
        if frame != u64::MAX {
            compare_span(&geometry, epoch, frame);
        }
    }
}

#[test]
fn compact_raw_negative_zero_integer_displacements_and_virtual_endpoint_keep_identity() {
    let geometry = variable();
    let epoch = new_epoch(
        &geometry,
        timeline(16_000.0, 0, 0.0),
        pair(0, 1_u64 << 63),
        0,
    );
    for (frame, whole) in [(0, 0), (4000, 6000), (16_000, 24_000), (64_000, 0)] {
        assert_pair(
            geometry.position_at_compact(epoch, frame).unwrap(),
            whole,
            1_u64 << 63,
        );
        compare_span(&geometry, epoch, frame);
    }
    let virtual_loop = make_geometry(
        &[(0.0, 0.0), (9.0 / 32768.0, 2.0_f64.powi(-13))],
        &["trusted"],
        8192,
        3,
        FrameRange { start: 0, end: 2 },
        2.0_f64.powi(-13),
    );
    let epoch = new_epoch(
        &virtual_loop,
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
        assert_pair(
            virtual_loop.position_at_compact(epoch, frame).unwrap(),
            whole,
            fraction,
        );
        compare_span(&virtual_loop, epoch, frame);
    }
}

#[test]
fn compact_partial_loop_keeps_negative_segment_offsets_and_ignores_unused_extreme_beats() {
    let partial = make_geometry(
        &[(0.0, 0.0), (0.5, 1.0), (1.0, 4.0)],
        &["trusted"; 2],
        48_000,
        96_000,
        FrameRange {
            start: 12_000,
            end: 40_000,
        },
        2.5,
    );
    let epoch = new_epoch(&partial, timeline(16_000.0, 0, 0.0), pair(12_000, 0), 0);
    for (frame, whole) in [
        (0, 12_000),
        (4000, 18_000),
        (8000, 24_000),
        (40_000, 12_000),
    ] {
        assert_pair(partial.position_at_compact(epoch, frame).unwrap(), whole, 0);
        compare_span(&partial, epoch, frame);
    }
    let exterior = make_geometry(
        &[
            (0.0, -2.0_f64.powi(500)),
            (0.25, 0.0),
            (0.5, 1.0),
            (1.0, 2.0_f64.powi(500)),
        ],
        &["uncertain", "trusted", "uncertain"],
        4,
        8,
        FrameRange { start: 1, end: 2 },
        1.0,
    );
    let epoch = new_epoch(
        &exterior,
        timeline(1.0, 0, 0.0),
        pair(1, 0.125_f64.to_bits()),
        0,
    );
    for frame in [0, 1, 2, 101, u64::MAX - 1] {
        compare_span(&exterior, epoch, frame);
    }
}

#[test]
fn compact_projection_replacement_and_pause_requalify_horizon_keep_original_m0() {
    let geometry = variable();
    let e = 1_u64 << 53;
    let actual = timeline(16_000.0, e + 1, 3.125);
    let original = new_epoch(&geometry, actual, pair(23_999, 0x3fef_ffff_ffff_ffff), e);
    let current = e + 32_000;
    let changed = timeline(
        12_000.0,
        current,
        original.addressed_master_at(current).unwrap(),
    );
    let replacement = original.with_timeline_at(changed, current).unwrap();
    assert_pair(
        geometry.position_at_compact(replacement, current).unwrap(),
        40_000,
        0,
    );
    assert!(
        geometry
            .position_at_compact(replacement, current - 1)
            .is_err()
    );
    for frame in [current, current + 1, current + 28_001, u64::MAX - 1] {
        compare_span(&geometry, replacement, frame);
    }
    let resumed_at = current + 31;
    let resumed = original.resumed_at(&actual, current, resumed_at).unwrap();
    for delta in [0, 1, 2, 12_000] {
        let expected = geometry.position_at(original, current + delta).unwrap();
        let observed = geometry
            .position_at_compact(resumed, resumed_at + delta)
            .unwrap();
        assert_pair(observed, expected.frame, expected.fraction.to_bits());
        compare_span(&geometry, resumed, resumed_at + delta);
    }
    let too_slow = original
        .with_timeline_at(
            timeline(
                32_000.0,
                current,
                original.addressed_master_at(current).unwrap(),
            ),
            current,
        )
        .unwrap();
    assert_eq!(
        geometry.position_at_compact(too_slow, current),
        Err(MappedClockError::UnsupportedRate)
    );
}

#[test]
fn compact_class_refusal_preserves_original_wide_qualification_and_raw_subnormal_path() {
    let tiny = 2.0_f64.powi(-200);
    let geometry = make_geometry(
        &[(0.0, 0.0), (tiny, tiny), (1.0, 1.0)],
        &["trusted"; 2],
        1,
        2,
        FrameRange { start: 0, end: 1 },
        1.0,
    );
    assert!(geometry.exact_continuation.unwrap().compact.is_none());
    let epoch = new_epoch(&geometry, timeline(1.0, 0, 0.0), pair(0, 0), 0);
    assert_pair(geometry.position_at(epoch, 0).unwrap(), 0, 0);
    assert!(geometry.position_at_compact(epoch, 0).is_err());
    assert!(geometry.refresh_span_compact(epoch, 0).is_err());
    let geometry = variable();
    let epoch = new_epoch(&geometry, timeline(16_000.0, 0, 0.0), pair(0, 1), 0);
    assert_pair(geometry.position_at(epoch, 0).unwrap(), 0, 1);
    assert!(geometry.position_at_compact(epoch, 0).is_err());
    assert!(
        geometry
            .refresh_span(epoch, 0)
            .unwrap()
            .compact_view(&geometry)
            .is_err()
    );
}

#[test]
fn compact_wrong_geometry_and_stale_cached_endpoint_refuse_without_owner_claim() {
    let geometry = variable();
    let epoch = new_epoch(&geometry, timeline(16_000.0, 0, 0.0), pair(0, 0), 0);
    let span = geometry.refresh_span_compact(epoch, 0).unwrap();
    let other = make_geometry(
        &[(0.0, 0.0), (0.25, 1.0), (1.0, 4.0)],
        &["trusted"; 2],
        48_000,
        96_000,
        FrameRange {
            start: 0,
            end: 48_000,
        },
        4.0,
    );
    assert!(span.compact_view(&other).is_err());
    let mut changed_endpoint = span;
    changed_endpoint.endpoint_fraction = 0.5;
    assert!(changed_endpoint.compact_view(&geometry).is_err());
    let mut changed_period = span;
    let mut program = changed_period.legacy_program().unwrap();
    program.source_period =
        f64::from_bits(span.legacy_program().unwrap().source_period.to_bits() + 1);
    changed_period.program = super::super::SpanProgram::legacy(program);
    assert!(changed_period.compact_view(&geometry).is_err());
    let mut changed_inverse = epoch;
    let MappedClockMode::Anchored(mut anchor) = epoch.mode else {
        unreachable!()
    };
    anchor.inverse_segment.source_end *= 0.5;
    changed_inverse.mode = MappedClockMode::Anchored(anchor);
    assert!(geometry.position_at_compact(changed_inverse, 0).is_err());
    // Equal original data is still numeric compatibility only. No owner token is manufactured.
    assert!(span.compact_view(&geometry.clone()).is_ok());
}

#[test]
fn compact_terminal_plateau_and_endpoint_normalization_keep_original_refusals() {
    let identity = make_geometry(
        &[(0.0, 0.0), (1.0, 1.0)],
        &["trusted"],
        1,
        2,
        FrameRange { start: 0, end: 1 },
        1.0,
    );
    let e = u64::MAX - 10;
    let epoch = new_epoch(
        &identity,
        timeline(1.0, 0, 0.0),
        pair(0, 0.25_f64.to_bits()),
        e,
    );
    let span = compare_span(&identity, epoch, e);
    assert_eq!(span.end_frame(), u64::MAX);
    assert_pair(
        span.compact_view(&identity)
            .unwrap()
            .position_at(u64::MAX)
            .unwrap(),
        0,
        0.25_f64.to_bits(),
    );
    assert!(identity.refresh_span_compact(epoch, u64::MAX).is_err());
    let epoch = new_epoch(
        &identity,
        timeline(2.0, 0, 0.0),
        pair(0, 0x3fdf_ffff_ffff_ffff),
        0,
    );
    assert_pair(identity.position_at_compact(epoch, 1).unwrap(), 0, 0);
    compare_span(&identity, epoch, 0);
    compare_span(&identity, epoch, 1);
}

#[test]
fn compact_tiny_fpb_terminal_plateau_keeps_exact_rate_original_period_and_raw_pair() {
    let geometry = make_geometry(
        &[(0.0, 0.0), (0.5, 2.0_f64.powi(125))],
        &["trusted"],
        1,
        2,
        FrameRange { start: 0, end: 1 },
        2.0_f64.powi(125),
    );
    assert_eq!(geometry.domain().musical_period.scalar_period(), Some(0.5));
    let mut invalid = TransportTimeline::new(1);
    assert!(
        !invalid.set_master_period_and_anchor_beat_position_at_frame(
            2.0_f64.powi(-127),
            0.0,
            u64::MAX,
        )
    );
    let actual = timeline(2.0_f64.powi(-127), 1_u64 << 63, -2.0_f64.powi(190));
    let e = u64::MAX - 1;
    // The actual FPB requires a finer grid than -126, while both addressed
    // frames cast to 2^64. The legal original projection evaluates exactly
    // -2^190 + (2^64 - 2^63) / 2^-127 = +0 on this terminal plateau.
    for raw_bits in [0.25_f64.to_bits(), (-0.0_f64).to_bits()] {
        let epoch = new_epoch(&geometry, actual, pair(0, raw_bits), e);
        assert_eq!(epoch.addressed_master_at(e).unwrap().to_bits(), 0);
        assert_eq!(epoch.addressed_master_at(u64::MAX).unwrap().to_bits(), 0);
        for frame in [e, u64::MAX] {
            assert_pair(geometry.position_at(epoch, frame).unwrap(), 0, raw_bits);
            assert_pair(
                geometry.position_at_compact(epoch, frame).unwrap(),
                0,
                raw_bits,
            );
        }
        let span = compare_span(&geometry, epoch, e);
        assert_eq!(span.end_frame(), u64::MAX);
        assert_eq!(span.local_rate(), Some(2.0));
        assert_eq!(
            span.legacy_program().unwrap().source_period.to_bits(),
            0.5_f64.to_bits()
        );
        assert_pair(
            span.compact_view(&geometry)
                .unwrap()
                .position_at(u64::MAX)
                .unwrap(),
            0,
            raw_bits,
        );
        assert!(geometry.refresh_span_compact(epoch, u64::MAX).is_err());
        assert!(geometry.refresh_span(epoch, u64::MAX).is_err());
    }
}

fn productive_epoch(
    geometry: &MappedLoopGeometry,
    actual: TransportTimeline,
    position: FractionalSourcePosition,
    frame: u64,
) -> MappedClockEpoch {
    MappedClockEpoch::anchored_compact(7, actual, geometry, geometry.domain(), position, frame)
        .unwrap()
}

#[test]
fn actual_e_compact_constructor_retains_original_inverse_and_distinct_mode_without_wide_work() {
    let geometry = variable();
    let first = (1_u64 << 53) + 7;
    let actual = timeline(16_000.0, first - 6, 3.125);
    for raw in [
        pair(17, (-0.0_f64).to_bits()),
        pair(23_999, 0x3fef_ffff_ffff_ffff),
    ] {
        let (wide, (_, wide_entries)) =
            observe_compact_queries_for_test(|| new_epoch(&geometry, actual, raw, first));
        assert!(wide_entries > 0);
        let (compact, (_, wide_entries)) =
            observe_compact_queries_for_test(|| productive_epoch(&geometry, actual, raw, first));
        assert_eq!(wide_entries, 0);
        assert!(compact.is_compact_anchored());
        assert!(!wide.is_compact_anchored());
        assert!(!compact.matches_exact(wide));
        assert!(compact.matches_exact(compact));
        assert!(
            compact
                .anchor()
                .unwrap()
                .matches_exact(wide.anchor().unwrap())
        );
        assert!(compact.projection.matches_exact(wide.projection));
        assert!(!compact.replacement_anchor_matches(wide, first));
        for frame in [
            first,
            first + 1,
            first + 32_000,
            first + 96_001,
            u64::MAX - 1,
        ] {
            // The separately constructed old epoch keeps this an independent wide oracle.
            let expected = geometry.position_at(wide, frame).unwrap();
            let (observed, (_, wide_entries)) = observe_compact_queries_for_test(|| {
                let span = geometry.refresh_span(compact, frame).unwrap();
                let local = span.local_compact_view().unwrap();
                let observed = geometry.position_at(compact, frame).unwrap();
                assert_pair(
                    local.position_at(frame).unwrap(),
                    observed.frame,
                    observed.fraction.to_bits(),
                );
                assert_pair(
                    span.position_at(frame).unwrap(),
                    observed.frame,
                    observed.fraction.to_bits(),
                );
                observed
            });
            assert_eq!(wide_entries, 0);
            assert_pair(observed, expected.frame, expected.fraction.to_bits());
        }
    }
}

#[test]
fn actual_e_inverse_orders_unused_finite_source_probes_outside_the_compact_grid() {
    let tiny = 2.0_f64.powi(-200);
    let geometry = make_geometry(
        &[
            (0.0, 0.0),
            (tiny, tiny),
            (tiny * 2.0, tiny * 2.0),
            (1.0, 1.0),
            (2.0, 2.0),
        ],
        &["uncertain", "uncertain", "uncertain", "trusted"],
        1,
        3,
        FrameRange { start: 1, end: 2 },
        1.0,
    );
    assert!(geometry.has_compact_continuation());
    let actual = timeline(1.0, 0, 0.0);
    let raw = pair(1, 0.25_f64.to_bits());
    let wide = new_epoch(&geometry, actual, raw, 0);
    let compact = productive_epoch(&geometry, actual, raw, 0);
    assert!(
        compact
            .anchor()
            .unwrap()
            .matches_exact(wide.anchor().unwrap())
    );
    for frame in [0, 1, 2, 99, u64::MAX - 1] {
        let a = geometry.position_at(compact, frame).unwrap();
        let b = geometry.position_at(wide, frame).unwrap();
        assert_pair(a, b.frame, b.fraction.to_bits());
    }
}

#[test]
fn tagged_local_coarser_grids_preserve_global_and_wide_queries_and_endpoints() {
    let tiny = 2.0_f64.powi(-60);
    let geometry = make_geometry(
        &[(0.0, 0.0), (tiny, tiny), (1.0, 1.0), (2.0, 4.0)],
        &["trusted"; 3],
        1,
        3,
        FrameRange { start: 0, end: 2 },
        4.0,
    );
    let actual = timeline(0.5, 0, 0.0);
    let raw = pair(1, 0.25_f64.to_bits());
    let compact = productive_epoch(&geometry, actual, raw, 0);
    let wide = new_epoch(&geometry, actual, raw, 0);
    let global = CompactClock::new(&geometry, compact).unwrap();
    let span = geometry.refresh_span(compact, 0).unwrap();
    let local = span.local_compact_view().unwrap();
    assert!(local.core.source_grid.exponent() > global.core.source_grid.exponent());
    assert!(local.core.beat_grid.exponent() > global.core.beat_grid.exponent());
    for frame in [span.start_frame(), span.end_frame() - 1, span.end_frame()] {
        let expected = geometry.position_at(wide, frame).unwrap();
        assert_pair(
            local.position_at(frame).unwrap(),
            expected.frame,
            expected.fraction.to_bits(),
        );
        assert_pair(
            global.position_at(frame).unwrap(),
            expected.frame,
            expected.fraction.to_bits(),
        );
    }
    assert!(
        geometry
            .refresh_span(wide, 0)
            .unwrap()
            .local_compact_view()
            .is_err()
    );
}

#[test]
fn actual_e_compact_constructor_preserves_static_dynamic_and_terminal_refusals() {
    let geometry = variable();
    let actual = timeline(16_000.0, 0, 0.0);
    let raw = pair(17, 0.25_f64.to_bits());
    for (marker, frame) in [(0, 0), (7, u64::MAX)] {
        assert!(matches!(
            MappedClockEpoch::anchored_compact(
                marker,
                actual,
                &geometry,
                geometry.domain(),
                raw,
                frame
            ),
            Err(MappedClockError::InvalidEpoch)
        ));
    }
    for position in [pair(17, 1), pair(48_000, 0), pair(17, 1.0_f64.to_bits())] {
        assert!(
            MappedClockEpoch::anchored_compact(
                7,
                actual,
                &geometry,
                geometry.domain(),
                position,
                0
            )
            .is_err()
        );
    }
    let mut changed = geometry.domain();
    changed.musical_period =
        crate::audio_engine::source_reader_period::SourceReaderPeriod::from_scalar_period(Some(
            f64::from_bits(changed.musical_period.scalar_period().unwrap().to_bits() + 1),
        ));
    assert!(MappedClockEpoch::anchored_compact(7, actual, &geometry, changed, raw, 0).is_err());
    let slow = timeline(32_000.0, 0, 0.0);
    assert!(matches!(
        MappedClockEpoch::anchored_compact(7, slow, &geometry, geometry.domain(), raw, 0),
        Err(MappedClockError::UnsupportedRate)
    ));
    let tiny = 2.0_f64.powi(-200);
    let static_unavailable = make_geometry(
        &[(0.0, 0.0), (tiny, tiny), (1.0, 1.0)],
        &["trusted"; 2],
        1,
        2,
        FrameRange { start: 0, end: 1 },
        1.0,
    );
    assert!(!static_unavailable.has_compact_continuation());
    assert!(
        MappedClockEpoch::anchored_compact(
            7,
            timeline(1.0, 0, 0.0),
            &static_unavailable,
            static_unavailable.domain(),
            pair(0, 0),
            0
        )
        .is_err()
    );
    // Its old exact qualification and explicit reference remain available.
    let wide = new_epoch(&static_unavailable, timeline(1.0, 0, 0.0), pair(0, 0), 0);
    assert_pair(static_unavailable.position_at(wide, 0).unwrap(), 0, 0);
}

#[test]
fn tagged_tiny_fpb_terminal_plateau_and_shifted_horizons_keep_original_raw_anchor() {
    let geometry = make_geometry(
        &[(0.0, 0.0), (0.5, 2.0_f64.powi(125))],
        &["trusted"],
        1,
        2,
        FrameRange { start: 0, end: 1 },
        2.0_f64.powi(125),
    );
    let actual = timeline(2.0_f64.powi(-127), 1_u64 << 63, -2.0_f64.powi(190));
    let first = u64::MAX - 1;
    for bits in [0.25_f64.to_bits(), (-0.0_f64).to_bits()] {
        let compact = productive_epoch(&geometry, actual, pair(0, bits), first);
        let wide = new_epoch(&geometry, actual, pair(0, bits), first);
        let span = geometry.refresh_span(compact, first).unwrap();
        let local = span.local_compact_view().unwrap();
        for frame in [first, u64::MAX] {
            assert_pair(local.position_at(frame).unwrap(), 0, bits);
            assert_pair(geometry.position_at(wide, frame).unwrap(), 0, bits);
        }
        assert_eq!(span.local_rate(), Some(2.0));
        assert!(geometry.refresh_span(compact, u64::MAX).is_err());
        assert_eq!(compact.anchor().unwrap().original_master.to_bits(), 0);
    }
}
