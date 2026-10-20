"""Real worker events and artifact promotion; Native publication remains a control fake."""

import hashlib
from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import stems
from flitzis_looper.controller.stem_cache import expected_stem_files, promote_generation_artifacts
from flitzis_looper.controller.stem_generation import AudioShape, StemGenerationRequest
from flitzis_looper.controller.stem_job import StemGenerationJob
from flitzis_looper.controller.stems import StemController
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState, StemCacheEntry
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import (
    FakeLoadedSourceDigests,
    FakeOfflineStemSourceLease,
    FakePreparedSourceTicket,
    FakeStemGenerationBackend,
)
from tests.flitzis_looper.controller.test_pair_stem_mirror import _prepare, _project_after
from tests.flitzis_looper.controller.test_stem_job import _Assets, _Lease, _Reservation

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
    from flitzis_looper.controller.pair_stem_mirror import PreparedPairStemMirror
    from flitzis_looper.controller.stem_generation import StemGenerationResult, StemProgressCallback
    from flitzis_looper.controller.stem_intent import StemGenerationIntent
    from flitzis_looper.controller.stem_job import StemGenerationInterests, StemSubscriber
    from flitzis_looper_audio import (
        AudioEngine,
        OfflineStemSourceLease,
        PreparedSourceTicket,
        PreparedStemPair,
    )


@dataclass
class _ObserverRig:
    controller: StemController
    assets: _Assets
    audio: Mock
    backend: FakeStemGenerationBackend
    loaded: FakeLoadedSourceDigests
    placements: dict[int, FakeOfflineStemSourceLease] = field(default_factory=dict)
    queued: list[tuple[int, StemCacheEntry, PreparedSourceTicket]] = field(default_factory=list)
    accepts: set[int] = field(default_factory=lambda: {0, 36, 215})


def _rig(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> _ObserverRig:
    monkeypatch.chdir(tmp_path)
    source = tmp_path / "samples" / "shared.wav"
    source.parent.mkdir()
    write_mono_pcm16_wav(source, 44_100)
    project = ProjectState()
    for sample_id, content_id in ((0, "a" * 32), (36, "c" * 32)):
        project.sample_paths[sample_id] = "samples/shared.wav"
        project.pad_content[sample_id] = PadContentIdentity(instance_id=content_id)
    audio = Mock(
        spec=[
            "loaded_source_digest",
            "offline_stem_source_is_current",
            "capture_prepared_stem_source",
        ]
    )
    assets = _Assets()
    backend = FakeStemGenerationBackend()
    controller = StemController(
        project,
        SessionState(),
        cast("AudioEngine", audio),
        stem_backend=backend,
        stem_task_runner=lambda _task: None,
        asset_lifecycle=cast("ProjectAssetLifecycle", assets),
    )
    rig = _ObserverRig(controller, assets, audio, backend, FakeLoadedSourceDigests())
    audio.loaded_source_digest.side_effect = lambda sample_id: (
        rig.placements[sample_id].digest if sample_id in rig.placements else None
    )
    audio.offline_stem_source_is_current.side_effect = lambda sample_id, source: (
        rig.placements.get(sample_id) is source
    )

    def capture(sample_id: int, source: OfflineStemSourceLease) -> FakePreparedSourceTicket:
        assert cast("OfflineStemSourceLease", rig.placements[sample_id]) is source
        return FakePreparedSourceTicket("accepted")

    def queue(
        sample_id: int,
        entry: StemCacheEntry,
        ticket: PreparedSourceTicket,
        **_options: object,
    ) -> bool:
        rig.queued.append((sample_id, entry, ticket))
        if sample_id in rig.accepts:
            project.stem_cache[sample_id] = entry
            return True
        return False

    audio.capture_prepared_stem_source.side_effect = capture
    monkeypatch.setattr(controller, "_queue_stem_publication", queue)
    return rig


def _subscribe(rig: _ObserverRig, job: StemGenerationJob, sample_id: int) -> None:
    project = rig.controller._project
    path = project.sample_paths[sample_id]
    content = project.pad_content[sample_id]
    assert path is not None
    assert content is not None
    rig.loaded.load(sample_id, path)
    digest = rig.loaded.get(sample_id)
    assert digest is not None
    version = f"{path}|sha256-v1:{digest}"
    source = rig.loaded.capture(sample_id, version)
    request = replace(job.request, sample_id=sample_id, source_version=version)
    intent = rig.controller._intents.capture(request, project.stem_processing_revision)
    job.add_subscriber(
        sample_id,
        cast("OfflineStemSourceLease", source),
        content.instance_id,
        request=request,
        intent=intent,
    )
    rig.placements[sample_id] = source
    rig.controller._generation_sources[sample_id] = cast("OfflineStemSourceLease", source)
    rig.controller._generation_intents[sample_id] = intent
    rig.controller._session.stem_generating_sample_ids.add(sample_id)
    rig.controller._session.stem_generation_source_versions[sample_id] = version
    rig.controller._session.stem_generation_progress[sample_id] = 17.0


def _job(rig: _ObserverRig, sample_id: int = 0) -> StemGenerationJob:
    project = rig.controller._project
    path = project.sample_paths[sample_id]
    content = project.pad_content[sample_id]
    assert path is not None
    assert content is not None
    rig.loaded.load(sample_id, path)
    digest = rig.loaded.get(sample_id)
    assert digest is not None
    version = f"{path}|sha256-v1:{digest}"
    source = rig.loaded.capture(sample_id, version)
    request = StemGenerationRequest(
        sample_id,
        Path(path).absolute(),
        version,
        Path(f"samples/stems/#{sample_id + 1}/.generation-{'1' * 32}").absolute(),
        AudioShape(*source.target_shape),
        Path("owned-model").absolute(),
        original_bytes=source.original_bytes,
    )
    intent = rig.controller._intents.capture(request, project.stem_processing_revision)
    job = StemGenerationJob(
        request,
        cast("OfflineStemSourceLease", source),
        cast("ProjectAssetLifecycle", rig.assets),
        content.instance_id,
        intent=intent,
    )
    job.resolved_request = request
    rig.controller._jobs[request.cache_dir] = job
    rig.controller._inference_jobs.add(request.cache_dir)
    rig.placements[sample_id] = source
    rig.controller._generation_sources[sample_id] = cast("OfflineStemSourceLease", source)
    rig.controller._generation_intents[sample_id] = intent
    rig.controller._session.stem_generating_sample_ids.add(sample_id)
    rig.controller._session.stem_generation_source_versions[sample_id] = version
    rig.controller._session.stem_generation_progress[sample_id] = 17.0
    return job


def _move(rig: _ObserverRig, mirror: PreparedPairStemMirror, acknowledgement: object) -> None:
    _project_after(rig.controller, mirror)
    mirror.apply(acknowledgement)
    rig.placements[215] = rig.placements.pop(0)


def _assert_completed_shared_job(
    rig: _ObserverRig,
    original_request: StemGenerationRequest,
    original: StemSubscriber,
    promotion: stems._PromotedStemGeneration,
) -> None:
    assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [36, 215]
    assert len(rig.backend.requests) == 1
    assert rig.backend.requests[0] is original_request
    assert [call.args[0] for call in rig.audio.capture_prepared_stem_source.call_args_list] == [
        36,
        215,
    ]
    assert rig.queued[0][1].cache_dir == rig.queued[1][1].cache_dir
    assert rig.queued[0][1] is not rig.queued[1][1]
    assert cast("_Lease", original.source_lease).released
    assert cast("_Reservation", original.retirement).closed
    assert cast("_Lease", promotion.lease).released
    assert promotion.released
    assert rig.assets.retired == [original_request.cache_dir]
    assert not rig.controller._jobs
    assert not rig.controller._pair_held_generation_events


def _assert_moved_interest(job: StemGenerationJob, original: StemSubscriber) -> None:
    moved = next(subscriber for subscriber in job.subscribers() if subscriber.sample_id == 215)
    assert moved is not original
    assert moved.source is original.source
    assert moved.source_lease is original.source_lease
    assert moved.retirement is original.retirement
    assert moved.intent is original.intent


def _resolve_identity(
    rig: _ObserverRig, job: StemGenerationJob, monkeypatch: pytest.MonkeyPatch
) -> Mock:
    """Run the actual identity worker with declared control-only producer custody."""
    rig.controller._inference_jobs.discard(job.request.cache_dir)
    guard = Mock(name="owned producer files")
    monkeypatch.setattr(rig.controller, "_resolved_processing_request", lambda request: request)
    monkeypatch.setattr(stems, "capture_processing_files", lambda _request: (guard, "2" * 64))
    monkeypatch.setattr(stems, "processing_source_roots", lambda _request, _guard: ())
    rig.backend.result = replace(
        rig.backend.result,
        backend_name="demucs",
        model_name="htdemucs",
        encoder_verified=True,
    )
    rig.controller._resolve_stem_identity(job)
    assert job.phase == "resolved"
    assert job.generation_fingerprint is not None
    assert job.processing_files is guard
    assert job.source is not None
    return guard


def _completed_entry(rig: _ObserverRig, job: StemGenerationJob) -> StemCacheEntry:
    resolved = job.resolved_request
    assert resolved is not None
    request = replace(
        resolved,
        cache_dir=Path(f"samples/stems/#37/.generation-{'2' * 32}").absolute(),
    )
    rig.backend.generate(request, lambda _percent, _stage: None)
    root = request.cache_dir.parent.relative_to(Path.cwd()).as_posix()
    entry = StemCacheEntry(
        source_version=request.source_version,
        cache_dir=root,
        stems=expected_stem_files(root),
        available=True,
        fingerprint_schema="effective-stem-v3",
        generation_fingerprint=job.generation_fingerprint,
        producer_identity=job.producer_identity,
    )
    return promote_generation_artifacts(entry, request.cache_dir)


def test_progress_backlog_coalesces_without_blocking_unrelated_job_terminal(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    job = _job(rig)
    unrelated = _job(rig, 36)
    mirror, acknowledgement = _prepare(rig.controller)
    rig.controller._pair_deferred_sample_ids = frozenset((0, 215))
    for number in range(256):
        rig.controller._put_generation_event(
            stems._StemBackendEvent(
                0,
                job.request.source_version,
                job.source,
                job.request.cache_dir,
                "progress",
                percent=float(number),
                stage="Actual continuing worker",
            )
        )
    rig.controller._put_generation_event(
        stems._StemBackendEvent(
            0,
            job.request.source_version,
            job.source,
            job.request.cache_dir,
            "progress",
            stage="Latest participant stage",
        )
    )
    rig.controller._run_stem_backend(unrelated)
    rig.controller._poll_generation_events()

    assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [36]
    assert 36 not in rig.controller._session.stem_generating_sample_ids
    assert rig.controller._jobs == {job.request.cache_dir: job}
    assert len(rig.controller._pair_held_generation_events) == 1
    held = rig.controller._pair_held_generation_events[job, "progress"]
    assert held.event.percent == 255.0
    assert held.event.stage == "Latest participant stage"
    assert rig.controller._session.stem_generation_progress[0] == 17.0
    assert rig.controller._stem_generation_events.empty()
    _move(rig, mirror, acknowledgement)
    rig.controller._pair_deferred_sample_ids = frozenset()
    rig.controller._poll_generation_events()
    assert rig.controller._session.stem_generation_progress[215] == 255.0
    assert rig.controller._session.stem_generation_stage[215] == "Latest participant stage"
    assert not rig.controller._pair_held_generation_events
    job.cancel()


@pytest.mark.parametrize("mirror_first", [False, True])
@pytest.mark.parametrize("unrelated_queued", [False, True])
def test_shared_success_retains_one_promotion_until_remaining_moved_interest_is_delivered(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    mirror_first: bool,
    unrelated_queued: bool,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    job = _job(rig)
    _subscribe(rig, job, 36)
    original = job.subscribers()[0]
    original_request = job.request
    physical_source = job.source
    mirror, acknowledgement = _prepare(rig.controller)
    rig.controller._pair_deferred_sample_ids = frozenset((0, 215))
    if not unrelated_queued:
        rig.accepts.remove(36)
    if mirror_first:
        _move(rig, mirror, acknowledgement)
    promote = promote_generation_artifacts
    promotions: list[Path] = []

    def promote_once(entry: StemCacheEntry, directory: Path) -> StemCacheEntry:
        promotions.append(directory)
        return promote(entry, directory)

    monkeypatch.setattr(stems, "promote_generation_artifacts", promote_once)
    rig.controller._run_stem_backend(job)
    rig.controller._poll_generation_events()
    held = rig.controller._pair_held_generation_events[job, "terminal"]
    terminal = held.event
    promotion = held.promotion
    assert promotion is not None
    assert terminal.source_lease is physical_source
    assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [36]
    assert job.request is original_request
    assert job.phase == "settled"
    assert rig.controller._jobs[job.request.cache_dir] is job
    assert not cast("_Lease", original.source_lease).released
    assert not cast("_Reservation", original.retirement).closed
    assert not cast("_Lease", promotion.lease).released
    marker = Path(promotion.entry.cache_dir) / ".complete.json"
    marker_digest = hashlib.sha256(marker.read_bytes()).hexdigest()
    if not mirror_first:
        _move(rig, mirror, acknowledgement)
    _assert_moved_interest(job, original)
    rig.controller._put_generation_event(terminal)
    rig.controller._poll_generation_events()
    assert rig.controller._pair_held_generation_events[job, "terminal"] is held
    assert rig.queued[0][0] == 36
    assert len(rig.queued) == 1
    rig.controller._pair_deferred_sample_ids = frozenset()
    rig.controller._poll_generation_events()
    rig.controller._put_generation_event(terminal)
    rig.controller._poll_generation_events()

    assert promotions == [original_request.cache_dir]
    assert hashlib.sha256(marker.read_bytes()).hexdigest() == marker_digest
    _assert_completed_shared_job(rig, original_request, original, promotion)


@pytest.mark.parametrize("terminal_kind", ["error", "settled"])
def test_shared_failure_retains_exact_terminal_owner_and_delivers_remaining_after_mirror(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    terminal_kind: str,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    job = _job(rig)
    _subscribe(rig, job, 36)
    original = job.subscribers()[0]
    mirror, acknowledgement = _prepare(rig.controller)
    rig.controller._pair_deferred_sample_ids = frozenset((0, 215))
    if terminal_kind == "error":
        rig.backend.error = RuntimeError("Original worker failure")
        rig.controller._run_stem_backend(job)
    else:

        def unexpected(
            _request: StemGenerationRequest,
            _progress: StemProgressCallback,
        ) -> StemGenerationResult:
            message = "unexpected worker failure"
            raise KeyError(message)

        monkeypatch.setattr(rig.backend, "generate", unexpected)
        with pytest.raises(KeyError, match="unexpected worker failure"):
            rig.controller._run_stem_backend(job)
    rig.controller._poll_generation_events()
    held = rig.controller._pair_held_generation_events[job, "terminal"]
    terminal = held.event
    assert terminal.event_type == terminal_kind
    assert terminal.error is not None
    assert rig.controller._session.stem_generation_errors[36] == terminal.error
    assert 0 not in rig.controller._session.stem_generation_errors
    assert not cast("_Lease", original.source_lease).released
    assert not cast("_Reservation", original.retirement).closed
    assert rig.assets.retired == []
    _move(rig, mirror, acknowledgement)
    rig.controller._put_generation_event(replace(terminal, error="Repeated delivery differs"))
    rig.controller._poll_generation_events()
    assert rig.controller._pair_held_generation_events[job, "terminal"] is held
    assert held.event is terminal
    rig.controller._pair_deferred_sample_ids = frozenset()
    rig.controller._poll_generation_events()
    rig.controller._poll_generation_events()
    assert rig.controller._session.stem_generation_errors[36] == terminal.error
    assert rig.controller._session.stem_generation_errors[215] == terminal.error
    assert rig.queued == []
    assert rig.assets.retired == [job.request.cache_dir]
    assert not rig.controller._jobs
    assert not rig.controller._pair_held_generation_events


def test_remaining_interest_still_requires_current_own_native_source_after_mirror(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    job = _job(rig)
    _subscribe(rig, job, 36)
    mirror, acknowledgement = _prepare(rig.controller)
    rig.controller._pair_deferred_sample_ids = frozenset((0, 215))
    rig.controller._run_stem_backend(job)
    rig.controller._poll_generation_events()
    _move(rig, mirror, acknowledgement)
    rig.placements[215] = replace(rig.placements[215], source_request=999)
    rig.controller._pair_deferred_sample_ids = frozenset()
    rig.controller._poll_generation_events()
    assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [36]
    assert [call.args[0] for call in rig.audio.capture_prepared_stem_source.call_args_list] == [36]
    assert rig.controller._project.stem_cache[215] is None
    assert rig.assets.retired == [job.request.cache_dir]
    assert not rig.controller._pair_held_generation_events


def test_identity_is_one_retained_owner_while_unrelated_terminal_continues(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    job = _job(rig)
    unrelated = _job(rig, 36)
    rig.controller._pair_deferred_sample_ids = frozenset((0, 215))
    event = stems._StemBackendEvent(
        0,
        job.request.source_version,
        job.source,
        job.request.cache_dir,
        "identity",
    )
    admissions: list[tuple[stems._StemBackendEvent, StemGenerationJob]] = []

    def admit(delivery: stems._StemJobEventDelivery, available: tuple[StemSubscriber, ...]) -> None:
        if not available:
            return
        admissions.append((delivery.event, delivery.job))
        rig.controller._pair_held_generation_events.pop((delivery.job, "identity"))

    monkeypatch.setattr(
        rig.controller,
        "_dispatch_resolved_identity",
        admit,
    )
    rig.controller._put_generation_event(event)
    rig.controller._put_generation_event(event)
    rig.controller._run_stem_backend(unrelated)
    rig.controller._poll_generation_events()
    assert admissions == []
    assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [36]
    assert len(rig.controller._pair_held_generation_events) == 1
    assert rig.controller._pair_held_generation_events[job, "identity"].event is event
    rig.controller._pair_deferred_sample_ids = frozenset()
    rig.controller._poll_generation_events()
    assert admissions == [(event, job)]
    assert not rig.controller._pair_held_generation_events
    job.cancel()


def test_shared_identity_admits_unrelated_inference_before_pair_then_moved_interest_once(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    job = _job(rig)
    _subscribe(rig, job, 36)
    original = job.subscribers()[0]
    original_request = job.request
    mirror, acknowledgement = _prepare(rig.controller)
    rig.controller._pair_deferred_sample_ids = frozenset((0, 215))
    tasks: list[Callable[[], None]] = []
    rig.controller._stem_task_runner = tasks.append
    guard = _resolve_identity(rig, job, monkeypatch)
    resolved = job.resolved_request
    assert resolved is not None
    rig.controller._poll_generation_events()
    identity = rig.controller._pair_held_generation_events[job, "identity"]
    assert identity.admission_route == "inference"
    assert identity.remaining == (original,)
    assert job.subscribers()[0] is original
    assert rig.controller._generation_intents[0] is original.intent
    assert not cast("_Lease", original.source_lease).released
    assert len(tasks) == 1
    assert job.request is original_request
    assert rig.backend.requests == []
    tasks.pop()()
    rig.controller._poll_generation_events()
    terminal = rig.controller._pair_held_generation_events[job, "terminal"]
    promotion = terminal.promotion
    assert promotion is not None
    assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [36]
    assert identity.remaining == (original,)
    assert rig.controller._jobs[job.request.cache_dir] is job
    assert job.processing_files is guard
    assert not cast("_Reservation", original.retirement).closed
    _move(rig, mirror, acknowledgement)
    rig.controller._pair_deferred_sample_ids = frozenset()
    rig.controller._poll_generation_events()
    rig.controller._poll_generation_events()
    assert tasks == []
    assert job.request is original_request
    _assert_completed_shared_job(rig, resolved, original, promotion)


def test_shared_identity_reuses_pinned_actual_entry_before_pair_and_only_remaining_after_mirror(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    job = _job(rig)
    _subscribe(rig, job, 36)
    original = job.subscribers()[0]
    mirror, acknowledgement = _prepare(rig.controller)
    rig.controller._pair_deferred_sample_ids = frozenset((0, 215))
    tasks: list[Callable[[], None]] = []
    rig.controller._stem_task_runner = tasks.append
    _resolve_identity(rig, job, monkeypatch)
    completed = _completed_entry(rig, job)
    rig.controller._project.stem_cache[86] = completed

    def prepare(
        sample_id: int,
        entry: StemCacheEntry,
        ticket: PreparedSourceTicket,
        **_options: object,
    ) -> bool:
        rig.queued.append((sample_id, entry, ticket))
        rig.controller._project.stem_cache[sample_id] = entry
        return True

    monkeypatch.setattr(rig.controller, "_prepare_pair", prepare)
    rig.controller._poll_generation_events()
    identity = rig.controller._pair_held_generation_events[job, "identity"]
    promotion = identity.promotion
    assert promotion is not None
    assert identity.admission_route == "reuse"
    assert identity.remaining == (original,)
    assert promotion.entry is not completed
    assert promotion.entry.cache_dir == completed.cache_dir
    assert not cast("_Lease", promotion.lease).released
    assert not cast("_Lease", original.source_lease).released
    assert job.subscribers()[0] is original
    assert rig.controller._generation_intents[0] is original.intent
    assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [36]
    assert tasks == []
    assert len(rig.backend.requests) == 1
    rig.controller._project.stem_cache[86] = None
    rig.controller._project.stem_cache[36] = None
    _move(rig, mirror, acknowledgement)
    rig.controller._pair_deferred_sample_ids = frozenset()
    rig.controller._poll_generation_events()
    rig.controller._poll_generation_events()
    assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [36, 215]
    assert rig.queued[1][1].cache_dir == completed.cache_dir
    assert tasks == []
    assert len(rig.backend.requests) == 1
    assert cast("_Lease", promotion.lease).released
    assert Path(completed.cache_dir, ".complete.json").is_file()
    assert rig.assets.retired == [job.request.cache_dir]
    assert not rig.controller._jobs
    assert not rig.controller._pair_held_generation_events


def test_distinct_canonical_job_keeps_partial_admission_visible_without_duplicate_inference(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    rig.controller._project.sample_paths[86] = "samples/shared.wav"
    rig.controller._project.pad_content[86] = PadContentIdentity(instance_id="d" * 32)
    canonical = _job(rig, 86)
    _resolve_identity(rig, canonical, monkeypatch)
    job = _job(rig)
    _subscribe(rig, job, 36)
    original = job.subscribers()[0]
    _resolve_identity(rig, job, monkeypatch)
    rig.controller._pair_deferred_sample_ids = frozenset((0, 215))
    tasks: list[Callable[[], None]] = []
    rig.controller._stem_task_runner = tasks.append
    rig.controller._poll_generation_events()
    identity = rig.controller._pair_held_generation_events[job, "identity"]
    assert identity.admission_route == "canonical_pending"
    assert identity.canonical is canonical
    assert identity.remaining == (original,)
    assert len(tasks) == 1
    assert canonical.request.cache_dir in rig.controller._inference_jobs
    assert job.request.cache_dir not in rig.controller._inference_jobs
    assert job.request.cache_dir not in rig.controller._waiting_jobs
    assert rig.controller._jobs[job.request.cache_dir] is job
    assert job.subscribers()[0] is original
    assert not cast("_Lease", original.source_lease).released
    assert "awaits Pair placement" in rig.controller._session.stem_generation_diagnostics[36]
    assert rig.backend.requests == []
    assert rig.queued == []
    job.cancel()
    canonical.cancel()


def _add_independent_pad(rig: _ObserverRig) -> None:
    rig.controller._project.sample_paths[86] = "samples/shared.wav"
    rig.controller._project.pad_content[86] = PadContentIdentity(instance_id="d" * 32)
    rig.accepts.add(86)


def _queue_reused_pair(rig: _ObserverRig, monkeypatch: pytest.MonkeyPatch) -> None:
    publish = rig.controller._queue_stem_publication

    def accepted_publication(
        sample_id: int,
        entry: StemCacheEntry,
        ticket: PreparedSourceTicket,
        *,
        prepared_pair: PreparedStemPair | None = None,
        generation_intent: StemGenerationIntent | None = None,
        own_source_version: str | None = None,
        artifact_source_version: str | None = None,
        interest_owner: StemGenerationInterests | None = None,
    ) -> bool:
        accepted = publish(
            sample_id,
            entry,
            ticket,
            prepared_pair=prepared_pair,
            generation_intent=generation_intent,
            own_source_version=own_source_version,
            artifact_source_version=artifact_source_version,
            interest_owner=interest_owner,
        )
        if accepted:
            # This test's Native-publication substitute has accepted a real
            # promoted artifact. Project the ready disk flag its ACK would set.
            rig.controller._project.stem_cache[sample_id] = entry.model_copy(
                update={"available": True}
            )
        return accepted

    def prepare(
        sample_id: int,
        entry: StemCacheEntry,
        ticket: PreparedSourceTicket,
        **_options: object,
    ) -> bool:
        rig.queued.append((sample_id, entry, ticket))
        rig.controller._project.stem_cache[sample_id] = entry
        return True

    monkeypatch.setattr(rig.controller, "_queue_stem_publication", accepted_publication)
    monkeypatch.setattr(rig.controller, "_prepare_pair", prepare)


def test_distinct_canonical_success_reuses_original_candidate_after_pair(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    _add_independent_pad(rig)
    canonical = _job(rig, 86)
    _resolve_identity(rig, canonical, monkeypatch)
    job = _job(rig)
    _subscribe(rig, job, 36)
    original = job.subscribers()[0]
    request = job.request
    mirror, acknowledgement = _prepare(rig.controller)
    rig.controller._pair_deferred_sample_ids = frozenset((0, 215))
    tasks: list[Callable[[], None]] = []
    rig.controller._stem_task_runner = tasks.append
    guard = _resolve_identity(rig, job, monkeypatch)
    _queue_reused_pair(rig, monkeypatch)
    rig.controller._poll_generation_events()
    identity = rig.controller._pair_held_generation_events[job, "identity"]
    assert identity.admission_route == "canonical_pending"
    assert len(tasks) == 1
    tasks.pop()()
    rig.controller._poll_generation_events()
    rig.controller._poll_generation_events()
    assert cast("str | None", identity.admission_route) == "reuse"
    assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [86, 36]
    assert job.request is request
    assert job.processing_files is guard
    assert job.subscribers()[0] is original
    assert not cast("_Lease", original.source_lease).released
    assert not job.cancel_event.is_set()
    _move(rig, mirror, acknowledgement)
    rig.controller._pair_deferred_sample_ids = frozenset()
    rig.controller._poll_generation_events()
    rig.controller._poll_generation_events()
    assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [86, 36, 215]
    assert len(rig.backend.requests) == 1
    assert tasks == []
    assert rig.queued[0][1].cache_dir == rig.queued[-1][1].cache_dir
    assert cast("_Lease", original.source_lease).released
    assert not rig.controller._jobs
    assert not rig.controller._pair_held_generation_events


def test_canonical_worker_return_between_observation_and_transfer_keeps_candidate(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    _add_independent_pad(rig)
    canonical = _job(rig, 86)
    _resolve_identity(rig, canonical, monkeypatch)
    job = _job(rig)
    original = job.subscribers()[0]
    _resolve_identity(rig, job, monkeypatch)
    tasks: list[Callable[[], None]] = []
    rig.controller._stem_task_runner = tasks.append
    transfer = job.transfer_subscribers_to

    def settle_before_transfer(
        target: StemGenerationJob, *, require_active: bool = False
    ) -> tuple[StemSubscriber, ...]:
        assert target is canonical
        assert require_active
        assert target.begin() is not None
        assert target.finish_read()
        return transfer(target, require_active=require_active)

    monkeypatch.setattr(job, "transfer_subscribers_to", settle_before_transfer)
    rig.controller._poll_generation_events()
    identity = rig.controller._pair_held_generation_events[job, "identity"]
    assert identity.admission_route == "canonical_pending"
    assert canonical.phase == "settled"
    own = job.subscribers()[0]
    assert own.source is original.source
    assert own.source_lease is original.source_lease
    assert own.retirement is original.retirement
    assert not job.cancel_event.is_set()
    assert not cast("_Lease", original.source_lease).released
    assert 0 not in {subscriber.sample_id for subscriber in canonical.subscribers()}
    assert len(tasks) == 1
    canonical.cancel()
    job.cancel()


def _settle_unavailable_canonical(
    rig: _ObserverRig,
    canonical: StemGenerationJob,
    terminal: str,
    tasks: list[Callable[[], None]],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    canonical_task = tasks.pop()
    if terminal == "removed":
        rig.controller._cancel_jobs(86)
        rig.controller._poll_generation_events()
        assert tasks == []
        assert canonical.request.cache_dir in rig.controller._inference_jobs
        canonical_task()
    elif terminal == "error":
        rig.backend.error = RuntimeError("Canonical inference failed")
        canonical_task()
        rig.backend.error = None
    else:
        generate = rig.backend.generate

        def cancelled_generate(
            resolved: StemGenerationRequest, progress: StemProgressCallback
        ) -> StemGenerationResult:
            assert canonical.running
            source = canonical.source
            rig.controller._cancel_jobs(86)
            assert canonical.source is source
            assert canonical.running
            return generate(resolved, progress)

        with monkeypatch.context() as patch:
            patch.setattr(rig.backend, "generate", cancelled_generate)
            canonical_task()


@pytest.mark.parametrize("terminal", ["error", "cancelled", "removed"])
def test_unavailable_canonical_rejoins_same_resolved_candidate_after_worker_settlement(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    terminal: str,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    _add_independent_pad(rig)
    canonical = _job(rig, 86)
    _resolve_identity(rig, canonical, monkeypatch)
    job = _job(rig)
    _subscribe(rig, job, 36)
    original = job.subscribers()[0]
    request = job.request
    mirror, acknowledgement = _prepare(rig.controller)
    rig.controller._pair_deferred_sample_ids = frozenset((0, 215))
    tasks: list[Callable[[], None]] = []
    rig.controller._stem_task_runner = tasks.append
    guard = _resolve_identity(rig, job, monkeypatch)
    rig.controller._poll_generation_events()
    identity = rig.controller._pair_held_generation_events[job, "identity"]
    _settle_unavailable_canonical(rig, canonical, terminal, tasks, monkeypatch)
    rig.controller._poll_generation_events()
    assert canonical.request.cache_dir not in rig.controller._inference_jobs
    rig.controller._poll_generation_events()
    assert identity.admission_route == "inference"
    assert identity.canonical is None
    assert len(tasks) == 1
    assert job.request is request
    assert job.processing_files is guard
    assert job.subscribers()[0] is original
    assert not job.cancel_event.is_set()
    assert not cast("_Lease", original.source_lease).released
    _move(rig, mirror, acknowledgement)
    rig.controller._pair_deferred_sample_ids = frozenset()
    rig.controller._poll_generation_events()
    moved = next(subscriber for subscriber in job.subscribers() if subscriber.sample_id == 215)
    assert moved.source is original.source
    assert moved.source_lease is original.source_lease
    assert moved.retirement is original.retirement
    assert moved.intent is rig.controller._generation_intents[215]
    assert moved.intent is not None
    assert original.intent is not None
    assert moved.intent.admission_sequence == original.intent.admission_sequence
    assert moved.intent.material_key == original.intent.material_key
    assert moved.intent.signature == original.intent.signature
    assert rig.controller._intents.is_current(moved.intent)
    tasks.pop()()
    rig.controller._poll_generation_events()
    rig.controller._poll_generation_events()
    assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [36, 215]
    assert rig.backend.requests[-1] is job.resolved_request
    assert tasks == []
    assert not rig.controller._jobs
    assert not rig.controller._pair_held_generation_events


def _assert_held_canonical_interest(
    rig: _ObserverRig,
    canonical: StemGenerationJob,
    identity: stems._StemJobEventDelivery,
    own: StemSubscriber,
    terminal: str,
) -> None:
    assert 86 not in {subscriber.sample_id for subscriber in canonical.subscribers()}
    assert len(rig.backend.requests) == 1
    assert terminal in {"error", "success"}
    if terminal == "error":
        assert identity.admission_route == "canonical_pending"
        assert not cast("_Lease", own.source_lease).released
        assert 86 not in rig.controller._session.stem_generation_errors
    else:
        assert identity.admission_route == "reuse"
        assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [36, 86]


@pytest.mark.parametrize("terminal", ["success", "error"])
def test_settlement_fenced_canonical_never_absorbs_uncaptured_terminal_interest(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    terminal: str,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    _add_independent_pad(rig)
    canonical = _job(rig)
    _subscribe(rig, canonical, 36)
    original = canonical.subscribers()[0]
    mirror, acknowledgement = _prepare(rig.controller)
    job = _job(rig, 86)
    own = job.subscribers()[0]
    rig.controller._pair_deferred_sample_ids = frozenset((0, 215))
    tasks: list[Callable[[], None]] = []
    rig.controller._stem_task_runner = tasks.append
    _resolve_identity(rig, canonical, monkeypatch)
    _resolve_identity(rig, job, monkeypatch)
    _queue_reused_pair(rig, monkeypatch)
    rig.controller._poll_generation_events()
    identity = rig.controller._pair_held_generation_events[job, "identity"]
    assert identity.remaining == ()
    assert identity.admission_route == "canonical_pending"
    assert job.subscribers()[0].source is own.source
    assert len(tasks) == 1
    if terminal == "error":
        rig.backend.error = RuntimeError("Canonical terminal is held")
    tasks.pop()()
    rig.backend.error = None
    rig.controller._poll_generation_events()
    held = rig.controller._pair_held_generation_events[canonical, "terminal"]
    assert held.remaining == (original,)
    rig.controller._poll_generation_events()
    assert tasks == []
    _assert_held_canonical_interest(rig, canonical, identity, own, terminal)
    _move(rig, mirror, acknowledgement)
    rig.controller._pair_deferred_sample_ids = frozenset()
    rig.controller._poll_generation_events()
    rig.controller._poll_generation_events()
    if terminal == "error":
        assert cast("str | None", identity.admission_route) == "inference"
        assert len(tasks) == 1
        tasks.pop()()
        rig.controller._poll_generation_events()
        assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [86]
    else:
        assert [sample_id for sample_id, _entry, _ticket in rig.queued] == [36, 86, 215]
    assert tasks == []
    assert not rig.controller._jobs
    assert not rig.controller._pair_held_generation_events
