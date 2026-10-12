"""Package an existing Release and installed runtime without installing anything."""

# Standalone build command, not an importable package.
# ruff: noqa: INP001
import argparse
import hashlib
import importlib.metadata
import json
import os
import re
import shutil
import subprocess
import sys
import sysconfig
import tomllib
import uuid
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile

from packaging.requirements import Requirement
from packaging.utils import canonicalize_name

REPO = Path(__file__).resolve().parents[2]
PORTABLE = Path(__file__).resolve().parent
DLL_NAMES = {
    "rubberband-3.dll",
    "samplerate.dll",
    "sleef.dll",
    "sleefdft.dll",
    "directml.dll",
    "msvcp140.dll",
    "msvcp140_1.dll",
    "vcomp140.dll",
    "vcruntime140.dll",
    "vcruntime140_1.dll",
}
LICENSE_NAMES = {
    "antlr-4.9.3.txt",
    "onnxruntime-1.24.2.txt",
    "onnxruntime-1.24.2-notices.txt",
    "directml-1.15.4.html",
    "msvc-2026.docx",
    "rubberband.txt",
    "samplerate.txt",
    "sleef.txt",
    "ffmpeg.txt",
    "ffmpeg-README.txt",
    "source-urls.json",
}
OWNER = {"app": "flitzis-looper", "schema_version": 1}
STARTER = (
    "@echo off\r\nsetlocal EnableExtensions DisableDelayedExpansion\r\n"
    'set "PORTABLE_CHECK="\r\nif /i "%~1"=="--check" set "PORTABLE_CHECK=-Check"\r\n'
    "powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "
    '"%~dp0launch.ps1" -Root "%~dp0." %PORTABLE_CHECK%\r\n'
    'set "PORTABLE_EXIT=%errorlevel%"\r\n'
    'if not "%PORTABLE_EXIT%"=="0" if /i not "%~1"=="--check" pause\r\n'
    "exit /b %PORTABLE_EXIT%\r\n"
)


def require(condition: object, message: str) -> None:
    if not condition:
        raise ValueError(message)


def emit(message: str) -> None:
    sys.stdout.write(message + "\n")
    sys.stdout.flush()


def digest(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def write_json(path: Path, value: object) -> None:
    temporary = path.with_name(path.name + "." + uuid.uuid4().hex + ".tmp")
    with temporary.open("w", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write("\n")
    temporary.replace(path)


def git(*arguments: str) -> str:
    return subprocess.check_output(["git", "-C", str(REPO), *arguments], text=True).strip()


def runtime_distributions() -> list[importlib.metadata.Distribution]:
    """Resolve the installed main dependency closure, including requirement extras."""
    project = tomllib.loads((REPO / "pyproject.toml").read_text(encoding="utf-8"))
    queue = [Requirement(value) for value in project["project"]["dependencies"]]
    visited: set[tuple[str, frozenset[str]]] = set()
    result: dict[str, importlib.metadata.Distribution] = {}
    while queue:
        requirement = queue.pop()
        if requirement.marker and not requirement.marker.evaluate():
            continue
        name = canonicalize_name(requirement.name)
        key = name, frozenset(requirement.extras)
        if key in visited:
            continue
        visited.add(key)
        distribution = importlib.metadata.distribution(requirement.name)
        require(
            requirement.specifier.contains(distribution.version, prereleases=True),
            f"Installed dependency does not satisfy {requirement}",
        )
        result[name] = distribution
        for value in distribution.requires or []:
            dependency = Requirement(value)
            if dependency.marker and not any(
                dependency.marker.evaluate({"extra": extra}) for extra in {"", *requirement.extras}
            ):
                continue
            dependency.marker = None  # Parent extras already decided this marker.
            queue.append(dependency)
    return [result[name] for name in sorted(result)]


def copy_runtime_dependencies(destination: Path) -> dict[str, str]:
    site = Path(sysconfig.get_path("purelib")).resolve()
    distributions = runtime_distributions()
    for distribution in distributions:
        require(distribution.files, f"Missing installed file inventory: {distribution.name}")
        for record in distribution.files or []:
            source = Path(distribution.locate_file(record)).resolve()
            if not source.is_relative_to(site):
                continue  # Console scripts are unused: workers use this Python.
            relative = source.relative_to(site)
            if source.name == "direct_url.json" or "__pycache__" in relative.parts:
                continue
            if source.suffix == ".pth":
                for line in source.read_text(encoding="utf-8").splitlines():
                    if line and not line.startswith(("#", "import ")):
                        require(
                            (site / line).resolve().is_relative_to(site),
                            f"External editable path in {source}",
                        )
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
    return {distribution.metadata["Name"]: distribution.version for distribution in distributions}


def source_identity(args: argparse.Namespace) -> tuple[str, bool]:
    require(
        os.name == "nt" and sys.version_info[:2] == (3, 14) and sys.maxsize > 2**32,
        "Build with the project's Windows x64 CPython 3.14 environment",
    )
    require(
        re.fullmatch(r"[a-f0-9]{40}", args.native_source_commit), "Use a full Native source commit"
    )
    commit = git("rev-parse", "HEAD")
    dirty = bool(git("status", "--porcelain"))
    require(
        not dirty or args.allow_dirty,
        "Commit reviewed source or explicitly build a labelled candidate",
    )
    unchanged = (
        subprocess.run(
            [
                "git",
                "-C",
                str(REPO),
                "diff",
                "--quiet",
                args.native_source_commit,
                "--",
                "rust",
                "src/flitzis_looper_audio",
            ],
            check=False,
        ).returncode
        == 0
    )
    require(unchanged, "Native inputs changed since the supplied Release source commit")
    return commit, dirty


def prepare_output(output: Path) -> Path:
    require(
        not output.resolve().is_relative_to(REPO),
        "Portable bundles and data must stay outside the repository",
    )
    owner = output / "portable-channel-owner.json"
    require(
        not output.exists() or not any(output.iterdir()) or owner.exists(),
        "Nonempty output is not an owned Portable channel",
    )
    output.mkdir(parents=True, exist_ok=True)
    require(
        not owner.exists() or json.loads(owner.read_text(encoding="utf-8")) == OWNER,
        "Unrecognized output owner",
    )
    write_json(owner, OWNER)
    staging = output / ("work-" + uuid.uuid4().hex)
    staging.mkdir()
    return staging


def copy_application(staging: Path, native_wheel: Path) -> Path:
    app = staging / "app"
    app.mkdir()
    with ZipFile(native_wheel) as archive:
        require(
            not any(name.endswith(".pth") for name in archive.namelist()),
            "An editable wheel is not a distributable Native Release",
        )
        for entry in archive.infolist():
            require(
                (app / entry.filename).resolve().is_relative_to(app.resolve()),
                "Native wheel contains an escaping entry",
            )
        archive.extractall(app)
    native = list(app.glob("flitzis_looper_audio/*.pyd"))
    require(len(native) == 1, "Native wheel must contain exactly one Windows extension")
    for relative in git("ls-files", "--", "src/flitzis_looper").splitlines():
        target = app / Path(relative).relative_to("src")
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(REPO / relative, target)
    shutil.copyfile(REPO / "assets/models/keynet.onnx", staging / "python/keynet.onnx")
    for name in ["bootstrap.py", "launch.ps1", "dispatch.ps1", "common.ps1", "build-path.ps1"]:
        shutil.copyfile(PORTABLE / name, staging / name)
    (staging / "START-LOOPER.bat").write_bytes(STARTER.encode("ascii"))
    return native[0]


def copy_external_runtime(staging: Path, args: argparse.Namespace) -> None:
    dll_files = {
        path.name.lower(): path
        for directory in args.dll_dir
        for path in directory.iterdir()
        if path.name.lower() in DLL_NAMES
    }
    require(
        not DLL_NAMES - dll_files.keys(),
        f"Required redistributable DLLs missing: {sorted(DLL_NAMES - dll_files.keys())}",
    )
    for name, source in dll_files.items():
        shutil.copyfile(source, staging / "app/flitzis_looper_audio" / name)
        if name.startswith("vcruntime"):
            shutil.copyfile(source, staging / "python" / name)
    ffmpeg = staging / "runtime/ffmpeg"
    ffmpeg.mkdir(parents=True)
    for path in args.ffmpeg_dir.iterdir():
        if path.suffix.lower() == ".dll" or path.name.lower() in {"ffmpeg.exe", "ffprobe.exe"}:
            shutil.copyfile(path, ffmpeg / path.name)


def copy_licenses(staging: Path, values: list[str]) -> None:
    licenses = staging / "licenses"
    licenses.mkdir()
    shutil.copyfile(REPO / "LICENSE.txt", licenses / "flitzis-looper.txt")
    shutil.copyfile(REPO / "vendor/bs_roformer/NOTICE.md", licenses / "bs-roformer-NOTICE.md")
    for value in values:
        name, path = value.split("=", 1)
        require(Path(name).name == name, "License names must be plain filenames")
        shutil.copyfile(path, licenses / name)
    require(
        not LICENSE_NAMES - {path.name for path in licenses.iterdir()},
        "Required external license texts missing",
    )


def preflight(staging: Path, output: Path) -> None:
    check_root = output / ("check-" + uuid.uuid4().hex)
    for name in ["data", "models", "cache/temp"]:
        (check_root / name).mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    for name in [
        "PYTHONPATH",
        "VIRTUAL_ENV",
        "VCPKG_ROOT",
        "RUBBERBAND_DLL_DIR",
        "RUBBERBAND_DLL_DIRS",
    ]:
        env.pop(name, None)
    env["PYTHONHOME"] = str(staging / "python")
    env["PYTHONNOUSERSITE"] = "1"
    env["PATH"] = os.pathsep.join([
        str(staging / "python"),
        str(Path(os.environ["SYSTEMROOT"]) / "System32"),
    ])
    subprocess.run(
        [
            str(staging / "python/python.exe"),
            "-s",
            "-B",
            str(staging / "bootstrap.py"),
            "--root",
            str(check_root),
            "--check",
        ],
        env=env,
        check=True,
    )


def package_runtime(staging: Path, output: Path, release: dict[str, object]) -> None:
    write_json(staging / "release-manifest.json", release)
    package = output / (str(release["release_id"]) + ".zip")
    require(not package.exists(), "An immutable package already exists for these inputs")
    emit(f"Compressing runtime: {package}")
    with ZipFile(package, "x", ZIP_DEFLATED, compresslevel=1) as archive:
        for path in sorted(staging.rglob("*")):
            if path.is_file():
                archive.write(path, path.relative_to(staging).as_posix())
    channel = {
        "schema_version": 1,
        "release_id": release["release_id"],
        "git_commit": release["git_commit"],
        "package": package.name,
        "package_sha256": digest(package),
        "package_size": package.stat().st_size,
    }
    manifest = output / "portable-manifest.json"
    write_json(manifest, channel)
    personal = (REPO / "INSTALL-LOOPER-PORTABLE.bat").read_text(encoding="utf-8-sig")
    installer = output / "INSTALL-LOOPER-PORTABLE.bat"
    installer.write_bytes(personal.replace("\r\n", "\n").replace("\n", "\r\n").encode("utf-8"))
    emit(json.dumps({"channel": str(manifest), "installer": str(installer), **channel}, indent=2))


def build(args: argparse.Namespace) -> None:
    commit, dirty = source_identity(args)
    # Keep an owned short build-drive path for Windows DLL loading. Resolve only
    # for containment comparisons; resolving operational paths defeats SUBST.
    output = args.output.absolute()
    staging = prepare_output(output)
    emit(f"Preparing private runtime: {staging}")
    shutil.copytree(
        Path(sys.base_prefix),
        staging / "python",
        ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "site-packages"),
    )
    dependencies = copy_runtime_dependencies(staging / "python/Lib/site-packages")
    shutil.copyfile(
        PORTABLE / "sitecustomize.py", staging / "python/Lib/site-packages/sitecustomize.py"
    )
    emit(f"Copied {len(dependencies)} non-development distributions")
    native = copy_application(staging, args.native_wheel)
    copy_external_runtime(staging, args)
    copy_licenses(staging, args.license_file)
    preflight(staging, output)
    if args.runtime_directory:
        destination = args.runtime_directory.absolute()
        require(not destination.exists(), "Runtime destination must be a fresh directory")
        require(
            not destination.resolve().is_relative_to(REPO),
            "Runtime must stay outside source checkout",
        )
        destination.parent.mkdir(parents=True, exist_ok=True)
        staging.rename(destination)
        write_json(
            destination / "release-manifest.json",
            {
                "schema_version": 1,
                "release_id": commit,
                "git_commit": commit,
                "source_dirty": dirty,
                "native_source_commit": args.native_source_commit,
                "native_sha256": digest(destination / native.relative_to(staging)),
                "runtime_dependencies": dependencies,
                "python_version": sys.version.split()[0],
            },
        )
        emit(json.dumps({"runtime_directory": str(destination), "git_commit": commit}))
        return
    fingerprint = hashlib.sha256()
    for path in sorted(staging.rglob("*")):
        if path.is_file():
            fingerprint.update(path.relative_to(staging).as_posix().encode("utf-8"))
            fingerprint.update(bytes.fromhex(digest(path)))
    package_runtime(
        staging,
        output,
        {
            "schema_version": 1,
            "release_id": commit[:12] + "-" + fingerprint.hexdigest()[:12],
            "git_commit": commit,
            "source_dirty": dirty,
            "native_source_commit": args.native_source_commit,
            "native_sha256": digest(native),
            "runtime_dependencies": dependencies,
            "python_version": sys.version.split()[0],
            "source_fingerprint": fingerprint.hexdigest(),
        },
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-wheel", type=Path, required=True)
    parser.add_argument("--native-source-commit", required=True)
    parser.add_argument("--ffmpeg-dir", type=Path, required=True)
    parser.add_argument("--dll-dir", type=Path, action="append", default=[])
    parser.add_argument("--license-file", action="append", default=[])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--allow-dirty", action="store_true")
    parser.add_argument("--runtime-directory", type=Path)
    build(parser.parse_args())


if __name__ == "__main__":
    main()
