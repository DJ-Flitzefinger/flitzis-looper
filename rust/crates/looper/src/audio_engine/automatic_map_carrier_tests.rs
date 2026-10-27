// Synthetic map coordinates test genuine source/artifact custody only. Real NN
// v4 production is checked separately by the opt-in complete-source integration.
use crate::audio_engine::complete_context::{CompleteSourceReader, NativeAnalysisOwner};
use crate::messages::{AutomaticSourceMap, BackgroundTaskKind, LoadedAnalysis};
use std::sync::atomic::AtomicBool;

pub(super) struct Carrier {
    _event: Py<PyAny>,
    delivery: Py<LoaderEventDelivery>,
    pub(super) original: NativeAnalysisOwner,
    claim: Arc<AtomicBool>,
    cancellation: Arc<AtomicBool>,
    pub(super) path: PathBuf,
}

pub(super) fn export_for_source(
    fixture: &ColdCustody,
    id: usize,
) -> (Carrier, SourceBeatmapArtifact, Py<LoaderEventDelivery>) {
    let carrier = make_carrier(fixture, id, false, analysis_for(fixture, id), true);
    let (slot, artifact, derived) = consume(&fixture.engine, &carrier).unwrap();
    assert_eq!(slot, id);
    (carrier, artifact, derived)
}

fn analysis_for(fixture: &ColdCustody, id: usize) -> LoadedAnalysis {
    LoadedAnalysis {
        bpm: 120.0,
        key: Some("C".into()),
        beat_grid: flitzis_looper_analysis::BeatGrid {
            beats: vec![],
            downbeats: vec![],
            bars: vec![],
        },
        bpm_method_json: None,
        beat_error: None,
        automatic_source_map: Some(AutomaticSourceMap::Pending(
            SourceBeatmap::parse(&record(fixture, id).to_string()).unwrap(),
        )),
        automatic_source_map_error: None,
    }
}

fn make_carrier(
    fixture: &ColdCustody,
    id: usize,
    cold: bool,
    mut analysis: LoadedAnalysis,
    cold_epoch: bool,
) -> Carrier {
    assert!(maps(fixture).is_empty());
    let sample = fixture.engine.sample_cache.lock().unwrap()[id]
        .clone()
        .unwrap();
    let reader = CompleteSourceReader::capture(&fixture.engine, id, sample).unwrap();
    let epoch = fixture.engine.prepared_source_epoch(id).unwrap();
    let expected = epoch.load(Ordering::Acquire);
    let original = {
        let _requests = fixture.engine.pad_request_ids.lock().unwrap();
        NativeAnalysisOwner::new(reader.guard.clone().unwrap(), epoch.clone())
    };
    let original = if cold {
        original.source_only()
    } else {
        original
    };
    let cancellation = Arc::new(AtomicBool::new(false));
    let original = original.with_cancellation(cancellation.clone());
    let cancelled = || !original.current();
    let (root, path) = reader.map_artifact_paths().unwrap();
    crate::audio_engine::app_analysis::store_automatic_map(
        &mut analysis,
        root,
        path,
        reader.held_bytes().unwrap(),
        &cancelled,
        |map| reader.verify_source_beatmap(map, &cancelled),
    );
    assert!(
        analysis.automatic_source_map_error.is_none(),
        "{:?}",
        analysis.automatic_source_map_error
    );
    let Some(AutomaticSourceMap::Stored { artifact, consumed }) = &analysis.automatic_source_map
    else {
        panic!("missing genuine stored artifact");
    };
    let claim = consumed.clone();
    let receipt: Value = serde_json::from_str(artifact.receipt_json()).unwrap();
    let path = artifact_path(fixture, &receipt);
    let event = if cold {
        LoaderEvent::Success {
            id,
            request_id: 1,
            timing_epoch: cold_epoch.then_some(expected),
            duration_s: 128.0 / 48_000.0,
            detected_loop_start_s: None,
            cached_path: "synthetic diagnostic only".into(),
            original_lease: None,
            analysis: Some(analysis),
            analysis_error: None,
            incoming_defaults: None,
            analysis_tempo_only: false,
        }
    } else {
        LoaderEvent::TaskSuccess {
            id,
            request_id: 1,
            task: BackgroundTaskKind::Analysis,
            analysis: Some(analysis),
        }
    };
    fixture
        .engine
        .loader_tx
        .send(original.event(event))
        .unwrap();
    let event = Python::attach(|py| fixture.engine.poll_loader_events(py).unwrap().unwrap());
    let owner = delivery(&event);
    Carrier {
        _event: event,
        delivery: owner,
        original,
        claim,
        cancellation,
        path,
    }
}

fn consume(
    engine: &AudioEngine,
    carrier: &Carrier,
) -> Option<(usize, SourceBeatmapArtifact, Py<LoaderEventDelivery>)> {
    Python::attach(|py| {
        engine
            .consume_automatic_source_beatmap(py, &carrier.delivery.borrow(py))
            .unwrap()
            .map(|tuple| {
                let tuple = tuple.bind(py);
                let id = tuple.get_item(0).unwrap().extract().unwrap();
                let artifact: Py<SourceBeatmapArtifact> =
                    tuple.get_item(1).unwrap().extract().unwrap();
                let derived = tuple.get_item(2).unwrap().extract().unwrap();
                let artifact = artifact.borrow(py).clone();
                (id, artifact, derived)
            })
    })
}

fn assert_original_cannot_export(fixture: &ColdCustody, carrier: &Carrier) {
    let binding = fixture
        .engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap();
    Python::attach(|py| {
        let original = carrier.delivery.borrow(py);
        assert!(!original.source_map_belongs_to(&fixture.engine.input_runtime_ownership));
        assert!(
            !original.source_map_binding_current(
                &fixture.engine.input_runtime_ownership,
                binding.binding
            )
        );
        assert!(original.qualify_source_map(&binding, None).is_err());
        assert!(
            original
                .source_beatmap_raw_for(&carrier.original, Arc::new(AtomicBool::new(false)))
                .unwrap()
                .is_none()
        );
        assert!(
            original
                .source_beatmap_for(&carrier.original)
                .unwrap()
                .is_none()
        );
        assert!(
            original
                .source_beatmap_artifact_for(&carrier.original)
                .unwrap()
                .is_none()
        );
    });
}

fn edit_bpm(fixture: &mut ColdCustody) {
    fixture.engine.test_output_format = Some((48_000, 2));
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    fixture.engine.test_parameter_producer = Some(Arc::new(Mutex::new(producer)));
    let generation = fixture.engine.loaded_source_generations.lock().unwrap()[0].0;
    fixture
        .engine
        .set_pad_bpm_with_source(0, Some(121.0), Some(generation))
        .unwrap();
    crate::audio_engine::audio_stream::drain_parameter_messages(
        &mut consumer,
        &mut fixture.mixer,
        &mut crate::audio_engine::transport::TransportTimeline::new(48_000),
    );
}

#[test]
fn automatic_carrier_consumes_once_and_preserves_original_epoch_and_retirement() {
    for cold in [false, true] {
        let mut fixture = ColdCustody::new();
        let carrier = make_carrier(&fixture, 0, cold, analysis_for(&fixture, 0), true);
        assert_original_cannot_export(&fixture, &carrier);
        Python::attach(|py| {
            assert_eq!(
                fixture
                    .engine
                    ._resolve_loader_event(&carrier.delivery.borrow(py))
                    .unwrap(),
                Some((0, false))
            );
        });
        let clone = Python::attach(|py| carrier.delivery.clone_ref(py));
        let (id, artifact, derived) = consume(&fixture.engine, &carrier).unwrap();
        assert_eq!(id, 0);
        Python::attach(|py| {
            assert_eq!(
                fixture
                    .engine
                    ._resolve_loader_event(&derived.borrow(py))
                    .unwrap(),
                Some((0, true))
            );
        });
        assert_original_cannot_export(&fixture, &carrier);
        assert!(carrier.claim.load(Ordering::Acquire));
        assert!(Python::attach(|py| fixture
            .engine
            .consume_automatic_source_beatmap(py, &clone.borrow(py))
            .unwrap()
            .is_none()));
        edit_bpm(&mut fixture);
        assert_original_cannot_export(&fixture, &carrier);
        if !cold {
            assert!(!carrier.original.current());
        }
        let binding = fixture
            .engine
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .unwrap();
        Python::attach(|py| {
            assert_eq!(
                fixture
                    .engine
                    ._resolve_loader_event(&derived.borrow(py))
                    .unwrap(),
                Some((0, true))
            );
            assert!(
                derived
                    .borrow(py)
                    .qualify_source_map(&binding, None)
                    .is_ok()
            )
        });
        artifact.retire_unselected();
        assert!(
            consume(&fixture.engine, &carrier).is_none(),
            "retirement cannot reopen an export"
        );
        let path = carrier.path.clone();
        drop(artifact);
        drop(derived);
        drop(clone);
        drop(carrier);
        flush_python_drops();
        wait_output_retired(&fixture, &path);
        assert!(!path.exists());
    }
}

#[test]
fn automatic_carrier_late_timing_and_missing_cold_epoch_do_not_spend_claim() {
    for cold in [false, true] {
        let mut fixture = ColdCustody::new();
        let carrier = make_carrier(&fixture, 0, cold, analysis_for(&fixture, 0), true);
        edit_bpm(&mut fixture);
        assert_original_cannot_export(&fixture, &carrier);
        assert!(consume(&fixture.engine, &carrier).is_none());
        assert!(!carrier.claim.load(Ordering::Acquire));
        let path = carrier.path.clone();
        drop(carrier);
        flush_python_drops();
        wait_output_retired(&fixture, &path);
        assert!(!path.exists());
    }
    let fixture = ColdCustody::new();
    let carrier = make_carrier(&fixture, 0, true, analysis_for(&fixture, 0), false);
    assert!(consume(&fixture.engine, &carrier).is_none());
    assert!(!carrier.claim.load(Ordering::Acquire));
    let path = carrier.path.clone();
    drop(carrier);
    flush_python_drops();
    wait_output_retired(&fixture, &path);
    assert!(!path.exists());
}

#[test]
fn automatic_carrier_foreign_and_pending_are_retryable_but_manual_is_ineligible() {
    let mut fixture = ColdCustody::new();
    let expected = record(&fixture, 0);
    let (job, event) = stored_event(&fixture, &expected);
    let manual = delivery(&event);
    Python::attach(|py| {
        assert!(
            fixture
                .engine
                .consume_automatic_source_beatmap(py, &manual.borrow(py))
                .unwrap()
                .is_none()
        )
    });
    drop(manual);
    drop(event);
    drop(job);
    flush_python_drops();
    let carrier = make_carrier(&fixture, 0, false, analysis_for(&fixture, 0), true);
    let foreign = AudioEngine::new().unwrap();
    assert!(consume(&foreign, &carrier).is_none());
    assert!(!carrier.claim.load(Ordering::Acquire));
    let ticket = own_pair(&mut fixture, 1);
    Python::attach(|py| {
        assert!(
            fixture
                .engine
                .consume_automatic_source_beatmap(py, &carrier.delivery.borrow(py))
                .is_err()
        )
    });
    assert!(!carrier.claim.load(Ordering::Acquire));
    assert!(pair::reconcile(&fixture.engine, &ticket).unwrap());
    let (id, artifact, derived) = consume(&fixture.engine, &carrier).unwrap();
    assert_eq!(id, 1);
    artifact.retire_unselected();
    let path = carrier.path.clone();
    drop(artifact);
    drop(derived);
    drop(carrier);
    flush_python_drops();
    wait_output_retired(&fixture, &path);
    assert!(!path.exists());
}

#[test]
fn automatic_carrier_admissions_do_not_share_claims_from_cached_pending_geometry() {
    let fixture = ColdCustody::new();
    let base = analysis_for(&fixture, 0);
    let a = make_carrier(&fixture, 0, false, base.clone(), true);
    let b = make_carrier(&fixture, 1, false, base, true);
    assert!(!Arc::ptr_eq(&a.claim, &b.claim));
    let (_, aa, ad) = consume(&fixture.engine, &a).unwrap();
    let (_, ba, bd) = consume(&fixture.engine, &b).unwrap();
    let peer = fixture
        .engine
        .current_input_runtime_pad_binding(1)
        .unwrap()
        .unwrap();
    Python::attach(|py| assert!(ad.borrow(py).qualify_source_map(&peer, None).is_err()));
    aa.retire_unselected();
    ba.retire_unselected();
    let paths = [a.path.clone(), b.path.clone()];
    drop((aa, ad, ba, bd, a, b));
    flush_python_drops();
    for path in paths {
        wait_output_retired(&fixture, &path);
        assert!(!path.exists());
    }
}

#[test]
fn automatic_carrier_derived_owner_rejects_unload_and_same_material_reload() {
    let mut fixture = ColdCustody::new();
    let carrier = make_carrier(&fixture, 0, false, analysis_for(&fixture, 0), true);
    let (_, artifact, derived) = consume(&fixture.engine, &carrier).unwrap();
    fixture.engine.unload_sample(0).unwrap();
    fixture.dispatch_one();
    fixture.load_and_ack_selected_with_frames(0, None, 128);
    let binding = fixture
        .engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap();
    Python::attach(|py| {
        assert!(
            derived
                .borrow(py)
                .qualify_source_map(&binding, None)
                .is_err()
        )
    });
    artifact.retire_unselected();
    let path = carrier.path.clone();
    drop((artifact, derived, carrier));
    flush_python_drops();
    wait_output_retired(&fixture, &path);
    assert!(!path.exists());
}

#[test]
fn automatic_carrier_optional_store_fault_preserves_scalar_key_and_actual_source() {
    let fixture = ColdCustody::new();
    let mut analysis = analysis_for(&fixture, 0);
    let before = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let original = original_path(&fixture, 0);
    let root = samples_root(&fixture);
    crate::audio_engine::app_analysis::store_automatic_map(
        &mut analysis,
        &root,
        &original,
        usize::MAX,
        &|| false,
        |_| Ok(()),
    );
    assert!(analysis.automatic_source_map.is_none());
    assert!(analysis.automatic_source_map_error.is_some());
    assert_eq!(analysis.bpm, 120.0);
    assert_eq!(analysis.key.as_deref(), Some("C"));
    assert!(analysis.beat_error.is_none());
    assert!(ready_maps(&fixture).is_empty());
    assert!(Arc::ptr_eq(
        &before.samples,
        &fixture.engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
}

#[test]
fn automatic_carrier_cancellation_fences_original_and_derived_without_reopening_export() {
    for after_export in [false, true] {
        let fixture = ColdCustody::new();
        let carrier = make_carrier(&fixture, 0, false, analysis_for(&fixture, 0), true);
        let exported = after_export.then(|| consume(&fixture.engine, &carrier).unwrap());
        carrier.cancellation.store(true, Ordering::Release);
        assert!(!carrier.original.current());
        assert!(consume(&fixture.engine, &carrier).is_none());
        if let Some((_, artifact, derived)) = &exported {
            let binding = fixture
                .engine
                .current_input_runtime_pad_binding(0)
                .unwrap()
                .unwrap();
            Python::attach(|py| {
                assert!(
                    derived
                        .borrow(py)
                        .qualify_source_map(&binding, None)
                        .is_err()
                )
            });
            artifact.retire_unselected();
        } else {
            assert!(!carrier.claim.load(Ordering::Acquire));
        }
        let path = carrier.path.clone();
        drop(exported);
        drop(carrier);
        flush_python_drops();
        wait_output_retired(&fixture, &path);
        assert!(!path.exists());
    }
}
