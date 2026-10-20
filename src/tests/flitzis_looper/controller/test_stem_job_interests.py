"""Independent callers share one PAD publication row, never Native publication authority."""

from dataclasses import FrozenInstanceError, fields, replace
from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import stem_job
from tests.flitzis_looper.controller.test_pair_stem_mirror import (
    _add_job,
    _controller,
    _prepare,
    _project_after,
)
from tests.flitzis_looper.controller.test_stem_job import (
    _Assets,
    _job,
    _Lease,
    _Reservation,
    _resolve,
)

if TYPE_CHECKING:
    from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
    from flitzis_looper_audio import OfflineStemSourceLease


def _has_interest(
    job: stem_job.StemGenerationJob,
    expected: stem_job.StemSubscriber,
    interest: stem_job.StemGenerationInterest,
) -> bool:
    return any(
        row.same_interest_owner(expected) and interest in row.interests for row in job.subscribers()
    )


def test_interest_tokens_use_identity_even_when_owner_labels_are_equal() -> None:
    first = stem_job.StemGenerationInterest("same batch")
    second = stem_job.StemGenerationInterest("same batch")

    assert first != second
    assert len({first, second}) == 2
    assert first.owner == second.owner == "same batch"
    assert stem_job.StemGenerationInterest().owner is None
    for field in fields(first):
        with pytest.raises(FrozenInstanceError):
            setattr(first, field.name, "changed")


def test_legacy_callers_keep_direct_interest_without_explicit_enrollment() -> None:
    assets = _Assets()
    job = _job(assets)
    job.add_subscriber(
        215,
        cast("OfflineStemSourceLease", Mock(name="other-pad-source")),
        "other-pad-content",
    )

    assert all(
        row.interests == frozenset({stem_job.DIRECT_STEM_INTEREST}) for row in job.subscribers()
    )
    job.cancel()


def test_explicit_batch_creation_never_creates_an_implicit_direct_interest() -> None:
    assets = _Assets()
    baseline = _job(assets)
    batch = stem_job.StemGenerationInterest("batch-only")
    request = replace(baseline.request, sample_id=215, cache_dir=Path("batch-generation"))
    source = cast("OfflineStemSourceLease", Mock(name="batch-only-source"))
    job = stem_job.StemGenerationJob(
        request,
        source,
        cast("ProjectAssetLifecycle", assets),
        "batch-only-content",
        interest=batch,
    )
    job.add_subscriber(
        36,
        cast("OfflineStemSourceLease", Mock(name="second-batch-only-source")),
        "second-batch-only-content",
        interest=batch,
    )

    assert all(row.interests == frozenset({batch}) for row in job.subscribers())
    assert all(
        not _has_interest(job, row, stem_job.DIRECT_STEM_INTEREST) for row in job.subscribers()
    )
    job.cancel()
    baseline.cancel()


def test_direct_and_two_batch_callers_detach_independently_without_new_publication_rows() -> None:
    assets = _Assets()
    job = _job(assets)
    row = job.subscribers()[0]
    direct_snapshot = row.interests
    first = stem_job.StemGenerationInterest("batch-one")
    second = stem_job.StemGenerationInterest("batch-two")
    leases = tuple(assets.leases)
    reservations = tuple(assets.reservations)

    assert job.add_interest(row, first)
    assert job.add_interest(row, second)
    assert job.subscribers() == (row,)
    assert job.subscribers()[0] is row
    assert direct_snapshot == frozenset({stem_job.DIRECT_STEM_INTEREST})
    assert row.interests == frozenset({stem_job.DIRECT_STEM_INTEREST, first, second})
    assert job.remove_interest(row, first) == "remaining"
    assert job.remove_interest(row, stem_job.DIRECT_STEM_INTEREST) == "remaining"
    assert row.interests == frozenset({second})
    assert _has_interest(job, row, second)
    assert not job.cancel_event.is_set()
    assert tuple(assets.leases) == leases
    assert tuple(assets.reservations) == reservations
    assert not any(lease.released for lease in leases)
    assert not any(reservation.closed for reservation in reservations)

    assert job.remove_interest(row, second) == "detached"
    assert not job.subscribers()
    assert job.cancel_event.is_set()
    assert all(lease.released for lease in leases)
    assert all(reservation.closed for reservation in reservations)


def test_duplicate_join_and_unknown_removal_leave_exact_row_and_membership_snapshot_unchanged() -> (
    None
):
    assets = _Assets()
    job = _job(assets)
    row = job.subscribers()[0]
    batch = stem_job.StemGenerationInterest("same-label")
    unrelated = stem_job.StemGenerationInterest("same-label")
    assert job.add_interest(row, batch)
    snapshot = row.interests

    assert not job.add_interest(row, batch)
    assert job.remove_interest(row, unrelated) == "absent"
    assert not _has_interest(job, row, unrelated)
    assert row.interests is snapshot
    assert job.subscribers()[0] is row
    assert not job.cancel_event.is_set()
    assert not any(lease.released for lease in assets.leases)
    assert not any(reservation.closed for reservation in assets.reservations)
    job.cancel()


def test_final_queued_interest_disposes_without_starting_any_backend_read() -> None:
    assets = _Assets()
    job = _job(assets)
    row = job.subscribers()[0]

    assert job.remove_interest(row, stem_job.DIRECT_STEM_INTEREST) == "detached"

    assert job.phase == "disposed"
    assert not job.running
    assert job.cancel_event.is_set()
    assert not _has_interest(job, row, stem_job.DIRECT_STEM_INTEREST)
    assert job.begin_resolution() is None
    assert job.begin() is None
    assert assets.retired == [job.request.cache_dir]
    assert all(lease.released for lease in assets.leases)
    assert all(reservation.closed for reservation in assets.reservations)
    assert job.remove_interest(row, stem_job.DIRECT_STEM_INTEREST) == "absent"


@pytest.mark.parametrize("phase", ["resolving", "running"])
def test_last_interest_keeps_actual_backend_lease_until_its_read_returns(phase: str) -> None:
    assets = _Assets()
    job = _job(assets)
    row = job.subscribers()[0]
    physical_source, generation_lease = assets.leases[:2]
    source = job.source
    assert source is not None
    if phase == "resolving":
        assert job.begin_resolution() is source
    else:
        _resolve(job)
        assert job.begin() is source
    guard = job.processing_files

    assert job.remove_interest(row, stem_job.DIRECT_STEM_INTEREST) == "detached"

    assert job.running
    assert job.cancel_event.is_set()
    assert job.source is source
    assert job.processing_files is guard
    assert not physical_source.released
    assert not generation_lease.released
    assert cast("_Lease", row.source_lease).released
    assert cast("_Reservation", row.retirement).closed
    assert not assets.retired
    if phase == "resolving":
        assert not job.finish_resolution()
    else:
        assert not job.finish_read()
    assert job.phase == "disposed"
    assert job.source is None
    assert job.processing_files is None
    assert all(lease.released for lease in assets.leases)
    assert all(reservation.closed for reservation in assets.reservations)


def test_last_token_of_one_pad_keeps_other_pad_and_shared_physical_work_alive() -> None:
    assets = _Assets()
    job = _job(assets)
    removed = job.subscribers()[0]
    job.add_subscriber(
        215,
        cast("OfflineStemSourceLease", Mock(name="independent-surviving-pad")),
        "surviving-content",
    )
    survivor = job.subscribers()[1]
    assert job.begin() is job.source

    assert job.remove_interest(removed, stem_job.DIRECT_STEM_INTEREST) == "detached"

    assert job.subscribers() == (survivor,)
    assert job.subscribers()[0] is survivor
    assert _has_interest(job, survivor, stem_job.DIRECT_STEM_INTEREST)
    assert not job.cancel_event.is_set()
    assert not assets.leases[0].released
    assert cast("_Lease", removed.source_lease).released
    assert not cast("_Lease", survivor.source_lease).released
    assert job.finish_read()
    job.cancel()


def test_intent_update_preserves_cancellation_owner_but_rejects_stale_join() -> None:
    assets = _Assets()
    job = _job(assets)
    before = job.subscribers()[0]
    batch = stem_job.StemGenerationInterest("original-intent-batch")
    assert job.add_interest(before, batch)
    assert before.intent is not None
    updated = replace(before.intent, material_revision=before.intent.material_revision + 1)

    job.update_intent(0, updated)

    current = job.subscribers()[0]
    assert current is not before
    assert current.same_interest_owner(before)
    assert current.interests is before.interests
    assert current.source is before.source
    assert current.request is before.request
    assert current.intent is updated
    with pytest.raises(RuntimeError):
        job.add_interest(before, stem_job.StemGenerationInterest("stale-join"))
    assert _has_interest(job, before, batch)
    assert job.remove_interest(before, batch) == "remaining"
    assert job.subscribers()[0] is current
    assert current.interests == frozenset({stem_job.DIRECT_STEM_INTEREST})
    job.cancel()


@pytest.mark.parametrize("terminal", ["cancelled", "disposed", "settled"])
def test_new_joins_reject_terminal_jobs_even_with_an_original_row(terminal: str) -> None:
    assets = _Assets()
    job = _job(assets)
    row = job.subscribers()[0]
    if terminal == "cancelled":
        assert job.begin() is job.source
        job.cancel()
    elif terminal == "disposed":
        job.dispose()
    else:
        assert job.begin() is job.source
        assert job.finish_read()
    batch = stem_job.StemGenerationInterest("late-terminal")
    snapshot = row.interests

    with pytest.raises(RuntimeError):
        job.add_interest(row, batch)
    with pytest.raises(RuntimeError):
        job.add_interest(row, stem_job.DIRECT_STEM_INTEREST)

    assert row.interests is snapshot
    assert not _has_interest(job, row, batch)
    if terminal == "cancelled":
        assert not job.finish_read()
    job.cancel()


def test_reused_pad_with_identical_source_and_content_does_not_inherit_old_holder() -> None:
    assets = _Assets()
    job = _job(assets)
    old = job.subscribers()[0]
    batch = stem_job.StemGenerationInterest("old-item")
    assert job.add_interest(old, batch)
    job.add_subscriber(
        215,
        cast("OfflineStemSourceLease", Mock(name="keep-job-alive")),
        "other-content",
    )
    assert job.remove_subscriber(0)
    job.add_subscriber(0, old.source, old.content_id, request=old.request, intent=old.intent)
    replacement = next(row for row in job.subscribers() if row.sample_id == 0)
    snapshot = replacement.interests

    assert not replacement.same_interest_owner(old)
    assert not _has_interest(job, old, batch)
    assert not _has_interest(job, old, stem_job.DIRECT_STEM_INTEREST)
    assert job.remove_interest(old, batch) == "absent"
    assert job.remove_interest(old, stem_job.DIRECT_STEM_INTEREST) == "absent"
    with pytest.raises(RuntimeError):
        job.add_interest(old, batch)
    assert replacement.interests is snapshot
    assert _has_interest(job, replacement, stem_job.DIRECT_STEM_INTEREST)
    assert not cast("_Lease", replacement.source_lease).released
    assert not job.cancel_event.is_set()
    job.cancel()


def test_add_subscriber_cannot_join_second_caller_through_a_duplicate_pad_row() -> None:
    assets = _Assets()
    job = _job(assets)
    row = job.subscribers()[0]
    batch = stem_job.StemGenerationInterest("must-use-add-interest")

    with pytest.raises(RuntimeError, match="already registered"):
        job.add_subscriber(0, row.source, row.content_id, interest=batch)

    assert job.subscribers() == (row,)
    assert row.interests == frozenset({stem_job.DIRECT_STEM_INTEREST})
    assert not _has_interest(job, row, batch)
    assert not cast("_Lease", row.source_lease).released
    assert assets.leases[-1].released
    assert assets.reservations[-1].closed
    job.cancel()


@pytest.mark.parametrize("occupied", [False, True])
def test_prepared_pair_keeps_latest_tokens_and_original_cancellation_owner_after_move_or_swap(
    *, occupied: bool
) -> None:
    controller, assets, _audio = _controller(occupied=occupied)
    job = _add_job(controller, assets)
    target_job = _add_job(controller, assets, 215) if occupied else None
    original = job.subscribers()[0]
    plan, acknowledgement = _prepare(controller)
    batch = stem_job.StemGenerationInterest("joined-after-pair-preparation")
    assert job.add_interest(original, batch)
    assert job.remove_interest(original, stem_job.DIRECT_STEM_INTEREST) == "remaining"
    assert job.subscribers()[0] is original

    _project_after(controller, plan)
    plan.apply(acknowledgement)

    moved = job.subscribers()[0]
    assert moved.sample_id == 215
    assert moved.same_interest_owner(original)
    assert moved.interests == frozenset({batch})
    assert moved.source is original.source
    assert moved.request is original.request
    assert moved.intent is original.intent
    assert moved.source_lease is original.source_lease
    assert moved.retirement is original.retirement
    assert _has_interest(job, original, batch)
    with pytest.raises(RuntimeError):
        job.add_interest(original, stem_job.StemGenerationInterest("stale-old-placement"))
    assert job.remove_interest(original, batch) == "detached"
    assert job.cancel_event.is_set()
    if target_job is not None:
        target = target_job.subscribers()[0]
        assert target.sample_id == 0
        assert target.interests == frozenset({stem_job.DIRECT_STEM_INTEREST})
        assert not target_job.cancel_event.is_set()
        target_job.cancel()


def test_shared_pair_swap_routes_each_original_holder_to_its_current_pad() -> None:
    assets = _Assets()
    job = _job(assets)
    source = job.subscribers()[0]
    job.add_subscriber(
        215,
        cast("OfflineStemSourceLease", Mock(name="independent-target-source")),
        "target-content",
    )
    target = job.subscribers()[1]
    source_batch = stem_job.StemGenerationInterest("source-batch")
    target_batch = stem_job.StemGenerationInterest("target-batch")
    assert source.content_id is not None
    plan = job.prepare_pair_subscribers(0, 215, source.content_id, target.content_id)
    assert job.add_interest(source, source_batch)
    assert job.add_interest(target, target_batch)
    with job._lock:
        job._subscribers = plan.merged_locked()

    moved = {row.sample_id: row for row in job.subscribers()}
    assert moved[215].same_interest_owner(source)
    assert moved[0].same_interest_owner(target)
    assert job.remove_interest(source, stem_job.DIRECT_STEM_INTEREST) == "remaining"
    assert job.remove_interest(source, source_batch) == "detached"
    assert job.subscribers() == (moved[0],)
    assert _has_interest(job, target, target_batch)
    assert _has_interest(job, target, stem_job.DIRECT_STEM_INTEREST)
    assert not job.cancel_event.is_set()
    job.cancel()


def test_canonical_transfer_keeps_complete_holder_and_own_source_request_intent() -> None:
    assets = _Assets()
    canonical = _job(assets)
    candidate = _job(assets, 215)
    original = candidate.subscribers()[0]
    batch = stem_job.StemGenerationInterest("candidate-batch")
    assert candidate.add_interest(original, batch)
    _resolve(canonical)
    _resolve(candidate)
    assert canonical.begin() is canonical.source

    assert candidate.transfer_subscribers_to(canonical) == (original,)

    transferred = canonical.subscribers()[1]
    assert transferred is original
    assert transferred.interests == frozenset({stem_job.DIRECT_STEM_INTEREST, batch})
    assert transferred.request is candidate.request
    assert transferred.source is original.source
    assert transferred.intent is original.intent
    assert transferred.source_lease is original.source_lease
    assert _has_interest(canonical, original, batch)
    assert not _has_interest(candidate, original, batch)
    assert candidate.remove_interest(original, batch) == "absent"
    assert canonical.remove_interest(original, batch) == "remaining"
    assert canonical.subscribers()[1] is original
    assert _has_interest(canonical, original, stem_job.DIRECT_STEM_INTEREST)
    assert not cast("_Lease", original.source_lease).released
    assert canonical.finish_read()
    canonical.cancel()


def test_prepared_bank_detach_removes_all_current_tokens_without_staling_exact_row_plan() -> None:
    assets = _Assets()
    job = _job(assets)
    removed = job.subscribers()[0]
    job.add_subscriber(
        36,
        cast("OfflineStemSourceLease", Mock(name="unaffected-bank-source")),
        "unaffected-bank-content",
    )
    survivor = job.subscribers()[1]
    plan = job.prepare_subscriber_detach(((0, removed.content_id),))
    batch = stem_job.StemGenerationInterest("joined-after-bank-preparation")
    assert job.add_interest(removed, batch)
    assert job.remove_interest(removed, stem_job.DIRECT_STEM_INTEREST) == "remaining"
    assert job.subscribers()[0] is removed
    assert job.begin() is job.source
    with job._lock:
        job._subscribers = plan.merged_locked()

    assert plan.settle()
    assert plan.settle()

    assert job.subscribers() == (survivor,)
    assert job.subscribers()[0] is survivor
    assert not _has_interest(job, removed, batch)
    assert job.remove_interest(removed, batch) == "absent"
    assert _has_interest(job, survivor, stem_job.DIRECT_STEM_INTEREST)
    assert cast("_Lease", removed.source_lease).released
    assert not cast("_Lease", survivor.source_lease).released
    assert not assets.leases[0].released
    assert not job.cancel_event.is_set()
    assert job.finish_read()
    job.cancel()


def test_bank_detach_of_last_multi_interest_row_retains_actual_running_reader() -> None:
    assets = _Assets()
    job = _job(assets)
    removed = job.subscribers()[0]
    assert job.add_interest(removed, stem_job.StemGenerationInterest("last-bank-batch"))
    plan = job.prepare_subscriber_detach(((0, removed.content_id),))
    source = job.source
    assert source is not None
    assert job.begin() is source
    with job._lock:
        job._subscribers = plan.merged_locked()

    assert plan.settle()

    assert job.cancel_event.is_set()
    assert job.running
    assert job.source is source
    assert not assets.leases[0].released
    assert not assets.leases[1].released
    assert cast("_Lease", removed.source_lease).released
    assert not job.finish_read()
    assert job.phase == "disposed"
    assert all(lease.released for lease in assets.leases)
    assert all(reservation.closed for reservation in assets.reservations)
