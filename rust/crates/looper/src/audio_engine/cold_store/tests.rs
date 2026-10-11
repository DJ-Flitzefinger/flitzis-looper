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
            let mut roots = vec![self.samples.join(".pcm-cache/v1")];
            if let Ok(materials) = fs::read_dir(&self.samples) {
                roots.extend(materials.map(|entry| entry.unwrap().path().join(".pcm-cache/v1")));
            }
            roots
                .into_iter()
                .filter_map(|root| fs::read_dir(root).ok())
                .flatten()
                .map(|entry| entry.unwrap().path())
                .collect()
        }
    }

    #[test]
    fn canonical_material_preserves_name_bytes_and_absolute_restore_reuses_binding() {
        let fixture = Fixture::new();
        let renamed = fixture.source.with_file_name("Mélodie 東京.flac");
        fs::rename(&fixture.source, &renamed).unwrap();
        let mut transaction =
            ColdTransaction::capture(&fixture.samples, &renamed, true, &|| false).unwrap();
        artifacts(&mut transaction);
        let cache = transaction.commit(&|| false).unwrap();
        let lease = transaction.into_lease();
        let digest = format!("{:x}", Sha256::digest(&fixture.bytes));
        let id = digest[..32].to_owned();
        assert_eq!(lease.material_id.as_deref(), Some(id.as_str()));
        assert_eq!(
            lease.original_path.file_name().unwrap(),
            "Mélodie 東京.flac"
        );
        assert_eq!(fs::read(&lease.original_path).unwrap(), fixture.bytes);
        assert!(
            cache
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".ready-")
        );
        assert_eq!(
            cache.parent().unwrap(),
            lease.original_path.parent().unwrap().join(".pcm-cache/v1")
        );
        let restored =
            ColdTransaction::capture(&fixture.samples, &lease.original_path, false, &|| false)
                .unwrap();
        assert_eq!(restored.material_id, lease.material_id);
        assert_eq!(restored.original_path, lease.original_path);
        assert!(!restored.owned_original);
        drop(restored);
        assert!(lease.original_path.exists());
    }

    #[test]
    fn dot_leading_audio_name_is_preserved_and_invalid_leaf_creates_no_samples() {
        let fixture = Fixture::new();
        let dot = fixture.source.with_file_name(".Take.wav");
        fs::rename(&fixture.source, &dot).unwrap();
        let mut transaction =
            ColdTransaction::capture(&fixture.samples, &dot, true, &|| false).unwrap();
        artifacts(&mut transaction);
        transaction.commit(&|| false).unwrap();
        let lease = transaction.into_lease();
        assert_eq!(lease.original_path.file_name().unwrap(), ".Take.wav");
        assert_eq!(fs::read(&lease.original_path).unwrap(), fixture.bytes);

        let untouched = fixture._temp.path().join("never-created").join("samples");
        assert!(
            ColdTransaction::capture(
                &untouched,
                &dot.with_file_name("bad.wav:stream"),
                true,
                &|| false
            )
            .is_err()
        );
        assert!(!untouched.parent().unwrap().exists());
    }

    #[test]
    fn actual_windows_long_paths_keep_exact_unicode_source_leaf_with_bounded_readable_directory() {
        let fixture = Fixture::new();
        let leaf = format!("{}.flac", "Mélodie東京".repeat(20));
        let source = fs::canonicalize(fixture.source.parent().unwrap())
            .unwrap()
            .join(&leaf);
        fs::rename(&fixture.source, &source).unwrap();
        let mut deep = fs::canonicalize(fixture._temp.path()).unwrap();
        for index in 0..3 {
            deep = deep.join(format!("project_{index}_{}", "x".repeat(80)));
            fs::create_dir(&deep).unwrap();
        }
        let samples = deep.join("samples");
        let mut transaction = ColdTransaction::capture(&samples, &source, true, &|| false).unwrap();
        artifacts(&mut transaction);
        transaction.commit(&|| false).unwrap();
        let lease = transaction.into_lease();
        assert_eq!(
            lease.original_path.file_name().unwrap().to_str().unwrap(),
            leaf
        );
        assert!(lease.original_path.to_string_lossy().len() > 260);
        assert!(
            lease
                .original_path
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .encode_utf16()
                .count()
                <= 96
        );
        assert_eq!(fs::read(&lease.original_path).unwrap(), fixture.bytes);
        assert!(lease.original_path.parent().unwrap().join("stems").is_dir());
        assert!(
            OpenOptions::new()
                .write(true)
                .open(lease.original_path.parent().unwrap().join(".material.json"))
                .is_err()
        );
    }

    #[test]
    fn occupied_unbound_readable_root_never_adopts_or_overwrites_unknown_binding() {
        let fixture = Fixture::new();
        let digest = format!("{:x}", Sha256::digest(&fixture.bytes));
        let occupied = fixture.samples.join("loop.wav");
        fs::create_dir_all(occupied.join("original")).unwrap();
        fs::write(occupied.join("original/Other.wav"), b"unknown material").unwrap();
        fs::write(occupied.join("private.keep"), b"preserved").unwrap();
        let transaction = fixture.capture(true);
        assert_eq!(transaction.material_id.as_deref(), Some(&digest[..32]));
        assert_ne!(
            transaction.original_root,
            fs::canonicalize(&occupied).unwrap()
        );
        assert_eq!(
            fs::read(occupied.join("original/Other.wav")).unwrap(),
            b"unknown material"
        );
        assert_eq!(
            fs::read(occupied.join("private.keep")).unwrap(),
            b"preserved"
        );
        assert!(!occupied.join("original/loop.wav").exists());
    }

    #[test]
    fn windows_case_collision_and_full_sha_prefix_extension_preserve_unknown_roots() {
        let fixture = Fixture::new();
        let digest = format!("{:x}", Sha256::digest(&fixture.bytes));
        fs::create_dir_all(fixture.samples.join("LOOP.WAV")).unwrap();
        fs::write(
            fixture.samples.join("LOOP.WAV/private.keep"),
            b"case neighbor",
        )
        .unwrap();
        let short = fixture.samples.join(format!("loop.wav~{}", &digest[..12]));
        fs::create_dir(&short).unwrap();
        fs::write(short.join("private.keep"), b"prefix neighbor").unwrap();
        let mut transaction = fixture.capture(true);
        assert_eq!(
            transaction
                .original_root
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            format!("loop.wav~{}", &digest[..16])
        );
        artifacts(&mut transaction);
        transaction.commit(&|| false).unwrap();
        let lease = transaction.into_lease();
        assert_eq!(lease.original_path.file_name().unwrap(), "loop.wav");
        assert_eq!(
            fs::read(fixture.samples.join("LOOP.WAV/private.keep")).unwrap(),
            b"case neighbor"
        );
        assert_eq!(
            fs::read(short.join("private.keep")).unwrap(),
            b"prefix neighbor"
        );
    }

    #[test]
    fn same_name_same_size_different_full_content_receives_distinct_safe_root() {
        let fixture = Fixture::new();
        let mut first = fixture.capture(true);
        artifacts(&mut first);
        first.commit(&|| false).unwrap();
        let first = first.into_lease();
        let mut changed = fixture.bytes.clone();
        *changed.last_mut().unwrap() ^= 1;
        fs::write(&fixture.source, &changed).unwrap();
        let mut second = fixture.capture(true);
        assert_ne!(second.material_id, first.material_id);
        artifacts(&mut second);
        second.commit(&|| false).unwrap();
        let second = second.into_lease();
        assert_ne!(first.original_path.parent(), second.original_path.parent());
        assert_eq!(fs::read(&first.original_path).unwrap(), fixture.bytes);
        assert_eq!(fs::read(&second.original_path).unwrap(), changed);
    }

    #[test]
    fn readable_canonical_source_late_corruption_or_extra_eof_is_never_reused() {
        for extra in [false, true] {
            let fixture = Fixture::new();
            let mut first = fixture.capture(true);
            artifacts(&mut first);
            first.commit(&|| false).unwrap();
            let first = first.into_lease();
            let old_path = first.original_path.clone();
            drop(first);
            let mut damaged = fixture.bytes.clone();
            if extra {
                damaged.push(0);
            } else {
                *damaged.last_mut().unwrap() ^= 1;
            }
            fs::write(&old_path, &damaged).unwrap();
            let second = fixture.capture(true);
            assert_ne!(second.original_path, old_path);
            assert_eq!(fs::read(&old_path).unwrap(), damaged);
            assert_eq!(second.integrity.original_copied_bytes, 0);
            drop(second);
        }
    }

    #[test]
    fn final_unowned_readable_root_prunes_scaffolding_but_preserves_unreceipted_binding() {
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(true);
        artifacts(&mut transaction);
        transaction.commit(&|| false).unwrap();
        let lease = transaction.into_lease();
        let original = lease.original_path.clone();
        let cache = lease.cache_path.clone();
        let root = original.parent().unwrap().to_owned();
        fs::create_dir_all(root.join(".pcm-cache/stems/v1/.pairs")).unwrap();
        let private = root.join("private.keep");
        fs::write(&private, b"unknown root neighbor").unwrap();
        drop(lease);
        for leaf in ["decoder.f32le", "playback.f32le", "manifest.json"] {
            fs::remove_file(cache.join(leaf)).unwrap();
        }
        fs::remove_dir(&cache).unwrap();
        fs::remove_file(&original).unwrap();
        let assets = super::super::super::project_assets::ProjectAssets::shared();
        assets.prune_material_if_unowned(&original);
        assert_eq!(fs::read(&private).unwrap(), b"unknown root neighbor");
        assert!(root.join(".material.json").exists());
        assert!(!root.join(".pcm-cache").exists());
        fs::remove_file(&private).unwrap();
        let binding = root.join(".material.json");
        let mut changed: serde_json::Value =
            serde_json::from_slice(&fs::read(&binding).unwrap()).unwrap();
        changed["sha256"] = serde_json::Value::String("f".repeat(64));
        let changed = serde_json::to_vec(&changed).unwrap();
        fs::write(&binding, &changed).unwrap();
        assets.prune_material_if_unowned(&original);
        assert_eq!(fs::read(&binding).unwrap(), changed);
        assert!(root.exists());
        assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
    }

    fn deny_material_replacement(root: &Path) {
        use std::os::windows::fs::OpenOptionsExt;
        let error = fs::rename(root, root.with_file_name("replaced-root")).unwrap_err();
        assert!(matches!(error.raw_os_error(), Some(5 | 32 | 33)), "{error}");
        // Reparse mutation needs a writable directory handle; the retained
        // root excludes it even at birth when the directory is still empty.
        let error = OpenOptions::new()
            .write(true)
            .share_mode(3)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(root)
            .unwrap_err();
        assert!(matches!(error.raw_os_error(), Some(5 | 32 | 33)), "{error}");
    }

    fn writable_directory_control(root: &Path) {
        use std::os::windows::fs::OpenOptionsExt;
        // Same real open flags, successful without guards: an invalid request or
        // ACL failure must not masquerade as canonical reparse-write exclusion.
        drop(
            OpenOptions::new()
                .write(true)
                .share_mode(3)
                .custom_flags(0x0200_0000 | 0x0020_0000)
                .open(root)
                .unwrap(),
        );
    }

    #[test]
    fn actual_birth_handle_denies_empty_root_replacement_before_setup() {
        let fixture = Fixture::new();
        let unheld = fixture._temp.path().join("unheld-root");
        fs::create_dir(&unheld).unwrap();
        writable_directory_control(&unheld);
        let observed = Arc::new(AtomicBool::new(false));
        let notified = observed.clone();
        material::observe_birth_for_test(move |root| {
            assert_eq!(fs::read_dir(root).unwrap().count(), 0);
            deny_material_replacement(root);
            let identity = super::super::super::project_assets::capture_identity(root)
                .unwrap()
                .unwrap();
            assert!(
                super::super::super::project_assets::remove_empty_directory(root, &identity)
                    .is_err()
            );
            notified.store(true, Ordering::Release);
        });
        let transaction = fixture.capture(true);
        assert!(observed.load(Ordering::Acquire));
        let root = transaction.original_root.clone();
        drop(transaction);
        assert!(
            !root.exists(),
            "our unchanged empty creation must roll back"
        );
        assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
    }

    #[test]
    fn selected_canonical_root_keeps_guards_before_transaction_handover() {
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(true);
        artifacts(&mut transaction);
        transaction.commit(&|| false).unwrap();
        let lease = transaction.into_lease();
        let original = lease.original_path.clone();
        drop(lease); // No old canonical directory handles mask the selection seam.
        let observed = Arc::new(AtomicBool::new(false));
        let notified = observed.clone();
        material::observe_selection_for_test(move |root| {
            deny_material_replacement(root);
            let parent = root.parent().unwrap();
            assert!(fs::rename(parent, parent.with_file_name("samples-replaced")).is_err());
            notified.store(true, Ordering::Release);
        });
        let follower = fixture.capture(true);
        assert!(observed.load(Ordering::Acquire));
        assert_eq!(follower.original_path, original);
        assert_eq!(
            follower.integrity.original_verify_bytes,
            fixture.bytes.len() as u64
        );
        assert_eq!(follower.integrity.source_copied_bytes, 0);
        drop(follower);
        writable_directory_control(original.parent().unwrap());
        assert_eq!(fs::read(&original).unwrap(), fixture.bytes);
    }

    #[test]
    fn content_claim_namespace_stays_guarded_until_canonical_commit() {
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(true);
        let claims = fixture.samples.join(".material-claims");
        let renamed = fixture.samples.join("claims-renamed");
        assert!(fs::rename(&claims, &renamed).is_err());
        artifacts(&mut transaction);
        transaction.commit(&|| false).unwrap();
        fs::rename(&claims, &renamed).unwrap();
        fs::rename(&renamed, &claims).unwrap();
        drop(transaction.into_lease());
    }

    #[test]
    fn cancelled_distinct_inputs_cannot_grow_durable_claim_namespace_beyond_fixed_shards() {
        let fixture = Fixture::new();
        for value in 0_u64..300 {
            fs::write(&fixture.source, value.to_le_bytes()).unwrap();
            let mut transaction = fixture.capture(true);
            assert_eq!(
                transaction.commit(&|| true).unwrap_err().kind(),
                io::ErrorKind::Interrupted
            );
            drop(transaction);
        }
        let entries = fs::read_dir(fixture.samples.join(".material-claims"))
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        assert!(
            entries.len() <= 256,
            "cancelled digests must not leave unbounded claim files"
        );
        for entry in entries {
            assert!(entry.metadata().unwrap().is_file());
            assert_eq!(entry.metadata().unwrap().len(), 0);
        }
        assert!(
            fs::read_dir(&fixture.samples)
                .unwrap()
                .all(|entry| entry.unwrap().file_name() == ".material-claims")
        );
    }

    #[test]
    fn foreign_original_appearing_after_actual_root_birth_is_never_adopted() {
        let fixture = Fixture::new();
        let bytes = fixture.bytes.clone();
        material::observe_birth_for_test(move |root| {
            fs::write(root.join("loop.wav"), bytes).unwrap();
        });
        let mut transaction = fixture.capture(true);
        let original = transaction.original_path.clone();
        let root = transaction.original_root.clone();
        artifacts(&mut transaction);
        assert_eq!(
            transaction.commit(&|| false).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        drop(transaction);
        assert_eq!(fs::read(original).unwrap(), fixture.bytes);
        assert!(!root.join(".material.json").exists());
        assert!(root.exists());
    }

    fn remove_test_artifacts(original: &Path, cache: &Path) {
        for leaf in ["decoder.f32le", "playback.f32le", "manifest.json"] {
            fs::remove_file(cache.join(leaf)).unwrap();
        }
        fs::remove_dir(cache).unwrap();
        fs::remove_file(original).unwrap();
    }

    #[test]
    fn unchanged_created_binding_root_cleanup_preserves_unknown_neighbor_until_removed() {
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(true);
        artifacts(&mut transaction);
        transaction.commit(&|| false).unwrap();
        let lease = transaction.into_lease();
        let original = lease.original_path.clone();
        let cache = lease.cache_path.clone();
        let root = original.parent().unwrap().to_owned();
        let receipt = lease.material_creation_receipt().unwrap();
        assert_eq!(
            receipt.root_identity,
            super::super::super::project_assets::capture_identity(&root)
                .unwrap()
                .unwrap()
        );
        let private = root.join("private.keep");
        fs::write(&private, b"unknown root neighbor").unwrap();
        drop(lease);
        remove_test_artifacts(&original, &cache);
        let assets = super::super::super::project_assets::ProjectAssets::shared();
        assets.prune_material_if_unowned(&original);
        assert_eq!(fs::read(&private).unwrap(), b"unknown root neighbor");
        assert!(root.join(".material.json").exists());
        fs::remove_file(private).unwrap();
        assets.prune_material_if_unowned(&original);
        assert!(!root.exists());
        assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
    }

    #[test]
    fn own_marker_or_root_replacement_with_identical_valid_bytes_never_inherits_receipt() {
        for replace_root in [false, true] {
            let fixture = Fixture::new();
            let mut transaction = fixture.capture(true);
            artifacts(&mut transaction);
            transaction.commit(&|| false).unwrap();
            let lease = transaction.into_lease();
            let original = lease.original_path.clone();
            let cache = lease.cache_path.clone();
            let root = original.parent().unwrap().to_owned();
            let marker = root.join(".material.json");
            let bytes = fs::read(&marker).unwrap();
            drop(lease);
            remove_test_artifacts(&original, &cache);
            if replace_root {
                fs::rename(&root, root.with_file_name("old-root")).unwrap();
                fs::create_dir(&root).unwrap();
                fs::create_dir(root.join("stems")).unwrap();
            } else {
                fs::rename(&marker, fixture._temp.path().join("old-marker.json")).unwrap();
            }
            fs::write(&marker, &bytes).unwrap();
            super::super::super::project_assets::ProjectAssets::shared()
                .prune_material_if_unowned(&original);
            assert_eq!(fs::read(marker).unwrap(), bytes);
            assert!(root.exists());
            if replace_root {
                assert!(root.join("stems").exists());
            }
        }
    }

    #[test]
    fn parsed_valid_restored_binding_has_no_creation_cleanup_rights() {
        let fixture = Fixture::new();
        let root = fixture.samples.join("restored-readable");
        fs::create_dir_all(&root).unwrap();
        let binding = super::super::super::material_paths::MaterialBinding {
            schema_version: 1,
            sha256: format!("{:x}", Sha256::digest(&fixture.bytes)),
            bytes: fixture.bytes.len() as u64,
            original_leaf: "loop.wav".to_owned(),
        };
        let marker = serde_json::to_vec(&binding).unwrap();
        fs::write(root.join(".material.json"), &marker).unwrap();
        let original = root.join("loop.wav");
        fs::write(&original, &fixture.bytes).unwrap();
        let mut transaction =
            ColdTransaction::capture(&fixture.samples, &original, false, &|| false).unwrap();
        artifacts(&mut transaction);
        transaction.commit(&|| false).unwrap();
        let lease = transaction.into_lease();
        assert!(
            material_creation_receipt(&fixture.samples, &original)
                .unwrap()
                .is_none()
        );
        let cache = lease.cache_path.clone();
        drop(lease);
        remove_test_artifacts(&original, &cache);
        super::super::super::project_assets::ProjectAssets::shared()
            .prune_material_if_unowned(&original);
        assert_eq!(fs::read(root.join(".material.json")).unwrap(), marker);
        assert!(root.exists());
    }

    #[test]
    fn cancel_after_real_binding_publication_waits_for_held_marker_before_exact_root_cleanup() {
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(true);
        artifacts(&mut transaction);
        transaction.create_original(&|| false).unwrap();
        let original = transaction.original_path.clone();
        let root = transaction.original_root.clone();
        let held = sealed_reader(&root.join(".material.json")).unwrap();
        assert_eq!(
            transaction.commit(&|| true).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        drop(transaction);
        assert!(!original.exists());
        assert!(root.join(".material.json").exists());
        drop(held);
        let deadline = Instant::now() + Duration::from_secs(5);
        while root.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !root.exists(),
            "actual marker reader return must drain own cleanup"
        );
        assert_eq!(fs::read(&fixture.source).unwrap(), fixture.bytes);
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
    fn held_source_excludes_external_replacement_and_aba_until_durable_original() {
        let fixture = Fixture::new();
        let mut transaction = fixture.capture(true);
        assert_eq!(transaction.source_bytes(), fixture.bytes.len() as u64);
        assert_eq!(
            transaction.source_digest(),
            format!("{:x}", Sha256::digest(&fixture.bytes))
        );
        // No snapshot copy is needed: the original held handle excludes actual ABA.
        let renamed = fixture.source.with_extension("old");
        let external = fixture.source.parent().unwrap();
        let renamed_external = external.with_file_name("external-renamed");
        assert!(fs::rename(external, &renamed_external).is_err());
        assert!(fs::rename(&fixture.source, &renamed).is_err());
        assert!(fs::write(&fixture.source, vec![99_u8; fixture.bytes.len()]).is_err());
        assert!(fs::remove_file(&fixture.source).is_err());
        assert_eq!(transaction.integrity.source_copied_bytes, 0);
        assert!(!transaction.snapshot_path.exists());
        let mut decoded_input = Vec::new();
        transaction
            .snapshot_file()
            .unwrap()
            .read_to_end(&mut decoded_input)
            .unwrap();
        assert_eq!(decoded_input, fixture.bytes);
        artifacts(&mut transaction);
        transaction.commit(&|| false).unwrap();
        fs::rename(external, &renamed_external).unwrap();
        fs::rename(&renamed_external, external).unwrap();
        fs::rename(&fixture.source, &renamed).unwrap();
        fs::rename(&renamed, &fixture.source).unwrap();
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
        assert!(!transaction.snapshot_path.exists());
        for path in [&transaction.original_path] {
            assert!(OpenOptions::new().write(true).open(path).is_err());
            assert!(fs::remove_file(path).is_err());
            assert!(fs::rename(path, path.with_extension("replacement")).is_err());
        }
        let mut clone = transaction.snapshot_file().unwrap();
        assert!(fs::remove_file(&transaction.original_path).is_err());
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
        let partial = first_path.parent().unwrap().join(&first_manifest.identity);
        fs::create_dir(&partial).unwrap();
        fs::write(partial.join("private.partial"), b"keep").unwrap();
        let mut second = fixture.capture(true);
        let second_manifest = artifacts(&mut second);
        let second_path = second.commit(&|| false).unwrap();
        let second = second.into_lease();
        assert_eq!(first_manifest.identity, second_manifest.identity);
        assert_ne!(first_path, second_path);
        assert_eq!(first.original_path, second.original_path);
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
            assert!(
                fs::read_dir(&fixture.samples)
                    .unwrap()
                    .all(|entry| entry.unwrap().file_name() == ".material-claims")
            );
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
            ".ready-{}",
            &format!(
                "{:x}",
                Sha256::digest(format!("{}|{generation}", manifest.identity).as_bytes())
            )[..32]
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
                    barrier.wait();
                    let mut transaction = fixture.capture(true);
                    artifacts(&mut transaction);
                    transaction.commit(&|| false).unwrap();
                    transaction.into_lease()
                }));
            }
            jobs.into_iter()
                .map(|job| job.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(leases[0].original_path, leases[1].original_path);
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
                fixture.samples.join(".material-claims")
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
