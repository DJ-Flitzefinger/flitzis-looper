//! Hardware-free strict neural response/canonical-byte regressions.
use super::raw as beat_this;
use super::raw::{BeatThisRawData, BeatThisSourceInput, expected_logit_count, pinned_model};
use crate::audio_engine::analysis_pcm::MONO_RULE;
use crate::audio_engine::cold_store::descriptor_digest;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

fn pack(values: &[f64]) -> Value {
    let bytes: Vec<u8> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    Value::String(STANDARD.encode(bytes))
}

fn fixture() -> (BeatThisSourceInput, Value, Value) {
    let frames = 96_037;
    let descriptor = json!({"schema_version":1,"original":{"sha256":"a".repeat(64),"bytes":128},
        "pcm":{"format":"f32-le-interleaved-v1","channel_layout":"interleaved-channel-index-order-v1",
            "rate_hz":48000,"channels":2,"full_frames":frames,"full_bytes":frames*8,
            "interleaved_sha256":"b".repeat(64),"mono_sha256":"c".repeat(64),
            "mono_revision":MONO_RULE,"source_zero_bits":"0000000000000000","provenance":{"decoder":"fixture"}}});
    let source = BeatThisSourceInput {
        decoder_identity: descriptor_digest(&descriptor).unwrap(),
        source_descriptor: descriptor,
        source_sample_rate_hz: 48_000,
        source_frame_count: frames,
        source_zero_frame: 0,
        mono_sha256: "d".repeat(64),
        mono_rule: MONO_RULE.into(),
    };
    let identity = json!({"pad_id":0,"request_id":3,"source_id":"actual-original-fixture","source_generation":7});
    let request = json!({"schema_version":1,"identity":identity,"model":pinned_model(),
        "pcm":{"path":std::env::temp_dir().join("capture-original-mono.f32"),"sample_rate_hz":48000,
            "frame_count":frames,"origin_seconds":0.0,"dtype":"float32-le","channels":1}});
    let logits: Vec<f64> = (0..expected_logit_count(frames, 48000).unwrap())
        .map(|index| match index % 3 {
            0 => -0.0,
            1 => f64::from_bits(0x3ff0000000000001),
            _ => -2.25,
        })
        .collect();
    let result = json!({"schema_version":1,"encoding":beat_this::WORKER_ENCODING,"identity":identity,
        "input":{"sample_rate_hz":48000,"frame_count":frames,"source_zero_frame":0,"mono_sha256":"d".repeat(64),"dtype":"float32-le"},
        "model":pinned_model(),"status":"ready","reason":"ready","resources_released":true,
        "predictions":{"encoding":"float64-le/base64","beat_seconds":pack(&[-0.0,f64::from_bits(0x3fe0000000000001),1.0,1.5]),
            "downbeat_seconds":pack(&[f64::from_bits(0x3fe0000000000002),1.5]),"beat_logits":pack(&logits),"downbeat_logits":pack(&logits)}});
    (source, request, result)
}

fn capture() -> BeatThisRawData {
    let (source, request, result) = fixture();
    BeatThisRawData::from_worker(source, &request.to_string(), &result.to_string())
        .unwrap()
        .into()
}

fn cuda_fixture() -> (BeatThisSourceInput, Value, Value) {
    let (source, mut request, mut result) = fixture();
    request["schema_version"] = json!(2);
    request["model"] = beat_this::pinned_cuda_model();
    request["execution_policy"] = json!("auto");
    result["schema_version"] = json!(2);
    result["encoding"] = json!("source-map-beat-this-worker-v2");
    result["model"] = request["model"].clone();
    result["request_json"] = json!(request.to_string());
    result["execution"] = json!({"schema_version":1,"requested_policy":"auto","device":"cuda:0",
        "device_name":"Fixture GPU","precision":"float32","torch_version":"2.8.0+cu128",
        "cuda_build_version":"12.8","fallback_reason":null});
    (source, request, result)
}

#[test]
fn cuda_capture_requires_native_expected_request_and_actual_receipt_without_relabeling_cpu() {
    let (source, request, result) = cuda_fixture();
    let raw =
        BeatThisRawData::from_worker(source, &request.to_string(), &result.to_string()).unwrap();
    assert_eq!(raw.execution().unwrap()["device"], "cuda:0");
    let encoded = raw.encode().unwrap();
    assert_eq!(
        BeatThisRawData::decode(&encoded).unwrap().encode().unwrap(),
        encoded
    );
    let old = capture().encode().unwrap();
    let (source, request, result) = fixture();
    let legacy =
        BeatThisRawData::from_worker(source, &request.to_string(), &result.to_string()).unwrap();
    assert_eq!(legacy.execution().unwrap()["torch_version"], "2.8.0+cpu");
    assert_eq!(
        legacy.execution().unwrap()["cuda_build_version"],
        Value::Null
    );
    let old_json: Value = serde_json::from_slice(&old).unwrap();
    assert!(
        old_json["data"]["request"]
            .get("execution_policy")
            .is_none()
    );
    assert!(old_json["data"]["result"].get("execution").is_none());
    assert!(old_json["data"]["result"].get("request_json").is_none());
    assert_eq!(
        BeatThisRawData::decode(&old).unwrap().encode().unwrap(),
        old
    );
}

#[test]
fn cuda_capture_rejects_cross_profile_unconfirmed_foreign_and_diagnostic_policy_receipts() {
    for mutation in [
        "missing",
        "foreign_request",
        "cpu_model",
        "diagnostic_policy",
        "impossible_gpu",
        "cpu_without_fallback",
        "wrong_build",
        "legacy_extra",
    ] {
        let (source, mut request, mut result) = cuda_fixture();
        match mutation {
            "missing" => {
                result.as_object_mut().unwrap().remove("execution");
            }
            "foreign_request" => {
                let mut foreign = request.clone();
                foreign["identity"]["source_generation"] = json!(99);
                result["request_json"] = json!(foreign.to_string());
            }
            "cpu_model" => {
                result["model"] = pinned_model();
            }
            "diagnostic_policy" => {
                request["execution_policy"] = json!("cuda");
                result["request_json"] = json!(request.to_string());
            }
            "impossible_gpu" => result["execution"]["device"] = json!("cuda:999"),
            "cpu_without_fallback" => {
                result["execution"]["device"] = json!("cpu");
                result["execution"]["device_name"] = Value::Null;
            }
            "wrong_build" => result["execution"]["torch_version"] = json!("2.8.0+cpu"),
            "legacy_extra" => {
                let (_, old_request, old_result) = fixture();
                request = old_request;
                let receipt = result["execution"].clone();
                result = old_result;
                result["execution"] = receipt;
            }
            _ => unreachable!(),
        }
        assert!(
            BeatThisRawData::from_worker(source, &request.to_string(), &result.to_string())
                .is_err(),
            "{mutation}"
        );
    }
    let (source, request, mut result) = cuda_fixture();
    result["execution"]["device"] = json!("cpu");
    result["execution"]["device_name"] = Value::Null;
    result["execution"]["fallback_reason"] = json!("cuda_inference_error");
    let raw =
        BeatThisRawData::from_worker(source, &request.to_string(), &result.to_string()).unwrap();
    assert_eq!(
        raw.execution().unwrap()["fallback_reason"],
        "cuda_inference_error"
    );
}

#[test]
fn mismatched_actual_input_model_identity_partial_arrays_and_retiring_results_refuse() {
    for mutation in [
        "hash",
        "identity",
        "model",
        "zero",
        "extent",
        "logits",
        "nan",
        "out_of_source",
        "unsorted",
        "retiring",
        "unavailable",
        "extra",
    ] {
        let (source, request, mut result) = fixture();
        match mutation {
            "hash" => result["input"]["mono_sha256"] = json!("e".repeat(64)),
            "identity" => result["identity"]["source_generation"] = json!(8),
            "model" => result["model"]["checkpoint"] = json!("small0"),
            "zero" => result["input"]["source_zero_frame"] = json!(1),
            "extent" => result["input"]["frame_count"] = json!(96036),
            "logits" => result["predictions"]["beat_logits"] = pack(&[1.0]),
            "nan" => result["predictions"]["beat_seconds"] = pack(&[f64::NAN]),
            "out_of_source" => result["predictions"]["downbeat_seconds"] = pack(&[3.0]),
            "unsorted" => result["predictions"]["beat_seconds"] = pack(&[1.0, 0.5]),
            "retiring" => result["resources_released"] = json!(false),
            "unavailable" => {
                result["status"] = json!("unavailable");
                result["model"] = Value::Null;
                result["predictions"] = Value::Null;
            }
            "extra" => result["predictions"]["guessed_count"] = json!(4),
            _ => unreachable!(),
        }
        assert!(
            BeatThisRawData::from_worker(source, &request.to_string(), &result.to_string())
                .is_err(),
            "{mutation}"
        );
    }
    let (source, mut request, result) = fixture();
    request["model"]["environment_id"] = json!("uv-lock-sha256:unverified");
    assert!(
        BeatThisRawData::from_worker(source, &request.to_string(), &result.to_string()).is_err()
    );
}

#[test]
fn complete_raw_roundtrip_preserves_all_four_binary64_arrays() {
    let raw = capture();
    let encoded = raw.encode().unwrap();
    let restored = BeatThisRawData::decode(&encoded).unwrap();
    assert_eq!(restored.encode().unwrap(), encoded);
    let actual = restored.predictions();
    let expected = raw.predictions();
    for (a, b) in [
        (&actual.beat_seconds, &expected.beat_seconds),
        (&actual.downbeat_seconds, &expected.downbeat_seconds),
        (&actual.beat_logits, &expected.beat_logits),
        (&actual.downbeat_logits, &expected.downbeat_logits),
    ] {
        assert_eq!(
            a.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            b.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }
    assert_eq!(actual.beat_seconds[0].to_bits(), (-0.0_f64).to_bits());
    assert_ne!(
        actual.beat_seconds[1].to_bits(),
        actual.downbeat_seconds[0].to_bits()
    );
}
