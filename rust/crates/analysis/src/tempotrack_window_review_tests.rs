//! Each RCF observation is local; temporal smoothing belongs to Viterbi.
//!
//! Primary QM-DSP source:
//! https://raw.githubusercontent.com/c4dm/qm-dsp/master/dsp/tempotracking/TempoTrackV2.cpp
//! calculateBeatPeriod lines 143-149 constructs a new zero-initialized RCF for
//! every window; get_rcf lines 162-210 computes that window's observation.
//! Register this file as a test-only child of `tempotrack`.

use super::TempoTrackV2;

const EPSILON: f64 = 0.0000008;

fn rayleigh_weights() -> Vec<f64> {
    let parameter: f64 = (60.0 * 44_100.0 / 512.0) / 120.0;
    (0..128)
        .map(|period| {
            let x = period as f64;
            x / parameter.powi(2) * (-x.powi(2) / (2.0 * parameter.powi(2))).exp()
        })
        .collect()
}

fn local_threshold(input: &[f64]) -> Vec<f64> {
    input
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let start = index.saturating_sub(8);
            let end = (index + 7).min(input.len() - 1);
            let mean = input[start..=end].iter().sum::<f64>() / (end - start + 1) as f64;
            (value - mean).max(0.0)
        })
        .collect()
}

/// Mathematical local observation, with no inherited output accumulator.
fn independent_window_bank(frame: &[f64], weights: &[f64]) -> Vec<f64> {
    let thresholded = local_threshold(frame);
    let correlations: Vec<f64> = (0..frame.len())
        .map(|lag| {
            thresholded[..frame.len() - lag]
                .iter()
                .zip(&thresholded[lag..])
                .map(|(left, right)| left * right)
                .sum::<f64>()
                / (frame.len() - lag) as f64
        })
        .collect();
    let mut scores = vec![0.0; 128];
    for candidate in 1..127 {
        let period = candidate + 1;
        let mut score = 0.0;
        for harmonic in 1..=4_isize {
            for displacement in 1 - harmonic..harmonic {
                let lag = harmonic * period as isize + displacement - 1;
                if let Some(correlation) = correlations.get(lag as usize) {
                    score += correlation * weights[candidate] / (2 * harmonic - 1) as f64;
                }
            }
        }
        scores[candidate] = score;
    }
    let mut probabilities = local_threshold(&scores);
    for probability in &mut probabilities {
        *probability += EPSILON;
    }
    let denominator = probabilities.iter().sum::<f64>() + EPSILON;
    probabilities
        .into_iter()
        .map(|probability| probability / denominator)
        .collect()
}

#[test]
fn rcf_current_silent_window_does_not_inherit_a_previous_rhythm() {
    let tracker = TempoTrackV2::new(44_100.0, 512.0 / 44_100.0);
    let weights = rayleigh_weights();
    let current = vec![0.0; 512];
    let expected = independent_window_bank(&current, &weights);
    for previous_period in [30, 43, 61] {
        let mut previous = vec![0.0; 512];
        for onset in (0..512).step_by(previous_period) {
            previous[onset] = 1.0;
        }
        let mut actual = vec![0.0; 128];
        tracker.get_rcf(&previous, &weights, &mut actual);
        tracker.get_rcf(&current, &weights, &mut actual);
        let maximum_error = actual
            .iter()
            .zip(&expected)
            .map(|(actual, expected)| (actual - expected).abs())
            .fold(0.0_f64, f64::max);
        assert!(
            maximum_error < 1e-12,
            "previous period {previous_period} contaminated the current local bank by {maximum_error}"
        );
    }
}

#[test]
fn beat_period_pipeline_uses_fresh_observations_before_viterbi_smoothing() {
    let tracker = TempoTrackV2::new(44_100.0, 512.0 / 44_100.0);
    let weights = rayleigh_weights();
    let mut odf = vec![0.0; 4096];
    for (start, end, period, amplitude) in [
        (0, 1408, 43, 1.0),
        (1408, 2816, 30, 0.08),
        (2816, 4096, 61, 0.02),
    ] {
        for onset in (start..end).step_by(period) {
            odf[onset] = amplitude;
        }
    }

    let observations: Vec<Vec<f64>> = (-256..odf.len() as isize - 256)
        .step_by(128)
        .map(|start| {
            // Explicit zero extension is independent of the production frame
            // copy code, while preserving its existing centered-window policy.
            let frame: Vec<f64> = (0..512)
                .map(|offset| {
                    let source = start + offset;
                    usize::try_from(source)
                        .ok()
                        .and_then(|index| odf.get(index))
                        .copied()
                        .unwrap_or(0.0)
                })
                .collect();
            independent_window_bank(&frame, &weights)
        })
        .collect();
    let mut expected = Vec::new();
    // Reuse the real temporal decoder so intentional HMM history is preserved;
    // the independent reference replaces only the local observation source.
    tracker.viterbi_decode(&observations, &weights, &mut expected);
    assert_eq!(expected.len(), 32);
    assert!(expected.windows(2).any(|pair| pair[0] != pair[1]));
    let mut actual = Vec::new();
    tracker.calculate_beat_period(&odf, &mut actual, 120.0, false);
    assert_eq!(actual, expected);
}

#[test]
fn short_odf_tail_attacks_participate_in_both_centered_windows() {
    let tracker = TempoTrackV2::new(44_100.0, 512.0 / 44_100.0);
    let weights = rayleigh_weights();
    let mut odf = vec![0.0; 256];
    for onset in [140, 170, 200, 230] {
        odf[onset] = 1.0;
    }
    // Both centered 512-frame windows intersect the complete short source.
    // In particular, the source's latter half is data, not right-side padding.
    let observations: Vec<Vec<f64>> = [-256_isize, -128]
        .into_iter()
        .map(|start| {
            let frame: Vec<f64> = (0..512)
                .map(|offset| {
                    usize::try_from(start + offset)
                        .ok()
                        .and_then(|source| odf.get(source))
                        .copied()
                        .unwrap_or(0.0)
                })
                .collect();
            assert_eq!(frame.iter().filter(|value| **value != 0.0).count(), 4);
            independent_window_bank(&frame, &weights)
        })
        .collect();
    let mut expected = Vec::new();
    tracker.viterbi_decode(&observations, &weights, &mut expected);
    assert_eq!(expected.len(), 2);
    let mut actual = Vec::new();
    tracker.calculate_beat_period(&odf, &mut actual, 120.0, false);
    assert_eq!(
        actual, expected,
        "valid attacks in the short source's tail must inform both centered observations"
    );
}
