//! Original gesture source admission joins the existing request-owned Pair capture.

use super::*;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::{
    LoadedSourcePublication, SourcePitchInitialization, publish_loaded_sample_with_arm,
};

fn binding(fixture: &LaunchFixture, source: usize) -> InputRuntimePadBinding {
    input_runtime_binding::capture(&fixture.engine, source)
        .unwrap()
        .unwrap()
}

fn reject_without_reservation(fixture: &LaunchFixture, expected: &InputRuntimePadBinding) {
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let ownership = &fixture.engine.input_runtime_ownership;
    let snapshots = [0, 1].map(|slot| ownership.slot_snapshot(slot).unwrap());
    let error = fixture
        .engine
        ._capture_pad_pair_with_source_binding(0, 1, Some(expected))
        .err()
        .expect("the original source must be rejected");
    assert!(error.to_string().contains("pair source binding"));
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    for (slot, snapshot) in [0, 1].into_iter().zip(snapshots) {
        assert_eq!(ownership.pair_reservation(slot), 0);
        assert_eq!(ownership.copy_reservation(slot), 0);
        assert!(
            ownership
                .slot_snapshot(slot)
                .unwrap()
                .same_projection(snapshot)
        );
    }
    assert!(fixture.consumer.is_empty());
}

#[test]
fn original_same_owner_source_enters_existing_pair_and_releases_only_on_cancel() {
    let fixture = LaunchFixture::new(4);
    let source = binding(&fixture, 0);
    let captured = fixture
        .engine
        ._capture_pad_pair_with_source_binding(0, 1, Some(&source))
        .unwrap();
    let ownership = &fixture.engine.input_runtime_ownership;
    let reservation = ownership.pair_reservation(0);
    assert_ne!(reservation, 0);
    assert_eq!(ownership.pair_reservation(1), reservation);
    assert!(fixture.consumer.is_empty());
    assert!(cancel(&fixture.engine, &captured).unwrap());
    assert_eq!(ownership.pair_reservation(0), 0);
    assert_eq!(ownership.pair_reservation(1), 0);
}

#[test]
fn wrong_source_slot_binding_cannot_reserve_either_pair_participant() {
    let fixture = LaunchFixture::new(4);
    let wrong_slot = binding(&fixture, 1);
    assert!(wrong_slot.is_current());
    reject_without_reservation(&fixture, &wrong_slot);
}

#[test]
fn foreign_live_source_owner_cannot_reserve_either_pair_participant() {
    let fixture = LaunchFixture::new(4);
    let foreign = LaunchFixture::new(4);
    let expected = binding(&foreign, 0);
    assert!(expected.is_current());
    assert_eq!(
        expected.source_generation,
        binding(&fixture, 0).source_generation
    );
    reject_without_reservation(&fixture, &expected);
}

#[test]
fn actual_replaced_source_generation_cannot_retarget_original_pair_gesture() {
    let fixture = LaunchFixture::new(4);
    let expected = binding(&fixture, 0);
    fixture.publish(0, 0.75, expected.source_generation + 1);
    assert!(!expected.is_current());
    assert!(binding(&fixture, 0).is_current());
    reject_without_reservation(&fixture, &expected);
}

#[test]
fn same_pcm_digest_and_generation_replacement_cannot_retarget_original_pair_gesture() {
    let mut fixture = LaunchFixture::new(4);
    let expected = binding(&fixture, 0);
    let sample = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let ownership = &fixture.engine.input_runtime_ownership;
    let original = ownership.slot_snapshot(0).unwrap();
    {
        let _requests = fixture.engine.pad_request_ids.lock().unwrap();
        let mut generations = fixture.engine.loaded_source_generations.lock().unwrap();
        let mut digests = fixture.engine.loaded_source_digests.lock().unwrap();
        let digest = digests[0].clone().unwrap();
        publish_loaded_sample_with_arm(
            &fixture.producer,
            &fixture.engine.sample_cache,
            0,
            sample.clone(),
            LoadedSourcePublication {
                initial_mix: None,
                ownership,
                placement: original,
                generation: expected.source_generation,
                rate: 48_000,
                generation_slot: &mut generations[0],
                digest_slot: &mut digests[0],
                digest,
                cold: false,
                cold_epoch: None,
                cold_adoption: None,
                replace_assignment: true,
                loop_region: None,
                resident_cancelled: None,
                incoming_defaults: None,
                initial_pitch: SourcePitchInitialization::FreshNeutral,
                intent: None,
            },
            || Ok(()),
        )
        .unwrap();
    }
    assert!(matches!(
        fixture.consumer.pop().unwrap(),
        ControlMessage::LoadSample { .. }
    ));
    let current = binding(&fixture, 0);
    assert!(current.is_current());
    assert_eq!(current.source_generation, expected.source_generation);
    assert_eq!(
        current.binding.source_address,
        expected.binding.source_address
    );
    assert!(Arc::ptr_eq(
        &fixture.engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples,
        &sample.samples,
    ));
    assert_ne!(
        ownership.slot_snapshot(0).unwrap().lifetime(),
        original.lifetime()
    );
    assert!(!expected.is_current());
    reject_without_reservation(&fixture, &expected);
}
