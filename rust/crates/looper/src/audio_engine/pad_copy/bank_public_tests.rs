//! Real public bank admission, complete callback adoption and exact old Hold lifetimes.
use super::*;
use crate::audio_engine::hold_action::{HoldActionState, HoldActionTicket};
use crate::audio_engine::pad_copy::bank::{BankMutationTicket, CapturedBankMutation};
use crate::audio_engine::source_playback::FractionalSourcePosition;
use pyo3::types::PyInt;

#[path = "bank_public_tests/participant_tests.rs"]
mod participant_tests;

#[path = "bank_public_tests/controller_native_tests.rs"]
mod controller_native_tests;

#[path = "bank_public_tests/closure_cases.rs"]
mod closure_cases;

#[path = "control/bank/recovery/tests.rs"]
mod recovered_bank_tests;

struct BankFixture {
    fixture: Fixture,
    // Fixture (including sealed readers) drops before its independently owned parent.
    _owned_root: tempfile::TempDir,
}

impl std::ops::Deref for BankFixture {
    type Target = Fixture;
    fn deref(&self) -> &Self::Target {
        &self.fixture
    }
}

impl std::ops::DerefMut for BankFixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.fixture
    }
}

fn bank_fixture(key_lock: bool, timing: &'static str) -> BankFixture {
    bank_fixture_with_stems(false, key_lock, timing)
}

fn bank_fixture_with_stems(stems: bool, key_lock: bool, timing: &'static str) -> BankFixture {
    use std::io::Write;
    use std::os::windows::fs::MetadataExt;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let parent = workspace.join("test-artifacts/bank36-native-20261020");
    for ancestor in parent.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => assert_eq!(
                metadata.file_attributes() & 0x400,
                0,
                "owned Bank fixture must not traverse a reparse point"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("owned Bank fixture ancestor is unavailable: {error}"),
        }
    }
    fs::create_dir_all(&parent).unwrap();
    let owned_root = tempfile::Builder::new()
        .prefix("bank-participants-")
        .tempdir_in(parent)
        .unwrap();
    fs::OpenOptions::new().write(true).create_new(true)
        .open(owned_root.path().join("OWNER.md")).unwrap()
        .write_all(b"Owned generated Bank36 Native fixture. No private samples; released after all fixture owners.")
        .unwrap();
    let fixture = Fixture::with_pair_registration_at(
        stems,
        true,
        key_lock,
        timing,
        Some(owned_root.path().to_path_buf()),
    );
    BankFixture {
        fixture,
        _owned_root: owned_root,
    }
}

fn put(fixture: &mut Fixture, target: usize) {
    // Existing single Copy is fixture setup only. The exercised bank operation emits ONE message.
    let captured = fixture.capture(target);
    let ticket = fixture.prepare(&captured);
    fixture.with_engine(|engine| engine.publish_pad_copy(&ticket).unwrap());
    assert_eq!(fixture.callback.drain(), 1);
    fixture.with_engine(|engine| {
        assert!(engine.reconcile_pad_copy(&ticket).unwrap());
        engine.finish_pad_copy(&ticket).unwrap();
    });
}

fn hold(fixture: &Fixture, id: usize, press: u64) -> HoldActionTicket {
    fixture.with_engine(|engine| {
        Python::attach(|py| {
            engine
                .pause_waveform_hold(id, PyInt::new(py, press).as_any(), None)
                .unwrap()
        })
    })
}

fn capture(fixture: &Fixture, source: Option<usize>, target: usize) -> CapturedBankMutation {
    let intents = (0..36)
        .map(|index| {
            source.and_then(|source| {
                fixture.callback.mixer.bank_for_measurement()[source * 36 + index]
                    .as_ref()
                    .map(|_| fixture.envelope())
            })
        })
        .collect();
    fixture.with_engine(|engine| {
        engine
            .capture_bank_mutation(source, target, intents)
            .unwrap()
    })
}

fn prepare(fixture: &Fixture, captured: &CapturedBankMutation) -> BankMutationTicket {
    Python::attach(|py| {
        AudioEngine::prepare_bank_mutation(fixture.engine.clone_ref(py), py, captured).unwrap()
    })
}

fn finish(fixture: &Fixture, ticket: &BankMutationTicket) {
    let ack = ticket.acknowledgement().expect("own complete parent ACK");
    fixture.with_engine(|engine| {
        assert!(engine.validate_bank_mutation(ticket, &ack).unwrap());
        assert!(engine.reconcile_bank_mutation(ticket, &ack).unwrap());
        engine.finish_bank_mutation(ticket, &ack).unwrap();
    });
}

fn voice(
    fixture: &Fixture,
    id: usize,
) -> (u64, FractionalSourcePosition, bool, usize, (usize, usize)) {
    let voice = fixture
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(id))
        .unwrap();
    (
        voice.generation,
        voice.source_playback.position(),
        voice.paused,
        voice.stretch.native_state_address(),
        voice.stretch.pending_fifo_frames(),
    )
}

fn sparse_profile(mutate: bool) -> Vec<f32> {
    let mut fixture = bank_fixture(true, "manual");
    put(&mut fixture, 35);
    for target in 36..72 {
        put(&mut fixture, target);
    }
    put(&mut fixture, 72);
    for target in [35, 37, 71, 72] {
        fixture.start(target);
    }
    fixture.callback.render(37);
    let source_hold = hold(&fixture, 0, 101);
    let removed_hold = hold(&fixture, 37, 102);
    let outside_hold = hold(&fixture, 72, 103);
    fixture.callback.drain();
    for action in [&source_hold, &removed_hold, &outside_hold] {
        assert_eq!(action.action.state(), HoldActionState::PausedOwned);
    }
    let before = [voice(&fixture, 0), voice(&fixture, 35), voice(&fixture, 72)];
    assert_ne!(
        before[1].3, 0,
        "the continuing source owns an actual Native processor"
    );
    if mutate {
        let target_lifetimes: Vec<_> = fixture.with_engine(|engine| {
            (36..72)
                .map(|id| engine.input_runtime_ownership.content_lifetime(id))
                .collect()
        });
        let captured = capture(&fixture, Some(0), 1);
        let ticket = prepare(&fixture, &captured);
        fixture.with_engine(|engine| {
            assert!(ticket.acknowledgement().is_none());
            // Source mutation admission is fenced, while the original matching Hold remains releasable.
            assert!(
                engine
                    .input_runtime_ownership
                    .prepare_source_request_advance(35)
                    .is_err()
            );
            engine.publish_bank_mutation(&ticket).unwrap();
        });
        assert!(matches!(
            fixture.callback.controls.peek().unwrap(),
            ControlMessage::BankMutation(_)
        ));
        assert_eq!(fixture.callback.drain(), 1);
        assert_eq!(ticket.publication_status(), "accepted");
        assert_eq!(
            [voice(&fixture, 0), voice(&fixture, 35), voice(&fixture, 72)],
            before
        );
        assert_eq!(source_hold.action.state(), HoldActionState::PausedOwned);
        assert_eq!(outside_hold.action.state(), HoldActionState::PausedOwned);
        assert_eq!(removed_hold.action.state(), HoldActionState::Retired);
        for index in 0..36 {
            assert_eq!(
                fixture.callback.mixer.bank_for_measurement()[36 + index].is_some(),
                index == 0 || index == 35
            );
        }
        assert!(
            !fixture
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active && (36..72).contains(&voice.sample_id))
        );
        let receipt: Value = serde_json::from_str(&ticket.metadata_json()).unwrap();
        assert_eq!(receipt["targets"].as_array().unwrap().len(), 36);
        assert_ne!(receipt["layout_revision"].as_u64(), Some(0));
        fixture.with_engine(|engine| {
            for (index, old) in target_lifetimes.iter().enumerate() {
                let now = engine
                    .input_runtime_ownership
                    .slot_snapshot(36 + index)
                    .unwrap();
                assert_eq!(
                    now.layout_revision(),
                    receipt["layout_revision"].as_u64().unwrap()
                );
                assert_ne!(now.lifetime(), *old);
            }
        });
        finish(&fixture, &ticket);
        put(&mut fixture, 37);
        fixture.with_engine(|engine| {
            assert!(engine.release_waveform_hold(&removed_hold).unwrap());
        });
        fixture.callback.drain();
        assert!(
            !fixture
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(37))
        );
    } else {
        // Independently initialized reference retains every reader/history and stops only
        // the otherwise audible target through the existing public transport owner.
        fixture.with_engine(|engine| engine.stop_sample(71).unwrap());
        fixture.callback.drain();
        assert_eq!(
            [voice(&fixture, 0), voice(&fixture, 35), voice(&fixture, 72)],
            before
        );
    }
    fixture.with_engine(|engine| {
        assert!(engine.release_waveform_hold(&source_hold).unwrap());
        assert!(engine.release_waveform_hold(&outside_hold).unwrap());
    });
    fixture.callback.drain();
    assert_eq!(source_hold.action.state(), HoldActionState::Resumed);
    assert_eq!(outside_hold.action.state(), HoldActionState::Resumed);
    let pcm = fixture.callback.render(96);
    assert!(pcm.iter().all(|value| value.is_finite()));
    assert!(pcm.iter().any(|value| value.abs() > 1e-6));
    pcm
}

#[test]
fn sparse_bank_copy_retires_empty_source_targets_and_preserves_actual_source_holds() {
    isolated(
        "bank_public_tests::sparse_bank_copy_retires_empty_source_targets_and_preserves_actual_source_holds",
        || {
            let actual = sparse_profile(true);
            let independent = sparse_profile(false);
            assert_eq!(
                actual, independent,
                "source/outside actual Native/FIFO/DSP chronology survives Bank36 exactly"
            );
        },
    );
}

#[test]
fn all36_stopped_copies_and_clear_use_complete_parent_ack_without_voice_admission() {
    isolated(
        "bank_public_tests::all36_stopped_copies_and_clear_use_complete_parent_ack_without_voice_admission",
        || {
            let mut fixture = bank_fixture(false, "tap");
            for source in 1..36 {
                put(&mut fixture, source);
            }
            let original = voice(&fixture, 0);
            let captured = capture(&fixture, Some(0), 1);
            let ticket = prepare(&fixture, &captured);
            fixture.with_engine(|engine| engine.publish_bank_mutation(&ticket).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(ticket.publication_status(), "accepted");
            assert_eq!(voice(&fixture, 0), original);
            assert!(
                fixture.callback.mixer.bank_for_measurement()[36..72]
                    .iter()
                    .all(Option::is_some)
            );
            assert!(
                !fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.active && (36..72).contains(&voice.sample_id))
            );
            finish(&fixture, &ticket);
            fixture.start(71);
            let old_hold = hold(&fixture, 71, 104);
            let outside_hold = hold(&fixture, 0, 105);
            fixture.callback.drain();
            let outside_before = voice(&fixture, 0);
            let clear = capture(&fixture, None, 1);
            let clear_ticket = prepare(&fixture, &clear);
            fixture.with_engine(|engine| engine.publish_bank_mutation(&clear_ticket).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(old_hold.action.state(), HoldActionState::Retired);
            assert_eq!(outside_hold.action.state(), HoldActionState::PausedOwned);
            assert_eq!(voice(&fixture, 0), outside_before);
            assert!(
                fixture.callback.mixer.bank_for_measurement()[36..72]
                    .iter()
                    .all(Option::is_none)
            );
            finish(&fixture, &clear_ticket);
            put(&mut fixture, 71);
            fixture.with_engine(|engine| {
                assert!(engine.release_waveform_hold(&old_hold).unwrap());
            });
            fixture.callback.drain();
            assert!(
                !fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(71))
            );
            fixture.with_engine(|engine| {
                assert!(engine.release_waveform_hold(&outside_hold).unwrap());
            });
            fixture.callback.drain();
            assert_eq!(outside_hold.action.state(), HoldActionState::Resumed);
            assert!(
                fixture
                    .callback
                    .render(96)
                    .iter()
                    .any(|value| value.abs() > 1e-6)
            );
        },
    );
}

#[test]
fn last_participant_stale_and_queue_full_cancel_leave_every_target_unapplied() {
    isolated(
        "bank_public_tests::last_participant_stale_and_queue_full_cancel_leave_every_target_unapplied",
        || {
            let mut fixture = bank_fixture(false, "manual");
            put(&mut fixture, 35);
            let captured = capture(&fixture, Some(0), 1);
            let ticket = prepare(&fixture, &captured);
            let before: Vec<_> = fixture.with_engine(|engine| {
                (36..72)
                    .map(|id| engine.input_runtime_ownership.slot_snapshot(id).unwrap())
                    .collect()
            });
            fixture.with_engine(|engine| engine.publish_bank_mutation(&ticket).unwrap());
            // Deliberate guard fault injection at the final source participant, not synthetic adoption.
            fixture.with_engine(|engine| {
                engine
                    .prepared_source_epoch(35)
                    .unwrap()
                    .fetch_add(1, Ordering::AcqRel);
            });
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(ticket.publication_status(), "rejected");
            assert!(ticket.acknowledgement().is_none());
            fixture.with_engine(|engine| {
                for (index, snapshot) in before.iter().enumerate() {
                    assert!(
                        snapshot.same_projection(
                            engine
                                .input_runtime_ownership
                                .slot_snapshot(36 + index)
                                .unwrap()
                        )
                    );
                }
                assert!(engine.cancel_bank_mutation(&captured).unwrap());
            });
            assert!(
                fixture.callback.mixer.bank_for_measurement()[36..72]
                    .iter()
                    .all(Option::is_none)
            );
            // Use a fresh source bank capture after the deliberately stale preparation epoch.
            let clear = capture(&fixture, None, 1);
            let clear_ticket = prepare(&fixture, &clear);
            fixture.with_engine(|engine| {
                let mut producer = fixture.producer.lock().unwrap();
                while !producer.is_full() {
                    producer.push(ControlMessage::Ping()).unwrap();
                }
                drop(producer);
                assert!(engine.publish_bank_mutation(&clear_ticket).is_err());
                assert!(engine.cancel_bank_mutation(&clear).unwrap());
                for id in 36..72 {
                    assert_eq!(engine.input_runtime_ownership.copy_reservation(id), 0);
                }
            });
        },
    );
}

#[test]
fn claimed_bank_without_ack_keeps_exact_subscriber_and_all_participant_reservations() {
    isolated(
        "bank_public_tests::claimed_bank_without_ack_keeps_exact_subscriber_and_all_participant_reservations",
        || {
            let mut fixture = bank_fixture(false, "manual");
            let lease = fixture.with_engine(|engine| {
                engine.cold_leases.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .clone()
            });
            let count = lease.cache_assignment_count();
            let captured = capture(&fixture, Some(0), 1);
            let ticket = prepare(&fixture, &captured);
            assert_eq!(lease.cache_assignment_count(), count + 1);
            fixture.with_engine(|engine| engine.publish_bank_mutation(&ticket).unwrap());
            let publication = match fixture.callback.controls.pop().unwrap() {
                ControlMessage::BankMutation(publication) => publication,
                other => panic!("expected one genuine prepared Bank payload: {other:?}"),
            };
            let held = publication.take().unwrap();
            // Fault boundary: retain the actual eligible payload after its irreversible CAS,
            // deliberately withholding installation/ACK. This is a custody negative, not adoption proof.
            assert!(held.guard.claim());
            assert_eq!(ticket.publication_status(), "adopting");
            assert!(ticket.acknowledgement().is_none());
            fixture.with_engine(|engine| {
                assert!(!engine.cancel_bank_mutation(&captured).unwrap());
                for id in 0..72 {
                    assert_eq!(
                        engine.input_runtime_ownership.copy_reservation(id),
                        held.guard.owner
                    );
                }
            });
            drop(ticket);
            drop(captured);
            assert_eq!(lease.cache_assignment_count(), count + 1);
            assert!(!lease.assignment_retired());
            assert_eq!(held.guard.phase.load(Ordering::Acquire), 1);
            assert!(
                fixture.callback.mixer.bank_for_measurement()[36..72]
                    .iter()
                    .all(Option::is_none)
            );
            drop(held);
            drop(publication);
        },
    );
}

#[test]
fn actual_accepted_bank_retains_custody_until_own_ack_is_observed_and_mirrored() {
    isolated(
        "bank_public_tests::actual_accepted_bank_retains_custody_until_own_ack_is_observed_and_mirrored",
        || {
            let mut fixture = bank_fixture(false, "manual");
            let lease = fixture.with_engine(|engine| {
                engine.cold_leases.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .clone()
            });
            let count = lease.cache_assignment_count();
            let source_hold = hold(&fixture, 0, 201);
            fixture.callback.drain();
            assert_eq!(source_hold.action.state(), HoldActionState::PausedOwned);
            let source_before = voice(&fixture, 0);
            let captured = capture(&fixture, Some(0), 1);
            let ticket = prepare(&fixture, &captured);
            fixture.with_engine(|engine| engine.publish_bank_mutation(&ticket).unwrap());
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(ticket.publication_status(), "accepted");
            assert_eq!(voice(&fixture, 0), source_before);
            assert!(fixture.callback.mixer.bank_for_measurement()[36].is_some());
            // The callback genuinely installed the whole image; the client has deliberately
            // not obtained its opaque ACK or mirrored any Native control owner yet.
            fixture.with_engine(|engine| {
                assert!(engine.sample_cache.lock().unwrap()[36].is_none());
                assert!(engine.cold_leases.lock().unwrap()[36].is_none());
                assert!(!engine.cancel_bank_mutation(&captured).unwrap());
                let owner = engine.input_runtime_ownership.copy_reservation(0);
                assert_ne!(owner, 0);
                for id in 0..72 {
                    assert_eq!(engine.input_runtime_ownership.copy_reservation(id), owner);
                }
                assert!(
                    engine
                        .input_runtime_ownership
                        .prepare_source_request_advance(36)
                        .is_err()
                );
            });
            assert_eq!(lease.cache_assignment_count(), count + 1);
            lease.verify_original_for_offline().unwrap();
            fixture.with_engine(|engine| {
                assert!(engine.release_waveform_hold(&source_hold).unwrap());
            });
            fixture.callback.drain();
            assert_eq!(source_hold.action.state(), HoldActionState::Resumed);
            assert!(
                !fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(36))
            );
            finish(&fixture, &ticket);
            let copied = fixture.with_engine(|engine| {
                for id in 0..72 {
                    assert_eq!(engine.input_runtime_ownership.copy_reservation(id), 0);
                }
                engine.cold_leases.lock().unwrap()[36]
                    .as_ref()
                    .unwrap()
                    .clone()
            });
            assert_ne!(copied.assignment_id(), lease.assignment_id());
            drop(ticket);
            drop(captured);
            assert!(!copied.assignment_retired());
            assert_eq!(lease.cache_assignment_count(), count + 1);
        },
    );
}

#[test]
fn admitted_bank_worker_cancellation_keeps_all_reservations_until_worker_return() {
    isolated(
        "bank_public_tests::admitted_bank_worker_cancellation_keeps_all_reservations_until_worker_return",
        || {
            let fixture = bank_fixture(false, "manual");
            let captured = capture(&fixture, Some(0), 1);
            let admitted = fixture.with_engine(|engine| {
                crate::audio_engine::pad_copy::bank::prepare(engine, &captured).unwrap()
            });
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let worker_barrier = barrier.clone();
            let worker = std::thread::spawn(move || {
                worker_barrier.wait();
                assert!(crate::audio_engine::pad_copy::bank::prepare_detached(admitted).is_err());
            });
            let (settled_early, retained, request_fenced) = fixture.with_engine(|engine| {
                let owner = engine.input_runtime_ownership.copy_reservation(0);
                assert_ne!(owner, 0);
                let settled = engine.cancel_bank_mutation(&captured).unwrap();
                (
                    settled,
                    (0..72).all(|id| engine.input_runtime_ownership.copy_reservation(id) == owner),
                    engine
                        .input_runtime_ownership
                        .prepare_source_request_advance(0)
                        .is_err(),
                )
            });
            let preparing_status = captured.publication_status();
            // Release and join even for the historical failing result; no stranded test thread.
            barrier.wait();
            worker.join().unwrap();
            assert!(
                !settled_early,
                "admitted worker still owns the bank attempt"
            );
            assert!(retained);
            assert!(request_fenced);
            assert_eq!(preparing_status, "cancelling");
            assert_eq!(captured.publication_status(), "rejected");
            fixture.with_engine(|engine| {
                assert!(engine.cancel_bank_mutation(&captured).unwrap());
                for id in 0..72 {
                    assert_eq!(engine.input_runtime_ownership.copy_reservation(id), 0);
                }
            });
            assert!(
                fixture.callback.mixer.bank_for_measurement()[36..72]
                    .iter()
                    .all(Option::is_none)
            );
            let unstarted = capture(&fixture, Some(0), 1);
            fixture.with_engine(|engine| {
                assert!(engine.cancel_bank_mutation(&unstarted).unwrap());
                assert!(crate::audio_engine::pad_copy::bank::prepare(engine, &unstarted).is_err());
                for id in 0..72 {
                    assert_eq!(engine.input_runtime_ownership.copy_reservation(id), 0);
                }
            });
        },
    );
}
