"""Verify mapped dependency paths without loading a foreign DLL."""

import runpy
from importlib import metadata
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[2]
BOOTSTRAP = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))
ORIGINS = BOOTSTRAP["dependency_dll_origins"]
TORCH_NAMES = {"torch_cpu.dll", "cudart64_13.dll"}


@pytest.fixture
def private_runtime(tmp_path: Path) -> tuple[Path, list[Path]]:
    runtime = tmp_path / "Portable Root/Runtime"
    torch = runtime / "python/Lib/site-packages/torch/lib"
    ffmpeg = runtime / "runtime/ffmpeg"
    torch.mkdir(parents=True)
    ffmpeg.mkdir(parents=True)
    torch_dlls = [torch / name for name in ("torch_cpu.dll", "cudart64_13.dll")]
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
    paths = torch_dlls + ffmpeg_dlls
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
    report = ORIGINS([*paths, ucrt, driver], runtime / ".." / "Runtime", system, TORCH_NAMES)
    assert report["torch"] == {path.name: str(path) for path in paths[:2]}
    assert report["ffmpeg"] == {path.name: str(path) for path in paths[2:]}
    assert report["ffmpeg_not_loaded"] == ["avdevice-62.dll"]
    assert report["os_dependencies"] == {"ucrtbase.dll": str(ucrt), "nvcuda.dll": str(driver)}
    assert ORIGINS(paths, runtime, system, TORCH_NAMES)["os_dependencies"] == {}


@pytest.mark.parametrize("basename", ["cudart64_13.dll", "avcodec-62.dll"])
def test_required_dlls_must_be_observed(
    tmp_path: Path, private_runtime: tuple[Path, list[Path]], basename: str
) -> None:
    runtime, paths = private_runtime
    with pytest.raises(RuntimeError, match="Missing loaded dependency DLLs"):
        ORIGINS([path for path in paths if path.name != basename], runtime, tmp_path, TORCH_NAMES)


@pytest.mark.parametrize(
    "foreign",
    ["python/cudart64_13.dll", "python/avcodec-62.dll", "runtime/ffmpeg/avcodec-61.dll"],
)
def test_foreign_duplicate_or_ffmpeg_version_fails_even_with_private_modules_present(
    tmp_path: Path, private_runtime: tuple[Path, list[Path]], foreign: str
) -> None:
    runtime, paths = private_runtime
    with pytest.raises(RuntimeError, match="Nonportable dependency DLL resolution"):
        ORIGINS([*paths, runtime / foreign], runtime, tmp_path, TORCH_NAMES)


def test_lookalike_private_driver_fails(
    tmp_path: Path, private_runtime: tuple[Path, list[Path]]
) -> None:
    runtime, paths = private_runtime
    with pytest.raises(RuntimeError, match="Unexpected Windows DLL origin"):
        ORIGINS(
            [*paths, runtime / "python/nvcuda.dll"],
            runtime,
            tmp_path / "Windows/System32",
            TORCH_NAMES,
        )


def test_missing_private_file_cannot_fall_back_to_external_cuda(
    tmp_path: Path, private_runtime: tuple[Path, list[Path]], monkeypatch: pytest.MonkeyPatch
) -> None:
    runtime, paths = private_runtime
    site = runtime / "python/Lib/site-packages"
    dist_info = site / "torch-2.12.0.dist-info"
    dist_info.mkdir()
    missing = paths[1]
    missing.unlink()
    (dist_info / "RECORD").write_text(
        "torch/lib/torch_cpu.dll,,\ntorch/lib/cudart64_13.dll,,\n", encoding="utf-8"
    )
    distribution = metadata.PathDistribution(dist_info)
    monkeypatch.setattr(metadata, "distribution", lambda _: distribution)
    expected = BOOTSTRAP["torch_wheel_dlls"](runtime)
    assert expected == TORCH_NAMES
    with pytest.raises(RuntimeError, match="Nonportable dependency DLL resolution"):
        ORIGINS([paths[0], *paths[2:], tmp_path / missing.name], runtime, tmp_path, expected)
