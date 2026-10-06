//! Public evidence and diagnostic types for offline constant-period candidates.

use std::{error::Error, fmt};

/// Frozen engineering policy; this is not a musical-quality acceptance threshold.
pub const POLICY_VERSION: &str = "constant-period-candidate-v1";
pub const MAX_RAW_POSITIONS: usize = 250_000;
pub const MAX_HYPOTHESES: usize = 8;

/// Identity of the complete loaded source and the raw detector result.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceIdentity {
    pub source_sha256: String,
    pub pcm_sha256: String,
    pub loaded_sample_rate_hz: u32,
    pub loaded_frame_count: u64,
    pub backend_revision: String,
    pub configuration_revision: String,
    pub raw_revision: String,
    /// Declared bound on each detector position, not a fitted confidence interval.
    pub timing_error_halfwidth_seconds: f64,
}

/// Complete immutable detector evidence; no cropping or origin replacement occurs.
#[derive(Debug, Clone, Copy)]
pub struct RawTempoEvidence<'a> {
    pub source: &'a SourceIdentity,
    pub beat_seconds: &'a [f64],
    /// An independently chosen source-grid origin, including valid signed origins.
    pub independent_origin_seconds: f64,
}

/// Whether the caller asserts an independently evidenced quarter-note interpretation.
/// This module validates the assertion's shape, not its musical truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuarterNoteVerification {
    Verified,
    Unverified,
}

/// Explicit rational quarter-note counts associated with every original raw position.
///
/// `None` retains an explicitly excluded extra detection. Missing beats instead
/// appear as positive count jumps. The fitter never inserts or renumbers counts.
#[derive(Debug, Clone, Copy)]
pub struct QuarterNoteHypothesis<'a> {
    pub id: &'a str,
    pub provenance: &'a str,
    pub verification: QuarterNoteVerification,
    /// Each integer numerator below represents count / denominator quarter notes.
    /// Values 1..=64 permit explicit subdivision and half-tempo interpretations.
    pub quarter_note_denominator: u32,
    pub quarter_counts: &'a [Option<i64>],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryStatus {
    Unsupported,
    Unverified,
    Ambiguous,
    SupportedCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HypothesisStatus {
    Unsupported,
    Unverified,
    SupportedCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionReason {
    InsufficientPositions,
    InsufficientTemporalCoverage,
    NoRobustFit,
    TooManyExclusions,
    ConsecutiveExclusions,
    InconsistentWindowPeriods,
    InconsistentWindowOffsets,
    InconsistentTimingBound,
    UnverifiedQuarterNotes,
}

/// A temporal seed selected before fitting; full-source checks always follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitRegion {
    Complete,
    Middle,
}

/// A fit in centered coordinates. Neither intercept changes the independent origin.
#[derive(Debug, Clone, PartialEq)]
pub struct PeriodFit {
    pub period_seconds_per_quarter: f64,
    pub reference_count_numerator: i64,
    pub quarter_note_denominator: u32,
    pub fitted_seconds_at_reference: f64,
    pub diagnostic_intercept_seconds: f64,
    /// Worst-case OLS slope sensitivity to the declared per-position error bound.
    /// It is conditional on supplied counts/inliers and is not statistical confidence.
    pub period_sensitivity_bound_seconds: f64,
    pub max_abs_inlier_residual_seconds: f64,
    pub inlier_residual_range_seconds: f64,
    pub window_period_spread_seconds: f64,
    pub numerical_tolerance_seconds: f64,
}

/// Independent early/middle/late temporal-window fit, retaining unsupported windows.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowDiagnostic {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub assigned_positions: usize,
    pub global_inlier_positions: usize,
    pub inlier_positions: usize,
    pub period_seconds_per_quarter: Option<f64>,
    pub period_sensitivity_bound_seconds: Option<f64>,
    pub median_global_residual_seconds: Option<f64>,
}

/// Derived result for one count hypothesis, with every original identity retained.
#[derive(Debug, Clone, PartialEq)]
pub struct HypothesisDiagnostic {
    pub id: String,
    pub provenance: String,
    pub verification: QuarterNoteVerification,
    pub quarter_note_denominator: u32,
    pub quarter_counts: Vec<Option<i64>>,
    pub status: HypothesisStatus,
    pub reasons: Vec<RejectionReason>,
    pub selected_region: Option<FitRegion>,
    pub fit: Option<PeriodFit>,
    /// Same extent as the raw sequence; unassigned positions have no residual.
    pub residual_seconds: Vec<Option<f64>>,
    pub inlier_raw_indices: Vec<usize>,
    pub excluded_raw_indices: Vec<usize>,
    pub windows: Vec<WindowDiagnostic>,
}

/// Source-bound offline candidate evidence, never accepted musical/runtime timing.
#[derive(Debug, Clone, PartialEq)]
pub struct TempoSummary {
    pub source: SourceIdentity,
    pub independent_origin_seconds: f64,
    pub policy_version: &'static str,
    pub raw_position_count: usize,
    pub raw_beat_seconds: Vec<f64>,
    pub status: SummaryStatus,
    /// Populated only for a uniquely supported verified count interpretation.
    pub supported_hypothesis_index: Option<usize>,
    pub hypotheses: Vec<HypothesisDiagnostic>,
}

/// Invalid evidence/configuration is distinct from a valid but unsupported fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempoSummaryError(pub(crate) &'static str);

impl fmt::Display for TempoSummaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for TempoSummaryError {}
