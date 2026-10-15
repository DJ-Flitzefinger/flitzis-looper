//! Actual callback producer tags remain original-content observations through Pair placement.

use super::*;
use crate::audio_engine::complete_context::source_custody_tests::own_pair_ack;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::pad_pair;
use std::sync::atomic::AtomicU8;

thread_local! {
    static AFTER_EFFECT: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

pub(super) fn after_effect() {
    let hook = AFTER_EFFECT.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn with_after_effect<T>(hook: impl FnOnce() + 'static, action: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            AFTER_EFFECT.with(|slot| *slot.borrow_mut() = None);
        }
    }
    AFTER_EFFECT.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    let _reset = Reset;
    action()
}

fn process(mixer: &mut RtMixer, message: ControlMessage, feedback: &mut impl AudioMessageSink) {
    process_control_message(
        message,
        &mut FixedCapacityScheduler::<8>::new(),
        0,
        &mut TriggerQuantization::Immediate,
        &mut TransportTimeline::new(48_000),
        mixer,
        feedback,
        &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement,
    );
}

fn source_adoption(
    fixture: &LaunchFixture,
    slot: usize,
    fresh_lifetime: bool,
) -> (ControlMessage, Arc<AtomicU8>) {
    let ownership = &fixture.engine.input_runtime_ownership;
    let sample = fixture.engine.sample_cache.lock().unwrap()[slot]
        .clone()
        .unwrap();
    let (generation, rate) = fixture.engine.loaded_source_generations.lock().unwrap()[slot];
    let before = ownership.slot_snapshot(slot).unwrap();
    let lifetime = if fresh_lifetime || before.lifetime() == 0 {
        crate::audio_engine::content_lifetime::reserve().unwrap()
    } else {
        before.lifetime()
    };
    let write = ownership
        .prepare_slot_write_fixed(before, lifetime)
        .unwrap();
    let publication = ownership
        .publish_source_write(
            &write,
            &sample,
            rate,
            generation,
            ownership.authority_revision(slot),
            None,
            true,
            || true,
        )
        .unwrap();
    let adoption = Arc::new(AtomicU8::new(0));
    (
        ControlMessage::LoadColdSample {
            publication,
            id: slot,
            sample,
            source_generation: generation,
            content_lifetime: lifetime,
            adoption: adoption.clone(),
            epoch: None,
            captured_epoch: 0,
            replace_assignment: false,
            loop_region: None,
            resident_cancelled: None,
            incoming_defaults: None,
            initial_pitch: crate::audio_engine::SourcePitchInitialization::FreshNeutral,
        },
        adoption,
    )
}

fn accepted_fixture() -> LaunchFixture {
    let fixture = LaunchFixture::new(4);
    let mut mixer = fixture.mixer();
    for slot in [0, 1] {
        let (message, adoption) = source_adoption(&fixture, slot, false);
        process(&mut mixer, message, &mut Vec::<AudioMessage>::new());
        assert_eq!(
            adoption.load(Ordering::Acquire),
            2,
            "actual source callback ACK"
        );
    }
    fixture
}

fn feedback(fixture: &LaunchFixture, message: AudioMessage) -> AudioMessage {
    let (producer, mut consumer) = RingBuffer::new(1);
    let mut tagged = ContentFeedbackProducer {
        producer,
        ownership: fixture.engine.input_runtime_ownership.clone(),
    };
    tagged.push_audio_message(message);
    consumer.pop().expect("actual fixed callback feedback")
}

#[test]
fn actual_started_stopped_peak_and_playhead_tags_resolve_original_owner_after_pair_control_ack() {
    for (signal, kind) in [
        (AudioMessage::SampleStarted { id: 0 }, "started"),
        (AudioMessage::SampleStopped { id: 0 }, "stopped"),
        (AudioMessage::PadPeak { id: 0, peak: 0.375 }, "peak"),
        (
            AudioMessage::PadPlayhead {
                id: 0,
                position_s: 1.25,
            },
            "playhead",
        ),
    ] {
        let mut fixture = accepted_fixture();
        let original = feedback(&fixture, signal);
        assert_eq!(original.feedback_kind(), Some(kind));
        assert_eq!(original.sample_id(), Some(0));
        assert_eq!(
            fixture.engine._resolve_pad_feedback(&original).unwrap(),
            Some(0)
        );
        let mut mixer = fixture.mixer();
        mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
        let ticket = own_pair_ack(&mut fixture, &mut mixer);
        assert!(fixture.engine._resolve_pad_feedback(&original).is_err());
        assert!(pad_pair::reconcile(&fixture.engine, &ticket).unwrap());
        assert_eq!(
            original.sample_id(),
            Some(0),
            "diagnostic id remains original"
        );
        assert_eq!(
            fixture.engine._resolve_pad_feedback(&original).unwrap(),
            Some(1)
        );
        match kind {
            "peak" => assert_eq!(original.pad_peak(), Some(0.375)),
            "playhead" => assert_eq!(original.pad_playhead(), Some(1.25)),
            _ => {}
        }
        let ack = ticket.own_acknowledgement().unwrap();
        fixture.engine._finish_pad_pair(&ticket, &ack).unwrap();
        assert_eq!(
            fixture.engine._resolve_pad_feedback(&original).unwrap(),
            Some(1)
        );
    }
}

#[test]
fn feedback_rejects_new_transport_intent_retirement_and_equal_backing_source_rebirth() {
    let mut fixture = accepted_fixture();
    let original = feedback(&fixture, AudioMessage::SampleStarted { id: 0 });
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let ticket = own_pair_ack(&mut fixture, &mut mixer);
    assert!(pad_pair::reconcile(&fixture.engine, &ticket).unwrap());
    let ack = ticket.own_acknowledgement().unwrap();
    fixture.engine._finish_pad_pair(&ticket, &ack).unwrap();
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&original).unwrap(),
        Some(1)
    );
    fixture.engine.cancel_pad_launches(1).unwrap();
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&original).unwrap(),
        None
    );
    let after_transport = feedback(
        &fixture,
        AudioMessage::PadPlayhead {
            id: 1,
            position_s: 1.25,
        },
    );
    assert_eq!(
        fixture
            .engine
            ._resolve_pad_feedback(&after_transport)
            .unwrap(),
        Some(1)
    );
    let source = fixture.engine.sample_cache.lock().unwrap()[1]
        .clone()
        .unwrap();
    let generation = fixture.engine.loaded_source_generations.lock().unwrap()[1];
    let old_lifetime = fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(1)
        .unwrap()
        .lifetime();
    crate::audio_engine::cold_load::unload_for_producer(&fixture.engine, 1, &fixture.producer)
        .unwrap();
    let (producer, mut retired_feedback) = RingBuffer::new(4);
    let mut tagged = ContentFeedbackProducer {
        producer,
        ownership: fixture.engine.input_runtime_ownership.clone(),
    };
    process(&mut mixer, fixture.consumer.pop().unwrap(), &mut tagged);
    let stopped = retired_feedback
        .pop()
        .expect("actual unload feedback survives the complete marker and tag producer");
    assert_eq!(stopped.feedback_kind(), Some("stopped"));
    assert!(
        matches!(&stopped, AudioMessage::ContentFeedback { lifetime, .. } if *lifetime == old_lifetime)
    );
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&stopped).unwrap(),
        None
    );
    assert_eq!(
        fixture
            .engine
            ._resolve_pad_feedback(&after_transport)
            .unwrap(),
        None
    );
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&original).unwrap(),
        None
    );
    fixture.engine.sample_cache.lock().unwrap()[1] = Some(source.clone());
    fixture.engine.loaded_source_generations.lock().unwrap()[1] = generation;
    fixture.engine.loaded_source_digests.lock().unwrap()[1] =
        Some(format!("{:064x}", generation.0));
    let (message, adoption) = source_adoption(&fixture, 1, true);
    process(&mut mixer, message, &mut tagged);
    assert_eq!(adoption.load(Ordering::Acquire), 2);
    assert!(
        fixture.engine.sample_cache.lock().unwrap()[1]
            .as_ref()
            .unwrap()
            .same_source(&source)
    );
    assert_ne!(
        fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(1)
            .unwrap()
            .lifetime(),
        old_lifetime
    );
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&original).unwrap(),
        None,
        "reset transport revision and equal source metadata cannot revive old lifetime"
    );
    assert_eq!(
        fixture
            .engine
            ._resolve_pad_feedback(&after_transport)
            .unwrap(),
        None
    );
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&stopped).unwrap(),
        None
    );
    let successor = feedback(&fixture, AudioMessage::SampleStarted { id: 1 });
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&successor).unwrap(),
        Some(1)
    );
}

#[test]
fn feedback_foreign_owner_invalid_cell_zero_lifetime_and_legacy_slot_messages_grant_no_projection()
{
    let fixture = accepted_fixture();
    let original = feedback(&fixture, AudioMessage::SampleStarted { id: 0 });
    let foreign = LaunchFixture::new(4);
    assert_eq!(
        foreign.engine._resolve_pad_feedback(&original).unwrap(),
        None
    );
    let AudioMessage::ContentFeedback {
        id,
        owner,
        cell,
        lifetime,
        transport_revision,
        kind,
        value,
    } = original
    else {
        panic!("actual original tags");
    };
    for (invalid_cell, invalid_lifetime) in [(NUM_SAMPLES, lifetime), (cell, 0)] {
        let invalid = AudioMessage::ContentFeedback {
            id,
            owner,
            cell: invalid_cell,
            lifetime: invalid_lifetime,
            transport_revision,
            kind,
            value,
        };
        assert_eq!(
            fixture.engine._resolve_pad_feedback(&invalid).unwrap(),
            None
        );
    }
    for legacy in [
        AudioMessage::SampleStarted { id: 0 },
        AudioMessage::SampleStopped { id: 0 },
        AudioMessage::PadPeak { id: 0, peak: 0.375 },
        AudioMessage::PadPlayhead {
            id: 0,
            position_s: 1.25,
        },
    ] {
        assert_eq!(fixture.engine._resolve_pad_feedback(&legacy).unwrap(), None);
    }
}

#[test]
fn unrelated_actual_pending_marker_keeps_feedback_as_a_bounded_retry() {
    let fixture = accepted_fixture();
    let original = feedback(&fixture, AudioMessage::PadPeak { id: 0, peak: 0.375 });
    let pending =
        crate::audio_engine::resident_relocation::control_publication_tests::PendingWriter::hold(
            fixture.engine.input_runtime_ownership.clone(),
        );
    assert!(fixture.engine._resolve_pad_feedback(&original).is_err());
    pending.complete();
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&original).unwrap(),
        Some(0)
    );
    assert_eq!(original.pad_peak(), Some(0.375));
}

#[test]
fn actual_callback_tag_producer_preserves_all_feedback_inside_its_own_odd_marker() {
    let fixture = accepted_fixture();
    let ownership = &fixture.engine.input_runtime_ownership;
    let before = ownership.slot_snapshot(0).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(before, before.lifetime())
        .unwrap();
    let (producer, mut consumer) = RingBuffer::new(4);
    let mut tagged = ContentFeedbackProducer {
        producer,
        ownership: ownership.clone(),
    };
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |_| {
                assert!(
                    ownership.slot_snapshot(0).is_none(),
                    "the callback actually owns the odd marker"
                );
                for message in [
                    AudioMessage::SampleStarted { id: 0 },
                    AudioMessage::SampleStopped { id: 0 },
                    AudioMessage::PadPeak { id: 0, peak: 0.375 },
                    AudioMessage::PadPlayhead {
                        id: 0,
                        position_s: 1.25,
                    },
                ] {
                    tagged.push_audio_message(message);
                }
            },
        )
        .unwrap();
    for kind in ["started", "stopped", "peak", "playhead"] {
        let message = consumer
            .pop()
            .expect("own marker must retain required feedback");
        assert_eq!(message.feedback_kind(), Some(kind));
        assert_eq!(
            fixture.engine._resolve_pad_feedback(&message).unwrap(),
            Some(0)
        );
    }
    assert!(consumer.is_empty());
}

#[test]
fn armed_equal_source_successor_cannot_tag_old_callback_pcm_before_its_own_source_ack() {
    let fixture = accepted_fixture();
    let original = feedback(&fixture, AudioMessage::SampleStarted { id: 0 });
    let old_lifetime = fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap()
        .lifetime();
    let mut mixer = fixture.mixer();
    let old_pcm = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let generation = fixture.engine.loaded_source_generations.lock().unwrap()[0];
    let (message, adoption) = source_adoption(&fixture, 0, true);
    let successor_lifetime = fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap()
        .lifetime();
    assert_ne!(successor_lifetime, old_lifetime);
    assert_eq!(adoption.load(Ordering::Acquire), 0);
    assert_eq!(
        fixture.engine.loaded_source_generations.lock().unwrap()[0],
        generation
    );
    assert!(
        fixture.engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .same_source(&old_pcm)
    );
    let (producer, mut consumer) = RingBuffer::new(4);
    let mut tagged = ContentFeedbackProducer {
        producer,
        ownership: fixture.engine.input_runtime_ownership.clone(),
    };
    for signal in [
        AudioMessage::SampleStarted { id: 0 },
        AudioMessage::SampleStopped { id: 0 },
        AudioMessage::PadPeak { id: 0, peak: 0.375 },
        AudioMessage::PadPlayhead {
            id: 0,
            position_s: 1.25,
        },
    ] {
        tagged.push_audio_message(signal);
    }
    assert!(
        consumer.is_empty(),
        "old callback PCM has no successor adoption permission"
    );
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&original).unwrap(),
        None
    );
    process(&mut mixer, message, &mut tagged);
    assert_eq!(adoption.load(Ordering::Acquire), 2);
    tagged.push_audio_message(AudioMessage::SampleStarted { id: 0 });
    let successor = consumer
        .pop()
        .expect("the actual accepted successor can emit feedback");
    assert!(
        matches!(&successor, AudioMessage::ContentFeedback { lifetime, .. } if *lifetime == successor_lifetime)
    );
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&successor).unwrap(),
        Some(0)
    );
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&original).unwrap(),
        None
    );
}

#[test]
fn actual_tracked_copy_adoption_grants_its_distinct_target_feedback_without_a_cold_ack() {
    use crate::audio_engine::pad_copy::{CopyRtGuard, CopyRtIntent, PadCopyRtWork};
    use crate::audio_engine::prepared_source::PreparedSourcePermit;
    use crate::messages::{ResidentSourceView, StemMixMode};
    use flitzis_looper_analysis::tempo_acceptance::TimingIntent;

    const TARGET: usize = 36;
    let fixture = accepted_fixture();
    let source_feedback = feedback(&fixture, AudioMessage::SampleStarted { id: 0 });
    let ownership = &fixture.engine.input_runtime_ownership;
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let source = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let mut copied = source.clone();
    let view = source.residency.as_ref().unwrap();
    copied.residency = Some(Arc::new(ResidentSourceView {
        source: view.source.clone(),
        start_frame: view.start_frame,
        window_revision: view.window_revision + 1,
        context: view.context,
    }));
    let original = crate::audio_engine::input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    let lifetime = crate::audio_engine::content_lifetime::reserve().unwrap();
    ownership.reserve_copy(TARGET, lifetime).unwrap();
    let target_before = ownership.slot_snapshot(TARGET).unwrap();
    let key = target_before
        .for_lifetime(lifetime)
        .unwrap()
        .content_key()
        .unwrap();
    let source_epoch = fixture.engine.prepared_source_epoch(0).unwrap().clone();
    let target_epoch = fixture
        .engine
        .prepared_source_epoch(TARGET)
        .unwrap()
        .clone();
    let next_epoch = target_epoch.load(Ordering::Acquire) + 1;
    let phase = Arc::new(AtomicU8::new(0));
    let publication = PreparedSourcePermit::new(target_epoch.clone(), next_epoch);
    publication.mark_pending().unwrap();
    let guard = CopyRtGuard {
        source: 0,
        target: TARGET,
        source_binding: original.binding,
        source_generation: original.source_generation,
        captured_source_epoch: source_epoch.load(Ordering::Acquire),
        source_epoch,
        source_selection_fences: None,
        captured_target_epoch: target_epoch.load(Ordering::Acquire),
        target_epoch,
        target_authority: ownership.authority_revision(TARGET),
        target_runtime: ownership.runtime_revision(TARGET),
        target_placement: target_before,
        target_binding: None,
        target_source_generation: 0,
        target_selection_fences: std::array::from_fn(|_| None),
        reservation: lifetime,
        phase: phase.clone(),
        ownership: ownership.clone(),
        acknowledgements: fixture.engine.current_timing_acknowledgements.clone(),
    };
    let work = Box::new(PadCopyRtWork {
        content_key: key,
        target: TARGET,
        sample: copied,
        publication: publication.clone(),
        stems: None,
        selection: None,
        selection_fences: None,
        projection: None,
        source_generation: 9,
        content_lifetime: lifetime,
        prepared_epoch: next_epoch,
        authority_revision: guard.target_authority + 1,
        guard,
        intent: CopyRtIntent {
            pitch_intent: crate::audio_engine::pitch_intent::PitchIntent::default(),
            period_seconds: Some(0.5),
            origin_frame: -3.25,
            loop_start_frame: 8,
            loop_end_frame: Some(96),
            key_lock: false,
            timing_intent: TimingIntent::Manual,
            stem_mix_mode: StemMixMode::FullMix,
            source_version_hash: 0,
            enabled_mask: 0,
            gain_db: 0.0,
            eq_low_db: 0.0,
            eq_mid_db: 0.0,
            eq_high_db: 0.0,
        },
        retired_receipt: None,
        retired_sample: None,
        retired_stems: None,
        retired_outgoing: None,
    });
    let (producer, mut consumer) = RingBuffer::new(4);
    let mut tagged = ContentFeedbackProducer {
        producer,
        ownership: ownership.clone(),
    };
    tagged.push_audio_message(AudioMessage::SampleStarted { id: TARGET });
    assert!(
        consumer.is_empty(),
        "prepared Copy grants no callback source receipt"
    );
    process(&mut mixer, ControlMessage::PadCopy(work), &mut tagged);
    assert_eq!(
        phase.load(Ordering::Acquire),
        2,
        "actual guarded Copy callback ACK"
    );
    assert_eq!(publication.status(), "accepted");
    assert_eq!(
        ownership.slot_snapshot(TARGET).unwrap().content_key(),
        Some(key)
    );
    assert!(
        mixer.bank_for_measurement()[TARGET]
            .as_ref()
            .unwrap()
            .same_source(&source)
    );
    assert_ne!(
        ownership.cold_status(TARGET, 9),
        Some(2),
        "Copy adoption has its own source receipt; no cold admission was fabricated"
    );
    tagged.push_audio_message(AudioMessage::SampleStarted { id: TARGET });
    let copied_feedback = consumer
        .pop()
        .expect("actual Copy receipt permits target telemetry");
    assert!(
        matches!(&copied_feedback, AudioMessage::ContentFeedback { lifetime: observed, .. } if *observed == lifetime)
    );
    assert_eq!(
        fixture
            .engine
            ._resolve_pad_feedback(&copied_feedback)
            .unwrap(),
        Some(TARGET)
    );
    assert_eq!(
        fixture
            .engine
            ._resolve_pad_feedback(&source_feedback)
            .unwrap(),
        Some(0)
    );
    assert_eq!(
        fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap()
            .content_key(),
        original.binding.placement.unwrap().content_key()
    );
    ownership.release_copy(TARGET, lifetime).unwrap();
}

#[test]
fn actual_tracked_legacy_load_adoption_grants_feedback_only_after_its_own_lifetime_receipt() {
    let fixture = accepted_fixture();
    let old_feedback = feedback(&fixture, AudioMessage::SampleStarted { id: 0 });
    let ownership = &fixture.engine.input_runtime_ownership;
    let mut mixer = fixture.mixer();
    let sample = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let (generation, rate) = fixture.engine.loaded_source_generations.lock().unwrap()[0];
    let before = ownership.slot_snapshot(0).unwrap();
    let lifetime = crate::audio_engine::content_lifetime::reserve().unwrap();
    let write = ownership
        .prepare_slot_write_fixed(before, lifetime)
        .unwrap();
    let publication = ownership
        .publish_source_write(
            &write,
            &sample,
            rate,
            generation,
            ownership.authority_revision(0),
            None,
            false,
            || true,
        )
        .unwrap();
    let (producer, mut consumer) = RingBuffer::new(4);
    let mut tagged = ContentFeedbackProducer {
        producer,
        ownership: ownership.clone(),
    };
    tagged.push_audio_message(AudioMessage::SampleStarted { id: 0 });
    assert!(
        consumer.is_empty(),
        "armed legacy source is not adopted callback PCM"
    );
    process(
        &mut mixer,
        ControlMessage::LoadSample {
            id: 0,
            sample,
            publication: Some(publication),
        },
        &mut tagged,
    );
    tagged.push_audio_message(AudioMessage::SampleStarted { id: 0 });
    let current = consumer
        .pop()
        .expect("actual legacy LoadSample grants its own source receipt");
    assert!(
        matches!(&current, AudioMessage::ContentFeedback { lifetime: observed, .. } if *observed == lifetime)
    );
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&current).unwrap(),
        Some(0)
    );
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&old_feedback).unwrap(),
        None
    );
}

fn admitted_trigger(fixture: &LaunchFixture, exclusive: bool) -> ScheduledCommand {
    use crate::audio_engine::input_runtime_binding::{self, LaunchWriteKind};
    let binding = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    let ownership = &fixture.engine.input_runtime_ownership;
    let _requests = fixture.engine.pad_request_ids.lock().unwrap();
    assert!(ownership.admission_current(0, binding.binding));
    let write = ownership
        .prepare_launch_write(ownership.slot_snapshot(0).unwrap(), LaunchWriteKind::Admit)
        .unwrap();
    ownership
        .commit_launch_write(
            write,
            || ownership.admission_fields_current(0, binding.binding),
            || {},
        )
        .unwrap();
    ScheduledCommand::TriggerInputPad {
        id: 0,
        start_s: 16.0 / 48_000.0,
        end_s: Some(96.0 / 48_000.0),
        exclusive,
        binding: binding.binding,
        received_at_ns: 42,
        resident_control: None,
        launch_revision: write.previous_revision,
    }
}

fn revision(message: &AudioMessage) -> u64 {
    let AudioMessage::ContentFeedback {
        transport_revision, ..
    } = message
    else {
        panic!("actual production producer must retain original content tags");
    };
    *transport_revision
}

#[test]
fn admitted_started_and_stopped_keep_effect_revision_when_later_cancel_wins_publication_gap() {
    for start in [true, false] {
        let mut fixture = accepted_fixture();
        let mut mixer = fixture.mixer();
        let ownership = fixture.engine.input_runtime_ownership.clone();
        let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
        let (producer, mut consumer) = RingBuffer::new(4);
        let mut tagged = ContentFeedbackProducer {
            producer,
            ownership: ownership.clone(),
        };
        if start {
            let command = admitted_trigger(&fixture, false);
            let own_revision = ownership
                .transport_intent_for_cell(key.cell())
                .load(Ordering::Acquire);
            with_after_effect(
                move || ownership.cancel_launches(0),
                || {
                    assert!(matches!(execute_scheduled_command(&mut mixer, &mut TransportTimeline::new(48_000),
                    0, command, &mut tagged, &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement), ScheduledExecution::Complete));
                },
            );
            let message = consumer.pop().expect("actual admitted START receipt");
            assert_eq!(message.feedback_kind(), Some("started"));
            assert_eq!(revision(&message), own_revision);
            assert_eq!(
                fixture.engine._resolve_pad_feedback(&message).unwrap(),
                None
            );
            assert!(
                mixer
                    .voices
                    .iter()
                    .any(|voice| voice.active && voice.sample_id == 0)
            );
        } else {
            assert!(mixer.play_sample(0, 1.0));
            assert!(
                crate::audio_engine::input_runtime_binding::enqueue_stop_with_producer(
                    &ownership,
                    &mut fixture.producer.lock().unwrap(),
                    Some(0)
                )
            );
            let own_revision = ownership
                .transport_intent_for_cell(key.cell())
                .load(Ordering::Acquire);
            let message = fixture.consumer.pop().unwrap();
            with_after_effect(
                move || ownership.cancel_launches(0),
                || process(&mut mixer, message, &mut tagged),
            );
            let message = consumer.pop().expect("actual admitted STOP receipt");
            assert_eq!(message.feedback_kind(), Some("stopped"));
            assert_eq!(revision(&message), own_revision);
            assert_eq!(
                fixture.engine._resolve_pad_feedback(&message).unwrap(),
                None
            );
            assert!(
                !mixer
                    .voices
                    .iter()
                    .any(|voice| voice.active && voice.sample_id == 0)
            );
        }
        assert!(consumer.is_empty());
        let observed = feedback(&fixture, AudioMessage::PadPeak { id: 0, peak: 0.375 });
        assert_eq!(
            fixture.engine._resolve_pad_feedback(&observed).unwrap(),
            Some(0),
            "later telemetry observes its actual current intent, not an earlier effect receipt"
        );
    }
}

#[test]
fn global_started_and_coalesced_stopped_keep_each_original_revision_after_later_admission() {
    use crate::audio_engine::{global_playback_batch, input_runtime_binding};
    for start in [true, false] {
        let mut fixture = accepted_fixture();
        let mut mixer = fixture.mixer();
        if !start {
            for slot in [0, 1] {
                assert!(mixer.play_sample(slot, 1.0));
            }
        }
        let first = input_runtime_binding::capture(&fixture.engine, 0)
            .unwrap()
            .unwrap();
        let second = input_runtime_binding::capture(&fixture.engine, 1)
            .unwrap()
            .unwrap();
        let ticket = global_playback_batch::enqueue(
            &fixture.engine,
            &fixture.producer,
            vec![
                (&first, 16.0 / 48_000.0, Some(96.0 / 48_000.0)),
                (&second, 16.0 / 48_000.0, Some(96.0 / 48_000.0)),
            ],
            start,
            Some(42),
        )
        .unwrap();
        let ownership = fixture.engine.input_runtime_ownership.clone();
        let own_revision = ownership
            .transport_intent_for_cell(first.binding.placement.unwrap().cell())
            .load(Ordering::Acquire);
        let (producer, mut consumer) = RingBuffer::new(4);
        let mut tagged = ContentFeedbackProducer {
            producer,
            ownership: ownership.clone(),
        };
        let message = fixture.consumer.pop().unwrap();
        with_after_effect(
            move || ownership.cancel_launches(0),
            || process(&mut mixer, message, &mut tagged),
        );
        assert_eq!(ticket.publication_status(), "accepted");
        let events = [consumer.pop().unwrap(), consumer.pop().unwrap()];
        for message in events {
            assert_eq!(
                message.feedback_kind(),
                Some(if start { "started" } else { "stopped" })
            );
            assert_eq!(revision(&message), own_revision);
            assert_eq!(
                fixture.engine._resolve_pad_feedback(&message).unwrap(),
                if message.sample_id() == Some(0) {
                    None
                } else {
                    Some(1)
                }
            );
        }
        assert!(consumer.is_empty());
        assert_eq!(
            mixer.voices.iter().filter(|voice| voice.active).count(),
            if start { 2 } else { 0 }
        );
    }
}

#[test]
fn exclusive_coalesced_stop_keeps_its_receipt_when_transport_changes_after_actual_start() {
    let fixture = accepted_fixture();
    let mut mixer = fixture.mixer();
    assert!(mixer.play_sample(1, 1.0));
    let command = admitted_trigger(&fixture, true);
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let key = ownership.slot_snapshot(1).unwrap().content_key().unwrap();
    let own_revision = ownership
        .transport_intent_for_cell(key.cell())
        .load(Ordering::Acquire);
    let (producer, mut consumer) = RingBuffer::new(4);
    let mut tagged = ContentFeedbackProducer {
        producer,
        ownership: ownership.clone(),
    };
    with_after_effect(
        move || ownership.cancel_launches(1),
        || {
            assert!(matches!(
                execute_scheduled_command(
                    &mut mixer,
                    &mut TransportTimeline::new(48_000),
                    0,
                    command,
                    &mut tagged,
                    &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement
                ),
                ScheduledExecution::Complete
            ));
        },
    );
    let stopped = consumer.pop().unwrap();
    assert_eq!(stopped.sample_id(), Some(1));
    assert_eq!(stopped.feedback_kind(), Some("stopped"));
    assert_eq!(revision(&stopped), own_revision);
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&stopped).unwrap(),
        None
    );
    let started = consumer.pop().unwrap();
    assert_eq!(started.feedback_kind(), Some("started"));
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&started).unwrap(),
        Some(0)
    );
    assert!(
        mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    assert!(
        !mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 1)
    );
}

#[test]
fn cold_replacement_stopped_keeps_source_adoption_effect_revision_before_later_cancel() {
    let fixture = accepted_fixture();
    let mut mixer = fixture.mixer();
    let (mut message, adoption) = source_adoption(&fixture, 0, true);
    let ControlMessage::LoadColdSample {
        replace_assignment, ..
    } = &mut message
    else {
        unreachable!()
    };
    *replace_assignment = true;
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
    let own_revision = ownership
        .transport_intent_for_cell(key.cell())
        .load(Ordering::Acquire);
    let (producer, mut consumer) = RingBuffer::new(4);
    let mut tagged = ContentFeedbackProducer {
        producer,
        ownership: ownership.clone(),
    };
    with_after_effect(
        move || ownership.cancel_launches(0),
        || process(&mut mixer, message, &mut tagged),
    );
    assert_eq!(adoption.load(Ordering::Acquire), 2);
    let stopped = consumer.pop().unwrap();
    assert_eq!(stopped.feedback_kind(), Some("stopped"));
    assert_eq!(revision(&stopped), own_revision);
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&stopped).unwrap(),
        None
    );
}

#[test]
fn actual_peak_and_playhead_observations_keep_their_own_revision_before_publication() {
    let fixture = accepted_fixture();
    let mut mixer = fixture.mixer();
    assert!(mixer.play_sample(0, 1.0));
    let mut peaks = [0.0; NUM_SAMPLES];
    let mut activity = RtRenderPadActivity::default();
    mixer.render_rt_at_output_frame_tracking_pads(
        &mut [0.0; 32],
        &mut peaks,
        0,
        &mut activity,
        &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement,
    );
    assert!(peaks[0] > 0.0);
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
    let original_revision = ownership
        .transport_intent_for_cell(key.cell())
        .load(Ordering::Acquire);
    let (producer, mut consumer) = RingBuffer::new(4);
    let mut tagged = ContentFeedbackProducer {
        producer,
        ownership: ownership.clone(),
    };
    with_after_effect(
        move || ownership.cancel_launches(0),
        || {
            publish_pad_telemetry(&mut tagged, &mixer, &peaks, &activity, 32, 1, &mut 0);
        },
    );
    let peak = consumer.pop().expect("actual observed peak");
    assert_eq!(peak.feedback_kind(), Some("peak"));
    assert_eq!(peak.pad_peak(), Some(peaks[0].clamp(0.0, 1.0)));
    assert_eq!(revision(&peak), original_revision);
    assert_eq!(fixture.engine._resolve_pad_feedback(&peak).unwrap(), None);
    let playhead = consumer.pop().expect("actual later playhead observation");
    assert_eq!(playhead.feedback_kind(), Some("playhead"));
    assert_eq!(playhead.pad_playhead(), mixer.pad_playhead_seconds(0));
    assert!(revision(&playhead) > original_revision);
    assert_eq!(
        fixture.engine._resolve_pad_feedback(&playhead).unwrap(),
        Some(0)
    );
    assert!(consumer.is_empty());
}
