//! Actual bounded waveform work follows its captured source through an own Pair mirror.

use super::*;
use crate::audio_engine::complete_context::source_custody_tests::own_pair_ack;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::pad_pair;
use pyo3::Python;
use pyo3::prelude::PyAnyMethods;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const PUBLIC_FRAMES: usize = 5_000;

/// Hold the actual workers without owning any queued request or PCM witness.
struct HeldWorkers(Vec<mpsc::Sender<()>>);

impl HeldWorkers {
    fn new(engine: &AudioEngine) -> Self {
        let mut held = Self(Vec::new());
        for _ in 0..crate::audio_engine::cold_jobs::WORKERS {
            let reservation = engine.cold_jobs.reserve().unwrap();
            let (started, entered) = mpsc::channel();
            let (release, resumed) = mpsc::channel();
            engine
                .cold_jobs
                .submit(reservation, move || {
                    started.send(()).unwrap();
                    let _ = resumed.recv();
                })
                .unwrap();
            held.0.push(release);
            entered.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        held
    }
}

impl Drop for HeldWorkers {
    fn drop(&mut self) {
        for release in self.0.drain(..) {
            let _ = release.send(());
        }
    }
}

fn request_public(engine: &AudioEngine, width: usize) {
    Python::attach(|py| {
        assert!(
            engine
                .get_waveform_render_data(py, 0, width, 0.0, PUBLIC_FRAMES as f64 / 48_000.0)
                .unwrap()
                .is_none()
        );
    });
}

fn pump(engine: &AudioEngine) {
    Python::attach(|py| assert!(engine.poll_loader_events(py).unwrap().is_none()));
}

fn wait_for_jobs(engine: &AudioEngine) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while engine.cold_jobs.counts_for_test() != (0, 0, 0) {
        assert!(
            Instant::now() < deadline,
            "waveform fixture jobs did not end"
        );
        std::thread::yield_now();
    }
}

#[test]
fn cancelled_queued_waveform_supersession_returns_capacity_before_workers_end() {
    let mut fixture = LaunchFixture::new(4);
    fixture.publish_frames(0, 0.25, 9, PUBLIC_FRAMES);
    let held = HeldWorkers::new(&fixture.engine);
    let peer_runs = Arc::new(AtomicUsize::new(0));
    let peer = peer_runs.clone();
    fixture
        .engine
        .cold_jobs
        .submit(fixture.engine.cold_jobs.reserve().unwrap(), move || {
            peer.fetch_add(1, Ordering::AcqRel);
        })
        .unwrap();
    let latest_width = crate::audio_engine::cold_jobs::QUEUED_JOBS - 1;
    for width in 1..=latest_width {
        request_public(&fixture.engine, width);
    }
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (2, 32, 34));
    pump(&fixture.engine);
    // Capture all facts before teardown; the failing original also joins its workers.
    let after_pump = fixture.engine.cold_jobs.counts_for_test();
    let pending = fixture.engine.waveform_readiness(0).unwrap();
    let peer_before_release = peer_runs.load(Ordering::Acquire);
    let mut returned = Vec::new();
    while let Ok(reservation) = fixture.engine.cold_jobs.reserve() {
        returned.push(reservation);
    }
    let available = returned.len();
    drop(returned);
    drop(held);
    wait_for_jobs(&fixture.engine);
    Python::attach(|py| {
        let (raw, xs, low, high) = fixture
            .engine
            .get_waveform_render_data(py, 0, latest_width, 0.0, PUBLIC_FRAMES as f64 / 48_000.0)
            .unwrap()
            .unwrap();
        assert!(!raw);
        for bin in 0..latest_width {
            assert_eq!(
                xs.bind(py).get_item(bin).unwrap().extract::<f64>().unwrap(),
                (bin * PUBLIC_FRAMES / latest_width) as f64 / 48_000.0
            );
            assert_eq!(
                low.bind(py)
                    .get_item(bin)
                    .unwrap()
                    .extract::<f32>()
                    .unwrap(),
                0.25
            );
            assert_eq!(
                high.as_ref()
                    .unwrap()
                    .bind(py)
                    .get_item(bin)
                    .unwrap()
                    .extract::<f32>()
                    .unwrap(),
                0.25
            );
        }
    });
    fixture.engine.cold_jobs.shutdown();
    assert_eq!(after_pump, (2, 2, 4));
    assert_eq!(available, 30);
    assert_eq!(pending, ("pending".into(), None));
    assert_eq!(peer_before_release, 0);
    assert_eq!(peer_runs.load(Ordering::Acquire), 1);
}

fn cancelled_queued_waveform_authority(mode: &str) {
    let mut fixture = LaunchFixture::new(4);
    fixture.publish_frames(0, 0.25, 9, PUBLIC_FRAMES);
    let held = HeldWorkers::new(&fixture.engine);
    let old_pcm = Arc::downgrade(
        &fixture.engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples,
    );
    request_public(&fixture.engine, 8);
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (2, 1, 3));
    match mode {
        "cancel" => fixture.engine.retry_waveform(0).unwrap(),
        "source" => fixture.publish_frames(0, -0.5, 10, PUBLIC_FRAMES),
        "shutdown" => fixture.engine.cold_cancelled.store(true, Ordering::Release),
        _ => unreachable!(),
    }
    pump(&fixture.engine);
    let after_pump = fixture.engine.cold_jobs.counts_for_test();
    let source_ended = old_pcm.strong_count() == 0;
    let status = fixture.engine.waveform_readiness(0).unwrap();
    drop(held);
    wait_for_jobs(&fixture.engine);
    fixture.engine.cold_jobs.shutdown();
    assert_eq!(after_pump, (2, 0, 2), "{mode}");
    assert_eq!(source_ended, mode == "source", "{mode}");
    if mode != "shutdown" {
        assert_eq!(status, ("idle".into(), None), "{mode}");
    }
}

#[test]
fn cancelled_queued_waveform_explicit_retry_retires_before_workers_end() {
    cancelled_queued_waveform_authority("cancel");
}

#[test]
fn cancelled_queued_waveform_source_replacement_ends_last_old_pcm_reader() {
    cancelled_queued_waveform_authority("source");
}

#[test]
fn cancelled_queued_waveform_shutdown_retires_before_workers_end() {
    cancelled_queued_waveform_authority("shutdown");
}

#[test]
fn actual_queued_waveform_worker_returns_only_to_moved_original_content_cell() {
    let mut fixture = LaunchFixture::new(4);
    let mut releases = Vec::new();
    for _ in 0..crate::audio_engine::cold_jobs::WORKERS {
        let reservation = fixture.engine.cold_jobs.reserve().unwrap();
        let (started, entered) = mpsc::channel();
        let (release, resumed) = mpsc::channel();
        fixture
            .engine
            .cold_jobs
            .submit(reservation, move || {
                started.send(()).unwrap();
                resumed.recv_timeout(Duration::from_secs(5)).unwrap();
            })
            .unwrap();
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        releases.push(release);
    }
    let reference = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    assert!(
        fixture
            .engine
            .waveform_requests
            .request(&fixture.engine, 0, &reference, 0..128, 8,)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .waveform_requests
            .status(&fixture.engine, 0)
            .unwrap()
            .0,
        "pending"
    );
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let ticket = own_pair_ack(&mut fixture, &mut mixer);
    assert!(pad_pair::reconcile(&fixture.engine, &ticket).unwrap());
    assert_eq!(
        fixture
            .engine
            .waveform_requests
            .status(&fixture.engine, 0)
            .unwrap()
            .0,
        "idle"
    );
    assert_eq!(
        fixture
            .engine
            .waveform_requests
            .status(&fixture.engine, 1)
            .unwrap()
            .0,
        "pending"
    );
    // An explicit old-slot retry cannot cancel A's already admitted request at its new slot.
    fixture
        .engine
        .waveform_requests
        .cancel(&fixture.engine, 0)
        .unwrap();
    for release in releases {
        release.send(()).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = fixture
            .engine
            .waveform_requests
            .status(&fixture.engine, 1)
            .unwrap()
            .0;
        if status == "ready" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "waveform source worker remained {status}"
        );
        std::thread::yield_now();
    }
    let moved = fixture.engine.sample_cache.lock().unwrap()[1]
        .clone()
        .unwrap();
    let data = fixture
        .engine
        .waveform_requests
        .request(&fixture.engine, 1, &moved, 0..128, 8)
        .unwrap()
        .unwrap();
    assert_eq!(data.y_min, vec![0.25; 8]);
    assert_eq!(data.y_max.as_ref().unwrap(), &vec![0.25; 8]);
    assert_eq!(
        fixture
            .engine
            .waveform_requests
            .status(&fixture.engine, 0)
            .unwrap()
            .0,
        "idle"
    );
}
