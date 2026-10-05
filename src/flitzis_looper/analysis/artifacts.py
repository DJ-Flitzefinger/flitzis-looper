"""Accepted Beat This provenance and bounded local installation verification."""

import hashlib
import re
import tomllib
from dataclasses import dataclass
from email.parser import BytesParser
from pathlib import Path, PurePosixPath
from typing import TYPE_CHECKING, Annotated, Literal

from pydantic import ConfigDict, Field, TypeAdapter

from flitzis_looper.analysis.acquisition import CHECKPOINT_URL
from flitzis_looper.analysis.contracts import BeatModelIdentity

if TYPE_CHECKING:
    from threading import Event

SOURCE_REVISION = "b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c"
FRONTEND_ID = "beat-this-1.1.0-soxr-hq-logmel-v1"
WORKER_FILES = ("pyproject.toml", "uv.lock", "worker.py")
_METADATA_LIMIT = 4 * 1024 * 1024
type Digest = Annotated[str, Field(pattern=r"^[0-9a-f]{64}$")]


class _StrictManifest:
    __slots__ = ()
    __pydantic_config__ = ConfigDict(extra="forbid")


@dataclass(frozen=True, slots=True)
class AcceptedArtifact(_StrictManifest):
    """Accepted bytes and immutable primary source/license evidence."""

    sha256: Digest
    source_url: str
    source_revision: str
    license: str
    license_evidence: str
    license_text: str
    size_bytes: int
    observed_date: str = "2026-10-05"
    checkpoint: Literal["final0"] = "final0"
    package_version: Literal["1.1.0"] = "1.1.0"
    postprocessor: Literal["minimal"] = "minimal"
    device: Literal["cpu"] = "cpu"
    precision: Literal["float32"] = "float32"
    frontend_id: str = FRONTEND_ID


# Observed author-hosted bytes, independently rehashed after explicit acquisition;
# upstream does not publish a signed digest for this artifact.
ACCEPTED_ARTIFACT = AcceptedArtifact(
    sha256="8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331",
    source_url=CHECKPOINT_URL,
    source_revision=SOURCE_REVISION,
    license="MIT",
    license_evidence=f"https://github.com/CPJKU/beat_this/blob/{SOURCE_REVISION}/README.md#license",
    license_text=f"https://github.com/CPJKU/beat_this/blob/{SOURCE_REVISION}/LICENSE",
    size_bytes=81058141,
)


@dataclass(frozen=True, slots=True)
class EnvironmentManifest(_StrictManifest):
    """Environment actually inspected by the isolated worker during setup."""

    schema_version: Literal[1]
    frontend_id: str
    environment_id: str
    python_version: str
    packages: dict[str, str]
    lock_sha256: Digest
    worker_sha256: Digest


@dataclass(frozen=True, slots=True)
class InstalledManifest(_StrictManifest):
    """Atomic-install receipt; paths are fixed by the application, never supplied here."""

    schema_version: Literal[1]
    artifact: AcceptedArtifact
    model: BeatModelIdentity
    project_files: dict[str, Digest]
    environment: EnvironmentManifest
    environment_files: dict[str, Digest]


@dataclass(frozen=True, slots=True)
class _Dependency:
    name: str


@dataclass(frozen=True, slots=True)
class _LockedPackage:
    name: str
    version: str
    dependencies: tuple[_Dependency, ...] = ()


@dataclass(frozen=True, slots=True)
class _Lock:
    package: tuple[_LockedPackage, ...]


ENVIRONMENT_ADAPTER = TypeAdapter(EnvironmentManifest)
MANIFEST_ADAPTER = TypeAdapter(InstalledManifest)
_LOCK_ADAPTER = TypeAdapter(_Lock)


def worker_source_directory() -> Path:
    """Locate the separately shipped worker project in a source checkout."""
    return Path(__file__).resolve().parents[3] / "workers" / "beat_this"


def file_digest(path: Path, *, max_bytes: int = _METADATA_LIMIT) -> str:
    """Hash one bounded local metadata/artifact file without importing optional code."""
    digest = hashlib.sha256()
    count = 0
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            count += len(chunk)
            if count > max_bytes:
                msg = "artifact_file_limit"
                raise ValueError(msg)
            digest.update(chunk)
    return digest.hexdigest()


def read_manifest(path: Path) -> InstalledManifest:
    """Read bounded strict install metadata without loading a model or starting a process."""
    if path.stat().st_size > _METADATA_LIMIT:
        msg = "installation_manifest_limit"
        raise ValueError(msg)
    return MANIFEST_ADAPTER.validate_json(path.read_bytes(), strict=True)


def expected_packages(project: Path) -> dict[str, str]:
    """Resolve the locked runtime dependency closure, excluding test-only packages."""
    with (project / "uv.lock").open("rb") as stream:
        locked = _LOCK_ADAPTER.validate_python(tomllib.load(stream))
    packages = {package.name: package for package in locked.package}
    pending = list(packages["flitzis-beat-this-worker"].dependencies)
    required: dict[str, str] = {}
    while pending:
        dependency = pending.pop()
        if dependency.name in required:
            continue
        package = packages[dependency.name]
        required[package.name] = package.version
        pending.extend(package.dependencies)
    return required


def validate_environment(environment: EnvironmentManifest, project: Path) -> None:
    """Reject recorded package/configuration provenance that differs from the locked project."""
    lock_digest = file_digest(project / "uv.lock")
    expected = EnvironmentManifest(
        schema_version=1,
        frontend_id=FRONTEND_ID,
        environment_id=f"uv-lock-sha256:{lock_digest}",
        python_version="3.12.13",
        packages=expected_packages(project),
        lock_sha256=lock_digest,
        worker_sha256=file_digest(project / "worker.py"),
    )
    if environment != expected:
        msg = "worker_environment_mismatch"
        raise ValueError(msg)


def model_identity(environment: EnvironmentManifest) -> BeatModelIdentity:
    """Bind the accepted model to its inspected frontend and locked environment."""
    return BeatModelIdentity(
        sha256=ACCEPTED_ARTIFACT.sha256,
        frontend_id=environment.frontend_id,
        environment_id=environment.environment_id,
    )


def verify_installation(manifest_path: Path, model: BeatModelIdentity, cancel: Event) -> None:
    """Verify accepted provenance, local script/lock/environment identity before inference."""
    manifest = read_manifest(manifest_path)
    project = worker_source_directory()
    installed = manifest_path.parent
    expected_files = {name: file_digest(project / name) for name in WORKER_FILES}
    if (
        manifest.artifact != ACCEPTED_ARTIFACT
        or manifest.model != model
        or manifest.model != model_identity(manifest.environment)
    ):
        msg = "installation_model_mismatch"
        raise ValueError(msg)
    if manifest.project_files != expected_files:
        msg = "installation_project_mismatch"
        raise ValueError(msg)
    validate_environment(manifest.environment, project)
    for name, digest in manifest.project_files.items():
        if file_digest(installed / name) != digest:
            msg = "installation_project_corrupt"
            raise ValueError(msg)
    _verify_environment_files(installed, manifest, cancel)


def environment_files(installed: Path) -> list[Path]:
    """List interpreter, package identity and small Beat This sources for integrity checks."""
    files = [installed / ".venv" / "pyvenv.cfg", installed / ".venv" / "Scripts" / "python.exe"]
    site_packages = installed / ".venv" / "Lib" / "site-packages"
    for filename in ("METADATA", "RECORD"):
        files.extend(site_packages.glob(f"*.dist-info/{filename}"))
    files.extend((site_packages / "beat_this").rglob("*.py"))
    return files


def _verify_environment_files(installed: Path, manifest: InstalledManifest, cancel: Event) -> None:
    files = manifest.environment_files
    required = {".venv/pyvenv.cfg", ".venv/Scripts/python.exe"}
    actual = {path.relative_to(installed).as_posix() for path in environment_files(installed)}
    if not required.issubset(files) or set(files) != actual:
        msg = "installation_environment_incomplete"
        raise ValueError(msg)
    packages: dict[str, str] = {}
    for name, digest in files.items():
        path = PurePosixPath(name)
        package_metadata = (
            name.startswith(".venv/Lib/site-packages/")
            and path.name in {"METADATA", "RECORD"}
            and path.parent.name.endswith(".dist-info")
        )
        package_source = (
            name.startswith(".venv/Lib/site-packages/beat_this/") and path.suffix == ".py"
        )
        allowed = name in required or package_metadata or package_source
        if not allowed or path.is_absolute() or ".." in path.parts or "\\" in name:
            msg = "installation_environment_path_invalid"
            raise ValueError(msg)
        if cancel.is_set():
            msg = "cancelled"
            raise ValueError(msg)
        if file_digest(installed / path) != digest:
            msg = "installation_environment_corrupt"
            raise ValueError(msg)
        if path.name == "METADATA":
            _read_distribution(installed / path, packages)
    if packages != manifest.environment.packages:
        msg = "installation_packages_mismatch"
        raise ValueError(msg)


def _read_distribution(path: Path, packages: dict[str, str]) -> None:
    metadata = BytesParser().parsebytes(path.read_bytes(), headersonly=True)
    name = re.sub(r"[-_.]+", "-", str(metadata.get("Name", ""))).lower()
    version = str(metadata.get("Version", ""))
    if not name or not version or name in packages or not path.with_name("RECORD").is_file():
        msg = "installation_package_metadata_invalid"
        raise ValueError(msg)
    packages[name] = version
