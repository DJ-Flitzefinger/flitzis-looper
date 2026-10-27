"""Closed CUDA-auto installation profile, separate from the legacy CPU receipt."""

import tomllib
from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from pydantic import ConfigDict, TypeAdapter

from flitzis_looper.analysis import artifacts
from flitzis_looper.analysis.execution import (
    CUDA_ENVIRONMENT_ID,
    CUDA_LOCK_SHA256,
    CudaBeatModelIdentity,
    ProtocolTwo,
)
from flitzis_looper.analysis.worker_profile_markers import expected_packages

if TYPE_CHECKING:
    from pathlib import Path
    from threading import Event

    from flitzis_looper.analysis.contracts import BeatModelIdentity

CUDA_WORKER_FILES = ("pyproject.toml", "uv.lock", "worker.py", "profile_markers.py")
type WorkerProfile = Literal["legacy-cpu-v1", "cuda-auto-v2"]


@dataclass(frozen=True, slots=True)
class CudaInstalledManifest:
    """The old artifact evidence describes checkpoint bytes, not CUDA acceptance."""

    __pydantic_config__ = ConfigDict(extra="forbid")

    schema_version: ProtocolTwo
    profile: Literal["cuda-auto-v2"]
    artifact: artifacts.AcceptedArtifact
    model: CudaBeatModelIdentity
    project_files: dict[str, artifacts.Digest]
    environment: artifacts.EnvironmentManifest
    environment_files: dict[str, artifacts.Digest]


CUDA_MANIFEST_ADAPTER = TypeAdapter(CudaInstalledManifest)
_INSTALLED_ADAPTER: TypeAdapter[artifacts.InstalledManifest | CudaInstalledManifest] = TypeAdapter(
    artifacts.InstalledManifest | CudaInstalledManifest
)


def read_installed_manifest(path: Path) -> artifacts.InstalledManifest | CudaInstalledManifest:
    if path.stat().st_size > 4 * 1024 * 1024:
        message = "installation_manifest_limit"
        raise ValueError(message)
    return _INSTALLED_ADAPTER.validate_json(path.read_bytes(), strict=True)


def verify_worker_installation(
    path: Path, model: BeatModelIdentity | CudaBeatModelIdentity, cancel: Event
) -> None:
    if isinstance(model, CudaBeatModelIdentity):
        if path.stat().st_size > 4 * 1024 * 1024:
            message = "installation_manifest_limit"
            raise ValueError(message)
        verify_cuda_installation(path, model, cancel)
    else:
        artifacts.verify_installation(path, model, cancel)


def cuda_source_directory() -> Path:
    return artifacts.worker_source_directory().with_name("beat_this_cuda")


def cuda_model_identity(environment: artifacts.EnvironmentManifest) -> CudaBeatModelIdentity:
    if environment.environment_id != CUDA_ENVIRONMENT_ID:
        msg = "unknown_cuda_worker_profile"
        raise ValueError(msg)
    return CudaBeatModelIdentity(
        sha256=artifacts.ACCEPTED_ARTIFACT.sha256,
        frontend_id=artifacts.FRONTEND_ID,
    )


def validate_cuda_environment(environment: artifacts.EnvironmentManifest, project: Path) -> None:
    lock_digest = artifacts.file_digest(project / "uv.lock")
    if lock_digest != CUDA_LOCK_SHA256:
        msg = "unknown_cuda_worker_lock"
        raise ValueError(msg)
    with (project / "uv.lock").open("rb") as stream:
        packages = expected_packages(tomllib.load(stream), "flitzis-beat-this-cuda-worker")
    expected = artifacts.EnvironmentManifest(
        schema_version=1,
        frontend_id=artifacts.FRONTEND_ID,
        environment_id=CUDA_ENVIRONMENT_ID,
        python_version="3.12.13",
        packages=packages,
        lock_sha256=CUDA_LOCK_SHA256,
        worker_sha256=artifacts.file_digest(project / "worker.py"),
    )
    if environment != expected:
        msg = "worker_environment_mismatch"
        raise ValueError(msg)


def verify_cuda_installation(
    manifest_path: Path, model: CudaBeatModelIdentity, cancel: Event
) -> None:
    manifest = CUDA_MANIFEST_ADAPTER.validate_json(manifest_path.read_bytes(), strict=True)
    project = cuda_source_directory()
    expected_files = {name: artifacts.file_digest(project / name) for name in CUDA_WORKER_FILES}
    if (
        manifest.artifact != artifacts.ACCEPTED_ARTIFACT
        or manifest.model != model
        or model != cuda_model_identity(manifest.environment)
        or manifest.project_files != expected_files
    ):
        msg = "installation_cuda_profile_mismatch"
        raise ValueError(msg)
    validate_cuda_environment(manifest.environment, project)
    for name, digest in expected_files.items():
        if cancel.is_set() or artifacts.file_digest(manifest_path.parent / name) != digest:
            msg = "installation_project_corrupt"
            raise ValueError(msg)
    artifacts.verify_environment_files(manifest_path.parent, manifest, cancel)
