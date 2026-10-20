//! Genuine target Cold and selected-pair restoration for the fixed recovery images.
use super::*;

pub(super) fn restore_target(
    fixture: &mut Fixture,
    target: usize,
    image: &str,
) -> Option<crate::audio_engine::CommittedStemSetTicket> {
    let envelope: Value = serde_json::from_str(image).unwrap();
    let snapshot = &envelope["snapshot"];
    let metadata = fixture.material.metadata();
    assert!(target < NUM_SAMPLES && target != 0);
    assert!(!fixture.key_lock && fixture.timing == "tap");
    assert_eq!(envelope["schema_version"], json!(1));
    assert_eq!(envelope["resolved_key_lock"], json!(false));
    assert_eq!(snapshot["source_path"], metadata["new_reference"]);
    assert_eq!(
        snapshot["source_identity"]["material_id"],
        metadata["material_id"]
    );
    assert_eq!(
        snapshot["source_identity"]["material_creation"],
        metadata["material_creation"]
    );
    assert_eq!(snapshot["duration"], json!(FRAMES as f64 / f64::from(RATE)));
    assert!(snapshot["analysis"].is_null());
    assert!(snapshot["key_intent"].is_null());
    assert_eq!(snapshot["key_lock"], json!(false));
    assert_eq!(snapshot["manual_bpm"], json!(BPM));
    assert_eq!(snapshot["timing_intent"], json!("tap"));
    assert_eq!(snapshot["gain_db"], json!(-7.0));
    assert_eq!(snapshot["eq_low_db"], json!(3.0));
    assert_eq!(snapshot["eq_mid_db"], json!(-4.0));
    assert_eq!(snapshot["eq_high_db"], json!(1.5));
    assert_eq!(
        snapshot["loop_start_s"],
        json!(START as f64 / f64::from(RATE))
    );
    assert_eq!(snapshot["loop_end_s"], json!(END as f64 / f64::from(RATE)));
    assert_eq!(snapshot["loop_auto"], json!(false));
    assert_eq!(snapshot["loop_bars"], json!(1.5));
    assert_eq!(snapshot["grid_anchor_s"], json!(ANCHOR));
    assert_eq!(snapshot["grid_offset_samples"], json!(OFFSET));
    assert_eq!(snapshot["stem_intent"]["schema_version"], json!(1));
    assert_eq!(snapshot["stem_intent"]["last_custom_mask"], json!(5));
    assert_eq!(snapshot["stem_intent"]["display_mode"], json!("custom"));
    let all_stems = match snapshot["stem_mix_mode"].as_str().unwrap() {
        "full_mix" => {
            assert_eq!(snapshot["stem_intent"]["enabled_mask"], json!(15));
            false
        }
        "all_stems" => {
            assert_eq!(snapshot["stem_intent"]["enabled_mask"], json!(0));
            true
        }
        other => panic!("unsupported fixed recovery fixture mode: {other}"),
    };
    let selected = !snapshot["stem_cache"].is_null();
    assert!(!all_stems || selected);
    if selected {
        assert!(fixture.pair.is_some());
        assert_eq!(snapshot["stem_cache"]["available"], json!(false));
        assert_eq!(snapshot["stem_cache"]["pair"], fixture.pair_json);
    }
    assert!(fixture.callback.controls.is_empty());
    let root = fixture.directory.path().join("samples");
    let preparation = fixture.with_engine(|engine| {
        let sample = engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .clone();
        let source = engine.cold_leases.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .clone();
        assert!(
            !source.assignment_retired(),
            "restore starts from actual live source custody"
        );
        // Preparation releases its own subscriber. A clone alone shares the
        // original assignment and would retire source0 when preparation ends.
        let lease = source.subscribe_verified().unwrap();
        assert_ne!(lease.assignment_id(), source.assignment_id());
        prepared_for_test(
            engine,
            PreparedMigrationMaterial::from_current(&root, sample, lease).unwrap(),
            &root,
        )
        .unwrap()
    });
    let loaded = fixture.with_engine(|engine| {
        adopt_for_format(
            engine,
            target,
            &preparation,
            fixture.producer.clone(),
            (2, RATE, root.clone()),
        )
        .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        fixture.callback.drain();
        if fixture.with_engine(|engine| engine.cold_loading[target].load(Ordering::Acquire) == 0) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "restored target needs its own real Cold ACK"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    fixture.with_engine(|engine| {
        let mut success = false;
        while let Ok(event) = engine
            .loader_rx
            .lock()
            .unwrap()
            .try_recv()
            .map(LoaderEvent::into_test_payload)
        {
            match event {
                LoaderEvent::Success { request_id, .. } if request_id == loaded.request_id() => {
                    success = true;
                }
                LoaderEvent::Error { error, .. } => panic!("restored target failed: {error:?}"),
                _ => {}
            }
        }
        assert!(success && loaded.is_current().unwrap());
        let binding = engine
            .current_input_runtime_pad_binding(target)
            .unwrap()
            .unwrap();
        assert!(
            engine
                .input_runtime_ownership
                .binding_source_adoption_accepted(target, binding.binding)
        );
        preparation.release_preparation().unwrap();
        assert!(
            !engine.cold_leases.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .assignment_retired(),
            "target preparation must preserve source0's independent assignment"
        );
        engine.set_pad_bpm(target, Some(BPM)).unwrap();
        engine
            .set_pad_timing_metadata(target, ANCHOR + OFFSET as f64 / f64::from(RATE))
            .unwrap();
        engine
            .set_pad_loop_region(
                target,
                START as f64 / f64::from(RATE),
                Some(END as f64 / f64::from(RATE)),
            )
            .unwrap();
        engine.set_pad_gain(target, -7.0).unwrap();
        engine.set_pad_eq(target, 3.0, -4.0, 1.5).unwrap();
        engine.set_pad_key_lock(target, false).unwrap();
        // BPM/origin setters declare Legacy; restore the actual saved TAP intent last.
        engine.set_pad_timing_intent(target, "tap").unwrap();
    });
    fixture.callback.drain();
    assert_eq!(
        fixture.callback.mixer.loop_region_frames(target),
        (START, Some(END))
    );
    assert!(!fixture.callback.mixer.key_lock_for_measurement(target));
    assert_eq!(
        fixture.callback.mixer.pad_pitch_intent(target),
        Some(crate::audio_engine::pitch_intent::PitchIntent::default())
    );
    let selection = if selected {
        let source = fixture.with_engine(|engine| {
            engine
                .capture_prepared_source(target, fixture.version.clone())
                .unwrap()
        });
        let pair = fixture.with_engine(|engine| {
            engine
                .prepare_stem_pair_at_root(
                    &root,
                    target,
                    &fixture.version,
                    fixture.pair_json["wav_generation"].as_str().unwrap(),
                    &source,
                    all_stems,
                    Some(fixture.pair_json["descriptor_reference"].as_str().unwrap()),
                )
                .unwrap()
        });
        let held_root = fixture.with_engine(|engine| {
            engine.cold_leases.lock().unwrap()[target]
                .as_ref()
                .unwrap()
                .samples_root()
                .to_path_buf()
        });
        assert_ne!(
            pair.root, held_root,
            "exercise ordinary versus Windows canonical spelling"
        );
        assert_eq!(std::fs::canonicalize(&pair.root).unwrap(), held_root);
        assert_eq!(pair.has_components(), all_stems);
        let selection = fixture.with_engine(|engine| {
            engine
                .register_committed_stem_pair_with_producer(&pair, &source, &fixture.producer)
                .unwrap()
        });
        assert_eq!(selection.publication_status(), "pending");
        assert_eq!(fixture.callback.drain(), 1);
        assert_eq!(selection.publication_status(), "accepted");
        fixture.with_engine(|engine| resident_relocation::reconcile(engine).unwrap());
        pair.select();
        if all_stems {
            // Residency effectiveness needs a real current target voice; source0's
            // pair ticket or callback ACK cannot stand in for this new target owner.
            fixture.start(target);
            let residency = fixture.with_engine(|engine| {
                engine
                    .request_committed_stem_residency_with_producer(
                        &selection,
                        "all_stems",
                        0,
                        &fixture.producer,
                    )
                    .unwrap()
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            while residency.effective_mode_status() != "effective" {
                fixture.callback.drain();
                fixture.callback.render(17);
                assert_ne!(
                    residency.residency_status(),
                    "rejected",
                    "{:?}",
                    residency.error()
                );
                assert!(
                    Instant::now() < deadline,
                    "restored target AllStems must receive its own effective ACK: residency={} effect={} error={:?}",
                    residency.residency_status(),
                    residency.effective_mode_status(),
                    residency.error()
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(residency.residency_status(), "accepted");
            fixture.with_engine(|engine| {
                resident_relocation::reconcile(engine).unwrap();
                engine.stop_sample(target).unwrap();
            });
            fixture.callback.drain();
        }
        Some(selection)
    } else {
        None
    };
    fixture.with_engine(|engine| {
        resident_relocation::reconcile(engine).unwrap();
        assert_eq!(engine.pad_timing_intent(target).unwrap(), "tap");
        let binding = engine
            .current_input_runtime_pad_binding(target)
            .unwrap()
            .unwrap();
        assert!(binding.is_current() && binding.available());
        assert!(
            engine
                .input_runtime_ownership
                .binding_source_adoption_accepted(target, binding.binding)
        );
    });
    assert!(
        !fixture
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.sample_id == target && (voice.active || voice.paused))
    );
    let (mode, mask, hash) = fixture.callback.mixer.stem_demand_for_measurement(target);
    if all_stems {
        assert_eq!(
            (mode, mask, hash),
            (
                StemMixMode::AllStems,
                0,
                source_version_hash(&fixture.version)
            )
        );
        assert!(fixture.callback.mixer.stems_for_measurement()[target].is_some());
    } else {
        assert_eq!((mode, mask, hash), (StemMixMode::FullMix, 15, 0));
        assert!(fixture.callback.mixer.stems_for_measurement()[target].is_none());
    }
    assert!(
        fixture
            .callback
            .mixer
            .outgoing_stems_for_measurement(target)
            .is_none()
    );
    assert!(fixture.callback.controls.is_empty());
    selection
}
