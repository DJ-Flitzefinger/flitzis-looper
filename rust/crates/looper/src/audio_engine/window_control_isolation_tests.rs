//! Public controls and real source/window readers; no App, device or inference.
use super::*;
use crate::audio_engine::native_owned_work::{self, WorkPoint};

struct SourceHold {
    gate: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    entered: mpsc::Receiver<()>,
}
impl SourceHold {
    fn new(point: WorkPoint) -> Self {
        let gate = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let held = gate.clone();
        let (tx, entered) = mpsc::channel();
        let first = std::sync::atomic::AtomicBool::new(true);
        native_owned_work::set_preparation_observer(Some(Box::new(move |observed| {
            if observed == point && first.swap(false, std::sync::atomic::Ordering::AcqRel) {
                tx.send(()).unwrap();
                let (lock, ready) = &*held;
                let (released, timeout) = ready
                    .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(10), |v| !*v)
                    .unwrap();
                assert!(
                    *released && !timeout.timed_out(),
                    "source reader was not released"
                );
            }
        })));
        Self { gate, entered }
    }
    fn wait(&self) {
        self.entered.recv_timeout(Duration::from_secs(10)).unwrap();
    }
    fn release(&self) {
        *self.gate.0.lock().unwrap() = true;
        self.gate.1.notify_all();
    }
}
impl Drop for SourceHold {
    fn drop(&mut self) {
        self.release();
    }
}

#[test]
fn conversion_cancel_keeps_real_worker_source_and_processing_until_native_return() {
    use crate::audio_engine::stem_conversion::StemConversionReservation;
    use pyo3::types::{PyCFunction, PyModule};
    use std::ffi::CString;
    let f = Productive::new(false, 1.0);
    let source =
        f.h.engine
            .capture_offline_stem_source(0, f.h.version.clone())
            .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let hold = SourceHold::new(WorkPoint::StemConversion);
    let observer = Arc::new(std::sync::Mutex::new(
        native_owned_work::take_preparation_observer(),
    ));
    let baseline = crate::audio_engine::resident_host_memory::pending_bytes().unwrap();
    let probe = Python::attach(|py| {
        let install = PyCFunction::new_closure(py, None, None, move |_, _| -> PyResult<()> {
            native_owned_work::set_preparation_observer(observer.lock().unwrap().take());
            Ok(())
        })
        .unwrap();
        let probe = PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/stem_conversion_cancel_probe.py"
            )))
            .unwrap(),
            &CString::new("stem_conversion_cancel_probe.py").unwrap(),
            &CString::new("stem_conversion_cancel_probe").unwrap(),
        )
        .unwrap();
        let session = probe
            .call_method1(
                "start",
                (
                    Py::new(py, source).unwrap(),
                    directory.path().to_string_lossy().as_ref(),
                    py.get_type::<StemConversionReservation>(),
                    install,
                ),
            )
            .unwrap();
        session.unbind()
    });
    hold.wait();
    assert!(crate::audio_engine::resident_host_memory::pending_bytes().unwrap() > baseline);
    Python::attach(|py| {
        probe
            .call_method0(py, "cancel_while_inside_native")
            .unwrap()
    });
    hold.release();
    Python::attach(|py| probe.call_method0(py, "finish").unwrap());
    assert_eq!(
        crate::audio_engine::resident_host_memory::pending_bytes().unwrap(),
        baseline
    );
}

#[test]
fn detached_conversion_allows_public_mutable_controls_actual_start_pcm_and_stop() {
    use crate::audio_engine::stem_conversion::StemConversionReservation;
    use pyo3::types::PyBytes;
    let mut f = Productive::new(false, 1.0);
    f.h.engine.test_output_format = Some((RATE, 2));
    let dry = f.h.callback.mixer.bank_for_measurement()[215]
        .as_ref()
        .unwrap()
        .clone();
    let mut reference = RtMixer::new(2, RATE as f32);
    reference.load_sample(215, dry);
    assert!(reference.play_sample_at_output_frame(215, 1.0, 0));
    let hold = SourceHold::new(WorkPoint::StemConversion);
    let observer = native_owned_work::take_preparation_observer();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _observed = native_owned_work::PreparationObservationScope::install(observer);
        let result = Python::attach(|py| {
            let raw: Vec<u8> = (0..8194).flat_map(|_| 1234_i16.to_le_bytes()).collect();
            let pcm = PyBytes::new(py, &raw);
            let sources = std::array::from_fn::<_, 4, _>(|_| {
                (48000_u32, 2_u16, 4097_u64, 0_u64, pcm.clone())
            });
            let reservation = py
                .get_type::<StemConversionReservation>()
                .call1((1024 * 1024, 1024 * 1024))?;
            let payloads =
                reservation.call_method1("convert", (sources, (48000, 2, 4096), 0, 4096))?;
            let output: Vec<Vec<u8>> = payloads.extract()?;
            assert_eq!(output.len(), 5);
            let expected: Vec<u8> = (0..8192).flat_map(|_| 1234_i16.to_le_bytes()).collect();
            assert_eq!(output[0], expected);
            assert_eq!(
                output[4],
                (0..8192)
                    .flat_map(|_| 3702_i16.to_le_bytes())
                    .collect::<Vec<_>>()
            );
            Ok::<_, PyErr>(())
        })
        .map_err(|error| error.to_string());
        tx.send(result).unwrap();
    });
    hold.wait();
    let engine = Python::attach(|py| Py::new(py, f.h.engine).unwrap());
    Python::attach(|py| engine.call_method1(py, "stop_sample", (0,)).unwrap());
    f.h.callback.drain(&mut f.h.consumer);
    public_controls(
        &engine,
        &mut f.h.callback,
        &mut f.h.consumer,
        Some(&mut reference),
    );
    assert!(
        rx.try_recv().is_err(),
        "conversion must still be inside detached core"
    );
    hold.release();
    rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    worker.join().unwrap();
}

fn fresh_window(h: &Harness) -> crate::audio_engine::resident_relocation::ResidentWindowTicket {
    prepare_window_with_producer(
        &h.engine,
        0,
        WindowRequest {
            storage_range: Some((
                (LOOP_START - 1) as f64 / f64::from(RATE),
                (LOOP_END + 1) as f64 / f64::from(RATE),
            )),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap()
}

// This helper actually takes a mutable Engine borrow, admits and applies an
// independent ordinary gesture, then applies its STOP while the caller holds I/O.
fn public_controls(
    engine: &Py<crate::audio_engine::AudioEngine>,
    callback: &mut Callback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    reference: Option<&mut RtMixer>,
) {
    let progress_started = Instant::now();
    let report_progress = reference.is_some();
    let control_engine = Python::attach(|py| engine.clone_ref(py));
    let (tx, rx) = mpsc::channel();
    let control = std::thread::spawn(move || {
        let result = Python::attach(|py| {
            control_engine.call_method0(py, "ping")?;
            let start =
                control_engine.call_method1(py, "capture_pad_start", (215, 0.0, py.None(), 1.0))?;
            let owner = start
                .extract::<PyRef<crate::audio_engine::pad_start::CapturedPadStart>>(py)?
                .clone();
            let admitted: bool = control_engine
                .call_method1(py, "publish_pad_start", (start,))?
                .extract(py)?;
            Ok::<_, PyErr>((admitted, owner))
        })
        .map_err(|error| error.to_string());
        tx.send(result).unwrap();
    });
    let result = rx.recv_timeout(Duration::from_millis(750));
    assert!(
        result.is_ok(),
        "public mutable control/independent trigger did not progress while held"
    );
    let (admitted, start) = result.unwrap().unwrap();
    control.join().unwrap();
    assert!(admitted);
    callback.drain(consumer);
    let original_t = start.original_frame();
    assert!(original_t.is_some());
    wait_until(|| {
        callback.mixer.pitch_runtime.as_mut().unwrap().poll();
        assert_ne!(start.state(), "rejected", "{:?}", start.reason());
        start.state() == "applied"
            || callback
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .has_ready_action(start.sequence())
    });
    crate::audio_engine::audio_stream::render_scheduled_audio(
        &mut callback.mixer,
        &mut callback.scheduler,
        &mut [],
        &mut [0.0; NUM_SAMPLES],
        callback.transport.output_frame(),
        2,
        &mut callback.transport,
        &mut callback.feedback,
        &mut callback.retirement,
    );
    assert_eq!(start.state(), "applied");
    assert_eq!(start.original_frame(), original_t);
    assert!(
        callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(215))
    );
    if let Some(reference) = reference {
        let mut output = [0.0; 128];
        crate::audio_engine::audio_stream::render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            callback.transport.output_frame(),
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        assert!(output.iter().any(|sample| sample.abs() > 0.000001));
        let mut expected = [0.0; 128];
        reference.render_rt(
            &mut expected,
            &mut [0.0; NUM_SAMPLES],
            &mut callback.retirement,
        );
        assert_eq!(output.map(f32::to_bits), expected.map(f32::to_bits));
    }
    Python::attach(|py| engine.call_method1(py, "stop_sample", (215,)).unwrap());
    callback.drain(consumer);
    assert!(
        !callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(215))
    );
    if report_progress {
        eprintln!(
            "stem-conversion held-core public START/64-frame bitexact PCM/STOP roundtrip_us={}",
            progress_started.elapsed().as_micros()
        );
    }
}

#[test]
fn window_real_source_and_component_reads_release_owners_and_public_controls() {
    for components in [false, true] {
        let mut f = Productive::new(false, 1.0);
        f.h.engine.test_output_format = Some((RATE, 2));
        if components {
            let all = f.request("all_stems", 15);
            f.capture_then_wait(&all);
            assert_eq!(f.h.callback.drain(&mut f.h.consumer), 1);
            reconcile(&f.h.engine).unwrap();
        }
        let source_hold = (!components).then(|| SourceHold::new(WorkPoint::WindowSourceChunk));
        let component_hold = components.then(|| ReadHold::new(&f.pair.pair_reader));
        let ticket = fresh_window(&f.h);
        if let Some(hold) = &source_hold {
            hold.wait();
        }
        if let Some(hold) = &component_hold {
            hold.wait();
        }
        assert!(f.h.engine.resident_stem_cache.try_lock().is_ok());
        assert!(f.h.producer.try_lock().is_ok());
        let status =
            f.h.engine
                .project_assets
                .resident_budget_status(&f.h.engine.resident_account)
                .unwrap();
        assert!(status.pending_reservation_count >= 2);
        if !components {
            assert_eq!(
                status.pending_resident_bytes,
                (LOOP_END - LOOP_START + 2) * 2 * 4
            );
        }
        let engine = Python::attach(|py| Py::new(py, f.h.engine).unwrap());
        public_controls(&engine, &mut f.h.callback, &mut f.h.consumer, None);
        assert_eq!(ticket.publication_status(), "preparing");
        if let Some(hold) = &source_hold {
            hold.release();
        }
        if let Some(hold) = &component_hold {
            hold.release();
        }
        wait_until(|| {
            ticket.publication_status() == "pending" || ticket.error().unwrap().is_some()
        });
        assert_eq!(
            ticket.publication_status(),
            "pending",
            "{:?}",
            ticket.error()
        );
        f.h.callback.drain(&mut f.h.consumer);
        assert_eq!(ticket.publication_status(), "accepted");
        let actual = f.h.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert_eq!(actual.resident_start(), LOOP_START - 1);
        assert_eq!(
            actual.samples.as_ref(),
            &f.h.material.sample.samples[(LOOP_START - 1) * 2..(LOOP_END + 1) * 2]
        );
        Python::attach(|py| reconcile(&engine.borrow(py)).unwrap());
        wait_until(|| {
            Python::attach(|py| engine.borrow(py).cold_jobs.counts_for_test() == (0, 0, 0))
        });
        let settled = Python::attach(|py| {
            let engine = engine.borrow(py);
            engine
                .project_assets
                .resident_budget_status(&engine.resident_account)
                .unwrap()
        });
        assert_eq!(
            (
                settled.pending_resident_bytes,
                settled.pending_reservation_count
            ),
            (0, 0)
        );
    }
}

#[test]
fn public_preflight_binding_and_both_registrations_release_engine_borrow() {
    for (method, point) in [
        (
            "request_committed_stem_residency",
            WorkPoint::CommittedStemPreflight,
        ),
        (
            "current_input_runtime_pad_binding",
            WorkPoint::BindingPreflight,
        ),
        (
            "register_committed_stem_pair",
            WorkPoint::CommittedStemRegistration,
        ),
        (
            "register_pending_committed_stem_pair",
            WorkPoint::CommittedStemRegistration,
        ),
    ] {
        let mut f = Productive::new(false, 1.0);
        f.h.engine.test_output_format = Some((RATE, 2));
        let source = f.h.ticket(0);
        let (engine, requester, pair, source, selected) = Python::attach(|py| {
            let engine = Py::new(py, f.h.engine).unwrap();
            (
                engine.clone_ref(py),
                engine,
                Py::new(py, f.pair).unwrap(),
                Py::new(py, source).unwrap(),
                Py::new(py, f.selected).unwrap(),
            )
        });
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let request = std::thread::spawn(move || {
            native_owned_work::set_observer(Some(Box::new(move |observed| {
                if observed == point {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                }
            })));
            let result = Python::attach(|py| match method {
                "request_committed_stem_residency" => {
                    requester.call_method1(py, method, (selected,))
                }
                "current_input_runtime_pad_binding" => requester.call_method1(py, method, (0,)),
                _ => requester.call_method1(py, method, (pair, source)),
            })
            .map_err(|error| error.to_string());
            native_owned_work::set_observer(None);
            done_tx.send(result).unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        public_controls(&engine, &mut f.h.callback, &mut f.h.consumer, None);
        release_tx.send(()).unwrap();
        let receipt = done_rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
        request.join().unwrap();
        if method == "request_committed_stem_residency" {
            Python::attach(|py| receipt.call_method0(py, "cancel").unwrap());
        }
        f.h.callback.drain(&mut f.h.consumer);
        wait_until(|| {
            Python::attach(|py| engine.borrow(py).cold_jobs.counts_for_test() == (0, 0, 0))
        });
    }
}

#[test]
fn source_read_history_follows_original_move_through_pending_mirror() {
    use crate::audio_engine::{content_placement::PlacementObservation, pad_pair};
    for mirror_while_held in [false, true] {
        let mut f = Productive::new(false, 1.0);
        let key =
            f.h.engine
                .input_runtime_ownership
                .slot_snapshot(0)
                .unwrap()
                .content_key()
                .unwrap();
        let original_row = f.h.engine.cold_pcm_history.lock().unwrap()[0]
            .identity
            .clone();
        let hold = SourceHold::new(WorkPoint::WindowSourceChunk);
        let window = fresh_window(&f.h);
        hold.wait();
        let captured = pad_pair::control::capture(&f.h.engine, 0, 1).unwrap();
        let pair = pad_pair::control::prepare(&f.h.engine, &captured, RATE, 2).unwrap();
        pad_pair::control::publish(&f.h.engine, &pair).unwrap();
        f.h.callback.drain(&mut f.h.consumer);
        assert!(pair.own_acknowledgement().is_some());
        assert!(matches!(
            f.h.engine.input_runtime_ownership.observe_control_key(key),
            PlacementObservation::Pending
        ));
        if !mirror_while_held {
            hold.release();
            wait_until(|| window.control_wait_observed_for_test());
            assert!(f.h.engine.resident_stem_cache.try_lock().is_ok());
        }
        assert!(pad_pair::control::reconcile(&f.h.engine, &pair).unwrap());
        let ack = pair.own_acknowledgement().unwrap();
        pad_pair::control::finish(&f.h.engine, &pair, &ack).unwrap();
        assert!(Arc::ptr_eq(
            &f.h.engine.cold_pcm_history.lock().unwrap()[1].identity,
            &original_row
        ));
        hold.release();
        wait_until(|| {
            window.publication_status() == "pending" || window.error().unwrap().is_some()
        });
        assert_eq!(
            window.publication_status(),
            "pending",
            "{:?}",
            window.error()
        );
        f.h.callback.drain(&mut f.h.consumer);
        assert_eq!(window.publication_status(), "accepted");
        reconcile(&f.h.engine).unwrap();
        let actual = f.h.callback.mixer.bank_for_measurement()[1]
            .as_ref()
            .unwrap();
        assert_eq!(
            actual.samples.as_ref(),
            &f.h.material.sample.samples[(LOOP_START - 1) * 2..(LOOP_END + 1) * 2]
        );
        let history = f.h.engine.cold_pcm_history.lock().unwrap();
        assert!(
            history[1]
                .iter()
                .any(|pcm| pcm.ptr_eq(&Arc::downgrade(&actual.samples)))
        );
        assert!(
            !history[0]
                .iter()
                .any(|pcm| pcm.ptr_eq(&Arc::downgrade(&actual.samples)))
        );
        drop(history);
        wait_until(|| f.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
        assert_eq!(
            f.h.engine
                .project_assets
                .resident_budget_status(&f.h.engine.resident_account)
                .unwrap()
                .pending_reservation_count,
            0
        );
    }
}

#[test]
fn complete_component_budget_refusal_precedes_source_allocation_and_unwinds() {
    let mut f = Productive::new(false, 1.0);
    let all = f.request("all_stems", 15);
    f.capture_then_wait(&all);
    f.h.callback.drain(&mut f.h.consumer);
    reconcile(&f.h.engine).unwrap();
    wait_until(|| f.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    let assets = &f.h.engine.project_assets;
    let account = &f.h.engine.resident_account;
    assets.set_resident_budget_mib(account, 128).unwrap();
    let before = assets.resident_budget_status(account).unwrap();
    let source_bytes = (LOOP_END - LOOP_START + 2) * 2 * 4;
    let promised = 128 * 1024 * 1024 - before.held_unique_pcm_bytes - source_bytes - 1;
    let blocker = assets.reserve_resident(account, promised, &[], 0).unwrap();
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let read = observed.clone();
    native_owned_work::set_preparation_observer(Some(Box::new(move |point| {
        if matches!(
            point,
            WorkPoint::WindowSourceRead | WorkPoint::WindowSourceChunk
        ) {
            read.store(true, std::sync::atomic::Ordering::Release);
        }
    })));
    let original = f.h.engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let window = fresh_window(&f.h);
    wait_until(|| window.error().unwrap().is_some());
    assert!(
        window
            .error()
            .unwrap()
            .unwrap()
            .contains("resident_pcm_budget_exceeded")
    );
    wait_until(|| f.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    assert!(!observed.load(std::sync::atomic::Ordering::Acquire));
    assert!(
        f.h.engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .same_window(&original)
    );
    let settled = assets.resident_budget_status(account).unwrap();
    assert_eq!(
        (
            settled.pending_resident_bytes,
            settled.pending_reservation_count
        ),
        (promised, 1)
    );
    drop(blocker);
    assert_eq!(
        assets
            .resident_budget_status(account)
            .unwrap()
            .pending_reservation_count,
        0
    );
}

#[test]
fn cancelled_real_source_reader_releases_promises_only_after_worker_return() {
    let f = Productive::new(false, 1.0);
    let hold = SourceHold::new(WorkPoint::WindowSourceChunk);
    let window = fresh_window(&f.h);
    hold.wait();
    assert!(window.cancel());
    assert_ne!(f.h.engine.cold_jobs.counts_for_test().0, 0);
    assert!(
        f.h.engine
            .project_assets
            .resident_budget_status(&f.h.engine.resident_account)
            .unwrap()
            .pending_reservation_count
            > 0
    );
    hold.release();
    wait_until(|| f.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    assert_eq!(window.publication_status(), "cancelled");
    assert!(f.h.consumer.is_empty());
    assert_eq!(
        f.h.engine
            .project_assets
            .resident_budget_status(&f.h.engine.resident_account)
            .unwrap()
            .pending_reservation_count,
        0
    );
}

#[test]
fn detached_registration_refuses_shutdown_and_discard_before_mutation() {
    use crate::audio_engine::resident_control_context::CommittedControlContext;
    for pending in [false, true] {
        for shutdown in [false, true] {
            let mut f = Productive::new(false, 1.0);
            let source = f.h.ticket(0);
            let context = CommittedControlContext::capture(&f.h.engine);
            let order = context
                .capture_committed_registration_order(&source)
                .unwrap();
            if shutdown {
                f.h.engine.shut_down().unwrap();
            } else {
                f.pair.discard().unwrap();
            }
            let result = if pending {
                context.register_pending_committed_stem_pair_with_producer(
                    &f.pair,
                    &source,
                    &f.h.producer,
                    order,
                )
            } else {
                context.register_committed_stem_pair_with_producer(
                    &f.pair,
                    &source,
                    &f.h.producer,
                    order,
                )
            };
            assert!(result.is_err());
            assert!(f.h.consumer.is_empty());
        }
    }
}

#[test]
fn copy_during_real_window_read_keeps_independent_history_and_original_custody() {
    use crate::audio_engine::{input_runtime_binding, pad_copy};
    let mut f = Productive::new(false, 1.0);
    let old = f.h.engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let row = f.h.engine.cold_pcm_history.lock().unwrap()[0]
        .identity
        .clone();
    let hold = SourceHold::new(WorkPoint::WindowSourceChunk);
    let window = fresh_window(&f.h);
    hold.wait();
    let metadata = f.h.material.metadata();
    let pair: serde_json::Value = serde_json::from_str(f.pair.selection_json()).unwrap();
    let envelope=serde_json::json!({"schema_version":1,"resolved_key_lock":false,"snapshot":{
        "source_path":metadata["new_reference"],
        "source_identity":{"instance_id":"88888888888888888888888888888888","material_id":metadata["material_id"],"material_creation":metadata["material_creation"],"copied_from":null},
        "duration":f.h.material.sample.frame_count() as f64 / f64::from(RATE),"analysis":null,
        "stem_cache":{"available":false,"pair":pair},"stem_mix_mode":"full_mix",
        "stem_intent":{"schema_version":1,"enabled_mask":15,"last_custom_mask":5,"display_mode":"all"},
        "key_lock":false,"manual_bpm":null,"timing_intent":"legacy","key_intent":null,
        "gain_db":0.0,"eq_low_db":0.0,"eq_mid_db":0.0,"eq_high_db":0.0,
        "loop_start_s":LOOP_START as f64 / f64::from(RATE),"loop_end_s":LOOP_END as f64 / f64::from(RATE),
        "loop_auto":false,"loop_bars":1.0,"grid_offset_samples":0,"grid_anchor_s":null
    }}).to_string();
    let binding = input_runtime_binding::capture(&f.h.engine, 0)
        .unwrap()
        .unwrap();
    let captured = pad_copy::capture(&f.h.engine, &binding, 1, &envelope, None).unwrap();
    let state = pad_copy::prepare(&f.h.engine, &captured).unwrap();
    let copy = pad_copy::prepare_detached(state).unwrap();
    pad_copy::publish(&f.h.engine, &copy).unwrap();
    f.h.callback.drain(&mut f.h.consumer);
    assert!(pad_copy::reconcile(&f.h.engine, &copy).unwrap());
    pad_copy::finish(&f.h.engine, &copy).unwrap();
    let target = f.h.callback.mixer.bank_for_measurement()[1]
        .clone()
        .unwrap();
    assert!(Arc::ptr_eq(&target.samples, &old.samples));
    assert!(!Arc::ptr_eq(
        &f.h.engine.cold_pcm_history.lock().unwrap()[1].identity,
        &row
    ));
    hold.release();
    wait_until(|| window.publication_status() == "pending" || window.error().unwrap().is_some());
    assert_eq!(
        window.publication_status(),
        "pending",
        "{:?}",
        window.error()
    );
    f.h.callback.drain(&mut f.h.consumer);
    assert_eq!(window.publication_status(), "accepted");
    reconcile(&f.h.engine).unwrap();
    let actual = f.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert_eq!(
        actual.samples.as_ref(),
        &f.h.material.sample.samples[(LOOP_START - 1) * 2..(LOOP_END + 1) * 2]
    );
    assert_eq!(
        f.h.callback.mixer.bank_for_measurement()[1]
            .as_ref()
            .unwrap()
            .samples
            .as_ref(),
        old.samples.as_ref()
    );
    let history = f.h.engine.cold_pcm_history.lock().unwrap();
    assert!(
        history[0]
            .iter()
            .any(|pcm| pcm.ptr_eq(&Arc::downgrade(&actual.samples)))
    );
    assert!(
        !history[1]
            .iter()
            .any(|pcm| pcm.ptr_eq(&Arc::downgrade(&actual.samples)))
    );
    drop(history);
    wait_until(|| f.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    assert_eq!(
        f.h.engine
            .project_assets
            .resident_budget_status(&f.h.engine.resident_account)
            .unwrap()
            .pending_reservation_count,
        0
    );
}

#[test]
fn registration_success_watermark_rejects_older_but_failed_newer_preserves_it() {
    use crate::audio_engine::resident_control_context::CommittedControlContext;
    for failed_newer in [false, true] {
        let mut f = Productive::new(false, 1.0);
        let source = f.h.ticket(0);
        let context = CommittedControlContext::capture(&f.h.engine);
        let older = context
            .capture_committed_registration_order(&source)
            .unwrap();
        let in_flight = if failed_newer {
            let ticket = f.request("all_stems", 15);
            f.capture_then_wait(&ticket);
            f.h.callback.drain(&mut f.h.consumer);
            assert_eq!(ticket.residency_status(), "accepted");
            assert_ne!(ticket.effective_mode_status(), "effective");
            Some(ticket)
        } else {
            None
        };
        let newer = context
            .capture_committed_registration_order(&source)
            .unwrap();
        let result = context.register_pending_committed_stem_pair_with_producer(
            &f.pair,
            &source,
            &f.h.producer,
            newer,
        );
        assert_eq!(result.is_err(), failed_newer);
        if failed_newer {
            assert!(
                result
                    .as_ref()
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("resident handover is in progress")
            );
        }
        let result = context.register_committed_stem_pair_with_producer(
            &f.pair,
            &source,
            &f.h.producer,
            older,
        );
        assert_eq!(result.is_ok(), failed_newer);
        if let Some(ticket) = in_flight {
            ticket.cancel();
            f.h.callback.drain(&mut f.h.consumer);
            wait_until(|| f.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
        }
    }
}

#[test]
fn public_preflight_retains_original_order_across_successful_or_failed_newer_request() {
    for failed_newer in [false, true] {
        let f = Productive::new(false, 1.0);
        let (engine, requester, selected, old_selected) = Python::attach(|py| {
            let engine = Py::new(py, f.h.engine).unwrap();
            let selected = Py::new(py, f.selected).unwrap();
            (
                engine.clone_ref(py),
                engine,
                selected.clone_ref(py),
                selected,
            )
        });
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let request = std::thread::spawn(move || {
            native_owned_work::set_observer(Some(Box::new(move |point| {
                if point == WorkPoint::CommittedStemPreflight {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                }
            })));
            let result = Python::attach(|py| {
                requester.call_method1(py, "request_committed_stem_residency", (old_selected,))
            })
            .map_err(|error| error.to_string());
            native_owned_work::set_observer(None);
            done_tx.send(result).unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let newer = Python::attach(|py| {
            let kwargs = PyDict::new(py);
            kwargs.set_item("mode", "full_mix").unwrap();
            if failed_newer {
                kwargs.set_item("enabled_mask", 16).unwrap();
            }
            engine
                .bind(py)
                .call_method(
                    "request_committed_stem_residency",
                    (selected,),
                    Some(&kwargs),
                )
                .map(|v| v.unbind())
                .map_err(|error| error.to_string())
        });
        assert_eq!(newer.is_err(), failed_newer);
        release_tx.send(()).unwrap();
        let older = done_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        request.join().unwrap();
        assert_eq!(older.is_ok(), failed_newer, "{older:?}");
        Python::attach(|py| {
            if let Ok(ticket) = older {
                ticket.call_method0(py, "cancel").unwrap();
            }
            if let Ok(ticket) = newer {
                ticket.call_method0(py, "cancel").unwrap();
            }
        });
        let mut callback = f.h.callback;
        let mut consumer = f.h.consumer;
        callback.drain(&mut consumer);
        wait_until(|| {
            Python::attach(|py| engine.borrow(py).cold_jobs.counts_for_test() == (0, 0, 0))
        });
    }
}

#[test]
fn legacy_complete_transient_is_promised_before_source_io_and_kept_until_return() {
    use crate::audio_engine::resident_host_memory::{HostHeadroom, pending_bytes};
    for refuse in [false, true] {
        let mut f = Productive::new(false, 1.0);
        let all = f.request("all_stems", 15);
        f.capture_then_wait(&all);
        f.h.callback.drain(&mut f.h.consumer);
        reconcile(&f.h.engine).unwrap();
        wait_until(|| f.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
        {
            let mut owners = f.h.engine.resident_stem_cache.lock().unwrap();
            // Reconstruct the supported legacy control row while retaining the real
            // set/PCM and Harness file custody. This probe cancels before conversion;
            // it grants no fabricated successful legacy publication.
            owners[0].committed = Default::default();
            owners[0].accepted.as_mut().unwrap().pair = None;
        }
        let transient = crate::audio_engine::stem_cache::admitted_stem_pcm_bytes(
            f.h.material.sample.frame_count(),
            2,
        )
        .unwrap();
        let source_bytes = (LOOP_END - LOOP_START + 2) * 2 * 4;
        let available = if refuse {
            (5 * source_bytes + 1) as u64
        } else {
            1024 * 1024 * 1024
        };
        f.h.engine
            .resident_account
            .set_host_memory_for_test(Ok(HostHeadroom {
                available_physical: 128 * 1024 * 1024 + available,
                caller_available_commit: 256 * 1024 * 1024 + available,
                system_available_commit: 256 * 1024 * 1024 + available,
            }));
        let baseline = pending_bytes().unwrap();
        let hold = (!refuse).then(|| SourceHold::new(WorkPoint::WindowSourceChunk));
        let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if refuse {
            let read = observed.clone();
            native_owned_work::set_preparation_observer(Some(Box::new(move |point| {
                if matches!(
                    point,
                    WorkPoint::WindowSourceRead | WorkPoint::WindowSourceChunk
                ) {
                    read.store(true, std::sync::atomic::Ordering::Release);
                }
            })));
        }
        let window = fresh_window(&f.h);
        if let Some(hold) = hold {
            hold.wait();
            assert!(
                pending_bytes().unwrap() >= baseline + transient as u64 + 5 * source_bytes as u64
            );
            window.cancel();
            assert!(pending_bytes().unwrap() >= baseline + transient as u64);
            hold.release();
        } else {
            wait_until(|| window.error().unwrap().is_some());
            assert!(
                window
                    .error()
                    .unwrap()
                    .unwrap()
                    .contains("resident_memory_pressure")
            );
            assert!(!observed.load(std::sync::atomic::Ordering::Acquire));
        }
        wait_until(|| f.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
        assert_eq!(pending_bytes().unwrap(), baseline);
        assert!(f.h.consumer.is_empty());
        assert_eq!(
            f.h.engine
                .project_assets
                .resident_budget_status(&f.h.engine.resident_account)
                .unwrap()
                .pending_reservation_count,
            0
        );
    }
}
