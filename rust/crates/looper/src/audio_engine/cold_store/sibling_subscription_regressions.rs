//! Independent assignment authority must survive another user's cleanup request.
//! Register this as a child of warm_tests so every case uses its real sealed fixture.
use super::*;

fn owner_sets(lease: &CommittedColdLease) -> (HashSet<u64>, HashSet<u64>) {
    (
        lease.cache.durable_assignments.lock().unwrap().clone(),
        lease._original.durable_assignments.lock().unwrap().clone(),
    )
}

fn verify_new_child(
    survivor: &CommittedColdLease,
    sample: &SampleBuffer,
    expected_owners: usize,
) -> CommittedColdLease {
    assert!(!survivor.assignment_retired());
    let child = survivor
        .subscribe_verified()
        .expect("a surviving active assignment can admit its own independent child");
    assert_ne!(child.assignment_id(), survivor.assignment_id());
    assert!(!child.created_original());
    assert!(!child.created_cache());
    assert_eq!(child.cache_assignment_count(), expected_owners);
    let (cache, original) = owner_sets(&child);
    assert_eq!(cache, original);
    assert!(cache.contains(&survivor.assignment_id()));
    assert!(cache.contains(&child.assignment_id()));
    child.verify_original_for_offline().unwrap();
    let window = child
        .read_window_cancellable(
            sample,
            17,
            128,
            2,
            crate::messages::ResidentContext::FiniteLoop,
            PCM_LIMIT_BYTES,
            &|| false,
        )
        .unwrap();
    assert_eq!(window.resident_start(), 17);
    assert_eq!(window.resident_end(), 128);
    assert_eq!(
        window.samples.as_ref(),
        &sample.samples[17 * sample.channels..128 * sample.channels]
    );
    child.open_complete_reader(&window).unwrap();
    child
}

#[test]
fn active_sibling_can_subscribe_after_peer_requests_shared_cache_retirement() {
    let fixture = Fixture::new(44_100, 1);
    let (_producer_pcm, producer) = fixture.cold(48_000, 2);
    let (_peer_pcm, peer) = fixture.warm(&fixture.source, true, 48_000, 2);
    let (survivor_pcm, survivor) = fixture.warm(&fixture.source, true, 48_000, 2);
    assert_eq!(survivor.cache_assignment_count(), 3);
    assert!(!peer.created_cache());
    peer.retire_cache();
    assert!(peer.assignment_retired());
    assert!(survivor.cache.retired.load(Ordering::Acquire));
    assert_eq!(survivor.cache_assignment_count(), 2);
    let before = owner_sets(&survivor);
    assert!(peer.subscribe_verified().is_err());
    assert_eq!(owner_sets(&survivor), before);

    let child = verify_new_child(&survivor, &survivor_pcm, 3);
    child.rollback_unadopted_original();
    child.rollback_unadopted_cache();
    assert!(child.assignment_retired());
    assert!(!survivor.assignment_retired());
    assert_eq!(owner_sets(&survivor), before);
    assert!(survivor.original_path.exists());
    assert!(survivor.cache_path.join("playback.f32le").exists());
    survivor.release_preparation_assignment();
    producer.release_preparation_assignment();
}

#[test]
fn active_sibling_can_subscribe_after_creator_rolls_back_only_its_assignment() {
    let fixture = Fixture::new(44_100, 1);
    let (_producer_pcm, producer) = fixture.cold(48_000, 2);
    let (survivor_pcm, survivor) = fixture.warm(&fixture.source, true, 48_000, 2);
    assert!(producer.created_original());
    assert!(producer.created_cache());
    assert_eq!(survivor.cache_assignment_count(), 2);
    producer.rollback_unadopted_original();
    producer.rollback_unadopted_cache();
    assert!(producer.assignment_retired());
    assert!(survivor.cache.retired.load(Ordering::Acquire));
    assert!(survivor._original.rollback.load(Ordering::Acquire));
    assert_eq!(survivor.cache_assignment_count(), 1);
    let before = owner_sets(&survivor);
    assert!(producer.subscribe_verified().is_err());
    assert_eq!(owner_sets(&survivor), before);

    let child = verify_new_child(&survivor, &survivor_pcm, 2);
    // Durable acceptance cancels the creator's pending original rollback through
    // its existing production gate; subscriber admission does not fabricate ACK.
    admit_original_owner(&survivor.original_path).unwrap();
    assert!(!survivor._original.rollback.load(Ordering::Acquire));
    child.release_preparation_assignment();
    assert_eq!(owner_sets(&survivor), before);
    assert!(survivor.original_path.exists());
    assert!(survivor.cache_path.join("playback.f32le").exists());
    survivor.release_preparation_assignment();
}

#[test]
fn retired_assignment_cannot_borrow_its_active_siblings_subscription_rights() {
    let fixture = Fixture::new(44_100, 1);
    let (_producer_pcm, producer) = fixture.cold(48_000, 2);
    let (_survivor_pcm, survivor) = fixture.warm(&fixture.source, true, 48_000, 2);
    producer.release_preparation_assignment();
    let before = owner_sets(&survivor);
    assert!(producer.subscribe_verified().is_err());
    assert_eq!(owner_sets(&survivor), before);
    assert!(!survivor.assignment_retired());
    survivor.release_preparation_assignment();
}

#[test]
fn changed_complete_binding_still_rejects_subscription_without_owner_mutation() {
    let fixture = Fixture::new(44_100, 1);
    let (_pcm, lease) = fixture.cold(48_000, 2);
    let before = owner_sets(&lease);
    let mut wrong_manifest = lease.clone();
    wrong_manifest.manifest.descriptor["decoder"]["original"]["sha256"] = json!("f".repeat(64));
    assert!(wrong_manifest.subscribe_verified().is_err());
    let mut wrong_path = lease.clone();
    wrong_path.original_path = fixture.source.clone();
    assert!(wrong_path.subscribe_verified().is_err());
    let mut wrong_material = lease.clone();
    wrong_material.material_id = Some("f".repeat(32));
    assert!(wrong_material.subscribe_verified().is_err());
    assert_eq!(owner_sets(&lease), before);
    assert!(!lease.assignment_retired());
    lease.release_preparation_assignment();
}

#[test]
fn exact_queued_verified_cache_retirement_still_rejects_subscription() {
    let fixture = Fixture::new(44_100, 1);
    let (_pcm, lease) = fixture.cold(48_000, 2);
    let before = owner_sets(&lease);
    let mut proofs = Vec::new();
    for name in ["decoder.f32le", "playback.f32le", "manifest.json"] {
        let mut reader = sealed_reader(&lease.cache_path.join(name)).unwrap();
        let identity = file_identity(&reader).unwrap();
        let (sha256, bytes) = source_hash(&mut reader, &|| false).unwrap();
        proofs.push(crate::audio_engine::project_assets::VerifiedFile {
            name: name.to_owned(),
            identity,
            bytes,
            sha256,
        });
    }
    // This is genuine checked production retirement admission, with real FileIDs
    // and hashes. Existing sealed owners keep deletion pending throughout the test.
    CommittedColdLease::queue_verified_retirement(
        &lease.cache_path,
        &lease.cache.identity,
        &proofs,
        None,
    )
    .unwrap();
    let error = lease.subscribe_verified().err().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(owner_sets(&lease), before);
    assert!(!lease.assignment_retired());
    assert!(lease.cache_path.join("playback.f32le").exists());
    lease.release_preparation_assignment();
}

#[test]
fn actual_subscriber_capacity_rejects_atomically_without_revoking_live_owners() {
    let fixture = Fixture::new(44_100, 1);
    let (_pcm, lease) = fixture.cold(48_000, 2);
    let mut subscribers = Vec::new();
    // Exercise actual independent subscriptions rather than populating either
    // ownership set with synthetic IDs or changing the bounded capacity.
    for _ in 1..4096 {
        subscribers.push(lease.subscribe_verified().unwrap());
    }
    let before = owner_sets(&lease);
    assert_eq!(before.0.len(), 4096);
    assert_eq!(before.0, before.1);
    let error = lease.subscribe_verified().err().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(owner_sets(&lease), before);
    assert!(!lease.assignment_retired());
    assert!(subscribers.iter().all(|child| !child.assignment_retired()));
    for child in &subscribers {
        child.release_preparation_assignment();
    }
    assert_eq!(lease.cache_assignment_count(), 1);
    lease.release_preparation_assignment();
}
