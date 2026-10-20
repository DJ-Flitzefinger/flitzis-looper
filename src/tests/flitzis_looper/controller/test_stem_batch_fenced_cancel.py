"""Actual cancellation/mirror guards with declared audio and opaque ACK substitutes.

These cases prove Python ownership and reader settlement only. The existing opaque
test receipts do not establish Native publication, callback, device or hearing acceptance.
"""

from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import stems
from flitzis_looper.controller.stem_job import (
    DIRECT_STEM_INTEREST,
    StemGenerationInterest,
    StemGenerationInterests,
)
from tests.flitzis_looper.controller.test_bank_arrangement_mirror import _harness
from tests.flitzis_looper.controller.test_pair_stem_mirror import (
    _add_job,
    _controller,
    _pending_preparation,
    _prepare,
    _project_after,
)

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller.pair_stem_mirror import PreparedPairStemMirror
    from flitzis_looper.controller.stem_job import StemGenerationJob, StemSubscriber
    from flitzis_looper.controller.stems import StemController, StemGenerationCancellation
    from flitzis_looper_audio import OfflineStemSourceLease, PreparedStemPair
    from tests.flitzis_looper.controller.test_stem_job import _Lease, _Reservation


def _batch_owner(
    job: StemGenerationJob, *, direct: bool = False
) -> tuple[StemGenerationInterests, StemGenerationInterest]:
    """Use the same stable owner and detach API reached by the global batch route."""
    row = job.subscribers()[0]
    token = StemGenerationInterest("original-confirmed-batch-row")
    assert job.add_interest(row, token)
    if not direct:
        assert row.interest_owner.detach(DIRECT_STEM_INTEREST) == "remaining"
    return row.interest_owner, token


@dataclass
class _PairCancellation:
    controller: StemController
    job: StemGenerationJob
    original: StemSubscriber
    owner: StemGenerationInterests
    token: StemGenerationInterest
    target_job: StemGenerationJob | None
    target: StemSubscriber | None
    physical: _Lease
    plan: PreparedPairStemMirror
    acknowledgement: object
    images: tuple[object, ...]


def _pair_cancellation(*, occupied: bool, running: bool) -> _PairCancellation:
    controller, assets, _audio = _controller(occupied=occupied)
    job = _add_job(controller, assets)
    original = job.subscribers()[0]
    owner, token = _batch_owner(job)
    target_job = _add_job(controller, assets, 215) if occupied else None
    target = target_job.subscribers()[0] if target_job is not None else None
    physical = assets.leases[0]
    if running:
        assert job.begin() is original.source
    controller._pair_deferred_sample_ids = frozenset({0, 215})
    plan, acknowledgement = _prepare(controller)
    return _PairCancellation(
        controller,
        job,
        original,
        owner,
        token,
        target_job,
        target,
        physical,
        plan,
        acknowledgement,
        (
            controller._generation_sources,
            controller._generation_intents,
            controller._session.stem_generation_progress,
            job._subscribers,
        ),
    )


def _assert_frozen_job_preserved(
    case: _PairCancellation, cancellation: StemGenerationCancellation
) -> None:
    assert cancellation.disposition == "detached"
    assert not case.owner.current
    assert not case.owner.committed
    assert cancellation.deferred_jobs == (case.job,)
    assert case.controller.generation_cancellation_is_pending(cancellation)
    assert case.job._subscribers is case.images[3]
    assert case.job.subscribers() == (case.original,)
    assert case.controller._generation_sources is case.images[0]
    assert case.controller._generation_intents is case.images[1]
    assert case.controller._session.stem_generation_progress is case.images[2]
    assert case.controller._generation_sources[0] is case.original.source
    assert not case.job.cancel_event.is_set()
    assert not case.physical.released
    assert not cast("_Lease", case.original.source_lease).released
    assert not cast("_Reservation", case.original.retirement).closed


def _apply_frozen_pair(case: _PairCancellation, cancellation: StemGenerationCancellation) -> None:
    # The real exact-row/opaque-receipt guards remain active across cancellation.
    with pytest.raises(RuntimeError, match=r"acknowledgement|receipt"):
        case.plan.apply(object())
    _project_after(case.controller, case.plan)
    case.plan.apply(case.acknowledgement)
    moved = case.job.subscribers()[0]
    assert case.plan.applied
    assert moved.sample_id == 215
    assert moved.interest_owner is case.owner
    assert moved.source is case.original.source
    assert moved.request is case.original.request
    assert case.controller._generation_sources[215] is case.original.source
    assert case.controller.generation_cancellation_is_pending(cancellation)
    case.controller._expire_generation_interests()
    assert case.job.subscribers() == (moved,)
    assert not case.job.cancel_event.is_set()


def _assert_last_row_cancelled(
    case: _PairCancellation, cancellation: StemGenerationCancellation, *, running: bool
) -> None:
    assert not case.job.subscribers()
    assert 215 not in case.controller._generation_sources
    assert 215 not in case.controller._generation_intents
    assert 215 not in case.controller._session.stem_generation_progress
    assert case.job.cancel_event.is_set()
    assert cast("_Lease", case.original.source_lease).released
    assert cast("_Reservation", case.original.retirement).closed
    if case.target_job is not None:
        assert case.target is not None
        survivor = case.target_job.subscribers()[0]
        assert survivor.sample_id == 0
        assert survivor.interest_owner is case.target.interest_owner
        assert case.controller._generation_sources[0] is case.target.source
        assert not case.target_job.cancel_event.is_set()
    else:
        assert 0 not in case.controller._generation_sources
    assert case.controller.generation_cancellation_is_pending(cancellation) is running
    if running:
        assert not case.physical.released
        assert not case.job.finish_read()
    assert case.physical.released
    assert not case.controller.generation_cancellation_is_pending(cancellation)


@pytest.mark.parametrize("occupied", [False, True], ids=["move", "swap"])
@pytest.mark.parametrize("running", [False, True], ids=["unstarted", "reader-entered"])
def test_final_batch_cancel_keeps_exact_pair_participants_until_moved_fence_clears(
    *, occupied: bool, running: bool
) -> None:
    case = _pair_cancellation(occupied=occupied, running=running)
    try:
        cancellation = case.controller.cancel_generation_interest(case.owner, case.token)
        _assert_frozen_job_preserved(case, cancellation)
        _apply_frozen_pair(case, cancellation)
        case.controller._pair_deferred_sample_ids = frozenset()
        case.controller._expire_generation_interests()
        _assert_last_row_cancelled(case, cancellation, running=running)
    finally:
        if case.job.running:
            case.job.finish_read()
        case.job.cancel()
        if case.target_job is not None:
            case.target_job.cancel()


def test_deferred_cancellation_does_not_wait_for_foreign_surviving_shared_reader() -> None:
    controller, assets, _audio = _controller()
    job = _add_job(controller, assets)
    original = job.subscribers()[0]
    owner, token = _batch_owner(job)
    foreign_source = cast("OfflineStemSourceLease", Mock(name="foreign-surviving-source"))
    job.add_subscriber(36, foreign_source, "c" * 32, request=replace(job.request, sample_id=36))
    foreign = next(row for row in job.subscribers() if row.sample_id == 36)
    controller._generation_sources[36] = foreign_source
    assert job.begin() is original.source
    controller._pair_deferred_sample_ids = frozenset({0, 215})
    plan, acknowledgement = _prepare(controller)
    try:
        cancellation = controller.cancel_generation_interest(owner, token)
        assert controller.generation_cancellation_is_pending(cancellation)
        _project_after(controller, plan)
        plan.apply(acknowledgement)
        controller._pair_deferred_sample_ids = frozenset()
        controller._expire_generation_interests()

        assert job.subscribers() == (foreign,)
        assert job.running
        assert not job.cancel_event.is_set()
        assert controller._generation_sources[36] is foreign_source
        assert not cast("_Lease", foreign.source_lease).released
        assert not cast("_Reservation", foreign.retirement).closed
        assert not controller.generation_cancellation_is_pending(cancellation)
        assert job.finish_read()
    finally:
        if job.running:
            job.finish_read()
        job.cancel()


def test_direct_interest_in_same_fenced_row_never_becomes_a_batch_cancellation_reader() -> None:
    controller, assets, _audio = _controller()
    job = _add_job(controller, assets)
    original = job.subscribers()[0]
    owner, token = _batch_owner(job, direct=True)
    assert job.begin() is original.source
    controller._pair_deferred_sample_ids = frozenset({0, 215})
    plan, acknowledgement = _prepare(controller)
    try:
        cancellation = controller.cancel_generation_interest(owner, token)

        assert cancellation.disposition == "remaining"
        assert owner.current == frozenset({DIRECT_STEM_INTEREST})
        assert not controller.generation_cancellation_is_pending(cancellation)
        assert job.subscribers() == (original,)
        _project_after(controller, plan)
        plan.apply(acknowledgement)
        controller._pair_deferred_sample_ids = frozenset()
        controller._expire_generation_interests()
        assert job.subscribers()[0].sample_id == 215
        assert job.subscribers()[0].interest_owner is owner
        assert job.running
        assert not job.cancel_event.is_set()
        assert job.finish_read()
    finally:
        if job.running:
            job.finish_read()
        job.cancel()


def test_new_direct_caller_rejoins_empty_fenced_owner_without_extending_batch_cancel() -> None:
    case = _pair_cancellation(occupied=False, running=True)
    try:
        cancellation = case.controller.cancel_generation_interest(case.owner, case.token)
        _assert_frozen_job_preserved(case, cancellation)

        # Admission attaches only the legitimate new token to the captured exact row.
        assert case.job.add_interest(case.original, DIRECT_STEM_INTEREST)
        assert case.owner.current == frozenset({DIRECT_STEM_INTEREST})
        assert case.job.subscribers() == (case.original,)
        assert case.job._subscribers is case.images[3]
        assert not case.controller.generation_cancellation_is_pending(cancellation)
        with pytest.raises(RuntimeError, match=r"acknowledgement|receipt"):
            case.plan.apply(object())
        _project_after(case.controller, case.plan)
        case.plan.apply(case.acknowledgement)

        assert case.plan.applied
        moved = case.job.subscribers()[0]
        assert moved.sample_id == 215
        assert moved.interest_owner is case.owner
        assert moved.source is case.original.source
        assert moved.request is case.original.request
        assert not case.controller.generation_cancellation_is_pending(cancellation)
        case.controller._pair_deferred_sample_ids = frozenset()
        case.controller._expire_generation_interests()

        assert case.job.subscribers() == (moved,)
        assert case.controller._generation_sources[215] is case.original.source
        assert case.owner.current == frozenset({DIRECT_STEM_INTEREST})
        assert case.job.running
        assert not case.job.cancel_event.is_set()
        assert not case.physical.released
        assert not cast("_Lease", moved.source_lease).released
        assert not cast("_Reservation", moved.retirement).closed
        assert not case.controller.generation_cancellation_is_pending(cancellation)
        assert case.job.finish_read()
    finally:
        if case.job.running:
            case.job.finish_read()
        case.job.cancel()


def test_final_batch_cancel_preserves_acknowledged_bank_clear_and_owned_last_read(
    tmp_path: Path,
) -> None:
    harness = _harness(tmp_path, clear=True)
    controller = harness.controls.app.stems
    job = _add_job(controller, harness.assets, 36)
    original = job.subscribers()[0]
    owner, token = _batch_owner(job)
    assert job.begin() is original.source
    physical = harness.assets.leases[0]
    controller._pair_deferred_sample_ids = frozenset(range(36, 72))
    plan = harness.prepare()
    sources = controller._generation_sources
    try:
        cancellation = controller.cancel_generation_interest(owner, token)

        assert not owner.current
        assert controller._generation_sources is sources
        assert sources[36] is original.source
        assert job.subscribers() == (original,)
        assert not job.cancel_event.is_set()
        assert controller.generation_cancellation_is_pending(cancellation)
        plan.apply(harness.controls.acknowledgement)
        assert plan.applied
        assert plan.settle_retired()
        assert plan.settled
        assert not job.subscribers()
        assert 36 not in controller._generation_sources
        assert all(value is None for value in controller._project.pad_content[36:72])
        assert job.running
        assert not physical.released
        assert controller.generation_cancellation_is_pending(cancellation)

        controller._pair_deferred_sample_ids = frozenset()
        controller._expire_generation_interests()
        assert controller.generation_cancellation_is_pending(cancellation)
        assert not job.finish_read()
        assert physical.released
        assert not controller.generation_cancellation_is_pending(cancellation)
    finally:
        if job.running:
            job.finish_read()
        job.cancel()


def test_final_batch_cancel_and_queued_pair_entry_keep_event_and_reader_until_mirror() -> None:
    controller, assets, audio = _controller()
    audio.prepare_stem_pair = Mock(name="no-native-preparation-during-cancelled-fence")
    request = _pending_preparation(controller, assets)
    token = StemGenerationInterest("queued-pair-batch-only")
    owner = StemGenerationInterests(frozenset({token}))
    request.interest_owner = owner
    controller._pair_deferred_sample_ids = frozenset({0, 215})
    plan, acknowledgement = _prepare(controller)
    cancelled = request.cancelled
    try:
        cancellation = controller.cancel_generation_interest(owner, token)

        assert not owner.current
        assert cancellation.preparations == (request,)
        assert controller.generation_cancellation_is_pending(cancellation)
        assert request.cancelled is cancelled
        assert not cancelled.is_set()
        assert controller._pair_preparations.pending[0] is request
        # Exercise the actual queued worker entry and its normal returned-result queue.
        controller._pair_preparations._run(request, components=True)
        assert request.started.is_set()
        audio.prepare_stem_pair.assert_not_called()
        assert not cancelled.is_set()
        assert not cast("_Lease", request.lease).released
        assert not cast("_Reservation", request.retirement).closed
        controller._poll_pair_preparations()
        assert controller._pair_preparations.pending[0] is request
        assert not cancelled.is_set()

        _project_after(controller, plan)
        plan.apply(acknowledgement)
        assert plan.applied
        assert controller._pair_preparations.pending[215] is request
        assert request.sample_id == 0
        assert not cancelled.is_set()
        assert controller.generation_cancellation_is_pending(cancellation)
        controller._pair_deferred_sample_ids = frozenset()
        controller._poll_pair_preparations()

        assert cancelled.is_set()
        assert request.sample_id == 215
        assert not controller._pair_preparations.pending
        assert cast("_Lease", request.lease).released
        assert cast("_Reservation", request.retirement).closed
        assert not controller.generation_cancellation_is_pending(cancellation)
        audio.prepare_stem_pair.assert_not_called()
    finally:
        request.close()


@pytest.mark.parametrize("route", ["returned_pair", "disk_settlement"])
def test_final_batch_cancel_preserves_returned_pair_and_settlement_event_guards(
    route: str,
) -> None:
    controller, assets, _audio = _controller()
    request = _pending_preparation(controller, assets)
    token = StemGenerationInterest("returned-pair-batch-only")
    owner = StemGenerationInterests(frozenset({token}))
    request.interest_owner = owner
    request.prepared = cast("PreparedStemPair", Mock(name="declared-returned-pair"))
    if route == "disk_settlement":
        controller._pair_preparations.pending.pop(0)
        controller._disk_selection_settlements[0] = stems._DiskSelectionSettlement(
            request,
            request.entry,
            request.ticket,
            request.lease,
            None,
            request.retirement,
        )
    else:
        controller._pair_preparations._results.put(request)
    controller._pair_deferred_sample_ids = frozenset({0, 215})
    plan, acknowledgement = _prepare(controller)
    cancelled = request.cancelled
    prepared = request.prepared
    try:
        cancellation = controller.cancel_generation_interest(owner, token)

        assert not owner.current
        assert cancellation.preparations == (request,)
        assert controller.generation_cancellation_is_pending(cancellation)
        assert request.cancelled is cancelled
        assert not cancelled.is_set()
        assert request.prepared is prepared
        controller._poll_pair_preparations()
        assert not cancelled.is_set()
        assert not cast("_Lease", request.lease).released
        assert not cast("_Reservation", request.retirement).closed
        _project_after(controller, plan)
        plan.apply(acknowledgement)

        assert plan.applied
        assert request.cancelled is cancelled
        assert not cancelled.is_set()
        assert request.prepared is prepared
        if route == "disk_settlement":
            assert controller._disk_selection_settlements[215].request is request
            assert request.sample_id == 215
        else:
            assert controller._pair_preparations.pending[215] is request
            assert request.sample_id == 0
        assert controller.generation_cancellation_is_pending(cancellation)
        cast("Mock", prepared).discard.assert_not_called()
        cast("Mock", prepared).select.assert_not_called()
    finally:
        request.close()
