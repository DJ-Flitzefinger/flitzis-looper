import urllib.request
from typing import TYPE_CHECKING
from unittest.mock import Mock

from flitzis_looper.controller import AppController, bs_roformer_assets
from flitzis_looper.controller.stem_separators import SelectedStemGenerationBackend
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import FakeStemGenerationBackend

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

    import pytest

    from flitzis_looper.controller.stem_generation import StemGenerationBackend
    from flitzis_looper.models import StemSeparator


def _forbid_downloads(monkeypatch: pytest.MonkeyPatch) -> Mock:
    forbidden = Mock(side_effect=AssertionError("generation must not download or install models"))
    monkeypatch.setattr(bs_roformer_assets, "install_model_assets", forbidden)
    monkeypatch.setattr(urllib.request, "urlopen", forbidden)
    return forbidden


def _load_pad(controller: AppController, tmp_path: Path, sample_id: int) -> None:
    samples = tmp_path / "samples"
    samples.mkdir(exist_ok=True)
    name = f"loop-{sample_id}.wav"
    write_mono_pcm16_wav(samples / name, 44_100)
    controller.project.sample_paths[sample_id] = f"samples/{name}"
    controller.project.sample_durations[sample_id] = 128 / 44_100
    controller._audio.load_sample_async(sample_id, f"samples/{name}")


def test_queued_model_intent_is_fenced_and_next_job_uses_demucs(
    audio_engine_mock: Mock, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    roformer = FakeStemGenerationBackend()
    demucs = FakeStemGenerationBackend()
    backends: dict[StemSeparator, StemGenerationBackend] = {
        "bs-roformer:musdb18hq": roformer,
        "demucs:htdemucs": demucs,
    }
    tasks: list[Callable[[], None]] = []
    controller = AppController(
        stem_backend=SelectedStemGenerationBackend(backends), stem_task_runner=tasks.append
    )
    _load_pad(controller, tmp_path, 0)
    _load_pad(controller, tmp_path, 1)
    controller.settings.set_stem_separator("bs-roformer:musdb18hq")
    assert controller.stems.generate_stems_async(0) is True
    captured = next(iter(controller.stems._jobs.values())).request
    assert captured.separator == "bs-roformer:musdb18hq"
    assert captured.model_cache_dir.name == "bs-roformer-musdb18hq"
    controller.settings.set_stem_separator("demucs:htdemucs")
    assert controller.stems.generate_stems_async(1) is True

    assert not roformer.requests
    assert not demucs.requests
    while tasks:
        tasks.pop(0)()
        controller.stems.on_frame_render()

    assert roformer.requests == []  # Obsolete queued intent never enters inference.
    assert [request.separator for request in demucs.requests] == ["demucs:htdemucs"]
    assert demucs.requests[0].model_cache_dir != captured.model_cache_dir
    assert controller.stems.stems_available(0) is False
    assert controller.stems.stems_available(1) is True
    assert audio_engine_mock.publish_prepared_stems.call_count == 1


def test_restore_complete_demucs_cache_needs_no_selected_roformer_model(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model_cache = tmp_path / "uninstalled-model"
    monkeypatch.setattr(
        "flitzis_looper.controller.stems.separator_model_cache_dir", lambda _separator: model_cache
    )
    _load_pad(controller, tmp_path, 0)
    assert controller.stems.generate_stems_async(0) is True
    controller.stems.on_frame_render()
    previous = controller.project.stem_cache[0]
    assert previous is not None
    assert not model_cache.exists()
    controller.settings.set_stem_separator("bs-roformer:musdb18hq")
    downloads = _forbid_downloads(monkeypatch)
    no_model_load = Mock(side_effect=AssertionError("restore must not load separator models"))
    monkeypatch.setattr(bs_roformer_assets, "verify_model_assets", no_model_load)
    audio_engine_mock.publish_prepared_stems.reset_mock()

    controller.stems.restore_stem_cache_from_project_state()
    assert controller.stems.stems_available(0) is False
    assert controller.stems.publish_restored_stem_cache_if_available(0) is True
    controller.stems.on_frame_render()

    restored = controller.project.stem_cache[0]
    assert restored is not None
    assert restored.cache_dir == previous.cache_dir
    assert restored.source_version == previous.source_version
    assert controller.stems.stems_available(0) is True
    assert len(stem_backend.requests) == 1
    no_model_load.assert_not_called()
    downloads.assert_not_called()
    audio_engine_mock.publish_prepared_stems.assert_called_once()
