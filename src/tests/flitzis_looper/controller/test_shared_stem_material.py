"""Real controller admission/fanout, with only native devices/separation substituted."""

import threading
from dataclasses import replace
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller import stem_provenance
from flitzis_looper.controller import stems as stems_module
from flitzis_looper.models import PadContentIdentity
from flitzis_looper.stem_execution import StemExecution
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import FakePreparedSourceTicket, FakeProjectAssetLease

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_generation import (
        StemGenerationRequest,
        StemGenerationResult,
        StemProgressCallback,
    )
    from tests.flitzis_looper.conftest import FakeStemGenerationBackend


@pytest.fixture(autouse=True)
def _declared_shared_producer(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    stem_backend: FakeStemGenerationBackend,
) -> None:
    """Attest this controller fixture with actual owned Native file custody."""
    model = tmp_path / "owned-shared-producer"
    model.mkdir()
    files = [model / name for name in ("checkpoint.bin", "definition.py", "ffmpeg.exe")]
    for index, path in enumerate(files):
        path.write_bytes(f"declared shared producer input {index}".encode())
    monkeypatch.setattr(stems_module, "separator_model_cache_dir", lambda _separator: model)
    monkeypatch.setattr(stems_module, "resolved_demucs_encoder", lambda _model: files[-1])
    monkeypatch.setattr(stems_module, "encoder_contract", lambda _encoder: "owned-fixture")
    monkeypatch.setattr(stem_provenance, "processing_paths", lambda _request: tuple(files))
    monkeypatch.setattr(
        stem_provenance, "processing_library_versions", lambda _request: (("owned-fixture", "1"),)
    )
    monkeypatch.setattr(
        stem_provenance, "loaded_code_identity", lambda _paths: "declared-shared-controller-fixture"
    )
    monkeypatch.setattr(
        stem_provenance, "processing_source_roots", lambda _request, _guard: (tmp_path,)
    )
    monkeypatch.setattr(
        stems_module, "processing_source_roots", stem_provenance.processing_source_roots
    )
    stem_backend.result = replace(
        stem_backend.result, backend_name="demucs", model_name="htdemucs", encoder_verified=True
    )


def _shared(
    controller: AppController, audio: Mock, tmp: Path
) -> tuple[list[Callable[[], None]], list[FakePreparedSourceTicket]]:
    source = tmp / "samples" / "shared.wav"
    source.parent.mkdir()
    write_mono_pcm16_wav(source, 44_100)
    tickets = [FakePreparedSourceTicket("pending"), FakePreparedSourceTicket("pending")]
    own_tickets = dict(zip((0, 215), tickets, strict=True))
    audio.capture_prepared_source.side_effect = lambda slot, _version: own_tickets[slot]
    for slot, identity in [(0, "1" * 32), (215, "2" * 32)]:
        controller.project.sample_paths[slot] = "samples/shared.wav"
        controller.project.sample_durations[slot] = 128 / 44_100
        controller.project.pad_content[slot] = PadContentIdentity(instance_id=identity)
        audio.load_sample_async(slot, "samples/shared.wav")
    tasks: list[Callable[[], None]] = []
    controller.stems._stem_task_runner = tasks.append
    return tasks, tickets


def _resolve_candidates(
    controller: AppController, tasks: list[Callable[[], None]], count: int = 1
) -> None:
    """Return full-byte resolutions to control before inference may be scheduled."""
    assert len(tasks) == count
    for _ in range(count):
        tasks.pop(0)()
    controller.stems.on_frame_render()
    assert len(tasks) == len(controller.stems._jobs) == 1
    job = next(iter(controller.stems._jobs.values()))
    assert job.resolved_request is not None
    assert job.generation_fingerprint is not None


def test_coalesced_alignment_retains_device_for_current_shared_subscribers(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    tasks, _tickets = _shared(controller, audio_engine_mock, tmp_path)
    assert controller.stems.generate_stems_async(0)
    assert controller.stems.generate_stems_async(215)
    _resolve_candidates(controller, tasks, 2)
    job = next(iter(controller.stems._jobs.values()))
    execution = StemExecution(backend="demucs", model_name="htdemucs", device="cuda")
    event = stems_module._StemBackendEvent(
        job.request.sample_id,
        job.request.source_version,
        job.source,
        job.request.cache_dir,
        "progress",
        percent=0.8,
        execution=execution,
    )
    first = controller.stems._capture_generation_delivery(event, job)
    assert first is not None
    last = controller.stems._capture_generation_delivery(
        replace(event, percent=0.85, stage="Aligning stem cache", execution=None), job
    )
    assert last is not None
    assert last.event.execution == execution
    controller.stems._dispatch_job_event(last.event, job, delivery=last)
    assert all(controller.session.stem_generation_execution.get(i) == execution for i in (0, 215))


def test_one_generation_has_independent_out_of_order_ack_and_saved_owners(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
) -> None:
    tasks, tickets = _shared(controller, audio_engine_mock, tmp_path)
    execution = StemExecution(backend="demucs", model_name="htdemucs", device="cuda")
    stem_backend.result = replace(stem_backend.result, execution=execution)
    assert controller.stems.generate_stems_async(0)
    assert controller.stems.generate_stems_async(215)
    assert len(tasks) == len(controller.stems._jobs) == 2
    _resolve_candidates(controller, tasks, 2)
    inference = tasks.pop(0)
    inference()
    inference()  # inference is single-run, even if a runner repeats delivery
    controller.stems.on_frame_render()
    assert len(stem_backend.requests) == 1
    entries = [controller.project.stem_cache[slot] for slot in (0, 215)]
    first, last = entries
    assert first is not None
    assert last is not None
    assert not first.available
    assert not last.available
    assert entries[0] is not entries[1]
    assert first.cache_dir == last.cache_dir
    assert first.execution == last.execution == execution
    assert first.fingerprint_schema == last.fingerprint_schema == "effective-stem-v3"
    assert first.generation_fingerprint == last.generation_fingerprint
    assert first.generation_fingerprint is not None
    calls = audio_engine_mock.publish_prepared_stems.call_args_list
    assert [(call.args[0], call.args[3]) for call in calls] == [(0, tickets[0]), (215, tickets[1])]
    tickets[1].status = "accepted"
    controller.stems.on_frame_render()
    assert controller.stems.stems_available(215)
    assert controller.stems.stem_execution(215) == execution
    assert not controller.stems.stems_available(0)
    tickets[0].status = "rejected"
    controller.stems.on_frame_render()
    assert controller.stems.stems_available(215)
    generation = tmp_path / last.cache_dir
    assert generation.is_dir()
    audio_engine_mock.retire_project_asset.reset_mock()
    assert controller.stems.delete_stems(0)
    assert not any(
        call.args[0] == str(generation)
        for call in audio_engine_mock.retire_project_asset.call_args_list
    )
    assert controller.stems.delete_stems(215)
    audio_engine_mock.retire_project_asset.assert_any_call(str(generation), recursive=True)


@pytest.mark.parametrize("failure", ["origin_cancel", "queue_full", "playing", "new_content"])
def test_one_subscriber_failure_does_not_revoke_the_other(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    failure: str,
) -> None:
    tasks, tickets = _shared(controller, audio_engine_mock, tmp_path)
    assert controller.stems.generate_stems_async(0)
    assert controller.stems.generate_stems_async(215)
    _resolve_candidates(controller, tasks, 2)
    if failure == "origin_cancel":
        controller.loader.unload_sample(0)
    elif failure == "playing":
        controller.session.active_sample_ids.add(0)
    elif failure == "new_content":
        controller.project.pad_content[0] = PadContentIdentity(instance_id="3" * 32)
    else:

        def publish(slot: int, *_args: object) -> None:
            if slot == 0:
                message = "control queue full"
                raise RuntimeError(message)

        audio_engine_mock.publish_prepared_stems.side_effect = publish
    tickets[1].status = "accepted"
    tasks.pop(0)()
    controller.stems.on_frame_render()
    assert len(stem_backend.requests) == 1
    assert controller.stems.stems_available(215)
    assert not controller.stems.stems_available(0)
    assert not controller.stems._jobs


def test_second_interest_capacity_failure_preserves_first_job(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
) -> None:
    tasks, tickets = _shared(controller, audio_engine_mock, tmp_path)
    assert controller.stems.generate_stems_async(0)
    assets = controller._assets
    assets._MAX_PENDING_RETIREMENTS = assets._reserved + 15
    assert not controller.stems.generate_stems_async(215)
    assert not controller.stems.is_stem_generation_running(215)
    assert controller.stems.is_stem_generation_running(0)
    assert controller.project.stem_cache[215] is None
    tickets[0].status = "accepted"
    _resolve_candidates(controller, tasks)
    tasks.pop(0)()
    controller.stems.on_frame_render()
    assert len(stem_backend.requests) == 1
    assert controller.stems.stems_available(0)


@pytest.mark.parametrize("defer", [False, True])
def test_rejection_uses_reserved_previous_owner_when_new_acquisition_is_full(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    defer: bool,
) -> None:
    tasks, tickets = _shared(controller, audio_engine_mock, tmp_path)
    tickets[0].status = "accepted"
    assert controller.stems.generate_stems_async(0)
    _resolve_candidates(controller, tasks)
    tasks.pop()()
    controller.stems.on_frame_render()
    previous = controller.project.stem_cache[0]
    assert previous is not None
    assert previous.available
    pending_ticket = FakePreparedSourceTicket("pending")
    audio_engine_mock.capture_prepared_source.side_effect = None
    audio_engine_mock.capture_prepared_source.return_value = pending_ticket
    controller.project.demucs_shifts += 1
    assert controller.stems.generate_stems_async(0)
    _resolve_candidates(controller, tasks)
    # The entire remaining backlog is reserved; publication must use transferred capacity.
    assets = controller._assets
    remaining = assets._MAX_PENDING_RETIREMENTS - assets._reserved
    saturation = assets.reserve(remaining)
    try:
        tasks.pop()()
        controller.stems.on_frame_render()
        pending = controller.stems._pending_stem_publications[0]
        owner = pending.previous_lease
        assert owner is not None
        audio_engine_mock.acquire_project_asset_lease.side_effect = RuntimeError(
            "owner registry full"
        )
        pending_ticket.status = "rejected"
        if defer:
            reclaim = owner.reclaim_stems

            def unavailable(_path: str) -> None:
                message = "sharing denied"
                raise OSError(message)

            monkeypatch.setattr(owner, "reclaim_stems", unavailable)
            controller.stems.on_frame_render()
            assert controller.stems._pending_stem_publications[0] is pending
            assert controller.project.stem_cache[0] is pending.entry
            assert not owner.released
            assert pending.retirement.remaining > 0
            assert "restore deferred" in controller.session.stem_generation_errors[0]
            monkeypatch.setattr(owner, "reclaim_stems", reclaim)
        controller.stems.on_frame_render()
        assert controller.project.stem_cache[0] is previous
        assert assets._assignments["stems", 0][1] is owner
        assert owner is not None
        assert not owner.released
        assert pending.previous_lease is None
    finally:
        saturation.close()


@pytest.mark.parametrize("last_cancel", [False, True])
def test_shared_running_read_survives_origin_and_releases_only_after_return(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    last_cancel: bool,
) -> None:
    tasks, tickets = _shared(controller, audio_engine_mock, tmp_path)
    assert controller.stems.generate_stems_async(0)
    assert controller.stems.generate_stems_async(215)
    _resolve_candidates(controller, tasks, 2)
    job = next(iter(controller.stems._jobs.values()))
    entered, finish = threading.Event(), threading.Event()
    generate = stem_backend.generate

    def blocked(
        request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        entered.set()
        assert finish.wait(5)
        assert request.source_path.is_file()
        with pytest.raises(PermissionError):
            (request.model_cache_dir / "checkpoint.bin").write_bytes(
                b"producer rewrite before the actual shared backend read returns"
            )
        return generate(request, progress)

    monkeypatch.setattr(stem_backend, "generate", blocked)
    thread = threading.Thread(target=tasks.pop(0))
    thread.start()
    assert entered.wait(5)
    try:
        controller.loader.unload_sample(0)
        if last_cancel:
            controller.loader.unload_sample(215)
        assert not job._source_lease.released
        assert not job._generation_lease.released
    finally:
        finish.set()
        thread.join(5)
    assert not thread.is_alive()
    assert job._source_lease.released
    tickets[1].status = "accepted"
    controller.stems.on_frame_render()
    assert controller.stems.stems_available(215) is (not last_cancel)
    assert job._generation_lease.released
    assert len(stem_backend.requests) == 1


def test_new_version_keeps_other_banks_saved_old_version_through_shutdown(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
) -> None:
    tasks, tickets = _shared(controller, audio_engine_mock, tmp_path)
    for ticket in tickets:
        ticket.status = "accepted"
    assert controller.stems.generate_stems_async(0)
    assert controller.stems.generate_stems_async(215)
    _resolve_candidates(controller, tasks, 2)
    tasks.pop()()
    controller.stems.on_frame_render()
    old = controller.project.stem_cache[215]
    assert old is not None
    audio_engine_mock.retire_project_asset.reset_mock()
    audio_engine_mock.capture_prepared_source.side_effect = None
    audio_engine_mock.capture_prepared_source.return_value = FakePreparedSourceTicket("accepted")
    controller.project.demucs_shifts += 1
    assert controller.stems.generate_stems_async(0)
    _resolve_candidates(controller, tasks)
    tasks.pop()()
    controller.stems.on_frame_render()
    assert controller.project.stem_cache[215] is old
    current = controller.project.stem_cache[0]
    assert current is not None
    assert current.cache_dir != old.cache_dir
    old_path = tmp_path / old.cache_dir
    assert not any(
        call.args[0] == str(old_path)
        for call in audio_engine_mock.retire_project_asset.call_args_list
    )
    controller.loader.unload_sample(0)
    assert controller.project.stem_cache[215] is old
    controller.shut_down()
    assert old_path.is_dir()
    assert not any(
        call.args[0] == str(old_path)
        for call in audio_engine_mock.retire_project_asset.call_args_list
    )


def test_reference_image_acquisition_failure_keeps_all_previous_owners(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
) -> None:
    _shared(controller, audio_engine_mock, tmp_path)
    assets = controller._assets
    assets.sync_assignments()
    previous = dict(assets._assignments)
    created: list[FakeProjectAssetLease] = []

    def acquire(path: str) -> FakeProjectAssetLease:
        if created:
            message = "owner registry full"
            raise OSError(message)
        lease = FakeProjectAssetLease(path)
        created.append(lease)
        return lease

    for slot in (0, 215):
        target = tmp_path / "samples" / f"new-{slot}.wav"
        write_mono_pcm16_wav(target, 44_100)
        controller.project.sample_paths[slot] = f"samples/new-{slot}.wav"
    audio_engine_mock.acquire_project_asset_lease.side_effect = acquire
    with pytest.raises(OSError, match="owner registry full"):
        assets.sync_assignments()
    assert assets._assignments == previous
    assert created[0].released
    audio_engine_mock.retire_project_asset.assert_not_called()
