//! One actual stock model on a stopped target beside six complete-source PADs.

use super::controller_overlap::{active, script_value, settle, start};
use super::six_pad_conversion::{sources, stop, voices};
use super::*;

type ChildSample = (u64, u64, u64, u64, u64);

struct CommandRuntime {
    original_executable: String,
    original_path: Py<PyAny>,
    command: PathBuf,
    overlay: PathBuf,
    restored: bool,
}

impl CommandRuntime {
    fn new() -> Self {
        let command = PathBuf::from(
            std::env::var_os("FLITZIS_INFERENCE_PYTHON")
                .expect("explicit checked command interpreter required"),
        );
        let overlay = PathBuf::from(
            std::env::var_os("FLITZIS_VENDOR_OVERLAY").expect("checked vendor overlay required"),
        );
        assert!(command.is_absolute() && command.is_file());
        assert!(overlay.is_absolute() && overlay.is_dir());
        let command = command.canonicalize().unwrap();
        assert_eq!(command.file_name().unwrap(), "python.exe");
        assert_ne!(
            command,
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
        let (original_executable, original_path) = Python::attach(|py| {
            let sys = py.import("sys").unwrap();
            let loaded: Vec<String> = sys
                .getattr("modules")
                .unwrap()
                .call_method0("keys")
                .unwrap()
                .try_iter()
                .unwrap()
                .map(|name| name.unwrap().extract().unwrap())
                .collect();
            assert!(
                !loaded
                    .iter()
                    .any(|name| name.starts_with("flitzis_bs_roformer"))
            );
            let original = sys.getattr("executable").unwrap().extract().unwrap();
            let path = sys.getattr("path").unwrap();
            let saved = path.call_method0("copy").unwrap().unbind();
            // libtest is not a Python command interpreter. Configure the same
            // checked interpreter as ordinary production before producer capture.
            sys.setattr("executable", command.to_string_lossy().as_ref())
                .unwrap();
            // The Rust runner prepends virtualenv site-packages. Restore the
            // already checked small overlay's parent/child import precedence.
            path.call_method1("insert", (0, overlay.to_string_lossy().as_ref()))
                .unwrap();
            (original, saved)
        });
        Self {
            original_executable,
            original_path,
            command,
            overlay,
            restored: false,
        }
    }

    fn restore(&mut self) {
        if !self.restored {
            Python::attach(|py| {
                let sys = py.import("sys").unwrap();
                sys.setattr("executable", &self.original_executable)
                    .unwrap();
                sys.setattr("path", self.original_path.bind(py)).unwrap();
            });
            self.restored = true;
        }
    }
}

impl Drop for CommandRuntime {
    fn drop(&mut self) {
        self.restore();
    }
}

fn install_current_native() {
    Python::initialize();
    Python::attach(|py| {
        let modules = py.import("sys").unwrap().getattr("modules").unwrap();
        let names: Vec<String> = modules
            .call_method0("keys")
            .unwrap()
            .try_iter()
            .unwrap()
            .map(|name| name.unwrap().extract().unwrap())
            .collect();
        assert!(
            !names.iter().any(|name| name.starts_with("flitzis_looper")),
            "the explicit profile requires a fresh process before any App/Native import"
        );
        let module = crate::flitzis_looper_audio::embedded_test_module(py).unwrap();
        modules.set_item("flitzis_looper_audio", &module).unwrap();
        modules
            .set_item("flitzis_looper_audio.flitzis_looper_audio", &module)
            .unwrap();
        assert!(
            module
                .getattr("AudioEngine")
                .unwrap()
                .is(py.get_type::<AudioEngine>())
        );
        assert!(
            module
                .getattr("OfflineWorkTicket")
                .unwrap()
                .is(py.get_type::<crate::audio_engine::heavy_work::OfflineWorkTicket>())
        );
        assert!(module.getattr("OfflineChildMemoryReservation").unwrap().is(
            py.get_type::<crate::audio_engine::stem_conversion::OfflineChildMemoryReservation>()
        ));
        assert!(
            module
                .getattr("StemConversionReservation")
                .unwrap()
                .is(py
                    .get_type::<crate::audio_engine::stem_conversion::StemConversionReservation>())
        );
        assert!(
            module
                .getattr("PreparedWindowsProcess")
                .unwrap()
                .is(py.get_type::<crate::offline_process::PreparedWindowsProcess>())
        );
    });
}

fn verify_original_native_identity(f: &Fixture) {
    f.script("from flitzis_looper.controller.stem_provenance import native_conversion_identity\nassert native_conversion_identity() is not None\nimport flitzis_looper_audio as actual_native\nassert type(probe._audio) is actual_native.AudioEngine\nassert actual_native.stem_conversion_identity.__module__ == actual_native.StemConversionReservation.__module__ == 'flitzis_looper_audio.flitzis_looper_audio'");
}

#[test]
#[ignore = "fresh embedded Native identity and shared-ledger preflight; no model/device"]
fn actual_stock_inference_native_identity_preflight() {
    install_current_native();
    let directory = tempfile::tempdir().unwrap();
    let mut f = Fixture::new(directory.path(), false);
    verify_original_native_identity(&f);
    let baseline = crate::audio_engine::resident_host_memory::pending_bytes().unwrap();
    f.script("import flitzis_looper_audio as actual_native\nprobe.native_budget = actual_native.OfflineChildMemoryReservation()\nassert probe.native_budget.limit_bytes > 0");
    assert!(crate::audio_engine::resident_host_memory::pending_bytes().unwrap() > baseline);
    f.script("probe.native_budget.close()\ndel probe.native_budget");
    assert_eq!(
        crate::audio_engine::resident_host_memory::pending_bytes().unwrap(),
        baseline
    );
    Python::attach(|py| f.engine.borrow_mut(py).shut_down().unwrap());
    f.close_callback();
    eprintln!(
        "stock_inference_native_identity_preflight shared-current-EXE-ledger verified; no model/device"
    );
}

struct InferenceSession(Py<PyAny>);

#[pyclass]
struct InferenceSourceShape {
    engine: Py<AudioEngine>,
}

#[pymethods]
impl InferenceSourceShape {
    /// Read this same engine's actual loaded geometry without a physical stream.
    fn loaded_sample_shape(&self, py: Python<'_>, id: usize) -> (u32, usize, usize) {
        assert!(id < NUM_SAMPLES);
        let engine = self.engine.borrow(py);
        let cache = engine.sample_cache.lock().unwrap();
        let sample = cache[id].as_ref().unwrap();
        let generations = engine.loaded_source_generations.lock().unwrap();
        let (generation, rate) = generations[id];
        assert!(generation > 0 && rate > 0);
        (rate, sample.channels, sample.frame_count())
    }
}

impl Drop for InferenceSession {
    fn drop(&mut self) {
        Python::attach(|py| {
            if let Err(error) = self.0.call_method0(py, "close") {
                eprintln!("stock inference cleanup failed: {error:?}");
            }
        });
    }
}

fn sample(session: &InferenceSession) -> ChildSample {
    Python::attach(|py| {
        session
            .0
            .call_method0(py, "sample")
            .unwrap()
            .extract(py)
            .unwrap()
    })
}

#[test]
#[ignore = "explicit stock CPU inference beside six paced PADs; no device"]
fn actual_stock_bs_cpu_inference_with_six_pads() {
    run(true);
}

#[test]
#[ignore = "explicit separately admitted available-CUDA stock inference; no device"]
fn actual_stock_bs_available_cuda_inference_with_six_pads() {
    run(false);
}

#[test]
#[ignore = "explicit stock BS CUDA/CPU learned rows and genuine allocator fault; no device"]
fn actual_stock_bs_productive_provider_qualification() {
    install_current_native();
    let mut runtime = CommandRuntime::new();
    let root = PathBuf::from(
        std::env::var_os("FLITZIS_INFERENCE_PROFILE_ROOT").expect("fresh owned root required"),
    );
    assert!(root.is_absolute() && !root.exists());
    fs::create_dir(&root).unwrap();
    let (module, shared) = Python::attach(|py| {
        let name = "tests.flitzis_looper.controller.provider_qualification_probe";
        assert!(
            !py.import("sys")
                .unwrap()
                .getattr("modules")
                .unwrap()
                .contains(name)
                .unwrap()
        );
        let module = py.import(name).unwrap();
        let actual: PathBuf = module.getattr("__file__").unwrap().extract().unwrap();
        let expected = PathBuf::from(std::env::var_os("FLITZIS_PROVIDER_HELPER_SOURCE").unwrap());
        assert_eq!(
            actual.canonicalize().unwrap(),
            expected.canonicalize().unwrap()
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(&actual).unwrap())),
            std::env::var("FLITZIS_PROVIDER_HELPER_SHA256").unwrap()
        );
        let shared = module
            .getattr("ProviderQualificationState")
            .unwrap()
            .call0()
            .unwrap()
            .unbind();
        (module.unbind(), shared)
    });
    for mode in ["preflight", "cuda", "cpu", "fault"] {
        productive_fixture(&root.join(mode), &module, &shared, mode);
    }
    let report: String = Python::attach(|py| {
        shared
            .call_method0(py, "finish")
            .unwrap()
            .extract(py)
            .unwrap()
    });
    Python::attach(|_| {
        drop(shared);
        drop(module);
    });
    runtime.restore();
    fs::write(root.join("provider-qualification.provisional.json"), report).unwrap();
    eprintln!("stock_bs_productive_provider_qualification completed all three original runs");
}

fn productive_fixture(root: &Path, module: &Py<PyModule>, shared: &Py<PyAny>, mode: &str) {
    fs::create_dir(root).unwrap();
    let mut f = Fixture::new(root, false);
    verify_original_native_identity(&f);
    let path = root.join("source-6.wav");
    f.script(&format!(
        "from tests.flitzis_looper.controller.controller_overlap_probe import write_source\nwrite_source({:?}, {RATE}, 3, 1606)\nprobe.load(6, {:?})",
        path.to_string_lossy(), path.to_string_lossy()
    ));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !script_value(&f, "probe.poll_load(6)") {
        f.drain();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    f.drain();
    let original_sources = sources(&mut f, &[6]);
    let session = InferenceSession(Python::attach(|py| {
        let options = PyDict::new(py);
        options
            .set_item(
                "shape_audio",
                Py::new(
                    py,
                    InferenceSourceShape {
                        engine: f.engine.clone_ref(py),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        options.set_item("shared", shared.bind(py)).unwrap();
        options.set_item("mode", mode).unwrap();
        module
            .bind(py)
            .getattr("ProviderQualification")
            .unwrap()
            .call((f.probe.bind(py),), Some(&options))
            .unwrap()
            .unbind()
    }));
    if mode != "preflight" {
        Python::attach(|py| session.0.call_method1(py, "start", (mode,)).unwrap());
        let began = Instant::now();
        loop {
            f.drain();
            f.render(512);
            f.call("poll_feedback");
            let complete = Python::attach(|py| {
                session
                    .0
                    .call_method0(py, "poll")
                    .unwrap()
                    .extract::<bool>(py)
                    .unwrap()
            });
            assert!(
                began.elapsed() < Duration::from_secs(600),
                "bounded {mode} qualification"
            );
            if complete {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
    assert_eq!(sources(&mut f, &[6]), original_sources);
    assert!(!active(&mut f, 6));
    let mut files: Vec<(String, String)> = Python::attach(|py| {
        let files = session
            .0
            .call_method0(
                py,
                if mode == "preflight" {
                    "preflight_files"
                } else {
                    "finish_run"
                },
            )
            .unwrap()
            .extract(py)
            .unwrap();
        session.0.call_method0(py, "close").unwrap();
        files
    });
    files.push((path.to_string_lossy().into_owned(), files[0].1.clone()));
    let closing = Instant::now();
    while !f.ready_to_close() {
        f.drain();
        f.call("poll_feedback");
        assert!(closing.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(1));
    }
    f.call("finish_close");
    f.script("assert probe.audio_output._operation is None\nassert not probe.audio_output.available\nassert not probe._runtime_activated");
    f.drain();
    let counts = Python::attach(|py| f.engine.borrow(py).cold_jobs.counts_for_test());
    assert_eq!(counts, (0, 0, 0));
    f.close_callback();
    Python::attach(|_| {
        drop(session);
        drop(f);
    });
    Python::attach(|py| {
        shared
            .call_method0(py, "assert_fixture_owners_ended")
            .unwrap();
    });
    let registry = productive_asset_registry(root);
    // The original registry intentionally keeps persisted file-only claims.
    // Verify bytes here; require exclusive access only after this exact EXE
    // and its enclosing Job end, without retiring those saved assignments.
    rehash_productive_files(&files);
    Python::attach(|py| {
        shared
            .call_method1(py, "fixture_ended", (mode, registry))
            .unwrap();
    });
}

fn productive_asset_registry(root: &Path) -> String {
    let began = Instant::now();
    let mut observations = Vec::new();
    loop {
        let registry = match crate::audio_engine::project_assets::ProjectAssets::shared()
            .observe_registry_for_test()
        {
            Ok(value) => serde_json::to_value(value).unwrap(),
            Err(error) => serde_json::json!({"error": error}),
        };
        let settled = [
            "native_live",
            "pending_without_native",
            "other_without_native_or_pending",
            "pending_assignments",
            "live_engine_witnesses",
            "live_owner_witnesses",
            "live_history_witnesses",
            "reserved_reader_slots",
            "retirements",
        ]
        .iter()
        .all(|key| registry[*key].as_u64() == Some(0))
            && [
                "externally_held_accounts",
                "live_owner_witnesses",
                "pending_reservations",
            ]
            .iter()
            .all(|key| registry["residency"][*key].as_u64() == Some(0))
            && registry["reader_records"].as_u64().is_some_and(|count| {
                count > 0
                    && ["saved_cold_file_only", "saved_claims", "cold_descriptors"]
                        .iter()
                        .all(|key| registry[*key].as_u64() == Some(count))
            });
        observations.push(serde_json::json!({
            "elapsed_ns": began.elapsed().as_nanos(), "registry": registry
        }));
        let trace = serde_json::json!({"settled": settled, "observations": &observations});
        let report = serde_json::to_string(&trace).unwrap();
        fs::write(root.join("asset-registry-after-fixture.json"), &report).unwrap();
        if settled {
            return report;
        }
        assert!(began.elapsed() < Duration::from_secs(2), "{report}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn rehash_productive_files(files: &[(String, String)]) {
    for (path, digest) in files {
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(path).unwrap())),
            *digest
        );
    }
}

fn run(cpu_only: bool) {
    install_current_native();
    let mut runtime = CommandRuntime::new();
    let pads = [0, 1, 2, 3, 4, 5];
    // Keep the original files for an exclusive parent reader after this exact
    // EXE and its owning WindowsJob end; a TempDir would erase that boundary.
    let root = PathBuf::from(
        std::env::var_os("FLITZIS_INFERENCE_PROFILE_ROOT").expect("fresh owned root required"),
    );
    assert!(root.is_absolute() && !root.exists());
    fs::create_dir(&root).unwrap();
    let mut f = Fixture::new(&root, false);
    verify_original_native_identity(&f);
    f.script("probe.conversion_pad_count = 6");
    let source_module = Python::attach(|py| {
        PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/controller_overlap_probe.py"
            )))
            .unwrap(),
            c"controller_overlap_probe.py",
            c"controller_overlap_probe",
        )
        .unwrap()
        .unbind()
    });
    for id in 0..7 {
        let path = root.join(format!("source-{id}.wav"));
        Python::attach(|py| {
            source_module
                .call_method1(
                    py,
                    "write_source",
                    (path.to_string_lossy().as_ref(), RATE, 3, 1000 + id * 101),
                )
                .unwrap();
        });
        f.script(&format!("probe.load({id}, {:?})", path.to_string_lossy()));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !script_value(&f, &format!("probe.poll_load({id})")) {
            f.drain();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
    }
    f.drain();
    let all = [0, 1, 2, 3, 4, 5, 6];
    let original_sources = sources(&mut f, &all);
    let markers = all.map(|id| f.state().mixer.loop_region_frames(id));
    f.script("probe.inference_sources = tuple((probe._audio.waveform_source_identity(i), probe._audio.loaded_source_digest(i), probe.source_history[i][-1]) for i in range(7))\nprobe.input_mapping.on_frame_render()\nprobe.select_pad(5)");
    let mut starts = Vec::new();
    for id in pads {
        let capture = start(&mut f, id, true);
        settle(&mut f, &pads[..=id]);
        assert_eq!(capture.state(), "applied");
        assert_eq!(capture.execution_frame(), capture.effective_frame());
        assert!(capture.original_frame().is_some());
        starts.push(capture);
    }
    let session = InferenceSession(Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/inference_overlap_probe.py"
            )))
            .unwrap(),
            c"inference_overlap_probe.py",
            c"inference_overlap_probe",
        )
        .unwrap();
        let options = PyDict::new(py);
        options.set_item("cpu_only", cpu_only).unwrap();
        options
            .set_item(
                "shape_audio",
                Py::new(
                    py,
                    InferenceSourceShape {
                        engine: f.engine.clone_ref(py),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        module
            .getattr("InferenceOverlap")
            .unwrap()
            .call((f.probe.bind(py),), Some(&options))
            .unwrap()
            .unbind()
    }));
    Python::attach(|py| session.0.call_method0(py, "start").unwrap());
    let began = Instant::now();
    let mut contributions = [0_u64; 6];
    let mut periods = 0;
    loop {
        assert!(
            began.elapsed() < Duration::from_secs(600),
            "bounded inference profile expired"
        );
        let target = Duration::from_secs_f64(periods as f64 * 512.0 / f64::from(RATE));
        if target > began.elapsed() {
            thread::sleep(target - began.elapsed());
        }
        let before = sample(&session);
        let first = f.state().transport.output_frame();
        let mut expected = voices(&mut f, &pads);
        for voice in &mut expected {
            assert!(voice.3.advance(512));
        }
        f.drain();
        let pcm = f.render(512);
        let after = sample(&session);
        let actual = voices(&mut f, &pads);
        for id in pads {
            assert_eq!(
                (actual[id].0, actual[id].1, actual[id].2),
                (expected[id].0, expected[id].1, expected[id].2)
            );
            assert!(actual[id].3.matches_exact(&expected[id].3));
            let peak = f.state().pad_peaks[id];
            assert!(
                peak.is_finite() && peak > 1e-6,
                "PAD{id} actual post-DSP contribution"
            );
            contributions[id] += 1;
        }
        assert!(!active(&mut f, 6));
        assert!(pcm.iter().all(|v| v.is_finite()) && pcm.iter().any(|v| v.abs() > 1e-6));
        f.call("poll_feedback");
        f.script("probe.transport.on_frame_render()\nprobe.input_mapping.on_frame_render()\nassert probe.session.active_sample_ids == set(range(6))\nassert not probe.session.paused_sample_ids");
        let complete = Python::attach(|py| {
            session
                .0
                .call_method1(py, "period", (first, before.1, after.0, before, after))
                .unwrap();
            session
                .0
                .call_method0(py, "poll")
                .unwrap()
                .extract::<bool>(py)
                .unwrap()
        });
        periods += 1;
        if complete {
            break;
        }
    }
    assert_eq!(contributions, [periods; 6]);
    assert_eq!(sources(&mut f, &all), original_sources);
    assert_eq!(
        all.map(|id| f.state().mixer.loop_region_frames(id)),
        markers
    );
    f.script("assert tuple((probe._audio.waveform_source_identity(i), probe._audio.loaded_source_digest(i), probe.source_history[i][-1]) for i in range(7)) == probe.inference_sources\nassert all(probe._audio.current_input_runtime_pad_binding(i).is_current() for i in range(7))\nassert probe.project.selected_pad == 5\nassert probe._audio.input_selected_projection()[1] == 5");
    stop(&mut f, &pads);
    let report: String = Python::attach(|py| {
        session
            .0
            .call_method0(py, "finish")
            .unwrap()
            .extract(py)
            .unwrap()
    });
    eprintln!(
        "stock_inference_overlap {{\"contributions\":{contributions:?},\"report\":{report}}}"
    );
    Python::attach(|py| session.0.call_method0(py, "close").unwrap());
    let counts = Python::attach(|py| {
        let mut engine = f.engine.borrow_mut(py);
        engine.shut_down().unwrap();
        engine.cold_jobs.counts_for_test()
    });
    assert_eq!(counts, (0, 0, 0));
    f.close_callback();
    Python::attach(|_| {
        drop(starts);
        drop(session);
        drop(source_module);
        drop(f);
    });
    runtime.restore();
    fs::write(
        root.join("stock-inference.provisional.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "cpu_only": cpu_only,
            "contributions": contributions,
            "original_sources": original_sources,
            "original_loop_regions": markers,
            "report": serde_json::from_str::<serde_json::Value>(&report).unwrap(),
            "cold_jobs_after_shutdown": counts,
            "fixture_owners_dropped_under_gil": true,
            "command_runtime": {
                "original_embedded_executable": runtime.original_executable,
                "configured_interpreter": runtime.command,
                "checked_vendor_overlay": runtime.overlay,
                "restored_after_fixture": runtime.restored
            },
            "reader_end": "pending exact original EXE AND owning Job END, then parent exclusive original-file proof"
        }))
        .unwrap(),
    )
    .unwrap();
}
