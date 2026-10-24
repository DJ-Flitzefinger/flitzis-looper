//! Immutable mapped loop geometry and fixed copied source addressing.
//!
//! Geometry is deliberately not an audio permit. The productive owner separately
//! retains its original qualified source/map pins and validates their lifetime.
//! Construction and explicit off-realtime qualification scan intervals. Frame queries and
//! refreshes borrow the original map without cloning or destroying an Arc on realtime.

use super::constants::{SPEED_MAX, SPEED_MIN};
use super::source_beatmap::{CanonicalBeatSegment, SourceBeatmap};
use super::source_playback::FractionalSourcePosition;
use super::source_reader::{ExplicitSeekMode, FrameRange, SourceLoopDomain};
use super::transport::{TransportBeatProjection, TransportTimeline};

#[path = "mapped_source_clock_anchored.rs"]
mod anchored;
#[path = "mapped_source_clock_compact.rs"]
mod compact;
#[path = "mapped_source_clock_exact.rs"]
mod exact;
#[path = "mapped_source_clock_general.rs"]
mod general;
pub(crate) use general::GeneralLocalSpanView;

use anchored::AnchoredClock;
#[cfg(test)]
pub(crate) use compact::observe_compact_queries_for_test;
pub(crate) use compact::{CompactLocalSpanView, CompactSpanView};
use exact::ExactContinuationQualification;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MappedClockError {
    InvalidGeometry,
    UnsupportedCoverage,
    UnsupportedRate,
    InvalidEpoch,
    NumericalUnavailable,
}

impl std::fmt::Display for MappedClockError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidGeometry => "mapped_clock_invalid_geometry",
            Self::UnsupportedCoverage => "mapped_clock_unsupported_coverage",
            Self::UnsupportedRate => "mapped_clock_unsupported_rate",
            Self::InvalidEpoch => "mapped_clock_invalid_epoch",
            Self::NumericalUnavailable => "mapped_clock_numerical_unavailable",
        })
    }
}

impl std::error::Error for MappedClockError {}

/// Owning immutable geometry. Construct and retire through off-realtime owners.
#[derive(Clone, Debug)]
pub(crate) struct MappedLoopGeometry {
    map: SourceBeatmap,
    domain: SourceLoopDomain,
    beat_start: f64,
    quarters: f64,
    source_end: f64,
    loaded_rate_hz: u32,
    min_seconds_per_quarter: f64,
    max_seconds_per_quarter: f64,
    exact_continuation: Option<ExactContinuationQualification>,
    general: Option<general::GeneralPreparation>,
}

/// Frozen actual transport epoch and caller-supplied non-owning identity marker.
/// An identity marker alone grants no admission, history or publication rights.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MappedClockEpoch {
    marker: u64,
    projection: TransportBeatProjection,
    address_shift: u64,
    first_output_frame: u64,
    mode: MappedClockMode,
}

#[derive(Clone, Copy, Debug)]
enum MappedClockMode {
    Legacy {
        phase_quarters: f64,
        continuation_anchor: Option<ContinuationAnchor>,
    },
    Anchored(AnchoredClock),
    // Same immutable anchor payload. Only the globally qualified compact constructor
    // creates this tag; copied spans may then prepare bounded local arithmetic.
    CompactAnchored(AnchoredClock),
    GeneralCompact(AnchoredClock),
}

#[derive(Clone, Copy, Debug)]
struct ContinuationAnchor {
    total_quarters: f64,
    master_quarters: f64,
}

impl MappedClockEpoch {
    pub(crate) fn new(
        marker: u64,
        timeline: TransportTimeline,
        phase_quarters: f64,
    ) -> Option<Self> {
        let projection = timeline.beat_projection()?;
        let frames_per_beat = projection.frames_per_beat();
        if marker == 0
            || !phase_quarters.is_finite()
            || !frames_per_beat.is_finite()
            || frames_per_beat <= 0.0
        {
            return None;
        }
        Some(Self {
            marker,
            projection,
            address_shift: 0,
            first_output_frame: 0,
            mode: MappedClockMode::Legacy {
                phase_quarters,
                continuation_anchor: None,
            },
        })
    }

    pub(crate) fn master_period_seconds(self) -> Option<f64> {
        Some(self.projection.master_period_seconds())
    }

    pub(crate) fn matches_timeline(self, timeline: &TransportTimeline) -> bool {
        self.projection.matches_timeline(timeline)
    }

    /// Reuse this voice's marker and creative phase with the same authoritative transport.
    /// The group claimant separately proves M(E), canonical position and original map ownership.
    pub(crate) fn with_timeline(self, timeline: TransportTimeline) -> Option<Self> {
        let MappedClockMode::Legacy {
            phase_quarters,
            continuation_anchor,
        } = self.mode
        else {
            return self.matches_timeline(&timeline).then_some(self);
        };
        if self.address_shift != 0 || continuation_anchor.is_some() {
            return None;
        }
        Self::new(self.marker, timeline, phase_quarters)
    }

    /// The all-dry claimant alone changes the actual master epoch. A paused-derived
    /// projection anchors its exact old total at E rather than extrapolating the
    /// new master slope backward through a previous pause's address translation.
    /// Anchored coordinates instead require addressed M-bit continuity and keep
    /// their original raw source anchor and cached M0; geometry admission is separate.
    pub(crate) fn with_timeline_at(self, timeline: TransportTimeline, frame: u64) -> Option<Self> {
        if self.projection.matches_timeline(&timeline) {
            return Some(self);
        }
        if self.is_anchored() {
            if frame < self.first_output_frame {
                return None;
            }
            let addressed = frame.checked_sub(self.address_shift)?;
            let last = u64::MAX.checked_sub(self.address_shift)?;
            let projection = timeline.beat_projection()?;
            let old = self.projection.beat_position_at_frame(addressed);
            let (new, _) = projection.finite_beats_for_horizon(addressed, last)?;
            if !old.is_finite() || old.to_bits() != new.to_bits() {
                return None;
            }
            return Some(Self {
                projection,
                first_output_frame: frame,
                ..self
            });
        }
        let MappedClockMode::Legacy {
            continuation_anchor,
            ..
        } = self.mode
        else {
            return None;
        };
        if self.address_shift == 0 && continuation_anchor.is_none() {
            return self.with_timeline(timeline);
        }
        let total_quarters = self.total_at(frame)?;
        let master_quarters = timeline.beat_position_at_frame(frame)?;
        if !master_quarters.is_finite() {
            return None;
        }
        let mut replacement = Self::new(self.marker, timeline, 0.0)?;
        replacement.first_output_frame = frame;
        replacement.mode = MappedClockMode::Legacy {
            phase_quarters: 0.0,
            continuation_anchor: Some(ContinuationAnchor {
                total_quarters,
                master_quarters,
            }),
        };
        Some(replacement)
    }

    /// Copy one voice's creative source phase; the authoritative transport does
    /// not change. At R, querying R-(R-P) repeats precisely the old evaluation P.
    pub(crate) fn resumed_at(
        self,
        actual: &TransportTimeline,
        paused: u64,
        resumed: u64,
    ) -> Option<Self> {
        if !self.matches_timeline(actual) || paused < self.first_output_frame {
            return None;
        }
        let duration = resumed.checked_sub(paused)?;
        if self.is_anchored() {
            self.addressed_master_at(paused)?;
        } else {
            self.total_at(paused)?;
        }
        let mut replacement = self;
        replacement.address_shift = self.address_shift.checked_add(duration)?;
        replacement.first_output_frame = self.first_output_frame.checked_add(duration)?;
        Some(replacement)
    }

    pub(crate) fn matches_exact(self, other: Self) -> bool {
        self.marker == other.marker
            && self.address_shift == other.address_shift
            && self.first_output_frame == other.first_output_frame
            && match (self.mode, other.mode) {
                (MappedClockMode::GeneralCompact(left), MappedClockMode::GeneralCompact(right))
                | (MappedClockMode::Anchored(left), MappedClockMode::Anchored(right))
                | (
                    MappedClockMode::CompactAnchored(left),
                    MappedClockMode::CompactAnchored(right),
                ) => left.matches_exact(right),
                (
                    MappedClockMode::Legacy {
                        phase_quarters: left_phase,
                        continuation_anchor: left,
                    },
                    MappedClockMode::Legacy {
                        phase_quarters: right_phase,
                        continuation_anchor: right,
                    },
                ) => {
                    left_phase.to_bits() == right_phase.to_bits()
                        && match (left, right) {
                            (None, None) => true,
                            (Some(left), Some(right)) => {
                                left.total_quarters.to_bits() == right.total_quarters.to_bits()
                                    && left.master_quarters.to_bits()
                                        == right.master_quarters.to_bits()
                            }
                            _ => false,
                        }
                }
                _ => false,
            }
            && self.projection.matches_exact(other.projection)
    }

    fn total_at(self, output_frame: u64) -> Option<f64> {
        let MappedClockMode::Legacy {
            phase_quarters,
            continuation_anchor,
        } = self.mode
        else {
            return None;
        };
        if output_frame < self.first_output_frame {
            return None;
        }
        let addressed = output_frame.checked_sub(self.address_shift)?;
        let master = self.projection.beat_position_at_frame(addressed);
        let total = continuation_anchor.map_or_else(
            || master + phase_quarters,
            |anchor| anchor.total_quarters + (master - anchor.master_quarters),
        );
        total.is_finite().then_some(total)
    }

    pub(crate) fn is_anchored(self) -> bool {
        self.anchor().is_some()
    }

    pub(crate) fn is_general_compact(self) -> bool {
        matches!(self.mode, MappedClockMode::GeneralCompact(_))
    }

    pub(crate) fn is_compact_anchored(self) -> bool {
        matches!(self.mode, MappedClockMode::CompactAnchored(_))
    }

    fn anchor(self) -> Option<AnchoredClock> {
        match self.mode {
            MappedClockMode::Anchored(anchor)
            | MappedClockMode::CompactAnchored(anchor)
            | MappedClockMode::GeneralCompact(anchor) => Some(anchor),
            MappedClockMode::Legacy { .. } => None,
        }
    }

    /// A changed projection cannot replace the immutable original anchor or mode.
    pub(crate) fn replacement_anchor_matches(self, other: Self, frame: u64) -> bool {
        match (self.mode, other.mode) {
            (MappedClockMode::Legacy { .. }, MappedClockMode::Legacy { .. }) => true,
            (MappedClockMode::GeneralCompact(left), MappedClockMode::GeneralCompact(right))
            | (MappedClockMode::Anchored(left), MappedClockMode::Anchored(right))
            | (MappedClockMode::CompactAnchored(left), MappedClockMode::CompactAnchored(right)) => {
                self.marker == other.marker
                    && self.address_shift == other.address_shift
                    && left.matches_exact(right)
                    && other.first_output_frame >= self.first_output_frame
                    && (self.projection.matches_exact(other.projection)
                        || other.first_output_frame == frame)
                    && match (
                        self.addressed_master_at(frame),
                        other.addressed_master_at(frame),
                    ) {
                        (Some(left), Some(right)) => left.to_bits() == right.to_bits(),
                        _ => false,
                    }
            }
            _ => false,
        }
    }

    fn addressed_master_at(self, output_frame: u64) -> Option<f64> {
        if output_frame < self.first_output_frame {
            return None;
        }
        let addressed = output_frame.checked_sub(self.address_shift)?;
        let master = self.projection.beat_position_at_frame(addressed);
        master.is_finite().then_some(master)
    }

    fn beat_at(self, output_frame: u64, start: f64, quarters: f64) -> Option<f64> {
        let beat = start + self.total_at(output_frame)?.rem_euclid(quarters);
        beat.is_finite().then_some(beat)
    }
}

/// Original affine anchors and canonical exclusive endpoint for one geometric
/// span. Absolute bounds never depend on a callback or preparation chunk size.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MappedSourceSpan {
    epoch: MappedClockEpoch,
    segment: CanonicalBeatSegment,
    program: SpanProgram,
    loaded_rate_hz: u32,
    start_frame: u64,
    end_frame: u64,
    endpoint_frame: usize,
    endpoint_fraction: f64,
    source_start: u64,
}

#[derive(Clone, Copy, Debug)]
struct LegacySpanProgram {
    beat_start: f64,
    quarters: f64,
    source_period: f64,
}
#[derive(Clone, Copy, Debug)]
enum SpanProgram {
    Legacy(LegacySpanProgram),
    General {
        cycle_coefficient: [u64; 2],
        cycle_exponent: i16,
    },
}
impl SpanProgram {
    fn matches_exact(self, other: Self) -> bool {
        match (self, other) {
            (Self::Legacy(a), Self::Legacy(b)) => {
                a.beat_start.to_bits() == b.beat_start.to_bits()
                    && a.quarters.to_bits() == b.quarters.to_bits()
                    && a.source_period.to_bits() == b.source_period.to_bits()
            }
            (
                Self::General {
                    cycle_coefficient: a,
                    cycle_exponent: x,
                },
                Self::General {
                    cycle_coefficient: b,
                    cycle_exponent: y,
                },
            ) => a == b && x == y,
            _ => false,
        }
    }
}

impl MappedSourceSpan {
    fn legacy_program(self) -> Option<LegacySpanProgram> {
        if let SpanProgram::Legacy(p) = self.program {
            Some(p)
        } else {
            None
        }
    }
    pub(crate) fn epoch(self) -> MappedClockEpoch {
        self.epoch
    }
    pub(crate) fn start_frame(self) -> u64 {
        self.start_frame
    }
    pub(crate) fn end_frame(self) -> u64 {
        self.end_frame
    }

    pub(crate) fn contains(self, output_frame: u64) -> bool {
        self.start_frame <= output_frame && output_frame < self.end_frame
    }

    /// Evaluates M(n), modulo and S from their original global anchors. The
    /// endpoint may be beyond several intervening knots and is never extrapolated.
    pub(crate) fn position_at(self, output_frame: u64) -> Option<FractionalSourcePosition> {
        if self.epoch.is_general_compact() {
            return None;
        }
        if output_frame == self.end_frame {
            return Some(FractionalSourcePosition {
                frame: self.endpoint_frame,
                fraction: self.endpoint_fraction,
                seek_mode: ExplicitSeekMode::Normal,
            });
        }
        if !self.contains(output_frame) {
            return None;
        }
        if self.epoch.is_compact_anchored() {
            return self.local_compact_view().ok()?.position_at(output_frame);
        }
        if self.epoch.is_anchored() {
            return anchored::position_in_span(self, output_frame);
        }
        let program = self.legacy_program()?;
        let beat = self
            .epoch
            .beat_at(output_frame, program.beat_start, program.quarters)?;
        position_from_source(self.segment.source_at_beat(beat)?, self.loaded_rate_hz)
    }

    /// Derivative of the same canonical mapping, excluding wrap discontinuity.
    pub(crate) fn local_rate(self) -> Option<f64> {
        let rate = f64::from(self.loaded_rate_hz) * self.segment.seconds_per_quarter()
            / self.epoch.projection.frames_per_beat();
        valid_rate(rate).then_some(rate)
    }

    pub(crate) fn matches_exact(self, other: Self) -> bool {
        self.epoch.matches_exact(other.epoch)
            && self.segment.matches_exact(other.segment)
            && self.program.matches_exact(other.program)
            && self.loaded_rate_hz == other.loaded_rate_hz
            && self.start_frame == other.start_frame
            && self.end_frame == other.end_frame
            && self.endpoint_frame == other.endpoint_frame
            && self.endpoint_fraction.to_bits() == other.endpoint_fraction.to_bits()
            && self.source_start == other.source_start
    }
}

impl MappedLoopGeometry {
    /// Physical start is the actual loaded integer marker. The exact quarter
    /// extent is supplied by the intent resolver, never inferred from rounded end.
    pub(crate) fn new(
        map: SourceBeatmap,
        sample_frames: usize,
        loaded_rate_hz: u32,
        region: FrameRange,
        exact_quarters: f64,
        require_four_four: bool,
    ) -> Result<Self, MappedClockError> {
        if loaded_rate_hz == 0
            || sample_frames == 0
            || region.start >= region.end
            || region.end > sample_frames
            || !exact_quarters.is_finite()
            || exact_quarters <= 0.0
        {
            return Err(MappedClockError::InvalidGeometry);
        }
        let interval = map
            .canonical_loop_interval(
                region.start as f64 / f64::from(loaded_rate_hz),
                exact_quarters,
                require_four_four,
            )
            .ok_or(MappedClockError::UnsupportedCoverage)?;
        let physical_end = interval.source_end * f64::from(loaded_rate_hz);
        if !physical_end.is_finite() || physical_end.round() != region.end as f64 {
            return Err(MappedClockError::InvalidGeometry);
        }
        let domain =
            SourceLoopDomain::musical(sample_frames, region, physical_end - region.start as f64)
                .ok_or(MappedClockError::InvalidGeometry)?;
        Ok(Self {
            map,
            domain,
            beat_start: interval.beat_start,
            quarters: exact_quarters,
            source_end: interval.source_end,
            loaded_rate_hz,
            min_seconds_per_quarter: interval.min_seconds_per_quarter,
            max_seconds_per_quarter: interval.max_seconds_per_quarter,
            exact_continuation: None,
            general: None,
        })
    }

    pub(crate) fn map(&self) -> &SourceBeatmap {
        &self.map
    }
    pub(crate) fn domain(&self) -> SourceLoopDomain {
        self.domain
    }
    /// Static optional preparation only; raw anchor and actual master admission
    /// remain separate checks at the real execution frame.
    pub(crate) fn has_compact_continuation(&self) -> bool {
        self.exact_continuation
            .is_some_and(|qualification| qualification.compact.is_some())
    }
    pub(crate) fn exact_quarters(&self) -> f64 {
        self.quarters
    }
    pub(crate) fn unrounded_source_end(&self) -> f64 {
        self.source_end
    }
    pub(crate) fn start_beat(&self) -> f64 {
        self.beat_start
    }

    /// O(1) admission using extrema scanned during non-realtime construction.
    pub(crate) fn validate_epoch(&self, epoch: MappedClockEpoch) -> Result<(), MappedClockError> {
        let frames_per_beat = epoch.projection.frames_per_beat();
        if !frames_per_beat.is_finite() || frames_per_beat <= 0.0 {
            return Err(MappedClockError::InvalidEpoch);
        }
        for slope in [self.min_seconds_per_quarter, self.max_seconds_per_quarter] {
            if !valid_rate(f64::from(self.loaded_rate_hz) * slope / frames_per_beat) {
                return Err(MappedClockError::UnsupportedRate);
            }
        }
        Ok(())
    }

    /// Direct O(log N) projection, including arbitrarily distant intended F/E.
    pub(crate) fn position_at(
        &self,
        epoch: MappedClockEpoch,
        output_frame: u64,
    ) -> Result<FractionalSourcePosition, MappedClockError> {
        if epoch.is_general_compact() {
            return self.position_at_general(epoch, output_frame);
        }
        if epoch.is_compact_anchored() {
            return self.position_at_compact(epoch, output_frame);
        }
        if epoch.is_anchored() {
            self.validate_exact_continuation_epoch(epoch)?;
            return self.anchored_position_at(epoch, output_frame);
        }
        self.validate_epoch(epoch)?;
        self.canonical_position_at(epoch, output_frame)
    }

    fn canonical_position_at(
        &self,
        epoch: MappedClockEpoch,
        output_frame: u64,
    ) -> Result<FractionalSourcePosition, MappedClockError> {
        let beat = epoch
            .beat_at(output_frame, self.beat_start, self.quarters)
            .ok_or(MappedClockError::NumericalUnavailable)?;
        let seconds = self
            .map
            .canonical_source_at_beat(beat)
            .ok_or(MappedClockError::UnsupportedCoverage)?;
        position_from_source(seconds, self.loaded_rate_hz)
            .ok_or(MappedClockError::NumericalUnavailable)
    }

    /// At most two 64-step scalar binary searches and O(log N) map queries.
    /// The end is canonical even if one output interval crosses many map knots.
    pub(crate) fn refresh_span(
        &self,
        epoch: MappedClockEpoch,
        output_frame: u64,
    ) -> Result<MappedSourceSpan, MappedClockError> {
        if epoch.is_general_compact() {
            return self.refresh_general_span(epoch, output_frame);
        }
        if epoch.is_compact_anchored() {
            return self.refresh_span_compact(epoch, output_frame);
        }
        if epoch.is_anchored() {
            return self.refresh_anchored_span(epoch, output_frame);
        }
        self.validate_epoch(epoch)?;
        let total = epoch
            .total_at(output_frame)
            .ok_or(MappedClockError::NumericalUnavailable)?;
        let cycle = cycle_at(total, self.quarters);
        let beat = epoch
            .beat_at(output_frame, self.beat_start, self.quarters)
            .ok_or(MappedClockError::NumericalUnavailable)?;
        // Floating remainder plus a large map offset can equal the exclusive
        // loop beat. Keep its exact canonical position, but use the admitted
        // left derivative rather than an interval outside this loop.
        let segment_probe = if beat == self.beat_start + self.quarters {
            beat.next_down()
        } else {
            beat
        };
        let segment = self
            .map
            .canonical_segment_at_beat(segment_probe)
            .ok_or(MappedClockError::UnsupportedCoverage)?;
        let left = segment.beat_start.max(self.beat_start);
        let right = segment.beat_end.min(self.beat_start + self.quarters);
        let boundary_reached = |frame, boundary| {
            let Some(candidate) = epoch.total_at(frame) else {
                return true;
            };
            let candidate_cycle = cycle_at(candidate, self.quarters);
            candidate_cycle > cycle
                || (candidate_cycle == cycle
                    && self.beat_start + candidate.rem_euclid(self.quarters) >= boundary)
        };
        let start_frame = first_true(epoch.first_output_frame, output_frame, |frame| {
            boundary_reached(frame, left)
        });
        let next = output_frame
            .checked_add(1)
            .ok_or(MappedClockError::NumericalUnavailable)?;
        if !boundary_reached(u64::MAX, right) {
            return Err(MappedClockError::NumericalUnavailable);
        }
        let end_frame = first_true(next, u64::MAX, |frame| boundary_reached(frame, right));
        let endpoint = self.canonical_position_at(epoch, end_frame)?;
        let span = MappedSourceSpan {
            epoch,
            segment,
            program: SpanProgram::Legacy(LegacySpanProgram {
                beat_start: self.beat_start,
                quarters: self.quarters,
                source_period: self
                    .domain
                    .musical_period
                    .scalar_period()
                    .ok_or(MappedClockError::InvalidGeometry)?,
            }),
            loaded_rate_hz: self.loaded_rate_hz,
            start_frame,
            end_frame,
            endpoint_frame: endpoint.frame,
            endpoint_fraction: endpoint.fraction,
            source_start: self.domain.region.start as u64,
        };
        if span.position_at(output_frame).is_none() || span.local_rate().is_none() {
            return Err(MappedClockError::NumericalUnavailable);
        }
        Ok(span)
    }
}

fn valid_rate(rate: f64) -> bool {
    rate.is_finite() && (SPEED_MIN..=SPEED_MAX).contains(&rate)
}

fn cycle_at(total: f64, quarters: f64) -> f64 {
    // Division alone can round a point immediately before a wrap to the next
    // integer. Derive its cycle from the SAME remainder used for source addressing.
    ((total - total.rem_euclid(quarters)) / quarters).round()
}

fn position_from_source(seconds: f64, sample_rate_hz: u32) -> Option<FractionalSourcePosition> {
    let coordinate = seconds * f64::from(sample_rate_hz);
    if !coordinate.is_finite() || coordinate < 0.0 || coordinate >= usize::MAX as f64 {
        return None;
    }
    let whole = coordinate.floor();
    Some(FractionalSourcePosition {
        frame: whole as usize,
        fraction: coordinate - whole,
        seek_mode: ExplicitSeekMode::Normal,
    })
}

fn first_true(mut low: u64, mut high: u64, predicate: impl Fn(u64) -> bool) -> u64 {
    while low < high {
        let middle = low + (high - low) / 2;
        if predicate(middle) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

#[cfg(test)]
#[path = "mapped_source_clock_tests.rs"]
pub(crate) mod tests;
