//! Genuine virtual callback adoption; recovered Copy accepts only fixed restored C.

use super::*;
use crate::audio_engine::audio_stream::process_control_message;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::input_mapping::launch_admission_tests::{LaunchFixture, RATE};
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, ControlMessage, TriggerQuantization};
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use std::sync::atomic::AtomicU8;

const TARGET: usize = 1;

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

fn mixer(fixture: &LaunchFixture) -> Box<RtMixer> {
    let mut mixer = crate::audio_engine::audio_stream::create_stream_mixer(
        1,
        RATE,
        fixture.engine.current_timing_acknowledgements.clone(),
        fixture.engine.input_runtime_ownership.clone(),
        fixture.engine.prepared_source_epochs.clone(),
    )
    .unwrap();
    for slot in [0, TARGET] {
        mixer.load_sample(
            slot,
            fixture.engine.sample_cache.lock().unwrap()[slot]
                .clone()
                .unwrap(),
        );
    }
    mixer
}

fn accepted_fixture() -> (LaunchFixture, Box<RtMixer>) {
    let fixture = LaunchFixture::new(4);
    let mut mixer = mixer(&fixture);
    for slot in [0, TARGET] {
        let (message, adoption) = source_adoption(&fixture, slot);
        process(&mut mixer, message);
        assert_eq!(adoption.load(Ordering::Acquire), 2);
    }
    (fixture, mixer)
}

fn target_binding(fixture: &LaunchFixture) -> InputRuntimePadBinding {
    input_runtime_binding::capture(&fixture.engine, TARGET)
        .unwrap()
        .unwrap()
}

#[test]
fn restored_copy_reserves_only_target_without_queue_request_or_layout_work() {
    let (fixture, _mixer) = accepted_fixture();
    let ownership = &fixture.engine.input_runtime_ownership;
    let snapshots = [0, TARGET].map(|slot| ownership.slot_snapshot(slot).unwrap());
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let acceptance = fixture
        .engine
        .capture_recovered_pad_copy(&target_binding(&fixture))
        .unwrap();
    assert!(
        fixture
            .engine
            .validate_recovered_pad_copy(&acceptance)
            .unwrap()
    );
    assert_eq!(ownership.copy_reservation(0), 0);
    assert_eq!(ownership.pair_reservation(0), 0);
    assert_eq!(ownership.pair_reservation(TARGET), 0);
    assert_eq!(ownership.copy_reservation(TARGET), acceptance.reservation);
    for original in snapshots {
        assert!(
            ownership
                .slot_snapshot(original.slot())
                .unwrap()
                .same_projection(original)
        );
    }
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    assert!(fixture.consumer.is_empty());
    assert!(
        fixture
            .engine
            .release_recovered_pad_copy(&acceptance)
            .unwrap()
    );
    assert!(
        fixture
            .engine
            .release_recovered_pad_copy(&acceptance)
            .unwrap()
    );
    assert!(!validate_recovered(&fixture.engine, &acceptance).unwrap());
    assert_eq!(ownership.copy_reservation(TARGET), 0);
    assert!(fixture.consumer.is_empty());
}

#[test]
fn target_mutations_reject_before_state_change_while_source_deletion_remains_admissible() {
    let (mut fixture, mut mixer) = accepted_fixture();
    let target = target_binding(&fixture);
    let snapshot = fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(TARGET)
        .unwrap();
    let acceptance = capture_recovered(&fixture.engine, &target).unwrap();
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    assert!(
        crate::audio_engine::cold_load::unload_for_producer(
            &fixture.engine,
            TARGET,
            &fixture.producer,
        )
        .is_err()
    );
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    assert!(fixture.consumer.is_empty());
    assert!(capture_recovered(&fixture.engine, &target).is_err());
    assert!(validate_recovered(&fixture.engine, &acceptance).unwrap());
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
    assert!(validate_recovered(&fixture.engine, &acceptance).unwrap());
    assert!(
        fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(TARGET)
            .unwrap()
            .same_projection(snapshot)
    );
    assert!(release_recovered(&fixture.engine, &acceptance).unwrap());
}

#[test]
fn recognized_target_acceptance_preserves_already_changed_or_deleted_source() {
    for deleted in [false, true] {
        let (mut fixture, mut mixer) = accepted_fixture();
        if deleted {
            crate::audio_engine::cold_load::unload_for_producer(
                &fixture.engine,
                0,
                &fixture.producer,
            )
            .unwrap();
            process(&mut mixer, fixture.consumer.pop().unwrap());
        } else {
            fixture.publish(0, 0.75, 99);
        }
        let source = fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap();
        let source_request = fixture.engine.pad_request_ids.lock().unwrap()[0];
        let acceptance = capture_recovered(&fixture.engine, &target_binding(&fixture)).unwrap();
        assert!(validate_recovered(&fixture.engine, &acceptance).unwrap());
        assert!(
            fixture
                .engine
                .input_runtime_ownership
                .slot_snapshot(0)
                .unwrap()
                .same_projection(source)
        );
        assert_eq!(
            fixture.engine.pad_request_ids.lock().unwrap()[0],
            source_request
        );
        assert_eq!(
            fixture.engine.input_runtime_ownership.copy_reservation(0),
            0
        );
        assert!(release_recovered(&fixture.engine, &acceptance).unwrap());
    }
}

#[test]
fn source_enqueue_and_other_participant_ack_do_not_grant_target_acceptance() {
    let fixture = LaunchFixture::new(4);
    let mut mixer = mixer(&fixture);
    let target = target_binding(&fixture);
    assert!(target.is_current());
    assert!(capture_recovered(&fixture.engine, &target).is_err());
    let (other, other_adoption) = source_adoption(&fixture, 0);
    process(&mut mixer, other);
    assert_eq!(other_adoption.load(Ordering::Acquire), 2);
    let (pending, target_adoption) = source_adoption(&fixture, TARGET);
    assert_eq!(target_adoption.load(Ordering::Acquire), 0);
    assert!(capture_recovered(&fixture.engine, &target_binding(&fixture)).is_err());
    assert_eq!(
        fixture
            .engine
            .input_runtime_ownership
            .copy_reservation(TARGET),
        0
    );
    process(&mut mixer, pending);
    assert_eq!(target_adoption.load(Ordering::Acquire), 2);
    let acceptance = capture_recovered(&fixture.engine, &target_binding(&fixture)).unwrap();
    assert!(validate_recovered(&fixture.engine, &acceptance).unwrap());
    assert!(release_recovered(&fixture.engine, &acceptance).unwrap());
}

#[test]
fn capture_rejects_foreign_stale_untracked_loading_and_unavailable_timing_bindings() {
    let (fixture, _mixer) = accepted_fixture();
    let (foreign, _foreign_mixer) = accepted_fixture();
    assert!(capture_recovered(&fixture.engine, &target_binding(&foreign)).is_err());
    let stale = target_binding(&fixture);
    fixture.publish(TARGET, -0.5, 8);
    assert!(capture_recovered(&fixture.engine, &stale).is_err());
    assert_eq!(
        fixture
            .engine
            .input_runtime_ownership
            .copy_reservation(TARGET),
        0
    );

    let (mut untracked, _mixer) = accepted_fixture();
    untracked.engine.input_runtime_ownership = Arc::default();
    untracked
        .engine
        .input_runtime_ownership
        .publish_content_lifetime(
            TARGET,
            crate::audio_engine::content_lifetime::reserve().unwrap(),
        );
    let binding = target_binding(&untracked);
    assert!(!untracked.engine.input_runtime_ownership.is_source_tracked());
    assert!(binding.is_current());
    assert!(capture_recovered(&untracked.engine, &binding).is_err());

    for cold in [false, true] {
        let (fixture, _mixer) = accepted_fixture();
        let binding = target_binding(&fixture);
        if cold {
            fixture.engine.cold_loading[TARGET].store(8, Ordering::Release);
        } else {
            fixture
                .engine
                .loading_sample_ids
                .lock()
                .unwrap()
                .insert(TARGET);
        }
        assert!(capture_recovered(&fixture.engine, &binding).is_err());
        assert_eq!(
            fixture
                .engine
                .input_runtime_ownership
                .copy_reservation(TARGET),
            0
        );
    }
    let (fixture, _mixer) = accepted_fixture();
    fixture.engine.timing_intents.lock().unwrap()[TARGET] = TimingIntent::Automatic;
    fixture
        .engine
        .input_runtime_ownership
        .set_timing_intent(TARGET, TimingIntent::Automatic);
    let binding = target_binding(&fixture);
    assert!(!binding.available());
    assert!(capture_recovered(&fixture.engine, &binding).is_err());
}

#[test]
fn current_target_guards_and_source_request_incarnation_remain_required_after_capture() {
    for case in 0..10 {
        let (fixture, _mixer) = accepted_fixture();
        let ownership = &fixture.engine.input_runtime_ownership;
        // Stage a fixture-only mutation before reserving the target. A fresh
        // request after capture must still fail its normal copy-reservation guard.
        let advance =
            (case == 6).then(|| ownership.prepare_source_request_advance(TARGET).unwrap());
        let acceptance = capture_recovered(&fixture.engine, &target_binding(&fixture)).unwrap();
        let placement = acceptance.participant.placement();
        match case {
            0 => {
                ownership
                    .runtime_for_cell(placement.cell())
                    .fetch_add(1, Ordering::AcqRel);
            }
            1 => {
                ownership
                    .authority_for_cell(placement.cell())
                    .fetch_add(1, Ordering::AcqRel);
            }
            2 => {
                ownership
                    .transport_intent_for_cell(placement.cell())
                    .fetch_add(1, Ordering::AcqRel);
            }
            3 => {
                fixture.engine.pad_request_ids.lock().unwrap()[TARGET] += 1;
            }
            4 => {
                fixture
                    .engine
                    .prepared_source_epoch(TARGET)
                    .unwrap()
                    .fetch_add(1, Ordering::AcqRel);
            }
            5 => {
                acceptance.participant.selection_fences[0]
                    .0
                    .fetch_add(1, Ordering::AcqRel);
            }
            6 => {
                assert!(ownership.prepare_source_request_advance(TARGET).is_err());
                advance.unwrap().commit();
            }
            7 => {
                ownership.set_timing_intent_for_cell(placement.cell(), TimingIntent::Legacy);
            }
            8 => {
                fixture
                    .engine
                    .current_timing_acknowledgements
                    .acknowledge_for_key(ownership, placement.content_key().unwrap(), 777);
            }
            9 => {
                let sample = fixture.engine.sample_cache.lock().unwrap()[TARGET]
                    .clone()
                    .unwrap();
                // Isolate a changed original source field without attempting a
                // new public mutation through the correctly fenced reservation.
                ownership.publish_source(TARGET, &sample, RATE, 9);
            }
            _ => unreachable!(),
        }
        assert!(
            !validate_recovered(&fixture.engine, &acceptance).unwrap(),
            "case {case}"
        );
        assert_eq!(ownership.copy_reservation(TARGET), acceptance.reservation);
        assert!(release_recovered(&fixture.engine, &acceptance).unwrap());
        assert_eq!(ownership.copy_reservation(TARGET), 0);
        assert!(fixture.consumer.is_empty());
    }
}

#[test]
fn foreign_and_replaced_reservations_cannot_release_a_later_target_owner() {
    let (fixture, _mixer) = accepted_fixture();
    let acceptance = capture_recovered(&fixture.engine, &target_binding(&fixture)).unwrap();
    let foreign = AudioEngine::new().unwrap();
    assert!(validate_recovered(&foreign, &acceptance).is_err());
    assert!(release_recovered(&foreign, &acceptance).is_err());
    let ownership = &fixture.engine.input_runtime_ownership;
    assert_eq!(ownership.copy_reservation(TARGET), acceptance.reservation);
    ownership
        .release_copy(TARGET, acceptance.reservation)
        .unwrap();
    let replacement = crate::audio_engine::content_lifetime::reserve().unwrap();
    ownership
        .reserve_copy_target(acceptance.participant.placement(), replacement)
        .unwrap();
    assert!(!validate_recovered(&fixture.engine, &acceptance).unwrap());
    assert!(release_recovered(&fixture.engine, &acceptance).is_err());
    assert_eq!(ownership.copy_reservation(TARGET), replacement);
    assert!(!acceptance.released.load(Ordering::Acquire));
    ownership.release_copy(TARGET, replacement).unwrap();
}

#[test]
fn stale_target_acceptance_retains_actual_pcm_custody_until_its_owner_drops() {
    let (fixture, mixer) = accepted_fixture();
    let sample = fixture.engine.sample_cache.lock().unwrap()[TARGET]
        .clone()
        .unwrap();
    let backing = Arc::downgrade(&sample.samples);
    drop(sample);
    let acceptance = capture_recovered(&fixture.engine, &target_binding(&fixture)).unwrap();
    fixture.engine.sample_cache.lock().unwrap()[TARGET] = None;
    drop(mixer);
    fixture
        .engine
        .prepared_source_epoch(TARGET)
        .unwrap()
        .fetch_add(1, Ordering::AcqRel);
    assert!(!validate_recovered(&fixture.engine, &acceptance).unwrap());
    assert!(backing.upgrade().is_some());
    assert!(release_recovered(&fixture.engine, &acceptance).unwrap());
    assert!(backing.upgrade().is_some());
    drop(acceptance);
    assert!(backing.upgrade().is_none());
}
