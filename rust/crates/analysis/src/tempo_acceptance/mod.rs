//! Immutable accepted constant timing, constructed entirely outside realtime paths.
//!
//! Acceptance requires fresh assessment of owned bound evidence and an explicit
//! caller decision. It does not select a musical policy, publish timing to the
//! engine or certify acoustic synchronization. No mutable summary/refinement can
//! be passed into construction. Current pad/intent validity is a separate guard.

mod adoption;
mod identity;
mod types;

use crate::tempo_evidence::{BoundTempoEvidence, PcmBinding, PcmBindingMetadata};
use crate::tempo_refinement::{
    IndependentQuarterEvidence, RefinementStatus, TempoRefinement, refine_comparable_attacks,
};
use crate::tempo_summary::{
    PeriodFit, QuarterNoteHypothesis, RawTempoEvidence, SummaryStatus, TempoSummary,
    summarize_constant_tempo,
};

pub use adoption::*;
pub use types::*;

/// Owned accepted timing and its complete evidence, with no mutation interface.
///
/// The authoritative period is the fitted binary64 seconds per quarter. No BPM
/// conversion, integer rounding or binary32 projection participates in acceptance.
#[derive(Debug, Clone)]
pub struct AcceptedConstantTiming {
    revision: String,
    period_seconds_per_quarter: f64,
    origin: IndependentTimingOrigin,
    evidence: BoundTempoEvidence,
    summary: TempoSummary,
    refinement: Option<TempoRefinement>,
    independent_quarters: Option<IndependentQuarterEvidence>,
    decision: TimingAcceptanceDecision,
}

impl AcceptedConstantTiming {
    /// Recompute complete raw/count assessment and accept only supported evidence.
    ///
    /// Hypotheses are copied into the fresh summary. The origin is independently
    /// selected rather than taken from the fitted intercept or detector evidence.
    /// The decision is a caller assertion; numerical quality cannot supply it.
    pub fn from_raw(
        evidence: BoundTempoEvidence,
        hypotheses: &[QuarterNoteHypothesis<'_>],
        origin: IndependentTimingOrigin,
        decision: TimingAcceptanceDecision,
    ) -> Result<Self, TempoAcceptanceError> {
        validate_assertions(&origin, &decision)?;
        let summary = summarize_constant_tempo(
            &RawTempoEvidence {
                source: evidence.source_identity(),
                beat_seconds: evidence.beat_seconds(),
                independent_origin_seconds: origin.seconds,
            },
            hypotheses,
        )?;
        Self::from_fresh_assessment(evidence, summary, None, None, origin, decision)
    }

    /// Verify exact PCM lineage, freshly refine complete attacks, then assess counts.
    ///
    /// The independent quarter assertion must match the complete feature sequence.
    /// This narrow discrete feature policy does not verify general musical onset
    /// truth. It retains the original detector evidence beside complete refinement.
    pub fn from_comparable_attacks(
        evidence: BoundTempoEvidence,
        binding: &PcmBinding<'_>,
        search_halfwidth_seconds: f64,
        independent_quarters: &IndependentQuarterEvidence,
        origin: IndependentTimingOrigin,
        decision: TimingAcceptanceDecision,
    ) -> Result<Self, TempoAcceptanceError> {
        validate_assertions(&origin, &decision)?;
        check_exact_binding(evidence.binding(), binding.metadata())?;
        let refinement = refine_comparable_attacks(
            evidence.source_identity(),
            binding.samples(),
            evidence.beat_seconds(),
            origin.seconds,
            search_halfwidth_seconds,
            Some(independent_quarters),
        )?;
        if refinement.status != RefinementStatus::ComparableAttacks {
            return Err(TempoAcceptanceError::UnsupportedRefinement);
        }
        let attack_seconds = refinement.attack_seconds();
        let hypotheses: Vec<_> = refinement
            .proposals
            .iter()
            .map(|proposal| proposal.as_attack_hypothesis())
            .collect();
        let summary = summarize_constant_tempo(
            &RawTempoEvidence {
                source: &refinement.refined_source,
                beat_seconds: &attack_seconds,
                independent_origin_seconds: origin.seconds,
            },
            &hypotheses,
        )?;
        Self::from_fresh_assessment(
            evidence,
            summary,
            Some(refinement),
            Some(independent_quarters.clone()),
            origin,
            decision,
        )
    }

    fn from_fresh_assessment(
        evidence: BoundTempoEvidence,
        summary: TempoSummary,
        refinement: Option<TempoRefinement>,
        independent_quarters: Option<IndependentQuarterEvidence>,
        origin: IndependentTimingOrigin,
        decision: TimingAcceptanceDecision,
    ) -> Result<Self, TempoAcceptanceError> {
        if summary.status != SummaryStatus::SupportedCandidate {
            return Err(TempoAcceptanceError::CandidateStatus(summary.status));
        }
        let fit = summary
            .supported_hypothesis_index
            .and_then(|index| summary.hypotheses.get(index))
            .and_then(|hypothesis| hypothesis.fit.as_ref())
            .ok_or(TempoAcceptanceError::InvalidSupportedFit)?;
        validate_fit(fit)?;
        let mut accepted = Self {
            revision: String::new(),
            period_seconds_per_quarter: fit.period_seconds_per_quarter,
            origin,
            evidence,
            summary,
            refinement,
            independent_quarters,
            decision,
        };
        accepted.revision = identity::revision(&accepted);
        Ok(accepted)
    }

    /// Versioned SHA-256 identity binding all retained evidence and accepted choices.
    pub fn revision(&self) -> &str {
        &self.revision
    }

    /// Authoritative fitted binary64 seconds per musical quarter.
    pub fn period_seconds_per_quarter(&self) -> f64 {
        self.period_seconds_per_quarter
    }

    /// Independently chosen grid origin and its explicit caller provenance.
    pub fn origin(&self) -> &IndependentTimingOrigin {
        &self.origin
    }

    /// Complete owned source/PCM/job/backend evidence prior to any refinement.
    pub fn evidence(&self) -> &BoundTempoEvidence {
        &self.evidence
    }

    /// Fresh complete assessment with counts, all hypotheses and error diagnostics.
    pub fn summary(&self) -> &TempoSummary {
        &self.summary
    }

    /// Fresh complete refinement when acceptance used comparable PCM attacks.
    pub fn refinement(&self) -> Option<&TempoRefinement> {
        self.refinement.as_ref()
    }

    /// Complete independent quarter assertion retained by PCM-based acceptance.
    pub fn independent_quarters(&self) -> Option<&IndependentQuarterEvidence> {
        self.independent_quarters.as_ref()
    }

    /// Explicit acceptance policy and caller decision provenance.
    pub fn decision(&self) -> &TimingAcceptanceDecision {
        &self.decision
    }

    /// Compare every retained binding field against caller-supplied current metadata.
    ///
    /// This pure comparison cannot read engine state or certify current-pad
    /// validity. A later publication caller must supply its authoritative current
    /// metadata and separately reject retired jobs or changed timing intent.
    pub fn check_binding(&self, current: &PcmBindingMetadata) -> Result<(), TempoAcceptanceError> {
        check_exact_binding(self.evidence.binding(), current)
    }
}

fn check_exact_binding(
    retained: &PcmBindingMetadata,
    current: &PcmBindingMetadata,
) -> Result<(), TempoAcceptanceError> {
    if retained.job != current.job
        || retained.source_sha256 != current.source_sha256
        || retained.source_provenance != current.source_provenance
        || retained.pcm_sha256 != current.pcm_sha256
        || retained.sample_rate_hz != current.sample_rate_hz
        || retained.frame_count != current.frame_count
        || retained.origin_seconds.to_bits() != current.origin_seconds.to_bits()
        || retained.mono_revision != current.mono_revision
    {
        return Err(TempoAcceptanceError::BindingMismatch);
    }
    Ok(())
}

fn validate_assertions(
    origin: &IndependentTimingOrigin,
    decision: &TimingAcceptanceDecision,
) -> Result<(), TempoAcceptanceError> {
    let valid_text = |value: &str| !value.trim().is_empty() && value.len() <= 4096;
    if !origin.seconds.is_finite() || !valid_text(&origin.provenance) {
        return Err(TempoAcceptanceError::InvalidOrigin);
    }
    if !valid_text(&decision.policy_version) || !valid_text(&decision.provenance) {
        return Err(TempoAcceptanceError::InvalidDecision);
    }
    Ok(())
}

fn validate_fit(fit: &PeriodFit) -> Result<(), TempoAcceptanceError> {
    if !fit.period_seconds_per_quarter.is_finite()
        || fit.period_seconds_per_quarter <= 0.0
        || !fit.fitted_seconds_at_reference.is_finite()
        || !fit.diagnostic_intercept_seconds.is_finite()
        || [
            fit.period_sensitivity_bound_seconds,
            fit.max_abs_inlier_residual_seconds,
            fit.inlier_residual_range_seconds,
            fit.window_period_spread_seconds,
            fit.numerical_tolerance_seconds,
        ]
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err(TempoAcceptanceError::InvalidSupportedFit);
    }
    Ok(())
}
