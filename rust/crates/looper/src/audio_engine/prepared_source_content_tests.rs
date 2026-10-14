use super::*;
use crate::audio_engine::complete_context::source_custody_tests::own_pair_ack;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::pad_pair;
use crate::messages::PreparedStemSet;

pub(in crate::audio_engine) fn ticket(fixture: &LaunchFixture) -> PreparedSourceTicket {
    fixture
        .engine
        .capture_prepared_source(0, format!("samples/a.wav|sha256-v1:{:064x}", 7))
        .unwrap()
}

pub(in crate::audio_engine) fn prepared_set(ticket: &PreparedSourceTicket) -> PreparedStemSet {
    PreparedStemSet {
        complete_set_identity: Arc::new([17; 32]),
        reference_samples: ticket.sample.samples.clone(),
        reference_residency: ticket.sample.residency.clone(),
        publication: ticket.publication.clone(),
        accepted_timing: ticket.publication.accepted_projection(),
        source_version_hash: crate::audio_engine::stem_cache::source_version_hash(
            &ticket.source_version,
        ),
        sample_rate_hz: ticket.sample_rate_hz,
        channels: ticket.sample.channels,
        frame_count: ticket.sample.frame_count(),
        available_mask: 15,
        stems: std::array::from_fn(|_| ticket.sample.clone()),
    }
}

#[test]
fn source_request_comparison_follows_content_and_rejects_actual_offline_supersession() {
    let mut fixture = LaunchFixture::new(4);
    let original = ticket(&fixture);
    let mut mixer = fixture.mixer();
    let pair = own_pair_ack(&mut fixture, &mut mixer);
    assert!(pad_pair::reconcile(&fixture.engine, &pair).unwrap());
    assert!(
        fixture
            .engine
            .capture_prepared_source(1, original.source_version.clone())
            .is_err()
    );
    let acknowledgement = pair.own_acknowledgement().unwrap();
    fixture
        .engine
        ._finish_pad_pair(&pair, &acknowledgement)
        .unwrap();
    let moved = fixture
        .engine
        .capture_prepared_source(1, original.source_version.clone())
        .unwrap();
    assert_ne!(original.id, moved.id);
    assert!(original.same_source_request(&moved));
    let other = fixture
        .engine
        .capture_prepared_source(0, format!("samples/b.wav|sha256-v1:{:064x}", 8))
        .unwrap();
    assert!(!original.same_source_request(&other));
    let reader = crate::audio_engine::complete_context::CompleteSourceReader::capture(
        &fixture.engine,
        1,
        moved.sample.clone(),
    )
    .unwrap();
    let job = fixture
        .engine
        .offline_jobs
        .begin_complete(
            1,
            reader,
            48_000,
            7,
            crate::audio_engine::analysis_jobs::OfflineRequestOwner {
                request_ids: fixture.engine.pad_request_ids.clone(),
                prepared_epoch: fixture.engine.prepared_source_epoch(1).unwrap().clone(),
            },
            fixture.engine.loader_tx.clone(),
        )
        .unwrap();
    let superseding = fixture
        .engine
        .capture_prepared_source(1, original.source_version.clone())
        .unwrap();
    assert_eq!(
        moved.admission.source_generation,
        superseding.admission.source_generation
    );
    assert!(moved.sample.same_source(&superseding.sample));
    assert!(!moved.same_source_request(&superseding));
    job.cancel();
    let cancelled = fixture
        .engine
        .capture_prepared_source(1, original.source_version.clone())
        .unwrap();
    assert!(!superseding.same_source_request(&cancelled));
}

#[test]
fn queued_prepared_stems_follow_original_key_after_actual_pair_ack_with_equal_shared_pcm() {
    let mut fixture = LaunchFixture::new(4);
    let sample = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    fixture
        .engine
        .input_runtime_ownership
        .publish_test_source(1, &sample, 48_000, 7);
    fixture.engine.sample_cache.lock().unwrap()[1] = Some(sample);
    fixture.engine.loaded_source_generations.lock().unwrap()[1] = (7, 48_000);
    fixture.engine.loaded_source_digests.lock().unwrap()[1] = Some(format!("{:064x}", 7));
    let ticket = ticket(&fixture);
    let stems = prepared_set(&ticket);
    let identity = stems.complete_set_identity.clone();
    enqueue_current_prepared_stems(
        &fixture.engine,
        &fixture.producer,
        &ticket,
        &ticket.source_version,
        stems,
    )
    .unwrap();
    let ControlMessage::PublishPreparedStems { id, stems } = fixture.consumer.pop().unwrap() else {
        panic!("prepared stems");
    };
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let pair = own_pair_ack(&mut fixture, &mut mixer);
    assert!(pad_pair::reconcile(&fixture.engine, &pair).unwrap());
    assert!(mixer.publish_prepared_stems(id, stems));
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(mixer.stems_for_measurement()[0].is_none());
    assert!(Arc::ptr_eq(
        &mixer.stems_for_measurement()[1]
            .as_ref()
            .unwrap()
            .complete_set_identity,
        &identity
    ));
}

#[test]
fn final_prepared_arm_after_pair_uses_original_epoch_and_does_not_retoken_physical_requests() {
    let mut fixture = LaunchFixture::new(4);
    let ticket = ticket(&fixture);
    let stems = prepared_set(&ticket);
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let pair = own_pair_ack(&mut fixture, &mut mixer);
    assert!(
        !try_enqueue_prepared_stems(
            &fixture.engine,
            &fixture.producer,
            &ticket,
            &ticket.source_version,
            stems.clone(),
            None
        )
        .unwrap()
    );
    assert_eq!(ticket.publication_status(), "captured");
    assert!(pad_pair::reconcile(&fixture.engine, &pair).unwrap());
    assert!(
        try_enqueue_prepared_stems(
            &fixture.engine,
            &fixture.producer,
            &ticket,
            &ticket.source_version,
            stems,
            None
        )
        .unwrap()
    );
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    let ControlMessage::PublishPreparedStems { id, stems } = fixture.consumer.pop().unwrap() else {
        panic!("prepared stems");
    };
    assert_eq!(id, 1);
    assert!(mixer.publish_prepared_stems(id, stems));
}

#[test]
fn queued_prepared_stems_reject_equal_source_rebirth_under_original_slot_and_counters() {
    let mut fixture = LaunchFixture::new(4);
    let ticket = ticket(&fixture);
    let sample = ticket.sample.clone();
    let stems = prepared_set(&ticket);
    enqueue_current_prepared_stems(
        &fixture.engine,
        &fixture.producer,
        &ticket,
        &ticket.source_version,
        stems,
    )
    .unwrap();
    let ControlMessage::PublishPreparedStems { id, stems } = fixture.consumer.pop().unwrap() else {
        panic!("prepared stems");
    };
    fixture.engine.input_runtime_ownership.revoke_source(0);
    fixture
        .engine
        .input_runtime_ownership
        .publish_test_source(0, &sample, 48_000, 7);
    let mut mixer = fixture.mixer();
    assert!(!mixer.publish_prepared_stems(id, stems));
    assert_eq!(ticket.rejection_reason(), Some("source-request-changed"));
    assert!(mixer.stems_for_measurement()[0].is_none());
}
