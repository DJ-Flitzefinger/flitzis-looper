"""Fixed-block payload checks on caller-owned stable recording files."""

import hashlib
import math
import struct
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable
    from typing import BinaryIO

_BLOCK_BYTES = 64 * 1024


def hash_recording_extent(
    handle: BinaryIO,
    count: int,
    *,
    finite_float: bool = False,
    checkpoint: Callable[[], None] | None = None,
) -> str:
    """Read exactly count bytes from the current position without closing/seeking.

    Float checks require complete float32 values. File identity, stability and
    whether anything may follow the selected extent remain the caller's duty.
    """
    if type(count) is not int or count < 0 or (finite_float and count % 4):
        message = "invalid recording payload extent"
        raise ValueError(message)
    digest = hashlib.sha256()
    while count:
        if checkpoint is not None:
            checkpoint()
        wanted = min(count, _BLOCK_BYTES)
        block = bytearray()
        while len(block) < wanted:
            chunk = handle.read(wanted - len(block))
            if chunk is None or len(chunk) > wanted - len(block):
                message = "recording payload read made invalid progress"
                raise OSError(message)
            if not chunk:
                message = "recording payload is truncated"
                raise EOFError(message)
            block.extend(chunk)
        if finite_float and any(
            not math.isfinite(value) for (value,) in struct.iter_unpack("<f", block)
        ):
            message = "decoded recording contains a nonfinite sample"
            raise ValueError(message)
        digest.update(block)
        count -= wanted
    if checkpoint is not None:
        checkpoint()
    return digest.hexdigest()
