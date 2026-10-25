//! Exact bounded arithmetic for the separately qualified general reader domain.
//! Copied certificates are 64 bytes; all larger values are ephemeral non-owning scratch.
//! Declared-Q queries admit N<2^386,D<2^380 and produce fractions with LSB>=-432.
//! Their cross-products with a reduced256-bit period need at most753bits for a whole/fraction
//! pair. Final-interval alpha uses at most690/689 numerator/denominator bits; binary32
//! subnormal RN adds at most149 numerator bits. Thus1024-bit scratch is sufficient.
//! Physical-Q has a separate1024-bit type and642/636 splitter theorem, LSB>=-688,
//! raw identity<=762bits, physical-boundary comparison<=700bits and RN numerator<=689bits.
//! Its source-frame period is the integer marker length, independent of beat-Q.
use std::cmp::Ordering;

use super::compact_source_numerics::{Grid, decode_f64_at_grid, exact_f64_parts};
use super::source_numerics::SourceCoordinate;
use super::source_reader::{FrameRange, SourceLoopDomain};
use super::source_reader_period::SourceReaderPeriod;

pub(super) type Magnitude = bnum::Uint<64>;
type Wide = bnum::Uint<128>;

pub(super) type Integer = super::bounded_source_integer::BoundedInteger<64>;

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

/// Exact positive quarter extent of an original physical marker interval.
/// These bytes are beats, not a source-frame reader period. Reduction is OFFRT.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalMapQuarters {
    numerator: [u8; 32],
    denominator: [u8; 32],
}

impl PhysicalMapQuarters {
    pub(super) fn new(numerator: Integer, denominator: Integer) -> Option<Self> {
        let reduced = MappedReaderPeriod::new(numerator, denominator)?;
        Some(Self {
            numerator: reduced.numerator,
            denominator: reduced.denominator,
        })
    }

    /// The opaque constructor already established reduction. No runtime GCD.
    pub(crate) fn is_valid(self) -> bool {
        self.numerator.iter().any(|v| *v != 0) && self.denominator.iter().any(|v| *v != 0)
    }

    pub(super) fn parts(self) -> (Integer, Integer) {
        let decode = |value: [u8; 32]| {
            let mut bytes = [0; 64];
            bytes[..32].copy_from_slice(&value);
            Integer::canonical(Magnitude::from_bytes(bytes), false)
        };
        (decode(self.numerator), decode(self.denominator))
    }

    /// Display metadata only, never a phase, reader or admission authority.
    pub(super) fn approximate(self) -> f64 {
        MappedReaderPeriod {
            numerator: self.numerator,
            denominator: self.denominator,
        }
        .approximate()
    }
}

/// Separate1024-bit scratch for the physical-Q theorem. No wide value is copied
/// into an epoch, span, playback or owner certificate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PhysicalInteger {
    pub(super) negative: bool,
    pub(super) magnitude: Wide,
}

impl PhysicalInteger {
    pub(super) const ZERO: Self = Self {
        negative: false,
        magnitude: Wide::MIN,
    };
    fn canonical(magnitude: Wide, negative: bool) -> Self {
        Self {
            negative: negative && !magnitude.is_zero(),
            magnitude,
        }
    }
    pub(super) fn from_integer(value: Integer) -> Self {
        Self::canonical(wide(value.magnitude), value.negative)
    }
    pub(super) fn from_i128(value: i128) -> Option<Self> {
        Some(Self::from_integer(Integer::from_i128(value)?))
    }
    pub(super) fn from_u128(value: u128) -> Option<Self> {
        Some(Self::from_integer(Integer::from_u128(value)?))
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

impl Ord for PhysicalInteger {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (true, true) => other.magnitude.cmp(&self.magnitude),
            (false, false) => self.magnitude.cmp(&other.magnitude),
        }
    }
}
impl PartialOrd for PhysicalInteger {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The physical-Q forward theorem admits N<2^642,D<2^636 independently of the
/// unchanged declared-Q386/380 splitter. Exact physical membership precedes RN.
pub(super) fn split_physical_coordinate(
    numerator: PhysicalInteger,
    denominator: PhysicalInteger,
    raw_fraction: f64,
    region: FrameRange,
) -> Option<SourceCoordinate> {
    if region.start >= region.end
        || numerator.negative
        || denominator <= PhysicalInteger::ZERO
        || numerator.magnitude.bit_width() > 642
        || denominator.magnitude.bit_width() > 636
    {
        return None;
    }
    let n = numerator.magnitude;
    let d = denominator.magnitude;
    let start = Wide::try_from(region.start).ok()?.checked_mul(d)?;
    let end = Wide::try_from(region.end).ok()?.checked_mul(d)?;
    if n < start || n >= end {
        return None;
    }
    let mut whole = u128::try_from(n.checked_div(d)?).ok()?;
    if whole > 1_u128 << 64 {
        return None;
    }
    let remainder = n.checked_rem(d)?;
    let (raw, raw_exp) = dyadic(raw_fraction)?;
    if raw < 0 || !raw_fraction.is_finite() || !(0.0..1.0).contains(&raw_fraction) || raw_exp < -126
    {
        return None;
    }
    // At most636+126=762 bits. Identity is generic, including signed zero.
    let same = shl(remainder, u32::try_from(-raw_exp.min(0)).ok()?)?
        == Wide::try_from(raw).ok()?.checked_mul(d)?;
    let fraction = if same {
        raw_fraction
    } else if remainder.is_zero() {
        0.0
    } else {
        // With remainder/d widths u,v, floor(log2(r/d)) is u-v or u-v-1.
        // The scaled RN numerator therefore needs at most v+53<=689 bits.
        let bits = round_fraction(remainder, d, 52, -1022, 1023)?;
        if bits == 0x3ff0_0000_0000_0000 {
            whole = whole.checked_add(1)?;
            0.0
        } else {
            f64::from_bits(bits)
        }
    };
    if whole == region.end as u128 && fraction == 0.0 {
        return Some(SourceCoordinate {
            whole: region.start as u128,
            fraction: 0.0,
        });
    }
    if whole < region.start as u128 || whole >= region.end as u128 {
        return None;
    }
    Some(SourceCoordinate { whole, fraction })
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ExactMappedTaps {
    pub(crate) left: usize,
    pub(crate) right: Option<usize>,
    pub(crate) right_gain: f32,
}

impl MappedReaderPeriod {
    pub(super) fn full_fraction_parts(self) -> ([u8; 32], [u8; 32]) {
        (self.numerator, self.denominator)
    }
    pub(super) fn full_fraction_pair_inside(
        self,
        whole: u128,
        fraction: f64,
        start: usize,
    ) -> bool {
        self.pair_order(Some(whole), fraction, start) == Some(Ordering::Less)
    }
    /// Exact physical-equivalent reader topology, without owner-side reduction.
    pub(crate) fn physical(length: usize) -> Option<Self> {
        if length == 0 {
            return None;
        }
        let mut numerator = [0; 32];
        numerator[..8].copy_from_slice(&u64::try_from(length).ok()?.to_le_bytes());
        let mut denominator = [0; 32];
        denominator[0] = 1;
        Some(Self {
            numerator,
            denominator,
        })
    }

    /// Temporary coordinates for the same actual scalar period, without a new
    /// reader domain or owner. Decoding already removes every common power of
    /// two, so this dyadic construction needs no runtime GCD or approximation.
    pub(super) fn from_scalar_exact(period: f64) -> Option<Self> {
        if !period.is_finite() || !(0.5..=18_446_744_073_709_551_616.0).contains(&period) {
            return None;
        }
        let (coefficient, exponent) = dyadic(period)?;
        let coefficient = u128::try_from(coefficient).ok()?;
        let (n, d) = if exponent >= 0 {
            (
                shl(Magnitude::try_from(coefficient).ok()?, exponent as u32)?,
                Magnitude::try_from(1_u8).ok()?,
            )
        } else {
            (
                Magnitude::try_from(coefficient).ok()?,
                shl(
                    Magnitude::try_from(1_u8).ok()?,
                    u32::try_from(-exponent).ok()?,
                )?,
            )
        };
        if n.is_zero() || n.bit_width() > 256 || d.bit_width() > 256 {
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

    /// Largest permitted virtual raw whole, before physical tap projection.
    /// This is exact ceil(start + P) - 1, not the rounded marker endpoint or
    /// the deliberately physical-clamped last tap prepared below.
    pub(super) fn max_canonical_frame(self, start: usize) -> Option<u64> {
        let n = stored(self.numerator);
        let d = stored(self.denominator);
        if n.is_zero() || d.is_zero() {
            return None;
        }
        let q = u128::try_from(n.checked_div(d)?).ok()?;
        let has_remainder = !n.checked_rem(d)?.is_zero();
        let exclusive = (start as u128)
            .checked_add(q)?
            .checked_add(u128::from(has_remainder))?;
        u64::try_from(exclusive.checked_sub(1)?).ok()
    }
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

/// Normalize the separately proved recovery-closed source value before using
/// any unchanged reader splitter. No period256 cross-product sees N461/D454.
/// The actual reader domain and its physical/rational membership stay intact.
pub(super) fn split_recovery_coordinate(
    numerator: Integer,
    denominator: Integer,
    recovery_exp: i16,
    raw_fraction: f64,
    domain: SourceLoopDomain,
) -> Option<SourceCoordinate> {
    let prepared = PreparedRecoverySplit::new(domain, recovery_exp, raw_fraction)?;
    prepared.split(normalize_recovery_coefficient(
        numerator,
        denominator,
        recovery_exp,
    )?)
}

/// Exact unrounded coefficient on the already proved recovery lattice. This
/// performs no binary64 source rounding and conveys no reader/domain permit.
pub(super) fn normalize_recovery_coefficient(
    numerator: Integer,
    denominator: Integer,
    recovery_exp: i16,
) -> Option<u128> {
    if numerator.negative
        || denominator <= Integer::ZERO
        || numerator.magnitude.bit_width() > 461
        || denominator.magnitude.bit_width() > 454
        || !(-126..=0).contains(&recovery_exp)
    {
        return None;
    }
    let powers = denominator.magnitude.trailing_zeros();
    let odd = denominator.magnitude >> powers;
    if !numerator.magnitude.checked_rem(odd)?.is_zero() {
        return None;
    }
    let value = numerator.magnitude.checked_div(odd)?;
    let shift = -i32::try_from(powers).ok()? - i32::from(recovery_exp);
    let normalized = if shift >= 0 {
        let shift = u32::try_from(shift).ok()?;
        if value.bit_width().checked_add(shift)? > 126 {
            return None;
        }
        shl(value, shift)?
    } else {
        let divisor = shl(Magnitude::try_from(1_u8).ok()?, u32::try_from(-shift).ok()?)?;
        if !value.checked_rem(divisor)?.is_zero() {
            return None;
        }
        value.checked_div(divisor)?
    };
    if normalized.bit_width() > 126 {
        return None;
    }
    u128::try_from(normalized).ok()
}

/// One temporary actual-domain splitter. Only constants/validation are cached;
/// membership, raw identity, RN-even and seam carry remain the original split.
pub(super) struct PreparedRecoverySplit {
    kind: RecoverySplitKind,
    scale: Integer,
    raw_fraction: f64,
}
#[derive(Clone, Copy)]
enum RecoverySplitKind {
    Physical(FrameRange),
    Mapped {
        period: MappedReaderPeriod,
        start: usize,
    },
}
impl PreparedRecoverySplit {
    pub(super) fn new(
        domain: SourceLoopDomain,
        recovery_exp: i16,
        raw_fraction: f64,
    ) -> Option<Self> {
        if !domain.is_valid() || !(-126..=0).contains(&recovery_exp) {
            return None;
        }
        let scale = Integer::from_u128(1)?.shl(u32::try_from(-recovery_exp).ok()?)?;
        let kind = match domain.musical_period {
            SourceReaderPeriod::Physical | SourceReaderPeriod::MappedPhysicalQuarters(_) => {
                RecoverySplitKind::Physical(domain.region)
            }
            SourceReaderPeriod::ScalarMusical(bits) => {
                let period = MappedReaderPeriod::from_scalar_exact(f64::from_bits(bits))?;
                period.validate_physical(domain.region).then_some(())?;
                RecoverySplitKind::Mapped {
                    period,
                    start: domain.region.start,
                }
            }
            SourceReaderPeriod::MappedRational(period) => RecoverySplitKind::Mapped {
                period,
                start: domain.region.start,
            },
        };
        Some(Self {
            kind,
            scale,
            raw_fraction,
        })
    }

    pub(super) fn split(&self, coefficient: u128) -> Option<SourceCoordinate> {
        if coefficient >= 1_u128 << 126 {
            return None;
        }
        let normalized = Integer::from_u128(coefficient)?;
        match self.kind {
            RecoverySplitKind::Physical(region) => split_physical_coordinate(
                PhysicalInteger::from_integer(normalized),
                PhysicalInteger::from_integer(self.scale),
                self.raw_fraction,
                region,
            ),
            RecoverySplitKind::Mapped { period, start } => {
                period.split(normalized, self.scale, self.raw_fraction, start)
            }
        }
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
            let right_gain = super::full_fraction_source_numerics::seam_gain(
                relative,
                self.last,
                fraction,
                self.period.denominator,
                *self.width.as_bytes(),
            )?;
            return Some(ExactMappedTaps {
                left: self.region.start + self.last,
                right: Some(self.region.start),
                right_gain,
            });
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
    fn physical_quarters_keep_beat_units_and_reduce_only_at_owner_construction() {
        let n = Integer::from_u128(973596225794487503992543076417536).unwrap();
        let d = Integer::from_u128(482883607843738976046606013087875).unwrap();
        let quarters = PhysicalMapQuarters::new(n, d).unwrap();
        assert_eq!(quarters.parts(), (n, d));
        assert!(quarters.is_valid());
        assert_eq!(std::mem::size_of::<PhysicalMapQuarters>(), 64);
        assert!(!std::mem::needs_drop::<PhysicalMapQuarters>());
        let one = Integer::from_u128(1).unwrap();
        let beyond = one.shl(256).unwrap();
        assert!(PhysicalMapQuarters::new(one, beyond).is_none());
        assert_eq!(
            PhysicalMapQuarters::new(beyond, beyond).unwrap().parts(),
            (one, one)
        );
        assert_eq!(MappedReaderPeriod::physical(1501), Some(period(1501, 1)));
        assert!(MappedReaderPeriod::physical(0).is_none());
    }

    #[test]
    fn physical_split_has_separate_capacity_and_correct_normal_fraction_ties() {
        let one = PhysicalInteger::from_u128(1).unwrap();
        let denominator = one.shl(635).unwrap();
        let half = one.shl(582).unwrap().add(one.shl(529).unwrap()).unwrap();
        let region = FrameRange { start: 0, end: 1 };
        let even = split_physical_coordinate(half, denominator, 0.0, region).unwrap();
        assert_eq!(even.fraction.to_bits(), 2_f64.powi(-53).to_bits());
        let above =
            split_physical_coordinate(half.add(one).unwrap(), denominator, 0.0, region).unwrap();
        assert_eq!(
            above.fraction.to_bits(),
            2_f64.powi(-53).next_up().to_bits()
        );
        let raw =
            split_physical_coordinate(one.shl(509).unwrap(), denominator, 2_f64.powi(-126), region)
                .unwrap();
        assert_eq!(raw.fraction.to_bits(), 2_f64.powi(-126).to_bits());
        let zero = split_physical_coordinate(PhysicalInteger::ZERO, one, -0.0, region).unwrap();
        assert_eq!(zero.fraction.to_bits(), (-0.0_f64).to_bits());
        assert!(split_physical_coordinate(one, one.shl(636).unwrap(), 0.0, region).is_none());

        let old_large = Integer::from_u128(1).unwrap().shl(390).unwrap();
        assert!(period(2, 1).split(old_large, old_large, 0.0, 0).is_none());
        let new = split_physical_coordinate(
            PhysicalInteger::from_integer(old_large),
            PhysicalInteger::from_integer(old_large),
            0.0,
            FrameRange { start: 0, end: 2 },
        )
        .unwrap();
        assert_eq!((new.whole, new.fraction), (1, 0.0));
    }

    #[test]
    fn physical_rounding_carry_requires_exact_inside_before_integer_endpoint() {
        let one = PhysicalInteger::from_u128(1).unwrap();
        let denominator = one.shl(60).unwrap();
        let end = denominator
            .mul(PhysicalInteger::from_u128(4).unwrap())
            .unwrap();
        let region = FrameRange { start: 1, end: 4 };
        let carried =
            split_physical_coordinate(end.sub(one).unwrap(), denominator, 0.0, region).unwrap();
        assert_eq!((carried.whole, carried.fraction.to_bits()), (1, 0));
        assert!(split_physical_coordinate(end, denominator, 0.0, region).is_none());
        assert!(
            split_physical_coordinate(PhysicalInteger::ZERO, denominator, 0.0, region).is_none()
        );
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

    #[test]
    fn fine_fraction_seam_is_shared_by_direct_and_prepared_reader() {
        let p = period(5, 2);
        let region = FrameRange { start: 0, end: 3 };
        let prepared = p.prepare(region).unwrap();
        for fraction in [2_f64.powi(-433), 2_f64.powi(-1000), f64::from_bits(1)] {
            let direct = p.seam_taps(2, fraction, region).unwrap();
            let cached = prepared.seam_taps(2, fraction).unwrap();
            assert_eq!(
                (direct.left, direct.right, direct.right_gain.to_bits()),
                (2, Some(0), 0)
            );
            assert_eq!(
                (cached.left, cached.right, cached.right_gain.to_bits()),
                (direct.left, direct.right, direct.right_gain.to_bits())
            );
        }
    }
}
