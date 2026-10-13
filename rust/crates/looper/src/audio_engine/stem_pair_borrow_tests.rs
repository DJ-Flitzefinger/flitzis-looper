//! Actual public PyO3 engine borrowing during an I/O-paused pair worker.
//! Source adoption and callback rendering use the device-free native harness;
//! preparation calls the public Python receiver, without an engine facade.
//! Only the exact child changes cwd. No stream or audio device is opened.

use super::*;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::native_owned_work::{WorkPoint, set_preparation_observer};
use pyo3::{Py, Python};
use std::sync::mpsc;

const BORROW_CHILD_ROOT: &str = "FLITZI_STEM_PAIR_BORROW_ROOT";
const BORROW_CHILD_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::stem_pair_borrow_tests::public_pair_worker_child_allows_receive_mutable_ping_and_pad1_render";

struct ObserverReset;
impl Drop for ObserverReset {
    fn drop(&mut self) {
        set_preparation_observer(None);
    }
}

// Also release the paused worker if an assertion in the UI oracle unwinds.
struct WorkerRelease(Option<mpsc::Sender<()>>);
impl WorkerRelease {
    fn release(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
impl Drop for WorkerRelease {
    fn drop(&mut self) {
        self.release();
    }
}

#[test]
fn public_pair_worker_releases_engine_borrow_before_heavy_io() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let scratch = workspace.join("scratch/goal-p3-stem-pair-20261010/borrow-runtime");
    fs::create_dir_all(&scratch).unwrap();
    let directory = tempfile::tempdir_in(scratch).unwrap();
    let output_path = directory.path().join("public-borrow-child.log");
    let output = fs::File::create(&output_path).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", BORROW_CHILD_TEST, "--nocapture"])
        .current_dir(directory.path())
        .env(BORROW_CHILD_ROOT, directory.path())
        .stdout(std::process::Stdio::from(output.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(output))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!(
                "owned public-borrow child timed out: {}",
                fs::read_to_string(output_path).unwrap()
            );
        }
        // Process completion polling is not the worker concurrency proof.
        std::thread::sleep(Duration::from_millis(1));
    };
    let output = fs::read_to_string(output_path).unwrap();
    assert!(
        status.success() && output.contains("1 passed"),
        "actual public-engine concurrency regression failed: {output}"
    );
    let receipt: Value = serde_json::from_slice(
        &fs::read(directory.path().join("public-borrow-verified.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["process_id"], child.id());
    assert_ne!(receipt["process_id"], std::process::id());
    for field in [
        "actual_public_receiver",
        "paused_after_pcm_write",
        "actual_receive_message",
        "actual_mutable_ping",
        "pad1_render_while_worker_paused",
        "pad2_own_callback_ack",
    ] {
        assert_eq!(receipt[field], true, "missing child assertion {field}");
    }
}

#[test]
fn public_native_background_borrow_prepared_stems_cold_and_legacy() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let scratch = workspace.join("scratch/goal-portable-native-borrow-20261012");
    fs::create_dir_all(&scratch).unwrap();
    for mode in ["cold", "legacy"] {
        let directory = tempfile::tempdir_in(&scratch).unwrap();
        let output_path = directory.path().join("public-stem-publication.log");
        let output = fs::File::create(&output_path).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", BORROW_CHILD_TEST, "--nocapture"])
            .current_dir(directory.path())
            .env(BORROW_CHILD_ROOT, directory.path())
            .env("FLITZI_STEM_PUBLICATION_BORROW_MODE", mode)
            .stdout(std::process::Stdio::from(output.try_clone().unwrap()))
            .stderr(std::process::Stdio::from(output))
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!(
                    "owned {mode} publication child timed out: {}",
                    fs::read_to_string(&output_path).unwrap()
                );
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let output = fs::read_to_string(output_path).unwrap();
        assert!(
            status.success() && output.contains("1 passed"),
            "public {mode} prepared-stems borrow proof failed: {output}"
        );
        let receipt: Value = serde_json::from_slice(
            &fs::read(directory.path().join("public-borrow-verified.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["process_id"], child.id());
        for field in [
            "actual_public_receiver",
            "actual_receive_message",
            "actual_mutable_ping",
            "pad1_render_while_worker_paused",
            "pad2_own_callback_ack",
            "public_prepared_stems",
            "actual_callback_pong_received",
        ] {
            assert_eq!(receipt[field], true, "missing {mode} assertion {field}");
        }
        assert_eq!(receipt["actual_legacy_read_seal"], mode == "legacy");
        assert_eq!(receipt["paused_after_pcm_write"], mode == "cold");
    }
}

#[test]
fn public_pair_worker_child_allows_receive_mutable_ping_and_pad1_render() {
    let Some(directory) = std::env::var_os(BORROW_CHILD_ROOT).map(PathBuf::from) else {
        return;
    };
    assert_eq!(std::env::current_dir().unwrap(), directory);
    let publication_mode = std::env::var("FLITZI_STEM_PUBLICATION_BORROW_MODE").ok();
    assert!(
        publication_mode
            .as_deref()
            .is_none_or(|mode| matches!(mode, "cold" | "legacy"))
    );
    Python::initialize();
    let root = directory.join("samples");
    fs::create_dir(&root).unwrap();
    let original = root.join("pad2.wav");
    wav(&original, RATE);
    let material = prepare_material(&root, &original, RATE, 2, &|| false).unwrap();
    let version = canonical_version(&material);
    let wav_reference = canonical_wav_reference(&material);
    write_complete_wavs(
        &directory.join(&wav_reference),
        &stereo_wav(material.sample.frame_count()),
        &version,
    );

    let mut engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    assert!(!Arc::ptr_eq(&previous.samples, &material.sample.samples));
    let mut callback = Callback::new(&engine, &previous);
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    if publication_mode.as_deref() == Some("legacy") {
        // The retained in-memory compatibility route has no cold lease. Install
        // its decoded source through the actual legacy callback command.
        let sample = material.sample.clone();
        engine.sample_cache.lock().unwrap()[1] = Some(sample.clone());
        engine.loaded_source_generations.lock().unwrap()[1] = (19, RATE);
        engine.loaded_source_digests.lock().unwrap()[1] = Some(
            material.metadata()["original"]["sha256"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
        engine.pad_request_ids.lock().unwrap()[1] = 19;
        engine
            .input_runtime_ownership
            .publish_source(1, &sample, RATE, 19);
        producer
            .lock()
            .unwrap()
            .push(ControlMessage::LoadSample { id: 1, sample })
            .unwrap();
        assert_eq!(callback.drain(&mut consumer), 1);
    } else {
        let preparation = control::prepared_for_test(
            &engine,
            PreparedMigrationMaterial::from_current(
                &root,
                material.sample.clone(),
                material.lease.clone(),
            )
            .unwrap(),
            &root,
        )
        .unwrap();
        let source = control::adopt_for_format(
            &engine,
            1,
            &preparation,
            producer.clone(),
            (2, RATE, root.clone()),
        )
        .unwrap();
        wait_until(|| consumer.peek().is_ok());
        assert_eq!(source.phase().unwrap(), "pending");
        assert_eq!(callback.drain(&mut consumer), 1);
        assert!(matches!(
            terminal(&engine, source.request_id()),
            LoaderEvent::Success { id: 1, .. }
        ));
        wait_until(|| engine.cold_loading[1].load(Ordering::Acquire) == 0);
        assert!(source.is_current().unwrap());
        callback.assert_old_voice(&previous);
        preparation.release_preparation().unwrap();
    }

    let (mut feedback_producer, feedback_consumer) = rtrb::RingBuffer::new(8);
    feedback_producer
        .push(AudioMessage::PadPeak { id: 0, peak: 0.25 })
        .unwrap();
    engine.test_control_channels =
        Some((producer.clone(), Arc::new(Mutex::new(feedback_consumer))));
    let (engine, ticket, worker_engine, worker_ticket) = Python::attach(|py| {
        let engine = Py::new(py, engine).unwrap();
        // Capture through the actual public receiver, not a fabricated permit.
        let ticket = engine
            .call_method1(py, "capture_prepared_source", (1, version.clone()))
            .unwrap()
            .extract::<Py<PreparedSourceTicket>>(py)
            .unwrap();
        assert_eq!(ticket.borrow(py).publication_status(), "captured");
        let worker_engine = engine.clone_ref(py);
        let worker_ticket = ticket.clone_ref(py);
        (engine, ticket, worker_engine, worker_ticket)
    });

    let (paused_sender, paused_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let mut release = WorkerRelease(Some(release_sender));
    let (result_sender, result_receiver) = mpsc::channel();
    let worker_mode = publication_mode.clone();
    let expected_point = if worker_mode.as_deref() == Some("legacy") {
        WorkPoint::LegacyStemFiles
    } else {
        WorkPoint::StemPcmFlush
    };
    let worker = std::thread::spawn(move || {
        let _reset = ObserverReset;
        let first = AtomicBool::new(true);
        set_preparation_observer(Some(Box::new(move |point| {
            if point == expected_point && first.swap(false, Ordering::AcqRel) {
                paused_sender.send(std::thread::current().id()).unwrap();
                release_receiver
                    .recv_timeout(Duration::from_secs(20))
                    .expect("UI oracle did not release the actual preparation worker");
            }
        })));
        let result = Python::attach(|py| {
            if worker_mode.is_some() {
                worker_engine
                    .call_method1(
                        py,
                        "publish_prepared_stems",
                        (1, version, wav_reference, worker_ticket),
                    )
                    .map(|_| None)
                    .map_err(|error| error.to_string())
            } else {
                worker_engine
                    .call_method1(
                        py,
                        "prepare_stem_pair",
                        (1, version, wav_reference, worker_ticket, true),
                    )
                    .and_then(|value| Ok(Some(value.extract::<Py<PreparedStemPair>>(py)?)))
                    .map_err(|error| error.to_string())
            }
        });
        result_sender.send(result).unwrap();
    });
    let pause_deadline = Instant::now() + Duration::from_secs(10);
    let paused_thread = paused_receiver
        .recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|pause_error| {
            match result_receiver
                .recv_timeout(pause_deadline.saturating_duration_since(Instant::now()))
            {
                Ok(Err(error)) => {
                    panic!("public preparation failed before {expected_point:?}: {error}")
                }
                Ok(Ok(_)) => {
                    panic!("public preparation returned without reaching {expected_point:?}")
                }
                Err(error) => panic!(
                    "public preparation never reached {expected_point:?}: pause {pause_error}; native result {error}"
                ),
            }
        });
    assert_ne!(paused_thread, std::thread::current().id());
    assert!(matches!(
        result_receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    Python::attach(|py| {
        let message = engine
            .call_method0(py, "receive_msg")
            .expect("UI receive_msg must be borrowable while the pair worker owns immutable pins");
        assert_eq!(
            message
                .call_method0(py, "sample_id")
                .unwrap()
                .extract::<Option<usize>>(py)
                .unwrap(),
            Some(0)
        );
        assert_eq!(
            message
                .call_method0(py, "pad_peak")
                .unwrap()
                .extract::<Option<f32>>(py)
                .unwrap(),
            Some(0.25)
        );
        assert!(engine.call_method0(py, "receive_msg").unwrap().is_none(py));
        engine
            .call_method0(py, "ping")
            .expect("the unchanged mutable public control receiver must also be borrowable");
        assert_eq!(ticket.borrow(py).publication_status(), "captured");
    });
    assert!(matches!(consumer.peek().unwrap(), ControlMessage::Ping()));
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(
        callback
            .feedback
            .events
            .iter()
            .any(|event| matches!(event, AudioMessage::Pong()))
    );
    if publication_mode.is_some() {
        let pong = callback
            .feedback
            .events
            .iter()
            .find(|event| matches!(event, AudioMessage::Pong()))
            .unwrap()
            .clone();
        feedback_producer.push(pong).unwrap();
        Python::attach(|py| {
            let message = engine.call_method0(py, "receive_msg").unwrap();
            assert!(matches!(
                &*message.extract::<PyRef<AudioMessage>>(py).unwrap(),
                AudioMessage::Pong()
            ));
            assert!(engine.call_method0(py, "receive_msg").unwrap().is_none(py));
        });
    }
    callback.assert_old_voice(&previous);
    let mut output = [0.0; 16];
    callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        0,
        &mut callback.retirement,
    );
    assert!(output.iter().all(|value| value.is_finite()));
    assert!(output.iter().any(|value| *value > 0.0));
    assert!(output.iter().all(|value| *value <= 0.25));
    if publication_mode.is_some() {
        assert_eq!(output, [0.25; 16]);
        let voice = callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap();
        assert_eq!(voice.source_playback.position().frame, 8);
        assert_eq!(voice.source_playback.position().fraction, 0.0);
    }
    callback.assert_old_voice(&previous);
    assert!(matches!(
        result_receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    release.release();
    let pair = result_receiver
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .expect("public pair worker failed after UI control and rendering");
    worker.join().unwrap();
    if let Some(pair) = pair {
        Python::attach(|py| {
            let engine = engine.borrow(py);
            let pair = pair.borrow(py);
            let ticket = ticket.borrow(py);
            assert!(pair.has_components());
            assert_complete_selection(&root, &selection(&pair));
            assert_eq!(ticket.publication_status(), "captured");
            engine
                .publish_stem_pair_with_producer(&pair, &ticket, &producer)
                .unwrap();
            assert_eq!(ticket.publication_status(), "pending");
        });
    } else {
        Python::attach(|py| assert_eq!(ticket.borrow(py).publication_status(), "pending"));
    }
    let stems = queued_stems(&mut consumer);
    assert_eq!(stems.stems.len(), 4);
    assert_eq!(callback.drain(&mut consumer), 1);
    Python::attach(|py| assert_eq!(ticket.borrow(py).publication_status(), "accepted"));
    callback.assert_old_voice(&previous);
    fs::write(
        directory.join("public-borrow-verified.json"),
        serde_json::to_vec(&json!({
            "process_id":std::process::id(), "actual_public_receiver":true,
            "paused_after_pcm_write":publication_mode.as_deref()!=Some("legacy"), "actual_receive_message":true,
            "actual_mutable_ping":true, "pad1_render_while_worker_paused":true,
            "pad2_own_callback_ack":true,
            "public_prepared_stems":publication_mode.is_some(),
            "actual_legacy_read_seal":publication_mode.as_deref()==Some("legacy"),
            "actual_callback_pong_received":publication_mode.is_some(),
        }))
        .unwrap(),
    )
    .unwrap();
}
