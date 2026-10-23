use super::{
    Magnitude, SOURCE_GRID_SHIFT, SOURCE_MAGNITUDE_BITS, SOURCE_WHOLE_LIMIT, SourceCoordinate,
    SourceInteger, magnitude_u128, split_source_coordinate,
    split_source_coordinate_preserving_fraction,
};
use crate::test_memory::{ThreadAllocations, observe_current_thread};
use std::cmp::Ordering;

fn integer(value: u128) -> SourceInteger {
    SourceInteger::from_integer(value).expect("bounded integer fixture")
}

fn power(exponent: u32) -> SourceInteger {
    integer(1)
        .checked_shl(exponent)
        .expect("bounded power fixture")
}

fn coordinate(numerator: SourceInteger, denominator: SourceInteger) -> SourceCoordinate {
    split_source_coordinate(numerator, denominator).expect("admitted rational fixture")
}

// Retain the pre-fusion decode followed by the original generic bnum multiplication.
// This oracle does not call either optimized grid decoding or optimized multiplication.
fn original_grid_scaled(value: f64, factor: u32) -> Option<SourceInteger> {
    let bits = value.to_bits();
    let exponent = u32::try_from((bits >> 52) & 0x7ff).ok()?;
    if exponent == 0x7ff {
        return None;
    }
    let fraction = bits & ((1_u64 << 52) - 1);
    let (significand, shift) = if exponent == 0 {
        (fraction, 0)
    } else {
        (fraction | (1_u64 << 52), exponent.checked_sub(1)?)
    };
    // Every finite unscaled decode is below 2^2098, so this shift cannot discard a bit.
    let decoded = Magnitude::try_from(significand).ok()?.checked_shl(shift)?;
    let magnitude = decoded.checked_mul(Magnitude::try_from(factor).ok()?)?;
    Some(SourceInteger::canonical(magnitude, bits >> 63 != 0))
}

// Preserve the original division/direct-qD algorithm independently of the new remainder API
// and shared multiplication shortcuts.
fn original_div_rem_euclid(
    numerator: SourceInteger,
    denominator: SourceInteger,
) -> Option<(SourceInteger, SourceInteger)> {
    if denominator.negative || denominator.magnitude.is_zero() {
        return None;
    }
    let quotient = numerator.magnitude.checked_div(denominator.magnitude)?;
    let remainder = numerator
        .magnitude
        .checked_sub(quotient.checked_mul(denominator.magnitude)?)?;
    if numerator.negative && !remainder.is_zero() {
        Some((
            SourceInteger::canonical(quotient.checked_add(Magnitude::from(true))?, true),
            SourceInteger::canonical(denominator.magnitude.checked_sub(remainder)?, false),
        ))
    } else {
        Some((
            SourceInteger::canonical(quotient, numerator.negative),
            SourceInteger::canonical(remainder, false),
        ))
    }
}

#[test]
fn shared_split_preserves_exact_fraction_identity_without_changing_numeric_zero() {
    for (whole, fraction) in [
        (0, -0.0),
        ((1_u128 << 53) + 7, -0.0),
        (7, f64::from_bits(1)),
        (11, 0.75),
    ] {
        let denominator = power(SOURCE_GRID_SHIFT);
        let numerator = integer(whole)
            .checked_mul(denominator)
            .unwrap()
            .checked_add(SourceInteger::from_f64_grid(fraction).unwrap())
            .unwrap();
        let raw =
            split_source_coordinate_preserving_fraction(numerator, denominator, fraction).unwrap();
        assert_eq!(raw.whole, whole);
        assert_eq!(raw.fraction.to_bits(), fraction.to_bits());
        let numeric = coordinate(numerator, denominator);
        assert_eq!(numeric.whole, whole);
        if fraction == 0.0 {
            assert_eq!(numeric.fraction.to_bits(), 0);
        }
    }
    // minsub/3 is not minsub: generic identity must allow legitimate underflow.
    let denominator = power(SOURCE_GRID_SHIFT).checked_mul(integer(3)).unwrap();
    let rounded =
        split_source_coordinate_preserving_fraction(integer(1), denominator, f64::from_bits(1))
            .unwrap();
    assert_eq!(rounded.fraction.to_bits(), 0);
}

#[test]
fn raw_fraction_hook_retains_capacity_carries_and_refuses_malformed_inputs() {
    let denominator = power(5303).checked_add(integer(1)).unwrap();
    let numerator = denominator
        .checked_mul(power(64))
        .unwrap()
        .checked_sub(integer(1))
        .unwrap();
    let boundary =
        split_source_coordinate_preserving_fraction(numerator, denominator, -0.0).unwrap();
    assert_eq!(boundary.whole, 1_u128 << 64);
    assert_eq!(boundary.fraction.to_bits(), 0);
    for raw in [-f64::from_bits(1), 1.0, f64::INFINITY, f64::NAN] {
        assert!(split_source_coordinate_preserving_fraction(integer(0), integer(1), raw).is_none());
    }
    assert!(split_source_coordinate_preserving_fraction(integer(1), integer(0), 0.0).is_none());
    assert!(
        split_source_coordinate_preserving_fraction(
            power(64).checked_add(integer(1)).unwrap(),
            integer(1),
            0.0
        )
        .is_none()
    );
}

fn assert_coordinate(
    numerator: SourceInteger,
    denominator: SourceInteger,
    whole: u128,
    fraction_bits: u64,
) {
    let actual = coordinate(numerator, denominator);
    assert_eq!(actual.whole, whole);
    assert_eq!(actual.fraction.to_bits(), fraction_bits);
}

#[test]
fn fixed_storage_has_no_heap_owner_or_drop_and_keeps_6400_bits() {
    fn copy_value<T: Copy>(value: T) -> (T, T) {
        (value, value)
    }
    assert_eq!(Magnitude::BITS, SOURCE_MAGNITUDE_BITS);
    assert_eq!(std::mem::size_of::<Magnitude>(), 800);
    assert!(std::mem::size_of::<SourceInteger>() <= 808);
    assert!(!std::mem::needs_drop::<Magnitude>());
    assert!(!std::mem::needs_drop::<SourceInteger>());
    assert!(!std::mem::needs_drop::<SourceCoordinate>());
    let copied = copy_value(integer(7));
    assert_eq!(copied.0, copied.1);
}

#[test]
fn finite_binary64_grid_covers_every_normal_exponent_and_fraction_edges() {
    let fraction_mask = (1_u64 << 52) - 1;
    for exponent in 1_u64..=2046 {
        for fraction in [0, 1, fraction_mask] {
            let bits = (exponent << 52) | fraction;
            let positive = SourceInteger::from_f64_grid(f64::from_bits(bits)).unwrap();
            let negative =
                SourceInteger::from_f64_grid(f64::from_bits(bits | (1_u64 << 63))).unwrap();
            assert_eq!(negative, positive.negated());
            assert_eq!(positive.checked_add(negative), Some(SourceInteger::ZERO));
            if exponent > 1 {
                // Doubling an exactly represented normal preserves its complete significand.
                let previous_bits = ((exponent - 1) << 52) | fraction;
                let previous = SourceInteger::from_f64_grid(f64::from_bits(previous_bits)).unwrap();
                assert_eq!(previous.checked_shl(1), Some(positive));
            }
        }
    }
    assert_eq!(
        SourceInteger::from_f64_grid(f64::from_bits(1)),
        Some(integer(1))
    );
    assert_eq!(
        SourceInteger::from_f64_grid(f64::from_bits(0x000f_ffff_ffff_ffff)),
        Some(integer((1_u128 << 52) - 1))
    );
    assert_eq!(
        SourceInteger::from_f64_grid(f64::MIN_POSITIVE),
        Some(power(52))
    );
    // MAX = (2^53-1)*2^971; multiplying by G gives 2^2098-2^2045.
    assert_eq!(
        SourceInteger::from_f64_grid(f64::MAX),
        power(2098).checked_sub(power(2045))
    );
    assert_eq!(
        SourceInteger::from_f64_grid(1.0),
        Some(power(SOURCE_GRID_SHIFT))
    );
}

#[test]
fn nonfinite_grid_inputs_refuse_and_numeric_zero_is_canonical() {
    for bits in [
        0x7ff0_0000_0000_0000,
        0xfff0_0000_0000_0000,
        0x7ff0_0000_0000_0001,
        0x7ff8_0000_0000_0000,
        0xfff8_1234_5678_9abc,
    ] {
        assert!(SourceInteger::from_f64_grid(f64::from_bits(bits)).is_none());
    }
    assert_eq!(SourceInteger::from_f64_grid(0.0), Some(SourceInteger::ZERO));
    assert_eq!(
        SourceInteger::from_f64_grid(-0.0),
        Some(SourceInteger::ZERO)
    );
    assert_eq!(SourceInteger::ZERO.negated(), SourceInteger::ZERO);
}

#[test]
fn fused_grid_scale_matches_original_exact_product_across_all_finite_exponents() {
    let factors = [0, 1, 2, 3, 10, 44_100, 48_000, u32::MAX];
    let fraction_mask = (1_u64 << 52) - 1;
    for exponent in 0_u64..=2046 {
        for fraction in [0, 1, fraction_mask] {
            let bits = (exponent << 52) | fraction;
            for factor in factors {
                let expected = original_grid_scaled(f64::from_bits(bits), factor).unwrap();
                for negative in [false, true] {
                    let signed_bits = bits | (u64::from(negative) << 63);
                    let actual =
                        SourceInteger::from_f64_grid_scaled(f64::from_bits(signed_bits), factor);
                    let signed_expected = if negative {
                        expected.negated()
                    } else {
                        expected
                    };
                    assert_eq!(
                        actual,
                        Some(signed_expected),
                        "binary64 bits {signed_bits:016x}, factor {factor}"
                    );
                }
            }
        }
    }
    for bits in [
        0x7ff0_0000_0000_0000,
        0xfff0_0000_0000_0000,
        0x7ff0_0000_0000_0001,
        0x7ff8_0000_0000_0000,
        0xfff8_1234_5678_9abc,
    ] {
        for factor in factors {
            assert!(original_grid_scaled(f64::from_bits(bits), factor).is_none());
            assert!(SourceInteger::from_f64_grid_scaled(f64::from_bits(bits), factor).is_none());
        }
    }
}

#[test]
fn fused_decimal_scale_retains_exact_residual_that_legacy_rounding_erases() {
    // binary64(.1) = 3602879701896397 / 2^55, hence its exact product with 10
    // is 1 + 2^-54. This expected integer is independent of the retained decoder oracle.
    let expected = power(SOURCE_GRID_SHIFT)
        .checked_add(power(SOURCE_GRID_SHIFT - 54))
        .unwrap();
    let exact = SourceInteger::from_f64_grid_scaled(0.1, 10).unwrap();
    assert_eq!(exact, expected);
    assert_eq!(
        SourceInteger::from_f64_grid_scaled(-0.1, 10),
        Some(expected.negated())
    );
    let legacy_rounded = 0.1_f64 * 10.0;
    assert_eq!(legacy_rounded.to_bits(), 1.0_f64.to_bits());
    assert_ne!(exact, SourceInteger::from_f64_grid(legacy_rounded).unwrap());
}

#[test]
fn numerical_source_pair_keeps_large_integer_and_tiny_fraction_separate() {
    let frame = (1_u64 << 53) + 7;
    let tiny = f64::from_bits(1);
    let pair = SourceInteger::from_source_pair_grid(frame, tiny).unwrap();
    assert_coordinate(pair, power(SOURCE_GRID_SHIFT), u128::from(frame), 1);
    let maximum = SourceInteger::from_source_pair_grid(u64::MAX, tiny).unwrap();
    assert_coordinate(maximum, power(SOURCE_GRID_SHIFT), u128::from(u64::MAX), 1);
    let zero_fraction = SourceInteger::from_source_pair_grid(frame, -0.0).unwrap();
    assert_coordinate(
        zero_fraction,
        power(SOURCE_GRID_SHIFT),
        u128::from(frame),
        0,
    );
    for fraction in [-tiny, -1.0, 1.0, f64::NAN, f64::INFINITY] {
        assert!(SourceInteger::from_source_pair_grid(frame, fraction).is_none());
    }
}

#[test]
fn signed_arithmetic_compares_cancels_and_multiplies_exactly() {
    let positive = integer(7);
    let negative = integer(11).negated();
    assert_eq!(positive.checked_add(negative), Some(integer(4).negated()));
    assert_eq!(negative.checked_sub(positive), Some(integer(18).negated()));
    assert_eq!(negative.checked_mul(positive), Some(integer(77).negated()));
    assert_eq!(negative.checked_mul(negative), Some(integer(121)));
    assert_eq!(positive.checked_sub(positive), Some(SourceInteger::ZERO));
    assert_eq!(
        negative.checked_mul(SourceInteger::ZERO),
        Some(SourceInteger::ZERO)
    );
    assert_eq!(negative.cmp(&integer(4).negated()), Ordering::Less);
    assert_eq!(negative.cmp(&SourceInteger::ZERO), Ordering::Less);
    assert_eq!(positive.cmp(&SourceInteger::ZERO), Ordering::Greater);
    assert_eq!(negative.partial_cmp(&negative), Some(Ordering::Equal));
}

#[test]
fn multiplication_shortcuts_match_original_bnum_in_both_orders_and_all_signs() {
    let values = [
        SourceInteger::ZERO,
        integer(1),
        integer(2),
        integer(3),
        integer(10),
        integer(u128::MAX),
        power(63),
        power(64),
        power(1074),
        power(3199),
        power(3200),
        power(6398),
        power(6399),
        power(6330).checked_add(integer(17)).unwrap(),
        SourceInteger::canonical(Magnitude::MAX, false),
    ];
    for (left_index, left) in values.into_iter().enumerate() {
        for (right_index, right) in values.into_iter().enumerate() {
            let original = left.magnitude.checked_mul(right.magnitude);
            for left_negative in [false, true] {
                for right_negative in [false, true] {
                    let left = if left_negative { left.negated() } else { left };
                    let right = if right_negative {
                        right.negated()
                    } else {
                        right
                    };
                    let expected = original.map(|magnitude| {
                        SourceInteger::canonical(magnitude, left_negative != right_negative)
                    });
                    assert_eq!(
                        left.checked_mul(right),
                        expected,
                        "operands {left_index}/{right_index}, signs {left_negative}/{right_negative}"
                    );
                }
            }
        }
    }
    // The largest exact single-bit product is admitted; the next bit still refuses.
    assert_eq!(power(3199).checked_mul(power(3200)), Some(power(6399)));
    assert!(power(3200).checked_mul(power(3200)).is_none());
}

fn assert_original_signed_products(left: SourceInteger, right: SourceInteger) {
    for (left, right) in [(left, right), (right, left)] {
        let original = left.magnitude.checked_mul(right.magnitude);
        for left_negative in [false, true] {
            for right_negative in [false, true] {
                let left = if left_negative { left.negated() } else { left };
                let right = if right_negative {
                    right.negated()
                } else {
                    right
                };
                let expected = original.map(|magnitude| {
                    SourceInteger::canonical(magnitude, left_negative != right_negative)
                });
                let (actual, allocations) = observe_current_thread(|| left.checked_mul(right));
                assert_eq!(actual, expected);
                assert_eq!(allocations, ThreadAllocations::default());
            }
        }
    }
}

#[test]
fn scaled_odd_coefficients_reconstruct_losslessly_across_native_word_boundaries() {
    for (left, left_shift, right, right_shift) in [
        (3, 0, 5, 0),
        (3, 31, 5, 32),
        ((1_u128 << 32) - 1, 63, (1_u128 << 32) + 1, 64),
        ((1_u128 << 63) + 1, 64, 3, 65),
        ((1_u128 << 64) - 1, 127, (1_u128 << 64) + 1, 128),
        ((1_u128 << 65) + 1, 129, 3, 1020),
        ((1_u128 << 63) - 1, 1074, (1_u128 << 63) + 1, 2045),
        ((1_u128 << 53) - 1, 1074, 375, 1074),
        // The native coefficient product is exactly u128::MAX. Shifting it by
        // 6272 places its highest bit at index 6399, preserving every bit at full capacity.
        ((1_u128 << 64) - 1, 3100, (1_u128 << 64) + 1, 3172),
    ] {
        assert!(left & 1 != 0 && right & 1 != 0);
        assert!(left.checked_mul(right).is_some());
        let left = integer(left).checked_shl(left_shift).unwrap();
        let right = integer(right).checked_shl(right_shift).unwrap();
        assert_original_signed_products(left, right);
    }
    let boundary_left = integer(3).checked_shl(3198).unwrap();
    let boundary_right = integer(5).checked_shl(3198).unwrap();
    let exact = boundary_left.checked_mul(boundary_right).unwrap();
    assert_eq!(exact, integer(15).checked_shl(6396).unwrap());
    assert_eq!(exact.magnitude.bit_width(), SOURCE_MAGNITUDE_BITS);
    let overflow_right = integer(5).checked_shl(3199).unwrap();
    assert!(boundary_left.checked_mul(overflow_right).is_none());
    assert_original_signed_products(boundary_left, boundary_right);
    assert_original_signed_products(boundary_left, overflow_right);
    let native_boundary_left = integer((1_u128 << 64) - 1).checked_shl(3100).unwrap();
    let native_boundary_right = integer((1_u128 << 64) + 1).checked_shl(3172).unwrap();
    let native_boundary = native_boundary_left
        .checked_mul(native_boundary_right)
        .unwrap();
    assert_eq!(
        native_boundary,
        integer(u128::MAX).checked_shl(6272).unwrap()
    );
    let native_overflow_right = integer((1_u128 << 64) + 1).checked_shl(3173).unwrap();
    assert!(
        native_boundary_left
            .checked_mul(native_overflow_right)
            .is_none()
    );
    assert_original_signed_products(native_boundary_left, native_overflow_right);
    // Even tiny coefficients cannot rescue an out-of-capacity combined scale.
    assert_original_signed_products(
        integer(3).checked_shl(3300).unwrap(),
        integer(5).checked_shl(3300).unwrap(),
    );
    assert_original_signed_products(
        integer(3).checked_shl(3300).unwrap(),
        integer(7).checked_shl(3100).unwrap(),
    );
}

#[test]
fn scaled_odd_products_keep_wide_fallback_and_capacity_refusals() {
    // Native coefficient overflow is only a fast-path miss, not an arithmetic refusal.
    let native_overflow_left = integer(u128::MAX).checked_shl(1074).unwrap();
    let native_overflow_right = integer(3).checked_shl(128).unwrap();
    assert!(
        native_overflow_left
            .checked_mul(native_overflow_right)
            .is_some()
    );
    assert_original_signed_products(native_overflow_left, native_overflow_right);
    assert_original_signed_products(integer(u128::MAX), integer(u128::MAX));
    let native_overflow_left = integer((1_u128 << 127) + 1).checked_shl(1074).unwrap();
    let native_overflow_right = integer(3).checked_shl(1074).unwrap();
    let wide = native_overflow_left
        .checked_mul(native_overflow_right)
        .unwrap();
    assert_eq!(wide.magnitude.bit_width(), 2277);
    assert_original_signed_products(native_overflow_left, native_overflow_right);

    let odd_129 = power(128).checked_add(integer(1)).unwrap();
    assert_original_signed_products(
        odd_129.checked_shl(13).unwrap(),
        integer(3).checked_shl(29).unwrap(),
    );
    assert_original_signed_products(
        odd_129.checked_shl(1074).unwrap(),
        integer(3).checked_shl(64).unwrap(),
    );
    // 1 + minsub has an odd 1075-bit grid coefficient; it must retain the wide path.
    let minsub_pair = SourceInteger::from_source_pair_grid(1, f64::from_bits(1)).unwrap();
    assert_eq!(minsub_pair, power(1074).checked_add(integer(1)).unwrap());
    assert_original_signed_products(minsub_pair, integer(3).checked_shl(64).unwrap());
    let scaled_375 = integer(375).checked_shl(1074).unwrap();
    assert_eq!(
        minsub_pair
            .checked_mul(scaled_375)
            .unwrap()
            .magnitude
            .bit_width(),
        2157
    );
    assert_original_signed_products(minsub_pair, scaled_375);
    let huge_odd = power(6330).checked_add(integer(17)).unwrap();
    assert_original_signed_products(huge_odd, integer(3));
    let dense = power(6398).checked_sub(integer(1)).unwrap();
    assert!(dense.checked_mul(integer(3)).is_some());
    assert!(dense.checked_mul(integer(5)).is_none());
    assert_original_signed_products(dense, integer(3));
    assert_original_signed_products(dense, integer(5));
    assert_original_signed_products(SourceInteger::canonical(Magnitude::MAX, false), integer(3));
}

#[test]
fn euclidean_division_floors_negative_inputs_without_cycle_narrowing() {
    for (value, divisor, quotient, remainder) in
        [(7, 3, -3, 2), (6, 3, -2, 0), (1, 2, -1, 1), (0, 3, 0, 0)]
    {
        let input = integer(value).negated();
        let (actual_q, actual_r) = input.div_rem_euclid(integer(divisor)).unwrap();
        let expected_q = integer(i128::unsigned_abs(quotient));
        let expected_q = if quotient < 0 {
            expected_q.negated()
        } else {
            expected_q
        };
        assert_eq!(actual_q, expected_q);
        assert_eq!(actual_r, integer(remainder));
        assert_eq!(
            actual_q
                .checked_mul(integer(divisor))
                .unwrap()
                .checked_add(actual_r),
            Some(input)
        );
    }
    let input = power(6330).checked_add(integer(17)).unwrap().negated();
    let (quotient, remainder) = input.div_rem_euclid(integer(3)).unwrap();
    assert!(quotient.magnitude.bit_width() > u64::BITS);
    assert!(remainder >= SourceInteger::ZERO && remainder < integer(3));
    assert_eq!(
        quotient
            .checked_mul(integer(3))
            .unwrap()
            .checked_add(remainder),
        Some(input)
    );
    assert!(input.div_rem_euclid(SourceInteger::ZERO).is_none());
    assert!(input.div_rem_euclid(integer(3).negated()).is_none());
}

#[test]
fn quotient_free_remainder_matches_original_division_at_signed_full_width_edges() {
    let maximum = SourceInteger::canonical(Magnitude::MAX, false);
    let numerators = [
        SourceInteger::ZERO,
        integer(1),
        integer(2),
        integer(6),
        integer(7),
        integer(11),
        power(6330).checked_add(integer(17)).unwrap(),
        power(6399),
        maximum,
    ];
    let denominators = [
        SourceInteger::ZERO,
        integer(1),
        integer(2),
        integer(3),
        integer(7),
        power(1074),
        power(6330).checked_add(integer(19)).unwrap(),
        power(6399),
        maximum.checked_sub(integer(1)).unwrap(),
        maximum,
    ];
    for numerator in numerators {
        for denominator in denominators {
            for numerator_negative in [false, true] {
                for denominator_negative in [false, true] {
                    let numerator = if numerator_negative {
                        numerator.negated()
                    } else {
                        numerator
                    };
                    let denominator = if denominator_negative {
                        denominator.negated()
                    } else {
                        denominator
                    };
                    let expected = original_div_rem_euclid(numerator, denominator);
                    assert_eq!(numerator.div_rem_euclid(denominator), expected);
                    let actual = numerator.rem_euclid(denominator);
                    assert_eq!(actual, expected.map(|(_, remainder)| remainder));
                    if let Some(remainder) = actual {
                        assert!(remainder >= SourceInteger::ZERO && remainder < denominator);
                    }
                }
            }
        }
    }
    assert!(
        SourceInteger::ZERO
            .rem_euclid(SourceInteger::ZERO)
            .is_none()
    );
    assert_eq!(
        maximum.negated().rem_euclid(maximum),
        Some(SourceInteger::ZERO)
    );
    assert_eq!(
        integer(1).negated().rem_euclid(maximum),
        maximum.checked_sub(integer(1))
    );
}

#[test]
fn lossless_shift_and_capacity_exhaustion_leave_original_values_unchanged() {
    let top = power(6399);
    let before = top;
    assert!(top.checked_shl(1).is_none());
    assert!(integer(1).checked_shl(6400).is_none());
    assert!(SourceInteger::ZERO.checked_shl(6400).is_none());
    assert_eq!(
        SourceInteger::ZERO.checked_shl(6399),
        Some(SourceInteger::ZERO)
    );
    assert!(top.checked_add(top).is_none());
    assert!(top.checked_mul(integer(2)).is_none());
    assert_eq!(top, before);
    assert_eq!(top.negated().checked_shl(0), Some(top.negated()));
    let maximum = SourceInteger::canonical(Magnitude::MAX, false);
    assert!(maximum.checked_add(integer(1)).is_none());
    assert_eq!(maximum.negated().negated(), maximum);
    assert!(magnitude_u128(top.magnitude).is_none());
}

#[test]
fn rational_normal_rounding_uses_exact_residual_and_ties_to_even() {
    for (numerator, denominator, whole, fraction_bits) in [
        (1, 3, 0, 0x3fd5_5555_5555_5555),
        (2, 3, 0, 0x3fe5_5555_5555_5555),
        (1, 10, 0, 0x3fb9_9999_9999_999a),
        (3, 10, 0, 0x3fd3_3333_3333_3333),
        (7, 2, 3, 0x3fe0_0000_0000_0000),
        ((1_u128 << 53) + 1, 1_u128 << 54, 0, 0x3fe0_0000_0000_0000),
        ((1_u128 << 53) + 3, 1_u128 << 54, 0, 0x3fe0_0000_0000_0002),
    ] {
        assert_coordinate(
            integer(numerator),
            integer(denominator),
            whole,
            fraction_bits,
        );
    }
    assert_coordinate(integer((1_u128 << 54) - 1), power(54), 1, 0);
}

#[test]
fn rational_subnormal_rounding_covers_halfway_zero_and_normal_carry() {
    assert_coordinate(integer(1), power(1075), 0, 0);
    assert_coordinate(integer(3), power(1075), 0, 2);
    assert_coordinate(integer(5), power(1075), 0, 2);
    assert_coordinate(integer(1), power(1076), 0, 0);
    assert_coordinate(integer(3), power(1076), 0, 1);
    assert_coordinate(integer(1), power(2000), 0, 0);
    assert_coordinate(
        integer((1_u128 << 53) - 1),
        power(1075),
        0,
        0x0010_0000_0000_0000,
    );
    assert_coordinate(
        integer((1_u128 << 54) - 3),
        power(1076),
        0,
        0x000f_ffff_ffff_ffff,
    );
}

#[test]
fn rational_source_endpoint_stays_u128_until_caller_normalization() {
    let large = (1_u128 << 53) + 7;
    assert_coordinate(integer(large), integer(1), large, 0);
    assert_coordinate(
        integer(u128::from(u64::MAX)),
        integer(1),
        u128::from(u64::MAX),
        0,
    );
    assert_coordinate(
        integer(SOURCE_WHOLE_LIMIT),
        integer(1),
        SOURCE_WHOLE_LIMIT,
        0,
    );
    // 2^64 - 2^-54 has floor u64::MAX, then fractional RN carries to 2^64.
    assert_coordinate(
        power(118).checked_sub(integer(1)).unwrap(),
        power(54),
        SOURCE_WHOLE_LIMIT,
        0,
    );
    // Exact 2^64 + the smallest binary64 quantum must refuse before RN hides it.
    let beyond = power(1138).checked_add(integer(1)).unwrap();
    assert!(split_source_coordinate(beyond, power(1074)).is_none());
    assert!(split_source_coordinate(integer(SOURCE_WHOLE_LIMIT + 1), integer(1)).is_none());
}

#[test]
fn rational_domain_and_explicit_core_capacity_failures_refuse() {
    assert!(split_source_coordinate(integer(1).negated(), integer(3)).is_none());
    assert!(split_source_coordinate(integer(1), integer(3).negated()).is_none());
    assert!(split_source_coordinate(integer(1), SourceInteger::ZERO).is_none());
    assert!(split_source_coordinate(power(6363), power(5303)).is_none());
    assert!(split_source_coordinate(SourceInteger::ZERO, power(5304)).is_none());
    assert_coordinate(SourceInteger::ZERO, power(5303), 0, 0);
}

#[test]
fn full_width_denominators_keep_subnormal_and_near_one_decisions_exact() {
    let denominator = power(5303);
    assert_coordinate(power(4229), denominator, 0, 1);
    assert_coordinate(
        denominator.checked_sub(integer(1)).unwrap(),
        denominator,
        1,
        0,
    );
    let exact_boundary = denominator.checked_shl(64).unwrap();
    assert_coordinate(exact_boundary, denominator, SOURCE_WHOLE_LIMIT, 0);
    let outside = exact_boundary.checked_add(integer(1)).unwrap();
    assert!(split_source_coordinate(outside, denominator).is_none());
}

#[test]
fn represented_fraction_bits_round_trip_without_an_absolute_float_coordinate() {
    let denominator = power(SOURCE_GRID_SHIFT);
    for bits in [
        0,
        1,
        2,
        0x000f_ffff_ffff_ffff,
        0x0010_0000_0000_0000,
        0x0010_0000_0000_0001,
        0x0100_0000_0000_0001,
        0x3c90_0000_0000_0000,
        0x3fcf_ffff_ffff_ffff,
        0x3fe0_0000_0000_0001,
        0x3fef_ffff_ffff_ffff,
    ] {
        let numerator = SourceInteger::from_f64_grid(f64::from_bits(bits)).unwrap();
        assert_coordinate(numerator, denominator, 0, bits);
    }
}

#[test]
fn independent_fraction_oracle_vectors() {
    // Independent Python stdlib Fraction/struct results retained by Root at
    // test-artifacts/bounded-source-numerics-20261023/fraction-oracle.md.
    // Fixtures below form exact integers only; expected f64 bits are copied from that oracle.
    let p53 = 1_u128 << 53;
    let p54 = 1_u128 << 54;
    let p63 = 1_u128 << 63;
    let p64 = SOURCE_WHOLE_LIMIT;
    let large_denominator = power(5303).checked_add(integer(1)).unwrap();
    let decimal_denominator = power(5300).checked_mul(integer(10)).unwrap();
    let vectors = [
        (
            integer(4 * (p53 + 1) + 1),
            integer(4),
            p53 + 1,
            0x3fd0_0000_0000_0000,
        ),
        (
            integer(4 * (p64 - 1) + 3),
            integer(4),
            p64 - 1,
            0x3fe8_0000_0000_0000,
        ),
        (integer((1_u128 << 118) - 1), power(54), p64, 0),
        (integer(p53 + 1), power(54), 0, 0x3fe0_0000_0000_0000),
        (integer(p53 + 3), power(54), 0, 0x3fe0_0000_0000_0002),
        (integer(p54 - 1), power(54), 1, 0),
        (integer(p54 - 1), power(55), 0, 0x3fe0_0000_0000_0000),
        (integer(1), power(1075), 0, 0),
        (integer(3), power(1075), 0, 2),
        (integer(5), power(1076), 0, 1),
        (integer(p53 - 1), power(1075), 0, 0x0010_0000_0000_0000),
        (integer(1), power(1076), 0, 0),
        (
            large_denominator
                .checked_shl(64)
                .unwrap()
                .checked_sub(integer(1))
                .unwrap(),
            large_denominator,
            p64,
            0,
        ),
        (integer(1), integer(10), 0, 0x3fb9_9999_9999_999a),
        (
            integer(10 * (p63 + 123) + 7),
            integer(10),
            p63 + 123,
            0x3fe6_6666_6666_6666,
        ),
        (integer(p64), integer(1), p64, 0),
        (integer(0), integer(1), 0, 0),
        (integer(1), power(1074), 0, 1),
        (
            integer(3 * (p53 + 5) + 1),
            integer(3),
            p53 + 5,
            0x3fd5_5555_5555_5555,
        ),
        (
            decimal_denominator
                .checked_mul(integer(p63 + 17))
                .unwrap()
                .checked_add(power(5300))
                .unwrap()
                .checked_add(integer(1))
                .unwrap(),
            decimal_denominator,
            p63 + 17,
            0x3fb9_9999_9999_999a,
        ),
    ];
    for (index, (numerator, denominator, whole, fraction_bits)) in vectors.into_iter().enumerate() {
        let actual = coordinate(numerator, denominator);
        assert_eq!(actual.whole, whole, "oracle case {}", index + 1);
        assert_eq!(
            actual.fraction.to_bits(),
            fraction_bits,
            "oracle case {}",
            index + 1
        );
    }
    assert!(split_source_coordinate(power(6362), large_denominator).is_none());
}

#[test]
fn numerical_operations_request_zero_rust_allocations_on_observed_thread() {
    let denominator = power(5303);
    let numerator = denominator.checked_sub(integer(1)).unwrap();
    let signed = power(6330).checked_add(integer(17)).unwrap().negated();
    let three = integer(3);
    let (actual, allocations) = observe_current_thread(|| {
        let decoded = SourceInteger::from_f64_grid_scaled(f64::MAX, u32::MAX)?;
        let cancelled = decoded.checked_add(decoded.negated())?;
        let divided = signed.div_rem_euclid(three)?;
        let remainder = signed.rem_euclid(three)?;
        let rounded = split_source_coordinate(numerator, denominator)?;
        let tiny = SourceInteger::from_source_pair_grid((1_u64 << 53) + 7, f64::from_bits(1))?;
        Some((cancelled, divided, remainder, rounded, tiny))
    });
    let (cancelled, divided, remainder, rounded, tiny) =
        actual.expect("all measured operations admitted");
    assert_eq!(cancelled, SourceInteger::ZERO);
    assert!(divided.1 >= SourceInteger::ZERO && divided.1 < three);
    assert_eq!(remainder, divided.1);
    assert_eq!(rounded.whole, 1);
    assert_eq!(rounded.fraction.to_bits(), 0);
    assert!(tiny > SourceInteger::ZERO);
    assert_eq!(allocations, ThreadAllocations::default());
}
