//! Genuine cold-worker cancellation probes and callback ACKs, without devices.
#![cfg(windows)]

use super::*;
use crate::audio_engine::audio_stream::drain_control_messages;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::resident_relocation::control_publication_tests::PendingWriter;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, TriggerQuantization};
use std::cell::RefCell;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(super) type BeforePrepareProbe = Box<dyn FnOnce(&ColdLoad) + Send>;

thread_local! {
    static BEFORE_PREPARE: RefCell<Option<BeforePrepareProbe>> = const { RefCell::new(None) };
    static BEFORE_PUBLICATION: RefCell<Option<BeforePrepareProbe>> = const { RefCell::new(None) };
}

pub(super) fn take_before_prepare_probe() -> Option<BeforePrepareProbe> {
    BEFORE_PREPARE.with(|slot| slot.borrow_mut().take())
}

pub(super) fn take_before_publication_probe() -> Option<BeforePrepareProbe> {
    BEFORE_PUBLICATION.with(|slot| slot.borrow_mut().take())
}

pub(super) fn probe_next_preparation(probe: impl FnOnce(&ColdLoad) + Send + 'static) {
    BEFORE_PREPARE.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "cold preparation probe already installed"
        );
        *slot.borrow_mut() = Some(Box::new(probe));
    });
}

pub(super) fn probe_next_publication(probe: impl FnOnce(&ColdLoad) + Send + 'static) {
    BEFORE_PUBLICATION.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "cold publication probe already installed"
        );
        *slot.borrow_mut() = Some(Box::new(probe));
    });
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "cold preparation probe timed out"
        );
        std::thread::yield_now();
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

fn fixture(
    probe: impl FnOnce(&ColdLoad) + Send + 'static,
) -> (
    tempfile::TempDir,
    AudioEngine,
    rtrb::Consumer<ControlMessage>,
    u64,
) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("probe.wav");
    let values = [0_i16, 8_192, -16_384, 32_767, 0, -4_096, 16_384];
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
    std::fs::write(&source, bytes).unwrap();
    let engine = AudioEngine::new().unwrap();
    let (producer, consumer) = rtrb::RingBuffer::new(4);
    probe_next_preparation(probe);
    let request = admit_for_format(
        &engine,
        0,
        source.to_string_lossy().into_owned(),
        false,
        false,
        true,
        Arc::new(Mutex::new(producer)),
        2,
        48_000,
        directory.path().join("samples"),
    )
    .unwrap();
    (directory, engine, consumer, request)
}

fn acknowledge(engine: &AudioEngine, consumer: &mut rtrb::Consumer<ControlMessage>) -> RtMixer {
    let mut mixer = RtMixer::new(2, 48_000.0);
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    let mut mode = TriggerQuantization::Immediate;
    let mut transport = TransportTimeline::new(48_000);
    let mut feedback: Vec<AudioMessage> = Vec::new();
    assert_eq!(
        drain_control_messages(
            consumer,
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
    mixer
}

#[test]
fn cold_pending_marker_keeps_original_preparation_and_policy_switches_reach_real_ack() {
    let (_directory, engine, mut consumer, request) = fixture(|work| {
        assert!(!work.is_cancelled());
        let held = PendingWriter::hold(work.ownership.clone());
        assert!(work.ownership.placement_pending());
        assert!(
            !work.flags_cancelled(),
            "unrelated Pending is not a definitive source cancellation"
        );
        assert!(!work.is_cancelled());
        held.complete();
        for behavior in [
            super::super::key_lock_start_policy::KeyLockStartBehavior::AttackAtGrid,
            super::super::key_lock_start_policy::KeyLockStartBehavior::FirstSoundAtGrid,
        ] {
            assert!(work.ownership.set_key_lock_start_behavior(behavior));
            assert!(!work.is_cancelled());
        }
    });
    wait_until(|| consumer.peek().is_ok() || engine.cold_loading[0].load(Ordering::Acquire) == 0);
    assert!(
        consumer.peek().is_ok(),
        "original cold worker never published"
    );
    let mut mixer = acknowledge(&engine, &mut consumer);
    assert!(matches!(
        terminal(&engine, request),
        LoaderEvent::Success { .. }
    ));
    assert!(mixer.bank_for_measurement()[0].is_some());
    assert!(
        mixer.play_sample(0, 1.0),
        "acknowledged pad remains startable"
    );
}

#[test]
fn cold_prepared_original_waits_for_pending_publication_without_advancing_authority() {
    let (sender, receiver) = mpsc::channel();
    probe_next_publication(move |work| {
        let held = PendingWriter::hold(work.ownership.clone());
        sender.send(held).unwrap();
    });
    let (_directory, engine, mut consumer, request) = fixture(|_| {});
    let held = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
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
                "Pending publication delivered a terminal original-source event: {event:?}"
            );
        }
        assert!(consumer.peek().is_err());
        assert!(engine.sample_cache.lock().unwrap()[0].is_none());
        assert_eq!(engine.pad_request_ids.lock().unwrap()[0], 0);
        assert_eq!(engine.prepared_source_epochs[0].load(Ordering::Acquire), 1);
        std::thread::yield_now();
    }
    // Waiting must release every control lock, rather than obstruct the writer,
    // shutdown, or a genuine source replacement while retaining prepared PCM.
    wait_until(|| engine.pad_request_ids.try_lock().is_ok());
    wait_until(|| engine.sample_cache.try_lock().is_ok());
    wait_until(|| engine.loaded_source_generations.try_lock().is_ok());
    held.complete();
    wait_until(|| consumer.peek().is_ok());
    let mut mixer = acknowledge(&engine, &mut consumer);
    assert!(matches!(
        terminal(&engine, request),
        LoaderEvent::Success { .. }
    ));
    assert!(mixer.play_sample(0, 1.0));
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[0],
        (request, 48_000)
    );
}

#[test]
fn cold_pending_publication_shutdown_rejects_original_without_arming() {
    let (sender, receiver) = mpsc::channel();
    probe_next_publication(move |work| {
        sender
            .send(PendingWriter::hold(work.ownership.clone()))
            .unwrap();
    });
    let (_directory, engine, mut consumer, request) = fixture(|_| {});
    let held = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    engine.cold_cancelled.store(true, Ordering::Release);
    assert!(matches!(
        terminal(&engine, request),
        LoaderEvent::Error { .. }
    ));
    assert!(consumer.peek().is_err());
    assert_eq!(engine.pad_request_ids.lock().unwrap()[0], 0);
    assert_eq!(engine.prepared_source_epochs[0].load(Ordering::Acquire), 1);
    assert!(engine.sample_cache.lock().unwrap()[0].is_none());
    held.complete();
}

#[test]
fn cold_pending_marker_does_not_mask_shutdown_adoption_proposal_epoch_or_request_cancellation() {
    for reason in 0..5 {
        let (_directory, engine, mut consumer, request) = fixture(move |work| {
            let held = PendingWriter::hold(work.ownership.clone());
            match reason {
                0 => work.cancelled.store(true, Ordering::Release),
                1 => work.adoption.store(3, Ordering::Release),
                2 => work.proposal.cancel(),
                3 => {
                    work.epoch.fetch_add(1, Ordering::AcqRel);
                }
                4 => work.requests.lock().unwrap()[work.id] += 1,
                _ => unreachable!(),
            }
            assert!(work.is_cancelled(), "real cancellation {reason} was hidden");
            held.complete();
        });
        assert!(matches!(
            terminal(&engine, request),
            LoaderEvent::Error { .. }
        ));
        assert!(consumer.peek().is_err());
        assert!(engine.sample_cache.lock().unwrap()[0].is_none());
    }
}

#[test]
fn cold_preparation_rejects_actual_replacement_of_its_original_slot() {
    let (_directory, engine, mut consumer, request) = fixture(|work| {
        let replacement = SampleBuffer {
            samples: Arc::from([0.5_f32; 32]),
            channels: 2,
            residency: None,
        };
        work.ownership
            .publish_test_source(0, &replacement, 48_000, 77);
        assert!(work.flags_cancelled());
    });
    assert!(matches!(
        terminal(&engine, request),
        LoaderEvent::Error { .. }
    ));
    assert!(consumer.peek().is_err());
}
