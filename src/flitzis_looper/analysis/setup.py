"""Explicit setup CLI/API for the installed Beat This ONNX worker."""

import argparse
import hashlib
import platform
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Literal
from uuid import uuid4

from pydantic import ConfigDict, TypeAdapter

from flitzis_looper.analysis import artifacts
from flitzis_looper.analysis.native_beat_worker import (
    NativeWorkerConfiguration,
    is_production_worker,
    load_native_configuration,
    require_native_worker_capability,
    stage_native_package,
    verify_native_package,
)


@dataclass(frozen=True, slots=True)
class _InstallationPointer:
    __pydantic_config__ = ConfigDict(extra="forbid")

    schema_version: Literal[1]
    directory: str
    manifest_sha256: str


_POINTER_ADAPTER = TypeAdapter(_InstallationPointer)


def stage_native_worker(
    install_dir: Path,
    scratch_dir: Path,
    package: Path,
    *,
    cuda_policy: Literal["cpu", "cuda", "auto"] | None = None,
    startup_fault_protocol: Literal[1] | None = None,
) -> NativeWorkerConfiguration:
    """Retain a complete local Native worker without changing the active selection."""
    _validate_setup_paths(install_dir, scratch_dir, package)
    install_dir.mkdir(parents=True, exist_ok=True)
    with TemporaryDirectory(prefix=".staging-", dir=install_dir) as temporary:
        staging = Path(temporary)
        stage_native_package(
            package, staging, cuda_policy=cuda_policy, startup_fault_protocol=startup_fault_protocol
        )
        retained = _retain_installation(staging, install_dir)
    return load_staged_worker_configuration(install_dir, retained, scratch_dir)


def load_staged_worker_configuration(
    install_dir: Path, installation: Path, scratch_dir: Path
) -> NativeWorkerConfiguration:
    """Verify one inactive direct child without reading or changing current.json."""
    _validate_setup_paths(install_dir, scratch_dir, None)
    installed = _validated_installation_child(install_dir, installation)
    manifest_path = installed / "manifest.json"
    configuration = _require_onnx_configuration(
        manifest_path, _manifest_bytes(manifest_path), scratch_dir
    )
    verify_native_package(configuration)
    return configuration


def activate_worker_installation(
    install_dir: Path, installation: Path, scratch_dir: Path
) -> NativeWorkerConfiguration:
    """Reverify a retained known installation and atomically select that exact child."""
    configuration = load_staged_worker_configuration(install_dir, installation, scratch_dir)
    manifest = configuration.manifest
    pointer = _InstallationPointer(1, manifest.parent.name, artifacts.file_digest(manifest))
    require_native_worker_capability(configuration)
    _write_installation_pointer(install_dir, pointer)
    return configuration


def _validated_installation_child(install_dir: Path, installation: Path) -> Path:
    valid_name = re.fullmatch(r"installation-[0-9a-f]{32}", installation.name) is not None
    if not installation.is_absolute() or installation.parent != install_dir or not valid_name:
        message = "installation_child_invalid"
        raise ValueError(message)
    if installation.is_symlink() or installation.is_junction():
        message = "installation_child_reparse"
        raise ValueError(message)
    if installation.resolve(strict=True).parent != install_dir.resolve(strict=True):
        message = "installation_child_outside_root"
        raise ValueError(message)
    return installation


def _manifest_bytes(path: Path) -> bytes:
    limit = 4 * 1024 * 1024
    with path.open("rb") as stream:
        raw = stream.read(limit + 1)
    if len(raw) > limit:
        message = "installation_manifest_limit"
        raise ValueError(message)
    return raw


def load_worker_configuration(
    install_dir: Path, scratch_dir: Path, *, native_only: bool = False
) -> NativeWorkerConfiguration:
    """Read the explicitly named installed worker without acquisition or subprocess startup."""
    if not install_dir.is_absolute() or not scratch_dir.is_absolute():
        msg = "worker installation and scratch paths must be absolute"
        raise ValueError(msg)
    pointer_path = install_dir / "current.json"
    if pointer_path.stat().st_size > 4096:
        msg = "installation_pointer_limit"
        raise ValueError(msg)
    pointer = _POINTER_ADAPTER.validate_json(pointer_path.read_bytes(), strict=True)
    if re.fullmatch(r"installation-[0-9a-f]{32}", pointer.directory) is None:
        msg = "installation_pointer_invalid"
        raise ValueError(msg)
    installed = _validated_installation_child(install_dir, install_dir / pointer.directory)
    manifest_path = installed / "manifest.json"
    raw = _manifest_bytes(manifest_path)
    if hashlib.sha256(raw).hexdigest() != pointer.manifest_sha256:
        msg = "installation_manifest_corrupt"
        raise ValueError(msg)
    configuration = _require_onnx_configuration(manifest_path, raw, scratch_dir)
    if native_only and not is_production_worker(configuration):
        msg = "Update the BeatThis ONNX worker through the installer or setup-runtime.py."
        raise ValueError(msg)
    return configuration


def _require_onnx_configuration(
    manifest: Path, raw: bytes, scratch_dir: Path
) -> NativeWorkerConfiguration:
    configuration = load_native_configuration(manifest, raw, scratch_dir)
    if configuration is None:
        msg = "BeatThis requires the ONNX worker. Update the installation or run setup-runtime.py."
        raise ValueError(msg)
    return configuration


def _validate_setup_paths(install_dir: Path, scratch_dir: Path, package: Path | None) -> None:
    if sys.platform != "win32" or platform.machine().lower() not in {"amd64", "x86_64"}:
        msg = "this locked worker currently supports Windows x64 only"
        raise ValueError(msg)
    paths = (install_dir, scratch_dir) + ((package,) if package is not None else ())
    if any(not path.is_absolute() for path in paths):
        msg = "setup paths must be absolute"
        raise ValueError(msg)


def _retain_installation(staging: Path, install_dir: Path) -> Path:
    directory = f"installation-{uuid4().hex}"
    installed = install_dir / directory
    staging.rename(installed)
    return installed


def _write_installation_pointer(install_dir: Path, pointer: _InstallationPointer) -> None:
    temporary_pointer = install_dir / f".current-{uuid4().hex}.json"
    try:
        temporary_pointer.write_bytes(_POINTER_ADAPTER.dump_json(pointer, indent=2))
        temporary_pointer.replace(install_dir / "current.json")
    finally:
        temporary_pointer.unlink(missing_ok=True)


def main() -> int:
    """Run explicit optional setup; print installed local paths or a concrete failure."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--install-dir", type=Path, required=True)
    parser.add_argument("--scratch-dir", type=Path, required=True)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--activate-installation", type=Path)
    action.add_argument("--stage-native-cpu-package", type=Path)
    action.add_argument("--stage-native-cuda-package", type=Path)
    parser.add_argument("--native-cuda-policy", choices=("cuda", "auto"))
    options = parser.parse_args()
    package = options.stage_native_cuda_package or options.stage_native_cpu_package
    if options.native_cuda_policy is not None and options.stage_native_cuda_package is None:
        parser.error("--native-cuda-policy requires --stage-native-cuda-package")
    cuda_policy = (
        options.native_cuda_policy or "cuda"
        if options.stage_native_cuda_package is not None
        else None
    )
    try:
        if package is not None:
            configuration = stage_native_worker(
                options.install_dir, options.scratch_dir, package, cuda_policy=cuda_policy
            )
        else:
            configuration = activate_worker_installation(
                options.install_dir, options.activate_installation, options.scratch_dir
            )
    except (OSError, ValueError) as error:
        sys.stderr.write(f"Beat This setup failed: {error}\n")
        return 1
    status = "Verified inactive" if package is not None else "Verified"
    sys.stdout.write(f"{status} Beat This worker: {configuration.manifest}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
