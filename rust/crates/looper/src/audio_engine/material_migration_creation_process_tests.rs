//! Genuine restart from an actual original-terminal outcome. Birth JSON alone
//! never retires a root; latest journal/current intent/inventory remain required.
use super::retirement_tests::{
    assert_blocked_in_new_process, assert_registration_available, assert_registration_blocked,
};
use super::*;
use crate::audio_engine::cold_store;

const CREATION_ROOT: &str = "FLITZI_MATERIAL_CREATION_RECOVERY_ROOT";
const CREATION_MODE: &str = "FLITZI_MATERIAL_CREATION_RECOVERY_MODE";
const CREATION_CHILD: &str = "audio_engine::material_migration_recovery::tests::material_migration_creation_process_tests::created_material_partial_retirement_fresh_child";
const CREATION_PRODUCER: &str = "audio_engine::material_migration_recovery::tests::material_migration_creation_process_tests::created_material_marker_retirement_producer";

fn execution_receipt() -> Value {
    let exe = std::env::current_exe().unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    let paths = [
        "rust/crates/looper/src/audio_engine/cold_store/material.rs",
        "rust/crates/looper/src/audio_engine/cold_store/lifecycle.rs",
        "rust/crates/looper/src/audio_engine/material_migration_recovery.rs",
        "rust/crates/looper/src/audio_engine/material_migration_creation_process_tests.rs",
        "src/flitzis_looper/controller/material_migration_cleanup.py",
        "src/flitzis_looper/material_migration_model.py",
    ];
    let sources: Vec<_> = paths
        .into_iter()
        .map(|relative| {
            let path = repo.join(relative);
            let bytes = fs::read(&path).unwrap();
            json!({"path":path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))})
        })
        .collect();
    let bytes = fs::read(&exe).unwrap();
    json!({"pid":std::process::id(),"test_executable":{"path":exe,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))},"source_images":sources})
}

fn persisted_fixture(
    root: &Path,
    material: &Value,
    original: &str,
    cache: &str,
) -> (String, String) {
    use pyo3::{prelude::PyAnyMethods, types::PyDictMethods};
    pyo3::Python::attach(|py| {
        let locals = pyo3::types::PyDict::new(py);
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        locals
            .set_item(
                "source_modules",
                repo.join("src").to_string_lossy().as_ref(),
            )
            .unwrap();
        locals
            .set_item(
                "project_root",
                root.parent().unwrap().to_string_lossy().as_ref(),
            )
            .unwrap();
        locals
            .set_item("material_json", material.to_string())
            .unwrap();
        locals.set_item("original_json", original).unwrap();
        locals.set_item("cache_json", cache).unwrap();
        let code = std::ffi::CString::new(r#"
import sys,json
from pathlib import Path
sys.path.insert(0,source_modules)
from flitzis_looper.models import ProjectState,PadContentIdentity
from flitzis_looper.material_migration_model import MaterialMigrationAlias,MaterialMigrationJournal,MigrationAssignment,MigrationArtifactRecord
from flitzis_looper.controller.persistence import ProjectPersistence
material=json.loads(material_json)
project=ProjectState()
project.sample_paths[0]=material['old_reference']
project.pad_content[0]=PadContentIdentity(instance_id='1'*32)
persistence=ProjectPersistence(project)
persistence.config_path=Path(project_root)/'project.config.json'
persistence.flush()
saved=ProjectState.model_validate_json(persistence.config_path.read_bytes())
digest=material['original']['sha256']
alias=MaterialMigrationAlias(transaction_id='9'*32,material_id=material['material_id'],old_reference=material['old_reference'],new_reference=material['new_reference'],original_sha256=digest,original_bytes=material['original']['bytes'],decoder_identity=material['decoder_identity'],playback_identity=material['playback_identity'],cache_path=material['cache_path'],old_source_version=f"{material['old_reference']}|sha256-v1:{digest}",new_source_version=f"{material['new_reference']}|sha256-v1:{digest}",material_creation=material['material_creation'])
journal=MaterialMigrationJournal(transaction_id=alias.transaction_id,phase='failed',config_reference=persistence.config_reference,captured_revision=saved.config_revision,snapshot_json=saved.model_dump_json(),assignments=(MigrationAssignment(sample_id=0,instance_id='1'*32,old_reference=alias.old_reference),),alias=alias,artifacts=(MigrationArtifactRecord(role='target',created=True,evidence=json.loads(original_json)),MigrationArtifactRecord(role='target',created=True,evidence=json.loads(cache_json))),error='bounded offline preparation cancelled before any source admission')
assert journal.alias.material_creation == journal.artifacts[0].evidence.material_creation
journal_json=journal.model_dump_json()
current_json=saved.model_dump_json()
"#).unwrap();
        py.run(&code, Some(&locals), None).unwrap();
        (
            locals
                .get_item("journal_json")
                .unwrap()
                .unwrap()
                .extract()
                .unwrap(),
            locals
                .get_item("current_json")
                .unwrap()
                .unwrap()
                .extract()
                .unwrap(),
        )
    })
}

#[test]
fn created_material_partial_retirement_restarts_with_exact_root_receipt_and_last_reader() {
    for mode in ["last-reader", "unknown-child", "replaced-marker"] {
        partial_retirement_fixture(mode);
    }
}

#[test]
fn created_material_marker_retirement_producer() {
    if std::env::var_os(CREATION_ROOT).is_some()
        && std::env::var(CREATION_MODE).as_deref() == Ok("marker-removed-producer")
    {
        partial_retirement_fixture("marker-removed-producer");
        panic!("actual producer must exit only after verified marker retirement");
    }
}

fn wait_creation_process(
    child: &mut std::process::Child,
    output: &Path,
) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!(
                "exact creation process timed out at {}: {}",
                output.display(),
                fs::read_to_string(output).unwrap()
            );
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn created_empty_material_root_restarts_after_actual_verified_marker_retirement() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let scratch = workspace.join("scratch/goal-readable-material-layout-20261011");
    fs::create_dir_all(&scratch).unwrap();
    for mode in [
        "empty-root-last-reader",
        "empty-root-unknown-child",
        "empty-root-replaced-root",
        "empty-root-reparse-root",
        "empty-root-reappeared-marker",
        "empty-root-reappeared-original",
    ] {
        let fixture = tempfile::tempdir_in(&scratch).unwrap().keep();
        let root = fixture.join("samples");
        let producer_output = fixture.join("marker-producer-output.txt");
        let output = File::create(&producer_output).unwrap();
        let mut producer = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CREATION_PRODUCER, "--nocapture"])
            .env(CREATION_ROOT, &root)
            .env(CREATION_MODE, "marker-removed-producer")
            .stdout(std::process::Stdio::from(output.try_clone().unwrap()))
            .stderr(std::process::Stdio::from(output))
            .spawn()
            .unwrap();
        assert_eq!(
            wait_creation_process(&mut producer, &producer_output).code(),
            Some(77),
            "actual marker producer: {}",
            fs::read_to_string(&producer_output).unwrap()
        );
        let produced: Value =
            serde_json::from_slice(&fs::read(fixture.join("producer-exit-verified.json")).unwrap())
                .unwrap();
        let checkpoint: Value = serde_json::from_slice(
            &fs::read(fixture.join("verified-marker-retired.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(produced["execution"]["pid"], producer.id());
        assert_eq!(checkpoint["execution"]["pid"], producer.id());
        assert_eq!(checkpoint["marker_verified_and_physically_retired"], true);
        assert_eq!(produced["original_terminal_verified"], true);
        assert_eq!(produced["inventory_retained_after_artifact_gc"], true);
        assert_eq!(produced["actual_external_root_reader_denied_delete"], true);
        let material = root
            .parent()
            .unwrap()
            .join(produced["creation"]["root_reference"].as_str().unwrap());
        assert!(material.is_dir() && fs::read_dir(&material).unwrap().next().is_none());
        let child_output = fixture.join("empty-root-child-output.txt");
        let output = File::create(&child_output).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CREATION_CHILD, "--nocapture"])
            .env(CREATION_ROOT, &root)
            .env(CREATION_MODE, mode)
            .stdout(std::process::Stdio::from(output.try_clone().unwrap()))
            .stderr(std::process::Stdio::from(output))
            .spawn()
            .unwrap();
        let status = wait_creation_process(&mut child, &child_output);
        let output = fs::read_to_string(&child_output).unwrap();
        assert!(
            status.success() && output.contains("1 passed"),
            "{mode}: {output}"
        );
        let recovered: Value = serde_json::from_slice(
            &fs::read(fixture.join("creation-child-verified.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(recovered["execution"]["pid"], child.id());
        assert_ne!(child.id(), producer.id());
        assert_ne!(child.id(), std::process::id());
        assert_eq!(recovered["missing_false_terminal_preserved"], true);
        assert_eq!(recovered["unreceipted_root_preserved"], true);
        assert_eq!(
            recovered["root_last_reader_verified"],
            mode == "empty-root-last-reader"
        );
        assert_eq!(material.exists(), mode != "empty-root-last-reader");
        fs::write(fixture.join("empty-root-creation-recovery-evidence.json"),serde_json::to_vec_pretty(&json!({"schema":"actual-marker-retired-empty-root-restart-v1","fixture":fixture,"mode":mode,"producer":produced,"checkpoint":checkpoint,"child":recovered,"scope":"actual native source terminal and exact verified binding retirement; producing process exits with own empty root; fresh current journal/config/Inventory restore; no source ACK, GUI/device/music/LONG acceptance"})).unwrap()).unwrap();
    }
}

fn empty_root_recovery_child(root: &Path, mode: &str, input: &Value) {
    use std::os::windows::fs::OpenOptionsExt;
    let parent = root.parent().unwrap();
    let material = PathBuf::from(input["material_root"].as_str().unwrap());
    let original = material.join(input["receipt"]["files"][0]["name"].as_str().unwrap());
    let marker = material.join(".material.json");
    let encoded = input["receipt"].to_string();
    let expected = parse_receipt(&encoded).unwrap().material_creation.unwrap();
    assert_eq!(
        project_assets::capture_identity(&material)
            .unwrap()
            .unwrap(),
        expected.root_identity
    );
    assert!(!original.exists() && !marker.exists());
    assert!(fs::read_dir(&material).unwrap().next().is_none());
    assert_eq!(input["original_terminal"], "complete");
    assert!(
        crate::audio_engine::material_paths::resolve(
            root,
            Path::new(input["receipt"]["reference"].as_str().unwrap())
        )
        .is_err(),
        "ordinary asset resolution still requires its material binding"
    );
    let config = parent.join("project.config.json");
    let current = input["current"].as_str().unwrap();
    let journal = input["journal"].as_str().unwrap();
    let store =
        crate::audio_engine::material_migration_journal::MaterialMigrationJournalStore::open(
            root.to_string_lossy().into(),
            "9".repeat(32),
        )
        .unwrap();
    assert_eq!(store.history().unwrap().last().unwrap(), journal);
    let mut guard = MigrationProjectGuard::create_native(root, &config).unwrap();
    let mut inventory = inventory_gate(root).unwrap();
    let weak = Arc::downgrade(inventory.handle.as_ref().unwrap());
    for missing in [false, true] {
        let mut not_terminal: Value = serde_json::from_str(journal).unwrap();
        if missing {
            not_terminal["artifacts"][0]
                .as_object_mut()
                .unwrap()
                .remove("original_retired");
        } else {
            not_terminal["artifacts"][0]["original_retired"] = json!(false);
        }
        let exact = not_terminal.to_string();
        store.append(exact.clone()).unwrap();
        let error = MigrationArtifactLease::resume_root_native(
            root,
            &encoded,
            &exact,
            &config.to_string_lossy(),
            current,
            &inventory,
        )
        .err()
        .unwrap();
        assert!(
            error
                .to_string()
                .contains("actual original-terminal receipts")
        );
        assert_eq!(
            project_assets::capture_identity(&material)
                .unwrap()
                .unwrap(),
            expected.root_identity
        );
        assert!(fs::read_dir(&material).unwrap().next().is_none());
    }
    store.append(journal.into()).unwrap();
    let mut unreceipted = input["receipt"].clone();
    unreceipted["material_creation"] = Value::Null;
    assert!(
        MigrationArtifactLease::resume_root_native(
            root,
            &unreceipted.to_string(),
            journal,
            &config.to_string_lossy(),
            current,
            &inventory
        )
        .is_err()
    );
    assert!(fs::read_dir(&material).unwrap().next().is_none());
    let mut replacement_bytes = None;
    if mode == "empty-root-unknown-child" {
        fs::write(
            material.join("unknown.keep"),
            b"unknown empty-root neighbor survives",
        )
        .unwrap();
    } else if mode == "empty-root-replaced-root" || mode == "empty-root-reparse-root" {
        fs::rename(&material, parent.join("actual-created-root-preserved")).unwrap();
        if mode == "empty-root-replaced-root" {
            fs::create_dir(&material).unwrap();
        } else {
            let foreign = parent.join("foreign-reparse-target");
            fs::create_dir(&foreign).unwrap();
            fs::write(foreign.join("private.keep"), b"foreign directory bytes").unwrap();
            let result = Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&material)
                .arg(&foreign)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "actual junction fixture creation failed"
            );
        }
    } else if mode == "empty-root-reappeared-marker" {
        let binding = persisted_binding_bytes(input);
        fs::write(&marker, &binding).unwrap();
        assert_ne!(
            project_assets::capture_identity(&marker).unwrap().unwrap(),
            expected.binding.identity
        );
        replacement_bytes = Some(binding);
    } else if mode == "empty-root-reappeared-original" {
        wav(&original);
        replacement_bytes = Some(fs::read(&original).unwrap());
    }
    if mode != "empty-root-last-reader" {
        assert!(
            MigrationArtifactLease::resume_root_native(
                root,
                &encoded,
                journal,
                &config.to_string_lossy(),
                current,
                &inventory
            )
            .is_err()
        );
        if mode == "empty-root-unknown-child" {
            assert_eq!(
                fs::read(material.join("unknown.keep")).unwrap(),
                b"unknown empty-root neighbor survives"
            );
        }
        if mode == "empty-root-replaced-root" {
            assert_ne!(
                project_assets::capture_identity(&material)
                    .unwrap()
                    .unwrap(),
                expected.root_identity
            );
            assert!(fs::read_dir(&material).unwrap().next().is_none());
        }
        if mode == "empty-root-reparse-root" {
            assert_eq!(
                fs::read(parent.join("foreign-reparse-target/private.keep")).unwrap(),
                b"foreign directory bytes"
            );
        }
        if let Some(bytes) = replacement_bytes {
            assert_eq!(
                fs::read(if mode == "empty-root-reappeared-marker" {
                    &marker
                } else {
                    &original
                })
                .unwrap(),
                bytes
            );
        }
        inventory.release();
        guard.release();
        wait_until(|| weak.upgrade().is_none());
        assert_registration_available(root);
    } else {
        let reader = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(&material)
            .unwrap();
        assert_eq!(
            project_assets::file_identity(&reader).unwrap(),
            expected.root_identity
        );
        let mut artifact = MigrationArtifactLease::resume_root_native(
            root,
            &encoded,
            journal,
            &config.to_string_lossy(),
            current,
            &inventory,
        )
        .unwrap();
        let outcome = artifact.material_outcome.as_ref().unwrap().clone();
        inventory.release();
        artifact.release();
        drop(artifact);
        guard.release();
        assert_eq!(outcome.status().unwrap(), ("pending", None));
        assert!(weak.upgrade().is_some());
        assert_registration_blocked(root);
        assert_blocked_in_new_process(root);
        assert!(fs::read_dir(&material).unwrap().next().is_none());
        let denied =
            project_assets::remove_empty_directory(&material, &expected.root_identity).unwrap_err();
        assert!(matches!(denied.raw_os_error(), Some(32 | 33)));
        drop(reader);
        wait_until(|| !material.exists() && outcome.status().unwrap().0 == "complete");
        wait_until(|| weak.upgrade().is_none());
        assert_registration_available(root);
    }
    fs::write(parent.join("creation-child-verified.json"),serde_json::to_vec(&json!({"execution":execution_receipt(),"mode":mode,"actual_marker_retired_before_process_exit":true,"missing_false_terminal_preserved":true,"unreceipted_root_preserved":true,"root_last_reader_verified":mode=="empty-root-last-reader","artifact_receipt":input["receipt"]})).unwrap()).unwrap();
}

fn persisted_binding_bytes(input: &Value) -> Vec<u8> {
    let bytes: Vec<u8> = input["binding_bytes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
        .collect();
    assert_eq!(
        bytes.len() as u64,
        input["receipt"]["material_creation"]["binding"]["bytes"]
            .as_u64()
            .unwrap()
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        input["receipt"]["material_creation"]["binding"]["sha256"]
            .as_str()
            .unwrap()
    );
    bytes
}

fn partial_retirement_fixture(mode: &str) {
    pyo3::Python::initialize();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let scratch = workspace.join("scratch/goal-readable-material-layout-20261011");
    fs::create_dir_all(&scratch).unwrap();
    let temp = if mode == "marker-removed-producer" {
        None
    } else {
        Some(tempfile::tempdir_in(&scratch).unwrap())
    };
    let fixture = if let Some(temp) = &temp {
        temp.path().to_owned()
    } else {
        PathBuf::from(std::env::var_os(CREATION_ROOT).unwrap())
            .parent()
            .unwrap()
            .to_owned()
    };
    let root = fixture.join("samples");
    fs::create_dir(&root).unwrap();
    let old = root.join("old.wav");
    wav(&old);
    // Productive bounded seed preparation; no full-source test helper or source ACK.
    let material =
        crate::audio_engine::material_migration::prepare_material(&root, &old, 48_000, 2, &|| {
            false
        })
        .unwrap();
    assert_eq!(material.sample.samples.len(), 2);
    let metadata = material.metadata();
    let original = material.lease.original_path.clone();
    let material_root = original.parent().unwrap().to_path_buf();
    let cache = material.lease.cache_path.clone();
    let mut original_artifact = MigrationArtifactLease::capture_native(&root, &original).unwrap();
    let original_receipt = encoded(&original_artifact);
    assert!(
        original_artifact
            .sealed
            .as_ref()
            .unwrap()
            .receipt
            .material_creation
            .is_some()
    );
    let mut cache_artifact = MigrationArtifactLease::capture_native(&root, &cache).unwrap();
    let cache_receipt = encoded(&cache_artifact);
    let (journal_json, current_json) =
        persisted_fixture(&root, &metadata, &original_receipt, &cache_receipt);
    let config = fixture.join("project.config.json");
    let mut guard = MigrationProjectGuard::create_native(&root, &config).unwrap();
    let store =
        crate::audio_engine::material_migration_journal::MaterialMigrationJournalStore::new(
            root.to_string_lossy().into(),
            "9".repeat(32),
        )
        .unwrap();
    store.append(journal_json.clone()).unwrap();
    fs::write(
        material_root.join("unknown.keep"),
        b"own fixture neighbor; cleanup has no authority over it",
    )
    .unwrap();
    let marker = cold_store::sealed_reader(&material_root.join(".material.json")).unwrap();
    let mut inventory = inventory_gate(&root).unwrap();
    let protection = Arc::new(project_assets::RetirementProtection {
        _inventory: inventory.handle.as_ref().unwrap().clone(),
        outcome: Arc::new(project_assets::RetirementOutcome::default()),
    });
    // Queue the real original through the existing exact-file worker. The root
    // remains independently held, so its terminal receipt can be made durable.
    let outcome = ProjectAssets::shared()
        .retire_verified_guarded(
            original_artifact.pin.as_ref().unwrap(),
            original_artifact
                .sealed
                .as_ref()
                .unwrap()
                .receipt
                .files
                .clone(),
            Some(protection),
        )
        .unwrap()
        .unwrap();
    cache_artifact.retire_native(&inventory).unwrap();
    let cache_outcome = cache_artifact.outcome.as_ref().unwrap().clone();
    inventory.release();
    original_artifact.release();
    cache_artifact.release();
    ProjectAssets::shared().orphan_cold(&material.lease);
    material.lease.release_preparation_assignment();
    drop(material);
    wait_until(|| {
        ProjectAssets::shared().collect_for_test();
        !original.exists()
            && !cache.exists()
            && outcome.status().unwrap().0 == "complete"
            && cache_outcome.status().unwrap().0 == "complete"
    });
    assert!(material_root.join(".material.json").exists());
    let mut terminal: Value = serde_json::from_str(&journal_json).unwrap();
    terminal["artifacts"][0]["original_retired"] = json!(true);
    // Revalidate the changed record through the same strict durable schema.
    let terminal = pyo3::Python::attach(|py| {
        use pyo3::{prelude::PyAnyMethods, types::PyDictMethods};
        let locals = pyo3::types::PyDict::new(py);
        locals.set_item("encoded", terminal.to_string()).unwrap();
        let code = std::ffi::CString::new("from flitzis_looper.material_migration_model import MaterialMigrationJournal\nterminal_json=MaterialMigrationJournal.model_validate_json(encoded).model_dump_json()").unwrap();
        py.run(&code, Some(&locals), None).unwrap();
        locals
            .get_item("terminal_json")
            .unwrap()
            .unwrap()
            .extract::<String>()
            .unwrap()
    });
    store.append(terminal.clone()).unwrap();
    assert_eq!(outcome.status().unwrap(), ("complete", None));
    drop(marker);
    fs::write(fixture.join("creation-input.json"), serde_json::to_vec(&json!({"receipt":serde_json::from_str::<Value>(&original_receipt).unwrap(),"journal":terminal,"current":current_json,"material_root":material_root,"binding_bytes":fs::read(material_root.join(".material.json")).unwrap(),"original_terminal":outcome.status().unwrap().0,"parent_execution":execution_receipt()})).unwrap()).unwrap();
    if mode == "marker-removed-producer" {
        use std::os::windows::fs::OpenOptionsExt;
        let external_root_reader = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(&material_root)
            .unwrap();
        let expected = parse_receipt(&original_receipt)
            .unwrap()
            .material_creation
            .unwrap();
        assert_eq!(
            project_assets::file_identity(&external_root_reader).unwrap(),
            expected.root_identity
        );
        let checkpoint_path = fixture.join("verified-marker-retired.json");
        let checkpoint_root = material_root.clone();
        let checkpoint_expected = expected.clone();
        cold_store::observe_material_marker_retired_for_test(&material_root, move |root_guard| {
            assert_eq!(
                project_assets::file_identity(root_guard)?,
                checkpoint_expected.root_identity
            );
            assert!(!checkpoint_root.join(".material.json").exists());
            assert!(fs::read_dir(&checkpoint_root)?.next().is_none());
            fs::write(checkpoint_path, serde_json::to_vec(&json!({"execution":execution_receipt(),"marker_verified_and_physically_retired":true,"root_identity":checkpoint_expected.root_identity,"material_creation":checkpoint_expected,"root_pending_before_empty_directory_delete":true})).unwrap())?;
            Err(io::Error::from_raw_os_error(32))
        });
        fs::remove_file(material_root.join("unknown.keep")).unwrap();
        let mut current_inventory = inventory_gate(&root).unwrap();
        let inventory_weak = Arc::downgrade(current_inventory.handle.as_ref().unwrap());
        let mut recovery = MigrationArtifactLease::resume_root_native(
            &root,
            &original_receipt,
            &terminal,
            &config.to_string_lossy(),
            &current_json,
            &current_inventory,
        )
        .unwrap();
        let root_outcome = recovery.material_outcome.as_ref().unwrap().clone();
        current_inventory.release();
        recovery.release();
        drop(recovery);
        wait_until(|| fixture.join("verified-marker-retired.json").exists());
        assert!(!original.exists() && !material_root.join(".material.json").exists());
        assert!(fs::read_dir(&material_root).unwrap().next().is_none());
        assert_eq!(root_outcome.status().unwrap(), ("pending", None));
        assert!(inventory_weak.upgrade().is_some());
        let retained = cold_store::known_material_creation_for_test(&material_root)
            .unwrap()
            .unwrap();
        assert_eq!(
            retained,
            (
                expected.root_identity.clone(),
                Some(expected.binding.clone()),
                parse_receipt(&original_receipt).unwrap().files[0]
                    .name
                    .clone()
            )
        );
        assert_registration_blocked(&root);
        // The observer returned a real retryable failure after verified marker
        // deletion. Retrying the physical empty-root deletion still fails on
        // this actual external root reader; authority and Inventory stay live.
        let denied =
            project_assets::remove_empty_directory(&material_root, &expected.root_identity)
                .unwrap_err();
        assert!(matches!(denied.raw_os_error(), Some(32 | 33)));
        assert_eq!(
            project_assets::file_identity(&external_root_reader).unwrap(),
            expected.root_identity
        );
        fs::write(fixture.join("producer-exit-verified.json"),serde_json::to_vec(&json!({"execution":execution_receipt(),"exit_after_actual_marker_retirement":true,"original_terminal_verified":true,"root_outcome":"pending","inventory_retained_after_artifact_gc":true,"actual_external_root_reader_denied_delete":true,"creation":expected})).unwrap()).unwrap();
        std::process::exit(77);
    }
    guard.release();
    drop(store);
    let output_path = fixture.join("creation-child-output.txt");
    let output = File::create(&output_path).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CREATION_CHILD, "--nocapture"])
        .env(CREATION_ROOT, &root)
        .env(CREATION_MODE, mode)
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
                "creation child timed out: {}",
                fs::read_to_string(&output_path).unwrap()
            );
        }
        thread::sleep(Duration::from_millis(5));
    };
    let output = fs::read_to_string(&output_path).unwrap();
    assert!(
        status.success() && output.contains("1 passed"),
        "{mode}: {output}"
    );
    let result: Value =
        serde_json::from_slice(&fs::read(fixture.join("creation-child-verified.json")).unwrap())
            .unwrap();
    assert_eq!(result["execution"]["pid"], child.id());
    assert_ne!(child.id(), std::process::id());
    assert_eq!(result["mode"], mode);
    assert_eq!(result["original_terminal_verified"], true);
    assert_eq!(material_root.exists(), mode != "last-reader");
    let kept = temp.unwrap().keep();
    fs::write(kept.join("material-creation-recovery-evidence.json"),serde_json::to_vec_pretty(&json!({"schema":"created-material-root-partial-retirement-v1","fixture":kept,"mode":mode,"child":result,"scope":"actual native original terminal and durable failed-target journal; fresh process exact custody and current inventory; no GUI/device/music/LONG acceptance"})).unwrap()).unwrap();
}

#[test]
fn created_material_partial_retirement_fresh_child() {
    let Some(root) = std::env::var_os(CREATION_ROOT).map(PathBuf::from) else {
        return;
    };
    let mode = std::env::var(CREATION_MODE).unwrap();
    let parent = root.parent().unwrap();
    let input: Value =
        serde_json::from_slice(&fs::read(parent.join("creation-input.json")).unwrap()).unwrap();
    if mode.starts_with("empty-root-") {
        empty_root_recovery_child(&root, &mode, &input);
        return;
    }
    let material = PathBuf::from(input["material_root"].as_str().unwrap());
    let marker = material.join(".material.json");
    let original = material.join(input["receipt"]["files"][0]["name"].as_str().unwrap());
    assert!(!original.exists());
    assert_eq!(input["original_terminal"], "complete");
    let before = fs::read(&marker).unwrap();
    let mut guard =
        MigrationProjectGuard::create_native(&root, &parent.join("project.config.json")).unwrap();
    let mut inventory = inventory_gate(&root).unwrap();
    let weak = Arc::downgrade(inventory.handle.as_ref().unwrap());
    let encoded = input["receipt"].to_string();
    let journal = input["journal"].as_str().unwrap();
    let config = parent.join("project.config.json");
    let current = input["current"].as_str().unwrap();
    let mut forged: Value = serde_json::from_str(journal).unwrap();
    forged["artifacts"][0]["original_retired"] = json!(false);
    let store =
        crate::audio_engine::material_migration_journal::MaterialMigrationJournalStore::open(
            root.to_string_lossy().into(),
            "9".repeat(32),
        )
        .unwrap();
    let without_terminal = forged.to_string();
    store.append(without_terminal.clone()).unwrap();
    let error = MigrationArtifactLease::resume_root_native(
        &root,
        &encoded,
        &without_terminal,
        &config.to_string_lossy(),
        current,
        &inventory,
    )
    .err()
    .unwrap();
    assert!(
        error
            .to_string()
            .contains("actual original-terminal receipts")
    );
    assert_eq!(fs::read(&marker).unwrap(), before);
    // Reuse the exact record already durably witnessed by the producing
    // process after its actual native original outcome completed.
    assert_eq!(store.history().unwrap()[1], journal);
    store.append(journal.into()).unwrap();
    let mut changed = input["receipt"].clone();
    changed["material_creation"]["binding"]["sha256"] = json!("0".repeat(64));
    assert!(
        MigrationArtifactLease::resume_root_native(
            &root,
            &changed.to_string(),
            journal,
            &config.to_string_lossy(),
            current,
            &inventory
        )
        .is_err()
    );
    assert_eq!(fs::read(&marker).unwrap(), before);
    if mode == "replaced-marker" {
        fs::rename(&marker, material.join("foreign-old-marker.keep")).unwrap();
        fs::write(&marker, &before).unwrap();
        let replacement_id = project_assets::capture_identity(&marker).unwrap().unwrap();
        assert_ne!(
            replacement_id,
            parse_receipt(&encoded)
                .unwrap()
                .material_creation
                .unwrap()
                .binding
                .identity
        );
        assert!(
            MigrationArtifactLease::resume_root_native(
                &root,
                &encoded,
                journal,
                &config.to_string_lossy(),
                current,
                &inventory
            )
            .is_err()
        );
        assert_eq!(
            project_assets::capture_identity(&marker).unwrap().unwrap(),
            replacement_id
        );
        assert_eq!(fs::read(&marker).unwrap(), before);
        assert_eq!(
            fs::read(material.join("foreign-old-marker.keep")).unwrap(),
            before
        );
        inventory.release();
    } else {
        let reader = if mode == "last-reader" {
            Some(cold_store::sealed_reader(&marker).unwrap())
        } else {
            None
        };
        if mode == "last-reader" {
            fs::remove_file(material.join("unknown.keep")).unwrap();
        }
        let mut artifact = MigrationArtifactLease::resume_root_native(
            &root,
            &encoded,
            journal,
            &config.to_string_lossy(),
            current,
            &inventory,
        )
        .unwrap();
        let outcome = artifact.material_outcome.as_ref().unwrap().clone();
        inventory.release();
        artifact.release();
        drop(artifact);
        if mode == "last-reader" {
            wait_until(|| outcome.status().unwrap().0 == "pending");
            assert!(weak.upgrade().is_some());
            assert_registration_blocked(&root);
            assert_blocked_in_new_process(&root);
            assert!(marker.exists() && !original.exists());
            assert!(fs::remove_file(&marker).is_err());
            drop(reader);
            wait_until(|| !material.exists() && outcome.status().unwrap().0 == "complete");
            wait_until(|| weak.upgrade().is_none());
            assert_registration_available(&root);
        } else {
            wait_until(|| outcome.status().unwrap().0 == "error");
            assert!(
                outcome
                    .status()
                    .unwrap()
                    .1
                    .unwrap()
                    .contains("unknown material children")
            );
            assert_eq!(fs::read(&marker).unwrap(), before);
            assert_eq!(
                fs::read(material.join("unknown.keep")).unwrap(),
                b"own fixture neighbor; cleanup has no authority over it"
            );
            wait_until(|| weak.upgrade().is_none());
        }
    }
    guard.release();
    fs::write(parent.join("creation-child-verified.json"),serde_json::to_vec(&json!({"execution":execution_receipt(),"mode":mode,"original_terminal_verified":true,"root_last_reader_verified":mode=="last-reader","replacement_or_unknown_preserved":mode!="last-reader","artifact_receipt":input["receipt"]})).unwrap()).unwrap();
}
