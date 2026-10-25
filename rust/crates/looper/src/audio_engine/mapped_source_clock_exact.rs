//! Opt-in off-realtime proof for the original geometry/reader intersection.
//!
//! This does not implement an anchored clock or grant source, history, adoption or callback
//! permission. Wide arithmetic and any last-owner destruction stay outside realtime.

use super::compact::{CompactGeometryBuilder, CompactGeometryPreparation};
use super::{CanonicalBeatSegment, MappedClockEpoch, MappedClockError, MappedLoopGeometry};
use crate::audio_engine::source_numerics::{SOURCE_GRID_SHIFT, SourceInteger};
use crate::audio_engine::source_playback::{FractionalSourcePosition, SourcePlayback};
use crate::audio_engine::source_reader::{ExplicitSeekMode, SourceLoopDomain};

/// Original segments attaining exact source-frames-per-quarter extrema.
/// This immutable owner-side proof never enters a copied playback span or lane.
#[derive(Clone, Copy, Debug)]
pub(super) struct ExactContinuationQualification {
    pub(super) first: CanonicalBeatSegment,
    pub(super) minimum: CanonicalBeatSegment,
    pub(super) maximum: CanonicalBeatSegment,
    pub(super) compact: Option<CompactGeometryPreparation>,
}

#[derive(Clone, Copy)]
struct ExactSlope {
    source_delta: SourceInteger,
    beat_delta: SourceInteger,
}

impl MappedLoopGeometry {
    /// Consume the same original immutable geometry and qualify only its retained loop.
    ///
    /// This explicit off-realtime scan retains every original field and map owner. Failure
    /// concerns the additional exact intersection, not validity of the legacy geometry.
    pub(crate) fn qualify_exact_continuation(mut self) -> Result<Self, MappedClockError> {
        if self.exact_continuation.is_some() {
            return Ok(self);
        }
        let unavailable = MappedClockError::NumericalUnavailable;
        let start = FractionalSourcePosition {
            frame: self.domain.region.start,
            fraction: 0.0,
            seek_mode: ExplicitSeekMode::Normal,
        };
        // Reuse the delivered exact P, |P-L| and canonical-domain guard. The temporary
        // coordinate value grants no source, reader or historical permission.
        SourcePlayback::scalar_musical_continuation(start, self.domain, 1.0, 1.0)
            .ok_or(unavailable)?;
        let period = self
            .domain
            .musical_period
            .scalar_period()
            .ok_or(unavailable)?;
        let end_beat = self.beat_start + self.quarters;
        let exact_end_beat = SourceInteger::from_f64_grid(self.beat_start)
            .and_then(|start| start.checked_add(SourceInteger::from_f64_grid(self.quarters)?))
            .ok_or(unavailable)?;
        if Some(exact_end_beat) != SourceInteger::from_f64_grid(end_beat) {
            return Err(unavailable);
        }

        let mut first = None;
        let mut last = None;
        let mut minimum: Option<(CanonicalBeatSegment, ExactSlope)> = None;
        let mut maximum: Option<(CanonicalBeatSegment, ExactSlope)> = None;
        // Compact suitability is optional metadata. Its failure must never narrow
        // the original independently admitted wide geometry or its refusals.
        let mut compact = CompactGeometryBuilder::new(&self);
        self.map
            .visit_canonical_segments(self.beat_start, end_beat, |segment| {
                let slope = exact_slope(segment, self.loaded_rate_hz)?;
                compact = compact
                    .and_then(|builder| builder.include_segment(segment, self.loaded_rate_hz));
                if first.is_none() {
                    first = Some(segment);
                }
                last = Some(segment);
                if let Some((_, previous)) = minimum {
                    if slope_less(slope, previous)? {
                        minimum = Some((segment, slope));
                    }
                } else {
                    minimum = Some((segment, slope));
                }
                if let Some((_, previous)) = maximum {
                    if slope_less(previous, slope)? {
                        maximum = Some((segment, slope));
                    }
                } else {
                    maximum = Some((segment, slope));
                }
                Some(())
            })
            .ok_or(unavailable)?;

        let source_start = SourceInteger::from_integer(self.domain.region.start as u128)
            .and_then(|value| value.checked_shl(SOURCE_GRID_SHIFT))
            .ok_or(unavailable)?;
        let source_end = source_start
            .checked_add(SourceInteger::from_f64_grid(period).ok_or(unavailable)?)
            .ok_or(unavailable)?;
        exact_endpoint(
            first.ok_or(unavailable)?,
            self.loaded_rate_hz,
            self.beat_start,
            source_start,
        )
        .ok_or(unavailable)?;
        exact_endpoint(
            last.ok_or(unavailable)?,
            self.loaded_rate_hz,
            end_beat,
            source_end,
        )
        .ok_or(unavailable)?;
        self.exact_continuation = Some(ExactContinuationQualification {
            first: first.ok_or(unavailable)?,
            minimum: minimum.ok_or(unavailable)?.0,
            maximum: maximum.ok_or(unavailable)?.0,
            compact: compact.and_then(CompactGeometryBuilder::finish),
        });
        Ok(self)
    }

    /// Cheap numeric compatibility only: full original domain equality plus the proof.
    /// Source, accepted timing, ACK and retained-map custody remain separate requirements.
    pub(crate) fn exact_continuation_matches_domain(&self, original: SourceLoopDomain) -> bool {
        self.exact_continuation.is_some() && self.domain == original
    }

    /// Check exact original slope extrema against the actual frozen FPB, off realtime.
    /// This does not certify the finite M(n) horizon or actual callback cost.
    pub(crate) fn validate_exact_continuation_epoch(
        &self,
        epoch: MappedClockEpoch,
    ) -> Result<(), MappedClockError> {
        let unavailable = MappedClockError::NumericalUnavailable;
        let qualification = self.exact_continuation.ok_or(unavailable)?;
        // Keep the established rounded derivative admission as well as these exact
        // bounds; a finite abstract ratio cannot excuse its intermediate overflow.
        self.validate_epoch(epoch)?;
        let fpb = epoch.projection.frames_per_beat();
        if !fpb.is_finite() || fpb <= 0.0 {
            return Err(MappedClockError::InvalidEpoch);
        }
        let fpb = SourceInteger::from_f64_grid(fpb).ok_or(unavailable)?;
        for segment in [qualification.minimum, qualification.maximum] {
            let slope = exact_slope(segment, self.loaded_rate_hz).ok_or(unavailable)?;
            let numerator = slope
                .source_delta
                .checked_shl(SOURCE_GRID_SHIFT)
                .ok_or(unavailable)?;
            let denominator = slope.beat_delta.checked_mul(fpb).ok_or(unavailable)?;
            if numerator.checked_shl(1).ok_or(unavailable)? < denominator
                || numerator > denominator.checked_shl(1).ok_or(unavailable)?
            {
                return Err(MappedClockError::UnsupportedRate);
            }
        }
        Ok(())
    }
}

fn original_frame_grid(seconds: f64, loaded_rate_hz: u32) -> Option<SourceInteger> {
    let exact = SourceInteger::from_f64_grid_scaled(seconds, loaded_rate_hz)?;
    let rounded = SourceInteger::from_f64_grid(seconds * f64::from(loaded_rate_hz))?;
    (exact == rounded).then_some(exact)
}

fn exact_slope(segment: CanonicalBeatSegment, loaded_rate_hz: u32) -> Option<ExactSlope> {
    let source_start = original_frame_grid(segment.source_start, loaded_rate_hz)?;
    let source_end = original_frame_grid(segment.source_end, loaded_rate_hz)?;
    let source_delta = source_end.checked_sub(source_start)?;
    let beat_delta = SourceInteger::from_f64_grid(segment.beat_end)?
        .checked_sub(SourceInteger::from_f64_grid(segment.beat_start)?)?;
    (source_delta > SourceInteger::ZERO && beat_delta > SourceInteger::ZERO).then_some(ExactSlope {
        source_delta,
        beat_delta,
    })
}

fn slope_less(left: ExactSlope, right: ExactSlope) -> Option<bool> {
    Some(
        left.source_delta.checked_mul(right.beat_delta)?
            < right.source_delta.checked_mul(left.beat_delta)?,
    )
}

fn exact_endpoint(
    segment: CanonicalBeatSegment,
    loaded_rate_hz: u32,
    target_beat: f64,
    expected_source: SourceInteger,
) -> Option<()> {
    let source_start = original_frame_grid(segment.source_start, loaded_rate_hz)?;
    let slope = exact_slope(segment, loaded_rate_hz)?;
    let beat_delta = SourceInteger::from_f64_grid(target_beat)?
        .checked_sub(SourceInteger::from_f64_grid(segment.beat_start)?)?;
    let left = expected_source
        .checked_sub(source_start)?
        .checked_mul(slope.beat_delta)?;
    let right = beat_delta.checked_mul(slope.source_delta)?;
    (left == right).then_some(())
}

#[cfg(test)]
#[path = "mapped_source_clock_exact_tests.rs"]
mod tests;
