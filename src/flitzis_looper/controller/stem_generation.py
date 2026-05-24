import math
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import wave
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Literal, Protocol

from flitzis_looper.models import STEM_KINDS

type StemDevicePolicy = Literal["auto", "cpu"]
type StemDevice = Literal["cuda", "cpu"]
type StemProgressCallback = Callable[[float, str], None]

DEMUCS_PROJECT_STEM_MAP: tuple[tuple[str, str], ...] = (
    ("vocals", "vocals"),
    ("drums", "drums"),
    ("bass", "bass"),
    ("melody", "other"),
)
DEMUCS_REQUIRED_CHECKPOINTS: dict[str, tuple[str, ...]] = {
    "htdemucs": ("955717e8-8726e21a.th",),
}
NO_MODEL_INSTALLED_MESSAGE = "no Model installed"
FFMPEG_UNAVAILABLE_MESSAGE = "FFmpeg/ffprobe unavailable"
TORCHCODEC_UNAVAILABLE_MESSAGE = "TorchCodec unavailable"
FFMPEG_DIR_ENV_VAR = "FLITZIS_FFMPEG_DIR"
DEFAULT_DEMUCS_SHIFTS = 10
MIN_DEMUCS_SHIFTS = 0
MAX_DEMUCS_SHIFTS = 20
DEFAULT_DEMUCS_OVERLAP = 0.5
MIN_DEMUCS_OVERLAP = 0.0
MAX_DEMUCS_OVERLAP = 0.95


@dataclass(frozen=True, slots=True)
class AudioShape:
    """Target audio shape expected by Rust prepared-stem publication."""

    sample_rate_hz: int
    channels: int
    frame_count: int


@dataclass(frozen=True, slots=True)
class CommandResult:
    """Bounded result from an external command run outside the audio callback."""

    returncode: int
    stdout: str
    stderr: str


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


@dataclass(frozen=True, slots=True)
class StemGenerationResult:
    """Small backend result suitable for non-audio-thread diagnostics."""

    backend_name: str
    model_name: str
    device: StemDevice
    cpu_fallback: bool
    artifact_count: int
    diagnostic: str | None = None


class StemGenerationError(RuntimeError):
    """Stem generation failed before valid cache artifacts were produced."""


class CommandRunner(Protocol):
    """Run an external command outside the audio callback."""

    def __call__(
        self,
        args: list[str],
        *,
        cwd: Path,
        env: dict[str, str],
    ) -> CommandResult: ...


class CudaDetector(Protocol):
    """Detect CUDA availability outside startup and outside the audio callback."""

    def __call__(self, env: dict[str, str], command_runner: CommandRunner) -> bool: ...


class StemGenerationBackend(Protocol):
    """Backend contract for offline stem-generation adapters."""

    def generate(
        self,
        request: StemGenerationRequest,
        progress: StemProgressCallback,
    ) -> StemGenerationResult: ...


@dataclass(frozen=True, slots=True)
class _AudioData:
    sample_rate_hz: int
    channels: int
    samples: list[float]

    @property
    def frame_count(self) -> int:
        return len(self.samples) // self.channels


def run_command(
    args: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
) -> CommandResult:
    """Run a subprocess and capture bounded text output for diagnostics."""
    completed = subprocess.run(
        args,
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    return CommandResult(
        returncode=completed.returncode,
        stdout=_bounded_text(completed.stdout),
        stderr=_bounded_text(completed.stderr),
    )


def detect_torch_cuda_available(env: dict[str, str], command_runner: CommandRunner) -> bool:
    """Return whether Torch reports CUDA availability from a background subprocess."""
    result = command_runner(
        [
            sys.executable,
            "-c",
            "import torch; raise SystemExit(0 if torch.cuda.is_available() else 1)",
        ],
        cwd=Path.cwd(),
        env=env,
    )
    return result.returncode == 0


def default_demucs_model_cache_dir(
    *,
    env: dict[str, str] | None = None,
    project_root: Path | None = None,
    home_dir: Path | None = None,
) -> Path:
    """Return the standard Torch Hub checkpoint directory used by Demucs."""
    _ = os.environ if env is None else env
    root = Path.cwd() if project_root is None else project_root
    home = Path.home() if home_dir is None else home_dir

    candidate = home / ".cache" / "torch" / "hub" / "checkpoints"
    return _safe_model_cache_dir(candidate, root)


def demucs_cache_environment(
    model_cache_dir: Path,
    *,
    base_env: dict[str, str] | None = None,
) -> dict[str, str]:
    """Return environment variables for Demucs model and tool resolution."""
    env = dict(os.environ if base_env is None else base_env)
    cache_dir = model_cache_dir.resolve(strict=False)
    if cache_dir.name == "checkpoints" and cache_dir.parent.name == "hub":
        env["TORCH_HOME"] = str(cache_dir.parent.parent)
    _add_ffmpeg_dir_to_path(env)
    return env


class DemucsStemGenerationBackend:
    """Demucs-based stem-generation backend that writes the project cache contract."""

    def __init__(
        self,
        *,
        model_name: str = "htdemucs",
        command_runner: CommandRunner = run_command,
        cuda_detector: CudaDetector = detect_torch_cuda_available,
    ) -> None:
        self._model_name = model_name
        self._command_runner = command_runner
        self._cuda_detector = cuda_detector

    def generate(
        self,
        request: StemGenerationRequest,
        progress: StemProgressCallback,
    ) -> StemGenerationResult:
        """Generate aligned project stem cache artifacts using Demucs."""
        _validate_request(request)
        request.model_cache_dir.mkdir(parents=True, exist_ok=True)
        env = demucs_cache_environment(request.model_cache_dir)
        if not _has_required_demucs_model(request.model_cache_dir, self._model_name):
            raise StemGenerationError(NO_MODEL_INSTALLED_MESSAGE)
        _require_demucs_audio_tools(env, self._command_runner)
        _require_torchcodec(env, self._command_runner)

        devices = self._devices_for_request(request, env)
        cuda_error: str | None = None

        for device in devices:
            progress(0.05, _stage_for_device(device, fallback=cuda_error is not None))
            with tempfile.TemporaryDirectory(prefix="flitzis-demucs-") as temp_dir_name:
                temp_dir = Path(temp_dir_name)
                result = self._run_demucs(request, temp_dir, env, device)
                if result.returncode != 0:
                    failure_diagnostic = _command_diagnostic(result)
                    if device == "cuda" and "cpu" in devices:
                        cuda_error = failure_diagnostic
                        continue
                    raise StemGenerationError(failure_diagnostic)

                progress(0.85, "Aligning stem cache")
                _write_project_cache_artifacts(
                    output_root=temp_dir,
                    cache_dir=request.cache_dir,
                    target_shape=request.target_shape,
                    progress=progress,
                )

            diagnostic: str | None = None
            if cuda_error is not None:
                diagnostic = f"CUDA failed; generated stems on CPU. {cuda_error}"

            return StemGenerationResult(
                backend_name="demucs",
                model_name=self._model_name,
                device=device,
                cpu_fallback=cuda_error is not None and device == "cpu",
                artifact_count=len(STEM_KINDS),
                diagnostic=_bounded_text(diagnostic) if diagnostic is not None else None,
            )

        msg = "Demucs did not run"
        raise StemGenerationError(msg)

    def _devices_for_request(
        self,
        request: StemGenerationRequest,
        env: dict[str, str],
    ) -> tuple[StemDevice, ...]:
        if request.device_policy == "cpu":
            return ("cpu",)

        try:
            cuda_available = self._cuda_detector(env, self._command_runner)
        except OSError:
            cuda_available = False

        if cuda_available:
            return ("cuda", "cpu")
        return ("cpu",)

    def _run_demucs(
        self,
        request: StemGenerationRequest,
        output_dir: Path,
        env: dict[str, str],
        device: StemDevice,
    ) -> CommandResult:
        args = [
            sys.executable,
            "-m",
            "demucs",
            "-n",
            self._model_name,
            "-o",
            str(output_dir),
            "--filename",
            "{stem}.{ext}",
            "--clip-mode",
            "clamp",
            "--shifts",
            str(request.demucs_shifts),
            "--overlap",
            _format_demucs_float(request.demucs_overlap),
            "-d",
            device,
            str(request.source_path),
        ]
        try:
            return self._command_runner(args, cwd=Path.cwd(), env=env)
        except OSError as err:
            msg = f"Failed to run Demucs: {err}"
            return CommandResult(returncode=1, stdout="", stderr=msg)


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
    shape = request.target_shape
    if shape.sample_rate_hz <= 0 or shape.channels <= 0 or shape.frame_count <= 0:
        msg = "target audio shape must be non-empty"
        raise StemGenerationError(msg)
    if not MIN_DEMUCS_SHIFTS <= request.demucs_shifts <= MAX_DEMUCS_SHIFTS:
        msg = (
            f"demucs_shifts must be between {MIN_DEMUCS_SHIFTS} "
            f"and {MAX_DEMUCS_SHIFTS}"
        )
        raise StemGenerationError(msg)
    if not (
        math.isfinite(request.demucs_overlap)
        and MIN_DEMUCS_OVERLAP <= request.demucs_overlap <= MAX_DEMUCS_OVERLAP
    ):
        msg = (
            f"demucs_overlap must be between {MIN_DEMUCS_OVERLAP:g} "
            f"and {MAX_DEMUCS_OVERLAP:g}"
        )
        raise StemGenerationError(msg)


def _format_demucs_float(value: float) -> str:
    return f"{value:g}"


def _safe_model_cache_dir(candidate: Path, project_root: Path) -> Path:
    candidate = candidate.resolve(strict=False)
    root = project_root.resolve(strict=False)
    samples = (root / "samples").resolve(strict=False)
    if _is_relative_to(candidate, root) or _is_relative_to(candidate, samples):
        return (Path(tempfile.gettempdir()) / "flitzis-looper" / "models" / "demucs").resolve(
            strict=False
        )
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
    return _tool_executable(directory, "ffmpeg").is_file() and _tool_executable(
        directory, "ffprobe"
    ).is_file()


def _tool_executable(directory: Path, tool: str) -> Path:
    suffix = ".exe" if os.name == "nt" else ""
    return directory / f"{tool}{suffix}"


def _is_relative_to(path: Path, parent: Path) -> bool:
    try:
        path.relative_to(parent)
    except ValueError:
        return False
    return True


def _stage_for_device(device: StemDevice, *, fallback: bool) -> str:
    if fallback:
        return "Running Demucs on CPU after CUDA fallback"
    if device == "cuda":
        return "Running Demucs on CUDA"
    return "Running Demucs on CPU"


def _has_required_demucs_model(model_cache_dir: Path, model_name: str) -> bool:
    required_checkpoints = DEMUCS_REQUIRED_CHECKPOINTS.get(model_name)
    if required_checkpoints is None:
        return False
    return all((model_cache_dir / checkpoint).is_file() for checkpoint in required_checkpoints)


def _require_demucs_audio_tools(env: dict[str, str], command_runner: CommandRunner) -> None:
    for tool in ("ffprobe", "ffmpeg"):
        try:
            result = command_runner([tool, "-version"], cwd=Path.cwd(), env=env)
        except OSError as err:
            raise StemGenerationError(FFMPEG_UNAVAILABLE_MESSAGE) from err
        if result.returncode != 0:
            raise StemGenerationError(FFMPEG_UNAVAILABLE_MESSAGE)


def _require_torchcodec(env: dict[str, str], command_runner: CommandRunner) -> None:
    try:
        result = command_runner(
            [sys.executable, "-c", "import torchcodec.encoders"],
            cwd=Path.cwd(),
            env=env,
        )
    except OSError as err:
        raise StemGenerationError(TORCHCODEC_UNAVAILABLE_MESSAGE) from err
    if result.returncode != 0:
        raise StemGenerationError(TORCHCODEC_UNAVAILABLE_MESSAGE)


def _command_diagnostic(result: CommandResult) -> str:
    text = result.stderr.strip() or result.stdout.strip()
    if not text:
        text = f"Demucs exited with status {result.returncode}"
    return _bounded_text(_last_relevant_diagnostic_line(text))


def _last_relevant_diagnostic_line(text: str) -> str:
    for raw_line in reversed(text.replace("\r", "\n").splitlines()):
        line = raw_line.strip()
        if line and not _looks_like_progress_line(line):
            return line
    return "Stem generation failed"


def _looks_like_progress_line(line: str) -> bool:
    has_rate = "B/s]" in line or "it/s]" in line or "/s]" in line
    has_progress_shape = "%" in line and "/" in line and "[" in line
    return has_rate or has_progress_shape


def _bounded_text(text: str | None, *, limit: int = 1000) -> str:
    if text is None:
        return ""
    compact = " ".join(text.split())
    if len(compact) <= limit:
        return compact
    return f"{compact[: limit - 3]}..."


def _write_project_cache_artifacts(
    *,
    output_root: Path,
    cache_dir: Path,
    target_shape: AudioShape,
    progress: StemProgressCallback,
) -> None:
    cache_dir.mkdir(parents=True, exist_ok=True)
    aligned: dict[str, list[float]] = {}

    for index, (project_stem, demucs_stem) in enumerate(DEMUCS_PROJECT_STEM_MAP):
        progress(0.85 + index * 0.025, "Aligning stem cache")
        source = _find_demucs_output_file(output_root, demucs_stem)
        aligned[project_stem] = _align_audio(_read_pcm16_wav(source), target_shape)

    aligned["instrumental"] = _sum_stems(
        aligned["drums"],
        aligned["bass"],
        aligned["melody"],
    )

    for stem_name in STEM_KINDS:
        _write_pcm16_wav(cache_dir / f"{stem_name}.wav", target_shape, aligned[stem_name])

    progress(1.0, "Stem cache ready")


def _find_demucs_output_file(output_root: Path, stem_name: str) -> Path:
    matches = sorted(output_root.rglob(f"{stem_name}.wav"))
    if not matches:
        msg = f"Demucs output is missing {stem_name}.wav"
        raise StemGenerationError(msg)
    return matches[0]


def _read_pcm16_wav(path: Path) -> _AudioData:
    with wave.open(str(path), "rb") as wav:
        channels = wav.getnchannels()
        sample_rate_hz = wav.getframerate()
        sample_width = wav.getsampwidth()
        frames = wav.getnframes()
        raw = wav.readframes(frames)

    if channels <= 0 or sample_rate_hz <= 0 or frames <= 0:
        msg = f"{path.name} has an invalid audio shape"
        raise StemGenerationError(msg)
    if sample_width != 2:
        msg = f"{path.name} must be 16-bit PCM WAV"
        raise StemGenerationError(msg)

    expected_bytes = frames * channels * sample_width
    if len(raw) != expected_bytes:
        msg = f"{path.name} has incomplete PCM data"
        raise StemGenerationError(msg)

    samples = [_pcm16_to_float(value[0]) for value in struct.iter_unpack("<h", raw)]
    return _AudioData(sample_rate_hz=sample_rate_hz, channels=channels, samples=samples)


def _align_audio(audio: _AudioData, target: AudioShape) -> list[float]:
    output = [0.0] * (target.frame_count * target.channels)
    if audio.frame_count == 0:
        return output

    rate_ratio = audio.sample_rate_hz / target.sample_rate_hz
    for target_frame in range(target.frame_count):
        source_position = target_frame * rate_ratio
        source_frame = math.floor(source_position)
        fraction = source_position - source_frame
        for channel in range(target.channels):
            before = _sample_at(audio, source_frame, channel, target.channels)
            after = _sample_at(audio, source_frame + 1, channel, target.channels)
            output[target_frame * target.channels + channel] = before + (after - before) * fraction
    return output


def _sample_at(
    audio: _AudioData,
    frame: int,
    target_channel: int,
    target_channels: int,
) -> float:
    if frame < 0 or frame >= audio.frame_count:
        return 0.0

    if audio.channels == target_channels:
        return audio.samples[frame * audio.channels + target_channel]

    if audio.channels == 1:
        return audio.samples[frame * audio.channels]

    if target_channels == 1:
        start = frame * audio.channels
        stop = start + audio.channels
        return sum(audio.samples[start:stop]) / audio.channels

    if target_channel < audio.channels:
        return audio.samples[frame * audio.channels + target_channel]
    return 0.0


def _sum_stems(*stems: list[float]) -> list[float]:
    if not stems:
        return []
    summed = [0.0] * len(stems[0])
    for stem in stems:
        if len(stem) != len(summed):
            msg = "aligned stem lengths do not match"
            raise StemGenerationError(msg)
        for index, sample in enumerate(stem):
            summed[index] = max(-1.0, min(1.0, summed[index] + sample))
    return summed


def _write_pcm16_wav(path: Path, shape: AudioShape, samples: list[float]) -> None:
    if len(samples) != shape.frame_count * shape.channels:
        msg = "aligned stem length does not match target shape"
        raise StemGenerationError(msg)

    temp_path = path.with_name(f"{path.name}.tmp")
    with wave.open(str(temp_path), "wb") as wav:
        wav.setnchannels(shape.channels)
        wav.setsampwidth(2)
        wav.setframerate(shape.sample_rate_hz)
        wav.writeframes(
            b"".join(struct.pack("<h", _float_to_pcm16(sample)) for sample in samples)
        )

    temp_path.replace(path)


def _pcm16_to_float(sample: int) -> float:
    if sample == -32768:
        return -1.0
    return sample / 32767.0


def _float_to_pcm16(sample: float) -> int:
    value = sample if math.isfinite(sample) else 0.0
    value = max(-1.0, min(1.0, value))
    if value >= 1.0:
        return 32767
    if value <= -1.0:
        return -32768
    return round(value * 32767.0)
