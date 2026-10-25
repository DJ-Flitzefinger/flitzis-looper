//! Declared-Q closure under repeated RN-binary64 source-pair recovery.
//!
//! Original axes stay126-bit. Only the raw fraction adds d<=1074 bits.
//! Phase N/D<=1590/1583, ordinary N/D<=1460/1454, period products<=1846/1839.
//! These2048-bit temporaries are never copied into a clock, span or playback.
//! Local Small intervals retain512-bit coefficients and prepare exact L/U once;
//! their separate1024-bit splitter covers membership<=768 and raw/RN<=565 bits.
use std::cmp::Ordering;

use super::bounded_source_integer::BoundedInteger;
use super::compact_source_numerics::{Grid, exact_f64_parts};
use super::general_source_numerics::MappedReaderPeriod;
use super::source_numerics::SourceCoordinate;
use super::source_playback::FractionalSourcePosition;
use super::source_reader::{ExplicitSeekMode, SourceLoopDomain};

pub(super) fn reader_period(domain: SourceLoopDomain) -> Option<MappedReaderPeriod> {
    use super::source_reader_period::SourceReaderPeriod;
    match domain.musical_period {
        SourceReaderPeriod::Physical => MappedReaderPeriod::physical(domain.region.len()),
        SourceReaderPeriod::ScalarMusical(bits) => {
            MappedReaderPeriod::from_scalar_exact(f64::from_bits(bits))
        }
        SourceReaderPeriod::MappedRational(period) => Some(period),
        SourceReaderPeriod::MappedPhysicalQuarters(_) => None,
    }
}

pub(super) type Integer = BoundedInteger<256>;
type Wide = bnum::Uint<256>;

fn shift<const N: usize>(value: bnum::Uint<N>, bits: u32) -> Option<bnum::Uint<N>> {
    if value.is_zero() {
        return Some(value);
    }
    if value.bit_width().checked_add(bits)? > bnum::Uint::<N>::BITS {
        return None;
    }
    value.checked_shl(bits)
}
fn stored<const N: usize>(value: [u8; 32]) -> bnum::Uint<N> {
    let mut bytes = [0; N];
    bytes[..32].copy_from_slice(&value);
    bnum::Uint::<N>::from_bytes(bytes)
}
fn from_u128<const N: usize>(value: u128) -> Option<bnum::Uint<N>> {
    bnum::Uint::<N>::try_from(value).ok()
}

/// Validated immutable numeric reader identity; this is not source authority.
#[derive(Clone, Copy)]
struct PreparedReader {
    period: MappedReaderPeriod,
    start: usize,
}
impl PreparedReader {
    fn new(domain: SourceLoopDomain) -> Option<Self> {
        if !domain.is_valid() || domain.musical_period.physical_quarters().is_some() {
            return None;
        }
        Some(Self {
            period: reader_period(domain)?,
            start: domain.region.start,
        })
    }
}

/// R represents the separate pair at source exponent s-d. No whole narrowing.
pub(super) fn raw_pair(frame: u64, fraction: f64, s: i16) -> Option<(Integer, u32)> {
    if !(-126..=0).contains(&s) || !fraction.is_finite() || !(0.0..1.0).contains(&fraction) {
        return None;
    }
    let (f, g) = exact_f64_parts(fraction)?;
    if f < 0 || g < -1074 {
        return None;
    }
    let d = if f == 0 { 0 } else { (s - g).max(0) as u32 };
    let whole = Integer::from_u128(u128::from(frame))?.shl(d.checked_add((-s) as u32)?)?;
    let fractional = if f == 0 {
        Integer::ZERO
    } else {
        Integer::from_i128(f)?.shl(u32::try_from(g - s + d as i16).ok()?)?
    };
    Some((whole.add(fractional)?.bounded(1139)?, d))
}

/// Comparisons align only equal effective tops; distant probes cannot grow scratch.
pub(super) fn compare_scaled<const N: usize>(
    a: BoundedInteger<N>,
    ae: i16,
    b: BoundedInteger<N>,
    be: i16,
) -> Option<Ordering> {
    if a.negative != b.negative {
        return Some(if a.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        });
    }
    if a.magnitude.is_zero() || b.magnitude.is_zero() {
        return Some(a.cmp(&b));
    }
    let at = a.magnitude.bit_width() as i32 + i32::from(ae);
    let bt = b.magnitude.bit_width() as i32 + i32::from(be);
    let order = if at != bt {
        at.cmp(&bt)
    } else if ae >= be {
        a.shl((ae - be) as u32)?.magnitude.cmp(&b.magnitude)
    } else {
        a.magnitude.cmp(&b.shl((be - ae) as u32)?.magnitude)
    };
    Some(if a.negative { order.reverse() } else { order })
}

pub(super) fn compare_source(
    value: f64,
    rate: u32,
    raw: Integer,
    d: u32,
    grid: Grid,
) -> Option<Ordering> {
    // A remote search probe need not share the admitted local grid.
    let (coefficient, exponent) = exact_f64_parts(value)?;
    let source = Integer::from_i128(coefficient)?.mul(Integer::from_u128(u128::from(rate))?)?;
    compare_scaled(
        source,
        exponent,
        raw,
        grid.exponent().checked_sub(d as i16)?,
    )
}

pub(super) fn inverse(
    raw: Integer,
    d: u32,
    x0: i128,
    dx: i128,
    y0: i128,
    dy: i128,
) -> Option<(Integer, Integer)> {
    if dx <= 0 || dy <= 0 {
        return None;
    }
    let left = Integer::from_i128(x0)?.shl(d)?;
    let a = Integer::from_i128(dx)?.shl(d)?;
    if raw < left || raw > left.add(a)? {
        return None;
    }
    let i = Integer::from_i128(y0)?
        .mul(a)?
        .add(raw.sub(left)?.mul(Integer::from_i128(dy)?)?)?;
    Some((i.bounded(255 + d)?, a.bounded(127 + d)?))
}

/// Positive RN-even ratio including minsubnormal ties, with top-prefiltered scaling.
fn round<const N: usize>(
    n: bnum::Uint<N>,
    d: bnum::Uint<N>,
    mantissa: i32,
    minimum: i32,
    bias: i32,
) -> Option<u64> {
    if d.is_zero() || n > d {
        return None;
    }
    if n.is_zero() {
        return Some(0);
    }
    let mut exponent = n.bit_width() as i32 - d.bit_width() as i32;
    let below = if exponent >= 0 {
        n < shift(d, exponent as u32)?
    } else {
        shift(n, (-exponent) as u32)? < d
    };
    if below {
        exponent -= 1;
    }
    let effective = exponent.max(minimum);
    let scaled = shift(n, u32::try_from(mantissa - effective).ok()?)?;
    let mut q = u64::try_from(scaled.checked_div(d)?).ok()?;
    let remainder = scaled.checked_sub(d.checked_mul(from_u128(u128::from(q))?)?)?;
    let twice = shift(remainder, 1)?;
    if twice > d || (twice == d && q & 1 != 0) {
        q = q.checked_add(1)?;
    }
    if exponent < minimum && q < 1_u64 << mantissa {
        return Some(q);
    }
    let mut output = effective;
    if q == 1_u64 << (mantissa + 1) {
        q >>= 1;
        output += 1;
    }
    Some(((output + bias) as u64) << mantissa | (q & ((1_u64 << mantissa) - 1)))
}

fn raw_equal<const N: usize>(
    remainder: bnum::Uint<N>,
    denominator: bnum::Uint<N>,
    fraction: f64,
) -> Option<bool> {
    let (f, exponent) = exact_f64_parts(fraction)?;
    if f < 0 {
        return None;
    }
    if remainder.is_zero() || f == 0 {
        return Some(remainder.is_zero() && f == 0);
    }
    let product = from_u128(f as u128)?.checked_mul(denominator)?;
    let left_top = remainder.bit_width() as i32;
    let right_top = product.bit_width() as i32 + i32::from(exponent);
    if left_top != right_top {
        return Some(false);
    }
    // Equal tops bound the aligned width by denominator bits+53, not +1074.
    Some(if exponent >= 0 {
        remainder == shift(product, exponent as u32)?
    } else {
        shift(remainder, (-exponent) as u32)? == product
    })
}

pub(super) fn split<const N: usize>(
    numerator: BoundedInteger<N>,
    denominator: BoundedInteger<N>,
    raw_fraction: f64,
    domain: SourceLoopDomain,
    ordinary: bool,
) -> Option<SourceCoordinate> {
    split_full(
        numerator,
        denominator,
        raw_fraction,
        PreparedReader::new(domain)?,
        ordinary,
    )
}

fn split_full<const N: usize>(
    numerator: BoundedInteger<N>,
    denominator: BoundedInteger<N>,
    raw_fraction: f64,
    reader: PreparedReader,
    ordinary: bool,
) -> Option<SourceCoordinate> {
    if numerator.negative
        || denominator <= BoundedInteger::<N>::ZERO
        || !raw_fraction.is_finite()
        || !(0.0..1.0).contains(&raw_fraction)
    {
        return None;
    }
    let n = numerator
        .bounded(if ordinary { 1460 } else { 1590 })?
        .resize::<256>()?
        .magnitude;
    let d = denominator
        .bounded(if ordinary { 1454 } else { 1583 })?
        .resize::<256>()?
        .magnitude;
    let (pn, pd) = reader.period.full_fraction_parts();
    let start = from_u128(reader.start as u128)?.checked_mul(d)?;
    if n < start {
        return None;
    }
    if n.checked_sub(start)?.checked_mul(stored(pd))? >= d.checked_mul(stored(pn))? {
        return None;
    }
    finish_split(n, d, raw_fraction, reader)
}

/// The caller proves exact containment before RN, for every output. Small's
///1024-bit scratch covers N/D<=512, raw/RN<=565 and period products<=768.
/// Full's separately retained2048-bit source/reader bounds are unchanged.
fn finish_split<const N: usize>(
    n: bnum::Uint<N>,
    d: bnum::Uint<N>,
    raw_fraction: f64,
    reader: PreparedReader,
) -> Option<SourceCoordinate> {
    let mut whole = u128::try_from(n.checked_div(d)?).ok()?;
    if whole > 1_u128 << 64 {
        return None;
    }
    // qD<=N, so a single checked quotient suffices for its exact remainder.
    let remainder = n.checked_sub(d.checked_mul(from_u128(whole)?)?)?;
    let fraction = if raw_equal(remainder, d, raw_fraction)? {
        raw_fraction
    } else if remainder.is_zero() {
        0.0
    } else {
        let bits = round(remainder, d, 52, -1022, 1023)?;
        if bits == 0x3ff0_0000_0000_0000 {
            whole = whole.checked_add(1)?;
            0.0
        } else {
            f64::from_bits(bits)
        }
    };
    // Exact-inside was established before RN. Exactly one canonical seam carry.
    if !reader
        .period
        .full_fraction_pair_inside(whole, fraction, reader.start)
    {
        return Some(SourceCoordinate {
            whole: reader.start as u128,
            fraction: 0.0,
        });
    }
    Some(SourceCoordinate { whole, fraction })
}

/// New seam theorem: N<=1394,D<=1330 and RN32 temporary<=1543 bits.
pub(super) fn seam_gain(
    relative: usize,
    last: usize,
    fraction: f64,
    denominator: [u8; 32],
    width: [u8; 128],
) -> Option<f32> {
    let (f, e) = exact_f64_parts(fraction)?;
    if f < 0 || !(-1074..-432).contains(&e) {
        return None;
    }
    let shift_bits = (-e) as u32;
    let delta: Wide = shift(from_u128(relative.checked_sub(last)? as u128)?, shift_bits)?
        .checked_add(from_u128(f as u128)?)?;
    let n = delta.checked_mul(stored(denominator))?;
    let mut bytes = [0; 256];
    bytes[..128].copy_from_slice(&width);
    let d = shift(Wide::from_bytes(bytes), shift_bits)?;
    if n.bit_width() > 1394 || d.bit_width() > 1330 {
        return None;
    }
    Some(f32::from_bits(
        u32::try_from(round(n, d, 23, -126, 127)?).ok()?,
    ))
}

/// A fixed exact affine interval; small specialization is chosen once, never per frame.
pub(super) enum RationalAffine {
    Small(SmallAffine),
    Full(Affine<256>),
}
pub(super) struct Affine<const N: usize> {
    n: BoundedInteger<N>,
    b: BoundedInteger<N>,
    d: BoundedInteger<N>,
    master: i128,
    last_master: i128,
    raw_fraction: f64,
    reader: PreparedReader,
    ordinary: bool,
}

pub(super) struct SmallAffine {
    affine: Affine<64>,
    bounds: SmallBounds,
}
struct SmallBounds {
    lower: bnum::Uint<128>,
    upper: bnum::Uint<128>,
}
impl SmallBounds {
    fn new(denominator: BoundedInteger<64>, reader: PreparedReader) -> Option<Self> {
        if denominator <= BoundedInteger::<64>::ZERO {
            return None;
        }
        let d = denominator.resize::<128>()?.magnitude;
        let lower = from_u128(reader.start as u128)?.checked_mul(d)?;
        let (pn, pd) = reader.period.full_fraction_parts();
        let product = d.checked_mul(stored(pn))?;
        let pd = stored::<128>(pd);
        let q = product.checked_div(pd)?;
        let rem = product.checked_sub(q.checked_mul(pd)?)?;
        let ceiling = q.checked_add(from_u128(u128::from(!rem.is_zero()))?)?;
        // N is integral: N < start*D + D*Pn/Pd iff N < L+ceil(D*Pn/Pd).
        Some(Self {
            lower,
            upper: lower.checked_add(ceiling)?,
        })
    }
    fn split(
        &self,
        n: BoundedInteger<64>,
        d: BoundedInteger<64>,
        raw: f64,
        reader: PreparedReader,
    ) -> Option<SourceCoordinate> {
        if n.negative || d <= BoundedInteger::<64>::ZERO {
            return None;
        }
        let n = n.resize::<128>()?.magnitude;
        if n < self.lower || n >= self.upper {
            return None;
        }
        finish_split(n, d.resize::<128>()?.magnitude, raw, reader)
    }
}
impl RationalAffine {
    pub(super) fn new(
        n: Integer,
        b: Integer,
        d: Integer,
        master: i128,
        last_master: i128,
        raw_fraction: f64,
        domain: SourceLoopDomain,
        ordinary: bool,
    ) -> Option<Self> {
        let delta = Integer::from_i128(last_master.checked_sub(master)?)?;
        if delta.negative || b.negative {
            return None;
        }
        let last = n.add(b.mul(delta)?)?;
        let reader = PreparedReader::new(domain)?;
        split_full(n, d, raw_fraction, reader, ordinary)?;
        split_full(last, d, raw_fraction, reader, ordinary)?;
        if let (Some(n), Some(b), Some(d), Some(_)) = (
            n.resize::<64>(),
            b.resize::<64>(),
            d.resize::<64>(),
            last.resize::<64>(),
        ) {
            let bounds = SmallBounds::new(d, reader)?;
            Some(Self::Small(SmallAffine {
                affine: Affine {
                    n,
                    b,
                    d,
                    master,
                    last_master,
                    raw_fraction,
                    reader,
                    ordinary,
                },
                bounds,
            }))
        } else {
            Some(Self::Full(Affine {
                n,
                b,
                d,
                master,
                last_master,
                raw_fraction,
                reader,
                ordinary,
            }))
        }
    }
    pub(super) fn position(&self, master: i128) -> Option<FractionalSourcePosition> {
        match self {
            Self::Small(a) => a.position(master),
            Self::Full(a) => a.position(master),
        }
    }
    #[cfg(test)]
    pub(super) fn split_bits_for_test(&self) -> usize {
        match self {
            Self::Small(_) => 1024,
            Self::Full(_) => 2048,
        }
    }
}
impl<const N: usize> Affine<N> {
    fn numerator(&self, master: i128) -> Option<BoundedInteger<N>> {
        if master > self.last_master {
            return None;
        }
        let delta = BoundedInteger::<N>::from_i128(master.checked_sub(self.master)?)?;
        if delta.negative {
            return None;
        }
        self.n.add(self.b.mul(delta)?)
    }
    fn position(&self, master: i128) -> Option<FractionalSourcePosition> {
        let c = split_full(
            self.numerator(master)?,
            self.d,
            self.raw_fraction,
            self.reader,
            self.ordinary,
        )?;
        Some(FractionalSourcePosition {
            frame: usize::try_from(c.whole).ok()?,
            fraction: c.fraction,
            seek_mode: ExplicitSeekMode::Normal,
        })
    }
}
impl SmallAffine {
    fn position(&self, master: i128) -> Option<FractionalSourcePosition> {
        let a = &self.affine;
        let c = self
            .bounds
            .split(a.numerator(master)?, a.d, a.raw_fraction, a.reader)?;
        Some(FractionalSourcePosition {
            frame: usize::try_from(c.whole).ok()?,
            fraction: c.fraction,
            seek_mode: ExplicitSeekMode::Normal,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::source_reader::{FrameRange, SourceReaderPeriod};

    fn integer(n: u128) -> Integer {
        Integer::from_u128(n).unwrap()
    }
    fn domain() -> SourceLoopDomain {
        SourceLoopDomain::physical(8, FrameRange { start: 0, end: 8 })
    }

    #[test]
    fn separate_raw_pair_scales_coarse_fraction_and_preserves_full_subnormal_domain() {
        let (r, d) = raw_pair(13, 0.25, -10).unwrap();
        assert_eq!(d, 0);
        assert_eq!(r, integer(13 * 1024 + 256));
        let (r, d) = raw_pair(13, f64::from_bits(1), -10).unwrap();
        assert_eq!(d, 1064);
        assert_eq!(r, integer(13).shl(1074).unwrap().add(integer(1)).unwrap());
        let (i, a) = inverse(r, d, 0, 20 * 1024, 0, 1024).unwrap();
        assert_eq!(i, r.mul(integer(1024)).unwrap());
        assert_eq!(a, integer(20 * 1024).shl(d).unwrap());
        assert!(inverse(r, d, 14 * 1024, 1024, 0, 1024).is_none());
        assert!(raw_pair(u64::MAX, f64::from_bits(1), -126).is_some());
        for bad in [-0.25, 1.0, f64::NAN, f64::INFINITY] {
            assert!(raw_pair(0, bad, -10).is_none());
        }
        assert!(raw_pair(0, 0.0, -127).is_none());
        assert_eq!(raw_pair(13, -0.0, -10), raw_pair(13, 0.0, -10));
    }

    #[test]
    fn full_split_rn64_subnormal_ties_raw_identity_and_virtual_carry() {
        let one = integer(1);
        let denominator = one.shl(1075).unwrap();
        assert_eq!(
            split(one, denominator, 0., domain(), true)
                .unwrap()
                .fraction
                .to_bits(),
            0
        );
        assert_eq!(
            split(integer(3), denominator, 0., domain(), true)
                .unwrap()
                .fraction
                .to_bits(),
            2
        );
        let raw = split(
            one,
            one.shl(1074).unwrap(),
            f64::from_bits(1),
            domain(),
            true,
        )
        .unwrap();
        assert_eq!(raw.fraction.to_bits(), 1);
        assert_eq!(
            split(Integer::ZERO, one, -0., domain(), true)
                .unwrap()
                .fraction
                .to_bits(),
            (-0_f64).to_bits()
        );
        let d = one.shl(1100).unwrap();
        let n = d.mul(integer(8)).unwrap().sub(one).unwrap();
        let carry = split(n, d, 0., domain(), true).unwrap();
        assert_eq!((carry.whole, carry.fraction.to_bits()), (0, 0));
        assert!(split(d.mul(integer(8)).unwrap(), d, 0., domain(), true).is_none());
        assert!(split(one, one.shl(1455).unwrap(), 0., domain(), true).is_none());
        assert!(raw_equal(one.magnitude, one.shl(1500).unwrap().magnitude, 0.5) == Some(false));
    }

    #[test]
    fn full_split_keeps_rational_seam_and_old_numeric_results() {
        use crate::audio_engine::general_source_numerics::Integer as Old;
        let period =
            MappedReaderPeriod::new(Old::from_u128(31).unwrap(), Old::from_u128(10).unwrap())
                .unwrap();
        let domain = SourceLoopDomain {
            sample_frames: 10,
            region: FrameRange { start: 1, end: 4 },
            musical_period: SourceReaderPeriod::MappedRational(period),
        };
        for (n, d, f) in [
            (1, 1, -0.),
            (7, 4, 0.),
            (41_u128 * (1 << 60) - 10, 10_u128 << 60, 0.),
        ] {
            let old = period
                .split(Old::from_u128(n).unwrap(), Old::from_u128(d).unwrap(), f, 1)
                .unwrap();
            let new = split(integer(n), integer(d), f, domain, true).unwrap();
            assert_eq!(
                (new.whole, new.fraction.to_bits()),
                (old.whole, old.fraction.to_bits())
            );
        }
        let d = integer(10).shl(1100).unwrap();
        let n = integer(41).shl(1100).unwrap().sub(integer(1)).unwrap();
        let carried = split(n, d, 0., domain, true).unwrap();
        assert_eq!((carried.whole, carried.fraction.to_bits()), (1, 0));
    }

    #[test]
    fn rational_affine_selects_bounded_class_and_is_addressed_without_accumulation() {
        for shift in [0, 1074] {
            let unit = integer(1).shl(shift).unwrap();
            let affine = RationalAffine::new(unit, unit, unit, 0, 3, 0., domain(), true).unwrap();
            assert_eq!(matches!(affine, RationalAffine::Small(_)), shift == 0);
            for n in [3, 0, 2, 1, 3] {
                let p = affine.position(n).unwrap();
                assert_eq!((p.frame, p.fraction.to_bits()), ((1 + n) as usize, 0));
            }
        }
    }

    fn compare_small_full(
        n: Integer,
        d: Integer,
        raw: f64,
        domain: SourceLoopDomain,
    ) -> Option<(u128, u64)> {
        let reader = PreparedReader::new(domain).unwrap();
        let d_small = d.resize::<64>().unwrap();
        let bounds = SmallBounds::new(d_small, reader).unwrap();
        let small = bounds
            .split(n.resize::<64>().unwrap(), d_small, raw, reader)
            .map(|p| (p.whole, p.fraction.to_bits()));
        for ordinary in [false, true] {
            let full = split(n, d, raw, domain, ordinary).map(|p| (p.whole, p.fraction.to_bits()));
            assert_eq!(small, full);
        }
        small
    }

    #[test]
    fn prepared_small_bounds_keep_strict_integer_and_noninteger_thresholds() {
        use crate::audio_engine::general_source_numerics::Integer as Old;
        let period =
            MappedReaderPeriod::new(Old::from_u128(31).unwrap(), Old::from_u128(10).unwrap())
                .unwrap();
        let domain = SourceLoopDomain {
            sample_frames: 10,
            region: FrameRange { start: 1, end: 4 },
            musical_period: SourceReaderPeriod::MappedRational(period),
        };
        for (den, lower, upper) in [(7, 7, 29), (10, 10, 41)] {
            let bounds = SmallBounds::new(
                Old::from_u128(den).unwrap(),
                PreparedReader::new(domain).unwrap(),
            )
            .unwrap();
            assert_eq!(u128::try_from(bounds.lower).unwrap(), lower);
            assert_eq!(u128::try_from(bounds.upper).unwrap(), upper);
            assert!(compare_small_full(integer(lower - 1), integer(den), 0., domain).is_none());
            assert_eq!(
                compare_small_full(integer(lower), integer(den), -0., domain),
                Some((1, (-0_f64).to_bits()))
            );
            assert!(compare_small_full(integer(upper - 1), integer(den), 0., domain).is_some());
            assert!(compare_small_full(integer(upper), integer(den), 0., domain).is_none());
        }
        assert_eq!(
            compare_small_full(
                integer((41_u128 << 60) - 10),
                integer(10_u128 << 60),
                0.,
                domain
            ),
            Some((1, 0))
        );
    }

    #[test]
    fn prepared_small_keeps_rn_ties_tiny_raw_identity_and_virtual_endpoint() {
        let one = integer(1);
        let d = one.shl(54).unwrap();
        assert_eq!(
            compare_small_full(integer((1_u128 << 53) + 1), d, 0., domain()),
            Some((0, 0.5_f64.to_bits()))
        );
        assert_eq!(
            compare_small_full(integer((1_u128 << 53) + 2), d, 0., domain()),
            Some((0, 0.5_f64.next_up().to_bits()))
        );
        assert_eq!(
            compare_small_full(one, one.shl(511).unwrap(), f64::from_bits(1), domain()),
            Some((0, 2_f64.powi(-511).to_bits()))
        );
        assert_eq!(
            compare_small_full(one, one.shl(511).unwrap(), 2_f64.powi(-511), domain()),
            Some((0, 2_f64.powi(-511).to_bits()))
        );
        let scalar = SourceLoopDomain::musical(8, FrameRange { start: 0, end: 8 }, 8.).unwrap();
        assert_eq!(
            compare_small_full(Integer::ZERO, one, -0., scalar),
            Some((0, (-0_f64).to_bits()))
        );
        let period = MappedReaderPeriod::physical(2).unwrap();
        let start = usize::MAX - 1;
        let virtual_domain = SourceLoopDomain {
            sample_frames: usize::MAX,
            region: FrameRange {
                start,
                end: usize::MAX,
            },
            musical_period: SourceReaderPeriod::MappedRational(period),
        };
        assert_eq!(
            compare_small_full(
                one.shl(124).unwrap().sub(one).unwrap(),
                one.shl(60).unwrap(),
                0.,
                virtual_domain
            ),
            Some((start as u128, 0))
        );
    }

    #[test]
    fn prepared_small_selection_checks_coefficients_and_whole_interval_before_evaluation() {
        let one = integer(1);
        for (bits, expected) in [(511, 1024), (512, 2048), (1074, 2048)] {
            let unit = one.shl(bits).unwrap();
            let affine =
                RationalAffine::new(unit, Integer::ZERO, unit, 10, 13, 0., domain(), false)
                    .unwrap();
            assert_eq!(affine.split_bits_for_test(), expected);
            for j in [13, 10, 12, 11, 13] {
                let p = affine.position(j).unwrap();
                assert_eq!((p.frame, p.fraction.to_bits()), (1, 0));
            }
            assert!(affine.position(9).is_none());
            assert!(affine.position(14).is_none());
        }
        let limit = one.shl(512).unwrap();
        let start = limit.sub(integer(2)).unwrap();
        let d = one.shl(510).unwrap();
        let affine = RationalAffine::new(start, one, d, 0, 2, 0., domain(), true).unwrap();
        assert_eq!(
            affine.split_bits_for_test(),
            2048,
            "last N needs513bits although n/b/d individually fit512"
        );
        for j in [0, 1, 2] {
            let direct = split(
                start.add(integer(j as u128)).unwrap(),
                d,
                0.,
                domain(),
                true,
            )
            .unwrap();
            let local = affine.position(j).unwrap();
            assert_eq!(
                (local.frame as u128, local.fraction.to_bits()),
                (direct.whole, direct.fraction.to_bits())
            );
        }
        let wide_multiplier =
            RationalAffine::new(one, limit, one, 0, 0, 0., domain(), false).unwrap();
        assert_eq!(
            wide_multiplier.split_bits_for_test(),
            2048,
            "a wide B cannot enter Small even on a master plateau"
        );
        assert_eq!(wide_multiplier.position(0).unwrap().frame, 1);
    }
}
