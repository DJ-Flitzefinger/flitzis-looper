//! Independent numerical/count fixtures; no candidate-derived musical labels.

use flitzis_looper_analysis::{
    selected_bpm::{
        ORDINAL_PROVENANCE, SelectedBpmInput, SelectedBpmSummary, summarize_selected_bpm,
    },
    tempo_summary::{
        HypothesisStatus, MAX_RAW_POSITIONS, QuarterNoteHypothesis, QuarterNoteVerification,
        RawTempoEvidence, SourceIdentity, summarize_constant_tempo,
    },
};

fn assess(
    times: &[f64],
    counts: Option<&[Option<i64>]>,
    denominator: u32,
    duration: f64,
) -> SelectedBpmSummary {
    summarize_selected_bpm(SelectedBpmInput {
        beat_seconds: times,
        sample_rate_hz: 48_000,
        frame_count: (duration * 48_000.0).round() as u64,
        quarter_counts: counts,
        quarter_note_denominator: denominator,
        count_provenance: ORDINAL_PROVENANCE,
    })
    .unwrap()
}

#[test]
fn full_centered_fit_preserves_exact_and_true_fractional_periods() {
    for bpm in [120.0, 123.45, 119.999] {
        let times: Vec<_> = (0..1200).map(|index| index as f64 * 60.0 / bpm).collect();
        let report = assess(&times, None, 1, 601.0);
        let fit = report.complete_fit.as_ref().unwrap();
        assert!((fit.bpm - bpm).abs() < 1e-10, "{bpm}: {fit:?}");
        assert_eq!(fit.assigned_observations, 1200);
        assert_eq!(report.raw_beat_seconds, times);
        assert_eq!(report.local_interval_seconds.len(), 1199);
        assert_eq!(report.complete_residual_seconds.len(), 1200);
        assert_eq!(report.global.status, HypothesisStatus::Unverified);
        assert_eq!(report.selected_region_id, Some("middle"));
        assert!(fit.period_sensitivity_bound_seconds > 0.0);
        assert_eq!(report.beat_unit, "quarter-note-assumption");
        assert!((report.alternatives_bpm[0].unwrap() - bpm * 0.5).abs() < 1e-10);
        assert!((report.alternatives_bpm[2].unwrap() - bpm * 2.0).abs() < 1e-10);
    }
}

#[test]
fn twenty_ms_lattice_ols_uses_interior_evidence_and_reports_conditional_sensitivity() {
    for bpm in [123.45, 119.999] {
        let period = 60.0 / bpm;
        let times: Vec<_> = (0..1200)
            .map(|index| (index as f64 * period / 0.02).round() * 0.02)
            .collect();
        let report = assess(&times, None, 1, 601.0);
        let fit = report.complete_fit.as_ref().unwrap();
        let endpoints = (times[1199] - times[0]) / 1199.0;
        assert!(
            (fit.period_seconds_per_quarter - period).abs() <= (endpoints - period).abs() + 1e-16
        );
        assert!(
            (fit.period_seconds_per_quarter - period).abs() <= fit.period_sensitivity_bound_seconds
        );
        assert!(fit.max_abs_residual_seconds <= 0.0101);
        assert_eq!(report.global.status, HypothesisStatus::Unverified);
        if bpm == 123.45 {
            assert_ne!(fit.bpm, fit.bpm.round());
        } else {
            // This 600-s119.999-BPM fixture aliases entirely to120 on20ms
            // coordinates; retained lattice uncertainty, not rounding, explains it.
            assert_eq!(fit.bpm, 120.0);
        }
    }
}

#[test]
fn missing_events_need_explicit_counts_and_extra_events_keep_full_raw_indices() {
    let mut times = Vec::new();
    let mut counts = Vec::new();
    for quarter in 0..1200 {
        if quarter % 37 != 18 {
            times.push(quarter as f64 * 0.5);
            counts.push(Some(quarter));
        }
        if quarter % 59 == 13 {
            times.push(quarter as f64 * 0.5 + 0.11);
            counts.push(None);
        }
    }
    let explicit = assess(&times, Some(&counts), 1, 600.0);
    assert_eq!(explicit.global.status, HypothesisStatus::Unverified);
    assert!((explicit.complete_fit.as_ref().unwrap().bpm - 120.0).abs() < 1e-12);
    assert_eq!(explicit.raw_beat_seconds, times);
    assert_eq!(explicit.quarter_counts, counts);
    assert_eq!(explicit.beat_unit, "explicit-quarter-note-count-assertion");
    for (index, count) in counts.iter().enumerate() {
        assert_eq!(
            explicit.complete_residual_seconds[index].is_none(),
            count.is_none()
        );
        assert_eq!(
            explicit.global.excluded_raw_indices.contains(&index),
            count.is_none()
        );
        if count.is_none() && index > 0 {
            assert_eq!(explicit.local_interval_bpm[index - 1], None);
        }
    }
    assert_eq!(
        assess(&times, None, 1, 600.0).global.status,
        HypothesisStatus::Unsupported
    );
}

#[test]
fn comparable_snares_use_supplied_quarter_counts_and_rational_denominator() {
    let times: Vec<_> = (0..150).map(|index| 4.0 * index as f64).collect();
    let counts: Vec<_> = (0..150).map(|index| Some(16 * index)).collect();
    let explicit = assess(&times, Some(&counts), 2, 600.0);
    assert_eq!(explicit.complete_fit.as_ref().unwrap().bpm, 120.0);
    assert_eq!(explicit.global.status, HypothesisStatus::Unverified);
    assert_eq!(explicit.local_interval_bpm, vec![Some(120.0); 149]);
    assert_eq!(
        assess(&times, None, 1, 600.0)
            .complete_fit
            .as_ref()
            .unwrap()
            .bpm,
        15.0
    );
}

#[test]
fn stable_middle_cannot_replace_unsupported_sparse_complete_evidence() {
    let times: Vec<_> = (240..960).map(|index| index as f64 * 0.5).collect();
    let report = assess(&times, None, 1, 600.0);
    assert_eq!(report.global.status, HypothesisStatus::Unsupported);
    assert_eq!(report.selected_region_id, Some("middle"));
    assert_eq!(report.representative_bpm, Some(120.0));
    assert_eq!(report.global.windows.len(), 3);
    assert_eq!(report.raw_beat_seconds.first(), Some(&120.0));
    // The intercept describes ordinal count zero at120s; no origin is adopted.
    assert_eq!(
        report
            .complete_fit
            .as_ref()
            .unwrap()
            .diagnostic_intercept_seconds,
        120.0
    );
}

#[test]
fn infeasible_ambiguous_middle_selects_another_viable_third_deterministically() {
    let times: Vec<_> = (0..1200)
        .map(|index| {
            let variation = if (400..800).contains(&index) {
                if index % 2 == 0 { 0.015 } else { -0.015 }
            } else {
                0.0
            };
            index as f64 * 0.5 + variation
        })
        .collect();
    let report = assess(&times, None, 1, 600.0);
    assert_eq!(report.global.status, HypothesisStatus::Unsupported);
    assert_eq!(report.regions[0].status, "unsupported");
    assert!(
        report.regions[0]
            .reasons
            .contains(&"inconsistent_timing_bound")
    );
    assert_eq!(report.selected_region_id, Some("early"));
    assert_eq!(report.representative_bpm, Some(120.0));
}

#[test]
fn changing_complete_tempo_retains_local_intervals_without_global_constant_claim() {
    let times: Vec<_> = (0..1200)
        .map(|index| {
            if index < 400 {
                index as f64 * 0.5
            } else if index < 800 {
                200.0 + (index - 400) as f64 * 0.45
            } else {
                380.0 + (index - 800) as f64 * 0.55
            }
        })
        .collect();
    let report = assess(&times, None, 1, 600.0);
    assert_eq!(report.global.status, HypothesisStatus::Unsupported);
    assert!(report.complete_fit.is_some());
    assert!((report.local_interval_bpm[500].unwrap() - 60.0 / 0.45).abs() < 1e-10);
    assert!((report.local_interval_bpm[1000].unwrap() - 60.0 / 0.55).abs() < 1e-10);
    assert_eq!(report.raw_beat_seconds, times);
}

#[test]
fn short_sources_expose_no_long_region_without_fallback() {
    let times: Vec<_> = (0..60).map(|index| index as f64 * 0.5).collect();
    let report = assess(&times, None, 1, 30.0);
    assert_eq!(report.complete_fit.as_ref().unwrap().bpm, 120.0);
    assert_eq!(report.selected_region_id, None);
    assert_eq!(report.representative_bpm, None);
    assert!(
        report
            .regions
            .iter()
            .all(|region| region.reasons.contains(&"no_long_region"))
    );
}

#[test]
fn explicit_exclusion_budget_and_runs_apply_inside_every_region() {
    let times: Vec<_> = (0..1200).map(|index| index as f64 * 0.5).collect();
    let mut counts: Vec<_> = (0..1200).map(Some).collect();
    for index in [250, 251, 252, 650, 651, 652, 1050, 1051, 1052] {
        counts[index] = None;
    }
    let report = assess(&times, Some(&counts), 1, 600.0);
    assert_eq!(report.global.status, HypothesisStatus::Unsupported);
    assert_eq!(report.selected_region_id, None);
    assert!(
        report
            .regions
            .iter()
            .all(|region| region.reasons.contains(&"consecutive_exclusions"))
    );
    let counts: Vec<_> = (0..1200)
        .map(|index| (index % 8 != 0).then_some(index))
        .collect();
    let report = assess(&times, Some(&counts), 1, 600.0);
    assert_eq!(report.selected_region_id, None);
    assert!(
        report
            .regions
            .iter()
            .all(|region| region.reasons.contains(&"too_many_exclusions"))
    );
}

#[test]
fn invalid_complete_domains_fail_instead_of_repair_or_truncation() {
    let valid = [0.0, 0.5, 1.0];
    let base = SelectedBpmInput {
        beat_seconds: &valid,
        sample_rate_hz: 48_000,
        frame_count: 96_000,
        quarter_counts: None,
        quarter_note_denominator: 1,
        count_provenance: ORDINAL_PROVENANCE,
    };
    for times in [
        vec![0.5, 0.0],
        vec![0.0, 0.0],
        vec![-0.1, 0.5],
        vec![0.0, 2.0],
        vec![f64::NAN],
        vec![f64::INFINITY],
    ] {
        assert!(
            summarize_selected_bpm(SelectedBpmInput {
                beat_seconds: &times,
                ..base
            })
            .is_err()
        );
    }
    for rate in [0, 7_999, 768_001, u32::MAX] {
        assert!(
            summarize_selected_bpm(SelectedBpmInput {
                sample_rate_hz: rate,
                ..base
            })
            .is_err()
        );
    }
    for frame_count in [0, (1_u64 << 53) + 1] {
        assert!(
            summarize_selected_bpm(SelectedBpmInput {
                frame_count,
                ..base
            })
            .is_err()
        );
    }
    for denominator in [0, 2, 65] {
        assert!(
            summarize_selected_bpm(SelectedBpmInput {
                quarter_note_denominator: denominator,
                ..base
            })
            .is_err()
        );
    }
    for counts in [
        vec![Some(0)],
        vec![Some(0), Some(0), Some(2)],
        vec![Some(1_i64 << 53), Some(2), Some(3)],
        vec![Some(-(1_i64 << 52)), Some(0), Some(1)],
    ] {
        assert!(
            summarize_selected_bpm(SelectedBpmInput {
                quarter_counts: Some(&counts),
                ..base
            })
            .is_err()
        );
    }
    let oversize = vec![0.0; MAX_RAW_POSITIONS + 1];
    assert!(
        summarize_selected_bpm(SelectedBpmInput {
            beat_seconds: &oversize,
            ..base
        })
        .is_err()
    );
    for provenance in ["", " \n "] {
        assert!(
            summarize_selected_bpm(SelectedBpmInput {
                count_provenance: provenance,
                ..base
            })
            .is_err()
        );
    }
    let provenance = "p".repeat(4097);
    assert!(
        summarize_selected_bpm(SelectedBpmInput {
            count_provenance: &provenance,
            ..base
        })
        .is_err()
    );
}

#[test]
fn bounded_edge_dimensions_and_empty_input_remain_explicitly_unsupported() {
    for sample_rate_hz in [8_000, 768_000] {
        for frame_count in [1, 1_u64 << 53] {
            let report = summarize_selected_bpm(SelectedBpmInput {
                beat_seconds: &[],
                sample_rate_hz,
                frame_count,
                quarter_counts: None,
                quarter_note_denominator: 1,
                count_provenance: ORDINAL_PROVENANCE,
            })
            .unwrap();
            assert_eq!(report.global.status, HypothesisStatus::Unsupported);
            assert_eq!(report.complete_fit, None);
            assert_eq!(report.representative_bpm, None);
        }
    }
}

#[test]
fn complete_position_limit_is_admitted_without_subsampling_the_ols() {
    let times: Vec<_> = (0..MAX_RAW_POSITIONS)
        .map(|index| index as f64 * 0.5)
        .collect();
    let report = assess(&times, None, 1, MAX_RAW_POSITIONS as f64 * 0.5);
    assert_eq!(
        report.complete_fit.as_ref().unwrap().assigned_observations,
        MAX_RAW_POSITIONS
    );
    assert_eq!(report.complete_fit.as_ref().unwrap().bpm, 120.0);
    assert_eq!(report.global.inlier_raw_indices.len(), MAX_RAW_POSITIONS);
    assert_eq!(report.raw_beat_seconds, times);
}

#[test]
fn large_signed_count_origins_keep_centered_period_and_explicit_units() {
    let times: Vec<_> = (0..1200).map(|index| index as f64 * 0.5).collect();
    for origin in [-(1_i64 << 52), (1_i64 << 52) - 1200] {
        let counts: Vec<_> = (0..1200).map(|index| Some(origin + index)).collect();
        let report = assess(&times, Some(&counts), 64, 600.0);
        assert_eq!(report.complete_fit.as_ref().unwrap().bpm, 1.875);
        assert_eq!(
            report
                .complete_fit
                .as_ref()
                .unwrap()
                .reference_count_numerator,
            origin
        );
        assert_eq!(report.global.status, HypothesisStatus::Unverified);
        assert_eq!(report.local_interval_bpm, vec![Some(1.875); 1199]);
    }
}

#[test]
fn shared_g2_numerical_assessment_matches_source_bound_original_diagnostics() {
    let times: Vec<_> = (0..1200)
        .map(|index| (index as f64 * 60.0 / 123.45 / 0.02).round() * 0.02)
        .collect();
    let counts: Vec<_> = (0..1200).map(Some).collect();
    let report = assess(&times, Some(&counts), 1, 600.0);
    let source = SourceIdentity {
        source_sha256: "1".repeat(64),
        pcm_sha256: "2".repeat(64),
        loaded_sample_rate_hz: 48_000,
        loaded_frame_count: 28_800_000,
        backend_revision: "independent-fixture".into(),
        configuration_revision: "fixture-v1".into(),
        raw_revision: "raw-fixture-v1".into(),
        timing_error_halfwidth_seconds: 0.01,
    };
    let expected = summarize_constant_tempo(
        &RawTempoEvidence {
            source: &source,
            beat_seconds: &times,
            independent_origin_seconds: -0.25,
        },
        &[QuarterNoteHypothesis {
            id: "selected-backend-counts-v1",
            provenance: ORDINAL_PROVENANCE,
            verification: QuarterNoteVerification::Unverified,
            quarter_note_denominator: 1,
            quarter_counts: &counts,
        }],
    )
    .unwrap();
    assert_eq!(report.global, expected.hypotheses[0]);
    assert_eq!(expected.independent_origin_seconds, -0.25);
}
