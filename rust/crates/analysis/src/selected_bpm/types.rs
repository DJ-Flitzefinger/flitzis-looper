//! Uncertified metadata, distinct from source-bound or accepted timing.

use crate::tempo_summary::HypothesisDiagnostic;

pub const POLICY_VERSION: &str = "selected-backend-bpm-v1";
pub const REGION_POLICY_VERSION: &str = "representative-middle-region-v1";
/// Half of the selected detector's 20-ms lattice; not an acoustic timing bound.
pub const TIMING_HALFWIDTH_SECONDS: f64 = 0.01;
pub const ORDINAL_PROVENANCE: &str = "selected-backend ordinal assumption";
pub const UNCERTAINTY: &str = "detector-lattice-only; musical timing/count unverified";

/// Complete binary64 coordinates and actual loaded extent. No source identity
/// or origin is asserted by this numerical input.
#[derive(Debug, Clone, Copy)]
pub struct SelectedBpmInput<'a> {
    pub beat_seconds: &'a [f64],
    pub sample_rate_hz: u32,
    pub frame_count: u64,
    /// Explicit exclusions and missing-event count jumps; None assumes ordinal
    /// counts, one unverified quarter per detected beat.
    pub quarter_counts: Option<&'a [Option<i64>]>,
    pub quarter_note_denominator: u32,
    pub count_provenance: &'a str,
}

/// Equal-observation centered OLS on precisely the reported retained positions.
/// Sensitivity is conditional on counts and the detector lattice, not a
/// calibrated confidence interval or a proven musical/acoustic bound.
#[derive(Debug, Clone, PartialEq)]
pub struct AggregateFit {
    pub assigned_observations: usize,
    pub period_seconds_per_quarter: f64,
    pub bpm: f64,
    pub reference_count_numerator: i64,
    pub quarter_note_denominator: u32,
    pub fitted_seconds_at_reference: f64,
    pub diagnostic_intercept_seconds: f64,
    pub period_sensitivity_bound_seconds: f64,
    pub max_abs_residual_seconds: f64,
    pub residual_range_seconds: f64,
    pub numerical_tolerance_seconds: f64,
}

/// One fixed temporal candidate, including reasons for rejecting a fitted island.
/// Status can only be unverified or unsupported; numerical metadata is never
/// a verified quarter-note interpretation or global timing publication.
#[derive(Debug, Clone, PartialEq)]
pub struct RepresentativeRegion {
    pub id: &'static str,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub assigned_positions: usize,
    pub raw_positions: usize,
    pub inlier_raw_indices: Vec<usize>,
    pub excluded_raw_indices: Vec<usize>,
    pub status: &'static str,
    pub reasons: Vec<&'static str>,
    pub fit: Option<AggregateFit>,
    pub(crate) observed_span_seconds: f64,
}

/// Immutable complete aggregation, global G2 assessment and independent region
/// metadata. A viable region never overrides an unsupported global assessment.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedBpmSummary {
    pub beat_unit: &'static str,
    pub count_provenance: String,
    pub sample_rate_hz: u32,
    pub frame_count: u64,
    pub raw_beat_seconds: Vec<f64>,
    pub quarter_counts: Vec<Option<i64>>,
    pub quarter_note_denominator: u32,
    pub local_interval_seconds: Vec<f64>,
    pub local_interval_bpm: Vec<Option<f64>>,
    pub complete_fit: Option<AggregateFit>,
    pub complete_residual_seconds: Vec<Option<f64>>,
    pub global: HypothesisDiagnostic,
    pub regions: Vec<RepresentativeRegion>,
    pub selected_region_id: Option<&'static str>,
    pub representative_bpm: Option<f64>,
    /// Diagnostic half/base/double alternatives of the complete count fit.
    pub alternatives_bpm: [Option<f64>; 3],
}
