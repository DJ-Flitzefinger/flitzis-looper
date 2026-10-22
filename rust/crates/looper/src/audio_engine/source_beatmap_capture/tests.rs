//! Hardware-free data contracts; real worker/custody composition is tested separately.

use super::beat_this::{BeatThisSourceInput, expected_logit_count, pinned_model};
use super::*;
use crate::audio_engine::analysis_pcm::MONO_RULE;
use crate::audio_engine::cold_store::descriptor_digest;
use crate::audio_engine::source_beatmap::{
    SourceBeatmap, editor,
    interpretation::{self, Interpretation},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};

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

fn capture() -> RawCapture {
    let (source, request, result) = fixture();
    BeatThisRawData::from_worker(source, &request.to_string(), &result.to_string())
        .unwrap()
        .into()
}

#[test]
fn complete_four_arrays_preserve_bits_independent_downbeats_and_truthful_input() {
    let raw = capture();
    let encoded = raw.encode().unwrap();
    let restored = RawCapture::decode(&encoded).unwrap();
    assert_eq!(restored.encode().unwrap(), encoded);
    assert_eq!(restored.event_count(), 4);
    assert_eq!(restored.downbeat_count(), 2);
    assert_eq!(
        restored.source_seconds(0).unwrap().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert!(restored.odf_frame(0).is_none());
    assert!(!restored.detected_downbeat(1));
    assert!(restored.detected_downbeat(3));
    assert!(
        restored
            .exact_matching_beat_index(restored.downbeat_seconds(0).unwrap())
            .is_none()
    );
    assert_eq!(restored.exact_matching_beat_index(1.5), Some(3));
    let before = raw.embedded_value().unwrap();
    let after = restored.embedded_value().unwrap();
    assert_eq!(before, after);
    let (_, _, original) = fixture();
    assert_eq!(
        after["data"]["result"]["predictions"],
        original["predictions"]
    );
    let info = restored.backend_info();
    assert_eq!(info["backend"], "beat_this");
    assert!(info["analyzer_sha256"].is_null());
    assert!(info["analyzer_sample_rate_hz"].is_null());
    assert_eq!(info["worker_input_sample_rate_hz"], 48000);
    assert_eq!(info["worker_input_sha256"], "d".repeat(64));
    assert_eq!(info["timing_uncertainty"], "unknown");
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
fn zero_request_id_refuses_worker_and_resealed_raw_and_embedded_map() {
    let (source, mut request, mut result) = fixture();
    // Match both identities so equality checks cannot hide the invalid ID.
    request["identity"]["request_id"] = json!(0);
    result["identity"]["request_id"] = json!(0);
    let error = BeatThisRawData::from_worker(source, &request.to_string(), &result.to_string())
        .unwrap_err();
    assert!(error.contains("source, request or pinned model differs"));

    let raw = capture();
    let intent = Interpretation::parse(
        &json!({"schema_version":1,"raw_revision":raw.revision(),"anchors":[
            {"raw_index":0,"quarter_bits":format!("{:016x}",0.0_f64.to_bits())},
            {"raw_index":3,"quarter_bits":format!("{:016x}",3.0_f64.to_bits())}]})
        .to_string(),
    )
    .unwrap();
    let map = interpretation::build(&raw, &intent, 4096).unwrap();
    let mut capture = raw.embedded_value().unwrap();
    capture["data"]["request"]["identity"]["request_id"] = json!(0);
    capture["data"]["result"]["identity"]["request_id"] = json!(0);
    let malformed: BeatThisRawData = serde_json::from_value(capture["data"].clone()).unwrap();
    let revision = codec::revision(beat_this::ENCODING, &malformed).unwrap();
    // Recreate the complete canonical Raw2 bytes with a correct payload hash.
    // Refusal must come from provenance validation, not stale digest/order data.
    let encoded = format!(
        "{{\"schema_version\":2,\"encoding\":\"{}\",\"raw_revision\":\"{}\",\"data\":{}}}",
        beat_this::ENCODING,
        revision,
        serde_json::to_string(&malformed).unwrap()
    );
    let error = RawCapture::decode(encoded.as_bytes()).unwrap_err();
    assert!(error.contains("source, request or pinned model differs"));

    let mut envelope: Value = serde_json::from_str(map.canonical_record()).unwrap();
    envelope["record"]["raw"]["capture"] = serde_json::from_str(&encoded).unwrap();
    // Seal recomputes both the Raw wrapper and Map3 revision; neither matching
    // hashes nor embedding may turn an invalid capture into accepted geometry.
    let error = editor::seal(envelope).unwrap_err();
    assert!(error.contains("source, request or pinned model differs"));
}

#[test]
fn map_v3_explicit_labels_edit_and_undo_retain_capture_without_qualification() {
    let raw = capture();
    let intent = Interpretation::parse(
        &json!({"schema_version":1,"raw_revision":raw.revision(),"anchors":[
        {"raw_index":0,"quarter_bits":format!("{:016x}",(-2.5_f64).to_bits())},
        {"raw_index":3,"quarter_bits":format!("{:016x}",7.25_f64.to_bits())}]})
        .to_string(),
    )
    .unwrap();
    let map = interpretation::build(&raw, &intent, 4096).unwrap();
    let value: Value = serde_json::from_str(map.canonical_record()).unwrap();
    assert_eq!(value["schema_version"], 3);
    assert_eq!(value["encoding"], "source-beatmap-pwa-v3");
    assert_eq!(
        value["record"]["raw"]["capture"],
        raw.embedded_value().unwrap()
    );
    assert_eq!(
        value["record"]["accepted"]["intervals"][0]["coverage"],
        "uncertain"
    );
    assert_eq!(
        value["record"]["accepted"]["intervals"][0]["meter"]["status"],
        "unknown"
    );
    assert_eq!(
        value["record"]["accepted"]["anchors"][0]["source_seconds_bits"],
        "8000000000000000"
    );
    let moved = map.edited(&json!({"schema_version":1,"operation":"translate","sample_offset":1,"sample_rate_hz":48000}).to_string()).unwrap();
    let undone = moved
        .edited(&json!({"schema_version":1,"operation":"undo"}).to_string())
        .unwrap();
    let undone: Value = serde_json::from_str(undone.canonical_record()).unwrap();
    assert_eq!(undone["schema_version"], 3);
    assert_eq!(undone["record"]["raw"], value["record"]["raw"]);
    assert_eq!(
        undone["record"]["accepted"]["anchors"],
        value["record"]["accepted"]["anchors"]
    );
    let mut wrong_version = value.clone();
    wrong_version["schema_version"] = json!(2);
    wrong_version["encoding"] = json!("source-beatmap-pwa-v2");
    assert!(editor::seal(wrong_version).is_err());
    let mut tampered = value;
    tampered["record"]["raw"]["capture"]["data"]["result"]["input"]["mono_sha256"] =
        json!("e".repeat(64));
    assert!(editor::seal(tampered).is_err());
    assert!(SourceBeatmap::parse(map.canonical_record()).is_ok());
}

#[test]
fn bounded_capture_decode_canonical_spelling_and_publication_peak_fail_closed() {
    let raw = capture();
    let encoded = raw.encode().unwrap();
    assert!(raw.publication_peak_bytes(4096, encoded.len()).unwrap() >= 3 * MAX_RAW_BYTES);
    assert!(raw.publication_peak_bytes(512 * 1024 * 1024, 0).is_err());
    assert!(raw.publication_peak_bytes(usize::MAX, 0).is_err());
    assert!(raw.publication_peak_bytes(0, MAX_RAW_BYTES + 1).is_err());
    assert!(expected_logit_count(usize::MAX, 8000).is_err());
    assert!(expected_logit_count(1, 48000).is_err());
    let mut noncanonical = b" ".to_vec();
    noncanonical.extend_from_slice(&encoded);
    assert!(RawCapture::decode(&noncanonical).is_err());
    let mut value: Value = serde_json::from_slice(&encoded).unwrap();
    value["encoding"] = json!(QM_ENCODING);
    value["schema_version"] = json!(1);
    assert!(RawCapture::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    let (source, request, mut result) = fixture();
    result["predictions"]["beat_seconds"] = json!(STANDARD.encode(vec![0_u8; (250_000 + 1) * 8]));
    assert!(
        BeatThisRawData::from_worker(source, &request.to_string(), &result.to_string()).is_err()
    );
}

#[test]
fn map_v3_accepted_raw_windows_keep_all_four_arrays_and_inner_revision_after_restore() {
    use pyo3::prelude::*;
    use pyo3::types::PyDict;
    let raw = capture();
    let intent = Interpretation::parse(
        &json!({"schema_version":1,"raw_revision":raw.revision(),"anchors":[
        {"raw_index":0,"quarter_bits":format!("{:016x}",0.0_f64.to_bits())},
        {"raw_index":2,"quarter_bits":format!("{:016x}",2.0_f64.to_bits())}]})
        .to_string(),
    )
    .unwrap();
    let map = interpretation::build(&raw, &intent, 4096).unwrap();
    let wire = |map: &SourceBeatmap, mode: &str, first: usize, last: usize, rows: Value| {
        json!({
        "schema_version":1,"operation":"review_raw_window","map_revision":map.map_revision(),
        "capture_revision":raw.revision(),"mode":mode,"first_anchor_index":first,"last_anchor_index":last,"rows":rows}).to_string()
    };
    let extended = map
        .edited(&wire(
            &map,
            "extend_end",
            1,
            1,
            json!([
        {"kind":"raw","raw_index":3,"quarter_bits":format!("{:016x}",3.0_f64.to_bits())}]),
        ))
        .unwrap();
    let repaired = extended
        .edited(&wire(
            &extended,
            "replace_interior",
            0,
            2,
            json!([
        {"kind":"raw","raw_index":1,"quarter_bits":format!("{:016x}",0.75_f64.to_bits())},
        {"kind":"existing","anchor_index":1,"quarter_bits":format!("{:016x}",2.25_f64.to_bits())}]),
        ))
        .unwrap();
    let restored = SourceBeatmap::parse(repaired.clone().canonical_record()).unwrap();
    let before: Value = serde_json::from_str(map.canonical_record()).unwrap();
    let after: Value = serde_json::from_str(restored.canonical_record()).unwrap();
    assert_eq!(before["record"]["raw"], after["record"]["raw"]);
    assert_eq!(
        after["record"]["raw"]["capture"]["data"]["result"]["predictions"],
        raw.embedded_value().unwrap()["data"]["result"]["predictions"]
    );
    Python::initialize();
    Python::attach(|py| {
        let object = Py::new(py, restored.clone()).unwrap();
        let page = object
            .bind(py)
            .call_method1("editor_review_page", (0, 3, 0, 128))
            .unwrap();
        let page = page.cast::<PyDict>().unwrap();
        assert_eq!(
            page.get_item("capture_revision")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            raw.revision()
        );
        let events = page.get_item("events").unwrap().unwrap();
        assert_eq!(events.len().unwrap(), 4);
        assert_eq!(
            events
                .get_item(1)
                .unwrap()
                .get_item("source_seconds_bits")
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "3fe0000000000001"
        );
        assert!(
            !events
                .get_item(1)
                .unwrap()
                .get_item("detected_downbeat")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(
            events
                .get_item(3)
                .unwrap()
                .get_item("detected_downbeat")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
    });
    let undone = restored
        .edited(r#"{"schema_version":1,"operation":"undo"}"#)
        .unwrap();
    let undone: Value = serde_json::from_str(undone.canonical_record()).unwrap();
    let extended: Value = serde_json::from_str(extended.canonical_record()).unwrap();
    assert_eq!(
        undone["record"]["accepted"]["anchors"],
        extended["record"]["accepted"]["anchors"]
    );
    assert_eq!(
        undone["record"]["accepted"]["intervals"],
        extended["record"]["accepted"]["intervals"]
    );
}
