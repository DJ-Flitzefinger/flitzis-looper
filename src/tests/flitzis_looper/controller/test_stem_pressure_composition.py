"""Resource policy through real controller/pool owners; no model/device acceptance.

Host images, loaded-source metadata, Native Pair preparation and separator work
are explicit substitutes. Existing source/job/interest/retirement/event ownership
and the original two-worker pool remain production code.
"""

import threading
from contextlib import contextmanager
from functools import partial
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.stem_job import StemGenerationInterest
from flitzis_looper.controller.stem_resources import StemResourceGuard
from flitzis_looper.controller.stem_workers import STEM_WORKERS
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.conftest import FakeOfflineStemSourceLease

# Pytest discovers this imported autouse fixture under its declared name.
from tests.flitzis_looper.controller.test_shared_stem_material import (
    _declared_shared_producer as _declared_shared_producer,  # noqa: PLC0414
)
from tests.flitzis_looper.controller.test_stem_admission import _held_pool
from tests.flitzis_looper.controller.test_stem_admission_composition import _observe_phase_returns
from tests.flitzis_looper.controller.test_stem_admission_retirement import _request_pair
from tests.flitzis_looper.controller.test_stem_pending_admission import _production_pool

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_job import StemGenerationInterests
    from flitzis_looper.controller.stem_workers import StemWorkerPool
    from flitzis_looper.controller.stems import StemController, _RequestedStemGeneration
    from tests.flitzis_looper.conftest import FakeStemGenerationBackend


class _ResourceImage:
    """Deterministic observations; no claim that a model has this memory cost."""

    def __init__(self, state: str = "clear") -> None:
        self.state = state
        self.now = 0.0

    def sample(self) -> dict[str, int]:
        if self.state == "unavailable":
            message = "fixture host query unavailable"
            raise OSError(message)
        gib = 1024**3
        return {
            "available_commit": gib if self.state == "pressure" else 4 * gib,
            "system_available_commit_bytes": gib if self.state == "pressure" else 4 * gib,
            "available_physical": 256 * 1024**2 if self.state == "pressure" else gib,
        }

    def change(self, state: str) -> None:
        self.state = state
        self.now += 0.5

    def bind(self, stems: StemController) -> StemResourceGuard:
        guard = StemResourceGuard(self.sample, lambda: self.now)
        stems._stem_resources = guard
        stems._pair_preparations._resources = guard
        return guard


@contextmanager
def _block_existing_workers(pool: StemWorkerPool) -> Iterator[None]:
    """Hold returned capacity on this same pool so the next original phase queues."""
    release = threading.Event()
    started = tuple(threading.Event() for _ in range(STEM_WORKERS))
    finished = tuple(threading.Event() for _ in range(STEM_WORKERS))

    def hold(index: int) -> None:
        started[index].set()
        try:
            assert release.wait(10)
        finally:
            finished[index].set()

    try:
        for index in range(STEM_WORKERS):
            pool(partial(hold, index))
        assert all(event.wait(5) for event in started)
        yield
    finally:
        release.set()
        assert all(event.wait(5) for event in finished)


def _assert_recovered_request(
    controller: AppController,
    audio: Mock,
    original: _RequestedStemGeneration,
    owner: StemGenerationInterests,
) -> None:
    """Observe transfer of the same immutable request into its actual admitted job."""
    stems = controller.stems
    assert 0 not in stems._requested_generations
    (job,) = stems._jobs.values()
    (subscriber,) = job.subscribers()
    assert job.request is subscriber.request is original.request
    assert job.intent is subscriber.intent is original.intent
    assert subscriber.interest_owner is owner
    content = original.capture.content
    assert content is not None
    assert subscriber.content_id == content.instance_id
    assert stems._generation_sources[0] is subscriber.source
    assert stems.generation_interest_is_pending(owner)
    assert owner.failure is None
    audio.capture_offline_stem_source.assert_called_once_with(0, original.request.source_version)


def _assert_failed_generation(
    stems: StemController, owner: StemGenerationInterests, expected: str, reserved: int
) -> None:
    """The original event/finally route must settle every generation owner once."""
    assert owner.failure is not None
    assert expected in owner.failure
    error = stems.stem_generation_error(0)
    assert error is not None
    assert expected in error
    assert not stems.generation_interest_is_pending(owner)
    assert not stems._jobs
    assert not stems._generation_sources
    assert not stems._generation_intents
    assert not stems._waiting_jobs
    assert not stems._inference_jobs
    assert not stems._session.stem_generating_sample_ids
    assert stems._assets._reserved == reserved


@pytest.mark.parametrize("state", ["pressure", "unavailable"])
def test_resource_wait_preserves_original_request_and_recovery_admits_same_owner(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    state: str,
) -> None:
    with _held_pool() as held:
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        image = _ResourceImage(state)
        guard = image.bind(stems)
        reserve = Mock(wraps=held.pool.try_reserve)
        monkeypatch.setattr(held.pool, "try_reserve", reserve)
        token = StemGenerationInterest("resource-wait-original")
        owner = stems.request_generation_interest(0, token)
        assert owner is not None
        original = stems._requested_generations[0]
        readiness = original.ready_sequence
        cursor = stems._stem_admission._cursor
        reserved = stems._assets._reserved
        assignments = stems._assets._assignments.copy()
        audio_engine_mock.acquire_project_asset_lease.reset_mock()
        try:
            for _ in range(3):
                stems.on_frame_render()
                assert guard.current.state == state
                assert stems.stem_generation_stage(0) == guard.current.message
                assert stems._requested_generations[0] is original
                assert original.owner is owner
                assert original.ready_sequence == readiness
                assert stems._stem_admission._cursor == cursor
                assert stems._assets._reserved == reserved
                assert stems._assets._assignments == assignments
                assert not stems._jobs
                assert not stems._generation_sources
                assert not stems._generation_intents
                assert not controller.session.stem_generating_sample_ids
                assert stems.generation_interest_is_pending(owner)
                assert owner.failure is None
                reserve.assert_not_called()
                audio_engine_mock.capture_offline_stem_source.assert_not_called()
                audio_engine_mock.acquire_project_asset_lease.assert_not_called()

            image.change("clear")
            stems.on_frame_render()
            assert guard.current.allowed
            _assert_recovered_request(controller, audio_engine_mock, original, owner)
            reserve.assert_called_once_with()
        finally:
            # Cancel the genuine queued worker before the fixture returns capacity.
            stems.shut_down()


@pytest.mark.parametrize("change", ["cancel", "stale_content"])
def test_pressure_does_not_postpone_original_cancel_or_stale_pruning(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    change: str,
) -> None:
    with _held_pool() as held:
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        guard = _ResourceImage("pressure").bind(stems)
        reserve = Mock(wraps=held.pool.try_reserve)
        monkeypatch.setattr(held.pool, "try_reserve", reserve)
        token = StemGenerationInterest("resource-pruned-original")
        owner = stems.request_generation_interest(0, token)
        assert owner is not None
        reserved = stems._assets._reserved
        stems.on_frame_render()
        assert guard.current.state == "pressure"
        cursor = stems._stem_admission._cursor
        if change == "cancel":
            assert stems.cancel_generation_interest(owner, token).disposition == "detached"
            assert not owner.current
        else:
            controller.project.pad_content[0] = PadContentIdentity(instance_id="f" * 32)
        try:
            stems.on_frame_render()
            assert 0 not in stems._requested_generations
            assert not stems.generation_interest_is_pending(owner)
            assert (owner.failure is not None) == (change == "stale_content")
            assert stems.stem_generation_error(0) is None
            assert not stems._jobs
            assert not controller.session.stem_generating_sample_ids
            assert stems._assets._reserved == reserved
            assert stems._stem_admission._cursor == cursor
            reserve.assert_not_called()
            audio_engine_mock.capture_offline_stem_source.assert_not_called()
        finally:
            stems.shut_down()


@pytest.mark.parametrize("state", ["pressure", "unavailable"])
@pytest.mark.parametrize("phase", ["identity", "backend"])
def test_fresh_queued_phase_resource_failure_settles_original_without_reads_or_backend(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    state: str,
    phase: str,
) -> None:
    stems = controller.stems
    resolved, inferred, _prepared = _observe_phase_returns(stems, monkeypatch)
    with _held_pool() as held:
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        image = _ResourceImage()
        image.bind(stems)
        reserved = stems._assets._reserved
        paths = controller.project.sample_paths.copy()
        owner = stems.request_generation_interest(0, StemGenerationInterest("queued-resource"))
        assert owner is not None
        try:
            stems.on_frame_render()
            (job,) = stems._jobs.values()
            source = job.source
            assert isinstance(source, FakeOfflineStemSourceLease)
            if phase == "backend":
                held.release.set()
                assert resolved.wait(5)
                assert job.resolved_request is not None
                assert source.verification_threads
                with _block_existing_workers(held.pool):
                    stems.on_frame_render()
                    assert job.request.cache_dir in stems._inference_jobs
                    assert not inferred.is_set()
                    reads = tuple(source.verification_threads)
                    image.change(state)
                assert inferred.wait(5)
            else:
                assert not resolved.is_set()
                reads = tuple(source.verification_threads)
                assert not reads
                image.change(state)
                held.release.set()
                assert resolved.wait(5)
            assert tuple(source.verification_threads) == reads
            assert not job.running
            assert not job.cancel_event.is_set()
            assert stem_backend.requests == []
            assert owner.failure is None  # The original control collector still owns delivery.
            stems.on_frame_render()
            expected = "resource_unavailable" if state == "unavailable" else "resource_pressure"
            _assert_failed_generation(stems, owner, expected, reserved)
            assert controller.project.sample_paths == paths
            assert stem_backend.requests == []
        finally:
            stems.shut_down()


@pytest.mark.parametrize("state", ["pressure", "unavailable"])
def test_fresh_queued_pair_failure_keeps_readers_until_own_control_collection(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    state: str,
) -> None:
    stems = controller.stems
    _resolved, _inferred, prepared = _observe_phase_returns(stems, monkeypatch)
    with _held_pool() as held:
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        image = _ResourceImage()
        image.bind(stems)
        reserved = stems._assets._reserved
        before = controller.project.stem_cache[0]
        owner = _request_pair(controller, audio_engine_mock)
        try:
            stems.on_frame_render()
            request = stems._pair_preparations.pending[0]
            lease = request.lease
            queued_reserved = stems._assets._reserved
            assert queued_reserved > reserved
            assert not lease.released
            assert not request.started.is_set()
            image.change(state)
            held.release.set()
            assert prepared.wait(5)
            expected = "resource_unavailable" if state == "unavailable" else "resource_pressure"
            assert request.error is not None
            assert expected in request.error
            assert request.prepared is None
            assert not request.cancelled.is_set()
            assert stems._pair_preparations.pending[0] is request
            assert not lease.released
            assert stems._assets._reserved == queued_reserved
            assert owner.failure is None
            audio_engine_mock.prepare_stem_pair.assert_not_called()

            stems._poll_pair_preparations()
            assert not stems._pair_preparations.pending
            assert lease.released
            assert stems._assets._reserved == reserved
            assert owner.failure is not None
            assert expected in owner.failure
            error = stems.stem_generation_error(0)
            assert error is not None
            assert expected in error
            assert controller.project.stem_cache[0] is before
            assert not stems._pending_stem_publications
            assert not stems._disk_selection_settlements
            audio_engine_mock.prepare_stem_pair.assert_not_called()
        finally:
            stems.shut_down()


def test_stem_pressure_does_not_block_arbitrary_targets_on_shared_pool(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    """Copy/migration use this generic runner; their independent bodies must execute."""
    with _held_pool() as held:
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        guard = _ResourceImage("pressure").bind(stems)
        owner = stems.request_generation_interest(0, StemGenerationInterest("pressure-only-stems"))
        assert owner is not None
        stems.on_frame_render()
        assert guard.current.state == "pressure"
        completed = (threading.Event(), threading.Event())
        try:
            for event in completed:
                stems._stem_task_runner(event.set)
            held.release.set()
            assert all(event.wait(5) for event in completed)
            assert stems.generation_interest_is_pending(owner)
            assert not stems._jobs
            audio_engine_mock.capture_offline_stem_source.assert_not_called()
        finally:
            stems.shut_down()
