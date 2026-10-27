"""Explicit setup CLI/API for the optional isolated Beat This reference worker."""

import argparse
import os
import platform
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Event
from typing import BinaryIO, Literal
from uuid import uuid4

from pydantic import ConfigDict, TypeAdapter

from flitzis_looper.analysis import artifacts
from flitzis_looper.analysis.acquisition import acquire_to_quarantine, copy_to_quarantine
from flitzis_looper.analysis.cuda_artifacts import (
    CUDA_MANIFEST_ADAPTER,
    CUDA_WORKER_FILES,
    CudaInstalledManifest,
    WorkerProfile,
    cuda_model_identity,
    cuda_source_directory,
    read_installed_manifest,
    validate_cuda_environment,
    verify_worker_installation,
)
from flitzis_looper.analysis.execution import (
    CudaBeatModelIdentity,
    require_cuda_native_capability,
)
from flitzis_looper.analysis.worker import WorkerConfiguration


@dataclass(frozen=True, slots=True)
class _InstallationPointer:
    __pydantic_config__ = ConfigDict(extra="forbid")

    schema_version: Literal[1]
    directory: str
    manifest_sha256: str


_POINTER_ADAPTER = TypeAdapter(_InstallationPointer)


def install_worker(
    install_dir: Path,
    scratch_dir: Path,
    *,
    checkpoint_file: Path | None = None,
    offline: bool = False,
    profile: WorkerProfile = "legacy-cpu-v1",
    stage_only: bool = False,
) -> WorkerConfiguration:
    """Provision a verified installation, optionally leaving the active pointer untouched.

    Existing installations are retained, including when download, validation or setup
    fails. The optional local checkpoint supports fully offline setup with a populated
    uv cache. No analysis call invokes this operation.
    """
    _validate_setup_paths(install_dir, scratch_dir, checkpoint_file)
    if profile not in {"legacy-cpu-v1", "cuda-auto-v2"}:
        msg = "unknown_worker_profile"
        raise ValueError(msg)
    if profile == "cuda-auto-v2" and not stage_only:
        require_cuda_native_capability()
    if offline and checkpoint_file is None:
        msg = "offline setup requires an explicit local checkpoint file"
        raise ValueError(msg)
    install_dir.mkdir(parents=True, exist_ok=True)
    scratch_dir.mkdir(parents=True, exist_ok=True)
    with TemporaryDirectory(prefix=".staging-", dir=install_dir) as temporary:
        staging = Path(temporary)
        checkpoint = staging / "final0.ckpt"
        acquired = (
            copy_to_quarantine(checkpoint_file, checkpoint)
            if checkpoint_file is not None
            else acquire_to_quarantine(checkpoint)
        )
        if (
            acquired.sha256 != artifacts.ACCEPTED_ARTIFACT.sha256
            or acquired.size_bytes != artifacts.ACCEPTED_ARTIFACT.size_bytes
        ):
            msg = "checkpoint_hash_mismatch"
            raise ValueError(msg)
        source = (
            cuda_source_directory()
            if profile == "cuda-auto-v2"
            else artifacts.worker_source_directory()
        )
        files = CUDA_WORKER_FILES if profile == "cuda-auto-v2" else artifacts.WORKER_FILES
        for name in files:
            shutil.copyfile(source / name, staging / name)
        _provision_environment(staging, scratch_dir, offline=offline)
        manifest = (
            _record_cuda_installation(staging)
            if profile == "cuda-auto-v2"
            else _record_installation(staging)
        )
        verify_worker_installation(staging / "manifest.json", manifest.model, Event())
        installed = _retain_installation(staging, install_dir)
        if stage_only:
            configuration = load_staged_worker_configuration(install_dir, installed, scratch_dir)
        else:
            configuration = activate_worker_installation(install_dir, installed, scratch_dir)
    return configuration


def load_staged_worker_configuration(
    install_dir: Path, installation: Path, scratch_dir: Path
) -> WorkerConfiguration:
    """Verify one inactive direct child without reading or changing current.json."""
    _validate_setup_paths(install_dir, scratch_dir, None)
    installed = _validated_installation_child(install_dir, installation)
    manifest_path = installed / "manifest.json"
    manifest = read_installed_manifest(manifest_path)
    verify_worker_installation(manifest_path, manifest.model, Event())
    checkpoint = installed / "final0.ckpt"
    expected = artifacts.ACCEPTED_ARTIFACT
    if (
        checkpoint.stat().st_size != expected.size_bytes
        or artifacts.file_digest(checkpoint, max_bytes=expected.size_bytes) != expected.sha256
    ):
        message = "checkpoint_hash_mismatch"
        raise ValueError(message)
    return _configuration_for_installation(installed, scratch_dir, manifest)


def activate_worker_installation(
    install_dir: Path, installation: Path, scratch_dir: Path
) -> WorkerConfiguration:
    """Reverify a retained known installation and atomically select that exact child."""
    configuration = load_staged_worker_configuration(install_dir, installation, scratch_dir)
    manifest = configuration.manifest
    assert manifest is not None
    pointer = _InstallationPointer(1, manifest.parent.name, artifacts.file_digest(manifest))
    _write_installation_pointer(
        install_dir, pointer, cuda=isinstance(configuration.model, CudaBeatModelIdentity)
    )
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


def _configuration_for_installation(
    installed: Path,
    scratch_dir: Path,
    manifest: artifacts.InstalledManifest | CudaInstalledManifest,
) -> WorkerConfiguration:
    return WorkerConfiguration(
        interpreter=installed / ".venv" / "Scripts" / "python.exe",
        script=installed / "worker.py",
        checkpoint=installed / "final0.ckpt",
        scratch_dir=scratch_dir,
        model=manifest.model,
        manifest=installed / "manifest.json",
    )


def load_worker_configuration(install_dir: Path, scratch_dir: Path) -> WorkerConfiguration:
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
    installed = install_dir / pointer.directory
    manifest_path = installed / "manifest.json"
    if artifacts.file_digest(manifest_path) != pointer.manifest_sha256:
        msg = "installation_manifest_corrupt"
        raise ValueError(msg)
    manifest = read_installed_manifest(manifest_path)
    verify_worker_installation(manifest_path, manifest.model, Event())
    return _configuration_for_installation(installed, scratch_dir, manifest)


def _validate_setup_paths(
    install_dir: Path, scratch_dir: Path, checkpoint_file: Path | None
) -> None:
    if sys.platform != "win32" or platform.machine().lower() not in {"amd64", "x86_64"}:
        msg = "this locked worker currently supports Windows x64 only"
        raise ValueError(msg)
    paths = (install_dir, scratch_dir) + ((checkpoint_file,) if checkpoint_file else ())
    if any(not path.is_absolute() for path in paths):
        msg = "setup paths must be absolute"
        raise ValueError(msg)


def _provision_environment(staging: Path, scratch_dir: Path, *, offline: bool) -> None:
    uv = shutil.which("uv")
    if uv is None:
        msg = "explicit worker setup requires uv on PATH"
        raise FileNotFoundError(msg)
    command = [
        uv,
        "sync",
        "--project",
        str(staging),
        "--python",
        "3.12.13",
        "--locked",
        "--no-dev",
        "--no-editable",
    ]
    if offline:
        command.append("--offline")
    environment = os.environ.copy()
    environment.update({
        "UV_PROJECT_ENVIRONMENT": str(staging / ".venv"),
        "UV_LINK_MODE": "copy",
        "UV_NO_PROGRESS": "1",
    })
    log_path = scratch_dir / f"beat-this-setup-{uuid4().hex}.log"
    with log_path.open("xb") as log:
        subprocess.run(
            command,
            cwd=staging,
            env=environment,
            stdout=log,
            stderr=subprocess.STDOUT,
            check=True,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        _inspect_environment(staging, log)


def _inspect_environment(staging: Path, log: BinaryIO) -> None:
    subprocess.run(
        [
            str(staging / ".venv" / "Scripts" / "python.exe"),
            "-I",
            str(staging / "worker.py"),
            "--environment-manifest",
            str(staging / "environment.json"),
        ],
        cwd=staging,
        stdout=log,
        stderr=subprocess.STDOUT,
        check=True,
        timeout=120,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )


def _record_installation(staging: Path) -> artifacts.InstalledManifest:
    environment_path = staging / "environment.json"
    if environment_path.stat().st_size > 64 * 1024:
        msg = "environment_manifest_limit"
        raise ValueError(msg)
    environment = artifacts.ENVIRONMENT_ADAPTER.validate_json(
        environment_path.read_bytes(), strict=True
    )
    artifacts.validate_environment(environment, staging)
    manifest = artifacts.InstalledManifest(
        schema_version=1,
        artifact=artifacts.ACCEPTED_ARTIFACT,
        model=artifacts.model_identity(environment),
        project_files={
            name: artifacts.file_digest(staging / name) for name in artifacts.WORKER_FILES
        },
        environment=environment,
        environment_files={
            path.relative_to(staging).as_posix(): artifacts.file_digest(path)
            for path in artifacts.environment_files(staging)
        },
    )
    (staging / "manifest.json").write_bytes(
        artifacts.MANIFEST_ADAPTER.dump_json(manifest, indent=2)
    )
    return manifest


def _record_cuda_installation(staging: Path) -> CudaInstalledManifest:
    path = staging / "environment.json"
    if path.stat().st_size > 64 * 1024:
        msg = "environment_manifest_limit"
        raise ValueError(msg)
    environment = artifacts.ENVIRONMENT_ADAPTER.validate_json(path.read_bytes(), strict=True)
    validate_cuda_environment(environment, staging)
    manifest = CudaInstalledManifest(
        schema_version=2,
        profile="cuda-auto-v2",
        artifact=artifacts.ACCEPTED_ARTIFACT,
        model=cuda_model_identity(environment),
        project_files={name: artifacts.file_digest(staging / name) for name in CUDA_WORKER_FILES},
        environment=environment,
        environment_files={
            path.relative_to(staging).as_posix(): artifacts.file_digest(path)
            for path in artifacts.environment_files(staging)
        },
    )
    (staging / "manifest.json").write_bytes(CUDA_MANIFEST_ADAPTER.dump_json(manifest, indent=2))
    return manifest


def _retain_installation(staging: Path, install_dir: Path) -> Path:
    directory = f"installation-{uuid4().hex}"
    installed = install_dir / directory
    staging.rename(installed)
    return installed


def _write_installation_pointer(
    install_dir: Path, pointer: _InstallationPointer, *, cuda: bool
) -> None:
    temporary_pointer = install_dir / f".current-{uuid4().hex}.json"
    try:
        temporary_pointer.write_bytes(_POINTER_ADAPTER.dump_json(pointer, indent=2))
        if cuda:
            require_cuda_native_capability()
        temporary_pointer.replace(install_dir / "current.json")
    finally:
        temporary_pointer.unlink(missing_ok=True)


def main() -> int:
    """Run explicit optional setup; print installed local paths or a concrete failure."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--install-dir", type=Path, required=True)
    parser.add_argument("--scratch-dir", type=Path, required=True)
    parser.add_argument("--checkpoint-file", type=Path)
    parser.add_argument("--offline", action="store_true")
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--stage-only", action="store_true")
    action.add_argument("--activate-installation", type=Path)
    parser.add_argument(
        "--profile", choices=("legacy-cpu-v1", "cuda-auto-v2"), default="legacy-cpu-v1"
    )
    options = parser.parse_args()
    if options.activate_installation is not None and (
        options.checkpoint_file is not None or options.offline or options.profile != "legacy-cpu-v1"
    ):
        parser.error(
            "activation uses the retained manifest; omit checkpoint, offline and profile options"
        )
    try:
        if options.activate_installation is not None:
            configuration = activate_worker_installation(
                options.install_dir, options.activate_installation, options.scratch_dir
            )
        else:
            configuration = install_worker(
                options.install_dir,
                options.scratch_dir,
                checkpoint_file=options.checkpoint_file,
                offline=options.offline,
                profile=options.profile,
                stage_only=options.stage_only,
            )
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.stderr.write(f"Beat This setup failed: {error}\n")
        return 1
    status = "Verified inactive" if options.stage_only else "Verified"
    sys.stdout.write(f"{status} Beat This worker: {configuration.manifest}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
