//! Canonical accepted identity; field order and explicit enum tags belong to v1.

use crate::canonical_digest::CanonicalDigest;
use crate::tempo_refinement::{
    CountProposal, IndependentQuarterEvidence, RefinementRejection, RefinementStatus,
    TempoRefinement,
};
use crate::tempo_summary::{
    FitRegion, HypothesisDiagnostic, HypothesisStatus, PeriodFit, QuarterNoteVerification,
    RejectionReason, SourceIdentity, SummaryStatus, TempoSummary, WindowDiagnostic,
};

use super::{AcceptedConstantTiming, TIMING_REVISION_VERSION};

pub(super) fn revision(timing: &AcceptedConstantTiming) -> String {
    let mut hash = CanonicalDigest::new(TIMING_REVISION_VERSION);
    source(&mut hash, timing.evidence.source_identity());
    timing.evidence.hash_canonical(&mut hash);
    summary(&mut hash, &timing.summary);
    hash.optional(timing.refinement.as_ref(), refinement);
    hash.optional(timing.independent_quarters.as_ref(), independent_quarters);
    hash.float(timing.period_seconds_per_quarter);
    hash.float(timing.origin.seconds);
    hash.text(&timing.origin.provenance);
    hash.text(&timing.decision.policy_version);
    hash.text(&timing.decision.provenance);
    format!("{TIMING_REVISION_VERSION}:{}", hash.finish())
}

fn source(hash: &mut CanonicalDigest, value: &SourceIdentity) {
    hash.text(&value.source_sha256);
    hash.text(&value.pcm_sha256);
    hash.number(u64::from(value.loaded_sample_rate_hz));
    hash.number(value.loaded_frame_count);
    hash.text(&value.backend_revision);
    hash.text(&value.configuration_revision);
    hash.text(&value.raw_revision);
    hash.float(value.timing_error_halfwidth_seconds);
}

fn summary(hash: &mut CanonicalDigest, value: &TempoSummary) {
    source(hash, &value.source);
    hash.float(value.independent_origin_seconds);
    hash.text(value.policy_version);
    hash.number(value.raw_position_count as u64);
    hash.float_array(&value.raw_beat_seconds);
    hash.number(match value.status {
        SummaryStatus::Unsupported => 0,
        SummaryStatus::Unverified => 1,
        SummaryStatus::Ambiguous => 2,
        SummaryStatus::SupportedCandidate => 3,
    });
    hash.optional(value.supported_hypothesis_index, |hash, index| {
        hash.number(index as u64);
    });
    hash.sequence(&value.hypotheses, hypothesis);
}

fn verification(hash: &mut CanonicalDigest, value: QuarterNoteVerification) {
    hash.number(match value {
        QuarterNoteVerification::Unverified => 0,
        QuarterNoteVerification::Verified => 1,
    });
}

fn counts(hash: &mut CanonicalDigest, values: &[Option<i64>]) {
    hash.sequence(values, |hash, value| {
        hash.optional(*value, |hash, count| hash.signed(count));
    });
}

fn indices(hash: &mut CanonicalDigest, values: &[usize]) {
    hash.sequence(values, |hash, value| hash.number(*value as u64));
}

fn hypothesis(hash: &mut CanonicalDigest, value: &HypothesisDiagnostic) {
    hash.text(&value.id);
    hash.text(&value.provenance);
    verification(hash, value.verification);
    hash.number(u64::from(value.quarter_note_denominator));
    counts(hash, &value.quarter_counts);
    hash.number(match value.status {
        HypothesisStatus::Unsupported => 0,
        HypothesisStatus::Unverified => 1,
        HypothesisStatus::SupportedCandidate => 2,
    });
    hash.sequence(&value.reasons, |hash, reason| {
        hash.number(match reason {
            RejectionReason::InsufficientPositions => 0,
            RejectionReason::InsufficientTemporalCoverage => 1,
            RejectionReason::NoRobustFit => 2,
            RejectionReason::TooManyExclusions => 3,
            RejectionReason::ConsecutiveExclusions => 4,
            RejectionReason::InconsistentWindowPeriods => 5,
            RejectionReason::InconsistentWindowOffsets => 6,
            RejectionReason::InconsistentTimingBound => 7,
            RejectionReason::UnverifiedQuarterNotes => 8,
        });
    });
    hash.optional(value.selected_region, |hash, region| {
        hash.number(match region {
            FitRegion::Complete => 0,
            FitRegion::Middle => 1,
        });
    });
    hash.optional(value.fit.as_ref(), fit);
    hash.sequence(&value.residual_seconds, |hash, residual| {
        hash.optional(*residual, |hash, value| hash.float(value));
    });
    indices(hash, &value.inlier_raw_indices);
    indices(hash, &value.excluded_raw_indices);
    hash.sequence(&value.windows, window);
}

fn fit(hash: &mut CanonicalDigest, value: &PeriodFit) {
    hash.float(value.period_seconds_per_quarter);
    hash.signed(value.reference_count_numerator);
    hash.number(u64::from(value.quarter_note_denominator));
    hash.float(value.fitted_seconds_at_reference);
    hash.float(value.diagnostic_intercept_seconds);
    hash.float(value.period_sensitivity_bound_seconds);
    hash.float(value.max_abs_inlier_residual_seconds);
    hash.float(value.inlier_residual_range_seconds);
    hash.float(value.window_period_spread_seconds);
    hash.float(value.numerical_tolerance_seconds);
}

fn window(hash: &mut CanonicalDigest, value: &WindowDiagnostic) {
    hash.float(value.start_seconds);
    hash.float(value.end_seconds);
    hash.number(value.assigned_positions as u64);
    hash.number(value.global_inlier_positions as u64);
    hash.number(value.inlier_positions as u64);
    hash.optional(value.period_seconds_per_quarter, |hash, value| {
        hash.float(value)
    });
    hash.optional(value.period_sensitivity_bound_seconds, |hash, value| {
        hash.float(value);
    });
    hash.optional(value.median_global_residual_seconds, |hash, value| {
        hash.float(value);
    });
}

fn refinement(hash: &mut CanonicalDigest, value: &TempoRefinement) {
    hash.text(value.policy_version);
    hash.number(match value.status {
        RefinementStatus::Unsupported => 0,
        RefinementStatus::ComparableAttacks => 1,
    });
    hash.sequence(&value.rejection_reasons, |hash, reason| {
        hash.number(match reason {
            RefinementRejection::InsufficientAttacks => 0,
            RefinementRejection::ActiveEventTooLong => 1,
            RefinementRejection::IncompleteLeadingSilence => 2,
            RefinementRejection::IncompleteTrailingSilence => 3,
            RefinementRejection::DifferentAttackShapes => 4,
            RefinementRejection::MultipleAttackMatches => 5,
            RefinementRejection::MultipleRawMatches => 6,
        });
    });
    source(hash, &value.original_source);
    source(hash, &value.refined_source);
    hash.optional(
        value.feature_timing_error_halfwidth_seconds,
        |hash, value| {
            hash.float(value);
        },
    );
    hash.float(value.independent_origin_seconds);
    hash.float_array(&value.raw_beat_seconds);
    hash.float(value.search_halfwidth_seconds);
    hash.sequence(&value.attacks, |hash, attack| {
        hash.number(attack.frame);
        hash.float(attack.seconds);
        hash.number(attack.end_frame_exclusive);
        hash.number(u64::from(attack.source_boundary));
    });
    hash.sequence(&value.raw_associations, |hash, association| {
        hash.number(association.raw_index as u64);
        hash.float(association.original_seconds);
        hash.optional(association.attack_index, |hash, index| {
            hash.number(index as u64)
        });
        hash.optional(association.refined_seconds, |hash, seconds| {
            hash.float(seconds)
        });
        hash.optional(association.displacement_seconds, |hash, seconds| {
            hash.float(seconds);
        });
    });
    indices(hash, &value.unmatched_attack_indices);
    hash.optional(value.attack_shape_sha256.as_deref(), |hash, digest| {
        hash.text(digest);
    });
    hash.sequence(&value.proposals, proposal);
}

fn proposal(hash: &mut CanonicalDigest, value: &CountProposal) {
    hash.text(&value.id);
    hash.text(&value.provenance);
    verification(hash, value.verification);
    hash.number(u64::from(value.quarter_note_denominator));
    counts(hash, &value.attack_quarter_counts);
    counts(hash, &value.raw_quarter_counts);
}

fn independent_quarters(hash: &mut CanonicalDigest, value: &IndependentQuarterEvidence) {
    hash.text(&value.source_sha256);
    hash.text(&value.pcm_sha256);
    hash.number(u64::from(value.loaded_sample_rate_hz));
    hash.number(value.loaded_frame_count);
    hash.text(&value.feature_policy_version);
    hash.sequence(&value.feature_frames, |hash, frame| hash.number(*frame));
    hash.sequence(&value.quarter_count_numerators, |hash, count| {
        hash.signed(*count)
    });
    hash.number(u64::from(value.quarter_note_denominator));
    hash.text(&value.provenance);
}
