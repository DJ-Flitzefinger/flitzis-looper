//! Equal-byte import reuse across a genuine process boundary, through real cold
//! admission and callback adoption. No source, lease, ticket or PCM is inherited.
use super::*;
use std::collections::BTreeMap;
use std::io::Read;

const REUSE_ROOT: &str = "FLITZI_READABLE_MATERIAL_REUSE_ROOT";
const REUSE_CHILD: &str = "audio_engine::cold_load::tests::material_migration_control_tests::material_migration_reuse_process_tests::readable_equal_bytes_new_process_child_imports_different_name_with_own_window_ack";
const FIRST_NAME: &str = "Mélodie first.wav";
const SECOND_NAME: &str = "Different incoming name.wav";

fn file_witness(path: &Path) -> serde_json::Value {
    let mut file = fs::File::open(path).unwrap();
    let expected_bytes = file.metadata().unwrap().len();
    let mut bytes = 0_u64;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        bytes += count as u64;
    }
    assert_eq!(bytes, expected_bytes);
    assert_eq!(file.read(&mut buffer).unwrap(), 0);
    serde_json::json!({"path":path, "bytes":bytes,
        "sha256":format!("{:x}",digest.finalize()), "eof_verified":true})
}

fn material_files(root: &Path) -> BTreeMap<String, serde_json::Value> {
    fn visit(root: &Path, directory: &Path, output: &mut BTreeMap<String, serde_json::Value>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            assert!(!kind.is_symlink());
            if kind.is_dir() {
                visit(root, &path, output);
            } else {
                assert!(kind.is_file());
                let mut witness = file_witness(&path);
                witness.as_object_mut().unwrap().remove("path");
                output.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    witness,
                );
            }
        }
    }
    let mut output = BTreeMap::new();
    visit(root, root, &mut output);
    output
}

fn clear_survivor_project(root: &Path) -> serde_json::Value {
    use pyo3::{prelude::PyAnyMethods, types::PyDictMethods};
    pyo3::Python::attach(|py| {
        let locals = migration_python_locals(py, root);
        let code=std::ffi::CString::new(r#"
import sys,json,os,hashlib
from pathlib import Path
from types import SimpleNamespace
sys.path.insert(0,source_modules)
from flitzis_looper.models import ProjectState
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.material_migration import MaterialMigrationController
config=Path(project_root)/'project.config.json'
before=ProjectState.model_validate_json(config.read_bytes())
assert before.sample_paths[0] is None and before.pad_content[0] is None
content=before.pad_content[215]
assert content is not None and content.instance_id=='f'*32 and content.material_creation is not None
assert list(before.material_migrations)==['6'*32]
previous_cwd=Path.cwd()
try:
    os.chdir(project_root)
    selected=MaterialMigrationController._saved_creation_journal(SimpleNamespace(_project=before),215,content)
    assert selected is None
finally:
    os.chdir(previous_cwd)
before.sample_paths[215]=None
before.pad_content[215]=None
writer=ProjectPersistence(before)
writer.config_path=config
writer.mark_dirty()
writer.flush()
current=ProjectState.model_validate_json(config.read_bytes())
assert current.sample_paths[215] is None and current.pad_content[215] is None
clear_json=json.dumps({'saved_survivor_instance':content.instance_id,'retained_origin_alias':'6'*32,'own_journal_selected':False,'saved_config':str(config),'saved_config_bytes':config.stat().st_size,'saved_config_sha256':hashlib.sha256(config.read_bytes()).hexdigest()})
"#).unwrap();
        py.run(&code, Some(&locals), None).unwrap();
        let encoded: String = locals
            .get_item("clear_json")
            .unwrap()
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    })
}

fn assert_survivor_registration(root: &Path, blocked: bool) {
    pyo3::Python::attach(|py| {
        let result = crate::audio_engine::material_migration_recovery::MigrationProjectGuard::new(
            py,
            root.to_string_lossy().into(),
            root.parent()
                .unwrap()
                .join("project.config.json")
                .to_string_lossy()
                .into(),
        );
        if blocked {
            assert!(
                result.is_err(),
                "actual queued root Inventory must deny registration"
            );
        } else {
            result.unwrap().release();
        }
    });
}

fn source_bytes() -> Vec<u8> {
    let frames = 64_000_u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + frames * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8_000_u32.to_le_bytes());
    bytes.extend_from_slice(&16_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(frames * 2).to_le_bytes());
    for frame in 0..frames {
        bytes.extend_from_slice(&(((frame % 251) as i16 - 125) * 200).to_le_bytes());
    }
    bytes
}

fn save_regular_creation(
    root: &Path,
    reference: &str,
    manifest: &serde_json::Value,
    material: &serde_json::Value,
    artifacts: &serde_json::Value,
) {
    use pyo3::types::PyDictMethods;
    pyo3::Python::attach(|py| {
        let locals = migration_python_locals(py, root);
        locals.set_item("reference", reference).unwrap();
        locals
            .set_item("manifest_json", manifest.to_string())
            .unwrap();
        locals
            .set_item("material_json", material.to_string())
            .unwrap();
        locals
            .set_item("artifacts_json", artifacts.to_string())
            .unwrap();
        let code = std::ffi::CString::new(r#"
import sys,json
from pathlib import Path
sys.path.insert(0,source_modules)
from flitzis_looper.models import ProjectState,PadContentIdentity
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.material_migration_model import MaterialMigrationAlias,MaterialMigrationJournal,MigrationAssignment
from flitzis_looper_audio import MaterialMigrationJournalStore
manifest=json.loads(manifest_json)
material=json.loads(material_json)
transaction='6'*32
digest=material['original']['sha256']
alias=MaterialMigrationAlias(transaction_id=transaction,material_id=material['material_id'],old_reference=material['old_reference'],new_reference=material['new_reference'],original_sha256=digest,original_bytes=material['original']['bytes'],decoder_identity=material['decoder_identity'],playback_identity=material['playback_identity'],cache_path=material['cache_path'],old_source_version=f"{material['old_reference']}|sha256-v1:{digest}",new_source_version=f"{material['new_reference']}|sha256-v1:{digest}",material_creation=material['material_creation'])
project=ProjectState()
project.sample_paths[0]=alias.old_reference
project.pad_content[0]=PadContentIdentity(instance_id='1'*32)
persistence=ProjectPersistence(project)
persistence.config_path=Path(project_root)/'project.config.json'
persistence.flush()
captured,snapshot,previous_sha=persistence.capture_migration(transaction)
persistence.mark_dirty()
candidate=project.model_copy(deep=True)
candidate.sample_paths[0]=alias.new_reference
candidate.pad_content[0]=PadContentIdentity(instance_id='1'*32,material_id=alias.material_id,material_creation=alias.material_creation)
candidate.material_migrations[transaction]=alias
revision,config_sha=persistence.commit_migration(transaction,persistence.revision,candidate)
journal=MaterialMigrationJournal(transaction_id=transaction,phase='config_committed',config_reference=persistence.config_reference,captured_revision=captured,intent_revision=revision,config_sha256=previous_sha,snapshot_json=snapshot.model_dump_json(),committed_revision=revision,committed_config_sha256=config_sha,alias=alias,assignments=(MigrationAssignment(sample_id=0,instance_id='1'*32,old_reference=alias.old_reference),),artifacts=json.loads(artifacts_json))
MaterialMigrationJournalStore(str(Path(project_root)/'samples'),transaction).append(journal.model_dump_json())
persistence.release_migration(transaction)
project=ProjectState.model_validate_json(persistence.config_path.read_bytes())
project.sample_paths[0]=None
project.pad_content[0]=None
project.sample_paths[215]=reference
project.pad_content[215]=PadContentIdentity(instance_id='f'*32,material_id=manifest['material_id'],material_creation=manifest['material_creation'])
persistence=ProjectPersistence(project)
persistence.config_path=Path(project_root)/'project.config.json'
persistence.mark_dirty()
persistence.flush()
saved=ProjectState.model_validate_json(persistence.config_path.read_bytes())
assert saved.pad_content[215].material_creation is not None
assert saved.sample_paths[0] is None and saved.pad_content[0] is None
assert saved.material_migrations[transaction] == alias
assert [item.sample_id for item in journal.assignments] == [0]
"#).unwrap();
        py.run(&code, Some(&locals), None).unwrap();
    });
}

fn import_with_window(
    engine: &AudioEngine,
    id: usize,
    incoming: &Path,
    root: &Path,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    start_s: f64,
    end_s: f64,
) -> (u64, Arc<AtomicU8>) {
    let admitted = admit_selected_material(
        engine,
        id,
        incoming.to_string_lossy().into(),
        (
            false,
            false,
            false,
            Some(crate::audio_engine::cold_residency::ResidentLoadHint {
                start_s,
                end_s,
                key_lock: false,
            }),
        ),
        producer,
        (1, 8_000, root.to_path_buf()),
        "import",
        None,
    )
    .unwrap();
    assert!(admitted.old_source.is_none());
    assert!(admitted.old_lease.is_none());
    (admitted.request_id, admitted.adoption)
}

fn success_and_stages(engine: &AudioEngine, id: usize, request: u64) -> (String, Vec<String>) {
    let mut stages = Vec::new();
    let mut completed = None;
    wait_until(|| {
        while let Ok(event) = engine.loader_rx.lock().unwrap().try_recv() {
            match event {
                LoaderEvent::Progress {
                    id: found_id,
                    request_id,
                    stage,
                    ..
                } => {
                    assert_eq!((found_id, request_id), (id, request));
                    stages.push(stage);
                }
                LoaderEvent::Success {
                    id: found_id,
                    request_id,
                    cached_path,
                    analysis,
                    analysis_error,
                    ..
                } => {
                    assert_eq!((found_id, request_id), (id, request));
                    assert!(analysis.is_none());
                    assert!(analysis_error.is_none());
                    assert!(completed.replace(cached_path).is_none());
                }
                LoaderEvent::Error { error, .. } => panic!("productive import failed: {error}"),
                LoaderEvent::Started {
                    id: found_id,
                    request_id,
                } => assert_eq!((found_id, request_id), (id, request)),
                event => panic!("unexpected offline work in import-only proof: {event:?}"),
            }
        }
        completed.is_some()
    });
    wait_until(|| engine.cold_loading[id].load(Ordering::Acquire) == 0);
    (completed.unwrap(), stages)
}

#[test]
fn readable_equal_bytes_new_process_import_reuses_first_root_and_original_leaf_without_copy_or_decode()
 {
    pyo3::Python::initialize();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let scratch = workspace.join("scratch/goal-readable-material-layout-20261011");
    fs::create_dir_all(&scratch).unwrap();
    let directory = tempfile::tempdir_in(&scratch).unwrap();
    let root = directory.path().join("samples");
    fs::create_dir(&root).unwrap();
    let first = directory.path().join(FIRST_NAME);
    let second = directory.path().join(SECOND_NAME);
    let bytes = source_bytes();
    fs::write(&first, &bytes).unwrap();
    fs::write(&second, &bytes).unwrap();
    let manifest;
    let reference;
    let migration_metadata;
    let artifacts;
    let mut saved_original_owner;
    let parent_assets;
    {
        let engine = AudioEngine::new().unwrap();
        parent_assets = engine.project_assets.clone();
        let mut callback = migration_callback_8000(&engine);
        let (producer, mut consumer) = rtrb::RingBuffer::new(4);
        let producer = Arc::new(Mutex::new(producer));
        let legacy = root.join(FIRST_NAME);
        fs::write(&legacy, &bytes).unwrap();
        let material = crate::audio_engine::material_migration::prepare_material(
            &root,
            &legacy,
            8_000,
            1,
            &|| false,
        )
        .unwrap();
        migration_metadata = material.metadata();
        assert_eq!(migration_metadata["created_original"], true);
        let preparation = control::prepared_for_test(&engine, material, &root).unwrap();
        let source = control::adopt_for_format(
            &engine,
            0,
            &preparation,
            producer.clone(),
            (1, 8_000, root.clone()),
        )
        .unwrap();
        wait_until(|| consumer.peek().is_ok());
        assert_eq!(source.phase().unwrap(), "pending");
        assert_eq!(callback.drain(&mut consumer), 1);
        (reference, _) = success_and_stages(&engine, 0, source.request_id());
        assert_eq!(source.phase().unwrap(), "acknowledged");
        assert!(source.is_current().unwrap());
        let creator_manifest = serde_json::from_str::<serde_json::Value>(
            &engine.cold_source_manifest(0).unwrap().unwrap(),
        )
        .unwrap();
        let (request, adoption) =
            import_with_window(&engine, 215, &second, &root, producer.clone(), 2.5, 3.5);
        wait_until(|| consumer.peek().is_ok());
        assert_eq!(callback.drain(&mut consumer), 1);
        let (follower_reference, _) = success_and_stages(&engine, 215, request);
        assert_eq!(adoption.load(Ordering::Acquire), 2);
        assert_eq!(follower_reference, reference);
        manifest = serde_json::from_str::<serde_json::Value>(
            &engine.cold_source_manifest(215).unwrap().unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["integrity"]["warm"], true);
        assert_eq!(
            manifest["integrity"]["incoming_verify_bytes"],
            bytes.len() as u64
        );
        assert_eq!(manifest["integrity"]["original_copied_bytes"], 0);
        assert_eq!(
            manifest["material_creation"],
            creator_manifest["material_creation"]
        );
        assert!(!manifest["material_creation"].is_null());
        {
            let leases = engine.cold_leases.lock().unwrap();
            let follower = leases[215].as_ref().unwrap();
            assert!(!follower.created_original());
            assert!(!follower.created_cache());
        }
        artifacts = pyo3::Python::attach(|py| {
            let mut records = Vec::new();
            for (field, created) in [
                ("new_reference", "created_original"),
                ("cache_path", "created_cache"),
            ] {
                let mut lease=crate::audio_engine::material_migration_recovery::MigrationArtifactLease::capture(py,root.to_string_lossy().into(),migration_metadata[field].as_str().unwrap().into()).unwrap();
                records.push(serde_json::json!({"role":"target","created":migration_metadata[created],"evidence":serde_json::from_str::<serde_json::Value>(&lease.receipt_json().unwrap()).unwrap()}));
                lease.release();
            }
            serde_json::json!(records)
        });
        let original = crate::audio_engine::material_paths::resolve(&root, Path::new(&reference))
            .unwrap()
            .path;
        saved_original_owner = Some(engine.project_assets.acquire(&root, &original).unwrap());
    }
    let canonical = crate::audio_engine::material_paths::resolve(&root, Path::new(&reference))
        .unwrap()
        .path;
    assert_eq!(canonical.file_name().unwrap(), FIRST_NAME);
    assert!(crate::audio_engine::material_paths::readable_original(&root, &canonical).unwrap());
    let first_root =
        crate::audio_engine::material_paths::material_root_for_original(&root, &canonical).unwrap();
    assert_eq!(
        crate::audio_engine::material_paths::read_binding(&first_root)
            .unwrap()
            .original_leaf,
        FIRST_NAME
    );
    let before = material_files(&first_root);
    save_regular_creation(
        &root,
        &reference,
        &manifest,
        &migration_metadata,
        &artifacts,
    );
    let seed = serde_json::json!({"first_reference":reference, "first_root":first_root,
        "first_original":file_witness(&canonical), "first_incoming":file_witness(&first),
        "second_incoming":file_witness(&second), "first_manifest":manifest, "material_files":before});
    fs::write(
        directory.path().join("reuse-input.json"),
        serde_json::to_vec(&seed).unwrap(),
    )
    .unwrap();
    let output_path = directory.path().join("reuse-child-output.txt");
    let output = fs::File::create(&output_path).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", REUSE_CHILD, "--nocapture"])
        .env(REUSE_ROOT, directory.path())
        .stdout(std::process::Stdio::from(output.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(output))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut released_parent_owner = false;
    let status = loop {
        if !released_parent_owner
            && directory
                .path()
                .join("reuse-child-intent-cleared.json")
                .exists()
        {
            let current: serde_json::Value = serde_json::from_slice(
                &fs::read(directory.path().join("project.config.json")).unwrap(),
            )
            .unwrap();
            assert!(current["sample_paths"][0].is_null());
            assert!(current["sample_paths"][215].is_null());
            assert!(current["pad_content"][215].is_null());
            drop(saved_original_owner.take());
            parent_assets.retire(&root, &canonical, false).unwrap();
            parent_assets.collect_for_test();
            fs::write(
                directory.path().join("reuse-parent-owner-released"),
                b"actual old saved owner released after durable clear",
            )
            .unwrap();
            released_parent_owner = true;
        }
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!(
                "owned reuse child timed out: {}",
                fs::read_to_string(&output_path).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let output = fs::read_to_string(&output_path).unwrap();
    assert!(
        status.success() && output.contains("1 passed"),
        "actual reuse child failed: {output}"
    );
    let receipt: serde_json::Value = serde_json::from_slice(
        &fs::read(directory.path().join("reuse-child-verified.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["process_id"], u64::from(child.id()));
    assert_ne!(child.id(), std::process::id());
    assert_eq!(receipt["source_intent"], "import");
    assert_eq!(receipt["source_window_callback_verified"], true);
    assert_eq!(receipt["canonical_reference"], seed["first_reference"]);
    assert!(released_parent_owner);
    assert_eq!(receipt["survivor_only_config_verified"], true);
    assert_eq!(receipt["exact_root_last_reader_cleanup_verified"], true);
    assert!(!first_root.exists());
    assert_eq!(fs::read(&first).unwrap(), bytes);
    assert_eq!(fs::read(&second).unwrap(), bytes);
    let kept = directory.keep();
    fs::write(kept.join("material-reuse-evidence.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "schema":"readable-material-fresh-import-equal-bytes-v1", "fixture_directory":kept,
        "parent_process_id":std::process::id(), "child":receipt, "seed":seed,
        "runtime_bindings":migration_execution_bindings(),
        "scope":"fresh process import=true through ordinary cold worker and own finite callback ACK; no inference entry point invoked; no GUI/device/music/LONG acceptance",
    })).unwrap()).unwrap();
    drop(saved_original_owner);
}

#[test]
fn readable_equal_bytes_new_process_child_imports_different_name_with_own_window_ack() {
    let Some(directory) = std::env::var_os(REUSE_ROOT).map(PathBuf::from) else {
        return;
    };
    pyo3::Python::initialize();
    let root = directory.join("samples");
    let seed: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("reuse-input.json")).unwrap()).unwrap();
    let incoming = directory.join(SECOND_NAME);
    let canonical = crate::audio_engine::material_paths::resolve(
        &root,
        Path::new(seed["first_reference"].as_str().unwrap()),
    )
    .unwrap()
    .path;
    assert_ne!(incoming.file_name(), canonical.file_name());
    let incoming_witness = file_witness(&incoming);
    let canonical_witness = file_witness(&canonical);
    assert_eq!(incoming_witness["sha256"], canonical_witness["sha256"]);
    assert_eq!(incoming_witness["bytes"], canonical_witness["bytes"]);
    assert_eq!(canonical_witness, seed["first_original"]);
    let first_root =
        crate::audio_engine::material_paths::material_root_for_original(&root, &canonical).unwrap();
    let before = material_files(&first_root);
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        seed["material_files"]
    );
    let engine = AudioEngine::new().unwrap();
    assert!(engine.sample_cache.lock().unwrap()[215].is_none());
    assert!(engine.cold_leases.lock().unwrap()[215].is_none());
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[215],
        (0, 0)
    );
    let mut callback = migration_callback_8000(&engine);
    assert!(callback.mixer.bank_for_measurement()[215].is_none());
    pyo3::Python::attach(|py| {
        let config = directory.join("project.config.json");
        let mut guard =
            crate::audio_engine::material_migration_recovery::MigrationProjectGuard::new(
                py,
                root.to_string_lossy().into(),
                config.to_string_lossy().into(),
            )
            .unwrap();
        let mut inventory = guard.lock_inventory(py).unwrap();
        assert!(
            crate::audio_engine::material_migration_recovery::restore_current_material_creation(
                &engine,
                215,
                &seed["first_manifest"]["material_creation"].to_string(),
                &config.to_string_lossy(),
                &"f".repeat(32),
                &inventory,
                None
            )
            .is_err()
        );
        inventory.release();
        guard.release();
    });
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let producer = Arc::new(Mutex::new(producer));
    let (request, adoption) =
        import_with_window(&engine, 215, &incoming, &root, producer.clone(), 2.5, 3.5);
    wait_until(|| consumer.peek().is_ok());
    assert_eq!(adoption.load(Ordering::Acquire), 0);
    let pending = engine.sample_cache.lock().unwrap()[215].clone().unwrap();
    assert_eq!(
        (
            pending.resident_start(),
            pending.resident_end(),
            pending.frame_count()
        ),
        (20_000, 28_000, 64_000)
    );
    let expected: Vec<f32> = (20_000..28_000)
        .map(|frame| (((frame % 251) as i16 - 125) * 200) as f32 / 32_768.0)
        .collect();
    assert_eq!(pending.samples.as_ref(), expected.as_slice());
    let ControlMessage::LoadColdSample {
        id,
        sample,
        loop_region,
        adoption: queued_adoption,
        ..
    } = consumer.peek().unwrap()
    else {
        panic!("fresh import did not queue productive cold source adoption");
    };
    assert_eq!(*id, 215);
    assert_eq!(*loop_region, Some((20_000, 28_000)));
    assert!(Arc::ptr_eq(&pending.samples, &sample.samples));
    assert!(Arc::ptr_eq(&adoption, queued_adoption));
    assert!(callback.mixer.bank_for_measurement()[215].is_none());
    pyo3::Python::attach(|py| {
        let config = directory.join("project.config.json");
        let mut guard =
            crate::audio_engine::material_migration_recovery::MigrationProjectGuard::new(
                py,
                root.to_string_lossy().into(),
                config.to_string_lossy().into(),
            )
            .unwrap();
        let mut inventory = guard.lock_inventory(py).unwrap();
        let error =
            crate::audio_engine::material_migration_recovery::restore_current_material_creation(
                &engine,
                215,
                &seed["first_manifest"]["material_creation"].to_string(),
                &config.to_string_lossy(),
                &"f".repeat(32),
                &inventory,
                None,
            )
            .unwrap_err();
        assert!(error.to_string().contains("actual current source ACK"));
        inventory.release();
        guard.release();
    });
    assert_eq!(callback.drain(&mut consumer), 1);
    let (reference, stages) = success_and_stages(&engine, 215, request);
    assert_eq!(reference, seed["first_reference"].as_str().unwrap());
    assert_eq!(adoption.load(Ordering::Acquire), 2);
    let actual = callback.mixer.bank_for_measurement()[215].as_ref().unwrap();
    assert!(Arc::ptr_eq(&actual.samples, &pending.samples));
    assert_eq!(
        (actual.resident_start(), actual.resident_end()),
        (20_000, 28_000)
    );
    assert_eq!(
        engine
            .input_runtime_ownership
            .source_generation(215, actual, 8_000),
        Some(request)
    );
    assert!(!stages.is_empty());
    assert!(
        stages.iter().all(|stage| stage == "Loading (Publishing…)"),
        "warm import ran offline preparation: {stages:?}"
    );
    let manifest: serde_json::Value =
        serde_json::from_str(&engine.cold_source_manifest(215).unwrap().unwrap()).unwrap();
    assert_eq!(manifest["cache_path"], seed["first_manifest"]["cache_path"]);
    assert_eq!(manifest["identity"], seed["first_manifest"]["identity"]);
    assert_eq!(manifest["descriptor"], seed["first_manifest"]["descriptor"]);
    let integrity = &manifest["integrity"];
    assert_eq!(integrity["warm"], true);
    assert_eq!(
        integrity["incoming_verify_bytes"],
        incoming_witness["bytes"]
    );
    assert_eq!(
        integrity["original_verify_bytes"],
        canonical_witness["bytes"]
    );
    for field in [
        "source_copied_bytes",
        "snapshot_verify_bytes",
        "original_copied_bytes",
        "assignment_copy_bytes",
        "stream_scratch_reserved_bytes",
    ] {
        assert_eq!(integrity[field], 0, "{field}");
    }
    let cache = PathBuf::from(manifest["cache_path"].as_str().unwrap());
    let decoder_witness = file_witness(&cache.join("decoder.f32le"));
    let playback_witness = file_witness(&cache.join("playback.f32le"));
    assert_eq!(integrity["decoder_verify_bytes"], decoder_witness["bytes"]);
    assert_eq!(
        integrity["playback_verify_bytes"],
        playback_witness["bytes"]
    );
    assert_eq!(integrity["playback_read_bytes"], 8_000 * 4);
    assert_eq!(
        decoder_witness["sha256"],
        manifest["descriptor"]["decoder"]["pcm"]["interleaved_sha256"]
    );
    assert_eq!(
        playback_witness["sha256"],
        manifest["descriptor"]["playback"]["pcm"]["interleaved_sha256"]
    );
    let material_root =
        crate::audio_engine::material_paths::material_root_for_original(&root, &canonical).unwrap();
    assert_eq!(
        material_root,
        PathBuf::from(seed["first_root"].as_str().unwrap())
    );
    let binding = crate::audio_engine::material_paths::read_binding(&material_root).unwrap();
    assert_eq!(binding.original_leaf, FIRST_NAME);
    assert_eq!(binding.sha256, incoming_witness["sha256"].as_str().unwrap());
    assert_eq!(binding.bytes, incoming_witness["bytes"].as_u64().unwrap());
    assert_eq!(material_files(&material_root), before);
    let (mut original_artifact, mut cache_artifact, original_receipt, cache_receipt) =
        pyo3::Python::attach(|py| {
            let config = directory.join("project.config.json");
            let mut guard =
                crate::audio_engine::material_migration_recovery::MigrationProjectGuard::new(
                    py,
                    root.to_string_lossy().into(),
                    config.to_string_lossy().into(),
                )
                .unwrap();
            let mut inventory = guard.lock_inventory(py).unwrap();
            assert!(
            crate::audio_engine::material_migration_recovery::restore_current_material_creation(
                &engine,
                215,
                &seed["first_manifest"]["material_creation"].to_string(),
                &config.to_string_lossy(),
                &"1".repeat(32),
                &inventory,
                None
            )
            .is_err()
        );
            crate::audio_engine::material_migration_recovery::restore_current_material_creation(
                &engine,
                215,
                &seed["first_manifest"]["material_creation"].to_string(),
                &config.to_string_lossy(),
                &"f".repeat(32),
                &inventory,
                None,
            )
            .unwrap();
            inventory.release();
            guard.release();
            let receipt =
                crate::audio_engine::material_migration_recovery::MigrationArtifactLease::capture(
                    py,
                    root.to_string_lossy().into(),
                    reference.clone(),
                )
                .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&receipt.receipt_json().unwrap())
                    .unwrap()["material_creation"],
                seed["first_manifest"]["material_creation"]
            );
            let original_receipt =
                serde_json::from_str::<serde_json::Value>(&receipt.receipt_json().unwrap())
                    .unwrap();
            let cache_artifact =
                crate::audio_engine::material_migration_recovery::MigrationArtifactLease::capture(
                    py,
                    root.to_string_lossy().into(),
                    cache.to_string_lossy().into(),
                )
                .unwrap();
            let cache_receipt =
                serde_json::from_str::<serde_json::Value>(&cache_artifact.receipt_json().unwrap())
                    .unwrap();
            (receipt, cache_artifact, original_receipt, cache_receipt)
        });
    let clear = clear_survivor_project(&root);
    crate::audio_engine::cold_load::unload_for_producer(&engine, 215, &producer).unwrap();
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(callback.mixer.bank_for_measurement()[215].is_none());
    assert!(engine.sample_cache.lock().unwrap()[215].is_none());
    assert!(engine.cold_leases.lock().unwrap()[215].is_none());
    callback.retirement.retired.clear();
    drop(pending);
    fs::write(directory.join("reuse-child-intent-cleared.json"),serde_json::to_vec(&serde_json::json!({"process_id":std::process::id(),"actual_unload_callback_verified":true,"clear":clear})).unwrap()).unwrap();
    wait_until(|| directory.join("reuse-parent-owner-released").exists());
    let source_reader = crate::audio_engine::cold_store::sealed_reader(&canonical).unwrap();
    let marker_reader =
        crate::audio_engine::cold_store::sealed_reader(&material_root.join(".material.json"))
            .unwrap();
    pyo3::Python::attach(|py| {
        let mut guard =
            crate::audio_engine::material_migration_recovery::MigrationProjectGuard::new(
                py,
                root.to_string_lossy().into(),
                directory
                    .join("project.config.json")
                    .to_string_lossy()
                    .into(),
            )
            .unwrap();
        let mut inventory = guard.lock_inventory(py).unwrap();
        original_artifact.retire(py, &inventory).unwrap();
        cache_artifact.retire(py, &inventory).unwrap();
        assert_eq!(original_artifact.retirement_status().unwrap(), "pending");
        inventory.release();
        original_artifact.release();
        cache_artifact.release();
        guard.release();
    });
    crate::audio_engine::project_assets::ProjectAssets::shared().collect_for_test();
    assert!(canonical.exists());
    assert_eq!(
        original_artifact.original_retirement_status().unwrap(),
        "pending"
    );
    assert_survivor_registration(&root, true);
    drop(source_reader);
    wait_until(|| {
        !canonical.exists()
            && original_artifact.original_retirement_status().unwrap() == "complete"
            && cache_artifact.retirement_status().unwrap() == "complete"
    });
    assert!(material_root.join(".material.json").exists());
    assert_eq!(original_artifact.retirement_status().unwrap(), "pending");
    assert_survivor_registration(&root, true);
    assert!(fs::remove_file(material_root.join(".material.json")).is_err());
    // The independently queued root owns Inventory even after both Python-facing
    // artifact objects have gone and the original/cache leaf outcomes are terminal.
    drop(original_artifact);
    drop(cache_artifact);
    assert_survivor_registration(&root, true);
    drop(marker_reader);
    wait_until(|| !material_root.exists());
    wait_until(|| {
        pyo3::Python::attach(|py| {
            crate::audio_engine::material_migration_recovery::MigrationProjectGuard::new(
                py,
                root.to_string_lossy().into(),
                directory
                    .join("project.config.json")
                    .to_string_lossy()
                    .into(),
            )
            .map(|mut guard| {
                guard.release();
                true
            })
            .unwrap_or(false)
        })
    });
    assert_survivor_registration(&root, false);
    fs::write(
        directory.join("reuse-child-verified.json"),
        serde_json::to_vec(&serde_json::json!({
            "process_id":std::process::id(), "source_intent":"import", "sample_id":215,
            "source_window_callback_verified":true, "source_window":[20_000,28_000],
            "saved_regular_birth_restored_after_own_ack":true,
            "survivor_only_config_verified":true,"exact_root_last_reader_cleanup_verified":true,
            "original_leaf_terminal_verified":true,"cache_terminal_verified":true,
            "clear_handshake":clear,"original_receipt":original_receipt,"cache_receipt":cache_receipt,
            "canonical_reference":reference, "canonical_root":material_root,
            "incoming":incoming_witness, "canonical_original":canonical_witness,
            "decoder_artifact":decoder_witness, "playback_artifact":playback_witness,
            "manifest":manifest, "actual_offline_stages":stages,
            "runtime_bindings":migration_execution_bindings(),
        }))
        .unwrap(),
    )
    .unwrap();
}
