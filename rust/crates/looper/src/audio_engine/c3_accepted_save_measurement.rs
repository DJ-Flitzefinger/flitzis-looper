//! Actual current600s raw-QM verification and atomic project-save costs.
use super::*;
use crate::audio_engine::constant_timing::{
    capture_saved, capture_saved_with_measurement_limit, export_current, restore_saved,
};
use pyo3::exceptions::PyValueError;
use pyo3::types::PyDict;

#[pyclass]
struct SaveOwner {
    engine: Arc<AudioEngine>,
}

#[pymethods]
impl SaveOwner {
    fn export_current_constant_timing(
        &self,
        py: Python<'_>,
        id: usize,
        source_path: String,
    ) -> PyResult<Option<String>> {
        py.detach(|| export_current(&self.engine, id, source_path))
            .map_err(PyValueError::new_err)
    }
    fn pad_timing_intent(&self, id: usize) -> PyResult<&'static str> {
        self.engine.pad_timing_intent(id)
    }
}

#[test]
#[ignore = "isolated actual600s current raw-QM accepted save/restore integrity and process resources"]
fn c3_actual_600s_accepted_save() {
    Python::initialize();
    let python_runtime = python_runtime_identity();
    let config_path = PathBuf::from(std::env::var_os("FLITZI_C3_LONG_CONFIG").unwrap());
    let config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    let root = PathBuf::from(config["project_root"].as_str().unwrap());
    std::env::set_current_dir(&root).unwrap();
    let original = root.join("samples/acceptance-source.wav");
    let frozen_seed = fs::read_to_string(config["seed"].as_str().unwrap()).unwrap();
    let saved_config_before = if config["stage"] == "warm" {
        Some(full_hash(&root.join("samples/flitzis_looper.config.json")))
    } else {
        None
    };
    let seed = if config["stage"] == "warm" {
        Python::attach(|py| {
            let globals = PyDict::new(py);
            py.run(c"from flitzis_looper.controller.persistence import ProjectPersistence\nimport json\nloaded=ProjectPersistence.from_config_path().project\nassert loaded.sample_paths[0]=='samples/acceptance-source.wav'\nassert loaded.sample_durations[0]==600.0\nassert loaded.pad_timing_intent[0]=='automatic'\nrestored_seed=json.dumps(loaded.sample_analysis[0].accepted_timing.model_dump(mode='json'))\n", None, Some(&globals)).unwrap();
            let actual: String = globals
                .get_item("restored_seed")
                .unwrap()
                .unwrap()
                .extract()
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&actual).unwrap(),
                serde_json::from_str::<Value>(&frozen_seed).unwrap()
            );
            actual
        })
    } else {
        frozen_seed.clone()
    };
    assert_eq!(
        full_hash(&original),
        "96ffe98cf44215719b0b57d605d6dc586c9c4e763ad3d47d512d0ba787d204ef"
    );
    let engine = Arc::new(AudioEngine::new().unwrap());
    let mut callback = Callback::new(&engine, 48_000);
    let (producer, mut consumer) = rtrb::RingBuffer::new(16);
    let producer = Arc::new(Mutex::new(producer));
    let load = measure(|| {
        println!("C3 600s: productive source restore and ACK");
        let request = admit_for_format_selected(
            &engine,
            0,
            "samples/acceptance-source.wav".into(),
            (
                false,
                true,
                true,
                Some(ResidentLoadHint {
                    start_s: 42.0,
                    end_s: 42.5,
                    key_lock: false,
                }),
            ),
            producer.clone(),
            (2, 48000, root.join("samples")),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(600);
        loop {
            drain(&mut callback, &mut consumer);
            let event = engine.loader_rx.lock().unwrap().try_recv();
            if let Ok(LoaderEvent::Success {
                id: 0, request_id, ..
            }) = event
            {
                assert_eq!(request_id, request);
                break;
            }
            if let Ok(LoaderEvent::Error { error, .. }) = event {
                panic!("actual600s load: {error}");
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        wait_until(Duration::from_secs(10), || {
            engine.cold_loading[0].load(Ordering::Acquire) == 0
        });
        let sample = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
        assert_eq!(sample.frame_count(), 28_800_000);
        assert_eq!(sample.samples.len() * 4, 192000);
        assert!(
            engine
                .input_runtime_ownership
                .source_current(0, &sample, 48000)
        );
        let manifest: Value =
            serde_json::from_str(&engine.cold_source_manifest(0).unwrap().unwrap()).unwrap();
        assert_eq!(
            fs::canonicalize(PathBuf::from(manifest["original_path"].as_str().unwrap())).unwrap(),
            fs::canonicalize(&original).unwrap()
        );
        if config["stage"] == "warm" {
            assert_eq!(manifest["integrity"]["warm"], true);
        }
        json!({"request":request,"matching_ack":true,"manifest":manifest,
            "full_frames":sample.frame_count(),"resident_pcm_bytes":sample.samples.len()*4})
    });
    // Consuming native metadata must also acknowledge its saved assignment,
    // as the productive loader does through ProjectAssetLifecycle.acquire().
    let saved_assignment = engine
        .acquire_project_asset_lease(original.to_string_lossy().into_owned())
        .unwrap();
    let saved_cache = PathBuf::from(load["detail"]["manifest"]["cache_path"].as_str().unwrap());
    let default_rejection = measure(|| {
        println!("C3 600s: ordinary512MiB rejection");
        let saved =
            capture_saved(&engine, 0, &seed, original.to_string_lossy().into_owned()).unwrap();
        let error = restore_saved(&engine, &producer, &saved)
            .err()
            .expect("default512MiB must reject");
        assert!(error.to_string().contains("PCM byte limit"), "{error}");
        json!({"limit_bytes":512*1024*1024,"error":error.to_string(),"accepted":false})
    });
    let adoption = measure(|| {
        println!("C3 600s: explicit1GiB source-verified adoption");
        let saved = capture_saved_with_measurement_limit(
            &engine,
            0,
            &seed,
            original.to_string_lossy().into_owned(),
            1024 * 1024 * 1024,
        )
        .unwrap();
        let ticket = restore_saved(&engine, &producer, &saved).unwrap();
        assert_eq!(ticket.publication_status().unwrap(), "pending");
        assert!(
            export_current(&engine, 0, original.to_string_lossy().into_owned())
                .unwrap()
                .is_none()
        );
        drain(&mut callback, &mut consumer);
        assert_eq!(ticket.publication_status().unwrap(), "accepted");
        let (ticket_metadata, current_metadata) = Python::attach(|py| {
            let json_module = py.import("json").unwrap();
            let ticket_metadata: String = json_module
                .call_method1("dumps", (ticket.metadata(py).unwrap(),))
                .unwrap()
                .extract()
                .unwrap();
            let current = crate::audio_engine::constant_timing::current_metadata(&engine, py, 0)
                .unwrap()
                .unwrap();
            let current_metadata: String = json_module
                .call_method1("dumps", (current,))
                .unwrap()
                .extract()
                .unwrap();
            (
                serde_json::from_str::<Value>(&ticket_metadata).unwrap(),
                serde_json::from_str::<Value>(&current_metadata).unwrap(),
            )
        });
        assert_eq!(
            ticket_metadata["request_id"],
            current_metadata["accepted_request_id"]
        );
        assert_eq!(ticket_metadata["source_id"], current_metadata["source_id"]);
        assert_eq!(
            current_metadata["publication_epoch"],
            engine.current_timing_acknowledgements.current_epoch(0)
        );
        json!({"status":ticket.publication_status().unwrap(),"loaded_source_request":load["detail"]["request"],
            "actual_ticket_metadata":ticket_metadata,"actual_current_metadata":current_metadata,
            "prepared_source_epoch":engine.prepared_source_epochs[0].load(Ordering::Acquire),
            "native_ack_publication_epoch":engine.current_timing_acknowledgements.current_epoch(0),
            "explicit_probe_pcm_budget_bytes":1024*1024*1024})
    });
    let globals = Python::attach(|py| {
        let globals = PyDict::new(py);
        globals
            .set_item(
                "audio",
                Py::new(
                    py,
                    SaveOwner {
                        engine: engine.clone(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        globals.set_item("seed", seed.clone()).unwrap();
        py.run(c"from flitzis_looper.controller.persistence import ProjectPersistence\nfrom flitzis_looper.models import ProjectState\nimport json\nproject=ProjectState()\nproject.sample_paths[0]='samples/acceptance-source.wav'\nproject.sample_durations[0]=600.0\nproject.pad_loop_auto[0]=False\nproject.pad_loop_start_s[0]=42.0\nproject.pad_loop_end_s[0]=42.5\nproject.pad_timing_intent[0]='automatic'\npersistence=ProjectPersistence(project)\npersistence.bind_audio(audio)\n",None,Some(&globals)).unwrap();
        globals.unbind()
    });
    let save = measure(|| {
        println!("C3 600s: actual atomic ProjectPersistence save");
        Python::attach(|py| {
            py.run(c"persistence.mark_dirty()\nassert persistence.flush_if_dirty()\nassert not persistence._dirty\nsaved=ProjectPersistence.from_config_path(persistence.config_path).project\nassert saved.pad_timing_intent[0]=='automatic'\nassert saved.sample_durations[0]==600.0\nassert saved.sample_paths[0]=='samples/acceptance-source.wav'\nassert saved.sample_analysis[0].accepted_timing.model_dump(mode='json') == json.loads(seed)\n",None,Some(globals.bind(py))).unwrap();
            json!({"actual_project_persistence":true,"config_bytes":fs::metadata(root.join("samples/flitzis_looper.config.json")).unwrap().len(),
            "current_raw_qm_source_verified":true,"explicit_probe_limit_bytes":1024*1024*1024})
        })
    });
    let exported = measure(|| {
        println!("C3 600s: actual native current export");
        let encoded = export_current(&engine, 0, original.to_string_lossy().into_owned())
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&encoded).unwrap(),
            serde_json::from_str::<Value>(&seed).unwrap()
        );
        json!({"encoded_bytes":encoded.len(),"same_supported_historical_envelope":true})
    });
    let corrupted = root.join("samples/isolated-corrupt-save.wav");
    fs::copy(&original, &corrupted).unwrap();
    {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&corrupted)
            .unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        let mut byte = [0_u8];
        file.read_exact(&mut byte).unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        file.write_all(&[byte[0] ^ 1]).unwrap();
        file.sync_all().unwrap();
    }
    assert_eq!(
        fs::metadata(&original).unwrap().len(),
        fs::metadata(&corrupted).unwrap().len()
    );
    let rejected_save = measure(|| {
        println!("C3 600s: actual same-size corruption rejection");
        Python::attach(|py| {
            py.run(c"before_rejection=persistence.config_path.read_bytes()\nproject.sample_paths[0]='samples/isolated-corrupt-save.wav'\npersistence.mark_dirty()\nassert not persistence.flush_if_dirty()\nassert persistence._dirty\nassert persistence.config_path.read_bytes()==before_rejection\nproject.sample_paths[0]='samples/acceptance-source.wav'\nassert persistence.flush_if_dirty()\nassert not persistence._dirty\n", None, Some(globals.bind(py))).unwrap();
            json!({"real_native_hash_rejection":true,"previous_atomic_file_preserved":true,"dirty_until_corrected":true,"isolated_same_size_corruption":true})
        })
    });
    Python::attach(|_| drop(globals));
    drop(callback);
    drop(saved_assignment);
    let before_drop = process::snapshot();
    drop(engine);
    Python::attach(|_| {});
    let after_drop = process::snapshot();
    std::thread::sleep(Duration::from_millis(100));
    for filename in ["decoder.f32le", "playback.f32le", "manifest.json"] {
        assert!(saved_cache.join(filename).is_file());
    }
    let result = json!({"schema":"c3-current600s-save-v1","configuration":config,
        "python_runtime":python_runtime,"native_runtime":process::runtime_identity().unwrap(),
        "saved_assignment_claimed":true,"persisted_cache_present_after_owner_drop":true,
        "source_sha256":full_hash(&original),"frozen_seed_sha256":format!("{:x}",Sha256::digest(frozen_seed.as_bytes())),
        "restored_seed_encoding_sha256":format!("{:x}",Sha256::digest(seed.as_bytes())), "saved_config_before_sha256":saved_config_before,
        "load":load,"ordinary_default_rejection":default_rejection,"explicit_adoption":adoption,
        "actual_project_save":save,"actual_native_export":exported,"rejected_actual_save":rejected_save,
        "before_drop":before_drop,"after_drop":after_drop,
        "scope":"real600s source/cache/currentACK and actual atomic ProjectPersistence; explicit diagnostic1GiB; ordinary512MiB rejection; no device/hearing acceptance"});
    fs::write(
        PathBuf::from(std::env::var_os("FLITZI_C3_OUTPUT").unwrap()),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
}
