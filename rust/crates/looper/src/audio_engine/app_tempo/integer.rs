//! Finite integer/fractional checks on one genuine original-ordinal region.
//! Requested windows never establish coverage, tempo, map or phase authority.
use super::{RESIDUAL_SECONDS, RegionEstimate, line, region};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct FractionalCheck {
    pub status: &'static str,
    pub max_residual_seconds: Option<f64>,
    pub rms_residual_seconds: Option<f64>,
    pub rms_penalty_seconds: Option<f64>,
    pub window_phase_difference_seconds: Option<f64>,
}

impl FractionalCheck {
    fn unavailable() -> Self {
        Self {
            status: "inconclusive",
            max_residual_seconds: None,
            rms_residual_seconds: None,
            rms_penalty_seconds: None,
            window_phase_difference_seconds: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Counterprobe {
    pub status: &'static str,
    pub reason: &'static str,
    pub candidate_bpm: f64,
    pub base_bpm: f64,
    pub windows: Vec<RegionEstimate>,
    pub common_region: Option<RegionEstimate>,
    pub common_max_residual_seconds: Option<f64>,
    pub common_rms_residual_seconds: Option<f64>,
    pub rms_penalty_seconds: Option<f64>,
    pub window_phase_difference_seconds: Option<f64>,
    pub evidence_tier: &'static str,
    pub window_gap_seconds: Option<f64>,
    pub pivot_raw_index: Option<f64>,
    pub pivot_seconds: Option<f64>,
    pub fractional_check: FractionalCheck,
}

type Point = (usize, f64);

fn retained(beats: &[f64], common: &RegionEstimate) -> Vec<Point> {
    beats
        .iter()
        .copied()
        .enumerate()
        .filter(|(i, t)| {
            *t >= common.window_seconds[0]
                && *t < common.window_seconds[1]
                && common.excluded_raw_indices.binary_search(i).is_err()
        })
        .collect()
}

fn observed_span(points: &[Point], window: &RegionEstimate) -> Option<(usize, f64, f64, f64)> {
    let mut values = points
        .iter()
        .filter(|(_, t)| *t >= window.window_seconds[0] && *t < window.window_seconds[1]);
    let first = values.next()?.1;
    let mut last = first;
    let mut count = 1;
    for point in values {
        count += 1;
        last = point.1;
    }
    Some((count, last - first, first, last))
}

fn qualifies(
    beats: &[f64],
    points: &[Point],
    window: &RegionEstimate,
    count: usize,
    span: f64,
) -> bool {
    observed_span(points, window).is_some_and(|(n, s, _, _)| n >= count && s >= span)
        && window.retained >= count
        && window_span(window, beats) >= span
}

// Each stability fit retains its OWN original observations, not the intersection
// of local and common masks. Only the common mask controls candidate phase.
fn window_span(window: &RegionEstimate, beats: &[f64]) -> f64 {
    let mut retained = beats.iter().enumerate().filter(|(i, t)| {
        **t >= window.window_seconds[0]
            && **t < window.window_seconds[1]
            && window.excluded_raw_indices.binary_search(i).is_err()
    });
    let Some(first) = retained.next() else {
        return 0.0;
    };
    let last = retained.last().unwrap_or(first);
    last.1 - first.1
}

fn adaptive_windows(beats: &[f64], points: &[Point], duration: f64) -> Vec<RegionEstimate> {
    if points.len() < 8 {
        return Vec::new();
    }
    let split = points.len() / 2;
    let total = points.last().unwrap().1 - points[0].1;
    // Aim for a ten-second separation without sacrificing ten-second windows.
    // Actual retained endpoints, not these requested widths, decide the tier.
    let width = if total >= 30.0 {
        ((total - 10.0) / 2.0).min(16.0)
    } else {
        16.0
    };
    let halves = [&points[..split], &points[split..]];
    halves
        .iter()
        .enumerate()
        .filter_map(|(side, half)| {
            let first = half.first()?.1;
            let last = half.last()?.1;
            let (lower, upper) = if side == 0 {
                let mut end = half.partition_point(|p| p.1 <= first + width).max(1);
                if last - first >= 10.0 {
                    end = end
                        .max(half.partition_point(|p| p.1 < first + 10.0) + 1)
                        .min(half.len());
                }
                (first, half[end - 1].1.next_up().min(duration))
            } else {
                let mut start = half.partition_point(|p| p.1 < last - width);
                if last - first >= 10.0 {
                    start = start.min(
                        half.partition_point(|p| p.1 <= last - 10.0)
                            .saturating_sub(1),
                    );
                }
                (half[start].1, last.next_up().min(duration))
            };
            region(beats, lower, upper, 4, 1.0)
        })
        .collect()
}

fn evaluate(
    points: &[Point],
    windows: &[RegionEstimate],
    pivot: (f64, f64, f64),
    bpm: f64,
) -> FractionalCheck {
    let (mx, my, free_period) = pivot;
    let period = 60.0 / bpm;
    let mut maximum = 0.0_f64;
    let mut squared = 0.0;
    let mut free_squared = 0.0;
    for &(i, t) in points {
        let x = i as f64 - mx;
        let residual = t - my - period * x;
        maximum = maximum.max(residual.abs());
        squared += residual * residual;
        free_squared += (t - my - free_period * x).powi(2);
    }
    let rms = (squared / points.len() as f64).sqrt();
    let penalty = (rms - (free_squared / points.len() as f64).sqrt()).max(0.0);
    let offsets: Vec<_> = windows
        .iter()
        .map(|w| {
            let mut sum = 0.0;
            let mut count = 0;
            for &(i, t) in points {
                if t >= w.window_seconds[0] && t < w.window_seconds[1] {
                    sum += t - my - period * (i as f64 - mx);
                    count += 1;
                }
            }
            sum / count as f64
        })
        .collect();
    let difference = (offsets[1] - offsets[0]).abs();
    FractionalCheck {
        status: if maximum <= RESIDUAL_SECONDS.min(0.05 * period)
            && difference <= 0.005
            && penalty <= 0.00025
        {
            "pass"
        } else {
            "fail"
        },
        max_residual_seconds: Some(maximum),
        rms_residual_seconds: Some(rms),
        rms_penalty_seconds: Some(penalty),
        window_phase_difference_seconds: Some(difference),
    }
}

pub(super) fn probe(
    beats: &[f64],
    duration: f64,
    selected: &RegionEstimate,
    regions: &[RegionEstimate],
) -> Counterprobe {
    let base = selected.bpm;
    let candidate = base.round().clamp(30.0, 300.0);
    let lower = 0.2 * duration;
    let upper = 0.8 * duration;
    let common = regions
        .iter()
        .find(|r| r.window_seconds == [lower, upper])
        .or_else(|| {
            regions.iter().find(|r| {
                *r == selected
                    && (r.window_seconds == [0.0, duration]
                        || super::stable_runs::is_candidate(beats, duration, r.window_seconds))
            })
        })
        .cloned();
    let mut result = Counterprobe {
        status: "inconclusive",
        reason: "insufficient_probe_coverage",
        candidate_bpm: candidate,
        base_bpm: base,
        windows: Vec::new(),
        common_region: common,
        common_max_residual_seconds: None,
        common_rms_residual_seconds: None,
        rms_penalty_seconds: None,
        window_phase_difference_seconds: None,
        evidence_tier: "inconclusive",
        window_gap_seconds: None,
        pivot_raw_index: None,
        pivot_seconds: None,
        fractional_check: FractionalCheck::unavailable(),
    };
    let Some(common) = &result.common_region else {
        return result;
    };
    let points = retained(beats, common);
    let Some(pivot) = line(&points, &vec![true; points.len()]) else {
        return result;
    };
    result.pivot_raw_index = Some(pivot.0);
    result.pivot_seconds = Some(pivot.1);
    if common.window_seconds == [lower, upper] && upper - lower >= 42.0 {
        let old: Vec<_> = [[lower, lower + 16.0], [upper - 16.0, upper]]
            .into_iter()
            .filter_map(|w| region(beats, w[0], w[1], 8, 10.0))
            .collect();
        if old.len() == 2 && old.iter().all(|w| qualifies(beats, &points, w, 8, 10.0)) {
            result.windows = old;
        }
    }
    if result.windows.len() != 2 {
        result.windows = adaptive_windows(beats, &points, duration);
    }
    let ten = result.windows.len() == 2
        && result
            .windows
            .iter()
            .all(|w| qualifies(beats, &points, w, 8, 10.0));
    let whole = common.window_seconds == [0.0, duration]
        && common.retained >= 8
        && points
            .last()
            .zip(points.first())
            .is_some_and(|(last, first)| last.1 - first.1 >= 4.0);
    let short = !ten
        && whole
        && result.windows.len() == 2
        && result
            .windows
            .iter()
            .all(|w| qualifies(beats, &points, w, 4, 1.0));
    let gap = result
        .windows
        .first()
        .zip(result.windows.get(1))
        .and_then(|(a, b)| {
            let (_, _, _, end) = observed_span(&points, a)?;
            let (_, _, begin, _) = observed_span(&points, b)?;
            (a.window_seconds[1] <= b.window_seconds[0] && begin > end).then_some(begin - end)
        });
    if (!ten && !short) || gap.is_none() {
        result.windows.clear();
        return result;
    }
    result.evidence_tier = if ten {
        "two_ten_second"
    } else {
        "short_source"
    };
    result.window_gap_seconds = gap;
    let integer = evaluate(&points, &result.windows, pivot, candidate);
    result.common_max_residual_seconds = integer.max_residual_seconds;
    result.common_rms_residual_seconds = integer.rms_residual_seconds;
    result.rms_penalty_seconds = integer.rms_penalty_seconds;
    result.window_phase_difference_seconds = integer.window_phase_difference_seconds;
    result.fractional_check = evaluate(&points, &result.windows, pivot, base);
    if integer.status == "pass" && (candidate - base).abs() <= 0.005 {
        result.status = "supported_integer_estimate";
        result.reason = "two_windows_and_common_phase";
    } else if result.fractional_check.status == "pass" {
        result.status = "fractional_estimate";
        result.reason = if integer.status == "pass" {
            "fractional_preferred"
        } else {
            "long_span_mismatch"
        };
    } else {
        result.reason = "no_supported_candidate";
    }
    result
}

#[cfg(test)]
mod tests {
    use super::super::estimate;

    fn observations(bpm: f64, duration: f64, quantize: bool) -> Vec<f64> {
        (0..)
            .map(|i| 0.07 + i as f64 * 60.0 / bpm)
            .take_while(|t| *t < duration)
            .map(|t| {
                if quantize {
                    (t / 0.02).round() * 0.02
                } else {
                    t
                }
            })
            .take_while(|t| *t < duration)
            .collect()
    }

    #[test]
    fn exact_integer_with_detector_quantization_uses_two_windows_and_one_phase() {
        let beats = observations(94.0, 109.0, true);
        let before = beats.clone();
        let result = estimate(&beats, 109.0);
        assert_eq!(result.bpm, Some(94.0));
        let probe = result.integer_probe.unwrap();
        assert_eq!(probe.status, "supported_integer_estimate");
        assert_eq!(probe.windows.len(), 2);
        assert_eq!(probe.windows[0].window_seconds, [21.8, 37.8]);
        assert_eq!(probe.windows[1].window_seconds, [71.2, 87.2]);
        assert_eq!(beats, before);
        assert_eq!(probe.base_bpm, result.selected.unwrap().bpm);
    }

    #[test]
    fn genuine_fractional_and_distant_integer_drift_are_retained() {
        for bpm in [93.99, 91.887_123] {
            let result = estimate(&observations(bpm, 301.0, true), 301.0);
            assert!((result.bpm.unwrap() - bpm).abs() < 0.002);
            assert_ne!(
                result.integer_probe.unwrap().status,
                "supported_integer_estimate"
            );
        }
        // The reviewed 109-second counterexample must not be rounded to 94.
        let result = estimate(&observations(93.99, 109.0, true), 109.0);
        assert!((result.bpm.unwrap() - 93.99).abs() < 0.002);
        assert_eq!(result.integer_probe.unwrap().status, "fractional_estimate");
    }

    #[test]
    fn near_integer_remains_an_explicit_ambiguous_policy_preference() {
        let result = estimate(&observations(93.999, 109.0, false), 109.0);
        let probe = result.integer_probe.unwrap();
        assert_eq!(result.bpm, Some(94.0));
        assert!((probe.base_bpm - 93.999).abs() < 1e-10);
        assert_eq!(probe.status, "supported_integer_estimate");
        // Finite observations cannot prove exact integer tempo; retain the base.
        assert_ne!(probe.base_bpm, probe.candidate_bpm);
    }

    #[test]
    fn short_source_boundary_and_missing_middle_never_invent_coverage() {
        for duration in [18.0, 24.0, 40.0, 69.999, 70.0] {
            let result = estimate(&observations(94.0, duration, false), duration);
            let probe = result.integer_probe.unwrap();
            assert_eq!(result.bpm, Some(94.0));
            assert_eq!(probe.status, "supported_integer_estimate");
            assert_eq!(probe.fractional_check.status, "pass");
            assert_eq!(
                probe.evidence_tier,
                if duration == 18.0 {
                    "short_source"
                } else {
                    "two_ten_second"
                }
            );
            assert!(probe.window_gap_seconds.unwrap() > 0.0);
        }
        let result = estimate(&observations(94.0, 70.0, false), 70.0);
        assert_eq!(result.integer_probe.unwrap().windows.len(), 2);
        let mut beats = observations(94.0, 301.0, true);
        beats.retain(|t| !(120.0..140.0).contains(t));
        if let Some(probe) = estimate(&beats, 301.0).integer_probe {
            assert_ne!(probe.status, "supported_integer_estimate");
        }
    }

    #[test]
    fn local_rephasing_cannot_hide_phase_step_or_swing() {
        for swing in [false, true] {
            let mut beats = observations(94.0, 301.0, false);
            for (i, t) in beats.iter_mut().enumerate() {
                *t += if swing {
                    if i % 2 == 0 { 0.04 } else { -0.04 }
                } else if *t > 150.0 {
                    0.04
                } else {
                    0.0
                };
            }
            let result = estimate(&beats, 301.0);
            assert!(
                result
                    .integer_probe
                    .is_none_or(|p| p.status != "supported_integer_estimate")
            );
        }
    }

    #[test]
    fn local_only_exclusion_still_contributes_to_common_phase() {
        let mut beats = observations(94.0, 200.0, false);
        for (i, t) in beats.iter_mut().enumerate() {
            if !(40.0..56.0).contains(t) && !(144.0..160.0).contains(t) {
                *t += if i % 2 == 0 { 0.008 } else { -0.008 };
            }
        }
        let index = beats.iter().position(|t| (45.0..46.0).contains(t)).unwrap();
        beats[index] += 0.015;
        let common = super::region(&beats, 40.0, 160.0, 24, 30.0).unwrap();
        assert!(!common.excluded_raw_indices.contains(&index));
        let probe = super::probe(&beats, 200.0, &common, &[common.clone()]);
        assert!(probe.windows[0].excluded_raw_indices.contains(&index));
        let count = beats.iter().filter(|&&t| (40.0..56.0).contains(&t)).count();
        assert!(
            (probe.window_phase_difference_seconds.unwrap() - 0.015 / count as f64).abs() < 1e-12
        );
    }

    #[test]
    fn endpoint_masks_qualify_independently_without_an_intersection_span() {
        let beats: Vec<_> = (0..22).map(|i| i as f64 * 0.5).collect();
        let common = super::RegionEstimate {
            window_seconds: [0.0, 11.0],
            bpm: 120.0,
            retained: 21,
            excluded_raw_indices: vec![0],
            max_residual_seconds: 0.0,
            rms_residual_seconds: 0.0,
        };
        let local = super::RegionEstimate {
            excluded_raw_indices: vec![21],
            ..common.clone()
        };
        let points = super::retained(&beats, &common);
        assert_eq!(super::observed_span(&points, &local).unwrap().1, 10.0);
        assert_eq!(super::window_span(&local, &beats), 10.0);
        let intersection: Vec<_> = points.iter().filter(|(i, _)| *i != 21).collect();
        assert_eq!(
            intersection.last().unwrap().1 - intersection.first().unwrap().1,
            9.5
        );
        assert!(super::qualifies(&beats, &points, &local, 8, 10.0));
        assert!(
            points.iter().any(|(i, _)| *i == 21),
            "local endpoint remains in common phase"
        );
    }

    #[test]
    fn fractional_candidate_is_checked_at_its_exact_selected_period() {
        let beats = observations(94.0, 109.0, false);
        let common = super::region(&beats, 0.2 * 109.0, 0.8 * 109.0, 24, 30.0).unwrap();
        let mut selected = common.clone();
        selected.bpm = 94.02;
        let result = super::probe(&beats, 109.0, &selected, &[common, selected.clone()]);
        assert_eq!(result.fractional_check.status, "fail");
        assert_eq!(result.status, "inconclusive");
        assert_eq!(result.reason, "no_supported_candidate");
        assert!(result.common_max_residual_seconds.unwrap() < 1e-10);
        let exact = estimate(&observations(91.892, 109.0, false), 109.0);
        assert!((exact.bpm.unwrap() - 91.892).abs() < 1e-10);
        assert_eq!(exact.integer_probe.unwrap().fractional_check.status, "pass");
        let beats = observations(94.01, 301.0, false);
        let common = super::region(&beats, 0.2 * 301.0, 0.8 * 301.0, 24, 30.0).unwrap();
        let mut selected = common.clone();
        selected.bpm = 94.02;
        let neither = super::probe(&beats, 301.0, &selected, &[common, selected.clone()]);
        assert_eq!(neither.base_bpm, 94.02);
        assert!(neither.window_phase_difference_seconds.unwrap() > 0.005);
        assert_eq!(neither.fractional_check.status, "fail");
        assert_eq!(neither.status, "inconclusive");
        assert_eq!(neither.reason, "no_supported_candidate");
    }

    #[test]
    fn short_islands_in_a_long_original_are_not_short_source_evidence() {
        let beats = observations(94.0, 18.0, false);
        let result = estimate(&beats, 109.0);
        assert!(result.bpm.is_none());
        assert!(result.integer_probe.is_none());
        let tiny = estimate(&beats[..6], 4.0);
        assert!(tiny.bpm.is_none());
        assert!(tiny.integer_probe.is_none());
    }

    #[test]
    fn insufficient_probe_coverage_leaves_both_checks_inconclusive() {
        let beats = observations(94.0, 109.0, false);
        let selected = super::region(&beats, 0.0, 109.0 / 3.0, 24, 30.0).unwrap();
        let probe = super::probe(&beats, 109.0, &selected, &[selected.clone()]);
        assert_eq!(probe.status, "inconclusive");
        assert_eq!(probe.fractional_check.status, "inconclusive");
        assert!(probe.common_region.is_none());
        assert!(probe.windows.is_empty());
    }
}
