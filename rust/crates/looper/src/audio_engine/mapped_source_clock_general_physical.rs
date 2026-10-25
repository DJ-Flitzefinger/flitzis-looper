//! Physical-marker Q=B(Z)-B(A), with independent bounded1024-bit arithmetic.
//! The copied domain retains exact Q; spans retain only k. No owner or pointer is
//! added, and the declared-Q512-bit theorem remains unchanged.
use super::*;
use crate::audio_engine::general_source_numerics::{
    PhysicalInteger as WideInteger, PhysicalMapQuarters, split_physical_coordinate,
};

fn bounded(value: WideInteger, bits: u32) -> Option<WideInteger> {
    (value.magnitude.bit_width() <= bits).then_some(value)
}
fn integer(value: i128) -> Option<WideInteger> {
    WideInteger::from_i128(value)
}
fn compare_wide_dyadics(a: WideInteger, ae: i16, b: WideInteger, be: i16) -> Option<Ordering> {
    if a.negative != b.negative || a == WideInteger::ZERO || b == WideInteger::ZERO {
        return Some(a.cmp(&b));
    }
    let at = a.magnitude.bit_width() as i32 + i32::from(ae);
    let bt = b.magnitude.bit_width() as i32 + i32::from(be);
    if at != bt {
        return Some(if a.negative { bt.cmp(&at) } else { at.cmp(&bt) });
    }
    let common = ae.min(be);
    Some(
        a.shl(u32::try_from(ae - common).ok()?)?
            .cmp(&b.shl(u32::try_from(be - common).ok()?)?),
    )
}
fn compare_wide_beat(value: f64, n: WideInteger, d: WideInteger, grid: Grid) -> Option<Ordering> {
    let (coefficient, exponent) = exact_f64_parts(value)?;
    compare_wide_dyadics(integer(coefficient)?.mul(d)?, exponent, n, grid.exponent())
}

impl MappedLoopGeometry {
    /// Separate OFFRT physical-marker qualification on original loaded knots.
    pub(crate) fn new_general_physical(
        map: SourceBeatmap,
        sample_frames: usize,
        rate: u32,
        region: FrameRange,
        require_four_four: bool,
    ) -> Result<Self, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        if rate == 0 || region.start >= region.end || region.end > sample_frames {
            return Err(MappedClockError::InvalidGeometry);
        }
        let initial = GridStats::new()
            .include_u64(region.start as u64)
            .and_then(|s| s.include_u64(region.end as u64))
            .ok_or(unavailable)?;
        let grid = initial
            .finish()
            .and_then(|s| s.refine(0))
            .ok_or(unavailable)?;
        let a = decode_source_pair_at_grid(region.start as u64, 0.0, grid).ok_or(unavailable)?;
        let z = decode_source_pair_at_grid(region.end as u64, 0.0, grid).ok_or(unavailable)?;
        let first = map
            .canonical_segment_by_source_ordering(|s| compare_scaled_f64_to_grid(s, rate, a, grid))
            .ok_or(MappedClockError::UnsupportedCoverage)?;
        // Z is exclusive. An unused exterior interval must not veto its left limit.
        let last = map
            .canonical_segment_by_source_ordering(|s| {
                let order = compare_scaled_f64_to_grid(s, rate, z, grid)?;
                Some(if order == Ordering::Equal {
                    Ordering::Greater
                } else {
                    order
                })
            })
            .ok_or(MappedClockError::UnsupportedCoverage)?;
        let (source, beat) = include_segment(initial, GridStats::new(), first, rate)
            .and_then(|(s, b)| include_segment(s, b, last, rate))
            .ok_or(unavailable)?;
        let sg = source
            .finish()
            .and_then(|s| s.refine(0))
            .ok_or(unavailable)?;
        let bg = beat.finish().and_then(|s| s.refine(0)).ok_or(unavailable)?;
        let ax = Axes::new(first, rate, sg, bg).ok_or(unavailable)?;
        let zx = Axes::new(last, rate, sg, bg).ok_or(unavailable)?;
        let a0 = ax
            .inverse(decode_source_pair_at_grid(region.start as u64, 0.0, sg).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        let z0 = zx
            .inverse(decode_source_pair_at_grid(region.end as u64, 0.0, sg).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        let ad = Integer::from_i128(ax.dx).ok_or(unavailable)?;
        let zd = Integer::from_i128(zx.dx).ok_or(unavailable)?;
        if z0
            .mul(ad)
            .and_then(|v| v.sub(a0.mul(zd)?))
            .ok_or(unavailable)?
            <= Integer::ZERO
        {
            return Err(MappedClockError::InvalidGeometry);
        }
        let mut source_stats = Some(source);
        let mut beat_stats = Some(beat);
        let mut low = f64::INFINITY;
        let mut high = 0.0_f64;
        map.visit_canonical_segments_exact(
            |v| compare_beat(v, a0, ad, bg),
            |v| compare_beat(v, z0, zd, bg),
            require_four_four,
            |s| {
                let (a, b) = include_segment(source_stats?, beat_stats?, s, rate)?;
                source_stats = Some(a);
                beat_stats = Some(b);
                let slope = s.seconds_per_quarter();
                if !slope.is_finite() || slope <= 0.0 {
                    return None;
                }
                low = low.min(slope);
                high = high.max(slope);
                Some(())
            },
        )
        .ok_or(MappedClockError::UnsupportedCoverage)?;
        let source = source_stats.and_then(|s| s.finish()).ok_or(unavailable)?;
        let beat = beat_stats.and_then(|s| s.finish()).ok_or(unavailable)?;
        let sg = source.refine(0).ok_or(unavailable)?;
        let bg = beat.refine(0).ok_or(unavailable)?;
        let ax = Axes::new(first, rate, sg, bg).ok_or(unavailable)?;
        let zx = Axes::new(last, rate, sg, bg).ok_or(unavailable)?;
        let a0 = ax
            .inverse(decode_source_pair_at_grid(region.start as u64, 0.0, sg).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        let z0 = zx
            .inverse(decode_source_pair_at_grid(region.end as u64, 0.0, sg).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        let ad = Integer::from_i128(ax.dx).ok_or(unavailable)?;
        let zd = Integer::from_i128(zx.dx).ok_or(unavailable)?;
        let n = z0
            .mul(ad)
            .and_then(|v| v.sub(a0.mul(zd)?))
            .ok_or(unavailable)?;
        let d = ad
            .mul(zd)
            .and_then(|v| v.shl(u32::try_from(-bg.exponent()).ok()?))
            .ok_or(unavailable)?;
        let quarters = PhysicalMapQuarters::new(n, d).ok_or(unavailable)?;
        let mut minimum = first;
        let mut maximum = first;
        map.visit_canonical_segments_exact(
            |v| compare_beat(v, a0, ad, bg),
            |v| compare_beat(v, z0, zd, bg),
            require_four_four,
            |s| {
                let axes = Axes::new(s, rate, sg, bg)?;
                if slope_less(axes, Axes::new(minimum, rate, sg, bg)?)? {
                    minimum = s;
                }
                if slope_less(Axes::new(maximum, rate, sg, bg)?, axes)? {
                    maximum = s;
                }
                Some(())
            },
        )
        .ok_or(unavailable)?;
        Ok(Self {
            map,
            domain: SourceLoopDomain::mapped_physical(sample_frames, region, quarters)
                .ok_or(unavailable)?,
            // These legacy observers are diagnostics, never physical-Q authority.
            beat_start: first.beat_start
                + (region.start as f64 / f64::from(rate) - first.source_start)
                    / first.seconds_per_quarter(),
            quarters: quarters.approximate(),
            source_end: region.end as f64 / f64::from(rate),
            loaded_rate_hz: rate,
            min_seconds_per_quarter: low,
            max_seconds_per_quarter: high,
            exact_continuation: None,
            general: Some(GeneralPreparation {
                source,
                beat,
                start_segment: first,
                minimum,
                maximum,
            }),
            full_fraction: None,
            common_phase: None,
        })
    }
}

#[derive(Clone, Copy)]
struct PhysicalCore {
    epoch: MappedClockEpoch,
    source: Grid,
    beat: Grid,
    inverse: Axes,
    raw_num: WideInteger,
    m0: i128,
    q_numerator: WideInteger,
    q_denominator: WideInteger,
    cycle_numerator: WideInteger,
    region: FrameRange,
}
impl PhysicalCore {
    fn new(
        epoch: MappedClockEpoch,
        rate: u32,
        source: BaseGrid,
        beat: BaseGrid,
        quarters: PhysicalMapQuarters,
        region: FrameRange,
        cycles: u128,
    ) -> Option<Self> {
        let anchor = epoch.anchor()?;
        if region.start >= region.end
            || !quarters.is_valid()
            || anchor.frame < region.start as u64
            || anchor.frame >= region.end as u64
            || !anchor.fraction.is_finite()
            || !(0.0..1.0).contains(&anchor.fraction)
        {
            return None;
        }
        let first = epoch.first_output_frame.checked_sub(epoch.address_shift)?;
        let last = u64::MAX.checked_sub(epoch.address_shift)?;
        if first < anchor.original_output_frame {
            return None;
        }
        let horizon = epoch.projection.finite_beats_for_horizon(first, last)?;
        let source = source.refine(required_grid_f64(anchor.fraction)?)?;
        let required = master_grid_exponent(
            epoch.projection.frames_per_beat(),
            epoch.projection.original_beat_anchor(),
        )?
        .min(required_grid_f64(anchor.original_master)?);
        let beat = beat.refine(required)?;
        decode_f64_at_grid(horizon.0, beat)?;
        decode_f64_at_grid(horizon.1, beat)?;
        let raw = decode_source_pair_at_grid(anchor.frame, anchor.fraction, source)?;
        let inverse = Axes::new(anchor.inverse_segment, rate, source, beat)?;
        if raw < inverse.x0 || raw > inverse.x0.checked_add(inverse.dx)? {
            return None;
        }
        let (qn, qd) = quarters.parts();
        let q_numerator = bounded(
            WideInteger::from_integer(qn).shl(u32::try_from(-beat.exponent()).ok()?)?,
            382,
        )?;
        let q_denominator = bounded(WideInteger::from_integer(qd), 256)?;
        let cycle_numerator = bounded(q_numerator.mul(WideInteger::from_u128(cycles)?)?, 510)?;
        if cycle_numerator >= q_denominator.shl(128)? {
            return None;
        }
        Some(Self {
            epoch,
            source,
            beat,
            inverse,
            raw_num: bounded(WideInteger::from_integer(inverse.inverse(raw)?), 255)?,
            m0: decode_f64_at_grid(anchor.original_master, beat)?,
            q_numerator,
            q_denominator,
            cycle_numerator,
            region,
        })
    }
    fn with_cycles(self, cycles: u128) -> Option<Self> {
        let cycle_numerator = bounded(self.q_numerator.mul(WideInteger::from_u128(cycles)?)?, 510)?;
        if cycle_numerator >= self.q_denominator.shl(128)? {
            return None;
        }
        Some(Self {
            cycle_numerator,
            ..self
        })
    }
    fn uncycled_num(&self, frame: u64) -> Option<WideInteger> {
        let delta = decode_f64_at_grid(self.epoch.addressed_master_at(frame)?, self.beat)?
            .checked_sub(self.m0)?;
        bounded(
            self.raw_num
                .add(integer(delta)?.mul(integer(self.inverse.dx)?)?)?,
            256,
        )
    }
    fn beat_num(&self, frame: u64) -> Option<WideInteger> {
        let base = bounded(self.uncycled_num(frame)?.mul(self.q_denominator)?, 512)?;
        let cycle = bounded(self.cycle_numerator.mul(integer(self.inverse.dx)?)?, 511)?;
        bounded(base.sub(cycle)?, 513)
    }
    fn beat_den(&self) -> Option<WideInteger> {
        bounded(integer(self.inverse.dx)?.mul(self.q_denominator)?, 383)
    }
    fn prepare_affine(&self, segment: CanonicalBeatSegment, rate: u32) -> Option<PhysicalAffine> {
        let axes = Axes::new(segment, rate, self.source, self.beat)?;
        let beat_den = self.beat_den()?;
        let raw = bounded(self.raw_num.mul(self.q_denominator)?, 511)?;
        let cycle = bounded(self.cycle_numerator.mul(integer(self.inverse.dx)?)?, 511)?;
        let beat_origin = bounded(raw.sub(cycle)?, 512)?;
        let segment_origin = bounded(integer(axes.y0)?.mul(beat_den)?, 509)?;
        let unshifted_den = bounded(beat_den.mul(integer(axes.dy)?)?, 510)?;
        let source_origin = bounded(integer(axes.x0)?.mul(unshifted_den)?, 636)?;
        let denominator = bounded(
            unshifted_den.shl(u32::try_from(-self.source.exponent()).ok()?)?,
            636,
        )?;
        // Factoring is exact: raw255, r127, D256, Cnum<D<<128 and the complete
        // actual-master coefficient126 proof make the old uncycled256/base512/
        // cycle511 intermediates total. This cannot rescue an excluded overflow.
        // Origins remain SIGNED: original M0 can precede this segment by many
        // wraps. Delta zero need not describe an in-domain source position.
        Some(PhysicalAffine {
            epoch: self.epoch,
            beat: self.beat,
            m0: self.m0,
            raw_fraction: self.epoch.anchor()?.fraction,
            region: self.region,
            beat_den,
            beat_origin,
            segment_origin,
            source_origin,
            denominator,
            source_delta: integer(axes.dx)?,
        })
    }
    fn position(
        &self,
        segment: CanonicalBeatSegment,
        rate: u32,
        frame: u64,
    ) -> Option<FractionalSourcePosition> {
        self.prepare_affine(segment, rate)?.position(frame)
    }
}

/// Once-per-span exact affine constants. This temporary value retains the
/// original epoch/M0 and is rebuilt with every segment/cycle/domain/epoch tuple.
/// It enters neither copied playback nor persistent voice state.
struct PhysicalAffine {
    epoch: MappedClockEpoch,
    beat: Grid,
    m0: i128,
    raw_fraction: f64,
    region: FrameRange,
    beat_den: WideInteger,
    beat_origin: WideInteger,
    segment_origin: WideInteger,
    source_origin: WideInteger,
    denominator: WideInteger,
    source_delta: WideInteger,
}
impl PhysicalAffine {
    fn position(&self, frame: u64) -> Option<FractionalSourcePosition> {
        let delta = decode_f64_at_grid(self.epoch.addressed_master_at(frame)?, self.beat)?
            .checked_sub(self.m0)?;
        let beat_num = bounded(
            self.beat_origin.add(integer(delta)?.mul(self.beat_den)?)?,
            513,
        )?;
        let offset = bounded(beat_num.sub(self.segment_origin)?, 514)?;
        let numerator = bounded(self.source_origin.add(offset.mul(self.source_delta)?)?, 642)?;
        let coordinate =
            split_physical_coordinate(numerator, self.denominator, self.raw_fraction, self.region)?;
        Some(FractionalSourcePosition {
            frame: usize::try_from(coordinate.whole).ok()?,
            fraction: coordinate.fraction,
            seek_mode: ExplicitSeekMode::Normal,
        })
    }
}

struct PhysicalClock<'a> {
    geometry: &'a MappedLoopGeometry,
    core: PhysicalCore,
    start_num: WideInteger,
    start_den: WideInteger,
    unfolded_den: WideInteger,
}
impl<'a> PhysicalClock<'a> {
    fn new(
        geometry: &'a MappedLoopGeometry,
        epoch: MappedClockEpoch,
    ) -> Result<Self, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        if !epoch.is_general_compact() || epoch.marker == 0 {
            return Err(unavailable);
        }
        let prep = geometry.general.ok_or(unavailable)?;
        let quarters = geometry
            .domain
            .musical_period
            .physical_quarters()
            .ok_or(unavailable)?;
        let anchor = epoch.anchor().ok_or(unavailable)?;
        if !geometry
            .map
            .canonical_segment_at_beat(anchor.inverse_segment.beat_start)
            .is_some_and(|s| s.matches_exact(anchor.inverse_segment))
        {
            return Err(unavailable);
        }
        let core = PhysicalCore::new(
            epoch,
            geometry.loaded_rate_hz,
            prep.source,
            prep.beat,
            quarters,
            geometry.domain.region,
            0,
        )
        .ok_or(unavailable)?;
        // Both endpoint inverses lie between the original admitted beat knots.
        if core.q_numerator >= core.q_denominator.shl(127).ok_or(unavailable)? {
            return Err(unavailable);
        }
        let first = Axes::new(
            prep.start_segment,
            geometry.loaded_rate_hz,
            core.source,
            core.beat,
        )
        .ok_or(unavailable)?;
        let start_num = WideInteger::from_integer(
            first
                .inverse(
                    decode_source_pair_at_grid(
                        geometry.domain.region.start as u64,
                        0.0,
                        core.source,
                    )
                    .ok_or(unavailable)?,
                )
                .ok_or(unavailable)?,
        );
        let start_den = integer(first.dx).ok_or(unavailable)?;
        let unfolded_den = bounded(
            start_den
                .mul(integer(core.inverse.dx).ok_or(unavailable)?)
                .ok_or(unavailable)?,
            254,
        )
        .ok_or(unavailable)?;
        geometry.validate_epoch(epoch)?;
        for segment in [prep.minimum, prep.maximum] {
            let a = Axes::new(segment, geometry.loaded_rate_hz, core.source, core.beat)
                .ok_or(unavailable)?;
            let (fpb, fe) =
                exact_f64_parts(epoch.projection.frames_per_beat()).ok_or(unavailable)?;
            let numerator = Integer::from_i128(a.dx).ok_or(unavailable)?;
            let denominator = Integer::from_i128(a.dy)
                .and_then(|v| v.mul(Integer::from_i128(fpb)?))
                .ok_or(unavailable)?;
            if compare_dyadics(
                numerator,
                core.source.exponent() + 1,
                denominator,
                core.beat.exponent().checked_add(fe).ok_or(unavailable)?,
            )
            .ok_or(unavailable)?
                == Ordering::Less
                || compare_dyadics(
                    numerator,
                    core.source.exponent(),
                    denominator,
                    core.beat
                        .exponent()
                        .checked_add(fe)
                        .and_then(|e| e.checked_add(1))
                        .ok_or(unavailable)?,
                )
                .ok_or(unavailable)?
                    == Ordering::Greater
            {
                return Err(MappedClockError::UnsupportedRate);
            }
        }
        let clock = Self {
            geometry,
            core,
            start_num,
            start_den,
            unfolded_den,
        };
        // U is nonnegative and monotone on the entire shifted finite horizon.
        // These endpoints additionally prove k<2^128 for every future refresh.
        clock.phase(epoch.first_output_frame).ok_or(unavailable)?;
        clock.phase(u64::MAX).ok_or(unavailable)?;
        Ok(clock)
    }
    fn unfolded(&self, frame: u64) -> Option<WideInteger> {
        let value = bounded(
            self.core
                .uncycled_num(frame)?
                .mul(self.start_den)?
                .sub(self.start_num.mul(integer(self.core.inverse.dx)?)?)?,
            384,
        )?;
        if value < WideInteger::ZERO || value >= self.unfolded_den.shl(128)? {
            return None;
        }
        Some(value)
    }
    fn scaled_unfolded(&self, frame: u64) -> Option<WideInteger> {
        bounded(self.unfolded(frame)?.mul(self.core.q_denominator)?, 640)
    }
    fn phase(&self, frame: u64) -> Option<(u128, PhysicalCore)> {
        let denominator = bounded(self.unfolded_den.mul(self.core.q_numerator)?, 636)?;
        let k = self.scaled_unfolded(frame)?.quotient(denominator)?.u128()?;
        Some((k, self.core.with_cycles(k)?))
    }
    fn segment(&self, core: &PhysicalCore, frame: u64) -> Option<CanonicalBeatSegment> {
        let n = core.beat_num(frame)?;
        let d = core.beat_den()?;
        self.geometry
            .map
            .canonical_segment_by_beat_ordering(|v| compare_wide_beat(v, n, d, self.core.beat))
    }
    fn position(&self, frame: u64) -> Option<FractionalSourcePosition> {
        let (_, core) = self.phase(frame)?;
        core.position(
            self.segment(&core, frame)?,
            self.geometry.loaded_rate_hz,
            frame,
        )
    }
    fn start_order(&self, beat: f64) -> Option<Ordering> {
        compare_wide_beat(beat, self.start_num, self.start_den, self.core.beat)
    }
    fn end_order(&self, beat: f64) -> Option<Ordering> {
        let n = self
            .start_num
            .mul(self.core.q_denominator)?
            .add(self.core.q_numerator.mul(self.start_den)?)?;
        let d = self.start_den.mul(self.core.q_denominator)?;
        compare_wide_beat(beat, n, d, self.core.beat)
    }
    fn threshold(&self, core: &PhysicalCore, boundary: Boundary) -> Option<WideInteger> {
        let cycle = core.cycle_numerator.mul(self.unfolded_den)?;
        let result = match boundary {
            Boundary::Start => cycle,
            Boundary::End => core
                .cycle_numerator
                .add(self.core.q_numerator)?
                .mul(self.unfolded_den)?,
            Boundary::Knot(value) => {
                let y = integer(decode_f64_at_grid(value, self.core.beat)?)?;
                y.mul(self.start_den)?
                    .sub(self.start_num)?
                    .mul(integer(self.core.inverse.dx)?)?
                    .mul(self.core.q_denominator)?
                    .add(cycle)?
            }
        };
        bounded(result, 640)
    }
}

enum Boundary {
    Start,
    End,
    Knot(f64),
}

pub(super) fn validate_epoch(
    geometry: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
) -> Result<(), MappedClockError> {
    PhysicalClock::new(geometry, epoch).map(|_| ())
}
pub(super) fn position(
    geometry: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
) -> Result<FractionalSourcePosition, MappedClockError> {
    PhysicalClock::new(geometry, epoch)?
        .position(frame)
        .ok_or(MappedClockError::NumericalUnavailable)
}
pub(super) fn refresh(
    geometry: &MappedLoopGeometry,
    epoch: MappedClockEpoch,
    frame: u64,
) -> Result<MappedSourceSpan, MappedClockError> {
    let unavailable = MappedClockError::NumericalUnavailable;
    if frame == u64::MAX {
        return Err(unavailable);
    }
    let clock = PhysicalClock::new(geometry, epoch)?;
    let (cycles, core) = clock.phase(frame).ok_or(unavailable)?;
    let segment = clock.segment(&core, frame).ok_or(unavailable)?;
    let left = if clock.start_order(segment.beat_start).ok_or(unavailable)? != Ordering::Greater {
        Boundary::Start
    } else {
        Boundary::Knot(segment.beat_start)
    };
    let right = if clock.end_order(segment.beat_end).ok_or(unavailable)? != Ordering::Less {
        Boundary::End
    } else {
        Boundary::Knot(segment.beat_end)
    };
    let lower = clock.threshold(&core, left).ok_or(unavailable)?;
    let upper = clock.threshold(&core, right).ok_or(unavailable)?;
    let start_frame = first_true_near_high(epoch.first_output_frame, frame, |n| {
        Some(clock.scaled_unfolded(n)? >= lower)
    })
    .ok_or(unavailable)?;
    let next = frame.checked_add(1).ok_or(unavailable)?;
    let end_frame = if clock.scaled_unfolded(u64::MAX).ok_or(unavailable)? < upper {
        u64::MAX
    } else {
        first_true_near_low(next, u64::MAX, |n| Some(clock.scaled_unfolded(n)? >= upper))
            .ok_or(unavailable)?
    };
    let endpoint = clock.position(end_frame).ok_or(unavailable)?;
    let span = MappedSourceSpan {
        epoch,
        segment,
        program: SpanProgram::physical([cycles as u64, (cycles >> 64) as u64]),
        loaded_rate_hz: geometry.loaded_rate_hz,
        start_frame,
        end_frame,
        endpoint_frame: endpoint.frame,
        endpoint_fraction: endpoint.fraction,
        source_start: geometry.domain.region.start as u64,
    };
    span.position_in_domain(geometry.domain, frame)
        .ok_or(unavailable)?;
    span.local_rate().ok_or(MappedClockError::UnsupportedRate)?;
    Ok(span)
}

pub(super) struct PhysicalLocalSpanView {
    span: MappedSourceSpan,
    affine: PhysicalAffine,
}
impl PhysicalLocalSpanView {
    pub(super) fn position_at(&self, frame: u64) -> Option<FractionalSourcePosition> {
        if frame == self.span.end_frame {
            return Some(FractionalSourcePosition {
                frame: self.span.endpoint_frame,
                fraction: self.span.endpoint_fraction,
                seek_mode: ExplicitSeekMode::Normal,
            });
        }
        self.span.contains(frame).then_some(())?;
        self.affine.position(frame)
    }
}

pub(super) fn local_view(
    span: MappedSourceSpan,
    domain: SourceLoopDomain,
) -> Result<PhysicalLocalSpanView, MappedClockError> {
    let unavailable = MappedClockError::NumericalUnavailable;
    let quarters = domain
        .musical_period
        .physical_quarters()
        .ok_or(unavailable)?;
    if !span.epoch.is_general_compact()
        || !domain.is_valid()
        || span.source_start != domain.region.start as u64
        || span.start_frame < span.epoch.first_output_frame
        || span.start_frame >= span.end_frame
        || span.endpoint_frame < domain.region.start
        || span.endpoint_frame >= domain.region.end
        || !span.endpoint_fraction.is_finite()
        || !(0.0..1.0).contains(&span.endpoint_fraction)
    {
        return Err(unavailable);
    }
    let cycles = span.program.physical_value().ok_or(unavailable)?;
    let cycles = u128::from(cycles[0]) | u128::from(cycles[1]) << 64;
    let anchor = span.epoch.anchor().ok_or(unavailable)?;
    let mut source = GridStats::new()
        .include_u64(span.source_start)
        .and_then(|s| s.include_u64(domain.region.end as u64))
        .and_then(|s| s.include_u64(anchor.frame))
        .and_then(|s| s.include_f64(anchor.fraction))
        .ok_or(unavailable)?;
    let mut beat = GridStats::new();
    for segment in [anchor.inverse_segment, span.segment] {
        let pair =
            include_segment(source, beat, segment, span.loaded_rate_hz).ok_or(unavailable)?;
        source = pair.0;
        beat = pair.1;
    }
    let first = span
        .epoch
        .first_output_frame
        .checked_sub(span.epoch.address_shift)
        .ok_or(unavailable)?;
    let last = u64::MAX
        .checked_sub(span.epoch.address_shift)
        .ok_or(unavailable)?;
    let horizon = span
        .epoch
        .projection
        .finite_beats_for_horizon(first, last)
        .ok_or(unavailable)?;
    beat = beat
        .include_f64(anchor.original_master)
        .and_then(|s| s.include_f64(horizon.0))
        .and_then(|s| s.include_f64(horizon.1))
        .ok_or(unavailable)?;
    let core = PhysicalCore::new(
        span.epoch,
        span.loaded_rate_hz,
        source.finish().ok_or(unavailable)?,
        beat.finish().ok_or(unavailable)?,
        quarters,
        domain.region,
        cycles,
    )
    .ok_or(unavailable)?;
    let affine = core
        .prepare_affine(span.segment, span.loaded_rate_hz)
        .ok_or(unavailable)?;
    Ok(PhysicalLocalSpanView { span, affine })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::mapped_source_clock::tests::map_with_source;

    fn geometry() -> MappedLoopGeometry {
        let map = map_with_source(
            &[(0.0, 0.0), (3.0, 2.0), (8.0, 5.0)],
            &["trusted", "trusted"],
            false,
            1,
            10,
        );
        MappedLoopGeometry::new_general_physical(map, 10, 1, FrameRange { start: 1, end: 4 }, false)
            .unwrap()
    }
    fn timeline() -> TransportTimeline {
        let mut timeline = TransportTimeline::new(1);
        assert!(timeline.set_master_period(2.0));
        timeline
    }
    fn epoch(g: &MappedLoopGeometry, fraction: f64) -> MappedClockEpoch {
        MappedClockEpoch::general_compact(
            9,
            timeline(),
            g,
            SourceLoopDomain::physical(10, g.domain().region),
            FractionalSourcePosition {
                frame: 1,
                fraction,
                seek_mode: ExplicitSeekMode::Normal,
            },
            0,
        )
        .unwrap()
    }

    #[test]
    fn rational_physical_extent_matches_independent_integer_wrap_oracle() {
        let g = geometry();
        let q = g.domain().musical_period.physical_quarters().unwrap();
        let (n, d) = q.parts();
        assert_eq!((n.u128(), d.u128()), (Some(29), Some(15)));
        let e = epoch(&g, -0.0);
        // Independently B(A)=2/3, B(Z)=13/5, Q=29/15. At actual M(n)=n/2,
        // b=(20+(15*n mod58))/30. The two original forward segments follow.
        for frame in 0..117_u64 {
            let beat = 20 + 15 * frame % 58;
            let (numerator, denominator) = if beat <= 60 {
                (beat, 20)
            } else {
                (beat - 6, 18)
            };
            let whole = numerator / denominator;
            let rem = numerator % denominator;
            let fraction = if rem == 0 {
                -0.0
            } else {
                rem as f64 / denominator as f64
            };
            let expected = FractionalSourcePosition {
                frame: whole as usize,
                fraction,
                seek_mode: ExplicitSeekMode::Normal,
            };
            let actual = g.position_at(e, frame).unwrap();
            assert_eq!(actual.frame, expected.frame, "frame={frame}");
            assert_eq!(
                actual.fraction.to_bits(),
                expected.fraction.to_bits(),
                "frame={frame}"
            );
            let span = g.refresh_span(e, frame).unwrap();
            assert_eq!(span.position_at(frame), None);
            let local = span
                .local_general_view(g.domain())
                .unwrap()
                .position_at(frame)
                .unwrap();
            assert_eq!(
                (local.frame, local.fraction.to_bits()),
                (actual.frame, actual.fraction.to_bits())
            );
        }
        assert_eq!(std::mem::size_of::<MappedClockEpoch>(), 128);
        assert!(std::mem::size_of::<MappedSourceSpan>() <= 240);
        assert!(!std::mem::needs_drop::<MappedSourceSpan>());
    }

    #[test]
    fn prepared_physical_affine_reuses_initial_and_late_wrapped_views_with_global_endpoints() {
        let g = geometry();
        let e = epoch(&g, -0.0);
        for requested in [0, 58] {
            let span = g.refresh_span(e, requested).unwrap();
            let view = local_view(span, g.domain()).unwrap();
            assert!(span.end_frame() - span.start_frame() >= 2);
            // At frame58 the same original raw/M0 anchor is fifteen cycles old.
            // Its signed delta-zero base is outside the current physical circle.
            assert_eq!(view.affine.beat_origin.negative, requested == 58);
            for frame in span.start_frame()..=span.end_frame() {
                // Independent integer oracle: Q=29/15 and actual M(n)=n/2.
                let beat = 20 + 15 * frame % 58;
                let (n, d) = if beat <= 60 {
                    (beat, 20)
                } else {
                    (beat - 6, 18)
                };
                let remainder = n % d;
                let expected_fraction = if remainder == 0 {
                    -0.0
                } else {
                    remainder as f64 / d as f64
                };
                let actual = view.position_at(frame).unwrap();
                assert_eq!(actual.frame, (n / d) as usize, "frame={frame}");
                assert_eq!(
                    actual.fraction.to_bits(),
                    expected_fraction.to_bits(),
                    "frame={frame}"
                );
            }
            assert!(view.position_at(span.end_frame() + 1).is_none());
        }
    }

    #[test]
    fn both_decimal_endpoint_denominators_are_retained_in_the_copied_domain() {
        let map = map_with_source(
            &[(0.0, 0.0), (0.001, 0.125), (0.031, 2.0), (0.062, 4.0)],
            &["trusted", "trusted", "trusted"],
            false,
            48_000,
            4_000,
        );
        let region = FrameRange {
            start: 1,
            end: 1502,
        };
        let g =
            MappedLoopGeometry::new_general_physical(map, 4_000, 48_000, region, false).unwrap();
        let (n, d) = g
            .domain()
            .musical_period
            .physical_quarters()
            .unwrap()
            .parts();
        assert_eq!(n.u128(), Some(973596225794487503992543076417536));
        assert_eq!(d.u128(), Some(482883607843738976046606013087875));
        assert!(d.magnitude.bit_width() > 64);
        assert_eq!(
            g.domain().musical_period.mapped_period(region),
            MappedReaderPeriod::physical(1501)
        );
        let mut timeline = TransportTimeline::new(48_000);
        assert!(timeline.set_master_period(512.0 / 48_000.0));
        let raw = FractionalSourcePosition {
            frame: 1,
            fraction: 0.125,
            seek_mode: ExplicitSeekMode::Normal,
        };
        let e = MappedClockEpoch::general_compact(
            17,
            timeline,
            &g,
            SourceLoopDomain::physical(4_000, region),
            raw,
            0,
        )
        .unwrap();
        assert_eq!(g.position_at(e, 0).unwrap(), raw);
        for frame in [1, 511, 512, 10_001, (1_u64 << 54) + 3, u64::MAX - 1] {
            let actual = g.position_at(e, frame).unwrap();
            let span = g.refresh_span(e, frame).unwrap();
            let local = span.position_in_domain(g.domain(), frame).unwrap();
            assert_eq!(
                (actual.frame, actual.fraction.to_bits()),
                (local.frame, local.fraction.to_bits())
            );
            assert!((region.start..region.end).contains(&actual.frame));
        }
    }

    #[test]
    fn physical_positive_interior_excludes_exterior_gaps_and_does_not_require_bar_meter() {
        let anchors = [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
        let map = map_with_source(
            &anchors,
            &["gap_unknown_count", "trusted", "gap_unknown_count"],
            false,
            1,
            4,
        );
        let region = FrameRange { start: 1, end: 2 };
        assert!(MappedLoopGeometry::new_general_physical(map.clone(), 4, 1, region, false).is_ok());
        assert!(MappedLoopGeometry::new_general_physical(map, 4, 1, region, true).is_err());
        let map = map_with_source(
            &anchors,
            &["trusted", "gap_unknown_count", "trusted"],
            false,
            1,
            4,
        );
        assert!(
            MappedLoopGeometry::new_general_physical(
                map,
                4,
                1,
                FrameRange { start: 0, end: 3 },
                false
            )
            .is_err()
        );
    }

    #[test]
    fn physical_end_and_original_eof_beyond_trusted_map_refuse_without_extrapolation() {
        let map = map_with_source(
            &[(0.0, 0.0), (3.0, 2.0), (8.0, 5.0)],
            &["trusted", "trusted"],
            false,
            1,
            10,
        );
        for end in [9, 10] {
            assert!(matches!(
                MappedLoopGeometry::new_general_physical(
                    map.clone(),
                    10,
                    1,
                    FrameRange { start: 1, end },
                    false,
                ),
                Err(MappedClockError::UnsupportedCoverage)
            ));
        }
    }

    #[test]
    fn physical_pause_terminal_span_and_malformed_cycle_keep_original_anchor_contract() {
        let g = geometry();
        let e = epoch(&g, 0.125);
        let resumed = e.resumed_at(&timeline(), 9, 109).unwrap();
        assert_eq!(
            g.position_at(e, 9).unwrap(),
            g.position_at(resumed, 109).unwrap()
        );
        assert_eq!(
            g.position_at(e, 19).unwrap(),
            g.position_at(resumed, 119).unwrap()
        );
        let terminal = g.refresh_span(e, u64::MAX - 1).unwrap();
        assert_eq!(terminal.end_frame(), u64::MAX);
        assert!(terminal.position_in_domain(g.domain(), u64::MAX).is_some());
        assert!(g.refresh_span(e, u64::MAX).is_err());
        let mut span = g.refresh_span(e, 0).unwrap();
        span.program = SpanProgram::physical([u64::MAX, u64::MAX]);
        assert!(span.local_general_view(g.domain()).is_err());
    }
}
