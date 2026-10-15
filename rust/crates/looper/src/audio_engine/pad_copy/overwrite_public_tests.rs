//! Actual public capture/prepare/publish/ACK/mirror/finish for occupied targets.
use super::*;
use crate::audio_engine::hold_action::{HoldActionState, HoldActionTicket};
use pyo3::types::PyInt;

fn settle_empty(fixture: &mut Fixture, target: usize) -> PadCopyTicket {
    let captured = fixture.capture(target);
    let ticket = fixture.prepare(&captured);
    fixture.with_engine(|engine| engine.publish_pad_copy(&ticket).unwrap());
    assert_eq!(fixture.callback.drain(), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    fixture.with_engine(|engine| {
        assert!(engine.reconcile_pad_copy(&ticket).unwrap());
        engine.finish_pad_copy(&ticket).unwrap();
    });
    ticket
}

fn hold(fixture: &Fixture, target: usize, press: u64) -> HoldActionTicket {
    fixture.with_engine(|engine| {
        Python::attach(|py| {
            engine
                .pause_waveform_hold(target, PyInt::new(py, press).as_any(), None)
                .unwrap()
        })
    })
}

#[test]
fn actual_overwrite_preserves_source_and_fences_both_removed_hold_release_orders() {
    isolated(
        "overwrite_public_tests::actual_overwrite_preserves_source_and_fences_both_removed_hold_release_orders",
        || {
            for stems in [false, true] {
                for release_first in [false, true] {
                    let mut fixture = Fixture::new(stems, false, "tap");
                    let previous = settle_empty(&mut fixture, 36);
                    let old_selection = previous.committed_stem_ticket().unwrap();
                    let old_lifetime = serde_json::from_str::<Value>(&previous.metadata_json())
                        .unwrap()["content_lifetime"]
                        .as_u64()
                        .unwrap();
                    fixture.start(36);
                    fixture.callback.render(19);
                    let target_hold = hold(&fixture, 36, 9001);
                    fixture.callback.drain();
                    assert_eq!(target_hold.publication_status(), "paused_owned");
                    let source_generation = fixture.source_voice().generation;
                    let source_position = fixture.source_voice().source_playback.position();
                    let source_sample = fixture.callback.mixer.bank_for_measurement()[0]
                        .as_ref()
                        .unwrap()
                        .clone();
                    let captured = fixture.with_engine(|engine| {
                        let source = engine
                            .current_input_runtime_pad_binding(0)
                            .unwrap()
                            .unwrap();
                        let target = engine
                            .current_input_runtime_pad_binding(36)
                            .unwrap()
                            .unwrap();
                        assert!(
                            engine
                                .capture_pad_copy(&source, 36, &fixture.envelope(), None)
                                .is_err()
                        );
                        engine
                            .capture_pad_copy(&source, 36, &fixture.envelope(), Some(&target))
                            .unwrap()
                    });
                    let ticket = fixture.prepare(&captured);
                    if release_first {
                        fixture.with_engine(|engine| {
                            assert!(engine.release_waveform_hold(&target_hold).unwrap())
                        });
                    }
                    fixture.with_engine(|engine| engine.publish_pad_copy(&ticket).unwrap());
                    if !release_first {
                        fixture.with_engine(|engine| {
                            assert!(engine.release_waveform_hold(&target_hold).unwrap())
                        });
                    }
                    fixture.callback.drain();
                    assert_eq!(ticket.publication_status(), "accepted");
                    assert!(target_hold.action.state().terminal());
                    assert_eq!(fixture.source_voice().generation, source_generation);
                    assert_eq!(
                        fixture.source_voice().source_playback.position(),
                        source_position
                    );
                    assert!(
                        !fixture
                            .callback
                            .mixer
                            .voices
                            .iter()
                            .any(|voice| voice.is_playing_sample(36))
                    );
                    let copy_sample = fixture.callback.mixer.bank_for_measurement()[36]
                        .as_ref()
                        .unwrap();
                    assert!(Arc::ptr_eq(&source_sample.samples, &copy_sample.samples));
                    let metadata: Value = serde_json::from_str(&ticket.metadata_json()).unwrap();
                    assert_eq!(
                        metadata["original_target_lifetime"].as_u64(),
                        Some(old_lifetime)
                    );
                    assert_ne!(metadata["content_lifetime"].as_u64(), Some(old_lifetime));
                    assert!(
                        old_selection
                            .as_ref()
                            .is_none_or(|selection| !selection.is_current())
                    );
                    fixture.with_engine(|engine| {
                        assert!(engine.reconcile_pad_copy(&ticket).unwrap());
                        assert_eq!(ticket.committed_stem_ticket().unwrap().is_some(), stems);
                        engine.finish_pad_copy(&ticket).unwrap();
                        assert!(engine.release_waveform_hold(&target_hold).unwrap());
                    });
                    fixture.callback.drain();
                    assert!(
                        !fixture
                            .callback
                            .mixer
                            .voices
                            .iter()
                            .any(|voice| voice.is_playing_sample(36))
                    );
                    assert_eq!(fixture.source_voice().generation, source_generation);
                }
            }
        },
    );
}

#[test]
fn actual_overwrite_cancel_and_stale_equal_pcm_binding_preserve_current_target() {
    isolated(
        "overwrite_public_tests::actual_overwrite_cancel_and_stale_equal_pcm_binding_preserve_current_target",
        || {
            let mut fixture = Fixture::new(false, false, "manual");
            settle_empty(&mut fixture, 215);
            fixture.start(215);
            let old_target = fixture.with_engine(|engine| {
                engine
                    .current_input_runtime_pad_binding(215)
                    .unwrap()
                    .unwrap()
            });
            let first = fixture.with_engine(|engine| {
                let source = engine
                    .current_input_runtime_pad_binding(0)
                    .unwrap()
                    .unwrap();
                engine
                    .capture_pad_copy(&source, 215, &fixture.envelope(), Some(&old_target))
                    .unwrap()
            });
            let prepared = fixture.prepare(&first);
            assert!(fixture.with_engine(|engine| engine.cancel_pad_copy(&first).unwrap()));
            assert_eq!(prepared.publication_status(), "rejected");
            assert!(old_target.is_current());
            assert!(
                fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(215))
            );
            let captured = fixture.with_engine(|engine| {
                let source = engine
                    .current_input_runtime_pad_binding(0)
                    .unwrap()
                    .unwrap();
                engine
                    .capture_pad_copy(&source, 215, &fixture.envelope(), Some(&old_target))
                    .unwrap()
            });
            let copy = fixture.prepare(&captured);
            fixture.with_engine(|engine| engine.publish_pad_copy(&copy).unwrap());
            fixture.callback.drain();
            fixture.with_engine(|engine| {
                assert!(engine.reconcile_pad_copy(&copy).unwrap());
                engine.finish_pad_copy(&copy).unwrap();
                let source = engine
                    .current_input_runtime_pad_binding(0)
                    .unwrap()
                    .unwrap();
                assert!(
                    engine
                        .capture_pad_copy(&source, 215, &fixture.envelope(), Some(&old_target))
                        .is_err()
                );
                assert_eq!(engine.input_runtime_ownership.copy_reservation(215), 0);
            });
            assert!(!old_target.is_current());
            assert_eq!(copy.publication_status(), "accepted");
            assert!(
                !fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(215))
            );
        },
    );
}

#[test]
fn actual_copy_of_held_source_over_playing_target_keeps_original_pause_effect() {
    isolated(
        "overwrite_public_tests::actual_copy_of_held_source_over_playing_target_keeps_original_pause_effect",
        || {
            let mut fixture = Fixture::new(false, false, "tap");
            settle_empty(&mut fixture, 36);
            fixture.start(36);
            let source_hold = hold(&fixture, 0, 4001);
            fixture.callback.drain();
            let original_effect = source_hold.action.effect().unwrap();
            let captured = fixture.with_engine(|engine| {
                let source = engine
                    .current_input_runtime_pad_binding(0)
                    .unwrap()
                    .unwrap();
                let target = engine
                    .current_input_runtime_pad_binding(36)
                    .unwrap()
                    .unwrap();
                engine
                    .capture_pad_copy(&source, 36, &fixture.envelope(), Some(&target))
                    .unwrap()
            });
            let ticket = fixture.prepare(&captured);
            fixture.with_engine(|engine| engine.publish_pad_copy(&ticket).unwrap());
            fixture.callback.drain();
            assert_eq!(source_hold.action.state(), HoldActionState::PausedOwned);
            assert_eq!(source_hold.action.effect(), Some(original_effect));
            assert!(fixture.source_voice().paused);
            fixture.with_engine(|engine| {
                assert!(engine.reconcile_pad_copy(&ticket).unwrap());
                engine.finish_pad_copy(&ticket).unwrap();
                assert!(engine.release_waveform_hold(&source_hold).unwrap());
            });
            fixture.callback.drain();
            assert_eq!(source_hold.action.state(), HoldActionState::Resumed);
            assert!(!fixture.source_voice().paused);
            assert!(
                !fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(36))
            );
        },
    );
}

#[test]
fn actual_full_mix_overwrite_clears_old_target_selected_stem_custody() {
    isolated(
        "overwrite_public_tests::actual_full_mix_overwrite_clears_old_target_selected_stem_custody",
        || {
            let mut fixture = Fixture::new(true, false, "tap");
            let previous = settle_empty(&mut fixture, 36);
            let old_selection = previous.committed_stem_ticket().unwrap().unwrap();
            assert!(old_selection.is_current());
            let root = fixture.directory.path().join("samples");
            let preparation = fixture.with_engine(|engine| {
                // The original material preparation assignment was released by
                // Fixture startup. Use an independent verified subscriber of the
                // actual living source; release below must never retire source0.
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
                    1,
                    &preparation,
                    fixture.producer.clone(),
                    (2, RATE, root.clone()),
                )
                .unwrap()
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                fixture.callback.drain();
                if fixture.with_engine(|engine| engine.cold_loading[1].load(Ordering::Acquire)) == 0
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "FullMix source lacks its actual source ACK"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
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
                            id: 1, request_id, ..
                        } if request_id == loaded.request_id() => success = true,
                        LoaderEvent::Error {
                            id: 1,
                            request_id,
                            error,
                        } if request_id == loaded.request_id() => {
                            panic!("second actual Cold source failed before overwrite: {error}");
                        }
                        _ => {}
                    }
                }
                success
            });
            assert!(
                success,
                "second source requires its own matching terminal Success"
            );
            assert_eq!(loaded.phase().unwrap(), "acknowledged");
            assert!(loaded.is_current().unwrap());
            preparation.release_preparation().unwrap();
            assert!(
                loaded.is_current().unwrap(),
                "release must retire only the independent preparation assignment"
            );
            fixture.with_engine(|engine| {
                assert!(
                    !engine.cold_leases.lock().unwrap()[0]
                        .as_ref()
                        .unwrap()
                        .assignment_retired()
                );
                assert!(
                    engine
                        .current_input_runtime_pad_binding(0)
                        .unwrap()
                        .unwrap()
                        .is_current()
                );
                engine.set_pad_bpm(1, Some(BPM)).unwrap();
                engine.set_pad_timing_intent(1, "tap").unwrap();
                engine.set_pad_key_lock(1, false).unwrap();
            });
            fixture.callback.drain();
            let mut envelope: Value = serde_json::from_str(&fixture.envelope()).unwrap();
            envelope["snapshot"]["stem_cache"] = Value::Null;
            envelope["snapshot"]["stem_mix_mode"] = json!("full_mix");
            let captured = fixture.with_engine(|engine| {
                let source = engine
                    .current_input_runtime_pad_binding(1)
                    .unwrap()
                    .unwrap();
                let target = engine
                    .current_input_runtime_pad_binding(36)
                    .unwrap()
                    .unwrap();
                engine
                    .capture_pad_copy(&source, 36, &envelope.to_string(), Some(&target))
                    .unwrap()
            });
            let ticket = fixture.prepare(&captured);
            fixture.with_engine(|engine| engine.publish_pad_copy(&ticket).unwrap());
            fixture.callback.drain();
            assert_eq!(ticket.publication_status(), "accepted");
            assert!(!old_selection.is_current());
            fixture.with_engine(|engine| {
                assert!(engine.reconcile_pad_copy(&ticket).unwrap());
                assert!(ticket.committed_stem_ticket().unwrap().is_none());
                let resident = engine.resident_stem_cache.lock().unwrap();
                assert!(
                    resident[36].accepted.is_none(),
                    "removed B component owner cannot survive in C"
                );
                assert!(!resident[36].ordinary_publication_pending());
                drop(resident);
                engine.finish_pad_copy(&ticket).unwrap();
            });
            assert_eq!(
                fixture.callback.mixer.stem_demand_for_measurement(36).0,
                StemMixMode::FullMix
            );
            assert!(
                !fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(36))
            );
        },
    );
}
