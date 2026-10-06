use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::sync::mpsc::{Receiver, channel};

fn sample() -> SampleBuffer {
    SampleBuffer {
        samples: Arc::from(vec![0.0_f32; 2048]),
        channels: 2,
    }
}

fn fresh_owner(request_ids: &Arc<Mutex<Vec<u64>>>) -> OfflineRequestOwner {
    OfflineRequestOwner {
        request_ids: request_ids.clone(),
        prepared_epoch: Arc::new(AtomicU64::new(1)),
    }
}

fn begin(
    jobs: &OfflineJobs,
    ids: &Arc<Mutex<Vec<u64>>>,
    tx: &Sender<LoaderEvent>,
) -> OfflineAnalysisJob {
    jobs.begin(0, sample(), 48_000, 1, fresh_owner(ids), tx.clone())
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

fn encoded_predictions(values: &[f64]) -> String {
    STANDARD.encode(
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

fn ready_envelope(job: &OfflineAnalysisJob, logit_count: usize) -> Value {
    let mut ready: Value = serde_json::from_str(&envelope(job)).unwrap();
    ready["schema_version"] = json!(2);
    ready["beat"] = json!({
        "identity": ready["identity"], "status": "ready", "reason": "ready",
        "resources_released": true,
        "model": {"sha256": "a".repeat(64), "frontend_id": "fixture", "environment_id": "fixture",
            "package_version": "1.1.0", "checkpoint": "final0", "postprocessor": "minimal",
            "device": "cpu", "precision": "float32"},
        "predictions": {"encoding": "float64-le/base64",
            "beat_seconds": encoded_predictions(&[0.0, 0.001, 0.02]),
            "downbeat_seconds": encoded_predictions(&[0.001]),
            "beat_logits": encoded_predictions(&vec![1.0000000000000002; logit_count]),
            "downbeat_logits": encoded_predictions(&vec![-0.0; logit_count])},
    });
    ready
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
    assert!(job.state().retire_pcm().unwrap_err().contains("retiring"));
    assert!(
        job.state()
            .finish(&envelope(&job))
            .unwrap_err()
            .contains("retiring")
    );
    assert!(
        jobs.begin(
            0,
            sample(),
            48_000,
            1,
            OfflineRequestOwner {
                request_ids: ids.clone(),
                prepared_epoch: Arc::new(AtomicU64::new(1))
            },
            tx.clone()
        )
        .is_err()
    );
    assert!(job.state().snapshot.lock().unwrap().is_none());
    assert!(job.state().staged_pcm.lock().unwrap().is_some());
    job.state().key_running.store(false, Ordering::Release);
    job.state().finish(&envelope(&job)).unwrap();
    assert!(job.state().snapshot.lock().unwrap().is_none());
    assert!(job.state().staged_pcm.lock().unwrap().is_none());
    assert_eq!(completions(&rx), 0);
    assert!(
        jobs.begin(
            0,
            sample(),
            48_000,
            1,
            OfflineRequestOwner {
                request_ids: ids,
                prepared_epoch: Arc::new(AtomicU64::new(1))
            },
            tx
        )
        .is_ok()
    );
}

#[test]
fn complete_export_retires_analysis_source_pin_but_preserves_playback_owner() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, _) = channel();
    let source = sample();
    let playback = source.samples.clone();
    let weak = Arc::downgrade(&playback);
    let job = jobs
        .begin(0, source, 96_000, 1, fresh_owner(&ids), tx)
        .unwrap();
    assert_eq!(Arc::strong_count(&playback), 2);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mono.f32");
    job.state().prepare(path.to_str().unwrap()).unwrap();
    assert_eq!(Arc::strong_count(&playback), 1);
    assert_eq!(playback.as_ref(), &[0.0; 2048]);
    assert_eq!(job.state().retained_source_bytes.load(Ordering::Acquire), 0);
    assert!(job.state().snapshot.lock().unwrap().is_none());
    assert!(job.state().staged_pcm.lock().unwrap().is_some());
    assert!(job.state().prepare(path.to_str().unwrap()).is_err());
    drop(playback);
    assert!(weak.upgrade().is_none());
    // Key still has the complete staged source after all playback owners vanish.
    let result: Value = serde_json::from_str(&job.state().key().unwrap()).unwrap();
    assert_eq!(result["status"], "failed"); // intentionally too short for CQT
    assert!(job.state().observed_key_peak_bytes.load(Ordering::Acquire) > 0);
    job.state().retire_pcm().unwrap();
    assert!(jobs.busy.load(Ordering::Acquire));
    assert!(job.state().staged_pcm.lock().unwrap().is_none());
    std::fs::remove_file(path).unwrap();
    job.state().finish(&envelope(&job)).unwrap();
    assert!(!jobs.busy.load(Ordering::Acquire));
}

#[test]
fn failed_partial_export_retires_source_before_cleanup_without_publishing_pcm() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let mut samples = vec![0.25; 8193];
    *samples.last_mut().unwrap() = f32::NAN;
    let source = SampleBuffer {
        samples: samples.into(),
        channels: 1,
    };
    let weak = Arc::downgrade(&source.samples);
    let job = jobs
        .begin(0, source, 96_000, 1, fresh_owner(&ids), tx.clone())
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("partial.f32");
    assert!(
        job.state()
            .prepare(path.to_str().unwrap())
            .unwrap_err()
            .contains("non-finite")
    );
    assert!(path.metadata().unwrap().len() < 8193 * 4);
    assert!(job.state().staged_pcm.lock().unwrap().is_none());
    assert!(weak.upgrade().is_some());
    assert!(
        jobs.begin(
            0,
            sample(),
            96_000,
            2,
            OfflineRequestOwner {
                request_ids: ids.clone(),
                prepared_epoch: Arc::new(AtomicU64::new(1))
            },
            tx.clone()
        )
        .is_err()
    );
    job.cancel();
    job.state().retire_pcm().unwrap();
    assert!(weak.upgrade().is_none());
    assert!(job.state().key().is_err());
    assert!(job.state().prepare(path.to_str().unwrap()).is_err());
    assert!(jobs.busy.load(Ordering::Acquire));
    std::fs::remove_file(path).unwrap();
    assert!(!job.state().finish(&envelope(&job)).unwrap());
    assert_eq!(completions(&rx), 0);
    assert!(
        jobs.begin(
            0,
            sample(),
            96_000,
            2,
            OfflineRequestOwner {
                request_ids: ids,
                prepared_epoch: Arc::new(AtomicU64::new(1))
            },
            tx
        )
        .is_ok()
    );
}

#[test]
fn cancelled_prepared_file_closes_before_cleanup_without_key_start() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let job = begin(&jobs, &ids, &tx);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mono.f32");
    job.state().prepare(path.to_str().unwrap()).unwrap();
    #[cfg(windows)]
    {
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(std::fs::remove_file(&path).is_err());
    }
    assert_eq!(std::fs::read(&path).unwrap().len(), 4096);
    job.cancel();
    job.state().retire_pcm().unwrap();
    job.state().retire_pcm().unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(jobs.busy.load(Ordering::Acquire));
    assert!(!job.state().finish(&envelope(&job)).unwrap());
    assert_eq!(completions(&rx), 0);
}

#[test]
fn valid_components_publish_once_and_keep_loaded_source_identity() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![7]));
    let (tx, rx) = channel();
    let job = jobs
        .begin(0, sample(), 44_100, 7, fresh_owner(&ids), tx.clone())
        .unwrap();
    assert_eq!(job.state().identity.source_generation, 7);
    assert_eq!(job.state().identity.request_id, 8);
    job.state().finish(&envelope(&job)).unwrap();
    assert!(job.state().finish(&envelope(&job)).is_err());
    assert_eq!(completions(&rx), 1);
    let next = jobs
        .begin(0, sample(), 44_100, 7, fresh_owner(&ids), tx)
        .unwrap();
    assert_eq!(next.state().identity.source_generation, 7);
    assert_eq!(next.state().identity.request_id, 9);
}

#[test]
fn offline_admission_rejects_either_counter_exhaustion_without_changing_ownership() {
    for (request, prepared_epoch) in [(u64::MAX, 7), (7, u64::MAX)] {
        let jobs = OfflineJobs::default();
        let ids = Arc::new(Mutex::new(vec![request]));
        let epoch = Arc::new(AtomicU64::new(prepared_epoch));
        let (tx, rx) = channel();
        let source = sample();
        assert!(
            jobs.begin(
                0,
                source.clone(),
                48_000,
                1,
                OfflineRequestOwner {
                    request_ids: ids.clone(),
                    prepared_epoch: epoch.clone()
                },
                tx
            )
            .is_err()
        );
        assert_eq!(ids.lock().unwrap()[0], request);
        assert_eq!(epoch.load(Ordering::Acquire), prepared_epoch);
        assert!(!jobs.busy.load(Ordering::Acquire));
        assert!(jobs.current.lock().unwrap().upgrade().is_none());
        assert_eq!(Arc::strong_count(&source.samples), 1);
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn cancellation_overflow_cancels_teardown_but_preserves_both_shared_counters() {
    for (request, prepared_epoch) in [(u64::MAX - 1, 7), (7, u64::MAX - 1)] {
        let jobs = OfflineJobs::default();
        let ids = Arc::new(Mutex::new(vec![request]));
        let epoch = Arc::new(AtomicU64::new(prepared_epoch));
        let (tx, rx) = channel();
        let job = jobs
            .begin(
                0,
                sample(),
                48_000,
                1,
                OfflineRequestOwner {
                    request_ids: ids.clone(),
                    prepared_epoch: epoch.clone(),
                },
                tx,
            )
            .unwrap();
        let current_request = ids.lock().unwrap()[0];
        let current_epoch = epoch.load(Ordering::Acquire);

        assert!(job.state().cancel_request().is_err());
        assert!(job.is_cancelled());
        assert_eq!(ids.lock().unwrap()[0], current_request);
        assert_eq!(epoch.load(Ordering::Acquire), current_epoch);
        assert!(!job.state().finish(&envelope(&job)).unwrap());
        assert_eq!(completions(&rx), 0);
        assert!(!jobs.busy.load(Ordering::Acquire));
        assert_eq!(ids.lock().unwrap()[0], current_request);
        assert_eq!(epoch.load(Ordering::Acquire), current_epoch);
    }
}

#[test]
fn superseded_offline_cancellation_preserves_the_new_preparation_owner() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![7]));
    let epoch = Arc::new(AtomicU64::new(1));
    let (tx, rx) = channel();
    let job = jobs
        .begin(
            0,
            sample(),
            48_000,
            1,
            OfflineRequestOwner {
                request_ids: ids.clone(),
                prepared_epoch: epoch.clone(),
            },
            tx,
        )
        .unwrap();
    let newer_request = next_pad_request_id(&ids, 0, &epoch).unwrap();
    let newer_epoch = epoch.load(Ordering::Acquire);

    job.state().cancel_request().unwrap();
    assert!(job.is_cancelled());
    assert_eq!(ids.lock().unwrap()[0], newer_request);
    assert_eq!(epoch.load(Ordering::Acquire), newer_epoch);
    assert!(!job.state().finish(&envelope(&job)).unwrap());
    assert_eq!(completions(&rx), 0);
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
    next_pad_request_id(&ids, 0, &job.state().prepared_epoch).unwrap();
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
        jobs.begin(
            0,
            sample(),
            0,
            1,
            OfflineRequestOwner {
                request_ids: ids.clone(),
                prepared_epoch: Arc::new(AtomicU64::new(1))
            },
            tx.clone()
        )
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
    assert!(
        jobs.begin(
            0,
            sample(),
            48_000,
            1,
            OfflineRequestOwner {
                request_ids: ids,
                prepared_epoch: Arc::new(AtomicU64::new(1))
            },
            tx
        )
        .is_err()
    );
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

#[test]
fn producer_keys_and_legacy_flat_aliases_retire_and_publish_unchanged() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let producer_keys = (0..24).map(|index| analysis::camelot_index_to_key(index).unwrap());
    let legacy_aliases = ["Abm", "Ebm", "Bbm", "Ab", "Eb", "Bb"];
    for key in producer_keys.chain(legacy_aliases) {
        let job = begin(&jobs, &ids, &tx);
        assert!(job.state().snapshot.lock().unwrap().is_some());
        let mut ready: Value = serde_json::from_str(&envelope(&job)).unwrap();
        ready["key"] = json!({"status": "ready", "key": key});
        assert!(job.state().finish(&ready.to_string()).unwrap(), "{key}");
        assert!(job.state().finished.load(Ordering::Acquire), "{key}");
        assert!(!jobs.busy.load(Ordering::Acquire), "{key}");
        assert!(job.state().snapshot.lock().unwrap().is_none(), "{key}");
        let completed: Vec<_> = rx
            .try_iter()
            .filter_map(|event| match event {
                LoaderEvent::OfflineAnalysisCompleted { result_json, .. } => Some(result_json),
                _ => None,
            })
            .collect();
        assert_eq!(completed.len(), 1, "{key}");
        let published: Value = serde_json::from_str(&completed[0]).unwrap();
        assert_eq!(published["key"]["key"], key);
    }
}

#[test]
fn malformed_ready_keys_do_not_publish_or_release_reservation() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let job = begin(&jobs, &ids, &tx);
    for key in [
        json!(""),
        json!("unknown"),
        json!("g#m"),
        json!("G#m "),
        json!("G##m"),
        json!("Cmajor"),
        json!("Db"),
        json!("G♯m"),
        json!(null),
        json!(24),
        json!(true),
    ] {
        let mut invalid: Value = serde_json::from_str(&envelope(&job)).unwrap();
        invalid["key"] = json!({"status": "ready", "key": key});
        assert_eq!(
            job.state().finish(&invalid.to_string()).unwrap_err(),
            "invalid ready key component",
            "{key}"
        );
        assert!(!job.state().finished.load(Ordering::Acquire), "{key}");
        assert!(jobs.busy.load(Ordering::Acquire), "{key}");
        assert!(job.state().snapshot.lock().unwrap().is_some(), "{key}");
        assert_eq!(completions(&rx), 0, "{key}");
    }
    job.state().finish(&envelope(&job)).unwrap();
}

#[test]
fn packed_long_result_publishes_once_without_expanding_or_changing_values() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let job = begin(&jobs, &ids, &tx);
    let ready = ready_envelope(&job, 30_000);
    let wire = ready.to_string();
    assert!(wire.len() < MAX_RESULT_BYTES);
    assert!(job.state().finish(&wire).unwrap());
    assert!(job.state().finish(&wire).is_err());
    let published: Vec<Value> = rx
        .try_iter()
        .filter_map(|event| match event {
            LoaderEvent::OfflineAnalysisCompleted { result_json, .. } => {
                Some(serde_json::from_str(&result_json).unwrap())
            }
            _ => None,
        })
        .collect();
    assert_eq!(published, [ready]);
    assert!(job.state().snapshot.lock().unwrap().is_none());
    assert!(job.state().staged_pcm.lock().unwrap().is_none());
    assert!(!jobs.busy.load(Ordering::Acquire));
    assert!(
        jobs.begin(
            0,
            sample(),
            48_000,
            1,
            OfflineRequestOwner {
                request_ids: ids,
                prepared_epoch: Arc::new(AtomicU64::new(1))
            },
            tx
        )
        .is_ok()
    );
}

#[test]
fn invalid_packed_result_keeps_native_admission_until_valid_retirement() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let job = begin(&jobs, &ids, &tx);
    let mut invalid = ready_envelope(&job, 1);
    invalid["beat"]["predictions"]["beat_logits"] = json!("AAAAAAAAAAB=");
    assert!(job.state().finish(&invalid.to_string()).is_err());
    assert!(jobs.busy.load(Ordering::Acquire));
    assert!(job.state().snapshot.lock().unwrap().is_some());
    assert!(!job.state().finished.load(Ordering::Acquire));
    assert_eq!(completions(&rx), 0);
    assert!(
        jobs.begin(
            0,
            sample(),
            48_000,
            1,
            OfflineRequestOwner {
                request_ids: ids,
                prepared_epoch: Arc::new(AtomicU64::new(1))
            },
            tx
        )
        .is_err()
    );
    assert!(
        job.state()
            .finish(&ready_envelope(&job, 1).to_string())
            .unwrap()
    );
    assert_eq!(completions(&rx), 1);
}

#[test]
fn cancelled_or_stale_packed_result_retires_without_publication() {
    for cancelled in [false, true] {
        let jobs = OfflineJobs::default();
        let ids = Arc::new(Mutex::new(vec![1]));
        let (tx, rx) = channel();
        let job = begin(&jobs, &ids, &tx);
        if cancelled {
            job.cancel();
        } else {
            next_pad_request_id(&ids, 0, &job.state().prepared_epoch).unwrap();
        }
        let ready = ready_envelope(&job, 1).to_string();
        job.state().key_running.store(true, Ordering::Release);
        assert!(job.state().finish(&ready).is_err());
        assert!(jobs.busy.load(Ordering::Acquire));
        assert!(job.state().snapshot.lock().unwrap().is_some());
        job.state().key_running.store(false, Ordering::Release);
        assert!(!job.state().finish(&ready).unwrap());
        assert_eq!(completions(&rx), 0);
        assert!(job.state().snapshot.lock().unwrap().is_none());
        assert!(
            jobs.begin(
                0,
                sample(),
                48_000,
                2,
                OfflineRequestOwner {
                    request_ids: ids,
                    prepared_epoch: Arc::new(AtomicU64::new(1))
                },
                tx
            )
            .is_ok()
        );
    }
}

#[test]
fn packed_result_still_obeys_final_byte_cap() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let job = begin(&jobs, &ids, &tx);
    let oversized = ready_envelope(&job, 150_000).to_string();
    assert!(oversized.len() > MAX_RESULT_BYTES);
    assert_eq!(
        job.state().finish(&oversized).unwrap_err(),
        "offline result limit exceeded"
    );
    assert!(jobs.busy.load(Ordering::Acquire));
    assert_eq!(completions(&rx), 0);
    job.cancel();
    job.state().finish(&envelope(&job)).unwrap();
}

#[test]
fn schema_versions_and_nonready_packed_predictions_are_explicit() {
    let jobs = OfflineJobs::default();
    let ids = Arc::new(Mutex::new(vec![1]));
    let (tx, rx) = channel();
    let job = begin(&jobs, &ids, &tx);
    let original = ready_envelope(&job, 1);
    for version in [
        json!(0),
        json!(3),
        json!(2.0),
        json!("2"),
        json!(true),
        json!(null),
    ] {
        let mut invalid = original.clone();
        invalid["schema_version"] = version;
        assert!(job.state().finish(&invalid.to_string()).is_err());
    }
    for status in ["failed", "unavailable", "cancelled"] {
        let mut invalid = original.clone();
        invalid["beat"]["status"] = json!(status);
        assert_eq!(
            job.state().finish(&invalid.to_string()).unwrap_err(),
            "unsuccessful beat component has predictions"
        );
    }
    let mut legacy = original;
    legacy["schema_version"] = json!(1);
    legacy["beat"]["predictions"] = json!({"beat_seconds": [0.0, 0.001, 0.02],
        "downbeat_seconds": [0.001], "beat_logits": [0.5], "downbeat_logits": [0.1]});
    assert!(job.state().finish(&legacy.to_string()).unwrap());
    assert_eq!(completions(&rx), 1);
}

#[test]
#[ignore = "requires private complete worker evidence"]
fn complete_private_worker_envelopes_pass_native_publication_validation() {
    let evidence_dir = std::path::PathBuf::from(
        std::env::var_os("FLITZIS_PUBLICATION_EVIDENCE_DIR")
            .expect("FLITZIS_PUBLICATION_EVIDENCE_DIR must identify complete private evidence"),
    );
    assert!(
        evidence_dir.is_absolute(),
        "evidence directory must be absolute"
    );
    for track_id in ["T04", "T05", "R01"] {
        let directory = evidence_dir.join(format!("worker-{track_id}"));
        let request_path = directory.join("worker-request.json");
        assert!(std::fs::metadata(&request_path).unwrap().len() <= 32 * 1024);
        let request: Value =
            serde_json::from_str(&std::fs::read_to_string(request_path).unwrap()).unwrap();
        assert_eq!(request["schema_version"], 1, "{track_id}");
        assert_eq!(request["pcm"]["origin_seconds"], 0.0, "{track_id}");
        let id = &request["identity"];
        let identity = PcmIdentity {
            pad_id: usize::try_from(id["pad_id"].as_u64().unwrap()).unwrap(),
            request_id: id["request_id"].as_u64().unwrap(),
            source_id: id["source_id"].as_str().unwrap().to_owned(),
            source_generation: id["source_generation"].as_u64().unwrap(),
        };
        let frames = request["pcm"]["frame_count"].as_u64().unwrap();
        let rate = request["pcm"]["sample_rate_hz"].as_u64().unwrap();
        assert!(frames > 0 && rate > 0, "{track_id}");
        let duration = frames as f64 / rate as f64;
        let envelope_path = directory.join("final-envelope.actual-probe.json");
        assert!(std::fs::metadata(&envelope_path).unwrap().len() <= MAX_RESULT_BYTES as u64);
        let wire = std::fs::read_to_string(envelope_path).unwrap();
        let original: Value = serde_json::from_str(&wire).unwrap();
        let validated = validate_envelope(&wire, &identity, duration).unwrap();
        assert_eq!(validated["schema_version"], 2, "{track_id}");
        assert_eq!(validated["beat"]["status"], "ready", "{track_id}");
        assert_eq!(validated["beat"]["model"], request["model"], "{track_id}");
        assert_eq!(
            validated["beat"]["predictions"], original["beat"]["predictions"],
            "{track_id}"
        );
        for name in [
            "beat_seconds",
            "downbeat_seconds",
            "beat_logits",
            "downbeat_logits",
        ] {
            assert!(
                validated["beat"]["predictions"][name].is_string(),
                "{track_id}/{name}"
            );
        }
        let republished = validated.to_string();
        assert!(republished.len() <= MAX_RESULT_BYTES, "{track_id}");
        assert_eq!(
            validate_envelope(&republished, &identity, duration).unwrap(),
            validated,
            "{track_id}"
        );
    }
}
