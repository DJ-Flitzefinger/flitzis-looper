//! Actual virtual Native source ACKs; recovery capture itself emits no callback/layout work.

use super::*;
use crate::audio_engine::audio_stream::process_control_message;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::input_mapping::launch_admission_tests::{LaunchFixture, RATE};
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, ControlMessage, TriggerQuantization};
use std::sync::atomic::AtomicU8;

fn process(mixer: &mut RtMixer, message: ControlMessage) {
    process_control_message(
        message,
        &mut FixedCapacityScheduler::<8>::new(),
        0,
        &mut TriggerQuantization::Immediate,
        &mut TransportTimeline::new(RATE),
        mixer,
        &mut Vec::<AudioMessage>::new(),
        &mut ImmediateAudioBufferRetirement,
    );
}

fn source_adoption(fixture: &LaunchFixture, slot: usize) -> (ControlMessage, Arc<AtomicU8>) {
    let ownership = &fixture.engine.input_runtime_ownership;
    let sample = fixture.engine.sample_cache.lock().unwrap()[slot]
        .clone()
        .unwrap();
    let (generation, rate) = fixture.engine.loaded_source_generations.lock().unwrap()[slot];
    let snapshot = ownership.slot_snapshot(slot).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(snapshot, snapshot.lifetime())
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
            initial_mix: None,
            publication,
            id: slot,
            sample,
            source_generation: generation,
            content_lifetime: snapshot.lifetime(),
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

fn accepted_fixture(empty_source: bool) -> LaunchFixture {
    let mut fixture = LaunchFixture::new(4);
    let mut mixer = fixture.mixer();
    for slot in [0, 1] {
        let (message, adoption) = source_adoption(&fixture, slot);
        process(&mut mixer, message);
        assert_eq!(adoption.load(Ordering::Acquire), 2);
    }
    if empty_source {
        crate::audio_engine::cold_load::unload_for_producer(&fixture.engine, 0, &fixture.producer)
            .unwrap();
        process(&mut mixer, fixture.consumer.pop().unwrap());
        assert_eq!(
            fixture
                .engine
                .input_runtime_ownership
                .slot_snapshot(0)
                .unwrap()
                .lifetime(),
            0
        );
    }
    fixture
}

#[test]
fn recovered_move_and_swap_afterimages_reserve_fresh_accepted_owners_without_layout_work() {
    for empty_source in [false, true] {
        let fixture = accepted_fixture(empty_source);
        let ownership = &fixture.engine.input_runtime_ownership;
        let before = [
            ownership.slot_snapshot(0).unwrap(),
            ownership.slot_snapshot(1).unwrap(),
        ];
        let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
        let acceptance = capture_recovered(&fixture.engine, 0, 1).unwrap();
        assert!(validate_recovered(&fixture.engine, &acceptance).unwrap());
        assert_ne!(ownership.pair_reservation(0), 0);
        assert_eq!(ownership.pair_reservation(0), ownership.pair_reservation(1));
        for (slot, original) in before.into_iter().enumerate() {
            assert!(
                ownership
                    .slot_snapshot(slot)
                    .unwrap()
                    .same_projection(original)
            );
        }
        assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
        assert!(fixture.consumer.is_empty());
        assert!(release_recovered(&fixture.engine, &acceptance).unwrap());
        assert!(release_recovered(&fixture.engine, &acceptance).unwrap());
        assert!(!validate_recovered(&fixture.engine, &acceptance).unwrap());
        assert_eq!(ownership.pair_reservation(0), 0);
        assert_eq!(ownership.pair_reservation(1), 0);
    }
}

#[test]
fn recovery_rejects_control_available_sources_without_their_actual_callback_ack() {
    let fixture = LaunchFixture::new(4);
    assert!(
        input_runtime_binding::capture(&fixture.engine, 0)
            .unwrap()
            .unwrap()
            .is_current()
    );
    assert!(capture_recovered(&fixture.engine, 0, 1).is_err());
    let mut mixer = fixture.mixer();
    let (pending, _) = source_adoption(&fixture, 0);
    assert!(capture_recovered(&fixture.engine, 0, 1).is_err());
    process(&mut mixer, pending);
    assert!(
        capture_recovered(&fixture.engine, 0, 1).is_err(),
        "target still lacks actual Native acceptance"
    );
    let (target, _) = source_adoption(&fixture, 1);
    process(&mut mixer, target);
    let acceptance = capture_recovered(&fixture.engine, 0, 1).unwrap();
    assert!(validate_recovered(&fixture.engine, &acceptance).unwrap());
    assert!(release_recovered(&fixture.engine, &acceptance).unwrap());
}

#[test]
fn stale_and_foreign_recovery_never_grant_acceptance_and_release_only_the_same_reservation() {
    for case in 0..4 {
        let fixture = accepted_fixture(false);
        let acceptance = capture_recovered(&fixture.engine, 0, 1).unwrap();
        let ownership = &fixture.engine.input_runtime_ownership;
        let target = acceptance.participants[1].placement();
        match case {
            0 => {
                ownership
                    .runtime_for_cell(target.cell())
                    .fetch_add(1, Ordering::AcqRel);
            }
            1 => {
                ownership
                    .transport_intent_for_cell(target.cell())
                    .fetch_add(1, Ordering::AcqRel);
            }
            2 => {
                fixture.engine.pad_request_ids.lock().unwrap()[1] += 1;
            }
            3 => {
                fixture
                    .engine
                    .prepared_source_epoch(1)
                    .unwrap()
                    .fetch_add(1, Ordering::AcqRel);
            }
            _ => unreachable!(),
        }
        assert!(
            !validate_recovered(&fixture.engine, &acceptance).unwrap(),
            "case {case}"
        );
        let foreign = AudioEngine::new().unwrap();
        assert!(validate_recovered(&foreign, &acceptance).is_err());
        assert!(release_recovered(&foreign, &acceptance).is_err());
        assert_eq!(ownership.pair_reservation(1), acceptance.reservation);
        assert!(release_recovered(&fixture.engine, &acceptance).unwrap());
        assert_eq!(ownership.pair_reservation(0), 0);
        assert_eq!(ownership.pair_reservation(1), 0);
        assert!(fixture.consumer.is_empty());
    }
}
