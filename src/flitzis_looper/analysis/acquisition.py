"""Bounded explicit acquisition; this module is never invoked by analysis or startup."""

import hashlib
import time
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import TYPE_CHECKING, BinaryIO

if TYPE_CHECKING:
    from pathlib import Path

CHECKPOINT_URL = "https://cloud.cp.jku.at/public.php/dav/files/7ik4RrBKTS273gp/final0.ckpt"
MAX_CHECKPOINT_BYTES = 256 * 1024 * 1024
_CHUNK_BYTES = 1024 * 1024


@dataclass(frozen=True, slots=True)
class AcquiredArtifact:
    """Quarantined bytes and measured digest, with no trust or installation implied."""

    path: Path
    sha256: str
    size_bytes: int


def acquire_to_quarantine(
    destination: Path,
    *,
    source_url: str = CHECKPOINT_URL,
    max_bytes: int = MAX_CHECKPOINT_BYTES,
) -> AcquiredArtifact:
    """Explicitly download bounded HTTPS bytes to a new file for subsequent verification.

    This operation does not accept a model, load pickle data, install dependencies or
    change an existing installation. On failure its incomplete output is removed.
    """
    parsed = urllib.parse.urlsplit(source_url)
    if parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password:
        msg = "checkpoint acquisition requires an HTTPS source without credentials"
        raise ValueError(msg)
    request = urllib.request.Request(source_url, headers={"User-Agent": "FlitzisLooper-setup/1"})
    with urllib.request.urlopen(request, timeout=30) as response:
        if urllib.parse.urlsplit(response.url).scheme != "https":
            msg = "checkpoint acquisition redirected away from HTTPS"
            raise ValueError(msg)
        return _copy_quarantined(response, destination, max_bytes)


def copy_to_quarantine(source: Path, destination: Path) -> AcquiredArtifact:
    """Copy a caller-selected local checkpoint without networking or trusting its name."""
    with source.open("rb") as stream:
        return _copy_quarantined(stream, destination, MAX_CHECKPOINT_BYTES)


def _copy_quarantined(stream: BinaryIO, destination: Path, max_bytes: int) -> AcquiredArtifact:
    if not destination.is_absolute() or max_bytes <= 0:
        msg = "quarantine path must be absolute and byte limit must be positive"
        raise ValueError(msg)
    digest = hashlib.sha256()
    size = 0
    complete = False
    deadline = time.monotonic() + 300
    # Exclusive creation ensures failure cleanup cannot remove a pre-existing file.
    with destination.open("xb") as output:
        try:
            while chunk := stream.read(min(_CHUNK_BYTES, max_bytes + 1 - size)):
                size += len(chunk)
                if size > max_bytes or time.monotonic() > deadline:
                    msg = "checkpoint acquisition exceeded its byte or time limit"
                    raise ValueError(msg)
                digest.update(chunk)
                output.write(chunk)
            if not size:
                msg = "checkpoint acquisition returned an empty artifact"
                raise ValueError(msg)
            complete = True
        finally:
            if not complete:
                output.close()
                destination.unlink()
    return AcquiredArtifact(destination, digest.hexdigest(), size)
