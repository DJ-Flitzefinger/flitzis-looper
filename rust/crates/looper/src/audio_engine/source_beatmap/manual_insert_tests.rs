// Captured-map insertion contracts; no detector or musical acceptance claim.

use super::*;
use crate::audio_engine::source_beatmap::{
    SourceBeatmap, budget, editor,
    interpretation::{self, Interpretation},
};
use pyo3::prelude::*;

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn value(map: &SourceBeatmap) -> Value {
    serde_json::from_str(map.canonical_record()).unwrap()
}

fn captured(coverage: &str) -> SourceBeatmap {
    let mut raw = capture();
    // QM stores fractional ODF-hop frames, not analyzer PCM sample indices.
    // Keep the genuine one-second/48-kHz descriptor and its exact source extent.
    raw.beat_frames = [4410.0, 13230.0, 22050.0, 30870.0]
        .map(|analyzer_sample| analyzer_sample / raw.odf_hop_samples as f64)
        .to_vec();
    raw.downbeat_indices = vec![0, 2];
    raw.raw_revision = raw.revision().unwrap();
    let intent = Interpretation::parse(
        &json!({"schema_version":1,"raw_revision":raw.raw_revision,
        "anchors":[{"raw_index":0,"quarter_bits":bits(-2.5)},
            {"raw_index":1,"quarter_bits":bits(1.25)},
            {"raw_index":2,"quarter_bits":bits(3.5)},
            {"raw_index":3,"quarter_bits":bits(8.75)}]})
        .to_string(),
    )
    .unwrap();
    let map = interpretation::build(&raw, &intent, 4096).unwrap();
    let mut data = value(&map);
    for interval in data["record"]["accepted"]["intervals"]
        .as_array_mut()
        .unwrap()
    {
        interval["coverage"] = json!(coverage);
        interval["meter"] = json!({"status":"quarter_note_4_4_verified","downbeat_label":3,
            "evidence":"Explicit test predecessor metadata, not musical acceptance"});
    }
    editor::seal(data).unwrap()
}

fn request(
    map: &SourceBeatmap,
    mode: &str,
    first: usize,
    last: usize,
    sample: u64,
    quarter: f64,
) -> Value {
    let data = value(map);
    json!({"schema_version":1,"operation":"insert_manual_beat","map_revision":map.map_revision(),
        "capture_revision":data["record"]["raw"]["capture"]["raw_revision"],"mode":mode,
        "first_anchor_index":first,"last_anchor_index":last,"sample_index":sample,
        "sample_rate_hz":48000,"quarter_bits":bits(quarter)})
}

fn undo(map: &SourceBeatmap) -> SourceBeatmap {
    map.edited(r#"{"schema_version":1,"operation":"undo"}"#)
        .unwrap()
}

fn geometry_equal(a: &SourceBeatmap, b: &SourceBeatmap) {
    let a = value(a);
    let b = value(b);
    for key in ["anchors", "intervals"] {
        assert_eq!(a["record"]["accepted"][key], b["record"]["accepted"][key]);
    }
    assert_eq!(a["record"]["raw"], b["record"]["raw"]);
}

fn refuses(map: &SourceBeatmap, intent: Value) {
    let before = map.canonical_record().to_owned();
    let result = std::panic::catch_unwind(|| map.edited(&intent.to_string()));
    assert!(result.is_ok(), "must not panic: {intent}");
    assert!(result.unwrap().is_err(), "must refuse: {intent}");
    assert_eq!(map.canonical_record(), before);
}

#[test]
fn manual_insert_missing_sample_resets_only_split_metadata_for_trusted_uncertain_and_gap() {
    for coverage in ["trusted", "uncertain", "gap_unknown_count"] {
        let map = captured(coverage);
        // Preserve an existing correction and all of its historical evidence.
        let positioned = map
            .edited(
                &json!({"schema_version":1,"operation":"move_beat","beat":1.25,
            "sample_index":14401,"sample_rate_hz":48000})
                .to_string(),
            )
            .unwrap();
        let before = value(&positioned);
        // Nested captures follow canonical map object order; the standalone Raw
        // transport decoder deliberately requires its own typed encoding order.
        let mut embedded: Envelope =
            serde_json::from_value(before["record"]["raw"]["capture"].clone()).unwrap();
        let capture = embedded.validate().unwrap();
        assert!(
            capture
                .beat_frames
                .iter()
                .all(|frame| capture.source_seconds(*frame).to_bits()
                    != (19201.0_f64 / 48000.0).to_bits())
        );
        let op = request(&positioned, "interior", 1, 2, 19201, 2.125);
        let changed = positioned.edited(&op.to_string()).unwrap();
        // The same complete request is deterministic, independent of its worker.
        assert_eq!(
            changed.canonical_record(),
            positioned
                .edited(&op.to_string())
                .unwrap()
                .canonical_record()
        );
        let after = value(&changed);
        let old = before["record"]["accepted"]["anchors"].as_array().unwrap();
        let anchors = after["record"]["accepted"]["anchors"].as_array().unwrap();
        assert_eq!(anchors.len(), 5);
        assert_eq!(&anchors[..2], &old[..2]);
        assert_eq!(&anchors[3..], &old[2..]);
        let inserted = &anchors[2];
        assert_eq!(inserted["source_seconds_bits"], bits(19201.0 / 48000.0));
        assert_eq!(inserted["beat_bits"], bits(2.125));
        assert!(inserted["raw_beat_index"].is_null());
        assert_eq!(inserted["review"], "reviewed");
        assert_eq!(inserted["sample_edit"]["sample_index"], 19201);
        assert_eq!(inserted["sample_edit"]["sample_rate_hz"], 48000);
        assert_eq!(inserted["sample_edit"]["author"], "performer");
        assert!(
            !inserted["sample_edit"]["evidence"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        assert_eq!(after["record"]["raw"], before["record"]["raw"]);
        let intervals = after["record"]["accepted"]["intervals"].as_array().unwrap();
        assert_eq!(intervals[0], before["record"]["accepted"]["intervals"][0]);
        assert_eq!(intervals[3], before["record"]["accepted"]["intervals"][2]);
        for interval in &intervals[1..3] {
            assert_eq!(interval["coverage"], "uncertain");
            assert_eq!(interval["meter"]["status"], "unknown");
            assert!(interval["meter"]["downbeat_label"].is_null());
        }
        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            let object = pyo3::Py::new(py, changed.clone()).unwrap();
            let result = object
                .bind(py)
                .call_method1("evaluate_beat", (2.125,))
                .unwrap();
            assert!(result.get_item("local_bpm").unwrap().is_none());
        });
        geometry_equal(
            &undo(&SourceBeatmap::parse(changed.canonical_record()).unwrap()),
            &positioned,
        );
        geometry_equal(&undo(&undo(&changed)), &map);
    }
}

#[test]
fn manual_insert_outer_actual_endpoints_include_source_zero_and_preserve_all_old_intervals() {
    let map = captured("trusted");
    for (mode, index, sample, quarter) in [
        ("extend_start", 0, 0, -3.75),
        ("extend_end", 3, 47999, 10.125),
    ] {
        let changed = map
            .edited(&request(&map, mode, index, index, sample, quarter).to_string())
            .unwrap();
        let before = value(&map);
        let after = value(&changed);
        let anchors = after["record"]["accepted"]["anchors"].as_array().unwrap();
        let intervals = after["record"]["accepted"]["intervals"].as_array().unwrap();
        let (new_index, old_offset, interval_offset) = if mode == "extend_start" {
            (0, 1, 1)
        } else {
            (4, 0, 0)
        };
        assert_eq!(
            anchors[new_index]["source_seconds_bits"],
            bits(sample as f64 / 48000.0)
        );
        assert_eq!(
            &anchors[old_offset..old_offset + 4],
            before["record"]["accepted"]["anchors"].as_array().unwrap()
        );
        assert_eq!(
            &intervals[interval_offset..interval_offset + 3],
            before["record"]["accepted"]["intervals"]
                .as_array()
                .unwrap()
        );
        assert_eq!(after["record"]["raw"], before["record"]["raw"]);
        geometry_equal(&undo(&changed), &map);
    }
}

#[test]
fn manual_insert_strict_wire_source_revisions_rates_endpoints_and_geometry_refuse_atomically() {
    let map = captured("gap_unknown_count");
    let valid = request(&map, "interior", 1, 2, 19200, 2.125);
    for field in [
        "schema_version",
        "map_revision",
        "capture_revision",
        "mode",
        "first_anchor_index",
        "last_anchor_index",
        "quarter_bits",
        "sample_index",
        "sample_rate_hz",
    ] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(field);
        refuses(&map, missing);
        let mut boolean = valid.clone();
        boolean[field] = json!(true);
        refuses(&map, boolean);
    }
    for (field, bad) in [
        ("schema_version", json!(2)),
        ("map_revision", json!("a".repeat(64))),
        (
            "capture_revision",
            json!(value(&map)["record"]["raw"]["raw_revision"]),
        ),
        ("capture_revision", json!("A".repeat(64))),
        ("mode", json!("replace_interior")),
        ("first_anchor_index", json!(0)),
        ("last_anchor_index", json!(3)),
        ("last_anchor_index", json!(250000)),
        ("sample_index", json!(-1)),
        ("sample_index", json!(48000)),
        ("sample_index", json!(u64::MAX)),
        ("sample_index", json!(14400)),
        ("sample_index", json!(24000)),
        ("sample_index", json!(19200.5)),
        ("sample_rate_hz", json!(44100)),
        ("sample_rate_hz", json!(0)),
        ("quarter_bits", json!(2.125)),
        ("quarter_bits", json!("7ff8000000000000")),
        ("quarter_bits", json!("7ff0000000000000")),
        ("quarter_bits", json!("3FF0000000000000")),
        ("quarter_bits", json!(bits(1.25))),
        ("quarter_bits", json!(bits(1.25_f64.next_up()))),
        ("quarter_bits", json!(bits(3.5))),
        ("quarter_bits", json!(bits(3.5_f64.next_down()))),
    ] {
        let mut op = valid.clone();
        op[field] = bad;
        refuses(&map, op);
    }
    let mut extra = valid.clone();
    extra["raw_index"] = json!(2);
    refuses(&map, extra);
    let duplicated = valid.to_string().replacen(
        "\"sample_index\":19200",
        "\"sample_index\":19200,\"sample_index\":19201",
        1,
    );
    assert_ne!(duplicated, valid.to_string());
    assert!(editor::validate_intent(&duplicated).is_err());
    assert!(editor::validate_intent(&" ".repeat(8193)).is_err());
    for (mode, first, last, sample, quarter) in [
        ("extend_start", 1, 1, 0, -5.0),
        ("extend_end", 2, 2, 47999, 12.0),
        ("extend_start", 0, 0, 4800, -5.0),
        ("extend_start", 0, 0, 0, -2.5),
        ("extend_end", 3, 3, 33600, 12.0),
        ("extend_end", 3, 3, 47999, 8.75),
    ] {
        refuses(&map, request(&map, mode, first, last, sample, quarter));
    }
    // The old MoveBeat contract stays closed in unsupported geometry.
    refuses(
        &map,
        json!({"schema_version":1,"operation":"move_beat","beat":2.125,"sample_index":19200,"sample_rate_hz":48000}),
    );
    let legacy = SourceBeatmap::manual(
        value(&map)["record"]["source"]["decoder_descriptor"].clone(),
        0.0,
        0.1,
    )
    .unwrap();
    refuses(&legacy, request(&map, "interior", 1, 2, 19200, 2.125));
}

#[test]
fn manual_insert_sequential_undo_survives_window_translate_count_clone_and_historical_sample_evidence()
 {
    let base = captured("trusted");
    let inserted = base
        .edited(&request(&base, "interior", 1, 2, 19201, 2.125).to_string())
        .unwrap();
    let translated = inserted.edited(&json!({"schema_version":1,"operation":"translate","sample_offset":1,"sample_rate_hz":48000}).to_string()).unwrap();
    let translated_data = value(&translated);
    assert_eq!(
        translated_data["record"]["accepted"]["anchors"][2]["sample_edit"]["sample_index"],
        19201
    );
    assert_ne!(
        translated_data["record"]["accepted"]["anchors"][2]["source_seconds_bits"],
        bits(19201.0 / 48000.0)
    );
    let counted = translated.edited(&json!({"schema_version":1,"operation":"set_count","first_anchor_index":1,"last_anchor_index":3,"quarter_notes":4.0}).to_string()).unwrap();
    let counted_data = value(&counted);
    let windowed = counted.edited(&json!({"schema_version":1,"operation":"review_raw_window","map_revision":counted.map_revision(),
        "capture_revision":counted_data["record"]["raw"]["capture"]["raw_revision"],"mode":"replace_interior",
        "first_anchor_index":1,"last_anchor_index":3,"rows":[]}).to_string()).unwrap();
    let mut current = SourceBeatmap::parse(windowed.clone().canonical_record()).unwrap();
    for predecessor in [&counted, &translated, &inserted, &base] {
        current = undo(&current);
        geometry_equal(&current, predecessor);
    }
    // A deliberately relabelled endpoint is repaired through the existing count
    // edit first; insertion itself never shifts the suffix or ordinal-labels it.
    let repaired = base.edited(&json!({"schema_version":1,"operation":"set_count","first_anchor_index":1,"last_anchor_index":2,"quarter_notes":0.125}).to_string()).unwrap();
    let changed = repaired
        .edited(&request(&repaired, "interior", 1, 2, 19201, 1.3125).to_string())
        .unwrap();
    geometry_equal(&undo(&changed), &repaired);
    geometry_equal(&undo(&undo(&changed)), &base);
}

#[test]
fn manual_insert_requires_separate_count_repair_when_existing_quarter_spacing_has_no_room() {
    let base = captured("gap_unknown_count");
    let mut data = value(&base);
    let left = 1.25_f64;
    let ulp = left.next_up() - left;
    data["record"]["accepted"]["anchors"][2]["beat_bits"] = json!(bits(left + 16.0 * ulp));
    let narrow = editor::seal(data).unwrap();
    refuses(
        &narrow,
        request(&narrow, "interior", 1, 2, 19201, left + 8.0 * ulp),
    );
    let repaired = narrow
        .edited(
            &json!({"schema_version":1,"operation":"set_count",
        "first_anchor_index":1,"last_anchor_index":2,"quarter_notes":1.0})
            .to_string(),
        )
        .unwrap();
    let inserted = repaired
        .edited(&request(&repaired, "interior", 1, 2, 19201, 1.75).to_string())
        .unwrap();
    geometry_equal(&undo(&inserted), &repaired);
    geometry_equal(&undo(&undo(&inserted)), &narrow);
}

#[test]
fn manual_insert_closed_replay_rejects_tampered_intent_inverse_parent_and_impossible_predecessor() {
    let base = captured("trusted");
    let changed = base
        .edited(&request(&base, "interior", 1, 2, 19201, 2.125).to_string())
        .unwrap();
    for mutation in [
        "sample", "quarter", "capture", "mode", "inverse", "parent", "kind", "extra",
    ] {
        let mut data = value(&changed);
        let last = data["record"]["accepted"]["lineage"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap();
        match mutation {
            "sample" => last["payload"]["intent"]["sample_index"] = json!(19202),
            "quarter" => last["payload"]["intent"]["quarter_bits"] = json!(bits(2.25)),
            "capture" => last["payload"]["intent"]["capture_revision"] = json!("e".repeat(64)),
            "mode" => last["payload"]["intent"]["mode"] = json!("extend_end"),
            "inverse" => {
                last["payload"]["inverse"]["intervals"][0]["meter"]["downbeat_label"] = json!(2)
            }
            "parent" => last["payload"]["predecessor_parent_revision"] = json!("e".repeat(64)),
            "kind" => last["kind"] = json!("source_position"),
            "extra" => last["payload"]["trusted"] = json!(true),
            _ => unreachable!(),
        }
        let forged = editor::seal(data).unwrap();
        assert!(
            forged
                .edited(r#"{"schema_version":1,"operation":"undo"}"#)
                .is_err(),
            "{mutation}"
        );
    }
    // An outer map can be a valid record while its claimed executable parent
    // metadata is impossible, even if the claimed digest is internally rebuilt.
    let mut data = value(&changed);
    let mut predecessor = value(&base)["record"].clone();
    predecessor["accepted"]["parent_revision"] = json!("invalid-parent");
    predecessor["accepted"]
        .as_object_mut()
        .unwrap()
        .remove("map_revision");
    let fake = descriptor_digest(&predecessor).unwrap();
    let last_index = data["record"]["accepted"]["lineage"]
        .as_array()
        .unwrap()
        .len()
        - 1;
    let last = data["record"]["accepted"]["lineage"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    last["id"] = json!(format!("editor-{}-{last_index}", &fake[..16]));
    last["parent_revision"] = json!(fake);
    last["payload"]["intent"]["map_revision"] = json!(fake);
    last["payload"]["predecessor_parent_revision"] = json!("invalid-parent");
    data["record"]["accepted"]["parent_revision"] = json!(fake);
    let inner = data["record"]["raw"]["capture"]["raw_revision"]
        .as_str()
        .unwrap();
    data["record"]["accepted"]["anchors"][2]["id"] =
        json!(format!("manual-{fake}-{inner}-19201-{}", bits(2.125)));
    let forged = editor::seal(data).unwrap();
    assert!(
        forged
            .edited(r#"{"schema_version":1,"operation":"undo"}"#)
            .is_err()
    );
}

#[test]
fn manual_insert_admission_counts_large_old_provenance_and_rejects_held_source_before_growth() {
    let map = captured("trusted");
    let mut data = value(&map);
    data["record"]["accepted"]["anchors"][1]["id"] = json!("i".repeat(4096));
    data["record"]["accepted"]["anchors"][1]["sample_edit"] = json!({"sample_index":14400,"sample_rate_hz":48000,
        "evidence":"e".repeat(4096),"author":"a".repeat(4096)});
    let map = editor::seal(data).unwrap();
    let op = request(&map, "interior", 1, 2, 19201, 2.125).to_string();
    assert!(op.len() < 8192);
    let changed = map.edited(&op).unwrap();
    let data = value(&changed);
    let last = data["record"]["accepted"]["lineage"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert!(budget::serialized_len(&last["payload"]["inverse"]).unwrap() > 8192);
    geometry_equal(&undo(&changed), &map);
    let before = map.canonical_record().to_owned();
    for held in [budget::MAX_BUILD_BYTES, usize::MAX] {
        assert!(map.edited_with_held(&op, held).is_err());
        assert_eq!(map.canonical_record(), before);
    }
}
