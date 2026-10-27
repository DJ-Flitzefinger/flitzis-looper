//! Pure bounded queries. Supported geometry is not physical/audio authority.

use std::cmp::Ordering;

use super::MAX_PROJECTION_POINTS;
use super::record::{Anchor, Coverage, ValidatedMap};

/// Fixed original anchors for the same inverse used by control queries.
/// Copying this value conveys geometry only, never source or map ownership.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CanonicalBeatSegment {
    pub(crate) source_start: f64,
    pub(crate) source_end: f64,
    pub(crate) beat_start: f64,
    pub(crate) beat_end: f64,
}

impl CanonicalBeatSegment {
    fn from_anchors(left: Anchor, right: Anchor) -> Self {
        Self {
            source_start: left.seconds,
            source_end: right.seconds,
            beat_start: left.beat,
            beat_end: right.beat,
        }
    }

    /// Keep exact anchor bits and the canonical interior operation order.
    pub(crate) fn source_at_beat(self, beat: f64) -> Option<f64> {
        if beat == self.beat_start {
            return Some(self.source_start);
        }
        if beat == self.beat_end {
            return Some(self.source_end);
        }
        self.interpolate(beat, false, false)
    }

    pub(crate) fn seconds_per_quarter(self) -> f64 {
        (self.source_end - self.source_start) / (self.beat_end - self.beat_start)
    }

    fn interpolate(self, target: f64, source_axis: bool, extrapolate: bool) -> Option<f64> {
        let (left, right, other_left, other_right) = if source_axis {
            (
                self.source_start,
                self.source_end,
                self.beat_start,
                self.beat_end,
            )
        } else {
            (
                self.beat_start,
                self.beat_end,
                self.source_start,
                self.source_end,
            )
        };
        let fraction = (target - left) / (right - left);
        let other = other_left + fraction * (other_right - other_left);
        (fraction.is_finite()
            && other.is_finite()
            && (extrapolate || (0.0..=1.0).contains(&fraction)))
        .then_some(other)
    }

    pub(crate) fn matches_exact(self, other: Self) -> bool {
        self.source_start.to_bits() == other.source_start.to_bits()
            && self.source_end.to_bits() == other.source_end.to_bits()
            && self.beat_start.to_bits() == other.beat_start.to_bits()
            && self.beat_end.to_bits() == other.beat_end.to_bits()
    }
}

/// Off-realtime whole-interval proof and extrema, derived from the original map.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CanonicalLoopInterval {
    pub(crate) beat_start: f64,
    pub(crate) source_end: f64,
    pub(crate) min_seconds_per_quarter: f64,
    pub(crate) max_seconds_per_quarter: f64,
}

/// Original metadata only. The caller separately qualifies quarter/bar closure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalPhaseReference {
    normalized_downbeat: u8,
    bar_qualified: bool,
}

impl CanonicalPhaseReference {
    pub(crate) fn downbeat(self) -> u8 {
        self.normalized_downbeat
    }

    pub(crate) fn bar_qualified(self) -> bool {
        self.bar_qualified
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Evaluation {
    pub(super) source_seconds: Option<f64>,
    pub(super) beat: Option<f64>,
    pub(super) local_bpm: Option<f64>,
    pub(super) coordinate_status: &'static str,
    pub(super) interval_status: &'static str,
    pub(super) segment_index: Option<usize>,
    pub(super) physical_domain: &'static str,
    pub(super) meter_status: &'static str,
    pub(super) downbeat_label: Option<i64>,
}

impl Evaluation {
    fn unavailable(status: &'static str) -> Self {
        Self {
            source_seconds: None,
            beat: None,
            local_bpm: None,
            coordinate_status: status,
            interval_status: "unavailable",
            segment_index: None,
            physical_domain: "unavailable",
            meter_status: "unavailable",
            downbeat_label: None,
        }
    }

    pub(super) fn supported(self) -> bool {
        matches!(self.coordinate_status, "supported" | "anchor")
            && matches!(self.interval_status, "trusted" | "estimated")
    }
}

pub(super) struct Projection {
    pub(super) status: &'static str,
    pub(super) points: Vec<Evaluation>,
}

impl Projection {
    fn refused(status: &'static str) -> Self {
        Self {
            status,
            points: Vec::new(),
        }
    }
}

impl ValidatedMap {
    pub(super) fn canonical_segment_by_source_ordering(
        &self,
        compare: impl FnMut(f64) -> Option<Ordering>,
    ) -> Option<CanonicalBeatSegment> {
        self.canonical_segment_by_ordering(true, false, compare)
    }

    pub(super) fn canonical_phase_origin_segment_by_source_ordering(
        &self,
        mut compare: impl FnMut(f64) -> Option<Ordering>,
    ) -> Option<(CanonicalBeatSegment, Option<f64>)> {
        // virtual-source-edge-origin-v1 extends only the requested phase axis.
        // A usable island after an intro gap or before a tail gap is no edge.
        let edge = if compare(0.0)? == Ordering::Greater {
            let left = *self.anchors.first()?;
            let right = *self.anchors.get(1)?;
            (left.seconds == 0.0 && self.segments.first()?.coverage.usable())
                .then_some((left, right, 0.0))?
        } else if compare(self.source_duration)? == Ordering::Less {
            let index = self.anchors.len().checked_sub(2)?;
            let left = self.anchors[index];
            let right = self.anchors[index + 1];
            (right.seconds == self.source_duration && self.segments[index].coverage.usable())
                .then_some((left, right, self.source_duration))?
        } else {
            return self
                .canonical_segment_by_ordering(true, true, compare)
                .map(|segment| (segment, None));
        };
        Some((
            CanonicalBeatSegment::from_anchors(edge.0, edge.1),
            Some(edge.2),
        ))
    }

    pub(super) fn canonical_segment_by_beat_ordering(
        &self,
        compare: impl FnMut(f64) -> Option<Ordering>,
    ) -> Option<CanonicalBeatSegment> {
        self.canonical_segment_by_ordering(false, false, compare)
    }

    fn canonical_segment_by_ordering(
        &self,
        source_axis: bool,
        require_supported_anchor: bool,
        mut compare: impl FnMut(f64) -> Option<Ordering>,
    ) -> Option<CanonicalBeatSegment> {
        let mut low = 0;
        let mut high = self.anchors.len();
        let mut final_ordering = None;
        // Binary search uses at most ceil(log2(MAX_ANCHORS+1)) = 18 comparisons;
        // the opt-in grid-origin guard then checks one exact anchor. Cache the final
        // anchor's comparison to distinguish equality from an outside query.
        while low < high {
            let middle = low + (high - low) / 2;
            let ordering = compare(axis(self.anchors[middle], source_axis))?;
            if middle + 1 == self.anchors.len() {
                final_ordering = Some(ordering);
            }
            if ordering == Ordering::Greater {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        if low == 0 || (low == self.anchors.len() && final_ordering? != Ordering::Equal) {
            return None;
        }
        // The explicit grid zero must be a supported point, including BOTH
        // neighbors at an exact anchor. Preserve old continuation selection.
        if require_supported_anchor
            && compare(axis(self.anchors[low - 1], source_axis))? == Ordering::Equal
            && (low
                .checked_sub(2)
                .is_some_and(|left| !self.segments[left].coverage.usable())
                || self
                    .segments
                    .get(low - 1)
                    .is_some_and(|right| !right.coverage.usable()))
        {
            return None;
        }
        let index = (low - 1).min(self.segments.len() - 1);
        self.segments[index].coverage.usable().then(|| {
            CanonicalBeatSegment::from_anchors(self.anchors[index], self.anchors[index + 1])
        })
    }

    pub(super) fn visit_canonical_segments(
        &self,
        beat_start: f64,
        beat_end: f64,
        mut visitor: impl FnMut(CanonicalBeatSegment) -> Option<()>,
    ) -> Option<()> {
        if !beat_start.is_finite()
            || !beat_end.is_finite()
            || beat_start >= beat_end
            || beat_start < self.anchors.first()?.beat
            || beat_end > self.anchors.last()?.beat
        {
            return None;
        }
        let first = self
            .anchors
            .partition_point(|anchor| anchor.beat <= beat_start)
            .saturating_sub(1)
            .min(self.segments.len() - 1);
        let after = self
            .anchors
            .partition_point(|anchor| anchor.beat < beat_end)
            .min(self.segments.len());
        // Match source_after_beats: an exterior gap touching only an endpoint
        // cannot contaminate the positive interior of this retained loop.
        if first >= after || self.unsupported_prefix[first] != self.unsupported_prefix[after] {
            return None;
        }
        for index in first..after {
            visitor(CanonicalBeatSegment::from_anchors(
                self.anchors[index],
                self.anchors[index + 1],
            ))?;
        }
        Some(())
    }

    /// The exact-boundary caller proves start < end before this metadata scan.
    /// Open exterior endpoints do not add a neighboring gap or meter interval.
    pub(super) fn visit_canonical_segments_exact(
        &self,
        compare_start: impl FnMut(f64) -> Option<Ordering>,
        compare_end: impl FnMut(f64) -> Option<Ordering>,
        require_four_four: bool,
        mut visitor: impl FnMut(CanonicalBeatSegment) -> Option<()>,
    ) -> Option<()> {
        let (first, after) = self.canonical_interval_indices_exact(compare_start, compare_end)?;
        if self.unsupported_prefix[first] != self.unsupported_prefix[after]
            || (require_four_four && self.meter_prefix[first] != self.meter_prefix[after])
        {
            return None;
        }
        for index in first..after {
            visitor(CanonicalBeatSegment::from_anchors(
                self.anchors[index],
                self.anchors[index + 1],
            ))?;
        }
        Some(())
    }

    pub(super) fn canonical_phase_reference_exact(
        &self,
        compare_start: impl FnMut(f64) -> Option<Ordering>,
        compare_end: impl FnMut(f64) -> Option<Ordering>,
    ) -> Option<CanonicalPhaseReference> {
        let (first, after) = self.canonical_interval_indices_exact(compare_start, compare_end)?;
        if self.unsupported_prefix[first] != self.unsupported_prefix[after] {
            return None;
        }
        let bar_qualified = self.meter_prefix[first] == self.meter_prefix[after]
            && self.unknown_phase_prefix[first] == self.unknown_phase_prefix[after]
            && self.phase_changes[first] == self.phase_changes[after - 1];
        let normalized_downbeat = if bar_qualified {
            u8::try_from(self.segments[first].downbeat_label?.rem_euclid(4)).ok()?
        } else {
            0
        };
        Some(CanonicalPhaseReference {
            normalized_downbeat,
            bar_qualified,
        })
    }

    /// The caller proves strictly ordered exact bounds. Both consumers retain
    /// the same right-interior/left-exclusive-end original segment selection.
    fn canonical_interval_indices_exact(
        &self,
        mut compare_start: impl FnMut(f64) -> Option<Ordering>,
        mut compare_end: impl FnMut(f64) -> Option<Ordering>,
    ) -> Option<(usize, usize)> {
        let first_beat = self.anchors.first()?.beat;
        let last_beat = self.anchors.last()?.beat;
        if compare_start(first_beat)? == Ordering::Greater
            || compare_start(last_beat)? != Ordering::Greater
            || compare_end(first_beat)? != Ordering::Less
            || compare_end(last_beat)? == Ordering::Less
        {
            return None;
        }
        let mut low = 0;
        let mut high = self.anchors.len();
        while low < high {
            let middle = low + (high - low) / 2;
            if compare_start(self.anchors[middle].beat)? != Ordering::Greater {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let first = low.checked_sub(1)?.min(self.segments.len() - 1);
        low = 0;
        high = self.anchors.len();
        while low < high {
            let middle = low + (high - low) / 2;
            if compare_end(self.anchors[middle].beat)? == Ordering::Less {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let after = low.min(self.segments.len());
        (first < after).then_some((first, after))
    }

    pub(super) fn canonical_segment_at_beat(&self, beat: f64) -> Option<CanonicalBeatSegment> {
        if !beat.is_finite()
            || beat < self.anchors[0].beat
            || beat > self.anchors[self.anchors.len() - 1].beat
        {
            return None;
        }
        let index = self
            .anchors
            .partition_point(|anchor| anchor.beat <= beat)
            .saturating_sub(1)
            .min(self.segments.len() - 1);
        self.segments[index].coverage.usable().then(|| {
            CanonicalBeatSegment::from_anchors(self.anchors[index], self.anchors[index + 1])
        })
    }

    pub(super) fn canonical_loop_interval(
        &self,
        source_start: f64,
        quarters: f64,
        four_four: bool,
    ) -> Option<CanonicalLoopInterval> {
        if !quarters.is_finite() || quarters <= 0.0 {
            return None;
        }
        let start = self.at_source(source_start);
        let beat_start = start.beat?;
        let end = self.source_after_beats(source_start, quarters, four_four);
        if !end.supported() || beat_start + quarters <= beat_start {
            return None;
        }
        let source_end = end.source_seconds?;
        let first = self
            .anchors
            .partition_point(|anchor| anchor.seconds <= source_start)
            .saturating_sub(1)
            .min(self.segments.len() - 1);
        let after = self
            .anchors
            .partition_point(|anchor| anchor.seconds < source_end)
            .max(first + 1)
            .min(self.segments.len());
        // This scan is construction-only. Realtime refresh uses binary lookup.
        let mut minimum = f64::INFINITY;
        let mut maximum = 0.0_f64;
        for index in first..after {
            let slope =
                CanonicalBeatSegment::from_anchors(self.anchors[index], self.anchors[index + 1])
                    .seconds_per_quarter();
            if !slope.is_finite() || slope <= 0.0 {
                return None;
            }
            minimum = minimum.min(slope);
            maximum = maximum.max(slope);
        }
        Some(CanonicalLoopInterval {
            beat_start,
            source_end,
            min_seconds_per_quarter: minimum,
            max_seconds_per_quarter: maximum,
        })
    }

    pub(super) fn sample_index(&self, seconds: f64) -> Option<u64> {
        if !seconds.is_finite() || seconds < 0.0 || seconds >= self.source_duration {
            return None;
        }
        let sample = (seconds * f64::from(self.sample_rate_hz)).round();
        (sample >= 0.0 && sample < self.source_frames as f64).then_some(sample as u64)
    }

    pub(super) fn at_source(&self, source: f64) -> Evaluation {
        self.evaluate(source, true, false)
    }

    pub(super) fn at_beat(&self, beat: f64) -> Evaluation {
        self.evaluate(beat, false, false)
    }

    pub(super) fn extrapolate_source(&self, source: f64) -> Evaluation {
        self.evaluate(source, true, true)
    }

    pub(super) fn extrapolate_beat(&self, beat: f64) -> Evaluation {
        self.evaluate(beat, false, true)
    }

    fn evaluate(&self, target: f64, source_axis: bool, extrapolate: bool) -> Evaluation {
        if !target.is_finite() {
            return Evaluation::unavailable("invalid");
        }
        let position = self.anchors.binary_search_by(|anchor| {
            axis(*anchor, source_axis)
                .partial_cmp(&target)
                .expect("validated finite axes")
        });
        match position {
            Ok(index) => self.exact(index),
            Err(index) if index == 0 || index == self.anchors.len() => {
                if !extrapolate {
                    return self.missing_axis(target, source_axis, "outside");
                }
                let segment = if index == 0 {
                    0
                } else {
                    self.segments.len() - 1
                };
                self.interval(target, source_axis, segment, true)
            }
            Err(index) => self.interval(target, source_axis, index - 1, false),
        }
    }

    fn exact(&self, index: usize) -> Evaluation {
        let anchor = self.anchors[index];
        let segment_index = index.min(self.segments.len() - 1);
        let touching = [
            index
                .checked_sub(1)
                .map(|left| self.segments[left].coverage),
            self.segments.get(index).map(|right| right.coverage),
        ];
        // Preserve the historical first unsupported neighbor. Estimated is
        // usable provenance and must never hide either unsupported class.
        let coverage = touching
            .into_iter()
            .flatten()
            .find(|c| !c.usable())
            .or_else(|| {
                touching
                    .into_iter()
                    .flatten()
                    .find(|c| *c == Coverage::Estimated)
            })
            .unwrap_or(Coverage::Trusted);
        Evaluation {
            source_seconds: Some(anchor.seconds),
            beat: Some(anchor.beat),
            local_bpm: coverage
                .usable()
                .then_some(self.segments[segment_index].bpm),
            coordinate_status: "anchor",
            interval_status: coverage.label(),
            segment_index: Some(segment_index),
            physical_domain: self.physical_domain(anchor.seconds),
            meter_status: self.meter_status(segment_index),
            downbeat_label: self.segments[segment_index].downbeat_label,
        }
    }

    fn interval(
        &self,
        target: f64,
        source_axis: bool,
        index: usize,
        extrapolate: bool,
    ) -> Evaluation {
        let segment = self.segments[index];
        if !segment.coverage.usable() {
            let mut missing = self.missing_axis(
                target,
                source_axis,
                if segment.coverage == Coverage::Uncertain {
                    "uncertain"
                } else {
                    "gap"
                },
            );
            missing.interval_status = segment.coverage.label();
            missing.segment_index = Some(index);
            missing.meter_status = self.meter_status(index);
            missing.downbeat_label = segment.downbeat_label;
            return missing;
        }
        let left = self.anchors[index];
        let right = self.anchors[index + 1];
        let Some(other) = CanonicalBeatSegment::from_anchors(left, right).interpolate(
            target,
            source_axis,
            extrapolate,
        ) else {
            return self.missing_axis(target, source_axis, "numerical_unavailable");
        };
        let (source, beat) = if source_axis {
            (target, other)
        } else {
            (other, target)
        };
        Evaluation {
            source_seconds: Some(source),
            beat: Some(beat),
            local_bpm: Some(segment.bpm),
            coordinate_status: if extrapolate {
                "unverified_extrapolation"
            } else {
                "supported"
            },
            interval_status: if extrapolate {
                "unverified_extrapolation"
            } else {
                segment.coverage.label()
            },
            segment_index: Some(index),
            physical_domain: self.physical_domain(source),
            meter_status: self.meter_status(index),
            downbeat_label: segment.downbeat_label,
        }
    }

    fn missing_axis(&self, target: f64, source_axis: bool, status: &'static str) -> Evaluation {
        let mut missing = Evaluation::unavailable(status);
        if source_axis {
            missing.source_seconds = Some(target);
            missing.physical_domain = self.physical_domain(target);
        } else {
            missing.beat = Some(target);
        }
        missing
    }

    fn physical_domain(&self, source: f64) -> &'static str {
        if source < 0.0 {
            "before_source"
        } else if source < self.source_duration {
            "within_source"
        } else if source == self.source_duration {
            "exclusive_source_end"
        } else {
            "after_source"
        }
    }

    fn meter_status(&self, index: usize) -> &'static str {
        if self.segments[index].four_four {
            "quarter_note_4_4_verified"
        } else {
            "unknown"
        }
    }

    pub(super) fn source_after_beats(
        &self,
        source: f64,
        beats: f64,
        four_four: bool,
    ) -> Evaluation {
        if !beats.is_finite() {
            return Evaluation::unavailable("invalid");
        }
        let start = self.at_source(source);
        if !matches!(start.coordinate_status, "supported" | "anchor") {
            return start;
        }
        let mut end = self.at_beat(start.beat.unwrap() + beats);
        if !matches!(end.coordinate_status, "supported" | "anchor") {
            return end;
        }
        if !matches!(
            start.physical_domain,
            "within_source" | "exclusive_source_end"
        ) || !matches!(
            end.physical_domain,
            "within_source" | "exclusive_source_end"
        ) {
            return Evaluation::unavailable("outside_physical_source");
        }
        let (low, high) = if source <= end.source_seconds.unwrap() {
            (source, end.source_seconds.unwrap())
        } else {
            (end.source_seconds.unwrap(), source)
        };
        // Interval-open endpoints: an adjacent gap outside this exact interval
        // cannot contaminate an otherwise supported bar endpoint.
        let first = self
            .anchors
            .partition_point(|anchor| anchor.seconds <= low)
            .saturating_sub(1)
            .min(self.segments.len() - 1);
        let after = self
            .anchors
            .partition_point(|anchor| anchor.seconds < high)
            .max(first + 1)
            .min(self.segments.len());
        if self.unsupported_prefix[first] != self.unsupported_prefix[after] {
            return Evaluation::unavailable("unsupported_crossing");
        }
        if four_four && self.meter_prefix[first] != self.meter_prefix[after] {
            return Evaluation::unavailable("unsupported_meter");
        }
        let endpoint_segment = if beats >= 0.0 { after - 1 } else { first };
        end.interval_status = if self.estimated_prefix[first] != self.estimated_prefix[after] {
            "estimated"
        } else {
            "trusted"
        };
        end.meter_status = self.meter_status(endpoint_segment);
        end.downbeat_label = self.segments[endpoint_segment].downbeat_label;
        end.local_bpm = Some(self.segments[endpoint_segment].bpm);
        end
    }

    /// Versioned later musical ties; scalar nearest-even behavior is untouched.
    pub(super) fn nearest_grid(&self, source: f64, step: f64) -> Evaluation {
        if !step.is_finite() || step <= 0.0 {
            return Evaluation::unavailable("invalid");
        }
        let current = self.at_source(source);
        if !current.supported() {
            return current;
        }
        let Some(beat) = current.beat else {
            return Evaluation::unavailable("numerical_unavailable");
        };
        let bar_grid = step >= 4.0 && step.rem_euclid(4.0) == 0.0;
        let phase = if bar_grid {
            let Some(label) = current.downbeat_label else {
                return Evaluation::unavailable("unsupported_downbeat");
            };
            label.rem_euclid(4) as f64
        } else {
            0.0
        };
        let compatible = |other| {
            self.same_supported_island(current, other)
                && (!bar_grid || self.same_supported_phase(current, other))
        };
        let steps = (beat - phase) / step;
        if !steps.is_finite() {
            return Evaluation::unavailable("numerical_unavailable");
        }
        let lower = steps.floor();
        let lower_beat = phase + lower * step;
        if !lower_beat.is_finite() {
            return Evaluation::unavailable("numerical_unavailable");
        }
        // An exact supported endpoint needs no hypothetical point beyond it.
        if lower_beat == beat && (!bar_grid || lower_beat.rem_euclid(4.0) == phase) {
            let exact = self.at_beat(lower_beat);
            if exact.source_seconds == Some(source) && compatible(exact) {
                return exact;
            }
        }
        let upper = lower + 1.0;
        let upper_beat = phase + upper * step;
        if upper <= lower
            || !upper_beat.is_finite()
            || lower_beat >= upper_beat
            || (bar_grid
                && (lower_beat.rem_euclid(4.0) != phase || upper_beat.rem_euclid(4.0) != phase))
        {
            return Evaluation::unavailable("numerical_unavailable");
        }
        // Compare the source coordinate with the shared inverse's midpoint.
        // A B/S roundoff at the midpoint must not erase a representable request
        // immediately before it. There is no tolerance or scalar tie change.
        let midpoint_tick = lower + 0.5;
        if midpoint_tick <= lower || midpoint_tick >= upper {
            return Evaluation::unavailable("numerical_unavailable");
        }
        let midpoint = self.at_beat(phase + midpoint_tick * step);
        // The musical coordinate first identifies the nearest target. A
        // supported same-island midpoint refines only the floating-point
        // comparison; missing coverage at that hypothetical point must not
        // discard an otherwise valid target in a partial final subdivision.
        let choose_lower = if compatible(midpoint) {
            match midpoint.source_seconds {
                Some(midpoint_source) => source < midpoint_source,
                None => return Evaluation::unavailable("numerical_unavailable"),
            }
        } else {
            steps - lower < 0.5
        };
        let target = self.at_beat(if choose_lower { lower_beat } else { upper_beat });
        if !target.supported() {
            return target;
        }
        if !compatible(target) {
            return Evaluation::unavailable("unsupported_crossing");
        }
        target
    }

    /// O(1) range check following the O(log N) endpoint queries. Both endpoint
    /// intervals must be trusted; the prefix also rejects an intervening gap.
    fn same_supported_island(&self, left: Evaluation, right: Evaluation) -> bool {
        if !left.supported() || !right.supported() {
            return false;
        }
        let (Some(left), Some(right)) = (left.segment_index, right.segment_index) else {
            return false;
        };
        let first = left.min(right);
        let after_last = left.max(right) + 1;
        self.unsupported_prefix
            .get(first)
            .zip(self.unsupported_prefix.get(after_last))
            .is_some_and(|(before, after)| before == after)
    }

    fn same_supported_phase(&self, left: Evaluation, right: Evaluation) -> bool {
        let (Some(left_index), Some(right_index), Some(left_phase), Some(right_phase)) = (
            left.segment_index,
            right.segment_index,
            left.downbeat_label,
            right.downbeat_label,
        ) else {
            return false;
        };
        let first = left_index.min(right_index);
        let last = left_index.max(right_index);
        left_phase.rem_euclid(4) == right_phase.rem_euclid(4)
            && self.phase_changes[first] == self.phase_changes[last]
    }

    pub(super) fn project(&self, first: f64, last: f64, step: f64, limit: usize) -> Projection {
        self.project_lattice(first, last, step, 0.0, limit)
    }

    pub(super) fn project_lattice(
        &self,
        first: f64,
        last: f64,
        step: f64,
        phase: f64,
        limit: usize,
    ) -> Projection {
        if !first.is_finite()
            || !last.is_finite()
            || !step.is_finite()
            || step <= 0.0
            || !phase.is_finite()
            || last < first
            || limit == 0
            || limit > MAX_PROJECTION_POINTS
        {
            return Projection::refused("invalid");
        }
        let start_tick = ((first - phase) / step).ceil();
        let end_tick = ((last - phase) / step).floor();
        if !start_tick.is_finite() || !end_tick.is_finite() {
            return Projection::refused("numerical_unavailable");
        }
        if end_tick < start_tick {
            return Projection {
                status: "complete",
                points: Vec::new(),
            };
        }
        let count = end_tick - start_tick + 1.0;
        if !count.is_finite() || count > limit as f64 {
            return Projection::refused("projection_limit");
        }
        let mut points = Vec::new();
        if points.try_reserve_exact(count as usize).is_err() {
            return Projection::refused("allocation_refused");
        }
        let mut previous = None;
        for index in 0..count as usize {
            let beat = phase + (start_tick + index as f64) * step;
            if !beat.is_finite()
                || previous.is_some_and(|before| beat <= before)
                || beat < first
                || beat > last
                || (step >= 4.0
                    && step.rem_euclid(4.0) == 0.0
                    && beat.rem_euclid(4.0) != phase.rem_euclid(4.0))
            {
                return Projection::refused("numerical_unavailable");
            }
            points.push(self.at_beat(beat));
            previous = Some(beat);
        }
        Projection {
            status: "complete",
            points,
        }
    }
}

fn axis(anchor: Anchor, source_axis: bool) -> f64 {
    if source_axis {
        anchor.seconds
    } else {
        anchor.beat
    }
}
