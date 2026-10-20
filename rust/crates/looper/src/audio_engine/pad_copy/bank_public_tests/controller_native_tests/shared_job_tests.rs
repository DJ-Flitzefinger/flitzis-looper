//! Real Bank -> running shared job -> original follower's own component ACK.
//! Only a declared nonneural five-WAV backend and headless shape routing differ.
use super::*;
use crate::audio_engine::{OfflineStemSourceLease, PreparedSourceTicket};
use pyo3::PyResult;

#[path = "batch_cancel_tests.rs"]
mod batch_cancel_tests;
#[path = "shared_crossing_tests.rs"]
mod shared_crossing_tests;

#[pyclass]
struct SharedJobSourceAudio {
    engine: Py<AudioEngine>,
}

#[pymethods]
impl SharedJobSourceAudio {
    fn loaded_source_digest(&self, py: Python<'_>, id: usize) -> PyResult<Option<String>> {
        self.engine.borrow(py).loaded_source_digest(id)
    }

    fn loaded_sample_shape(&self, py: Python<'_>, id: usize) -> PyResult<(u32, usize, usize)> {
        assert!([36, 71, 215].contains(&id));
        let engine = self.engine.borrow(py);
        assert!(engine.loaded_source_digest(id)?.is_some());
        let cache = engine.sample_cache.lock().unwrap();
        let sample = cache[id].as_ref().unwrap();
        let generations = engine.loaded_source_generations.lock().unwrap();
        let (generation, rate) = generations[id];
        assert!(generation > 0 && rate > 0);
        Ok((rate, sample.channels, sample.frame_count()))
    }

    fn capture_offline_stem_source(
        &self,
        py: Python<'_>,
        id: usize,
        version: String,
    ) -> PyResult<OfflineStemSourceLease> {
        self.engine
            .borrow(py)
            .capture_offline_stem_source(id, version)
    }

    fn offline_stem_source_is_current(
        &self,
        py: Python<'_>,
        id: usize,
        source: &OfflineStemSourceLease,
    ) -> PyResult<bool> {
        self.engine
            .borrow(py)
            .offline_stem_source_is_current(id, source)
    }

    fn capture_prepared_stem_source(
        &self,
        py: Python<'_>,
        id: usize,
        source: &OfflineStemSourceLease,
    ) -> PyResult<PreparedSourceTicket> {
        self.engine
            .borrow(py)
            .capture_prepared_stem_source(id, source)
    }
}

struct ReleaseBackend(Py<PyAny>);

impl Drop for ReleaseBackend {
    fn drop(&mut self) {
        Python::attach(|py| {
            self.0.call_method0(py, "release_test_backend").unwrap();
        });
    }
}

fn shared_controller(fixture: &mut Fixture) -> Py<PyAny> {
    fixture.with_engine(|engine| engine.stop_sample(0).unwrap());
    fixture.callback.drain();
    let seed = probe(&fixture.engine, "Probe", Some(fixture_input(fixture)));
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::SetKeyLock(false))
        .unwrap();
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::SetStemMixMode {
            id: 0,
            mode: StemMixMode::FullMix,
            source_version_hash: 0,
        })
        .unwrap();
    let controller = Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/bank_shared_job_native_probe.py"
            )))
            .unwrap(),
            c"bank_shared_job_native_probe.py",
            c"bank_shared_job_native_probe",
        )
        .unwrap();
        let source = Py::new(
            py,
            SharedJobSourceAudio {
                engine: fixture.engine.clone_ref(py),
            },
        )
        .unwrap();
        let outputs = [wav(&[1]), wav(&[0]), wav(&[2]), wav(&[3]), wav(&[0, 2, 3])]
            .map(|bytes| pyo3::types::PyBytes::new(py, &bytes).unbind());
        let output_tuple = pyo3::types::PyTuple::new(py, outputs).unwrap();
        let controller = module
            .getattr("SharedJobProbe")
            .unwrap()
            .call1((seed.bind(py), source, output_tuple))
            .unwrap();
        controller.call_method0("prepare_full_mix").unwrap();
        controller.unbind()
    });
    fixture.callback.drain();
    fixture.start(0);
    let warm = render_fixed(fixture, WARMUP);
    assert!(warm.iter().all(|sample| sample.is_finite()));
    assert!(warm.iter().any(|sample| sample.abs() > 1e-4));
    controller
}

fn bool_call(controller: &Py<PyAny>, method: &str, clear: Option<bool>) -> bool {
    Python::attach(|py| {
        let result = if let Some(clear) = clear {
            controller.call_method1(py, method, (if clear { "clear" } else { "copy" },))
        } else {
            controller.call_method0(py, method)
        };
        result.unwrap().extract(py).unwrap()
    })
}

fn stage(controller: &Py<PyAny>, method: &str, clear: bool) {
    Python::attach(|py| {
        controller
            .call_method1(py, method, (if clear { "clear" } else { "copy" },))
            .unwrap()
    });
}

fn wait_stage(
    fixture: &mut Fixture,
    controller: &Py<PyAny>,
    method: &str,
    clear: Option<bool>,
    callback: bool,
) {
    wait_for_with_diagnostics(
        || {
            if callback {
                fixture.callback.drain();
            }
            bool_call(controller, method, clear)
        },
        method,
        || {
            Python::attach(|py| {
                controller
                    .call_method0(py, "diagnostics")
                    .unwrap()
                    .extract(py)
                    .unwrap()
            })
        },
    );
}

fn shared_profile(clear: bool) {
    use std::io::Write;
    use std::os::windows::fs::MetadataExt;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let parent = workspace.join("test-artifacts/bank36-shared-job-20261020");
    for ancestor in parent.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => assert_eq!(
                metadata.file_attributes() & 0x400,
                0,
                "shared Bank fixture must not traverse a reparse point"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("shared Bank fixture ancestor is unavailable: {error}"),
        }
    }
    fs::create_dir_all(&parent).unwrap();
    for ancestor in parent.ancestors() {
        assert_eq!(
            fs::symlink_metadata(ancestor).unwrap().file_attributes() & 0x400,
            0,
            "created shared Bank fixture must not traverse a reparse point"
        );
    }
    let owned_root = tempfile::Builder::new()
        .prefix("shared-bank-")
        .tempdir_in(parent)
        .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(owned_root.path().join("OWNER.md"))
        .unwrap()
        .write_all(b"Owned generated Bank/SharedStemJob Native fixture; no Human data or installation. Released after actual jobs and fixture readers.")
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
    for slot in if clear { [36, 71] } else { [36, 215] } {
        seed(&mut fixture, &controller, slot);
    }
    stage(&controller, "start_shared_job", clear);
    wait_stage(&mut fixture, &controller, "shared_job_running", None, false);
    let source_before = proof(&fixture, 0);
    let old_target = model_identity(&controller, 36).unwrap();
    stage(&controller, "begin_shared_bank", clear);
    wait_stage(
        &mut fixture,
        &controller,
        "shared_bank_pending",
        None,
        false,
    );
    assert!(matches!(
        fixture.callback.controls.peek().unwrap(),
        ControlMessage::BankMutation(_)
    ));
    let ticket: Py<BankMutationTicket> = Python::attach(|py| {
        controller
            .call_method0(py, "shared_bank_ticket")
            .unwrap()
            .extract(py)
            .unwrap()
    });
    assert_eq!(fixture.callback.drain(), 1);
    Python::attach(|py| {
        let native = ticket.borrow(py);
        assert_eq!(native.publication_status(), "accepted");
        let ack = native.acknowledgement().unwrap();
        fixture
            .with_engine(|engine| assert!(engine.validate_bank_mutation(&native, &ack).unwrap()));
    });
    stage(&controller, "settle_shared_bank", clear);
    assert_eq!(proof(&fixture, 0), source_before);
    if clear {
        for slot in 36..72 {
            assert!(fixture.callback.mixer.bank_for_measurement()[slot].is_none());
            assert!(fixture.callback.mixer.stems_for_measurement()[slot].is_none());
            assert!(model_identity(&controller, slot).is_none());
        }
    } else {
        assert_ne!(model_identity(&controller, 36), Some(old_target));
        assert_eq!(
            model_parent(&controller, 36),
            model_identity(&controller, 0)
        );
        call(&controller, "request_follower_all");
        wait_stage(&mut fixture, &controller, "follower_all_ready", None, true);
    }
    drop(ticket);
    call(&controller, "release_backend");
    wait_stage(
        &mut fixture,
        &controller,
        "completion_ready",
        Some(clear),
        true,
    );
    if clear {
        for slot in 36..72 {
            assert!(fixture.callback.mixer.bank_for_measurement()[slot].is_none());
            assert!(fixture.callback.mixer.stems_for_measurement()[slot].is_none());
        }
    } else {
        assert!(fixture.callback.mixer.stems_for_measurement()[215].is_some());
        assert!(fixture.callback.mixer.bank_for_measurement()[215].is_some());
        assert_eq!(
            fixture.callback.mixer.stem_demand_for_measurement(215).0,
            StemMixMode::AllStems
        );
    }
    assert!(fixture.callback.controls.is_empty());
    drop(release);
    close(controller);
    drop(previous);
    drop(fixture);
    drop(owned_root);
}

#[test]
fn actual_bank_copy_shared_job_publishes_only_the_original_follower() {
    isolated(
        "bank_public_tests::controller_native_tests::shared_job_tests::actual_bank_copy_shared_job_publishes_only_the_original_follower",
        || shared_profile(false),
    );
}

#[test]
fn actual_bank_clear_shared_job_retains_final_read_until_stale_completion() {
    isolated(
        "bank_public_tests::controller_native_tests::shared_job_tests::actual_bank_clear_shared_job_retains_final_read_until_stale_completion",
        || shared_profile(true),
    );
}
