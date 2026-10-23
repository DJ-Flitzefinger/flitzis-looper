use super::{
    Magnitude, SOURCE_GRID_SHIFT, SOURCE_MAGNITUDE_BITS, SOURCE_WHOLE_LIMIT, SourceCoordinate,
    SourceInteger, magnitude_u128, split_source_coordinate,
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
        let decoded = SourceInteger::from_f64_grid(f64::MAX)?;
        let cancelled = decoded.checked_add(decoded.negated())?;
        let divided = signed.div_rem_euclid(three)?;
        let rounded = split_source_coordinate(numerator, denominator)?;
        let tiny = SourceInteger::from_source_pair_grid((1_u64 << 53) + 7, f64::from_bits(1))?;
        Some((cancelled, divided, rounded, tiny))
    });
    let (cancelled, divided, rounded, tiny) = actual.expect("all measured operations admitted");
    assert_eq!(cancelled, SourceInteger::ZERO);
    assert!(divided.1 >= SourceInteger::ZERO && divided.1 < three);
    assert_eq!(rounded.whole, 1);
    assert_eq!(rounded.fraction.to_bits(), 0);
    assert!(tiny > SourceInteger::ZERO);
    assert_eq!(allocations, ThreadAllocations::default());
}
