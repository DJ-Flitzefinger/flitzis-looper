//! Pure bounded queries. Supported geometry is not physical/audio authority.

use super::MAX_PROJECTION_POINTS;
use super::record::{Anchor, Coverage, ValidatedMap};

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
        }
    }

    pub(super) fn supported(self) -> bool {
        matches!(self.coordinate_status, "supported" | "anchor")
            && self.interval_status == "trusted"
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
        let touching_gap = index
            .checked_sub(1)
            .map(|left| self.segments[left].coverage)
            .into_iter()
            .chain(self.segments.get(index).map(|right| right.coverage))
            .find(|coverage| *coverage != Coverage::Trusted);
        let coverage = touching_gap.unwrap_or(Coverage::Trusted);
        Evaluation {
            source_seconds: Some(anchor.seconds),
            beat: Some(anchor.beat),
            local_bpm: (coverage == Coverage::Trusted).then_some(self.segments[segment_index].bpm),
            coordinate_status: "anchor",
            interval_status: coverage.label(),
            segment_index: Some(segment_index),
            physical_domain: self.physical_domain(anchor.seconds),
            meter_status: self.meter_status(segment_index),
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
        if segment.coverage != Coverage::Trusted {
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
            return missing;
        }
        let left = self.anchors[index];
        let right = self.anchors[index + 1];
        let fraction = (target - axis(left, source_axis))
            / (axis(right, source_axis) - axis(left, source_axis));
        let other = axis(left, !source_axis)
            + fraction * (axis(right, !source_axis) - axis(left, !source_axis));
        if !fraction.is_finite()
            || !other.is_finite()
            || (!extrapolate && !(0.0..=1.0).contains(&fraction))
        {
            return self.missing_axis(target, source_axis, "numerical_unavailable");
        }
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
                "trusted"
            },
            segment_index: Some(index),
            physical_domain: self.physical_domain(source),
            meter_status: self.meter_status(index),
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
        end.interval_status = "trusted";
        end.meter_status = self.meter_status(endpoint_segment);
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
        let steps = beat / step;
        if !steps.is_finite() {
            return Evaluation::unavailable("numerical_unavailable");
        }
        let lower = steps.floor();
        let lower_beat = lower * step;
        if !lower_beat.is_finite() {
            return Evaluation::unavailable("numerical_unavailable");
        }
        // An exact supported endpoint needs no hypothetical point beyond it.
        if lower_beat == beat {
            let exact = self.at_beat(lower_beat);
            if exact.source_seconds == Some(source) && self.same_supported_island(current, exact) {
                return exact;
            }
        }
        let upper = lower + 1.0;
        let upper_beat = upper * step;
        if upper <= lower || !upper_beat.is_finite() || lower_beat >= upper_beat {
            return Evaluation::unavailable("numerical_unavailable");
        }
        // Compare the source coordinate with the shared inverse's midpoint.
        // A B/S roundoff at the midpoint must not erase a representable request
        // immediately before it. There is no tolerance or scalar tie change.
        let midpoint_tick = lower + 0.5;
        if midpoint_tick <= lower || midpoint_tick >= upper {
            return Evaluation::unavailable("numerical_unavailable");
        }
        let midpoint = self.at_beat(midpoint_tick * step);
        // The musical coordinate first identifies the nearest target. A
        // supported same-island midpoint refines only the floating-point
        // comparison; missing coverage at that hypothetical point must not
        // discard an otherwise valid target in a partial final subdivision.
        let choose_lower = if self.same_supported_island(current, midpoint) {
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
        if !self.same_supported_island(current, target) {
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

    pub(super) fn project(&self, first: f64, last: f64, step: f64, limit: usize) -> Projection {
        if !first.is_finite()
            || !last.is_finite()
            || !step.is_finite()
            || step <= 0.0
            || last < first
            || limit == 0
            || limit > MAX_PROJECTION_POINTS
        {
            return Projection::refused("invalid");
        }
        let start_tick = (first / step).ceil();
        let end_tick = (last / step).floor();
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
            let beat = (start_tick + index as f64) * step;
            if !beat.is_finite() || previous.is_some_and(|before| beat <= before) {
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
