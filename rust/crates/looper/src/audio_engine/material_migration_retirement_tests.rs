//! Physical queue/lock oracles; no device, callback mock or serialized authority.
#![cfg(windows)]

use super::*;
use crate::audio_engine::cold_store;

const SHUTDOWN_CHILD: &str = "FLITZI_L4_ASSET_SHUTDOWN_CHILD";
const SHUTDOWN_TEST: &str = "audio_engine::material_migration_recovery::tests::retirement_tests::actual_engine_shutdown_keeps_late_lease_cleanup_and_sharing_retry";
const PRESERVATION_CHILD: &str = "FLITZI_L4_ASSET_PRESERVATION_CHILD";
const PRESERVATION_TEST: &str = "audio_engine::material_migration_recovery::tests::retirement_tests::actual_asset_worker_preserves_targets_changed_after_admission";
const REPARSE_CHILD: &str = "FLITZI_L4_ASSET_REPARSE_CHILD";
const REPARSE_TEST: &str = "audio_engine::material_migration_recovery::tests::retirement_tests::actual_asset_worker_preserves_external_junction_after_retirement_admission";

#[test]
fn actual_asset_worker_preserves_external_junction_after_retirement_admission() {
    if std::env::var_os(REPARSE_CHILD).is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", REPARSE_TEST, "--nocapture", "--test-threads=1"])
            .env(REPARSE_CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "asset reparse child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        return;
    }
    let scratch = std::env::temp_dir().join("codex");
    fs::create_dir_all(&scratch).unwrap();
    let temp = tempfile::Builder::new()
        .prefix("flitzi-asset-reparse-")
        .tempdir_in(scratch)
        .unwrap();
    let root = temp.path().join("samples");
    let material = root
        .join("materials")
        .join("M0123456789abcdef0123456789abcdef");
    let original = material.join("original/source.wav");
    fs::create_dir_all(original.parent().unwrap()).unwrap();
    wav(&original);
    let bytes = fs::read(&original).unwrap();
    let foreign = temp.path().join("foreign-material");
    let foreign_original = foreign.join("original/source.wav");
    fs::create_dir_all(foreign_original.parent().unwrap()).unwrap();
    // Even equal bytes at the redirected locator grant no deletion authority.
    fs::write(&foreign_original, &bytes).unwrap();
    let foreign_keep = foreign.join("private.keep");
    fs::write(&foreign_keep, b"test-generated external neighbor").unwrap();
    let moved = temp.path().join("moved-material");
    let engine = crate::audio_engine::AudioEngine::new().unwrap();
    let assets = engine.project_assets.clone();
    assert!(Arc::ptr_eq(&assets, &ProjectAssets::shared()));
    let mut job = assets.acquire_pin(&root, &original).unwrap();
    let mut inventory = inventory_gate(&root).unwrap();
    let inventory_lifetime = Arc::downgrade(inventory.handle.as_ref().unwrap());
    let mut artifact = MigrationArtifactLease::capture_native(&root, &original).unwrap();
    artifact.retire_native(&inventory).unwrap();
    let outcome = artifact.outcome.as_ref().unwrap().clone();
    inventory.release();
    artifact.release();
    drop(artifact);
    assert_eq!(outcome.status().unwrap(), ("pending", None));
    assert_registration_blocked(&root);
    // Real queued inventory custody guards Samples itself. Never weaken that
    // owner merely to manufacture a root replacement that cannot actually occur.
    assert!(matches!(
        fs::rename(&root, temp.path().join("moved-samples"))
            .unwrap_err()
            .raw_os_error(),
        Some(32 | 33)
    ));
    // The released artifact seal no longer guards this descendant directory.
    // Keep the actual inventory/root custody and genuine transient job pin.
    fs::rename(&material, &moved).unwrap();
    let junction = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&material)
        .arg(&foreign)
        .output()
        .unwrap();
    assert!(
        junction.status.success(),
        "contained fixture junction failed: {:?}\n{}\n{}",
        junction.status,
        String::from_utf8_lossy(&junction.stdout),
        String::from_utf8_lossy(&junction.stderr)
    );
    assert!(project_assets::reject_links(&original).is_err());
    assert_eq!(outcome.status().unwrap(), ("pending", None));
    assert!(inventory_lifetime.upgrade().is_some());
    assert_registration_blocked(&root);
    job.release();
    drop(job);
    // Only the actual shared background worker processes this admitted receipt.
    // No direct collector, synthetic worker error or target re-admission runs.
    wait_until(|| outcome.status().unwrap().0 == "error");
    let terminal = outcome.status().unwrap();
    let error = terminal.1.as_ref().unwrap();
    assert!(
        error.contains("path contains a link") || error.contains("path contains a reparse point"),
        "{error}"
    );
    assert!(error.chars().count() <= 2048);
    wait_until(|| inventory_lifetime.upgrade().is_none());
    assert_eq!(fs::read(&foreign_original).unwrap(), bytes);
    assert_eq!(fs::read(moved.join("original/source.wav")).unwrap(), bytes);
    assert_eq!(
        fs::read(&foreign_keep).unwrap(),
        b"test-generated external neighbor"
    );
    assert_registration_available(&root);
    // Remove the caller's junction itself, without traversing its foreign tree.
    fs::remove_dir(&material).unwrap();
    fs::rename(&moved, &material).unwrap();
    let sentinel = root.join("materials/Mfedcba9876543210fedcba9876543210/original/sentinel.wav");
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    wav(&sentinel);
    let mut later_inventory = inventory_gate(&root).unwrap();
    let later_lifetime = Arc::downgrade(later_inventory.handle.as_ref().unwrap());
    let mut receipt = MigrationArtifactLease::capture_native(&root, &sentinel).unwrap();
    receipt.retire_native(&later_inventory).unwrap();
    let sentinel_outcome = receipt.outcome.as_ref().unwrap().clone();
    later_inventory.release();
    receipt.release();
    drop(receipt);
    wait_until(|| !sentinel.exists() && sentinel_outcome.status().unwrap().0 == "complete");
    wait_until(|| later_lifetime.upgrade().is_none());
    assert_eq!(outcome.status().unwrap(), terminal);
    assert_eq!(fs::read(&original).unwrap(), bytes);
    assert_eq!(fs::read(&foreign_original).unwrap(), bytes);
    assert_eq!(
        fs::read(&foreign_keep).unwrap(),
        b"test-generated external neighbor"
    );
    assert_registration_available(&root);
}

#[test]
fn actual_asset_worker_preserves_targets_changed_after_admission() {
    if std::env::var_os(PRESERVATION_CHILD).is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                PRESERVATION_TEST,
                "--nocapture",
                "--test-threads=1",
            ])
            .env(PRESERVATION_CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "asset preservation child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        return;
    }
    let scratch = std::env::temp_dir().join("codex");
    fs::create_dir_all(&scratch).unwrap();
    let engine = crate::audio_engine::AudioEngine::new().unwrap();
    let assets = engine.project_assets.clone();
    assert!(Arc::ptr_eq(&assets, &ProjectAssets::shared()));
    for change in ["unknown-child", "replacement", "same-file-edit"] {
        let temp = tempfile::Builder::new()
            .prefix("flitzi-asset-preservation-")
            .tempdir_in(&scratch)
            .unwrap();
        let root = temp.path().join("samples");
        let original = root.join("materials/M0123456789abcdef0123456789abcdef/original/source.wav");
        fs::create_dir_all(original.parent().unwrap()).unwrap();
        wav(&original);
        let original_before = fs::read(&original).unwrap();
        let target = if change == "unknown-child" {
            let directory = root.join("materials/M0123456789abcdef0123456789abcdef/stems/.ready-0123456789abcdef0123456789abcdef");
            let source = format!(
                "samples/materials/M0123456789abcdef0123456789abcdef/original/source.wav|sha256-v1:{:x}",
                Sha256::digest(&original_before)
            );
            stems(&directory, &source);
            directory
        } else {
            original.clone()
        };
        let known_paths: Vec<_> = if change == "unknown-child" {
            STEM_NAMES.iter().map(|name| target.join(name)).collect()
        } else {
            vec![target.clone()]
        };
        let mut expected_bytes: Vec<_> = known_paths
            .iter()
            .map(|path| (path.clone(), fs::read(path).unwrap()))
            .collect();
        let identity_before = project_assets::capture_identity(&target).unwrap();
        let neighbor = root.join("unrelated.keep");
        fs::write(&neighbor, b"outside the exact retirement target").unwrap();
        let mut job = assets.acquire_pin(&root, &target).unwrap();
        let mut inventory = inventory_gate(&root).unwrap();
        let inventory_lifetime = Arc::downgrade(inventory.handle.as_ref().unwrap());
        let mut artifact = MigrationArtifactLease::capture_native(&root, &target).unwrap();
        assert_eq!(
            artifact.sealed.as_ref().unwrap().receipt.files.len(),
            known_paths.len()
        );
        artifact.retire_native(&inventory).unwrap();
        let outcome = artifact.outcome.as_ref().unwrap().clone();
        inventory.release();
        artifact.release();
        drop(artifact);
        assert_eq!(outcome.status().unwrap(), ("pending", None));
        assert_registration_blocked(&root);
        // Mutations happen after exact admission/seal release, while a genuine
        // transient job lease still excludes physical worker deletion.
        let unknown = target.join("late-private.keep");
        let moved = root.join("moved-original.wav");
        let expected_error = match change {
            "unknown-child" => {
                fs::write(&unknown, b"introduced after retirement admission").unwrap();
                assert_eq!(
                    project_assets::capture_identity(&target).unwrap(),
                    identity_before
                );
                "generation contains an unknown file; preserved"
            }
            "replacement" => {
                fs::rename(&target, &moved).unwrap();
                wav(&target);
                assert_ne!(
                    project_assets::capture_identity(&target).unwrap(),
                    identity_before
                );
                assert_eq!(fs::read(&target).unwrap(), original_before);
                "retired original was replaced; preserved"
            }
            "same-file-edit" => {
                let mut changed = original_before.clone();
                *changed.last_mut().unwrap() ^= 1;
                fs::write(&target, &changed).unwrap();
                assert_eq!(
                    project_assets::capture_identity(&target).unwrap(),
                    identity_before
                );
                assert_eq!(changed.len(), original_before.len());
                "retired leaf changed contents; preserved"
            }
            _ => unreachable!(),
        };
        if change != "unknown-child" {
            expected_bytes[0].1 = fs::read(&target).unwrap();
        }
        assert_eq!(outcome.status().unwrap(), ("pending", None));
        assert_registration_blocked(&root);
        job.release();
        drop(job);
        // The actual process worker alone must settle this own receipt.
        // No collect*(), injected failure or target re-admission is used.
        wait_until(|| outcome.status().unwrap().0 == "error");
        let terminal = outcome.status().unwrap();
        let error = terminal.1.as_ref().unwrap();
        assert!(error.contains(expected_error), "{change}: {error}");
        assert!(error.chars().count() <= 2048);
        wait_until(|| inventory_lifetime.upgrade().is_none());
        for (path, bytes) in &expected_bytes {
            assert_eq!(fs::read(path).unwrap(), *bytes, "{change}: {path:?}");
        }
        assert_registration_available(&root);
        if change == "unknown-child" {
            assert_eq!(
                fs::read(&unknown).unwrap(),
                b"introduced after retirement admission"
            );
            assert_eq!(fs::read(&original).unwrap(), original_before);
            // Removing this test-created obstruction must not revive a terminal
            // failed retirement when the worker subsequently processes other work.
            fs::remove_file(&unknown).unwrap();
        } else if change == "replacement" {
            assert_eq!(fs::read(&moved).unwrap(), original_before);
        }
        let sentinel =
            root.join("materials/Mfedcba9876543210fedcba9876543210/original/sentinel.wav");
        fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
        wav(&sentinel);
        let mut later_inventory = inventory_gate(&root).unwrap();
        let later_lifetime = Arc::downgrade(later_inventory.handle.as_ref().unwrap());
        let mut sentinel_receipt =
            MigrationArtifactLease::capture_native(&root, &sentinel).unwrap();
        sentinel_receipt.retire_native(&later_inventory).unwrap();
        let sentinel_outcome = sentinel_receipt.outcome.as_ref().unwrap().clone();
        later_inventory.release();
        sentinel_receipt.release();
        drop(sentinel_receipt);
        wait_until(|| !sentinel.exists() && sentinel_outcome.status().unwrap().0 == "complete");
        wait_until(|| later_lifetime.upgrade().is_none());
        assert_eq!(outcome.status().unwrap(), terminal);
        for (path, bytes) in &expected_bytes {
            assert_eq!(fs::read(path).unwrap(), *bytes, "{change}: {path:?}");
        }
        if change == "replacement" {
            assert_eq!(fs::read(&moved).unwrap(), original_before);
        }
        assert_eq!(
            fs::read(&neighbor).unwrap(),
            b"outside the exact retirement target"
        );
        assert_registration_available(&root);
    }
}

#[test]
fn actual_engine_shutdown_keeps_late_lease_cleanup_and_sharing_retry() {
    if std::env::var_os(SHUTDOWN_CHILD).is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", SHUTDOWN_TEST, "--nocapture", "--test-threads=1"])
            .env(SHUTDOWN_CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "asset shutdown child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        return;
    }
    use std::os::windows::fs::OpenOptionsExt;

    let scratch = std::env::temp_dir().join("codex");
    fs::create_dir_all(&scratch).unwrap();
    let temp = tempfile::Builder::new()
        .prefix("flitzi-asset-shutdown-")
        .tempdir_in(scratch)
        .unwrap();
    let root = temp.path().join("samples");
    let original = root.join("materials/M0123456789abcdef0123456789abcdef/original/blocked.wav");
    let sentinel = root.join("materials/Mfedcba9876543210fedcba9876543210/original/sentinel.wav");
    for path in [&original, &sentinel] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        wav(path);
    }
    let before = fs::read(&original).unwrap();
    let unknown = original.parent().unwrap().join("unknown.keep");
    fs::write(&unknown, b"outside both exact retirement receipts").unwrap();
    // This real external handle grants reads only. It is not an asset lease,
    // native PCM owner or test-injected cleanup failure.
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&original)
        .unwrap();
    assert!(matches!(
        fs::remove_file(&original).unwrap_err().raw_os_error(),
        Some(32 | 33)
    ));
    let engine = Arc::new(crate::audio_engine::AudioEngine::new().unwrap());
    let engine_lifetime = Arc::downgrade(&engine);
    let assets = engine.project_assets.clone();
    assert!(Arc::ptr_eq(&assets, &ProjectAssets::shared()));
    // Transient source/job leases protect an unassigned original. A current
    // saved assignment correctly denies this retirement admission altogether.
    let mut original_lease = assets.acquire_pin(&root, &original).unwrap();
    let mut sentinel_lease = assets.acquire_pin(&root, &sentinel).unwrap();
    let mut inventory = inventory_gate(&root).unwrap();
    let inventory_lifetime = Arc::downgrade(inventory.handle.as_ref().unwrap());
    let mut original_receipt = MigrationArtifactLease::capture_native(&root, &original).unwrap();
    let mut sentinel_receipt = MigrationArtifactLease::capture_native(&root, &sentinel).unwrap();
    original_receipt.retire_native(&inventory).unwrap();
    sentinel_receipt.retire_native(&inventory).unwrap();
    let original_outcome = original_receipt.outcome.as_ref().unwrap().clone();
    let sentinel_outcome = sentinel_receipt.outcome.as_ref().unwrap().clone();
    inventory.release();
    original_receipt.release();
    sentinel_receipt.release();
    drop(original_receipt);
    drop(sentinel_receipt);
    assert_eq!(original_outcome.status().unwrap(), ("pending", None));
    assert_eq!(sentinel_outcome.status().unwrap(), ("pending", None));
    // No facade or cloned Engine remains. Its actual Drop closes admission/jobs;
    // the independent shared asset worker must still finish later releases.
    drop(engine);
    assert!(engine_lifetime.upgrade().is_none());
    assert_eq!(fs::read(&original).unwrap(), before);
    assert!(sentinel.is_file());
    original_lease.release();
    sentinel_lease.release();
    drop(original_lease);
    drop(sentinel_lease);
    // Own unblocked progress proves the real worker ran after Engine shutdown.
    // Never call collect/collect_for_test or re-admit a retirement in this case.
    wait_until(|| !sentinel.exists() && sentinel_outcome.status().unwrap().0 == "complete");
    assert_eq!(fs::read(&original).unwrap(), before);
    assert_eq!(original_outcome.status().unwrap(), ("pending", None));
    assert!(inventory_lifetime.upgrade().is_some());
    assert_registration_blocked(&root);
    assert_eq!(
        fs::read(&unknown).unwrap(),
        b"outside both exact retirement receipts"
    );
    drop(handle);
    wait_until(|| !original.exists() && original_outcome.status().unwrap().0 == "complete");
    wait_until(|| inventory_lifetime.upgrade().is_none());
    assert_eq!(original_outcome.status().unwrap(), ("complete", None));
    assert_eq!(sentinel_outcome.status().unwrap(), ("complete", None));
    assert_eq!(
        fs::read(&unknown).unwrap(),
        b"outside both exact retirement receipts"
    );
    assert_registration_available(&root);
}

pub(super) fn assert_registration_blocked(root: &Path) {
    let config = root.parent().unwrap().join("project.config.json");
    let error = MigrationProjectGuard::create_native(root, &config)
        .err()
        .expect("actual queued inventory lock must deny registration");
    assert!(matches!(error.raw_os_error(), Some(32 | 33)));
}

pub(super) fn assert_registration_available(root: &Path) {
    let config = root.parent().unwrap().join("project.config.json");
    let mut guard = MigrationProjectGuard::create_native(root, &config).unwrap();
    guard.release();
}

pub(super) fn assert_blocked_in_new_process(root: &Path) {
    let ready = root.parent().unwrap().join("child-ready.json");
    if ready.exists() {
        fs::remove_file(&ready).unwrap();
    }
    let mut process = child(root, "blocked");
    assert!(process.wait().unwrap().success());
    let result: Value = serde_json::from_slice(&fs::read(ready).unwrap()).unwrap();
    assert_eq!(result["blocked"], true);
    assert_ne!(
        result["pid"].as_u64().unwrap(),
        u64::from(std::process::id())
    );
}

#[test]
fn original_queue_keeps_inventory_after_caller_release_and_artifact_gc_until_last_job() {
    original_queue_keeps_actual_custody(false);
}

#[test]
fn mid_original_queue_keeps_inventory_after_artifact_gc_until_actual_last_reader() {
    original_queue_keeps_actual_custody(true);
}

fn original_queue_keeps_actual_custody(mid_layout: bool) {
    let (temp, root) = root();
    let path = if mid_layout {
        root.join("materials/M0123456789abcdef0123456789abcdef/original/old.wav")
    } else {
        root.join("old.wav")
    };
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    wav(&path);
    let neighbor = path.parent().unwrap().join("unknown.keep");
    fs::write(&neighbor, b"outside the exact original receipt").unwrap();
    let reader = cold_store::sealed_reader(&path).unwrap();
    assert!(matches!(
        fs::remove_file(&path).unwrap_err().raw_os_error(),
        Some(32 | 33)
    ));
    assert!(matches!(
        fs::rename(&path, path.with_extension("renamed.wav"))
            .unwrap_err()
            .raw_os_error(),
        Some(32 | 33)
    ));
    let assets = ProjectAssets::shared();
    let mut job = assets.acquire_pin(&root, &path).unwrap();
    let mut inventory = inventory_gate(&root).unwrap();
    let inventory_weak = Arc::downgrade(inventory.handle.as_ref().unwrap());
    let mut artifact = MigrationArtifactLease::capture_native(&root, &path).unwrap();
    assert_eq!(artifact.retirement_status().unwrap(), "idle");
    assert!(artifact.retirement_error().unwrap().is_none());
    artifact.retire_native(&inventory).unwrap();
    let outcome = artifact.outcome.as_ref().unwrap().clone();
    assert_eq!(artifact.retirement_status().unwrap(), "pending");
    inventory.release();
    artifact.release();
    assert_eq!(artifact.retirement_status().unwrap(), "pending");
    drop(artifact);
    assets.collect_for_test();
    assert!(path.exists());
    assert_eq!(outcome.status().unwrap(), ("pending", None));
    assert!(inventory_weak.upgrade().is_some());
    assert_registration_blocked(&root);
    assert_blocked_in_new_process(&root);

    job.release();
    drop(job);
    assets.collect_for_test();
    assert!(path.exists());
    assert_eq!(outcome.status().unwrap(), ("pending", None));
    assert!(inventory_weak.upgrade().is_some());
    assert_registration_blocked(&root);
    assert_blocked_in_new_process(&root);
    drop(reader);
    wait_until(|| !path.exists() && outcome.status().unwrap().0 == "complete");
    wait_until(|| inventory_weak.upgrade().is_none());
    assert_eq!(outcome.status().unwrap(), ("complete", None));
    assert_eq!(
        fs::read(&neighbor).unwrap(),
        b"outside the exact original receipt"
    );
    assert_registration_available(&root);

    // The same actual child which was denied can now register its fresh owner.
    fs::remove_file(temp.path().join("child-ready.json")).unwrap();
    let mut process = child(&root, "live");
    wait_until(|| temp.path().join("child-ready.json").exists());
    let result: Value =
        serde_json::from_slice(&fs::read(temp.path().join("child-ready.json")).unwrap()).unwrap();
    assert!(
        result["instance_id"]
            .as_str()
            .is_some_and(material_paths::valid_id)
    );
    assert_ne!(
        result["pid"].as_u64().unwrap(),
        u64::from(std::process::id())
    );
    fs::write(
        temp.path().join("child-stop"),
        "exit after actual registration",
    )
    .unwrap();
    assert_eq!(process.wait().unwrap().code(), Some(77));
}

#[test]
fn complete_six_leaf_stem_queue_keeps_lock_until_final_job_and_exact_deletion() {
    let (_temp, root) = root();
    wav(&root.join("old.wav"));
    let source = format!(
        "samples/old.wav|sha256-v1:{:x}",
        Sha256::digest(fs::read(root.join("old.wav")).unwrap())
    );
    let directory = root.join("stems/#216");
    stems(&directory, &source);
    let assets = ProjectAssets::shared();
    let mut job = assets.acquire_pin(&root, &directory).unwrap();
    let mut inventory = inventory_gate(&root).unwrap();
    let inventory_weak = Arc::downgrade(inventory.handle.as_ref().unwrap());
    let mut artifact = MigrationArtifactLease::capture_native(&root, &directory).unwrap();
    assert_eq!(artifact.sealed.as_ref().unwrap().receipt.files.len(), 6);
    artifact.retire_native(&inventory).unwrap();
    let outcome = artifact.outcome.as_ref().unwrap().clone();
    inventory.release();
    artifact.release();
    drop(artifact);
    assets.collect_for_test();
    assert!(STEM_NAMES.iter().all(|name| directory.join(name).exists()));
    assert_eq!(outcome.status().unwrap().0, "pending");
    assert_registration_blocked(&root);

    job.release();
    assets.collect_for_test();
    wait_until(|| !directory.exists() && outcome.status().unwrap().0 == "complete");
    wait_until(|| inventory_weak.upgrade().is_none());
    assert!(
        root.join("old.wav").exists(),
        "unrelated input is outside stem proof"
    );
    assert!(root.exists());
    assert_registration_available(&root);
}

#[test]
fn replacement_and_same_file_content_edit_set_terminal_preserved_error() {
    for replacement in [false, true] {
        let (_temp, root) = root();
        let path = root.join("old.wav");
        wav(&path);
        let identity = project_assets::capture_identity(&path).unwrap();
        let assets = ProjectAssets::shared();
        let mut job = assets.acquire_pin(&root, &path).unwrap();
        let mut inventory = inventory_gate(&root).unwrap();
        let inventory_weak = Arc::downgrade(inventory.handle.as_ref().unwrap());
        let mut artifact = MigrationArtifactLease::capture_native(&root, &path).unwrap();
        artifact.retire_native(&inventory).unwrap();
        let outcome = artifact.outcome.as_ref().unwrap().clone();
        inventory.release();
        artifact.release();
        drop(artifact);
        if replacement {
            fs::rename(&path, root.join("preserved-old.wav")).unwrap();
            wav(&path);
            assert_ne!(project_assets::capture_identity(&path).unwrap(), identity);
        } else {
            let mut bytes = fs::read(&path).unwrap();
            *bytes.last_mut().unwrap() ^= 1;
            fs::write(&path, bytes).unwrap();
            assert_eq!(project_assets::capture_identity(&path).unwrap(), identity);
        }
        let current = fs::read(&path).unwrap();
        assert_registration_blocked(&root);
        job.release();
        assets.collect_for_test();
        wait_until(|| outcome.status().unwrap().0 == "error");
        wait_until(|| inventory_weak.upgrade().is_none());
        assert!(outcome.status().unwrap().1.is_some());
        assert_eq!(fs::read(&path).unwrap(), current);
        if replacement {
            assert!(root.join("preserved-old.wav").exists());
        }
        assets.collect_for_test();
        assert_eq!(outcome.status().unwrap().0, "error");
        assert_registration_available(&root);
    }
}

#[test]
fn wrong_root_or_released_inventory_rejects_before_any_retirement_mutation() {
    let (_temp, root) = root();
    let (_other_temp, other_root) = self::root();
    let path = root.join("old.wav");
    wav(&path);
    let before = fs::read(&path).unwrap();
    let mut artifact = MigrationArtifactLease::capture_native(&root, &path).unwrap();
    let mut other_inventory = inventory_gate(&other_root).unwrap();
    assert!(artifact.retire_native(&other_inventory).is_err());
    assert!(artifact.outcome.is_none());
    assert_eq!(artifact.retirement_status().unwrap(), "idle");
    other_inventory.release();
    assert!(artifact.retire_native(&other_inventory).is_err());
    ProjectAssets::shared().collect_for_test();
    assert!(artifact.outcome.is_none());
    assert_eq!(fs::read(&path).unwrap(), before);
    let mut inventory = inventory_gate(&root).unwrap();
    artifact.retire_native(&inventory).unwrap();
    let outcome = artifact.outcome.as_ref().unwrap().clone();
    inventory.release();
    artifact.release();
    wait_until(|| !path.exists() && outcome.status().unwrap().0 == "complete");
}

#[test]
fn pcm_store_handoff_keeps_inventory_after_gc_until_actual_old_voice_and_job_end() {
    let (_temp, root) = root();
    wav(&root.join("old.wav"));
    let material = prepare_material(&root, &root.join("old.wav"), 48_000, 2, &|| false).unwrap();
    let cache = material.lease.cache_path.clone();
    let original = material.lease.original_path.clone();
    let assets = ProjectAssets::shared();
    let engine = Arc::new(());
    assets
        .retain_cold(
            material.lease.clone(),
            &material.sample,
            Arc::downgrade(&engine),
        )
        .unwrap();
    // This preparation never delivered a saved assignment. Only its actual PCM
    // readers below remain; clearing pending metadata does not revoke them.
    assets.orphan_cold(&material.lease);
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
                .is_some_and(|sample| Arc::ptr_eq(&sample.samples, &job_reader.samples))
    }));
    let mut inventory = inventory_gate(&root).unwrap();
    let inventory_weak = Arc::downgrade(inventory.handle.as_ref().unwrap());
    let mut artifact = MigrationArtifactLease::capture_native(&root, &cache).unwrap();
    assert_eq!(artifact.sealed.as_ref().unwrap().receipt.files.len(), 3);
    artifact.retire_native(&inventory).unwrap();
    let outcome = artifact.outcome.as_ref().unwrap().clone();
    inventory.release();
    artifact.release();
    drop(artifact);
    drop(material);
    assets.collect_for_test();
    wait_until(|| cold_store::has_verified_retirement(&cache, &outcome));
    assert!(PCM_NAMES.iter().all(|name| cache.join(name).exists()));
    assert_eq!(outcome.status().unwrap().0, "pending");
    assert!(inventory_weak.upgrade().is_some());
    assert_registration_blocked(&root);

    drop(job_reader);
    assets.collect_for_test();
    assert!(
        cache.exists(),
        "actual old mixer voice still owns its complete PCM reader"
    );
    assert!(cold_store::has_verified_retirement(&cache, &outcome));
    assert_eq!(outcome.status().unwrap().0, "pending");
    assert_registration_blocked(&root);
    mixer.stop_sample(0);
    assets.collect_for_test();
    wait_until(|| !cache.exists() && outcome.status().unwrap().0 == "complete");
    wait_until(|| inventory_weak.upgrade().is_none());
    assert!(!cold_store::has_verified_retirement(&cache, &outcome));
    assert!(original.exists() && root.join("old.wav").exists());
    assert_registration_available(&root);
}

#[test]
fn new_saved_original_owner_cancels_queued_delete_with_terminal_preservation() {
    let (_temp, root) = root();
    let path = root.join("old.wav");
    wav(&path);
    let before = fs::read(&path).unwrap();
    let assets = ProjectAssets::shared();
    let mut job = assets.acquire_pin(&root, &path).unwrap();
    let mut inventory = inventory_gate(&root).unwrap();
    let inventory_weak = Arc::downgrade(inventory.handle.as_ref().unwrap());
    let mut artifact = MigrationArtifactLease::capture_native(&root, &path).unwrap();
    artifact.retire_native(&inventory).unwrap();
    let outcome = artifact.outcome.as_ref().unwrap().clone();
    inventory.release();
    artifact.release();
    drop(artifact);
    assert_registration_blocked(&root);
    let mut owner = assets.acquire(&root, &path).unwrap();
    assert_eq!(outcome.status().unwrap().0, "error");
    assert!(outcome.status().unwrap().1.is_some());
    wait_until(|| inventory_weak.upgrade().is_none());
    job.release();
    owner.release();
    assets.collect_for_test();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_registration_available(&root);
}

#[test]
fn delivered_original_ack_cancels_queued_delete_without_losing_outcome() {
    let (_temp, root) = root();
    let path = root.join("old.wav");
    wav(&path);
    let before = fs::read(&path).unwrap();
    let assets = ProjectAssets::shared();
    let mut delivered = assets.acquire_pin(&root, &path).unwrap();
    let mut inventory = inventory_gate(&root).unwrap();
    let inventory_weak = Arc::downgrade(inventory.handle.as_ref().unwrap());
    let mut artifact = MigrationArtifactLease::capture_native(&root, &path).unwrap();
    artifact.retire_native(&inventory).unwrap();
    let outcome = artifact.outcome.as_ref().unwrap().clone();
    inventory.release();
    artifact.release();
    drop(artifact);
    assert_registration_blocked(&root);
    delivered.acknowledge(plain_path(&path)).unwrap();
    assert_eq!(outcome.status().unwrap().0, "error");
    assert!(
        outcome
            .status()
            .unwrap()
            .1
            .as_deref()
            .is_some_and(|error| error.contains("delivered"))
    );
    wait_until(|| inventory_weak.upgrade().is_none());
    delivered.release();
    assets.collect_for_test();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_registration_available(&root);
}

#[test]
fn reclaimed_stem_owner_sets_terminal_preservation_before_releasing_queue_lock() {
    let (_temp, root) = root();
    let directory = root.join("stems/#1");
    wav(&root.join("old.wav"));
    let source = format!(
        "samples/old.wav|sha256-v1:{:x}",
        Sha256::digest(fs::read(root.join("old.wav")).unwrap())
    );
    stems(&directory, &source);
    let before: Vec<_> = STEM_NAMES
        .iter()
        .map(|name| fs::read(directory.join(name)).unwrap())
        .collect();
    let assets = ProjectAssets::shared();
    let mut reserved = assets.acquire_pin(&root, &directory).unwrap();
    let mut inventory = inventory_gate(&root).unwrap();
    let inventory_weak = Arc::downgrade(inventory.handle.as_ref().unwrap());
    let mut artifact = MigrationArtifactLease::capture_native(&root, &directory).unwrap();
    artifact.retire_native(&inventory).unwrap();
    let outcome = artifact.outcome.as_ref().unwrap().clone();
    inventory.release();
    artifact.release();
    drop(artifact);
    assert_registration_blocked(&root);
    reserved.reclaim_stems(plain_path(&directory)).unwrap();
    assert_eq!(outcome.status().unwrap().0, "error");
    assert!(
        outcome
            .status()
            .unwrap()
            .1
            .as_deref()
            .is_some_and(|error| error.contains("stem"))
    );
    wait_until(|| inventory_weak.upgrade().is_none());
    reserved.release();
    assets.collect_for_test();
    assert_eq!(
        STEM_NAMES
            .iter()
            .map(|name| fs::read(directory.join(name)).unwrap())
            .collect::<Vec<_>>(),
        before
    );
    assert_registration_available(&root);
}

#[test]
fn ordinary_and_verified_duplicate_requests_preserve_original_queue_outcome_and_lock() {
    let (_temp, root) = root();
    let path = root.join("old.wav");
    wav(&path);
    let assets = ProjectAssets::shared();
    let mut job = assets.acquire_pin(&root, &path).unwrap();
    let mut inventory = inventory_gate(&root).unwrap();
    let inventory_weak = Arc::downgrade(inventory.handle.as_ref().unwrap());
    let mut first = MigrationArtifactLease::capture_native(&root, &path).unwrap();
    first.retire_native(&inventory).unwrap();
    let outcome = first.outcome.as_ref().unwrap().clone();
    let mut second = MigrationArtifactLease::reopen_native(&root, &encoded(&first)).unwrap();
    second.retire_native(&inventory).unwrap();
    assert!(Arc::ptr_eq(second.outcome.as_ref().unwrap(), &outcome));
    assets.retire(&root, &path, false).unwrap();
    inventory.release();
    first.release();
    second.release();
    drop(first);
    drop(second);
    assets.collect_for_test();
    assert_eq!(outcome.status().unwrap(), ("pending", None));
    assert!(inventory_weak.upgrade().is_some() && path.exists());
    assert_registration_blocked(&root);
    job.release();
    assets.collect_for_test();
    wait_until(|| !path.exists() && outcome.status().unwrap().0 == "complete");
    wait_until(|| inventory_weak.upgrade().is_none());
    assert_registration_available(&root);
}
