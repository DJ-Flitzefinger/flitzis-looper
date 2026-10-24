//! Recovery-closed original-map convergence on the unchanged binary64 master.
//!
//! Original axes remain on admitted 126-bit grids. The recovery/rate gate proves
//! r+k<=74 before evaluation: source N<2^461,D<2^454 and cycle products<2^459.
//! All scratch is fixed512. Exact dyadic normalization precedes the UNCHANGED
//! original reader splitter; this module does not enlarge its numeric theorem.
//! Intentional B(A)-M(T) debt uses two original inverses only in <=459-bit
//! comparisons. At capture symbolic cancellation restores the start denominator;
//! captured source N<2^388,D<2^380, within the original461/454 normalization gate.

use super::{
    CanonicalBeatSegment, ExplicitSeekMode, FractionalSourcePosition, MappedClockEpoch,
    MappedClockError, MappedClockMode, MappedLoopGeometry, MappedSourceSpan, SourceLoopDomain,
    SpanProgram, TransportTimeline, valid_rate,
};
use crate::audio_engine::compact_source_numerics::{
    BaseGrid, Grid, decode_f64_at_grid, decode_scaled_f64_at_grid, decode_source_pair_at_grid,
    exact_f64_parts, master_grid_exponent, required_grid_f64,
};
use crate::audio_engine::general_source_numerics::{
    Integer, Magnitude, MappedReaderPeriod, PreparedRecoverySplit, normalize_recovery_coefficient,
    split_recovery_coordinate,
};
use crate::audio_engine::loop_phase_reference::LoopPhaseReference;
use std::cmp::Ordering;

const CAPTURED: u8 = 0;
const GLOBAL: u8 = 1;
const WINDOW: u8 = 2;
const NEGATIVE: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhaseConfig {
    delta_exp: i16,
    source_exp: i8,
    beat_exp: i8,
    recovery_exp: i8,
    k: u8,
    flags: u8,
    reserved: u8,
}
impl PhaseConfig {
    fn valid(self) -> bool {
        self.reserved == 0
            && self.flags & 0xc0 == 0
            && self.kind() <= WINDOW
            && (-126..=0).contains(&self.source_exp)
            && (-126..=0).contains(&self.beat_exp)
            && (-126..=0).contains(&self.recovery_exp)
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

/// A separate64-byte payload. E/projection are immutable; M0 is reconstructed.
#[derive(Clone, Copy, Debug)]
pub(super) struct CommonPhaseAnchor {
    frame: u64,
    fraction: f64,
    inverse_segment: CanonicalBeatSegment,
    quarters: f64,
    config: PhaseConfig,
}
impl CommonPhaseAnchor {
    pub(super) fn matches_exact(self, other: Self) -> bool {
        self.frame == other.frame
            && self.fraction.to_bits() == other.fraction.to_bits()
            && self.inverse_segment.matches_exact(other.inverse_segment)
            && self.quarters.to_bits() == other.quarters.to_bits()
            && self.config == other.config
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
    odd_common: u128,
    minimum_slope_exp: i16,
    intercept_exp: Option<i16>,
    recovery_exp: i16,
    quarters: f64,
    downbeat: u8,
    bar: bool,
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
fn abs(value: Integer) -> Integer {
    Integer {
        negative: false,
        ..value
    }
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
fn residue(n: Integer, d: Integer) -> Option<Integer> {
    let rem = n.magnitude.checked_rem(d.magnitude)?;
    let rem = if n.negative && !rem.is_zero() {
        d.magnitude.checked_sub(rem)?
    } else {
        rem
    };
    Some(Integer {
        negative: false,
        magnitude: rem,
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
        if !reference.current(timeline) {
            return Err(MappedClockError::InvalidEpoch);
        }
        let start = self
            .common_phase
            .ok_or(MappedClockError::NumericalUnavailable)?
            .start;
        Ok(reference.qualify(start))
    }

    /// Optional OFFRT metadata. Failure never invalidates ordinary Ready geometry.
    pub(crate) fn prepare_common_phase(&mut self) {
        self.common_phase = prepare(self);
    }
    pub(crate) fn has_common_phase(&self) -> bool {
        self.common_phase.is_some()
    }
}

fn prepare(g: &MappedLoopGeometry) -> Option<CommonPhasePreparation> {
    let (source, beat) = if let Some(p) = g.general {
        p.original_grids()
    } else {
        g.exact_continuation?.compact?.original_grids()
    };
    let period = reader_period(g.domain)?;
    let maximum = period.max_canonical_frame(g.domain.region.start)?;
    let recovery_exp = source.recovery_exponent(maximum)?;
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
    let mut minimum: Option<(CanonicalBeatSegment, Axes, Option<Seed>)> = None;
    let mut maximum_slope: Option<(CanonicalBeatSegment, Axes, Option<Seed>)> = None;
    g.map.visit_canonical_segments_exact(
        |v| compare_f64(v, begin, divisor, bg.exponent()),
        |v| compare_f64(v, end, divisor, bg.exponent()),
        false,
        |segment| {
            let a = Axes::new(segment, g.loaded_rate_hz, sg, bg)?;
            let (odd, exponent) = a.slope(sg.exponent(), bg.exponent())?;
            common = gcd(common, odd);
            fmin = fmin.min(exponent);
            if let Some(e) = a.intercept_exp(sg.exponent())? {
                cmin = Some(cmin.map_or(e, |old: i16| old.min(e)));
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
        minimum,
        maximum,
        positive_seed,
        negative_seed,
        odd_common: common,
        minimum_slope_exp: fmin,
        intercept_exp: cmin,
        recovery_exp,
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
    pub(crate) fn is_common_phase(self) -> bool {
        matches!(self.mode, MappedClockMode::CommonPhase(_))
    }
    pub(crate) fn common_phase(
        marker: u64,
        timeline: TransportTimeline,
        geometry: &MappedLoopGeometry,
        raw: FractionalSourcePosition,
        execution: u64,
        reference: LoopPhaseReference,
    ) -> Result<Self, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        if marker == 0 || execution == u64::MAX || raw.seek_mode != ExplicitSeekMode::Normal {
            return Err(MappedClockError::InvalidEpoch);
        }
        let p = geometry.common_phase.ok_or(unavailable)?;
        let (launch_master, start_inverse) = reference.mapped_parts().ok_or(unavailable)?;
        if !reference.current(&timeline) || !start_inverse.matches_exact(p.start) {
            return Err(MappedClockError::InvalidEpoch);
        }
        let period = reader_period(geometry.domain).ok_or(unavailable)?;
        if !period.canonical_pair(raw.frame, raw.fraction, geometry.domain.region.start) {
            return Err(unavailable);
        }
        let sg = p
            .source
            .refine(required_grid_f64(raw.fraction).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        p.source.refine(p.recovery_exp).ok_or(unavailable)?;
        let source =
            decode_source_pair_at_grid(raw.frame as u64, raw.fraction, sg).ok_or(unavailable)?;
        let inverse_segment = geometry
            .map
            .canonical_segment_by_source_ordering(|v| {
                crate::audio_engine::compact_source_numerics::compare_scaled_f64_to_grid(
                    v,
                    geometry.loaded_rate_hz,
                    source,
                    sg,
                )
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
        let bg = p
            .beat
            .refine(
                gm.min(required_grid_f64(m0).ok_or(unavailable)?)
                    .min(required_grid_f64(launch_master).ok_or(unavailable)?),
            )
            .ok_or(unavailable)?;
        decode_f64_at_grid(last, bg).ok_or(unavailable)?;
        let inverse =
            Axes::new(inverse_segment, geometry.loaded_rate_hz, sg, bg).ok_or(unavailable)?;
        let i = inverse.inverse(source).ok_or(unavailable)?;
        let odd_a = (inverse.dx as u128) >> (inverse.dx as u128).trailing_zeros();
        if i.mul(uint(p.odd_common).ok_or(unavailable)?)
            .ok_or(unavailable)?
            .magnitude
            .checked_rem(Magnitude::try_from(odd_a).map_err(|_| unavailable)?)
            .ok_or(unavailable)?
            != Magnitude::MIN
        {
            return Err(unavailable);
        }
        let e0 = if i == Integer::ZERO {
            i16::MAX
        } else {
            bg.exponent() + i.magnitude.trailing_zeros() as i16
                - (inverse.dx as u128).trailing_zeros() as i16
        };
        let config = PhaseConfig {
            delta_exp: 0,
            source_exp: sg.exponent() as i8,
            beat_exp: bg.exponent() as i8,
            recovery_exp: p.recovery_exp as i8,
            k: 0,
            flags: (p.downbeat << 3) | ((p.bar as u8) << 5),
            reserved: 0,
        };
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
                config,
            }),
        };
        let core = Core::new(
            initial,
            geometry.loaded_rate_hz,
            0.0,
            reference,
            geometry.domain.region.start as u64,
        )
        .ok_or(unavailable)?;
        let positive = core.positive_debt().ok_or(unavailable)?;
        let negative = core
            .debt_period()
            .and_then(|h| h.sub(positive))
            .ok_or(unavailable)?;
        if positive == Integer::ZERO {
            if recovery_ok(
                p,
                p.minimum_slope_exp
                    .checked_add(e0.min(gm).min(0))
                    .ok_or(unavailable)?,
            ) {
                validate(geometry, initial, reference)?;
                return Ok(initial);
            }
            return Err(unavailable);
        }
        // The target-start inverse has its OWN recovery certificate. Raw inverse
        // closure alone cannot certify exact B(A)-M(T) on every forward segment.
        let c = core.start.dx as u128;
        let odd_c = c >> c.trailing_zeros();
        if core
            .target_origin
            .mul(uint(p.odd_common).ok_or(unavailable)?)
            .ok_or(unavailable)?
            .magnitude
            .checked_rem(Magnitude::try_from(odd_c).map_err(|_| unavailable)?)
            .ok_or(unavailable)?
            != Magnitude::MIN
        {
            return Err(unavailable);
        }
        let e_l = if core.target_origin == Integer::ZERO {
            i16::MAX
        } else {
            bg.exponent() + core.target_origin.magnitude.trailing_zeros() as i16
                - c.trailing_zeros() as i16
        };
        if !recovery_ok(
            p,
            p.minimum_slope_exp
                .checked_add(e_l.min(gm).min(0))
                .ok_or(unavailable)?,
        ) {
            return Err(unavailable);
        }
        let directions = if positive <= negative {
            [false, true]
        } else {
            [true, false]
        };
        for negative in directions {
            for k in 6..=64 {
                let k = k as u8;
                let global_exp = p
                    .minimum_slope_exp
                    .checked_add(e0.min(gm - k as i16))
                    .ok_or(unavailable)?;
                if recovery_ok(p, global_exp)
                    && rates(geometry, initial, k, negative, true).ok_or(unavailable)?
                {
                    let epoch = configured(initial, GLOBAL, k, negative, 0).ok_or(unavailable)?;
                    if Core::new(
                        epoch,
                        geometry.loaded_rate_hz,
                        0.0,
                        reference,
                        geometry.domain.region.start as u64,
                    )
                    .and_then(|c| c.value(u64::MAX))
                    .is_some_and(|v| v.captured)
                    {
                        validate(geometry, epoch, reference)?;
                        return Ok(epoch);
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
                let g = e0
                    .min(gm)
                    .min(required_grid_f64(seed.start).ok_or(unavailable)?)
                    .min(delta)
                    .min(0)
                    - k as i16;
                let output = p.minimum_slope_exp.checked_add(g).ok_or(unavailable)?;
                let r = (bg.exponent() - delta).max(0);
                if !recovery_ok(p, output)
                    || r + k as i16 > 74
                    || !rates(geometry, initial, k, negative, false).ok_or(unavailable)?
                {
                    continue;
                }
                let epoch = configured(initial, WINDOW, k, negative, delta).ok_or(unavailable)?;
                if Core::new(
                    epoch,
                    geometry.loaded_rate_hz,
                    seed.start,
                    reference,
                    geometry.domain.region.start as u64,
                )
                .and_then(|c| c.value(u64::MAX))
                .is_some_and(|v| v.captured)
                {
                    validate(geometry, epoch, reference)?;
                    return Ok(epoch);
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
        let c = clock(geometry, self, reference)?;
        c.value(frame)
            .map(|v| v.captured)
            .ok_or(MappedClockError::NumericalUnavailable)
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
    source_exp.min(p.intercept_exp.unwrap_or(i16::MAX)) >= p.recovery_exp
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
struct Core {
    epoch: MappedClockEpoch,
    anchor: CommonPhaseAnchor,
    inverse: Axes,
    start: Axes,
    target_origin: Integer,
    initial: Integer,
    m0: i128,
    q: Integer,
    rate: u32,
    seed: f64,
}
#[derive(Clone, Copy)]
struct Value {
    numerator: Integer,
    denominator: Integer,
    exponent: i16,
    captured: bool,
    region: u8,
    lower: Option<Integer>,
    upper: Option<Integer>,
}
impl Core {
    fn new(
        epoch: MappedClockEpoch,
        rate: u32,
        seed: f64,
        reference: LoopPhaseReference,
        source_start: u64,
    ) -> Option<Self> {
        let anchor = phase_anchor(epoch)?;
        let sg = Grid::from_admitted_exponent(anchor.config.source_exp as i16)?;
        let bg = Grid::from_admitted_exponent(anchor.config.beat_exp as i16)?;
        let raw = decode_source_pair_at_grid(anchor.frame, anchor.fraction, sg)?;
        let inverse = Axes::new(anchor.inverse_segment, rate, sg, bg)?;
        let (master, start_inverse) = reference.mapped_parts()?;
        let start = Axes::new(start_inverse, rate, sg, bg)?;
        let a = decode_source_pair_at_grid(source_start, 0.0, sg)?;
        if a < start.x0 || a > start.x0.checked_add(start.dx)? {
            return None;
        }
        let target_origin = start
            .inverse(a)?
            .sub(int(decode_f64_at_grid(master, bg)?)?.mul(int(start.dx)?)?)?;
        if raw < inverse.x0 || raw > inverse.x0.checked_add(inverse.dx)? {
            return None;
        }
        let (m0, last) = epoch
            .projection
            .finite_beats_for_horizon(epoch.first_output_frame, u64::MAX)?;
        let m0 = decode_f64_at_grid(m0, bg)?;
        decode_f64_at_grid(last, bg)?;
        let q = int(decode_f64_at_grid(anchor.quarters, bg)?)?;
        if anchor.config.kind() == WINDOW {
            decode_f64_at_grid(seed, bg)?;
        }
        Some(Self {
            epoch,
            anchor,
            inverse,
            start,
            target_origin,
            initial: inverse.inverse(raw)?,
            m0,
            q,
            rate,
            seed,
        })
    }
    fn delta(self, frame: u64) -> Option<Integer> {
        if frame < self.epoch.first_output_frame {
            return None;
        }
        let grid = Grid::from_admitted_exponent(self.anchor.config.beat_exp as i16)?;
        let d = int(decode_f64_at_grid(
            self.epoch.projection.beat_position_at_frame(frame),
            grid,
        )?
        .checked_sub(self.m0)?)?;
        (!d.negative).then_some(d)
    }
    fn debt_numerator(self) -> Option<Integer> {
        let a = int(self.inverse.dx)?;
        let c = int(self.start.dx)?;
        self.target_origin
            .mul(a)?
            .add(int(self.m0)?.mul(a)?.mul(c)?)?
            .sub(self.initial.mul(c)?)
    }
    fn debt_period(self) -> Option<Integer> {
        self.q.mul(int(self.inverse.dx)?)?.mul(int(self.start.dx)?)
    }
    fn positive_debt(self) -> Option<Integer> {
        residue(self.debt_numerator()?, self.debt_period()?)
    }
    fn debt(self) -> Option<Integer> {
        let p = self.positive_debt()?;
        if self.anchor.config.negative() {
            self.debt_period()?.sub(p)
        } else {
            Some(p)
        }
    }
    fn value(self, frame: u64) -> Option<Value> {
        let config = self.anchor.config;
        let b = config.beat_exp as i16;
        let exponent = config.fine();
        let r = (b - exponent) as u32;
        let h = power(config.k as u32)?;
        let a = int(self.inverse.dx)?;
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
        let cap = self.debt()?.shl(r)?.mul(h)?;
        let (uncapped, region, lower, upper) = if config.kind() == GLOBAL {
            (signed(delta.mul(a)?, config.negative())?, 1, None, None)
        } else {
            let bg = Grid::from_admitted_exponent(b)?;
            let mut entry = int(decode_f64_at_grid(self.seed, bg)?)?;
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
            let step = power((config.delta_exp - exponent) as u32)?;
            let period = q.sub(signed(step, config.negative())?)?;
            if period <= Integer::ZERO {
                return None;
            }
            let divisor = period.mul(a)?;
            let j = floor_ratio(v, divisor)?;
            let j128 = j.u128()?;
            let rem = residue(v, divisor)?;
            let width = h.mul(step)?.mul(a)?;
            let correcting = rem < width;
            let factor = h.add(signed(uint(1)?, config.negative())?)?;
            let x = if correcting {
                factor.mul(rem)?
            } else {
                rem.add(signed(step.mul(a)?, config.negative())?)?.mul(h)?
            };
            let beta = entry.add(q.mul(uint(j128)?)?)?.mul(denominator)?.add(x)?;
            let cycle = j.mul(divisor)?;
            let lo = wait
                .add(cycle)?
                .add(if correcting { Integer::ZERO } else { width })?;
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
        if uncapped != Integer::ZERO && uncapped.negative != config.negative() {
            return None;
        }
        let captured = abs(uncapped).mul(int(self.start.dx)?)? >= cap;
        if captured {
            // Cancel the raw inverse symbolically. Never round a new anchor or
            // multiply both inverse denominators into the source reader path.
            let winding = floor_ratio(self.debt_numerator()?, self.debt_period()?)?
                .add(uint(u128::from(config.negative()))?)?;
            let master = int(self.m0)?.add(self.delta(frame)?)?;
            let c = int(self.start.dx)?;
            return Some(Value {
                numerator: self
                    .target_origin
                    .add(master.sub(winding.mul(self.q)?)?.mul(c)?)?,
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
    fn at_least_time(self, frame: u64, target: Integer) -> Option<bool> {
        let r = (self.anchor.config.beat_exp as i16 - self.anchor.config.fine()) as u32;
        Some(self.delta(frame)?.shl(r)?.mul(int(self.inverse.dx)?)? >= target)
    }
    fn position(
        self,
        segment: CanonicalBeatSegment,
        cycle: u128,
        domain: SourceLoopDomain,
        frame: u64,
    ) -> Option<FractionalSourcePosition> {
        let (n, d) = self.source_terms(segment, cycle, frame)?;
        let coordinate = split_recovery_coordinate(
            n,
            d,
            self.anchor.config.recovery_exp as i16,
            self.anchor.fraction,
            domain,
        )?;
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
    ) -> Option<(Integer, Integer)> {
        let value = self.value(frame)?;
        let b = self.anchor.config.beat_exp as i16;
        let r = (b - value.exponent) as u32;
        let sg = Grid::from_admitted_exponent(self.anchor.config.source_exp as i16)?;
        let bg = Grid::from_admitted_exponent(b)?;
        let axes = Axes::new(segment, self.rate, sg, bg)?;
        let cycle = uint(cycle)?.shl(r)?;
        let canonical = value.numerator.sub(cycle.mul(value.denominator)?)?;
        let offset = canonical.sub(int(axes.y0)?.shl(r)?.mul(value.denominator)?)?;
        let d = value.denominator.mul(int(axes.dy)?)?.shl(r)?;
        let n = int(axes.x0)?.mul(d)?.add(offset.mul(int(axes.dx)?)?)?;
        let d = d.shl((-sg.exponent()) as u32)?;
        Some((n, d))
    }
}

fn clock(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    reference: LoopPhaseReference,
) -> Result<Core, MappedClockError> {
    let unavailable = MappedClockError::NumericalUnavailable;
    let p = g.common_phase.ok_or(unavailable)?;
    let a = phase_anchor(epoch).ok_or(unavailable)?;
    if !reference
        .mapped_parts()
        .is_some_and(|(_, start)| start.matches_exact(p.start))
    {
        return Err(unavailable);
    }
    if a.quarters.to_bits() != p.quarters.to_bits()
        || a.config.downbeat() != p.downbeat
        || a.config.bar_qualified() != p.bar
        || a.config.recovery_exp as i16 != p.recovery_exp
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
    Core::new(
        epoch,
        g.loaded_rate_hz,
        seed,
        reference,
        g.domain.region.start as u64,
    )
    .ok_or(unavailable)
}
pub(super) fn validate(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    reference: LoopPhaseReference,
) -> Result<(), MappedClockError> {
    let core = clock(g, epoch, reference)?;
    let unavailable = MappedClockError::NumericalUnavailable;
    if !rates(g, epoch, 0, false, false).ok_or(unavailable)? {
        return Err(MappedClockError::UnsupportedRate);
    }
    core.value(epoch.first_output_frame).ok_or(unavailable)?;
    core.value(u64::MAX).ok_or(unavailable)?;
    let observed = position(g, epoch, epoch.first_output_frame, reference)?;
    if observed.frame != core.anchor.frame as usize
        || observed.fraction.to_bits() != core.anchor.fraction.to_bits()
    {
        return Err(unavailable);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Target {
    n: Integer,
    d: Integer,
    e: i16,
}
fn reached(core: Core, frame: u64, target: Target) -> Option<bool> {
    let v = core.value(frame)?;
    compare_scaled(
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
fn source_phase(
    g: &MappedLoopGeometry,
    core: Core,
    frame: u64,
) -> Option<(Value, Integer, Integer, u128, CanonicalBeatSegment)> {
    let value = core.value(frame)?;
    let config = core.anchor.config;
    let sg = Grid::from_admitted_exponent(config.source_exp as i16)?;
    let bg = Grid::from_admitted_exponent(config.beat_exp as i16)?;
    let start = Axes::new(g.common_phase?.start, g.loaded_rate_hz, sg, bg)?;
    let raw = decode_source_pair_at_grid(g.domain.region.start as u64, 0.0, sg)?;
    let j = start.inverse(raw)?;
    let c = int(start.dx)?;
    let r = (bg.exponent() - value.exponent) as u32;
    let unfolded = value
        .numerator
        .mul(c)?
        .sub(j.shl(r)?.mul(value.denominator)?)?;
    let period = core.q.shl(r)?.mul(c)?.mul(value.denominator)?;
    let cycle = unfolded.quotient(period)?.mul(core.q)?.u128()?;
    let canonical = value
        .numerator
        .sub(uint(cycle)?.shl(r)?.mul(value.denominator)?)?;
    let segment = g.map.canonical_segment_by_beat_ordering(|v| {
        compare_f64(v, canonical, value.denominator, value.exponent)
    })?;
    Some((value, j, c, cycle, segment))
}
pub(super) fn position(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
    reference: LoopPhaseReference,
) -> Result<FractionalSourcePosition, MappedClockError> {
    let core = clock(g, epoch, reference)?;
    let (_, _, _, cycle, segment) =
        source_phase(g, core, frame).ok_or(MappedClockError::NumericalUnavailable)?;
    core.position(segment, cycle, g.domain, frame)
        .ok_or(MappedClockError::NumericalUnavailable)
}
pub(super) fn refresh(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
    reference: LoopPhaseReference,
) -> Result<MappedSourceSpan, MappedClockError> {
    let unavailable = MappedClockError::NumericalUnavailable;
    if frame == u64::MAX {
        return Err(unavailable);
    }
    let core = clock(g, epoch, reference)?;
    let (value, j, c, cycle, segment) = source_phase(g, core, frame).ok_or(unavailable)?;
    let bg = Grid::from_admitted_exponent(core.anchor.config.beat_exp as i16).ok_or(unavailable)?;
    let loop_left = Target {
        n: j.add(uint(cycle).and_then(|v| v.mul(c)).ok_or(unavailable)?)
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
        n: int(decode_f64_at_grid(segment.beat_start, bg).ok_or(unavailable)?)
            .and_then(|v| v.add(uint(cycle)?))
            .ok_or(unavailable)?,
        d: uint(1).ok_or(unavailable)?,
        e: bg.exponent(),
    };
    let knot_right = Target {
        n: int(decode_f64_at_grid(segment.beat_end, bg).ok_or(unavailable)?)
            .and_then(|v| v.add(uint(cycle)?))
            .ok_or(unavailable)?,
        ..knot_left
    };
    let less = |a: Target, b: Target| compare_scaled(a.n.mul(b.d)?, a.e, b.n.mul(a.d)?, b.e);
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
    let mut start =
        first(epoch.first_output_frame, frame, |n| reached(core, n, left)).ok_or(unavailable)?;
    let mut end = if reached(core, u64::MAX, right).ok_or(unavailable)? {
        first(frame + 1, u64::MAX, |n| reached(core, n, right)).ok_or(unavailable)?
    } else {
        u64::MAX
    };
    if value.captured {
        start = start.max(
            first(epoch.first_output_frame, frame, |n| {
                Some(core.value(n)?.captured)
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
        if core.value(u64::MAX).ok_or(unavailable)?.captured {
            end = end.min(
                first(frame + 1, u64::MAX, |n| Some(core.value(n)?.captured)).ok_or(unavailable)?,
            );
        }
    }
    if start > frame || end <= frame {
        return Err(unavailable);
    }
    let endpoint = position(g, epoch, end, reference)?;
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

/// A nonowning temporary local view; all grids are copied admitted originals.
pub(crate) struct PhaseLocalSpanView {
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
            .checked_sub(i16::from(config.recovery_exp))?;
        if !(0..=74).contains(&shift) {
            return None;
        }
        let product = odd.checked_mul(factor)?;
        if product == 0 || (128 - product.leading_zeros()).checked_add(shift as u32)? > 75 {
            return None;
        }
        let multiplier = product.checked_shl(shift as u32)?;
        let (n, d) = core.source_terms(span.segment, cycle, span.start_frame)?;
        let source_start = normalize_recovery_coefficient(n, d, config.recovery_exp as i16)?;
        let split =
            PreparedRecoverySplit::new(domain, config.recovery_exp as i16, core.anchor.fraction)?;
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

fn inside_held_segment(
    core: Core,
    segment: CanonicalBeatSegment,
    cycle: u128,
    value: Value,
) -> Option<bool> {
    let b = core.anchor.config.beat_exp as i16;
    let r = u32::try_from(b.checked_sub(value.exponent)?).ok()?;
    let grid = Grid::from_admitted_exponent(b)?;
    let canonical = value
        .numerator
        .sub(uint(cycle)?.shl(r)?.mul(value.denominator)?)?;
    let left = int(decode_f64_at_grid(segment.beat_start, grid)?)?
        .shl(r)?
        .mul(value.denominator)?;
    let right = int(decode_f64_at_grid(segment.beat_end, grid)?)?
        .shl(r)?
        .mul(value.denominator)?;
    Some(canonical >= left && canonical < right)
}

impl PhaseLocalSpanView {
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
        let view = PhaseLocalSpanView {
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
        Ok(view)
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
