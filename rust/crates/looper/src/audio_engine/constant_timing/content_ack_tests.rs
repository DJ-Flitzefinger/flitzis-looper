//! Real atomic ACK owners exercise incarnation reuse and placement independently of PCM equality.

use super::*;

fn source() -> SampleBuffer {
    SampleBuffer {
        channels: 1,
        samples: Arc::from([0.25; 32]),
        residency: None,
    }
    .with_complete_source(48_000)
}

fn assign(
    owner: &InputRuntimeOwnership,
    id: usize,
    sample: &SampleBuffer,
    generation: u64,
) -> ContentKey {
    owner.publish_content_lifetime(id, super::super::content_lifetime::reserve().unwrap());
    owner.publish_source(id, sample, 48_000, generation);
    owner.slot_snapshot(id).unwrap().content_key().unwrap()
}

#[test]
fn accepted_ack_follows_its_cell_while_the_old_slot_snapshot_loses_admission() {
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let acknowledgements = CurrentTimingAcknowledgements::default();
    let key = assign(&owner, 0, &source(), 1);
    let original = owner.slot_snapshot(0).unwrap();
    let target = owner.slot_snapshot(36).unwrap();
    assert!(acknowledgements.acknowledge_for_key(&owner, key, 7));
    let prepared = owner.prepare_placement_pair(original, target).unwrap();
    owner
        .commit_placement_pair(&prepared, true, || true, || {})
        .unwrap();
    assert!(owner.content_key_current(key));
    assert!(!owner.slot_snapshot_current(original));
    assert_eq!(owner.slot_snapshot(36).unwrap().content_key(), Some(key));
    assert_eq!(acknowledgements.current_for_key(&owner, key), 7);
    assert_eq!(acknowledgements.current_for_slot(&owner, 36), 7);
    assert_eq!(acknowledgements.current_for_slot(&owner, 0), 0);
}

#[test]
fn new_incarnation_low_epoch_is_not_revoked_by_the_previous_high_counter() {
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let acknowledgements = CurrentTimingAcknowledgements::default();
    let sample = source();
    let previous = assign(&owner, 0, &sample, 1);
    assert!(acknowledgements.acknowledge_for_key(&owner, previous, 80));
    assert!(acknowledgements.revoke_for_key(&owner, previous, 100));
    assert_eq!(acknowledgements.current_for_key(&owner, previous), 0);
    let current = assign(&owner, 0, &sample, 2);
    assert_ne!(previous, current);
    assert_eq!(acknowledgements.current_for_key(&owner, current), 0);
    assert!(acknowledgements.acknowledge_for_key(&owner, current, 1));
    assert_eq!(acknowledgements.current_for_key(&owner, current), 1);
    assert!(!acknowledgements.acknowledge_for_key(&owner, previous, 101));
    assert!(!acknowledgements.revoke_for_key(&owner, previous, 200));
    assert_eq!(acknowledgements.current_for_key(&owner, current), 1);
}

#[test]
fn revocation_before_first_ack_survives_new_incarnation_ack_publication() {
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let acknowledgements = CurrentTimingAcknowledgements::default();
    let sample = source();
    let previous = assign(&owner, 0, &sample, 1);
    assert!(acknowledgements.revoke_for_key(&owner, previous, 100));
    let current = assign(&owner, 0, &sample, 2);
    assert!(acknowledgements.revoke_for_key(&owner, current, 2));
    assert!(acknowledgements.acknowledge_for_key(&owner, current, 1));
    assert_eq!(acknowledgements.current_for_key(&owner, current), 0);
    assert!(acknowledgements.acknowledge_for_key(&owner, current, 2));
    assert_eq!(acknowledgements.current_for_key(&owner, current), 0);
    assert!(acknowledgements.acknowledge_for_key(&owner, current, 3));
    assert_eq!(acknowledgements.current_for_key(&owner, current), 3);
    assert!(acknowledgements.revoke_for_key(&owner, current, 1));
    assert_eq!(acknowledgements.current_for_key(&owner, current), 3);
}

#[test]
fn late_clear_and_foreign_owner_cannot_mutate_the_current_ack() {
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let foreign = Arc::new(InputRuntimeOwnership::tracked());
    let acknowledgements = CurrentTimingAcknowledgements::default();
    let sample = source();
    let previous = assign(&owner, 0, &sample, 1);
    let current = assign(&owner, 0, &sample, 2);
    assert!(acknowledgements.acknowledge_for_key(&owner, current, 1));
    assert!(!acknowledgements.clear_for_key(&owner, previous));
    // Equal synthetic scalars still cannot cross actual retained engine ownership.
    foreign.publish_content_lifetime(0, current.lifetime());
    foreign.publish_source(0, &sample, 48_000, 2);
    assert!(!acknowledgements.clear_for_key(&foreign, current));
    assert!(!acknowledgements.revoke_for_key(&foreign, current, 100));
    assert!(!acknowledgements.acknowledge_for_key(&foreign, current, 100));
    assert_eq!(acknowledgements.current_for_key(&foreign, current), 0);
    assert_eq!(acknowledgements.current_for_key(&owner, current), 1);
    assert!(acknowledgements.clear_for_key(&owner, current));
    assert_eq!(acknowledgements.current_for_key(&owner, current), 0);
}

#[test]
fn in_progress_pair_publication_does_not_revoke_an_accepted_content_ack() {
    let owner = Arc::new(InputRuntimeOwnership::tracked());
    let acknowledgements = CurrentTimingAcknowledgements::default();
    let key = assign(&owner, 0, &source(), 1);
    assert!(acknowledgements.acknowledge_for_key(&owner, key, 7));
    let prepared = owner
        .prepare_placement_pair(
            owner.slot_snapshot(0).unwrap(),
            owner.slot_snapshot(36).unwrap(),
        )
        .unwrap();
    owner
        .commit_placement_pair(
            &prepared,
            true,
            || {
                assert_eq!(acknowledgements.current_for_key(&owner, key), 7);
                assert_eq!(acknowledgements.current_for_slot(&owner, 0), 0);
                true
            },
            || {
                assert_eq!(acknowledgements.current_for_key(&owner, key), 7);
            },
        )
        .unwrap();
    assert_eq!(acknowledgements.current_for_slot(&owner, 36), 7);
}
