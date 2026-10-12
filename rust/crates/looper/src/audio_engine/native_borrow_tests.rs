//! Actual public Engine receivers during owned timing worker execution.

use super::*;
use crate::audio_engine::{
    audio_stream::drain_control_messages,
    buffer_retirement::ImmediateAudioBufferRetirement,
    cold_load::admit_for_format_selected,
    cold_residency::ResidentLoadHint,
    mixer::RtMixer,
    native_owned_work::{WorkPoint, set_observer},
    scheduler::FixedCapacityScheduler,
    transport::TransportTimeline,
};
use crate::messages::{AudioMessage, LoaderEvent, TriggerQuantization};
use pyo3::types::PyTuple;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const RATE: u32 = tests::RATE;

#[path = "material_borrow_tests.rs"]
mod material_borrow_tests;
#[path = "material_relocation_borrow_tests.rs"]
mod material_relocation_borrow_tests;

struct ObserverReset;
impl Drop for ObserverReset {
    fn drop(&mut self) {
        set_observer(None);
    }
}

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

pub(super) struct Callback {
    mixer: RtMixer,
    scheduler: FixedCapacityScheduler<8>,
    transport: TransportTimeline,
    quantization: TriggerQuantization,
    consumer: rtrb::Consumer<ControlMessage>,
    feedback: Producer<AudioMessage>,
    retirement: ImmediateAudioBufferRetirement,
    independent: SampleBuffer,
    consumed: usize,
}

impl Callback {
    fn drain(&mut self) -> usize {
        drain_control_messages(
            &mut self.consumer,
            &mut self.scheduler,
            self.consumed as u64,
            &mut self.quantization,
            &mut self.transport,
            &mut self.mixer,
            &mut self.feedback,
            &mut self.retirement,
        )
    }

    fn render(&mut self) {
        let mut output = [0.0; 17];
        self.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            self.consumed as u64,
            &mut self.retirement,
        );
        assert_eq!(output, [0.25; 17]);
        self.consumed += output.len();
        let voice = self
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(215))
            .unwrap();
        assert!(Arc::ptr_eq(
            &voice.sample.as_ref().unwrap().samples,
            &self.independent.samples
        ));
        let position = voice.source_playback.position();
        assert_eq!(
            position.frame,
            self.consumed % self.independent.frame_count()
        );
        assert_eq!(position.fraction, 0.0);
    }
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "actual source worker did not complete"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(super) fn engine() -> (tempfile::TempDir, Py<AudioEngine>, Callback, String, String) {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let directory = tempfile::tempdir_in(workspace.join("scratch")).unwrap();
    let (engine, callback, path, envelope) = engine_at(directory.path());
    (directory, engine, callback, path, envelope)
}

pub(super) fn engine_at(
    directory: &std::path::Path,
) -> (Py<AudioEngine>, Callback, String, String) {
    Python::initialize();
    let (original, envelope) = migration_test_fixture();
    let mut engine = AudioEngine::new().unwrap();
    let (producer, consumer) = tests::queue(16);
    let (feedback, feedback_consumer) = rtrb::RingBuffer::new(32);
    engine.test_control_channels =
        Some((producer.clone(), Arc::new(Mutex::new(feedback_consumer))));
    let request = admit_for_format_selected(
        &engine,
        0,
        original,
        (
            false,
            true,
            true,
            Some(ResidentLoadHint {
                start_s: 2.25,
                end_s: 3.25,
                key_lock: false,
            }),
        ),
        producer.clone(),
        (1, RATE, directory.join("samples")),
    )
    .unwrap();
    let independent_path = directory.join("independent.wav");
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&164_u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&128_u32.to_le_bytes());
    for _ in 0..64 {
        wav.extend_from_slice(&8192_i16.to_le_bytes());
    }
    std::fs::write(&independent_path, wav).unwrap();
    let independent_request = admit_for_format_selected(
        &engine,
        215,
        independent_path.to_string_lossy().into_owned(),
        (false, false, true, None),
        producer,
        (1, RATE, directory.join("samples")),
    )
    .unwrap();
    let mut mixer = RtMixer::new(1, RATE as f32);
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    wait_until(|| consumer.slots() == 2);
    let mut callback = Callback {
        mixer,
        scheduler: FixedCapacityScheduler::new(),
        transport: TransportTimeline::new(RATE),
        quantization: TriggerQuantization::Immediate,
        consumer,
        feedback,
        retirement: ImmediateAudioBufferRetirement,
        independent: SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from([0.25; 64]),
        },
        consumed: 0,
    };
    assert_eq!(callback.drain(), 2);
    let mut success = [false; 2];
    wait_until(|| {
        while let Ok(event) = engine.loader_rx.lock().unwrap().try_recv() {
            match event {
                LoaderEvent::Success {
                    id: 0, request_id, ..
                } if request_id == request => success[0] = true,
                LoaderEvent::Success {
                    id: 215,
                    request_id,
                    ..
                } if request_id == independent_request => success[1] = true,
                LoaderEvent::Error { error, .. } => panic!("actual source load failed: {error}"),
                _ => {}
            }
        }
        success == [true; 2]
            && engine.cold_loading[0].load(Ordering::Acquire) == 0
            && engine.cold_loading[215].load(Ordering::Acquire) == 0
    });
    let source = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    assert_eq!(
        (source.resident_start(), source.resident_end()),
        (18_000, 26_000)
    );
    assert_eq!(callback.mixer.loop_region_frames(0), (18_000, Some(26_000)));
    assert!(
        engine
            .input_runtime_ownership
            .source_generation(0, &source, RATE)
            .is_some()
    );
    let path = engine.cold_leases.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .original_path
        .to_string_lossy()
        .into_owned();
    callback.independent = engine.sample_cache.lock().unwrap()[215].clone().unwrap();
    assert_eq!(callback.independent.samples.as_ref(), &[0.25; 64]);
    assert_eq!(
        engine
            .input_runtime_ownership
            .source_generation(215, &callback.independent, RATE),
        Some(independent_request)
    );
    assert_eq!(
        engine.timing_intents.lock().unwrap()[215],
        TimingIntent::Legacy
    );
    assert_eq!(
        engine
            .input_runtime_ownership
            .cold_status(215, independent_request),
        Some(2)
    );
    assert!(
        callback.mixer.can_play_sample(215, 1.0),
        "ACKed independent Legacy source not playable: loop={:?}, resident={:?}, pending={}",
        callback.mixer.loop_region_frames(215),
        callback.independent.resident_binding(),
        engine.input_runtime_ownership.resident_control_pending(215)
    );
    assert!(callback.mixer.play_sample(215, 1.0));
    let engine = Python::attach(|py| Py::new(py, engine).unwrap());
    Python::attach(|py| {
        engine
            .call_method1(py, "set_pad_timing_intent", (0, "automatic"))
            .unwrap()
    });
    assert_eq!(callback.drain(), 1);
    Python::attach(
        |py| {
            while !engine.call_method0(py, "receive_msg").unwrap().is_none(py) {}
        },
    );
    (engine, callback, path, envelope)
}

pub(super) fn held_call(
    engine: &Py<AudioEngine>,
    callback: &mut Callback,
    method: &'static str,
    arguments: Py<PyTuple>,
    point: WorkPoint,
    during: impl FnOnce(&Py<AudioEngine>, &mut Callback),
) -> Result<Py<PyAny>, String> {
    let worker_engine = Python::attach(|py| engine.clone_ref(py));
    let (paused_sender, paused_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let mut release = WorkerRelease(Some(release_sender));
    let (result_sender, result_receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _reset = ObserverReset;
        let first = AtomicBool::new(true);
        set_observer(Some(Box::new(move |actual| {
            if actual == point && first.swap(false, Ordering::AcqRel) {
                paused_sender.send(std::thread::current().id()).unwrap();
                release_receiver
                    .recv_timeout(Duration::from_secs(20))
                    .expect("UI did not release held actual timing work");
            }
        })));
        let result = Python::attach(|py| {
            worker_engine
                .call_method1(py, method, arguments)
                .map_err(|error| error.to_string())
        });
        result_sender.send(result).unwrap();
    });
    let worker_thread = paused_receiver
        .recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| match result_receiver.try_recv() {
            Ok(Err(error)) => panic!("public {method} failed before {point:?}: {error}"),
            Ok(Ok(_)) => panic!("public {method} returned without reaching {point:?}"),
            Err(error) => panic!("public {method} did not reach {point:?}: {error}"),
        });
    assert_ne!(worker_thread, std::thread::current().id());
    assert!(matches!(
        result_receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    Python::attach(|py| {
        engine
            .call_method0(py, "ping")
            .expect("mutable public ping was borrowed by detached work")
    });
    assert_eq!(callback.drain(), 1);
    Python::attach(|py| {
        let message = engine
            .call_method0(py, "receive_msg")
            .expect("public receive_msg was borrowed by detached work");
        assert!(matches!(
            &*message.extract::<PyRef<AudioMessage>>(py).unwrap(),
            AudioMessage::Pong()
        ));
        assert!(engine.call_method0(py, "receive_msg").unwrap().is_none(py));
    });
    callback.render();
    during(engine, callback);
    assert!(matches!(
        result_receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    release.release();
    let result = result_receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("public timing work did not finish after release");
    worker.join().unwrap();
    result
}

fn prepare_arguments(py: Python<'_>) -> Py<PyTuple> {
    (0, 0.05, "independent fixture matching tolerance")
        .into_pyobject(py)
        .unwrap()
        .unbind()
}

fn publication_arguments(py: Python<'_>, ticket: &Py<ConstantTimingTicket>) -> Py<PyTuple> {
    let ticket_ref = ticket.borrow(py);
    let counts: Vec<Option<i64>> = ticket_ref
        .evidence
        .beat_seconds()
        .iter()
        .map(|time| Some((time / tests::PERIOD).round() as i64))
        .collect();
    let hypotheses = serde_json::json!([{"id":"independent-pulse-quarters", "provenance":"generated quarter truth", "verification":"verified", "quarter_note_denominator":1, "quarter_counts":counts}]).to_string();
    (
        ticket.clone_ref(py),
        hypotheses,
        tests::origin().seconds,
        tests::origin().provenance,
        tests::decision().policy_version,
        tests::decision().provenance,
    )
        .into_pyobject(py)
        .unwrap()
        .unbind()
}

fn acknowledge(
    engine: &Py<AudioEngine>,
    callback: &mut Callback,
    ticket: &Py<ConstantTimingTicket>,
) {
    Python::attach(|py| {
        assert_eq!(ticket.borrow(py).publication_status().unwrap(), "pending");
        assert!(
            engine
                .call_method1(py, "current_constant_timing", (0,))
                .unwrap()
                .is_none(py)
        );
    });
    assert_eq!(callback.drain(), 1);
    Python::attach(|py| {
        assert_eq!(ticket.borrow(py).publication_status().unwrap(), "accepted");
        assert!(
            !engine
                .call_method1(py, "current_constant_timing", (0,))
                .unwrap()
                .is_none(py)
        );
    });
    callback.render();
}

fn accept_current_source_timing(engine: &Py<AudioEngine>, callback: &mut Callback) -> u64 {
    let ticket = Python::attach(|py| {
        engine
            .call_method1(py, "prepare_constant_timing", prepare_arguments(py))
            .unwrap()
            .extract::<Py<ConstantTimingTicket>>(py)
            .unwrap()
    });
    Python::attach(|py| {
        engine
            .call_method1(
                py,
                "publish_constant_timing",
                publication_arguments(py, &ticket),
            )
            .unwrap();
    });
    acknowledge(engine, callback, &ticket);
    Python::attach(|py| ticket.borrow(py).request_id)
}

#[test]
fn public_native_background_borrow_preparation_publication_and_busy_guard() {
    for captured in [false, true] {
        let (_directory, engine, mut callback, _path, _envelope) = engine();
        let (method, arguments) = Python::attach(|py| {
            if captured {
                let binding = engine
                    .call_method1(py, "current_input_runtime_pad_binding", (0,))
                    .unwrap();
                assert!(!binding.is_none(py));
                let capture = engine
                    .call_method1(
                        py,
                        "capture_current_constant_timing",
                        (binding, 0.05, "independent fixture matching tolerance"),
                    )
                    .unwrap();
                (
                    "prepare_captured_constant_timing",
                    (capture,).into_pyobject(py).unwrap().unbind(),
                )
            } else {
                ("prepare_constant_timing", prepare_arguments(py))
            }
        });
        let result = held_call(
            &engine,
            &mut callback,
            method,
            arguments,
            WorkPoint::TimingPcm,
            |engine, callback| {
                Python::attach(|py| {
                    let error = engine
                        .call_method1(py, "prepare_constant_timing", prepare_arguments(py))
                        .unwrap_err();
                    assert!(
                        error
                            .to_string()
                            .contains("constant timing preparation busy")
                    );
                });
                assert!(callback.consumer.peek().is_err());
            },
        )
        .unwrap();
        let ticket = Python::attach(|py| result.extract::<Py<ConstantTimingTicket>>(py).unwrap());
        Python::attach(|py| {
            assert_eq!(ticket.borrow(py).publication_status().unwrap(), "captured")
        });
        let args = Python::attach(|py| publication_arguments(py, &ticket));
        held_call(
            &engine,
            &mut callback,
            "publish_constant_timing",
            args,
            WorkPoint::AcceptedTiming,
            |_, callback| assert!(callback.consumer.peek().is_err()),
        )
        .unwrap();
        acknowledge(&engine, &mut callback, &ticket);
    }
}

#[test]
fn public_native_background_borrow_saved_restore_export_and_second_engine() {
    let mut saved_roundtrip = None;
    for _ in 0..2 {
        let (_directory, engine, mut callback, path, envelope) = engine();
        let envelope = saved_roundtrip.as_ref().unwrap_or(&envelope);
        let args = Python::attach(|py| {
            let saved = engine
                .call_method1(py, "capture_saved_constant_timing", (0, envelope, &path))
                .unwrap();
            (saved,).into_pyobject(py).unwrap().unbind()
        });
        let result = held_call(
            &engine,
            &mut callback,
            "restore_constant_timing",
            args,
            WorkPoint::SavedTimingSource,
            |_, callback| assert!(callback.consumer.peek().is_err()),
        )
        .unwrap();
        let ticket = Python::attach(|py| result.extract::<Py<ConstantTimingTicket>>(py).unwrap());
        acknowledge(&engine, &mut callback, &ticket);
        let mut last_export = None;
        for point in [
            WorkPoint::SavedTimingEncode,
            WorkPoint::SavedTimingSerialize,
            WorkPoint::SavedTimingSource,
        ] {
            let args = Python::attach(|py| (0, &path).into_pyobject(py).unwrap().unbind());
            let exported = held_call(
                &engine,
                &mut callback,
                "export_current_constant_timing",
                args,
                point,
                |_, callback| assert!(callback.consumer.peek().is_err()),
            )
            .unwrap();
            let exported: String = Python::attach(|py| exported.extract(py).unwrap());
            assert_eq!(&exported, envelope);
            last_export = Some(exported);
        }
        assert!(callback.consumer.peek().is_err());
        saved_roundtrip = last_export;
    }
}

#[test]
fn public_native_background_borrow_rejects_changed_intent_and_unloaded_source() {
    for unload in [false, true] {
        let (_directory, engine, mut callback, path, envelope) = engine();
        let args = Python::attach(|py| {
            let saved = engine
                .call_method1(py, "capture_saved_constant_timing", (0, envelope, path))
                .unwrap();
            (saved,).into_pyobject(py).unwrap().unbind()
        });
        let result = held_call(
            &engine,
            &mut callback,
            "restore_constant_timing",
            args,
            WorkPoint::SavedTimingSource,
            |engine, callback| {
                Python::attach(|py| {
                    if unload {
                        engine.call_method1(py, "unload_sample", (0,)).unwrap();
                    } else {
                        engine
                            .call_method1(py, "set_pad_timing_intent", (0, "manual"))
                            .unwrap();
                    }
                });
                assert!(callback.drain() >= 1);
                callback.render();
            },
        );
        let error = result.unwrap_err();
        assert!(
            error.contains("cancelled")
                || error.contains("stale")
                || error.contains("not automatic"),
            "unexpected stale rejection: {error}"
        );
        assert!(callback.consumer.peek().is_err());
        Python::attach(|py| {
            assert!(
                engine
                    .call_method1(py, "current_constant_timing", (0,))
                    .unwrap()
                    .is_none(py)
            );
            assert!(
                !engine
                    .borrow(py)
                    .constant_timing_busy
                    .load(Ordering::Acquire)
            );
        });
    }
}

#[test]
fn public_native_background_borrow_full_queue_preserves_unpublished_ticket() {
    let (_directory, engine, mut callback, _path, _envelope) = engine();
    let ticket = Python::attach(|py| {
        engine
            .call_method1(py, "prepare_constant_timing", prepare_arguments(py))
            .unwrap()
            .extract::<Py<ConstantTimingTicket>>(py)
            .unwrap()
    });
    let args = Python::attach(|py| publication_arguments(py, &ticket));
    let result = held_call(
        &engine,
        &mut callback,
        "publish_constant_timing",
        args,
        WorkPoint::AcceptedTiming,
        |engine, _| {
            Python::attach(|py| {
                for _ in 0..16 {
                    engine.call_method0(py, "ping").unwrap();
                }
                assert!(
                    engine
                        .call_method0(py, "ping")
                        .unwrap_err()
                        .to_string()
                        .contains("buffer may be full")
                );
            });
        },
    );
    assert!(
        result
            .unwrap_err()
            .contains("Failed to send PublishConstantTiming")
    );
    Python::attach(|py| {
        assert_eq!(ticket.borrow(py).publication_status().unwrap(), "captured");
        assert!(
            engine
                .call_method1(py, "current_constant_timing", (0,))
                .unwrap()
                .is_none(py)
        );
    });
    assert_eq!(callback.drain(), 16);
    callback.render();
    let args = Python::attach(|py| publication_arguments(py, &ticket));
    Python::attach(|py| {
        engine
            .call_method1(py, "publish_constant_timing", args)
            .unwrap()
    });
    acknowledge(&engine, &mut callback, &ticket);
}
