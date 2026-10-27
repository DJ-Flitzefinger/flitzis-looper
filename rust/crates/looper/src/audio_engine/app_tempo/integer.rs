//! A finite integer prior, not a certificate that the source has exact DAW BPM.
//! Short windows share the original event ordinals and one long-span phase.
use super::{RESIDUAL_SECONDS, RegionEstimate, line, region};
use serde::Serialize;

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
}

pub(super) fn probe(
    beats: &[f64],
    duration: f64,
    base: f64,
    regions: &[RegionEstimate],
) -> Counterprobe {
    let candidate = base.round().clamp(30.0, 300.0);
    let mut result = Counterprobe {
        status: "inconclusive",
        reason: "insufficient_inner_coverage",
        candidate_bpm: candidate,
        base_bpm: base,
        windows: Vec::new(),
        common_region: None,
        common_max_residual_seconds: None,
        common_rms_residual_seconds: None,
        rms_penalty_seconds: None,
        window_phase_difference_seconds: None,
    };
    let lower = 0.2 * duration;
    let upper = 0.8 * duration;
    // Two sixteen-second windows with at least ten seconds BETWEEN them. At
    // least ten seconds of actual retained beats, rather than file length alone.
    if upper - lower < 42.0 {
        return result;
    }
    result.windows = [[lower, lower + 16.0], [upper - 16.0, upper]]
        .into_iter()
        .filter_map(|w| region(beats, w[0], w[1], 8, 10.0))
        .collect();
    result.common_region = regions
        .iter()
        .find(|r| r.window_seconds == [lower, upper])
        .cloned();
    // Only an independently fitted, candidate-neutral mask may remove events.
    // If that fit failed, unmasked data can reject an integer but cannot adopt it.
    let excluded = result
        .common_region
        .as_ref()
        .map_or(&[][..], |r| r.excluded_raw_indices.as_slice());
    let points: Vec<_> = beats
        .iter()
        .copied()
        .enumerate()
        .filter(|(i, t)| *t >= lower && *t < upper && excluded.binary_search(i).is_err())
        .collect();
    let Some((mx, my, free_period)) = line(&points, &vec![true; points.len()]) else {
        return result;
    };
    let period = 60.0 / candidate;
    let mut maximum = 0.0_f64;
    let mut squared = 0.0;
    let mut free_squared = 0.0;
    for &(i, t) in &points {
        let x = i as f64 - mx;
        let residual = t - my - period * x;
        maximum = maximum.max(residual.abs());
        squared += residual * residual;
        free_squared += (t - my - free_period * x).powi(2);
    }
    let rms = (squared / points.len() as f64).sqrt();
    let penalty = (rms - (free_squared / points.len() as f64).sqrt()).max(0.0);
    result.common_max_residual_seconds = Some(maximum);
    result.common_rms_residual_seconds = Some(rms);
    result.rms_penalty_seconds = Some(penalty);
    if result.windows.len() == 2 {
        let offsets: Vec<_> = result
            .windows
            .iter()
            .map(|w| {
                let mut sum = 0.0;
                let mut count = 0;
                let mut first = None;
                let mut last = 0.0;
                for &(i, t) in &points {
                    if t >= w.window_seconds[0] && t < w.window_seconds[1] {
                        sum += t - my - period * (i as f64 - mx);
                        count += 1;
                        first.get_or_insert(t);
                        last = t;
                    }
                }
                (count >= 8 && first.is_some_and(|t| last - t >= 10.0)).then(|| sum / count as f64)
            })
            .collect();
        if let [Some(a), Some(b)] = offsets.as_slice() {
            result.window_phase_difference_seconds = Some((b - a).abs());
        }
    }
    if result.common_region.is_none()
        || result.windows.len() != 2
        || result.window_phase_difference_seconds.is_none()
    {
        // Residual diagnostics remain useful, but missing stable coverage cannot
        // become either an adoption or a claimed data rejection.
        return result;
    }
    if maximum > RESIDUAL_SECONDS.min(0.05 * period)
        || result
            .window_phase_difference_seconds
            .is_some_and(|v| v > 0.005)
        || penalty > 0.00025
    {
        result.status = "fractional_estimate";
        result.reason = "long_span_mismatch";
    } else if (candidate - base).abs() > 0.005 {
        result.status = "fractional_estimate";
        result.reason = "fractional_preferred";
    } else if result.common_region.is_some()
        && result.windows.len() == 2
        && result.window_phase_difference_seconds.is_some()
    {
        result.status = "supported_integer_estimate";
        result.reason = "two_windows_and_common_phase";
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
        for duration in [18.0, 69.999] {
            let result = estimate(&observations(94.0, duration, false), duration);
            assert_eq!(result.integer_probe.unwrap().status, "inconclusive");
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
        let probe = super::probe(&beats, 200.0, common.bpm, &[common]);
        assert!(probe.windows[0].excluded_raw_indices.contains(&index));
        let count = beats.iter().filter(|&&t| (40.0..56.0).contains(&t)).count();
        assert!(
            (probe.window_phase_difference_seconds.unwrap() - 0.015 / count as f64).abs() < 1e-12
        );
    }
}
