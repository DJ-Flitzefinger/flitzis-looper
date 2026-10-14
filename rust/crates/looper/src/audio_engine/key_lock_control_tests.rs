//! Actual control admission and callback effects retain fixed empty/source identity.

use super::*;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::cold_residency::{ResidentLoadGuard, ResidentLoadHint};
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, SampleBuffer, TriggerQuantization};
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use rtrb::{Consumer, RingBuffer};
use std::cell::Cell;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Barrier, Mutex};

const ID: usize = 0;
const OTHER: usize = NUM_SAMPLES - 1;
const RATE: u32 = 48_000;
const GENERATION: u64 = 7;

struct Fixture {
    engine: AudioEngine,
    controls: Consumer<ControlMessage>,
    guard: Arc<ResidentLoadGuard>,
}

impl Fixture {
    fn new(loaded: bool, capacity: usize) -> Self {
        Python::initialize();
        let mut engine = AudioEngine::new().unwrap();
        let (producer, controls) = RingBuffer::new(capacity);
        let (_, feedback) = RingBuffer::<AudioMessage>::new(8);
        engine.test_control_channels = Some((
            Arc::new(Mutex::new(producer)),
            Arc::new(Mutex::new(feedback)),
        ));
        let guard = Arc::new(ResidentLoadGuard {
            request: 19,
            hint: ResidentLoadHint {
                start_s: 0.0,
                end_s: 1.0,
                key_lock: false,
            },
            rate: RATE,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        engine.cold_loading[ID].store(guard.request, Ordering::Release);
        engine.resident_restore_guards.lock().unwrap()[ID] = Some(guard.clone());
        let fixture = Self {
            engine,
            controls,
            guard,
        };
        if loaded {
            fixture.publish(ID, 0.25, GENERATION);
        }
        fixture
    }

    fn publish(&self, id: usize, value: f32, generation: u64) -> SampleBuffer {
        let sample = SampleBuffer {
            samples: Arc::from([value; 128]),
            channels: 1,
            residency: None,
        }
        .with_complete_source(RATE);
        let mut requests = self.engine.pad_request_ids.lock().unwrap();
        let ownership = &self.engine.input_runtime_ownership;
        let captured = ownership.slot_snapshot(id).unwrap();
        let lifetime = crate::audio_engine::content_lifetime::reserve().unwrap();
        let write = ownership
            .prepare_slot_write_fixed(captured, lifetime)
            .unwrap();
        ownership
            .publish_source_write(
                &write,
                &sample,
                RATE,
                generation,
                ownership.next_authority(id).unwrap(),
                Some(TimingIntent::Manual),
                false,
                || true,
            )
            .unwrap();
        requests[id] = generation;
        self.engine.sample_cache.lock().unwrap()[id] = Some(sample.clone());
        self.engine.loaded_source_generations.lock().unwrap()[id] = (generation, RATE);
        self.engine.loaded_source_digests.lock().unwrap()[id] = Some(format!("{generation:064x}"));
        self.engine.timing_intents.lock().unwrap()[id] = TimingIntent::Manual;
        sample
    }

    fn request(&self) -> u64 {
        let ownership = &self.engine.input_runtime_ownership;
        ownership
            .key_lock_request_for_cell(ownership.slot_snapshot(ID).unwrap().cell())
            .load(Ordering::Acquire)
    }

    fn mixer(&self) -> RtMixer {
        let mut mixer = RtMixer::new(1, RATE as f32);
        mixer.set_input_runtime_ownership(self.engine.input_runtime_ownership.clone());
        mixer.set_current_timing_acknowledgements(
            self.engine.current_timing_acknowledgements.clone(),
        );
        for id in [ID, OTHER] {
            if let Some(sample) = &self.engine.sample_cache.lock().unwrap()[id] {
                mixer.load_sample(id, sample.clone());
            }
        }
        mixer
    }
}

fn dispatch(mixer: &mut RtMixer, message: ControlMessage) {
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    let mut transport = TransportTimeline::new(RATE);
    let mut quantization = TriggerQuantization::Immediate;
    let (mut feedback, _) = RingBuffer::<AudioMessage>::new(8);
    crate::audio_engine::audio_stream::process_control_message(
        message,
        &mut scheduler,
        0,
        &mut quantization,
        &mut transport,
        mixer,
        &mut feedback,
        &mut ImmediateAudioBufferRetirement,
    );
}

#[test]
fn loaded_key_lock_publication_claim_uses_captured_cell_and_exact_request() {
    let mut fixture = Fixture::new(true, 8);
    let ownership = &fixture.engine.input_runtime_ownership;
    let before = ownership.slot_snapshot(ID).unwrap();
    ownership
        .key_lock_request_for_cell(before.cell())
        .store(41, Ordering::Release);
    assert_eq!(
        enqueue_with_source(&fixture.engine, ID, true, Some(GENERATION)).unwrap(),
        42
    );
    let after = ownership.slot_snapshot(ID).unwrap();
    assert!(after.same_projection(before));
    assert_eq!(after.layout_revision(), before.layout_revision() + 2);
    assert_eq!(fixture.request(), 42);
    assert!(fixture.guard.cancelled.load(Ordering::Acquire));
    let message = fixture.controls.pop().unwrap();
    assert!(matches!(&message, ControlMessage::SetPadKeyLockRequest {
        id: ID, enabled: true, request_id: 42, binding, source_generation: GENERATION,
    } if binding.placement.unwrap().same_projection(before)));
    let mut mixer = fixture.mixer();
    dispatch(&mut mixer, message);
    assert!(mixer.key_lock_for_measurement(ID));
    let status = ownership.key_lock_status(ID).unwrap();
    assert_eq!(status.request_id, 42);
    assert_eq!(status.source_generation, GENERATION);
    assert!(fixture.controls.is_empty());
}

#[test]
fn key_lock_pending_after_preparation_preserves_intent_and_request_then_retries() {
    for loaded in [false, true] {
        let mut fixture = Fixture::new(loaded, 8);
        let ownership = fixture.engine.input_runtime_ownership.clone();
        let before = ownership.slot_snapshot(ID).unwrap();
        let other = ownership.slot_snapshot(OTHER).unwrap();
        let write = ownership
            .prepare_slot_write_fixed(other, other.lifetime())
            .unwrap();
        let after_preparation = Barrier::new(2);
        let writer_is_odd = Barrier::new(2);
        let release_writer = Barrier::new(2);
        let reached = Cell::new(false);
        let result = std::thread::scope(|scope| {
            let writer = scope.spawn(|| {
                after_preparation.wait();
                ownership.commit_slot_write_fixed(
                    &write,
                    || true,
                    |_| {
                        writer_is_odd.wait();
                        release_writer.wait();
                    },
                )
            });
            let result = enqueue_with_source_before_commit(
                &fixture.engine,
                ID,
                true,
                loaded.then_some(GENERATION),
                || {
                    reached.set(true);
                    after_preparation.wait();
                    writer_is_odd.wait();
                },
            );
            release_writer.wait();
            writer.join().unwrap().unwrap();
            result
        });
        assert!(reached.get());
        assert!(result.is_err());
        assert!(ownership.slot_snapshot(ID).unwrap().same_projection(before));
        assert_eq!(fixture.request(), 0);
        assert!(!fixture.guard.cancelled.load(Ordering::Acquire));
        assert!(fixture.controls.is_empty());
        let expected = u64::from(loaded);
        assert_eq!(
            enqueue_with_source(&fixture.engine, ID, true, loaded.then_some(GENERATION)).unwrap(),
            expected
        );
        assert_eq!(fixture.request(), expected);
        assert!(fixture.guard.cancelled.load(Ordering::Acquire));
        assert!(fixture.controls.pop().is_ok());
        assert!(fixture.controls.is_empty());
    }
}

#[test]
fn key_lock_overflow_and_full_queue_fail_before_any_publication() {
    for overflow in [false, true] {
        let mut fixture = Fixture::new(true, 1);
        let ownership = &fixture.engine.input_runtime_ownership;
        let before = ownership.slot_snapshot(ID).unwrap();
        let previous = if overflow { u64::MAX } else { 13 };
        ownership
            .key_lock_request_for_cell(before.cell())
            .store(previous, Ordering::Release);
        if !overflow {
            fixture
                .engine
                .control_producer()
                .unwrap()
                .lock()
                .unwrap()
                .push(ControlMessage::Ping())
                .unwrap();
        }
        let result =
            enqueue_with_source_before_commit(&fixture.engine, ID, true, Some(GENERATION), || {
                panic!("failed preflight reached the claim seam")
            });
        assert!(result.is_err());
        assert_eq!(ownership.slot_snapshot(ID), Some(before));
        assert_eq!(fixture.request(), previous);
        assert!(!fixture.guard.cancelled.load(Ordering::Acquire));
        if !overflow {
            assert!(matches!(
                fixture.controls.pop().unwrap(),
                ControlMessage::Ping()
            ));
        }
        assert!(fixture.controls.is_empty());
    }
}

#[test]
fn key_lock_changed_request_or_restore_request_rejects_the_complete_claim() {
    for changed_key_request in [false, true] {
        let fixture = Fixture::new(true, 8);
        let ownership = &fixture.engine.input_runtime_ownership;
        let before = ownership.slot_snapshot(ID).unwrap();
        let result =
            enqueue_with_source_before_commit(&fixture.engine, ID, true, Some(GENERATION), || {
                if changed_key_request {
                    ownership
                        .key_lock_request_for_cell(before.cell())
                        .store(88, Ordering::Release);
                } else {
                    fixture.engine.cold_loading[ID].store(20, Ordering::Release);
                }
            });
        assert!(result.is_err());
        assert_eq!(ownership.slot_snapshot(ID), Some(before));
        assert_eq!(fixture.request(), if changed_key_request { 88 } else { 0 });
        assert!(!fixture.guard.cancelled.load(Ordering::Acquire));
        assert!(fixture.controls.is_empty());
    }
}

#[test]
fn key_lock_changed_source_projection_rejects_before_intent_or_request_publication() {
    for change in 0..3 {
        let fixture = Fixture::new(true, 8);
        let ownership = &fixture.engine.input_runtime_ownership;
        let before = ownership.slot_snapshot(ID).unwrap();
        let result =
            enqueue_with_source_before_commit(&fixture.engine, ID, true, Some(GENERATION), || {
                match change {
                    0 => {
                        ownership
                            .authority_for_cell(before.cell())
                            .fetch_add(1, Ordering::AcqRel);
                    }
                    1 => {
                        ownership.set_timing_intent_for_cell(before.cell(), TimingIntent::Tap);
                    }
                    _ => {
                        let source = fixture.engine.sample_cache.lock().unwrap()[ID]
                            .as_ref()
                            .unwrap()
                            .clone();
                        let window = source
                            .window(
                                0,
                                source.frame_count(),
                                source.window_revision() + 1,
                                crate::messages::ResidentContext::FullTrack,
                            )
                            .unwrap();
                        let write = ownership
                            .prepare_slot_write_fixed(before, before.lifetime())
                            .unwrap();
                        ownership
                            .commit_slot_write_fixed(
                                &write,
                                || true,
                                |cell| ownership.publish_window_for_cell(cell, &window),
                            )
                            .unwrap();
                    }
                }
            });
        assert!(result.is_err());
        assert!(ownership.slot_snapshot(ID).unwrap().same_projection(before));
        assert_eq!(fixture.request(), 0);
        assert!(!fixture.guard.cancelled.load(Ordering::Acquire));
        assert!(fixture.controls.is_empty());
    }
}

#[test]
fn key_lock_empty_capture_rejects_pending_instead_of_inventing_saved_intent() {
    let fixture = Fixture::new(false, 8);
    let ownership = &fixture.engine.input_runtime_ownership;
    let other = ownership.slot_snapshot(OTHER).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(other, other.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |_| {
                assert!(enqueue_with_source(&fixture.engine, ID, true, None).is_err());
                assert!(!fixture.guard.cancelled.load(Ordering::Acquire));
                assert!(fixture.controls.is_empty());
            },
        )
        .unwrap();
    assert_eq!(fixture.request(), 0);
}

#[test]
fn empty_saved_key_lock_rejects_replacement_and_empty_aba() {
    for empty_again in [false, true] {
        let mut fixture = Fixture::new(false, 8);
        assert_eq!(
            enqueue_with_source(&fixture.engine, ID, true, None).unwrap(),
            0
        );
        let message = fixture.controls.pop().unwrap();
        let captured = match &message {
            ControlMessage::SetPadKeyLockSavedIntent {
                placement,
                enabled: true,
            } => *placement,
            _ => panic!("empty admission must retain the captured empty slot"),
        };
        let mut mixer = fixture.mixer();
        let source = fixture.publish(ID, 0.5, GENERATION);
        mixer.load_sample(ID, source);
        if empty_again {
            let ownership = &fixture.engine.input_runtime_ownership;
            let loaded = ownership.slot_snapshot(ID).unwrap();
            let write = ownership.prepare_slot_write_fixed(loaded, 0).unwrap();
            ownership
                .retire_source_write(&write, ownership.next_authority(ID).unwrap(), || true)
                .unwrap();
            mixer.unload_sample(ID);
            let empty = ownership.slot_snapshot(ID).unwrap();
            assert_eq!(empty.lifetime(), 0);
            assert_eq!(empty.cell(), captured.cell());
            assert!(empty.binding_epoch() > captured.binding_epoch());
        }
        dispatch(&mut mixer, message);
        assert!(!mixer.key_lock_for_measurement(ID));
        assert_eq!(fixture.request(), 0);
        assert!(fixture.controls.is_empty());
    }
}

#[test]
fn admitted_empty_key_lock_effect_uses_fixed_fields_during_unrelated_pending() {
    let mut fixture = Fixture::new(false, 8);
    assert_eq!(
        enqueue_with_source(&fixture.engine, ID, true, None).unwrap(),
        0
    );
    let placement = match fixture.controls.pop().unwrap() {
        ControlMessage::SetPadKeyLockSavedIntent {
            placement,
            enabled: true,
        } => placement,
        _ => panic!("expected captured empty intent"),
    };
    let mut mixer = fixture.mixer();
    let ownership = &fixture.engine.input_runtime_ownership;
    let other = ownership.slot_snapshot(OTHER).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(other, other.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |_| {
                assert!(ownership.placement_pending());
                mixer.set_pad_key_lock_saved_intent(placement, true);
                assert!(mixer.key_lock_for_measurement(ID));
            },
        )
        .unwrap();
    assert!(mixer.key_lock_for_measurement(ID));
    assert!(ownership.source_cell_empty(placement.cell()));
    assert_eq!(fixture.request(), 0);
}

#[test]
fn actual_key_lock_dispatch_defers_same_payload_under_pending_and_retries() {
    for loaded in [false, true] {
        let mut fixture = Fixture::new(loaded, 8);
        enqueue_with_source(&fixture.engine, ID, true, loaded.then_some(GENERATION)).unwrap();
        let message = fixture.controls.pop().unwrap();
        let mut mixer = fixture.mixer();
        let ownership = &fixture.engine.input_runtime_ownership;
        let other = ownership.slot_snapshot(OTHER).unwrap();
        let write = ownership
            .prepare_slot_write_fixed(other, other.lifetime())
            .unwrap();
        ownership
            .commit_slot_write_fixed(
                &write,
                || true,
                |_| {
                    dispatch(&mut mixer, message);
                    assert!(!mixer.key_lock_for_measurement(ID));
                    assert!(mixer.deferred_control().is_some());
                },
            )
            .unwrap();
        let retained = mixer.take_deferred_control().unwrap();
        dispatch(&mut mixer, retained);
        assert!(mixer.key_lock_for_measurement(ID));
        assert!(mixer.deferred_control().is_none());
        assert_eq!(fixture.request(), u64::from(loaded));
    }
}

#[test]
fn loaded_key_lock_effect_follows_actual_content_swap_and_rejects_retirement() {
    for retire in [false, true] {
        let mut fixture = Fixture::new(true, 8);
        fixture.publish(OTHER, -0.5, GENERATION + 1);
        let mut mixer = fixture.mixer();
        mixer.set_pad_key_lock(ID, true);
        mixer.set_pad_key_lock(OTHER, true);
        assert!(mixer.key_lock_for_measurement(ID));
        assert!(mixer.key_lock_for_measurement(OTHER));
        assert_eq!(
            enqueue_with_source(&fixture.engine, ID, false, Some(GENERATION)).unwrap(),
            1
        );
        let message = fixture.controls.pop().unwrap();
        let ownership = &fixture.engine.input_runtime_ownership;
        let source = ownership.slot_snapshot(ID).unwrap();
        let target = ownership.slot_snapshot(OTHER).unwrap();
        let pair = ownership.prepare_placement_pair(source, target).unwrap();
        ownership
            .commit_placement_pair(
                &pair,
                true,
                || true,
                || mixer.swap_occupied_images_for_key_lock_test(ID, OTHER),
            )
            .unwrap();
        assert_eq!(
            ownership.callback_key_slot(source.content_key().unwrap()),
            Some(OTHER)
        );
        if retire {
            let captured = ownership.slot_snapshot(OTHER).unwrap();
            let write = ownership.prepare_slot_write_fixed(captured, 0).unwrap();
            ownership
                .retire_source_write(&write, ownership.next_authority(OTHER).unwrap(), || true)
                .unwrap();
            mixer.unload_sample(OTHER);
            // The numeric request and identical immutable PCM cannot revive a retired key.
            ownership
                .key_lock_request_for_cell(captured.cell())
                .store(1, Ordering::Release);
        }
        dispatch(&mut mixer, message);
        assert!(
            mixer.key_lock_for_measurement(ID),
            "new numeric-slot occupant stays untouched"
        );
        assert_eq!(mixer.key_lock_for_measurement(OTHER), retire);
        if !retire {
            let status = ownership.key_lock_status(OTHER).unwrap();
            assert_eq!(status.request_id, 1);
            assert_eq!(status.source_generation, GENERATION);
            assert!(!status.effective);
        }
    }
}
