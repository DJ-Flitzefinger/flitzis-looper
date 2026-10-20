use super::*;
use std::cell::{Cell, RefCell};

thread_local! {
    static BEFORE_MARKER_COMPARE_EXCHANGE: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

pub(super) fn before_marker_compare_exchange() {
    let hook = BEFORE_MARKER_COMPARE_EXCHANGE.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn with_before_marker_compare_exchange<T>(
    hook: impl FnOnce() + 'static,
    action: impl FnOnce() -> T,
) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            BEFORE_MARKER_COMPARE_EXCHANGE.with(|slot| *slot.borrow_mut() = None);
        }
    }
    BEFORE_MARKER_COMPARE_EXCHANGE.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    let _reset = Reset;
    action()
}

struct Fixture {
    owner: Arc<()>,
    placement: ContentPlacement,
    lifetimes: [AtomicU64; NUM_SAMPLES],
}

impl Fixture {
    fn new() -> Self {
        Self {
            owner: Arc::new(()),
            placement: ContentPlacement::new(),
            lifetimes: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn identity(&self) -> OwnerIdentity {
        OwnerIdentity::of(&self.owner)
    }

    fn snapshot(&self, slot: usize) -> SlotSnapshot {
        current(
            self.placement
                .capture_slot(self.identity(), &self.lifetimes, slot),
        )
        .unwrap()
    }

    fn assign(&self, slot: usize, lifetime: u64) -> ContentKey {
        let cell = self.snapshot(slot).cell();
        self.lifetimes[cell.index()].store(lifetime, Ordering::SeqCst);
        self.snapshot(slot).content_key().unwrap()
    }

    fn prepare(&self, source: usize, target: usize) -> PreparedPlacement {
        self.placement
            .prepare_pair(
                self.identity(),
                &self.lifetimes,
                self.snapshot(source),
                self.snapshot(target),
            )
            .unwrap()
    }

    fn commit(&self, source: usize, target: usize) -> PlacementCommit {
        let prepared = self.prepare(source, target);
        self.placement
            .commit_with(
                self.identity(),
                &self.lifetimes,
                &prepared,
                true,
                || true,
                || {},
            )
            .unwrap()
    }

    fn observe(&self, key: ContentKey) -> PlacementObservation<SlotSnapshot> {
        self.placement
            .observe_key(self.identity(), &self.lifetimes, key)
    }

    fn image(&self) -> Vec<SlotSnapshot> {
        (0..NUM_SAMPLES).map(|slot| self.snapshot(slot)).collect()
    }

    fn assert_permutation(&self) {
        let mut cells: Vec<_> = self
            .image()
            .iter()
            .map(|value| value.cell.index())
            .collect();
        cells.sort_unstable();
        assert_eq!(cells, (0..NUM_SAMPLES).collect::<Vec<_>>());
        for slot in 0..NUM_SAMPLES {
            let snapshot = self.snapshot(slot);
            assert_eq!(
                usize::from(
                    self.placement.cell_to_slot[snapshot.cell.index()].load(Ordering::SeqCst)
                ),
                slot
            );
        }
    }
}

#[test]
fn complete_initial_permutation_has_empty_cells_and_no_content_authority() {
    let fixture = Fixture::new();
    fixture.assert_permutation();
    assert_eq!(
        fixture.identity(),
        OwnerIdentity::of_ref(fixture.owner.as_ref())
    );
    for slot in 0..NUM_SAMPLES {
        let snapshot = fixture.snapshot(slot);
        assert_eq!(snapshot.slot(), slot);
        assert_eq!(snapshot.cell().index(), slot);
        assert_eq!(snapshot.binding_epoch(), 1);
        assert_eq!(snapshot.layout_revision(), 2);
        assert_eq!(snapshot.lifetime(), 0);
        assert_eq!(snapshot.content_key(), None);
        assert_eq!(
            ContentKey::new(fixture.identity(), snapshot.cell(), 0),
            None
        );
    }
    assert_eq!(ContentCell::from_index(NUM_SAMPLES), None);
    assert_eq!(
        fixture
            .placement
            .capture_slot(fixture.identity(), &fixture.lifetimes, NUM_SAMPLES),
        PlacementObservation::Stale
    );
}

#[test]
fn read_token_is_stable_for_unchanged_complete_reads_without_creating_content_authority() {
    let fixture = Fixture::new();
    let before = fixture.image();
    let token = fixture.placement.read_token().unwrap();
    assert_eq!(fixture.placement.read_token(), Some(token));
    assert_eq!(fixture.placement.read_token(), Some(token));
    assert_eq!(fixture.image(), before);
    assert!(
        before
            .iter()
            .all(|snapshot| snapshot.content_key().is_none())
    );
}

#[test]
fn pair_marker_read_token_changes_on_success_and_on_rejection_with_exact_snapshot_rollback() {
    let fixture = Fixture::new();
    let key = fixture.assign(0, 10);
    let prepared = fixture.prepare(0, 36);
    let before = fixture.image();
    let token = fixture.placement.read_token().unwrap();
    assert_eq!(
        fixture.placement.commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            false,
            || panic!("unavailable capacity must not claim"),
            || panic!("unavailable capacity must not apply"),
        ),
        Err(PlacementError::CapacityUnavailable)
    );
    assert_eq!(fixture.placement.read_token(), Some(token));
    assert_eq!(
        fixture.placement.commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            true,
            || {
                assert!(fixture.placement.pending());
                assert_eq!(fixture.placement.read_token(), None);
                assert!(
                    fixture
                        .placement
                        .key_current(fixture.identity(), &fixture.lifetimes, key)
                );
                false
            },
            || panic!("rejected claim must not apply"),
        ),
        Err(PlacementError::ClaimRejected)
    );
    assert_eq!(fixture.image(), before);
    assert!(!fixture.placement.pending());
    let rejected = fixture.placement.read_token().unwrap();
    assert_ne!(rejected, token);
    assert_eq!(fixture.placement.read_token(), Some(rejected));
    fixture
        .placement
        .commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            true,
            || {
                assert_eq!(fixture.placement.read_token(), None);
                true
            },
            || assert_eq!(fixture.placement.read_token(), None),
        )
        .unwrap();
    assert_ne!(fixture.placement.read_token().unwrap(), rejected);
    assert_eq!(current(fixture.observe(key)).unwrap().slot(), 36);
    fixture.assert_permutation();
}

#[test]
fn slot_marker_read_token_changes_on_success_and_on_rejection_with_exact_snapshot_rollback() {
    let fixture = Fixture::new();
    let key = fixture.assign(0, 11);
    let before = fixture.image();
    let prepared = fixture
        .placement
        .prepare_slot_write(
            fixture.identity(),
            &fixture.lifetimes,
            fixture.snapshot(0),
            11,
        )
        .unwrap();
    let token = fixture.placement.read_token().unwrap();
    assert_eq!(
        fixture.placement.commit_slot_write(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            || {
                assert!(fixture.placement.pending());
                assert_eq!(fixture.placement.read_token(), None);
                false
            },
            |_| panic!("rejected claim must not apply"),
        ),
        Err(PlacementError::ClaimRejected)
    );
    assert_eq!(fixture.image(), before);
    assert!(!fixture.placement.pending());
    let rejected = fixture.placement.read_token().unwrap();
    assert_ne!(rejected, token);
    fixture
        .placement
        .commit_slot_write(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            || {
                assert_eq!(fixture.placement.read_token(), None);
                true
            },
            |_| assert_eq!(fixture.placement.read_token(), None),
        )
        .unwrap();
    assert_ne!(fixture.placement.read_token().unwrap(), rejected);
    assert!(
        fixture
            .placement
            .key_current(fixture.identity(), &fixture.lifetimes, key)
    );
    fixture.assert_permutation();
}

#[test]
fn exhausted_observation_generation_rejects_both_writers_without_callbacks_or_content_mutation() {
    let fixture = Fixture::new();
    fixture.assign(0, 12);
    let pair = fixture.prepare(0, 36);
    let slot = fixture
        .placement
        .prepare_slot_write(
            fixture.identity(),
            &fixture.lifetimes,
            fixture.snapshot(0),
            13,
        )
        .unwrap();
    let before = fixture.image();
    fixture
        .placement
        .observation_generation
        .store(u64::MAX, Ordering::SeqCst);
    assert_eq!(fixture.placement.read_token(), None);
    assert_eq!(
        fixture.placement.read_token_result(),
        Err(PlacementError::ObservationGenerationExhausted)
    );
    assert_eq!(
        fixture.placement.commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &pair,
            true,
            || panic!("exhaustion must precede claim"),
            || panic!("exhaustion must precede apply"),
        ),
        Err(PlacementError::ObservationGenerationExhausted)
    );
    assert_eq!(
        fixture.placement.commit_slot_write(
            fixture.identity(),
            &fixture.lifetimes,
            &slot,
            || panic!("exhaustion must precede claim"),
            |_| panic!("exhaustion must precede apply"),
        ),
        Err(PlacementError::ObservationGenerationExhausted)
    );
    assert_eq!(fixture.image(), before);
    assert!(!fixture.placement.pending());
    assert_eq!(
        fixture
            .placement
            .observation_generation
            .load(Ordering::SeqCst),
        u64::MAX
    );
    assert_eq!(fixture.placement.read_token(), None);
}

#[test]
fn final_observation_generation_is_permanently_unobservable_after_rejected_claim() {
    let fixture = Fixture::new();
    fixture.assign(0, 14);
    let pair = fixture.prepare(0, 36);
    let before = fixture.image();
    fixture
        .placement
        .observation_generation
        .store(u64::MAX - 1, Ordering::SeqCst);
    assert_eq!(fixture.placement.read_token(), Some(u64::MAX - 1));
    assert_eq!(
        fixture.placement.commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &pair,
            true,
            || {
                assert_eq!(fixture.placement.read_token(), None);
                false
            },
            || panic!("rejected claim must not apply"),
        ),
        Err(PlacementError::ClaimRejected)
    );
    assert_eq!(fixture.image(), before);
    assert!(!fixture.placement.pending());
    assert_eq!(fixture.placement.read_token(), None);
    assert_eq!(
        fixture
            .placement
            .observation_generation
            .load(Ordering::SeqCst),
        u64::MAX
    );
    assert_eq!(
        fixture.placement.commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &pair,
            true,
            || panic!("already exhausted generation must not claim again"),
            || panic!("already exhausted generation must not apply"),
        ),
        Err(PlacementError::ObservationGenerationExhausted)
    );
    assert_eq!(fixture.image(), before);
}

#[test]
fn rejected_marker_between_capacity_check_and_cas_cannot_wrap_or_call_stale_writer() {
    let fixture = Arc::new(Fixture::new());
    fixture.assign(0, 15);
    fixture
        .placement
        .observation_generation
        .store(u64::MAX - 1, Ordering::SeqCst);
    let before = fixture.image();
    let pair = fixture.prepare(0, 36);
    let crossed = fixture.clone();
    let result = with_before_marker_compare_exchange(
        move || {
            let slot = crossed
                .placement
                .prepare_slot_write(
                    crossed.identity(),
                    &crossed.lifetimes,
                    crossed.snapshot(0),
                    15,
                )
                .unwrap();
            assert_eq!(
                crossed.placement.commit_slot_write(
                    crossed.identity(),
                    &crossed.lifetimes,
                    &slot,
                    || false,
                    |_| panic!("intervening rejected marker must not apply"),
                ),
                Err(PlacementError::ClaimRejected)
            );
        },
        || {
            fixture.placement.commit_with(
                fixture.identity(),
                &fixture.lifetimes,
                &pair,
                true,
                || panic!("post-CAS actual exhaustion must precede claim"),
                || panic!("post-CAS actual exhaustion must precede apply"),
            )
        },
    );
    assert_eq!(result, Err(PlacementError::ObservationGenerationExhausted));
    assert_eq!(fixture.image(), before);
    assert!(!fixture.placement.pending());
    assert_eq!(
        fixture
            .placement
            .observation_generation
            .load(Ordering::SeqCst),
        u64::MAX
    );
    assert_eq!(fixture.placement.read_token(), None);
}

#[test]
fn rejected_marker_between_capture_and_cas_is_counted_separately_from_outer_rejection() {
    let fixture = Arc::new(Fixture::new());
    fixture.assign(0, 16);
    let before = fixture.image();
    let token = fixture.placement.read_token().unwrap();
    let pair = fixture.prepare(0, 36);
    let crossed = fixture.clone();
    let result = with_before_marker_compare_exchange(
        move || {
            let slot = crossed
                .placement
                .prepare_slot_write(
                    crossed.identity(),
                    &crossed.lifetimes,
                    crossed.snapshot(0),
                    16,
                )
                .unwrap();
            assert_eq!(
                crossed.placement.commit_slot_write(
                    crossed.identity(),
                    &crossed.lifetimes,
                    &slot,
                    || false,
                    |_| panic!("intervening rejected marker must not apply"),
                ),
                Err(PlacementError::ClaimRejected)
            );
            assert_eq!(crossed.placement.read_token(), Some(token + 1));
        },
        || {
            fixture.placement.commit_with(
                fixture.identity(),
                &fixture.lifetimes,
                &pair,
                true,
                || false,
                || panic!("outer rejected marker must not apply"),
            )
        },
    );
    assert_eq!(result, Err(PlacementError::ClaimRejected));
    assert_eq!(fixture.image(), before);
    assert!(!fixture.placement.pending());
    assert_eq!(fixture.placement.read_token(), Some(token + 2));
}

#[test]
fn final_successful_claim_may_consume_max_but_later_content_mutation_is_fenced() {
    let fixture = Fixture::new();
    let key = fixture.assign(0, 17);
    fixture
        .placement
        .observation_generation
        .store(u64::MAX - 1, Ordering::SeqCst);
    let pair = fixture.prepare(0, 36);
    fixture
        .placement
        .commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &pair,
            true,
            || {
                assert_eq!(fixture.placement.read_token(), None);
                true
            },
            || assert_eq!(fixture.placement.read_token(), None),
        )
        .unwrap();
    assert!(!fixture.placement.pending());
    assert_eq!(fixture.placement.read_token(), None);
    assert_eq!(current(fixture.observe(key)).unwrap().slot(), 36);
    assert_eq!(
        fixture
            .placement
            .observation_generation
            .load(Ordering::SeqCst),
        u64::MAX
    );
    let before = fixture.image();
    let later = fixture.prepare(36, 215);
    assert_eq!(
        fixture.placement.commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &later,
            true,
            || panic!("MAX must fence subsequent content mutation"),
            || panic!("MAX must fence subsequent content mutation"),
        ),
        Err(PlacementError::ObservationGenerationExhausted)
    );
    assert_eq!(fixture.image(), before);
}

#[test]
fn move_returns_empty_cell_and_old_slot_reuse_cannot_redirect_content_key() {
    let fixture = Fixture::new();
    let key_a = fixture.assign(0, 10);
    let old_source = fixture.snapshot(0);
    let empty_target = fixture.snapshot(36);
    let prepared = fixture.prepare(0, 36);
    assert_eq!(prepared.kind(), PairPlacementKind::Move);
    assert_eq!(prepared.source(), old_source);
    assert_eq!(prepared.target(), empty_target);
    let committed = fixture.commit(0, 36);
    assert_eq!(committed.target_after.content_key(), Some(key_a));
    assert_eq!(committed.source_after.cell(), empty_target.cell());
    assert_eq!(committed.source_after.content_key(), None);
    assert_eq!(committed.source_after.binding_epoch(), 2);
    assert_eq!(committed.target_after.binding_epoch(), 2);
    assert_eq!(committed.layout_revision, 4);
    assert_eq!(current(fixture.observe(key_a)).unwrap().slot(), 36);
    assert_eq!(
        fixture
            .placement
            .observe_snapshot(fixture.identity(), &fixture.lifetimes, old_source,),
        PlacementObservation::Stale
    );

    let key_b = fixture.assign(0, 11);
    assert_ne!(key_a.cell(), key_b.cell());
    assert_eq!(current(fixture.observe(key_a)).unwrap().slot(), 36);
    assert_eq!(current(fixture.observe(key_b)).unwrap().slot(), 0);
    assert!(
        fixture
            .placement
            .key_current(fixture.identity(), &fixture.lifetimes, key_a)
    );
    fixture.assert_permutation();
}

#[test]
fn swap_and_repeated_moves_preserve_keys_but_fence_slot_aba() {
    let fixture = Fixture::new();
    let key_a = fixture.assign(0, 20);
    let key_b = fixture.assign(36, 21);
    let old_source = fixture.snapshot(0);
    assert_eq!(fixture.prepare(0, 36).kind(), PairPlacementKind::Swap);
    fixture.commit(0, 36);
    assert_eq!(current(fixture.observe(key_a)).unwrap().slot(), 36);
    assert_eq!(current(fixture.observe(key_b)).unwrap().slot(), 0);
    fixture.commit(36, 215);
    fixture.commit(215, 0);
    assert_eq!(current(fixture.observe(key_a)).unwrap().slot(), 0);
    assert_eq!(current(fixture.observe(key_b)).unwrap().slot(), 215);
    assert_eq!(fixture.snapshot(0).content_key(), Some(key_a));
    assert!(fixture.snapshot(0).binding_epoch() > old_source.binding_epoch());
    assert_eq!(
        fixture
            .placement
            .observe_snapshot(fixture.identity(), &fixture.lifetimes, old_source,),
        PlacementObservation::Stale
    );
    assert_eq!(
        fixture.lifetimes[key_a.cell().index()].load(Ordering::SeqCst),
        20
    );
    assert_eq!(
        fixture.lifetimes[key_b.cell().index()].load(Ordering::SeqCst),
        21
    );
    fixture.assert_permutation();
}

#[test]
fn unrelated_pair_does_not_revoke_unchanged_slot_snapshot() {
    let fixture = Fixture::new();
    fixture.assign(0, 30);
    fixture.assign(36, 31);
    let unchanged = fixture.snapshot(0);
    fixture.commit(36, 215);
    let observed = current(fixture.placement.observe_snapshot(
        fixture.identity(),
        &fixture.lifetimes,
        unchanged,
    ))
    .unwrap();
    assert_eq!(observed.content_key(), unchanged.content_key());
    assert_eq!(observed.binding_epoch(), unchanged.binding_epoch());
    assert_ne!(observed.layout_revision(), unchanged.layout_revision());
    let prepared = fixture
        .placement
        .prepare_pair(
            fixture.identity(),
            &fixture.lifetimes,
            unchanged,
            fixture.snapshot(1),
        )
        .unwrap();
    assert_eq!(
        prepared.source().layout_revision(),
        observed.layout_revision()
    );
}

#[test]
fn cell_reuse_and_zero_lifetime_retire_old_current_key() {
    let fixture = Fixture::new();
    let old_key = fixture.assign(0, 40);
    fixture.commit(0, 36);
    let old_snapshot = fixture.snapshot(36);
    fixture.lifetimes[old_key.cell().index()].store(0, Ordering::SeqCst);
    assert_eq!(fixture.observe(old_key), PlacementObservation::Retired);
    assert!(
        !fixture
            .placement
            .key_current(fixture.identity(), &fixture.lifetimes, old_key)
    );
    let replacement = fixture.assign(36, 41);
    assert_eq!(replacement.cell(), old_key.cell());
    assert_ne!(replacement.lifetime(), old_key.lifetime());
    assert_eq!(fixture.observe(old_key), PlacementObservation::Retired);
    assert_eq!(current(fixture.observe(replacement)).unwrap().slot(), 36);
    assert_eq!(
        fixture
            .placement
            .observe_snapshot(fixture.identity(), &fixture.lifetimes, old_snapshot,),
        PlacementObservation::Retired
    );
}

#[test]
fn prospective_snapshot_confers_no_key_authority_before_exact_native_publication() {
    let fixture = Fixture::new();
    let empty = fixture.snapshot(36);
    assert_eq!(empty.for_lifetime(0), None);
    let prospective = empty.for_lifetime(45).unwrap();
    let key = prospective.content_key().unwrap();
    assert_eq!(prospective.cell(), empty.cell());
    assert_eq!(prospective.binding_epoch(), empty.binding_epoch());
    assert_eq!(prospective.layout_revision(), empty.layout_revision());
    assert!(
        !fixture
            .placement
            .key_current(fixture.identity(), &fixture.lifetimes, key)
    );
    assert_eq!(fixture.observe(key), PlacementObservation::Retired);
    assert_eq!(fixture.snapshot(36), empty);
    assert_eq!(fixture.assign(36, 45), key);
    assert!(
        fixture
            .placement
            .key_current(fixture.identity(), &fixture.lifetimes, key)
    );
    assert_eq!(current(fixture.observe(key)).unwrap(), prospective);
}

#[test]
fn pending_publication_keeps_content_guard_valid_and_hides_partial_routing() {
    let fixture = Fixture::new();
    let key_a = fixture.assign(0, 50);
    let key_b = fixture.assign(36, 51);
    let prepared = fixture.prepare(0, 36);
    let pending_source = prepared.source();
    let nested = fixture.prepare(0, 215);
    let applied = Cell::new(false);
    fixture
        .placement
        .commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            true,
            || {
                assert!(fixture.placement.key_current(
                    fixture.identity(),
                    &fixture.lifetimes,
                    key_a
                ));
                assert!(fixture.placement.key_current(
                    fixture.identity(),
                    &fixture.lifetimes,
                    key_b
                ));
                assert_eq!(fixture.observe(key_a), PlacementObservation::Pending);
                true
            },
            || {
                applied.set(true);
                assert_eq!(
                    fixture
                        .placement
                        .capture_slot(fixture.identity(), &fixture.lifetimes, 0),
                    PlacementObservation::Pending
                );
                assert_eq!(
                    fixture.placement.observe_snapshot(
                        fixture.identity(),
                        &fixture.lifetimes,
                        pending_source,
                    ),
                    PlacementObservation::Pending
                );
                assert_eq!(
                    fixture.placement.commit_with(
                        fixture.identity(),
                        &fixture.lifetimes,
                        &nested,
                        true,
                        || panic!("a pending writer must not call another claim"),
                        || panic!("a pending writer must not apply another image"),
                    ),
                    Err(PlacementError::Pending)
                );
            },
        )
        .unwrap();
    assert!(applied.get());
    assert_eq!(current(fixture.observe(key_a)).unwrap().slot(), 36);
    assert_eq!(current(fixture.observe(key_b)).unwrap().slot(), 0);
}

#[test]
fn capacity_and_rejected_claim_leave_mapping_epochs_and_revision_unchanged() {
    let fixture = Fixture::new();
    fixture.assign(0, 60);
    let prepared = fixture.prepare(0, 36);
    let before = fixture.image();
    assert_eq!(
        fixture.placement.commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            false,
            || panic!("capacity must precede native claim"),
            || panic!("capacity failure must not apply"),
        ),
        Err(PlacementError::CapacityUnavailable)
    );
    assert_eq!(fixture.image(), before);
    assert_eq!(
        fixture.placement.commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            true,
            || false,
            || panic!("rejected native claim must not apply"),
        ),
        Err(PlacementError::ClaimRejected)
    );
    assert_eq!(fixture.image(), before);
    fixture
        .placement
        .commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            true,
            || true,
            || {},
        )
        .unwrap();
    fixture.assert_permutation();
}

#[test]
fn foreign_owner_cannot_observe_prepare_or_publish_equal_numbered_content() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let key = fixture.assign(0, 70);
    other.assign(0, 70);
    assert_ne!(fixture.identity(), other.identity());
    assert_eq!(other.observe(key), PlacementObservation::Stale);
    assert!(
        !other
            .placement
            .key_current(other.identity(), &other.lifetimes, key)
    );
    let source = fixture.snapshot(0);
    let target = fixture.snapshot(36);
    assert_eq!(
        other
            .placement
            .prepare_pair(other.identity(), &other.lifetimes, source, target)
            .unwrap_err(),
        PlacementError::ForeignOwner
    );
    let prepared = fixture.prepare(0, 36);
    let before = other.image();
    assert_eq!(
        other.placement.commit_with(
            other.identity(),
            &other.lifetimes,
            &prepared,
            true,
            || panic!("foreign key cannot reach claim"),
            || panic!("foreign key cannot reach apply"),
        ),
        Err(PlacementError::ForeignOwner)
    );
    assert_eq!(other.image(), before);
}

#[test]
fn stale_prepared_transaction_cannot_replay_after_placement_aba() {
    let fixture = Fixture::new();
    fixture.assign(0, 80);
    let stale = fixture.prepare(0, 36);
    fixture.commit(0, 36);
    fixture.commit(36, 0);
    let before = fixture.image();
    assert_eq!(
        fixture.placement.commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &stale,
            true,
            || panic!("stale transaction cannot reach claim"),
            || panic!("stale transaction cannot reach apply"),
        ),
        Err(PlacementError::Stale)
    );
    assert_eq!(fixture.image(), before);
}

#[test]
fn same_slot_empty_source_and_epoch_overflow_preserve_complete_image() {
    let fixture = Fixture::new();
    fixture.assign(0, 90);
    let before = fixture.image();
    assert_eq!(
        fixture
            .placement
            .prepare_pair(
                fixture.identity(),
                &fixture.lifetimes,
                fixture.snapshot(0),
                fixture.snapshot(0),
            )
            .unwrap_err(),
        PlacementError::SameSlot
    );
    assert_eq!(
        fixture
            .placement
            .prepare_pair(
                fixture.identity(),
                &fixture.lifetimes,
                fixture.snapshot(36),
                fixture.snapshot(0),
            )
            .unwrap_err(),
        PlacementError::EmptySource
    );
    assert_eq!(fixture.image(), before);

    fixture.placement.slot_epochs[36].store(u64::MAX, Ordering::SeqCst);
    let before = fixture.image();
    assert_eq!(
        fixture
            .placement
            .prepare_pair(
                fixture.identity(),
                &fixture.lifetimes,
                fixture.snapshot(0),
                fixture.snapshot(36),
            )
            .unwrap_err(),
        PlacementError::SlotEpochExhausted
    );
    assert_eq!(fixture.image(), before);
    fixture.placement.slot_epochs[36].store(1, Ordering::SeqCst);
    fixture
        .placement
        .revision
        .store(u64::MAX - 1, Ordering::SeqCst);
    let before = fixture.image();
    assert_eq!(
        fixture
            .placement
            .prepare_pair(
                fixture.identity(),
                &fixture.lifetimes,
                fixture.snapshot(0),
                fixture.snapshot(36),
            )
            .unwrap_err(),
        PlacementError::LayoutRevisionExhausted
    );
    assert_eq!(fixture.image(), before);
}

#[test]
fn lifetime_changed_after_preparation_rejects_before_claim_and_keeps_replacement() {
    let fixture = Fixture::new();
    let old = fixture.assign(0, 100);
    let prepared = fixture.prepare(0, 36);
    let replacement = fixture.assign(0, 101);
    let before = fixture.image();
    assert_eq!(
        fixture.placement.commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            true,
            || panic!("retired source cannot reach claim"),
            || panic!("retired source cannot reach apply"),
        ),
        Err(PlacementError::Retired)
    );
    assert_eq!(fixture.image(), before);
    assert_eq!(fixture.observe(old), PlacementObservation::Retired);
    assert_eq!(current(fixture.observe(replacement)).unwrap().slot(), 0);
}

#[test]
fn interrupted_irreversible_apply_stays_pending_without_guessed_rollback() {
    let fixture = Fixture::new();
    let key = fixture.assign(0, 110);
    let prepared = fixture.prepare(0, 36);
    let native_image_changed = Cell::new(false);
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = fixture.placement.commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            true,
            || true,
            || {
                native_image_changed.set(true);
                panic!("simulated interruption after irreversible native claim");
            },
        );
    }));
    assert!(interrupted.is_err());
    assert!(native_image_changed.get());
    assert_eq!(fixture.observe(key), PlacementObservation::Pending);
    assert!(
        fixture
            .placement
            .key_current(fixture.identity(), &fixture.lifetimes, key)
    );
    assert_eq!(
        fixture.placement.commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            true,
            || panic!("uncertain claim cannot be reclaimed"),
            || panic!("uncertain image cannot be applied again"),
        ),
        Err(PlacementError::Pending)
    );
}

#[test]
fn slot_write_rejected_claim_preserves_exact_revision_lifetime_and_slot_epochs() {
    let fixture = Fixture::new();
    let key = fixture.assign(0, 201);
    fixture.assign(36, 202);
    let captured = fixture.snapshot(0);
    let before = fixture.image();
    let prepared = fixture
        .placement
        .prepare_slot_write(fixture.identity(), &fixture.lifetimes, captured, 203)
        .unwrap();
    assert_eq!(prepared.before(), captured);
    assert_eq!(prepared.after().lifetime(), 203);
    assert_eq!(
        prepared.after().binding_epoch(),
        captured.binding_epoch() + 1
    );
    let claimed = Cell::new(false);
    assert_eq!(
        fixture.placement.commit_slot_write(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            || {
                claimed.set(true);
                assert!(fixture.placement.pending());
                assert!(
                    fixture
                        .placement
                        .key_current(fixture.identity(), &fixture.lifetimes, key)
                );
                false
            },
            |_| panic!("a rejected slot write must not apply"),
        ),
        Err(PlacementError::ClaimRejected)
    );
    assert!(claimed.get());
    assert!(!fixture.placement.pending());
    assert_eq!(fixture.image(), before);
    assert_eq!(
        fixture.observe(key),
        PlacementObservation::Current(captured)
    );
    assert_eq!(
        fixture.lifetimes[key.cell().index()].load(Ordering::SeqCst),
        201
    );
}

#[test]
fn slot_write_new_lifetime_bumps_only_own_epoch_and_preserves_unrelated_admission() {
    let fixture = Fixture::new();
    let old_key = fixture.assign(0, 211);
    let other_key = fixture.assign(36, 212);
    let before = fixture.snapshot(0);
    let other = fixture.snapshot(36);
    let image = fixture.image();
    let prepared = fixture
        .placement
        .prepare_slot_write(fixture.identity(), &fixture.lifetimes, before, 213)
        .unwrap();
    let applied = Cell::new(false);
    let after = fixture
        .placement
        .commit_slot_write(
            fixture.identity(),
            &fixture.lifetimes,
            &prepared,
            || true,
            |cell| {
                applied.set(true);
                assert_eq!(cell, before.cell());
                assert!(fixture.placement.pending());
                assert_eq!(fixture.lifetimes[cell.index()].load(Ordering::SeqCst), 211);
            },
        )
        .unwrap();
    assert!(applied.get());
    assert_eq!(after.cell(), before.cell());
    assert_eq!(after.binding_epoch(), before.binding_epoch() + 1);
    assert_eq!(after.layout_revision(), before.layout_revision() + 2);
    assert_eq!(after.lifetime(), 213);
    assert_eq!(fixture.observe(old_key), PlacementObservation::Retired);
    assert_eq!(
        current(
            fixture
                .placement
                .observe_snapshot(fixture.identity(), &fixture.lifetimes, other)
        )
        .unwrap()
        .content_key(),
        Some(other_key)
    );
    for (slot, previous) in image.iter().enumerate() {
        let current = fixture.snapshot(slot);
        assert_eq!(current.cell(), previous.cell());
        if slot != 0 {
            assert_eq!(current.binding_epoch(), previous.binding_epoch());
            assert_eq!(current.lifetime(), previous.lifetime());
        }
    }
    let same_lifetime = fixture
        .placement
        .prepare_slot_write(fixture.identity(), &fixture.lifetimes, after, 213)
        .unwrap();
    let maintained = fixture
        .placement
        .commit_slot_write(
            fixture.identity(),
            &fixture.lifetimes,
            &same_lifetime,
            || true,
            |_| {},
        )
        .unwrap();
    assert_eq!(maintained.content_key(), after.content_key());
    assert_eq!(maintained.binding_epoch(), after.binding_epoch());
    assert_eq!(maintained.layout_revision(), after.layout_revision() + 2);
    fixture.assert_permutation();
}

#[test]
fn pending_slot_write_blocks_pair_prepare_and_commit_before_claim_or_apply() {
    let fixture = Fixture::new();
    fixture.assign(0, 221);
    fixture.assign(36, 222);
    let source = fixture.snapshot(0);
    let target = fixture.snapshot(36);
    let pair = fixture.prepare(0, 36);
    let slot_write = fixture
        .placement
        .prepare_slot_write(fixture.identity(), &fixture.lifetimes, source, 223)
        .unwrap();
    let after = fixture
        .placement
        .commit_slot_write(
            fixture.identity(),
            &fixture.lifetimes,
            &slot_write,
            || true,
            |_| {
                assert!(fixture.placement.pending());
                assert!(matches!(
                    fixture.placement.prepare_pair(
                        fixture.identity(),
                        &fixture.lifetimes,
                        source,
                        target
                    ),
                    Err(PlacementError::Pending)
                ));
                assert_eq!(
                    fixture.placement.commit_with(
                        fixture.identity(),
                        &fixture.lifetimes,
                        &pair,
                        true,
                        || panic!("a pending source writer must fence Pair claim"),
                        || panic!("a pending source writer must fence Pair apply"),
                    ),
                    Err(PlacementError::Pending)
                );
            },
        )
        .unwrap();
    assert_eq!(after.cell(), source.cell());
    assert_eq!(fixture.snapshot(36).cell(), target.cell());
    assert_eq!(fixture.snapshot(36).binding_epoch(), target.binding_epoch());
    assert_eq!(after.lifetime(), 223);
    fixture.assert_permutation();
}

#[test]
fn pending_pair_blocks_slot_write_prepare_and_commit_without_write_apply() {
    let fixture = Fixture::new();
    let source_key = fixture.assign(0, 231);
    let target_key = fixture.assign(36, 232);
    let unrelated_key = fixture.assign(215, 233);
    let unrelated = fixture.snapshot(215);
    let pair = fixture.prepare(0, 36);
    let slot_write = fixture
        .placement
        .prepare_slot_write(fixture.identity(), &fixture.lifetimes, unrelated, 234)
        .unwrap();
    fixture
        .placement
        .commit_with(
            fixture.identity(),
            &fixture.lifetimes,
            &pair,
            true,
            || true,
            || {
                assert!(fixture.placement.pending());
                assert!(matches!(
                    fixture.placement.prepare_slot_write(
                        fixture.identity(),
                        &fixture.lifetimes,
                        unrelated,
                        234,
                    ),
                    Err(PlacementError::Pending)
                ));
                assert_eq!(
                    fixture.placement.commit_slot_write(
                        fixture.identity(),
                        &fixture.lifetimes,
                        &slot_write,
                        || panic!("a pending Pair must fence source write claim"),
                        |_| panic!("a pending Pair must fence source write apply"),
                    ),
                    Err(PlacementError::Pending)
                );
            },
        )
        .unwrap();
    assert_eq!(current(fixture.observe(source_key)).unwrap().slot(), 36);
    assert_eq!(current(fixture.observe(target_key)).unwrap().slot(), 0);
    assert_eq!(fixture.snapshot(215).content_key(), Some(unrelated_key));
    assert_eq!(
        fixture.snapshot(215).binding_epoch(),
        unrelated.binding_epoch()
    );
    let after = fixture
        .placement
        .commit_slot_write(
            fixture.identity(),
            &fixture.lifetimes,
            &slot_write,
            || true,
            |_| {},
        )
        .unwrap();
    assert_eq!(after.lifetime(), 234);
    assert_eq!(after.cell(), unrelated.cell());
    assert_eq!(after.binding_epoch(), unrelated.binding_epoch() + 1);
    assert_eq!(
        fixture.observe(unrelated_key),
        PlacementObservation::Retired
    );
    fixture.assert_permutation();
}
