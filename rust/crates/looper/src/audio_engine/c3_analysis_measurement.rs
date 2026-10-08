//! Real complete-source analysis, export and cancellation observations.
use super::*;
use crate::audio_engine::analysis_jobs::OfflineRequestOwner;
use crate::audio_engine::complete_context::CompleteSourceReader;

fn job_stats(job: &crate::audio_engine::OfflineAnalysisJob) -> Value {
    Python::attach(|py| {
        let data = job.staging_stats(py).unwrap();
        let text: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (data,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&text).unwrap()
    })
}

pub(super) fn analyze(engine: &Py<AudioEngine>, root: &Path, expected: &Value) -> Value {
    let export = root.join("complete-analysis-mono.f32le");
    let (job, bank) = Python::attach(|py| {
        let owner = engine.borrow(py);
        let sample = owner.sample_cache.lock().unwrap()[0].clone().unwrap();
        let generation = owner.loaded_source_generations.lock().unwrap()[0].0;
        let reader = CompleteSourceReader::capture(&owner, 0, sample.clone()).unwrap();
        let job = owner
            .offline_jobs
            .begin_complete(
                0,
                reader,
                48_000,
                generation,
                OfflineRequestOwner {
                    request_ids: owner.pad_request_ids.clone(),
                    prepared_epoch: owner.prepared_source_epochs[0].clone(),
                },
                owner.loader_tx.clone(),
            )
            .unwrap();
        (job, sample)
    });
    let streamed = measure(|| {
        println!("C3 analysis: complete mono export");
        Python::attach(|py| {
            job.prepare_export(py, export.to_string_lossy().into_owned())
                .unwrap()
        });
        let stats = job_stats(&job);
        assert_eq!(fs::metadata(&export).unwrap().len(), 23_040_000);
        json!({"staging_stats":stats, "full_mono_bytes":23_040_000})
    });
    assert_eq!(full_hash(&export), expected["decoder_sha256"]);
    let key = measure(|| {
        println!("C3 analysis: actual offline key branch");
        let key = Python::attach(|py| job.analyze_key(py).unwrap());
        json!({"actual_key":key, "staging_stats":job_stats(&job),
            "limit":"synthetic source; analysis completion, no musical label acceptance"})
    });
    let retirement = measure(|| {
        println!("C3 analysis: PCM retirement");
        Python::attach(|py| job.retire_pcm(py).unwrap());
        json!({"staging_stats":job_stats(&job)})
    });
    drop(job);
    wait_until(Duration::from_secs(30), || {
        Python::attach(|py| !engine.borrow(py).offline_jobs.has_pad(0))
    });
    let native = measure(|| {
        println!("C3 analysis: actual legacy native task");
        let request = Python::attach(|py| {
            crate::audio_engine::complete_context::start_analysis(&engine.borrow(py), 0).unwrap()
        });
        let deadline = Instant::now() + Duration::from_secs(3600);
        let mut stages = Vec::new();
        loop {
            let event = Python::attach(|py| engine.borrow(py).loader_rx.lock().unwrap().try_recv());
            if let Ok(event) = event {
                match event {
                    LoaderEvent::TaskSuccess {
                        id: 0,
                        request_id,
                        analysis,
                        ..
                    } if request_id == request => {
                        assert!(analysis.is_some());
                        return json!({"request":request, "terminal":"success", "stages":stages});
                    }
                    LoaderEvent::TaskError {
                        id: 0,
                        request_id,
                        error,
                        ..
                    } if request_id == request => {
                        return json!({"request":request, "terminal":"error", "error":error, "stages":stages});
                    }
                    LoaderEvent::TaskProgress {
                        id: 0,
                        request_id,
                        stage,
                        ..
                    } if request_id == request => {
                        stages.push(stage);
                    }
                    _ => {}
                }
            }
            assert!(
                Instant::now() < deadline,
                "productive legacy native analysis timeout"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    wait_until(Duration::from_secs(30), || {
        Python::attach(|py| engine.borrow(py).active_tasks.lock().unwrap().is_empty())
    });
    Python::attach(|py| {
        let owner = engine.borrow(py);
        assert!(
            owner.sample_cache.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .same_window(&bank)
        );
        assert!(
            owner
                .input_runtime_ownership
                .source_current(0, &bank, 48_000)
        );
    });
    let cancelled = measure(|| {
        println!("C3 analysis: cancelled offline export");
        let job = Python::attach(|py| {
            let owner = engine.borrow(py);
            let reader = CompleteSourceReader::capture(&owner, 0, bank.clone()).unwrap();
            // Admission rechecks this generation. End the mutex guard before
            // invoking begin_complete(), which locks the same generation list.
            let generation = owner.loaded_source_generations.lock().unwrap()[0].0;
            owner
                .offline_jobs
                .begin_complete(
                    0,
                    reader,
                    48_000,
                    generation,
                    OfflineRequestOwner {
                        request_ids: owner.pad_request_ids.clone(),
                        prepared_epoch: owner.prepared_source_epochs[0].clone(),
                    },
                    owner.loader_tx.clone(),
                )
                .unwrap()
        });
        job.cancel();
        assert!(job.is_cancelled());
        let failure = Python::attach(|py| {
            job.prepare_export(
                py,
                root.join("cancelled-export.f32le")
                    .to_string_lossy()
                    .into_owned(),
            )
            .unwrap_err()
            .to_string()
        });
        assert!(!root.join("cancelled-export.f32le").exists());
        Python::attach(|py| job.retire_pcm(py).unwrap());
        job.abort_unstarted().unwrap();
        let stats = job_stats(&job);
        drop(job);
        wait_until(Duration::from_secs(30), || {
            Python::attach(|py| !engine.borrow(py).offline_jobs.has_pad(0))
        });
        json!({"cancelled":true, "error":failure, "staging_stats":stats,
            "cancelled_export_absent":true, "actual_offline_reservation_released":true})
    });
    json!({"complete_export":streamed, "key_analysis":key, "retirement":retirement,
        "native_legacy_analysis":native, "cancelled_offline_export":cancelled})
}
