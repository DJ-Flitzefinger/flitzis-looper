//! Bounded receipt/guard protocol tests using the real permit state methods.
//! No callback, Native adoption, source ACK or audio integration is claimed here.
//! The productive existing-dependency and ABA histories are tested separately.

use super::*;

struct ReceiptOwner {
    publication: PreparedSourcePermit,
    epoch: Arc<AtomicU64>,
    root_cancelled: Arc<AtomicBool>,
    member_cancelled: Arc<AtomicBool>,
}

impl ReceiptOwner {
    fn new(expected: u64) -> Self {
        let epoch = Arc::new(AtomicU64::new(expected));
        Self {
            publication: PreparedSourcePermit::new(epoch.clone(), expected),
            epoch,
            root_cancelled: Arc::new(AtomicBool::new(false)),
            member_cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    fn guard(&self) -> ResidentLaunchGuard {
        ResidentLaunchGuard {
            publication: self.publication.resident_launch_permit().unwrap(),
            cancelled: self.root_cancelled.clone(),
            member_cancelled: Some(self.member_cancelled.clone()),
        }
    }

    fn claim_storage(&self) {
        self.publication.mark_pending().unwrap();
        assert!(self.publication.claim_resident());
        assert_eq!(self.publication.status(), "adopting");
        self.publication.mark_resident_storage_adopted();
        assert!(self.publication.resident_storage_adopted());
    }

    fn accept_transition(&self, before: u64, after: u64) {
        self.claim_storage();
        assert!(
            self.publication
                .note_resident_loop_transition(before, after)
        );
        // Use the original publication's actual transition method, after its
        // successful claim/storage/record protocol; never replace its counter
        // or construct another accepted permit to stand in for that owner.
        self.publication.mark_accepted();
    }
}

#[test]
fn transition_needs_claim_storage_record_and_final_acceptance() {
    let owner = ReceiptOwner::new(7);
    let guard = owner.guard();
    assert!(!owner.publication.note_resident_loop_transition(40, 41));
    assert!(!guard.current());
    assert!(!guard.owns_source_control_transition(40, 41));

    owner.publication.mark_pending().unwrap();
    assert!(!owner.publication.note_resident_loop_transition(40, 41));
    assert!(!guard.owns_source_control_transition(40, 41));
    assert!(owner.publication.claim_resident());
    assert!(!owner.publication.note_resident_loop_transition(40, 41));

    owner.publication.mark_resident_storage_adopted();
    assert!(owner.publication.resident_storage_adopted());
    assert!(!guard.owns_source_control_transition(40, 41));
    assert!(owner.publication.note_resident_loop_transition(40, 41));
    assert_eq!(owner.publication.status(), "adopting");
    assert!(!guard.current());
    assert!(!guard.owns_source_control_transition(40, 41));

    owner.publication.mark_accepted();
    assert!(guard.current());
    assert!(guard.owns_source_control_transition(40, 41));
    assert!(!guard.owns_source_control_transition(39, 41));
    assert!(!guard.owns_source_control_transition(40, 42));
}

#[test]
fn original_receipt_is_one_shot_shared_by_clones_and_retained_by_guard() {
    let owner = ReceiptOwner::new(11);
    let guard = owner.guard();
    let publication_clone = owner.publication.clone();
    owner.claim_storage();
    assert!(publication_clone.note_resident_loop_transition(90, 91));
    assert!(!owner.publication.note_resident_loop_transition(90, 91));
    assert!(!owner.publication.note_resident_loop_transition(91, 92));
    assert!(!guard.owns_source_control_transition(90, 91));

    owner.publication.mark_accepted();
    assert!(!publication_clone.note_resident_loop_transition(91, 92));
    assert!(guard.owns_source_control_transition(90, 91));
    assert!(!guard.owns_source_control_transition(91, 92));
    drop(publication_clone);
    drop(owner);
    // The existing scalar guard keeps that SAME original receipt and epoch
    // alive; this creates no second publication, accepted counter or registry.
    assert!(guard.owns_source_control_transition(90, 91));
}

#[test]
fn accepted_foreign_publication_cannot_borrow_another_roots_record() {
    let original = ReceiptOwner::new(13);
    let foreign = ReceiptOwner::new(29);
    let original_guard = original.guard();
    let foreign_guard = foreign.guard();
    original.accept_transition(100, 101);
    foreign.claim_storage();
    foreign.publication.mark_accepted();
    assert!(original_guard.owns_source_control_transition(100, 101));
    assert!(foreign_guard.current());
    assert!(!foreign_guard.owns_source_control_transition(100, 101));
    assert!(!foreign.publication.note_resident_loop_transition(100, 101));
    assert!(!original_guard.owns_source_control_transition(99, 101));
    assert!(!original_guard.owns_source_control_transition(100, 102));
}

#[test]
fn rejected_storage_and_unclaimed_cancellation_never_authorize_transition() {
    let rejected = ReceiptOwner::new(31);
    let rejected_guard = rejected.guard();
    rejected.claim_storage();
    assert!(rejected.publication.note_resident_loop_transition(8, 9));
    rejected.publication.mark_rejected();
    // Preserve the established storage-reconciliation observation even when
    // later mode settlement rejects. It must not become launch permission.
    assert!(rejected.publication.resident_storage_adopted());
    assert!(!rejected_guard.current());
    assert!(!rejected_guard.owns_source_control_transition(8, 9));
    assert!(!rejected.publication.note_resident_loop_transition(9, 10));

    for pending in [false, true] {
        let cancelled = ReceiptOwner::new(37);
        let guard = cancelled.guard();
        if pending {
            cancelled.publication.mark_pending().unwrap();
        }
        assert!(cancelled.publication.cancel_unclaimed());
        assert!(!cancelled.publication.claim_resident());
        assert!(!cancelled.publication.note_resident_loop_transition(8, 9));
        assert!(!cancelled.publication.resident_storage_adopted());
        assert!(!guard.current());
        assert!(!guard.owns_source_control_transition(8, 9));
    }
}

#[test]
fn preserved_window_ack_remains_current_without_loop_transition_permission() {
    let seek = ReceiptOwner::new(41);
    let seek_guard = seek.guard();
    // The established stopped-seek protocol claims directly from captured and
    // accepts its preserved window without claiming resident storage adoption.
    assert!(seek.publication.claim_resident_capture());
    assert!(!seek.publication.note_resident_loop_transition(20, 21));
    seek.publication.mark_accepted_preserved_window();
    assert!(seek_guard.current());
    assert!(!seek.publication.resident_storage_adopted());
    assert!(!seek_guard.owns_source_control_transition(20, 21));

    // Even previously recorded data cannot turn status5 into a loop ACK.
    // This deliberately exercises the receipt's rejection boundary; productive
    // root-loop and preserved-seek writers must remain distinct.
    let preserved = ReceiptOwner::new(43);
    let guard = preserved.guard();
    preserved.claim_storage();
    assert!(preserved.publication.note_resident_loop_transition(20, 21));
    preserved.publication.mark_accepted_preserved_window();
    assert!(guard.current());
    assert!(!guard.owns_source_control_transition(20, 21));
}

#[test]
fn invalid_exhausted_backwards_and_skipped_revisions_leave_no_permission() {
    for (before, after) in [
        (u64::MAX, 0),
        (0, u64::MAX),
        (u64::MAX, u64::MAX),
        (u64::MAX - 1, u64::MAX),
        (7, 6),
        (7, 9),
    ] {
        let owner = ReceiptOwner::new(47);
        let guard = owner.guard();
        owner.claim_storage();
        assert!(
            !owner
                .publication
                .note_resident_loop_transition(before, after),
            "invalid transition recorded: {before}->{after}"
        );
        // A failed note neither rejects nor accepts a real storage transaction.
        assert_eq!(owner.publication.status(), "adopting");
        assert!(owner.publication.resident_storage_adopted());
        owner.publication.mark_accepted();
        assert!(guard.current());
        assert!(
            !guard.owns_source_control_transition(before, after),
            "invalid transition granted permission: {before}->{after}"
        );
    }
    let last_valid = ReceiptOwner::new(53);
    let guard = last_valid.guard();
    last_valid.accept_transition(u64::MAX - 2, u64::MAX - 1);
    assert!(guard.owns_source_control_transition(u64::MAX - 2, u64::MAX - 1));
    assert!(!guard.owns_source_control_transition(u64::MAX, u64::MAX - 1));
    assert!(!guard.owns_source_control_transition(u64::MAX - 2, u64::MAX));
}

#[test]
fn unchanged_loop_commit_preserves_exact_revision_without_authorizing_drift() {
    let owner = ReceiptOwner::new(59);
    let guard = owner.guard();
    owner.accept_transition(70, 70);
    assert!(guard.owns_source_control_transition(70, 70));
    assert!(!guard.owns_source_control_transition(69, 70));
    assert!(!guard.owns_source_control_transition(70, 71));
    assert!(!owner.publication.note_resident_loop_transition(70, 71));
}

#[test]
fn publication_and_source_epoch_changes_revoke_recording_and_accepted_permission() {
    for change_source in [false, true] {
        let mut pending = ReceiptOwner::new(61);
        let source_epoch = Arc::new(AtomicU64::new(67));
        pending.publication = pending
            .publication
            .with_source_epoch(source_epoch.clone(), 67);
        let pending_guard = pending.guard();
        pending.claim_storage();
        if change_source {
            source_epoch.store(68, Ordering::Release);
        } else {
            pending.epoch.store(62, Ordering::Release);
        }
        assert!(!pending.publication.note_resident_loop_transition(3, 4));
        assert_eq!(pending.publication.status(), "adopting");
        assert!(!pending_guard.current());
        assert!(!pending_guard.owns_source_control_transition(3, 4));

        let mut accepted = ReceiptOwner::new(71);
        let accepted_source = Arc::new(AtomicU64::new(73));
        accepted.publication = accepted
            .publication
            .with_source_epoch(accepted_source.clone(), 73);
        let accepted_guard = accepted.guard();
        accepted.accept_transition(3, 4);
        assert!(accepted_guard.owns_source_control_transition(3, 4));
        if change_source {
            accepted_source.store(74, Ordering::Release);
        } else {
            accepted.epoch.store(72, Ordering::Release);
        }
        assert_eq!(accepted.publication.status(), "accepted");
        assert!(!accepted_guard.current());
        assert!(!accepted_guard.owns_source_control_transition(3, 4));
    }
}

#[test]
fn cancellation_is_member_scoped_and_root_cancel_revokes_all_members() {
    let first = ReceiptOwner::new(79);
    let first_guard = first.guard();
    let second_cancelled = Arc::new(AtomicBool::new(false));
    let second_guard = ResidentLaunchGuard {
        publication: first.publication.resident_launch_permit().unwrap(),
        cancelled: first.root_cancelled.clone(),
        member_cancelled: Some(second_cancelled.clone()),
    };
    let root_guard = ResidentLaunchGuard {
        publication: first.publication.resident_launch_permit().unwrap(),
        cancelled: first.root_cancelled.clone(),
        member_cancelled: None,
    };
    first.accept_transition(200, 201);
    for guard in [&first_guard, &second_guard, &root_guard] {
        assert!(guard.owns_source_control_transition(200, 201));
    }
    first.member_cancelled.store(true, Ordering::Release);
    assert!(!first_guard.owns_source_control_transition(200, 201));
    assert!(second_guard.owns_source_control_transition(200, 201));
    assert!(root_guard.owns_source_control_transition(200, 201));
    second_cancelled.store(true, Ordering::Release);
    assert!(!second_guard.owns_source_control_transition(200, 201));
    assert!(root_guard.owns_source_control_transition(200, 201));
    assert_eq!(first.publication.status(), "accepted");

    first.root_cancelled.store(true, Ordering::Release);
    for guard in [&first_guard, &second_guard, &root_guard] {
        assert!(!guard.current());
        assert!(!guard.owns_source_control_transition(200, 201));
    }
    assert_eq!(first.publication.status(), "accepted");
    assert!(first.publication.resident_storage_adopted());
}
