use super::*;
use crate::audio_engine::hold_action::{HoldWorkKind, NativeHoldProjection};
use crate::audio_engine::input_mapping::launch_admission_tests::{LaunchFixture, OTHER, dispatch};
use crate::audio_engine::resident_relocation::control_publication_tests::PendingWriter;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, TriggerQuantization};
use std::sync::atomic::Ordering;

fn transport_revision(fixture: &LaunchFixture, slot: usize) -> u64 {
    let ownership = &fixture.engine.input_runtime_ownership;
    ownership
        .transport_intent_for_cell(ownership.slot_snapshot(slot).unwrap().cell())
        .load(Ordering::Acquire)
}

fn pop_hold(fixture: &mut LaunchFixture) -> Box<HoldActionRtWork> {
    match fixture.consumer.pop().unwrap() {
        ControlMessage::HoldAction(work) => work,
        message => panic!("expected owning Hold work, got {message:?}"),
    }
}

fn apply(
    mixer: &mut crate::audio_engine::mixer::RtMixer,
    work: Box<HoldActionRtWork>,
) -> Vec<AudioMessage> {
    let mut scheduler = FixedCapacityScheduler::<8>::new();
    let mut transport = TransportTimeline::new(48_000);
    let mut quantization = TriggerQuantization::Immediate;
    let mut feedback = Vec::new();
    dispatch(
        mixer,
        ControlMessage::HoldAction(work),
        &mut scheduler,
        &mut transport,
        &mut quantization,
        &mut feedback,
    );
    feedback
}

fn original_projection(ticket: &HoldActionTicket) -> NativeHoldProjection {
    ticket.action.projection()
}

#[test]
fn pause_full_queue_leaves_original_transport_source_and_launch_flags_unchanged() {
    let mut fixture = LaunchFixture::new(1);
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let placement = ownership.slot_snapshot(0).unwrap();
    let transport = transport_revision(&fixture, 0);
    let launch = ownership.launch_revision(0);
    let request = fixture.engine.pad_request_ids.lock().unwrap()[0];
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    assert!(pause_for_producer(&fixture.engine, &fixture.producer, 0, 11, 123).is_err());
    assert_eq!(transport_revision(&fixture, 0), transport);
    assert_eq!(ownership.launch_revision(0), launch);
    assert!(ownership.admitted_launch_ids().is_empty());
    assert_eq!(fixture.engine.pad_request_ids.lock().unwrap()[0], request);
    assert_eq!(ownership.slot_snapshot(0).unwrap(), placement);
    assert!(ownership.content_key_current(placement.content_key().unwrap()));
    assert!(matches!(
        fixture.consumer.pop().unwrap(),
        ControlMessage::Ping()
    ));
    assert!(fixture.consumer.is_empty());
}

#[test]
fn pause_queue_admission_publishes_its_own_revision_before_actual_effect_ack() {
    let mut fixture = LaunchFixture::new(4);
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let original = ownership.slot_snapshot(0).unwrap();
    let previous = transport_revision(&fixture, 0);
    let launch = ownership.launch_revision(0);
    let ticket = pause_for_producer(&fixture.engine, &fixture.producer, 0, 12, 987).unwrap();
    assert_eq!(ticket.action.state(), HoldActionState::Admitted);
    assert!(ticket.action.effect().is_none());
    assert_eq!(ticket.action.transport_revision(), previous + 1);
    assert_eq!(transport_revision(&fixture, 0), previous + 1);
    assert_eq!(ownership.launch_revision(0), launch);
    assert!(ownership.admitted_launch_ids().is_empty());
    assert_eq!(ticket.action.captured(), original);
    let work = pop_hold(&mut fixture);
    assert_eq!(work.kind, HoldWorkKind::Pause);
    assert!(Arc::ptr_eq(&work.action, &ticket.action));
    assert_eq!(work.action.key(), original.content_key().unwrap());
    assert_eq!(original_projection(&ticket).physical_press_id, 12);
    assert!(fixture.consumer.is_empty());
}

#[test]
fn early_release_retries_same_ticket_after_actual_queue_pressure_then_resumes_once() {
    let mut fixture = LaunchFixture::new(1);
    let mut mixer = fixture.mixer();
    assert!(mixer.play_sample(0, 1.0));
    assert!(mixer.play_sample(OTHER, 1.0));
    let held_lane = mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(0))
        .unwrap();
    let other_lane = mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(OTHER))
        .unwrap();
    let ticket = pause_for_producer(&fixture.engine, &fixture.producer, 0, 13, 42).unwrap();
    let identity = ticket.action.action_id();
    let revision = transport_revision(&fixture, 0);
    for _ in 0..3 {
        assert!(!release_for_producer(&fixture.engine, &fixture.producer, &ticket).unwrap());
        assert!(ticket.action.release_requested());
        assert!(!ticket.action.release_admitted());
        assert_eq!(ticket.action.action_id(), identity);
        assert_eq!(ticket.action.state(), HoldActionState::Admitted);
        assert!(ticket.action.effect().is_none());
        assert_eq!(transport_revision(&fixture, 0), revision);
    }
    let pause = pop_hold(&mut fixture);
    assert!(Arc::ptr_eq(&pause.action, &ticket.action));
    assert!(release_for_producer(&fixture.engine, &fixture.producer, &ticket).unwrap());
    assert!(ticket.action.release_admitted());
    // Queue remains full with exactly that one release. Duplicate observation is idempotent.
    assert!(release_for_producer(&fixture.engine, &fixture.producer, &ticket).unwrap());
    let release = pop_hold(&mut fixture);
    assert_eq!(release.kind, HoldWorkKind::Release);
    assert!(Arc::ptr_eq(&release.action, &ticket.action));
    assert_eq!(transport_revision(&fixture, 0), revision);
    assert!(fixture.consumer.is_empty());
    assert!(apply(&mut mixer, pause).is_empty());
    assert_eq!(ticket.action.state(), HoldActionState::PausedOwned);
    assert!(mixer.voices[held_lane].paused);
    assert!(!mixer.voices[other_lane].paused);
    assert!(apply(&mut mixer, release).is_empty());
    assert_eq!(ticket.action.state(), HoldActionState::Resumed);
    assert!(!mixer.voices[held_lane].paused);
    assert!(!mixer.voices[other_lane].paused);
    let terminal = original_projection(&ticket);
    assert!(release_for_producer(&fixture.engine, &fixture.producer, &ticket).unwrap());
    assert_eq!(original_projection(&ticket), terminal);
    assert!(fixture.consumer.is_empty());
}

#[test]
fn foreign_ticket_does_not_publish_even_a_release_requested_flag() {
    let mut original = LaunchFixture::new(2);
    let foreign = LaunchFixture::new(2);
    let ticket = pause_for_producer(&original.engine, &original.producer, 0, 14, 43).unwrap();
    let before = original_projection(&ticket);
    let other_revision = transport_revision(&foreign, 0);
    assert!(release_for_producer(&foreign.engine, &foreign.producer, &ticket).is_err());
    assert_eq!(original_projection(&ticket), before);
    assert!(!ticket.action.release_requested());
    assert!(!ticket.action.release_admitted());
    assert_eq!(transport_revision(&foreign, 0), other_revision);
    assert!(foreign.consumer.is_empty());
    assert_eq!(pop_hold(&mut original).kind, HoldWorkKind::Pause);
}

#[test]
fn release_during_native_pending_retains_original_owner_and_retries_after_marker() {
    let mut fixture = LaunchFixture::new(4);
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let ticket = pause_for_producer(&fixture.engine, &fixture.producer, 0, 15, 44).unwrap();
    let pause = pop_hold(&mut fixture);
    let original = ticket.action.captured();
    let revision = ticket.action.transport_revision();
    let pending = PendingWriter::hold(ownership.clone());
    assert!(!release_for_producer(&fixture.engine, &fixture.producer, &ticket).unwrap());
    assert!(ticket.action.release_requested());
    assert!(!ticket.action.release_admitted());
    assert_eq!(ticket.action.captured(), original);
    assert_eq!(ticket.action.transport_revision(), revision);
    assert!(fixture.consumer.is_empty());
    pending.complete();
    assert!(release_for_producer(&fixture.engine, &fixture.producer, &ticket).unwrap());
    let release = pop_hold(&mut fixture);
    assert!(Arc::ptr_eq(&pause.action, &release.action));
    assert_eq!(transport_revision(&fixture, 0), revision);
}

#[test]
fn pair_reservation_fences_new_pause_but_allows_original_matching_release() {
    let mut fixture = LaunchFixture::new(4);
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let ticket = pause_for_producer(&fixture.engine, &fixture.producer, 0, 16, 45).unwrap();
    let pause = pop_hold(&mut fixture);
    let participants = [
        ownership.slot_snapshot(0).unwrap(),
        ownership.slot_snapshot(OTHER).unwrap(),
    ];
    let revision = transport_revision(&fixture, 0);
    ownership.reserve_pair(&participants, 777).unwrap();
    assert!(pause_for_producer(&fixture.engine, &fixture.producer, 0, 17, 46).is_err());
    assert_eq!(transport_revision(&fixture, 0), revision);
    assert!(fixture.consumer.is_empty());
    assert!(release_for_producer(&fixture.engine, &fixture.producer, &ticket).unwrap());
    let release = pop_hold(&mut fixture);
    assert!(Arc::ptr_eq(&pause.action, &release.action));
    assert_eq!(transport_revision(&fixture, 0), revision);
    assert_eq!(ownership.pair_reservation(0), 777);
    ownership.release_pair(&participants, 777).unwrap();
}

#[test]
fn removed_lifetime_release_queues_original_retirement_without_claiming_replacement() {
    let mut fixture = LaunchFixture::new(4);
    let mut mixer = fixture.mixer();
    assert!(mixer.play_sample(0, 1.0));
    let ticket = pause_for_producer(&fixture.engine, &fixture.producer, 0, 18, 47).unwrap();
    let pause = pop_hold(&mut fixture);
    fixture.publish(0, 0.75, 17);
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let replacement = ownership.slot_snapshot(0).unwrap();
    let replacement_revision = transport_revision(&fixture, 0);
    assert_ne!(replacement.content_key(), Some(ticket.action.key()));
    assert_eq!(ticket.action.state(), HoldActionState::Admitted);
    assert!(release_for_producer(&fixture.engine, &fixture.producer, &ticket).unwrap());
    assert_eq!(
        ticket.action.state(),
        HoldActionState::Admitted,
        "only callback may publish terminal Native ACK"
    );
    assert_eq!(ownership.slot_snapshot(0).unwrap(), replacement);
    assert_eq!(transport_revision(&fixture, 0), replacement_revision);
    let release = pop_hold(&mut fixture);
    assert_eq!(release.action.key(), pause.action.key());
    assert!(Arc::ptr_eq(&release.action, &ticket.action));
    mixer.stop_sample(0);
    mixer.load_sample(
        0,
        fixture.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap(),
    );
    assert!(mixer.play_sample(0, 1.0));
    let replacement_lane = mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(0))
        .unwrap();
    let generation = mixer.voices[replacement_lane].generation;
    assert!(apply(&mut mixer, pause).is_empty());
    assert_eq!(ticket.action.state(), HoldActionState::Retired);
    assert!(apply(&mut mixer, release).is_empty());
    assert_eq!(ticket.action.state(), HoldActionState::Retired);
    assert!(!mixer.voices[replacement_lane].paused);
    assert_eq!(mixer.voices[replacement_lane].generation, generation);
    assert_eq!(ownership.slot_snapshot(0).unwrap(), replacement);
}

#[test]
fn superseded_transport_still_settles_original_release_without_rewriting_revision() {
    let mut fixture = LaunchFixture::new(4);
    let mut mixer = fixture.mixer();
    assert!(mixer.play_sample(0, 1.0));
    let ticket = pause_for_producer(&fixture.engine, &fixture.producer, 0, 19, 48).unwrap();
    let pause = pop_hold(&mut fixture);
    apply(&mut mixer, pause);
    assert_eq!(ticket.action.state(), HoldActionState::PausedOwned);
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let later = ownership
        .prepare_transport_intent(ownership.slot_snapshot(0).unwrap())
        .unwrap();
    ownership
        .commit_transport_intent(later, || true, || {})
        .unwrap();
    let revision = transport_revision(&fixture, 0);
    assert_ne!(ticket.action.transport_revision(), revision);
    assert!(release_for_producer(&fixture.engine, &fixture.producer, &ticket).unwrap());
    assert_eq!(transport_revision(&fixture, 0), revision);
    let release = pop_hold(&mut fixture);
    apply(&mut mixer, release);
    assert_eq!(ticket.action.state(), HoldActionState::Superseded);
    assert!(
        mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0) && voice.paused)
    );
}

#[test]
fn transport_exhaustion_rejects_pause_without_wrap_or_queue_publication() {
    let fixture = LaunchFixture::new(4);
    let ownership = &fixture.engine.input_runtime_ownership;
    let captured = ownership.slot_snapshot(0).unwrap();
    ownership
        .transport_intent_for_cell(captured.cell())
        .store(u64::MAX, Ordering::Release);
    assert!(pause_for_producer(&fixture.engine, &fixture.producer, 0, 20, 49).is_err());
    assert_eq!(transport_revision(&fixture, 0), u64::MAX);
    assert_eq!(ownership.slot_snapshot(0).unwrap(), captured);
    assert!(fixture.consumer.is_empty());
    assert!(ownership.admitted_launch_ids().is_empty());
}
