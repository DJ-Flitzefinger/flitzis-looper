import subprocess
import threading
from pathlib import Path
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller import stem_generation
from flitzis_looper.models import STEM_KINDS
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import FakeProjectAssetLease

if TYPE_CHECKING:
    from collections.abc import Callable
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_generation import (
        StemGenerationRequest,
        StemGenerationResult,
        StemProgressCallback,
    )
    from flitzis_looper_audio import ProjectAssetLease
    from tests.flitzis_looper.conftest import FakeStemGenerationBackend


def _source(controller: AppController, tmp_path: Path) -> Path:
    source = tmp_path / "samples" / "shared.wav"
    source.parent.mkdir(exist_ok=True)
    write_mono_pcm16_wav(source, 44_100)
    controller.project.sample_paths[0] = "samples/shared.wav"
    controller.project.sample_durations[0] = 128 / 44_100
    return source


def _track_leases(audio_engine_mock: Mock) -> list[FakeProjectAssetLease]:
    leases: list[FakeProjectAssetLease] = []

    def capture(path: str) -> FakeProjectAssetLease:
        lease = FakeProjectAssetLease(path)
        leases.append(lease)
        return lease

    audio_engine_mock.acquire_project_asset_lease.side_effect = capture
    return leases


class _EagerLease(FakeProjectAssetLease):
    def __init__(
        self, path: str, drain: Callable[[], None], reclaim: Callable[[Path], None]
    ) -> None:
        super().__init__(path)
        self._drain = drain
        self._reclaim = reclaim

    def reclaim_stems(self, expected_path: str) -> None:
        super().reclaim_stems(expected_path)
        self._reclaim(Path(expected_path))

    def release(self) -> None:
        super().release()
        self._drain()


def _install_eager_registry(audio: Mock) -> None:
    """Delete eligible declared files immediately, including on final release."""
    leases: list[_EagerLease] = []
    pending: dict[Path, bool] = {}

    def drain() -> None:
        for target, recursive in tuple(pending.items()):
            owners = [Path(lease.path) for lease in leases if not lease.released]
            if any(
                owner == target or owner.is_relative_to(target) or target.is_relative_to(owner)
                for owner in owners
            ):
                continue
            pending.pop(target)
            if recursive and target.is_dir():
                for name in (*(f"{kind}.wav" for kind in STEM_KINDS), ".complete.json"):
                    (target / name).unlink(missing_ok=True)
                target.rmdir()
            else:
                target.unlink(missing_ok=True)

    def reclaim(target: Path) -> None:
        pending.pop(target, None)

    def acquire(path: str) -> _EagerLease:
        pending.pop(Path(path), None)
        lease = _EagerLease(path, drain, reclaim)
        leases.append(lease)
        return lease

    def retire(path: str, *, recursive: bool = False) -> None:
        pending[Path(path)] = recursive
        drain()

    audio.acquire_project_asset_lease.side_effect = acquire
    audio.retire_project_asset.side_effect = retire


def test_unloading_shared_original_releases_only_one_assignment(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    source = _source(controller, tmp_path)
    controller.project.sample_paths[1] = "samples\\shared.wav"
    leases = _track_leases(audio_engine_mock)

    controller.loader.unload_sample(0)

    source_owners = [lease for lease in leases if lease.path == str(source)]
    assert len(source_owners) == 2
    assert source_owners[0].released is True
    assert source_owners[1].released is False
    assert source.exists()
    assert controller.project.sample_paths[1] == "samples\\shared.wav"
    audio_engine_mock.retire_project_asset.assert_not_called()
    controller.loader.unload_sample(1)
    assert source_owners[1].released is True
    audio_engine_mock.retire_project_asset.assert_called_once_with(str(source), recursive=False)


def test_retirement_queue_failure_keeps_owner_until_productive_poll_retry(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    source = _source(controller, tmp_path)
    leases = _track_leases(audio_engine_mock)
    audio_engine_mock.retire_project_asset.side_effect = ValueError(
        "project asset retirement queue full"
    )

    controller.loader.unload_sample(0)

    assert controller.project.sample_paths[0] is None
    assert leases[0].released is False
    assert source in controller._assets.retirement_errors
    audio_engine_mock.retire_project_asset.side_effect = None
    audio_engine_mock.poll_loader_events.return_value = None
    controller.loader.poll_loader_events()
    assert leases[0].released is True
    assert controller._assets.retirement_errors == {}


def test_saved_original_reacquire_cancels_unadmitted_old_retirement_before_shutdown(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    source = _source(controller, tmp_path)
    _install_eager_registry(audio_engine_mock)
    assets = controller._assets
    monkeypatch.setattr(assets, "_start_retry_worker_locked", lambda: None)
    assets.sync_assignments()
    previous_lease = assets._assignments["original", 0][1]
    native_retire = audio_engine_mock.retire_project_asset.side_effect
    audio_engine_mock.retire_project_asset.side_effect = ValueError(
        "project asset retirement queue full"
    )

    controller.loader.unload_sample(0)
    assert not previous_lease.released
    assert source in assets.retirement_errors
    controller.project.sample_paths[1] = "samples/shared.wav"
    assets.sync_assignments()
    audio_engine_mock.retire_project_asset.side_effect = native_retire
    assets.retry_retirements()
    controller.shut_down()

    assert source.exists()
    assert controller.project.sample_paths[1] == "samples/shared.wav"
    assert previous_lease.released
    assert not assets._pending
    assert source not in assets.retirement_errors


def test_relative_stem_retirement_is_cancelled_by_absolute_saved_reacquire(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _source(controller, tmp_path)
    _install_eager_registry(audio_engine_mock)
    assets = controller._assets
    monkeypatch.setattr(assets, "_start_retry_worker_locked", lambda: None)
    assert controller.stems.generate_stems_async(0)
    controller.stems.on_frame_render()
    entry = controller.project.stem_cache[0]
    assert entry is not None
    relative = Path(entry.cache_dir)
    absolute = tmp_path / relative
    previous = assets.acquire(absolute)
    native_retire = audio_engine_mock.retire_project_asset.side_effect
    audio_engine_mock.retire_project_asset.side_effect = ValueError(
        "project asset retirement queue full"
    )
    assets.retire(relative, recursive=True, lease=previous)
    assert not previous.released

    saved = assets.acquire(absolute)
    audio_engine_mock.retire_project_asset.side_effect = native_retire
    assets.retry_retirements()
    controller.shut_down()
    saved.release()

    assert controller.project.stem_cache[0] is entry
    assert absolute.exists()
    assert (absolute / ".complete.json").exists()
    assert previous.released
    assert not assets._pending
    assert not assets.retirement_errors


def test_rejected_legacy_stems_cancel_old_child_retirement_retries_before_shutdown(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _source(controller, tmp_path)
    _install_eager_registry(audio_engine_mock)
    assets = controller._assets
    monkeypatch.setattr(assets, "_start_retry_worker_locked", lambda: None)
    assert controller.stems.generate_stems_async(0)
    controller.stems.on_frame_render()
    generated = controller.project.stem_cache[0]
    assert generated is not None
    ready = tmp_path / generated.cache_dir
    legacy_path = ready.parent
    for file in ready.iterdir():
        file.replace(legacy_path / file.name)
    ready.rmdir()
    legacy = generated.model_copy(update={"cache_dir": str(Path(generated.cache_dir).parent)})
    controller.project.stem_cache[0] = legacy
    assets.sync_assignments()
    expected = {file.name: file.read_bytes() for file in legacy_path.iterdir()}
    ticket = audio_engine_mock.capture_prepared_source.return_value
    ticket.status = "pending"
    native_retire = audio_engine_mock.retire_project_asset.side_effect
    audio_engine_mock.retire_project_asset.side_effect = ValueError(
        "project asset retirement queue full"
    )

    assert controller.stems.generate_stems_async(0)
    controller.stems.on_frame_render()
    replacement = controller.project.stem_cache[0]
    assert replacement is not None
    failed_ready = tmp_path / replacement.cache_dir
    failed_private = legacy_path / f".generation-{'f' * 32}"
    failed_private.mkdir()
    write_mono_pcm16_wav(failed_private / "vocals.wav", 44_100)
    failed_private_lease = assets.acquire(failed_private)
    assets.retire(failed_private, recursive=True, lease=failed_private_lease)
    declared_files = {f"{kind}.wav" for kind in STEM_KINDS} | {".complete.json"}
    assert sum(
        path.parent == legacy_path and path.name in declared_files for path, _ in assets._pending
    ) == len(declared_files)
    assert len(assets._pending_groups) == 1
    ticket.status = "rejected"
    controller.stems.on_frame_render()
    assert (failed_ready, True) in assets._pending
    assert (failed_private, True) in assets._pending
    assert not assets._pending_groups
    audio_engine_mock.retire_project_asset.side_effect = native_retire
    assets.retry_retirements()
    assert failed_ready.exists()
    assert failed_private.exists()
    controller.shut_down()

    assert controller.project.stem_cache[0] is legacy
    assert {file.name: file.read_bytes() for file in legacy_path.iterdir()} == expected
    assert not failed_ready.exists()
    assert not failed_private.exists()
    assert not assets._pending
    assert not assets._pending_groups
    assert assets._reserved == 0


@pytest.mark.parametrize("source_path", ["../private.wav", "samples/../../private.wav"])
def test_unload_rejects_external_or_traversing_original_retirement(
    controller: AppController, audio_engine_mock: Mock, source_path: str
) -> None:
    controller.project.sample_paths[0] = source_path

    controller.loader.unload_sample(0)

    audio_engine_mock.acquire_project_asset_lease.assert_not_called()
    audio_engine_mock.retire_project_asset.assert_not_called()


def test_reparse_original_is_never_admitted_or_retired(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
) -> None:
    foreign = tmp_path / "foreign"
    foreign.mkdir()
    source = foreign / "shared.wav"
    write_mono_pcm16_wav(source, 44_100)
    samples = tmp_path / "samples"
    result = subprocess.run(
        ["cmd", "/c", "mklink", "/J", str(samples), str(foreign)],
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    controller.project.sample_paths[0] = "samples/shared.wav"

    controller.loader.unload_sample(0)

    assert source.exists()
    audio_engine_mock.acquire_project_asset_lease.assert_not_called()
    audio_engine_mock.retire_project_asset.assert_not_called()
    samples.rmdir()


def test_cancelled_unstarted_separator_never_reads_source(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
) -> None:
    source = _source(controller, tmp_path)
    leases = _track_leases(audio_engine_mock)
    queued: list[Callable[[], None]] = []
    controller.stems._stem_task_runner = queued.append
    assert controller.stems.generate_stems_async(0) is True

    controller.loader.unload_sample(0)
    queued[0]()

    assert stem_backend.requests == []
    assert all(lease.released for lease in leases if lease.path == str(source))
    audio_engine_mock.publish_prepared_stems.assert_not_called()


@pytest.mark.parametrize("shutdown", [False, True])
def test_cancelled_running_separator_retains_source_until_backend_returns(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    shutdown: bool,
) -> None:
    source = _source(controller, tmp_path)
    leases = _track_leases(audio_engine_mock)
    entered, finish = threading.Event(), threading.Event()
    worker_threads: list[threading.Thread] = []
    original_generate = stem_backend.generate

    def blocked_backend(
        request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        entered.set()
        assert finish.wait(5)
        assert source.exists()
        return original_generate(request, progress)

    monkeypatch.setattr(stem_backend, "generate", blocked_backend)

    def schedule(target: Callable[[], None]) -> None:
        thread = threading.Thread(target=target)
        worker_threads.append(thread)
        thread.start()

    controller.stems._stem_task_runner = schedule
    assert controller.stems.generate_stems_async(0) is True
    assert entered.wait(5)
    try:
        controller.loader.unload_sample(0)
        if shutdown:
            controller.shut_down()
        source_owners = [lease for lease in leases if lease.path == str(source)]
        assert len(source_owners) == 2
        assert source_owners[0].released is True
        assert source_owners[1].released is False
    finally:
        finish.set()
        worker_threads[0].join(5)
    assert not worker_threads[0].is_alive()
    assert all(lease.released for lease in source_owners)
    controller.stems.on_frame_render()
    audio_engine_mock.publish_prepared_stems.assert_not_called()
    assert any(
        call.kwargs["recursive"] and ".generation-" in call.args[0]
        for call in audio_engine_mock.retire_project_asset.call_args_list
    )


def test_retiring_old_generation_preserves_new_generation_and_unknown_files(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
) -> None:
    _source(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    previous = controller.project.stem_cache[0]
    assert previous is not None
    private_note = tmp_path / Path(previous.cache_dir).parent / "private-note.txt"
    private_note.write_text("preserve", encoding="utf-8")
    stem_backend.sample_value = -1024
    audio_engine_mock.retire_project_asset.reset_mock()

    assert controller.stems.generate_stems_async(0) is True
    assert controller.project.stem_cache[0] is previous
    controller.stems.on_frame_render()

    current = controller.project.stem_cache[0]
    assert current is not None
    assert current.cache_dir != previous.cache_dir
    assert private_note.read_text(encoding="utf-8") == "preserve"
    audio_engine_mock.retire_project_asset.assert_any_call(
        str(tmp_path / previous.cache_dir), recursive=True
    )
    assert all(
        call.args[0] != str(tmp_path / current.cache_dir)
        for call in audio_engine_mock.retire_project_asset.call_args_list
    )
    assert all((tmp_path / current.cache_dir / f"{kind}.wav").is_file() for kind in STEM_KINDS)


def test_failed_stem_regeneration_keeps_previous_project_set(
    controller: AppController,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
) -> None:
    _source(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    previous = controller.project.stem_cache[0]
    stem_backend.error = RuntimeError("separator failed")

    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()

    assert controller.project.stem_cache[0] is previous
    assert controller.stems.stems_available(0) is True


def test_shutdown_keeps_saved_original_without_scheduling_its_delete(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    source = _source(controller, tmp_path)
    controller._assets.sync_assignments()

    controller.shut_down()

    assert source.exists()
    assert controller.project.sample_paths[0] == "samples/shared.wav"
    audio_engine_mock.retire_project_asset.assert_not_called()


def test_shared_original_saved_alias_survives_unload_and_shutdown(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    source = _source(controller, tmp_path)
    controller.project.sample_paths[1] = "samples\\shared.wav"
    _install_eager_registry(audio_engine_mock)

    controller.loader.unload_sample(0)
    assert source.exists()
    controller.shut_down()

    assert source.exists()
    assert controller.project.sample_paths[1] == "samples\\shared.wav"
    audio_engine_mock.retire_project_asset.assert_not_called()


def test_shared_original_retires_only_after_last_alias_unloads(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    source = _source(controller, tmp_path)
    controller.project.sample_paths[1] = "samples\\shared.wav"
    _install_eager_registry(audio_engine_mock)

    controller.loader.unload_sample(0)
    assert source.exists()
    audio_engine_mock.retire_project_asset.assert_not_called()
    controller.loader.unload_sample(1)

    assert not source.exists()
    assert controller.project.sample_paths[:2] == [None, None]
    audio_engine_mock.retire_project_asset.assert_called_once_with(str(source), recursive=False)


def test_stale_acked_same_original_preserves_saved_assignment_through_shutdown(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    source = _source(controller, tmp_path)
    _install_eager_registry(audio_engine_mock)
    controller._assets.sync_assignments()
    controller.loader._load_request_ids[0] = 9
    audio_engine_mock.acquire_project_asset_lease.reset_mock()

    controller.loader._handle_loader_success(
        0,
        {"type": "success", "id": 0, "request_id": 8, "cached_path": "samples\\shared.wav"},
    )
    controller.shut_down()

    assert source.exists()
    assert controller.project.sample_paths[0] == "samples/shared.wav"
    audio_engine_mock.acquire_project_asset_lease.assert_called_once_with(str(source))
    audio_engine_mock.retire_project_asset.assert_not_called()


def test_stale_acked_success_retires_only_orphan_original_after_current_owners_acquired(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    current = _source(controller, tmp_path)
    orphan = current.with_name("acked-orphan.wav")
    orphan.write_bytes(current.read_bytes())
    controller.loader._load_request_ids[0] = 9
    leases = _track_leases(audio_engine_mock)

    controller.loader._handle_loader_success(
        0,
        {"type": "success", "id": 0, "request_id": 8, "cached_path": "samples/acked-orphan.wav"},
    )

    assert controller.project.sample_paths[0] == "samples/shared.wav"
    assert len(leases) == 1
    assert leases[0].path == str(current)
    assert not leases[0].released
    audio_engine_mock.retire_project_asset.assert_called_once_with(str(orphan), recursive=False)


def test_same_original_success_acknowledges_delivery_without_releasing_saved_owner(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    source = _source(controller, tmp_path)
    leases = _track_leases(audio_engine_mock)
    controller._assets.sync_assignments()

    controller.loader._handle_loader_success(
        0, {"type": "success", "id": 0, "cached_path": "samples/shared.wav"}
    )

    assert len(leases) == 2
    assert all(lease.path == str(source) for lease in leases)
    assert not leases[0].released
    assert leases[1].released
    assert controller.project.sample_paths[0] == "samples/shared.wav"
    audio_engine_mock.retire_project_asset.assert_not_called()


def test_rejected_regenerated_stems_restore_previous_set(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
) -> None:
    _source(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    previous = controller.project.stem_cache[0]
    ticket = audio_engine_mock.capture_prepared_source.return_value
    ticket.status = "pending"

    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    assert controller.project.stem_cache[0] is not previous
    ticket.status = "rejected"
    controller.stems.on_frame_render()

    assert controller.project.stem_cache[0] is previous
    assert previous is not None
    assert previous.available


def test_separator_thread_admission_failure_releases_job_owners(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    source = _source(controller, tmp_path)
    leases = _track_leases(audio_engine_mock)

    def reject_worker(_target: Callable[[], None]) -> None:
        message = "cannot start worker thread"
        raise RuntimeError(message)

    controller.stems._stem_task_runner = reject_worker
    assert controller.stems.generate_stems_async(0) is False

    source_owners = [lease for lease in leases if lease.path == str(source)]
    assert len(source_owners) == 2
    assert source_owners[0].released is False
    assert source_owners[1].released is True
    assert 0 not in controller.session.stem_generating_sample_ids
    assert "worker thread" in controller.session.stem_generation_errors[0]


@pytest.mark.parametrize("outcome", ["rejected", "status_error", "shutdown", "accepted"])
def test_previous_stem_files_survive_pending_ack_and_rejection_with_eager_cleanup(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    outcome: str,
) -> None:
    _source(controller, tmp_path)
    _install_eager_registry(audio_engine_mock)
    assert controller.stems.generate_stems_async(0)
    controller.stems.on_frame_render()
    previous = controller.project.stem_cache[0]
    assert previous is not None
    previous_path = tmp_path / previous.cache_dir
    previous_bytes = {kind: (previous_path / f"{kind}.wav").read_bytes() for kind in STEM_KINDS}
    ticket = audio_engine_mock.capture_prepared_source.return_value
    ticket.status = "pending"

    assert controller.stems.generate_stems_async(0)
    controller.stems.on_frame_render()
    pending = controller.stems._pending_stem_publications[0]
    current_path = tmp_path / pending.entry.cache_dir
    assert current_path != previous_path
    assert pending.previous_lease is not None
    assert not pending.previous_lease.released
    previous_lease = pending.previous_lease
    assert all(
        (previous_path / f"{kind}.wav").read_bytes() == content
        for kind, content in previous_bytes.items()
    )

    if outcome == "shutdown":
        controller.shut_down()
    else:
        if outcome == "status_error":

            def fail_status() -> str:
                message = "publication feedback failed"
                raise RuntimeError(message)

            ticket.publication_status = fail_status
        else:
            ticket.status = outcome
        controller.stems.on_frame_render()

    if outcome in {"rejected", "status_error"}:
        assert pending.previous_lease is None
        assert controller._assets._assignments["stems", 0][1] is previous_lease
        assert not previous_lease.released
    else:
        assert previous_lease.released
    assert pending.retirement.remaining == 0
    assert controller._assets._reserved == 0
    if outcome == "accepted":
        assert not previous_path.exists()
        assert current_path.exists()
    else:
        assert controller.project.stem_cache[0] is previous
        assert previous.available
        assert not current_path.exists()
        assert all(
            (previous_path / f"{kind}.wav").read_bytes() == content
            for kind, content in previous_bytes.items()
        )


def test_retirement_capacity_failure_preserves_intent_before_native_admission(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _source(controller, tmp_path)
    assert controller.stems.generate_stems_async(0)
    controller.stems.on_frame_render()
    previous = controller.project.stem_cache[0]
    controller._assets._MAX_PENDING_RETIREMENTS = 32
    reservation = controller._assets.reserve(32)
    try:
        controller.loader.load_sample_async(0, "new.wav")
        audio_engine_mock.load_sample_async.assert_not_called()
        assert "backlog full" in controller.session.sample_load_errors[0]
        with pytest.raises(RuntimeError, match="backlog full"):
            controller.loader.unload_sample(0)
        audio_engine_mock.unload_sample.assert_not_called()
        assert controller.stems.delete_stems(0) is False
        assert controller.stems.generate_stems_async(0) is False
        assert controller.project.sample_paths[0] == "samples/shared.wav"
        assert controller.project.stem_cache[0] is previous
    finally:
        reservation.close()


def test_repeated_same_path_retirement_owners_are_included_in_finite_bound(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    source = _source(controller, tmp_path)
    assets = controller._assets
    monkeypatch.setattr(assets, "_MAX_PENDING_RETIREMENTS", 4)
    monkeypatch.setattr(assets, "_start_retry_worker_locked", lambda: None)
    audio_engine_mock.retire_project_asset.side_effect = ValueError(
        "project asset retirement queue full"
    )
    owners = [FakeProjectAssetLease(str(source)) for _ in range(4)]
    for lease in owners[:3]:
        assets.retire(source, lease=cast("ProjectAssetLease", lease))
    with pytest.raises(RuntimeError, match="reserved retirement slot"):
        assets.retire(source, lease=cast("ProjectAssetLease", owners[3]))
    assert assets._pending_units_locked() == 4
    assert all(not lease.released for lease in owners)
    audio_engine_mock.retire_project_asset.side_effect = None
    assets.retry_retirements()
    assert all(lease.released for lease in owners[:3])
    assert not owners[3].released
    owners[3].release()


def test_sharing_blocked_admissions_cannot_starve_later_cleanup(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    assets = controller._assets
    monkeypatch.setattr(assets, "_start_retry_worker_locked", lambda: None)
    paths = [tmp_path / "samples" / f"owner-{index}.wav" for index in range(9)]
    paths[0].parent.mkdir()
    owners = [FakeProjectAssetLease(str(path)) for path in paths]
    audio_engine_mock.retire_project_asset.side_effect = RuntimeError("sharing (os error 32)")
    for path, owner in zip(paths, owners, strict=True):
        path.write_bytes(b"retained")
        assets.retire(path, lease=cast("ProjectAssetLease", owner))

    def accept_last(path: str, *, recursive: bool = False) -> None:
        if path != str(paths[-1]):
            message = "sharing (os error 32)"
            raise RuntimeError(message)

    audio_engine_mock.retire_project_asset.side_effect = accept_last
    assets.retry_retirements()
    assert not owners[-1].released
    assets.retry_retirements()
    assert owners[-1].released
    assert all(not owner.released for owner in owners[:-1])
    audio_engine_mock.retire_project_asset.side_effect = None
    assets.retry_retirements()
    assert all(owner.released for owner in owners)


def test_terminal_native_validation_preserves_bytes_and_bounds_reported_errors(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    assets = controller._assets
    samples = tmp_path / "samples"
    samples.mkdir()
    audio_engine_mock.retire_project_asset.side_effect = RuntimeError(
        "asset has no recognized assignment or reader ownership"
    )
    for index in range(20):
        path = samples / f"preserved-{index}.wav"
        path.write_bytes(b"unknown replacement")
        owner = FakeProjectAssetLease(str(path))
        assets.retire(path, lease=cast("ProjectAssetLease", owner))
        assert owner.released
        assert path.read_bytes() == b"unknown replacement"
    assert not assets._pending
    assert len(assets.retirement_errors) == 16
    assert path in assets.retirement_errors


@pytest.mark.parametrize("failure", ["malformed_wav", "value_error", "type_error", "eof"])
def test_backend_invalid_output_releases_readers_and_cleanup_reservation(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    failure: str,
) -> None:
    source = _source(controller, tmp_path)
    leases = _track_leases(audio_engine_mock)
    assert controller.stems.generate_stems_async(0)
    controller.stems.on_frame_render()
    previous = controller.project.stem_cache[0]
    audio_engine_mock.publish_prepared_stems.reset_mock()

    def reject_output(
        request: StemGenerationRequest, _progress: StemProgressCallback
    ) -> StemGenerationResult:
        if failure in {"malformed_wav", "eof"}:
            request.cache_dir.mkdir(parents=True)
            output = request.cache_dir / "vocals.wav"
            output.write_bytes(b"" if failure == "eof" else b"malformed WAV output")
            stem_generation._read_pcm16_wav(output)
        if failure == "type_error":
            message = "invalid backend result type"
            raise TypeError(message)
        message = "invalid backend sample shape"
        raise ValueError(message)

    monkeypatch.setattr(stem_backend, "generate", reject_output)
    assert controller.stems.generate_stems_async(0)
    controller.stems.on_frame_render()

    source_owners = [lease for lease in leases if lease.path == str(source)]
    assert len(source_owners) == 3
    assert not source_owners[0].released
    assert all(lease.released for lease in source_owners[1:])
    assert controller.project.stem_cache[0] is previous
    assert controller.stems.stems_available(0)
    assert 0 not in controller.session.stem_generating_sample_ids
    assert 0 in controller.session.stem_generation_errors
    assert controller.session.stem_generation_errors[0]
    assert not controller.stems._jobs
    assert controller._assets._reserved == 0
    audio_engine_mock.publish_prepared_stems.assert_not_called()


def test_unexpected_backend_exception_propagates_after_reader_and_reservation_cleanup(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    source = _source(controller, tmp_path)
    leases = _track_leases(audio_engine_mock)

    def unexpected(
        _request: StemGenerationRequest, _progress: StemProgressCallback
    ) -> StemGenerationResult:
        message = "unexpected backend bug"
        raise ArithmeticError(message)

    monkeypatch.setattr(stem_backend, "generate", unexpected)
    with pytest.raises(ArithmeticError, match="unexpected backend bug"):
        controller.stems.generate_stems_async(0)
    source_owners = [lease for lease in leases if lease.path == str(source)]
    assert len(source_owners) == 2
    assert not source_owners[0].released
    assert source_owners[1].released
    assert controller._assets._reserved == 0
    assert all(job._disposed and not job._running for job in controller.stems._jobs.values())
    audio_engine_mock.publish_prepared_stems.assert_not_called()
