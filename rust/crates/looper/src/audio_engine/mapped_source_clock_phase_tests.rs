//! Bounded original-seconds numerical regressions; no source/ACK or musical claim.
use super::*;
use crate::audio_engine::loop_phase_reference::{GridPhaseOrigin, LoopPhaseReference};
use crate::audio_engine::mapped_source_clock::tests::map_with_source;
use crate::audio_engine::transport::TransportTimeline;

const RATE: u32 = 48_000;

// Independent normal-binary64 division of the small integer oracle, including
// ties-to-even. Casting a large numerator/denominator separately would round twice.
fn oracle_fraction(mut numerator: u128, denominator: u128) -> f64 {
    if numerator == 0 {
        return 0.;
    }
    assert!(numerator < denominator);
    let mut exponent = 0_i32;
    while numerator < denominator {
        numerator <<= 1;
        exponent -= 1;
    }
    assert!(numerator <= u128::MAX >> 52);
    let scaled = numerator << 52;
    let mut significand = scaled / denominator;
    let remainder = scaled % denominator;
    if remainder * 2 > denominator || (remainder * 2 == denominator && significand & 1 != 0) {
        significand += 1;
    }
    if significand == 1_u128 << 53 {
        significand >>= 1;
        exponent += 1;
    }
    assert!((-1022..=0).contains(&exponent));
    f64::from_bits(((exponent + 1023) as u64) << 52 | (significand as u64 & ((1_u64 << 52) - 1)))
}

// Independent exact lift of the actual binary64 master onto this fixture's
// original map. Source folding remains Q=8; the integer beat lift is explicit.
fn grid_eight_oracle(target_n: i128, target_d: i128, master: f64, lift: i128) -> (usize, f64) {
    let bits = master.to_bits();
    let (mn, md) = if master == 0.0 {
        (0, 1)
    } else {
        let magnitude = i128::from((bits & ((1_u64 << 52) - 1)) | (1_u64 << 52));
        let mantissa = if bits >> 63 == 0 {
            magnitude
        } else {
            -magnitude
        };
        let exponent = ((bits >> 52) & 2047) as i32 - 1023 - 52;
        if exponent >= 0 {
            (mantissa << exponent, 1)
        } else {
            (mantissa, 1_i128 << -exponent)
        }
    };
    let d = target_d * md;
    let b = 2 * d + (target_n * md + mn * target_d + (lift - 2) * d).rem_euclid(8 * d);
    let n = if b <= 4 * d {
        b * 24_000
    } else if b <= 8 * d {
        96_000 * d + (b - 4 * d) * 48_000
    } else {
        288_000 * d + (b - 8 * d) * 36_000
    };
    let mut whole = (n / d) as usize;
    let mut fraction = oracle_fraction((n % d) as u128, d as u128);
    if fraction == 1.0 {
        whole += 1;
        fraction = 0.0;
    }
    (whole, fraction)
}

fn grid_origin_geometry(origin_frame: f64, authority_revision: u64) -> MappedLoopGeometry {
    let map = map_with_source(
        &[(0., 0.), (2., 4.), (6., 8.), (18., 24.), (34., 40.)],
        &["trusted"; 4],
        true,
        RATE,
        1_632_001,
    );
    MappedLoopGeometry::new_general_declared(
        map,
        1_632_001,
        RATE,
        FrameRange {
            start: 48_000,
            end: 360_000,
        },
        8.,
        false,
    )
    .unwrap()
    .with_grid_phase_origin(Some(
        crate::audio_engine::loop_phase_reference::GridPhaseOrigin {
            frame_bits: origin_frame.to_bits(),
            authority_revision,
        },
    ))
    .unwrap()
}

#[test]
fn phase_explicit_grid_retains_whole_beat_offsets_and_captures_both_fractional_directions() {
    let geometry = grid_origin_geometry(0.0, 17);
    let reference = LoopPhaseReference::Unavailable;
    let execution = 1_000;
    for master in [-2.0, 0.0, 2.0, 34.0] {
        let actual = timeline(36_000.0, execution, master);
        let epoch = MappedClockEpoch::common_phase(
            811,
            actual,
            &geometry,
            raw(48_000, 0.0),
            execution,
            reference,
        )
        .unwrap();
        assert!(
            settled(&geometry, epoch, reference, execution).unwrap(),
            "whole beat offset must not be pulled through a loop"
        );
        assert_pair(
            position(&geometry, epoch, reference, execution).unwrap(),
            48_000,
            0.0,
        );
        assert_pair(
            position(&geometry, epoch, reference, execution + 36_000).unwrap(),
            72_000,
            0.0,
        );
        assert_local(&geometry, epoch, reference, execution + 24_001);
    }
    for (master, lift) in [(0.25, 2), (0.5, 2), (0.75, 1), (34.25, -32), (-0.25, 2)] {
        let actual = timeline(36_000.0, execution, master);
        let epoch = MappedClockEpoch::common_phase(
            812,
            actual,
            &geometry,
            raw(48_000, 0.0),
            execution,
            reference,
        )
        .unwrap();
        assert_pair(
            position(&geometry, epoch, reference, execution).unwrap(),
            48_000,
            0.0,
        );
        assert!(!settled(&geometry, epoch, reference, execution).unwrap());
        let mut high = execution + 36_000;
        while !settled(&geometry, epoch, reference, high).unwrap() {
            high = execution + 2 * (high - execution);
            assert!(high - execution <= 36_000 * 4096);
        }
        let mut low = execution;
        while low + 1 < high {
            let middle = low + (high - low) / 2;
            if settled(&geometry, epoch, reference, middle).unwrap() {
                high = middle;
            } else {
                low = middle;
            }
        }
        assert!(!settled(&geometry, epoch, reference, high - 1).unwrap());
        for frame in [high, high + 1, high + 36_000, high + 192_001] {
            let (whole, fraction) =
                grid_eight_oracle(0, 1, actual.beat_position_at_frame(frame).unwrap(), lift);
            assert_pair(
                position(&geometry, epoch, reference, frame).unwrap(),
                whole,
                fraction,
            );
            assert_local(&geometry, epoch, reference, frame);
        }
        if epoch.has_full_fraction_recovery() {
            phase::assert_full_refresh_equivalent_for_test(
                &geometry,
                epoch,
                reference,
                &[execution, high - 1, high, high + 1],
                &[high - 1, high],
            );
        }
    }
}

#[test]
fn phase_requested_grid_origin_uses_third_exact_inverse_preserves_e_and_converges_both_signs() {
    println!(
        "GRID_PHASE_COPY_BYTES epoch={} span={} reference={} local={:?}",
        std::mem::size_of::<MappedClockEpoch>(),
        std::mem::size_of::<MappedSourceSpan>(),
        std::mem::size_of::<LoopPhaseReference>(),
        MappedClockEpoch::phase_local_payload_sizes_for_test()
    );
    // A=1s lies in the first segment, initial s=4s in the second,
    // g=12s+1/48000 in the third. B(g)=16+1/36000 is not binary64.
    let geometry = grid_origin_geometry(576_001., 17);
    let domain = geometry.domain();
    assert_eq!(
        domain.region,
        FrameRange {
            start: 48_000,
            end: 360_000
        }
    );
    for (marker, master, winding) in [(101, 5.25, 15), (102, 5.75, 16)] {
        let execution = 1_000;
        let actual = timeline(36_000., execution, master);
        let reference = launch_reference(&geometry, actual, 0);
        let first = raw(192_000, 0.5);
        let epoch =
            MappedClockEpoch::common_phase(marker, actual, &geometry, first, execution, reference)
                .unwrap();
        assert_pair(
            position(&geometry, epoch, reference, execution).unwrap(),
            first.frame,
            first.fraction,
        );
        assert!(!settled(&geometry, epoch, reference, execution).unwrap());
        let mut high = execution + 24_000;
        while !settled(&geometry, epoch, reference, high).unwrap() {
            high = execution + 2 * (high - execution);
            assert!(high - execution <= 24_000 * 4096);
        }
        let mut low = execution;
        while low + 1 < high {
            let middle = low + (high - low) / 2;
            if settled(&geometry, epoch, reference, middle).unwrap() {
                high = middle;
            } else {
                low = middle;
            }
        }
        assert!(!settled(&geometry, epoch, reference, high - 1).unwrap());
        // Independent integer oracle of exact B(g) plus the ACTUAL binary64
        // master M(n), not an invented rational frame/tempo transport.
        for frame in [
            high,
            high + 1,
            high + 24_000,
            high + 192_000,
            high + 384_001,
        ] {
            let master_bits = actual.beat_position_at_frame(frame).unwrap().to_bits();
            let mantissa = i128::from((master_bits & ((1_u64 << 52) - 1)) | (1_u64 << 52));
            let exponent = ((master_bits >> 52) & 2047) as i32 - 1023 - 52;
            let (master_n, master_d) = if exponent >= 0 {
                (mantissa << exponent, 1_i128)
            } else {
                (mantissa, 1_i128 << (-exponent))
            };
            let denominator = 36_000 * master_d;
            let beat_n = 2 * denominator
                + ((16 - 2 - winding) * denominator + 36_000 * master_n + master_d)
                    .rem_euclid(8 * denominator);
            let source_n = if beat_n <= 4 * denominator {
                beat_n * 24_000
            } else if beat_n <= 8 * denominator {
                96_000 * denominator + (beat_n - 4 * denominator) * 48_000
            } else {
                288_000 * denominator + (beat_n - 8 * denominator) * 36_000
            };
            let mut whole = (source_n / denominator) as usize;
            let mut fraction =
                oracle_fraction((source_n % denominator) as u128, denominator as u128);
            if fraction == 1. {
                whole += 1;
                fraction = 0.;
            }
            assert_pair(
                position(&geometry, epoch, reference, frame).unwrap(),
                whole,
                fraction,
            );
            assert_affine_shuffled(&geometry, epoch, reference, frame);
        }
        for frame in [execution, execution + 1, execution + 137, high - 1] {
            let span = assert_local(&geometry, epoch, reference, frame);
            let rate = span.local_rate().unwrap();
            assert!(
                (crate::audio_engine::constants::SPEED_MIN
                    ..=crate::audio_engine::constants::SPEED_MAX)
                    .contains(&rate)
            );
        }
        assert_eq!(geometry.domain(), domain);
    }
}

#[test]
fn phase_requested_grid_origin_rejects_changed_geometry_and_retains_its_old_local_target() {
    let original = grid_origin_geometry(576_001., 17);
    let actual = timeline(24_000., 1_000, 5.);
    let reference = launch_reference(&original, actual, 0);
    let epoch =
        MappedClockEpoch::common_phase(103, actual, &original, raw(192_000, 0.5), 1_000, reference)
            .unwrap();
    for replacement in [
        grid_origin_geometry(576_002., 17),
        grid_origin_geometry(576_001., 18),
    ] {
        let changed = launch_reference(&replacement, actual, 0);
        assert!(
            reference.matches_exact(changed),
            "launch history does not contain requested grid data"
        );
        assert!(position(&replacement, epoch, changed, 1_000).is_err());
        let span = refresh(&original, epoch, reference, 1_000).unwrap();
        let local = span.local_phase_view(original.domain(), changed).unwrap();
        assert_pair(local.position_at(1_000).unwrap(), 192_000, 0.5);
    }
    // Change ONLY the later g-side inverse. Physical A/Z/Q and raw's segment stay.
    let changed_map = map_with_source(
        &[
            (0., 0.),
            (2., 4.),
            (6., 8.),
            (7.5, 10.),
            (18., 26.),
            (34., 42.),
        ],
        &["trusted"; 5],
        true,
        RATE,
        1_632_001,
    );
    let changed = MappedLoopGeometry::new_general_declared(
        changed_map,
        1_632_001,
        RATE,
        original.domain().region,
        8.,
        false,
    )
    .unwrap()
    .with_grid_phase_origin(Some(
        crate::audio_engine::loop_phase_reference::GridPhaseOrigin {
            frame_bits: 576_001_f64.to_bits(),
            authority_revision: 17,
        },
    ))
    .unwrap();
    let changed_reference = launch_reference(&changed, actual, 0);
    assert!(reference.matches_exact(changed_reference));
    assert!(position(&changed, epoch, changed_reference, 1_000).is_err());
    let old_span = refresh(&original, epoch, reference, 1_000).unwrap();
    let retained = old_span
        .local_phase_view(original.domain(), changed_reference)
        .unwrap();
    assert_pair(
        retained.position_at(1_001).unwrap(),
        position(&original, epoch, reference, 1_001).unwrap().frame,
        position(&original, epoch, reference, 1_001)
            .unwrap()
            .fraction,
    );
    assert!(
        MappedClockEpoch::common_phase(
            105,
            actual,
            &original,
            raw(192_000, 0.5),
            1_000,
            LoopPhaseReference::Unavailable
        )
        .is_ok(),
        "absolute coordinates do not manufacture or require an intentional launch"
    );
}

#[test]
fn phase_requested_grid_origin_refuses_gap_seams_and_outside_without_old_target_fallback() {
    let map = map_with_source(
        &[(0., 0.), (2., 4.), (4., 8.), (6., 12.)],
        &["trusted", "gap_unknown_count", "trusted"],
        false,
        RATE,
        288_001,
    );
    let mut base = MappedLoopGeometry::new_general_declared(
        map,
        288_001,
        RATE,
        FrameRange {
            start: 0,
            end: 48_000,
        },
        2.,
        false,
    )
    .unwrap();
    base.prepare_common_phase();
    assert!(base.has_common_phase());
    for frame in [
        96_000.,
        144_000.,
        192_000.,
        192_001.,
        1_000_000.,
        f64::INFINITY,
    ] {
        let result = base.clone().with_grid_phase_origin(Some(
            crate::audio_engine::loop_phase_reference::GridPhaseOrigin {
                frame_bits: frame.to_bits(),
                authority_revision: 1,
            },
        ));
        assert!(
            result.is_err(),
            "unsupported explicit grid target {frame} cannot fall back to A"
        );
    }
    assert!(
        base.clone()
            .with_grid_phase_origin(Some(
                crate::audio_engine::loop_phase_reference::GridPhaseOrigin {
                    frame_bits: 95_999_f64.to_bits(),
                    authority_revision: 1
                }
            ))
            .is_ok()
    );
    assert!(
        base.with_grid_phase_origin(None)
            .unwrap()
            .has_common_phase()
    );
    // Reverse direction: A after the gap and g before it is equally unavailable.
    let map = map_with_source(
        &[(0., 0.), (2., 4.), (4., 8.), (6., 12.)],
        &["trusted", "gap_unknown_count", "trusted"],
        false,
        RATE,
        288_001,
    );
    let later = MappedLoopGeometry::new_general_declared(
        map,
        288_001,
        RATE,
        FrameRange {
            start: 216_000,
            end: 264_000,
        },
        2.,
        false,
    )
    .unwrap();
    let target = |frame: f64| {
        Some(crate::audio_engine::loop_phase_reference::GridPhaseOrigin {
            frame_bits: frame.to_bits(),
            authority_revision: 1,
        })
    };
    assert!(
        later
            .clone()
            .with_grid_phase_origin(target(48_000.))
            .is_err()
    );
    assert!(
        later.with_grid_phase_origin(target(216_000.)).is_ok(),
        "A==g is one supported point, not an empty interval failure"
    );
}

#[test]
fn phase_virtual_grid_origin_extends_only_original_edges_with_exact_both_sign_oracle() {
    let map = map_with_source(
        &[(0., 0.), (2., 4.), (6., 8.), (18., 24.), (34., 40.)],
        &["trusted"; 4],
        false,
        RATE,
        1_632_000,
    );
    let record = map.canonical_record().to_owned();
    let base = MappedLoopGeometry::new_general_declared(
        map,
        1_632_000,
        RATE,
        FrameRange {
            start: 48_000,
            end: 360_000,
        },
        8.,
        false,
    )
    .unwrap();
    for (g, target_n, target_d) in [
        (-1_f64, -1_i128, 24_000_i128),
        (1_632_001., 1_920_001, 48_000),
    ] {
        let geometry = base
            .clone()
            .with_grid_phase_origin(Some(GridPhaseOrigin {
                frame_bits: g.to_bits(),
                authority_revision: 71,
            }))
            .unwrap();
        assert_eq!(geometry.domain(), base.domain());
        assert_eq!(geometry.map.canonical_record(), record);
        assert!(
            geometry
                .map
                .canonical_beat_at_source(g / f64::from(RATE))
                .is_none()
        );
        for master in [5., 7.] {
            let execution = 1_000;
            let actual = timeline(36_000., execution, master);
            let reference = LoopPhaseReference::Unavailable;
            let first = raw(192_000, 0.5);
            let epoch =
                MappedClockEpoch::common_phase(111, actual, &geometry, first, execution, reference)
                    .unwrap();
            assert_pair(
                position(&geometry, epoch, reference, execution).unwrap(),
                first.frame,
                first.fraction,
            );
            let mut high = execution + 24_000;
            while !settled(&geometry, epoch, reference, high).unwrap() {
                high = execution + 2 * (high - execution);
                assert!(high - execution <= 24_000 * 4096);
            }
            for frame in [
                execution + 1,
                high - 1,
                high,
                high + 1,
                high + 192_000,
                high + 384_001,
            ] {
                assert_local(&geometry, epoch, reference, frame);
                if frame < high {
                    continue;
                }
                // Independent exact original-map affine target and actual b64 M.
                let bits = actual.beat_position_at_frame(frame).unwrap().to_bits();
                let mantissa = i128::from((bits & ((1_u64 << 52) - 1)) | (1_u64 << 52));
                let exponent = ((bits >> 52) & 2047) as i32 - 1023 - 52;
                let (mn, md) = if exponent >= 0 {
                    (mantissa << exponent, 1_i128)
                } else {
                    (mantissa, 1_i128 << (-exponent))
                };
                let d = target_d * md;
                let winding = if target_n < 0 {
                    master as i128 - 6
                } else {
                    master as i128 + 34
                };
                let b =
                    2 * d + (target_n * md + mn * target_d - (2 + winding) * d).rem_euclid(8 * d);
                let n = if b <= 4 * d {
                    b * 24_000
                } else if b <= 8 * d {
                    96_000 * d + (b - 4 * d) * 48_000
                } else {
                    288_000 * d + (b - 8 * d) * 36_000
                };
                let mut whole = (n / d) as usize;
                let mut fraction = oracle_fraction((n % d) as u128, d as u128);
                if fraction == 1. {
                    whole += 1;
                    fraction = 0.;
                }
                assert_pair(
                    position(&geometry, epoch, reference, frame).unwrap(),
                    whole,
                    fraction,
                );
            }
        }
    }
    // Signed virtual g never changes any copied epoch/program payload size.
    assert_eq!(std::mem::size_of::<MappedClockEpoch>(), 176);
    assert_eq!(std::mem::size_of::<MappedSourceSpan>(), 288);
}

#[test]
fn phase_virtual_grid_origin_refuses_missing_unusable_edges_and_physical_bridge_holes() {
    let origin = |g: f64| {
        Some(GridPhaseOrigin {
            frame_bits: g.to_bits(),
            authority_revision: 72,
        })
    };
    let prepare = |anchors: &[(f64, f64)], coverage: &[&str], start: usize, end: usize| {
        MappedLoopGeometry::new_general_declared(
            map_with_source(anchors, coverage, false, RATE, 288_000),
            288_000,
            RATE,
            FrameRange { start, end },
            2.,
            false,
        )
        .unwrap()
    };
    // The first/last usable island cannot replace a missing physical edge.
    let missing_head = prepare(
        &[(1., 2.), (2., 4.), (4., 8.), (6., 12.)],
        &["trusted"; 3],
        48_000,
        96_000,
    );
    assert!(missing_head.with_grid_phase_origin(origin(-1.)).is_err());
    let missing_tail = prepare(
        &[(0., 0.), (2., 4.), (4., 8.), (5., 10.)],
        &["trusted"; 3],
        0,
        48_000,
    );
    assert!(
        missing_tail
            .with_grid_phase_origin(origin(288_001.))
            .is_err()
    );
    for unusable in ["gap_unknown_count", "uncertain"] {
        let head = prepare(
            &[(0., 0.), (2., 4.), (4., 8.), (6., 12.)],
            &[unusable, "trusted", "trusted"],
            120_000,
            168_000,
        );
        assert!(head.with_grid_phase_origin(origin(-1.)).is_err());
        let tail = prepare(
            &[(0., 0.), (2., 4.), (4., 8.), (6., 12.)],
            &["trusted", "trusted", unusable],
            0,
            48_000,
        );
        assert!(tail.with_grid_phase_origin(origin(288_001.)).is_err());
        // Original edge usable, but the physical edge-to-A path crosses a hole.
        let before = prepare(
            &[(0., 0.), (2., 4.), (4., 8.), (6., 12.)],
            &["trusted", unusable, "trusted"],
            216_000,
            264_000,
        );
        assert!(before.with_grid_phase_origin(origin(-1.)).is_err());
        let after = prepare(
            &[(0., 0.), (2., 4.), (4., 8.), (6., 12.)],
            &["trusted", unusable, "trusted"],
            0,
            48_000,
        );
        assert!(after.with_grid_phase_origin(origin(288_001.)).is_err());
    }
}

#[test]
fn phase_virtual_grid_origin_uses_exact_decoder_extent_not_rounded_loaded_eof() {
    // D=88201/44100, loaded D=96001+13/147 frames. Frame96001 is INSIDE,
    // while96002 is outside. Both use the exact original physical-edge segment.
    let duration = 88_201_f64 / 44_100.;
    let map = map_with_source(
        &[(0., 0.), (1., 2.), (duration, 4.)],
        &["trusted"; 2],
        false,
        44_100,
        88_201,
    );
    let base = MappedLoopGeometry::new_general_declared(
        map,
        96_001,
        RATE,
        FrameRange {
            start: 0,
            end: 48_000,
        },
        2.,
        false,
    )
    .unwrap();
    for g in [
        -0.0_f64,
        0.,
        96_001.,
        96_002.,
        -(((1_u64 << 53) - 1) as f64),
        ((1_u64 << 53) - 1) as f64,
    ] {
        let geometry = base
            .clone()
            .with_grid_phase_origin(Some(GridPhaseOrigin {
                frame_bits: g.to_bits(),
                authority_revision: 73,
            }))
            .unwrap();
        assert_eq!(geometry.domain(), base.domain());
        assert_eq!(geometry.phase_origin.unwrap().frame_bits, g.to_bits());
        let actual = timeline(24_000., 0, 0.);
        let e = MappedClockEpoch::common_phase(
            112,
            actual,
            &geometry,
            raw(13, -0.),
            0,
            LoopPhaseReference::Unavailable,
        )
        .unwrap();
        assert_pair(
            position(&geometry, e, LoopPhaseReference::Unavailable, 0).unwrap(),
            13,
            -0.,
        );
    }
    for g in [
        -((1_u64 << 53) as f64),
        (1_u64 << 53) as f64,
        f64::NEG_INFINITY,
        0.5,
    ] {
        assert!(
            base.clone()
                .with_grid_phase_origin(Some(GridPhaseOrigin {
                    frame_bits: g.to_bits(),
                    authority_revision: 73
                }))
                .is_err()
        );
    }
}

#[test]
fn phase_requested_grid_origin_uses_loaded_frames_while_preserving_decoder_rate() {
    let map = map_with_source(&[(0., 0.), (2., 4.)], &["trusted"], false, 44_100, 88_201);
    let revision = map.map_revision().to_owned();
    let geometry = MappedLoopGeometry::new_general_declared(
        map,
        96_001,
        RATE,
        FrameRange {
            start: 0,
            end: 48_000,
        },
        2.,
        false,
    )
    .unwrap()
    .with_grid_phase_origin(Some(
        crate::audio_engine::loop_phase_reference::GridPhaseOrigin {
            frame_bits: 1_f64.to_bits(),
            authority_revision: 3,
        },
    ))
    .unwrap();
    assert_eq!(geometry.map.map_revision(), revision);
    let actual = timeline(24_000., 0, 0.);
    let reference = launch_reference(&geometry, actual, 0);
    let epoch =
        MappedClockEpoch::common_phase(104, actual, &geometry, raw(1, -0.), 0, reference).unwrap();
    assert!(settled(&geometry, epoch, reference, 0).unwrap());
    assert_pair(position(&geometry, epoch, reference, 0).unwrap(), 1, -0.);
    assert_pair(
        position(&geometry, epoch, reference, 24_000).unwrap(),
        24_001,
        -0.,
    );
    assert_affine_shuffled(&geometry, epoch, reference, 47_999);
}

fn geometry(anchors: &[(f64, f64)], quarters: f64, frames: usize) -> MappedLoopGeometry {
    let map = map_with_source(
        anchors,
        &vec!["trusted"; anchors.len() - 1],
        true,
        RATE,
        frames + 1,
    );
    let mut result = MappedLoopGeometry::new(
        map,
        frames + 1,
        RATE,
        FrameRange {
            start: 0,
            end: frames,
        },
        quarters,
        false,
    )
    .unwrap()
    .qualify_exact_continuation()
    .unwrap();
    result.prepare_common_phase();
    assert!(result.has_common_phase());
    result
}

fn timeline(fpb: f64, frame: u64, beat: f64) -> TransportTimeline {
    let mut result = TransportTimeline::new(RATE);
    assert!(result.set_master_period_and_anchor_beat_position_at_frame(
        fpb / f64::from(RATE),
        beat,
        frame
    ));
    result
}

fn raw(frame: usize, fraction: f64) -> FractionalSourcePosition {
    FractionalSourcePosition {
        frame,
        fraction,
        seek_mode: ExplicitSeekMode::Normal,
    }
}

fn launch_reference(
    geometry: &MappedLoopGeometry,
    actual: TransportTimeline,
    launch_frame: u64,
) -> LoopPhaseReference {
    // Numerical fixture only: the test declares its original launch T explicitly.
    // Source ownership and actual scheduler custody require separate tests.
    geometry
        .qualify_phase_reference(
            LoopPhaseReference::capture(Some(actual), launch_frame),
            &actual,
        )
        .unwrap()
}

fn position(
    geometry: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    reference: LoopPhaseReference,
    frame: u64,
) -> Result<FractionalSourcePosition, MappedClockError> {
    geometry.position_at_with_reference(epoch, frame, reference)
}

fn refresh(
    geometry: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    reference: LoopPhaseReference,
    frame: u64,
) -> Result<MappedSourceSpan, MappedClockError> {
    geometry.refresh_span_with_reference(epoch, frame, reference)
}

fn settled(
    geometry: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    reference: LoopPhaseReference,
    frame: u64,
) -> Result<bool, MappedClockError> {
    epoch.common_phase_settled(geometry, frame, reference)
}

fn assert_pair(actual: FractionalSourcePosition, whole: usize, fraction: f64) {
    assert_eq!(
        (actual.frame, actual.fraction.to_bits(), actual.seek_mode),
        (whole, fraction.to_bits(), ExplicitSeekMode::Normal)
    );
}

fn assert_local(
    geometry: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    reference: LoopPhaseReference,
    frame: u64,
) -> MappedSourceSpan {
    let span = refresh(geometry, epoch, reference, frame).unwrap();
    assert!(span.contains(frame));
    assert!(
        span.end_frame() > frame,
        "every half-open region must advance"
    );
    let local = span.local_phase_view(geometry.domain(), reference).unwrap();
    for point in [
        frame,
        span.start_frame(),
        span.end_frame() - 1,
        span.end_frame(),
    ] {
        if point >= span.start_frame() && point <= span.end_frame() {
            let expected = position(geometry, epoch, reference, point).unwrap();
            let actual = local.position_at(point).unwrap();
            assert_pair(actual, expected.frame, expected.fraction);
        }
    }
    if span.start_frame() > 0 {
        assert!(local.position_at(span.start_frame() - 1).is_none());
    }
    if span.end_frame() < u64::MAX {
        assert!(local.position_at(span.end_frame() + 1).is_none());
    }
    span
}

#[test]
fn phase_decimal_original_seconds_use_closed_full_fraction_recovery() {
    // Declared Q2 avoids confusing this with physical-marker nonintegral Q:
    // exact binary64(0.91) * 48000 is slightly greater than integer43680.
    let map = map_with_source(
        &[(0., 0.), (0.37, 1.), (0.91, 2.)],
        &["trusted"; 2],
        true,
        RATE,
        48000,
    );
    let mut geometry = MappedLoopGeometry::new_general_declared(
        map,
        48000,
        RATE,
        FrameRange {
            start: 0,
            end: 43680,
        },
        2.,
        true,
    )
    .unwrap();
    geometry.prepare_common_phase();
    assert!(geometry.has_common_phase());
    let actual = timeline(24000., 0, 0.);
    let reference = launch_reference(&geometry, actual, 0);
    let first = raw(13, 0.);
    let ordinary = MappedClockEpoch::general_compact(
        88,
        actual,
        &geometry,
        SourceLoopDomain::physical(48000, geometry.domain().region),
        first,
        0,
    )
    .unwrap();
    assert_pair(geometry.position_at(ordinary, 0).unwrap(), 13, 0.);
    let later = geometry.position_at(ordinary, 1).unwrap();
    assert_eq!(later.frame, 13);
    assert!(later.fraction > 0. && later.fraction < 1.);
    // Independent Fraction oracle retained in decimal_phase_fraction_oracle-v1.json.
    let phase = MappedClockEpoch::common_phase(89, actual, &geometry, first, 0, reference).unwrap();
    assert!(phase.has_full_fraction_recovery());
    for (frame, whole, bits) in [
        (0, 13, 0),
        (1, 13, 0x3fe74f5c28f5c28f),
        (1124, 831, 0x3fe870a3d70a3ba6),
        (1125, 832, 0x3fdfffffffffff4c),
        (23999, 17759, 0x3fd0a3d70a3d2200),
        (24000, 17759, 0x3feffffffffff880),
        (24001, 17761, 0x3fb47ae147af4c05),
        (47999, 43678, 0x3fed70a3d70a437f),
        (48000, 0, 0),
        (48001, 0, 0x3fe7ae147ae16780),
    ] {
        assert_pair(
            position(&geometry, phase, reference, frame).unwrap(),
            whole,
            f64::from_bits(bits),
        );
        let span = assert_local(&geometry, phase, reference, frame);
        assert_eq!(
            span.local_phase_view(geometry.domain(), reference)
                .unwrap()
                .full_fraction_split_bits_for_test(),
            Some(1024)
        );
    }
    assert!(!settled(&geometry, phase, reference, 1124).unwrap());
    assert!(settled(&geometry, phase, reference, 1125).unwrap());
    let unchanged = geometry.position_at(ordinary, 1).unwrap();
    assert_pair(unchanged, later.frame, later.fraction);
    // Negative GLOBAL debt, captured neighbours and the original knot/wrap.
    phase::assert_full_refresh_equivalent_for_test(
        &geometry,
        phase,
        reference,
        &[0, 1, 1124, 1125, 1126, 23999, 24000, 48000, u64::MAX],
        &[1124, 1125, 24000, u64::MAX],
    );
}

#[test]
fn full_fraction_zero_debt_refresh_keeps_the_captured_shortcut() {
    let map = map_with_source(
        &[(0., 0.), (0.37, 1.), (0.91, 2.), (1.47, 3.)],
        &["trusted"; 3],
        true,
        RATE,
        70561,
    );
    let mut g = MappedLoopGeometry::new_general_declared(
        map,
        70561,
        RATE,
        FrameRange {
            start: 1000,
            end: 45194,
        },
        2.,
        true,
    )
    .unwrap();
    g.prepare_common_phase();
    let actual = timeline(24000., 0, 0.);
    let reference = launch_reference(&g, actual, 0);
    // Nonclosed B(A) selects FullFraction even though the intentional launch
    // itself has exactly zero debt. No numeric capability is forced by the test.
    let epoch =
        MappedClockEpoch::common_phase(190, actual, &g, raw(1000, -0.), 0, reference).unwrap();
    assert!(epoch.has_full_fraction_recovery());
    assert!(settled(&g, epoch, reference, 0).unwrap());
    assert_pair(position(&g, epoch, reference, 0).unwrap(), 1000, -0.);
    phase::assert_full_refresh_equivalent_for_test(
        &g,
        epoch,
        reference,
        &[0, 1, 23999, 24000, 24001, 47999, 48000, 48001, u64::MAX],
        &[0, 24000, 48000, u64::MAX],
    );
}

#[test]
fn full_fraction_distinct_original_inverses_keep_nonzero_start_knot_capture_and_reader() {
    let map = map_with_source(
        &[(0., 0.), (0.37, 1.), (0.91, 2.), (1.47, 3.)],
        &["trusted"; 3],
        true,
        RATE,
        70561,
    );
    let mut g = MappedLoopGeometry::new_general_declared(
        map,
        70561,
        RATE,
        FrameRange {
            start: 1000,
            end: 45194,
        },
        2.,
        true,
    )
    .unwrap();
    g.prepare_common_phase();
    let actual = timeline(24000., 0, 0.);
    let reference = launch_reference(&g, actual, 0);
    let epoch =
        MappedClockEpoch::common_phase(191, actual, &g, raw(20000, 0.), 27000, reference).unwrap();
    assert!(epoch.has_full_fraction_recovery());
    let raw_inverse = g.map.canonical_segment_at_beat(1.).unwrap();
    assert!(!raw_inverse.matches_exact(reference.mapped_parts().unwrap().1));
    // Independent Fraction records from decimal-distinct-inverse-oracle-v2.json.
    for (frame, whole, bits) in [
        (27000, 20000, 0),
        (27001, 20001, 0x3fb8ccccccce4625),
        (48588, 43679, 0x3fd59999999b6408),
        (48589, 43680, 0x3fdcd4629b7df17f),
        (172745, 24344, 0x3fa7ffffffe951e4),
        (172746, 24345, 0x3fc1d9cebcc0f669),
        (172747, 24346, 0x3fcc173f6092666c),
    ] {
        let point = position(&g, epoch, reference, frame).unwrap();
        assert_pair(point, whole, f64::from_bits(bits));
        assert_affine_shuffled(&g, epoch, reference, frame);
        let period = g
            .domain()
            .musical_period
            .mapped_period(g.domain().region)
            .unwrap();
        let taps = period
            .seam_taps(point.frame, point.fraction, g.domain().region)
            .unwrap();
        assert_eq!(
            (taps.left, taps.right, taps.right_gain.to_bits()),
            (
                whole,
                (bits != 0).then_some(whole + 1),
                (f64::from_bits(bits) as f32).to_bits()
            )
        );
    }
    assert!(!settled(&g, epoch, reference, 172745).unwrap());
    assert!(settled(&g, epoch, reference, 172746).unwrap());
    // Positive GLOBAL debt with different original raw/start inverses. The
    // pre-epoch query must remain unavailable in both evaluators.
    phase::assert_full_refresh_equivalent_for_test(
        &g,
        epoch,
        reference,
        &[26999, 27000, 48588, 48589, 172745, 172746, 172747, u64::MAX],
        &[26999, 172745, 172746],
    );
}

#[test]
fn compact_preferred_geometry_has_closed_subnormal_phase_and_ordinary_recovery() {
    let g = geometry(
        &[(0., 0.), (1. / 64., 8.), (5. / 64., 24.), (9. / 64., 32.)],
        32.,
        6750,
    );
    assert!(g.general.is_none());
    assert!(g.has_compact_continuation());
    assert!(g.full_fraction.is_some());
    let actual = timeline(187.5, 0, 0.);
    let reference = launch_reference(&g, actual, 0);
    let mut point = raw(0, f64::from_bits(1));
    let mut execution = 0;
    for pass in 0..4 {
        let epoch =
            MappedClockEpoch::common_phase(200 + pass, actual, &g, point, execution, reference)
                .unwrap();
        if pass == 0 {
            assert!(epoch.has_full_fraction_recovery());
        }
        assert_pair(
            position(&g, epoch, reference, execution).unwrap(),
            point.frame,
            point.fraction,
        );
        assert_affine_shuffled(&g, epoch, reference, execution);
        // Freeze the exact emitted RN pair, then recover against live R. This
        // numerical fixture declares a pause; it grants no live voice authority.
        point = position(&g, epoch, reference, execution + 1).unwrap();
        execution += 27;
        let ordinary = MappedClockEpoch::full_fraction_ordinary_recovery(
            300 + pass,
            actual,
            &g,
            point,
            execution,
        )
        .unwrap();
        assert!(ordinary.is_full_fraction_general());
        assert_pair(
            g.position_at(ordinary, execution).unwrap(),
            point.frame,
            point.fraction,
        );
        let span = g.refresh_span(ordinary, execution).unwrap();
        let local = span.local_general_view(g.domain()).unwrap();
        for n in [execution, span.end_frame() - 1, span.end_frame()] {
            let expected = g.position_at(ordinary, n).unwrap();
            assert_pair(
                local.position_at(n).unwrap(),
                expected.frame,
                expected.fraction,
            );
        }
        point = g.position_at(ordinary, execution + 1).unwrap();
        execution += 1;
    }
    // The full fractional domain is also accepted as the original ordinary raw
    // pair; it never has to fit the original source grid or an i128 coefficient.
    let fine = raw(13, f64::from_bits(1));
    let epoch =
        MappedClockEpoch::full_fraction_ordinary_recovery(400, actual, &g, fine, 0).unwrap();
    assert_pair(g.position_at(epoch, 0).unwrap(), 13, f64::from_bits(1));
    assert!(MappedClockEpoch::ordinary_mapped_recovery(401, actual, &g, fine, 0).is_err());
}

#[test]
fn compact_full_fraction_window_corrects_twice_then_captures_without_rounded_reanchor() {
    let g = geometry(
        &[(0., 0.), (1. / 64., 8.), (5. / 64., 24.), (9. / 64., 32.)],
        32.,
        6750,
    );
    let base = g;
    for origin in [
        None,
        Some(GridPhaseOrigin {
            frame_bits: 0.0_f64.to_bits(),
            authority_revision: 71,
        }),
    ] {
        let g = base.clone().with_grid_phase_origin(origin).unwrap();
        assert!(g.general.is_none());
        let actual = timeline(187.5, 0, 0.);
        let reference = launch_reference(&g, actual, 0);
        let epoch = MappedClockEpoch::common_phase(
            450,
            actual,
            &g,
            raw(13, f64::from_bits(1)),
            0,
            reference,
        )
        .unwrap();
        assert!(epoch.has_full_fraction_recovery());
        // Independent stdlib Fraction oracle: compact-window-fraction-oracle-v1.json.
        // beta0=(13+2^-1074)*4/375, seed24, h64, delta1/8, cycle257/8.
        // The first window removes only1/8; the remaining debt needs a second window.
        for (frame, whole, bits, region) in [
            (0, 13, 1, 0),
            (1, 13, 0x3fe0000000000000, 0),
            (1499, 774, 0x3feffffffffffe04, 0),
            (1500, 776, 2, 0),
            (4473, 3749, 0x3d55c00000000000, 0),
            (4474, 3750, 0x3d5b900000000000, 1),
            (4475, 3751, 0x3fef000000000627, 1),
            (5973, 6701, 0x3fc40000000055a4, 1),
            (5974, 6703, 0x3fc0000000003800, 2),
            (5975, 6705, 0x3fc0000000001900, 2),
            (5999, 0, 0x3fe90000000001f0, 2),
            (6000, 1, 0x3fd2000000000000, 2),
            (10497, 3749, 0x3fe2000000000ba0, 2),
            (10498, 3751, 0x3fbb800000007a10, 1),
            (10499, 3753, 0x3fb3800000003d08, 1),
            (10661, 4072, 0x3f8bfffffff8dd00, 1),
            (10662, 4073, 0x3fefffffffffdb40, 3),
            (10663, 4075, 0x3fefffffffffd380, 3),
        ] {
            assert_pair(
                position(&g, epoch, reference, frame).unwrap(),
                whole,
                f64::from_bits(bits),
            );
            let span = assert_affine_shuffled(&g, epoch, reference, frame);
            let (_, _, seed, actual_region) = span.program.phase_value().unwrap();
            assert_eq!(
                span.local_phase_view(g.domain(), reference)
                    .unwrap()
                    .full_fraction_split_bits_for_test(),
                Some(if region == 3 { 1024 } else { 2048 })
            );
            assert_eq!((seed.to_bits(), actual_region), (24_f64.to_bits(), region));
            assert_eq!(settled(&g, epoch, reference, frame).unwrap(), region == 3);
            if region == 1 {
                assert_eq!(
                    span.local_rate().unwrap().to_bits(),
                    (63_f64 / 32.).to_bits()
                );
            }
        }
        let first_window = refresh(&g, epoch, reference, 4474).unwrap();
        assert_eq!(
            (first_window.start_frame(), first_window.end_frame()),
            (4474, 5974)
        );
        let second_window = refresh(&g, epoch, reference, 10498).unwrap();
        assert_eq!(
            (second_window.start_frame(), second_window.end_frame()),
            (10498, 10662)
        );
        // Integer master checkpoints independently simplify the exact piecewise
        // law to these fractions; the tiny initial source remains in the algebra.
        for (frame, whole, fraction) in [
            (4500, 3801, 3. / 16.),
            (6000, 1, 9. / 32.),
            (10500, 3755, 23. / 512.),
            (10875, 4500, 0.),
        ] {
            assert_pair(
                position(&g, epoch, reference, frame).unwrap(),
                whole,
                fraction,
            );
        }
        phase::assert_full_refresh_equivalent_for_test(
            &g,
            epoch,
            reference,
            &[
                0,
                4473,
                4474,
                5973,
                5974,
                5999,
                6000,
                10497,
                10498,
                10661,
                10662,
                10663,
                u64::MAX,
            ],
            &[4473, 4474, 5974, 10498, 10662],
        );
    }
}

#[test]
fn grid_class_retains_512_law_and_dyadic_affine_across_both_corrections() {
    let g = geometry(&[(0., 0.), (2., 8.), (10., 24.), (18., 32.)], 32., 864000);
    let actual = timeline(24000., 0, 0.);
    let reference = launch_reference(&g, actual, 0);
    for (execution, point) in [(0, raw(13, 0.)), (24000, raw(13, 0.))] {
        let epoch =
            MappedClockEpoch::common_phase(490, actual, &g, point, execution, reference).unwrap();
        assert!(!epoch.has_full_fraction_recovery());
        for frame in [
            execution,
            execution + 1,
            execution + 24000,
            execution + 600000,
            u64::MAX,
        ] {
            assert!(epoch.phase_grid_equivalent_for_test(&g, reference, frame));
        }
        assert_affine_shuffled(&g, epoch, reference, execution + 1);
    }
}

#[test]
fn phase_real_48k_seconds_axes_capture_uses_actual_binary64_time_for_own_q32_and_q256() {
    for (anchors, quarters, period, capture, before, after) in [
        (
            &[(0., 0.), (2., 8.), (10., 24.), (18., 32.)][..],
            32.,
            864_000,
            577_639,
            (483_275, 1. - 3213. / 2_f64.powi(46)),
            (483_278, 11. / 2_f64.powi(39)),
        ),
        (
            &[(0., 0.), (16., 64.), (80., 192.), (144., 256.)][..],
            256.,
            6_912_000,
            4_609_638,
            (3_843_274, 1. / 32. - 1953. / 2_f64.powi(44)),
            (3_843_276, 81. / 2_f64.powi(38)),
        ),
    ] {
        let geometry = geometry(anchors, quarters, period);
        let actual = timeline(24_000., 0, 0.);
        let reference = launch_reference(&geometry, actual, 0);
        let epoch =
            MappedClockEpoch::common_phase(71, actual, &geometry, raw(13, 0.), 0, reference)
                .unwrap();
        assert!(epoch.is_common_phase());
        assert_pair(position(&geometry, epoch, reference, 0).unwrap(), 13, 0.);
        assert_pair(
            position(&geometry, epoch, reference, capture - 1).unwrap(),
            before.0,
            before.1,
        );
        assert_pair(
            position(&geometry, epoch, reference, capture).unwrap(),
            after.0,
            after.1,
        );
        assert!(!settled(&geometry, epoch, reference, capture - 1).unwrap());
        assert!(settled(&geometry, epoch, reference, capture).unwrap());
        for frame in [
            0,
            1,
            capture - 1,
            capture,
            capture + 1,
            8 * 24_000,
            32 * 24_000,
            256 * 24_000,
        ] {
            assert_local(&geometry, epoch, reference, frame);
        }
    }
}

#[test]
fn phase_source_knots_window_entry_exit_capture_and_wraps_are_right_sided_and_progress() {
    let geometry = geometry(&[(0., 0.), (2., 8.), (10., 24.), (18., 32.)], 32., 864_000);
    let actual = timeline(24_000., 0, 0.);
    let reference = launch_reference(&geometry, actual, 0);
    let epoch =
        MappedClockEpoch::common_phase(72, actual, &geometry, raw(13, 0.), 0, reference).unwrap();
    let mut cursor = 0;
    let mut regions = 0;
    while cursor < 2_000_000 {
        let span = assert_local(&geometry, epoch, reference, cursor);
        let next = span.end_frame();
        assert!(next > cursor);
        assert_pair(
            position(&geometry, epoch, reference, next).unwrap(),
            span.local_phase_view(geometry.domain(), reference)
                .unwrap()
                .position_at(next)
                .unwrap()
                .frame,
            span.local_phase_view(geometry.domain(), reference)
                .unwrap()
                .position_at(next)
                .unwrap()
                .fraction,
        );
        cursor = next;
        regions += 1;
        assert!(regions < 128, "a fixed edge cannot be revisited");
    }
    assert!(
        regions >= 6,
        "the fixture must cross actual source/control regions"
    );
}

#[test]
fn phase_zero_debt_keeps_raw_negative_zero_and_both_saturated_nominal_slopes() {
    let geometry = geometry(&[(0., 0.), (2., 8.), (10., 24.), (18., 32.)], 32., 864_000);
    let actual = timeline(24_000., 0, 0.);
    let reference = launch_reference(&geometry, actual, 0);
    let epoch =
        MappedClockEpoch::common_phase(73, actual, &geometry, raw(0, -0.), 0, reference).unwrap();
    assert!(settled(&geometry, epoch, reference, 0).unwrap());
    for (frame, source) in [
        (0, 0),
        (24_000, 12_000),
        (192_000, 96_000),
        (576_000, 480_000),
        (768_000, 0),
    ] {
        assert_pair(
            position(&geometry, epoch, reference, frame).unwrap(),
            source,
            -0.,
        );
        assert_local(&geometry, epoch, reference, frame);
    }
}

#[test]
fn phase_large_cast_plateaus_do_not_create_zero_length_spans_or_fake_motion() {
    let geometry = geometry(&[(0., 0.), (2., 8.), (10., 24.), (18., 32.)], 32., 864_000);
    let e = 1_u64 << 53;
    let actual = timeline(24_000., e + 1, 0.);
    let reference = launch_reference(&geometry, actual, e);
    let epoch =
        MappedClockEpoch::common_phase(74, actual, &geometry, raw(13, 0.), e, reference).unwrap();
    let first = position(&geometry, epoch, reference, e).unwrap();
    assert_pair(
        position(&geometry, epoch, reference, e + 1).unwrap(),
        first.frame,
        first.fraction,
    );
    for offset in [0, 1, 2, 3, 63, 64, 577_638, 577_639, 768_000] {
        assert_local(&geometry, epoch, reference, e + offset);
    }
}

#[test]
fn phase_invalid_marker_raw_pair_pre_epoch_and_terminal_span_refuse_without_mutation() {
    let geometry = geometry(&[(0., 0.), (2., 8.), (10., 24.), (18., 32.)], 32., 864_000);
    let actual = timeline(24_000., 100, 0.);
    let reference = launch_reference(&geometry, actual, 100);
    for pair in [
        raw(864_000, 0.),
        raw(13, 1.),
        raw(13, f64::NAN),
        raw(13, -0.25),
    ] {
        assert!(
            MappedClockEpoch::common_phase(75, actual, &geometry, pair, 100, reference).is_err()
        );
    }
    assert!(
        MappedClockEpoch::common_phase(0, actual, &geometry, raw(13, 0.), 100, reference).is_err()
    );
    let epoch =
        MappedClockEpoch::common_phase(75, actual, &geometry, raw(13, 0.), 100, reference).unwrap();
    assert!(position(&geometry, epoch, reference, 99).is_err());
    assert!(refresh(&geometry, epoch, reference, u64::MAX).is_err());
    assert_pair(position(&geometry, epoch, reference, 100).unwrap(), 13, 0.);
}

fn relabel(map: SourceBeatmap, labels: &[Option<i64>], unknown: Option<usize>) -> SourceBeatmap {
    let mut value: serde_json::Value = serde_json::from_str(map.canonical_record()).unwrap();
    for (index, label) in labels.iter().enumerate() {
        value["record"]["accepted"]["intervals"][index]["meter"]["downbeat_label"] =
            serde_json::json!(label);
        if unknown == Some(index) {
            value["record"]["accepted"]["intervals"][index]["meter"]["status"] =
                serde_json::json!("unknown");
            value["record"]["accepted"]["intervals"][index]["meter"]["downbeat_label"] =
                serde_json::Value::Null;
        }
    }
    crate::audio_engine::source_beatmap::editor::seal(value).unwrap()
}

#[test]
fn phase_original_reference_normalizes_equivalent_labels_and_unknown_or_inconsistent_meter() {
    let base = map_with_source(
        &[(0., -3.), (2., 5.), (10., 21.), (18., 29.)],
        &["trusted"; 3],
        true,
        RATE,
        864001,
    );
    let minus = relabel(base.clone(), &[Some(-3); 3], None);
    let plus = relabel(base.clone(), &[Some(1); 3], None);
    for map in [&minus, &plus] {
        let reference = map
            .canonical_phase_reference_exact(
                |beat| beat.partial_cmp(&-3.),
                |beat| beat.partial_cmp(&29.),
            )
            .unwrap();
        assert!(reference.bar_qualified());
        assert_eq!(reference.downbeat(), 1);
    }
    let unknown = relabel(base.clone(), &[Some(1); 3], Some(1));
    let mixed = relabel(base, &[Some(1), Some(2), Some(1)], None);
    for map in [&unknown, &mixed] {
        let reference = map
            .canonical_phase_reference_exact(
                |beat| beat.partial_cmp(&-3.),
                |beat| beat.partial_cmp(&29.),
            )
            .unwrap();
        assert!(!reference.bar_qualified());
        assert_eq!(reference.downbeat(), 0);
    }
    let make = |map| {
        let mut result = MappedLoopGeometry::new(
            map,
            864001,
            RATE,
            FrameRange {
                start: 0,
                end: 864000,
            },
            32.,
            false,
        )
        .unwrap()
        .qualify_exact_continuation()
        .unwrap();
        result.prepare_common_phase();
        result
    };
    let left = make(minus);
    let right = make(plus);
    let actual = timeline(24000., 0, -4.);
    let left_reference = launch_reference(&left, actual, 0);
    let right_reference = launch_reference(&right, actual, 0);
    let a =
        MappedClockEpoch::common_phase(76, actual, &left, raw(0, -0.), 0, left_reference).unwrap();
    let b = MappedClockEpoch::common_phase(76, actual, &right, raw(0, -0.), 0, right_reference)
        .unwrap();
    assert!(
        settled(&left, a, left_reference, 0).unwrap(),
        "zero debt uses original B(A)=-3 and M0=M(T), independently of meter labels"
    );
    for frame in [0, 1, 192000, 576000, 768000, 1_000_001] {
        let expected = position(&left, a, left_reference, frame).unwrap();
        assert_pair(
            position(&right, b, right_reference, frame).unwrap(),
            expected.frame,
            expected.fraction,
        );
    }
}

#[test]
fn phase_fractional_q_has_no_false_quarter_closure_while_integer_nonbar_q_remains_legal() {
    let map = map_with_source(
        &[(0., 0.), (2., 8.), (10., 24.), (18., 32.)],
        &["trusted"; 3],
        true,
        RATE,
        864001,
    );
    let mut fractional = MappedLoopGeometry::new_general_declared(
        map.clone(),
        864001,
        RATE,
        FrameRange {
            start: 0,
            end: 840000,
        },
        31.5,
        true,
    )
    .unwrap();
    fractional.prepare_common_phase();
    assert!(!fractional.has_common_phase());
    assert!(
        fractional
            .clone()
            .with_grid_phase_origin(Some(GridPhaseOrigin {
                frame_bits: 0.0_f64.to_bits(),
                authority_revision: 71
            }))
            .is_err()
    );
    let mut quarter = MappedLoopGeometry::new(
        map,
        864001,
        RATE,
        FrameRange {
            start: 0,
            end: 24000,
        },
        2.,
        false,
    )
    .unwrap()
    .qualify_exact_continuation()
    .unwrap();
    quarter.prepare_common_phase();
    assert!(quarter.has_common_phase());
    let actual = timeline(24000., 0, 0.);
    let reference = launch_reference(&quarter, actual, 0);
    let epoch =
        MappedClockEpoch::common_phase(77, actual, &quarter, raw(0, 0.), 0, reference).unwrap();
    assert!(settled(&quarter, epoch, reference, 0).unwrap());
}

#[test]
fn phase_fine_cursor_uses_full_fraction_while_grid_retry_recovers_after_ordinary_progress() {
    let geometry = geometry(&[(0., 0.), (2., 8.), (10., 24.), (18., 32.)], 32., 864000);
    let actual = timeline(24000., 0, 0.);
    let reference = launch_reference(&geometry, actual, 0);
    let raw = raw(13, 2_f64.powi(-106));
    let mut grid_only = geometry.clone();
    grid_only.full_fraction = None;
    assert!(
        MappedClockEpoch::common_phase(78, actual, &grid_only, raw, 0, reference).is_err(),
        "the original Grid class still refuses a fraction outside its recovery grid"
    );
    let ordinary =
        MappedClockEpoch::anchored_compact(78, actual, &geometry, geometry.domain(), raw, 0)
            .unwrap();
    let fine = MappedClockEpoch::common_phase(78, actual, &geometry, raw, 0, reference).unwrap();
    assert!(fine.has_full_fraction_recovery());
    assert_pair(
        position(&geometry, fine, reference, 0).unwrap(),
        13,
        raw.fraction,
    );
    let progressed = geometry.position_at(ordinary, 1).unwrap();
    assert_ne!(progressed.fraction.to_bits(), raw.fraction.to_bits());
    let retry =
        MappedClockEpoch::common_phase(78, actual, &geometry, progressed, 1, reference).unwrap();
    assert!(!retry.has_full_fraction_recovery());
    assert_pair(
        position(&geometry, retry, reference, 1).unwrap(),
        progressed.frame,
        progressed.fraction,
    );
}

#[test]
fn phase_recovery_normalizes_large_common_odd_before_the_unchanged_reader_splitter() {
    use crate::audio_engine::general_source_numerics::{Integer, split_recovery_coordinate};
    let domain = SourceLoopDomain::physical(100, FrameRange { start: 0, end: 64 });
    let n = Integer::from_u128(39).unwrap().shl(440).unwrap();
    let d = Integer::from_u128(3).unwrap().shl(440).unwrap();
    let pair = split_recovery_coordinate(n, d, -106, -0., domain).unwrap();
    assert_eq!(
        (pair.whole, pair.fraction.to_bits()),
        (13, (-0_f64).to_bits())
    );
    let nondivisible = Integer::from_u128(40).unwrap().shl(440).unwrap();
    assert!(split_recovery_coordinate(nondivisible, d, -106, 0., domain).is_none());
    let offgrid = Integer::from_u128(13)
        .unwrap()
        .shl(107)
        .unwrap()
        .add(Integer::from_u128(1).unwrap())
        .unwrap();
    let fine = Integer::from_u128(1).unwrap().shl(107).unwrap();
    assert!(split_recovery_coordinate(offgrid, fine, -106, 0., domain).is_none());
    assert!(split_recovery_coordinate(n, d, -127, 0., domain).is_none());
}

#[test]
fn phase_exact_max_canonical_whole_includes_virtual_period_frames_and_rejects_bad_scalar_inputs() {
    use crate::audio_engine::general_source_numerics::MappedReaderPeriod;
    let period = MappedReaderPeriod::from_scalar_exact(2.25).unwrap();
    assert_eq!(period.max_canonical_frame(13), Some(15));
    assert_eq!(
        MappedReaderPeriod::from_scalar_exact(2.)
            .unwrap()
            .max_canonical_frame(13),
        Some(14)
    );
    for invalid in [0., -1., f64::NAN, f64::INFINITY] {
        assert!(MappedReaderPeriod::from_scalar_exact(invalid).is_none());
    }
}

#[test]
fn phase_nearest_infeasible_winding_tries_the_other_global_direction_without_widening_recovery() {
    let quarters = 2_f64.powi(41);
    let period = 18000_usize << 41;
    let geometry = geometry(
        &[
            (0., 0.),
            (2_f64.powi(-67), 2_f64.powi(-66)),
            (3. * 2_f64.powi(-68), 2_f64.powi(-65)),
            (3. * 2_f64.powi(38), quarters),
        ],
        quarters,
        period,
    );
    let actual = timeline(24000., 0, 0.);
    // The nearest debt is negative. Nominal MIN=.5 forbids global negative,
    // and the tiny MAX window would emit below the unchanged recovery grid.
    // Original slope exponents are6/5/4; source top56 gives GR=-70, while
    // gM=-67 gives positive-global Gout=-69 and negative-window Gout=-74.
    // The original beat/master grids and opposite global horizon remain legal.
    // The earlier tiny Q32 fixture could not even admit its full master horizon.
    let reference = launch_reference(&geometry, actual, 0);
    let epoch =
        MappedClockEpoch::common_phase(79, actual, &geometry, raw(39, 0.), 0, reference).unwrap();
    let ordinary = MappedClockEpoch::anchored_compact(
        79,
        actual,
        &geometry,
        geometry.domain(),
        raw(39, 0.),
        0,
    )
    .unwrap();
    let corrected = position(&geometry, epoch, reference, 1).unwrap();
    let plain = geometry.position_at(ordinary, 1).unwrap();
    assert_eq!(corrected.frame, plain.frame);
    assert!(
        corrected.fraction > plain.fraction,
        "the feasible other winding must correct positively"
    );
    assert!(!settled(&geometry, epoch, reference, 1).unwrap());
    assert_local(&geometry, epoch, reference, 1);
}

#[test]
fn phase_global_headroom_uses_independent_fraction_pairs_at_capture_knots_and_own_wrap() {
    let geometry = geometry(
        &[(0., 0.), (1. / 32., 8.), (5. / 64., 24.), (9. / 64., 32.)],
        32.,
        6750,
    );
    let actual = timeline(200., 0, 0.);
    let reference = launch_reference(&geometry, actual, 0);
    let epoch =
        MappedClockEpoch::common_phase(80, actual, &geometry, raw(13, 0.), 0, reference).unwrap();
    // Generated by the retained independent Fraction oracle from original
    // seconds axes and actual binary64 M, not the production phase helpers.
    for (frame, whole, bits) in [
        (0, 13, 0),
        (1, 13, 4606487527451262976),
        (887, 831, 4603303341777223030),
        (888, 832, 4602678819172647572),
        (6400, 0, 0),
        (9000, 2203, 4593671619917905920),
    ] {
        assert_pair(
            position(&geometry, epoch, reference, frame).unwrap(),
            whole,
            f64::from_bits(bits),
        );
        assert_local(&geometry, epoch, reference, frame);
    }
    assert!(!settled(&geometry, epoch, reference, 887).unwrap());
    assert!(settled(&geometry, epoch, reference, 888).unwrap());
}

#[test]
fn phase_exact_control_edges_choose_following_derivative_at_entry_exit_capture_and_wrap() {
    let geometry = geometry(&[(0., 0.), (2., 8.), (10., 24.), (18., 32.)], 32., 864000);
    let actual = timeline(24000., 0, 0.);
    let reference = launch_reference(&geometry, actual, 0);
    let epoch = MappedClockEpoch::common_phase(81, actual, &geometry, raw(12000, 0.), 0, reference)
        .unwrap();
    // beta0=1, debt=-1, k=6, MAX seed[24,32], delta=1/8. Entry t=23
    // coincides with source knot24. Exit t=31; wrap t=31+1/8; capture
    // t=23+7*(32+1/8)+8=255+7/8 coincides with another window exit.
    for (frame, before, at, after) in [
        (552000, 1_f64, 63_f64 / 32., 63_f64 / 32.),
        (744000, 63. / 32., 2., 2.),
        (747000, 2., 0.5, 0.5),
        (6141000, 63. / 32., 2., 2.),
    ] {
        for (query, expected) in [(frame - 1, before), (frame, at), (frame + 1, after)] {
            let span = assert_local(&geometry, epoch, reference, query);
            // Original loaded F=12000/24000/48000 and actual FPB=24000
            // give nominal .5/1/2; only the MAX window multiplies by63/64.
            assert_eq!(
                span.local_rate().unwrap().to_bits(),
                expected.to_bits(),
                "following derivative at actual frame{query}"
            );
        }
    }
    assert!(!settled(&geometry, epoch, reference, 6140999).unwrap());
    assert!(settled(&geometry, epoch, reference, 6141000).unwrap());
    let e = 1_u64 << 53;
    let plateau = timeline(24000., e, 0.);
    let plateau_reference = launch_reference(&geometry, plateau, e);
    let large = MappedClockEpoch::common_phase(
        82,
        plateau,
        &geometry,
        raw(12000, 0.),
        e,
        plateau_reference,
    )
    .unwrap();
    let entry = e + 552000;
    assert_eq!(
        plateau.beat_position_at_frame(entry - 1).unwrap().to_bits(),
        plateau.beat_position_at_frame(entry).unwrap().to_bits()
    );
    for query in [entry - 1, entry, entry + 1] {
        let span = assert_local(&geometry, large, plateau_reference, query);
        assert_eq!(
            span.local_rate().unwrap().to_bits(),
            (63_f64 / 32.).to_bits()
        );
    }
}

fn assert_affine_shuffled(
    geometry: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    reference: LoopPhaseReference,
    frame: u64,
) -> MappedSourceSpan {
    let span = refresh(geometry, epoch, reference, frame).unwrap();
    let local = span.local_phase_view(geometry.domain(), reference).unwrap();
    assert!(
        local.uses_affine_for_test(),
        "this legal fixed source/control region must exercise the optional affine cache"
    );
    let length = span.end_frame() - span.start_frame();
    // Absolute requests deliberately move backward and repeat. No previous
    // query or callback endpoint may become a new phase/time origin.
    for query in [
        span.end_frame() - 1,
        span.start_frame(),
        span.start_frame() + length / 3,
        span.end_frame(),
        frame,
        span.start_frame() + length / 2,
        span.start_frame(),
        span.end_frame() - 1,
    ] {
        let expected = position(geometry, epoch, reference, query).unwrap();
        assert_pair(
            local.position_at(query).unwrap(),
            expected.frame,
            expected.fraction,
        );
    }
    if span.start_frame() > 0 {
        assert!(local.position_at(span.start_frame() - 1).is_none());
    }
    if span.end_frame() < u64::MAX {
        assert!(local.position_at(span.end_frame() + 1).is_none());
    }
    span
}

#[test]
fn phase_affine_queries_keep_original_global_anchors_across_later_spans_and_actual_plateaus() {
    let geometry = geometry(&[(0., 0.), (2., 8.), (10., 24.), (18., 32.)], 32., 864000);
    for (e, anchor_frame, master) in [(0, 0, -3.125_f64), (1_u64 << 53, (1_u64 << 53) + 1, 3.125)] {
        let actual = timeline(24000., anchor_frame, master);
        let reference = launch_reference(&geometry, actual, e);
        let epoch =
            MappedClockEpoch::common_phase(83, actual, &geometry, raw(13, 0.), e, reference)
                .unwrap();
        let mut later = false;
        for offset in [0, 1, 2, 123, 192000, 577639, 768000, 1_536_001] {
            let span = assert_affine_shuffled(&geometry, epoch, reference, e + offset);
            later |= span.start_frame() > e;
        }
        assert!(
            later,
            "the test must not only exercise a view based at the epoch"
        );
        if e > 0 {
            assert_eq!(
                actual.beat_position_at_frame(e).unwrap().to_bits(),
                actual.beat_position_at_frame(e + 1).unwrap().to_bits()
            );
            let span = refresh(&geometry, epoch, reference, e).unwrap();
            let local = span.local_phase_view(geometry.domain(), reference).unwrap();
            let first = local.position_at(e).unwrap();
            assert_pair(
                local.position_at(e + 1).unwrap(),
                first.frame,
                first.fraction,
            );
        }
    }
}

#[test]
fn phase_affine_integer_master_delta_preserves_the_unrounded_halfway_witness() {
    let geometry = geometry(&[(0., 0.), (2., 8.), (10., 24.), (18., 32.)], 32., 864000);
    let actual = timeline(24000., 0, -2_f64.powi(-70));
    let reference = launch_reference(&geometry, actual, 0);
    let epoch =
        MappedClockEpoch::common_phase(84, actual, &geometry, raw(13, 0.5), 0, reference).unwrap();
    let span = assert_affine_shuffled(&geometry, epoch, reference, 2);
    let local = span.local_phase_view(geometry.domain(), reference).unwrap();
    assert_pair(position(&geometry, epoch, reference, 2).unwrap(), 14, 0.5);
    assert_pair(local.position_at(2).unwrap(), 14, 0.5);
    // Independent exact source coordinates are14.5-857/2^65. Subtracting
    // binary64 M first loses the tiny original anchor and instead produces
    // 14.5-1232/2^65: their fractional RN results lie on opposite sides.
    assert_eq!(
        (0.5_f64 - 857. / 2_f64.powi(65)).to_bits(),
        0.5_f64.to_bits()
    );
    assert_eq!(
        (0.5_f64 - 1232. / 2_f64.powi(65)).to_bits(),
        0.5_f64.to_bits() - 1
    );
    let m0 = actual.beat_position_at_frame(0).unwrap();
    let m2 = actual.beat_position_at_frame(2).unwrap();
    assert_eq!(
        (m2 - m0).to_bits(),
        m2.to_bits(),
        "the float-delta mutant really discards this original M0"
    );
}

#[test]
fn phase_nonclosed_start_uses_full_fraction_and_keeps_ordinary_rational_seam() {
    use crate::audio_engine::general_source_numerics::{Integer, MappedReaderPeriod};
    let map = map_with_source(
        &[(0., 0.), (6., 8.), (34., 64.)],
        &["trusted"; 2],
        true,
        RATE,
        1_632_001,
    );
    let region = FrameRange {
        start: 1,
        end: 864001,
    };
    let old = MappedLoopGeometry::new(map.clone(), 1_632_001, RATE, region, 32., true).unwrap();
    assert!(
        old.qualify_exact_continuation().is_err(),
        "non-binary64 B(A)=1/36000 must not be a compact certificate"
    );
    let mut geometry =
        MappedLoopGeometry::new_general_declared(map, 1_632_001, RATE, region, 32., true).unwrap();
    geometry.prepare_common_phase();
    assert!(geometry.has_general_continuation());
    assert!(!geometry.has_compact_continuation());
    assert!(geometry.has_common_phase());
    let exact_period = MappedReaderPeriod::new(
        Integer::from_u128(2_591_999).unwrap(),
        Integer::from_u128(3).unwrap(),
    )
    .unwrap();
    assert_eq!(
        geometry.domain().musical_period,
        crate::audio_engine::source_reader::SourceReaderPeriod::MappedRational(exact_period)
    );
    let actual = timeline(24000., 0, 0.);
    let reference = launch_reference(&geometry, actual, 0);
    // Raw B(39)=13/12000 meets common-odd closure, but the intentional
    // B(A)=1/36000 does not: multiplying by the second slope24000 gives2/3.
    // Its old absolute-zero phase oracle was not a valid launch-relative target.
    let phase =
        MappedClockEpoch::common_phase(85, actual, &geometry, raw(39, -0.), 0, reference).unwrap();
    assert!(phase.has_full_fraction_recovery());
    assert_pair(position(&geometry, phase, reference, 0).unwrap(), 39, -0.);
    let ordinary_raw =
        MappedClockEpoch::ordinary_mapped_recovery(85, actual, &geometry, raw(39, -0.), 0).unwrap();
    assert_pair(geometry.position_at(ordinary_raw, 0).unwrap(), 39, -0.);
    assert!(geometry.position_at(ordinary_raw, 1).is_ok());
    let ordinary =
        MappedClockEpoch::ordinary_mapped_recovery(85, actual, &geometry, raw(864000, -0.), 0)
            .unwrap();
    // P=863999+2/3 still permits this virtual last whole. The original
    // general reader retains its exact seam and first raw negative-zero bits.
    assert_pair(geometry.position_at(ordinary, 0).unwrap(), 864000, -0.);
    let seam = geometry.refresh_span(ordinary, 0).unwrap();
    assert!(seam.end_frame() > 0);
    let local = seam.local_general_view(geometry.domain()).unwrap();
    for frame in [0, seam.end_frame()] {
        let expected = geometry.position_at(ordinary, frame).unwrap();
        assert_pair(
            local.position_at(frame).unwrap(),
            expected.frame,
            expected.fraction,
        );
    }
    assert_eq!(geometry.position_at(ordinary, 1).unwrap().frame, 1);
    assert!(geometry.position_at(ordinary, seam.end_frame()).is_ok());
}

#[test]
fn phase_affine_optional_cache_falls_back_for_a_copy_crossing_only_a_control_exit() {
    let geometry = geometry(&[(0., 0.), (2., 8.), (10., 24.), (18., 32.)], 32., 864000);
    let actual = timeline(24000., 0, 0.);
    let reference = launch_reference(&geometry, actual, 0);
    let epoch = MappedClockEpoch::common_phase(86, actual, &geometry, raw(12000, 0.), 0, reference)
        .unwrap();
    let original = refresh(&geometry, epoch, reference, 600000).unwrap();
    assert_eq!(
        (original.start_frame(), original.end_frame()),
        (552000, 744000)
    );
    assert!(
        original
            .local_phase_view(geometry.domain(), reference)
            .unwrap()
            .uses_affine_for_test()
    );
    let endpoint = position(&geometry, epoch, reference, 744100).unwrap();
    // Numeric malformed-copy negative only. The held MAX source segment is
    // still correct until the original source wrap747000, but this copy's
    // fixed control region is no longer valid after744000. It grants no permit.
    let copied = MappedSourceSpan {
        end_frame: 744100,
        endpoint_frame: endpoint.frame,
        endpoint_fraction: endpoint.fraction,
        ..original
    };
    let local = copied
        .local_phase_view(geometry.domain(), reference)
        .unwrap();
    assert!(!local.uses_affine_for_test());
    for frame in [744099, 552000, 743999, 744001, 600000, 744000, 744100] {
        let expected = position(&geometry, epoch, reference, frame).unwrap();
        assert_pair(
            local.position_at(frame).unwrap(),
            expected.frame,
            expected.fraction,
        );
    }
}

#[test]
fn phase_affine_later_wrap_reference_keeps_exact_unrounded_source_instead_of_its_reader_pair() {
    let geometry = geometry(&[(0., 0.), (8., 32.)], 32., 384000);
    let actual = timeline(24000., 0, 0.);
    let reference = launch_reference(&geometry, actual, 0);
    let raw_fraction = f64::from_bits(4_562_146_422_526_312_748);
    assert_eq!(
        raw_fraction.to_bits(),
        (2_f64.powi(-10) + 300. * 2_f64.powi(-62)).to_bits()
    );
    let epoch =
        MappedClockEpoch::common_phase(87, actual, &geometry, raw(13, raw_fraction), 0, reference)
            .unwrap();
    let span = refresh(&geometry, epoch, reference, 756161).unwrap();
    assert_eq!(span.start_frame(), 756160);
    assert!(span.start_frame() > 0);
    assert!(!settled(&geometry, epoch, reference, 756160).unwrap());
    let local = span.local_phase_view(geometry.domain(), reference).unwrap();
    assert!(local.uses_affine_for_test());

    // Independent Fraction proof retained by Root: at the first wrap the
    // exact source base is577586652224909387/2^60. Its reader pair rounds
    // upward to4512395720507105/2^53, an error of53/2^60=212/2^62.
    // Reusing that rounded pair as an affine origin adds26 fraction ulps at
    // the very next output frame. Neither expected bit pattern uses a cache
    // helper or the global implementation to generate its expected value.
    let exact_base_n = 577_586_652_224_909_387_u128;
    let rounded_base_n = 4_512_395_720_507_105_u128;
    assert_eq!((rounded_base_n << 7) - exact_base_n, 53);
    assert_ne!(exact_base_n % 128, 0);
    let rounded = position(&geometry, epoch, reference, 756160).unwrap();
    assert_pair(rounded, 0, f64::from_bits(4_602_687_615_265_783_521));
    assert_pair(local.position_at(756160).unwrap(), 0, rounded.fraction);
    let correct_bits = 4_576_220_171_353_924_646_u64;
    let rounded_base_mutant_bits = 4_576_220_171_353_924_672_u64;
    assert_eq!(rounded_base_mutant_bits - correct_bits, 26);
    assert_pair(
        position(&geometry, epoch, reference, 756161).unwrap(),
        1,
        f64::from_bits(correct_bits),
    );
    let actual = local.position_at(756161).unwrap();
    assert_pair(actual, 1, f64::from_bits(correct_bits));
    assert_ne!(actual.fraction.to_bits(), rounded_base_mutant_bits);
}

#[test]
fn phase_intentional_common_launch_preserves_original_b64_b256_and_each_own_wrap() {
    let master = geometry(&[(0., 64.), (2., 72.)], 8., 96_000);
    let slave = geometry(
        &[(0., 256.), (16., 320.), (80., 448.), (144., 512.)],
        256.,
        6_912_000,
    );
    let actual = timeline(24_000., 0, 64.);
    let master_reference = launch_reference(&master, actual, 0);
    let slave_reference = launch_reference(&slave, actual, 0);
    assert!(
        MappedClockEpoch::common_phase(91, actual, &slave, raw(0, -0.), 0, master_reference,)
            .is_err()
    );
    for (geometry, reference, marker, wrap) in [
        (&master, master_reference, 90, 192_000),
        (&slave, slave_reference, 91, 6_144_000),
    ] {
        let epoch =
            MappedClockEpoch::common_phase(marker, actual, geometry, raw(0, -0.), 0, reference)
                .unwrap();
        assert!(settled(geometry, epoch, reference, 0).unwrap());
        assert_pair(position(geometry, epoch, reference, 0).unwrap(), 0, -0.);
        assert_pair(
            position(geometry, epoch, reference, 24_000).unwrap(),
            12_000,
            -0.,
        );
        assert_pair(position(geometry, epoch, reference, wrap).unwrap(), 0, -0.);
        for frame in [0, 1, 24_000, wrap - 1, wrap, wrap + 1] {
            assert_affine_shuffled(geometry, epoch, reference, frame);
        }
        // A copied phase epoch has no implicit canonical-zero target.
        assert!(geometry.position_at(epoch, 0).is_err());
        assert!(geometry.refresh_span(epoch, 0).is_err());
        assert!(geometry.validate_epoch(epoch).is_err());
        assert!(
            MappedClockEpoch::common_phase(
                marker,
                actual,
                geometry,
                raw(0, -0.),
                0,
                LoopPhaseReference::Unavailable,
            )
            .is_err()
        );
        let span = refresh(geometry, epoch, reference, 0).unwrap();
        assert!(
            span.local_phase_view(geometry.domain(), LoopPhaseReference::Unavailable)
                .is_err()
        );
    }
}

#[test]
fn phase_fractional_original_start_uses_two_inverse_segments_for_both_capture_directions() {
    use crate::audio_engine::general_source_numerics::{Integer, MappedReaderPeriod};
    let map = map_with_source(
        &[(0., 0.), (6., 8.), (27., 64.)],
        &["trusted"; 2],
        true,
        RATE,
        1_296_001,
    );
    let mut geometry = MappedLoopGeometry::new_general_declared(
        map,
        1_296_001,
        RATE,
        FrameRange {
            start: 1,
            end: 720_001,
        },
        32.,
        true,
    )
    .unwrap();
    geometry.prepare_common_phase();
    assert!(geometry.has_common_phase());
    assert_eq!(
        geometry.domain().musical_period,
        crate::audio_engine::source_reader::SourceReaderPeriod::MappedRational(
            MappedReaderPeriod::new(
                Integer::from_u128(1_439_999).unwrap(),
                Integer::from_u128(2).unwrap(),
            )
            .unwrap()
        )
    );
    let actual = timeline(24_000., 0, 64.);
    let reference = launch_reference(&geometry, actual, 0);
    // Original B(A)=1/36000 is not binary64. Forward slopes36000/18000
    // nevertheless share odd1125, so both target and current inverse close.
    // The current points are in the SECOND segment; A is in the FIRST.
    // Positive: beta0=31+B(A), M0=M(T), debt+1 with winding-1.
    // Negative: beta0=9+B(A), M0-M(T)=8, debt-1 with winding0.
    // With k6 both first captures occur after exactly64 actual quarters.
    for (marker, execution, first, capture, expected, next_wrap, negative) in [
        (
            92,
            0,
            raw(702_000, 0.5),
            1_536_000,
            raw(1, 0.),
            2_304_000,
            false,
        ),
        (
            93,
            192_000,
            raw(306_000, 0.5),
            1_728_000,
            raw(288_000, 0.5),
            2_304_000,
            true,
        ),
    ] {
        let epoch =
            MappedClockEpoch::common_phase(marker, actual, &geometry, first, execution, reference)
                .unwrap();
        assert_pair(
            position(&geometry, epoch, reference, execution).unwrap(),
            first.frame,
            first.fraction,
        );
        assert!(!settled(&geometry, epoch, reference, execution).unwrap());
        assert!(!settled(&geometry, epoch, reference, capture - 1).unwrap());
        assert!(settled(&geometry, epoch, reference, capture).unwrap());
        assert_pair(
            position(&geometry, epoch, reference, capture).unwrap(),
            expected.frame,
            expected.fraction,
        );
        assert_pair(
            position(&geometry, epoch, reference, next_wrap).unwrap(),
            1,
            0.,
        );
        let first_span = refresh(&geometry, epoch, reference, execution).unwrap();
        assert_eq!(
            first_span.local_rate().unwrap().to_bits(),
            (if negative {
                189_f64 / 256.
            } else {
                195_f64 / 256.
            })
            .to_bits(),
        );
        let captured_span = refresh(&geometry, epoch, reference, capture).unwrap();
        assert_eq!(
            captured_span.local_rate().unwrap().to_bits(),
            (if negative { 0.75_f64 } else { 1.5_f64 }).to_bits(),
        );
        for frame in [
            execution,
            execution + 1,
            capture - 1,
            capture,
            capture + 1,
            next_wrap - 1,
            next_wrap,
            next_wrap + 1,
        ] {
            assert_affine_shuffled(&geometry, epoch, reference, frame);
        }
    }
}
