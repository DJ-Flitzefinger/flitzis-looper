"""Prepare/check an installation-owned, movable BeatThis runtime without inference."""

import argparse
import base64
import hashlib
import importlib.util
import json
import os
import re
import shutil
import stat
import subprocess
import sys
from pathlib import Path
from uuid import uuid4


def invalid(message):
    raise ValueError(message)


def physical(path: Path, root: Path | None = None) -> Path:
    path = Path(os.path.abspath(path))
    for part in reversed((path, *path.parents)):
        if part.exists() or part.is_symlink():
            info = part.lstat()
            if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
                invalid("Portable analysis paths must not traverse links")
    if root is not None:

        def canonical(value):
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


def read_json(path: Path) -> dict:
    physical(path)
    if path.stat().st_size > 4 * 1024 * 1024:
        invalid("Portable analysis metadata exceeds its bound")
    return json.loads(path.read_text(encoding="utf-8-sig"))


def write_new_json(path: Path, value: dict) -> None:
    physical(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")


def clean_environment(root: Path) -> dict:
    environment = dict(os.environ)
    for name in tuple(environment):
        if name.startswith(("PYTHON", "FLITZIS_WORKSPACE", "UV_")) or name in {
            "VIRTUAL_ENV",
            "FLITZIS_NATIVE_EXTENSION_DIR",
            "FLITZIS_BEAT_THIS_ROOT",
        }:
            environment.pop(name, None)
    environment.update(
        PYTHONDONTWRITEBYTECODE="1",
        PYTHONNOUSERSITE="1",
        UV_CACHE_DIR=str(root / "cache/uv"),
        UV_PYTHON_INSTALL_DIR=str(root / "tools/python"),
        UV_TOOL_DIR=str(root / "tools/uv-tools"),
        UV_LINK_MODE="copy",
        UV_PYTHON_PREFERENCE="only-managed",
        UV_NO_PROGRESS="1",
        TEMP=str(root / "cache/temp"),
        TMP=str(root / "cache/temp"),
        TORCH_HOME=str(root / "models/torch"),
        HF_HOME=str(root / "models/huggingface"),
        HF_HUB_CACHE=str(root / "models/huggingface/hub"),
    )
    return environment


def inspect_python(interpreter: Path, root: Path) -> dict:
    """Inspect real import origins; an executable's name is not portability proof."""
    code = (
        "import json,pathlib,sys,ssl,sqlite3,ctypes; "
        "print(json.dumps({'version':list(sys.version_info[:3]), 'executable':sys.executable, "
        "'prefix':sys.prefix, 'base_prefix':sys.base_prefix, 'paths':sys.path}))"
    )
    process = subprocess.run(
        [str(interpreter), "-I", "-B", "-c", code],
        env=clean_environment(root),
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    )
    value = json.loads(process.stdout)
    if value["version"] != [3, 12, 13]:
        invalid("Portable analysis requires CPython 3.12.13")
    for name in ("executable", "prefix", "base_prefix"):
        physical(Path(value[name]), root)
    for entry in value["paths"]:
        if entry:
            physical(Path(entry), root)
    return value


def materialize_python(base: Path, environment: Path, root: Path) -> None:
    """Replace this owned venv redirector with real Python and relative search paths.

    CPython's documented ._pth layout ignores registry/environment discovery.
    No host home/executable or staging path survives in pyvenv.cfg.
    """
    base, environment = physical(base, root), physical(environment, root)
    base_info = inspect_python(base, root)
    source = physical(Path(base_info["base_prefix"]), root)
    copied = []
    for current, directories, files in os.walk(source, followlinks=False):
        directories[:] = sorted(
            name
            for name in directories
            if name
            not in {
                "site-packages",
                "__pycache__",
                "Scripts",
                "include",
                "libs",
                "tcl",
                "Doc",
            }
        )
        for name in directories:
            physical(Path(current) / name, source)
        relative = Path(current).relative_to(source)
        for name in sorted(files):
            original = physical(Path(current) / name, source)
            if original.suffix in {".pyc", ".pyo", ".pdb"} or name.endswith("._pth"):
                continue
            destination = physical(environment / relative / name, environment)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(original, destination)
            copied.append(destination)
    scripts = environment / "Scripts"
    scripts.mkdir(exist_ok=True)
    for original in [source / "python.exe", *source.glob("*.dll")]:
        destination = scripts / original.name
        shutil.copyfile(physical(original, source), physical(destination, environment))
        copied.append(destination)
    search = "../Lib\n../DLLs\n../Lib/site-packages\nimport site\n"
    for name in ("python312._pth", "python._pth"):
        path = scripts / name
        path.write_text(search, encoding="ascii", newline="\n")
        copied.append(path)
    configuration = environment / "pyvenv.cfg"
    configuration.write_text(
        "include-system-site-packages = false\n", encoding="ascii", newline="\n"
    )
    copied.append(configuration)
    inventory = {path.relative_to(environment).as_posix(): digest(path) for path in copied}
    write_new_json(
        environment / "portable-python.json",
        {
            "schema_version": 1,
            "python_version": "3.12.13",
            "files": inventory,
        },
    )
    inspect_python(scripts / "python.exe", environment)


def verify_python(environment: Path, root: Path) -> dict:
    environment = physical(environment, root)
    inventory = read_json(environment / "portable-python.json")
    if inventory.get("schema_version") != 1 or inventory.get("python_version") != "3.12.13":
        invalid("Unknown Portable analysis interpreter inventory")
    files = inventory.get("files")
    required = {
        "Scripts/python.exe",
        "Scripts/python312.dll",
        "Scripts/python312._pth",
        "pyvenv.cfg",
    }
    if not isinstance(files, dict) or not required <= files.keys() or len(files) > 20000:
        invalid("Incomplete Portable analysis interpreter closure")
    for name, expected in files.items():
        path = Path(name)
        if (
            path.is_absolute()
            or ".." in path.parts
            or "\\" in name
            or not re.fullmatch(r"[0-9a-f]{64}", expected)
        ):
            invalid("Invalid Portable analysis interpreter inventory path")
        if digest(physical(environment / path, environment)) != expected:
            invalid("Portable analysis interpreter bytes changed")
    return inspect_python(environment / "Scripts/python.exe", environment)


def configure(root: Path, checkout: Path, runtime: Path):
    specification = importlib.util.spec_from_file_location(
        "portable_analysis_bootstrap", checkout / "scripts/portable/bootstrap.py"
    )
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module.configure(root, checkout, runtime)


def verified_installation(root: Path, installation: Path, setup, scratch: Path):
    configuration = setup.load_staged_worker_configuration(
        root / "analysis-runtime/beat-this", installation, scratch
    )
    interpreter = verify_python(installation / ".venv", root)
    report = scratch / ("worker-environment-" + uuid4().hex + ".json")
    subprocess.run(
        [
            str(configuration.interpreter),
            "-I",
            "-B",
            str(configuration.script),
            "--environment-manifest",
            str(report),
        ],
        env=clean_environment(root),
        cwd=installation,
        check=True,
    )
    manifest = setup.read_installed_manifest(configuration.manifest)
    observed = setup.artifacts.ENVIRONMENT_ADAPTER.validate_json(report.read_bytes(), strict=True)
    if observed != manifest.environment:
        invalid("Portable worker's actual environment differs from its receipt")
    return configuration, interpreter


def selected_profile(install_root, setup):
    try:
        pointer = read_json(install_root / "current.json")
        if not re.fullmatch(r"installation-[0-9a-f]{32}", pointer["directory"]):
            invalid("Invalid selected worker directory")
        manifest = physical(install_root / pointer["directory"] / "manifest.json", install_root)
        if digest(manifest) != pointer["manifest_sha256"]:
            invalid("Selected worker manifest changed")
        return getattr(setup.read_installed_manifest(manifest), "profile", "legacy-cpu-v1")
    except OSError, ValueError, KeyError:
        return "legacy-cpu-v1"


def provision_environment(installation, uv, base, root):
    environment = clean_environment(root)
    environment["UV_PROJECT_ENVIRONMENT"] = str(installation / ".venv")
    subprocess.run(
        [
            str(uv),
            "sync",
            "--project",
            str(installation),
            "--python",
            str(base),
            "--locked",
            "--no-dev",
            "--no-editable",
        ],
        cwd=installation,
        env=environment,
        check=True,
    )
    materialize_python(base, installation / ".venv", root)


def stage_installation(args, root, checkout, setup, scratch, profile):
    if any(value is None for value in (args.uv, args.base_python, args.checkpoint)):
        invalid("Worker repair requires its private uv/Python and pinned checkpoint")
    uv, base, checkpoint = (
        physical(value, root) for value in (args.uv, args.base_python, args.checkpoint)
    )
    artifact = setup.artifacts.ACCEPTED_ARTIFACT
    if checkpoint.stat().st_size != artifact.size_bytes or digest(checkpoint) != artifact.sha256:
        invalid("Pinned Portable BeatThis checkpoint mismatch")
    installation = root / "analysis-runtime/beat-this" / ("installation-" + uuid4().hex)
    installation.mkdir(parents=True)
    source = (
        setup.cuda_source_directory()
        if profile == "cuda-auto-v2"
        else setup.artifacts.worker_source_directory()
    )
    names = setup.CUDA_WORKER_FILES if profile == "cuda-auto-v2" else setup.artifacts.WORKER_FILES
    for name in names:
        shutil.copyfile(physical(source / name, checkout), installation / name)
    shutil.copyfile(checkpoint, installation / "final0.ckpt")
    provision_environment(installation, uv, base, root)
    with (scratch / ("inspect-" + uuid4().hex + ".log")).open("xb") as log:
        setup._inspect_environment(installation, log)
    if profile == "cuda-auto-v2":
        setup._record_cuda_installation(installation)
    else:
        setup._record_installation(installation)
    return verified_installation(root, installation, setup, scratch)


def parse_arguments(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--checkout", required=True, type=Path)
    parser.add_argument("--runtime", required=True, type=Path)
    parser.add_argument("--mode", required=True, choices=("metadata", "stage", "check"))
    parser.add_argument("--report-json", type=Path)
    parser.add_argument("--uv", type=Path)
    parser.add_argument("--base-python", type=Path)
    parser.add_argument("--checkpoint", type=Path)
    return parser.parse_args(argv)


def worker_result(configuration, profile, interpreter):
    pointer = {
        "schema_version": 1,
        "directory": configuration.manifest.parent.name,
        "manifest_sha256": digest(configuration.manifest),
    }
    payload = (json.dumps(pointer, indent=2) + "\n").encode("utf-8")
    return {
        "portable_analysis": "passed",
        "profile": profile,
        "directory": configuration.manifest.parent.name,
        "pointer_bytes": base64.b64encode(payload).decode("ascii"),
        "interpreter": interpreter,
        "inference_executed": False,
    }


def main(argv=None) -> int:
    args = parse_arguments(argv)
    root = physical(args.root)
    checkout, runtime = physical(args.checkout, root), physical(args.runtime, root)
    handles = configure(root, checkout, runtime)
    from flitzis_looper.analysis import setup  # noqa: PLC0415

    if args.mode == "metadata":
        sys.stdout.write(
            json.dumps({
                "url": setup.artifacts.ACCEPTED_ARTIFACT.source_url,
                "sha256": setup.artifacts.ACCEPTED_ARTIFACT.sha256,
                "bytes": setup.artifacts.ACCEPTED_ARTIFACT.size_bytes,
                "python": "3.12.13",
            })
            + "\n"
        )
        return 0
    scratch = physical(root / "cache/analysis", root)
    scratch.mkdir(parents=True, exist_ok=True)
    install_root = physical(root / "analysis-runtime/beat-this", root)
    profile = selected_profile(install_root, setup)
    try:
        current = setup.load_worker_configuration(install_root, scratch)
        configuration, interpreter = verified_installation(
            root, current.manifest.parent, setup, scratch
        )
    except OSError, ValueError, RuntimeError, subprocess.SubprocessError:
        if args.mode == "check":
            raise
        configuration, interpreter = stage_installation(
            args, root, checkout, setup, scratch, profile
        )
    result = worker_result(configuration, profile, interpreter)
    if args.report_json is not None:
        write_new_json(physical(args.report_json, root / "cache"), result)
    sys.stdout.write(json.dumps(result, indent=2) + "\n")
    assert handles is not None
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        sys.stderr.write(f"Portable analysis setup failed: {error}\n")
        raise SystemExit(1) from None
