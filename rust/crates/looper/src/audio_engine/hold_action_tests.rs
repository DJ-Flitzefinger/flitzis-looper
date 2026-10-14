use super::*;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::messages::SampleBuffer;

fn prepared() -> Arc<NativeHoldAction> {
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    let sample = SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from([0.25; 128]),
    };
    ownership.publish_test_source(3, &sample, 8_000, 1);
    NativeHoldAction::prepare(
        ownership.clone(),
        ownership.slot_snapshot(3).unwrap(),
        1,
        17,
        8_765_432,
    )
    .unwrap()
}

#[test]
fn identity_reservation_fails_closed_before_reuse() {
    let counter = AtomicU64::new(u64::MAX - 1);
    assert_eq!(reserve_action_id(&counter), Ok(u64::MAX - 1));
    assert_eq!(
        reserve_action_id(&counter),
        Err("hold action identity exhausted")
    );
    assert_eq!(
        reserve_action_id(&counter),
        Err("hold action identity exhausted")
    );
}

#[test]
fn one_ticket_retains_early_release_without_inventing_a_pause_effect() {
    let action = prepared();
    assert_eq!(action.state(), HoldActionState::Prepared);
    assert!(action.mark_admitted());
    assert!(!action.mark_admitted());
    assert!(action.request_release());
    assert!(action.request_release());
    let projection = action.projection();
    assert_eq!(projection.state, HoldActionState::Admitted);
    assert!(projection.release_requested);
    assert!(!projection.release_admitted);
    assert!(!projection.effect_current);
    assert!(projection.effect.is_none());
    // Capacity failure performs no admission publication. Later polling owns the same Arc.
    assert!(!action.release_admitted());
    assert!(action.mark_release_admitted());
    assert!(!action.mark_release_admitted());
    assert!(action.release_requested());
}

#[test]
fn terminal_native_ack_cannot_be_reopened_by_late_duplicate_input() {
    let action = prepared();
    assert!(action.mark_admitted());
    action.settle(HoldActionState::Retired, 23);
    assert!(!action.request_release());
    assert!(!action.mark_release_admitted());
    assert!(!action.mark_admitted());
    action.settle(HoldActionState::Resumed, 24);
    assert_eq!(action.state(), HoldActionState::Retired);
    assert_eq!(action.projection().settled_at_output_frame, 23);
    assert!(action.effect().is_none());
}

#[test]
fn owning_ticket_keeps_exact_engine_owner_and_diagnostic_capture() {
    let action = prepared();
    let second = prepared();
    assert_ne!(action.action_id(), second.action_id());
    assert!(!Arc::ptr_eq(action.ownership(), second.ownership()));
    assert_eq!(action.captured().slot(), 3);
    assert_eq!(action.captured().cell().index(), 3);
    assert_eq!(action.captured().content_key(), Some(action.key()));
    assert!(action.key().cell().index() < NUM_SAMPLES);
    assert_eq!(action.projection().physical_press_id, 17);
    assert_eq!(action.received_at_ns, 8_765_432);
}

#[test]
fn preparation_rejects_empty_foreign_or_zero_identity_without_authority() {
    let action = prepared();
    let owner = action.ownership();
    let empty = owner.slot_snapshot(4).unwrap();
    assert!(NativeHoldAction::prepare(owner.clone(), empty, 1, 1, 0).is_err());
    assert!(NativeHoldAction::prepare(owner.clone(), action.captured(), 0, 1, 0).is_err());
    assert!(NativeHoldAction::prepare(owner.clone(), action.captured(), 1, 0, 0).is_err());
    let foreign = prepared();
    assert!(NativeHoldAction::prepare(owner.clone(), foreign.captured(), 1, 1, 0).is_err());
}
