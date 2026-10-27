// Actual source-qualified durable artifacts; bounded software proof, not musical acceptance.

mod automatic_carrier {
    use super::*;
    include!("automatic_map_carrier_tests.rs");
}

mod editor_async_tests {
    use super::*;
    include!("source_beatmap_editor_async_tests.rs");
}

use super::*;
use crate::audio_engine::SourceBeatmapArtifact;
use crate::audio_engine::source_beatmap_artifact::StorePoint;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[test]
fn automatic_selection_intent_preserves_legacy_shape_and_strict_optional_provenance() {
    use crate::audio_engine::source_beatmap_artifact::validate_intent_value;
    let mut intent = serde_json::json!({"schema_version": 1, "revision": 1, "selected": null});
    assert!(validate_intent_value(&intent).is_ok());
    intent["automatic"] = false.into();
    assert!(validate_intent_value(&intent).is_ok());
    intent["automatic"] = true.into();
    assert!(validate_intent_value(&intent).is_err());
    intent["automatic"] = 1.into();
    assert!(validate_intent_value(&intent).is_err());
    intent["selected"] = serde_json::json!({
        "schema_version": 1, "kind": "source-beatmap-v1",
        "reference": format!("samples/.pcm-cache/source-beatmaps/v1/.ready-{}/map.json", "a".repeat(32)),
        "bytes": 2048, "sha256": "b".repeat(64),
        "map_revision": "c".repeat(64), "decoder_identity": "d".repeat(64)
    });
    intent["automatic"] = true.into();
    assert!(validate_intent_value(&intent).is_ok());
    intent["unexpected"] = false.into();
    assert!(validate_intent_value(&intent).is_err());
}

fn samples_root(fixture: &ColdCustody) -> PathBuf {
    fixture.engine.cold_leases.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .original_path
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "samples"))
        .unwrap()
        .to_path_buf()
}

fn original_path(fixture: &ColdCustody, id: usize) -> PathBuf {
    fixture.engine.cold_leases.lock().unwrap()[id]
        .as_ref()
        .unwrap()
        .original_path
        .clone()
}

fn receipt_from_event(event: &Py<PyAny>) -> Value {
    Python::attach(|py| {
        let encoded: String = event
            .bind(py)
            .cast::<PyDict>()
            .unwrap()
            .get_item("artifact_receipt")
            .unwrap()
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    })
}

fn artifact_path(fixture: &ColdCustody, receipt: &Value) -> PathBuf {
    crate::audio_engine::material_paths::resolve(
        &samples_root(fixture),
        Path::new(receipt["reference"].as_str().unwrap()),
    )
    .unwrap()
    .path
}

fn resolve_artifact(
    job: &OfflineAnalysisJob,
    owner: &Py<LoaderEventDelivery>,
) -> Option<SourceBeatmapArtifact> {
    Python::attach(|py| {
        job.resolve_source_beatmap_artifact_completion(&owner.borrow(py))
            .unwrap()
    })
}

fn artifact_record(artifact: &SourceBeatmapArtifact) -> Value {
    Python::attach(|py| {
        let object = Py::new(py, artifact.map()).unwrap();
        let encoded: String = object
            .bind(py)
            .call_method0("record_json")
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    })
}

fn flush_python_drops() {
    Python::attach(|_| {});
}

fn wait_output_retired(fixture: &ColdCustody, path: &Path) {
    wait_until(|| {
        fixture.engine.project_assets.held_owner_count(path) == 0
            && !fixture
                .engine
                .project_assets
                .map_retirement_pending_for_test(path)
    });
}

fn ready_maps(fixture: &ColdCustody) -> Vec<PathBuf> {
    let root = crate::audio_engine::material_paths::material_root_for_original(
        &samples_root(fixture),
        &original_path(fixture, 0),
    )
    .unwrap()
    .join(".pcm-cache")
    .join("source-beatmaps")
    .join("v1");
    if !root.exists() {
        return Vec::new();
    }
    let mut paths: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(crate::audio_engine::material_paths::ready_generation)
        })
        .map(|entry| entry.path().join("map.json"))
        .collect();
    paths.sort();
    paths
}

fn blocked_store(
    fixture: &ColdCustody,
    expected: &Value,
) -> (OfflineAnalysisJob, OccupiedWorkers, PathBuf) {
    let held = OccupiedWorkers {
        gate: Arc::new((Mutex::new(false), Condvar::new())),
    };
    let gate = held.gate.clone();
    let (entered, started) = mpsc::channel();
    let job = fixture
        .engine
        .begin_source_beatmap_store_async_with(0, expected.to_string(), move || {
            entered.send(()).unwrap();
            let mut open = gate.0.lock().unwrap();
            while !*open {
                open = gate.1.wait(open).unwrap();
            }
        })
        .unwrap();
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(async_status(&job)["stage"], "running");
    wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (1, 0, 1));
    let paths = ready_maps(fixture);
    assert_eq!(paths.len(), 1);
    assert!(fixture.engine.project_assets.held_owner_count(&paths[0]) > 0);
    (job, held, paths[0].clone())
}

fn stored_event(fixture: &ColdCustody, expected: &Value) -> (OfflineAnalysisJob, Py<PyAny>) {
    assert!(maps(fixture).is_empty());
    let job = fixture
        .engine
        .begin_source_beatmap_store_async(0, expected.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (0, 0, 0));
    let mut events = maps(fixture);
    assert_eq!(events.len(), 1);
    (job, events.pop().unwrap())
}

fn load_actual(fixture: &mut ColdCustody, samples: &Path, original: &Path, id: usize) {
    let request = crate::audio_engine::cold_load::admit_for_format_selected(
        &fixture.engine,
        id,
        original.to_string_lossy().into_owned(),
        (
            false,
            false,
            true,
            Some(ResidentLoadHint {
                start_s: 32.0 / 48_000.0,
                end_s: 64.0 / 48_000.0,
                key_lock: false,
            }),
        ),
        fixture.producer.clone(),
        (2, 48_000, samples.to_path_buf()),
    )
    .unwrap();
    wait_until(|| {
        fixture
            .engine
            .input_runtime_ownership
            .cold_status(id, request)
            == Some(0)
            && fixture.consumer.slots() == 1
    });
    fixture.dispatch_one();
    assert_eq!(
        fixture
            .engine
            .input_runtime_ownership
            .cold_status(id, request),
        Some(2)
    );
    wait_until(|| fixture.engine.cold_loading[id].load(Ordering::Acquire) == 0);
    let sample = fixture.engine.sample_cache.lock().unwrap()[id]
        .clone()
        .unwrap();
    assert_eq!(sample.frame_count(), 128);
    assert_eq!((sample.resident_start(), sample.resident_end()), (32, 64));
    assert!(
        fixture
            .engine
            .input_runtime_ownership
            .source_current(id, &sample, 48_000)
    );
}

fn reopened_fixture(samples: &Path, original: &Path) -> ColdCustody {
    Python::initialize();
    let mut engine = AudioEngine::new().unwrap();
    assert!(engine.stream_handle.is_none());
    let (producer, consumer) = RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    let (_, feedback) = RingBuffer::<AudioMessage>::new(8);
    engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(feedback))));
    let mut mixer = Box::new(RtMixer::new(2, 48_000.0));
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    mixer.set_prepared_source_epochs(engine.prepared_source_epochs.clone());
    let mut fixture = ColdCustody {
        engine,
        directory: tempfile::tempdir().unwrap(),
        producer,
        consumer,
        mixer,
    };
    // The dummy directory owns no shared data. All actual source work uses the
    // parent's explicit owned samples root and the genuine cold admission/ACK.
    load_actual(&mut fixture, samples, original, 0);
    load_actual(&mut fixture, samples, original, 1);
    assert!(maps(&fixture).is_empty());
    fixture
}

fn claim_original(fixture: &ColdCustody) -> crate::audio_engine::project_assets::ProjectAssetLease {
    let original = original_path(fixture, 0);
    let lease = fixture
        .engine
        .project_assets
        .acquire(&samples_root(fixture), &original)
        .unwrap();
    lease
        .acknowledge(original.to_string_lossy().into_owned())
        .unwrap();
    lease
}

fn layout_fixture(layout: &str) -> ColdCustody {
    let mut fixture = ColdCustody::new();
    if layout == "readable" {
        return fixture;
    }
    let samples = samples_root(&fixture);
    let wav = fs::read(fixture.directory.path().join("external.wav")).unwrap();
    let original = if layout == "legacy" {
        samples.join("legacy.wav")
    } else {
        let digest = format!("{:x}", Sha256::digest(&wav));
        samples
            .join("materials")
            .join(format!("M{}", &digest[..32]))
            .join("original")
            .join("source.wav")
    };
    fs::create_dir_all(original.parent().unwrap()).unwrap();
    fs::write(&original, wav).unwrap();
    for id in [0, 1] {
        crate::audio_engine::cold_load::unload_for_producer(&fixture.engine, id, &fixture.producer)
            .unwrap();
        fixture.dispatch_one();
        load_actual(&mut fixture, &samples, &original, id);
    }
    assert!(maps(&fixture).is_empty());
    assert_eq!(
        original_path(&fixture, 0),
        fs::canonicalize(original).unwrap()
    );
    fixture
}

fn assert_portable_receipt(receipt: &Value, expected: &Value, path: &Path) {
    let mut keys: Vec<_> = receipt
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "bytes",
            "decoder_identity",
            "kind",
            "map_revision",
            "reference",
            "schema_version",
            "sha256"
        ]
    );
    assert_eq!(receipt["schema_version"], 1);
    assert_eq!(receipt["kind"], "source-beatmap-v1");
    assert_eq!(
        receipt["map_revision"],
        expected["record"]["accepted"]["map_revision"]
    );
    assert_eq!(
        receipt["decoder_identity"],
        expected["record"]["source"]["decoder_identity"]
    );
    let bytes = fs::read(path).unwrap();
    assert_eq!(receipt["bytes"], bytes.len() as u64);
    assert_eq!(receipt["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), *expected);
}

fn refuse_restore(fixture: &ColdCustody, receipt: &Value) {
    let fences = source_fences(fixture);
    match fixture
        .engine
        .begin_source_beatmap_restore_async(0, receipt.to_string())
    {
        Err(_) => {}
        Ok(job) => {
            let terminal = wait_terminal(&job);
            assert_eq!(terminal["stage"], "failed");
        }
    }
    wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    assert_offline_available(fixture);
    assert!(maps(fixture).is_empty());
    assert_eq!(source_fences(fixture), fences);
}

#[test]
fn real_store_and_fresh_engine_restore_use_each_explicit_typed_namespace() {
    for layout in ["legacy", "readable", "mid"] {
        let mut fixture = layout_fixture(layout);
        let expected = record(&fixture, 0);
        let fences = source_fences(&fixture);
        let assignment = claim_original(&fixture);
        let (job, event) = stored_event(&fixture, &expected);
        let owner = delivery(&event);
        let artifact = resolve_artifact(&job, &owner).unwrap();
        let receipt: Value = serde_json::from_str(artifact.receipt_json()).unwrap();
        let path = artifact_path(&fixture, &receipt);
        assert_portable_receipt(&receipt, &expected, &path);
        assert_eq!(artifact.reference(), receipt["reference"].as_str().unwrap());
        assert_eq!(artifact_record(&artifact), expected);
        let prefix = if layout == "legacy" {
            "samples/.pcm-cache/source-beatmaps/v1/".to_owned()
        } else {
            let source = original_path(&fixture, 0);
            let root = crate::audio_engine::material_paths::material_root_for_original(
                &samples_root(&fixture),
                &source,
            )
            .unwrap();
            format!(
                "samples/{}/.pcm-cache/source-beatmaps/v1/",
                root.strip_prefix(samples_root(&fixture))
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            )
        };
        assert!(
            artifact.reference().starts_with(&prefix),
            "{layout}: {}",
            artifact.reference()
        );
        assert_eq!(source_fences(&fixture), fences);
        assert_offline_available(&fixture);
        assert!(
            fs::OpenOptions::new().write(true).open(&path).is_err(),
            "output keeps actual sealed backing after lane retirement"
        );
        assert!(
            fixture.engine.project_assets.held_owner_count(&path) > 0,
            "output keeps the existing asset pin"
        );
        let samples = samples_root(&fixture);
        let original = original_path(&fixture, 0);
        drop(artifact);
        drop(owner);
        drop(event);
        drop(job);
        flush_python_drops();
        wait_output_retired(&fixture, &path);
        assert!(path.exists());
        fixture.engine.shut_down().unwrap();
        let ColdCustody {
            engine,
            directory,
            producer,
            consumer,
            mixer,
        } = fixture;
        drop(engine);
        drop(producer);
        drop(consumer);
        drop(mixer);
        drop(assignment);
        let mut restored = reopened_fixture(&samples, &original);
        let restore_fences = source_fences(&restored);
        let restore = restored
            .engine
            .begin_source_beatmap_restore_async(0, receipt.to_string())
            .unwrap();
        assert_eq!(wait_terminal(&restore)["stage"], "ready");
        let event = maps(&restored).pop().unwrap();
        let output = resolve_artifact(&restore, &delivery(&event)).unwrap();
        assert_eq!(artifact_record(&output), expected);
        assert_eq!(
            serde_json::from_str::<Value>(output.receipt_json()).unwrap(),
            receipt
        );
        assert_eq!(source_fences(&restored), restore_fences);
        assert_eq!(restored.engine.cold_jobs.counts_for_test(), (0, 0, 0));
        drop(output);
        drop(event);
        drop(restore);
        flush_python_drops();
        restored.engine.shut_down().unwrap();
        drop(restored);
        assert!(
            path.exists(),
            "a restored output never becomes an owned new birth"
        );
        drop(directory);
    }
}

const CHILD_ROOT: &str = "LOOPER_SOURCE_BEATMAP_ARTIFACT_CHILD_ROOT";

#[test]
fn fresh_process_restore_child() {
    let Some(root) = std::env::var_os(CHILD_ROOT) else {
        return;
    };
    let root = PathBuf::from(root);
    let original = PathBuf::from(fs::read_to_string(root.join("original-path.txt")).unwrap());
    let receipt_text = fs::read_to_string(root.join("receipt.json")).unwrap();
    let expected: Value =
        serde_json::from_slice(&fs::read(root.join("expected.json")).unwrap()).unwrap();
    let mut fixture = reopened_fixture(&root.join("samples"), &original);
    let job = fixture
        .engine
        .begin_source_beatmap_restore_async(0, receipt_text.clone())
        .unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    let event = maps(&fixture).pop().unwrap();
    let output = resolve_artifact(&job, &delivery(&event)).unwrap();
    assert_eq!(artifact_record(&output), expected);
    assert_eq!(
        serde_json::from_str::<Value>(output.receipt_json()).unwrap(),
        serde_json::from_str::<Value>(&receipt_text).unwrap()
    );
    let receipt: Value = serde_json::from_str(output.receipt_json()).unwrap();
    let path = artifact_path(&fixture, &receipt);
    drop(output);
    drop(event);
    drop(job);
    flush_python_drops();
    wait_output_retired(&fixture, &path);
    fixture.engine.shut_down().unwrap();
    drop(fixture);
    fs::write(
        root.join("child-restored.txt"),
        "genuine fresh-process cold source ACK and private artifact resolution\n",
    )
    .unwrap();
}

#[test]
fn resolved_store_survives_first_engine_exit_and_actual_separate_process_restore() {
    let mut fixture = ColdCustody::new();
    let assignment = claim_original(&fixture);
    let expected = record(&fixture, 0);
    let (job, event) = stored_event(&fixture, &expected);
    let output = resolve_artifact(&job, &delivery(&event)).unwrap();
    let receipt: Value = serde_json::from_str(output.receipt_json()).unwrap();
    let path = artifact_path(&fixture, &receipt);
    let root = fixture.directory.path().to_owned();
    fs::write(
        root.join("original-path.txt"),
        original_path(&fixture, 0).to_string_lossy().as_bytes(),
    )
    .unwrap();
    fs::write(root.join("receipt.json"), output.receipt_json()).unwrap();
    fs::write(root.join("expected.json"), expected.to_string()).unwrap();
    drop(output);
    drop(event);
    drop(job);
    flush_python_drops();
    wait_output_retired(&fixture, &path);
    fixture.engine.shut_down().unwrap();
    let ColdCustody {
        engine,
        directory,
        producer,
        consumer,
        mixer,
    } = fixture;
    drop(engine);
    drop(producer);
    drop(consumer);
    drop(mixer);
    drop(assignment);
    let child_name = format!(
        "{}::fresh_process_restore_child",
        module_path!().split_once("::").unwrap().1
    );
    let stdout = fs::File::create(root.join("child.stdout.txt")).unwrap();
    let stderr = fs::File::create(root.join("child.stderr.txt")).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &child_name, "--nocapture", "--test-threads=1"])
        .env(CHILD_ROOT, &root)
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "owned fresh-process restore exceeded 30 seconds; stdout={} stderr={}",
                fs::read_to_string(root.join("child.stdout.txt")).unwrap(),
                fs::read_to_string(root.join("child.stderr.txt")).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(
        status.success(),
        "stdout={} stderr={}",
        fs::read_to_string(root.join("child.stdout.txt")).unwrap(),
        fs::read_to_string(root.join("child.stderr.txt")).unwrap()
    );
    assert!(
        root.join("child-restored.txt").is_file(),
        "the exact child test actually ran"
    );
    assert_portable_receipt(&receipt, &expected, &path);
    drop(directory);
}

#[test]
fn receipt_schema_paths_and_all_declared_source_hash_fields_are_strict() {
    let fixture = ColdCustody::new();
    let expected = record(&fixture, 0);
    let (job, event) = stored_event(&fixture, &expected);
    let output = resolve_artifact(&job, &delivery(&event)).unwrap();
    let receipt: Value = serde_json::from_str(output.receipt_json()).unwrap();
    for (name, bad) in [
        ("schema_version", json!(true)),
        ("schema_version", json!(2)),
        ("kind", json!("original")),
        ("bytes", json!(0)),
        ("bytes", json!(true)),
        ("bytes", json!(64_u64 * 1024 * 1024 + 1)),
        ("sha256", json!("0".repeat(64))),
        ("map_revision", json!("0".repeat(64))),
        ("decoder_identity", json!("0".repeat(64))),
        ("reference", json!("samples/../foreign/map.json")),
        (
            "reference",
            json!(receipt["reference"].as_str().unwrap().replace('/', "\\")),
        ),
        (
            "reference",
            json!("samples/.pcm-cache/v1/.ready-00000000000000000000000000000000/map.json"),
        ),
        (
            "reference",
            json!(
                artifact_path(&fixture, &receipt)
                    .to_string_lossy()
                    .into_owned()
            ),
        ),
        (
            "reference",
            json!(
                receipt["reference"]
                    .as_str()
                    .unwrap()
                    .replace("/v1/", "//v1/")
            ),
        ),
    ] {
        let mut damaged = receipt.clone();
        damaged[name] = bad;
        refuse_restore(&fixture, &damaged);
    }
    for extra in ["file_id", "verified", "current", "record"] {
        let mut damaged = receipt.clone();
        damaged[extra] = json!(true);
        refuse_restore(&fixture, &damaged);
    }
    let mut missing = receipt.clone();
    missing.as_object_mut().unwrap().remove("sha256");
    refuse_restore(&fixture, &missing);
    assert!(artifact_path(&fixture, &receipt).exists());
}

#[test]
fn restored_files_are_never_new_births_when_hash_extent_or_actual_source_disagrees() {
    let mut fixture = ColdCustody::new();
    let expected = record(&fixture, 0);
    let (job, event) = stored_event(&fixture, &expected);
    let output = resolve_artifact(&job, &delivery(&event)).unwrap();
    let receipt: Value = serde_json::from_str(output.receipt_json()).unwrap();
    let path = artifact_path(&fixture, &receipt);
    let original = fs::read(&path).unwrap();
    drop(output);
    drop(event);
    drop(job);
    flush_python_drops();
    wait_output_retired(&fixture, &path);
    let mut changed = original.clone();
    changed[0] ^= 1;
    fs::write(&path, &changed).unwrap();
    refuse_restore(&fixture, &receipt);
    assert_eq!(fs::read(&path).unwrap(), changed);
    fs::write(&path, &original).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    refuse_restore(&fixture, &receipt);
    assert_eq!(fs::metadata(&path).unwrap().len(), 64 * 1024 * 1024 + 1);
    fs::write(&path, &original).unwrap();
    let mut source = fs::read(fixture.directory.path().join("external.wav")).unwrap();
    let last = source.len() - 1;
    source[last] ^= 1;
    let wrong = samples_root(&fixture).join("different.wav");
    fs::write(&wrong, source).unwrap();
    let samples = samples_root(&fixture);
    load_actual(&mut fixture, &samples, &wrong, 0);
    assert_ne!(
        manifest(&fixture, 0)["decoder_identity"],
        receipt["decoder_identity"]
    );
    refuse_restore(&fixture, &receipt);
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn geometry_only_resolution_never_acknowledges_an_unresolved_stored_birth() {
    let fixture = ColdCustody::new();
    let expected = record(&fixture, 0);
    let fences = source_fences(&fixture);
    let (job, event) = stored_event(&fixture, &expected);
    let receipt = receipt_from_event(&event);
    let path = artifact_path(&fixture, &receipt);
    let owner = delivery(&event);
    assert_eq!(resolved_record(&job, &owner), Some(expected));
    assert!(path.exists());
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    drop(owner);
    drop(event);
    drop(job);
    flush_python_drops();
    wait_until(|| !path.exists());
    assert_eq!(source_fences(&fixture), fences);
    assert_offline_available(&fixture);
}

#[test]
fn exact_artifact_resolution_ignores_dict_spoof_and_other_equal_counter_admissions() {
    let fixture = ColdCustody::new();
    let expected = record(&fixture, 0);
    let (first, first_event) = stored_event(&fixture, &expected);
    let first_receipt = receipt_from_event(&first_event);
    let first_path = artifact_path(&fixture, &first_receipt);
    let first_owner = delivery(&first_event);
    let (second, second_event) = stored_event(&fixture, &expected);
    let second_receipt = receipt_from_event(&second_event);
    let second_path = artifact_path(&fixture, &second_receipt);
    let second_owner = delivery(&second_event);
    assert_eq!(
        metadata(&first)["request_id"],
        metadata(&second)["request_id"]
    );
    assert_ne!(first_path, second_path);
    assert!(resolve_artifact(&first, &second_owner).is_none());
    assert!(resolve_artifact(&second, &first_owner).is_none());
    Python::attach(|py| {
        let first = first_event.bind(py).cast::<PyDict>().unwrap();
        let second = second_event.bind(py).cast::<PyDict>().unwrap();
        assert!(!first.contains("artifact").unwrap());
        first
            .set_item(
                "artifact_receipt",
                second.get_item("artifact_receipt").unwrap().unwrap(),
            )
            .unwrap();
        first
            .set_item("map", second.get_item("map").unwrap().unwrap())
            .unwrap();
        first.set_item("id", 215).unwrap();
        first.set_item("request_id", 0).unwrap();
    });
    let output = resolve_artifact(&first, &first_owner).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(output.receipt_json()).unwrap(),
        first_receipt
    );
    second.cancel();
    assert!(resolve_artifact(&second, &second_owner).is_none());
    drop(output);
    drop(first_owner);
    drop(second_owner);
    drop(first_event);
    drop(second_event);
    drop(first);
    drop(second);
    flush_python_drops();
    wait_until(|| !second_path.exists());
    assert!(
        first_path.exists(),
        "only the genuine original artifact was durably acknowledged"
    );
}

#[test]
fn queued_store_and_restore_cancel_or_drop_retain_the_original_shared_reservations() {
    for restore in [false, true] {
        for cancel in [false, true] {
            let fixture = ColdCustody::new();
            let expected = record(&fixture, 0);
            let receipt = if restore {
                let (job, event) = stored_event(&fixture, &expected);
                let output = resolve_artifact(&job, &delivery(&event)).unwrap();
                let receipt = serde_json::from_str::<Value>(output.receipt_json()).unwrap();
                let path = artifact_path(&fixture, &receipt);
                drop(output);
                drop(event);
                drop(job);
                flush_python_drops();
                wait_output_retired(&fixture, &path);
                Some(receipt)
            } else {
                assert!(maps(&fixture).is_empty());
                None
            };
            let before = ready_maps(&fixture);
            let fences = source_fences(&fixture);
            let held = occupy_workers(&fixture);
            let job = match &receipt {
                Some(receipt) => fixture
                    .engine
                    .begin_source_beatmap_restore_async(0, receipt.to_string()),
                None => fixture
                    .engine
                    .begin_source_beatmap_store_async(0, expected.to_string()),
            }
            .unwrap();
            assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (2, 1, 3));
            assert_eq!(async_status(&job)["stage"], "queued");
            if cancel {
                job.cancel();
                assert_eq!(async_status(&job)["terminal"], false);
                assert_eq!(async_status(&job)["cancellation_requested"], true);
                assert_offline_busy(&fixture);
                held.release();
                assert_eq!(wait_terminal(&job)["stage"], "cancelled");
            } else {
                drop(job);
                assert_offline_busy(&fixture);
                assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (2, 1, 3));
                held.release();
            }
            wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
            assert_offline_available(&fixture);
            assert!(maps(&fixture).is_empty());
            assert_eq!(ready_maps(&fixture), before);
            assert_eq!(source_fences(&fixture), fences);
        }
    }
}

#[test]
fn active_cancel_or_drop_retires_only_its_unacknowledged_prepared_birth() {
    for cancel in [false, true] {
        let fixture = ColdCustody::new();
        assert!(maps(&fixture).is_empty());
        let expected = record(&fixture, 0);
        let fences = source_fences(&fixture);
        let (job, held, path) = blocked_store(&fixture, &expected);
        assert!(path.is_file());
        assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
        assert!(
            fs::rename(
                path.parent().unwrap(),
                path.parent().unwrap().with_extension("foreign")
            )
            .is_err(),
            "actual output ancestor guards refuse replacement while preparation owns them"
        );
        if cancel {
            job.cancel();
            assert_eq!(async_status(&job)["terminal"], false);
            assert_offline_busy(&fixture);
            held.release();
            assert_eq!(wait_terminal(&job)["stage"], "cancelled");
        } else {
            drop(job);
            assert_offline_busy(&fixture);
            assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (1, 0, 1));
            held.release();
        }
        wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
        wait_output_retired(&fixture, &path);
        assert!(!path.exists());
        assert!(!path.parent().unwrap().exists());
        assert!(maps(&fixture).is_empty());
        assert_eq!(source_fences(&fixture), fences);
        assert_offline_available(&fixture);
    }
}

#[test]
fn unknown_children_survive_a_real_cancelled_prepared_birth_cleanup_attempt() {
    let fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let (job, held, path) = blocked_store(&fixture, &expected);
    let unknown = path.parent().unwrap().join("unknown-owner.txt");
    fs::write(&unknown, b"preserve the unrelated child").unwrap();
    job.cancel();
    held.release();
    assert_eq!(wait_terminal(&job)["stage"], "cancelled");
    wait_output_retired(&fixture, &path);
    assert_eq!(fs::read(&unknown).unwrap(), b"preserve the unrelated child");
    assert!(
        path.exists(),
        "unknown contents refuse the whole exact-birth cleanup"
    );
    assert!(maps(&fixture).is_empty());
}

#[test]
fn existing_other_logical_pins_retain_exact_unresolved_proof_until_actual_last_reader() {
    for pin_directory in [false, true] {
        let fixture = ColdCustody::new();
        let expected = record(&fixture, 0);
        let (job, event) = stored_event(&fixture, &expected);
        let receipt = receipt_from_event(&event);
        let path = artifact_path(&fixture, &receipt);
        let pinned = if pin_directory {
            path.parent().unwrap().to_path_buf()
        } else {
            path.clone()
        };
        let mut other = fixture
            .engine
            .project_assets
            .acquire_pin(&samples_root(&fixture), &pinned)
            .unwrap();
        assert_eq!(
            fixture.engine.project_assets.held_owner_count(&pinned),
            if pin_directory { 1 } else { 2 }
        );
        drop(event);
        drop(job);
        flush_python_drops();
        wait_until(|| {
            fixture.engine.project_assets.held_owner_count(&path)
                == if pin_directory { 0 } else { 1 }
        });
        assert!(
            path.exists(),
            "a genuine outside logical owner blocks birth rollback"
        );
        assert!(
            fs::OpenOptions::new().write(true).open(&path).is_ok(),
            "outside logical pin itself holds no sealed file reader"
        );
        other.release();
        assert_eq!(fixture.engine.project_assets.held_owner_count(&pinned), 0);
        wait_until(|| !path.parent().unwrap().exists());
        assert!(
            !path.parent().unwrap().exists(),
            "same queued proof retires the exact generation after last reader"
        );
    }
}

#[test]
#[ignore = "requires exclusive process-wide asset capacity; run this exact test alone"]
fn full_asset_retirement_queue_refuses_actual_worker_store_before_any_generation_birth() {
    let fixture = ColdCustody::new();
    let expected = record(&fixture, 0);
    let mut reservations = Vec::new();
    while let Ok(slot) = fixture.engine.project_assets.reserve_map_retirement() {
        reservations.push(slot);
    }
    assert!(!reservations.is_empty());
    let before = ready_maps(&fixture);
    let job = fixture
        .engine
        .begin_source_beatmap_store_async(0, expected.to_string())
        .unwrap();
    let status = wait_terminal(&job);
    assert_eq!(status["stage"], "failed");
    assert!(
        status["detail"]
            .as_str()
            .unwrap()
            .contains("retirement queue full")
    );
    assert_eq!(
        ready_maps(&fixture),
        before,
        "capacity refusal precedes generation creation"
    );
    assert!(maps(&fixture).is_empty());
    drop(reservations);
    assert_offline_available(&fixture);
}

#[test]
#[ignore = "requires exclusive process-wide owner capacity; run this exact test alone"]
fn acknowledged_unselected_artifact_uses_original_cleanup_proof_when_owner_registry_is_full() {
    let fixture = ColdCustody::new();
    let expected = record(&fixture, 0);
    let (job, event) = stored_event(&fixture, &expected);
    let artifact = resolve_artifact(&job, &delivery(&event)).unwrap();
    let receipt: Value = serde_json::from_str(artifact.receipt_json()).unwrap();
    let path = artifact_path(&fixture, &receipt);
    let mut pins = Vec::new();
    while let Ok(pin) = fixture
        .engine
        .project_assets
        .acquire_pin(&samples_root(&fixture), &path)
    {
        pins.push(pin);
    }
    assert!(!pins.is_empty());
    let refused = fixture
        .engine
        .project_assets
        .acquire(&samples_root(&fixture), &path)
        .unwrap_err();
    assert!(refused.to_string().contains("owner registry full"));
    artifact.retire_unselected();
    drop(artifact);
    drop(event);
    drop(job);
    flush_python_drops();
    wait_until(|| {
        fixture
            .engine
            .project_assets
            .map_retirement_pending_for_test(&path)
    });
    assert!(
        path.exists(),
        "actual independently acquired pins retain the exact queued birth"
    );
    drop(pins);
    wait_until(|| !path.parent().unwrap().exists());
    assert!(
        !path.exists(),
        "original reserved proof cleans up without another owner admission"
    );
}

#[test]
fn discarding_a_fresh_source_restored_artifact_never_creates_birth_deletion_rights() {
    let fixture = ColdCustody::new();
    let expected = record(&fixture, 0);
    let (job, event) = stored_event(&fixture, &expected);
    let artifact = resolve_artifact(&job, &delivery(&event)).unwrap();
    let encoded = artifact.receipt_json().to_owned();
    let receipt: Value = serde_json::from_str(&encoded).unwrap();
    let path = artifact_path(&fixture, &receipt);
    drop(artifact);
    drop(event);
    drop(job);
    flush_python_drops();
    wait_output_retired(&fixture, &path);
    assert!(path.exists());
    let job = fixture
        .engine
        .begin_source_beatmap_restore_async(0, encoded)
        .unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    let event = maps(&fixture).pop().unwrap();
    let restored = resolve_artifact(&job, &delivery(&event)).unwrap();
    restored.retire_unselected();
    drop(restored);
    drop(event);
    drop(job);
    flush_python_drops();
    wait_output_retired(&fixture, &path);
    assert!(
        path.exists(),
        "genuine restore owns readers, never the original export birth"
    );
}

#[test]
fn publication_failures_cleanup_only_exact_known_births_at_every_store_boundary() {
    for point in [
        StorePoint::AfterBirth,
        StorePoint::AfterWrite,
        StorePoint::BeforeRename,
        StorePoint::AfterRename,
    ] {
        let fixture = ColdCustody::new();
        assert!(maps(&fixture).is_empty());
        let expected = record(&fixture, 0);
        let observed = Arc::new(Mutex::new(None::<PathBuf>));
        let captured = observed.clone();
        let job = fixture
            .engine
            .begin_source_beatmap_store_async_with_hooks(
                0,
                expected.to_string(),
                || {},
                move |at, directory| {
                    if at == point {
                        *captured.lock().unwrap() = Some(directory.to_path_buf());
                        return Err(format!("injected storage failure at {point:?}"));
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(wait_terminal(&job)["stage"], "failed");
        assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (0, 0, 0));
        let directory = observed
            .lock()
            .unwrap()
            .clone()
            .expect("the actual named publication boundary ran");
        wait_until(|| !directory.exists());
        assert!(!directory.exists(), "known birth survives {point:?}");
        assert!(maps(&fixture).is_empty());
        assert_offline_available(&fixture);
    }
}

#[test]
fn changed_staging_bytes_or_leaf_identity_are_preserved_after_actual_reopen_refusal() {
    for replacement in [false, true] {
        let fixture = ColdCustody::new();
        assert!(maps(&fixture).is_empty());
        let expected = record(&fixture, 0);
        let observed = Arc::new(Mutex::new(None::<PathBuf>));
        let captured = observed.clone();
        let backup = fixture.directory.path().join("displaced-owned-map.json");
        let displaced = backup.clone();
        let job = fixture
            .engine
            .begin_source_beatmap_store_async_with_hooks(
                0,
                expected.to_string(),
                || {},
                move |point, directory| {
                    if point == StorePoint::AfterWrite {
                        *captured.lock().unwrap() = Some(directory.to_path_buf());
                        let leaf = directory.join("map.json");
                        if replacement {
                            fs::rename(&leaf, &displaced).map_err(|error| error.to_string())?;
                        }
                        fs::write(&leaf, b"changed fixture-owned bytes")
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(wait_terminal(&job)["stage"], "failed");
        let directory = observed.lock().unwrap().clone().unwrap();
        assert_eq!(
            fs::read(directory.join("map.json")).unwrap(),
            b"changed fixture-owned bytes"
        );
        if replacement {
            assert_eq!(
                serde_json::from_slice::<Value>(&fs::read(&backup).unwrap()).unwrap(),
                expected
            );
        }
        assert!(maps(&fixture).is_empty());
        assert_offline_available(&fixture);
    }
}

#[test]
fn artifact_ack_waits_for_real_pair_mirror_and_prior_cancel_never_acknowledges() {
    for cancel in [false, true] {
        let mut fixture = ColdCustody::new();
        let expected = record(&fixture, 0);
        let (job, event) = stored_event(&fixture, &expected);
        let receipt = receipt_from_event(&event);
        let path = artifact_path(&fixture, &receipt);
        let owner = delivery(&event);
        let ticket = own_pair(&mut fixture, 1);
        Python::attach(|py| {
            let error = job
                .resolve_source_beatmap_artifact_completion(&owner.borrow(py))
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("loader event content placement pending")
            );
        });
        if cancel {
            job.cancel();
        }
        assert!(pair::reconcile(&fixture.engine, &ticket).unwrap());
        if cancel {
            assert!(resolve_artifact(&job, &owner).is_none());
        } else {
            let output = resolve_artifact(&job, &owner).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(output.receipt_json()).unwrap(),
                receipt
            );
            assert_eq!(artifact_record(&output), expected);
            drop(output);
        }
        drop(owner);
        drop(event);
        drop(job);
        flush_python_drops();
        wait_output_retired(&fixture, &path);
        assert_eq!(path.exists(), !cancel);
    }
}

#[test]
fn source_supersession_after_polling_refuses_artifact_ack_and_retires_unresolved_birth() {
    let fixture = ColdCustody::new();
    let expected = record(&fixture, 0);
    let (job, event) = stored_event(&fixture, &expected);
    let receipt = receipt_from_event(&event);
    let path = artifact_path(&fixture, &receipt);
    let owner = delivery(&event);
    crate::audio_engine::next_pad_request_id(
        &fixture.engine.pad_request_ids,
        0,
        fixture.engine.prepared_source_epoch(0).unwrap(),
    )
    .unwrap();
    assert!(resolve_artifact(&job, &owner).is_none());
    drop(owner);
    drop(event);
    drop(job);
    flush_python_drops();
    wait_output_retired(&fixture, &path);
    assert!(!path.exists());
    assert_offline_available(&fixture);
}

#[test]
fn actual_engine_shutdown_waits_for_prepared_store_and_removes_only_unacknowledged_birth() {
    let mut fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let shutdown = fixture.engine.cold_cancelled.clone();
    let (job, path) = std::thread::scope(|scope| {
        // Unwind releases this actual worker gate before scoped shutdown joins.
        let (job, held, path) = blocked_store(&fixture, &expected);
        let (finished, receipt) = mpsc::channel();
        let engine = &mut fixture.engine;
        scope.spawn(move || {
            let _ = finished.send(engine.shut_down());
        });
        wait_until(|| shutdown.load(Ordering::Acquire));
        let status = async_status(&job);
        assert_eq!(status["cancellation_requested"], true);
        assert_eq!(status["terminal"], false);
        assert!(path.exists());
        assert!(matches!(receipt.try_recv(), Err(mpsc::TryRecvError::Empty)));
        held.release();
        receipt
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
        (job, path)
    });
    assert_eq!(wait_terminal(&job)["stage"], "cancelled");
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (0, 0, 0));
    assert!(!fixture.engine.offline_jobs.has_pad(0));
    // Cold-lane shutdown and physical retirement have distinct owners. The
    // original asset worker must actually finish the deferred exact cleanup.
    wait_output_retired(&fixture, &path);
    assert!(
        !path.exists(),
        "actual shutdown retired its unresolved map birth"
    );
    assert!(!path.parent().unwrap().exists());
    assert!(maps(&fixture).is_empty());
}
mod source_map_admission_custody_tests {
    include!("source_map_admission_custody_tests.rs");
}
