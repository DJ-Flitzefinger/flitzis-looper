"""Changing headroom withdraws only inactive common-FIFO metadata waiters."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import stem_admission
from flitzis_looper.controller.stem_pair_preparation import StemPairIntentBinding
from flitzis_looper.controller.stem_resources import StemResourceGuard
from flitzis_looper.controller.stem_workers import StemWorkerPool
from flitzis_looper.models import PadContentIdentity
from flitzis_looper_audio import OfflineWorkTicket

# Pytest discovers the explicit owned producer fixture under its declared name.
from tests.flitzis_looper.controller.test_shared_stem_material import (
    _declared_shared_producer as _declared_shared_producer,  # noqa: PLC0414
)
from tests.flitzis_looper.controller.test_shared_stem_reuse import _known_set
from tests.flitzis_looper.controller.test_stem_admission_fanout import (
    _complete_job,
    _resolved_subscribers,
)
from tests.flitzis_looper.controller.test_stem_admission_retirement import _request_pair
from tests.flitzis_looper.controller.test_stem_heavy_admission_failure import (
    _assert_deferred_failure_custody,
    _Refusal,
    _waiting,
)
from tests.flitzis_looper.controller.test_stem_pending_admission import _production_pool
from tests.flitzis_looper.controller.test_stem_resources import _Clock, _headroom

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_admission import StemHeavyAdmission
    from flitzis_looper.controller.stem_job import StemGenerationJob


def _deny(sample: Mock, clock: _Clock, denial: str) -> None:
    clock.now += 0.25
    if denial == "pressure":
        sample.return_value = _headroom(low_memory=1)
    else:
        sample.side_effect = OSError("fixture memory query unavailable")


def _exercise_fifo(
    controller: AppController,
    heavy: StemHeavyAdmission,
    unchanged: Callable[[], None],
    denial: str,
) -> None:
    stems = controller.stems
    clock = _Clock()
    sample = Mock(return_value=_headroom())
    stems._stem_resources = StemResourceGuard(sample, clock)
    blocker = OfflineWorkTicket()
    successor = follower = None
    try:
        assert blocker.try_acquire()
        stems._poll_stem_admissions()
        assert heavy.registered
        # Construction itself registers in the Native FIFO.
        successor = OfflineWorkTicket()
        assert not successor.try_acquire()
        unchanged()

        _deny(sample, clock, denial)
        stems._poll_stem_admissions()
        assert stems._stem_resources.current.state == denial
        blocker.close()
        # This actual FIFO outcome fails before the controller correction.
        assert successor.try_acquire()
        assert not heavy.registered
        unchanged()

        # Continuing OS pressure leaves the original metadata owner waiting.
        sample.side_effect = None
        sample.return_value = _headroom(low_memory=1)
        clock.now += 0.25
        stems._poll_stem_admissions()
        assert stems._stem_resources.current.state == "pressure"
        assert not heavy.registered
        follower = OfflineWorkTicket()
        assert not follower.try_acquire()
        unchanged()

        # The SAME metadata owner rejoins behind an already waiting successor.
        sample.return_value = _headroom()
        clock.now += 0.25
        stems._poll_stem_admissions()
        assert stems._stem_resources.current.allowed
        assert heavy.registered
        unchanged()
        successor.close()
        assert follower.try_acquire()
        stems._poll_stem_admissions()
        unchanged()
    finally:
        blocker.close()
        if successor is not None:
            successor.close()
        if follower is not None:
            follower.close()


def _assert_job_custody(job: StemGenerationJob) -> None:
    assert job.source is not None
    assert job.processing_files is not None
    job.processing_files.verify()
    assert not job.cancel_event.is_set()
    for row in job.subscribers():
        assert row.interest_owner.failure is None
        assert row.interests
        assert row.source_lease is not None
        assert not row.source_lease.released


@pytest.mark.parametrize("denial", ["pressure", "unavailable"])
def test_resource_ineligible_inference_yields_fifo_without_retiring_original(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    denial: str,
) -> None:
    job = _waiting(controller, audio_engine_mock, tmp_path)
    stems = controller.stems
    pool = StemWorkerPool()
    stems._stem_worker_pool = pool
    rows = job.subscribers()
    original = job.request, job.source, job.processing_files, stems._assets._reserved

    def unchanged() -> None:
        assert stems._jobs[job.request.cache_dir] is job
        assert job.request.cache_dir in stems._waiting_jobs
        assert not stems._inference_jobs
        assert job.subscribers() == rows
        assert (job.request, job.source, job.processing_files, stems._assets._reserved) == original
        assert not stems._pair_preparations.pending
        _assert_job_custody(job)

    try:
        _exercise_fifo(controller, job.heavy_work, unchanged, denial)
    finally:
        stems.shut_down()
        pool._executor.shutdown(wait=True)


def test_resource_denial_keeps_failed_deferred_terminal_custody(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stems = controller.stems
    reserved = stems._assets._reserved
    job = _waiting(controller, audio_engine_mock, tmp_path, shared=True)
    rows = {row.sample_id: row for row in job.subscribers()}
    controller.project.pad_content[0] = PadContentIdentity(instance_id="3" * 32)
    stems._pair_deferred_sample_ids = frozenset({0})
    pool = StemWorkerPool()
    stems._stem_worker_pool = pool
    refusal = _Refusal("acquisition")
    successor = None
    try:
        with monkeypatch.context() as patch:
            patch.setattr(stem_admission, "OfflineWorkTicket", refusal.construct)
            stems.on_frame_render()
        _assert_deferred_failure_custody(controller, job, rows, refusal, reserved)
        successor = OfflineWorkTicket()
        assert not successor.try_acquire()

        # An independent current preparation makes the normal resource poll run.
        version = stems._loaded_source_version_for_pad(215)
        assert version is not None
        entry = stems._eligible_generation_entry(215, version)
        assert entry is not None
        assert stems._prepare_pair(
            215,
            entry,
            stems._capture_source_ticket(215, version),
            binding=StemPairIntentBinding(own_source_version=version),
        )
        assert stems._stem_admission_candidates()
        clock = _Clock()
        sample = Mock(return_value=_headroom())
        stems._stem_resources = StemResourceGuard(sample, clock)
        _deny(sample, clock, "pressure")
        stems._poll_stem_admissions()
        assert stems._stem_resources.current.state == "pressure"
        _assert_deferred_failure_custody(controller, job, rows, refusal, reserved)
        assert not successor.try_acquire()
        stems._pair_deferred_sample_ids = frozenset()
        stems.on_frame_render()
        assert refusal.closed == 1
        assert successor.try_acquire()
    finally:
        stems._pair_deferred_sample_ids = frozenset()
        if successor is not None:
            successor.close()
        stems.shut_down()
        pool._executor.shutdown(wait=True)


@pytest.mark.parametrize("denial", ["pressure", "unavailable"])
def test_resource_ineligible_requested_pair_yields_fifo_without_new_readers(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    denial: str,
) -> None:
    pool = StemWorkerPool()
    _production_pool(controller, audio_engine_mock, tmp_path, pool)
    stems = controller.stems
    owner = _request_pair(controller, audio_engine_mock)
    requested = stems._requested_preparations[0]
    reserved = stems._assets._reserved
    audio_engine_mock.acquire_project_asset_lease.reset_mock()

    def unchanged() -> None:
        assert stems._requested_preparations[0] is requested
        assert stems.generation_interest_is_pending(owner)
        assert owner.failure is None
        assert not stems._pair_preparations.pending
        assert stems._assets._reserved == reserved
        audio_engine_mock.prepare_stem_pair.assert_not_called()
        audio_engine_mock.acquire_project_asset_lease.assert_not_called()

    try:
        _exercise_fifo(controller, requested.heavy_work, unchanged, denial)
    finally:
        stems.shut_down()
        pool._executor.shutdown(wait=True)


@pytest.mark.parametrize("denial", ["pressure", "unavailable"])
def test_resource_ineligible_reuse_yields_fifo_without_consuming_held_delivery(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    denial: str,
) -> None:
    _known_set(controller, audio_engine_mock, tmp_path, monkeypatch)
    pool = StemWorkerPool()
    stems = controller.stems
    stems._stem_worker_pool = pool
    job, tasks = _resolved_subscribers(controller, audio_engine_mock, (0,), reuse_only=True)
    delivery = stems._pair_held_generation_events[job, "identity"]
    rows = delivery.remaining
    original = job.source, job.processing_files, stems._assets._reserved

    def unchanged() -> None:
        assert stems._pair_held_generation_events[job, "identity"] is delivery
        assert delivery.admission_route == "reuse"
        assert delivery.remaining == rows
        assert (job.source, job.processing_files, stems._assets._reserved) == original
        assert not tasks
        assert not stems._pair_preparations.pending
        audio_engine_mock.prepare_stem_pair.assert_not_called()
        _assert_job_custody(job)

    try:
        _exercise_fifo(controller, job.heavy_work, unchanged, denial)
    finally:
        stems.shut_down()
        pool._executor.shutdown(wait=True)


@pytest.mark.parametrize("denial", ["pressure", "unavailable"])
def test_resource_denial_keeps_active_completed_generation_and_borrow(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    denial: str,
) -> None:
    _production_pool(controller, audio_engine_mock, tmp_path, StemWorkerPool())
    stems = controller.stems
    pool = stems._stem_worker_pool
    assert pool is not None
    job, tasks = _resolved_subscribers(controller, audio_engine_mock, (0,))
    _complete_job(controller, job, tasks)
    stems._poll_generation_events()
    delivery = stems._pair_held_generation_events[job, "terminal"]
    rows = delivery.remaining
    original = job.source, job.processing_files, stems._assets._reserved
    borrow = job.heavy_work.borrow()
    successor = OfflineWorkTicket()
    clock = _Clock()
    sample = Mock(return_value=_headroom())
    stems._stem_resources = StemResourceGuard(sample, clock)
    try:
        assert not successor.try_acquire()
        _deny(sample, clock, denial)
        stems._poll_stem_admissions()
        assert stems._stem_resources.current.state == denial
        assert job.heavy_work.registered
        assert delivery.remaining == rows
        # Backend finish_read already ended its source read before this handoff.
        assert job.source is None
        assert (job.source, job.processing_files, stems._assets._reserved) == original
        assert job.processing_files is not None
        job.processing_files.verify()
        assert not job.cancel_event.is_set()
        assert not successor.try_acquire()
        stems.shut_down()
        assert not successor.try_acquire()  # Actual independent borrow still owns capacity.
        borrow.close()
        assert successor.try_acquire()
    finally:
        borrow.close()
        successor.close()
        stems.shut_down()
        pool._executor.shutdown(wait=True)
