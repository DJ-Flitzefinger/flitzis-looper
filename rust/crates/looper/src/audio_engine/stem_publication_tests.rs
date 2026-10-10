//! Real offline worker, immutable cache, native publication/ACK and render regression.
//! Explicit root/producer replaces device startup; separation writes synthetic WAVs.
//! The initial injected task runner executes real worker bodies inline; restored
//! controllers use the ordinary pool. Source, Pair and callback kernels are genuine.

use super::*;
use crate::audio_engine::resident_relocation::{ResidentWindowTicket, WindowRequest};
use crate::audio_engine::{PreparedSourceTicket, PreparedStemPair};
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
const SAVED_ROOT_MARKER: &str = "FLITZI_P1B_SAVED_RESTORE_ROOT";

#[path = "stem_mouse_trigger_tests.rs"]
mod mouse_triggers;

#[pyclass]
struct NativeBridge {
    #[pyo3(get)]
    engine: Py<AudioEngine>,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    root: PathBuf,
}

#[pymethods]
impl NativeBridge {
    #[pyo3(signature = (id, source_version, cache_dir, source_ticket, components, descriptor_reference=None))]
    fn prepare_stem_pair(
        &self,
        py: Python<'_>,
        id: usize,
        source_version: String,
        cache_dir: String,
        source_ticket: &PreparedSourceTicket,
        components: bool,
        descriptor_reference: Option<String>,
    ) -> PyResult<PreparedStemPair> {
        let held = self.engine.borrow(py);
        let engine: &AudioEngine = &held;
        py.detach(|| {
            engine.prepare_stem_pair_at_root(
                &self.root,
                id,
                &source_version,
                &cache_dir,
                source_ticket,
                components,
                descriptor_reference.as_deref(),
            )
        })
        .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    fn publish_stem_pair(
        &self,
        py: Python<'_>,
        prepared: &PreparedStemPair,
        source_ticket: &PreparedSourceTicket,
    ) -> PyResult<()> {
        self.engine.borrow(py).publish_stem_pair_with_producer(
            prepared,
            source_ticket,
            &self.producer,
        )
    }

    #[pyo3(signature = (id, mode, source_version=None))]
    fn set_stem_mix_mode(
        &self,
        id: usize,
        mode: &str,
        source_version: Option<String>,
    ) -> PyResult<()> {
        let (mode, source_version_hash) = match mode {
            "full_mix" => (StemMixMode::FullMix, 0),
            "all_stems" => (
                StemMixMode::AllStems,
                super::super::stem_cache::source_version_hash(
                    source_version.as_deref().ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("missing source version")
                    })?,
                ),
            ),
            _ => return Err(pyo3::exceptions::PyValueError::new_err("invalid stem mode")),
        };
        self.producer
            .lock()
            .unwrap()
            .push(ControlMessage::SetStemMixMode {
                id,
                mode,
                source_version_hash,
            })
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("stem mode queue full"))
    }

    fn set_stem_enabled_mask(
        &self,
        id: usize,
        enabled_stem_mask: u8,
        source_version: String,
    ) -> PyResult<()> {
        self.producer
            .lock()
            .unwrap()
            .push(ControlMessage::SetStemEnabledMask {
                id,
                enabled_stem_mask,
                source_version_hash: super::super::stem_cache::source_version_hash(&source_version),
            })
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("stem mask queue full"))
    }

    fn set_stem_pair_full_mix(&self, py: Python<'_>, id: usize) -> PyResult<()> {
        super::super::resident_relocation::set_stem_pair_full_mix_with_producer(
            &self.engine.borrow(py),
            id,
            &self.producer,
        )
    }

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
    run_publication_and_mouse_proof(TEST_NAME, false, false);
}

#[test]
fn accepted_stem_mouse_start_uses_native_ack_and_render_under_cold_pressure() {
    run_publication_and_mouse_proof(
        "audio_engine::cold_residency_tests::stem_publication_tests::accepted_stem_mouse_start_uses_native_ack_and_render_under_cold_pressure",
        true,
        false,
    );
}

#[test]
fn shared_material_publish_reuses_pcm_with_own_native_subscriber_ack() {
    run_publication_and_mouse_proof(
        "audio_engine::cold_residency_tests::stem_publication_tests::shared_material_publish_reuses_pcm_with_own_native_subscriber_ack",
        true,
        true,
    );
}

fn run_publication_and_mouse_proof(test_name: &str, accepted_only: bool, shared: bool) {
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
    if shared && let Ok(root) = std::env::var(SAVED_ROOT_MARKER) {
        restore_saved_material(Path::new(&root));
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
                root: root.clone(),
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
    // The current ALL preference publishes its real mode and component mask only
    // after the independently accepted Pair ticket; keep later queue oracles exact.
    assert_eq!(callback.drain(&mut consumer), 2);
    if shared {
        prove_shared_material(
            &engine,
            &probe,
            &version,
            &root,
            &producer,
            &mut consumer,
            &mut callback,
        );
        call(&probe, "save_survivor");
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
            .env(CHILD_MARKER, test_name)
            .env(SAVED_ROOT_MARKER, directory.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "saved #216 fresh-process restore failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        Python::attach(|py| {
            probe
                .getattr(py, "controller")
                .unwrap()
                .call_method0(py, "shut_down")
                .unwrap();
            probe
                .getattr(py, "controller")
                .unwrap()
                .getattr(py, "_assets")
                .unwrap()
                .call_method0(py, "release_saved_assignments")
                .unwrap();
            engine.borrow_mut(py).shut_down().unwrap();
        });
        return;
    }
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
    assert_eq!(callback.drain(&mut consumer), 2);
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

fn restore_saved_material(directory: &Path) {
    std::env::set_current_dir(directory).unwrap();
    let root = directory.join("samples");
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    Python::initialize();
    let engine = Python::attach(|py| Py::new(py, AudioEngine::new().unwrap()).unwrap());
    let (producer, mut consumer) = rtrb::RingBuffer::new(16);
    let producer = Arc::new(Mutex::new(producer));
    let mut callback = Python::attach(|py| Callback::new(&engine.borrow(py), RATE));
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
                root: root.clone(),
            },
        )
        .unwrap();
        module
            .getattr("SavedMaterialProbe")
            .unwrap()
            .call1((bridge,))
            .unwrap()
            .unbind()
    });
    let source = Python::attach(|py| {
        probe
            .getattr(py, "project")
            .unwrap()
            .getattr(py, "sample_paths")
            .unwrap()
            .bind(py)
            .get_item(215)
            .unwrap()
            .extract::<String>()
            .unwrap()
    });
    load_slot(
        &engine,
        215,
        source,
        &root,
        &producer,
        &mut consumer,
        &mut callback,
    );
    call(&probe, "begin");
    assert_eq!(callback.drain(&mut consumer), 1);
    call(&probe, "accepted");
    assert_eq!(callback.drain(&mut consumer), 2);
    Python::attach(|py| {
        probe
            .getattr(py, "controller")
            .unwrap()
            .call_method0(py, "shut_down")
            .unwrap();
        probe
            .getattr(py, "controller")
            .unwrap()
            .getattr(py, "_assets")
            .unwrap()
            .call_method0(py, "release_saved_assignments")
            .unwrap();
        engine.borrow_mut(py).shut_down().unwrap();
    });
}

fn load_slot(
    engine: &Py<AudioEngine>,
    slot: usize,
    source: String,
    root: &Path,
    producer: &Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    callback: &mut Callback,
) {
    let request = Python::attach(|py| {
        admit_for_format_selected(
            &engine.borrow(py),
            slot,
            source,
            (false, false, false, None),
            producer.clone(),
            (2, RATE, root.to_path_buf()),
        )
        .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        callback.drain(consumer);
        let event = Python::attach(|py| engine.borrow(py).loader_rx.lock().unwrap().try_recv());
        match event {
            Ok(LoaderEvent::Success { request_id, .. }) if request_id == request => break,
            Ok(LoaderEvent::Error { error, .. }) => {
                panic!("saved subscriber source load failed: {error}")
            }
            _ => {}
        }
        assert!(Instant::now() < deadline, "saved subscriber timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn prove_shared_material(
    engine: &Py<AudioEngine>,
    probe: &Py<PyAny>,
    version: &str,
    root: &Path,
    producer: &Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    callback: &mut Callback,
) {
    let (source, generation, descriptor_reference): (String, String, String) =
        Python::attach(|py| {
            (
                probe
                    .getattr(py, "project")
                    .unwrap()
                    .getattr(py, "sample_paths")
                    .unwrap()
                    .bind(py)
                    .get_item(0)
                    .unwrap()
                    .extract()
                    .unwrap(),
                probe
                    .getattr(py, "saved_path")
                    .unwrap()
                    .extract(py)
                    .unwrap(),
                probe
                    .getattr(py, "project")
                    .unwrap()
                    .getattr(py, "stem_cache")
                    .unwrap()
                    .bind(py)
                    .get_item(0)
                    .unwrap()
                    .getattr("pair")
                    .unwrap()
                    .getattr("descriptor_reference")
                    .unwrap()
                    .extract()
                    .unwrap(),
            )
        });
    load_slot(engine, 215, source, root, producer, consumer, callback);
    let ticket = Python::attach(|py| {
        engine
            .borrow(py)
            .capture_prepared_source(215, version.to_owned())
            .unwrap()
    });
    let existing = Python::attach(|py| {
        let engine = engine.borrow(py);
        let reference = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
        let (_, path) =
            super::super::project_assets::owned_path(root, Path::new(&generation)).unwrap();
        engine
            .project_assets
            .shared_stems(&path, &reference, version, RATE)
            .unwrap()
            .unwrap()
    });
    // This subscriber names the legacy original, while the complete canonical
    // pair marker names its verified canonical copy. The ordinary Pair opener
    // keeps that disk lineage separate from this subscriber's own runtime ticket.
    let pair = Python::attach(|py| {
        let held = engine.borrow(py);
        let native: &AudioEngine = &held;
        py.detach(|| {
            native.prepare_stem_pair_at_root(
                root,
                215,
                version,
                &generation,
                &ticket,
                true,
                Some(&descriptor_reference),
            )
        })
        .unwrap()
    });
    assert!(pair.has_components());
    let selection: serde_json::Value = serde_json::from_str(pair.selection_json()).unwrap();
    assert_eq!(selection["descriptor_reference"], descriptor_reference);
    assert_eq!(selection["wav_generation"], generation);
    assert_eq!(ticket.publication_status(), "captured");
    // Background preparation legitimately registers the fully sealed Pair.
    // Queue admission failures must add no further subscriber/reader booking.
    let readers = Python::attach(|py| engine.borrow(py).project_assets.status().unwrap().1);
    {
        let mut producer = producer.lock().unwrap();
        while !producer.is_full() {
            producer.push(ControlMessage::Ping()).unwrap();
        }
    }
    for _ in 0..32 {
        Python::attach(|py| {
            let engine = engine.borrow(py);
            let rejected = engine
                .capture_prepared_source(215, version.to_owned())
                .unwrap();
            assert!(
                engine
                    .publish_stem_pair_with_producer(&pair, &rejected, producer)
                    .is_err()
            );
            assert_eq!(rejected.publication_status(), "captured");
            assert_eq!(
                engine.project_assets.status().unwrap().1,
                readers,
                "failed shared subscriber left a reader booking"
            );
        });
    }
    while consumer.pop().is_ok() {}
    Python::attach(|py| {
        engine
            .borrow(py)
            .publish_stem_pair_with_producer(&pair, &ticket, producer)
            .unwrap()
    });
    assert_eq!(ticket.publication_status(), "pending");
    let message = consumer.pop().unwrap();
    let ControlMessage::PublishPreparedStems { id, stems } = &message else {
        panic!("expected publication");
    };
    assert_eq!(*id, 215);
    for index in 0..crate::messages::STEM_BUFFER_COUNT {
        assert!(
            Arc::ptr_eq(&existing.stems[index].samples, &stems.stems[index].samples),
            "subscriber allocated duplicate aligned PCM"
        );
    }
    assert!(Arc::ptr_eq(
        &existing.complete_set_identity,
        &stems.complete_set_identity
    ));
    producer.lock().unwrap().push(message).unwrap();
    assert_eq!(callback.drain(consumer), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    {
        let mut producer = producer.lock().unwrap();
        producer
            .push(ControlMessage::SetStemMixMode {
                id: 215,
                mode: StemMixMode::AllStems,
                source_version_hash: super::super::stem_cache::source_version_hash(version),
            })
            .unwrap();
        producer
            .push(ControlMessage::SetStemEnabledMask {
                id: 215,
                enabled_stem_mask: 8,
                source_version_hash: super::super::stem_cache::source_version_hash(version),
            })
            .unwrap();
    }
    assert_eq!(callback.drain(consumer), 2);
    // An independently invalidated origin cannot revoke the accepted other slot.
    Python::attach(|py| {
        let engine = engine.borrow(py);
        super::super::next_pad_request_id(
            &engine.pad_request_ids,
            0,
            &engine.prepared_source_epochs[0],
        )
        .unwrap();
    });
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(
        callback
            .mixer
            .play_sample_rt(215, 1.0, &mut callback.retirement)
    );
    let mut output = [0.0; 64];
    callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        0,
        &mut callback.retirement,
    );
    assert!(
        output
            .iter()
            .all(|value| (*value - 5120.0 / 32767.0).abs() < 1e-6),
        "second pad independent stem mask did not render its shared data"
    );
    callback.mixer.stop_sample_rt(215, &mut callback.retirement);
}
