//! Actual216 canonical sources -> public warning/batch -> same2/32 pool -> Config -> own ACKs.
//! Inference is explicitly declared nonneural output. No child-process/RSS/music acceptance.
use super::*;
use pyo3::PyResult;
use std::collections::BTreeSet;
use std::io::Write;
use std::os::windows::fs::MetadataExt;

#[path = "batch_neural_tests.rs"]
mod batch_neural_tests;

#[pyclass]
struct Batch216SourceShape {
    engine: Py<AudioEngine>,
}

#[pymethods]
impl Batch216SourceShape {
    /// Read the same loaded cache without requiring a CPAL stream_handle.
    fn loaded_sample_shape(&self, py: Python<'_>, id: usize) -> PyResult<(u32, usize, usize)> {
        assert!(id < NUM_SAMPLES);
        let engine = self.engine.borrow(py);
        assert!(engine.loaded_source_digest(id)?.is_some());
        let cache = engine.sample_cache.lock().unwrap();
        let sample = cache[id].as_ref().unwrap();
        let generations = engine.loaded_source_generations.lock().unwrap();
        let (generation, rate) = generations[id];
        assert!(generation > 0 && rate > 0);
        Ok((rate, sample.channels, sample.frame_count()))
    }
}

struct ReleaseProducerCheckpoint(Py<PyAny>);

impl Drop for ReleaseProducerCheckpoint {
    fn drop(&mut self) {
        Python::attach(|py| {
            self.0.call_method0(py, "release").unwrap();
        });
    }
}

fn owned_directory(distinct: bool) -> tempfile::TempDir {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let parent = workspace.join("test-artifacts/b1-stem-batch-216-20261020");
    for ancestor in parent.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => assert_eq!(metadata.file_attributes() & 0x400, 0),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("owned216 fixture ancestor unavailable: {error}"),
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
        .prefix(if distinct { "distinct-" } else { "duplicate-" })
        .tempdir_in(parent)
        .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(owned.path().join("OWNER.md"))
        .unwrap()
        .write_all(b"Owned actual216 canonical-source/batch/Config/Native software fixture. Synthetic PCM and explicitly declared nonneural inference output; no Human data or installation. The original owners release before this disposable directory.")
        .unwrap();
    owned
}

fn source_wav(index: usize) -> Vec<u8> {
    let mut encoded = wav(&[0, 1, 2, 3]);
    // Change actual decoded PCM, never merely a name/header/path. The first
    // stereo channel's authored initial value has ample i16 headroom.
    let value = i16::from_le_bytes(encoded[44..46].try_into().unwrap());
    let changed = value
        .checked_add(i16::try_from(index * 4).unwrap())
        .unwrap();
    encoded[44..46].copy_from_slice(&changed.to_le_bytes());
    encoded
}

fn load_sources(
    engine: &AudioEngine,
    callback: &mut Callback,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    root: &Path,
    distinct: bool,
) -> Vec<Value> {
    let mut rows = vec![Value::Null; NUM_SAMPLES];
    for material_index in 0..if distinct { NUM_SAMPLES } else { 1 } {
        let source = root.join(format!("input-{material_index}.wav"));
        fs::write(&source, source_wav(material_index)).unwrap();
        let material =
            prepare_material_complete_for_test(root, &source, RATE, 2, &|| false).unwrap();
        let metadata = material.metadata();
        let preparation = prepared_for_test(engine, material, root).unwrap();
        let ids: Vec<_> = if distinct {
            vec![material_index]
        } else {
            (0..NUM_SAMPLES).collect()
        };
        for id in ids {
            let source_ticket = adopt_for_format(
                engine,
                id,
                &preparation,
                producer.clone(),
                (2, RATE, root.to_path_buf()),
            )
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while engine.cold_loading[id].load(Ordering::Acquire) != 0 {
                callback.drain();
                assert!(
                    Instant::now() < deadline,
                    "source{id} never received its own cold ACK"
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
                        if request_id == source_ticket.request_id() =>
                    {
                        success = true
                    }
                    LoaderEvent::Error { error, .. } => panic!("source{id} failed: {error:?}"),
                    _ => {}
                }
            }
            assert!(success && source_ticket.is_current().unwrap());
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
    let original_digests: BTreeSet<_> = (0..NUM_SAMPLES)
        .map(|id| engine.loaded_source_digest(id).unwrap().unwrap())
        .collect();
    let transformed_digests: BTreeSet<_> = (0..NUM_SAMPLES)
        .map(|id| engine.waveform_source_identity(id).unwrap().unwrap().1)
        .collect();
    let expected = if distinct { NUM_SAMPLES } else { 1 };
    assert_eq!(original_digests.len(), expected);
    assert_eq!(transformed_digests.len(), expected);
    assert!(original_digests.iter().all(|digest| digest.len() == 64));
    assert!(transformed_digests.iter().all(|digest| digest.len() == 64));
    rows
}

fn batch_probe(engine: &Py<AudioEngine>, rows: &[Value]) -> Py<PyAny> {
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
                "/../../../src/tests/flitzis_looper/controller/stem_batch_216_native_probe.py"
            )))
            .unwrap(),
            c"stem_batch_216_native_probe.py",
            c"stem_batch_216_native_probe",
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
            .getattr("StemBatch216Probe")
            .unwrap()
            .call1((
                engine.clone_ref(py),
                shape,
                serde_json::to_string(rows).unwrap(),
            ))
            .unwrap()
            .unbind()
    })
}

fn wait_batch(callback: &mut Callback, probe: &Py<PyAny>, condition: &str, request_all: bool) {
    // Initial120s host estimate expired while making real progress (distinct:
    // 109 inferences/77 saved; duplicate trace:35 saved). Keep every completion
    // and own-ACK oracle, finite total and semantic stall bounds. No production
    // performer, process or worker deadline changes here.
    let began = Instant::now();
    let deadline = began + Duration::from_secs(600);
    let mut next_observation = began;
    let mut next_heartbeat = began + Duration::from_secs(30);
    let mut last_progress = began;
    let mut last_marker = None;
    loop {
        callback.drain();
        Python::attach(|py| {
            probe.call_method0(py, "poll").unwrap();
            if request_all {
                probe.call_method0(py, "adopt_all").unwrap();
            }
        });
        if Python::attach(|py| {
            probe
                .call_method0(py, condition)
                .unwrap()
                .extract::<bool>(py)
                .unwrap()
        }) {
            return;
        }
        let now = Instant::now();
        if now >= next_observation {
            let marker = Python::attach(|py| {
                probe
                    .call_method0(py, "progress_marker")
                    .unwrap()
                    .extract::<(usize, usize, usize, usize, usize)>(py)
                    .unwrap()
            });
            if last_marker != Some(marker) {
                last_marker = Some(marker);
                last_progress = now;
            }
            assert!(
                now.duration_since(last_progress) < Duration::from_secs(60),
                "{condition}: no semantic progress for60s: {}",
                Python::attach(|py| {
                    probe
                        .call_method0(py, "diagnostics")
                        .unwrap()
                        .extract::<String>(py)
                        .unwrap()
                })
            );
            next_observation = now + Duration::from_secs(1);
            if now >= next_heartbeat {
                println!(
                    "actual216_progress phase={condition} elapsed_s={} marker={marker:?}",
                    began.elapsed().as_secs_f64()
                );
                next_heartbeat = now + Duration::from_secs(30);
            }
        }
        assert!(
            Instant::now() < deadline,
            "{condition}: {}",
            Python::attach(|py| {
                probe
                    .call_method0(py, "diagnostics")
                    .unwrap()
                    .extract::<String>(py)
                    .unwrap()
            })
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn profile(distinct: bool) {
    Python::initialize();
    let started = Instant::now();
    let process_before = crate::audio_engine::c3_process_metrics::snapshot();
    let owned = owned_directory(distinct);
    let previous = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(owned.path()).unwrap();
    let root = owned.path().join("samples");
    fs::create_dir(&root).unwrap();
    let (engine, mut callback, producer) = empty_engine();
    engine
        .project_assets
        .set_resident_budget_mib(&engine.resident_account, 128)
        .unwrap();
    let rows = load_sources(&engine, &mut callback, &producer, &root, distinct);
    assert!(!callback.mixer.voices.iter().any(|voice| voice.active));
    let engine = Python::attach(|py| Py::new(py, engine).unwrap());
    let probe = batch_probe(&engine, &rows);
    let release = ReleaseProducerCheckpoint(Python::attach(|py| probe.clone_ref(py)));
    call(&probe, "start");
    wait_batch(&mut callback, &probe, "full_pool_ready", false);
    call(&probe, "release");
    wait_batch(&mut callback, &probe, "complete", false);
    wait_batch(&mut callback, &probe, "all_ready", true);
    callback.drain();
    for id in 0..NUM_SAMPLES {
        assert!(callback.mixer.bank_for_measurement()[id].is_some());
        assert!(callback.mixer.stems_for_measurement()[id].is_some());
        assert_eq!(
            callback.mixer.stem_demand_for_measurement(id).0,
            StemMixMode::AllStems
        );
    }
    assert!(!callback.mixer.voices.iter().any(|voice| voice.active));
    assert!(callback.controls.is_empty());
    let (mut evidence, project_assets, native_ownership) = Python::attach(|py| {
        let native = engine.borrow(py);
        let status = native
            .project_assets
            .resident_budget_status(&native.resident_account)
            .unwrap();
        assert_eq!(status.budget_mib, 128);
        assert!(!status.over_budget);
        assert!(status.held_unique_pcm_bytes > 0);
        assert_eq!(status.pending_reservation_count, 0);
        assert_eq!(status.pending_resident_bytes, 0);
        assert!(status.admitted_total_bytes <= status.budget_bytes);
        let report = probe
            .call_method0(py, "finish")
            .unwrap()
            .extract::<String>(py)
            .unwrap();
        let evidence = json!({
            "controller": serde_json::from_str::<Value>(&report).unwrap(),
            "native_pcm_budget": {
                "budget_mib": status.budget_mib,
                "budget_bytes": status.budget_bytes,
                "held_unique_pcm_bytes": status.held_unique_pcm_bytes,
                "admitted_total_bytes": status.admitted_total_bytes,
                "pending_reservation_count": status.pending_reservation_count,
                "pending_resident_bytes": status.pending_resident_bytes,
            },
            "process_before": process_before,
            "process_after": crate::audio_engine::c3_process_metrics::snapshot(),
            "scope": "isolated actual216 declared-output software profile; Windows process counters include Python/Native and retained PCM, no neural child trees or music/device acceptance",
        });
        (
            evidence,
            native.project_assets.clone(),
            Arc::downgrade(&native.input_runtime_ownership),
        )
    });
    call(&probe, "close");
    callback.drain();
    // Production controller callbacks carry ordinary Python cycles. Collect
    // after shutdown, while the actual accepted bank and engine remain alive.
    // Release attached Python references explicitly: detached Py<T> Drop only
    // schedules decref for a later attachment and is not an owner-drop boundary.
    let controller_gc_collected = Python::attach(|py| {
        drop(release);
        probe.drop_ref(py);
        py.import("gc")
            .unwrap()
            .call_method0("collect")
            .unwrap()
            .extract::<usize>()
            .unwrap()
    });
    let process_controller_dropped_bank_alive = crate::audio_engine::c3_process_metrics::snapshot();
    // The fixture's existing Callback owns its retirement worker. Its ordinary
    // Drop joins that worker after the actual mixer/bank and queued payloads drop.
    drop(callback);
    drop(producer);
    let native_gc_collected = Python::attach(|py| {
        engine.drop_ref(py);
        py.import("gc")
            .unwrap()
            .call_method0("collect")
            .unwrap()
            .extract::<usize>()
            .unwrap()
    });
    assert!(
        native_ownership.upgrade().is_none(),
        "actual216 teardown still retains an original Native authority owner"
    );
    // Use the existing service's bounded synchronous collection pass to release
    // dead reader metadata. Retain its real status, without claiming all shared
    // background services or file retirements have reached an idle baseline.
    project_assets.collect_for_test();
    let (retiring, readers, deleted, errors) = project_assets.status().unwrap();
    drop(project_assets);
    let process_native_owners_dropped = crate::audio_engine::c3_process_metrics::snapshot();
    evidence["process_phases"] = json!({
        "before_profile": evidence["process_before"],
        "before_close": evidence["process_after"],
        "controller_dropped_bank_alive": process_controller_dropped_bank_alive,
        "native_owners_dropped_after_asset_collect": process_native_owners_dropped,
        "controller_release_delta": crate::audio_engine::c3_process_metrics::delta(
            &evidence["process_after"], &process_controller_dropped_bank_alive),
        "native_release_delta": crate::audio_engine::c3_process_metrics::delta(
            &process_controller_dropped_bank_alive, &process_native_owners_dropped),
        "whole_profile_delta": crate::audio_engine::c3_process_metrics::delta(
            &evidence["process_before"], &process_native_owners_dropped),
        "controller_gc_collected": controller_gc_collected,
        "native_gc_collected": native_gc_collected,
        "native_ownership_released": true,
        "callback_retirement_worker_joined": true,
        "asset_service_after_one_existing_collect_pass": {
            "retiring": retiring,
            "readers": readers,
            "deleted": deleted,
            "errors": errors,
        },
        "scope": "same PID/creation identity at existing owner-release boundaries; ordinary Python runtime and shared services remain loaded; one existing asset collection pass, no handle-baseline or leak inference",
    });
    println!(
        "actual216_batch_software distinct={distinct} elapsed_s={} {evidence}",
        started.elapsed().as_secs_f64()
    );
    drop(previous);
    drop(owned);
}

#[test]
fn actual_216_duplicate_batch_commits_once_per_pad_and_accepts_own_native_receipts() {
    isolated(
        "bank_public_tests::controller_native_tests::batch_216_tests::actual_216_duplicate_batch_commits_once_per_pad_and_accepts_own_native_receipts",
        || profile(false),
    );
}

#[test]
fn actual_216_distinct_batch_keeps_bounded_pool_and_each_own_native_receipt() {
    isolated(
        "bank_public_tests::controller_native_tests::batch_216_tests::actual_216_distinct_batch_keeps_bounded_pool_and_each_own_native_receipt",
        || profile(true),
    );
}
