//! Productive Native analysis ownership through own Pair ACK and complete control settlement.
use super::*;
use crate::audio_engine::analysis_jobs::OfflineRequestOwner;
use crate::audio_engine::complete_context::source_custody_tests::own_pair_ack;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::pad_pair;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyDictMethods};
use serde_json::{Value, json};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn source(fixture: &LaunchFixture) -> CompleteSourceReader {
    let sample = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    CompleteSourceReader::capture(&fixture.engine, 0, sample).unwrap()
}

#[test]
fn actual_manual_analysis_waits_without_blocking_cold_and_retires_before_next_job() {
    let fixture = LaunchFixture::new(4);
    let samples = fixture.engine.sample_cache.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .samples
        .clone();
    let baseline = Arc::strong_count(&samples);
    let blocker = super::super::heavy_work::ticket().unwrap();
    assert!(blocker.try_acquire().unwrap());
    let (entered_tx, entered_rx) = mpsc::channel();
    let cancelled = fixture.engine.cold_cancelled.clone();
    super::start_analysis_with(&fixture.engine, 0, move |_| {
        assert!(super::super::heavy_work::current().is_some());
        // A real captured request reached its worker, but no materialization,
        // frontend or model is needed to verify the admission/lifetime boundary.
        cancelled.store(true, Ordering::Release);
        entered_tx.send(()).unwrap();
    })
    .unwrap();
    let successor = super::super::heavy_work::ticket().unwrap();
    let (ordinary_tx, ordinary_rx) = mpsc::channel();
    fixture
        .engine
        .cold_jobs
        .submit(fixture.engine.cold_jobs.reserve().unwrap(), move || {
            ordinary_tx.send(()).unwrap();
        })
        .unwrap();
    ordinary_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(entered_rx.try_recv().is_err());
    assert!(!successor.try_acquire().unwrap());
    drop(blocker);
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !successor.try_acquire().unwrap() {
        assert!(
            Instant::now() < deadline,
            "analysis retained whole-job admission"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (0, 0, 0));
    assert_eq!(
        Arc::strong_count(&samples),
        baseline,
        "captured PCM reader survived release"
    );
}

enum QueuedAnalysisCancellation {
    Request,
    Source,
    Shutdown,
}

fn observe_cancelled_queued_analysis(kind: QueuedAnalysisCancellation, occupy_workers: bool) {
    let fixture = LaunchFixture::new(4);
    let samples = fixture.engine.sample_cache.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .samples
        .clone();
    let baseline = Arc::strong_count(&samples);
    let blocker = super::super::heavy_work::ticket().unwrap();
    assert!(blocker.try_acquire().unwrap());
    let (entered, started) = mpsc::channel();
    start_analysis_with(&fixture.engine, 0, move |_| {
        let _ = entered.send(());
    })
    .unwrap();
    let row = (
        fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap()
            .cell()
            .index(),
        BackgroundTaskKind::Analysis,
    );
    let token = fixture.engine.analysis_task_tokens.lock().unwrap()[&row].clone();
    let (ordinary, ran) = mpsc::channel();
    fixture
        .engine
        .cold_jobs
        .submit(fixture.engine.cold_jobs.reserve().unwrap(), move || {
            ordinary.send(()).unwrap();
        })
        .unwrap();
    let ordinary_ran = ran.recv_timeout(Duration::from_secs(5)).is_ok();
    let mut releases = Vec::new();
    if occupy_workers {
        for _ in 0..super::super::cold_jobs::WORKERS {
            let (entered, started) = mpsc::channel();
            let (release, resumed) = mpsc::channel();
            fixture
                .engine
                .cold_jobs
                .submit(fixture.engine.cold_jobs.reserve().unwrap(), move || {
                    entered.send(()).unwrap();
                    let _ = resumed.recv_timeout(Duration::from_secs(10));
                })
                .unwrap();
            started.recv_timeout(Duration::from_secs(5)).unwrap();
            releases.push(release);
        }
    }
    match kind {
        QueuedAnalysisCancellation::Request => {
            super::super::next_pad_request_id(
                &fixture.engine.pad_request_ids,
                0,
                fixture.engine.prepared_source_epoch(0).unwrap(),
            )
            .unwrap();
        }
        QueuedAnalysisCancellation::Source => {
            fixture.engine.input_runtime_ownership.revoke_source(0);
        }
        QueuedAnalysisCancellation::Shutdown => {
            fixture.engine.cold_cancelled.store(true, Ordering::Release);
        }
    }
    let active = if occupy_workers {
        super::super::cold_jobs::WORKERS
    } else {
        0
    };
    let deadline = Instant::now() + Duration::from_millis(500);
    let retired = loop {
        // Exercise the actual non-realtime control pump, including with both
        // existing cold workers occupied by independent ordinary work.
        if occupy_workers {
            Python::attach(|py| {
                fixture.engine.poll_loader_events(py).unwrap();
            });
        }
        if Arc::strong_count(&samples) == baseline
            && !fixture.engine.active_tasks.lock().unwrap().contains(&row)
            && !fixture
                .engine
                .analysis_task_tokens
                .lock()
                .unwrap()
                .contains_key(&row)
            && Arc::strong_count(&token) == 1
            && fixture.engine.cold_jobs.counts_for_test() == (active, 0, active)
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let counts = fixture.engine.cold_jobs.counts_for_test();
    let payload_started = started.try_recv().is_ok();
    let token_owners = Arc::strong_count(&token);
    let blocker_still_active = blocker.is_active();

    // Record the failed boundary before cleanup; no red assertion strands an
    // actual global FIFO owner or leaves the fixture's worker payload running.
    fixture.engine.cold_cancelled.store(true, Ordering::Release);
    for release in releases {
        let _ = release.send(());
    }
    drop(blocker);
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.engine.cold_jobs.counts_for_test() != (0, 0, 0) {
        assert!(
            Instant::now() < deadline,
            "fixture analysis cleanup did not retire"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        ordinary_ran,
        "queued analysis blocked an ordinary cold peer"
    );
    assert!(blocker_still_active);
    assert!(
        !payload_started,
        "cancelled queued payload entered heavy work"
    );
    assert!(
        retired,
        "cancelled queued analysis retained PCM/task owners: {counts:?}, token owners {token_owners}"
    );
    assert_eq!(token_owners, 1);
    assert_eq!(counts, (active, 0, active));
}

#[test]
fn queued_analysis_request_cancellation_retires_before_heavy_predecessor() {
    observe_cancelled_queued_analysis(QueuedAnalysisCancellation::Request, false);
}

#[test]
fn queued_analysis_source_cancellation_retires_before_heavy_predecessor() {
    observe_cancelled_queued_analysis(QueuedAnalysisCancellation::Source, false);
}

#[test]
fn queued_analysis_shutdown_retires_before_heavy_predecessor() {
    observe_cancelled_queued_analysis(QueuedAnalysisCancellation::Shutdown, false);
}

#[test]
fn queued_analysis_control_poll_retires_with_both_workers_occupied() {
    observe_cancelled_queued_analysis(QueuedAnalysisCancellation::Request, true);
}

fn owner(fixture: &LaunchFixture) -> NativeAnalysisOwner {
    let _requests = fixture.engine.pad_request_ids.lock().unwrap();
    NativeAnalysisOwner::new(
        source(fixture).guard.unwrap(),
        fixture.engine.prepared_source_epoch(0).unwrap().clone(),
    )
}

fn event_id(py: pyo3::Python<'_>, event: &pyo3::Py<pyo3::PyAny>) -> usize {
    event
        .bind(py)
        .cast::<PyDict>()
        .unwrap()
        .get_item("id")
        .unwrap()
        .unwrap()
        .extract()
        .unwrap()
}

#[test]
fn owned_loader_payload_waits_for_full_pair_mirror_then_delivers_once_in_order() {
    let mut fixture = LaunchFixture::new(4);
    let owner = owner(&fixture);
    let first = owner.event(LoaderEvent::TaskProgress {
        id: 0,
        request_id: 7,
        task: BackgroundTaskKind::Analysis,
        percent: 0.5,
        stage: "original owned payload".into(),
    });
    let original_payload = match &first {
        LoaderEvent::NativeOwned { event, .. } => std::ptr::from_ref(event.as_ref()) as usize,
        _ => unreachable!(),
    };
    fixture.engine.loader_tx.send(first).unwrap();
    fixture
        .engine
        .loader_tx
        .send(owner.event(LoaderEvent::TaskError {
            id: 0,
            request_id: 7,
            task: BackgroundTaskKind::Analysis,
            error: "second exact owner".into(),
        }))
        .unwrap();
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let ticket = own_pair_ack(&mut fixture, &mut mixer);
    assert!(!fixture.engine.input_runtime_ownership.placement_pending());
    assert!(matches!(
        owner.observe_control(),
        PlacementObservation::Pending
    ));
    pyo3::Python::attach(|py| {
        assert!(fixture.engine.poll_loader_events(py).unwrap().is_none());
        let pending = fixture.engine.pending_loader_event.lock().unwrap();
        assert!(
            matches!(pending.as_ref(), Some(LoaderEvent::NativeOwned { event, .. })
            if std::ptr::from_ref(event.as_ref()) as usize == original_payload)
        );
        drop(pending);
        assert!(fixture.engine.poll_loader_events(py).unwrap().is_none());
        assert!(pad_pair::reconcile(&fixture.engine, &ticket).unwrap());
        // Unchanged numeric destination request is 8; the original request is 7.
        assert_eq!(fixture.engine.pad_request_ids.lock().unwrap()[1], 8);
        let first = fixture.engine.poll_loader_events(py).unwrap().unwrap();
        assert_eq!(event_id(py, &first), 1);
        assert_eq!(
            first
                .bind(py)
                .cast::<PyDict>()
                .unwrap()
                .get_item("stage")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "original owned payload"
        );
        let second = fixture.engine.poll_loader_events(py).unwrap().unwrap();
        assert_eq!(event_id(py, &second), 1);
        assert_eq!(
            second
                .bind(py)
                .cast::<PyDict>()
                .unwrap()
                .get_item("msg")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "second exact owner"
        );
        assert!(fixture.engine.poll_loader_events(py).unwrap().is_none());
    });
}

#[test]
fn native_event_rejects_revoked_owner_even_with_equal_pcm_generation_and_request() {
    let fixture = LaunchFixture::new(4);
    let owner = owner(&fixture);
    let sample = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    fixture
        .engine
        .loader_tx
        .send(owner.event(LoaderEvent::TaskError {
            id: 0,
            request_id: 7,
            task: BackgroundTaskKind::Analysis,
            error: "must never reach replacement".into(),
        }))
        .unwrap();
    fixture.engine.input_runtime_ownership.revoke_source(0);
    fixture
        .engine
        .input_runtime_ownership
        .publish_test_source(0, &sample, 48_000, 7);
    assert!(!owner.current());
    pyo3::Python::attach(|py| {
        assert!(fixture.engine.poll_loader_events(py).unwrap().is_none());
    });
}

#[test]
fn native_event_rejects_foreign_live_owner_with_equal_loaded_metadata() {
    let first = LaunchFixture::new(4);
    let second = LaunchFixture::new(4);
    let owner = owner(&first);
    assert!(owner.current());
    second
        .engine
        .loader_tx
        .send(owner.event(LoaderEvent::TaskError {
            id: 0,
            request_id: 7,
            task: BackgroundTaskKind::Analysis,
            error: "foreign current owner".into(),
        }))
        .unwrap();
    pyo3::Python::attach(|py| {
        assert!(second.engine.poll_loader_events(py).unwrap().is_none());
    });
}

#[test]
fn actual_analysis_admission_keeps_its_range_after_settings_change() {
    let fixture = LaunchFixture::new(4);
    fixture
        .engine
        .configure_automatic_bpm_range(true, 50.0, 170.0)
        .unwrap();
    let (entered, started) = mpsc::channel();
    let (release, resumed) = mpsc::channel();
    start_analysis_with(&fixture.engine, 0, move |snapshot| {
        entered.send(snapshot).unwrap();
        resumed.recv_timeout(Duration::from_secs(5)).unwrap();
    })
    .unwrap();
    fixture
        .engine
        .configure_automatic_bpm_range(false, 30.0, 300.0)
        .unwrap();
    let snapshot = started
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.minimum_bpm, 50.0);
    assert_eq!(snapshot.maximum_bpm, 170.0);
    assert!(fixture.engine.automatic_bpm_range.lock().unwrap().is_none());
    // No detector is needed to prove the admitted snapshot. Retire this actual
    // worker through the original whole-request cancellation boundary.
    fixture.engine.cold_cancelled.store(true, Ordering::Release);
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.engine.cold_jobs.counts_for_test() != (0, 0, 0) {
        assert!(
            Instant::now() < deadline,
            "range snapshot worker did not retire"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn actual_admitted_analysis_worker_follows_pair_and_ignores_vacated_slot_request() {
    let mut fixture = LaunchFixture::new(4);
    let (entered, started) = mpsc::channel();
    let (release, resumed) = mpsc::channel();
    let request = start_analysis_with(&fixture.engine, 0, move |_| {
        entered.send(()).unwrap();
        resumed.recv_timeout(Duration::from_secs(5)).unwrap();
    })
    .unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let ticket = own_pair_ack(&mut fixture, &mut mixer);
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    pyo3::Python::attach(|py| {
        loop {
            assert!(fixture.engine.poll_loader_events(py).unwrap().is_none());
            if fixture
                .engine
                .pending_loader_event
                .lock()
                .unwrap()
                .is_some()
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "actual worker did not enqueue its owning event"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(pad_pair::reconcile(&fixture.engine, &ticket).unwrap());
        super::super::next_pad_request_id(
            &fixture.engine.pad_request_ids,
            0,
            fixture.engine.prepared_source_epoch(0).unwrap(),
        )
        .unwrap();
        let mut terminal = false;
        while !terminal {
            if let Some(event) = fixture.engine.poll_loader_events(py).unwrap() {
                let dict = event.bind(py).cast::<PyDict>().unwrap();
                assert_eq!(event_id(py, &event), 1);
                assert_eq!(
                    dict.get_item("request_id")
                        .unwrap()
                        .unwrap()
                        .extract::<u64>()
                        .unwrap(),
                    request
                );
                let kind = dict
                    .get_item("type")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap();
                terminal = matches!(kind.as_str(), "task_success" | "task_error");
                if kind == "task_error" {
                    assert!(
                        !dict
                            .get_item("msg")
                            .unwrap()
                            .unwrap()
                            .extract::<String>()
                            .unwrap()
                            .contains("superseded")
                    );
                }
            } else {
                assert!(Instant::now() < deadline, "actual worker did not settle");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    });
}

#[test]
fn old_analysis_worker_retirement_preserves_blocked_successor_after_actual_unload() {
    let fixture = LaunchFixture::new(4);
    let original = fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap();
    let row = (original.cell().index(), BackgroundTaskKind::Analysis);
    let (old_entered, old_started) = mpsc::channel();
    let (old_release, old_resumed) = mpsc::channel();
    start_analysis_with(&fixture.engine, 0, move |_| {
        old_entered.send(()).unwrap();
        old_resumed.recv_timeout(Duration::from_secs(5)).unwrap();
    })
    .unwrap();
    old_started.recv_timeout(Duration::from_secs(5)).unwrap();
    let original_token = fixture
        .engine
        .analysis_task_tokens
        .lock()
        .unwrap()
        .get(&row)
        .cloned()
        .unwrap();
    assert!(fixture.engine.active_tasks.lock().unwrap().contains(&row));

    crate::audio_engine::cold_load::unload_for_producer(&fixture.engine, 0, &fixture.producer)
        .unwrap();
    assert!(!fixture.engine.active_tasks.lock().unwrap().contains(&row));
    fixture.publish(0, 0.75, 17);
    let successor = fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap();
    assert_eq!(successor.cell(), original.cell());
    assert_ne!(successor.lifetime(), original.lifetime());
    let (new_entered, new_started) = mpsc::channel();
    let (new_release, new_resumed) = mpsc::channel();
    start_analysis_with(&fixture.engine, 0, move |_| {
        new_entered.send(()).unwrap();
        new_resumed.recv_timeout(Duration::from_secs(5)).unwrap();
    })
    .unwrap();
    // The successor already owns its new task token, but its payload must
    // remain queued until the old physical reader and heavy owner retire.
    assert!(new_started.try_recv().is_err());
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (1, 1, 2));
    let successor_token = fixture
        .engine
        .analysis_task_tokens
        .lock()
        .unwrap()
        .get(&row)
        .cloned()
        .unwrap();
    assert!(!Arc::ptr_eq(&original_token, &successor_token));

    old_release.send(()).unwrap();
    new_started.recv_timeout(Duration::from_secs(5)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    // The actual pool retires its admission only after the old closure and its
    // PadTaskGuard have dropped. The sole remaining worker is the blocked successor.
    while fixture.engine.cold_jobs.counts_for_test() != (1, 0, 1) {
        assert!(
            Instant::now() < deadline,
            "old worker did not actually retire"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(fixture.engine.active_tasks.lock().unwrap().contains(&row));
    assert!(
        fixture
            .engine
            .analysis_task_tokens
            .lock()
            .unwrap()
            .get(&row)
            .is_some_and(|token| Arc::ptr_eq(token, &successor_token))
    );

    new_release.send(()).unwrap();
    while fixture.engine.cold_jobs.counts_for_test() != (0, 0, 0) {
        assert!(
            Instant::now() < deadline,
            "successor worker did not actually retire"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!fixture.engine.active_tasks.lock().unwrap().contains(&row));
    assert!(
        !fixture
            .engine
            .analysis_task_tokens
            .lock()
            .unwrap()
            .contains_key(&row)
    );
}

#[test]
fn offline_native_job_exports_and_publishes_original_diagnostic_after_pair() {
    let mut fixture = LaunchFixture::new(4);
    fixture.publish(36, 0.75, 9);
    let job = fixture
        .engine
        .offline_jobs
        .begin_complete(
            0,
            source(&fixture),
            48_000,
            7,
            OfflineRequestOwner {
                request_ids: fixture.engine.pad_request_ids.clone(),
                prepared_epoch: fixture.engine.prepared_source_epoch(0).unwrap().clone(),
            },
            fixture.engine.loader_tx.clone(),
        )
        .unwrap();
    let identity = pyo3::Python::attach(|py| {
        let metadata = job.metadata(py).unwrap();
        let dict = metadata.bind(py).cast::<PyDict>().unwrap();
        json!({"pad_id":0,
            "request_id":dict.get_item("request_id").unwrap().unwrap().extract::<u64>().unwrap(),
            "source_id":dict.get_item("source_id").unwrap().unwrap().extract::<String>().unwrap(),
            "source_generation":7})
    });
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let ticket = own_pair_ack(&mut fixture, &mut mixer);
    assert!(fixture.engine.offline_jobs.has_pad(0));
    assert!(fixture.engine.offline_jobs.has_pad(1));
    assert!(!fixture.engine.offline_jobs.has_pad(36));
    let unrelated = fixture
        .engine
        .offline_jobs
        .with_pad_idle(36, |idle| {
            assert!(idle);
            let sample = fixture.engine.sample_cache.lock().unwrap()[36]
                .clone()
                .unwrap();
            CompleteSourceReader::capture(&fixture.engine, 36, sample)
                .map_err(PyRuntimeError::new_err)
        })
        .unwrap();
    assert!(unrelated.guard.as_ref().unwrap().current());
    assert!(pad_pair::reconcile(&fixture.engine, &ticket).unwrap());
    assert!(!fixture.engine.offline_jobs.has_pad(0));
    assert!(fixture.engine.offline_jobs.has_pad(1));
    super::super::next_pad_request_id(
        &fixture.engine.pad_request_ids,
        0,
        fixture.engine.prepared_source_epoch(0).unwrap(),
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("owned.f32");
    pyo3::Python::attach(|py| {
        job.prepare_export(py, path.to_string_lossy().into())
            .unwrap();
        let result = json!({"schema_version":1, "identity":identity,
            "beat":{"status":"unavailable"}, "key":{"status":"unavailable"}})
        .to_string();
        assert!(job.finish(py, result).unwrap());
        let mut completed = None;
        while let Some(event) = fixture.engine.poll_loader_events(py).unwrap() {
            let dict = event.bind(py).cast::<PyDict>().unwrap();
            assert_eq!(event_id(py, &event), 1);
            if dict
                .get_item("type")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap()
                == "offline_analysis_completed"
            {
                completed = Some(
                    dict.get_item("result_json")
                        .unwrap()
                        .unwrap()
                        .extract::<String>()
                        .unwrap(),
                );
            }
        }
        let parsed: Value = serde_json::from_str(&completed.unwrap()).unwrap();
        assert_eq!(parsed["identity"], identity);
        assert_eq!(parsed["identity"]["pad_id"], 0);
    });
    assert_eq!(std::fs::metadata(path).unwrap().len(), 128 * 4);
    assert!(!fixture.engine.offline_jobs.has_pad(1));
}

#[test]
fn new_request_on_moved_content_rejects_queued_native_completion() {
    let mut fixture = LaunchFixture::new(4);
    let owner = owner(&fixture);
    fixture
        .engine
        .loader_tx
        .send(owner.event(LoaderEvent::TaskError {
            id: 0,
            request_id: 7,
            task: BackgroundTaskKind::Analysis,
            error: "obsolete own request".into(),
        }))
        .unwrap();
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let ticket = own_pair_ack(&mut fixture, &mut mixer);
    assert!(pad_pair::reconcile(&fixture.engine, &ticket).unwrap());
    super::super::next_pad_request_id(
        &fixture.engine.pad_request_ids,
        1,
        fixture.engine.prepared_source_epoch(1).unwrap(),
    )
    .unwrap();
    assert!(!owner.current());
    pyo3::Python::attach(|py| {
        assert!(fixture.engine.poll_loader_events(py).unwrap().is_none());
    });
}

#[test]
fn pending_offline_cancellation_targets_exact_native_and_control_custody_only() {
    for cancel_slot in [0, 1] {
        let mut fixture = LaunchFixture::new(4);
        fixture.publish(36, 0.75, 9);
        let other_sample = fixture.engine.sample_cache.lock().unwrap()[1]
            .clone()
            .unwrap();
        let other_guard = CompleteSourceReader::capture(&fixture.engine, 1, other_sample)
            .unwrap()
            .guard
            .unwrap();
        let job = fixture
            .engine
            .offline_jobs
            .begin_complete(
                0,
                source(&fixture),
                48_000,
                7,
                OfflineRequestOwner {
                    request_ids: fixture.engine.pad_request_ids.clone(),
                    prepared_epoch: fixture.engine.prepared_source_epoch(0).unwrap().clone(),
                },
                fixture.engine.loader_tx.clone(),
            )
            .unwrap();
        let request = pyo3::Python::attach(|py| {
            job.metadata(py)
                .unwrap()
                .bind(py)
                .cast::<PyDict>()
                .unwrap()
                .get_item("request_id")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap()
        });
        let mut mixer = fixture.mixer();
        mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
        let ticket = own_pair_ack(&mut fixture, &mut mixer);
        let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
        assert!(!job.is_cancelled());
        fixture.engine.offline_jobs.cancel(Some(36));
        assert!(!job.is_cancelled());
        // Either own custody address reaches the same original job. A Pending
        // cancellation sets its flag and never advances another current occupant.
        fixture.engine.offline_jobs.cancel(Some(cancel_slot));
        assert!(job.is_cancelled());
        assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
        assert!(other_guard.current());
        assert!(pad_pair::reconcile(&fixture.engine, &ticket).unwrap());
        assert!(fixture.engine.offline_jobs.has_pad(1)); // exact source remains retiring
        assert!(!fixture.engine.offline_jobs.has_pad(0));
        let result = json!({"schema_version":1,
            "identity":{"pad_id":0,"request_id":request,"source_id":"loaded-0-7","source_generation":7},
            "beat":{"status":"cancelled"},"key":{"status":"cancelled"}}).to_string();
        pyo3::Python::attach(|py| {
            assert!(!job.finish(py, result).unwrap());
            while let Some(event) = fixture.engine.poll_loader_events(py).unwrap() {
                let kind = event
                    .bind(py)
                    .cast::<PyDict>()
                    .unwrap()
                    .get_item("type")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap();
                assert_ne!(kind, "offline_analysis_completed");
            }
        });
        assert!(!fixture.engine.offline_jobs.has_pad(1));
        assert!(other_guard.current());
    }
}
