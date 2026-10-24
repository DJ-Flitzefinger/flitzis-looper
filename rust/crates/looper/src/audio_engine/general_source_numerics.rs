//! Exact bounded arithmetic for the separately qualified general reader domain.
//! Copied certificates are 64 bytes; all larger values are ephemeral non-owning scratch.
//! General queries admit N<2^386,D<2^380 and produce fractions with LSB>=-432.
//! Cross-products with a reduced256-bit period need at most753bits for a whole/fraction
//! pair. Final-interval alpha uses at most690/689 numerator/denominator bits; binary32
//! subnormal RN adds at most149 numerator bits. Thus1024-bit scratch is sufficient.
use std::cmp::Ordering;

use super::compact_source_numerics::{Grid, decode_f64_at_grid, exact_f64_parts};
use super::source_numerics::SourceCoordinate;
use super::source_reader::FrameRange;

pub(super) type Magnitude = bnum::Uint<64>;
type Wide = bnum::Uint<128>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Integer {
    pub(super) negative: bool,
    pub(super) magnitude: Magnitude,
}

impl Integer {
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
    pub(super) fn from_i128(value: i128) -> Option<Self> {
        Some(Self::canonical(
            Magnitude::try_from(value.unsigned_abs()).ok()?,
            value < 0,
        ))
    }
    pub(super) fn from_u128(value: u128) -> Option<Self> {
        Some(Self::canonical(Magnitude::try_from(value).ok()?, false))
    }
    pub(super) fn add(self, other: Self) -> Option<Self> {
        if self.negative == other.negative {
            return Some(Self::canonical(
                self.magnitude.checked_add(other.magnitude)?,
                self.negative,
            ));
        }
        let (large, small) = if self.magnitude >= other.magnitude {
            (self, other)
        } else {
            (other, self)
        };
        Some(Self::canonical(
            large.magnitude.checked_sub(small.magnitude)?,
            large.negative,
        ))
    }
    pub(super) fn sub(self, other: Self) -> Option<Self> {
        self.add(Self::canonical(other.magnitude, !other.negative))
    }
    pub(super) fn mul(self, other: Self) -> Option<Self> {
        Some(Self::canonical(
            self.magnitude.checked_mul(other.magnitude)?,
            self.negative != other.negative,
        ))
    }
    pub(super) fn shl(self, shift: u32) -> Option<Self> {
        Some(Self::canonical(shl(self.magnitude, shift)?, self.negative))
    }
    pub(super) fn quotient(self, other: Self) -> Option<Self> {
        if self.negative || other.negative || other.magnitude.is_zero() {
            return None;
        }
        Some(Self::canonical(
            self.magnitude.checked_div(other.magnitude)?,
            false,
        ))
    }
    pub(super) fn u128(self) -> Option<u128> {
        (!self.negative).then_some(())?;
        u128::try_from(self.magnitude).ok()
    }
}
impl Ord for Integer {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (true, true) => other.magnitude.cmp(&self.magnitude),
            (false, false) => self.magnitude.cmp(&other.magnitude),
        }
    }
}
impl PartialOrd for Integer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn shl<const N: usize>(v: bnum::Uint<N>, shift: u32) -> Option<bnum::Uint<N>> {
    if shift >= bnum::Uint::<N>::BITS || v.bit_width().checked_add(shift)? > bnum::Uint::<N>::BITS {
        return None;
    }
    v.checked_shl(shift)
}
fn wide(value: Magnitude) -> Wide {
    let mut bytes = [0; 128];
    bytes[..64].copy_from_slice(value.as_bytes());
    Wide::from_bytes(bytes)
}
fn stored(value: [u8; 32]) -> Wide {
    let mut bytes = [0; 128];
    bytes[..32].copy_from_slice(&value);
    Wide::from_bytes(bytes)
}
fn dyadic(value: f64) -> Option<(i128, i16)> {
    exact_f64_parts(value)
}

/// Positive reduced N/D. The private fields prevent unchecked copied certificates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MappedReaderPeriod {
    numerator: [u8; 32],
    denominator: [u8; 32],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ExactMappedTaps {
    pub(crate) left: usize,
    pub(crate) right: Option<usize>,
    pub(crate) right_gain: f32,
}

impl MappedReaderPeriod {
    /// Display metadata only; exact admission and addressing never use this value.
    pub(super) fn approximate(self) -> f64 {
        let convert = |bytes: [u8; 32]| {
            bytes
                .iter()
                .rev()
                .fold(0.0, |value, byte| value * 256.0 + f64::from(*byte))
        };
        convert(self.numerator) / convert(self.denominator)
    }
    /// Owner-side reduction is deliberately outside per-frame queries.
    pub(super) fn new(numerator: Integer, denominator: Integer) -> Option<Self> {
        if numerator <= Integer::ZERO || denominator <= Integer::ZERO {
            return None;
        }
        let (mut a, mut b) = (numerator.magnitude, denominator.magnitude);
        while !b.is_zero() {
            let r = a.checked_rem(b)?;
            a = b;
            b = r;
        }
        let n = numerator.magnitude.checked_div(a)?;
        let d = denominator.magnitude.checked_div(a)?;
        if n.bit_width() > 256 || d.bit_width() > 256 {
            return None;
        }
        let mut numerator = [0; 32];
        numerator.copy_from_slice(&n.as_bytes()[..32]);
        let mut denominator = [0; 32];
        denominator.copy_from_slice(&d.as_bytes()[..32]);
        Some(Self {
            numerator,
            denominator,
        })
    }
    pub(crate) fn validate_physical(self, region: FrameRange) -> bool {
        self.validate(region).is_some()
    }
    fn validate(self, region: FrameRange) -> Option<()> {
        if region.start >= region.end {
            return None;
        }
        let n = stored(self.numerator);
        let d = stored(self.denominator);
        if n.is_zero() || d.is_zero() || shl(n, 1)? < d {
            return None;
        }
        let h = Wide::try_from(region.len()).ok()?.checked_mul(d)?;
        let difference = if n >= h {
            n.checked_sub(h)?
        } else {
            h.checked_sub(n)?
        };
        if difference > d {
            return None;
        }
        let end = Wide::try_from(region.start)
            .ok()?
            .checked_mul(d)?
            .checked_add(n)?;
        (end <= shl(d, 64)?).then_some(())
    }
    /// Separate q/f membership, without a rounded whole-plus-fraction operation.
    pub(crate) fn canonical_pair(self, frame: usize, fraction: f64, start: usize) -> bool {
        self.pair_order(u128::try_from(frame).ok(), fraction, start)
            .is_some_and(|o| o == Ordering::Less)
    }
    fn pair_order(self, frame: Option<u128>, fraction: f64, start: usize) -> Option<Ordering> {
        let frame = frame?;
        if frame < start as u128
            || frame > (1_u128 << 64)
            || !fraction.is_finite()
            || !(0.0..1.0).contains(&fraction)
        {
            return None;
        }
        let n = stored(self.numerator);
        let d = stored(self.denominator);
        let whole = Wide::try_from(frame.checked_sub(start as u128)?)
            .ok()?
            .checked_mul(d)?;
        if whole > n {
            return Some(Ordering::Greater);
        }
        let residual = n.checked_sub(whole)?;
        if residual >= d {
            return Some(Ordering::Less);
        }
        let (f, exponent) = dyadic(fraction)?;
        if f < 0 {
            return None;
        }
        let left = Wide::try_from(f).ok()?.checked_mul(d)?;
        if left.is_zero() || residual.is_zero() {
            return Some(left.cmp(&residual));
        }
        let lt = left.bit_width() as i32 + i32::from(exponent);
        let rt = residual.bit_width() as i32;
        if lt != rt {
            return Some(lt.cmp(&rt));
        }
        if exponent >= 0 {
            Some(shl(left, exponent as u32)?.cmp(&residual))
        } else {
            Some(left.cmp(&shl(residual, (-exponent) as u32)?))
        }
    }

    /// Exact-inside is checked before the sole RN split and rational-boundary carry.
    pub(super) fn split(
        self,
        numerator: Integer,
        denominator: Integer,
        raw_fraction: f64,
        start: usize,
    ) -> Option<SourceCoordinate> {
        if numerator.negative
            || denominator <= Integer::ZERO
            || numerator.magnitude.bit_width() > 386
            || denominator.magnitude.bit_width() > 380
        {
            return None;
        }
        let n = wide(numerator.magnitude);
        let d = wide(denominator.magnitude);
        let a = Wide::try_from(start).ok()?.checked_mul(d)?;
        if n < a {
            return None;
        }
        let relative = n.checked_sub(a)?;
        if relative.checked_mul(stored(self.denominator))?
            >= d.checked_mul(stored(self.numerator))?
        {
            return None;
        }
        let whole = n.checked_div(d)?;
        let remainder = n.checked_rem(d)?;
        let mut whole = u128::try_from(whole).ok()?;
        if whole > (1_u128 << 64) {
            return None;
        }
        let (raw, raw_exp) = dyadic(raw_fraction)?;
        if raw < 0
            || !raw_fraction.is_finite()
            || !(0.0..1.0).contains(&raw_fraction)
            || raw_exp < -126
        {
            return None;
        }
        let raw_shift = u32::try_from(-raw_exp.min(0)).ok()?;
        let same = shl(remainder, raw_shift)? == Wide::try_from(raw).ok()?.checked_mul(d)?;
        let fraction = if same {
            raw_fraction
        } else if remainder.is_zero() {
            0.0
        } else {
            let bits = round_fraction(remainder, d, 52, -1022, 1023)?;
            if bits == 0x3ff0_0000_0000_0000 {
                whole = whole.checked_add(1)?;
                0.0
            } else {
                f64::from_bits(bits)
            }
        };
        if self.pair_order(Some(whole), fraction, start)? != Ordering::Less {
            return Some(SourceCoordinate {
                whole: start as u128,
                fraction: 0.0,
            });
        }
        Some(SourceCoordinate { whole, fraction })
    }
    pub(crate) fn prepare(self, region: FrameRange) -> Option<PreparedMappedReaderPeriod> {
        self.validate(region)?;
        let n = stored(self.numerator);
        let d = stored(self.denominator);
        let physical = n == Wide::try_from(region.len()).ok()?.checked_mul(d)?;
        let quotient = n.checked_div(d)?;
        let remainder = n.checked_rem(d)?;
        let ceil = quotient.checked_add(Wide::try_from(u8::from(!remainder.is_zero())).ok()?)?;
        let last = ceil
            .checked_sub(Wide::try_from(1_u8).ok()?)?
            .min(Wide::try_from(region.len() - 1).ok()?);
        let last = usize::try_from(last).ok()?;
        let width = n.checked_sub(Wide::try_from(last).ok()?.checked_mul(d)?)?;
        Some(PreparedMappedReaderPeriod {
            period: self,
            region,
            last,
            denominator: d,
            width,
            physical,
        })
    }
    pub(crate) fn seam_taps(
        self,
        frame: usize,
        fraction: f64,
        region: FrameRange,
    ) -> Option<ExactMappedTaps> {
        self.prepare(region)?.seam_taps(frame, fraction)
    }
    pub(super) fn exact_marker_end(self, start: usize) -> Option<u128> {
        let n = stored(self.numerator);
        let d = stored(self.denominator);
        let q = n.checked_div(d)?;
        let r = n.checked_rem(d)?;
        let increment = shl(r, 1)? >= d; // Positive marker ties are away from zero.
        (start as u128)
            .checked_add(u128::try_from(q).ok()?)?
            .checked_add(u128::from(increment))
    }
}

/// Ephemeral validated reader constants prepared once per full/checked chunk.
/// Interior taps need neither a rational division nor a large comparison.
#[derive(Clone, Copy)]
pub(crate) struct PreparedMappedReaderPeriod {
    period: MappedReaderPeriod,
    region: FrameRange,
    last: usize,
    denominator: Wide,
    width: Wide,
    physical: bool,
}
impl PreparedMappedReaderPeriod {
    pub(crate) fn seam_taps(&self, frame: usize, fraction: f64) -> Option<ExactMappedTaps> {
        if !fraction.is_finite() || !(0.0..1.0).contains(&fraction) {
            return None;
        }
        let relative = frame.checked_sub(self.region.start)?;
        if relative < self.last {
            return Some(ExactMappedTaps {
                left: frame,
                right: (fraction != 0.0).then_some(frame.checked_add(1)?),
                right_gain: fraction as f32,
            });
        }
        if !self
            .period
            .canonical_pair(frame, fraction, self.region.start)
        {
            return None;
        }
        if self.physical {
            let left = self.region.start + relative % self.region.len();
            return Some(ExactMappedTaps {
                left,
                right: (fraction != 0.0).then_some(
                    self.region.start + (left - self.region.start + 1) % self.region.len(),
                ),
                right_gain: fraction as f32,
            });
        }
        let (f, exponent) = dyadic(fraction)?;
        if exponent < -432 {
            return None;
        }
        let shift = u32::try_from(-exponent.min(0)).ok()?;
        let delta = shl(Wide::try_from(relative - self.last).ok()?, shift)?
            .checked_add(Wide::try_from(f).ok()?)?;
        let alpha_n = delta.checked_mul(self.denominator)?;
        let alpha_d = shl(self.width, shift)?;
        let bits = round_fraction(alpha_n, alpha_d, 23, -126, 127)?;
        Some(ExactMappedTaps {
            left: self.region.start + self.last,
            right: Some(self.region.start),
            right_gain: f32::from_bits(u32::try_from(bits).ok()?),
        })
    }
}

/// RN-even for positive ratios in [0,1], including f32 subnormal ties. At most839
/// significant scratch bits are needed by admitted seam inputs; storage is1024bits.
fn round_fraction(
    n: Wide,
    d: Wide,
    mantissa_bits: i32,
    minimum_normal: i32,
    bias: i32,
) -> Option<u64> {
    if d.is_zero() || n > d {
        return None;
    }
    if n.is_zero() {
        return Some(0);
    }
    let mut exponent = n.bit_width() as i32 - d.bit_width() as i32;
    let below_power = if exponent >= 0 {
        n < shl(d, exponent as u32)?
    } else {
        shl(n, (-exponent) as u32)? < d
    };
    if below_power {
        exponent -= 1;
    }
    let effective = exponent.max(minimum_normal);
    let scaled = shl(n, u32::try_from(mantissa_bits - effective).ok()?)?;
    let mut q = scaled.checked_div(d)?;
    let remainder = scaled.checked_rem(d)?;
    let twice = shl(remainder, 1)?;
    let q_value = u64::try_from(q).ok()?;
    if twice > d || (twice == d && q_value & 1 != 0) {
        q = q.checked_add(Wide::try_from(1_u8).ok()?)?;
    }
    let mut q = u64::try_from(q).ok()?;
    if exponent < minimum_normal && q < (1_u64 << mantissa_bits) {
        return Some(q);
    }
    let mut output_exp = effective;
    if q == 1_u64 << (mantissa_bits + 1) {
        q >>= 1;
        output_exp += 1;
    }
    Some(((output_exp + bias) as u64) << mantissa_bits | (q & ((1_u64 << mantissa_bits) - 1)))
}

pub(super) fn on_grid(value: f64, grid: Grid) -> Option<Integer> {
    Integer::from_i128(decode_f64_at_grid(value, grid)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn period(n: u128, d: u128) -> MappedReaderPeriod {
        MappedReaderPeriod::new(
            Integer::from_u128(n).unwrap(),
            Integer::from_u128(d).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn exact_period_reader_preserves_interior_and_computes_rational_seam() {
        let p = period(31, 10);
        let region = FrameRange { start: 1, end: 4 };
        assert!(p.validate_physical(region));
        let tap = p.seam_taps(4, 0.0625, region).unwrap();
        assert_eq!((tap.left, tap.right), (3, Some(1)));
        // Independently: (3+1/16-2)/(31/10-2)=85/88.
        assert_eq!(tap.right_gain.to_bits(), (85.0_f32 / 88.0).to_bits());
        assert!(!p.canonical_pair(4, 0.125, 1));
        assert_eq!(std::mem::size_of::<MappedReaderPeriod>(), 64);
    }
    #[test]
    fn exact_period_fraction_rounds_ties_even_and_accepts_physical_equivalence() {
        let p = period(3, 1);
        let r = FrameRange { start: 1, end: 4 };
        assert_eq!(p.seam_taps(3, 0.0, r).unwrap().right, None);
        let d = Wide::try_from(1_u128 << 25).unwrap();
        assert_eq!(
            round_fraction(
                Wide::try_from((1_u128 << 24) + 1).unwrap(),
                d,
                23,
                -126,
                127
            ),
            Some(0.5_f32.to_bits() as u64)
        );
        let zero_half = shl(Wide::try_from(1_u8).unwrap(), 150).unwrap();
        assert_eq!(
            round_fraction(Wide::try_from(1_u8).unwrap(), zero_half, 23, -126, 127),
            Some(0)
        );
    }

    #[test]
    fn rational_boundary_carry_requires_exact_inside_and_preserves_raw_zero_identity() {
        let p = period(31, 10);
        // 41/10-2^-60 is inside, but its separately rounded fractional part
        // is binary64(.1), which lies just beyond the exact rational boundary.
        let n = Integer::from_u128((41_u128 << 60) - 10).unwrap();
        let d = Integer::from_u128(10_u128 << 60).unwrap();
        let carried = p.split(n, d, 0.0, 1).unwrap();
        assert_eq!((carried.whole, carried.fraction.to_bits()), (1, 0));
        assert!(!p.canonical_pair(4, 0.1, 1));
        assert!(
            p.split(
                Integer::from_u128(41).unwrap(),
                Integer::from_u128(10).unwrap(),
                0.0,
                1
            )
            .is_none()
        );
        let raw = p
            .split(
                Integer::from_u128(1).unwrap(),
                Integer::from_u128(1).unwrap(),
                -0.0,
                1,
            )
            .unwrap();
        assert_eq!(raw.fraction.to_bits(), (-0.0_f64).to_bits());
    }

    #[test]
    fn certificate_reduction_capacity_and_subnormal_seam_gain_are_exact() {
        let one = Integer::from_u128(1).unwrap();
        let large = one.shl(300).unwrap();
        assert!(MappedReaderPeriod::new(large, one).is_none());
        assert_eq!(MappedReaderPeriod::new(large, large), Some(period(1, 1)));
        let taps = period(5, 2)
            .seam_taps(2, 2_f64.powi(-150), FrameRange { start: 0, end: 3 })
            .unwrap();
        assert_eq!(taps.right_gain.to_bits(), 1);
        let denominator = shl(Wide::try_from(1_u8).unwrap(), 150).unwrap();
        assert_eq!(
            round_fraction(Wide::try_from(3_u8).unwrap(), denominator, 23, -126, 127),
            Some(2)
        );
    }
    #[test]
    fn virtual_whole_two_to_sixty_four_is_normalized_before_address_cast() {
        let p = period(2, 1);
        let start = usize::MAX - 1;
        assert!(p.validate_physical(FrameRange {
            start,
            end: usize::MAX
        }));
        let n = Integer::from_u128((1_u128 << 124) - 1).unwrap();
        let d = Integer::from_u128(1_u128 << 60).unwrap();
        let result = p.split(n, d, 0.0, start).unwrap();
        assert_eq!((result.whole, result.fraction), (start as u128, 0.0));
    }
    #[test]
    fn virtual_period_ceiling_is_clamped_before_physical_knot_cast() {
        let p = period(1_u128 << 64, 1);
        let region = FrameRange {
            start: 0,
            end: usize::MAX,
        };
        assert!(p.validate_physical(region));
        let taps = p.seam_taps(usize::MAX, 0.5, region).unwrap();
        assert_eq!(
            (taps.left, taps.right, taps.right_gain),
            (usize::MAX - 1, Some(0), 0.75)
        );
    }
}
