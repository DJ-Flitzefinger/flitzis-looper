"""Prepare and reuse the installation-owned BS-RoFormer CUDA kernel cache."""

import hashlib
import json
import math
import os
import re
import struct
import wave
from contextlib import ExitStack
from dataclasses import asdict, dataclass, replace
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Event
from time import monotonic
from typing import TYPE_CHECKING, Literal, TypeIs
from uuid import uuid4

from flitzis_looper.analysis.beat_cuda_cache import (
    _cache_lock,
    _ordinary,
    _ordinary_tree,
    cuda_device_identity,
)
from flitzis_looper.analysis.process import heavy_work_scope, offline_work_scope, run_owned_command
from flitzis_looper.bs_onnx_installation import (
    BS_ONNX_CUDA_PROVIDER_SHA256,
    BS_ONNX_WEIGHTS_SHA256,
    bs_onnx_worker_cwd,
)
from flitzis_looper.offline_runtime import child_environment
from flitzis_looper.pcm16_wav import PcmWavReader
from flitzis_looper.recording_paths import _pin_ancestors, _windows_read_handle
from flitzis_looper.stem_cancellation import CANCEL_PATH_ENV
from flitzis_looper.system_resources import OfflineMemoryMonitor, stem_memory_snapshot

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.analysis.beat_cuda_cache import CudaDeviceIdentity
    from flitzis_looper.analysis.process import OwnedCommandResult
    from flitzis_looper.bs_onnx_installation import BsOnnxInstallation

_RECIPE = "bs-roformer-amp-stereo-485101-b2-b2-b1-v1"
_RATE = 44_100
_FRAMES = 485_101
_SOURCES = {"drums", "bass", "other", "vocals"}
_RECEIPT_BYTES = 256 * 1024
_GENERATION = re.compile(r"[0-9a-f]{8}")
_SHA256 = re.compile(r"[0-9a-f]{64}")
_WINDOWS_MAX_PATH = 260
# NVIDIA uses a two-character subdirectory and a 62-hex-digit binary name.
_KERNEL_PATH_SUFFIX = len("/00/") + 62 + len(".bin")
# On a portable layout only, shorten long paths with a private per-user cache.
# The full installation path contributes to every key in that shared root.
_USER_CACHE_FOLDER = "LoPrJit"

type _State = Literal["ready", "unavailable", "pending", "failed"]
type _WarmupRunner = Callable[[list[str], Path, dict[str, str]], OwnedCommandResult]


class _OwnerInterruptionError(Exception):
    """Preserve an application owner's interruption beyond cache settlement."""

    def __init__(self, error: RuntimeError) -> None:
        super().__init__(str(error))
        self.error = error


@dataclass(frozen=True, slots=True)
class _Preparation:
    cancel: Event
    runner: _WarmupRunner | None
    checkpoint: Callable[[], None] | None

    def check(self) -> None:
        if self.cancel.is_set():
            msg = "BS-RoFormer CUDA preparation cancelled"
            raise RuntimeError(msg)
        if self.checkpoint is not None:
            try:
                self.checkpoint()
            except RuntimeError as error:
                raise _OwnerInterruptionError(error) from error

    def run(self, arguments: list[str], cwd: Path, env: dict[str, str]) -> OwnedCommandResult:
        if self.runner is None:
            return _setup_warmup_runner(self.cancel)(arguments, cwd, env)
        try:
            return self.runner(arguments, cwd, env)
        except RuntimeError as error:
            # Only a settled child response can inform shared cache readiness.
            # In particular, caller playback pressure must never poison it.
            raise _OwnerInterruptionError(error) from error


@dataclass(frozen=True, slots=True)
class BsCudaCacheStatus:
    """Readiness and the verified directory to pass to a CUDA child."""

    status: _State
    detail: str
    cache_path: Path | None = None
    key: str | None = None
    warmup_executed: bool = False
    kernel_events: int | None = None
    seconds: float | None = None


@dataclass(frozen=True, slots=True)
class _Cache:
    root: Path
    key: str
    device: CudaDeviceIdentity
    generation: str | None = None

    @property
    def folder(self) -> Path:
        # The receipt always verifies the complete hash before reuse.
        return self.root / self.key[:24]

    @property
    def receipt(self) -> Path:
        return self.folder / "preparation.json"

    @property
    def kernels(self) -> Path:
        assert self.generation is not None
        return self.folder / self.generation

    def fits_windows(self) -> bool:
        prospective = replace(self, generation="0" * 8).kernels
        chars = len(str(prospective).encode("utf-16-le")) // 2
        return chars + _KERNEL_PATH_SUFFIX + 1 <= _WINDOWS_MAX_PATH


def _select(installation: BsOnnxInstallation, device: CudaDeviceIdentity) -> _Cache:
    _, package_digest = installation.package(cuda=True)
    inputs = {
        "recipe": _RECIPE,
        "device": asdict(device),
        "worker": installation.executable_sha256,
        "graph": installation.graph_digest(cuda=True),
        "weights": BS_ONNX_WEIGHTS_SHA256,
        "provider_package": package_digest,
        "provider": BS_ONNX_CUDA_PROVIDER_SHA256,
        "crt": installation.crt_files,
    }
    key = hashlib.sha256(json.dumps(inputs, sort_keys=True).encode("ascii")).hexdigest()
    cache = _Cache(installation.root.parent / "jit", key, device)
    if os.name != "nt" or cache.fits_windows():
        return cache
    local = os.environ.get("LOCALAPPDATA")
    user_cache = Path(local) if local else Path.home() / "AppData" / "Local"
    fallback = user_cache / _USER_CACHE_FOLDER
    installation_path = os.path.normcase(str(installation.root.parent))
    key = hashlib.sha256(f"{key}:{installation_path}".encode()).hexdigest()
    shorter = _Cache(fallback, key, device)
    return (
        shorter if fallback.is_absolute() and len(str(fallback)) < len(str(cache.root)) else cache
    )


def _inventory(directory: Path) -> dict[str, int]:
    _ordinary_tree(directory)
    files = {}
    for folder in directory.iterdir():
        if re.fullmatch(r"[0-9a-fA-F]{2}", folder.name):
            _ordinary(folder, directory=True)
            for path in folder.glob("*.bin"):
                size = _ordinary(path, directory=False).st_size
                if size > 0:
                    files[path.relative_to(directory).as_posix()] = size
    return files


def _read(cache: _Cache) -> dict[str, object]:
    try:
        _ordinary_tree(cache.folder)
        _ordinary(cache.receipt, directory=False)
        if os.name == "nt":
            with (
                _windows_read_handle(cache.receipt, directory=False, sharing=0x7) as handle,
                os.fdopen(os.dup(handle), "rb") as stream,
            ):
                raw = stream.read(_RECEIPT_BYTES + 1)
        else:
            with cache.receipt.open("rb") as stream:
                raw = stream.read(_RECEIPT_BYTES + 1)
    except FileNotFoundError:
        return {}
    if len(raw) > _RECEIPT_BYTES:
        return {}
    try:
        value = json.loads(raw)
    except UnicodeError, ValueError:
        return {}
    if (
        type(value) is dict
        and value.get("key") == cache.key
        and value.get("recipe") == _RECIPE
        and value.get("device") == asdict(cache.device)
    ):
        return value
    return {}


def _status(cache: _Cache, receipt: dict[str, object]) -> BsCudaCacheStatus:
    if receipt.get("status") == "failed" and isinstance(detail := receipt.get("detail"), str):
        return BsCudaCacheStatus("failed", detail, key=cache.key)
    generation = receipt.get("generation")
    files = receipt.get("files")
    count = receipt.get("kernel_events")
    seconds = receipt.get("seconds")
    valid_path = isinstance(generation, str) and _GENERATION.fullmatch(generation)
    if (
        receipt.get("status") == "ready"
        and valid_path
        and isinstance(files, dict)
        and files
        and _positive_int(count)
    ):
        assert isinstance(generation, str)
        directory = replace(cache, generation=generation).kernels
        try:
            observed = _inventory(directory)
        except FileNotFoundError:
            observed = {}
        if all(observed.get(name) == size for name, size in files.items()):
            return BsCudaCacheStatus(
                "ready",
                "BS-RoFormer CUDA cache is ready",
                directory,
                cache.key,
                kernel_events=count,
                seconds=seconds if _positive_seconds(seconds) else None,
            )
    return BsCudaCacheStatus(
        "pending", "BS-RoFormer CUDA cache requires preparation", key=cache.key
    )


def bs_cuda_cache_status(
    installation: BsOnnxInstallation, device: CudaDeviceIdentity | None
) -> BsCudaCacheStatus:
    """Check the installation's selected CUDA cache without updating it."""
    if device is None or not installation.cuda:
        return BsCudaCacheStatus("unavailable", "BS-RoFormer CUDA is unavailable")
    try:
        cache = _select(installation, device)
        return _status(cache, _read(cache))
    except (OSError, RuntimeError, ValueError) as error:
        return BsCudaCacheStatus("failed", f"BS-RoFormer CUDA cache inspection failed: {error}")


def _write_signal(path: Path, preparation: _Preparation) -> str:
    frames = _RATE // 2
    angular_rate = math.tau / _RATE
    left = (0.21 * math.sin(angular_rate * 173 * n) for n in range(frames))
    right = (0.17 * math.cos(angular_rate * 327 * n) for n in range(frames))
    block = struct.pack(
        f"<{2 * frames}f", *(value for pair in zip(left, right, strict=True) for value in pair)
    )
    repeats, tail = divmod(_FRAMES, frames)
    digest = hashlib.sha256()
    with path.open("xb") as stream:
        for piece in (*((block,) * repeats), block[: tail * 8]):
            preparation.check()
            stream.write(piece)
            digest.update(piece)
    return digest.hexdigest()


def _warmup_environment(
    installation: BsOnnxInstallation, cache: _Cache, work: Path
) -> dict[str, str]:
    package_path, package_digest = installation.package(cuda=True)
    env = child_environment({
        "SystemRoot": os.environ["SYSTEMROOT"],
        "TEMP": str(work),
        "TMP": str(work),
    })
    env.update({
        "CUDA_VISIBLE_DEVICES": cache.device.uuid,
        "CUDA_CACHE_PATH": str(cache.kernels),
        "CUDA_CACHE_DISABLE": "0",
        "FLITZIS_BS_ONNX_WARMUP": "1",
        "FLITZIS_TRIAL_ORT_PACKAGE": str(package_path),
        "FLITZIS_TRIAL_ORT_PACKAGE_SHA256": package_digest,
        "FLITZIS_TRIAL_ORT_LOAD_POLICY": "package-system32",
        "FLITZIS_BEAT_THIS_CRT": str(installation.root / "crt"),
        CANCEL_PATH_ENV: str(work / "cancel"),
    })
    return env


def _positive_int(value: object) -> TypeIs[int]:
    return type(value) is int and value > 0


def _positive_seconds(value: object) -> TypeIs[float]:
    return type(value) is float and math.isfinite(value) and value > 0


def _cuda_receipt_matches(value: object, cache: _Cache, package: str) -> bool:
    if type(value) is not dict or not isinstance(uuid := value.get("device_uuid"), str):
        return False
    return (
        uuid.lower() == cache.device.uuid.lower()
        and value.get("package_sha256") == package
        and value.get("provider_sha256") == BS_ONNX_CUDA_PROVIDER_SHA256
    )


def _output_receipt_matches(value: object) -> TypeIs[dict[str, str]]:
    return (
        type(value) is dict
        and set(value) == _SOURCES
        and all(isinstance(digest, str) and _SHA256.fullmatch(digest) for digest in value.values())
    )


def _checked_execution(
    data: bytes, installation: BsOnnxInstallation, cache: _Cache, digest: str
) -> tuple[int, dict[str, str]]:
    receipt = json.loads(data)
    package = installation.package(cuda=True)[1]
    expected = {
        "schema_version": 1,
        "mode": "track",
        "device": "cuda",
        "provider": "CUDAExecutionProvider",
        "ort_version": "1.24.4",
        "ort_api": 24,
        "ort_package_sha256": package,
        "graph_sha256": installation.graph_digest(cuda=True),
        "weights_sha256": BS_ONNX_WEIGHTS_SHA256,
        "frames": _FRAMES,
        "sample_rate_hz": _RATE,
        "channels": 2,
        "pcm_sha256": digest,
    }
    if type(receipt) is dict and all(
        receipt.get(name) == value for name, value in expected.items()
    ):
        cuda = receipt.get("cuda")
        events = receipt.get("cuda_kernel_events")
        outputs = receipt.get("outputs")
        if (
            _cuda_receipt_matches(cuda, cache, package)
            and _positive_int(events)
            and _output_receipt_matches(outputs)
        ):
            return events, outputs
    msg = "BS-RoFormer CUDA preparation lacked a matching completed CUDA execution receipt"
    raise RuntimeError(msg)


def _verify_outputs(directory: Path, outputs: dict[str, str], preparation: _Preparation) -> None:
    for name, digest in outputs.items():
        path = directory / f"{name}.wav"
        _ordinary(path, directory=False)
        with PcmWavReader(path) as wav:
            geometry = (
                wav.getnframes(),
                wav.getframerate(),
                wav.getnchannels(),
                wav.getsampwidth(),
            )
            if (
                geometry != (_FRAMES, _RATE, 2, 2)
                or wav.verify(checkpoint=preparation.check) != digest
            ):
                msg = f"BS-RoFormer CUDA warmup output changed: {name}"
                raise RuntimeError(msg)


def _setup_warmup_runner(cancel: Event) -> _WarmupRunner:
    retired = Event()
    monitor = OfflineMemoryMonitor(stem_memory_snapshot, monotonic)

    def stop() -> str | None:
        return monitor.observe().refusal_reason

    def run(arguments: list[str], _cwd: Path, env: dict[str, str]) -> OwnedCommandResult:
        with offline_work_scope(cancel):
            result = run_owned_command(
                arguments,
                cwd=bs_onnx_worker_cwd(env["SystemRoot"]),
                env=env,
                cancel=cancel,
                retired=retired,
                strict_completion=True,
                stop_reason=stop,
                before_resume=stop,
            )
        if not retired.is_set():
            msg = "BS-RoFormer CUDA warmup did not retire its process and readers"
            raise RuntimeError(msg)
        return result

    return run


def _warmup(
    installation: BsOnnxInstallation, cache: _Cache, preparation: _Preparation
) -> tuple[int, float]:
    started = monotonic()
    with TemporaryDirectory(prefix="bs-cuda-warmup-", dir=cache.root) as temporary:
        work = Path(temporary)
        pcm = work / "warmup.f32le"
        digest = _write_signal(pcm, preparation)
        result = preparation.run(
            [
                str(installation.executable),
                "--cuda",
                "track",
                str(pcm),
                str(_FRAMES),
                digest,
                str(work),
                installation.graph_digest(cuda=True),
            ],
            work,
            _warmup_environment(installation, cache, work),
        )
        if result.reason != "ok" or result.returncode != 0:
            detail = result.stderr.decode(errors="replace").strip()[-1500:]
            msg = f"BS-RoFormer CUDA warmup failed ({result.reason}): {detail}"
            raise RuntimeError(msg)
        events, outputs = _checked_execution(result.stdout, installation, cache, digest)
        _verify_outputs(work, outputs, preparation)
    return events, monotonic() - started


def _publish(cache: _Cache, status: BsCudaCacheStatus, files: dict[str, int]) -> None:
    temporary = cache.folder / f".preparation-{uuid4().hex}.json"
    data = json.dumps(
        {
            "key": cache.key,
            "recipe": _RECIPE,
            "device": asdict(cache.device),
            "generation": cache.generation,
            "status": status.status,
            "detail": status.detail[:1600],
            "kernel_events": status.kernel_events,
            "seconds": status.seconds,
            "files": files,
        },
        sort_keys=True,
    ).encode("utf-8")
    if len(data) > _RECEIPT_BYTES:
        msg = "BS-RoFormer CUDA cache inventory exceeds its metadata bound"
        raise ValueError(msg)
    try:
        with temporary.open("xb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(cache.receipt)
    finally:
        temporary.unlink(missing_ok=True)


def _confirm_cache(cache: _Cache) -> dict[str, int]:
    if cuda_device_identity() != cache.device:
        msg = "NVIDIA device or driver changed during BS-RoFormer CUDA preparation"
        raise RuntimeError(msg)
    files = _inventory(cache.kernels)
    if not files:
        msg = "BS-RoFormer CUDA preparation produced no kernel cache files"
        raise RuntimeError(msg)
    return files


def _cancelled(key: str | None = None) -> BsCudaCacheStatus:
    return BsCudaCacheStatus("pending", "BS-RoFormer CUDA preparation cancelled", key=key)


def _prepare(
    cache: _Cache, installation: BsOnnxInstallation, preparation: _Preparation
) -> BsCudaCacheStatus:
    cache = replace(cache, generation=uuid4().hex[:8])
    cache.folder.mkdir(exist_ok=True)
    with ExitStack() as custody:
        _pin_ancestors(custody, cache.folder)
        cache.kernels.mkdir()
        _pin_ancestors(custody, cache.kernels)
        try:
            events, seconds = _warmup(installation, cache, preparation)
            files = _confirm_cache(cache)
        except (EOFError, MemoryError, OSError, RuntimeError, ValueError, wave.Error) as error:
            status = BsCudaCacheStatus(
                "failed", f"BS-RoFormer CUDA preparation failed: {error}", key=cache.key
            )
            files = {}
        else:
            status = BsCudaCacheStatus(
                "ready",
                "BS-RoFormer CUDA cache is ready",
                cache.kernels,
                cache.key,
                warmup_executed=True,
                kernel_events=events,
                seconds=seconds,
            )
        if preparation.cancel.is_set():
            return _cancelled(cache.key)
        _publish(cache, status, files)
    return status


def _needs_preparation(status: BsCudaCacheStatus, *, retry_failed: bool) -> bool:
    return status.status == "pending" or (status.status == "failed" and retry_failed)


def _ensure(
    installation: BsOnnxInstallation,
    device: CudaDeviceIdentity,
    preparation: _Preparation,
    *,
    retry_failed: bool,
) -> BsCudaCacheStatus:
    cache = _select(installation, device)
    initial = _read(cache)
    status = _status(cache, initial)
    if not _needs_preparation(status, retry_failed=retry_failed):
        return status
    preparation.check()
    with heavy_work_scope(preparation.cancel), _cache_lock(cache.root, preparation.cancel):
        preparation.check()
        if cuda_device_identity() != device:
            return BsCudaCacheStatus(
                "pending", "NVIDIA device or driver changed before preparation"
            )
        receipt = _read(cache)
        status = _status(cache, receipt)
        if _needs_preparation(status, retry_failed=retry_failed and receipt == initial):
            return _prepare(cache, installation, preparation)
        return status


def ensure_bs_cuda_cache(
    installation: BsOnnxInstallation,
    device: CudaDeviceIdentity | None,
    cancel: Event | None = None,
    *,
    retry_failed: bool = False,
    runner: _WarmupRunner | None = None,
    checkpoint: Callable[[], None] | None = None,
) -> BsCudaCacheStatus:
    """Prepare once under existing offline ownership and the process-shared lock.

    An application runner must settle all child, tree and reader ownership before
    returning. Its checkpoint retains the caller's cancellation/resource policy.
    """
    if device is None or not installation.cuda:
        return BsCudaCacheStatus("unavailable", "BS-RoFormer CUDA is unavailable")
    cancel = cancel if cancel is not None else Event()
    try:
        preparation = _Preparation(cancel, runner, checkpoint)
        return _ensure(installation, device, preparation, retry_failed=retry_failed)
    except _OwnerInterruptionError as error:
        raise error.error from None
    except (MemoryError, OSError, RuntimeError, ValueError) as error:
        if cancel.is_set():
            return _cancelled()
        return BsCudaCacheStatus("failed", f"BS-RoFormer CUDA preparation failed: {error}")
