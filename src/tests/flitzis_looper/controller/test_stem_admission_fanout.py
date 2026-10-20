"""Production Pair admission over retained, genuine completed-generation rows.

The deterministic worker-delivery fixture runs the real identity/backend bodies.
The executor, capacity reservations, terminal custody and Pair submission are real;
cached Native sources and the tiny file-writing separator remain explicit doubles.
"""

from contextlib import ExitStack
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import stems as stems_module
from flitzis_looper.controller.stem_cache import promote_generation_artifacts
from flitzis_looper.controller.stem_job import (
    DIRECT_STEM_INTEREST,
    StemGenerationInterest,
)
from flitzis_looper.controller.stem_workers import STEM_QUEUED_JOBS
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.conftest import FakePreparedSourceTicket

# Pytest discovers this imported autouse fixture in the requesting module.
from tests.flitzis_looper.controller.test_shared_stem_material import (
    _declared_shared_producer,  # noqa: F401
)
from tests.flitzis_looper.controller.test_shared_stem_reuse import _known_set
from tests.flitzis_looper.controller.test_stem_admission import _held_pool
from tests.flitzis_looper.controller.test_stem_pending_admission import _production_pool

pytestmark = pytest.mark.usefixtures("_declared_shared_producer")

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_intent import StemGenerationIntent
    from flitzis_looper.controller.stem_job import StemGenerationJob, StemSubscriber
    from flitzis_looper.controller.stem_workers import StemWorkerPool, StemWorkerReservation
    from flitzis_looper.controller.stems import _StemJobEventDelivery
    from flitzis_looper.models import StemCacheEntry
    from flitzis_looper_audio import OfflineStemSourceLease, PreparedSourceTicket
    from tests.flitzis_looper.conftest import FakeProjectAssetLease, FakeStemGenerationBackend


def _reserve_queue(pool: StemWorkerPool, stack: ExitStack) -> tuple[StemWorkerReservation, ...]:
    """Hold all queue permits while the helper's two actual workers are blocked."""
    reservations = []
    for _ in range(STEM_QUEUED_JOBS):
        reservation = pool.try_reserve()
        assert reservation is not None
        reservations.append(stack.enter_context(reservation))
    assert pool.try_reserve() is None
    return tuple(reservations)


def _add_subscriber(
    controller: AppController, audio: Mock, job: StemGenerationJob, sample_id: int
) -> None:
    """Use existing row admission with a distinct cached Native source incarnation."""
    stems = controller.stems
    version = stems._loaded_source_version_for_pad(sample_id)
    content = controller.project.pad_content[sample_id]
    assert version is not None
    assert content is not None
    request = replace(job.request, sample_id=sample_id, source_version=version)
    source = audio.capture_offline_stem_source(sample_id, version)
    intent = stems._intents.capture(request, controller.project.stem_processing_revision)
    job.add_subscriber(sample_id, source, content.instance_id, request=request, intent=intent)
    stems._generation_sources[sample_id] = source
    stems._generation_intents[sample_id] = intent
    controller.session.stem_generating_sample_ids.add(sample_id)
    controller.session.stem_generation_source_versions[sample_id] = version


def _resolved_subscribers(
    controller: AppController,
    audio: Mock,
    sample_ids: tuple[int, ...],
    *,
    reuse_only: bool = False,
) -> tuple[StemGenerationJob, list[Callable[[], None]]]:
    """Prepare actual independent rows; only preterminal worker delivery is controlled."""
    stems = controller.stems
    tasks: list[Callable[[], None]] = []
    stems._stem_task_runner = tasks.append
    origin = sample_ids[0]
    source_path = controller.project.sample_paths[origin]
    assert source_path is not None
    for sample_id in sample_ids:
        if controller.project.sample_paths[sample_id] is None:
            controller.project.sample_paths[sample_id] = source_path
            controller.project.sample_durations[sample_id] = 128 / 44_100
            audio.load_sample_async(sample_id, source_path)
        controller.project.pad_content[sample_id] = PadContentIdentity(
            instance_id=f"{sample_id + 1:032x}"
        )
    stems._assets.sync_assignments()
    version = stems._loaded_source_version_for_pad(origin)
    assert version is not None
    request = stems._generation_request(origin, version)
    assert request is not None
    source = audio.capture_offline_stem_source(origin, version)
    assert stems._admit_stem_generation(request, source, reuse_only=reuse_only)
    job = stems._jobs[request.cache_dir]
    for sample_id in sample_ids[1:]:
        _add_subscriber(controller, audio, job, sample_id)
    assert len(tasks) == 1
    tasks.pop()()  # Real full-source/producer identity resolution.
    stems._poll_generation_events()
    assert job.resolved_request is not None
    assert job.generation_fingerprint is not None
    return job, tasks


def _complete_job(
    controller: AppController, job: StemGenerationJob, tasks: list[Callable[[], None]]
) -> None:
    """Complete real artifact production before exercising the Pair capacity boundary."""
    stems = controller.stems
    stems._waiting_jobs.discard(job.request.cache_dir)
    assert stems._schedule_stem_job(job)
    assert len(tasks) == 1
    tasks.pop()()
    assert str(job.phase) == "settled"
    assert job.request.cache_dir.is_dir()


@dataclass(frozen=True, slots=True)
class _TerminalImage:
    delivery: _StemJobEventDelivery
    rows: tuple[StemSubscriber, ...]
    sources: dict[int, OfflineStemSourceLease]
    intents: dict[int, StemGenerationIntent]
    entries: tuple[StemCacheEntry | None, ...]
    retirement: int


def _terminal_image(controller: AppController, job: StemGenerationJob) -> _TerminalImage:
    """Snapshot pre-delivery state before the first full-pool control drain."""
    stems = controller.stems
    original = job.subscribers()
    sources = dict(stems._generation_sources)
    intents = dict(stems._generation_intents)
    entries = tuple(controller.project.stem_cache)
    retirement = stems._assets._reserved
    stems.on_frame_render()
    return _TerminalImage(
        stems._pair_held_generation_events[job, "terminal"],
        original,
        sources,
        intents,
        entries,
        retirement,
    )


def _assert_source_readers_retained(rows: tuple[StemSubscriber, ...]) -> None:
    for row in rows:
        assert row.source_lease is not None
        assert not cast("FakeProjectAssetLease", row.source_lease).released


def _assert_terminal_backpressure(
    controller: AppController, audio: Mock, image: _TerminalImage, promote: Mock
) -> None:
    stems = controller.stems
    delivery, original = image.delivery, image.rows
    assert stems._pair_held_generation_events[delivery.job, "terminal"] is delivery
    assert len(delivery.remaining) == 216
    assert all(left is right for left, right in zip(delivery.remaining, original, strict=True))
    assert delivery.job.subscribers() == original
    assert stems._generation_sources == image.sources
    assert stems._generation_intents == image.intents
    assert tuple(controller.project.stem_cache) == image.entries
    assert controller.session.stem_generating_sample_ids == set(range(216))
    assert stems._assets._reserved == image.retirement
    assert all(row.retirement.remaining == 16 for row in original)
    _assert_source_readers_retained(original)
    assert not stems._pair_preparations.pending
    assert not stems._requested_preparations
    assert not delivery.promotion_attempted
    assert delivery.promotion is None
    promote.assert_not_called()
    audio.capture_prepared_stem_source.assert_not_called()
    audio.prepare_stem_pair.assert_not_called()
    audio.acquire_project_asset_lease.assert_not_called()
    audio.retire_project_asset.assert_not_called()


def _assert_two_granted_rows(
    controller: AppController,
    audio: Mock,
    image: _TerminalImage,
    tickets: dict[int, FakePreparedSourceTicket],
    tmp_path: Path,
) -> None:
    stems = controller.stems
    delivery, original = image.delivery, image.rows
    assert delivery.remaining == original[2:]
    assert len(delivery.remaining) == 214
    assert set(stems._pair_preparations.pending) == {0, 1}
    assert stems._stem_preparation_count() == 2
    assert controller.session.stem_generating_sample_ids == set(range(2, 216))
    assert all(row.retirement.remaining == 0 for row in original[:2])
    assert all(row.retirement.remaining == 16 for row in original[2:])
    _assert_source_readers_retained(original)
    assert delivery.promotion is not None
    assert (tmp_path / delivery.promotion.entry.cache_dir).is_dir()
    assert not delivery.job.request.cache_dir.exists()
    first, second = (stems._pair_preparations.pending[sample_id] for sample_id in (0, 1))
    assert first.entry is not second.entry
    assert first.entry.cache_dir == second.entry.cache_dir
    for row, pending in zip(original[:2], (first, second), strict=True):
        assert pending.generation_intent is row.intent
        assert pending.interest_owner is row.interest_owner
        assert pending.content_id == row.content_id
        assert pending.ticket is cast("PreparedSourceTicket", tickets[row.sample_id])
    captures = audio.capture_prepared_stem_source.call_args_list
    assert [call.args[0] for call in captures] == [0, 1]


def test_full_pool_terminal_custody_keeps_216_rows_then_consumes_only_two_granted_rows(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with _held_pool() as held, ExitStack() as stack:
        reservations = _reserve_queue(held.pool, stack)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        tickets = {sample_id: FakePreparedSourceTicket() for sample_id in range(216)}
        audio_engine_mock.capture_prepared_source.side_effect = lambda sample_id, _version: tickets[
            sample_id
        ]
        audio_engine_mock.prepare_stem_pair = Mock()
        promote = Mock(wraps=promote_generation_artifacts)
        monkeypatch.setattr(stems_module, "promote_generation_artifacts", promote)
        try:
            job, tasks = _resolved_subscribers(controller, audio_engine_mock, tuple(range(216)))
            _assert_source_readers_retained(job.subscribers())
            _complete_job(controller, job, tasks)
            audio_engine_mock.acquire_project_asset_lease.reset_mock()
            audio_engine_mock.retire_project_asset.reset_mock()
            image = _terminal_image(controller, job)
            for _ in range(2):
                stems.on_frame_render()
                _assert_terminal_backpressure(controller, audio_engine_mock, image, promote)
            late_batch = StemGenerationInterest("joins retained terminal row")
            original = image.rows[0]
            assert stems.request_generation_interest(0, late_batch) is original.interest_owner
            assert original.interests == frozenset({DIRECT_STEM_INTEREST, late_batch})
            cancellation = stems.cancel_generation_interest(original.interest_owner, late_batch)
            assert cancellation.disposition == "remaining"
            assert original.interests == frozenset({DIRECT_STEM_INTEREST})
            assert job.subscribers()[0] is original
            assert image.delivery.remaining[0] is original
            _assert_terminal_backpressure(controller, audio_engine_mock, image, promote)
            reservations[0].close()
            reservations[1].close()
            stems.on_frame_render()
            _assert_two_granted_rows(controller, audio_engine_mock, image, tickets, tmp_path)
            assert held.pool.try_reserve() is None
            promote.assert_called_once()
            pending = dict(stems._pair_preparations.pending)
            stems.on_frame_render()
            assert image.delivery.remaining == image.rows[2:]
            assert stems._pair_preparations.pending == pending
            assert len(stem_backend.requests) == 1
            promote.assert_called_once()
            audio_engine_mock.prepare_stem_pair.assert_not_called()
        finally:
            stems.shut_down()


def test_full_pool_reuse_retains_exact_identity_rows_until_one_pair_permit_returns(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    entry, selected, _files = _known_set(controller, audio_engine_mock, tmp_path, monkeypatch)
    with _held_pool() as held, ExitStack() as stack:
        reservations = _reserve_queue(held.pool, stack)
        stems = controller.stems
        stems._stem_worker_pool = held.pool
        stems._pair_preparations._runner = held.pool
        try:
            job, tasks = _resolved_subscribers(
                controller, audio_engine_mock, (215, 0), reuse_only=True
            )
            delivery = stems._pair_held_generation_events[job, "identity"]
            original = job.subscribers()
            assert delivery.admission_route == "reuse"
            assert delivery.remaining == original
            assert delivery.promotion is not None
            assert delivery.promotion.entry.cache_dir == entry.cache_dir
            assert not tasks
            for _ in range(2):
                stems.on_frame_render()
                assert stems._pair_held_generation_events[job, "identity"] is delivery
                assert delivery.remaining == original
                assert all(row.retirement.remaining == 16 for row in original)
                _assert_source_readers_retained(original)
                assert not stems._pair_preparations.pending
                audio_engine_mock.capture_prepared_stem_source.assert_not_called()
                audio_engine_mock.prepare_stem_pair.assert_not_called()
            reservations[0].close()
            stems.on_frame_render()
            assert delivery.remaining == original[1:]
            assert set(stems._pair_preparations.pending) == {215}
            pending = stems._pair_preparations.pending[215]
            assert pending.interest_owner is original[0].interest_owner
            assert pending.generation_intent is original[0].intent
            assert original[0].retirement.remaining == 0
            assert original[1].retirement.remaining == 16
            assert held.pool.try_reserve() is None
            stems.on_frame_render()
            assert delivery.remaining == original[1:]
            assert stems._pair_preparations.pending == {215: pending}
            audio_engine_mock.capture_prepared_stem_source.assert_called_once()
            audio_engine_mock.prepare_stem_pair.assert_not_called()
            assert not stem_backend.requests
            assert selected.selected == selected.discarded == 0
        finally:
            stems.shut_down()


def test_direct_and_batch_share_one_pending_owner_and_final_pregrant_cancel_reads_nothing(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with _held_pool() as held, ExitStack() as stack:
        reservations = _reserve_queue(held.pool, stack)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        batch = StemGenerationInterest("fanout-pregrant-batch")
        try:
            owner = stems.request_generation_interest(0, batch)
            assert owner is not None
            requested = stems._requested_generations[0]
            assert stems.generate_stems_async(0)
            assert stems.request_generation_interest(0, batch) is owner
            assert owner.current == frozenset({batch, DIRECT_STEM_INTEREST})
            assert stems.cancel_generation_interest(owner, batch).disposition == "remaining"
            assert owner.current == frozenset({DIRECT_STEM_INTEREST})
            assert stems._requested_generations[0] is requested
            assert stems.generation_interest_is_pending(owner)
            cancellation = stems.cancel_generation_interest(owner, DIRECT_STEM_INTEREST)
            assert cancellation.disposition == "detached"
            assert not owner.current
            assert not stems.generation_interest_is_pending(owner)
            reservations[0].close()
            stems.on_frame_render()
            assert not stems._requested_generations
            assert not stems._jobs
            assert not stems._pair_preparations.pending
            assert not stems._generation_sources
            assert not stems._generation_intents
            assert not controller.session.stem_generating_sample_ids
            audio_engine_mock.capture_offline_stem_source.assert_not_called()
            audio_engine_mock.capture_prepared_stem_source.assert_not_called()
        finally:
            stems.shut_down()


def test_terminal_subscriber_changed_after_reservation_has_original_failure_outcome(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with _held_pool() as held, ExitStack() as stack:
        reservations = _reserve_queue(held.pool, stack)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        audio_engine_mock.prepare_stem_pair = Mock()
        try:
            job, tasks = _resolved_subscribers(controller, audio_engine_mock, (0,))
            _complete_job(controller, job, tasks)
            stems.on_frame_render()
            (original,) = job.subscribers()
            delivery = stems._pair_held_generation_events[job, "terminal"]
            assert delivery.remaining == (original,)
            reserve = held.pool.try_reserve
            changed = False

            def reserve_then_replace() -> StemWorkerReservation | None:
                nonlocal changed
                reservation = reserve()
                if reservation is not None and not changed:
                    changed = True
                    controller.project.pad_content[0] = PadContentIdentity(instance_id="e" * 32)
                return reservation

            monkeypatch.setattr(held.pool, "try_reserve", reserve_then_replace)
            reservations[0].close()
            stems.on_frame_render()
            assert changed
            assert original.interest_owner.failure is not None
            assert not original.interest_owner.committed
            assert not stems.generation_interest_is_pending(original.interest_owner)
            assert not delivery.remaining
            assert not stems._pair_preparations.pending
            assert not stems._jobs
            assert stems.stem_generation_error(0) is None
            audio_engine_mock.capture_prepared_stem_source.assert_not_called()
            audio_engine_mock.prepare_stem_pair.assert_not_called()
            reservation = reserve()
            assert reservation is not None
            reservation.close()
        finally:
            stems.shut_down()
