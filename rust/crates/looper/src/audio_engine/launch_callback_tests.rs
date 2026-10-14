//! Actual callback claims preserve admitted source birth and one exact deferred due payload.
use super::*;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::input_mapping::launch_admission_tests::{LaunchFixture, OTHER, RATE};
use crate::audio_engine::input_runtime_binding;
use crate::audio_engine::resident_relocation::control_publication_tests::PendingWriter;
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::Ordering;

thread_local! {
    static BEFORE_CLAIM: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

pub(super) fn before_claim() {
    let hook = BEFORE_CLAIM.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn with_before_claim<T>(hook: impl FnOnce() + 'static, action: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            BEFORE_CLAIM.with(|slot| *slot.borrow_mut() = None);
        }
    }
    BEFORE_CLAIM.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    let _reset = Reset;
    action()
}

fn trigger_command(fixture: &mut LaunchFixture) -> ScheduledCommand {
    assert!(fixture.trigger(|| {}));
    let ControlMessage::TriggerInputPad {
        id,
        start_s,
        end_s,
        exclusive,
        binding,
        received_at_ns,
        resident_control,
        launch_revision,
    } = fixture.consumer.pop().unwrap()
    else {
        panic!("real MIDI admission must publish its trigger")
    };
    ScheduledCommand::TriggerInputPad {
        id,
        start_s,
        end_s,
        exclusive,
        binding,
        received_at_ns,
        resident_control,
        launch_revision,
    }
}

#[test]
fn late_pending_retains_exact_due_command_target_and_queue_order_then_starts_one_original_voice() {
    let mut fixture = LaunchFixture::new(8);
    let mut mixer = fixture.mixer();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
    let command = trigger_command(&mut fixture);
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    scheduler.schedule(12, command.clone()).unwrap();
    scheduler
        .schedule(
            12,
            ScheduledCommand::PlaySample {
                id: OTHER,
                volume: 1.0,
                received_at_ns: Some(43),
                expected_source_generation: Some(8),
            },
        )
        .unwrap();
    let mut transport = TransportTimeline::new(RATE);
    let mut feedback = Vec::new();
    let held = Rc::new(RefCell::new(None));
    let hook_hold = held.clone();
    with_before_claim(
        move || *hook_hold.borrow_mut() = Some(PendingWriter::hold(ownership)),
        || {
            drain_scheduler_due_at_callback_start(
                &mut scheduler,
                12,
                &mut mixer,
                &mut transport,
                &mut feedback,
                &mut ImmediateAudioBufferRetirement,
            );
        },
    );
    let pending = scheduler.deferred_due().unwrap();
    assert_eq!(pending.target_frame, 12);
    assert_eq!(pending.command, command);
    assert_eq!(scheduler.len(), 2);
    assert!(feedback.is_empty());
    assert!(!mixer.voices.iter().any(|voice| voice.active));
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    let mut mode = TriggerQuantization::Immediate;
    assert_eq!(
        drain_control_messages(
            &mut fixture.consumer,
            &mut scheduler,
            12,
            &mut mode,
            &mut transport,
            &mut mixer,
            &mut feedback,
            &mut ImmediateAudioBufferRetirement
        ),
        0
    );
    assert!(matches!(
        fixture.consumer.peek(),
        Ok(ControlMessage::Ping())
    ));
    assert_eq!(scheduler.deferred_due().unwrap().command, command);
    held.borrow_mut().take().unwrap().complete();
    assert_eq!(
        drain_control_messages(
            &mut fixture.consumer,
            &mut scheduler,
            13,
            &mut mode,
            &mut transport,
            &mut mixer,
            &mut feedback,
            &mut ImmediateAudioBufferRetirement
        ),
        1
    );
    assert!(scheduler.is_empty());
    assert!(matches!(
        feedback.as_slice(),
        [
            AudioMessage::SampleStarted { id: 0 },
            AudioMessage::SampleStarted { id: OTHER },
            AudioMessage::Pong()
        ]
    ));
    let voice = mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap();
    assert_eq!(
        voice
            .source_admission
            .as_ref()
            .unwrap()
            .content_key_for_test(),
        Some(key)
    );
}

#[test]
fn in_buffer_pending_renders_remaining_audio_without_retry_spin_and_retains_original_target() {
    let mut fixture = LaunchFixture::new(8);
    let mut mixer = fixture.mixer();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
    let command = trigger_command(&mut fixture);
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    scheduler.schedule(4, command.clone()).unwrap();
    let mut transport = TransportTimeline::new(RATE);
    let mut feedback = Vec::new();
    let mut output = [99.0; 8];
    let mut peaks = [0.0; NUM_SAMPLES];
    let held = Rc::new(RefCell::new(None));
    let hook_hold = held.clone();
    with_before_claim(
        move || *hook_hold.borrow_mut() = Some(PendingWriter::hold(ownership)),
        || {
            render_scheduled_audio(
                &mut mixer,
                &mut scheduler,
                &mut output,
                &mut peaks,
                0,
                1,
                &mut transport,
                &mut feedback,
                &mut ImmediateAudioBufferRetirement,
            );
        },
    );
    assert_eq!(output, [0.0; 8]);
    assert_eq!(scheduler.deferred_due().unwrap().target_frame, 4);
    assert_eq!(scheduler.deferred_due().unwrap().command, command);
    assert!(feedback.is_empty());
    held.borrow_mut().take().unwrap().complete();
    render_scheduled_audio(
        &mut mixer,
        &mut scheduler,
        &mut output,
        &mut peaks,
        8,
        1,
        &mut transport,
        &mut feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    assert!(scheduler.is_empty());
    assert!(matches!(
        feedback.as_slice(),
        [AudioMessage::SampleStarted { id: 0 }]
    ));
    let voice = mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap();
    assert_eq!(
        voice
            .source_admission
            .as_ref()
            .unwrap()
            .content_key_for_test(),
        Some(key)
    );
}

#[test]
fn full_scheduler_immediate_fallback_retains_one_pending_payload_ahead_of_future_events() {
    let mut fixture = LaunchFixture::new(8);
    let mut mixer = fixture.mixer();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let command = trigger_command(&mut fixture);
    let mut scheduler = FixedCapacityScheduler::<1>::new();
    scheduler
        .schedule(
            100,
            ScheduledCommand::PlaySample {
                id: OTHER,
                volume: 1.0,
                received_at_ns: None,
                expected_source_generation: Some(8),
            },
        )
        .unwrap();
    let mut transport = TransportTimeline::new(RATE);
    let mut feedback = Vec::new();
    let held = Rc::new(RefCell::new(None));
    let hook_hold = held.clone();
    with_before_claim(
        move || *hook_hold.borrow_mut() = Some(PendingWriter::hold(ownership)),
        || {
            schedule_immediate_command(
                &mut scheduler,
                0,
                command.clone(),
                &mut mixer,
                &mut transport,
                &mut feedback,
                &mut ImmediateAudioBufferRetirement,
            );
        },
    );
    assert_eq!(scheduler.deferred_due().unwrap().target_frame, 0);
    assert_eq!(scheduler.deferred_due().unwrap().command, command);
    assert_eq!(scheduler.len(), 2);
    assert!(feedback.is_empty());
    held.borrow_mut().take().unwrap().complete();
    drain_scheduler_due_at_callback_start(
        &mut scheduler,
        1,
        &mut mixer,
        &mut transport,
        &mut feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    assert!(scheduler.deferred_due().is_none());
    assert_eq!(scheduler.peek_next_target_frame(), Some(100));
    assert!(matches!(
        feedback.as_slice(),
        [AudioMessage::SampleStarted { id: 0 }]
    ));
}

#[test]
fn actual_equal_pcm_reassignment_after_preparation_cannot_mint_new_lifetime_birth_for_old_trigger()
{
    let mut fixture = LaunchFixture::new(8);
    let mut mixer = fixture.mixer();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let original = ownership.slot_snapshot(0).unwrap();
    let sample = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let command = trigger_command(&mut fixture);
    let changed = Rc::new(Cell::new(None));
    let hook_changed = changed.clone();
    let mut transport = TransportTimeline::new(RATE);
    let mut feedback = Vec::new();
    let result = with_before_claim(
        move || {
            let write = ownership
                .prepare_slot_write_fixed(
                    original,
                    crate::audio_engine::content_lifetime::reserve().unwrap(),
                )
                .unwrap();
            let publication = ownership
                .publish_source_write(
                    &write,
                    &sample,
                    RATE,
                    7,
                    ownership.next_authority(0).unwrap(),
                    Some(TimingIntent::Manual),
                    false,
                    || true,
                )
                .unwrap();
            assert!(publication.current(&sample, RATE));
            hook_changed.set(publication.key());
        },
        || {
            execute_scheduled_command(
                &mut mixer,
                &mut transport,
                0,
                command,
                &mut feedback,
                &mut ImmediateAudioBufferRetirement,
            )
        },
    );
    assert!(matches!(result, ScheduledExecution::Complete));
    assert_ne!(changed.get(), original.content_key());
    assert!(feedback.is_empty());
    assert!(
        !mixer
            .voices
            .iter()
            .any(|voice| voice.active || voice.source_admission.is_some())
    );
    let current = fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap();
    assert_eq!(current.content_key(), changed.get());
    assert_eq!(
        fixture
            .engine
            .input_runtime_ownership
            .launch_revision_for_cell(current.cell())
            .load(Ordering::Acquire),
        0
    );
}

#[test]
fn global_pending_retains_the_exact_arc_and_publishes_all_original_births_once() {
    let mut fixture = LaunchFixture::new(8);
    let mut mixer = fixture.mixer();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let a = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    let b = input_runtime_binding::capture(&fixture.engine, OTHER)
        .unwrap()
        .unwrap();
    let ticket = crate::audio_engine::global_playback_batch::enqueue(
        &fixture.engine,
        &fixture.producer,
        vec![(&a, 0.0, None), (&b, 0.0, None)],
        true,
        Some(42),
    )
    .unwrap();
    let ControlMessage::GlobalPlaybackBatch(batch) = fixture.consumer.pop().unwrap() else {
        panic!("batch")
    };
    let original = batch.clone();
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    scheduler
        .schedule(0, ScheduledCommand::GlobalPlaybackBatch(batch))
        .unwrap();
    let mut transport = TransportTimeline::new(RATE);
    let mut feedback = Vec::new();
    let held = Rc::new(RefCell::new(None));
    let hook_hold = held.clone();
    with_before_claim(
        move || *hook_hold.borrow_mut() = Some(PendingWriter::hold(ownership)),
        || {
            drain_scheduler_due_at_callback_start(
                &mut scheduler,
                0,
                &mut mixer,
                &mut transport,
                &mut feedback,
                &mut ImmediateAudioBufferRetirement,
            );
        },
    );
    let ScheduledCommand::GlobalPlaybackBatch(retained) =
        &scheduler.deferred_due().unwrap().command
    else {
        panic!("batch")
    };
    assert!(Arc::ptr_eq(retained, &original));
    assert_eq!(ticket.publication_status(), "pending");
    assert!(feedback.is_empty());
    assert!(!mixer.voices.iter().any(|voice| voice.active));
    held.borrow_mut().take().unwrap().complete();
    drain_scheduler_due_at_callback_start(
        &mut scheduler,
        1,
        &mut mixer,
        &mut transport,
        &mut feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(scheduler.is_empty());
    assert!(matches!(
        feedback.as_slice(),
        [
            AudioMessage::SampleStarted { id: 0 },
            AudioMessage::SampleStarted { id: OTHER }
        ]
    ));
    for (id, binding) in [(0, a.binding), (OTHER, b.binding)] {
        let voice = mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(id))
            .unwrap();
        assert_eq!(
            voice
                .source_admission
                .as_ref()
                .unwrap()
                .content_key_for_test(),
            binding.placement.unwrap().content_key()
        );
    }
}

#[test]
fn global_late_equal_pcm_rebind_of_last_participant_rejects_the_whole_birth() {
    let mut fixture = LaunchFixture::new(8);
    let mut mixer = fixture.mixer();
    assert!(mixer.play_sample(0, 1.0));
    let before_generation = mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
        .generation;
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let a = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    let b = input_runtime_binding::capture(&fixture.engine, OTHER)
        .unwrap()
        .unwrap();
    let old_b = b.binding.placement.unwrap();
    let sample = fixture.engine.sample_cache.lock().unwrap()[OTHER]
        .clone()
        .unwrap();
    let ticket = crate::audio_engine::global_playback_batch::enqueue(
        &fixture.engine,
        &fixture.producer,
        vec![
            (&a, 16.0 / f64::from(RATE), None),
            (&b, 32.0 / f64::from(RATE), None),
        ],
        true,
        Some(42),
    )
    .unwrap();
    let ControlMessage::GlobalPlaybackBatch(batch) = fixture.consumer.pop().unwrap() else {
        panic!("batch")
    };
    let mut transport = TransportTimeline::new(RATE);
    let mut feedback = Vec::new();
    let result = with_before_claim(
        move || {
            let write = ownership
                .prepare_slot_write_fixed(
                    old_b,
                    crate::audio_engine::content_lifetime::reserve().unwrap(),
                )
                .unwrap();
            ownership
                .publish_source_write(
                    &write,
                    &sample,
                    RATE,
                    8,
                    ownership.next_authority(OTHER).unwrap(),
                    Some(TimingIntent::Manual),
                    false,
                    || true,
                )
                .unwrap();
        },
        || {
            execute_scheduled_command(
                &mut mixer,
                &mut transport,
                0,
                ScheduledCommand::GlobalPlaybackBatch(batch),
                &mut feedback,
                &mut ImmediateAudioBufferRetirement,
            )
        },
    );
    assert!(matches!(result, ScheduledExecution::Complete));
    assert_eq!(ticket.publication_status(), "rejected");
    assert!(feedback.is_empty());
    assert!(
        !mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(OTHER))
    );
    let voice = mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap();
    assert_eq!(voice.generation, before_generation);
    assert_eq!(
        voice
            .source_admission
            .as_ref()
            .unwrap()
            .content_key_for_test(),
        a.binding.placement.unwrap().content_key()
    );
}

#[test]
fn captured_stop_late_pending_follows_moved_key_and_keeps_later_queue_order() {
    let mut fixture = LaunchFixture::new(8);
    let mut mixer = fixture.mixer();
    assert!(mixer.play_sample(0, 1.0));
    assert!(mixer.play_sample(OTHER, 1.0));
    let ownership = fixture.engine.input_runtime_ownership.clone();
    assert!(input_runtime_binding::enqueue_stop_with_producer(
        &ownership,
        &mut fixture.producer.lock().unwrap(),
        Some(0)
    ));
    let message = fixture.consumer.pop().unwrap();
    let ControlMessage::StopSampleForPlacement { placement } = message else {
        panic!("captured stop")
    };
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    let mut transport = TransportTimeline::new(RATE);
    let mut mode = TriggerQuantization::Immediate;
    let mut feedback = Vec::new();
    let held = Rc::new(RefCell::new(None));
    let hook_hold = held.clone();
    with_before_claim(
        move || *hook_hold.borrow_mut() = Some(PendingWriter::hold(ownership)),
        || {
            process_control_message(
                message,
                &mut scheduler,
                0,
                &mut mode,
                &mut transport,
                &mut mixer,
                &mut feedback,
                &mut ImmediateAudioBufferRetirement,
            );
        },
    );
    assert_eq!(
        scheduler.deferred_due().unwrap().command,
        ScheduledCommand::StopSampleForPlacement { placement }
    );
    assert!(feedback.is_empty());
    assert!(mixer.voices.iter().any(|voice| voice.is_playing_sample(0)));
    held.borrow_mut().take().unwrap().complete();
    fixture.swap(&mut mixer);
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    assert_eq!(
        drain_control_messages(
            &mut fixture.consumer,
            &mut scheduler,
            1,
            &mut mode,
            &mut transport,
            &mut mixer,
            &mut feedback,
            &mut ImmediateAudioBufferRetirement
        ),
        1
    );
    assert!(scheduler.is_empty());
    assert!(mixer.voices.iter().any(|voice| voice.is_playing_sample(0)));
    assert!(
        !mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(OTHER))
    );
    assert!(matches!(
        feedback.as_slice(),
        [
            AudioMessage::SampleStopped { id: OTHER },
            AudioMessage::Pong()
        ]
    ));
}

#[test]
fn captured_stop_equal_pcm_reassignment_after_preflight_cannot_stop_successor_lifetime() {
    let fixture = LaunchFixture::new(8);
    let mut mixer = fixture.mixer();
    assert!(mixer.play_sample(0, 1.0));
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let placement = ownership.slot_snapshot(0).unwrap();
    let sample = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let command = ScheduledCommand::StopSampleForPlacement { placement };
    let changed = Rc::new(Cell::new(None));
    let hook_changed = changed.clone();
    let mut transport = TransportTimeline::new(RATE);
    let mut feedback = Vec::new();
    let result = with_before_claim(
        move || {
            let write = ownership
                .prepare_slot_write_fixed(
                    placement,
                    crate::audio_engine::content_lifetime::reserve().unwrap(),
                )
                .unwrap();
            let publication = ownership
                .publish_source_write(
                    &write,
                    &sample,
                    RATE,
                    7,
                    ownership.next_authority(0).unwrap(),
                    Some(TimingIntent::Manual),
                    false,
                    || true,
                )
                .unwrap();
            hook_changed.set(publication.key());
        },
        || {
            execute_scheduled_command(
                &mut mixer,
                &mut transport,
                0,
                command.clone(),
                &mut feedback,
                &mut ImmediateAudioBufferRetirement,
            )
        },
    );
    assert!(matches!(result, ScheduledExecution::Complete));
    assert!(feedback.is_empty());
    assert!(mixer.voices.iter().any(|voice| voice.is_playing_sample(0)));
    assert!(mixer.play_sample(0, 1.0));
    let voice = mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap();
    assert_eq!(
        voice
            .source_admission
            .as_ref()
            .unwrap()
            .content_key_for_test(),
        changed.get()
    );
    let generation = voice.generation;
    assert!(matches!(
        execute_scheduled_command(
            &mut mixer,
            &mut transport,
            1,
            command,
            &mut feedback,
            &mut ImmediateAudioBufferRetirement
        ),
        ScheduledExecution::Complete
    ));
    let voice = mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap();
    assert_eq!(voice.generation, generation);
    assert!(feedback.is_empty());
}

#[test]
fn captured_empty_stop_pending_cannot_cross_empty_aba_or_stop_later_occupant() {
    for occupied_after_aba in [false, true] {
        let fixture = LaunchFixture::new(8);
        let mut mixer = fixture.mixer();
        let ownership = fixture.engine.input_runtime_ownership.clone();
        let placement = ownership.slot_snapshot(2).unwrap();
        let command = ScheduledCommand::StopSampleForPlacement { placement };
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        scheduler.schedule(0, command.clone()).unwrap();
        let mut transport = TransportTimeline::new(RATE);
        let mut feedback = Vec::new();
        let held = Rc::new(RefCell::new(None));
        let hook_hold = held.clone();
        let hook_ownership = ownership.clone();
        with_before_claim(
            move || *hook_hold.borrow_mut() = Some(PendingWriter::hold(hook_ownership)),
            || {
                drain_scheduler_due_at_callback_start(
                    &mut scheduler,
                    0,
                    &mut mixer,
                    &mut transport,
                    &mut feedback,
                    &mut ImmediateAudioBufferRetirement,
                );
            },
        );
        assert_eq!(scheduler.deferred_due().unwrap().command, command);
        assert!(feedback.is_empty());
        held.borrow_mut().take().unwrap().complete();
        fixture.publish(2, 0.75, 9);
        let installed = ownership.slot_snapshot(2).unwrap();
        let retirement = ownership.prepare_slot_write_fixed(installed, 0).unwrap();
        ownership
            .retire_source_write(&retirement, ownership.next_authority(2).unwrap(), || true)
            .unwrap();
        assert_eq!(ownership.slot_snapshot(2).unwrap().lifetime(), 0);
        assert!(!ownership.slot_snapshot_fields_current(placement));
        if occupied_after_aba {
            fixture.publish(2, 0.75, 10);
            mixer.load_sample(
                2,
                fixture.engine.sample_cache.lock().unwrap()[2]
                    .clone()
                    .unwrap(),
            );
            assert!(mixer.play_sample(2, 1.0));
        }
        drain_scheduler_due_at_callback_start(
            &mut scheduler,
            1,
            &mut mixer,
            &mut transport,
            &mut feedback,
            &mut ImmediateAudioBufferRetirement,
        );
        assert!(scheduler.is_empty());
        assert!(feedback.is_empty());
        assert_eq!(
            mixer.voices.iter().any(|voice| voice.is_playing_sample(2)),
            occupied_after_aba
        );
    }
}
