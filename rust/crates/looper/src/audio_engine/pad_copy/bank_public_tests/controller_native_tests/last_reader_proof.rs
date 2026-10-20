//! Last-reader tail for the actual completed fresh-process Bank receiver.
//! The caller finishes all continuity assertions before any Clear is admitted.
use super::*;
use std::io::Write;
use std::os::windows::fs::MetadataExt;

#[path = "last_reader_proof/b2_delete_tests.rs"]
mod b2_delete_tests;

fn module<'py>(py: Python<'py>) -> Bound<'py, PyModule> {
    PyModule::from_code(
        py,
        &CString::new(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../src/tests/flitzis_looper/controller/bank_last_reader_native_probe.py"
        )))
        .unwrap(),
        c"bank_last_reader_native_probe.py",
        c"bank_last_reader_native_probe",
    )
    .unwrap()
}

fn invoke(controller: &Py<PyAny>, method: &str) {
    Python::attach(|py| {
        module(py)
            .getattr(method)
            .unwrap()
            .call1((controller.bind(py),))
            .unwrap();
    });
}

fn json_invoke(controller: &Py<PyAny>, method: &str) -> Value {
    Python::attach(|py| {
        let encoded: String = module(py)
            .getattr(method)
            .unwrap()
            .call1((controller.bind(py),))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    })
}

fn no_reparse(path: &Path) {
    assert_eq!(
        fs::symlink_metadata(path).unwrap().file_attributes() & 0x400,
        0,
        "last-reader fixture must not traverse reparse points: {}",
        path.display()
    );
}

fn evidence_directory() -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let parent = workspace.join("test-artifacts/bank36-last-reader-20261020");
    for ancestor in parent.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => assert_eq!(metadata.file_attributes() & 0x400, 0),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("last-reader ancestor is unavailable: {error}"),
        }
    }
    fs::create_dir_all(&parent).unwrap();
    for ancestor in parent.ancestors() {
        no_reparse(ancestor);
    }
    let output = tempfile::Builder::new()
        .prefix("actual-bank-reader-")
        .tempdir_in(parent)
        .unwrap()
        .keep();
    no_reparse(&output);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("OWNER.md"))
        .unwrap()
        .write_all(b"Owned scalar evidence for the existing synthetic Bank receiver's last-reader proof. No new source/engine fixture or private audio.")
        .unwrap();
    output
}

#[derive(Debug, PartialEq)]
struct Artifact {
    identity: crate::audio_engine::project_assets::FileIdentity,
    files: ArtifactSnapshot,
}

fn snapshot(path: &Path) -> Artifact {
    fn visit(root: &Path, path: &Path, files: &mut ArtifactSnapshot) {
        no_reparse(path);
        let metadata = fs::symlink_metadata(path).unwrap();
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), files);
            }
        } else {
            assert!(metadata.is_file());
            assert!(
                files.len() < 64,
                "synthetic last-reader artifact exceeded its file bound"
            );
            files.insert(
                path.strip_prefix(root).unwrap().to_path_buf(),
                (
                    crate::audio_engine::project_assets::capture_identity(path)
                        .unwrap()
                        .unwrap(),
                    format!("{:x}", Sha256::digest(fs::read(path).unwrap())),
                ),
            );
        }
    }
    let identity = crate::audio_engine::project_assets::capture_identity(path)
        .unwrap()
        .unwrap();
    let mut files = BTreeMap::new();
    visit(path, path, &mut files);
    Artifact { identity, files }
}

fn snapshot_all(paths: &[PathBuf; 5]) -> Vec<Artifact> {
    paths.iter().map(|path| snapshot(path)).collect()
}

fn wait_for_publication(controller: &Py<PyAny>, callback: &mut Callback) {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        callback.drain();
        let ready: bool = Python::attach(|py| {
            module(py)
                .getattr("poll_publication")
                .unwrap()
                .call1((controller.bind(py),))
                .unwrap()
                .extract()
                .unwrap()
        });
        if ready {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "actual Bank Clear worker did not publish: {}",
            json_invoke(controller, "diagnostics")
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn clear(controller: &Py<PyAny>, engine: &Py<AudioEngine>, callback: &mut Callback, bank: usize) {
    Python::attach(|py| {
        module(py)
            .getattr("capture_clear")
            .unwrap()
            .call1((controller.bind(py), bank))
            .unwrap();
    });
    wait_for_publication(controller, callback);
    let ticket: Py<BankMutationTicket> = Python::attach(|py| {
        controller
            .getattr(py, "ticket")
            .unwrap()
            .extract(py)
            .unwrap()
    });
    assert!(matches!(
        callback.controls.peek().unwrap(),
        ControlMessage::BankMutation(_)
    ));
    Python::attach(|py| {
        assert_eq!(ticket.borrow(py).publication_status(), "pending");
        assert!(ticket.borrow(py).acknowledgement().is_none());
    });
    super::fresh_recovery_tests::deliver_owned_checkpoint(callback, controller, Some(&ticket));
    Python::attach(|py| {
        let ticket = ticket.borrow(py);
        assert_eq!(ticket.publication_status(), "accepted");
        let acknowledgement = ticket.acknowledgement().unwrap();
        assert!(
            engine
                .borrow(py)
                .validate_bank_mutation(&ticket, &acknowledgement)
                .unwrap()
        );
    });
    let receipt = json_invoke(controller, "settle_clear");
    assert_eq!(receipt["phase"], json!("settled"));
    assert_eq!(receipt["commit_count"], json!(1));
    assert_eq!(receipt["revision"], receipt["current_revision"]);
    assert_eq!(receipt["sha256"], receipt["config_sha256"]);
    assert_eq!(receipt["native_finished"], json!(true));
    assert_eq!(receipt["writer_released"], json!(true));
    assert_eq!(receipt["completed"], json!(true));
    for slot in bank * 36..bank * 36 + 36 {
        assert!(callback.mixer.bank_for_measurement()[slot].is_none());
    }
    // No opaque ticket/ACK/mirror test holder may become an unlabelled final reader.
    Python::attach(|_| drop(ticket));
    invoke(controller, "forget_clear");
}

/// Called only on the existing successful receiver, after its continuity proofs.
pub(super) fn run(controller: &Py<PyAny>, engine: &Py<AudioEngine>, callback: &mut Callback) {
    let output = evidence_directory();
    let owner = output.join("OWNER.md");
    let owner_bytes = fs::read(&owner).unwrap();
    let sentinel = std::env::current_dir()
        .unwrap()
        .join("samples/bank-last-reader-independent.txt");
    let sentinel_bytes =
        b"Independent generated sentinel; Bank Clear and reader retirement must preserve it.";
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&sentinel)
        .unwrap()
        .write_all(sentinel_bytes)
        .unwrap();
    let held = json_invoke(controller, "begin");
    let (original, source_pcm) = Python::attach(|py| {
        let native = engine.borrow(py);
        let leases = native.cold_leases.lock().unwrap();
        let lease = leases[36].as_ref().unwrap();
        // Scalar paths only: never clone the source lease or prepared pair.
        (lease.original_path.clone(), lease.cache_path.clone())
    });
    let paths = [
        original,
        source_pcm,
        PathBuf::from(held["wav"].as_str().unwrap()),
        PathBuf::from(held["stem_pcm"].as_str().unwrap()),
        PathBuf::from(held["descriptor"].as_str().unwrap()),
    ];
    assert!(paths.iter().all(|path| path.is_absolute()));
    assert_eq!(
        paths
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        5
    );
    let original_artifacts = snapshot_all(&paths);
    let config = PathBuf::from(held["config"].as_str().unwrap());
    fs::write(
        output.join("before.json"),
        serde_json::to_vec_pretty(&held).unwrap(),
    )
    .unwrap();
    for bank in [1, 0, 2] {
        clear(controller, engine, callback, bank);
        assert_eq!(snapshot_all(&paths), original_artifacts);
        assert_eq!(fs::read(&owner).unwrap(), owner_bytes);
        assert_eq!(fs::read(&sentinel).unwrap(), sentinel_bytes);
        assert!(config.is_file());
    }
    assert!(
        callback
            .mixer
            .bank_for_measurement()
            .iter()
            .all(Option::is_none)
    );
    Python::attach(|py| {
        let native = engine.borrow(py);
        for slot in 0..NUM_SAMPLES {
            assert_eq!(native.input_runtime_ownership.copy_reservation(slot), 0);
            assert_eq!(native.input_runtime_ownership.pair_reservation(slot), 0);
        }
        let encoded: String = module(py)
            .getattr("assert_held")
            .unwrap()
            .call1((
                controller.bind(py),
                held["startup_id"].as_u64().unwrap(),
                held["acceptance_id"].as_u64().unwrap(),
            ))
            .unwrap()
            .extract()
            .unwrap();
        let reread: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(reread["geometry"], held["reader_geometry"]);
        assert_eq!(reread["sha256"], held["reader_sha256"]);
    });
    assert_eq!(snapshot_all(&paths), original_artifacts);
    let config_after_clears = fs::read(&config).unwrap();
    invoke(controller, "release_reader");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        callback.drain();
        invoke(controller, "poll_retirements");
        assert_eq!(fs::read(&config).unwrap(), config_after_clears);
        assert_eq!(fs::read(&owner).unwrap(), owner_bytes);
        assert_eq!(fs::read(&sentinel).unwrap(), sentinel_bytes);
        if paths.iter().all(|path| !path.exists()) {
            break;
        }
        if Instant::now() >= deadline {
            let diagnostic = json!({
                "paths": paths.iter().map(|path| json!({"path":path,"exists":path.exists()})).collect::<Vec<_>>(),
                "runtime": json_invoke(controller, "diagnostics"),
            });
            fs::write(
                output.join("retirement-pending.json"),
                serde_json::to_vec_pretty(&diagnostic).unwrap(),
            )
            .unwrap();
            panic!("actual Bank final reader did not retire all five artifacts: {diagnostic}");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let completed = json!({"all_five_absent":true,"paths":paths,"held_reader":held,
        "config_preserved":config,"owner_preserved":owner,"sentinel_preserved":sentinel});
    fs::write(
        output.join("completed.json"),
        serde_json::to_vec_pretty(&completed).unwrap(),
    )
    .unwrap();
}
