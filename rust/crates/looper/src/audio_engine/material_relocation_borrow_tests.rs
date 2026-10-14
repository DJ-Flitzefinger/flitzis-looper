//! An actual material birth survives an installer-owned physical Samples move.

use super::*;
use crate::audio_engine::{
    cold_store,
    material_migration::prepare_material_complete_for_test,
    material_migration_control as control,
    material_migration_journal::MaterialMigrationJournalStore,
    material_migration_recovery::{
        MigrationArtifactLease, MigrationProjectGuard, portable_relocate_data,
        resolve_migration_config_reference,
    },
    material_paths, project_assets,
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const RESIDENT_START: usize = 18_000;
const RESIDENT_END: usize = 26_000;
const BIRTH_PROCESS_ROOT: &str = "FLITZI_NATIVE_MATERIAL_RELOCATION_BIRTH_ROOT";
const BIRTH_PROCESS_TEST: &str = "audio_engine::constant_timing::native_borrow_tests::material_relocation_borrow_tests::public_native_background_borrow_material_restore_after_native_root_relocation";

#[derive(serde::Serialize, serde::Deserialize)]
struct FileProof {
    relative: PathBuf,
    identity: project_assets::FileIdentity,
    bytes: Option<Vec<u8>>,
}

fn tree_proofs(root: &Path) -> Vec<FileProof> {
    let mut pending = vec![root.to_owned()];
    let mut proofs = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path).unwrap();
            assert!(!metadata.is_symlink());
            assert!(metadata.is_file() || metadata.is_dir());
            if metadata.is_dir() {
                pending.push(path.clone());
            }
            proofs.push(FileProof {
                relative: path.strip_prefix(root).unwrap().to_owned(),
                identity: project_assets::capture_identity(&path).unwrap().unwrap(),
                bytes: metadata.is_file().then(|| std::fs::read(&path).unwrap()),
            });
        }
    }
    proofs
}

fn assert_proofs(root: &Path, proofs: &[FileProof], include_registration: bool, stage: &str) {
    for proof in proofs {
        if !include_registration && proof.relative.starts_with(".migration-projects") {
            continue;
        }
        let path = root.join(&proof.relative);
        assert_eq!(
            project_assets::capture_identity(&path).unwrap().as_ref(),
            Some(&proof.identity),
            "{stage} changed FileID: {}",
            proof.relative.display()
        );
        if let Some(expected) = &proof.bytes {
            assert_eq!(
                &std::fs::read(&path).unwrap(),
                expected,
                "{stage} changed original/PCM/config/journal bytes: {}",
                proof.relative.display()
            );
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedBirth {
    reference: String,
    independent_reference: String,
    creation: String,
    config_reference: String,
    instance: String,
    journal: String,
    transaction: String,
    complete_pcm: Vec<f32>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct BirthProcessReceipt {
    process_id: u32,
    saved: SavedBirth,
    proofs: Vec<FileProof>,
    samples_identity: project_assets::FileIdentity,
}

fn acquire_current_saved_originals(engine: &Py<AudioEngine>) -> Vec<Py<PyAny>> {
    Python::attach(|py| {
        let paths = {
            let borrowed = engine.borrow(py);
            let leases = borrowed.cold_leases.lock().unwrap();
            [0, 215].map(|id| {
                material_paths::ordinary_windows_path(&leases[id].as_ref().unwrap().original_path)
            })
        };
        paths
            .into_iter()
            .map(|path| {
                engine
                    .call_method1(py, "acquire_project_asset_lease", (path,))
                    .unwrap()
            })
            .collect()
    })
}

fn produce_saved_birth(directory: &Path) {
    assert_eq!(
        std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
        std::fs::canonicalize(directory).unwrap()
    );
    let (first, mut callback, _original, _envelope) = engine_at(directory);
    let source_root = std::fs::canonicalize(directory.join("samples")).unwrap();
    // This is the public saved-assignment admission used by AssetLifecycle's
    // persisted image. A config/inventory registration alone is no delivery ACK.
    let mut saved_owners = acquire_current_saved_originals(&first);
    let old_readers = Python::attach(|py| {
        let borrowed = first.borrow(py);
        let leases = borrowed.cold_leases.lock().unwrap();
        [0, 215].map(|id| {
            leases[id]
                .as_ref()
                .unwrap()
                .reader_lifetime_probe_for_test()
        })
    });
    let old_pcm = Python::attach(|py| {
        let borrowed = first.borrow(py);
        let cache = borrowed.sample_cache.lock().unwrap();
        [0, 215].map(|id| Arc::downgrade(&cache[id].as_ref().unwrap().samples))
    });
    let saved = saved_birth(&first, &mut callback, &source_root);
    saved_owners.extend(acquire_current_saved_originals(&first));
    let (material_reader, material_pcm) = Python::attach(|py| {
        let borrowed = first.borrow(py);
        (
            borrowed.cold_leases.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .reader_lifetime_probe_for_test(),
            Arc::downgrade(
                &borrowed.sample_cache.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .samples,
            ),
        )
    });
    let proofs = tree_proofs(&source_root);
    let samples_identity = project_assets::capture_identity(&source_root)
        .unwrap()
        .unwrap();
    // Ordinary saved shutdown releases real process owners without retiring the
    // persisted image. Its bounded file-only saved descriptors remain until the
    // actual process exits; they must not be faked away to make a move possible.
    Python::attach(|py| {
        for owner in saved_owners {
            owner.call_method0(py, "release").unwrap();
        }
    });
    drop(callback);
    Python::attach(|_| drop(first));
    wait_until(|| old_pcm.iter().all(|pcm| pcm.strong_count() == 0));
    assert_eq!(material_pcm.strong_count(), 0);
    assert!(old_readers.iter().all(|released| !released()));
    assert!(!material_reader());
    assert_proofs(
        &source_root,
        &proofs,
        true,
        "saved shutdown before actual process exit",
    );
    std::fs::write(
        directory.join("saved-birth-before-exit.json"),
        serde_json::to_vec(&BirthProcessReceipt {
            process_id: std::process::id(),
            saved,
            proofs,
            samples_identity,
        })
        .unwrap(),
    )
    .unwrap();
}

fn birth_in_actual_process(directory: &Path) -> BirthProcessReceipt {
    let output_path = directory.join("saved-birth-process.log");
    let output = std::fs::File::create(&output_path).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", BIRTH_PROCESS_TEST, "--nocapture"])
        .current_dir(directory)
        .env(BIRTH_PROCESS_ROOT, directory)
        .stdout(std::process::Stdio::from(output.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(output))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!(
                "owned material birth process timed out: {}",
                std::fs::read_to_string(output_path).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let output = std::fs::read_to_string(output_path).unwrap();
    assert!(
        status.success() && output.contains("1 passed"),
        "actual saved material birth process failed: {output}"
    );
    let receipt: BirthProcessReceipt = serde_json::from_slice(
        &std::fs::read(directory.join("saved-birth-before-exit.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt.process_id, child.id());
    assert_ne!(receipt.process_id, std::process::id());
    receipt
}

fn saved_birth(engine: &Py<AudioEngine>, callback: &mut Callback, root: &Path) -> SavedBirth {
    accept_current_source_timing(engine, callback);
    let input = root.join("material-input.wav");
    let original_bytes = Python::attach(|py| {
        std::fs::read(
            &engine.borrow(py).cold_leases.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .original_path,
        )
        .unwrap()
    });
    assert_eq!(&original_bytes[..4], b"RIFF");
    assert_eq!(&original_bytes[8..16], b"WAVEfmt ");
    assert_eq!(&original_bytes[36..40], b"data");
    // Keep independently generated quarter-pulse PCM exact, but give this
    // ordinary WAV its own bytes so content reuse cannot substitute for birth.
    let annotation = b"portable-native-material-birth-v1";
    let mut input_bytes = original_bytes[..36].to_vec();
    input_bytes.extend_from_slice(b"JUNK");
    input_bytes.extend_from_slice(&u32::try_from(annotation.len()).unwrap().to_le_bytes());
    input_bytes.extend_from_slice(annotation);
    if !annotation.len().is_multiple_of(2) {
        input_bytes.push(0);
    }
    input_bytes.extend_from_slice(&original_bytes[36..]);
    let extent = u32::try_from(input_bytes.len() - 8).unwrap();
    input_bytes[4..8].copy_from_slice(&extent.to_le_bytes());
    assert_ne!(input_bytes, original_bytes);
    std::fs::write(&input, &input_bytes).unwrap();
    let material = prepare_material_complete_for_test(root, &input, RATE, 1, &|| false).unwrap();
    let metadata = material.metadata();
    assert_eq!(metadata["created_original"], true);
    let creation = metadata["material_creation"].clone();
    assert!(!creation.is_null());
    let reference = metadata["new_reference"].as_str().unwrap().to_owned();
    let preparation = Python::attach(|py| {
        control::prepared_for_test(&engine.borrow(py), material, root).unwrap()
    });
    let source = Python::attach(|py| {
        let borrowed = engine.borrow(py);
        control::adopt_for_format(
            &borrowed,
            0,
            &preparation,
            borrowed.control_producer().unwrap().clone(),
            (1, RATE, root.to_owned()),
        )
        .unwrap()
    });
    wait_until(|| callback.consumer.peek().is_ok());
    assert_eq!(source.phase().unwrap(), "pending");
    assert!(!source.is_current().unwrap());
    assert_eq!(callback.drain(), 1);
    let mut acknowledged = false;
    wait_until(|| {
        Python::attach(|py| {
            let borrowed = engine.borrow(py);
            while let Ok(event) = borrowed
                .loader_rx
                .lock()
                .unwrap()
                .try_recv()
                .map(LoaderEvent::into_test_payload)
            {
                match event {
                    LoaderEvent::Success { request_id, .. }
                        if request_id == source.request_id() =>
                    {
                        acknowledged = true
                    }
                    LoaderEvent::Error { error, .. } => panic!("material birth failed: {error}"),
                    _ => {}
                }
            }
            acknowledged && borrowed.cold_loading[0].load(Ordering::Acquire) == 0
        })
    });
    assert!(source.is_current().unwrap());
    preparation.release_preparation().unwrap();
    accept_current_source_timing(engine, callback);
    callback.render();
    let (complete_pcm, independent_reference) = Python::attach(|py| {
        let borrowed = engine.borrow(py);
        let cache = borrowed.sample_cache.lock().unwrap();
        let current = cache[0].as_ref().unwrap();
        assert_eq!(
            (current.resident_start(), current.resident_end()),
            (RESIDENT_START, RESIDENT_END)
        );
        let expected_pcm: Vec<_> = tests::source()
            .samples
            .iter()
            .map(|value| value * 0.5)
            .collect();
        assert_eq!(
            current.samples.as_ref(),
            &expected_pcm[RESIDENT_START..RESIDENT_END]
        );
        let leases = borrowed.cold_leases.lock().unwrap();
        let complete = leases[0]
            .as_ref()
            .unwrap()
            .read_complete(current, crate::audio_engine::constant_timing::MAX_PCM_BYTES)
            .unwrap();
        assert_eq!(
            (complete.resident_start(), complete.resident_end()),
            (0, expected_pcm.len())
        );
        assert_eq!(complete.samples.as_ref(), expected_pcm.as_slice());
        assert_eq!(
            std::fs::read(&leases[0].as_ref().unwrap().original_path).unwrap(),
            input_bytes
        );
        let independent = Path::new("samples")
            .join(
                leases[215]
                    .as_ref()
                    .unwrap()
                    .original_path
                    .strip_prefix(root)
                    .unwrap(),
            )
            .to_string_lossy()
            .replace('\\', "/");
        (complete.samples.to_vec(), independent)
    });
    let evidence = Python::attach(|py| {
        let mut artifact = MigrationArtifactLease::capture(
            py,
            material_paths::ordinary_windows_path(root),
            reference.clone(),
        )
        .unwrap();
        let receipt: serde_json::Value =
            serde_json::from_str(&artifact.receipt_json().unwrap()).unwrap();
        assert_eq!(receipt["material_creation"], creation);
        artifact.release();
        receipt
    });
    let instance = "1".repeat(32);
    let transaction = "2".repeat(32);
    let alias = serde_json::json!({
        "new_reference":reference,
        "material_id":metadata["material_id"],
        "material_creation":creation,
    });
    let mut paths = vec![serde_json::Value::Null; NUM_SAMPLES];
    let mut content = vec![serde_json::Value::Null; NUM_SAMPLES];
    paths[0] = serde_json::json!(reference);
    paths[215] = serde_json::json!(independent_reference);
    content[0] = serde_json::json!({
        "instance_id":instance,
        "material_id":metadata["material_id"],
        "material_creation":creation,
    });
    let config = serde_json::json!({
        "config_revision":1,
        "sample_paths":paths,
        "pad_content":content,
        "material_migrations":{transaction.clone():alias},
    });
    let config_bytes = serde_json::to_vec(&config).unwrap();
    let config_path = root.join("flitzis_looper.config.json");
    let config_reference = material_paths::ordinary_windows_path(&config_path);
    std::fs::write(&config_path, &config_bytes).unwrap();
    let config_sha256 = format!("{:x}", Sha256::digest(&config_bytes));
    let journal = serde_json::json!({
        "schema_version":1,"transaction_id":transaction,"phase":"config_committed",
        "config_reference":config_reference,"captured_revision":0,"intent_revision":0,
        "config_sha256":config_sha256,"snapshot_json":config.to_string(),
        "assignments":[{"sample_id":0,"instance_id":instance,"old_reference":metadata["old_reference"]}],
        "alias":alias,"committed_revision":1,"committed_config_sha256":config_sha256,
        "artifacts":[{"role":"target","created":true,"evidence":evidence}],
        "resume_of":null,"cleanup_complete":false,"error":null,
    }).to_string();
    let store = MaterialMigrationJournalStore::new(
        material_paths::ordinary_windows_path(root),
        transaction.clone(),
    )
    .unwrap();
    store.append(journal.clone()).unwrap();
    assert_eq!(store.history().unwrap(), vec![journal.clone()]);
    Python::attach(|py| {
        let mut project = MigrationProjectGuard::new(
            py,
            material_paths::ordinary_windows_path(root),
            config_reference.clone(),
        )
        .unwrap();
        let mut inventory = project.lock_inventory(py).unwrap();
        assert_eq!(inventory.records(py).unwrap().len(), 1);
        inventory.release();
        project.release();
    });
    drop(store);
    drop(source);
    drop(preparation);
    SavedBirth {
        reference,
        independent_reference,
        creation: creation.to_string(),
        config_reference,
        instance,
        journal,
        transaction,
        complete_pcm,
    }
}

fn fresh_engine(root: &Path, saved: &SavedBirth) -> (Py<AudioEngine>, Callback, [u64; 2]) {
    let mut engine = AudioEngine::new().unwrap();
    let (producer, consumer) = tests::queue(16);
    let (feedback, feedback_consumer) = rtrb::RingBuffer::new(32);
    engine.test_control_channels =
        Some((producer.clone(), Arc::new(Mutex::new(feedback_consumer))));
    let requests = [&saved.reference, &saved.independent_reference]
        .into_iter()
        .zip([0, 215])
        .map(|(reference, id)| {
            let hint = (id == 0).then_some(ResidentLoadHint {
                start_s: 2.25,
                end_s: 3.25,
                key_lock: false,
            });
            admit_for_format_selected(
                &engine,
                id,
                reference.clone(),
                (false, id == 0, true, hint),
                producer.clone(),
                (1, RATE, root.to_owned()),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mixer = create_stream_mixer(
        1,
        RATE,
        engine.current_timing_acknowledgements.clone(),
        engine.input_runtime_ownership.clone(),
        engine.prepared_source_epochs.clone(),
    )
    .unwrap();
    let callback = Callback {
        mixer,
        scheduler: FixedCapacityScheduler::new(),
        transport: TransportTimeline::new(RATE),
        quantization: TriggerQuantization::Immediate,
        consumer,
        feedback,
        retirement: ImmediateAudioBufferRetirement,
        independent: SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from([0.25; 64]),
        },
        consumed: 0,
    };
    (
        Python::attach(|py| Py::new(py, engine).unwrap()),
        callback,
        requests.try_into().unwrap(),
    )
}

#[test]
fn public_native_background_borrow_material_restore_after_native_root_relocation() {
    if let Some(directory) = std::env::var_os(BIRTH_PROCESS_ROOT) {
        produce_saved_birth(Path::new(&directory));
        return;
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let directory = tempfile::tempdir_in(workspace.join("scratch")).unwrap();
    // No Engine is created in the installer process before the genuine producing
    // process exits and closes every old saved/native file handle.
    let BirthProcessReceipt {
        saved,
        proofs,
        samples_identity,
        ..
    } = birth_in_actual_process(directory.path());
    let source_root = std::fs::canonicalize(directory.path().join("samples")).unwrap();
    assert_proofs(&source_root, &proofs, true, "actual producing process exit");
    Python::initialize();
    std::fs::write(
        directory.path().join("portable-root.json"),
        br#"{"schema_version":1,"app":"flitzis-looper"}"#,
    )
    .unwrap();
    std::fs::create_dir(directory.path().join("cache")).unwrap();
    std::fs::create_dir(directory.path().join("flitzis-looper")).unwrap();
    let target_root = directory.path().join("flitzis-looper/samples");
    let record = directory
        .path()
        .join("cache")
        .join(format!("relocation-{}.json", "3".repeat(32)));
    // The pre-exit exact tree above includes every saved original/cache. Capture
    // every actual remaining file/directory immediately before Native capture.
    let proofs = tree_proofs(&source_root);
    Python::attach(|py| {
        let mover = pyo3::wrap_pyfunction!(portable_relocate_data, py).unwrap();
        for (action, phase) in [("capture", "captured"), ("move", "moved")] {
            let encoded = mover
                .call1((
                    material_paths::ordinary_windows_path(directory.path()),
                    material_paths::ordinary_windows_path(&source_root),
                    material_paths::ordinary_windows_path(&target_root),
                    material_paths::ordinary_windows_path(&record),
                    action,
                ))
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&encoded).unwrap()["phase"],
                phase
            );
        }
    });
    assert!(!source_root.exists());
    assert_eq!(
        project_assets::capture_identity(&target_root)
            .unwrap()
            .as_ref(),
        Some(&samples_identity)
    );
    assert_proofs(&target_root, &proofs, true, "immediate Native move");
    let target_root = std::fs::canonicalize(target_root).unwrap();
    let material_original = material_paths::resolve(&target_root, Path::new(&saved.reference))
        .unwrap()
        .path;
    let material_root = material_paths::material_root(&material_original).unwrap();
    assert!(
        cold_store::known_material_creation_for_test(material_root)
            .unwrap()
            .is_none()
    );
    Python::attach(|py| {
        let resolver = pyo3::wrap_pyfunction!(resolve_migration_config_reference, py).unwrap();
        assert_eq!(
            resolver
                .call1((
                    material_paths::ordinary_windows_path(&target_root),
                    saved.config_reference.clone()
                ))
                .unwrap()
                .extract::<String>()
                .unwrap(),
            material_paths::ordinary_windows_path(&target_root.join("flitzis_looper.config.json"))
        );
    });
    let (fresh, mut callback, requests) = fresh_engine(&target_root, &saved);
    wait_until(|| callback.consumer.slots() == 2);
    let (mut project, inventory) = Python::attach(|py| {
        let project = MigrationProjectGuard::new(
            py,
            material_paths::ordinary_windows_path(&target_root),
            saved.config_reference.clone(),
        )
        .unwrap();
        let inventory = Py::new(py, project.lock_inventory(py).unwrap()).unwrap();
        (project, inventory)
    });
    let arguments = Python::attach(|py| {
        (
            0,
            saved.creation.clone(),
            saved.config_reference.clone(),
            saved.instance.clone(),
            inventory.clone_ref(py),
            Some(saved.journal.clone()),
        )
            .into_pyobject(py)
            .unwrap()
            .unbind()
    });
    Python::attach(|py| {
        let borrowed = fresh.borrow(py);
        let current = borrowed.sample_cache.lock().unwrap()[0].clone().unwrap();
        assert_eq!(
            borrowed.cold_loading[0].load(Ordering::Acquire),
            requests[0]
        );
        assert_eq!(
            borrowed
                .input_runtime_ownership
                .source_generation(0, &current, RATE),
            Some(requests[0])
        );
        assert_eq!(
            borrowed.input_runtime_ownership.cold_status(0, requests[0]),
            Some(0)
        );
        assert!(borrowed.cold_leases.lock().unwrap()[0].is_none());
        drop(borrowed);
        let error = fresh
            .call_method1(py, "restore_material_creation", arguments.clone_ref(py))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("requires this process's actual current source ACK"),
            "pending own-source ACK must reject material restoration: {error}"
        );
    });
    assert_eq!(callback.consumer.slots(), 2);
    assert!(
        cold_store::known_material_creation_for_test(material_root)
            .unwrap()
            .is_none()
    );
    assert_eq!(callback.drain(), 2);
    let mut success = [false; 2];
    wait_until(|| {
        Python::attach(|py| {
            let borrowed = fresh.borrow(py);
            while let Ok(event) = borrowed
                .loader_rx
                .lock()
                .unwrap()
                .try_recv()
                .map(LoaderEvent::into_test_payload)
            {
                match event {
                    LoaderEvent::Success { id, request_id, .. } => {
                        let index = if id == 0 {
                            0
                        } else {
                            assert_eq!(id, 215);
                            1
                        };
                        assert_eq!(request_id, requests[index]);
                        success[index] = true;
                    }
                    LoaderEvent::Error { error, .. } => {
                        panic!("moved-source own load failed: {error}")
                    }
                    _ => {}
                }
            }
            success == [true; 2]
                && [0, 215]
                    .into_iter()
                    .all(|id| borrowed.cold_loading[id].load(Ordering::Acquire) == 0)
        })
    });
    Python::attach(|py| {
        let borrowed = fresh.borrow(py);
        let cache = borrowed.sample_cache.lock().unwrap();
        let current = cache[0].as_ref().unwrap();
        assert_eq!(
            current.samples.as_ref(),
            &saved.complete_pcm[RESIDENT_START..RESIDENT_END]
        );
        assert_eq!(
            (current.resident_start(), current.resident_end()),
            (RESIDENT_START, RESIDENT_END)
        );
        let leases = borrowed.cold_leases.lock().unwrap();
        let complete = leases[0]
            .as_ref()
            .unwrap()
            .read_complete(current, crate::audio_engine::constant_timing::MAX_PCM_BYTES)
            .unwrap();
        assert_eq!(
            (complete.resident_start(), complete.resident_end()),
            (0, saved.complete_pcm.len())
        );
        assert_eq!(complete.samples.as_ref(), saved.complete_pcm.as_slice());
        assert_eq!(
            borrowed
                .input_runtime_ownership
                .source_generation(0, current, RATE),
            Some(requests[0])
        );
        assert!(
            leases[0]
                .as_ref()
                .unwrap()
                .material_creation_receipt()
                .is_none()
        );
        callback.independent = cache[215].as_ref().unwrap().clone();
        assert_eq!(callback.independent.samples.as_ref(), &[0.25; 64]);
        assert_eq!(
            borrowed.timing_intents.lock().unwrap()[215],
            TimingIntent::Legacy
        );
        for (id, request) in [0, 215].into_iter().zip(requests) {
            assert_eq!(
                borrowed.input_runtime_ownership.cold_status(id, request),
                Some(2)
            );
        }
    });
    let fresh_stops = [0, 215].map(|id| OriginalStopFeedback::capture(&fresh, id));
    assert!(callback.mixer.can_play_sample(215, 1.0));
    assert!(callback.mixer.play_sample(215, 1.0));
    callback.render();
    let timing_request = accept_current_source_timing(&fresh, &mut callback);
    // Both actual replacement LoadColdSample ACKs publish owning STOP feedback even
    // without an old voice. Reconcile those public events before the held-call
    // oracle requires an otherwise empty feedback FIFO and its own exact Pong.
    Python::attach(|py| {
        let mut stopped = Vec::with_capacity(2);
        for index in 0..2 {
            let message = fresh.call_method0(py, "receive_msg").unwrap();
            assert!(
                !message.is_none(py),
                "fresh own-source ACK feedback {index} missing; received={stopped:?}"
            );
            let event = message.extract::<PyRef<AudioMessage>>(py).unwrap();
            let original = fresh_stops
                .iter()
                .find(|original| event.sample_id() == Some(original.id))
                .unwrap_or_else(|| panic!(
                    "unexpected fresh own-source ACK feedback {index}: {:?}; received={stopped:?}",
                    &*event,
                ));
            original.assert_message(&fresh.borrow(py), &event, Some(original.id));
            stopped.push(original.id);
        }
        let received = stopped.clone();
        stopped.sort_unstable();
        assert_eq!(stopped, [0, 215], "actual ACK feedback order={received:?}");
        let trailing = fresh.call_method0(py, "receive_msg").unwrap();
        assert!(
            trailing.is_none(py),
            "fresh own-source/timing ACKs left unexpected public feedback: {:?}",
            trailing.extract::<PyRef<AudioMessage>>(py).unwrap()
        );
    });
    let result = held_call(
        &fresh,
        &mut callback,
        "restore_material_creation",
        arguments,
        WorkPoint::MaterialRestoreIo,
        |_, callback| callback.render(),
    );
    assert!(Python::attach(|py| result.unwrap().is_none(py)));
    assert!(callback.consumer.peek().is_err());
    Python::attach(|py| {
        let borrowed = fresh.borrow(py);
        let leases = borrowed.cold_leases.lock().unwrap();
        assert_eq!(
            serde_json::to_value(
                leases[0]
                    .as_ref()
                    .unwrap()
                    .material_creation_receipt()
                    .unwrap()
            )
            .unwrap()
            .to_string(),
            saved.creation
        );
        let cache = borrowed.sample_cache.lock().unwrap();
        assert_eq!(
            borrowed
                .input_runtime_ownership
                .source_generation(0, cache[0].as_ref().unwrap(), RATE),
            Some(requests[0])
        );
        assert_eq!(
            borrowed.loaded_source_generations.lock().unwrap()[0],
            (requests[0], RATE)
        );
        assert_eq!(borrowed.pad_request_ids.lock().unwrap()[0], timing_request);
    });
    assert!(
        cold_store::known_material_creation_for_test(material_root)
            .unwrap()
            .is_some()
    );
    let journal = MaterialMigrationJournalStore::open(
        material_paths::ordinary_windows_path(&target_root),
        saved.transaction,
    )
    .unwrap();
    assert_eq!(journal.history().unwrap(), vec![saved.journal]);
    assert_proofs(
        &target_root,
        &proofs,
        false,
        "fresh source ACK/material restore",
    );
    callback.render();
    Python::attach(|py| inventory.borrow_mut(py).release());
    project.release();
}
