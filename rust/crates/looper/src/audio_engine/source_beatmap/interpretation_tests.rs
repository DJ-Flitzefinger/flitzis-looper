use super::*;
use crate::audio_engine::source_beatmap::{
    SourceBeatmap, editor,
    interpretation::{self, Interpretation},
};

fn labelled_capture() -> (RawData, Interpretation) {
    let mut raw = capture();
    // Keep extra and outside observations, including the original signed zero.
    raw.beat_frames = vec![-0.0, 10.0, 20.0, 30.0, 1000.0, 44100.0];
    raw.downbeat_indices = vec![0, 3, 5];
    raw.raw_revision = raw.revision().unwrap();
    let intent = Interpretation::parse(
        &json!({"schema_version":1,
        "raw_revision":raw.raw_revision,"anchors":[
            {"raw_index":0,"quarter_bits":format!("{:016x}",(-2.5_f64).to_bits())},
            {"raw_index":3,"quarter_bits":format!("{:016x}",7.25_f64.to_bits())}
        ]})
        .to_string(),
    )
    .unwrap();
    (raw, intent)
}

fn map_value(map: &SourceBeatmap) -> Value {
    serde_json::from_str(map.canonical_record()).unwrap()
}

#[test]
fn v2_preserves_complete_capture_and_three_revisions_without_ordinal_inference() {
    let (raw, intent) = labelled_capture();
    let map = interpretation::build(&raw, &intent, 4096).unwrap();
    let value = map_value(&map);
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["encoding"], "source-beatmap-pwa-v2");
    assert_eq!(value["record"]["raw"].as_object().unwrap().len(), 2);
    assert_eq!(
        value["record"]["raw"]["capture"],
        raw.embedded_value().unwrap()
    );
    let wrapper = value["record"]["raw"]["raw_revision"].as_str().unwrap();
    assert_ne!(wrapper, raw.raw_revision);
    assert_ne!(map.map_revision(), wrapper);
    assert_ne!(map.map_revision(), raw.raw_revision);
    let accepted = &value["record"]["accepted"];
    assert_eq!(accepted["anchors"].as_array().unwrap().len(), 2);
    assert_eq!(accepted["anchors"][1]["raw_beat_index"], 3);
    assert_eq!(
        accepted["anchors"][1]["beat_bits"],
        format!("{:016x}", 7.25_f64.to_bits())
    );
    assert_eq!(
        accepted["anchors"][0]["source_seconds_bits"],
        "8000000000000000"
    );
    assert_eq!(accepted["anchors"][0]["review"], "estimated");
    assert_eq!(accepted["intervals"][0]["coverage"], "uncertain");
    assert_eq!(accepted["intervals"][0]["meter"]["status"], "unknown");
    assert!(accepted["intervals"][0]["meter"]["downbeat_label"].is_null());
    assert_eq!(
        SourceBeatmap::parse(map.canonical_record())
            .unwrap()
            .canonical_record(),
        map.canonical_record()
    );
}

#[test]
fn explicit_review_refuses_invalid_labels_fields_revision_and_numeric_geometry() {
    let (raw, intent) = labelled_capture();
    let base = serde_json::to_value(intent).unwrap();
    for mutation in [
        "one",
        "duplicate_index",
        "reverse_index",
        "reverse_quarter",
        "nan",
        "uppercase",
        "outside",
        "missing",
        "extra",
        "wrong_revision",
        "too_close",
    ] {
        let mut value = base.clone();
        match mutation {
            "one" => {
                value["anchors"].as_array_mut().unwrap().pop();
            }
            "duplicate_index" => value["anchors"][1]["raw_index"] = json!(0),
            "reverse_index" => {
                value["anchors"][0]["raw_index"] = json!(3);
                value["anchors"][1]["raw_index"] = json!(0);
            }
            "reverse_quarter" => {
                value["anchors"][1]["quarter_bits"] = value["anchors"][0]["quarter_bits"].clone()
            }
            "nan" => value["anchors"][1]["quarter_bits"] = json!("7ff8000000000000"),
            "uppercase" => value["anchors"][1]["quarter_bits"] = json!("401D000000000000"),
            "outside" => value["anchors"][1]["raw_index"] = json!(5),
            "missing" => {
                value["anchors"][1]
                    .as_object_mut()
                    .unwrap()
                    .remove("quarter_bits");
            }
            "extra" => value["anchors"][1]["source_seconds"] = json!(0.2),
            "wrong_revision" => value["raw_revision"] = json!("a".repeat(64)),
            "too_close" => {
                value["anchors"][0]["quarter_bits"] = json!("3ff0000000000000");
                value["anchors"][1]["quarter_bits"] = json!("3ff0000000000001");
            }
            _ => unreachable!(),
        }
        assert!(
            Interpretation::parse(&value.to_string())
                .and_then(|intent| intent.validate_for(&raw))
                .is_err(),
            "{mutation}"
        );
    }
    assert!(Interpretation::parse(&" ".repeat(8193)).is_err());
    let mut many = base;
    many["anchors"] = json!(
        (0..65)
            .map(|i| json!({"raw_index":i,"quarter_bits":format!("{:016x}",(i as f64).to_bits())}))
            .collect::<Vec<_>>()
    );
    assert!(Interpretation::parse(&many.to_string()).is_err());
}

#[test]
fn v2_complete_capture_tamper_and_envelope_substitution_are_refused_even_when_resealed() {
    let (raw, intent) = labelled_capture();
    let map = interpretation::build(&raw, &intent, 0).unwrap();
    let base = map_value(&map);
    for change in [
        "bits",
        "downbeat",
        "missing",
        "extra",
        "raw_array",
        "version",
    ] {
        let mut value = base.clone();
        match change {
            "bits" => {
                value["record"]["raw"]["capture"]["data"]["beat_frames"][1] =
                    json!("4024000000000001")
            }
            "downbeat" => {
                value["record"]["raw"]["capture"]["data"]["downbeat_indices"][1] = json!(2)
            }
            "missing" => {
                value["record"]["raw"]["capture"]["data"]
                    .as_object_mut()
                    .unwrap()
                    .remove("mono_rule");
            }
            "extra" => value["record"]["raw"]["capture"]["data"]["verified"] = json!(true),
            "raw_array" => value["record"]["raw"]["beat_seconds_bits"] = json!([]),
            "version" => {
                value["schema_version"] = json!(1);
                value["encoding"] = json!("source-beatmap-pwa-v1");
            }
            _ => unreachable!(),
        }
        assert!(editor::seal(value).is_err(), "{change}");
    }
    let mut stale_map = base.clone();
    stale_map["record"]["accepted"]["anchors"][1]["beat_bits"] =
        json!(format!("{:016x}", 8.0_f64.to_bits()));
    assert!(SourceBeatmap::parse(&stale_map.to_string()).is_err());
    let mut stale_wrapper = base;
    stale_wrapper["record"]["raw"]["raw_revision"] = json!("a".repeat(64));
    assert!(SourceBeatmap::parse(&stale_wrapper.to_string()).is_err());
}

#[test]
fn v2_editor_and_undo_keep_actual_version_and_full_immutable_capture() {
    let (raw, intent) = labelled_capture();
    let map = interpretation::build(&raw, &intent, 0).unwrap();
    let before = map_value(&map);
    let edited = map.edited(&json!({"schema_version":1,"operation":"translate","sample_offset":1,"sample_rate_hz":48000}).to_string()).unwrap();
    let edited_value = map_value(&edited);
    assert_eq!(edited_value["schema_version"], 2);
    assert_eq!(edited_value["record"]["raw"], before["record"]["raw"]);
    let undone = edited
        .edited(r#"{"schema_version":1,"operation":"undo"}"#)
        .unwrap();
    let undone_value = map_value(&undone);
    assert_eq!(undone_value["record"]["raw"], before["record"]["raw"]);
    assert_eq!(
        undone_value["record"]["accepted"]["anchors"],
        before["record"]["accepted"]["anchors"]
    );
    assert_eq!(
        SourceBeatmap::parse(undone.canonical_record())
            .unwrap()
            .canonical_record(),
        undone.canonical_record()
    );
}

#[test]
fn interpretation_peak_charges_retained_source_and_checked_trees_before_growth() {
    let (raw, _) = labelled_capture();
    let encoded = raw.encoded_len().unwrap();
    assert!(interpretation::budget(encoded, 4096, 2).unwrap() > encoded * 32);
    assert!(interpretation::budget(encoded, 512 * 1024 * 1024, 2).is_err());
    assert!(interpretation::budget(usize::MAX, 0, 2).is_err());
    assert!(interpretation::budget(encoded, usize::MAX, 2).is_err());
    assert!(interpretation::budget(encoded, 0, 65).is_err());
}

#[test]
fn v2_signed_virtual_translation_preserves_geometry_but_grants_no_physical_indices_or_loop() {
    use pyo3::prelude::*;
    use pyo3::types::PyDict;
    let (raw, intent) = labelled_capture();
    let map = interpretation::build(&raw, &intent, 0).unwrap();
    let before = map_value(&map);
    Python::initialize();
    for (offset, domain) in [(-1_i64, "before_source"), (48001_i64, "after_source")] {
        let shifted = map
            .edited(
                &json!({"schema_version":1,"operation":"translate",
            "sample_offset":offset,"sample_rate_hz":48000})
                .to_string(),
            )
            .unwrap();
        let shifted_value = map_value(&shifted);
        assert_eq!(shifted_value["schema_version"], 2);
        assert_eq!(shifted_value["record"]["raw"], before["record"]["raw"]);
        assert_eq!(
            SourceBeatmap::parse(shifted.canonical_record())
                .unwrap()
                .canonical_record(),
            shifted.canonical_record()
        );
        let first_bits = shifted_value["record"]["accepted"]["anchors"][0]["source_seconds_bits"]
            .as_str()
            .unwrap();
        let first_seconds = f64::from_bits(u64::from_str_radix(first_bits, 16).unwrap());
        Python::attach(|py| {
            let object = Py::new(py, shifted.clone()).unwrap();
            let evaluation = object
                .bind(py)
                .call_method1("evaluate_beat", (-2.5,))
                .unwrap();
            let evaluation = evaluation.cast::<PyDict>().unwrap();
            assert_eq!(
                evaluation
                    .get_item("physical_domain")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                domain
            );
            assert_eq!(
                evaluation
                    .get_item("source_seconds")
                    .unwrap()
                    .unwrap()
                    .extract::<f64>()
                    .unwrap()
                    .to_bits(),
                first_seconds.to_bits()
            );
            let span = object.bind(py).call_method1("editor_span", (0, 1)).unwrap();
            let span = span.cast::<PyDict>().unwrap();
            let first = span.get_item("first_anchor").unwrap().unwrap();
            assert!(
                first
                    .cast::<PyDict>()
                    .unwrap()
                    .get_item("sample_index")
                    .unwrap()
                    .unwrap()
                    .is_none()
            );
            let endpoint = object
                .bind(py)
                .call_method1("source_after_beats", (first_seconds, 9.75, false))
                .unwrap();
            assert_eq!(
                endpoint
                    .cast::<PyDict>()
                    .unwrap()
                    .get_item("coordinate_status")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "outside_physical_source"
            );
            assert!(
                endpoint
                    .cast::<PyDict>()
                    .unwrap()
                    .get_item("source_seconds")
                    .unwrap()
                    .unwrap()
                    .is_none()
            );
        });
        let undone = shifted
            .edited(r#"{"schema_version":1,"operation":"undo"}"#)
            .unwrap();
        let undone_value = map_value(&undone);
        assert_eq!(undone_value["record"]["raw"], before["record"]["raw"]);
        assert_eq!(
            undone_value["record"]["accepted"]["anchors"],
            before["record"]["accepted"]["anchors"]
        );
    }
}
