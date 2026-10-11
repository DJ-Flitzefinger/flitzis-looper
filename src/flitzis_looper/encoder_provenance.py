"""Bounded witnesses for the supported Windows shared PCM16 encoder.

This module inspects only a caller-owned encoder process. It has no source,
publication, asset or deletion authority and imports no Native extension.
"""

import ctypes
import hashlib
import json
import os
import re
import time
from ctypes import wintypes
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, BinaryIO, TypedDict

if TYPE_CHECKING:
    import subprocess

ENCODER_CONTRACT_ENV = "FLITZIS_DEMUCS_ENCODER_CONTRACT"
ENCODER_RECEIPT = ".encoder-execution.json"
ENCODER_RESULT = "FLITZIS_ENCODER_BIRTH_V1:"
SHARED_LIBRARIES = (
    "avcodec-62.dll",
    "avdevice-62.dll",
    "avfilter-11.dll",
    "avformat-62.dll",
    "avutil-60.dll",
    "swresample-6.dll",
    "swscale-9.dll",
)


class EncoderProof(TypedDict):
    pid: int
    images: list[list[str | int]]


@dataclass(slots=True)
class _EncoderRecord:
    handle: BinaryIO
    contract: str
    runs: list[dict[str, object]]
    verified: bool = True


# A Demucs process runs one attempt, with at most one CUDA-to-CPU retry.
# Handles are born exclusively and kept through all four sequential outputs.
_records: dict[Path, _EncoderRecord] = {}
_completed: dict[Path, tuple[int, int, str]] = {}


def supported_encoder_images(executable: Path) -> tuple[Path, ...]:
    """Require the explicit supported shared build; other converters stay unknown."""
    if os.name != "nt" or executable.name.lower() != "ffmpeg.exe":
        return ()
    images = (executable, *(executable.parent / name for name in SHARED_LIBRARIES))
    actual = {path.name.lower() for path in executable.parent.glob("*.dll")}
    redirection = executable.with_name(executable.name + ".local")
    if actual != set(SHARED_LIBRARIES) or redirection.exists():
        return ()
    return images if all(path.is_file() for path in images) else ()


def encoder_contract(executable: Path) -> str | None:
    """Capture path/FileID/extent while the parent holds all Native file witnesses."""
    images = supported_encoder_images(executable)
    if not images:
        return None
    return json.dumps([_file_witness(path) for path in images], separators=(",", ":"))


def _file_witness(path: Path) -> tuple[str, int, int, int]:
    path = path.resolve(strict=True)
    stat = path.stat()
    return str(path), stat.st_dev, stat.st_ino, stat.st_size


class _ModuleEntry(ctypes.Structure):
    # MODULEENTRY32W: Microsoft Win32 ToolHelp's actual process image inventory.
    _fields_ = (
        ("dwSize", wintypes.DWORD),
        ("th32ModuleID", wintypes.DWORD),
        ("th32ProcessID", wintypes.DWORD),
        ("GlblcntUsage", wintypes.DWORD),
        ("ProccntUsage", wintypes.DWORD),
        ("modBaseAddr", ctypes.c_void_p),
        ("modBaseSize", wintypes.DWORD),
        ("hModule", wintypes.HMODULE),
        ("szModule", wintypes.WCHAR * 256),
        ("szExePath", wintypes.WCHAR * 260),
    )


def _module_images(pid: int) -> tuple[Path, ...]:
    api = ctypes.WinDLL("kernel32", use_last_error=True)
    api.CreateToolhelp32Snapshot.argtypes = (wintypes.DWORD, wintypes.DWORD)
    api.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    api.Module32FirstW.argtypes = (wintypes.HANDLE, ctypes.POINTER(_ModuleEntry))
    api.Module32FirstW.restype = wintypes.BOOL
    api.Module32NextW.argtypes = api.Module32FirstW.argtypes
    api.Module32NextW.restype = wintypes.BOOL
    api.CloseHandle.argtypes = (wintypes.HANDLE,)
    api.CloseHandle.restype = wintypes.BOOL
    snapshot = api.CreateToolhelp32Snapshot(0x18, pid)
    if snapshot == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        item = _ModuleEntry()
        item.dwSize = ctypes.sizeof(item)
        if not api.Module32FirstW(snapshot, ctypes.byref(item)):
            raise ctypes.WinError(ctypes.get_last_error())
        paths: list[Path] = []
        for _ in range(256):
            paths.append(Path(item.szExePath))
            if not api.Module32NextW(snapshot, ctypes.byref(item)):
                if ctypes.get_last_error() != 18:  # ERROR_NO_MORE_FILES
                    raise ctypes.WinError(ctypes.get_last_error())
                return tuple(paths)
        msg = "encoder module inventory exceeds its bounded capacity"
        raise OSError(msg)
    finally:
        api.CloseHandle(snapshot)


def _system_directory() -> Path:
    api = ctypes.WinDLL("kernel32", use_last_error=True)
    api.GetSystemDirectoryW.argtypes = (wintypes.LPWSTR, wintypes.UINT)
    api.GetSystemDirectoryW.restype = wintypes.UINT
    value = ctypes.create_unicode_buffer(32768)
    size = api.GetSystemDirectoryW(value, len(value))
    if not 0 < size < len(value):
        raise ctypes.WinError(ctypes.get_last_error())
    return Path(value.value).resolve(strict=True)


def _matches_images(paths: tuple[Path, ...], expected: list[list[str | int]]) -> bool:
    expected_paths = {os.path.normcase(str(item[0])): item for item in expected}
    actual: set[str] = set()
    system = _system_directory()
    for candidate in paths:
        path = candidate.resolve(strict=True)
        normalized = os.path.normcase(str(path))
        if normalized in expected_paths:
            if list(_file_witness(path)) != expected_paths[normalized]:
                return False
            actual.add(normalized)
        elif not _system_image(path, system):
            return False
    return actual == set(expected_paths)


def _system_image(path: Path, system: Path) -> bool:
    """Explicit OS-build policy: System32 and the observed Windows common-controls assembly."""
    if path.is_relative_to(system):
        return True
    return (
        path.name.lower() == "comctl32.dll"
        and path.parent.parent == system.parent / "WinSxS"
        and re.fullmatch(
            r"amd64_microsoft.windows.common-controls_6595b64144ccf1df_5\.82\.\d+\.\d+"
            r"_none_[0-9a-f]{16}",
            path.parent.name.lower(),
        )
        is not None
    )


def inspect_encoder(process: subprocess.Popen[bytes]) -> EncoderProof | None:
    """Observe the actual converter waiting on its PCM pipe; missing proof is unknown."""
    encoded = os.environ.get(ENCODER_CONTRACT_ENV)
    if encoded is None or os.name != "nt":
        return None
    try:
        expected = json.loads(encoded)
        if not isinstance(expected, list) or len(expected) != 8:
            return None
        deadline = time.monotonic() + 2
        while process.poll() is None and time.monotonic() < deadline:
            try:
                paths = _module_images(process.pid)
                if _matches_images(paths, expected):
                    return {"pid": process.pid, "images": expected}
            except OSError:
                pass  # Loader snapshots can transiently report ERROR_BAD_LENGTH.
            time.sleep(0.005)
    except OSError, TypeError, ValueError:
        return None
    return None


def record_encoder(path: Path, proof: EncoderProof | None) -> None:
    """Write an execution diagnostic inside this owned backend attempt, never an asset permit."""
    encoded = os.environ.get(ENCODER_CONTRACT_ENV)
    if encoded is None:
        return
    target = path.parent / ENCODER_RECEIPT
    record = _records.get(target)
    if record is None:
        if len(_records) >= 2:
            return
        try:
            record = _EncoderRecord(target.open("xb"), encoded, [])
        except OSError:
            return  # An existing diagnostic is never overwritten or adopted.
        _records[target] = record
    if record.contract != encoded or len(record.runs) >= 4:
        return
    try:
        held = os.fstat(record.handle.fileno())
        current = target.stat()
        if (held.st_dev, held.st_ino) != (current.st_dev, current.st_ino):
            record.verified = False
            return
        record.runs.append({"leaf": path.name, "proof": proof})
        record.verified = record.verified and proof is not None
        encoded_record = json.dumps(
            {
                "contract": record.contract,
                "runs": record.runs,
                "verified": record.verified,
            },
            separators=(",", ":"),
        ).encode("utf-8")
        record.handle.seek(0)
        record.handle.write(encoded_record)
        record.handle.truncate()
        record.handle.flush()
        if len(record.runs) == 4 and record.verified:
            _completed[target] = (
                held.st_dev,
                held.st_ino,
                hashlib.sha256(encoded_record).hexdigest(),
            )
    except OSError:
        record.verified = False
    finally:
        if len(record.runs) == 4 or not record.verified:
            record.handle.close()
            _records.pop(target, None)


def encoder_completion_line() -> str:
    """Return actual exclusive-birth receipts to the parent through captured child stdout."""
    receipts = [[str(path), *witness] for path, witness in _completed.items()]
    return ENCODER_RESULT + json.dumps(receipts, separators=(",", ":"))


def _read_encoder_receipt(output_root: Path, stdout: str) -> bytes | None:
    claims = [
        line.removeprefix(ENCODER_RESULT)
        for line in stdout.splitlines()
        if line.startswith(ENCODER_RESULT)
    ]
    if len(claims) != 1:
        return None
    receipts = json.loads(claims[0])
    if len(receipts) != 1 or len(receipts[0]) != 4:
        return None
    path, device, identity, digest = receipts[0]
    target = Path(path).resolve(strict=True)
    if target.name != ENCODER_RECEIPT or not target.is_relative_to(output_root.resolve()):
        return None
    with target.open("rb") as handle:
        stat = os.fstat(handle.fileno())
        if (stat.st_dev, stat.st_ino) != (device, identity):
            return None
        encoded = handle.read(32769)
    if len(encoded) > 32768 or hashlib.sha256(encoded).hexdigest() != digest:
        return None
    return encoded


def verified_encoder_output(output_root: Path, contract: str | None, stdout: str) -> bool:
    """Require all four actual converter executions, bound to the parent-held images."""
    if contract is None:
        return False
    try:
        encoded = _read_encoder_receipt(output_root, stdout)
        if encoded is None:
            return False
        record = json.loads(encoded)
        runs = record["runs"]
        return (
            record["contract"] == contract
            and record["verified"] is True
            and len(runs) == 4
            and {run["leaf"] for run in runs}
            == {"drums.wav", "bass.wav", "other.wav", "vocals.wav"}
            and all(run["proof"]["images"] == json.loads(contract) for run in runs)
            and len({run["proof"]["pid"] for run in runs}) == 4
        )
    except OSError, IndexError, KeyError, TypeError, ValueError:
        return False
