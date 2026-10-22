fn created(fixture: &ColdCustody) -> SourceBeatmapArtifact {
    let job = fixture
        .engine
        .begin_source_beatmap_create_async(0, 0.0, 0.001, None)
        .unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    let event = maps(fixture).pop().unwrap();
    resolve_artifact(&job, &delivery(&event)).unwrap()
}

#[test]
fn actual_manual_job_edits_undoes_and_restores_exact_same_source_artifacts() {
    let fixture = ColdCustody::new();
    let fences = source_fences(&fixture);
    let base = created(&fixture);
    let base_record = artifact_record(&base);
    assert_eq!(
        base_record["record"]["source"]["decoder_identity"],
        manifest(&fixture, 0)["decoder_identity"]
    );
    assert_eq!(base_record["record"]["raw"]["beat_seconds_bits"], json!([]));
    let edit = json!({"schema_version":1,"operation":"move_beat","beat":0.0,"sample_index":1,"sample_rate_hz":48000});
    let job = fixture
        .engine
        .begin_source_beatmap_edit_async(0, base.receipt_json().to_owned(), edit.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    let event = maps(&fixture).pop().unwrap();
    let edited = resolve_artifact(&job, &delivery(&event)).unwrap();
    assert_eq!(artifact_record(&base), base_record);
    assert_eq!(
        artifact_record(&edited)["record"]["raw"],
        base_record["record"]["raw"]
    );
    assert_eq!(
        artifact_record(&edited)["record"]["accepted"]["parent_revision"],
        base_record["record"]["accepted"]["map_revision"]
    );
    let undo = fixture
        .engine
        .begin_source_beatmap_edit_async(
            0,
            edited.receipt_json().to_owned(),
            json!({"schema_version":1,"operation":"undo"}).to_string(),
        )
        .unwrap();
    assert_eq!(wait_terminal(&undo)["stage"], "ready");
    let undo_event = maps(&fixture).pop().unwrap();
    let undone = resolve_artifact(&undo, &delivery(&undo_event)).unwrap();
    assert_eq!(
        artifact_record(&undone)["record"]["accepted"]["anchors"],
        base_record["record"]["accepted"]["anchors"]
    );
    let restored = fixture
        .engine
        .begin_source_beatmap_restore_async(0, edited.receipt_json().to_owned())
        .unwrap();
    assert_eq!(wait_terminal(&restored)["stage"], "ready");
    let restored_event = maps(&fixture).pop().unwrap();
    assert_eq!(
        artifact_record(&resolve_artifact(&restored, &delivery(&restored_event)).unwrap()),
        artifact_record(&edited)
    );
    assert_eq!(source_fences(&fixture), fences);
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (0, 0, 0));
    assert_offline_available(&fixture);
}

#[test]
fn cancelled_manual_and_edit_jobs_keep_original_admission_until_actual_worker_retirement() {
    for editing in [false, true] {
        let fixture = ColdCustody::new();
        let base = created(&fixture);
        let paths = ready_maps(&fixture);
        let fences = source_fences(&fixture);
        let held = occupy_workers(&fixture);
        let job = if editing {
            fixture.engine.begin_source_beatmap_edit_async(0,base.receipt_json().to_owned(),json!({"schema_version":1,"operation":"translate","sample_offset":-1,"sample_rate_hz":48000}).to_string()).unwrap()
        } else {
            fixture
                .engine
                .begin_source_beatmap_create_async(0, 0.0, 0.001, None)
                .unwrap()
        };
        assert_eq!(async_status(&job)["stage"], "queued");
        assert_offline_busy(&fixture);
        job.cancel();
        assert!(!async_status(&job)["terminal"].as_bool().unwrap());
        assert_offline_busy(&fixture);
        held.release();
        assert_eq!(wait_terminal(&job)["stage"], "cancelled");
        assert!(maps(&fixture).is_empty());
        assert_eq!(ready_maps(&fixture), paths);
        assert_eq!(source_fences(&fixture), fences);
        assert_offline_available(&fixture);
    }
}

#[test]
fn create_generation_fence_and_edit_rate_fail_without_new_artifact_or_source_change() {
    let fixture = ColdCustody::new();
    let base = created(&fixture);
    let fences = source_fences(&fixture);
    let paths = ready_maps(&fixture);
    let job = fixture
        .engine
        .begin_source_beatmap_create_async(0, 0.0, 0.001, Some(u64::MAX))
        .unwrap();
    let status = wait_terminal(&job);
    assert_eq!(status["stage"], "failed");
    assert!(
        status["detail"]
            .as_str()
            .unwrap()
            .contains("generation is stale")
    );
    assert!(maps(&fixture).is_empty());
    let job=fixture.engine.begin_source_beatmap_edit_async(0,base.receipt_json().to_owned(),json!({"schema_version":1,"operation":"translate","sample_offset":1,"sample_rate_hz":44100}).to_string()).unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "failed");
    assert!(maps(&fixture).is_empty());
    assert_eq!(ready_maps(&fixture), paths);
    assert_eq!(source_fences(&fixture), fences);
    assert_offline_available(&fixture);
}

#[test]
fn manual_and_edit_use_actual_full_queue_refusal_and_strict_small_intent_admission() {
    let fixture = ColdCustody::new();
    let base = created(&fixture);
    let fences = source_fences(&fixture);
    assert!(
        fixture
            .engine
            .begin_source_beatmap_edit_async(0, base.receipt_json().to_owned(), " ".repeat(8193))
            .is_err()
    );
    assert!(
        fixture
            .engine
            .begin_source_beatmap_edit_async(
                0,
                base.receipt_json().to_owned(),
                json!({"schema_version":1,"operation":"undo","extra":true}).to_string()
            )
            .is_err()
    );
    let held = occupy_workers(&fixture);
    for _ in 0..32 {
        fixture
            .engine
            .cold_jobs
            .submit(fixture.engine.cold_jobs.reserve().unwrap(), || {})
            .unwrap();
    }
    assert!(
        fixture
            .engine
            .begin_source_beatmap_create_async(0, 0.0, 0.001, None)
            .is_err()
    );
    assert!(
        fixture
            .engine
            .begin_source_beatmap_edit_async(
                0,
                base.receipt_json().to_owned(),
                json!({"schema_version":1,"operation":"undo"}).to_string()
            )
            .is_err()
    );
    assert_eq!(source_fences(&fixture), fences);
    assert_offline_available(&fixture);
    held.release();
    wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
}

#[test]
fn edit_receipt_is_reverified_against_replaced_actual_source_and_does_not_publish() {
    let mut fixture = ColdCustody::new();
    let base = created(&fixture);
    let base_record = artifact_record(&base);
    let mut source = fs::read(fixture.directory.path().join("external.wav")).unwrap();
    let last = source.len() - 1;
    source[last] ^= 1;
    let samples = samples_root(&fixture);
    let wrong = samples.join("different.wav");
    fs::write(&wrong, source).unwrap();
    load_actual(&mut fixture, &samples, &wrong, 0);
    let job=fixture.engine.begin_source_beatmap_edit_async(0,base.receipt_json().to_owned(),json!({"schema_version":1,"operation":"translate","sample_offset":1,"sample_rate_hz":48000}).to_string()).unwrap();
    let status = wait_terminal(&job);
    assert_eq!(status["stage"], "failed");
    assert!(
        status["detail"]
            .as_str()
            .unwrap()
            .contains("decoder identity differs")
    );
    assert!(maps(&fixture).is_empty());
    assert_eq!(artifact_record(&base), base_record);
    assert_offline_available(&fixture);
}

#[test]
fn actual_validate_store_restore_jobs_refuse_peak_before_json_parse_or_receipt_read() {
    let fixture = ColdCustody::new();
    let base = created(&fixture);
    let fences = source_fences(&fixture);
    let paths = ready_maps(&fixture);
    // Below the structural 64-MiB cap, but already beyond simultaneous trees,
    // retained input/index and actual held-source admission. Malformed input
    // and a deliberately incorrect receipt extent prove which guard ran first.
    let bytes = 16 * 1024 * 1024;
    for store in [false, true] {
        let input = " ".repeat(bytes);
        let job = if store {
            fixture
                .engine
                .begin_source_beatmap_store_async(0, input)
                .unwrap()
        } else {
            fixture.engine.begin_source_beatmap_async(0, input).unwrap()
        };
        let status = wait_terminal(&job);
        assert_eq!(status["stage"], "failed");
        assert!(status["detail"].as_str().unwrap().contains("512-MiB"));
        assert!(maps(&fixture).is_empty());
        assert_eq!(source_fences(&fixture), fences);
        assert_eq!(ready_maps(&fixture), paths);
        assert_offline_available(&fixture);
    }
    let mut receipt: Value = serde_json::from_str(base.receipt_json()).unwrap();
    receipt["bytes"] = json!(bytes);
    let job = fixture
        .engine
        .begin_source_beatmap_restore_async(0, receipt.to_string())
        .unwrap();
    let status = wait_terminal(&job);
    assert_eq!(status["stage"], "failed");
    assert!(status["detail"].as_str().unwrap().contains("512-MiB"));
    assert!(maps(&fixture).is_empty());
    assert_eq!(source_fences(&fixture), fences);
    assert_eq!(ready_maps(&fixture), paths);
    assert_offline_available(&fixture);
}
