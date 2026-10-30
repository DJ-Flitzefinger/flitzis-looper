//! Actual admitted groups exercise terminal proof and a lost peer-rejection CAS.

use super::*;
use crate::audio_engine::audio_stream::drain_control_messages;
use crate::audio_engine::ordinary_endpoint_reserve_tests::{
    adopt_current_sources, refresh_actual_runtime_bindings,
};
use crate::audio_engine::pad_start::CapturedPadStart;
use crate::audio_engine::pitch_runtime_tests::Fixture;
use crate::messages::TriggerQuantization;
use pyo3::prelude::*;

fn drain_grid(fixture: &mut Fixture) {
    drain_control_messages(
        &mut fixture.control.consumer,
        &mut fixture.scheduler,
        fixture.transport.output_frame(),
        &mut TriggerQuantization::Grid { step_64ths: 16 },
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
}

fn admitted_group() -> (Fixture, Vec<CapturedPadStart>) {
    let mut fixture = Fixture::new(false);
    adopt_current_sources(&mut fixture, false);
    refresh_actual_runtime_bindings(&fixture);
    assert!(
        fixture
            .render_to(17_000)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    fixture.refresh_clock();
    let captured = Python::attach(|py| {
        let entries = (0..2)
            .map(|id| {
                let binding = fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                (Py::new(py, binding).unwrap(), 0.1, Some(2.1), None)
            })
            .collect();
        fixture
            .control
            .engine
            .capture_global_start(py, entries, None)
            .unwrap()
    });
    drain_grid(&mut fixture);
    let starts = captured
        .resolve()
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|member| member.start)
        .collect::<Vec<_>>();
    assert!(
        fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .unwrap()
            .is_some()
    );
    drain_grid(&mut fixture);
    assert!(
        starts.iter().all(
            |start| start.original_frame() == Some(24_000) && start.effective_frame().is_none()
        )
    );
    assert!(
        starts.iter().all(|start| start.state() == "scheduled"),
        "{:?}",
        starts
            .iter()
            .map(|start| (start.state(), start.reason()))
            .collect::<Vec<_>>()
    );
    (fixture, starts)
}

#[test]
fn group_peer_rejection_cas_loss_stays_a_barrier_until_actual_terminal_status() {
    let (fixture, starts) = admitted_group();
    assert!(starts[0].cancel());
    let peer = starts[1].clone();
    let runtime = fixture.mixer.pitch_runtime.as_ref().unwrap();
    CapturedPadStart::with_reject_interleaving_for_test(
        move || assert!(peer.mark_prepared()),
        || runtime.reject_impossible_group_peers(),
    );
    assert_eq!(
        starts[1].state(),
        "prepared",
        "lost CAS is not terminal proof"
    );
    assert_eq!(runtime.unused_action_capacity().0, 62);
    assert_eq!(fixture.scheduler.len(), 1);
    runtime.reject_impossible_group_peers();
    assert_eq!(starts[1].state(), "rejected");
    assert!(starts.iter().all(|start| start.effective_frame().is_none()));
    assert_eq!(runtime.unused_action_capacity().0, 62);
}

#[test]
fn group_peer_proof_requires_rejected_owner_and_complete_metadata_not_epoch_suspicion() {
    let (mut fixture, starts) = admitted_group();
    let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
    let header = runtime
        .slots
        .iter()
        .position(|slot| slot.group.is_some())
        .unwrap();
    let indices = runtime.slots[header].group.as_ref().unwrap().slots;
    let epoch = runtime.slots[indices[0]].epoch.as_ref().unwrap().clone();
    let saved_epoch = epoch.swap(0, Ordering::AcqRel);
    runtime.reject_impossible_group_peers();
    assert!(starts.iter().all(|start| start.state() == "scheduled"));
    epoch.store(saved_epoch, Ordering::Release);
    assert!(starts[0].cancel());
    let owner = runtime.slots[indices[0]].owner.take();
    runtime.reject_impossible_group_peers();
    assert_eq!(
        starts[1].state(),
        "scheduled",
        "unobserved owner cannot prove impossibility"
    );
    runtime.slots[indices[0]].owner = owner;
    runtime.reject_impossible_group_peers();
    assert_eq!(starts[1].state(), "rejected");
    assert_eq!(runtime.unused_action_capacity().0, 62);
    assert_eq!(fixture.scheduler.len(), 1);
}

#[test]
fn live_group_peer_still_fences_a_later_original_without_terminal_proof() {
    let (mut fixture, starts) = admitted_group();
    let next = fixture
        .control
        .engine
        .capture_pad_start(1, 0.1, Some(2.1), 1.0, false, None, None)
        .unwrap();
    drain_grid(&mut fixture);
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&next, None)
            .unwrap()
    );
    drain_grid(&mut fixture);
    let runtime = fixture.mixer.pitch_runtime.as_ref().unwrap();
    let index = runtime
        .slots
        .iter()
        .position(|slot| slot.sequence == next.sequence())
        .unwrap();
    assert!(runtime.preceding_endpoint(index).is_some());
    assert!(
        starts
            .iter()
            .all(|start| start.state() == "scheduled" && start.effective_frame().is_none())
    );
    assert_eq!(runtime.unused_action_capacity().0, 61);
    assert_eq!(fixture.scheduler.len(), 2);
}
