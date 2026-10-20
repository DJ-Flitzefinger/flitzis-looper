//! Original shared completion returns while the real Bank parent owns Config.
//! A child of shared_job_tests so the real fixture/backend/source adapters are reused.
use super::*;

fn crossing_observer(controller: &Py<PyAny>) -> Py<PyAny> {
    Python::attach(|py| {
        PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/bank_pending_shared_job_native_probe.py"
            )))
            .unwrap(),
            c"bank_pending_shared_job_native_probe.py",
            c"bank_pending_shared_job_native_probe",
        )
        .unwrap()
        .getattr("PendingSharedJobProbe")
        .unwrap()
        .call1((controller.bind(py),))
        .unwrap()
        .unbind()
    })
}

fn wait_crossing(fixture: &mut Fixture, observer: &Py<PyAny>, method: &str, callback: bool) {
    wait_for_with_diagnostics(
        || {
            if callback {
                fixture.callback.drain();
            }
            bool_call(observer, method, None)
        },
        method,
        || {
            Python::attach(|py| {
                observer
                    .call_method0(py, "diagnostics")
                    .unwrap()
                    .extract(py)
                    .unwrap()
            })
        },
    );
}

fn crossing_profile() {
    use std::io::Write;
    use std::os::windows::fs::MetadataExt;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let parent = workspace.join("test-artifacts/bank36-shared-crossing-20261020");
    for ancestor in parent.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => assert_eq!(
                metadata.file_attributes() & 0x400,
                0,
                "pending shared Bank fixture must not traverse a reparse point"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("pending shared Bank fixture ancestor is unavailable: {error}"),
        }
    }
    fs::create_dir_all(&parent).unwrap();
    for ancestor in parent.ancestors() {
        assert_eq!(
            fs::symlink_metadata(ancestor).unwrap().file_attributes() & 0x400,
            0,
            "created pending shared Bank fixture must not traverse a reparse point"
        );
    }
    let owned_root = tempfile::Builder::new()
        .prefix("cross-")
        .tempdir_in(parent)
        .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(owned_root.path().join("OWNER.md"))
        .unwrap()
        .write_all(b"Owned generated Bank/shared-result writer-crossing fixture. Synthetic source and declared five-WAV backend; no Human data or installation.")
        .unwrap();
    let mut fixture = Fixture::with_pair_registration_at(
        true,
        true,
        false,
        "tap",
        Some(owned_root.path().to_path_buf()),
    );
    let previous = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(fixture.directory.path()).unwrap();
    let controller = shared_controller(&mut fixture);
    let release = ReleaseBackend(Python::attach(|py| controller.clone_ref(py)));
    for slot in [36, 215] {
        seed(&mut fixture, &controller, slot);
    }
    stage(&controller, "start_shared_job", false);
    wait_stage(&mut fixture, &controller, "shared_job_running", None, false);
    let source_before = proof(&fixture, 0);
    let old_target = model_identity(&controller, 36).unwrap();
    let original_follower = model_identity(&controller, 215).unwrap();
    stage(&controller, "begin_shared_bank", false);
    wait_stage(
        &mut fixture,
        &controller,
        "shared_bank_pending",
        None,
        false,
    );
    let ticket: Py<BankMutationTicket> = Python::attach(|py| {
        controller
            .call_method0(py, "shared_bank_ticket")
            .unwrap()
            .extract(py)
            .unwrap()
    });
    let observer = crossing_observer(&controller);
    call(&observer, "release_while_pending");
    wait_crossing(
        &mut fixture,
        &observer,
        "completion_retained_before_ack",
        false,
    );
    assert_eq!(proof(&fixture, 0), source_before);
    assert_eq!(model_identity(&controller, 36), Some(old_target.clone()));
    assert_eq!(
        model_identity(&controller, 215),
        Some(original_follower.clone())
    );
    assert!(matches!(
        fixture.callback.controls.peek().unwrap(),
        ControlMessage::BankMutation(_)
    ));
    Python::attach(|py| {
        assert_eq!(ticket.borrow(py).publication_status(), "pending");
        assert!(ticket.borrow(py).acknowledgement().is_none());
    });
    assert_eq!(fixture.callback.drain(), 1);
    Python::attach(|py| {
        let native = ticket.borrow(py);
        assert_eq!(native.publication_status(), "accepted");
        let ack = native.acknowledgement().unwrap();
        fixture
            .with_engine(|engine| assert!(engine.validate_bank_mutation(&native, &ack).unwrap()));
    });
    wait_crossing(&mut fixture, &observer, "settle_bank", false);
    assert_eq!(proof(&fixture, 0), source_before);
    assert_ne!(model_identity(&controller, 36), Some(old_target));
    assert_eq!(
        model_parent(&controller, 36),
        model_identity(&controller, 0)
    );
    assert_eq!(model_identity(&controller, 215), Some(original_follower));
    drop(ticket);
    call(&controller, "request_follower_all");
    wait_crossing(&mut fixture, &observer, "completion_ready", true);
    assert_eq!(proof(&fixture, 0), source_before);
    assert!(fixture.callback.mixer.stems_for_measurement()[215].is_some());
    assert!(fixture.callback.mixer.bank_for_measurement()[215].is_some());
    assert_eq!(
        fixture.callback.mixer.stem_demand_for_measurement(215).0,
        StemMixMode::AllStems
    );
    assert!(fixture.callback.controls.is_empty());
    drop(observer);
    drop(release);
    close(controller);
    drop(previous);
    drop(fixture);
    drop(owned_root);
}

#[test]
fn actual_shared_result_survives_pending_bank_config_writer() {
    isolated(
        "bank_public_tests::controller_native_tests::shared_job_tests::shared_crossing_tests::actual_shared_result_survives_pending_bank_config_writer",
        crossing_profile,
    );
}
