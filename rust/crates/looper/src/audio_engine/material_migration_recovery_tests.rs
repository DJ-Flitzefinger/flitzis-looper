//! Actual handle, reader and second-process oracles; no audio device is opened.
#![cfg(windows)]
use super::*;
use crate::audio_engine::{material_migration::prepare_material, mixer::RtMixer};
use crate::messages::SampleBuffer;
use std::{
    process::Command,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

fn root() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("samples");
    fs::create_dir(&root).unwrap();
    (temp, root)
}

fn wav(path: &Path) {
    let mut bytes = Vec::new();
    let data = 128 * 4_u32;
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x02\0");
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&192_000_u32.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for frame in 0..128_i16 {
        bytes.extend_from_slice(&(frame * 32 - 1024).to_le_bytes());
        bytes.extend_from_slice(&(1024 - frame * 16).to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
}

fn stems(directory: &Path, source: &str) {
    fs::create_dir_all(directory).unwrap();
    let mut hashes = serde_json::Map::new();
    for name in &STEM_NAMES[..5] {
        wav(&directory.join(name));
        hashes.insert(
            name.strip_suffix(".wav").unwrap().to_owned(),
            json!(format!(
                "{:x}",
                Sha256::digest(fs::read(directory.join(name)).unwrap())
            )),
        );
    }
    fs::write(
        directory.join(".complete.json"),
        serde_json::to_vec(
            &json!({"schema":"stem-set-sha256-v1","source_version":source,"stems":hashes}),
        )
        .unwrap(),
    )
    .unwrap();
}

fn encoded(lease: &MigrationArtifactLease) -> String {
    serde_json::to_string(&lease.sealed.as_ref().unwrap().receipt).unwrap()
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "bounded native/process condition did not settle"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn isolated_proof(
    root: &Path,
    reference: &Path,
    assets: &ProjectAssets,
) -> (SealedArtifact, ProjectAssetLease) {
    let sealed = seal_artifact(root, reference).unwrap();
    let pin = assets
        .acquire_verified_pin(root, &sealed.path, &sealed.receipt.identity)
        .unwrap();
    (sealed, pin)
}

#[test]
fn original_receipt_reopens_complete_bytes_and_rejects_root_hardlink() {
    let (_temp, root) = root();
    wav(&root.join("old.wav"));
    let mut lease =
        MigrationArtifactLease::capture_native(&root, Path::new("samples/old.wav")).unwrap();
    let receipt = encoded(&lease);
    assert!(
        OpenOptions::new()
            .write(true)
            .open(root.join("old.wav"))
            .is_err(),
        "sealed evidence must deny writes"
    );
    let mut reopened = MigrationArtifactLease::reopen_native(&root, &receipt).unwrap();
    assert_eq!(encoded(&reopened), receipt);
    lease.release();
    reopened.release();
    let (_other, other_root) = self::root();
    fs::hard_link(root.join("old.wav"), other_root.join("old.wav")).unwrap();
    assert_eq!(
        project_assets::capture_identity(&root.join("old.wav")).unwrap(),
        project_assets::capture_identity(&other_root.join("old.wav")).unwrap()
    );
    assert!(
        MigrationArtifactLease::reopen_native(&other_root, &receipt).is_err(),
        "same file ID cannot transfer project permission"
    );
    assert_eq!(
        fs::read(root.join("old.wav")).unwrap(),
        fs::read(other_root.join("old.wav")).unwrap()
    );
}

#[test]
fn original_hash_change_same_identity_and_path_aba_reject_without_retirement() {
    let (_temp, root) = root();
    let path = root.join("old.wav");
    wav(&path);
    let mut lease = MigrationArtifactLease::capture_native(&root, &path).unwrap();
    let receipt = encoded(&lease);
    let identity = project_assets::capture_identity(&path).unwrap();
    lease.release();
    fs::write(&path, b"edited same file object").unwrap();
    assert_eq!(project_assets::capture_identity(&path).unwrap(), identity);
    assert!(MigrationArtifactLease::reopen_native(&root, &receipt).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"edited same file object");
    fs::rename(&path, root.join("kept.wav")).unwrap();
    wav(&path);
    assert!(MigrationArtifactLease::reopen_native(&root, &receipt).is_err());
    assert!(path.exists() && root.join("kept.wav").exists());
}

#[test]
fn strict_receipts_and_typed_traversal_artifacts_are_rejected() {
    let (_temp, root) = root();
    wav(&root.join("old.wav"));
    let lease =
        MigrationArtifactLease::capture_native(&root, Path::new("samples/old.wav")).unwrap();
    let base: Value = serde_json::from_str(&encoded(&lease)).unwrap();
    for changed in [
        {
            let mut v = base.clone();
            v["extra"] = json!(true);
            v
        },
        {
            let mut v = base.clone();
            v["identity"] = json!([1, 2]);
            v
        },
        {
            let mut v = base.clone();
            v["identity"] = json!([true, 2, 3]);
            v
        },
        {
            let mut v = base.clone();
            v["schema_version"] = json!(2);
            v
        },
        {
            let mut v = base.clone();
            v["files"][0]["name"] = json!("other.wav");
            v
        },
        {
            let mut v = base.clone();
            v["files"][0]["sha256"] = json!("A".repeat(64));
            v
        },
        {
            let mut v = base.clone();
            v["reference"] = json!("samples/../old.wav");
            v
        },
        {
            let mut v = base.clone();
            v["kind"] = json!("pcm_directory");
            v
        },
    ] {
        assert!(MigrationArtifactLease::reopen_native(&root, &changed.to_string()).is_err());
    }
    for reference in [
        "samples/../outside.wav",
        "samples/old.wav:stream",
        "samples/config.json",
        "samples/stems/#1/.complete.json",
    ] {
        assert!(MigrationArtifactLease::capture_native(&root, Path::new(reference)).is_err());
    }
    let duplicate = encoded(&lease).replacen(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
        1,
    );
    assert!(parse_receipt(&duplicate).is_err());
    assert!(root.join("old.wav").exists());
}

#[test]
fn reparse_original_is_rejected_before_owner_admission() {
    let (temp, root) = root();
    let outside = temp.path().join("external");
    let material = format!("M{}", "c".repeat(32));
    let original = outside.join(&material).join("original/take.wav");
    fs::create_dir_all(original.parent().unwrap()).unwrap();
    wav(&original);
    let link = root.join("materials");
    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "actual fixture junction creation failed"
    );
    assert!(
        MigrationArtifactLease::capture_native(
            &root,
            &link.join(&material).join("original/take.wav")
        )
        .is_err()
    );
    assert!(original.exists());
    fs::remove_dir(&link).unwrap();
}

#[test]
fn complete_legacy_stems_wait_for_final_job_and_prune_only_empty_container() {
    let (_temp, root) = root();
    let directory = root.join("stems/#216");
    stems(&directory, "samples/old.wav|sha256-v1:verified-by-producer");
    let assets = ProjectAssets::isolated();
    let (sealed, mut pin) = isolated_proof(&root, &directory, &assets);
    let mut job = assets.acquire_pin(&root, &directory).unwrap();
    assets
        .retire_verified(&pin, sealed.receipt.files.clone())
        .unwrap();
    drop(sealed);
    pin.release();
    assets.collect_for_test();
    assert!(STEM_NAMES.iter().all(|name| directory.join(name).exists()));
    assert_eq!(assets.status().unwrap().0, 1);
    job.release();
    assets.collect_for_test();
    assert!(!directory.exists());
    assert_eq!(assets.status().unwrap().0, 0);
    assert!(root.exists());
}

#[test]
fn unknown_or_replaced_stem_leaf_preserves_all_unrecognized_objects() {
    for case in 0..3 {
        let (_temp, root) = root();
        let directory = root.join("stems/#1");
        stems(&directory, "samples/old.wav|sha256-v1:verified-by-producer");
        let assets = ProjectAssets::isolated();
        let (sealed, mut pin) = isolated_proof(&root, &directory, &assets);
        assets
            .retire_verified(&pin, sealed.receipt.files.clone())
            .unwrap();
        drop(sealed);
        if case == 1 {
            fs::rename(
                directory.join("instrumental.wav"),
                root.join("retained-old.wav"),
            )
            .unwrap();
            wav(&directory.join("instrumental.wav"));
        } else if case == 2 {
            let path = directory.join("instrumental.wav");
            let identity = project_assets::capture_identity(&path).unwrap();
            let mut bytes = fs::read(&path).unwrap();
            *bytes.last_mut().unwrap() ^= 1;
            fs::write(&path, bytes).unwrap();
            assert_eq!(project_assets::capture_identity(&path).unwrap(), identity);
        } else {
            fs::write(directory.join("private.txt"), "keep").unwrap();
        }
        pin.release();
        assets.collect_for_test();
        assert!(
            STEM_NAMES.iter().all(|name| directory.join(name).exists()),
            "no known leaf may be deleted before complete identity validation"
        );
        assert_eq!(assets.status().unwrap().0, 0);
        assert!(!assets.status().unwrap().3.is_empty());
        if case == 1 {
            assert!(root.join("retained-old.wav").exists());
        } else if case == 0 {
            assert_eq!(fs::read(directory.join("private.txt")).unwrap(), b"keep");
        }
    }
}

#[test]
fn complete_pcm_receipt_uses_store_queue_and_actual_last_voice_and_job_reader() {
    let (_temp, root) = root();
    wav(&root.join("old.wav"));
    let material = prepare_material(&root, &root.join("old.wav"), 48_000, 2, &|| false).unwrap();
    let original = material.lease.original_path.clone();
    let cache = material.lease.cache_path.clone();
    let assets = ProjectAssets::isolated();
    let engine = Arc::new(());
    assets
        .retain_cold(
            material.lease.clone(),
            &material.sample,
            Arc::downgrade(&engine),
        )
        .unwrap();
    let mut durable = assets.acquire(&root, &original).unwrap();
    let (original_proof, mut original_pin) = isolated_proof(&root, &original, &assets);
    let (pcm_proof, mut pcm_pin) = isolated_proof(&root, &cache, &assets);
    assert_eq!(pcm_proof.receipt.files.len(), 3);
    let receipt = serde_json::to_string(&pcm_proof.receipt).unwrap();
    let mut reopened = MigrationArtifactLease::reopen_native(&root, &receipt).unwrap();
    reopened.release();
    let job_reader = material.sample.clone();
    let mut mixer = RtMixer::new(2, 48_000.0);
    mixer.load_sample(0, material.sample.clone());
    assert!(mixer.play_sample(0, 1.0));
    mixer.load_sample(
        0,
        SampleBuffer {
            channels: 2,
            samples: Arc::from([0.0_f32; 256]),
            residency: None,
        },
    );
    assert!(mixer.voices.iter().any(|voice| {
        voice.is_playing_sample(0)
            && voice
                .sample
                .as_ref()
                .is_some_and(|sample| Arc::ptr_eq(&sample.samples, &material.sample.samples))
    }));
    assets
        .retire_verified(&original_pin, original_proof.receipt.files.clone())
        .unwrap();
    assets
        .retire_verified(&pcm_pin, pcm_proof.receipt.files.clone())
        .unwrap();
    drop(original_proof);
    drop(pcm_proof);
    original_pin.release();
    pcm_pin.release();
    durable.release();
    drop(material);
    assets.collect_for_test();
    assert!(
        original.exists() && cache.exists(),
        "actual live PCM/cache readers must defer queued deletion"
    );
    drop(job_reader);
    assets.collect_for_test();
    assert!(
        original.exists() && cache.exists(),
        "old actual voice still retains the material"
    );
    mixer.stop_sample(0);
    assets.collect_for_test();
    wait_until(|| !original.exists() && !cache.exists());
    assert!(
        root.join("old.wav").exists(),
        "migration rollback/target proof cannot remove unrelated old input"
    );
}

#[test]
fn corrupt_or_incomplete_pcm_and_stem_sets_never_gain_receipts() {
    let (_temp, root) = root();
    wav(&root.join("old.wav"));
    let material = prepare_material(&root, &root.join("old.wav"), 48_000, 2, &|| false).unwrap();
    let cache = material.lease.cache_path.clone();
    drop(material);
    let mut bytes = fs::read(cache.join("decoder.f32le")).unwrap();
    bytes[0] ^= 1;
    fs::write(cache.join("decoder.f32le"), &bytes).unwrap();
    assert!(MigrationArtifactLease::capture_native(&root, &cache).is_err());
    assert!(cache.exists());
    let directory = root.join("stems/#1");
    stems(&directory, "samples/old.wav|sha256-v1:producer");
    fs::remove_file(directory.join("drums.wav")).unwrap();
    assert!(MigrationArtifactLease::capture_native(&root, &directory).is_err());
    assert!(directory.join(".complete.json").exists());
    let staging = root.join("stems/#1/.generation-0123456789abcdef0123456789abcdef");
    stems(&staging, "samples/old.wav|sha256-v1:producer");
    assert!(MigrationArtifactLease::capture_native(&root, &staging).is_err());
}

#[test]
fn inventory_reports_partial_unknown_binding_and_capacity_without_deletion() {
    let (temp, root) = root();
    let config = temp.path().join("project.config.json");
    fs::write(&config, "{}").unwrap();
    let mut guard = MigrationProjectGuard::create_native(&root, &config).unwrap();
    let instance = guard.instance.clone();
    let mut gate = inventory_gate(&root).unwrap();
    let records = gate.records_native().unwrap();
    let record: Value = serde_json::from_str(&records[0]).unwrap();
    assert_eq!(record["instance_id"], instance);
    assert_eq!(record["live"], true);
    gate.release();
    guard.release();
    let directory = root.join(".migration-projects");
    fs::write(directory.join("unknown.txt"), "private").unwrap();
    fs::write(directory.join(format!("{}.json", "a".repeat(32))), "{").unwrap();
    let mut gate = inventory_gate(&root).unwrap();
    let records = gate.records_native().unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(
        records
            .iter()
            .filter(|record| serde_json::from_str::<Value>(record).unwrap()["error"].is_string())
            .count(),
        2
    );
    gate.release();
    assert!(directory.join("unknown.txt").exists());
    for id in 0..253_u32 {
        let instance_id = format!("{id:032x}");
        let record = ProjectRecord {
            schema_version: 1,
            instance_id: instance_id.clone(),
            config_reference: plain_path(&temp.path().join(format!("distinct-{id}.config.json"))),
        };
        fs::write(
            directory.join(format!("{instance_id}.json")),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(
        inventory_gate(&root)
            .unwrap()
            .records_native()
            .unwrap()
            .len(),
        RECORD_LIMIT
    );
    assert!(
        MigrationProjectGuard::create_native(&root, &temp.path().join("new.config.json")).is_err()
    );
    assert_eq!(fs::read(&config).unwrap(), b"{}");
    assert_eq!(fs::read(directory.join("unknown.txt")).unwrap(), b"private");
}

#[test]
fn project_registration_rejects_traversal_outside_and_inventory_aba() {
    let (temp, root) = root();
    let config = temp.path().join("project.config.json");
    let raw_root = temp.path().join("child/../samples");
    fs::create_dir(temp.path().join("child")).unwrap();
    assert!(MigrationProjectGuard::create_native(&raw_root, &config).is_err());
    assert!(
        MigrationArtifactLease::capture_native(&raw_root, Path::new("samples/old.wav")).is_err()
    );
    for invalid_config in [
        root.join("../project.config.json"),
        temp.path().parent().unwrap().join("unrelated.config.json"),
        temp.path().join("project.config.json:stream"),
    ] {
        assert!(MigrationProjectGuard::create_native(&root, &invalid_config).is_err());
    }
    assert!(!root.join(".migration-projects").exists());
    let mut guard = MigrationProjectGuard::create_native(&root, &config).unwrap();
    let mut gate = inventory_gate(&root).unwrap();
    assert!(
        fs::rename(&root, temp.path().join("replaced-samples")).is_err(),
        "held raw-root ancestor guards must deny actual root ABA"
    );
    let record_path = gate
        .handle
        .as_ref()
        .unwrap()
        .path
        .join(format!("{}.json", guard.instance));
    assert!(
        fs::rename(
            &record_path,
            gate.handle.as_ref().unwrap().path.join("renamed.json")
        )
        .is_err()
    );
    assert!(
        fs::rename(
            gate.handle.as_ref().unwrap().path.join(".inventory.lock"),
            gate.handle.as_ref().unwrap().path.join("old.lock")
        )
        .is_err()
    );
    assert!(
        MigrationProjectGuard::create_native(&root, &config).is_err(),
        "held inventory gate must reject new registration"
    );
    gate.release();
    guard.release();
    let mut replacement = MigrationProjectGuard::create_native(&root, &config).unwrap();
    assert_ne!(replacement.instance, guard.instance);
    replacement.release();
}

#[test]
fn more_than_256_same_config_reopens_compact_only_recognized_inactive_records() {
    let (temp, root) = root();
    let config = temp.path().join("project.config.json");
    fs::write(&config, "{\"performer\":\"unchanged\"}").unwrap();
    let mut previous = None;
    for _ in 0..258 {
        let mut guard = MigrationProjectGuard::create_native(&root, &config).unwrap();
        assert_ne!(previous.as_ref(), Some(&guard.instance));
        previous = Some(guard.instance.clone());
        guard.release();
        let records = inventory_gate(&root).unwrap().records_native().unwrap();
        assert_eq!(records.len(), 1);
        let record: Value = serde_json::from_str(&records[0]).unwrap();
        assert_eq!(record["instance_id"], previous.as_deref().unwrap());
        assert_eq!(record["live"], false);
    }
    assert_eq!(fs::read(&config).unwrap(), b"{\"performer\":\"unchanged\"}");
}

#[test]
fn project_registration_fault_before_and_after_new_record_retains_known_binding() {
    let (temp, root) = root();
    let config = temp.path().join("project.config.json");
    let mut old = MigrationProjectGuard::create_native(&root, &config).unwrap();
    let old_instance = old.instance.clone();
    old.release();
    for (phase, expected) in [("before_new_record", 1), ("new_record_sealed", 2)] {
        let error = MigrationProjectGuard::create_at(&root, &config, &|actual| {
            if actual == phase {
                Err(invalid(format!("injected {phase}")))
            } else {
                Ok(())
            }
        })
        .err()
        .unwrap();
        assert!(error.to_string().contains(phase));
        let records = inventory_gate(&root).unwrap().records_native().unwrap();
        assert_eq!(records.len(), expected);
        assert!(records.iter().any(
            |record| serde_json::from_str::<Value>(record).unwrap()["instance_id"] == old_instance
        ));
        assert!(records.iter().all(|record| {
            let value: Value = serde_json::from_str(record).unwrap();
            value["error"].is_null()
                && value["config_reference"] == plain_path(&config)
                && value["live"] == false
        }));
    }
    let mut retry = MigrationProjectGuard::create_native(&root, &config).unwrap();
    retry.release();
    assert_eq!(
        inventory_gate(&root)
            .unwrap()
            .records_native()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn same_config_compaction_preserves_live_owner_foreign_config_and_unknown_records() {
    let (temp, root) = root();
    let first_config = temp.path().join("first.config.json");
    let mut live = MigrationProjectGuard::create_native(&root, &first_config).unwrap();
    let mut foreign =
        MigrationProjectGuard::create_native(&root, &temp.path().join("second.config.json"))
            .unwrap();
    let foreign_id = foreign.instance.clone();
    foreign.release();
    fs::write(root.join(".migration-projects/private.txt"), "preserve").unwrap();
    let mut same = MigrationProjectGuard::create_native(&root, &first_config).unwrap();
    let records = inventory_gate(&root).unwrap().records_native().unwrap();
    assert_eq!(records.len(), 4);
    assert!(records.iter().any(|record| {
        let value: Value = serde_json::from_str(record).unwrap();
        value["instance_id"] == live.instance && value["live"] == true
    }));
    assert!(
        records.iter().any(
            |record| serde_json::from_str::<Value>(record).unwrap()["instance_id"] == foreign_id
        )
    );
    same.release();
    live.release();
    let mut retry = MigrationProjectGuard::create_native(&root, &first_config).unwrap();
    assert_eq!(
        inventory_gate(&root)
            .unwrap()
            .records_native()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        fs::read(root.join(".migration-projects/private.txt")).unwrap(),
        b"preserve"
    );
    assert!(
        root.join(".migration-projects")
            .join(format!("{foreign_id}.json"))
            .exists()
    );
    retry.release();
}

#[test]
fn self_consistent_pcm_with_unsupported_transform_or_decoder_is_preserved() {
    for provenance in [false, true] {
        let (_temp, root) = root();
        wav(&root.join("old.wav"));
        let material =
            prepare_material(&root, &root.join("old.wav"), 48_000, 2, &|| false).unwrap();
        let cache = material.lease.cache_path.clone();
        drop(material);
        let manifest_path = cache.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        if provenance {
            manifest["descriptor"]["decoder"]["pcm"]["provenance"]["version"] =
                json!("unsupported");
        } else {
            manifest["descriptor"]["playback"]["transform"]["processing"] = json!("unsupported");
        }
        let decoder_id = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&manifest["descriptor"]["decoder"]).unwrap())
        );
        manifest["decoder_identity"] = json!(decoder_id);
        manifest["descriptor"]["playback"]["parent_identity"] =
            manifest["decoder_identity"].clone();
        manifest["identity"] = json!(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&manifest["descriptor"]).unwrap())
        ));
        let forged = serde_json::to_vec(&manifest).unwrap();
        fs::write(&manifest_path, &forged).unwrap();
        assert!(MigrationArtifactLease::capture_native(&root, &cache).is_err());
        assert_eq!(fs::read(&manifest_path).unwrap(), forged);
        assert!(PCM_NAMES.iter().all(|name| cache.join(name).exists()));
    }
}

fn child(root: &Path, mode: &str) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("audio_engine::material_migration_recovery::tests::project_guard_process_child")
        .arg("--nocapture")
        .env("FLITZI_MIGRATION_GUARD_CHILD_ROOT", root)
        .env("FLITZI_MIGRATION_GUARD_CHILD_MODE", mode)
        .spawn()
        .unwrap()
}

fn child_ready(parent: &Path, value: Value) {
    let temporary = parent.join("child-ready.part");
    let mut file = File::create(&temporary).unwrap();
    file.write_all(value.to_string().as_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    fs::rename(temporary, parent.join("child-ready.json")).unwrap();
}

#[test]
fn project_guard_actual_second_process_live_then_inactive_and_inventory_blocks_registration() {
    let (temp, root) = root();
    let ready = temp.path().join("child-ready.json");
    let stop = temp.path().join("child-stop");
    let mut process = child(&root, "live");
    wait_until(|| ready.exists());
    let child_record: Value = serde_json::from_slice(&fs::read(&ready).unwrap()).unwrap();
    assert_ne!(
        child_record["pid"].as_u64().unwrap(),
        u64::from(std::process::id())
    );
    let mut gate = inventory_gate(&root).unwrap();
    let records = gate.records_native().unwrap();
    assert_eq!(records.len(), 1);
    let live: Value = serde_json::from_str(&records[0]).unwrap();
    assert_eq!(live["instance_id"], child_record["instance_id"]);
    assert_eq!(live["live"], true);
    fs::write(&stop, "release").unwrap();
    assert_eq!(
        process.wait().unwrap().code(),
        Some(77),
        "child terminates with its native record handles still held"
    );
    let records = gate.records_native().unwrap();
    let inactive: Value = serde_json::from_str(&records[0]).unwrap();
    assert_eq!(inactive["instance_id"], live["instance_id"]);
    assert_eq!(inactive["config_reference"], live["config_reference"]);
    assert_eq!(inactive["live"], false);
    assert!(inactive["error"].is_null());
    fs::remove_file(&ready).unwrap();
    let mut blocked = child(&root, "blocked");
    assert!(blocked.wait().unwrap().success());
    let result: Value = serde_json::from_slice(&fs::read(&ready).unwrap()).unwrap();
    assert_eq!(result["blocked"], true);
    assert_eq!(
        gate.records_native().unwrap().len(),
        1,
        "denied child cannot leave an owner record"
    );
    gate.release();
    assert_eq!(
        fs::read(root.join(".migration-projects").join(format!(
            "{}.json",
            child_record["instance_id"].as_str().unwrap()
        )))
        .unwrap()
        .is_empty(),
        false
    );
}

#[test]
fn project_guard_process_child() {
    let Some(root) = std::env::var_os("FLITZI_MIGRATION_GUARD_CHILD_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let parent = root.parent().unwrap();
    let config = parent.join("project.config.json");
    let mode = std::env::var("FLITZI_MIGRATION_GUARD_CHILD_MODE").unwrap();
    if mode == "blocked" {
        let error = MigrationProjectGuard::create_native(&root, &config)
            .err()
            .expect("second process must see genuine inventory lock denial");
        assert!(
            matches!(error.raw_os_error(), Some(32 | 33)),
            "denial must be actual Windows sharing, not a mocked bool"
        );
        child_ready(parent, json!({"blocked":true,"pid":std::process::id()}));
        return;
    }
    let guard = MigrationProjectGuard::create_native(&root, &config).unwrap();
    child_ready(
        parent,
        json!({"instance_id":guard.instance,"pid":std::process::id()}),
    );
    wait_until(|| parent.join("child-stop").exists());
    // Deliberately bypass Rust drops: the real kernel releases crashed-owner handles.
    std::process::exit(77);
}

#[path = "material_migration_retirement_tests.rs"]
mod retirement_tests;
