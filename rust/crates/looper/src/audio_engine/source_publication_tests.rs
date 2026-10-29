//! Actual source publications fence stale asynchronous work by owner and content lifetime.

use super::*;
use crate::audio_engine::content_lifetime;
use crate::audio_engine::content_placement::{PlacementError, PreparedSlotWrite};

const ID: usize = 37;
const RATE: u32 = 48_000;
const GENERATION: u64 = 7;

#[test]
fn tracked_source_busy_stop_and_collapse_admissions_preserve_voices_fences_and_mode() {
    use crate::audio_engine::global_playback_batch::{
        prepare_explicit_stop, with_target_source_read_for_test,
    };
    use crate::audio_engine::input_mapping::{InputRuntime, launch_admission_tests::LaunchFixture};
    for collapse in [false, true] {
        let fixture = LaunchFixture::new(8);
        let ownership = fixture.engine.input_runtime_ownership.clone();
        assert!(ownership.source_tracking);
        let mut mixer = fixture.mixer();
        assert!(mixer.play_sample(0, 1.0));
        assert!(mixer.play_sample(1, 1.0));
        let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
        assert!(ownership.record_explicit_playback(key, 100, 7));
        let runtime = InputRuntime::new_with_ownership(
            fixture.producer.clone(),
            crate::audio_engine::timing::InputClock::new(),
            ownership.clone(),
        );
        let bindings: Vec<_> = (0..NUM_SAMPLES)
            .map(|id| capture(&fixture.engine, id).unwrap())
            .collect();
        let publish_image = |multi_loop| {
            runtime.set_runtime_state_with_pitch_projection(
                multi_loop,
                bindings.iter().map(Option::is_some).collect(),
                vec![0.0; NUM_SAMPLES],
                vec![None; NUM_SAMPLES],
                bindings.iter().map(Option::as_ref).collect(),
                None,
            )
        };
        assert!(publish_image(true).unwrap().is_none());
        let before_voices: Vec<_> = mixer
            .voices
            .iter()
            .map(|voice| {
                (
                    voice.active,
                    voice.paused,
                    voice.generation,
                    voice.frame_pos,
                )
            })
            .collect();
        let before_fences: Vec<_> = (0..NUM_SAMPLES)
            .map(|id| ownership.launch_revision(id))
            .collect();
        let before_hierarchy = ownership.hierarchy_stamp.load(Ordering::Acquire);
        let admit = || {
            if collapse {
                publish_image(false)
            } else {
                fixture
                    .engine
                    .stop_explicit_playback()
                    .map(Some)
                    .map_err(|error| error.to_string())
            }
        };
        ownership.sources[key.cell().index()]
            .generation
            .store(0, Ordering::SeqCst);
        assert!(
            admit().is_err(),
            "stable same-epoch unavailability must refuse before any fence"
        );
        ownership.sources[key.cell().index()]
            .generation
            .store(GENERATION, Ordering::SeqCst);
        let beginning = ownership.clone();
        let completing = ownership.clone();
        assert!(
            with_target_source_read_for_test(
                move || {
                    beginning.sources[key.cell().index()]
                        .generation
                        .store(0, Ordering::SeqCst);
                },
                move || {
                    let placement = completing.slot_snapshot(0).unwrap();
                    let write = completing
                        .prepare_slot_write_fixed(placement, placement.lifetime())
                        .unwrap();
                    completing
                        .commit_slot_write_fixed(
                            &write,
                            || true,
                            |_| {
                                completing.sources[key.cell().index()]
                                    .generation
                                    .store(GENERATION, Ordering::SeqCst);
                            },
                        )
                        .unwrap();
                },
                admit
            )
            .is_err(),
            "completion before row capture cannot turn busy target into an omitted target"
        );
        let prepared = prepare_explicit_stop(&ownership, collapse).unwrap();
        let placement = ownership.slot_snapshot(0).unwrap();
        let crossing = ownership
            .prepare_slot_write_fixed(placement, placement.lifetime())
            .unwrap();
        assert!(
            ownership
                .commit_slot_write_fixed(&crossing, || false, |_| unreachable!())
                .is_err()
        );
        let anchor = prepared.writes[0].placement;
        let write = ownership
            .prepare_slot_write_fixed(anchor, anchor.lifetime())
            .unwrap();
        assert!(
            ownership
                .commit_slot_write_fixed(
                    &write,
                    || prepared.current(&ownership),
                    |_| panic!(
                        "an intervening rejected source marker must not admit old preflight"
                    )
                )
                .is_err()
        );
        assert!(fixture.consumer.is_empty());
        assert_eq!(
            before_fences,
            (0..NUM_SAMPLES)
                .map(|id| ownership.launch_revision(id))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            before_hierarchy,
            ownership.hierarchy_stamp.load(Ordering::Acquire)
        );
        assert_eq!(
            before_voices,
            mixer
                .voices
                .iter()
                .map(|voice| (
                    voice.active,
                    voice.paused,
                    voice.generation,
                    voice.frame_pos
                ))
                .collect::<Vec<_>>()
        );
        assert!(
            publish_image(false).unwrap().is_some(),
            "both refused collapse attempts left the actual runtime image ON"
        );
    }
}

#[test]
fn explicit_target_tracked_source_unavailability_and_same_slot_aba_are_never_absent_history() {
    use crate::audio_engine::explicit_playback_target::{
        ExplicitPlaybackTarget, with_after_source_read_for_test,
    };
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    let pcm = sample(0.25);
    let publication = publish(&ownership, &pcm, GENERATION);
    let key = publication.key().unwrap();
    assert!(ownership.record_explicit_playback(key, 100, 7));
    let target = ExplicitPlaybackTarget {
        ownership: ownership.clone(),
        value: ownership.explicit_target_snapshot().unwrap().unwrap(),
    };
    let epoch = ownership
        .ordinary_source_identity_epoch(key.cell())
        .unwrap();
    // Exercise the actual tracked readiness field independently of semantic identity.
    ownership.sources[key.cell().index()]
        .generation
        .store(0, Ordering::SeqCst);
    assert!(target.current_control_slot().is_err());
    let finishing = ownership.clone();
    let result = with_after_source_read_for_test(
        move || {
            let write = current_write(&finishing);
            finishing
                .commit_slot_write_fixed(
                    &write,
                    || true,
                    |_| {
                        finishing.sources[key.cell().index()]
                            .generation
                            .store(GENERATION, Ordering::SeqCst);
                    },
                )
                .unwrap();
        },
        || target.current_control_slot(),
    );
    assert!(
        result.is_err(),
        "completion before the second same-slot read is a busy observation"
    );
    assert_eq!(
        ownership.ordinary_source_identity_epoch(key.cell()),
        Some(epoch)
    );
    assert_eq!(target.current_control_slot().unwrap(), Some(ID));
    let crossing = ownership.clone();
    assert!(
        with_after_source_read_for_test(
            move || {
                let write = current_write(&crossing);
                assert!(
                    crossing
                        .commit_slot_write_fixed(&write, || false, |_| unreachable!())
                        .is_err()
                );
            },
            || target.current_control_slot()
        )
        .is_err(),
        "even rejected same-slot ABA changes the observation token"
    );
    ownership.revoke_source(ID);
    assert_eq!(
        target.current_control_slot().unwrap(),
        None,
        "stable actual semantic revocation is invalid history"
    );
}

fn sample(value: f32) -> SampleBuffer {
    SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from([value; 128]),
    }
}

fn publish(
    ownership: &Arc<InputRuntimeOwnership>,
    sample: &SampleBuffer,
    generation: u64,
) -> SourcePublication {
    let captured = ownership.slot_snapshot(ID).unwrap();
    let lifetime = content_lifetime::reserve().unwrap();
    let prepared = ownership.prepare_slot_write(captured, lifetime).unwrap();
    let authority = ownership.authority_revision(ID).checked_add(1).unwrap();
    ownership
        .publish_source_write(
            &prepared,
            sample,
            RATE,
            generation,
            authority,
            Some(TimingIntent::Legacy),
            true,
            || true,
        )
        .unwrap()
}

fn current_write(ownership: &InputRuntimeOwnership) -> PreparedSlotWrite {
    let captured = ownership.slot_snapshot(ID).unwrap();
    ownership
        .prepare_slot_write_fixed(captured, captured.lifetime())
        .unwrap()
}

fn cold_row(ownership: &InputRuntimeOwnership, publication: &SourcePublication) -> u64 {
    ownership.cold_adoptions[publication.placement.cell().index()].load(Ordering::Acquire)
}

#[test]
fn stale_equal_pcm_publication_cannot_claim_finish_cancel_or_rollback_new_lifetime() {
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    let source = sample(0.25);
    let first = publish(&ownership, &source, GENERATION);
    let first_snapshot = ownership.slot_snapshot(ID).unwrap();
    let second = publish(&ownership, &source, GENERATION);
    let second_snapshot = ownership.slot_snapshot(ID).unwrap();
    assert_eq!(first.placement.cell(), second.placement.cell());
    assert_ne!(first.placement.lifetime(), second.placement.lifetime());
    assert_eq!(first.source_generation, second.source_generation);
    assert_eq!(
        second_snapshot.binding_epoch(),
        first_snapshot.binding_epoch() + 1
    );
    assert!(!first.current(&source, RATE));
    assert!(second.current(&source, RATE));
    assert!(ownership.source_publication_current(&second, &source, RATE));
    assert_eq!(cold_row(&ownership, &second), GENERATION * 4);

    // Both pending rows have identical generation bits: only the actual content owner fences
    // this old claim/unclaimed completion from consuming the newer source's pending admission.
    let write = current_write(&ownership);
    let rejected = ownership.commit_slot_write_fixed(
        &write,
        || {
            assert!(!ownership.claim_cold_publication(&first));
            assert!(!ownership.finish_cold_publication(&first, false, true));
            assert!(!ownership.finish_cold_publication(&first, false, false));
            false
        },
        |_| panic!("a stale source publication must not apply"),
    );
    assert_eq!(rejected, Err(PlacementError::ClaimRejected));
    assert_eq!(ownership.slot_snapshot(ID), Some(second_snapshot));
    assert_eq!(cold_row(&ownership, &second), GENERATION * 4);
    assert_eq!(ownership.cancel_cold_publication_fixed(&first), Ok(false));
    assert!(
        !ownership
            .rollback_source_write(
                &first,
                &source,
                RATE,
                first_snapshot,
                None,
                Some(TimingIntent::Legacy),
            )
            .unwrap()
    );
    assert_eq!(ownership.slot_snapshot(ID), Some(second_snapshot));
    assert_eq!(cold_row(&ownership, &second), GENERATION * 4);

    // An old claimed completion must also fail when the real new source has won its own claim.
    let write = current_write(&ownership);
    ownership
        .commit_slot_write_fixed(&write, || ownership.claim_cold_publication(&second), |_| {})
        .unwrap();
    assert_eq!(cold_row(&ownership, &second), GENERATION * 4 + 1);
    let write = current_write(&ownership);
    ownership
        .commit_slot_write_fixed(
            &write,
            || ownership.source_publication_current(&second, &source, RATE),
            |_| {
                assert!(!ownership.finish_cold_publication(&first, true, true));
                assert!(!ownership.finish_cold_publication(&first, true, false));
                assert_eq!(cold_row(&ownership, &second), GENERATION * 4 + 1);
                assert!(ownership.finish_cold_publication(&second, true, true));
            },
        )
        .unwrap();
    assert_eq!(cold_row(&ownership, &second), GENERATION * 4 + 2);
    assert!(second.current(&source, RATE));
    assert!(!first.current(&source, RATE));
}

#[test]
fn rejected_source_write_preserves_complete_predecessor_snapshot_source_and_authority() {
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    let source = sample(0.25);
    let replacement = sample(0.75);
    let current = publish(&ownership, &source, GENERATION);
    let before = ownership.slot_snapshot(ID).unwrap();
    let binding = current.binding(&source, RATE);
    let authority = ownership.authority_revision(ID);
    let row = cold_row(&ownership, &current);
    assert!(ownership.admission_current(ID, binding));
    let rejected_lifetime = content_lifetime::reserve().unwrap();
    let prepared = ownership
        .prepare_slot_write(before, rejected_lifetime)
        .unwrap();

    let rejected = ownership.publish_source_write(
        &prepared,
        &replacement,
        RATE,
        GENERATION + 1,
        authority + 1,
        Some(TimingIntent::Automatic),
        true,
        || {
            assert!(ownership.placement_pending());
            assert!(current.current(&source, RATE));
            assert!(!ownership.admission_current(ID, binding));
            false
        },
    );

    assert!(rejected.is_err());
    assert_eq!(ownership.slot_snapshot(ID), Some(before));
    assert_eq!(ownership.authority_revision(ID), authority);
    assert_eq!(
        ownership.source_generation(ID, &source, RATE),
        Some(GENERATION)
    );
    assert_eq!(ownership.source_generation(ID, &replacement, RATE), None);
    assert_eq!(cold_row(&ownership, &current), row);
    assert!(ownership.admission_current(ID, binding));
    assert!(current.current(&source, RATE));
    assert!(!ownership.content_key_current(prepared.after().content_key().unwrap()));
}

#[test]
fn current_source_rollback_restores_lifetime_without_reviving_old_slot_or_authority() {
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    let source = sample(0.25);
    let replacement = sample(0.75);
    let predecessor = publish(&ownership, &source, GENERATION);
    let before = ownership.slot_snapshot(ID).unwrap();
    let old_admission = predecessor.binding(&source, RATE);
    let current = publish(&ownership, &replacement, GENERATION + 1);
    let replacement_snapshot = ownership.slot_snapshot(ID).unwrap();
    let authority = ownership.authority_revision(ID);
    assert!(!ownership.slot_snapshot_current(before));
    assert!(!ownership.admission_current(ID, old_admission));

    assert!(
        ownership
            .rollback_source_write(
                &current,
                &replacement,
                RATE,
                before,
                Some((&source, RATE, GENERATION)),
                Some(TimingIntent::Legacy),
            )
            .unwrap()
    );

    let restored = ownership.slot_snapshot(ID).unwrap();
    assert_eq!(restored.cell(), before.cell());
    assert_eq!(restored.content_key(), before.content_key());
    assert_eq!(restored.lifetime(), before.lifetime());
    assert!(restored.binding_epoch() > replacement_snapshot.binding_epoch());
    assert_eq!(ownership.authority_revision(ID), authority);
    assert!(authority > predecessor.authority_revision);
    assert_eq!(
        ownership.source_generation(ID, &source, RATE),
        Some(GENERATION)
    );
    assert_eq!(ownership.source_generation(ID, &replacement, RATE), None);
    assert!(!ownership.slot_snapshot_current(before));
    assert!(!ownership.admission_current(ID, old_admission));
    assert!(!predecessor.current(&source, RATE));
    assert!(!current.current(&replacement, RATE));
    assert!(
        !ownership
            .rollback_source_write(
                &current,
                &replacement,
                RATE,
                before,
                Some((&source, RATE, GENERATION)),
                Some(TimingIntent::Legacy),
            )
            .unwrap()
    );
    assert_eq!(ownership.slot_snapshot(ID), Some(restored));
    let fresh_admission = InputPadBinding {
        placement: Some(restored),
        authority_revision: authority,
        ..old_admission
    };
    assert!(ownership.admission_current(ID, fresh_admission));
}

#[test]
fn foreign_engine_publication_cannot_consume_matching_local_source_or_cold_row() {
    let local = Arc::new(InputRuntimeOwnership::tracked());
    let foreign = Arc::new(InputRuntimeOwnership::tracked());
    let source = sample(0.25);
    let local_publication = publish(&local, &source, GENERATION);
    let foreign_publication = publish(&foreign, &source, GENERATION);
    let before = local.slot_snapshot(ID).unwrap();
    let row = cold_row(&local, &local_publication);
    assert!(local_publication.current(&source, RATE));
    assert!(foreign_publication.current(&source, RATE));
    assert_eq!(
        local_publication.source_generation,
        foreign_publication.source_generation
    );
    assert_eq!(
        local_publication.authority_revision,
        foreign_publication.authority_revision
    );
    assert!(!Arc::ptr_eq(
        &local_publication.ownership,
        &foreign_publication.ownership
    ));
    assert!(!local.source_publication_current(&foreign_publication, &source, RATE));
    assert!(!local.source_publication_binding_current(
        &foreign_publication,
        foreign_publication.binding(&source, RATE),
    ));
    assert!(!foreign.source_publication_current(&local_publication, &source, RATE));
    let write = current_write(&local);
    let rejected = local.commit_slot_write_fixed(
        &write,
        || {
            assert!(!local.claim_cold_publication(&foreign_publication));
            assert!(!local.finish_cold_publication(&foreign_publication, false, true));
            assert!(!local.finish_cold_publication(&foreign_publication, false, false));
            false
        },
        |_| panic!("a foreign source publication must not apply"),
    );
    assert_eq!(rejected, Err(PlacementError::ClaimRejected));
    assert_eq!(
        local.cancel_cold_publication_fixed(&foreign_publication),
        Ok(false)
    );
    assert!(
        !local
            .rollback_source_write(
                &foreign_publication,
                &source,
                RATE,
                before,
                None,
                Some(TimingIntent::Legacy),
            )
            .unwrap()
    );
    assert_eq!(local.slot_snapshot(ID), Some(before));
    assert_eq!(cold_row(&local, &local_publication), row);
    assert!(local_publication.current(&source, RATE));
    assert!(foreign_publication.current(&source, RATE));

    // Rejection of the foreign caller leaves the local request able to complete its own ACK.
    let write = current_write(&local);
    local
        .commit_slot_write_fixed(
            &write,
            || local.claim_cold_publication(&local_publication),
            |_| assert!(local.finish_cold_publication(&local_publication, true, true)),
        )
        .unwrap();
    assert_eq!(cold_row(&local, &local_publication), GENERATION * 4 + 2);
    assert_eq!(cold_row(&foreign, &foreign_publication), GENERATION * 4);
}
