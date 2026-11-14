"""Provision the production Windows BeatThis package outside startup and inference."""

import os
import platform
import shutil
import subprocess
import sys
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import TYPE_CHECKING, BinaryIO, cast
from zipfile import ZipFile

from flitzis_looper.analysis import setup
from flitzis_looper.analysis.acquisition import _copy_quarantined, acquire_pinned_asset
from flitzis_looper.analysis.artifacts import file_digest
from flitzis_looper.analysis.native_beat_worker import (
    _SOURCE_ASSETS,
    _WORKER_EXE,
    CRT_DLLS,
    NATIVE_WORKER_PROTOCOL,
    NativeWorkerConfiguration,
    _cuda_assets,
    is_production_worker,
    verify_native_package,
)

if TYPE_CHECKING:
    from collections.abc import Iterator

_TARGET = "x86_64-pc-windows-msvc"
_MODEL_ASSETS = Path("assets/models/beat-this")
_ORT_ASSETS = Path("workers/beat_this/native_cuda")
_PUBLISHED_ASSETS = (
    "https://raw.githubusercontent.com/DJ-Flitzefinger/flitzis-looper/"
    "cfd0c83eb6896dd30d6a2974e47b3aeb0f38c267/assets/models/beat-this/"
)


@dataclass(frozen=True, slots=True)
class _Archive:
    name: str
    url: str
    size: int
    sha256: str

    def acquire(self, cache: Path, *, offline: bool) -> Path:
        destination = cache / self.name
        if offline:
            if (
                destination.stat().st_size != self.size
                or file_digest(destination, max_bytes=self.size) != self.sha256
            ):
                msg = f"Offline BeatThis download differs from its pin: {self.name}"
                raise ValueError(msg)
            return destination
        return acquire_pinned_asset(
            destination,
            source_url=self.url,
            sha256=self.sha256,
            size_bytes=self.size,
        ).path


_ORT_CPU = _Archive(
    "onnxruntime-1.24.4-cp312-cp312-win_amd64.whl",
    "https://files.pythonhosted.org/packages/8b/f4/"
    "6b89e297b93704345f0f3f8c62229bee323ef25682a3f9b4f89a39324950/"
    "onnxruntime-1.24.4-cp312-cp312-win_amd64.whl",
    12_596_856,
    "535b29475ca42b593c45fbb2152fbf1cdf3f287315bf650e6a724a0a1d065cdb",
)
_ORT_CUDA = _Archive(
    "onnxruntime_gpu-1.24.4-cp312-cp312-win_amd64.whl",
    "https://files.pythonhosted.org/packages/f2/86/"
    "70cecfdab1e963cc7f8c11e72040dfcd5cff85b1de2de74deba9611e0059/"
    "onnxruntime_gpu-1.24.4-cp312-cp312-win_amd64.whl",
    207_197_978,
    "da5c1e327d8e119a831be2790e69f93cf6daab9145ed0aca7577f412a620f709",
)


def _published_asset(name: str, source: str) -> _Archive:
    size, digest = _SOURCE_ASSETS[name]
    return _Archive(name, _PUBLISHED_ASSETS + source, size, digest)


_GRAPH = _published_asset("beat-this-dynamic.onnx", "beat-this-dynamic.onnx")
_SOXR = _published_asset("soxr.dll", "windows-x64/soxr.dll")


def _nvidia_archive(component: str, version: str, size: int, sha256: str) -> _Archive:
    family = "cudnn" if component == "cudnn" else "cuda"
    name = f"{component}-windows-x86_64-{version}-archive.zip"
    return _Archive(
        name,
        f"https://developer.download.nvidia.com/compute/{family}/redist/"
        f"{component}/windows-x86_64/{name}",
        size,
        sha256,
    )


# NVIDIA CUDA 12.8.1 and cuDNN 9.10.2 redistributable catalogues.
_NVIDIA_ARCHIVES = (
    (
        _nvidia_archive(
            "libcublas",
            "12.8.4.1",
            563_660_944,
            "57a470112cec7e112c95253dde8b3c7184d795dbd92b0bde77a4cb7f8c94c8aa",
        ),
        ("cublas64_12", "cublasLt64_12"),
    ),
    (
        _nvidia_archive(
            "cuda_cudart",
            "12.8.90",
            3_037_735,
            "4a39058fd8519444a81cfc7ae055d136f48d1a31ffa41ae255b35b2edd61e13b",
        ),
        ("cudart64_12",),
    ),
    (
        _nvidia_archive(
            "libcufft",
            "11.3.3.83",
            190_568_498,
            "cc6e0ba958cf23387b462017a24464c72bd901549046133f3d1ebcc3d7444c90",
        ),
        ("cufft64_11", "cufftw64_11"),
    ),
    (
        _nvidia_archive(
            "libcurand",
            "10.3.9.90",
            61_958_747,
            "46eba36c20748b21a5927b569b943337dd18485132119dfb39a5bcda4edb12e2",
        ),
        ("curand64_10",),
    ),
    (
        _nvidia_archive(
            "libcusolver",
            "11.7.3.90",
            253_486_232,
            "c71e33208740f6bce9f1da8fc7e9fa679ddfc831b5368a121f9c1ada3fa3d980",
        ),
        ("cusolver64_11", "cusolverMg64_11"),
    ),
    (
        _nvidia_archive(
            "libcusparse",
            "12.5.8.93",
            281_620_210,
            "f13b243912d8b07cc0136ad3760cd351553c74c1b54b7a6dbedb1dcc98ee8eb5",
        ),
        ("cusparse64_12",),
    ),
    (
        _nvidia_archive(
            "libnvjitlink",
            "12.8.93",
            257_345_522,
            "5680b0a42ddf20f11705ca9c365d002f032ab876d3fe44382eeba633b558ccc0",
        ),
        ("nvJitLink_120_0",),
    ),
    (
        _nvidia_archive(
            "cuda_nvrtc",
            "12.8.93",
            305_588_898,
            "a63302a077f0248a743a1a7caa7dbd80d0fac56c6cfa9c41fa05fac9b7e5eda5",
        ),
        ("nvrtc-builtins64_128", "nvrtc64_120_0.alt", "nvrtc64_120_0"),
    ),
    (
        _nvidia_archive(
            "cudnn",
            "9.10.2.21_cuda12",
            683_336_095,
            "c1a4567d822ebda7373fa1f19255dff4942302de741f830160b6c7d1fb31af23",
        ),
        (
            "cudnn64_9",
            "cudnn_adv64_9",
            "cudnn_cnn64_9",
            "cudnn_engines_precompiled64_9",
            "cudnn_engines_runtime_compiled64_9",
            "cudnn_graph64_9",
            "cudnn_heuristic64_9",
            "cudnn_ops64_9",
        ),
    ),
)


def build_native_worker(
    repository: Path,
    scratch_dir: Path,
    *,
    cargo: Path | str = "cargo",
    environment: dict[str, str] | None = None,
    offline: bool = False,
) -> Path:
    """Build the standalone worker from this checkout and its locked Rust dependencies."""
    _require_windows_x64()
    target = scratch_dir / "native-beat-this-build"
    subprocess.run(
        [
            cargo,
            "rustc",
            "--locked",
            *(("--offline",) if offline else ()),
            "--release",
            "--target",
            _TARGET,
            "--target-dir",
            target,
            "--manifest-path",
            repository / "rust/Cargo.toml",
            "-p",
            "flitzis-looper-analysis",
            "--features",
            "native-beat-this-worker",
            "--bin",
            "native-beat-this-worker",
            "--",
            "-C",
            "target-feature=+crt-static",
        ],
        cwd=repository,
        env=environment,
        check=True,
    )
    return target / _TARGET / "release" / _WORKER_EXE


def _require_windows_x64() -> None:
    if sys.platform != "win32" or platform.machine().lower() not in {"amd64", "x86_64"}:
        msg = "Production BeatThis ONNX setup requires Windows x64"
        raise ValueError(msg)


def _visual_cpp_runtime() -> Path:
    # App-local redistribution uses the versioned VC Redist directory, never
    # an arbitrary DLL on PATH or a copy from Windows/System32.
    if redist := os.environ.get("VCTOOLSREDISTDIR"):
        base = Path(redist).resolve(strict=True)
    else:
        vswhere = (
            Path(os.environ.get("PROGRAMFILES(X86)", r"C:\Program Files (x86)"))
            / "Microsoft Visual Studio/Installer/vswhere.exe"
        )
        output = subprocess.check_output(
            [
                vswhere,
                "-latest",
                "-products",
                "*",
                "-requires",
                "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
                "-property",
                "installationPath",
                "-utf8",
            ],
            encoding="utf-8",
        ).strip()
        if not output:
            msg = "BeatThis setup requires Visual Studio's x64 MSVC redistributable files"
            raise ValueError(msg)
        versions = [
            path
            for path in (Path(output) / "VC/Redist/MSVC").iterdir()
            if path.is_dir() and all(part.isdecimal() for part in path.name.split("."))
        ]
        if not versions:
            msg = "Visual Studio contains no versioned MSVC redistributable files"
            raise ValueError(msg)
        base = max(versions, key=lambda path: tuple(map(int, path.name.split("."))))
    for directory in sorted((base / "x64").glob("Microsoft.VC*.CRT"), reverse=True):
        if all((directory / name).is_file() for name in CRT_DLLS):
            return directory
    msg = "Visual Studio contains no complete x64 MSVC CRT directory"
    raise ValueError(msg)


def _extract_members(
    path: Path, prefix: str, package: Path, assets: dict[str, tuple[int, str]]
) -> None:
    with ZipFile(path) as archive:
        for name, (size, checksum) in assets.items():
            member = archive.getinfo(prefix + Path(name).name)
            if member.file_size != size:
                msg = f"BeatThis archive member differs from its pin: {name}"
                raise ValueError(msg)
            with archive.open(member) as source:
                copied = _copy_quarantined(cast("BinaryIO", source), package / name, size)
            if (copied.size_bytes, copied.sha256) != (size, checksum):
                msg = f"BeatThis archive member differs from its pin: {name}"
                raise ValueError(msg)


@contextmanager
def prepare_native_package(
    *,
    repository: Path,
    scratch_dir: Path,
    executable: Path,
    offline: bool = False,
    crt_dir: Path | None = None,
    asset_cache: Path | None = None,
    include_cuda: bool = True,
) -> Iterator[Path]:
    """Assemble the closed CPU/CUDA package for the existing atomic staging API."""
    _require_windows_x64()
    cache = asset_cache if asset_cache is not None else scratch_dir / "beat-this-downloads"
    cache.mkdir(parents=True, exist_ok=True)
    with TemporaryDirectory(prefix="beat-this-package-", dir=scratch_dir) as temporary:
        package = Path(temporary)
        for directory in ("ort", "crt"):
            (package / directory).mkdir()
        shutil.copyfile(executable, package / _WORKER_EXE)
        for asset in (_GRAPH, _SOXR):
            shutil.copyfile(asset.acquire(cache, offline=offline), package / asset.name)
        crt = crt_dir if crt_dir is not None else _visual_cpp_runtime()
        for name in CRT_DLLS:
            shutil.copyfile(crt / name, package / "crt" / name)
        _extract_members(
            _ORT_CPU.acquire(cache, offline=offline),
            "onnxruntime/capi/",
            package,
            {name: pin for name, pin in _SOURCE_ASSETS.items() if name.startswith("ort/")},
        )
        if include_cuda:
            _prepare_cuda_package(repository, package, cache, offline=offline)
        yield package


def _prepare_cuda_package(repository: Path, package: Path, cache: Path, *, offline: bool) -> None:
    (package / "cuda").mkdir()
    for name in ("step23-package.json", "step23-runtime.json"):
        shutil.copyfile(repository / _ORT_ASSETS / name, package / "cuda" / name)
    cuda = {name: pin for name, pin in _cuda_assets(package).items() if name.endswith(".dll")}
    _extract_members(
        _ORT_CUDA.acquire(cache, offline=offline),
        "onnxruntime/capi/",
        package,
        {name: pin for name, pin in cuda.items() if Path(name).name.startswith("onnxruntime")},
    )
    for archive, dlls in _NVIDIA_ARCHIVES:
        path = archive.acquire(cache, offline=offline)
        names = [f"cuda/{name}.dll" for name in dlls]
        _extract_members(path, f"{path.stem}/bin/", package, {name: cuda[name] for name in names})


def load_production_worker(install_root: Path, scratch_dir: Path) -> NativeWorkerConfiguration:
    """Load and verify the selected production closure without starting a model."""
    selected = setup.load_worker_configuration(install_root, scratch_dir, native_only=True)
    assert isinstance(selected, NativeWorkerConfiguration)
    verify_native_package(selected)
    return selected


def stage_production_worker(
    install_root: Path,
    scratch_dir: Path,
    repository: Path,
    executable: Path,
    *,
    crt_dir: Path | None = None,
    offline: bool = False,
    asset_cache: Path | None = None,
    include_cuda: bool | None = None,
) -> NativeWorkerConfiguration:
    """Verify or stage a Windows package without replacing the active pointer."""
    from flitzis_looper.analysis.beat_cuda_cache import cuda_device_identity  # noqa: PLC0415

    _require_windows_x64()
    scratch_dir.mkdir(parents=True, exist_ok=True)
    try:
        selected = setup.load_worker_configuration(install_root, scratch_dir, native_only=True)
    except OSError, ValueError:
        selected = None
    if include_cuda is None:
        try:
            include_cuda = cuda_device_identity() is not None
        except OSError, RuntimeError, ValueError:
            # Keep the CPU available and let the readiness check expose the probe error.
            include_cuda = False
    checksum = file_digest(executable, max_bytes=64 * 1024 * 1024)
    if (
        isinstance(selected, NativeWorkerConfiguration)
        and is_production_worker(selected)
        and selected.executable_sha256 == checksum
        and (not include_cuda or selected.execution_policy == "auto")
    ):
        try:
            verify_native_package(selected)
        except OSError, ValueError:
            pass
        else:
            return selected
    with prepare_native_package(
        repository=repository,
        scratch_dir=scratch_dir,
        executable=executable,
        offline=offline,
        crt_dir=crt_dir,
        asset_cache=asset_cache,
        include_cuda=include_cuda,
    ) as package:
        return setup.stage_native_worker(
            install_root,
            scratch_dir,
            package,
            cuda_policy="auto" if include_cuda else "cpu",
            startup_fault_protocol=NATIVE_WORKER_PROTOCOL,
        )
