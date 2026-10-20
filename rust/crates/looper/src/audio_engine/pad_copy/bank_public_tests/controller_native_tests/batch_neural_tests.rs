//! Actual model children, canonical sources, ordinary batch/Config and each PAD's own ALL ACK.
//! One-second synthetic stereo inputs; no audio device, simultaneous-voice or hearing claim.
use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};

const NEURAL_CHILD: &str = "FLITZIS_B1_NEURAL_CHILD";
const PROFILE_PREFIX: &str = "b1_neural_child_profile ";
const SOURCE_FRAMES: usize = 48_000;
const BUDGET_MIB: usize = 512;

#[derive(Clone, Copy)]
struct NeuralCase {
    separator: &'static str,
    label: &'static str,
    full: bool,
    distinct: bool,
}

impl NeuralCase {
    fn ids(self) -> Vec<usize> {
        if self.full {
            (0..NUM_SAMPLES).collect()
        } else {
            vec![0, NUM_SAMPLES - 1]
        }
    }

    fn jobs(self) -> usize {
        if self.distinct { self.ids().len() } else { 1 }
    }
}

fn bound_seconds(name: &str, default: u64) -> Duration {
    let seconds = match std::env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("{name} must be an integer number of seconds")),
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => panic!("{name} is unavailable: {error}"),
    };
    assert!(
        (30..=86_400).contains(&seconds),
        "{name} must be30..86400 seconds"
    );
    Duration::from_secs(seconds)
}

fn neural_directory(label: &str) -> tempfile::TempDir {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let parent = workspace.join("test-artifacts/b1-stem-batch-neural-20261020");
    for ancestor in parent.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => assert_eq!(metadata.file_attributes() & 0x400, 0),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("owned neural fixture ancestor unavailable: {error}"),
        }
    }
    fs::create_dir_all(&parent).unwrap();
    for ancestor in parent.ancestors() {
        assert_eq!(
            fs::symlink_metadata(ancestor).unwrap().file_attributes() & 0x400,
            0
        );
    }
    let owned = tempfile::Builder::new()
        .prefix(&format!("{label}-"))
        .tempdir_in(parent)
        .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(owned.path().join("OWNER.md"))
        .unwrap()
        .write_all(b"Owned B1 actual-model batch software fixture. Generated one-second48000Hz stereo PCM, canonical copies and ordinary model outputs; no Human files or installations. Shared model caches are external and must not be deleted. Keep this directory if cooperative child/worker settlement is unproven.")
        .unwrap();
    owned
}

fn neural_source_wav(index: usize) -> Vec<u8> {
    // Every source has the full authored one-second extent. Identity comes from
    // decoded PCM across both channels, never a renamed file or shortened input.
    let mut encoded = Vec::with_capacity(44 + SOURCE_FRAMES * 4);
    let bytes = u32::try_from(SOURCE_FRAMES * 4).unwrap();
    encoded.extend_from_slice(b"RIFF");
    encoded.extend_from_slice(&(36 + bytes).to_le_bytes());
    encoded.extend_from_slice(b"WAVEfmt ");
    encoded.extend_from_slice(&16_u32.to_le_bytes());
    encoded.extend_from_slice(&1_u16.to_le_bytes());
    encoded.extend_from_slice(&2_u16.to_le_bytes());
    encoded.extend_from_slice(&RATE.to_le_bytes());
    encoded.extend_from_slice(&(RATE * 4).to_le_bytes());
    encoded.extend_from_slice(&4_u16.to_le_bytes());
    encoded.extend_from_slice(&16_u16.to_le_bytes());
    encoded.extend_from_slice(b"data");
    encoded.extend_from_slice(&bytes.to_le_bytes());
    for frame in 0..SOURCE_FRAMES {
        let time = frame as f64 / f64::from(RATE);
        let envelope = (frame.min(SOURCE_FRAMES - 1 - frame) as f64 / 240.0).min(1.0);
        for channel in 0..2 {
            let phase = channel as f64 * 0.37;
            let frequency = 110.0 + index as f64 * 1.37;
            let tone = (std::f64::consts::TAU * frequency * time + phase).sin() * 3500.0
                + (std::f64::consts::TAU * frequency * 2.013 * time + phase).sin() * 1700.0
                + (std::f64::consts::TAU * (43.0 + index as f64 * 0.11) * time).sin() * 1100.0;
            encoded.extend_from_slice(&((tone * envelope).round() as i16).to_le_bytes());
        }
    }
    assert_eq!(encoded.len(), 44 + SOURCE_FRAMES * 4);
    encoded
}

fn neural_sources(
    engine: &AudioEngine,
    callback: &mut Callback,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    root: &Path,
    case: NeuralCase,
) -> Vec<Value> {
    let ids = case.ids();
    let mut rows = vec![Value::Null; NUM_SAMPLES];
    for material_index in 0..case.jobs() {
        let source = root.join(format!("input-{material_index}.wav"));
        fs::write(&source, neural_source_wav(material_index)).unwrap();
        let material =
            prepare_material_complete_for_test(root, &source, RATE, 2, &|| false).unwrap();
        assert_eq!(material.sample.frame_count(), SOURCE_FRAMES);
        assert_eq!(material.sample.channels, 2);
        let metadata = material.metadata();
        let preparation = prepared_for_test(engine, material, root).unwrap();
        let targets = if case.distinct {
            vec![ids[material_index]]
        } else {
            ids.clone()
        };
        for id in targets {
            let ticket = adopt_for_format(
                engine,
                id,
                &preparation,
                producer.clone(),
                (2, RATE, root.to_path_buf()),
            )
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            while engine.cold_loading[id].load(Ordering::Acquire) != 0 {
                callback.drain();
                assert!(
                    Instant::now() < deadline,
                    "neural source{id} lacks its own cold ACK"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            let mut success = false;
            while let Ok(event) = engine
                .loader_rx
                .lock()
                .unwrap()
                .try_recv()
                .map(LoaderEvent::into_test_payload)
            {
                match event {
                    LoaderEvent::Success { request_id, .. }
                        if request_id == ticket.request_id() =>
                    {
                        success = true;
                    }
                    LoaderEvent::Error { error, .. } => panic!("neural source{id}: {error:?}"),
                    _ => {}
                }
            }
            assert!(success && ticket.is_current().unwrap());
            assert!(
                engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap()
                    .is_current()
            );
            rows[id] = metadata.clone();
        }
        preparation.release_preparation().unwrap();
    }
    let original: BTreeSet<_> = ids
        .iter()
        .map(|&id| engine.loaded_source_digest(id).unwrap().unwrap())
        .collect();
    let transformed: BTreeSet<_> = ids
        .iter()
        .map(|&id| engine.waveform_source_identity(id).unwrap().unwrap().1)
        .collect();
    assert_eq!(original.len(), case.jobs());
    assert_eq!(transformed.len(), case.jobs());
    assert!(original.iter().all(|digest| digest.len() == 64));
    assert!(transformed.iter().all(|digest| digest.len() == 64));
    assert_eq!(rows.iter().filter(|row| !row.is_null()).count(), ids.len());
    rows
}

fn neural_probe(engine: &Py<AudioEngine>, rows: &[Value], case: NeuralCase) -> Py<PyAny> {
    Python::attach(|py| {
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
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/stem_batch_neural_native_probe.py"
            )))
            .unwrap(),
            c"stem_batch_neural_native_probe.py",
            c"stem_batch_neural_native_probe",
        )
        .unwrap();
        let shape = Py::new(
            py,
            Batch216SourceShape {
                engine: engine.clone_ref(py),
            },
        )
        .unwrap();
        module
            .getattr("StemBatchNeuralProbe")
            .unwrap()
            .call1((
                engine.clone_ref(py),
                shape,
                serde_json::to_string(rows).unwrap(),
                case.separator,
                case.label,
                case.jobs(),
            ))
            .unwrap()
            .unbind()
    })
}

fn probe_text(probe: &Py<PyAny>, method: &str) -> String {
    Python::attach(|py| {
        probe
            .call_method0(py, method)
            .unwrap()
            .extract::<String>(py)
            .unwrap()
    })
}

/// Retain fixture files until actual cooperative child/worker and Native settlement.
/// Failed assertions still initiate cancellation; Drop must never add another panic.
struct NeuralRun {
    probe: Option<Py<PyAny>>,
    callback: Option<Callback>,
    producer: Option<Arc<Mutex<Producer<ControlMessage>>>>,
    engine: Option<Py<AudioEngine>>,
    directory: Option<tempfile::TempDir>,
    previous_directory: PathBuf,
    cleanup_bound: Duration,
    cleanup_result: Option<Result<(), String>>,
    embedded_python_executable: String,
}

impl NeuralRun {
    fn cleanup(&mut self) -> Result<(), String> {
        if let Some(result) = &self.cleanup_result {
            return result.clone();
        }
        let result = self.settle();
        self.cleanup_result = Some(result.clone());
        result
    }

    fn settle(&mut self) -> Result<(), String> {
        if let Some(probe) = &self.probe {
            Python::attach(|py| probe.call_method0(py, "close"))
                .map_err(|error| format!("cooperative close failed: {error}"))?;
        }
        let deadline = Instant::now() + self.cleanup_bound;
        loop {
            if let Some(callback) = &mut self.callback {
                callback.drain();
            }
            let pending = Python::attach(|py| -> PyResult<bool> {
                let python_pending = if let Some(probe) = &self.probe {
                    probe.call_method0(py, "poll")?;
                    probe
                        .call_method0(py, "process_pending")?
                        .extract::<bool>(py)?
                } else {
                    false
                };
                let native_pending = self.engine.as_ref().is_some_and(|engine| {
                    let engine = engine.borrow(py);
                    engine.cold_jobs.counts_for_test() != (0, 0, 0)
                        || engine
                            .cold_loading
                            .iter()
                            .any(|loading| loading.load(Ordering::Acquire) != 0)
                        || engine
                            .project_assets
                            .resident_budget_status(&engine.resident_account)
                            .map_or(true, |status| {
                                status.pending_reservation_count != 0
                                    || status.pending_resident_bytes != 0
                            })
                });
                Ok(python_pending || native_pending)
            })
            .map_err(|error| format!("cleanup settlement observation failed: {error}"))?;
            let controls_pending = self
                .callback
                .as_ref()
                .is_some_and(|callback| !callback.controls.is_empty());
            if !pending && !controls_pending {
                break;
            }
            if Instant::now() >= deadline {
                return Err(
                    "cooperative child/worker settlement exceeded its engineering bound".into(),
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let native_ownership = Python::attach(|py| {
            self.engine
                .as_ref()
                .map(|engine| Arc::downgrade(&engine.borrow(py).input_runtime_ownership))
        });
        Python::attach(|py| {
            if let Some(probe) = self.probe.take() {
                probe.drop_ref(py);
            }
            py.import("gc")?.call_method0("collect").map(|_| ())
        })
        .map_err(|error| format!("controller collection failed: {error}"))?;
        drop(self.callback.take());
        drop(self.producer.take());
        Python::attach(|py| {
            if let Some(engine) = self.engine.take() {
                engine.drop_ref(py);
            }
            py.import("gc")?.call_method0("collect").map(|_| ())
        })
        .map_err(|error| format!("Native collection failed: {error}"))?;
        if native_ownership.is_some_and(|ownership| ownership.upgrade().is_some()) {
            return Err("actual Native authority owner remains after explicit release".into());
        }
        Python::attach(|py| {
            py.import("sys")?
                .setattr("executable", &self.embedded_python_executable)
        })
        .map_err(|error| format!("embedded interpreter restoration failed: {error}"))?;
        std::env::set_current_dir(&self.previous_directory)
            .map_err(|error| format!("fixture working-directory restoration failed: {error}"))?;
        Ok(())
    }
}

impl Drop for NeuralRun {
    fn drop(&mut self) {
        let settled = matches!(
            catch_unwind(AssertUnwindSafe(|| self.cleanup())),
            Ok(Ok(()))
        );
        if !settled || std::thread::panicking() {
            if let Some(directory) = self.directory.take() {
                eprintln!(
                    "b1 neural fixture retained: {} settlement={:?}",
                    directory.keep().display(),
                    self.cleanup_result
                );
            }
        }
        // Restore best-effort even if cancellation/collection failed. No deletion
        // or alternate cleanup path is attempted for an unproven retained fixture.
        if let Err(error) = std::env::set_current_dir(&self.previous_directory) {
            eprintln!("b1 neural working-directory restoration: {error}");
        }
    }
}

fn wait_neural(run: &mut NeuralRun, began: Instant, total: Duration, stall: Duration, all: bool) {
    let phase = if all { "all_ready" } else { "complete" };
    let mut last_marker = None;
    let mut last_progress = Instant::now();
    let mut next_observation = last_progress;
    let mut next_heartbeat = last_progress + Duration::from_secs(30);
    loop {
        run.callback.as_mut().unwrap().drain();
        let probe = run.probe.as_ref().unwrap();
        let ready = Python::attach(|py| {
            probe.call_method0(py, "poll").unwrap();
            if all {
                probe.call_method0(py, "adopt_all").unwrap();
            }
            probe
                .call_method0(py, phase)
                .unwrap()
                .extract::<bool>(py)
                .unwrap()
        });
        if ready {
            return;
        }
        let now = Instant::now();
        if now >= next_observation {
            let marker = Python::attach(|py| {
                (
                    probe
                        .call_method0(py, "progress_marker")
                        .unwrap()
                        .extract::<(u64, u64, u64, u64, u64)>(py)
                        .unwrap(),
                    probe
                        .call_method0(py, "process_pending")
                        .unwrap()
                        .extract::<bool>(py)
                        .unwrap(),
                )
            });
            if last_marker != Some(marker) {
                last_marker = Some(marker);
                last_progress = now;
            }
            assert!(
                now.duration_since(last_progress) < stall,
                "{phase}: frozen semantic/child-CPU progress for{}s: {}",
                stall.as_secs(),
                probe_text(probe, "diagnostics")
            );
            assert!(
                now.duration_since(began) < total,
                "{phase}: total engineering bound{}s: {}",
                total.as_secs(),
                probe_text(probe, "diagnostics")
            );
            next_observation = now + Duration::from_secs(1);
            if now >= next_heartbeat {
                println!(
                    "b1_neural_progress phase={phase} elapsed_s={} marker={marker:?}",
                    began.elapsed().as_secs_f64()
                );
                next_heartbeat = now + Duration::from_secs(30);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn neural_profile(case: NeuralCase) {
    // Capture true system/caller context before CPython imports or source setup.
    // These test-only queries are separate from the numeric process-delta API.
    let host_before_python = crate::audio_engine::c3_process_metrics::host_snapshot()
        .expect("actual baseline host memory query must succeed");
    let current_job_context = crate::audio_engine::c3_process_metrics::current_job_context();
    Python::initialize();
    // PyO3 embeds the base runtime in libtest; sys.executable otherwise names
    // this Rust EXE. Configure the actual uv-discovered command interpreter
    // before original producer capture or workers, preserving every production
    // command/cancellation/lineage check. The dedicated child runs one case.
    let command_python = PathBuf::from(
        std::env::var_os("FLITZIS_B1_NEURAL_PYTHON")
            .expect("explicit uv-discovered Python interpreter is required"),
    );
    assert!(command_python.is_absolute() && command_python.is_file());
    let command_python = command_python.canonicalize().unwrap();
    assert_eq!(command_python.file_name().unwrap(), "python.exe");
    assert_ne!(
        command_python,
        std::env::current_exe().unwrap().canonicalize().unwrap()
    );
    let embedded_python_executable = Python::attach(|py| {
        let sys = py.import("sys").unwrap();
        let original = sys
            .getattr("executable")
            .unwrap()
            .extract::<String>()
            .unwrap();
        sys.setattr("executable", command_python.to_string_lossy().as_ref())
            .unwrap();
        original
    });
    let runtime_configuration = json!({
        "embedded_original_executable": embedded_python_executable,
        "configured_command_interpreter": command_python,
        "scope": "explicit test-only embedded-runtime configuration before unchanged production capture and commands",
    });
    let total = bound_seconds("FLITZIS_B1_NEURAL_TOTAL_SECONDS", 7200);
    let stall = bound_seconds("FLITZIS_B1_NEURAL_STALL_SECONDS", 600);
    let began = Instant::now();
    let process_before = crate::audio_engine::c3_process_metrics::snapshot();
    let directory = neural_directory(case.label);
    let previous_directory = std::env::current_dir().unwrap();
    let mut run = NeuralRun {
        probe: None,
        callback: None,
        producer: None,
        engine: None,
        directory: Some(directory),
        previous_directory,
        cleanup_bound: stall,
        cleanup_result: None,
        embedded_python_executable,
    };
    std::env::set_current_dir(run.directory.as_ref().unwrap().path()).unwrap();
    let root = run.directory.as_ref().unwrap().path().join("samples");
    fs::create_dir(&root).unwrap();
    let (engine, callback, producer) = empty_engine();
    run.callback = Some(callback);
    run.producer = Some(producer);
    engine
        .project_assets
        .set_resident_budget_mib(&engine.resident_account, BUDGET_MIB)
        .unwrap();
    run.engine = Some(Python::attach(|py| Py::new(py, engine).unwrap()));
    let rows = Python::attach(|py| {
        neural_sources(
            &run.engine.as_ref().unwrap().borrow(py),
            run.callback.as_mut().unwrap(),
            run.producer.as_ref().unwrap(),
            &root,
            case,
        )
    });
    assert!(
        !run.callback
            .as_ref()
            .unwrap()
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active)
    );
    run.probe = Some(neural_probe(run.engine.as_ref().unwrap(), &rows, case));
    call(run.probe.as_ref().unwrap(), "start");
    wait_neural(&mut run, began, total, stall, false);
    wait_neural(&mut run, began, total, stall, true);
    let callback = run.callback.as_mut().unwrap();
    callback.drain();
    for id in case.ids() {
        assert!(callback.mixer.bank_for_measurement()[id].is_some());
        assert!(callback.mixer.stems_for_measurement()[id].is_some());
        assert_eq!(
            callback.mixer.stem_demand_for_measurement(id).0,
            StemMixMode::AllStems
        );
    }
    assert!(!callback.mixer.voices.iter().any(|voice| voice.active));
    assert!(callback.controls.is_empty());
    let budget = Python::attach(|py| {
        let engine = run.engine.as_ref().unwrap().borrow(py);
        let status = engine
            .project_assets
            .resident_budget_status(&engine.resident_account)
            .unwrap();
        assert_eq!(status.budget_mib, BUDGET_MIB);
        assert!(!status.over_budget);
        assert!(status.held_unique_pcm_bytes > 0);
        assert_eq!(status.pending_reservation_count, 0);
        assert_eq!(status.pending_resident_bytes, 0);
        assert!(status.admitted_total_bytes <= status.budget_bytes);
        for id in case.ids() {
            assert!(
                engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap()
                    .is_current()
            );
        }
        json!({"budget_mib":status.budget_mib,"budget_bytes":status.budget_bytes,
            "held_unique_pcm_bytes":status.held_unique_pcm_bytes,
            "admitted_total_bytes":status.admitted_total_bytes,
            "pending_reservation_count":status.pending_reservation_count,
            "pending_resident_bytes":status.pending_resident_bytes})
    });
    let controller: Value =
        serde_json::from_str(&probe_text(run.probe.as_ref().unwrap(), "finish")).unwrap();
    // This process-wide service already owns its lifetime. Retaining its Arc
    // adds no bank/PCM/engine authority and permits the established collection
    // boundary after both Python/controller and Native owners have been dropped.
    let project_assets = Python::attach(|py| {
        run.engine
            .as_ref()
            .unwrap()
            .borrow(py)
            .project_assets
            .clone()
    });
    let before_shutdown = crate::audio_engine::c3_process_metrics::snapshot();
    run.cleanup()
        .unwrap_or_else(|error| panic!("neural fixture cleanup unproven: {error}"));
    // Preserve the original combined-teardown observation before requesting
    // any extra existing service pass. This is not a Native-only drop baseline.
    let process_after = crate::audio_engine::c3_process_metrics::snapshot();
    let (retiring_before, readers_before, deleted_before, errors_before) =
        project_assets.status().unwrap();
    let cold_cleanup_before = crate::audio_engine::cold_store::cleanup_status().unwrap();
    let cold_admission_before = crate::audio_engine::cold_store::cleanup_admission_status();
    // Reuse exactly one explicit pass from the declared216 harness. Ordinary
    // background workers remain active; pending/error counts are observations,
    // never forced to zero or promoted into a claim that every service is idle.
    project_assets.collect_for_test();
    let (retiring_after, readers_after, deleted_after, errors_after) =
        project_assets.status().unwrap();
    let cold_cleanup_after = crate::audio_engine::cold_store::cleanup_status().unwrap();
    let cold_admission_after = crate::audio_engine::cold_store::cleanup_admission_status();
    drop(project_assets);
    // Registry locks and temporary Arc custody have ended before C3 observes
    // the same process/creation identity. Python/modules remain loaded.
    let process_after_asset_collect = crate::audio_engine::c3_process_metrics::snapshot();
    let host_after_asset_collect = crate::audio_engine::c3_process_metrics::host_snapshot()
        .expect("actual final host memory query must succeed");
    let evidence = json!({"case":case.label,"separator":case.separator,
        "runtime_configuration": runtime_configuration,
        "occupied_pads":case.ids(),"expected_jobs":case.jobs(),"distinct":case.distinct,
        "source_shape":{"sample_rate_hz":RATE,"channels":2,"frames":SOURCE_FRAMES},
        "engineering_bounds":{"total_seconds":total.as_secs(),"stall_seconds":stall.as_secs()},
        "elapsed_seconds":began.elapsed().as_secs_f64(),"controller":controller,
        "native_pcm_budget":budget,"native_process_before":process_before,
        "native_process_before_shutdown":before_shutdown,"native_process_after":process_after,
        "native_process_delta":crate::audio_engine::c3_process_metrics::delta(&process_before,&process_after),
        "host_context": {
            "before_python_imports_and_source_setup": host_before_python,
            "after_combined_teardown_and_asset_collection": host_after_asset_collect,
            "baseline_current_job": current_job_context,
            "scope": "baseline/final read-only caller and true system commit context; current Job only, not all enclosing restrictions; no resource admission policy"
        },
        "process_phases": {
            "before_probe_imports_and_profile": process_before,
            "before_combined_controller_native_shutdown": before_shutdown,
            "after_combined_controller_native_shutdown_before_asset_collect": process_after,
            "after_combined_controller_native_shutdown_after_asset_collect": process_after_asset_collect,
            "asset_collect_process_delta": crate::audio_engine::c3_process_metrics::delta(
                &process_after, &process_after_asset_collect),
            "whole_profile_after_asset_collect_delta": crate::audio_engine::c3_process_metrics::delta(
                &process_before, &process_after_asset_collect),
            "explicit_asset_collection_passes": 1,
            "test_engine_asset_registry_before_collect": {
                "retiring": retiring_before, "readers": readers_before,
                "deleted": deleted_before, "errors": errors_before,
            },
            "test_engine_asset_registry_after_collect": {
                "retiring": retiring_after, "readers": readers_after,
                "deleted": deleted_after, "errors": errors_after,
            },
            "process_wide_cold_cleanup_before_collect": cold_cleanup_before,
            "process_wide_cold_cleanup_after_collect": cold_cleanup_after,
            "process_wide_cold_cleanup_admission_before_collect": cold_admission_before,
            "process_wide_cold_cleanup_admission_after_collect": cold_admission_after,
            "scope": "same PID/creation identity; original combined teardown remains separate; one explicit existing ProjectAssets pass with ordinary background workers still active; registry census is reader/retirement totals, not exact File/producer-pin census; imported Python/modules/shared services remain loaded; no zero/idle/leak or Native-only baseline inference",
        },
        "cleanup_settled":true,
        "scope":"actual model subprocess/batch/Config/own Native ACK software profile; Native PCM budget excludes Python/model/child-process memory; parent and child process counters have separate scope; no music/device or216simultaneous-voice acceptance"});
    println!("{PROFILE_PREFIX}{evidence}");
}

fn isolated_neural(name: &str, case: NeuralCase) {
    if std::env::var_os(NEURAL_CHILD).is_some() {
        neural_profile(case);
        return;
    }
    let (_, modules) = module_path!().split_once("::").unwrap();
    let test = format!("{modules}::{name}");
    let before = crate::audio_engine::c3_process_metrics::snapshot();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &test,
            "--include-ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(NEURAL_CHILD, "1")
        .output()
        .unwrap();
    let after = crate::audio_engine::c3_process_metrics::snapshot();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "neural child {test}:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "neural child did not run exactly once:\n{stdout}\n{stderr}"
    );
    let reports: Vec<_> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix(PROFILE_PREFIX))
        .collect();
    assert_eq!(
        reports.len(),
        1,
        "one neural profile required:\n{stdout}\n{stderr}"
    );
    let mut evidence: Value = serde_json::from_str(reports[0]).unwrap();
    evidence["launcher_process_before"] = before.clone();
    evidence["launcher_process_after"] = after.clone();
    evidence["launcher_process_delta"] =
        crate::audio_engine::c3_process_metrics::delta(&before, &after);
    println!("b1_neural_profile {evidence}");
}

#[test]
#[ignore = "explicit actual stock-model CPU resource profile; requires installed model assets"]
fn neural_demucs_two_distinct_sources_commit_and_accept_own_all() {
    isolated_neural(
        "neural_demucs_two_distinct_sources_commit_and_accept_own_all",
        NeuralCase {
            separator: "demucs:htdemucs",
            label: "demucs-two-distinct",
            full: false,
            distinct: true,
        },
    );
}

#[test]
#[ignore = "explicit actual stock-model CPU resource profile; requires installed model assets"]
fn neural_bs_roformer_two_distinct_sources_commit_and_accept_own_all() {
    isolated_neural(
        "neural_bs_roformer_two_distinct_sources_commit_and_accept_own_all",
        NeuralCase {
            separator: "bs-roformer:musdb18hq",
            label: "bs-roformer-two-distinct",
            full: false,
            distinct: true,
        },
    );
}

#[test]
#[ignore = "explicit full216 actual Demucs profile; bounded real model work"]
fn neural_demucs_full216_duplicate_sources_commit_and_accept_own_all() {
    isolated_neural(
        "neural_demucs_full216_duplicate_sources_commit_and_accept_own_all",
        NeuralCase {
            separator: "demucs:htdemucs",
            label: "demucs216-duplicate",
            full: true,
            distinct: false,
        },
    );
}

#[test]
#[ignore = "explicit full216 actual Demucs profile; bounded real model work"]
fn neural_demucs_full216_distinct_sources_commit_and_accept_own_all() {
    isolated_neural(
        "neural_demucs_full216_distinct_sources_commit_and_accept_own_all",
        NeuralCase {
            separator: "demucs:htdemucs",
            label: "demucs216-distinct",
            full: true,
            distinct: true,
        },
    );
}

#[test]
#[ignore = "explicit full216 actual BS-RoFormer profile; bounded real model work"]
fn neural_bs_roformer_full216_duplicate_sources_commit_and_accept_own_all() {
    isolated_neural(
        "neural_bs_roformer_full216_duplicate_sources_commit_and_accept_own_all",
        NeuralCase {
            separator: "bs-roformer:musdb18hq",
            label: "bs-roformer216-duplicate",
            full: true,
            distinct: false,
        },
    );
}

#[test]
#[ignore = "explicit full216 actual BS-RoFormer profile; bounded real model work"]
fn neural_bs_roformer_full216_distinct_sources_commit_and_accept_own_all() {
    isolated_neural(
        "neural_bs_roformer_full216_distinct_sources_commit_and_accept_own_all",
        NeuralCase {
            separator: "bs-roformer:musdb18hq",
            label: "bs-roformer216-distinct",
            full: true,
            distinct: true,
        },
    );
}
