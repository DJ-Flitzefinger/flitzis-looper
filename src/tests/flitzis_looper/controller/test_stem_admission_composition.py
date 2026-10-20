"""Real shared-pool phase handoff; only device and separator work are substituted."""

import threading
from contextlib import contextmanager
from functools import partial
from typing import TYPE_CHECKING
from unittest.mock import Mock

from flitzis_looper.controller.stem_job import StemGenerationInterest
from flitzis_looper.controller.stem_workers import STEM_QUEUED_JOBS, STEM_WORKERS

# Pytest discovers this imported autouse fixture under its declared name.
from tests.flitzis_looper.controller.test_shared_stem_material import (
    _declared_shared_producer as _declared_shared_producer,  # noqa: PLC0414
)
from tests.flitzis_looper.controller.test_stem_admission import _held_pool
from tests.flitzis_looper.controller.test_stem_pair_preparation import _PreparedPair
from tests.flitzis_looper.controller.test_stem_pending_admission import _production_pool

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path

    import pytest

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_intent import StemGenerationIntent
    from flitzis_looper.controller.stem_job import StemGenerationInterests, StemGenerationJob
    from flitzis_looper.controller.stem_pair_preparation import PairPreparation
    from flitzis_looper.controller.stem_workers import StemWorkerPool
    from flitzis_looper.controller.stems import StemController
    from flitzis_looper_audio import OfflineStemSourceLease
    from tests.flitzis_looper.conftest import FakeStemGenerationBackend


@contextmanager
def _saturated(pool: StemWorkerPool) -> Iterator[None]:
    """Occupy the same existing pool until both actual workers and its queue are full."""
    release = threading.Event()
    started = tuple(threading.Event() for _ in range(STEM_WORKERS))
    finished = tuple(threading.Event() for _ in range(STEM_WORKERS))
    queued: list[threading.Event] = []
    admitted_workers = 0

    def hold(index: int) -> None:
        started[index].set()
        try:
            assert release.wait(10)
        finally:
            finished[index].set()

    try:
        for index in range(STEM_WORKERS):
            pool(partial(hold, index))
            admitted_workers += 1
        assert all(event.wait(5) for event in started)
        for _ in range(STEM_QUEUED_JOBS):
            completed = threading.Event()
            pool(completed.set)
            queued.append(completed)
        assert pool.try_reserve() is None
        yield
    finally:
        release.set()
        assert all(event.wait(5) for event in finished[:admitted_workers])
        assert all(event.wait(5) for event in queued)


def _observe_phase_returns(
    stems: StemController, monkeypatch: pytest.MonkeyPatch
) -> tuple[threading.Event, threading.Event, threading.Event]:
    resolved, inferred, prepared = threading.Event(), threading.Event(), threading.Event()
    resolve = stems._resolve_stem_identity
    infer = stems._run_stem_backend
    prepare = stems._pair_preparations._run

    def resolve_and_signal(job: StemGenerationJob) -> None:
        try:
            resolve(job)
        finally:
            resolved.set()

    def infer_and_signal(job: StemGenerationJob) -> None:
        try:
            infer(job)
        finally:
            inferred.set()

    def prepare_and_signal(request: PairPreparation, *, components: bool) -> None:
        try:
            prepare(request, components=components)
        finally:
            prepared.set()

    monkeypatch.setattr(stems, "_resolve_stem_identity", resolve_and_signal)
    monkeypatch.setattr(stems, "_run_stem_backend", infer_and_signal)
    monkeypatch.setattr(stems._pair_preparations, "_run", prepare_and_signal)
    return resolved, inferred, prepared


def _assert_inference_handoff(
    stems: StemController,
    audio: Mock,
    backend: FakeStemGenerationBackend,
    pool: StemWorkerPool,
    job: StemGenerationJob,
    owner: StemGenerationInterests,
    original_source: OfflineStemSourceLease | None,
    inferred: threading.Event,
) -> StemGenerationIntent | None:
    with _saturated(pool):
        stems.on_frame_render()
        (row,) = job.subscribers()
        assert row.interest_owner is owner
        assert row.source is original_source
        assert row.request is job.request
        bound_intent = row.intent
        assert stems.generation_interest_is_pending(owner)
        assert owner.failure is None
        assert stems.stem_generation_error(0) is None
        assert not inferred.is_set()
        assert backend.requests == []
        audio.prepare_stem_pair.assert_not_called()

    stems.on_frame_render()
    assert inferred.wait(5)
    assert len(backend.requests) == 1
    assert backend.requests[0] is job.resolved_request
    return bound_intent


def _assert_preparation_handoff(
    stems: StemController,
    audio: Mock,
    backend: FakeStemGenerationBackend,
    pool: StemWorkerPool,
    job: StemGenerationJob,
    owner: StemGenerationInterests,
    original_source: OfflineStemSourceLease | None,
    bound_intent: StemGenerationIntent | None,
    prepared: threading.Event,
) -> None:
    with _saturated(pool):
        stems.on_frame_render()
        assert stems.generation_interest_is_pending(owner)
        assert owner.failure is None
        assert stems.stem_generation_error(0) is None
        assert not stems._pair_preparations.pending
        assert not prepared.is_set()
        audio.prepare_stem_pair.assert_not_called()

    stems.on_frame_render()
    assert prepared.wait(5)
    request = stems._pair_preparations.pending[0]
    assert request.interest_owner is owner
    assert request.generation_intent is bound_intent
    assert request.own_source_version == job.request.source_version
    audio.prepare_stem_pair.assert_called_once()
    assert request.ticket is audio.prepare_stem_pair.call_args.args[3]
    audio.capture_prepared_stem_source.assert_called_once_with(0, original_source)
    assert len(backend.requests) == 1
    assert stems.stem_generation_error(0) is None


def test_original_holder_survives_full_pool_at_inference_and_pair_preparation_handoffs(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stems = controller.stems
    resolved, inferred, prepared = _observe_phase_returns(stems, monkeypatch)
    monkeypatch.setattr(
        audio_engine_mock, "prepare_stem_pair", Mock(return_value=_PreparedPair()), raising=False
    )

    with _held_pool() as held:
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        try:
            token = StemGenerationInterest("same-owner-through-all-phases")
            owner = stems.request_generation_interest(0, token)
            assert owner is not None
            requested = stems._requested_generations[0]
            assert requested.owner is owner
            assert not stems._jobs
            stems.on_frame_render()
            (job,) = stems._jobs.values()
            assert job.request is requested.request
            original_source = job.source
            held.release.set()
            assert resolved.wait(5)
            assert job.resolved_request is not None
            assert job.generation_fingerprint is not None

            bound_intent = _assert_inference_handoff(
                stems,
                audio_engine_mock,
                stem_backend,
                held.pool,
                job,
                owner,
                original_source,
                inferred,
            )
            _assert_preparation_handoff(
                stems,
                audio_engine_mock,
                stem_backend,
                held.pool,
                job,
                owner,
                original_source,
                bound_intent,
                prepared,
            )
        finally:
            stems.shut_down()
