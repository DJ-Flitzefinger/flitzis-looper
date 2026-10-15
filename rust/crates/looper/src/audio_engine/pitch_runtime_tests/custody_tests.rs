//! Real publication/admission/retirement pressure, independent of requested shadows.

use super::*;
use crate::audio_engine::pitch_actions::{PitchActionPreparation, PitchActionWork};
use crate::audio_engine::pitch_publication::PitchWorkCapacity;

fn wait_released(fixture: &Fixture) {
    let deadline = Instant::now() + WORKER_WAIT;
    while fixture.control.engine.pitch_work_capacity.used() != 0 {
        assert!(Instant::now() < deadline, "terminal work did not retire");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn invalid_formats_and_failed_worker_release_only_the_original_storage_claim() {
    let fixture = Fixture::new(false);
    let capacity = PitchWorkCapacity::default();
    let action = fixture.capture(0, "pitch.extra:7:set_retrigger");
    for (rate, channels) in [(0, 1), (RATE, 0), (RATE, usize::MAX)] {
        assert!(
            PitchActionWork::new(action.clone(), rate, channels, capacity.claim().unwrap())
                .is_err()
        );
        assert_eq!(capacity.used(), 0);
        assert_eq!(capacity.allocations(), (0, 0));
    }
    let work = PitchActionWork::new(action.clone(), RATE, 1, capacity.claim().unwrap()).unwrap();
    assert!(work.candidate.is_none());
    assert_eq!(capacity.allocations(), (1, 0));
    let (mut lane, worker) = PitchActionPreparation::new(1).unwrap();
    let mut retained = Some(work);
    assert_eq!(lane.publish(&mut retained), Ok(true));
    let deadline = Instant::now() + WORKER_WAIT;
    let returned = loop {
        if let Some(work) = lane.poll() {
            break work;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(action.reason(), Some("prepared_mismatch"));
    assert!(returned.candidate.is_none());
    assert_eq!(capacity.used(), 1);
    assert_eq!(capacity.allocations(), (1, 0));
    let mut retained = Some(returned);
    assert!(lane.retire(&mut retained));
    drop(worker); // Join proves off-callback destruction has finished.
    assert_eq!(capacity.used(), 0);
    assert_eq!(capacity.allocations(), (1, 0));
}

#[test]
fn full_control_queue_refuses_before_work_or_native_allocation() {
    let fixture = Fixture::new(false);
    let action = fixture.capture(0, "pitch.extra:7:set_retrigger");
    let target = action.original_due_frame();
    {
        let mut producer = fixture.control.producer.lock().unwrap();
        while producer.slots() != 0 {
            producer.push(ControlMessage::StopAll()).unwrap();
        }
    }
    assert!(
        fixture
            .control
            .engine
            .publish_pitch_action(&action)
            .is_err()
    );
    assert_eq!(action.reason(), Some("capacity"));
    assert_eq!(action.original_due_frame(), target);
    assert!(!action.accepted() && !action.effective());
    assert_eq!(fixture.control.engine.pitch_work_capacity.used(), 0);
    assert_eq!(
        fixture.control.engine.pitch_work_capacity.allocations(),
        (0, 0)
    );
}

#[test]
fn sixty_four_cancelled_publications_hold_custody_until_retired_then_fresh_pcm_recovers() {
    let mut fixture = Fixture::new(false);
    // Preserve the actual shared producer Arc; enlarge only test command storage so
    // the production 64-owner limit, rather than command capacity, is saturated.
    let (producer, consumer) = RingBuffer::new(128);
    *fixture.control.producer.lock().unwrap() = producer;
    fixture.control.consumer = consumer;
    for cycle in 0..3 {
        wait_released(&fixture);
        let before = fixture.control.engine.pitch_work_capacity.allocations();
        let mut receipts = Vec::new();
        for index in 0..pitch_runtime::MAX_PITCH_ACTIONS {
            let action = fixture.capture(
                0,
                if index % 2 == 0 {
                    "pitch.extra:7:set_retrigger"
                } else {
                    "pitch.extra:-7:set_retrigger"
                },
            );
            assert!(
                fixture
                    .control
                    .engine
                    .publish_pitch_action(&action)
                    .unwrap()
            );
            assert!(fixture.control.engine.cancel_pitch_action(&action).unwrap());
            receipts.push(action);
        }
        assert_eq!(fixture.control.engine.pitch_work_capacity.used(), 64);
        assert_eq!(
            fixture.control.engine.pitch_work_capacity.allocations(),
            (before.0 + 64, before.1)
        );
        let overflow = fixture.capture(0, "pitch.extra:2:set_retrigger");
        assert!(
            fixture
                .control
                .engine
                .publish_pitch_action(&overflow)
                .is_err()
        );
        assert_eq!(overflow.reason(), Some("capacity"));
        assert_eq!(fixture.control.engine.pitch_work_capacity.used(), 64);
        assert_eq!(
            fixture.control.engine.pitch_work_capacity.allocations(),
            (before.0 + 64, before.1)
        );
        fixture.drain();
        assert!(fixture.scheduler.is_empty());
        wait_released(&fixture);
        // Keeping every receipt alive must not keep its heavy work capacity alive.
        assert!(receipts.iter().all(|action| !action.effective()));
        let fresh = if cycle % 2 == 0 {
            fixture.capture(0, "pitch.extra:7:set_retrigger")
        } else {
            fixture.midi_capture("pitch.extra:7:set_retrigger")
        };
        let original = fresh.original_due_frame();
        let timestamp = fresh.received_timestamp();
        let content = fresh.content_key();
        let starts = fixture.starts(0);
        fixture.publish(&fresh);
        fixture.apply(&fresh);
        assert_eq!(fresh.original_due_frame(), original);
        assert_eq!(fresh.execution_target(), original + 8192);
        assert_eq!(fresh.received_timestamp(), timestamp);
        assert_eq!(fresh.content_key(), content);
        assert_eq!(fixture.starts(0), starts + 1);
        assert_native_source(&fixture, 0, &fresh);
        let pcm = fixture.pcm(48_000);
        assert_tone(&pcm[12_000..], 0, 7, 1.0, false);
        wait_released(&fixture);
        assert_eq!(
            fixture.control.engine.pitch_work_capacity.allocations(),
            (before.0 + 65, before.1 + 1)
        );
        assert!(fresh.effective());
    }
}

#[test]
fn ready_cancel_retains_claim_until_scheduled_retirement_and_scalar_set_never_constructs_native() {
    let mut fixture = Fixture::new(false);
    let stopped = fixture.capture(0, "pitch.extra:7:set");
    fixture.publish(&stopped);
    fixture.apply(&stopped);
    wait_released(&fixture);
    assert_eq!(
        fixture.control.engine.pitch_work_capacity.allocations(),
        (1, 0)
    );
    let action = fixture.capture(0, "pitch.extra:7:set_retrigger");
    fixture.publish(&action);
    fixture.wait_worker(&[&action]);
    assert_eq!(
        fixture.control.engine.pitch_work_capacity.allocations(),
        (2, 1)
    );
    assert!(fixture.control.engine.cancel_pitch_action(&action).unwrap());
    assert_eq!(fixture.control.engine.pitch_work_capacity.used(), 1);
    assert!(!action.effective());
    fixture.render_to(action.execution_target());
    assert_eq!(fixture.control.engine.pitch_work_capacity.used(), 1);
    fixture.render(1);
    wait_released(&fixture);
    assert_eq!(fixture.starts(0), 0);
    let fresh = fixture.midi_capture("pitch.extra:-7:set_retrigger");
    fixture.publish(&fresh);
    fixture.apply(&fresh);
    let pcm = fixture.pcm(48_000);
    assert_tone(&pcm[12_000..], 0, -7, 1.0, false);
    wait_released(&fixture);
    assert_eq!(
        fixture.control.engine.pitch_work_capacity.allocations(),
        (3, 2)
    );
}
