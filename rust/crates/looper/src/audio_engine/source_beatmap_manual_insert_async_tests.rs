// Actual captured Edit/store/private completion/Undo/fresh-engine restore.
use super::*;

#[test]
fn actual_manual_missing_beat_job_stores_undoes_and_fresh_engine_restores_without_raw_lease() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut fixture = review_fixture();
    let raw = ready_review(&fixture);
    let capture = raw.capture.clone();
    let raw_path = raw.path.clone();
    let captured = crate::audio_engine::source_beatmap_capture::RawCapture::decode(
        &fs::read(&raw_path).unwrap(),
    )
    .unwrap();
    let frames = capture["data"]["source_frame_count"].as_u64().unwrap() as usize;
    let rate = capture["data"]["source_sample_rate_hz"].as_u64().unwrap() as u32;
    let original = fixture.engine.cold_leases.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .original_path
        .clone();
    let samples = original
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "samples"))
        .unwrap()
        .to_owned();
    let original_assignment = fixture
        .engine
        .project_assets
        .acquire(&samples, &original)
        .unwrap();
    original_assignment
        .acknowledge(original.to_string_lossy().into_owned())
        .unwrap();
    let accepted = begin_review(&fixture, &raw);
    let base = review_artifact(&accepted, &fixture);
    let before = review_artifact_record(&base);
    let endpoint = |index: usize| {
        f64::from_bits(
            u64::from_str_radix(
                before["record"]["accepted"]["anchors"][index]["source_seconds_bits"]
                    .as_str()
                    .unwrap(),
                16,
            )
            .unwrap(),
        )
    };
    let left = endpoint(0);
    let right = endpoint(1);
    let sample = (((left * f64::from(rate)).floor() as u64 + 1)
        ..(right * f64::from(rate)).ceil() as u64)
        .find(|sample| {
            let seconds = *sample as f64 / f64::from(rate);
            seconds > left
                && seconds < right
                && (0..captured.event_count()).all(|index| {
                    captured.source_seconds(index).unwrap().to_bits() != seconds.to_bits()
                })
        })
        .expect("fixture supplies a physical sample omitted by every detector event");
    let seconds = sample as f64 / f64::from(rate);
    let fences = source_fences(&fixture);
    let raw_job = raw.discard();
    wait_review_retired(&fixture, &raw_job);
    assert!(!raw_path.exists());
    let inserted = review_edit_artifact(
        &fixture,
        &base,
        json!({
        "schema_version":1,"operation":"insert_manual_beat",
        "map_revision":before["record"]["accepted"]["map_revision"],
        "capture_revision":capture["raw_revision"],"mode":"interior",
        "first_anchor_index":0,"last_anchor_index":1,
        "sample_index":sample,"sample_rate_hz":rate,"quarter_bits":bits(1.25)}),
    );
    let expected = review_artifact_record(&inserted);
    let anchors = expected["record"]["accepted"]["anchors"]
        .as_array()
        .unwrap();
    assert_eq!(anchors.len(), 3);
    assert_eq!(anchors[0], before["record"]["accepted"]["anchors"][0]);
    assert_eq!(anchors[2], before["record"]["accepted"]["anchors"][1]);
    assert_eq!(anchors[1]["source_seconds_bits"], bits(seconds));
    assert_eq!(anchors[1]["beat_bits"], bits(1.25));
    assert!(anchors[1]["raw_beat_index"].is_null());
    assert_eq!(anchors[1]["review"], "reviewed");
    assert_eq!(anchors[1]["sample_edit"]["sample_index"], sample);
    assert_eq!(anchors[1]["sample_edit"]["sample_rate_hz"], rate);
    assert_eq!(expected["record"]["raw"], before["record"]["raw"]);
    for interval in expected["record"]["accepted"]["intervals"]
        .as_array()
        .unwrap()
    {
        assert_eq!(interval["coverage"], "uncertain");
        assert_eq!(interval["meter"]["status"], "unknown");
        assert!(interval["meter"]["downbeat_label"].is_null());
    }
    assert!(review_public_gates(&inserted)["point"]["local_bpm"].is_null());
    let undone = review_edit_artifact(
        &fixture,
        &inserted,
        json!({"schema_version":1,"operation":"undo"}),
    );
    let undone_record = review_artifact_record(&undone);
    for key in ["anchors", "intervals"] {
        assert_eq!(
            undone_record["record"]["accepted"][key],
            before["record"]["accepted"][key]
        );
    }
    assert_eq!(source_fences(&fixture), fences);
    let receipt = inserted.receipt_json().to_owned();
    let receipt_value: Value = serde_json::from_str(&receipt).unwrap();
    let accepted_path = crate::audio_engine::material_paths::resolve(
        &samples,
        Path::new(receipt_value["reference"].as_str().unwrap()),
    )
    .unwrap()
    .path;
    drop((base, inserted, undone, accepted, raw_job, captured));
    Python::attach(|_| {});
    wait_until(|| {
        fixture.engine.project_assets.collect_for_test();
        fixture
            .engine
            .project_assets
            .held_owner_count(&accepted_path)
            == 0
            && !fixture
                .engine
                .project_assets
                .map_retirement_pending_for_test(&accepted_path)
    });
    fixture.engine.shut_down().unwrap();
    let ColdCustody {
        engine,
        directory,
        producer,
        consumer,
        mixer,
    } = fixture;
    drop((engine, producer, consumer, mixer, original_assignment));

    // Same-process fresh-engine proof; no separate-process/music/device claim.
    let mut reopened = reopened_review_fixture(&samples, &original, frames);
    let reopened_fences = source_fences(&reopened);
    let restore = reopened
        .engine
        .begin_source_beatmap_restore_async(0, receipt.clone())
        .unwrap();
    let restored = review_artifact(&restore, &reopened);
    assert_eq!(review_artifact_record(&restored), expected);
    assert_eq!(restored.receipt_json(), receipt);
    assert!(review_public_gates(&restored)["point"]["local_bpm"].is_null());
    let restored_undo = review_edit_artifact(
        &reopened,
        &restored,
        json!({"schema_version":1,"operation":"undo"}),
    );
    let restored_undo_record = review_artifact_record(&restored_undo);
    for key in ["anchors", "intervals"] {
        assert_eq!(
            restored_undo_record["record"]["accepted"][key],
            before["record"]["accepted"][key]
        );
    }
    assert_eq!(source_fences(&reopened), reopened_fences);
    assert!(!raw_path.exists());
    drop((restored, restored_undo, restore));
    Python::attach(|_| {});
    wait_until(|| {
        reopened.engine.project_assets.collect_for_test();
        reopened
            .engine
            .project_assets
            .held_owner_count(&accepted_path)
            == 0
            && !reopened
                .engine
                .project_assets
                .map_retirement_pending_for_test(&accepted_path)
    });
    reopened.engine.shut_down().unwrap();
    drop(reopened);
    assert!(accepted_path.is_file());
    drop(directory);
}
