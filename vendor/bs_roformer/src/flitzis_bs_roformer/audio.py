"""Read the complete BS-RoFormer stereo float PCM in model chunks."""

from typing import BinaryIO

import torch
from torch import Tensor

from flitzis_looper.bs_decode import FRAME_BYTES, decode_source

__all__ = ["decode_source"]


def _read_frames(handle: BinaryIO, start: int, count: int, *, reverse: bool = False) -> Tensor:
    handle.seek(start * FRAME_BYTES)
    data = bytearray(handle.read(count * FRAME_BYTES))
    if len(data) != count * FRAME_BYTES:
        msg = "Decoded source changed or contains incomplete PCM"
        raise ValueError(msg)
    result = torch.frombuffer(data, dtype=torch.float32).reshape(count, 2).transpose(0, 1)
    if reverse:
        return result.flip(1)
    return result.contiguous()


def read_virtual_chunk(
    handle: BinaryIO,
    start: int,
    length: int,
    source_frames: int,
    border: int,
) -> Tensor:
    """Read a chunk from the upstream reflection-padded virtual stereo source."""
    segments: list[Tensor] = []
    end = start + length
    position = start
    if position < border:
        stop = min(end, border)
        segments.append(_read_frames(handle, border - stop + 1, stop - position, reverse=True))
        position = stop
    if position < end and position < border + source_frames:
        stop = min(end, border + source_frames)
        segments.append(_read_frames(handle, position - border, stop - position))
        position = stop
    if position < end:
        original_start = 2 * source_frames + border - 1 - end
        segments.append(_read_frames(handle, original_start, end - position, reverse=True))
    if not segments:
        msg = "Cannot read an empty model chunk"
        raise ValueError(msg)
    result = segments[0] if len(segments) == 1 else torch.cat(segments, dim=1)
    if not bool(torch.isfinite(result).all()):
        msg = "Decoded source contains non-finite PCM"
        raise ValueError(msg)
    return result
