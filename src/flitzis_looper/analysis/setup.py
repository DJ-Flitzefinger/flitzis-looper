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
) -> WorkerConfiguration:
    """Explicitly provision and atomically select one fully verified isolated installation.

    Existing installations are retained, including when download, validation or setup
    fails. The optional local checkpoint supports fully offline setup with a populated
    uv cache. No analysis call invokes this operation.
    """
    _validate_setup_paths(install_dir, scratch_dir, checkpoint_file)
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
        source = artifacts.worker_source_directory()
        for name in artifacts.WORKER_FILES:
            shutil.copyfile(source / name, staging / name)
        _provision_environment(staging, scratch_dir, offline=offline)
        manifest = _record_installation(staging)
        artifacts.verify_installation(staging / "manifest.json", manifest.model, Event())
        _publish_installation(staging, install_dir)
    return load_worker_configuration(install_dir, scratch_dir)


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
    manifest = artifacts.read_manifest(manifest_path)
    artifacts.verify_installation(manifest_path, manifest.model, Event())
    return WorkerConfiguration(
        interpreter=installed / ".venv" / "Scripts" / "python.exe",
        script=installed / "worker.py",
        checkpoint=installed / "final0.ckpt",
        scratch_dir=scratch_dir,
        model=manifest.model,
        manifest=manifest_path,
    )


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
            timeout=900,
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


def _publish_installation(staging: Path, install_dir: Path) -> None:
    directory = f"installation-{uuid4().hex}"
    pointer = _InstallationPointer(1, directory, artifacts.file_digest(staging / "manifest.json"))
    staging.rename(install_dir / directory)
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
    parser.add_argument("--checkpoint-file", type=Path)
    parser.add_argument("--offline", action="store_true")
    options = parser.parse_args()
    try:
        configuration = install_worker(
            options.install_dir,
            options.scratch_dir,
            checkpoint_file=options.checkpoint_file,
            offline=options.offline,
        )
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.stderr.write(f"Beat This setup failed: {error}\n")
        return 1
    sys.stdout.write(f"Verified Beat This worker: {configuration.manifest}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
