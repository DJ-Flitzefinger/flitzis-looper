"""Exclusive recording attempts and atomic non-replacing filesystem publication.

These primitives prove namespace/file custody only. The caller must separately
verify the complete codec/payload and retain its original live Native permission.
Nothing here retires files, grants capture authority or claims a completed take.
"""

import ctypes
import json
import os
import re
import stat
import sys
from contextlib import ExitStack, contextmanager
from ctypes import wintypes
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import TYPE_CHECKING
from uuid import uuid4

from flitzis_looper.recording_options import RecordingOptions

if sys.platform == "win32":
    import msvcrt

if TYPE_CHECKING:
    from collections.abc import Iterator

type FileIdentity = tuple[int, int]

_REPARSE_POINT = 0x400
_OWNER_SCHEMA = "flitzi-recording-owner-v1"


class RecordingNamespaceError(OSError):
    """The original owned namespace or file identity is unavailable."""


class RecordingCustodyReleaseError(RecordingNamespaceError):
    """An actual pin close failed, so completed retirement is unproven."""

    def __init__(self, path: Path, cause: OSError) -> None:
        self.path = path
        self.retirement_uncertain = True
        super().__init__(f"recording custody release failed for {path}: {cause}")


class RecordingPublicationCommittedError(RecordingNamespaceError):
    """The final link committed, but subsequent custody release reported an error."""

    def __init__(self, path: Path, identity: FileIdentity, cause: OSError) -> None:
        self.published_path = path
        self.output_identity = identity
        self.retirement_uncertain = True
        super().__init__(f"recording published at {path}; custody release failed: {cause}")


@dataclass(frozen=True, kw_only=True)
class RecordingPaths:
    """One caller-owned attempt; metadata is not Native or codec authority."""

    attempt_id: str
    created_at_utc: str
    options: RecordingOptions
    root: Path
    directory: Path
    spool_path: Path
    pcm_path: Path
    encoded_path: Path
    decoded_path: Path
    metadata_path: Path
    final_path: Path
    root_identity: FileIdentity
    directory_identity: FileIdentity
    metadata_identity: FileIdentity


def _identity(metadata: os.stat_result) -> FileIdentity:
    identity = metadata.st_dev, metadata.st_ino
    if identity[1] == 0:
        message = "recording filesystem does not expose a stable file identity"
        raise RecordingNamespaceError(message)
    return identity


def _plain_metadata(path: Path, *, directory: bool) -> os.stat_result:
    metadata = path.stat(follow_symlinks=False)
    if (
        stat.S_ISLNK(metadata.st_mode)
        or getattr(metadata, "st_file_attributes", 0) & _REPARSE_POINT
    ):
        message = "recording paths must not contain symlinks or reparse points"
        raise RecordingNamespaceError(message)
    appropriate = stat.S_ISDIR(metadata.st_mode) if directory else stat.S_ISREG(metadata.st_mode)
    if not appropriate:
        message = "recording path has an unsupported file type"
        raise RecordingNamespaceError(message)
    return metadata


def _absolute(path: Path) -> None:
    if not isinstance(path, Path) or not path.is_absolute() or ".." in path.parts:
        message = "recording root must be an absolute path without parent traversal"
        raise ValueError(message)


@contextmanager
def _windows_read_handle(path: Path, *, directory: bool, sharing: int) -> Iterator[int]:
    """Own one original read handle; the two callers select their fixed share policy."""
    api = ctypes.WinDLL("kernel32", use_last_error=True)
    api.CreateFileW.argtypes = (
        wintypes.LPCWSTR,
        wintypes.DWORD,
        wintypes.DWORD,
        ctypes.c_void_p,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.HANDLE,
    )
    api.CreateFileW.restype = wintypes.HANDLE
    api.CloseHandle.argtypes = (wintypes.HANDLE,)
    api.CloseHandle.restype = wintypes.BOOL
    access = 0x80000000  # GENERIC_READ also enforces directory share-delete denial.
    flags = 0x00200000 | (0x02000000 if directory else 0)  # OPEN_REPARSE / BACKUP_SEMANTICS.
    handle = api.CreateFileW(str(path), access, sharing, None, 3, flags, None)
    if handle == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    descriptor: int | None = None
    try:
        descriptor = msvcrt.open_osfhandle(int(handle), os.O_RDONLY | os.O_BINARY)
        yield descriptor
    finally:
        try:
            if descriptor is None:
                if not api.CloseHandle(handle):
                    raise ctypes.WinError(ctypes.get_last_error())
            else:
                os.close(descriptor)
        except OSError as error:
            raise RecordingCustodyReleaseError(path, error) from error


@contextmanager
def _windows_pin(path: Path, *, directory: bool) -> Iterator[int]:
    """Hold the actual Windows entry without sharing delete; files also deny writes."""
    sharing = 0x3 if directory else 0x1  # READ|WRITE / READ, never DELETE.
    with _windows_read_handle(path, directory=directory, sharing=sharing) as descriptor:
        yield descriptor


def _pin(stack: ExitStack, path: Path, *, directory: bool) -> FileIdentity:
    before = _identity(_plain_metadata(path, directory=directory))
    if sys.platform == "win32":
        descriptor = stack.enter_context(_windows_pin(path, directory=directory))
        held = os.fstat(descriptor)
        if getattr(held, "st_file_attributes", 0) & _REPARSE_POINT or _identity(held) != before:
            message = "recording entry changed before its Windows pin"
            raise RecordingNamespaceError(message)
    if _identity(_plain_metadata(path, directory=directory)) != before:
        message = "recording entry changed during identity capture"
        raise RecordingNamespaceError(message)
    return before


def _pin_ancestors(stack: ExitStack, path: Path) -> FileIdentity:
    _absolute(path)
    result: FileIdentity | None = None
    for ancestor in reversed((path, *path.parents)):
        result = _pin(stack, ancestor, directory=True)
    assert result is not None
    return result


@contextmanager
def hold_live_recording_spool(path: Path) -> Iterator[FileIdentity]:
    """Bridge the original ready Windows writer to the final closed-spool pin.

    Caller retains the original live Native ticket and namespace. Native's
    creating writer denies replacement; this read handle permits that writer's
    WRITE access and keeps the same ordinary file and ancestors against deletion
    through finalizer and outer cleanup. It authenticates identity only, never
    bytes, readiness, completion or publication. The final original writer hash
    and stronger closed-spool verification remain mandatory.

    Fail closed on platforms without this Windows sharing contract. A caller
    must supply an equally strong platform adapter rather than a no-op fallback.
    """
    _absolute(path)
    if sys.platform != "win32":
        message = "live recording spool custody requires the Windows sharing contract"
        raise RecordingNamespaceError(message)
    with ExitStack() as stack:
        _pin_ancestors(stack, path.parent)
        before = _identity(_plain_metadata(path, directory=False))
        descriptor = stack.enter_context(_windows_read_handle(path, directory=False, sharing=0x3))
        held = os.fstat(descriptor)
        if (
            not stat.S_ISREG(held.st_mode)
            or getattr(held, "st_file_attributes", 0) & _REPARSE_POINT
            or _identity(held) != before
            or _identity(_plain_metadata(path, directory=False)) != before
        ):
            message = "recording live spool changed before its original identity pin"
            raise RecordingNamespaceError(message)
        yield before


def _owner_bytes(paths: RecordingPaths) -> bytes:
    return (
        json.dumps(
            {
                "schema": _OWNER_SCHEMA,
                "attempt_id": paths.attempt_id,
                "created_at_utc": paths.created_at_utc,
                "options": paths.options.model_dump(mode="json"),
                "root_identity": paths.root_identity,
                "directory_identity": paths.directory_identity,
                "metadata_identity": paths.metadata_identity,
                "spool": "float.capture",
                "pcm": "input.pcm",
                "encoded": "encoded.partial",
                "decoded": "decoded.pcm",
                "final": paths.final_path.name,
            },
            sort_keys=True,
            separators=(",", ":"),
        )
        + "\n"
    ).encode("utf-8")


def _assert_layout(paths: RecordingPaths) -> None:
    _absolute(paths.root)
    RecordingOptions.model_validate(paths.options, strict=True)
    if (
        re.fullmatch(r"[0-9a-f]{32}", paths.attempt_id) is None
        or re.fullmatch(r"[0-9]{8}T[0-9]{12}Z", paths.created_at_utc) is None
    ):
        message = "invalid recording attempt identity"
        raise RecordingNamespaceError(message)
    directory = paths.root / f".incomplete-{paths.attempt_id}"
    expected = (
        directory,
        directory / "float.capture",
        directory / "input.pcm",
        directory / "encoded.partial",
        directory / "decoded.pcm",
        directory / "owner.json",
        paths.root / f"recording-{paths.created_at_utc}-{paths.attempt_id}.{paths.options.format}",
    )
    actual = (
        paths.directory,
        paths.spool_path,
        paths.pcm_path,
        paths.encoded_path,
        paths.decoded_path,
        paths.metadata_path,
        paths.final_path,
    )
    if actual != expected:
        message = "recording attempt paths changed"
        raise RecordingNamespaceError(message)


def _assert_pinned(paths: RecordingPaths, stack: ExitStack) -> None:
    _assert_layout(paths)
    if _pin_ancestors(stack, paths.root) != paths.root_identity:
        message = "recording root identity changed"
        raise RecordingNamespaceError(message)
    if _pin(stack, paths.directory, directory=True) != paths.directory_identity:
        message = "recording attempt directory identity changed"
        raise RecordingNamespaceError(message)
    if _pin(stack, paths.metadata_path, directory=False) != paths.metadata_identity:
        message = "recording owner metadata identity changed"
        raise RecordingNamespaceError(message)
    expected = _owner_bytes(paths)
    with paths.metadata_path.open("rb") as owner:
        if (
            _identity(os.fstat(owner.fileno())) != paths.metadata_identity
            or owner.read(len(expected) + 1) != expected
        ):
            message = "recording owner metadata changed"
            raise RecordingNamespaceError(message)


def create_recording_namespace(record_root: Path, options: RecordingOptions) -> RecordingPaths:
    """Create one exclusive incomplete directory and fsynced owner record.

    The parent must already exist. Existing roots/unknown files are never claimed
    or modified. A failed attempt remains in place; no cleanup is implicit.
    """
    _absolute(record_root)
    if not isinstance(options, RecordingOptions):
        message = "recording options must be a validated RecordingOptions instance"
        raise TypeError(message)
    options = RecordingOptions.model_validate(options, strict=True)
    with ExitStack() as stack:
        _pin_ancestors(stack, record_root.parent)
        record_root.mkdir(exist_ok=True)
        root_identity = _pin(stack, record_root, directory=True)
        attempt_id = uuid4().hex
        created = datetime.now(UTC).strftime("%Y%m%dT%H%M%S%fZ")
        directory = record_root / f".incomplete-{attempt_id}"
        directory.mkdir()
        directory_identity = _pin(stack, directory, directory=True)
        metadata_path = directory / "owner.json"
        with metadata_path.open("xb") as owner:
            paths = RecordingPaths(
                attempt_id=attempt_id,
                created_at_utc=created,
                options=options,
                root=record_root,
                directory=directory,
                spool_path=directory / "float.capture",
                pcm_path=directory / "input.pcm",
                encoded_path=directory / "encoded.partial",
                decoded_path=directory / "decoded.pcm",
                metadata_path=metadata_path,
                final_path=record_root / f"recording-{created}-{attempt_id}.{options.format}",
                root_identity=root_identity,
                directory_identity=directory_identity,
                metadata_identity=_identity(os.fstat(owner.fileno())),
            )
            owner.write(_owner_bytes(paths))
            owner.flush()
            os.fsync(owner.fileno())
        _assert_pinned(paths, stack)
        return paths


def assert_recording_namespace(paths: RecordingPaths) -> None:
    """Validate the original ordinary root, directory and exact owner metadata."""
    with ExitStack() as stack:
        _assert_pinned(paths, stack)


@contextmanager
def hold_recording_namespace(paths: RecordingPaths) -> Iterator[None]:
    """Hold the existing Windows namespace pins until normal or exceptional exit.

    Exclusive stage leaves may be created while the original root, ancestors,
    attempt directory and owner record remain pinned. This does not pin stage
    contents or grant capture/codec permission. Other platforms currently check
    the namespace on entry without the Windows held-handle guarantee.
    """
    with ExitStack() as stack:
        _assert_pinned(paths, stack)
        yield


def publish_recording_no_replace(paths: RecordingPaths, encoded_identity: FileIdentity) -> Path:
    """Atomically add the final link; never replace, rename, remove or clean up.

    Windows pins the original source and ancestors through publication. Other
    platforms currently provide pre-link checks, not that Windows pin guarantee.
    Codec/payload and live capture permission remain caller duties. Successful
    os.link is the commit point; post-commit release errors retain that outcome.
    """
    if (
        type(encoded_identity) is not tuple
        or len(encoded_identity) != 2
        or any(type(value) is not int or value < 0 for value in encoded_identity)
    ):
        message = "encoded file identity must contain two strict nonnegative integers"
        raise ValueError(message)
    committed = False
    try:
        with ExitStack() as stack:
            _assert_pinned(paths, stack)
            if _pin(stack, paths.encoded_path, directory=False) != encoded_identity:
                message = "encoded recording identity changed"
                raise RecordingNamespaceError(message)
            os.link(paths.encoded_path, paths.final_path, follow_symlinks=False)
            committed = True
    except OSError as error:
        if committed:
            raise RecordingPublicationCommittedError(
                paths.final_path, encoded_identity, error
            ) from error
        raise
    else:
        return paths.final_path
