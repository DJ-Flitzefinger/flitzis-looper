//! Whole global admission publishes captured guards, flags and one prepared payload together.
use super::*;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::input_mapping::launch_admission_tests::{
    LaunchFixture, OTHER, RATE, dispatch,
};
use crate::audio_engine::resident_relocation::control_publication_tests::PendingWriter;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, TriggerQuantization};
use std::cell::RefCell;

#[test]
fn global_launch_pending_or_late_participant_change_publishes_no_partial_flags() {
    for pending in [false, true] {
        let mut fixture = LaunchFixture::new(8);
        let ownership = fixture.engine.input_runtime_ownership.clone();
        let a = input_runtime_binding::capture(&fixture.engine, 0)
            .unwrap()
            .unwrap();
        let b = input_runtime_binding::capture(&fixture.engine, OTHER)
            .unwrap()
            .unwrap();
        let held = RefCell::new(None);
        assert!(
            enqueue_before_commit(
                &fixture.engine,
                &fixture.producer,
                vec![(&a, 0.0, None), (&b, 0.0, None)],
                true,
                Some(42),
                || {
                    if pending {
                        *held.borrow_mut() = Some(PendingWriter::hold(ownership.clone()));
                    } else {
                        ownership.revoke(OTHER, ownership.next_authority(OTHER).unwrap());
                    }
                }
            )
            .is_err()
        );
        if let Some(writer) = held.into_inner() {
            writer.complete();
        }
        assert!(ownership.admitted_launch_ids().is_empty());
        assert_eq!(ownership.launch_revision(0), 0);
        assert_eq!(ownership.launch_revision(OTHER), 0);
        assert!(fixture.consumer.is_empty());
        if pending {
            assert!(
                enqueue(
                    &fixture.engine,
                    &fixture.producer,
                    vec![(&a, 0.0, None), (&b, 0.0, None)],
                    true,
                    Some(43)
                )
                .is_ok()
            );
            assert_eq!(ownership.admitted_launch_ids(), vec![0, OTHER]);
            assert!(matches!(
                fixture.consumer.pop().unwrap(),
                ControlMessage::GlobalPlaybackBatch(_)
            ));
        }
    }
}

#[test]
fn global_launch_full_queue_and_late_exhaustion_leave_earlier_entries_unpublished() {
    for full in [false, true] {
        let mut fixture = LaunchFixture::new(1);
        let ownership = &fixture.engine.input_runtime_ownership;
        let a = input_runtime_binding::capture(&fixture.engine, 0)
            .unwrap()
            .unwrap();
        let b = input_runtime_binding::capture(&fixture.engine, OTHER)
            .unwrap()
            .unwrap();
        if full {
            fixture
                .producer
                .lock()
                .unwrap()
                .push(ControlMessage::Ping())
                .unwrap();
        } else {
            ownership
                .launch_revision_for_cell(b.binding.placement.unwrap().cell())
                .store(u64::MAX, Ordering::Release);
        }
        assert!(
            enqueue_before_commit(
                &fixture.engine,
                &fixture.producer,
                vec![(&a, 0.0, None), (&b, 0.0, None)],
                true,
                None,
                || panic!("failed preflight entered the mutation seam")
            )
            .is_err()
        );
        assert_eq!(ownership.launch_revision(0), 0);
        assert!(ownership.admitted_launch_ids().is_empty());
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
fn admitted_global_start_resolves_every_original_content_after_quantized_swap() {
    for quantized in [false, true] {
        let mut fixture = LaunchFixture::new(8);
        let mut mixer = fixture.mixer();
        let a = input_runtime_binding::capture(&fixture.engine, 0)
            .unwrap()
            .unwrap();
        let b = input_runtime_binding::capture(&fixture.engine, OTHER)
            .unwrap()
            .unwrap();
        let ticket = enqueue(
            &fixture.engine,
            &fixture.producer,
            vec![
                (&a, 16.0 / f64::from(RATE), Some(96.0 / f64::from(RATE))),
                (&b, 32.0 / f64::from(RATE), Some(112.0 / f64::from(RATE))),
            ],
            true,
            Some(42),
        )
        .unwrap();
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
        if quantized {
            let event = scheduler.pop_due_through(0, u64::MAX).unwrap();
            crate::audio_engine::audio_stream::execute_scheduled_command(
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
        assert_eq!(ticket.publication_status(), "accepted");
        assert!(matches!(
            feedback.as_slice(),
            [
                AudioMessage::SampleStarted { id: OTHER },
                AudioMessage::SampleStarted { id: 0 }
            ]
        ));
        for (id, address) in [
            (OTHER, a.binding.source_address),
            (0, b.binding.source_address),
        ] {
            let voice = mixer
                .voices
                .iter()
                .find(|voice| voice.is_playing_sample(id))
                .unwrap();
            assert_eq!(voice.sample.as_ref().unwrap().source_address(), address);
        }
    }
}

#[test]
fn one_retired_global_participant_rejects_all_effects_and_preserves_other_voice() {
    let mut fixture = LaunchFixture::new(8);
    let mut mixer = fixture.mixer();
    assert!(mixer.play_sample(OTHER, 1.0));
    let a = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    let b = input_runtime_binding::capture(&fixture.engine, OTHER)
        .unwrap()
        .unwrap();
    let ticket = enqueue(
        &fixture.engine,
        &fixture.producer,
        vec![
            (&a, 16.0 / f64::from(RATE), None),
            (&b, 32.0 / f64::from(RATE), None),
        ],
        true,
        None,
    )
    .unwrap();
    let ownership = &fixture.engine.input_runtime_ownership;
    let captured = ownership.slot_snapshot(0).unwrap();
    let write = ownership.prepare_slot_write_fixed(captured, 0).unwrap();
    ownership
        .retire_source_write(&write, ownership.next_authority(0).unwrap(), || true)
        .unwrap();
    mixer.unload_sample(0);
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    let mut transport = TransportTimeline::new(RATE);
    let mut mode = TriggerQuantization::Immediate;
    let mut feedback = Vec::new();
    dispatch(
        &mut mixer,
        fixture.consumer.pop().unwrap(),
        &mut scheduler,
        &mut transport,
        &mut mode,
        &mut feedback,
    );
    assert_eq!(ticket.publication_status(), "rejected");
    assert!(feedback.is_empty());
    assert!(
        mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(OTHER))
    );
}
