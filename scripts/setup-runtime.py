"""Prepare or check required source-clone BPM resources without App/device/inference."""

# Public command filename follows the repository's existing hyphenated CLI convention.
# ruff: noqa: N999

import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING, cast
from uuid import uuid4

from flitzis_looper.analysis import artifacts, setup
from flitzis_looper.analysis.acquisition import acquire_pinned_asset
from flitzis_looper.analysis.cuda_artifacts import CudaInstalledManifest, read_installed_manifest
from flitzis_looper.controller import bs_roformer_assets
from flitzis_looper.controller.model_pins import DEMUCS_ASSETS
from flitzis_looper.controller.stem_generation import (
    DEMUCS_REQUIRED_CHECKPOINTS,
    default_demucs_model_cache_dir,
    demucs_cache_environment,
    protected_model_roots,
)

if TYPE_CHECKING:
    from flitzis_looper.analysis.cuda_artifacts import WorkerProfile
    from flitzis_looper.analysis.worker import WorkerConfiguration


def require(*, condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def owned_directory(root: Path, relative: str) -> Path:
    """Check source-owned model/scratch topology before creating any directory."""
    candidate = root / relative
    require(
        condition=not any(
            candidate.resolve().is_relative_to(protected)
            for protected in protected_model_roots(root)
        ),
        message="Source runtime model/scratch path overlaps protected project data",
    )
    require(
        condition=candidate.resolve().is_relative_to(root / Path(relative).parts[0]),
        message="Source runtime model/scratch path escapes its owned directory",
    )
    for part in (candidate, *candidate.parents):
        require(
            condition=not part.is_symlink() and not part.is_junction(),
            message="Source runtime model/scratch paths must not traverse links",
        )
        require(
            condition=not part.exists() or part.is_dir(),
            message="Source runtime model/scratch directory is occupied by a file",
        )
        if part == root:
            break
    return candidate


def selected_profile(install_root: Path) -> WorkerProfile:
    """Retain a selected known profile even if its packages need repair."""
    try:
        require(
            condition=(install_root / "current.json").stat().st_size <= 4096,
            message="Selected worker pointer exceeds its metadata limit",
        )
        pointer = setup._POINTER_ADAPTER.validate_json(
            (install_root / "current.json").read_bytes(), strict=True
        )
        installed = setup._validated_installation_child(
            install_root, install_root / pointer.directory
        )
        manifest = installed / "manifest.json"
        require(
            condition=artifacts.file_digest(manifest) == pointer.manifest_sha256,
            message="Selected worker manifest identity differs from its pointer",
        )
        receipt = read_installed_manifest(manifest)
        return "cuda-auto-v2" if isinstance(receipt, CudaInstalledManifest) else "legacy-cpu-v1"
    except OSError, ValueError:
        return "legacy-cpu-v1"


def worker_environment(scratch: Path) -> dict[str, str]:
    environment = dict(os.environ)
    for name in ("PYTHONHOME", "PYTHONPATH", "VIRTUAL_ENV", "PYTHONUSERBASE"):
        environment.pop(name, None)
    environment.update(
        PYTHONDONTWRITEBYTECODE="1",
        PYTHONNOUSERSITE="1",
        NUMBA_CACHE_DIR=str(scratch / "numba"),
        CUDA_CACHE_PATH=str(scratch / "cuda"),
        TORCHINDUCTOR_CACHE_DIR=str(scratch / "torchinductor"),
        TEMP=str(scratch),
        TMP=str(scratch),
    )
    return environment


def inspect_worker(configuration: WorkerConfiguration, scratch: Path) -> None:
    """Compare a fresh real environment inspection with the immutable installation receipt."""
    manifest = configuration.manifest
    assert manifest is not None
    report = scratch / f"worker-environment-{uuid4().hex}.json"
    subprocess.run(
        [
            str(configuration.interpreter),
            "-I",
            "-B",
            str(configuration.script),
            "--environment-manifest",
            str(report),
        ],
        cwd=manifest.parent,
        env=worker_environment(scratch),
        timeout=120,
        check=True,
    )
    observed = artifacts.ENVIRONMENT_ADAPTER.validate_json(report.read_bytes(), strict=True)
    if observed != read_installed_manifest(manifest).environment:
        message = "Actual worker environment differs from its installation receipt"
        raise ValueError(message)


def prepare_worker(
    install_root: Path, scratch: Path, *, check: bool, profile: WorkerProfile | None
) -> WorkerConfiguration:
    chosen = selected_profile(install_root) if profile is None else profile
    scratch.mkdir(parents=True, exist_ok=True)
    try:
        configuration = setup.load_worker_configuration(install_root, scratch)
        assert configuration.manifest is not None
        configuration = setup.load_staged_worker_configuration(
            install_root, configuration.manifest.parent, scratch
        )
        require(
            condition=chosen == selected_profile(install_root),
            message="Selected worker differs from the requested profile",
        )
        inspect_worker(configuration, scratch)
    except OSError, ValueError, subprocess.SubprocessError:
        if check:
            raise
        configuration = setup.install_worker(install_root, scratch, profile=chosen, stage_only=True)
        inspect_worker(configuration, scratch)
        assert configuration.manifest is not None
        configuration = setup.activate_worker_installation(
            install_root, configuration.manifest.parent, scratch
        )
    return configuration


def verify_asset(path: Path, checksum: str, size: int) -> None:
    with path.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha256").hexdigest()
    if actual != checksum or path.stat().st_size != size:
        message = f"Model integrity check failed: {path}"
        raise ValueError(message)


def prepare_stems(root: Path, *, check: bool) -> None:
    """Acquire full pins explicitly, then run existing fresh CPU checks with networking denied."""
    cache = default_demucs_model_cache_dir(project_root=root)
    for filename in DEMUCS_REQUIRED_CHECKPOINTS["htdemucs"]:
        size, checksum, url = DEMUCS_ASSETS[filename]
        if check:
            verify_asset(cache / filename, checksum, size)
        else:
            acquire_pinned_asset(cache / filename, source_url=url, sha256=checksum, size_bytes=size)
    roformer = cache / "bs-roformer-musdb18hq"
    if check:
        bs_roformer_assets.verify_model_assets(roformer)
    else:
        bs_roformer_assets.install_model_assets(roformer)
    subprocess.run(
        [
            sys.executable,
            "-s",
            "-B",
            str(root / "scripts/portable/prepare_models.py"),
            "--root",
            str(root),
            "--checkout",
            str(root),
            "--mode",
            "check",
            "--source-clone",
        ],
        cwd=root,
        env=demucs_cache_environment(cache),
        check=True,
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check", action="store_true", help="Verify installed resources without downloads"
    )
    parser.add_argument(
        "--with-stems", action="store_true", help="Also verify both optional Stem models on CPU"
    )
    parser.add_argument("--profile", choices=("legacy-cpu-v1", "cuda-auto-v2"))
    options = parser.parse_args(argv)
    root = Path(__file__).resolve().parents[1]
    try:
        worker_root = owned_directory(root, "models/beat-this")
        scratch = owned_directory(root, ".cache/runtime-setup")
        configuration = prepare_worker(
            worker_root,
            scratch,
            check=options.check,
            profile=cast("WorkerProfile | None", options.profile),
        )
        if options.with_stems:
            prepare_stems(root, check=options.check)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        sys.stderr.write(f"Source runtime setup failed: {error}\n")
        return 1
    sys.stdout.write(
        json.dumps({
            "source_runtime": "passed",
            "worker": str(configuration.manifest),
            "stems_checked": options.with_stems,
            "inference_executed": False,
        })
        + "\n"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
