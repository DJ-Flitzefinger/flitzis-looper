use super::*;
use crate::audio_engine::analysis_pcm::MONO_RULE;
use crate::audio_engine::cold_store::descriptor_digest;
use crate::audio_engine::source_beatmap_capture::beat_this::{
    BeatThisSourceInput, expected_logit_count, pinned_model,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};

fn pack(values: &[f64]) -> Value {
    json!(
        STANDARD.encode(
            values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>()
        )
    )
}

fn capture(beats: &[f64], duration: f64) -> BeatThisRawData {
    let frames = (duration * 48_000.0).round() as usize;
    let descriptor = json!({"schema_version":1,"original":{"sha256":"a".repeat(64),"bytes":128},
        "pcm":{"format":"f32-le-interleaved-v1","channel_layout":"interleaved-channel-index-order-v1",
            "rate_hz":48000,"channels":2,"full_frames":frames,"full_bytes":frames*8,
            "interleaved_sha256":"b".repeat(64),"mono_sha256":"c".repeat(64),
            "mono_revision":MONO_RULE,"source_zero_bits":"0000000000000000","provenance":{"decoder":"fixture"}}});
    let source = BeatThisSourceInput {
        decoder_identity: descriptor_digest(&descriptor).unwrap(),
        source_descriptor: descriptor,
        source_sample_rate_hz: 48000,
        source_frame_count: frames,
        source_zero_frame: 0,
        mono_sha256: "d".repeat(64),
        mono_rule: MONO_RULE.into(),
    };
    let identity = json!({"pad_id":0,"request_id":3,"source_id":"numeric-fixture-not-source-custody","source_generation":7});
    let request = json!({"schema_version":1,"identity":identity,"model":pinned_model(),
        "pcm":{"path":std::env::temp_dir().join("automatic-map-original-mono.f32"),"sample_rate_hz":48000,
            "frame_count":frames,"origin_seconds":0.0,"dtype":"float32-le","channels":1}});
    let logits = vec![0.0; expected_logit_count(frames, 48000).unwrap()];
    let response = json!({"schema_version":1,"encoding":"source-map-beat-this-worker-v1","identity":identity,
        "input":{"sample_rate_hz":48000,"frame_count":frames,"source_zero_frame":0,"mono_sha256":"d".repeat(64),"dtype":"float32-le"},
        "model":pinned_model(),"status":"ready","reason":"numeric-only","resources_released":true,
        "predictions":{"encoding":"float64-le/base64","beat_seconds":pack(beats),"downbeat_seconds":pack(&[]),
            "beat_logits":pack(&logits),"downbeat_logits":pack(&logits)}});
    BeatThisRawData::from_worker(source, &request.to_string(), &response.to_string()).unwrap()
}

fn record(map: &SourceBeatmap) -> Value {
    serde_json::from_str(map.canonical_record()).unwrap()
}

#[test]
fn automatic_fractional_jitter_is_smoothed_and_raw_meter_provenance_stays_truthful() {
    let period = 60.0 / 91.887123;
    let beats: Vec<_> = (0..460)
        .map(|i| ((0.07 + i as f64 * period) / 0.02).round() * 0.02)
        .collect();
    let raw = capture(&beats, 301.0);
    let map = build(&raw, 0).unwrap();
    let value = record(&map);
    assert_eq!(value["schema_version"], 4);
    assert_eq!(
        value["record"]["raw"]["capture"],
        raw.embedded_value().unwrap()
    );
    assert_eq!(
        SourceBeatmap::parse(map.canonical_record())
            .unwrap()
            .canonical_record(),
        map.canonical_record()
    );
    let accepted = &value["record"]["accepted"];
    for anchor in accepted["anchors"].as_array().unwrap() {
        assert_eq!(anchor["review"], "estimated");
        assert!(anchor["sample_edit"].is_null());
    }
    for interval in accepted["intervals"].as_array().unwrap() {
        assert_eq!(interval["meter"]["status"], "unknown");
        assert!(interval["meter"]["downbeat_label"].is_null());
    }
    let mut worst_period_error = 0.0_f64;
    for i in 20..440 {
        let segment = map.canonical_segment_at_beat(i as f64).unwrap();
        worst_period_error =
            worst_period_error.max((segment.seconds_per_quarter() / period - 1.0).abs());
        let predicted = map.canonical_source_at_beat(i as f64).unwrap();
        assert!((predicted - beats[i]).abs() <= limit(segment.seconds_per_quarter()));
    }
    assert!(worst_period_error < 0.01); // Raw 20-ms intervals can differ by several percent.
    assert_eq!(map.inner.at_source(80.0).interval_status, "estimated");
    assert_eq!(
        map.inner
            .source_after_beats(40.0, 32.0, false)
            .interval_status,
        "estimated"
    );
    assert_eq!(
        map.inner
            .source_after_beats(40.0, 32.0, true)
            .coordinate_status,
        "unsupported_meter"
    );
    let phase = map
        .canonical_phase_reference_exact(|q| q.partial_cmp(&60.0), |q| q.partial_cmp(&100.0))
        .unwrap();
    assert!(!phase.bar_qualified());
    for version in 1..=3 {
        let mut old = value.clone();
        old["schema_version"] = json!(version);
        old["encoding"] = json!(format!("source-beatmap-pwa-v{version}"));
        assert!(SourceBeatmap::parse(&old.to_string()).is_err());
    }
}

#[test]
fn automatic_local_timing_survives_genuine_drift_without_a_global_constant_fit() {
    let beats: Vec<_> = (0..420)
        .map(|i| 0.1 + 0.65 * i as f64 + 0.0001 * (i as f64).powi(2))
        .collect();
    assert!(
        crate::audio_engine::app_tempo::estimate(&beats, 292.0)
            .bpm
            .is_none()
    );
    let raw = capture(&beats, 292.0);
    let map = build(&raw, 0).unwrap();
    let early = map
        .canonical_segment_at_beat(40.0)
        .unwrap()
        .seconds_per_quarter();
    let late = map
        .canonical_segment_at_beat(360.0)
        .unwrap()
        .seconds_per_quarter();
    assert!(late > early + 0.05);
    assert!(map.canonical_loop_interval(30.0, 256.0, false).is_some());
}

#[test]
fn automatic_missing_extra_and_bad_between_knot_events_remain_gaps() {
    for bad in ["missing", "extra", "between", "boundary"] {
        let mut beats: Vec<_> = (0..120).map(|i| 0.1 + i as f64 * 0.5).collect();
        match bad {
            "missing" => {
                beats.remove(61);
            }
            "extra" => beats.insert(61, beats[60] + 0.05),
            "between" => beats[61] += 0.08,
            "boundary" => beats[64] += 0.08,
            _ => unreachable!(),
        }
        let map = build(&capture(&beats, 61.0), 0).unwrap();
        assert!(
            map.canonical_loop_interval(5.0, 12.0, false).is_some(),
            "{bad}"
        );
        assert!(
            map.canonical_loop_interval(27.0, 16.0, false).is_none(),
            "{bad}"
        );
        assert!(
            map.canonical_loop_interval(40.0, 12.0, false).is_some(),
            "{bad}"
        );
    }
}

#[test]
fn automatic_edges_are_explicit_bounded_estimates_without_duplicate_boundaries() {
    for (origin, tail, expected) in [
        (0.0, 0.5, true),
        (0.5, 0.5, true),
        (0.5001, 0.5, false),
        (0.5, 0.5001, false),
    ] {
        let beats: Vec<_> = (0..40).map(|i| origin + i as f64 * 0.5).collect();
        let duration = beats[39] + tail;
        let map = build(&capture(&beats, duration), 0).unwrap();
        assert_eq!(
            map.inner.at_source(0.0).supported()
                && map.inner.at_source(map.inner.source_duration).supported(),
            expected
        );
        assert_eq!(map.inner.anchors.first().unwrap().seconds, 0.0);
        assert_eq!(
            map.inner.anchors.last().unwrap().seconds,
            map.inner.source_duration
        );
        assert!(
            map.inner
                .anchors
                .windows(2)
                .all(|p| p[0].seconds < p[1].seconds)
        );
    }
}

#[test]
fn automatic_mixed_query_provenance_and_gap_anchor_precedence_are_preserved() {
    let beats: Vec<_> = (0..40).map(|i| 0.1 + i as f64 * 0.5).collect();
    let map = build(&capture(&beats, 20.0), 0).unwrap();
    let mut value = record(&map);
    let intervals = value["record"]["accepted"]["intervals"]
        .as_array_mut()
        .unwrap();
    for i in intervals.iter_mut() {
        i["coverage"] = json!("trusted");
    }
    intervals[3]["coverage"] = json!("estimated");
    let mixed = editor::seal(value.clone()).unwrap();
    assert_eq!(
        mixed
            .inner
            .source_after_beats(1.0, 24.0, false)
            .interval_status,
        "estimated"
    );
    value["record"]["accepted"]["intervals"][4]["coverage"] = json!("gap_unknown_count");
    let gapped = editor::seal(value).unwrap();
    let boundary = gapped.inner.anchors[4].seconds;
    assert_eq!(
        gapped.inner.at_source(boundary).interval_status,
        "gap_unknown_count"
    );
    assert!(!gapped.inner.at_source(boundary).supported());
    let unsupported = gapped.inner.source_after_beats(1.0, 30.0, false);
    assert_eq!(unsupported.coordinate_status, "unsupported_crossing");
    assert!(
        gapped
            .edited(
                &json!({"schema_version":1,"operation":"set_coverage",
        "first_anchor_index":0,"last_anchor_index":1,"coverage":"estimated",
        "reviewed_quarter_notes":0.2})
                .to_string()
            )
            .is_err()
    );
}

#[test]
fn automatic_budget_refuses_oversize_before_geometry_or_capture_embedding() {
    let raw = capture(
        &(0..40).map(|i| 0.1 + i as f64 * 0.5).collect::<Vec<_>>(),
        20.0,
    );
    assert!(build(&raw, usize::MAX).is_err());
    assert!(admission(usize::MAX, 0, 100, &json!({})).is_err());
    assert!(admission(1024, usize::MAX, 100, &json!({})).is_err());
    assert!(admission(1024, 0, MAX_ANCHORS + 1, &json!({})).is_err());
    assert!(admission(1024, 0, 8, &json!({})).is_err());
    assert!(admission(1024 * 1024, 0, 600, &json!({})).is_ok());
    let per_anchor = size_of::<super::super::record::Anchor>()
        + size_of::<super::super::record::Segment>()
        + size_of::<super::super::review_index::AcceptedMetadata>()
        + 8 * size_of::<u32>();
    assert!(per_anchor <= 128); // Existing index_bound accounts for the new prefix too.
}

#[test]
fn automatic_unknown_meter_prepares_existing_native_beat_phase() {
    use crate::audio_engine::mapped_source_clock::MappedLoopGeometry;
    use crate::audio_engine::source_reader::FrameRange;
    let beats: Vec<_> = (0..40).map(|i| 0.125 + i as f64 * 0.5).collect();
    let map = build(&capture(&beats, 20.0), 0).unwrap();
    let interval = map.canonical_loop_interval(1.0, 16.0, false).unwrap();
    let end = (interval.source_end * 48_000.0).round() as usize;
    let region = FrameRange { start: 48_000, end };
    assert!(MappedLoopGeometry::new(map.clone(), 960_000, 48_000, region, 16.0, true).is_err());
    let mut geometry = MappedLoopGeometry::new(map, 960_000, 48_000, region, 16.0, false)
        .unwrap()
        .qualify_exact_continuation()
        .unwrap();
    geometry.prepare_common_phase();
    assert!(geometry.has_common_phase());
}
