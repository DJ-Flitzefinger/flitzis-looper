import hashlib
import os
from pathlib import Path
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.constants import DEFAULT_DEMUCS_OVERLAP, DEFAULT_DEMUCS_SHIFTS
from flitzis_looper.controller.stem_cache import (
    cache_dir_for_sample_id,
    expected_stem_files,
    source_version_for_sample_path,
)
from flitzis_looper.models import (
    STEM_COMPONENT_MASK,
    STEM_INSTRUMENTAL_PRESET_MASK,
    STEM_KINDS,
    STEM_MASK_BASS,
    STEM_MASK_DRUMS,
    STEM_MASK_MELODY,
    STEM_MASK_VOCALS,
    StemCacheEntry,
)
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import FakePreparedSourceTicket, write_test_stem_marker

if TYPE_CHECKING:
    from collections.abc import Callable
    from io import BufferedReader
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from tests.flitzis_looper.conftest import FakeStemGenerationBackend


def _load_project_sample(controller: AppController, tmp_path: Path, name: str = "loop.wav") -> str:
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir(exist_ok=True)
    sample_path = samples_dir / name
    write_mono_pcm16_wav(sample_path, 44_100)
    project_path = f"samples/{name}"
    controller.project.sample_paths[0] = project_path
    controller.project.sample_durations[0] = 128 / 44_100
    return project_path


def test_source_version_uses_project_path_and_full_sha256(tmp_path: Path) -> None:
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    sample_path = samples_dir / "loop.wav"
    write_mono_pcm16_wav(sample_path, 44_100)

    version = source_version_for_sample_path("samples/loop.wav", project_root=tmp_path)

    assert version is not None
    expected_digest = hashlib.sha256(sample_path.read_bytes()).hexdigest()
    assert version == f"samples/loop.wav|sha256-v1:{expected_digest}"


def test_source_version_detects_same_size_and_mtime_replacement(tmp_path: Path) -> None:
    sample = tmp_path / "source.wav"
    sample.write_bytes(b"source-a")
    stat = sample.stat()
    original = source_version_for_sample_path("source.wav", project_root=tmp_path)
    sample.write_bytes(b"source-b")
    os.utime(sample, ns=(stat.st_atime_ns, stat.st_mtime_ns))

    replacement = source_version_for_sample_path("source.wav", project_root=tmp_path)

    assert sample.stat().st_size == stat.st_size
    assert sample.stat().st_mtime_ns == stat.st_mtime_ns
    assert original is not None
    assert replacement is not None
    assert replacement != original


def test_source_version_rejects_file_change_during_streaming_hash(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sample = tmp_path / "source.wav"
    sample.write_bytes(b"source-a")
    stat = sample.stat()
    original_digest = hashlib.file_digest
    reads = 0

    def mutate_after_read(source: BufferedReader, digest: str) -> object:
        nonlocal reads
        result = original_digest(source, digest)
        reads += 1
        if reads == 1:
            sample.write_bytes(b"source-b")
            os.utime(sample, ns=(stat.st_atime_ns, stat.st_mtime_ns))
        return result

    monkeypatch.setattr(hashlib, "file_digest", mutate_after_read)

    assert source_version_for_sample_path("source.wav", project_root=tmp_path) is None


def test_cache_dir_for_sample_id_uses_pad_label() -> None:
    cache_dir = cache_dir_for_sample_id(0)

    assert cache_dir == "samples/stems/#1"
    assert "\\" not in cache_dir


def test_expected_stem_files_include_all_supported_kinds() -> None:
    files = expected_stem_files("samples/stems/cache")

    for kind in STEM_KINDS:
        assert files.path_for(kind) == f"samples/stems/cache/{kind}.wav"


def test_generate_stems_async_schedules_stopped_loaded_pad(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
) -> None:
    _load_project_sample(controller, tmp_path)

    scheduled = controller.stems.generate_stems_async(0)

    assert scheduled is True
    version = controller.session.stem_generation_source_versions[0]
    cache_dir = cache_dir_for_sample_id(0)
    audio_engine_mock.generate_stems_async.assert_not_called()
    assert len(stem_backend.requests) == 1
    assert stem_backend.requests[0].sample_id == 0
    assert stem_backend.requests[0].source_version == version
    assert stem_backend.requests[0].cache_dir.parent == tmp_path / cache_dir
    assert stem_backend.requests[0].cache_dir.name.startswith(".generation-")
    assert 0 in controller.session.stem_generating_sample_ids
    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.source_version == version
    assert entry.cache_dir == cache_dir
    assert entry.available is False
    assert stem_backend.requests[0].demucs_shifts == DEFAULT_DEMUCS_SHIFTS
    assert stem_backend.requests[0].demucs_overlap == pytest.approx(DEFAULT_DEMUCS_OVERLAP)


def test_generate_stems_async_uses_configured_demucs_quality(
    controller: AppController,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
) -> None:
    _load_project_sample(controller, tmp_path)
    controller.settings.set_demucs_quality(shifts=4, overlap=0.25)

    assert controller.stems.generate_stems_async(0) is True

    assert stem_backend.requests[0].demucs_shifts == 4
    assert stem_backend.requests[0].demucs_overlap == pytest.approx(0.25)


@pytest.mark.parametrize(
    ("shifts", "overlap", "expected"),
    [
        (-1, 0.5, "demucs shifts"),
        (21, 0.5, "demucs shifts"),
        (10, -0.1, "demucs overlap"),
        (10, 1.0, "demucs overlap"),
    ],
)
def test_set_demucs_quality_rejects_invalid_values(
    controller: AppController,
    shifts: int,
    overlap: float,
    expected: str,
) -> None:
    with pytest.raises(ValueError, match=expected):
        controller.settings.set_demucs_quality(shifts=shifts, overlap=overlap)

    assert controller.project.demucs_shifts == DEFAULT_DEMUCS_SHIFTS
    assert controller.project.demucs_overlap == pytest.approx(DEFAULT_DEMUCS_OVERLAP)


@pytest.mark.parametrize(
    ("field_name", "expected"),
    [
        ("active_sample_ids", "playing"),
        ("loading_sample_ids", "loading"),
        ("analyzing_sample_ids", "another pad task"),
        ("stem_generating_sample_ids", "already running"),
    ],
)
def test_generate_stems_async_rejects_conflicting_pad_state(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    field_name: str,
    expected: str,
) -> None:
    _load_project_sample(controller, tmp_path)
    getattr(controller.session, field_name).add(0)

    scheduled = controller.stems.generate_stems_async(0)

    assert scheduled is False
    audio_engine_mock.generate_stems_async.assert_not_called()
    assert expected in controller.session.stem_generation_errors[0]


def test_generate_stems_async_rejects_missing_loaded_source(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    scheduled = controller.stems.generate_stems_async(0)

    assert scheduled is False
    audio_engine_mock.generate_stems_async.assert_not_called()
    assert "loaded sample" in controller.session.stem_generation_errors[0]


def test_generate_stems_async_rejects_missing_source_file(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/missing.wav"

    scheduled = controller.stems.generate_stems_async(0)

    assert scheduled is False
    audio_engine_mock.generate_stems_async.assert_not_called()
    assert "source file is missing" in controller.session.stem_generation_errors[0]


def test_stem_generation_block_reason_reports_non_io_gate(
    controller: AppController, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    controller.session.active_sample_ids.add(0)

    blocker = controller.stems.stem_generation_block_reason(0)

    assert blocker == "Cannot generate stems while the pad is playing"


def test_stem_grid_indicator_state_uses_session_and_project_snapshots(
    controller: AppController, tmp_path: Path
) -> None:
    assert controller.stems.stem_grid_indicator_state(0) is None

    _load_project_sample(controller, tmp_path)
    controller.session.active_sample_ids.add(0)
    assert controller.stems.stem_grid_indicator_state(0) == "blocked"

    controller.session.stem_generation_errors[0] = "failed"
    assert controller.stems.stem_grid_indicator_state(0) == "error"

    controller.session.stem_generation_errors.pop(0)
    controller.session.active_sample_ids.clear()
    controller.session.stem_generating_sample_ids.add(0)
    assert controller.stems.stem_grid_indicator_state(0) == "generating"

    controller.session.stem_generating_sample_ids.clear()
    version = source_version_for_sample_path(controller.project.sample_paths[0] or "")
    assert version is not None
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=version,
        cache_dir=cache_dir_for_sample_id(0),
        stems=expected_stem_files(cache_dir_for_sample_id(0)),
        available=True,
    )
    assert controller.stems.stem_grid_indicator_state(0) == "available"


def test_generate_stems_async_backend_error_clears_running_state(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
) -> None:
    _load_project_sample(controller, tmp_path)
    stem_backend.error = RuntimeError("Demucs unavailable")

    scheduled = controller.stems.generate_stems_async(0)
    controller.stems.on_frame_render()

    assert scheduled is True
    assert 0 not in controller.session.stem_generating_sample_ids
    assert 0 not in controller.session.stem_generation_source_versions
    assert controller.session.stem_generation_errors[0] == "Demucs unavailable"
    audio_engine_mock.publish_prepared_stems.assert_not_called()


def test_stem_backend_success_publishes_prepared_stems(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)

    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()

    version = source_version_for_sample_path("samples/loop.wav")
    assert version is not None
    cache_dir = cache_dir_for_sample_id(0)
    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is True
    audio_engine_mock.publish_prepared_stems.assert_called_once_with(
        0, version, cache_dir, audio_engine_mock.capture_prepared_source.return_value
    )
    assert 0 not in controller.session.stem_generating_sample_ids


def test_stem_publication_retains_admission_ticket_when_timing_changes(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    ticket = FakePreparedSourceTicket()
    audio_engine_mock.capture_prepared_source.return_value = ticket
    assert controller.stems.generate_stems_async(0) is True
    audio_engine_mock.publish_prepared_stems.side_effect = ValueError("stale timing publication")

    controller.stems.on_frame_render()

    version = source_version_for_sample_path("samples/loop.wav")
    audio_engine_mock.capture_prepared_source.assert_called_once_with(0, version)
    audio_engine_mock.publish_prepared_stems.assert_called_once_with(
        0, version, cache_dir_for_sample_id(0), ticket
    )
    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is False
    assert "stale timing" in controller.session.stem_generation_errors[0]


def test_stem_availability_and_preference_wait_for_callback_acceptance(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _load_project_sample(controller, tmp_path)
    ticket = FakePreparedSourceTicket("pending")
    audio_engine_mock.capture_prepared_source.return_value = ticket
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()

    assert controller.stems.stems_available(0) is False
    assert "another pad task" in (controller.stems.stem_generation_block_reason(0) or "")
    audio_engine_mock.set_stem_mix_mode.assert_not_called()
    audio_engine_mock.set_stem_enabled_mask.assert_not_called()
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "stem_generation"},
        {"type": "task_error", "id": 0, "task": "stem_generation", "msg": "obsolete"},
        None,
    ]
    controller.loader.poll_loader_events()
    assert controller.session.stem_generating_sample_ids == set()
    assert controller.session.stem_generation_errors == {}

    def forbid_file_hash(*_args: object, **_kwargs: object) -> None:
        msg = "Publication acknowledgement must not read audio files"
        raise AssertionError(msg)

    monkeypatch.setattr(
        "flitzis_looper.controller.stems.source_version_for_sample_path", forbid_file_hash
    )
    monkeypatch.setattr(
        "flitzis_looper.controller.stems.verified_stem_cache_available", forbid_file_hash
    )
    controller.stems.on_frame_render()
    assert controller.stems.stems_available(0) is False

    ticket.status = "accepted"
    controller.stems.on_frame_render()

    assert controller.stems.stems_available(0) is True
    entry = controller.project.stem_cache[0]
    assert entry is not None
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(
        0, "all_stems", entry.source_version
    )
    audio_engine_mock.set_stem_enabled_mask.assert_called_once_with(
        0, STEM_COMPONENT_MASK, entry.source_version
    )
    controller.stems.on_frame_render()
    assert audio_engine_mock.set_stem_mix_mode.call_count == 1


def test_late_callback_rejection_keeps_stems_unavailable(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    ticket = FakePreparedSourceTicket("pending")
    audio_engine_mock.capture_prepared_source.return_value = ticket
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    assert controller.stems.stems_available(0) is False

    ticket.status = "rejected"
    controller.stems.on_frame_render()

    assert controller.stems.stems_available(0) is False
    assert "native source/request/timing" in controller.session.stem_generation_errors[0]
    audio_engine_mock.set_stem_mix_mode.assert_not_called()
    audio_engine_mock.set_stem_enabled_mask.assert_not_called()


def test_pending_acceptance_cannot_promote_new_same_source_assignment(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    old_ticket = FakePreparedSourceTicket("pending")
    current_ticket = FakePreparedSourceTicket("pending")
    audio_engine_mock.capture_prepared_source.side_effect = [old_ticket, current_ticket]
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    assert controller.stems.stems_available(0) is False
    old_entry = controller.project.stem_cache[0]
    controller.loader.unload_sample(0)
    _load_project_sample(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    current_entry = controller.project.stem_cache[0]
    assert old_entry is not None
    assert current_entry is not None
    assert current_entry.source_version == old_entry.source_version

    old_ticket.status = "accepted"
    controller.stems.on_frame_render()

    assert controller.stems.stems_available(0) is False
    current_ticket.status = "accepted"
    controller.stems.on_frame_render()
    assert controller.stems.stems_available(0) is True


def test_stem_performance_controls_use_native_identity_without_audio_file_hashing(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _load_project_sample(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    audio_engine_mock.capture_prepared_source.reset_mock()
    entry = controller.project.stem_cache[0]
    assert entry is not None

    def forbid_file_hash(*_args: object, **_kwargs: object) -> None:
        msg = "Stem performance controls must not read source or cache audio files"
        raise AssertionError(msg)

    monkeypatch.setattr(
        "flitzis_looper.controller.stems.source_version_for_sample_path", forbid_file_hash
    )
    monkeypatch.setattr(
        "flitzis_looper.controller.stems.verified_stem_cache_available", forbid_file_hash
    )

    assert controller.stems.set_stem_mix_mode(0, "all_stems") is True
    assert controller.stems.set_stem_enabled_mask(0, STEM_MASK_VOCALS) is True
    assert controller.stems.publish_stem_mix_mode_if_available(0) is True
    assert controller.stems.publish_stem_enabled_mask_if_available(0) is True

    assert audio_engine_mock.capture_prepared_source.call_count == 4
    for call in audio_engine_mock.capture_prepared_source.call_args_list:
        assert call.args == (0, entry.source_version)


def test_stem_performance_controls_invalidate_metadata_when_native_source_changed(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    audio_engine_mock.capture_prepared_source.side_effect = ValueError("loaded content changed")
    audio_engine_mock.set_stem_mix_mode.reset_mock()

    assert controller.stems.set_stem_mix_mode(0, "all_stems") is False

    assert controller.stems.stems_available(0) is False
    assert "loaded content changed" in controller.session.stem_generation_errors[0]
    audio_engine_mock.set_stem_mix_mode.assert_not_called()


def test_same_source_reload_does_not_accept_old_job_completion(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
) -> None:
    _load_project_sample(controller, tmp_path)
    targets: list[Callable[[], None]] = []
    controller.stems._stem_task_runner = targets.append
    old_ticket, current_ticket = FakePreparedSourceTicket(), FakePreparedSourceTicket()
    audio_engine_mock.capture_prepared_source.side_effect = [old_ticket, current_ticket]
    assert controller.stems.generate_stems_async(0) is True
    original_version = controller.session.stem_generation_source_versions[0]
    controller.loader.unload_sample(0)
    _load_project_sample(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    assert controller.session.stem_generation_source_versions[0] == original_version

    targets[0]()
    controller.stems.on_frame_render()

    assert 0 in controller.session.stem_generating_sample_ids
    assert controller.session.stem_generation_source_versions[0] == original_version
    assert not stem_backend.requests[0].cache_dir.exists()
    audio_engine_mock.publish_prepared_stems.assert_not_called()
    targets[1]()
    controller.stems.on_frame_render()
    audio_engine_mock.publish_prepared_stems.assert_called_once_with(
        0, original_version, cache_dir_for_sample_id(0), current_ticket
    )
    assert not stem_backend.requests[1].cache_dir.exists()


def test_obsolete_worker_cannot_overwrite_current_canonical_stems(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
) -> None:
    _load_project_sample(controller, tmp_path)
    targets: list[Callable[[], None]] = []
    controller.stems._stem_task_runner = targets.append
    audio_engine_mock.capture_prepared_source.side_effect = [
        FakePreparedSourceTicket(),
        FakePreparedSourceTicket(),
    ]
    assert controller.stems.generate_stems_async(0) is True
    controller.loader.unload_sample(0)
    _load_project_sample(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    targets[1]()
    controller.stems.on_frame_render()
    current_files = {
        kind: (tmp_path / cache_dir_for_sample_id(0) / f"{kind}.wav").read_bytes()
        for kind in STEM_KINDS
    }

    stem_backend.sample_value = -1024
    targets[0]()
    controller.stems.on_frame_render()

    for kind, expected in current_files.items():
        assert (tmp_path / cache_dir_for_sample_id(0) / f"{kind}.wav").read_bytes() == expected
    assert not stem_backend.requests[1].cache_dir.exists()
    assert audio_engine_mock.publish_prepared_stems.call_count == 1


def test_legacy_native_events_cannot_complete_or_abort_current_python_job(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    targets: list[Callable[[], None]] = []
    controller.stems._stem_task_runner = targets.append
    assert controller.stems.generate_stems_async(0) is True
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "stem_generation"},
        {"type": "task_progress", "id": 0, "task": "stem_generation", "percent": 0.75},
        {"type": "task_success", "id": 0, "task": "stem_generation"},
        {"type": "task_error", "id": 0, "task": "stem_generation", "msg": "old failure"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 in controller.session.stem_generating_sample_ids
    assert controller.session.stem_generation_errors == {}
    assert controller.session.stem_generation_progress == {}
    audio_engine_mock.publish_prepared_stems.assert_not_called()
    targets[0]()
    controller.stems.on_frame_render()
    assert audio_engine_mock.publish_prepared_stems.call_count == 1


@pytest.mark.parametrize(
    "failure", [RuntimeError("changed source"), TypeError("missing API"), None]
)
def test_stem_generation_fails_closed_when_admission_fails(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    failure: BaseException | None,
) -> None:
    _load_project_sample(controller, tmp_path)
    audio_engine_mock.capture_prepared_source.side_effect = failure
    audio_engine_mock.capture_prepared_source.return_value = None

    assert controller.stems.generate_stems_async(0) is False

    assert stem_backend.requests == []
    assert controller.session.stem_generating_sample_ids == set()
    assert controller.session.stem_generation_source_versions == {}
    assert "Stem admission failed" in controller.session.stem_generation_errors[0]
    audio_engine_mock.publish_prepared_stems.assert_not_called()


def test_stem_generation_requires_native_admission_api(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
) -> None:
    _load_project_sample(controller, tmp_path)
    del audio_engine_mock.capture_prepared_source

    assert controller.stems.generate_stems_async(0) is False

    assert stem_backend.requests == []
    assert "Stem admission failed" in controller.session.stem_generation_errors[0]
    assert controller.session.stem_generating_sample_ids == set()


def test_restore_stem_cache_marks_missing_files_unavailable(
    controller: AppController, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path)
    version = source_version_for_sample_path(project_path)
    assert version is not None

    cache_dir = cache_dir_for_sample_id(0)
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )

    controller.stems.restore_stem_cache_from_project_state()

    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is False


def test_restore_complete_cache_waits_for_native_acceptance(
    controller: AppController, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path)
    version = source_version_for_sample_path(project_path)
    assert version is not None

    cache_dir = cache_dir_for_sample_id(0)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")
    write_test_stem_marker(stems_dir, version)

    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=False,
    )

    controller.stems.restore_stem_cache_from_project_state()

    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is False
    assert controller.stems.publish_restored_stem_cache_if_available(0) is True
    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is True


def test_restored_cache_uses_fresh_native_ticket_for_loaded_source(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    old_ticket = FakePreparedSourceTicket()
    audio_engine_mock.capture_prepared_source.return_value = old_ticket
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    audio_engine_mock.publish_prepared_stems.reset_mock()
    audio_engine_mock.capture_prepared_source.reset_mock()
    restored_ticket = FakePreparedSourceTicket()
    audio_engine_mock.capture_prepared_source.return_value = restored_ticket

    assert controller.stems.publish_restored_stem_cache_if_available(0) is True

    version = source_version_for_sample_path("samples/loop.wav")
    audio_engine_mock.capture_prepared_source.assert_called_once_with(0, version)
    audio_engine_mock.publish_prepared_stems.assert_called_once_with(
        0, version, cache_dir_for_sample_id(0), restored_ticket
    )


def test_restored_cache_admission_failure_preserves_full_mix(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    audio_engine_mock.publish_prepared_stems.reset_mock()
    audio_engine_mock.capture_prepared_source.side_effect = ValueError("changed loaded source")

    assert controller.stems.publish_restored_stem_cache_if_available(0) is False

    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is False
    assert controller.project.sample_paths[0] == "samples/loop.wav"
    assert "Restored stem publication failed" in controller.session.stem_generation_errors[0]
    audio_engine_mock.publish_prepared_stems.assert_not_called()


def test_restored_cache_waits_for_callback_acknowledgement(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    ticket = FakePreparedSourceTicket("pending")
    audio_engine_mock.capture_prepared_source.return_value = ticket
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    audio_engine_mock.set_stem_mix_mode.reset_mock()
    controller.stems.restore_stem_cache_from_project_state()
    assert controller.stems.stems_available(0) is False

    assert controller.stems.publish_restored_stem_cache_if_available(0) is True

    assert controller.stems.stems_available(0) is False
    audio_engine_mock.set_stem_mix_mode.assert_not_called()
    ticket.status = "accepted"
    controller.stems.on_frame_render()
    assert controller.stems.stems_available(0) is True
    assert audio_engine_mock.set_stem_mix_mode.call_count == 1


def test_restore_requires_complete_set_marker(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    audio_engine_mock.publish_prepared_stems.reset_mock()
    (tmp_path / cache_dir_for_sample_id(0) / ".complete.json").unlink()

    controller.stems.restore_stem_cache_from_project_state()
    assert controller.stems.publish_restored_stem_cache_if_available(0) is True

    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is False
    audio_engine_mock.publish_prepared_stems.assert_not_called()


def test_restore_rejects_changed_stem_bytes_despite_complete_file_set(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    audio_engine_mock.publish_prepared_stems.reset_mock()
    vocals = tmp_path / cache_dir_for_sample_id(0) / "vocals.wav"
    original = vocals.read_bytes()
    vocals.write_bytes(original[:-2] + b"\x00\x00")

    controller.stems.restore_stem_cache_from_project_state()
    assert controller.stems.publish_restored_stem_cache_if_available(0) is True

    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is False
    audio_engine_mock.publish_prepared_stems.assert_not_called()


def test_interrupted_promotion_cannot_restore_mixed_generation(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _load_project_sample(controller, tmp_path)
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    audio_engine_mock.publish_prepared_stems.reset_mock()
    original_replace = Path.replace

    def fail_after_first_artifact(source: Path, target: Path) -> Path:
        if source.name == "drums.wav" and source.parent.name.startswith(".generation-"):
            msg = "interrupted artifact promotion"
            raise OSError(msg)
        return original_replace(source, target)

    monkeypatch.setattr(Path, "replace", fail_after_first_artifact)
    stem_backend.sample_value = -1024
    assert controller.stems.generate_stems_async(0) is True

    controller.stems.on_frame_render()
    controller.stems.restore_stem_cache_from_project_state()
    assert controller.stems.publish_restored_stem_cache_if_available(0) is True

    assert not (tmp_path / cache_dir_for_sample_id(0) / ".complete.json").exists()
    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is False
    assert "interrupted artifact promotion" in controller.session.stem_generation_errors[0]
    audio_engine_mock.publish_prepared_stems.assert_not_called()


def test_restore_invalidates_legacy_stat_identity_without_promoting_timing(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    stat = (tmp_path / "samples/loop.wav").stat()
    cache_dir = cache_dir_for_sample_id(0)
    cache = tmp_path / cache_dir
    cache.mkdir(parents=True)
    for kind in STEM_KINDS:
        (cache / f"{kind}.wav").write_bytes(b"stem")
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=f"samples/loop.wav|{stat.st_size}|{stat.st_mtime_ns}",
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    controller.project.manual_bpm[0] = 123.5

    controller.stems.restore_stem_cache_from_project_state()
    assert controller.stems.publish_restored_stem_cache_if_available(0) is True

    assert controller.project.stem_cache[0] is None
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert controller.project.manual_bpm[0] == 123.5
    audio_engine_mock.capture_prepared_source.assert_not_called()
    audio_engine_mock.publish_prepared_stems.assert_not_called()


def test_restore_stem_cache_clears_stale_source_version(
    controller: AppController, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path, "loop-a.wav")
    old_version = source_version_for_sample_path(project_path)
    assert old_version is not None
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=old_version,
        cache_dir=cache_dir_for_sample_id(0),
    )

    _load_project_sample(controller, tmp_path, "loop-b.wav")

    controller.stems.restore_stem_cache_from_project_state()

    assert controller.project.stem_cache[0] is None


def test_invalidate_stem_cache_clears_cache_and_generation_state(
    controller: AppController,
) -> None:
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version="samples/loop.wav|10|20",
        cache_dir="samples/stems/cache",
    )
    controller.session.stem_generating_sample_ids.add(0)
    controller.session.stem_generation_source_versions[0] = "samples/loop.wav|10|20"
    controller.session.stem_generation_progress[0] = 0.5
    controller.session.stem_generation_stage[0] = "Generating"
    controller.session.stem_generation_errors[0] = "old"
    controller.session.pad_stem_enabled_mask[0] = STEM_MASK_VOCALS
    controller.session.pad_stem_last_custom_mask[0] = STEM_MASK_VOCALS
    controller.session.pad_stem_mask_display_mode[0] = "custom"

    controller.stems.invalidate_stem_cache(0)

    assert controller.project.stem_cache[0] is None
    assert 0 not in controller.session.stem_generating_sample_ids
    assert 0 not in controller.session.stem_generation_source_versions
    assert 0 not in controller.session.stem_generation_progress
    assert 0 not in controller.session.stem_generation_stage
    assert 0 not in controller.session.stem_generation_errors
    assert controller.session.pad_stem_enabled_mask[0] == STEM_COMPONENT_MASK
    assert controller.session.pad_stem_last_custom_mask[0] == STEM_COMPONENT_MASK
    assert controller.session.pad_stem_mask_display_mode[0] == "all"


def test_delete_stems_removes_pad_cache_and_resets_mix_state(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
) -> None:
    cache_dir = cache_dir_for_sample_id(0)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")

    controller.project.stem_cache[0] = StemCacheEntry(
        source_version="samples/loop.wav|10|20",
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    controller.session.stem_generating_sample_ids.add(0)
    controller.session.pad_stem_enabled_mask[0] = STEM_MASK_VOCALS
    controller.session.pad_stem_last_custom_mask[0] = STEM_MASK_VOCALS
    controller.session.pad_stem_mask_display_mode[0] = "custom"

    deleted = controller.stems.delete_stems(0)

    assert deleted is True
    assert not stems_dir.exists()
    assert controller.project.stem_cache[0] is None
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert controller.stems.has_stem_cache(0) is False
    assert 0 not in controller.session.stem_generating_sample_ids
    assert controller.session.pad_stem_enabled_mask[0] == STEM_COMPONENT_MASK
    assert controller.session.pad_stem_last_custom_mask[0] == STEM_COMPONENT_MASK
    assert controller.session.pad_stem_mask_display_mode[0] == "all"
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "full_mix")


def test_stem_mix_mode_defaults_to_full_mix(controller: AppController) -> None:
    assert controller.stems.stem_mix_mode(0) == "full_mix"
    assert controller.stems.stems_available(0) is False
    assert controller.stems.stem_enabled_mask(0) == STEM_COMPONENT_MASK
    assert controller.session.pad_stem_last_custom_mask[0] == STEM_COMPONENT_MASK
    assert controller.stems.stem_mask_display_mode(0) == "all"
    assert controller.stems.stem_mask_controls_enabled(0) is False


def test_set_stem_mix_mode_full_mix_updates_project_and_audio(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.pad_stem_mix_mode[0] = "all_stems"

    updated = controller.stems.set_stem_mix_mode(0, "full_mix")

    assert updated is True
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "full_mix")


def test_set_stem_mix_mode_all_stems_rejects_without_available_cache(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    updated = controller.stems.set_stem_mix_mode(0, "all_stems")

    assert updated is False
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    audio_engine_mock.set_stem_mix_mode.assert_not_called()
    audio_engine_mock.set_stem_enabled_mask.assert_not_called()


def test_set_stem_mix_mode_all_stems_publishes_current_available_cache(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path)
    version = source_version_for_sample_path(project_path)
    assert version is not None

    cache_dir = cache_dir_for_sample_id(0)
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )

    updated = controller.stems.set_stem_mix_mode(0, "all_stems")

    assert updated is True
    assert controller.project.pad_stem_mix_mode[0] == "all_stems"
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "all_stems", version)
    audio_engine_mock.set_stem_enabled_mask.assert_called_once_with(0, STEM_COMPONENT_MASK, version)


def test_set_stem_mix_mode_all_stems_keeps_full_mix_when_mode_publish_fails(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path)
    version = source_version_for_sample_path(project_path)
    assert version is not None

    cache_dir = cache_dir_for_sample_id(0)
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )
    audio_engine_mock.set_stem_mix_mode.side_effect = RuntimeError("buffer may be full")

    updated = controller.stems.set_stem_mix_mode(0, "all_stems")

    assert updated is False
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert controller.session.pad_stem_enabled_mask[0] == STEM_COMPONENT_MASK
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "all_stems", version)
    audio_engine_mock.set_stem_enabled_mask.assert_not_called()
    assert "Stem mix update failed" in controller.session.stem_generation_errors[0]


def test_set_stem_mix_mode_all_stems_keeps_full_mix_when_mask_publish_fails(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path)
    version = source_version_for_sample_path(project_path)
    assert version is not None

    cache_dir = cache_dir_for_sample_id(0)
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )
    controller.session.pad_stem_enabled_mask[0] = STEM_MASK_VOCALS
    audio_engine_mock.set_stem_enabled_mask.side_effect = RuntimeError("buffer may be full")

    updated = controller.stems.set_stem_mix_mode(0, "all_stems")

    assert updated is False
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert controller.session.pad_stem_enabled_mask[0] == STEM_MASK_VOCALS
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "all_stems", version)
    audio_engine_mock.set_stem_enabled_mask.assert_called_once_with(0, STEM_MASK_VOCALS, version)
    assert "Stem mask update failed" in controller.session.stem_generation_errors[0]


def test_stem_mask_controls_require_available_all_stems(
    controller: AppController, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path)
    version = source_version_for_sample_path(project_path)
    assert version is not None
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=version,
        cache_dir=cache_dir_for_sample_id(0),
        stems=expected_stem_files(cache_dir_for_sample_id(0)),
        available=True,
    )

    assert controller.stems.stem_mask_controls_enabled(0) is False

    controller.project.pad_stem_mix_mode[0] = "all_stems"

    assert controller.stems.stem_mask_controls_enabled(0) is True


def test_set_stem_enabled_mask_updates_session_and_audio(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path)
    version = source_version_for_sample_path(project_path)
    assert version is not None
    cache_dir = cache_dir_for_sample_id(0)
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )

    updated = controller.stems.set_stem_enabled_mask(
        0, STEM_INSTRUMENTAL_PRESET_MASK, "instrumental"
    )

    assert updated is True
    assert controller.session.pad_stem_enabled_mask[0] == STEM_INSTRUMENTAL_PRESET_MASK
    assert controller.session.pad_stem_mask_display_mode[0] == "instrumental"
    audio_engine_mock.set_stem_enabled_mask.assert_called_once_with(
        0, STEM_INSTRUMENTAL_PRESET_MASK, version
    )


def test_set_stem_enabled_mask_does_not_publish_without_available_cache(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    updated = controller.stems.set_stem_enabled_mask(0, STEM_MASK_VOCALS, "custom")

    assert updated is True
    assert controller.session.pad_stem_enabled_mask[0] == STEM_MASK_VOCALS
    assert controller.session.pad_stem_last_custom_mask[0] == STEM_MASK_VOCALS
    assert controller.session.pad_stem_mask_display_mode[0] == "custom"
    audio_engine_mock.set_stem_enabled_mask.assert_not_called()


def test_set_stem_enabled_mask_remembers_custom_mask_across_presets(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    custom_mask = STEM_MASK_VOCALS | STEM_MASK_BASS

    assert controller.stems.set_stem_enabled_mask(0, custom_mask, "custom") is True
    assert controller.session.pad_stem_last_custom_mask[0] == custom_mask

    assert (
        controller.stems.set_stem_enabled_mask(
            0,
            STEM_INSTRUMENTAL_PRESET_MASK,
            "instrumental",
        )
        is True
    )
    assert controller.stems.stem_enabled_mask(0) == STEM_INSTRUMENTAL_PRESET_MASK
    assert controller.stems.stem_mask_display_mode(0) == "instrumental"
    assert controller.session.pad_stem_last_custom_mask[0] == custom_mask

    assert controller.stems.set_stem_enabled_mask(0, STEM_COMPONENT_MASK, "all") is True
    assert controller.stems.stem_enabled_mask(0) == STEM_COMPONENT_MASK
    assert controller.stems.stem_mask_display_mode(0) == "all"
    assert controller.session.pad_stem_last_custom_mask[0] == custom_mask

    assert controller.stems.set_stem_enabled_mask(0, custom_mask, "custom") is True
    assert controller.stems.stem_enabled_mask(0) == custom_mask
    assert controller.stems.stem_mask_display_mode(0) == "custom"
    assert controller.session.pad_stem_last_custom_mask[0] == custom_mask
    audio_engine_mock.set_stem_enabled_mask.assert_not_called()


def test_set_stem_enabled_mask_updates_remembered_mask_when_leaving_custom(
    controller: AppController,
) -> None:
    custom_mask = STEM_MASK_DRUMS | STEM_MASK_MELODY
    controller.session.pad_stem_enabled_mask[0] = custom_mask
    controller.session.pad_stem_last_custom_mask[0] = STEM_MASK_VOCALS
    controller.session.pad_stem_mask_display_mode[0] = "custom"

    updated = controller.stems.set_stem_enabled_mask(
        0,
        STEM_INSTRUMENTAL_PRESET_MASK,
        "instrumental",
    )

    assert updated is True
    assert controller.session.pad_stem_last_custom_mask[0] == custom_mask


def test_set_stem_enabled_mask_rejects_invalid_mask(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    with pytest.raises(ValueError, match="stem enabled mask"):
        controller.stems.set_stem_enabled_mask(0, 1 << 4)

    audio_engine_mock.set_stem_enabled_mask.assert_not_called()


def test_set_stem_enabled_mask_records_audio_error(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path)
    version = source_version_for_sample_path(project_path)
    assert version is not None
    cache_dir = cache_dir_for_sample_id(0)
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )
    audio_engine_mock.set_stem_enabled_mask.side_effect = RuntimeError("buffer may be full")

    updated = controller.stems.set_stem_enabled_mask(0, STEM_MASK_VOCALS)

    assert updated is False
    assert "Stem mask update failed" in controller.session.stem_generation_errors[0]


def test_set_stem_mix_mode_all_stems_rejects_stale_cache(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version="samples/old.wav|1|2",
        cache_dir="samples/stems/cache",
        available=True,
    )
    audio_engine_mock.capture_prepared_source.side_effect = ValueError("loaded source mismatch")

    updated = controller.stems.set_stem_mix_mode(0, "all_stems")

    assert updated is False
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    audio_engine_mock.set_stem_mix_mode.assert_not_called()


def test_set_stem_mix_mode_rejects_invalid_mode(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    set_stem_mix_mode = cast(
        "Callable[[int, str], bool]",
        controller.stems.set_stem_mix_mode,
    )

    with pytest.raises(ValueError, match="stem mix mode"):
        set_stem_mix_mode(0, "half_stems")

    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    audio_engine_mock.set_stem_mix_mode.assert_not_called()


def test_publish_stem_mix_mode_records_audio_error(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path)
    version = source_version_for_sample_path(project_path)
    assert version is not None

    cache_dir = cache_dir_for_sample_id(0)
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )
    audio_engine_mock.set_stem_mix_mode.side_effect = RuntimeError("buffer may be full")

    published = controller.stems.publish_stem_mix_mode_if_available(0)

    assert published is False
    assert "Stem mix update failed" in controller.session.stem_generation_errors[0]
