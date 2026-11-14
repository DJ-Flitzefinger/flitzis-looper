"""Prepare the installation-owned CUDA kernel cache outside realtime work."""

import ctypes
import errno
import hashlib
import json
import math
import os
import re
import stat
import struct
from contextlib import ExitStack, contextmanager
from dataclasses import asdict, dataclass, field, replace
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Event
from typing import TYPE_CHECKING, Literal, TypedDict
from uuid import UUID, uuid4

from flitzis_looper.analysis import native_beat_worker
from flitzis_looper.analysis.acquisition import _change_lock
from flitzis_looper.analysis.artifacts import NATIVE_MODEL
from flitzis_looper.analysis.contracts import MonoPcmInput
from flitzis_looper.analysis.execution import NATIVE_CUDA_PACKAGE_SHA
from flitzis_looper.analysis.process import heavy_work_scope
from flitzis_looper.recording_paths import _pin_ancestors, _windows_read_handle

if TYPE_CHECKING:
    from collections.abc import Iterator

if os.name == "nt":
    from _ctypes import FreeLibrary  # noqa: PLC2701 -- Release the explicitly loaded DLL.

_SAMPLE_RATE = 48_000
_FRAMES = (1_452_000, 7_776_000, 384_000)
_RECIPE = "beat-this-fp32-repeated-half-second-v1"
_SYSTEM32 = 0x800
_REPARSE = 0x400
_RECEIPT_BYTES = 256 * 1024

type CacheState = Literal["ready", "unavailable", "pending", "failed"]


class CudaCacheStatus(TypedDict):
    """Current preparation state and the cache directory released for use."""

    status: CacheState
    cache_path: str | None
    detail: str
    device_uuid: str | None
    key: str | None
    warmup_executed: bool


@dataclass(frozen=True, slots=True)
class CudaDeviceIdentity:
    """The selected CUDA device UUID and its actual NVIDIA driver release."""

    uuid: str
    driver_release: str


@contextmanager
def _system_library(name: str) -> Iterator[ctypes.WinDLL]:
    dll = ctypes.WinDLL(name, winmode=_SYSTEM32)
    try:
        yield dll
    finally:
        FreeLibrary(dll._handle)


def _require_success(operation: str, status: int) -> None:
    if status != 0:
        message = f"{operation} failed with status {status}"
        raise RuntimeError(message)


class _CudaIdentityError(RuntimeError):
    """A present NVIDIA stack could not provide a complete device identity."""


def _cuda_ordinal_zero(driver: ctypes.WinDLL) -> str | None:
    driver.cuInit.argtypes = (ctypes.c_uint,)
    driver.cuInit.restype = ctypes.c_int
    driver.cuDeviceGet.argtypes = (ctypes.POINTER(ctypes.c_int), ctypes.c_int)
    driver.cuDeviceGet.restype = ctypes.c_int
    driver.cuDeviceGetUuid.argtypes = (ctypes.c_void_p, ctypes.c_int)
    driver.cuDeviceGetUuid.restype = ctypes.c_int
    status = driver.cuInit(0)
    if status == 100:  # CUDA_ERROR_NO_DEVICE.
        return None
    _require_success("CUDA driver initialization", status)
    device = ctypes.c_int()
    status = driver.cuDeviceGet(ctypes.byref(device), 0)
    if status in {100, 101}:  # No device at the worker's ordinal zero.
        return None
    _require_success("CUDA device selection", status)
    identifier = ctypes.create_string_buffer(16)
    _require_success("CUDA device UUID query", driver.cuDeviceGetUuid(identifier, device.value))
    if not any(identifier.raw):
        message = "CUDA device returned an empty UUID"
        raise ValueError(message)
    return f"GPU-{UUID(bytes=identifier.raw)}"


def _driver_release(nvml: ctypes.WinDLL) -> str:
    nvml.nvmlInit_v2.argtypes = ()
    nvml.nvmlInit_v2.restype = ctypes.c_int
    nvml.nvmlShutdown.argtypes = ()
    nvml.nvmlShutdown.restype = ctypes.c_int
    nvml.nvmlSystemGetDriverVersion.argtypes = (ctypes.c_char_p, ctypes.c_uint)
    nvml.nvmlSystemGetDriverVersion.restype = ctypes.c_int
    _require_success("NVML initialization", nvml.nvmlInit_v2())
    try:
        buffer = ctypes.create_string_buffer(80)
        _require_success("NVIDIA driver release query", nvml.nvmlSystemGetDriverVersion(buffer, 80))
        release = buffer.value.decode("ascii")
        if re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", release) is None:
            message = "NVML returned an invalid NVIDIA driver release"
            raise ValueError(message)
        return release
    finally:
        _require_success("NVML shutdown", nvml.nvmlShutdown())


def cuda_device_identity() -> CudaDeviceIdentity | None:
    """Observe the worker's Windows CUDA ordinal zero and its driver release.

    Return None when the NVIDIA driver or its ordinal-zero device is absent.
    Other driver failures propagate so callers can distinguish failed probes.
    No CUDA context, session or inference is created by this observation.
    """
    if os.name != "nt":
        return None
    try:
        with _system_library("nvcuda.dll") as driver:
            identifier = _cuda_ordinal_zero(driver)
    except AttributeError as error:
        message = f"The installed CUDA driver is missing a device identity function: {error}"
        raise _CudaIdentityError(message) from error
    except OSError as error:
        if error.winerror == 126:  # ERROR_MOD_NOT_FOUND.
            return None
        raise
    if identifier is None:
        return None
    try:
        with _system_library("nvml.dll") as nvml:
            return CudaDeviceIdentity(identifier, _driver_release(nvml))
    except (AttributeError, OSError) as error:
        message = f"The installed NVIDIA driver release could not be observed: {error}"
        raise _CudaIdentityError(message) from error


@dataclass(frozen=True, slots=True)
class _Cache:
    root: Path
    key: str
    identity: CudaDeviceIdentity
    generation: str | None = None

    @property
    def folder(self) -> Path:
        # The receipt retains the whole key; shorter path components leave room
        # for NVIDIA's own nested cache paths in a relocated installation.
        return self.root / self.key[:32]

    @property
    def kernels(self) -> Path:
        assert self.generation is not None
        return self.folder / f"kernels-{self.generation}"

    @property
    def receipt(self) -> Path:
        return self.folder / "preparation.json"


def _selection(
    configuration: native_beat_worker.NativeWorkerConfiguration,
) -> _Cache | CudaCacheStatus:
    identity = cuda_device_identity()
    if identity is None:
        return _status("unavailable", "No NVIDIA CUDA device is available at ordinal zero")
    if configuration.execution_policy == "cpu":
        return _status("pending", "Run setup to add the CUDA package for this NVIDIA device")
    inputs = {
        "device": asdict(identity),
        "graph": NATIVE_MODEL.sha256,
        "frontend": NATIVE_MODEL.frontend_id,
        "worker": configuration.executable_sha256,
        "provider_package": NATIVE_CUDA_PACKAGE_SHA,
        "crt": configuration.crt_files,
        "recipe": _RECIPE,
        "sample_rate": _SAMPLE_RATE,
        "warmup_frames": _FRAMES,
    }
    key = hashlib.sha256(json.dumps(inputs, sort_keys=True).encode()).hexdigest()
    return _Cache(configuration.manifest.parent.parent / "cuda-cache", key, identity)


def _status(
    state: CacheState, detail: str, cache: _Cache | None = None, *, executed: bool = False
) -> CudaCacheStatus:
    return {
        "status": state,
        "cache_path": str(cache.kernels) if state == "ready" and cache is not None else None,
        "detail": detail,
        "device_uuid": cache.identity.uuid if cache is not None else None,
        "key": cache.key if cache is not None else None,
        "warmup_executed": executed,
    }


def _ordinary(path: Path, *, directory: bool) -> os.stat_result:
    info = path.lstat()
    expected = stat.S_ISDIR if directory else stat.S_ISREG
    if getattr(info, "st_file_attributes", 0) & _REPARSE or stat.S_ISLNK(info.st_mode):
        message = "BeatThis CUDA cache paths must not traverse links or reparse points"
        raise ValueError(message)
    if not expected(info.st_mode):
        message = "BeatThis CUDA cache path has an unsupported file type"
        raise ValueError(message)
    return info


def _ordinary_tree(path: Path) -> None:
    for directory in reversed((path, *path.parents)):
        _ordinary(directory, directory=True)


def _kernel_files(cache: _Cache) -> dict[str, int]:
    files = {}
    try:
        _ordinary_tree(cache.kernels)
        for folder in cache.kernels.iterdir():
            if re.fullmatch(r"[0-9a-fA-F]{2}", folder.name) is None:
                continue
            _ordinary(folder, directory=True)
            for binary in folder.glob("*.bin"):
                size = _ordinary(binary, directory=False).st_size
                if size > 0:
                    files[binary.relative_to(cache.kernels).as_posix()] = size
    except FileNotFoundError:
        return {}
    return files


def _retained_kernel_files(cache: _Cache, required: object) -> bool:
    if not isinstance(required, dict) or not required:
        return False
    current = _kernel_files(cache)
    return all(
        isinstance(name, str) and current.get(name) == size for name, size in required.items()
    )


def _read_receipt(cache: _Cache) -> dict[str, object] | None:
    try:
        _ordinary_tree(cache.folder)
        _ordinary(cache.receipt, directory=False)
        if os.name == "nt":
            # Share delete so a read-only start check never stalls atomic publication.
            with (
                _windows_read_handle(cache.receipt, directory=False, sharing=0x7) as handle,
                os.fdopen(os.dup(handle), "rb") as stream,
            ):
                content = stream.read(_RECEIPT_BYTES + 1)
        else:
            with cache.receipt.open("rb") as stream:
                content = stream.read(_RECEIPT_BYTES + 1)
    except FileNotFoundError:
        return None
    except OSError as error:
        if error.winerror in {2, 3}:  # The publisher renamed it between stat and open.
            return None
        raise
    if len(content) > _RECEIPT_BYTES:
        return None
    try:
        receipt = json.loads(content)
    except UnicodeError, ValueError:
        return None
    return receipt if isinstance(receipt, dict) and receipt.get("key") == cache.key else None


def _cached_status(cache: _Cache, receipt: dict[str, object] | None) -> CudaCacheStatus:
    if (
        receipt is not None
        and receipt.get("schema") == 1
        and receipt.get("device") == asdict(cache.identity)
        and receipt.get("warmup_frames") == list(_FRAMES)
    ):
        detail = receipt.get("detail")
        if receipt.get("status") == "failed" and isinstance(detail, str):
            return _status("failed", detail, cache)
        events = receipt.get("kernel_events")
        generation = receipt.get("generation")
        valid_generation = isinstance(generation, str) and re.fullmatch(r"[0-9a-f]{12}", generation)
        if (
            receipt.get("status") == "ready"
            and isinstance(events, list)
            and len(events) == len(_FRAMES)
            and all(type(value) is int and 0 < value <= 100_000 for value in events)
            and valid_generation
        ):
            assert isinstance(generation, str)
            prepared = replace(cache, generation=generation)
            if _retained_kernel_files(prepared, receipt.get("kernel_files")):
                return _status("ready", "BeatThis CUDA cache is ready", prepared)
    return _status("pending", "BeatThis CUDA cache requires preparation", cache)


def cuda_cache_status(
    configuration: native_beat_worker.NativeWorkerConfiguration,
) -> CudaCacheStatus:
    """Inspect the current device and cache state without warming or writing."""
    try:
        cache = _selection(configuration)
    except (OSError, RuntimeError, ValueError) as error:
        return _status("failed", f"BeatThis CUDA cache inspection failed: {error}")
    else:
        if not isinstance(cache, _Cache):
            return cache

    try:
        return _cached_status(cache, _read_receipt(cache))
    except (OSError, ValueError) as error:
        return _status("failed", f"BeatThis CUDA cache inspection failed: {error}", cache)


@contextmanager
def _cache_lock(root: Path, cancel: Event) -> Iterator[None]:
    _ordinary_tree(root.parent)
    root.mkdir(exist_ok=True)
    lock = root / "preparation.lock"
    with ExitStack() as pins:
        _pin_ancestors(pins, root)
        if lock.exists() or lock.is_symlink():
            _ordinary(lock, directory=False)
        stream = pins.enter_context(lock.open("a+b"))
        while not cancel.is_set():
            try:
                _change_lock(stream, unlock=False)
            except OSError as error:
                if error.errno not in {errno.EACCES, errno.EAGAIN, errno.EDEADLK}:
                    raise
                cancel.wait(0.1)
            else:
                try:
                    yield
                finally:
                    _change_lock(stream, unlock=True)
                return
    message = "BeatThis CUDA preparation cancelled"
    raise RuntimeError(message)


def _write_receipt(
    cache: _Cache,
    state: CacheState,
    detail: str,
    events: list[int],
    kernel_files: dict[str, int] | None = None,
) -> None:
    identifier = uuid4().hex
    temporary = cache.folder / f".preparation-{identifier}.json"
    try:
        with temporary.open("x", encoding="utf-8") as stream:
            json.dump(
                {
                    "schema": 1,
                    "key": cache.key,
                    "attempt": identifier,
                    "device": asdict(cache.identity),
                    "status": state,
                    "detail": detail[:400],
                    "warmup_frames": _FRAMES,
                    "kernel_events": events,
                    "generation": cache.generation,
                    "kernel_files": kernel_files,
                },
                stream,
                ensure_ascii=False,
                separators=(",", ":"),
            )
            stream.flush()
            if stream.tell() > _RECEIPT_BYTES:
                message = "BeatThis CUDA cache inventory exceeds its metadata bound"
                raise ValueError(message)
            os.fsync(stream.fileno())
        temporary.replace(cache.receipt)
    finally:
        temporary.unlink(missing_ok=True)


def _warmup_signal() -> bytes:
    def sample(frame: int) -> float:
        time = frame / _SAMPLE_RATE
        phase = time % 0.5
        value = 0.10 * math.sin(2 * math.pi * 110 * time)
        value += 0.42 * math.exp(-45 * phase) * math.sin(2 * math.pi * 58 * phase)
        return value + 0.12 * math.exp(-110 * (phase % 0.25)) * math.sin(2 * math.pi * 670 * time)

    return struct.pack("<24000f", *(sample(frame) for frame in range(_SAMPLE_RATE // 2)))


def _write_pcm(path: Path, block: bytes, frames: int, cancel: Event) -> str:
    repeats, remainder = divmod(frames, _SAMPLE_RATE // 2)
    digest = hashlib.sha256()
    with path.open("wb") as stream:
        for _ in range(repeats):
            if cancel.is_set():
                message = "BeatThis CUDA preparation cancelled"
                raise RuntimeError(message)
            stream.write(block)
            digest.update(block)
        final = block[: remainder * 4]
        stream.write(final)
        digest.update(final)
    return digest.hexdigest()


@dataclass(slots=True)
class _Warmup:
    started: bool = False
    events: list[int] = field(default_factory=list)

    def run(
        self,
        cache: _Cache,
        configuration: native_beat_worker.NativeWorkerConfiguration,
        cancel: Event,
    ) -> None:
        strict = replace(configuration, execution_policy="cuda")
        block = _warmup_signal()
        with TemporaryDirectory(prefix="beat-this-warmup-", dir=configuration.scratch_dir) as work:
            path = Path(work) / "synthetic.f32le"
            for frames in _FRAMES:
                digest = _write_pcm(path, block, frames, cancel)
                self.started = True
                _, receipt = native_beat_worker.run_native_worker(
                    strict,
                    MonoPcmInput(path, _SAMPLE_RATE, frames),
                    digest,
                    cancel,
                    cuda_cache_path=cache.kernels,
                    cuda_device_uuid=cache.identity.uuid,
                )
                if (
                    receipt is None
                    or receipt.device != "cuda:0"
                    or receipt.cuda_kernel_events is None
                    or receipt.cuda_kernel_events <= 0
                ):
                    message = "BeatThis CUDA warmup lacked a completed CUDA execution receipt"
                    raise RuntimeError(message)
                self.events.append(receipt.cuda_kernel_events)

    def verify(self, cache: _Cache) -> dict[str, int]:
        if cuda_device_identity() != cache.identity:
            message = "NVIDIA device or driver release changed during BeatThis CUDA preparation"
            raise RuntimeError(message)
        files = _kernel_files(cache)
        if not files:
            message = "BeatThis CUDA preparation produced no kernel cache files"
            raise RuntimeError(message)
        return files


def _prepare(
    cache: _Cache,
    configuration: native_beat_worker.NativeWorkerConfiguration,
    cancel: Event,
) -> CudaCacheStatus:
    cache = replace(cache, generation=uuid4().hex[:12])
    cache.folder.mkdir(exist_ok=True)
    _ordinary(cache.folder, directory=True)
    cache.kernels.mkdir()
    _ordinary(cache.kernels, directory=True)
    cache.receipt.unlink(missing_ok=True)
    warmup = _Warmup()
    try:
        warmup.run(cache, configuration, cancel)
        if cancel.is_set():
            return _status(
                "pending", "BeatThis CUDA preparation cancelled", cache, executed=warmup.started
            )
        kernel_files = warmup.verify(cache)
        _write_receipt(cache, "ready", "BeatThis CUDA cache is ready", warmup.events, kernel_files)
    except (MemoryError, OSError, RuntimeError, ValueError) as error:
        if cancel.is_set():
            return _status(
                "pending", "BeatThis CUDA preparation cancelled", cache, executed=warmup.started
            )
        detail = f"BeatThis CUDA preparation failed: {error}"
        _write_receipt(cache, "failed", detail, warmup.events)
        return _status("failed", detail, cache, executed=warmup.started)
    return _status("ready", "BeatThis CUDA cache is ready", cache, executed=True)


def _ensure(
    configuration: native_beat_worker.NativeWorkerConfiguration,
    cancel: Event,
    *,
    retry_failed: bool,
) -> CudaCacheStatus:
    cache = _selection(configuration)
    if not isinstance(cache, _Cache):
        return cache
    initial = _read_receipt(cache)
    status = _cached_status(cache, initial)
    if status["status"] == "ready" or (status["status"] == "failed" and not retry_failed):
        return status
    # Preserve the same lock order when invoked by an existing BPM analysis owner.
    with heavy_work_scope(cancel), _cache_lock(cache.root, cancel):
        current = _selection(configuration)
        if not isinstance(current, _Cache):
            return current
        receipt = _read_receipt(current)
        status = _cached_status(current, receipt)
        if status["status"] == "ready" or (
            status["status"] == "failed" and (not retry_failed or receipt != initial)
        ):
            return status
        return _prepare(current, configuration, cancel)


def ensure_cuda_cache(
    configuration: native_beat_worker.NativeWorkerConfiguration,
    cancel: Event | None = None,
    *,
    retry_failed: bool = False,
) -> CudaCacheStatus:
    """Share preparation across background owners and installer processes.

    An explicit installer repair may retry an earlier settled failure. Background
    requests reuse that failure for its key until repair or a driver/package change.
    An interrupted preparation leaves its key pending for a later owner.
    """
    cancel = cancel if cancel is not None else Event()
    if cancel.is_set():
        return _status("pending", "BeatThis CUDA preparation cancelled")
    try:
        return _ensure(configuration, cancel, retry_failed=retry_failed)
    except (MemoryError, OSError, RuntimeError, ValueError) as error:
        if cancel.is_set():
            return _status("pending", "BeatThis CUDA preparation cancelled")
        return _status("failed", f"BeatThis CUDA preparation failed: {error}")
