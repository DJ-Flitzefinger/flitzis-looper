// Actual cold admission and immutable map custody; no inferred musical acceptance.

use super::*;
use crate::audio_engine::SourceBeatmap;
use crate::audio_engine::analysis_jobs::OfflineAnalysisJob;
use crate::audio_engine::cold_store::descriptor_digest;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyDictMethods};
use serde_json::{Value, json};

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn manifest(fixture: &ColdCustody, id: usize) -> Value {
    serde_json::from_str(&fixture.engine.cold_source_manifest(id).unwrap().unwrap()).unwrap()
}

fn seal(record: &mut Value) {
    record["record"]["source"]["decoder_identity"] =
        json!(descriptor_digest(&record["record"]["source"]["decoder_descriptor"]).unwrap());
    record["record"]["raw"]
        .as_object_mut()
        .unwrap()
        .remove("raw_revision");
    record["record"]["raw"]["raw_revision"] =
        json!(descriptor_digest(&record["record"]["raw"]).unwrap());
    record["record"]["accepted"]
        .as_object_mut()
        .unwrap()
        .remove("map_revision");
    record["record"]["accepted"]["map_revision"] =
        json!(descriptor_digest(&record["record"]).unwrap());
}

fn record(fixture: &ColdCustody, id: usize) -> Value {
    let actual = manifest(fixture, id);
    let mut value = json!({
        "schema_version": 1, "encoding": "source-beatmap-pwa-v1",
        "record": {
            "source": {
                "decoder_identity": actual["decoder_identity"],
                "decoder_descriptor": actual["descriptor"]["decoder"],
            },
            "raw": {
                "raw_revision": "0".repeat(64),
                "detector": {"detector_id": "manual", "code_revision": "custody-fixture-v1",
                    "configuration": {}, "model": null, "preprocessing": {}},
                "analysis_input": null, "timestamp_resolution_seconds_bits": null,
                "beat_seconds_bits": [], "downbeat_seconds_bits": [],
                "uncertainty": "synthetic coordinates prove source custody, never musical timing",
            },
            "accepted": {
                "map_revision": "0".repeat(64), "parent_revision": null,
                "beat_unit": "quarter_note",
                "interpolation_policy": "pwa-source-seconds-quarter-v1",
                "boundary_policy": "right-interior-left-final-gap-unavailable-v1",
                "snap_policy": "nearest-quarter-subdivision-later-tie-v1",
                "anchors": [
                    {"id":"start", "source_seconds_bits":bits(0.0), "beat_bits":bits(0.0),
                        "raw_beat_index":null, "review":"reviewed", "sample_edit":null},
                    {"id":"middle", "source_seconds_bits":bits(64.0 / 48_000.0),
                        "beat_bits":bits(1.0), "raw_beat_index":null,
                        "review":"reviewed", "sample_edit":null},
                ],
                "intervals": [{"coverage":"trusted", "meter":{"status":"unknown",
                    "downbeat_label":null, "evidence":"synthetic source-coordinate fixture"}}],
                "lineage": [],
            },
        },
    });
    seal(&mut value);
    value
}

fn metadata(job: &OfflineAnalysisJob) -> Value {
    Python::attach(|py| {
        let metadata = job.metadata(py).unwrap();
        let encoded: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (metadata,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    })
}

fn maps(fixture: &ColdCustody) -> Vec<Py<PyAny>> {
    Python::attach(|py| {
        let mut maps = Vec::new();
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
            assert_ne!(
                kind, "offline_analysis_completed",
                "map work must not fabricate diagnostics"
            );
            if kind == "source_beatmap_completed" {
                maps.push(event);
            }
        }
        maps
    })
}

fn assert_event(event: &Py<PyAny>, id: usize, request: u64, expected: &Value) {
    Python::attach(|py| {
        let event = event.bind(py).cast::<PyDict>().unwrap();
        assert_eq!(
            event
                .get_item("id")
                .unwrap()
                .unwrap()
                .extract::<usize>()
                .unwrap(),
            id
        );
        assert_eq!(
            event
                .get_item("request_id")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            request
        );
        assert!(!event.contains("record_json").unwrap());
        assert!(!event.contains("result_json").unwrap());
        let map = event.get_item("map").unwrap().unwrap();
        assert!(map.extract::<PyRef<'_, SourceBeatmap>>().is_ok());
        let actual: String = map.call_method0("record_json").unwrap().extract().unwrap();
        assert_eq!(serde_json::from_str::<Value>(&actual).unwrap(), *expected);
        assert_eq!(
            map.call_method0("revision")
                .unwrap()
                .extract::<String>()
                .unwrap(),
            expected["record"]["accepted"]["map_revision"]
                .as_str()
                .unwrap()
        );
        assert!(
            map.setattr("revision", "forged").is_err(),
            "completion map must remain frozen"
        );
        let evaluation = map
            .call_method1("evaluate_source", (32.0 / 48_000.0,))
            .unwrap();
        let beat: f64 = evaluation
            .cast::<PyDict>()
            .unwrap()
            .get_item("beat")
            .unwrap()
            .unwrap()
            .extract()
            .unwrap();
        assert_eq!(beat, 0.5);
    });
}

fn prepare(job: &OfflineAnalysisJob, record: &Value) {
    Python::attach(|py| job.prepare_source_beatmap(py, record.to_string()).unwrap());
}

fn finish(job: &OfflineAnalysisJob) -> bool {
    Python::attach(|py| job.finish_source_beatmap(py).unwrap())
}

fn own_pair(fixture: &mut ColdCustody, target: usize) -> PadPairTicket {
    let captured = pair::capture(&fixture.engine, 0, target).unwrap();
    let ticket = pair::prepare(&fixture.engine, &captured, 48_000, 2).unwrap();
    assert!(!pair::reconcile(&fixture.engine, &ticket).unwrap());
    pair::publish(&fixture.engine, &ticket).unwrap();
    fixture.dispatch_one();
    assert!(ticket.native_acknowledgement().is_some());
    ticket
}

#[test]
fn actual_complete_and_finite_sources_accept_maps_without_export_or_scalar_changes() {
    for finite in [false, true] {
        let mut fixture = if finite {
            ColdCustody::new()
        } else {
            ColdCustody::new_selected(None)
        };
        assert!(maps(&fixture).is_empty());
        let sample = fixture.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap();
        let peer = fixture.engine.sample_cache.lock().unwrap()[1]
            .clone()
            .unwrap();
        let generation = fixture
            .engine
            .loaded_source_generations
            .lock()
            .unwrap()
            .clone();
        let digests = fixture.engine.loaded_source_digests.lock().unwrap().clone();
        let timing = fixture.mixer.loop_region_frames(0);
        let pitch = fixture.mixer.pad_pitch_intent(0);
        let lock = fixture.mixer.key_lock_for_measurement(0);
        let expected = record(&fixture, 0);
        let job = fixture.engine.begin_source_beatmap(0).unwrap();
        let identity = metadata(&job);
        assert_eq!(identity["frame_count"], 128);
        assert_eq!(identity["channels"], 2);
        let unused_export = fixture.directory.path().join("must-not-export.f32");
        Python::attach(|py| {
            assert!(
                job.prepare_export(py, unused_export.to_string_lossy().into())
                    .is_err()
            );
            assert!(job.staging_stats(py).is_err());
            assert!(job.analyze_key(py).is_err());
            assert!(job.analyze_corrected_legacy(py, None).is_err());
            assert!(job.retire_pcm(py).is_err());
            assert!(job.finish(py, "{}".into()).is_err());
        });
        assert!(!unused_export.exists());
        prepare(&job, &expected);
        assert!(finish(&job));
        // Ordinary finished-handle retirement must not revoke its queued DTO.
        drop(job);
        let completed = maps(&fixture);
        assert_eq!(completed.len(), 1);
        assert_event(
            &completed[0],
            0,
            identity["request_id"].as_u64().unwrap(),
            &expected,
        );
        assert!(maps(&fixture).is_empty());
        assert!(!fixture.engine.offline_jobs.has_pad(0));
        let cache = fixture.engine.sample_cache.lock().unwrap();
        assert!(cache[0].as_ref().unwrap().same_window(&sample));
        assert!(Arc::ptr_eq(
            &cache[0].as_ref().unwrap().samples,
            &sample.samples
        ));
        assert!(Arc::ptr_eq(
            &cache[1].as_ref().unwrap().samples,
            &peer.samples
        ));
        drop(cache);
        assert_eq!(
            *fixture.engine.loaded_source_generations.lock().unwrap(),
            generation
        );
        assert_eq!(
            *fixture.engine.loaded_source_digests.lock().unwrap(),
            digests
        );
        assert_eq!(fixture.mixer.loop_region_frames(0), timing);
        assert_eq!(fixture.mixer.pad_pitch_intent(0), pitch);
        assert_eq!(fixture.mixer.key_lock_for_measurement(0), lock);
        if !finite {
            assert!(fixture.mixer.play_sample(0, 1.0));
            let mut output = vec![0.0; 64];
            fixture.mixer.render(
                &mut output,
                &mut [0.0; crate::audio_engine::constants::NUM_SAMPLES],
            );
            assert!(output.iter().all(|sample| sample.is_finite()));
            assert!(output.iter().any(|sample| *sample != 0.0));
        }
    }
}

#[test]
fn self_consistent_historical_map_cannot_name_a_foreign_actual_decoder() {
    let fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let mut foreign = record(&fixture, 0);
    foreign["record"]["source"]["decoder_descriptor"]["original"]["sha256"] = json!("0".repeat(64));
    seal(&mut foreign);
    Python::attach(|py| {
        assert!(
            py.get_type::<SourceBeatmap>()
                .call_method1("from_json", (foreign.to_string(),))
                .is_ok()
        );
    });
    let job = fixture.engine.begin_source_beatmap(0).unwrap();
    Python::attach(|py| assert!(job.prepare_source_beatmap(py, foreign.to_string()).is_err()));
    job.cancel();
    Python::attach(|py| assert!(job.finish_source_beatmap(py).is_err()));
    assert!(maps(&fixture).is_empty());
}

#[test]
fn prepared_map_rejects_cancel_request_supersession_and_equal_byte_source_aba() {
    for mutation in ["cancel", "request", "source_aba"] {
        let mut fixture = ColdCustody::new();
        assert!(maps(&fixture).is_empty());
        let expected = record(&fixture, 0);
        let old_assignment = fixture.engine.cold_leases.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .assignment_id();
        let job = fixture.engine.begin_source_beatmap(0).unwrap();
        prepare(&job, &expected);
        match mutation {
            "cancel" => job.cancel(),
            "request" => {
                crate::audio_engine::next_pad_request_id(
                    &fixture.engine.pad_request_ids,
                    0,
                    fixture.engine.prepared_source_epoch(0).unwrap(),
                )
                .unwrap();
            }
            "source_aba" => {
                crate::audio_engine::cold_load::unload_for_producer(
                    &fixture.engine,
                    0,
                    &fixture.producer,
                )
                .unwrap();
                fixture.dispatch_one();
                fixture.load_and_ack_selected(
                    0,
                    Some(ResidentLoadHint {
                        start_s: 32.0 / 48_000.0,
                        end_s: 64.0 / 48_000.0,
                        key_lock: false,
                    }),
                );
                assert_eq!(
                    manifest(&fixture, 0)["decoder_identity"],
                    expected["record"]["source"]["decoder_identity"]
                );
                assert_ne!(
                    fixture.engine.cold_leases.lock().unwrap()[0]
                        .as_ref()
                        .unwrap()
                        .assignment_id(),
                    old_assignment
                );
            }
            _ => unreachable!(),
        }
        assert!(!finish(&job), "stale {mutation} work must not publish");
        assert!(maps(&fixture).is_empty());
    }
}

#[test]
fn queued_map_completion_rechecks_cancel_request_and_equal_byte_source_before_delivery() {
    for mutation in ["cancel", "request", "source_aba"] {
        let mut fixture = ColdCustody::new();
        assert!(maps(&fixture).is_empty());
        let expected = record(&fixture, 0);
        let job = fixture.engine.begin_source_beatmap(0).unwrap();
        prepare(&job, &expected);
        assert!(finish(&job));
        if mutation == "source_aba" {
            crate::audio_engine::cold_load::unload_for_producer(
                &fixture.engine,
                0,
                &fixture.producer,
            )
            .unwrap();
            fixture.dispatch_one();
            fixture.load_and_ack_selected(
                0,
                Some(ResidentLoadHint {
                    start_s: 32.0 / 48_000.0,
                    end_s: 64.0 / 48_000.0,
                    key_lock: false,
                }),
            );
            assert_eq!(
                manifest(&fixture, 0)["decoder_identity"],
                expected["record"]["source"]["decoder_identity"]
            );
        } else if mutation == "request" {
            crate::audio_engine::next_pad_request_id(
                &fixture.engine.pad_request_ids,
                0,
                fixture.engine.prepared_source_epoch(0).unwrap(),
            )
            .unwrap();
        } else {
            job.cancel();
        }
        assert!(
            maps(&fixture).is_empty(),
            "queued completion must not outlive its admission"
        );
    }
}

#[test]
fn unowned_historical_map_cannot_publish_an_actual_source_completion() {
    let fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let encoded = record(&fixture, 0).to_string();
    let historical = Python::attach(|py| {
        let object = py
            .get_type::<SourceBeatmap>()
            .call_method1("from_json", (encoded,))
            .unwrap();
        let borrowed = object.extract::<PyRef<'_, SourceBeatmap>>().unwrap();
        (*borrowed).clone()
    });
    let request_id = fixture.engine.pad_request_ids.lock().unwrap()[0];
    fixture
        .engine
        .loader_tx
        .send(LoaderEvent::SourceBeatmapCompleted {
            id: 0,
            request_id,
            map: historical,
            artifact: None,
        })
        .unwrap();
    assert!(
        maps(&fixture).is_empty(),
        "valid historical data confers no NativeOwned admission"
    );
}

#[test]
fn actual_swap_and_move_map_completion_wait_for_mirror_and_follow_original_content() {
    for (target, cancel_pending) in [(1, false), (2, false), (1, true)] {
        let mut fixture = ColdCustody::new();
        assert!(maps(&fixture).is_empty());
        let expected = record(&fixture, 0);
        let placements: [SlotSnapshot; 2] = [0, 1].map(|id| {
            fixture
                .engine
                .input_runtime_ownership
                .slot_snapshot(id)
                .unwrap()
        });
        let assignments: [u64; 2] = [0, 1].map(|id| {
            fixture.engine.cold_leases.lock().unwrap()[id]
                .as_ref()
                .unwrap()
                .assignment_id()
        });
        let job = fixture.engine.begin_source_beatmap(0).unwrap();
        let identity = metadata(&job);
        prepare(&job, &expected);
        let ticket = own_pair(&mut fixture, target);
        assert!(fixture.engine.offline_jobs.has_pad(0));
        assert!(fixture.engine.offline_jobs.has_pad(target));
        assert!(finish(&job));
        assert!(
            maps(&fixture).is_empty(),
            "Native ACK alone cannot complete the control mirror"
        );
        assert!(
            fixture
                .engine
                .pending_loader_event
                .lock()
                .unwrap()
                .is_some()
        );
        if cancel_pending {
            // Explicit cancellation must revoke even an already queued map whose
            // original content is currently between native ACK and control mirror.
            job.cancel();
        }
        assert!(pair::reconcile(&fixture.engine, &ticket).unwrap());
        crate::audio_engine::next_pad_request_id(
            &fixture.engine.pad_request_ids,
            0,
            fixture.engine.prepared_source_epoch(0).unwrap(),
        )
        .unwrap();
        let completed = maps(&fixture);
        if cancel_pending {
            assert!(
                completed.is_empty(),
                "pending-placement cancellation must revoke the original queued map"
            );
        } else {
            assert_eq!(completed.len(), 1);
            assert_event(
                &completed[0],
                target,
                identity["request_id"].as_u64().unwrap(),
                &expected,
            );
        }
        assert_eq!(
            fixture
                .engine
                .input_runtime_ownership
                .slot_snapshot(target)
                .unwrap()
                .content_key(),
            placements[0].content_key()
        );
        assert_eq!(
            fixture.engine.cold_leases.lock().unwrap()[target]
                .as_ref()
                .unwrap()
                .assignment_id(),
            assignments[0]
        );
        let peer_slot = if target == 1 { 0 } else { 1 };
        assert_eq!(
            fixture
                .engine
                .input_runtime_ownership
                .slot_snapshot(peer_slot)
                .unwrap()
                .content_key(),
            placements[1].content_key()
        );
        assert_eq!(
            fixture.engine.cold_leases.lock().unwrap()[peer_slot]
                .as_ref()
                .unwrap()
                .assignment_id(),
            assignments[1]
        );
        assert!(maps(&fixture).is_empty());
    }
}

#[test]
fn blocked_verified_prepare_keeps_shared_admission_and_actual_readers_until_work_ends() {
    let mut fixture = ColdCustody::new_selected(None);
    assert!(maps(&fixture).is_empty());
    let expected = record(&fixture, 0);
    let (readers_retired, original) = {
        let leases = fixture.engine.cold_leases.lock().unwrap();
        let lease = leases[0].as_ref().unwrap();
        (
            lease.reader_lifetime_probe_for_test(),
            lease.original_path.clone(),
        )
    };
    let assets = fixture.engine.project_assets.clone();
    let job = fixture.engine.begin_source_beatmap(0).unwrap();
    let (entered, started) = mpsc::channel();
    let (release, resumed) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = job.prepare_source_beatmap_with(&expected.to_string(), || {
            entered.send(()).unwrap();
            resumed.recv_timeout(Duration::from_secs(10)).unwrap();
        });
        assert!(
            result.is_err(),
            "cancellation during actual verified work must win"
        );
        Python::attach(|py| assert!(job.finish_source_beatmap(py).is_err()));
    });
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    fixture.engine.offline_jobs.cancel(Some(0));
    let peer_request = fixture.engine.pad_request_ids.lock().unwrap()[1];
    assert!(
        fixture.engine.begin_source_beatmap(1).is_err(),
        "cancelled running map must retain the shared offline slot"
    );
    assert_eq!(
        fixture.engine.pad_request_ids.lock().unwrap()[1],
        peer_request
    );
    for id in [0, 1] {
        crate::audio_engine::cold_load::unload_for_producer(&fixture.engine, id, &fixture.producer)
            .unwrap();
        fixture.dispatch_one();
    }
    let ColdCustody {
        engine,
        directory,
        producer,
        consumer,
        mixer,
    } = fixture;
    drop(engine);
    drop(producer);
    drop(consumer);
    drop(mixer);
    assets.collect_for_test();
    assert!(
        !readers_retired(),
        "actual preparing reader must survive every other source owner"
    );
    assert!(fs::OpenOptions::new().write(true).open(&original).is_err());
    release.send(()).unwrap();
    worker.join().unwrap();
    wait_until(|| {
        assets.collect_for_test();
        readers_retired()
    });
    drop(directory);
}

#[test]
fn unstarted_map_abort_releases_admission_without_a_diagnostic_completion() {
    let fixture = ColdCustody::new();
    assert!(maps(&fixture).is_empty());
    let job = fixture.engine.begin_source_beatmap(0).unwrap();
    job.abort_unstarted().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let next = loop {
        match fixture.engine.begin_source_beatmap(1) {
            Ok(next) => break next,
            Err(_) => {
                assert!(
                    Instant::now() < deadline,
                    "unstarted map retirement kept the offline slot"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    };
    assert!(maps(&fixture).is_empty());
    next.abort_unstarted().unwrap();
}

fn source_fences(fixture: &ColdCustody) -> (Vec<u64>, [u64; 2], [u64; 2]) {
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let incarnations = [0, 1].map(|id| {
        let cell = fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(id)
            .unwrap()
            .cell();
        fixture
            .engine
            .input_runtime_ownership
            .source_request_for_cell(cell)
    });
    let epochs = [0, 1].map(|id| {
        fixture
            .engine
            .prepared_source_epoch(id)
            .unwrap()
            .load(Ordering::Acquire)
    });
    (requests, incarnations, epochs)
}

#[test]
fn map_only_work_preserves_already_pending_ordinary_and_piano_playback() {
    use crate::audio_engine::audio_stream::{drain_control_messages, render_scheduled_audio};
    use crate::audio_engine::input_mapping::{InputRuntime, PitchPadProjection};
    use crate::audio_engine::pitch_actions::PitchKeyMode;
    use crate::audio_engine::timing::{OutputClockSnapshot, SharedOutputClock};

    const RATE: u32 = 48_000;
    // Both public capture routes use the existing nearest-input grid. This
    // declared future-half position selects 24000 without a late/past target.
    const FRAME: u64 = 17_000;
    const TARGET: u64 = 24_000;
    const EMITTED_FRAMES: usize = 32;
    const SLOTS: usize = crate::audio_engine::constants::NUM_SAMPLES;

    for (map_id, outcome) in [0, 1].into_iter().flat_map(|map_id| {
        ["finish", "cancel", "drop", "abort"]
            .into_iter()
            .map(move |outcome| (map_id, outcome))
    }) {
        let mut fixture = ColdCustody::new_selected(None);
        assert!(maps(&fixture).is_empty());
        fixture.engine.test_output_format = Some((RATE, 2));
        fixture.engine.input_runtime = Some(InputRuntime::new_with_ownership(
            fixture.producer.clone(),
            fixture.engine.input_clock,
            fixture.engine.input_runtime_ownership.clone(),
        ));
        let runtime = fixture.engine.input_runtime.as_ref().unwrap();
        runtime.set_render_format(RATE, 2).unwrap();
        let clock = Arc::new(SharedOutputClock::new());
        runtime.attach_pitch_clock(clock.clone()).unwrap();
        let mut loaded = vec![false; SLOTS];
        let starts = vec![0.0; SLOTS];
        let mut ends = vec![None; SLOTS];
        let mut bindings = vec![None; SLOTS];
        let mut rows = vec![PitchPadProjection::default(); SLOTS];
        for id in [0, 1] {
            loaded[id] = true;
            ends[id] = Some(128.0 / f64::from(RATE));
            bindings[id] =
                crate::audio_engine::input_runtime_binding::capture(&fixture.engine, id).unwrap();
            rows[id] = PitchPadProjection {
                source_root: Some(0),
                source_mode: Some(PitchKeyMode::Major),
                quantize_step_64ths: Some(16),
                ..PitchPadProjection::default()
            };
            fixture.mixer.set_pad_loop_region(id, 0.0, ends[id]);
        }
        runtime
            .set_runtime_state_with_pitch(
                true,
                loaded,
                starts,
                ends,
                bindings.iter().map(Option::as_ref).collect(),
                rows,
            )
            .unwrap();
        let now = fixture.engine.input_clock.capture_ns();
        clock.publish(OutputClockSnapshot {
            valid: true,
            observed_at_ns: now,
            audible_at_ns: now,
            output_frame: FRAME,
            sample_rate_hz: RATE,
            master_period_seconds: Some(0.5),
            downbeat_frame: 0,
            master_beat: FRAME as f64 / 24_000.0,
            freshness_ns: 2_000_000_000,
        });
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(RATE);
        transport.advance_by_rendered_frames(FRAME as usize);
        let mut feedback = Vec::<AudioMessage>::new();
        let mut retirement = ImmediateAudioBufferRetirement;
        let mut quantize = TriggerQuantization::Grid { step_64ths: 16 };
        let ordinary = fixture
            .engine
            .capture_pad_start(
                0,
                0.0,
                Some(128.0 / f64::from(RATE)),
                0.4,
                false,
                None,
                None,
            )
            .unwrap();
        // The real first callback captures the ordinary target; producer
        // publication alone must not manufacture Native admission.
        drain_control_messages(
            &mut fixture.consumer,
            &mut scheduler,
            FRAME,
            &mut quantize,
            &mut transport,
            &mut fixture.mixer,
            &mut feedback,
            &mut retirement,
        );
        assert_eq!(ordinary.original_frame(), Some(TARGET));
        assert!(fixture.engine.publish_pad_start(&ordinary, None).unwrap());
        let piano = fixture
            .engine
            .capture_pad_pitch_action(1, "pitch.extra:0:set_retrigger", None)
            .unwrap();
        assert!(fixture.engine.publish_pitch_action(&piano).unwrap());
        drain_control_messages(
            &mut fixture.consumer,
            &mut scheduler,
            FRAME,
            &mut quantize,
            &mut transport,
            &mut fixture.mixer,
            &mut feedback,
            &mut retirement,
        );
        assert!(
            ordinary.accepted(),
            "ordinary admission {:?}",
            ordinary.reason()
        );
        assert!(piano.accepted(), "piano admission {:?}", piano.reason());
        assert_ne!(ordinary.state(), "applied");
        assert!(!piano.effective());
        assert_eq!(ordinary.execution_frame(), Some(TARGET));
        assert_eq!(piano.original_due_frame(), TARGET);
        assert_eq!(piano.execution_target(), TARGET);
        assert_eq!(scheduler.len(), 2);
        assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));

        let original_fences = source_fences(&fixture);
        let expected = record(&fixture, map_id);
        let job = fixture.engine.begin_source_beatmap(map_id).unwrap();
        assert_eq!(
            source_fences(&fixture),
            original_fences,
            "map admission changed source authority"
        );
        assert_eq!(
            metadata(&job)["request_id"].as_u64().unwrap(),
            original_fences.0[map_id]
        );
        if outcome != "abort" {
            prepare(&job, &expected);
            assert_eq!(
                source_fences(&fixture),
                original_fences,
                "map preparation changed source authority"
            );
        }
        match outcome {
            "finish" => {
                assert!(finish(&job));
                drop(job);
            }
            "cancel" => {
                job.cancel();
                assert_eq!(
                    source_fences(&fixture),
                    original_fences,
                    "map cancellation changed source authority"
                );
                assert!(!finish(&job));
                drop(job);
            }
            "drop" => drop(job),
            "abort" => {
                job.abort_unstarted().unwrap();
                drop(job);
            }
            _ => unreachable!(),
        }
        wait_until(|| !fixture.engine.offline_jobs.has_pad(map_id));
        assert_eq!(
            source_fences(&fixture),
            original_fences,
            "map completion/retirement changed source authority"
        );
        let completed = maps(&fixture);
        if outcome == "finish" {
            assert_eq!(completed.len(), 1);
            assert_event(&completed[0], map_id, original_fences.0[map_id], &expected);
        } else {
            assert!(completed.is_empty());
        }

        while transport.output_frame() < TARGET {
            let frame = transport.output_frame();
            let frames = (TARGET - frame).min(511) as usize;
            let mut output = vec![0.0; frames * 2];
            render_scheduled_audio(
                &mut fixture.mixer,
                &mut scheduler,
                &mut output,
                &mut [0.0; SLOTS],
                frame,
                2,
                &mut transport,
                &mut feedback,
                &mut retirement,
            );
            assert!(output.iter().all(|sample| *sample == 0.0));
            transport.advance_by_rendered_frames(frames);
        }
        let mut output = vec![0.0; EMITTED_FRAMES * 2];
        render_scheduled_audio(
            &mut fixture.mixer,
            &mut scheduler,
            &mut output,
            &mut [0.0; SLOTS],
            TARGET,
            2,
            &mut transport,
            &mut feedback,
            &mut retirement,
        );
        assert_eq!(
            ordinary.state(),
            "applied",
            "map {outcome}: {:?}",
            ordinary.reason()
        );
        assert_eq!(ordinary.original_frame(), Some(TARGET));
        assert_eq!(ordinary.effective_frame(), Some(TARGET));
        assert!(piano.effective(), "map {outcome}: {:?}", piano.reason());
        assert_eq!(piano.original_due_frame(), TARGET);
        assert!(scheduler.is_empty());
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(output.iter().any(|sample| *sample != 0.0));
        for id in [0, 1] {
            let voice = fixture
                .mixer
                .voices
                .iter()
                .find(|voice| voice.active && voice.sample_id == id)
                .unwrap();
            assert_eq!(voice.source_playback.position().frame, EMITTED_FRAMES);
            assert_eq!(voice.source_playback.tempo_ratio(), 1.0);
            let starts = feedback
                .iter()
                .filter(|message| {
                    matches!(message,
                AudioMessage::ContentFeedback { id: started, kind: 1, .. }
                | AudioMessage::SampleStarted { id: started } if *started == id)
                })
                .count();
            assert_eq!(
                starts, 1,
                "map {outcome} changed real PAD{id} start effects"
            );
        }
        assert_eq!(source_fences(&fixture), original_fences);
    }
}

mod source_beatmap_async_tests {
    include!("source_beatmap_async_tests.rs");

    mod source_beatmap_raw_tests {
        include!("source_beatmap_raw_tests.rs");
        mod source_beatmap_review_tests {
            include!("source_beatmap_review_tests.rs");
        }
    }

    mod source_beatmap_artifact_tests {
        include!("source_beatmap_artifact_tests.rs");

        mod selection_vertical {
            include!("source_beatmap_selection_vertical_tests.rs");
        }
    }
}
