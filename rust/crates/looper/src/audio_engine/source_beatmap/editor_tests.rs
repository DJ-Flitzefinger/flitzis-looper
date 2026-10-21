fn edit(map: &SourceBeatmap, intent: Value) -> SourceBeatmap {
    map.edited(&intent.to_string()).unwrap()
}
fn record_of(map: &SourceBeatmap) -> Value {
    serde_json::from_str(map.canonical_record()).unwrap()
}
fn positions(map: &SourceBeatmap) -> Vec<Value> {
    record_of(map)["record"]["accepted"]["anchors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|anchor| anchor["source_seconds_bits"].clone())
        .collect()
}
fn four_four(mut value: Value) -> Value {
    for interval in value["record"]["accepted"]["intervals"]
        .as_array_mut()
        .unwrap()
    {
        interval["meter"]["status"] = json!("quarter_note_4_4_verified");
    }
    value
}

mod interval_editor_tests {
    use super::*;
    include!("interval_editor_tests.rs");
}

fn phased_map(anchors: &[(f64, f64)], labels: &[Option<i64>]) -> SourceBeatmap {
    let mut value = four_four(fixture(anchors, &vec!["trusted"; labels.len()]));
    for (interval, label) in value["record"]["accepted"]["intervals"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(labels)
    {
        interval["meter"]["downbeat_label"] = json!(label);
    }
    prepared(value)
}

#[test]
fn native_phase_grid_reduces_i64_before_float_and_keeps_coarse_nonzero_origin() {
    // i64::MAX is phase three; conversion to f64 first would falsely yield phase zero.
    let map = phased_map(&[(0.0, 0.0), (10.0, 20.0)], &[Some(i64::MAX)]);
    let fine = map.viewport(0.0, 10.0, 0.0, 100.0, 2048);
    assert!(
        fine.lines
            .iter()
            .any(|line| line.downbeat && line.loop_beat == 4.0)
    );
    assert!(
        fine.lines
            .iter()
            .all(|line| !line.downbeat || (line.loop_beat - 4.0).rem_euclid(4.0) == 0.0)
    );
    let coarse = map.viewport(0.0, 10.0, 0.0, 8.0, 2048);
    assert_eq!(coarse.step, 4.0);
    assert_eq!(
        coarse
            .lines
            .iter()
            .map(|line| line.loop_beat - 1.0)
            .collect::<Vec<_>>(),
        vec![3.0, 7.0, 11.0, 15.0, 19.0]
    );
    assert!(coarse.lines.iter().all(|line| line.downbeat));
    let far = map.viewport(0.0, 10.0, 0.0, 0.1, 2048);
    assert_eq!(far.step, 16.0);
    assert_eq!(
        far.lines
            .iter()
            .map(|line| line.loop_beat - 1.0)
            .collect::<Vec<_>>(),
        vec![3.0, 19.0]
    );
    let snapped = map.inner.nearest_grid(2.0, 4.0);
    assert_eq!(snapped.beat, Some(3.0));
    assert_eq!(snapped.source_seconds, Some(1.5));
}

#[test]
fn coarse_projection_obeys_each_segment_phase_and_refuses_incompatible_bar_snap() {
    let map = phased_map(&[(0.0, 0.0), (2.0, 4.0), (5.0, 10.0)], &[Some(1), Some(2)]);
    let view = map.viewport(0.0, 5.0, 0.0, 6.0, 2048);
    assert_eq!(view.step, 4.0);
    assert_eq!(
        view.lines
            .iter()
            .map(|line| line.loop_beat - 1.0)
            .collect::<Vec<_>>(),
        vec![1.0, 6.0, 10.0]
    );
    assert!(view.lines.iter().all(|line| line.downbeat));
    assert_eq!(
        map.inner.nearest_grid(1.9, 4.0).coordinate_status,
        "unsupported_crossing"
    );
    assert!(map.inner.nearest_grid(1.9, 0.25).supported());
}

#[test]
fn unknown_phase_has_no_bar_claim_but_four_quarter_duration_remains_supported() {
    let map = phased_map(&[(0.0, 0.0), (10.0, 20.0)], &[None]);
    let view = map.viewport(0.0, 10.0, 0.0, 100.0, 2048);
    assert_eq!(view.status, "unknown_downbeat");
    assert!(view.lines.iter().all(|line| !line.downbeat));
    assert_eq!(
        map.inner.nearest_grid(1.0, 4.0).coordinate_status,
        "unsupported_downbeat"
    );
    assert!(map.inner.nearest_grid(1.0, 0.25).supported());
    assert_eq!(
        map.inner.source_after_beats(0.5, 4.0, true).source_seconds,
        Some(2.5)
    );
}

#[test]
fn derived_inspector_metadata_preserves_equivalent_signed_labels_and_virtual_endpoints() {
    let map = phased_map(
        &[(-1.0, -2.0), (1.0, 2.0), (10.0, 20.0)],
        &[Some(-3), Some(i64::MIN + 1)],
    );
    assert_eq!(map.inner.phase_changes.as_ref(), &[0, 0]);
    assert_eq!(map.inner.coverage_changes.as_ref(), &[0, 0]);
    assert_eq!(map.inner.meter_changes.as_ref(), &[0, 0]);
    assert_eq!(map.inner.sample_index(-1.0), None);
    assert_eq!(map.inner.sample_index(10.0), None);
    assert_eq!(map.inner.sample_index(1.0), Some(48000));
    assert_eq!(map.inner.segments[0].downbeat_label, Some(-3));
    let mut legacy = record_of(&map);
    legacy["record"]["accepted"]["intervals"][0]["meter"]["status"] = json!("unknown");
    let legacy = prepared(legacy);
    assert_eq!(legacy.inner.at_source(0.0).downbeat_label, None);
    assert_eq!(legacy.inner.phase_changes.as_ref(), &[0, 1]);
}

#[test]
fn phased_viewport_keeps_total_line_and_anchor_capacity_and_final_boundaries() {
    let map = phased_map(
        &[(0.0, 0.0), (1.5, 3.0), (3.5, 7.0), (5.5, 11.0)],
        &[Some(3), Some(3), Some(3)],
    );
    let view = map.viewport(0.0, 5.5, 0.0, 8.0, 7);
    assert!(view.lines.len() + view.anchors.len() <= 7);
    assert_eq!(
        view.lines
            .iter()
            .filter(|line| line.loop_beat == 4.0)
            .count(),
        1
    );
    assert_eq!(
        view.lines
            .iter()
            .filter(|line| line.loop_beat == 12.0)
            .count(),
        1
    );
    assert_eq!(
        map.viewport(0.0, 5.5, 0.0, 8.0, 3).status,
        "projection_limit"
    );
    assert_eq!(map.viewport(0.0, 5.5, 0.0, 8.0, 2049).status, "invalid");
}

#[test]
fn unrepresentable_phase_at_large_quarter_coordinates_refuses_instead_of_claiming_zero_phase() {
    let origin = (1_u64 << 54) as f64;
    let map = phased_map(&[(0.0, origin), (10.0, origin + 256.0)], &[Some(1)]);
    let snapped = map.inner.nearest_grid(5.0, 4.0);
    assert_eq!(snapped.coordinate_status, "numerical_unavailable");
    let view = map.viewport(0.0, 10.0, 0.0, 0.1, 2048);
    assert!(view.lines.iter().all(|line| !line.downbeat));
    assert_ne!(view.status, "complete");
}

#[test]
fn manual_seed_keeps_empty_raw_and_explicit_quarter_meter_full_source_coverage() {
    let value = fixture(&[(0.0, 0.0), (10.0, 20.0)], &["trusted"]);
    let map = SourceBeatmap::manual(
        value["record"]["source"]["decoder_descriptor"].clone(),
        0.123,
        0.51,
    )
    .unwrap();
    let value = record_of(&map);
    assert_eq!(value["record"]["raw"]["beat_seconds_bits"], json!([]));
    assert_eq!(value["record"]["raw"]["downbeat_seconds_bits"], json!([]));
    assert_eq!(
        value["record"]["raw"]["detector"]["detector_id"],
        "explicit-manual-source-grid"
    );
    assert!(map.inner.at_source(0.0).supported());
    assert!(map.inner.at_source(10.0).supported());
    assert!((map.inner.at_source(0.123).beat.unwrap()).abs() < 1e-13);
    assert_eq!(
        map.inner.at_source(0.123).meter_status,
        "quarter_note_4_4_verified"
    );
    assert!(!map.inner.can_undo);
}

#[test]
fn sample_insert_is_variable_local_and_undo_restores_exact_anchor_geometry_raw_meter() {
    let base = prepared(four_four(fixture(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)],
        &["trusted", "trusted"],
    )));
    let moved = edit(
        &base,
        json!({"schema_version":1,"operation":"move_beat","beat":0.5,"sample_index":24001,"sample_rate_hz":48000}),
    );
    assert_eq!(moved.inner.anchors.len(), 4);
    assert_eq!(
        moved.inner.at_beat(0.5).source_seconds,
        Some(24001.0 / 48000.0)
    );
    assert_ne!(
        moved.inner.at_source(0.1).local_bpm,
        moved.inner.at_source(0.9).local_bpm
    );
    assert_eq!(
        record_of(&moved)["record"]["raw"],
        record_of(&base)["record"]["raw"]
    );
    assert_eq!(
        record_of(&moved)["record"]["accepted"]["parent_revision"],
        json!(base.map_revision())
    );
    let undone = edit(&moved, json!({"schema_version":1,"operation":"undo"}));
    assert_ne!(undone.map_revision(), base.map_revision());
    assert_eq!(positions(&undone), positions(&base));
    assert_eq!(
        record_of(&undone)["record"]["accepted"]["intervals"],
        record_of(&base)["record"]["accepted"]["intervals"]
    );
    assert!(!undone.inner.can_undo);
    assert!(
        undone
            .edited(&json!({"schema_version":1,"operation":"undo"}).to_string())
            .is_err()
    );
}

#[test]
fn existing_anchor_correction_retains_raw_link_and_exact_undo_provenance() {
    let base = prepared(fixture(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)],
        &["trusted", "trusted"],
    ));
    let moved = edit(
        &base,
        json!({"schema_version":1,"operation":"move_beat","beat":1.0,"sample_index":48001,"sample_rate_hz":48000}),
    );
    let value = record_of(&moved);
    assert_eq!(
        value["record"]["accepted"]["anchors"][1]["sample_edit"]["sample_index"],
        48001
    );
    let undone = edit(&moved, json!({"schema_version":1,"operation":"undo"}));
    assert_eq!(
        record_of(&undone)["record"]["accepted"]["anchors"],
        record_of(&base)["record"]["accepted"]["anchors"]
    );
}

#[test]
fn signed_translation_keeps_beats_raw_gaps_and_restores_exact_binary_positions() {
    let base = prepared(fixture(
        &[(0.1, 0.0), (0.3, 1.0), (1.7, 2.0)],
        &["trusted", "gap_unknown_count"],
    ));
    let shifted = edit(
        &base,
        json!({"schema_version":1,"operation":"translate","sample_offset":-48001,"sample_rate_hz":48000}),
    );
    assert!(shifted.inner.anchors[0].seconds < 0.0);
    assert_eq!(
        shifted
            .inner
            .anchors
            .iter()
            .map(|a| a.beat)
            .collect::<Vec<_>>(),
        vec![0.0, 1.0, 2.0]
    );
    assert_eq!(
        record_of(&shifted)["record"]["raw"],
        record_of(&base)["record"]["raw"]
    );
    assert_eq!(
        record_of(&shifted)["record"]["accepted"]["intervals"],
        record_of(&base)["record"]["accepted"]["intervals"]
    );
    let undone = edit(&shifted, json!({"schema_version":1,"operation":"undo"}));
    assert_eq!(positions(&undone), positions(&base));
}

#[test]
fn successive_undo_revisions_walk_corrections_without_toggling_prior_undo() {
    let base = prepared(fixture(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)],
        &["trusted", "trusted"],
    ));
    let moved = edit(
        &base,
        json!({"schema_version":1,"operation":"move_beat","beat":0.5,"sample_index":24001,"sample_rate_hz":48000}),
    );
    let shifted = edit(
        &moved,
        json!({"schema_version":1,"operation":"translate","sample_offset":-1,"sample_rate_hz":48000}),
    );
    let undo_shift = edit(&shifted, json!({"schema_version":1,"operation":"undo"}));
    assert_eq!(positions(&undo_shift), positions(&moved));
    assert!(undo_shift.inner.can_undo);
    let undo_move = edit(&undo_shift, json!({"schema_version":1,"operation":"undo"}));
    assert_eq!(positions(&undo_move), positions(&base));
    assert!(!undo_move.inner.can_undo);
    assert!(
        undo_move
            .edited(&json!({"schema_version":1,"operation":"undo"}).to_string())
            .is_err()
    );
    assert_eq!(
        record_of(&undo_move)["record"]["accepted"]["lineage"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
}

fn imported_undo_refuses_without_panic_or_mutation(value: Value) {
    // Generic historical lineage remains accepted durable provenance. Only the
    // closed editor contract can supply an executable inverse.
    let map = prepared(value);
    assert!(!map.inner.can_undo);
    let before = map.canonical_record().to_owned();
    let outcome = std::panic::catch_unwind(|| {
        map.edited(&json!({"schema_version":1,"operation":"undo"}).to_string())
    });
    assert!(outcome.is_ok(), "imported provenance must never panic");
    assert!(outcome.unwrap().is_err());
    assert_eq!(map.canonical_record(), before);
}

#[test]
fn imported_non_editor_or_malformed_source_inverse_remains_data_and_cannot_execute() {
    let base = prepared(fixture(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)],
        &["trusted", "trusted"],
    ));
    let moved = edit(
        &base,
        json!({"schema_version":1,"operation":"move_beat","beat":1.0,"sample_index":48001,"sample_rate_hz":48000}),
    );
    let original = record_of(&moved);
    for kind in [
        "beat_count",
        "downbeat_meter",
        "refinement",
        "whole_map_translation",
    ] {
        let mut value = original.clone();
        value["record"]["accepted"]["lineage"][0]["kind"] = json!(kind);
        imported_undo_refuses_without_panic_or_mutation(value);
    }
    for (path, bad) in [
        ("index", json!(u64::MAX)),
        ("action", json!("insert_anchor")),
        ("action", json!("unknown")),
        ("index", json!(-1)),
        ("index", json!(1.0)),
        ("extra", json!(true)),
        ("anchor", json!({})),
    ] {
        let mut value = original.clone();
        value["record"]["accepted"]["lineage"][0]["payload"]["inverse"][path] = bad;
        imported_undo_refuses_without_panic_or_mutation(value);
    }
    let mut value = original.clone();
    value["record"]["accepted"]["lineage"][0]["payload"]["inverse"]["anchor"]["source_seconds_bits"] =
        json!(bits(2.0));
    imported_undo_refuses_without_panic_or_mutation(value);
    for (field, bad) in [
        ("schema_version", json!(2)),
        ("sample_rate_hz", json!(44100)),
        ("sample_index", json!(480000)),
        ("operation", json!("beat_count")),
    ] {
        let mut value = original.clone();
        value["record"]["accepted"]["lineage"][0]["payload"]["intent"][field] = bad;
        imported_undo_refuses_without_panic_or_mutation(value);
    }
    let mut value = original;
    value["record"]["accepted"]["lineage"][0]["payload"]["extra"] = json!(true);
    imported_undo_refuses_without_panic_or_mutation(value);
    let inserted = edit(
        &base,
        json!({"schema_version":1,"operation":"move_beat","beat":0.5,"sample_index":24001,"sample_rate_hz":48000}),
    );
    let mut value = record_of(&inserted);
    value["record"]["accepted"]["intervals"][1]["coverage"] = json!("gap_unknown_count");
    imported_undo_refuses_without_panic_or_mutation(value);
}

#[test]
fn imported_translation_positions_and_forged_undo_targets_refuse_safely() {
    let base = prepared(fixture(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)],
        &["trusted", "trusted"],
    ));
    let shifted = edit(
        &base,
        json!({"schema_version":1,"operation":"translate","sample_offset":-1,"sample_rate_hz":48000}),
    );
    let original = record_of(&shifted);
    for bad in [
        json!("not positions"),
        json!([]),
        json!([bits(0.0), bits(1.0)]),
        json!([bits(0.0), "nan", bits(2.0)]),
        json!([bits(0.0), bits(2.0), bits(1.0)]),
        json!([bits(0.0), bits(1.0), bits(3.0)]),
    ] {
        let mut value = original.clone();
        value["record"]["accepted"]["lineage"][0]["payload"]["inverse"]["positions"] = bad;
        imported_undo_refuses_without_panic_or_mutation(value);
    }
    let undone = edit(&shifted, json!({"schema_version":1,"operation":"undo"}));
    let mut value = record_of(&undone);
    value["record"]["accepted"]["lineage"][1]["payload"]["undoes_operation_id"] =
        json!("unknown-operation");
    imported_undo_refuses_without_panic_or_mutation(value);
    let mut value = record_of(&undone);
    value["record"]["accepted"]["lineage"][1]["kind"] = json!("source_position");
    imported_undo_refuses_without_panic_or_mutation(value);
    let mut malformed = json!({"anchors":[],"intervals":[]});
    // Internal inverse helpers also refuse invalid insertion bounds before Vec::insert.
    assert!(
        super::super::editor::apply_inverse(
            &mut malformed,
            &json!({"action":"insert_anchor","index":u64::MAX,"anchor":{},"interval":{}})
        )
        .is_err()
    );
    let mut malformed =
        json!({"anchors":record_of(&base)["record"]["accepted"]["anchors"],"intervals":[]});
    let before = malformed.clone();
    assert!(
        super::super::editor::apply_inverse(
            &mut malformed,
            &json!({"action":"insert_anchor","index":1,"anchor":{},"interval":{}})
        )
        .is_err()
    );
    assert_eq!(malformed, before);
}

#[test]
fn forged_remove_inverse_cannot_remove_existing_or_raw_linked_accepted_anchor() {
    for linked in [false, true] {
        let mut value = fixture(
            &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)],
            &["trusted", "trusted"],
        );
        if linked {
            value["record"]["raw"]["beat_seconds_bits"] = json!([bits(1.0)]);
            value["record"]["raw"]["timestamp_resolution_seconds_bits"] =
                json!(bits(1.0 / 48000.0));
            value["record"]["raw"]["analysis_input"] = json!({"pcm_sha256":"d".repeat(64),"sample_rate_hz":48000,"frame_count":480000,"source_zero_bits":bits(0.0),"preprocessing_identity":"fixture-analysis-v1"});
            value["record"]["accepted"]["anchors"][1]["raw_beat_index"] = json!(0);
        }
        let base = prepared(value);
        let moved = edit(
            &base,
            json!({"schema_version":1,"operation":"move_beat","beat":1.0,"sample_index":48001,"sample_rate_hz":48000}),
        );
        assert!(moved.inner.can_undo);
        let mut forged = record_of(&moved);
        forged["record"]["accepted"]["lineage"][0]["payload"]["inverse"] =
            json!({"action":"remove_anchor","index":1});
        imported_undo_refuses_without_panic_or_mutation(forged);
    }
    let base = prepared(fixture(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)],
        &["trusted", "trusted"],
    ));
    let inserted = edit(
        &base,
        json!({"schema_version":1,"operation":"move_beat","beat":0.5,"sample_index":24001,"sample_rate_hz":48000}),
    );
    assert!(inserted.inner.can_undo);
    let undone = edit(&inserted, json!({"schema_version":1,"operation":"undo"}));
    assert_eq!(
        record_of(&undone)["record"]["accepted"]["anchors"],
        record_of(&base)["record"]["accepted"]["anchors"]
    );
}

#[test]
fn invalid_rates_samples_monotonicity_gaps_and_unknown_intent_fields_refuse() {
    let base = prepared(fixture(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)],
        &["trusted", "gap_unknown_count", "trusted"],
    ));
    for intent in [
        json!({"schema_version":1,"operation":"move_beat","beat":0.5,"sample_index":24000,"sample_rate_hz":44100}),
        json!({"schema_version":1,"operation":"move_beat","beat":0.5,"sample_index":480000,"sample_rate_hz":48000}),
        json!({"schema_version":1,"operation":"move_beat","beat":0.5,"sample_index":48000,"sample_rate_hz":48000}),
        json!({"schema_version":1,"operation":"move_beat","beat":1.5,"sample_index":72000,"sample_rate_hz":48000}),
        json!({"schema_version":1,"operation":"undo"}),
        json!({"schema_version":1,"operation":"undo","surprise":true}),
        json!({"schema_version":2,"operation":"translate","sample_offset":1,"sample_rate_hz":48000}),
    ] {
        assert!(base.edited(&intent.to_string()).is_err(), "{intent}");
    }
    assert!(super::super::editor::validate_intent(&" ".repeat(8193)).is_err());
    assert!(
        super::super::editor::validate_intent(
            r#"{"schema_version":1,"schema_version":1,"operation":"undo"}"#
        )
        .is_err()
    );
    assert!(
        super::super::editor::validate_intent(r#"{"schema_version":1.0,"operation":"undo"}"#)
            .is_err()
    );
}

#[test]
fn bar_endpoint_checks_entire_supported_meter_interval_and_physical_source() {
    let map = prepared(four_four(fixture(
        &[(0.0, 0.0), (1.0, 2.0), (2.0, 6.0), (3.0, 8.0)],
        &["trusted", "trusted", "trusted"],
    )));
    assert_eq!(
        map.inner.source_after_beats(0.5, 4.0, true).source_seconds,
        Some(1.75)
    );
    let gap = prepared(four_four(fixture(
        &[(0.0, 0.0), (1.0, 2.0), (2.0, 6.0), (3.0, 8.0)],
        &["trusted", "gap_unknown_count", "trusted"],
    )));
    assert_eq!(
        gap.inner
            .source_after_beats(0.5, 6.0, true)
            .coordinate_status,
        "unsupported_crossing"
    );
    assert_eq!(
        gap.inner.source_after_beats(0.5, 1.0, true).source_seconds,
        Some(1.0)
    );
    assert_eq!(
        gap.inner.source_after_beats(1.0, -1.0, true).source_seconds,
        Some(0.5)
    );
    let meter = prepared(fixture(&[(0.0, 0.0), (3.0, 8.0)], &["trusted"]));
    assert_eq!(
        meter
            .inner
            .source_after_beats(0.5, 4.0, true)
            .coordinate_status,
        "unsupported_meter"
    );
    assert!(meter.inner.source_after_beats(0.5, 4.0, false).supported());
    let virtual_map = prepared(four_four(fixture(
        &[(-1.0, -2.0), (12.0, 24.0)],
        &["trusted"],
    )));
    assert_eq!(
        virtual_map
            .inner
            .source_after_beats(9.5, 4.0, true)
            .coordinate_status,
        "outside_physical_source"
    );
}

#[test]
fn viewport_uses_actual_inverse_finest_spaced_lines_bounded_projection_and_gap_status() {
    let map = phased_map(&[(0.0, 0.0), (1.0, 4.0), (3.0, 8.0)], &[Some(0), Some(0)]);
    let view = map.viewport(0.0, 3.0, 0.0, 100.0, 2048);
    assert_eq!(view.status, "complete");
    assert_eq!(view.step, 0.5);
    for line in &view.lines {
        assert_eq!(
            map.inner.at_beat(line.loop_beat - 1.0).source_seconds,
            Some(line.seconds)
        );
    }
    assert!(
        view.lines
            .windows(2)
            .all(|pair| (pair[1].seconds - pair[0].seconds) * 100.0 >= 12.0)
    );
    assert!(view.lines.len() + view.anchors.len() <= 2048);
    assert!(view.lines.iter().any(|line| line.reference));
    assert_eq!(
        map.viewport(0.0, 3.0, 0.0, 100.0, 2).status,
        "projection_limit"
    );
    assert_eq!(map.viewport(0.0, 3.0, 0.0, 100.0, 2049).status, "invalid");
    let gap = prepared(four_four(fixture(
        &[(0.0, 0.0), (1.0, 4.0), (2.0, 5.0), (3.0, 8.0)],
        &["trusted", "gap_unknown_count", "trusted"],
    )));
    let view = gap.viewport(0.0, 3.0, 0.0, 100.0, 2048);
    assert_eq!(view.status, "gap");
    assert!(
        view.lines
            .iter()
            .all(|line| line.seconds <= 1.0 || line.seconds >= 2.0)
    );
    let dense = phased_map(&[(0.0, 0.0), (10.0, 128.0)], &[Some(0)]);
    let fallback = dense.viewport(0.0, 10.0, 0.0, 0.01, 2048);
    assert_eq!(fallback.status, "complete");
    assert_eq!(fallback.step, 16.0);
    assert_eq!(fallback.lines.len(), 9);
}
