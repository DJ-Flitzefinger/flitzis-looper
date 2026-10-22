//! Independent coordinate expectations; no source/audio authority is constructed.

mod editor_tests {
    use super::*;
    include!("editor_tests.rs");
}

mod unsupported_editor_tests {
    use super::*;
    include!("unsupported_editor_tests.rs");
}

use super::super::cold_store::descriptor_digest;
use super::*;
use serde_json::{Value, json};

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn fixture(anchors: &[(f64, f64)], coverage: &[&str]) -> Value {
    let decoder = json!({
        "schema_version": 1,
        "original": {"sha256": "a".repeat(64), "bytes": 1000},
        "pcm": {
            "format": "f32-le-interleaved-v1",
            "channel_layout": "interleaved-channel-index-order-v1",
            "rate_hz": 48000, "channels": 2, "full_frames": 480000,
            "full_bytes": 3840000, "interleaved_sha256": "b".repeat(64),
            "mono_sha256": "c".repeat(64), "mono_revision": "test-mono-v1",
            "source_zero_bits": bits(0.0), "provenance": {"test_only": true},
        },
    });
    let anchors: Vec<Value> = anchors
        .iter()
        .enumerate()
        .map(|(index, (seconds, beat))| {
            json!({
                "id": format!("anchor-{index}"), "source_seconds_bits": bits(*seconds),
                "beat_bits": bits(*beat), "raw_beat_index": null, "review": "reviewed",
                "sample_edit": null,
            })
        })
        .collect();
    let intervals: Vec<Value> = coverage
        .iter()
        .map(|coverage| {
            json!({
                "coverage": coverage,
                "meter": {"status": "unknown", "downbeat_label": null, "evidence": "synthetic"},
            })
        })
        .collect();
    json!({
        "schema_version": 1, "encoding": "source-beatmap-pwa-v1",
        "record": {
            "source": {"decoder_identity": descriptor_digest(&decoder).unwrap(),
                "decoder_descriptor": decoder},
            "raw": {
                "raw_revision": "0".repeat(64),
                "detector": {"detector_id": "manual", "code_revision": "fixture-v1",
                    "configuration": {}, "model": null, "preprocessing": {}},
                "analysis_input": null, "timestamp_resolution_seconds_bits": null,
                "beat_seconds_bits": [], "downbeat_seconds_bits": [],
                "uncertainty": "synthetic geometry does not prove musical or source acceptance",
            },
            "accepted": {
                "map_revision": "0".repeat(64), "parent_revision": null,
                "beat_unit": "quarter_note",
                "interpolation_policy": "pwa-source-seconds-quarter-v1",
                "boundary_policy": "right-interior-left-final-gap-unavailable-v1",
                "snap_policy": "nearest-quarter-subdivision-later-tie-v1",
                "anchors": anchors, "intervals": intervals, "lineage": [],
            },
        },
    })
}

fn seal(value: &mut Value) {
    let source_identity =
        descriptor_digest(&value["record"]["source"]["decoder_descriptor"]).unwrap();
    value["record"]["source"]["decoder_identity"] = json!(source_identity);
    value["record"]["raw"]
        .as_object_mut()
        .unwrap()
        .remove("raw_revision");
    let raw_revision = descriptor_digest(&value["record"]["raw"]).unwrap();
    value["record"]["raw"]["raw_revision"] = json!(raw_revision);
    value["record"]["accepted"]
        .as_object_mut()
        .unwrap()
        .remove("map_revision");
    let revision = descriptor_digest(&value["record"]).unwrap();
    value["record"]["accepted"]["map_revision"] = json!(revision);
}

fn prepared(mut value: Value) -> SourceBeatmap {
    seal(&mut value);
    SourceBeatmap::parse(&value.to_string()).unwrap()
}

fn failure(mut value: Value) -> String {
    seal(&mut value);
    SourceBeatmap::parse(&value.to_string()).expect_err("must reject")
}

#[test]
fn unequal_intervals_share_inverse_and_right_sided_local_tempo() {
    let map = prepared(fixture(
        &[(1.0, 2.0), (1.6, 3.0), (2.1, 4.0)],
        &["trusted", "trusted"],
    ));
    let forward = map.inner.at_source(1.3);
    assert!((forward.beat.unwrap() - 2.5).abs() < 1e-14);
    let inverse = map.inner.at_beat(2.5);
    assert!((inverse.source_seconds.unwrap() - 1.3).abs() < 1e-14);
    assert!((forward.local_bpm.unwrap() - 100.0).abs() < 1e-12);
    assert!((map.inner.at_source(1.6).local_bpm.unwrap() - 120.0).abs() < 1e-12);
    assert!((map.inner.at_source(2.1).local_bpm.unwrap() - 120.0).abs() < 1e-12);
}

#[test]
fn exact_anchor_queries_and_durable_roundtrip_preserve_stored_bits() {
    let source = 1.0000000000000002;
    let beat = -0.0;
    let map = prepared(fixture(&[(source, beat), (2.0, 1.0)], &["trusted"]));
    assert_eq!(
        map.inner.at_source(source).beat.unwrap().to_bits(),
        beat.to_bits()
    );
    assert_eq!(
        map.inner.at_beat(beat).source_seconds.unwrap().to_bits(),
        source.to_bits()
    );
    let restored = SourceBeatmap::parse(map.record_json()).unwrap();
    assert_eq!(restored.record_json(), map.record_json());
    assert_eq!(restored.revision(), map.revision());
    assert_eq!(restored.source_identity(), map.source_identity());
}

#[test]
fn gap_and_uncertain_edges_have_coordinates_without_a_local_tempo_bridge() {
    for coverage in ["gap_unknown_count", "uncertain"] {
        let map = prepared(fixture(
            &[(0.0, 0.0), (1.0, 1.0), (2.0, 3.0), (3.0, 4.0)],
            &["trusted", coverage, "trusted"],
        ));
        for boundary in [1.0, 2.0] {
            let result = map.inner.at_source(boundary);
            assert_eq!(result.coordinate_status, "anchor");
            assert_eq!(result.interval_status, coverage);
            assert!(result.beat.is_some());
            assert!(result.local_bpm.is_none());
        }
        let interior = map.inner.at_source(1.5);
        assert!(interior.beat.is_none());
        assert!(interior.local_bpm.is_none());
        assert!(map.inner.at_beat(2.0).source_seconds.is_none());
    }
}

#[test]
fn signed_extrapolation_is_explicitly_unverified_and_not_a_readable_index() {
    let map = prepared(fixture(&[(0.0, 0.0), (1.0, 2.0)], &["trusted"]));
    assert_eq!(map.inner.at_source(-1.0).coordinate_status, "outside");
    let result = map.inner.extrapolate_source(-1.0);
    assert_eq!(result.beat, Some(-2.0));
    assert_eq!(result.coordinate_status, "unverified_extrapolation");
    assert_eq!(result.physical_domain, "before_source");
    assert_eq!(map.inner.extrapolate_beat(-2.0).source_seconds, Some(-1.0));
    let map = prepared(fixture(&[(0.0, 0.0), (1.0, 2.0)], &["uncertain"]));
    assert!(map.inner.extrapolate_source(-1.0).beat.is_none());
}

#[test]
fn variable_halfway_selects_later_without_an_epsilon_or_scalar_changes() {
    let map = prepared(fixture(&[(-2.0, -2.0), (2.0, 2.0)], &["trusted"]));
    assert_eq!(map.inner.nearest_grid(-0.5, 1.0).beat, Some(0.0));
    assert_eq!(map.inner.nearest_grid(0.5, 1.0).beat, Some(1.0));
    assert_eq!(
        map.inner.nearest_grid(0.5_f64.next_down(), 1.0).beat,
        Some(0.0)
    );
    assert_eq!(
        map.inner.nearest_grid(0.5_f64.next_up(), 1.0).beat,
        Some(1.0)
    );
}

#[test]
fn exact_grid_endpoints_snap_to_the_stored_anchor_without_outside_queries() {
    let map = prepared(fixture(&[(0.0, 0.0), (1.0, 2.0)], &["trusted"]));
    for (source, beat) in [(0.0_f64, 0.0_f64), (1.0_f64, 2.0_f64)] {
        let snapped = map.inner.nearest_grid(source, 1.0);
        assert_eq!(
            snapped.source_seconds.map(f64::to_bits),
            Some(source.to_bits())
        );
        assert_eq!(snapped.beat.map(f64::to_bits), Some(beat.to_bits()));
        assert_eq!(snapped.coordinate_status, "anchor");
        assert_eq!(snapped.interval_status, "trusted");
    }
}

#[test]
fn partial_final_subdivision_snaps_to_the_supported_nearest_grid_target() {
    let map = prepared(fixture(&[(0.0, 0.0), (1.2, 1.2)], &["trusted"]));
    let snapped = map.inner.nearest_grid(1.1, 1.0);
    assert_eq!(snapped.source_seconds, Some(1.0));
    assert_eq!(snapped.beat, Some(1.0));
    assert_eq!(snapped.coordinate_status, "supported");
    assert_eq!(snapped.interval_status, "trusted");
}

#[test]
fn exact_grid_anchor_does_not_require_a_representable_later_neighbor() {
    let mut value = fixture(&[(0.0, 0.0), (f64::MAX, f64::MAX)], &["trusted"]);
    assert_eq!(
        prepared(value.clone())
            .inner
            .nearest_grid(f64::MAX, f64::MAX)
            .coordinate_status,
        "unsupported_downbeat"
    );
    // This is a bar-sized lattice: qualify its meter and phase while keeping
    // the exact maximum endpoint and all no-later-neighbor assertions intact.
    value["record"]["accepted"]["intervals"][0]["meter"]["status"] =
        json!("quarter_note_4_4_verified");
    value["record"]["accepted"]["intervals"][0]["meter"]["downbeat_label"] = json!(0);
    let map = prepared(value);
    let snapped = map.inner.nearest_grid(f64::MAX, f64::MAX);
    assert_eq!(snapped.source_seconds, Some(f64::MAX));
    assert_eq!(snapped.beat, Some(f64::MAX));
    assert_eq!(snapped.coordinate_status, "anchor");
    assert_eq!(snapped.interval_status, "trusted");
    assert_eq!(snapped.physical_domain, "after_source");
}

#[test]
fn same_island_target_survives_a_hypothetical_midpoint_across_unsupported_coverage() {
    for coverage in ["gap_unknown_count", "uncertain"] {
        let map = prepared(fixture(
            &[(0.0, 0.0), (1.2, 1.2), (1.4, 1.4), (3.0, 3.0)],
            &["trusted", coverage, "trusted"],
        ));
        // The nearest target is source/beat 1 in the current island. The
        // hypothetical beat-1.5 midpoint is in a different supported island.
        let snapped = map.inner.nearest_grid(1.1, 1.0);
        assert_eq!(snapped.source_seconds, Some(1.0));
        assert_eq!(snapped.beat, Some(1.0));
        assert_eq!(snapped.coordinate_status, "supported");
        assert_eq!(snapped.interval_status, "trusted");
    }
}

#[test]
fn nearest_grid_refuses_to_cross_a_gap_even_when_both_endpoints_are_supported() {
    for coverage in ["gap_unknown_count", "uncertain"] {
        let forward = prepared(fixture(
            &[(0.0, 0.0), (0.8, 0.8), (0.9, 0.9), (2.0, 2.0)],
            &["trusted", coverage, "trusted"],
        ));
        let reverse = prepared(fixture(
            &[(0.0, 0.0), (0.1, 0.1), (0.2, 0.2), (2.0, 2.0)],
            &["trusted", coverage, "trusted"],
        ));
        for (map, source, target) in [(&forward, 0.79, 1.0), (&reverse, 0.49, 0.0)] {
            assert_eq!(map.inner.at_source(source).interval_status, "trusted");
            assert_eq!(map.inner.at_beat(target).interval_status, "trusted");
            let snapped = map.inner.nearest_grid(source, 1.0);
            assert_eq!(snapped.coordinate_status, "unsupported_crossing");
            assert!(snapped.source_seconds.is_none());
            assert!(snapped.beat.is_none());
            assert!(snapped.local_bpm.is_none());
        }
    }
}

#[test]
fn projection_has_an_inclusive_limit_and_never_labels_partial_success() {
    let map = prepared(fixture(
        &[(-10000.0, -10000.0), (10000.0, 10000.0)],
        &["trusted"],
    ));
    let exact = map.inner.project(0.0, 2047.0, 1.0, MAX_PROJECTION_POINTS);
    assert_eq!(exact.status, "complete");
    assert_eq!(exact.points.len(), MAX_PROJECTION_POINTS);
    let excess = map.inner.project(0.0, 2048.0, 1.0, MAX_PROJECTION_POINTS);
    assert_eq!(excess.status, "projection_limit");
    assert!(excess.points.is_empty());
    assert_eq!(map.inner.project(0.0, 1.0, 1.0, 0).status, "invalid");
    assert_eq!(map.inner.project(0.0, 1.0, 1.0, 2049).status, "invalid");
}

#[test]
fn invalid_and_overflowing_queries_are_explicitly_unavailable() {
    let map = prepared(fixture(&[(0.0, 0.0), (1.0, 2.0)], &["trusted"]));
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(map.inner.at_source(invalid).coordinate_status, "invalid");
        assert_eq!(map.inner.at_beat(invalid).coordinate_status, "invalid");
    }
    assert_eq!(
        map.inner.extrapolate_source(f64::MAX).coordinate_status,
        "numerical_unavailable"
    );
    assert_eq!(
        map.inner.project(1e300, 1e301, 1e-300, 2048).status,
        "numerical_unavailable"
    );
}

#[test]
fn finite_axes_do_not_hide_nonfinite_slopes_bpm_or_insufficient_spacing() {
    for anchors in [
        [(0.0, 0.0), (0.0, 1.0)],
        [(1.0, 0.0), (1.0_f64.next_up(), 1.0)],
        [(0.0, 0.0), (1e-300, 1e8)], // finite forward slope, overflowing BPM
        [(0.0, 0.0), (1e308, 1e-300)], // inverse overflow/forward underflow
        [(-f64::MAX, 0.0), (f64::MAX, 1.0)], // finite endpoints, overflowing difference
        [(0.0, 0.0), (f64::INFINITY, 1.0)],
    ] {
        assert!(!failure(fixture(&anchors, &["trusted"])).is_empty());
    }
    // Compute BPM from the finite forward slope, not multiply-first differences.
    let map = prepared(fixture(&[(0.0, 0.0), (1e307, 1e307)], &["trusted"]));
    assert_eq!(map.inner.at_source(1e306).local_bpm, Some(60.0));
}

#[test]
fn raw_positions_need_complete_provenance_and_physical_extent() {
    let mut value = fixture(&[(0.0, 0.0), (1.0, 1.0)], &["trusted"]);
    value["record"]["raw"]["beat_seconds_bits"] = json!([bits(0.5)]);
    assert!(failure(value.clone()).contains("raw_event_provenance_missing"));
    value["record"]["raw"]["analysis_input"] = json!({
        "pcm_sha256": "d".repeat(64), "sample_rate_hz": 48000,
        "frame_count": 480000, "source_zero_bits": bits(0.0),
        "preprocessing_identity": "test-only",
    });
    value["record"]["raw"]["timestamp_resolution_seconds_bits"] = json!(bits(0.02));
    let map = prepared(value.clone());
    assert_eq!(map.inner.at_source(0.5).beat, Some(0.5));
    value["record"]["raw"]["beat_seconds_bits"] = json!([bits(10.0)]);
    assert!(failure(value).contains("raw_source_positions_invalid"));
}

#[test]
fn full_digests_and_supported_schema_are_checked_without_granting_custody() {
    let mut value = fixture(&[(0.0, 0.0), (1.0, 1.0)], &["trusted"]);
    seal(&mut value);
    value["record"]["accepted"]["map_revision"] = json!("f".repeat(64));
    assert!(
        SourceBeatmap::parse(&value.to_string())
            .err()
            .unwrap()
            .contains("map_revision_mismatch")
    );
    value["record"]["source"]["decoder_identity"] = json!("bad");
    assert!(SourceBeatmap::parse(&value.to_string()).is_err());
    value["schema_version"] = json!(true);
    assert!(SourceBeatmap::parse(&value.to_string()).is_err());
    let map = prepared(fixture(&[(0.0, 0.0), (1.0, 1.0)], &["trusted"]));
    // These source hashes are intentionally invented fixture data; parsing never
    // exposes a loaded owner, source permit, native ACK or musical acceptance.
    assert_eq!(map.inner.at_source(0.5).coordinate_status, "supported");
}

#[test]
fn count_and_byte_limits_refuse_before_geometry_or_digest_promotion() {
    let mut value = fixture(&[(0.0, 0.0), (1.0, 1.0)], &["trusted"]);
    value["record"]["raw"]["beat_seconds_bits"] = json!(vec![bits(0.0); MAX_ANCHORS + 1]);
    assert!(
        SourceBeatmap::parse(&value.to_string())
            .err()
            .unwrap()
            .contains("beatmap_count_limit")
    );
    let oversized = " ".repeat(MAX_RECORD_BYTES + 1);
    assert!(
        SourceBeatmap::parse(&oversized)
            .err()
            .unwrap()
            .contains("beatmap_byte_limit")
    );
}

#[test]
fn clones_share_immutable_data_and_loaded_rate_does_not_change_map_identity() {
    let map = prepared(fixture(&[(1.0, 2.0), (1.6, 3.0)], &["trusted"]));
    let copy = map.clone();
    assert!(Arc::ptr_eq(&map.inner, &copy.inner));
    let source = copy.inner.at_beat(2.5).source_seconds.unwrap();
    for rate in [44100.0, 48000.0, 96000.0] {
        assert!((source * rate - 1.3 * rate).abs() <= 1.0);
        assert_eq!(copy.source_identity(), map.source_identity());
        assert_eq!(copy.revision(), map.revision());
    }
    let changed = prepared(fixture(&[(1.0, 2.0), (1.7, 3.0)], &["trusted"]));
    assert_ne!(changed.revision(), map.revision());
    assert_eq!(copy.record_json(), map.record_json());
}

#[test]
fn long_sample_coordinates_and_correction_provenance_survive_reconstruction() {
    for rate in [44100_u32, 48000, 96000] {
        let frame = u64::from(rate) * 1800 + 1;
        let source = frame as f64 / f64::from(rate);
        let mut value = fixture(&[(600.0, 0.0), (source, 2400.0)], &["trusted"]);
        value["record"]["source"]["decoder_descriptor"]["pcm"]["full_frames"] = json!(96000000_u64);
        value["record"]["source"]["decoder_descriptor"]["pcm"]["full_bytes"] = json!(768000000_u64);
        value["record"]["accepted"]["anchors"][1]["sample_edit"] = json!({
            "sample_index": frame, "sample_rate_hz": rate,
            "evidence": "explicit-source-sample-fixture", "author": "test-only",
        });
        value["record"]["accepted"]["lineage"] = json!([{
            "id": "edit-1", "parent_revision": null, "kind": "source_position",
            "payload": {"anchor_id": "anchor-1", "source_seconds_bits": bits(source)},
            "evidence": "explicit sample placement", "author": "test-only",
        }]);
        let map = prepared(value);
        let actual = map.inner.at_beat(2400.0).source_seconds.unwrap();
        assert_eq!(actual.to_bits(), source.to_bits());
        assert_eq!((actual * f64::from(rate)).round() as u64, frame);
        let roundtrip: Value = serde_json::from_str(map.record_json()).unwrap();
        assert_eq!(
            roundtrip["record"]["accepted"]["anchors"][1]["sample_edit"]["sample_index"],
            frame
        );
        assert_eq!(
            roundtrip["record"]["accepted"]["lineage"][0]["kind"],
            "source_position"
        );
    }
}

#[test]
fn independent_anchor_and_lineage_ids_do_not_alias_or_hide_bad_sample_rates() {
    let mut value = fixture(&[(0.0, 0.0), (1.0, 1.0)], &["trusted"]);
    value["record"]["accepted"]["anchors"][1]["id"] = json!("anchor-0");
    assert!(failure(value).contains("duplicate_anchor_id"));
    let mut value = fixture(&[(0.0, 0.0), (1.0, 1.0)], &["trusted"]);
    value["record"]["accepted"]["anchors"][0]["sample_edit"] = json!({
        "sample_index": 0, "sample_rate_hz": 0, "evidence": "invalid", "author": "test-only",
    });
    assert!(failure(value).contains("sample_edit_rate_invalid"));
    let mut value = fixture(&[(0.0, 0.0), (1.0, 1.0)], &["trusted"]);
    let operation = json!({
        "id": "duplicate-operation", "parent_revision": null, "kind": "beat_count",
        "payload": {"anchor_id": "anchor-1"}, "evidence": "test", "author": "test-only",
    });
    value["record"]["accepted"]["lineage"] = json!([operation.clone(), operation]);
    assert!(failure(value).contains("lineage_invalid"));
}
