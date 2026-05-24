import os
import struct
import sys
import tomllib
import wave
from pathlib import Path

import pytest

from flitzis_looper.controller.stem_generation import (
    DEFAULT_DEMUCS_OVERLAP,
    DEFAULT_DEMUCS_SHIFTS,
    FFMPEG_DIR_ENV_VAR,
    FFMPEG_UNAVAILABLE_MESSAGE,
    MAX_DEMUCS_OVERLAP,
    MAX_DEMUCS_SHIFTS,
    MIN_DEMUCS_OVERLAP,
    MIN_DEMUCS_SHIFTS,
    NO_MODEL_INSTALLED_MESSAGE,
    TORCHCODEC_UNAVAILABLE_MESSAGE,
    AudioShape,
    CommandResult,
    DemucsStemGenerationBackend,
    StemGenerationError,
    StemGenerationRequest,
    default_demucs_model_cache_dir,
    demucs_cache_environment,
)


def test_demucs_declared_as_runtime_dependency() -> None:
    pyproject_path = Path(__file__).parents[4] / "pyproject.toml"
    pyproject = tomllib.loads(pyproject_path.read_text(encoding="utf-8"))
    dependencies = pyproject["project"]["dependencies"]

    assert any(dependency.lower().startswith("demucs") for dependency in dependencies)
    assert any(dependency.lower().startswith("torchcodec") for dependency in dependencies)


def test_default_demucs_model_cache_uses_standard_torch_checkpoint_dir(tmp_path: Path) -> None:
    project_root = tmp_path / "repo"
    home = tmp_path / "home"

    cache_dir = default_demucs_model_cache_dir(
        env={"LOCALAPPDATA": str(tmp_path / "LocalAppData")},
        project_root=project_root,
        home_dir=home,
    )

    assert cache_dir == home / ".cache" / "torch" / "hub" / "checkpoints"


def test_default_demucs_model_cache_avoids_project_root(tmp_path: Path) -> None:
    project_root = tmp_path / "repo"
    project_root.mkdir()

    cache_dir = default_demucs_model_cache_dir(
        env={},
        project_root=project_root,
        home_dir=project_root,
    )

    assert not _is_relative_to(cache_dir, project_root)
    assert not _is_relative_to(cache_dir, project_root / "samples")


def test_demucs_cache_environment_points_torch_home_to_standard_root(tmp_path: Path) -> None:
    cache_dir = tmp_path / ".cache" / "torch" / "hub" / "checkpoints"

    env = demucs_cache_environment(cache_dir, base_env={})

    assert env["TORCH_HOME"] == str(tmp_path / ".cache" / "torch")
    assert "XDG_CACHE_HOME" not in env
    assert "DORA_CACHE" not in env
    assert "HF_HOME" not in env


def test_demucs_cache_environment_adds_explicit_ffmpeg_dir_to_path(tmp_path: Path) -> None:
    cache_dir = tmp_path / ".cache" / "torch" / "hub" / "checkpoints"
    ffmpeg_root = tmp_path / "tools" / "ffmpeg"
    ffmpeg_bin = ffmpeg_root / "bin"
    _write_ffmpeg_tools(ffmpeg_bin)

    env = demucs_cache_environment(
        cache_dir,
        base_env={
            FFMPEG_DIR_ENV_VAR: str(ffmpeg_root),
            "PATH": "existing",
        },
    )

    assert env["PATH"].split(os.pathsep)[0] == str(ffmpeg_bin.resolve(strict=False))


def test_demucs_cache_environment_falls_back_to_winget_ffmpeg_package(
    tmp_path: Path,
) -> None:
    cache_dir = tmp_path / ".cache" / "torch" / "hub" / "checkpoints"
    local_appdata = tmp_path / "LocalAppData"
    ffmpeg_bin = (
        local_appdata
        / "Microsoft"
        / "WinGet"
        / "Packages"
        / "Gyan.FFmpeg.Shared_Microsoft.Winget.Source_8wekyb3d8bbwe"
        / "ffmpeg-8.1.1-full_build-shared"
        / "bin"
    )
    _write_ffmpeg_tools(ffmpeg_bin)

    env = demucs_cache_environment(
        cache_dir,
        base_env={
            "LOCALAPPDATA": str(local_appdata),
            "PATH": "",
        },
    )

    assert env["PATH"] == str(ffmpeg_bin.resolve(strict=False))


def test_demucs_backend_requires_preinstalled_model(tmp_path: Path) -> None:
    source = tmp_path / "source.wav"
    _write_pcm16_wav(source, 44_100, 1, [0.0, 0.0])
    commands: list[list[str]] = []

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        commands.append(args)
        return CommandResult(returncode=0, stdout="", stderr="")

    backend = DemucsStemGenerationBackend(
        command_runner=runner,
        cuda_detector=lambda _env, _runner: False,
    )

    with pytest.raises(StemGenerationError, match=NO_MODEL_INSTALLED_MESSAGE):
        backend.generate(
            _request(
                tmp_path,
                source,
                AudioShape(sample_rate_hz=44_100, channels=1, frame_count=2),
            ),
            lambda _percent, _stage: None,
        )

    assert commands == []


def test_demucs_backend_maps_other_to_melody_and_aligns_outputs(tmp_path: Path) -> None:
    source = tmp_path / "source.wav"
    _write_pcm16_wav(source, 44_100, 1, [0.0, 0.0])
    commands: list[list[str]] = []

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        commands.append(args)
        if _is_backend_preflight(args):
            return CommandResult(returncode=0, stdout="ok", stderr="")
        output_dir = _arg_path(args, "-o")
        demucs_dir = output_dir / "htdemucs" / "source"
        demucs_dir.mkdir(parents=True)
        _write_pcm16_wav(demucs_dir / "vocals.wav", 22_050, 1, [0.1, 0.1])
        _write_pcm16_wav(demucs_dir / "drums.wav", 22_050, 1, [0.2, 0.2])
        _write_pcm16_wav(demucs_dir / "bass.wav", 22_050, 1, [0.3, 0.3])
        _write_pcm16_wav(demucs_dir / "other.wav", 22_050, 1, [0.4, 0.4])
        return CommandResult(returncode=0, stdout="", stderr="")

    backend = DemucsStemGenerationBackend(
        command_runner=runner,
        cuda_detector=lambda _env, _runner: False,
    )
    request = _request(
        tmp_path,
        source,
        AudioShape(sample_rate_hz=44_100, channels=2, frame_count=4),
    )
    _install_htdemucs_checkpoint(request.model_cache_dir)

    result = backend.generate(request, lambda _percent, _stage: None)

    assert result.backend_name == "demucs"
    assert result.device == "cpu"
    assert commands[-1][1:3] == ["-m", "demucs"]
    assert commands[-1][commands[-1].index("--shifts") + 1] == str(DEFAULT_DEMUCS_SHIFTS)
    assert commands[-1][commands[-1].index("--overlap") + 1] == str(DEFAULT_DEMUCS_OVERLAP)
    assert (_read_pcm16_wav(request.cache_dir / "melody.wav")[0]) == pytest.approx(0.4, abs=1e-3)
    instrumental = _read_pcm16_wav(request.cache_dir / "instrumental.wav")
    assert instrumental[0] == pytest.approx(0.9, abs=1e-3)
    for stem_name in ("vocals", "melody", "bass", "drums", "instrumental"):
        with wave.open(str(request.cache_dir / f"{stem_name}.wav"), "rb") as wav:
            assert wav.getframerate() == 44_100
            assert wav.getnchannels() == 2
            assert wav.getnframes() == 4


def test_demucs_backend_accepts_custom_quality_parameters_for_future_settings(
    tmp_path: Path,
) -> None:
    source = tmp_path / "source.wav"
    _write_pcm16_wav(source, 44_100, 1, [0.0, 0.0])
    commands: list[list[str]] = []

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        commands.append(args)
        if _is_backend_preflight(args):
            return CommandResult(returncode=0, stdout="ok", stderr="")
        output_dir = _arg_path(args, "-o")
        demucs_dir = output_dir / "htdemucs" / "source"
        demucs_dir.mkdir(parents=True)
        for stem_name in ("vocals", "drums", "bass", "other"):
            _write_pcm16_wav(demucs_dir / f"{stem_name}.wav", 44_100, 1, [0.1, 0.1])
        return CommandResult(returncode=0, stdout="", stderr="")

    backend = DemucsStemGenerationBackend(
        command_runner=runner,
        cuda_detector=lambda _env, _runner: False,
    )
    request = _request(
        tmp_path,
        source,
        AudioShape(sample_rate_hz=44_100, channels=1, frame_count=2),
        demucs_shifts=4,
        demucs_overlap=0.25,
    )
    _install_htdemucs_checkpoint(request.model_cache_dir)

    result = backend.generate(request, lambda _percent, _stage: None)

    assert result.device == "cpu"
    assert commands[-1][commands[-1].index("--shifts") + 1] == "4"
    assert commands[-1][commands[-1].index("--overlap") + 1] == "0.25"


@pytest.mark.parametrize(
    ("demucs_shifts", "demucs_overlap", "expected"),
    [
        (MIN_DEMUCS_SHIFTS - 1, DEFAULT_DEMUCS_OVERLAP, "demucs_shifts"),
        (MAX_DEMUCS_SHIFTS + 1, DEFAULT_DEMUCS_OVERLAP, "demucs_shifts"),
        (DEFAULT_DEMUCS_SHIFTS, MIN_DEMUCS_OVERLAP - 0.1, "demucs_overlap"),
        (DEFAULT_DEMUCS_SHIFTS, MAX_DEMUCS_OVERLAP + 0.1, "demucs_overlap"),
    ],
)
def test_demucs_backend_rejects_out_of_range_quality_parameters(
    tmp_path: Path,
    demucs_shifts: int,
    demucs_overlap: float,
    expected: str,
) -> None:
    source = tmp_path / "source.wav"
    _write_pcm16_wav(source, 44_100, 1, [0.0, 0.0])
    commands: list[list[str]] = []

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        commands.append(args)
        return CommandResult(returncode=0, stdout="", stderr="")

    backend = DemucsStemGenerationBackend(
        command_runner=runner,
        cuda_detector=lambda _env, _runner: False,
    )

    with pytest.raises(StemGenerationError, match=expected):
        backend.generate(
            _request(
                tmp_path,
                source,
                AudioShape(sample_rate_hz=44_100, channels=1, frame_count=2),
                demucs_shifts=demucs_shifts,
                demucs_overlap=demucs_overlap,
            ),
            lambda _percent, _stage: None,
        )

    assert commands == []


def test_demucs_backend_retries_cpu_after_cuda_failure(tmp_path: Path) -> None:
    source = tmp_path / "source.wav"
    _write_pcm16_wav(source, 44_100, 1, [0.0, 0.0])
    devices: list[str] = []

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        if _is_backend_preflight(args):
            return CommandResult(returncode=0, stdout="ok", stderr="")
        device = args[args.index("-d") + 1]
        devices.append(device)
        if device == "cuda":
            return CommandResult(returncode=1, stdout="", stderr="CUDA out of memory")

        output_dir = _arg_path(args, "-o")
        demucs_dir = output_dir / "htdemucs" / "source"
        demucs_dir.mkdir(parents=True)
        for stem_name in ("vocals", "drums", "bass", "other"):
            _write_pcm16_wav(demucs_dir / f"{stem_name}.wav", 44_100, 1, [0.1, 0.1])
        return CommandResult(returncode=0, stdout="", stderr="")

    backend = DemucsStemGenerationBackend(
        command_runner=runner,
        cuda_detector=lambda _env, _runner: True,
    )
    request = _request(
        tmp_path,
        source,
        AudioShape(sample_rate_hz=44_100, channels=1, frame_count=2),
    )
    _install_htdemucs_checkpoint(request.model_cache_dir)

    result = backend.generate(
        request,
        lambda _percent, _stage: None,
    )

    assert devices == ["cuda", "cpu"]
    assert result.device == "cpu"
    assert result.cpu_fallback is True
    assert result.diagnostic is not None
    assert "CUDA failed" in result.diagnostic


def test_demucs_backend_reports_missing_dependency(tmp_path: Path) -> None:
    source = tmp_path / "source.wav"
    _write_pcm16_wav(source, 44_100, 1, [0.0, 0.0])

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        if _is_backend_preflight(args):
            return CommandResult(returncode=0, stdout="ok", stderr="")
        return CommandResult(returncode=1, stdout="", stderr="No module named demucs")

    backend = DemucsStemGenerationBackend(
        command_runner=runner,
        cuda_detector=lambda _env, _runner: False,
    )
    request = _request(
        tmp_path,
        source,
        AudioShape(sample_rate_hz=44_100, channels=1, frame_count=2),
    )
    _install_htdemucs_checkpoint(request.model_cache_dir)

    with pytest.raises(StemGenerationError, match="No module named demucs"):
        backend.generate(
            request,
            lambda _percent, _stage: None,
        )


def test_demucs_backend_does_not_report_progress_stream_as_error(tmp_path: Path) -> None:
    source = tmp_path / "source.wav"
    _write_pcm16_wav(source, 44_100, 1, [0.0, 0.0])

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        if _is_backend_preflight(args):
            return CommandResult(returncode=0, stdout="ok", stderr="")
        return CommandResult(
            returncode=1,
            stdout="",
            stderr="0% | 0.00/80.2M [00:00<?, ?B/s]\r12% | 9.90M/80.2M [00:02<00:20]",
        )

    backend = DemucsStemGenerationBackend(
        command_runner=runner,
        cuda_detector=lambda _env, _runner: False,
    )
    request = _request(
        tmp_path,
        source,
        AudioShape(sample_rate_hz=44_100, channels=1, frame_count=2),
    )
    _install_htdemucs_checkpoint(request.model_cache_dir)

    with pytest.raises(StemGenerationError) as exc_info:
        backend.generate(request, lambda _percent, _stage: None)

    assert str(exc_info.value) == "Stem generation failed"


def test_demucs_backend_reports_unavailable_ffmpeg_before_demucs(tmp_path: Path) -> None:
    source = tmp_path / "source.wav"
    _write_pcm16_wav(source, 44_100, 1, [0.0, 0.0])
    commands: list[list[str]] = []

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        commands.append(args)
        return CommandResult(returncode=1, stdout="", stderr="access denied")

    backend = DemucsStemGenerationBackend(
        command_runner=runner,
        cuda_detector=lambda _env, _runner: False,
    )
    request = _request(
        tmp_path,
        source,
        AudioShape(sample_rate_hz=44_100, channels=1, frame_count=2),
    )
    _install_htdemucs_checkpoint(request.model_cache_dir)

    with pytest.raises(StemGenerationError, match=FFMPEG_UNAVAILABLE_MESSAGE):
        backend.generate(request, lambda _percent, _stage: None)

    assert commands == [["ffprobe", "-version"]]


def test_demucs_backend_reports_unavailable_torchcodec_before_demucs(tmp_path: Path) -> None:
    source = tmp_path / "source.wav"
    _write_pcm16_wav(source, 44_100, 1, [0.0, 0.0])
    commands: list[list[str]] = []

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        commands.append(args)
        if _is_tool_probe(args):
            return CommandResult(returncode=0, stdout="ok", stderr="")
        return CommandResult(returncode=1, stdout="", stderr="torchcodec failed")

    backend = DemucsStemGenerationBackend(
        command_runner=runner,
        cuda_detector=lambda _env, _runner: False,
    )
    request = _request(
        tmp_path,
        source,
        AudioShape(sample_rate_hz=44_100, channels=1, frame_count=2),
    )
    _install_htdemucs_checkpoint(request.model_cache_dir)

    with pytest.raises(StemGenerationError, match=TORCHCODEC_UNAVAILABLE_MESSAGE):
        backend.generate(request, lambda _percent, _stage: None)

    assert commands == [
        ["ffprobe", "-version"],
        ["ffmpeg", "-version"],
        _torchcodec_probe_command(),
    ]


def _request(
    tmp_path: Path,
    source: Path,
    shape: AudioShape,
    *,
    demucs_shifts: int = DEFAULT_DEMUCS_SHIFTS,
    demucs_overlap: float = DEFAULT_DEMUCS_OVERLAP,
) -> StemGenerationRequest:
    return StemGenerationRequest(
        sample_id=0,
        source_path=source,
        source_version="samples/source.wav|1|2",
        cache_dir=tmp_path / "samples" / "stems" / "cache",
        target_shape=shape,
        model_cache_dir=tmp_path / ".cache" / "torch" / "hub" / "checkpoints",
        demucs_shifts=demucs_shifts,
        demucs_overlap=demucs_overlap,
    )


def _arg_path(args: list[str], flag: str) -> Path:
    return Path(args[args.index(flag) + 1])


def _is_backend_preflight(args: list[str]) -> bool:
    return _is_tool_probe(args) or args == _torchcodec_probe_command()


def _is_tool_probe(args: list[str]) -> bool:
    return args in (["ffprobe", "-version"], ["ffmpeg", "-version"])


def _torchcodec_probe_command() -> list[str]:
    return [sys.executable, "-c", "import torchcodec.encoders"]


def _install_htdemucs_checkpoint(model_cache_dir: Path) -> None:
    model_cache_dir.mkdir(parents=True, exist_ok=True)
    (model_cache_dir / "955717e8-8726e21a.th").write_bytes(b"placeholder")


def _write_ffmpeg_tools(directory: Path) -> None:
    directory.mkdir(parents=True)
    suffix = ".exe" if os.name == "nt" else ""
    (directory / f"ffmpeg{suffix}").write_bytes(b"placeholder")
    (directory / f"ffprobe{suffix}").write_bytes(b"placeholder")


def _write_pcm16_wav(path: Path, sample_rate_hz: int, channels: int, samples: list[float]) -> None:
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(2)
        wav.setframerate(sample_rate_hz)
        payload = b"".join(
            struct.pack("<h", round(sample * 32767.0))
            for sample in samples
            for _ in range(channels)
        )
        wav.writeframes(payload)


def _read_pcm16_wav(path: Path) -> list[float]:
    with wave.open(str(path), "rb") as wav:
        raw = wav.readframes(wav.getnframes())
    return [sample[0] / 32767.0 for sample in struct.iter_unpack("<h", raw)]


def _is_relative_to(path: Path, parent: Path) -> bool:
    try:
        path.relative_to(parent)
    except ValueError:
        return False
    return True
