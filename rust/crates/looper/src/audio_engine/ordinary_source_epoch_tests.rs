use super::*;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;

#[test]
fn ordinary_transport_terminal_semantic_source_fence_is_explicitly_unavailable() {
    let mut fixture = LaunchFixture::new(4);
    let old = fixture.engine.pause_sample(0).unwrap();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let cell = ownership.slot_snapshot(0).unwrap().cell();
    ownership.sources[cell.index()]
        .ordinary_epoch
        .store(u64::MAX, Ordering::SeqCst);
    assert!(
        fixture
            .engine
            ._current_ordinary_transport_observation(0)
            .err()
            .unwrap()
            .to_string()
            .contains("ordinary transport observation unavailable")
    );
    assert!(
        fixture
            .engine
            .resume_sample(0)
            .err()
            .unwrap()
            .to_string()
            .contains("ordinary transport observation unavailable")
    );
    assert!(
        old.current_control_slot()
            .unwrap_err()
            .to_string()
            .contains("ordinary transport observation unavailable")
    );
    assert!(
        old.current_observation()
            .err()
            .unwrap()
            .to_string()
            .contains("ordinary transport observation unavailable")
    );
    let message = crate::messages::AudioMessage::OrdinaryTransportFeedback {
        id: 0,
        owner: ownership.owner_identity().diagnostic(),
        cell: cell.index(),
        lifetime: old.content_lifetime(),
        transport_revision: old.transport_revision(),
        observation_sequence: 2,
    };
    assert!(
        fixture
            .engine
            ._resolve_ordinary_transport_feedback(&message)
            .err()
            .unwrap()
            .to_string()
            .contains("ordinary transport observation unavailable")
    );
    ownership.revoke_source(0);
    assert_eq!(
        ownership.sources[cell.index()]
            .ordinary_epoch
            .load(Ordering::SeqCst),
        u64::MAX
    );
}
