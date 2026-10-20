//! Actual dispatcher selection must retain its original input admission.
//!
//! The runtime mutex holds the real SelectPad route after lookup. The lookup's
//! extra strong mapping owner proves capture without a second dispatch helper.

use super::*;
use crate::audio_engine::input_mapping::launch_admission_tests::{LaunchFixture, RATE};
use std::time::Instant;

const BINDING: &str = "midi:note:1:60";
const SELECT_A: &str = "ui.select_pad:0";
const SELECT_B: &str = "ui.select_pad:1";

#[derive(Clone, Copy, Debug)]
enum Revocation {
    Disabled,
    Learn,
    Remapped,
    DisabledAndRestored,
    LearnAndRestored,
    RemappedAndRestored,
}

fn fixture() -> (LaunchFixture, InputRuntime) {
    let control = LaunchFixture::new(8);
    let runtime = InputRuntime::new_with_ownership(
        control.producer.clone(),
        control.engine.input_clock,
        control.engine.input_runtime_ownership.clone(),
    );
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings: Vec<Option<InputRuntimePadBinding>> =
        (0..NUM_SAMPLES).map(|_| None).collect();
    for id in [0, 1] {
        loaded[id] = true;
        starts[id] = 16.0 / f64::from(RATE);
        ends[id] = Some(96.0 / f64::from(RATE));
        bindings[id] = control
            .engine
            .current_input_runtime_pad_binding(id)
            .unwrap();
        assert!(bindings[id].as_ref().unwrap().current());
    }
    runtime
        .set_runtime_state(
            true,
            loaded,
            starts,
            ends,
            bindings.iter().map(Option::as_ref).collect(),
        )
        .unwrap();
    runtime
        .set_selected_binding(bindings[0].as_ref().unwrap())
        .unwrap();
    runtime.replace_mappings(vec![(BINDING.into(), SELECT_B.into())]);
    runtime.set_enabled(true);
    (control, runtime)
}

fn wait_for_actual_lookup(owner: &Arc<AtomicBool>, before: usize) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Arc::strong_count(owner) <= before {
        assert!(
            Instant::now() < deadline,
            "actual dispatcher did not retain its looked-up SelectPad mapping"
        );
        thread::yield_now();
    }
    assert!(Arc::strong_count(owner) > before);
}

fn event(runtime: &InputRuntime) -> InputRuntimeEvent {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(event) = runtime.poll_event() {
            return event;
        }
        assert!(
            Instant::now() < deadline,
            "actual SelectPad receipt timed out"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn revoke(runtime: &InputRuntime, change: Revocation) {
    match change {
        Revocation::Disabled => runtime.set_enabled(false),
        Revocation::Learn => runtime.set_learn_capture_active(true),
        Revocation::Remapped => {
            runtime.replace_mappings(vec![(BINDING.into(), SELECT_A.into())]);
        }
        Revocation::DisabledAndRestored => {
            runtime.set_enabled(false);
            runtime.set_enabled(true);
        }
        Revocation::LearnAndRestored => {
            runtime.set_learn_capture_active(true);
            runtime.set_learn_capture_active(false);
        }
        Revocation::RemappedAndRestored => {
            runtime.replace_mappings(vec![(BINDING.into(), SELECT_A.into())]);
            runtime.replace_mappings(vec![(BINDING.into(), SELECT_B.into())]);
        }
    }
}

fn revoked_lookup_cannot_select(change: Revocation) {
    let (mut control, runtime) = fixture();
    let original = runtime.selected_projection().unwrap();
    assert_eq!(original.1, Some(0));
    let endpoint = control.engine.input_runtime_ownership.endpoint_selection();
    let original_owner = runtime.mappings.lock().unwrap()[0].owner.clone();
    let owner_count = Arc::strong_count(&original_owner);
    let state = runtime.runtime_state.lock().unwrap();
    assert!(runtime.inject_midi_message(&[0x90, 60, 100]));
    wait_for_actual_lookup(&original_owner, owner_count);
    assert!(runtime.poll_event().is_none());
    assert_eq!(state.selection_revision, original.0);
    revoke(&runtime, change);
    drop(state);

    let receipt = event(&runtime);
    assert_eq!(receipt.binding_key, BINDING);
    assert_eq!(receipt.action_key.as_deref(), Some(SELECT_B));
    assert!(
        !receipt.dispatched,
        "SelectPad captured before {change:?} must refuse at actual selection admission"
    );
    assert!(receipt.selection_revision.is_none());
    assert_eq!(runtime.selected_projection().unwrap(), original);
    assert_eq!(
        control.engine.input_runtime_ownership.endpoint_selection(),
        endpoint
    );
    assert!(control.consumer.pop().is_err());

    runtime.set_enabled(true);
    runtime.set_learn_capture_active(false);
    assert!(runtime.poll_event().is_none());
    assert_eq!(runtime.selected_projection().unwrap(), original);

    // Only a new physical input may acquire the current mapping/selection owner.
    assert!(runtime.inject_midi_message(&[0x90, 60, 101]));
    let fresh = event(&runtime);
    let (selected, action) = if matches!(change, Revocation::Remapped) {
        (0, SELECT_A)
    } else {
        (1, SELECT_B)
    };
    assert_eq!(fresh.action_key.as_deref(), Some(action));
    assert!(fresh.dispatched);
    assert_eq!(fresh.selection_revision, Some(original.0 + 1));
    assert_eq!(
        runtime.selected_projection().unwrap(),
        (original.0 + 1, Some(selected))
    );
    assert!(control.consumer.pop().is_err());
}

#[test]
fn actual_select_dispatch_refuses_disable_after_lookup() {
    revoked_lookup_cannot_select(Revocation::Disabled);
}

#[test]
fn actual_select_dispatch_refuses_learn_after_lookup() {
    revoked_lookup_cannot_select(Revocation::Learn);
}

#[test]
fn actual_select_dispatch_refuses_remap_after_lookup() {
    revoked_lookup_cannot_select(Revocation::Remapped);
}

#[test]
fn actual_select_dispatch_refuses_disable_restore_aba_after_lookup() {
    revoked_lookup_cannot_select(Revocation::DisabledAndRestored);
}

#[test]
fn actual_select_dispatch_refuses_learn_restore_aba_after_lookup() {
    revoked_lookup_cannot_select(Revocation::LearnAndRestored);
}

#[test]
fn actual_select_dispatch_refuses_remap_restore_aba_after_lookup() {
    revoked_lookup_cannot_select(Revocation::RemappedAndRestored);
}

#[test]
fn actual_select_dispatch_admits_original_lookup_once_before_later_disable() {
    let (mut control, runtime) = fixture();
    let original = runtime.selected_projection().unwrap();
    let original_owner = runtime.mappings.lock().unwrap()[0].owner.clone();
    let owner_count = Arc::strong_count(&original_owner);
    let state = runtime.runtime_state.lock().unwrap();
    assert!(runtime.inject_midi_message(&[0x90, 60, 100]));
    wait_for_actual_lookup(&original_owner, owner_count);
    assert_eq!(state.selection_revision, original.0);
    drop(state);
    let receipt = event(&runtime);
    assert_eq!(receipt.action_key.as_deref(), Some(SELECT_B));
    assert!(receipt.dispatched);
    assert!(!receipt.direct);
    assert_eq!(receipt.selection_revision, Some(original.0 + 1));
    assert_eq!(
        runtime.selected_projection().unwrap(),
        (original.0 + 1, Some(1))
    );

    runtime.set_enabled(false);
    assert_eq!(
        runtime.selected_projection().unwrap(),
        (original.0 + 1, Some(1))
    );
    assert!(runtime.poll_event().is_none());
    assert!(control.consumer.pop().is_err());
}
