//! Resident launch admission retains an accepted scalar permit and real captured source guards.
use super::*;
use crate::audio_engine::input_mapping::launch_admission_tests::{LaunchFixture, RATE};
use crate::audio_engine::resident_relocation::control_publication_tests::PendingWriter;
use std::cell::RefCell;

// This fixture isolates launch admission after accepted preserved-window publication.
// Actual resident preparation/ACK/rendering remains covered by resident_ready_control_tests.
fn accepted_ticket(fixture: &LaunchFixture) -> ResidentWindowTicket {
    let binding = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    let epoch = Arc::new(AtomicU64::new(1));
    let publication = PreparedSourcePermit::new(epoch, 1);
    publication.mark_accepted_preserved_window();
    ResidentWindowTicket {
        input_trigger: None,
        publication,
        state: Arc::new(WindowState::for_placement(
            binding.binding.placement.unwrap(),
            binding.source_generation,
        )),
        window_revision: binding.binding.resident.unwrap().revision,
        previous_window_revision: binding.binding.resident.unwrap().revision,
        start_frame: 0,
        end_frame: 128,
        full_frame_count: 128,
        id: 0,
        source_generation: binding.source_generation,
        binding: binding.binding,
        ownership: binding.ownership,
        acknowledgements: binding.acknowledgements,
        loop_region: Some((16.0 / f64::from(RATE), Some(96.0 / f64::from(RATE)))),
        key_lock_request_id: None,
    }
}

fn captured_midi(fixture: &mut LaunchFixture) -> CapturedInputPadTrigger {
    let runtime = crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
        fixture.producer.clone(),
        fixture.engine.input_clock,
        fixture.engine.input_runtime_ownership.clone(),
    );
    runtime.set_enabled(true);
    runtime.replace_mappings(vec![("midi:note:1:60".into(), "pad.trigger:0".into())]);
    let bindings: Vec<_> = (0..NUM_SAMPLES)
        .map(|id| input_runtime_binding::capture(&fixture.engine, id).unwrap())
        .collect();
    runtime
        .set_runtime_state(
            true,
            bindings.iter().map(Option::is_some).collect(),
            vec![16.0 / f64::from(RATE); NUM_SAMPLES],
            vec![Some(96.0 / f64::from(RATE)); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
        )
        .unwrap();
    let trigger = runtime
        .capture_trigger_for_test("midi:note:1:60", 123)
        .unwrap();
    fixture.engine.input_runtime = Some(runtime);
    trigger
}

#[test]
fn captured_midi_window_launch_requires_its_exact_owner_and_preserves_time_after_pair() {
    let mut fixture = LaunchFixture::new(4);
    let trigger = captured_midi(&mut fixture);
    let foreign_window = accepted_ticket(&fixture);
    let mut own_window = accepted_ticket(&fixture);
    own_window.input_trigger = Some(trigger.clone());
    // Equal accepted source/window values are insufficient without the original event owner.
    assert!(
        !launch_captured_with_producer(
            &fixture.engine,
            &trigger,
            &foreign_window,
            &fixture.producer
        )
        .unwrap()
    );
    // Captured MIDI tickets have one authoritative launch lane.
    assert!(
        !launch_with_producer(&fixture.engine, &own_window, true, 999, &fixture.producer).unwrap()
    );
    let mut mixer = fixture.mixer();
    let pair = crate::audio_engine::complete_context::source_custody_tests::own_pair_ack(
        &mut fixture,
        &mut mixer,
    );
    assert!(crate::audio_engine::pad_pair::reconcile(&fixture.engine, &pair).unwrap());
    assert!(
        launch_captured_with_producer(&fixture.engine, &trigger, &own_window, &fixture.producer)
            .unwrap()
    );
    assert!(matches!(
        fixture.consumer.pop().unwrap(),
        ControlMessage::TriggerInputPad {
            id: 1,
            received_at_ns: 123,
            exclusive: false,
            resident_control: Some(_),
            ..
        }
    ));
    assert_eq!(trigger.original_owner_status(), "admitted");
    assert!(
        !launch_captured_with_producer(&fixture.engine, &trigger, &own_window, &fixture.producer)
            .unwrap()
    );
    assert!(fixture.consumer.is_empty());
}

#[test]
fn resident_launch_pending_after_payload_preserves_flags_then_retries() {
    let mut fixture = LaunchFixture::new(8);
    let ticket = accepted_ticket(&fixture);
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let held = RefCell::new(None);
    assert!(
        !launch_with_producer_before_commit(
            &fixture.engine,
            &ticket,
            false,
            42,
            &fixture.producer,
            || *held.borrow_mut() = Some(PendingWriter::hold(ownership.clone()))
        )
        .unwrap()
    );
    held.into_inner().unwrap().complete();
    assert!(ownership.admitted_launch_ids().is_empty());
    assert_eq!(ownership.launch_revision(0), 0);
    assert!(fixture.consumer.is_empty());
    assert!(launch_with_producer(&fixture.engine, &ticket, false, 43, &fixture.producer).unwrap());
    assert_eq!(ownership.admitted_launch_ids(), vec![0]);
    assert!(matches!(
        fixture.consumer.pop().unwrap(),
        ControlMessage::TriggerInputPad {
            received_at_ns: 43,
            resident_control: Some(_),
            ..
        }
    ));
}

#[test]
fn resident_launch_full_queue_cancellation_and_rebinding_publish_nothing() {
    for case in 0..4 {
        let mut fixture = LaunchFixture::new(1);
        let ticket = accepted_ticket(&fixture);
        if case == 0 {
            fixture
                .producer
                .lock()
                .unwrap()
                .push(ControlMessage::Ping())
                .unwrap();
        }
        if case == 1 {
            assert!(ticket.cancel_launch());
        }
        if case >= 2 {
            let mut mixer = fixture.mixer();
            fixture.swap(&mut mixer);
            if case == 3 {
                fixture.swap(&mut mixer);
            }
        }
        let result = launch_with_producer(&fixture.engine, &ticket, false, 42, &fixture.producer);
        if case == 0 {
            assert!(result.is_err());
            assert!(matches!(
                fixture.consumer.pop().unwrap(),
                ControlMessage::Ping()
            ));
        } else {
            assert!(!result.unwrap());
        }
        assert!(fixture.consumer.is_empty());
        assert!(
            fixture
                .engine
                .input_runtime_ownership
                .admitted_launch_ids()
                .is_empty()
        );
    }
}

#[test]
fn resident_launch_rechecks_original_publication_before_flag_and_queue_commit() {
    let fixture = LaunchFixture::new(8);
    let ticket = accepted_ticket(&fixture);
    assert!(
        !launch_with_producer_before_commit(
            &fixture.engine,
            &ticket,
            false,
            42,
            &fixture.producer,
            || ticket.publication.epoch.store(2, Ordering::Release)
        )
        .unwrap()
    );
    assert!(fixture.consumer.is_empty());
    assert!(
        fixture
            .engine
            .input_runtime_ownership
            .admitted_launch_ids()
            .is_empty()
    );
}

#[test]
fn resident_launch_retains_source_only_semantics_when_midi_runtime_revision_changes() {
    let fixture = LaunchFixture::new(8);
    let ticket = accepted_ticket(&fixture);
    let ownership = &fixture.engine.input_runtime_ownership;
    ownership
        .runtime_for_cell(ticket.binding.placement.unwrap().cell())
        .store(17, Ordering::Release);
    assert!(launch_with_producer(&fixture.engine, &ticket, false, 42, &fixture.producer).unwrap());
    assert_eq!(ownership.admitted_launch_ids(), vec![0]);
}
