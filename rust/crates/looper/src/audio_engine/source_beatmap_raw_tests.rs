// Genuine source-qualified jobs, detector bytes and owner retirement. No device/listening claim.
use super::*;
use crate::audio_engine::SourceBeatmapRawCandidate;
use crate::audio_engine::cold_residency::ResidentLoadHint;
use crate::audio_engine::source_beatmap_artifact::StorePoint;
use pyo3::types::PyList;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

// The product admission is process-wide. Only these raw tests share this test
// serialization; every job still passes the real Native admission and lane.
static RAW_TEST_GATE: Mutex<()> = Mutex::new(());

fn generation(fixture: &ColdCustody) -> u64 {
    fixture.engine.loaded_source_generations.lock().unwrap()[0].0
}

fn raw_event(fixture: &ColdCustody) -> Py<PyAny> {
    let mut result = None;
    wait_until(|| {
        Python::attach(|py| {
            while let Some(event) = fixture.engine.poll_loader_events(py).unwrap() {
                let kind: String = event
                    .bind(py)
                    .cast::<PyDict>()
                    .unwrap()
                    .get_item("type")
                    .unwrap()
                    .unwrap()
                    .extract()
                    .unwrap();
                if kind == "source_beatmap_raw_completed" {
                    result = Some(event);
                }
                assert_ne!(kind, "offline_analysis_completed");
            }
        });
        result.is_some()
    });
    result.unwrap()
}

fn candidate(
    job: &OfflineAnalysisJob,
    owner: &Py<LoaderEventDelivery>,
) -> SourceBeatmapRawCandidate {
    Python::attach(|py| {
        job.resolve_source_beatmap_raw_completion(&owner.borrow(py))
            .unwrap()
            .unwrap()
    })
}

fn query(candidate: &SourceBeatmapRawCandidate, page: Option<(i64, i64)>) -> Value {
    Python::attach(|py| {
        let object = Py::new(py, candidate.clone()).unwrap();
        let value = if let Some((start, maximum)) = page {
            object
                .bind(py)
                .call_method1("inspect", (start, maximum))
                .unwrap()
        } else {
            object.bind(py).call_method0("info").unwrap()
        };
        let encoded: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (value,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    })
}

struct ObservedEvent {
    raw_index: usize,
    odf_bits: u64,
    seconds_bits: u64,
    detected_downbeat: bool,
}

fn query_event_bits(candidate: &SourceBeatmapRawCandidate, start: usize) -> Vec<ObservedEvent> {
    Python::attach(|py| {
        let object = Py::new(py, candidate.clone()).unwrap();
        let page = object
            .bind(py)
            .call_method1("inspect", (start, 128))
            .unwrap();
        let page = page.cast::<PyDict>().unwrap();
        assert_eq!(
            page.get_item("status")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "ready"
        );
        let events = page.get_item("events").unwrap().unwrap();
        // Extract real Python binary64 values directly. A decimal JSON reparse
        // in a test must not change the original-bit acceptance criterion.
        events
            .cast::<PyList>()
            .unwrap()
            .iter()
            .map(|event| {
                let row = event.cast::<PyDict>().unwrap();
                ObservedEvent {
                    raw_index: row
                        .get_item("raw_index")
                        .unwrap()
                        .unwrap()
                        .extract()
                        .unwrap(),
                    odf_bits: row
                        .get_item("odf_frame")
                        .unwrap()
                        .unwrap()
                        .extract::<f64>()
                        .unwrap()
                        .to_bits(),
                    seconds_bits: row
                        .get_item("source_seconds")
                        .unwrap()
                        .unwrap()
                        .extract::<f64>()
                        .unwrap()
                        .to_bits(),
                    detected_downbeat: row
                        .get_item("detected_downbeat")
                        .unwrap()
                        .unwrap()
                        .extract()
                        .unwrap(),
                }
            })
            .collect()
    })
}

fn wait_review_retired(fixture: &ColdCustody, job: &OfflineAnalysisJob) {
    wait_until(|| {
        fixture.engine.project_assets.collect_for_test();
        async_status(job)["terminal"] == true && async_status(job)["raw_review_retired"] == true
    });
}

fn signal() -> Vec<i16> {
    (0..48_000 * 12 + 37)
        .map(|frame| {
            if frame < 8193 {
                return 0;
            }
            let within = (frame - 8193) % 24_000;
            let envelope = if within < 1200 {
                (1200 - within) as f64 / 1200.0
            } else {
                0.03
            };
            (envelope * (frame as f64 * std::f64::consts::TAU * 130.0 / 48_000.0).sin() * 24_000.0)
                as i16
        })
        .collect()
}

#[test]
fn raw_complete_and_finite_original_jobs_retain_actual_hashes_and_detector_binary64_bits() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let pcm = signal();
    let mono: Vec<f32> = pcm
        .iter()
        .map(|sample| f32::from(*sample) / 32768.0)
        .collect();
    let expected_mono = format!(
        "{:x}",
        Sha256::digest(
            mono.iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>()
        )
    );
    let converted = crate::audio_engine::analysis_pcm::resample_mono_cancellable(
        mono,
        48_000,
        44_100,
        512 * 1024 * 1024,
        &|| false,
    )
    .unwrap();
    let analyzer: Vec<f64> = converted.into_iter().map(f64::from).collect();
    let expected_analyzer = format!(
        "{:x}",
        Sha256::digest(
            analyzer
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>()
        )
    );
    let expected = flitzis_looper_analysis::analyze_bpm_raw(
        &analyzer,
        44_100,
        &flitzis_looper_analysis::AnalysisConfig::default(),
    )
    .unwrap();
    assert!(
        !expected.beat_frames().is_empty(),
        "the original-bit oracle must exercise actual detections"
    );
    let mut revisions = Vec::new();
    for finite in [false, true] {
        let hint = finite.then_some(ResidentLoadHint {
            start_s: 0.25,
            end_s: 0.5,
            key_lock: false,
        });
        let fixture = ColdCustody::new_selected_with_signal(hint, &pcm);
        let source = fixture.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap();
        if finite {
            assert!(source.resident_end() < source.frame_count());
        }
        let fences = source_fences(&fixture);
        let job = fixture
            .engine
            .begin_source_beatmap_raw_async(0, generation(&fixture))
            .unwrap();
        assert_eq!(wait_terminal(&job)["stage"], "ready");
        let event = raw_event(&fixture);
        let owner = delivery(&event);
        let candidate = candidate(&job, &owner);
        let info = query(&candidate, None);
        assert_eq!(info["source_frame_count"], pcm.len());
        assert_eq!(info["mono_sha256"], expected_mono);
        assert_eq!(info["analyzer_sha256"], expected_analyzer);
        assert_eq!(info["analyzer_frame_count"], analyzer.len());
        assert_eq!(info["odf_hop_samples"], expected.odf_hop_samples());
        assert_eq!(info["interpretation"], "unverified");
        assert_eq!(info["timing_uncertainty"], "unknown");
        assert_eq!(async_status(&job)["raw_review_retired"], false);
        let mut rows = Vec::new();
        for start in (0..expected.beat_frames().len()).step_by(128) {
            rows.extend(query_event_bits(&candidate, start));
        }
        for (index, (row, frame)) in rows.iter().zip(expected.beat_frames()).enumerate() {
            assert_eq!(row.raw_index, index);
            assert_eq!(row.odf_bits, frame.to_bits());
            assert_eq!(
                row.seconds_bits,
                expected.beat_seconds().nth(index).unwrap().to_bits()
            );
            assert_eq!(
                row.detected_downbeat,
                expected.downbeat_raw_indices().contains(&index)
            );
        }
        assert_eq!(rows.len(), expected.beat_frames().len());
        assert!(
            query(&candidate, Some((rows.len() as i64, 128)))["events"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        Python::attach(|py| {
            let object = Py::new(py, candidate.clone()).unwrap();
            for args in [(-1, 128), (0, 0), (0, 129), (rows.len() as i64 + 1, 1)] {
                assert!(object.bind(py).call_method1("inspect", args).is_err());
            }
            for forbidden in ["map", "receipt_json", "from_json", "acknowledge_export"] {
                assert!(!object.bind(py).hasattr(forbidden).unwrap());
            }
            assert!(
                job.resolve_source_beatmap_completion(&owner.borrow(py))
                    .unwrap()
                    .is_none()
            );
            assert!(
                job.resolve_source_beatmap_artifact_completion(&owner.borrow(py))
                    .unwrap()
                    .is_none()
            );
        });
        assert!(
            fixture
                .engine
                .begin_source_beatmap_raw_async(1, generation(&fixture))
                .is_err()
        );
        assert_eq!(source_fences(&fixture), fences);
        revisions.push(info["raw_revision"].clone());
        job.cancel();
        assert_eq!(query(&candidate, Some((0, 128)))["status"], "cancelled");
        assert!(
            query(&candidate, Some((0, 128)))["events"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        drop((candidate, owner, event));
        wait_review_retired(&fixture, &job);
    }
    assert_eq!(
        revisions[0], revisions[1],
        "resident window is not detector input"
    );
}

#[test]
fn raw_job_cancel_and_ready_review_preserve_original_accepted_delivery_and_allow_accepted_edit() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let accepted = fixture
        .engine
        .begin_source_beatmap_store_async(0, record(&fixture, 0).to_string())
        .unwrap();
    wait_terminal(&accepted);
    let event = maps(&fixture).pop().unwrap();
    let accepted_owner = delivery(&event);
    let artifact = Python::attach(|py| {
        accepted
            .resolve_source_beatmap_artifact_completion(&accepted_owner.borrow(py))
            .unwrap()
            .unwrap()
    });
    let fences = source_fences(&fixture);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let job = fixture
        .engine
        .begin_source_beatmap_raw_async_with(0, generation(&fixture), move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        })
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(resolved_record(&accepted, &accepted_owner).is_some());
    job.cancel();
    assert_eq!(async_status(&job)["terminal"], false);
    assert_eq!(async_status(&job)["raw_review_retired"], false);
    assert!(resolved_record(&accepted, &accepted_owner).is_some());
    release_tx.send(()).unwrap();
    wait_review_retired(&fixture, &job);
    assert!(resolved_record(&accepted, &accepted_owner).is_some());
    assert_eq!(source_fences(&fixture), fences);
    let raw = fixture
        .engine
        .begin_source_beatmap_raw_async(0, generation(&fixture))
        .unwrap();
    wait_terminal(&raw);
    let raw_event = raw_event(&fixture);
    let raw_owner = delivery(&raw_event);
    let candidate = candidate(&raw, &raw_owner);
    let edit = fixture.engine.begin_source_beatmap_edit_async(0, artifact.receipt_json().to_owned(), json!({"schema_version":1,"operation":"translate","sample_offset":1,"sample_rate_hz":48_000}).to_string()).unwrap();
    assert_eq!(wait_terminal(&edit)["stage"], "ready");
    assert_eq!(query(&candidate, None)["status"], "ready");
    assert!(resolved_record(&accepted, &accepted_owner).is_some());
    assert_eq!(source_fences(&fixture), fences);
    raw.cancel();
    drop((candidate, raw_owner, raw_event));
    wait_review_retired(&fixture, &raw);
}

#[test]
fn raw_original_private_admission_follows_move_but_refuses_equal_request_peer_and_source_aba() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut fixture = ColdCustody::new();
    let job = fixture
        .engine
        .begin_source_beatmap_raw_async(0, generation(&fixture))
        .unwrap();
    wait_terminal(&job);
    let event = raw_event(&fixture);
    let owner = delivery(&event);
    let candidate = candidate(&job, &owner);
    let peer = fixture.engine.begin_source_beatmap(1).unwrap();
    Python::attach(|py| {
        assert!(
            peer.resolve_source_beatmap_raw_completion(&owner.borrow(py))
                .unwrap()
                .is_none()
        )
    });
    peer.abort_unstarted().unwrap();
    wait_until(|| !fixture.engine.offline_jobs.has_pad(1));
    let ticket = own_pair(&mut fixture, 1);
    assert_eq!(
        query(&candidate, Some((0, 1)))["status"],
        "placement_pending"
    );
    assert!(
        query(&candidate, Some((0, 1)))["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(pair::reconcile(&fixture.engine, &ticket).unwrap());
    assert_eq!(query(&candidate, None)["status"], "ready");
    fixture.engine.input_runtime_ownership.revoke_source(1);
    assert_eq!(query(&candidate, Some((0, 1)))["status"], "stale");
    assert!(
        query(&candidate, Some((0, 1)))["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    Python::attach(|py| {
        assert!(
            job.resolve_source_beatmap_raw_completion(&owner.borrow(py))
                .unwrap()
                .is_none()
        )
    });
    job.cancel();
    drop((candidate, owner, event));
    wait_review_retired(&fixture, &job);
}

#[test]
fn queued_raw_cancel_and_queue_discard_retain_review_and_source_until_actual_retirement() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for close in [false, true] {
        let fixture = ColdCustody::new();
        let fences = source_fences(&fixture);
        let held = occupy_workers(&fixture);
        let job = fixture
            .engine
            .begin_source_beatmap_raw_async(0, generation(&fixture))
            .unwrap();
        assert_eq!(async_status(&job)["stage"], "queued");
        job.cancel();
        assert_eq!(async_status(&job)["terminal"], false);
        assert_eq!(async_status(&job)["raw_review_retired"], false);
        if close {
            fixture.engine.cold_jobs.close_admission();
        } else {
            held.release();
        }
        wait_review_retired(&fixture, &job);
        assert_eq!(source_fences(&fixture), fences);
        assert!(
            !async_status(&job)
                .as_object()
                .unwrap()
                .contains_key("map_revision")
        );
        held.release();
        wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    }
}

#[test]
fn raw_artifact_reopen_tamper_refuses_and_preserves_unknown_replacements() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for replace in [false, true] {
        let fixture = ColdCustody::new();
        let seen = Arc::new(Mutex::new(None::<PathBuf>));
        let observed = seen.clone();
        let displaced = fixture.directory.path().join("displaced-owned-raw.json");
        let job = fixture
            .engine
            .begin_source_beatmap_raw_async_with_hooks(
                0,
                generation(&fixture),
                || {},
                move |point, directory| {
                    if point == StorePoint::AfterWrite {
                        *observed.lock().unwrap() = Some(directory.to_owned());
                        let leaf = directory.join("map.json");
                        if replace {
                            fs::rename(&leaf, &displaced).map_err(|error| error.to_string())?;
                        }
                        fs::write(&leaf, b"unknown replacement")
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(wait_terminal(&job)["stage"], "failed");
        wait_review_retired(&fixture, &job);
        assert_eq!(
            fs::read(seen.lock().unwrap().as_ref().unwrap().join("map.json")).unwrap(),
            b"unknown replacement"
        );
    }
}

#[test]
fn raw_final_reader_retirement_not_job_terminal_controls_reacquisition() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fixture = ColdCustody::new();
    let job = fixture
        .engine
        .begin_source_beatmap_raw_async(0, generation(&fixture))
        .unwrap();
    wait_terminal(&job);
    let event = raw_event(&fixture);
    let owner = delivery(&event);
    let candidate = candidate(&job, &owner);
    let last = candidate.clone();
    job.cancel();
    drop((candidate, owner, event));
    assert_eq!(async_status(&job)["terminal"], true);
    assert_eq!(async_status(&job)["raw_review_retired"], false);
    assert!(
        fixture
            .engine
            .begin_source_beatmap_raw_async(0, generation(&fixture))
            .is_err()
    );
    drop(last);
    wait_review_retired(&fixture, &job);
    let next = fixture
        .engine
        .begin_source_beatmap_raw_async(0, generation(&fixture))
        .unwrap();
    next.cancel();
    wait_review_retired(&fixture, &next);
}

#[test]
fn raw_arm_and_submit_failure_retire_the_original_early_review_lease_without_a_handle() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for after_arm in [false, true] {
        let fixture = ColdCustody::new();
        let fences = source_fences(&fixture);
        let refused = fixture
            .engine
            .begin_source_beatmap_raw_admission_fault_for_test(0, generation(&fixture), after_arm);
        assert!(refused.is_err());
        wait_until(|| {
            !fixture.engine.offline_jobs.has_pad(0)
                && !crate::audio_engine::project_assets::ProjectAssets::shared()
                    .raw_review_busy
                    .load(std::sync::atomic::Ordering::Acquire)
        });
        assert_eq!(source_fences(&fixture), fences);
        let lease = crate::audio_engine::source_beatmap_raw::RawReviewLease::reserve().unwrap();
        let receipt = lease.receipt();
        drop(lease);
        assert!(receipt.retired());
        assert_offline_available(&fixture);
    }
}
