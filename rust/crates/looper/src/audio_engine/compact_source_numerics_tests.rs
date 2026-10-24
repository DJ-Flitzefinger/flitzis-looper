use super::*;
use crate::audio_engine::source_numerics::{
    SOURCE_GRID_SHIFT, SourceInteger, split_source_coordinate_preserving_fraction,
};
use crate::test_memory::{ThreadAllocations, observe_current_thread};

fn grid(exponent: i16) -> Grid {
    GridStats::new().finish().unwrap().refine(exponent).unwrap()
}

fn phase(value: i128) -> PhaseInteger {
    PhaseInteger::from_i128(value).unwrap()
}

fn coordinate(value: i128) -> CoordinateInteger {
    CoordinateInteger::from_i128(value).unwrap()
}

fn power(exponent: u32) -> CoordinateInteger {
    coordinate(1).checked_shl(exponent).unwrap()
}

fn wide<const N: usize>(value: SmallInteger<N>) -> SourceInteger {
    // Independent 128-bit chunks reconstruct the exact value in the unchanged reference.
    let mut result = SourceInteger::ZERO;
    for chunk in value.magnitude.as_bytes().rchunks(16) {
        let mut bytes = [0_u8; 16];
        bytes[..chunk.len()].copy_from_slice(chunk);
        result = result
            .checked_shl(128)
            .unwrap()
            .checked_add(SourceInteger::from_integer(u128::from_le_bytes(bytes)).unwrap())
            .unwrap();
    }
    if value.negative {
        result.negated()
    } else {
        result
    }
}

fn wide_pair(coefficient: i128, exponent: i16) -> SourceInteger {
    wide(phase(coefficient))
        .checked_shl(u32::try_from(SOURCE_GRID_SHIFT as i16 + exponent).unwrap())
        .unwrap()
}

fn assert_pair(actual: SourceCoordinate, whole: u128, fraction_bits: u64) {
    assert_eq!(actual.whole, whole);
    assert_eq!(actual.fraction.to_bits(), fraction_bits);
}

#[test]
fn grid_statistics_keep_exact_full_scan_headroom_and_refuse_finer_class() {
    let base = GridStats::new()
        .include_f64(1.25)
        .unwrap()
        .finish()
        .unwrap();
    let finest = base.refine(-125).unwrap();
    assert_eq!(finest.exponent(), -125);
    assert_eq!(decode_f64_at_grid(1.25, finest), Some(5_i128 << 123));
    assert!(base.refine(-126).is_none());
    assert!(
        GridStats::new()
            .include_f64(2.0_f64.powi(126))
            .unwrap()
            .finish()
            .is_none()
    );
    assert!(
        GridStats::new()
            .include_f64(2.0_f64.powi(-127))
            .unwrap()
            .finish()
            .is_none()
    );
    let integer = GridStats::new()
        .include_u64(u64::MAX)
        .unwrap()
        .finish()
        .unwrap();
    assert!(integer.refine(-62).is_some());
    assert!(integer.refine(-63).is_none());
    assert_eq!(required_grid_f64(-0.0), Some(0));
    assert_eq!(required_grid_f64(f64::from_bits(1)), Some(-1074));
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(GridStats::new().include_f64(invalid).is_none());
        assert!(GridStats::new().include_scaled_f64(invalid, 0).is_none());
    }
}

#[test]
fn exact_decode_keeps_unrounded_scaled_significand_and_separate_source_pair() {
    let fine = grid(-60);
    let exact = decode_scaled_f64_at_grid(0.1, 10, fine).unwrap();
    assert_ne!(exact, decode_f64_at_grid(0.1 * 10.0, fine).unwrap());
    assert_eq!(
        wide_pair(exact, -60),
        SourceInteger::from_f64_grid_scaled(0.1, 10).unwrap()
    );
    assert_eq!(decode_scaled_f64_at_grid(-0.1, 10, fine), Some(-exact));
    assert_eq!(decode_scaled_f64_at_grid(f64::MAX, 0, fine), Some(0));
    assert!(decode_scaled_f64_at_grid(f64::INFINITY, 0, fine).is_none());
    assert!(decode_scaled_f64_at_grid(f64::NAN, 0, fine).is_none());
    let pair_grid = grid(-61);
    let frame = (1_u64 << 53) + 7;
    let decoded = decode_source_pair_at_grid(frame, 0.25, pair_grid).unwrap();
    assert_eq!(
        wide_pair(decoded, -61),
        SourceInteger::from_source_pair_grid(frame, 0.25).unwrap()
    );
    assert_eq!(
        decode_source_pair_at_grid(frame, -0.0, pair_grid),
        Some(i128::from(frame) << 61)
    );
    for fraction in [-0.25, 1.0, f64::INFINITY, f64::NAN, f64::from_bits(1)] {
        assert!(decode_source_pair_at_grid(frame, fraction, pair_grid).is_none());
    }
    assert!(decode_f64_at_grid(2.0_f64.powi(126), grid(0)).is_none());
    assert!(decode_f64_at_grid(-2.0_f64.powi(126), grid(0)).is_none());
    assert!(decode_f64_at_grid(0.5, grid(0)).is_none());
}

#[test]
fn remote_scaled_probes_match_general_exact_ordering_for_all_binary64_exponents() {
    let target_grid = grid(-53);
    for encoded in 0_u64..=2046 {
        for fraction in [0, 1, (1_u64 << 52) - 1] {
            for sign in [0, 1_u64 << 63] {
                let value = f64::from_bits(sign | (encoded << 52) | fraction);
                for factor in [0, 48_000, u32::MAX] {
                    let original = SourceInteger::from_f64_grid_scaled(value, factor).unwrap();
                    for target in [-((1_i128 << 125) - 1), -1, 0, 1, (1_i128 << 125) - 1] {
                        assert_eq!(
                            compare_scaled_f64_to_grid(value, factor, target, target_grid),
                            Some(original.cmp(&wide_pair(target, -53))),
                            "encoded={encoded} fraction={fraction} sign={sign} factor={factor} target={target}"
                        );
                    }
                }
            }
        }
    }
    assert!(compare_scaled_f64_to_grid(f64::NAN, 0, 0, target_grid).is_none());
    assert!(compare_scaled_f64_to_grid(f64::INFINITY, 0, 0, target_grid).is_none());
    assert!(compare_scaled_f64_to_grid(0.0, 1, 1_i128 << 126, target_grid).is_none());
}

#[test]
fn structural_master_grid_includes_interior_precision_and_original_anchor() {
    assert_eq!(master_grid_exponent(24_000.0, 0.0), Some(-67));
    assert_eq!(master_grid_exponent(1.0, 0.0), Some(-53));
    assert_eq!(master_grid_exponent(2.0_f64.powi(150), 0.0), Some(-203));
    // Individually integer endpoints do not permit an endpoint-only coarse grid.
    let endpoints = GridStats::new()
        .include_f64(0.0)
        .unwrap()
        .include_f64(1.0)
        .unwrap()
        .finish()
        .unwrap();
    assert!(
        endpoints
            .refine(master_grid_exponent(2.0_f64.powi(150), 0.0).unwrap())
            .is_none()
    );
    assert_eq!(master_grid_exponent(1.0, 2.0_f64.powi(-100)), Some(-100));
    assert_eq!(master_grid_exponent(1.0, f64::from_bits(1)), Some(-1074));
    for invalid in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert!(master_grid_exponent(invalid, 0.0).is_none());
    }
    assert!(master_grid_exponent(1.0, f64::NAN).is_none());
    assert_eq!(exact_f64_parts(2.0_f64.powi(-1000)), Some((1, -1000)));
    assert_eq!(exact_f64_parts(f64::from_bits(1)), Some((1, -1074)));
    assert_eq!(exact_f64_parts(-0.75), Some((-3, -2)));
    assert_eq!(exact_f64_parts(-0.0), Some((0, 0)));
    assert!(exact_f64_parts(f64::INFINITY).is_none());
    assert!(exact_f64_parts(f64::NAN).is_none());
}

#[test]
fn fixed_sign_arithmetic_keeps_native_overflow_euclidean_zero_and_lossless_shifts() {
    let maximum = phase(i128::MAX);
    let product = maximum.checked_mul_i128(i128::MAX).unwrap();
    assert_eq!(
        wide(product),
        wide(maximum).checked_mul(wide(maximum)).unwrap()
    );
    assert_eq!(product.magnitude_bits(), 254);
    assert_eq!(phase(-7).rem_euclid(phase(3)), Some(phase(2)));
    assert_eq!(phase(-6).rem_euclid(phase(3)), Some(PhaseInteger::ZERO));
    assert!(!phase(-6).rem_euclid(phase(3)).unwrap().is_negative());
    assert_eq!(
        phase(i128::MIN).checked_sub(phase(i128::MIN)),
        Some(PhaseInteger::ZERO)
    );
    assert_eq!(phase(-11).checked_mul_i128(-3), Some(phase(33)));
    assert!(phase(1).rem_euclid(PhaseInteger::ZERO).is_none());
    assert!(phase(1).rem_euclid(phase(-1)).is_none());
    let high = phase(1).checked_shl(255).unwrap();
    assert!(high.checked_mul_i128(2).is_none());
    assert!(high.checked_shl(1).is_none());
    assert!(PhaseInteger::ZERO.checked_shl(256).is_none());
    assert_eq!(
        PhaseInteger::ZERO.checked_shl(255),
        Some(PhaseInteger::ZERO)
    );
    assert_eq!(wide(high.widen()), wide(high));
    assert!(power(511).checked_mul_i128(2).is_none());
    assert!(CoordinateInteger::ZERO.checked_shl(512).is_none());
    assert!(!std::mem::needs_drop::<PhaseInteger>());
    assert!(!std::mem::needs_drop::<CoordinateInteger>());
    assert_eq!(std::mem::size_of::<bnum::Uint<32>>(), 32);
    assert_eq!(std::mem::size_of::<bnum::Uint<64>>(), 64);
}

#[test]
fn split_rounds_independent_even_odd_ties_carries_and_virtual_endpoint() {
    let source_grid = grid(-60);
    let denominator = coordinate(1_i128 << 54);
    let even = split_normalized_source_coordinate(
        coordinate((1_i128 << 53) + 1),
        denominator,
        source_grid,
        0.0,
        0,
        2.0,
    )
    .unwrap();
    assert_pair(even, 0, 0x3fe0_0000_0000_0000);
    let odd = split_normalized_source_coordinate(
        coordinate((1_i128 << 53) + 3),
        denominator,
        source_grid,
        0.0,
        0,
        2.0,
    )
    .unwrap();
    assert_pair(odd, 0, 0x3fe0_0000_0000_0002);
    let carry = split_normalized_source_coordinate(
        coordinate((1_i128 << 60) - 1),
        coordinate(1_i128 << 60),
        source_grid,
        -0.0,
        0,
        2.0,
    )
    .unwrap();
    assert_pair(carry, 1, 0);
    let boundary = split_normalized_source_coordinate(
        coordinate((1_i128 << 124) - 1),
        coordinate(1_i128 << 60),
        grid(0),
        -0.0,
        0,
        WHOLE_LIMIT as f64,
    )
    .unwrap();
    assert_pair(boundary, 0, 0);
    let exact_endpoint = split_normalized_source_coordinate(
        coordinate(1_i128 << 64),
        coordinate(1),
        grid(0),
        -0.0,
        0,
        WHOLE_LIMIT as f64,
    )
    .unwrap();
    assert_pair(exact_endpoint, 0, 0);
    assert!(
        split_normalized_source_coordinate(
            coordinate((1_i128 << 65) + 1),
            coordinate(2),
            grid(0),
            0.0,
            0,
            WHOLE_LIMIT as f64
        )
        .is_none()
    );
}

#[test]
fn raw_fraction_identity_and_finer_rounded_outputs_keep_original_domain() {
    let source_grid = grid(-2);
    assert_pair(
        split_normalized_source_coordinate(
            coordinate(71),
            coordinate(4),
            source_grid,
            0.75,
            0,
            64.0,
        )
        .unwrap(),
        17,
        0.75_f64.to_bits(),
    );
    assert_pair(
        split_normalized_source_coordinate(
            coordinate(51),
            coordinate(3),
            source_grid,
            -0.0,
            0,
            64.0,
        )
        .unwrap(),
        17,
        (-0.0_f64).to_bits(),
    );
    let fine = SourceCoordinate {
        whole: 0,
        fraction: 2.0_f64.powi(-400),
    };
    assert_pair(
        normalize_source_coordinate(fine, grid(0), 0, 1.0).unwrap(),
        0,
        fine.fraction.to_bits(),
    );
    assert!(
        normalize_source_coordinate(
            SourceCoordinate {
                whole: 0,
                fraction: 2.0_f64.powi(-433)
            },
            grid(0),
            0,
            1.0
        )
        .is_none()
    );
    let high_denominator = power(379).checked_add(power(327)).unwrap();
    let actual =
        split_normalized_source_coordinate(coordinate(1), high_denominator, grid(0), 0.0, 0, 1.0)
            .unwrap();
    let reference = split_source_coordinate_preserving_fraction(
        wide(coordinate(1)),
        wide(high_denominator),
        0.0,
    )
    .unwrap();
    assert_pair(actual, reference.whole, reference.fraction.to_bits());
    assert!(required_grid_f64(actual.fraction).unwrap() < -126);
    assert_pair(
        normalize_source_coordinate(
            SourceCoordinate {
                whole: 17,
                fraction: -0.0,
            },
            source_grid,
            17,
            0.5,
        )
        .unwrap(),
        17,
        (-0.0_f64).to_bits(),
    );
    assert_pair(
        normalize_source_coordinate(
            SourceCoordinate {
                whole: 17,
                fraction: 0.5,
            },
            source_grid,
            17,
            0.5,
        )
        .unwrap(),
        17,
        0,
    );
}

#[test]
fn compact_split_matches_unchanged_wide_reference_for_bounded_rational_fixtures() {
    for denominator in [3_i128, 7, 11, (1_i128 << 53) - 1, (1_i128 << 80) + 19] {
        for whole in [0_i128, 1, 17, (1_i128 << 53) + 7] {
            for remainder in [0, 1, denominator / 2, denominator - 1] {
                let den = coordinate(denominator);
                let num = den
                    .checked_mul_i128(whole)
                    .unwrap()
                    .checked_add(coordinate(remainder))
                    .unwrap();
                for raw in [-0.0, 0.25, 0.75] {
                    let reference =
                        split_source_coordinate_preserving_fraction(wide(num), wide(den), raw)
                            .unwrap();
                    let actual = split_normalized_source_coordinate(
                        num,
                        den,
                        grid(-2),
                        raw,
                        0,
                        (1_u64 << 54) as f64,
                    )
                    .unwrap();
                    assert_pair(actual, reference.whole, reference.fraction.to_bits());
                }
            }
        }
    }
}

#[test]
fn compact_factorization_matches_every_original_scaled_intermediate_and_source_pair() {
    // Original seconds 0,.5,1 -> beats 0,1,4 at 48k. Anchor in the second
    // segment, then add half a beat. Its original slope gives exactly 44000+.25.
    let s = -60_i16;
    let t = -67_i16;
    let x0 = 24_000_i128 << -s;
    let x1 = 48_000_i128 << -s;
    let b0 = 1_i128 << -t;
    let b1 = 4_i128 << -t;
    let q = 4_i128 << -t;
    let a = x1 - x0;
    let db = b1 - b0;
    let anchor = decode_source_pair_at_grid(40_000, 0.25, grid(s)).unwrap();
    let c = phase(x0)
        .checked_mul_i128(db)
        .unwrap()
        .checked_sub(phase(b0).checked_mul_i128(a).unwrap())
        .unwrap();
    let initial = phase(anchor)
        .checked_mul_i128(db)
        .unwrap()
        .checked_sub(c)
        .unwrap();
    let period = phase(q).checked_mul_i128(a).unwrap();
    assert!(initial >= PhaseInteger::ZERO && initial < period);
    let unfolded = initial
        .checked_add(phase(1_i128 << (-t - 1)).checked_mul_i128(a).unwrap())
        .unwrap();
    let remainder = unfolded.rem_euclid(period).unwrap();
    let numerator = c
        .widen()
        .checked_mul_i128(a)
        .unwrap()
        .checked_add(remainder.widen().checked_mul_i128(a).unwrap())
        .unwrap();
    let raw_denominator = phase(a).checked_mul_i128(db).unwrap();

    let gx0 = wide_pair(x0, s);
    let ga = wide_pair(x1, s).checked_sub(gx0).unwrap();
    let gb0 = wide_pair(b0, t);
    let gdb = wide_pair(b1, t).checked_sub(gb0).unwrap();
    // These are the preceding original operation order and explicit G scales.
    let original_initial = gb0
        .checked_mul(ga)
        .unwrap()
        .checked_add(
            wide_pair(anchor, s)
                .checked_sub(gx0)
                .unwrap()
                .checked_mul(gdb)
                .unwrap(),
        )
        .unwrap();
    let original_period = wide_pair(q, t).checked_mul(ga).unwrap();
    let original_unfolded = original_initial
        .checked_add(wide_pair(1_i128 << (-t - 1), t).checked_mul(ga).unwrap())
        .unwrap();
    let original_remainder = original_unfolded.rem_euclid(original_period).unwrap();
    let original_offset = original_remainder
        .checked_sub(gb0.checked_mul(ga).unwrap())
        .unwrap();
    let original_numerator = gx0
        .checked_mul(ga)
        .unwrap()
        .checked_mul(gdb)
        .unwrap()
        .checked_add(original_offset.checked_mul(ga).unwrap())
        .unwrap();
    let original_denominator = ga
        .checked_mul(gdb)
        .unwrap()
        .checked_shl(SOURCE_GRID_SHIFT)
        .unwrap();
    let phase_scale = u32::try_from(2148 + s + t).unwrap();
    assert_eq!(
        original_initial,
        wide(initial).checked_shl(phase_scale).unwrap()
    );
    assert_eq!(
        original_period,
        wide(period).checked_shl(phase_scale).unwrap()
    );
    assert_eq!(
        original_unfolded,
        wide(unfolded).checked_shl(phase_scale).unwrap()
    );
    assert_eq!(
        original_remainder,
        wide(remainder).checked_shl(phase_scale).unwrap()
    );
    assert_eq!(
        original_numerator,
        wide(numerator)
            .checked_shl(u32::try_from(3222 + 2 * s + t).unwrap())
            .unwrap()
    );
    assert_eq!(
        original_denominator,
        wide(raw_denominator)
            .checked_shl(u32::try_from(3222 + s + t).unwrap())
            .unwrap()
    );
    let effective_denominator = raw_denominator
        .widen()
        .checked_shl(u32::try_from(-s).unwrap())
        .unwrap();
    let actual = split_normalized_source_coordinate(
        numerator,
        effective_denominator,
        grid(s),
        0.25,
        0,
        48_000.0,
    )
    .unwrap();
    let reference =
        split_source_coordinate_preserving_fraction(original_numerator, original_denominator, 0.25)
            .unwrap();
    assert_pair(actual, 44_000, 0.25_f64.to_bits());
    assert_pair(actual, reference.whole, reference.fraction.to_bits());
}

#[test]
fn compact_local_capacity_and_invalid_inputs_refuse_without_hidden_wide_fallback() {
    assert!(
        split_normalized_source_coordinate(power(383), power(379), grid(0), 0.0, 0, 32.0).is_none()
    );
    assert!(
        split_normalized_source_coordinate(coordinate(1), power(380), grid(0), 0.0, 0, 1.0)
            .is_none()
    );
    // The explicit preceding reference still accepts this value outside the compact class.
    assert!(
        split_source_coordinate_preserving_fraction(wide(coordinate(1)), wide(power(380)), 0.0)
            .is_some()
    );
    for denominator in [coordinate(0), coordinate(-1)] {
        assert!(
            split_normalized_source_coordinate(coordinate(0), denominator, grid(0), 0.0, 0, 1.0)
                .is_none()
        );
    }
    assert!(
        split_normalized_source_coordinate(coordinate(-1), coordinate(1), grid(0), 0.0, 0, 1.0)
            .is_none()
    );
    for raw in [f64::from_bits(1), -0.25, 1.0, f64::INFINITY, f64::NAN] {
        assert!(
            split_normalized_source_coordinate(coordinate(0), coordinate(1), grid(0), raw, 0, 1.0)
                .is_none()
        );
    }
    for period in [0.0, 0.25, f64::INFINITY, f64::NAN] {
        assert!(
            split_normalized_source_coordinate(
                coordinate(0),
                coordinate(1),
                grid(0),
                0.0,
                0,
                period
            )
            .is_none()
        );
    }
}

#[test]
fn admitted_small_arithmetic_requests_no_rust_allocations_on_observed_thread() {
    let source_grid = grid(-60);
    let numerator = coordinate((1_i128 << 60) - 1);
    let denominator = coordinate(1_i128 << 60);
    let ((remainder, position, ordering), allocations) = observe_current_thread(|| {
        (
            phase(-7).rem_euclid(phase(3)),
            split_normalized_source_coordinate(numerator, denominator, source_grid, -0.0, 0, 2.0),
            compare_scaled_f64_to_grid(f64::MAX, u32::MAX, 7, source_grid),
        )
    });
    assert_eq!(remainder, Some(phase(2)));
    assert_pair(position.unwrap(), 1, 0);
    assert_eq!(ordering, Some(Ordering::Greater));
    assert_eq!(allocations, ThreadAllocations::default());
}
