//! Exact ordinary master cutovers, separate from initial and CommonPhase kernels.
//!
//! Original source/beat axes have magnitude <2^126. The signed fixed-2^-126
//! basis has |B|<2^252, independently of the number of previous cuts. At the
//! selected original beat grid g, |B/2^(g+126)|<2^126 is a rational bound, not
//! an integrality requirement. Join S=2^(g+126) ONCE: raw inverse I/D becomes
//! (I*S + (m*S-B)*D)/(D*S). The start inverse stays unjoined. Physical Q's
//! numerator/denominator is multiplied once after this join, never S squared.
//! After the original admission checks, cancel only the common power of two
//! in B/S. This divides initial, den and every derived affine coefficient by
//! the same exact factor, preserving all ratios, cycles and rounded pairs.
//! The stored basis, original grids and numeric class never change.
//!
//! Declared source N/D<=512/506, reader products<=768, raw identity<=632,
//! RN<=559, final pair/period<=880. Physical N/D<=768/762, unfolded<=510,
//! search/threshold<=766, raw identity<=888, membership<=826, RN<=815.
//! FullFraction adds d<=1074 only to its original raw inverse: source
//! N/D<=1586/1580, search/cycle<=1587, reader products<=1842/1836,
//! raw/RN<=1633. Thus ordinary uses1024 and FullFraction uses2048 scratch.
//! Cycle admission is a VALUE bound (<2^128 beat-grid units), separately
//! from the larger numerator widths. No copied owner or new clock authority.

use super::anchored::{first_true_near_high, first_true_near_low};
use super::general::GeneralPreparation;
use super::{
    CanonicalBeatSegment, MappedClockEpoch, MappedClockError, MappedClockMode, MappedLoopGeometry,
    MappedSourceSpan, SpanProgram,
};
use crate::audio_engine::bounded_source_integer::BoundedInteger as Num;
use crate::audio_engine::compact_source_numerics::{
    BaseGrid, Grid, GridStats, decode_f64_at_grid, decode_scaled_f64_at_grid,
    decode_source_pair_at_grid, exact_f64_parts, master_grid_exponent, required_grid_f64,
};
use crate::audio_engine::full_fraction_source_numerics::{
    self as numeric, ComposedAffine, ComposedClass,
};
use crate::audio_engine::general_source_numerics::MappedReaderPeriod;
use crate::audio_engine::source_playback::FractionalSourcePosition;
use crate::audio_engine::source_reader::{ExplicitSeekMode, SourceLoopDomain};
use crate::audio_engine::transport::TransportBeatProjection;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug)]
struct RawAnchor {
    frame: u64,
    fraction: f64,
    original_output_frame: u64,
    inverse_segment: CanonicalBeatSegment,
}
impl RawAnchor {
    fn matches_exact(self, other: Self) -> bool {
        self.frame == other.frame
            && self.fraction.to_bits() == other.fraction.to_bits()
            && self.original_output_frame == other.original_output_frame
            && self.inverse_segment.matches_exact(other.inverse_segment)
    }
}
/// Signed256-bit numeric coefficient, not an encoded flag or rounded observer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Basis(bnum::Int<32>);
impl Basis {
    fn from_num<const N: usize>(n: Num<N>) -> Option<Self> {
        let n = n.bounded(252)?.resize::<32>()?;
        let value = bnum::Int::<32>::from_bytes(*n.magnitude.as_bytes());
        Some(Self(if n.negative {
            value.checked_neg()?
        } else {
            value
        }))
    }
    fn number<const N: usize>(self) -> Option<Num<N>> {
        Num::<32>::canonical(self.0.unsigned_abs(), self.0.is_negative()).resize()
    }
    fn from_master(value: f64) -> Option<Self> {
        let (n, e) = exact_f64_parts(value)?;
        if n == 0 {
            return Self::from_num(Num::<32>::ZERO);
        }
        Self::from_num(Num::<32>::from_i128(n)?.shl(u32::try_from(e.checked_add(126)?).ok()?)?)
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) struct ComposedAnchor {
    raw: RawAnchor,
    basis: Basis,
}
impl ComposedAnchor {
    pub(super) fn matches_exact(self, other: Self) -> bool {
        self.raw.matches_exact(other.raw) && self.basis == other.basis
    }
}
fn anchor(epoch: MappedClockEpoch) -> Option<ComposedAnchor> {
    match epoch.mode {
        MappedClockMode::ComposedOrdinary(a) | MappedClockMode::ComposedFullFraction(a) => Some(a),
        MappedClockMode::CompactAnchored(a)
        | MappedClockMode::GeneralCompact(a)
        | MappedClockMode::FullFractionGeneral(a) => Some(ComposedAnchor {
            raw: RawAnchor {
                frame: a.frame,
                fraction: a.fraction,
                original_output_frame: a.original_output_frame,
                inverse_segment: a.inverse_segment,
            },
            basis: Basis::from_master(a.original_master)?,
        }),
        _ => None,
    }
}
fn full(epoch: MappedClockEpoch) -> bool {
    matches!(
        epoch.mode,
        MappedClockMode::FullFractionGeneral(_) | MappedClockMode::ComposedFullFraction(_)
    )
}
pub(super) fn replace(
    old: MappedClockEpoch,
    projection: TransportBeatProjection,
    frame: u64,
) -> Option<MappedClockEpoch> {
    if frame == u64::MAX || frame < old.first_output_frame || old.marker == 0 {
        return None;
    }
    let a = anchor(old)?;
    let addressed = frame.checked_sub(old.address_shift)?;
    if addressed < a.raw.original_output_frame {
        return None;
    }
    let actual_old = old.projection.beat_position_at_frame(frame);
    let (actual_new, last) = projection.finite_beats_for_horizon(frame, u64::MAX)?;
    if !actual_old.is_finite() || actual_old.to_bits() != actual_new.to_bits() {
        return None;
    }
    // Productive master finite/grid bounds, including the entire future horizon.
    Basis::from_master(last)?;
    let before = Basis::from_master(old.addressed_master_at(frame)?)?.number::<32>()?;
    let after = Basis::from_master(actual_new)?.number::<32>()?;
    let basis = a.basis.number::<32>()?;
    if basis > before || before > after {
        return None;
    }
    // Sum/difference intermediates <2^254; the final monotone basis remains
    // between its previous value and actual M(E), so repeated cuts do not grow it.
    let next = basis.add(after)?.sub(before)?;
    if next < basis || next > after {
        return None;
    }
    let a = ComposedAnchor {
        basis: Basis::from_num(next)?,
        ..a
    };
    Some(MappedClockEpoch {
        marker: old.marker,
        projection,
        address_shift: 0,
        first_output_frame: frame,
        mode: if full(old) {
            MappedClockMode::ComposedFullFraction(a)
        } else {
            MappedClockMode::ComposedOrdinary(a)
        },
    })
}

#[derive(Clone, Copy)]
struct Axes {
    x: i128,
    dx: i128,
    y: i128,
    dy: i128,
}
impl Axes {
    fn new(s: CanonicalBeatSegment, rate: u32, source: Grid, beat: Grid) -> Option<Self> {
        let x = decode_scaled_f64_at_grid(s.source_start, rate, source)?;
        let dx = decode_scaled_f64_at_grid(s.source_end, rate, source)?.checked_sub(x)?;
        let y = decode_f64_at_grid(s.beat_start, beat)?;
        let dy = decode_f64_at_grid(s.beat_end, beat)?.checked_sub(y)?;
        (dx > 0 && dy > 0).then_some(Self { x, dx, y, dy })
    }
    fn inverse<const N: usize>(self, raw: i128) -> Option<Num<N>> {
        if raw < self.x || raw > self.x.checked_add(self.dx)? {
            return None;
        }
        Num::from_i128(self.y)?
            .mul(Num::from_i128(self.dx)?)?
            .add(Num::from_i128(raw.checked_sub(self.x)?)?.mul(Num::from_i128(self.dy)?)?)
    }
}
fn compare<const N: usize>(v: f64, n: Num<N>, d: Num<N>, grid: Grid) -> Option<Ordering> {
    let (f, e) = exact_f64_parts(v)?;
    numeric::compare_scaled(Num::from_i128(f)?.mul(d)?, e, n, grid.exponent())
}
fn preparation(g: &MappedLoopGeometry, is_full: bool) -> Option<GeneralPreparation> {
    if is_full {
        return g.full_fraction;
    }
    if let Some(p) = g.general {
        return Some(p);
    }
    let p = g.exact_continuation?;
    let (source, beat) = p.compact?.original_grids();
    Some(GeneralPreparation {
        source,
        beat,
        start_segment: p.first,
        minimum: p.minimum,
        maximum: p.maximum,
    })
}
fn reader(domain: SourceLoopDomain) -> Option<MappedReaderPeriod> {
    if domain.musical_period.physical_quarters().is_some() {
        MappedReaderPeriod::physical(domain.region.len())
    } else {
        numeric::reader_period(domain)
    }
}

#[derive(Clone, Copy)]
struct Core<const N: usize> {
    epoch: MappedClockEpoch,
    source: Grid,
    beat: Grid,
    initial: Num<N>,
    den: Num<N>,
    cycle: Num<N>,
    qd: Num<N>,
    domain: SourceLoopDomain,
    class: ComposedClass,
}
impl<const N: usize> Core<N> {
    fn new(
        epoch: MappedClockEpoch,
        rate: u32,
        source: BaseGrid,
        beat: BaseGrid,
        domain: SourceLoopDomain,
        cycle: u128,
        cycle_exponent: i16,
    ) -> Option<Self> {
        Self::new_with_basis::<true>(epoch, rate, source, beat, domain, cycle, cycle_exponent)
    }
    // The false specialization is used only by the unreduced test oracle.
    fn new_with_basis<const REDUCE: bool>(
        epoch: MappedClockEpoch,
        rate: u32,
        source: BaseGrid,
        beat: BaseGrid,
        domain: SourceLoopDomain,
        cycle: u128,
        cycle_exponent: i16,
    ) -> Option<Self> {
        if !epoch.is_composed_ordinary() || !domain.is_valid() {
            return None;
        }
        let a = anchor(epoch)?;
        let physical = domain.musical_period.physical_quarters();
        let class = if full(epoch) {
            if N != 256 || physical.is_some() {
                return None;
            }
            ComposedClass::FullFraction
        } else {
            if N != 128 {
                return None;
            }
            if physical.is_some() {
                ComposedClass::Physical
            } else {
                ComposedClass::Declared
            }
        };
        let first = epoch.first_output_frame.checked_sub(epoch.address_shift)?;
        let last = u64::MAX.checked_sub(epoch.address_shift)?;
        if first < a.raw.original_output_frame {
            return None;
        }
        let horizon = epoch.projection.finite_beats_for_horizon(first, last)?;
        let source = source.refine(if full(epoch) {
            0
        } else {
            required_grid_f64(a.raw.fraction)?
        })?;
        let beat = beat.refine(
            master_grid_exponent(
                epoch.projection.frames_per_beat(),
                epoch.projection.original_beat_anchor(),
            )?
            .min(cycle_exponent),
        )?;
        decode_f64_at_grid(horizon.0, beat)?;
        decode_f64_at_grid(horizon.1, beat)?;
        let basis = a
            .basis
            .number::<N>()?
            .bounded(u32::try_from(252 + beat.exponent()).ok()?)?;
        let shift = u32::try_from(beat.exponent() + 126).ok()?;
        let m_first = Num::<N>::from_i128(decode_f64_at_grid(horizon.0, beat)?)?.shl(shift)?;
        if basis > m_first {
            return None;
        }
        if !reader(domain)?.canonical_pair(
            a.raw.frame as usize,
            a.raw.fraction,
            domain.region.start,
        ) {
            return None;
        }
        let ax = Axes::new(a.raw.inverse_segment, rate, source, beat)?;
        let (raw, inverse_den) = if full(epoch) {
            let (r, d) = numeric::raw_pair(a.raw.frame, a.raw.fraction, source.exponent())?;
            let (i, den) = numeric::inverse(r, d, ax.x, ax.dx, ax.y, ax.dy)?;
            (i.resize::<N>()?, den.resize::<N>()?)
        } else {
            let raw = decode_source_pair_at_grid(a.raw.frame, a.raw.fraction, source)?;
            (
                ax.inverse::<N>(raw)?.bounded(255)?,
                Num::<N>::from_i128(ax.dx)?,
            )
        };
        let (cycle, qd) = if let Some(q) = physical {
            let (qn, qd) = q.parts();
            let qn = qn.resize::<N>()?.shl((-beat.exponent()) as u32)?;
            (qn.mul(Num::from_u128(cycle)?)?, qd.resize::<N>()?)
        } else {
            (
                Num::<N>::from_u128(cycle)?
                    .shl(u32::try_from(cycle_exponent - beat.exponent()).ok()?)?,
                Num::from_i128(1)?,
            )
        };
        if cycle >= qd.shl(128)? {
            return None;
        }
        // All original magnitude, horizon, raw/inverse and cycle admission
        // checks above use the unreduced basis and original axes. This exact
        // cancellation changes only temporary arithmetic. In particular,
        // negative B is shifted through its magnitude, never rounded toward
        // minus infinity; zero cancels the full denominator power.
        let powers = if REDUCE {
            if basis.magnitude.is_zero() {
                shift
            } else {
                shift.min(basis.magnitude.trailing_zeros())
            }
        } else {
            0
        };
        let basis = Num::canonical(basis.magnitude.checked_shr(powers)?, basis.negative);
        let shift = shift.checked_sub(powers)?;
        let den = inverse_den.shl(shift)?;
        let initial = raw.shl(shift)?.sub(basis.mul(inverse_den)?)?;
        Some(Self {
            epoch,
            source,
            beat,
            initial,
            den,
            cycle,
            qd,
            domain,
            class,
        })
    }
    fn master(&self, frame: u64) -> Option<i128> {
        decode_f64_at_grid(self.epoch.addressed_master_at(frame)?, self.beat)
    }
    fn uncycled(&self, frame: u64) -> Option<Num<N>> {
        self.initial
            .add(Num::from_i128(self.master(frame)?)?.mul(self.den)?)
    }
    fn numerator(&self, frame: u64) -> Option<Num<N>> {
        self.uncycled(frame)?
            .mul(self.qd)?
            .sub(self.cycle.mul(self.den)?)
    }
    fn denominator(&self) -> Option<Num<N>> {
        self.den.mul(self.qd)
    }
    fn terms(
        &self,
        segment: CanonicalBeatSegment,
        rate: u32,
        frame: u64,
    ) -> Option<(Num<N>, Num<N>, Num<N>)> {
        let a = Axes::new(segment, rate, self.source, self.beat)?;
        let den = self.denominator()?;
        let offset = self.numerator(frame)?.sub(Num::from_i128(a.y)?.mul(den)?)?;
        let d = den.mul(Num::from_i128(a.dy)?)?;
        let n = Num::from_i128(a.x)?
            .mul(d)?
            .add(offset.mul(Num::from_i128(a.dx)?)?)?;
        // den includes S. Omitting it here changes every prepared future sample.
        let slope = den.mul(Num::from_i128(a.dx)?)?;
        Some((n, d.shl((-self.source.exponent()) as u32)?, slope))
    }
    fn position(
        &self,
        segment: CanonicalBeatSegment,
        rate: u32,
        frame: u64,
    ) -> Option<FractionalSourcePosition> {
        let (n, d, _) = self.terms(segment, rate, frame)?;
        let p = numeric::split_composed(
            n,
            d,
            anchor(self.epoch)?.raw.fraction,
            self.domain,
            self.class,
        )?;
        Some(FractionalSourcePosition {
            frame: usize::try_from(p.whole).ok()?,
            fraction: p.fraction,
            seek_mode: ExplicitSeekMode::Normal,
        })
    }
}
struct Clock<'a, const N: usize> {
    g: &'a MappedLoopGeometry,
    core: Core<N>,
    start: Num<N>,
    start_den: Num<N>,
    qn: Num<N>,
}
impl<'a, const N: usize> Clock<'a, N> {
    fn new(g: &'a MappedLoopGeometry, epoch: MappedClockEpoch) -> Option<Self> {
        Self::new_with_basis::<true>(g, epoch)
    }
    fn new_with_basis<const REDUCE: bool>(
        g: &'a MappedLoopGeometry,
        epoch: MappedClockEpoch,
    ) -> Option<Self> {
        let p = preparation(g, full(epoch))?;
        let a = anchor(epoch)?;
        if !g
            .map
            .canonical_segment_at_beat(a.raw.inverse_segment.beat_start)?
            .matches_exact(a.raw.inverse_segment)
        {
            return None;
        }
        let core = Core::new_with_basis::<REDUCE>(
            epoch,
            g.loaded_rate_hz,
            p.source,
            p.beat,
            g.domain,
            0,
            0,
        )?;
        let ax = Axes::new(p.start_segment, g.loaded_rate_hz, core.source, core.beat)?;
        // Unjoined start inverse: the raw full-fraction extension never enters it.
        let start = ax.inverse::<N>(decode_source_pair_at_grid(
            g.domain.region.start as u64,
            0.,
            core.source,
        )?)?;
        let start_den = Num::from_i128(ax.dx)?;
        let qn = if let Some(q) = g.domain.musical_period.physical_quarters() {
            q.parts()
                .0
                .resize::<N>()?
                .shl((-core.beat.exponent()) as u32)?
        } else {
            Num::from_i128(decode_f64_at_grid(g.quarters, core.beat)?)?
        };
        if qn <= Num::ZERO {
            return None;
        }
        let c = Self {
            g,
            core,
            start,
            start_den,
            qn,
        };
        c.phase(epoch.first_output_frame)?;
        c.phase(u64::MAX)?;
        Some(c)
    }
    fn unfolded_den(&self) -> Option<Num<N>> {
        self.start_den.mul(self.core.den)
    }
    fn unfolded(&self, frame: u64) -> Option<Num<N>> {
        let n = self
            .core
            .uncycled(frame)?
            .mul(self.start_den)?
            .sub(self.start.mul(self.core.den)?)?;
        if n.negative || n >= self.unfolded_den()?.shl(128)? {
            return None;
        }
        n.bounded(if N == 256 { 1584 } else { 510 })
    }
    fn scaled(&self, frame: u64) -> Option<Num<N>> {
        self.unfolded(frame)?
            .mul(self.core.qd)?
            .bounded(if N == 256 { 1587 } else { 766 })
    }
    fn phase(&self, frame: u64) -> Option<(u128, Core<N>, CanonicalBeatSegment)> {
        let cycles = self
            .scaled(frame)?
            .quotient(self.unfolded_den()?.mul(self.qn)?)?
            .u128()?;
        let cycle = self.qn.mul(Num::from_u128(cycles)?)?;
        if cycle >= self.core.qd.shl(128)? {
            return None;
        }
        let core = Core { cycle, ..self.core };
        let n = core.numerator(frame)?;
        let den = core.denominator()?;
        let segment = self
            .g
            .map
            .canonical_segment_by_beat_ordering(|v| compare(v, n, den, core.beat))?;
        Some((cycles, core, segment))
    }
    fn position(&self, frame: u64) -> Option<FractionalSourcePosition> {
        let (_, core, segment) = self.phase(frame)?;
        core.position(segment, self.g.loaded_rate_hz, frame)
    }
    fn threshold(&self, cycle: Num<N>, value: f64, end: bool) -> Option<Num<N>> {
        let is_outside = if end {
            compare(
                value,
                self.start
                    .mul(self.core.qd)?
                    .add(self.qn.mul(self.start_den)?)?,
                self.start_den.mul(self.core.qd)?,
                self.core.beat,
            )? != Ordering::Less
        } else {
            compare(value, self.start, self.start_den, self.core.beat)? != Ordering::Greater
        };
        let base = cycle.mul(self.unfolded_den()?)?;
        let n = if is_outside {
            if end {
                base.add(self.qn.mul(self.unfolded_den()?)?)?
            } else {
                base
            }
        } else {
            Num::<N>::from_i128(decode_f64_at_grid(value, self.core.beat)?)?
                .mul(self.start_den)?
                .sub(self.start)?
                .mul(self.core.den)?
                .mul(self.core.qd)?
                .add(base)?
        };
        n.bounded(if N == 256 { 1587 } else { 766 })
    }
}
fn qualified_clock<const N: usize>(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
) -> Result<Clock<'_, N>, MappedClockError> {
    let unavailable = MappedClockError::NumericalUnavailable;
    let c = Clock::<N>::new(g, epoch).ok_or(unavailable)?;
    for slope in [g.min_seconds_per_quarter, g.max_seconds_per_quarter] {
        if !super::valid_rate(
            f64::from(g.loaded_rate_hz) * slope / epoch.projection.frames_per_beat(),
        ) {
            return Err(MappedClockError::UnsupportedRate);
        }
    }
    let p = preparation(g, full(epoch)).ok_or(unavailable)?;
    let (fpb, e) = exact_f64_parts(epoch.projection.frames_per_beat()).ok_or(unavailable)?;
    for segment in [p.minimum, p.maximum] {
        let a =
            Axes::new(segment, g.loaded_rate_hz, c.core.source, c.core.beat).ok_or(unavailable)?;
        let n = Num::<N>::from_i128(a.dx).ok_or(unavailable)?;
        let d = Num::<N>::from_i128(a.dy)
            .and_then(|v| v.mul(Num::from_i128(fpb)?))
            .ok_or(unavailable)?;
        if numeric::compare_scaled(
            n,
            c.core.source.exponent() + 1,
            d,
            c.core.beat.exponent() + e,
        )
        .ok_or(unavailable)?
            == Ordering::Less
            || numeric::compare_scaled(
                n,
                c.core.source.exponent(),
                d,
                c.core.beat.exponent() + e + 1,
            )
            .ok_or(unavailable)?
                == Ordering::Greater
        {
            return Err(MappedClockError::UnsupportedRate);
        }
    }
    c.position(epoch.first_output_frame).ok_or(unavailable)?;
    Ok(c)
}
pub(super) fn validate(
    g: &MappedLoopGeometry,
    e: MappedClockEpoch,
) -> Result<(), MappedClockError> {
    if full(e) {
        qualified_clock::<256>(g, e).map(|_| ())
    } else {
        qualified_clock::<128>(g, e).map(|_| ())
    }
}
pub(super) fn position(
    g: &MappedLoopGeometry,
    e: MappedClockEpoch,
    f: u64,
) -> Result<FractionalSourcePosition, MappedClockError> {
    let result = if full(e) {
        qualified_clock::<256>(g, e)?.position(f)
    } else {
        qualified_clock::<128>(g, e)?.position(f)
    };
    result.ok_or(MappedClockError::NumericalUnavailable)
}
fn refresh_typed<const N: usize>(c: Clock<'_, N>, f: u64) -> Option<MappedSourceSpan> {
    if f == u64::MAX {
        return None;
    }
    let g = c.g;
    let e = c.core.epoch;
    let (cycles, core, segment) = c.phase(f)?;
    let lo = c.threshold(core.cycle, segment.beat_start, false)?;
    let hi = c.threshold(core.cycle, segment.beat_end, true)?;
    let start_frame = first_true_near_high(e.first_output_frame, f, |n| Some(c.scaled(n)? >= lo))?;
    let end_frame = if c.scaled(u64::MAX)? < hi {
        u64::MAX
    } else {
        first_true_near_low(f + 1, u64::MAX, |n| Some(c.scaled(n)? >= hi))?
    };
    let endpoint = c.position(end_frame)?;
    let program = if g.domain.musical_period.physical_quarters().is_some() {
        SpanProgram::physical([cycles as u64, (cycles >> 64) as u64])
    } else {
        let cycle = core.cycle.u128()?;
        SpanProgram::general([cycle as u64, (cycle >> 64) as u64], core.beat.exponent())
    };
    let span = MappedSourceSpan {
        epoch: e,
        segment,
        program,
        loaded_rate_hz: g.loaded_rate_hz,
        start_frame,
        end_frame,
        endpoint_frame: endpoint.frame,
        endpoint_fraction: endpoint.fraction,
        source_start: g.domain.region.start as u64,
    };
    span.local_rate()?;
    Some(span)
}
pub(super) fn refresh(
    g: &MappedLoopGeometry,
    e: MappedClockEpoch,
    f: u64,
) -> Result<MappedSourceSpan, MappedClockError> {
    let span = if full(e) {
        refresh_typed(qualified_clock::<256>(g, e)?, f)
    } else {
        refresh_typed(qualified_clock::<128>(g, e)?, f)
    };
    span.ok_or(MappedClockError::NumericalUnavailable)
}

pub(super) enum LocalView {
    Ordinary(Local<128>),
    Full(Local<256>),
}
pub(super) struct Local<const N: usize> {
    span: MappedSourceSpan,
    beat: Grid,
    affine: ComposedAffine<N>,
}
impl<const N: usize> Local<N> {
    fn position_at(&self, frame: u64) -> Option<FractionalSourcePosition> {
        if frame == self.span.end_frame {
            return Some(FractionalSourcePosition {
                frame: self.span.endpoint_frame,
                fraction: self.span.endpoint_fraction,
                seek_mode: ExplicitSeekMode::Normal,
            });
        }
        self.span.contains(frame).then_some(())?;
        self.affine.position(decode_f64_at_grid(
            self.span.epoch.addressed_master_at(frame)?,
            self.beat,
        )?)
    }
}
impl LocalView {
    pub(super) fn position_at(&self, frame: u64) -> Option<FractionalSourcePosition> {
        match self {
            Self::Ordinary(v) => v.position_at(frame),
            Self::Full(v) => v.position_at(frame),
        }
    }
}
fn local_typed<const N: usize>(
    span: MappedSourceSpan,
    domain: SourceLoopDomain,
) -> Option<Local<N>> {
    if !domain.is_valid()
        || span.source_start != domain.region.start as u64
        || span.start_frame < span.epoch.first_output_frame
        || span.start_frame >= span.end_frame
    {
        return None;
    }
    let a = anchor(span.epoch)?;
    let mut source = GridStats::new().include_u64(span.source_start)?;
    let mut beat = GridStats::new();
    for s in [a.raw.inverse_segment, span.segment] {
        source = source
            .include_scaled_f64(s.source_start, span.loaded_rate_hz)?
            .include_scaled_f64(s.source_end, span.loaded_rate_hz)?;
        beat = beat.include_f64(s.beat_start)?.include_f64(s.beat_end)?;
    }
    let (cycle, exponent) = if domain.musical_period.physical_quarters().is_some() {
        (span.program.physical_value()?, 0)
    } else {
        span.program.general_value()?
    };
    let core = Core::<N>::new(
        span.epoch,
        span.loaded_rate_hz,
        source.finish()?,
        beat.finish()?,
        domain,
        u128::from(cycle[0]) | (u128::from(cycle[1]) << 64),
        exponent,
    )?;
    let ax = Axes::new(span.segment, span.loaded_rate_hz, core.source, core.beat)?;
    let lower = Num::<N>::from_i128(ax.y)?.mul(core.denominator()?)?;
    let upper = Num::<N>::from_i128(ax.y.checked_add(ax.dy)?)?.mul(core.denominator()?)?;
    for f in [span.start_frame, span.end_frame - 1] {
        let n = core.numerator(f)?;
        if n < lower || n >= upper {
            return None;
        }
    }
    let (n, d, slope) = core.terms(span.segment, span.loaded_rate_hz, span.start_frame)?;
    let affine = ComposedAffine::new(
        n,
        slope,
        d,
        core.master(span.start_frame)?,
        core.master(span.end_frame - 1)?,
        a.raw.fraction,
        domain,
        core.class,
    )?;
    if !reader(domain)?.canonical_pair(
        span.endpoint_frame,
        span.endpoint_fraction,
        domain.region.start,
    ) {
        return None;
    }
    Some(Local {
        span,
        beat: core.beat,
        affine,
    })
}
pub(super) fn local(span: MappedSourceSpan, domain: SourceLoopDomain) -> Option<LocalView> {
    if full(span.epoch) {
        local_typed::<256>(span, domain).map(LocalView::Full)
    } else {
        local_typed::<128>(span, domain).map(LocalView::Ordinary)
    }
}

#[cfg(test)]
impl MappedSourceSpan {
    pub(crate) fn composed_split_for_test(self, domain: SourceLoopDomain) -> Option<(bool, usize)> {
        Some(match local(self, domain)? {
            LocalView::Ordinary(v) => (
                v.affine.uses_small_for_test(),
                v.affine.split_bits_for_test(),
            ),
            LocalView::Full(v) => (
                v.affine.uses_small_for_test(),
                v.affine.split_bits_for_test(),
            ),
        })
    }
}

#[cfg(test)]
#[path = "mapped_source_clock_composed_tests.rs"]
mod tests;
