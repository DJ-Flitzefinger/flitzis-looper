//! Explicit declared-Q ordinary recovery on original grids and a separate RN pair.
use super::anchored::AnchoredClock;
use super::*;
use crate::audio_engine::compact_source_numerics::{
    BaseGrid, Grid, GridStats, decode_f64_at_grid, decode_scaled_f64_at_grid, master_grid_exponent,
    required_grid_f64,
};
use crate::audio_engine::full_fraction_source_numerics::{
    self as numeric, Integer, RationalAffine,
};
use std::cmp::Ordering;

fn int(n: i128) -> Option<Integer> {
    Integer::from_i128(n)
}
fn uint(n: u128) -> Option<Integer> {
    Integer::from_u128(n)
}
fn compare(v: f64, n: Integer, d: Integer, b: Grid) -> Option<Ordering> {
    let (c, e) = crate::audio_engine::compact_source_numerics::exact_f64_parts(v)?;
    numeric::compare_scaled(int(c)?.mul(d)?, e, n, b.exponent())
}
#[derive(Clone, Copy)]
struct Axes {
    x0: i128,
    dx: i128,
    y0: i128,
    dy: i128,
}
impl Axes {
    fn new(s: CanonicalBeatSegment, rate: u32, source: Grid, beat: Grid) -> Option<Self> {
        let x0 = decode_scaled_f64_at_grid(s.source_start, rate, source)?;
        let y0 = decode_f64_at_grid(s.beat_start, beat)?;
        let dx = decode_scaled_f64_at_grid(s.source_end, rate, source)?.checked_sub(x0)?;
        let dy = decode_f64_at_grid(s.beat_end, beat)?.checked_sub(y0)?;
        (dx > 0 && dy > 0).then_some(Self { x0, dx, y0, dy })
    }
    fn inverse(self, frame: u64, fraction: f64, source: Grid) -> Option<(Integer, Integer)> {
        let (raw, d) = numeric::raw_pair(frame, fraction, source.exponent())?;
        numeric::inverse(raw, d, self.x0, self.dx, self.y0, self.dy)
    }
}

impl MappedLoopGeometry {
    /// Numeric capability only; independent of any phase-reference/meter decision.
    pub(super) fn prepare_full_fraction(&mut self) {
        self.full_fraction = self.full_fraction_preparation();
    }
    fn full_fraction_preparation(&self) -> Option<general::GeneralPreparation> {
        if self.domain.musical_period.physical_quarters().is_some() {
            return None;
        }
        if let Some(p) = self.general {
            return Some(p);
        }
        let (source, beat) = self.exact_continuation?.compact?.original_grids();
        let sg = source.refine(0)?;
        let bg = beat.refine(0)?;
        let (raw, d) = numeric::raw_pair(self.domain.region.start as u64, 0.0, sg.exponent())?;
        let start = self.map.canonical_segment_by_source_ordering(|v| {
            numeric::compare_source(v, self.loaded_rate_hz, raw, d, sg)
        })?;
        let a = Axes::new(start, self.loaded_rate_hz, sg, bg)?;
        let (begin, den) = a.inverse(self.domain.region.start as u64, 0.0, sg)?;
        let end = begin.add(int(decode_f64_at_grid(self.quarters, bg)?)?.mul(den)?)?;
        let mut minimum = (start, a);
        let mut maximum = (start, a);
        self.map.visit_canonical_segments_exact(
            |v| compare(v, begin, den, bg),
            |v| compare(v, end, den, bg),
            false,
            |s| {
                let ax = Axes::new(s, self.loaded_rate_hz, sg, bg)?;
                if int(ax.dx)?.mul(int(minimum.1.dy)?)? < int(minimum.1.dx)?.mul(int(ax.dy)?)? {
                    minimum = (s, ax);
                }
                if int(ax.dx)?.mul(int(maximum.1.dy)?)? > int(maximum.1.dx)?.mul(int(ax.dy)?)? {
                    maximum = (s, ax);
                }
                Some(())
            },
        )?;
        Some(general::GeneralPreparation {
            source,
            beat,
            start_segment: start,
            minimum: minimum.0,
            maximum: maximum.0,
        })
    }
}

#[derive(Clone, Copy)]
struct Core {
    epoch: MappedClockEpoch,
    source: Grid,
    beat: Grid,
    inverse_den: Integer,
    initial: Integer,
    m0: i128,
    cycle: Integer,
    domain: SourceLoopDomain,
}
impl Core {
    fn new(
        epoch: MappedClockEpoch,
        rate: u32,
        source: BaseGrid,
        beat: BaseGrid,
        domain: SourceLoopDomain,
        cycle: Integer,
        cycle_exp: i16,
    ) -> Option<Self> {
        if !epoch.is_full_fraction_general() || domain.musical_period.physical_quarters().is_some()
        {
            return None;
        }
        let anchor = epoch.anchor()?;
        let first = epoch.first_output_frame.checked_sub(epoch.address_shift)?;
        let last = u64::MAX.checked_sub(epoch.address_shift)?;
        if first < anchor.original_output_frame {
            return None;
        }
        let horizon = epoch.projection.finite_beats_for_horizon(first, last)?;
        let sg = source.refine(0)?;
        let bg = beat.refine(
            master_grid_exponent(
                epoch.projection.frames_per_beat(),
                epoch.projection.original_beat_anchor(),
            )?
            .min(cycle_exp)
            .min(required_grid_f64(anchor.original_master)?),
        )?;
        decode_f64_at_grid(horizon.0, bg)?;
        decode_f64_at_grid(horizon.1, bg)?;
        let period = numeric::reader_period(domain)?;
        if !period.canonical_pair(anchor.frame as usize, anchor.fraction, domain.region.start) {
            return None;
        }
        let axes = Axes::new(anchor.inverse_segment, rate, sg, bg)?;
        let (initial, inverse_den) = axes.inverse(anchor.frame, anchor.fraction, sg)?;
        let cycle = cycle
            .shl(u32::try_from(cycle_exp - bg.exponent()).ok()?)?
            .bounded(128)?;
        Some(Self {
            epoch,
            source: sg,
            beat: bg,
            inverse_den,
            initial,
            m0: decode_f64_at_grid(anchor.original_master, bg)?,
            cycle,
            domain,
        })
    }
    fn master(self, frame: u64) -> Option<i128> {
        decode_f64_at_grid(self.epoch.addressed_master_at(frame)?, self.beat)
    }
    fn beat(self, frame: u64) -> Option<Integer> {
        self.initial.add(
            int(self.master(frame)?.checked_sub(self.m0)?)?
                .sub(self.cycle)?
                .mul(self.inverse_den)?,
        )
    }
    fn terms(
        self,
        segment: CanonicalBeatSegment,
        rate: u32,
        frame: u64,
    ) -> Option<(Integer, Integer, Integer)> {
        let ax = Axes::new(segment, rate, self.source, self.beat)?;
        let offset = self.beat(frame)?.sub(int(ax.y0)?.mul(self.inverse_den)?)?;
        let d = self.inverse_den.mul(int(ax.dy)?)?;
        let n = int(ax.x0)?.mul(d)?.add(offset.mul(int(ax.dx)?)?)?;
        let multiplier = self.inverse_den.mul(int(ax.dx)?)?;
        Some((
            n.bounded(1460)?,
            d.shl((-self.source.exponent()) as u32)?.bounded(1454)?,
            multiplier,
        ))
    }
    fn position(
        self,
        s: CanonicalBeatSegment,
        rate: u32,
        frame: u64,
    ) -> Option<FractionalSourcePosition> {
        let (n, d, _) = self.terms(s, rate, frame)?;
        let p = numeric::split(n, d, self.epoch.anchor()?.fraction, self.domain, true)?;
        Some(FractionalSourcePosition {
            frame: usize::try_from(p.whole).ok()?,
            fraction: p.fraction,
            seek_mode: ExplicitSeekMode::Normal,
        })
    }
}
struct Clock<'a> {
    g: &'a MappedLoopGeometry,
    core: Core,
    start: Integer,
    den: Integer,
    q: Integer,
}
impl<'a> Clock<'a> {
    fn new(g: &'a MappedLoopGeometry, epoch: MappedClockEpoch) -> Option<Self> {
        let p = g.full_fraction?;
        let anchor = epoch.anchor()?;
        if !g
            .map
            .canonical_segment_at_beat(anchor.inverse_segment.beat_start)?
            .matches_exact(anchor.inverse_segment)
        {
            return None;
        }
        let core = Core::new(
            epoch,
            g.loaded_rate_hz,
            p.source,
            p.beat,
            g.domain,
            Integer::ZERO,
            0,
        )?;
        let ax = Axes::new(p.start_segment, g.loaded_rate_hz, core.source, core.beat)?;
        let (start, den) = ax.inverse(g.domain.region.start as u64, 0.0, core.source)?;
        let q = int(decode_f64_at_grid(g.quarters, core.beat)?)?;
        if q <= Integer::ZERO {
            return None;
        }
        let c = Self {
            g,
            core,
            start,
            den,
            q,
        };
        c.phase(epoch.first_output_frame)?;
        c.phase(u64::MAX)?;
        Some(c)
    }
    fn unfolded(&self, frame: u64) -> Option<Integer> {
        self.core
            .beat(frame)?
            .mul(self.den)?
            .sub(self.start.mul(self.core.inverse_den)?)
    }
    fn phase(&self, frame: u64) -> Option<(Integer, CanonicalBeatSegment)> {
        let p = self.den.mul(self.core.inverse_den)?.mul(self.q)?;
        let cycle = self
            .unfolded(frame)?
            .quotient(p)?
            .mul(self.q)?
            .bounded(128)?;
        let n = self
            .core
            .beat(frame)?
            .sub(cycle.mul(self.core.inverse_den)?)?;
        let segment = self.g.map.canonical_segment_by_beat_ordering(|v| {
            compare(v, n, self.core.inverse_den, self.core.beat)
        })?;
        Some((cycle, segment))
    }
    fn position(&self, frame: u64) -> Option<FractionalSourcePosition> {
        let (cycle, segment) = self.phase(frame)?;
        Core { cycle, ..self.core }.position(segment, self.g.loaded_rate_hz, frame)
    }
}

pub(super) fn coordinates(
    marker: u64,
    timeline: TransportTimeline,
    g: &MappedLoopGeometry,
    raw: FractionalSourcePosition,
    frame: u64,
) -> Result<MappedClockEpoch, MappedClockError> {
    let unavailable = MappedClockError::NumericalUnavailable;
    if marker == 0 || frame == u64::MAX || raw.seek_mode != ExplicitSeekMode::Normal {
        return Err(MappedClockError::InvalidEpoch);
    }
    let p = g.full_fraction.ok_or(unavailable)?;
    let period = numeric::reader_period(g.domain).ok_or(unavailable)?;
    if !period.canonical_pair(raw.frame, raw.fraction, g.domain.region.start) {
        return Err(unavailable);
    }
    let sg = p.source.refine(0).ok_or(unavailable)?;
    let (r, d) =
        numeric::raw_pair(raw.frame as u64, raw.fraction, sg.exponent()).ok_or(unavailable)?;
    let inverse_segment = g
        .map
        .canonical_segment_by_source_ordering(|v| {
            numeric::compare_source(v, g.loaded_rate_hz, r, d, sg)
        })
        .ok_or(unavailable)?;
    let projection = timeline
        .beat_projection()
        .ok_or(MappedClockError::InvalidEpoch)?;
    let (original_master, _) = projection
        .finite_beats_for_horizon(frame, u64::MAX)
        .ok_or(unavailable)?;
    let epoch = MappedClockEpoch {
        marker,
        projection,
        address_shift: 0,
        first_output_frame: frame,
        mode: MappedClockMode::FullFractionGeneral(AnchoredClock {
            frame: raw.frame as u64,
            fraction: raw.fraction,
            original_output_frame: frame,
            original_master,
            inverse_segment,
        }),
    };
    validate(g, epoch)?;
    let observed = position(g, epoch, frame)?;
    if observed.frame != raw.frame || observed.fraction.to_bits() != raw.fraction.to_bits() {
        return Err(unavailable);
    }
    Ok(epoch)
}
pub(super) fn validate(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
) -> Result<(), MappedClockError> {
    let clock = Clock::new(g, epoch).ok_or(MappedClockError::NumericalUnavailable)?;
    // The original geometry's exact rate checks apply to the unchanged projection.
    for slope in [g.min_seconds_per_quarter, g.max_seconds_per_quarter] {
        if !valid_rate(f64::from(g.loaded_rate_hz) * slope / epoch.projection.frames_per_beat()) {
            return Err(MappedClockError::UnsupportedRate);
        }
    }
    let p = g
        .full_fraction
        .ok_or(MappedClockError::NumericalUnavailable)?;
    let (fpb, fe) = crate::audio_engine::compact_source_numerics::exact_f64_parts(
        epoch.projection.frames_per_beat(),
    )
    .ok_or(MappedClockError::NumericalUnavailable)?;
    for segment in [p.minimum, p.maximum] {
        let ax = Axes::new(
            segment,
            g.loaded_rate_hz,
            clock.core.source,
            clock.core.beat,
        )
        .ok_or(MappedClockError::NumericalUnavailable)?;
        let n = int(ax.dx).ok_or(MappedClockError::NumericalUnavailable)?;
        let d = int(ax.dy)
            .and_then(|v| v.mul(int(fpb)?))
            .ok_or(MappedClockError::NumericalUnavailable)?;
        let lower = numeric::compare_scaled(
            n,
            clock.core.source.exponent() + 1,
            d,
            clock.core.beat.exponent() + fe,
        )
        .ok_or(MappedClockError::NumericalUnavailable)?;
        let upper = numeric::compare_scaled(
            n,
            clock.core.source.exponent(),
            d,
            clock.core.beat.exponent() + fe + 1,
        )
        .ok_or(MappedClockError::NumericalUnavailable)?;
        if lower == Ordering::Less || upper == Ordering::Greater {
            return Err(MappedClockError::UnsupportedRate);
        }
    }
    clock
        .position(epoch.first_output_frame)
        .ok_or(MappedClockError::NumericalUnavailable)?;
    Ok(())
}
pub(super) fn position(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
) -> Result<FractionalSourcePosition, MappedClockError> {
    Clock::new(g, epoch)
        .and_then(|c| c.position(frame))
        .ok_or(MappedClockError::NumericalUnavailable)
}
pub(super) fn refresh(
    g: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
) -> Result<MappedSourceSpan, MappedClockError> {
    let unavailable = MappedClockError::NumericalUnavailable;
    if frame == u64::MAX {
        return Err(unavailable);
    }
    let c = Clock::new(g, epoch).ok_or(unavailable)?;
    let (cycle, segment) = c.phase(frame).ok_or(unavailable)?;
    let begin = c.start;
    let end = begin
        .add(c.q.mul(c.den).ok_or(unavailable)?)
        .ok_or(unavailable)?;
    let knot = |v: f64| int(decode_f64_at_grid(v, c.core.beat)?)?.mul(c.den);
    let left = begin.max(knot(segment.beat_start).ok_or(unavailable)?);
    let right = end.min(knot(segment.beat_end).ok_or(unavailable)?);
    let target = |v: Integer| {
        v.sub(begin)?
            .add(cycle.mul(c.den)?)?
            .mul(c.core.inverse_den)
    };
    let lo = target(left).ok_or(unavailable)?;
    let hi = target(right).ok_or(unavailable)?;
    let start = super::anchored::first_true_near_high(epoch.first_output_frame, frame, |n| {
        Some(c.unfolded(n)? >= lo)
    })
    .ok_or(unavailable)?;
    let finish = if c.unfolded(u64::MAX).ok_or(unavailable)? < hi {
        u64::MAX
    } else {
        super::anchored::first_true_near_low(frame + 1, u64::MAX, |n| Some(c.unfolded(n)? >= hi))
            .ok_or(unavailable)?
    };
    let endpoint = c.position(finish).ok_or(unavailable)?;
    let cycle = cycle.u128().ok_or(unavailable)?;
    Ok(MappedSourceSpan {
        epoch,
        segment,
        program: SpanProgram::general([cycle as u64, (cycle >> 64) as u64], c.core.beat.exponent()),
        loaded_rate_hz: g.loaded_rate_hz,
        start_frame: start,
        end_frame: finish,
        endpoint_frame: endpoint.frame,
        endpoint_fraction: endpoint.fraction,
        source_start: g.domain.region.start as u64,
    })
}

pub(crate) struct FullFractionLocalView {
    span: MappedSourceSpan,
    beat: Grid,
    affine: RationalAffine,
}
impl FullFractionLocalView {
    pub(crate) fn position_at(&self, frame: u64) -> Option<FractionalSourcePosition> {
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
pub(super) fn local(
    span: MappedSourceSpan,
    domain: SourceLoopDomain,
) -> Option<FullFractionLocalView> {
    if !domain.is_valid()
        || span.source_start != domain.region.start as u64
        || span.start_frame < span.epoch.first_output_frame
        || span.start_frame >= span.end_frame
    {
        return None;
    }
    let anchor = span.epoch.anchor()?;
    let mut source = GridStats::new().include_u64(span.source_start)?;
    let mut beat = GridStats::new();
    for s in [anchor.inverse_segment, span.segment] {
        source = source
            .include_scaled_f64(s.source_start, span.loaded_rate_hz)?
            .include_scaled_f64(s.source_end, span.loaded_rate_hz)?;
        beat = beat.include_f64(s.beat_start)?.include_f64(s.beat_end)?;
    }
    let (coefficient, e) = span.program.general_value()?;
    let cycle = uint(u128::from(coefficient[0]) | (u128::from(coefficient[1]) << 64))?;
    let core = Core::new(
        span.epoch,
        span.loaded_rate_hz,
        source.finish()?,
        beat.finish()?,
        domain,
        cycle,
        e,
    )?;
    let axes = Axes::new(span.segment, span.loaded_rate_hz, core.source, core.beat)?;
    let lower = int(axes.y0)?.mul(core.inverse_den)?;
    let upper = int(axes.y0.checked_add(axes.dy)?)?.mul(core.inverse_den)?;
    for frame in [span.start_frame, span.end_frame - 1] {
        let beat = core.beat(frame)?;
        if beat < lower || beat >= upper {
            return None;
        }
    }
    let (n, d, b) = core.terms(span.segment, span.loaded_rate_hz, span.start_frame)?;
    let affine = RationalAffine::new(
        n,
        b,
        d,
        core.master(span.start_frame)?,
        core.master(span.end_frame - 1)?,
        anchor.fraction,
        domain,
        true,
    )?;
    let period = numeric::reader_period(domain)?;
    if !period.canonical_pair(
        span.endpoint_frame,
        span.endpoint_fraction,
        domain.region.start,
    ) {
        return None;
    }
    Some(FullFractionLocalView {
        span,
        beat: core.beat,
        affine,
    })
}
