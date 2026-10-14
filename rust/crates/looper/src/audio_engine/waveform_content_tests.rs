//! Actual bounded waveform work follows its captured source through an own Pair mirror.

use super::*;
use crate::audio_engine::complete_context::source_custody_tests::own_pair_ack;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::pad_pair;
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
