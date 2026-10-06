//! Behavioral checks against independently constructed source/count truth.
//!
//! These are offline numerical fixtures, not detector, PCM-refinement or musical
//! acceptance tests. No fixture obtains its expected tempo from the fitter.

use flitzis_looper_analysis::tempo_summary::{
    MAX_HYPOTHESES, MAX_RAW_POSITIONS, POLICY_VERSION, QuarterNoteHypothesis,
    QuarterNoteVerification, RawTempoEvidence, SourceIdentity, SummaryStatus, TempoSummary,
    summarize_constant_tempo,
};

const RATE: u32 = 48_000;
const COUNT: usize = 1_200;

fn source(rate: u32, duration_seconds: f64, halfwidth_seconds: f64) -> SourceIdentity {
    SourceIdentity {
        source_sha256: "1".repeat(64),
        pcm_sha256: "2".repeat(64),
        loaded_sample_rate_hz: rate,
        loaded_frame_count: (duration_seconds * f64::from(rate)).round() as u64,
        backend_revision: "independent-synthetic-events-v1".into(),
        configuration_revision: "source-zero-no-preprocessing-v1".into(),
        raw_revision: "immutable-raw-fixture-v1".into(),
        timing_error_halfwidth_seconds: halfwidth_seconds,
    }
}

fn standard_source() -> SourceIdentity {
    source(RATE, 601.0, 0.5 / f64::from(RATE))
}

fn consecutive_counts(length: usize) -> Vec<Option<i64>> {
    (0..length).map(|index| Some(index as i64)).collect()
}

fn hypothesis<'a>(
    id: &'a str,
    counts: &'a [Option<i64>],
    verification: QuarterNoteVerification,
) -> QuarterNoteHypothesis<'a> {
    QuarterNoteHypothesis {
        id,
        provenance: "explicit-independent-fixture-quarter-counts-v1",
        verification,
        quarter_counts: counts,
        quarter_note_denominator: 1,
    }
}

fn summarize(identity: &SourceIdentity, times: &[f64], counts: &[Option<i64>]) -> TempoSummary {
    summarize_constant_tempo(
        &RawTempoEvidence {
            source: identity,
            beat_seconds: times,
            independent_origin_seconds: 0.0,
        },
        &[hypothesis(
            "fixture-quarter-notes",
            counts,
            QuarterNoteVerification::Verified,
        )],
    )
    .expect("the complete fixture is structurally valid")
}

fn supported_period(summary: &TempoSummary) -> f64 {
    assert_eq!(
        summary.status,
        SummaryStatus::SupportedCandidate,
        "{summary:#?}"
    );
    summary.hypotheses[summary.supported_hypothesis_index.unwrap()]
        .fit
        .as_ref()
        .unwrap()
        .period_seconds_per_quarter
}

fn assert_unsupported(summary: &TempoSummary) {
    assert_eq!(summary.status, SummaryStatus::Unsupported, "{summary:#?}");
    assert_eq!(summary.supported_hypothesis_index, None);
}

#[test]
fn exact_and_fractional_tempos_retain_independent_source_periods_at_loaded_rates() {
    for rate in [44_100, 48_000, 96_000] {
        for bpm in [120.0, 119.999, 123.45] {
            let period = 60.0 / bpm;
            let times: Vec<_> = (0..COUNT).map(|index| index as f64 * period).collect();
            let identity = source(rate, 601.0, 0.5 / f64::from(rate));
            let summary = summarize(&identity, &times, &consecutive_counts(COUNT));
            let actual = supported_period(&summary);
            let measured_slope_error_frames =
                (actual - period).abs() * (COUNT - 1) as f64 * f64::from(rate);
            assert!(measured_slope_error_frames < 1e-4, "{bpm}: {actual}");
            assert!((60.0 / actual - bpm).abs() < 1e-10);
            assert_eq!(summary.policy_version, POLICY_VERSION);
            assert_eq!(summary.raw_position_count, COUNT);
            assert_eq!(summary.raw_beat_seconds, times);
            assert_eq!(summary.hypotheses[0].inlier_raw_indices.len(), COUNT);
            assert_eq!(summary.hypotheses[0].windows.len(), 3);
            let bound = summary.hypotheses[0]
                .fit
                .as_ref()
                .unwrap()
                .period_sensitivity_bound_seconds;
            assert!(bound.is_finite() && bound > 0.0);
            assert!((actual - period).abs() <= bound);
        }
    }
}

#[test]
fn all_positions_reduce_quantized_endpoint_bias_without_claiming_sample_certainty() {
    let hop_seconds = 512.0 / 44_100.0;
    // Independent source truth is n*24000 frames at 48 kHz. The raw detector
    // fixture rounds every event to its own 512/44100-s lattice, omitting zero.
    let times: Vec<_> = (1..COUNT)
        .map(|quarter| (quarter as f64 * 0.5 / hop_seconds).round() * hop_seconds)
        .collect();
    let counts: Vec<_> = (1..COUNT).map(|quarter| Some(quarter as i64)).collect();
    let identity = source(RATE, 600.0, hop_seconds / 2.0);
    let summary = summarize(&identity, &times, &counts);
    let actual = supported_period(&summary);
    let measured_quarters = (COUNT - 2) as f64;
    let endpoint_period = (times[times.len() - 1] - times[0]) / measured_quarters;
    let fit_error = (actual - 0.5).abs() * measured_quarters * f64::from(RATE);
    let endpoint_error = (endpoint_period - 0.5).abs() * measured_quarters * f64::from(RATE);
    assert!(
        endpoint_error > 100.0,
        "fixture must reproduce endpoint bias"
    );
    assert!(
        fit_error <= 1.0,
        "measured-span fitted slope error: {fit_error} frames"
    );
    assert!(fit_error < endpoint_error / 100.0);
    let uncertainty = summary.hypotheses[0]
        .fit
        .as_ref()
        .unwrap()
        .period_sensitivity_bound_seconds
        * measured_quarters
        * f64::from(RATE);
    assert!(
        uncertainty > 1.0,
        "a coarse lattice is not exact PCM evidence"
    );
    // The 600-s grid position is extrapolation; it is distinct from measured
    // n=1..1199 evidence and receives no pulse-accuracy assertion here.
    assert!((1200.0 * actual).is_finite());
}

#[test]
fn missing_events_require_explicit_nonconsecutive_quarter_counts() {
    let quarters: Vec<_> = (0..COUNT).filter(|quarter| quarter % 37 != 18).collect();
    let times: Vec<_> = quarters
        .iter()
        .map(|quarter| *quarter as f64 * 0.5)
        .collect();
    let correct: Vec<_> = quarters
        .iter()
        .map(|quarter| Some(*quarter as i64))
        .collect();
    let summary = summarize(&standard_source(), &times, &correct);
    assert!((supported_period(&summary) - 0.5).abs() < 1e-12);
    assert_eq!(summary.hypotheses[0].quarter_counts, correct);
    assert!(summary.hypotheses[0].excluded_raw_indices.is_empty());
    assert_unsupported(&summarize(
        &standard_source(),
        &times,
        &consecutive_counts(times.len()),
    ));
}

#[test]
fn extra_events_remain_in_raw_evidence_and_need_explicit_exclusions() {
    let mut times = Vec::new();
    let mut counts = Vec::new();
    let mut extra_indices = Vec::new();
    for quarter in 0..COUNT {
        times.push(quarter as f64 * 0.5);
        counts.push(Some(quarter as i64));
        if quarter % 100 == 48 {
            extra_indices.push(times.len());
            times.push(quarter as f64 * 0.5 + 0.25);
            counts.push(None);
        }
    }
    let summary = summarize(&standard_source(), &times, &counts);
    assert!((supported_period(&summary) - 0.5).abs() < 1e-12);
    assert_eq!(summary.raw_position_count, times.len());
    assert_eq!(summary.raw_beat_seconds, times);
    assert_eq!(summary.hypotheses[0].excluded_raw_indices, extra_indices);
    assert_eq!(summary.hypotheses[0].quarter_counts, counts);
    for index in extra_indices {
        assert_eq!(summary.hypotheses[0].residual_seconds[index], None);
    }
    assert_unsupported(&summarize(
        &standard_source(),
        &times,
        &consecutive_counts(times.len()),
    ));
}

#[test]
fn half_and_double_tempo_interpretations_are_ambiguous_even_with_one_verified() {
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    let identity = standard_source();
    let normal = consecutive_counts(COUNT);
    let double: Vec<_> = (0..COUNT).map(|index| Some(2 * index as i64)).collect();
    let mut half = hypothesis("half", &normal, QuarterNoteVerification::Unverified);
    half.quarter_note_denominator = 2;
    let summary = summarize_constant_tempo(
        &RawTempoEvidence {
            source: &identity,
            beat_seconds: &times,
            independent_origin_seconds: 0.0,
        },
        &[
            hypothesis("one", &normal, QuarterNoteVerification::Verified),
            hypothesis("two", &double, QuarterNoteVerification::Unverified),
            half,
        ],
    )
    .unwrap();
    assert_eq!(summary.status, SummaryStatus::Ambiguous);
    assert_eq!(summary.supported_hypothesis_index, None);
    for (candidate, expected) in summary.hypotheses.iter().zip([0.5, 0.25, 1.0]) {
        assert!(
            (candidate.fit.as_ref().unwrap().period_seconds_per_quarter - expected).abs() < 1e-12
        );
    }
}

#[test]
fn unverified_quarter_units_never_become_supported_from_a_perfect_fit() {
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    let counts = consecutive_counts(COUNT);
    let identity = standard_source();
    let summary = summarize_constant_tempo(
        &RawTempoEvidence {
            source: &identity,
            beat_seconds: &times,
            independent_origin_seconds: 0.0,
        },
        &[hypothesis(
            "unknown-unit",
            &counts,
            QuarterNoteVerification::Unverified,
        )],
    )
    .unwrap();
    assert_eq!(summary.status, SummaryStatus::Unverified);
    assert_eq!(summary.supported_hypothesis_index, None);
}

#[test]
fn count_offsets_and_fit_intercepts_do_not_replace_signed_origin_or_identity() {
    let times: Vec<_> = (0..COUNT).map(|index| 0.037 + index as f64 * 0.5).collect();
    let original_times = times.clone();
    let normal = consecutive_counts(COUNT);
    let shifted: Vec<_> = (0..COUNT).map(|index| Some(index as i64 - 6_000)).collect();
    let identity = standard_source();
    let original_identity = identity.clone();
    let summary = summarize_constant_tempo(
        &RawTempoEvidence {
            source: &identity,
            beat_seconds: &times,
            independent_origin_seconds: -0.03125,
        },
        &[
            hypothesis(
                "zero-count-origin",
                &normal,
                QuarterNoteVerification::Verified,
            ),
            hypothesis(
                "shifted-count-origin",
                &shifted,
                QuarterNoteVerification::Verified,
            ),
        ],
    )
    .unwrap();
    assert!((supported_period(&summary) - 0.5).abs() < 1e-12);
    assert_eq!(
        summary.independent_origin_seconds.to_bits(),
        (-0.03125_f64).to_bits()
    );
    assert_eq!(summary.source, original_identity);
    assert_eq!(identity, original_identity);
    assert_eq!(times, original_times);
    assert_eq!(summary.raw_beat_seconds, original_times);
    assert_eq!(summary.hypotheses[0].quarter_counts, normal);
    assert_eq!(summary.hypotheses[1].quarter_counts, shifted);
    let intercept = summary.hypotheses[0]
        .fit
        .as_ref()
        .unwrap()
        .diagnostic_intercept_seconds;
    assert!((intercept - 0.037).abs() < 1e-10);
    assert_ne!(intercept, summary.independent_origin_seconds);
}

#[test]
fn tempo_ramps_steps_and_phase_shifts_cannot_certify_one_constant_period() {
    let ramp: Vec<_> = (0..COUNT)
        .map(|index| 0.45 * index as f64 + 0.00004 * (index * index) as f64)
        .collect();
    let step: Vec<_> = (0..COUNT)
        .map(|index| 0.5 * index.min(600) as f64 + 0.49 * index.saturating_sub(600) as f64)
        .collect();
    let phase_jump: Vec<_> = (0..COUNT)
        .map(|index| 0.5 * index as f64 + if index >= 600 { 0.035 } else { 0.0 })
        .collect();
    let stable_middle: Vec<_> = (0..COUNT)
        .map(|index| {
            0.49 * index.min(200) as f64
                + 0.5 * index.saturating_sub(200).min(800) as f64
                + 0.51 * index.saturating_sub(1_000) as f64
        })
        .collect();
    for times in [ramp, step, phase_jump, stable_middle] {
        assert_unsupported(&summarize(
            &standard_source(),
            &times,
            &consecutive_counts(COUNT),
        ));
    }
}

#[test]
fn isolated_outliers_are_reported_but_a_cluster_is_not_hidden_by_robust_fitting() {
    let mut times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    let isolated_indices: Vec<_> = (0..COUNT).filter(|index| index % 97 == 48).collect();
    for &index in &isolated_indices {
        times[index] += 0.08;
    }
    let summary = summarize(&standard_source(), &times, &consecutive_counts(COUNT));
    assert!((supported_period(&summary) - 0.5).abs() < 1e-12);
    assert_eq!(summary.hypotheses[0].excluded_raw_indices, isolated_indices);
    for index in isolated_indices {
        assert!((summary.hypotheses[0].residual_seconds[index].unwrap() - 0.08).abs() < 1e-10);
    }
    for time in &mut times[590..593] {
        *time += 0.06;
    }
    assert_unsupported(&summarize(
        &standard_source(),
        &times,
        &consecutive_counts(COUNT),
    ));
}

#[test]
fn explicit_clustered_or_excessive_exclusions_do_not_create_complete_support() {
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    let mut cluster = consecutive_counts(COUNT);
    cluster[590..593].fill(None);
    assert_unsupported(&summarize(&standard_source(), &times, &cluster));
    let mut excessive = consecutive_counts(COUNT);
    for index in (0..COUNT).step_by(8) {
        excessive[index] = None;
    }
    assert_unsupported(&summarize(&standard_source(), &times, &excessive));
}

#[test]
fn sparse_short_or_middle_only_evidence_is_insufficient() {
    let sparse: Vec<_> = (0..12).map(|index| index as f64 * 50.0).collect();
    let sparse_counts: Vec<_> = (0..12).map(|index| Some(index * 100)).collect();
    assert_unsupported(&summarize(&standard_source(), &sparse, &sparse_counts));
    let short: Vec<_> = (0..60).map(|index| index as f64 * 0.5).collect();
    assert_unsupported(&summarize(
        &standard_source(),
        &short,
        &consecutive_counts(short.len()),
    ));
    let middle: Vec<_> = (0..400).map(|index| 200.0 + index as f64 * 0.5).collect();
    assert_unsupported(&summarize(
        &standard_source(),
        &middle,
        &consecutive_counts(middle.len()),
    ));
    assert_unsupported(&summarize(&standard_source(), &[], &[]));
    assert_unsupported(&summarize(&standard_source(), &[1.0], &[Some(0)]));
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    assert_unsupported(&summarize(&standard_source(), &times, &vec![None; COUNT]));
}

#[test]
fn nonfinite_unordered_negative_and_out_of_source_events_are_invalid() {
    let identity = standard_source();
    for times in [
        vec![0.0, f64::NAN],
        vec![0.0, f64::INFINITY],
        vec![f64::NEG_INFINITY, 0.0],
        vec![-0.001, 0.0],
        vec![0.0, 0.0],
        vec![1.0, 0.5],
        vec![0.0, 601.0],
        vec![0.0, f64::MAX],
    ] {
        let counts = consecutive_counts(times.len());
        assert!(
            summarize_constant_tempo(
                &RawTempoEvidence {
                    source: &identity,
                    beat_seconds: &times,
                    independent_origin_seconds: 0.0
                },
                &[hypothesis(
                    "invalid-times",
                    &counts,
                    QuarterNoteVerification::Verified
                )],
            )
            .is_err(),
            "invalid times: {times:?}"
        );
    }
}

#[test]
fn malformed_source_identity_and_nonfinite_origin_are_rejected() {
    let mut identities = Vec::new();
    for halfwidth in [f64::NAN, f64::INFINITY, -0.01, 602.0] {
        let mut identity = standard_source();
        identity.timing_error_halfwidth_seconds = halfwidth;
        identities.push(identity);
    }
    for rate in [0, u32::MAX] {
        let mut identity = standard_source();
        identity.loaded_sample_rate_hz = rate;
        identities.push(identity);
    }
    for frames in [0, u64::MAX] {
        let mut identity = standard_source();
        identity.loaded_frame_count = frames;
        identities.push(identity);
    }
    for field in 0..5 {
        let mut identity = standard_source();
        match field {
            0 => identity.source_sha256.clear(),
            1 => identity.pcm_sha256 = "z".repeat(64),
            2 => identity.backend_revision.clear(),
            3 => identity.configuration_revision = " ".into(),
            _ => identity.raw_revision = "x".repeat(4_097),
        }
        identities.push(identity);
    }
    let counts = [Some(0), Some(1)];
    let candidate = hypothesis(
        "source-validation",
        &counts,
        QuarterNoteVerification::Verified,
    );
    for identity in &identities {
        assert!(
            summarize_constant_tempo(
                &RawTempoEvidence {
                    source: identity,
                    beat_seconds: &[0.0, 0.5],
                    independent_origin_seconds: 0.0
                },
                &[candidate],
            )
            .is_err()
        );
    }
    let identity = standard_source();
    for origin in [f64::NAN, f64::NEG_INFINITY, f64::INFINITY] {
        assert!(
            summarize_constant_tempo(
                &RawTempoEvidence {
                    source: &identity,
                    beat_seconds: &[0.0, 0.5],
                    independent_origin_seconds: origin
                },
                &[candidate],
            )
            .is_err()
        );
    }
}

#[test]
fn invalid_count_extents_order_and_overflow_are_rejected_without_panics() {
    let identity = standard_source();
    let raw = RawTempoEvidence {
        source: &identity,
        beat_seconds: &[0.0, 0.5],
        independent_origin_seconds: 0.0,
    };
    for counts in [
        vec![],
        vec![Some(0)],
        vec![Some(0), Some(1), Some(2)],
        vec![Some(1), Some(1)],
        vec![Some(1), Some(0)],
        vec![Some(i64::MIN), Some(i64::MAX)],
        vec![Some(-(1_i64 << 52)), Some(1_i64 << 52)],
    ] {
        assert!(
            summarize_constant_tempo(
                &raw,
                &[hypothesis(
                    "invalid-counts",
                    &counts,
                    QuarterNoteVerification::Verified
                )],
            )
            .is_err(),
            "invalid counts: {counts:?}"
        );
    }
    let counts = [Some(0), Some(1)];
    assert!(
        summarize_constant_tempo(
            &raw,
            &[hypothesis("", &counts, QuarterNoteVerification::Verified)],
        )
        .is_err()
    );
    let mut candidate = hypothesis(
        "missing-provenance",
        &counts,
        QuarterNoteVerification::Verified,
    );
    candidate.provenance = "";
    assert!(summarize_constant_tempo(&raw, &[candidate]).is_err());
    let duplicate = hypothesis("duplicate-id", &counts, QuarterNoteVerification::Verified);
    assert!(summarize_constant_tempo(&raw, &[duplicate, duplicate]).is_err());
}

#[test]
fn raw_position_and_hypothesis_limits_are_checked_before_fitting() {
    let identity = standard_source();
    let times: Vec<_> = (0..=MAX_RAW_POSITIONS)
        .map(|index| index as f64 / 1_000.0)
        .collect();
    let counts = consecutive_counts(times.len());
    let raw = RawTempoEvidence {
        source: &identity,
        beat_seconds: &times,
        independent_origin_seconds: 0.0,
    };
    assert!(
        summarize_constant_tempo(
            &raw,
            &[hypothesis(
                "oversized-raw",
                &counts,
                QuarterNoteVerification::Verified
            )],
        )
        .is_err()
    );
    let raw = RawTempoEvidence {
        source: &identity,
        beat_seconds: &[0.0, 0.5],
        independent_origin_seconds: 0.0,
    };
    let counts = [Some(0), Some(1)];
    let ids: Vec<_> = (0..=MAX_HYPOTHESES)
        .map(|index| format!("hypothesis-{index}"))
        .collect();
    let candidates: Vec<_> = ids
        .iter()
        .map(|id| hypothesis(id, &counts, QuarterNoteVerification::Verified))
        .collect();
    assert!(summarize_constant_tempo(&raw, &candidates).is_err());
    assert!(summarize_constant_tempo(&raw, &[]).is_err());
}

#[test]
fn large_signed_count_offsets_do_not_lose_fractional_period_precision() {
    let period = 60.0 / 123.45;
    let times: Vec<_> = (0..COUNT)
        .map(|index| 0.125 + index as f64 * period)
        .collect();
    for offset in [-(1_i64 << 52), (1_i64 << 52) - COUNT as i64] {
        let counts: Vec<_> = (0..COUNT)
            .map(|index| Some(offset + index as i64))
            .collect();
        let summary = summarize(&standard_source(), &times, &counts);
        let actual = supported_period(&summary);
        assert!((actual - period).abs() * (COUNT - 1) as f64 * f64::from(RATE) < 1e-4);
        let fit = summary.hypotheses[0].fit.as_ref().unwrap();
        assert_eq!(fit.reference_count_numerator, offset);
        assert!((fit.fitted_seconds_at_reference - 0.125).abs() < 1e-10);
        assert!(fit.diagnostic_intercept_seconds.is_finite());
    }
}

#[test]
fn declared_uncertainty_contains_correlated_detector_error_without_noise_assumptions() {
    let halfwidth = 0.005;
    let identity = source(RATE, 601.0, halfwidth);
    let counts = consecutive_counts(COUNT);
    for step_error in [false, true] {
        let times: Vec<_> = (0..COUNT)
            .map(|index| {
                let signed_error = if step_error {
                    if index < COUNT / 2 {
                        -halfwidth
                    } else {
                        halfwidth
                    }
                } else {
                    halfwidth * (2.0 * index as f64 / (COUNT - 1) as f64 - 1.0)
                };
                0.125 + index as f64 * 0.5 + signed_error
            })
            .collect();
        let summary = summarize(&identity, &times, &counts);
        let actual = supported_period(&summary);
        let fit = summary.hypotheses[0].fit.as_ref().unwrap();
        assert!(
            (actual - 0.5).abs() > 1e-6,
            "the fixture must introduce a measurable slope"
        );
        assert!((actual - 0.5).abs() <= fit.period_sensitivity_bound_seconds + 1e-14);
        assert_eq!(summary.hypotheses[0].inlier_raw_indices.len(), COUNT);
        assert!(summary.hypotheses[0].excluded_raw_indices.is_empty());
    }
}

#[test]
fn zero_declared_error_keeps_numerical_tolerance_distinct_from_measurement_uncertainty() {
    let identity = source(RATE, 601.0, 0.0);
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    let summary = summarize(&identity, &times, &consecutive_counts(COUNT));
    assert_eq!(supported_period(&summary), 0.5);
    let fit = summary.hypotheses[0].fit.as_ref().unwrap();
    assert_eq!(fit.period_sensitivity_bound_seconds, 0.0);
    assert!(fit.numerical_tolerance_seconds.is_finite() && fit.numerical_tolerance_seconds > 0.0);
}

#[test]
fn equivalent_scaled_count_fractions_retain_one_physical_interpretation() {
    let identity = standard_source();
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    let integers = consecutive_counts(COUNT);
    let doubled: Vec<_> = (0..COUNT)
        .map(|index| Some(2 * index as i64 + 800))
        .collect();
    let tripled: Vec<_> = (0..COUNT)
        .map(|index| Some(3 * index as i64 - 600))
        .collect();
    let mut half_ticks = hypothesis("half-ticks", &doubled, QuarterNoteVerification::Verified);
    half_ticks.quarter_note_denominator = 2;
    let mut third_ticks = hypothesis("third-ticks", &tripled, QuarterNoteVerification::Verified);
    third_ticks.quarter_note_denominator = 3;
    let summary = summarize_constant_tempo(
        &RawTempoEvidence {
            source: &identity,
            beat_seconds: &times,
            independent_origin_seconds: -0.0625,
        },
        &[
            hypothesis(
                "integer-count",
                &integers,
                QuarterNoteVerification::Verified,
            ),
            half_ticks,
            third_ticks,
        ],
    )
    .unwrap();
    assert_eq!(supported_period(&summary), 0.5);
    for (diagnostic, denominator) in summary.hypotheses.iter().zip([1, 2, 3]) {
        assert_eq!(diagnostic.quarter_note_denominator, denominator);
        let fit = diagnostic.fit.as_ref().unwrap();
        assert_eq!(fit.quarter_note_denominator, denominator);
        assert!((fit.period_seconds_per_quarter - 0.5).abs() < 1e-12);
    }
    assert_eq!(summary.independent_origin_seconds, -0.0625);
}

#[test]
fn explicit_triplet_positions_keep_thirds_as_musical_quarter_units() {
    let identity = source(RATE, 201.0, 0.5 / f64::from(RATE));
    let counts = consecutive_counts(COUNT);
    // A 120-BPM quarter lasts 0.5s; three equally spaced subdivision events per
    // quarter must not be mistaken for three quarter notes at 360 BPM.
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 / 6.0).collect();
    let mut candidate = hypothesis("triplet-events", &counts, QuarterNoteVerification::Verified);
    candidate.quarter_note_denominator = 3;
    let summary = summarize_constant_tempo(
        &RawTempoEvidence {
            source: &identity,
            beat_seconds: &times,
            independent_origin_seconds: 0.0,
        },
        &[candidate],
    )
    .unwrap();
    assert!((supported_period(&summary) - 0.5).abs() < 1e-12);
    assert_eq!(summary.hypotheses[0].inlier_raw_indices.len(), COUNT);
    assert!(summary.hypotheses[0].excluded_raw_indices.is_empty());
}

#[test]
fn malformed_quarter_count_denominators_are_rejected() {
    let identity = standard_source();
    let counts = [Some(0), Some(1)];
    let raw = RawTempoEvidence {
        source: &identity,
        beat_seconds: &[0.0, 0.5],
        independent_origin_seconds: 0.0,
    };
    for denominator in [0, 65, u32::MAX] {
        let mut candidate = hypothesis(
            "invalid-divisor",
            &counts,
            QuarterNoteVerification::Verified,
        );
        candidate.quarter_note_denominator = denominator;
        assert!(summarize_constant_tempo(&raw, &[candidate]).is_err());
    }
}

#[test]
fn alternating_offsets_outside_declared_error_cannot_pass_on_small_ols_residuals() {
    let identity = source(RATE, 601.0, 0.005);
    // OLS can leave residuals near +/-0.0075s, below a 2*halfwidth inlier
    // threshold. Nevertheless no constant line explains all events within the
    // declared +/-0.005s error; the accepted error strip must be feasible.
    let times: Vec<_> = (0..COUNT)
        .map(|index| index as f64 * 0.5 + (index % 2) as f64 * 0.015)
        .collect();
    assert_unsupported(&summarize(&identity, &times, &consecutive_counts(COUNT)));
}

#[test]
fn distant_windows_require_global_support_not_only_a_separate_local_fit() {
    let identity = source(RATE, 600.0, 0.005);
    let quarters: Vec<i64> = (0..6).chain(400..430).chain(1_000..1_030).collect();
    let times: Vec<_> = quarters
        .iter()
        .enumerate()
        .map(|(index, quarter)| {
            let error = if index >= 6 {
                0.0
            } else if index % 2 == 0 {
                0.005
            } else {
                0.013
            };
            *quarter as f64 * 0.5 + error
        })
        .collect();
    let counts: Vec<_> = quarters.into_iter().map(Some).collect();
    // The first six events admit their own shifted local line. Only three
    // agree with the complete-source fit, so the early window cannot certify
    // the required six independently supported positions for that global line.
    assert_unsupported(&summarize(&identity, &times, &counts));
}
