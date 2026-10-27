// Genuine source/artifact/async custody; synthetic NN events are not inference quality.
use crate::audio_engine::analysis_pcm::MONO_RULE;
use crate::audio_engine::source_beatmap_capture::beat_this::{
    BeatThisRawData, BeatThisSourceInput, expected_logit_count, pinned_model,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};

fn automatic_fixture() -> (ColdCustody, Carrier, SourceBeatmapArtifact) {
    let fixture = ColdCustody::new_selected_with_signal(None, &vec![8192; 8 * 48_000]);
    let descriptor = manifest(&fixture, 0)["descriptor"]["decoder"].clone();
    let frames = descriptor["pcm"]["full_frames"].as_u64().unwrap() as usize;
    let rate = descriptor["pcm"]["rate_hz"].as_u64().unwrap() as u32;
    let mono = descriptor["pcm"]["mono_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let source = BeatThisSourceInput {
        decoder_identity: descriptor_digest(&descriptor).unwrap(),
        source_descriptor: descriptor,
        source_sample_rate_hz: rate,
        source_frame_count: frames,
        source_zero_frame: 0,
        mono_sha256: mono.clone(),
        mono_rule: MONO_RULE.into(),
    };
    let identity = json!({"pad_id":0,"request_id":1,"source_id":"synthetic-events-real-source-custody","source_generation":fixture.engine.loaded_source_generations.lock().unwrap()[0].0});
    let request = json!({"schema_version":1,"identity":identity,"model":pinned_model(),
        "pcm":{"path":fixture.directory.path().join("synthetic-worker-input.f32"),"sample_rate_hz":rate,"frame_count":frames,"origin_seconds":0.0,"dtype":"float32-le","channels":1}});
    let pack = |values: &[f64]| {
        STANDARD.encode(
            values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
    };
    let beats: Vec<_> = (0..16).map(|i| 0.125 + i as f64 * 0.5).collect();
    let logits = vec![0_f64; expected_logit_count(frames, rate).unwrap()];
    let response = json!({"schema_version":1,"encoding":"source-map-beat-this-worker-v1","identity":identity,
        "input":{"sample_rate_hz":rate,"frame_count":frames,"source_zero_frame":0,"mono_sha256":mono,"dtype":"float32-le"},
        "model":pinned_model(),"status":"ready","reason":"synthetic-events-real-source-custody","resources_released":true,
        "predictions":{"encoding":"float64-le/base64","beat_seconds":pack(&beats),"downbeat_seconds":pack(&[]),"beat_logits":pack(&logits),"downbeat_logits":pack(&logits)}});
    let raw =
        BeatThisRawData::from_worker(source, &request.to_string(), &response.to_string()).unwrap();
    let map = crate::audio_engine::source_beatmap::automatic::build(&raw, 0).unwrap();
    assert!(map.is_pristine_automatic());
    let mut analysis = analysis_for(&fixture, 0);
    analysis.automatic_source_map = Some(AutomaticSourceMap::Pending(map));
    let carrier = make_carrier(&fixture, 0, false, analysis, true);
    let (_, artifact, derived) = consume(&fixture.engine, &carrier).unwrap();
    drop(derived);
    (fixture, carrier, artifact)
}

fn export_job(
    fixture: &ColdCustody,
    owner: &Py<LoaderEventDelivery>,
) -> Option<(SourceBeatmapArtifact, Py<LoaderEventDelivery>)> {
    Python::attach(|py| {
        fixture
            .engine
            .consume_automatic_source_beatmap(py, &owner.borrow(py))
            .unwrap()
            .map(|tuple| {
                let tuple = tuple.bind(py);
                assert_eq!(tuple.get_item(0).unwrap().extract::<usize>().unwrap(), 0);
                let artifact = tuple
                    .get_item(1)
                    .unwrap()
                    .extract::<Py<SourceBeatmapArtifact>>()
                    .unwrap()
                    .borrow(py)
                    .clone();
                (artifact, tuple.get_item(2).unwrap().extract().unwrap())
            })
    })
}

fn assert_source_only(fixture: &ColdCustody, owner: &Py<LoaderEventDelivery>, current: bool) {
    Python::attach(|py| {
        assert_eq!(
            fixture
                .engine
                ._resolve_loader_event(&owner.borrow(py))
                .unwrap(),
            current.then_some((0, true))
        );
    });
}

#[test]
fn automatic_map_only_restore_exports_once_without_advancing_source_or_timing_and_survives_edit() {
    let (mut fixture, carrier, historical) = automatic_fixture();
    fixture.engine.set_pad_timing_intent(0, "manual").unwrap();
    fixture.dispatch_one();
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let epochs: Vec<_> = fixture
        .engine
        .prepared_source_epochs
        .iter()
        .map(|epoch| epoch.load(Ordering::Acquire))
        .collect();
    let raw_slot = crate::audio_engine::source_beatmap_raw::RawReviewLease::reserve().unwrap();
    let job = fixture
        .engine
        .begin_automatic_source_beatmap_restore_async(0, historical.receipt_json().to_owned())
        .unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    assert_eq!(
        fixture
            .engine
            .prepared_source_epochs
            .iter()
            .map(|e| e.load(Ordering::Acquire))
            .collect::<Vec<_>>(),
        epochs
    );
    let event = maps(&fixture).pop().unwrap();
    let original = delivery(&event);
    Python::attach(|py| {
        assert!(
            job.resolve_source_beatmap_completion(&original.borrow(py))
                .unwrap()
                .is_none()
        );
        assert!(
            job.resolve_source_beatmap_artifact_completion(&original.borrow(py))
                .unwrap()
                .is_none()
        );
    });
    let (_, derived) = export_job(&fixture, &original).unwrap();
    let clone = Python::attach(|py| Py::new(py, original.borrow(py).clone()).unwrap());
    assert!(export_job(&fixture, &clone).is_none());
    assert_source_only(&fixture, &derived, true);
    drop(job); // Natural drop only after actual cleanup/lane retirement.
    edit_bpm(&mut fixture);
    assert_source_only(&fixture, &derived, true);
    assert!(export_job(&fixture, &original).is_none());
    assert!(carrier.path.is_file());
    drop(raw_slot);
}

#[test]
fn automatic_map_only_restore_claims_are_independent_and_cancel_remains_explicit() {
    let (mut fixture, _carrier, historical) = automatic_fixture();
    for (intent, cancel) in [("tap", false), ("manual", true)] {
        fixture.engine.set_pad_timing_intent(0, intent).unwrap();
        fixture.dispatch_one();
        let job = fixture
            .engine
            .begin_automatic_source_beatmap_restore_async(0, historical.receipt_json().to_owned())
            .unwrap();
        assert_eq!(wait_terminal(&job)["stage"], "ready");
        let event = maps(&fixture).pop().unwrap();
        let original = delivery(&event);
        let (_, derived) = export_job(&fixture, &original).unwrap();
        if cancel {
            job.cancel();
        }
        assert_source_only(&fixture, &derived, !cancel);
        drop(job);
        assert!(historical.map().is_pristine_automatic());
    }
}

#[test]
fn automatic_map_only_restore_stale_epoch_and_early_drop_do_not_export_or_delete_history() {
    let (mut fixture, carrier, historical) = automatic_fixture();
    let receipt = historical.receipt_json().to_owned();
    let job = fixture
        .engine
        .begin_automatic_source_beatmap_restore_async(0, receipt.clone())
        .unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    let event = maps(&fixture).pop().unwrap();
    let original = delivery(&event);
    edit_bpm(&mut fixture);
    assert!(export_job(&fixture, &original).is_none());
    drop(job);
    for consume_first in [false, true] {
        let unfinished = fixture.engine.begin_source_beatmap(0).unwrap();
        unfinished.publish_automatic_map_before_retirement_for_test(&receipt);
        let event = maps(&fixture).pop().unwrap();
        let original = delivery(&event);
        let derived = consume_first.then(|| export_job(&fixture, &original).unwrap().1);
        if let Some(derived) = &derived {
            assert_source_only(&fixture, derived, true);
        }
        drop(unfinished); // Must cancel, rather than falsely declare terminal.
        assert!(export_job(&fixture, &original).is_none());
        if let Some(derived) = &derived {
            assert_source_only(&fixture, derived, false);
        }
        assert!(
            carrier.path.is_file(),
            "restored artifact has no new birth/deletion right"
        );
        wait_until(|| !fixture.engine.offline_jobs.has_pad(0));
    }
}

#[test]
fn automatic_map_only_restore_refuses_manual_and_edited_descendant_provenance() {
    let (fixture, _carrier, historical) = automatic_fixture();
    let manual = fixture
        .engine
        .begin_source_beatmap_store_async(0, record(&fixture, 0).to_string())
        .unwrap();
    assert_eq!(wait_terminal(&manual)["stage"], "ready");
    let event = maps(&fixture).pop().unwrap();
    let artifact = Python::attach(|py| {
        manual
            .resolve_source_beatmap_artifact_completion(&delivery(&event).borrow(py))
            .unwrap()
            .unwrap()
    });
    let edit=fixture.engine.begin_source_beatmap_edit_async(0,historical.receipt_json().to_owned(),json!({"schema_version":1,"operation":"move_beat","beat":4.0,"sample_index":102001,"sample_rate_hz":48000}).to_string()).unwrap();
    assert_eq!(wait_terminal(&edit)["stage"], "ready");
    let event = maps(&fixture).pop().unwrap();
    let descendant = Python::attach(|py| {
        edit.resolve_source_beatmap_artifact_completion(&delivery(&event).borrow(py))
            .unwrap()
            .unwrap()
    });
    assert!(!descendant.map().is_pristine_automatic());
    for unsupported in [&artifact, &descendant] {
        let job = fixture
            .engine
            .begin_automatic_source_beatmap_restore_async(0, unsupported.receipt_json().to_owned())
            .unwrap();
        assert_eq!(wait_terminal(&job)["stage"], "failed");
        assert!(maps(&fixture).is_empty());
    }
}
