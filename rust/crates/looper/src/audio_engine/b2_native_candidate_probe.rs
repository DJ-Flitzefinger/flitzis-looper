//! Explicitly invoked complete native candidate evidence, without a CPAL stream.
//! The productive cold loader and offline reservation retain their ordinary limits.
use super::*;
use crate::audio_engine::analysis_jobs::OfflineRequestOwner;
use crate::audio_engine::complete_context::CompleteSourceReader;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyList;
use serde_json::{Value, json};
use std::ffi::CString;
use std::io::{BufWriter, Write};

#[pyclass]
struct NativeBridge {
    #[pyo3(get)]
    engine: Py<AudioEngine>,
}

#[pymethods]
impl NativeBridge {
    // This is the production begin_offline_analysis body after its stream-format
    // precondition. Cold preparation below supplies the actual format and ACK.
    fn begin_offline_analysis(
        &self,
        py: Python<'_>,
        id: usize,
    ) -> PyResult<crate::audio_engine::OfflineAnalysisJob> {
        if id != 0 {
            return Err(PyRuntimeError::new_err("probe only admits pad zero"));
        }
        let owner = self.engine.borrow(py);
        if owner.loading_sample_ids.lock().unwrap().contains(&id)
            || owner
                .active_tasks
                .lock()
                .unwrap()
                .iter()
                .any(|(pad, _)| *pad == id)
        {
            return Err(PyRuntimeError::new_err("native source is still loading"));
        }
        let sample = owner.sample_cache.lock().unwrap()[id]
            .clone()
            .ok_or_else(|| PyRuntimeError::new_err("native source is absent"))?;
        let reader =
            CompleteSourceReader::capture(&owner, id, sample).map_err(PyRuntimeError::new_err)?;
        let (generation, rate) = owner.loaded_source_generations.lock().unwrap()[id];
        owner
            .offline_jobs
            .begin_complete(
                id,
                reader,
                rate,
                generation,
                OfflineRequestOwner {
                    request_ids: owner.pad_request_ids.clone(),
                    prepared_epoch: owner.prepared_source_epochs[id].clone(),
                },
                owner.loader_tx.clone(),
            )
            .map_err(PyRuntimeError::new_err)
    }
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn native_source_proof(engine: &AudioEngine, output: &Path, rate: u32) -> Value {
    let sample = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let reader = CompleteSourceReader::capture(engine, 0, sample.clone()).unwrap();
    let lease = engine.cold_leases.lock().unwrap()[0].clone().unwrap();
    let manifest: Value =
        serde_json::from_str(&engine.cold_source_manifest(0).unwrap().unwrap()).unwrap();
    let path = output.join("complete-native-loaded-pcm.f32le");
    let mut file = BufWriter::new(fs::File::create(&path).unwrap());
    let mut interleaved = Sha256::new();
    let mut mono = Sha256::new();
    // CompleteSourceReader visits at most 4096 samples per chunk. Reuse bounded
    // byte scratch so Debug probes hash/write chunks rather than millions of
    // four-byte updates. Every sample and channel mean remains checked in order.
    let mut interleaved_bytes = Vec::with_capacity(4096 * 4);
    let mut mono_bytes = Vec::with_capacity(4096 * 4);
    let mut frames = 0;
    reader
        .visit_region(0..sample.frame_count(), &|| false, |first, values| {
            assert_eq!(first, frames);
            assert!(values.len() <= 4096);
            interleaved_bytes.clear();
            mono_bytes.clear();
            for frame in values.chunks_exact(sample.channels) {
                let mut sum = 0.0_f64;
                for value in frame {
                    assert!(value.is_finite(), "complete native PCM must be finite");
                    let bytes = value.to_le_bytes();
                    interleaved_bytes.extend_from_slice(&bytes);
                    sum += f64::from(*value);
                }
                mono_bytes
                    .extend_from_slice(&((sum / sample.channels as f64) as f32).to_le_bytes());
                frames += 1;
            }
            file.write_all(&interleaved_bytes)?;
            interleaved.update(&interleaved_bytes);
            mono.update(&mono_bytes);
            Ok(())
        })
        .unwrap();
    file.flush().unwrap();
    file.get_ref().sync_all().unwrap();
    drop(file);
    assert_eq!(frames, sample.frame_count());
    let interleaved_hash = format!("{:x}", interleaved.finalize());
    let mono_hash = format!("{:x}", mono.finalize());
    let pcm = &manifest["descriptor"]["playback"]["pcm"];
    assert_eq!(pcm["interleaved_sha256"], interleaved_hash);
    assert_eq!(pcm["mono_sha256"], mono_hash);
    assert_eq!(pcm["full_frames"].as_u64(), Some(frames as u64));
    assert_eq!(pcm["rate_hz"].as_u64(), Some(u64::from(rate)));
    assert_eq!(
        fs::metadata(&path).unwrap().len(),
        (frames * sample.channels * 4) as u64
    );
    assert!(sample.resident_start() > 0);
    assert!(sample.resident_end() < frames);
    assert!(
        engine
            .input_runtime_ownership
            .source_current(0, &sample, rate)
    );
    let snapshot = output.join("native-original-snapshot.bin");
    fs::copy(&lease.original_path, &snapshot).unwrap();
    let original_hash = full_hash(&snapshot);
    assert_eq!(
        manifest["descriptor"]["decoder"]["original"]["sha256"],
        original_hash
    );
    write_json(&output.join("cold-source-manifest.json"), &manifest);
    let generation = engine.loaded_source_generations.lock().unwrap()[0].0;
    let result = json!({
        "cold_manifest":manifest,
        "cold_original_path":lease.original_path,
        "cold_cache_path":lease.cache_path,
        "snapshot_retained_path":snapshot,
        "cached_original":{"path":snapshot,"sha256":original_hash,
            "bytes":fs::metadata(&snapshot).unwrap().len()},
        "complete_loaded_pcm":{"path":path,"sha256":interleaved_hash,
            "bytes":(frames*sample.channels*4),"full_frames":frames,
            "channels":sample.channels,"rate_hz":rate,"finite_values":true},
        "complete_mono_sha256":mono_hash,
        "resident_window":{"start_frame":sample.resident_start(),
            "end_frame":sample.resident_end(),"window_revision":sample.window_revision()},
        "acknowledged_source_generation":generation,
        "source_zero_frame":0,
        "scan":"CompleteSourceReader::capture/visit_region; full frame-zero to exclusive end",
        "hardware_started":false
    });
    write_json(&output.join("native-source-proof.json"), &result);
    result
}

#[test]
#[ignore = "explicit private source/model probe; no device, app, stream or recorder"]
fn b2_fresh_native_candidate_probe() {
    run_native_probe(false);
}

#[test]
#[ignore = "explicit private corrected QM probe; no device, app, stream or model"]
fn b2_corrected_legacy_native_probe() {
    run_native_probe(true);
}

fn run_native_probe(corrected_legacy: bool) {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    let attached_workspace = repository.parent().unwrap();
    let config_path = PathBuf::from(
        std::env::var_os(if corrected_legacy {
            "FLITZIS_B2_LEGACY_CONFIG"
        } else {
            "FLITZIS_B2_NATIVE_CONFIG"
        })
        .expect("explicit private B2 probe config"),
    );
    assert!(config_path.is_absolute());
    assert!(
        config_path
            .canonicalize()
            .unwrap()
            .starts_with(attached_workspace.join("scratch").canonicalize().unwrap())
    );
    let config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    let output = PathBuf::from(config["output_directory"].as_str().unwrap());
    let workspace = PathBuf::from(config["workspace"].as_str().unwrap());
    assert_eq!(
        workspace.canonicalize().unwrap(),
        attached_workspace.canonicalize().unwrap()
    );
    let relative_source = Path::new(config["actual_source_path"].as_str().unwrap());
    assert!(
        relative_source
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
    );
    let source = workspace.join(relative_source);
    assert!(workspace.is_absolute() && output.is_absolute() && config_path.is_absolute());
    assert!(
        !output
            .components()
            .any(|part| part == std::path::Component::ParentDir)
    );
    assert!(
        output.starts_with(workspace.join("scratch/b2-fresh-lineage-20261008/probes"))
            || output.starts_with(workspace.join("scratch/b2-bpm-regions-20261008/native-probes"))
            || (corrected_legacy
                && output
                    .starts_with(workspace.join("scratch/b2-corrected-legacy-20261008/probes")))
    );
    assert!(
        !output.exists(),
        "fresh output directory must not already exist"
    );
    assert!(
        source
            .canonicalize()
            .unwrap()
            .starts_with(workspace.join("test-audio").canonicalize().unwrap())
    );
    assert_eq!(full_hash(&source), config["source_sha256"]);
    assert_eq!(
        fs::metadata(&source).unwrap().len(),
        config["source_bytes"].as_u64().unwrap()
    );
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("compiled-producer.rs"),
        include_str!("b2_native_candidate_probe.rs"),
    )
    .unwrap();
    let python_producer = if corrected_legacy {
        include_str!("b2_corrected_legacy_probe.py")
    } else {
        include_str!("b2_native_candidate_probe.py")
    };
    fs::write(output.join("compiled-producer.py"), python_producer).unwrap();
    let executable = std::env::current_exe().unwrap();
    let executable_copy = output.join("native-test-executable.exe");
    fs::copy(&executable, &executable_copy).unwrap();
    write_json(
        &output.join("native-runtime.json"),
        &json!({
            "native_test_executable":{"path":executable,"retained_path":executable_copy,
                "sha256":full_hash(&executable_copy),"bytes":fs::metadata(&executable_copy).unwrap().len()},
            "build_profile":if cfg!(debug_assertions) {"debug"} else {"release"},
            "installed_pyd":"not_used_by_embedded_native_harness",
            "repository":repository,
            "hardware_started":false
        }),
    );
    // Standalone cargo tests embed base Python, whose startup does not execute
    // the editable virtualenv's .pth hook. Verify and inject only this repo's
    // source modules before importing the actual service/worker dependencies.
    // Import first so missing Python setup fails before any expensive cold load.
    std::env::set_current_dir(&repository).unwrap();
    Python::initialize();
    let probe_module = Python::attach(|py| {
        let source_modules = repository.join("src");
        assert!(
            source_modules
                .canonicalize()
                .unwrap()
                .starts_with(repository.canonicalize().unwrap())
        );
        let python_path = py
            .import("sys")
            .unwrap()
            .getattr("path")
            .unwrap()
            .cast_into::<PyList>()
            .unwrap();
        python_path
            .insert(0, source_modules.to_string_lossy().as_ref())
            .unwrap();
        PyModule::from_code(
            py,
            &CString::new(python_producer).unwrap(),
            c"b2_native_candidate_probe.py",
            c"b2_native_candidate_probe",
        )
        .unwrap()
        .unbind()
    });
    write_json(
        &output.join("python-import-preflight.json"),
        &json!({
            "source_modules":repository.join("src"),
            "working_directory":repository,
            "producer_module_imported":true,
            "cold_source_work_started":false,
            "hardware_started":false
        }),
    );
    let engine = Python::attach(|py| Py::new(py, AudioEngine::new().unwrap()).unwrap());
    let rate = 96_000;
    let hint = ResidentLoadHint {
        start_s: 42.0,
        end_s: 42.5,
        key_lock: false,
    };
    let (request, mut consumer) = Python::attach(|py| {
        selected_load(
            &engine.borrow(py),
            &source,
            &output.join("samples"),
            rate,
            hint,
        )
    });
    let mut callback = Python::attach(|py| Callback::new(&engine.borrow(py), rate));
    // Test-only collection deadline accommodates unoptimized Windows cold loading
    // of the full private corpus; production and acceptance budgets are unchanged.
    let deadline = Instant::now() + Duration::from_secs(900);
    let event = loop {
        callback.drain(&mut consumer);
        let event = Python::attach(|py| engine.borrow(py).poll_loader_events(py).unwrap());
        if let Some(event) = event {
            let json: String = Python::attach(|py| {
                py.import("json")
                    .unwrap()
                    .call_method1("dumps", (event,))
                    .unwrap()
                    .extract()
                    .unwrap()
            });
            let event: Value = serde_json::from_str(&json).unwrap();
            if event["type"] == "error" {
                panic!("actual native source load failed: {event}");
            }
            if event["type"] == "success" {
                assert_eq!(event["request_id"].as_u64(), Some(request));
                break event;
            }
        }
        assert!(Instant::now() < deadline, "native cold source ACK deadline");
        std::thread::sleep(Duration::from_millis(1));
    };
    write_json(&output.join("native-loaded-event.json"), &event);
    wait_until(Duration::from_secs(30), || {
        Python::attach(|py| engine.borrow(py).cold_loading[0].load(Ordering::Acquire) == 0)
    });
    let proof = Python::attach(|py| native_source_proof(&engine.borrow(py), &output, rate));
    assert_eq!(
        proof["complete_mono_sha256"],
        config["expected_mono_sha256"]
    );
    assert_eq!(
        proof["complete_loaded_pcm"]["full_frames"],
        config["expected_full_frames"]
    );
    let probe = Python::attach(|py| {
        let bridge = Py::new(
            py,
            NativeBridge {
                engine: engine.clone_ref(py),
            },
        )
        .unwrap();
        probe_module
            .bind(py)
            .getattr("Probe")
            .unwrap()
            .call1((bridge, config_path.to_string_lossy().as_ref()))
            .unwrap()
            .unbind()
    });
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        let done: bool =
            Python::attach(|py| probe.call_method0(py, "poll").unwrap().extract(py).unwrap());
        if done {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "actual native worker/service retirement deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    Python::attach(|py| {
        probe.call_method0(py, "finish").unwrap();
    });
    wait_until(Duration::from_secs(30), || {
        !Python::attach(|py| engine.borrow(py).offline_jobs.has_pad(0))
    });
    assert!(output.join("jobs").read_dir().unwrap().next().is_none());
    Python::attach(|py| {
        engine.borrow_mut(py).shut_down().unwrap();
    });
}
