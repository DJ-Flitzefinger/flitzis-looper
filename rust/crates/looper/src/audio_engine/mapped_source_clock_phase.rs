//! Recovery-closed original-map convergence on the unchanged binary64 master.
//!
//! Original axes remain on admitted 126-bit grids. The recovery/rate gate proves
//! Grid recovery proves r+k<=74: source N<2^461,D<2^454 and cycle products<2^459.
//! That class retains fixed512 scratch and the original dyadic reader splitter.
//! Declared-Q FullFraction instead proves r+k<=129, raw extension d<=1074,
//! source N<=1590/D<=1583 bits and separate fixed2048 scratch/splitting.
//! Intentional B(A)-M(T) debt uses two original inverses only in <=459-bit
//! comparisons. At capture symbolic cancellation restores the start denominator;
//! captured source N<2^388,D<2^380, within the original461/454 normalization gate.
//! An explicit grid target substitutes exact B(g) for B(A)-M(T). Its original
//! segment and loaded-frame g join the checked 126-bit grids before preparation.
//! Debt pairs raw/g inverses; uncaptured values retain only raw's denominator,
//! captured values cancel raw symbolically and retain only g's denominator.
//! Physical folding then pairs A with that one denominator. No operation forms
//! a raw*g*A denominator product; the same fixed512/2048 worst-width gates apply.
//! A private virtual edge target permits signed g outside audio. Original axes
//! have magnitude<2^126, so |g-x0|<2^127 and the inverse sum needs at most254 bits.
//! It uses the same checked broad scratch bounds, never playable extrapolation.

use super::{
    CanonicalBeatSegment, ExplicitSeekMode, FractionalSourcePosition, MappedClockEpoch,
    MappedClockError, MappedClockMode, MappedLoopGeometry, MappedSourceSpan, SourceLoopDomain,
    SpanProgram, TransportTimeline, valid_rate,
};
use crate::audio_engine::compact_source_numerics::{
    BaseGrid, Grid, GridStats, decode_f64_at_grid, decode_scaled_f64_at_grid,
    decode_source_pair_at_grid, exact_f64_parts, master_grid_exponent, required_grid_f64,
};
use crate::audio_engine::general_source_numerics::{
    Integer, Magnitude, MappedReaderPeriod, PreparedRecoverySplit, normalize_recovery_coefficient,
    split_recovery_coordinate,
};
use crate::audio_engine::loop_phase_reference::{
    GridPhaseOrigin, GridPhaseTarget, LoopPhaseReference, same_grid_target,
};
use std::cmp::Ordering;

const CAPTURED: u8 = 0;
const GLOBAL: u8 = 1;
const WINDOW: u8 = 2;
const NEGATIVE: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum RecoveryKind {
    Grid(i8),
    FullFraction,
}
impl RecoveryKind {
    fn grid(self) -> Option<i8> {
        if let Self::Grid(e) = self {
            Some(e)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhaseConfig {
    delta_exp: i16,
    source_exp: i8,
    beat_exp: i8,
    recovery: RecoveryKind,
    k: u8,
    flags: u8,
}
impl PhaseConfig {
    fn valid(self) -> bool {
        self.flags & 0xc0 == 0
            && self.kind() <= WINDOW
            && (-126..=0).contains(&self.source_exp)
            && (-126..=0).contains(&self.beat_exp)
            && self.recovery.grid().is_none_or(|e| (-126..=0).contains(&e))
            && if self.kind() == CAPTURED {
                self.k == 0 && self.delta_exp == 0
            } else {
                (6..=64).contains(&self.k)
                    && (self.kind() != WINDOW || (-191..=41).contains(&self.delta_exp))
            }
    }
    fn kind(self) -> u8 {
        self.flags & 3
    }
    fn negative(self) -> bool {
        self.flags & NEGATIVE != 0
    }
    fn downbeat(self) -> u8 {
        (self.flags >> 3) & 3
    }
    fn bar_qualified(self) -> bool {
        self.flags & 32 != 0
    }
    fn fine(self) -> i16 {
        if self.kind() == WINDOW {
            i16::from(self.beat_exp).min(self.delta_exp)
        } else {
            i16::from(self.beat_exp)
        }
    }
}

/// Fixed copied payload. E/projection and optional requested grid identity are immutable.
#[derive(Clone, Copy, Debug)]
pub(super) struct CommonPhaseAnchor {
    frame: u64,
    fraction: f64,
    inverse_segment: CanonicalBeatSegment,
    quarters: f64,
    config: PhaseConfig,
    grid_target: Option<GridPhaseTarget>,
}
impl CommonPhaseAnchor {
    pub(super) fn matches_exact(self, other: Self) -> bool {
        self.frame == other.frame
            && self.fraction.to_bits() == other.fraction.to_bits()
            && self.inverse_segment.matches_exact(other.inverse_segment)
            && self.quarters.to_bits() == other.quarters.to_bits()
            && self.config == other.config
            && same_grid_target(self.grid_target, other.grid_target)
    }
}

#[derive(Clone, Copy, Debug)]
struct Seed {
    start: f64,
    width_exp: i16,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CommonPhasePreparation {
    source: BaseGrid,
    beat: BaseGrid,
    start: CanonicalBeatSegment,
    minimum: CanonicalBeatSegment,
    maximum: CanonicalBeatSegment,
    positive_seed: Option<Seed>,
    negative_seed: Option<Seed>,
    dyadic: Option<DyadicRecovery>,
    quarters: f64,
    downbeat: u8,
    bar: bool,
    grid_target: Option<GridPhaseTarget>,
}
#[derive(Clone, Copy, Debug)]
struct DyadicRecovery {
    odd_common: u128,
    minimum_slope_exp: i16,
    intercept_exp: Option<i16>,
    recovery_exp: i16,
}

#[derive(Clone, Copy)]
struct Axes {
    x0: i128,
    dx: i128,
    y0: i128,
    dy: i128,
}
impl Axes {
    fn new(segment: CanonicalBeatSegment, rate: u32, s: Grid, b: Grid) -> Option<Self> {
        let x0 = decode_scaled_f64_at_grid(segment.source_start, rate, s)?;
        let x1 = decode_scaled_f64_at_grid(segment.source_end, rate, s)?;
        let y0 = decode_f64_at_grid(segment.beat_start, b)?;
        let y1 = decode_f64_at_grid(segment.beat_end, b)?;
        let dx = x1.checked_sub(x0)?;
        let dy = y1.checked_sub(y0)?;
        (dx > 0 && dy > 0).then_some(Self { x0, dx, y0, dy })
    }
    fn inverse(self, raw: i128) -> Option<Integer> {
        int(self.y0)?
            .mul(int(self.dx)?)?
            .add(int(raw.checked_sub(self.x0)?)?.mul(int(self.dy)?)?)
    }
    fn slope(self, s: i16, b: i16) -> Option<(u128, i16)> {
        let dy = self.dy as u128;
        let odd = dy >> dy.trailing_zeros();
        let dx = self.dx as u128;
        if dx % odd != 0 {
            return None;
        }
        let q = dx / odd;
        let exponent = s
            .checked_sub(b)?
            .checked_sub(dy.trailing_zeros() as i16)?
            .checked_add(q.trailing_zeros() as i16)?;
        (-252..=252)
            .contains(&exponent)
            .then_some((q >> q.trailing_zeros(), exponent))
    }
    fn intercept_exp(self, s: i16) -> Option<Option<i16>> {
        let n = int(self.x0)?
            .mul(int(self.dy)?)?
            .sub(int(self.dx)?.mul(int(self.y0)?)?)?;
        if n == Integer::ZERO {
            return Some(None);
        }
        Some(Some(
            s.checked_add(n.magnitude.trailing_zeros() as i16)?
                .checked_sub((self.dy as u128).trailing_zeros() as i16)?,
        ))
    }
}

fn int(value: i128) -> Option<Integer> {
    Integer::from_i128(value)
}
fn uint(value: u128) -> Option<Integer> {
    Integer::from_u128(value)
}
fn power(shift: u32) -> Option<Integer> {
    uint(1)?.shl(shift)
}
fn negate(value: Integer) -> Option<Integer> {
    Integer::ZERO.sub(value)
}
fn signed(value: Integer, negative: bool) -> Option<Integer> {
    if negative { negate(value) } else { Some(value) }
}
fn floor_ratio(n: Integer, d: Integer) -> Option<Integer> {
    if d <= Integer::ZERO {
        return None;
    }
    let q = n.magnitude.checked_div(d.magnitude)?;
    let rem = n.magnitude.checked_rem(d.magnitude)?;
    let q = if n.negative && !rem.is_zero() {
        q.checked_add(Magnitude::try_from(1_u8).ok()?)?
    } else {
        q
    };
    Some(Integer {
        negative: n.negative && !q.is_zero(),
        magnitude: q,
    })
}
fn compare_scaled(a: Integer, ae: i16, b: Integer, be: i16) -> Option<Ordering> {
    if a.negative != b.negative || a == Integer::ZERO || b == Integer::ZERO {
        return Some(a.cmp(&b));
    }
    let at = a.magnitude.bit_width() as i32 + i32::from(ae);
    let bt = b.magnitude.bit_width() as i32 + i32::from(be);
    if at != bt {
        return Some(if a.negative { bt.cmp(&at) } else { at.cmp(&bt) });
    }
    let e = ae.min(be);
    Some(a.shl((ae - e) as u32)?.cmp(&b.shl((be - e) as u32)?))
}
fn compare_f64(value: f64, n: Integer, d: Integer, exponent: i16) -> Option<Ordering> {
    let (c, e) = exact_f64_parts(value)?;
    compare_scaled(int(c)?.mul(d)?, e, n, exponent)
}
type Num<const N: usize> = crate::audio_engine::bounded_source_integer::BoundedInteger<N>;
fn g_int<const N: usize>(value: i128) -> Option<Num<N>> {
    Num::<N>::from_i128(value)
}
fn g_uint<const N: usize>(value: u128) -> Option<Num<N>> {
    Num::<N>::from_u128(value)
}
fn g_power<const N: usize>(shift: u32) -> Option<Num<N>> {
    g_uint(1)?.shl(shift)
}
fn g_negate<const N: usize>(value: Num<N>) -> Option<Num<N>> {
    Num::<N>::ZERO.sub(value)
}
fn g_signed<const N: usize>(value: Num<N>, negative: bool) -> Option<Num<N>> {
    if negative {
        g_negate(value)
    } else {
        Some(value)
    }
}
fn g_abs<const N: usize>(value: Num<N>) -> Num<N> {
    Num {
        negative: false,
        ..value
    }
}
fn g_floor_ratio<const N: usize>(n: Num<N>, d: Num<N>) -> Option<Num<N>> {
    if d <= Num::<N>::ZERO {
        return None;
    }
    let q = n.magnitude.checked_div(d.magnitude)?;
    let rem = n.magnitude.checked_rem(d.magnitude)?;
    let q = if n.negative && !rem.is_zero() {
        q.checked_add(bnum::Uint::<N>::try_from(1_u8).ok()?)?
    } else {
        q
    };
    Some(Num {
        negative: n.negative && !q.is_zero(),
        magnitude: q,
    })
}
fn g_residue<const N: usize>(n: Num<N>, d: Num<N>) -> Option<Num<N>> {
    let rem = n.magnitude.checked_rem(d.magnitude)?;
    let rem = if n.negative && !rem.is_zero() {
        d.magnitude.checked_sub(rem)?
    } else {
        rem
    };
    Some(Num {
        negative: false,
        magnitude: rem,
    })
}
fn g_compare_scaled<const N: usize>(a: Num<N>, ae: i16, b: Num<N>, be: i16) -> Option<Ordering> {
    if a.negative != b.negative || a == Num::<N>::ZERO || b == Num::<N>::ZERO {
        return Some(a.cmp(&b));
    }
    let at = a.magnitude.bit_width() as i32 + i32::from(ae);
    let bt = b.magnitude.bit_width() as i32 + i32::from(be);
    if at != bt {
        return Some(if a.negative { bt.cmp(&at) } else { at.cmp(&bt) });
    }
    let e = ae.min(be);
    Some(a.shl((ae - e) as u32)?.cmp(&b.shl((be - e) as u32)?))
}
fn g_compare_f64<const N: usize>(
    value: f64,
    n: Num<N>,
    d: Num<N>,
    exponent: i16,
) -> Option<Ordering> {
    let (c, e) = exact_f64_parts(value)?;
    g_compare_scaled(g_int(c)?.mul(d)?, e, n, exponent)
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}
fn slope_less(a: Axes, b: Axes) -> Option<bool> {
    Some(int(a.dx)?.mul(int(b.dy)?)? < int(b.dx)?.mul(int(a.dy)?)?)
}
fn exact_float(coefficient: Integer, exponent: i16) -> Option<f64> {
    if coefficient == Integer::ZERO {
        return Some(0.0);
    }
    let zeros = coefficient.magnitude.trailing_zeros();
    let significand = coefficient.magnitude >> zeros;
    if significand.bit_width() > 53 {
        return None;
    }
    let value =
        u64::try_from(significand).ok()? as f64 * 2.0_f64.powi(i32::from(exponent) + zeros as i32);
    value
        .is_finite()
        .then_some(if coefficient.negative { -value } else { value })
}

fn reader_period(domain: SourceLoopDomain) -> Option<MappedReaderPeriod> {
    domain
        .musical_period
        .mapped_period(domain.region)
        .or_else(|| {
            domain
                .musical_period
                .scalar_period()
                .and_then(MappedReaderPeriod::from_scalar_exact)
        })
}

impl MappedLoopGeometry {
    pub(crate) fn qualify_phase_reference(
        &self,
        reference: LoopPhaseReference,
        timeline: &TransportTimeline,
    ) -> Result<LoopPhaseReference, MappedClockError> {
        let preparation = self
            .common_phase
            .ok_or(MappedClockError::NumericalUnavailable)?;
        if preparation.grid_target.is_some() {
            // Absolute phase owns its target and actual-E projection in the epoch.
            // Retain the original launch solely as history, including Unavailable.
            return Ok(reference);
        }
        if !reference.current(timeline) {
            return Err(MappedClockError::InvalidEpoch);
        }
        Ok(reference.qualify(preparation.start))
    }

    pub(crate) fn has_absolute_grid_phase(&self) -> bool {
        self.common_phase
            .is_some_and(|preparation| preparation.grid_target.is_some())
    }

    /// Off-realtime target preparation. It never translates the map or reader domain.
    pub(crate) fn with_grid_phase_origin(
        mut self,
        origin: Option<GridPhaseOrigin>,
    ) -> Result<Self, MappedClockError> {
        if let Some(origin) = origin {
            let frame = f64::from_bits(origin.frame_bits);
            if !frame.is_finite() || frame.abs() >= (1_u64 << 53) as f64 || frame.trunc() != frame {
                return Err(MappedClockError::InvalidGeometry);
            }
        }
        self.phase_origin = origin;
        self.prepare_common_phase();
        if origin.is_some() && !self.has_common_phase() {
            return Err(MappedClockError::NumericalUnavailable);
        }
        Ok(self)
    }

    /// Optional OFFRT metadata. Failure never invalidates ordinary Ready geometry.
    pub(crate) fn prepare_common_phase(&mut self) {
        self.prepare_full_fraction();
        self.common_phase = prepare(self);
    }
    pub(crate) fn has_common_phase(&self) -> bool {
        self.common_phase.is_some()
    }
}

fn prepare(g: &MappedLoopGeometry) -> Option<CommonPhasePreparation> {
    let (mut source, mut beat) = if let Some(p) = g.general {
        p.original_grids()
    } else {
        g.exact_continuation?.compact?.original_grids()
    };
    let mut bridge_edge = None;
    let grid_target = if let Some(origin) = g.phase_origin {
        let frame = f64::from_bits(origin.frame_bits);
        let initial = GridStats::new().include_f64(frame)?.finish()?.refine(0)?;
        let raw = decode_f64_at_grid(frame, initial)?;
        let (inverse, edge) = g
            .map
            .canonical_phase_origin_segment_by_source_ordering(|v| {
                crate::audio_engine::compact_source_numerics::compare_scaled_f64_to_grid(
                    v,
                    g.loaded_rate_hz,
                    raw,
                    initial,
                )
            })?;
        bridge_edge = edge;
        source = GridStats::new()
            .include_base(source)
            .include_f64(frame)?
            .include_scaled_f64(inverse.source_start, g.loaded_rate_hz)?
            .include_scaled_f64(inverse.source_end, g.loaded_rate_hz)?
            .finish()?;
        beat = GridStats::new()
            .include_base(beat)
            .include_f64(inverse.beat_start)?
            .include_f64(inverse.beat_end)?
            .finish()?;
        Some(GridPhaseTarget { origin, inverse })
    } else {
        None
    };
    let period = reader_period(g.domain)?;
    let maximum = period.max_canonical_frame(g.domain.region.start)?;
    let recovery_exp = source.recovery_exponent(maximum);
    let sg = source.refine(0)?;
    let bg = beat.refine(0)?;
    let quarters = if let Some(q) = g.domain.musical_period.physical_quarters() {
        let (n, d) = q.parts();
        if n.magnitude.checked_rem(d.magnitude)? != Magnitude::MIN {
            return None;
        }
        n.quotient(d)?.u128()? as f64
    } else {
        g.quarters
    };
    if !(1.0..(1_u64 << 48) as f64).contains(&quarters) || quarters.trunc() != quarters {
        return None;
    }
    let q = int(decode_f64_at_grid(quarters, bg)?)?;
    let raw = decode_source_pair_at_grid(g.domain.region.start as u64, 0.0, sg)?;
    let start = g.map.canonical_segment_by_source_ordering(|v| {
        crate::audio_engine::compact_source_numerics::compare_scaled_f64_to_grid(
            v,
            g.loaded_rate_hz,
            raw,
            sg,
        )
    })?;
    let ax = Axes::new(start, g.loaded_rate_hz, sg, bg)?;
    let begin = ax.inverse(raw)?;
    let divisor = int(ax.dx)?;
    if let Some(target) = grid_target {
        let target_axes = Axes::new(target.inverse, g.loaded_rate_hz, sg, bg)?;
        // Validate only the physical edge-to-A path for a virtual target.
        // Its exterior affine phase is not claimed as source/map coverage.
        let bridge_frame = if let Some(edge) = bridge_edge {
            decode_scaled_f64_at_grid(edge, g.loaded_rate_hz, sg)?
        } else {
            decode_f64_at_grid(f64::from_bits(target.origin.frame_bits), sg)?
        };
        let target_beat = target_axes.inverse(bridge_frame)?;
        let target_divisor = int(target_axes.dx)?;
        let ordering = begin.mul(target_divisor)?.cmp(&target_beat.mul(divisor)?);
        if ordering != Ordering::Equal {
            let (left, left_d, right, right_d) = if ordering == Ordering::Less {
                (begin, divisor, target_beat, target_divisor)
            } else {
                (target_beat, target_divisor, begin, divisor)
            };
            // Usable endpoints do not establish beat count across a gap.
            g.map.visit_canonical_segments_exact(
                |v| compare_f64(v, left, left_d, bg.exponent()),
                |v| compare_f64(v, right, right_d, bg.exponent()),
                false,
                |_| Some(()),
            )?;
        }
    }
    let end = begin.add(q.mul(divisor)?)?;
    let reference = g.map.canonical_phase_reference_exact(
        |v| compare_f64(v, begin, divisor, bg.exponent()),
        |v| compare_f64(v, end, divisor, bg.exponent()),
    )?;
    let bar = reference.bar_qualified() && (quarters as u64).is_multiple_of(4);
    let downbeat = if bar { reference.downbeat() } else { 0 };
    let mut common = 0;
    let mut fmin = i16::MAX;
    let mut cmin = None;
    let mut dyadic_slopes = true;
    let mut minimum: Option<(CanonicalBeatSegment, Axes, Option<Seed>)> = None;
    let mut maximum_slope: Option<(CanonicalBeatSegment, Axes, Option<Seed>)> = None;
    g.map.visit_canonical_segments_exact(
        |v| compare_f64(v, begin, divisor, bg.exponent()),
        |v| compare_f64(v, end, divisor, bg.exponent()),
        false,
        |segment| {
            let a = Axes::new(segment, g.loaded_rate_hz, sg, bg)?;
            if let Some((odd, exponent)) = a.slope(sg.exponent(), bg.exponent()) {
                common = gcd(common, odd);
                fmin = fmin.min(exponent);
                if let Some(e) = a.intercept_exp(sg.exponent())? {
                    cmin = Some(cmin.map_or(e, |old: i16| old.min(e)));
                }
            } else {
                dyadic_slopes = false;
            }
            let seed = seed(segment, begin, end, divisor, bg)?;
            let better_seed = |new: Option<Seed>, old: Option<Seed>| {
                new.is_some_and(|n| old.is_none_or(|o| n.width_exp > o.width_exp))
            };
            if let Some((_, old, old_seed)) = minimum {
                if slope_less(a, old)? || (!slope_less(old, a)? && better_seed(seed, old_seed)) {
                    minimum = Some((segment, a, seed));
                }
            } else {
                minimum = Some((segment, a, seed));
            }
            if let Some((_, old, old_seed)) = maximum_slope {
                if slope_less(old, a)? || (!slope_less(a, old)? && better_seed(seed, old_seed)) {
                    maximum_slope = Some((segment, a, seed));
                }
            } else {
                maximum_slope = Some((segment, a, seed));
            }
            Some(())
        },
    )?;
    let (minimum, _, positive_seed) = minimum?;
    let (maximum, _, negative_seed) = maximum_slope?;
    Some(CommonPhasePreparation {
        source,
        beat,
        start,
        grid_target,
        minimum,
        maximum,
        positive_seed,
        negative_seed,
        dyadic: recovery_exp
            .filter(|_| dyadic_slopes)
            .map(|recovery_exp| DyadicRecovery {
                odd_common: common,
                minimum_slope_exp: fmin,
                intercept_exp: cmin,
                recovery_exp,
            }),
        quarters,
        downbeat,
        bar,
    })
}

/// Return optional seed without making a tiny clipped interval erase metadata.
fn seed(
    segment: CanonicalBeatSegment,
    begin: Integer,
    end: Integer,
    d: Integer,
    grid: Grid,
) -> Option<Option<Seed>> {
    let left = int(decode_f64_at_grid(segment.beat_start, grid)?)?
        .mul(d)?
        .max(begin);
    let right = int(decode_f64_at_grid(segment.beat_end, grid)?)?
        .mul(d)?
        .min(end);
    for w in (grid.exponent()..=47).rev() {
        let width = power((w - grid.exponent()) as u32)?;
        let denom = d.mul(width)?;
        let index = negate(floor_ratio(negate(left)?, denom)?)?;
        let a = index.mul(width)?;
        if a.add(width)?.mul(d)? > right {
            continue;
        }
        if let Some(start) = exact_float(a, grid.exponent()) {
            return Some(Some(Seed {
                start,
                width_exp: w,
            }));
        }
    }
    Some(None)
}

impl MappedClockEpoch {
    pub(crate) fn is_absolute_grid_phase(self) -> bool {
        phase_anchor(self).is_some_and(|anchor| anchor.grid_target.is_some())
    }
    #[cfg(test)]
    pub(crate) fn phase_grid_equivalent_for_test(
        self,
        g: &MappedLoopGeometry,
        reference: LoopPhaseReference,
        frame: u64,
    ) -> bool {
        let equal = || -> Option<bool> {
            if self.has_full_fraction_recovery() {
                return Some(false);
            }
            let a = clock::<64>(g, self, reference).ok()?.value(frame)?;
            let b = clock::<256>(g, self, reference).ok()?.value(frame)?;
            Some(
                a.numerator.resize::<256>()? == b.numerator
                    && a.denominator.resize::<256>()? == b.denominator
                    && (a.exponent, a.captured, a.region) == (b.exponent, b.captured, b.region)
                    && a.lower.and_then(|v| v.resize::<256>()) == b.lower
                    && a.upper.and_then(|v| v.resize::<256>()) == b.upper,
            )
        };
        equal() == Some(true)
    }
    #[cfg(test)]
    pub(crate) fn phase_local_payload_sizes_for_test() -> (usize, usize) {
        (
            std::mem::size_of::<GridPhaseLocalSpanView>(),
            std::mem::size_of::<FullPhaseLocalView>(),
        )
    }
    pub(crate) fn is_common_phase(self) -> bool {
        matches!(self.mode, MappedClockMode::CommonPhase(_))
    }
    pub(crate) fn has_full_fraction_recovery(self) -> bool {
        self.is_full_fraction_general()
            || phase_anchor(self).is_some_and(|a| a.config.recovery == RecoveryKind::FullFraction)
    }
    pub(crate) fn common_phase(
        marker: u64,
        timeline: TransportTimeline,
        geometry: &MappedLoopGeometry,
        raw: FractionalSourcePosition,
        execution: u64,
        reference: LoopPhaseReference,
    ) -> Result<Self, MappedClockError> {
        use crate::audio_engine::full_fraction_source_numerics as full;
        let unavailable = MappedClockError::NumericalUnavailable;
        if marker == 0 || execution == u64::MAX || raw.seek_mode != ExplicitSeekMode::Normal {
            return Err(MappedClockError::InvalidEpoch);
        }
        let p = geometry.common_phase.ok_or(unavailable)?;
        let launch_master = if p.grid_target.is_none() {
            let (master, start_inverse) = reference.mapped_parts().ok_or(unavailable)?;
            if !reference.current(&timeline) || !start_inverse.matches_exact(p.start) {
                return Err(MappedClockError::InvalidEpoch);
            }
            Some(master)
        } else {
            None
        };
        let period = reader_period(geometry.domain).ok_or(unavailable)?;
        if !period.canonical_pair(raw.frame, raw.fraction, geometry.domain.region.start) {
            return Err(unavailable);
        }
        let grid_source = p
            .source
            .refine(required_grid_f64(raw.fraction).ok_or(unavailable)?);
        let sg = grid_source
            .or_else(|| p.source.refine(0))
            .ok_or(unavailable)?;
        let (source, shift) =
            full::raw_pair(raw.frame as u64, raw.fraction, sg.exponent()).ok_or(unavailable)?;
        let inverse_segment = geometry
            .map
            .canonical_segment_by_source_ordering(|v| {
                full::compare_source(v, geometry.loaded_rate_hz, source, shift, sg)
            })
            .ok_or(unavailable)?;
        let projection = timeline
            .beat_projection()
            .ok_or(MappedClockError::InvalidEpoch)?;
        let (m0, last) = projection
            .finite_beats_for_horizon(execution, u64::MAX)
            .ok_or(unavailable)?;
        let gm = master_grid_exponent(
            projection.frames_per_beat(),
            projection.original_beat_anchor(),
        )
        .ok_or(unavailable)?;
        let mut required = gm.min(required_grid_f64(m0).ok_or(unavailable)?);
        if let Some(master) = launch_master {
            required = required.min(required_grid_f64(master).ok_or(unavailable)?);
        }
        let bg = p.beat.refine(required).ok_or(unavailable)?;
        decode_f64_at_grid(last, bg).ok_or(unavailable)?;
        let inverse =
            Axes::new(inverse_segment, geometry.loaded_rate_hz, sg, bg).ok_or(unavailable)?;
        let (i, a) = full::inverse(
            source, shift, inverse.x0, inverse.dx, inverse.y0, inverse.dy,
        )
        .ok_or(unavailable)?;
        let e0 = if i == full::Integer::ZERO {
            i16::MAX
        } else {
            bg.exponent() + i.magnitude.trailing_zeros() as i16
                - a.magnitude.trailing_zeros() as i16
        };
        let full_ready = geometry.full_fraction.is_some()
            && geometry.domain.musical_period.physical_quarters().is_none();
        let closes =
            |n: full::Integer, den: full::Integer, dyadic: DyadicRecovery| -> Option<bool> {
                let odd = den.magnitude >> den.magnitude.trailing_zeros();
                Some(
                    n.mul(full::Integer::from_u128(dyadic.odd_common)?)?
                        .magnitude
                        .checked_rem(odd)?
                        .is_zero(),
                )
            };
        let raw_grid =
            grid_source.is_some() && p.dyadic.is_some_and(|d| closes(i, a, d) == Some(true));
        // A capability choice precedes evaluation. Errors in the selected numeric
        // class are never caught and retried as a different ownership/rate path.
        let recovery = if raw_grid {
            RecoveryKind::Grid(p.dyadic.unwrap().recovery_exp as i8)
        } else {
            RecoveryKind::FullFraction
        };
        if recovery == RecoveryKind::FullFraction && !full_ready {
            return Err(unavailable);
        }
        let initial = Self {
            marker,
            projection,
            address_shift: 0,
            first_output_frame: execution,
            mode: MappedClockMode::CommonPhase(CommonPhaseAnchor {
                frame: raw.frame as u64,
                fraction: raw.fraction,
                inverse_segment,
                quarters: p.quarters,
                config: PhaseConfig {
                    delta_exp: 0,
                    source_exp: sg.exponent() as i8,
                    beat_exp: bg.exponent() as i8,
                    recovery,
                    k: 0,
                    flags: (p.downbeat << 3) | ((p.bar as u8) << 5),
                },
                grid_target: p.grid_target,
            }),
        };
        let (positive, period, target_origin, start_dx) = if recovery.grid().is_some() {
            admission_debt::<64>(initial, geometry, reference)
        } else {
            admission_debt::<256>(initial, geometry, reference)
        }
        .ok_or(unavailable)?;
        let negative = period.sub(positive).ok_or(unavailable)?;
        let choose = |grid: bool| -> Option<RecoveryKind> {
            if grid {
                Some(RecoveryKind::Grid(p.dyadic?.recovery_exp as i8))
            } else {
                full_ready.then_some(RecoveryKind::FullFraction)
            }
        };
        let with_recovery = |mut epoch: Self, recovery: RecoveryKind| {
            if let MappedClockMode::CommonPhase(ref mut a) = epoch.mode {
                a.config.recovery = recovery;
            }
            epoch
        };
        if positive == full::Integer::ZERO {
            let grid = raw_grid
                && p.dyadic
                    .is_some_and(|d| recovery_ok(p, d.minimum_slope_exp + e0.min(gm).min(0)));
            let epoch = with_recovery(initial, choose(grid).ok_or(unavailable)?);
            validate(geometry, epoch, reference)?;
            return Ok(epoch);
        }
        let c = full::Integer::from_i128(start_dx).ok_or(unavailable)?;
        let el = if target_origin == full::Integer::ZERO {
            i16::MAX
        } else {
            bg.exponent() + target_origin.magnitude.trailing_zeros() as i16
                - c.magnitude.trailing_zeros() as i16
        };
        let target_grid = p.dyadic.is_some_and(|d| {
            closes(target_origin, c, d) == Some(true)
                && recovery_ok(p, d.minimum_slope_exp + el.min(gm).min(0))
        });
        // Preserve every admitted old Grid candidate and its ordering before
        // considering the separately proved broader numeric capability.
        for full_pass in [false, true] {
            if full_pass && !full_ready || !full_pass && !(raw_grid && target_grid) {
                continue;
            }
            let choose = |grid: bool| {
                if full_pass {
                    Some(RecoveryKind::FullFraction)
                } else {
                    grid.then(|| RecoveryKind::Grid(p.dyadic.unwrap().recovery_exp as i8))
                }
            };
            for negative in if positive <= negative {
                [false, true]
            } else {
                [true, false]
            } {
                for k in 6..=64_u8 {
                    let grid = raw_grid
                        && target_grid
                        && p.dyadic.is_some_and(|d| {
                            recovery_ok(p, d.minimum_slope_exp + e0.min(gm - k as i16))
                        });
                    if let Some(recovery) = choose(grid) {
                        if rates(geometry, initial, k, negative, true).ok_or(unavailable)? {
                            let epoch = with_recovery(
                                configured(initial, GLOBAL, k, negative, 0).ok_or(unavailable)?,
                                recovery,
                            );
                            if admission_captures(epoch, geometry, reference, 0.0) {
                                validate(geometry, epoch, reference)?;
                                return Ok(epoch);
                            }
                        }
                    }
                    let Some(seed) = (if negative {
                        p.negative_seed
                    } else {
                        p.positive_seed
                    }) else {
                        continue;
                    };
                    let delta = seed.width_exp - k as i16 - if negative { 0 } else { 1 };
                    let r = (bg.exponent() - delta).max(0);
                    if r + k as i16 > 129 {
                        continue;
                    }
                    let g = e0
                        .min(gm)
                        .min(required_grid_f64(seed.start).ok_or(unavailable)?)
                        .min(delta)
                        .min(0)
                        - k as i16;
                    let grid = raw_grid
                        && target_grid
                        && r + k as i16 <= 74
                        && p.dyadic
                            .is_some_and(|d| recovery_ok(p, d.minimum_slope_exp + g));
                    let Some(recovery) = choose(grid) else {
                        continue;
                    };
                    if !rates(geometry, initial, k, negative, false).ok_or(unavailable)? {
                        continue;
                    }
                    let epoch = with_recovery(
                        configured(initial, WINDOW, k, negative, delta).ok_or(unavailable)?,
                        recovery,
                    );
                    if admission_captures(epoch, geometry, reference, seed.start) {
                        validate(geometry, epoch, reference)?;
                        return Ok(epoch);
                    }
                }
            }
        }
        Err(unavailable)
    }
    pub(crate) fn common_phase_settled(
        self,
        geometry: &MappedLoopGeometry,
        frame: u64,
        reference: LoopPhaseReference,
    ) -> Result<bool, MappedClockError> {
        if self.has_full_fraction_recovery() {
            clock::<256>(geometry, self, reference)?
                .value(frame)
                .map(|v| v.captured)
                .ok_or(MappedClockError::NumericalUnavailable)
        } else {
            clock::<64>(geometry, self, reference)?
                .value(frame)
                .map(|v| v.captured)
                .ok_or(MappedClockError::NumericalUnavailable)
        }
    }
}

fn admission_debt<const N: usize>(
    epoch: MappedClockEpoch,
    g: &MappedLoopGeometry,
    reference: LoopPhaseReference,
) -> Option<(Num<256>, Num<256>, Num<256>, i128)> {
    let core = Core::<N>::new(
        epoch,
        g.loaded_rate_hz,
        0.0,
        reference,
        g.domain.region.start as u64,
    )?;
    Some((
        core.positive_debt()?.resize::<256>()?,
        core.debt_period()?.resize::<256>()?,
        core.target_origin.resize::<256>()?,
        core.start.dx,
    ))
}
fn admission_captures(
    epoch: MappedClockEpoch,
    g: &MappedLoopGeometry,
    reference: LoopPhaseReference,
    seed: f64,
) -> bool {
    if epoch.has_full_fraction_recovery() {
        Core::<256>::new(
            epoch,
            g.loaded_rate_hz,
            seed,
            reference,
            g.domain.region.start as u64,
        )
        .and_then(|c| c.value(u64::MAX))
        .is_some_and(|v| v.captured)
    } else {
        Core::<64>::new(
            epoch,
            g.loaded_rate_hz,
            seed,
            reference,
            g.domain.region.start as u64,
        )
        .and_then(|c| c.value(u64::MAX))
        .is_some_and(|v| v.captured)
    }
}

fn configured(
    mut epoch: MappedClockEpoch,
    kind: u8,
    k: u8,
    negative: bool,
    delta: i16,
) -> Option<MappedClockEpoch> {
    let MappedClockMode::CommonPhase(ref mut a) = epoch.mode else {
        return None;
    };
    a.config.flags = (a.config.flags & !7) | kind | if negative { NEGATIVE } else { 0 };
    a.config.k = k;
    a.config.delta_exp = delta;
    a.config.valid().then_some(epoch)
}
fn recovery_ok(p: CommonPhasePreparation, source_exp: i16) -> bool {
    p.dyadic
        .is_some_and(|d| source_exp.min(d.intercept_exp.unwrap_or(i16::MAX)) >= d.recovery_exp)
}

fn rates(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    k: u8,
    negative: bool,
    global: bool,
) -> Option<bool> {
    let p = g.common_phase?;
    let a = phase_anchor(epoch)?;
    let sg = Grid::from_admitted_exponent(a.config.source_exp as i16)?;
    let bg = Grid::from_admitted_exponent(a.config.beat_exp as i16)?;
    let (fpb, fe) = exact_f64_parts(epoch.projection.frames_per_beat())?;
    let h = power(k as u32)?;
    let factor = h.add(signed(uint(1)?, negative)?)?;
    for segment in [p.minimum, p.maximum] {
        let ax = Axes::new(segment, g.loaded_rate_hz, sg, bg)?;
        let nominal = int(ax.dx)?;
        let denominator = int(ax.dy)?.mul(int(fpb)?)?;
        if compare_scaled(nominal, sg.exponent() + 1, denominator, bg.exponent() + fe)?
            == Ordering::Less
            || compare_scaled(nominal, sg.exponent(), denominator, bg.exponent() + fe + 1)?
                == Ordering::Greater
        {
            return Some(false);
        }
        let rounded = f64::from(g.loaded_rate_hz) * segment.seconds_per_quarter()
            / epoch.projection.frames_per_beat();
        if !valid_rate(rounded) {
            return Some(false);
        }
        if k != 0 && (global || segment.matches_exact(if negative { p.maximum } else { p.minimum }))
        {
            let n = nominal.mul(factor)?;
            let d = denominator.mul(h)?;
            if compare_scaled(n, sg.exponent() + 1, d, bg.exponent() + fe)? == Ordering::Less
                || compare_scaled(n, sg.exponent(), d, bg.exponent() + fe + 1)? == Ordering::Greater
            {
                return Some(false);
            }
        }
    }
    Some(true)
}

fn phase_anchor(epoch: MappedClockEpoch) -> Option<CommonPhaseAnchor> {
    let MappedClockMode::CommonPhase(a) = epoch.mode else {
        return None;
    };
    (epoch.marker != 0
        && epoch.address_shift == 0
        && a.config.valid()
        && a.quarters >= 1.0
        && a.quarters < (1_u64 << 48) as f64
        && a.quarters.trunc() == a.quarters)
        .then_some(a)
}

#[derive(Clone, Copy)]
struct Core<const N: usize = 64> {
    epoch: MappedClockEpoch,
    anchor: CommonPhaseAnchor,
    inverse_den: Num<N>,
    start: Axes,
    target_origin: Num<N>,
    initial: Num<N>,
    m0: i128,
    q: Num<N>,
    rate: u32,
    seed: f64,
}
#[derive(Clone, Copy)]
struct Value<const N: usize = 64> {
    numerator: Num<N>,
    denominator: Num<N>,
    exponent: i16,
    captured: bool,
    region: u8,
    lower: Option<Num<N>>,
    upper: Option<Num<N>>,
}
impl<const N: usize> Core<N> {
    fn new(
        epoch: MappedClockEpoch,
        rate: u32,
        seed: f64,
        reference: LoopPhaseReference,
        source_start: u64,
    ) -> Option<Self> {
        let anchor = phase_anchor(epoch)?;
        // The immutable epoch is the only target input. A different historical
        // launch reference cannot replace its captured g-side inverse.
        let target = anchor.grid_target;
        let sg = Grid::from_admitted_exponent(anchor.config.source_exp as i16)?;
        let bg = Grid::from_admitted_exponent(anchor.config.beat_exp as i16)?;
        let inverse = Axes::new(anchor.inverse_segment, rate, sg, bg)?;
        let (initial, inverse_den) = if anchor.config.recovery == RecoveryKind::FullFraction {
            let (raw, d) = crate::audio_engine::full_fraction_source_numerics::raw_pair(
                anchor.frame,
                anchor.fraction,
                sg.exponent(),
            )?;
            let (i, a) = crate::audio_engine::full_fraction_source_numerics::inverse(
                raw, d, inverse.x0, inverse.dx, inverse.y0, inverse.dy,
            )?;
            (i.resize::<N>()?, a.resize::<N>()?)
        } else {
            let raw = decode_source_pair_at_grid(anchor.frame, anchor.fraction, sg)?;
            if raw < inverse.x0 || raw > inverse.x0.checked_add(inverse.dx)? {
                return None;
            }
            (inverse.inverse(raw)?.resize::<N>()?, g_int(inverse.dx)?)
        };
        let legacy = if target.is_none() {
            Some(reference.mapped_parts()?)
        } else {
            None
        };
        let start_inverse = target
            .map(|target| target.inverse)
            .or_else(|| legacy.map(|(_, segment)| segment))?;
        let start = Axes::new(start_inverse, rate, sg, bg)?;
        let a = if let Some(target) = target {
            decode_f64_at_grid(f64::from_bits(target.origin.frame_bits), sg)?
        } else {
            decode_source_pair_at_grid(source_start, 0.0, sg)?
        };
        // Some targets passed the strict point/physical-edge policy off RT.
        // Only their requested axis may extend outside the original segment.
        if target.is_none() && (a < start.x0 || a > start.x0.checked_add(start.dx)?) {
            return None;
        }
        let target_origin = start.inverse(a)?.resize::<N>()?;
        let target_origin = if target.is_some() {
            target_origin
        } else {
            target_origin.sub(g_int(decode_f64_at_grid(legacy?.0, bg)?)?.mul(g_int(start.dx)?)?)?
        };
        let (m0, last) = epoch
            .projection
            .finite_beats_for_horizon(epoch.first_output_frame, u64::MAX)?;
        let m0 = decode_f64_at_grid(m0, bg)?;
        decode_f64_at_grid(last, bg)?;
        let q = g_int(decode_f64_at_grid(anchor.quarters, bg)?)?;
        if anchor.config.kind() == WINDOW {
            decode_f64_at_grid(seed, bg)?;
        }
        Some(Self {
            epoch,
            anchor,
            inverse_den,
            start,
            target_origin,
            initial,
            m0,
            q,
            rate,
            seed,
        })
    }
    fn delta(self, frame: u64) -> Option<Num<N>> {
        if frame < self.epoch.first_output_frame {
            return None;
        }
        let grid = Grid::from_admitted_exponent(self.anchor.config.beat_exp as i16)?;
        let d = g_int(
            decode_f64_at_grid(self.epoch.projection.beat_position_at_frame(frame), grid)?
                .checked_sub(self.m0)?,
        )?;
        (!d.negative).then_some(d)
    }
    fn debt_numerator(self) -> Option<Num<N>> {
        let a = self.inverse_den;
        let c = g_int(self.start.dx)?;
        self.target_origin
            .mul(a)?
            .add(g_int(self.m0)?.mul(a)?.mul(c)?)?
            .sub(self.initial.mul(c)?)
    }
    fn correction_period(self) -> Option<Num<N>> {
        if self.anchor.grid_target.is_none() {
            return Some(self.q);
        }
        // An explicit source grid asks for beat phase, not a whole-loop phrase
        // position. Positive integer Q is already required by phase admission.
        // Keep Q for physical folding and the bounded correction-window schedule.
        let grid = Grid::from_admitted_exponent(self.anchor.config.beat_exp as i16)?;
        g_int(decode_f64_at_grid(1.0, grid)?)
    }
    fn debt_period(self) -> Option<Num<N>> {
        self.correction_period()?
            .mul(self.inverse_den)?
            .mul(g_int(self.start.dx)?)
    }
    fn positive_debt(self) -> Option<Num<N>> {
        g_residue(self.debt_numerator()?, self.debt_period()?)
    }
    fn debt(self) -> Option<Num<N>> {
        let p = self.positive_debt()?;
        if self.anchor.config.negative() {
            self.debt_period()?.sub(p)
        } else {
            Some(p)
        }
    }
    fn value(self, frame: u64) -> Option<Value<N>> {
        self.value_with_correction(
            frame,
            |r, h| self.debt()?.shl(r)?.mul(h),
            || {
                g_floor_ratio(self.debt_numerator()?, self.debt_period()?)?
                    .add(g_uint(u128::from(self.anchor.config.negative()))?)
            },
        )
    }
    fn value_with_correction(
        self,
        frame: u64,
        correction_cap: impl FnOnce(u32, Num<N>) -> Option<Num<N>>,
        captured_winding: impl FnOnce() -> Option<Num<N>>,
    ) -> Option<Value<N>> {
        let config = self.anchor.config;
        let b = config.beat_exp as i16;
        let exponent = config.fine();
        let r = (b - exponent) as u32;
        let h = g_power(config.k as u32)?;
        let a = self.inverse_den;
        let initial = self.initial.shl(r)?;
        let delta = self.delta(frame)?.shl(r)?;
        let base = initial.add(delta.mul(a)?)?.mul(h)?;
        let denominator = a.mul(h)?;
        if config.kind() == CAPTURED {
            return Some(Value {
                numerator: base,
                denominator,
                exponent,
                captured: true,
                region: 3,
                lower: None,
                upper: None,
            });
        }
        let cap = correction_cap(r, h)?;
        let (uncapped, region, lower, upper) = if config.kind() == GLOBAL {
            (g_signed(delta.mul(a)?, config.negative())?, 1, None, None)
        } else {
            let bg = Grid::from_admitted_exponent(b)?;
            let mut entry = g_int(decode_f64_at_grid(self.seed, bg)?)?;
            if self.initial > entry.mul(a)? {
                entry = entry.add(self.q)?;
            }
            let entry = entry.shl(r)?;
            let wait = entry.mul(a)?.sub(initial)?;
            if wait.negative {
                return None;
            }
            let v = delta.mul(a)?.sub(wait)?;
            if v.negative {
                return Some(Value {
                    numerator: base,
                    denominator,
                    exponent,
                    captured: false,
                    region: 0,
                    lower: None,
                    upper: Some(wait),
                });
            }
            let q = self.q.shl(r)?;
            let step = g_power((config.delta_exp - exponent) as u32)?;
            let period = q.sub(g_signed(step, config.negative())?)?;
            if period <= Num::<N>::ZERO {
                return None;
            }
            let divisor = period.mul(a)?;
            let j = g_floor_ratio(v, divisor)?;
            let j128 = j.u128()?;
            let rem = g_residue(v, divisor)?;
            let width = h.mul(step)?.mul(a)?;
            let correcting = rem < width;
            let factor = h.add(g_signed(g_uint(1)?, config.negative())?)?;
            let x = if correcting {
                factor.mul(rem)?
            } else {
                rem.add(g_signed(step.mul(a)?, config.negative())?)?
                    .mul(h)?
            };
            let beta = entry.add(q.mul(g_uint(j128)?)?)?.mul(denominator)?.add(x)?;
            let cycle = j.mul(divisor)?;
            let lo = wait
                .add(cycle)?
                .add(if correcting { Num::<N>::ZERO } else { width })?;
            let hi = wait
                .add(cycle)?
                .add(if correcting { width } else { divisor })?;
            (
                beta.sub(base)?,
                if correcting { 1 } else { 2 },
                Some(lo),
                Some(hi),
            )
        };
        if uncapped != Num::<N>::ZERO && uncapped.negative != config.negative() {
            return None;
        }
        let captured = g_abs(uncapped).mul(g_int(self.start.dx)?)? >= cap;
        if captured {
            // Cancel the raw inverse symbolically. Never round a new anchor or
            // multiply both inverse denominators into the source reader path.
            let winding = captured_winding()?;
            let master = g_int(self.m0)?.add(self.delta(frame)?)?;
            let c = g_int(self.start.dx)?;
            return Some(Value {
                numerator: self.target_origin.add(
                    master
                        .sub(winding.mul(self.correction_period()?)?)?
                        .mul(c)?,
                )?,
                denominator: c,
                exponent: b,
                captured: true,
                region: 3,
                lower: None,
                upper: None,
            });
        }
        Some(Value {
            numerator: base.add(uncapped)?,
            denominator,
            exponent,
            captured,
            region: if captured { 3 } else { region },
            lower: if captured { None } else { lower },
            upper: if captured { None } else { upper },
        })
    }
    fn at_least_time(self, frame: u64, target: Num<N>) -> Option<bool> {
        let r = (self.anchor.config.beat_exp as i16 - self.anchor.config.fine()) as u32;
        Some(self.delta(frame)?.shl(r)?.mul(self.inverse_den)? >= target)
    }
    fn position(
        self,
        segment: CanonicalBeatSegment,
        cycle: u128,
        domain: SourceLoopDomain,
        frame: u64,
    ) -> Option<FractionalSourcePosition> {
        let (n, d) = self.source_terms(segment, cycle, frame)?;
        let coordinate = match self.anchor.config.recovery {
            RecoveryKind::Grid(e) => split_recovery_coordinate(
                n.resize::<64>()?,
                d.resize::<64>()?,
                e as i16,
                self.anchor.fraction,
                domain,
            )?,
            RecoveryKind::FullFraction => {
                crate::audio_engine::full_fraction_source_numerics::split(
                    n,
                    d,
                    self.anchor.fraction,
                    domain,
                    false,
                )?
            }
        };
        Some(FractionalSourcePosition {
            frame: usize::try_from(coordinate.whole).ok()?,
            fraction: coordinate.fraction,
            seek_mode: ExplicitSeekMode::Normal,
        })
    }

    /// The original unrounded rational, before source RN or canonical carry.
    fn source_terms(
        self,
        segment: CanonicalBeatSegment,
        cycle: u128,
        frame: u64,
    ) -> Option<(Num<N>, Num<N>)> {
        let value = self.value(frame)?;
        let b = self.anchor.config.beat_exp as i16;
        let r = (b - value.exponent) as u32;
        let sg = Grid::from_admitted_exponent(self.anchor.config.source_exp as i16)?;
        let bg = Grid::from_admitted_exponent(b)?;
        let axes = Axes::new(segment, self.rate, sg, bg)?;
        let cycle = g_uint(cycle)?.shl(r)?;
        let canonical = value.numerator.sub(cycle.mul(value.denominator)?)?;
        let offset = canonical.sub(g_int(axes.y0)?.shl(r)?.mul(value.denominator)?)?;
        let d = value.denominator.mul(g_int(axes.dy)?)?.shl(r)?;
        let n = g_int(axes.x0)?.mul(d)?.add(offset.mul(g_int(axes.dx)?)?)?;
        let d = d.shl((-sg.exponent()) as u32)?;
        Some((n, d))
    }
}

/// Immutable arithmetic reuse for ONE FullFraction refresh, never a source permit.
/// E<2^(384+d), P<2^(380+d), d<=1074 and r+k<=129 give cap<=1583
/// bits and winding<=1460 bits. These two values stay outside every copied clock
/// and local reader payload. Grid continues to evaluate its original512-bit law.
struct FullRefreshEvaluator<'a> {
    core: &'a Core<256>,
    cap: Option<Num<256>>,
    winding: Option<Num<256>>,
}
impl<'a> FullRefreshEvaluator<'a> {
    fn new(core: &'a Core<256>) -> Self {
        let config = core.anchor.config;
        if config.kind() == CAPTURED {
            // The original shortcut has no debt or winding precondition.
            return Self {
                core,
                cap: None,
                winding: None,
            };
        }
        let numerator = core.debt_numerator();
        let period = core.debt_period();
        let positive = numerator.zip(period).and_then(|(n, p)| g_residue(n, p));
        let debt = if config.negative() {
            period
                .zip(positive)
                .and_then(|(p, positive)| p.sub(positive))
        } else {
            positive
        };
        let r = (config.beat_exp as i16 - config.fine()) as u32;
        let cap = debt.and_then(|v| v.shl(r)?.mul(g_power(config.k as u32)?));
        let winding = numerator
            .zip(period)
            .and_then(|(n, p)| g_floor_ratio(n, p)?.add(g_uint(u128::from(config.negative()))?));
        // Keep failures as values: winding is required only after capture, and
        // neither cache entry may reject a frame before its original use site.
        Self { core, cap, winding }
    }
    fn value(&self, frame: u64) -> Option<Value<256>> {
        self.core
            .value_with_correction(frame, |_, _| self.cap, || self.winding)
    }
}

#[cfg(test)]
pub(super) fn assert_full_refresh_equivalent_for_test(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    reference: LoopPhaseReference,
    frames: &[u64],
    boundaries: &[u64],
) {
    assert!(epoch.has_full_fraction_recovery());
    assert!(std::mem::size_of::<FullRefreshEvaluator<'_>>() <= 544);
    assert!(!std::mem::needs_drop::<FullRefreshEvaluator<'_>>());
    let core = clock::<256>(g, epoch, reference).unwrap();
    let evaluator = FullRefreshEvaluator::new(&core);
    let fields = |value: Option<Value<256>>| {
        value.map(|v| {
            (
                v.numerator,
                v.denominator,
                v.exponent,
                v.captured,
                v.region,
                v.lower,
                v.upper,
            )
        })
    };
    if core.anchor.config.kind() == CAPTURED {
        assert!(evaluator.cap.is_none() && evaluator.winding.is_none());
    } else {
        assert!(evaluator.cap.unwrap().magnitude.bit_width() <= 1583);
        assert!(evaluator.winding.unwrap().magnitude.bit_width() <= 1460);
    }
    for &frame in frames {
        let original = core.value(frame);
        assert_eq!(
            fields(evaluator.value(frame)),
            fields(original),
            "frame={frame}"
        );
        if core.anchor.config.kind() == CAPTURED {
            assert_eq!(
                fields(core.value_with_correction(
                    frame,
                    |_, _| panic!("CAPTURED must not require debt"),
                    || panic!("CAPTURED must not require winding"),
                )),
                fields(original),
            );
        } else {
            // A cached failure is consumed only at its original use site. In
            // particular a missing winding cannot reject a still-correcting or
            // waiting frame; it does reject the first frame that needs capture.
            let deferred = FullRefreshEvaluator {
                core: &core,
                cap: evaluator.cap,
                winding: None,
            };
            let expected = original.filter(|v| !v.captured);
            assert_eq!(
                fields(deferred.value(frame)),
                fields(expected),
                "frame={frame}"
            );
            let missing_cap = FullRefreshEvaluator {
                core: &core,
                cap: None,
                winding: evaluator.winding,
            };
            assert!(missing_cap.value(frame).is_none());
        }
    }
    for &frame in boundaries {
        // Original search bounds/predicates, original endpoint construction and
        // every copied span field must agree, including rejected queries.
        match (
            refresh_full(g, epoch, frame, reference),
            refresh_typed::<256>(g, epoch, frame, reference),
        ) {
            (Ok(cached), Ok(original)) => assert!(cached.matches_exact(original), "frame={frame}"),
            (Err(cached), Err(original)) => assert_eq!(cached, original, "frame={frame}"),
            _ => panic!("cached/original refresh admission differs at frame={frame}"),
        }
    }
}

pub(super) fn validate(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    reference: LoopPhaseReference,
) -> Result<(), MappedClockError> {
    if epoch.has_full_fraction_recovery() {
        validate_typed::<256>(g, epoch, reference)
    } else {
        validate_typed::<64>(g, epoch, reference)
    }
}
pub(super) fn position(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
    reference: LoopPhaseReference,
) -> Result<FractionalSourcePosition, MappedClockError> {
    if epoch.has_full_fraction_recovery() {
        position_typed::<256>(g, epoch, frame, reference)
    } else {
        position_typed::<64>(g, epoch, frame, reference)
    }
}
pub(super) fn refresh(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
    reference: LoopPhaseReference,
) -> Result<MappedSourceSpan, MappedClockError> {
    if epoch.has_full_fraction_recovery() {
        refresh_full(g, epoch, frame, reference)
    } else {
        refresh_typed::<64>(g, epoch, frame, reference)
    }
}
fn clock<const N: usize>(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    reference: LoopPhaseReference,
) -> Result<Core<N>, MappedClockError> {
    let unavailable = MappedClockError::NumericalUnavailable;
    let p = g.common_phase.ok_or(unavailable)?;
    let a = phase_anchor(epoch).ok_or(unavailable)?;
    if a.config.recovery == RecoveryKind::FullFraction
        && (g.full_fraction.is_none() || g.domain.musical_period.physical_quarters().is_some())
    {
        return Err(unavailable);
    }
    if p.grid_target.is_none()
        && !reference
            .mapped_parts()
            .is_some_and(|(_, start)| start.matches_exact(p.start))
    {
        return Err(unavailable);
    }
    if a.quarters.to_bits() != p.quarters.to_bits()
        || !same_grid_target(a.grid_target, p.grid_target)
        || a.config.downbeat() != p.downbeat
        || a.config.bar_qualified() != p.bar
        || a.config
            .recovery
            .grid()
            .is_some_and(|e| p.dyadic.is_none_or(|d| e as i16 != d.recovery_exp))
    {
        return Err(unavailable);
    }
    if !g
        .map
        .canonical_segment_at_beat(a.inverse_segment.beat_start)
        .is_some_and(|s| s.matches_exact(a.inverse_segment))
    {
        return Err(unavailable);
    }
    let seed = if a.config.kind() == WINDOW {
        (if a.config.negative() {
            p.negative_seed
        } else {
            p.positive_seed
        })
        .ok_or(unavailable)?
        .start
    } else {
        0.0
    };
    Core::<N>::new(
        epoch,
        g.loaded_rate_hz,
        seed,
        reference,
        g.domain.region.start as u64,
    )
    .ok_or(unavailable)
}
fn validate_typed<const N: usize>(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    reference: LoopPhaseReference,
) -> Result<(), MappedClockError> {
    let core = clock::<N>(g, epoch, reference)?;
    let unavailable = MappedClockError::NumericalUnavailable;
    if !rates(g, epoch, 0, false, false).ok_or(unavailable)? {
        return Err(MappedClockError::UnsupportedRate);
    }
    core.value(epoch.first_output_frame).ok_or(unavailable)?;
    core.value(u64::MAX).ok_or(unavailable)?;
    let observed = position_typed::<N>(g, epoch, epoch.first_output_frame, reference)?;
    if observed.frame != core.anchor.frame as usize
        || observed.fraction.to_bits() != core.anchor.fraction.to_bits()
    {
        return Err(unavailable);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Target<const N: usize> {
    n: Num<N>,
    d: Num<N>,
    e: i16,
}
fn reached<const N: usize>(v: Value<N>, target: Target<N>) -> Option<bool> {
    g_compare_scaled(
        v.numerator.mul(target.d)?,
        v.exponent,
        target.n.mul(v.denominator)?,
        target.e,
    )
    .map(|o| o != Ordering::Less)
}
fn first(mut lo: u64, mut hi: u64, predicate: impl Fn(u64) -> Option<bool>) -> Option<u64> {
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if predicate(mid)? {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Some(lo)
}
fn source_phase<const N: usize>(
    g: &MappedLoopGeometry,
    core: Core<N>,
    frame: u64,
) -> Option<(Value<N>, Num<N>, Num<N>, u128, CanonicalBeatSegment)> {
    let value = core.value(frame)?;
    source_phase_value(g, core, value)
}
fn source_phase_value<const N: usize>(
    g: &MappedLoopGeometry,
    core: Core<N>,
    value: Value<N>,
) -> Option<(Value<N>, Num<N>, Num<N>, u128, CanonicalBeatSegment)> {
    let config = core.anchor.config;
    let sg = Grid::from_admitted_exponent(config.source_exp as i16)?;
    let bg = Grid::from_admitted_exponent(config.beat_exp as i16)?;
    let start = Axes::new(g.common_phase?.start, g.loaded_rate_hz, sg, bg)?;
    let raw = decode_source_pair_at_grid(g.domain.region.start as u64, 0.0, sg)?;
    let j = start.inverse(raw)?.resize::<N>()?;
    let c = g_int(start.dx)?;
    let r = (bg.exponent() - value.exponent) as u32;
    let unfolded = value
        .numerator
        .mul(c)?
        .sub(j.shl(r)?.mul(value.denominator)?)?;
    let period = core.q.shl(r)?.mul(c)?.mul(value.denominator)?;
    let cycle = unfolded.quotient(period)?.mul(core.q)?.u128()?;
    let canonical = value
        .numerator
        .sub(g_uint(cycle)?.shl(r)?.mul(value.denominator)?)?;
    let segment = g.map.canonical_segment_by_beat_ordering(|v| {
        g_compare_f64(v, canonical, value.denominator, value.exponent)
    })?;
    Some((value, j, c, cycle, segment))
}
fn position_typed<const N: usize>(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
    reference: LoopPhaseReference,
) -> Result<FractionalSourcePosition, MappedClockError> {
    let core = clock::<N>(g, epoch, reference)?;
    let (_, _, _, cycle, segment) =
        source_phase(g, core, frame).ok_or(MappedClockError::NumericalUnavailable)?;
    core.position(segment, cycle, g.domain, frame)
        .ok_or(MappedClockError::NumericalUnavailable)
}
fn refresh_typed<const N: usize>(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
    reference: LoopPhaseReference,
) -> Result<MappedSourceSpan, MappedClockError> {
    let unavailable = MappedClockError::NumericalUnavailable;
    if frame == u64::MAX {
        return Err(unavailable);
    }
    let core = clock::<N>(g, epoch, reference)?;
    refresh_core(g, epoch, frame, reference, core, |n| core.value(n))
}
fn refresh_full(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
    reference: LoopPhaseReference,
) -> Result<MappedSourceSpan, MappedClockError> {
    if frame == u64::MAX {
        return Err(MappedClockError::NumericalUnavailable);
    }
    let core = clock::<256>(g, epoch, reference)?;
    let evaluator = FullRefreshEvaluator::new(&core);
    refresh_core(g, epoch, frame, reference, core, |n| evaluator.value(n))
}
fn refresh_core<const N: usize>(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
    reference: LoopPhaseReference,
    core: Core<N>,
    evaluate: impl Fn(u64) -> Option<Value<N>>,
) -> Result<MappedSourceSpan, MappedClockError> {
    let unavailable = MappedClockError::NumericalUnavailable;
    let (value, j, c, cycle, segment) =
        source_phase_value(g, core, evaluate(frame).ok_or(unavailable)?).ok_or(unavailable)?;
    let bg = Grid::from_admitted_exponent(core.anchor.config.beat_exp as i16).ok_or(unavailable)?;
    let loop_left = Target {
        n: j.add(g_uint(cycle).and_then(|v| v.mul(c)).ok_or(unavailable)?)
            .ok_or(unavailable)?,
        d: c,
        e: bg.exponent(),
    };
    let loop_right = Target {
        n: loop_left
            .n
            .add(core.q.mul(c).ok_or(unavailable)?)
            .ok_or(unavailable)?,
        ..loop_left
    };
    let knot_left = Target {
        n: g_int(decode_f64_at_grid(segment.beat_start, bg).ok_or(unavailable)?)
            .and_then(|v| v.add(g_uint(cycle)?))
            .ok_or(unavailable)?,
        d: g_uint(1).ok_or(unavailable)?,
        e: bg.exponent(),
    };
    let knot_right = Target {
        n: g_int(decode_f64_at_grid(segment.beat_end, bg).ok_or(unavailable)?)
            .and_then(|v| v.add(g_uint(cycle)?))
            .ok_or(unavailable)?,
        ..knot_left
    };
    let less =
        |a: Target<N>, b: Target<N>| g_compare_scaled(a.n.mul(b.d)?, a.e, b.n.mul(a.d)?, b.e);
    let left = if less(loop_left, knot_left).ok_or(unavailable)? == Ordering::Less {
        knot_left
    } else {
        loop_left
    };
    let right = if less(loop_right, knot_right).ok_or(unavailable)? == Ordering::Greater {
        knot_right
    } else {
        loop_right
    };
    let mut start = first(epoch.first_output_frame, frame, |n| {
        reached(evaluate(n)?, left)
    })
    .ok_or(unavailable)?;
    let mut end = if reached(evaluate(u64::MAX).ok_or(unavailable)?, right).ok_or(unavailable)? {
        first(frame + 1, u64::MAX, |n| reached(evaluate(n)?, right)).ok_or(unavailable)?
    } else {
        u64::MAX
    };
    if value.captured {
        start = start.max(
            first(epoch.first_output_frame, frame, |n| {
                Some(evaluate(n)?.captured)
            })
            .ok_or(unavailable)?,
        );
    } else {
        if let Some(target) = value.lower {
            start = start.max(
                first(epoch.first_output_frame, frame, |n| {
                    core.at_least_time(n, target)
                })
                .ok_or(unavailable)?,
            );
        }
        if let Some(target) = value.upper {
            if core.at_least_time(u64::MAX, target).ok_or(unavailable)? {
                end = end.min(
                    first(frame + 1, u64::MAX, |n| core.at_least_time(n, target))
                        .ok_or(unavailable)?,
                );
            }
        }
        if evaluate(u64::MAX).ok_or(unavailable)?.captured {
            end = end.min(
                first(frame + 1, u64::MAX, |n| Some(evaluate(n)?.captured)).ok_or(unavailable)?,
            );
        }
    }
    if start > frame || end <= frame {
        return Err(unavailable);
    }
    let endpoint = position_typed::<N>(g, epoch, end, reference)?;
    Ok(MappedSourceSpan {
        epoch,
        segment,
        program: SpanProgram::phase(cycle, bg.exponent(), core.seed, value.region),
        loaded_rate_hz: g.loaded_rate_hz,
        start_frame: start,
        end_frame: end,
        endpoint_frame: endpoint.frame,
        endpoint_fraction: endpoint.fraction,
        source_start: g.domain.region.start as u64,
    })
}

pub(crate) enum PhaseLocalSpanView {
    Grid(GridPhaseLocalSpanView),
    Full(FullPhaseLocalView),
}
impl PhaseLocalSpanView {
    #[cfg(test)]
    pub(crate) fn full_fraction_split_bits_for_test(&self) -> Option<usize> {
        match self {
            Self::Grid(_) => None,
            Self::Full(view) => Some(view.affine.split_bits_for_test()),
        }
    }
    pub(crate) fn position_at(&self, frame: u64) -> Option<FractionalSourcePosition> {
        match self {
            Self::Grid(v) => v.position_at(frame),
            Self::Full(v) => v.position_at(frame),
        }
    }
    #[cfg(test)]
    pub(crate) fn uses_affine_for_test(&self) -> bool {
        match self {
            Self::Grid(v) => v.affine.is_some(),
            Self::Full(_) => true,
        }
    }
}
pub(crate) struct FullPhaseLocalView {
    span: MappedSourceSpan,
    beat: Grid,
    affine: crate::audio_engine::full_fraction_source_numerics::RationalAffine,
}
impl FullPhaseLocalView {
    fn new(
        span: MappedSourceSpan,
        domain: SourceLoopDomain,
        reference: LoopPhaseReference,
    ) -> Option<Self> {
        let (cycle, b, seed, region) = span.program.phase_value()?;
        let core = Core::<256>::new(
            span.epoch,
            span.loaded_rate_hz,
            seed,
            reference,
            span.source_start,
        )?;
        if core.anchor.config.recovery != RecoveryKind::FullFraction
            || domain.musical_period.physical_quarters().is_some()
            || !domain.is_valid()
            || span.source_start != domain.region.start as u64
            || span.start_frame < span.epoch.first_output_frame
            || span.start_frame >= span.end_frame
            || b != core.anchor.config.beat_exp as i16
        {
            return None;
        }
        let start = core.value(span.start_frame)?;
        let last = core.value(span.end_frame - 1)?;
        if start.region != region || start.region != last.region || start.captured != last.captured
        {
            return None;
        }
        if let Some(upper) = start.upper {
            if core.at_least_time(span.end_frame - 1, upper)? {
                return None;
            }
        }
        if !inside_held_segment(core, span.segment, cycle, start)?
            || !inside_held_segment(core, span.segment, cycle, last)?
        {
            return None;
        }
        let (n, d) = core.source_terms(span.segment, cycle, span.start_frame)?;
        let sg = Grid::from_admitted_exponent(core.anchor.config.source_exp as i16)?;
        let beat = Grid::from_admitted_exponent(b)?;
        let axes = Axes::new(span.segment, span.loaded_rate_hz, sg, beat)?;
        let r = (b - start.exponent) as u32;
        let k = if start.region == 1 {
            core.anchor.config.k
        } else {
            0
        };
        let h = g_power::<256>(k as u32)?;
        let factor = if k == 0 {
            g_uint(1)?
        } else {
            h.add(g_signed(g_uint(1)?, core.anchor.config.negative())?)?
        };
        let scaled = start
            .denominator
            .mul(factor)?
            .shl(r)?
            .mul(g_int(axes.dx)?)?;
        if !scaled.magnitude.checked_rem(h.magnitude)?.is_zero() {
            return None;
        }
        let multiplier = scaled.quotient(h)?;
        let master =
            |frame| decode_f64_at_grid(span.epoch.projection.beat_position_at_frame(frame), beat);
        let affine = crate::audio_engine::full_fraction_source_numerics::RationalAffine::new(
            n,
            multiplier,
            d,
            master(span.start_frame)?,
            master(span.end_frame - 1)?,
            core.anchor.fraction,
            domain,
            false,
        )?;
        if !reader_period(domain)?.canonical_pair(
            span.endpoint_frame,
            span.endpoint_fraction,
            domain.region.start,
        ) {
            return None;
        }
        Some(Self { span, beat, affine })
    }
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
            self.span.epoch.projection.beat_position_at_frame(frame),
            self.beat,
        )?)
    }
}

/// A nonowning temporary local view; all grids are copied admitted originals.
pub(crate) struct GridPhaseLocalSpanView {
    segment: CanonicalBeatSegment,
    start_frame: u64,
    end_frame: u64,
    endpoint_frame: usize,
    endpoint_fraction: f64,
    core: Core,
    cycle: u128,
    domain: SourceLoopDomain,
    affine: Option<PreparedAffine>,
}

/// Exact ephemeral coordinates on one already checked source/control interval.
/// Rate+recovery prove multiplier<2^75 and every interior result<2^126.
/// No rounded pair, source offset or incremental time becomes an anchor.
struct PreparedAffine {
    source_start: u128,
    master_start: i128,
    master_grid: Grid,
    multiplier: u128,
    split: PreparedRecoverySplit,
}
impl PreparedAffine {
    fn new(
        span: MappedSourceSpan,
        core: Core,
        cycle: u128,
        domain: SourceLoopDomain,
    ) -> Option<Self> {
        let last = span.end_frame.checked_sub(1)?;
        let start_value = core.value(span.start_frame)?;
        let last_value = core.value(last)?;
        if start_value.captured != last_value.captured {
            return None;
        }
        if let Some(target) = start_value.upper {
            if core.at_least_time(last, target)? {
                return None;
            }
        }
        if !inside_held_segment(core, span.segment, cycle, start_value)?
            || !inside_held_segment(core, span.segment, cycle, last_value)?
        {
            return None;
        }
        // The original split checks exact membership before its RN/seam carry.
        // These optional endpoint checks do not replace any reader preflight.
        core.position(span.segment, cycle, domain, span.start_frame)?;
        core.position(span.segment, cycle, domain, last)?;
        let config = core.anchor.config;
        let master_exp = master_grid_exponent(
            span.epoch.projection.frames_per_beat(),
            span.epoch.projection.original_beat_anchor(),
        )?;
        let master_grid = Grid::from_admitted_exponent(master_exp)?;
        let master_start = decode_f64_at_grid(
            span.epoch
                .projection
                .beat_position_at_frame(span.start_frame),
            master_grid,
        )?;
        let source_grid = Grid::from_admitted_exponent(config.source_exp as i16)?;
        let beat_grid = Grid::from_admitted_exponent(config.beat_exp as i16)?;
        let axes = Axes::new(span.segment, span.loaded_rate_hz, source_grid, beat_grid)?;
        let (odd, slope_exp) = axes.slope(source_grid.exponent(), beat_grid.exponent())?;
        let correcting = start_value.region == 1;
        let k = if correcting { config.k } else { 0 };
        let factor = if correcting {
            let h = 1_u128.checked_shl(u32::from(k))?;
            if config.negative() {
                h.checked_sub(1)?
            } else {
                h.checked_add(1)?
            }
        } else {
            1
        };
        let shift = master_exp
            .checked_add(slope_exp)?
            .checked_sub(i16::from(k))?
            .checked_sub(i16::from(config.recovery.grid()?))?;
        if !(0..=74).contains(&shift) {
            return None;
        }
        let product = odd.checked_mul(factor)?;
        if product == 0 || (128 - product.leading_zeros()).checked_add(shift as u32)? > 75 {
            return None;
        }
        let multiplier = product.checked_shl(shift as u32)?;
        let (n, d) = core.source_terms(span.segment, cycle, span.start_frame)?;
        let source_start = normalize_recovery_coefficient(n, d, config.recovery.grid()? as i16)?;
        let split = PreparedRecoverySplit::new(
            domain,
            config.recovery.grid()? as i16,
            core.anchor.fraction,
        )?;
        Some(Self {
            source_start,
            master_start,
            master_grid,
            multiplier,
            split,
        })
    }

    fn position(&self, core: &Core, frame: u64) -> Option<FractionalSourcePosition> {
        // Decode separately: binary64 subtraction here would lose actual M bits.
        let master = decode_f64_at_grid(
            core.epoch.projection.beat_position_at_frame(frame),
            self.master_grid,
        )?;
        let delta = u128::try_from(master.checked_sub(self.master_start)?).ok()?;
        let coefficient = self
            .source_start
            .checked_add(delta.checked_mul(self.multiplier)?)?;
        if coefficient >= 1_u128 << 126 {
            return None;
        }
        let coordinate = self.split.split(coefficient)?;
        Some(FractionalSourcePosition {
            frame: usize::try_from(coordinate.whole).ok()?,
            fraction: coordinate.fraction,
            seek_mode: ExplicitSeekMode::Normal,
        })
    }
}

fn inside_held_segment<const N: usize>(
    core: Core<N>,
    segment: CanonicalBeatSegment,
    cycle: u128,
    value: Value<N>,
) -> Option<bool> {
    let b = core.anchor.config.beat_exp as i16;
    let r = u32::try_from(b.checked_sub(value.exponent)?).ok()?;
    let grid = Grid::from_admitted_exponent(b)?;
    let canonical = value
        .numerator
        .sub(g_uint(cycle)?.shl(r)?.mul(value.denominator)?)?;
    let left = g_int(decode_f64_at_grid(segment.beat_start, grid)?)?
        .shl(r)?
        .mul(value.denominator)?;
    let right = g_int(decode_f64_at_grid(segment.beat_end, grid)?)?
        .shl(r)?
        .mul(value.denominator)?;
    Some(canonical >= left && canonical < right)
}

impl GridPhaseLocalSpanView {
    #[cfg(test)]
    pub(crate) fn uses_affine_for_test(&self) -> bool {
        self.affine.is_some()
    }

    pub(crate) fn position_at(&self, frame: u64) -> Option<FractionalSourcePosition> {
        if frame == self.end_frame {
            return Some(FractionalSourcePosition {
                frame: self.endpoint_frame,
                fraction: self.endpoint_fraction,
                seek_mode: ExplicitSeekMode::Normal,
            });
        }
        (self.start_frame <= frame && frame < self.end_frame).then_some(())?;
        if let Some(position) = self
            .affine
            .as_ref()
            .and_then(|affine| affine.position(&self.core, frame))
        {
            return Some(position);
        }
        self.core
            .position(self.segment, self.cycle, self.domain, frame)
    }
}
impl MappedSourceSpan {
    pub(crate) fn local_phase_view(
        self,
        domain: SourceLoopDomain,
        reference: LoopPhaseReference,
    ) -> Result<PhaseLocalSpanView, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        if self.epoch.has_full_fraction_recovery() {
            return FullPhaseLocalView::new(self, domain, reference)
                .map(PhaseLocalSpanView::Full)
                .ok_or(unavailable);
        }
        let (cycle, b, seed, region) = self.program.phase_value().ok_or(unavailable)?;
        let anchor = phase_anchor(self.epoch).ok_or(unavailable)?;
        if !domain.is_valid()
            || self.source_start != domain.region.start as u64
            || self.start_frame < self.epoch.first_output_frame
            || self.start_frame >= self.end_frame
            || b != anchor.config.beat_exp as i16
        {
            return Err(unavailable);
        }
        let core = Core::new(
            self.epoch,
            self.loaded_rate_hz,
            seed,
            reference,
            self.source_start,
        )
        .ok_or(unavailable)?;
        if core.value(self.start_frame).ok_or(unavailable)?.region != region {
            return Err(unavailable);
        }
        let period = reader_period(domain).ok_or(unavailable)?;
        if !period.canonical_pair(
            self.endpoint_frame,
            self.endpoint_fraction,
            domain.region.start,
        ) {
            return Err(unavailable);
        }
        let view = GridPhaseLocalSpanView {
            segment: self.segment,
            start_frame: self.start_frame,
            end_frame: self.end_frame,
            endpoint_frame: self.endpoint_frame,
            endpoint_fraction: self.endpoint_fraction,
            core,
            cycle,
            domain,
            affine: PreparedAffine::new(self, core, cycle, domain),
        };
        view.position_at(self.start_frame).ok_or(unavailable)?;
        Ok(PhaseLocalSpanView::Grid(view))
    }
}
pub(super) fn span_rate(span: MappedSourceSpan) -> Option<f64> {
    let (_, _, _, region) = span.program.phase_value()?;
    let a = phase_anchor(span.epoch)?;
    let rate = f64::from(span.loaded_rate_hz) * span.segment.seconds_per_quarter()
        / span.epoch.projection.frames_per_beat();
    let factor = if region == 1 {
        1.0 + if a.config.negative() {
            -2.0_f64.powi(-(a.config.k as i32))
        } else {
            2.0_f64.powi(-(a.config.k as i32))
        }
    } else {
        1.0
    };
    let rate = rate * factor;
    valid_rate(rate).then_some(rate)
}
