//! Real offline worker, immutable cache, native publication/ACK and render regression.
//! Only CPAL startup and neural separation are replaced; the native kernel is shared.

use super::*;
use crate::audio_engine::PreparedSourceTicket;
use crate::audio_engine::resident_relocation::{ResidentWindowTicket, WindowRequest};
use crate::messages::StemMixMode;
use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyModule};
use std::ffi::CString;
use std::process::Command;

const RATE: u32 = 8_000;
const FRAMES: usize = 4_096;
// Stem WAV decoding normalizes positive PCM16 against its signed maximum.
// Keep the oracle independent of the native decoder's conversion helper.
const VOCALS_AMPLITUDE: f32 = 2_048.0 / 32_767.0;
const TEST_NAME: &str = "audio_engine::cold_residency_tests::stem_publication_tests::worker_generation_publishes_native_stems_and_restores_with_fresh_ticket";
const CHILD_MARKER: &str = "FLITZI_STEM_PUBLICATION_TEST_CHILD";

#[path = "stem_mouse_trigger_tests.rs"]
mod mouse_triggers;

#[pyclass]
struct NativeBridge {
    #[pyo3(get)]
    engine: Py<AudioEngine>,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
}

#[pymethods]
impl NativeBridge {
    fn publish_prepared_stems(
        &self,
        py: Python<'_>,
        id: usize,
        source_version: String,
        cache_dir: String,
        source_ticket: &PreparedSourceTicket,
    ) -> PyResult<()> {
        self.engine.borrow(py).publish_prepared_stems_with_producer(
            py,
            id,
            source_version,
            cache_dir,
            source_ticket,
            &self.producer,
        )
    }

    fn loaded_sample_shape(&self, py: Python<'_>, id: usize) -> PyResult<(u32, usize, usize)> {
        let engine = self.engine.borrow(py);
        let cache = engine.sample_cache.lock().unwrap();
        let sample = cache[id].as_ref().unwrap();
        Ok((RATE, sample.channels, sample.frame_count()))
    }

    #[pyo3(signature = (id, start_s=None, end_s=None, position_s=None, key_lock=None))]
    fn prepare_resident_control(
        &self,
        py: Python<'_>,
        id: usize,
        start_s: Option<f64>,
        end_s: Option<f64>,
        position_s: Option<f64>,
        key_lock: Option<bool>,
    ) -> PyResult<ResidentWindowTicket> {
        super::super::resident_relocation::prepare_window_with_producer(
            &self.engine.borrow(py),
            id,
            WindowRequest {
                loop_region: start_s.map(|start| (start, end_s)),
                seek_position_s: position_s,
                key_lock,
                ..WindowRequest::default()
            },
            self.producer.clone(),
        )
    }

    #[pyo3(signature = (ticket, exclusive=false, received_at_ns=None))]
    fn play_resident_control(
        &self,
        py: Python<'_>,
        ticket: &ResidentWindowTicket,
        exclusive: bool,
        received_at_ns: Option<u64>,
    ) -> PyResult<bool> {
        let engine = self.engine.borrow(py);
        let now = engine.capture_input_timestamp_ns();
        super::super::resident_relocation::launch_with_producer(
            &engine,
            ticket,
            exclusive,
            super::super::timing::validated_input_timestamp(received_at_ns, now).unwrap_or(now),
            &self.producer,
        )
    }

    fn cancel_pad_launches(&self, py: Python<'_>, id: usize) -> PyResult<bool> {
        super::super::input_runtime_binding::cancel_launches_with_producer(
            &self.engine.borrow(py).input_runtime_ownership,
            Some(&self.producer),
            Some(id),
        )
        .map(|targets| !targets.is_empty())
    }

    fn stop_sample(&self, py: Python<'_>, id: usize) -> PyResult<()> {
        super::super::input_runtime_binding::enqueue_stop_with_producer(
            &self.engine.borrow(py).input_runtime_ownership,
            &mut self.producer.lock().unwrap(),
            Some(id),
        )
        .then_some(())
        .ok_or_else(|| pyo3::exceptions::PyRuntimeError::new_err("stop queue is full"))
    }
}

fn call(probe: &Py<PyAny>, method: &str) {
    Python::attach(|py| {
        probe.call_method0(py, method).unwrap();
    });
}

fn render(callback: &mut Callback, expected: f32) {
    assert!(
        callback
            .mixer
            .play_sample_rt(0, 1.0, &mut callback.retirement)
    );
    let mut output = vec![0.0; 512 * 2];
    callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        0,
        &mut callback.retirement,
    );
    // Ignore the bounded source-selection ramp; all later stereo samples have an
    // independent constant PCM16 oracle, distinct from the source/full mix.
    for (index, sample) in output.iter().enumerate().skip(256 * 2) {
        assert_eq!(
            sample.to_bits(),
            expected.to_bits(),
            "render sample {index}"
        );
    }
    callback.mixer.stop_sample_rt(0, &mut callback.retirement);
}

#[test]
fn worker_generation_publishes_native_stems_and_restores_with_fresh_ticket() {
    run_publication_and_mouse_proof(TEST_NAME, false);
}

#[test]
fn accepted_stem_mouse_start_uses_native_ack_and_render_under_cold_pressure() {
    run_publication_and_mouse_proof(
        "audio_engine::cold_residency_tests::stem_publication_tests::accepted_stem_mouse_start_uses_native_ack_and_render_under_cold_pressure",
        true,
    );
}

fn run_publication_and_mouse_proof(test_name: &str, accepted_only: bool) {
    if std::env::var(CHILD_MARKER).as_deref() != Ok(test_name) {
        // Python controllers use project-relative paths. A fresh test process
        // isolates their cwd and embedded interpreter from concurrent Rust tests.
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
            .env(CHILD_MARKER, test_name)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "isolated stem publication regression failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        return;
    }
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    let directory = tempfile::Builder::new()
        .prefix("stem-publication-test-")
        .tempdir_in(repository.parent().unwrap().join("scratch"))
        .unwrap();
    struct CurrentDirectory(PathBuf);
    impl Drop for CurrentDirectory {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).unwrap();
        }
    }
    let _restore_directory = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(directory.path()).unwrap();
    let root = directory.path().join("samples");
    fs::create_dir(&root).unwrap();
    write_pcm16(&root.join("source.wav"), RATE, 2, &vec![8_192; FRAMES * 2]);
    Python::initialize();
    let engine = Python::attach(|py| Py::new(py, AudioEngine::new().unwrap()).unwrap());
    let (producer, mut consumer) = rtrb::RingBuffer::new(16);
    let producer = Arc::new(Mutex::new(producer));
    let mut callback = Python::attach(|py| Callback::new(&engine.borrow(py), RATE));
    let request = Python::attach(|py| {
        admit_for_format_selected(
            &engine.borrow(py),
            0,
            "samples/source.wav".into(),
            (false, false, false, None),
            producer.clone(),
            (2, RATE, root.clone()),
        )
        .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let cached_path = loop {
        callback.drain(&mut consumer);
        let event = Python::attach(|py| engine.borrow(py).loader_rx.lock().unwrap().try_recv());
        match event {
            Ok(LoaderEvent::Success {
                request_id,
                cached_path,
                ..
            }) if request_id == request => {
                break cached_path;
            }
            Ok(LoaderEvent::Error { error, .. }) => panic!("source load failed: {error}"),
            _ => {}
        }
        assert!(Instant::now() < deadline, "actual source load timed out");
        std::thread::sleep(Duration::from_millis(1));
    };
    let probe = Python::attach(|py| {
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
            &CString::new(include_str!("stem_publication_probe.py")).unwrap(),
            c"stem_publication_probe.py",
            c"stem_publication_probe",
        )
        .unwrap();
        let bridge = Py::new(
            py,
            NativeBridge {
                engine: engine.clone_ref(py),
                producer: producer.clone(),
            },
        )
        .unwrap();
        module
            .getattr("Probe")
            .unwrap()
            .call1((bridge, cached_path, FRAMES, RATE))
            .unwrap()
            .unbind()
    });
    if !accepted_only {
        mouse_triggers::prove_ready_mouse_triggers(
            &engine,
            &probe,
            &mut callback,
            &producer,
            &mut consumer,
            None,
        );
    }
    let version: String = Python::attach(|py| {
        probe
            .call_method0(py, "begin")
            .unwrap()
            .extract(py)
            .unwrap()
    });
    assert_eq!(callback.drain(&mut consumer), 1);
    call(&probe, "accepted");
    mouse_triggers::prove_ready_mouse_triggers(
        &engine,
        &probe,
        &mut callback,
        &producer,
        &mut consumer,
        Some(&version),
    );
    let version_hash = super::super::stem_cache::source_version_hash(&version);
    {
        let mut producer = producer.lock().unwrap();
        producer
            .push(ControlMessage::SetStemMixMode {
                id: 0,
                mode: StemMixMode::AllStems,
                source_version_hash: version_hash,
            })
            .unwrap();
        producer
            .push(ControlMessage::SetStemEnabledMask {
                id: 0,
                enabled_stem_mask: 1,
                source_version_hash: version_hash,
            })
            .unwrap();
    }
    assert_eq!(callback.drain(&mut consumer), 2);
    render(&mut callback, VOCALS_AMPLITUDE);
    call(&probe, "restore");
    assert_eq!(callback.drain(&mut consumer), 1);
    call(&probe, "restored_accepted");
    render(&mut callback, VOCALS_AMPLITUDE);
    call(&probe, "reject_tampered_restore");
    assert_eq!(callback.drain(&mut consumer), 0);
    call(&probe, "begin_stale_restore");
    Python::attach(|py| {
        let engine = engine.borrow(py);
        super::super::next_pad_request_id(
            &engine.pad_request_ids,
            0,
            &engine.prepared_source_epochs[0],
        )
        .unwrap();
    });
    assert_eq!(callback.drain(&mut consumer), 1);
    call(&probe, "rejected");
    // Late rejection preserves the previously accepted native stem set.
    render(&mut callback, VOCALS_AMPLITUDE);
    Python::attach(|py| engine.borrow_mut(py).shut_down().unwrap());
}
