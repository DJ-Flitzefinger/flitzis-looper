"""Install and supervise the Windows x64 Native BeatThis CPU worker."""

import hashlib
import json
import math
import os
import stat
import struct
from contextlib import ExitStack
from dataclasses import dataclass
from functools import partial
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Event
from typing import TYPE_CHECKING, Literal

from pydantic import ConfigDict, FiniteFloat, TypeAdapter

from flitzis_looper.analysis.acquisition import copy_to_quarantine
from flitzis_looper.analysis.artifacts import NATIVE_MODEL, Digest, file_digest
from flitzis_looper.analysis.contracts import (
    BeatPredictions,
    WorkerLimits,
    validate_predictions,
)
from flitzis_looper.analysis.process import offline_work_scope, run_owned_command
from flitzis_looper.offline_runtime import child_environment
from flitzis_looper.recording_paths import _pin_ancestors, _windows_pin
from flitzis_looper.stem_cancellation import CANCEL_PATH_ENV
from flitzis_looper_audio import OfflineChildMemoryReservation

if TYPE_CHECKING:
    from flitzis_looper.analysis.contracts import MonoPcmInput

NATIVE_PROFILE = "native-cpu-v3"
_WORKER_EXE = "native-beat-this-worker.exe"
_EXE_LIMIT = 64 * 1024 * 1024
_MANIFEST_LIMIT = 4096
_ORT_MANIFEST = "ort/step23-runtime.json"
_ORT_PACKAGE = "ort/step23-package.json"
_PACKAGE_SHA = "7a5e216d9cc35418e8c38afa293ece04e86abc63153553fe56e81aa901cf2319"
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
    _ORT_PACKAGE: (474, _PACKAGE_SHA),
}


@dataclass(frozen=True, slots=True)
class NativeWorkerManifest:
    __pydantic_config__ = ConfigDict(extra="forbid")

    schema_version: Literal[3]
    profile: Literal["native-cpu-v3"]
    executable_sha256: Digest


_MANIFEST = TypeAdapter(NativeWorkerManifest)


@dataclass(frozen=True, slots=True)
class NativeWorkerConfiguration:
    manifest: Path
    scratch_dir: Path
    executable_sha256: str


@dataclass(frozen=True, slots=True)
class _NativeResult:
    __pydantic_config__ = ConfigDict(extra="forbid")

    schema_version: Literal[1]
    mono_sha256: Digest
    frame_count: int
    sample_rate_hz: int
    graph_sha256: Digest
    ort_version: Literal["1.24.4"]
    ort_api: Literal[24]
    provider: Literal["CPUExecutionProvider"]
    logit_count: int
    beat_seconds: tuple[FiniteFloat, ...]
    downbeat_seconds: tuple[FiniteFloat, ...]


_RESULT = TypeAdapter(_NativeResult)


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


def _package_files(root: Path, *, installed: bool = True) -> None:
    # Both DLL loading directories are closed sets. A sibling DLL may affect
    # dependency resolution even when the primary library's bytes are pinned.
    assets = _ASSETS if installed else _SOURCE_ASSETS
    allowed = {_WORKER_EXE, "beat-this-dynamic.onnx", "soxr.dll", "ort", "manifest.json"}
    if {path.name for path in root.iterdir()} - allowed:
        message = "unexpected Native BeatThis package entry"
        raise ValueError(message)
    ort = root / "ort"
    if ort.is_symlink() or ort.is_junction() or not ort.is_dir():
        message = "invalid Native BeatThis ORT directory"
        raise ValueError(message)
    if {path.name for path in ort.iterdir()} != {Path(name).name for name in assets if "/" in name}:
        message = "unexpected Native BeatThis ORT package entries"
        raise ValueError(message)
    _plain_file(root / _WORKER_EXE, _EXE_LIMIT)
    for name, (size, _) in assets.items():
        if _plain_file(root / name, size) != size:
            message = f"Native BeatThis asset extent differs: {name}"
            raise ValueError(message)


def load_native_configuration(
    manifest: Path, raw: bytes, scratch_dir: Path
) -> NativeWorkerConfiguration | None:
    """Recognize only the closed third profile in the existing installation pointer."""
    if len(raw) > _MANIFEST_LIMIT:
        return None  # Legacy v1/v2 may contain their entire venv inventory.
    payload = json.loads(raw)
    if not isinstance(payload, dict) or payload.get("profile") != NATIVE_PROFILE:
        return None
    record = _MANIFEST.validate_json(raw, strict=True)
    return NativeWorkerConfiguration(manifest, scratch_dir, record.executable_sha256)


def verify_native_package(configuration: NativeWorkerConfiguration) -> None:
    root = configuration.manifest.parent
    _package_files(root)
    for name, (size, expected) in _ASSETS.items():
        if file_digest(root / name, max_bytes=size) != expected:
            message = f"Native BeatThis asset digest differs: {name}"
            raise ValueError(message)
    if file_digest(root / _WORKER_EXE, max_bytes=_EXE_LIMIT) != configuration.executable_sha256:
        message = "Native BeatThis executable digest differs"
        raise ValueError(message)


def stage_native_package(package: Path, stage: Path) -> None:
    """Copy only named, checked artifacts from an explicit local package."""
    if not package.is_absolute() or package.is_symlink() or package.is_junction():
        message = "Native BeatThis package must be an absolute ordinary directory"
        raise ValueError(message)
    _package_files(package, installed=False)
    (stage / "ort").mkdir()
    for name, (size, expected) in _SOURCE_ASSETS.items():
        copied = copy_to_quarantine(package / name, stage / name, max_bytes=size)
        if copied.size_bytes != size or copied.sha256 != expected:
            message = f"Native BeatThis package bytes differ: {name}"
            raise ValueError(message)
    _write_ort_manifests(stage)
    binary = copy_to_quarantine(package / _WORKER_EXE, stage / _WORKER_EXE, max_bytes=_EXE_LIMIT)
    (stage / "manifest.json").write_bytes(
        _MANIFEST.dump_json(NativeWorkerManifest(3, NATIVE_PROFILE, binary.sha256))
    )


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


def _cancel_marker(marker: Path) -> None:
    with marker.open("xb") as stream:
        stream.write(b"cancel")


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


def run_native_cpu(
    configuration: NativeWorkerConfiguration, pcm: MonoPcmInput, mono_sha: str, cancel: Event
) -> BeatPredictions:
    """Keep the borrowed PCM and all child readers under the original App owner."""
    from flitzis_looper.controller.stem_resources import StemResourceGuard  # noqa: PLC0415

    root = configuration.manifest.parent
    exe = root / _WORKER_EXE
    limits = WorkerLimits()
    guard = StemResourceGuard()
    with offline_work_scope(cancel), ExitStack() as pins:
        guard.entry()
        _pin_ancestors(pins, root / "ort")
        _package_files(root)
        descriptor = pins.enter_context(_windows_pin(exe, directory=False))
        meta = os.fstat(descriptor)
        if getattr(meta, "st_file_attributes", 0) & 0x400 or not 0 < meta.st_size <= _EXE_LIMIT:
            message = "invalid Native BeatThis executable handle"
            raise ValueError(message)
        with os.fdopen(os.dup(descriptor), "rb") as binary:
            if hashlib.file_digest(binary, "sha256").hexdigest() != configuration.executable_sha256:
                message = "Native BeatThis executable digest differs"
                raise ValueError(message)
        budget = OfflineChildMemoryReservation()
        try:
            with TemporaryDirectory(prefix="native-beat-", dir=configuration.scratch_dir) as name:
                out = Path(name)
                marker = out / "cancel"
                env = child_environment({
                    "SystemRoot": os.environ["SystemRoot"],
                    "TEMP": name,
                    "TMP": name,
                    CANCEL_PATH_ENV: str(marker),
                    "FLITZIS_TRIAL_ORT_PACKAGE": str(root / _ORT_PACKAGE),
                    "FLITZIS_TRIAL_ORT_PACKAGE_SHA256": _PACKAGE_SHA,
                    "FLITZIS_TRIAL_ORT_LOAD_POLICY": "package-system32",
                })
                result = run_owned_command(
                    [
                        str(exe),
                        str(pcm.path),
                        str(pcm.sample_rate_hz),
                        str(pcm.frame_count),
                        mono_sha,
                        name,
                    ],
                    cwd=out,
                    env=env,
                    cancel=cancel,
                    retired=Event(),
                    limits=limits,
                    request_abort=partial(_cancel_marker, marker),
                    abort_grace_seconds=0.4,
                    stop_reason=guard.command_stop_reason,
                    commit_limit_bytes=budget.limit_bytes,
                )
                if result.reason != "ok" or result.returncode != 0 or cancel.is_set():
                    message = (
                        "cancelled"
                        if cancel.is_set()
                        else f"Native BeatThis worker: {result.reason}"
                    )
                    raise RuntimeError(message)
                return _predictions(result.stdout, out, pcm, mono_sha, limits)
        finally:
            budget.close()


def _predictions(
    response: bytes, output: Path, pcm: MonoPcmInput, mono_sha: str, limits: WorkerLimits
) -> BeatPredictions:
    receipt = _RESULT.validate_json(response, strict=True)
    resampled = (2 * pcm.frame_count * 22_050 + pcm.sample_rate_hz) // (2 * pcm.sample_rate_hz)
    expected = resampled // 441 + 1
    if (
        receipt.mono_sha256 != mono_sha
        or receipt.graph_sha256 != NATIVE_MODEL.sha256
        or receipt.frame_count != pcm.frame_count
        or receipt.sample_rate_hz != pcm.sample_rate_hz
        or receipt.logit_count != expected
        or not 0 < expected <= limits.max_prediction_count
    ):
        message = "Native BeatThis receipt differs from the admitted PCM or graph"
        raise ValueError(message)
    beat = _read_logits(output / "beat.f32", expected)
    downbeat = _read_logits(output / "downbeat.f32", expected)
    predictions = BeatPredictions(receipt.beat_seconds, receipt.downbeat_seconds, beat, downbeat)
    validate_predictions(predictions, pcm, limits)
    return predictions
