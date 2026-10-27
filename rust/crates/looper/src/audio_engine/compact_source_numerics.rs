//! Lossless small arithmetic for an explicit compact-only anchored query.
//!
//! This is not a replacement for the general 6400-bit reference. The caller must retain
//! its original geometry/domain/epoch qualification. Coefficients have magnitude below
//! 2^126 on grids in [-126, 0]; every original intermediate then fits its preceding bounds.
//! Scratch is local, has no heap owner, and grants no callback, source or adoption permit.
//!
//! With axis differences below 2^127, C is below 2^255, unchecked I below 2^256,
//! admitted P below 2^253 and U below 2^255. Forward N is below 2^383 and its
//! effective denominator below 2^380. Restoring the original G factors bounds
//! original phase intermediates by 2403 bits and original N/D by 3605/3476 bits,
//! below the unchanged 6400-bit algebra and 6363/5304-bit splitter limits. Thus
//! later cancellation cannot rescue any original overflowing intermediate here.

use std::cmp::Ordering;

use super::source_numerics::SourceCoordinate;

const COEFFICIENT_BITS: u32 = 126;
const MIN_GRID_EXPONENT: i16 = -126;
const NORMALIZE_EXPONENT: i16 = -432;
const WHOLE_LIMIT: u128 = 1_u128 << 64;
const FRACTION_MASK: u64 = (1_u64 << 52) - 1;
const FRACTION_ONE_BITS: u64 = 0x3ff0_0000_0000_0000;
type CoordinateMagnitude = bnum::Uint<64>;

#[derive(Clone, Copy, Debug)]
struct FiniteDyadic {
    negative: bool,
    coefficient: u128,
    exponent: i16,
}

impl FiniteDyadic {
    fn scaled(value: f64, factor: u32) -> Option<Self> {
        let bits = value.to_bits();
        let encoded = i16::try_from((bits >> 52) & 0x7ff).ok()?;
        // In particular, zero scaling must not conceal a nonfinite input.
        if encoded == 0x7ff {
            return None;
        }
        let fraction = bits & FRACTION_MASK;
        let (significand, exponent) = if encoded == 0 {
            (fraction, -1074)
        } else {
            (fraction | (1_u64 << 52), encoded - 1075)
        };
        let coefficient = u128::from(significand).checked_mul(u128::from(factor))?;
        if coefficient == 0 {
            return Some(Self {
                negative: false,
                coefficient: 0,
                exponent: 0,
            });
        }
        let zeros = coefficient.trailing_zeros();
        Some(Self {
            negative: bits >> 63 != 0,
            coefficient: coefficient >> zeros,
            exponent: exponent.checked_add(i16::try_from(zeros).ok()?)?,
        })
    }

    fn top(self) -> i16 {
        self.exponent + (u128::BITS - self.coefficient.leading_zeros()) as i16
    }
}

/// Constant-size statistics from the existing off-realtime original-segment scan.
#[derive(Clone, Copy, Debug)]
pub(super) struct GridStats {
    minimum_exponent: i16,
    maximum_top: Option<i16>,
}

impl GridStats {
    /// Extend existing admitted bounds without losing any original coefficient.
    pub(super) fn include_base(self, base: BaseGrid) -> Self {
        Self {
            minimum_exponent: self.minimum_exponent.min(base.exponent),
            maximum_top: Some(
                self.maximum_top
                    .map_or(base.maximum_top(), |top| top.max(base.maximum_top())),
            ),
        }
    }

    pub(super) const fn new() -> Self {
        Self {
            minimum_exponent: 0,
            maximum_top: None,
        }
    }

    fn include(self, value: FiniteDyadic) -> Self {
        if value.coefficient == 0 {
            return self;
        }
        Self {
            minimum_exponent: self.minimum_exponent.min(value.exponent),
            maximum_top: Some(
                self.maximum_top
                    .map_or(value.top(), |top| top.max(value.top())),
            ),
        }
    }

    pub(super) fn include_f64(self, value: f64) -> Option<Self> {
        self.include_scaled_f64(value, 1)
    }

    pub(super) fn include_scaled_f64(self, value: f64, factor: u32) -> Option<Self> {
        Some(self.include(FiniteDyadic::scaled(value, factor)?))
    }

    pub(super) fn include_u64(self, value: u64) -> Option<Self> {
        if value == 0 {
            return Some(self);
        }
        let zeros = value.trailing_zeros();
        Some(self.include(FiniteDyadic {
            negative: false,
            coefficient: u128::from(value >> zeros),
            exponent: i16::try_from(zeros).ok()?,
        }))
    }

    pub(super) fn finish(self) -> Option<BaseGrid> {
        let bits = self
            .maximum_top
            .map_or(0, |top| top - self.minimum_exponent);
        if self.minimum_exponent < MIN_GRID_EXPONENT || bits > COEFFICIENT_BITS as i16 {
            return None;
        }
        Some(BaseGrid {
            exponent: self.minimum_exponent,
            maximum_bits: u16::try_from(bits).ok()?,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct BaseGrid {
    exponent: i16,
    maximum_bits: u16,
}

impl BaseGrid {
    pub(super) fn maximum_top(self) -> i16 {
        self.exponent + self.maximum_bits as i16
    }

    pub(super) fn recovery_exponent(self, maximum_frame: u64) -> Option<i16> {
        let whole_top = if maximum_frame == 0 {
            0
        } else {
            (64 - maximum_frame.leading_zeros()) as i16
        };
        let exponent = (-126).max(self.maximum_top().max(whole_top) - 126);
        self.refine(exponent)?;
        Some(exponent)
    }
    /// Refine the shared static grid, retaining headroom for every observed original value.
    pub(super) fn refine(self, required_exponent: i16) -> Option<Grid> {
        let exponent = self.exponent.min(required_exponent).min(0);
        let shift = self.exponent.checked_sub(exponent)?;
        if exponent < MIN_GRID_EXPONENT
            || i32::from(self.maximum_bits) + i32::from(shift) > COEFFICIENT_BITS as i32
        {
            return None;
        }
        Some(Grid { exponent })
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Grid {
    exponent: i16,
}

impl Grid {
    /// Reconstruct only an already certified original grid, never a wider class.
    pub(super) fn from_admitted_exponent(exponent: i16) -> Option<Self> {
        (-126..=0).contains(&exponent).then_some(Self { exponent })
    }
    pub(super) fn exponent(self) -> i16 {
        self.exponent
    }
}

/// Coarsest necessary integer-or-finer grid; numeric zero places no finer requirement.
pub(super) fn required_grid_f64(value: f64) -> Option<i16> {
    Some(FiniteDyadic::scaled(value, 1)?.exponent.min(0))
}

/// Finite normalized signed significand and unrestricted dyadic exponent. This
/// does not assert compact-grid admission of the value itself (notably FPB).
pub(super) fn exact_f64_parts(value: f64) -> Option<(i128, i16)> {
    let dyadic = FiniteDyadic::scaled(value, 1)?;
    let coefficient = i128::try_from(dyadic.coefficient).ok()?;
    Some((
        if dyadic.negative {
            -coefficient
        } else {
            coefficient
        },
        dyadic.exponent,
    ))
}

/// Grid of every finite result of the unchanged cast/subtract/divide/add evaluator.
/// Endpoint evaluations bound magnitudes only, never this interior precision requirement.
pub(super) fn master_grid_exponent(actual_fpb: f64, original_anchor_beat: f64) -> Option<i16> {
    if actual_fpb <= 0.0 {
        return None;
    }
    let divisor = FiniteDyadic::scaled(actual_fpb, 1)?;
    let floor_log2 = divisor.top().checked_sub(1)?;
    let quotient_grid = (-1074).max(floor_log2.checked_neg()?.checked_sub(53)?);
    Some(
        quotient_grid
            .min(required_grid_f64(original_anchor_beat)?)
            .min(0),
    )
}

fn native_shl(value: u128, shift: u32) -> Option<u128> {
    if shift >= u128::BITS || (u128::BITS - value.leading_zeros()).checked_add(shift)? > u128::BITS
    {
        return None;
    }
    value.checked_shl(shift)
}

fn coefficient_at_grid(value: FiniteDyadic, grid: Grid) -> Option<i128> {
    if value.coefficient == 0 {
        return Some(0);
    }
    // The significand is odd: a finer dyadic cannot be an integer on this grid.
    let shift = u32::try_from(value.exponent.checked_sub(grid.exponent)?).ok()?;
    let magnitude = native_shl(value.coefficient, shift)?;
    if magnitude >= 1_u128 << COEFFICIENT_BITS {
        return None;
    }
    let coefficient = i128::try_from(magnitude).ok()?;
    Some(if value.negative {
        -coefficient
    } else {
        coefficient
    })
}

pub(super) fn decode_f64_at_grid(value: f64, grid: Grid) -> Option<i128> {
    decode_scaled_f64_at_grid(value, 1, grid)
}

pub(super) fn decode_scaled_f64_at_grid(value: f64, factor: u32, grid: Grid) -> Option<i128> {
    coefficient_at_grid(FiniteDyadic::scaled(value, factor)?, grid)
}

pub(super) fn decode_source_pair_at_grid(frame: u64, fraction: f64, grid: Grid) -> Option<i128> {
    if !fraction.is_finite() || !(0.0..1.0).contains(&fraction) {
        return None;
    }
    let whole = native_shl(u128::from(frame), u32::try_from(-grid.exponent).ok()?)?;
    let fractional = u128::try_from(decode_f64_at_grid(fraction, grid)?).ok()?;
    let coefficient = whole.checked_add(fractional)?;
    if coefficient >= 1_u128 << COEFFICIENT_BITS {
        return None;
    }
    i128::try_from(coefficient).ok()
}

/// Exact ordering even for remote finite probes that are not on the compact loop grid.
pub(super) fn compare_scaled_f64_to_grid(
    value: f64,
    factor: u32,
    coefficient: i128,
    grid: Grid,
) -> Option<Ordering> {
    let probe = FiniteDyadic::scaled(value, factor)?;
    let target = coefficient.unsigned_abs();
    if target >= 1_u128 << COEFFICIENT_BITS {
        return None;
    }
    let target_negative = coefficient < 0;
    if probe.negative != target_negative {
        return Some(if probe.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        });
    }
    let ordering = if probe.coefficient == 0 || target == 0 {
        probe.coefficient.cmp(&target)
    } else {
        let target_top = grid.exponent + (u128::BITS - target.leading_zeros()) as i16;
        match probe.top().cmp(&target_top) {
            Ordering::Equal => {
                // Equal top exponents bound the aligned width by max(85,126).
                let difference = probe.exponent - grid.exponent;
                if difference >= 0 {
                    native_shl(probe.coefficient, u32::try_from(difference).ok()?)?.cmp(&target)
                } else {
                    probe
                        .coefficient
                        .cmp(&native_shl(target, u32::try_from(-difference).ok()?)?)
                }
            }
            ordering => ordering,
        }
    };
    Some(if probe.negative {
        ordering.reverse()
    } else {
        ordering
    })
}

/// Private-magnitude checked sign arithmetic. Numeric zero is always canonical.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SmallInteger<const N: usize> {
    negative: bool,
    magnitude: bnum::Uint<N>,
}

pub(super) type PhaseInteger = SmallInteger<32>;
pub(super) type CoordinateInteger = SmallInteger<64>;

impl<const N: usize> SmallInteger<N> {
    pub(super) const ZERO: Self = Self {
        negative: false,
        magnitude: bnum::Uint::<N>::MIN,
    };

    fn canonical(magnitude: bnum::Uint<N>, negative: bool) -> Self {
        Self {
            negative: negative && !magnitude.is_zero(),
            magnitude,
        }
    }

    pub(super) fn from_i128(value: i128) -> Option<Self> {
        Some(Self::canonical(
            bnum::Uint::<N>::try_from(value.unsigned_abs()).ok()?,
            value < 0,
        ))
    }

    pub(super) fn is_zero(self) -> bool {
        self.magnitude.is_zero()
    }
    pub(super) fn is_negative(self) -> bool {
        self.negative
    }
    pub(super) fn magnitude_bits(self) -> u32 {
        self.magnitude.bit_width()
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
        self.checked_add(Self::canonical(other.magnitude, !other.negative))
    }

    pub(super) fn checked_mul_i128(self, other: i128) -> Option<Self> {
        Some(Self::canonical(
            self.magnitude
                .checked_mul(bnum::Uint::<N>::try_from(other.unsigned_abs()).ok()?)?,
            self.negative != (other < 0),
        ))
    }

    pub(super) fn checked_shl(self, shift: u32) -> Option<Self> {
        Some(Self::canonical(
            lossless_shl(self.magnitude, shift)?,
            self.negative,
        ))
    }
}

impl PhaseInteger {
    pub(super) fn rem_euclid(self, denominator: Self) -> Option<Self> {
        if denominator.negative || denominator.is_zero() {
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

    pub(super) fn widen(self) -> CoordinateInteger {
        let mut bytes = [0; 64];
        bytes[..32].copy_from_slice(self.magnitude.as_bytes());
        CoordinateInteger::canonical(CoordinateMagnitude::from_bytes(bytes), self.negative)
    }
}

impl<const N: usize> Ord for SmallInteger<N> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (true, true) => other.magnitude.cmp(&self.magnitude),
            (false, false) => self.magnitude.cmp(&other.magnitude),
        }
    }
}

impl<const N: usize> PartialOrd for SmallInteger<N> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn lossless_shl<const N: usize>(value: bnum::Uint<N>, shift: u32) -> Option<bnum::Uint<N>> {
    if shift >= bnum::Uint::<N>::BITS
        || value.bit_width().checked_add(shift)? > bnum::Uint::<N>::BITS
    {
        return None;
    }
    value.checked_shl(shift)
}

fn unsigned_div_rem(
    numerator: CoordinateMagnitude,
    denominator: CoordinateMagnitude,
) -> Option<(CoordinateMagnitude, CoordinateMagnitude)> {
    let quotient = numerator.checked_div(denominator)?;
    let remainder = numerator.checked_sub(quotient.checked_mul(denominator)?)?;
    Some((quotient, remainder))
}

fn validate_domain(grid: Grid, start: u64, period: f64) -> Option<()> {
    if !period.is_finite() || !(0.5..=WHOLE_LIMIT as f64).contains(&period) {
        return None;
    }
    decode_source_pair_at_grid(start, 0.0, grid)?;
    decode_f64_at_grid(period, grid)?;
    Some(())
}

/// Split the effective rational, preserve raw fractional identity, then normalize the
/// original domain. No broad-reference fallback or physical address conversion occurs.
pub(super) fn split_normalized_source_coordinate(
    numerator: CoordinateInteger,
    effective_denominator: CoordinateInteger,
    source_grid: Grid,
    raw_fraction: f64,
    original_start: u64,
    original_period: f64,
) -> Option<SourceCoordinate> {
    validate_domain(source_grid, original_start, original_period)?;
    if numerator.negative
        || effective_denominator.negative
        || effective_denominator.is_zero()
        || numerator.magnitude_bits() > 383
        || effective_denominator.magnitude_bits() > 380
        || !raw_fraction.is_finite()
        || !(0.0..1.0).contains(&raw_fraction)
    {
        return None;
    }
    let raw_coefficient = u128::try_from(decode_f64_at_grid(raw_fraction, source_grid)?).ok()?;
    let denominator = effective_denominator.magnitude;
    let (whole, remainder) = unsigned_div_rem(numerator.magnitude, denominator)?;
    let mut whole = u128::try_from(whole).ok()?;
    if whole > WHOLE_LIMIT || (whole == WHOLE_LIMIT && !remainder.is_zero()) {
        return None;
    }
    let identical = lossless_shl(remainder, u32::try_from(-source_grid.exponent).ok()?)?
        == CoordinateMagnitude::try_from(raw_coefficient)
            .ok()?
            .checked_mul(denominator)?;
    let fraction = if identical {
        raw_fraction
    } else if remainder.is_zero() {
        0.0
    } else {
        let bits = rounded_fraction_bits(remainder, denominator)?;
        if bits == FRACTION_ONE_BITS {
            whole = whole.checked_add(1)?;
            if whole > WHOLE_LIMIT {
                return None;
            }
            0.0
        } else {
            f64::from_bits(bits)
        }
    };
    normalize_source_coordinate(
        SourceCoordinate { whole, fraction },
        source_grid,
        original_start,
        original_period,
    )
}

fn rounded_fraction_bits(
    remainder: CoordinateMagnitude,
    denominator: CoordinateMagnitude,
) -> Option<u64> {
    let mut exponent =
        i32::try_from(remainder.bit_width()).ok()? - i32::try_from(denominator.bit_width()).ok()?;
    let below = if exponent >= 0 {
        remainder < lossless_shl(denominator, u32::try_from(exponent).ok()?)?
    } else {
        lossless_shl(remainder, u32::try_from(-exponent).ok()?)? < denominator
    };
    if below {
        exponent -= 1;
    }
    // D<2^380 and integer r>=1 prove a normal fraction with exponent at least -380.
    if !(-380..=0).contains(&exponent) {
        return None;
    }
    let scaled = lossless_shl(remainder, u32::try_from(52 - exponent).ok()?)?;
    let (mantissa, residual) = unsigned_div_rem(scaled, denominator)?;
    let mut mantissa = u128::try_from(mantissa).ok()?;
    let twice = lossless_shl(residual, 1)?;
    if twice > denominator || (twice == denominator && mantissa & 1 != 0) {
        mantissa += 1;
    }
    if mantissa == 1_u128 << 53 {
        mantissa >>= 1;
        exponent += 1;
    }
    if !((1_u128 << 52)..(1_u128 << 53)).contains(&mantissa) || !(-380..=0).contains(&exponent) {
        return None;
    }
    Some(
        (u64::try_from(exponent + 1023).ok()? << 52)
            | (u64::try_from(mantissa).ok()? & FRACTION_MASK),
    )
}

fn on_normalization_grid(value: FiniteDyadic) -> Option<CoordinateMagnitude> {
    if value.negative {
        return None;
    }
    if value.coefficient == 0 {
        return Some(CoordinateMagnitude::MIN);
    }
    let shift = u32::try_from(value.exponent.checked_sub(NORMALIZE_EXPONENT)?).ok()?;
    lossless_shl(
        CoordinateMagnitude::try_from(value.coefficient).ok()?,
        shift,
    )
}

/// Output fractions may be finer than the raw source grid. Their proved minimum LSB
/// is -432; exact virtual Whole=2^64 normalization needs at most497 bits, not496.
pub(super) fn normalize_source_coordinate(
    coordinate: SourceCoordinate,
    source_grid: Grid,
    original_start: u64,
    original_period: f64,
) -> Option<SourceCoordinate> {
    validate_domain(source_grid, original_start, original_period)?;
    if coordinate.whole > WHOLE_LIMIT
        || !coordinate.fraction.is_finite()
        || !(0.0..1.0).contains(&coordinate.fraction)
    {
        return None;
    }
    let start = lossless_shl(CoordinateMagnitude::try_from(original_start).ok()?, 432)?;
    let endpoint = start.checked_add(on_normalization_grid(FiniteDyadic::scaled(
        original_period,
        1,
    )?)?)?;
    let value =
        lossless_shl(CoordinateMagnitude::try_from(coordinate.whole).ok()?, 432)?.checked_add(
            on_normalization_grid(FiniteDyadic::scaled(coordinate.fraction, 1)?)?,
        )?;
    if value == endpoint {
        return Some(SourceCoordinate {
            whole: u128::from(original_start),
            fraction: 0.0,
        });
    }
    if value < start || value >= endpoint {
        return None;
    }
    Some(coordinate)
}

#[cfg(test)]
#[path = "compact_source_numerics_tests.rs"]
mod tests;
