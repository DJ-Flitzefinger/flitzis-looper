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
