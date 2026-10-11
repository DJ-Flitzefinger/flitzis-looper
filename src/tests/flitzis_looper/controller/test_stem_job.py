"""Job phase and independent subscriber custody; no native playback or inference claims."""

from contextlib import nullcontext
from dataclasses import dataclass, replace
from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.stem_generation import AudioShape, StemGenerationRequest
from flitzis_looper.controller.stem_identity import request_fingerprint
from flitzis_looper.controller.stem_intent import StemIntentRegistry
from flitzis_looper.controller.stem_job import StemGenerationJob

if TYPE_CHECKING:
    from contextlib import AbstractContextManager

    from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
    from flitzis_looper_audio import PreparedSourceTicket, SealedProcessingFiles


@dataclass
class _Lease:
    path: Path
    released: bool = False

    def release(self) -> None:
        self.released = True


@dataclass
class _Reservation:
    closed: bool = False

    def close(self) -> None:
        self.closed = True

    def activate(self) -> AbstractContextManager[None]:
        return nullcontext()


class _Assets:
    def __init__(self) -> None:
        self.leases: list[_Lease] = []
        self.reservations: list[_Reservation] = []
        self.retired: list[Path] = []

    def acquire(self, path: Path) -> _Lease:
        lease = _Lease(path)
        self.leases.append(lease)
        return lease

    def reserve(self, _count: int) -> _Reservation:
        reservation = _Reservation()
        self.reservations.append(reservation)
        return reservation

    def retire(self, path: Path, *, recursive: bool, lease: _Lease) -> None:
        assert recursive
        self.retired.append(path)
        lease.release()


def _job(assets: _Assets, sample_id: int = 0) -> StemGenerationJob:
    request = StemGenerationRequest(
        sample_id=sample_id,
        source_path=Path(f"source-{sample_id}.wav"),
        source_version=f"source-{sample_id}.wav|sha256-v1:{'1' * 64}",
        cache_dir=Path(f"private-generation-{sample_id}"),
        target_shape=AudioShape(48_000, 2, 512),
        model_cache_dir=Path(f"model-{sample_id}"),
        original_bytes=2092,
    )
    ticket = cast("PreparedSourceTicket", Mock(name=f"own-ticket-{sample_id}"))
    intent = StemIntentRegistry().capture(request, 0)
    return StemGenerationJob(
        request,
        ticket,
        cast("ProjectAssetLifecycle", assets),
        f"own-content-{sample_id}",
        intent=intent,
    )


def _resolve(job: StemGenerationJob) -> None:
    assert job.begin_resolution() is job.source_ticket
    job.resolved_request = job.request
    job.producer_identity = "2" * 64
    job.generation_fingerprint = request_fingerprint(job.request, job.producer_identity)
    job.processing_files = cast("SealedProcessingFiles", Mock(name="owned-processing-guard"))
    assert job.finish_resolution()


def test_phase_gap_keeps_source_and_guard_until_actual_cancelled_read_end() -> None:
    assets = _Assets()
    job = _job(assets)
    physical_source = assets.leases[0]
    subscriber_source = assets.leases[2]
    assert str(job.phase) == "queued"
    _resolve(job)
    guard = job.processing_files
    assert str(job.phase) == "resolved"
    assert not job.running
    assert not physical_source.released
    assert not subscriber_source.released

    assert job.begin() is job.source_ticket
    assert str(job.phase) == "running"
    job.cancel()
    assert job.running
    assert not physical_source.released
    assert subscriber_source.released
    assert job.processing_files is guard
    assert not job.finish_read()
    assert str(job.phase) == "disposed"
    assert physical_source.released
    assert job.processing_files is None
    assert job.source_ticket is not None
    assert all(item.closed for item in assets.reservations)


def test_cancellation_during_resolution_waits_for_its_actual_read_end() -> None:
    assets = _Assets()
    job = _job(assets)
    physical_source = assets.leases[0]
    assert job.begin_resolution() is job.source_ticket
    guard = cast("SealedProcessingFiles", Mock(name="active-resolution-guard"))
    job.processing_files = guard

    job.cancel()

    assert job.running
    assert not physical_source.released
    assert job.processing_files is guard
    assert not job.finish_resolution()
    assert physical_source.released
    assert job.processing_files is None
    assert job.begin() is None


def test_cancellation_in_the_resolved_waiting_gap_retires_without_starting_inference() -> None:
    assets = _Assets()
    job = _job(assets)
    _resolve(job)

    job.cancel()

    assert job.phase == "disposed"
    assert not job.running
    assert job.begin() is None
    assert job.processing_files is None
    assert all(item.released for item in assets.leases)


def test_resolved_candidate_transfer_keeps_each_own_request_ticket_and_custody() -> None:
    assets = _Assets()
    target = _job(assets)
    candidate = _job(assets, 215)
    _resolve(target)
    _resolve(candidate)
    original = candidate.subscribers()[0]
    candidate_physical_source = assets.leases[3]
    assert target.begin() is target.source_ticket

    transferred = candidate.transfer_subscribers_to(target)

    assert transferred == (original,)
    adopted = next(item for item in target.subscribers() if item.sample_id == 215)
    assert adopted is original
    assert adopted.ticket is candidate.source_ticket
    assert adopted.request is candidate.request
    assert adopted.source_lease is original.source_lease
    assert not cast("_Lease", adopted.source_lease).released
    assert not cast("_Reservation", adopted.retirement).closed
    assert candidate.phase == "disposed"
    assert candidate.processing_files is None
    assert candidate_physical_source.released
    assert target.remove_subscriber(0)
    assert target.running
    assert target.finish_read()
    target.cancel()
    assert cast("_Lease", adopted.source_lease).released
    assert all(item.closed for item in assets.reservations)


def test_cohort_locators_never_substitute_for_resolved_full_content_matching() -> None:
    assets = _Assets()
    job = _job(assets)
    exact = replace(job.request, sample_id=215)
    same_bytes_elsewhere = replace(
        exact,
        source_path=Path("equal-copy.wav"),
        source_version=f"equal-copy.wav|sha256-v1:{'1' * 64}",
        model_cache_dir=Path("equal-model-copy"),
    )
    assert job.cohorts(exact)
    assert not job.cohorts(same_bytes_elsewhere)
    assert not job.matches(exact, "2" * 64)
    _resolve(job)
    assert not job.cohorts(exact)
    assert job.matches(same_bytes_elsewhere, "2" * 64)
    assert not job.matches(exact)
    assert not job.matches(exact, "3" * 64)
    assert not job.matches(replace(exact, original_bytes=2094), "2" * 64)
    job.cancel()


def test_resolved_revision_update_keeps_captured_job_context_and_subscriber_readers() -> None:
    assets = _Assets()
    job = _job(assets)
    subscriber = job.subscribers()[0]
    assert subscriber.intent is not None
    updated = replace(subscriber.intent, material_revision=subscriber.intent.material_revision + 1)

    job.update_intent(0, updated)

    current = job.subscribers()[0]
    assert current.intent is updated
    assert job.intent is subscriber.intent
    assert current.ticket is subscriber.ticket
    assert current.request is subscriber.request
    assert current.source_lease is subscriber.source_lease
    assert current.retirement is subscriber.retirement
    job.cancel()


def test_transfer_rejects_unresolved_or_different_keys_without_releasing_interests() -> None:
    assets = _Assets()
    target = _job(assets)
    candidate = _job(assets, 215)
    with pytest.raises(ValueError, match="full fingerprints"):
        candidate.transfer_subscribers_to(target)
    _resolve(target)
    _resolve(candidate)
    candidate.generation_fingerprint = "4" * 64
    with pytest.raises(ValueError, match="full fingerprints"):
        candidate.transfer_subscribers_to(target)
    assert len(candidate.subscribers()) == 1
    assert len(target.subscribers()) == 1
    target.cancel()
    candidate.cancel()
