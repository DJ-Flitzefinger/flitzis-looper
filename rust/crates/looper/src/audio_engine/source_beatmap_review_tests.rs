// Genuine Raw-to-accepted admission and reader retirement. No device/music claim.
use super::*;
use crate::audio_engine::SourceBeatmapArtifact;
use std::path::Path;

struct ReadyReview {
    job: OfflineAnalysisJob,
    event: Py<PyAny>,
    owner: Py<LoaderEventDelivery>,
    candidate: SourceBeatmapRawCandidate,
    intent: String,
    capture: Value,
    path: PathBuf,
}

impl ReadyReview {
    fn discard(self) -> OfflineAnalysisJob {
        self.job.cancel();
        drop((self.candidate, self.owner, self.event));
        self.job
    }
}

fn review_fixture() -> ColdCustody {
    ColdCustody::new_selected_with_signal(
        Some(ResidentLoadHint {
            start_s: 0.25,
            end_s: 0.5,
            key_lock: false,
        }),
        &signal(),
    )
}

fn ready_review(fixture: &ColdCustody) -> ReadyReview {
    assert!(maps(fixture).is_empty());
    let observed = Arc::new(Mutex::new(None::<PathBuf>));
    let captured = observed.clone();
    let job = fixture
        .engine
        .begin_source_beatmap_raw_async_with_hooks(
            0,
            generation(fixture),
            || {},
            move |point, directory| {
                if point == StorePoint::AfterRename {
                    *captured.lock().unwrap() = Some(directory.join("map.json"));
                }
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(wait_terminal(&job)["stage"], "ready");
    let event = raw_event(fixture);
    let owner = delivery(&event);
    let candidate = candidate(&job, &owner);
    let info = query(&candidate, None);
    let duration = info["source_frame_count"].as_u64().unwrap() as f64
        / info["source_sample_rate_hz"].as_u64().unwrap() as f64;
    let usable: Vec<_> = query_event_bits(&candidate, 0)
        .into_iter()
        .filter(|event| f64::from_bits(event.seconds_bits) < duration)
        .collect();
    assert!(
        usable.len() >= 3,
        "the genuine fixture must supply a strict pair"
    );
    // These are actual supplied labels; neither index spacing nor a default
    // meter supplies quarter coordinates, and intervening events remain raw.
    let intent = json!({
        "schema_version": 1,
        "raw_revision": info["raw_revision"],
        "anchors": [
            {"raw_index": usable[0].raw_index, "quarter_bits": bits(-2.5)},
            {"raw_index": usable[2].raw_index, "quarter_bits": bits(7.25)},
        ],
    })
    .to_string();
    let path = observed.lock().unwrap().clone().unwrap();
    let capture = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    ReadyReview {
        job,
        event,
        owner,
        candidate,
        intent,
        capture,
        path,
    }
}

fn begin_review(fixture: &ColdCustody, raw: &ReadyReview) -> OfflineAnalysisJob {
    fixture
        .engine
        .begin_source_beatmap_review_async(
            0,
            &raw.candidate,
            raw.intent.clone(),
            generation(fixture),
        )
        .unwrap()
}

fn assert_review_capture(job: &OfflineAnalysisJob, fixture: &ColdCustody, capture: &Value) {
    assert_eq!(wait_terminal(job)["stage"], "ready");
    let events = maps(fixture);
    assert_eq!(events.len(), 1);
    let owner = delivery(&events[0]);
    let accepted = resolved_record(job, &owner).unwrap();
    assert_eq!(accepted["schema_version"], 2);
    assert_eq!(accepted["encoding"], "source-beatmap-pwa-v2");
    assert_eq!(accepted["record"]["raw"]["capture"], *capture);
    assert_eq!(
        accepted["record"]["accepted"]["anchors"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for anchor in accepted["record"]["accepted"]["anchors"]
        .as_array()
        .unwrap()
    {
        assert_eq!(anchor["review"], "estimated");
    }
    let interval = &accepted["record"]["accepted"]["intervals"][0];
    assert_eq!(interval["coverage"], "uncertain");
    assert_eq!(interval["meter"]["status"], "unknown");
    assert_eq!(interval["meter"]["downbeat_label"], Value::Null);
    // Accepted delivery has its own cancellation authority after Raw retirement.
    job.cancel();
    assert!(resolved_record(job, &owner).is_none());
}

fn review_artifact(job: &OfflineAnalysisJob, fixture: &ColdCustody) -> SourceBeatmapArtifact {
    assert_eq!(wait_terminal(job)["stage"], "ready");
    let events = maps(fixture);
    assert_eq!(events.len(), 1);
    let owner = delivery(&events[0]);
    Python::attach(|py| {
        job.resolve_source_beatmap_artifact_completion(&owner.borrow(py))
            .unwrap()
            .unwrap()
    })
}

fn review_artifact_record(artifact: &SourceBeatmapArtifact) -> Value {
    serde_json::from_str(artifact.map().canonical_record()).unwrap()
}

fn review_edit_artifact(
    fixture: &ColdCustody,
    predecessor: &SourceBeatmapArtifact,
    operation: Value,
) -> SourceBeatmapArtifact {
    let before = review_artifact_record(predecessor);
    let job = fixture
        .engine
        .begin_source_beatmap_edit_async(
            0,
            predecessor.receipt_json().to_owned(),
            operation.to_string(),
        )
        .unwrap();
    let edited = review_artifact(&job, fixture);
    let after = review_artifact_record(&edited);
    assert_eq!(after["schema_version"], 2);
    assert_eq!(after["encoding"], "source-beatmap-pwa-v2");
    assert_eq!(after["record"]["raw"], before["record"]["raw"]);
    assert_eq!(
        after["record"]["accepted"]["parent_revision"],
        before["record"]["accepted"]["map_revision"]
    );
    assert_ne!(
        after["record"]["accepted"]["map_revision"],
        before["record"]["accepted"]["map_revision"]
    );
    assert_ne!(
        after["record"]["raw"]["raw_revision"],
        after["record"]["raw"]["capture"]["raw_revision"]
    );
    assert_ne!(
        after["record"]["accepted"]["map_revision"],
        after["record"]["raw"]["raw_revision"]
    );
    edited
}

fn review_public_gates(artifact: &SourceBeatmapArtifact) -> Value {
    let record = review_artifact_record(artifact);
    let anchors = record["record"]["accepted"]["anchors"].as_array().unwrap();
    let seconds = |index: usize| {
        f64::from_bits(
            u64::from_str_radix(anchors[index]["source_seconds_bits"].as_str().unwrap(), 16)
                .unwrap(),
        )
    };
    let first = seconds(0);
    let middle = first + (seconds(anchors.len() - 1) - first) / 2.0;
    Python::attach(|py| {
        let object = Py::new(py, artifact.map()).unwrap();
        let result = PyDict::new(py);
        result
            .set_item(
                "point",
                object
                    .bind(py)
                    .call_method1("evaluate_source", (middle,))
                    .unwrap(),
            )
            .unwrap();
        result
            .set_item(
                "four",
                object
                    .bind(py)
                    .call_method1("source_after_beats", (first, 4.0, true))
                    .unwrap(),
            )
            .unwrap();
        result
            .set_item(
                "bar",
                object
                    .bind(py)
                    .call_method1("nearest_grid_source", (middle, 4.0))
                    .unwrap(),
            )
            .unwrap();
        result
            .set_item(
                "span",
                object
                    .bind(py)
                    .call_method1("editor_span", (0, anchors.len() - 1))
                    .unwrap(),
            )
            .unwrap();
        let encoded: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (result,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    })
}

fn reopened_review_fixture(samples: &Path, original: &Path, frames: usize) -> ColdCustody {
    Python::initialize();
    let mut engine = AudioEngine::new().unwrap();
    assert!(engine.stream_handle.is_none());
    let (producer, consumer) = RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    let (_, feedback) = RingBuffer::<AudioMessage>::new(8);
    engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(feedback))));
    let mut mixer = Box::new(RtMixer::new(2, 48_000.0));
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    mixer.set_prepared_source_epochs(engine.prepared_source_epochs.clone());
    let mut fixture = ColdCustody {
        engine,
        // This directory owns no shared source/accepted data. The caller keeps
        // its original fixture directory alive across the engine boundary.
        directory: tempfile::tempdir().unwrap(),
        producer,
        consumer,
        mixer,
    };
    for id in [0, 1] {
        let request = crate::audio_engine::cold_load::admit_for_format_selected(
            &fixture.engine,
            id,
            original.to_string_lossy().into_owned(),
            (
                false,
                false,
                true,
                Some(ResidentLoadHint {
                    start_s: 0.25,
                    end_s: 0.5,
                    key_lock: false,
                }),
            ),
            fixture.producer.clone(),
            (2, 48_000, samples.to_owned()),
        )
        .unwrap();
        wait_until(|| {
            fixture
                .engine
                .input_runtime_ownership
                .cold_status(id, request)
                == Some(0)
                && fixture.consumer.slots() == 1
        });
        fixture.dispatch_one();
        assert_eq!(
            fixture
                .engine
                .input_runtime_ownership
                .cold_status(id, request),
            Some(2)
        );
        wait_until(|| fixture.engine.cold_loading[id].load(Ordering::Acquire) == 0);
        let sample = fixture.engine.sample_cache.lock().unwrap()[id]
            .clone()
            .unwrap();
        assert_eq!(sample.frame_count(), frames);
        assert_eq!(
            (sample.resident_start(), sample.resident_end()),
            (12_000, 24_000)
        );
        assert!(
            fixture
                .engine
                .input_runtime_ownership
                .source_current(id, &sample, 48_000)
        );
    }
    assert!(maps(&fixture).is_empty());
    fixture
}

#[test]
fn review_admission_refuses_foreign_engine_equal_source_peer_generation_and_revision() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fixture = review_fixture();
    let foreign = review_fixture();
    let raw = ready_review(&fixture);
    let fences = source_fences(&fixture);
    let foreign_fences = source_fences(&foreign);
    assert_eq!(
        manifest(&fixture, 0)["decoder_identity"],
        manifest(&foreign, 0)["decoder_identity"]
    );
    let mut wrong_revision: Value = serde_json::from_str(&raw.intent).unwrap();
    wrong_revision["raw_revision"] = json!("0".repeat(64));
    for (engine, id, expected_generation, intent) in [
        (&fixture.engine, 1, generation(&fixture), raw.intent.clone()),
        (&foreign.engine, 0, generation(&foreign), raw.intent.clone()),
        (
            &fixture.engine,
            0,
            generation(&fixture) + 1,
            raw.intent.clone(),
        ),
        (
            &fixture.engine,
            0,
            generation(&fixture),
            wrong_revision.to_string(),
        ),
    ] {
        assert!(
            engine
                .begin_source_beatmap_review_async(id, &raw.candidate, intent, expected_generation)
                .is_err()
        );
        assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (0, 0, 0));
        assert_eq!(foreign.engine.cold_jobs.counts_for_test(), (0, 0, 0));
        assert_offline_available(&fixture);
        assert_offline_available(&foreign);
    }
    assert_eq!(query(&raw.candidate, None)["status"], "ready");
    assert_eq!(source_fences(&fixture), fences);
    assert_eq!(source_fences(&foreign), foreign_fences);
    assert!(maps(&fixture).is_empty());
    assert!(maps(&foreign).is_empty());
    let raw_job = raw.discard();
    wait_review_retired(&fixture, &raw_job);
}

#[test]
fn review_admission_refuses_raw_cancel_before_transition_without_an_accepted_reservation() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fixture = review_fixture();
    let raw = ready_review(&fixture);
    let fences = source_fences(&fixture);
    raw.job.cancel();
    assert!(
        fixture
            .engine
            .begin_source_beatmap_review_async(
                0,
                &raw.candidate,
                raw.intent.clone(),
                generation(&fixture)
            )
            .is_err()
    );
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (0, 0, 0));
    assert_eq!(async_status(&raw.job)["raw_review_retired"], false);
    assert_offline_available(&fixture);
    assert!(maps(&fixture).is_empty());
    assert_eq!(source_fences(&fixture), fences);
    let raw_job = raw.discard();
    wait_review_retired(&fixture, &raw_job);
}

#[test]
fn review_admission_waits_for_real_pair_mirror_then_follows_the_original_content() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut fixture = review_fixture();
    let raw = ready_review(&fixture);
    let participants = [0, 1].map(|id| {
        fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(id)
            .unwrap()
    });
    let ticket = own_pair(&mut fixture, 1);
    let reservation = fixture.engine.input_runtime_ownership.pair_reservation(0);
    assert_ne!(reservation, 0);
    assert_eq!(
        fixture.engine.input_runtime_ownership.pair_reservation(1),
        reservation
    );
    assert_eq!(query(&raw.candidate, None)["status"], "placement_pending");
    for id in [0, 1] {
        assert!(
            fixture
                .engine
                .begin_source_beatmap_review_async(
                    id,
                    &raw.candidate,
                    raw.intent.clone(),
                    generation(&fixture)
                )
                .is_err()
        );
    }
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (0, 0, 0));
    assert!(pair::reconcile(&fixture.engine, &ticket).unwrap());
    assert_eq!(query(&raw.candidate, None)["status"], "ready");
    // The real control mirror deliberately retains the physical reservation.
    // Even a ready candidate must not bypass that independent admission fence.
    let moved_generation = fixture.engine.loaded_source_generations.lock().unwrap()[1].0;
    let reserved = fixture
        .engine
        .begin_source_beatmap_review_async(1, &raw.candidate, raw.intent.clone(), moved_generation)
        .err()
        .unwrap();
    assert!(reserved.to_string().contains("pad content pair is pending"));
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (0, 0, 0));
    // Match the native_move_source_capture private test seam: release only the
    // exact original participants' reservation, granting no public/durable Pair
    // or outer Config settlement. Its ticket remains durably unfinished.
    fixture
        .engine
        .input_runtime_ownership
        .release_pair(&participants, reservation)
        .unwrap();
    let pair_state: Value = serde_json::from_str(&ticket.snapshot_envelope_json()).unwrap();
    assert_eq!(pair_state["durable_finished"], false);
    for id in [0, 1] {
        assert_eq!(
            fixture.engine.input_runtime_ownership.pair_reservation(id),
            0
        );
    }
    assert!(
        fixture
            .engine
            .begin_source_beatmap_review_async(
                0,
                &raw.candidate,
                raw.intent.clone(),
                generation(&fixture)
            )
            .is_err()
    );
    let accepted = fixture
        .engine
        .begin_source_beatmap_review_async(1, &raw.candidate, raw.intent.clone(), moved_generation)
        .unwrap();
    let capture = raw.capture.clone();
    let raw_job = raw.discard();
    assert_eq!(wait_terminal(&accepted)["stage"], "ready");
    wait_review_retired(&fixture, &raw_job);
    let events = maps(&fixture);
    assert_eq!(events.len(), 1);
    let owner = delivery(&events[0]);
    Python::attach(|py| {
        assert_eq!(
            fixture
                .engine
                ._resolve_loader_event(&owner.borrow(py))
                .unwrap(),
            Some((1, false))
        );
    });
    assert_eq!(
        resolved_record(&accepted, &owner).unwrap()["record"]["raw"]["capture"],
        capture
    );
}

#[test]
fn queued_review_keeps_actual_raw_reader_after_discard_and_delivers_an_independent_map() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fixture = review_fixture();
    let raw = ready_review(&fixture);
    let fences = source_fences(&fixture);
    let capture = raw.capture.clone();
    let raw_path = raw.path.clone();
    let held = occupy_workers(&fixture);
    let accepted = begin_review(&fixture, &raw);
    assert_eq!(async_status(&accepted)["stage"], "queued");
    let raw_job = raw.discard();
    fixture.engine.project_assets.collect_for_test();
    assert_eq!(async_status(&raw_job)["raw_review_retired"], false);
    assert_eq!(async_status(&accepted)["cancellation_requested"], false);
    assert!(raw_path.is_file());
    assert!(fixture.engine.project_assets.held_owner_count(&raw_path) > 0);
    assert_offline_busy(&fixture);
    held.release();
    assert_eq!(wait_terminal(&accepted)["stage"], "ready");
    wait_review_retired(&fixture, &raw_job);
    assert!(
        !raw_path.exists(),
        "accepted bytes must not depend on the transient Raw artifact"
    );
    assert_eq!(source_fences(&fixture), fences);
    assert_review_capture(&accepted, &fixture, &capture);
}

#[test]
fn running_review_ignores_old_raw_cancel_until_its_real_worker_and_reader_retire() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fixture = review_fixture();
    let raw = ready_review(&fixture);
    let capture = raw.capture.clone();
    let raw_path = raw.path.clone();
    let held = OccupiedWorkers {
        gate: Arc::new((Mutex::new(false), Condvar::new())),
    };
    let gate = held.gate.clone();
    let (entered, started) = mpsc::channel();
    let accepted = fixture
        .engine
        .begin_source_beatmap_review_async_with(
            0,
            &raw.candidate,
            raw.intent.clone(),
            generation(&fixture),
            move || {
                entered.send(()).unwrap();
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            },
        )
        .unwrap();
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    let raw_job = raw.discard();
    fixture.engine.project_assets.collect_for_test();
    assert_eq!(async_status(&accepted)["stage"], "running");
    assert_eq!(async_status(&accepted)["terminal"], false);
    assert_eq!(async_status(&accepted)["cancellation_requested"], false);
    assert_eq!(async_status(&raw_job)["raw_review_retired"], false);
    assert!(raw_path.is_file());
    assert_offline_busy(&fixture);
    held.release();
    assert_eq!(wait_terminal(&accepted)["stage"], "ready");
    wait_review_retired(&fixture, &raw_job);
    assert!(!raw_path.exists());
    assert_review_capture(&accepted, &fixture, &capture);
}

#[test]
fn accepted_review_cancel_handle_drop_and_queue_discard_retire_the_original_input_once() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for action in ["cancel", "drop", "discard"] {
        let fixture = review_fixture();
        let raw = ready_review(&fixture);
        let fences = source_fences(&fixture);
        let raw_path = raw.path.clone();
        let held = occupy_workers(&fixture);
        let mut accepted = Some(begin_review(&fixture, &raw));
        let raw_job = raw.discard();
        match action {
            "cancel" => accepted.as_ref().unwrap().cancel(),
            "drop" => drop(accepted.take()),
            "discard" => fixture.engine.cold_jobs.close_admission(),
            _ => unreachable!(),
        }
        if action != "discard" {
            fixture.engine.project_assets.collect_for_test();
            assert_eq!(async_status(&raw_job)["raw_review_retired"], false);
            assert!(raw_path.is_file());
            assert_offline_busy(&fixture);
            held.release();
        }
        wait_review_retired(&fixture, &raw_job);
        if let Some(job) = &accepted {
            assert!(matches!(
                wait_terminal(job)["stage"].as_str(),
                Some("cancelled" | "failed")
            ));
        }
        assert!(!raw_path.exists());
        assert!(maps(&fixture).is_empty());
        assert_eq!(source_fences(&fixture), fences);
        assert_offline_available(&fixture);
        held.release();
        wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    }
}

#[test]
fn full_or_stopped_review_lane_refuses_before_retaining_a_raw_or_source_admission() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for stopped in [false, true] {
        let fixture = review_fixture();
        let raw = ready_review(&fixture);
        let fences = source_fences(&fixture);
        let raw_path = raw.path.clone();
        let held = occupy_workers(&fixture);
        if stopped {
            fixture.engine.cold_jobs.close_admission();
        } else {
            for _ in 0..32 {
                fixture
                    .engine
                    .cold_jobs
                    .submit(fixture.engine.cold_jobs.reserve().unwrap(), || {})
                    .unwrap();
            }
            assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (2, 32, 34));
        }
        let refused = fixture.engine.begin_source_beatmap_review_async(
            0,
            &raw.candidate,
            raw.intent.clone(),
            generation(&fixture),
        );
        assert!(refused.is_err());
        let detail = refused.err().unwrap().to_string();
        assert!(
            detail.contains(if stopped { "stopped" } else { "queue full" }),
            "{detail}"
        );
        assert_eq!(query(&raw.candidate, None)["status"], "ready");
        assert_eq!(source_fences(&fixture), fences);
        assert_offline_available(&fixture);
        assert!(maps(&fixture).is_empty());
        let raw_job = raw.discard();
        wait_review_retired(&fixture, &raw_job);
        assert!(
            !raw_path.exists(),
            "a refused admission must not leak its temporary Raw clone"
        );
        held.release();
        wait_until(|| fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    }
}

#[test]
fn review_arm_and_submit_failure_release_the_new_admission_without_revoking_raw() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for after_arm in [false, true] {
        let fixture = review_fixture();
        let raw = ready_review(&fixture);
        let fences = source_fences(&fixture);
        let raw_path = raw.path.clone();
        let refused = fixture
            .engine
            .begin_source_beatmap_review_admission_fault_for_test(
                0,
                &raw.candidate,
                raw.intent.clone(),
                generation(&fixture),
                after_arm,
            );
        assert!(refused.is_err());
        wait_until(|| {
            !fixture.engine.offline_jobs.has_pad(0)
                && fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0)
        });
        assert_eq!(query(&raw.candidate, None)["status"], "ready");
        assert_eq!(async_status(&raw.job)["raw_review_retired"], false);
        assert!(raw_path.is_file());
        assert_eq!(source_fences(&fixture), fences);
        assert!(maps(&fixture).is_empty());
        assert_offline_available(&fixture);
        let raw_job = raw.discard();
        wait_review_retired(&fixture, &raw_job);
        assert!(
            !raw_path.exists(),
            "failed accepted admission retained an original Raw reader"
        );
    }
}

#[test]
fn review_refuses_actual_equal_byte_source_aba_before_admission() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut fixture = review_fixture();
    let raw = ready_review(&fixture);
    let identity = manifest(&fixture, 0)["decoder_identity"].clone();
    crate::audio_engine::cold_load::unload_for_producer(&fixture.engine, 0, &fixture.producer)
        .unwrap();
    fixture.dispatch_one();
    fixture.load_and_ack_selected_with_frames(0, None, signal().len());
    assert_eq!(manifest(&fixture, 0)["decoder_identity"], identity);
    assert_eq!(query(&raw.candidate, None)["status"], "stale");
    assert!(
        fixture
            .engine
            .begin_source_beatmap_review_async(
                0,
                &raw.candidate,
                raw.intent.clone(),
                generation(&fixture)
            )
            .is_err()
    );
    assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (0, 0, 0));
    assert_offline_available(&fixture);
    assert!(maps(&fixture).is_empty());
    let raw_job = raw.discard();
    wait_review_retired(&fixture, &raw_job);
}

#[test]
fn new_review_source_owner_still_refuses_supersession_after_raw_cancel() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fixture = review_fixture();
    let raw = ready_review(&fixture);
    let raw_path = raw.path.clone();
    let held = occupy_workers(&fixture);
    let accepted = begin_review(&fixture, &raw);
    let raw_job = raw.discard();
    fixture.engine.input_runtime_ownership.revoke_source(0);
    assert_eq!(async_status(&accepted)["terminal"], false);
    assert_offline_busy(&fixture);
    held.release();
    assert!(matches!(
        wait_terminal(&accepted)["stage"].as_str(),
        Some("failed" | "cancelled")
    ));
    wait_review_retired(&fixture, &raw_job);
    assert!(!raw_path.exists());
    assert!(maps(&fixture).is_empty());
    assert_offline_available(&fixture);
}

#[test]
fn review_store_failure_preserves_the_raw_review_and_previous_accepted_delivery() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fixture = review_fixture();
    let previous = record(&fixture, 0);
    let predecessor = fixture
        .engine
        .begin_source_beatmap_store_async(0, previous.to_string())
        .unwrap();
    assert_eq!(wait_terminal(&predecessor)["stage"], "ready");
    let predecessor_event = maps(&fixture).pop().unwrap();
    let predecessor_owner = delivery(&predecessor_event);
    let raw = ready_review(&fixture);
    let fences = source_fences(&fixture);
    let observed = Arc::new(Mutex::new(None::<PathBuf>));
    let captured = observed.clone();
    let accepted = fixture
        .engine
        .begin_source_beatmap_review_async_with_hooks(
            0,
            &raw.candidate,
            raw.intent.clone(),
            generation(&fixture),
            || {},
            move |point, directory| {
                if point == StorePoint::BeforeRename {
                    *captured.lock().unwrap() = Some(directory.to_owned());
                    return Err("injected accepted review storage refusal".into());
                }
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(wait_terminal(&accepted)["stage"], "failed");
    let failed_birth = observed
        .lock()
        .unwrap()
        .clone()
        .expect("actual accepted storage boundary ran");
    wait_until(|| !failed_birth.exists());
    assert_eq!(query(&raw.candidate, None)["status"], "ready");
    assert!(raw.path.is_file());
    assert_eq!(async_status(&raw.job)["raw_review_retired"], false);
    assert_eq!(
        resolved_record(&predecessor, &predecessor_owner),
        Some(previous)
    );
    assert!(maps(&fixture).is_empty());
    assert_eq!(source_fences(&fixture), fences);
    assert_offline_available(&fixture);
    let raw_job = raw.discard();
    wait_review_retired(&fixture, &raw_job);
}

#[test]
fn review_v2_edits_undoes_and_restores_complete_capture_in_a_fresh_verified_engine() {
    let _serial = RAW_TEST_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut fixture = review_fixture();
    let raw = ready_review(&fixture);
    let capture = raw.capture.clone();
    let raw_path = raw.path.clone();
    let frames = capture["data"]["source_frame_count"].as_u64().unwrap() as usize;
    let fences = source_fences(&fixture);
    let original = fixture.engine.cold_leases.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .original_path
        .clone();
    let samples = original
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "samples"))
        .unwrap()
        .to_owned();
    // Keep the genuine imported Original durable while its first engine exits.
    let original_assignment = fixture
        .engine
        .project_assets
        .acquire(&samples, &original)
        .unwrap();
    original_assignment
        .acknowledge(original.to_string_lossy().into_owned())
        .unwrap();
    let accepted = begin_review(&fixture, &raw);
    let base = review_artifact(&accepted, &fixture);
    let base_record = review_artifact_record(&base);
    assert_eq!(base_record["schema_version"], 2);
    assert_eq!(base_record["record"]["raw"]["capture"], capture);
    let raw_job = raw.discard();
    wait_review_retired(&fixture, &raw_job);
    assert!(!raw_path.exists());

    let initial = review_public_gates(&base);
    assert_eq!(initial["point"]["coordinate_status"], "uncertain");
    assert_eq!(initial["span"]["coverage"], "uncertain");
    assert_eq!(initial["span"]["meter_status"], "unknown");
    assert_eq!(initial["span"]["downbeat_status"], "unknown");
    assert!(initial["span"]["downbeat_label"].is_null());
    let counted = review_edit_artifact(
        &fixture,
        &base,
        json!({
            "schema_version":1,"operation":"set_count","first_anchor_index":0,
            "last_anchor_index":1,"quarter_notes":8.0,
        }),
    );
    let count_probe = review_public_gates(&counted);
    assert_eq!(count_probe["span"]["quarter_notes"].as_f64(), Some(8.0));
    assert_eq!(count_probe["span"]["coverage"], "uncertain");
    assert_eq!(count_probe["span"]["meter_status"], "unknown");
    assert!(count_probe["span"]["downbeat_label"].is_null());
    assert_eq!(count_probe["point"]["coordinate_status"], "uncertain");
    let covered = review_edit_artifact(
        &fixture,
        &counted,
        json!({
            "schema_version":1,"operation":"set_coverage","first_anchor_index":0,
            "last_anchor_index":1,"coverage":"trusted","reviewed_quarter_notes":8.0,
        }),
    );
    let coverage_probe = review_public_gates(&covered);
    assert_eq!(coverage_probe["point"]["coordinate_status"], "supported");
    assert_eq!(coverage_probe["span"]["meter_status"], "unknown");
    assert_eq!(
        coverage_probe["four"]["coordinate_status"],
        "unsupported_meter"
    );
    assert_eq!(
        coverage_probe["bar"]["coordinate_status"],
        "unsupported_downbeat"
    );
    let refused_phase = fixture
        .engine
        .begin_source_beatmap_edit_async(
            0,
            covered.receipt_json().to_owned(),
            json!({
                "schema_version":1,"operation":"set_downbeat","first_anchor_index":0,
                "last_anchor_index":1,"downbeat_label":0,
            })
            .to_string(),
        )
        .unwrap();
    assert_eq!(wait_terminal(&refused_phase)["stage"], "failed");
    assert!(maps(&fixture).is_empty());
    assert_eq!(review_public_gates(&covered), coverage_probe);
    let metered = review_edit_artifact(
        &fixture,
        &covered,
        json!({
            "schema_version":1,"operation":"set_meter","first_anchor_index":0,
            "last_anchor_index":1,"meter_status":"quarter_note_4_4_verified",
        }),
    );
    let meter_probe = review_public_gates(&metered);
    assert_eq!(
        meter_probe["span"]["meter_status"],
        "quarter_note_4_4_verified"
    );
    assert_eq!(meter_probe["span"]["downbeat_status"], "unknown");
    assert!(meter_probe["span"]["downbeat_label"].is_null());
    assert_eq!(meter_probe["four"]["coordinate_status"], "supported");
    assert_eq!(meter_probe["four"]["interval_status"], "trusted");
    assert_eq!(
        meter_probe["bar"]["coordinate_status"],
        "unsupported_downbeat"
    );
    let phased = review_edit_artifact(
        &fixture,
        &metered,
        json!({
            "schema_version":1,"operation":"set_downbeat","first_anchor_index":0,
            "last_anchor_index":1,"downbeat_label":0,
        }),
    );
    let phase_probe = review_public_gates(&phased);
    assert_eq!(phase_probe["span"]["downbeat_status"], "known");
    assert_eq!(phase_probe["span"]["downbeat_label"], 0);
    assert_eq!(phase_probe["bar"]["coordinate_status"], "supported");
    assert_eq!(phase_probe["bar"]["interval_status"], "trusted");
    let reviewed_record = review_artifact_record(&phased);
    let anchors = reviewed_record["record"]["accepted"]["anchors"]
        .as_array()
        .unwrap();
    let left_s = f64::from_bits(
        u64::from_str_radix(anchors[0]["source_seconds_bits"].as_str().unwrap(), 16).unwrap(),
    );
    let right_s = f64::from_bits(
        u64::from_str_radix(anchors[1]["source_seconds_bits"].as_str().unwrap(), 16).unwrap(),
    );
    let inserted_sample = ((left_s + (right_s - left_s) / 2.0) * 48_000.0).round() as u64 + 1;
    let edit = fixture.engine.begin_source_beatmap_edit_async(
        0,
        phased.receipt_json().to_owned(),
        json!({"schema_version":1,"operation":"move_beat","beat":1.5,"sample_index":inserted_sample,"sample_rate_hz":48000}).to_string(),
    ).unwrap();
    let edited = review_artifact(&edit, &fixture);
    let edited_record = review_artifact_record(&edited);
    assert_eq!(edited_record["schema_version"], 2);
    assert_eq!(edited_record["encoding"], "source-beatmap-pwa-v2");
    assert_eq!(edited_record["record"]["raw"], base_record["record"]["raw"]);
    assert_eq!(
        edited_record["record"]["accepted"]["anchors"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let inserted = &edited_record["record"]["accepted"]["anchors"][1];
    assert_eq!(inserted["beat_bits"], bits(1.5));
    assert_eq!(
        inserted["source_seconds_bits"],
        bits(inserted_sample as f64 / 48_000.0)
    );
    assert!(inserted["raw_beat_index"].is_null());
    assert_eq!(inserted["review"], "reviewed");
    assert_eq!(inserted["sample_edit"]["sample_index"], inserted_sample);
    assert_ne!(
        edited_record["record"]["accepted"]["anchors"],
        reviewed_record["record"]["accepted"]["anchors"]
    );
    assert_eq!(
        edited_record["record"]["accepted"]["parent_revision"],
        reviewed_record["record"]["accepted"]["map_revision"]
    );
    assert_ne!(
        edited_record["record"]["accepted"]["map_revision"],
        reviewed_record["record"]["accepted"]["map_revision"]
    );
    let undo = fixture
        .engine
        .begin_source_beatmap_edit_async(
            0,
            edited.receipt_json().to_owned(),
            json!({"schema_version":1,"operation":"undo"}).to_string(),
        )
        .unwrap();
    let undone = review_artifact(&undo, &fixture);
    let expected = review_artifact_record(&undone);
    assert_eq!(expected["schema_version"], 2);
    assert_eq!(expected["encoding"], "source-beatmap-pwa-v2");
    assert_eq!(expected["record"]["raw"], base_record["record"]["raw"]);
    assert_eq!(
        expected["record"]["accepted"]["anchors"],
        reviewed_record["record"]["accepted"]["anchors"]
    );
    assert_eq!(
        expected["record"]["accepted"]["intervals"],
        reviewed_record["record"]["accepted"]["intervals"]
    );
    assert_eq!(
        expected["record"]["accepted"]["parent_revision"],
        edited_record["record"]["accepted"]["map_revision"]
    );
    assert_ne!(
        expected["record"]["accepted"]["map_revision"],
        edited_record["record"]["accepted"]["map_revision"]
    );
    let undone_probe = review_public_gates(&undone);
    let mut expected_span = phase_probe["span"].clone();
    expected_span["map_revision"] = expected["record"]["accepted"]["map_revision"].clone();
    assert_eq!(undone_probe["span"], expected_span);
    assert_eq!(review_artifact_record(&base), base_record);
    assert_eq!(source_fences(&fixture), fences);
    let receipt = undone.receipt_json().to_owned();
    let receipt_value: Value = serde_json::from_str(&receipt).unwrap();
    let accepted_path = crate::audio_engine::material_paths::resolve(
        &samples,
        Path::new(receipt_value["reference"].as_str().unwrap()),
    )
    .unwrap()
    .path;
    drop((
        base,
        counted,
        covered,
        metered,
        phased,
        edited,
        undone,
        accepted,
        edit,
        undo,
        raw_job,
        refused_phase,
    ));
    Python::attach(|_| {});
    wait_until(|| {
        fixture.engine.project_assets.collect_for_test();
        fixture
            .engine
            .project_assets
            .held_owner_count(&accepted_path)
            == 0
            && !fixture
                .engine
                .project_assets
                .map_retirement_pending_for_test(&accepted_path)
    });
    assert!(accepted_path.is_file());
    fixture.engine.shut_down().unwrap();
    let ColdCustody {
        engine,
        directory,
        producer,
        consumer,
        mixer,
    } = fixture;
    drop((engine, producer, consumer, mixer, original_assignment));

    // A genuinely new engine captures and verifies the same retained Original.
    // This is same-process fresh-engine proof, not a separate-process or outer
    // Python settings/CopyBank acceptance claim. No Raw artifact remains.
    let mut reopened = reopened_review_fixture(&samples, &original, frames);
    let restored_fences = source_fences(&reopened);
    let restore = reopened
        .engine
        .begin_source_beatmap_restore_async(0, receipt.clone())
        .unwrap();
    let restored = review_artifact(&restore, &reopened);
    assert_eq!(review_artifact_record(&restored), expected);
    assert_eq!(review_public_gates(&restored), undone_probe);
    assert_eq!(restored.receipt_json(), receipt);
    assert_eq!(source_fences(&reopened), restored_fences);
    assert!(!raw_path.exists());
    drop((restored, restore));
    Python::attach(|_| {});
    wait_until(|| {
        reopened.engine.project_assets.collect_for_test();
        reopened
            .engine
            .project_assets
            .held_owner_count(&accepted_path)
            == 0
            && !reopened
                .engine
                .project_assets
                .map_retirement_pending_for_test(&accepted_path)
    });
    reopened.engine.shut_down().unwrap();
    drop(reopened);
    assert!(
        accepted_path.is_file(),
        "restored readers gain no export-birth deletion rights"
    );
    drop(directory);
}
