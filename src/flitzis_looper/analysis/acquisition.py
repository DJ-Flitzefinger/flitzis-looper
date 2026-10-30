"""Explicit resumable acquisition; never invoked by analysis or startup."""

from __future__ import annotations

import errno
import hashlib
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from contextlib import contextmanager
from dataclasses import dataclass
from typing import TYPE_CHECKING, BinaryIO, cast
from uuid import uuid4

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path

CHECKPOINT_URL = "https://cloud.cp.jku.at/public.php/dav/files/7ik4RrBKTS273gp/final0.ckpt"
MAX_CHECKPOINT_BYTES = 256 * 1024 * 1024
_CHUNK_BYTES = 1024 * 1024
_PERMANENT_HTTP = {400, 401, 403, 404, 410, 416}
_PERMANENT_LOCAL = {
    errno.ENOSPC,
    errno.EACCES,
    errno.EROFS,
    errno.EISDIR,
    errno.ENOTDIR,
    errno.EINVAL,
    errno.ENAMETOOLONG,
    errno.ELOOP,
    errno.EFBIG,
}


@dataclass(frozen=True, slots=True)
class AcquiredArtifact:
    """Measured bytes; acquisition alone does not install or deserialize a model."""

    path: Path
    sha256: str
    size_bytes: int


def _measure(path: Path) -> AcquiredArtifact:
    with path.open("rb") as stream:
        before = os.fstat(stream.fileno())
        checksum = hashlib.file_digest(stream, "sha256").hexdigest()
        after = os.fstat(stream.fileno())
    if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
        message = "artifact changed while verifying its identity"
        raise ValueError(message)
    return AcquiredArtifact(path, checksum, after.st_size)


def _validate_identity(artifact: AcquiredArtifact, sha256: str | None, size: int | None) -> None:
    if (sha256 is not None and artifact.sha256 != sha256) or (
        size is not None and artifact.size_bytes != size
    ):
        message = "download integrity check failed; retained bytes were not published"
        raise ValueError(message)


def _change_lock(stream: BinaryIO, *, unlock: bool) -> None:
    stream.seek(0)
    if sys.platform == "win32":
        import msvcrt  # noqa: PLC0415 -- Windows-only kernel file locking.

        msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK if unlock else msvcrt.LK_NBLCK, 1)
    else:
        import fcntl  # noqa: PLC0415 -- POSIX-only kernel file locking.

        fcntl.flock(stream.fileno(), fcntl.LOCK_UN if unlock else fcntl.LOCK_EX | fcntl.LOCK_NB)


@contextmanager
def _download_lock(destination: Path) -> Iterator[None]:
    """Kernel locks survive process interruption without stale ownership files."""
    lock = destination.with_name(destination.name + ".lock")
    with lock.open("a+b") as stream:
        if not lock.stat().st_size:
            stream.write(b"\0")
            stream.flush()
        delay = 20
        while True:
            try:
                _change_lock(stream, unlock=False)
                break
            except OSError as error:
                if error.errno not in {errno.EACCES, errno.EAGAIN, errno.EDEADLK}:
                    raise
                time.sleep(delay)
                delay = min(60, delay * 2)
        try:
            yield
        finally:
            _change_lock(stream, unlock=True)


def _validate_source(destination: Path, source_url: str, max_bytes: int) -> None:
    parsed = urllib.parse.urlsplit(source_url)
    if parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password:
        message = "checkpoint acquisition requires an HTTPS source without credentials"
        raise ValueError(message)
    if not destination.is_absolute() or max_bytes <= 0:
        message = "quarantine path must be absolute and byte limit must be positive"
        raise ValueError(message)


def _validate_local_paths(destination: Path) -> None:
    leaves = [destination]
    leaves.extend(
        destination.with_name(destination.name + suffix)
        for suffix in (".partial", ".source.json", ".lock")
    )
    paths = [*leaves, *destination.parents]
    if any(path.is_symlink() or path.is_junction() for path in paths):
        message = "download paths must not traverse links or reparse directories"
        raise ValueError(message)
    if any(path.exists() and not path.is_file() for path in leaves):
        message = "download target, partial and ownership sidecars must be regular files"
        raise ValueError(message)


def _bind_partial(destination: Path, identity: dict[str, str | int | None]) -> None:
    owner = destination.with_name(destination.name + ".source.json")
    if owner.exists():
        if json.loads(owner.read_text(encoding="utf-8")) != identity:
            message = "retained download belongs to a different URL or pin"
            raise ValueError(message)
    else:
        partial = destination.with_name(destination.name + ".partial")
        if partial.exists():
            message = "retained download has no ownership identity"
            raise ValueError(message)
        temporary = owner.with_name(owner.name + f".pending-{uuid4().hex}")
        with temporary.open("x", encoding="utf-8") as stream:
            json.dump(identity, stream, sort_keys=True)
        temporary.rename(owner)


def _check_prefix(stream: BinaryIO, partial: Path) -> None:
    with partial.open("rb") as previous:
        while expected := previous.read(_CHUNK_BYTES):
            found = bytearray()
            while len(found) < len(expected):
                chunk = stream.read(len(expected) - len(found))
                if not chunk:
                    message = "server response ended inside the retained prefix"
                    raise OSError(message)
                found.extend(chunk)
            if found != expected:
                message = "server payload differs from retained download prefix"
                raise ValueError(message)


def _resume_stream(
    stream: BinaryIO, partial: Path, offset: int, status: int, content_range: str
) -> None:
    if status == 206:
        if not content_range.startswith(f"bytes {offset}-"):
            message = "payload resumed at an unexpected offset"
            raise ValueError(message)
    elif status == 200 and offset:
        _check_prefix(stream, partial)
    elif status != 200:
        message = f"unexpected payload response: HTTP {status}"
        raise ValueError(message)


def _receive(source_url: str, partial: Path, max_bytes: int) -> None:
    offset = partial.stat().st_size if partial.exists() else 0
    if offset > max_bytes:
        message = "retained payload exceeded its byte limit"
        raise ValueError(message)
    headers = {"User-Agent": "FlitzisLooper-setup/2"}
    if offset:
        headers["Range"] = f"bytes={offset}-"
    request = urllib.request.Request(source_url, headers=headers)
    with urllib.request.urlopen(request, timeout=30) as response:
        if urllib.parse.urlsplit(response.url).scheme != "https":
            message = "checkpoint acquisition redirected away from HTTPS"
            raise ValueError(message)
        stream = cast("BinaryIO", response)
        status = getattr(response, "status", 200)
        content_range = response.headers.get("Content-Range", "") if status == 206 else ""
        _resume_stream(stream, partial, offset, status, content_range)
        with partial.open("ab") as output:
            size = offset
            while chunk := stream.read(min(_CHUNK_BYTES, max_bytes + 1 - size)):
                output.write(chunk)
                size += len(chunk)
                if size > max_bytes:
                    message = "checkpoint acquisition exceeded its byte limit"
                    raise ValueError(message)


def _download(
    destination: Path, source_url: str, max_bytes: int, expected_bytes: int | None
) -> Path:
    partial = destination.with_name(destination.name + ".partial")
    delay = 20
    while True:
        if (
            expected_bytes is not None
            and partial.exists()
            and partial.stat().st_size == expected_bytes
        ):
            return partial
        try:
            _receive(source_url, partial, max_bytes)
            _require_complete(partial, expected_bytes)
        except urllib.error.HTTPError as error:
            if error.code in _PERMANENT_HTTP:
                raise
        except OSError as error:
            if error.errno in _PERMANENT_LOCAL:
                raise
        else:
            return partial
        time.sleep(delay)
        delay = min(60, delay * 2)


def _require_complete(partial: Path, expected_bytes: int | None) -> None:
    if expected_bytes is not None and partial.stat().st_size < expected_bytes:
        message = "payload ended before its pinned byte length"
        raise OSError(message)


def acquire_to_quarantine(
    destination: Path,
    *,
    source_url: str = CHECKPOINT_URL,
    max_bytes: int = MAX_CHECKPOINT_BYTES,
    expected_sha256: str | None = None,
    expected_bytes: int | None = None,
) -> AcquiredArtifact:
    """Retain/resume HTTPS partials without a total deadline; publish verified bytes."""
    _validate_source(destination, source_url, max_bytes)
    _validate_local_paths(destination)
    if (expected_bytes is not None and not 0 < expected_bytes <= max_bytes) or (
        expected_sha256 is not None and re.fullmatch(r"[a-f0-9]{64}", expected_sha256) is None
    ):
        message = "download requires a positive pinned size and complete SHA256"
        raise ValueError(message)
    with _download_lock(destination):
        if destination.exists():
            if expected_sha256 is None:
                raise FileExistsError(destination)
            artifact = _measure(destination)
            _validate_identity(artifact, expected_sha256, expected_bytes)
            return artifact
        _bind_partial(
            destination,
            {
                "url": source_url,
                "max_bytes": max_bytes,
                "sha256": expected_sha256,
                "bytes": expected_bytes,
            },
        )
        partial = _download(destination, source_url, max_bytes, expected_bytes)
        artifact = _measure(partial)
        if not artifact.size_bytes:
            message = "checkpoint acquisition returned an empty artifact"
            raise ValueError(message)
        _validate_identity(artifact, expected_sha256, expected_bytes)
        partial.replace(destination)
        return AcquiredArtifact(destination, artifact.sha256, artifact.size_bytes)


def acquire_pinned_asset(
    destination: Path, *, source_url: str, sha256: str, size_bytes: int
) -> AcquiredArtifact:
    """Preserve an invalid installed model until its fully pinned replacement is ready."""
    _validate_source(destination, source_url, size_bytes)
    _validate_local_paths(destination)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with _download_lock(destination):
        if destination.is_file():
            artifact = _measure(destination)
            if (artifact.sha256, artifact.size_bytes) == (sha256, size_bytes):
                return artifact
        staged = destination.with_name(destination.name + ".download")
        artifact = acquire_to_quarantine(
            staged,
            source_url=source_url,
            max_bytes=size_bytes,
            expected_sha256=sha256,
            expected_bytes=size_bytes,
        )
        if destination.exists():
            destination.rename(destination.with_name(destination.name + f".corrupt-{uuid4().hex}"))
        staged.replace(destination)
        return AcquiredArtifact(destination, artifact.sha256, artifact.size_bytes)


def copy_to_quarantine(source: Path, destination: Path) -> AcquiredArtifact:
    """Copy selected local bytes without networking or trusting the source filename."""
    with source.open("rb") as stream:
        return _copy_quarantined(stream, destination, MAX_CHECKPOINT_BYTES)


def _copy_quarantined(stream: BinaryIO, destination: Path, max_bytes: int) -> AcquiredArtifact:
    if not destination.is_absolute() or max_bytes <= 0:
        message = "quarantine path must be absolute and byte limit must be positive"
        raise ValueError(message)
    digest = hashlib.sha256()
    size = 0
    with destination.open("xb") as output:
        while chunk := stream.read(min(_CHUNK_BYTES, max_bytes + 1 - size)):
            output.write(chunk)
            size += len(chunk)
            if size > max_bytes:
                message = "checkpoint acquisition exceeded its byte limit"
                raise ValueError(message)
            digest.update(chunk)
        if not size:
            message = "checkpoint acquisition returned an empty artifact"
            raise ValueError(message)
    return AcquiredArtifact(destination, digest.hexdigest(), size)
