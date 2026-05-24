from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller.stems import (
    cache_dir_for_source_version,
    expected_stem_files,
    source_version_for_sample_path,
)
from flitzis_looper.models import STEM_KINDS, StemCacheEntry
from tests.conftest import write_mono_pcm16_wav

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def _load_project_sample(controller: AppController, tmp_path: Path, name: str = "loop.wav") -> str:
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir(exist_ok=True)
    sample_path = samples_dir / name
    write_mono_pcm16_wav(sample_path, 44_100)
    project_path = f"samples/{name}"
    controller.project.sample_paths[0] = project_path
    return project_path


def test_source_version_uses_project_path_size_and_mtime(tmp_path: Path) -> None:
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    sample_path = samples_dir / "loop.wav"
    write_mono_pcm16_wav(sample_path, 44_100)

    version = source_version_for_sample_path("samples/loop.wav", project_root=tmp_path)

    assert version is not None
    assert version.startswith("samples/loop.wav|")


def test_cache_dir_for_source_version_is_project_local() -> None:
    cache_dir = cache_dir_for_source_version("samples/loop.wav|10|20")

    assert cache_dir.startswith("samples/stems/")
    assert "\\" not in cache_dir


def test_expected_stem_files_include_all_supported_kinds() -> None:
    files = expected_stem_files("samples/stems/cache")

    for kind in STEM_KINDS:
        assert files.path_for(kind) == f"samples/stems/cache/{kind}.wav"


def test_generate_stems_async_schedules_stopped_loaded_pad(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)

    scheduled = controller.stems.generate_stems_async(0)

    assert scheduled is True
    version = controller.session.stem_generation_source_versions[0]
    cache_dir = cache_dir_for_source_version(version)
    audio_engine_mock.generate_stems_async.assert_called_once_with(0, version, cache_dir)
    assert 0 in controller.session.stem_generating_sample_ids
    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.source_version == version
    assert entry.cache_dir == cache_dir
    assert entry.available is False


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


def test_generate_stems_async_runtime_error_clears_running_state(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    audio_engine_mock.generate_stems_async.side_effect = RuntimeError("engine busy")

    scheduled = controller.stems.generate_stems_async(0)

    assert scheduled is False
    assert 0 not in controller.session.stem_generating_sample_ids
    assert 0 not in controller.session.stem_generation_source_versions
    assert controller.session.stem_generation_errors[0] == "engine busy"


def test_restore_stem_cache_marks_missing_files_unavailable(
    controller: AppController, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path)
    version = source_version_for_sample_path(project_path)
    assert version is not None

    cache_dir = cache_dir_for_source_version(version)
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


def test_restore_stem_cache_keeps_complete_current_cache(
    controller: AppController, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path)
    version = source_version_for_sample_path(project_path)
    assert version is not None

    cache_dir = cache_dir_for_source_version(version)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")

    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=False,
    )

    controller.stems.restore_stem_cache_from_project_state()

    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is True


def test_restore_stem_cache_clears_stale_source_version(
    controller: AppController, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path, "loop-a.wav")
    old_version = source_version_for_sample_path(project_path)
    assert old_version is not None
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=old_version,
        cache_dir=cache_dir_for_source_version(old_version),
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

    controller.stems.invalidate_stem_cache(0)

    assert controller.project.stem_cache[0] is None
    assert 0 not in controller.session.stem_generating_sample_ids
    assert 0 not in controller.session.stem_generation_source_versions
    assert 0 not in controller.session.stem_generation_progress
    assert 0 not in controller.session.stem_generation_stage
    assert 0 not in controller.session.stem_generation_errors


def test_stem_mix_mode_defaults_to_full_mix(controller: AppController) -> None:
    assert controller.stems.stem_mix_mode(0) == "full_mix"
    assert controller.stems.stems_available(0) is False


def test_set_stem_mix_mode_full_mix_updates_project_and_audio(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.pad_stem_mix_mode[0] = "all_stems"

    updated = controller.stems.set_stem_mix_mode(0, "full_mix")

    assert updated is True
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "full_mix")


def test_set_stem_mix_mode_all_stems_persists_without_available_cache(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    updated = controller.stems.set_stem_mix_mode(0, "all_stems")

    assert updated is True
    assert controller.project.pad_stem_mix_mode[0] == "all_stems"
    audio_engine_mock.set_stem_mix_mode.assert_not_called()


def test_set_stem_mix_mode_all_stems_publishes_current_available_cache(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    project_path = _load_project_sample(controller, tmp_path)
    version = source_version_for_sample_path(project_path)
    assert version is not None

    cache_dir = cache_dir_for_source_version(version)
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


def test_set_stem_mix_mode_all_stems_does_not_publish_stale_cache(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _load_project_sample(controller, tmp_path)
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version="samples/old.wav|1|2",
        cache_dir="samples/stems/cache",
        available=True,
    )

    updated = controller.stems.set_stem_mix_mode(0, "all_stems")

    assert updated is True
    assert controller.project.pad_stem_mix_mode[0] == "all_stems"
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

    cache_dir = cache_dir_for_source_version(version)
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
