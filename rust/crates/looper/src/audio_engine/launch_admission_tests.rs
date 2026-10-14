//! Real source guards and captured launch admission, separated from fixed slot snapshots.
use super::*;
use crate::audio_engine::AudioEngine;
use crate::audio_engine::audio_stream::{execute_scheduled_command, process_control_message};
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::content_placement::SlotSnapshot;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::resident_relocation::control_publication_tests::PendingWriter;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, SampleBuffer, TriggerQuantization};
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use rtrb::{Consumer, RingBuffer};
use std::cell::RefCell;

pub(in crate::audio_engine) const RATE: u32 = 48_000;
pub(in crate::audio_engine) const OTHER: usize = 1;

pub(in crate::audio_engine) struct LaunchFixture {
    pub(in crate::audio_engine) engine: AudioEngine,
    pub(in crate::audio_engine) producer: Arc<Mutex<Producer<ControlMessage>>>,
    pub(in crate::audio_engine) consumer: Consumer<ControlMessage>,
    state: Arc<Mutex<RuntimeState>>,
}

impl LaunchFixture {
    pub(in crate::audio_engine) fn new(capacity: usize) -> Self {
        pyo3::Python::initialize();
        let engine = AudioEngine::new().unwrap();
        let (producer, consumer) = RingBuffer::new(capacity);
        let fixture = Self {
            engine,
            producer: Arc::new(Mutex::new(producer)),
            consumer,
            state: Arc::new(Mutex::new(RuntimeState::default())),
        };
        fixture.publish(0, 0.25, 7);
        fixture.publish(OTHER, -0.5, 8);
        let mut state = fixture.state.lock().unwrap();
        state.multi_loop = true;
        for id in [0, OTHER] {
            let binding = crate::audio_engine::input_runtime_binding::capture(&fixture.engine, id)
                .unwrap()
                .unwrap();
            state.pads[id] = RuntimePadState {
                loaded: true,
                loop_start_s: 16.0 / f64::from(RATE),
                loop_end_s: Some(96.0 / f64::from(RATE)),
                binding: Some(binding.binding),
            };
        }
        drop(state);
        fixture
    }

    pub(in crate::audio_engine) fn publish(&self, id: usize, value: f32, generation: u64) {
        let sample = SampleBuffer {
            samples: Arc::from([value; 128]),
            channels: 1,
            residency: None,
        }
        .with_complete_source(RATE);
        let ownership = &self.engine.input_runtime_ownership;
        let captured = ownership.slot_snapshot(id).unwrap();
        let write = ownership
            .prepare_slot_write_fixed(
                captured,
                crate::audio_engine::content_lifetime::reserve().unwrap(),
            )
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
        self.engine.sample_cache.lock().unwrap()[id] = Some(sample);
        self.engine.pad_request_ids.lock().unwrap()[id] = generation;
        self.engine.loaded_source_generations.lock().unwrap()[id] = (generation, RATE);
        self.engine.loaded_source_digests.lock().unwrap()[id] = Some(format!("{generation:064x}"));
        self.engine.timing_intents.lock().unwrap()[id] = TimingIntent::Manual;
    }

    pub(in crate::audio_engine) fn mixer(&self) -> RtMixer {
        let mut mixer = RtMixer::new(1, RATE as f32);
        mixer.set_input_runtime_ownership(self.engine.input_runtime_ownership.clone());
        mixer.set_current_timing_acknowledgements(
            self.engine.current_timing_acknowledgements.clone(),
        );
        for id in [0, OTHER] {
            mixer.load_sample(
                id,
                self.engine.sample_cache.lock().unwrap()[id]
                    .clone()
                    .unwrap(),
            );
        }
        mixer
    }

    pub(in crate::audio_engine) fn trigger(&self, before_commit: impl FnOnce()) -> bool {
        dispatch_trigger_pad_before_commit(
            0,
            42,
            &self.state,
            &self.engine.input_runtime_ownership,
            &self.producer,
            None,
            before_commit,
        )
        .dispatched
    }

    pub(in crate::audio_engine) fn swap(&self, mixer: &mut RtMixer) {
        let ownership = &self.engine.input_runtime_ownership;
        let pair = ownership
            .prepare_placement_pair(
                ownership.slot_snapshot(0).unwrap(),
                ownership.slot_snapshot(OTHER).unwrap(),
            )
            .unwrap();
        ownership
            .commit_placement_pair(
                &pair,
                true,
                || true,
                || {
                    mixer.swap_occupied_images_for_key_lock_test(0, OTHER);
                },
            )
            .unwrap();
    }
}

pub(in crate::audio_engine) fn dispatch(
    mixer: &mut RtMixer,
    message: ControlMessage,
    scheduler: &mut FixedCapacityScheduler<8>,
    transport: &mut TransportTimeline,
    mode: &mut TriggerQuantization,
    feedback: &mut Vec<AudioMessage>,
) {
    process_control_message(
        message,
        scheduler,
        transport.output_frame(),
        mode,
        transport,
        mixer,
        feedback,
        &mut ImmediateAudioBufferRetirement,
    );
}

#[test]
fn midi_launch_pending_after_preparation_preserves_flags_and_queue_then_retries() {
    let mut fixture = LaunchFixture::new(8);
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let before = ownership.slot_snapshot(0).unwrap();
    let revision = ownership.launch_revision(0);
    let held = RefCell::new(None);
    assert!(!fixture.trigger(|| *held.borrow_mut() = Some(PendingWriter::hold(ownership.clone()))));
    held.into_inner().unwrap().complete();
    assert!(ownership.slot_snapshot(0).unwrap().same_projection(before));
    assert_eq!(ownership.launch_revision(0), revision);
    assert!(ownership.admitted_launch_ids().is_empty());
    assert!(fixture.consumer.is_empty());
    assert!(fixture.trigger(|| {}));
    assert_eq!(ownership.admitted_launch_ids(), vec![0]);
    assert!(matches!(
        fixture.consumer.pop().unwrap(),
        ControlMessage::TriggerInputPad {
            received_at_ns: 42,
            ..
        }
    ));
}

#[test]
fn midi_launch_full_queue_and_superseded_runtime_publish_no_admitted_flag() {
    for full in [false, true] {
        let mut fixture = LaunchFixture::new(1);
        let ownership = fixture.engine.input_runtime_ownership.clone();
        let before = ownership.slot_snapshot(0).unwrap();
        if full {
            fixture
                .producer
                .lock()
                .unwrap()
                .push(ControlMessage::Ping())
                .unwrap();
        }
        assert!(!fixture.trigger(|| {
            assert!(!full, "full queue must fail before claim seam");
            ownership
                .runtime_for_cell(before.cell())
                .store(1, Ordering::Release);
        }));
        assert!(ownership.admitted_launch_ids().is_empty());
        assert_eq!(ownership.launch_revision(0), 0);
        if full {
            assert!(matches!(
                fixture.consumer.pop().unwrap(),
                ControlMessage::Ping()
            ));
        }
        assert!(fixture.consumer.is_empty());
    }
}

#[test]
fn unadmitted_midi_snapshot_rejects_slot_rebind_and_aba() {
    for aba in [false, true] {
        let fixture = LaunchFixture::new(8);
        let mut mixer = fixture.mixer();
        fixture.swap(&mut mixer);
        if aba {
            fixture.swap(&mut mixer);
        }
        assert!(!fixture.trigger(|| {}));
        assert!(fixture.consumer.is_empty());
        assert!(
            fixture
                .engine
                .input_runtime_ownership
                .admitted_launch_ids()
                .is_empty()
        );
    }
}

#[test]
fn admitted_midi_immediate_and_quantized_effects_follow_swap_and_fence_retirement() {
    for quantized in [false, true] {
        for retired in [false, true] {
            let mut fixture = LaunchFixture::new(8);
            let mut mixer = fixture.mixer();
            assert!(fixture.trigger(|| {}));
            let message = fixture.consumer.pop().unwrap();
            let mut scheduler = FixedCapacityScheduler::<8>::new();
            let mut transport = TransportTimeline::new(RATE);
            transport.set_master_bpm(120.0);
            transport.advance_by_rendered_frames(1);
            let mut mode = if quantized {
                TriggerQuantization::Grid { step_64ths: 4 }
            } else {
                TriggerQuantization::Immediate
            };
            let mut feedback = Vec::new();
            if quantized {
                dispatch(
                    &mut mixer,
                    message.clone(),
                    &mut scheduler,
                    &mut transport,
                    &mut mode,
                    &mut feedback,
                );
            }
            fixture.swap(&mut mixer);
            if retired {
                let ownership = &fixture.engine.input_runtime_ownership;
                let captured = ownership.slot_snapshot(OTHER).unwrap();
                let write = ownership.prepare_slot_write_fixed(captured, 0).unwrap();
                ownership
                    .retire_source_write(&write, ownership.next_authority(OTHER).unwrap(), || true)
                    .unwrap();
                mixer.unload_sample(OTHER);
            }
            if quantized {
                let event = scheduler.pop_due_through(0, u64::MAX).unwrap();
                execute_scheduled_command(
                    &mut mixer,
                    &mut transport,
                    event.execution_frame,
                    event.command,
                    &mut feedback,
                    &mut ImmediateAudioBufferRetirement,
                );
            } else {
                dispatch(
                    &mut mixer,
                    message,
                    &mut scheduler,
                    &mut transport,
                    &mut mode,
                    &mut feedback,
                );
            }
            assert!(!mixer.voices.iter().any(|voice| voice.is_playing_sample(0)));
            assert_eq!(
                mixer
                    .voices
                    .iter()
                    .any(|voice| voice.is_playing_sample(OTHER)),
                !retired
            );
            if retired {
                assert!(feedback.is_empty());
            } else {
                assert!(matches!(
                    feedback.as_slice(),
                    [AudioMessage::SampleStarted { id: OTHER }]
                ));
            }
        }
    }
}

#[test]
fn admitted_stop_follows_content_and_empty_stop_cannot_cross_aba() {
    let fixture = LaunchFixture::new(8);
    let mut mixer = fixture.mixer();
    assert!(mixer.play_sample(0, 1.0));
    assert!(mixer.play_sample(OTHER, 1.0));
    let placement = fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap();
    fixture.swap(&mut mixer);
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    let mut transport = TransportTimeline::new(RATE);
    let mut mode = TriggerQuantization::Immediate;
    let mut feedback = Vec::new();
    dispatch(
        &mut mixer,
        ControlMessage::StopSampleForPlacement { placement },
        &mut scheduler,
        &mut transport,
        &mut mode,
        &mut feedback,
    );
    assert!(mixer.voices.iter().any(|voice| voice.is_playing_sample(0)));
    assert!(
        !mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(OTHER))
    );
    let ownership = &fixture.engine.input_runtime_ownership;
    let empty: SlotSnapshot = ownership.slot_snapshot(2).unwrap();
    fixture.publish(2, 0.75, 9);
    mixer.load_sample(
        2,
        fixture.engine.sample_cache.lock().unwrap()[2]
            .clone()
            .unwrap(),
    );
    assert!(mixer.play_sample(2, 1.0));
    dispatch(
        &mut mixer,
        ControlMessage::StopSampleForPlacement { placement: empty },
        &mut scheduler,
        &mut transport,
        &mut mode,
        &mut feedback,
    );
    assert!(mixer.voices.iter().any(|voice| voice.is_playing_sample(2)));
}
