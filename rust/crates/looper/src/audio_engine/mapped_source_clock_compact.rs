//! Explicit compact reference queries borrowing the same original immutable geometry.
//!
//! Suitability is independent of the original wide qualification. These queries neither fall
//! back to wide arithmetic nor grant productive conversion, source ownership or realtime cost.

use super::anchored::{AnchoredClock, first_true_near_high, first_true_near_low};
use super::{
    CanonicalBeatSegment, MappedClockEpoch, MappedClockError, MappedClockMode, MappedLoopGeometry,
    MappedSourceSpan,
};
use crate::audio_engine::compact_source_numerics::{
    BaseGrid, CoordinateInteger, Grid, GridStats, PhaseInteger, decode_f64_at_grid,
    decode_scaled_f64_at_grid, decode_source_pair_at_grid, exact_f64_parts, master_grid_exponent,
    normalize_source_coordinate, required_grid_f64, split_normalized_source_coordinate,
};
use crate::audio_engine::source_numerics::SourceCoordinate;
use crate::audio_engine::source_playback::FractionalSourcePosition;
use crate::audio_engine::source_reader::ExplicitSeekMode;
use std::cmp::Ordering;

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
                .include_f64(geometry.domain.musical_period?)?,
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
    epoch: MappedClockEpoch,
    anchor: AnchoredClock,
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
        let unavailable = MappedClockError::NumericalUnavailable;
        if output_frame == u64::MAX {
            return Err(unavailable);
        }
        let clock = CompactClock::new(self, epoch)?;
        let phase = clock.phase_at(output_frame).ok_or(unavailable)?;
        let segment = clock.forward_segment(&phase).ok_or(unavailable)?;
        let cycle_base = phase
            .unfolded
            .checked_sub(phase.remainder)
            .ok_or(unavailable)?;
        let loop_end = self.beat_start + self.quarters;
        let left = segment.beat_start.max(self.beat_start);
        let right = segment.beat_end.min(loop_end);
        let left_target = cycle_base
            .checked_add(clock.beat_offset(left).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        let right_offset = if right == loop_end {
            clock.period
        } else {
            clock.beat_offset(right).ok_or(unavailable)?
        };
        let right_target = cycle_base.checked_add(right_offset).ok_or(unavailable)?;
        if phase.unfolded < left_target || phase.unfolded >= right_target {
            return Err(unavailable);
        }
        let start_frame = first_true_near_high(epoch.first_output_frame, output_frame, |frame| {
            Some(clock.unfolded_at(frame)? >= left_target)
        })
        .ok_or(unavailable)?;
        let next = output_frame.checked_add(1).ok_or(unavailable)?;
        let end_frame = if clock.unfolded_at(u64::MAX).ok_or(unavailable)? < right_target {
            u64::MAX
        } else {
            first_true_near_low(next, u64::MAX, |frame| {
                Some(clock.unfolded_at(frame)? >= right_target)
            })
            .ok_or(unavailable)?
        };
        let endpoint = clock.position_at(end_frame).ok_or(unavailable)?;
        let span = MappedSourceSpan {
            epoch,
            segment,
            beat_start: self.beat_start,
            quarters: self.quarters,
            loaded_rate_hz: self.loaded_rate_hz,
            start_frame,
            end_frame,
            endpoint_frame: endpoint.frame,
            endpoint_fraction: endpoint.fraction,
            source_start: self.domain.region.start as u64,
            source_period: self.domain.musical_period.ok_or(unavailable)?,
        };
        let prepared = clock.prepared_segment(segment).ok_or(unavailable)?;
        clock
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
        if self.beat_start.to_bits() != geometry.beat_start.to_bits()
            || self.quarters.to_bits() != geometry.quarters.to_bits()
            || self.loaded_rate_hz != geometry.loaded_rate_hz
            || self.source_start != geometry.domain.region.start as u64
            || geometry.domain.musical_period.map(f64::to_bits)
                != Some(self.source_period.to_bits())
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
        let segment = clock.prepared_segment(self.segment).ok_or(unavailable)?;
        self.local_rate().ok_or(MappedClockError::UnsupportedRate)?;
        Ok(CompactSpanView {
            span: self,
            clock,
            segment,
        })
    }
}

impl CompactSpanView<'_> {
    pub(crate) fn position_at(&self, output_frame: u64) -> Option<FractionalSourcePosition> {
        if output_frame == self.span.end_frame {
            return Some(FractionalSourcePosition {
                frame: self.span.endpoint_frame,
                fraction: self.span.endpoint_fraction,
                seek_mode: ExplicitSeekMode::Normal,
            });
        }
        if !self.span.contains(output_frame) {
            return None;
        }
        let phase = self.clock.phase_at(output_frame)?;
        // Retain exact original knot/wrap classification even for a borrowed span.
        // The span's interval was frozen independently of all callback partitions.
        self.clock.position_on_segment(&phase, &self.segment)
    }
}

impl<'a> CompactClock<'a> {
    fn new(
        geometry: &'a MappedLoopGeometry,
        epoch: MappedClockEpoch,
    ) -> Result<Self, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        let MappedClockMode::Anchored(anchor) = epoch.mode else {
            return Err(unavailable);
        };
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
        let first = epoch
            .first_output_frame
            .checked_sub(epoch.address_shift)
            .ok_or(unavailable)?;
        let last = u64::MAX
            .checked_sub(epoch.address_shift)
            .ok_or(unavailable)?;
        if first < anchor.original_output_frame {
            return Err(unavailable);
        }
        let (first_master, last_master) = epoch
            .projection
            .finite_beats_for_horizon(first, last)
            .ok_or(unavailable)?;
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
        decode_f64_at_grid(first_master, beat_grid).ok_or(unavailable)?;
        decode_f64_at_grid(last_master, beat_grid).ok_or(unavailable)?;
        let original_master =
            decode_f64_at_grid(anchor.original_master, beat_grid).ok_or(unavailable)?;
        let start_beat = decode_f64_at_grid(geometry.beat_start, beat_grid).ok_or(unavailable)?;
        let quarters = decode_f64_at_grid(geometry.quarters, beat_grid).ok_or(unavailable)?;
        if quarters <= 0 {
            return Err(unavailable);
        }
        let source = decode_source_pair_at_grid(anchor.frame, anchor.fraction, source_grid)
            .ok_or(unavailable)?;
        let inverse = anchor.inverse_segment;
        let left_ordering =
            decode_scaled_f64_at_grid(inverse.source_start, geometry.loaded_rate_hz, source_grid)
                .ok_or(unavailable)?
                .cmp(&source);
        let right_ordering =
            decode_scaled_f64_at_grid(inverse.source_end, geometry.loaded_rate_hz, source_grid)
                .ok_or(unavailable)?
                .cmp(&source);
        if left_ordering == Ordering::Greater
            || right_ordering == Ordering::Less
            || (right_ordering == Ordering::Equal
                && !geometry
                    .map
                    .canonical_segment_at_beat(inverse.beat_end)
                    .is_some_and(|original| original.matches_exact(inverse)))
        {
            return Err(unavailable);
        }
        let normalized = normalize_source_coordinate(
            SourceCoordinate {
                whole: u128::from(anchor.frame),
                fraction: anchor.fraction,
            },
            source_grid,
            geometry.domain.region.start as u64,
            geometry.domain.musical_period.ok_or(unavailable)?,
        )
        .ok_or(unavailable)?;
        if normalized.whole != u128::from(anchor.frame)
            || normalized.fraction.to_bits() != anchor.fraction.to_bits()
        {
            return Err(unavailable);
        }
        let mut clock = Self {
            geometry,
            epoch,
            anchor,
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
        geometry.validate_epoch(epoch)?;
        for segment in [qualification.minimum, qualification.maximum] {
            clock
                .validate_exact_rate(segment)
                .ok_or(MappedClockError::UnsupportedRate)?;
        }
        Ok(clock)
    }

    fn segment(&self, segment: CanonicalBeatSegment) -> Option<CompactSegment> {
        let source_start = decode_scaled_f64_at_grid(
            segment.source_start,
            self.geometry.loaded_rate_hz,
            self.source_grid,
        )?;
        let source_end = decode_scaled_f64_at_grid(
            segment.source_end,
            self.geometry.loaded_rate_hz,
            self.source_grid,
        )?;
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
                Some(self.beat_offset(beat)?.cmp(&phase.remainder))
            })
    }

    fn position_at(&self, frame: u64) -> Option<FractionalSourcePosition> {
        let phase = self.phase_at(frame)?;
        let segment = self.prepared_segment(self.forward_segment(&phase)?)?;
        self.position_on_segment(&phase, &segment)
    }

    fn position_on_segment(
        &self,
        phase: &CompactPhase,
        segment: &CompactPreparedSegment,
    ) -> Option<FractionalSourcePosition> {
        let start = self.geometry.domain.region.start as u64;
        let period = self.geometry.domain.musical_period?;
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
