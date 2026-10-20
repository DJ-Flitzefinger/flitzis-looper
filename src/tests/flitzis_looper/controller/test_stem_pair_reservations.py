"""Reserved ordinary Pair admission on the real shared pool; no audio acceptance claim."""

from contextlib import contextmanager
from dataclasses import replace
from threading import Event
from time import monotonic
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.stem_job import StemGenerationInterest, StemGenerationInterests
from flitzis_looper.controller.stem_pair_preparation import (
    StemPairIntentBinding,
    StemPairPreparationQueue,
    StemPairPreparationSpec,
)
from flitzis_looper.controller.stem_workers import (
    STEM_QUEUED_JOBS,
    STEM_WORKERS,
    StemWorkerPool,
)
from tests.flitzis_looper.conftest import FakePreparedSourceTicket
from tests.flitzis_looper.controller.test_stem_interest_handoff import _PairAssets
from tests.flitzis_looper.controller.test_stem_pair_preparation import _known_pair_entry
from tests.flitzis_looper.controller.test_stem_worker_reservations import _occupied_pool

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

    from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
    from flitzis_looper.controller.stem_generation import StemGenerationRequest
    from flitzis_looper.controller.stem_intent import StemGenerationIntent
    from flitzis_looper.controller.stem_pair_preparation import PairPreparation
    from flitzis_looper.controller.stem_workers import StemWorkerReservation
    from flitzis_looper_audio import AudioEngine, PreparedSourceTicket, PreparedStemPair
    from tests.flitzis_looper.controller.test_stem_job import _Lease, _Reservation


def _spec(
    sample_id: int = 0, binding: StemPairIntentBinding | None = None
) -> StemPairPreparationSpec:
    return StemPairPreparationSpec(
        sample_id,
        _known_pair_entry(),
        cast("PreparedSourceTicket", FakePreparedSourceTicket("pending")),
        f"original-content-{sample_id}",
        None,
        components=False,
        binding=binding,
    )


def _queue(
    assets: _PairAssets, runner: Callable[[Callable[[], None]], None]
) -> tuple[StemPairPreparationQueue, Mock]:
    audio = Mock(name="ordinary-reserved-pair-reader")
    return (
        StemPairPreparationQueue(
            cast("AudioEngine", audio), cast("ProjectAssetLifecycle", assets), runner
        ),
        audio,
    )


@contextmanager
def _reserved_slots(pool: StemWorkerPool, count: int) -> Iterator[list[StemWorkerReservation]]:
    held: list[StemWorkerReservation] = []
    try:
        for _ in range(count):
            reservation = pool.try_reserve()
            assert reservation is not None
            held.append(reservation)
        yield held
    finally:
        for reservation in held:
            reservation.close()


def _collect(queue: StemPairPreparationQueue, count: int = 1) -> tuple[PairPreparation, ...]:
    returned: list[PairPreparation] = []
    deadline = monotonic() + 5
    wait = Event()
    while len(returned) < count and monotonic() < deadline:
        returned.extend(queue.collect())
        wait.wait(0.001)
    assert len(returned) == count, "Pair workers did not return within the bounded wait"
    return tuple(returned)


def _wait_for_capacity(pool: StemWorkerPool) -> StemWorkerReservation:
    deadline = monotonic() + 5
    wait = Event()
    while monotonic() < deadline:
        reservation = pool.try_reserve()
        if reservation is not None:
            return reservation
        wait.wait(0.001)
    message = "Actual Pair worker did not return its pool capacity"
    raise AssertionError(message)


def test_reserved_capacity_precedes_exact_pair_owners_and_preserves_captured_metadata(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    assets = _PairAssets()
    owner = StemGenerationInterests()
    spec = replace(
        _spec(
            binding=StemPairIntentBinding(
                generation_intent=cast("StemGenerationIntent", Mock(name="captured-intent")),
                own_source_version="original-own-source-version",
                artifact_source_version=_known_pair_entry().source_version,
                source_path="samples/original-own-source.wav",
                verify_saved_selection=True,
                interest_owner=owner,
            )
        ),
        previous_entry=_known_pair_entry(),
        processing_request=cast("StemGenerationRequest", Mock(name="captured-producer-request")),
        regenerate_on_incompatible=True,
        residency_only=True,
    )
    assert not assets.leases
    assert not assets.reservations
    with _occupied_pool() as (pool, release), _reserved_slots(pool, STEM_QUEUED_JOBS - 1):
        queue, audio = _queue(assets, pool)
        reserve = assets.reserve_pending

        def check_granted(count: int) -> _Reservation:
            assert pool.try_reserve() is None
            return reserve(count)

        monkeypatch.setattr(assets, "reserve_pending", check_granted)
        reservation = pool.try_reserve()
        assert reservation is not None
        queue.submit_reserved(spec, reservation)
        request = queue.pending[spec.sample_id]
        assert request.entry is spec.entry
        assert request.ticket is spec.ticket
        assert request.content_id == spec.content_id
        assert request.previous_entry is spec.previous_entry
        assert request.processing_request is spec.processing_request
        assert request.regenerate_on_incompatible
        assert request.residency_only
        assert spec.binding is not None
        assert request.verify_saved_selection
        assert request.generation_intent is spec.binding.generation_intent
        assert request.own_source_version == spec.binding.own_source_version
        assert request.artifact_source_version == spec.binding.artifact_source_version
        assert request.source_path == spec.binding.source_path
        assert request.interest_owner is owner
        queue.cancel(spec.sample_id)
        release.set()
        assert _collect(queue) == (request,)
        audio.prepare_stem_pair.assert_not_called()


def test_exhausted_pool_leaves_original_candidate_and_existing_owners_untouched() -> None:
    assets = _PairAssets()
    owner = StemGenerationInterests()
    spec = _spec(binding=StemPairIntentBinding(interest_owner=owner))
    snapshot = owner.current
    with _occupied_pool() as (pool, _release):
        for _ in range(STEM_QUEUED_JOBS):
            pool(lambda: None)
        queue, audio = _queue(assets, pool)
        submit = Mock(wraps=queue.submit_reserved)
        reservation = pool.try_reserve()
        if reservation is not None:
            submit(spec, reservation)
        submit.assert_not_called()
        assert not queue.pending
        assert not assets.leases
        assert not assets.reservations
        assert owner.current is snapshot
        audio.prepare_stem_pair.assert_not_called()


@pytest.mark.parametrize("blocker", ["pending", "discard"])
def test_reserved_admission_preserves_exact_existing_pending_or_discard_owner(blocker: str) -> None:
    assets = _PairAssets()
    with _occupied_pool() as (pool, release):
        queue, _audio = _queue(assets, pool)
        reservation = pool.try_reserve()
        assert reservation is not None
        queue.submit_reserved(_spec(), reservation)
        spec = _spec()
        if blocker == "discard":
            spec = _spec(1)
            prepared = cast("PreparedStemPair", Mock(name="retained-discard-owner"))
            queue._discard_retries[id(prepared)] = spec.sample_id, prepared
        before = queue.pending.copy(), tuple(assets.leases), tuple(assets.reservations)
        with _reserved_slots(pool, STEM_QUEUED_JOBS - 2):
            reservation = pool.try_reserve()
            assert reservation is not None
            with pytest.raises(RuntimeError, match=r"already pending|cleanup admission"):
                queue.submit_reserved(spec, reservation)
            assert (queue.pending, tuple(assets.leases), tuple(assets.reservations)) == before
            with _reserved_slots(pool, 1):
                assert pool.try_reserve() is None
        release.set()
        for request in _collect(queue):
            request.close()


@pytest.mark.parametrize("failure", ["source", "binding", "empty-holder", "retirement"])
def test_failed_reserved_admission_returns_unused_capacity_without_pending_mutation(
    monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    assets = _PairAssets()
    spec = _spec()
    if failure == "source":
        monkeypatch.setattr(assets, "acquire", Mock(side_effect=ValueError("invalid source")))
    elif failure == "binding":
        spec = replace(spec, binding=StemPairIntentBinding(own_source_version="different-source"))
    elif failure == "empty-holder":
        spec = replace(
            spec, binding=StemPairIntentBinding(interest_owner=StemGenerationInterests(frozenset()))
        )
    else:
        monkeypatch.setattr(
            assets, "reserve_pending", Mock(side_effect=RuntimeError("retirement full"))
        )
    with _occupied_pool() as (pool, _release), _reserved_slots(pool, STEM_QUEUED_JOBS - 1):
        queue, audio = _queue(assets, pool)
        reservation = pool.try_reserve()
        assert reservation is not None
        with pytest.raises(ValueError if failure in {"source", "binding"} else RuntimeError):
            queue.submit_reserved(spec, reservation)
        with _reserved_slots(pool, 1):
            assert pool.try_reserve() is None
        assert not queue.pending
        assert not assets.leases
        assert all(reservation.closed for reservation in assets.reservations)
        audio.prepare_stem_pair.assert_not_called()


def test_shutdown_submission_failure_rolls_back_pair_owners_and_capacity() -> None:
    assets = _PairAssets()
    pool = StemWorkerPool()
    queue, audio = _queue(assets, pool)
    reservation = pool.try_reserve()
    assert reservation is not None
    pool.shutdown()
    with pytest.raises(RuntimeError, match="cannot schedule new futures after shutdown"):
        queue.submit_reserved(_spec(), reservation)
    assert not queue.pending
    assert len(assets.leases) == len(assets.reservations) == 1
    assert assets.leases[0].released
    assert assets.reservations[0].closed
    with _reserved_slots(pool, STEM_WORKERS + STEM_QUEUED_JOBS):
        assert pool.try_reserve() is None
    audio.prepare_stem_pair.assert_not_called()


@pytest.mark.parametrize("notify_queue", [False, True])
def test_final_queued_caller_cancel_performs_no_native_read_and_keeps_exact_reader_until_return(
    *, notify_queue: bool
) -> None:
    assets = _PairAssets()
    batch = StemGenerationInterest("reserved-queued-batch")
    owner = StemGenerationInterests(frozenset({batch}))
    with _occupied_pool() as (pool, release):
        queue, audio = _queue(assets, pool)
        reservation = pool.try_reserve()
        assert reservation is not None
        queue.submit_reserved(
            _spec(binding=StemPairIntentBinding(interest_owner=owner)), reservation
        )
        request = queue.pending[0]
        assert owner.detach(batch) == "detached"
        if notify_queue:
            queue.cancel_interest_owner(owner)
        assert not cast("_Lease", request.lease).released
        assert not cast("_Reservation", request.retirement).closed
        assert queue.collect() == ()
        release.set()
        assert _collect(queue) == (request,)
        assert request.cancelled.is_set()
        assert cast("_Lease", request.lease).released
        assert cast("_Reservation", request.retirement).closed
        audio.prepare_stem_pair.assert_not_called()


def test_cancelled_running_pair_holds_pool_permit_and_reader_until_actual_return() -> None:
    assets = _PairAssets()
    pool = StemWorkerPool()
    queue, audio = _queue(assets, pool)
    entered, finish = Event(), Event()

    def prepare(*_args: object) -> PreparedStemPair:
        entered.set()
        assert finish.wait(5), "Pair reader did not receive its bounded release"
        return cast("PreparedStemPair", Mock(name="actual-returned-pair"))

    audio.prepare_stem_pair.side_effect = prepare
    reservation = pool.try_reserve()
    assert reservation is not None
    try:
        queue.submit_reserved(_spec(), reservation)
        assert entered.wait(5)
        request = queue.pending[0]
        with _reserved_slots(pool, STEM_WORKERS + STEM_QUEUED_JOBS - 1):
            assert pool.try_reserve() is None
            queue.cancel(0)
            reservation.close()
            assert pool.try_reserve() is None
            assert not cast("_Lease", request.lease).released
            assert not cast("_Reservation", request.retirement).closed
            assert queue.collect() == ()
            finish.set()
            assert _collect(queue) == (request,)
            with _wait_for_capacity(pool):
                assert pool.try_reserve() is None
        assert request.error == "Stem pair preparation cancelled"
        assert cast("_Lease", request.lease).released
        assert cast("_Reservation", request.retirement).closed
    finally:
        finish.set()
        pool.shutdown()


def test_legacy_and_reserved_pair_submission_share_actual_pool_limits() -> None:
    assets = _PairAssets()
    with _occupied_pool() as (pool, release):
        for _ in range(STEM_QUEUED_JOBS - 2):
            pool(lambda: None)
        queue, _audio = _queue(assets, pool)
        reservation = pool.try_reserve()
        assert reservation is not None
        queue.submit_reserved(_spec(), reservation)
        spec = _spec(1)
        queue.submit(
            spec.sample_id,
            spec.entry,
            spec.ticket,
            spec.content_id,
            spec.previous_entry,
            components=spec.components,
        )
        assert pool.try_reserve() is None
        with pytest.raises(RuntimeError, match="Stem queue full"):
            queue.submit(2, spec.entry, spec.ticket, "third-content", None, components=False)
        assert set(queue.pending) == {0, 1}
        assert len(assets.leases) == len(assets.reservations) == 3
        assert assets.leases[-1].released
        assert assets.reservations[-1].closed
        release.set()
        for request in _collect(queue, 2):
            request.close()
        assert queue.collect() == ()


def test_returned_reserved_requests_collect_once_and_preserve_moved_and_replacement_owners() -> (
    None
):
    assets = _PairAssets()
    original_owner = StemGenerationInterests()
    replacement_owner = StemGenerationInterests()
    with _occupied_pool() as (pool, release):
        queue, audio = _queue(assets, pool)
        reservation = pool.try_reserve()
        assert reservation is not None
        queue.submit_reserved(
            _spec(binding=StemPairIntentBinding(interest_owner=original_owner)), reservation
        )
        original = queue.pending.pop(0)
        queue.pending[215] = original
        reservation = pool.try_reserve()
        assert reservation is not None
        queue.submit_reserved(
            _spec(binding=StemPairIntentBinding(interest_owner=replacement_owner)), reservation
        )
        replacement = queue.pending[0]
        release.set()
        returned = _collect(queue, 2)
        assert {id(request) for request in returned} == {id(original), id(replacement)}
        assert queue.collect() == ()
        assert queue.pending == {215: original}
        assert original.sample_id == replacement.sample_id == 0
        assert original.interest_owner is original_owner
        assert replacement.interest_owner is replacement_owner
        assert original_owner.current == replacement_owner.current
        assert original_owner.current
        assert audio.prepare_stem_pair.call_count == 2
        assert queue.pending.pop(215) is original
        for request in returned:
            request.close()
