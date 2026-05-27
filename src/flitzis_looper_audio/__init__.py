# ruff: noqa: I001
from __future__ import annotations

import os
from pathlib import Path


_DLL_DIRECTORY_HANDLES: list[object] = []


def _split_path_entries(value: str | None) -> tuple[Path, ...]:
    if not value:
        return ()
    return tuple(Path(entry) for entry in value.split(os.pathsep) if entry)


def _windows_dll_directories() -> list[Path]:
    candidates: list[Path] = [Path(__file__).resolve().parent]
    candidates.extend(_split_path_entries(os.environ.get("RUBBERBAND_DLL_DIRS")))
    candidates.extend(_split_path_entries(os.environ.get("RUBBERBAND_DLL_DIR")))

    triplet = os.environ.get("RUBBERBAND_VCPKG_TRIPLET", "x64-windows")
    if vcpkg_root := os.environ.get("VCPKG_ROOT"):
        candidates.append(Path(vcpkg_root) / "installed" / triplet / "bin")
    if local_app_data := os.environ.get("LOCALAPPDATA"):
        candidates.append(Path(local_app_data) / "vcpkg" / "installed" / triplet / "bin")

    candidates.extend(
        path_entry
        for path_entry in _split_path_entries(os.environ.get("PATH"))
        if (path_entry / "rubberband-3.dll").exists()
    )

    seen: set[str] = set()
    directories: list[Path] = []
    for candidate in candidates:
        normalized = os.path.normcase(os.path.abspath(str(candidate)))
        if normalized in seen:
            continue
        seen.add(normalized)
        directories.append(candidate)
    return directories


def _add_windows_dll_directories() -> None:
    add_dll_directory = getattr(os, "add_dll_directory", None)
    if os.name != "nt" or add_dll_directory is None:
        return

    for directory in _windows_dll_directories():
        if not directory.is_dir():
            continue
        try:
            _DLL_DIRECTORY_HANDLES.append(add_dll_directory(directory))
        except OSError:
            continue


_add_windows_dll_directories()

from .flitzis_looper_audio import *  # noqa: E402,F403

_flitzis_looper_audio = flitzis_looper_audio  # noqa: F405

__doc__ = _flitzis_looper_audio.__doc__

if hasattr(_flitzis_looper_audio, "__all__"):
    __all__ = _flitzis_looper_audio.__all__
