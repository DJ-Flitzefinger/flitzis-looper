"""Caller ownership survives actual reader handoff without keeping inference jobs alive."""

from dataclasses import replace
from pathlib import Path
from threading import Event, Thread
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.controller.stem_job import (
    DIRECT_STEM_INTEREST,
    StemGenerationInterest,
    StemGenerationInterests,
    StemGenerationJob,
)
from flitzis_looper.controller.stem_pair_preparation import (
    StemPairIntentBinding,
    StemPairPreparationQueue,
)
from flitzis_looper.models import StemCacheEntry
from tests.flitzis_looper.controller.test_stem_job import (
    _Assets,
    _job,
    _Lease,
    _Reservation,
    _resolve,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
    from flitzis_looper_audio import (
        AudioEngine,
        OfflineStemSourceLease,
        PreparedSourceTicket,
        PreparedStemPair,
    )


class _PairAssets(_Assets):
    def reserve_pending(self, count: int) -> _Reservation:
        return self.reserve(count)


def _pair_queue(
    assets: _PairAssets,
) -> tuple[StemPairPreparationQueue, list[Callable[[], None]], Mock]:
    jobs: list[Callable[[], None]] = []
    audio = Mock(name="ordinary-pair-reader")
    queue = StemPairPreparationQueue(
        cast("AudioEngine", audio), cast("ProjectAssetLifecycle", assets), jobs.append
    )
    return queue, jobs, audio


def _submit(
    queue: StemPairPreparationQueue, owner: StemGenerationInterests, sample_id: int = 0
) -> None:
    cache_dir = f"ready-pair-{sample_id}"
    entry = StemCacheEntry(
        source_version=f"source-{sample_id}.wav|sha256-v1:{'1' * 64}",
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
    )
    queue.submit(
        sample_id,
        entry,
        cast("PreparedSourceTicket", Mock(name=f"own-source-ticket-{sample_id}")),
        f"own-content-{sample_id}",
        None,
        components=False,
        binding=StemPairIntentBinding(interest_owner=owner),
    )


def test_successful_job_disposal_preserves_callers_until_its_pending_pair_cancels() -> None:
    assets = _PairAssets()
    job = _job(assets)
    original = job.subscribers()[0]
    owner = original.interest_owner
    batch = StemGenerationInterest("pending-pair-batch")
    assert job.add_interest(original, batch)
    snapshot = owner.current
    _resolve(job)
    assert job.begin() is job.source
    assert job.finish_read()
    queue, jobs, audio = _pair_queue(assets)
    _submit(queue, owner)
    preparation = queue.pending[0]

    job.dispose()

    assert job.phase == "disposed"
    assert job.subscribers() == ()
    assert original.interest_owner is preparation.interest_owner is owner
    assert owner.current is snapshot
    assert cast("_Lease", original.source_lease).released
    assert not cast("_Lease", preparation.lease).released
    assert not cast("_Reservation", preparation.retirement).closed
    assert owner.detach(batch) == "remaining"
    assert owner.detach(DIRECT_STEM_INTEREST) == "detached"
    queue.cancel_interest_owner(owner)
    assert preparation.cancelled.is_set()
    assert not cast("_Lease", preparation.lease).released
    jobs[0]()
    assert queue.collect() == (preparation,)
    audio.prepare_stem_pair.assert_not_called()
    assert cast("_Lease", preparation.lease).released
    assert cast("_Reservation", preparation.retirement).closed


def test_final_caller_cancel_keeps_pair_reader_alive_until_actual_worker_return() -> None:
    assets = _PairAssets()
    batch = StemGenerationInterest("running-pair-batch")
    owner = StemGenerationInterests(frozenset({batch}))
    queue, jobs, audio = _pair_queue(assets)
    entered = Event()
    allow_return = Event()
    prepared = cast("PreparedStemPair", Mock(name="returned-owned-pair"))

    def prepare(*_args: object) -> PreparedStemPair:
        entered.set()
        assert allow_return.wait(timeout=5), "Pair reader did not receive its bounded release"
        return prepared

    audio.prepare_stem_pair.side_effect = prepare
    _submit(queue, owner)
    preparation = queue.pending[0]
    worker = Thread(target=jobs[0])
    worker.start()
    try:
        assert entered.wait(timeout=5), "Pair reader did not enter"
        assert owner.detach(batch) == "detached"
        queue.cancel_interest_owner(owner)
        assert preparation.cancelled.is_set()
        assert queue.pending[0] is preparation
        assert queue.collect() == ()
        assert not cast("_Lease", preparation.lease).released
        assert not cast("_Reservation", preparation.retirement).closed
    finally:
        allow_return.set()
        worker.join(timeout=5)

    assert not worker.is_alive()
    assert preparation.prepared is prepared
    assert preparation.error == "Stem pair preparation cancelled"
    assert cast("_Lease", preparation.lease).released
    assert cast("_Reservation", preparation.retirement).closed
    assert queue.collect() == (preparation,)


@pytest.mark.parametrize("operation", ["cancel", "source-removal", "bank-detach"])
def test_whole_content_removal_invalidates_all_callers_before_releasing_custody(
    operation: str,
) -> None:
    assets = _Assets()
    job = _job(assets)
    original = job.subscribers()[0]
    owner = original.interest_owner
    assert job.add_interest(original, StemGenerationInterest("whole-content-batch"))
    snapshot = owner.current
    assert job.begin() is job.source

    if operation == "cancel":
        job.cancel()
    elif operation == "source-removal":
        assert not job.remove_subscriber(original.sample_id)
    else:
        plan = job.prepare_subscriber_detach(((original.sample_id, original.content_id),))
        with job._lock:
            job._subscribers = plan.merged_locked()
        assert plan.settle()

    assert len(snapshot) == 2
    assert owner.current == frozenset()
    assert not owner.committed
    assert job.cancel_event.is_set()
    assert job.running
    assert not assets.leases[0].released
    assert not assets.leases[1].released
    assert cast("_Lease", original.source_lease).released
    assert not job.finish_read()
    assert all(lease.released for lease in assets.leases)


def test_whole_pair_cancellation_invalidates_all_callers_after_job_disposal() -> None:
    assets = _PairAssets()
    job = _job(assets)
    original = job.subscribers()[0]
    assert job.add_interest(original, StemGenerationInterest("disposed-job-batch"))
    queue, jobs, audio = _pair_queue(assets)
    _submit(queue, original.interest_owner)
    preparation = queue.pending[0]
    job.dispose()

    queue.cancel(0)

    assert original.interest_owner.current == frozenset()
    assert preparation.cancelled.is_set()
    assert not cast("_Lease", preparation.lease).released
    jobs[0]()
    assert queue.collect() == (preparation,)
    audio.prepare_stem_pair.assert_not_called()
    assert cast("_Lease", preparation.lease).released


def test_restarted_generation_reuses_all_original_callers_without_implicit_direct() -> None:
    assets = _Assets()
    baseline = _job(assets)
    request = replace(baseline.request, cache_dir=Path("retry-private-generation"))
    baseline.cancel()
    batch = StemGenerationInterest("first-batch")
    other_batch = StemGenerationInterest("second-batch")
    owner = StemGenerationInterests(frozenset({batch, other_batch}))
    job = StemGenerationJob(
        request,
        cast("OfflineStemSourceLease", Mock(name="fresh-retry-offline-source")),
        cast("ProjectAssetLifecycle", assets),
        "original-retry-content",
        interest_owner=owner,
    )
    subscriber = job.subscribers()[0]

    assert subscriber.interest_owner is owner
    assert subscriber.interests == frozenset({batch, other_batch})
    assert DIRECT_STEM_INTEREST not in subscriber.interests
    assert job.remove_interest(subscriber, batch) == "remaining"
    assert job.subscribers() == (subscriber,)
    assert job.remove_interest(subscriber, other_batch) == "detached"
    assert job.cancel_event.is_set()
    assert job.phase == "disposed"


def test_explicit_subscriber_holder_preserves_tokens_and_rejects_empty_admission() -> None:
    assets = _Assets()
    job = _job(assets)
    batch = StemGenerationInterest("joined-batch")
    owner = StemGenerationInterests(frozenset({batch}))
    job.add_subscriber(
        215,
        cast("OfflineStemSourceLease", Mock(name="joined-source")),
        "joined-content",
        interest_owner=owner,
    )
    joined = job.subscribers()[1]
    assert joined.interest_owner is owner
    assert joined.interests == frozenset({batch})
    empty = StemGenerationInterests(frozenset())
    leases_before = tuple(assets.leases)
    reservations_before = tuple(assets.reservations)

    with pytest.raises(RuntimeError, match="nonempty"):
        job.add_subscriber(36, joined.source, "rejected-content", interest_owner=empty)
    with pytest.raises(RuntimeError, match="nonempty"):
        StemGenerationJob(
            replace(job.request, cache_dir=Path("rejected-generation")),
            joined.source,
            cast("ProjectAssetLifecycle", assets),
            "rejected-content",
            interest_owner=empty,
        )

    assert tuple(assets.leases) == leases_before
    assert tuple(assets.reservations) == reservations_before
    assert job.subscribers()[1] is joined
    job.cancel()


def test_final_owner_cancellation_follows_relocated_pair_without_touching_replacement() -> None:
    assets = _PairAssets()
    batch = StemGenerationInterest("relocated-original")
    owner = StemGenerationInterests(frozenset({batch}))
    replacement_owner = StemGenerationInterests()
    queue, jobs, audio = _pair_queue(assets)
    _submit(queue, owner)
    original = queue.pending.pop(0)
    queue.pending[215] = original
    _submit(queue, replacement_owner)
    replacement = queue.pending[0]
    replacement_snapshot = replacement_owner.current

    assert owner.detach(batch) == "detached"
    queue.cancel_interest_owner(owner)

    assert original.sample_id == 0  # The active worker keeps its historical capture ID.
    assert original.cancelled.is_set()
    assert queue.pending[215] is original
    assert not replacement.cancelled.is_set()
    assert replacement_owner.current is replacement_snapshot
    assert not cast("_Lease", original.lease).released
    jobs[0]()
    jobs[1]()
    assert queue.collect() == (original, replacement)
    assert audio.prepare_stem_pair.call_count == 1
    # The controller projects a returned worker's moved delivery key before settlement.
    assert queue.pending.pop(215) is original
    replacement.close()


def test_committed_result_survives_precommit_failure_and_all_later_token_cancellation() -> None:
    batch = StemGenerationInterest("committed-batch")
    owner = StemGenerationInterests(frozenset({batch}))
    snapshot = owner.current
    owner.mark_failed("first precommit failure")
    owner.mark_failed("later precommit failure")
    assert owner.failure == "first precommit failure"
    assert not owner.committed

    owner.mark_committed()
    assert owner.detach(batch) == "detached"
    owner.invalidate_all()
    owner.mark_failed("later settlement or adoption error")

    assert owner.committed
    assert owner.failure is None
    assert owner.current == frozenset()
    assert snapshot == frozenset({batch})
    assert owner.detach(batch) == "absent"
