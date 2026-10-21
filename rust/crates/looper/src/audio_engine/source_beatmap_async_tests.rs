// Actual existing-lane admission, retirement and exact immutable DTO ownership.

use super::*;
use crate::audio_engine::loader_events::LoaderEventDelivery;
use std::sync::Condvar;

struct OccupiedWorkers {
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl OccupiedWorkers {
    fn release(&self) {
        *self
            .gate
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.gate.1.notify_all();
    }
}

impl Drop for OccupiedWorkers {
    fn drop(&mut self) {
        // A failed assertion must not leave fixture shutdown joining blocked workers.
        self.release();
    }
}

fn occupy_workers(fixture: &ColdCustody) -> OccupiedWorkers {
    wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    // Arm panic cleanup before even the first real worker can block.
    let held = OccupiedWorkers { gate: gate.clone() };
    let (entered, started) = mpsc::channel();
    for _ in 0..2 {
        let gate = gate.clone();
        let entered = entered.clone();
        fixture
            .engine
            .cold_jobs
            .submit(fixture.engine.cold_jobs.reserve().unwrap(), move || {
                entered.send(()).unwrap();
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            })
            .unwrap();
    }
    for _ in 0..2 {
        started.recv_timeout(Duration::from_secs(10)).unwrap();
    }
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (2, 0, 2));
    held
}

fn queue_while_workers_held(
    fixture: &ColdCustody,
    held: &OccupiedWorkers,
    encoded: String,
) -> OfflineAnalysisJob {
    std::thread::scope(|scope| {
        let engine = &fixture.engine;
        let (returned, receipt) = mpsc::channel();
        scope.spawn(move || {
            let _ = returned.send(engine.begin_source_beatmap_async(0, encoded));
        });
        match receipt.recv_timeout(Duration::from_secs(10)) {
            Ok(job) => job.unwrap(),
            Err(error) => {
                // Let a wrongly blocking admission exit before scoped joining.
                held.release();
                panic!("map admission waited for an occupied cold worker: {error}");
            }
        }
    })
}

fn async_status(job: &OfflineAnalysisJob) -> Value {
    Python::attach(|py| {
        let status = job.source_beatmap_async_status(py).unwrap().unwrap();
        let encoded: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (status,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    })
}

fn wait_terminal(job: &OfflineAnalysisJob) -> Value {
    wait_until(|| async_status(job)["terminal"] == true);
    async_status(job)
}

fn delivery(event: &Py<PyAny>) -> Py<LoaderEventDelivery> {
    Python::attach(|py| {
        event
            .bind(py)
            .cast::<PyDict>()
            .unwrap()
            .get_item("_native_event_owner")
            .unwrap()
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn resolved_record(job: &OfflineAnalysisJob, owner: &Py<LoaderEventDelivery>) -> Option<Value> {
    Python::attach(|py| {
        job.resolve_source_beatmap_completion(&owner.borrow(py))
            .unwrap()
            .map(|map| {
                let object = Py::new(py, map).unwrap();
                let encoded: String = object
                    .bind(py)
                    .call_method0("record_json")
                    .unwrap()
                    .extract()
                    .unwrap();
                serde_json::from_str(&encoded).unwrap()
            })
    })
}

fn assert_offline_busy(fixture: &ColdCustody) {
    assert!(
        fixture.engine.begin_source_beatmap(1).is_err(),
        "queued/running original map still owns the shared offline reservation"
    );
}

fn assert_offline_available(fixture: &ColdCustody) {
    let next = fixture.engine.begin_source_beatmap(1).unwrap();
    next.abort_unstarted().unwrap();
    wait_until(|| !fixture.engine.offline_jobs.has_pad(1));
}

#[test]
fn async_map_really_queues_and_cancel_or_drop_keeps_shared_capacity_until_worker_retirement() {
    for cancel in [false, true] {
        let fixture = ColdCustody::new_selected(None);
        assert!(maps(&fixture).is_empty());
        let expected = record(&fixture, 0);
        let fences = source_fences(&fixture);
        let original = fixture.engine.cold_leases.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .original_path
            .clone();
        let held = occupy_workers(&fixture);
        let job = queue_while_workers_held(&fixture, &held, expected.to_string());
        assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (2, 1, 3));
        let status = async_status(&job);
        assert_eq!(status["stage"], "queued");
        assert_eq!(status["terminal"], false);
        assert_eq!(status["cancellation_requested"], false);
        assert_eq!(source_fences(&fixture), fences);
        // An original progress event has the same admission, but no private map.
        Python::attach(|py| {
            let event = fixture.engine.poll_loader_events(py).unwrap().unwrap();
            let dict = event.bind(py).cast::<PyDict>().unwrap();
            assert_ne!(
                dict.get_item("type")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "source_beatmap_completed"
            );
            let owner = delivery(&event);
            assert!(
                job.resolve_source_beatmap_completion(&owner.borrow(py))
                    .unwrap()
                    .is_none()
            );
        });
        if cancel {
            job.cancel();
            let status = async_status(&job);
            assert_eq!(status["cancellation_requested"], true);
            assert_eq!(
                status["terminal"], false,
                "a cancellation request is not actual queued retirement"
            );
            assert_offline_busy(&fixture);
            assert!(fs::OpenOptions::new().write(true).open(&original).is_err());
            held.release();
            assert_eq!(wait_terminal(&job)["stage"], "cancelled");
        } else {
            drop(job);
            assert_offline_busy(&fixture);
            assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (2, 1, 3));
            assert!(fs::OpenOptions::new().write(true).open(&original).is_err());
            held.release();
        }
        wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
        assert_offline_available(&fixture);
        assert!(maps(&fixture).is_empty());
        assert_eq!(source_fences(&fixture), fences);
    }
}

#[test]
fn full_or_stopped_actual_lane_refuses_before_offline_admission_or_source_changes() {
    for stopped in [false, true] {
        let fixture = ColdCustody::new();
        assert!(maps(&fixture).is_empty());
        let expected = record(&fixture, 0);
        let fences = source_fences(&fixture);
        let held = occupy_workers(&fixture);
        if stopped {
            fixture.engine.cold_jobs.close_admission();
        } else {
            for _ in 0..32 {
                fixture
                    .engine
                    .cold_jobs
                    .submit(fixture.engine.cold_jobs.reserve().unwrap(), || {})
                    .unwrap();
            }
            assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (2, 32, 34));
        }
        let refused = fixture
            .engine
            .begin_source_beatmap_async(0, expected.to_string());
        assert!(refused.is_err());
        let detail = refused.err().unwrap().to_string();
        assert!(
            detail.contains(if stopped { "stopped" } else { "queue full" }),
            "{detail}"
        );
        assert_eq!(source_fences(&fixture), fences);
        assert_offline_available(&fixture);
        assert!(maps(&fixture).is_empty());
        held.release();
        wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    }
}

#[test]
fn discarding_actual_queued_closure_retires_readers_and_reservation_with_handle_alive() {
    let fixture = ColdCustody::new_selected(None);
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let fences = source_fences(&fixture);
    let readers_retired = fixture.engine.cold_leases.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .reader_lifetime_probe_for_test();
    let assets = fixture.engine.project_assets.clone();
    let held = occupy_workers(&fixture);
    let job = fixture
        .engine
        .begin_source_beatmap_async(0, expected.to_string())
        .unwrap();
    assert_offline_busy(&fixture);
    assert_eq!(async_status(&job)["terminal"], false);
    fixture.engine.cold_jobs.close_admission();
    let terminal = wait_terminal(&job);
    assert!(matches!(
        terminal["stage"].as_str(),
        Some("cancelled" | "failed")
    ));
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (2, 0, 2));
    assert_offline_available(&fixture);
    assert!(maps(&fixture).is_empty());
    assert_eq!(source_fences(&fixture), fences);
    held.release();
    wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    let ColdCustody {
        engine,
        directory,
        producer,
        consumer,
        mixer,
    } = fixture;
    drop(engine);
    drop(producer);
    drop(consumer);
    drop(mixer);
    // Actual sealed readers must retire even while the terminal handle survives.
    wait_until(|| {
        assets.collect_for_test();
        readers_retired()
    });
    assert_eq!(async_status(&job)["terminal"], true);
    drop(directory);
}

#[test]
fn queued_source_supersession_is_terminal_without_delivering_a_map() {
    let fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let held = occupy_workers(&fixture);
    let job = fixture
        .engine
        .begin_source_beatmap_async(0, expected.to_string())
        .unwrap();
    crate::audio_engine::next_pad_request_id(
        &fixture.engine.pad_request_ids,
        0,
        fixture.engine.prepared_source_epoch(0).unwrap(),
    )
    .unwrap();
    assert_offline_busy(&fixture);
    held.release();
    let terminal = wait_terminal(&job);
    assert!(matches!(
        terminal["stage"].as_str(),
        Some("cancelled" | "failed")
    ));
    wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    assert!(maps(&fixture).is_empty());
    assert_offline_available(&fixture);
}

#[test]
fn active_verified_async_cancellation_or_drop_waits_for_real_worker_return() {
    for cancel in [false, true] {
        let fixture = ColdCustody::new();
        assert!(maps(&fixture).is_empty());
        let expected = record(&fixture, 0);
        let fences = source_fences(&fixture);
        let (entered, started) = mpsc::channel();
        let held = OccupiedWorkers {
            gate: Arc::new((Mutex::new(false), Condvar::new())),
        };
        let gate = held.gate.clone();
        let job = fixture
            .engine
            .begin_source_beatmap_async_with(0, expected.to_string(), move || {
                entered.send(()).unwrap();
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            })
            .unwrap();
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(async_status(&job)["stage"], "running");
        wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (1, 0, 1));
        if cancel {
            fixture.engine.cold_jobs.close_admission();
            job.cancel();
            assert_eq!(async_status(&job)["terminal"], false);
            assert_eq!(async_status(&job)["cancellation_requested"], true);
            assert_offline_busy(&fixture);
            assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (1, 0, 1));
            assert_eq!(source_fences(&fixture), fences);
            held.release();
            assert_eq!(wait_terminal(&job)["stage"], "cancelled");
        } else {
            drop(job);
            assert_offline_busy(&fixture);
            assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (1, 0, 1));
            assert_eq!(source_fences(&fixture), fences);
            held.release();
        }
        wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
        assert_offline_available(&fixture);
        assert!(maps(&fixture).is_empty());
        assert_eq!(source_fences(&fixture), fences);
    }
}

#[test]
fn actual_engine_shutdown_cancels_active_verified_work_only_after_real_retirement() {
    let mut fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let shutdown_requested = fixture.engine.cold_cancelled.clone();
    assert!(!shutdown_requested.load(Ordering::Acquire));

    let job = std::thread::scope(|scope| {
        // This guard MUST live inside the scope: unwind releases the actual
        // verification gate before scope joins a child blocked in shut_down().
        let held = OccupiedWorkers {
            gate: Arc::new((Mutex::new(false), Condvar::new())),
        };
        let gate = held.gate.clone();
        let (entered, started) = mpsc::channel();
        let job = fixture
            .engine
            .begin_source_beatmap_async_with(0, expected.to_string(), move || {
                entered.send(()).unwrap();
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            })
            .unwrap();
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(async_status(&job)["stage"], "running");
        let (finished, receipt) = mpsc::channel();
        let engine = &mut fixture.engine;
        scope.spawn(move || {
            let _ = finished.send(engine.shut_down());
        });
        wait_until(|| shutdown_requested.load(Ordering::Acquire));
        let status = async_status(&job);
        assert_eq!(status["cancellation_requested"], true);
        assert_eq!(status["terminal"], false);
        assert_eq!(status["stage"], "running");
        assert!(
            matches!(receipt.try_recv(), Err(mpsc::TryRecvError::Empty)),
            "actual shutdown must still await the original blocked worker"
        );
        held.release();
        receipt
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
        job
    });

    let terminal = wait_terminal(&job);
    assert_eq!(terminal["stage"], "cancelled");
    assert_eq!(terminal["cancellation_requested"], true);
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (0, 0, 0));
    assert!(!fixture.engine.offline_jobs.has_pad(0));
    assert!(maps(&fixture).is_empty());
}

#[test]
fn actual_verified_worker_failure_retires_once_and_does_not_poison_the_existing_lane() {
    let fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let job = fixture
        .engine
        .begin_source_beatmap_async_with(0, expected.to_string(), || {
            panic!("injected failure after actual source/map verification");
        })
        .unwrap();
    let terminal = wait_terminal(&job);
    assert_eq!(terminal["stage"], "failed");
    assert!(
        terminal["detail"]
            .as_str()
            .is_some_and(|detail| !detail.is_empty() && detail.chars().count() <= 256)
    );
    wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    assert_offline_available(&fixture);
    assert!(maps(&fixture).is_empty());
    let next = fixture
        .engine
        .begin_source_beatmap_async(0, expected.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&next)["stage"], "ready");
    assert_eq!(maps(&fixture).len(), 1);
}

#[test]
fn sequential_equal_request_and_revision_maps_resolve_only_their_own_admissions() {
    let fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let fences = source_fences(&fixture);
    let first = fixture
        .engine
        .begin_source_beatmap_async(0, expected.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&first)["stage"], "ready");
    let first_events = maps(&fixture);
    assert_eq!(first_events.len(), 1);
    let first_owner = delivery(&first_events[0]);
    let second = fixture
        .engine
        .begin_source_beatmap_async(0, expected.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&second)["stage"], "ready");
    let second_events = maps(&fixture);
    assert_eq!(second_events.len(), 1);
    let second_owner = delivery(&second_events[0]);
    assert_eq!(
        metadata(&first)["request_id"],
        metadata(&second)["request_id"]
    );
    assert_eq!(
        resolved_record(&first, &first_owner),
        Some(expected.clone())
    );
    assert_eq!(
        resolved_record(&second, &second_owner),
        Some(expected.clone())
    );
    assert_eq!(resolved_record(&first, &second_owner), None);
    assert_eq!(resolved_record(&second, &first_owner), None);
    first.cancel();
    assert_eq!(
        resolved_record(&first, &first_owner),
        None,
        "cancellation after Python polling revokes the retained private DTO"
    );
    assert_eq!(resolved_record(&second, &second_owner), Some(expected));
    assert_eq!(source_fences(&fixture), fences);
}

#[test]
fn mutable_python_event_substitution_cannot_change_the_private_original_map() {
    let fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let mut different = expected.clone();
    different["record"]["accepted"]["anchors"][1]["beat_bits"] = json!(bits(2.0));
    seal(&mut different);
    let first = fixture
        .engine
        .begin_source_beatmap_async(0, expected.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&first)["stage"], "ready");
    let first_event = maps(&fixture).pop().unwrap();
    let second = fixture
        .engine
        .begin_source_beatmap_async(0, different.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&second)["stage"], "ready");
    let second_event = maps(&fixture).pop().unwrap();
    let first_owner = delivery(&first_event);
    let second_owner = delivery(&second_event);
    Python::attach(|py| {
        let forged = first_event.bind(py).cast::<PyDict>().unwrap();
        let other = second_event.bind(py).cast::<PyDict>().unwrap();
        forged
            .set_item("map", other.get_item("map").unwrap().unwrap())
            .unwrap();
        forged.set_item("id", 215).unwrap();
        forged.set_item("request_id", 0).unwrap();
    });
    assert_eq!(resolved_record(&first, &first_owner), Some(expected));
    assert_eq!(resolved_record(&second, &second_owner), Some(different));
    assert_eq!(resolved_record(&first, &second_owner), None);
    assert_eq!(resolved_record(&second, &first_owner), None);
}

#[test]
fn private_completion_waits_for_real_pair_mirror_without_bypassing_durable_settlement() {
    let mut fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let job = fixture
        .engine
        .begin_source_beatmap_async(0, expected.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    let event = maps(&fixture).pop().unwrap();
    let owner = delivery(&event);
    let ticket = own_pair(&mut fixture, 1);
    Python::attach(|py| {
        let error = job
            .resolve_source_beatmap_completion(&owner.borrow(py))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("loader event content placement pending")
        );
    });
    assert!(pair::reconcile(&fixture.engine, &ticket).unwrap());
    Python::attach(|py| {
        assert_eq!(
            fixture
                .engine
                ._resolve_loader_event(&owner.borrow(py))
                .unwrap(),
            Some((1, false))
        );
    });
    assert_eq!(resolved_record(&job, &owner), Some(expected.clone()));
    crate::audio_engine::next_pad_request_id(
        &fixture.engine.pad_request_ids,
        0,
        fixture.engine.prepared_source_epoch(0).unwrap(),
    )
    .unwrap();
    assert_eq!(resolved_record(&job, &owner), Some(expected.clone()));
    // This Native fixture proves ACK/mirror delivery, not the outer Config
    // transaction. Its original participant fence must still guard later edits.
    let refusal =
        crate::audio_engine::cold_load::unload_for_producer(&fixture.engine, 1, &fixture.producer)
            .unwrap_err();
    assert!(refusal.to_string().contains("pad content pair is pending"));
    assert_eq!(resolved_record(&job, &owner), Some(expected));
}

#[test]
fn private_completion_rejects_actual_equal_byte_source_replacement() {
    let mut fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let job = fixture
        .engine
        .begin_source_beatmap_async(0, expected.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    let event = maps(&fixture).pop().unwrap();
    let owner = delivery(&event);
    assert_eq!(resolved_record(&job, &owner), Some(expected.clone()));
    crate::audio_engine::cold_load::unload_for_producer(&fixture.engine, 0, &fixture.producer)
        .unwrap();
    fixture.dispatch_one();
    fixture.load_and_ack_selected(
        0,
        Some(ResidentLoadHint {
            start_s: 32.0 / 48_000.0,
            end_s: 64.0 / 48_000.0,
            key_lock: false,
        }),
    );
    assert_eq!(
        manifest(&fixture, 0)["decoder_identity"],
        expected["record"]["source"]["decoder_identity"]
    );
    assert_eq!(resolved_record(&job, &owner), None);
}

#[test]
fn asynchronous_owner_refuses_direct_prepare_finish_or_abort_without_losing_queued_work() {
    let fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let direct = fixture.engine.begin_source_beatmap(0).unwrap();
    Python::attach(|py| assert!(direct.source_beatmap_async_status(py).unwrap().is_none()));
    direct.abort_unstarted().unwrap();
    wait_until(|| !fixture.engine.offline_jobs.has_pad(0));

    let held = occupy_workers(&fixture);
    let job = fixture
        .engine
        .begin_source_beatmap_async(0, expected.to_string())
        .unwrap();
    Python::attach(|py| {
        assert!(
            job.prepare_source_beatmap(py, expected.to_string())
                .is_err()
        );
        assert!(job.finish_source_beatmap(py).is_err());
    });
    assert!(job.abort_unstarted().is_err());
    assert_eq!(async_status(&job)["stage"], "queued");
    assert_eq!(async_status(&job)["terminal"], false);
    assert_offline_busy(&fixture);
    held.release();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    let event = maps(&fixture).pop().unwrap();
    assert_eq!(resolved_record(&job, &delivery(&event)), Some(expected));
}

#[test]
fn naturally_dropping_a_finished_async_handle_preserves_its_genuine_queued_completion() {
    let fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let job = fixture
        .engine
        .begin_source_beatmap_async(0, expected.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    let request = metadata(&job)["request_id"].as_u64().unwrap();
    drop(job);
    let events = maps(&fixture);
    assert_eq!(events.len(), 1);
    assert_event(&events[0], 0, request, &expected);
    let owner = delivery(&events[0]);
    Python::attach(|py| {
        assert_eq!(
            fixture
                .engine
                ._resolve_loader_event(&owner.borrow(py))
                .unwrap(),
            Some((0, false))
        );
    });
}

#[test]
fn a_genuine_completion_from_another_engine_never_authorizes_this_job_or_source() {
    let original = ColdCustody::new();
    let other = ColdCustody::new();
    assert!(maps(&original).is_empty());
    assert!(maps(&other).is_empty());
    let first_record = record(&original, 0);
    let other_record = record(&other, 0);
    let first = original
        .engine
        .begin_source_beatmap_async(0, first_record.to_string())
        .unwrap();
    let second = other
        .engine
        .begin_source_beatmap_async(0, other_record.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&first)["stage"], "ready");
    assert_eq!(wait_terminal(&second)["stage"], "ready");
    let first_owner = delivery(&maps(&original).pop().unwrap());
    let other_owner = delivery(&maps(&other).pop().unwrap());
    assert_eq!(resolved_record(&first, &other_owner), None);
    assert_eq!(resolved_record(&second, &first_owner), None);
    assert_eq!(resolved_record(&first, &first_owner), Some(first_record));
    assert_eq!(resolved_record(&second, &other_owner), Some(other_record));
    Python::attach(|py| {
        assert_eq!(
            original
                .engine
                ._resolve_loader_event(&other_owner.borrow(py))
                .unwrap(),
            None
        );
        assert_eq!(
            other
                .engine
                ._resolve_loader_event(&first_owner.borrow(py))
                .unwrap(),
            None
        );
    });
}
