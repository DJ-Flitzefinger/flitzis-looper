"""Decode arbitrary FFmpeg-readable inputs to stereo float PCM on disk."""

import json
import math
import subprocess
from typing import TYPE_CHECKING, BinaryIO

import torch
from torch import Tensor

from flitzis_bs_roformer.model import SAMPLE_RATE

if TYPE_CHECKING:
    from pathlib import Path

FRAME_BYTES = 8


def decode_source(source: Path, destination: Path) -> int:
    """Probe before decoding, then validate the complete stereo float32 file shape.

    No full-track PCM allocation occurs here. FFmpeg writes its bounded decode
    buffers directly to a private temporary file; the inference reader reads
    at most one model chunk at a time. All conversion and resampling is offline.
    """
    probe = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=sample_rate,channels,duration:format=duration",
            "-of",
            "json",
            str(source),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(probe.stdout)
    streams = metadata.get("streams", [])
    if len(streams) != 1:
        msg = "Source must contain a decodable audio stream"
        raise ValueError(msg)
    stream = streams[0]
    channels = int(stream.get("channels", 0))
    rate = int(stream.get("sample_rate", 0))
    duration = float(stream.get("duration", metadata.get("format", {}).get("duration", 0)))
    if not 1 <= channels <= 64 or rate <= 0 or not math.isfinite(duration) or duration <= 0:
        msg = "Source has invalid audio metadata"
        raise ValueError(msg)
    channel_conversion = ["-af", "pan=stereo|c0=c0|c1=c0"] if channels == 1 else []
    subprocess.run(
        [
            "ffmpeg",
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-i",
            str(source),
            "-map",
            "0:a:0",
            "-vn",
            "-threads",
            "1",
            *channel_conversion,
            "-ac",
            "2",
            "-ar",
            str(SAMPLE_RATE),
            "-c:a",
            "pcm_f32le",
            "-f",
            "f32le",
            str(destination),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    decoded_bytes = destination.stat().st_size
    if decoded_bytes == 0 or decoded_bytes % FRAME_BYTES:
        msg = "Decoded source has incomplete stereo float32 PCM"
        raise ValueError(msg)
    return decoded_bytes // FRAME_BYTES


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
