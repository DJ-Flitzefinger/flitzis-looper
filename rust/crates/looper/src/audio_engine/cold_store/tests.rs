use super::*;

#[test]
fn canonical_identity_binds_all_domains_and_ignores_key_insertion_order() {
    let a = json!({"source":{"sha256":"a","bytes":4},"pcm":{"rate":44100,"channels":2,"frames":1},"version":1});
    let b = json!({"version":1,"pcm":{"frames":1,"channels":2,"rate":44100},"source":{"bytes":4,"sha256":"a"}});
    assert_eq!(
        descriptor_digest(&a).unwrap(),
        descriptor_digest(&b).unwrap()
    );
    for pointer in [
        "/source/sha256",
        "/source/bytes",
        "/pcm/rate",
        "/pcm/channels",
        "/pcm/frames",
        "/version",
    ] {
        let mut changed = a.clone();
        *changed.pointer_mut(pointer).unwrap() = json!("different");
        assert_ne!(
            descriptor_digest(&a).unwrap(),
            descriptor_digest(&changed).unwrap()
        );
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::cell::Cell;

    struct Fixture {
        _temp: tempfile::TempDir,
        samples: PathBuf,
        source: PathBuf,
        bytes: Vec<u8>,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let samples = temp.path().join("samples");
            let source = temp.path().join("external").join("loop.wav");
            fs::create_dir_all(source.parent().unwrap()).unwrap();
            let bytes: Vec<_> = (0..(CHUNK_BYTES * 2 + 317))
                .map(|n| (n % 251) as u8)
                .collect();
            fs::write(&source, &bytes).unwrap();
            Self {
                _temp: temp,
                samples,
                source,
                bytes,
            }
        }

        fn capture(&self, import: bool) -> ColdTransaction {
            let source = if import {
                self.source.clone()
            } else {
                fs::create_dir_all(&self.samples).unwrap();
                let restored = self.samples.join("restored.wav");
                if !restored.exists() {
                    fs::write(&restored, &self.bytes).unwrap();
                }
                restored
            };
            ColdTransaction::capture(&self.samples, &source, import, &|| false).unwrap()
        }

        fn entries(&self) -> Vec<PathBuf> {
            let cache = self.samples.join(".pcm-cache").join("v1");
            fs::read_dir(cache)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect()
        }
    }

    fn artifacts(transaction: &mut ColdTransaction) -> ColdManifest {
        transaction
            .write_pcm_artifacts(
                PcmArtifactInput {
                    samples: &[0.0, 0.5, -0.75, 0.25, 1.0, 0.0],
                    rate_hz: 44_100,
                    channels: 2,
                    provenance: json!({"decoder":"fixture-v1","delay_policy":"retain"}),
                },
                PcmArtifactInput {
                    samples: &[0.25, -0.25, 0.5],
                    rate_hz: 48_000,
                    channels: 1,
                    provenance: json!({"processing":"fixture-v1"}),
                },
                json!({"kind":"fixture","from_rate":44100,"to_rate":48000,"origin":"source-zero"}),
                &|| false,
            )
            .unwrap()
    }

    fn digest_f32(values: &[f32]) -> String {
        let bytes: Vec<_> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        format!("{:x}", Sha256::digest(bytes))
    }

    #[test]
    fn copy_first_survives_external_replacement_and_aba_byte_exactly() {
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(true);
        assert_eq!(transaction.source_bytes(), fixture.bytes.len() as u64);
        assert_eq!(
            transaction.source_digest(),
            format!("{:x}", Sha256::digest(&fixture.bytes))
        );
        // Import no longer reads or locks the external path after sealed capture.
        let renamed = fixture.source.with_extension("old");
        fs::rename(&fixture.source, &renamed).unwrap();
        fs::write(&fixture.source, vec![99_u8; fixture.bytes.len()]).unwrap();
        fs::remove_file(&fixture.source).unwrap();
        fs::rename(&renamed, &fixture.source).unwrap();
        let mut decoded_input = Vec::new();
        transaction
            .snapshot_file()
            .unwrap()
            .read_to_end(&mut decoded_input)
            .unwrap();
        assert_eq!(decoded_input, fixture.bytes);
        artifacts(&mut transaction);
        transaction.commit(&|| false).unwrap();
        let original = transaction.original_path().to_owned();
        assert_eq!(fs::read(&original).unwrap(), fixture.bytes);
        let lease = transaction.into_lease();
        drop(lease);
        assert_eq!(
            fs::read(&original).unwrap(),
            fs::read(&fixture.source).unwrap()
        );
    }

    #[test]
    fn preexisting_writer_refuses_capture_without_creating_assets() {
        let fixture = Fixture::new();
        let writer = OpenOptions::new()
            .write(true)
            .open(&fixture.source)
            .unwrap();
        let error =
            match ColdTransaction::capture(&fixture.samples, &fixture.source, true, &|| false) {
                Ok(_) => panic!("active writer admitted"),
                Err(error) => error,
            };
        assert_eq!(error.raw_os_error(), Some(32)); // ERROR_SHARING_VIOLATION
        assert!(!fixture.samples.exists());
        drop(writer);
        assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
    }

    #[test]
    fn source_lock_excludes_real_writes_replacement_and_aba_during_copy_chunks() {
        let fixture = Fixture::new();
        let calls = Cell::new(0);
        let blocked = Cell::new(0);
        let cancelled = || {
            calls.set(calls.get() + 1);
            if calls.get() == 3 {
                assert!(
                    OpenOptions::new()
                        .write(true)
                        .open(&fixture.source)
                        .is_err()
                );
                assert!(fs::rename(&fixture.source, fixture.source.with_extension("aba")).is_err());
                assert!(fs::remove_file(&fixture.source).is_err());
                blocked.set(blocked.get() + 1);
            }
            false
        };
        let transaction =
            ColdTransaction::capture(&fixture.samples, &fixture.source, true, &cancelled).unwrap();
        assert_eq!(blocked.get(), 1);
        assert_eq!(
            transaction.source_digest(),
            format!("{:x}", Sha256::digest(&fixture.bytes))
        );
        drop(transaction);
        assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
    }

    #[test]
    fn sealed_snapshot_and_restored_original_exclude_mutation_replacement_delete() {
        let fixture = Fixture::new();
        let transaction = fixture.capture(false);
        for path in [&transaction.snapshot_path, &transaction.original_path] {
            assert!(OpenOptions::new().write(true).open(path).is_err());
            assert!(fs::remove_file(path).is_err());
            assert!(fs::rename(path, path.with_extension("replacement")).is_err());
        }
        let mut clone = transaction.snapshot_file().unwrap();
        assert!(fs::remove_file(&transaction.snapshot_path).is_err());
        let mut bytes = Vec::new();
        clone.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, fixture.bytes);
        drop(clone);
        drop(transaction);
        assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
        assert!(fixture.entries().is_empty());
    }

    #[test]
    fn complete_manifest_uses_independent_full_interleaved_and_mono_oracles() {
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(true);
        let manifest = artifacts(&mut transaction);
        let decoder = &manifest.descriptor["decoder"]["pcm"];
        let playback = &manifest.descriptor["playback"]["pcm"];
        assert_eq!(decoder["full_frames"], 3);
        assert_eq!(decoder["full_bytes"], 24);
        assert_eq!(decoder["rate_hz"], 44100);
        assert_eq!(decoder["channels"], 2);
        assert_eq!(
            decoder["interleaved_sha256"],
            digest_f32(&[0.0, 0.5, -0.75, 0.25, 1.0, 0.0])
        );
        assert_eq!(decoder["mono_sha256"], digest_f32(&[0.25, -0.25, 0.5]));
        assert_ne!(decoder["interleaved_sha256"], decoder["mono_sha256"]);
        assert_eq!(playback["rate_hz"], 48000);
        assert_eq!(playback["full_frames"], 3);
        assert_eq!(
            playback["interleaved_sha256"],
            digest_f32(&[0.25, -0.25, 0.5])
        );
        assert_eq!(
            manifest.decoder_identity,
            descriptor_digest(&manifest.descriptor["decoder"]).unwrap()
        );
        assert_eq!(
            manifest.identity,
            descriptor_digest(&manifest.descriptor).unwrap()
        );
        let cache = transaction.commit(&|| false).unwrap();
        let lease = transaction.into_lease();
        assert_eq!(lease.manifest.identity, manifest.identity);
        assert_eq!(lease.cache_path, cache);
        assert_eq!(fs::read_dir(&cache).unwrap().count(), 3);
        assert_eq!(fs::read(cache.join("decoder.f32le")).unwrap().len(), 24);
        let saved: Value =
            serde_json::from_slice(&fs::read(cache.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(saved, manifest.encoded());
        drop(lease);
    }

    #[test]
    fn committed_lease_keeps_original_pcm_manifest_and_directory_immutable() {
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(true);
        artifacts(&mut transaction);
        transaction.commit(&|| false).unwrap();
        let lease = transaction.into_lease();
        for path in [
            &lease.original_path,
            &lease.cache_path.join("decoder.f32le"),
            &lease.cache_path.join("playback.f32le"),
            &lease.cache_path.join("manifest.json"),
        ] {
            assert!(OpenOptions::new().write(true).open(path).is_err());
            assert!(fs::remove_file(path).is_err());
        }
        assert!(
            fs::rename(
                &lease.cache_path,
                lease.cache_path.with_extension("renamed")
            )
            .is_err()
        );
        let original = lease.original_path.clone();
        let cache = lease.cache_path.clone();
        drop(lease);
        // C1a successful disk entries remain durable; C1b owns eventual deletion.
        assert!(original.exists());
        assert!(cache.exists());
        assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
    }

    #[test]
    fn independent_cold_generations_never_replace_identical_or_partial_entries() {
        let fixture = Fixture::new();
        let mut first = fixture.capture(true);
        let first_manifest = artifacts(&mut first);
        let first_path = first.commit(&|| false).unwrap();
        let first = first.into_lease();
        let partial = fixture
            .samples
            .join(".pcm-cache")
            .join("v1")
            .join(&first_manifest.identity);
        fs::create_dir(&partial).unwrap();
        fs::write(partial.join("private.partial"), b"keep").unwrap();
        let mut second = fixture.capture(true);
        let second_manifest = artifacts(&mut second);
        let second_path = second.commit(&|| false).unwrap();
        let second = second.into_lease();
        assert_eq!(first_manifest.identity, second_manifest.identity);
        assert_ne!(first_path, second_path);
        assert_ne!(first.original_path, second.original_path);
        assert_eq!(fs::read(partial.join("private.partial")).unwrap(), b"keep");
        drop((first, second));
    }

    #[test]
    fn snapshot_copy_cancellation_rolls_back_exclusive_staging_and_preserves_external() {
        let fixture = Fixture::new();
        let calls = Cell::new(0);
        let cancelled = || {
            calls.set(calls.get() + 1);
            calls.get() >= 4
        };
        assert!(
            ColdTransaction::capture(&fixture.samples, &fixture.source, true, &cancelled).is_err()
        );
        assert!(fixture.entries().is_empty());
        assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
    }

    #[test]
    fn pcm_cancel_nonfinite_and_missing_manifest_never_publish() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let fixture = Fixture::new();
            let mut transaction = fixture.capture(true);
            let result = transaction.write_pcm_artifacts(
                PcmArtifactInput {
                    samples: &[0.0, bad],
                    rate_hz: 44100,
                    channels: 1,
                    provenance: json!({}),
                },
                PcmArtifactInput {
                    samples: &[0.0],
                    rate_hz: 48000,
                    channels: 1,
                    provenance: json!({}),
                },
                json!({}),
                &|| false,
            );
            assert!(result.is_err());
            assert!(transaction.commit(&|| false).is_err());
            drop(transaction);
            assert!(fixture.entries().is_empty());
            assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
        }
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(true);
        let samples = vec![0.0; CHUNK_BYTES];
        let calls = Cell::new(0);
        let result = transaction.write_pcm_artifacts(
            PcmArtifactInput {
                samples: &samples,
                rate_hz: 44100,
                channels: 1,
                provenance: json!({}),
            },
            PcmArtifactInput {
                samples: &samples,
                rate_hz: 48000,
                channels: 1,
                provenance: json!({}),
            },
            json!({}),
            &|| {
                calls.set(calls.get() + 1);
                calls.get() > 3
            },
        );
        assert!(result.is_err());
        drop(transaction);
        assert!(fixture.entries().is_empty());
    }

    #[test]
    fn stale_or_queue_failure_after_disk_commit_rolls_back_every_new_owned_asset() {
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(true);
        artifacts(&mut transaction);
        let cache = transaction.commit(&|| false).unwrap();
        let original = transaction.original_path().to_owned();
        assert!(cache.exists());
        assert!(original.exists());
        // The productive caller rejects stale/queue-full and drops this transaction.
        drop(transaction);
        assert!(!cache.exists());
        assert!(!original.exists());
        assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
    }

    #[test]
    fn cancellation_during_original_copy_and_committed_verification_rolls_back() {
        for threshold in [3, 7, 11, 15] {
            let fixture = Fixture::new();
            let mut transaction = fixture.capture(true);
            artifacts(&mut transaction);
            let calls = Cell::new(0);
            let error = transaction
                .commit(&|| {
                    calls.set(calls.get() + 1);
                    calls.get() >= threshold
                })
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Interrupted);
            drop(transaction);
            assert!(fixture.entries().is_empty());
            assert_eq!(fs::read_dir(&fixture.samples).unwrap().count(), 1);
            assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
        }
    }

    #[test]
    fn rename_collision_preserves_foreign_entry_and_rolls_back_new_original() {
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(true);
        let manifest = artifacts(&mut transaction);
        let staging = transaction.staging.as_ref().unwrap();
        let generation = staging.file_name().unwrap().to_string_lossy();
        let destination = transaction.cache_root.join(format!(
            "{}-{}",
            manifest.identity,
            generation.trim_start_matches(".staging-")
        ));
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("private.failed"), b"preserve").unwrap();
        assert_eq!(
            transaction.commit(&|| false).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        let original = transaction.original_path().to_owned();
        drop(transaction);
        assert!(!original.exists());
        assert_eq!(
            fs::read(destination.join("private.failed")).unwrap(),
            b"preserve"
        );
        assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
    }

    #[test]
    fn corruption_between_stage_verification_and_commit_rejects_publication() {
        for filename in ["decoder.f32le", "playback.f32le", "manifest.json"] {
            let fixture = Fixture::new();
            let mut transaction = fixture.capture(true);
            artifacts(&mut transaction);
            // Explicit fault injection models a failed handover/seal protocol.
            // Production holds these handles until the rename, then re-verifies.
            transaction.artifact_readers.clear();
            let path = transaction.staging.as_ref().unwrap().join(filename);
            let mut bytes = fs::read(&path).unwrap();
            bytes[0] ^= 1;
            fs::write(&path, bytes).unwrap();
            let error = transaction.commit(&|| false).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            let original = transaction.original_path().to_owned();
            drop(transaction);
            assert!(!original.exists());
            assert!(fixture.entries().is_empty());
            assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
        }
    }

    #[test]
    fn concurrent_imports_reserve_exclusive_originals_and_complete_generations() {
        let fixture = Fixture::new();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let leases = std::thread::scope(|scope| {
            let mut jobs = Vec::new();
            for _ in 0..2 {
                let barrier = barrier.clone();
                let fixture = &fixture;
                jobs.push(scope.spawn(move || {
                    let mut transaction = fixture.capture(true);
                    artifacts(&mut transaction);
                    barrier.wait();
                    transaction.commit(&|| false).unwrap();
                    transaction.into_lease()
                }));
            }
            jobs.into_iter()
                .map(|job| job.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_ne!(leases[0].original_path, leases[1].original_path);
        assert_ne!(leases[0].cache_path, leases[1].cache_path);
        for lease in &leases {
            assert_eq!(fs::read(&lease.original_path).unwrap(), fixture.bytes);
            assert_eq!(fs::read_dir(&lease.cache_path).unwrap().count(), 3);
        }
        drop(leases);
    }

    #[test]
    fn full_verify_rejects_same_size_corruption_and_extra_eof_bytes() {
        let fixture = Fixture::new();
        for bytes in [&b"same"[..], &b"same plus tail"[..]] {
            let path = fixture.source.with_extension("pcm");
            fs::write(&path, bytes).unwrap();
            let mut file = sealed_reader(&path).unwrap();
            let expected = format!("{:x}", Sha256::digest(b"good"));
            assert!(verify_file(&mut file, &expected, 4, &|| false).is_err());
        }
    }

    #[test]
    fn restore_creates_no_durable_second_original_and_retains_original_lock() {
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(false);
        artifacts(&mut transaction);
        transaction.commit(&|| false).unwrap();
        let lease = transaction.into_lease();
        assert_eq!(
            lease.original_path,
            fs::canonicalize(fixture.samples.join("restored.wav")).unwrap()
        );
        assert_eq!(fs::read_dir(&fixture.samples).unwrap().count(), 2);
        assert!(
            OpenOptions::new()
                .write(true)
                .open(&lease.original_path)
                .is_err()
        );
        drop(lease);
        assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
    }

    #[test]
    fn restore_cannot_adopt_external_original_or_traversal_into_external_root() {
        let fixture = Fixture::new();
        fs::create_dir(&fixture.samples).unwrap();
        for source in [
            &fixture.source,
            &fixture.samples.join("..").join("external").join("loop.wav"),
        ] {
            let result = ColdTransaction::capture(&fixture.samples, source, false, &|| false);
            assert!(result.is_err());
            assert!(fixture.entries().is_empty());
            assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
        }
    }

    #[test]
    fn samples_and_cache_junctions_cannot_grant_ownership_of_foreign_files() {
        for case in 0..3 {
            let fixture = Fixture::new();
            let foreign = fixture._temp.path().join("foreign");
            fs::create_dir(&foreign).unwrap();
            fs::write(foreign.join("private.failed"), b"keep").unwrap();
            let link = if case != 1 {
                fixture.samples.clone()
            } else {
                fs::create_dir(&fixture.samples).unwrap();
                fixture.samples.join(".pcm-cache")
            };
            let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&foreign)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "could not create contained test junction: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let samples = if case == 2 {
                fixture.samples.join("must-not-be-created")
            } else {
                fixture.samples.clone()
            };
            let result = ColdTransaction::capture(&samples, &fixture.source, true, &|| false);
            assert!(result.is_err());
            assert_eq!(fs::read(foreign.join("private.failed")).unwrap(), b"keep");
            assert_eq!(fs::read_dir(&foreign).unwrap().count(), 1);
            assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
            // Remove the junction itself, never traverse/delete its target.
            fs::remove_dir(&link).unwrap();
        }
    }
}
