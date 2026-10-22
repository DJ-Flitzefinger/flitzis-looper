// Inspection and explicit positioning do not qualify uncertain musical geometry.

use crate::audio_engine::analysis_pcm::MONO_RULE;
use crate::audio_engine::source_beatmap::interpretation::{self, Interpretation};
use crate::audio_engine::source_beatmap_capture::{RawCapture, beat_this};
use crate::audio_engine::source_beatmap_raw_data::{RawConfiguration, RawData};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use flitzis_looper_analysis::AnalysisConfig;
use sha2::{Digest, Sha256};

fn record(map: &SourceBeatmap) -> Value {
    serde_json::from_str(map.canonical_record()).unwrap()
}

fn move_anchor(map: &SourceBeatmap, beat: f64, sample: u64) -> Result<SourceBeatmap, String> {
    map.edited(
        &json!({"schema_version":1,"operation":"move_beat","beat":beat,
            "sample_index":sample,"sample_rate_hz":48000})
        .to_string(),
    )
}

#[test]
fn unsupported_reference_retains_existing_anchors_without_lines_or_loop_numbering() {
    for coverage in ["uncertain", "gap_unknown_count", "trusted"] {
        let map = prepared(fixture(
            &[(0.0, -2.0), (1.0, 0.0), (2.0, 3.0)],
            &[coverage, coverage],
        ));
        let references: &[f64] = if coverage == "trusted" {
            &[-1.0, 3.0]
        } else {
            &[0.0, 0.5, 1.0, 1.5, 2.0]
        };
        for reference in references {
            let view = map.viewport(0.0, 2.0, *reference, 100.0, 3);
            assert_eq!(view.status, "unsupported_reference");
            assert_eq!(view.step, 0.0);
            assert!(view.lines.is_empty());
            assert_eq!(view.anchors, vec![(0.0, -2.0), (1.0, 0.0), (2.0, 3.0)]);
            assert!(!map.inner.at_source(*reference).supported());
        }
    }
}

#[test]
fn unsupported_inspection_physically_clamps_and_refuses_overfull_ranges_without_truncation() {
    let map = prepared(fixture(
        &[
            (-1.0, -2.0),
            (0.0, 0.0),
            (1.0, 2.0),
            (10.0, 20.0),
            (11.0, 22.0),
        ],
        &["uncertain"; 4],
    ));
    let view = map.viewport(-2.0, 12.0, 0.5, 100.0, 3);
    assert_eq!(view.status, "unsupported_reference");
    assert_eq!(view.anchors, vec![(0.0, 0.0), (1.0, 2.0), (10.0, 20.0)]);
    assert!(view.lines.is_empty());
    assert_eq!(map.inner.sample_index(0.0), Some(0));
    assert_eq!(map.inner.sample_index(10.0), None);
    let overfull = map.viewport(-2.0, 12.0, 0.5, 100.0, 2);
    assert_eq!(overfull.status, "projection_limit");
    assert!(overfull.anchors.is_empty());
    assert!(overfull.lines.is_empty());
    assert_eq!(map.viewport(12.0, 13.0, 0.5, 100.0, 3).status, "outside");

    let anchors: Vec<(f64, f64)> = (0..2049)
        .map(|index| (1.0 + f64::from(index) / 1024.0, f64::from(index)))
        .collect();
    let dense = prepared(fixture(&anchors, &vec!["uncertain"; anchors.len() - 1]));
    let full = dense.viewport(1.0, anchors[2047].0, 1.0, 100.0, 2048);
    assert_eq!(full.status, "unsupported_reference");
    assert_eq!(full.anchors.len(), 2048);
    assert!(full.lines.is_empty());
    let overfull = dense.viewport(1.0, 3.0, 1.0, 100.0, 2048);
    assert_eq!(overfull.status, "projection_limit");
    assert!(overfull.anchors.is_empty());
    assert!(overfull.lines.is_empty());
}

#[test]
fn unsupported_inspection_keeps_invalid_argument_refusals_empty() {
    let map = prepared(fixture(&[(0.0, 0.0), (2.0, 4.0)], &["uncertain"]));
    for reference in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let view = map.viewport(0.0, 2.0, reference, 100.0, 2048);
        assert_eq!(view.status, "invalid");
        assert!(view.anchors.is_empty());
        assert!(view.lines.is_empty());
    }
    for (start, end, pixels, limit) in [
        (f64::NAN, 2.0, 100.0, 2048),
        (0.0, f64::INFINITY, 100.0, 2048),
        (2.0, 1.0, 100.0, 2048),
        (0.0, 2.0, f64::NAN, 2048),
        (0.0, 2.0, 0.0, 2048),
        (0.0, 2.0, 100.0, 0),
        (0.0, 2.0, 100.0, 2049),
    ] {
        let view = map.viewport(start, end, 0.5, pixels, limit);
        assert_eq!(view.status, "invalid");
        assert!(view.anchors.is_empty());
        assert!(view.lines.is_empty());
    }
}

fn assert_position_only_and_closed_undo(base: &SourceBeatmap) {
    let before = record(base);
    let moved = move_anchor(base, 1.0, 48001).unwrap();
    let after = record(&moved);
    assert_eq!(after["schema_version"], before["schema_version"]);
    assert_eq!(after["encoding"], before["encoding"]);
    assert_eq!(after["record"]["source"], before["record"]["source"]);
    assert_eq!(after["record"]["raw"], before["record"]["raw"]);
    assert_eq!(
        after["record"]["accepted"]["intervals"],
        before["record"]["accepted"]["intervals"]
    );
    let mut expected = before["record"]["accepted"]["anchors"].clone();
    expected[1]["source_seconds_bits"] = json!(bits(48001.0 / 48000.0));
    expected[1]["sample_edit"] = json!({"sample_index":48001,"sample_rate_hz":48000,
        "evidence":"Explicit source-sample positioning","author":"performer"});
    assert_eq!(after["record"]["accepted"]["anchors"], expected);
    assert_eq!(
        after["record"]["accepted"]["anchors"][1]["review"],
        "estimated"
    );
    assert_eq!(
        after["record"]["accepted"]["parent_revision"],
        base.map_revision()
    );
    assert!(moved.inner.can_undo);
    assert!(!moved.inner.at_beat(1.0).supported());
    assert!(!moved.inner.at_source(0.5).supported());
    assert!(!moved.inner.source_after_beats(0.5, 1.0, false).supported());
    let reparsed = SourceBeatmap::parse(moved.canonical_record()).unwrap();
    assert!(reparsed.inner.can_undo);
    let undone = reparsed
        .edited(&json!({"schema_version":1,"operation":"undo"}).to_string())
        .unwrap();
    let undone = record(&undone);
    assert_eq!(undone["record"]["raw"], before["record"]["raw"]);
    assert_eq!(
        undone["record"]["accepted"]["anchors"],
        before["record"]["accepted"]["anchors"]
    );
    assert_eq!(
        undone["record"]["accepted"]["intervals"],
        before["record"]["accepted"]["intervals"]
    );
}

#[test]
fn existing_uncertain_or_gap_anchor_accepts_sample_position_without_changing_certainty() {
    for coverage in ["uncertain", "gap_unknown_count"] {
        let mut value = fixture(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)], &[coverage, coverage]);
        value["record"]["accepted"]["anchors"][1]["review"] = json!("estimated");
        assert_position_only_and_closed_undo(&prepared(value));
    }
}

#[test]
fn unsupported_nonanchors_and_invalid_existing_sample_positions_still_refuse() {
    for coverage in ["uncertain", "gap_unknown_count"] {
        let base = prepared(fixture(
            &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)],
            &[coverage, coverage],
        ));
        let before = base.canonical_record().to_owned();
        for beat in [-1.0, 0.5, 1.0_f64.next_up(), 1.5, 3.0] {
            assert_eq!(
                move_anchor(&base, beat, 48001).unwrap_err(),
                "beatmap_edit_unsupported_beat"
            );
        }
        for sample in [0, 96000, 480000, u64::MAX] {
            assert!(move_anchor(&base, 1.0, sample).is_err());
        }
        for (beat, rate) in [(json!(1.0), 44100), (Value::Null, 48000)] {
            assert!(
                base.edited(
                    &json!({"schema_version":1,"operation":"move_beat",
                "beat":beat,"sample_index":48001,"sample_rate_hz":rate})
                    .to_string()
                )
                .is_err()
            );
        }
        assert_eq!(base.canonical_record(), before);
    }
}

fn pack(values: &[f64]) -> Value {
    let bytes: Vec<u8> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    json!(STANDARD.encode(bytes))
}

fn captured_maps() -> Vec<SourceBeatmap> {
    let descriptor = fixture(&[(0.0, 0.0), (2.0, 2.0)], &["uncertain"])
        ["record"]["source"]["decoder_descriptor"].clone();
    let decoder_identity = descriptor_digest(&descriptor).unwrap();
    let defaults = AnalysisConfig::default();
    let hop = (44100.0 * defaults.step_secs) as usize;
    let mut qm = RawData {
        decoder_identity: decoder_identity.clone(),
        source_descriptor: descriptor.clone(),
        source_sample_rate_hz: 48000,
        source_frame_count: 480000,
        source_zero_frame: 0,
        mono_sha256: "d".repeat(64),
        mono_rule: MONO_RULE.into(),
        analyzer_sha256: "e".repeat(64),
        analyzer_sample_rate_hz: 44100,
        analyzer_frame_count: 441000,
        conversion_revision: "rubato-fft-1.0-44100-delay-trim-tail-flush-v1+exact-f32-to-f64-v1"
            .into(),
        detector_id: "legacy-qm-dsp".into(),
        code_revision: "native-qm-complete-raw-integer-hop-v1".into(),
        odf_hop_samples: hop,
        configuration: RawConfiguration {
            step_secs: defaults.step_secs,
            max_bin_hz: defaults.max_bin_hz,
            input_tempo: defaults.input_tempo,
            alpha: defaults.alpha,
            tightness: defaults.tightness,
            viterbi_sigma: defaults.viterbi_sigma,
            window_length: defaults.window_length,
            hop_size: defaults.hop_size,
        },
        raw_revision: String::new(),
        beat_frames: (0..4)
            .map(|index| f64::from(index) * 44100.0 / hop as f64)
            .collect(),
        downbeat_indices: vec![0, 3],
    };
    let mut digest = Sha256::new();
    digest.update(b"source-beatmap-unreviewed-qm-raw-v1\0");
    digest.update(serde_json::to_vec(&qm).unwrap());
    qm.raw_revision = format!("{:x}", digest.finalize());

    let source = beat_this::BeatThisSourceInput {
        decoder_identity,
        source_descriptor: descriptor,
        source_sample_rate_hz: 48000,
        source_frame_count: 480000,
        source_zero_frame: 0,
        mono_sha256: "d".repeat(64),
        mono_rule: MONO_RULE.into(),
    };
    let identity =
        json!({"pad_id":0,"request_id":3,"source_id":"original-fixture","source_generation":7});
    let request = json!({"schema_version":1,"identity":identity,"model":beat_this::pinned_model(),
        "pcm":{"path":std::env::temp_dir().join("uncertain-editor-mono.f32"),"sample_rate_hz":48000,
            "frame_count":480000,"origin_seconds":0.0,"dtype":"float32-le","channels":1}});
    let logits = vec![0.0; beat_this::expected_logit_count(480000, 48000).unwrap()];
    let result = json!({"schema_version":1,"encoding":beat_this::WORKER_ENCODING,"identity":identity,
        "input":{"sample_rate_hz":48000,"frame_count":480000,"source_zero_frame":0,
            "mono_sha256":"d".repeat(64),"dtype":"float32-le"},
        "model":beat_this::pinned_model(),"status":"ready","reason":"fixture","resources_released":true,
        "predictions":{"encoding":"float64-le/base64","beat_seconds":pack(&[-0.0,1.0,2.0,3.0]),
            "downbeat_seconds":pack(&[0.0,3.0]),"beat_logits":pack(&logits),"downbeat_logits":pack(&logits)}});
    let beat_this =
        beat_this::BeatThisRawData::from_worker(source, &request.to_string(), &result.to_string())
            .unwrap();
    [RawCapture::from(qm), RawCapture::from(beat_this)]
        .iter()
        .map(|capture| {
            let intent = Interpretation::parse(
                &json!({"schema_version":1,"raw_revision":capture.revision(),
            "anchors":[{"raw_index":0,"quarter_bits":bits(0.0)},
                {"raw_index":1,"quarter_bits":bits(1.0)},{"raw_index":2,"quarter_bits":bits(2.0)}]})
                .to_string(),
            )
            .unwrap();
            interpretation::build(capture, &intent, 4096).unwrap()
        })
        .collect()
}

#[test]
fn v2_and_v3_uncertain_positions_keep_complete_capture_and_exact_closed_undo() {
    let maps = captured_maps();
    assert_eq!(record(&maps[0])["schema_version"], 2);
    assert_eq!(record(&maps[1])["schema_version"], 3);
    for map in maps {
        assert_position_only_and_closed_undo(&map);
        let value = record(&map);
        let anchors = value["record"]["accepted"]["anchors"].as_array().unwrap();
        let view = map.viewport(0.0, 3.0, 0.5, 100.0, 2048);
        assert_eq!(view.status, "unsupported_reference");
        assert_eq!(view.anchors.len(), anchors.len());
        assert!(view.lines.is_empty());
    }
}
