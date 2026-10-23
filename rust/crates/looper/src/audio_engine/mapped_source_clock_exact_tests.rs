use super::*;
use crate::audio_engine::mapped_source_clock::tests::map_with_source;
use crate::audio_engine::source_reader::FrameRange;
use crate::audio_engine::transport::TransportTimeline;

const RATE: u32 = 48_000;
const FRAMES: usize = 96_000;
const VARIABLE: &[(f64, f64)] = &[(0.0, 0.0), (0.5, 1.0), (1.0, 4.0)];

fn geometry(
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
}

fn variable() -> MappedLoopGeometry {
    geometry(
        VARIABLE,
        &["trusted", "trusted"],
        RATE,
        FRAMES,
        FrameRange {
            start: 0,
            end: 48_000,
        },
        4.0,
    )
}

fn epoch(fpb: f64) -> MappedClockEpoch {
    let mut timeline = TransportTimeline::new(1);
    assert!(timeline.set_master_period(fpb));
    MappedClockEpoch::new(7, timeline, 0.0).unwrap()
}

#[test]
fn independent_fraction_eligibility_vectors_use_actual_legacy_geometry() {
    // Expected bits and eligibility are independent Fraction/struct oracle vectors 1..10:
    // test-artifacts/anchored-geometry-qualification-20261023/eligibility-oracle.md.
    // These are geometry fixtures, not accepted source, ACK, PCM or music evidence.
    struct Case {
        anchors: Vec<(f64, f64)>,
        rate: u32,
        frames: usize,
        region: FrameRange,
        beat_bits: u64,
        quarters_bits: u64,
        period_bits: u64,
        eligible: bool,
    }
    let cases = [
        Case {
            anchors: VARIABLE.to_vec(),
            rate: RATE,
            frames: FRAMES,
            region: FrameRange {
                start: 0,
                end: 48_000,
            },
            beat_bits: 0,
            quarters_bits: 0x4010_0000_0000_0000,
            period_bits: 0x40e7_7000_0000_0000,
            eligible: true,
        },
        Case {
            anchors: VARIABLE.to_vec(),
            rate: RATE,
            frames: FRAMES,
            region: FrameRange {
                start: 24_000,
                end: 48_000,
            },
            beat_bits: 0x3ff0_0000_0000_0000,
            quarters_bits: 0x4008_0000_0000_0000,
            period_bits: 0x40d7_7000_0000_0000,
            eligible: true,
        },
        Case {
            anchors: vec![(0.0, 0.0), (1.0, 2.0)],
            rate: RATE,
            frames: FRAMES,
            region: FrameRange {
                start: 12_000,
                end: 36_000,
            },
            beat_bits: 0x3fe0_0000_0000_0000,
            quarters_bits: 0x3ff0_0000_0000_0000,
            period_bits: 0x40d7_7000_0000_0000,
            eligible: true,
        },
        Case {
            anchors: vec![
                (0.0, 0.0),
                (
                    f64::from_bits(0x3f24_0000_0000_0000),
                    f64::from_bits(0x3f10_0000_0000_0000),
                ),
            ],
            rate: 8192,
            frames: 2,
            region: FrameRange { start: 0, end: 1 },
            beat_bits: 0,
            quarters_bits: 0x3f10_0000_0000_0000,
            period_bits: 0x3ff4_0000_0000_0000,
            eligible: true,
        },
        Case {
            anchors: vec![
                (0.0, 0.0),
                (
                    f64::from_bits(0x3f18_0000_0000_0000),
                    f64::from_bits(0x3f10_0000_0000_0000),
                ),
            ],
            rate: 8192,
            frames: 2,
            region: FrameRange { start: 0, end: 1 },
            beat_bits: 0,
            quarters_bits: 0x3f10_0000_0000_0000,
            period_bits: 0x3fe8_0000_0000_0000,
            eligible: true,
        },
        Case {
            anchors: vec![(0.0, 0.0), (0.1, 1.0), (0.2, 2.0)],
            rate: 10,
            frames: 3,
            region: FrameRange { start: 1, end: 2 },
            beat_bits: 0x3ff0_0000_0000_0000,
            quarters_bits: 0x3ff0_0000_0000_0000,
            period_bits: 0x3ff0_0000_0000_0000,
            eligible: false,
        },
        Case {
            anchors: vec![(0.0, 0.0), (0.5, 1.0), (1.0, 4.0), (1.1, 5.0)],
            rate: RATE,
            frames: 60_000,
            region: FrameRange {
                start: 0,
                end: 24_000,
            },
            beat_bits: 0,
            quarters_bits: 0x3ff0_0000_0000_0000,
            period_bits: 0x40d7_7000_0000_0000,
            eligible: true,
        },
        Case {
            anchors: vec![(0.0, 0.0), (0.1, 0.5), (0.5, 2.0), (1.0, 4.0)],
            rate: RATE,
            frames: FRAMES,
            region: FrameRange {
                start: 0,
                end: 48_000,
            },
            beat_bits: 0,
            quarters_bits: 0x4010_0000_0000_0000,
            period_bits: 0x40e7_7000_0000_0000,
            eligible: false,
        },
        Case {
            anchors: vec![
                (0.0, f64::from_bits(0x4360_0000_0000_0001)),
                (32.0, f64::from_bits(0x4360_0000_0000_0011)),
            ],
            rate: RATE,
            frames: 2_000_000,
            region: FrameRange {
                start: 0,
                end: 96_000,
            },
            beat_bits: 0x4360_0000_0000_0001,
            quarters_bits: 0x4010_0000_0000_0000,
            period_bits: 0x40f7_7000_0000_0000,
            eligible: false,
        },
        Case {
            anchors: vec![(0.0, 0.0), (2.0, 7.0)],
            rate: RATE,
            frames: 100_000,
            region: FrameRange {
                start: 0,
                end: 13_714,
            },
            beat_bits: 0,
            quarters_bits: 0x3ff0_0000_0000_0000,
            period_bits: 0x40ca_c924_9249_2492,
            eligible: false,
        },
    ];
    for (index, case) in cases.into_iter().enumerate() {
        let coverage = vec!["trusted"; case.anchors.len() - 1];
        let original = geometry(
            &case.anchors,
            &coverage,
            case.rate,
            case.frames,
            case.region,
            f64::from_bits(case.quarters_bits),
        );
        assert_eq!(
            original.start_beat().to_bits(),
            case.beat_bits,
            "vector {} B",
            index + 1
        );
        assert_eq!(original.exact_quarters().to_bits(), case.quarters_bits);
        assert_eq!(
            original.domain().musical_period.unwrap().to_bits(),
            case.period_bits,
            "vector {} P",
            index + 1
        );
        assert!(!original.exact_continuation_matches_domain(original.domain()));
        let result = original.clone().qualify_exact_continuation();
        assert_eq!(result.is_ok(), case.eligible, "vector {}", index + 1);
        if let Ok(qualified) = result {
            assert!(qualified.exact_continuation_matches_domain(original.domain()));
            assert_eq!(qualified.domain(), original.domain());
            assert_eq!(
                qualified.map().canonical_record(),
                original.map().canonical_record()
            );
            assert!(
                qualified
                    .validate_exact_continuation_epoch(epoch(16_000.0))
                    .is_ok()
            );
        } else {
            assert!(matches!(
                result,
                Err(MappedClockError::NumericalUnavailable)
            ));
        }
    }
}

#[test]
fn qualification_and_clone_retain_original_owner_fields_and_legacy_queries() {
    fn require_copy<T: Copy>() {}
    require_copy::<ExactContinuationQualification>();
    assert!(!std::mem::needs_drop::<ExactContinuationQualification>());
    let original = variable();
    let original_record = original.map().canonical_record().as_ptr();
    let qualified = original.clone().qualify_exact_continuation().unwrap();
    let cloned = qualified.clone();
    assert_eq!(qualified.map().canonical_record().as_ptr(), original_record);
    assert_eq!(cloned.map().canonical_record().as_ptr(), original_record);
    assert_eq!(
        qualified.start_beat().to_bits(),
        original.start_beat().to_bits()
    );
    assert_eq!(
        qualified.exact_quarters().to_bits(),
        original.exact_quarters().to_bits()
    );
    assert_eq!(
        qualified.unrounded_source_end().to_bits(),
        original.unrounded_source_end().to_bits()
    );
    assert!(cloned.exact_continuation_matches_domain(original.domain()));
    assert!(
        cloned
            .qualify_exact_continuation()
            .unwrap()
            .exact_continuation_matches_domain(original.domain())
    );
    for frame in [0, 1, 15_999, 16_000, 63_999, 64_000, 1_000_003] {
        let old = original.position_at(epoch(16_000.0), frame).unwrap();
        let prepared = qualified.position_at(epoch(16_000.0), frame).unwrap();
        assert_eq!(old.frame, prepared.frame);
        assert_eq!(old.fraction.to_bits(), prepared.fraction.to_bits());
        assert_eq!(old.seek_mode, prepared.seek_mode);
    }
}

#[test]
fn qualified_reader_match_requires_complete_original_domain() {
    let qualified = variable().qualify_exact_continuation().unwrap();
    let domain = qualified.domain();
    assert!(qualified.exact_continuation_matches_domain(domain));
    let different_period = SourceLoopDomain::musical(
        domain.sample_frames,
        domain.region,
        f64::from_bits(domain.musical_period.unwrap().to_bits() + 1),
    )
    .unwrap();
    let physical = SourceLoopDomain::physical(domain.sample_frames, domain.region);
    let different_source = SourceLoopDomain {
        sample_frames: domain.sample_frames + 1,
        ..domain
    };
    let different_start = SourceLoopDomain {
        region: FrameRange {
            start: 1,
            ..domain.region
        },
        ..domain
    };
    let different_end = SourceLoopDomain {
        region: FrameRange {
            end: domain.region.end - 1,
            ..domain.region
        },
        ..domain
    };
    for other in [
        different_period,
        physical,
        different_source,
        different_start,
        different_end,
    ] {
        assert!(!qualified.exact_continuation_matches_domain(other));
    }
}

#[test]
fn partial_variable_loop_closes_at_exact_integer_end_without_replacing_knots() {
    let original = geometry(
        VARIABLE,
        &["trusted", "trusted"],
        RATE,
        FRAMES,
        FrameRange {
            start: 12_000,
            end: 40_000,
        },
        2.5,
    );
    assert_eq!(original.start_beat(), 0.5);
    assert_eq!(original.domain().musical_period, Some(28_000.0));
    let qualified = original.qualify_exact_continuation().unwrap();
    let proof = qualified.exact_continuation.unwrap();
    assert_eq!(proof.minimum.source_start, 0.5);
    assert_eq!(proof.minimum.source_end, 1.0);
    assert_eq!(proof.maximum.source_start, 0.0);
    assert_eq!(proof.maximum.source_end, 0.5);
    qualified
        .validate_exact_continuation_epoch(epoch(16_000.0))
        .unwrap();
}

#[test]
fn exterior_gaps_and_inexact_unused_knots_do_not_veto_either_endpoint() {
    let original = geometry(
        &[(0.1, 0.0), (0.5, 1.0), (1.0, 4.0), (1.1, 5.0)],
        &["gap_unknown_count", "trusted", "uncertain"],
        RATE,
        FRAMES,
        FrameRange {
            start: 24_000,
            end: 48_000,
        },
        3.0,
    );
    let qualified = original.qualify_exact_continuation().unwrap();
    let proof = qualified.exact_continuation.unwrap();
    assert_eq!(proof.minimum.source_start, 0.5);
    assert_eq!(proof.maximum.source_end, 1.0);
    // FPB4000 gives exactly 2; the unused left slope is over twice this maximum.
    qualified
        .validate_exact_continuation_epoch(epoch(4000.0))
        .unwrap();
}

#[test]
fn borrowed_segment_visit_keeps_original_knots_and_positive_interior_coverage() {
    let map = map_with_source(
        &[(0.0, 0.0), (0.5, 1.0), (1.0, 4.0), (2.0, 5.0)],
        &["gap_unknown_count", "trusted", "uncertain"],
        false,
        RATE,
        FRAMES,
    );
    let mut visited = Vec::new();
    map.visit_canonical_segments(1.5, 3.5, |segment| {
        visited.push(segment);
        Some(())
    })
    .unwrap();
    assert_eq!(visited.len(), 1);
    assert_eq!(visited[0].source_start.to_bits(), 0.5_f64.to_bits());
    assert_eq!(visited[0].source_end.to_bits(), 1.0_f64.to_bits());
    assert_eq!(visited[0].beat_start, 1.0);
    assert_eq!(visited[0].beat_end, 4.0);
    let mut count = 0;
    map.visit_canonical_segments(1.0, 4.0, |_| {
        count += 1;
        Some(())
    })
    .unwrap();
    assert_eq!(count, 1);
    for (start, end) in [
        (0.0, 4.0),
        (1.0, 5.0),
        (1.0, 1.0),
        (4.0, 1.0),
        (-1.0, 1.0),
        (1.0, 6.0),
        (f64::NAN, 4.0),
    ] {
        assert!(
            map.visit_canonical_segments(start, end, |_| panic!("unproved range visited"))
                .is_none()
        );
    }
}

#[test]
fn borrowed_segment_visit_propagates_failure_without_visiting_later_segments() {
    let map = map_with_source(VARIABLE, &["trusted", "trusted"], false, RATE, FRAMES);
    let mut count = 0;
    assert!(
        map.visit_canonical_segments(0.0, 4.0, |_| {
            count += 1;
            None
        })
        .is_none()
    );
    assert_eq!(count, 1);
}

#[test]
fn exact_rate_extrema_include_both_bounds_and_refuse_adjacent_values() {
    let qualified = variable().qualify_exact_continuation().unwrap();
    qualified
        .validate_exact_continuation_epoch(epoch(12_000.0))
        .unwrap();
    qualified
        .validate_exact_continuation_epoch(epoch(16_000.0))
        .unwrap();
    for fpb in [
        f64::from_bits(12_000.0_f64.to_bits() - 1),
        f64::from_bits(16_000.0_f64.to_bits() + 1),
    ] {
        assert_eq!(
            qualified.validate_exact_continuation_epoch(epoch(fpb)),
            Err(MappedClockError::UnsupportedRate)
        );
    }
    assert_eq!(
        variable().validate_exact_continuation_epoch(epoch(16_000.0)),
        Err(MappedClockError::NumericalUnavailable)
    );
}

#[test]
fn independent_exact_rates_refuse_when_legacy_rounding_hides_boundary_crossing() {
    // Independent Fraction oracle: the first exact rate is
    // 9007199254740992/18014398509481985 < .5; the second is
    // 36028797018963968/18014398509481983 > 2. Legacy rounds to the admitted bounds.
    for (quarters, period_bits, fpb_bits, rounded_rate) in [
        (5.0, 0x3f09_9999_9999_999a, 0x3fd9_9999_9999_999a, 0.5),
        (3.0, 0x3ef5_5555_5555_5555, 0x3fc5_5555_5555_5555, 2.0),
    ] {
        let original = geometry(
            &[
                (0.0, 0.0),
                (f64::from_bits(0x3f20_0000_0000_0000), quarters),
            ],
            &["trusted"],
            8192,
            2,
            FrameRange { start: 0, end: 1 },
            quarters,
        );
        assert_eq!(original.domain().musical_period, Some(1.0));
        let mut timeline = TransportTimeline::new(8192);
        assert!(timeline.set_master_period(f64::from_bits(period_bits)));
        let epoch = MappedClockEpoch::new(7, timeline, 0.0).unwrap();
        assert_eq!(epoch.projection.frames_per_beat().to_bits(), fpb_bits);
        original.validate_epoch(epoch).unwrap();
        assert_eq!(
            f64::from(original.loaded_rate_hz) * original.min_seconds_per_quarter
                / epoch.projection.frames_per_beat(),
            rounded_rate,
        );
        let qualified = original.qualify_exact_continuation().unwrap();
        assert_eq!(
            qualified.validate_exact_continuation_epoch(epoch),
            Err(MappedClockError::UnsupportedRate),
        );
    }
}

#[test]
fn exact_extrema_remain_correct_when_slow_and_fast_segments_reverse_order() {
    let qualified = geometry(
        &[(0.0, 0.0), (0.5, 3.0), (1.0, 4.0)],
        &["trusted", "trusted"],
        RATE,
        FRAMES,
        FrameRange {
            start: 0,
            end: 48_000,
        },
        4.0,
    )
    .qualify_exact_continuation()
    .unwrap();
    let proof = qualified.exact_continuation.unwrap();
    assert_eq!(proof.minimum.beat_start, 0.0);
    assert_eq!(proof.maximum.beat_start, 3.0);
    qualified
        .validate_exact_continuation_epoch(epoch(12_000.0))
        .unwrap();
    qualified
        .validate_exact_continuation_epoch(epoch(16_000.0))
        .unwrap();
}

#[test]
fn finite_wide_original_beat_offsets_and_tiny_extents_are_exactly_qualified() {
    for (beat_start, quarters, fpb) in [
        (2.0_f64.powi(1000), 2.0_f64.powi(952), 2.0_f64.powi(-952)),
        (0.0, 2.0_f64.powi(-1020), 2.0_f64.powi(1020)),
    ] {
        let original = geometry(
            &[(0.0, beat_start), (1.0, beat_start + quarters)],
            &["trusted"],
            1,
            2,
            FrameRange { start: 0, end: 1 },
            quarters,
        );
        let qualified = original.qualify_exact_continuation().unwrap();
        qualified
            .validate_exact_continuation_epoch(epoch(fpb))
            .unwrap();
    }
}

#[test]
fn exact_musical_guard_refuses_a_wide_domain_hidden_by_float_length_rounding() {
    let mut numeric_fixture = variable();
    // Deliberately malformed private numeric state, not a claimed legacy constructor result.
    // L=2^53+1 and P=2^53-1 differ by 2, although binary64 L makes the gap appear 1.
    let length = (1_usize << 53) + 1;
    let period = (1_u64 << 53) as f64 - 1.0;
    let domain = SourceLoopDomain::musical(
        length,
        FrameRange {
            start: 0,
            end: length,
        },
        period,
    )
    .unwrap();
    numeric_fixture.domain = domain;
    assert!(matches!(
        numeric_fixture.qualify_exact_continuation(),
        Err(MappedClockError::NumericalUnavailable)
    ));
}
