"""Repair pinned Portable models and verify both in fresh CPU processes."""

import argparse
import importlib
import importlib.util
import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Literal, cast

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper.models import StemSeparator

type ModelMode = Literal["repair", "check"]
type ModelAction = Literal["prepare", "load"]


@dataclass(frozen=True, slots=True)
class ModelContext:
    root: Path
    checkout: Path
    runtime: Path | None
    mode: ModelMode


class ModelCheckError(RuntimeError):
    __slots__ = ("separator", "stage")

    def __init__(self, message: str, *, separator: str, stage: str) -> None:
        super().__init__(message)
        self.separator = separator
        self.stage = stage


def configure(context: ModelContext) -> list[object]:
    """Use this checkout's bootstrap before any application or Torch import."""
    path = context.checkout / "scripts" / "portable" / "bootstrap.py"
    spec = importlib.util.spec_from_file_location("portable_model_bootstrap", path)
    if spec is None or spec.loader is None:
        msg = "Portable checkout bootstrap is unavailable"
        raise RuntimeError(msg)
    bootstrap = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = bootstrap
    spec.loader.exec_module(bootstrap)
    return cast(
        "list[object]", bootstrap.configure(context.root, context.checkout, context.runtime)
    )


def model_cache_dirs(context: ModelContext) -> dict[StemSeparator, Path]:
    """Resolve the same two caches used by the application's offline workers."""
    models = importlib.import_module("flitzis_looper.models")
    separators = importlib.import_module("flitzis_looper.controller.stem_separators")
    result: dict[StemSeparator, Path] = {}
    for separator in models.STEM_SEPARATORS:
        cache = separators.separator_model_cache_dir(
            separator, env=dict(os.environ), project_root=context.checkout
        ).resolve()
        if not cache.is_relative_to(context.root / "models") or cache.is_relative_to(
            context.checkout
        ):
            msg = f"Portable model cache leaves the owned sibling models directory: {cache}"
            raise RuntimeError(msg)
        result[separator] = cache
    return result


def demucs_checkpoints(cache: Path) -> Iterator[tuple[Path, str]]:
    """Take required filenames and their checksum suffixes from the existing table."""
    generation = importlib.import_module("flitzis_looper.controller.stem_generation")
    for filename in generation.DEMUCS_REQUIRED_CHECKPOINTS["htdemucs"]:
        path = cache / filename
        if not path.resolve().is_relative_to(cache.resolve()):
            msg = f"Demucs checkpoint leaves its owned cache: {path}"
            raise RuntimeError(msg)
        yield path, Path(filename).stem.rsplit("-", 1)[1]


def verify_demucs(cache: Path) -> None:
    """Rehash cache hits, which Demucs' remote downloader otherwise trusts."""
    repo = importlib.import_module("demucs.repo")
    for path, checksum in demucs_checkpoints(cache):
        if not path.is_file():
            msg = f"no Model installed: demucs:htdemucs ({path.name})"
            raise RuntimeError(msg)
        repo.check_checksum(path, checksum)


def repair_demucs(cache: Path) -> None:
    """Let the locked Demucs downloader replace only absent or corrupt required files."""
    repo = importlib.import_module("demucs.repo")
    cache.mkdir(parents=True, exist_ok=True)
    needs_download = False
    for path, checksum in demucs_checkpoints(cache):
        if not path.exists():
            needs_download = True
            continue
        try:
            repo.check_checksum(path, checksum)
        except repo.ModelLoadingError:
            path.unlink()
            needs_download = True
    if needs_download:
        importlib.import_module("demucs.pretrained").get_model("htdemucs")
    verify_demucs(cache)


def operate_model(separator: StemSeparator, cache: Path, action: ModelAction) -> None:
    """Prepare assets or load the exact selected model on CPU without inference."""
    if separator == "demucs:htdemucs":
        if action == "prepare":
            repair_demucs(cache)
        else:
            verify_demucs(cache)
            importlib.import_module("demucs.pretrained").get_model("htdemucs").to("cpu")
        return
    if separator != "bs-roformer:musdb18hq":
        msg = f"Unsupported stem separator: {separator}"
        raise ValueError(msg)
    assets = importlib.import_module("flitzis_looper.controller.bs_roformer_assets")
    if action == "prepare":
        assets.install_model_assets(cache)
        return
    assets.verify_model_assets(cache)
    importlib.import_module("flitzis_bs_roformer.model").load_model(
        cache / assets.CHECKPOINT_FILENAME, cache / assets.CONFIG_FILENAME, "cpu"
    )


def run_model_child(context: ModelContext, separator: StemSeparator, action: ModelAction) -> None:
    """Wait for one fresh private process to exit before starting another model."""
    command = [
        sys.executable,
        "-s",
        "-B",
        str(context.checkout / "scripts" / "portable" / "prepare_models.py"),
        "--root",
        str(context.root),
        "--checkout",
        str(context.checkout),
        "--mode",
        context.mode,
        "--child-action",
        action,
        "--separator",
        separator,
    ]
    if context.runtime is not None:
        command.extend(["--runtime", str(context.runtime)])
    result = subprocess.run(
        command,
        cwd=context.checkout,
        env=dict(os.environ),
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        diagnostic = result.stderr.strip() or result.stdout.strip() or "model child failed"
        raise ModelCheckError(diagnostic, separator=separator, stage=action)


def check_models(context: ModelContext) -> dict[str, object]:
    """Finish repairs before the two final fresh, sequential CPU load checks."""
    caches = model_cache_dirs(context)
    if context.mode == "repair":
        for separator in caches:
            run_model_child(context, separator, "prepare")
    for separator in caches:
        run_model_child(context, separator, "load")
    return {
        "portable_models": "passed",
        "mode": context.mode,
        "device": "cpu",
        "models": {separator: str(cache) for separator, cache in caches.items()},
    }


def write_report(context: ModelContext, report: dict[str, object], path: Path | None) -> None:
    payload = json.dumps(report, sort_keys=True) + "\n"
    if path is not None:
        path = path.resolve()
        if not path.is_relative_to(context.root / "cache"):
            msg = "Portable model report must stay inside the owned Root/cache directory"
            raise ValueError(msg)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(payload, encoding="utf-8")
    sys.stdout.write(payload)
    sys.stdout.flush()


def run_internal_child(context: ModelContext, separator: str, action: ModelAction) -> None:
    caches = model_cache_dirs(context)
    if separator not in caches or (action == "prepare" and context.mode != "repair"):
        msg = "Invalid Portable model child action"
        raise ValueError(msg)
    selected = cast("StemSeparator", separator)
    operate_model(selected, caches[selected], action)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--checkout", type=Path, required=True)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--mode", choices=("repair", "check"), required=True)
    parser.add_argument("--report-json", type=Path)
    parser.add_argument("--child-action", choices=("prepare", "load"), help=argparse.SUPPRESS)
    parser.add_argument("--separator", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    context = ModelContext(
        args.root.resolve(),
        args.checkout.resolve(),
        None if args.runtime is None else args.runtime.resolve(),
        args.mode,
    )
    dll_handles: list[object] = []
    try:
        dll_handles = configure(context)
        if args.child_action is not None:
            run_internal_child(context, args.separator, args.child_action)
            report: dict[str, object] = {
                "portable_models": "passed",
                "mode": context.mode,
                "stage": args.child_action,
                "separator": args.separator,
            }
        else:
            report = check_models(context)
    except ModelCheckError as error:
        report = {
            "portable_models": "failed",
            "mode": context.mode,
            "stage": error.stage,
            "separator": error.separator,
            "error": str(error),
        }
    except (OSError, RuntimeError, ImportError, ValueError) as error:
        report = {
            "portable_models": "failed",
            "mode": context.mode,
            "stage": "configuration" if args.child_action is None else args.child_action,
            "error": str(error),
        }
    finally:
        del dll_handles
    write_report(context, report, args.report_json)
    return 0 if report["portable_models"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
