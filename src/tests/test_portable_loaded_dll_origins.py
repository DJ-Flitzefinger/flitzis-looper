"""Verify mapped dependency paths without loading a foreign DLL."""

import runpy
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[2]
BOOTSTRAP = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))
ORIGINS = BOOTSTRAP["dependency_dll_origins"]


@pytest.fixture
def private_runtime(tmp_path: Path) -> tuple[Path, list[Path]]:
    runtime = tmp_path / "Portable Root/Runtime"
    ffmpeg = runtime / "runtime/ffmpeg"
    ffmpeg.mkdir(parents=True)
    ffmpeg_dlls = [
        ffmpeg / name
        for name in (
            "avcodec-62.dll",
            "avfilter-11.dll",
            "avformat-62.dll",
            "avutil-60.dll",
            "swresample-6.dll",
            "swscale-9.dll",
        )
    ]
    paths = ffmpeg_dlls
    for path in [*paths, ffmpeg / "avdevice-62.dll"]:
        path.touch()
    return runtime, paths


def test_report_separates_mapped_private_and_os_dependencies(
    tmp_path: Path, private_runtime: tuple[Path, list[Path]]
) -> None:
    runtime, paths = private_runtime
    system = tmp_path / "Windows/System32"
    ucrt = system / "ucrtbase.dll"
    driver = system / "DriverStore/FileRepository/nvidia/nvcuda.dll"
    report = ORIGINS([*paths, ucrt, driver], runtime / ".." / "Runtime", system)
    assert report["ffmpeg"] == {path.name: str(path) for path in paths}
    assert report["os_dependencies"] == {"ucrtbase.dll": str(ucrt), "nvcuda.dll": str(driver)}
    assert ORIGINS(paths, runtime, system)["os_dependencies"] == {}


@pytest.mark.parametrize(
    "foreign",
    ["python/avcodec-62.dll", "runtime/ffmpeg/avcodec-61.dll"],
)
def test_foreign_duplicate_or_ffmpeg_version_fails_even_with_private_modules_present(
    tmp_path: Path, private_runtime: tuple[Path, list[Path]], foreign: str
) -> None:
    runtime, paths = private_runtime
    with pytest.raises(RuntimeError, match="Nonportable dependency DLL resolution"):
        ORIGINS([*paths, runtime / foreign], runtime, tmp_path)


def test_lookalike_private_driver_fails(
    tmp_path: Path, private_runtime: tuple[Path, list[Path]]
) -> None:
    runtime, paths = private_runtime
    with pytest.raises(RuntimeError, match="Unexpected Windows DLL origin"):
        ORIGINS(
            [*paths, runtime / "python/nvcuda.dll"],
            runtime,
            tmp_path / "Windows/System32",
        )
