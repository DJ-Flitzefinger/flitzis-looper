//! Explicit private fixture gate, using the established native complete-input converter.
//! No audio device, inference, publication or callback is involved.

use std::{fs, path::Path};

use base64::Engine;
use flitzis_looper_analysis::{
    AnalysisConfig, analyze_bpm_raw,
    tempo_evidence::{
        BackendEvidence, BeatThisModelIdentity, BeatThisRawEvidence, BeatThisRequestIdentity,
        BoundTempoEvidence, JobIdentity, MONO_REVISION, PcmBinding, PcmBindingMetadata,
        QmInputDescriptor, QmInputTransform, TimingBound, f64_input_sha256,
    },
    tempo_refinement::{IndependentQuarterEvidence, RefinementStatus, refine_comparable_attacks},
    tempo_summary::{RawTempoEvidence, SummaryStatus, summarize_constant_tempo},
};
use serde_json::{Value, json};

use super::resample_mono_cancellable;

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().expect("declared text field").into()
}

fn number(value: &Value, key: &str) -> u64 {
    value[key].as_u64().expect("declared integer field")
}

fn job(value: &Value) -> JobIdentity {
    JobIdentity {
        pad_id: number(value, "pad_id"),
        request_id: number(value, "request_id"),
        source_id: text(value, "source_id"),
        source_generation: number(value, "source_generation"),
    }
}

fn model(value: &Value) -> BeatThisModelIdentity {
    BeatThisModelIdentity {
        sha256: text(value, "sha256"),
        frontend_id: text(value, "frontend_id"),
        environment_id: text(value, "environment_id"),
        package_version: text(value, "package_version"),
        checkpoint: text(value, "checkpoint"),
        postprocessor: text(value, "postprocessor"),
        device: text(value, "device"),
        precision: text(value, "precision"),
    }
}

fn array(value: &Value, key: &str) -> Vec<f64> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value[key].as_str().expect("complete lossless array"))
        .expect("canonical base64 from the existing publication writer");
    assert_eq!(bytes.len() % 8, 0);
    bytes
        .chunks_exact(8)
        .map(|item| f64::from_le_bytes(item.try_into().unwrap()))
        .collect()
}

fn binding<'a>(pcm: &'a [f32], gate: &Value, identity: JobIdentity) -> PcmBinding<'a> {
    PcmBinding::verify(
        pcm,
        PcmBindingMetadata {
            job: identity,
            source_sha256: text(gate, "source_sha256"),
            source_provenance: text(gate, "source_provenance"),
            pcm_sha256: text(gate, "pcm_sha256"),
            sample_rate_hz: u32::try_from(number(gate, "sample_rate_hz")).unwrap(),
            frame_count: number(gate, "frame_count"),
            origin_seconds: 0.0,
            mono_revision: MONO_REVISION.into(),
        },
    )
    .expect("complete actual PCM content and original provenance")
}

fn assess(bound: &BoundTempoEvidence, pcm: &[f32], quarters: &IndependentQuarterEvidence) -> Value {
    let refined = refine_comparable_attacks(
        bound.source_identity(),
        pcm,
        bound.beat_seconds(),
        0.0,
        0.05,
        Some(quarters),
    )
    .expect("source-matching complete feature evidence");
    assert_eq!(refined.status, RefinementStatus::ComparableAttacks);
    assert_eq!(refined.attacks.len(), 1200);
    let times = refined.attack_seconds();
    let hypotheses: Vec<_> = refined
        .proposals
        .iter()
        .map(|p| p.as_attack_hypothesis())
        .collect();
    let summary = summarize_constant_tempo(
        &RawTempoEvidence {
            source: &refined.refined_source,
            beat_seconds: &times,
            independent_origin_seconds: refined.independent_origin_seconds,
        },
        &hypotheses,
    )
    .expect("complete independent counts");
    assert_eq!(summary.status, SummaryStatus::SupportedCandidate);
    assert_eq!(summary.independent_origin_seconds, 0.0);
    let diagnostic = &summary.hypotheses[summary.supported_hypothesis_index.unwrap()];
    let fit = diagnostic.fit.as_ref().unwrap();
    assert_eq!(diagnostic.inlier_raw_indices.len(), 1200);
    let measured_frames = (fit.period_seconds_per_quarter - 0.5) * 1199.0 * 48_000.0;
    let extrapolated_frames = (fit.period_seconds_per_quarter - 0.5) * 1200.0 * 48_000.0;
    assert!(
        measured_frames.abs() <= 1.0,
        "actual signal slope: {measured_frames} frames"
    );
    let raw_hypotheses: Vec<_> = refined
        .proposals
        .iter()
        .map(|p| p.as_raw_hypothesis())
        .collect();
    let raw_summary = summarize_constant_tempo(&bound.raw_evidence(), &raw_hypotheses).unwrap();
    let unknown = refine_comparable_attacks(
        bound.source_identity(),
        pcm,
        bound.beat_seconds(),
        0.0,
        0.05,
        None,
    )
    .unwrap();
    let unknown_hypotheses: Vec<_> = unknown
        .proposals
        .iter()
        .map(|p| p.as_attack_hypothesis())
        .collect();
    let unknown_summary = summarize_constant_tempo(
        &RawTempoEvidence {
            source: &unknown.refined_source,
            beat_seconds: &times,
            independent_origin_seconds: 0.0,
        },
        &unknown_hypotheses,
    )
    .unwrap();
    assert_eq!(unknown_summary.status, SummaryStatus::Ambiguous);
    let associations: Vec<_> = refined
        .raw_associations
        .iter()
        .map(|a| {
            json!({
                "raw_index": a.raw_index, "original_seconds": a.original_seconds,
                "attack_index": a.attack_index, "refined_seconds": a.refined_seconds,
                "displacement_seconds": a.displacement_seconds,
            })
        })
        .collect();
    json!({
        "raw_revision": bound.source_identity().raw_revision,
        "configuration_revision": bound.source_identity().configuration_revision,
        "backend_revision": bound.source_identity().backend_revision,
        "original_raw_count": bound.beat_seconds().len(),
        "original_raw_seconds": bound.beat_seconds(),
        "original_raw_bound_seconds": bound.timing_bound().halfwidth_seconds,
        "raw_summary_status": format!("{:?}", raw_summary.status),
        "refinement_policy": refined.policy_version,
        "feature_bound_seconds": refined.refined_source.timing_error_halfwidth_seconds,
        "attack_shape_sha256": refined.attack_shape_sha256,
        "feature_frames": refined.attacks.iter().map(|a| a.frame).collect::<Vec<_>>(),
        "raw_associations": associations,
        "unmatched_attack_indices": refined.unmatched_attack_indices,
        "quarter_counts": diagnostic.quarter_counts,
        "quarter_count_provenance": diagnostic.provenance,
        "all1200_inliers": diagnostic.inlier_raw_indices.len(),
        "summary_status": format!("{:?}", summary.status),
        "without_independent_unit_status": format!("{:?}", unknown_summary.status),
        "period_seconds_per_quarter": fit.period_seconds_per_quarter,
        "bpm": 60.0 / fit.period_seconds_per_quarter,
        "measured_span_seconds": 599.5,
        "measured_span_quarter_intervals": 1199,
        "measured_slope_error_loaded_frames": measured_frames,
        "600_second_extrapolation_error_loaded_frames": extrapolated_frames,
        "conditional_period_sensitivity_seconds": fit.period_sensitivity_bound_seconds,
        "conditional_measured_span_sensitivity_frames": fit.period_sensitivity_bound_seconds * 1199.0 * 48000.0,
        "max_abs_feature_residual_seconds": fit.max_abs_inlier_residual_seconds,
        "all_feature_residuals_seconds": diagnostic.residual_seconds,
        "independent_origin_seconds": summary.independent_origin_seconds,
        "diagnostic_intercept_seconds": fit.diagnostic_intercept_seconds,
        "window_period_spread_seconds": fit.window_period_spread_seconds,
    })
}

#[test]
#[ignore = "explicit private full-source gate; needs G2B2_GATE_INPUT and G2B2_GATE_OUTPUT"]
fn private_exact_wav_source_bound_tempo_gate() {
    let input = std::env::var("G2B2_GATE_INPUT").expect("explicit private gate input");
    let output = std::env::var("G2B2_GATE_OUTPUT").expect("explicit private gate output");
    assert!(fs::metadata(&input).unwrap().len() <= 8 * 1024 * 1024);
    let gate: Value = serde_json::from_slice(&fs::read(input).unwrap()).unwrap();
    let pcm_path = text(&gate, "pcm_path");
    assert!(Path::new(&pcm_path).is_absolute());
    assert_eq!(fs::metadata(&pcm_path).unwrap().len(), 28_800_000 * 4);
    let bytes = fs::read(pcm_path).unwrap();
    let pcm: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    drop(bytes);
    let native_binding = binding(&pcm, &gate, job(&gate["native_job"]));
    let converted =
        resample_mono_cancellable(pcm.clone(), 48_000, 44_100, 512 * 1024 * 1024, &|| false)
            .unwrap();
    let analyzer: Vec<f64> = converted.iter().map(|v| f64::from(*v)).collect();
    drop(converted);
    let raw = analyze_bpm_raw(&analyzer, 44_100, &AnalysisConfig::default()).unwrap();
    let legacy_bpm = raw.legacy_result().0;
    assert_eq!(legacy_bpm, 120.001_29_f32);
    let qm_input = QmInputDescriptor {
        job: native_binding.metadata().job.clone(),
        pcm_sha256: native_binding.metadata().pcm_sha256.clone(),
        input_sha256: f64_input_sha256(&analyzer),
        sample_rate_hz: 44_100, frame_count: analyzer.len() as u64, origin_seconds: 0.0,
        transform: QmInputTransform::Rubato44100 {
            revision: "rubato-fft-1.0-44100-delay-trim-tail-flush-v1".into(),
            provenance: "established native resample_mono_cancellable executed in this gate on the verified loaded mono".into(),
        },
    };
    let qm = BoundTempoEvidence::from_qm(&native_binding, raw, &analyzer, qm_input,
        TimingBound { halfwidth_seconds: 0.05, provenance: "explicit conservative 50ms feature matching engineering bound, not calibrated musical error".into() }, 0.0).unwrap();
    drop(analyzer);
    let quarters = &gate["independent_quarters"];
    let independent = IndependentQuarterEvidence {
        source_sha256: text(&gate, "source_sha256"),
        pcm_sha256: text(&gate, "pcm_sha256"),
        loaded_sample_rate_hz: 48_000,
        loaded_frame_count: 28_800_000,
        feature_policy_version: flitzis_looper_analysis::tempo_refinement::POLICY_VERSION.into(),
        feature_frames: quarters["feature_frames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect(),
        quarter_count_numerators: quarters["quarter_count_numerators"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect(),
        quarter_note_denominator: 1,
        provenance: text(quarters, "provenance"),
    };
    let request = &gate["beat_this_request"];
    let beat_binding = binding(&pcm, &gate, job(&request["identity"]));
    let expected_request = BeatThisRequestIdentity {
        job: job(&request["identity"]),
        pcm_sha256: text(&gate, "pcm_sha256"),
        pcm_path: text(&request["pcm"], "path"),
        sample_rate_hz: 48_000,
        frame_count: 28_800_000,
        origin_seconds: 0.0,
        dtype: "float32-le".into(),
        channels: 1,
        schema_version: 1,
        model: model(&request["model"]),
    };
    let predictions = &gate["beat_this_packed_predictions"];
    let beats = BoundTempoEvidence::from_beat_this(&beat_binding, BeatThisRawEvidence {
        response_job: expected_request.job.clone(), response_model: expected_request.model.clone(),
        response_schema_version: 1, expected_request,
        beat_seconds: array(predictions, "beat_seconds"), downbeat_seconds: array(predictions, "downbeat_seconds"),
        beat_logits: array(predictions, "beat_logits"), downbeat_logits: array(predictions, "downbeat_logits"),
    }, TimingBound { halfwidth_seconds: 0.05, provenance: "explicit conservative 50ms feature matching engineering bound;20ms model lattice is not musical certainty".into() }, 0.0).unwrap();
    let BackendEvidence::BeatThis(retained) = beats.backend() else {
        unreachable!()
    };
    let mut array_digests = serde_json::Map::new();
    for (name, values) in [
        ("beat_seconds", &retained.beat_seconds),
        ("downbeat_seconds", &retained.downbeat_seconds),
        ("beat_logits", &retained.beat_logits),
        ("downbeat_logits", &retained.downbeat_logits),
    ] {
        let digest = f64_input_sha256(values);
        assert_eq!(digest, text(&gate["beat_this_array_sha256"], name));
        array_digests.insert(name.into(), Value::String(digest));
    }
    let qm_report = assess(&qm, &pcm, &independent);
    assert_eq!(qm_report["original_raw_count"], 1199);
    assert_eq!(qm_report["unmatched_attack_indices"], json!([0]));
    let beat_report = assess(&beats, &pcm, &independent);
    assert_eq!(beat_report["original_raw_count"], 1200);
    assert_eq!(beat_report["unmatched_attack_indices"], json!([]));
    let BackendEvidence::Qm { raw, input } = qm.backend() else {
        unreachable!()
    };
    let report = json!({
        "source_sha256": text(&gate, "source_sha256"), "pcm_sha256": text(&gate, "pcm_sha256"),
        "loaded_rate_hz": 48000, "loaded_frame_count": 28800000,
        "legacy_automatic_bpm_unchanged": legacy_bpm,
        "qm_input_rate_hz": input.sample_rate_hz, "qm_input_frame_count": input.frame_count,
        "qm_input_sha256": input.input_sha256, "qm_actual_hop": raw.odf_hop_samples(),
        "qm_original_frames": raw.beat_frames(), "qm_original_downbeat_indices": raw.downbeat_raw_indices(),
        "qm": qm_report, "beat_this": beat_report,
        "retained_beat_this_response_sha256": text(&gate, "retained_response_sha256"),
        "lossless_beat_this_array_sha256": array_digests,
        "new_inference": false, "app_or_audio_device_session": false, "runtime_publication": false,
    });
    fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
