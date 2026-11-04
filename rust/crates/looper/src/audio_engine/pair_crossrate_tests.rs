//! Real decoder, five-output conversion and no-device recovery oracles.
#![cfg(windows)]
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn wav(path: &Path, frames: usize, rate: u32) {
    let size = (frames * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(size + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for i in 0..frames {
        bytes.extend_from_slice(&((i as i16).wrapping_mul(53)).to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
}

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    material: material_migration::PreparedMigrationMaterial,
    version: String,
    wav_reference: String,
}
impl Fixture {
    fn new(frames: usize) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let input = root.join("input.wav");
        wav(&input, frames, 44_100);
        let material =
            material_migration::prepare_material(&root, &input, 96_000, 1, &|| false).unwrap();
        let data = material.metadata();
        let version = format!(
            "{}|sha256-v1:{}",
            data["new_reference"].as_str().unwrap(),
            data["original"]["sha256"].as_str().unwrap()
        );
        let material_root =
            material_paths::material_root_for_original(&root, &material.lease.original_path)
                .unwrap();
        let directory = material_root.join("stems/.ready-0123456789abcdef0123456789abcdef");
        fs::create_dir_all(&directory).unwrap();
        let raw = sample_loader::fft_stream::output_frames(frames, 44_100, 48_000).unwrap();
        let mut hashes = serde_json::Map::new();
        for name in stem_cache::STEM_FILE_NAMES {
            let path = directory.join(format!("{name}.wav"));
            wav(&path, raw, 48_000);
            hashes.insert(
                name.into(),
                serde_json::json!(format!("{:x}", Sha256::digest(fs::read(path).unwrap()))),
            );
        }
        fs::write(directory.join(".complete.json"),serde_json::to_vec(&serde_json::json!({"schema":"stem-set-sha256-v1","source_version":version,"stems":hashes})).unwrap()).unwrap();
        let wav_reference = format!(
            "samples/{}",
            directory
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        );
        Self {
            _temp: temp,
            root,
            material,
            version,
            wav_reference,
        }
    }
    fn pair(&self) -> stem_pair::VerifiedStemPair {
        stem_pair::prepare_complete_pair(
            &self.root,
            &self.material,
            &self.version,
            &self.wav_reference,
            &|| false,
        )
        .unwrap()
    }
}

#[test]
fn pair_crossrate_actual_original_clock_five_completions_and_recovery() {
    for frames in [1, 44_101] {
        let f = Fixture::new(frames);
        let pair = f.pair();
        let descriptor = pair.descriptor.clone();
        let reference = pair.descriptor_reference.clone();
        let conversion = descriptor.content.conversion.resampling.as_ref().unwrap();
        assert_eq!(descriptor.content.conversion.schema_version, 2);
        assert_eq!(conversion.original_frames, frames as u64);
        assert_eq!(conversion.original_rate_hz, 44_100);
        assert_eq!(conversion.trimmed_tail_frames, 1);
        assert!(conversion.delay_frames > 0);
        for artifact in &descriptor.content.artifacts {
            assert_eq!(artifact.pcm_bytes, conversion.target_frames * 4);
        }
        drop(stem_pair::open_verified_pair(&f.root, &reference, &f.material, &|| false).unwrap());
        // Path-only recovery cannot issue any v2 lease or receipt.
        assert!(stem_pair::verify_recovery_area(&f.root, Path::new(&reference)).is_err());
        let account = resident_accounting::ResidentAccount::new();
        let proof =
            stem_pair::acquire_recovery_clock(&f.root, Path::new(&reference), &account, &|| false)
                .unwrap()
                .unwrap();
        assert_eq!(proof.clock().frames, frames);
        let held = f
            .material
            .lease
            .original_clock(&account, &|| false)
            .unwrap();
        let other_rate = material_migration::prepare_material(
            &f.root,
            &f.material.lease.original_path,
            48_000,
            1,
            &|| false,
        )
        .unwrap();
        let reused = other_rate
            .lease
            .original_clock(&account, &|| false)
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(&held, &reused));
        drop(
            stem_pair::verify_recovery_area_with_proof(
                &f.root,
                Path::new(&reference),
                Some(&proof),
                &|| false,
            )
            .unwrap(),
        );
        drop(pair);
        // A self-consistent rehashed DTO may pass pure metadata validation; it
        // cannot override the separately executed Original decoder.
        let path = f.root.parent().unwrap().join(&reference);
        let mut forged = descriptor.clone();
        let data = forged.content.conversion.resampling.as_mut().unwrap();
        data.original_frames *= 2;
        data.original_rate_hz *= 2;
        forged.stem_set_identity = forged.content.logical_identity().unwrap();
        let manifest_path = f
            .root
            .parent()
            .unwrap()
            .join(&descriptor.pcm_generation)
            .join("manifest.json");
        let original_manifest = fs::read(&manifest_path).unwrap();
        let mut manifest: stem_pair_descriptor::StemPcmManifest =
            serde_json::from_slice(&original_manifest).unwrap();
        manifest.content = forged.content.clone();
        manifest.stem_set_identity = forged.stem_set_identity.clone();
        let encoded = manifest.canonical_bytes().unwrap();
        forged.pcm_manifest_sha256 = format!("{:x}", Sha256::digest(&encoded));
        fs::write(&manifest_path, encoded).unwrap();
        fs::write(&path, forged.canonical_bytes().unwrap()).unwrap();
        assert!(
            stem_pair::acquire_recovery_clock(&f.root, Path::new(&reference), &account, &|| false)
                .is_err()
        );
        assert!(
            stem_pair::verify_recovery_area_with_proof(
                &f.root,
                Path::new(&reference),
                Some(&proof),
                &|| false
            )
            .is_err()
        );
        fs::write(&path, descriptor.canonical_bytes().unwrap()).unwrap();
        fs::write(manifest_path, original_manifest).unwrap();
        drop(proof);
    }
}

#[test]
fn pair_crossrate_real_plan_refuses_headroom_before_decoder_or_output_file() {
    let f = Fixture::new(1);
    let account = resident_accounting::ResidentAccount::new();
    account.set_host_memory_for_test(Ok(resident_host_memory::HostHeadroom {
        available_physical: 0,
        caller_available_commit: 0,
        system_available_commit: 0,
    }));
    let plan = sample_loader::StreamDecodePlan::probe(
        cold_store::sealed_reader(&f.material.lease.original_path).unwrap(),
        &f.material.lease.original_path,
    )
    .unwrap();
    assert!(plan.working_peak_bytes() > 64 * 1024);
    assert!(plan.reserve(&account).is_err());
    assert!(
        original_clock::OriginalClockProof::acquire(
            &f.root,
            &f.material.lease.original_path,
            &account,
            &|| false
        )
        .is_err()
    );
    account.clear_host_memory_for_test();
    assert!(
        original_clock::OriginalClockProof::acquire(
            &f.root,
            &f.material.lease.original_path,
            &account,
            &|| true
        )
        .is_err()
    );
    let proof = original_clock::OriginalClockProof::acquire(
        &f.root,
        &f.material.lease.original_path,
        &account,
        &|| false,
    )
    .unwrap();
    assert_eq!(proof.clock().frames, 1);
    let paths = cold_store::take_temporary_paths_for_test();
    assert!(!paths.is_empty() && paths.iter().all(|path| !path.exists()));
    let f = Fixture::new(44_101);
    let before = resident_host_memory::promised_bytes_for_test();
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    cold_store::cancel_temporary_for_test(Some(cancelled.clone()));
    assert!(
        original_clock::OriginalClockProof::acquire(
            &f.root,
            &f.material.lease.original_path,
            &account,
            &|| cancelled.load(std::sync::atomic::Ordering::Acquire)
        )
        .is_err()
    );
    cold_store::cancel_temporary_for_test(None);
    assert!(cancelled.load(std::sync::atomic::Ordering::Acquire));
    let paths = cold_store::take_temporary_paths_for_test();
    assert!(!paths.is_empty() && paths.iter().all(|path| !path.exists()));
    assert_eq!(resident_host_memory::promised_bytes_for_test(), before);
    cold_store::fail_temporary_io_for_test(true);
    let result = original_clock::OriginalClockProof::acquire(
        &f.root,
        &f.material.lease.original_path,
        &account,
        &|| false,
    );
    cold_store::fail_temporary_io_for_test(false);
    assert!(
        result
            .err()
            .unwrap()
            .contains("temporary PCM write refusal")
    );
    let paths = cold_store::take_temporary_paths_for_test();
    assert!(!paths.is_empty() && paths.iter().all(|path| !path.exists()));
    assert_eq!(resident_host_memory::promised_bytes_for_test(), before);
}

#[test]
fn pair_crossrate_actual_recovery_lease_retirement_on_this_engine_lane() {
    use material_migration_recovery::{ArtifactRecoveryWork, MigrationProjectGuard};
    use std::sync::atomic::Ordering;
    pyo3::Python::initialize();
    let f = Fixture::new(44_101);
    let pair = f.pair();
    let reference = pair.descriptor_reference.clone();
    let target = f.root.parent().unwrap().join(&reference);
    drop(pair);
    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(f.root.parent().unwrap()).unwrap();
    let mut engine = AudioEngine::new().unwrap();
    assert!(engine.stream_handle.is_none());
    let epoch = engine
        .input_runtime_ownership
        .output_change_epoch
        .load(Ordering::Acquire);
    let work = ArtifactRecoveryWork::capture(&engine, reference.clone(), false).unwrap();
    let mut lease = work.prepare().unwrap();
    let encoded = lease.receipt_json().unwrap();
    lease.release();
    drop(lease);
    let mut reopened = ArtifactRecoveryWork::capture(&engine, encoded, true)
        .unwrap()
        .prepare()
        .unwrap();
    engine
        .input_runtime_ownership
        .output_change_epoch
        .store(epoch + 2, Ordering::Release);
    // Completed Original evidence survives an unrelated output epoch change.
    let config = f.root.parent().unwrap().join("recovery.json");
    fs::write(&config, b"{}").unwrap();
    let mut project = pyo3::Python::attach(|py| {
        MigrationProjectGuard::new(
            py,
            f.root.to_string_lossy().into(),
            config.to_string_lossy().into(),
        )
        .unwrap()
    });
    let mut inventory = pyo3::Python::attach(|py| project.lock_inventory(py).unwrap());
    pyo3::Python::attach(|py| reopened.retire(py, &inventory).unwrap());
    inventory.release();
    reopened.release();
    for _ in 0..500 {
        if reopened.retirement_status().unwrap() == "complete" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(reopened.retirement_status().unwrap(), "complete");
    assert!(!target.exists());
    let stale = ArtifactRecoveryWork::capture(&engine, reference.clone(), false).unwrap();
    engine
        .input_runtime_ownership
        .output_change_epoch
        .store(epoch + 3, Ordering::Release);
    assert!(stale.prepare().is_err());
    assert!(ArtifactRecoveryWork::capture(&engine, reference, false).is_err());
    engine
        .input_runtime_ownership
        .output_change_epoch
        .store(epoch + 4, Ordering::Release);
    engine.cold_cancelled.store(true, Ordering::Release);
    assert!(ArtifactRecoveryWork::capture(&engine, "samples/anything".into(), false).is_err());
    engine.cold_jobs.shutdown_checked().unwrap();
    drop(reopened);
    project.release();
    drop(project);
    drop(engine);
    std::env::set_current_dir(cwd).unwrap();
}

#[test]
fn pair_crossrate_recovery_occupies_this_lane_until_actual_owner_end() {
    use std::sync::{atomic::Ordering, mpsc};
    let mut engine = AudioEngine::new().unwrap();
    let epoch = engine
        .input_runtime_ownership
        .output_change_epoch
        .load(Ordering::Acquire);
    let handle = engine.cold_jobs.handle();
    let nested = handle.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let job = std::thread::spawn(move || {
        handle.execute_recovery_owned_until_terminal(
            epoch,
            || false,
            move || {
                assert!(
                    nested
                        .execute_recovery_owned_until_terminal(epoch, || false, || Ok(()))
                        .is_err()
                );
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            },
        )
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(engine.cold_jobs.with_idle(|| Ok(())).is_err());
    release_tx.send(()).unwrap();
    job.join().unwrap().unwrap();
    engine.cold_jobs.shutdown_checked().unwrap();
    assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
    engine.cold_jobs.with_idle(|| Ok(())).unwrap();
}

#[test]
fn pair_crossrate_qualified_foreign_copy_reuses_complete_raw_geometry() {
    let f = Fixture::new(44_101);
    let pair = f.pair();
    // Restore an actual historical typed material, rather than defeating the
    // ordinary importer's intentional same-content deduplication.
    let input = f
        .root
        .join("materials/M11111111111111111111111111111111/original/copy.wav");
    fs::create_dir_all(input.parent().unwrap()).unwrap();
    fs::write(&input, fs::read(&f.material.lease.original_path).unwrap()).unwrap();
    let mut transaction =
        cold_store::ColdTransaction::capture(&f.root, &input, false, &|| false).unwrap();
    let sample = material_migration::prepare_pcm(
        &mut transaction,
        &input,
        96_000,
        1,
        cold_jobs::PCM_LIMIT_BYTES,
        None,
        &|| false,
        &mut |_| {},
    )
    .unwrap();
    transaction.commit(&|| false).unwrap();
    let target = material_migration::PreparedMigrationMaterial::from_current(
        &f.root,
        sample,
        transaction.into_lease(),
    )
    .unwrap();
    assert_ne!(target.lease.material_id, f.material.lease.material_id);
    assert_ne!(target.lease.original_path, f.material.lease.original_path);
    let metadata = target.metadata();
    let version = format!(
        "{}|sha256-v1:{}",
        metadata["new_reference"].as_str().unwrap(),
        metadata["original"]["sha256"].as_str().unwrap()
    );
    let proof = stem_pair::verify_pair_rebind(
        &f.root,
        &pair.descriptor_reference,
        &f.wav_reference,
        &f.version,
        &target,
        &|| false,
    )
    .unwrap();
    let generation = "11111111111111111111111111111111";
    let copied = material_migration_stems::copy_pair_generation(
        &f.root,
        &target,
        &proof,
        &version,
        generation,
        &|| false,
    )
    .unwrap();
    assert!(copied.created);
    let reopened = stem_pair::prepare_complete_pair(
        &f.root,
        &target,
        &version,
        &copied.cache_reference,
        &|| false,
    )
    .unwrap();
    assert_eq!(
        reopened.descriptor.content.source,
        pair.descriptor.content.source
    );
    assert!(reopened.descriptor.content.conversion.resampling.is_some());
    let retry = material_migration_stems::copy_pair_generation(
        &f.root,
        &target,
        &proof,
        &version,
        generation,
        &|| false,
    )
    .unwrap();
    assert!(!retry.created);
    assert_eq!(retry.cache_reference, copied.cache_reference);
    let sibling = material_migration_stems::copy_pair_generation(
        &f.root,
        &target,
        &proof,
        &version,
        "22222222222222222222222222222222",
        &|| false,
    )
    .unwrap();
    assert!(!sibling.created);
    assert_eq!(sibling.cache_reference, copied.cache_reference);
}

#[test]
fn pair_crossrate_fresh_receipt_rejects_equal_bytes_original_replacement() {
    use material_migration_recovery::ArtifactRecoveryWork;
    pyo3::Python::initialize();
    let f = Fixture::new(1);
    let pair = f.pair();
    let reference = pair.descriptor_reference.clone();
    drop(pair);
    let original = f.material.lease.original_path.clone();
    let bytes = fs::read(&original).unwrap();
    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(f.root.parent().unwrap()).unwrap();
    let mut engine = AudioEngine::new().unwrap();
    let mut lease = ArtifactRecoveryWork::capture(&engine, reference.clone(), false)
        .unwrap()
        .prepare()
        .unwrap();
    let encoded = lease.receipt_json().unwrap();
    let mut missing: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    missing.as_object_mut().unwrap().remove("original_clock");
    assert!(
        ArtifactRecoveryWork::capture(&engine, missing.to_string(), true)
            .unwrap()
            .prepare()
            .is_err()
    );
    lease.release();
    drop(lease);
    drop(f.material);
    let backup = original.with_file_name("old-file-id.wav");
    fs::rename(&original, &backup).unwrap();
    fs::write(&original, &bytes).unwrap();
    assert!(
        ArtifactRecoveryWork::capture(&engine, encoded, true)
            .unwrap()
            .prepare()
            .is_err()
    );
    let mut current = ArtifactRecoveryWork::capture(&engine, reference, false)
        .unwrap()
        .prepare()
        .unwrap();
    current.release();
    drop(current);
    engine.cold_jobs.shutdown_checked().unwrap();
    drop(engine);
    std::env::set_current_dir(cwd).unwrap();
}
