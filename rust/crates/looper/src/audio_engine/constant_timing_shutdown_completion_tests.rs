//! Detached timing work must not acquire publication after its stream has shut down.

use super::*;
use std::sync::atomic::AtomicU64;

struct ShutdownSnapshot {
    epoch: Arc<AtomicU64>,
    epoch_value: u64,
    request: u64,
    key: ContentKey,
    request_incarnation: Option<u64>,
    current_producer: Option<Arc<Mutex<Producer<ControlMessage>>>>,
    replacement_consumer: Option<rtrb::Consumer<ControlMessage>>,
}

fn shut_down_while_unpublished(
    engine: &Py<AudioEngine>,
    callback: &mut Callback,
    replace_stream: bool,
) -> ShutdownSnapshot {
    assert!(
        callback.consumer.peek().is_err(),
        "the held worker has not acquired queue admission before shutdown"
    );
    Python::attach(|py| {
        let original_producer = engine.borrow(py).control_producer().unwrap().clone();
        let key = engine
            .borrow(py)
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap()
            .content_key()
            .unwrap();
        engine.call_method0(py, "shut_down").unwrap();
        let mut native = engine.borrow_mut(py);
        // Production shutdown removes stream_handle and therefore its current
        // producer. The device-free facade substitutes this field for that handle.
        // Retain the callback receiver only to detect any late old-queue enqueue.
        native.test_control_channels = None;
        assert!(native.control_producer().is_err());
        let replacement_consumer = if replace_stream {
            let (producer, consumer) = tests::queue(16);
            assert!(!Arc::ptr_eq(&original_producer, &producer));
            let (_, feedback) = rtrb::RingBuffer::<AudioMessage>::new(1);
            native.test_control_channels = Some((producer, Arc::new(Mutex::new(feedback))));
            Some(consumer)
        } else {
            None
        };
        assert!(
            native
                .current_constant_timing
                .lock()
                .unwrap()
                .iter()
                .all(Vec::is_empty)
        );
        let epoch = native.prepared_source_epochs[key.cell().index()].clone();
        let request = native.pad_request_ids.lock().unwrap()[0];
        let request_incarnation = native
            .input_runtime_ownership
            .key_source_request_incarnation(key);
        let current_producer = native.control_producer().ok().cloned();
        ShutdownSnapshot {
            epoch_value: epoch.load(Ordering::Acquire),
            epoch,
            request,
            key,
            request_incarnation,
            current_producer,
            replacement_consumer,
        }
    })
}

fn assert_completion_did_not_publish(
    engine: &Py<AudioEngine>,
    callback: &mut Callback,
    snapshot: &mut ShutdownSnapshot,
) {
    assert!(
        callback.consumer.peek().is_err(),
        "a completion first admitted after shutdown reached the old stream queue"
    );
    if let Some(consumer) = snapshot.replacement_consumer.as_mut() {
        assert!(
            consumer.peek().is_err(),
            "old detached work cannot recapture the replacement stream queue"
        );
    }
    Python::attach(|py| {
        let native = engine.borrow(py);
        match snapshot.current_producer.as_ref() {
            Some(producer) => assert!(Arc::ptr_eq(native.control_producer().unwrap(), producer)),
            None => assert!(native.control_producer().is_err()),
        }
        assert_eq!(snapshot.epoch.load(Ordering::Acquire), snapshot.epoch_value);
        assert_eq!(native.pad_request_ids.lock().unwrap()[0], snapshot.request);
        assert_eq!(
            native
                .input_runtime_ownership
                .key_source_request_incarnation(snapshot.key),
            snapshot.request_incarnation
        );
        assert!(
            native
                .current_constant_timing
                .lock()
                .unwrap()
                .iter()
                .all(Vec::is_empty),
            "shutdown-cleared timing records must not gain a new pending publication"
        );
    });
}

fn accepted_timing_completion_loses_stream(replace_stream: bool) {
    let (_directory, engine, mut callback, _path, _envelope) = engine();
    let ticket = Python::attach(|py| {
        engine
            .call_method1(py, "prepare_constant_timing", prepare_arguments(py))
            .unwrap()
            .extract::<Py<ConstantTimingTicket>>(py)
            .unwrap()
    });
    let arguments = Python::attach(|py| publication_arguments(py, &ticket));
    let mut shutdown = None;
    let result = held_call(
        &engine,
        &mut callback,
        "publish_constant_timing",
        arguments,
        WorkPoint::AcceptedTiming,
        |engine, callback| {
            Python::attach(|py| {
                assert_eq!(ticket.borrow(py).publication_status().unwrap(), "captured");
            });
            shutdown = Some(shut_down_while_unpublished(
                engine,
                callback,
                replace_stream,
            ));
        },
    );
    assert_completion_did_not_publish(&engine, &mut callback, &mut shutdown.unwrap());
    assert!(
        result.is_err(),
        "work held before admission must reject after shutdown, rather than report publication"
    );
    Python::attach(|py| {
        let ticket = ticket.borrow(py);
        assert!(ticket.publication.lock().unwrap().is_none());
        assert!(ticket.guard.lock().unwrap().accepted().is_none());
    });
}

fn saved_timing_restore_completion_loses_stream(replace_stream: bool) {
    let (_directory, engine, mut callback, path, envelope) = engine();
    let arguments = Python::attach(|py| {
        let saved = engine
            .call_method1(py, "capture_saved_constant_timing", (0, envelope, path))
            .unwrap();
        (saved,).into_pyobject(py).unwrap().unbind()
    });
    let mut shutdown = None;
    let result = held_call(
        &engine,
        &mut callback,
        "restore_constant_timing",
        arguments,
        WorkPoint::SavedTimingSource,
        |engine, callback| {
            shutdown = Some(shut_down_while_unpublished(
                engine,
                callback,
                replace_stream,
            ));
        },
    );
    assert_completion_did_not_publish(&engine, &mut callback, &mut shutdown.unwrap());
    assert!(
        result.is_err(),
        "a restore held before admission must reject after shutdown, rather than return a pending ticket"
    );
}

#[test]
fn public_accepted_timing_completion_after_shutdown_cannot_first_publish() {
    accepted_timing_completion_loses_stream(false);
}

#[test]
fn public_accepted_timing_completion_cannot_publish_to_old_or_replacement_stream() {
    accepted_timing_completion_loses_stream(true);
}

#[test]
fn public_saved_timing_restore_completion_after_shutdown_cannot_first_publish() {
    saved_timing_restore_completion_loses_stream(false);
}

#[test]
fn public_saved_timing_restore_completion_cannot_publish_to_old_or_replacement_stream() {
    saved_timing_restore_completion_loses_stream(true);
}

#[test]
fn public_accepted_timing_completion_keeps_actual_unchanged_stream_admission() {
    let (_directory, engine, mut callback, _path, _envelope) = engine();
    let ticket = Python::attach(|py| {
        engine
            .call_method1(py, "prepare_constant_timing", prepare_arguments(py))
            .unwrap()
            .extract::<Py<ConstantTimingTicket>>(py)
            .unwrap()
    });
    let producer = Python::attach(|py| engine.borrow(py).control_producer().unwrap().clone());
    let arguments = Python::attach(|py| publication_arguments(py, &ticket));
    held_call(
        &engine,
        &mut callback,
        "publish_constant_timing",
        arguments,
        WorkPoint::AcceptedTiming,
        |engine, callback| {
            assert!(callback.consumer.peek().is_err());
            Python::attach(|py| {
                assert!(Arc::ptr_eq(
                    engine.borrow(py).control_producer().unwrap(),
                    &producer
                ));
            });
        },
    )
    .unwrap();
    acknowledge(&engine, &mut callback, &ticket);
}

#[test]
fn public_saved_timing_restore_completion_keeps_actual_unchanged_stream_admission() {
    let (_directory, engine, mut callback, path, envelope) = engine();
    let producer = Python::attach(|py| engine.borrow(py).control_producer().unwrap().clone());
    let arguments = Python::attach(|py| {
        let saved = engine
            .call_method1(py, "capture_saved_constant_timing", (0, envelope, path))
            .unwrap();
        (saved,).into_pyobject(py).unwrap().unbind()
    });
    let result = held_call(
        &engine,
        &mut callback,
        "restore_constant_timing",
        arguments,
        WorkPoint::SavedTimingSource,
        |engine, callback| {
            assert!(callback.consumer.peek().is_err());
            Python::attach(|py| {
                assert!(Arc::ptr_eq(
                    engine.borrow(py).control_producer().unwrap(),
                    &producer
                ));
            });
        },
    )
    .unwrap();
    let ticket = Python::attach(|py| result.extract::<Py<ConstantTimingTicket>>(py).unwrap());
    acknowledge(&engine, &mut callback, &ticket);
}
