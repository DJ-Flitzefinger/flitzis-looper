import math
import os
import shutil
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Literal, Protocol

from flitzis_looper.constants import (
    DEFAULT_DEMUCS_OVERLAP,
    DEFAULT_DEMUCS_SHIFTS,
    MAX_DEMUCS_OVERLAP,
    MAX_DEMUCS_SHIFTS,
    MIN_DEMUCS_OVERLAP,
    MIN_DEMUCS_SHIFTS,
)
from flitzis_looper.controller.stem_artifacts import StemArtifactError as StemGenerationError
from flitzis_looper.controller.stem_artifacts import validate_target_shape
from flitzis_looper.offline_runtime import child_environment

if TYPE_CHECKING:
    from flitzis_looper.controller.stem_provider import ProviderCalibration
    from flitzis_looper.models import StemSeparator
    from flitzis_looper.stem_execution_protocol import ExecutionReceipt

__all__ = ["StemGenerationError"]

type StemDevicePolicy = Literal["auto", "cpu"]
type StemDevice = Literal["cuda", "cpu"]
type StemProgressCallback = Callable[[float, str], None]

FFMPEG_DIR_ENV_VAR = "FLITZIS_FFMPEG_DIR"
MODEL_CACHE_DIR_ENV_VAR = "FLITZIS_MODEL_CACHE_DIR"
STEM_MODEL_CACHE_SUBDIR = "models/stem-checkpoints"


@dataclass(frozen=True, slots=True)
class AudioShape:
    """Target audio shape expected by Rust prepared-stem publication."""

    sample_rate_hz: int
    channels: int
    frame_count: int


@dataclass(frozen=True, slots=True)
class StemGenerationRequest:
    """Stable file/artifact request passed to a stem-generation backend."""

    sample_id: int
    source_path: Path
    source_version: str
    cache_dir: Path
    target_shape: AudioShape
    model_cache_dir: Path
    device_policy: StemDevicePolicy = "auto"
    demucs_shifts: int = DEFAULT_DEMUCS_SHIFTS
    demucs_overlap: float = DEFAULT_DEMUCS_OVERLAP
    separator: StemSeparator = "demucs:htdemucs"
    encoder_executable: Path | None = None
    encoder_contract: str | None = None
    original_bytes: int | None = None
    producer_identity: str | None = None
    bs_onnx_manifest: Path | None = None


@dataclass(frozen=True, slots=True)
class StemGenerationResult:
    """Small backend result suitable for non-audio-thread diagnostics."""

    backend_name: str
    model_name: str
    device: StemDevice
    cpu_fallback: bool
    artifact_count: int
    diagnostic: str | None = None
    encoder_verified: bool = False
    decoder_verified: bool = False
    execution: ExecutionReceipt | None = None
    provider_calibration: ProviderCalibration | None = None
    executed_producer_identity: str | None = None
    calibration_family: str | None = None


class StemGenerationBackend(Protocol):
    """Backend contract for offline stem-generation adapters."""

    def generate(
        self,
        request: StemGenerationRequest,
        progress: StemProgressCallback,
    ) -> StemGenerationResult: ...


def default_stem_model_cache_dir(
    *, env: dict[str, str] | None = None, project_root: Path | None = None
) -> Path:
    """Resolve the explicit cache or this checkout's ignored stem model directory."""
    environment = os.environ if env is None else env
    root = Path.cwd() if project_root is None else project_root
    override = environment.get(MODEL_CACHE_DIR_ENV_VAR)
    candidate = Path(override) if override else root / STEM_MODEL_CACHE_SUBDIR
    return _safe_model_cache_dir(candidate, root)


def stem_model_environment(
    model_cache_dir: Path, *, base_env: dict[str, str] | None = None
) -> dict[str, str]:
    """Bind setup and its owned children to the selected model and codec paths."""
    env = child_environment(dict(os.environ if base_env is None else base_env))
    env[MODEL_CACHE_DIR_ENV_VAR] = str(model_cache_dir.resolve(strict=False))
    _add_ffmpeg_dir_to_path(env)
    return env


def resolved_demucs_encoder(model_cache_dir: Path) -> Path | None:
    """Resolve the shared FFmpeg selected for stem input and output."""
    env = stem_model_environment(model_cache_dir)
    executable = shutil.which("ffmpeg", path=env.get("PATH"))
    return Path(executable).resolve(strict=True) if executable is not None else None


def _validate_request(request: StemGenerationRequest) -> None:
    if request.sample_id < 0:
        msg = "sample_id must be non-negative"
        raise StemGenerationError(msg)
    if not request.source_version.strip():
        msg = "source_version must not be empty"
        raise StemGenerationError(msg)
    if not request.source_path.is_file():
        msg = "source audio file is missing"
        raise StemGenerationError(msg)
    validate_target_shape(request.target_shape)
    shape = request.target_shape
    if shape.sample_rate_hz <= 0 or shape.channels <= 0 or shape.frame_count <= 0:
        msg = "target audio shape must be non-empty"
        raise StemGenerationError(msg)
    if not MIN_DEMUCS_SHIFTS <= request.demucs_shifts <= MAX_DEMUCS_SHIFTS:
        msg = f"demucs_shifts must be between {MIN_DEMUCS_SHIFTS} and {MAX_DEMUCS_SHIFTS}"
        raise StemGenerationError(msg)
    if not (
        math.isfinite(request.demucs_overlap)
        and MIN_DEMUCS_OVERLAP <= request.demucs_overlap <= MAX_DEMUCS_OVERLAP
    ):
        msg = f"demucs_overlap must be between {MIN_DEMUCS_OVERLAP:g} and {MAX_DEMUCS_OVERLAP:g}"
        raise StemGenerationError(msg)


def _format_demucs_float(value: float) -> str:
    return f"{value:g}"


def protected_model_roots(root: Path) -> tuple[Path, ...]:
    """Resolve source/data roots that model and setup outputs must never overlap."""
    protected = (
        "samples",
        "record",
        "config",
        ".git",
        "src",
        "rust",
        "assets",
        "scripts",
        "docs",
        "openspec",
        "exports",
        "output",
        "outputs",
        "dist",
        "build",
    )
    return tuple((root / name).resolve(strict=False) for name in protected)


def _safe_model_cache_dir(candidate: Path, project_root: Path) -> Path:
    logical_candidate = candidate.absolute()
    logical_root = project_root.absolute()
    candidate = candidate.resolve(strict=False)
    root = project_root.resolve(strict=False)
    logical_inside = _is_relative_to(logical_candidate, logical_root)
    physical_models = _is_relative_to(candidate, root / "models")
    aliases_protected = any(
        _is_relative_to(candidate, protected) for protected in protected_model_roots(root)
    )
    invalid_logical = logical_inside and (
        not _is_relative_to(logical_candidate, logical_root / "models") or not physical_models
    )
    invalid_physical = _is_relative_to(candidate, root) and not physical_models
    if invalid_logical or invalid_physical or aliases_protected:
        message = "Repository model files must stay inside the owned models directory"
        raise ValueError(message)
    return candidate


def _add_ffmpeg_dir_to_path(env: dict[str, str]) -> None:
    ffmpeg_dir = _find_ffmpeg_bin_dir(env)
    if ffmpeg_dir is None:
        return

    current_path = env.get("PATH", "")
    path_parts = [part for part in current_path.split(os.pathsep) if part]
    resolved = _resolve_path(ffmpeg_dir)
    if any(_resolve_path(Path(part)) == resolved for part in path_parts):
        return
    env["PATH"] = str(resolved) if not current_path else f"{resolved}{os.pathsep}{current_path}"


def _find_ffmpeg_bin_dir(env: dict[str, str]) -> Path | None:
    explicit_dir = _explicit_ffmpeg_bin_dir(env)
    if explicit_dir is not None:
        return explicit_dir

    path = env.get("PATH")
    if shutil.which("ffmpeg", path=path) is not None and shutil.which("ffprobe", path=path):
        return None

    return _winget_ffmpeg_bin_dir(env)


def _explicit_ffmpeg_bin_dir(env: dict[str, str]) -> Path | None:
    raw_dir = env.get(FFMPEG_DIR_ENV_VAR)
    if raw_dir is None or not raw_dir.strip():
        return None

    candidate = Path(raw_dir).expanduser()
    if _contains_ffmpeg_tools(candidate):
        return candidate

    bin_candidate = candidate / "bin"
    if _contains_ffmpeg_tools(bin_candidate):
        return bin_candidate

    return candidate


def _resolve_path(path: Path) -> Path:
    try:
        return path.resolve(strict=False)
    except OSError:
        return path


def _winget_ffmpeg_bin_dir(env: dict[str, str]) -> Path | None:
    local_appdata = env.get("LOCALAPPDATA")
    roots = []
    if local_appdata is not None and local_appdata.strip():
        roots.append(Path(local_appdata) / "Microsoft" / "WinGet" / "Packages")
    else:
        roots.append(Path.home() / "AppData" / "Local" / "Microsoft" / "WinGet" / "Packages")

    candidates: list[Path] = []
    for root in roots:
        if not root.is_dir():
            continue
        candidates.extend(root.glob("Gyan.FFmpeg*_*/*/bin"))

    valid = [candidate for candidate in candidates if _contains_ffmpeg_tools(candidate)]
    if not valid:
        return None
    return max(valid, key=_path_mtime_ns)


def _path_mtime_ns(path: Path) -> int:
    try:
        return path.stat().st_mtime_ns
    except OSError:
        return 0


def _contains_ffmpeg_tools(directory: Path) -> bool:
    return (
        _tool_executable(directory, "ffmpeg").is_file()
        and _tool_executable(directory, "ffprobe").is_file()
    )


def _tool_executable(directory: Path, tool: str) -> Path:
    suffix = ".exe" if os.name == "nt" else ""
    return directory / f"{tool}{suffix}"


def _is_relative_to(path: Path, parent: Path) -> bool:
    try:
        path.relative_to(parent)
    except ValueError:
        return False
    return True
