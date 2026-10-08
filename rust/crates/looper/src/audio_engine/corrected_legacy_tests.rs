use super::super::{OfflineAnalysisJob, OfflineJobs, OfflineRequestOwner};
use super::*;
use crate::messages::{LoaderEvent, SampleBuffer};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc::{Receiver, channel},
};

fn source() -> SampleBuffer {
    let mut samples = vec![0.0_f32; 48_000 * 6 + 7];
    for index in (0..samples.len() - 4).step_by(24_000) {
        samples[index..index + 4].copy_from_slice(&[0.5, 1.0, 0.5, 0.25]);
    }
    SampleBuffer {
        residency: None,
        samples: samples.into(),
        channels: 1,
    }
}

fn reservation(
    jobs: &OfflineJobs,
    ids: &Arc<Mutex<Vec<u64>>>,
    tx: &std::sync::mpsc::Sender<LoaderEvent>,
) -> OfflineAnalysisJob {
    jobs.begin(
        0,
        source(),
        48_000,
        1,
        OfflineRequestOwner {
            request_ids: ids.clone(),
            prepared_epoch: Arc::new(AtomicU64::new(1)),
        },
        tx.clone(),
    )
    .unwrap()
}

fn prepared() -> (
    OfflineJobs,
    Arc<Mutex<Vec<u64>>>,
    OfflineAnalysisJob,
    Receiver<LoaderEvent>,
    tempfile::TempDir,
) {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let job = reservation(&jobs, &ids, &tx);
    let directory = tempfile::tempdir().unwrap();
    job.state()
        .prepare(directory.path().join("mono.f32le").to_str().unwrap())
        .unwrap();
    (jobs, ids, job, rx, directory)
}

fn values_f64(value: &Value) -> Vec<f64> {
    STANDARD
        .decode(value.as_str().unwrap())
        .unwrap()
        .chunks_exact(8)
        .map(|item| f64::from_le_bytes(item.try_into().unwrap()))
        .collect()
}

#[test]
fn complete_actual_capture_retains_native_input_and_publishes_losslessly_once() {
    let (jobs, ids, job, rx, directory) = prepared();
    let analyzer_path = directory.path().join("actual-input.f64le");
    let wire = job
        .state()
        .corrected_legacy(Some(analyzer_path.to_str().unwrap()))
        .unwrap();
    let result: Value = serde_json::from_str(&wire).unwrap();
    let converted = resample_mono_cancellable(
        source().samples.to_vec(),
        48_000,
        44_100,
        MAX_PCM_BYTES,
        &|| false,
    )
    .unwrap();
    let expected: Vec<f64> = converted.into_iter().map(f64::from).collect();
    let bytes = std::fs::read(&analyzer_path).unwrap();
    assert_eq!(
        bytes,
        expected
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        result["analyzer"]["sha256"],
        format!("{:x}", Sha256::digest(&bytes))
    );
    assert_eq!(result["analyzer"]["frame_count"], expected.len());
    assert_eq!(result["loaded"]["frame_count"], source().frame_count());
    let raw = analyze_bpm_raw(&expected, 44_100, &AnalysisConfig::default()).unwrap();
    assert!(!raw.beat_frames().is_empty());
    assert_eq!(values_f64(&result["raw"]["beat_frames"]), raw.beat_frames());
    assert_eq!(
        result["raw"]["beat_seconds"],
        packed_f64(raw.beat_seconds())
    );
    assert_eq!(
        result["raw"]["downbeat_seconds"],
        packed_f64(raw.downbeat_seconds())
    );
    let (bpm, compatibility) = raw.legacy_result();
    assert_eq!(result["compatibility"]["bpm"], packed_f32([bpm]));
    assert_eq!(
        result["compatibility"]["beats"],
        packed_f32(compatibility.beats)
    );
    assert_eq!(
        result["compatibility"]["downbeats"],
        packed_f32(compatibility.downbeats)
    );
    assert_eq!(
        result["compatibility"]["bars"],
        packed_f32(compatibility.bars)
    );
    assert!(job.state().corrected_legacy(None).is_err());
    job.state().retire_pcm().unwrap();
    assert!(jobs.busy.load(Ordering::Acquire));
    assert!(job.state().finish_corrected_legacy(&wire).unwrap());
    assert!(!jobs.busy.load(Ordering::Acquire));
    assert!(job.state().finish_corrected_legacy(&wire).is_err());
    let completions: Vec<_> = rx
        .try_iter()
        .filter_map(|event| match event {
            LoaderEvent::OfflineAnalysisCompleted {
                id,
                request_id,
                result_json,
            } => Some((id, request_id, result_json)),
            _ => None,
        })
        .collect();
    assert_eq!(completions.len(), 1);
    assert_eq!(
        (completions[0].0, completions[0].1),
        (0, ids.lock().unwrap()[0])
    );
    let published: Value = serde_json::from_str(&completions[0].2).unwrap();
    assert_eq!(published["raw"], result["raw"]);
    assert_eq!(published["compatibility"], result["compatibility"]);
}

#[test]
fn publication_requires_actual_native_success_and_rejects_self_consistent_spoofs() {
    let (_, _, job, _, _directory) = prepared();
    let wire = job.state().corrected_legacy(None).unwrap();
    let original: Value = serde_json::from_str(&wire).unwrap();
    for (pointer, substitute) in [
        ("/identity/request_id", json!(999)),
        ("/loaded/mono_sha256", json!("a".repeat(64))),
        ("/analyzer/sha256", json!("b".repeat(64))),
        ("/analyzer/odf_hop_samples", json!(0)),
        ("/configuration/alpha", json!(0.8)),
        ("/raw/beat_seconds", json!(packed_f64([0.1, 0.2]))),
        ("/compatibility/bpm", json!(packed_f32([1.0]))),
        ("/diagnostic_only", json!(false)),
    ] {
        let mut corrupted = original.clone();
        *corrupted.pointer_mut(pointer).unwrap() = substitute;
        assert!(
            job.state()
                .finish_corrected_legacy(&corrupted.to_string())
                .is_err(),
            "{pointer}"
        );
    }
    let mut extra = original.clone();
    extra["model"] = json!({"sha256":"a".repeat(64)});
    assert!(
        job.state()
            .finish_corrected_legacy(&extra.to_string())
            .is_err()
    );
    assert!(
        job.state()
            .finish_corrected_legacy(&" ".repeat(MAX_RESULT_BYTES + 1))
            .is_err()
    );
    let (_, _, other, _, _other_directory) = prepared();
    let duplicated = wire.replace(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
    );
    assert_ne!(duplicated, wire);
    assert!(job.state().finish_corrected_legacy(&duplicated).is_err());
    assert!(
        job.state()
            .finish_corrected_legacy(&serde_json::to_string_pretty(&original).unwrap())
            .is_err()
    );
    assert!(
        other
            .state()
            .finish_corrected_legacy(&wire)
            .unwrap_err()
            .contains("successful native")
    );
    assert!(job.state().finish(&wire).is_err());
    assert!(job.state().finish_corrected_legacy(&wire).unwrap());
}

#[test]
fn cancelled_native_capture_never_publishes_and_retires_reservation() {
    let (jobs, _, job, rx, _directory) = prepared();
    let wire = job.state().corrected_legacy(None).unwrap();
    job.cancel();
    assert!(!job.state().finish_corrected_legacy(&wire).unwrap());
    assert!(!jobs.busy.load(Ordering::Acquire));
    assert!(
        rx.try_iter()
            .all(|event| !matches!(event, LoaderEvent::OfflineAnalysisCompleted { .. }))
    );
}

#[test]
fn running_legacy_preserves_retirement_and_publication_guards() {
    let (jobs, _, job, _, _directory) = prepared();
    let wire = job.state().corrected_legacy(None).unwrap();
    job.state().legacy_running.store(true, Ordering::Release);
    assert!(job.state().retire_pcm().unwrap_err().contains("retiring"));
    assert!(
        job.state()
            .finish_corrected_legacy(&wire)
            .unwrap_err()
            .contains("retiring")
    );
    assert!(jobs.busy.load(Ordering::Acquire));
    assert!(job.state().key().is_err());
    assert!(!job.state().key_started.load(Ordering::Acquire));
    job.state().legacy_running.store(false, Ordering::Release);
    assert!(job.state().finish_corrected_legacy(&wire).unwrap());
}

#[test]
fn legacy_and_key_claims_are_reciprocally_exclusive_through_retirement() {
    let (_, _, legacy, _, _legacy_directory) = prepared();
    let wire = legacy.state().corrected_legacy(None).unwrap();
    assert!(legacy.state().key().is_err());
    assert!(!legacy.state().key_started.load(Ordering::Acquire));
    assert!(legacy.state().finish_corrected_legacy(&wire).unwrap());

    let (jobs, _, key, _, _key_directory) = prepared();
    // The key claim is set before its noninterruptible inference. Exercise that
    // lifecycle without requiring the private KeyNet installation in this test.
    key.state().key_started.store(true, Ordering::Release);
    key.state().key_running.store(true, Ordering::Release);
    assert!(key.state().corrected_legacy(None).is_err());
    assert!(!key.state().legacy_started.load(Ordering::Acquire));
    assert!(key.state().retire_pcm().unwrap_err().contains("retiring"));
    assert!(jobs.busy.load(Ordering::Acquire));
    key.state().key_running.store(false, Ordering::Release);
    key.state().retire_pcm().unwrap();
    assert!(key.state().corrected_legacy(None).is_err());
    assert!(jobs.busy.load(Ordering::Acquire));
}

#[test]
fn abort_started_legacy_rejects_without_waiting_for_its_pcm_reader() {
    let (jobs, _, job, _, _directory) = prepared();
    let job = Arc::new(job);
    job.state().legacy_started.store(true, Ordering::Release);
    job.state().legacy_running.store(true, Ordering::Release);
    let staged_reader = job.state().staged_pcm.lock().unwrap();
    let (tx, rx) = channel();
    let abort_job = job.clone();
    let abort = std::thread::spawn(move || {
        tx.send(abort_job.abort_unstarted().is_err()).unwrap();
    });
    let immediate = rx.recv_timeout(std::time::Duration::from_secs(2));
    // Always release the held PCM lock, even when the regression blocks abort.
    drop(staged_reader);
    abort.join().unwrap();
    assert_eq!(immediate.unwrap(), true);
    assert!(jobs.busy.load(Ordering::Acquire));
    assert!(!job.state().retirement_started.load(Ordering::Acquire));
    job.state().legacy_running.store(false, Ordering::Release);
}

#[test]
fn explicit_input_export_never_overwrites_and_capture_has_finite_geometry() {
    let (_, _, job, _, directory) = prepared();
    let path = directory.path().join("existing.f64le");
    std::fs::write(&path, b"preserve").unwrap();
    assert!(
        job.state()
            .corrected_legacy(Some(path.to_str().unwrap()))
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"preserve");
    let input = directory.path().join("invalid.f32le");
    std::fs::write(&input, f32::NAN.to_le_bytes()).unwrap();
    let mut file = File::open(&input).unwrap();
    assert!(
        read_mono(&mut file, 1, &|| false)
            .unwrap_err()
            .contains("nonfinite")
    );
    assert!(
        read_mono(&mut file, 2, &|| false)
            .unwrap_err()
            .contains("geometry")
    );
    assert!(
        read_mono(&mut file, 1, &|| true)
            .unwrap_err()
            .contains("cancelled")
    );
}
