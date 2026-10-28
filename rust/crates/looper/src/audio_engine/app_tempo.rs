//! App tempo estimates, not scientific constant-timing or musical certificates.
//! The detector observations remain unchanged. All work runs off realtime.

use serde::Serialize;

pub(super) const POLICY: &str = "neural-stable-regions-v4";
const RESIDUAL_SECONDS: f64 = 0.025;
#[path = "app_tempo/inferred_ordinals.rs"]
mod inferred_ordinals;
#[path = "app_tempo/integer.rs"]
mod integer;
#[path = "app_tempo/stable_runs.rs"]
mod stable_runs;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct RegionEstimate {
    pub window_seconds: [f64; 2],
    pub bpm: f64,
    pub retained: usize,
    pub excluded_raw_indices: Vec<usize>,
    pub max_residual_seconds: f64,
    pub rms_residual_seconds: f64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TempoEstimate {
    pub policy: &'static str,
    pub status: &'static str,
    pub bpm: Option<f64>,
    pub selected: Option<RegionEstimate>,
    pub regions: Vec<RegionEstimate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integer_probe: Option<integer::Counterprobe>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordinal_fit: Option<inferred_ordinals::OrdinalFit>,
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_unstable_by(f64::total_cmp);
    let n = values.len();
    if n.is_multiple_of(2) {
        (values[n / 2 - 1] + values[n / 2]) * 0.5
    } else {
        values[n / 2]
    }
}

fn line(points: &[(usize, f64)], keep: &[bool]) -> Option<(f64, f64, f64)> {
    let count = keep.iter().filter(|&&yes| yes).count();
    if count < 2 {
        return None;
    }
    let n = count as f64;
    let mx = points
        .iter()
        .zip(keep)
        .filter(|(_, k)| **k)
        .map(|(p, _)| p.0 as f64)
        .sum::<f64>()
        / n;
    let my = points
        .iter()
        .zip(keep)
        .filter(|(_, k)| **k)
        .map(|(p, _)| p.1)
        .sum::<f64>()
        / n;
    let (xx, xy) =
        points
            .iter()
            .zip(keep)
            .filter(|(_, k)| **k)
            .fold((0.0, 0.0), |(xx, xy), (p, _)| {
                let x = p.0 as f64 - mx;
                (xx + x * x, xy + x * (p.1 - my))
            });
    let period = xy / xx;
    (period.is_finite() && (0.2..=2.0).contains(&period)).then_some((mx, my, period))
}

fn region(
    beats: &[f64],
    lower: f64,
    upper: f64,
    minimum_points: usize,
    minimum_span: f64,
) -> Option<RegionEstimate> {
    let points: Vec<_> = beats
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, t)| *t >= lower && *t < upper)
        .collect();
    if points.len() < minimum_points {
        return None;
    }
    let mut keep = vec![true; points.len()];
    // A bounded robust fit removes isolated model errors without moving retained
    // observations or renumbering their original detector indices.
    for _ in 0..5 {
        let (mx, my, period) = line(&points, &keep)?;
        let residual: Vec<_> = points
            .iter()
            .map(|&(i, t)| t - my - period * (i as f64 - mx))
            .collect();
        let mut retained: Vec<_> = residual
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(r, _)| *r)
            .collect();
        let center = median(&mut retained);
        let mut deviations: Vec<_> = retained.iter().map(|r| (r - center).abs()).collect();
        let limit = RESIDUAL_SECONDS.min(0.05 * period);
        let threshold = (3.5 * 1.4826 * median(&mut deviations)).clamp(0.010, limit);
        let next: Vec<_> = residual
            .iter()
            .map(|r| (r - center).abs() <= threshold)
            .collect();
        if next == keep {
            break;
        }
        keep = next;
    }
    let excluded: Vec<_> = points
        .iter()
        .zip(&keep)
        .filter(|(_, k)| !**k)
        .map(|(p, _)| p.0)
        .collect();
    if excluded.len() * 20 > points.len() || keep.windows(2).any(|k| !k[0] && !k[1]) {
        return None;
    }
    let retained: Vec<_> = points
        .iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .map(|(p, _)| *p)
        .collect();
    if retained.len() < minimum_points {
        return None;
    }
    let first = retained.first()?.1;
    let last = retained.last()?.1;
    if last - first < minimum_span
        || last - first < 0.6 * (upper - lower)
        || first > lower + 0.2 * (upper - lower)
        || last < upper - 0.2 * (upper - lower)
    {
        return None;
    }
    let (mx, my, period) = line(&points, &keep)?;
    let residual: Vec<_> = retained
        .iter()
        .map(|&(i, t)| t - my - period * (i as f64 - mx))
        .collect();
    let maximum = residual.iter().copied().map(f64::abs).fold(0.0, f64::max);
    if maximum > RESIDUAL_SECONDS.min(0.05 * period) {
        return None;
    }
    Some(RegionEstimate {
        window_seconds: [lower, upper],
        bpm: 60.0 / period,
        retained: retained.len(),
        excluded_raw_indices: excluded,
        max_residual_seconds: maximum,
        rms_residual_seconds: (residual.iter().map(|r| r * r).sum::<f64>() / residual.len() as f64)
            .sqrt(),
    })
}

pub(super) fn estimate(beats: &[f64], duration: f64) -> TempoEstimate {
    estimate_with_budget(beats, duration, 0, usize::MAX)
}

pub(super) fn estimate_with_budget(
    beats: &[f64],
    duration: f64,
    downbeats: usize,
    available_bytes: usize,
) -> TempoEstimate {
    estimate_with_budget_cancelled(beats, duration, downbeats, available_bytes, &|| false)
        .expect("non-cancelling tempo estimator")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Cancelled;

/// Recompute the narrow optional acoustic-corroboration gate without altering
/// the original estimate or granting its failed phase check a passing status.
pub(super) fn phase_only_period(
    beats: &[f64],
    duration: f64,
    estimate: &TempoEstimate,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<f64>, Cancelled> {
    let control = inferred_ordinals::Control::new(cancelled);
    if control.cancelled() {
        return Err(Cancelled);
    }
    if estimate.policy != inferred_ordinals::POLICY || estimate.bpm.is_some() {
        return Ok(None);
    }
    let Some(inferred_ordinals::OrdinalFit::Evaluated(evidence)) = &estimate.ordinal_fit else {
        return Ok(None);
    };
    inferred_ordinals::phase_only_period(beats, duration, evidence, &control)
}

pub(super) fn preparation_reservation(beats: usize, downbeats: usize) -> Option<usize> {
    inferred_ordinals::preparation_bytes(beats, downbeats)
}

pub(super) fn estimate_with_budget_cancelled(
    beats: &[f64],
    duration: f64,
    downbeats: usize,
    available_bytes: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<TempoEstimate, Cancelled> {
    let control = inferred_ordinals::Control::new(cancelled);
    if control.cancelled() {
        return Err(Cancelled);
    }
    let mut result = estimate_v4(beats, duration);
    if control.cancelled() {
        return Err(Cancelled);
    }
    if result.bpm.is_some() || result.status == "invalid" {
        return Ok(result);
    }
    if downbeats > 250_000 {
        result.status = "invalid";
        return Ok(result);
    }
    let required =
        inferred_ordinals::preparation_bytes(beats.len(), downbeats).unwrap_or(usize::MAX);
    let (bpm, fit) =
        inferred_ordinals::recover(beats, duration, required, available_bytes, &control)?;
    result.policy = inferred_ordinals::POLICY;
    result.bpm = bpm;
    if bpm.is_some() {
        result.status = if duration < 30.0 {
            "single_region_estimate"
        } else {
            "estimated"
        };
    }
    result.ordinal_fit = Some(fit);
    if control.cancelled() {
        return Err(Cancelled);
    }
    Ok(result)
}

fn estimate_v4(beats: &[f64], duration: f64) -> TempoEstimate {
    let mut result = TempoEstimate {
        policy: POLICY,
        status: "insufficient",
        bpm: None,
        selected: None,
        regions: Vec::new(),
        integer_probe: None,
        ordinal_fit: None,
    };
    if !duration.is_finite()
        || duration <= 0.0
        || beats.len() > 250_000
        || beats
            .iter()
            .any(|t| !t.is_finite() || *t < 0.0 || *t >= duration)
        || beats.windows(2).any(|pair| pair[0] >= pair[1])
    {
        result.status = "invalid";
        return result;
    }
    let windows = [
        [0.2 * duration, 0.8 * duration],
        [0.0, duration / 3.0],
        [duration / 3.0, 2.0 * duration / 3.0],
        [2.0 * duration / 3.0, duration],
    ];
    result.regions = windows
        .into_iter()
        .filter_map(|w| region(beats, w[0], w[1], 24, 30.0))
        .collect();
    // The corroborating region must extend beyond the selected window. A central
    // third wholly inside the middle is not independent coverage.
    let selected = result
        .regions
        .iter()
        .find(|candidate| {
            result.regions.iter().any(|other| {
                ((other.window_seconds[0] < candidate.window_seconds[0]
                    && other.window_seconds[1] < candidate.window_seconds[1])
                    || (other.window_seconds[0] > candidate.window_seconds[0]
                        && other.window_seconds[1] > candidate.window_seconds[1]))
                    && (other.bpm / candidate.bpm - 1.0).abs() <= 0.00025
            })
        })
        .cloned();
    if let Some(selected) = selected {
        result.status = "estimated";
        result.bpm = Some(selected.bpm);
        result.selected = Some(selected);
    } else if let Some(whole) = region(beats, 0.0, duration, 8, 4.0) {
        // One whole-source fit is useful for short clips whose thirds cannot
        // meet the long-region requirements. It does not claim independent
        // regional corroboration, and has no arbitrary duration discontinuity.
        result.status = "single_region_estimate";
        result.bpm = Some(whole.bpm);
        result.selected = Some(whole.clone());
        result.regions.push(whole);
    } else if !result.regions.is_empty() {
        result.status = "uncorroborated";
    }
    if result.bpm.is_none() {
        stable_runs::recover(beats, duration, &mut result);
    }
    if result.bpm.is_some() {
        let probe = integer::probe(
            beats,
            duration,
            result
                .selected
                .as_ref()
                .expect("a base owns its selected fit"),
            &result.regions,
        );
        if probe.status == "supported_integer_estimate" {
            result.bpm = Some(probe.candidate_bpm);
        }
        result.integer_probe = Some(probe);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantized_fractional_tempo_rejects_bad_edges_and_is_not_integer_snapped() {
        let period = 60.0 / 91.887_123;
        let mut beats: Vec<_> = (0..460)
            .map(|i| ((0.07 + i as f64 * period) / 0.02).round() * 0.02)
            .collect();
        beats[0] += 0.11;
        beats[459] -= 0.13;
        let estimate = estimate(&beats, 301.0);
        assert_eq!(estimate.status, "estimated");
        assert!((estimate.bpm.unwrap() - 91.887_123).abs() < 0.002);
        assert_eq!(beats[0], 0.19); // Raw observations were not rewritten.
    }

    #[test]
    fn isolated_errors_can_be_excluded_but_changing_tempo_is_not_constant() {
        let mut beats: Vec<_> = (0..420).map(|i| 0.05 + i as f64 * 0.65).collect();
        beats[150] += 0.055;
        let fitted = estimate(&beats, 274.0);
        assert_eq!(fitted.status, "estimated");
        assert!(fitted.selected.unwrap().excluded_raw_indices.contains(&150));
        let changing: Vec<_> = (0..420)
            .map(|i| 0.05 + 0.65 * i as f64 + 0.0001 * (i as f64).powi(2))
            .collect();
        assert!(estimate(&changing, 292.0).bpm.is_none());
        assert_eq!(
            estimate(&beats[..20], 14.0).status,
            "single_region_estimate"
        );
        assert!(estimate(&beats[..6], 4.0).bpm.is_none());
    }

    #[test]
    fn malformed_and_repeated_positions_fail_without_tempo() {
        for beats in [vec![0.2, 0.2], vec![0.2, f64::NAN], vec![-0.1, 0.5]] {
            assert_eq!(estimate(&beats, 100.0).status, "invalid");
        }
    }

    #[test]
    fn constant_short_sources_do_not_lose_bpm_at_region_duration_boundaries() {
        for duration in [49.0, 50.0, 60.0, 89.0, 90.0, 108.0, 121.0] {
            let beats: Vec<_> = (0..)
                .map(|i| 0.1 + i as f64 * 0.5)
                .take_while(|t| *t < duration)
                .collect();
            let estimate = estimate(&beats, duration);
            assert!(
                (estimate.bpm.unwrap() - 120.0).abs() < 1e-10,
                "{duration}: {estimate:?}"
            );
        }
    }

    #[test]
    fn fast_tracks_keep_the_period_relative_residual_limit() {
        let beats: Vec<_> = (0..600)
            .map(|i| 0.05 + i as f64 * 0.25 + if i % 2 == 0 { 0.016 } else { -0.016 })
            .collect();
        assert!(estimate(&beats, 151.0).bpm.is_none());
    }

    #[test]
    fn reversing_nested_middle_and_central_does_not_invent_corroboration() {
        let beats: Vec<_> = (0..600)
            .map(|i| {
                let t = 0.1 + i as f64 * 0.5;
                t + if !(60.0..240.0).contains(&t) {
                    if i % 2 == 0 { 0.06 } else { -0.06 }
                } else {
                    0.0
                }
            })
            .collect();
        let result = estimate(&beats, 300.0);
        assert_eq!(result.regions.len(), 2);
        assert_eq!(result.status, "uncorroborated");
        assert!(result.bpm.is_none());
    }

    #[test]
    #[ignore = "explicit local previously acquired neural observations; numeric comparison only"]
    fn retained_neural_observation_comparison() {
        let path =
            std::env::var("FLITZI_NEURAL_OBSERVATIONS").expect("explicit owned numeric input");
        let data: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for pad in data["pads"].as_array().unwrap() {
            let beats: Vec<_> = pad["beat_seconds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let fit = estimate(&beats, pad["duration_s"].as_f64().unwrap());
            println!(
                "NEURAL_TEMPO pad={} {}",
                pad["pad_id"],
                serde_json::to_string(&fit).unwrap()
            );
            assert_eq!(fit.status, "estimated");
            if let Some(expected) = pad["expected_bpm"].as_f64() {
                let actual = fit.bpm.unwrap();
                if expected.fract() == 0.0 {
                    assert_eq!(actual, expected);
                } else {
                    // Fixture JSON parsing can differ by one binary64 ULP;
                    // this is a numeric comparison, not a source identity seal.
                    let (mut lower, mut upper) = (expected, expected);
                    for _ in 0..4 {
                        lower = lower.next_down();
                        upper = upper.next_up();
                    }
                    assert!((lower..=upper).contains(&actual));
                    assert_eq!(actual, fit.selected.as_ref().unwrap().bpm);
                }
            }
            if let Some(expected) = pad["expected_integer_status"].as_str() {
                assert_eq!(fit.integer_probe.as_ref().unwrap().status, expected);
            }
        }
    }
}
