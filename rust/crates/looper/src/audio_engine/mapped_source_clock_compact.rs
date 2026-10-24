//! Bounded compact coordinates from the same original immutable geometry.
//!
//! Whole-geometry admission creates the compact tag. Its copied spans can prepare local
//! arithmetic without owning geometry; source/adoption permission remains with the claimant.

use super::anchored::{AnchoredClock, first_true_near_high, first_true_near_low};
use super::{
    CanonicalBeatSegment, MappedClockEpoch, MappedClockError, MappedClockMode, MappedLoopGeometry,
    MappedSourceSpan,
};
use crate::audio_engine::compact_source_numerics::{
    BaseGrid, CoordinateInteger, Grid, GridStats, PhaseInteger, compare_scaled_f64_to_grid,
    decode_f64_at_grid, decode_scaled_f64_at_grid, decode_source_pair_at_grid, exact_f64_parts,
    master_grid_exponent, normalize_source_coordinate, required_grid_f64,
    split_normalized_source_coordinate,
};
use crate::audio_engine::source_numerics::SourceCoordinate;
use crate::audio_engine::source_playback::{FractionalSourcePosition, SourcePlayback};
use crate::audio_engine::source_reader::{ExplicitSeekMode, SourceLoopDomain};
use crate::audio_engine::transport::TransportTimeline;
use std::cmp::Ordering;

#[cfg(test)]
thread_local! {
    static QUERY_COUNTS: std::cell::Cell<Option<(usize, usize)>> = const { std::cell::Cell::new(None) };
}

/// Count local preparations and entries into the explicit wide implementation on this thread.
#[cfg(test)]
pub(crate) fn observe_compact_queries_for_test<T>(
    action: impl FnOnce() -> T,
) -> (T, (usize, usize)) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            QUERY_COUNTS.with(|counts| counts.set(None));
        }
    }
    QUERY_COUNTS.with(|counts| assert!(counts.replace(Some((0, 0))).is_none()));
    let _reset = Reset;
    let result = action();
    let counts = QUERY_COUNTS.with(|counts| counts.get().unwrap());
    (result, counts)
}

#[cfg(test)]
pub(super) fn observe_wide_entry_for_test() {
    QUERY_COUNTS.with(|counts| {
        if let Some((local, wide)) = counts.get() {
            counts.set(Some((local, wide + 1)));
        }
    });
}

/// Only common-grid/headroom metadata; original segments remain in the original map.
#[derive(Clone, Copy, Debug)]
pub(super) struct CompactGeometryPreparation {
    source: BaseGrid,
    beat: BaseGrid,
}

#[derive(Clone, Copy)]
pub(super) struct CompactGeometryBuilder {
    source: GridStats,
    beat: GridStats,
}

impl CompactGeometryBuilder {
    pub(super) fn new(geometry: &MappedLoopGeometry) -> Option<Self> {
        Some(Self {
            source: GridStats::new()
                .include_u64(geometry.domain.region.start as u64)?
                .include_f64(geometry.domain.musical_period.scalar_period()?)?,
            beat: GridStats::new()
                .include_f64(geometry.beat_start)?
                .include_f64(geometry.quarters)?
                .include_f64(geometry.beat_start + geometry.quarters)?,
        })
    }

    pub(super) fn include_segment(self, segment: CanonicalBeatSegment, rate: u32) -> Option<Self> {
        Some(Self {
            source: self
                .source
                .include_scaled_f64(segment.source_start, rate)?
                .include_scaled_f64(segment.source_end, rate)?,
            beat: self
                .beat
                .include_f64(segment.beat_start)?
                .include_f64(segment.beat_end)?,
        })
    }

    pub(super) fn finish(self) -> Option<CompactGeometryPreparation> {
        Some(CompactGeometryPreparation {
            source: self.source.finish()?,
            beat: self.beat.finish()?,
        })
    }
}

/// No owner, raw pointer or stored playback state. Borrow only for one checked query/read.
pub(crate) struct CompactSpanView<'a> {
    span: MappedSourceSpan,
    clock: CompactClock<'a>,
    segment: CompactPreparedSegment,
}

/// Derived only from a tagged, globally admitted copied span. No map association,
/// source permission, owner or additional stored playback state is created here.
pub(crate) struct CompactLocalSpanView {
    span: MappedSourceSpan,
    core: CompactClockCore,
    segment: CompactPreparedSegment,
}

#[derive(Clone, Copy)]
struct CompactSegment {
    source_delta: i128,
    beat_delta: i128,
    constant: PhaseInteger,
}

struct CompactPreparedSegment {
    source_delta: i128,
    numerator_constant: CoordinateInteger,
    denominator: CoordinateInteger,
}

struct CompactClock<'a> {
    geometry: &'a MappedLoopGeometry,
    core: CompactClockCore,
}

struct CompactClockCore {
    epoch: MappedClockEpoch,
    anchor: AnchoredClock,
    loaded_rate_hz: u32,
    source_start: u64,
    source_period: f64,
    source_grid: Grid,
    beat_grid: Grid,
    start_beat: i128,
    inverse_delta: i128,
    original_master: i128,
    initial: PhaseInteger,
    period: PhaseInteger,
}

struct CompactPhase {
    unfolded: PhaseInteger,
    remainder: PhaseInteger,
}

impl MappedClockEpoch {
    /// Invert the original separate raw pair at actual E with bounded small
    /// arithmetic. Original source/map custody and the live claim are separate.
    pub(crate) fn anchored_compact(
        marker: u64,
        timeline: TransportTimeline,
        geometry: &MappedLoopGeometry,
        original_domain: SourceLoopDomain,
        position: FractionalSourcePosition,
        output_frame: u64,
    ) -> Result<Self, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        if marker == 0 || output_frame == u64::MAX {
            return Err(MappedClockError::InvalidEpoch);
        }
        if !geometry.exact_continuation_matches_domain(original_domain) {
            return Err(unavailable);
        }
        SourcePlayback::scalar_musical_continuation(position, original_domain, 1.0, 1.0)
            .ok_or(unavailable)?;
        let preparation = geometry
            .exact_continuation
            .and_then(|qualification| qualification.compact)
            .ok_or(unavailable)?;
        let source_grid = preparation
            .source
            .refine(required_grid_f64(position.fraction).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        let frame = u64::try_from(position.frame).map_err(|_| unavailable)?;
        let source =
            decode_source_pair_at_grid(frame, position.fraction, source_grid).ok_or(unavailable)?;
        let projection = timeline
            .beat_projection()
            .ok_or(MappedClockError::InvalidEpoch)?;
        let (original_master, _) = projection
            .finite_beats_for_horizon(output_frame, u64::MAX)
            .ok_or(unavailable)?;
        let inverse_segment = geometry
            .map
            .canonical_segment_by_source_ordering(|seconds| {
                compare_scaled_f64_to_grid(seconds, geometry.loaded_rate_hz, source, source_grid)
            })
            .ok_or(unavailable)?;
        let epoch = Self {
            marker,
            projection,
            address_shift: 0,
            first_output_frame: output_frame,
            mode: MappedClockMode::CompactAnchored(AnchoredClock {
                frame,
                fraction: position.fraction,
                original_output_frame: output_frame,
                original_master,
                inverse_segment,
            }),
        };
        // The tag is returned only after the same complete original compact
        // geometry, phase, master-horizon and nominal/exact-extrema admission.
        CompactClock::new(geometry, epoch)?;
        geometry.refresh_span_compact(epoch, output_frame)?;
        Ok(epoch)
    }
}

impl MappedLoopGeometry {
    /// Coordinates only, using the explicit compact class with no reference fallback.
    pub(crate) fn position_at_compact(
        &self,
        epoch: MappedClockEpoch,
        output_frame: u64,
    ) -> Result<FractionalSourcePosition, MappedClockError> {
        CompactClock::new(self, epoch)?
            .position_at(output_frame)
            .ok_or(MappedClockError::NumericalUnavailable)
    }

    /// Same admitted boundary search and original endpoints as the wide reference.
    pub(crate) fn refresh_span_compact(
        &self,
        epoch: MappedClockEpoch,
        output_frame: u64,
    ) -> Result<MappedSourceSpan, MappedClockError> {
        if epoch.is_general_compact() {
            return self.refresh_general_span(epoch, output_frame);
        }
        let unavailable = MappedClockError::NumericalUnavailable;
        if output_frame == u64::MAX {
            return Err(unavailable);
        }
        let clock = CompactClock::new(self, epoch)?;
        let phase = clock.core.phase_at(output_frame).ok_or(unavailable)?;
        let segment = clock.forward_segment(&phase).ok_or(unavailable)?;
        let cycle_base = phase
            .unfolded
            .checked_sub(phase.remainder)
            .ok_or(unavailable)?;
        let loop_end = self.beat_start + self.quarters;
        let left = segment.beat_start.max(self.beat_start);
        let right = segment.beat_end.min(loop_end);
        let left_target = cycle_base
            .checked_add(clock.core.beat_offset(left).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        let right_offset = if right == loop_end {
            clock.core.period
        } else {
            clock.core.beat_offset(right).ok_or(unavailable)?
        };
        let right_target = cycle_base.checked_add(right_offset).ok_or(unavailable)?;
        if phase.unfolded < left_target || phase.unfolded >= right_target {
            return Err(unavailable);
        }
        let start_frame = first_true_near_high(epoch.first_output_frame, output_frame, |frame| {
            Some(clock.core.unfolded_at(frame)? >= left_target)
        })
        .ok_or(unavailable)?;
        let next = output_frame.checked_add(1).ok_or(unavailable)?;
        let end_frame = if clock.core.unfolded_at(u64::MAX).ok_or(unavailable)? < right_target {
            u64::MAX
        } else {
            first_true_near_low(next, u64::MAX, |frame| {
                Some(clock.core.unfolded_at(frame)? >= right_target)
            })
            .ok_or(unavailable)?
        };
        let endpoint = clock.position_at(end_frame).ok_or(unavailable)?;
        let span = MappedSourceSpan {
            epoch,
            segment,
            program: super::SpanProgram::Legacy(super::LegacySpanProgram {
                beat_start: self.beat_start,
                quarters: self.quarters,
                source_period: self
                    .domain
                    .musical_period
                    .scalar_period()
                    .ok_or(unavailable)?,
            }),
            loaded_rate_hz: self.loaded_rate_hz,
            start_frame,
            end_frame,
            endpoint_frame: endpoint.frame,
            endpoint_fraction: endpoint.fraction,
            source_start: self.domain.region.start as u64,
        };
        let prepared = clock.core.prepared_segment(segment).ok_or(unavailable)?;
        clock
            .core
            .position_on_segment(&phase, &prepared)
            .ok_or(unavailable)?;
        span.local_rate().ok_or(MappedClockError::UnsupportedRate)?;
        Ok(span)
    }
}

impl MappedSourceSpan {
    /// Validate numeric association with the borrowed original geometry. This proves no owner.
    pub(crate) fn compact_view(
        self,
        geometry: &MappedLoopGeometry,
    ) -> Result<CompactSpanView<'_>, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        let program = self.legacy_program().ok_or(unavailable)?;
        if program.beat_start.to_bits() != geometry.beat_start.to_bits()
            || program.quarters.to_bits() != geometry.quarters.to_bits()
            || self.loaded_rate_hz != geometry.loaded_rate_hz
            || self.source_start != geometry.domain.region.start as u64
            || geometry
                .domain
                .musical_period
                .scalar_period()
                .map(f64::to_bits)
                != Some(program.source_period.to_bits())
            || self.start_frame < self.epoch.first_output_frame
            || self.start_frame >= self.end_frame
            || !original_segment(geometry, self.segment)
        {
            return Err(unavailable);
        }
        let clock = CompactClock::new(geometry, self.epoch)?;
        // One output interval may skip multiple knots. Local segment equality cannot
        // establish that another geometry supplied the same cached global endpoint.
        let endpoint = clock.position_at(self.end_frame).ok_or(unavailable)?;
        if endpoint.frame != self.endpoint_frame
            || endpoint.fraction.to_bits() != self.endpoint_fraction.to_bits()
        {
            return Err(unavailable);
        }
        let segment = clock
            .core
            .prepared_segment(self.segment)
            .ok_or(unavailable)?;
        self.local_rate().ok_or(MappedClockError::UnsupportedRate)?;
        Ok(CompactSpanView {
            span: self,
            clock,
            segment,
        })
    }

    /// Only the private globally admitted tag permits this geometry-free query.
    /// The original map association and global endpoint were checked when the
    /// span was constructed/refreshed; a copied endpoint conveys no ownership.
    pub(crate) fn local_compact_view(self) -> Result<CompactLocalSpanView, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        if !self.epoch.is_compact_anchored()
            || self.start_frame < self.epoch.first_output_frame
            || self.start_frame >= self.end_frame
        {
            return Err(unavailable);
        }
        let program = self.legacy_program().ok_or(unavailable)?;
        let anchor = self.epoch.anchor().ok_or(unavailable)?;
        let horizon = finite_horizon(self.epoch, anchor).ok_or(unavailable)?;
        let preparation = local_preparation(self, anchor, horizon).ok_or(unavailable)?;
        let core = CompactClockCore::new(
            preparation,
            self.epoch,
            program.beat_start,
            program.quarters,
            self.loaded_rate_hz,
            self.source_start,
            program.source_period,
            horizon,
        )?;
        let segment = core.prepared_segment(self.segment).ok_or(unavailable)?;
        #[cfg(test)]
        QUERY_COUNTS.with(|counts| {
            if let Some((local, wide)) = counts.get() {
                counts.set(Some((local + 1, wide)));
            }
        });
        Ok(CompactLocalSpanView {
            span: self,
            core,
            segment,
        })
    }
}

impl CompactSpanView<'_> {
    pub(crate) fn position_at(&self, output_frame: u64) -> Option<FractionalSourcePosition> {
        position_in_prepared_span(self.span, &self.clock.core, &self.segment, output_frame)
    }
}

impl CompactLocalSpanView {
    pub(crate) fn position_at(&self, output_frame: u64) -> Option<FractionalSourcePosition> {
        position_in_prepared_span(self.span, &self.core, &self.segment, output_frame)
    }
}

fn position_in_prepared_span(
    span: MappedSourceSpan,
    core: &CompactClockCore,
    segment: &CompactPreparedSegment,
    output_frame: u64,
) -> Option<FractionalSourcePosition> {
    if output_frame == span.end_frame {
        return Some(FractionalSourcePosition {
            frame: span.endpoint_frame,
            fraction: span.endpoint_fraction,
            seek_mode: ExplicitSeekMode::Normal,
        });
    }
    if !span.contains(output_frame) {
        return None;
    }
    let phase = core.phase_at(output_frame)?;
    core.position_on_segment(&phase, segment)
}

fn finite_horizon(epoch: MappedClockEpoch, anchor: AnchoredClock) -> Option<(f64, f64)> {
    let first = epoch.first_output_frame.checked_sub(epoch.address_shift)?;
    let last = u64::MAX.checked_sub(epoch.address_shift)?;
    if first < anchor.original_output_frame {
        return None;
    }
    epoch.projection.finite_beats_for_horizon(first, last)
}

fn local_preparation(
    span: MappedSourceSpan,
    anchor: AnchoredClock,
    horizon: (f64, f64),
) -> Option<CompactGeometryPreparation> {
    // These original values are a subset of the whole admitted geometry plus
    // the raw pair/M0/horizon already admitted on its global grids. Therefore
    // local grids are no finer. Coarsening source/beat by S/B divides C/I/P/U
    // by SB and forward N/D by S^2 B: exact modulo, RN and raw identity agree,
    // and every checked intermediate is no larger. This cannot widen global
    // admission: only the already certified private epoch tag reaches here.
    let program = span.legacy_program()?;
    let mut builder = CompactGeometryBuilder {
        source: GridStats::new()
            .include_u64(span.source_start)?
            .include_f64(program.source_period)?
            .include_u64(anchor.frame)?
            .include_f64(anchor.fraction)?,
        beat: GridStats::new()
            .include_f64(program.beat_start)?
            .include_f64(program.quarters)?
            .include_f64(program.beat_start + program.quarters)?
            .include_f64(anchor.original_master)?
            .include_f64(horizon.0)?
            .include_f64(horizon.1)?,
    };
    for segment in [anchor.inverse_segment, span.segment] {
        builder = builder.include_segment(segment, span.loaded_rate_hz)?;
    }
    // The transport's original beat anchor refines precision in core.new. Its
    // magnitude itself is not an admitted M value and is not added to these stats.
    builder.finish()
}

impl<'a> CompactClock<'a> {
    fn new(
        geometry: &'a MappedLoopGeometry,
        epoch: MappedClockEpoch,
    ) -> Result<Self, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        let anchor = epoch.anchor().ok_or(unavailable)?;
        let qualification = geometry.exact_continuation.ok_or(unavailable)?;
        let preparation = qualification.compact.ok_or(unavailable)?;
        if epoch.marker == 0
            || anchor.original_output_frame == u64::MAX
            || !anchor.fraction.is_finite()
            || !(0.0..1.0).contains(&anchor.fraction)
            || !original_segment(geometry, anchor.inverse_segment)
        {
            return Err(unavailable);
        }
        let source_grid = preparation
            .source
            .refine(required_grid_f64(anchor.fraction).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        let source = decode_source_pair_at_grid(anchor.frame, anchor.fraction, source_grid)
            .ok_or(unavailable)?;
        let inverse = anchor.inverse_segment;
        if decode_scaled_f64_at_grid(inverse.source_end, geometry.loaded_rate_hz, source_grid)
            .ok_or(unavailable)?
            == source
            && !geometry
                .map
                .canonical_segment_at_beat(inverse.beat_end)
                .is_some_and(|original| original.matches_exact(inverse))
        {
            return Err(unavailable);
        }
        let core = CompactClockCore::new(
            preparation,
            epoch,
            geometry.beat_start,
            geometry.quarters,
            geometry.loaded_rate_hz,
            geometry.domain.region.start as u64,
            geometry
                .domain
                .musical_period
                .scalar_period()
                .ok_or(unavailable)?,
            finite_horizon(epoch, anchor).ok_or(unavailable)?,
        )?;
        geometry.validate_epoch(epoch)?;
        for segment in [qualification.minimum, qualification.maximum] {
            core.validate_exact_rate(segment)
                .ok_or(MappedClockError::UnsupportedRate)?;
        }
        Ok(Self { geometry, core })
    }

    fn forward_segment(&self, phase: &CompactPhase) -> Option<CanonicalBeatSegment> {
        let end = self.geometry.beat_start + self.geometry.quarters;
        self.geometry
            .map
            .canonical_segment_by_beat_ordering(|beat| {
                if beat < self.geometry.beat_start {
                    return Some(Ordering::Less);
                }
                if beat >= end {
                    return Some(Ordering::Greater);
                }
                Some(self.core.beat_offset(beat)?.cmp(&phase.remainder))
            })
    }

    fn position_at(&self, frame: u64) -> Option<FractionalSourcePosition> {
        let phase = self.core.phase_at(frame)?;
        let segment = self.core.prepared_segment(self.forward_segment(&phase)?)?;
        self.core.position_on_segment(&phase, &segment)
    }
}

impl CompactClockCore {
    #[allow(clippy::too_many_arguments)]
    fn new(
        preparation: CompactGeometryPreparation,
        epoch: MappedClockEpoch,
        beat_start: f64,
        quarters: f64,
        loaded_rate_hz: u32,
        source_start: u64,
        source_period: f64,
        horizon: (f64, f64),
    ) -> Result<Self, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        let anchor = epoch.anchor().ok_or(unavailable)?;
        if epoch.marker == 0
            || anchor.original_output_frame == u64::MAX
            || !anchor.fraction.is_finite()
            || !(0.0..1.0).contains(&anchor.fraction)
        {
            return Err(unavailable);
        }
        let required_beat_grid = master_grid_exponent(
            epoch.projection.frames_per_beat(),
            epoch.projection.original_beat_anchor(),
        )
        .ok_or(unavailable)?
        .min(required_grid_f64(anchor.original_master).ok_or(unavailable)?);
        let beat_grid = preparation
            .beat
            .refine(required_beat_grid)
            .ok_or(unavailable)?;
        let source_grid = preparation
            .source
            .refine(required_grid_f64(anchor.fraction).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        // Horizon endpoints bound magnitude, never the precision of interior M evaluations.
        decode_f64_at_grid(horizon.0, beat_grid).ok_or(unavailable)?;
        decode_f64_at_grid(horizon.1, beat_grid).ok_or(unavailable)?;
        let original_master =
            decode_f64_at_grid(anchor.original_master, beat_grid).ok_or(unavailable)?;
        let start_beat = decode_f64_at_grid(beat_start, beat_grid).ok_or(unavailable)?;
        let quarters = decode_f64_at_grid(quarters, beat_grid).ok_or(unavailable)?;
        if quarters <= 0 {
            return Err(unavailable);
        }
        let source = decode_source_pair_at_grid(anchor.frame, anchor.fraction, source_grid)
            .ok_or(unavailable)?;
        let inverse = anchor.inverse_segment;
        let left_ordering =
            decode_scaled_f64_at_grid(inverse.source_start, loaded_rate_hz, source_grid)
                .ok_or(unavailable)?
                .cmp(&source);
        let right_ordering =
            decode_scaled_f64_at_grid(inverse.source_end, loaded_rate_hz, source_grid)
                .ok_or(unavailable)?
                .cmp(&source);
        if left_ordering == Ordering::Greater || right_ordering == Ordering::Less {
            return Err(unavailable);
        }
        let normalized = normalize_source_coordinate(
            SourceCoordinate {
                whole: u128::from(anchor.frame),
                fraction: anchor.fraction,
            },
            source_grid,
            source_start,
            source_period,
        )
        .ok_or(unavailable)?;
        if normalized.whole != u128::from(anchor.frame)
            || normalized.fraction.to_bits() != anchor.fraction.to_bits()
        {
            return Err(unavailable);
        }
        let mut clock = Self {
            epoch,
            anchor,
            loaded_rate_hz,
            source_start,
            source_period,
            source_grid,
            beat_grid,
            start_beat,
            inverse_delta: 0,
            original_master,
            initial: PhaseInteger::ZERO,
            period: PhaseInteger::ZERO,
        };
        let inverse = clock.segment(anchor.inverse_segment).ok_or(unavailable)?;
        clock.inverse_delta = inverse.source_delta;
        clock.initial = PhaseInteger::from_i128(source)
            .and_then(|v| v.checked_mul_i128(inverse.beat_delta))
            .and_then(|v| v.checked_sub(inverse.constant))
            .ok_or(unavailable)?;
        clock.period = PhaseInteger::from_i128(quarters)
            .and_then(|v| v.checked_mul_i128(clock.inverse_delta))
            .ok_or(unavailable)?;
        if clock.initial < PhaseInteger::ZERO || clock.initial >= clock.period {
            return Err(unavailable);
        }
        Ok(clock)
    }

    fn segment(&self, segment: CanonicalBeatSegment) -> Option<CompactSegment> {
        let source_start =
            decode_scaled_f64_at_grid(segment.source_start, self.loaded_rate_hz, self.source_grid)?;
        let source_end =
            decode_scaled_f64_at_grid(segment.source_end, self.loaded_rate_hz, self.source_grid)?;
        let beat_start = decode_f64_at_grid(segment.beat_start, self.beat_grid)?;
        let beat_end = decode_f64_at_grid(segment.beat_end, self.beat_grid)?;
        let source_delta = source_end.checked_sub(source_start)?;
        let beat_delta = beat_end.checked_sub(beat_start)?;
        if source_delta <= 0 || beat_delta <= 0 {
            return None;
        }
        let constant = PhaseInteger::from_i128(source_start)?
            .checked_mul_i128(beat_delta)?
            .checked_sub(
                PhaseInteger::from_i128(beat_start.checked_sub(self.start_beat)?)?
                    .checked_mul_i128(source_delta)?,
            )?;
        Some(CompactSegment {
            source_delta,
            beat_delta,
            constant,
        })
    }

    fn prepared_segment(&self, segment: CanonicalBeatSegment) -> Option<CompactPreparedSegment> {
        let segment = self.segment(segment)?;
        Some(CompactPreparedSegment {
            source_delta: segment.source_delta,
            numerator_constant: segment
                .constant
                .widen()
                .checked_mul_i128(self.inverse_delta)?,
            denominator: CoordinateInteger::from_i128(self.inverse_delta)?
                .checked_mul_i128(segment.beat_delta)?
                .checked_shl(u32::try_from(-self.source_grid.exponent()).ok()?)?,
        })
    }

    fn validate_exact_rate(&self, segment: CanonicalBeatSegment) -> Option<()> {
        let segment = self.segment(segment)?;
        let fpb = self.epoch.projection.frames_per_beat();
        let (coefficient, exponent) = exact_f64_parts(fpb)?;
        if coefficient <= 0 {
            return None;
        }
        let numerator = CoordinateInteger::from_i128(segment.source_delta)?;
        let denominator =
            CoordinateInteger::from_i128(segment.beat_delta)?.checked_mul_i128(coefficient)?;
        let numerator_exponent = self.source_grid.exponent();
        let denominator_exponent = self.beat_grid.exponent().checked_add(exponent)?;
        // Compare top exponents first. Tiny finite FPB can be valid on a terminal
        // cast plateau; forcing FPB itself onto the [-126,0] grids would narrow it.
        (compare_positive_dyadics(
            numerator,
            numerator_exponent.checked_add(1)?,
            denominator,
            denominator_exponent,
        )? != Ordering::Less
            && compare_positive_dyadics(
                numerator,
                numerator_exponent,
                denominator,
                denominator_exponent.checked_add(1)?,
            )? != Ordering::Greater)
            .then_some(())
    }

    fn unfolded_at(&self, frame: u64) -> Option<PhaseInteger> {
        let master = decode_f64_at_grid(self.epoch.addressed_master_at(frame)?, self.beat_grid)?;
        let delta = master.checked_sub(self.original_master)?;
        self.initial
            .checked_add(PhaseInteger::from_i128(delta)?.checked_mul_i128(self.inverse_delta)?)
    }

    fn phase_at(&self, frame: u64) -> Option<CompactPhase> {
        let unfolded = self.unfolded_at(frame)?;
        Some(CompactPhase {
            unfolded,
            remainder: unfolded.rem_euclid(self.period)?,
        })
    }

    fn beat_offset(&self, beat: f64) -> Option<PhaseInteger> {
        let coefficient = decode_f64_at_grid(beat, self.beat_grid)?;
        PhaseInteger::from_i128(coefficient.checked_sub(self.start_beat)?)?
            .checked_mul_i128(self.inverse_delta)
    }

    fn position_on_segment(
        &self,
        phase: &CompactPhase,
        segment: &CompactPreparedSegment,
    ) -> Option<FractionalSourcePosition> {
        let start = self.source_start;
        let period = self.source_period;
        let coordinate = if phase.remainder == self.initial {
            normalize_source_coordinate(
                SourceCoordinate {
                    whole: u128::from(self.anchor.frame),
                    fraction: self.anchor.fraction,
                },
                self.source_grid,
                start,
                period,
            )?
        } else {
            let numerator = segment.numerator_constant.checked_add(
                phase
                    .remainder
                    .widen()
                    .checked_mul_i128(segment.source_delta)?,
            )?;
            split_normalized_source_coordinate(
                numerator,
                segment.denominator,
                self.source_grid,
                self.anchor.fraction,
                start,
                period,
            )?
        };
        Some(FractionalSourcePosition {
            frame: usize::try_from(coordinate.whole).ok()?,
            fraction: coordinate.fraction,
            seek_mode: ExplicitSeekMode::Normal,
        })
    }
}

fn original_segment(geometry: &MappedLoopGeometry, segment: CanonicalBeatSegment) -> bool {
    geometry
        .map
        .canonical_segment_at_beat(segment.beat_start)
        .is_some_and(|original| original.matches_exact(segment))
}

fn compare_positive_dyadics(
    left: CoordinateInteger,
    left_exponent: i16,
    right: CoordinateInteger,
    right_exponent: i16,
) -> Option<Ordering> {
    if left <= CoordinateInteger::ZERO || right <= CoordinateInteger::ZERO {
        return None;
    }
    let left_top = i32::try_from(left.magnitude_bits()).ok()? + i32::from(left_exponent);
    let right_top = i32::try_from(right.magnitude_bits()).ok()? + i32::from(right_exponent);
    if left_top != right_top {
        return Some(left_top.cmp(&right_top));
    }
    let common = left_exponent.min(right_exponent);
    Some(
        left.checked_shl(u32::try_from(left_exponent - common).ok()?)?
            .cmp(&right.checked_shl(u32::try_from(right_exponent - common).ok()?)?),
    )
}

#[cfg(test)]
#[path = "mapped_source_clock_compact_tests.rs"]
mod tests;
