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
        material_migration::prepare_material(root, &old_path, 48_000, 2, &|| false).unwrap();
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
        while let Ok(event) = engine.loader_rx.lock().unwrap().try_recv() {
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
fn material_migration_hold_rejects_queued_starts_and_release_never_revalidates_old_revision() {
    let engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    let mut callback = Callback::new(&engine, &previous);
    callback.mixer.stop_sample(0);
    let binding = input_runtime_binding::capture(&engine, 0)
        .unwrap()
        .unwrap()
        .binding;
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
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
    producer.lock().unwrap().push(start(old_revision)).unwrap();
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
    assert!(
        !callback
            .feedback
            .events
            .iter()
            .any(|event| matches!(event, AudioMessage::SampleStarted { .. }))
    );
    let fresh = engine.input_runtime_ownership.launch_revision(0);
    assert!(engine.input_runtime_ownership.launch_current(0, fresh));
    producer.lock().unwrap().push(start(fresh)).unwrap();
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(
        callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    assert!(
        callback
            .feedback
            .events
            .iter()
            .any(|event| matches!(event, AudioMessage::SampleStarted { id: 0 }))
    );
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
        mixer: RtMixer::new(1, 8_000.0),
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
    assert_eq!(engine.current_timing_acknowledgements.current_epoch(0), 0);
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
    assert_eq!(engine.current_timing_acknowledgements.current_epoch(0), 0);
    assert!(
        constant_timing::export_current(engine, 0, original.to_string_lossy().into())
            .unwrap()
            .is_none()
    );
    assert_eq!(callback.drain(consumer), 1);
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    let acknowledged = engine.current_timing_acknowledgements.current_epoch(0);
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
)
project = ProjectState(config_revision=17)
for sample_id, uuid, raw, correction, base, extra, retrigger in (
    (0, '1'*32, 'Em', 'Cm', -4, 12, True),
    (215, 'f'*32, 'Bb', 'unknown retained correction', 6, -18, False),
):
    project.sample_paths[sample_id] = alias.old_reference
    project.pad_content[sample_id] = PadContentIdentity(instance_id=uuid)
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
    assert snapshot.pad_content[sample_id].material_id is None
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
    'new_reference': alias.new_reference, 'cache_path': alias.cache_path,
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
    pyo3::Python::initialize();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let scratch = workspace.join("scratch/goal-p2a-material-migration-20261010");
    fs::create_dir_all(&scratch).unwrap();
    let directory = tempfile::tempdir_in(&scratch).unwrap();
    let root = directory.path().join("samples");
    fs::create_dir(&root).unwrap();
    let transaction = "77777777777777777777777777777777";
    let (fixture_original, envelope) = constant_timing::migration_test_fixture();
    let legacy = root.join("old.wav");
    let original_bytes = fs::read(&fixture_original).unwrap();
    fs::write(&legacy, &original_bytes).unwrap();
    let metadata;
    let encoded_record;
    // All source tickets, registrations, permits and PCM Arcs die before the child
    // process starts. Only verified original/cache files, journal and evidence remain.
    {
        let engine = Arc::new(AudioEngine::new().unwrap());
        let material =
            material_migration::prepare_material(&root, &legacy, 8_000, 1, &|| false).unwrap();
        metadata = material.metadata();
        let preparation = control::prepared_for_test(&engine, material, &root).unwrap();
        let (producer, mut consumer) = rtrb::RingBuffer::new(8);
        let producer = Arc::new(Mutex::new(producer));
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
    assert!(
        material["new_reference"]
            .as_str()
            .unwrap()
            .starts_with(&format!(
                "samples/materials/M{}/original/",
                material["material_id"].as_str().unwrap()
            ),)
    );
    let engine = AudioEngine::new().unwrap();
    assert!(engine.sample_cache.lock().unwrap()[0].is_none());
    assert!(engine.cold_leases.lock().unwrap()[0].is_none());
    assert_eq!(engine.loaded_source_generations.lock().unwrap()[0], (0, 0));
    assert_eq!(engine.prepared_source_epochs[0].load(Ordering::Acquire), 1);
    assert_eq!(engine.current_timing_acknowledgements.current_epoch(0), 0);
    assert!(engine.sample_cache.lock().unwrap()[215].is_none());
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[215],
        (0, 0)
    );
    assert_eq!(engine.current_timing_acknowledgements.current_epoch(215), 0);
    let mut callback = migration_callback_8000(&engine);
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    // Ordinary verified canonical restore, not migration preparation or a carried
    // parent subscriber. A new process allocates its own source/lease/permit.
    let admitted = admit_selected_material(
        &engine,
        0,
        original.to_string_lossy().into(),
        (false, false, false, None),
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
        (false, false, false, None),
        producer.clone(),
        (1, 8_000, root.clone()),
        "restore",
        None,
    )
    .unwrap();
    wait_until(|| consumer.slots() >= 2);
    assert_eq!(admitted.adoption.load(Ordering::Acquire), 0);
    assert_eq!(last_admitted.adoption.load(Ordering::Acquire), 0);
    assert_eq!(engine.current_timing_acknowledgements.current_epoch(0), 0);
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
    assert_eq!(
        fs::read(&original).unwrap(),
        fs::read(root.join("old.wav")).unwrap()
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
        }))
        .unwrap(),
    )
    .unwrap();
}

#[path = "material_migration_resume_tests.rs"]
mod material_migration_resume_tests;
