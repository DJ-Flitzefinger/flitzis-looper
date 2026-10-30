//! Exact per-pass witnesses, chronological slot reuse and strict negative cases.

use super::*;
use crate::audio_engine::ordinary_endpoint_reserve_tests::{
    adopt_current_sources, refresh_actual_runtime_bindings,
};
use crate::audio_engine::ordinary_start_tests::{
    preparation_deadline_tests::WorkerGate, wait_ready,
};
use crate::audio_engine::pitch_runtime_tests::Fixture;

fn ready_pair() -> (
    Fixture,
    Vec<crate::audio_engine::pad_start::CapturedPadStart>,
) {
    let mut fixture = Fixture::new(true);
    adopt_current_sources(&mut fixture, true);
    refresh_actual_runtime_bindings(&fixture);
    fixture.mixer.set_bpm_lock(true);
    fixture.mixer.set_master_bpm(120.0);
    fixture.mixer.set_pad_bpm(0, Some(150.0));
    let mut starts = Vec::new();
    let mut gate = None;
    for frame in [0, 16] {
        fixture.render_to(frame);
        fixture.refresh_clock();
        let start = fixture
            .control
            .engine
            .capture_pad_start(0, 0.1, Some(2.1), 1.0, false, None, None)
            .unwrap();
        if frame == 0 {
            gate = Some(WorkerGate::for_ordinary(&start));
        }
        fixture.drain();
        assert!(
            fixture
                .control
                .engine
                .publish_pad_start(&start, None)
                .unwrap()
        );
        fixture.drain();
        if frame == 0 {
            gate.as_ref().unwrap().wait_for(false);
        }
        starts.push(start);
    }
    fixture.render_to(64);
    let gate = gate.unwrap();
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    wait_ready(
        &mut fixture,
        &starts
            .iter()
            .map(|start| start.sequence())
            .collect::<Vec<_>>(),
    );
    (fixture, starts)
}

#[test]
fn live_original_chain_uses_original_order_after_physical_slot_reuse() {
    let (mut fixture, starts) = ready_pair();
    fixture
        .mixer
        .pitch_runtime
        .as_mut()
        .unwrap()
        .slots
        .swap(0, 1);
    fixture.scheduler = FixedCapacityScheduler::new();
    for (slot, start) in [(1, &starts[0]), (0, &starts[1])] {
        fixture
            .scheduler
            .schedule(
                start.execution_frame().unwrap(),
                ScheduledCommand::PitchAction {
                    slot,
                    action_sequence: start.sequence(),
                },
            )
            .unwrap();
    }
    fixture.render(512);
    assert_eq!(starts[0].effective_frame(), Some(64));
    assert_eq!(starts[1].effective_frame(), Some(80));
    assert_eq!(fixture.starts(0), 2);
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(starts[1].sequence())
    );
}

#[test]
fn live_original_chain_requires_fresh_receipts_exact_metadata_and_checked_spacing() {
    let (mut fixture, starts) = ready_pair();
    let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
    let mut receipts = [None; MAX_PITCH_ACTIONS];
    assert_eq!(runtime.completion_execution(1, 64, &receipts), None);
    receipts[0] = Some(CompletionReceipt {
        header: 0,
        sequence: starts[0].sequence(),
        execution: 64,
    });
    assert_eq!(
        runtime.completion_execution(1, 64, &receipts),
        None,
        "old planned E cannot acquire a receipt"
    );
    runtime.slots[0].planned = 64;
    assert_eq!(runtime.completion_execution(1, 64, &receipts), Some(80));
    receipts[0].as_mut().unwrap().sequence += 1;
    assert_eq!(runtime.completion_execution(1, 64, &receipts), None);
    receipts[0].as_mut().unwrap().sequence -= 1;
    runtime.slots[0].planned = u64::MAX - 8;
    receipts[0].as_mut().unwrap().execution = u64::MAX - 8;
    assert_eq!(
        runtime.completion_execution(1, 64, &receipts),
        None,
        "overflow is no proof"
    );
    runtime.slots[0].planned = 64;
    receipts[0].as_mut().unwrap().execution = 64;
    runtime.slots[0].owner = None;
    assert_eq!(
        runtime.completion_execution(1, 64, &receipts),
        None,
        "missing owner stays strict"
    );
    // Even ready Work cannot manufacture a receipt for that unknown predecessor.
    fixture.render(512);
    assert!(starts.iter().all(|start| start.effective_frame().is_none()));
}

#[test]
fn live_original_chain_keeps_a_ready_dry_piano_behind_an_actual_busy_wet_original() {
    let mut fixture = Fixture::new(true);
    adopt_current_sources(&mut fixture, true);
    refresh_actual_runtime_bindings(&fixture);
    fixture
        .mixer
        .set_pad_pitch_intent(
            0,
            crate::audio_engine::pitch_intent::PitchIntent::new(0, 3).unwrap(),
        )
        .unwrap();
    let first = fixture
        .control
        .engine
        .capture_pad_start(0, 0.1, Some(2.1), 1.0, false, None, None)
        .unwrap();
    let gate = WorkerGate::for_ordinary(&first);
    fixture.drain();
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&first, None)
            .unwrap()
    );
    fixture.drain();
    gate.wait_for(false);
    let next = fixture.capture(0, "pitch.extra:0:set_retrigger");
    assert!(fixture.control.engine.publish_pitch_action(&next).unwrap());
    // Admit through the genuine command seam before the following due pass, so
    // the test can observe the independently ready dry owner behind live work.
    while let Ok(message) = fixture.control.consumer.pop() {
        crate::audio_engine::audio_stream::process_control_message(
            message,
            &mut fixture.scheduler,
            0,
            &mut crate::messages::TriggerQuantization::Immediate,
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut fixture.feedback,
            &mut fixture.retirement,
        );
    }
    let runtime = fixture.mixer.pitch_runtime.as_ref().unwrap();
    assert!(!runtime.has_ready_action(first.sequence()));
    assert!(runtime.has_ready_action(next.sequence()));
    assert_eq!(next.execution_target(), 0);
    assert!(fixture.render(512).iter().all(|value| *value == 0.0));
    assert_eq!(first.effective_frame(), None);
    assert!(!next.effective());
    assert_ne!(first.state(), "rejected");
    assert!(fixture.scheduler.deferred_due().is_none());
    // Only the pre-existing live-order late path may replan this already-due
    // dry event. The optimization cannot put a busy original in deferred_due.
    assert_eq!(next.execution_target(), 8192);
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    wait_ready(&mut fixture, &[first.sequence(), next.sequence()]);
    fixture.render(0);
    assert_eq!(first.effective_frame(), Some(512));
    assert!(next.effective());
    assert_eq!(next.execution_target(), 512);
    assert!(first.sequence() < next.sequence());
    assert_eq!(fixture.starts(0), 2);
    let sample = fixture.voice(0).sample.as_ref().unwrap();
    assert_eq!(
        sample.resident_start(),
        crate::audio_engine::pitch_runtime_tests::LOOP_START
    );
    let source = sample.samples.clone();
    assert_eq!(fixture.pcm(24_000), source[..24_000]);
}
