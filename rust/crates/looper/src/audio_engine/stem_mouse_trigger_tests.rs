//! Real ImGui edges -> UiContext -> controller -> native ACK/launch -> PCM.
//! Cold workers are held by condition variables; no timing sleep proves readiness.
use super::*;
use std::sync::{Condvar, mpsc};

struct ColdPressure {
    release: Arc<(Mutex<bool>, Condvar)>,
    _reservations: Vec<super::super::super::cold_jobs::Reservation>,
}

impl ColdPressure {
    fn new(engine: &AudioEngine) -> Self {
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let mut gate = Self {
            release: release.clone(),
            _reservations: Vec::new(),
        };
        let (started_tx, started_rx) = mpsc::channel();
        for _ in 0..2 {
            let worker_release = release.clone();
            let started = started_tx.clone();
            engine
                .cold_jobs
                .submit(engine.cold_jobs.reserve().unwrap(), move || {
                    started.send(()).unwrap();
                    let (lock, wake) = &*worker_release;
                    let mut done = lock.lock().unwrap();
                    while !*done {
                        done = wake.wait(done).unwrap();
                    }
                })
                .unwrap();
        }
        for _ in 0..2 {
            started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        gate._reservations = (0..32)
            .map(|_| engine.cold_jobs.reserve().unwrap())
            .collect();
        assert_eq!(engine.cold_jobs.counts_for_test(), (2, 0, 34));
        assert!(engine.cold_jobs.reserve().is_err());
        gate
    }
}

impl Drop for ColdPressure {
    fn drop(&mut self) {
        let (lock, wake) = &*self.release;
        *lock.lock().unwrap() = true;
        wake.notify_all();
    }
}

fn gesture(probe: &Py<PyAny>, inside: bool, down: bool) -> u64 {
    Python::attach(|py| {
        let keywords = pyo3::types::PyDict::new(py);
        keywords.set_item("inside", inside).unwrap();
        keywords.set_item("down", down).unwrap();
        probe
            .bind(py)
            .call_method("mouse_frame", (), Some(&keywords))
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn render_started(callback: &mut Callback, expected: f32) {
    assert!(
        callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    let mut output = [0.0; 512 * 2];
    callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        0,
        &mut callback.retirement,
    );
    assert!(output.iter().all(|value| value.is_finite()));
    assert!(output[..8].iter().any(|value| *value > 0.0));
    for (index, value) in output.iter().enumerate().skip(256 * 2) {
        assert_eq!(value.to_bits(), expected.to_bits(), "mouse PCM {index}");
    }
}

pub(super) fn prove_ready_mouse_triggers(
    engine: &Py<AudioEngine>,
    probe: &Py<PyAny>,
    callback: &mut Callback,
    producer: &Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    version: Option<&str>,
) {
    call(probe, "mouse_setup");
    let _pressure = Python::attach(|py| ColdPressure::new(&engine.borrow(py)));
    let sample = Python::attach(|py| {
        engine.borrow(py).sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap()
    });
    for selection in 0..if version.is_some() { 3 } else { 1 } {
        let expected = if selection == 0 {
            // Complete-source i16 decode uses Symphonia's signed PCM scale;
            // prepared component WAV decode independently uses i16::MAX.
            8192.0 / 32768.0
        } else {
            VOCALS_AMPLITUDE
        };
        if let Some(version) = version {
            let mut producer = producer.lock().unwrap();
            let source_version_hash = super::super::super::stem_cache::source_version_hash(version);
            producer
                .push(ControlMessage::SetStemMixMode {
                    id: 0,
                    mode: if selection == 0 {
                        StemMixMode::FullMix
                    } else {
                        StemMixMode::AllStems
                    },
                    source_version_hash,
                })
                .unwrap();
            producer
                .push(ControlMessage::SetStemEnabledMask {
                    id: 0,
                    enabled_stem_mask: 1,
                    source_version_hash,
                })
                .unwrap();
            drop(producer);
            assert_eq!(callback.drain(consumer), 2);
        }
        gesture(probe, true, false);
        for index in 0..12 {
            while callback.feedback_rx.pop().is_ok() {}
            let capture_count = Python::attach(|py| {
                probe
                    .call_method0(py, "mouse_capture_count")
                    .unwrap()
                    .extract::<usize>(py)
                    .unwrap()
            });
            let mut timestamp = gesture(probe, true, true);
            assert!(timestamp > 0);
            assert_eq!(
                Python::attach(|py| {
                    probe
                        .call_method0(py, "mouse_capture_count")
                        .unwrap()
                        .extract::<usize>(py)
                        .unwrap()
                }),
                capture_count + 1
            );
            assert!(
                timestamp <= Python::attach(|py| engine.borrow(py).capture_input_timestamp_ns())
            );
            assert!(
                matches!(consumer.peek(), Ok(ControlMessage::RelocateResident(_))),
                "ready mouse gesture must enqueue ACK without cold capacity: {}",
                Python::attach(|py| probe
                    .call_method0(py, "mouse_status")
                    .unwrap()
                    .extract::<String>(py)
                    .unwrap())
            );
            if index == 0 {
                // Several real down edges before ACK share one transaction, and
                // only the latest original native timestamp reaches playback.
                for _ in 0..2 {
                    gesture(probe, false, false);
                    let next_timestamp = gesture(probe, true, true);
                    assert!(next_timestamp > timestamp);
                    timestamp = next_timestamp;
                }
            }
            // ACK is still mandatory; polling before native drain cannot start.
            call(probe, "mouse_poll");
            assert!(
                !callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(0))
            );
            assert_eq!(callback.drain(consumer), 1);
            call(probe, "mouse_poll");
            let Ok(ControlMessage::TriggerInputPad {
                received_at_ns,
                resident_control,
                ..
            }) = consumer.peek()
            else {
                panic!("matching ACK must enqueue the guarded mouse trigger");
            };
            assert_eq!(*received_at_ns, timestamp);
            assert!(resident_control.is_some());
            assert_eq!(callback.drain(consumer), 1);
            render_started(callback, expected);
            let current = Python::attach(|py| {
                engine.borrow(py).sample_cache.lock().unwrap()[0]
                    .clone()
                    .unwrap()
            });
            assert!(Arc::ptr_eq(&sample.samples, &current.samples));
            assert_eq!(sample.window_revision(), current.window_revision());
            assert_eq!(
                Python::attach(|py| engine.borrow(py).cold_jobs.counts_for_test()),
                (2, 0, 34)
            );
            // Holding never repeats. Release outside must permit the next edge.
            assert_eq!(gesture(probe, true, true), timestamp);
            assert!(consumer.is_empty());
            gesture(probe, index % 2 == 0, false);
            call(probe, "mouse_stop");
            assert_eq!(callback.drain(consumer), 1);
            assert!(
                !callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(0))
            );
        }
    }
    if version.is_some() {
        // A real cache-restoration publication replaces the complete-set token.
        // The old ready ACK has happened, but Python has not observed/launched it.
        gesture(probe, true, false);
        let old_timestamp = gesture(probe, true, true);
        assert_eq!(callback.drain(consumer), 1);
        call(probe, "restore");
        gesture(probe, false, false);
        let latest_timestamp = gesture(probe, true, true);
        assert!(latest_timestamp > old_timestamp);
        let status = Python::attach(|py| {
            probe
                .call_method0(py, "mouse_status")
                .unwrap()
                .extract::<String>(py)
                .unwrap()
        });
        assert!(
            status.contains("resident stem publication is pending"),
            "{status}"
        );
        assert_eq!(callback.drain(consumer), 1);
        call(probe, "restored_accepted");
        call(probe, "mouse_poll");
        assert!(matches!(
            consumer.peek(),
            Ok(ControlMessage::RelocateResident(_))
        ));
        assert!(
            !callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(0))
        );
        assert_eq!(callback.drain(consumer), 1);
        call(probe, "mouse_poll");
        let Ok(ControlMessage::TriggerInputPad {
            received_at_ns,
            resident_control,
            ..
        }) = consumer.peek()
        else {
            panic!("replacement requires fresh ACK and guarded latest launch");
        };
        assert_eq!(*received_at_ns, latest_timestamp);
        assert!(resident_control.is_some());
        assert_eq!(callback.drain(consumer), 1);
        render_started(callback, VOCALS_AMPLITUDE);
        gesture(probe, false, false);
        call(probe, "mouse_stop");
        assert_eq!(callback.drain(consumer), 1);
    }
    call(probe, "mouse_close");
}
