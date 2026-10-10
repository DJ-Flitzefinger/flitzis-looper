#![cfg(windows)]
use super::*;
use crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES;
use crate::audio_engine::cold_residency;
use crate::audio_engine::material_migration::{prepare_material, prepare_pcm};

struct Fixture {
    _temp: tempfile::TempDir,
    samples: PathBuf,
    original: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let samples = temp.path().join("samples");
        fs::create_dir(&samples).unwrap();
        let original = samples.join("old.wav");
        let frames = 1024_u32;
        let rate = 48_000_u32;
        let channels = 2_u16;
        let bytes = frames * u32::from(channels) * 2;
        let mut writer = File::create(&original).unwrap();
        writer.write_all(b"RIFF").unwrap();
        writer.write_all(&(bytes + 36).to_le_bytes()).unwrap();
        writer.write_all(b"WAVEfmt \x10\0\0\0\x01\0").unwrap();
        writer.write_all(&channels.to_le_bytes()).unwrap();
        writer.write_all(&rate.to_le_bytes()).unwrap();
        writer
            .write_all(&(rate * u32::from(channels) * 2).to_le_bytes())
            .unwrap();
        writer.write_all(&(channels * 2).to_le_bytes()).unwrap();
        writer.write_all(&16_u16.to_le_bytes()).unwrap();
        writer.write_all(b"data").unwrap();
        writer.write_all(&bytes.to_le_bytes()).unwrap();
        for frame in 0..frames {
            writer
                .write_all(&((frame as i16 - 512) * 16).to_le_bytes())
                .unwrap();
            writer
                .write_all(&((512 - frame as i16) * 8).to_le_bytes())
                .unwrap();
        }
        drop(writer);
        Self {
            _temp: temp,
            samples,
            original,
        }
    }

    fn legacy(&self, rate: u32, channels: usize) -> (SampleBuffer, CommittedColdLease) {
        let mut transaction =
            ColdTransaction::capture(&self.samples, &self.original, false, &|| false).unwrap();
        let _gate = transaction
            .preparation_gate(rate, channels, &|| false)
            .unwrap();
        let mut sample = prepare_pcm(
            &mut transaction,
            &self.original,
            rate,
            channels,
            PCM_LIMIT_BYTES,
            None,
            &|| false,
            &mut |_| {},
        )
        .unwrap();
        cold_residency::attach(&mut sample, transaction.manifest().unwrap()).unwrap();
        transaction.commit(&|| false).unwrap();
        transaction.bind_pcm(&sample.samples);
        (sample, transaction.into_lease())
    }
}

#[test]
fn migration_copies_complete_verified_legacy_pcm_without_decoder_and_keeps_old_readers() {
    let fixture = Fixture::new();
    let original_bytes = fs::read(&fixture.original).unwrap();
    let (old_sample, old_lease) = fixture.legacy(48_000, 2);
    let prepared =
        prepare_material(&fixture.samples, &fixture.original, 48_000, 2, &|| false).unwrap();
    assert!(
        prepared.lease.integrity.warm,
        "verified legacy PCM must avoid decoding"
    );
    assert_eq!(
        prepared.lease.manifest.descriptor,
        old_lease.manifest.descriptor
    );
    assert_eq!(
        prepared.sample.samples.as_ref(),
        old_sample.samples.as_ref()
    );
    assert!(!Arc::ptr_eq(&prepared.sample.samples, &old_sample.samples));
    assert_eq!(
        fs::read(&prepared.lease.original_path).unwrap(),
        original_bytes
    );
    assert_ne!(prepared.lease.original_path, old_lease.original_path);
    assert_ne!(prepared.lease.cache_path, old_lease.cache_path);
    for name in ["decoder.f32le", "playback.f32le", "manifest.json"] {
        assert_eq!(
            fs::read(prepared.lease.cache_path.join(name)).unwrap(),
            fs::read(old_lease.cache_path.join(name)).unwrap()
        );
    }
    old_lease.open_complete_reader(&old_sample).unwrap();
    let metadata = prepared.metadata();
    assert_eq!(metadata["old_reference"], "samples/old.wav");
    assert!(
        metadata["new_reference"]
            .as_str()
            .unwrap()
            .starts_with("samples/materials/M")
    );
    assert_eq!(
        metadata["material_id"],
        prepared.lease.material_id.as_deref().unwrap()
    );
    assert_eq!(
        metadata["decoder_identity"],
        old_lease.manifest.decoder_identity
    );
    assert_eq!(metadata["playback_identity"], old_lease.manifest.identity);
    assert_eq!(metadata["original"]["bytes"], original_bytes.len() as u64);
    assert!(metadata.get("assignment_id").is_none());
    assert!(metadata.get("ack").is_none());
    assert_eq!(fs::read(&fixture.original).unwrap(), original_bytes);
}

#[test]
fn migration_retry_reuses_verified_canonical_generation_and_fresh_assignment() {
    let fixture = Fixture::new();
    let (_old_sample, old_lease) = fixture.legacy(48_000, 2);
    let old_manifest = fs::read(old_lease.cache_path.join("manifest.json")).unwrap();
    let first =
        prepare_material(&fixture.samples, &fixture.original, 48_000, 2, &|| false).unwrap();
    let root = first.lease.cache_path.parent().unwrap();
    let before = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<HashSet<_>>();
    let second =
        prepare_material(&fixture.samples, &fixture.original, 48_000, 2, &|| false).unwrap();
    let after = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<HashSet<_>>();
    assert_eq!(first.lease.material_id, second.lease.material_id);
    assert_eq!(first.lease.original_path, second.lease.original_path);
    assert_eq!(first.lease.cache_path, second.lease.cache_path);
    assert_eq!(before, after);
    assert_ne!(first.lease.assignment_id(), second.lease.assignment_id());
    assert!(!Arc::ptr_eq(&first.sample.samples, &second.sample.samples));
    assert!(!second.lease.created_cache && !second.lease.created_original);
    assert_eq!(second.lease.integrity.original_copied_bytes, 0);
    assert_eq!(
        fs::read(old_lease.cache_path.join("manifest.json")).unwrap(),
        old_manifest
    );
}

#[test]
fn incompatible_legacy_pcm_falls_back_to_decoder_without_changing_old_assets() {
    let fixture = Fixture::new();
    let (old_sample, old_lease) = fixture.legacy(32_000, 1);
    let old_manifest = fs::read(old_lease.cache_path.join("manifest.json")).unwrap();
    let prepared =
        prepare_material(&fixture.samples, &fixture.original, 48_000, 2, &|| false).unwrap();
    assert!(!prepared.lease.integrity.warm);
    assert_eq!(prepared.sample.channels, 2);
    assert_eq!(prepared.sample.frame_count(), 1024);
    assert_eq!(old_sample.channels, 1);
    assert_eq!(
        fs::read(old_lease.cache_path.join("manifest.json")).unwrap(),
        old_manifest
    );
    old_lease.open_complete_reader(&old_sample).unwrap();
}

#[test]
fn corrupt_complete_legacy_pcm_is_preserved_and_cannot_authorize_migration() {
    let fixture = Fixture::new();
    let (sample, lease) = fixture.legacy(48_000, 2);
    let cache = lease.cache_path.clone();
    lease.release_preparation_assignment();
    drop(lease);
    drop(sample);
    let path = cache.join("playback.f32le");
    let mut corrupt = fs::read(&path).unwrap();
    corrupt[17] ^= 1;
    fs::write(&path, &corrupt).unwrap();
    let prepared =
        prepare_material(&fixture.samples, &fixture.original, 48_000, 2, &|| false).unwrap();
    assert!(!prepared.lease.integrity.warm);
    assert_eq!(prepared.sample.frame_count(), 1024);
    assert_eq!(fs::read(&path).unwrap(), corrupt);
}

#[test]
fn migration_rejects_non_original_traversal_and_cancellation_without_source_loss() {
    let fixture = Fixture::new();
    let bytes = fs::read(&fixture.original).unwrap();
    assert!(
        prepare_material(
            &fixture.samples,
            &fixture.samples.join("../outside.wav"),
            48_000,
            2,
            &|| false
        )
        .is_err()
    );
    assert!(
        prepare_material(
            &fixture.samples,
            &fixture
                .samples
                .join("materials/M00000000000000000000000000000000/manifest.json"),
            48_000,
            2,
            &|| false
        )
        .is_err()
    );
    assert!(!fixture.samples.join("materials").exists());
    assert!(prepare_material(&fixture.samples, &fixture.original, 48_000, 2, &|| true).is_err());
    assert_eq!(fs::read(&fixture.original).unwrap(), bytes);
}

#[test]
fn verified_subscribers_have_independent_ids_and_no_producer_rollback_rights() {
    let fixture = Fixture::new();
    let prepared =
        prepare_material(&fixture.samples, &fixture.original, 48_000, 2, &|| false).unwrap();
    let parent = &prepared.lease;
    let first = parent.subscribe_verified().unwrap();
    let second = parent.subscribe_verified().unwrap();
    assert_eq!(parent.clone().assignment_id(), parent.assignment_id());
    assert_ne!(first.assignment_id(), parent.assignment_id());
    assert_ne!(second.assignment_id(), parent.assignment_id());
    assert_ne!(first.assignment_id(), second.assignment_id());
    assert_eq!(parent.cache_assignment_count(), 3);
    assert!(!first.created_original && !first.created_cache);
    parent.release_preparation_assignment();
    assert!(parent.assignment_retired());
    assert_eq!(second.cache_assignment_count(), 2);
    first.rollback_unadopted_original();
    first.rollback_unadopted_cache();
    assert!(first.assignment_retired());
    assert!(!second.assignment_retired());
    assert!(!second.cache.retired.load(Ordering::Acquire));
    assert!(!second._original.rollback.load(Ordering::Acquire));
    assert_eq!(second.cache_assignment_count(), 1);
    second.open_complete_reader(&prepared.sample).unwrap();
    assert!(second.original_path.exists());
    assert!(second.cache_path.exists());
    second.release_preparation_assignment();
}

#[test]
fn subscriber_owner_capacity_failure_is_atomic_for_both_registrations() {
    for fill_original in [false, true] {
        let fixture = Fixture::new();
        let prepared =
            prepare_material(&fixture.samples, &fixture.original, 48_000, 2, &|| false).unwrap();
        let lease = &prepared.lease;
        let set = if fill_original {
            &lease._original.durable_assignments
        } else {
            &lease.cache.durable_assignments
        };
        {
            let mut owners = set.lock().unwrap();
            for n in 0..4095 {
                owners.insert(u64::MAX - n);
            }
            assert_eq!(owners.len(), 4096);
        }
        let before_cache = lease.cache.durable_assignments.lock().unwrap().clone();
        let before_original = lease._original.durable_assignments.lock().unwrap().clone();
        let error = lease.subscribe_verified().err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert_eq!(
            *lease.cache.durable_assignments.lock().unwrap(),
            before_cache
        );
        assert_eq!(
            *lease._original.durable_assignments.lock().unwrap(),
            before_original
        );
        assert!(!lease.assignment_retired());
        set.lock()
            .unwrap()
            .retain(|id| *id == lease.assignment_id());
    }
}

#[test]
fn subscriber_rejects_changed_binding_manifest_and_retired_parent_before_mutation() {
    let fixture = Fixture::new();
    let prepared =
        prepare_material(&fixture.samples, &fixture.original, 48_000, 2, &|| false).unwrap();
    let lease = &prepared.lease;
    let mut wrong_path = lease.clone();
    wrong_path.original_path = fixture.original.clone();
    assert!(wrong_path.subscribe_verified().is_err());
    let mut wrong_manifest = lease.clone();
    wrong_manifest.manifest.descriptor["decoder"]["original"]["sha256"] = json!("f".repeat(64));
    assert!(wrong_manifest.subscribe_verified().is_err());
    let mut wrong_material = lease.clone();
    wrong_material.material_id = Some("f".repeat(32));
    assert!(wrong_material.subscribe_verified().is_err());
    assert_eq!(lease.cache_assignment_count(), 1);
    assert!(
        fs::rename(
            &lease.original_path,
            lease.original_path.with_extension("replaced")
        )
        .is_err()
    );
    let current = lease.subscribe_verified().unwrap();
    assert_eq!(lease.cache_assignment_count(), 2);
    lease.release_preparation_assignment();
    assert!(lease.subscribe_verified().is_err());
    assert_eq!(current.cache_assignment_count(), 1);
    current.release_preparation_assignment();
}
