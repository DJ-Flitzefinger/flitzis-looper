import hashlib
import os
import urllib.request
import wave
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import AppController, bs_roformer_assets
from flitzis_looper.controller.bs_roformer_generation import BsRoformerStemGenerationBackend
from flitzis_looper.controller.stem_generation import (
    AudioShape,
    CommandResult,
    StemGenerationError,
    StemGenerationRequest,
)
from flitzis_looper.controller.stem_separators import SelectedStemGenerationBackend
from flitzis_looper.models import STEM_KINDS
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import FakeStemGenerationBackend

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.stem_generation import StemGenerationBackend
    from flitzis_looper.models import StemSeparator


def _request(tmp_path: Path) -> StemGenerationRequest:
    source = tmp_path / "source.wav"
    write_mono_pcm16_wav(source, 44_100)
    return StemGenerationRequest(
        sample_id=0,
        source_path=source,
        source_version="source.wav|sha256-v1:" + hashlib.sha256(source.read_bytes()).hexdigest(),
        cache_dir=tmp_path / "generation",
        target_shape=AudioShape(sample_rate_hz=44_100, channels=1, frame_count=128),
        model_cache_dir=tmp_path / "models",
        separator="bs-roformer:musdb18hq",
    )


def _install_test_assets(directory: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    # Small fixtures retain real size/digest verification at the adapter boundary;
    # they are never deserialized or presented as actual model inference.
    directory.mkdir()
    pins = []
    for filename in (bs_roformer_assets.CONFIG_FILENAME, bs_roformer_assets.CHECKPOINT_FILENAME):
        payload = filename.encode("ascii")
        (directory / filename).write_bytes(payload)
        pins.append((filename, len(payload), hashlib.sha256(payload).hexdigest()))
    monkeypatch.setattr(bs_roformer_assets, "MODEL_ASSETS", tuple(pins))


def _forbid_downloads(monkeypatch: pytest.MonkeyPatch) -> Mock:
    forbidden = Mock(side_effect=AssertionError("generation must not download or install models"))
    monkeypatch.setattr(bs_roformer_assets, "install_model_assets", forbidden)
    monkeypatch.setattr(urllib.request, "urlopen", forbidden)
    return forbidden


@pytest.mark.parametrize("filename", [None, bs_roformer_assets.CONFIG_FILENAME])
def test_missing_or_corrupt_model_rejects_before_subprocess(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, filename: str | None
) -> None:
    request = _request(tmp_path)
    if filename is not None:
        _install_test_assets(request.model_cache_dir, monkeypatch)
        asset = request.model_cache_dir / filename
        stat = asset.stat()
        asset.write_bytes(b"x" * stat.st_size)
        os.utime(asset, ns=(stat.st_atime_ns, stat.st_mtime_ns))
    runner = Mock()
    downloads = _forbid_downloads(monkeypatch)
    backend = BsRoformerStemGenerationBackend(command_runner=runner)

    with pytest.raises(StemGenerationError, match=r"no Model installed|integrity check failed"):
        backend.generate(request, lambda _percent, _stage: None)

    runner.assert_not_called()
    downloads.assert_not_called()
    assert not request.cache_dir.exists()


def test_same_size_same_mtime_checkpoint_corruption_is_rejected(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    request = _request(tmp_path)
    _install_test_assets(request.model_cache_dir, monkeypatch)
    asset = request.model_cache_dir / bs_roformer_assets.CHECKPOINT_FILENAME
    stat = asset.stat()
    asset.write_bytes(b"x" * stat.st_size)
    os.utime(asset, ns=(stat.st_atime_ns, stat.st_mtime_ns))
    runner = Mock()
    backend = BsRoformerStemGenerationBackend(command_runner=runner)

    with pytest.raises(StemGenerationError, match="integrity check failed"):
        backend.generate(request, lambda _percent, _stage: None)

    runner.assert_not_called()
    assert asset.stat().st_size == stat.st_size
    assert asset.stat().st_mtime_ns == stat.st_mtime_ns


def test_cuda_failure_retries_identical_model_on_cpu_without_download(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    request = _request(tmp_path)
    _install_test_assets(request.model_cache_dir, monkeypatch)
    downloads = _forbid_downloads(monkeypatch)
    commands: list[list[str]] = []
    stages: list[str] = []

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        if args[0] in {"ffprobe", "ffmpeg"}:
            return CommandResult(0, "ok", "")
        commands.append(args)
        if args[args.index("--device") + 1] == "cuda":
            return CommandResult(1, "", "CUDA out of memory")
        output = Path(args[args.index("--output") + 1])
        for name in ("drums", "bass", "other", "vocals"):
            write_mono_pcm16_wav(output / f"{name}.wav", 44_100)
        return CommandResult(0, "", "")

    backend = BsRoformerStemGenerationBackend(
        command_runner=runner, cuda_detector=lambda _env, _runner: True
    )
    result = backend.generate(request, lambda _percent, stage: stages.append(stage))

    assert result.backend_name == "bs-roformer"
    assert result.model_name == bs_roformer_assets.MODEL_NAME
    assert result.device == "cpu"
    assert result.cpu_fallback is True
    assert result.artifact_count == len(STEM_KINDS)
    assert "same model" in (result.diagnostic or "")
    assert [args[args.index("--device") + 1] for args in commands] == ["cuda", "cpu"]
    for args in commands:
        assert args[1:3] == ["-m", "flitzis_bs_roformer"]
        assert Path(args[args.index("--checkpoint") + 1]) == (
            request.model_cache_dir / bs_roformer_assets.CHECKPOINT_FILENAME
        )
        assert Path(args[args.index("--config") + 1]) == (
            request.model_cache_dir / bs_roformer_assets.CONFIG_FILENAME
        )
    assert any("retrying same BS-RoFormer model on CPU" in stage for stage in stages)
    downloads.assert_not_called()
    assert {path.name for path in request.cache_dir.iterdir()} == {
        f"{kind}.wav" for kind in STEM_KINDS
    }
    for kind in STEM_KINDS:
        with wave.open(str(request.cache_dir / f"{kind}.wav"), "rb") as wav:
            assert (wav.getframerate(), wav.getnchannels(), wav.getnframes()) == (44_100, 1, 128)


def test_incomplete_model_output_preserves_existing_cache(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    request = _request(tmp_path)
    _install_test_assets(request.model_cache_dir, monkeypatch)
    request.cache_dir.mkdir()
    previous = {kind: kind.encode("ascii") for kind in STEM_KINDS}
    for kind, contents in previous.items():
        (request.cache_dir / f"{kind}.wav").write_bytes(contents)

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        if args[0] in {"ffprobe", "ffmpeg"}:
            return CommandResult(0, "ok", "")
        output = Path(args[args.index("--output") + 1])
        for name in ("drums", "other", "vocals"):
            write_mono_pcm16_wav(output / f"{name}.wav", 44_100)
        return CommandResult(0, "", "")

    backend = BsRoformerStemGenerationBackend(
        command_runner=runner, cuda_detector=lambda _env, _runner: False
    )
    with pytest.raises(StemGenerationError, match=r"missing bass\.wav"):
        backend.generate(request, lambda _percent, _stage: None)

    assert {path.name for path in request.cache_dir.iterdir()} == {
        f"{kind}.wav" for kind in STEM_KINDS
    }
    for kind, contents in previous.items():
        assert (request.cache_dir / f"{kind}.wav").read_bytes() == contents


def _load_pad(controller: AppController, tmp_path: Path, sample_id: int) -> None:
    samples = tmp_path / "samples"
    samples.mkdir(exist_ok=True)
    name = f"loop-{sample_id}.wav"
    write_mono_pcm16_wav(samples / name, 44_100)
    controller.project.sample_paths[sample_id] = f"samples/{name}"
    controller.project.sample_durations[sample_id] = 128 / 44_100


def test_queued_selection_is_captured_and_next_job_returns_to_demucs(
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
    controller.settings.set_stem_separator("demucs:htdemucs")
    assert controller.stems.generate_stems_async(1) is True

    assert not roformer.requests
    assert not demucs.requests
    for task in tasks:
        task()
    controller.stems.on_frame_render()

    assert [request.separator for request in roformer.requests] == ["bs-roformer:musdb18hq"]
    assert [request.separator for request in demucs.requests] == ["demucs:htdemucs"]
    assert roformer.requests[0].model_cache_dir.name == "bs-roformer-musdb18hq"
    assert demucs.requests[0].model_cache_dir != roformer.requests[0].model_cache_dir
    assert controller.stems.stems_available(0) is True
    assert controller.stems.stems_available(1) is True
    assert audio_engine_mock.publish_prepared_stems.call_count == 2


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
    monkeypatch.setattr(
        "flitzis_looper.controller.bs_roformer_generation.verify_model_assets", no_model_load
    )
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
