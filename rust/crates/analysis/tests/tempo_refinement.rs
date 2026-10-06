//! Public signal/count evidence gates against independently constructed PCM.

use flitzis_looper_analysis::{
    tempo_refinement::{
        IndependentQuarterEvidence, POLICY_VERSION, RefinementRejection, RefinementStatus,
        TempoRefinement, refine_comparable_attacks,
    },
    tempo_summary::{
        QuarterNoteVerification, RawTempoEvidence, SourceIdentity, SummaryStatus,
        summarize_constant_tempo,
    },
};
use sha2::{Digest, Sha256};

const COUNT: usize = 64;
const SHAPE: [f32; 8] = [0.5, -0.25, 0.0, 0.125, -0.0625, 0.0, 0.03125, -0.015625];

fn digest(pcm: &[f32]) -> String {
    let mut hash = Sha256::new();
    for sample in pcm {
        hash.update(sample.to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn fixture(
    rate: u32,
    quarter_times: &[f64],
    duration: f64,
) -> (SourceIdentity, Vec<f32>, Vec<u64>) {
    let frames: Vec<_> = quarter_times
        .iter()
        .map(|time| (time * f64::from(rate)).round() as u64)
        .collect();
    let mut pcm = vec![0.0; (duration * f64::from(rate)).ceil() as usize];
    for &frame in &frames {
        pcm[frame as usize..frame as usize + SHAPE.len()].copy_from_slice(&SHAPE);
    }
    let identity = SourceIdentity {
        source_sha256: "1".repeat(64),
        pcm_sha256: digest(&pcm),
        loaded_sample_rate_hz: rate,
        loaded_frame_count: pcm.len() as u64,
        backend_revision: "independent-PCM-fixture-v1".into(),
        configuration_revision: "complete-native-mono-no-crop-v1".into(),
        raw_revision: "independently-generated-detector-events-v1".into(),
        timing_error_halfwidth_seconds: 0.01,
    };
    (identity, pcm, frames)
}

fn quarters(source: &SourceIdentity, frames: &[u64]) -> IndependentQuarterEvidence {
    IndependentQuarterEvidence {
        source_sha256: source.source_sha256.clone(),
        pcm_sha256: source.pcm_sha256.clone(),
        loaded_sample_rate_hz: source.loaded_sample_rate_hz,
        loaded_frame_count: source.loaded_frame_count,
        feature_policy_version: POLICY_VERSION.into(),
        feature_frames: frames.to_vec(),
        quarter_count_numerators: (0..frames.len()).map(|index| index as i64).collect(),
        quarter_note_denominator: 1,
        provenance: "independent synthetic pulse/quarter construction; predictions not used".into(),
    }
}

fn assess(refinement: &TempoRefinement) -> flitzis_looper_analysis::tempo_summary::TempoSummary {
    let seconds = refinement.attack_seconds();
    let hypotheses: Vec<_> = refinement
        .proposals
        .iter()
        .map(|proposal| proposal.as_attack_hypothesis())
        .collect();
    summarize_constant_tempo(
        &RawTempoEvidence {
            source: &refinement.refined_source,
            beat_seconds: &seconds,
            independent_origin_seconds: refinement.independent_origin_seconds,
        },
        &hypotheses,
    )
    .unwrap()
}

#[test]
fn complete_attack_scan_retains_missing_detector_zero_and_count_gaps_and_extras() {
    let rate = 48_000;
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    let (source, pcm, frames) = fixture(rate, &times, COUNT as f64 * 0.5);
    let mut raw: Vec<_> = times
        .iter()
        .enumerate()
        .filter_map(|(index, &time)| (index != 0 && index != 20).then_some(time + 0.002))
        .collect();
    raw.insert(8, 4.25);
    let evidence = quarters(&source, &frames);
    let report =
        refine_comparable_attacks(&source, &pcm, &raw, -0.125, 0.01, Some(&evidence)).unwrap();
    assert_eq!(report.status, RefinementStatus::ComparableAttacks);
    assert_eq!(report.policy_version, POLICY_VERSION);
    assert_eq!(report.raw_beat_seconds, raw);
    assert_eq!(report.original_source, source);
    assert_eq!(report.independent_origin_seconds, -0.125);
    assert_eq!(report.attacks.len(), COUNT);
    assert!(report.attacks[0].source_boundary);
    assert_eq!(report.unmatched_attack_indices, [0, 20]);
    assert_ne!(report.refined_source.raw_revision, source.raw_revision);
    assert_eq!(
        report.refined_source.timing_error_halfwidth_seconds,
        0.5 / 48_000.0
    );
    assert_eq!(report.raw_associations[8].attack_index, None);
    assert_eq!(report.proposals[0].raw_quarter_counts[8], None);
    let mapped: Vec<_> = report.proposals[0]
        .raw_quarter_counts
        .iter()
        .flatten()
        .copied()
        .collect();
    assert!(mapped.windows(2).any(|pair| pair == [19, 21]));
    assert_eq!(
        report.proposals[0].verification,
        QuarterNoteVerification::Verified
    );
    for association in &report.raw_associations {
        if let Some(index) = association.attack_index {
            assert_eq!(association.refined_seconds, Some(times[index]));
            assert!((association.displacement_seconds.unwrap() + 0.002).abs() < 1e-12);
        }
    }
    let summary = assess(&report);
    assert_eq!(summary.status, SummaryStatus::SupportedCandidate);
    assert_eq!(summary.raw_position_count, COUNT);
    assert_eq!(
        summary.hypotheses[0]
            .fit
            .as_ref()
            .unwrap()
            .period_seconds_per_quarter,
        0.5
    );
    assert_eq!(summary.independent_origin_seconds, -0.125);
    let raw_summary = summarize_constant_tempo(
        &RawTempoEvidence {
            source: &report.original_source,
            beat_seconds: &report.raw_beat_seconds,
            independent_origin_seconds: report.independent_origin_seconds,
        },
        &[report.proposals[0].as_raw_hypothesis()],
    )
    .unwrap();
    assert_eq!(raw_summary.status, SummaryStatus::SupportedCandidate);
    assert_eq!(raw_summary.hypotheses[0].excluded_raw_indices, [8]);
}

#[test]
fn comparable_repetition_does_not_verify_quarters_or_resolve_half_double_tempo() {
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    let (source, pcm, _) = fixture(48_000, &times, COUNT as f64 * 0.5);
    let report = refine_comparable_attacks(&source, &pcm, &times, 0.0, 0.01, None).unwrap();
    assert_eq!(report.status, RefinementStatus::ComparableAttacks);
    assert_eq!(report.proposals.len(), 3);
    assert!(
        report
            .proposals
            .iter()
            .all(|proposal| proposal.verification == QuarterNoteVerification::Unverified)
    );
    assert_eq!(report.proposals[0].quarter_note_denominator, 2);
    assert_eq!(report.proposals[0].attack_quarter_counts[1], Some(1));
    assert_eq!(report.proposals[2].attack_quarter_counts[1], Some(2));
    assert_eq!(assess(&report).status, SummaryStatus::Ambiguous);
    let seconds = report.attack_seconds();
    let single = summarize_constant_tempo(
        &RawTempoEvidence {
            source: &report.refined_source,
            beat_seconds: &seconds,
            independent_origin_seconds: 0.0,
        },
        &[report.proposals[1].as_attack_hypothesis()],
    )
    .unwrap();
    assert_eq!(single.status, SummaryStatus::Unverified);
}

#[test]
fn true_fractional_periods_survive_sample_quantization_at_all_loaded_rates() {
    for rate in [44_100, 48_000, 96_000] {
        for bpm in [119.999, 123.45] {
            let period = 60.0 / bpm;
            let times: Vec<_> = (0..COUNT).map(|index| index as f64 * period).collect();
            let (source, pcm, frames) = fixture(rate, &times, COUNT as f64 * period);
            let evidence = quarters(&source, &frames);
            let report =
                refine_comparable_attacks(&source, &pcm, &times, 0.0, 0.01, Some(&evidence))
                    .unwrap();
            assert_eq!(report.status, RefinementStatus::ComparableAttacks);
            let summary = assess(&report);
            assert_eq!(
                summary.status,
                SummaryStatus::SupportedCandidate,
                "{rate}/{bpm}: {summary:#?}"
            );
            let fitted = summary.hypotheses[0]
                .fit
                .as_ref()
                .unwrap()
                .period_seconds_per_quarter;
            let slope_frames = (fitted - period).abs() * (COUNT - 1) as f64 * f64::from(rate);
            assert!(slope_frames < 1.0, "{rate}/{bpm}: {slope_frames}");
            assert!((60.0 / fitted - bpm.round()).abs() > 0.0005);
        }
    }
}

#[test]
fn comparable_attacks_with_genuine_tempo_variation_remain_unsupported_by_constant_core() {
    let times: Vec<_> = (0..COUNT)
        .map(|index| index as f64 * 0.5 + (index as f64).powi(2) * 0.0002)
        .collect();
    let (source, pcm, frames) = fixture(48_000, &times, times[COUNT - 1] + 0.5);
    let evidence = quarters(&source, &frames);
    let report =
        refine_comparable_attacks(&source, &pcm, &times, 0.0, 0.01, Some(&evidence)).unwrap();
    assert_eq!(report.status, RefinementStatus::ComparableAttacks);
    assert_eq!(assess(&report).status, SummaryStatus::Unsupported);
}

#[test]
fn duplicate_raw_to_attack_matches_are_not_arbitrarily_deleted() {
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    let (source, pcm, _) = fixture(48_000, &times, COUNT as f64 * 0.5);
    let mut raw = times.clone();
    raw.insert(11, 5.002);
    let report = refine_comparable_attacks(&source, &pcm, &raw, 0.0, 0.01, None).unwrap();
    assert_eq!(report.status, RefinementStatus::Unsupported);
    assert!(
        report
            .rejection_reasons
            .contains(&RefinementRejection::MultipleRawMatches)
    );
    assert_eq!(report.raw_associations[10].attack_index, None);
    assert_eq!(report.raw_associations[11].attack_index, None);
    assert!(report.proposals.is_empty());
    assert_eq!(report.feature_timing_error_halfwidth_seconds, None);
    assert_eq!(report.refined_source, source);
    assert_eq!(report.raw_beat_seconds, raw);
}

#[test]
fn two_plausible_attack_matches_remain_unsupported() {
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.1).collect();
    let (source, pcm, _) = fixture(48_000, &times, COUNT as f64 * 0.1);
    let raw = [0.05];
    let report = refine_comparable_attacks(&source, &pcm, &raw, 0.0, 0.1, None).unwrap();
    assert_eq!(report.status, RefinementStatus::Unsupported);
    assert!(
        report
            .rejection_reasons
            .contains(&RefinementRejection::MultipleAttackMatches)
    );
    assert_eq!(report.raw_associations[0].attack_index, None);
    assert!(report.proposals.is_empty());
}

#[test]
fn shape_difference_sustained_audio_and_incomplete_boundary_evidence_reject_refinement() {
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    let (mut source, mut pcm, frames) = fixture(48_000, &times, COUNT as f64 * 0.5);
    pcm[frames[20] as usize + 1] = -0.24;
    source.pcm_sha256 = digest(&pcm);
    let report = refine_comparable_attacks(&source, &pcm, &times, 0.0, 0.01, None).unwrap();
    assert!(
        report
            .rejection_reasons
            .contains(&RefinementRejection::DifferentAttackShapes)
    );
    assert!(report.proposals.is_empty());

    let sustained = vec![0.5; 48_000];
    source.loaded_frame_count = sustained.len() as u64;
    source.pcm_sha256 = digest(&sustained);
    let report = refine_comparable_attacks(&source, &sustained, &[0.0], 0.0, 0.01, None).unwrap();
    assert!(
        report
            .rejection_reasons
            .contains(&RefinementRejection::ActiveEventTooLong)
    );
    assert!(
        report
            .rejection_reasons
            .contains(&RefinementRejection::IncompleteTrailingSilence)
    );

    let (mut source, mut pcm, _) = fixture(48_000, &[0.001, 0.501], 1.0);
    let report =
        refine_comparable_attacks(&source, &pcm, &[0.001, 0.501], 0.0, 0.01, None).unwrap();
    assert!(
        report
            .rejection_reasons
            .contains(&RefinementRejection::IncompleteLeadingSilence)
    );
    pcm.truncate(24_048 + SHAPE.len());
    source.loaded_frame_count = pcm.len() as u64;
    source.pcm_sha256 = digest(&pcm);
    let report =
        refine_comparable_attacks(&source, &pcm, &[0.001, 0.501], 0.0, 0.01, None).unwrap();
    assert!(
        report
            .rejection_reasons
            .contains(&RefinementRejection::IncompleteTrailingSilence)
    );
}

#[test]
fn mismatched_source_pcm_features_counts_and_bounds_are_errors() {
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    let (source, pcm, frames) = fixture(48_000, &times, COUNT as f64 * 0.5);
    let evidence = quarters(&source, &frames);
    let mut wrong_source = source.clone();
    wrong_source.pcm_sha256 = "2".repeat(64);
    assert!(refine_comparable_attacks(&wrong_source, &pcm, &times, 0.0, 0.01, None).is_err());
    for modified in 0..8 {
        let mut wrong = evidence.clone();
        match modified {
            0 => wrong.source_sha256 = "3".repeat(64),
            1 => wrong.pcm_sha256 = "3".repeat(64),
            2 => wrong.feature_frames[10] += 1,
            3 => wrong.quarter_count_numerators[10] = 9,
            4 => {
                wrong.feature_frames.pop();
            }
            5 => wrong.loaded_sample_rate_hz *= 2,
            6 => wrong.loaded_frame_count += 1,
            _ => wrong.feature_policy_version = "different-feature-policy".into(),
        }
        assert!(refine_comparable_attacks(&source, &pcm, &times, 0.0, 0.01, Some(&wrong)).is_err());
    }
    for halfwidth in [-0.1, 0.1001, f64::NAN] {
        assert!(refine_comparable_attacks(&source, &pcm, &times, 0.0, halfwidth, None).is_err());
    }
    let mut nonfinite = pcm.clone();
    nonfinite[3] = f32::NAN;
    assert!(refine_comparable_attacks(&source, &nonfinite, &times, 0.0, 0.01, None).is_err());
    assert!(refine_comparable_attacks(&source, &pcm, &[1.0, 0.5], 0.0, 0.01, None).is_err());
}

#[test]
fn same_pcm_bytes_with_a_different_loaded_rate_cannot_reuse_verified_quarter_evidence() {
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * 0.5).collect();
    let (mut source, pcm, frames) = fixture(48_000, &times, COUNT as f64 * 0.5);
    let evidence = quarters(&source, &frames);
    source.loaded_sample_rate_hz = 96_000;
    let reinterpreted: Vec<_> = times.iter().map(|time| time * 0.5).collect();
    assert!(
        refine_comparable_attacks(&source, &pcm, &reinterpreted, 0.0, 0.01, Some(&evidence))
            .is_err()
    );
    let unverified =
        refine_comparable_attacks(&source, &pcm, &reinterpreted, 0.0, 0.01, None).unwrap();
    assert_eq!(unverified.status, RefinementStatus::ComparableAttacks);
    assert!(
        unverified
            .proposals
            .iter()
            .all(|proposal| proposal.verification == QuarterNoteVerification::Unverified)
    );
}

#[test]
fn independent_rational_counts_and_explicit_missing_signal_beat_jumps_are_preserved() {
    let times: Vec<_> = (0..COUNT)
        .filter(|&index| index != 20)
        .map(|index| index as f64 * 0.5)
        .collect();
    let (source, pcm, frames) = fixture(48_000, &times, COUNT as f64 * 0.5);
    let mut evidence = quarters(&source, &frames);
    evidence.quarter_count_numerators = (0..COUNT)
        .filter(|&index| index != 20)
        .map(|index| index as i64 + 123)
        .collect();
    evidence.quarter_note_denominator = 2;
    let report =
        refine_comparable_attacks(&source, &pcm, &times, -0.5, 0.01, Some(&evidence)).unwrap();
    assert_eq!(report.proposals[0].quarter_note_denominator, 2);
    assert_eq!(
        report.proposals[0].attack_quarter_counts[19..21],
        [Some(142), Some(144)]
    );
    let summary = assess(&report);
    assert_eq!(summary.status, SummaryStatus::SupportedCandidate);
    assert_eq!(
        summary.hypotheses[0]
            .fit
            .as_ref()
            .unwrap()
            .period_seconds_per_quarter,
        1.0
    );
    assert_eq!(summary.independent_origin_seconds, -0.5);
}
