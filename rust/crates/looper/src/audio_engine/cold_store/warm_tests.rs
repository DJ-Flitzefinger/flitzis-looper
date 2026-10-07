#![cfg(windows)]
use super::*;
use crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES;
use crate::audio_engine::sample_loader::{decode_audio_snapshot, prepare_playback};
use std::sync::atomic::AtomicBool;

struct Fixture {
    _temp: tempfile::TempDir,
    samples: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new(rate: u32, channels: usize) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("external.wav");
        let frames = 11_023;
        let bytes = (frames * channels * 2) as u32;
        let mut writer = File::create(&source).unwrap();
        writer.write_all(b"RIFF").unwrap();
        writer.write_all(&(bytes + 36).to_le_bytes()).unwrap();
        writer.write_all(b"WAVEfmt \x10\0\0\0\x01\0").unwrap();
        writer.write_all(&(channels as u16).to_le_bytes()).unwrap();
        writer.write_all(&rate.to_le_bytes()).unwrap();
        writer
            .write_all(&(rate * channels as u32 * 2).to_le_bytes())
            .unwrap();
        writer
            .write_all(&(channels as u16 * 2).to_le_bytes())
            .unwrap();
        writer.write_all(&16_u16.to_le_bytes()).unwrap();
        writer.write_all(b"data").unwrap();
        writer.write_all(&bytes.to_le_bytes()).unwrap();
        for frame in 0..frames {
            for channel in 0..channels {
                let value: i16 = if [0, 17, 1023, frames - 1].contains(&frame) {
                    (channel as i16 + 1) * 8192
                } else {
                    ((frame % 4096) as i16 - 2048) * (channel as i16 + 1)
                };
                writer.write_all(&value.to_le_bytes()).unwrap();
            }
        }
        Self {
            samples: temp.path().join("samples"),
            source,
            _temp: temp,
        }
    }

    fn cold(&self, rate: u32, channels: usize) -> (SampleBuffer, CommittedColdLease) {
        let transaction =
            ColdTransaction::capture(&self.samples, &self.source, true, &|| false).unwrap();
        let _gate = transaction
            .preparation_gate(rate, channels, &|| false)
            .unwrap();
        self.finish_cold(transaction, rate, channels)
    }

    fn finish_cold(
        &self,
        mut transaction: ColdTransaction,
        rate: u32,
        channels: usize,
    ) -> (SampleBuffer, CommittedColdLease) {
        let decoded = decode_audio_snapshot(
            transaction.snapshot_file().unwrap(),
            &self.source,
            rate,
            PCM_LIMIT_BYTES,
            &|| false,
            |_| {},
        )
        .unwrap();
        let (sample, transform) =
            prepare_playback(&decoded, channels, rate, PCM_LIMIT_BYTES, &|| false, |_| {}).unwrap();
        transaction
            .write_pcm_artifacts(
                PcmArtifactInput {
                    samples: &decoded.samples,
                    rate_hz: decoded.rate_hz,
                    channels: decoded.channels,
                    provenance: decoded.decoder.to_json(),
                },
                PcmArtifactInput {
                    samples: &sample.samples,
                    rate_hz: rate,
                    channels,
                    provenance: json!({"processing":"full-buffer-playback-v1"}),
                },
                transform.to_json(),
                &|| false,
            )
            .unwrap();
        transaction.commit(&|| false).unwrap();
        transaction.bind_pcm(&sample.samples);
        (sample, transaction.into_lease())
    }

    fn warm(
        &self,
        path: &Path,
        import: bool,
        rate: u32,
        channels: usize,
    ) -> (SampleBuffer, CommittedColdLease) {
        let mut transaction =
            ColdTransaction::capture(&self.samples, path, import, &|| false).unwrap();
        let _gate = transaction
            .preparation_gate(rate, channels, &|| false)
            .unwrap();
        let sample = transaction
            .try_reuse(rate, channels, PCM_LIMIT_BYTES, &|| false)
            .unwrap()
            .expect("complete compatible cache");
        transaction.commit(&|| false).unwrap();
        transaction.bind_pcm(&sample.samples);
        (sample, transaction.into_lease())
    }

    fn rejected(&self, rate: u32, channels: usize) -> ColdTransaction {
        let mut transaction =
            ColdTransaction::capture(&self.samples, &self.source, true, &|| false).unwrap();
        let _gate = transaction
            .preparation_gate(rate, channels, &|| false)
            .unwrap();
        assert!(
            transaction
                .try_reuse(rate, channels, PCM_LIMIT_BYTES, &|| false)
                .unwrap()
                .is_none()
        );
        transaction
    }
}

fn wait_absent(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !path.exists(),
        "deferred retirement did not drain: {}",
        path.display()
    );
}

fn rewrite_manifest(path: &Path, mutate: impl FnOnce(&mut Value)) -> PathBuf {
    let mut saved: Value =
        serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
    mutate(&mut saved["descriptor"]);
    let decoder_identity = descriptor_digest(&saved["descriptor"]["decoder"]).unwrap();
    saved["decoder_identity"] = json!(decoder_identity);
    saved["descriptor"]["playback"]["parent_identity"] = saved["decoder_identity"].clone();
    let identity = descriptor_digest(&saved["descriptor"]).unwrap();
    saved["identity"] = json!(identity);
    fs::write(
        path.join("manifest.json"),
        serde_json::to_vec(&canonical(&saved)).unwrap(),
    )
    .unwrap();
    let new_path = path.parent().unwrap().join(format!(
        "{identity}-fault-{}",
        NEXT_GENERATION.fetch_add(1, Ordering::Relaxed)
    ));
    fs::rename(path, &new_path).unwrap();
    new_path
}

#[test]
fn fresh_warm_leases_verify_full_content_and_share_pcm_at_all_device_rates() {
    for source_rate in [44_100, 48_000, 96_000] {
        for output_rate in [44_100, 48_000, 96_000] {
            for channels in [1, 2] {
                let fixture = Fixture::new(source_rate, channels);
                let (cold, first) = fixture.cold(output_rate, channels);
                let (warm, second) = fixture.warm(&fixture.source, true, output_rate, channels);
                assert!(Arc::ptr_eq(&cold.samples, &warm.samples));
                assert_eq!(&*cold.samples, &*warm.samples);
                assert_eq!(first.cache_path, second.cache_path);
                assert_ne!(first.original_path, second.original_path);
                assert_eq!(first.manifest.identity, second.manifest.identity);
                assert!(second.integrity.warm);
                assert_eq!(
                    second.integrity.decoder_verify_bytes,
                    11_023 * channels as u64 * 4
                );
                assert_eq!(
                    second.integrity.playback_verify_bytes,
                    warm.samples.len() as u64 * 4
                );
                assert!(second.integrity.manifest_verify_bytes > 0);
                assert_eq!(second.integrity.playback_read_bytes, 0);
                assert_eq!(
                    second.integrity.source_copied_bytes,
                    fs::metadata(&fixture.source).unwrap().len()
                );
                assert_eq!(
                    second.integrity.snapshot_verify_bytes,
                    second.integrity.source_copied_bytes
                );
                assert_eq!(
                    second.integrity.original_copied_bytes,
                    second.integrity.source_copied_bytes
                );
                assert_eq!(
                    second.integrity.original_verify_bytes,
                    second.integrity.source_copied_bytes
                );
                assert!(second.integrity.wall_nanos > 0);
                assert!(second.integrity.cpu_nanos.is_some());
                let cache = first.cache_path.clone();
                drop((cold, warm, first, second));
                assert!(
                    cache.exists(),
                    "durable cache survives ordinary lease shutdown"
                );
            }
        }
    }
}

#[test]
fn warm_restore_uses_fresh_sealed_files_and_reads_pcm_when_previous_arc_is_gone() {
    let fixture = Fixture::new(44_100, 2);
    let (sample, lease) = fixture.cold(48_000, 2);
    let original = lease.original_path.clone();
    let cache = lease.cache_path.clone();
    let expected = sample.samples.to_vec();
    drop((sample, lease));
    let (warm, lease) = fixture.warm(&original, false, 48_000, 2);
    assert_eq!(&*warm.samples, expected);
    assert_eq!(lease.cache_path, cache);
    assert_eq!(lease.original_path, original);
    assert_eq!(lease.integrity.original_copied_bytes, 0);
    assert_eq!(lease.integrity.original_verify_bytes, 0);
    assert_eq!(
        lease.integrity.playback_read_bytes,
        warm.samples.len() as u64 * 4
    );
    for path in [
        original,
        cache.join("decoder.f32le"),
        cache.join("playback.f32le"),
        cache.join("manifest.json"),
    ] {
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(fs::remove_file(&path).is_err());
    }
}

#[test]
fn same_size_corrupt_partial_and_extra_eof_entries_regenerate_without_overwrite() {
    for (file_name, mode) in [
        ("decoder.f32le", 0),
        ("playback.f32le", 0),
        ("manifest.json", 0),
        ("manifest.json", 1),
        ("decoder.f32le", 2),
        ("playback.f32le", 2),
    ] {
        let fixture = Fixture::new(44_100, 2);
        let (sample, lease) = fixture.cold(48_000, 2);
        let old = lease.cache_path.clone();
        drop((sample, lease));
        let path = old.join(file_name);
        if mode == 1 {
            fs::remove_file(&path).unwrap();
        } else {
            let mut bytes = fs::read(&path).unwrap();
            if mode == 0 {
                let index = bytes.len() / 2;
                bytes[index] ^= 1;
            } else {
                bytes.extend_from_slice(b"tail");
            }
            fs::write(&path, bytes).unwrap();
        }
        let damaged = fs::read(&path).ok();
        let transaction = fixture.rejected(48_000, 2);
        let (sample, regenerated) = fixture.finish_cold(transaction, 48_000, 2);
        assert_ne!(regenerated.cache_path, old);
        assert_eq!(fs::read(&path).ok(), damaged);
        assert!(!sample.samples.is_empty());
    }
}

#[test]
fn self_consistent_forged_mono_versions_and_actual_decoder_configuration_are_rejected() {
    for pointer in [
        "/decoder/pcm/mono_sha256",
        "/playback/pcm/mono_sha256",
        "/decoder/pcm/provenance/version",
        "/decoder/pcm/provenance/codec",
        "/decoder/pcm/provenance/codec_config_sha256",
        "/playback/transform/resampler",
        "/playback/transform/trimmed_algorithmic_delay_frames",
    ] {
        let fixture = Fixture::new(44_100, 2);
        let (sample, lease) = fixture.cold(48_000, 2);
        let old = lease.cache_path.clone();
        drop((sample, lease));
        let corrupt = rewrite_manifest(&old, |descriptor| {
            *descriptor.pointer_mut(pointer).unwrap() = json!("self-consistent-fault");
        });
        drop(fixture.rejected(48_000, 2));
        assert!(corrupt.exists());
    }
}

#[test]
fn device_change_and_changed_actual_original_cannot_reuse_old_pcm() {
    let fixture = Fixture::new(44_100, 1);
    let (sample, lease) = fixture.cold(48_000, 2);
    drop(fixture.rejected(96_000, 2));
    drop(fixture.rejected(48_000, 1));
    let mut bytes = fs::read(&fixture.source).unwrap();
    let index = bytes.len() - 2;
    bytes[index] ^= 0x40;
    fs::write(&fixture.source, bytes).unwrap();
    let changed = fixture.rejected(48_000, 2);
    assert_ne!(
        changed.source_digest(),
        lease.manifest.descriptor["decoder"]["original"]["sha256"]
            .as_str()
            .unwrap()
    );
    drop((changed, sample, lease));
}

#[test]
fn warm_external_aba_after_capture_keeps_snapshot_and_project_original_identity() {
    let fixture = Fixture::new(44_100, 2);
    let (sample, lease) = fixture.cold(48_000, 2);
    let original = fs::read(&fixture.source).unwrap();
    let mut transaction =
        ColdTransaction::capture(&fixture.samples, &fixture.source, true, &|| false).unwrap();
    let backup = fixture.source.with_extension("aba");
    fs::rename(&fixture.source, &backup).unwrap();
    fs::write(&fixture.source, b"different source").unwrap();
    fs::remove_file(&fixture.source).unwrap();
    fs::rename(&backup, &fixture.source).unwrap();
    let _gate = transaction.preparation_gate(48_000, 2, &|| false).unwrap();
    let warm = transaction
        .try_reuse(48_000, 2, PCM_LIMIT_BYTES, &|| false)
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(&sample.samples, &warm.samples));
    transaction.commit(&|| false).unwrap();
    let new_lease = transaction.into_lease();
    assert_eq!(fs::read(&new_lease.original_path).unwrap(), original);
    assert_eq!(lease.manifest.identity, new_lease.manifest.identity);
}

#[test]
fn canceled_subscriber_does_not_cancel_digest_producer_or_shared_cache() {
    let fixture = Fixture::new(44_100, 1);
    let first =
        ColdTransaction::capture(&fixture.samples, &fixture.source, true, &|| false).unwrap();
    let gate = first.preparation_gate(48_000, 2, &|| false).unwrap();
    let second =
        ColdTransaction::capture(&fixture.samples, &fixture.source, true, &|| false).unwrap();
    let canceled = Arc::new(AtomicBool::new(false));
    let flag = canceled.clone();
    let follower = std::thread::spawn(move || {
        second
            .preparation_gate(48_000, 2, &|| flag.load(Ordering::Acquire))
            .err()
            .unwrap()
            .kind()
    });
    canceled.store(true, Ordering::Release);
    assert_eq!(follower.join().unwrap(), io::ErrorKind::Interrupted);
    let (sample, lease) = fixture.finish_cold(first, 48_000, 2);
    drop(gate);
    let (warm, next) = fixture.warm(&fixture.source, true, 48_000, 2);
    assert!(Arc::ptr_eq(&sample.samples, &warm.samples));
    assert_eq!(lease.cache_path, next.cache_path);
}

#[test]
fn retired_shared_cache_survives_other_owner_and_windows_partial_delete_retries() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new(44_100, 1);
    let (sample, first) = fixture.cold(48_000, 2);
    let (warm, second) = fixture.warm(&fixture.source, true, 48_000, 2);
    let path = first.cache_path.clone();
    let original = first.original_path.clone();
    first.retire_cache();
    drop((first, sample));
    std::thread::sleep(Duration::from_millis(75));
    assert!(path.join("decoder.f32le").exists());
    let sharing = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path.join("playback.f32le"))
        .unwrap();
    second.retire_cache();
    drop((second, warm));
    let deadline = Instant::now() + Duration::from_secs(5);
    while path.join("decoder.f32le").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!path.join("decoder.f32le").exists());
    assert!(path.join("playback.f32le").exists());
    drop(sharing);
    wait_absent(&path);
    assert!(
        original.exists(),
        "ordinary cache retirement never deletes originals"
    );
    assert!(fixture.source.exists());
}

#[test]
fn failed_shared_subscriber_then_shutdown_preserves_surviving_durable_warm_restore() {
    let fixture = Fixture::new(44_100, 1);
    let (first_pcm, first) = fixture.cold(48_000, 2);
    let original = first.original_path.clone();
    let cache = first.cache_path.clone();
    let (failed_pcm, failed) = fixture.warm(&fixture.source, true, 48_000, 2);
    let failed_original = failed.original_path.clone();
    assert!(Arc::ptr_eq(&first_pcm.samples, &failed_pcm.samples));
    failed.rollback_unadopted_original();
    failed.retire_cache();
    drop((failed_pcm, failed));
    wait_absent(&failed_original);
    drop((first_pcm, first)); // ordinary reader shutdown, surviving saved assignment
    std::thread::sleep(Duration::from_millis(100));
    assert!(cache.join("decoder.f32le").exists());
    assert!(original.exists());
    let (restored_pcm, restored) = fixture.warm(&original, false, 48_000, 2);
    assert!(restored.integrity.warm);
    assert_eq!(restored.cache_path, cache);
    assert!(!restored_pcm.samples.is_empty());
}

#[test]
fn failed_restore_never_rolls_back_the_existing_imported_original() {
    let fixture = Fixture::new(44_100, 1);
    let (first_pcm, first) = fixture.cold(48_000, 2);
    let original = first.original_path.clone();
    let cache = first.cache_path.clone();
    let (failed_pcm, failed) = fixture.warm(&original, false, 48_000, 2);
    failed.rollback_unadopted_original();
    failed.retire_cache();
    drop((failed_pcm, failed, first_pcm, first));
    std::thread::sleep(Duration::from_millis(100));
    assert!(original.exists());
    assert!(cache.exists());
    let (_, restored) = fixture.warm(&original, false, 48_000, 2);
    assert!(restored.integrity.warm);
}

#[test]
fn producer_rollback_after_another_restore_adopts_cannot_delete_its_saved_original() {
    let fixture = Fixture::new(44_100, 1);
    let (producer_pcm, producer) = fixture.cold(48_000, 2);
    let original = producer.original_path.clone();
    let cache = producer.cache_path.clone();
    let (survivor_pcm, survivor) = fixture.warm(&original, false, 48_000, 2);
    admit_original_owner(&original).unwrap();
    producer.rollback_unadopted_original(); // late claimed-fault/rejection
    producer.retire_cache();
    drop((producer_pcm, producer));
    drop((survivor_pcm, survivor)); // successful ordinary shutdown
    std::thread::sleep(Duration::from_millis(100));
    assert!(original.exists());
    assert!(cache.exists());
    let (_, restored) = fixture.warm(&original, false, 48_000, 2);
    assert!(restored.integrity.warm);
}

#[test]
fn original_owner_admission_cancels_rollback_before_last_immutable_reader_closes() {
    let fixture = Fixture::new(44_100, 1);
    let (sample, lease) = fixture.cold(48_000, 2);
    let original = lease.original_path.clone();
    lease.rollback_unadopted_original();
    admit_original_owner(&original).unwrap();
    drop((sample, lease));
    std::thread::sleep(Duration::from_millis(100));
    assert!(original.exists());
    assert!(fixture.source.exists());
}

#[test]
fn exclusive_rollback_retries_until_external_snapshot_reader_retires() {
    let fixture = Fixture::new(44_100, 1);
    let transaction =
        ColdTransaction::capture(&fixture.samples, &fixture.source, true, &|| false).unwrap();
    let stage = transaction.staging.as_ref().unwrap().clone();
    let reader = transaction.snapshot_file().unwrap();
    drop(transaction);
    std::thread::sleep(Duration::from_millis(75));
    assert!(stage.join("snapshot.original").exists());
    drop(reader);
    wait_absent(&stage);
    assert!(fixture.source.exists());
}

#[test]
fn unknown_cache_children_and_fresh_validated_readers_exclude_cleanup() {
    let fixture = Fixture::new(44_100, 1);
    let (sample, first) = fixture.cold(48_000, 2);
    let path = first.cache_path.clone();
    fs::write(path.join("private.reference"), b"keep").unwrap();
    first.retire_cache();
    drop((first, sample));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fs::read(path.join("private.reference")).unwrap(), b"keep");
    let (warm, owner) = fixture.warm(&fixture.source, true, 48_000, 2);
    assert_eq!(owner.cache_path, path);
    fs::remove_file(path.join("private.reference")).unwrap();
    std::thread::sleep(Duration::from_millis(75));
    assert!(path.exists());
    owner.retire_cache();
    drop((warm, owner));
    wait_absent(&path);
}

#[test]
fn recognized_dead_crash_staging_recovers_while_live_unknown_and_private_files_survive() {
    let fixture = Fixture::new(44_100, 1);
    let live =
        ColdTransaction::capture(&fixture.samples, &fixture.source, true, &|| false).unwrap();
    let root = live.cache_root.clone();
    let dead = root.join(".staging-4294967295-123");
    fs::create_dir(&dead).unwrap();
    staging::record_owner(&root, &dead).unwrap();
    let owner_path = dead.join("owner.json");
    let mut owner: Value = serde_json::from_slice(&fs::read(&owner_path).unwrap()).unwrap();
    owner["pid"] = json!(u32::MAX);
    fs::write(&owner_path, serde_json::to_vec(&canonical(&owner)).unwrap()).unwrap();
    fs::write(dead.join("snapshot.original"), b"crash-only-owned").unwrap();
    let unknown = root.join(".staging-unknown");
    fs::create_dir(&unknown).unwrap();
    fs::write(unknown.join("private.reference"), b"keep").unwrap();
    let current_path = live.staging.as_ref().unwrap().clone();
    staging::recover(&root).unwrap();
    assert!(!dead.exists());
    assert!(current_path.join("snapshot.original").exists());
    assert_eq!(
        fs::read(unknown.join("private.reference")).unwrap(),
        b"keep"
    );
    assert!(fixture.source.exists());
}

#[test]
fn hostile_manifest_and_dimension_extents_fail_before_pcm_allocation() {
    let fixture = Fixture::new(44_100, 1);
    let (sample, first) = fixture.cold(48_000, 2);
    let path = first.cache_path.clone();
    drop((sample, first));
    fs::write(path.join("manifest.json"), vec![b' '; 256 * 1024 + 1]).unwrap();
    drop(fixture.rejected(48_000, 2));
    let decoder =
        json!({"channels":32,"full_frames":u64::MAX,"rate_hz":48000,"full_bytes":u64::MAX});
    assert!(warm::dimensions(&decoder, PCM_LIMIT_BYTES).is_err());
}

#[test]
fn full_validation_then_pcm_budget_failure_keeps_cleanup_reservation_for_valid_retry() {
    let fixture = Fixture::new(48_000, 2);
    let (pcm, lease) = fixture.cold(48_000, 2);
    let cache = lease.cache_path.clone();
    let playback_bytes = pcm.samples.len() * 4;
    drop((pcm, lease));
    let mut transaction =
        ColdTransaction::capture(&fixture.samples, &fixture.source, true, &|| false).unwrap();
    let _gate = transaction.preparation_gate(48_000, 2, &|| false).unwrap();
    let admission_before = cleanup_admission_status().0;
    assert!(
        transaction
            .try_reuse(48_000, 2, playback_bytes + 64, &|| false)
            .unwrap()
            .is_none()
    );
    assert!(transaction.integrity.decoder_verify_bytes > 0);
    assert!(transaction.integrity.playback_verify_bytes > 0);
    assert!(transaction.cache_slot.is_some());
    // Other tests share the real service; only this transaction's retained slot
    // is asserted exactly, while the global counter remains within its hard cap.
    assert!(admission_before <= cleanup_admission_status().1);
    let pcm = transaction
        .try_reuse(48_000, 2, PCM_LIMIT_BYTES, &|| false)
        .unwrap()
        .unwrap();
    transaction.commit(&|| false).unwrap();
    let lease = transaction.into_lease();
    assert_eq!(lease.cache_path, cache);
    assert!(!pcm.samples.is_empty());
    assert!(lease.cache.cleanup.is_some());
}

#[test]
fn off_thread_cache_pruning_releases_last_dead_pcm_weak_without_dropping_file_lease() {
    let fixture = Fixture::new(48_000, 2);
    let (pcm, lease) = fixture.cold(48_000, 2);
    let cache = lease.cache_path.clone();
    assert_eq!(
        Arc::weak_count(&pcm.samples),
        1,
        "store owns one actual allocation weak"
    );
    let observer = Arc::downgrade(&pcm.samples);
    assert_eq!(Arc::weak_count(&pcm.samples), 2);
    lease.prune_dead_pcm();
    assert_eq!(Arc::weak_count(&pcm.samples), 2, "live PCM is retained");
    drop(pcm);
    assert!(observer.upgrade().is_none());
    drop(observer);
    assert!(lease.cache.pcm.lock().unwrap().is_some());
    lease.prune_dead_pcm();
    assert!(
        lease.cache.pcm.lock().unwrap().is_none(),
        "last backing-allocation weak was released off-thread"
    );
    assert!(cache.join("playback.f32le").exists());
    assert!(
        OpenOptions::new()
            .write(true)
            .open(cache.join("playback.f32le"))
            .is_err(),
        "durable file lease remains sealed"
    );
}
