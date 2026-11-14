"""Prepare or check the Windows source-clone BPM and optional Stem models."""

# Public command filename follows the repository's existing hyphenated CLI convention.
# ruff: noqa: N999

import argparse
import hashlib
import json
import platform
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING

from flitzis_looper.analysis import setup
from flitzis_looper.analysis.acquisition import acquire_pinned_asset
from flitzis_looper.analysis.native_beat_worker import (
    NativeWorkerConfiguration,
    require_native_worker_capability,
)
from flitzis_looper.analysis.native_provisioning import (
    build_native_worker,
    load_production_worker,
    stage_production_worker,
)
from flitzis_looper.bs_input_provenance import bs_input_contract
from flitzis_looper.bs_onnx_installation import BS_ONNX_MODELS_DIRECTORY
from flitzis_looper.bs_onnx_provisioning import (
    build_bs_onnx_worker,
    select_bs_onnx_installation,
    stage_bs_onnx_installation,
)
from flitzis_looper.controller import bs_roformer_assets
from flitzis_looper.controller.model_pins import DEMUCS_ASSETS
from flitzis_looper.controller.stem_generation import (
    default_stem_model_cache_dir,
    protected_model_roots,
    resolved_demucs_encoder,
    stem_model_environment,
)
from flitzis_looper.htdemucs_onnx_provisioning import (
    build_htdemucs_onnx_worker,
    htdemucs_source_crt,
    stage_htdemucs_onnx_installation,
)

if TYPE_CHECKING:
    from flitzis_looper.analysis.beat_cuda_cache import CudaCacheStatus
    from flitzis_looper.bs_onnx_cuda_cache import BsCudaCacheStatus


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


def production_windows() -> bool:
    return sys.platform == "win32" and platform.machine().lower() in {"amd64", "x86_64"}


def prepare_windows_worker(
    repository: Path, install_root: Path, scratch: Path, *, check: bool
) -> tuple[NativeWorkerConfiguration, CudaCacheStatus]:
    """Select the verified ONNX package and prepare CUDA on setup or repair."""
    from flitzis_looper.analysis.beat_cuda_cache import (  # noqa: PLC0415
        cuda_cache_status,
        ensure_cuda_cache,
    )

    if check:
        configuration = load_production_worker(install_root, scratch)
        require_native_worker_capability(configuration)
        return configuration, cuda_cache_status(configuration)
    executable = build_native_worker(repository, scratch)
    configuration = stage_production_worker(install_root, scratch, repository, executable)
    require_native_worker_capability(configuration)
    status = ensure_cuda_cache(configuration, retry_failed=True)
    if status["status"] not in {"ready", "unavailable"}:
        raise RuntimeError(status["detail"])
    selected = setup.activate_worker_installation(
        install_root, configuration.manifest.parent, scratch
    )
    assert isinstance(selected, NativeWorkerConfiguration)
    return selected, status


def verify_asset(path: Path, checksum: str, size: int) -> None:
    with path.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha256").hexdigest()
    if actual != checksum or path.stat().st_size != size:
        message = f"Model integrity check failed: {path}"
        raise ValueError(message)


def check_stem_codecs(root: Path, cache: Path) -> None:
    """Check the shared codec build and both programs selected by generation."""
    executable = resolved_demucs_encoder(cache)
    if executable is None or bs_input_contract(executable) is None:
        message = "Stem setup requires a supported shared FFmpeg build with FFprobe"
        raise ValueError(message)
    environment = stem_model_environment(cache)
    for tool in (executable, executable.with_name("ffprobe.exe")):
        subprocess.run(
            [str(tool), "-version"],
            cwd=root,
            env=environment,
            stdout=subprocess.DEVNULL,
            check=True,
            timeout=30,
        )


def prepare_stems(root: Path, *, check: bool) -> BsCudaCacheStatus | None:
    """Verify pinned Stem resources and report CUDA work done by this setup."""
    cache = default_stem_model_cache_dir(project_root=root)
    check_stem_codecs(root, cache)
    for filename, (size, checksum, url) in DEMUCS_ASSETS.items():
        if check:
            verify_asset(cache / filename, checksum, size)
        else:
            acquire_pinned_asset(cache / filename, source_url=url, sha256=checksum, size_bytes=size)
    roformer = cache / "bs-roformer-musdb18hq"
    if check:
        bs_roformer_assets.verify_model_assets(roformer)
    else:
        bs_roformer_assets.install_model_assets(roformer)
    preparations: list[BsCudaCacheStatus] = []
    if not check:
        scratch = owned_directory(root, ".cache/runtime-setup")
        stage_htdemucs_onnx_installation(
            root,
            cache,
            build_htdemucs_onnx_worker(root, scratch),
            scratch / "beat-this-downloads",
            crt_dir=htdemucs_source_crt(),
            local_build=True,
        )
        worker = build_bs_onnx_worker(root, scratch)
        package_root = owned_directory(root, BS_ONNX_MODELS_DIRECTORY)
        installation = stage_bs_onnx_installation(
            root,
            package_root,
            worker,
            roformer,
            scratch / "beat-this-downloads",
            cuda_preparation=preparations.append,
        )
        select_bs_onnx_installation(package_root, installation)
        return preparations[0]
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
        env=stem_model_environment(cache),
        check=True,
    )
    return None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check", action="store_true", help="Verify installed resources without downloads"
    )
    parser.add_argument(
        "--with-stems", action="store_true", help="Also prepare or verify both optional Stem models"
    )
    options = parser.parse_args(argv)
    root = Path(__file__).resolve().parents[1]
    try:
        require(condition=production_windows(), message="Model setup requires Windows x64")
        worker_root = owned_directory(root, "models/beat-this")
        scratch = owned_directory(root, ".cache/runtime-setup")
        configuration, cuda_cache = prepare_windows_worker(
            root, worker_root, scratch, check=options.check
        )
        stem_cuda_cache = None
        if options.with_stems:
            stem_cuda_cache = prepare_stems(root, check=options.check)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        sys.stderr.write(f"Source runtime setup failed: {error}\n")
        return 1
    stem_warmup = bool(stem_cuda_cache and stem_cuda_cache.warmup_executed)
    sys.stdout.write(
        json.dumps({
            "source_runtime": "passed",
            "worker": str(configuration.manifest),
            "stems_checked": options.with_stems,
            "inference_executed": stem_warmup or bool(cuda_cache and cuda_cache["warmup_executed"]),
            "bs_roformer_cuda_warmup_executed": stem_warmup,
            "cuda_cache": cuda_cache,
        })
        + "\n"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
