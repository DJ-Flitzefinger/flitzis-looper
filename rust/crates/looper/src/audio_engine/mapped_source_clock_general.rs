//! Productive exact original-map coordinates. No owning state is copied.
#[path = "mapped_source_clock_general_physical.rs"]
mod physical;
use super::anchored::{AnchoredClock, first_true_near_high, first_true_near_low};
use super::{
    CanonicalBeatSegment, MappedClockEpoch, MappedClockError, MappedClockMode, MappedLoopGeometry,
    MappedSourceSpan, SpanProgram,
};
use crate::audio_engine::compact_source_numerics::{
    BaseGrid, Grid, GridStats, compare_scaled_f64_to_grid, decode_f64_at_grid,
    decode_scaled_f64_at_grid, decode_source_pair_at_grid, exact_f64_parts, master_grid_exponent,
    required_grid_f64,
};
use crate::audio_engine::general_source_numerics::{Integer, MappedReaderPeriod, on_grid};
use crate::audio_engine::source_beatmap::SourceBeatmap;
use crate::audio_engine::source_playback::{FractionalSourcePosition, SourcePlayback};
use crate::audio_engine::source_reader::{ExplicitSeekMode, FrameRange, SourceLoopDomain};
use crate::audio_engine::transport::TransportTimeline;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug)]
pub(super) struct GeneralPreparation {
    pub(super) source: BaseGrid,
    pub(super) beat: BaseGrid,
    pub(super) start_segment: CanonicalBeatSegment,
    pub(super) minimum: CanonicalBeatSegment,
    pub(super) maximum: CanonicalBeatSegment,
}
impl GeneralPreparation {
    pub(super) fn original_grids(self) -> (BaseGrid, BaseGrid) {
        (self.source, self.beat)
    }
}
#[derive(Clone, Copy)]
struct Axes {
    x0: i128,
    dx: i128,
    y0: i128,
    dy: i128,
}
impl Axes {
    fn new(segment: CanonicalBeatSegment, rate: u32, source: Grid, beat: Grid) -> Option<Self> {
        let x0 = decode_scaled_f64_at_grid(segment.source_start, rate, source)?;
        let x1 = decode_scaled_f64_at_grid(segment.source_end, rate, source)?;
        let y0 = decode_f64_at_grid(segment.beat_start, beat)?;
        let y1 = decode_f64_at_grid(segment.beat_end, beat)?;
        let dx = x1.checked_sub(x0)?;
        let dy = y1.checked_sub(y0)?;
        (dx > 0 && dy > 0).then_some(Self { x0, dx, y0, dy })
    }
    fn inverse(self, source: i128) -> Option<Integer> {
        Integer::from_i128(self.y0)?
            .mul(Integer::from_i128(self.dx)?)?
            .add(
                Integer::from_i128(source.checked_sub(self.x0)?)?
                    .mul(Integer::from_i128(self.dy)?)?,
            )
    }
}
fn compare_dyadics(a: Integer, ae: i16, b: Integer, be: i16) -> Option<Ordering> {
    if a.negative != b.negative {
        return Some(a.cmp(&b));
    }
    if a == Integer::ZERO || b == Integer::ZERO {
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
fn compare_beat(value: f64, n: Integer, d: Integer, grid: Grid) -> Option<Ordering> {
    let (coefficient, exponent) = exact_f64_parts(value)?;
    compare_dyadics(
        Integer::from_i128(coefficient)?.mul(d)?,
        exponent,
        n,
        grid.exponent(),
    )
}
fn include_segment(
    source: GridStats,
    beat: GridStats,
    s: CanonicalBeatSegment,
    rate: u32,
) -> Option<(GridStats, GridStats)> {
    Some((
        source
            .include_scaled_f64(s.source_start, rate)?
            .include_scaled_f64(s.source_end, rate)?,
        beat.include_f64(s.beat_start)?.include_f64(s.beat_end)?,
    ))
}
fn slope_less(a: Axes, b: Axes) -> Option<bool> {
    Some(
        Integer::from_i128(a.dx)?.mul(Integer::from_i128(b.dy)?)?
            < Integer::from_i128(b.dx)?.mul(Integer::from_i128(a.dy)?)?,
    )
}

impl MappedLoopGeometry {
    #[cfg(test)]
    pub(crate) fn general_local_payload_sizes_for_test() -> (usize, usize, usize) {
        (
            std::mem::size_of::<(MappedSourceSpan, Core)>(),
            std::mem::size_of::<physical::PhysicalLocalSpanView>(),
            std::mem::size_of::<super::full_fraction::FullFractionLocalView>(),
        )
    }
    /// Separate OFFRT admission; the legacy rounded inverse is not a prerequisite.
    pub(crate) fn new_general_declared(
        map: SourceBeatmap,
        sample_frames: usize,
        rate: u32,
        region: FrameRange,
        q: f64,
        require_four_four: bool,
    ) -> Result<Self, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        if rate == 0
            || region.start >= region.end
            || region.end > sample_frames
            || !q.is_finite()
            || q <= 0.0
        {
            return Err(MappedClockError::InvalidGeometry);
        }
        let source_stats = GridStats::new()
            .include_u64(region.start as u64)
            .ok_or(unavailable)?;
        let initial_grid = source_stats
            .finish()
            .and_then(|b| b.refine(0))
            .ok_or(unavailable)?;
        let initial_start = decode_source_pair_at_grid(region.start as u64, 0.0, initial_grid)
            .ok_or(unavailable)?;
        let first = map
            .canonical_segment_by_source_ordering(|s| {
                compare_scaled_f64_to_grid(s, rate, initial_start, initial_grid)
            })
            .ok_or(MappedClockError::UnsupportedCoverage)?;
        let (source_stats, beat_stats) = include_segment(
            source_stats,
            GridStats::new().include_f64(q).ok_or(unavailable)?,
            first,
            rate,
        )
        .ok_or(unavailable)?;
        let sg = source_stats
            .finish()
            .and_then(|b| b.refine(0))
            .ok_or(unavailable)?;
        let bg = beat_stats
            .finish()
            .and_then(|b| b.refine(0))
            .ok_or(unavailable)?;
        let ax = Axes::new(first, rate, sg, bg).ok_or(unavailable)?;
        let start = ax
            .inverse(decode_source_pair_at_grid(region.start as u64, 0.0, sg).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        let divisor = Integer::from_i128(ax.dx).ok_or(unavailable)?;
        let end = start
            .add(
                on_grid(q, bg)
                    .and_then(|v| v.mul(divisor))
                    .ok_or(unavailable)?,
            )
            .ok_or(unavailable)?;
        let mut source = Some(source_stats);
        let mut beat = Some(beat_stats);
        let mut minimum = None;
        let mut maximum = None;
        let mut low = f64::INFINITY;
        let mut high = 0.0_f64;
        map.visit_canonical_segments_exact(
            |v| compare_beat(v, start, divisor, bg),
            |v| compare_beat(v, end, divisor, bg),
            require_four_four,
            |s| {
                let (a, b) = include_segment(source?, beat?, s, rate)?;
                source = Some(a);
                beat = Some(b);
                let slope = s.seconds_per_quarter();
                if !slope.is_finite() || slope <= 0.0 {
                    return None;
                }
                low = low.min(slope);
                high = high.max(slope);
                // Initial grids need not contain another segment, so extrema are compared
                // after the final lossless global grids have been established below.
                minimum.get_or_insert(s);
                maximum = Some(s);
                Some(())
            },
        )
        .ok_or(MappedClockError::UnsupportedCoverage)?;
        let source = source.and_then(|s| s.finish()).ok_or(unavailable)?;
        let beat = beat.and_then(|s| s.finish()).ok_or(unavailable)?;
        let sg = source.refine(0).ok_or(unavailable)?;
        let bg = beat.refine(0).ok_or(unavailable)?;
        let ax = Axes::new(first, rate, sg, bg).ok_or(unavailable)?;
        let start = ax
            .inverse(decode_source_pair_at_grid(region.start as u64, 0.0, sg).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        let divisor = Integer::from_i128(ax.dx).ok_or(unavailable)?;
        let end = start
            .add(
                on_grid(q, bg)
                    .and_then(|v| v.mul(divisor))
                    .ok_or(unavailable)?,
            )
            .ok_or(unavailable)?;
        let end_segment = map
            .canonical_segment_by_beat_ordering(|v| {
                let ordering = compare_beat(v, end, divisor, bg)?;
                Some(if ordering == Ordering::Equal {
                    Ordering::Greater
                } else {
                    ordering
                })
            })
            .ok_or(MappedClockError::UnsupportedCoverage)?;
        let end_axes = Axes::new(end_segment, rate, sg, bg).ok_or(unavailable)?;
        let delta = end
            .sub(
                Integer::from_i128(end_axes.y0)
                    .and_then(|v| v.mul(divisor))
                    .ok_or(unavailable)?,
            )
            .ok_or(unavailable)?;
        let denominator = divisor
            .mul(Integer::from_i128(end_axes.dy).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        let numerator = Integer::from_i128(end_axes.x0)
            .and_then(|v| v.mul(denominator))
            .and_then(|v| v.add(delta.mul(Integer::from_i128(end_axes.dx)?)?))
            .and_then(|v| {
                v.sub(
                    Integer::from_i128(decode_source_pair_at_grid(region.start as u64, 0.0, sg)?)?
                        .mul(denominator)?,
                )
            })
            .ok_or(unavailable)?;
        let denominator = denominator
            .shl(u32::try_from(-sg.exponent()).map_err(|_| unavailable)?)
            .ok_or(unavailable)?;
        let period = MappedReaderPeriod::new(numerator, denominator).ok_or(unavailable)?;
        if !period.validate_physical(region)
            || period.exact_marker_end(region.start) != Some(region.end as u128)
        {
            return Err(MappedClockError::InvalidGeometry);
        }
        map.visit_canonical_segments_exact(
            |v| compare_beat(v, start, divisor, bg),
            |v| compare_beat(v, end, divisor, bg),
            require_four_four,
            |s| {
                let axes = Axes::new(s, rate, sg, bg)?;
                if let Some(old) = minimum {
                    if slope_less(axes, Axes::new(old, rate, sg, bg)?)? {
                        minimum = Some(s);
                    }
                } else {
                    minimum = Some(s);
                }
                if let Some(old) = maximum {
                    if slope_less(Axes::new(old, rate, sg, bg)?, axes)? {
                        maximum = Some(s);
                    }
                } else {
                    maximum = Some(s);
                }
                Some(())
            },
        )
        .ok_or(unavailable)?;
        Ok(Self {
            map,
            domain: SourceLoopDomain::mapped(sample_frames, region, period).ok_or(unavailable)?,
            // Rounded legacy observers remain diagnostic. General classification
            // and addressing use the original exact inverse expressions above.
            beat_start: first.beat_start
                + (region.start as f64 / f64::from(rate) - first.source_start)
                    / first.seconds_per_quarter(),
            quarters: q,
            source_end: (region.start as f64 + period.approximate()) / f64::from(rate),
            loaded_rate_hz: rate,
            min_seconds_per_quarter: low,
            max_seconds_per_quarter: high,
            exact_continuation: None,
            general: Some(GeneralPreparation {
                source,
                beat,
                start_segment: first,
                minimum: minimum.ok_or(unavailable)?,
                maximum: maximum.ok_or(unavailable)?,
            }),
            full_fraction: None,
            common_phase: None,
        })
    }
    pub(crate) fn has_general_continuation(&self) -> bool {
        self.general.is_some()
    }
    pub(crate) fn position_at_general(
        &self,
        epoch: MappedClockEpoch,
        frame: u64,
    ) -> Result<FractionalSourcePosition, MappedClockError> {
        if epoch.is_full_fraction_general() {
            return super::full_fraction::position(self, epoch, frame);
        }
        if self.domain.musical_period.physical_quarters().is_some() {
            return physical::position(self, epoch, frame);
        }
        let clock = Clock::new(self, epoch)?;
        clock
            .position(frame)
            .ok_or(MappedClockError::NumericalUnavailable)
    }
    pub(crate) fn refresh_general_span(
        &self,
        epoch: MappedClockEpoch,
        frame: u64,
    ) -> Result<MappedSourceSpan, MappedClockError> {
        if epoch.is_full_fraction_general() {
            return super::full_fraction::refresh(self, epoch, frame);
        }
        if self.domain.musical_period.physical_quarters().is_some() {
            return physical::refresh(self, epoch, frame);
        }
        let unavailable = MappedClockError::NumericalUnavailable;
        if frame == u64::MAX {
            return Err(unavailable);
        }
        let clock = Clock::new(self, epoch)?;
        let phase = clock.phase(frame).ok_or(unavailable)?;
        let segment = clock.segment(&phase).ok_or(unavailable)?;
        let cycle = phase.cycle;
        let start_beat = clock.start_num;
        let end_beat = start_beat
            .add(
                Integer::from_i128(clock.q)
                    .and_then(|v| v.mul(clock.start_den))
                    .ok_or(unavailable)?,
            )
            .ok_or(unavailable)?;
        let left = if compare_beat(
            segment.beat_start,
            start_beat,
            clock.start_den,
            clock.core.beat,
        )
        .ok_or(unavailable)?
            == Ordering::Less
        {
            start_beat
        } else {
            on_grid(segment.beat_start, clock.core.beat)
                .and_then(|v| v.mul(clock.start_den))
                .ok_or(unavailable)?
        };
        let right = if compare_beat(segment.beat_end, end_beat, clock.start_den, clock.core.beat)
            .ok_or(unavailable)?
            == Ordering::Greater
        {
            end_beat
        } else {
            on_grid(segment.beat_end, clock.core.beat)
                .and_then(|v| v.mul(clock.start_den))
                .ok_or(unavailable)?
        };
        let boundary = |v: Integer| -> Option<Integer> {
            v.sub(start_beat)?
                .add(cycle.mul(clock.start_den)?)?
                .mul(Integer::from_i128(clock.core.inverse.dx)?)
        };
        let lower = boundary(left).ok_or(unavailable)?;
        let upper = boundary(right).ok_or(unavailable)?;
        let start_frame = first_true_near_high(epoch.first_output_frame, frame, |n| {
            Some(clock.unfolded(n)? >= lower)
        })
        .ok_or(unavailable)?;
        let next = frame.checked_add(1).ok_or(unavailable)?;
        let end_frame = if clock.unfolded(u64::MAX).ok_or(unavailable)? < upper {
            u64::MAX
        } else {
            first_true_near_low(next, u64::MAX, |n| Some(clock.unfolded(n)? >= upper))
                .ok_or(unavailable)?
        };
        let endpoint = clock.position(end_frame).ok_or(unavailable)?;
        let cycle = cycle.u128().ok_or(unavailable)?;
        let span = MappedSourceSpan {
            epoch,
            segment,
            program: SpanProgram::general(
                [cycle as u64, (cycle >> 64) as u64],
                clock.core.beat.exponent(),
            ),
            loaded_rate_hz: self.loaded_rate_hz,
            start_frame,
            end_frame,
            endpoint_frame: endpoint.frame,
            endpoint_fraction: endpoint.fraction,
            source_start: self.domain.region.start as u64,
        };
        span.position_in_domain(self.domain, frame)
            .ok_or(unavailable)?;
        span.local_rate().ok_or(MappedClockError::UnsupportedRate)?;
        Ok(span)
    }
}

impl MappedClockEpoch {
    pub(crate) fn general_compact(
        marker: u64,
        timeline: TransportTimeline,
        geometry: &MappedLoopGeometry,
        original_scalar_domain: SourceLoopDomain,
        position: FractionalSourcePosition,
        output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        if marker == 0 || output_frame == u64::MAX {
            return Err(MappedClockError::InvalidEpoch);
        }
        if original_scalar_domain.sample_frames != geometry.domain.sample_frames
            || original_scalar_domain.region != geometry.domain.region
            || original_scalar_domain.musical_period.is_mapped()
            || position.seek_mode != ExplicitSeekMode::Normal
        {
            return Err(unavailable);
        }
        // Numerical class validation only. A replacement's actual raw pair may
        // be outside its retained historical scalar circle. Source permission
        // and actual preceding/target first PCM remain with the original claim.
        let scalar_probe = FractionalSourcePosition {
            frame: original_scalar_domain.region.start,
            fraction: 0.0,
            seek_mode: ExplicitSeekMode::Normal,
        };
        if original_scalar_domain
            .musical_period
            .scalar_period()
            .is_some()
        {
            SourcePlayback::scalar_musical_continuation(
                scalar_probe,
                original_scalar_domain,
                1.0,
                1.0,
            )
            .ok_or(unavailable)?;
        } else {
            SourcePlayback::scalar_continuation(scalar_probe, original_scalar_domain, 1.0, 1.0)
                .ok_or(unavailable)?;
        }
        Self::general_coordinates(marker, timeline, geometry, position, output_frame)
    }

    /// Pure coordinates for an already mapped claimant. This does not fabricate
    /// a scalar birth witness; the caller retains and checks its previous owner.
    pub(super) fn general_coordinates(
        marker: u64,
        timeline: TransportTimeline,
        geometry: &MappedLoopGeometry,
        position: FractionalSourcePosition,
        output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        if marker == 0 || output_frame == u64::MAX || position.seek_mode != ExplicitSeekMode::Normal
        {
            return Err(MappedClockError::InvalidEpoch);
        }
        let period = geometry
            .domain
            .musical_period
            .mapped_period(geometry.domain.region)
            .ok_or(unavailable)?;
        if !period.canonical_pair(
            position.frame,
            position.fraction,
            geometry.domain.region.start,
        ) {
            return Err(unavailable);
        }
        let prep = geometry.general.ok_or(unavailable)?;
        let grid = prep
            .source
            .refine(required_grid_f64(position.fraction).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        let raw = decode_source_pair_at_grid(position.frame as u64, position.fraction, grid)
            .ok_or(unavailable)?;
        let inverse_segment = geometry
            .map
            .canonical_segment_by_source_ordering(|s| {
                compare_scaled_f64_to_grid(s, geometry.loaded_rate_hz, raw, grid)
            })
            .ok_or(unavailable)?;
        let projection = timeline
            .beat_projection()
            .ok_or(MappedClockError::InvalidEpoch)?;
        let (original_master, _) = projection
            .finite_beats_for_horizon(output_frame, u64::MAX)
            .ok_or(unavailable)?;
        let epoch = Self {
            marker,
            projection,
            address_shift: 0,
            first_output_frame: output_frame,
            mode: MappedClockMode::GeneralCompact(AnchoredClock {
                frame: position.frame as u64,
                fraction: position.fraction,
                original_output_frame: output_frame,
                original_master,
                inverse_segment,
            }),
        };
        if geometry.domain.musical_period.physical_quarters().is_some() {
            physical::validate_epoch(geometry, epoch)?;
        } else {
            Clock::new(geometry, epoch)?;
        }
        let span = geometry.refresh_general_span(epoch, output_frame)?;
        let observed = span
            .position_in_domain(geometry.domain, output_frame)
            .ok_or(unavailable)?;
        if observed.frame != position.frame
            || observed.fraction.to_bits() != position.fraction.to_bits()
        {
            return Err(unavailable);
        }
        Ok(epoch)
    }
}

#[derive(Clone, Copy)]
struct Core {
    epoch: MappedClockEpoch,
    source: Grid,
    beat: Grid,
    inverse: Axes,
    raw_num: Integer,
    m0: i128,
    period: MappedReaderPeriod,
    start: usize,
    cycle: Integer,
}
impl Core {
    fn new(
        epoch: MappedClockEpoch,
        rate: u32,
        source: BaseGrid,
        beat: BaseGrid,
        period: MappedReaderPeriod,
        start: usize,
        cycle: (Integer, i16),
    ) -> Option<Self> {
        let (cycle, cycle_exponent) = cycle;
        let anchor = epoch.anchor()?;
        let first = epoch.first_output_frame.checked_sub(epoch.address_shift)?;
        let last = u64::MAX.checked_sub(epoch.address_shift)?;
        if first < anchor.original_output_frame {
            return None;
        }
        let horizon = epoch.projection.finite_beats_for_horizon(first, last)?;
        let source = source.refine(required_grid_f64(anchor.fraction)?)?;
        let exponent = master_grid_exponent(
            epoch.projection.frames_per_beat(),
            epoch.projection.original_beat_anchor(),
        )?
        .min(required_grid_f64(anchor.original_master)?)
        .min(cycle_exponent);
        let beat = beat.refine(exponent)?;
        decode_f64_at_grid(horizon.0, beat)?;
        decode_f64_at_grid(horizon.1, beat)?;
        let raw = decode_source_pair_at_grid(anchor.frame, anchor.fraction, source)?;
        if !period.canonical_pair(usize::try_from(anchor.frame).ok()?, anchor.fraction, start) {
            return None;
        }
        let inverse = Axes::new(anchor.inverse_segment, rate, source, beat)?;
        if raw < inverse.x0 || raw > inverse.x0.checked_add(inverse.dx)? {
            return None;
        }
        let cycle = cycle.shl(u32::try_from(cycle_exponent - beat.exponent()).ok()?)?;
        if cycle.magnitude.bit_width() > 128 {
            return None;
        }
        Some(Self {
            epoch,
            source,
            beat,
            inverse,
            raw_num: inverse.inverse(raw)?,
            m0: decode_f64_at_grid(anchor.original_master, beat)?,
            period,
            start,
            cycle,
        })
    }
    fn beat_num(&self, frame: u64) -> Option<Integer> {
        let delta = decode_f64_at_grid(self.epoch.addressed_master_at(frame)?, self.beat)?
            .checked_sub(self.m0)?;
        self.raw_num.add(
            Integer::from_i128(delta)?
                .sub(self.cycle)?
                .mul(Integer::from_i128(self.inverse.dx)?)?,
        )
    }
    fn position(
        &self,
        segment: CanonicalBeatSegment,
        rate: u32,
        frame: u64,
    ) -> Option<FractionalSourcePosition> {
        let axes = Axes::new(segment, rate, self.source, self.beat)?;
        let offset = self
            .beat_num(frame)?
            .sub(Integer::from_i128(axes.y0)?.mul(Integer::from_i128(self.inverse.dx)?)?)?;
        let d = Integer::from_i128(self.inverse.dx)?.mul(Integer::from_i128(axes.dy)?)?;
        let n = Integer::from_i128(axes.x0)?
            .mul(d)?
            .add(offset.mul(Integer::from_i128(axes.dx)?)?)?;
        let d = d.shl(u32::try_from(-self.source.exponent()).ok()?)?;
        let coordinate = self
            .period
            .split(n, d, self.epoch.anchor()?.fraction, self.start)?;
        Some(FractionalSourcePosition {
            frame: usize::try_from(coordinate.whole).ok()?,
            fraction: coordinate.fraction,
            seek_mode: ExplicitSeekMode::Normal,
        })
    }
}

struct Phase {
    cycle: Integer,
    beat_num: Integer,
}
struct Clock<'a> {
    geometry: &'a MappedLoopGeometry,
    core: Core,
    start_num: Integer,
    start_den: Integer,
    q: i128,
}
impl<'a> Clock<'a> {
    fn new(
        geometry: &'a MappedLoopGeometry,
        epoch: MappedClockEpoch,
    ) -> Result<Self, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        if !epoch.is_general_compact()
            || epoch.marker == 0
            || geometry.domain.musical_period.physical_quarters().is_some()
        {
            return Err(unavailable);
        }
        let p = geometry.general.ok_or(unavailable)?;
        let anchor = epoch.anchor().ok_or(unavailable)?;
        if !geometry
            .map
            .canonical_segment_at_beat(anchor.inverse_segment.beat_start)
            .is_some_and(|original| original.matches_exact(anchor.inverse_segment))
        {
            return Err(unavailable);
        }
        let period = geometry
            .domain
            .musical_period
            .mapped_period(geometry.domain.region)
            .ok_or(unavailable)?;
        let core = Core::new(
            epoch,
            geometry.loaded_rate_hz,
            p.source,
            p.beat,
            period,
            geometry.domain.region.start,
            (Integer::ZERO, 0),
        )
        .ok_or(unavailable)?;
        let first = Axes::new(
            p.start_segment,
            geometry.loaded_rate_hz,
            core.source,
            core.beat,
        )
        .ok_or(unavailable)?;
        let start_num = first
            .inverse(
                decode_source_pair_at_grid(geometry.domain.region.start as u64, 0.0, core.source)
                    .ok_or(unavailable)?,
            )
            .ok_or(unavailable)?;
        let start_den = Integer::from_i128(first.dx).ok_or(unavailable)?;
        let q = decode_f64_at_grid(geometry.quarters, core.beat).ok_or(unavailable)?;
        if q <= 0 {
            return Err(unavailable);
        }
        geometry.validate_epoch(epoch)?;
        for segment in [p.minimum, p.maximum] {
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
            q,
        };
        let initial = clock
            .unfolded(epoch.first_output_frame)
            .ok_or(unavailable)?;
        if initial < Integer::ZERO {
            return Err(unavailable);
        }
        // Full-horizon endpoint admission plus coefficient bounds prove every
        // monotone phase/search product fits the new512-bit class.
        clock.phase(epoch.first_output_frame).ok_or(unavailable)?;
        clock.phase(u64::MAX).ok_or(unavailable)?;
        Ok(clock)
    }
    fn unfolded(&self, frame: u64) -> Option<Integer> {
        self.core.beat_num(frame)?.mul(self.start_den)?.sub(
            self.start_num
                .mul(Integer::from_i128(self.core.inverse.dx)?)?,
        )
    }
    fn phase(&self, frame: u64) -> Option<Phase> {
        let unfolded = self.unfolded(frame)?;
        let denominator = self
            .start_den
            .mul(Integer::from_i128(self.core.inverse.dx)?)?;
        let period = denominator.mul(Integer::from_i128(self.q)?)?;
        let k = unfolded.quotient(period)?;
        let cycle = k.mul(Integer::from_i128(self.q)?)?;
        if cycle.magnitude.bit_width() > 128 {
            return None;
        }
        let beat_num = self
            .core
            .beat_num(frame)?
            .sub(cycle.mul(Integer::from_i128(self.core.inverse.dx)?)?)?;
        Some(Phase { cycle, beat_num })
    }
    fn segment(&self, phase: &Phase) -> Option<CanonicalBeatSegment> {
        self.geometry.map.canonical_segment_by_beat_ordering(|v| {
            compare_beat(
                v,
                phase.beat_num,
                Integer::from_i128(self.core.inverse.dx)?,
                self.core.beat,
            )
        })
    }
    fn position(&self, frame: u64) -> Option<FractionalSourcePosition> {
        let phase = self.phase(frame)?;
        let segment = self.segment(&phase)?;
        let core = Core {
            cycle: phase.cycle,
            ..self.core
        };
        core.position(segment, self.geometry.loaded_rate_hz, frame)
    }
}

/// Once-per-chunk local context; no map owner or per-frame inverse/modulo.
pub(crate) struct GeneralLocalSpanView {
    state: LocalGeneralState,
}
enum LocalGeneralState {
    Full(super::full_fraction::FullFractionLocalView),
    Declared { span: MappedSourceSpan, core: Core },
    Physical(physical::PhysicalLocalSpanView),
}
impl GeneralLocalSpanView {
    pub(crate) fn position_at(&self, frame: u64) -> Option<FractionalSourcePosition> {
        match &self.state {
            LocalGeneralState::Full(view) => view.position_at(frame),
            LocalGeneralState::Physical(view) => view.position_at(frame),
            LocalGeneralState::Declared { span, core } => {
                if frame == span.end_frame {
                    return Some(FractionalSourcePosition {
                        frame: span.endpoint_frame,
                        fraction: span.endpoint_fraction,
                        seek_mode: ExplicitSeekMode::Normal,
                    });
                }
                span.contains(frame).then_some(())?;
                core.position(span.segment, span.loaded_rate_hz, frame)
            }
        }
    }
}
impl MappedSourceSpan {
    /// Coordinates only. The caller supplies the exact domain privately bound to
    /// this span by the same prepared geometry. Standalone spans store no period
    /// certificate and cannot establish that association or an adoption permit.
    pub(crate) fn position_in_domain(
        self,
        domain: SourceLoopDomain,
        frame: u64,
    ) -> Option<FractionalSourcePosition> {
        self.position_in_domain_with_reference(
            domain,
            frame,
            super::LoopPhaseReference::Unavailable,
        )
    }

    pub(crate) fn position_in_domain_with_reference(
        self,
        domain: SourceLoopDomain,
        frame: u64,
        reference: super::LoopPhaseReference,
    ) -> Option<FractionalSourcePosition> {
        if self.epoch.is_common_phase() {
            self.local_phase_view(domain, reference)
                .ok()?
                .position_at(frame)
        } else if self.epoch.is_general_family() {
            self.local_general_view(domain).ok()?.position_at(frame)
        } else {
            self.position_at(frame)
        }
    }
    /// Borrow numeric context for an already bound span/domain tuple. Playback's
    /// private construction and full-domain refresh/configure guards maintain
    /// that tuple; this method does not certify arbitrary compatible domains.
    pub(crate) fn local_general_view(
        self,
        domain: SourceLoopDomain,
    ) -> Result<GeneralLocalSpanView, MappedClockError> {
        if self.epoch.is_full_fraction_general() {
            return super::full_fraction::local(self, domain)
                .map(|view| GeneralLocalSpanView {
                    state: LocalGeneralState::Full(view),
                })
                .ok_or(MappedClockError::NumericalUnavailable);
        }
        if domain.musical_period.physical_quarters().is_some() {
            return physical::local_view(self, domain).map(|view| GeneralLocalSpanView {
                state: LocalGeneralState::Physical(view),
            });
        }
        let unavailable = MappedClockError::NumericalUnavailable;
        if !self.epoch.is_general_compact()
            || !domain.is_valid()
            || self.source_start != domain.region.start as u64
            || self.start_frame < self.epoch.first_output_frame
            || self.start_frame >= self.end_frame
        {
            return Err(unavailable);
        }
        let period = domain
            .musical_period
            .mapped_period(domain.region)
            .ok_or(unavailable)?;
        if !period.canonical_pair(
            self.endpoint_frame,
            self.endpoint_fraction,
            domain.region.start,
        ) {
            return Err(unavailable);
        }
        let (cycle_coefficient, cycle_exponent) =
            self.program.general_value().ok_or(unavailable)?;
        let anchor = self.epoch.anchor().ok_or(unavailable)?;
        let mut source = GridStats::new()
            .include_u64(self.source_start)
            .and_then(|s| s.include_u64(anchor.frame))
            .and_then(|s| s.include_f64(anchor.fraction))
            .ok_or(unavailable)?;
        let mut beat = GridStats::new();
        for s in [anchor.inverse_segment, self.segment] {
            let pair = include_segment(source, beat, s, self.loaded_rate_hz).ok_or(unavailable)?;
            source = pair.0;
            beat = pair.1;
        }
        let first = self
            .epoch
            .first_output_frame
            .checked_sub(self.epoch.address_shift)
            .ok_or(unavailable)?;
        let last = u64::MAX
            .checked_sub(self.epoch.address_shift)
            .ok_or(unavailable)?;
        let horizon = self
            .epoch
            .projection
            .finite_beats_for_horizon(first, last)
            .ok_or(unavailable)?;
        beat = beat
            .include_f64(anchor.original_master)
            .and_then(|s| s.include_f64(horizon.0))
            .and_then(|s| s.include_f64(horizon.1))
            .ok_or(unavailable)?;
        let cycle = u128::from(cycle_coefficient[0]) | (u128::from(cycle_coefficient[1]) << 64);
        let core = Core::new(
            self.epoch,
            self.loaded_rate_hz,
            source.finish().ok_or(unavailable)?,
            beat.finish().ok_or(unavailable)?,
            domain
                .musical_period
                .mapped_period(domain.region)
                .ok_or(unavailable)?,
            domain.region.start,
            (
                Integer::from_u128(cycle).ok_or(unavailable)?,
                cycle_exponent,
            ),
        )
        .ok_or(unavailable)?;
        Ok(GeneralLocalSpanView {
            state: LocalGeneralState::Declared { span: self, core },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::mapped_source_clock::tests::map_with_source;

    fn geometry(coverage: &[&str], meter: bool) -> MappedLoopGeometry {
        let map = map_with_source(
            &[(0.0, 0.0), (3.0, 2.0), (8.0, 5.0)],
            coverage,
            meter,
            1,
            10,
        );
        MappedLoopGeometry::new_general_declared(
            map,
            10,
            1,
            FrameRange { start: 1, end: 4 },
            2.0,
            meter,
        )
        .unwrap()
    }
    fn timeline() -> TransportTimeline {
        let mut t = TransportTimeline::new(1);
        assert!(t.set_master_period(2.0));
        t
    }
    fn epoch(g: &MappedLoopGeometry, fraction: f64) -> MappedClockEpoch {
        MappedClockEpoch::general_compact(
            7,
            timeline(),
            g,
            SourceLoopDomain::physical(10, FrameRange { start: 1, end: 4 }),
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
    fn original_inverse_and_rational_period_follow_independent_small_integer_oracle() {
        let g = geometry(&["trusted", "trusted"], true);
        let e = epoch(&g, -0.0);
        // Independently B(1)=2/3, B(1)+Q=8/3, S(8/3)=37/9, P=28/9.
        // With actual M(n)=n/2, the four source phases are1,7/4,5/2,59/18.
        let expected = [
            (1, (-0.0_f64).to_bits()),
            (1, 0.75_f64.to_bits()),
            (2, 0.5_f64.to_bits()),
            (3, (5.0_f64 / 18.0).to_bits()),
        ];
        for n in 0..24 {
            let actual = g.position_at(e, n).unwrap();
            let (frame, fraction) = expected[n as usize % 4];
            assert_eq!((actual.frame, actual.fraction.to_bits()), (frame, fraction));
            let span = g.refresh_span(e, n).unwrap();
            assert_eq!(span.position_in_domain(g.domain(), n), Some(actual));
            assert_eq!(
                span.local_general_view(g.domain()).unwrap().position_at(n),
                Some(actual)
            );
            assert_eq!(span.position_at(n), None);
        }
        assert!(std::mem::size_of::<MappedSourceSpan>() <= 240);
        assert_eq!(std::mem::size_of::<MappedClockEpoch>(), 128);
    }

    #[test]
    fn bound_general_view_rejects_malformed_domain_and_noncanonical_cached_endpoint() {
        let g = geometry(&["trusted", "trusted"], true);
        let span = g.refresh_span(epoch(&g, 0.0), 0).unwrap();
        let original = span.position_in_domain(g.domain(), 0).unwrap();
        let invalid = SourceLoopDomain {
            sample_frames: 2,
            ..g.domain()
        };
        assert!(span.local_general_view(invalid).is_err());
        let physical = SourceLoopDomain::physical(10, g.domain().region);
        assert!(span.local_general_view(physical).is_err());
        let invalid = MappedSourceSpan {
            endpoint_frame: 10,
            ..span
        };
        assert!(invalid.local_general_view(g.domain()).is_err());
        let invalid = MappedSourceSpan {
            endpoint_fraction: f64::NAN,
            ..span
        };
        assert!(invalid.local_general_view(g.domain()).is_err());
        assert_eq!(span.position_in_domain(g.domain(), 0), Some(original));
    }
    #[test]
    fn actual_cast_plateaus_and_shifted_pause_queries_keep_original_evaluation() {
        let g = geometry(&["trusted", "trusted"], true);
        let e = epoch(&g, 0.0);
        let n = 1_u64 << 53;
        assert_eq!(
            g.position_at(e, n).unwrap(),
            g.position_at(e, n + 1).unwrap()
        );
        assert_eq!(g.position_at(e, n + 2).unwrap().fraction, 0.5);
        let resumed = e.resumed_at(&timeline(), 2, 102).unwrap();
        assert_eq!(
            g.position_at(e, 2).unwrap(),
            g.position_at(resumed, 102).unwrap()
        );
        assert_eq!(
            g.position_at(e, 9).unwrap(),
            g.position_at(resumed, 109).unwrap()
        );
    }
    #[test]
    fn ordinary_decimal_original_products_are_accepted_by_separate_general_class() {
        let map = map_with_source(
            &[(0.0, 0.0), (0.03, 2.0), (0.08, 5.0)],
            &["trusted", "trusted"],
            true,
            100,
            1000,
        );
        let region = FrameRange { start: 1, end: 4 };
        let old = MappedLoopGeometry::new(map.clone(), 1000, 100, region, 2.0, true).unwrap();
        assert!(old.qualify_exact_continuation().is_err());
        let g =
            MappedLoopGeometry::new_general_declared(map, 1000, 100, region, 2.0, true).unwrap();
        let mut t = TransportTimeline::new(100);
        assert!(t.set_master_period(0.02));
        let raw = FractionalSourcePosition {
            frame: 1,
            fraction: 0.125,
            seek_mode: ExplicitSeekMode::Normal,
        };
        let e = MappedClockEpoch::general_compact(
            3,
            t,
            &g,
            SourceLoopDomain::physical(1000, region),
            raw,
            0,
        )
        .unwrap();
        assert_eq!(g.position_at(e, 0).unwrap(), raw);
    }
    #[test]
    fn exact_interval_refuses_real_gap_and_required_meter() {
        let map = map_with_source(
            &[(0.0, 0.0), (3.0, 2.0), (8.0, 5.0)],
            &["trusted", "gap_unknown_count"],
            true,
            1,
            10,
        );
        assert!(
            MappedLoopGeometry::new_general_declared(
                map,
                10,
                1,
                FrameRange { start: 1, end: 4 },
                2.0,
                true
            )
            .is_err()
        );
        let map = map_with_source(
            &[(0.0, 0.0), (3.0, 2.0), (8.0, 5.0)],
            &["trusted", "trusted"],
            false,
            1,
            10,
        );
        assert!(
            MappedLoopGeometry::new_general_declared(
                map,
                10,
                1,
                FrameRange { start: 1, end: 4 },
                2.0,
                true
            )
            .is_err()
        );
    }
    #[test]
    fn exterior_gap_touching_exact_declared_end_does_not_contaminate_interior() {
        let map = map_with_source(
            &[(0.0, 0.0), (3.0, 2.0), (8.0, 5.0)],
            &["trusted", "gap_unknown_count"],
            true,
            1,
            10,
        );
        let g = MappedLoopGeometry::new_general_declared(
            map,
            10,
            1,
            FrameRange { start: 0, end: 3 },
            2.0,
            true,
        )
        .unwrap();
        let raw = FractionalSourcePosition {
            frame: 0,
            fraction: 0.0,
            seek_mode: ExplicitSeekMode::Normal,
        };
        let e = MappedClockEpoch::general_compact(
            2,
            timeline(),
            &g,
            SourceLoopDomain::physical(10, FrameRange { start: 0, end: 3 }),
            raw,
            0,
        )
        .unwrap();
        assert_eq!(g.position_at(e, 4).unwrap(), raw);
    }

    #[test]
    fn general_replacement_does_not_reapply_historical_scalar_membership() {
        let g = geometry(&["trusted", "trusted"], true);
        let region = FrameRange { start: 1, end: 4 };
        let original = SourceLoopDomain::musical(10, region, 2.75).unwrap();
        let raw = FractionalSourcePosition {
            frame: 3,
            fraction: 0.9,
            seek_mode: ExplicitSeekMode::Normal,
        };
        assert!(SourcePlayback::scalar_musical_continuation(raw, original, 1.0, 1.0).is_none());
        let e = MappedClockEpoch::general_compact(9, timeline(), &g, original, raw, 0).unwrap();
        assert_eq!(g.position_at(e, 0).unwrap(), raw);
    }
    #[test]
    fn retained_scalar_class_still_requires_its_supported_period_bounds() {
        let map = map_with_source(&[(0.0, 0.0), (3.0, 3.0)], &["trusted"], true, 1, 10);
        let region = FrameRange { start: 0, end: 1 };
        let g = MappedLoopGeometry::new_general_declared(map, 10, 1, region, 1.0, true).unwrap();
        let original = SourceLoopDomain::musical(10, region, 0.25).unwrap();
        let raw = FractionalSourcePosition {
            frame: 0,
            fraction: 0.0,
            seek_mode: ExplicitSeekMode::Normal,
        };
        let mut t = TransportTimeline::new(1);
        assert!(t.set_master_period(1.0));
        assert!(MappedClockEpoch::general_compact(9, t, &g, original, raw, 0).is_err());
    }
}
