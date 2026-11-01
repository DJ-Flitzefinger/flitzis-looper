//! Actual asynchronous load admission, with all observations before blocker teardown.
use super::*;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use pyo3::types::PyDictMethods;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cancellation {
    Request,
    Epoch,
    Placement,
    Resident,
    Shutdown,
    ReplacementToken,
}

fn queued_load_cancellation(kind: Cancellation, analysis: bool, occupy_workers: bool) {
    let fixture = LaunchFixture::new(4);
    let directory = tempfile::tempdir().unwrap();
    let samples = fixture.engine.sample_cache.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .samples
        .clone();
    let baseline = Arc::strong_count(&samples);
    let blocker = super::super::heavy_work::ticket().unwrap();
    assert!(blocker.try_acquire().unwrap());
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
    let (entered, started) = mpsc::channel();
    super::pending_cancellation_tests::probe_next_preparation(move |_| {
        let _ = entered.send(());
    });
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    // This source is deliberately never opened: the real queued request is
    // cancelled before preparation, not failed by a decoder or model fixture.
    let admitted = admit_selected_material(
        &fixture.engine,
        0,
        directory
            .path()
            .join("never-opened.wav")
            .to_string_lossy()
            .into_owned(),
        (
            analysis,
            false,
            true,
            Some(ResidentLoadHint {
                start_s: 0.0,
                end_s: 0.01,
                key_lock: false,
            }),
        ),
        Arc::new(Mutex::new(producer)),
        (2, 48_000, directory.path().join("samples")),
        "import",
        None,
    )
    .unwrap();
    let request = admitted.request_id;
    let adoption = admitted.adoption.clone();
    let settled = admitted.cancellation_settled.clone();
    drop(admitted);
    assert!(Arc::strong_count(&samples) > baseline);
    match kind {
        Cancellation::Request => {
            super::super::next_pad_request_id(
                &fixture.engine.pad_request_ids,
                0,
                fixture.engine.prepared_source_epoch(0).unwrap(),
            )
            .unwrap();
        }
        Cancellation::Epoch => {
            fixture
                .engine
                .prepared_source_epoch(0)
                .unwrap()
                .fetch_add(1, Ordering::AcqRel);
        }
        Cancellation::Placement => {
            let replacement = SampleBuffer {
                samples: Arc::from([0.5_f32; 32]),
                channels: 2,
                residency: None,
            };
            fixture
                .engine
                .input_runtime_ownership
                .publish_test_source(0, &replacement, 48_000, 77);
        }
        Cancellation::Resident | Cancellation::ReplacementToken => {
            if kind == Cancellation::ReplacementToken {
                fixture.engine.cold_loading[0].store(request + 1, Ordering::Release);
            }
            fixture.engine.resident_restore_guards.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .cancelled
                .store(true, Ordering::Release);
        }
        Cancellation::Shutdown => fixture.engine.cold_cancelled.store(true, Ordering::Release),
    }
    let active = if occupy_workers {
        super::super::cold_jobs::WORKERS
    } else {
        0
    };
    let mut errors = 0;
    let mut other_terminal = false;
    let expected_loading = if kind == Cancellation::ReplacementToken {
        request + 1
    } else {
        0
    };
    let deadline = Instant::now() + Duration::from_millis(500);
    let retired = loop {
        // Exercise the real off-GIL control pump and request-filtered PyO3
        // delivery as well as worker polling, including two occupied workers.
        Python::attach(|py| {
            while let Some(event) = fixture.engine.poll_loader_events(py).unwrap() {
                let event = event.bind(py).cast::<pyo3::types::PyDict>().unwrap();
                if event
                    .get_item("request_id")
                    .unwrap()
                    .unwrap()
                    .extract::<u64>()
                    .unwrap()
                    == request
                {
                    match event
                        .get_item("type")
                        .unwrap()
                        .unwrap()
                        .extract::<String>()
                        .unwrap()
                        .as_str()
                    {
                        "error" => errors += 1,
                        "success" | "started" => other_terminal = true,
                        _ => {}
                    }
                }
            }
        });
        if Arc::strong_count(&samples) == baseline
            && adoption.load(Ordering::Acquire) == 3
            && settled.load(Ordering::Acquire)
            && fixture.engine.cold_loading[0].load(Ordering::Acquire) == expected_loading
            && fixture
                .engine
                .loading_sample_ids
                .lock()
                .unwrap()
                .contains(&0)
                == (expected_loading != 0)
            && fixture.engine.cold_jobs.counts_for_test() == (active, 0, active)
            && errors == 1
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let counts = fixture.engine.cold_jobs.counts_for_test();
    let pcm_owners = Arc::strong_count(&samples);
    let loading = fixture.engine.cold_loading[0].load(Ordering::Acquire);
    let phase = adoption.load(Ordering::Acquire);
    let cancelled = settled.load(Ordering::Acquire);
    let entered_payload = started.try_recv().is_ok();
    let published = consumer.peek().is_ok();
    let blocker_live = blocker.is_active();

    fixture.engine.cold_cancelled.store(true, Ordering::Release);
    if kind == Cancellation::ReplacementToken {
        fixture.engine.cold_loading[0].store(0, Ordering::Release);
        fixture.engine.loading_sample_ids.lock().unwrap().remove(&0);
    }
    for release in releases {
        let _ = release.send(());
    }
    drop(blocker);
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.engine.cold_jobs.counts_for_test() != (0, 0, 0) {
        assert!(
            Instant::now() < deadline,
            "queued load fixture did not retire"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(blocker_live);
    assert!(!entered_payload && !published && !other_terminal);
    assert!(
        retired,
        "{kind:?}: counts={counts:?}, PCM={pcm_owners}/{baseline}, loading={loading}, adoption={phase}, settled={cancelled}, errors={errors}"
    );
}

#[test]
fn queued_analysis_load_request_retires_before_heavy_predecessor() {
    queued_load_cancellation(Cancellation::Request, true, false);
}
#[test]
fn queued_analysis_load_epoch_retires_before_heavy_predecessor() {
    queued_load_cancellation(Cancellation::Epoch, true, false);
}
#[test]
fn queued_analysis_load_placement_retires_before_heavy_predecessor() {
    queued_load_cancellation(Cancellation::Placement, true, false);
}
#[test]
fn queued_analysis_load_resident_retires_before_heavy_predecessor() {
    queued_load_cancellation(Cancellation::Resident, true, false);
}
#[test]
fn queued_analysis_load_shutdown_retires_before_heavy_predecessor() {
    queued_load_cancellation(Cancellation::Shutdown, true, false);
}
#[test]
fn queued_ordinary_load_shutdown_retires_with_both_workers_busy() {
    queued_load_cancellation(Cancellation::Shutdown, false, true);
}

#[test]
fn queued_load_retirement_preserves_a_replacement_loading_token() {
    queued_load_cancellation(Cancellation::ReplacementToken, true, false);
}

#[test]
fn queued_pending_load_survives_and_reaches_original_own_ack() {
    use super::super::resident_relocation::control_publication_tests::PendingWriter;
    Python::initialize();
    let engine = AudioEngine::new().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pending.wav");
    let values = [0_i16, 8192, -16384, 32767, 0, -4096, 16384];
    let mut bytes = Vec::from(&b"RIFF"[..]);
    bytes.extend_from_slice(&(36_u32 + 14).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&96_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&14_u32.to_le_bytes());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    std::fs::write(path.clone(), bytes).unwrap();
    let mut releases = Vec::new();
    for _ in 0..super::super::cold_jobs::WORKERS {
        let (entered, started) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        engine
            .cold_jobs
            .submit(engine.cold_jobs.reserve().unwrap(), move || {
                entered.send(()).unwrap();
                let _ = resume.recv_timeout(Duration::from_secs(10));
            })
            .unwrap();
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        releases.push(release);
    }
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let request = admit_for_format(
        &engine,
        0,
        path.to_string_lossy().into_owned(),
        false,
        false,
        true,
        Arc::new(Mutex::new(producer)),
        2,
        48_000,
        directory.path().join("samples"),
    )
    .unwrap();
    let pending = PendingWriter::hold(engine.input_runtime_ownership.clone());
    let deadline = Instant::now() + Duration::from_millis(50);
    while Instant::now() < deadline {
        Python::attach(|py| assert!(engine.poll_loader_events(py).unwrap().is_none()));
        assert_eq!(engine.cold_jobs.counts_for_test(), (2, 1, 3));
        assert_eq!(engine.cold_loading[0].load(Ordering::Acquire), request);
        std::thread::sleep(Duration::from_millis(1));
    }
    pending.complete();
    assert!(engine.input_runtime_ownership.set_key_lock_start_behavior(
        super::super::key_lock_start_policy::KeyLockStartBehavior::FirstSoundAtGrid
    ));
    for release in releases {
        release.send(()).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while consumer.peek().is_err() && engine.cold_loading[0].load(Ordering::Acquire) != 0 {
        assert!(
            Instant::now() < deadline,
            "current queued source never published"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(consumer.peek().is_ok());
    let mut mixer = super::pending_cancellation_tests::acknowledge(&engine, &mut consumer);
    assert!(matches!(
        super::pending_cancellation_tests::terminal(&engine, request),
        LoaderEvent::Success { .. }
    ));
    assert!(mixer.play_sample(0, 1.0));
    assert_eq!(engine.pad_request_ids.lock().unwrap()[0], request);
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[0],
        (request, 48_000)
    );
}
