//! Genuine copied assignments through seek capture and Pair ACK/mirror collection.

use super::*;
use crate::audio_engine::cold_load::admit_selected_material;
use crate::audio_engine::cold_residency::ResidentLoadHint;

fn fixture(key_lock: bool) -> Fixture {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let parent = workspace.join("test-artifacts/copied-seek-pair-20261021/native");
    fs::create_dir_all(&parent).unwrap();
    fs::write(
        parent.join("OWNER.md"),
        b"Owned synthetic Copy/seek/Pair/loop regression. No device, Human audio or installed Native. Root owns execution, review and cleanup.\n",
    )
    .unwrap();
    let mut fixture =
        Fixture::with_pair_registration_at(false, true, key_lock, "tap", Some(parent));
    let ticket = launch_tests::request_window(&fixture, 0, START, END, key_lock);
    launch_tests::wait_window(&mut fixture, &ticket);
    drop(ticket);
    copy_to(&mut fixture, 1);
    copy_to(&mut fixture, 2);
    fixture.start(1);
    fixture.start(2);
    collect(&fixture);
    fixture
}

fn seek(fixture: &mut Fixture, slot: usize, frame: usize) {
    let ticket = fixture.with_engine(|engine| {
        let binding = engine
            .current_input_runtime_pad_binding(slot)
            .unwrap()
            .unwrap();
        engine
            .prepare_resident_control_with_source(
                slot,
                None,
                None,
                Some(frame as f64 / f64::from(RATE)),
                None,
                Some(binding.source_generation),
            )
            .unwrap()
    });
    launch_tests::wait_window(fixture, &ticket);
    assert_eq!(
        ticket.effective_seek_seconds(),
        Some(frame as f64 / f64::from(RATE))
    );
    drop(ticket);
    collect(fixture);
}

fn pair(fixture: &mut Fixture, source: usize, target: usize) {
    let captured = fixture.with_engine(|engine| engine._capture_pad_pair(source, target).unwrap());
    let ticket = fixture.with_engine(|engine| engine._prepare_pad_pair(&captured).unwrap());
    collect(fixture);
    fixture.with_engine(|engine| engine._publish_pad_pair(&ticket).unwrap());
    assert_eq!(fixture.callback.drain(), 1);
    let ack = ticket.own_acknowledgement().unwrap();
    collect(fixture);
    fixture.with_engine(|engine| {
        assert!(engine._reconcile_pad_pair(&ticket).unwrap());
    });
    collect(fixture);
    fixture.with_engine(|engine| engine._finish_pad_pair(&ticket, &ack).unwrap());
    drop(ack);
    drop(ticket);
    drop(captured);
    collect(fixture);
}

fn copy_seek_pair_case(key_lock: bool) {
    let mut fixture = fixture(key_lock);
    let original = state(&fixture, 0);
    let copied = state(&fixture, 1);
    let neighbor = state(&fixture, 2);
    // Capture the actual active finite voice, then require prefix/tail storage.
    // Neither a helper PCM owner nor a complete-reader materialization precedes it.
    seek(&mut fixture, 1, START / 2);
    seek(&mut fixture, 1, END + 64);
    assert_eq!(state(&fixture, 1).assignment, copied.assignment);
    assert_eq!(state(&fixture, 0), original);
    assert_eq!(state(&fixture, 2), neighbor);
    pair(&mut fixture, 1, 3); // Move to a genuine empty cell.
    assert_eq!(state(&fixture, 3).assignment, copied.assignment);
    assert_eq!(state(&fixture, 3).lifetime, copied.lifetime);
    pair(&mut fixture, 3, 2); // Swap two genuine independent copied contents.
    assert_eq!(state(&fixture, 2).assignment, copied.assignment);
    assert_eq!(state(&fixture, 3).assignment, neighbor.assignment);
    for (slot, inset) in [(2, 64), (3, 128)] {
        let ticket =
            launch_tests::request_window(&fixture, slot, START + inset, END - inset, key_lock);
        launch_tests::wait_window(&mut fixture, &ticket);
        assert_eq!(
            state(&fixture, slot).loop_region,
            (START + inset, Some(END - inset))
        );
        drop(ticket);
        collect(&fixture);
    }
    assert_eq!(state(&fixture, 0), original);
    assert_eq!(state(&fixture, 2).assignment, copied.assignment);
    assert_eq!(state(&fixture, 3).assignment, neighbor.assignment);
    // Complete work must resolve both moved assignments after original handles
    // have gone and collection has run on both sides of actual Pair ACK/mirror.
    for slot in [0, 2, 3] {
        {
            let work = fixture.with_engine(|engine| {
                let cache = engine.sample_cache.lock().unwrap();
                CompleteSampleWork::capture(engine, slot, cache[slot].as_ref().unwrap()).unwrap()
            });
            let sample = work
                .materialize(crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES)
                .unwrap();
            assert_eq!(
                (sample.resident_start(), sample.resident_end()),
                (0, FRAMES)
            );
        }
        collect(&fixture);
    }
}

fn copied_voice_bank_case(key_lock: bool, same_source: bool) {
    let mut fixture = fixture(key_lock);
    let old_assignment = state(&fixture, 1).assignment;
    let neighbor = state(&fixture, 2);
    let root = fixture.directory.path().join("samples");
    let prepared = if same_source {
        fixture.with_engine(|engine| {
            let sample = engine.sample_cache.lock().unwrap()[1].clone().unwrap();
            let lease = engine.cold_leases.lock().unwrap()[1].clone().unwrap();
            PreparedMigrationMaterial::from_current(&root, sample, lease).unwrap()
        })
    } else {
        let replacement = root.join("different-bank.wav");
        fs::write(&replacement, wav(&[0, 2])).unwrap();
        prepare_material_complete_for_test(&root, &replacement, RATE, 2, &|| false).unwrap()
    };
    let source = prepared.metadata()["new_reference"]
        .as_str()
        .unwrap()
        .to_owned();
    let admitted = fixture.with_engine(|engine| {
        admit_selected_material(
            engine,
            1,
            source,
            (
                false,
                false,
                false,
                Some(ResidentLoadHint {
                    start_s: 256.0 / f64::from(RATE),
                    end_s: 1024.0 / f64::from(RATE),
                    key_lock,
                }),
            ),
            fixture.producer.clone(),
            (2, RATE, root.clone()),
            "restore",
            Some(Arc::new(prepared)),
        )
        .unwrap()
    });
    let request_id = admitted.request_id;
    wait_for_with_diagnostics(
        || {
            fixture.callback.drain();
            fixture.with_engine(|engine| engine.cold_loading[1].load(Ordering::Acquire) == 0)
        },
        "copied slot replacement did not receive its own cold ACK",
        || format!("request={request_id}"),
    );
    fixture.with_engine(|engine| {
        let events = engine.loader_rx.lock().unwrap();
        let mut success = false;
        while let Ok(event) = events.try_recv() {
            match event.into_test_payload() {
                LoaderEvent::Success {
                    request_id: actual, ..
                } if actual == request_id => success = true,
                LoaderEvent::Error { error, .. } => panic!("replacement failed: {error:?}"),
                _ => {}
            }
        }
        assert!(success);
    });
    drop(admitted);
    let bank = state(&fixture, 1);
    assert_ne!(bank.assignment, old_assignment);
    assert_eq!(bank.window, (256, 1024));
    fixture.with_engine(|engine| {
        let cache = engine.sample_cache.lock().unwrap();
        let bank = cache[1].as_ref().unwrap();
        let voice = fixture
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(1))
            .unwrap();
        let sample = voice.sample.as_ref().unwrap();
        assert_eq!(
            (sample.resident_start(), sample.resident_end()),
            (START, END)
        );
        assert!(!sample.same_window(bank));
        assert_eq!(sample.same_source(bank), same_source);
    });
    // No replacement-admission handle or extra finite Voice/Bank PCM clone
    // masks collection. Fixture's distinct original complete material seed stays.
    // The real old copied voice remains after the original PAD is unloaded.
    fixture.with_engine(|engine| engine.unload_sample(0).unwrap());
    fixture.callback.drain();
    collect(&fixture);
    let before_seek = {
        let voice = fixture
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(1))
            .unwrap();
        let sample = voice.sample.as_ref().unwrap();
        (
            sample.source_address(),
            sample.frame_count(),
            voice.generation,
        )
    };
    seek(&mut fixture, 1, START / 2);
    {
        let mut voices = fixture
            .callback
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.is_playing_sample(1));
        let voice = voices.next().unwrap();
        assert!(voices.next().is_none());
        let sample = voice.sample.as_ref().unwrap();
        assert_eq!(
            (sample.source_address(), sample.frame_count()),
            (before_seek.0, before_seek.1)
        );
        assert_ne!(voice.generation, before_seek.2);
    }
    assert_eq!(state(&fixture, 1).assignment, bank.assignment);
    if !same_source {
        assert_eq!(state(&fixture, 1), bank);
    }
    let ticket = launch_tests::request_window(&fixture, 1, START + 64, END - 64, key_lock);
    launch_tests::wait_window(&mut fixture, &ticket);
    drop(ticket);
    collect(&fixture);
    let after = state(&fixture, 1);
    assert_eq!(after.assignment, bank.assignment);
    assert_eq!(after.lifetime, bank.lifetime);
    assert_eq!(after.window, (START + 64, END - 64));
    assert_eq!(state(&fixture, 2), neighbor);
    let work = fixture.with_engine(|engine| {
        let cache = engine.sample_cache.lock().unwrap();
        CompleteSampleWork::capture(engine, 1, cache[1].as_ref().unwrap()).unwrap()
    });
    let complete = work
        .materialize(crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES)
        .unwrap();
    assert_eq!(
        (complete.resident_start(), complete.resident_end()),
        (0, FRAMES)
    );
    drop(complete);
    drop(work);
    collect(&fixture);
}

#[test]
fn copied_voice_seek_after_same_source_bank_adoption_keeps_current_loop_assignment() {
    isolated(
        "copied_loop_custody_tests::seek_pair_tests::copied_voice_seek_after_same_source_bank_adoption_keeps_current_loop_assignment",
        || {
            for key_lock in [false, true] {
                copied_voice_bank_case(key_lock, true);
            }
        },
    );
}

#[test]
fn copied_voice_seek_after_different_source_bank_adoption_keeps_current_loop_assignment() {
    isolated(
        "copied_loop_custody_tests::seek_pair_tests::copied_voice_seek_after_different_source_bank_adoption_keeps_current_loop_assignment",
        || {
            for key_lock in [false, true] {
                copied_voice_bank_case(key_lock, false);
            }
        },
    );
}

#[test]
fn copied_seek_move_swap_loop_keeps_own_assignment_with_key_lock_off() {
    isolated(
        "copied_loop_custody_tests::seek_pair_tests::copied_seek_move_swap_loop_keeps_own_assignment_with_key_lock_off",
        || copy_seek_pair_case(false),
    );
}

#[test]
fn copied_seek_move_swap_loop_keeps_own_assignment_with_key_lock_on() {
    isolated(
        "copied_loop_custody_tests::seek_pair_tests::copied_seek_move_swap_loop_keeps_own_assignment_with_key_lock_on",
        || copy_seek_pair_case(true),
    );
}
