//! Immutable diagnostic contracts for the narrow repeated-attack policy.

use std::{error::Error, fmt};

use crate::tempo_summary::{QuarterNoteHypothesis, QuarterNoteVerification, SourceIdentity};

/// A conservative discrete PCM feature policy, not general musical acceptance.
pub const POLICY_VERSION: &str = "isolated-comparable-attack-v1";
pub const MAX_PCM_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_ATTACKS: usize = 250_000;
pub const MAX_SEARCH_HALFWIDTH_SECONDS: f64 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefinementStatus {
    Unsupported,
    ComparableAttacks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefinementRejection {
    InsufficientAttacks,
    ActiveEventTooLong,
    IncompleteLeadingSilence,
    IncompleteTrailingSilence,
    DifferentAttackShapes,
    MultipleAttackMatches,
    MultipleRawMatches,
}

/// One independently scanned PCM threshold feature in loaded-source coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct ComparableAttack {
    pub frame: u64,
    pub seconds: f64,
    pub end_frame_exclusive: u64,
    /// Source frame zero supplies no observation of preceding silence.
    pub source_boundary: bool,
}

/// Every original detector index is retained, including unassociated extras.
#[derive(Debug, Clone, PartialEq)]
pub struct RawAttackAssociation {
    pub raw_index: usize,
    pub original_seconds: f64,
    pub attack_index: Option<usize>,
    pub refined_seconds: Option<f64>,
    pub displacement_seconds: Option<f64>,
}

/// An explicit caller assertion of independently established musical counts.
///
/// Repetition, detector outputs and a successful fit do not establish this
/// assertion. The API checks its complete source/feature correspondence only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndependentQuarterEvidence {
    pub source_sha256: String,
    pub pcm_sha256: String,
    pub loaded_sample_rate_hz: u32,
    pub loaded_frame_count: u64,
    pub feature_policy_version: String,
    pub feature_frames: Vec<u64>,
    pub quarter_count_numerators: Vec<i64>,
    pub quarter_note_denominator: u32,
    pub provenance: String,
}

/// Owned count hypotheses for both complete attacks and original raw positions.
#[derive(Debug, Clone, PartialEq)]
pub struct CountProposal {
    pub id: String,
    pub provenance: String,
    pub verification: QuarterNoteVerification,
    pub quarter_note_denominator: u32,
    pub attack_quarter_counts: Vec<Option<i64>>,
    pub raw_quarter_counts: Vec<Option<i64>>,
}

impl CountProposal {
    /// Borrow the complete PCM feature mapping for the independent fit core.
    pub fn as_attack_hypothesis(&self) -> QuarterNoteHypothesis<'_> {
        self.hypothesis(&self.attack_quarter_counts)
    }

    /// Borrow the original raw mapping, including explicit unassociated extras.
    pub fn as_raw_hypothesis(&self) -> QuarterNoteHypothesis<'_> {
        self.hypothesis(&self.raw_quarter_counts)
    }

    fn hypothesis<'a>(&'a self, counts: &'a [Option<i64>]) -> QuarterNoteHypothesis<'a> {
        QuarterNoteHypothesis {
            id: &self.id,
            provenance: &self.provenance,
            verification: self.verification,
            quarter_note_denominator: self.quarter_note_denominator,
            quarter_counts: counts,
        }
    }
}

/// Complete offline feature/refinement evidence; no runtime timing is published.
#[derive(Debug, Clone, PartialEq)]
pub struct TempoRefinement {
    pub policy_version: &'static str,
    pub status: RefinementStatus,
    pub rejection_reasons: Vec<RefinementRejection>,
    pub original_source: SourceIdentity,
    /// A feature revision/bound is assigned only when this policy supports it.
    pub refined_source: SourceIdentity,
    /// Conditional on comparable discrete features; absent for unsupported PCM.
    pub feature_timing_error_halfwidth_seconds: Option<f64>,
    pub independent_origin_seconds: f64,
    pub raw_beat_seconds: Vec<f64>,
    pub search_halfwidth_seconds: f64,
    pub attacks: Vec<ComparableAttack>,
    pub raw_associations: Vec<RawAttackAssociation>,
    pub unmatched_attack_indices: Vec<usize>,
    pub attack_shape_sha256: Option<String>,
    pub proposals: Vec<CountProposal>,
}

impl TempoRefinement {
    /// Materialize every feature second without replacing the original raw array.
    pub fn attack_seconds(&self) -> Vec<f64> {
        self.attacks.iter().map(|attack| attack.seconds).collect()
    }
}

/// Malformed identities/evidence differ from valid but unsupported PCM features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempoRefinementError(pub(crate) &'static str);

impl fmt::Display for TempoRefinementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for TempoRefinementError {}
