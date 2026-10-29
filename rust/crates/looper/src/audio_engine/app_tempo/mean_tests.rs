use super::*;

fn pulse(duration: f64, period: f64) -> Vec<f64> {
    (0..)
        .map(|i| 0.1 + i as f64 * period)
        .take_while(|t| *t < duration)
        .collect()
}

fn synthetic_mean(periods: [f64; 3]) -> InteriorMean {
    let regions = periods
        .into_iter()
        .enumerate()
        .map(|(i, p)| RegionEstimate {
            window_seconds: [60.0 * (i + 1) as f64, 60.0 * (i + 2) as f64],
            bpm: 60.0 / p,
            retained: 100,
            excluded_raw_indices: vec![],
            max_residual_seconds: 0.0,
            rms_residual_seconds: 0.0,
        })
        .collect();
    InteriorMean {
        regions,
        bpm: periods.into_iter().map(|p| 60.0 / p).sum::<f64>() / 3.0,
    }
}

#[test]
fn six_fresh_regions_find_a_mean_without_repairing_the_constant_estimate() {
    let duration = 300.0;
    let mut beats = Vec::new();
    // Five individually stable sections have different precise periods. The
    // silent joins are real observed gaps, not renumbered constant-line masks.
    for section in 0..5 {
        let start = section as f64 * 60.0 + 0.1;
        let period = 0.5 + section as f64 * 0.0005;
        beats.extend(
            (0..)
                .map(|i| start + i as f64 * period)
                .take_while(|t| *t < start + 55.0),
        );
    }
    let original = beats.clone();
    let precise = super::super::estimate(&beats, duration);
    assert!(precise.bpm.is_none());
    assert!(!super::super::optional_resource_refusal(&precise));
    let mean = interior(&beats, duration, usize::MAX, &|| false)
        .unwrap()
        .unwrap();
    let expected = (60.0 / 0.5005 + 60.0 / 0.501 + 60.0 / 0.5015) / 3.0;
    assert!((mean.bpm - expected).abs() < 1e-10);
    assert_eq!(
        agreement(&mean, &mean),
        Some((
            1.0,
            mean.regions
                .iter()
                .map(|r| 60.0 / r.bpm)
                .fold(f64::NEG_INFINITY, f64::max)
                / mean
                    .regions
                    .iter()
                    .map(|r| 60.0 / r.bpm)
                    .fold(f64::INFINITY, f64::min)
        ))
    );
    assert_eq!(beats, original);
    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for (i, r) in mean.regions.iter().enumerate() {
        let left = (i + 1) as f64 * duration / 5.0;
        let right = (i + 2) as f64 * duration / 5.0;
        assert_eq!(r.window_seconds, [left, right]);
        numerator += (right - left) * r.bpm;
        denominator += right - left;
    }
    assert_eq!(mean.bpm.to_bits(), (numerator / denominator).to_bits());
}

#[test]
fn excluded_edges_and_unrelated_global_rivals_do_not_control_the_interior_policy() {
    let mut beats = pulse(300.0, 0.5);
    for (i, t) in beats.iter_mut().enumerate() {
        if !(60.0..240.0).contains(t) {
            *t += if i % 2 == 0 { 0.09 } else { -0.09 };
        }
    }
    let mean = interior(&beats, 300.0, usize::MAX, &|| false)
        .unwrap()
        .unwrap();
    // The ordinary fit and weighted mean keep their binary64 rounding; this
    // synthetic mathematical constant is not a request to snap publication.
    assert!((mean.bpm - 120.0).abs() < 1e-10);
    // The independent local proof consumes no inferred pool/basis or rival rows.
    let (factor, ratio) = agreement(&mean, &mean).unwrap();
    assert_eq!(factor, 1.0);
    assert!((ratio - 1.0).abs() < 1e-12);
}

#[test]
fn missing_subdivision_and_short_local_support_are_not_averaged() {
    let beats = pulse(300.0, 0.5);
    let mut missing = beats.clone();
    missing.remove(300);
    let mut extra = beats.clone();
    extra.insert(301, 150.35);
    for altered in [missing, extra] {
        assert!(
            interior(&altered, 300.0, usize::MAX, &|| false)
                .unwrap()
                .is_none()
        );
    }
    assert!(
        interior(&pulse(149.0, 0.5), 149.0, usize::MAX, &|| false)
            .unwrap()
            .is_none()
    );
}

#[test]
fn one_global_factor_and_direct_ratio_boundary_have_no_per_section_unit_choice() {
    let neural = synthetic_mean([0.5; 3]);
    let qm = synthetic_mean([1.0; 3]);
    assert_eq!(agreement(&neural, &qm), Some((2.0, 1.0)));
    assert!(agreement(&neural, &synthetic_mean([1.0, 0.5, 1.0])).is_none());
    let exact = synthetic_mean([1.0, 1.0, 1.01]);
    let ratio = exact
        .regions
        .iter()
        .map(|r| 60.0 / r.bpm)
        .fold(f64::NEG_INFINITY, f64::max)
        / exact
            .regions
            .iter()
            .map(|r| 60.0 / r.bpm)
            .fold(f64::INFINITY, f64::min);
    assert_eq!(ratio, 1.01);
    assert_eq!(agreement(&exact, &exact), Some((1.0, 1.01)));
    let outside = synthetic_mean([1.0, 1.0, 1.01_f64.next_up()]);
    assert!(agreement(&outside, &outside).is_none());
}

#[test]
fn local_preparation_refusal_and_latched_cancellation_are_distinct() {
    let beats = pulse(300.0, 0.5);
    let required = preparation_bytes(beats.len()).unwrap();
    assert_eq!(
        interior(&beats, 300.0, required - 1, &|| false).unwrap_err(),
        Failure::Budget
    );
    assert_eq!(
        interior(&beats, 300.0, 0, &|| true).unwrap_err(),
        Failure::Cancelled
    );
    assert!(preparation_bytes(usize::MAX).is_none());
    let checks = std::cell::Cell::new(0);
    let cancelled = || {
        checks.set(checks.get() + 1);
        checks.get() > 8
    };
    assert_eq!(
        interior(&beats, 300.0, required, &cancelled).unwrap_err(),
        Failure::Cancelled
    );
}
