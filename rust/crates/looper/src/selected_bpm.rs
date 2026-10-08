//! Pure offline Python binding; no engine, source reads, device or callback.

use flitzis_looper_analysis::{
    selected_bpm::{
        AggregateFit, ORDINAL_PROVENANCE, POLICY_VERSION, REGION_POLICY_VERSION, SelectedBpmInput,
        SelectedBpmSummary, TIMING_HALFWIDTH_SECONDS, UNCERTAINTY, summarize_selected_bpm,
    },
    tempo_summary::{self, HypothesisStatus, PeriodFit, RejectionReason},
};
use pyo3::{exceptions::PyValueError, prelude::*};
use serde_json::{Value, json};

/// Separate numerical metadata budget. Existing worker/final wire limits remain
/// unchanged; oversized metadata fails explicitly and is never truncated.
const MAX_METADATA_JSON_BYTES: usize = 64 * 1024 * 1024;

#[pyfunction]
#[pyo3(signature = (
    beat_seconds, sample_rate_hz, frame_count, quarter_counts=None,
    quarter_note_denominator=1, count_provenance=ORDINAL_PROVENANCE
))]
pub(crate) fn summarize_selected_bpm_json(
    py: Python<'_>,
    beat_seconds: Vec<f64>,
    sample_rate_hz: u32,
    frame_count: u64,
    quarter_counts: Option<Vec<Option<i64>>>,
    quarter_note_denominator: u32,
    count_provenance: &str,
) -> PyResult<String> {
    let count_provenance = count_provenance.to_owned();
    py.detach(move || {
        summarize_selected_bpm(SelectedBpmInput {
            beat_seconds: &beat_seconds,
            sample_rate_hz,
            frame_count,
            quarter_counts: quarter_counts.as_deref(),
            quarter_note_denominator,
            count_provenance: &count_provenance,
        })
        .map_err(|error| error.to_string())
        .and_then(|summary| encode_summary(&summary))
    })
    .map_err(PyValueError::new_err)
}

fn aggregate_json(fit: &AggregateFit) -> Value {
    json!({
        "assigned_observations": fit.assigned_observations,
        "period_seconds_per_quarter": fit.period_seconds_per_quarter,
        "bpm": fit.bpm,
        "reference_count_numerator": fit.reference_count_numerator,
        "quarter_note_denominator": fit.quarter_note_denominator,
        "fitted_seconds_at_reference": fit.fitted_seconds_at_reference,
        "diagnostic_intercept_seconds": fit.diagnostic_intercept_seconds,
        "period_sensitivity_bound_seconds": fit.period_sensitivity_bound_seconds,
        "max_abs_residual_seconds": fit.max_abs_residual_seconds,
        "residual_range_seconds": fit.residual_range_seconds,
        "numerical_tolerance_seconds": fit.numerical_tolerance_seconds,
    })
}

fn global_fit_json(fit: &PeriodFit) -> Value {
    json!({
        "period_seconds_per_quarter": fit.period_seconds_per_quarter,
        "bpm": 60.0 / fit.period_seconds_per_quarter,
        "reference_count_numerator": fit.reference_count_numerator,
        "quarter_note_denominator": fit.quarter_note_denominator,
        "fitted_seconds_at_reference": fit.fitted_seconds_at_reference,
        "diagnostic_intercept_seconds": fit.diagnostic_intercept_seconds,
        "period_sensitivity_bound_seconds": fit.period_sensitivity_bound_seconds,
        "max_abs_inlier_residual_seconds": fit.max_abs_inlier_residual_seconds,
        "inlier_residual_range_seconds": fit.inlier_residual_range_seconds,
        "window_period_spread_seconds": fit.window_period_spread_seconds,
        "numerical_tolerance_seconds": fit.numerical_tolerance_seconds,
    })
}

fn reason_name(reason: RejectionReason) -> &'static str {
    match reason {
        RejectionReason::InsufficientPositions => "insufficient_positions",
        RejectionReason::InsufficientTemporalCoverage => "insufficient_temporal_coverage",
        RejectionReason::NoRobustFit => "no_robust_fit",
        RejectionReason::TooManyExclusions => "too_many_exclusions",
        RejectionReason::ConsecutiveExclusions => "consecutive_exclusions",
        RejectionReason::InconsistentWindowPeriods => "inconsistent_window_periods",
        RejectionReason::InconsistentWindowOffsets => "inconsistent_window_offsets",
        RejectionReason::InconsistentTimingBound => "inconsistent_timing_bound",
        RejectionReason::UnverifiedQuarterNotes => "unverified_quarter_notes",
    }
}

fn encode_summary(summary: &SelectedBpmSummary) -> Result<String, String> {
    let windows: Vec<_> = summary
        .global
        .windows
        .iter()
        .map(|window| {
            json!({
                "start_seconds": window.start_seconds,
                "end_seconds": window.end_seconds,
                "assigned_positions": window.assigned_positions,
                "global_inlier_positions": window.global_inlier_positions,
                "inlier_positions": window.inlier_positions,
                "period_seconds_per_quarter": window.period_seconds_per_quarter,
                "period_sensitivity_bound_seconds": window.period_sensitivity_bound_seconds,
                "median_global_residual_seconds": window.median_global_residual_seconds,
            })
        })
        .collect();
    let regions: Vec<_> = summary
        .regions
        .iter()
        .map(|region| {
            json!({
                "id": region.id,
                "start_seconds": region.start_seconds,
                "end_seconds": region.end_seconds,
                "assigned_positions": region.assigned_positions,
                "raw_positions": region.raw_positions,
                "inlier_raw_indices": region.inlier_raw_indices,
                "excluded_raw_indices": region.excluded_raw_indices,
                "status": region.status,
                "reasons": region.reasons,
                "fit": region.fit.as_ref().map(aggregate_json),
            })
        })
        .collect();
    let value = json!({
        "schema_version": 1,
        "policy_version": POLICY_VERSION,
        "region_policy_version": REGION_POLICY_VERSION,
        "global_policy_version": tempo_summary::POLICY_VERSION,
        "beat_unit": summary.beat_unit,
        "count_provenance": summary.count_provenance,
        "timing_halfwidth_seconds": TIMING_HALFWIDTH_SECONDS,
        "uncertainty": UNCERTAINTY,
        "sample_rate_hz": summary.sample_rate_hz,
        "frame_count": summary.frame_count,
        "raw_position_count": summary.raw_beat_seconds.len(),
        "raw_beat_seconds": summary.raw_beat_seconds,
        "quarter_counts": summary.quarter_counts,
        "quarter_note_denominator": summary.quarter_note_denominator,
        "local_interval_seconds": summary.local_interval_seconds,
        "local_interval_bpm": summary.local_interval_bpm,
        "complete_fit": summary.complete_fit.as_ref().map(aggregate_json),
        "complete_residual_seconds": summary.complete_residual_seconds,
        "global_status": if summary.global.status == HypothesisStatus::Unsupported {
            "unsupported"
        } else {
            "unverified"
        },
        "global_reasons": summary.global.reasons.iter().copied().map(reason_name).collect::<Vec<_>>(),
        "global_fit": summary.global.fit.as_ref().map(global_fit_json),
        "global_inlier_raw_indices": summary.global.inlier_raw_indices,
        "global_excluded_raw_indices": summary.global.excluded_raw_indices,
        "global_residual_seconds": summary.global.residual_seconds,
        "global_windows": windows,
        "regions": regions,
        "selected_region_id": summary.selected_region_id,
        "representative_bpm": summary.representative_bpm,
        "alternatives_bpm": {
            "half": summary.alternatives_bpm[0],
            "ordinal": summary.alternatives_bpm[1],
            "double": summary.alternatives_bpm[2],
        },
    });
    let encoded = serde_json::to_string(&value).map_err(|error| error.to_string())?;
    if encoded.len() > MAX_METADATA_JSON_BYTES {
        return Err("selected BPM metadata exceeds separate 64 MiB JSON limit".to_owned());
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_metadata_retains_complete_unverified_domains_without_engine() {
        let times: Vec<_> = (0..1200).map(|index| index as f64 * 0.5).collect();
        let summary = summarize_selected_bpm(SelectedBpmInput {
            beat_seconds: &times,
            sample_rate_hz: 48_000,
            frame_count: 28_800_000,
            quarter_counts: None,
            quarter_note_denominator: 1,
            count_provenance: ORDINAL_PROVENANCE,
        })
        .unwrap();
        let value: Value = serde_json::from_str(&encode_summary(&summary).unwrap()).unwrap();
        assert_eq!(value["raw_beat_seconds"].as_array().unwrap().len(), 1200);
        assert_eq!(
            value["local_interval_seconds"].as_array().unwrap().len(),
            1199
        );
        assert_eq!(value["global_status"], "unverified");
        assert_eq!(value["complete_fit"]["bpm"], 120.0);
        assert_eq!(value["selected_region_id"], "middle");
        assert_eq!(
            value["alternatives_bpm"],
            json!({"half":60.0,"ordinal":120.0,"double":240.0})
        );
        assert!(value.get("source_sha256").is_none());
        assert!(value.get("independent_origin_seconds").is_none());
        assert!(value.get("accepted").is_none());
    }

    #[test]
    fn valid_empty_sequence_serializes_explicit_null_fits() {
        let summary = summarize_selected_bpm(SelectedBpmInput {
            beat_seconds: &[],
            sample_rate_hz: 8_000,
            frame_count: 1,
            quarter_counts: None,
            quarter_note_denominator: 1,
            count_provenance: ORDINAL_PROVENANCE,
        })
        .unwrap();
        let value: Value = serde_json::from_str(&encode_summary(&summary).unwrap()).unwrap();
        for key in [
            "complete_fit",
            "global_fit",
            "representative_bpm",
            "selected_region_id",
        ] {
            assert!(value[key].is_null(), "{key}");
        }
        assert_eq!(value["global_status"], "unsupported");
    }
}
