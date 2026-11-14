"""Stage and verify the private Windows x64 ONNX BeatThis worker."""

import argparse
import base64
import hashlib
import importlib.util
import json
import os
import stat
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from flitzis_looper.analysis.native_beat_worker import NativeWorkerConfiguration

WORKER_EXECUTABLE = Path("runtime/beat-this/native-beat-this-worker.exe")
WORKER_EXE_LIMIT = 64 * 1024 * 1024


def invalid(message: str) -> None:
    raise ValueError(message)


def physical(path: Path, root: Path | None = None) -> Path:
    """Reject reparse points and compare containment independently of SUBST spelling."""
    path = Path(os.path.abspath(path))
    for part in reversed((path, *path.parents)):
        if part.exists() or part.is_symlink():
            info = part.lstat()
            if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
                invalid("Portable analysis paths must not traverse links")
    if root is not None:

        def canonical(value: Path) -> Path:
            name = str(value.resolve())
            if name.startswith("\\\\?\\UNC\\"):
                name = "\\\\" + name[8:]
            elif name.startswith("\\\\?\\"):
                name = name[4:]
            return Path(name)

        if not canonical(path).is_relative_to(canonical(physical(root))):
            invalid("Portable analysis path escapes its installation")
    return path


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_new_json(path: Path, value: dict) -> None:
    physical(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")


def configure(root: Path, checkout: Path, runtime: Path) -> list[object]:
    specification = importlib.util.spec_from_file_location(
        "portable_analysis_bootstrap", checkout / "scripts/portable/bootstrap.py"
    )
    assert specification is not None
    assert specification.loader is not None
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module.configure(root, checkout, runtime)


def verify_release_worker(
    configuration: NativeWorkerConfiguration, executable: Path, *, engine_type: object | None = None
) -> None:
    """Bind the selected worker to the packaged executable and matching App capability."""
    from flitzis_looper.analysis.native_beat_worker import (  # noqa: PLC0415
        is_production_worker,
        require_native_worker_capability,
    )

    if not is_production_worker(configuration):
        invalid("Portable BeatThis requires the production ONNX worker; run the installer")
    info = physical(executable).stat()
    if not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= WORKER_EXE_LIMIT:
        invalid("Invalid Portable BeatThis Release executable")
    if digest(executable) != configuration.executable_sha256:
        invalid("Portable BeatThis worker differs from this Release; run the installer")
    require_native_worker_capability(configuration, engine_type=engine_type)


def load_verified_selection(
    install_root: Path, scratch: Path, executable: Path, *, engine_type: object | None = None
) -> NativeWorkerConfiguration:
    """Verify the current pointer, provider package, and release identity without inference."""
    from flitzis_looper.analysis.native_provisioning import load_production_worker  # noqa: PLC0415

    physical(install_root / "current.json", install_root)
    configuration = load_production_worker(install_root, scratch)
    verify_release_worker(configuration, executable, engine_type=engine_type)
    return configuration


def worker_result(
    configuration: NativeWorkerConfiguration, cache_status: dict[str, object]
) -> dict[str, object]:
    """Keep the journal's pointer contract and report actual preparation status."""
    pointer = {
        "schema_version": 1,
        "directory": configuration.manifest.parent.name,
        "manifest_sha256": digest(configuration.manifest),
    }
    payload = (json.dumps(pointer, indent=2) + "\n").encode("utf-8")
    return {
        "portable_analysis": "passed",
        "execution_policy": configuration.execution_policy,
        "directory": configuration.manifest.parent.name,
        "pointer_bytes": base64.b64encode(payload).decode("ascii"),
        "cuda_cache": cache_status,
        "inference_executed": cache_status.get("warmup_executed", False),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--checkout", required=True, type=Path)
    parser.add_argument("--runtime", required=True, type=Path)
    parser.add_argument("--mode", required=True, choices=("stage", "check", "bundle"))
    parser.add_argument("--report-json", type=Path)
    parser.add_argument("--asset-cache", type=Path)
    args = parser.parse_args(argv)
    if args.asset_cache and args.mode != "bundle":
        parser.error("A separate acquisition cache is only supported when preparing a bundle")

    root = physical(args.root)
    checkout, runtime = physical(args.checkout, root), physical(args.runtime, root)
    handles = configure(root, checkout, runtime)
    from flitzis_looper.analysis.beat_cuda_cache import (  # noqa: PLC0415
        cuda_cache_status,
        ensure_cuda_cache,
    )
    from flitzis_looper.analysis.native_provisioning import stage_production_worker  # noqa: PLC0415

    scratch = physical(root / "cache/analysis", root)
    install_root = physical(root / "analysis-runtime/beat-this", root)
    physical(install_root / "current.json", install_root)
    executable = physical(runtime / WORKER_EXECUTABLE, runtime)
    if args.mode == "check":
        configuration = load_verified_selection(install_root, scratch, executable)
        cache_status = cuda_cache_status(configuration)
    else:
        bundle = args.mode == "bundle"
        configuration = stage_production_worker(
            install_root,
            scratch,
            repository=checkout,
            executable=executable,
            crt_dir=checkout / "src/flitzis_looper_audio",
            include_cuda=True if bundle else None,
            asset_cache=physical(args.asset_cache, root.parent) if args.asset_cache else None,
        )
        verify_release_worker(configuration, executable)
        cache_status = (
            {"status": "pending", "detail": "CUDA preparation belongs to the target installation"}
            if bundle
            else ensure_cuda_cache(configuration, retry_failed=True)
        )
        if not bundle and cache_status["status"] not in {"ready", "unavailable"}:
            message = f"CUDA preparation {cache_status['status']}: {cache_status.get('detail', '')}"
            raise ValueError(message)
    result = worker_result(configuration, cache_status)
    if args.report_json is not None:
        write_new_json(physical(args.report_json, root / "cache"), result)
    sys.stdout.write(json.dumps(result, indent=2) + "\n")
    assert handles
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        sys.stderr.write(f"Portable analysis setup failed: {error}\n")
        raise SystemExit(1) from None
