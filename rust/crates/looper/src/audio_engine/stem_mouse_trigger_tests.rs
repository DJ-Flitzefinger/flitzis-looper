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

fn original_start(
    engine: &Py<AudioEngine>,
    probe: &Py<PyAny>,
    timestamp: u64,
) -> crate::audio_engine::pad_start::CapturedPadStart {
    Python::attach(|py| {
        let start = probe
            .call_method1(py, "mouse_original_start", (timestamp,))
            .unwrap()
            .extract::<PyRef<crate::audio_engine::pad_start::CapturedPadStart>>(py)
            .unwrap()
            .clone();
        assert_eq!(start.received_timestamp(), timestamp);
        assert!(start.belongs_to(&engine.borrow(py).input_runtime_ownership));
        assert_eq!(start.effective_frame(), None);
        start
    })
}

fn drain_one(callback: &mut Callback, consumer: &mut rtrb::Consumer<ControlMessage>) {
    let message = consumer.pop().expect("one original producer message");
    let (mut producer, mut one) = rtrb::RingBuffer::new(1);
    producer.push(message).unwrap();
    assert_eq!(callback.drain(&mut one), 1);
}

fn acknowledge_windows(
    probe: &Py<PyAny>,
    callback: &mut Callback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    originals: &[crate::audio_engine::pad_start::CapturedPadStart],
    expected_windows: usize,
) {
    let mut captures = 0;
    let mut windows = 0;
    while let Ok(message) = consumer.peek() {
        match message {
            ControlMessage::CapturePadStartTarget(start) => {
                assert!(start.same_owner(&originals[captures]));
                captures += 1;
            }
            ControlMessage::RelocateResident(_) => windows += 1,
            _ => panic!("unexpected work before the original WindowACK: {message:?}"),
        }
        drain_one(callback, consumer);
    }
    assert_eq!(captures, originals.len());
    assert_eq!(windows, expected_windows);
    for original in originals {
        assert_eq!(original.original_frame(), Some(0));
        assert_eq!(original.effective_frame(), None);
        Python::attach(|py| {
            let ticket = probe
                .call_method1(py, "mouse_start_window", (original.received_timestamp(),))
                .unwrap();
            let ticket = ticket.extract::<PyRef<ResidentWindowTicket>>(py).unwrap();
            assert_eq!(ticket.publication_status(), "accepted");
            assert!(ticket.is_current());
            assert!(
                ticket
                    .capture_pad_start_launch_guard(original)
                    .unwrap()
                    .is_some()
            );
        });
    }
}

fn apply_originals(
    engine: &Py<AudioEngine>,
    callback: &mut Callback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    originals: &[crate::audio_engine::pad_start::CapturedPadStart],
) {
    for (index, original) in originals.iter().enumerate() {
        assert!(matches!(
            consumer.peek(),
            Ok(ControlMessage::PreparedStart(_))
        ));
        drain_one(callback, consumer);
        assert_eq!(original.state(), "applied", "{:?}", original.reason());
        assert_eq!(original.original_frame(), Some(0));
        assert_eq!(original.execution_frame(), Some(0));
        assert_eq!(original.effective_frame(), Some(0));
        assert!(
            originals[index + 1..]
                .iter()
                .all(|later| later.effective_frame().is_none())
        );
        // Each independent exclusive effect owns one birth, preceded by its actual
        // old-voice STOP when another member has already become active.
        for expected_kind in if index == 0 { &[1][..] } else { &[2, 1][..] } {
            let message = callback.feedback_rx.pop().expect("own effect feedback");
            Python::attach(|py| {
                let engine = engine.borrow(py);
                let ownership = &engine.input_runtime_ownership;
                let key = ownership.callback_feedback_key(0).unwrap();
                let AudioMessage::ContentFeedback {
                    id,
                    owner,
                    cell,
                    lifetime,
                    transport_revision,
                    kind,
                    value,
                } = &message
                else {
                    panic!("original effect lacks owned feedback: {message:?}")
                };
                assert_eq!(
                    (*id, *owner, *cell, *lifetime, *kind, value.to_bits()),
                    (
                        0,
                        ownership.owner_identity().diagnostic(),
                        key.cell().index(),
                        key.lifetime(),
                        *expected_kind,
                        0.0_f64.to_bits()
                    )
                );
                assert_eq!(
                    *transport_revision,
                    ownership
                        .transport_intent_for_cell(key.cell())
                        .load(Ordering::Acquire)
                );
                assert_eq!(engine._resolve_pad_feedback(&message).unwrap(), Some(0));
            });
        }
        assert!(
            callback.feedback_rx.is_empty(),
            "one exact original effect's receipts"
        );
    }
    assert!(consumer.is_empty());
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
    let pressure = Python::attach(|py| ColdPressure::new(&engine.borrow(py)));
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
            let mut originals = vec![original_start(engine, probe, timestamp)];
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
                matches!(
                    consumer.peek(),
                    Ok(ControlMessage::CapturePadStartTarget(_))
                ),
                "ready mouse gesture must retain its original capture without cold capacity: {}",
                Python::attach(|py| probe
                    .call_method0(py, "mouse_status")
                    .unwrap()
                    .extract::<String>(py)
                    .unwrap())
            );
            if index == 0 {
                // Equal real down edges share readiness while retaining three
                // separately owned accepted attacks, timestamps and sequences.
                for _ in 0..2 {
                    gesture(probe, false, false);
                    let next_timestamp = gesture(probe, true, true);
                    assert!(next_timestamp > timestamp);
                    timestamp = next_timestamp;
                    let next = original_start(engine, probe, timestamp);
                    assert!(next.sequence() > originals.last().unwrap().sequence());
                    assert!(!next.same_owner(originals.last().unwrap()));
                    assert_eq!(next.content_key(), originals[0].content_key());
                    originals.push(next);
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
            acknowledge_windows(probe, callback, consumer, &originals, 1);
            call(probe, "mouse_poll");
            apply_originals(engine, callback, consumer, &originals);
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
        while callback.feedback_rx.pop().is_ok() {}
        // Re-verifying the identical committed pair can reuse its accepted source
        // registration. It must not fabricate rejection or a replacement owner.
        gesture(probe, true, false);
        let old_timestamp = gesture(probe, true, true);
        let old_original = original_start(engine, probe, old_timestamp);
        acknowledge_windows(probe, callback, consumer, &[old_original.clone()], 1);
        let (old_window, old_guard, old_control_epoch, source_generation) = Python::attach(|py| {
            let window = probe
                .call_method1(py, "mouse_start_window", (old_timestamp,))
                .unwrap()
                .extract::<Py<ResidentWindowTicket>>(py)
                .unwrap();
            let (_, guard) = window
                .borrow(py)
                .capture_pad_start_launch_guard(&old_original)
                .unwrap()
                .unwrap();
            let engine = engine.borrow(py);
            let control_epoch = engine.resident_stem_cache.lock().unwrap()[0]
                .intent_epoch
                .load(Ordering::Acquire);
            let generation = engine.loaded_source_generations.lock().unwrap()[0];
            (window, guard, control_epoch, generation)
        });
        assert!(old_guard.current());
        // The occupied-lane ready-start proof is complete. The separate offline
        // restore requires real worker admission, so await every pressure job's
        // return before entering Python's restore/preparation wait.
        drop(pressure);
        wait_until(Duration::from_secs(10), || {
            Python::attach(|py| engine.borrow(py).cold_jobs.counts_for_test()) == (0, 0, 0)
        });
        assert_eq!(old_original.effective_frame(), None);
        call(probe, "restore");
        committed_probe_ack(engine, callback, consumer, probe, "restored", 0);
        call(probe, "restored_accepted");
        assert_eq!(old_original.effective_frame(), None);
        // The explicit committed residency owns a newer control epoch even when
        // immutable PCM and its physical window revision are reused. Historical
        // WindowACK is not current authority for this still-unpublished attack.
        let restored_control_epoch = Python::attach(|py| {
            let engine = engine.borrow(py);
            let epoch = engine.resident_stem_cache.lock().unwrap()[0]
                .intent_epoch
                .load(Ordering::Acquire);
            let pending_control = engine
                .input_runtime_ownership
                .resident_control_for_cell(old_original.content_key().cell())
                .load(Ordering::Acquire);
            let current = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
            let old = old_window.borrow(py);
            let old_status = old.publication_status();
            let old_current = old.is_current();
            let old_guard_current = old_guard.current();
            let own_guard_available = old
                .capture_pad_start_launch_guard(&old_original)
                .unwrap()
                .is_some();
            let published = engine
                ._publish_pad_start(&old_original, Some(&old))
                .unwrap();
            let observed = format!(
                "post-restore original: intent {old_control_epoch}->{epoch}, pending_control={pending_control}, \
                 window {}/{}, status={old_status}, ticket_current={old_current}, \
                 guard_current={old_guard_current}, own_guard={own_guard_available}, \
                 publish={published}, state={}, T={:?}, E={:?}, effective={:?}",
                sample.window_revision(),
                current.window_revision(),
                old_original.state(),
                old_original.original_frame(),
                old_original.execution_frame(),
                old_original.effective_frame(),
            );
            // resident_control is the transient in-flight marker. ACK clears it;
            // the retained publication's genuine fence is owner.intent_epoch.
            assert_eq!(pending_control, 0, "{observed}");
            assert_eq!(
                epoch,
                old_control_epoch.checked_add(1).unwrap(),
                "{observed}"
            );
            assert_eq!(
                engine.loaded_source_generations.lock().unwrap()[0],
                source_generation
            );
            assert!(Arc::ptr_eq(&sample.samples, &current.samples));
            assert_eq!(current.window_revision(), sample.window_revision());
            assert_eq!(old_status, "accepted", "{observed}");
            assert!(!old_current, "{observed}");
            assert!(!old_guard_current, "{observed}");
            assert!(!own_guard_available, "{observed}");
            assert!(!published, "{observed}");
            epoch
        });
        assert!(!old_guard.current());
        assert_eq!(old_original.state(), "captured");
        assert!(!old_original.is_published());
        assert_eq!(old_original.original_frame(), Some(0));
        assert_eq!(old_original.execution_frame(), None);
        assert_eq!(old_original.effective_frame(), None);
        assert!(consumer.is_empty());
        assert!(callback.feedback_rx.is_empty());
        assert!(
            !callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(0))
        );
        // Withdraw the honestly refused, unapplied gesture before asking for a
        // fresh one. Cancellation never relabels its prior ACK as a new owner.
        assert!(old_original.cancel());
        call(probe, "mouse_poll");
        assert_eq!(old_original.state(), "rejected");
        assert_eq!(old_original.reason(), Some("launch_cancelled"));
        assert!(consumer.is_empty());
        let _renewed_pressure = Python::attach(|py| ColdPressure::new(&engine.borrow(py)));
        gesture(probe, false, false);
        let latest_timestamp = gesture(probe, true, true);
        assert!(latest_timestamp > old_timestamp);
        let latest = original_start(engine, probe, latest_timestamp);
        assert!(latest.sequence() > old_original.sequence());
        assert!(!latest.same_owner(&old_original));
        assert_eq!(latest.content_key(), old_original.content_key());
        // A new original needs its own current WindowACK under the newer epoch;
        // equal physical window geometry cannot substitute for that receipt.
        acknowledge_windows(probe, callback, consumer, &[latest.clone()], 1);
        Python::attach(|py| {
            let engine = engine.borrow(py);
            assert_eq!(
                engine.resident_stem_cache.lock().unwrap()[0]
                    .intent_epoch
                    .load(Ordering::Acquire),
                restored_control_epoch.checked_add(1).unwrap()
            );
            assert_eq!(
                engine
                    .input_runtime_ownership
                    .resident_control_for_cell(latest.content_key().cell())
                    .load(Ordering::Acquire),
                0
            );
            let window = probe
                .call_method1(py, "mouse_start_window", (latest_timestamp,))
                .unwrap();
            let revision = window
                .getattr(py, "window_revision")
                .unwrap()
                .extract::<u64>(py)
                .unwrap();
            assert_eq!(revision, sample.window_revision());
            let window = window.extract::<PyRef<ResidentWindowTicket>>(py).unwrap();
            assert!(
                window
                    .capture_pad_start_launch_guard(&old_original)
                    .unwrap()
                    .is_none()
            );
            assert!(
                window
                    .capture_pad_start_launch_guard(&latest)
                    .unwrap()
                    .is_some()
            );
            assert_eq!(engine.cold_jobs.counts_for_test(), (2, 0, 34));
        });
        assert!(
            !callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(0))
        );
        call(probe, "mouse_poll");
        apply_originals(engine, callback, consumer, &[latest]);
        assert_eq!(old_original.effective_frame(), None);
        render_started(callback, VOCALS_AMPLITUDE);
        gesture(probe, false, false);
        call(probe, "mouse_stop");
        assert_eq!(callback.drain(consumer), 1);
    }
    call(probe, "mouse_close");
}
