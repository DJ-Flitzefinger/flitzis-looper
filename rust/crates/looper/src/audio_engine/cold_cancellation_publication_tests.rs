//! Exact-incarnation cancellation and complete migration-hold publication.
#![cfg(windows)]

use super::*;
use crate::audio_engine::audio_stream::drain_control_messages;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
use crate::audio_engine::material_migration_control as migration;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, TriggerQuantization};
use std::fs;
use std::sync::{Condvar, mpsc};
use std::time::{Duration, Instant};

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "cold cancellation synchronization timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn terminal(engine: &AudioEngine, request: u64) -> LoaderEvent {
    let mut terminal = None;
    wait_until(|| {
        while let Ok(event) = engine
            .loader_rx
            .lock()
            .unwrap()
            .try_recv()
            .map(LoaderEvent::into_test_payload)
        {
            if matches!(&event, LoaderEvent::Success { request_id, .. } | LoaderEvent::Error { request_id, .. } if *request_id == request)
            {
                terminal = Some(event);
            }
        }
        terminal.is_some()
    });
    terminal.unwrap()
}

fn sample() -> SampleBuffer {
    SampleBuffer {
        channels: 2,
        samples: Arc::from([0.25; 32]),
        residency: None,
    }
}

fn cold_publication(
    ownership: &Arc<InputRuntimeOwnership>,
    id: usize,
    sample: &SampleBuffer,
    generation: u64,
) -> SourcePublication {
    let captured = ownership.slot_snapshot(id).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(
            captured,
            crate::audio_engine::content_lifetime::reserve().unwrap(),
        )
        .unwrap();
    let authority = ownership.next_authority(id).unwrap();
    ownership
        .publish_source_write(
            &write,
            sample,
            48_000,
            generation,
            authority,
            None,
            true,
            || true,
        )
        .unwrap()
}

fn ready_material(
    engine: &AudioEngine,
    root: &std::path::Path,
) -> migration::MaterialMigrationPreparation {
    fs::create_dir_all(root).unwrap();
    let path = root.join("old.wav");
    let values = [0_i16, 8_192, -16_384, 32_767];
    let data = (values.len() * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&96_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fs::write(&path, bytes).unwrap();
    let material =
        crate::audio_engine::material_migration::prepare_material(root, &path, 48_000, 2, &|| {
            false
        })
        .unwrap();
    migration::prepared_for_test(engine, material, root).unwrap()
}

fn with_pending_marker(ownership: &InputRuntimeOwnership, operation: impl FnOnce()) {
    with_pending_marker_at(ownership, 0, operation);
}

fn with_pending_marker_at(ownership: &InputRuntimeOwnership, id: usize, operation: impl FnOnce()) {
    let captured = ownership.slot_snapshot(id).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(captured, captured.lifetime())
        .unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let writer = scope.spawn(move || {
            ownership
                .commit_slot_write_fixed(
                    &write,
                    || true,
                    |_| {
                        entered_tx.send(()).unwrap();
                        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    },
                )
                .unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        operation();
        release_tx.send(()).unwrap();
        writer.join().unwrap();
    });
}

#[test]
fn cold_cancellation_before_worker_preparation_cannot_arm_or_advance_source_request() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    let engine = AudioEngine::new().unwrap();
    let preparation = ready_material(&engine, &root);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (entered_tx, entered_rx) = mpsc::channel();
    for _ in 0..crate::audio_engine::cold_jobs::WORKERS {
        let reservation = engine.cold_jobs.reserve().unwrap();
        let gate = gate.clone();
        let entered_tx = entered_tx.clone();
        engine
            .cold_jobs
            .submit(reservation, move || {
                entered_tx.send(()).unwrap();
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            })
            .unwrap();
    }
    for _ in 0..crate::audio_engine::cold_jobs::WORKERS {
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    }
    let request_before = engine.pad_request_ids.lock().unwrap()[0];
    let epoch_before = engine.prepared_source_epochs[0].load(Ordering::Acquire);
    let (producer, consumer) = rtrb::RingBuffer::new(4);
    let ticket = migration::adopt_for_format(
        &engine,
        0,
        &preparation,
        Arc::new(Mutex::new(producer)),
        (2, 48_000, root),
    )
    .unwrap();
    let cancelled = ticket.cancel_unclaimed();
    let phase = ticket.phase().unwrap();
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    assert!(cancelled);
    assert_eq!(phase, "rejected");
    assert!(matches!(
        terminal(&engine, ticket.request_id()),
        LoaderEvent::Error { .. }
    ));
    wait_until(|| engine.cold_loading[0].load(Ordering::Acquire) == 0);
    assert_eq!(consumer.slots(), 0);
    assert_eq!(engine.pad_request_ids.lock().unwrap()[0], request_before);
    assert_eq!(
        engine.prepared_source_epochs[0].load(Ordering::Acquire),
        epoch_before
    );
    assert!(engine.sample_cache.lock().unwrap()[0].is_none());
    assert_eq!(engine.input_runtime_ownership.content_lifetime(0), 0);
    assert_eq!(
        engine
            .input_runtime_ownership
            .cold_status(0, ticket.request_id()),
        None
    );
    preparation.abort_rejected().unwrap();
}

#[test]
fn cold_cancellation_after_key_move_rejects_original_row_and_preserves_equal_pcm_successor() {
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    let shared_pcm = sample();
    let first = cold_publication(&ownership, 0, &shared_pcm, 7);
    let last = cold_publication(&ownership, 215, &shared_pcm, 7);
    let pair = ownership
        .prepare_placement_pair(
            ownership.slot_snapshot(0).unwrap(),
            ownership.slot_snapshot(215).unwrap(),
        )
        .unwrap();
    ownership
        .commit_placement_pair(&pair, true, || true, || {})
        .unwrap();
    let handle = Mutex::new(Some(first.clone()));
    let private = AtomicU8::new(0);
    let settled = AtomicBool::new(false);
    assert!(cancel_unclaimed_material(
        &Mutex::new(vec![0; 216]),
        &handle,
        &private,
        &settled,
        &ownership
    ));
    assert_eq!(private.load(Ordering::Acquire), 3);
    assert!(settled.load(Ordering::Acquire));
    assert_eq!(handle.lock().unwrap().as_ref().unwrap().key(), first.key());
    assert_eq!(ownership.cold_status(215, 7), Some(3));
    assert_eq!(ownership.cold_status(0, 7), Some(0));
    assert!(last.current(&shared_pcm, 48_000));
    assert!(
        matches!(ownership.observe_source_publication(&last), PlacementObservation::Current(snapshot) if snapshot.slot() == 0 && snapshot.content_key() == last.key())
    );
}

#[test]
fn cold_cancellation_pending_retains_ticket_and_prevents_terminal_or_preparation_rollback() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    let engine = AudioEngine::new().unwrap();
    let preparation = ready_material(&engine, &root);
    let original = preparation.material().unwrap().lease.original_path.clone();
    let cache_path = preparation.material().unwrap().lease.cache_path.clone();
    let (producer, consumer) = rtrb::RingBuffer::new(4);
    let ticket = migration::adopt_for_format(
        &engine,
        0,
        &preparation,
        Arc::new(Mutex::new(producer)),
        (2, 48_000, root),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok());
    let ControlMessage::LoadColdSample {
        publication,
        adoption,
        sample,
        ..
    } = consumer.peek().unwrap()
    else {
        panic!("actual cold command missing");
    };
    let captured_key = publication.key();
    let private = adoption.clone();
    let pending_pcm = sample.samples.clone();
    let request = engine.pad_request_ids.lock().unwrap()[0];
    let generation = engine.loaded_source_generations.lock().unwrap()[0];
    with_pending_marker(&engine.input_runtime_ownership, || {
        assert!(!ticket.cancel_unclaimed());
        assert_eq!(private.load(Ordering::Acquire), 0);
        assert_eq!(ticket.phase().unwrap(), "pending");
        // A private rejection cannot retire a queued incarnation while Native cleanup
        // is excluded by somebody else's marker; both worker and caller must retry.
        private.store(3, Ordering::Release);
        assert!(!ticket.cancel_unclaimed());
        assert_eq!(ticket.phase().unwrap(), "pending");
        assert!(preparation.abort_rejected().is_err());
        let deadline = Instant::now() + Duration::from_millis(15);
        while Instant::now() < deadline {
            while let Ok(event) = engine
                .loader_rx
                .lock()
                .unwrap()
                .try_recv()
                .map(LoaderEvent::into_test_payload)
            {
                assert!(!matches!(
                    event,
                    LoaderEvent::Success { .. } | LoaderEvent::Error { .. }
                ));
            }
            std::thread::yield_now();
        }
        assert_eq!(engine.pad_request_ids.lock().unwrap()[0], request);
        assert_eq!(
            engine.loaded_source_generations.lock().unwrap()[0],
            generation
        );
        assert!(Arc::ptr_eq(
            &engine.sample_cache.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .samples,
            &pending_pcm
        ));
        assert!(original.is_file());
        assert!(cache_path.join("playback.f32le").is_file());
        let ControlMessage::LoadColdSample {
            publication,
            adoption,
            ..
        } = consumer.peek().unwrap()
        else {
            panic!("ticket payload lost");
        };
        assert_eq!(publication.key(), captured_key);
        assert!(Arc::ptr_eq(adoption, &private));
    });
    assert!(ticket.cancel_unclaimed());
    assert!(matches!(
        terminal(&engine, ticket.request_id()),
        LoaderEvent::Error { .. }
    ));
    wait_until(|| engine.cold_loading[0].load(Ordering::Acquire) == 0);
    assert_eq!(ticket.phase().unwrap(), "rejected");
    assert!(engine.sample_cache.lock().unwrap()[0].is_none());
}

#[test]
fn cold_cancellation_preserves_private_and_native_claim_or_ack_winners() {
    for (private_phase, native_phase) in [(1, 0), (2, 0), (0, 1), (0, 2)] {
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        let source = sample();
        let publication = cold_publication(&ownership, 0, &source, 7);
        if native_phase != 0 {
            assert!(ownership.claim_cold_publication(&publication));
            if native_phase == 2 {
                assert!(ownership.finish_cold_publication(&publication, true, true));
            }
        }
        let private = AtomicU8::new(private_phase);
        let settled = AtomicBool::new(false);
        assert!(!cancel_unclaimed_material(
            &Mutex::new(vec![0; 216]),
            &Mutex::new(Some(publication.clone())),
            &private,
            &settled,
            &ownership
        ));
        assert_eq!(private.load(Ordering::Acquire), private_phase);
        assert_eq!(ownership.cold_status(0, 7), Some(native_phase));
        assert!(!settled.load(Ordering::Acquire));
        assert!(ownership.source_publication_identity_current(&publication, &source, 48_000));
    }
}

#[test]
fn cold_cancellation_retired_key_never_cancels_equal_pcm_and_generation_aba_birth() {
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    let shared_pcm = sample();
    let retired = cold_publication(&ownership, 0, &shared_pcm, 7);
    let successor = cold_publication(&ownership, 0, &shared_pcm, 7);
    assert_ne!(retired.key(), successor.key());
    let private = AtomicU8::new(0);
    let settled = AtomicBool::new(false);
    assert!(cancel_unclaimed_material(
        &Mutex::new(vec![0; 216]),
        &Mutex::new(Some(retired)),
        &private,
        &settled,
        &ownership
    ));
    assert_eq!(private.load(Ordering::Acquire), 3);
    assert_eq!(ownership.cold_status(0, 7), Some(0));
    assert!(ownership.source_publication_identity_current(&successor, &shared_pcm, 48_000));
}

#[test]
fn cold_cancellation_acknowledged_migration_keeps_actual_source_and_complete_artifacts() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    let engine = AudioEngine::new().unwrap();
    let preparation = ready_material(&engine, &root);
    let material = preparation.material().unwrap();
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let ticket = migration::adopt_for_format(
        &engine,
        0,
        &preparation,
        Arc::new(Mutex::new(producer)),
        (2, 48_000, root),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok());
    let mut mixer = RtMixer::new(2, 48_000.0);
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    let mut mode = TriggerQuantization::Immediate;
    let mut transport = TransportTimeline::new(48_000);
    let mut feedback: Vec<AudioMessage> = Vec::new();
    assert_eq!(
        drain_control_messages(
            &mut consumer,
            &mut scheduler,
            0,
            &mut mode,
            &mut transport,
            &mut mixer,
            &mut feedback,
            &mut ImmediateAudioBufferRetirement
        ),
        1
    );
    assert!(matches!(
        terminal(&engine, ticket.request_id()),
        LoaderEvent::Success { .. }
    ));
    assert_eq!(ticket.phase().unwrap(), "acknowledged");
    assert!(!ticket.cancel_unclaimed());
    assert!(ticket.is_current().unwrap());
    assert!(preparation.abort_rejected().is_err());
    assert_eq!(
        engine
            .input_runtime_ownership
            .cold_status(0, ticket.request_id()),
        Some(2)
    );
    assert!(material.lease.original_path.is_file());
    assert!(material.lease.cache_path.join("playback.f32le").is_file());
    let current = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    assert!(Arc::ptr_eq(
        &mixer.bank_for_measurement()[0].as_ref().unwrap().samples,
        &current.samples
    ));
}

#[test]
fn cold_acknowledged_worker_retries_unrelated_pending_marker_before_own_metadata_success() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    let engine = AudioEngine::new().unwrap();
    let preparation = ready_material(&engine, &root);
    let material = preparation.material().unwrap();
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let ticket = migration::adopt_for_format(
        &engine,
        0,
        &preparation,
        Arc::new(Mutex::new(producer)),
        (2, 48_000, root),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok());
    let mut mixer = RtMixer::new(2, 48_000.0);
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    let mut mode = TriggerQuantization::Immediate;
    let mut transport = TransportTimeline::new(48_000);
    let mut feedback: Vec<AudioMessage> = Vec::new();
    // The productive callback ACKs without this control lock. Retaining it places
    // the unrelated publication fence strictly before this worker's metadata read.
    let request_guard = engine.pad_request_ids.lock().unwrap();
    assert_eq!(
        drain_control_messages(
            &mut consumer,
            &mut scheduler,
            0,
            &mut mode,
            &mut transport,
            &mut mixer,
            &mut feedback,
            &mut ImmediateAudioBufferRetirement,
        ),
        1
    );
    assert_eq!(ticket.phase().unwrap(), "acknowledged");
    let acknowledged_pcm = mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap()
        .samples
        .clone();
    with_pending_marker_at(&engine.input_runtime_ownership, 215, || {
        drop(request_guard);
        let deadline = Instant::now() + Duration::from_millis(20);
        while Instant::now() < deadline {
            while let Ok(event) = engine
                .loader_rx
                .lock()
                .unwrap()
                .try_recv()
                .map(LoaderEvent::into_test_payload)
            {
                assert!(
                    !matches!(
                        event,
                        LoaderEvent::Success { .. } | LoaderEvent::Error { .. }
                    ),
                    "Pending delivered a terminal source outcome: {event:?}"
                );
            }
            std::thread::yield_now();
        }
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
            &acknowledged_pcm
        ));
        assert!(engine.cold_leases.lock().unwrap()[0].is_none());
        assert_eq!(engine.cold_lease_generations[0].load(Ordering::Acquire), 0);
        assert_ne!(engine.cold_loading[0].load(Ordering::Acquire), 0);
        assert!(material.lease.original_path.is_file());
        assert!(material.lease.cache_path.join("playback.f32le").is_file());
        assert!(!material.lease.assignment_retired());
        // The worker must release its control locks between Pending retries.
        wait_until(|| engine.pad_request_ids.try_lock().is_ok());
        wait_until(|| engine.sample_cache.try_lock().is_ok());
        wait_until(|| engine.loaded_source_generations.try_lock().is_ok());
    });
    let event = terminal(&engine, ticket.request_id());
    assert!(
        matches!(event, LoaderEvent::Success { id: 0, request_id, .. }
        if request_id == ticket.request_id())
    );
    wait_until(|| engine.cold_loading[0].load(Ordering::Acquire) == 0);
    assert!(ticket.is_current().unwrap());
    assert_eq!(
        engine.cold_lease_generations[0].load(Ordering::Acquire),
        ticket.request_id()
    );
    assert!(engine.cold_leases.lock().unwrap()[0].is_some());
    assert!(material.lease.original_path.is_file());
    assert!(material.lease.cache_path.join("playback.f32le").is_file());
}

#[test]
fn cold_claim_timeout_preserves_real_native_ack_before_final_private_ack_store() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    let engine = AudioEngine::new().unwrap();
    let preparation = ready_material(&engine, &root);
    let material = preparation.material().unwrap();
    let gate = gate_next_adoption_wait();
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let ticket = migration::adopt_for_format(
        &engine,
        0,
        &preparation,
        Arc::new(Mutex::new(producer)),
        (2, 48_000, root),
    )
    .unwrap();
    wait_until(|| gate.0.entered.load(Ordering::Acquire));
    let ControlMessage::LoadColdSample {
        publication,
        adoption,
        sample,
        ..
    } = consumer.peek().unwrap()
    else {
        panic!("productive cold command missing");
    };
    let publication = publication.clone();
    let private = adoption.clone();
    let source = sample.clone();
    let mut mixer = RtMixer::new(2, 48_000.0);
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    let mut mode = TriggerQuantization::Immediate;
    let mut transport = TransportTimeline::new(48_000);
    let mut feedback: Vec<AudioMessage> = Vec::new();
    assert_eq!(
        drain_control_messages(
            &mut consumer,
            &mut scheduler,
            0,
            &mut mode,
            &mut transport,
            &mut mixer,
            &mut feedback,
            &mut ImmediateAudioBufferRetirement,
        ),
        1
    );
    assert_eq!(private.load(Ordering::Acquire), 2);
    assert_eq!(
        engine
            .input_runtime_ownership
            .cold_status(0, ticket.request_id()),
        Some(2)
    );
    assert!(!engine.input_runtime_ownership.placement_pending());
    // The real callback already owns the source/Native ACK. Reconstruct only the
    // reachable gap between closing that marker and its last private2 store while
    // the per-work gate proves await_adoption has not observed private2 yet.
    private.store(1, Ordering::Release);
    engine.cold_cancelled.store(true, Ordering::Release);
    drop(gate);
    let mut incomplete_diagnostic = false;
    wait_until(|| {
        while let Ok(event) = engine
            .loader_rx
            .lock()
            .unwrap()
            .try_recv()
            .map(LoaderEvent::into_test_payload)
        {
            assert!(
                !matches!(
                    event,
                    LoaderEvent::Success { .. } | LoaderEvent::Error { .. }
                ),
                "incomplete private ACK terminalized the actual Native winner: {event:?}"
            );
            if let LoaderEvent::Progress { stage, .. } = event
                && stage.contains("cold callback acknowledgement incomplete; artifacts retained")
            {
                incomplete_diagnostic = true;
            }
        }
        incomplete_diagnostic
    });
    wait_until(|| engine.cold_loading[0].load(Ordering::Acquire) == 0);
    assert_eq!(ticket.phase().unwrap(), "claimed");
    assert!(!ticket.cancel_unclaimed());
    assert!(preparation.abort_rejected().is_err());
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[0],
        (ticket.request_id(), 48_000)
    );
    assert_eq!(
        engine
            .input_runtime_ownership
            .cold_status(0, ticket.request_id()),
        Some(2)
    );
    assert!(
        engine
            .input_runtime_ownership
            .source_publication_identity_current(&publication, &source, 48_000)
    );
    assert!(Arc::ptr_eq(
        &mixer.bank_for_measurement()[0].as_ref().unwrap().samples,
        &source.samples
    ));
    assert!(material.lease.original_path.is_file());
    assert!(material.lease.cache_path.join("playback.f32le").is_file());
    assert!(!material.lease.assignment_retired());
    // The callback's eventual last store preserves its existing Native/source truth.
    private.store(2, Ordering::Release);
    assert_eq!(ticket.phase().unwrap(), "acknowledged");
    assert!(ticket.is_current().unwrap());
    assert!(!ticket.cancel_unclaimed());
    assert!(
        engine
            .input_runtime_ownership
            .source_publication_identity_current(&publication, &source, 48_000)
    );
    assert_eq!(
        engine
            .input_runtime_ownership
            .cold_status(0, ticket.request_id()),
        Some(2)
    );
}

#[test]
fn migration_bulk_holds_later_owner_mismatch_cannot_partially_acquire_or_release() {
    let engine = AudioEngine::new().unwrap();
    let ownership = &engine.input_runtime_ownership;
    let (producer, _consumer) = rtrb::RingBuffer::new(4);
    let producer = Arc::new(Mutex::new(producer));
    ownership.mark_launch_admitted(0);
    ownership.commit_migration_holds(&[215], 44, true).unwrap();
    let first_revision = ownership.launch_revision(0);
    assert!(migration::hold_for_producer(&engine, vec![0, 215], &producer).is_err());
    assert_eq!(ownership.migration_hold(0), 0);
    assert_eq!(ownership.launch_revision(0), first_revision);
    assert!(ownership.admitted_launch_ids().contains(&0));
    ownership.commit_migration_holds(&[215], 44, false).unwrap();
    let hold = migration::hold_for_producer(&engine, vec![0, 215], &producer).unwrap();
    let owner = ownership.migration_hold(0);
    let held_revision = ownership.launch_revision(0);
    ownership
        .commit_migration_holds(&[215], owner, false)
        .unwrap();
    ownership.commit_migration_holds(&[215], 99, true).unwrap();
    assert!(hold.release().is_err());
    assert_eq!(ownership.migration_hold(0), owner);
    assert_eq!(ownership.migration_hold(215), 99);
    assert_eq!(ownership.launch_revision(0), held_revision);
    ownership
        .commit_migration_holds(&[0], owner, false)
        .unwrap();
    ownership.commit_migration_holds(&[215], 99, false).unwrap();
}

#[test]
fn cold_worker_panic_settles_exact_unclaimed_publication_before_artifact_retirement() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    let engine = AudioEngine::new().unwrap();
    let preparation = ready_material(&engine, &root);
    let material = preparation.material().unwrap();
    let publication = cold_publication(&engine.input_runtime_ownership, 0, &material.sample, 7);
    let cleanup = ColdPublicationCleanup {
        adoption: Arc::new(AtomicU8::new(0)),
        publication: Arc::new(Mutex::new(Some(publication))),
        cancellation_settled: Arc::new(AtomicBool::new(false)),
        requests: engine.pad_request_ids.clone(),
        ownership: engine.input_runtime_ownership.clone(),
        lease: Arc::new(Mutex::new(Some(material.lease.clone()))),
        assets: engine.project_assets.clone(),
    };
    let held = PublicationLease {
        lease: material.lease.clone(),
        adopted: false,
        assets: engine.project_assets.clone(),
        _pcm: material.sample.samples.clone(),
        cleanup: cleanup.clone(),
    };
    let (sender, receiver) = mpsc::channel();
    run_worker_catching_panic_captured(cleanup.clone(), &sender, 0, 7, || {
        let _held = held;
        panic!("injected publication panic");
    });
    assert_eq!(cleanup.adoption.load(Ordering::Acquire), 3);
    assert!(cleanup.cancellation_settled.load(Ordering::Acquire));
    assert_eq!(engine.input_runtime_ownership.cold_status(0, 7), Some(3));
    assert!(material.lease.assignment_retired());
    assert!(
        matches!(receiver.try_recv().unwrap(), LoaderEvent::Error { error, .. }
        if error.contains("injected publication panic") && error.contains("native rejection settled"))
    );
}

#[test]
fn cold_worker_panic_retains_claimed_acknowledged_or_pending_publication_and_artifacts() {
    for phase in [0, 1, 2] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("samples");
        let engine = AudioEngine::new().unwrap();
        let preparation = ready_material(&engine, &root);
        let material = preparation.material().unwrap();
        let publication = cold_publication(&engine.input_runtime_ownership, 0, &material.sample, 7);
        if phase != 0 {
            assert!(
                engine
                    .input_runtime_ownership
                    .claim_cold_publication(&publication)
            );
            if phase == 2 {
                assert!(engine.input_runtime_ownership.finish_cold_publication(
                    &publication,
                    true,
                    true
                ));
            }
        }
        let key = publication.key();
        let cleanup = ColdPublicationCleanup {
            adoption: Arc::new(AtomicU8::new(phase)),
            publication: Arc::new(Mutex::new(Some(publication))),
            cancellation_settled: Arc::new(AtomicBool::new(false)),
            requests: engine.pad_request_ids.clone(),
            ownership: engine.input_runtime_ownership.clone(),
            lease: Arc::new(Mutex::new(Some(material.lease.clone()))),
            assets: engine.project_assets.clone(),
        };
        let held = PublicationLease {
            lease: material.lease.clone(),
            adopted: false,
            assets: engine.project_assets.clone(),
            _pcm: material.sample.samples.clone(),
            cleanup: cleanup.clone(),
        };
        let (sender, receiver) = mpsc::channel();
        let operation = || {
            run_worker_catching_panic_captured(cleanup.clone(), &sender, 0, 7, || {
                let _held = held;
                panic!("injected unresolved publication panic");
            });
            assert_eq!(cleanup.adoption.load(Ordering::Acquire), phase);
            assert!(!cleanup.cancellation_settled.load(Ordering::Acquire));
            assert!(!material.lease.assignment_retired());
            assert!(material.lease.original_path.is_file());
            assert!(material.lease.cache_path.join("playback.f32le").is_file());
            assert_eq!(
                cleanup.publication.lock().unwrap().as_ref().unwrap().key(),
                key
            );
            assert!(
                matches!(receiver.try_recv().unwrap(), LoaderEvent::Progress { stage, .. }
                if stage.contains("injected unresolved publication panic")
                    && stage.contains("permission unresolved or adopted"))
            );
        };
        if phase == 0 {
            with_pending_marker(&engine.input_runtime_ownership, operation);
            assert!(cleanup.cancel_unclaimed());
            cleanup.rollback_rejected_artifacts();
            assert!(material.lease.assignment_retired());
        } else {
            operation();
            assert_eq!(
                engine.input_runtime_ownership.cold_status(0, 7),
                Some(phase)
            );
        }
    }
}
