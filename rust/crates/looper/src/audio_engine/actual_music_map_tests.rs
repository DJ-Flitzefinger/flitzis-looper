// Exact authorized music, genuine full-source NN and Native map qualification.
use crate::audio_engine::loader_events::LoaderEventDelivery;
use crate::audio_engine::source_map_admission::{
    RuntimeMapClass, SourceMapProjection, SourceMapRow,
};
use pyo3::types::PyDict;

#[test]
#[ignore = "authorized two real songs; genuine map-only NN and 8/32/64-bar Native qualification"]
fn authorized_music_automatic_maps_qualify_actual_loops() {
    const TEST: &str = "audio_engine::cold_load::tests::automatic_analysis_tests::actual_music_loop_tests::maps::authorized_music_automatic_maps_qualify_actual_loops";
    if std::env::var_os(CHILD).is_none() {
        child(TEST, 48_000, true);
        return;
    }
    Python::initialize();
    let root = PathBuf::from(std::env::var_os("FLITZI_MUSIC_LOOP_ROOT").expect("owned root"));
    let input_path = PathBuf::from(
        std::env::var_os("FLITZI_MUSIC_LOOP_INPUTS").expect("explicit source-bound inputs"),
    );
    let installation =
        std::env::var("LOOPER_BEAT_THIS_INSTALL_DIR").expect("verified installed worker root");
    assert!(root.is_absolute() && input_path.is_absolute());
    let inputs: Value = serde_json::from_slice(&fs::read(&input_path).unwrap()).unwrap();
    let sources = inputs["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    let (_directory, _cwd) = owned_directory_at(&root.join("tmp"));
    let mut path = Path::new();
    let mut source_events = Vec::new();
    let mut results = Vec::new();
    let mut all_ready = true;
    for (id, source) in sources.iter().enumerate() {
        let (event, decoder, playback) = verified_import(&mut path, id, id, source);
        source_events.push(event);
        let (artifact, derived) = automatic_map(&mut path, id, &installation, None);
        let map = artifact.map();
        assert!(map.is_pristine_automatic());
        fs::write(
            root.join(format!("source-map-pad{id}.json")),
            map.canonical_record(),
        )
        .unwrap();
        let value: Value = serde_json::from_str(map.canonical_record()).unwrap();
        assert_eq!(
            value["record"]["source"]["decoder_descriptor"]["pcm"],
            decoder
        );
        assert!(
            value["record"]["accepted"]["intervals"]
                .as_array()
                .unwrap()
                .iter()
                .all(|i| i["meter"]["status"] == "unknown")
        );
        let mut cases = Vec::new();
        let bars = if id == 0 { vec![32, 64] } else { vec![8] };
        for bars in bars {
            let start = (source["saved_start"].as_f64().unwrap() * 48_000.).round() / 48_000.;
            let Some(interval) = map.canonical_loop_interval(start, f64::from(bars) * 4., false)
            else {
                cases.push(
                    json!({"bars":bars,"start_s":start,"status":"unsupported_original_coverage"}),
                );
                all_ready = false;
                continue;
            };
            let end = (interval.source_end * 48_000.).round() / 48_000.;
            assert!(end < playback["full_frames"].as_u64().unwrap() as f64 / 48_000.);
            let window = path
                .engine
                .prepare_resident_control_with_source(
                    id,
                    Some(start),
                    Some(end),
                    None,
                    Some(false),
                    Some(path.generation(id)),
                )
                .unwrap();
            wait_until_for(Duration::from_secs(120), || {
                path.drain();
                resident_relocation::reconcile(&path.engine).unwrap();
                match window.publication_status() {
                    "accepted" => true,
                    "preparing" | "pending" | "adopting" => false,
                    other => panic!("actual map resident ACK {other}: {:?}", window.error()),
                }
            });
            let binding = match path
                .engine
                .current_input_runtime_pad_binding(id)
                .map_err(|e| e.to_string())
                .and_then(|b| b.ok_or_else(|| "current source binding unavailable".to_owned()))
            {
                Ok(binding) => binding,
                Err(error) => {
                    cases.push(json!({"bars":bars,"start_s":start,"end_s":end,"status":"binding_refused","error":error}));
                    all_ready = false;
                    continue;
                }
            };
            let row = SourceMapRow {
                sync_requested: true,
                intent_revision: 1,
                delivery: Some(Python::attach(|py| derived.borrow(py).clone())),
                exact_quarters: Some(f64::from(bars) * 4.),
                ordinary_pending: false,
                phase_origin_seconds: Some(source["grid_origin"].as_f64().unwrap()),
                operational_four_quarter_bars: true,
            };
            let qualified = match SourceMapProjection::qualify(&row, Some(&binding)).and_then(
                |mut projection| {
                    projection.remask_parents(true, true);
                    projection
                        .snapshot(&path.engine.input_runtime_ownership, binding.binding)
                        .admit(&path.engine.input_runtime_ownership, binding.binding)
                        .map_err(|e| format!("{e:?}"))?
                        .ok_or_else(|| "qualified source map unavailable".to_owned())
                },
            ) {
                Ok(qualified) => qualified,
                Err(error) => {
                    cases.push(json!({"bars":bars,"start_s":start,"end_s":end,"status":"qualification_refused","error":error}));
                    all_ready = false;
                    continue;
                }
            };
            let (prepared, source) =
                crate::audio_engine::pad_start::MappedStartSource::prepare_runtime_map(
                    &qualified,
                    binding.binding,
                    start,
                    Some(end),
                );
            let ready = prepared.class == RuntimeMapClass::Ready
                && qualified.loaded_rate() == 48_000
                && qualified.map().map_revision() == map.map_revision()
                && source
                    .as_ref()
                    .is_some_and(|s| s.geometry().has_absolute_grid_phase());
            all_ready &= ready;
            cases.push(json!({"bars":bars,"start_s":start,"end_s":end,"status":format!("{:?}",prepared.class),"absolute_grid_ready":ready,"native_loaded_rate":qualified.loaded_rate(),"original_source_rate":decoder["rate_hz"],"same_original_map_revision":qualified.map().map_revision()==map.map_revision()}));
        }
        results.push(json!({"pad_id":id,"source_sha256":source["source_sha256"],"decoder":decoder,"playback":playback,"map_revision":map.map_revision(),"cases":cases}));
        fs::write(root.join("actual-music-maps.json"),serde_json::to_vec_pretty(&json!({"scope":"Partial genuine source/NN/OwnACK/row qualification; no mapped reads or hearing acceptance","complete":false,"all_ready":all_ready,"sources":results})).unwrap()).unwrap();
        artifact.retire_unselected();
        assert_eq!(
            hash_file(std::path::Path::new(source["path"].as_str().unwrap())),
            source["source_sha256"]
        );
    }
    path.engine.stop_all().unwrap();
    path.drain();
    path.engine.shut_down().unwrap();
    fs::write(root.join("actual-music-maps.json"),serde_json::to_vec_pretty(&json!({"scope":"Genuine source/NN/OwnACK/row qualification only; no mapped audio read, device, 64-bar listening or musical acceptance","complete":true,"all_ready":all_ready,"sources":results})).unwrap()).unwrap();
    assert!(
        all_ready,
        "actual maps must qualify current 8/32/64bar loops; original failure retained, no marker shift/clipping/fallback"
    );
    println!(
        "ACTUAL_MUSIC_MAPS {}",
        root.join("actual-music-maps.json").display()
    );
}

pub(super) fn verified_import(
    path: &mut Path,
    id: usize,
    expected_source: usize,
    source: &Value,
) -> (LoaderEvent, Value, Value) {
    assert_eq!(source["pad_id"], expected_source);
    let original = PathBuf::from(source["path"].as_str().unwrap());
    let expected = [
        "339911dd7a54af5efc80fdcc05e1432648de9640348803e9b4c0657f59c1a5ea",
        "fadd1534a3a51cf6de7fcd4d3ebefdc3e596a38c451e3c71cb6dc8c9350f8497",
    ][expected_source];
    assert_eq!(source["source_sha256"], expected);
    assert!(original.is_absolute());
    assert_eq!(hash_file(&original), expected);
    let request = path
        .engine
        .load_sample_async(
            id,
            original.to_str().unwrap().into(),
            Some(false),
            false,
            true,
            None,
            None,
            false,
            "import",
        )
        .unwrap();
    wait_until_for(Duration::from_secs(120), || {
        path.drain();
        path.engine.input_runtime_ownership.cold_status(id, request) == Some(2)
    });
    let event = terminal(&path.engine, request);
    assert!(matches!(&event,LoaderEvent::Success {id:actual,analysis:None,..} if *actual==id));
    let (decoder, playback) = {
        let leases = path.engine.cold_leases.lock().unwrap();
        let lease = leases[id].as_ref().unwrap();
        assert_eq!(hash_file(&lease.original_path), expected);
        let decoder = lease.manifest.descriptor["decoder"]["pcm"].clone();
        let playback = lease.manifest.descriptor["playback"]["pcm"].clone();
        assert_eq!(
            hash_file(&lease.cache_path.join("decoder.f32le")),
            decoder["interleaved_sha256"]
        );
        assert_eq!(
            hash_file(&lease.cache_path.join("playback.f32le")),
            playback["interleaved_sha256"]
        );
        assert_eq!(&decoder, &source["expected_decoder_pcm"]);
        assert!(
            [
                "rate_hz",
                "channels",
                "full_frames",
                "interleaved_sha256",
                "mono_sha256",
                "source_zero_bits"
            ]
            .iter()
            .all(|key| !source["expected_playback_pcm"][key].is_null()),
            "independent playback fingerprint is incomplete"
        );
        for (key, value) in source["expected_playback_pcm"].as_object().unwrap() {
            assert_eq!(&playback[key], value);
        }
        (decoder, playback)
    };
    assert_eq!(playback["rate_hz"], 48_000);
    if id == 0 {
        assert_eq!(decoder["rate_hz"], 44_100);
    }
    path.engine
        .set_pad_bpm_with_source(
            id,
            Some(source["bpm"].as_f64().unwrap()),
            Some(path.generation(id)),
        )
        .unwrap();
    path.drain();
    path.engine
        .set_pad_timing_metadata_with_source(
            id,
            source["grid_origin"].as_f64().unwrap(),
            Some(path.generation(id)),
        )
        .unwrap();
    path.drain();
    (event, decoder, playback)
}

pub(super) fn automatic_map(
    path: &mut Path,
    id: usize,
    installation: &str,
    restore: Option<String>,
) -> (
    crate::audio_engine::SourceBeatmapArtifact,
    Py<LoaderEventDelivery>,
) {
    let requests = path.engine.pad_request_ids.lock().unwrap().clone();
    let epochs: Vec<_> = path
        .engine
        .prepared_source_epochs
        .iter()
        .map(|e| e.load(Ordering::Acquire))
        .collect();
    let job = match restore {
        Some(receipt) => path
            .engine
            .begin_automatic_source_beatmap_restore_async(id, receipt)
            .unwrap(),
        None => path
            .engine
            .begin_automatic_source_beatmap_async(id, installation.to_owned(), path.generation(id))
            .unwrap(),
    };
    let mut status_detail = None;
    wait_until_for(Duration::from_secs(240), || {
        Python::attach(|py| {
            let status = job.source_beatmap_async_status(py).unwrap().unwrap();
            let status = status.bind(py);
            status_detail = status
                .get_item("detail")
                .unwrap()
                .unwrap()
                .extract::<Option<String>>()
                .unwrap();
            if status
                .get_item("terminal")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
            {
                assert_eq!(
                    status
                        .get_item("stage")
                        .unwrap()
                        .unwrap()
                        .extract::<String>()
                        .unwrap(),
                    "ready",
                    "{status_detail:?}"
                );
                true
            } else {
                false
            }
        })
    });
    assert_eq!(*path.engine.pad_request_ids.lock().unwrap(), requests);
    assert_eq!(
        path.engine
            .prepared_source_epochs
            .iter()
            .map(|e| e.load(Ordering::Acquire))
            .collect::<Vec<_>>(),
        epochs
    );
    let (artifact, derived) = Python::attach(|py| {
        let mut found = None;
        while let Some(event) = path.engine.poll_loader_events(py).unwrap() {
            let dict = event.bind(py).cast::<PyDict>().unwrap();
            if dict
                .get_item("type")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap()
                == "source_beatmap_completed"
            {
                assert_eq!(
                    dict.get_item("id")
                        .unwrap()
                        .unwrap()
                        .extract::<usize>()
                        .unwrap(),
                    id
                );
                let owner = dict
                    .get_item("_native_event_owner")
                    .unwrap()
                    .unwrap()
                    .extract::<PyRef<'_, LoaderEventDelivery>>()
                    .unwrap();
                let export = path
                    .engine
                    .consume_automatic_source_beatmap(py, &owner)
                    .unwrap()
                    .unwrap();
                let tuple = export.bind(py);
                assert_eq!(tuple.get_item(0).unwrap().extract::<usize>().unwrap(), id);
                let artifact = tuple
                    .get_item(1)
                    .unwrap()
                    .extract::<Py<crate::audio_engine::SourceBeatmapArtifact>>()
                    .unwrap()
                    .borrow(py)
                    .clone();
                let derived = tuple
                    .get_item(2)
                    .unwrap()
                    .extract::<Py<LoaderEventDelivery>>()
                    .unwrap();
                assert!(
                    path.engine
                        .consume_automatic_source_beatmap(py, &owner)
                        .unwrap()
                        .is_none()
                );
                found = Some((artifact, derived));
            }
        }
        found.expect("genuine automatic map-only delivery")
    });
    drop(job); // Strict terminal already observed, so natural release cannot cancel.
    (artifact, derived)
}
