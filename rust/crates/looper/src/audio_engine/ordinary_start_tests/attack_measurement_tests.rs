//! The original workers retain measured proposals without claiming audible alignment.
use super::*;
use crate::audio_engine::key_lock_start_policy::KeyLockStartBehavior;

fn attack_policy(fixture: &mut Fixture) {
    fixture.drain();
    // Only the actual render callback can republish idle after source admission.
    assert!(fixture.render(1).iter().all(|sample| *sample == 0.0));
    assert!(fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior("attack_at_grid")
            .unwrap()
    );
}

#[test]
fn actual_ordinary_and_retrigger_workers_keep_their_own_source_measurement() {
    let mut ordinary = Fixture::new(true);
    let mut retrigger = Fixture::new(true);
    attack_policy(&mut ordinary);
    attack_policy(&mut retrigger);
    let pitch = PitchIntent::new(0, 7).unwrap();
    ordinary.mixer.set_pad_pitch_intent(0, pitch).unwrap();
    let start = capture(&ordinary, 1.0);
    assert_eq!(start.start_behavior(), KeyLockStartBehavior::AttackAtGrid);
    ordinary.drain();
    assert!(
        ordinary
            .control
            .engine
            .publish_pad_start(&start, None)
            .unwrap()
    );
    ordinary.drain();
    let action = capture_future_pitch_reference(&mut retrigger, "pitch.extra:7:set_retrigger");
    assert_eq!(
        action.start_behavior(),
        Some(KeyLockStartBehavior::AttackAtGrid)
    );
    retrigger.publish(&action);
    wait_ready(&mut ordinary, &[start.sequence()]);
    wait_ready(&mut retrigger, &[action.sequence()]);
    let execution = start.execution_frame().unwrap();
    assert_eq!(action.execution_target(), action.original_due_frame());
    // rt_ready verifies a current, exact source/render measurement on each own
    // prepared work. Ambiguity/absence remain data, not a fabricated accepted C.
    let pcm = complete_attack(&mut ordinary, execution);
    let expected = complete_attack(&mut retrigger, action.execution_target());
    assert_eq!(pcm.len(), expected.len());
    assert_eq!(
        pcm.iter()
            .zip(&expected)
            .position(|(actual, expected)| actual != expected),
        None,
        "complete independent on-time piano beginning differs"
    );
    assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
    assert_eq!(start.state(), "applied", "{:?}", start.reason());
    assert!(action.effective(), "{:?}", action.reason());
    assert_eq!(start.effective_frame(), Some(execution));
    assert_eq!(ordinary.starts(0), 1);
    assert_eq!(retrigger.starts(0), 1);
    assert_eq!(ordinary.mixer.pad_pitch_intent(0), Some(pitch));
    assert_eq!(retrigger.mixer.pad_pitch_intent(0), Some(pitch));
}
