use super::*;

#[test]
fn ordinary_transport_after_effect_newer_admission_preserves_original_completion() {
    let mut fixture = accepted_fixture();
    let mut mixer = fixture.mixer();
    assert!(mixer.play_sample(0, 1.0));
    let pause = fixture.engine.pause_sample(0).unwrap();
    let message = fixture.consumer.pop().unwrap();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let (producer, mut consumer) = RingBuffer::new(4);
    let mut tagged = ContentFeedbackProducer {
        producer,
        ownership: ownership.clone(),
    };
    let admission_owner = ownership.clone();
    with_after_effect(
        move || {
            let placement = admission_owner.slot_snapshot(0).unwrap();
            let next = admission_owner.prepare_transport_intent(placement).unwrap();
            admission_owner
                .commit_transport_intent(next, || true, || {})
                .unwrap();
        },
        || process(&mut mixer, message, &mut tagged),
    );
    let message = consumer.pop().unwrap();
    let observed = fixture
        .engine
        ._resolve_ordinary_transport_feedback(&message)
        .unwrap()
        .unwrap();
    assert_eq!(observed.transport_revision(), pause.transport_revision());
    assert!(observed.matches_admission(&pause));
    assert!(observed.ordinary_paused());
    assert!(
        matches!(message, AudioMessage::OrdinaryTransportFeedback { transport_revision, .. } if transport_revision == pause.transport_revision())
    );
}

#[test]
fn ordinary_transport_observation_follows_actual_pair_and_waits_for_control_mirror() {
    let mut fixture = accepted_fixture();
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    assert!(mixer.play_sample(0, 1.0));
    let pause = fixture.engine.pause_sample(0).unwrap();
    let message = fixture.consumer.pop().unwrap();
    let mut feedback = Vec::<AudioMessage>::new();
    process(&mut mixer, message, &mut feedback);
    let receipt = feedback
        .iter()
        .find(|message| matches!(message, AudioMessage::OrdinaryTransportFeedback { .. }))
        .unwrap()
        .clone();
    let sequence = pause
        .current_observation()
        .unwrap()
        .unwrap()
        .observation_sequence();
    let ticket = own_pair_ack(&mut fixture, &mut mixer);
    assert!(
        fixture
            .engine
            ._current_ordinary_transport_observation(0)
            .is_err()
    );
    assert!(
        fixture
            .engine
            ._current_ordinary_transport_observation(1)
            .is_err()
    );
    assert!(pause.current_control_slot().is_err());
    assert!(pause.current_observation().is_err());
    assert!(
        fixture
            .engine
            ._resolve_ordinary_transport_feedback(&receipt)
            .is_err()
    );
    assert!(pad_pair::reconcile(&fixture.engine, &ticket).unwrap());
    let acknowledgement = ticket.own_acknowledgement().unwrap();
    fixture
        .engine
        ._finish_pad_pair(&ticket, &acknowledgement)
        .unwrap();
    assert_eq!(pause.current_control_slot().unwrap(), Some(1));
    let bootstrap = fixture
        .engine
        ._current_ordinary_transport_observation(1)
        .unwrap()
        .unwrap();
    assert_eq!(bootstrap.current_control_slot().unwrap(), Some(1));
    assert_eq!(bootstrap.observation_sequence(), sequence);
    assert!(bootstrap.ordinary_paused());
    assert!(
        fixture
            .engine
            ._current_ordinary_transport_observation(0)
            .unwrap()
            .is_none_or(|value| !value.ordinary_paused())
    );
    let observation = pause.current_observation().unwrap().unwrap();
    assert_eq!(observation.observation_sequence(), sequence);
    assert!(observation.ordinary_paused());
    assert!(observation.matches_admission(&pause));
    assert!(
        fixture
            .engine
            ._resolve_ordinary_transport_feedback(&receipt)
            .unwrap()
            .is_some()
    );
}
