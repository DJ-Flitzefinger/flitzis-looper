use super::*;
use crate::messages::SampleBuffer;

const SOURCE: usize = 3;
const TARGET: usize = 178;
const RATE: u32 = 8_000;

fn sample(value: f32) -> SampleBuffer {
    SampleBuffer {
        samples: Arc::from([value; 128]),
        channels: 1,
        residency: None,
    }
    .with_complete_source(RATE)
}

fn binding(
    ownership: &InputRuntimeOwnership,
    slot: usize,
    sample: &SampleBuffer,
) -> InputPadBinding {
    InputPadBinding {
        placement: ownership.slot_snapshot(slot),
        resident: sample.resident_binding(),
        source_address: sample.source_address(),
        sample_count: sample.source_sample_count(),
        channels: sample.channels,
        sample_rate_hz: RATE,
        authority_revision: ownership.authority_revision(slot),
        runtime_revision: ownership.runtime_revision(slot),
        accepted: None,
    }
}

struct GuardFixture {
    guard: PairRtGuard,
    _source: SampleBuffer,
    _target: SampleBuffer,
}

impl std::ops::Deref for GuardFixture {
    type Target = PairRtGuard;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

fn fixture(empty: bool) -> GuardFixture {
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    let source = sample(0.25);
    let target = sample(0.5);
    ownership.publish_test_source(SOURCE, &source, RATE, 11);
    if !empty {
        ownership.publish_test_source(TARGET, &target, RATE, 23);
    }
    let participants = [SOURCE, TARGET].map(|slot| {
        let snapshot = ownership.slot_snapshot(slot).unwrap();
        let binding = if slot == SOURCE {
            Some(binding(&ownership, slot, &source))
        } else if empty {
            None
        } else {
            Some(binding(&ownership, slot, &target))
        };
        PairParticipantGuard::capture(
            &ownership,
            &acknowledgements,
            snapshot,
            binding,
            Arc::new(AtomicU64::new(7)),
            (1..=3)
                .map(|epoch| (Arc::new(AtomicU64::new(epoch)), epoch))
                .collect(),
        )
        .unwrap()
    });
    let snapshots = participants
        .each_ref()
        .map(|participant| participant.placement());
    let guard = PairRtGuard::new(participants, ownership.clone(), acknowledgements, 313).unwrap();
    ownership
        .reserve_pair(&snapshots, guard.reservation)
        .unwrap();
    // The actual engine/mixer owns these PCM pins in production. Scalar guard fixtures keep
    // them for the entire test because a source address without its backing is not custody.
    GuardFixture {
        guard,
        _source: source,
        _target: target,
    }
}

#[test]
fn preparation_reservation_preserves_current_content_authority_and_counters() {
    let guard = fixture(false);
    for participant in &guard.participants {
        let key = participant.placement().content_key().unwrap();
        let binding = participant.control.binding().unwrap();
        assert!(guard.ownership.content_key_current(key));
        assert!(
            guard
                .ownership
                .current(participant.placement().slot(), binding)
        );
        assert!(
            !guard
                .ownership
                .admission_current(participant.placement().slot(), binding)
        );
        assert_eq!(participant.preparation_epoch.load(Ordering::Acquire), 7);
    }
    assert!(guard.stable_current());
    assert_eq!(guard.phase(), 0);
    assert!(guard.native_acknowledgement().is_none());
}

#[test]
fn both_original_epoch_owners_and_empty_participant_fields_are_required() {
    let guard = fixture(true);
    assert!(guard.stable_current());
    let source_before = guard.ownership.slot_snapshot(SOURCE).unwrap();
    let target_before = guard.ownership.slot_snapshot(TARGET).unwrap();
    guard.participants[1]
        .preparation_epoch
        .store(8, Ordering::Release);
    assert!(!guard.claim_stable());
    assert_eq!(guard.phase(), 0);
    assert_eq!(guard.ownership.slot_snapshot(SOURCE), Some(source_before));
    assert_eq!(guard.ownership.slot_snapshot(TARGET), Some(target_before));
    assert!(guard.native_acknowledgement().is_none());
}

#[test]
fn original_selection_registration_or_transport_change_rejects_whole_pair() {
    for participant in 0..2 {
        for fence in 0..3 {
            let guard = fixture(false);
            let (epoch, expected) = &guard.participants[participant].selection_fences[fence];
            epoch.store(expected + 1, Ordering::Release);
            assert!(!guard.claim_stable());
            assert_eq!(guard.phase(), 0);
            assert!(guard.native_acknowledgement().is_none());
        }
        let guard = fixture(false);
        let projection = guard.participants[participant].control;
        guard
            .ownership
            .transport_intent_for_cell(projection.placement().cell())
            .store(projection.transport_intent() + 1, Ordering::Release);
        assert!(!guard.claim_stable());
        assert_eq!(guard.phase(), 0);
    }
}

#[test]
fn cancelled_pair_can_never_claim_and_claimed_pair_can_never_cancel() {
    let cancelled = fixture(false);
    assert!(cancelled.reject());
    assert!(!cancelled.claim_stable());
    assert_eq!(cancelled.phase(), 3);
    assert!(cancelled.native_acknowledgement().is_none());
    let claimed = fixture(false);
    assert!(claimed.claim_stable());
    assert!(!claimed.reject());
    assert!(!claimed.claim_stable());
    assert_eq!(claimed.phase(), 1);
    assert!(claimed.native_acknowledgement().is_none());
    assert_eq!(
        claimed.ownership.pair_reservation(SOURCE),
        claimed.reservation
    );
    assert_eq!(
        claimed.ownership.pair_reservation(TARGET),
        claimed.reservation
    );
}

#[test]
fn whole_pair_receipt_publishes_complete_original_hold_effect_without_new_authority() {
    let guard = fixture(false);
    let work = PadPairRtWork::new(guard.guard.clone(), RATE, 1).unwrap();
    let key = guard.participants[0].placement().content_key().unwrap();
    let effect = NativePauseEffect {
        key,
        action_id: 41,
        effect_id: 41,
        transport_revision: 0,
        cohort_mask: (1 << 2) | (1 << 17),
        changed_mask: 1 << 17,
        generations: std::array::from_fn(|index| (index * 7 + 1) as u64),
        paused_at_output_frame: 991,
    };
    let hold = NativeHoldProjection {
        key,
        action_id: 41,
        transport_revision: 0,
        physical_press_id: 59,
        state: HoldActionState::PausedOwned,
        release_requested: true,
        release_admitted: false,
        effect_current: false,
        effect: Some(effect),
        settled_at_output_frame: 0,
    };
    let committed = guard
        .ownership
        .commit_placement_pair(&work.prepared, true, || guard.claim_stable(), || {})
        .unwrap();
    guard.acknowledge(committed, [Some(hold), None]);
    let receipt = guard.native_acknowledgement().unwrap();
    assert_eq!(receipt.kind, PairPlacementKind::Swap);
    assert!(receipt.source_after.matches(committed.source_after));
    assert!(receipt.target_after.matches(committed.target_after));
    assert_eq!(receipt.layout_revision, committed.layout_revision);
    assert_eq!(receipt.holds, [Some(hold), None]);
    assert_eq!(receipt.preparation_epochs, [7, 7]);
    assert_eq!(receipt.timing_acknowledgements, [0, 0]);
    assert_eq!(receipt.participants[0], guard.participants[0].control);
    // A historical revoked effect is retained as evidence, never promoted to an owned effect.
    assert!(!receipt.holds[0].unwrap().effect_current);
    assert_eq!(guard.ownership.callback_key_slot(key), Some(TARGET));
    assert_eq!(guard.ownership.pair_reservation(SOURCE), guard.reservation);
    assert_eq!(guard.ownership.pair_reservation(TARGET), guard.reservation);
}

#[test]
fn preparation_captures_post_reservation_revision_and_failure_leaves_image_unclaimed() {
    let guard = fixture(true);
    let work = PadPairRtWork::new(guard.guard.clone(), RATE, 1).unwrap();
    assert_eq!(work.kind(), PairPlacementKind::Move);
    assert_eq!(
        work.prepared.source(),
        guard.ownership.slot_snapshot(SOURCE).unwrap()
    );
    assert_eq!(
        work.prepared.target(),
        guard.ownership.slot_snapshot(TARGET).unwrap()
    );
    let source_before = guard.ownership.slot_snapshot(SOURCE).unwrap();
    let result = guard.ownership.commit_placement_pair(
        &work.prepared,
        false,
        || guard.claim_stable(),
        || panic!("no apply without capacity"),
    );
    assert_eq!(result, Err(PlacementError::CapacityUnavailable));
    assert_eq!(guard.phase(), 0);
    assert_eq!(guard.ownership.slot_snapshot(SOURCE), Some(source_before));
    assert!(guard.native_acknowledgement().is_none());
}
