//! Public Copy owns each fresh subscriber through rejection or actual target retirement.
//! Register as a child of public_complete_tests to reuse its acknowledged fixture.
use super::*;
use crate::audio_engine::cold_store::CommittedColdLease;

fn source_lease(fixture: &Fixture) -> CommittedColdLease {
    fixture.with_engine(|engine| {
        engine.cold_leases.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .clone()
    })
}

fn assert_source_still_owned(fixture: &Fixture, lease: &CommittedColdLease) {
    assert!(!lease.assignment_retired());
    lease.verify_original_for_offline().unwrap();
    assert!(lease.cache_path.join("playback.f32le").exists());
    fixture.with_engine(|engine| {
        assert!(
            engine
                .current_input_runtime_pad_binding(0)
                .unwrap()
                .unwrap()
                .is_current()
        );
        assert_eq!(
            engine.cold_leases.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .assignment_id(),
            lease.assignment_id()
        );
    });
    assert!(fixture.source_voice().is_playing_sample(0));
}

#[test]
fn prepared_public_copy_cancel_releases_only_its_fresh_subscriber() {
    isolated(
        "assignment_custody_regressions::prepared_public_copy_cancel_releases_only_its_fresh_subscriber",
        || {
            let fixture = Fixture::new(false, false, "tap");
            let lease = source_lease(&fixture);
            let baseline = lease.cache_assignment_count();
            let source_generation = fixture.source_voice().generation;
            let source_position = fixture.source_voice().source_playback.position();
            let captured = fixture.capture(36);
            let ticket = fixture.prepare(&captured);
            assert_eq!(lease.cache_assignment_count(), baseline + 1);
            assert_ne!(ticket.publication_status(), "accepted");
            fixture.with_engine(|engine| {
                assert!(engine.sample_cache.lock().unwrap()[36].is_none());
                assert!(engine.cancel_pad_copy(&captured).unwrap());
                assert_eq!(engine.input_runtime_ownership.copy_reservation(36), 0);
            });
            assert_eq!(ticket.publication_status(), "rejected");
            drop(ticket);
            drop(captured);
            assert_eq!(
                lease.cache_assignment_count(),
                baseline,
                "a cancelled unpublished Copy cannot leave an unreachable durable subscriber"
            );
            assert_source_still_owned(&fixture, &lease);
            assert_eq!(fixture.source_voice().generation, source_generation);
            assert_eq!(
                fixture.source_voice().source_playback.position(),
                source_position
            );
        },
    );
}

#[test]
fn actual_postsubscribe_copy_gain_error_releases_the_failed_subscriber() {
    isolated(
        "assignment_custody_regressions::actual_postsubscribe_copy_gain_error_releases_the_failed_subscriber",
        || {
            let fixture = Fixture::new(false, false, "tap");
            let lease = source_lease(&fixture);
            let baseline = lease.cache_assignment_count();
            let mut envelope: Value = serde_json::from_str(&fixture.envelope()).unwrap();
            envelope["snapshot"]["gain_db"] =
                json!(f64::from(crate::audio_engine::constants::PAD_GAIN_DB_MAX) + 1.0);
            let captured = fixture.with_engine(|engine| {
                let binding = engine
                    .current_input_runtime_pad_binding(0)
                    .unwrap()
                    .unwrap();
                engine
                    .capture_pad_copy(&binding, 36, &envelope.to_string(), None)
                    .unwrap()
            });
            assert_eq!(lease.cache_assignment_count(), baseline);
            // The ordinary public preparation really subscribes and reads before
            // its scalar conversion rejects this out-of-range gain. No hook or
            // synthetic phase/owner mutation manufactures the failure.
            let error = Python::attach(|py| {
                AudioEngine::prepare_pad_copy(fixture.engine.clone_ref(py), py, &captured)
                    .err()
                    .expect("actual worker scalar validation must reject the gain")
            });
            assert!(error.to_string().contains("gain_db"), "{error}");
            assert_eq!(captured.publication_status(), "rejected");
            fixture.with_engine(|engine| {
                assert!(engine.cancel_pad_copy(&captured).unwrap());
                assert_eq!(engine.input_runtime_ownership.copy_reservation(36), 0);
                assert!(engine.sample_cache.lock().unwrap()[36].is_none());
                assert!(engine.cold_leases.lock().unwrap()[36].is_none());
            });
            drop(captured);
            assert_eq!(
                lease.cache_assignment_count(),
                baseline,
                "a post-subscribe worker failure must release its independent ID"
            );
            assert_source_still_owned(&fixture, &lease);
        },
    );
}

#[test]
fn claimed_public_copy_refuses_cancel_and_retains_exact_subscriber_custody() {
    isolated(
        "assignment_custody_regressions::claimed_public_copy_refuses_cancel_and_retains_exact_subscriber_custody",
        || {
            let mut fixture = Fixture::new(false, false, "tap");
            let lease = source_lease(&fixture);
            let baseline = lease.cache_assignment_count();
            let captured = fixture.capture(36);
            let ticket = fixture.prepare(&captured);
            fixture.with_engine(|engine| engine.publish_pad_copy(&ticket).unwrap());
            let held_claim = match fixture.callback.controls.pop().unwrap() {
                ControlMessage::PadCopy(work) => {
                    // Claim the actual eligible public payload while withholding
                    // installation and ACK, exactly as the existing negative gate.
                    assert!(work.guard.claim());
                    work
                }
                other => panic!("expected actual prepared Copy payload: {other:?}"),
            };
            assert_eq!(ticket.publication_status(), "adopting");
            assert_eq!(held_claim.publication.status(), "pending");
            assert_eq!(lease.cache_assignment_count(), baseline + 1);
            fixture.with_engine(|engine| {
                assert!(!engine.cancel_pad_copy(&captured).unwrap());
                assert!(!engine.reconcile_pad_copy(&ticket).unwrap());
                assert!(engine.finish_pad_copy(&ticket).is_err());
                assert_ne!(engine.input_runtime_ownership.copy_reservation(36), 0);
                assert!(engine.sample_cache.lock().unwrap()[36].is_none());
            });
            drop(ticket);
            drop(captured);
            // Dropping client handles cannot classify an actual claimed attempt
            // as rejected or revoke the engine's retained subscriber custody.
            assert_eq!(held_claim.guard.phase.load(Ordering::Acquire), 1);
            assert_eq!(lease.cache_assignment_count(), baseline + 1);
            assert_source_still_owned(&fixture, &lease);
            let assets = fixture.with_engine(|engine| engine.project_assets.clone());
            fixture.with_engine(|engine| engine.shut_down().unwrap());
            assets.collect_for_test();
            // Shutdown rotates the engine's ordinary pending-delivery epoch.
            // This still-owned claimed transaction must keep its actual epoch
            // alive; that rotation cannot turn phase 1 into an orphan rejection.
            assert_eq!(held_claim.guard.phase.load(Ordering::Acquire), 1);
            assert_eq!(lease.cache_assignment_count(), baseline + 1);
            assert!(lease.original_path.exists());
            assert!(lease.cache_path.join("playback.f32le").exists());
            drop(held_claim);
        },
    );
}

#[test]
fn accepted_public_copy_keeps_its_subscriber_until_actual_target_retirement() {
    isolated(
        "assignment_custody_regressions::accepted_public_copy_keeps_its_subscriber_until_actual_target_retirement",
        || {
            let mut fixture = Fixture::new(false, false, "tap");
            let lease = source_lease(&fixture);
            let assets = fixture.with_engine(|engine| engine.project_assets.clone());
            let samples_root = lease.samples_root().to_owned();
            let original = lease.original_path.clone();
            let cache = lease.cache_path.clone();
            let external = fixture.directory.path().join("samples/copy-source.wav");
            let external_bytes = fs::read(&external).unwrap();
            let mut saved_owner = assets.acquire(&samples_root, &original).unwrap();
            let baseline = lease.cache_assignment_count();
            let captured = fixture.capture(36);
            let ticket = fixture.prepare(&captured);
            assert_eq!(lease.cache_assignment_count(), baseline + 1);
            fixture.with_engine(|engine| engine.publish_pad_copy(&ticket).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(ticket.publication_status(), "accepted");
            let target_lease = fixture.with_engine(|engine| {
                assert!(engine.reconcile_pad_copy(&ticket).unwrap());
                engine.finish_pad_copy(&ticket).unwrap();
                assert_eq!(engine.input_runtime_ownership.copy_reservation(36), 0);
                engine.cold_leases.lock().unwrap()[36]
                    .as_ref()
                    .unwrap()
                    .clone()
            });
            assert_ne!(target_lease.assignment_id(), lease.assignment_id());
            drop(ticket);
            drop(captured);
            // Success transfers this new ID to the real target. Finishing the
            // transaction and dropping client handles must not roll it back.
            assert!(!target_lease.assignment_retired());
            assert_eq!(lease.cache_assignment_count(), baseline + 1);
            assert!(fixture.callback.mixer.bank_for_measurement()[36].is_some());
            fixture.with_engine(|engine| {
                assert!(
                    engine
                        .current_input_runtime_pad_binding(36)
                        .unwrap()
                        .unwrap()
                        .is_current()
                );
                engine.unload_sample(36).unwrap();
            });
            assert_eq!(fixture.callback.drain(), 1);
            assert!(fixture.callback.mixer.bank_for_measurement()[36].is_none());
            fixture.with_engine(|engine| {
                assert!(engine.sample_cache.lock().unwrap()[36].is_none());
                assert!(engine.cold_leases.lock().unwrap()[36].is_none());
            });
            // Native C unload is deliberately not the final-user oracle. A still
            // owns the shared PCM and the genuine saved PathOwner keeps its file
            // relation durable; the normal registry may retain/coalesce saved IDs.
            assets.collect_for_test();
            assert!(original.exists());
            assert!(cache.join("playback.f32le").exists());
            assert_source_still_owned(&fixture, &lease);
            fixture.with_engine(|engine| engine.unload_sample(0).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            assert!(fixture.callback.mixer.bank_for_measurement()[0].is_none());
            assert!(
                !fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(0) || voice.is_playing_sample(36))
            );
            // Release the actual retired callback aggregates, control cache,
            // fixture preparation pins and all real source PCM before last-user
            // cleanup. Keep the TempDir and file-only lease probes for assertions.
            let Fixture {
                engine,
                callback,
                producer,
                material,
                pair,
                directory,
                ..
            } = fixture;
            Python::attach(|_| drop(engine));
            drop(callback);
            drop(producer);
            drop(pair);
            drop(material);
            assets.collect_for_test();
            assert!(original.exists());
            assert!(cache.join("playback.f32le").exists());
            saved_owner.release();
            assets.retire(&samples_root, &original, false).unwrap();
            wait_for_with_diagnostics(
                || {
                    assets.collect_for_test();
                    target_lease.assignment_retired()
                        && lease.assignment_retired()
                        && lease.cache_assignment_count() == 0
                },
                "actual final-reader and saved-owner retirement left a Copy subscriber",
                || {
                    format!(
                        "initial_baseline={baseline}, actual={}, target_retired={}, source_retired={}",
                        lease.cache_assignment_count(),
                        target_lease.assignment_retired(),
                        lease.assignment_retired()
                    )
                },
            );
            drop(target_lease);
            drop(lease);
            wait_for(
                || {
                    assets.collect_for_test();
                    !original.exists() && !cache.exists()
                },
                "final contained Copy original/cache retirement did not finish",
            );
            assert_eq!(fs::read(&external).unwrap(), external_bytes);
            drop(directory);
        },
    );
}

#[test]
fn accepted_finite_public_copy_materializes_its_own_complete_timing_input() {
    isolated(
        "assignment_custody_regressions::accepted_finite_public_copy_materializes_its_own_complete_timing_input",
        || {
            use crate::audio_engine::native_owned_work::{self, WorkPoint};
            use std::sync::atomic::AtomicUsize;

            let mut fixture = Fixture::new(false, false, "tap");
            let window = fixture.with_engine(|engine| {
                engine
                    .prepare_resident_control(
                        0,
                        Some(START as f64 / f64::from(RATE)),
                        Some(END as f64 / f64::from(RATE)),
                        None,
                        Some(false),
                    )
                    .unwrap()
            });
            wait_for_with_diagnostics(
                || {
                    fixture.callback.drain();
                    window.publication_status() == "accepted"
                },
                "source A finite resident control did not obtain its actual callback ACK",
                || format!("{}: {:?}", window.publication_status(), window.error()),
            );
            fixture.with_engine(|engine| {
                assert!(
                    engine
                        .current_input_runtime_pad_binding(0)
                        .unwrap()
                        .unwrap()
                        .is_current()
                );
                let cache = engine.sample_cache.lock().unwrap();
                let sample = cache[0].as_ref().unwrap();
                assert_eq!(
                    (sample.resident_start(), sample.resident_end()),
                    (START, END)
                );
                assert!(sample.resident_end() < sample.frame_count());
            });
            let materialize = |slot: usize| {
                let observed = Arc::new(AtomicUsize::new(0));
                let worker_observed = observed.clone();
                native_owned_work::set_observer(Some(Box::new(move |point| {
                    if point == WorkPoint::TimingPcm {
                        worker_observed.fetch_add(1, Ordering::AcqRel);
                    }
                })));
                let result = Python::attach(|py| {
                    fixture.engine.call_method1(
                        py,
                        "prepare_constant_timing",
                        (slot, 0.05, "actual finite copied-input custody regression"),
                    )
                });
                native_owned_work::set_observer(None);
                (observed.load(Ordering::Acquire), result)
            };
            // Public timing preparation requires declared Automatic intent. It
            // does not require or manufacture previously accepted QM evidence.
            fixture.with_engine(|engine| engine.set_pad_timing_intent(0, "automatic").unwrap());
            fixture.callback.drain();
            let (source_observed, source_result) = materialize(0);
            assert_eq!(
                source_observed,
                1,
                "real source A control did not materialize complete timing PCM: {:?}",
                source_result.as_ref().err()
            );
            drop(source_result);
            // Restore the fixture's genuine available TAP source before Copy;
            // no analysis result is published as accepted timing for either pad.
            fixture.with_engine(|engine| engine.set_pad_timing_intent(0, "tap").unwrap());
            fixture.callback.drain();
            let captured = fixture.capture(36);
            let ticket = fixture.prepare(&captured);
            fixture.with_engine(|engine| engine.publish_pad_copy(&ticket).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(ticket.publication_status(), "accepted");
            fixture.with_engine(|engine| {
                assert!(engine.reconcile_pad_copy(&ticket).unwrap());
                engine.finish_pad_copy(&ticket).unwrap();
                let cache = engine.sample_cache.lock().unwrap();
                let copied = cache[36].as_ref().unwrap();
                assert_eq!(
                    (copied.resident_start(), copied.resident_end()),
                    (START, END)
                );
                assert!(copied.resident_end() < copied.frame_count());
                drop(cache);
                let leases = engine.cold_leases.lock().unwrap();
                assert_ne!(
                    leases[0].as_ref().unwrap().assignment_id(),
                    leases[36].as_ref().unwrap().assignment_id()
                );
            });
            fixture.with_engine(|engine| engine.set_pad_timing_intent(36, "automatic").unwrap());
            fixture.callback.drain();
            let (copied_observed, copied_result) = materialize(36);
            // This point is emitted only after the real complete-read plus exact
            // assignment reader registration succeed. The short fixture's later
            // QM outcome is separate and grants no musical/timing acceptance.
            assert_eq!(
                copied_observed,
                1,
                "accepted finite Copy cannot materialize timing PCM under its own ID: {:?}",
                copied_result.as_ref().err()
            );
            drop(copied_result);
            assert_source_still_owned(&fixture, &source_lease(&fixture));
        },
    );
}
