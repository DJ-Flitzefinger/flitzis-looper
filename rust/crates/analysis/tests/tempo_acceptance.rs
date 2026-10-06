//! Public offline acceptance and control-ownership contracts.
//!
//! PCM, event times and musical counts are independently constructed fixtures.
//! The worker envelope is synthetic: these tests run no model, device or private
//! audio and make no claim about detector accuracy or audible synchronization.

use flitzis_looper_analysis::{
    tempo_acceptance::{
        AcceptedConstantTiming, IndependentTimingOrigin, TempoAcceptanceError,
        TimingAcceptanceDecision, TimingAdoptionError, TimingAdoptionGuard, TimingIntent,
    },
    tempo_evidence::{
        BackendEvidence, BeatThisModelIdentity, BeatThisRawEvidence, BeatThisRequestIdentity,
        BoundTempoEvidence, JobIdentity, MONO_REVISION, PcmBinding, PcmBindingMetadata,
        TimingBound, f32_pcm_sha256,
    },
    tempo_refinement::{IndependentQuarterEvidence, POLICY_VERSION as FEATURE_POLICY},
    tempo_summary::{QuarterNoteHypothesis, QuarterNoteVerification, SummaryStatus},
};

const COUNT: usize = 64;
const SHAPE: [f32; 8] = [0.5, -0.25, 0.0, 0.125, -0.0625, 0.0, 0.03125, -0.015625];

fn fixture(rate: u32, period: f64) -> (Vec<f32>, Vec<f64>, Vec<u64>) {
    let times: Vec<_> = (0..COUNT).map(|index| index as f64 * period).collect();
    let frames: Vec<_> = times
        .iter()
        .map(|time| (time * f64::from(rate)).round() as u64)
        .collect();
    let mut pcm = vec![0.0; (COUNT as f64 * period * f64::from(rate)).ceil() as usize];
    for &frame in &frames {
        pcm[frame as usize..frame as usize + SHAPE.len()].copy_from_slice(&SHAPE);
    }
    (pcm, times, frames)
}

fn metadata(pcm: &[f32], rate: u32) -> PcmBindingMetadata {
    PcmBindingMetadata {
        job: JobIdentity {
            pad_id: 2,
            request_id: 42,
            source_id: "synthetic-source-7".into(),
            source_generation: 7,
        },
        source_sha256: "1".repeat(64),
        source_provenance: "independent fixture digest assertion; associated with generated PCM"
            .into(),
        pcm_sha256: f32_pcm_sha256(pcm),
        sample_rate_hz: rate,
        frame_count: pcm.len() as u64,
        origin_seconds: 0.0,
        mono_revision: MONO_REVISION.into(),
    }
}

fn timing() -> TimingBound {
    TimingBound {
        halfwidth_seconds: 0.001,
        provenance: "declared synthetic event positioning bound; no musical calibration".into(),
    }
}

fn origin() -> IndependentTimingOrigin {
    IndependentTimingOrigin {
        seconds: -0.375,
        provenance: "independently chosen signed fixture grid origin".into(),
    }
}

fn decision() -> TimingAcceptanceDecision {
    TimingAcceptanceDecision {
        policy_version: "explicit-fixture-acceptance-v1".into(),
        provenance: "caller accepts independently constructed musical quarter truth".into(),
    }
}

fn worker_raw(binding: &PcmBinding<'_>, times: &[f64]) -> BeatThisRawEvidence {
    let model = BeatThisModelIdentity {
        sha256: "a".repeat(64),
        frontend_id: "synthetic-worker-envelope-no-inference".into(),
        environment_id: "independent-fixture-v1".into(),
        package_version: "1.1.0".into(),
        checkpoint: "final0".into(),
        postprocessor: "minimal".into(),
        device: "cpu".into(),
        precision: "float32".into(),
    };
    let expected_request = BeatThisRequestIdentity {
        job: binding.metadata().job.clone(),
        pcm_sha256: binding.metadata().pcm_sha256.clone(),
        pcm_path: "synthetic-complete-mono.f32le".into(),
        sample_rate_hz: binding.metadata().sample_rate_hz,
        frame_count: binding.metadata().frame_count,
        origin_seconds: 0.0,
        dtype: "float32-le".into(),
        channels: 1,
        schema_version: 1,
        model,
    };
    BeatThisRawEvidence {
        response_job: expected_request.job.clone(),
        response_model: expected_request.model.clone(),
        response_schema_version: 1,
        expected_request,
        beat_seconds: times.to_vec(),
        // Deliberately independent of beat-index associations.
        downbeat_seconds: vec![0.125, 1.125, 2.125],
        beat_logits: vec![-0.0, f64::from_bits(1), 1.0000000000000002, -f64::MAX],
        downbeat_logits: vec![0.0, -f64::from_bits(1), -2.5, f64::MAX],
    }
}

fn evidence(binding: &PcmBinding<'_>, times: &[f64]) -> BoundTempoEvidence {
    BoundTempoEvidence::from_beat_this(binding, worker_raw(binding, times), timing(), 0.125)
        .unwrap()
}

fn counts() -> Vec<Option<i64>> {
    (0..COUNT).map(|index| Some(index as i64)).collect()
}

fn hypothesis<'a>(counts: &'a [Option<i64>]) -> QuarterNoteHypothesis<'a> {
    QuarterNoteHypothesis {
        id: "independent-fixture-quarters",
        provenance: "explicit generated pulse/quarter construction; not inferred from fit",
        verification: QuarterNoteVerification::Verified,
        quarter_note_denominator: 1,
        quarter_counts: counts,
    }
}

fn accept(evidence: BoundTempoEvidence) -> AcceptedConstantTiming {
    AcceptedConstantTiming::from_raw(evidence, &[hypothesis(&counts())], origin(), decision())
        .unwrap()
}

fn independent_quarters(binding: &PcmBinding<'_>, frames: &[u64]) -> IndependentQuarterEvidence {
    IndependentQuarterEvidence {
        source_sha256: binding.metadata().source_sha256.clone(),
        pcm_sha256: binding.metadata().pcm_sha256.clone(),
        loaded_sample_rate_hz: binding.metadata().sample_rate_hz,
        loaded_frame_count: binding.metadata().frame_count,
        feature_policy_version: FEATURE_POLICY.into(),
        feature_frames: frames.to_vec(),
        quarter_count_numerators: (0..frames.len()).map(|index| index as i64).collect(),
        quarter_note_denominator: 1,
        provenance: "independent PCM pulse construction establishes quarter counts".into(),
    }
}

fn assert_bits(left: &[f64], right: &[f64]) {
    assert_eq!(left.len(), right.len());
    for (left, right) in left.iter().zip(right) {
        assert_eq!(left.to_bits(), right.to_bits());
    }
}

#[test]
fn reconstruction_is_deterministic_and_retains_all_original_evidence() {
    let (pcm, times, _) = fixture(8_000, 0.5);
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let raw = worker_raw(&binding, &times);
    let bound = evidence(&binding, &times);
    let accepted = accept(bound.clone());
    let reconstructed = accept(evidence(&binding, &times));
    // Independently serialized with Python hashlib/struct, including -0.0,
    // subnormal/max logits, all model/job metadata and complete PCM content.
    // Freezes the existing raw-revision bytes across shared hashing factoring.
    assert_eq!(
        bound.source_identity().raw_revision,
        "2c68ad1090804a61ef058184523a747a1bb36cc1a6d9fabd8657fb39bc9c7e8b"
    );
    assert_eq!(accepted.revision(), reconstructed.revision());
    assert_eq!(accepted.revision(), accepted.clone().revision());
    assert!(
        accepted
            .revision()
            .starts_with("accepted-constant-timing-v1:")
    );
    assert_eq!(accepted.period_seconds_per_quarter(), 0.5);
    assert_ne!(accepted.revision(), bound.source_identity().raw_revision);
    assert_eq!(accepted.origin(), &origin());
    assert_eq!(accepted.decision(), &decision());
    assert_eq!(accepted.evidence().binding(), binding.metadata());
    assert_eq!(accepted.evidence().timing_bound(), &timing());
    assert_eq!(accepted.summary().independent_origin_seconds, -0.375);
    assert_eq!(
        accepted
            .evidence()
            .raw_evidence()
            .independent_origin_seconds,
        0.125
    );
    assert!(accepted.refinement().is_none());
    assert!(accepted.independent_quarters().is_none());
    assert_bits(accepted.evidence().beat_seconds(), &times);
    let BackendEvidence::BeatThis(retained) = accepted.evidence().backend() else {
        panic!("original backend evidence was lost");
    };
    assert_eq!(retained.expected_request, raw.expected_request);
    assert_eq!(retained.response_job, raw.response_job);
    assert_eq!(retained.response_model, raw.response_model);
    assert_bits(&retained.beat_seconds, &raw.beat_seconds);
    assert_bits(&retained.downbeat_seconds, &raw.downbeat_seconds);
    assert_bits(&retained.beat_logits, &raw.beat_logits);
    assert_bits(&retained.downbeat_logits, &raw.downbeat_logits);
    // A diagnostic intercept cannot silently become the selected origin.
    let fit = accepted.summary().hypotheses[0].fit.as_ref().unwrap();
    assert_eq!(fit.diagnostic_intercept_seconds, 0.0);
    assert_eq!(accepted.origin().seconds, -0.375);
}

#[test]
fn complete_backend_arrays_and_exact_bits_change_the_accepted_revision() {
    let (pcm, times, _) = fixture(8_000, 0.5);
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let original = worker_raw(&binding, &times);
    let baseline = accept(evidence(&binding, &times));
    for case in 0..7 {
        let mut changed = original.clone();
        match case {
            0 => changed.beat_seconds[0] = -0.0,
            1 => changed.beat_seconds[40] = f64::from_bits(times[40].to_bits() + 1),
            2 => changed.downbeat_seconds[1] += 0.001,
            3 => changed.beat_logits[0] = 0.0,
            4 => changed.downbeat_logits[0] = -0.0,
            5 => {
                changed.expected_request.model.environment_id += "/another-environment";
                changed.response_model = changed.expected_request.model.clone();
            }
            _ => changed.expected_request.pcm_path = "another-synthetic-export.f32le".into(),
        }
        let bound = BoundTempoEvidence::from_beat_this(&binding, changed, timing(), 0.125).unwrap();
        let accepted = accept(bound);
        assert_ne!(baseline.revision(), accepted.revision(), "case {case}");
        assert!(accepted.period_seconds_per_quarter().is_finite());
        assert_eq!(
            accepted.evidence().binding().pcm_sha256,
            binding.metadata().pcm_sha256
        );
    }
}

#[test]
fn same_raw_revision_retains_distinct_half_double_and_rational_count_choices() {
    let (pcm, times, _) = fixture(8_000, 0.5);
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let bound = evidence(&binding, &times);
    let mut revisions = Vec::new();
    for (multiplier, denominator, offset, expected_period) in [
        (1, 1, 0, 0.5),
        (1, 2, 0, 1.0),
        (2, 1, 0, 0.25),
        (2, 2, 0, 0.5),
        (1, 1, 123, 0.5),
    ] {
        let chosen: Vec<_> = (0..COUNT)
            .map(|index| Some(index as i64 * multiplier + offset))
            .collect();
        let mut candidate = hypothesis(&chosen);
        candidate.quarter_note_denominator = denominator;
        let accepted =
            AcceptedConstantTiming::from_raw(bound.clone(), &[candidate], origin(), decision())
                .unwrap();
        assert_eq!(accepted.period_seconds_per_quarter(), expected_period);
        assert_eq!(accepted.summary().hypotheses[0].quarter_counts, chosen);
        assert_eq!(
            accepted.summary().hypotheses[0].quarter_note_denominator,
            denominator
        );
        assert_eq!(
            accepted.evidence().source_identity().raw_revision,
            bound.source_identity().raw_revision
        );
        assert!(!revisions.contains(&accepted.revision().to_owned()));
        revisions.push(accepted.revision().to_owned());
    }
}

#[test]
fn origin_decision_count_and_error_provenance_are_part_of_accepted_identity() {
    let (pcm, times, _) = fixture(8_000, 0.5);
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let bound = evidence(&binding, &times);
    let baseline = accept(bound.clone());
    for case in 0..9 {
        let chosen = counts();
        let mut candidate = hypothesis(&chosen);
        let mut changed_origin = origin();
        let mut changed_decision = decision();
        let mut changed_timing = timing();
        match case {
            0 => candidate.id = "another-assertion-id",
            1 => candidate.provenance = "another independent quarter provenance",
            2 => changed_origin.seconds = -0.25,
            3 => changed_origin.provenance += "/another-origin-choice",
            4 => changed_decision.policy_version += "/v2",
            5 => changed_decision.provenance += "/another-accepting-caller",
            6 => changed_timing.provenance += "/another-declared-error-establishment",
            7 => changed_timing.halfwidth_seconds *= 0.5,
            _ => changed_origin.seconds = f64::from_bits(origin().seconds.to_bits() + 1),
        }
        let changed_bound = BoundTempoEvidence::from_beat_this(
            &binding,
            worker_raw(&binding, &times),
            changed_timing,
            0.125,
        )
        .unwrap();
        assert_eq!(
            changed_bound.source_identity().raw_revision,
            bound.source_identity().raw_revision
        );
        let changed = AcceptedConstantTiming::from_raw(
            changed_bound,
            &[candidate],
            changed_origin,
            changed_decision,
        )
        .unwrap();
        assert_eq!(changed.period_seconds_per_quarter(), 0.5);
        assert_ne!(baseline.revision(), changed.revision(), "case {case}");
    }
    // Even an equivalent second interpretation remains evaluated evidence.
    let primary = counts();
    let doubled: Vec<_> = primary
        .iter()
        .map(|count| count.map(|value| value * 2))
        .collect();
    let mut equivalent = hypothesis(&doubled);
    equivalent.id = "equivalent-rational-evidence";
    equivalent.quarter_note_denominator = 2;
    let extended = AcceptedConstantTiming::from_raw(
        bound,
        &[hypothesis(&primary), equivalent],
        origin(),
        decision(),
    )
    .unwrap();
    assert_eq!(extended.summary().hypotheses.len(), 2);
    assert_ne!(baseline.revision(), extended.revision());
}

#[test]
fn malformed_assertions_and_nonaccepted_candidate_states_are_rejected() {
    let (pcm, times, _) = fixture(8_000, 0.5);
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let bound = evidence(&binding, &times);
    let chosen = counts();
    for case in 0..7 {
        let mut invalid_origin = origin();
        let mut invalid_decision = decision();
        match case {
            0 => invalid_origin.seconds = f64::NAN,
            1 => invalid_origin.seconds = f64::INFINITY,
            2 => invalid_origin.provenance = " ".into(),
            3 => invalid_decision.policy_version.clear(),
            4 => invalid_decision.provenance = "\t".into(),
            5 => invalid_decision.policy_version = "x".repeat(4097),
            _ => invalid_origin.provenance = "x".repeat(4097),
        }
        assert!(
            AcceptedConstantTiming::from_raw(
                bound.clone(),
                &[hypothesis(&chosen)],
                invalid_origin,
                invalid_decision,
            )
            .is_err(),
            "case {case}"
        );
    }
    let mut unverified = hypothesis(&chosen);
    unverified.verification = QuarterNoteVerification::Unverified;
    assert!(matches!(
        AcceptedConstantTiming::from_raw(bound.clone(), &[unverified], origin(), decision()),
        Err(TempoAcceptanceError::CandidateStatus(
            SummaryStatus::Unverified
        ))
    ));
    let doubled: Vec<_> = chosen
        .iter()
        .map(|count| count.map(|value| value * 2))
        .collect();
    let mut double = hypothesis(&doubled);
    double.id = "verified-double";
    assert!(matches!(
        AcceptedConstantTiming::from_raw(
            bound.clone(),
            &[hypothesis(&chosen), double],
            origin(),
            decision()
        ),
        Err(TempoAcceptanceError::CandidateStatus(
            SummaryStatus::Ambiguous
        ))
    ));
    let short = evidence(&binding, &times[..12]);
    assert!(matches!(
        AcceptedConstantTiming::from_raw(short, &[hypothesis(&chosen[..12])], origin(), decision()),
        Err(TempoAcceptanceError::CandidateStatus(
            SummaryStatus::Unsupported
        ))
    ));
}

#[test]
fn fractional_periods_remain_binary64_with_conditional_uncertainty_at_each_loaded_rate() {
    for rate in [44_100, 48_000, 96_000] {
        for bpm in [119.999_f64, 123.45] {
            let truth = 60.0 / bpm;
            let (pcm, times, frames) = fixture(rate, truth);
            let binding = PcmBinding::verify(&pcm, metadata(&pcm, rate)).unwrap();
            let bound = evidence(&binding, &times);
            let raw = accept(bound.clone());
            assert!((raw.period_seconds_per_quarter() - truth).abs() < 1e-12);
            let assertion = independent_quarters(&binding, &frames);
            let refined = AcceptedConstantTiming::from_comparable_attacks(
                bound,
                &binding,
                0.01,
                &assertion,
                origin(),
                decision(),
            )
            .unwrap();
            let fit = refined.summary().hypotheses[0].fit.as_ref().unwrap();
            assert_eq!(
                refined.period_seconds_per_quarter().to_bits(),
                fit.period_seconds_per_quarter.to_bits()
            );
            let accumulated_frames = (refined.period_seconds_per_quarter() - truth).abs()
                * (COUNT - 1) as f64
                * f64::from(rate);
            assert!(
                accumulated_frames < 1.0,
                "{rate}/{bpm}: {accumulated_frames}"
            );
            assert!(fit.period_sensitivity_bound_seconds > 0.0);
            assert_eq!(refined.summary().source.loaded_sample_rate_hz, rate);
            assert_eq!(
                refined.summary().source.loaded_frame_count,
                pcm.len() as u64
            );
            assert_eq!(
                refined.summary().source.timing_error_halfwidth_seconds,
                0.5 / f64::from(rate)
            );
            let projected = 60.0 / (60.0 / refined.period_seconds_per_quarter()) as f32 as f64;
            assert_ne!(
                refined.period_seconds_per_quarter().to_bits(),
                projected.to_bits()
            );
            assert!((60.0 / refined.period_seconds_per_quarter() - bpm.round()).abs() > 0.0005);
        }
    }
}

#[test]
fn comparable_attack_acceptance_keeps_missing_raw_zero_and_independent_count_evidence() {
    let (pcm, times, frames) = fixture(8_000, 0.5);
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let mut detector: Vec<_> = times.iter().skip(1).map(|time| time + 0.002).collect();
    detector.insert(8, 4.25);
    let bound = evidence(&binding, &detector);
    let assertion = independent_quarters(&binding, &frames);
    let accepted = AcceptedConstantTiming::from_comparable_attacks(
        bound.clone(),
        &binding,
        0.01,
        &assertion,
        origin(),
        decision(),
    )
    .unwrap();
    assert_eq!(accepted.period_seconds_per_quarter(), 0.5);
    assert_eq!(accepted.independent_quarters(), Some(&assertion));
    assert_eq!(
        accepted.evidence().source_identity(),
        bound.source_identity()
    );
    assert_bits(accepted.evidence().beat_seconds(), &detector);
    let refinement = accepted.refinement().unwrap();
    assert_eq!(refinement.raw_beat_seconds, detector);
    assert_eq!(refinement.unmatched_attack_indices, [0]);
    assert_eq!(refinement.attacks.len(), COUNT);
    assert_eq!(refinement.raw_associations[8].attack_index, None);
    assert_eq!(accepted.summary().raw_beat_seconds, times);
    assert_eq!(refinement.independent_origin_seconds, origin().seconds);
    assert_eq!(
        accepted
            .evidence()
            .raw_evidence()
            .independent_origin_seconds,
        0.125
    );
    let repeated = AcceptedConstantTiming::from_comparable_attacks(
        bound.clone(),
        &binding,
        0.01,
        &assertion,
        origin(),
        decision(),
    )
    .unwrap();
    assert_eq!(accepted.revision(), repeated.revision());
    let mut changed_assertion = assertion.clone();
    changed_assertion.provenance += "/another-independent-count-assertion";
    let changed = AcceptedConstantTiming::from_comparable_attacks(
        bound.clone(),
        &binding,
        0.01,
        &changed_assertion,
        origin(),
        decision(),
    )
    .unwrap();
    assert_ne!(accepted.revision(), changed.revision());
    let changed_search = AcceptedConstantTiming::from_comparable_attacks(
        bound,
        &binding,
        0.02,
        &assertion,
        origin(),
        decision(),
    )
    .unwrap();
    assert_ne!(accepted.revision(), changed_search.revision());
}

#[test]
fn refinement_rechecks_complete_pcm_binding_features_and_counts() {
    let (pcm, times, frames) = fixture(8_000, 0.5);
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let bound = evidence(&binding, &times);
    let assertion = independent_quarters(&binding, &frames);
    for case in 0..8 {
        let mut invalid = assertion.clone();
        match case {
            0 => invalid.source_sha256 = "2".repeat(64),
            1 => invalid.pcm_sha256 = "2".repeat(64),
            2 => invalid.loaded_sample_rate_hz *= 2,
            3 => invalid.loaded_frame_count -= 1,
            4 => invalid.feature_frames[10] += 1,
            5 => invalid.quarter_count_numerators[10] = 9,
            6 => invalid.feature_policy_version = "forged-feature-support".into(),
            _ => invalid.provenance.clear(),
        }
        assert!(
            AcceptedConstantTiming::from_comparable_attacks(
                bound.clone(),
                &binding,
                0.01,
                &invalid,
                origin(),
                decision(),
            )
            .is_err(),
            "case {case}"
        );
    }
    let mut altered = pcm.clone();
    altered[frames[20] as usize + 1] = -0.24;
    let altered_binding = PcmBinding::verify(&altered, metadata(&altered, 8_000)).unwrap();
    assert!(matches!(
        AcceptedConstantTiming::from_comparable_attacks(
            bound,
            &altered_binding,
            0.01,
            &assertion,
            origin(),
            decision()
        ),
        Err(TempoAcceptanceError::BindingMismatch)
    ));
    let altered_assertion = independent_quarters(&altered_binding, &frames);
    assert!(matches!(
        AcceptedConstantTiming::from_comparable_attacks(
            evidence(&altered_binding, &times),
            &altered_binding,
            0.01,
            &altered_assertion,
            origin(),
            decision()
        ),
        Err(TempoAcceptanceError::UnsupportedRefinement)
    ));
}

#[test]
fn exact_binding_comparison_rejects_every_source_request_and_timebase_change() {
    let (pcm, times, _) = fixture(8_000, 0.5);
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let accepted = accept(evidence(&binding, &times));
    assert!(accepted.check_binding(binding.metadata()).is_ok());
    for case in 0..12 {
        let mut changed = binding.metadata().clone();
        match case {
            0 => changed.job.pad_id += 1,
            1 => changed.job.request_id += 1,
            2 => changed.job.source_generation += 1,
            3 => changed.job.source_id += "/replacement",
            4 => changed.source_sha256 = "2".repeat(64),
            5 => changed.source_provenance += "/different-association",
            6 => changed.pcm_sha256 = "2".repeat(64),
            7 => changed.sample_rate_hz *= 2,
            8 => changed.frame_count += 1,
            9 => changed.origin_seconds = -0.0,
            10 => changed.origin_seconds = 1.0 / 8_000.0,
            _ => changed.mono_revision += "/different-mix",
        }
        assert_eq!(
            accepted.check_binding(&changed),
            Err(TempoAcceptanceError::BindingMismatch),
            "case {case}"
        );
    }
}

#[test]
fn newer_requests_and_successful_adoption_invalidate_old_or_reused_tickets() {
    let (pcm, times, _) = fixture(8_000, 0.5);
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let accepted = accept(evidence(&binding, &times));
    let mut guard = TimingAdoptionGuard::new(&binding, TimingIntent::Automatic).unwrap();
    let old = guard.issue_ticket().unwrap();
    let current = guard.issue_ticket().unwrap();
    assert_eq!(
        guard.adopt(&old, accepted.clone()),
        Err(TimingAdoptionError::StaleTicket)
    );
    assert!(guard.accepted().is_none());
    guard.adopt(&current, accepted.clone()).unwrap();
    assert_eq!(guard.accepted().unwrap().revision(), accepted.revision());
    assert_eq!(
        guard.adopt(&current.clone(), accepted.clone()),
        Err(TimingAdoptionError::StaleTicket)
    );
    assert_eq!(guard.accepted().unwrap().revision(), accepted.revision());
    let pending = guard.issue_ticket().unwrap();
    assert_eq!(guard.accepted().unwrap().revision(), accepted.revision());
    guard.set_intent(TimingIntent::Automatic).unwrap();
    assert_eq!(
        guard.adopt(&pending, accepted),
        Err(TimingAdoptionError::StaleTicket)
    );
}

#[test]
fn manual_tap_and_legacy_intents_reject_and_cannot_revive_automatic_work() {
    let (pcm, times, _) = fixture(8_000, 0.5);
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let accepted = accept(evidence(&binding, &times));
    for intent in [
        TimingIntent::Manual,
        TimingIntent::Tap,
        TimingIntent::Legacy,
    ] {
        let mut initially_explicit = TimingAdoptionGuard::new(&binding, intent).unwrap();
        assert_eq!(initially_explicit.intent(), intent);
        assert!(matches!(
            initially_explicit.issue_ticket(),
            Err(TimingAdoptionError::IntentNotAutomatic)
        ));
        let mut guard = TimingAdoptionGuard::new(&binding, TimingIntent::Automatic).unwrap();
        let pending = guard.issue_ticket().unwrap();
        guard.set_intent(intent).unwrap();
        assert_eq!(guard.intent(), intent);
        assert!(guard.adopt(&pending, accepted.clone()).is_err());
        assert!(guard.accepted().is_none());
        guard.set_intent(TimingIntent::Automatic).unwrap();
        assert_eq!(
            guard.adopt(&pending, accepted.clone()),
            Err(TimingAdoptionError::StaleTicket)
        );
        let fresh = guard.issue_ticket().unwrap();
        guard.adopt(&fresh, accepted.clone()).unwrap();
    }
}

#[test]
fn foreign_tickets_and_binding_failures_preserve_accepted_state_and_current_ticket() {
    let (pcm, times, _) = fixture(8_000, 0.5);
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let accepted = accept(evidence(&binding, &times));
    let mut guard = TimingAdoptionGuard::new(&binding, TimingIntent::Automatic).unwrap();
    let mut other = TimingAdoptionGuard::new(&binding, TimingIntent::Automatic).unwrap();
    let own = guard.issue_ticket().unwrap();
    let foreign = other.issue_ticket().unwrap();
    assert_eq!(
        guard.adopt(&foreign, accepted.clone()),
        Err(TimingAdoptionError::StaleTicket)
    );
    guard.adopt(&own, accepted.clone()).unwrap();
    let pending = guard.issue_ticket().unwrap();
    let mut other_metadata = binding.metadata().clone();
    other_metadata.job.request_id += 1;
    let other_binding = PcmBinding::verify(&pcm, other_metadata).unwrap();
    let mismatched = accept(evidence(&other_binding, &times));
    assert_eq!(
        guard.adopt(&pending, mismatched),
        Err(TimingAdoptionError::BindingMismatch)
    );
    assert_eq!(guard.accepted().unwrap().revision(), accepted.revision());
    assert_eq!(guard.intent(), TimingIntent::Automatic);
    guard.adopt(&pending, accepted.clone()).unwrap();
    assert_eq!(guard.accepted().unwrap().revision(), accepted.revision());
}

#[test]
fn source_replacement_and_unload_reject_retired_content_even_with_identical_shape() {
    let (pcm, times, _) = fixture(8_000, 0.5);
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let accepted = accept(evidence(&binding, &times));
    for case in 0..7 {
        let mut replacement_pcm = pcm.clone();
        let mut replacement_metadata = binding.metadata().clone();
        match case {
            0 => replacement_metadata.job.request_id += 1,
            1 => replacement_metadata.job.source_generation += 1,
            2 => replacement_metadata.job.source_id += "/same-sized-replacement",
            3 => replacement_metadata.source_sha256 = "2".repeat(64),
            4 => replacement_metadata.sample_rate_hz *= 2,
            5 => {
                replacement_pcm[100] = -0.0;
                replacement_metadata.pcm_sha256 = f32_pcm_sha256(&replacement_pcm);
            }
            _ => replacement_metadata.source_provenance += "/another-source-association",
        }
        let replacement = PcmBinding::verify(&replacement_pcm, replacement_metadata).unwrap();
        let mut guard = TimingAdoptionGuard::new(&binding, TimingIntent::Automatic).unwrap();
        let retired = guard.issue_ticket().unwrap();
        guard
            .replace_source(&replacement, TimingIntent::Automatic)
            .unwrap();
        assert_eq!(
            guard.adopt(&retired, accepted.clone()),
            Err(TimingAdoptionError::StaleTicket),
            "case {case}"
        );
        let current = guard.issue_ticket().unwrap();
        assert_eq!(
            guard.adopt(&current, accepted.clone()),
            Err(TimingAdoptionError::BindingMismatch),
            "case {case}"
        );
        assert!(guard.accepted().is_none());
    }
    let mut guard = TimingAdoptionGuard::new(&binding, TimingIntent::Automatic).unwrap();
    let adopted = guard.issue_ticket().unwrap();
    guard.adopt(&adopted, accepted.clone()).unwrap();
    let pending = guard.issue_ticket().unwrap();
    guard.unload().unwrap();
    assert!(guard.accepted().is_none());
    assert_eq!(
        guard.adopt(&pending, accepted),
        Err(TimingAdoptionError::StaleTicket)
    );
    assert!(matches!(
        guard.issue_ticket(),
        Err(TimingAdoptionError::NoSource)
    ));
}
