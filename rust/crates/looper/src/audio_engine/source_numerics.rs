//! Checked exact arithmetic for a future source-coordinate proof, not an audio permit.
//!
//! The fixed magnitude has no heap owner. Values are returned before callers mutate anything;
//! no wide value is added to a playback span or lane. The operation count is bounded, but actual
//! stack use and callback throughput have not been qualified. Geometry, raw-fraction identity,
//! source/reader ownership and productive conversion remain the caller's separate obligations.

use std::cmp::Ordering;

type Magnitude = bnum::Uint<800>;

pub(super) const SOURCE_MAGNITUDE_BITS: u32 = 6400;
pub(super) const SOURCE_GRID_SHIFT: u32 = 1074;
const SOURCE_NUMERATOR_BITS: u32 = 6363;
const SOURCE_DENOMINATOR_BITS: u32 = 5304;
const SOURCE_WHOLE_LIMIT: u128 = 1_u128 << 64;
const FRACTION_MASK: u64 = (1_u64 << 52) - 1;
const FRACTION_ONE_BITS: u64 = 0x3ff0_0000_0000_0000;

/// Exact signed integer with canonical numeric zero and 6400 magnitude bits.
///
/// Grid decoding interprets this integer as `value * 2^1074`; arithmetic itself is ordinary
/// integer arithmetic, so products keep their explicit caller-owned scale. Raw `-0` identity
/// cannot be inferred from this canonical numeric representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SourceInteger {
    negative: bool,
    magnitude: Magnitude,
}

impl SourceInteger {
    pub(super) const ZERO: Self = Self {
        negative: false,
        magnitude: Magnitude::MIN,
    };

    fn canonical(magnitude: Magnitude, negative: bool) -> Self {
        Self {
            negative: negative && !magnitude.is_zero(),
            magnitude,
        }
    }

    /// Construct an unscaled integer, independently of the binary64 grid convention.
    pub(super) fn from_integer(value: u128) -> Option<Self> {
        Some(Self::canonical(Magnitude::try_from(value).ok()?, false))
    }

    /// Decode every finite binary64 exactly onto the integer grid `G = 2^1074`.
    pub(super) fn from_f64_grid(value: f64) -> Option<Self> {
        Self::from_f64_grid_scaled(value, 1)
    }

    /// Decode `value * factor` onto G without an intermediate rounded binary64 product.
    pub(super) fn from_f64_grid_scaled(value: f64, factor: u32) -> Option<Self> {
        let bits = value.to_bits();
        let exponent = u32::try_from((bits >> 52) & 0x7ff).ok()?;
        if exponent == 0x7ff {
            return None;
        }
        // Keep the finite guard first: even a zero factor must refuse NaN and infinity.
        if factor == 0 {
            return Some(Self::ZERO);
        }
        let fraction = bits & FRACTION_MASK;
        let (significand, shift) = if exponent == 0 {
            (fraction, 0)
        } else {
            (fraction | (1_u64 << 52), exponent.checked_sub(1)?)
        };
        // At most 53 + 32 bits before the grid shift; no precision or capacity is lost.
        let significand = u128::from(significand).checked_mul(u128::from(factor))?;
        let magnitude = lossless_shl(Magnitude::try_from(significand).ok()?, shift)?;
        Some(Self::canonical(magnitude, bits >> 63 != 0))
    }

    /// Construct only the numerical pair `frame + fraction` on G, with no source admission.
    pub(super) fn from_source_pair_grid(frame: u64, fraction: f64) -> Option<Self> {
        if !fraction.is_finite() || !(0.0..1.0).contains(&fraction) {
            return None;
        }
        Self::from_integer(u128::from(frame))?
            .checked_shl(SOURCE_GRID_SHIFT)?
            .checked_add(Self::from_f64_grid(fraction)?)
    }

    pub(super) fn checked_add(self, other: Self) -> Option<Self> {
        if self.negative == other.negative {
            return Some(Self::canonical(
                self.magnitude.checked_add(other.magnitude)?,
                self.negative,
            ));
        }
        let (larger, smaller) = if self.magnitude >= other.magnitude {
            (self, other)
        } else {
            (other, self)
        };
        Some(Self::canonical(
            larger.magnitude.checked_sub(smaller.magnitude)?,
            larger.negative,
        ))
    }

    pub(super) fn checked_sub(self, other: Self) -> Option<Self> {
        self.checked_add(other.negated())
    }

    pub(super) fn checked_mul(self, other: Self) -> Option<Self> {
        Some(Self::canonical(
            checked_magnitude_mul(self.magnitude, other.magnitude)?,
            self.negative != other.negative,
        ))
    }

    /// Sign-magnitude negation is total; there is no asymmetric signed minimum.
    pub(super) fn negated(self) -> Self {
        Self::canonical(self.magnitude, !self.negative)
    }

    /// Refuse both an out-of-range count and loss of any high magnitude bit.
    pub(super) fn checked_shl(self, shift: u32) -> Option<Self> {
        Some(Self::canonical(
            lossless_shl(self.magnitude, shift)?,
            self.negative,
        ))
    }

    /// Exact nonnegative remainder for a strictly positive denominator, without a quotient.
    pub(super) fn rem_euclid(self, denominator: Self) -> Option<Self> {
        if denominator.negative || denominator.magnitude.is_zero() {
            return None;
        }
        let remainder = self.magnitude.checked_rem(denominator.magnitude)?;
        let remainder = if self.negative && !remainder.is_zero() {
            denominator.magnitude.checked_sub(remainder)?
        } else {
            remainder
        };
        Some(Self::canonical(remainder, false))
    }

    /// Exact floor quotient and nonnegative remainder for a strictly positive denominator.
    pub(super) fn div_rem_euclid(self, denominator: Self) -> Option<(Self, Self)> {
        if denominator.negative || denominator.magnitude.is_zero() {
            return None;
        }
        let (quotient, remainder) = unsigned_div_rem(self.magnitude, denominator.magnitude)?;
        if self.negative && !remainder.is_zero() {
            let quotient = quotient.checked_add(Magnitude::from(true))?;
            let remainder = denominator.magnitude.checked_sub(remainder)?;
            Some((
                Self::canonical(quotient, true),
                Self::canonical(remainder, false),
            ))
        } else {
            Some((
                Self::canonical(quotient, self.negative),
                Self::canonical(remainder, false),
            ))
        }
    }
}

impl Ord for SourceInteger {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (true, true) => other.magnitude.cmp(&self.magnitude),
            (false, false) => self.magnitude.cmp(&other.magnitude),
        }
    }
}

impl PartialOrd for SourceInteger {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Separately rounded coordinates, retaining the virtual `2^64` boundary before address casts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SourceCoordinate {
    pub(super) whole: u128,
    pub(super) fraction: f64,
}

/// Split a nonnegative exact rational source coordinate with one RN of its fraction.
///
/// The admitted core bounds are `N < 2^6363`, `0 < D < 2^5304`, and exact `N/D <= 2^64`.
/// An exact boundary plus a nonzero fraction refuses before rounding. A fractional value below
/// the boundary may round up to `2^64`; callers must normalize their declared loop before any
/// physical address cast. Numeric zero is `+0`; raw anchor identity remains outside this API.
pub(super) fn split_source_coordinate(
    numerator: SourceInteger,
    denominator: SourceInteger,
) -> Option<SourceCoordinate> {
    split_source_coordinate_impl(numerator, denominator, None)
}

/// The same one-round split, retaining raw fraction bits on exact fractional identity.
///
/// Both `r*G` and `I(raw_fraction)*D` need at most 6378 bits under the existing
/// denominator bound. The full numerator is never shifted. This grants no anchor permission.
pub(super) fn split_source_coordinate_preserving_fraction(
    numerator: SourceInteger,
    denominator: SourceInteger,
    raw_fraction: f64,
) -> Option<SourceCoordinate> {
    if !raw_fraction.is_finite() || !(0.0..1.0).contains(&raw_fraction) {
        return None;
    }
    split_source_coordinate_impl(numerator, denominator, Some(raw_fraction))
}

fn split_source_coordinate_impl(
    numerator: SourceInteger,
    denominator: SourceInteger,
    raw_fraction: Option<f64>,
) -> Option<SourceCoordinate> {
    if numerator.negative
        || denominator.negative
        || denominator.magnitude.is_zero()
        || numerator.magnitude.bit_width() > SOURCE_NUMERATOR_BITS
        || denominator.magnitude.bit_width() > SOURCE_DENOMINATOR_BITS
    {
        return None;
    }
    let (whole, remainder) = unsigned_div_rem(numerator.magnitude, denominator.magnitude)?;
    let mut whole = magnitude_u128(whole)?;
    if whole > SOURCE_WHOLE_LIMIT || (whole == SOURCE_WHOLE_LIMIT && !remainder.is_zero()) {
        return None;
    }
    if let Some(raw) = raw_fraction {
        let identical = if raw == 0.0 {
            remainder.is_zero()
        } else {
            lossless_shl(remainder, SOURCE_GRID_SHIFT)?
                == checked_magnitude_mul(
                    SourceInteger::from_f64_grid(raw)?.magnitude,
                    denominator.magnitude,
                )?
        };
        if identical {
            return Some(SourceCoordinate {
                whole,
                fraction: raw,
            });
        }
    }
    let fraction_bits = if remainder.is_zero() {
        0
    } else {
        rounded_fraction_bits(remainder, denominator.magnitude)?
    };
    let fraction = if fraction_bits == FRACTION_ONE_BITS {
        whole = whole.checked_add(1)?;
        if whole > SOURCE_WHOLE_LIMIT {
            return None;
        }
        0.0
    } else {
        f64::from_bits(fraction_bits)
    };
    Some(SourceCoordinate { whole, fraction })
}

fn lossless_shl(value: Magnitude, shift: u32) -> Option<Magnitude> {
    if shift >= SOURCE_MAGNITUDE_BITS
        || value.bit_width().checked_add(shift)? > SOURCE_MAGNITUDE_BITS
    {
        return None;
    }
    value.checked_shl(shift)
}

/// Exact common factors avoid the fixed-width generic multiply without changing overflow.
fn checked_magnitude_mul(left: Magnitude, right: Magnitude) -> Option<Magnitude> {
    if left.is_zero() || right.is_zero() {
        return Some(Magnitude::MIN);
    }
    let one = Magnitude::from(true);
    if left == one {
        return Some(right);
    }
    if right == one {
        return Some(left);
    }
    if left.is_power_of_two() {
        return lossless_shl(right, left.trailing_zeros());
    }
    if right.is_power_of_two() {
        return lossless_shl(left, right.trailing_zeros());
    }
    left.checked_mul(right)
}

/// Bnum has no inherent fused checked div/rem: use one division and the exact qD residual.
fn unsigned_div_rem(
    numerator: Magnitude,
    denominator: Magnitude,
) -> Option<(Magnitude, Magnitude)> {
    let quotient = numerator.checked_div(denominator)?;
    let remainder = numerator.checked_sub(checked_magnitude_mul(quotient, denominator)?)?;
    Some((quotient, remainder))
}

fn magnitude_u128(value: Magnitude) -> Option<u128> {
    u128::try_from(value).ok()
}

/// Correct RN ties-to-even for `0 < remainder/denominator < 1`, including gradual underflow.
fn rounded_fraction_bits(remainder: Magnitude, denominator: Magnitude) -> Option<u64> {
    let remainder_bits = i32::try_from(remainder.bit_width()).ok()?;
    let denominator_bits = i32::try_from(denominator.bit_width()).ok()?;
    let mut exponent = remainder_bits.checked_sub(denominator_bits)?;
    // The bit-length estimate differs from floor(log2(r/D)) by at most one.
    let below_estimate = if exponent >= 0 {
        remainder < lossless_shl(denominator, u32::try_from(exponent).ok()?)?
    } else {
        lossless_shl(remainder, u32::try_from(exponent.checked_neg()?).ok()?)? < denominator
    };
    if below_estimate {
        exponent = exponent.checked_sub(1)?;
    }
    if exponent < -1075 {
        return Some(0);
    }
    let subnormal = exponent < -1022;
    let shift = if subnormal {
        SOURCE_GRID_SHIFT
    } else {
        u32::try_from(52_i32.checked_sub(exponent)?).ok()?
    };
    // D has at most 5304 bits and the shift is at most 1074: even the untrimmed
    // 6378-bit numerator is below the fixed 6400-bit capacity. No floating quotient.
    let scaled = lossless_shl(remainder, shift)?;
    let (mantissa, residual) = unsigned_div_rem(scaled, denominator)?;
    let mut mantissa = magnitude_u128(mantissa)?;
    let twice_residual = lossless_shl(residual, 1)?;
    if twice_residual > denominator || (twice_residual == denominator && mantissa & 1 != 0) {
        mantissa = mantissa.checked_add(1)?;
    }
    if subnormal {
        // A carry to bit 52 encodes the smallest normal directly.
        if mantissa > 1_u128 << 52 {
            return None;
        }
        return u64::try_from(mantissa).ok();
    }
    if mantissa == 1_u128 << 53 {
        mantissa >>= 1;
        exponent = exponent.checked_add(1)?;
    }
    if !((1_u128 << 52)..(1_u128 << 53)).contains(&mantissa) || !(-1022..=0).contains(&exponent) {
        return None;
    }
    let biased = u64::try_from(exponent.checked_add(1023)?).ok()?;
    Some((biased << 52) | (u64::try_from(mantissa).ok()? & FRACTION_MASK))
}

#[cfg(test)]
#[path = "source_numerics_tests.rs"]
mod tests;
