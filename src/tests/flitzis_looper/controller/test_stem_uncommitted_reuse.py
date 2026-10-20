"""Complete generation custody across real queued/returned Pair and Config owners.

The controller, pool, identity/backend bodies, Pair queue and Config writer are
real. Cached Native sources, Native pair decoding and tiny separator outputs are
explicit fixtures; no Native ACK, actual reader or musical acceptance is claimed.
"""

import hashlib
import json
import time
from contextlib import ExitStack
from dataclasses import dataclass
from pathlib import Path
from threading import Event
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller import stem_provenance
from flitzis_looper.controller import stems as stems_module
from flitzis_looper.controller.stem_job import DIRECT_STEM_INTEREST, StemGenerationInterest
from flitzis_looper.models import PadContentIdentity
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import FakePreparedSourceTicket
from tests.flitzis_looper.controller.test_shared_stem_material import (
    _declared_shared_producer,  # noqa: F401
)
from tests.flitzis_looper.controller.test_stem_admission import _held_pool
from tests.flitzis_looper.controller.test_stem_admission_fanout import (
    _complete_job,
    _reserve_queue,
    _resolved_subscribers,
)
from tests.flitzis_looper.controller.test_stem_pair_preparation import _PreparedPair
from tests.flitzis_looper.controller.test_stem_pending_admission import _production_pool

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_job import StemGenerationJob, StemSubscriber
    from flitzis_looper.controller.stem_pair_preparation import PairPreparation
    from flitzis_looper.controller.stem_workers import StemWorkerReservation
    from flitzis_looper.controller.stems import _StemJobEventDelivery
    from flitzis_looper.models import StemCacheEntry
    from flitzis_looper_audio import PreparedSourceTicket
    from tests.flitzis_looper.conftest import FakeProjectAssetLease, FakeStemGenerationBackend
    from tests.flitzis_looper.controller.test_stem_admission import _HeldPool


class _PairBoundary:
    """Block only the declared Native decoder, preserving the real queue/result owners."""

    def __init__(self, controller: AppController) -> None:
        self.controller = controller
        self.entered = Event()
        self.release = Event()
        self.returned = {slot: Event() for slot in (0, 215)}
        self.prepared: dict[int, _PreparedPair] = {}
        self.failed_result = False

    def prepare(
        self,
        slot: int,
        _version: str,
        cache: str,
        _ticket: PreparedSourceTicket,
        *,
        _components: bool,
        _descriptor: str | None,
        _artifact_version: str | None,
    ) -> _PreparedPair:
        request = self.controller.stems._pair_preparations.pending[slot]
        assert request.entry.cache_dir == cache
        if slot == 0:
            self.entered.set()
            assert self.release.wait(5), "declared Native Pair boundary was never released"
        material = cache.rsplit("/stems/", 1)[0]
        generation = f"{slot + 1:032x}"
        entry = request.entry
        pair = _PreparedPair(
            components=False,
            selection={
                "schema_version": 1,
                "descriptor_reference": f"{material}/.pcm-cache/stems/v1/.pairs/{generation}.json",
                "stem_set_identity": hashlib.sha256(f"declared pair {slot}".encode()).hexdigest(),
                "wav_generation": cache,
                "pcm_generation": f"{material}/.pcm-cache/stems/v1/.ready-{generation}",
                "provenance": {
                    "schema": entry.fingerprint_schema,
                    "generation_fingerprint": entry.generation_fingerprint,
                    "producer_identity": entry.producer_identity,
                },
            },
        )
        # Only structural persistence fixtures, never a claimed Native descriptor.
        descriptor = Path(str(pair.selection["descriptor_reference"]))
        descriptor.parent.mkdir(parents=True, exist_ok=True)
        descriptor.write_text("{}", encoding="utf-8", newline="\n")
        Path(str(pair.selection["pcm_generation"])).mkdir(parents=True, exist_ok=True)
        if slot == 0 and self.failed_result:
            request.error = "declared Native result failed final verification"
            pair.fail_discard = True
        self.prepared[slot] = pair
        return pair


@dataclass(slots=True)
class _Rig:
    app: AppController
    audio: Mock
    backend: FakeStemGenerationBackend
    held: _HeldPool
    boundary: _PairBoundary
    first_job: StemGenerationJob
    first_row: StemSubscriber
    first: PairPreparation
    before: bytes
    reservations: tuple[object, ...]

    def returned(self) -> None:
        self.held.release.set()
        self.boundary.release.set()
        assert self.boundary.returned[0].wait(5)
        assert self.first.prepared is not None


def _readable_sources(app: AppController, audio: Mock, tmp_path: Path) -> None:
    material = tmp_path / "samples" / "Uncommitted.wav"
    material.mkdir()
    source = material / "Uncommitted.wav"
    write_mono_pcm16_wav(source, 44_100)
    (material / ".material.json").write_text(
        json.dumps({
            "schema_version": 1,
            "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "bytes": source.stat().st_size,
            "original_leaf": source.name,
        }),
        encoding="utf-8",
        newline="\n",
    )
    for slot in (0, 215):
        relative = source.relative_to(tmp_path).as_posix()
        app.project.sample_paths[slot] = relative
        audio.load_sample_async(slot, relative)
    tickets = {slot: FakePreparedSourceTicket() for slot in (0, 215)}
    tickets[0].source_request, tickets[215].source_request = 11, 22
    audio.capture_prepared_source.side_effect = lambda slot, _version: tickets[slot]
    app._assets.sync_assignments()


@pytest.fixture
def uncommitted(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> Iterator[_Rig]:
    with _held_pool() as held, ExitStack() as stack:
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        _readable_sources(controller, audio_engine_mock, tmp_path)
        reservations = _reserve_queue(held.pool, stack)
        boundary = _PairBoundary(controller)
        monkeypatch.setattr(
            audio_engine_mock,
            "prepare_stem_pair",
            lambda *args: boundary.prepare(
                *args[:4],
                _components=args[4],
                _descriptor=args[5],
                _artifact_version=args[6],
            ),
        )
        queue = controller.stems._pair_preparations
        run = queue._run

        def observe_return(request: PairPreparation, *, components: bool) -> None:
            try:
                run(request, components=components)
            finally:
                boundary.returned[request.sample_id].set()

        monkeypatch.setattr(queue, "_run", observe_return)
        first_job, tasks = _resolved_subscribers(controller, audio_engine_mock, (0,))
        first_row = first_job.subscribers()[0]
        _complete_job(controller, first_job, tasks)
        controller.stems.on_frame_render()
        reservations[0].close()
        controller.stems.on_frame_render()
        first = queue.pending[0]
        assert str(first_job.phase) == "disposed"
        assert not controller.stems._jobs
        assert not controller.stems._pair_held_generation_events
        assert len(stem_backend.requests) == 1
        entry = controller.project.stem_cache[0]
        assert entry is not None
        assert not entry.available
        assert entry.pair is None
        controller._persistence.flush()
        rig = _Rig(
            controller,
            audio_engine_mock,
            stem_backend,
            held,
            boundary,
            first_job,
            first_row,
            first,
            controller._persistence.config_path.read_bytes(),
            reservations,
        )
        try:
            yield rig
        finally:
            controller.stems.shut_down()
            boundary.release.set()
            held.release.set()
            assert boundary.returned[0].wait(5)


def _second(rig: _Rig) -> StemGenerationJob:
    job, tasks = _resolved_subscribers(rig.app, rig.audio, (215,))
    assert tasks == []
    assert len(rig.backend.requests) == 1
    assert job.resolved_request is not None
    assert job.generation_fingerprint is not None
    return job


def _assert_reuse(rig: _Rig, job: StemGenerationJob) -> _StemJobEventDelivery:
    stems = rig.app.stems
    assert job.request.cache_dir not in stems._waiting_jobs, (
        "equal resolved work started a second inference while complete Pair/Config custody survived"
    )
    delivery = stems._pair_held_generation_events[job, "identity"]
    assert delivery.admission_route == "reuse"
    assert delivery.promotion is not None
    assert delivery.promotion.entry.cache_dir == rig.first.entry.cache_dir
    assert delivery.promotion.lease is not rig.first.lease
    assert not cast("FakeProjectAssetLease", delivery.promotion.lease).released
    (own,) = job.subscribers()
    assert own.sample_id == 215
    assert own.content_id != rig.first.content_id
    assert own.source is not rig.first_row.source
    assert own.interest_owner is not rig.first.interest_owner
    assert not cast("FakeProjectAssetLease", own.source_lease).released
    assert own.retirement.remaining == 16
    with pytest.raises(RuntimeError, match="exact live subscriber"):
        rig.first_job.add_interest(rig.first_row, StemGenerationInterest("consumed terminal"))
    return delivery


def _complete_survivor(rig: _Rig, job: StemGenerationJob) -> None:
    stems = rig.app.stems
    assert rig.first.interest_owner is not None
    stems.cancel_generation_interest(rig.first.interest_owner, DIRECT_STEM_INTEREST)
    rig.app.project.pad_content[0] = PadContentIdentity(instance_id="f" * 32)
    for reservation in rig.reservations:
        cast("StemWorkerReservation", reservation).close()
    rig.held.release.set()
    rig.boundary.release.set()
    deadline = time.monotonic() + 5
    own = job.subscribers()[0]
    while not own.interest_owner.committed:
        stems.on_frame_render()
        assert time.monotonic() < deadline, rig.app.session.stem_generation_errors
        Event().wait(0.001)
    assert rig.boundary.returned[215].wait(5)
    assert len(rig.backend.requests) == 1
    entry = rig.app.project.stem_cache[215]
    assert entry is not None
    assert entry.pair is not None
    assert entry.available
    content = rig.app.project.pad_content[0]
    assert content is not None
    assert content.instance_id == "f" * 32
    assert rig.app._persistence.config_path.read_bytes() != rig.before
    assert not own.interest_owner.failure


@pytest.mark.parametrize("phase", ["queued", "running", "returned"])
def test_equal_resolved_source_reuses_actual_uncommitted_pair_and_outlives_origin(
    uncommitted: _Rig,
    phase: str,
) -> None:
    rig = uncommitted
    if phase == "running":
        rig.held.release.set()
        assert rig.boundary.entered.wait(5)
        assert rig.first.started.is_set()
        assert rig.first.prepared is None
    elif phase == "returned":
        rig.returned()
        rig.app.stems._disk_selection_writer_blocked = lambda: True
        rig.app.stems._poll_pair_preparations()
        assert rig.app.stems._pair_preparations.pending[0] is rig.first
        assert rig.app.stems._pair_preparations._held_results == [rig.first]
        assert rig.first.prepared is not None
    else:
        assert not rig.first.started.is_set()
    assert not cast("FakeProjectAssetLease", rig.first.lease).released
    assert rig.app._persistence.config_path.read_bytes() == rig.before
    job = _second(rig)
    _assert_reuse(rig, job)
    rig.app.stems._disk_selection_writer_blocked = rig.app._persistence.stem_selection_write_blocked
    _complete_survivor(rig, job)


def test_equal_resolution_during_real_config_commit_uses_exact_disk_settlement_owner(
    uncommitted: _Rig,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = uncommitted
    rig.returned()
    stems = rig.app.stems
    commit = rig.app._persistence.commit_stem_selection
    observed: list[StemGenerationJob] = []

    def during_commit(
        slot: int,
        previous: StemCacheEntry | None,
        selected: StemCacheEntry,
        current: Callable[[], bool],
    ) -> None:
        assert slot == 0
        assert 0 not in stems._pair_preparations.pending
        settlement = stems._disk_selection_settlements[0]
        assert settlement.request is rig.first
        assert not cast("FakeProjectAssetLease", settlement.lease).released
        assert rig.app._persistence.config_path.read_bytes() == rig.before
        job = _second(rig)
        _assert_reuse(rig, job)
        observed.append(job)
        commit(slot, previous, selected, current)

    monkeypatch.setattr(stems, "_commit_disk_selection", during_commit)
    stems._poll_pair_preparations()
    assert len(observed) == 1
    assert rig.first.interest_owner is not None
    assert rig.first.interest_owner.committed
    assert rig.app._persistence.config_path.read_bytes() != rig.before
    assert len(rig.backend.requests) == 1


def _other_producer(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    directory = tmp_path / "different-owned-producer"
    directory.mkdir()
    files = tuple(directory / name for name in ("checkpoint.bin", "definition.py", "ffmpeg.exe"))
    for index, path in enumerate(files):
        path.write_bytes(f"different complete producer input {index}".encode())
    monkeypatch.setattr(stems_module, "separator_model_cache_dir", lambda _separator: directory)
    monkeypatch.setattr(stems_module, "resolved_demucs_encoder", lambda _model: files[-1])
    monkeypatch.setattr(stem_provenance, "processing_paths", lambda _request: files)
    monkeypatch.setattr(stem_provenance, "loaded_code_identity", lambda _paths: "different-fixture")


@pytest.mark.parametrize("invalid", ["different_producer", "cancelled", "failed_discard", "closed"])
def test_pending_metadata_without_eligible_complete_owner_cannot_authorize_reuse(
    uncommitted: _Rig,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    invalid: str,
) -> None:
    rig = uncommitted
    if invalid == "different_producer":
        _other_producer(tmp_path, monkeypatch)
    elif invalid == "cancelled":
        rig.first.cancelled.set()
    elif invalid == "failed_discard":
        rig.boundary.failed_result = True
        rig.returned()
        rig.app.stems._poll_pair_preparations()
        assert rig.app.stems._pair_preparations._discard_retries
        assert rig.first.error is not None
        assert cast("FakeProjectAssetLease", rig.first.lease).released
    else:
        # Deliberately retained stale metadata without a physical artifact owner.
        # Cleanup cancels the queued request before its worker may inspect files.
        rig.first.close()
        assert not rig.first.cancelled.is_set()
        assert rig.first.error is None
        assert cast("FakeProjectAssetLease", rig.first.lease).released
    job = _second(rig)
    if invalid == "different_producer":
        assert job.producer_identity != rig.first.entry.producer_identity
    assert job.request.cache_dir in rig.app.stems._waiting_jobs
    assert (job, "identity") not in rig.app.stems._pair_held_generation_events
    assert len(rig.backend.requests) == 1
