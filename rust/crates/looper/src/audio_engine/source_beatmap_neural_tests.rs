//! Actual cold-source/worker composition. No audio device or listening assertions.

use super::*;
use crate::audio_engine::LoaderEventDelivery;
use crate::audio_engine::OfflineAnalysisJob;
use crate::audio_engine::cold_residency::ResidentLoadHint;
use crate::audio_engine::source_beatmap_capture::RawCapture;
use crate::audio_engine::source_map_test_support::ColdCustody;
use pyo3::types::PyDict;
use std::time::{Duration, Instant};

#[path = "source_beatmap_neural_cancel_tests.rs"]
mod cancellation;

fn fixture(seconds: usize) -> (ColdCustody, Vec<i16>) {
    let pcm = (0..seconds * 48_000)
        .map(|frame| {
            let phase = frame % 24_000;
            let kick = if phase < 2400 {
                (-(phase as f64) / 480.0).exp()
                    * (2.0 * std::f64::consts::PI * 70.0 * phase as f64 / 48_000.0).sin()
                    * 0.65
            } else {
                0.0
            };
            let tone = (2.0 * std::f64::consts::PI * 211.0 * frame as f64 / 48_000.0).sin() * 0.035;
            ((kick + tone) * 32767.0).round() as i16
        })
        .collect::<Vec<_>>();
    let fixture = ColdCustody::new_selected_with_signal(
        Some(ResidentLoadHint {
            start_s: 0.0,
            end_s: seconds as f64,
            key_lock: false,
        }),
        &pcm,
    );
    (fixture, pcm)
}

fn generation(fixture: &ColdCustody) -> u64 {
    fixture.engine.loaded_source_generations.lock().unwrap()[0].0
}

fn terminal(job: &OfflineAnalysisJob, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        let (terminal, stage) = Python::attach(|py| {
            let status = job.source_beatmap_async_status(py).unwrap().unwrap();
            let status = status.bind(py);
            (
                status
                    .get_item("terminal")
                    .unwrap()
                    .unwrap()
                    .extract::<bool>()
                    .unwrap(),
                status
                    .get_item("stage")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
            )
        });
        if terminal {
            return stage;
        }
        assert!(
            Instant::now() < deadline,
            "actual Native source-map job did not settle"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn no_staging(fixture: &ColdCustody) {
    let mut directories = vec![fixture.directory.path().to_owned()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            assert!(
                !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".beat-this-source-map-")
            );
            if entry.file_type().unwrap().is_dir() {
                directories.push(entry.path());
            }
        }
    }
}

#[test]
fn original_beat_this_invalid_admission_preserves_source_and_audio_epochs() {
    let (fixture, _) = fixture(1);
    let source_generation = generation(&fixture);
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let epochs = fixture
        .engine
        .prepared_source_epochs
        .iter()
        .map(|epoch| epoch.load(Ordering::Acquire))
        .collect::<Vec<_>>();
    assert!(
        fixture
            .engine
            .begin_source_beatmap_beat_this_raw_async(
                0,
                "relative-installation".into(),
                source_generation
            )
            .is_err()
    );
    assert!(
        fixture
            .engine
            .begin_source_beatmap_beat_this_raw_async(
                0,
                fixture.directory.path().to_str().unwrap().into(),
                0
            )
            .is_err()
    );
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    assert_eq!(
        fixture
            .engine
            .prepared_source_epochs
            .iter()
            .map(|epoch| epoch.load(Ordering::Acquire))
            .collect::<Vec<_>>(),
        epochs
    );
    no_staging(&fixture);
}

#[test]
fn original_beat_this_missing_worker_retires_real_staging_and_allows_retry() {
    let (fixture, _) = fixture(1);
    let missing = fixture
        .directory
        .path()
        .join("missing-worker")
        .to_str()
        .unwrap()
        .to_owned();
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    for _ in 0..2 {
        let job = fixture
            .engine
            .begin_source_beatmap_beat_this_raw_async(0, missing.clone(), generation(&fixture))
            .unwrap();
        assert_eq!(terminal(&job, Duration::from_secs(15)), "failed");
        Python::attach(|py| {
            let status = job.source_beatmap_async_status(py).unwrap().unwrap();
            let detail = status
                .bind(py)
                .get_item("detail")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert!(
                detail.contains("worker_installation_unavailable"),
                "{detail}"
            );
        });
        no_staging(&fixture);
        Python::attach(|py| {
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
                assert_ne!(kind, "source_beatmap_raw_completed");
                assert_ne!(kind, "source_beatmap_completed");
            }
        });
    }
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
}

#[test]
fn automatic_map_only_missing_worker_preserves_epochs_and_does_not_reserve_raw_review() {
    let (fixture, _) = fixture(1);
    let raw_slot = crate::audio_engine::source_beatmap_raw::RawReviewLease::reserve().unwrap();
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let epochs: Vec<_> = fixture
        .engine
        .prepared_source_epochs
        .iter()
        .map(|e| e.load(Ordering::Acquire))
        .collect();
    for _ in 0..2 {
        let job = fixture
            .engine
            .begin_automatic_source_beatmap_async(
                0,
                fixture
                    .directory
                    .path()
                    .join("missing-worker")
                    .to_str()
                    .unwrap()
                    .to_owned(),
                generation(&fixture),
            )
            .unwrap();
        assert_eq!(terminal(&job, Duration::from_secs(15)), "failed");
        no_staging(&fixture);
    }
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    assert_eq!(
        fixture
            .engine
            .prepared_source_epochs
            .iter()
            .map(|e| e.load(Ordering::Acquire))
            .collect::<Vec<_>>(),
        epochs
    );
    drop(raw_slot);
}

#[test]
#[ignore = "explicit real pinned NN map-only composition; no device/hearing acceptance"]
fn automatic_map_only_real_worker_exports_source_bound_v4_without_raw_slot_or_tempo_changes() {
    let installation =
        std::env::var("LOOPER_BEAT_THIS_INSTALL_DIR").expect("explicit verified worker root");
    let (fixture, _pcm) = fixture(32);
    let raw_slot = crate::audio_engine::source_beatmap_raw::RawReviewLease::reserve().unwrap();
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let epochs: Vec<_> = fixture
        .engine
        .prepared_source_epochs
        .iter()
        .map(|e| e.load(Ordering::Acquire))
        .collect();
    let job = fixture
        .engine
        .begin_automatic_source_beatmap_async(0, installation, generation(&fixture))
        .unwrap();
    let stage = terminal(&job, Duration::from_secs(180));
    let detail = Python::attach(|py| {
        job.source_beatmap_async_status(py)
            .unwrap()
            .unwrap()
            .bind(py)
            .get_item("detail")
            .unwrap()
            .unwrap()
            .extract::<Option<String>>()
            .unwrap()
    });
    assert_eq!(stage, "ready", "{detail:?}");
    let artifact = Python::attach(|py| {
        let mut result = None;
        while let Some(event) = fixture.engine.poll_loader_events(py).unwrap() {
            let event = event.bind(py).cast::<PyDict>().unwrap();
            let kind = event
                .get_item("type")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert_ne!(kind, "source_beatmap_raw_completed");
            if kind == "source_beatmap_completed" {
                let original = event
                    .get_item("_native_event_owner")
                    .unwrap()
                    .unwrap()
                    .extract::<PyRef<'_, LoaderEventDelivery>>()
                    .unwrap();
                assert!(
                    job.resolve_source_beatmap_completion(&original)
                        .unwrap()
                        .is_none()
                );
                let export = fixture
                    .engine
                    .consume_automatic_source_beatmap(py, &original)
                    .unwrap()
                    .unwrap();
                let artifact = export
                    .bind(py)
                    .get_item(1)
                    .unwrap()
                    .extract::<Py<crate::audio_engine::SourceBeatmapArtifact>>()
                    .unwrap()
                    .borrow(py)
                    .clone();
                assert!(artifact.map().is_pristine_automatic());
                assert!(
                    fixture
                        .engine
                        .consume_automatic_source_beatmap(py, &original)
                        .unwrap()
                        .is_none()
                );
                result = Some(artifact);
            }
        }
        result.expect("genuine private automatic map-only completion")
    });
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    assert_eq!(
        fixture
            .engine
            .prepared_source_epochs
            .iter()
            .map(|e| e.load(Ordering::Acquire))
            .collect::<Vec<_>>(),
        epochs
    );
    no_staging(&fixture);
    println!(
        "AUTOMATIC_MAP_ONLY_REALNN schema=4 source_frames=1536000 source_rate=48000 raw_slot_occupied=true request_epoch_unchanged=true genuine_one_shot_export=true; synthetic click music, no hearing acceptance"
    );
    artifact.retire_unselected();
    drop(artifact);
    drop(raw_slot);
}

#[test]
#[ignore = "explicit real pinned Beat This worker on generated original PCM; no device/music qualification"]
fn original_beat_this_real_worker_publishes_complete_source_bound_capture() {
    let installation = std::env::var("LOOPER_BEAT_THIS_INSTALL_DIR")
        .expect("explicit verified worker installation required");
    let (fixture, pcm) = fixture(32);
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let job = fixture
        .engine
        .begin_source_beatmap_beat_this_raw_async(0, installation, generation(&fixture))
        .unwrap();
    let stage = terminal(&job, Duration::from_secs(180));
    let detail = Python::attach(|py| {
        let status = job.source_beatmap_async_status(py).unwrap().unwrap();
        status
            .bind(py)
            .get_item("detail")
            .unwrap()
            .unwrap()
            .extract::<Option<String>>()
            .unwrap()
    });
    assert_eq!(stage, "ready", "{detail:?}");
    let candidate = Python::attach(|py| {
        let mut candidate = None;
        while let Some(event) = fixture.engine.poll_loader_events(py).unwrap() {
            let event = event.bind(py).cast::<PyDict>().unwrap();
            if event
                .get_item("type")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap()
                == "source_beatmap_raw_completed"
            {
                let owner = event.get_item("_native_event_owner").unwrap().unwrap();
                let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
                candidate = job
                    .resolve_source_beatmap_raw_completion(&delivery)
                    .unwrap();
            }
        }
        candidate.expect("actual private Native Raw delivery required")
    });
    let capture = candidate.captured_data_for_test();
    assert!(matches!(capture, RawCapture::BeatThis(_)));
    assert_eq!(capture.source_frame_count(), pcm.len());
    assert_eq!(capture.source_sample_rate_hz(), 48_000);
    let mut digest = Sha256::new();
    for sample in pcm {
        digest.update((f32::from(sample) / 32768.0).to_le_bytes());
    }
    let info = capture.backend_info();
    assert_eq!(
        info["worker_input_sha256"],
        format!("{:x}", digest.finalize())
    );
    assert_eq!(info["beat_logit_count"], 1601);
    assert_eq!(info["downbeat_logit_count"], 1601);
    let encoded = capture.encode().unwrap();
    let reparsed = RawCapture::decode(&encoded).unwrap();
    assert_eq!(capture.revision(), reparsed.revision());
    assert_eq!(encoded, reparsed.encode().unwrap());
    println!(
        "BEAT_THIS_ORIGINAL_CAPTURE {}",
        json!({"source_frames":capture.source_frame_count(),
        "raw_revision":capture.revision(),"raw_bytes":encoded.len(),"info":info,
        "technical_complete":true,"automatic_musical_qualification":false,
        "renderer_or_device_acceptance":false})
    );
    // Explicit labels are supplied intent, never inferred from detection order.
    let expected_capture = capture.embedded_value().unwrap();
    assert!(capture.event_count() >= 2);
    let intent = json!({"schema_version":1,"raw_revision":capture.revision(),"anchors":[
        {"raw_index":0,"quarter_bits":format!("{:016x}",0.0_f64.to_bits())},
        {"raw_index":1,"quarter_bits":format!("{:016x}",1.0_f64.to_bits())}
    ]})
    .to_string();
    let review = fixture
        .engine
        .begin_source_beatmap_review_async(0, &candidate, intent, generation(&fixture))
        .unwrap();
    assert_eq!(terminal(&review, Duration::from_secs(30)), "ready");
    let artifact = Python::attach(|py| {
        let mut artifact = None;
        while let Some(event) = fixture.engine.poll_loader_events(py).unwrap() {
            let event = event.bind(py).cast::<PyDict>().unwrap();
            if event
                .get_item("type")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap()
                == "source_beatmap_completed"
            {
                let owner = event.get_item("_native_event_owner").unwrap().unwrap();
                let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
                artifact = review
                    .resolve_source_beatmap_artifact_completion(&delivery)
                    .unwrap();
            }
        }
        artifact.expect("genuine Accepted V3 private artifact required")
    });
    job.cancel();
    drop(candidate);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        fixture.engine.project_assets.collect_for_test();
        let retired = Python::attach(|py| {
            job.source_beatmap_async_status(py)
                .unwrap()
                .unwrap()
                .bind(py)
                .get_item("raw_review_retired")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        });
        if retired {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "original Raw reader did not retire"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!review.state().cancelled());
    assert!(review.state().native_owner.as_ref().unwrap().current());
    let accepted = artifact.map();
    let record: serde_json::Value = serde_json::from_str(accepted.canonical_record()).unwrap();
    assert_eq!(record["schema_version"], 3);
    assert_eq!(record["record"]["raw"]["capture"], expected_capture);
    assert_eq!(
        record["record"]["accepted"]["intervals"][0]["coverage"],
        "uncertain"
    );
    assert_eq!(
        record["record"]["accepted"]["intervals"][0]["meter"]["status"],
        "unknown"
    );
    assert!(!artifact.receipt_json().is_empty());
    assert!(
        crate::audio_engine::source_beatmap::SourceBeatmap::parse(accepted.canonical_record())
            .is_ok()
    );
    no_staging(&fixture);
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
}
