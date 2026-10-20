//! Three sequential real process lifetimes: pending -> lost receipt return -> fresh all36 ACK.
//! Generated sources only; no device, App.run, Native install or historical-JSON authority.
use super::*;
use std::io::Write;
use std::os::windows::fs::MetadataExt;
use std::process::{Command, Stdio};

const STAGE: &str = "FLITZI_BANK_RECOVERY_PROCESS_STAGE";
const COORDINATOR: &str = "FLITZI_BANK_RECOVERY_COORDINATOR_ROOT";
const SAVED_ROOT: &str = "FLITZI_BANK_RECOVERY_SAVED_ROOT";
const HANDOFF: &str = "bank-process-handoff.json";
const CASE: &str = "actual_bank_recovery_three_processes_preserve_fixed36_and_current_receipt";

fn full_test_name() -> String {
    let (_, modules) = module_path!().split_once("::").unwrap();
    format!("{modules}::{CASE}")
}

fn recovery_module<'py>(py: Python<'py>) -> Bound<'py, PyModule> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    py.import("sys")
        .unwrap()
        .getattr("path")
        .unwrap()
        .call_method1(
            "insert",
            (0, repository.join("src").to_string_lossy().as_ref()),
        )
        .unwrap();
    PyModule::from_code(
        py,
        &CString::new(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../src/tests/flitzis_looper/controller/bank_recovery_native_probe.py"
        )))
        .unwrap(),
        c"bank_recovery_native_probe.py",
        c"bank_recovery_native_probe",
    )
    .unwrap()
}

fn restored_controller(engine: &Py<AudioEngine>, stage: &str) -> Py<PyAny> {
    Python::attach(|py| {
        recovery_module(py)
            .getattr("RecoveryProbe")
            .unwrap()
            .call1((engine.bind(py), stage))
            .unwrap()
            .unbind()
    })
}

fn json_call(controller: &Py<PyAny>, method: &str) -> Value {
    Python::attach(|py| {
        serde_json::from_str(
            &controller
                .call_method0(py, method)
                .unwrap()
                .extract::<String>(py)
                .unwrap(),
        )
        .unwrap()
    })
}

fn immutable_artifacts(root: &Path) -> ArtifactSnapshot {
    // Config and ordinal histories deliberately change. Every original/PCM/pair
    // artifact keeps its actual FileID and complete byte digest across processes.
    fn visit(root: &Path, path: &Path, result: &mut ArtifactSnapshot) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap();
            if relative == Path::new("copy-public.json")
                || relative.starts_with(".material-migrations")
            {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).unwrap();
            assert_eq!(metadata.file_attributes() & 0x400, 0);
            if metadata.is_dir() {
                visit(root, &path, result);
            } else {
                assert!(metadata.is_file());
                assert!(
                    result.len() < 128,
                    "synthetic immutable artifact bound exceeded"
                );
                result.insert(
                    relative.to_path_buf(),
                    (
                        crate::audio_engine::project_assets::capture_identity(&path)
                            .unwrap()
                            .unwrap(),
                        format!("{:x}", Sha256::digest(fs::read(&path).unwrap())),
                    ),
                );
            }
        }
    }
    let samples = root.join("samples");
    let mut result = BTreeMap::new();
    visit(&samples, &samples, &mut result);
    result
}

fn write_json(path: &Path, data: &Value) {
    fs::write(path, serde_json::to_vec_pretty(data).unwrap()).unwrap();
}

fn check_prior_processes(root: &Path, expected: usize) {
    let handoff: Value =
        serde_json::from_slice(&fs::read(root.parent().unwrap().join(HANDOFF)).unwrap()).unwrap();
    assert_eq!(
        fs::canonicalize(handoff["root"].as_str().unwrap()).unwrap(),
        fs::canonicalize(root).unwrap()
    );
    let exited = handoff["exited_pids"].as_array().unwrap();
    assert_eq!(exited.len(), expected);
    for pid in exited {
        let pid = u32::try_from(pid.as_u64().unwrap()).unwrap();
        assert_ne!(pid, std::process::id());
        assert!(
            producer_process_has_exited(pid),
            "prior Bank process {pid} still lives"
        );
    }
}

fn producer(parent: &Path) {
    // Use the fixture directly. BankFixture's extra TempDir owner would remove
    // the saved project between child A and B even after Fixture::keep().
    let mut fixture =
        Fixture::with_pair_registration_at(true, true, false, "tap", Some(parent.to_path_buf()));
    let previous = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(fixture.directory.path()).unwrap();
    let controller = controller(&mut fixture, false);
    for target in [35, 36, 37, 71, 72] {
        seed(&mut fixture, &controller, target);
    }
    let saved = config(&controller);
    let ticket = begin_bank(&mut fixture, &controller, Some(0), 1);
    assert_eq!(config(&controller), saved);
    assert_eq!(fixture.callback.controls.slots(), 1);
    let evidence: Value = Python::attach(|py| {
        let text: String = recovery_module(py)
            .getattr("export_pending")
            .unwrap()
            .call1((controller.bind(py),))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&text).unwrap()
    });
    assert_eq!(evidence["phase"], json!("publication_pending"));
    assert_eq!(evidence["slots"], json!(36));
    Python::attach(|py| assert_eq!(ticket.borrow(py).publication_status(), "pending"));
    let root = fixture.directory.path().to_path_buf();
    write_json(
        &parent.join(HANDOFF),
        &json!({
            "root": root,
            "producer_pid": std::process::id(),
            "exited_pids": [],
            "publication": evidence,
        }),
    );
    // Deliberately omit settle/close/cancel. Process exit ends all Native owners;
    // the coordinator waits for that exit before allowing any restored reader.
    std::mem::forget(ticket);
    std::mem::forget(controller);
    drop(previous);
    assert_eq!(fixture.keep_directory_for_process_exit(), root);
    println!("BANK_PROCESS_A_PUBLICATION_PENDING");
}

fn poll_until_restore_checkpoint(controller: &Py<PyAny>, callback: &mut Callback, method: &str) {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        callback.drain();
        let ready: bool = Python::attach(|py| {
            controller
                .call_method0(py, method)
                .unwrap()
                .extract(py)
                .unwrap()
        });
        if ready {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "real Bank restore did not reach its own {method} checkpoint: {}",
            json_call(controller, "diagnostics")
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn restored_bank_matches(callback: &Callback, occupied: &[usize], slots: std::ops::Range<usize>) {
    for slot in slots {
        assert_eq!(
            callback.mixer.bank_for_measurement()[slot].is_some(),
            occupied.contains(&slot),
            "whole callback image differs at PAD{slot}"
        );
        if occupied.contains(&slot) {
            assert_eq!(callback.mixer.loop_region_frames(slot), (START, Some(END)));
            assert!(!callback.mixer.key_lock_for_measurement(slot));
            assert_eq!(
                callback.mixer.pad_pitch_intent(slot),
                Some(PitchIntent::default())
            );
            assert_eq!(
                callback.mixer.stem_demand_for_measurement(slot).0,
                StemMixMode::FullMix
            );
        }
    }
    assert!(
        !callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active || voice.paused)
    );
}

pub(super) fn deliver_owned_checkpoint(
    callback: &mut Callback,
    controller: &Py<PyAny>,
    bank_ticket: Option<&Py<BankMutationTicket>>,
) {
    // The original Bank/query command is verified at the FIFO head below. Real
    // outside restoration may enqueue independent commands behind it; the exact
    // original ticket/nonce and full callback image, rather than global queue
    // cardinality, establish this transaction's own effect and acknowledgement.
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let delivered = callback.drain();
        assert!(delivered <= crate::audio_engine::audio_stream::MAX_CONTROL_MESSAGES_PER_CALLBACK);
        let phase = if let Some(ticket) = bank_ticket {
            Python::attach(|py| ticket.borrow(py).publication_status().to_owned())
        } else {
            json_call(controller, "diagnostics")["fresh_native"]["status"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        if phase == "accepted" {
            return;
        }
        assert!(
            matches!(phase.as_str(), "captured" | "pending" | "checking"),
            "own Native checkpoint did not accept: {phase}; {}",
            json_call(controller, "diagnostics")
        );
        assert!(
            Instant::now() < deadline,
            "own Native checkpoint remained pending"
        );
        // Existing independent Loader/Stem observers may finish outside marker
        // adoption. This hook never polls Bank or commits its Config receipt.
        call(controller, "poll_outside_restore");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn resume_before(root: &Path) {
    check_prior_processes(root, 1);
    let _previous = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(root).unwrap();
    Python::initialize();
    let before = immutable_artifacts(root);
    let (engine, mut callback, _producer) = empty_engine();
    let engine = Python::attach(|py| Py::new(py, engine).unwrap());
    let controller = restored_controller(&engine, "resume_before");
    assert!(
        callback
            .mixer
            .bank_for_measurement()
            .iter()
            .all(Option::is_none)
    );
    call(&controller, "start_restore");
    poll_until_restore_checkpoint(&controller, &mut callback, "poll_restore");
    restored_bank_matches(&callback, &[0, 35, 36, 37, 71], 0..72);
    poll_until_restore_checkpoint(&controller, &mut callback, "poll_bank_preparation");
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
        let ticket = ticket.borrow(py);
        assert_eq!(ticket.publication_status(), "pending");
        assert!(ticket.acknowledgement().is_none());
        let metadata: Value = serde_json::from_str(&ticket.metadata_json()).unwrap();
        let owner = metadata["owner"].as_u64().unwrap();
        let native = engine.borrow(py);
        for slot in 0..NUM_SAMPLES {
            assert_eq!(
                native.input_runtime_ownership.copy_reservation(slot),
                if slot < 72 { owner } else { 0 },
                "resumed parent keeps its actual source/target participant reservations: PAD{slot}"
            );
            assert_eq!(native.input_runtime_ownership.pair_reservation(slot), 0);
        }
    });
    deliver_owned_checkpoint(&mut callback, &controller, Some(&ticket));
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
    poll_until_restore_checkpoint(&controller, &mut callback, "poll_outside_restore");
    restored_bank_matches(&callback, &[0, 35, 36, 71, 72], 0..NUM_SAMPLES);
    let evidence = json_call(&controller, "interrupt_after_actual_write");
    assert_eq!(evidence["phase"], json!("native_ack_observed"));
    assert_eq!(
        evidence["lost_return"],
        json!("BANK_ACTUAL_CONFIG_REPLACED_RETURN_LOST")
    );
    assert_eq!(
        immutable_artifacts(root),
        before,
        "resume must reuse every original/PCM/pair"
    );
    write_json(&root.join("bank-process-b.json"), &evidence);
    std::mem::forget(ticket);
    std::mem::forget(controller);
    std::mem::forget(engine);
    println!("BANK_PROCESS_B_ACTUAL_CONFIG_REPLACED_RETURN_LOST");
}

fn recover_after(root: &Path) {
    check_prior_processes(root, 2);
    let _previous = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(root).unwrap();
    Python::initialize();
    let before = immutable_artifacts(root);
    let (engine, mut callback, _producer) = empty_engine();
    let engine = Python::attach(|py| Py::new(py, engine).unwrap());
    let controller = restored_controller(&engine, "recover_after");
    assert!(
        callback
            .mixer
            .bank_for_measurement()
            .iter()
            .all(Option::is_none)
    );
    call(&controller, "start_restore");
    poll_until_restore_checkpoint(&controller, &mut callback, "poll_restore");
    restored_bank_matches(&callback, &[36, 71], 36..72);
    let pending = json_call(&controller, "assert_fresh_probe_pending");
    assert_eq!(pending["status"], json!("pending"));
    assert!(matches!(
        callback.controls.peek().unwrap(),
        ControlMessage::VerifyRecoveredBank(_)
    ));
    let owner = pending["owner"].as_u64().unwrap();
    Python::attach(|py| {
        let native = engine.borrow(py);
        assert!(native.stream_handle.is_none(), "this proof opens no device");
        for slot in 0..NUM_SAMPLES {
            assert_eq!(
                native.input_runtime_ownership.copy_reservation(slot),
                if (36..72).contains(&slot) { owner } else { 0 },
                "fresh callback query owns all36 genuine occupied/Empty targets: PAD{slot}"
            );
            assert_eq!(native.input_runtime_ownership.pair_reservation(slot), 0);
        }
    });
    deliver_owned_checkpoint(&mut callback, &controller, None);
    poll_until_restore_checkpoint(&controller, &mut callback, "poll_outside_restore");
    // Failure reports the actual fresh receipt status. No fixture edits masks,
    // timing, phases, nonces or selection readiness to make this assertion pass.
    let evidence = json_call(&controller, "settle_recovered");
    assert_eq!(evidence["phase"], json!("settled"));
    assert_eq!(evidence["original_compacted"], json!(true));
    Python::attach(|py| {
        let native = engine.borrow(py);
        for slot in 0..NUM_SAMPLES {
            assert_eq!(native.input_runtime_ownership.copy_reservation(slot), 0);
            assert_eq!(native.input_runtime_ownership.pair_reservation(slot), 0);
        }
    });
    restored_bank_matches(&callback, &[0, 35, 36, 71, 72], 0..NUM_SAMPLES);
    assert_eq!(
        immutable_artifacts(root),
        before,
        "fresh restore must not recopy assets"
    );
    write_json(&root.join("bank-process-c.json"), &evidence);
    super::last_reader_proof::run(&controller, &engine, &mut callback);
    close(controller);
    println!("BANK_PROCESS_C_FRESH_ALL36_ACK_SETTLED");
}

fn run_stage(parent: &Path, root: Option<&Path>, stage: &str, marker: &str) -> u32 {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            &full_test_name(),
            "--nocapture",
            "--test-threads=1",
        ])
        .env(STAGE, stage)
        .env(COORDINATOR, parent)
        .env_remove(SAVED_ROOT)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(root) = root {
        command.env(SAVED_ROOT, root).current_dir(root);
    }
    let child = command.spawn().unwrap();
    let pid = child.id();
    let output = child.wait_with_output().unwrap();
    fs::write(parent.join(format!("{stage}.stdout.log")), &output.stdout).unwrap();
    fs::write(parent.join(format!("{stage}.stderr.log")), &output.stderr).unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Bank process {stage} failed:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "Bank process {stage} did not run:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains(marker),
        "Bank process {stage} missed its real checkpoint:\n{stdout}\n{stderr}"
    );
    assert!(stderr.contains(&format!("actual Native Bank recovery child PID={pid}")));
    assert!(
        producer_process_has_exited(pid),
        "Bank process {pid} has not exited"
    );
    pid
}

fn coordinate() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let owned = workspace.join("test-artifacts/bank36-process-20261020");
    for ancestor in owned.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => assert_eq!(metadata.file_attributes() & 0x400, 0),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("Bank process fixture ancestor is unavailable: {error}"),
        }
    }
    fs::create_dir_all(&owned).unwrap();
    for ancestor in owned.ancestors() {
        assert_eq!(
            fs::symlink_metadata(ancestor).unwrap().file_attributes() & 0x400,
            0
        );
    }
    // Retain bounded generated inputs and positive/negative child logs for Root's
    // review. No automatic cleanup removes the failing boundary's evidence.
    let parent = tempfile::Builder::new()
        .prefix("bank-three-processes-")
        .tempdir_in(&owned)
        .unwrap()
        .keep();
    assert_eq!(
        fs::symlink_metadata(&parent).unwrap().file_attributes() & 0x400,
        0
    );
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(parent.join("OWNER.md"))
        .unwrap()
        .write_all(b"Owned generated Bank three-process recovery fixture. Synthetic audio only; retain positive and negative process logs for review.")
        .unwrap();
    let producer = run_stage(
        &parent,
        None,
        "producer",
        "BANK_PROCESS_A_PUBLICATION_PENDING",
    );
    let handoff_path = parent.join(HANDOFF);
    let mut handoff: Value = serde_json::from_slice(&fs::read(&handoff_path).unwrap()).unwrap();
    assert_eq!(handoff["producer_pid"].as_u64(), Some(u64::from(producer)));
    let root = PathBuf::from(handoff["root"].as_str().unwrap());
    let canonical_root = fs::canonicalize(&root).unwrap();
    let canonical_parent = fs::canonicalize(&parent).unwrap();
    assert_eq!(canonical_root.parent(), Some(canonical_parent.as_path()));
    handoff["exited_pids"] = json!([producer]);
    handoff["coordinator_pid"] = json!(std::process::id());
    write_json(&handoff_path, &handoff);
    let resumed = run_stage(
        &parent,
        Some(&root),
        "resume_before",
        "BANK_PROCESS_B_ACTUAL_CONFIG_REPLACED_RETURN_LOST",
    );
    assert_ne!(producer, resumed);
    handoff["exited_pids"] = json!([producer, resumed]);
    write_json(&handoff_path, &handoff);
    let recovered = run_stage(
        &parent,
        Some(&root),
        "recover_after",
        "BANK_PROCESS_C_FRESH_ALL36_ACK_SETTLED",
    );
    assert_ne!(recovered, producer);
    assert_ne!(recovered, resumed);
    handoff["exited_pids"] = json!([producer, resumed, recovered]);
    write_json(&handoff_path, &handoff);
}

#[test]
fn actual_bank_recovery_three_processes_preserve_fixed36_and_current_receipt() {
    match std::env::var(STAGE).ok().as_deref() {
        None => coordinate(),
        Some("producer") => producer(Path::new(&std::env::var_os(COORDINATOR).unwrap())),
        Some("resume_before") => resume_before(Path::new(&std::env::var_os(SAVED_ROOT).unwrap())),
        Some("recover_after") => recover_after(Path::new(&std::env::var_os(SAVED_ROOT).unwrap())),
        Some(other) => panic!("unknown Bank process stage: {other}"),
    }
    if std::env::var_os(STAGE).is_some() {
        eprintln!(
            "actual Native Bank recovery child PID={}",
            std::process::id()
        );
    }
}
