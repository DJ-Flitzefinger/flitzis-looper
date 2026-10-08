//! Frozen temporal windows over the complete unchanged detector sequence.

use super::{RepresentativeRegion, TIMING_HALFWIDTH_SECONDS, aggregate_fit, numerical_tolerance};
use crate::tempo_summary::math::{Point, feasible_timing_bound, robust_line};

const MIN_POSITIONS: usize = 24;
const MIN_OBSERVED_SPAN_SECONDS: f64 = 30.0;

pub(super) fn evaluate(
    beat_seconds: &[f64],
    points: &[Point],
    duration: f64,
    denominator: u32,
) -> Vec<RepresentativeRegion> {
    [
        ("middle", duration * 0.2, duration * 0.8),
        ("early", 0.0, duration / 3.0),
        ("central", duration / 3.0, duration * 2.0 / 3.0),
        ("late", duration * 2.0 / 3.0, duration),
    ]
    .into_iter()
    .map(|(id, start, end)| evaluate_region(id, start, end, beat_seconds, points, denominator))
    .collect()
}

fn evaluate_region(
    id: &'static str,
    start_seconds: f64,
    end_seconds: f64,
    beat_seconds: &[f64],
    points: &[Point],
    denominator: u32,
) -> RepresentativeRegion {
    let raw_indices: Vec<_> = beat_seconds
        .iter()
        .enumerate()
        .filter_map(|(index, &time)| (time >= start_seconds && time < end_seconds).then_some(index))
        .collect();
    let local: Vec<_> = points
        .iter()
        .copied()
        .filter(|point| point.seconds >= start_seconds && point.seconds < end_seconds)
        .collect();
    let mut result = RepresentativeRegion {
        id,
        start_seconds,
        end_seconds,
        assigned_positions: local.len(),
        raw_positions: raw_indices.len(),
        inlier_raw_indices: Vec::new(),
        excluded_raw_indices: raw_indices.clone(),
        status: "unsupported",
        reasons: Vec::new(),
        fit: None,
        observed_span_seconds: 0.0,
    };
    let window_span = end_seconds - start_seconds;
    if window_span < MIN_OBSERVED_SPAN_SECONDS {
        result.reasons.push("no_long_region");
    }
    if local.len() < MIN_POSITIONS {
        result.reasons.push("insufficient_positions");
        return result;
    }
    let tolerance = numerical_tolerance(&local);
    let threshold = 2.0 * TIMING_HALFWIDTH_SECONDS + tolerance;
    let Some((line, inliers)) = robust_line(&local, &local, threshold, TIMING_HALFWIDTH_SECONDS)
    else {
        result.reasons.push("no_robust_fit");
        return result;
    };
    result.inlier_raw_indices = inliers.iter().map(|point| point.raw_index).collect();
    let mut retained = result.inlier_raw_indices.iter().peekable();
    result.excluded_raw_indices = raw_indices
        .iter()
        .copied()
        .filter(|index| {
            if retained.peek().is_some_and(|&&next| next == *index) {
                retained.next();
                false
            } else {
                true
            }
        })
        .collect();
    if result.excluded_raw_indices.len() > raw_indices.len() / 10 {
        result.reasons.push("too_many_exclusions");
    }
    if result
        .excluded_raw_indices
        .windows(3)
        .any(|indices| indices[1] == indices[0] + 1 && indices[2] == indices[1] + 1)
    {
        result.reasons.push("consecutive_exclusions");
    }
    if inliers.len() < MIN_POSITIONS {
        result.reasons.push("insufficient_positions");
    }
    let first = inliers
        .first()
        .expect("robust fit has at least six inliers")
        .seconds;
    let last = inliers
        .last()
        .expect("robust fit has at least six inliers")
        .seconds;
    result.observed_span_seconds = last - first;
    if result.observed_span_seconds < MIN_OBSERVED_SPAN_SECONDS
        && !result.reasons.contains(&"no_long_region")
    {
        result.reasons.push("no_long_region");
    }
    if first > start_seconds + window_span * 0.2
        || last < end_seconds - window_span * 0.2
        || last - first < window_span * 0.6
    {
        result.reasons.push("insufficient_temporal_coverage");
    }
    if threshold >= line.period * f64::from(denominator) * 0.25 {
        result.reasons.push("insufficient_period_separation");
    }
    if !feasible_timing_bound(&inliers, line, TIMING_HALFWIDTH_SECONDS, tolerance) {
        result.reasons.push("inconsistent_timing_bound");
    }
    result.fit = aggregate_fit(line, &inliers, denominator);
    if result.fit.is_none() {
        result.reasons.push("no_finite_fit");
    }
    if result.reasons.is_empty() {
        result.status = "unverified";
        result.reasons.push("unverified_quarter_notes");
    }
    result
}

pub(super) fn select(regions: &[RepresentativeRegion]) -> Option<&RepresentativeRegion> {
    if let Some(central) = regions
        .first()
        .filter(|region| region.status == "unverified")
    {
        return Some(central);
    }
    regions
        .iter()
        .skip(1)
        .filter(|region| region.status == "unverified")
        .max_by(|left, right| {
            left.observed_span_seconds
                .total_cmp(&right.observed_span_seconds)
                .then_with(|| {
                    left.inlier_raw_indices
                        .len()
                        .cmp(&right.inlier_raw_indices.len())
                })
                .then_with(|| right.start_seconds.total_cmp(&left.start_seconds))
        })
}
