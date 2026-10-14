use super::*;
use crate::audio_engine::complete_context::source_custody_tests::own_pair_ack;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::pad_pair;
use crate::audio_engine::prepared_source::content_tests::{prepared_set, ticket};

fn publication(
    fixture: &LaunchFixture,
    ticket: &PreparedSourceTicket,
    root: &std::path::Path,
) -> PreparedStemsPublication {
    let path = root.join("stems").join("#1");
    std::fs::create_dir_all(&path).unwrap();
    let lease = fixture.engine.project_assets.acquire(root, &path).unwrap();
    PreparedStemsPublication {
        source_version: ticket.source_version.clone(),
        cache_dir: path.to_string_lossy().into_owned(),
        publication: PreparedPublication::Legacy(Box::new(LegacyPublication {
            stems: prepared_set(ticket),
            path,
            _lease: lease,
        })),
    }
}

#[test]
fn exact_prepared_result_and_lease_survive_control_gap_and_queue_pressure_until_original_callback()
{
    let mut fixture = LaunchFixture::new(1);
    let ticket = ticket(&fixture);
    let root = tempfile::tempdir().unwrap();
    let prepared = publication(&fixture, &ticket, root.path());
    let identity = match &prepared.publication {
        PreparedPublication::Legacy(p) => p.stems.complete_set_identity.clone(),
        _ => unreachable!(),
    };
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let pair = own_pair_ack(&mut fixture, &mut mixer);
    prepared
        .finish(&fixture.engine, &ticket, &fixture.producer)
        .unwrap();
    assert!(has_pending(&fixture.engine, &ticket).unwrap());
    let key = ticket.publication.content_key().unwrap();
    let original = {
        let pending = fixture.engine.pending_prepared_stems.lock().unwrap();
        let PreparedPublication::Legacy(p) = &pending[key.cell().index()]
            .as_ref()
            .unwrap()
            .prepared
            .publication
        else {
            unreachable!()
        };
        std::ptr::from_ref(p.as_ref()) as usize
    };
    reconcile(&fixture.engine).unwrap();
    assert!(has_pending(&fixture.engine, &ticket).unwrap());
    assert!(pad_pair::reconcile(&fixture.engine, &pair).unwrap());
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    reconcile(&fixture.engine).unwrap();
    assert!(has_pending(&fixture.engine, &ticket).unwrap());
    let held_identity = {
        let pending = fixture.engine.pending_prepared_stems.lock().unwrap();
        let prepared = &pending[key.cell().index()].as_ref().unwrap().prepared;
        let PreparedPublication::Legacy(p) = &prepared.publication else {
            unreachable!()
        };
        assert_eq!(std::ptr::from_ref(p.as_ref()) as usize, original);
        p.stems.complete_set_identity.clone()
    };
    assert!(Arc::ptr_eq(&identity, &held_identity));
    fixture.consumer.pop().unwrap();
    reconcile(&fixture.engine).unwrap();
    assert!(!has_pending(&fixture.engine, &ticket).unwrap());
    let ControlMessage::PublishPreparedStems { id, stems } = fixture.consumer.pop().unwrap() else {
        panic!("prepared publication");
    };
    assert_eq!(id, 1);
    assert!(Arc::ptr_eq(&identity, &stems.complete_set_identity));
    assert!(mixer.publish_prepared_stems(id, stems));
    assert_eq!(ticket.publication_status(), "accepted");
}

#[test]
fn retained_prepared_result_rejects_own_source_epoch_change_without_publishing_to_an_equal_occupant()
 {
    let mut fixture = LaunchFixture::new(1);
    let ticket = ticket(&fixture);
    let root = tempfile::tempdir().unwrap();
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::Ping())
        .unwrap();
    publication(&fixture, &ticket, root.path())
        .finish(&fixture.engine, &ticket, &fixture.producer)
        .unwrap();
    assert!(has_pending(&fixture.engine, &ticket).unwrap());
    ticket
        .publication
        .epoch
        .fetch_add(1, std::sync::atomic::Ordering::Release);
    fixture.consumer.pop().unwrap();
    reconcile(&fixture.engine).unwrap();
    assert!(!has_pending(&fixture.engine, &ticket).unwrap());
    assert_eq!(ticket.publication_status(), "rejected");
    assert!(fixture.consumer.pop().is_err());
}
