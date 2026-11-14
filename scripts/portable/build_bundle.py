"""Package a Release runtime and the matching BeatThis worker."""

# Standalone build command.
import argparse
import base64
import csv
import hashlib
import importlib.metadata
import io
import json
import os
import re
import shutil
import subprocess
import sys
import sysconfig
import tomllib
import uuid
from email.parser import BytesParser
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile

from packaging.requirements import Requirement
from packaging.utils import canonicalize_name, parse_wheel_filename

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
CANDIDATE_OWNER = {"app": "flitzis-looper-candidate", "schema_version": 1}
NATIVE_NAME = "flitzis_looper_audio.cp314-win_amd64.pyd"
NATIVE_ENTRY = "flitzis_looper_audio/" + NATIVE_NAME
WHEEL_TAG = "cp314-cp314-win_amd64"
BEAT_WORKER_EXECUTABLE = Path("runtime/beat-this/native-beat-this-worker.exe")
VENDORED_PACKAGE = Path("flitzis_bs_roformer")
VENDORED_SOURCE = Path("vendor/bs_roformer/src") / VENDORED_PACKAGE
NATIVE_SOURCE_RECORD = "windows-redistribution-sources.json"
RUNTIME_REDISTRIBUTION_RECORD = "windows-runtime-redistribution.json"
# Keep these literals readable by the legacy inventory parser.
TRACKED_LICENSES = {
    "flitzis-looper.txt": "LICENSE.txt",
    "bs-roformer-NOTICE.md": "vendor/bs_roformer/NOTICE.md",
    "asio-sdk-LICENSE.txt": "rust/crates/looper/vendor/asio-sdk/LICENSE.txt",
    "asio-sdk-README.md": "rust/crates/looper/vendor/asio-sdk/README.md",
    "asio-sdk-provenance.json": "rust/crates/looper/vendor/asio-sdk/provenance.json",
    "cpal-LICENSE.txt": "rust/vendor/cpal/LICENSE",
    "cpal-provenance.json": "rust/vendor/cpal/looper-provenance.json",
    "windows-redistribution-sources.json": "scripts/native/windows-redistribution-sources.json",
    "windows-runtime-redistribution.json": "scripts/portable/windows-runtime-redistribution.json",
}
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
    return subprocess.check_output(["git", "-C", str(REPO), *arguments], encoding="utf-8").strip()


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


def check_vendored_source(distribution: importlib.metadata.Distribution) -> None:
    """Reject a same-version vendored wheel built from different source."""
    tracked = {
        Path(name).relative_to(VENDORED_SOURCE)
        for name in git("ls-files", "-z", "--", VENDORED_SOURCE.as_posix()).split("\0")
        if name
    }
    packaged = {
        Path(record).relative_to(VENDORED_PACKAGE): record
        for record in distribution.files or []
        if Path(record).is_relative_to(VENDORED_PACKAGE) and "__pycache__" not in record.parts
    }
    require(
        tracked and packaged.keys() == tracked, "BS-RoFormer wheel file list differs from source"
    )
    for path, record in sorted(packaged.items()):
        installed = Path(distribution.locate_file(record)).read_bytes()
        require(
            installed == (REPO / VENDORED_SOURCE / path).read_bytes(),
            f"BS-RoFormer wheel differs from tracked source: {path.as_posix()}",
        )
        checksum = base64.urlsafe_b64encode(hashlib.sha256(installed).digest()).decode().rstrip("=")
        require(
            record.size == len(installed)
            and record.hash is not None
            and record.hash.mode == "sha256"
            and record.hash.value == checksum,
            f"BS-RoFormer wheel RECORD differs from source: {path.as_posix()}",
        )


def copy_runtime_dependencies(destination: Path) -> dict[str, str]:
    site = Path(sysconfig.get_path("purelib")).resolve()
    distributions = runtime_distributions()
    for distribution in distributions:
        if canonicalize_name(distribution.name) == "flitzis-bs-roformer":
            check_vendored_source(distribution)
    for distribution in distributions:
        copy_distribution(distribution, site, destination)
    return {distribution.metadata["Name"]: distribution.version for distribution in distributions}


def copy_distribution(
    distribution: importlib.metadata.Distribution, site: Path, destination: Path
) -> None:
    require(distribution.files, f"Missing installed file inventory: {distribution.name}")
    distribution_site = Path(distribution.locate_file("")).resolve()
    vendor = canonicalize_name(distribution.name) == "flitzis-bs-roformer"
    require(
        distribution_site == site or vendor,
        f"Distribution is outside the build environment: {distribution.name}",
    )
    vendor_metadata = Path(f"flitzis_bs_roformer-{distribution.version}.dist-info")
    for record in distribution.files or []:
        source = Path(distribution.locate_file(record)).resolve()
        if not source.is_relative_to(distribution_site):
            continue  # Console scripts are unused: workers use this Python.
        relative = source.relative_to(distribution_site)
        if vendor:
            require(
                relative == Path(record)
                and (
                    relative.is_relative_to(VENDORED_PACKAGE)
                    or relative.is_relative_to(vendor_metadata)
                ),
                f"Unexpected BS-RoFormer wheel entry: {record}",
            )
        if source.name == "direct_url.json" or "__pycache__" in relative.parts:
            continue
        if source.suffix == ".pth":
            for line in source.read_text(encoding="utf-8").splitlines():
                if line and not line.startswith(("#", "import ")):
                    require(
                        (distribution_site / line).resolve().is_relative_to(distribution_site),
                        f"External editable path in {source}",
                    )
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)


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
                "scripts/native/vcpkg-overlay",
            ],
            check=False,
        ).returncode
        == 0
    )
    require(unchanged, "Native inputs changed since the supplied Release source commit")
    require(
        not git("ls-files", "--others", "--", "scripts/native/vcpkg-overlay"),
        "Commit Native dependency overlay inputs before packaging",
    )
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


def read_native_wheel(native_wheel: Path) -> tuple[bytes, str]:
    """Verify the exact package and recorded extension before copying it."""
    name, version, _, tags = parse_wheel_filename(native_wheel.name)
    require(
        name == "flitzis-looper" and {str(tag) for tag in tags} == {WHEEL_TAG},
        "Native wheel must be the flitzis-looper Windows x64 CPython 3.14 wheel",
    )
    dist_info = f"flitzis_looper-{version}.dist-info"
    with native_wheel.open("rb") as wheel:
        wheel_sha256 = hashlib.file_digest(wheel, "sha256").hexdigest()
        wheel.seek(0)
        with ZipFile(wheel) as archive:
            names = archive.namelist()
            require(
                len({name.casefold() for name in names}) == len(names),
                "Native wheel contains duplicate entries",
            )
            require(
                not any(name.casefold().endswith(".pth") for name in names),
                "An editable wheel is not a distributable Native Release",
            )
            require(
                [name for name in names if name.casefold().endswith(".pyd")] == [NATIVE_ENTRY]
                and {f"{dist_info}/{name}" for name in ("WHEEL", "METADATA", "RECORD")}
                <= set(names),
                "Native wheel is missing its exact extension or distribution metadata",
            )
            for name in names:
                entry = Path(name)
                require(
                    not entry.anchor
                    and ".." not in entry.parts
                    and not any(":" in part for part in entry.parts),
                    "Native wheel contains an escaping entry",
                )
            parser = BytesParser()
            wheel_metadata = parser.parsebytes(archive.read(f"{dist_info}/WHEEL"))
            metadata = parser.parsebytes(archive.read(f"{dist_info}/METADATA"))
            require(
                wheel_metadata.get_all("Tag") == [WHEEL_TAG]
                and wheel_metadata["Root-Is-Purelib"] == "false"
                and metadata["Name"] is not None
                and canonicalize_name(metadata["Name"]) == "flitzis-looper"
                and metadata["Version"] == str(version),
                "Native wheel metadata does not match its project and ABI",
            )
            binary = archive.read(NATIVE_ENTRY)
            record = csv.reader(io.StringIO(archive.read(f"{dist_info}/RECORD").decode("utf-8")))
            extension_rows = [row for row in record if row and row[0] == NATIVE_ENTRY]
    hash_value = base64.urlsafe_b64encode(hashlib.sha256(binary).digest()).decode().rstrip("=")
    require(
        extension_rows == [[NATIVE_ENTRY, "sha256=" + hash_value, str(len(binary))]],
        "Native wheel RECORD does not match the extension bytes",
    )
    return binary, wheel_sha256


def copy_application(staging: Path, checkout: Path, native_wheel: Path) -> tuple[Path, str]:
    """Place only Native outputs in the complete checkout; preserve tracked wrappers."""
    native_directory = checkout / "src/flitzis_looper_audio"
    require(native_directory.is_dir(), "Complete checkout lacks the Native Python package")
    binary, wheel_sha256 = read_native_wheel(native_wheel)
    native = native_directory / NATIVE_NAME
    with native.open("xb") as target:
        target.write(binary)
    for name in [
        "bootstrap.py",
        "launch.ps1",
        "dispatch.ps1",
        "common.ps1",
        "build-path.ps1",
        "prepare_analysis.py",
    ]:
        shutil.copyfile(PORTABLE / name, staging / name)
    (staging / "START-LOOPER.bat").write_bytes(STARTER.encode("ascii"))
    return native, wheel_sha256


def copy_external_runtime(staging: Path, checkout: Path, args: argparse.Namespace) -> None:
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
        shutil.copyfile(source, checkout / "src/flitzis_looper_audio" / name)
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
    tracked_names = {name.casefold() for name in TRACKED_LICENSES}
    for name, relative in TRACKED_LICENSES.items():
        shutil.copyfile(REPO / relative, licenses / name)
    for value in values:
        name, path = value.split("=", 1)
        require(
            Path(name).name == name
            and name
            and not any(character in name for character in ":/\\")
            and name == name.rstrip(" ."),
            "License names must be plain filenames",
        )
        target = licenses / name
        require(
            name.casefold() not in tracked_names
            and not (
                target.exists()
                and any(target.samefile(licenses / tracked) for tracked in TRACKED_LICENSES)
            ),
            "External license must not overwrite a tracked license or provenance file",
        )
        shutil.copyfile(path, target)
    require(
        not LICENSE_NAMES - {path.name for path in licenses.iterdir()},
        "Required external license texts missing",
    )


def private_python_environment(runtime: Path) -> dict[str, str]:
    env = os.environ.copy()
    for name in [
        "PYTHONPATH",
        "VIRTUAL_ENV",
        "VCPKG_ROOT",
        "RUBBERBAND_DLL_DIR",
        "RUBBERBAND_DLL_DIRS",
    ]:
        env.pop(name, None)
    env["PYTHONHOME"] = str(runtime / "python")
    env["PYTHONNOUSERSITE"] = "1"
    env["PATH"] = os.pathsep.join([
        str(runtime / "python"),
        str(Path(os.environ["SYSTEMROOT"]) / "System32"),
    ])
    return env


def copy_beat_worker(runtime: Path, executable: Path) -> None:
    require(
        executable.is_file()
        and not any(
            path.is_symlink() or path.is_junction() for path in (executable, *executable.parents)
        )
        and 0 < executable.stat().st_size <= 64 * 1024 * 1024,
        "Use the matching Windows x64 Release BeatThis worker executable",
    )
    destination = runtime / BEAT_WORKER_EXECUTABLE
    destination.parent.mkdir(parents=True)
    shutil.copyfile(executable, destination)


def beat_worker_executable(args: argparse.Namespace, output: Path) -> Path:
    """Use the installer's own Cargo output, or build from this ZIP candidate's source."""
    executable = args.beat_worker_executable
    if args.runtime_directory:
        require(executable is not None, "Ordinary installs must supply their built BeatThis worker")
        assert executable is not None
        return executable
    require(executable is None, "The ZIP builder compiles its BeatThis worker from this checkout")
    from flitzis_looper.analysis.native_provisioning import build_native_worker  # noqa: PLC0415

    return build_native_worker(REPO, output / "beat-worker-build")


def stage_bundle_worker(root: Path, runtime: Path, checkout: Path, asset_cache: Path) -> None:
    """Give a standalone ZIP its own complete, relocatable worker selection."""
    report = root / "cache/analysis-package.json"
    subprocess.run(
        [
            str(runtime / "python/python.exe"),
            "-s",
            "-B",
            str(runtime / "prepare_analysis.py"),
            "--root",
            str(root),
            "--checkout",
            str(checkout),
            "--runtime",
            str(runtime),
            "--mode",
            "bundle",
            "--asset-cache",
            str(asset_cache),
            "--report-json",
            str(report),
        ],
        env=private_python_environment(runtime),
        check=True,
    )
    result = json.loads(report.read_text(encoding="utf-8"))
    require(
        result["portable_analysis"] == "passed" and result["execution_policy"] == "auto",
        "Bundle preparation did not establish the ONNX Runtime auto worker",
    )
    selected = root / "analysis-runtime/beat-this/current.json"
    with selected.open("xb") as stream:
        stream.write(base64.b64decode(result["pointer_bytes"], validate=True))
    report.unlink()


def preflight(runtime: Path, checkout: Path, root: Path, *, require_analysis: bool) -> None:
    subprocess.run(
        [
            str(runtime / "python/python.exe"),
            "-s",
            "-B",
            str(runtime / "bootstrap.py"),
            "--root",
            str(root),
            "--checkout",
            str(checkout),
            "--runtime",
            str(runtime),
            "--check-runtime" if require_analysis else "--check-build-runtime",
        ],
        env=private_python_environment(runtime),
        check=True,
    )


def package_runtime(staging: Path, output: Path, release: dict[str, object]) -> None:
    write_json(staging / "release-manifest.json", release)
    package = output / (str(release["release_id"]) + ".zip")
    require(not package.exists(), "An immutable package already exists for these inputs")
    emit(f"Compressing runtime: {package}")
    temporary = package.with_name(package.name + "." + uuid.uuid4().hex + ".tmp")
    try:
        with ZipFile(temporary, "x", ZIP_DEFLATED, compresslevel=1) as archive:
            for path in sorted(staging.rglob("*")):
                archive.write(path, path.relative_to(staging).as_posix())
        temporary.rename(package)
    finally:
        temporary.unlink(missing_ok=True)
    channel = {
        "schema_version": 1,
        "source_kind": "candidate",
        "release_id": release["release_id"],
        "git_commit": release["git_commit"],
        "package": package.name,
        "package_sha256": digest(package),
        "package_size": package.stat().st_size,
    }
    manifest = output / "portable-manifest.json"
    write_json(manifest, channel)
    emit(json.dumps({"channel": str(manifest), **channel}, indent=2))


def file_inventory(directory: Path) -> dict[str, str]:
    """Record exact owned files before user data is introduced."""
    return {
        path.relative_to(directory).as_posix(): digest(path)
        for path in sorted(directory.rglob("*"))
        if path.is_file()
    }


def native_redistribution(
    runtime: Path, checkout_files: dict[str, str], runtime_files: dict[str, str]
) -> dict[str, object]:
    """Compare staged files with the qualified Windows Native dependency reference."""
    return _redistribution_reference(runtime, NATIVE_SOURCE_RECORD, checkout_files, runtime_files)


def _redistribution_reference(
    runtime: Path,
    record_name: str,
    checkout_files: dict[str, str],
    runtime_files: dict[str, str],
) -> dict[str, object]:
    record_path = f"licenses/{record_name}"
    record = json.loads((runtime / record_path).read_text(encoding="utf-8"))
    inventories = {"checkout_files": checkout_files, "runtime_files": runtime_files}
    mismatches = {
        name: [
            path
            for path, sha256 in record["reference"][name].items()
            if inventory.get(path) != sha256
        ]
        for name, inventory in inventories.items()
    }
    return {
        "source_record": record_path,
        "reference_match": not any(mismatches.values()),
        "reference_mismatches": mismatches,
    }


def directory_inventory(directory: Path) -> list[str]:
    """Record owned directories, including empty Git and structural folders."""
    return [
        path.relative_to(directory).as_posix()
        for path in sorted(directory.rglob("*"))
        if path.is_dir()
    ]


def complete_bundle_checkout(destination: Path, *, dirty: bool) -> None:
    """Copy tracked working bytes without Git metadata or local remote paths."""
    destination.mkdir()
    entries = git("ls-files", "--stage", "-z").split("\0")
    for entry in filter(None, entries):
        metadata, relative = entry.split("\t", 1)
        mode, _, stage = metadata.split()
        require(
            mode in {"100644", "100755"} and stage == "0",
            f"Unsupported tracked candidate entry: {relative}",
        )
        source = REPO / relative
        require(
            source.resolve() == source and not source.is_symlink(),
            f"Tracked candidate path must not be a link: {relative}",
        )
        if dirty and not source.is_file():
            continue  # The explicit dirty candidate may omit deleted tracked files.
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)


def build(args: argparse.Namespace) -> None:
    commit, dirty = source_identity(args)
    # Keep an owned short build-drive path for Windows DLL loading. Resolve only
    # for containment comparisons; resolving operational paths defeats SUBST.
    output = args.output.absolute()
    staging = prepare_output(output)
    if args.runtime_directory:
        require(
            args.checkout_directory,
            "Ordinary installs require their complete candidate checkout",
        )
        checkout = args.checkout_directory.absolute()
        require(checkout.resolve() == REPO, "Candidate checkout must be the exact build source")
        runtime = staging
        require(args.root, "Ordinary installs require their owned installation root")
        root = args.root.absolute()
    else:
        checkout = staging / "flitzis-looper"
        complete_bundle_checkout(checkout, dirty=dirty)
        runtime = staging / "Runtime"
        runtime.mkdir()
        root = staging
        (root / "cache/temp").mkdir(parents=True)
    emit(f"Preparing private runtime: {staging}")
    shutil.copytree(
        Path(sys.base_prefix),
        runtime / "python",
        ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "site-packages"),
    )
    dependencies = copy_runtime_dependencies(runtime / "python/Lib/site-packages")
    shutil.copyfile(
        PORTABLE / "sitecustomize.py", runtime / "python/Lib/site-packages/sitecustomize.py"
    )
    emit(f"Copied {len(dependencies)} non-development distributions")
    native, wheel_sha256 = copy_application(runtime, checkout, args.native_wheel)
    copy_external_runtime(runtime, checkout, args)
    copy_beat_worker(runtime, beat_worker_executable(args, output))
    copy_licenses(runtime, args.license_file)
    if not args.runtime_directory:
        stage_bundle_worker(root, runtime, checkout, output / "beat-worker-assets")
    preflight(runtime, checkout, root, require_analysis=not args.runtime_directory)
    # Input data is introduced only during activation; its structural parent
    # must already be owned so a later empty parent is safe to retire.
    (checkout / "config").mkdir(exist_ok=True)
    checkout_files = file_inventory(checkout)
    runtime_files = file_inventory(runtime)
    release = {
        "schema_version": 2,
        "release_id": commit,
        "git_commit": commit,
        "source_dirty": dirty,
        "native_source_commit": args.native_source_commit,
        "native_sha256": digest(native),
        "native_wheel_sha256": wheel_sha256,
        "runtime_dependencies": dependencies,
        "python_version": sys.version.split()[0],
        "checkout_files": checkout_files,
        "checkout_directories": directory_inventory(checkout),
        "runtime_files": runtime_files,
        "native_redistribution": native_redistribution(runtime, checkout_files, runtime_files),
        "runtime_redistribution": _redistribution_reference(
            runtime, RUNTIME_REDISTRIBUTION_RECORD, checkout_files, runtime_files
        ),
    }
    if args.runtime_directory:
        destination = args.runtime_directory.absolute()
        require(not destination.exists(), "Runtime destination must be a fresh directory")
        require(
            not destination.resolve().is_relative_to(REPO),
            "Runtime must stay outside source checkout",
        )
        destination.parent.mkdir(parents=True, exist_ok=True)
        runtime.rename(destination)
        write_json(destination / "release-manifest.json", release)
        emit(json.dumps({"runtime_directory": str(destination), "git_commit": commit}))
        return
    package_candidate(staging, runtime, output, release)


def package_candidate(
    staging: Path, runtime: Path, output: Path, release: dict[str, object]
) -> None:
    commit = str(release["git_commit"])
    wheel_sha256 = str(release["native_wheel_sha256"])
    fingerprint = hashlib.sha256()
    fingerprint.update(bytes.fromhex(wheel_sha256))
    for path in sorted(staging.rglob("*")):
        if path.is_file():
            fingerprint.update(path.relative_to(staging).as_posix().encode("utf-8"))
            fingerprint.update(bytes.fromhex(digest(path)))
    fingerprint_value = fingerprint.hexdigest()
    release_id = commit[:12] + "-" + fingerprint_value[:12]
    release["release_id"] = release_id
    release["source_kind"] = "candidate"
    write_json(runtime / "release-manifest.json", release)
    for name in ["launch.ps1", "START-LOOPER.bat", "start.bat", "start-release.bat"]:
        template = runtime / ("dispatch.ps1" if name == "launch.ps1" else "START-LOOPER.bat")
        shutil.copyfile(template, staging / name)
    write_json(staging / "portable-root.json", CANDIDATE_OWNER)
    write_json(
        staging / "current.json",
        {
            "schema_version": 2,
            "git_commit": commit,
            "source_kind": "candidate",
            "release_id": release_id,
            "release": release,
        },
    )
    (staging / ".install.lock").touch()
    package_runtime(
        staging,
        output,
        {
            "schema_version": 1,
            "source_kind": "candidate",
            "release_id": release_id,
            "git_commit": commit,
            "source_dirty": release["source_dirty"],
            "native_source_commit": release["native_source_commit"],
            "native_sha256": release["native_sha256"],
            "native_wheel_sha256": wheel_sha256,
            "runtime_dependencies": release["runtime_dependencies"],
            "python_version": release["python_version"],
            "source_fingerprint": fingerprint_value,
        },
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-wheel", type=Path, required=True)
    parser.add_argument(
        "--beat-worker-executable", type=Path, help="Internal source installer input"
    )
    parser.add_argument("--native-source-commit", required=True)
    parser.add_argument("--ffmpeg-dir", type=Path, required=True)
    parser.add_argument("--dll-dir", type=Path, action="append", default=[])
    parser.add_argument("--license-file", action="append", default=[])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--allow-dirty", action="store_true")
    parser.add_argument("--runtime-directory", type=Path)
    parser.add_argument("--checkout-directory", type=Path)
    parser.add_argument("--root", type=Path)
    build(parser.parse_args())


if __name__ == "__main__":
    main()
