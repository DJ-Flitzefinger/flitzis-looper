//! Explicit event correspondence and musical-unit provenance, never tempo snapping.

use crate::tempo_summary::{QuarterNoteVerification, SourceIdentity};

use super::{
    ComparableAttack, CountProposal, IndependentQuarterEvidence, POLICY_VERSION,
    RawAttackAssociation, RefinementRejection, TempoRefinementError,
};

pub(super) fn associate(
    raw: &[f64],
    attacks: &[ComparableAttack],
    halfwidth: f64,
) -> (Vec<RawAttackAssociation>, Vec<RefinementRejection>) {
    let mut associations = Vec::with_capacity(raw.len());
    let mut uses = vec![0_usize; attacks.len()];
    let mut reasons = Vec::new();
    let mut first_possible = 0;
    for (raw_index, &original_seconds) in raw.iter().enumerate() {
        while first_possible < attacks.len()
            && attacks[first_possible].seconds < original_seconds - halfwidth
        {
            first_possible += 1;
        }
        let mut candidates = 0;
        let mut candidate = None;
        for (index, attack) in attacks.iter().enumerate().skip(first_possible) {
            if attack.seconds > original_seconds + halfwidth {
                break;
            }
            candidates += 1;
            candidate = Some(index);
            if candidates == 2 {
                break;
            }
        }
        if candidates > 1 {
            if !reasons.contains(&RefinementRejection::MultipleAttackMatches) {
                reasons.push(RefinementRejection::MultipleAttackMatches);
            }
            candidate = None;
        }
        if let Some(index) = candidate {
            uses[index] += 1;
        }
        associations.push(RawAttackAssociation {
            raw_index,
            original_seconds,
            attack_index: candidate,
            refined_seconds: candidate.map(|index| attacks[index].seconds),
            displacement_seconds: candidate.map(|index| attacks[index].seconds - original_seconds),
        });
    }
    if uses.iter().any(|&count| count > 1) {
        reasons.push(RefinementRejection::MultipleRawMatches);
        for association in &mut associations {
            if association
                .attack_index
                .is_some_and(|index| uses[index] > 1)
            {
                association.attack_index = None;
                association.refined_seconds = None;
                association.displacement_seconds = None;
            }
        }
    }
    (associations, reasons)
}

pub(super) fn proposals(
    source: &SourceIdentity,
    attacks: &[ComparableAttack],
    associations: &[RawAttackAssociation],
    independent: Option<&IndependentQuarterEvidence>,
) -> Result<Vec<CountProposal>, TempoRefinementError> {
    if let Some(evidence) = independent {
        validate_independent(source, attacks, evidence)?;
        return Ok(vec![make_proposal(
            "independent-quarter-evidence",
            &evidence.provenance,
            QuarterNoteVerification::Verified,
            evidence.quarter_note_denominator,
            evidence
                .quarter_count_numerators
                .iter()
                .map(|&count| Some(count))
                .collect(),
            associations,
        )]);
    }
    Ok([
        ("attack-events-as-half-quarters", 1_i64, 2_u32),
        ("attack-events-as-quarters", 1, 1),
        ("attack-events-as-double-quarters", 2, 1),
    ]
    .into_iter()
    .map(|(id, numerator_scale, denominator)| {
        make_proposal(
            id,
            "comparable-PCM-event-ordinals; musical quarter unit independently unverified",
            QuarterNoteVerification::Unverified,
            denominator,
            (0..attacks.len())
                .map(|index| Some(index as i64 * numerator_scale))
                .collect(),
            associations,
        )
    })
    .collect())
}

pub(super) fn validate_independent(
    source: &SourceIdentity,
    attacks: &[ComparableAttack],
    evidence: &IndependentQuarterEvidence,
) -> Result<(), TempoRefinementError> {
    if evidence.source_sha256 != source.source_sha256
        || evidence.pcm_sha256 != source.pcm_sha256
        || evidence.loaded_sample_rate_hz != source.loaded_sample_rate_hz
        || evidence.loaded_frame_count != source.loaded_frame_count
        || evidence.feature_policy_version != POLICY_VERSION
        || evidence.feature_frames.len() != attacks.len()
        || evidence.quarter_count_numerators.len() != attacks.len()
        || !(1..=64).contains(&evidence.quarter_note_denominator)
        || evidence.provenance.trim().is_empty()
        || evidence.provenance.len() > 4096
        || evidence
            .feature_frames
            .iter()
            .zip(attacks)
            .any(|(&frame, attack)| frame != attack.frame)
    {
        return Err(TempoRefinementError(
            "independent quarter evidence does not match complete source features",
        ));
    }
    let mut previous = None;
    let first = evidence.quarter_count_numerators.first().copied();
    for &count in &evidence.quarter_count_numerators {
        if count.unsigned_abs() > (1_u64 << 52)
            || previous.is_some_and(|before| count <= before)
            || first.is_some_and(|begin| i128::from(count) - i128::from(begin) > (1_i128 << 52))
        {
            return Err(TempoRefinementError(
                "invalid independent quarter count coordinates",
            ));
        }
        previous = Some(count);
    }
    Ok(())
}

fn make_proposal(
    id: &str,
    provenance: &str,
    verification: QuarterNoteVerification,
    denominator: u32,
    attack_quarter_counts: Vec<Option<i64>>,
    associations: &[RawAttackAssociation],
) -> CountProposal {
    let raw_quarter_counts = associations
        .iter()
        .map(|association| {
            association
                .attack_index
                .and_then(|index| attack_quarter_counts[index])
        })
        .collect();
    CountProposal {
        id: id.into(),
        provenance: provenance.into(),
        verification,
        quarter_note_denominator: denominator,
        attack_quarter_counts,
        raw_quarter_counts,
    }
}
