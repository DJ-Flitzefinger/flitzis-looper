"""Stage and verify the Windows HTDemucs model and its private runtime."""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Event
from time import monotonic
from uuid import uuid4

from flitzis_looper.analysis.acquisition import _download_lock
from flitzis_looper.analysis.artifacts import file_digest
from flitzis_looper.analysis.execution import NATIVE_CPU_PACKAGE_SHA
from flitzis_looper.analysis.native_beat_worker import (
    _ASSETS as _BEAT_ASSETS,
)
from flitzis_looper.analysis.native_beat_worker import (
    CRT_DLLS,
    _cuda_assets,
    _plain_file,
)
from flitzis_looper.analysis.process import offline_work_scope, run_owned_command
from flitzis_looper.offline_runtime import child_environment
from flitzis_looper.stem_cancellation import CANCEL_PATH_ENV
from flitzis_looper.system_resources import OfflineMemoryMonitor, stem_memory_snapshot
from flitzis_looper_audio import SealedProcessingFiles

NATIVE_DEMUCS_EXECUTABLE = "native-htdemucs-worker.exe"
HTDEMUCS_RUNTIME_EXECUTABLE = f"runtime/htdemucs/{NATIVE_DEMUCS_EXECUTABLE}"
_CPU_ASSETS = Path("workers/htdemucs/native_cpu")
_CUDA_ASSETS = Path("workers/htdemucs/native_cuda")
_INSTALLATIONS = "native-htdemucs"
_CPU_ORT = {key: pin for key, pin in _BEAT_ASSETS.items() if key.startswith("ort/")}
_TARGET = "x86_64-pc-windows-msvc"
_GRAPH_BYTES = 192 * 1024 * 1024
_WORKER_BYTES = 64 * 1024 * 1024
_CRT_BYTES = 8 * 1024 * 1024
_METADATA_BYTES = 4096
_DIGEST = re.compile(r"[0-9a-f]{64}")


@dataclass(frozen=True, slots=True)
class _Graph:
    assets: Path
    name: str
    sha256: str
    extent: int
    trace_sha256: str
    approval_sha256: str
    template_sha256: str
    recipe_sha256: str


@dataclass(frozen=True, slots=True)
class _Release:
    graph: _Graph
    worker_sha256: str
    worker_bytes: int
    cuda_graph: _Graph

    @property
    def graphs(self) -> tuple[_Graph, _Graph]:
        return self.graph, self.cuda_graph


@dataclass(frozen=True, slots=True)
class HtdemucsInstallation:
    directory: Path
    worker_sha256: str
    worker_bytes: int


def _ordinary(path: Path) -> None:
    if (
        not path.is_absolute()
        or ".." in path.parts
        or any(part.is_symlink() or part.is_junction() for part in (path, *path.parents))
    ):
        msg = "HTDemucs paths must be ordinary absolute paths"
        raise ValueError(msg)


def _read_metadata(path: Path) -> tuple[bytes, object]:
    _plain_file(path, _METADATA_BYTES)
    with path.open("rb") as stream:
        raw = stream.read(_METADATA_BYTES + 1)
    if len(raw) > _METADATA_BYTES:
        msg = "HTDemucs metadata exceeds its extent"
        raise ValueError(msg)
    return raw, json.loads(raw)


def _digest(value: object) -> str:
    if not isinstance(value, str) or not _DIGEST.fullmatch(value):
        msg = "Invalid HTDemucs SHA-256 pin"
        raise ValueError(msg)
    return value


def _extent(value: object, maximum: int) -> int:
    if type(value) is not int or not 0 < value <= maximum:
        msg = "Invalid HTDemucs file extent"
        raise ValueError(msg)
    return value


def htdemucs_onnx_released(repository: Path) -> bool:
    """Distinguish the checkout's pending HTDemucs route from an approved release."""
    assets = repository / _CPU_ASSETS
    _ordinary(assets)
    try:
        _, graph = _read_metadata(assets / "release.json")
        _, worker = _read_metadata(assets / "worker.json")
    except FileNotFoundError:
        if not assets.exists():
            return False
        raise
    cuda = repository / _CUDA_ASSETS / "release.json"
    _ordinary(cuda)
    cuda_graph = _read_metadata(cuda)[1] if cuda.is_file() else None
    if graph is None and worker is None and cuda_graph is None:
        return False
    if not all(isinstance(pin, dict) for pin in (graph, worker, cuda_graph)):
        msg = "HTDemucs CPU, CUDA and worker approvals must be complete"
        raise ValueError(msg)
    return True


def _graph(assets: Path, name: str) -> _Graph:
    _ordinary(assets)
    raw, graph = _read_metadata(assets / "release.json")
    if type(graph) is not dict or set(graph) != {
        "graph_bytes",
        "graph_sha256",
        "trace_sha256",
        "attention_sha256",
    }:
        msg = "HTDemucs graph approval is unavailable"
        raise ValueError(msg)
    _, hydration = _read_metadata(assets / "hydration.json")
    if type(hydration) is not dict or set(hydration) != {"template_sha256", "recipe_sha256"}:
        msg = "HTDemucs hydration recipe approval is unavailable"
        raise ValueError(msg)
    _digest(graph["attention_sha256"])
    return _Graph(
        assets,
        name,
        _digest(graph["graph_sha256"]),
        _extent(graph["graph_bytes"], _GRAPH_BYTES),
        _digest(graph["trace_sha256"]),
        hashlib.sha256(raw).hexdigest(),
        _digest(hydration["template_sha256"]),
        _digest(hydration["recipe_sha256"]),
    )


def _release(repository: Path) -> _Release:
    cpu = _graph(repository / _CPU_ASSETS, "htdemucs.onnx")
    _, worker = _read_metadata(cpu.assets / "worker.json")
    if type(worker) is not dict or set(worker) != {"bytes", "sha256"}:
        msg = "HTDemucs worker approval is unavailable"
        raise ValueError(msg)
    alternate = _graph(repository / _CUDA_ASSETS, "htdemucs-cuda.onnx")
    return _Release(
        cpu, _digest(worker["sha256"]), _extent(worker["bytes"], _WORKER_BYTES), alternate
    )


def _key(graph_sha256: str, worker_sha256: str) -> str:
    return hashlib.sha256(f"{_digest(graph_sha256)}:{_digest(worker_sha256)}".encode()).hexdigest()


def htdemucs_crt_witnesses(directory: Path) -> tuple[tuple[str, int, str], ...]:
    """Return the CRT bytes verified before this installation was selected."""
    inventory = directory / "crt" / "installed.json"
    _, record = _read_metadata(inventory)
    if not isinstance(record, dict) or set(record) != set(CRT_DLLS):
        msg = "Unexpected HTDemucs CRT receipt"
        raise ValueError(msg)
    result = []
    for name in CRT_DLLS:
        value = record[name]
        if not isinstance(value, list) or len(value) != 2:
            msg = f"Invalid HTDemucs CRT receipt for {name}"
            raise ValueError(msg)
        extent, digest = value
        result.append((name, _extent(extent, _CRT_BYTES), _digest(digest)))
    return tuple(result)


def _read_selection(
    checkpoint_cache: Path,
    graph_sha256: str,
    reference_worker_sha256: str,
    portable_worker_sha256: str | None = None,
    portable_crt_sha256: str | None = None,
) -> tuple[Path, dict[str, object]]:
    root = checkpoint_cache / _INSTALLATIONS
    key = _key(graph_sha256, reference_worker_sha256)
    _ordinary(root)
    pointer = _selection_path(root, key, portable_worker_sha256, portable_crt_sha256)
    _, selection = _read_metadata(pointer)
    if not isinstance(selection, dict) or set(selection) not in (
        {"directory", "crt_sha256"},
        {"directory", "crt_sha256", "worker_sha256", "worker_bytes", "cuda_graph_sha256"},
    ):
        msg = "Invalid HTDemucs installation record"
        raise ValueError(msg)
    value = selection["directory"]
    if not isinstance(value, str) or not re.fullmatch(rf"{key}-[0-9a-f]{{12}}", value):
        msg = "Invalid HTDemucs installation selection"
        raise ValueError(msg)
    selected = root / value
    _ordinary(selected)
    if not selected.is_dir():
        msg = "Selected HTDemucs installation is missing"
        raise FileNotFoundError(msg)
    raw, _ = _read_metadata(selected / "crt" / "installed.json")
    crt_sha = hashlib.sha256(raw).hexdigest()
    if crt_sha != _digest(selection["crt_sha256"]) or portable_crt_sha256 not in {None, crt_sha}:
        msg = "HTDemucs installed CRT receipt differs from its selection"
        raise ValueError(msg)
    return selected, selection


def selected_htdemucs_onnx(checkpoint_cache: Path, graph_sha256: str, worker_sha256: str) -> Path:
    """Resolve the immutable directory for a released graph and worker build recipe."""
    return _read_selection(checkpoint_cache, graph_sha256, worker_sha256)[0]


def selected_htdemucs_installation(
    checkpoint_cache: Path,
    graph_sha256: str,
    reference_worker_sha256: str,
    reference_worker_bytes: int,
    *,
    cuda_graph_sha256: str,
    worker_executable: Path | None = None,
    crt_dir: Path | None = None,
) -> HtdemucsInstallation:
    """Resolve the actual installer-admitted worker for both released graphs."""
    packaged = None if worker_executable is None else _worker_identity(worker_executable)
    portable_crt = None if crt_dir is None else hashlib.sha256(_crt_metadata(crt_dir)).hexdigest()
    try:
        directory, selection = _read_selection(
            checkpoint_cache,
            graph_sha256,
            reference_worker_sha256,
            None if packaged is None else packaged[0],
            portable_crt,
        )
    except FileNotFoundError:
        if packaged != (reference_worker_sha256, reference_worker_bytes):
            raise
        directory, selection = _read_selection(
            checkpoint_cache, graph_sha256, reference_worker_sha256
        )
        if portable_crt is not None and _digest(selection["crt_sha256"]) != portable_crt:
            msg = "HTDemucs installed CRT differs from this packaged Release build"
            raise ValueError(msg) from None
    if "worker_sha256" not in selection:
        identity = (reference_worker_sha256, reference_worker_bytes)
    else:
        if _digest(selection["cuda_graph_sha256"]) != cuda_graph_sha256:
            msg = "HTDemucs selected installation differs from the CUDA graph approval"
            raise ValueError(msg)
        identity = (
            _digest(selection["worker_sha256"]),
            _extent(selection["worker_bytes"], _WORKER_BYTES),
        )
    if packaged is None and identity != (reference_worker_sha256, reference_worker_bytes):
        msg = "Source HTDemucs requires the reviewed reference worker"
        raise ValueError(msg)
    if packaged is not None and packaged != identity:
        msg = "HTDemucs installed worker differs from this packaged Release build"
        raise ValueError(msg)
    return HtdemucsInstallation(directory, *identity)


def _verify_file(path: Path, extent: int, sha256: str) -> None:
    if _plain_file(path, extent) != extent or file_digest(path, max_bytes=extent) != sha256:
        msg = f"HTDemucs asset differs from its pin: {path.name}"
        raise ValueError(msg)


def _verify_release_worker(executable: Path, release: _Release) -> None:
    _ordinary(executable)
    _verify_file(executable, release.worker_bytes, release.worker_sha256)


def _worker_identity(executable: Path) -> tuple[str, int]:
    _ordinary(executable)
    if executable.name != NATIVE_DEMUCS_EXECUTABLE:
        msg = "HTDemucs executable must be the trusted checkout build output"
        raise ValueError(msg)
    extent = _plain_file(executable, _WORKER_BYTES)
    return file_digest(executable, max_bytes=extent), extent


def verify_htdemucs_checkpoint(checkpoint_cache: Path) -> None:
    """Verify the complete upstream asset before local ONNX derivation."""
    from flitzis_looper.controller.model_pins import DEMUCS_ASSETS  # noqa: PLC0415

    (name,) = DEMUCS_ASSETS
    extent, checksum, _url = DEMUCS_ASSETS[name]
    checkpoint = checkpoint_cache / name
    _ordinary(checkpoint)
    _verify_file(checkpoint, extent, checksum)


def _verify_package(installation: HtdemucsInstallation, release: _Release, crt: Path) -> None:
    directory = installation.directory
    _verify_file(
        directory / NATIVE_DEMUCS_EXECUTABLE,
        installation.worker_bytes,
        installation.worker_sha256,
    )
    for graph in release.graphs:
        _verify_file(directory / graph.name, graph.extent, graph.sha256)
    if {path.name for path in directory.iterdir()} != {
        NATIVE_DEMUCS_EXECUTABLE,
        "ort",
        "cuda",
        "crt",
        *(graph.name for graph in release.graphs),
    }:
        msg = "Unexpected file in the HTDemucs installation"
        raise ValueError(msg)
    for name in ("ort", "cuda", "crt"):
        _ordinary(directory / name)
    runtime = _CPU_ORT | _cuda_assets(directory)
    for folder in ("ort", "cuda"):
        names = {Path(name).name for name in runtime if name.startswith(f"{folder}/")}
        if {path.name for path in (directory / folder).iterdir()} != names:
            msg = f"Unexpected HTDemucs runtime entries in {folder}"
            raise ValueError(msg)
    for name, (size, checksum) in runtime.items():
        _verify_file(directory / name, size, checksum)
    if {path.name for path in (directory / "crt").iterdir()} != {*CRT_DLLS, "installed.json"}:
        msg = "Unexpected HTDemucs CRT entries"
        raise ValueError(msg)
    for name, size, digest in htdemucs_crt_witnesses(directory):
        source = crt / name
        _ordinary(source)
        _verify_file(source, size, digest)
        _verify_file(directory / "crt" / name, size, digest)


def _receipt_runtime(receipt: dict[str, object]) -> bool:
    runtime = receipt.get("runtime")
    if not isinstance(runtime, dict):
        return False
    declared, loaded = runtime.get("declared_package"), runtime.get("loaded_package")
    return (
        runtime.get("ort_api") == 24
        and runtime.get("runtime_version") == "1.24.4"
        and runtime.get("manifest_sha256") == _CPU_ORT["ort/step23-runtime.json"][1]
        and isinstance(declared, dict)
        and declared.get("manifest_sha256") == NATIVE_CPU_PACKAGE_SHA
        and isinstance(loaded, dict)
        and loaded.get("policy") == "package-system32"
    )


def _load_cpu(directory: Path, scratch: Path, release: _Release) -> None:
    env = child_environment({
        "SystemRoot": os.environ["SYSTEMROOT"],
        "TEMP": str(scratch),
        "TMP": str(scratch),
        "CUDA_VISIBLE_DEVICES": "-1",
        "FLITZIS_BEAT_THIS_CRT": str(directory / "crt"),
        "FLITZIS_TRIAL_ORT_LOAD_POLICY": "package-system32",
        "FLITZIS_TRIAL_ORT_PACKAGE": str(directory / "ort/step23-package.json"),
        "FLITZIS_TRIAL_ORT_PACKAGE_SHA256": NATIVE_CPU_PACKAGE_SHA,
        CANCEL_PATH_ENV: str(scratch / "cancel"),
    })
    cancel, retired = Event(), Event()
    monitor = OfflineMemoryMonitor(stem_memory_snapshot, monotonic)

    def stop() -> str | None:
        return monitor.observe().refusal_reason

    with offline_work_scope(cancel):
        result = run_owned_command(
            [str(directory / NATIVE_DEMUCS_EXECUTABLE), "load", str(scratch)],
            cwd=Path(env["SystemRoot"]),
            env=env,
            retired=retired,
            cancel=cancel,
            strict_completion=True,
            stop_reason=stop,
            before_resume=stop,
        )
    if result.reason != "ok" or result.returncode != 0 or not retired.is_set():
        detail = result.stderr.decode(errors="replace").strip()[-1500:]
        msg = f"HTDemucs CPU package load failed ({result.reason}): {detail}"
        raise RuntimeError(msg)
    receipt = json.loads(result.stdout)
    expected = {
        "schema_version": 3,
        "mode": "load",
        "device": "cpu",
        "job": "windows-owned",
        "graph_sha256": release.graph.sha256,
        "trace_sha256": release.graph.trace_sha256,
        "approval_sha256": release.graph.approval_sha256,
    }
    if (
        not isinstance(receipt, dict)
        or set(receipt) != {*expected, "runtime"}
        or any(receipt.get(key) != value for key, value in expected.items())
        or not _receipt_runtime(receipt)
    ):
        msg = "HTDemucs CPU load receipt differs from the staged package"
        raise ValueError(msg)


def build_htdemucs_onnx_worker(
    repository: Path, scratch: Path, *, cargo: str | Path = "cargo"
) -> Path:
    """Compile the approved standalone worker from the locked checkout."""
    from flitzis_looper.analysis.native_provisioning import _require_windows_x64  # noqa: PLC0415

    _require_windows_x64()
    binary = Path(NATIVE_DEMUCS_EXECUTABLE).stem
    target = scratch / "native-htdemucs-build"
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
            "native-demucs-worker",
            "--bin",
            binary,
            "--",
            "-C",
            "target-feature=+crt-static",
            "-C",
            "link-arg=/Brepro",
        ],
        cwd=repository,
        check=True,
    )
    return target / _TARGET / "release" / NATIVE_DEMUCS_EXECUTABLE


def _hydrate(repository: Path, checkpoint_cache: Path, stage: Path, graph: _Graph) -> None:
    from flitzis_looper.controller.model_pins import DEMUCS_ASSETS  # noqa: PLC0415

    (checkpoint,) = DEMUCS_ASSETS
    source = checkpoint_cache / checkpoint
    template = graph.assets / "graph.template"
    recipe = graph.assets / "graph.recipe.json"
    for path in (source, template, recipe):
        _ordinary(path)
    subprocess.run(
        [
            sys.executable,
            "-I",
            "-S",
            "-B",
            repository / "scripts/htdemucs_learned_weights.py",
            source,
            template,
            recipe,
            stage / graph.name,
            "--template-sha256",
            graph.template_sha256,
            "--recipe-sha256",
            graph.recipe_sha256,
            "--graph-sha256",
            graph.sha256,
        ],
        cwd=repository,
        check=True,
    )


def _installed(
    cache: Path,
    release: _Release,
    crt: Path | None,
    worker_executable: Path | None = None,
    *,
    verify_assets: bool = True,
) -> HtdemucsInstallation:
    installation = selected_htdemucs_installation(
        cache,
        release.graph.sha256,
        release.worker_sha256,
        release.worker_bytes,
        cuda_graph_sha256=release.cuda_graph.sha256,
        worker_executable=worker_executable,
        crt_dir=crt if worker_executable is not None else None,
    )
    if verify_assets:
        assert crt is not None
        _verify_package(installation, release, crt)
    else:
        _verify_file(
            installation.directory / NATIVE_DEMUCS_EXECUTABLE,
            installation.worker_bytes,
            installation.worker_sha256,
        )
    return installation


def verify_htdemucs_release_worker(
    repository: Path, executable: Path, *, source_built: bool = False
) -> None:
    """Check a reference binary or an installer-owned checkout build before staging."""
    release = _release(repository)
    if source_built:
        _worker_identity(executable)
    else:
        _verify_release_worker(executable, release)


def _publish(
    pointer: Path, installation: HtdemucsInstallation, temporary: Path, release: _Release
) -> None:
    selection = temporary / "selection.json"
    raw, _ = _read_metadata(installation.directory / "crt" / "installed.json")
    record: dict[str, object] = {
        "directory": installation.directory.name,
        "crt_sha256": hashlib.sha256(raw).hexdigest(),
    }
    if (installation.worker_sha256, installation.worker_bytes) != (
        release.worker_sha256,
        release.worker_bytes,
    ):
        record.update(
            worker_sha256=installation.worker_sha256,
            worker_bytes=installation.worker_bytes,
            cuda_graph_sha256=release.cuda_graph.sha256,
        )
    with selection.open("x", encoding="ascii") as stream:
        json.dump(record, stream)
        stream.flush()
        os.fsync(stream.fileno())
    _ordinary(pointer)
    selection.replace(pointer)


def _selection_path(
    root: Path,
    key: str,
    portable_worker_sha256: str | None,
    portable_crt_sha256: str | None,
) -> Path:
    if portable_worker_sha256 is None:
        return root / f"{key}.json"
    identity = _key(key, portable_worker_sha256)
    if portable_crt_sha256 is not None:
        identity = _key(identity, portable_crt_sha256)
    return root / f"portable-{identity}.json"


def _crt_metadata(crt_dir: Path) -> bytes:
    files = {}
    for name in CRT_DLLS:
        source = crt_dir / name
        _ordinary(source)
        extent = _plain_file(source, _CRT_BYTES)
        files[name] = [extent, file_digest(source, max_bytes=extent)]
    return json.dumps(files, sort_keys=True).encode("ascii")


def _build_package(
    repository: Path,
    checkpoint_cache: Path,
    runtime_cache: Path,
    executable: Path,
    release: _Release,
    installation: HtdemucsInstallation,
    crt: Path,
    *,
    offline: bool,
) -> None:
    from flitzis_looper.analysis.native_beat_worker import _write_ort_manifests  # noqa: PLC0415
    from flitzis_looper.analysis.native_provisioning import provision_ort_runtime  # noqa: PLC0415

    stage = installation.directory
    stage.mkdir()
    (stage / "ort").mkdir()
    (stage / "crt").mkdir()
    shutil.copyfile(executable, stage / NATIVE_DEMUCS_EXECUTABLE)
    for graph in release.graphs:
        _hydrate(repository, checkpoint_cache, stage, graph)
    provision_ort_runtime(repository, stage, runtime_cache, offline=offline, include_cuda=True)
    _write_ort_manifests(stage)
    for name in CRT_DLLS:
        shutil.copyfile(crt / name, stage / "crt" / name)
    with (stage / "crt" / "installed.json").open("xb") as stream:
        stream.write(_crt_metadata(crt))
    _verify_package(installation, release, crt)


def _load_verified(installation: HtdemucsInstallation, scratch: Path, release: _Release) -> None:
    worker = installation.directory / NATIVE_DEMUCS_EXECUTABLE
    guard = SealedProcessingFiles([str(worker)])
    try:
        if guard.witnesses()[0][1:] != (installation.worker_sha256, installation.worker_bytes):
            msg = "HTDemucs package lost custody of its installer-built worker"
            raise ValueError(msg)
        _load_cpu(installation.directory, scratch, release)
        guard.verify()
    finally:
        del guard


def stage_htdemucs_onnx_installation(
    repository: Path,
    checkpoint_cache: Path,
    executable: Path,
    runtime_cache: Path,
    *,
    crt_dir: Path | None = None,
    offline: bool = False,
    portable: bool = False,
) -> Path:
    """Derive, verify and select a package for an installer-owned checkout build."""
    from flitzis_looper.analysis.native_provisioning import (  # noqa: PLC0415
        _require_windows_x64,
        _visual_cpp_runtime,
    )

    _require_windows_x64()
    release = _release(repository)
    if not portable:
        _verify_release_worker(executable, release)
    worker_sha256, worker_bytes = (
        _worker_identity(executable) if portable else (release.worker_sha256, release.worker_bytes)
    )
    crt = crt_dir if crt_dir is not None else _visual_cpp_runtime()
    root = checkpoint_cache / _INSTALLATIONS
    _ordinary(root)
    _ordinary(runtime_cache)
    root.mkdir(parents=True, exist_ok=True)
    runtime_cache.mkdir(parents=True, exist_ok=True)
    key = _key(release.graph.sha256, release.worker_sha256)
    pointer = _selection_path(
        root,
        key,
        worker_sha256 if portable else None,
        hashlib.sha256(_crt_metadata(crt)).hexdigest() if portable else None,
    )
    _ordinary(pointer.with_name(pointer.name + ".lock"))
    with _download_lock(pointer), TemporaryDirectory(prefix=".staging-", dir=root) as stage_root:
        temporary = Path(stage_root)
        scratch = temporary / "load"
        scratch.mkdir()

        try:
            installed = _installed(checkpoint_cache, release, crt, executable if portable else None)
        except FileNotFoundError, ValueError:
            # Stage repair in a fresh generation. Existing readers retain their files.
            pass
        else:
            if (installed.worker_sha256, installed.worker_bytes) == (worker_sha256, worker_bytes):
                _load_verified(installed, scratch, release)
                return installed.directory
        stage = temporary / "package"
        installation = HtdemucsInstallation(stage, worker_sha256, worker_bytes)
        _build_package(
            repository,
            checkpoint_cache,
            runtime_cache,
            executable,
            release,
            installation,
            crt,
            offline=offline,
        )
        _load_verified(installation, scratch, release)
        destination = root / f"{key}-{uuid4().hex[:12]}"
        stage.rename(destination)
        _publish(
            pointer,
            HtdemucsInstallation(destination, worker_sha256, worker_bytes),
            temporary,
            release,
        )
        return destination


def verify_selected_htdemucs_onnx(
    repository: Path,
    checkpoint_cache: Path,
    *,
    executable: Path | None = None,
    crt_dir: Path | None = None,
    load: bool = True,
    verify_assets: bool = True,
) -> Path:
    """Verify a retained package and optionally prove a fresh CPU load."""
    from flitzis_looper.analysis.native_provisioning import (  # noqa: PLC0415
        _require_windows_x64,
        _visual_cpp_runtime,
    )

    _require_windows_x64()
    release = _release(repository)
    if load and not verify_assets:
        msg = "HTDemucs CPU load requires full package verification"
        raise ValueError(msg)
    crt = crt_dir if crt_dir is not None else _visual_cpp_runtime() if verify_assets else None
    installed = _installed(checkpoint_cache, release, crt, executable, verify_assets=verify_assets)
    if load:
        with TemporaryDirectory(prefix="htdemucs-load-") as temporary:
            _load_verified(installed, Path(temporary).resolve(), release)
    return installed.directory
