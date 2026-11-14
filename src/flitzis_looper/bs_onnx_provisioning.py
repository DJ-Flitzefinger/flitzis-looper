"""Stage the locally derived Windows BS-RoFormer model and its native worker."""

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Event
from time import monotonic
from typing import TYPE_CHECKING
from uuid import uuid4

from flitzis_looper.analysis.acquisition import _download_lock
from flitzis_looper.analysis.artifacts import file_digest
from flitzis_looper.analysis.beat_cuda_cache import cuda_device_identity
from flitzis_looper.analysis.execution import NATIVE_CPU_PACKAGE_SHA, NATIVE_CUDA_PACKAGE_SHA
from flitzis_looper.analysis.native_beat_worker import CRT_DLLS, _write_ort_manifests
from flitzis_looper.analysis.native_provisioning import (
    _require_windows_x64,
    _visual_cpp_runtime,
    provision_ort_runtime,
)
from flitzis_looper.analysis.process import offline_work_scope, run_owned_command
from flitzis_looper.bs_onnx_cuda_cache import ensure_bs_cuda_cache
from flitzis_looper.bs_onnx_installation import (
    BS_ONNX_EXECUTABLE,
    BS_ONNX_GRAPH,
    BS_ONNX_WEIGHTS,
    BS_ONNX_WEIGHTS_SHA256,
    BsOnnxInstallation,
    bs_onnx_worker_cwd,
    load_bs_onnx_installation,
    verify_bs_onnx_installation,
)
from flitzis_looper.bs_onnx_mkl import provision_bs_mkl, verify_bs_mkl
from flitzis_looper.controller.bs_roformer_assets import CHECKPOINT_FILENAME, CONFIG_FILENAME
from flitzis_looper.offline_runtime import child_environment
from flitzis_looper.stem_cancellation import CANCEL_PATH_ENV
from flitzis_looper.system_resources import OfflineMemoryMonitor, stem_memory_snapshot

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.analysis.beat_cuda_cache import CudaDeviceIdentity
    from flitzis_looper.bs_onnx_cuda_cache import BsCudaCacheStatus


BS_ONNX_AMP_GRAPH = "bs-mask-amp.onnx"
BS_ONNX_GRAPH_SHA256 = "1bbb2bfd27e4cb71b6fce2c1cb455afca44b98335ab8de38540e0f750d4a15c9"
BS_ONNX_AMP_GRAPH_SHA256 = "19a9be3bc2e836d06918c7d9877f4def4f8822de3fb69134b0695ebd7afef3d5"
_EXECUTABLE_BYTES = 64 * 1024 * 1024
_GRAPH_BYTES = 4 * 1024 * 1024
_CRT_BYTES = 8 * 1024 * 1024
_TARGET = "x86_64-pc-windows-msvc"
_INSTALLATION_NAME = re.compile(r"installation-[0-9a-f]{64}(?:-[0-9a-f]{12})?")


def build_bs_onnx_worker(repository: Path, scratch: Path, *, cargo: str | Path = "cargo") -> Path:
    """Build only the standalone worker from the locked repository."""
    _require_windows_x64()
    target = scratch / "native-bs-roformer-build"
    subprocess.run(
        [
            cargo,
            "rustc",
            "--locked",
            "--release",
            "--target",
            _TARGET,
            "--target-dir",
            target,
            "--manifest-path",
            repository / "rust/Cargo.toml",
            "-p",
            "flitzis-looper-analysis",
            "--features",
            "native-bs-roformer-worker",
            "--bin",
            "native-bs-roformer-worker",
            "--",
            "-C",
            "target-feature=+crt-static",
        ],
        cwd=repository,
        check=True,
    )
    return target / _TARGET / "release" / BS_ONNX_EXECUTABLE


def _ordinary(directory: Path) -> None:
    if (
        not directory.is_absolute()
        or ".." in directory.parts
        or any(part.is_symlink() or part.is_junction() for part in (directory, *directory.parents))
    ):
        msg = "BS-RoFormer installations require an ordinary absolute directory"
        raise ValueError(msg)


def _graph_pins(*, cuda: bool) -> tuple[tuple[str, str], ...]:
    graph: tuple[tuple[str, str], ...] = ((BS_ONNX_GRAPH, BS_ONNX_GRAPH_SHA256),)
    if cuda:
        graph += ((BS_ONNX_AMP_GRAPH, BS_ONNX_AMP_GRAPH_SHA256),)
    return graph


def _path(root: Path, executable_sha: str, *, cuda: bool) -> Path:
    ort = (NATIVE_CPU_PACKAGE_SHA, NATIVE_CUDA_PACKAGE_SHA if cuda else None)
    fft_recipe = "mkl-2026.0.0-20260401-intel-lp64-with-license"
    identity = hashlib.sha256(
        f"{executable_sha}:{_graph_pins(cuda=cuda)}:{ort}:{fft_recipe}".encode()
    ).hexdigest()
    return root / f"installation-{identity}"


def _selected(directory: Path, executable_sha: str, *, cuda: bool) -> BsOnnxInstallation:
    _ordinary(directory)
    installation = load_bs_onnx_installation(directory / "manifest.json")
    if (
        installation.executable_sha256 != executable_sha
        or installation.graph_sha256 != BS_ONNX_GRAPH_SHA256
        or installation.cuda_graph_sha256 != (BS_ONNX_AMP_GRAPH_SHA256 if cuda else None)
    ):
        msg = "BS-RoFormer installation differs from the selected worker and model"
        raise ValueError(msg)
    verify_bs_onnx_installation(installation)
    verify_bs_mkl(directory)
    return installation


def _load_staged_cpu(installation: BsOnnxInstallation, directory: Path) -> None:
    with TemporaryDirectory(prefix="load-", dir=directory) as name:
        scratch = Path(name)
        marker = scratch / "cancel"
        package, digest = installation.package(cuda=False)
        env = child_environment({
            "SystemRoot": os.environ["SYSTEMROOT"],
            "TEMP": str(scratch),
            "TMP": str(scratch),
            "CUDA_VISIBLE_DEVICES": "-1",
            "FLITZIS_BEAT_THIS_CRT": str(installation.root / "crt"),
            "FLITZIS_TRIAL_ORT_LOAD_POLICY": "package-system32",
            "FLITZIS_TRIAL_ORT_PACKAGE": str(package),
            "FLITZIS_TRIAL_ORT_PACKAGE_SHA256": digest,
            CANCEL_PATH_ENV: str(marker),
        })
        retired = Event()
        cancel = Event()
        monitor = OfflineMemoryMonitor(stem_memory_snapshot, monotonic)

        def stop() -> str | None:
            return monitor.observe().refusal_reason

        with offline_work_scope(cancel):
            result = run_owned_command(
                [str(installation.executable), "load", str(scratch), installation.graph_sha256],
                cwd=bs_onnx_worker_cwd(env["SystemRoot"]),
                env=env,
                retired=retired,
                cancel=cancel,
                strict_completion=True,
                stop_reason=stop,
                before_resume=stop,
            )
        if result.reason != "ok" or result.returncode != 0 or not retired.is_set():
            detail = result.stderr.decode(errors="replace").strip()[-1500:]
            msg = f"BS-RoFormer CPU package load failed ({result.reason}): {detail}"
            raise RuntimeError(msg)
        receipt = json.loads(result.stdout)
        expected = {
            "schema_version": 1,
            "mode": "load",
            "provider": "CPUExecutionProvider",
            "ort_api": 24,
            "ort_version": "1.24.4",
            "fft_profile": "onemkl-2026.0-20260401-intel-lp64-stft",
            "ort_package_sha256": NATIVE_CPU_PACKAGE_SHA,
            "graph_sha256": installation.graph_sha256,
            "weights_sha256": BS_ONNX_WEIGHTS_SHA256,
        }
        if (
            type(receipt) is not dict
            or receipt.get("inference_executed") is not False
            or any(receipt.get(name) != value for name, value in expected.items())
        ):
            msg = "BS-RoFormer CPU load receipt differs from the staged package"
            raise RuntimeError(msg)


def _copy_model_graphs(repository: Path, stage: Path, *, cuda: bool) -> None:
    for filename, digest in _graph_pins(cuda=cuda):
        path = stage / filename
        shutil.copyfile(repository / "workers/bs_roformer" / filename, path)
        if file_digest(path, max_bytes=_GRAPH_BYTES) != digest:
            msg = f"BS-RoFormer graph differs from its qualification pin: {filename}"
            raise ValueError(msg)


def _stage_model_weights(repository: Path, assets: Path, stage: Path) -> None:
    subprocess.run(
        [
            sys.executable,
            "-B",
            str(repository / "scripts/bs_learned_weights.py"),
            str(assets / CHECKPOINT_FILENAME),
            str(assets / CONFIG_FILENAME),
            str(stage / BS_ONNX_WEIGHTS),
        ],
        cwd=repository,
        check=True,
    )


def _populate_stage(
    repository: Path,
    stage: Path,
    assets: Path,
    executable: Path,
    cache: Path,
    crt_dir: Path,
    *,
    cuda: bool,
) -> BsOnnxInstallation:
    (stage / "ort").mkdir()
    (stage / "crt").mkdir()
    shutil.copyfile(executable, stage / BS_ONNX_EXECUTABLE)
    worker_sha = file_digest(stage / BS_ONNX_EXECUTABLE, max_bytes=_EXECUTABLE_BYTES)
    _copy_model_graphs(repository, stage, cuda=cuda)
    _stage_model_weights(repository, assets, stage)
    provision_ort_runtime(repository, stage, cache, offline=False, include_cuda=cuda)
    _write_ort_manifests(stage)
    provision_bs_mkl(stage, cache)
    crt_files = {}
    for name in CRT_DLLS:
        dll = stage / "crt" / name
        shutil.copyfile(crt_dir / name, dll)
        crt_files[name] = [dll.stat().st_size, file_digest(dll, max_bytes=_CRT_BYTES)]
    manifest = stage / "manifest.json"
    manifest.write_text(
        json.dumps({
            "schema_version": 1,
            "executable_sha256": worker_sha,
            "graph_sha256": BS_ONNX_GRAPH_SHA256,
            "cuda_graph_sha256": BS_ONNX_AMP_GRAPH_SHA256 if cuda else None,
            "crt_files": crt_files,
        }),
        encoding="utf-8",
    )
    return load_bs_onnx_installation(manifest)


def _prepare_cuda(
    installation: BsOnnxInstallation, device: CudaDeviceIdentity | None
) -> BsCudaCacheStatus:
    status = ensure_bs_cuda_cache(installation, device, retry_failed=True)
    if status.status not in {"ready", "unavailable"}:
        raise RuntimeError(status.detail)
    return status


def _keyed_installation(base: Path) -> Path:
    pointer = base.with_suffix(".json")
    if pointer.is_symlink() or pointer.is_junction():
        msg = "BS-RoFormer repair selection must be an ordinary file"
        raise ValueError(msg)
    try:
        with pointer.open("r", encoding="ascii") as stream:
            leaf = stream.read(128).strip()
    except FileNotFoundError:
        return base
    if _INSTALLATION_NAME.fullmatch(leaf) is None or not leaf.startswith(base.name + "-"):
        msg = "Invalid BS-RoFormer repair selection"
        raise ValueError(msg)
    return base.with_name(leaf)


def _write_repair_pointer(base: Path, leaf: str) -> None:
    pointer = base.with_suffix(".json")
    temporary = base.with_name(f".repair-{uuid4().hex}.json")
    try:
        with temporary.open("x", encoding="ascii") as stream:
            stream.write(leaf + "\n")
        temporary.replace(pointer)
    finally:
        temporary.unlink(missing_ok=True)


def _existing(base: Path, worker_sha: str, *, cuda: bool) -> BsOnnxInstallation | None:
    directory = _keyed_installation(base)
    _ordinary(directory)
    if not directory.exists():
        return None
    try:
        return _selected(directory, worker_sha, cuda=cuda)
    except FileNotFoundError, ValueError:
        # A damaged immutable package keeps its storage and any existing readers.
        # Build the repaired generation separately and select it only after verification.
        return None


def _retain_bs_installation(
    repository: Path,
    base: Path,
    executable: Path,
    assets: Path,
    cache: Path,
    crt_dir: Path | None,
    device: CudaDeviceIdentity | None,
) -> tuple[BsOnnxInstallation, BsCudaCacheStatus]:
    cuda = device is not None
    worker_sha = file_digest(executable, max_bytes=_EXECUTABLE_BYTES)
    root = base.parent
    root.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    _ordinary(base.with_suffix(".lock"))
    with _download_lock(base):
        if (existing := _existing(base, worker_sha, cuda=cuda)) is not None:
            return existing, _prepare_cuda(existing, device)
        repairing = base.exists() or base.with_suffix(".json").exists()
        destination = base.with_name(base.name + "-" + uuid4().hex[:12]) if repairing else base
        with TemporaryDirectory(prefix=".staging-", dir=root) as name:
            temporary = Path(name)
            stage = temporary / "package"
            stage.mkdir()
            candidate = _populate_stage(
                repository,
                stage,
                assets,
                executable,
                cache,
                crt_dir or _visual_cpp_runtime(),
                cuda=cuda,
            )
            _load_staged_cpu(candidate, temporary)
            stage.rename(destination)
        installed = _selected(destination, worker_sha, cuda=cuda)
        status = _prepare_cuda(installed, device)
        if repairing:
            _write_repair_pointer(base, destination.name)
        return installed, status


def stage_bs_onnx_installation(
    repository: Path,
    root: Path,
    executable: Path,
    assets: Path,
    cache: Path,
    *,
    crt_dir: Path | None = None,
    cuda_preparation: Callable[[BsCudaCacheStatus], None] | None = None,
) -> BsOnnxInstallation:
    """Derive data locally, verify a CPU load and retain an immutable installation."""
    _require_windows_x64()
    _ordinary(root)
    _ordinary(cache)
    device = cuda_device_identity()
    worker_sha = file_digest(executable, max_bytes=_EXECUTABLE_BYTES)
    base = _path(root, worker_sha, cuda=device is not None)
    installed, status = _retain_bs_installation(
        repository, base, executable, assets, cache, crt_dir, device
    )
    _publish_selection(root, _portable_pointer(root, worker_sha), installed)
    if cuda_preparation is not None:
        cuda_preparation(status)
    return installed


def _portable_pointer(root: Path, executable_sha256: str) -> Path:
    return root / f"worker-{executable_sha256}.json"


def verify_bs_onnx_for_executable(root: Path, executable: Path) -> BsOnnxInstallation:
    """Verify the last successfully prepared package for this exact Release worker."""
    _ordinary(root)
    digest = file_digest(executable, max_bytes=_EXECUTABLE_BYTES)
    installation = _read_selection(root, _portable_pointer(root, digest))
    return _selected(installation.root, digest, cuda=installation.cuda)


def _read_selection(root: Path, pointer: Path) -> BsOnnxInstallation:
    _ordinary(root)
    if pointer.is_symlink() or pointer.is_junction():
        msg = "BS-RoFormer worker pointer must be an ordinary file"
        raise ValueError(msg)
    with pointer.open("rb") as stream:
        data = stream.read(4097)
    if len(data) > 4096:
        msg = "BS-RoFormer worker pointer exceeds its metadata limit"
        raise ValueError(msg)
    value = json.loads(data)
    if type(value) is not dict or set(value) != {"schema_version", "directory", "manifest_sha256"}:
        msg = "Invalid BS-RoFormer worker pointer fields"
        raise ValueError(msg)
    leaf = value["directory"]
    if (
        type(value["schema_version"]) is not int
        or value["schema_version"] != 1
        or type(leaf) is not str
        or _INSTALLATION_NAME.fullmatch(leaf) is None
    ):
        msg = "Invalid BS-RoFormer worker pointer selection"
        raise ValueError(msg)
    directory = root / leaf
    _ordinary(directory)
    manifest = directory / "manifest.json"
    installation = load_bs_onnx_installation(manifest)
    if file_digest(manifest, max_bytes=8192) != value["manifest_sha256"]:
        msg = "BS-RoFormer selected manifest differs from its pointer"
        raise ValueError(msg)
    return installation


def selected_bs_onnx_installation(root: Path, *, verify: bool = False) -> BsOnnxInstallation:
    """Resolve the retained Source installation pointer without starting a model."""
    installation = _read_selection(root, root / "current.json")
    if verify:
        return _selected(installation.root, installation.executable_sha256, cuda=installation.cuda)
    return installation


def _publish_selection(root: Path, pointer: Path, installation: BsOnnxInstallation) -> None:
    _ordinary(root)
    if (
        installation.root.parent != root
        or _INSTALLATION_NAME.fullmatch(installation.root.name) is None
    ):
        msg = "BS-RoFormer worker must be retained before it can be selected"
        raise ValueError(msg)
    payload = {
        "schema_version": 1,
        "directory": installation.root.name,
        "manifest_sha256": file_digest(installation.manifest, max_bytes=8192),
    }
    temporary = root / f".current-{uuid4().hex}.json"
    try:
        with temporary.open("x", encoding="utf-8") as stream:
            json.dump(payload, stream, indent=2)
        _ordinary(pointer)
        temporary.replace(pointer)
    finally:
        temporary.unlink(missing_ok=True)


def select_bs_onnx_installation(root: Path, installation: BsOnnxInstallation) -> None:
    """Atomically publish a fully prepared Source installation."""
    _publish_selection(root, root / "current.json", installation)
