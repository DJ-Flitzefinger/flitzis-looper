// Real captured-envelope software contracts; no musical/source-current claim.

use super::*;
use crate::audio_engine::source_beatmap::{
    SourceBeatmap, budget, editor,
    interpretation::{self, Interpretation},
};
use pyo3::prelude::*;
use pyo3::types::PyDict;

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}
fn value(map: &SourceBeatmap) -> Value {
    serde_json::from_str(map.canonical_record()).unwrap()
}

fn captured(indices: &[usize]) -> SourceBeatmap {
    let mut raw = capture();
    raw.beat_frames = (0..240)
        .map(|index| if index == 0 { -0.0 } else { index as f64 * 0.1 })
        .collect();
    raw.downbeat_indices = vec![0, 40, 120, 200];
    raw.raw_revision = raw.revision().unwrap();
    let intent = Interpretation::parse(&json!({"schema_version":1,"raw_revision":raw.raw_revision,
        "anchors":indices.iter().map(|index| json!({"raw_index":index,"quarter_bits":bits(*index as f64)})).collect::<Vec<_>>()}).to_string()).unwrap();
    interpretation::build(&raw, &intent, 4096).unwrap()
}

fn request(map: &SourceBeatmap, mode: &str, first: usize, last: usize, rows: Vec<Value>) -> Value {
    let data = value(map);
    json!({"schema_version":1,"operation":"review_raw_window","map_revision":map.map_revision(),
        "capture_revision":data["record"]["raw"]["capture"]["raw_revision"],"mode":mode,
        "first_anchor_index":first,"last_anchor_index":last,"rows":rows})
}
fn raw(index: usize, quarter: f64) -> Value {
    json!({"kind":"raw","raw_index":index,"quarter_bits":bits(quarter)})
}
fn existing(index: usize, quarter: f64) -> Value {
    json!({"kind":"existing","anchor_index":index,"quarter_bits":bits(quarter)})
}
fn undo(map: &SourceBeatmap) -> SourceBeatmap {
    map.edited(r#"{"schema_version":1,"operation":"undo"}"#)
        .unwrap()
}
fn geometry_equal(a: &SourceBeatmap, b: &SourceBeatmap) {
    let a = value(a);
    let b = value(b);
    for field in ["anchors", "intervals"] {
        assert_eq!(
            a["record"]["accepted"][field],
            b["record"]["accepted"][field]
        );
    }
    assert_eq!(a["record"]["raw"], b["record"]["raw"]);
}

#[test]
fn accepted_raw_windows_extend_64_to_127_to_190_keep_raw_and_closed_multiple_undo_after_clone_restore()
 {
    let first = captured(&(0..64).collect::<Vec<_>>());
    let second = first
        .edited(
            &request(
                &first,
                "extend_end",
                63,
                63,
                (64..127).map(|i| raw(i, i as f64)).collect(),
            )
            .to_string(),
        )
        .unwrap();
    let third = second
        .edited(
            &request(
                &second,
                "extend_end",
                126,
                126,
                (127..190).map(|i| raw(i, i as f64)).collect(),
            )
            .to_string(),
        )
        .unwrap();
    for (before, after, count) in [(&first, &second, 127), (&second, &third, 190)] {
        let a = value(before);
        let b = value(after);
        let old = a["record"]["accepted"]["anchors"].as_array().unwrap();
        assert_eq!(
            b["record"]["accepted"]["anchors"].as_array().unwrap().len(),
            count
        );
        assert_eq!(
            &b["record"]["accepted"]["anchors"].as_array().unwrap()[..old.len()],
            old
        );
        assert_eq!(a["record"]["raw"], b["record"]["raw"]);
        assert!(
            b["record"]["accepted"]["intervals"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["coverage"] == "uncertain"
                    && r["meter"]["status"] == "unknown"
                    && r["meter"]["downbeat_label"].is_null())
        );
    }
    let restored = SourceBeatmap::parse(third.clone().canonical_record()).unwrap();
    Python::initialize();
    Python::attach(|py| {
        let object = Py::new(py, restored.clone()).unwrap();
        assert!(
            object
                .bind(py)
                .call_method1("editor_review_page", (0, 64, 0, 128))
                .is_err()
        );
    });
    let undone = undo(&restored);
    geometry_equal(&undone, &second);
    geometry_equal(&undo(&undone), &first);
    let start = captured(&(63..127).collect::<Vec<_>>());
    let extended = start
        .edited(
            &request(
                &start,
                "extend_start",
                0,
                0,
                (0..63).map(|i| raw(i, i as f64)).collect(),
            )
            .to_string(),
        )
        .unwrap();
    assert_eq!(
        value(&extended)["record"]["accepted"]["anchors"]
            .as_array()
            .unwrap()
            .len(),
        127
    );
    geometry_equal(&undo(&extended), &start);
}

#[test]
fn accepted_raw_window_nonordinal_deletion_existing_quarter_repair_preserves_sample_edit_and_exterior_evidence()
 {
    let map = captured(&[0, 10, 20, 30, 40]);
    let positioned = map
        .edited(
            &json!({"schema_version":1,"operation":"move_beat","beat":20.0,
        "sample_index":1116,"sample_rate_hz":48000})
            .to_string(),
        )
        .unwrap();
    let qualified = positioned
        .edited(
            &json!({"schema_version":1,"operation":"set_coverage","first_anchor_index":0,
        "last_anchor_index":4,"coverage":"trusted","reviewed_quarter_notes":40.0})
            .to_string(),
        )
        .unwrap();
    let qualified = qualified
        .edited(
            &json!({"schema_version":1,"operation":"set_meter","first_anchor_index":0,
        "last_anchor_index":4,"meter_status":"quarter_note_4_4_verified"})
            .to_string(),
        )
        .unwrap();
    let qualified = qualified
        .edited(
            &json!({"schema_version":1,"operation":"set_downbeat","first_anchor_index":0,
        "last_anchor_index":4,"downbeat_label":2})
            .to_string(),
        )
        .unwrap();
    assert!(
        qualified
            .edited(
                &request(
                    &qualified,
                    "replace_interior",
                    1,
                    4,
                    vec![existing(2, 20.0), existing(3, 30.0)]
                )
                .to_string()
            )
            .unwrap_err()
            .contains("no_change")
    );
    let edited = qualified
        .edited(
            &request(
                &qualified,
                "replace_interior",
                1,
                4,
                vec![existing(2, 17.25), raw(35, 33.5)],
            )
            .to_string(),
        )
        .unwrap();
    let before = value(&qualified);
    let after = value(&edited);
    assert_eq!(
        after["record"]["accepted"]["anchors"][0],
        before["record"]["accepted"]["anchors"][0]
    );
    assert_eq!(
        after["record"]["accepted"]["intervals"][0],
        before["record"]["accepted"]["intervals"][0]
    );
    assert_eq!(
        after["record"]["accepted"]["anchors"][1],
        before["record"]["accepted"]["anchors"][1]
    );
    assert_eq!(
        after["record"]["accepted"]["anchors"][4],
        before["record"]["accepted"]["anchors"][4]
    );
    let mut expected = before["record"]["accepted"]["anchors"][2].clone();
    expected["beat_bits"] = json!(bits(17.25));
    assert_eq!(after["record"]["accepted"]["anchors"][2], expected);
    assert!(
        after["record"]["accepted"]["intervals"].as_array().unwrap()[1..]
            .iter()
            .all(|r| r["coverage"] == "uncertain"
                && r["meter"]["status"] == "unknown"
                && r["meter"]["downbeat_label"].is_null())
    );
    assert_eq!(after["record"]["raw"], before["record"]["raw"]);
    Python::initialize();
    Python::attach(|py| {
        let object = Py::new(py, edited.clone()).unwrap();
        let result = object
            .bind(py)
            .call_method1("evaluate_source", (0.03,))
            .unwrap();
        assert!(
            result
                .cast::<PyDict>()
                .unwrap()
                .get_item("local_bpm")
                .unwrap()
                .unwrap()
                .is_none()
        );
    });
    let count = 40.0 - 10.0;
    let trusted = edited
        .edited(
            &json!({"schema_version":1,"operation":"set_coverage","first_anchor_index":1,
        "last_anchor_index":4,"coverage":"trusted","reviewed_quarter_notes":count})
            .to_string(),
        )
        .unwrap();
    Python::attach(|py| {
        let object = Py::new(py, trusted).unwrap();
        let result = object
            .bind(py)
            .call_method1("evaluate_source", (0.03,))
            .unwrap();
        assert!(
            result
                .cast::<PyDict>()
                .unwrap()
                .get_item("local_bpm")
                .unwrap()
                .unwrap()
                .extract::<f64>()
                .unwrap()
                > 0.0
        );
    });
    geometry_equal(&undo(&edited), &qualified);
    let deleted = qualified
        .edited(&request(&qualified, "replace_interior", 1, 4, vec![]).to_string())
        .unwrap();
    assert_eq!(
        value(&deleted)["record"]["accepted"]["anchors"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    geometry_equal(&undo(&deleted), &qualified);
}

#[test]
fn accepted_raw_windows_refuse_stale_invalid_duplicates_exterior_associations_and_noop_atomically()
{
    let map = captured(&[0, 10, 20, 30, 40]);
    let original = map.canonical_record().to_owned();
    let base = request(
        &map,
        "replace_interior",
        1,
        4,
        vec![raw(15, 15.0), existing(2, 22.0)],
    );
    for change in [
        "map",
        "capture",
        "outside",
        "duplicate",
        "existing_outside",
        "order",
        "bits",
        "extra",
        "too_many",
        "range",
    ] {
        let mut op = base.clone();
        match change {
            "map" => op["map_revision"] = json!("a".repeat(64)),
            "capture" => op["capture_revision"] = json!("b".repeat(64)),
            "outside" => op["rows"][0]["raw_index"] = json!(999),
            "duplicate" => op["rows"] = json!([raw(15, 15.0), raw(15, 17.0)]),
            "existing_outside" => op["rows"][1]["anchor_index"] = json!(0),
            "order" => op["rows"][1]["quarter_bits"] = json!(bits(12.0)),
            "bits" => op["rows"][0]["quarter_bits"] = json!("7ff8000000000000"),
            "extra" => op["rows"][0]["source_seconds"] = json!(0.01),
            "too_many" => op["rows"] = json!((0..63).map(|i| raw(i, i as f64)).collect::<Vec<_>>()),
            "range" => op["last_anchor_index"] = json!(100),
            _ => unreachable!(),
        }
        assert!(map.edited(&op.to_string()).is_err(), "{change}");
        assert_eq!(map.canonical_record(), original);
    }
    assert!(
        map.edited(&request(&map, "replace_interior", 1, 3, vec![existing(2, 20.0)]).to_string())
            .unwrap_err()
            .contains("no_change")
    );
    // Correct exterior raw25 to sample0, then correct the left endpoint earlier
    // than raw25's original timestamp. Geometry alone would allow duplicate25.
    let custom = captured(&[25, 30, 40, 50]);
    let custom = custom
        .edited(
            &json!({"schema_version":1,"operation":"move_beat","beat":25.0,
        "sample_index":0,"sample_rate_hz":48000})
            .to_string(),
        )
        .unwrap();
    let custom = custom
        .edited(
            &json!({"schema_version":1,"operation":"move_beat","beat":30.0,
        "sample_index":1116,"sample_rate_hz":48000})
            .to_string(),
        )
        .unwrap();
    assert!(
        custom
            .edited(&request(&custom, "replace_interior", 1, 3, vec![raw(25, 35.0)]).to_string())
            .unwrap_err()
            .contains("duplicate_observation")
    );
    let mut duplicated = value(&map);
    duplicated["record"]["accepted"]["anchors"][4]["raw_beat_index"] = json!(0);
    let duplicated = editor::seal(duplicated).unwrap();
    assert!(
        duplicated
            .edited(
                &request(&duplicated, "replace_interior", 1, 3, vec![raw(25, 25.0)]).to_string()
            )
            .unwrap_err()
            .contains("duplicate_observation")
    );
}

#[test]
fn accepted_raw_windows_bounded_readback_uses_inner_revision_after_clone_restore_and_never_emits_provenance_strings()
 {
    let map = captured(&(0..64).collect::<Vec<_>>());
    let map = SourceBeatmap::parse(map.clone().canonical_record()).unwrap();
    let data = value(&map);
    Python::initialize();
    Python::attach(|py| {
        let object = Py::new(py, map.clone()).unwrap();
        let info = object.bind(py).call_method0("editor_review_info").unwrap();
        let info = info.cast::<PyDict>().unwrap();
        assert_eq!(
            info.get_item("capture_revision")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            data["record"]["raw"]["capture"]["raw_revision"]
                .as_str()
                .unwrap()
        );
        assert_ne!(
            info.get_item("capture_revision")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            data["record"]["raw"]["raw_revision"].as_str().unwrap()
        );
        let page = object
            .bind(py)
            .call_method1("editor_review_page", (0, 63, 112, 128))
            .unwrap();
        let page = page.cast::<PyDict>().unwrap();
        assert_eq!(
            page.get_item("returned_count")
                .unwrap()
                .unwrap()
                .extract::<usize>()
                .unwrap(),
            128
        );
        let anchors = page.get_item("anchors").unwrap().unwrap();
        assert_eq!(anchors.len().unwrap(), 64);
        let row = anchors.get_item(0).unwrap();
        assert!(!row.contains("id").unwrap());
        assert!(!row.contains("sample_edit").unwrap());
        assert_eq!(
            row.get_item("source_seconds_bits")
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "8000000000000000"
        );
        assert!(
            object
                .bind(py)
                .call_method1("editor_review_page", (0, 63, 0, 129))
                .is_err()
        );
        assert!(
            object
                .bind(py)
                .call_method1("editor_review_page", (-1, 2, 0, 128))
                .is_err()
        );
    });
}

#[test]
fn accepted_raw_windows_closed_inverse_tamper_refuses_and_shared_peak_boundaries_precede_growth() {
    let map = captured(&[0, 10, 20]);
    let edited = map
        .edited(&request(&map, "extend_end", 2, 2, vec![raw(30, 30.0)]).to_string())
        .unwrap();
    for change in ["inverse", "parent", "kind", "intent", "payload_extra"] {
        let mut data = value(&edited);
        let last = data["record"]["accepted"]["lineage"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap();
        match change {
            "inverse" => last["payload"]["inverse"]["anchors"][0]["review"] = json!("reviewed"),
            "parent" => last["payload"]["predecessor_parent_revision"] = json!("e".repeat(64)),
            "kind" => last["kind"] = json!("source_position"),
            "intent" => last["payload"]["intent"]["rows"][0]["quarter_bits"] = json!(bits(29.0)),
            "payload_extra" => last["payload"]["trusted"] = json!(true),
            _ => unreachable!(),
        }
        let forged = editor::seal(data).unwrap();
        assert!(
            forged
                .edited(r#"{"schema_version":1,"operation":"undo"}"#)
                .is_err(),
            "{change}"
        );
    }
    let fixed = budget::peak(0, 0, 0, 0, 0).unwrap();
    let limit = budget::MAX_BUILD_BYTES;
    assert_eq!(budget::peak(0, limit - fixed, 0, 0, 0).unwrap(), limit);
    assert!(budget::peak(0, limit - fixed + 1, 0, 0, 0).is_err());
    assert!(budget::peak(usize::MAX, 0, 0, 0, 0).is_err());
    assert!(budget::edit_bound(usize::MAX).is_err());
    let raw_capture = capture();
    let encoded = raw_capture.encoded_len().unwrap();
    assert_eq!(
        interpretation::budget(encoded, 4096, 2).unwrap(),
        encoded * 32 + 4096 + 2 * 8192 + 16 * 1024 * 1024
    );
    let before = map.canonical_record().to_owned();
    for op in [
        request(&map, "extend_end", 2, 2, vec![raw(30, 30.0)]),
        json!({"schema_version":1,"operation":"translate","sample_offset":1,"sample_rate_hz":48000}),
    ] {
        assert!(map.edited_with_held(&op.to_string(), limit).is_err());
        assert_eq!(map.canonical_record(), before);
    }
}

#[test]
fn accepted_raw_window_large_existing_provenance_is_counted_in_inverse_not_confused_with_intent_limit()
 {
    let map = captured(&[0, 10, 20, 30]);
    let mut data = value(&map);
    let anchor = &mut data["record"]["accepted"]["anchors"][1];
    anchor["id"] = json!("i".repeat(4096));
    anchor["sample_edit"] = json!({"sample_index":557,"sample_rate_hz":48000,
        "evidence":"e".repeat(4096),"author":"a".repeat(4096)});
    let map = editor::seal(data).unwrap();
    let op = request(
        &map,
        "replace_interior",
        0,
        3,
        vec![existing(1, 8.5), existing(2, 21.5)],
    )
    .to_string();
    assert!(op.len() < 8192);
    let edited = map.edited(&op).unwrap();
    let data = value(&edited);
    let last = data["record"]["accepted"]["lineage"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert!(budget::serialized_len(&last["payload"]["inverse"]).unwrap() > 8192);
    geometry_equal(&undo(&edited), &map);
    let retained = map.canonical_record().len();
    let prospective = budget::edit_bound(map.canonical_record().len()).unwrap();
    let admitted = budget::edit(prospective, 0, retained).unwrap();
    assert!(
        map.edited_with_held(&op, budget::MAX_BUILD_BYTES - admitted + 1)
            .is_err()
    );
    assert!(budget::parse(usize::MAX, 0).is_err());
}

#[test]
fn accepted_raw_window_legacy_readback_unavailable_preserves_existing_editor_behavior() {
    let captured = captured(&[0, 10]);
    let data = value(&captured);
    let legacy = SourceBeatmap::manual(
        data["record"]["source"]["decoder_descriptor"].clone(),
        0.0,
        0.1,
    )
    .unwrap();
    Python::initialize();
    Python::attach(|py| {
        let object = Py::new(py, legacy.clone()).unwrap();
        let info = object.bind(py).call_method0("editor_review_info").unwrap();
        let info = info.cast::<PyDict>().unwrap();
        assert_eq!(
            info.get_item("status")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "unavailable"
        );
        assert!(
            info.get_item("capture_revision")
                .unwrap()
                .unwrap()
                .is_none()
        );
        let page = object
            .bind(py)
            .call_method1("editor_review_page", (0, 1, 0, 128))
            .unwrap();
        let page = page.cast::<PyDict>().unwrap();
        assert_eq!(page.get_item("anchors").unwrap().unwrap().len().unwrap(), 0);
        assert_eq!(page.get_item("events").unwrap().unwrap().len().unwrap(), 0);
    });
    let moved=legacy.edited(&json!({"schema_version":1,"operation":"translate","sample_offset":1,"sample_rate_hz":48000}).to_string()).unwrap();
    geometry_equal(&undo(&moved), &legacy);
}

#[test]
fn closed_window_and_interval_undo_refuse_self_consistent_impossible_predecessor_parent_metadata() {
    let base = captured(&[0, 10, 20, 30]);
    let window = base
        .edited(
            &request(
                &base,
                "replace_interior",
                0,
                3,
                vec![existing(1, 8.5), existing(2, 21.5)],
            )
            .to_string(),
        )
        .unwrap();
    let interval = base
        .edited(
            &json!({"schema_version":1,"operation":"set_coverage",
        "first_anchor_index":0,"last_anchor_index":3,"coverage":"trusted",
        "reviewed_quarter_notes":30.0})
            .to_string(),
        )
        .unwrap();
    Python::initialize();
    let can_undo = |map: &SourceBeatmap| {
        Python::attach(|py| {
            let object = Py::new(py, map.clone()).unwrap();
            object
                .bind(py)
                .call_method0("editor_info")
                .unwrap()
                .cast::<PyDict>()
                .unwrap()
                .get_item("can_undo")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        })
    };
    for changed in [&window, &interval] {
        assert!(can_undo(changed));
        geometry_equal(&undo(changed), &base);
        for bad_parent in [
            json!({"impossible":"parent"}),
            json!(7),
            json!("A".repeat(64)),
            json!("short"),
        ] {
            // Rehash the impossible predecessor and update every closed lineage
            // reference. Only predecessor metadata validity can reject this;
            // digest, geometry and deterministic Existing-only replay all agree.
            let mut predecessor = value(&base)["record"].clone();
            predecessor["accepted"]["parent_revision"] = bad_parent.clone();
            predecessor["accepted"]
                .as_object_mut()
                .unwrap()
                .remove("map_revision");
            let fake_parent =
                crate::audio_engine::cold_store::descriptor_digest(&predecessor).unwrap();
            let mut current = value(changed);
            let lineage = current["record"]["accepted"]["lineage"]
                .as_array_mut()
                .unwrap();
            let index = lineage.len() - 1;
            let last = &mut lineage[index];
            last["parent_revision"] = json!(fake_parent);
            last["id"] = json!(format!("editor-{}-{index}", &fake_parent[..16]));
            last["payload"]["predecessor_parent_revision"] = bad_parent;
            if last["kind"] == "refinement" {
                last["payload"]["intent"]["map_revision"] = json!(fake_parent);
            }
            let forged = editor::seal(current).unwrap();
            assert!(!can_undo(&forged));
            let before = forged.canonical_record().to_owned();
            assert!(
                forged
                    .edited(r#"{"schema_version":1,"operation":"undo"}"#)
                    .is_err()
            );
            assert_eq!(forged.canonical_record(), before);
        }
    }
}
