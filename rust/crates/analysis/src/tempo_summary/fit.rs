//! Conservative region selection and complete-source consistency diagnostics.

use super::{
    FitRegion, HypothesisDiagnostic, HypothesisStatus, PeriodFit, QuarterNoteHypothesis,
    QuarterNoteVerification, RawTempoEvidence, RejectionReason, WindowDiagnostic,
    math::{Line, Point, feasible_timing_bound, median, robust_line},
};

const MIN_POSITIONS: usize = 24;
const MIN_WINDOW_POSITIONS: usize = 6;
const MAX_CONSECUTIVE_EXCLUSIONS: usize = 2;

struct Candidate {
    line: Line,
    inliers: Vec<Point>,
    region: FitRegion,
}

fn initial_diagnostic(hypothesis: &QuarterNoteHypothesis<'_>) -> HypothesisDiagnostic {
    let count = hypothesis.quarter_counts.len();
    HypothesisDiagnostic {
        id: hypothesis.id.to_owned(),
        provenance: hypothesis.provenance.to_owned(),
        verification: hypothesis.verification,
        quarter_note_denominator: hypothesis.quarter_note_denominator,
        quarter_counts: hypothesis.quarter_counts.to_vec(),
        status: HypothesisStatus::Unsupported,
        reasons: Vec::new(),
        selected_region: None,
        fit: None,
        residual_seconds: vec![None; count],
        inlier_raw_indices: Vec::new(),
        excluded_raw_indices: (0..count).collect(),
        windows: Vec::new(),
    }
}

fn select_candidate(
    points: &[Point],
    duration: f64,
    threshold: f64,
    halfwidth: f64,
    denominator: f64,
) -> Option<Candidate> {
    let middle: Vec<_> = points
        .iter()
        .copied()
        .filter(|point| point.seconds >= duration * 0.2 && point.seconds < duration * 0.8)
        .collect();
    // The middle wins a support-count tie, but neither seed can omit distant checks.
    let mut chosen: Option<Candidate> = None;
    for (region, seed) in [
        (FitRegion::Middle, middle.as_slice()),
        (FitRegion::Complete, points),
    ] {
        let Some((line, inliers)) = robust_line(seed, points, threshold, halfwidth) else {
            continue;
        };
        if threshold >= line.period * denominator * 0.25 {
            continue;
        }
        if chosen
            .as_ref()
            .is_none_or(|previous| inliers.len() > previous.inliers.len())
        {
            chosen = Some(Candidate {
                line,
                inliers,
                region,
            });
        }
    }
    chosen
}

pub(super) fn diagnose(
    evidence: &RawTempoEvidence<'_>,
    hypothesis: &QuarterNoteHypothesis<'_>,
) -> HypothesisDiagnostic {
    let source = evidence.source;
    diagnose_numerical(
        evidence.beat_seconds,
        source.loaded_frame_count as f64 / source.loaded_sample_rate_hz as f64,
        source.timing_error_halfwidth_seconds,
        hypothesis,
    )
}

/// Shared numerical assessment after the caller validates its input domain.
/// Source-bound G2 remains the validating wrapper; metadata must not invent a
/// source identity to obtain the same bounded fit and distant-window checks.
pub(crate) fn diagnose_numerical(
    beat_seconds: &[f64],
    duration: f64,
    halfwidth: f64,
    hypothesis: &QuarterNoteHypothesis<'_>,
) -> HypothesisDiagnostic {
    let mut result = initial_diagnostic(hypothesis);
    let points: Vec<_> = hypothesis
        .quarter_counts
        .iter()
        .enumerate()
        .filter_map(|(raw_index, quarter)| {
            quarter.map(|quarter| Point {
                raw_index,
                quarter,
                seconds: beat_seconds[raw_index],
            })
        })
        .collect();
    if points.len() < MIN_POSITIONS {
        result.reasons.push(RejectionReason::InsufficientPositions);
        return result;
    }
    let denominator = f64::from(hypothesis.quarter_note_denominator);
    let numerical_tolerance =
        points.last().expect("nonempty points").seconds.max(1.0) * 64.0 * f64::EPSILON;
    let threshold = 2.0 * halfwidth + numerical_tolerance;
    let Some(candidate) = select_candidate(&points, duration, threshold, halfwidth, denominator)
    else {
        result.reasons.push(RejectionReason::NoRobustFit);
        return result;
    };
    result.selected_region = Some(candidate.region);
    record_positions(&mut result, &points, &candidate);
    check_coverage(&mut result, &candidate.inliers, duration);
    if !feasible_timing_bound(
        &candidate.inliers,
        candidate.line,
        halfwidth,
        numerical_tolerance,
    ) {
        result
            .reasons
            .push(RejectionReason::InconsistentTimingBound);
    }
    result.windows = window_diagnostics(
        &points,
        duration,
        threshold,
        halfwidth,
        candidate.line,
        denominator,
    );
    let spread = check_windows(
        &mut result,
        candidate.line,
        threshold,
        numerical_tolerance,
        denominator,
    );
    let residuals: Vec<_> = candidate
        .inliers
        .iter()
        .map(|&point| candidate.line.residual(point))
        .collect();
    let minimum = residuals.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = residuals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    result.fit = Some(PeriodFit {
        period_seconds_per_quarter: candidate.line.period * denominator,
        reference_count_numerator: candidate.line.reference,
        quarter_note_denominator: hypothesis.quarter_note_denominator,
        fitted_seconds_at_reference: candidate.line.seconds_at(candidate.line.reference),
        diagnostic_intercept_seconds: candidate.line.mean_seconds
            - candidate.line.period
                * (candidate.line.reference as f64 + candidate.line.mean_quarters),
        period_sensitivity_bound_seconds: candidate.line.sensitivity * denominator,
        max_abs_inlier_residual_seconds: minimum.abs().max(maximum.abs()),
        inlier_residual_range_seconds: maximum - minimum,
        window_period_spread_seconds: spread,
        numerical_tolerance_seconds: numerical_tolerance,
    });
    if result.reasons.is_empty() {
        result.status = match hypothesis.verification {
            QuarterNoteVerification::Verified => HypothesisStatus::SupportedCandidate,
            QuarterNoteVerification::Unverified => {
                result.reasons.push(RejectionReason::UnverifiedQuarterNotes);
                HypothesisStatus::Unverified
            }
        };
    }
    result
}

fn record_positions(result: &mut HypothesisDiagnostic, points: &[Point], candidate: &Candidate) {
    let mut inlier_flags = vec![false; result.quarter_counts.len()];
    for point in &candidate.inliers {
        inlier_flags[point.raw_index] = true;
    }
    for &point in points {
        result.residual_seconds[point.raw_index] = Some(candidate.line.residual(point));
    }
    result.inlier_raw_indices = candidate
        .inliers
        .iter()
        .map(|point| point.raw_index)
        .collect();
    result.excluded_raw_indices = inlier_flags
        .iter()
        .enumerate()
        .filter_map(|(index, &inlier)| (!inlier).then_some(index))
        .collect();
    if result.excluded_raw_indices.len() > result.quarter_counts.len() / 10 {
        result.reasons.push(RejectionReason::TooManyExclusions);
    }
    let mut run = 0;
    for inlier in inlier_flags {
        run = if inlier { 0 } else { run + 1 };
        if run > MAX_CONSECUTIVE_EXCLUSIONS {
            result.reasons.push(RejectionReason::ConsecutiveExclusions);
            break;
        }
    }
}

fn check_coverage(result: &mut HypothesisDiagnostic, inliers: &[Point], duration: f64) {
    if inliers.len() < MIN_POSITIONS {
        result.reasons.push(RejectionReason::InsufficientPositions);
    }
    let first = inliers.first().expect("robust fit retains inliers").seconds;
    let last = inliers.last().expect("robust fit retains inliers").seconds;
    if first > duration * 0.2 || last < duration * 0.8 || last - first < duration * 0.6 {
        result
            .reasons
            .push(RejectionReason::InsufficientTemporalCoverage);
    }
}

fn window_diagnostics(
    points: &[Point],
    duration: f64,
    threshold: f64,
    halfwidth: f64,
    global: Line,
    denominator: f64,
) -> Vec<WindowDiagnostic> {
    (0..3)
        .map(|index| {
            let start_seconds = duration * index as f64 / 3.0;
            let end_seconds = duration * (index + 1) as f64 / 3.0;
            let local: Vec<_> = points
                .iter()
                .copied()
                .filter(|point| point.seconds >= start_seconds && point.seconds < end_seconds)
                .collect();
            let fit = robust_line(&local, &local, threshold, halfwidth);
            let residual = fit.as_ref().map(|(_, inliers)| {
                let mut residuals: Vec<_> = inliers
                    .iter()
                    .map(|&point| global.residual(point))
                    .collect();
                median(&mut residuals)
            });
            WindowDiagnostic {
                start_seconds,
                end_seconds,
                assigned_positions: local.len(),
                global_inlier_positions: local
                    .iter()
                    .filter(|&&point| global.residual(point).abs() <= threshold)
                    .count(),
                inlier_positions: fit.as_ref().map_or(0, |(_, inliers)| inliers.len()),
                period_seconds_per_quarter: fit.as_ref().map(|(line, _)| line.period * denominator),
                period_sensitivity_bound_seconds: fit
                    .as_ref()
                    .map(|(line, _)| line.sensitivity * denominator),
                median_global_residual_seconds: residual,
            }
        })
        .collect()
}

fn check_windows(
    result: &mut HypothesisDiagnostic,
    global: Line,
    threshold: f64,
    numerical_tolerance: f64,
    denominator: f64,
) -> f64 {
    let mut periods = Vec::with_capacity(3);
    let mut offsets = Vec::with_capacity(3);
    let mut inconsistent_periods = false;
    let mut insufficient_coverage = false;
    for window in &result.windows {
        if window.global_inlier_positions < MIN_WINDOW_POSITIONS
            || window.inlier_positions < MIN_WINDOW_POSITIONS
            || window.inlier_positions * 10 < window.assigned_positions * 9
        {
            insufficient_coverage = true;
        }
        if let (Some(period), Some(sensitivity), Some(offset)) = (
            window.period_seconds_per_quarter,
            window.period_sensitivity_bound_seconds,
            window.median_global_residual_seconds,
        ) {
            periods.push(period);
            offsets.push(offset);
            // Sensitivity is conditional on the supplied error bound and counts;
            // this consistency rule makes no independent-noise assumption.
            let numerical_period = numerical_tolerance * global.period * denominator
                / (window.end_seconds - window.start_seconds);
            if (period - global.period * denominator).abs()
                > sensitivity + global.sensitivity * denominator + 8.0 * numerical_period
            {
                inconsistent_periods = true;
            }
        } else {
            insufficient_coverage = true;
        }
    }
    if insufficient_coverage
        && !result
            .reasons
            .contains(&RejectionReason::InsufficientTemporalCoverage)
    {
        result
            .reasons
            .push(RejectionReason::InsufficientTemporalCoverage);
    }
    if inconsistent_periods {
        result
            .reasons
            .push(RejectionReason::InconsistentWindowPeriods);
    }
    let offset_min = offsets.iter().copied().fold(f64::INFINITY, f64::min);
    let offset_max = offsets.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !offsets.is_empty() && offset_max - offset_min > threshold + numerical_tolerance {
        result
            .reasons
            .push(RejectionReason::InconsistentWindowOffsets);
    }
    let min_period = periods.iter().copied().fold(f64::INFINITY, f64::min);
    let max_period = periods.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if periods.is_empty() {
        0.0
    } else {
        max_period - min_period
    }
}
