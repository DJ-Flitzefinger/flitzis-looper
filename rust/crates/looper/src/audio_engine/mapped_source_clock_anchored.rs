//! Certified anchored reference coordinates. This wide path has no productive RT caller.
//!
//! Original owning geometry remains outside copied state. Every wide value below is local
//! sequential scratch; fixed capacity and zero allocation do not qualify stack or throughput.

use super::{
    CanonicalBeatSegment, MappedClockEpoch, MappedClockError, MappedClockMode, MappedLoopGeometry,
    MappedSourceSpan,
};
use crate::audio_engine::source_numerics::{
    SOURCE_GRID_SHIFT, SourceCoordinate, SourceInteger, split_source_coordinate_preserving_fraction,
};
use crate::audio_engine::source_playback::{FractionalSourcePosition, SourcePlayback};
use crate::audio_engine::source_reader::{ExplicitSeekMode, SourceLoopDomain};
use crate::audio_engine::transport::TransportTimeline;

#[derive(Clone, Copy, Debug)]
pub(super) struct AnchoredClock {
    pub(super) frame: u64,
    pub(super) fraction: f64,
    pub(super) original_output_frame: u64,
    pub(super) original_master: f64,
    pub(super) inverse_segment: CanonicalBeatSegment,
}

impl AnchoredClock {
    pub(super) fn matches_exact(self, other: Self) -> bool {
        self.frame == other.frame
            && self.fraction.to_bits() == other.fraction.to_bits()
            && self.original_output_frame == other.original_output_frame
            && self.original_master.to_bits() == other.original_master.to_bits()
            && self.inverse_segment.matches_exact(other.inverse_segment)
    }
}

struct AnchorTerms {
    source_delta: SourceInteger,
    initial: SourceInteger,
    period: SourceInteger,
}

struct Phase {
    terms: AnchorTerms,
    unfolded: SourceInteger,
    remainder: SourceInteger,
}

impl MappedClockEpoch {
    /// Construct immutable reference coordinates at actual E, with the original raw pair.
    /// This off-realtime numerical operation supplies no source/adoption or callback permit.
    pub(crate) fn anchored(
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
        let projection = timeline
            .beat_projection()
            .ok_or(MappedClockError::InvalidEpoch)?;
        let (original_master, _) = projection
            .finite_beats_for_horizon(output_frame, u64::MAX)
            .ok_or(unavailable)?;
        let frame = u64::try_from(position.frame).map_err(|_| unavailable)?;
        let source =
            SourceInteger::from_source_pair_grid(frame, position.fraction).ok_or(unavailable)?;
        let inverse_segment = geometry
            .map
            .canonical_segment_by_source_ordering(|seconds| {
                Some(frame_grid(seconds, geometry.loaded_rate_hz)?.cmp(&source))
            })
            .ok_or(unavailable)?;
        let epoch = Self {
            marker,
            projection,
            address_shift: 0,
            first_output_frame: output_frame,
            mode: MappedClockMode::Anchored(AnchoredClock {
                frame,
                fraction: position.fraction,
                original_output_frame: output_frame,
                original_master,
                inverse_segment,
            }),
        };
        geometry.validate_exact_continuation_epoch(epoch)?;
        anchor_terms(
            epoch,
            geometry.beat_start,
            geometry.quarters,
            geometry.loaded_rate_hz,
        )
        .ok_or(unavailable)?;
        Ok(epoch)
    }

    fn anchored_horizon(self) -> Option<()> {
        self.projection.finite_beats_for_horizon(
            self.first_output_frame.checked_sub(self.address_shift)?,
            u64::MAX.checked_sub(self.address_shift)?,
        )?;
        Some(())
    }
}

impl MappedLoopGeometry {
    pub(super) fn anchored_position_at(
        &self,
        epoch: MappedClockEpoch,
        output_frame: u64,
    ) -> Result<FractionalSourcePosition, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        epoch.anchored_horizon().ok_or(unavailable)?;
        let phase = phase_at(
            epoch,
            self.beat_start,
            self.quarters,
            self.loaded_rate_hz,
            output_frame,
        )
        .ok_or(unavailable)?;
        let segment = forward_segment(self, &phase).ok_or(unavailable)?;
        position_on_segment(
            epoch,
            &phase,
            segment,
            self.beat_start,
            self.loaded_rate_hz,
            self.domain.region.start as u64,
            self.domain.musical_period.ok_or(unavailable)?,
        )
        .ok_or(unavailable)
    }

    pub(super) fn refresh_anchored_span(
        &self,
        epoch: MappedClockEpoch,
        output_frame: u64,
    ) -> Result<MappedSourceSpan, MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        if output_frame == u64::MAX {
            return Err(unavailable);
        }
        self.validate_exact_continuation_epoch(epoch)?;
        epoch.anchored_horizon().ok_or(unavailable)?;
        let phase = phase_at(
            epoch,
            self.beat_start,
            self.quarters,
            self.loaded_rate_hz,
            output_frame,
        )
        .ok_or(unavailable)?;
        let segment = forward_segment(self, &phase).ok_or(unavailable)?;
        let cycle_base = phase
            .unfolded
            .checked_sub(phase.remainder)
            .ok_or(unavailable)?;
        let loop_end = self.beat_start + self.quarters;
        let left = segment.beat_start.max(self.beat_start);
        let right = segment.beat_end.min(loop_end);
        let left_target = cycle_base
            .checked_add(
                beat_offset(left, self.beat_start, phase.terms.source_delta).ok_or(unavailable)?,
            )
            .ok_or(unavailable)?;
        let right_offset = if right == loop_end {
            phase.terms.period
        } else {
            beat_offset(right, self.beat_start, phase.terms.source_delta).ok_or(unavailable)?
        };
        let right_target = cycle_base.checked_add(right_offset).ok_or(unavailable)?;
        if phase.unfolded < left_target || phase.unfolded >= right_target {
            return Err(unavailable);
        }
        // The original admission/horizon/range gates make this predicate total and
        // monotone; the current frame is known true. The MAX probe below certifies
        // the right search's true upper endpoint without changing its terminal branch.
        let start_frame = first_true_near_high(epoch.first_output_frame, output_frame, |frame| {
            Some(unfolded_at(epoch, &phase.terms, frame)? >= left_target)
        })
        .ok_or(unavailable)?;
        let next = output_frame.checked_add(1).ok_or(unavailable)?;
        let end_frame =
            if unfolded_at(epoch, &phase.terms, u64::MAX).ok_or(unavailable)? < right_target {
                u64::MAX
            } else {
                first_true_near_low(next, u64::MAX, |frame| {
                    Some(unfolded_at(epoch, &phase.terms, frame)? >= right_target)
                })
                .ok_or(unavailable)?
            };
        // Evaluate the global endpoint, including any skipped original knots or cycles.
        let endpoint = self.anchored_position_at(epoch, end_frame)?;
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
        span.position_at(output_frame).ok_or(unavailable)?;
        span.local_rate().ok_or(MappedClockError::UnsupportedRate)?;
        Ok(span)
    }
}

pub(super) fn position_in_span(
    span: MappedSourceSpan,
    output_frame: u64,
) -> Option<FractionalSourcePosition> {
    // Extrema and the complete horizon were admitted at span construction; never
    // repeat those wide admission scans on this per-frame reference-query path.
    let phase = phase_at(
        span.epoch,
        span.beat_start,
        span.quarters,
        span.loaded_rate_hz,
        output_frame,
    )?;
    position_on_segment(
        span.epoch,
        &phase,
        span.segment,
        span.beat_start,
        span.loaded_rate_hz,
        span.source_start,
        span.source_period,
    )
}

fn anchor(epoch: MappedClockEpoch) -> Option<AnchoredClock> {
    match epoch.mode {
        MappedClockMode::Anchored(anchor) => Some(anchor),
        MappedClockMode::Legacy { .. } => None,
    }
}

fn frame_grid(seconds: f64, loaded_rate_hz: u32) -> Option<SourceInteger> {
    // Outside search probes need exact arithmetic, not an extra legacy-knot veto.
    SourceInteger::from_f64_grid_scaled(seconds, loaded_rate_hz)
}

fn anchor_terms(
    epoch: MappedClockEpoch,
    beat_start: f64,
    quarters: f64,
    rate: u32,
) -> Option<AnchorTerms> {
    let anchor = anchor(epoch)?;
    let segment = anchor.inverse_segment;
    let x0 = frame_grid(segment.source_start, rate)?;
    let source_delta = frame_grid(segment.source_end, rate)?.checked_sub(x0)?;
    let b0 = SourceInteger::from_f64_grid(segment.beat_start)?;
    let beat_delta = SourceInteger::from_f64_grid(segment.beat_end)?.checked_sub(b0)?;
    if source_delta <= SourceInteger::ZERO || beat_delta <= SourceInteger::ZERO {
        return None;
    }
    let source = SourceInteger::from_source_pair_grid(anchor.frame, anchor.fraction)?;
    let initial = b0
        .checked_sub(SourceInteger::from_f64_grid(beat_start)?)?
        .checked_mul(source_delta)?
        .checked_add(source.checked_sub(x0)?.checked_mul(beat_delta)?)?;
    let period = SourceInteger::from_f64_grid(quarters)?.checked_mul(source_delta)?;
    if initial < SourceInteger::ZERO || initial >= period {
        return None;
    }
    Some(AnchorTerms {
        source_delta,
        initial,
        period,
    })
}

fn unfolded_at(epoch: MappedClockEpoch, terms: &AnchorTerms, frame: u64) -> Option<SourceInteger> {
    let delta = SourceInteger::from_f64_grid(epoch.addressed_master_at(frame)?)?.checked_sub(
        SourceInteger::from_f64_grid(anchor(epoch)?.original_master)?,
    )?;
    terms
        .initial
        .checked_add(delta.checked_mul(terms.source_delta)?)
}

fn phase_at(
    epoch: MappedClockEpoch,
    beat_start: f64,
    quarters: f64,
    rate: u32,
    frame: u64,
) -> Option<Phase> {
    let terms = anchor_terms(epoch, beat_start, quarters, rate)?;
    let unfolded = unfolded_at(epoch, &terms, frame)?;
    let remainder = unfolded.rem_euclid(terms.period)?;
    Some(Phase {
        terms,
        unfolded,
        remainder,
    })
}

fn beat_offset(beat: f64, start: f64, source_delta: SourceInteger) -> Option<SourceInteger> {
    SourceInteger::from_f64_grid(beat)?
        .checked_sub(SourceInteger::from_f64_grid(start)?)?
        .checked_mul(source_delta)
}

fn forward_segment(geometry: &MappedLoopGeometry, phase: &Phase) -> Option<CanonicalBeatSegment> {
    geometry.map.canonical_segment_by_beat_ordering(|beat| {
        Some(
            beat_offset(beat, geometry.beat_start, phase.terms.source_delta)?.cmp(&phase.remainder),
        )
    })
}

fn position_on_segment(
    epoch: MappedClockEpoch,
    phase: &Phase,
    segment: CanonicalBeatSegment,
    beat_start: f64,
    rate: u32,
    source_start: u64,
    source_period: f64,
) -> Option<FractionalSourcePosition> {
    let anchor = anchor(epoch)?;
    let coordinate = if phase.remainder == phase.terms.initial {
        // A generic whole-phase identity, including every revisit and master plateau.
        SourceCoordinate {
            whole: u128::from(anchor.frame),
            fraction: anchor.fraction,
        }
    } else {
        let x0 = frame_grid(segment.source_start, rate)?;
        let dx = frame_grid(segment.source_end, rate)?.checked_sub(x0)?;
        let db = SourceInteger::from_f64_grid(segment.beat_end)?
            .checked_sub(SourceInteger::from_f64_grid(segment.beat_start)?)?;
        let offset = phase.remainder.checked_sub(beat_offset(
            segment.beat_start,
            beat_start,
            phase.terms.source_delta,
        )?)?;
        let numerator = x0
            .checked_mul(phase.terms.source_delta)?
            .checked_mul(db)?
            .checked_add(offset.checked_mul(dx)?)?;
        let denominator = phase
            .terms
            .source_delta
            .checked_mul(db)?
            .checked_shl(SOURCE_GRID_SHIFT)?;
        split_source_coordinate_preserving_fraction(numerator, denominator, anchor.fraction)?
    };
    normalize(coordinate, source_start, source_period)
}

fn normalize(
    coordinate: SourceCoordinate,
    start: u64,
    period: f64,
) -> Option<FractionalSourcePosition> {
    if coordinate.whole > (1_u128 << 64)
        || !coordinate.fraction.is_finite()
        || !(0.0..1.0).contains(&coordinate.fraction)
    {
        return None;
    }
    let start_grid =
        SourceInteger::from_integer(u128::from(start))?.checked_shl(SOURCE_GRID_SHIFT)?;
    let endpoint_grid = start_grid.checked_add(SourceInteger::from_f64_grid(period)?)?;
    let coordinate_grid = SourceInteger::from_integer(coordinate.whole)?
        .checked_shl(SOURCE_GRID_SHIFT)?
        .checked_add(SourceInteger::from_f64_grid(coordinate.fraction)?)?;
    if coordinate_grid == endpoint_grid {
        return Some(FractionalSourcePosition {
            frame: usize::try_from(start).ok()?,
            fraction: 0.0,
            seek_mode: ExplicitSeekMode::Normal,
        });
    }
    if coordinate_grid < start_grid || coordinate_grid >= endpoint_grid {
        return None;
    }
    Some(FractionalSourcePosition {
        frame: usize::try_from(coordinate.whole).ok()?,
        fraction: coordinate.fraction,
        seek_mode: ExplicitSeekMode::Normal,
    })
}

fn first_true(
    mut low: u64,
    mut high: u64,
    mut predicate: impl FnMut(u64) -> Option<bool>,
) -> Option<u64> {
    if low > high {
        return None;
    }
    while low < high {
        let middle = low + (high - low) / 2;
        if predicate(middle)? {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    Some(low)
}

/// Requires a total monotone predicate and a known-true high endpoint.
/// Only the already-admitted anchored refresh supplies this stronger contract.
pub(super) fn first_true_near_high(
    low: u64,
    high: u64,
    mut predicate: impl FnMut(u64) -> Option<bool>,
) -> Option<u64> {
    if low > high {
        return None;
    }
    if low == high {
        return Some(low);
    }
    let predecessor = high.checked_sub(1)?;
    if !predicate(predecessor)? {
        return Some(high);
    }
    first_true(low, predecessor, predicate)
}

/// Requires a total monotone predicate and a known-true high endpoint.
/// Unexpected probe failures propagate; this is not a general fallible-search replacement.
pub(super) fn first_true_near_low(
    low: u64,
    high: u64,
    mut predicate: impl FnMut(u64) -> Option<bool>,
) -> Option<u64> {
    if low > high {
        return None;
    }
    if low == high {
        return Some(low);
    }
    if predicate(low)? {
        return Some(low);
    }
    let second = low.checked_add(1)?;
    if second == high {
        return Some(high);
    }
    if predicate(second)? {
        return Some(second);
    }
    first_true(second.checked_add(1)?, high, predicate)
}

#[cfg(test)]
#[path = "mapped_source_clock_anchored_tests.rs"]
mod tests;
