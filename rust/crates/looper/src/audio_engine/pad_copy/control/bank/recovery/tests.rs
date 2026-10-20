//! Real callback verification of restored images; held messages are labelled faults.
use super::*;
use crate::audio_engine::RecoveredBankAcceptance;
use crate::audio_engine::buffer_retirement::AudioBufferRetirement;
use crate::audio_engine::{
    LoadedSourcePublication, SourcePitchInitialization, publish_loaded_sample_with_arm,
};

#[path = "automatic_tests.rs"]
mod automatic_tests;

#[path = "restoration_fixture.rs"]
mod restoration_fixture;

#[path = "parked_mask_tests.rs"]
mod parked_mask_tests;

fn recovery_fixture(stems: bool, registered: bool) -> BankFixture {
    use std::io::Write;
    use std::os::windows::fs::MetadataExt;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let parent = workspace.join("test-artifacts/bank36-recovery-20261020");
    for ancestor in parent.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => assert_eq!(
                metadata.file_attributes() & 0x400,
                0,
                "owned recovery root must not traverse reparse points"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("owned recovery ancestor is unavailable: {error}"),
        }
    }
    fs::create_dir_all(&parent).unwrap();
    let owned = tempfile::Builder::new()
        .prefix("native-recovered-bank-")
        .tempdir_in(parent)
        .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(owned.path().join("OWNER.md"))
        .unwrap()
        .write_all(
            b"Owned synthetic Native recovered Bank fixture. No private audio, App or device.",
        )
        .unwrap();
    let fixture = Fixture::with_pair_registration_at(
        stems,
        registered,
        false,
        "tap",
        Some(owned.path().to_path_buf()),
    );
    BankFixture {
        fixture,
        _owned_root: owned,
    }
}

fn view(fixture: &Fixture, target: usize) -> crate::audio_engine::BankParticipants {
    fixture.with_engine(|engine| engine.capture_bank_participants(None, target).unwrap())
}

fn restored_image(fixture: &Fixture) -> String {
    let mut image: Value = serde_json::from_str(&fixture.envelope()).unwrap();
    if image["snapshot"]["stem_mix_mode"] == json!("full_mix") {
        // These genuine Cold-restored cells use the ordinary fresh FullMix
        // mask. The Copy fixture's dormant mask0 is a different saved intent.
        image["snapshot"]["stem_intent"]["enabled_mask"] = json!(15);
    }
    image.to_string()
}

fn put(fixture: &mut Fixture, target: usize) {
    let image = restored_image(fixture);
    let registration = restoration_fixture::restore_target(fixture, target, &image);
    assert!(
        registration.is_none(),
        "unselected recovery fixture must not create selection authority"
    );
}

fn images(fixture: &Fixture, bank: usize) -> Vec<Option<String>> {
    fixture.callback.mixer.bank_for_measurement()[bank * 36..bank * 36 + 36]
        .iter()
        .map(|sample| sample.as_ref().map(|_| restored_image(fixture)))
        .collect()
}

fn status(acceptance: &RecoveredBankAcceptance) -> Value {
    serde_json::from_str(&acceptance.metadata_json()).unwrap()
}

fn accept(
    fixture: &Fixture,
    original: &crate::audio_engine::BankParticipants,
    images: Vec<Option<String>>,
) -> RecoveredBankAcceptance {
    fixture.with_engine(|engine| engine.capture_recovered_bank(original, images).unwrap())
}

fn all_unreserved(fixture: &Fixture) {
    fixture.with_engine(|engine| {
        for id in 0..NUM_SAMPLES {
            assert_eq!(
                engine.input_runtime_ownership.copy_reservation(id),
                0,
                "slot{id}"
            );
            assert_eq!(
                engine.input_runtime_ownership.pair_reservation(id),
                0,
                "slot{id}"
            );
        }
    });
}

fn snapshots(fixture: &Fixture) -> Vec<crate::audio_engine::content_placement::SlotSnapshot> {
    fixture.with_engine(|engine| {
        (0..NUM_SAMPLES)
            .map(|id| engine.input_runtime_ownership.slot_snapshot(id).unwrap())
            .collect()
    })
}

fn unchanged(fixture: &Fixture, before: &[crate::audio_engine::content_placement::SlotSnapshot]) {
    fixture.with_engine(|engine| {
        for (id, before) in before.iter().enumerate() {
            assert!(
                engine
                    .input_runtime_ownership
                    .slot_snapshot(id)
                    .unwrap()
                    .same_projection(*before),
                "slot{id}"
            );
        }
    });
}

fn release(fixture: &Fixture, acceptance: &RecoveredBankAcceptance) {
    fixture.with_engine(|engine| {
        assert!(engine.release_recovered_bank(acceptance).unwrap());
        assert!(engine.release_recovered_bank(acceptance).unwrap());
        assert!(!engine.validate_recovered_bank(acceptance).unwrap());
    });
    assert_eq!(status(acceptance)["status"], json!("released"));
    all_unreserved(fixture);
}

fn put_selected_image(
    fixture: &mut Fixture,
    target: usize,
    image: &str,
) -> crate::audio_engine::CommittedStemSetTicket {
    let registration = restoration_fixture::restore_target(fixture, target, image).unwrap();
    assert_eq!(registration.publication_status(), "accepted");
    assert!(registration.is_current());
    registration
}

#[test]
fn accepted_all_stems_custody_is_required_and_revocation_cannot_fallback_to_worker() {
    isolated(
        "bank_public_tests::recovered_bank_tests::accepted_all_stems_custody_is_required_and_revocation_cannot_fallback_to_worker",
        || {
            let mut fixture = recovery_fixture(true, true);
            let image = restored_image(&fixture);
            let registration = put_selected_image(&mut fixture, 36, &image);
            fixture.with_engine(|engine| {
                let owners = engine.resident_stem_cache.lock().unwrap();
                assert!(
                    owners[36].accepted.is_some(),
                    "the selected test must own actual accepted component PCM"
                );
            });
            assert!(fixture.callback.mixer.stems_for_measurement()[36].is_some());
            let original = view(&fixture, 1);
            let acceptance = accept(&fixture, &original, images(&fixture, 1));
            assert_eq!(fixture.callback.drain(), 1);
            fixture.with_engine(|engine| {
                assert!(engine.validate_recovered_bank(&acceptance).unwrap())
            });
            release(&fixture, &acceptance);
            drop(acceptance);

            // This is the actual accepted target registration's public revocation.
            // Its retained readers stay live, so file/PCM presence alone cannot pass.
            let original = view(&fixture, 1);
            let before = snapshots(&fixture);
            let requests =
                fixture.with_engine(|engine| engine.pad_request_ids.lock().unwrap().clone());
            assert!(registration.cancel());
            assert!(!registration.is_current());
            assert!(fixture.callback.mixer.stems_for_measurement()[36].is_some());
            fixture.with_engine(|engine| {
                assert!(
                    engine
                        .capture_recovered_bank(&original, images(&fixture, 1))
                        .is_err()
                )
            });
            assert!(
                fixture.callback.controls.is_empty(),
                "strict recovery must not enqueue a fallback selection worker/publication"
            );
            unchanged(&fixture, &before);
            fixture.with_engine(|engine| {
                assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests)
            });
            all_unreserved(&fixture);
        },
    );
}

#[test]
fn full_mix_disk_only_accepted_pair_is_pinned_without_claiming_component_availability() {
    isolated(
        "bank_public_tests::recovered_bank_tests::full_mix_disk_only_accepted_pair_is_pinned_without_claiming_component_availability",
        || {
            let mut fixture = recovery_fixture(true, true);
            let mut image: Value = serde_json::from_str(&restored_image(&fixture)).unwrap();
            image["snapshot"]["stem_mix_mode"] = json!("full_mix");
            image["snapshot"]["stem_intent"]["enabled_mask"] = json!(15);
            let image = image.to_string();
            let registration = put_selected_image(&mut fixture, 36, &image);
            assert_eq!(registration.publication_status(), "accepted");
            assert!(
                registration.is_current(),
                "FullMix retains its actual selected disk owner"
            );
            fixture.with_engine(|engine| {
                let owners = engine.resident_stem_cache.lock().unwrap();
                assert!(
                    owners[36].accepted.is_none(),
                    "registration does not imply resident components"
                );
            });
            assert!(fixture.callback.mixer.stems_for_measurement()[36].is_none());
            let original = view(&fixture, 1);
            let mut expected = vec![None; 36];
            expected[0] = Some(image);
            let acceptance = accept(&fixture, &original, expected);
            let occupied = acceptance.state.images[0].as_ref().unwrap();
            assert!(occupied.selection.is_some());
            assert!(occupied.stems.is_none());
            assert_eq!(fixture.callback.drain(), 1);
            fixture.with_engine(|engine| {
                assert!(engine.validate_recovered_bank(&acceptance).unwrap())
            });
            assert!(registration.is_current());
            assert!(fixture.callback.mixer.stems_for_measurement()[36].is_none());
            // Existing public owner revocation must invalidate the retained custody,
            // even though the sample, immutable pair and callback receipt remain.
            assert!(registration.cancel());
            fixture.with_engine(|engine| {
                assert!(!engine.validate_recovered_bank(&acceptance).unwrap())
            });
            release(&fixture, &acceptance);
        },
    );
}

fn continuity_profile(query: bool) -> Vec<f32> {
    let mut fixture = recovery_fixture(false, true);
    for target in [36, 71, 72] {
        put(&mut fixture, target);
    }
    fixture.start(72);
    let source = hold(&fixture, 0, 801);
    let outside = hold(&fixture, 72, 802);
    fixture.callback.drain();
    assert_eq!(source.action.state(), HoldActionState::PausedOwned);
    assert_eq!(outside.action.state(), HoldActionState::PausedOwned);
    let before = [voice(&fixture, 0), voice(&fixture, 72)];
    let slots = snapshots(&fixture);
    let requests = fixture.with_engine(|engine| engine.pad_request_ids.lock().unwrap().clone());
    let frame = fixture.callback.frame;
    if query {
        let original = view(&fixture, 1);
        let acceptance = accept(&fixture, &original, images(&fixture, 1));
        assert_eq!(status(&acceptance)["status"], json!("pending"));
        fixture.with_engine(|engine| {
            assert!(!engine.validate_recovered_bank(&acceptance).unwrap());
            assert!(!engine.release_recovered_bank(&acceptance).unwrap());
            for id in 0..NUM_SAMPLES {
                assert_eq!(
                    engine.input_runtime_ownership.copy_reservation(id) != 0,
                    (36..72).contains(&id)
                );
            }
        });
        assert_eq!(fixture.callback.controls.slots(), 1);
        assert_eq!(fixture.callback.drain(), 1);
        assert_eq!(status(&acceptance)["status"], json!("accepted"));
        assert_eq!(status(&acceptance)["accepted_nonce"], json!(1));
        assert_eq!(status(&acceptance)["accepted_output_frame"], json!(frame));
        fixture.with_engine(|engine| assert!(engine.validate_recovered_bank(&acceptance).unwrap()));
        release(&fixture, &acceptance);
    }
    unchanged(&fixture, &slots);
    assert_eq!(fixture.callback.frame, frame);
    assert_eq!([voice(&fixture, 0), voice(&fixture, 72)], before);
    fixture.with_engine(|engine| assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests));
    for action in [&source, &outside] {
        assert_eq!(action.action.state(), HoldActionState::PausedOwned);
        fixture.with_engine(|engine| assert!(engine.release_waveform_hold(action).unwrap()));
    }
    fixture.callback.drain();
    assert_eq!(source.action.state(), HoldActionState::Resumed);
    assert_eq!(outside.action.state(), HoldActionState::Resumed);
    let pcm = fixture.callback.render(2048);
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| sample.abs() > 1e-4));
    pcm
}

#[test]
fn fresh_mixed_callback_receipt_preserves_source_outside_pcm_holds_and_all_incarnations() {
    isolated(
        "bank_public_tests::recovered_bank_tests::fresh_mixed_callback_receipt_preserves_source_outside_pcm_holds_and_all_incarnations",
        || {
            assert_eq!(continuity_profile(true), continuity_profile(false));
        },
    );
}

#[test]
fn actual_withheld_unload_needs_same_owner_fresh_nonce_after_callback_empty() {
    isolated(
        "bank_public_tests::recovered_bank_tests::actual_withheld_unload_needs_same_owner_fresh_nonce_after_callback_empty",
        || {
            let mut fixture = recovery_fixture(false, true);
            put(&mut fixture, 71);
            fixture.with_engine(|engine| engine.unload_sample(71).unwrap());
            // Labelled callback-delivery fault: retain the genuine already-admitted
            // ordinary Unload payload. Control Empty is real; callback still owns old PCM.
            let unload = fixture.callback.controls.pop().unwrap();
            assert!(matches!(unload, ControlMessage::UnloadSample { .. }));
            assert!(fixture.callback.mixer.bank_for_measurement()[71].is_some());
            let original = view(&fixture, 1);
            let acceptance = accept(&fixture, &original, vec![None; 36]);
            let owner = status(&acceptance)["owner"].clone();
            let before = snapshots(&fixture);
            let source = voice(&fixture, 0);
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(status(&acceptance)["status"], json!("waiting_restore"));
            assert_eq!(status(&acceptance)["accepted_nonce"], json!(0));
            assert!(
                fixture.callback.controls.is_empty(),
                "mismatch probe must retire instead of blocking the restore FIFO"
            );
            fixture
                .with_engine(|engine| assert!(engine.retry_recovered_bank(&acceptance).unwrap()));
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(status(&acceptance)["status"], json!("waiting_restore"));
            assert_eq!(status(&acceptance)["nonce"], json!(2));
            fixture.producer.lock().unwrap().push(unload).unwrap();
            assert_eq!(fixture.callback.drain(), 1);
            assert!(fixture.callback.mixer.bank_for_measurement()[71].is_none());
            fixture.with_engine(|engine| {
                assert!(
                    !engine.validate_recovered_bank(&acceptance).unwrap(),
                    "Unload ACK cannot manufacture aggregate acceptance"
                );
                assert!(engine.retry_recovered_bank(&acceptance).unwrap());
            });
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(status(&acceptance)["owner"], owner);
            assert_eq!(status(&acceptance)["nonce"], json!(3));
            assert_eq!(status(&acceptance)["accepted_nonce"], json!(3));
            fixture.with_engine(|engine| {
                assert!(engine.validate_recovered_bank(&acceptance).unwrap())
            });
            unchanged(&fixture, &before);
            assert_eq!(voice(&fixture, 0), source);
            release(&fixture, &acceptance);
        },
    );
}

#[test]
fn all36_occupied_and_all_empty_images_each_get_one_read_only_receipt() {
    isolated(
        "bank_public_tests::recovered_bank_tests::all36_occupied_and_all_empty_images_each_get_one_read_only_receipt",
        || {
            let mut fixture = recovery_fixture(false, true);
            // Restore the actual 36 target assignments through their own Cold
            // publications; a preceding Bank Copy ACK is not a restore ACK.
            for id in 36..72 {
                put(&mut fixture, id);
            }
            let before = snapshots(&fixture);
            let original = view(&fixture, 1);
            let acceptance = accept(&fixture, &original, images(&fixture, 1));
            assert_eq!(fixture.callback.controls.slots(), 1);
            assert_eq!(fixture.callback.drain(), 1);
            fixture.with_engine(|engine| {
                assert!(engine.validate_recovered_bank(&acceptance).unwrap())
            });
            assert!(
                !fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| (36..72).contains(&voice.sample_id)
                        && (voice.active || voice.paused))
            );
            unchanged(&fixture, &before);
            release(&fixture, &acceptance);
            let cleared = capture(&fixture, None, 1);
            let ticket = prepare(&fixture, &cleared);
            fixture.with_engine(|engine| engine.publish_bank_mutation(&ticket).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            finish(&fixture, &ticket);
            let before = snapshots(&fixture);
            let original = view(&fixture, 1);
            let acceptance = accept(&fixture, &original, vec![None; 36]);
            assert_eq!(fixture.callback.drain(), 1);
            fixture.with_engine(|engine| {
                assert!(engine.validate_recovered_bank(&acceptance).unwrap())
            });
            unchanged(&fixture, &before);
            release(&fixture, &acceptance);
        },
    );
}

#[test]
fn stopped_gain_target_accepts_legitimate_retained_smoother_history() {
    isolated(
        "bank_public_tests::recovered_bank_tests::stopped_gain_target_accepts_legitimate_retained_smoother_history",
        || {
            let mut fixture = recovery_fixture(false, true);
            put(&mut fixture, 36);
            fixture.start(36);
            fixture.with_engine(|engine| engine.set_pad_gain(36, -9.0).unwrap());
            fixture.callback.drain();
            fixture.with_engine(|engine| engine.stop_sample(36).unwrap());
            fixture.callback.drain();
            let gain = fixture.callback.mixer.recovered_gain_state_for_test(36);
            assert_ne!(
                gain.0.to_bits(),
                gain.1.to_bits(),
                "real preceding live edit retains its current ramp"
            );
            let mut fixed = images(&fixture, 1);
            let mut image: Value = serde_json::from_str(fixed[0].as_ref().unwrap()).unwrap();
            image["snapshot"]["gain_db"] = json!(-9.0);
            fixed[0] = Some(image.to_string());
            let original = view(&fixture, 1);
            let acceptance = accept(&fixture, &original, fixed);
            assert_eq!(fixture.callback.drain(), 1);
            fixture.with_engine(|engine| {
                assert!(engine.validate_recovered_bank(&acceptance).unwrap())
            });
            assert_eq!(
                fixture.callback.mixer.recovered_gain_state_for_test(36),
                gain,
                "verification cannot repair or reset history"
            );
            release(&fixture, &acceptance);
        },
    );
}

#[test]
fn original_final_occupied_and_empty_aba_foreign_and_source_bank_views_reject_without_reservation()
{
    isolated(
        "bank_public_tests::recovered_bank_tests::original_final_occupied_and_empty_aba_foreign_and_source_bank_views_reject_without_reservation",
        || {
            let mut fixture = recovery_fixture(false, true);
            let foreign = recovery_fixture(false, true);
            let empty = view(&fixture, 1);
            put(&mut fixture, 71);
            fixture.with_engine(|engine| engine.unload_sample(71).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            fixture.with_engine(|engine| {
                assert!(
                    engine
                        .capture_recovered_bank(&empty, vec![None; 36])
                        .is_err()
                )
            });
            put(&mut fixture, 71);
            let occupied = view(&fixture, 1);
            let (sample, generation, before) = fixture.with_engine(|engine| {
                (
                    engine.sample_cache.lock().unwrap()[71]
                        .as_ref()
                        .unwrap()
                        .clone(),
                    engine.loaded_source_generations.lock().unwrap()[71].0,
                    engine.input_runtime_ownership.slot_snapshot(71).unwrap(),
                )
            });
            fixture.with_engine(|engine| {
                let _requests = engine.pad_request_ids.lock().unwrap();
                let mut generations = engine.loaded_source_generations.lock().unwrap();
                let mut digests = engine.loaded_source_digests.lock().unwrap();
                let digest = digests[71].clone().unwrap();
                publish_loaded_sample_with_arm(
                    &fixture.producer,
                    &engine.sample_cache,
                    71,
                    sample.clone(),
                    LoadedSourcePublication {
                        initial_mix: None,
                        ownership: &engine.input_runtime_ownership,
                        placement: before,
                        generation,
                        rate: RATE,
                        generation_slot: &mut generations[71],
                        digest_slot: &mut digests[71],
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
            assert_eq!(fixture.callback.drain(), 1);
            assert!(Arc::ptr_eq(
                &sample.samples,
                &fixture.callback.mixer.bank_for_measurement()[71]
                    .as_ref()
                    .unwrap()
                    .samples
            ));
            let foreign_view = view(&foreign, 1);
            let source_view =
                fixture.with_engine(|engine| engine.capture_bank_participants(Some(0), 1).unwrap());
            let before = snapshots(&fixture);
            let requests =
                fixture.with_engine(|engine| engine.pad_request_ids.lock().unwrap().clone());
            for original in [&occupied, &foreign_view, &source_view] {
                fixture.with_engine(|engine| {
                    assert!(
                        engine
                            .capture_recovered_bank(original, images(&fixture, 1))
                            .is_err()
                    )
                });
                all_unreserved(&fixture);
                unchanged(&fixture, &before);
                assert!(fixture.callback.controls.is_empty());
            }
            fixture.with_engine(|engine| {
                assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests)
            });
        },
    );
}

#[test]
fn final_guard_stale_after_capture_rejects_receipt_and_release_keeps_foreign_reservations() {
    isolated(
        "bank_public_tests::recovered_bank_tests::final_guard_stale_after_capture_rejects_receipt_and_release_keeps_foreign_reservations",
        || {
            let mut fixture = recovery_fixture(false, true);
            put(&mut fixture, 71);
            let original = view(&fixture, 1);
            let acceptance = accept(&fixture, &original, images(&fixture, 1));
            let before = snapshots(&fixture);
            // Labelled final-participant fault. Ordinary new edits are already fenced;
            // this emulates an earlier admitted epoch writer winning before callback.
            fixture.with_engine(|engine| {
                engine
                    .prepared_source_epoch(71)
                    .unwrap()
                    .fetch_add(1, Ordering::AcqRel);
            });
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(status(&acceptance)["status"], json!("stale"));
            fixture.with_engine(|engine| {
                assert!(!engine.validate_recovered_bank(&acceptance).unwrap());
                assert!(!engine.retry_recovered_bank(&acceptance).unwrap());
            });
            unchanged(&fixture, &before);
            release(&fixture, &acceptance);
            let unrelated =
                fixture.with_engine(|engine| engine.capture_bank_participants(None, 2).unwrap());
            let other = accept(&fixture, &unrelated, vec![None; 36]);
            assert_eq!(fixture.callback.drain(), 1);
            fixture.with_engine(|engine| {
                assert!(engine.release_recovered_bank(&acceptance).unwrap());
                for id in 72..108 {
                    assert_ne!(engine.input_runtime_ownership.copy_reservation(id), 0);
                }
            });
            release(&fixture, &other);
        },
    );
}

#[test]
fn queued_probe_retirement_is_terminal_without_ack_and_drops_independent_custody_off_rt() {
    isolated(
        "bank_public_tests::recovered_bank_tests::queued_probe_retirement_is_terminal_without_ack_and_drops_independent_custody_off_rt",
        || {
            let mut fixture = recovery_fixture(false, true);
            put(&mut fixture, 36);
            let lease = fixture.with_engine(|engine| {
                engine.cold_leases.lock().unwrap()[36]
                    .as_ref()
                    .unwrap()
                    .clone()
            });
            let count = lease.cache_assignment_count();
            let original = view(&fixture, 1);
            let acceptance = accept(&fixture, &original, images(&fixture, 1));
            assert!(
                lease.cache_assignment_count() > count,
                "fresh recovery owns an independent source subscriber"
            );
            let before = snapshots(&fixture);
            let ControlMessage::VerifyRecoveredBank(probe) =
                fixture.callback.controls.pop().unwrap()
            else {
                panic!("actual queued probe");
            };
            fixture
                .callback
                .retirement
                .retire_recovered_bank_probe(probe);
            wait_for(
                || status(&acceptance)["status"] == json!("stale"),
                "actual worker retires queued probe",
            );
            fixture.with_engine(|engine| {
                assert!(!engine.validate_recovered_bank(&acceptance).unwrap())
            });
            release(&fixture, &acceptance);
            drop(acceptance);
            wait_for(
                || lease.cache_assignment_count() == count,
                "independent recovery source subscriber retires",
            );
            unchanged(&fixture, &before);
        },
    );
}

#[test]
fn full_command_queue_fails_before_any_recovery_reservation_or_custody_leak() {
    isolated(
        "bank_public_tests::recovered_bank_tests::full_command_queue_fails_before_any_recovery_reservation_or_custody_leak",
        || {
            let mut fixture = recovery_fixture(false, true);
            put(&mut fixture, 71);
            let original = view(&fixture, 1);
            let before = snapshots(&fixture);
            let lease = fixture.with_engine(|engine| {
                engine.cold_leases.lock().unwrap()[71]
                    .as_ref()
                    .unwrap()
                    .clone()
            });
            let count = lease.cache_assignment_count();
            while !fixture.producer.lock().unwrap().is_full() {
                fixture
                    .producer
                    .lock()
                    .unwrap()
                    .push(ControlMessage::Ping())
                    .unwrap();
            }
            let queued = fixture.callback.controls.slots();
            fixture.with_engine(|engine| {
                let error = engine
                    .capture_recovered_bank(&original, images(&fixture, 1))
                    .err()
                    .expect("full callback queue rejects before reservation");
                assert!(
                    error.to_string().contains("command queue is full"),
                    "{error}"
                );
            });
            assert_eq!(fixture.callback.controls.slots(), queued);
            assert_eq!(lease.cache_assignment_count(), count);
            unchanged(&fixture, &before);
            all_unreserved(&fixture);
        },
    );
}
