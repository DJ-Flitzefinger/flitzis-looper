//! Test-only declared synthetic BeatThis evidence bound to actual loaded PCM.
//! No analyzer or model inference runs here. Source/request capture, verified
//! complete mono, acceptance, publication and callback acknowledgement are real.

use super::*;
use flitzis_looper_analysis::tempo_evidence::{
    BeatThisModelIdentity, BeatThisRawEvidence, BeatThisRequestIdentity,
};
use serde_json::json;

pub(crate) fn publish_synthetic_loaded_timing(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    id: usize,
    period_seconds: f64,
) -> ConstantTimingTicket {
    let captured = capture_preparation(
        engine,
        id,
        TimingBound {
            halfwidth_seconds: 0.000_001,
            provenance: "declared L3 synthetic timing fixture error; no inference".into(),
        },
        None,
    )
    .unwrap();
    let (complete, _) = complete_timing_sample(
        &crate::audio_engine::native_owned_work::CompleteSampleWork::capture(
            engine,
            id,
            &captured.sample,
        )
        .unwrap(),
        &captured.sample,
        captured.pcm_budget,
    )
    .unwrap();
    let snapshot = LoadedPcmSnapshot::new(
        complete,
        captured.sample_rate_hz,
        PcmIdentity {
            pad_id: id,
            request_id: captured.request_id,
            source_id: format!("loaded-{id}-{}", captured.source_generation),
            source_generation: captured.source_generation,
        },
        captured.pcm_budget.limit_bytes(),
    )
    .unwrap();
    let mono = snapshot
        .prepare_complete_mono(captured.pcm_budget.limit_bytes(), &|| false)
        .unwrap();
    let binding = PcmBinding::verify(
        &mono,
        PcmBindingMetadata {
            job: JobIdentity {
                pad_id: id as u64,
                request_id: captured.request_id,
                source_id: format!("loaded-{id}-{}", captured.source_generation),
                source_generation: captured.source_generation,
            },
            source_sha256: captured.source_digest.clone(),
            source_provenance: SOURCE_PROVENANCE.into(),
            pcm_sha256: f32_pcm_sha256(&mono),
            sample_rate_hz: captured.sample_rate_hz,
            frame_count: mono.len() as u64,
            origin_seconds: 0.0,
            mono_revision: MONO_REVISION.into(),
        },
    )
    .unwrap();
    let duration = mono.len() as f64 / f64::from(captured.sample_rate_hz);
    // Explicit synthetic 1/64-quarter labels provide enough independently
    // declared positions in this short real-PCM fixture. They are not acoustic
    // or general musical labels inferred from its sine-wave components.
    let beat_seconds: Vec<_> = (0..)
        .map(|index| index as f64 * period_seconds / 64.0)
        .take_while(|seconds| *seconds < duration)
        .collect();
    assert!(beat_seconds.len() >= 24);
    let expected = BeatThisRequestIdentity {
        job: binding.metadata().job.clone(),
        pcm_sha256: binding.metadata().pcm_sha256.clone(),
        pcm_path: "synthetic-l3-loaded-pcm.f32le".into(),
        sample_rate_hz: captured.sample_rate_hz,
        frame_count: mono.len() as u64,
        origin_seconds: 0.0,
        dtype: "float32-le".into(),
        channels: 1,
        schema_version: 1,
        model: BeatThisModelIdentity {
            sha256: "b".repeat(64),
            frontend_id: "synthetic-native-fixture-no-inference".into(),
            environment_id: "independently-declared-l3-fixture".into(),
            package_version: "1.1.0".into(),
            checkpoint: "final0".into(),
            postprocessor: "minimal".into(),
            device: "cpu".into(),
            precision: "float32".into(),
        },
    };
    let count = beat_seconds.len();
    let raw = BeatThisRawEvidence {
        response_job: expected.job.clone(),
        response_model: expected.model.clone(),
        response_schema_version: 1,
        expected_request: expected,
        beat_seconds,
        downbeat_seconds: vec![0.0],
        beat_logits: vec![0.0; count],
        downbeat_logits: vec![-0.0; count],
    };
    let evidence =
        BoundTempoEvidence::from_beat_this(&binding, raw, captured.timing_bound.clone(), 0.0)
            .unwrap();
    let mut guard = TimingAdoptionGuard::new(&binding, TimingIntent::Automatic).unwrap();
    let adoption_ticket = guard.issue_ticket().unwrap();
    let ticket = ConstantTimingTicket {
        placement: captured.placement,
        ownership: captured.ownership,
        id,
        request_id: captured.request_id,
        request_incarnation: captured.request_incarnation,
        source_generation: captured.source_generation,
        sample_rate_hz: captured.sample_rate_hz,
        source_digest: captured.source_digest,
        sample: captured.sample,
        epoch: captured.epoch,
        captured_epoch: captured.captured_epoch,
        binding: binding.metadata().clone(),
        evidence,
        guard: Mutex::new(guard),
        adoption_ticket,
        publication: Mutex::new(None),
        pcm_budget: captured.pcm_budget,
    };
    let hypotheses = json!([{
        "id": "declared-synthetic-l3-sixty-fourths",
        "provenance": "explicit synthetic 1/64-quarter labels; no acoustic/inference assertion",
        "verification": "verified",
        "quarter_note_denominator": 64,
        "quarter_counts": (0..count).map(|index| Some(index as i64)).collect::<Vec<_>>(),
    }]);
    publish(
        engine,
        producer,
        &ticket,
        &hypotheses.to_string(),
        IndependentTimingOrigin {
            seconds: -0.217_123_456_789,
            provenance: "independently chosen signed fractional L3 fixture origin".into(),
        },
        TimingAcceptanceDecision {
            policy_version: "explicit-l3-synthetic-fixture-acceptance-v1".into(),
            provenance:
                "explicit test-only synthetic labels; no BeatThis inference or hearing acceptance"
                    .into(),
        },
    )
    .unwrap();
    ticket
}
