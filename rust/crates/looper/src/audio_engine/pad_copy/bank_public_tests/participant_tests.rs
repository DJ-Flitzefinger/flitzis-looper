//! Original complete witnesses enter the real public aggregate; metadata never grants authority.
use super::*;
use crate::audio_engine::BankParticipants;
use crate::audio_engine::input_mapping::{InputRuntime, PitchPadProjection};
use crate::audio_engine::{
    LoadedSourcePublication, SourcePitchInitialization, publish_loaded_sample_with_arm,
};

fn view(fixture: &Fixture, source: Option<usize>, target: usize) -> BankParticipants {
    fixture.with_engine(|engine| engine.capture_bank_participants(source, target).unwrap())
}

fn intents(fixture: &Fixture, source: Option<usize>) -> Vec<Option<String>> {
    (0..36)
        .map(|index| {
            source.and_then(|bank| {
                fixture.callback.mixer.bank_for_measurement()[bank * 36 + index]
                    .as_ref()
                    .map(|_| fixture.envelope())
            })
        })
        .collect()
}

fn reject_unchanged(
    fixture: &mut Fixture,
    original: &BankParticipants,
    source: Option<usize>,
    target: usize,
) {
    let queue = fixture.callback.controls.slots();
    let native: Vec<_> = fixture
        .callback
        .mixer
        .bank_for_measurement()
        .iter()
        .map(|sample| {
            sample
                .as_ref()
                .map(|sample| sample.samples.as_ptr() as usize)
        })
        .collect();
    let intent = intents(fixture, source);
    fixture.with_engine(|engine| {
        let requests = engine.pad_request_ids.lock().unwrap().clone();
        let snapshots: Vec<_> = (0..NUM_SAMPLES)
            .map(|id| engine.input_runtime_ownership.slot_snapshot(id).unwrap())
            .collect();
        let epochs: Vec<_> = (0..NUM_SAMPLES)
            .map(|id| {
                engine
                    .prepared_source_epoch(id)
                    .unwrap()
                    .load(Ordering::Acquire)
            })
            .collect();
        let leases: Vec<_> = engine
            .cold_leases
            .lock()
            .unwrap()
            .iter()
            .map(|lease| {
                lease.as_ref().map(|lease| {
                    (
                        lease.assignment_id(),
                        lease.cache_assignment_count(),
                        lease.assignment_retired(),
                    )
                })
            })
            .collect();
        let owners = engine.bank_mutations.lock().unwrap().len();
        assert!(
            engine
                .capture_bank_mutation_with_participants(source, target, intent, Some(original))
                .is_err()
        );
        assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests);
        assert_eq!(engine.bank_mutations.lock().unwrap().len(), owners);
        for (id, before) in snapshots.iter().enumerate() {
            assert!(
                engine
                    .input_runtime_ownership
                    .slot_snapshot(id)
                    .unwrap()
                    .same_projection(*before)
            );
            assert_eq!(engine.input_runtime_ownership.copy_reservation(id), 0);
            assert_eq!(engine.input_runtime_ownership.pair_reservation(id), 0);
            assert_eq!(
                engine
                    .prepared_source_epoch(id)
                    .unwrap()
                    .load(Ordering::Acquire),
                epochs[id]
            );
        }
        let after: Vec<_> = engine
            .cold_leases
            .lock()
            .unwrap()
            .iter()
            .map(|lease| {
                lease.as_ref().map(|lease| {
                    (
                        lease.assignment_id(),
                        lease.cache_assignment_count(),
                        lease.assignment_retired(),
                    )
                })
            })
            .collect();
        assert_eq!(after, leases);
    });
    assert_eq!(fixture.callback.controls.slots(), queue);
    let after: Vec<_> = fixture
        .callback
        .mixer
        .bank_for_measurement()
        .iter()
        .map(|sample| {
            sample
                .as_ref()
                .map(|sample| sample.samples.as_ptr() as usize)
        })
        .collect();
    assert_eq!(after, native);
}

#[test]
fn original_views_are_read_only_and_reject_foreign_or_wrong_ordered_banks() {
    isolated(
        "bank_public_tests::participant_tests::original_views_are_read_only_and_reject_foreign_or_wrong_ordered_banks",
        || {
            let mut fixture = bank_fixture(false, "manual");
            let foreign_fixture = bank_fixture(false, "manual");
            let sample = fixture.with_engine(|engine| {
                engine.sample_cache.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .samples
                    .clone()
            });
            let lease = fixture.with_engine(|engine| {
                engine.cold_leases.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .clone()
            });
            let readers = Arc::strong_count(&sample);
            let assignments = lease.cache_assignment_count();
            let original = view(&fixture, Some(0), 1);
            assert_eq!(Arc::strong_count(&sample), readers);
            assert_eq!(lease.cache_assignment_count(), assignments);
            fixture.with_engine(|engine| {
                for id in 0..72 {
                    assert_eq!(engine.input_runtime_ownership.copy_reservation(id), 0);
                }
            });
            let foreign = view(&foreign_fixture, Some(0), 1);
            reject_unchanged(&mut fixture, &foreign, Some(0), 1);
            reject_unchanged(&mut fixture, &original, Some(0), 2);
            reject_unchanged(&mut fixture, &original, Some(1), 0);
            reject_unchanged(&mut fixture, &original, None, 1);
        },
    );
}

#[test]
fn completed_same_pcm_digest_and_generation_replacement_rejects_original_bank_view() {
    isolated(
        "bank_public_tests::participant_tests::completed_same_pcm_digest_and_generation_replacement_rejects_original_bank_view",
        || {
            let mut fixture = bank_fixture(false, "manual");
            let original = view(&fixture, Some(0), 1);
            let (sample, generation, before) = fixture.with_engine(|engine| {
                let sample = engine.sample_cache.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .clone();
                let binding = engine
                    .current_input_runtime_pad_binding(0)
                    .unwrap()
                    .unwrap();
                (
                    sample,
                    binding.source_generation,
                    engine.input_runtime_ownership.slot_snapshot(0).unwrap(),
                )
            });
            fixture.with_engine(|engine| {
                let _requests = engine.pad_request_ids.lock().unwrap();
                let mut generations = engine.loaded_source_generations.lock().unwrap();
                let mut digests = engine.loaded_source_digests.lock().unwrap();
                let digest = digests[0].clone().unwrap();
                publish_loaded_sample_with_arm(
                    &fixture.producer,
                    &engine.sample_cache,
                    0,
                    sample.clone(),
                    LoadedSourcePublication {
                        initial_mix: None,
                        ownership: &engine.input_runtime_ownership,
                        placement: before,
                        generation,
                        rate: RATE,
                        generation_slot: &mut generations[0],
                        digest_slot: &mut digests[0],
                        digest,
                        cold: false,
                        cold_epoch: None,
                        cold_adoption: None,
                        replace_assignment: true,
                        loop_region: None,
                        resident_cancelled: None,
                        incoming_defaults: None,
                        initial_pitch: SourcePitchInitialization::FreshNeutral,
                        intent: None,
                    },
                    || Ok(()),
                )
                .unwrap();
            });
            assert_eq!(
                fixture.callback.drain(),
                1,
                "the replacement must actually complete its callback"
            );
            assert!(Arc::ptr_eq(
                &fixture.callback.mixer.bank_for_measurement()[0]
                    .as_ref()
                    .unwrap()
                    .samples,
                &sample.samples
            ));
            fixture.with_engine(|engine| {
                let binding = engine
                    .current_input_runtime_pad_binding(0)
                    .unwrap()
                    .unwrap();
                assert!(binding.is_current());
                assert_eq!(binding.source_generation, generation);
                assert_ne!(
                    engine
                        .input_runtime_ownership
                        .slot_snapshot(0)
                        .unwrap()
                        .lifetime(),
                    before.lifetime()
                );
                assert!(engine.capture_bank_participants(Some(0), 1).is_ok());
            });
            reject_unchanged(&mut fixture, &original, Some(0), 1);
        },
    );
}

#[test]
fn completed_empty_copy_unload_aba_rejects_last_original_source_and_target_witnesses() {
    isolated(
        "bank_public_tests::participant_tests::completed_empty_copy_unload_aba_rejects_last_original_source_and_target_witnesses",
        || {
            let mut fixture = bank_fixture(false, "tap");
            let original = view(&fixture, Some(0), 1);
            let before = fixture
                .with_engine(|engine| engine.input_runtime_ownership.slot_snapshot(71).unwrap());
            assert_eq!(before.lifetime(), 0);
            put(&mut fixture, 71);
            fixture.with_engine(|engine| engine.unload_sample(71).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            let after = fixture
                .with_engine(|engine| engine.input_runtime_ownership.slot_snapshot(71).unwrap());
            assert_eq!(after.lifetime(), 0);
            assert_ne!(after.binding_epoch(), before.binding_epoch());
            assert!(fixture.callback.mixer.bank_for_measurement()[71].is_none());
            reject_unchanged(&mut fixture, &original, Some(0), 1);

            let original = view(&fixture, Some(0), 1);
            let source_before = fixture
                .with_engine(|engine| engine.input_runtime_ownership.slot_snapshot(0).unwrap());
            put(&mut fixture, 35);
            fixture.with_engine(|engine| engine.unload_sample(35).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            assert!(fixture.callback.mixer.bank_for_measurement()[35].is_none());
            fixture.with_engine(|engine| {
                assert_eq!(
                    engine
                        .input_runtime_ownership
                        .slot_snapshot(35)
                        .unwrap()
                        .lifetime(),
                    0
                );
                assert!(
                    engine
                        .input_runtime_ownership
                        .slot_snapshot(0)
                        .unwrap()
                        .same_projection(source_before)
                );
            });
            reject_unchanged(&mut fixture, &original, Some(0), 1);
            let clear_original = view(&fixture, None, 1);
            fixture.with_engine(|engine| {
                let captured = engine
                    .capture_bank_mutation_with_participants(
                        None,
                        1,
                        vec![None; 36],
                        Some(&clear_original),
                    )
                    .unwrap();
                assert!(engine.cancel_bank_mutation(&captured).unwrap());
            });
        },
    );
}

fn publish_runtime(fixture: &Fixture, last_end: usize) {
    fixture.with_engine(|engine| {
        if engine.input_runtime.is_none() {
            engine.input_runtime = Some(InputRuntime::new_with_ownership(
                fixture.producer.clone(),
                engine.input_clock,
                engine.input_runtime_ownership.clone(),
            ));
        }
        let bindings: Vec<_> = (0..NUM_SAMPLES)
            .map(|id| engine.current_input_runtime_pad_binding(id).unwrap())
            .collect();
        let loaded: Vec<_> = bindings.iter().map(Option::is_some).collect();
        let starts = (0..NUM_SAMPLES)
            .map(|id| {
                if loaded[id] {
                    START as f64 / f64::from(RATE)
                } else {
                    0.0
                }
            })
            .collect();
        let ends = (0..NUM_SAMPLES)
            .map(|id| {
                loaded[id].then_some(if id == 35 { last_end } else { END } as f64 / f64::from(RATE))
            })
            .collect();
        engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_runtime_state_with_pitch(
                true,
                loaded,
                starts,
                ends,
                bindings.iter().map(Option::as_ref).collect(),
                vec![PitchPadProjection::default(); NUM_SAMPLES],
            )
            .unwrap();
    });
}

#[test]
fn final_source_runtime_and_logical_request_changes_reject_original_complete_view() {
    isolated(
        "bank_public_tests::participant_tests::final_source_runtime_and_logical_request_changes_reject_original_complete_view",
        || {
            let mut fixture = bank_fixture(false, "manual");
            put(&mut fixture, 35);
            publish_runtime(&fixture, END);
            let original = view(&fixture, Some(0), 1);
            fixture.with_engine(|engine| {
                assert!(engine.input_runtime_ownership.runtime_revision(35) > 0)
            });
            publish_runtime(&fixture, END - 1);
            reject_unchanged(&mut fixture, &original, Some(0), 1);
            let original = view(&fixture, Some(0), 1);
            fixture.with_engine(|engine| {
                let _requests = engine.pad_request_ids.lock().unwrap();
                engine
                    .input_runtime_ownership
                    .prepare_source_request_advance(35)
                    .unwrap()
                    .commit();
            });
            reject_unchanged(&mut fixture, &original, Some(0), 1);
        },
    );
}

#[test]
fn unrelated_real_copy_preserves_original_view_and_source_hold_release_under_bank_reservation() {
    isolated(
        "bank_public_tests::participant_tests::unrelated_real_copy_preserves_original_view_and_source_hold_release_under_bank_reservation",
        || {
            let mut fixture = bank_fixture(false, "manual");
            let held = hold(&fixture, 0, 401);
            fixture.callback.drain();
            let ordinary = fixture.with_engine(|engine| {
                engine
                    .current_input_runtime_pad_binding(0)
                    .unwrap()
                    .unwrap()
            });
            let original = view(&fixture, Some(0), 1);
            let before = fixture
                .with_engine(|engine| engine.input_runtime_ownership.slot_snapshot(0).unwrap());
            put(&mut fixture, 72);
            let after = fixture
                .with_engine(|engine| engine.input_runtime_ownership.slot_snapshot(0).unwrap());
            assert!(before.same_projection(after));
            assert_ne!(before.layout_revision(), after.layout_revision());
            let captured = fixture.with_engine(|engine| {
                engine
                    .capture_bank_mutation_with_participants(
                        Some(0),
                        1,
                        intents(&fixture, Some(0)),
                        Some(&original),
                    )
                    .unwrap()
            });
            fixture.with_engine(|engine| {
                assert!(!ordinary.is_current());
                assert!(
                    engine.current_input_runtime_pad_binding(0).is_err(),
                    "ordinary binding admission still refuses own Bank reservation"
                );
                assert!(engine.release_waveform_hold(&held).unwrap());
            });
            fixture.callback.drain();
            assert_eq!(held.action.state(), HoldActionState::Resumed);
            let ticket = prepare(&fixture, &captured);
            fixture.with_engine(|engine| engine.publish_bank_mutation(&ticket).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(ticket.publication_status(), "accepted");
            finish(&fixture, &ticket);
            assert!(fixture.callback.mixer.bank_for_measurement()[72].is_some());
        },
    );
}

#[test]
fn active_placement_marker_returns_bounded_pending_without_bank_side_effects() {
    isolated(
        "bank_public_tests::participant_tests::active_placement_marker_returns_bounded_pending_without_bank_side_effects",
        || {
            let fixture = bank_fixture(false, "manual");
            let original = view(&fixture, Some(0), 1);
            let intent = intents(&fixture, Some(0));
            let ownership = fixture.with_engine(|engine| engine.input_runtime_ownership.clone());
            let before = ownership.slot_snapshot(72).unwrap();
            let marker = ownership
                .prepare_slot_write_fixed(before, before.lifetime())
                .unwrap();
            let requests =
                fixture.with_engine(|engine| engine.pad_request_ids.lock().unwrap().clone());
            ownership
                .commit_slot_write_fixed(
                    &marker,
                    || true,
                    |_| {
                        fixture.with_engine(|engine| {
                            assert!(engine.capture_bank_participants(Some(0), 1).is_err());
                            assert!(
                                engine
                                    .capture_bank_mutation_with_participants(
                                        Some(0),
                                        1,
                                        intent,
                                        Some(&original)
                                    )
                                    .is_err()
                            );
                            assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests);
                            for id in 0..72 {
                                assert_eq!(ownership.copy_reservation(id), 0);
                            }
                            assert!(engine.bank_mutations.lock().unwrap().is_empty());
                        });
                    },
                )
                .unwrap();
            assert!(fixture.callback.controls.is_empty());
            assert!(
                fixture.callback.mixer.bank_for_measurement()[36..72]
                    .iter()
                    .all(Option::is_none)
            );
        },
    );
}

#[test]
fn admitted_cold_load_is_never_recaptured_as_empty_and_rejections_publish_no_bank_work() {
    isolated(
        "bank_public_tests::participant_tests::admitted_cold_load_is_never_recaptured_as_empty_and_rejections_publish_no_bank_work",
        || {
            let mut fixture = bank_fixture(false, "manual");
            let original = view(&fixture, Some(0), 1);
            let root = fixture.directory.path().join("samples");
            let preparation = fixture.with_engine(|engine| {
                let _requests = engine.pad_request_ids.lock().unwrap();
                let sample = engine.sample_cache.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .clone();
                let lease = engine.cold_leases.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .subscribe_verified()
                    .unwrap();
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
                    71,
                    &preparation,
                    fixture.producer.clone(),
                    (2, RATE, root.clone()),
                )
                .unwrap()
            });
            // Freeze at the actual queued Cold publication before its callback ACK.
            let deadline = Instant::now() + Duration::from_secs(10);
            while fixture.callback.controls.is_empty() {
                assert!(
                    Instant::now() < deadline,
                    "Cold did not publish its owned command"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            let intent = intents(&fixture, Some(0));
            let queue = fixture.callback.controls.slots();
            let (rejected, readonly_rejected, unchanged) = fixture.with_engine(|engine| {
                let requests = engine.pad_request_ids.lock().unwrap().clone();
                let snapshots: Vec<_> = (0..72)
                    .map(|id| engine.input_runtime_ownership.slot_snapshot(id).unwrap())
                    .collect();
                let owners = engine.bank_mutations.lock().unwrap().len();
                let rejected = engine
                    .capture_bank_mutation_with_participants(Some(0), 1, intent, Some(&original))
                    .is_err();
                let readonly_rejected = engine.capture_bank_participants(Some(0), 1).is_err();
                let unchanged = *engine.pad_request_ids.lock().unwrap() == requests
                    && engine.bank_mutations.lock().unwrap().len() == owners
                    && snapshots.iter().enumerate().all(|(id, before)| {
                        engine
                            .input_runtime_ownership
                            .slot_snapshot(id)
                            .unwrap()
                            .same_projection(*before)
                            && engine.input_runtime_ownership.copy_reservation(id) == 0
                            && engine.input_runtime_ownership.pair_reservation(id) == 0
                    });
                (rejected, readonly_rejected, unchanged)
            });
            let queue_unchanged = fixture.callback.controls.slots() == queue;
            // Always settle the real admitted worker before asserting the capture outcomes.
            let deadline = Instant::now() + Duration::from_secs(10);
            while fixture.with_engine(|engine| engine.cold_loading[71].load(Ordering::Acquire)) != 0
            {
                fixture.callback.drain();
                assert!(
                    Instant::now() < deadline,
                    "Cold did not receive its actual terminal ACK"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(loaded.phase().unwrap(), "acknowledged");
            let success = fixture.with_engine(|engine| {
                let mut success = false;
                while let Ok(event) = engine
                    .loader_rx
                    .lock()
                    .unwrap()
                    .try_recv()
                    .map(LoaderEvent::into_test_payload)
                {
                    match event {
                        LoaderEvent::Success {
                            id: 71, request_id, ..
                        } if request_id == loaded.request_id() => success = true,
                        LoaderEvent::Error {
                            id: 71,
                            request_id,
                            error,
                        } if request_id == loaded.request_id() => {
                            panic!("the independently admitted Cold load failed: {error}");
                        }
                        _ => {}
                    }
                }
                success
            });
            preparation.release_preparation().unwrap();
            assert!(success && rejected && readonly_rejected && unchanged && queue_unchanged);
            assert!(
                fixture.callback.mixer.bank_for_measurement()[36..71]
                    .iter()
                    .all(Option::is_none)
            );
            assert!(fixture.callback.mixer.bank_for_measurement()[71].is_some());
        },
    );
}

#[test]
fn selected_request_stales_original_view_and_component_metadata_distinguishes_registration() {
    isolated(
        "bank_public_tests::participant_tests::selected_request_stales_original_view_and_component_metadata_distinguishes_registration",
        || {
            let mut fixture = bank_fixture_with_stems(true, false, "manual");
            let copied = fixture.capture(35);
            let copied_ticket = fixture.prepare(&copied);
            fixture.with_engine(|engine| engine.publish_pad_copy(&copied_ticket).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            fixture
                .with_engine(|engine| assert!(engine.reconcile_pad_copy(&copied_ticket).unwrap()));
            let selection = copied_ticket.committed_stem_ticket().unwrap().unwrap();
            fixture.with_engine(|engine| engine.finish_pad_copy(&copied_ticket).unwrap());
            fixture.start(35);
            let original = view(&fixture, Some(0), 1);
            let full_mix = fixture.with_engine(|engine| {
                engine
                    .request_committed_stem_residency_with_producer(
                        &selection,
                        "full_mix",
                        0,
                        &fixture.producer,
                    )
                    .unwrap()
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            while full_mix.effective_mode_status() != "effective" {
                fixture.callback.drain();
                fixture.callback.render(17);
                assert!(
                    Instant::now() < deadline,
                    "FullMix did not receive its own effective ACK: {:?}",
                    full_mix.error()
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            fixture.with_engine(|engine| resident_relocation::reconcile(engine).unwrap());
            reject_unchanged(&mut fixture, &original, Some(0), 1);
            assert!(selection.is_current());
            let original = view(&fixture, Some(0), 1);
            let mut intent = intents(&fixture, Some(0));
            let mut full_mix_intent: Value =
                serde_json::from_str(intent[35].as_ref().unwrap()).unwrap();
            full_mix_intent["snapshot"]["stem_mix_mode"] = json!("full_mix");
            intent[35] = Some(full_mix_intent.to_string());
            let captured = fixture.with_engine(|engine| {
                engine
                    .capture_bank_mutation_with_participants(Some(0), 1, intent, Some(&original))
                    .unwrap()
            });
            let ticket = prepare(&fixture, &captured);
            let before: Value = serde_json::from_str(&ticket.metadata_json()).unwrap();
            assert!(
                before["targets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|row| row["components_available"] == json!(false))
            );
            fixture.with_engine(|engine| engine.publish_bank_mutation(&ticket).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            let after: Value = serde_json::from_str(&ticket.metadata_json()).unwrap();
            for index in 0..36 {
                assert_eq!(
                    after["targets"][index]["components_available"],
                    json!(index == 0)
                );
            }
            let ack = ticket.acknowledgement().unwrap();
            fixture.with_engine(|engine| {
                assert!(engine.reconcile_bank_mutation(&ticket, &ack).unwrap())
            });
            let selections = ticket.committed_stem_tickets().unwrap();
            assert!(selections[0].as_ref().unwrap().is_current());
            assert!(
                selections[35].as_ref().unwrap().is_current(),
                "registered disk pair alone is not resident components"
            );
            assert!(selections[1..35].iter().all(Option::is_none));
            fixture.with_engine(|engine| engine.finish_bank_mutation(&ticket, &ack).unwrap());
        },
    );
}
