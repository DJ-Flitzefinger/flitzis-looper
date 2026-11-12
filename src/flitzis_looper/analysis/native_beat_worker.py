"""Install and supervise the Windows x64 Native BeatThis worker."""

import hashlib
import json
import math
import os
import shutil
import stat
import struct
from contextlib import ExitStack, suppress
from dataclasses import dataclass
from functools import partial
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Event
from typing import TYPE_CHECKING, Final, Literal

from pydantic import ConfigDict, FiniteFloat, TypeAdapter, ValidationError

from flitzis_looper.analysis.acquisition import copy_to_quarantine
from flitzis_looper.analysis.artifacts import NATIVE_MODEL, Digest, file_digest
from flitzis_looper.analysis.contracts import (
    BeatPredictions,
    WorkerLimits,
    validate_predictions,
)
from flitzis_looper.analysis.execution import (
    NATIVE_CPU_PACKAGE_SHA,
    NATIVE_CUDA_PACKAGE_SHA,
    NativeCudaError,
    NativeCudaExecutionReceipt,
    NativeCudaPolicy,
    NativeCudaRuntime,
    validate_execution,
)
from flitzis_looper.analysis.process import offline_work_scope, run_owned_command
from flitzis_looper.offline_runtime import child_environment
from flitzis_looper.recording_paths import _pin_ancestors, _windows_pin
from flitzis_looper.stem_cancellation import CANCEL_PATH_ENV

if TYPE_CHECKING:
    from flitzis_looper.analysis.contracts import MonoPcmInput
    from flitzis_looper.controller.stem_resources import StemResourceGuard

NATIVE_PROFILE: Final = "native-cpu-v3"
NATIVE_CUDA_PROFILE: Final = "native-cuda-v4"
_WORKER_EXE = "native-beat-this-worker.exe"
_EXE_LIMIT = 64 * 1024 * 1024
_MANIFEST_LIMIT = 4096
_CANCEL_MARKER = b"cancel"
_ORT_MANIFEST = "ort/step23-runtime.json"
_ORT_PACKAGE = "ort/step23-package.json"
_CUDA_PACKAGE = "cuda/step23-package.json"
_CUDA_MANIFESTS = {
    "cuda/step23-runtime.json": (
        220,
        "7ebc26a630b9fb721539e7821b918cf9f4a9aa94b856dcc153b81a7b140b240d",
    ),
    _CUDA_PACKAGE: (4470, NATIVE_CUDA_PACKAGE_SHA),
}
_SOURCE_ASSETS = {
    "beat-this-dynamic.onnx": (82_104_250, NATIVE_MODEL.sha256),
    "soxr.dll": (228_352, "b2e3a0ff4806540250fa45e9522c3a3549663888397fce4708249369b25db37e"),
    "ort/onnxruntime.dll": (
        16_119_840,
        "87da0279ab54add397fd518f7022efe8b9a89fd968669d0adff9f9f242344b6c",
    ),
    "ort/onnxruntime_providers_shared.dll": (
        22_048,
        "838048bae6e1d040e41e4d428a831ac9c5bc91da24abced383d42cb24f2b1376",
    ),
}
_ASSETS = {
    **_SOURCE_ASSETS,
    _ORT_MANIFEST: (220, "4929eb4f6a3aebe2c117850a2e1354c48bc004bfa5ff4d805c2ce0ecbef2915d"),
    _ORT_PACKAGE: (474, NATIVE_CPU_PACKAGE_SHA),
}


@dataclass(frozen=True, slots=True)
class NativeWorkerManifest:
    __pydantic_config__ = ConfigDict(extra="forbid")

    schema_version: Literal[3]
    profile: Literal["native-cpu-v3"]
    executable_sha256: Digest


@dataclass(frozen=True, slots=True)
class NativeCudaWorkerManifest:
    __pydantic_config__ = ConfigDict(extra="forbid")

    schema_version: Literal[4]
    profile: Literal["native-cuda-v4"]
    executable_sha256: Digest
    execution_policy: NativeCudaPolicy


_MANIFEST = TypeAdapter(NativeWorkerManifest)
_CUDA_MANIFEST = TypeAdapter(NativeCudaWorkerManifest)


@dataclass(frozen=True, slots=True)
class NativeWorkerConfiguration:
    manifest: Path
    scratch_dir: Path
    executable_sha256: str
    execution_policy: Literal["cpu", "cuda", "auto"] = "cpu"


@dataclass(frozen=True, slots=True)
class _NativeInput:
    __pydantic_config__ = ConfigDict(extra="forbid")

    schema_version: Literal[1]
    mono_sha256: Digest
    frame_count: int
    sample_rate_hz: int
    graph_sha256: Digest


@dataclass(frozen=True, slots=True)
class _NativePredictions(_NativeInput):
    ort_version: Literal["1.24.4"]
    ort_api: Literal[24]
    logit_count: int
    beat_seconds: tuple[FiniteFloat, ...]
    downbeat_seconds: tuple[FiniteFloat, ...]


@dataclass(frozen=True, slots=True)
class _NativeResult(_NativePredictions):
    provider: Literal["CPUExecutionProvider"]


@dataclass(frozen=True, slots=True)
class _NativeCudaResult(_NativePredictions):
    provider: Literal["CUDAExecutionProvider"]
    cuda: NativeCudaRuntime
    cuda_kernel_events: int


@dataclass(frozen=True, slots=True)
class _NativeCudaFault(_NativeInput):
    cuda: NativeCudaRuntime
    cuda_error: NativeCudaError


_RESULT = TypeAdapter(_NativeResult)
_CUDA_RESULT = TypeAdapter(_NativeCudaResult)
_CUDA_FAULT = TypeAdapter(_NativeCudaFault)


def _plain_file(path: Path, maximum: int) -> int:
    info = path.lstat()
    if (
        not stat.S_ISREG(info.st_mode)
        or getattr(info, "st_file_attributes", 0) & 0x400
        or not 0 < info.st_size <= maximum
    ):
        message = f"invalid Native BeatThis package file: {path.name}"
        raise ValueError(message)
    return info.st_size


def _cuda_assets(root: Path) -> dict[str, tuple[int, str]]:
    # Read the pinned inventory before allowing it to name any path.
    size, digest = _CUDA_MANIFESTS[_CUDA_PACKAGE]
    package = root / _CUDA_PACKAGE
    if _plain_file(package, size) != size:
        msg = "Native BeatThis CUDA package manifest extent differs"
        raise ValueError(msg)
    with package.open("rb") as stream:
        raw = stream.read(size + 1)
    if len(raw) != size or hashlib.sha256(raw).hexdigest() != digest:
        msg = "Native BeatThis CUDA package manifest digest differs"
        raise ValueError(msg)
    return {
        **_CUDA_MANIFESTS,
        **{
            f"cuda/{item['name']}": (item["bytes"], item["sha256"])
            for item in json.loads(raw)["components"]
        },
    }


def _package_files(
    root: Path, *, cuda: bool = False, installed: bool = True
) -> dict[str, tuple[int, str]]:
    # Both DLL loading directories are closed sets.
    assets = dict(_ASSETS if installed else _SOURCE_ASSETS)
    directories = ("ort", "cuda") if cuda else ("ort",)
    allowed = {_WORKER_EXE, "beat-this-dynamic.onnx", "soxr.dll", "manifest.json", *directories}
    if {path.name for path in root.iterdir()} - allowed:
        msg = "unexpected Native BeatThis package entry"
        raise ValueError(msg)
    for directory in directories:
        folder = root / directory
        if folder.is_symlink() or folder.is_junction() or not folder.is_dir():
            msg = f"invalid Native BeatThis runtime directory: {directory}"
            raise ValueError(msg)
        if directory == "cuda":
            assets.update(_cuda_assets(root))
        expected = {Path(name).name for name in assets if name.startswith(f"{directory}/")}
        if {path.name for path in folder.iterdir()} != expected:
            msg = f"unexpected Native BeatThis runtime entries: {directory}"
            raise ValueError(msg)
    _plain_file(root / _WORKER_EXE, _EXE_LIMIT)
    for name, (size, _) in assets.items():
        if _plain_file(root / name, size) != size:
            msg = f"Native BeatThis asset extent differs: {name}"
            raise ValueError(msg)
    return assets


class NativeWorkerConfigurationError(ValueError):
    """A selected Native manifest was identified but cannot be used."""


def load_native_configuration(
    manifest: Path, raw: bytes, scratch_dir: Path
) -> NativeWorkerConfiguration | None:
    """Recognize the closed Native profiles in the existing installation pointer."""
    payload = json.loads(raw)
    profile = payload.get("profile") if isinstance(payload, dict) else None
    if not isinstance(profile, str) or profile not in {NATIVE_PROFILE, NATIVE_CUDA_PROFILE}:
        return None
    if len(raw) > _MANIFEST_LIMIT:
        message = "Native BeatThis manifest exceeds its byte limit"
        raise NativeWorkerConfigurationError(message)
    try:
        if profile == NATIVE_PROFILE:
            cpu = _MANIFEST.validate_json(raw, strict=True)
            return NativeWorkerConfiguration(manifest, scratch_dir, cpu.executable_sha256)
        cuda = _CUDA_MANIFEST.validate_json(raw, strict=True)
        return NativeWorkerConfiguration(
            manifest, scratch_dir, cuda.executable_sha256, cuda.execution_policy
        )
    except ValidationError as error:
        message = "Selected Native BeatThis manifest differs from the supported profile"
        raise NativeWorkerConfigurationError(message) from error


def verify_native_package(configuration: NativeWorkerConfiguration) -> None:
    """Verify the retained worker and its exact installed assets."""
    root = configuration.manifest.parent
    assets = _package_files(root, cuda=configuration.execution_policy != "cpu")
    for name, (size, expected) in assets.items():
        if file_digest(root / name, max_bytes=size) != expected:
            msg = f"Native BeatThis asset digest differs: {name}"
            raise ValueError(msg)
    if file_digest(root / _WORKER_EXE, max_bytes=_EXE_LIMIT) != configuration.executable_sha256:
        msg = "Native BeatThis executable digest differs"
        raise ValueError(msg)


def stage_native_package(
    package: Path, stage: Path, *, cuda_policy: NativeCudaPolicy | None = None
) -> None:
    """Copy only named, checked artifacts from an explicit local package."""
    if not package.is_absolute() or package.is_symlink() or package.is_junction():
        msg = "Native BeatThis package must be an absolute ordinary directory"
        raise ValueError(msg)
    assets = _package_files(package, cuda=cuda_policy is not None, installed=False)
    (stage / "ort").mkdir()
    if cuda_policy is not None:
        (stage / "cuda").mkdir()
    for name, (size, expected) in assets.items():
        copied = copy_to_quarantine(package / name, stage / name, max_bytes=size)
        if copied.size_bytes != size or copied.sha256 != expected:
            msg = f"Native BeatThis package bytes differ: {name}"
            raise ValueError(msg)
    _write_ort_manifests(stage)
    binary = copy_to_quarantine(package / _WORKER_EXE, stage / _WORKER_EXE, max_bytes=_EXE_LIMIT)
    if cuda_policy is None:
        manifest = _MANIFEST.dump_json(NativeWorkerManifest(3, NATIVE_PROFILE, binary.sha256))
    else:
        manifest = _CUDA_MANIFEST.dump_json(
            NativeCudaWorkerManifest(4, NATIVE_CUDA_PROFILE, binary.sha256, cuda_policy)
        )
    (stage / "manifest.json").write_bytes(manifest)


def _write_ort_manifests(stage: Path) -> None:
    header = {"schema_version": 1, "os": "windows", "arch": "x86_64"}
    runtime = "onnxruntime.dll"
    size, sha = _SOURCE_ASSETS[f"ort/{runtime}"]
    manifest = {**header, "library": runtime, "bytes": size, "sha256": sha, "version": "1.24.4"}
    components = []
    for library, role in (
        (runtime, "ort-runtime"),
        ("onnxruntime_providers_shared.dll", "ort-provider-shared"),
    ):
        size, sha = _SOURCE_ASSETS[f"ort/{library}"]
        components.append({"name": library, "role": role, "bytes": size, "sha256": sha})
    for name, value in (
        (_ORT_MANIFEST, manifest),
        (_ORT_PACKAGE, {**header, "components": components}),
    ):
        data = json.dumps(value, indent=2).encode("utf-8")
        size, sha = _ASSETS[name]
        if len(data) != size or hashlib.sha256(data).hexdigest() != sha:
            message = "Native BeatThis ORT manifest bytes differ from the loader pin"
            raise ValueError(message)
        with (stage / name).open("xb") as stream:
            stream.write(data)


def require_native_cpu_capability() -> None:
    from flitzis_looper_audio import AudioEngine  # noqa: PLC0415

    version = getattr(AudioEngine, "BEAT_THIS_NATIVE_CPU_API_VERSION", None)
    if os.name != "nt" or type(version) is not int or version != 1:
        message = "Native BeatThis CPU selection requires the matching App Native library"
        raise ValueError(message)


def require_native_worker_capability(configuration: NativeWorkerConfiguration) -> None:
    """Require the selected worker protocol on the App library before activation."""
    if configuration.execution_policy == "cpu":
        require_native_cpu_capability()
        return
    from flitzis_looper_audio import AudioEngine  # noqa: PLC0415

    version = getattr(AudioEngine, "BEAT_THIS_NATIVE_CUDA_API_VERSION", None)
    if os.name != "nt" or type(version) is not int or version != 1:
        msg = "Native BeatThis CUDA selection requires the matching App Native library"
        raise ValueError(msg)


def _cancel_marker(marker: Path) -> None:
    # A failed cooperative marker still leaves the owning Job able to stop the
    # process. Keep the original cancel or resource reason through that teardown.
    with suppress(OSError), marker.open("xb") as stream:
        stream.write(_CANCEL_MARKER)


def _read_logits(path: Path, count: int) -> tuple[float, ...]:
    length = count * 4
    with path.open("rb") as stream:
        data = stream.read(length + 1)
    if len(data) != length:
        message = "Native BeatThis logit file extent differs"
        raise ValueError(message)
    logits = struct.unpack(f"<{count}f", data)
    if not all(map(math.isfinite, logits)):
        message = "Native BeatThis logits are nonfinite"
        raise ValueError(message)
    return logits


@dataclass(frozen=True, slots=True)
class _Attempt:
    executable: Path
    scratch: Path
    pcm: MonoPcmInput
    mono_sha: str
    cancel: Event
    guard: StemResourceGuard
    limits: WorkerLimits

    def run(
        self, *, cuda: bool, allow_fallback: bool = False
    ) -> tuple[BeatPredictions, _NativePredictions] | _NativeCudaFault:
        if self.cancel.is_set():
            msg = "cancelled"
            raise RuntimeError(msg)
        if reason := self.guard.entry_reason():
            raise RuntimeError(reason)
        count = _expected_logit_count(self.pcm, self.limits)
        # Two float32 logit files and the cooperative cancel marker have known extents.
        required_bytes = 2 * count * 4 + len(_CANCEL_MARKER)
        with TemporaryDirectory(prefix="native-beat-", dir=self.scratch) as name:
            output = Path(name)
            if shutil.disk_usage(output).free < required_bytes:
                reason = "resource_disk_full"
                raise RuntimeError(reason)
            marker = output / "cancel"
            package = _CUDA_PACKAGE if cuda else _ORT_PACKAGE
            digest = NATIVE_CUDA_PACKAGE_SHA if cuda else NATIVE_CPU_PACKAGE_SHA
            env = child_environment({
                "SystemRoot": os.environ["SYSTEMROOT"],
                "TEMP": name,
                "TMP": name,
                CANCEL_PATH_ENV: str(marker),
                "FLITZIS_TRIAL_ORT_PACKAGE": str(self.executable.parent / package),
                "FLITZIS_TRIAL_ORT_PACKAGE_SHA256": digest,
                "FLITZIS_TRIAL_ORT_LOAD_POLICY": "package-system32",
            })
            if cuda:
                cache = output / "cuda-cache"
                cache.mkdir()
                env["CUDA_CACHE_PATH"] = str(cache)
            retired = Event()
            result = run_owned_command(
                [
                    str(self.executable),
                    *(("--cuda",) if cuda else ()),
                    str(self.pcm.path),
                    str(self.pcm.sample_rate_hz),
                    str(self.pcm.frame_count),
                    self.mono_sha,
                    name,
                ],
                cwd=output,
                env=env,
                cancel=self.cancel,
                retired=retired,
                limits=self.limits,
                request_abort=partial(_cancel_marker, marker),
                abort_grace_seconds=0.4,
                stop_reason=self.guard.command_stop_reason,
                strict_completion=True,
                before_resume=self.guard.entry_reason,
            )
            if not retired.is_set():
                # The supervisor has synchronous custody. Never leave scope
                # or start a second attempt without its retirement event.
                retired.wait()
            if self.cancel.is_set():
                msg = "cancelled"
                raise RuntimeError(msg)
            if allow_fallback and result.reason == "worker_crashed" and result.returncode == 70:
                fault = _CUDA_FAULT.validate_json(result.stdout, strict=True)
                _validate_input(fault, self.pcm, self.mono_sha)
                return fault
            if result.reason != "ok" or result.returncode != 0:
                raise RuntimeError(result.reason)
            receipt = (
                _CUDA_RESULT.validate_json(result.stdout, strict=True)
                if cuda
                else _RESULT.validate_json(result.stdout, strict=True)
            )
            predictions = _read_predictions(
                receipt, output, self.pcm, self.mono_sha, count, self.limits
            )
            return predictions, receipt


def _run_selected(
    attempt: _Attempt, policy: Literal["cpu", "auto", "cuda"]
) -> tuple[BeatPredictions, NativeCudaExecutionReceipt | None]:
    if policy == "cpu":
        result = attempt.run(cuda=False)
        assert not isinstance(result, _NativeCudaFault)
        return result[0], None

    result = attempt.run(cuda=True, allow_fallback=policy == "auto")
    if isinstance(result, _NativeCudaFault):
        # The GPU job, both readers and scratch have ended. This
        # second guarded child uses the same executable, PCM and pinned graph.
        receipt = NativeCudaExecutionReceipt(
            1,
            policy,
            "cpu",
            None,
            "float32",
            "1.24.4",
            24,
            NATIVE_CPU_PACKAGE_SHA,
            result.cuda,
            None,
            "cuda_device_error",
            result.cuda_error,
        )
        validate_execution(receipt, policy)
        cpu = attempt.run(cuda=False)
        assert not isinstance(cpu, _NativeCudaFault)
        return cpu[0], receipt
    predictions, cuda_result = result
    assert isinstance(cuda_result, _NativeCudaResult)
    receipt = NativeCudaExecutionReceipt(
        1,
        policy,
        "cuda:0",
        cuda_result.cuda.device_name,
        "float32",
        cuda_result.ort_version,
        cuda_result.ort_api,
        NATIVE_CUDA_PACKAGE_SHA,
        cuda_result.cuda,
        cuda_result.cuda_kernel_events,
        None,
        None,
    )
    validate_execution(receipt, policy)
    return predictions, receipt


def run_native_worker(
    configuration: NativeWorkerConfiguration, pcm: MonoPcmInput, mono_sha: str, cancel: Event
) -> tuple[BeatPredictions, NativeCudaExecutionReceipt | None]:
    """Retain the selected installation and PCM owner through each settled child."""
    from flitzis_looper.controller.stem_resources import StemResourceGuard  # noqa: PLC0415

    root = configuration.manifest.parent
    exe = root / _WORKER_EXE
    use_cuda = configuration.execution_policy != "cpu"
    guard = StemResourceGuard()
    with offline_work_scope(cancel), ExitStack() as pins:
        if reason := guard.entry_reason():
            raise RuntimeError(reason)
        _pin_ancestors(pins, root / "ort")
        if use_cuda:
            _pin_ancestors(pins, root / "cuda")
        _package_files(root, cuda=use_cuda)
        descriptor = pins.enter_context(_windows_pin(exe, directory=False))
        meta = os.fstat(descriptor)
        if getattr(meta, "st_file_attributes", 0) & 0x400 or not 0 < meta.st_size <= _EXE_LIMIT:
            msg = "invalid Native BeatThis executable handle"
            raise ValueError(msg)
        with os.fdopen(os.dup(descriptor), "rb") as binary:
            if hashlib.file_digest(binary, "sha256").hexdigest() != configuration.executable_sha256:
                msg = "Native BeatThis executable digest differs"
                raise ValueError(msg)
        attempt = _Attempt(
            exe, configuration.scratch_dir, pcm, mono_sha, cancel, guard, WorkerLimits()
        )
        return _run_selected(attempt, configuration.execution_policy)


def _validate_input(receipt: _NativeInput, pcm: MonoPcmInput, mono_sha: str) -> None:
    if (
        receipt.mono_sha256 != mono_sha
        or receipt.graph_sha256 != NATIVE_MODEL.sha256
        or receipt.frame_count != pcm.frame_count
        or receipt.sample_rate_hz != pcm.sample_rate_hz
    ):
        msg = "Native BeatThis receipt differs from the admitted PCM or graph"
        raise ValueError(msg)


def _expected_logit_count(pcm: MonoPcmInput, limits: WorkerLimits) -> int:
    resampled = (2 * pcm.frame_count * 22_050 + pcm.sample_rate_hz) // (2 * pcm.sample_rate_hz)
    count = resampled // 441 + 1
    if not 0 < count <= limits.max_prediction_count:
        msg = "Native BeatThis input exceeds the logit extent limit"
        raise ValueError(msg)
    return count


def _read_predictions(
    receipt: _NativePredictions,
    output: Path,
    pcm: MonoPcmInput,
    mono_sha: str,
    expected: int,
    limits: WorkerLimits,
) -> BeatPredictions:
    _validate_input(receipt, pcm, mono_sha)
    if receipt.logit_count != expected:
        msg = "Native BeatThis receipt logit extent differs"
        raise ValueError(msg)
    beat = _read_logits(output / "beat.f32", expected)
    downbeat = _read_logits(output / "downbeat.f32", expected)
    predictions = BeatPredictions(receipt.beat_seconds, receipt.downbeat_seconds, beat, downbeat)
    validate_predictions(predictions, pcm, limits)
    return predictions
