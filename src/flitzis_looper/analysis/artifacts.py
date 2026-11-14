"""Pinned Beat This ONNX identity and bounded artifact hashing."""

import hashlib
from typing import TYPE_CHECKING, Annotated

from pydantic import Field

from flitzis_looper.analysis.contracts import BeatModelIdentity

if TYPE_CHECKING:
    from pathlib import Path

NATIVE_MODEL = BeatModelIdentity(
    sha256="46bfb2c1a1f1e5c1d8786d0d9462f285971ff7fa8a592ac5f4ab609698bd44ca",
    frontend_id="beat-this-1.1.0-native-soxr-hq-logmel-v1",
    environment_id="native-onnxruntime-1.24.4-api24-windows-x64-cpu-v1",
)
_METADATA_LIMIT = 4 * 1024 * 1024
type Digest = Annotated[str, Field(pattern=r"^[0-9a-f]{64}$")]


def file_digest(path: Path, *, max_bytes: int = _METADATA_LIMIT) -> str:
    """Hash one bounded local metadata/artifact file without importing optional code."""
    digest = hashlib.sha256()
    count = 0
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            count += len(chunk)
            if count > max_bytes:
                msg = "artifact_file_limit"
                raise ValueError(msg)
            digest.update(chunk)
    return digest.hexdigest()
