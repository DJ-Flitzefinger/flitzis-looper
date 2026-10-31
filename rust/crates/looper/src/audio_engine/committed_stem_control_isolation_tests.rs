//! Hold an actual positioned component read, without a device or model inference.
use super::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::audio_engine::stem_pair::ComponentReadHold as ReadHold;

fn shared_selections(h: &mut Harness) -> (PreparedStemPair, Vec<CommittedStemSetTicket>) {
    let pair = h.prepare(0, &h.ticket(0), false, None);
    let mut selected = Vec::new();
    for id in [0, 215] {
        let ticket = h
            .engine
            .register_committed_stem_pair_with_producer(&pair, &h.ticket(id), &h.producer)
            .unwrap();
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert_eq!(ticket.publication_status(), "accepted");
        selected.push(ticket);
    }
    (pair, selected)
}

#[test]
fn concurrent_components_share_backings_and_keep_independent_own_ack_and_reservations() {
    let mut h = long_harness();
    let (pair, selections) = shared_selections(&mut h);
    let hold = ReadHold::new(&pair.pair_reader);
    let first = h
        .engine
        .request_committed_stem_residency_with_interest_and_producer(
            &selections[0],
            "full_mix",
            15,
            "preload",
            &h.producer,
        )
        .unwrap();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    hold.wait();
    let second = h
        .engine
        .request_committed_stem_residency_with_interest_and_producer(
            &selections[1],
            "full_mix",
            15,
            "preload",
            &h.producer,
        )
        .unwrap();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    wait_until(|| h.engine.cold_jobs.counts_for_test().0 == 2);
    let reserved = h
        .engine
        .project_assets
        .resident_budget_status(&h.engine.resident_account)
        .unwrap();
    assert_eq!(
        reserved.pending_resident_bytes,
        h.material.sample.samples.len() * 16
    );
    assert_eq!(reserved.pending_reservation_count, 1);
    assert!(h.engine.resident_stem_cache.try_lock().is_ok());
    assert!(h.producer.try_lock().is_ok());
    h.engine
        .resident_account
        .set_host_memory_for_test(Err("controlled host query failure".into()));
    hold.release();
    wait_until(|| h.consumer.slots() >= 2 || first.error().is_some() || second.error().is_some());
    assert!(first.error().is_none(), "{:?}", first.error());
    assert!(second.error().is_none(), "{:?}", second.error());
    assert_eq!(h.callback.drain(&mut h.consumer), 2);
    assert_eq!(first.residency_status(), "accepted");
    assert_eq!(second.residency_status(), "accepted");
    assert!(first.is_current() && second.is_current());
    let sets = h.callback.mixer.stems_for_measurement();
    let a = sets[0].as_ref().unwrap();
    let b = sets[215].as_ref().unwrap();
    for (a, b) in a.stems.iter().zip(&b.stems) {
        assert!(Arc::ptr_eq(&a.samples, &b.samples));
    }
    assert!(!Arc::ptr_eq(&a.publication.epoch, &b.publication.epoch));
    assert!(a.publication.current() && b.publication.current());
    wait_until(|| h.engine.cold_jobs.counts_for_test().0 == 0);
    let settled = h
        .engine
        .project_assets
        .resident_budget_status(&h.engine.resident_account)
        .unwrap();
    assert_eq!(settled.pending_resident_bytes, 0);
    assert_eq!(settled.pending_reservation_count, 0);
    assert_eq!(
        settled.held_unique_pcm_bytes,
        reserved.held_unique_pcm_bytes + h.material.sample.samples.len() * 16
    );
}

#[test]
fn cancelled_component_gate_waiter_retires_before_slow_reader_returns() {
    let mut h = long_harness();
    let (pair, selections) = shared_selections(&mut h);
    let hold = ReadHold::new(&pair.pair_reader);
    let first = h
        .engine
        .request_committed_stem_residency_with_producer(
            &selections[0],
            "all_stems",
            15,
            &h.producer,
        )
        .unwrap();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    hold.wait();
    let second = h
        .engine
        .request_committed_stem_residency_with_producer(
            &selections[1],
            "all_stems",
            15,
            &h.producer,
        )
        .unwrap();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    wait_until(|| h.engine.cold_jobs.counts_for_test().0 == 2);
    assert!(second.cancel());
    wait_until(|| h.engine.cold_jobs.counts_for_test().0 == 1);
    assert_eq!(second.residency_status(), "rejected");
    assert!(h.consumer.is_empty());
    assert!(first.cancel());
    hold.release();
    wait_until(|| h.engine.cold_jobs.counts_for_test().0 == 0);
    assert_eq!(first.residency_status(), "rejected");
    assert!(h.consumer.is_empty());
    let settled = h
        .engine
        .project_assets
        .resident_budget_status(&h.engine.resident_account)
        .unwrap();
    assert_eq!(
        (
            settled.pending_resident_bytes,
            settled.pending_reservation_count
        ),
        (0, 0)
    );
}

#[test]
fn cancelled_or_stopped_component_read_never_publishes_and_keeps_full_mix_pcm() {
    for stop in [false, true] {
        let mut f = Productive::new(true, 0.8);
        let hold = ReadHold::new(&f.pair.pair_reader);
        let before = voice(&f.h.callback.mixer).stretch.native_state_address();
        let ticket = f.request("all_stems", 15);
        assert_eq!(f.h.callback.drain(&mut f.h.consumer), 1);
        hold.wait();
        if stop {
            f.h.engine.stop_sample(0).unwrap();
            assert_eq!(f.h.callback.drain(&mut f.h.consumer), 1);
        } else {
            assert!(ticket.cancel());
            f.render(257, false);
            assert_eq!(
                voice(&f.h.callback.mixer).stretch.native_state_address(),
                before
            );
        }
        hold.release();
        wait_until(|| f.h.engine.cold_jobs.counts_for_test().0 == 0);
        assert_eq!(ticket.residency_status(), "rejected");
        assert!(f.h.consumer.is_empty());
        assert!(f.h.callback.mixer.stems_for_measurement()[0].is_none());
        if stop {
            assert!(
                !f.h.callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(0))
            );
        } else {
            f.render(1024, false);
        }
        let settled =
            f.h.engine
                .project_assets
                .resident_budget_status(&f.h.engine.resident_account)
                .unwrap();
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
fn owned_admission_rechecks_source_before_mutating_or_enqueuing() {
    let mut h = long_harness();
    let (_pair, selected) = shared_selections(&mut h);
    let admission = h
        .engine
        .capture_committed_stem_residency(&selected[0], "all_stems", 15, "explicit", &h.producer)
        .unwrap();
    let epoch = h.engine.prepared_source_epoch(0).unwrap();
    epoch.fetch_add(1, Ordering::AcqRel);
    assert!(admission.admit().is_err());
    assert!(h.consumer.is_empty());
    assert_eq!(h.engine.cold_jobs.counts_for_test(), (0, 0, 0));
}

#[test]
fn owned_admission_cannot_revive_older_explicit_or_policy_intent() {
    for policy in [false, true] {
        for (newer_admitted_first, newer_acked_before_late) in
            [(false, false), (true, false), (true, true)]
        {
            let mut f = Productive::new(false, 1.0);
            let older =
                f.h.engine
                    .capture_committed_stem_residency(
                        &f.selected,
                        if policy { "full_mix" } else { "all_stems" },
                        15,
                        if policy { "preload" } else { "explicit" },
                        &f.h.producer,
                    )
                    .unwrap();
            let newer =
                f.h.engine
                    .capture_committed_stem_residency(
                        &f.selected,
                        "full_mix",
                        15,
                        if policy {
                            "withdraw_preload"
                        } else {
                            "explicit"
                        },
                        &f.h.producer,
                    )
                    .unwrap();
            let mut late_older = None;
            let mut predecessor = None;
            let receipt = if newer_admitted_first {
                let receipt = newer.admit().unwrap();
                if newer_acked_before_late {
                    late_older = Some(older);
                } else {
                    assert!(older.admit().is_err());
                }
                receipt
            } else {
                let prior = older.admit().unwrap();
                assert!(prior.is_current());
                predecessor = Some(prior);
                newer.admit().unwrap()
            };
            assert_eq!(receipt.residency_status(), "captured");
            assert_eq!(
                f.h.callback.drain(&mut f.h.consumer),
                if newer_admitted_first { 1 } else { 2 }
            );
            wait_until(|| f.h.consumer.peek().is_ok() || receipt.error().is_some());
            assert_eq!(f.h.callback.drain(&mut f.h.consumer), 1);
            assert_eq!(
                receipt.residency_status(),
                "accepted",
                "{:?}",
                receipt.error()
            );
            assert!(receipt.is_current());
            f.render(1024, false);
            if let Some(older) = late_older {
                assert!(older.admit().is_err());
            }
            assert!(receipt.is_current());
            assert_eq!(
                receipt.effective_mode_status(),
                if policy { "not_requested" } else { "effective" }
            );
            assert!(f.h.callback.mixer.stems_for_measurement()[0].is_none());
            wait_until(|| f.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
            if let Some(prior) = predecessor {
                assert_eq!(prior.residency_status(), "rejected");
            }
        }
    }
}

#[test]
fn queue_failed_newer_capture_keeps_older_context_admissible() {
    for policy in [false, true] {
        let mut f = Productive::new(false, 1.0);
        let older =
            f.h.engine
                .capture_committed_stem_residency(
                    &f.selected,
                    "full_mix",
                    15,
                    if policy {
                        "withdraw_preload"
                    } else {
                        "explicit"
                    },
                    &f.h.producer,
                )
                .unwrap();
        let newer =
            f.h.engine
                .capture_committed_stem_residency(
                    &f.selected,
                    if policy { "full_mix" } else { "all_stems" },
                    15,
                    if policy { "preload" } else { "explicit" },
                    &f.h.producer,
                )
                .unwrap();
        {
            let mut producer = f.h.producer.lock().unwrap();
            while !producer.is_full() {
                producer.push(ControlMessage::Ping()).unwrap();
            }
        }
        assert!(newer.admit().is_err());
        f.h.callback.drain(&mut f.h.consumer);
        let receipt = older.admit().unwrap();
        assert_eq!(f.h.callback.drain(&mut f.h.consumer), 1);
        wait_until(|| f.h.consumer.peek().is_ok() || receipt.error().is_some());
        assert_eq!(f.h.callback.drain(&mut f.h.consumer), 1);
        assert_eq!(
            receipt.residency_status(),
            "accepted",
            "{:?}",
            receipt.error()
        );
        assert!(receipt.is_current());
        f.render(1024, false);
        wait_until(|| f.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    }
}

#[test]
fn queue_failed_newer_capture_keeps_admitted_predecessor_current() {
    let mut f = Productive::new(false, 1.0);
    let prior = f.request("full_mix", 15);
    let newer = f
        .h
        .engine
        .capture_committed_stem_residency(&f.selected, "all_stems", 15, "explicit", &f.h.producer)
        .unwrap();
    {
        let mut producer = f.h.producer.lock().unwrap();
        while !producer.is_full() {
            producer.push(ControlMessage::Ping()).unwrap();
        }
    }
    assert!(newer.admit().is_err());
    assert!(prior.is_current());
    assert_eq!(prior.residency_status(), "captured");
    f.h.callback.drain(&mut f.h.consumer);
    wait_until(|| f.h.consumer.peek().is_ok() || prior.error().is_some());
    assert_eq!(f.h.callback.drain(&mut f.h.consumer), 1);
    assert_eq!(prior.residency_status(), "accepted", "{:?}", prior.error());
    f.render(1024, false);
    assert!(prior.is_current());
    assert_eq!(prior.effective_mode_status(), "effective");
    wait_until(|| f.h.engine.cold_jobs.counts_for_test() == (0, 0, 0));
}

#[test]
fn public_owned_admission_wait_releases_python_and_mutable_engine_borrow() {
    use crate::audio_engine::native_owned_work::{WorkPoint, set_observer};
    let mut f = Productive::new(false, 1.0);
    let (engine, request_engine, control_engine, selection) = Python::attach(|py| {
        let engine = Py::new(py, f.h.engine).unwrap();
        let selection = Py::new(py, f.selected.clone()).unwrap();
        (
            engine.clone_ref(py),
            engine.clone_ref(py),
            engine,
            selection,
        )
    });
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let request = std::thread::spawn(move || {
        set_observer(Some(Box::new(move |point| {
            if point == WorkPoint::CommittedStemAdmission {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            }
        })));
        let result = Python::attach(|py| {
            request_engine.call_method1(py, "request_committed_stem_residency", (selection,))
        })
        .map_err(|error| error.to_string());
        set_observer(None);
        done_tx.send(result).unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (control_tx, control_rx) = mpsc::channel();
    let control = std::thread::spawn(move || {
        let result = Python::attach(|py| {
            control_engine.call_method0(py, "ping")?;
            assert!(control_engine.call_method0(py, "receive_msg")?.is_none(py));
            Ok::<_, PyErr>(())
        })
        .map_err(|error| error.to_string());
        control_tx.send(result).unwrap();
    });
    let progress = control_rx.recv_timeout(Duration::from_millis(500));
    let before_release = progress.is_ok();
    release_tx.send(()).unwrap();
    progress
        .unwrap_or_else(|_| control_rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .unwrap();
    let receipt = done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    request.join().unwrap();
    control.join().unwrap();
    Python::attach(|py| receipt.call_method0(py, "cancel").unwrap());
    f.h.callback.drain(&mut f.h.consumer);
    wait_until(|| Python::attach(|py| engine.borrow(py).cold_jobs.counts_for_test() == (0, 0, 0)));
    assert!(
        before_release,
        "native admission retained Python or the Engine borrow"
    );
    Python::attach(|_| {
        drop(receipt);
        drop(engine);
    });
}

#[test]
#[ignore = "explicit paired control-stall diagnostic; no device or inference"]
fn controlled_component_read_control_progress() {
    let mut f = Productive::new(false, 1.0);
    let hold = ReadHold::new(&f.pair.pair_reader);
    let ticket = f
        .h
        .engine
        .request_committed_stem_residency_with_producer(&f.selected, "all_stems", 15, &f.h.producer)
        .unwrap();
    assert_eq!(f.h.callback.drain(&mut f.h.consumer), 1);
    hold.wait();
    let owners_free = f.h.engine.resident_stem_cache.try_lock().is_ok();
    let engine = &f.h.engine;
    let selected = &f.selected;
    let producer = &f.h.producer;
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let before = Instant::now();
    let (completed_while_held, elapsed, newer) = std::thread::scope(|scope| {
        scope.spawn(move || {
            started_tx.send(()).unwrap();
            let result = engine
                .request_committed_stem_residency_with_producer(selected, "full_mix", 15, producer);
            done_tx.send(result).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let result = done_rx.recv_timeout(Duration::from_millis(200));
        let completed = result.is_ok();
        let elapsed = before.elapsed();
        if !completed {
            hold.release();
        }
        let newer = result
            .unwrap_or_else(|_| done_rx.recv_timeout(Duration::from_secs(5)).unwrap())
            .unwrap();
        assert_eq!(newer.residency_status(), "captured");
        (completed, elapsed, newer)
    });
    f.h.callback.drain(&mut f.h.consumer);
    wait_until(|| f.h.consumer.peek().is_ok() || newer.error().is_some());
    f.h.callback.drain(&mut f.h.consumer);
    assert_eq!(newer.residency_status(), "accepted", "{:?}", newer.error());
    assert!(newer.is_current());
    f.render(1024, false);
    assert_eq!(newer.effective_mode_status(), "effective");
    hold.release();
    wait_until(|| f.h.engine.cold_jobs.counts_for_test().0 == 0);
    assert_eq!(ticket.residency_status(), "rejected");
    assert!(f.h.consumer.is_empty());
    assert!(f.h.callback.mixer.stems_for_measurement()[0].is_none());
    let baseline = std::env::var_os("FLITZIS_STEM_CONTROL_BASELINE").is_some();
    eprintln!(
        "stem-control owners_free={owners_free} admission_while_read={completed_while_held} observation_us={} baseline={baseline}",
        elapsed.as_micros()
    );
    assert_eq!(owners_free, !baseline);
    assert_eq!(completed_while_held, !baseline);
}

#[test]
#[ignore = "explicit paired public Python control-stall diagnostic"]
fn controlled_component_read_public_python_progress() {
    let mut f = Productive::new(false, 1.0);
    // Match the real stereo callback; the device-free public fallback is mono.
    f.h.engine.test_output_format = Some((RATE, 2));
    let hold = ReadHold::new(&f.pair.pair_reader);
    let ticket = f.request("all_stems", 15);
    assert_eq!(f.h.callback.drain(&mut f.h.consumer), 1);
    hold.wait();
    let (engine, request_engine, control_engine, selection) = Python::attach(|py| {
        let engine = Py::new(py, f.h.engine).unwrap();
        let selection = Py::new(py, f.selected.clone()).unwrap();
        (
            engine.clone_ref(py),
            engine.clone_ref(py),
            engine,
            selection,
        )
    });
    let (started_tx, started_rx) = mpsc::channel();
    let (requested_tx, requested_rx) = mpsc::channel();
    let request = std::thread::spawn(move || {
        let result = Python::attach(|py| {
            let kwargs = PyDict::new(py);
            kwargs.set_item("mode", "full_mix").unwrap();
            started_tx.send(()).unwrap();
            request_engine
                .bind(py)
                .call_method(
                    "request_committed_stem_residency",
                    (selection,),
                    Some(&kwargs),
                )
                .map(|value| value.unbind())
                .map_err(|error| error.to_string())
        });
        requested_tx.send(result).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    // Baseline admission monopolizes Python while waiting for the read's owners.
    let request_before = requested_rx.recv_timeout(Duration::from_millis(200));
    let (control_tx, control_rx) = mpsc::channel();
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
        control_tx.send(result).unwrap();
    });
    let control_before = control_rx.recv_timeout(Duration::from_millis(200));
    let request_progress = request_before.is_ok();
    let python_progress = control_before.is_ok();
    let baseline = std::env::var_os("FLITZIS_STEM_CONTROL_BASELINE").is_some();
    eprintln!(
        "stem-public request_while_read={request_progress} python_control_while_read={python_progress} baseline={baseline}"
    );
    let mut effect_controls = |start: &crate::audio_engine::pad_start::CapturedPadStart| {
        f.h.callback.drain(&mut f.h.consumer);
        let callback = &mut f.h.callback;
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
        assert_eq!(
            start.effective_frame(),
            Some(callback.transport.output_frame())
        );
        assert!(
            callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(215))
        );
        Python::attach(|py| engine.call_method1(py, "stop_sample", (215,)).unwrap());
        callback.drain(&mut f.h.consumer);
        assert!(
            !callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(215))
        );
    };
    if python_progress {
        let (admitted, start) = control_before.as_ref().unwrap().as_ref().unwrap();
        assert!(*admitted);
        effect_controls(start);
    }
    hold.release();
    let requested = request_before
        .unwrap_or_else(|_| requested_rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .unwrap();
    let (admitted, start) = control_before
        .unwrap_or_else(|_| control_rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .unwrap();
    assert!(admitted);
    request.join().unwrap();
    control.join().unwrap();
    if !python_progress {
        effect_controls(&start);
    }
    assert_eq!(request_progress, !baseline);
    assert_eq!(python_progress, !baseline);
    Python::attach(|py| requested.call_method0(py, "cancel").unwrap());
    f.h.callback.drain(&mut f.h.consumer);
    wait_until(|| Python::attach(|py| engine.borrow(py).cold_jobs.counts_for_test() == (0, 0, 0)));
    Python::attach(|_| {
        drop(requested);
        drop(engine);
    });
    drop(ticket);
}
