// Productive Loader selection, Copy, actual Config and separate-process restore.

use super::*;
use crate::messages::ControlParameterMessage;
use pyo3::types::PyModule;
use std::ffi::CString;

const PRODUCER: &str = "FLITZI_SOURCE_MAP_VERTICAL_PRODUCER";
const RESTORE: &str = "FLITZI_SOURCE_MAP_VERTICAL_RESTORE";

struct WorkingDirectory(PathBuf);
impl Drop for WorkingDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

fn probe(engine: &Py<AudioEngine>, input: Option<String>) -> Py<PyAny> {
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
            &CString::new(include_str!("source_beatmap_selection_probe.py")).unwrap(),
            c"source_beatmap_selection_probe.py",
            c"source_beatmap_selection_probe",
        )
        .unwrap();
        let class = module.getattr("Probe").unwrap();
        match input {
            Some(input) => class.call1((engine.clone_ref(py), input)).unwrap().unbind(),
            None => class.call1((engine.clone_ref(py),)).unwrap().unbind(),
        }
    })
}

fn drain(
    consumer: &mut rtrb::Consumer<ControlMessage>,
    parameters: &mut rtrb::Consumer<ControlParameterMessage>,
    mixer: &mut RtMixer,
) {
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    let mut quantization = TriggerQuantization::Immediate;
    let mut transport = TransportTimeline::new(48_000);
    crate::audio_engine::audio_stream::drain_parameter_messages(parameters, mixer, &mut transport);
    while let Ok(message) = consumer.pop() {
        process_control_message(
            message,
            &mut scheduler,
            0,
            &mut quantization,
            &mut transport,
            mixer,
            &mut Vec::<AudioMessage>::new(),
            &mut ImmediateAudioBufferRetirement,
        );
    }
}

fn wait_probe(
    probe: &Py<PyAny>,
    method: &str,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    parameters: &mut rtrb::Consumer<ControlParameterMessage>,
    mixer: &mut RtMixer,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        drain(consumer, parameters, mixer);
        let complete = Python::attach(|py| {
            probe
                .call_method0(py, method)
                .unwrap()
                .extract::<bool>(py)
                .unwrap()
        });
        if complete {
            return;
        }
        if Instant::now() >= deadline {
            let diagnostic = Python::attach(|py| {
                let state: String = probe
                    .call_method0(py, "diagnostics")
                    .unwrap()
                    .extract(py)
                    .unwrap();
                let object = probe.bind(py).getattr("audio").unwrap();
                let audio = object.extract::<PyRef<'_, AudioEngine>>().unwrap();
                format!(
                    "python={state}; native coldpool={:?} offline36={} coldloading36={} generation36={:?} resident36={:?}",
                    audio.cold_jobs.counts_for_test(),
                    audio.offline_jobs.has_pad(36),
                    audio.cold_loading[36].load(Ordering::Acquire),
                    audio.loaded_source_generations.lock().unwrap()[36],
                    audio.sample_cache.lock().unwrap()[36]
                        .as_ref()
                        .map(|sample| (
                            sample.frame_count(),
                            sample.resident_start(),
                            sample.resident_end()
                        ))
                )
            });
            panic!("productive map wait {method} timed out: {diagnostic}");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn child_name(name: &str) -> String {
    format!("{}::{name}", module_path!().split_once("::").unwrap().1)
}

#[test]
fn fresh_config_selected_copy_child() {
    let Some(root) = std::env::var_os(RESTORE) else {
        return;
    };
    let root = PathBuf::from(root);
    let _cwd = WorkingDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(&root).unwrap();
    Python::initialize();
    let mut engine = AudioEngine::new().unwrap();
    assert!(engine.stream_handle.is_none());
    let (producer, mut consumer) = RingBuffer::new(64);
    let producer = Arc::new(Mutex::new(producer));
    let (_, feedback) = RingBuffer::<AudioMessage>::new(64);
    engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(feedback))));
    let (parameters, mut parameter_consumer) = RingBuffer::new(64);
    engine.test_parameter_producer = Some(Arc::new(Mutex::new(parameters)));
    engine.test_output_format = Some((48_000, 2));
    let mut mixer = Box::new(RtMixer::new(2, 48_000.0));
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    mixer.set_prepared_source_epochs(engine.prepared_source_epochs.clone());
    assert!(
        engine
            .sample_cache
            .lock()
            .unwrap()
            .iter()
            .all(Option::is_none)
    );
    assert!(
        engine
            .cold_leases
            .lock()
            .unwrap()
            .iter()
            .all(Option::is_none)
    );
    let engine = Python::attach(|py| Py::new(py, engine).unwrap());
    let probe = probe(&engine, None);
    Python::attach(|py| probe.call_method0(py, "restore").unwrap());
    wait_probe(
        &probe,
        "restored_copy",
        &mut consumer,
        &mut parameter_consumer,
        &mut mixer,
    );
    drain(&mut consumer, &mut parameter_consumer, &mut mixer);
    let actual: String = Python::attach(|py| {
        probe
            .call_method0(py, "restored_record")
            .unwrap()
            .extract(py)
            .unwrap()
    });
    let expected: Value =
        serde_json::from_slice(&fs::read(root.join("expected-map.json")).unwrap()).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&actual).unwrap(), expected);
    Python::attach(|py| {
        let engine = engine.borrow(py);
        assert!(engine.sample_cache.lock().unwrap()[0].is_none());
        assert!(engine.sample_cache.lock().unwrap()[36].is_some());
        assert!(
            engine.cold_leases.lock().unwrap()[36].is_some(),
            "actual fresh cold ACK, not receipt-only projection"
        );
        assert_eq!(engine.pad_timing_intent(36).unwrap(), "tap");
    });
    Python::attach(|py| probe.call_method0(py, "close").unwrap());
    Python::attach(|py| engine.borrow_mut(py).shut_down().unwrap());
    drop(probe);
    drop(engine);
    Python::attach(|py| {
        py.import("gc").unwrap().call_method0("collect").unwrap();
    });
    fs::write(root.join("fresh-config-map-restored.txt"),
        b"Actual separate-process Loader cold ACK restored selected copied content from Config with TAP and sample_analysis=None.\n").unwrap();
    eprintln!("fresh Config source-map child PID={}", std::process::id());
}

#[test]
fn productive_map_select_copy_origin_unload_config_and_fresh_process_restore() {
    let name =
        child_name("productive_map_select_copy_origin_unload_config_and_fresh_process_restore");
    if std::env::var_os(PRODUCER).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--nocapture", "--test-threads=1"])
            .env(PRODUCER, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("productive selected-map vertical PID=")
        );
        return;
    }
    let fixture = ColdCustody::new_selected(None);
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let original = original_path(&fixture, 0);
    let root = fixture.directory.path().to_owned();
    fs::write(root.join("OWNER.md"), b"Owned synthetic source-map selection/Copy/Config/fresh-process test. No Human originals, App or device. Root owns execution and cleanup.\n").unwrap();
    let samples = samples_root(&fixture);
    let material_id = fixture.engine.cold_leases.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .material_id
        .clone();
    let birth =
        crate::audio_engine::cold_store::material_creation_receipt(&samples, &original).unwrap();
    let input = json!({"reference":format!("samples/{}",original.strip_prefix(&samples).unwrap().to_string_lossy().replace('\\',"/")),
        "material_id":material_id,"material_creation":birth}).to_string();
    fs::write(root.join("expected-map.json"), expected.to_string()).unwrap();
    let _cwd = WorkingDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(&root).unwrap();
    let ColdCustody {
        mut engine,
        directory,
        producer,
        mut consumer,
        mut mixer,
    } = fixture;
    let (parameters, mut parameter_consumer) = RingBuffer::new(64);
    engine.test_parameter_producer = Some(Arc::new(Mutex::new(parameters)));
    engine.test_output_format = Some((48_000, 2));
    let engine = Python::attach(|py| Py::new(py, engine).unwrap());
    let probe = probe(&engine, Some(input));
    drain(&mut consumer, &mut parameter_consumer, &mut mixer);
    Python::attach(|py| {
        probe
            .call_method1(py, "select", (expected.to_string(),))
            .unwrap()
    });
    wait_probe(
        &probe,
        "selected",
        &mut consumer,
        &mut parameter_consumer,
        &mut mixer,
    );
    Python::attach(|py| probe.call_method0(py, "copy").unwrap());
    wait_probe(
        &probe,
        "copied",
        &mut consumer,
        &mut parameter_consumer,
        &mut mixer,
    );
    Python::attach(|py| {
        probe.call_method0(py, "unload_origin").unwrap();
        // The source-custody fixture initially loads a second independent source
        // reader. Remove it too, so only the productive copied content retains it.
        engine.borrow_mut(py).unload_sample(1).unwrap();
    });
    drain(&mut consumer, &mut parameter_consumer, &mut mixer);
    for _ in 0..4 {
        Python::attach(|py| engine.borrow(py).project_assets.collect_for_test());
    }
    let retained: String = Python::attach(|py| {
        probe
            .call_method0(py, "retained_after_collection")
            .unwrap()
            .extract(py)
            .unwrap()
    });
    assert_eq!(serde_json::from_str::<Value>(&retained).unwrap(), expected);
    let config = root.join("samples/selected-map-vertical.json");
    let before = fs::read(&config).unwrap();
    Python::attach(|py| probe.call_method0(py, "close").unwrap());
    Python::attach(|py| engine.borrow_mut(py).shut_down().unwrap());
    drop(probe);
    drop(engine);
    drop(mixer);
    drop(consumer);
    drop(producer);
    Python::attach(|py| {
        py.import("gc").unwrap().call_method0("collect").unwrap();
    });
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &child_name("fresh_config_selected_copy_child"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env(RESTORE, &root)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "fresh stdout={} stderr={}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(root.join("fresh-config-map-restored.txt").is_file());
    assert_eq!(
        fs::read(&config).unwrap(),
        before,
        "fresh restore preserves exact saved compact Config bytes"
    );
    eprintln!(
        "productive selected-map vertical PID={}; fresh child completed after all producer engine/probe owners dropped",
        std::process::id()
    );
    drop(_cwd);
    drop(directory);
}
