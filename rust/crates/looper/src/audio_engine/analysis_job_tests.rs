use super::*;
use std::sync::mpsc::{Receiver, channel};

fn sample() -> SampleBuffer {
    SampleBuffer {
        samples: Arc::from(vec![0.0_f32; 2048]),
        channels: 2,
    }
}

fn begin(
    jobs: &OfflineJobs,
    ids: &Arc<Mutex<Vec<u64>>>,
    tx: &Sender<LoaderEvent>,
) -> OfflineAnalysisJob {
    jobs.begin(0, sample(), 48_000, 1, ids.clone(), tx.clone())
        .unwrap()
}

fn envelope(job: &OfflineAnalysisJob) -> String {
    let id = &job.state().identity;
    json!({"schema_version":1,"identity":{"pad_id":id.pad_id,"request_id":id.request_id,
        "source_id":id.source_id,"source_generation":id.source_generation},
        "beat":{"status":"unavailable"},"key":{"status":"failed","key":"unknown"}})
    .to_string()
}

fn completions(rx: &Receiver<LoaderEvent>) -> usize {
    rx.try_iter()
        .filter(|e| matches!(e, LoaderEvent::OfflineAnalysisCompleted { .. }))
        .count()
}

#[test]
fn retiring_key_retains_slot_and_pcm_until_actual_terminal() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let job = begin(&jobs, &ids, &tx);
    let dir = tempfile::tempdir().unwrap();
    job.state()
        .prepare(dir.path().join("mono.f32").to_str().unwrap())
        .unwrap();
    job.state().key_running.store(true, Ordering::Release);
    jobs.cancel(Some(0));
    assert!(job.is_cancelled());
    assert!(
        job.state()
            .finish(&envelope(&job))
            .unwrap_err()
            .contains("retiring")
    );
    assert!(
        jobs.begin(0, sample(), 48_000, 1, ids.clone(), tx.clone())
            .is_err()
    );
    assert!(job.state().snapshot.lock().unwrap().is_some());
    assert!(job.state().mono.lock().unwrap().is_some());
    job.state().key_running.store(false, Ordering::Release);
    job.state().finish(&envelope(&job)).unwrap();
    assert!(job.state().snapshot.lock().unwrap().is_none());
    assert!(job.state().mono.lock().unwrap().is_none());
    assert_eq!(completions(&rx), 0);
    assert!(jobs.begin(0, sample(), 48_000, 1, ids, tx).is_ok());
}

#[test]
fn valid_components_publish_once_and_keep_loaded_source_identity() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![7]));
    let (tx, rx) = channel();
    let job = jobs
        .begin(0, sample(), 44_100, 7, ids.clone(), tx.clone())
        .unwrap();
    assert_eq!(job.state().identity.source_generation, 7);
    assert_eq!(job.state().identity.request_id, 8);
    job.state().finish(&envelope(&job)).unwrap();
    assert!(job.state().finish(&envelope(&job)).is_err());
    assert_eq!(completions(&rx), 1);
    let next = jobs.begin(0, sample(), 44_100, 7, ids, tx).unwrap();
    assert_eq!(next.state().identity.source_generation, 7);
    assert_eq!(next.state().identity.request_id, 9);
}

#[test]
fn stale_source_and_bad_identity_never_publish_or_release_live_slot() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let job = begin(&jobs, &ids, &tx);
    let mut wrong: Value = serde_json::from_str(&envelope(&job)).unwrap();
    wrong["identity"]["source_generation"] = json!(99);
    assert!(job.state().finish(&wrong.to_string()).is_err());
    assert!(jobs.busy.load(Ordering::Acquire));
    next_pad_request_id(&ids, 0).unwrap();
    assert!(job.is_cancelled());
    job.state().finish(&envelope(&job)).unwrap();
    assert_eq!(completions(&rx), 0);
}

#[test]
fn key_failure_is_independent_and_export_is_full_loaded_pcm() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let job = begin(&jobs, &ids, &tx);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mono.f32");
    job.state().prepare(path.to_str().unwrap()).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 1024 * 4);
    let key: Value = serde_json::from_str(&job.state().key().unwrap()).unwrap();
    assert_eq!(key["status"], "failed");
    assert_eq!(key["key"], "unknown");
    assert!(job.state().key().is_err());
    job.state().finish(&envelope(&job)).unwrap();
    assert_eq!(completions(&rx), 1);
}

#[test]
fn invalid_input_does_not_change_request_or_consume_admission() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, _) = channel();
    assert!(
        jobs.begin(0, sample(), 0, 1, ids.clone(), tx.clone())
            .is_err()
    );
    assert_eq!(current_pad_request_id(&ids, 0).unwrap(), 1);
    assert!(!jobs.busy.load(Ordering::Acquire));
    let job = begin(&jobs, &ids, &tx);
    let oversized = "x".repeat(MAX_RESULT_BYTES + 1);
    assert!(
        job.state()
            .finish(&oversized)
            .unwrap_err()
            .contains("limit")
    );
    assert!(jobs.busy.load(Ordering::Acquire));
    job.cancel();
    job.state().finish(&envelope(&job)).unwrap();
}

#[test]
fn repeated_old_retirement_cannot_release_a_new_reservation() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, _) = channel();
    let old = begin(&jobs, &ids, &tx);
    old.state().finish(&envelope(&old)).unwrap();
    let next = begin(&jobs, &ids, &tx);
    old.state().release_reservation();
    assert!(jobs.busy.load(Ordering::Acquire));
    assert!(jobs.begin(0, sample(), 48_000, 1, ids, tx).is_err());
    next.state().finish(&envelope(&next)).unwrap();
}

#[test]
fn cancelling_invalidates_request_and_disallows_new_native_work() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let job = begin(&jobs, &ids, &tx);
    job.cancel();
    assert_ne!(
        current_pad_request_id(&ids, 0).unwrap(),
        job.state().identity.request_id
    );
    assert!(job.state().key().is_err());
    let dir = tempfile::tempdir().unwrap();
    assert!(
        job.state()
            .prepare(dir.path().join("cancelled.f32").to_str().unwrap())
            .is_err()
    );
    job.state().finish(&envelope(&job)).unwrap();
    assert_eq!(completions(&rx), 0);
}

#[test]
fn nested_stale_nonterminal_or_missing_ready_components_are_rejected() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, _) = channel();
    let job = begin(&jobs, &ids, &tx);
    let original: Value = serde_json::from_str(&envelope(&job)).unwrap();
    let mut nested = original.clone();
    nested["beat"]["identity"] = json!({"request_id": 999});
    assert!(job.state().finish(&nested.to_string()).is_err());
    let mut retiring = original.clone();
    retiring["beat"]["resources_released"] = json!(false);
    assert!(job.state().finish(&retiring.to_string()).is_err());
    let mut ready = original.clone();
    ready["beat"]["status"] = json!("ready");
    assert!(job.state().finish(&ready.to_string()).is_err());
    let mut invalid_key = original;
    invalid_key["key"]["status"] = json!("ready");
    assert!(job.state().finish(&invalid_key.to_string()).is_err());
    job.state().finish(&envelope(&job)).unwrap();
}
