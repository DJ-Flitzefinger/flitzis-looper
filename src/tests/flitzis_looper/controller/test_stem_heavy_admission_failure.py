"""Real controller failure delivery with documented Native admission refusals."""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller import stem_admission
from flitzis_looper.controller.stem_workers import STEM_QUEUED_JOBS, STEM_WORKERS, StemWorkerPool
from flitzis_looper.models import PadContentIdentity
from flitzis_looper_audio import OfflineWorkTicket

# Pytest discovers the explicit owned producer fixture under its declared name.
from tests.flitzis_looper.controller.test_shared_stem_material import (
    _declared_shared_producer as _declared_shared_producer,  # noqa: PLC0414
)
from tests.flitzis_looper.controller.test_shared_stem_material import _shared

if TYPE_CHECKING:
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_job import StemGenerationJob, StemSubscriber
    from tests.flitzis_looper.conftest import FakeStemGenerationBackend


class _Refusal:
    def __init__(self, phase: str) -> None:
        self.phase = phase
        self.calls = 0
        self.closed = 0
        self.ticket: OfflineWorkTicket | None = None

    def construct(self) -> _Refusal:
        if self.phase == "constructor":
            self.calls += 1
            message = "fixture Native heavy constructor refused"
            raise RuntimeError(message)
        self.ticket = OfflineWorkTicket()
        return self

    def try_acquire(self) -> bool:
        self.calls += 1
        message = "fixture Native heavy acquisition refused"
        raise RuntimeError(message)

    @property
    def active(self) -> bool:
        return self.ticket is not None and self.ticket.active

    def close(self) -> None:
        self.closed += 1
        assert self.ticket is not None
        self.ticket.close()


def _waiting(
    controller: AppController, audio: Mock, tmp_path: Path, *, shared: bool = False
) -> StemGenerationJob:
    tasks, _ = _shared(controller, audio, tmp_path)
    stems = controller.stems
    controller.session.active_sample_ids.add(0)
    assert stems.generate_stems_async(0)
    if shared:
        assert stems.generate_stems_async(215)
    while tasks:
        tasks.pop(0)()
    stems.on_frame_render()
    assert not tasks
    (job,) = stems._jobs.values()
    assert job.resolved_request is not None
    assert job.processing_files is not None
    assert job.request.cache_dir in stems._waiting_jobs
    assert not job.heavy_work.registered
    controller.session.active_sample_ids.clear()
    return job


def _all_permits_returned(pool: StemWorkerPool) -> None:
    reservations = []
    try:
        for _ in range(STEM_WORKERS + STEM_QUEUED_JOBS):
            reservation = pool.try_reserve()
            assert reservation is not None
            reservations.append(reservation)
        assert pool.try_reserve() is None
    finally:
        for reservation in reservations:
            reservation.close()


@pytest.mark.parametrize("phase", ["constructor", "acquisition"])
@pytest.mark.parametrize("production", [False, True])
def test_waiting_inference_refusal_settles_original_without_backend_or_poll_exception(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    phase: str,
    *,
    production: bool,
) -> None:
    stems = controller.stems
    reserved = stems._assets._reserved
    job = _waiting(controller, audio_engine_mock, tmp_path)
    (subscriber,) = job.subscribers()
    before = controller.project.stem_cache[0]
    pool = StemWorkerPool()
    refusal = _Refusal(phase)
    successor = None
    try:
        if production:
            stems._stem_worker_pool = pool
            stems._stem_task_runner = pool
        with monkeypatch.context() as patch:
            patch.setattr(stem_admission, "OfflineWorkTicket", refusal.construct)
            stems.on_frame_render()
            stems.on_frame_render()
        assert refusal.calls == 1
        assert subscriber.interest_owner.failure is not None
        assert "fixture Native heavy" in subscriber.interest_owner.failure
        assert "fixture Native heavy" in (stems.stem_generation_error(0) or "")
        assert not stems.generation_interest_is_pending(subscriber.interest_owner)
        assert not stems._jobs
        assert not stems._waiting_jobs
        assert not stems._inference_jobs
        assert not stems._generation_sources
        assert not stems._generation_intents
        assert not controller.session.stem_generating_sample_ids
        assert stems._assets._reserved == reserved
        assert controller.project.stem_cache[0] is before
        assert not job.cancel_event.is_set()
        assert job.source is job.processing_files is None
        assert not job.heavy_work.registered
        assert refusal.closed == (phase == "acquisition")
        assert not stem_backend.requests
        _all_permits_returned(pool)
        successor = OfflineWorkTicket()
        assert successor.try_acquire()
    finally:
        if successor is not None:
            successor.close()
        stems.shut_down()
        pool._executor.shutdown(wait=True)


def _assert_deferred_failure_custody(
    controller: AppController,
    job: StemGenerationJob,
    rows: dict[int, StemSubscriber],
    refusal: _Refusal,
    reserved: int,
) -> None:
    stems = controller.stems
    assert refusal.calls == 1
    assert rows[215].interest_owner.failure is not None
    assert stems.stem_generation_error(215) is not None
    assert rows[0].interest_owner.failure is None
    assert stems.stem_generation_error(0) is None
    assert job.request.cache_dir not in stems._waiting_jobs
    assert stems._jobs[job.request.cache_dir] is job
    assert job.source is not None
    assert job.processing_files is not None
    assert job.heavy_work.registered
    assert refusal.closed == 0
    assert stems._assets._reserved > reserved


def test_shared_refusal_keeps_stale_deferred_custody_until_original_terminal_delivery(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stems = controller.stems
    reserved = stems._assets._reserved
    job = _waiting(controller, audio_engine_mock, tmp_path, shared=True)
    rows = {row.sample_id: row for row in job.subscribers()}
    assert set(rows) == {0, 215}
    before = controller.project.stem_cache[0]
    controller.project.pad_content[0] = PadContentIdentity(instance_id="3" * 32)
    stems._pair_deferred_sample_ids = frozenset({0})
    pool = StemWorkerPool()
    stems._stem_worker_pool = pool
    stems._stem_task_runner = pool
    refusal = _Refusal("acquisition")
    successor = None
    try:
        with monkeypatch.context() as patch:
            patch.setattr(stem_admission, "OfflineWorkTicket", refusal.construct)
            stems.on_frame_render()
            _assert_deferred_failure_custody(controller, job, rows, refusal, reserved)
            successor = OfflineWorkTicket()
            assert not successor.try_acquire()
            stems.on_frame_render()
            assert refusal.calls == 1
            assert not successor.try_acquire()
            stems._pair_deferred_sample_ids = frozenset()
            stems.on_frame_render()
        assert not stems._jobs
        assert not stems._pair_held_generation_events
        assert stems._assets._reserved == reserved
        assert job.source is job.processing_files is None
        assert not job.heavy_work.registered
        assert refusal.closed == 1
        assert successor.try_acquire()
        assert controller.project.stem_cache[0] is before
        assert rows[0].interest_owner.failure is None
        assert stems.stem_generation_error(0) is None
        assert not stem_backend.requests
        assert not job.cancel_event.is_set()
        _all_permits_returned(pool)
    finally:
        stems._pair_deferred_sample_ids = frozenset()
        if successor is not None:
            successor.close()
        stems.shut_down()
        pool._executor.shutdown(wait=True)


def test_legacy_failure_delivery_schedules_other_waiting_owner_exactly_once(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tasks, _ = _shared(controller, audio_engine_mock, tmp_path)
    other = tmp_path / "samples" / "other.wav"
    # Different complete original bytes prevent full-fingerprint coalescing.
    other.write_bytes((tmp_path / "samples" / "shared.wav").read_bytes() + b"x")
    controller.project.sample_paths[215] = "samples/other.wav"
    audio_engine_mock.load_sample_async(215, "samples/other.wav")
    controller.session.active_sample_ids.update({0, 215})
    stems = controller.stems
    assert stems.generate_stems_async(0)
    assert stems.generate_stems_async(215)
    while tasks:
        tasks.pop(0)()
    stems.on_frame_render()
    assert len(stems._waiting_jobs) == len(stems._jobs) == 2
    first, second = (stems._jobs[path] for path in stems._waiting_jobs)
    (failed,) = first.subscribers()
    (surviving,) = second.subscribers()
    constructions = []

    def refuse_first() -> OfflineWorkTicket:
        constructions.append(True)
        if len(constructions) == 1:
            message = "fixture first waiting admission refused"
            raise RuntimeError(message)
        return OfflineWorkTicket()

    controller.session.active_sample_ids.clear()
    try:
        with monkeypatch.context() as patch:
            patch.setattr(stem_admission, "OfflineWorkTicket", refuse_first)
            stems.on_frame_render()
        assert len(constructions) == 2
        assert len(tasks) == 1
        assert failed.interest_owner.failure is not None
        assert surviving.interest_owner.failure is None
        assert stems._inference_jobs == {second.request.cache_dir}
        assert not stems._waiting_jobs
        assert first.request.cache_dir not in stems._jobs
        assert not stem_backend.requests
        tasks.pop(0)()
        assert len(stem_backend.requests) == 1
        assert stem_backend.requests[0] is second.resolved_request
        stems.on_frame_render()
        assert stems.stem_generation_error(surviving.sample_id) is None
    finally:
        stems.shut_down()
        tasks.clear()
