//! Productive migration control/callback and journal oracles; no CPAL device.
#![cfg(windows)]

use super::*;
use crate::audio_engine::constant_timing;
use crate::audio_engine::input_runtime_binding;
use crate::audio_engine::material_migration;
use crate::audio_engine::material_migration_control as control;
use crate::messages::ControlParameterMessage;
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use flitzis_looper_analysis::tempo_evidence::TimingBound;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize};
use std::sync::{Barrier, mpsc};

struct ReadyMaterial {
    preparation: control::MaterialMigrationPreparation,
    sample: SampleBuffer,
    parent_assignment: u64,
    original_path: PathBuf,
    cache_path: PathBuf,
}

fn ready_material(engine: &AudioEngine, root: &Path) -> ReadyMaterial {
    fs::create_dir_all(root).unwrap();
    let old_path = root.join("old.wav");
    wav(&old_path, 48_000);
    let material =
        material_migration::prepare_material_complete_for_test(root, &old_path, 48_000, 2, &|| {
            false
        })
        .unwrap();
    let sample = material.sample.clone();
    let parent_assignment = material.lease.assignment_id();
    let cache_path = material.lease.cache_path.clone();
    let metadata = material.metadata();
    let original_path = crate::audio_engine::material_paths::resolve(
        root,
        Path::new(metadata["new_reference"].as_str().unwrap()),
    )
    .unwrap()
    .path;
    for runtime_key in ["request_id", "assignment_id", "adoption", "permit", "ack"] {
        assert!(metadata.get(runtime_key).is_none(), "{runtime_key}");
    }
    ReadyMaterial {
        preparation: control::prepared_for_test(engine, material, root).unwrap(),
        sample,
        parent_assignment,
        original_path,
        cache_path,
    }
}

fn terminals(engine: &AudioEngine, requests: &[u64]) -> Vec<LoaderEvent> {
    let mut output = Vec::new();
    wait_until(|| {
        while let Ok(event) = engine
            .loader_rx
            .lock()
            .unwrap()
            .try_recv()
            .map(LoaderEvent::into_test_payload)
        {
            let event = match event {
                LoaderEvent::NativeOwned { event, .. } => *event,
                event => event,
            };
            if matches!(&event,
                LoaderEvent::Success { request_id, .. } | LoaderEvent::Error { request_id, .. }
                    if requests.contains(request_id))
            {
                output.push(event);
            }
        }
        output.len() == requests.len()
    });
    output
}

#[test]
fn material_migration_independent_first_last_pad_ack_preserves_old_voice_and_shared_backing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    let engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    let mut callback = Callback::new(&engine, &previous);
    let ready = ready_material(&engine, &root);
    assert!(!Arc::ptr_eq(&ready.sample.samples, &previous.samples));
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    let first = control::adopt_for_format(
        &engine,
        0,
        &ready.preparation,
        producer.clone(),
        (2, 48_000, root.clone()),
    )
    .unwrap();
    let last = control::adopt_for_format(
        &engine,
        215,
        &ready.preparation,
        producer,
        (2, 48_000, root.clone()),
    )
    .unwrap();
    assert_ne!(first.request_id(), last.request_id());
    assert_eq!((first.sample_id(), last.sample_id()), (0, 215));
    wait_until(|| consumer.slots() >= 2);
    assert_eq!(first.phase().unwrap(), "pending");
    assert_eq!(last.phase().unwrap(), "pending");
    assert!(!first.is_current().unwrap());
    assert!(!last.is_current().unwrap());
    callback.assert_old_voice(&previous);
    assert_eq!(callback.drain(&mut consumer), 2);
    let events = terminals(&engine, &[first.request_id(), last.request_id()]);
    assert!(
        events
            .iter()
            .all(|event| matches!(event, LoaderEvent::Success { .. }))
    );
    assert_eq!(first.phase().unwrap(), "acknowledged");
    assert_eq!(last.phase().unwrap(), "acknowledged");
    assert!(first.is_current().unwrap());
    assert!(last.is_current().unwrap());
    callback.assert_old_voice(&previous);
    let cache = engine.sample_cache.lock().unwrap();
    assert!(Arc::ptr_eq(
        &cache[0].as_ref().unwrap().samples,
        &ready.sample.samples
    ));
    assert!(Arc::ptr_eq(
        &cache[215].as_ref().unwrap().samples,
        &ready.sample.samples
    ));
    drop(cache);
    let leases = engine.cold_leases.lock().unwrap();
    let first_lease = leases[0].as_ref().unwrap();
    let last_lease = leases[215].as_ref().unwrap();
    assert_ne!(first_lease.assignment_id(), last_lease.assignment_id());
    assert_ne!(first_lease.assignment_id(), ready.parent_assignment);
    assert_ne!(last_lease.assignment_id(), ready.parent_assignment);
    assert_eq!(first_lease.cache_path, last_lease.cache_path);
    first_lease.rollback_unadopted_cache();
    first_lease.rollback_unadopted_original();
    assert!(!last_lease.assignment_retired());
    drop(leases);
    let saved_owner = engine
        .project_assets
        .acquire(&root, &ready.original_path)
        .unwrap();
    ready.preparation.release_preparation().unwrap();
    assert!(last.is_current().unwrap());
    assert!(ready.original_path.is_file());
    assert!(ready.cache_path.join("playback.f32le").is_file());
    assert!(root.join("old.wav").is_file());
    callback.assert_old_voice(&previous);
    drop(saved_owner);
}

#[test]
fn material_migration_current_poll_overlaps_real_ack_publication_and_rejected_rollback() {
    for reject in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("samples");
        let engine = AudioEngine::new().unwrap();
        let previous = old(&engine);
        let mut callback = Callback::new(&engine, &previous);
        let ready = ready_material(&engine, &root);
        let (producer, mut consumer) = rtrb::RingBuffer::new(4);
        let ticket = control::adopt_for_format(
            &engine,
            0,
            &ready.preparation,
            Arc::new(Mutex::new(producer)),
            (2, 48_000, root.clone()),
        )
        .unwrap();
        let stop = AtomicBool::new(false);
        let observed = AtomicUsize::new(0);
        let start = Barrier::new(2);
        std::thread::scope(|scope| {
            let poller = scope.spawn(|| {
                start.wait();
                while !stop.load(Ordering::Acquire) {
                    let current = ticket.is_current().unwrap();
                    if reject {
                        assert!(!current);
                    }
                    observed.fetch_add(1, Ordering::Release);
                    std::thread::yield_now();
                }
            });
            start.wait();
            wait_until(|| observed.load(Ordering::Acquire) > 0 && consumer.peek().is_ok());
            if reject {
                assert!(ticket.cancel_unclaimed());
            }
            assert_eq!(callback.drain(&mut consumer), 1);
            let event = terminals(&engine, &[ticket.request_id()]).pop().unwrap();
            assert_eq!(matches!(event, LoaderEvent::Error { .. }), reject);
            wait_until(|| engine.cold_loading[0].load(Ordering::Acquire) == 0);
            assert_eq!(ticket.is_current().unwrap(), !reject);
            stop.store(true, Ordering::Release);
            poller.join().unwrap();
        });
        assert!(observed.load(Ordering::Acquire) > 0);
        callback.assert_old_voice(&previous);
        assert!(ready.original_path.is_file());
        assert!(ready.cache_path.join("playback.f32le").is_file());
        if reject {
            assert_eq!(ticket.phase().unwrap(), "rejected");
            assert!(Arc::ptr_eq(
                &engine.sample_cache.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .samples,
                &previous.samples,
            ));
            assert_eq!(
                engine.loaded_source_generations.lock().unwrap()[0],
                (7, 48_000)
            );
        } else {
            assert_eq!(ticket.phase().unwrap(), "acknowledged");
        }
    }
}

#[test]
fn material_migration_late_ack_metadata_cannot_restore_revoked_native_source_authority() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    let engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    let mut callback = Callback::new(&engine, &previous);
    let ready = ready_material(&engine, &root);
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let ticket = control::adopt_for_format(
        &engine,
        0,
        &ready.preparation,
        Arc::new(Mutex::new(producer)),
        (2, 48_000, root),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok());
    // Hold the actual request mutex only on control: the callback cannot need it.
    // This deterministically places revocation after ACK and before worker metadata.
    let request_guard = engine.pad_request_ids.lock().unwrap();
    assert_eq!(callback.drain(&mut consumer), 1);
    assert_eq!(ticket.phase().unwrap(), "acknowledged");
    engine.input_runtime_ownership.revoke_source(0);
    drop(request_guard);
    let events = terminals(&engine, &[ticket.request_id()]);
    assert_eq!(events.len(), 1);
    assert_eq!(ticket.phase().unwrap(), "acknowledged");
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[0],
        (ticket.request_id(), 48_000)
    );
    assert!(Arc::ptr_eq(
        &engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples,
        &ready.sample.samples,
    ));
    assert!(!ticket.is_current().unwrap());
    assert!(!ticket.cancel_unclaimed());
    assert!(ready.preparation.abort_rejected().is_err());
    callback.assert_old_voice(&previous);
}

#[test]
fn material_migration_current_source_survives_same_source_timing_capture_and_parameter_drain() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    let engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    let mut callback = Callback::new(&engine, &previous);
    let ready = ready_material(&engine, &root);
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let producer = Arc::new(Mutex::new(producer));
    let ticket = control::adopt_for_format(
        &engine,
        0,
        &ready.preparation,
        producer.clone(),
        (2, 48_000, root),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok());
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(matches!(
        terminals(&engine, &[ticket.request_id()])[0],
        LoaderEvent::Success { .. }
    ));
    wait_until(|| engine.cold_loading[0].load(Ordering::Acquire) == 0);
    constant_timing::set_intent(&engine, &producer, 0, TimingIntent::Automatic).unwrap();
    assert_eq!(callback.drain(&mut consumer), 1);
    let original_request = engine.pad_request_ids.lock().unwrap()[0];
    let _captured = constant_timing::capture_preparation(
        &engine,
        0,
        TimingBound {
            halfwidth_seconds: 0.0,
            provenance: "explicit test capture".into(),
        },
        None,
    )
    .unwrap();
    assert_ne!(engine.pad_request_ids.lock().unwrap()[0], original_request);
    assert!(ticket.is_current().unwrap());
    constant_timing::set_intent(&engine, &producer, 0, TimingIntent::Manual).unwrap();
    assert_eq!(callback.drain(&mut consumer), 1);
    let (mut parameters, mut parameter_consumer) = rtrb::RingBuffer::new(2);
    parameters
        .push(ControlParameterMessage::SetPadGain {
            id: 0,
            gain_db: -6.0,
        })
        .unwrap();
    crate::audio_engine::audio_stream::drain_parameter_messages(
        &mut parameter_consumer,
        &mut callback.mixer,
        &mut callback.transport,
    );
    assert!(parameter_consumer.is_empty());
    assert!(ticket.is_current().unwrap());
    callback.assert_old_voice(&previous);
}

#[test]
fn material_migration_real_panic_handler_rejects_only_unclaimed_phase_and_reports_error() {
    for phase in [0, 1, 2, 3] {
        let adoption = Arc::new(AtomicU8::new(phase));
        let (sender, receiver) = mpsc::channel();
        run_worker_catching_panic(adoption.clone(), &sender, 215, 91, || {
            panic!("injected productive worker panic");
        });
        assert_eq!(
            adoption.load(Ordering::Acquire),
            if phase == 0 { 3 } else { phase }
        );
        let event = receiver.try_recv().unwrap();
        assert!(matches!(
            event,
            LoaderEvent::Error {
                id: 215,
                request_id: 91,
                ..
            }
        ));
        assert!(receiver.try_recv().is_err());
    }
}

#[test]
fn material_migration_hold_transfer_retains_each_real_owner_without_reopening_launch() {
    let engine = AudioEngine::new().unwrap();
    let (producer, _consumer) = rtrb::RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    let mut parent = control::hold_for_producer(&engine, vec![0, 215], &producer).unwrap();
    let first_revision = engine.input_runtime_ownership.launch_revision(0);
    let last_revision = engine.input_runtime_ownership.launch_revision(215);
    assert!(parent.take(vec![0, 0]).is_err());
    assert!(parent.take(vec![1]).is_err());
    let child = parent.take(vec![0]).unwrap();
    assert!(parent.take(vec![0]).is_err());
    assert_eq!(
        engine.input_runtime_ownership.launch_revision(0),
        first_revision
    );
    assert_eq!(
        engine.input_runtime_ownership.launch_revision(215),
        last_revision
    );
    assert!(control::hold_for_producer(&engine, vec![0], &producer).is_err());
    assert!(control::hold_for_producer(&engine, vec![215], &producer).is_err());
    parent.release().unwrap();
    assert!(
        !engine
            .input_runtime_ownership
            .launch_current(0, first_revision)
    );
    assert!(
        engine
            .input_runtime_ownership
            .launch_current(215, engine.input_runtime_ownership.launch_revision(215))
    );
    assert!(control::hold_for_producer(&engine, vec![0], &producer).is_err());
    child.release().unwrap();
    assert!(
        !engine
            .input_runtime_ownership
            .launch_current(0, first_revision)
    );
    assert!(
        engine
            .input_runtime_ownership
            .launch_current(0, engine.input_runtime_ownership.launch_revision(0))
    );
}

#[test]
fn migration_hold_release_keeps_original_cell_through_pair_and_source_replacement() {
    use crate::audio_engine::complete_context::source_custody_tests::own_pair_ack;
    use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
    use crate::audio_engine::pad_pair;
    let mut fixture = LaunchFixture::new(4);
    let mut parent =
        control::hold_for_producer(&fixture.engine, vec![0, 215], &fixture.producer).unwrap();
    let owner = fixture.engine.input_runtime_ownership.migration_hold(0);
    let pair_participants = [0, 1].map(|id| {
        fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(id)
            .unwrap()
    });
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let ticket = own_pair_ack(&mut fixture, &mut mixer);
    assert!(pad_pair::reconcile(&fixture.engine, &ticket).unwrap());
    // This fixture owns the internal Pair. Release only its exact reservation to
    // admit the later replacement; this does not certify outer durable settlement.
    let reservation = fixture.engine.input_runtime_ownership.pair_reservation(0);
    assert_ne!(reservation, 0);
    fixture
        .engine
        .input_runtime_ownership
        .release_pair(&pair_participants, reservation)
        .unwrap();
    assert_eq!(fixture.engine.input_runtime_ownership.migration_hold(0), 0);
    assert_eq!(
        fixture.engine.input_runtime_ownership.migration_hold(1),
        owner
    );
    let child = parent.take(vec![0]).unwrap();
    let other = control::hold_for_producer(&fixture.engine, vec![0], &fixture.producer).unwrap();
    let other_owner = fixture.engine.input_runtime_ownership.migration_hold(0);
    assert_ne!(owner, other_owner);
    // Migration intentionally replaces its source lifetime inside the held cell.
    fixture.publish(1, 0.25, 7);
    fixture.publish(215, 0.25, 7); // the originally EMPTY held cell is now populated
    assert_eq!(
        fixture.engine.input_runtime_ownership.migration_hold(1),
        owner
    );
    assert_eq!(
        fixture.engine.input_runtime_ownership.migration_hold(215),
        owner
    );
    parent.release().unwrap();
    assert_eq!(
        fixture.engine.input_runtime_ownership.migration_hold(215),
        0
    );
    child.release().unwrap();
    assert_eq!(fixture.engine.input_runtime_ownership.migration_hold(1), 0);
    assert_eq!(
        fixture.engine.input_runtime_ownership.migration_hold(0),
        other_owner
    );
    other.release().unwrap();
}

#[test]
fn actual_migration_source_ticket_follows_pair_but_rejects_equal_new_incarnation() {
    use crate::audio_engine::pad_pair;
    use crate::messages::AudioMessage;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    let mut engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    let mut callback = Callback::new(&engine, &previous);
    let ready = ready_material(&engine, &root);
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let producer = Arc::new(Mutex::new(producer));
    let source = control::adopt_for_format(
        &engine,
        0,
        &ready.preparation,
        producer.clone(),
        (2, 48_000, root),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok());
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(matches!(
        terminals(&engine, &[source.request_id()])[0],
        LoaderEvent::Success { .. }
    ));
    wait_until(|| engine.cold_loading[0].load(Ordering::Acquire) == 0);
    assert!(source.is_current().unwrap());
    let original_key = engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap()
        .content_key()
        .unwrap();
    let pair_participants =
        [0, 1].map(|id| engine.input_runtime_ownership.slot_snapshot(id).unwrap());
    let (_, feedback) = rtrb::RingBuffer::<AudioMessage>::new(8);
    engine.test_control_channels = Some((producer, Arc::new(std::sync::Mutex::new(feedback))));
    callback
        .mixer
        .set_prepared_source_epochs(engine.prepared_source_epochs.clone());
    let captured = pad_pair::capture(&engine, 0, 1).unwrap();
    let pair = pad_pair::prepare(&engine, &captured, 48_000, 2).unwrap();
    pad_pair::publish(&engine, &pair).unwrap();
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(pair.native_acknowledgement().is_some());
    assert!(!source.is_current().unwrap());
    assert!(pad_pair::reconcile(&engine, &pair).unwrap());
    assert!(source.is_current().unwrap());
    assert!(engine.sample_cache.lock().unwrap()[0].is_none());
    let reservation = engine.input_runtime_ownership.pair_reservation(0);
    assert_ne!(reservation, 0);
    engine
        .input_runtime_ownership
        .release_pair(&pair_participants, reservation)
        .unwrap();
    let equal = engine.sample_cache.lock().unwrap()[1].clone().unwrap();
    engine.input_runtime_ownership.revoke_source(1);
    engine
        .input_runtime_ownership
        .publish_test_source(1, &equal, 48_000, source.request_id());
    assert_ne!(
        engine
            .input_runtime_ownership
            .slot_snapshot(1)
            .unwrap()
            .content_key(),
        Some(original_key)
    );
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[1],
        (source.request_id(), 48_000)
    );
    assert!(!source.is_current().unwrap());
}

#[test]
fn material_migration_hold_rejects_queued_starts_and_release_never_revalidates_old_revision() {
    let engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    let mut callback = Callback::new(&engine, &previous);
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    // The control-only old() seam does not grant callback telemetry authority.
    // Adopt this exact source through the productive guarded source transaction,
    // so the later launch receipt has a genuine callback source ACK to name.
    let ownership = &engine.input_runtime_ownership;
    let source_placement = ownership.slot_snapshot(0).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(source_placement, source_placement.lifetime())
        .unwrap();
    let publication = ownership
        .publish_source_write(
            &write,
            &previous,
            48_000,
            7,
            ownership.authority_revision(0),
            None,
            true,
            || true,
        )
        .unwrap();
    let adoption = Arc::new(AtomicU8::new(0));
    producer
        .lock()
        .unwrap()
        .push(ControlMessage::LoadColdSample {
            initial_mix: None,
            publication,
            id: 0,
            sample: previous.clone(),
            source_generation: 7,
            content_lifetime: source_placement.lifetime(),
            adoption: adoption.clone(),
            epoch: None,
            captured_epoch: 0,
            replace_assignment: false,
            loop_region: None,
            resident_cancelled: None,
            incoming_defaults: None,
            initial_pitch: crate::audio_engine::SourcePitchInitialization::FreshNeutral,
        })
        .unwrap();
    assert_eq!(callback.drain(&mut consumer), 1);
    assert_eq!(adoption.load(Ordering::Acquire), 2);
    assert_eq!(ownership.cold_status(0, 7), Some(2));
    callback.assert_old_voice(&previous);
    assert!(callback.feedback.events.is_empty());
    callback.mixer.stop_sample(0);
    let binding = input_runtime_binding::capture(&engine, 0)
        .unwrap()
        .unwrap()
        .binding;
    let original_key = binding.placement.unwrap().content_key().unwrap();
    assert_eq!(ownership.callback_feedback_key(0), Some(original_key));
    let start = |revision| ControlMessage::TriggerInputPad {
        id: 0,
        start_s: 0.0,
        end_s: None,
        exclusive: false,
        binding,
        received_at_ns: 42,
        resident_control: None,
        launch_revision: revision,
    };
    let old_revision = engine.input_runtime_ownership.launch_revision(0);
    let enqueue_admitted = || {
        let mut queue = producer.lock().unwrap();
        assert!(!queue.is_full());
        let placement = ownership.slot_snapshot(0).unwrap();
        let prepared = ownership
            .prepare_launch_write(placement, input_runtime_binding::LaunchWriteKind::Admit)
            .unwrap();
        let revision = prepared.previous_revision;
        ownership
            .commit_launch_write(
                prepared,
                || ownership.admission_fields_current(0, binding),
                || queue.push(start(revision)).unwrap(),
            )
            .unwrap();
        revision
    };
    assert_eq!(enqueue_admitted(), old_revision);
    let hold = control::hold_for_producer(&engine, vec![0, 215], &producer).unwrap();
    assert!(
        !engine
            .input_runtime_ownership
            .launch_current(0, old_revision)
    );
    let held_revision = engine.input_runtime_ownership.launch_revision(0);
    assert!(
        !engine
            .input_runtime_ownership
            .launch_current(0, held_revision)
    );
    let held_last = engine.input_runtime_ownership.launch_revision(215);
    assert!(
        !engine
            .input_runtime_ownership
            .launch_current(215, held_last)
    );
    assert!(control::hold_for_producer(&engine, vec![215], &producer).is_err());
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(!callback.mixer.voices.iter().any(|voice| voice.active));
    assert!(callback.feedback.events.is_empty());
    producer.lock().unwrap().push(start(held_revision)).unwrap();
    hold.release().unwrap();
    assert!(
        !engine
            .input_runtime_ownership
            .launch_current(0, held_revision)
    );
    assert!(
        !engine
            .input_runtime_ownership
            .launch_current(215, held_last)
    );
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(!callback.mixer.voices.iter().any(|voice| voice.active));
    assert!(callback.feedback.events.is_empty());
    let fresh = engine.input_runtime_ownership.launch_revision(0);
    assert!(engine.input_runtime_ownership.launch_current(0, fresh));
    assert_eq!(enqueue_admitted(), fresh);
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(
        callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    assert_eq!(callback.feedback.events.len(), 1);
    let started = &callback.feedback.events[0];
    assert!(matches!(
        started,
        AudioMessage::ContentFeedback {
            id: 0, owner, cell, lifetime, kind: 1, value, ..
        } if *owner == engine.input_runtime_ownership.owner_identity().diagnostic()
            && *cell == original_key.cell().index()
            && *lifetime == original_key.lifetime()
            && *value == 0.0
    ));
    assert_eq!(engine._resolve_pad_feedback(started).unwrap(), Some(0));
    assert!(hold.release().is_err());
}

fn journal_record(transaction: &str, phase: &str) -> String {
    serde_json::json!({
        "transaction_id": transaction,
        "phase": phase,
        "captured_revision": 17,
        "config_sha256": null,
        "snapshot_json": "{}",
        "assignments": [{
            "sample_id": 0,
            "instance_id": "22222222222222222222222222222222",
            "old_reference": "samples/old.wav"
        }],
        "alias": null,
        "committed_revision": null,
        "committed_config_sha256": null,
        "error": null
    })
    .to_string()
}

fn scanned(root: &Path) -> Vec<serde_json::Value> {
    pyo3::Python::initialize();
    control::MaterialMigrationJournalStore::scan(root.to_string_lossy().into())
        .unwrap()
        .iter()
        .map(|item| serde_json::from_str(item).unwrap())
        .collect()
}

#[test]
fn material_migration_journal_exclusive_records_reopen_and_scan_do_not_restore_native_ack() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    let engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    let mut callback = Callback::new(&engine, &previous);
    let ready = ready_material(&engine, &root);
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let ticket = control::adopt_for_format(
        &engine,
        0,
        &ready.preparation,
        Arc::new(Mutex::new(producer)),
        (2, 48_000, root.clone()),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok());
    let transaction = "11111111111111111111111111111111";
    let store = control::MaterialMigrationJournalStore::new(
        root.to_string_lossy().into(),
        transaction.into(),
    )
    .unwrap();
    let first = journal_record(transaction, "captured");
    let path = PathBuf::from(store.append(first.clone()).unwrap());
    assert_eq!(path.file_name().unwrap(), "00.json");
    assert_eq!(fs::read(&path).unwrap(), first.as_bytes());
    assert!(
        control::MaterialMigrationJournalStore::new(
            root.to_string_lossy().into(),
            transaction.into(),
        )
        .is_err()
    );
    let reopened = control::MaterialMigrationJournalStore::open(
        root.to_string_lossy().into(),
        transaction.into(),
    )
    .unwrap();
    let historic_ack = journal_record(transaction, "ack_confirmed");
    let second = PathBuf::from(reopened.append(historic_ack.clone()).unwrap());
    assert_eq!(second.file_name().unwrap(), "01.json");
    assert!(store.append(journal_record(transaction, "failed")).is_err());
    assert_eq!(fs::read(&path).unwrap(), first.as_bytes());
    assert_eq!(fs::read(&second).unwrap(), historic_ack.as_bytes());
    let records = scanned(&root);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["transaction_id"], transaction);
    assert!(records[0]["error"].is_null());
    assert_eq!(records[0]["record"], historic_ack);
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&second).unwrap()).unwrap();
    for key in ["request_id", "permit", "assignment_id", "adoption", "ack"] {
        assert!(saved.get(key).is_none());
    }
    assert_eq!(ticket.phase().unwrap(), "pending");
    assert!(!ticket.is_current().unwrap());
    assert!(engine.cold_leases.lock().unwrap()[0].is_none());
    assert!(ticket.cancel_unclaimed());
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(matches!(
        terminals(&engine, &[ticket.request_id()])[0],
        LoaderEvent::Error { .. }
    ));
    callback.assert_old_voice(&previous);
}

#[test]
fn material_migration_journal_partial_unknown_and_gapped_records_remain_visible_and_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    fs::create_dir(&root).unwrap();
    let parent = root.join(".material-migrations");
    fs::create_dir(&parent).unwrap();
    let cases = [
        ("11111111111111111111111111111111", "00.json", "{"),
        ("22222222222222222222222222222222", "01.json", "{}"),
        (
            "33333333333333333333333333333333",
            "owner.txt",
            "foreign owner",
        ),
    ];
    for (id, leaf, bytes) in cases {
        let path = parent.join(id);
        fs::create_dir(&path).unwrap();
        fs::write(path.join(leaf), bytes).unwrap();
        assert!(
            control::MaterialMigrationJournalStore::open(root.to_string_lossy().into(), id.into(),)
                .is_err()
        );
    }
    let foreign_transaction = parent.join("44444444444444444444444444444444");
    fs::write(
        &foreign_transaction,
        "unknown file instead of transaction directory",
    )
    .unwrap();
    let records = scanned(&root);
    assert_eq!(records.len(), 4);
    assert!(
        records
            .iter()
            .all(|record| record["record"].is_null() && record["error"].is_string())
    );
    for (id, leaf, bytes) in cases {
        assert_eq!(
            fs::read(parent.join(id).join(leaf)).unwrap(),
            bytes.as_bytes()
        );
        let item = records
            .iter()
            .find(|item| item["transaction_id"] == id)
            .unwrap();
        assert!(!item["error"].as_str().unwrap().is_empty());
    }
    assert!(foreign_transaction.is_file());
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 4);
}

#[test]
fn material_migration_journal_rejects_invalid_typed_ids_and_preserves_bounded_record_capacity() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    fs::create_dir(&root).unwrap();
    for invalid in [
        "",
        "..",
        "../outside",
        "samples/old.wav",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "a",
    ] {
        assert!(
            control::MaterialMigrationJournalStore::new(
                root.to_string_lossy().into(),
                invalid.into(),
            )
            .is_err()
        );
    }
    assert!(!root.join(".material-migrations").exists());
    let transaction = "55555555555555555555555555555555";
    let store = control::MaterialMigrationJournalStore::new(
        root.to_string_lossy().into(),
        transaction.into(),
    )
    .unwrap();
    let path = root.join(".material-migrations").join(transaction);
    assert!(store.append(String::new()).is_err());
    assert!(store.append("{invalid".into()).is_err());
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
    for ordinal in 0..16 {
        let text = serde_json::json!({"ordinal": ordinal}).to_string();
        let record = PathBuf::from(store.append(text.clone()).unwrap());
        assert_eq!(
            record.file_name().unwrap().to_string_lossy(),
            format!("{ordinal:02}.json")
        );
        assert_eq!(fs::read(record).unwrap(), text.as_bytes());
    }
    assert!(store.append("{\"overflow\":true}".into()).is_err());
    assert_eq!(fs::read_dir(&path).unwrap().count(), 16);
    assert!(
        crate::audio_engine::material_paths::resolve(&root, &path.join("unknown.wav"),).is_err()
    );
    assert!(
        control::MaterialMigrationJournalStore::open(
            root.to_string_lossy().into(),
            transaction.into(),
        )
        .unwrap()
        .append("{}".into())
        .is_err()
    );
}

#[test]
fn material_migration_journal_holds_directory_identity_and_rejects_replacement_unknown_owner() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    fs::create_dir(&root).unwrap();
    let transaction = "66666666666666666666666666666666";
    let store = control::MaterialMigrationJournalStore::new(
        root.to_string_lossy().into(),
        transaction.into(),
    )
    .unwrap();
    let text = journal_record(transaction, "captured");
    let record = PathBuf::from(store.append(text.clone()).unwrap());
    let original = record.parent().unwrap();
    let retained = original.with_file_name("retained-transaction");
    // The actual Windows directory handle forbids replacement during a writer lifetime.
    assert!(fs::rename(original, &retained).is_err());
    assert_eq!(fs::read(&record).unwrap(), text.as_bytes());
    drop(store);
    fs::rename(original, &retained).unwrap();
    fs::create_dir(original).unwrap();
    let unknown = original.join("foreign-owner.txt");
    fs::write(&unknown, "preserve replacement owner").unwrap();
    assert!(
        control::MaterialMigrationJournalStore::open(
            root.to_string_lossy().into(),
            transaction.into(),
        )
        .is_err()
    );
    assert_eq!(fs::read(retained.join("00.json")).unwrap(), text.as_bytes());
    assert_eq!(fs::read(&unknown).unwrap(), b"preserve replacement owner");
    let records = scanned(&root);
    let current = records
        .iter()
        .find(|item| item["transaction_id"] == transaction)
        .unwrap();
    assert!(current["record"].is_null());
    assert!(current["error"].is_string());
}

pub(super) fn migration_callback_8000(engine: &AudioEngine) -> Callback {
    let mut callback = Callback {
        mixer: crate::audio_engine::audio_stream::create_stream_mixer(
            1,
            8_000,
            engine.current_timing_acknowledgements.clone(),
            engine.input_runtime_ownership.clone(),
            engine.prepared_source_epochs.clone(),
        )
        .unwrap(),
        scheduler: FixedCapacityScheduler::new(),
        transport: TransportTimeline::new(8_000),
        quantization: TriggerQuantization::Immediate,
        feedback: Feedback {
            slots: usize::MAX,
            events: Vec::new(),
        },
        retirement: Retirement {
            slots: usize::MAX,
            retired: Vec::new(),
            actual: None,
        },
    };
    callback
        .mixer
        .set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    callback
        .mixer
        .set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    callback
}

pub(super) fn restore_migration_timing_with_actual_ack(
    engine: &AudioEngine,
    callback: &mut Callback,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    original: &Path,
    envelope: &str,
    source_request: u64,
) {
    let sample = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    assert_eq!(
        engine
            .input_runtime_ownership
            .source_generation(0, &sample, 8_000),
        Some(source_request)
    );
    assert_eq!(
        engine
            .current_timing_acknowledgements
            .current_for_slot(&engine.input_runtime_ownership, 0),
        0
    );
    constant_timing::set_intent(engine, producer, 0, TimingIntent::Automatic).unwrap();
    assert_eq!(callback.drain(consumer), 1);
    let before_request = engine.pad_request_ids.lock().unwrap()[0];
    let before_epoch = engine.prepared_source_epochs[0].load(Ordering::Acquire);
    let saved =
        constant_timing::capture_saved(engine, 0, envelope, original.to_string_lossy().into())
            .unwrap();
    let timing_request = engine.pad_request_ids.lock().unwrap()[0];
    assert_eq!(timing_request, before_request + 1);
    assert!(engine.prepared_source_epochs[0].load(Ordering::Acquire) > before_epoch);
    let ticket = constant_timing::restore_saved(engine, producer, &saved).unwrap();
    assert_eq!(ticket.publication_status().unwrap(), "pending");
    assert_eq!(
        engine
            .current_timing_acknowledgements
            .current_for_slot(&engine.input_runtime_ownership, 0),
        0
    );
    assert!(
        constant_timing::export_current(engine, 0, original.to_string_lossy().into())
            .unwrap()
            .is_none()
    );
    assert_eq!(callback.drain(consumer), 1);
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    let acknowledged = engine
        .current_timing_acknowledgements
        .current_for_slot(&engine.input_runtime_ownership, 0);
    assert_ne!(acknowledged, 0);
    let encoded: serde_json::Value = serde_json::from_str(envelope).unwrap();
    let period_bits =
        u64::from_str_radix(encoded["record"]["period_bits"].as_str().unwrap(), 16).unwrap();
    let records = engine.current_constant_timing.lock().unwrap();
    let projection = records[0].last().unwrap().projection;
    assert_eq!(projection.publication_epoch, acknowledged);
    assert_eq!(projection.sample_rate_hz, 8_000);
    assert_eq!(projection.period_seconds.to_bits(), period_bits);
    drop(records);
    assert_eq!(
        callback
            .mixer
            .output_bpm_for_sample_id(0)
            .unwrap()
            .to_bits(),
        (60.0 / f64::from_bits(period_bits)).to_bits()
    );
    // The entire accepted QM arrays, hypotheses, precise period and revision survive;
    // neither the new source request nor this fresh publication becomes saved authority.
    let exported = constant_timing::export_current(engine, 0, original.to_string_lossy().into())
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&exported).unwrap(),
        encoded
    );
    use pyo3::{prelude::PyAnyMethods, types::PyDictMethods};
    pyo3::Python::attach(|py| {
        let metadata = constant_timing::current_metadata(engine, py, 0)
            .unwrap()
            .unwrap();
        let metadata = metadata.bind(py).cast::<pyo3::types::PyDict>().unwrap();
        assert_eq!(
            metadata
                .get_item("source_generation")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            source_request
        );
        assert_eq!(
            metadata
                .get_item("accepted_request_id")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            timing_request
        );
        assert_eq!(
            metadata
                .get_item("publication_epoch")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            acknowledged
        );
    });
    assert_eq!(
        engine
            .input_runtime_ownership
            .source_generation(0, &sample, 8_000),
        Some(source_request)
    );
}

const MIGRATION_CHILD_ROOT: &str = "FLITZI_MATERIAL_MIGRATION_REOPEN_ROOT";
const MIGRATION_CHILD_TEST: &str = "audio_engine::cold_load::tests::material_migration_control_tests::material_migration_new_process_child_reopens_journal_with_fresh_source_and_timing_ack";

// The facade changes device startup and project-root path routing only. Timing
// export, file/PCM verification and callback-owned acceptance are productive Rust.
#[pyo3::pyclass]
struct MigrationNativeSaveOwner {
    engine: Arc<AudioEngine>,
    samples_root: PathBuf,
}

#[pyo3::pymethods]
impl MigrationNativeSaveOwner {
    fn pad_timing_intent(&self, id: usize) -> PyResult<&'static str> {
        self.engine.pad_timing_intent(id)
    }

    fn export_current_constant_timing(
        &self,
        py: pyo3::Python<'_>,
        id: usize,
        source_path: String,
    ) -> PyResult<Option<String>> {
        let original = crate::audio_engine::material_paths::resolve(
            &self.samples_root,
            Path::new(&source_path),
        )
        .map_err(|error| pyo3::exceptions::PyValueError::new_err(error.to_string()))?
        .path;
        py.detach(|| {
            constant_timing::export_current(&self.engine, id, original.to_string_lossy().into())
        })
        .map_err(pyo3::exceptions::PyValueError::new_err)
    }
}

fn migration_python_locals<'py>(
    py: pyo3::Python<'py>,
    root: &Path,
) -> pyo3::Bound<'py, pyo3::types::PyDict> {
    use pyo3::types::PyDictMethods;
    let locals = pyo3::types::PyDict::new(py);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    locals
        .set_item(
            "source_modules",
            repo.join("src").to_string_lossy().as_ref(),
        )
        .unwrap();
    locals
        .set_item(
            "project_root",
            root.parent().unwrap().to_string_lossy().as_ref(),
        )
        .unwrap();
    locals
}

fn commit_actual_migration_project(
    engine: Arc<AudioEngine>,
    root: &Path,
    transaction: &str,
    material: &serde_json::Value,
) -> String {
    use pyo3::{prelude::PyAnyMethods, types::PyDictMethods};
    pyo3::Python::attach(|py| {
        let locals = migration_python_locals(py, root);
        locals
            .set_item("material_json", material.to_string())
            .unwrap();
        locals.set_item("transaction_id", transaction).unwrap();
        let mut artifacts = Vec::new();
        for (field, created) in [
            (
                "new_reference",
                material["created_original"].as_bool().unwrap(),
            ),
            ("cache_path", material["created_cache"].as_bool().unwrap()),
        ] {
            let mut lease =
                crate::audio_engine::material_migration_recovery::MigrationArtifactLease::capture(
                    py,
                    root.to_string_lossy().into(),
                    material[field].as_str().unwrap().into(),
                )
                .unwrap();
            artifacts.push(serde_json::json!({"role":"target", "created":created, "evidence":serde_json::from_str::<serde_json::Value>(&lease.receipt_json().unwrap()).unwrap()}));
            lease.release();
        }
        locals
            .set_item("artifact_json", serde_json::to_string(&artifacts).unwrap())
            .unwrap();
        locals
            .set_item(
                "native_owner",
                pyo3::Py::new(
                    py,
                    MigrationNativeSaveOwner {
                        engine,
                        samples_root: root.into(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        let code = std::ffi::CString::new(r#"
import sys, json, hashlib
from pathlib import Path
sys.path.insert(0, source_modules)
from flitzis_looper.models import ProjectState, PadContentIdentity
from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion, MAX_KEY_EPOCH
from flitzis_looper.material_migration_model import (
    MaterialMigrationAlias, MaterialMigrationJournal, MigrationAssignment,
)
from flitzis_looper.controller.persistence import ProjectPersistence
material = json.loads(material_json)
digest = material['original']['sha256']
alias = MaterialMigrationAlias(
    transaction_id=transaction_id, material_id=material['material_id'],
    old_reference=material['old_reference'], new_reference=material['new_reference'],
    original_sha256=digest, original_bytes=material['original']['bytes'],
    decoder_identity=material['decoder_identity'], playback_identity=material['playback_identity'],
    cache_path=material['cache_path'],
    old_source_version=f"{material['old_reference']}|sha256-v1:{digest}",
    new_source_version=f"{material['new_reference']}|sha256-v1:{digest}",
    material_creation=material['material_creation'],
)
project = ProjectState(config_revision=17)
for sample_id, uuid, raw, correction, base, extra, retrigger in (
    (0, '1'*32, 'Em', 'Cm', -4, 12, True),
    (215, 'f'*32, 'Bb', 'unknown retained correction', 6, -18, False),
):
    project.sample_paths[sample_id] = alias.old_reference
    old_material = alias.old_reference.split('/')[2][1:] if alias.old_reference.startswith('samples/materials/M') else None
    project.pad_content[sample_id] = PadContentIdentity(instance_id=uuid, material_id=old_material)
    project.pad_key_intent[sample_id] = PadKeyIntent(
        source=SourceKeyVersion(version=MAX_KEY_EPOCH, raw_key=raw),
        correction=correction, analysis_epoch=MAX_KEY_EPOCH, correction_epoch=MAX_KEY_EPOCH,
        base_shift=base, extra_shift=extra, retrigger=retrigger,
    )
project.pad_timing_intent[0] = 'automatic'
persistence = ProjectPersistence(project)
persistence.config_path = Path(project_root) / 'samples/flitzis_looper.config.json'
persistence.bind_audio(native_owner)
persistence.flush()
captured_revision, snapshot, previous_digest = persistence.capture_migration(transaction_id)
assert previous_digest == hashlib.sha256(persistence.config_path.read_bytes()).hexdigest()
(Path(project_root) / 'captured-config.json').write_bytes(persistence.config_path.read_bytes())
persistence.mark_dirty()
candidate = project.model_copy(deep=True)
for sample_id in (0,215):
    candidate.sample_paths[sample_id] = alias.new_reference
    candidate.pad_content[sample_id] = PadContentIdentity(
        instance_id=project.pad_content[sample_id].instance_id, material_id=alias.material_id,
        material_creation=alias.material_creation,
    )
candidate.material_migrations[transaction_id] = alias
revision, config_digest = persistence.commit_migration(transaction_id, persistence.revision, candidate)
config_bytes = persistence.config_path.read_bytes()
assert config_digest == hashlib.sha256(config_bytes).hexdigest()
saved = ProjectState.model_validate_json(config_bytes)
assert saved.config_revision == revision == 18
for sample_id in (0,215):
    assert saved.pad_content[sample_id].instance_id == snapshot.pad_content[sample_id].instance_id
    assert saved.pad_content[sample_id].material_id == alias.material_id
    assert saved.pad_key_intent[sample_id] == snapshot.pad_key_intent[sample_id]
    assert saved.sample_paths[sample_id] == alias.new_reference
assert saved.material_migrations[transaction_id] == alias
assert saved.sample_analysis[0].accepted_timing is not None
journal = MaterialMigrationJournal(
    transaction_id=transaction_id, phase='config_committed',
    config_reference=persistence.config_reference,
    captured_revision=captured_revision, intent_revision=revision, config_sha256=previous_digest,
    snapshot_json=snapshot.model_dump_json(),
    assignments=tuple(MigrationAssignment(sample_id=i, instance_id=snapshot.pad_content[i].instance_id,
        old_reference=alias.old_reference) for i in (0,215)), alias=alias,
    committed_revision=revision, committed_config_sha256=config_digest,
    artifacts=json.loads(artifact_json),
)
assert ProjectState.model_validate_json(journal.snapshot_json) == snapshot
journal_json = journal.model_dump_json()
assert MaterialMigrationJournal.model_validate_json(journal_json) == journal
persistence.release_migration(transaction_id)
"#).unwrap();
        py.run(&code, Some(&locals), None).unwrap();
        locals
            .get_item("journal_json")
            .unwrap()
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn validate_actual_migration_project(root: &Path, journal_json: &str) -> serde_json::Value {
    use pyo3::{prelude::PyAnyMethods, types::PyDictMethods};
    pyo3::Python::attach(|py| {
        let locals = migration_python_locals(py, root);
        locals.set_item("journal_json", journal_json).unwrap();
        let code = std::ffi::CString::new(r#"
import sys, json, hashlib
from pathlib import Path
sys.path.insert(0, source_modules)
from flitzis_looper.models import ProjectState
from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion, MAX_KEY_EPOCH
from flitzis_looper.material_migration_model import MaterialMigrationAlias, MaterialMigrationJournal
record = MaterialMigrationJournal.model_validate_json(journal_json)
config_bytes = (Path(project_root) / 'samples/flitzis_looper.config.json').read_bytes()
state = ProjectState.model_validate_json(config_bytes)
snapshot = ProjectState.model_validate_json(record.snapshot_json)
assert record.phase == 'config_committed' and record.error is None
import os
assert record.config_reference == os.path.normcase(str(
    (Path(project_root) / 'samples/flitzis_looper.config.json').resolve()))
assert hashlib.sha256(config_bytes).hexdigest() == record.committed_config_sha256
assert state.config_revision == record.committed_revision == record.intent_revision == 18
assert snapshot.config_revision == record.captured_revision == 17
assert record.config_sha256 is not None
previous_bytes = (Path(project_root) / 'captured-config.json').read_bytes()
assert hashlib.sha256(previous_bytes).hexdigest() == record.config_sha256
previous = ProjectState.model_validate_json(previous_bytes)
assert previous.config_revision == record.captured_revision
alias = MaterialMigrationAlias.model_validate(record.alias.model_dump())
assert alias == state.material_migrations[record.transaction_id]
assert alias.transaction_id == record.transaction_id
assert alias.old_source_version == f'{alias.old_reference}|sha256-v1:{alias.original_sha256}'
assert alias.new_source_version == f'{alias.new_reference}|sha256-v1:{alias.original_sha256}'
assert [(a.sample_id,a.instance_id,a.old_reference) for a in record.assignments] == [
    (0,'1'*32,alias.old_reference), (215,'f'*32,alias.old_reference),
]
for sample_id, uuid, raw, correction, base, extra, retrigger in (
    (0, '1'*32, 'Em', 'Cm', -4, 12, True),
    (215, 'f'*32, 'Bb', 'unknown retained correction', 6, -18, False),
):
    expected_key = PadKeyIntent(
        source=SourceKeyVersion(version=MAX_KEY_EPOCH, raw_key=raw), correction=correction,
        analysis_epoch=MAX_KEY_EPOCH, correction_epoch=MAX_KEY_EPOCH,
        base_shift=base, extra_shift=extra, retrigger=retrigger,
    )
    assert state.pad_content[sample_id].instance_id == snapshot.pad_content[sample_id].instance_id == uuid
    assert state.pad_content[sample_id].material_id == alias.material_id
    old_material = alias.old_reference.split('/')[2][1:] if alias.old_reference.startswith('samples/materials/M') else None
    assert snapshot.pad_content[sample_id].material_id == old_material
    assert state.pad_key_intent[sample_id] == snapshot.pad_key_intent[sample_id] == expected_key
    assert previous.pad_key_intent[sample_id] == expected_key
    assert previous.pad_content[sample_id] == snapshot.pad_content[sample_id]
    assert previous.sample_paths[sample_id] == alias.old_reference
    assert state.sample_paths[sample_id] == alias.new_reference
    assert snapshot.sample_paths[sample_id] == alias.old_reference
assert state.pad_key_intent[0] is not state.pad_key_intent[215]
assert state.pad_timing_intent[0] == 'automatic'
assert state.sample_analysis[0].accepted_timing is not None
assert state.sample_analysis[0].accepted_timing.model_dump() == json.loads(
    (Path(project_root)/'accepted-timing.json').read_text(encoding='utf-8'))
# Only validated durable primitives cross back to this new native engine. Full
# saved evidence remains historical provenance, never a source/permit/ACK owner.
validated_json = json.dumps({
    'old_reference': alias.old_reference, 'new_reference': alias.new_reference, 'cache_path': alias.cache_path,
    'material_id': alias.material_id, 'original_sha256': alias.original_sha256,
    'original_bytes': alias.original_bytes,
    'accepted_timing': state.sample_analysis[0].accepted_timing.model_dump(),
})
"#).unwrap();
        py.run(&code, Some(&locals), None).unwrap();
        let encoded: String = locals
            .get_item("validated_json")
            .unwrap()
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    })
}

#[test]
fn material_migration_new_process_reopens_journal_with_fresh_source_and_timing_ack() {
    readable_migration_new_process(false);
}

#[test]
fn material_migration_mid_new_process_reopens_readable_journal_with_fresh_source_window_timing_pair_ack()
 {
    readable_migration_new_process(true);
}

fn migration_execution_bindings() -> serde_json::Value {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    let inputs = [
        "rust/crates/looper/src/audio_engine/material_paths.rs",
        "rust/crates/looper/src/audio_engine/cold_store.rs",
        "rust/crates/looper/src/audio_engine/material_migration.rs",
        "rust/crates/looper/src/audio_engine/material_migration_control_tests.rs",
        "rust/crates/looper/src/audio_engine/material_migration_reuse_process_tests.rs",
        "rust/crates/looper/src/audio_engine/stem_pair.rs",
        "src/flitzis_looper/controller/persistence.py",
        "src/flitzis_looper/material_migration_model.py",
    ];
    let hashes = inputs
        .into_iter()
        .map(|relative| {
            (
                relative.to_owned(),
                serde_json::json!(format!(
                    "{:x}",
                    Sha256::digest(fs::read(repo.join(relative)).unwrap())
                )),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let image = std::env::current_exe().unwrap();
    serde_json::json!({"source_sha256":hashes, "test_executable":image,
        "test_executable_sha256":format!("{:x}", Sha256::digest(fs::read(&image).unwrap()))})
}

fn readable_migration_new_process(mid_layout: bool) {
    pyo3::Python::initialize();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let scratch = workspace.join("scratch/goal-readable-material-layout-20261011");
    fs::create_dir_all(&scratch).unwrap();
    let directory = tempfile::tempdir_in(&scratch).unwrap();
    let root = directory.path().join("samples");
    fs::create_dir(&root).unwrap();
    let transaction = "77777777777777777777777777777777";
    let (fixture_original, envelope) = constant_timing::migration_test_fixture();
    let legacy = if mid_layout {
        root.join("materials/M0123456789abcdef0123456789abcdef/original/old.wav")
    } else {
        root.join("old.wav")
    };
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    let original_bytes = fs::read(&fixture_original).unwrap();
    fs::write(&legacy, &original_bytes).unwrap();
    let metadata;
    let encoded_record;
    let pair_selection;
    // All source tickets, registrations, permits and PCM Arcs die before the child
    // process starts. Only verified original/cache files, journal and evidence remain.
    {
        let (producer, mut consumer) = rtrb::RingBuffer::new(8);
        let producer = Arc::new(Mutex::new(producer));
        let engine = Arc::new(constant_timing::with_control_producer(
            AudioEngine::new().unwrap(),
            &producer,
        ));
        let mut old_transaction = crate::audio_engine::cold_store::ColdTransaction::capture(
            &root,
            &legacy,
            false,
            &|| false,
        )
        .unwrap();
        let old_sample = material_migration::prepare_pcm(
            &mut old_transaction,
            &legacy,
            8_000,
            1,
            crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES,
            Some(crate::audio_engine::cold_residency::ResidentLoadHint {
                start_s: 2.25,
                end_s: 3.25,
                key_lock: false,
            }),
            &|| false,
            &mut |_| {},
        )
        .unwrap();
        old_transaction.commit(&|| false).unwrap();
        old_transaction.bind_pcm(&old_sample.samples);
        let old_lease = old_transaction.into_lease();
        let old_cache = old_lease.cache_path.clone();
        let material = material_migration::prepare_material_for_reference(
            &root,
            &legacy,
            8_000,
            1,
            &|| false,
            &old_sample,
        )
        .unwrap();
        assert!(
            material.lease.integrity.warm,
            "compatible old PCM must migrate without decoding"
        );
        assert_eq!(material.sample.resident_start(), 18_000);
        assert_eq!(material.sample.resident_end(), 26_000);
        assert_eq!(material.sample.samples.len(), 8_000);
        assert_ne!(material.lease.original_path, legacy);
        assert!(
            crate::audio_engine::material_paths::readable_original(
                &root,
                &material.lease.original_path
            )
            .unwrap()
        );
        metadata = material.metadata();
        let old_reference = metadata["old_reference"].as_str().unwrap();
        let digest = metadata["original"]["sha256"].as_str().unwrap();
        let old_version = format!("{old_reference}|sha256-v1:{digest}");
        let new_version = format!(
            "{}|sha256-v1:{digest}",
            metadata["new_reference"].as_str().unwrap()
        );
        let old_stems = if mid_layout {
            legacy
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("stems/.ready-0123456789abcdef0123456789abcdef")
        } else {
            root.join("stems/#1")
        };
        fs::create_dir_all(&old_stems).unwrap();
        let mut wav_hashes = serde_json::Map::new();
        for name in crate::audio_engine::stem_cache::STEM_FILE_NAMES {
            fs::write(old_stems.join(format!("{name}.wav")), &original_bytes).unwrap();
            wav_hashes.insert(
                name.into(),
                serde_json::json!(format!("{:x}", Sha256::digest(&original_bytes))),
            );
        }
        fs::write(old_stems.join(".complete.json"), serde_json::to_vec(&serde_json::json!({"schema":"stem-set-sha256-v1", "source_version":old_version, "stems":wav_hashes})).unwrap()).unwrap();
        let copied = crate::audio_engine::material_migration_stems::copy_stem_generation(
            &root,
            &material,
            &old_stems,
            &old_version,
            &new_version,
            "abcdef0123456789abcdef0123456789",
            &|| false,
        )
        .unwrap();
        assert!(copied.created);
        // The old source and complete cache stay held while the new paths are
        // prepared. Each adopting subscriber derives its own current window.
        for id in [0, 215] {
            engine.sample_cache.lock().unwrap()[id] = Some(old_sample.clone());
            engine.cold_leases.lock().unwrap()[id] = Some(old_lease.clone());
            engine
                .input_runtime_ownership
                .publish_test_source(id, &old_sample, 8_000, 7);
        }
        let preparation = control::prepared_for_test(&engine, material, &root).unwrap();

        let mut callback = migration_callback_8000(&engine);
        let source = control::adopt_for_format(
            &engine,
            0,
            &preparation,
            producer.clone(),
            (1, 8_000, root.clone()),
        )
        .unwrap();
        let last_source = control::adopt_for_format(
            &engine,
            215,
            &preparation,
            producer.clone(),
            (1, 8_000, root.clone()),
        )
        .unwrap();
        wait_until(|| consumer.slots() >= 2);
        assert_eq!(source.phase().unwrap(), "pending");
        assert_eq!(last_source.phase().unwrap(), "pending");
        assert_eq!(callback.drain(&mut consumer), 2);
        let events = terminals(&engine, &[source.request_id(), last_source.request_id()]);
        assert!(
            events
                .iter()
                .all(|event| matches!(event, LoaderEvent::Success { .. }))
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, LoaderEvent::Success { id: 0, .. }))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, LoaderEvent::Success { id: 215, .. }))
                .count(),
            1
        );
        wait_until(|| {
            [0, 215]
                .into_iter()
                .all(|id| engine.cold_loading[id].load(Ordering::Acquire) == 0)
        });
        assert_eq!(source.phase().unwrap(), "acknowledged");
        assert_eq!(last_source.phase().unwrap(), "acknowledged");
        assert!(source.is_current().unwrap());
        assert!(last_source.is_current().unwrap());
        let ticket = engine
            .capture_prepared_source(0, new_version.clone())
            .unwrap();
        let pair = engine
            .prepare_stem_pair_at_root(
                &root,
                0,
                &new_version,
                &copied.cache_reference,
                &ticket,
                true,
                None,
            )
            .unwrap();
        pair_selection = serde_json::from_str::<serde_json::Value>(pair.selection_json()).unwrap();
        pair.select();
        engine
            .publish_stem_pair_with_producer(&pair, &ticket, &producer)
            .unwrap();
        assert_eq!(ticket.publication_status(), "pending");
        assert_eq!(callback.drain(&mut consumer), 1);
        assert_eq!(ticket.publication_status(), "accepted");
        assert!(old_cache.join("playback.f32le").is_file());
        assert_eq!(fs::read(&legacy).unwrap(), original_bytes);
        let original = crate::audio_engine::material_paths::resolve(
            &root,
            Path::new(metadata["new_reference"].as_str().unwrap()),
        )
        .unwrap()
        .path;
        assert_eq!(fs::read(&original).unwrap(), original_bytes);
        restore_migration_timing_with_actual_ack(
            &engine,
            &mut callback,
            &producer,
            &mut consumer,
            &original,
            &envelope,
            source.request_id(),
        );
        assert!(source.is_current().unwrap());
        assert!(last_source.is_current().unwrap());
        encoded_record =
            commit_actual_migration_project(engine.clone(), &root, transaction, &metadata);
        preparation.release_preparation().unwrap();
    }
    fs::write(
        directory.path().join("pair-selection.json"),
        serde_json::to_vec(&pair_selection).unwrap(),
    )
    .unwrap();
    let evidence_path = directory.path().join("accepted-timing.json");
    fs::write(&evidence_path, &envelope).unwrap();
    let config_path = root.join("flitzis_looper.config.json");
    let config_bytes = fs::read(&config_path).unwrap();
    let record: serde_json::Value = serde_json::from_str(&encoded_record).unwrap();
    assert_eq!(record["phase"], "config_committed");
    assert_eq!(record["committed_revision"], 18);
    assert_eq!(
        record["committed_config_sha256"],
        format!("{:x}", Sha256::digest(&config_bytes))
    );
    assert!(record.get("verified_material").is_none());
    let store = control::MaterialMigrationJournalStore::new(
        root.to_string_lossy().into(),
        transaction.into(),
    )
    .unwrap();
    let journal_path = PathBuf::from(store.append(encoded_record.clone()).unwrap());
    drop(store);
    let expected_cache = crate::audio_engine::material_paths::resolve(
        &root,
        Path::new(metadata["cache_path"].as_str().unwrap()),
    )
    .unwrap()
    .path;
    let cache_generations = fs::read_dir(expected_cache.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<std::collections::BTreeSet<_>>();
    let output_path = directory.path().join("child-output.txt");
    let output_file = fs::File::create(&output_path).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg(MIGRATION_CHILD_TEST)
        .arg("--nocapture")
        .env(MIGRATION_CHILD_ROOT, directory.path())
        .stdout(std::process::Stdio::from(output_file.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(output_file))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!(
                "owned migration child timed out: {}",
                fs::read_to_string(&output_path).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let output = fs::read_to_string(&output_path).unwrap();
    assert!(status.success(), "fresh migration child failed: {output}");
    assert!(
        output.contains("1 passed"),
        "exact child entry was not executed: {output}"
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.path().join("child-verified.json")).unwrap())
            .unwrap();
    assert_eq!(
        receipt["process_id"].as_u64().unwrap(),
        u64::from(child.id())
    );
    assert_ne!(
        receipt["process_id"].as_u64().unwrap(),
        u64::from(std::process::id())
    );
    assert_eq!(receipt["source_callback_verified"], true);
    assert_eq!(receipt["timing_callback_verified"], true);
    assert_eq!(receipt["window_callback_verified"], true);
    assert_eq!(receipt["pair_callback_verified"], true);
    let pair_subscribers = receipt["pair_callback_subscribers"].as_array().unwrap();
    assert_eq!(pair_subscribers.len(), 2);
    for (subscriber, id, window) in [
        (&pair_subscribers[0], 0, serde_json::json!([18_000, 26_000])),
        (
            &pair_subscribers[1],
            215,
            serde_json::json!([20_000, 28_000]),
        ),
    ] {
        assert_eq!(subscriber["sample_id"], id);
        assert_eq!(subscriber["window"], window);
        assert_eq!(subscriber["publication_status"], "accepted");
        assert_eq!(subscriber["selected_pair"], pair_selection);
        assert_eq!(subscriber["actual_callback_components"], 4);
    }
    assert_eq!(receipt["config_and_journal_verified"], true);
    assert_eq!(fs::read(&evidence_path).unwrap(), envelope.as_bytes());
    assert_eq!(fs::read(&config_path).unwrap(), config_bytes);
    assert_eq!(fs::read(&journal_path).unwrap(), encoded_record.as_bytes());
    let reopened = scanned(&root);
    assert_eq!(reopened.len(), 1);
    assert!(reopened[0]["error"].is_null());
    assert_eq!(reopened[0]["record"].as_str().unwrap(), encoded_record);
    assert_eq!(
        fs::read_dir(expected_cache.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<std::collections::BTreeSet<_>>(),
        cache_generations
    );
    assert_eq!(fs::read(&legacy).unwrap(), original_bytes);
    let kept = directory.keep();
    let evidence = serde_json::json!({
        "schema":"readable-material-migration-native-reopen-v1",
        "layout":if mid_layout {"material_id"} else {"legacy"},
        "parent_process_id":std::process::id(), "child":receipt,
        "fixture_directory":kept, "old_original":legacy,
        "new_reference":metadata["new_reference"], "cache_reference":metadata["cache_path"],
        "pair_selection":pair_selection, "runtime_bindings":migration_execution_bindings(),
        "config_sha256":format!("{:x}",Sha256::digest(&config_bytes)),
        "journal_sha256":format!("{:x}",Sha256::digest(encoded_record.as_bytes())),
        "original_sha256":format!("{:x}",Sha256::digest(&original_bytes)),
        "scope":"productive owned migration source/PCM/five-WAV/current-config/journal/fresh Native source-window-timing-pair ACK; no GUI/device/music/whole Goal acceptance"
    });
    fs::write(
        kept.join("material-migration-evidence.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}

#[test]
fn material_migration_new_process_child_reopens_journal_with_fresh_source_and_timing_ack() {
    let Some(directory) = std::env::var_os(MIGRATION_CHILD_ROOT).map(PathBuf::from) else {
        return; // The ordinary suite invokes the parent, which runs this exact entry.
    };
    pyo3::Python::initialize();
    let root = directory.join("samples");
    let records = scanned(&root);
    assert_eq!(records.len(), 1);
    assert!(records[0]["error"].is_null());
    let transaction = records[0]["transaction_id"].as_str().unwrap();
    let reopened = control::MaterialMigrationJournalStore::open(
        root.to_string_lossy().into(),
        transaction.into(),
    )
    .unwrap();
    let journal_json = records[0]["record"].as_str().unwrap();
    let record: serde_json::Value = serde_json::from_str(journal_json).unwrap();
    for runtime_key in [
        "source_ticket",
        "permit",
        "assignment_id",
        "adoption",
        "verified_material",
    ] {
        assert!(record.get(runtime_key).is_none(), "persisted {runtime_key}");
    }
    // Genuine current config, rollback snapshot and committed alias are checked
    // by their strict production schemas before they select the new native source.
    let material = validate_actual_migration_project(&root, journal_json);
    for runtime_key in ["request_id", "assignment_id", "adoption", "permit", "ack"] {
        assert!(
            material.get(runtime_key).is_none(),
            "persisted {runtime_key}"
        );
    }
    let original = crate::audio_engine::material_paths::resolve(
        &root,
        Path::new(material["new_reference"].as_str().unwrap()),
    )
    .unwrap()
    .path;
    let expected_cache = crate::audio_engine::material_paths::resolve(
        &root,
        Path::new(material["cache_path"].as_str().unwrap()),
    )
    .unwrap()
    .path;
    let envelope = material["accepted_timing"].to_string();
    let original_bytes = fs::read(&original).unwrap();
    assert_eq!(
        material["original_bytes"].as_u64().unwrap(),
        original_bytes.len() as u64
    );
    assert_eq!(
        material["original_sha256"],
        format!("{:x}", Sha256::digest(&original_bytes))
    );
    assert!(crate::audio_engine::material_paths::readable_original(&root, &original).unwrap());
    let engine = AudioEngine::new().unwrap();
    assert!(engine.sample_cache.lock().unwrap()[0].is_none());
    assert!(engine.cold_leases.lock().unwrap()[0].is_none());
    assert_eq!(engine.loaded_source_generations.lock().unwrap()[0], (0, 0));
    assert_eq!(engine.prepared_source_epochs[0].load(Ordering::Acquire), 1);
    assert_eq!(
        engine
            .current_timing_acknowledgements
            .current_for_slot(&engine.input_runtime_ownership, 0),
        0
    );
    assert!(engine.sample_cache.lock().unwrap()[215].is_none());
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[215],
        (0, 0)
    );
    assert_eq!(
        engine
            .current_timing_acknowledgements
            .current_for_slot(&engine.input_runtime_ownership, 215),
        0
    );
    let mut callback = migration_callback_8000(&engine);
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    let engine = constant_timing::with_control_producer(engine, &producer);
    // Ordinary verified canonical restore, not migration preparation or a carried
    // parent subscriber. A new process allocates its own source/lease/permit.
    let admitted = admit_selected_material(
        &engine,
        0,
        original.to_string_lossy().into(),
        (
            false,
            false,
            false,
            Some(crate::audio_engine::cold_residency::ResidentLoadHint {
                start_s: 2.25,
                end_s: 3.25,
                key_lock: false,
            }),
        ),
        producer.clone(),
        (1, 8_000, root.clone()),
        "restore",
        None,
    )
    .unwrap();
    let last_admitted = admit_selected_material(
        &engine,
        215,
        original.to_string_lossy().into(),
        (
            false,
            false,
            false,
            Some(crate::audio_engine::cold_residency::ResidentLoadHint {
                start_s: 2.5,
                end_s: 3.5,
                key_lock: false,
            }),
        ),
        producer.clone(),
        (1, 8_000, root.clone()),
        "restore",
        None,
    )
    .unwrap();
    wait_until(|| consumer.slots() >= 2);
    assert_eq!(admitted.adoption.load(Ordering::Acquire), 0);
    assert_eq!(last_admitted.adoption.load(Ordering::Acquire), 0);
    assert_eq!(
        engine
            .current_timing_acknowledgements
            .current_for_slot(&engine.input_runtime_ownership, 0),
        0
    );
    let pending = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let last_pending = engine.sample_cache.lock().unwrap()[215].clone().unwrap();
    assert_eq!(
        (pending.resident_start(), pending.resident_end()),
        (18_000, 26_000)
    );
    assert_eq!(
        (last_pending.resident_start(), last_pending.resident_end()),
        (20_000, 28_000)
    );
    assert!(!Arc::ptr_eq(&pending.samples, &last_pending.samples));
    assert_eq!(callback.drain(&mut consumer), 2);
    let events = terminals(&engine, &[admitted.request_id, last_admitted.request_id]);
    assert!(
        events
            .iter()
            .all(|event| matches!(event, LoaderEvent::Success { .. }))
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, LoaderEvent::Success { id: 0, .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, LoaderEvent::Success { id: 215, .. }))
            .count(),
        1
    );
    wait_until(|| {
        [0, 215]
            .into_iter()
            .all(|id| engine.cold_loading[id].load(Ordering::Acquire) == 0)
    });
    assert_eq!(admitted.adoption.load(Ordering::Acquire), 2);
    assert_eq!(last_admitted.adoption.load(Ordering::Acquire), 2);
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[0],
        (admitted.request_id, 8_000)
    );
    assert_eq!(
        engine.cold_leases.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .cache_path,
        expected_cache
    );
    assert_eq!(
        engine.cold_leases.lock().unwrap()[215]
            .as_ref()
            .unwrap()
            .cache_path,
        expected_cache
    );
    let last_sample = engine.sample_cache.lock().unwrap()[215].clone().unwrap();
    assert_eq!(
        engine
            .input_runtime_ownership
            .source_generation(215, &last_sample, 8_000),
        Some(last_admitted.request_id)
    );
    restore_migration_timing_with_actual_ack(
        &engine,
        &mut callback,
        &producer,
        &mut consumer,
        &original,
        &envelope,
        admitted.request_id,
    );
    let selected: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("pair-selection.json")).unwrap()).unwrap();
    let source_version = format!(
        "{}|sha256-v1:{}",
        material["new_reference"].as_str().unwrap(),
        material["original_sha256"].as_str().unwrap()
    );
    let mut pair_acknowledgements = Vec::new();
    for (id, source_request, expected_window) in [
        (0, admitted.request_id, (18_000, 26_000)),
        (215, last_admitted.request_id, (20_000, 28_000)),
    ] {
        assert!(callback.mixer.stems_for_measurement()[id].is_none());
        let ticket = engine
            .capture_prepared_source(id, source_version.clone())
            .unwrap();
        assert_eq!(ticket.id, id);
        assert_eq!(
            ticket.request_id,
            engine.pad_request_ids.lock().unwrap()[id]
        );
        assert_eq!(ticket.publication_status(), "captured");
        let pair = engine
            .prepare_stem_pair_at_root(
                &root,
                id,
                &source_version,
                selected["wav_generation"].as_str().unwrap(),
                &ticket,
                true,
                Some(selected["descriptor_reference"].as_str().unwrap()),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(pair.selection_json()).unwrap(),
            selected
        );
        pair.select();
        engine
            .publish_stem_pair_with_producer(&pair, &ticket, &producer)
            .unwrap();
        assert_eq!(ticket.publication_status(), "pending");
        let ControlMessage::PublishPreparedStems {
            id: queued_id,
            stems,
        } = consumer.peek().unwrap()
        else {
            panic!("fresh subscriber did not queue its actual prepared stem set");
        };
        assert_eq!(*queued_id, id);
        let queued = stems.clone();
        assert!(callback.mixer.stems_for_measurement()[id].is_none());
        assert_eq!(callback.drain(&mut consumer), 1);
        assert_eq!(ticket.publication_status(), "accepted");
        let actual = callback.mixer.stems_for_measurement()[id].as_ref().unwrap();
        for (prepared, acknowledged) in queued.stems.iter().zip(&actual.stems) {
            assert!(Arc::ptr_eq(&prepared.samples, &acknowledged.samples));
            assert_eq!(
                (acknowledged.resident_start(), acknowledged.resident_end()),
                expected_window
            );
        }
        let source = engine.sample_cache.lock().unwrap()[id].clone().unwrap();
        assert_eq!(
            engine
                .input_runtime_ownership
                .source_generation(id, &source, 8_000),
            Some(source_request)
        );
        pair_acknowledgements.push(serde_json::json!({
            "sample_id":id, "publication_status":ticket.publication_status(),
            "window":[expected_window.0,expected_window.1],
            "selected_pair":selected, "actual_callback_components":actual.stems.len(),
        }));
    }
    assert!(!Arc::ptr_eq(
        &callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .stems[0]
            .samples,
        &callback.mixer.stems_for_measurement()[215]
            .as_ref()
            .unwrap()
            .stems[0]
            .samples,
    ));
    let artifact_receipts = pyo3::Python::attach(|py| {
        ["wav_generation", "pcm_generation", "descriptor_reference"]
            .into_iter()
            .map(|field| {
                let mut lease =
                    crate::audio_engine::material_migration_recovery::MigrationArtifactLease::capture(
                        py,
                        root.to_string_lossy().into(),
                        selected[field].as_str().unwrap().into(),
                    )
                    .unwrap();
                let receipt = lease.receipt_json().unwrap();
                let mut reopened =
                    crate::audio_engine::material_migration_recovery::MigrationArtifactLease::reopen(
                        py,
                        root.to_string_lossy().into(),
                        receipt.clone(),
                    )
                    .unwrap();
                assert_eq!(reopened.receipt_json().unwrap(), receipt);
                reopened.release();
                lease.release();
                serde_json::from_str::<serde_json::Value>(&receipt).unwrap()
            })
            .collect::<Vec<_>>()
    });
    pyo3::Python::attach(|py| {
        let config = root.join("flitzis_looper.config.json");
        let mut guard =
            crate::audio_engine::material_migration_recovery::MigrationProjectGuard::new(
                py,
                root.to_string_lossy().into(),
                config.to_string_lossy().into(),
            )
            .unwrap();
        let mut inventory = guard.lock_inventory(py).unwrap();
        for (id, instance) in [(0, "1".repeat(32)), (215, "f".repeat(32))] {
            crate::audio_engine::material_migration_recovery::restore_current_material_creation(
                &engine,
                id,
                &record["alias"]["material_creation"].to_string(),
                &config.to_string_lossy(),
                &instance,
                &inventory,
                Some(journal_json),
            )
            .unwrap();
        }
        inventory.release();
        guard.release();
    });
    let old_original = crate::audio_engine::material_paths::resolve(
        &root,
        Path::new(material["old_reference"].as_str().unwrap()),
    )
    .unwrap()
    .path;
    assert_eq!(
        fs::read(&original).unwrap(),
        fs::read(&old_original).unwrap()
    );
    drop(reopened);
    // This test receipt records assertions only; it carries no replayable ticket,
    // source request, epoch, permit or ACK object back to the parent process.
    fs::write(
        directory.join("child-verified.json"),
        serde_json::to_vec(&serde_json::json!({
            "process_id": std::process::id(),
            "config_and_journal_verified": true,
            "source_callback_verified": true,
            "timing_callback_verified": true,
            "window_callback_verified": true,
            "pair_callback_verified": true,
            "pair_callback_subscribers":pair_acknowledgements,
            "source_windows":[[18_000,26_000],[20_000,28_000]],
            "old_original":old_original, "new_original":original,
            "pair_artifact_receipts":artifact_receipts,
            "runtime_bindings":migration_execution_bindings(),
        }))
        .unwrap(),
    )
    .unwrap();
}

#[path = "material_migration_resume_tests.rs"]
mod material_migration_resume_tests;

#[path = "material_migration_reuse_process_tests.rs"]
mod material_migration_reuse_process_tests;

#[test]
fn material_migration_seed_subscriber_reads_actual_finite_range_before_own_callback_ack() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    fs::create_dir_all(&root).unwrap();
    let original = root.join("old.wav");
    let protected = wav(&original, 48_000);
    let engine = AudioEngine::new().unwrap();
    let previous = old(&engine)
        .with_complete_source(48_000)
        .window(2, 6, 3, crate::messages::ResidentContext::FiniteLoop)
        .unwrap();
    engine.sample_cache.lock().unwrap()[0] = Some(previous.clone());
    engine
        .input_runtime_ownership
        .publish_test_source(0, &previous, 48_000, 7);
    let mut callback = Callback::new(&engine, &previous);

    // Use the productive metadata preparation entry point, which holds one frame
    // and the complete sealed artifact identity, without an ALL playback demand.
    let material =
        material_migration::prepare_material(&root, &original, 48_000, 2, &|| false).unwrap();
    let seed = material.sample.clone();
    assert_eq!(
        (
            seed.frame_count(),
            seed.resident_start(),
            seed.resident_end()
        ),
        (7, 0, 1)
    );
    assert_eq!(seed.samples.len(), 2);
    let parent_assignment = material.lease.assignment_id();
    let expected_cache = material.lease.cache_path.clone();
    let preparation = control::prepared_for_test(&engine, material, &root).unwrap();
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let ticket = control::adopt_for_format(
        &engine,
        0,
        &preparation,
        Arc::new(Mutex::new(producer)),
        (2, 48_000, root.clone()),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok());
    assert_eq!(ticket.phase().unwrap(), "pending");
    assert!(!ticket.is_current().unwrap());
    // Control publishes its pending cache entry before enqueueing the guarded
    // command. The native bank and ticket authority still require their own ACK.
    let pending = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    assert_eq!(
        (
            pending.frame_count(),
            pending.resident_start(),
            pending.resident_end()
        ),
        (7, 2, 6)
    );
    assert!(Arc::ptr_eq(
        &callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples,
        &previous.samples
    ));
    let ControlMessage::LoadColdSample {
        sample,
        adoption,
        loop_region,
        ..
    } = consumer.peek().unwrap()
    else {
        panic!("migration did not queue the productive guarded cold command");
    };
    assert!(Arc::ptr_eq(&sample.samples, &pending.samples));
    assert_eq!(*loop_region, Some((2, 6)));
    assert_eq!(adoption.load(Ordering::Acquire), 0);
    assert_eq!(
        engine
            .input_runtime_ownership
            .cold_status(0, ticket.request_id()),
        Some(0)
    );
    callback.assert_old_voice(&previous);
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(matches!(
        terminal(&engine, ticket.request_id()),
        LoaderEvent::Success { .. }
    ));
    wait_until(|| engine.cold_loading[0].load(Ordering::Acquire) == 0);
    assert_eq!(ticket.phase().unwrap(), "acknowledged");
    assert!(ticket.is_current().unwrap());

    let selected = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    assert!(Arc::ptr_eq(&selected.samples, &pending.samples));
    assert!(Arc::ptr_eq(
        &callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples,
        &selected.samples
    ));
    assert_eq!(
        (
            selected.frame_count(),
            selected.resident_start(),
            selected.resident_end()
        ),
        (7, 2, 6)
    );
    assert_eq!(
        selected.residency.as_ref().unwrap().context,
        crate::messages::ResidentContext::KeyLockFiniteLoop
    );
    // PreserveEffective prepares Native-capable geometry without enabling the
    // effective bank's KEY LOCK mode.
    assert!(!callback.mixer.key_lock_for_measurement(0));
    // This independently derives only the actual selected frames from the PCM16
    // original; no productive decoder, seed bytes or whole-track array is used.
    let expected: Vec<f32> = [-16_384_i16, 32_767, 0, -4_096]
        .into_iter()
        .flat_map(|value| [f32::from(value) / 32_768.0; 2])
        .collect();
    assert_eq!(selected.samples.as_ref(), expected.as_slice());
    assert!(!Arc::ptr_eq(&selected.samples, &seed.samples));
    let selected_identity = &selected.residency.as_ref().unwrap().source;
    assert_eq!(
        selected_identity.as_ref(),
        seed.residency.as_ref().unwrap().source.as_ref()
    );
    let leases = engine.cold_leases.lock().unwrap();
    let lease = leases[0].as_ref().unwrap();
    assert_ne!(lease.assignment_id(), parent_assignment);
    assert_eq!(lease.cache_path, expected_cache);
    assert_eq!(
        lease.manifest.descriptor["playback"]["pcm"]["full_frames"],
        7
    );
    drop(leases);
    assert_eq!(fs::read(&original).unwrap(), protected);
    callback.assert_old_voice(&previous);
    preparation.release_preparation().unwrap();
    assert!(ticket.is_current().unwrap());
    callback.assert_old_voice(&previous);
}
