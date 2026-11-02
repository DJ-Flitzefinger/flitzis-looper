//! Whole-window cancellation retires unclaimed readers without waiting for a worker.
use super::*;
use crate::audio_engine::cold_jobs::{QUEUED_JOBS, WORKERS};
use std::sync::atomic::AtomicUsize;

fn queued_window(close: bool) {
    Python::initialize();
    let mut fixture = PairWorkerFixture::new();
    let engine = &mut fixture.source.fixture.engine;
    wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    // Finish this fixture's original real load delivery before measuring the
    // independent window request. Drop its Python/native event owners here too.
    let original_request = engine.pad_request_ids.lock().unwrap()[0];
    Python::attach(|py| {
        let mut successes = 0;
        while let Some(event) = engine.poll_loader_events(py).unwrap() {
            let dict = event.bind(py).cast::<pyo3::types::PyDict>().unwrap();
            let field = |key| dict.get_item(key).unwrap().unwrap();
            assert_eq!(field("id").extract::<usize>().unwrap(), 0);
            assert_eq!(
                field("request_id").extract::<u64>().unwrap(),
                original_request
            );
            let kind = field("type").extract::<String>().unwrap();
            assert!(matches!(kind.as_str(), "started" | "progress" | "success"));
            successes += usize::from(kind == "success");
        }
        assert_eq!(successes, 1);
    });
    let samples = Arc::downgrade(
        &engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples,
    );
    let owners_before = samples.strong_count();
    let held = BlockedWorkers::hold(engine);
    let ticket = resident_relocation::prepare_window_with_producer(
        engine,
        0,
        resident_relocation::WindowRequest {
            loop_region: Some((512.0 / 48_000.0, Some(1024.0 / 48_000.0))),
            ..resident_relocation::WindowRequest::default()
        },
        fixture.producer.clone(),
    )
    .unwrap();
    let captured_owners = samples.strong_count();
    let peer_runs = Arc::new(AtomicUsize::new(0));
    for _ in 0..QUEUED_JOBS - 1 {
        let ran = peer_runs.clone();
        engine
            .cold_jobs
            .submit(engine.cold_jobs.reserve().unwrap(), move || {
                ran.fetch_add(1, Ordering::AcqRel);
            })
            .unwrap();
    }
    let admitted_before = engine.cold_jobs.counts_for_test();
    if close {
        // This separate positive control exercises the original unrun JobGuard.
        engine.cold_jobs.close_admission();
    } else {
        assert!(ticket.cancel());
    }
    Python::attach(|py| assert!(engine.poll_loader_events(py).unwrap().is_none()));
    let after_pump = engine.cold_jobs.counts_for_test();
    let owners_after_pump = samples.strong_count();
    let status = ticket.publication_status();
    let current = ticket.is_current();
    let selected_current = fixture.selection.is_current();
    let original = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let window_unchanged = original.resident_start() == 0
        && original.resident_end() == 4096
        && original.same_source(fixture.mixer.bank_for_measurement()[0].as_ref().unwrap());
    drop(original);
    let no_publication = fixture.consumer.is_empty();
    let peers_before_release = peer_runs.load(Ordering::Acquire);
    let replacement_runs = Arc::new(AtomicUsize::new(0));
    let replacement_admitted = if close {
        false
    } else {
        let ran = replacement_runs.clone();
        match engine.cold_jobs.reserve() {
            Ok(reservation) => {
                engine
                    .cold_jobs
                    .submit(reservation, move || {
                        ran.fetch_add(1, Ordering::AcqRel);
                    })
                    .unwrap();
                true
            }
            Err(_) => false,
        }
    };
    eprintln!(
        "queued window close={close}: before={admitted_before:?} after={after_pump:?} \
         PCMowners={owners_before}/{captured_owners}/{owners_after_pump} \
         status={status} current={current} selected={selected_current} \
         original={window_unchanged} no_publication={no_publication} \
         peer_before={peers_before_release} replacement={replacement_admitted}"
    );
    // Join the actual workers before evaluating the pre-release observations.
    drop(held);
    wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    engine.cold_jobs.shutdown();
    // Other legitimate owners still hold this backing. Only these two actual
    // WindowWork readers must end; this does not certify its final physical reader.
    assert_eq!(captured_owners, owners_before + 2);
    assert_eq!(
        owners_after_pump, owners_before,
        "queued old/bank readers end"
    );
    assert_eq!(
        admitted_before,
        (WORKERS, QUEUED_JOBS, WORKERS + QUEUED_JOBS)
    );
    let peers = if close { 0 } else { QUEUED_JOBS - 1 };
    assert_eq!(after_pump, (WORKERS, peers, WORKERS + peers));
    assert_eq!(status, "cancelled");
    assert!(!current);
    assert!(selected_current);
    assert!(window_unchanged);
    assert!(no_publication);
    assert_eq!(peers_before_release, 0);
    assert_eq!(replacement_admitted, !close);
    assert_eq!(peer_runs.load(Ordering::Acquire), peers);
    assert_eq!(
        replacement_runs.load(Ordering::Acquire),
        usize::from(!close)
    );
}

#[test]
fn cancelled_unclaimed_window_releases_old_bank_readers_and_full_lane_capacity() {
    queued_window(false);
}

#[test]
fn uncancelled_close_retires_queued_window_readers_and_settles_original_guard() {
    queued_window(true);
}
