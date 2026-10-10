//! Physical queue/lock oracles; no device, callback mock or serialized authority.
#![cfg(windows)]

use super::*;
use crate::audio_engine::cold_store;

fn assert_registration_blocked(root: &Path) {
    let config = root.parent().unwrap().join("project.config.json");
    let error = MigrationProjectGuard::create_native(root, &config)
        .err()
        .expect("actual queued inventory lock must deny registration");
    assert!(matches!(error.raw_os_error(), Some(32 | 33)));
}

fn assert_registration_available(root: &Path) {
    let config = root.parent().unwrap().join("project.config.json");
    let mut guard = MigrationProjectGuard::create_native(root, &config).unwrap();
    guard.release();
}

fn assert_blocked_in_new_process(root: &Path) {
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
    let (temp, root) = root();
    let path = root.join("old.wav");
    wav(&path);
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
    wait_until(|| !path.exists() && outcome.status().unwrap().0 == "complete");
    wait_until(|| inventory_weak.upgrade().is_none());
    assert_eq!(outcome.status().unwrap(), ("complete", None));
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
