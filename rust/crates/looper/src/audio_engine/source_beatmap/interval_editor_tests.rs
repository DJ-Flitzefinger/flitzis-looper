fn interval_intent(operation: &str, first: usize, last: usize) -> Value {
    json!({
        "schema_version": 1,
        "operation": operation,
        "first_anchor_index": first,
        "last_anchor_index": last,
    })
}

fn count_intent(first: usize, last: usize, quarter_notes: f64) -> Value {
    let mut intent = interval_intent("set_count", first, last);
    intent["quarter_notes"] = json!(quarter_notes);
    intent
}

fn coverage_intent(first: usize, last: usize, coverage: &str, reviewed: Value) -> Value {
    let mut intent = interval_intent("set_coverage", first, last);
    intent["coverage"] = json!(coverage);
    intent["reviewed_quarter_notes"] = reviewed;
    intent
}

fn physical_anchors(map: &SourceBeatmap) -> Value {
    let mut anchors = record_of(map)["record"]["accepted"]["anchors"].clone();
    for anchor in anchors.as_array_mut().unwrap() {
        anchor.as_object_mut().unwrap().remove("beat_bits");
    }
    anchors
}

fn refuses_interval_intent_without_mutation(map: &SourceBeatmap, intent: Value) {
    let before = map.canonical_record().to_owned();
    let result = std::panic::catch_unwind(|| map.edited(&intent.to_string()));
    assert!(
        result.is_ok(),
        "invalid interval intent must not panic: {intent}"
    );
    assert!(result.unwrap().is_err(), "must refuse: {intent}");
    assert_eq!(map.canonical_record(), before);
}

#[test]
fn interval_count_affine_relabels_only_beats_and_keeps_raw_sample_and_source_identity() {
    let mut value = fixture(
        &[
            (0.0, -3.0),
            (0.5, -1.0),
            (1.25, -0.5),
            (2.75, 1.0),
            (4.0, 5.0),
            (5.5, 7.0),
        ],
        &[
            "trusted",
            "uncertain",
            "gap_unknown_count",
            "trusted",
            "trusted",
        ],
    );
    value["record"]["raw"]["beat_seconds_bits"] = json!([bits(0.5), bits(1.25), bits(2.75)]);
    value["record"]["raw"]["downbeat_seconds_bits"] = json!([bits(0.5)]);
    value["record"]["raw"]["timestamp_resolution_seconds_bits"] = json!(bits(1.0 / 48000.0));
    value["record"]["raw"]["analysis_input"] = json!({
        "pcm_sha256": "d".repeat(64),
        "sample_rate_hz": 48000,
        "frame_count": 480000,
        "source_zero_bits": bits(0.0),
        "preprocessing_identity": "interval-count-fixture-v1",
    });
    for index in 1..=3 {
        value["record"]["accepted"]["anchors"][index]["raw_beat_index"] = json!(index - 1);
    }
    value["record"]["accepted"]["anchors"][2]["sample_edit"] = json!({
        "sample_index": 60000,
        "sample_rate_hz": 48000,
        "evidence": "Retained explicit source-position correction",
        "author": "performer",
    });
    let base = prepared(value);
    let changed = edit(&base, count_intent(1, 3, 3.5));
    let before = record_of(&base);
    let after = record_of(&changed);
    assert_eq!(
        changed
            .inner
            .anchors
            .iter()
            .map(|anchor| anchor.beat)
            .collect::<Vec<_>>(),
        vec![-3.0, -1.0, -0.125, 2.5, 6.5, 8.5]
    );
    assert_eq!(positions(&changed), positions(&base));
    assert_eq!(physical_anchors(&changed), physical_anchors(&base));
    assert_eq!(after["record"]["source"], before["record"]["source"]);
    assert_eq!(after["record"]["raw"], before["record"]["raw"]);
    assert_eq!(
        after["record"]["accepted"]["intervals"],
        before["record"]["accepted"]["intervals"]
    );
    assert_eq!(
        after["record"]["accepted"]["lineage"][0]["kind"],
        "beat_count"
    );
    assert_eq!(
        after["record"]["accepted"]["lineage"][0]["payload"]["inverse"]["count_phase_policy"],
        "selected-clear-suffix-integral-mod-four-v1"
    );
    assert!(!changed.inner.at_source(1.0).supported());
    assert!(!changed.inner.at_source(2.0).supported());
    assert_eq!(changed.inner.at_source(3.0).local_bpm, Some(192.0));
    assert_eq!(base.inner.at_source(3.0).local_bpm, Some(192.0));
}

#[test]
fn interval_count_transports_only_integral_suffix_phase_and_never_invents_null_phase() {
    for (count, suffix_phase) in [(4.0, json!(1)), (1.0, json!(2)), (2.5, Value::Null)] {
        let mut value = four_four(fixture(
            &[
                (0.0, 0.0),
                (1.0, 1.0),
                (2.0, 2.0),
                (3.0, 3.0),
                (4.0, 4.0),
                (5.0, 5.0),
            ],
            &[
                "trusted",
                "trusted",
                "uncertain",
                "trusted",
                "gap_unknown_count",
            ],
        ));
        for (index, label) in [
            json!(i64::MIN),
            json!(-9),
            json!(2),
            json!(i64::MAX),
            Value::Null,
        ]
        .into_iter()
        .enumerate()
        {
            value["record"]["accepted"]["intervals"][index]["meter"]["downbeat_label"] = label;
        }
        let base = prepared(value);
        let changed = edit(&base, count_intent(1, 3, count));
        let before = record_of(&base);
        let after = record_of(&changed);
        let intervals = after["record"]["accepted"]["intervals"].as_array().unwrap();
        assert_eq!(intervals[0], before["record"]["accepted"]["intervals"][0]);
        assert!(intervals[1]["meter"]["downbeat_label"].is_null());
        assert!(intervals[2]["meter"]["downbeat_label"].is_null());
        assert_eq!(intervals[3]["meter"]["downbeat_label"], suffix_phase);
        assert!(intervals[4]["meter"]["downbeat_label"].is_null());
        for (index, interval) in intervals.iter().enumerate() {
            assert_eq!(
                interval["coverage"],
                before["record"]["accepted"]["intervals"][index]["coverage"]
            );
            assert_eq!(interval["meter"]["status"], "quarter_note_4_4_verified");
            assert_eq!(
                interval["meter"]["evidence"],
                before["record"]["accepted"]["intervals"][index]["meter"]["evidence"]
            );
        }
        assert_eq!(physical_anchors(&changed), physical_anchors(&base));
        let undone = edit(&changed, json!({"schema_version":1,"operation":"undo"}));
        assert_eq!(
            record_of(&undone)["record"]["accepted"]["anchors"],
            before["record"]["accepted"]["anchors"]
        );
        assert_eq!(
            record_of(&undone)["record"]["accepted"]["intervals"],
            before["record"]["accepted"]["intervals"]
        );
    }
}

#[test]
fn interval_unchanged_count_retains_every_anchor_bit_and_noncanonical_phase_label() {
    let mut value = four_four(fixture(
        &[(0.0, -0.0), (1.0, 0.25), (2.0, 2.0), (3.0, 5.0)],
        &["trusted", "gap_unknown_count", "uncertain"],
    ));
    value["record"]["accepted"]["intervals"][0]["meter"]["downbeat_label"] = json!(i64::MAX);
    value["record"]["accepted"]["intervals"][1]["meter"]["downbeat_label"] = json!(-17);
    let base = prepared(value);
    let changed = edit(&base, count_intent(0, 2, 2.0));
    let before = record_of(&base);
    let after = record_of(&changed);
    assert_eq!(
        after["record"]["accepted"]["anchors"],
        before["record"]["accepted"]["anchors"]
    );
    assert_eq!(
        after["record"]["accepted"]["intervals"],
        before["record"]["accepted"]["intervals"]
    );
    assert_eq!(
        changed.inner.anchors[0].beat.to_bits(),
        (-0.0_f64).to_bits()
    );
    assert!(changed.inner.can_undo);
    assert_ne!(changed.map_revision(), base.map_revision());
}

#[test]
fn interval_meter_changes_clear_old_phase_without_promoting_coverage_or_changing_geometry() {
    let mut value = fixture(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 3.0), (3.0, 4.0)],
        &["gap_unknown_count", "uncertain", "trusted"],
    );
    value["record"]["accepted"]["intervals"][0]["meter"]["downbeat_label"] = json!(17);
    value["record"]["accepted"]["intervals"][1]["meter"]["status"] =
        json!("quarter_note_4_4_verified");
    value["record"]["accepted"]["intervals"][1]["meter"]["downbeat_label"] = json!(i64::MAX);
    let base = prepared(value);
    let mut intent = interval_intent("set_meter", 0, 2);
    intent["meter_status"] = json!("quarter_note_4_4_verified");
    let changed = edit(&base, intent);
    let before = record_of(&base);
    let after = record_of(&changed);
    assert_eq!(
        after["record"]["accepted"]["anchors"],
        before["record"]["accepted"]["anchors"]
    );
    assert!(after["record"]["accepted"]["intervals"][0]["meter"]["downbeat_label"].is_null());
    assert_eq!(
        after["record"]["accepted"]["intervals"][1]["meter"]["downbeat_label"],
        i64::MAX
    );
    assert_eq!(
        after["record"]["accepted"]["intervals"][2],
        before["record"]["accepted"]["intervals"][2]
    );
    for index in 0..3 {
        assert_eq!(
            after["record"]["accepted"]["intervals"][index]["coverage"],
            before["record"]["accepted"]["intervals"][index]["coverage"]
        );
    }
    assert!(!changed.inner.at_source(0.5).supported());
    assert!(!changed.inner.at_source(1.5).supported());
    let mut clear = interval_intent("set_meter", 1, 2);
    clear["meter_status"] = json!("unknown");
    let cleared = edit(&changed, clear);
    let cleared_record = record_of(&cleared);
    assert_eq!(
        cleared_record["record"]["accepted"]["intervals"][1]["meter"]["status"],
        "unknown"
    );
    assert!(
        cleared_record["record"]["accepted"]["intervals"][1]["meter"]["downbeat_label"].is_null()
    );
    assert_eq!(cleared_record["record"]["raw"], before["record"]["raw"]);
}

#[test]
fn interval_downbeat_requires_existing_meter_and_accepts_exact_i64_without_touching_geometry() {
    let base = prepared(fixture(
        &[(0.0, 0.0), (1.0, 2.0), (2.0, 4.0)],
        &["trusted", "gap_unknown_count"],
    ));
    let mut label = interval_intent("set_downbeat", 0, 2);
    label["downbeat_label"] = json!(i64::MIN);
    refuses_interval_intent_without_mutation(&base, label.clone());
    let mut meter = interval_intent("set_meter", 0, 1);
    meter["meter_status"] = json!("quarter_note_4_4_verified");
    let partly_verified = edit(&base, meter);
    refuses_interval_intent_without_mutation(&partly_verified, label.clone());
    label["last_anchor_index"] = json!(1);
    for exact in [i64::MIN, i64::MAX, -5] {
        label["downbeat_label"] = json!(exact);
        let changed = edit(&partly_verified, label.clone());
        let before = record_of(&partly_verified);
        let after = record_of(&changed);
        assert_eq!(
            after["record"]["accepted"]["anchors"],
            before["record"]["accepted"]["anchors"]
        );
        assert_eq!(
            after["record"]["accepted"]["intervals"][0]["meter"]["downbeat_label"],
            exact
        );
        assert_eq!(changed.inner.at_source(0.5).downbeat_label, Some(exact));
        assert_eq!(changed.inner.at_source(0.5).local_bpm, Some(120.0));
        assert_eq!(
            after["record"]["accepted"]["intervals"][0]["coverage"],
            "trusted"
        );
        assert_eq!(
            after["record"]["accepted"]["intervals"][1],
            before["record"]["accepted"]["intervals"][1]
        );
    }
}

#[test]
fn interval_legacy_unknown_meter_phase_and_verified_null_remain_distinct_on_restore_and_clear() {
    let mut value = fixture(
        &[(0.0, 0.0), (1.0, 4.0), (2.0, 8.0)],
        &["trusted", "trusted"],
    );
    value["record"]["accepted"]["intervals"][0]["meter"]["downbeat_label"] = json!(-5);
    value["record"]["accepted"]["intervals"][1]["meter"]["status"] =
        json!("quarter_note_4_4_verified");
    let base = prepared(value);
    assert_eq!(
        record_of(&base)["record"]["accepted"]["intervals"][0]["meter"]["downbeat_label"],
        -5
    );
    assert_eq!(base.inner.at_source(0.5).meter_status, "unknown");
    assert_eq!(base.inner.at_source(0.5).downbeat_label, None);
    assert_eq!(
        base.inner.at_source(1.5).meter_status,
        "quarter_note_4_4_verified"
    );
    assert_eq!(base.inner.at_source(1.5).downbeat_label, None);
    assert!(base.inner.source_after_beats(1.0, 4.0, true).supported());
    let restored = SourceBeatmap::parse(base.canonical_record()).unwrap();
    assert_eq!(restored.canonical_record(), base.canonical_record());
    let mut clear = interval_intent("set_downbeat", 0, 2);
    clear["downbeat_label"] = Value::Null;
    let cleared = edit(&restored, clear);
    let after = record_of(&cleared);
    assert!(after["record"]["accepted"]["intervals"][0]["meter"]["downbeat_label"].is_null());
    assert_eq!(
        after["record"]["accepted"]["intervals"][0]["meter"]["status"],
        "unknown"
    );
    assert_eq!(
        after["record"]["accepted"]["intervals"][1]["meter"]["status"],
        "quarter_note_4_4_verified"
    );
    assert_eq!(
        after["record"]["accepted"]["anchors"],
        record_of(&base)["record"]["accepted"]["anchors"]
    );
    let undone = edit(&cleared, json!({"schema_version":1,"operation":"undo"}));
    assert_eq!(
        record_of(&undone)["record"]["accepted"]["intervals"],
        record_of(&base)["record"]["accepted"]["intervals"]
    );
}

#[test]
fn interval_coverage_requires_current_exact_count_and_is_the_only_operation_that_removes_a_gap() {
    let base = prepared(fixture(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 3.0), (3.0, 4.0)],
        &["gap_unknown_count", "uncertain", "trusted"],
    ));
    let counted = edit(&base, count_intent(0, 2, 4.5));
    let mut meter = interval_intent("set_meter", 0, 2);
    meter["meter_status"] = json!("quarter_note_4_4_verified");
    let interpreted = edit(&counted, meter);
    assert!(!interpreted.inner.at_source(0.5).supported());
    assert!(!interpreted.inner.at_source(1.5).supported());
    for coverage in ["trusted", "uncertain"] {
        for bad_count in [
            Value::Null,
            json!(0),
            json!(-1),
            json!(3.0),
            json!(4.5_f64.next_up()),
        ] {
            refuses_interval_intent_without_mutation(
                &interpreted,
                coverage_intent(0, 2, coverage, bad_count),
            );
        }
        let changed = edit(&interpreted, coverage_intent(0, 2, coverage, json!(4.5)));
        let before = record_of(&interpreted);
        let after = record_of(&changed);
        assert_eq!(
            after["record"]["accepted"]["anchors"],
            before["record"]["accepted"]["anchors"]
        );
        assert_eq!(after["record"]["raw"], before["record"]["raw"]);
        for index in 0..2 {
            assert_eq!(
                after["record"]["accepted"]["intervals"][index]["coverage"],
                coverage
            );
            assert_eq!(
                after["record"]["accepted"]["intervals"][index]["meter"],
                before["record"]["accepted"]["intervals"][index]["meter"]
            );
        }
        assert_eq!(
            after["record"]["accepted"]["intervals"][2],
            before["record"]["accepted"]["intervals"][2]
        );
        assert_eq!(
            after["record"]["accepted"]["lineage"][2]["kind"],
            "coverage"
        );
        assert_eq!(
            changed.inner.at_source(0.5).supported(),
            coverage == "trusted"
        );
        let gap = edit(
            &changed,
            coverage_intent(0, 2, "gap_unknown_count", Value::Null),
        );
        assert!(!gap.inner.at_source(0.5).supported());
        assert_eq!(
            record_of(&gap)["record"]["accepted"]["anchors"],
            after["record"]["accepted"]["anchors"]
        );
    }
}

#[test]
fn interval_mixed_source_and_interpretation_undo_restores_exact_predecessors_and_operation_kinds() {
    let base = prepared(fixture(
        &[(0.0, -0.0), (1.0, 1.0), (2.0, 3.0), (3.0, 5.0)],
        &["gap_unknown_count", "uncertain", "trusted"],
    ));
    let mut meter = interval_intent("set_meter", 0, 2);
    meter["meter_status"] = json!("quarter_note_4_4_verified");
    let mut downbeat = interval_intent("set_downbeat", 0, 2);
    downbeat["downbeat_label"] = json!(-7);
    let actions = [
        (count_intent(0, 2, 4.5), "beat_count"),
        (meter, "downbeat_meter"),
        (downbeat, "downbeat_meter"),
        (coverage_intent(0, 2, "trusted", json!(4.5)), "coverage"),
        (
            json!({"schema_version":1,"operation":"move_beat","beat":4.5,"sample_index":96001,"sample_rate_hz":48000}),
            "source_position",
        ),
        (
            json!({"schema_version":1,"operation":"translate","sample_offset":-1,"sample_rate_hz":48000}),
            "whole_map_translation",
        ),
    ];
    let mut stages = vec![base];
    for (intent, kind) in &actions {
        let changed = edit(stages.last().unwrap(), intent.clone());
        let changed_record = record_of(&changed);
        assert_eq!(
            changed_record["record"]["accepted"]["lineage"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()["kind"],
            *kind
        );
        assert!(changed.inner.can_undo);
        stages.push(changed);
    }
    let mut current = SourceBeatmap::parse(stages.last().unwrap().canonical_record()).unwrap();
    for index in (0..actions.len()).rev() {
        let expected = record_of(&stages[index]);
        let original_operation =
            record_of(&stages[index + 1])["record"]["accepted"]["lineage"][index].clone();
        current = edit(&current, json!({"schema_version":1,"operation":"undo"}));
        let actual = record_of(&current);
        assert_eq!(
            actual["record"]["accepted"]["anchors"],
            expected["record"]["accepted"]["anchors"]
        );
        assert_eq!(
            actual["record"]["accepted"]["intervals"],
            expected["record"]["accepted"]["intervals"]
        );
        assert_eq!(actual["record"]["source"], expected["record"]["source"]);
        assert_eq!(actual["record"]["raw"], expected["record"]["raw"]);
        let undo = actual["record"]["accepted"]["lineage"]
            .as_array()
            .unwrap()
            .last()
            .unwrap();
        assert_eq!(undo["kind"], original_operation["kind"]);
        assert_eq!(
            undo["payload"]["undoes_operation_id"],
            original_operation["id"]
        );
        assert_eq!(current.inner.can_undo, index != 0);
        current = SourceBeatmap::parse(current.canonical_record()).unwrap();
    }
    refuses_interval_intent_without_mutation(
        &current,
        json!({"schema_version":1,"operation":"undo"}),
    );
}

#[test]
fn interval_forged_count_inverse_cannot_replace_a_two_anchor_predecessor_count() {
    let base = prepared(fixture(&[(0.0, 0.0), (1.0, 2.0)], &["trusted"]));
    let changed = edit(&base, count_intent(0, 1, 4.0));
    assert!(changed.inner.can_undo);
    let mut forged = record_of(&changed);
    // Both 2 and 3 would deterministically become 4 under set_count. Only the
    // recorded predecessor revision distinguishes the actual previous count.
    forged["record"]["accepted"]["lineage"][0]["payload"]["inverse"]["beat_bits"][1] =
        json!(bits(3.0));
    imported_undo_refuses_without_panic_or_mutation(forged);
    let mut forged = record_of(&changed);
    forged["record"]["accepted"]["lineage"][0]["payload"]["predecessor_parent_revision"] =
        json!("e".repeat(64));
    imported_undo_refuses_without_panic_or_mutation(forged);
}

#[test]
fn interval_closed_undo_binds_overwritten_interpretation_and_unchanged_sample_provenance() {
    let base = prepared(fixture(&[(0.0, 0.0), (1.0, 2.0)], &["gap_unknown_count"]));
    let covered = edit(&base, coverage_intent(0, 1, "trusted", json!(2.0)));
    let mut forged = record_of(&covered);
    forged["record"]["accepted"]["lineage"][0]["payload"]["inverse"]["intervals"][0]["coverage"] =
        json!("uncertain");
    imported_undo_refuses_without_panic_or_mutation(forged);
    let mut meter = interval_intent("set_meter", 0, 1);
    meter["meter_status"] = json!("quarter_note_4_4_verified");
    let verified = edit(&base, meter);
    let mut forged = record_of(&verified);
    forged["record"]["accepted"]["lineage"][0]["payload"]["inverse"]["intervals"][0]["meter"]["downbeat_label"] =
        json!(17);
    imported_undo_refuses_without_panic_or_mutation(forged);
    let counted = edit(&base, count_intent(0, 1, 4.0));
    for original in [
        record_of(&counted),
        record_of(&covered),
        record_of(&verified),
    ] {
        let mut forged = original;
        // This changes only retained sample-edit provenance, not either axis.
        forged["record"]["accepted"]["anchors"][1]["sample_edit"] = json!({
            "sample_index": 48001,
            "sample_rate_hz": 48000,
            "evidence": "Imported sample-only mutation",
            "author": "performer",
        });
        imported_undo_refuses_without_panic_or_mutation(forged);
    }
}

#[test]
fn interval_imported_inverse_shapes_indices_phase_policy_and_kinds_never_become_authority() {
    let base = prepared(four_four(fixture(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)],
        &["trusted", "trusted", "trusted"],
    )));
    let changed = edit(&base, count_intent(1, 2, 2.0));
    let original = record_of(&changed);
    for (field, bad) in [
        ("action", json!("restore_positions")),
        ("first_anchor_index", json!(u64::MAX)),
        ("first_anchor_index", json!(-1)),
        ("first_anchor_index", json!(1.0)),
        ("last_anchor_index", json!(250000)),
        ("last_anchor_index", json!(1)),
        ("beat_bits", json!([])),
        ("beat_bits", json!([bits(1.0), bits(2.0)])),
        ("beat_bits", json!([bits(1.0), bits(f64::NAN), bits(3.0)])),
        ("beat_bits", json!([bits(1.0), bits(0.5), bits(3.0)])),
        ("intervals", json!([])),
        ("count_phase_policy", Value::Null),
        ("count_phase_policy", json!("silently-preserve-phase")),
        ("extra", json!(true)),
    ] {
        let mut forged = original.clone();
        forged["record"]["accepted"]["lineage"][0]["payload"]["inverse"][field] = bad;
        imported_undo_refuses_without_panic_or_mutation(forged);
    }
    for kind in [
        "coverage",
        "downbeat_meter",
        "source_position",
        "refinement",
    ] {
        let mut forged = original.clone();
        forged["record"]["accepted"]["lineage"][0]["kind"] = json!(kind);
        imported_undo_refuses_without_panic_or_mutation(forged);
    }
    for (field, bad) in [
        ("first_anchor_index", json!(0)),
        ("last_anchor_index", json!(3)),
        ("quarter_notes", json!(3.0)),
    ] {
        let mut forged = original.clone();
        forged["record"]["accepted"]["lineage"][0]["payload"]["intent"][field] = bad;
        imported_undo_refuses_without_panic_or_mutation(forged);
    }
}

#[test]
fn interval_all_operations_refuse_invalid_or_ambiguous_ranges_and_unknown_fields() {
    let base = prepared(fixture(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)],
        &["trusted", "trusted"],
    ));
    let mut meter = interval_intent("set_meter", 0, 1);
    meter["meter_status"] = json!("quarter_note_4_4_verified");
    let mut downbeat = interval_intent("set_downbeat", 0, 1);
    downbeat["downbeat_label"] = Value::Null;
    for original in [
        count_intent(0, 1, 2.0),
        meter,
        downbeat,
        coverage_intent(0, 1, "trusted", json!(1.0)),
    ] {
        for (field, bad) in [
            ("first_anchor_index", json!(-1)),
            ("first_anchor_index", json!(0.0)),
            ("first_anchor_index", json!(u64::MAX)),
            ("first_anchor_index", json!(1)),
            ("last_anchor_index", json!(0)),
            ("last_anchor_index", json!(3)),
            ("last_anchor_index", json!(u64::MAX)),
            ("schema_version", json!(2)),
            ("surprise", json!(true)),
        ] {
            let mut intent = original.clone();
            intent[field] = bad;
            refuses_interval_intent_without_mutation(&base, intent);
        }
    }
    for intent in [
        json!({"schema_version":1,"operation":"set_downbeat","first_anchor_index":0,"last_anchor_index":1}),
        json!({"schema_version":1,"operation":"set_downbeat","first_anchor_index":0,"last_anchor_index":1,"downbeat_label":1.0}),
        json!({"schema_version":1,"operation":"set_downbeat","first_anchor_index":0,"last_anchor_index":1,"downbeat_label":u64::MAX}),
        json!({"schema_version":1,"operation":"set_meter","first_anchor_index":0,"last_anchor_index":1,"meter_status":"4/4"}),
        json!({"schema_version":1,"operation":"set_coverage","first_anchor_index":0,"last_anchor_index":1,"coverage":"trusted"}),
        json!({"schema_version":1,"operation":"set_coverage","first_anchor_index":0,"last_anchor_index":1,"coverage":"supported","reviewed_quarter_notes":1.0}),
    ] {
        refuses_interval_intent_without_mutation(&base, intent);
    }
}

#[test]
fn interval_count_refuses_nonfinite_overflow_and_collapsed_spacing_without_clamping() {
    let base = prepared(fixture(
        &[(0.0, 1.0), (1.0, 2.0), (2.0, 3.0), (3.0, 4.0)],
        &["trusted", "trusted", "trusted"],
    ));
    for count in [
        0.0,
        -0.0,
        -1.0,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        f64::MAX,
    ] {
        refuses_interval_intent_without_mutation(&base, count_intent(0, 2, count));
    }
    let before = base.canonical_record().to_owned();
    for count in ["NaN", "Infinity", "-Infinity", "1e309", "null"] {
        let encoded = format!(
            "{{\"schema_version\":1,\"operation\":\"set_count\",\"first_anchor_index\":0,\"last_anchor_index\":2,\"quarter_notes\":{count}}}"
        );
        assert!(base.edited(&encoded).is_err());
        assert_eq!(base.canonical_record(), before);
    }
    let high_origin = prepared(fixture(
        &[(0.0, 1.0e16), (1.0, 1.0e16 + 64.0), (2.0, 1.0e16 + 128.0)],
        &["trusted", "trusted"],
    ));
    refuses_interval_intent_without_mutation(&high_origin, count_intent(0, 1, 1.0));
}
