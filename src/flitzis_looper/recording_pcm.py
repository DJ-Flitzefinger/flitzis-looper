"""Bounded off-realtime conversion of a caller-owned float32 recording payload.

This module creates no files and grants no take, Native or publication authority.
The caller owns distinct source/output identities, file lifetime and any partial
output after an error. Conversion neither flushes nor closes either handle.
"""

import hashlib
import math
import struct
from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable
    from typing import BinaryIO

    from flitzis_looper.recording_options import WavEncoding

_BLOCK_BYTES = 64 * 1024
_U64_MAX = (1 << 64) - 1
_OUTPUT_WIDTH = {"float32": 4, "pcm16": 2, "pcm24": 3}


@dataclass(frozen=True, slots=True)
class RecordingPcmStats:
    """Complete payload measurements; container and take checks remain separate."""

    frames: int
    channels: int
    peak_abs: float
    source_overrange_samples: int
    source_overrange_frames: int
    source_payload_sha256: str
    output_payload_sha256: str


def _validate_geometry(frames: int, channels: int, encoding: WavEncoding) -> int:
    if type(frames) is not int or frames <= 0:
        message = "recording PCM requires a positive integer frame count"
        raise ValueError(message)
    if type(channels) is not int or not 1 <= channels <= 16:
        message = "recording PCM requires an integer channel count between 1 and 16"
        raise ValueError(message)
    if type(encoding) is not str or encoding not in _OUTPUT_WIDTH:
        message = "recording PCM requires float32, pcm16 or pcm24 output"
        raise ValueError(message)
    payload_bytes = frames * channels * 4
    if payload_bytes > _U64_MAX:
        message = "recording float32 payload exceeds its unsigned 64-bit extent"
        raise ValueError(message)
    return payload_bytes


def _read_block(source: BinaryIO, count: int) -> bytearray:
    payload = bytearray(count)
    view = memoryview(payload)
    position = 0
    while position < count:
        received = source.read(count - position)
        if received is None or len(received) > count - position:
            message = "recording PCM read made invalid progress"
            raise OSError(message)
        if not received:
            message = "recording float32 payload is truncated"
            raise EOFError(message)
        view[position : position + len(received)] = received
        position += len(received)
    return payload


@dataclass(slots=True)
class _Measurements:
    peak: float = 0.0
    outside_samples: int = 0
    outside_frames: int = 0

    def observe(self, payload: bytearray, channels: int) -> None:
        frame_outside = False
        for index, (sample,) in enumerate(struct.iter_unpack("<f", payload), start=1):
            if not math.isfinite(sample):
                message = "recording float32 payload contains a nonfinite sample"
                raise ValueError(message)
            magnitude = abs(sample)
            self.peak = max(self.peak, magnitude)
            if magnitude > 1.0:
                self.outside_samples += 1
                frame_outside = True
            if index % channels == 0:
                self.outside_frames += int(frame_outside)
                frame_outside = False


def _integer_payload(payload: bytearray, width: int) -> bytearray:
    output = bytearray(len(payload) // 4 * width)
    scale = 1 << (width * 8 - 1)
    for index, (sample,) in enumerate(struct.iter_unpack("<f", payload)):
        quantized = round(min(1.0, max(-1.0, sample)) * scale)
        quantized = min(scale - 1, max(-scale, quantized))
        offset = index * width
        if width == 2:
            struct.pack_into("<h", output, offset, quantized)
        else:
            output[offset] = quantized & 0xFF
            output[offset + 1] = (quantized >> 8) & 0xFF
            output[offset + 2] = (quantized >> 16) & 0xFF
    return output


def _write_block(output: BinaryIO, payload: bytearray) -> None:
    view = memoryview(payload)
    while view:
        written = output.write(view)
        if type(written) is not int or not 0 < written <= len(view):
            message = "recording PCM write made invalid progress"
            raise OSError(message)
        view = view[written:]


def convert_recording_pcm(
    source: BinaryIO,
    output: BinaryIO,
    *,
    frames: int,
    channels: int,
    encoding: WavEncoding,
    checkpoint: Callable[[], None] | None = None,
) -> RecordingPcmStats:
    """Convert exactly one finite little-endian float32 payload to selected PCM.

    Source starts at its payload and must end immediately after the expected
    frames. The caller supplies a distinct output positioned for payload writes;
    aliasing file identities through different handles is the caller's concern.
    Reads are frame-aligned blocks of at most 64 KiB, with bounded short I/O
    retries. Checkpoints precede each block and follow the final EOF check.

    Float32 retains exact source bits, including signed zero and overrange.
    Integers use fixed full scale, ties to even and signed-range saturation,
    without dither, normalization, rate conversion or channel changes.
    """
    remaining = _validate_geometry(frames, channels, encoding)
    if source is output:
        message = "recording PCM source and output must be distinct handles"
        raise ValueError(message)
    block_bytes = _BLOCK_BYTES // (channels * 4) * channels * 4
    source_hash = hashlib.sha256()
    output_hash = hashlib.sha256()
    measurements = _Measurements()
    while remaining:
        if checkpoint is not None:
            checkpoint()
        payload = _read_block(source, min(block_bytes, remaining))
        measurements.observe(payload, channels)
        source_hash.update(payload)
        converted = (
            payload if encoding == "float32" else _integer_payload(payload, _OUTPUT_WIDTH[encoding])
        )
        _write_block(output, converted)
        output_hash.update(converted)
        remaining -= len(payload)
    extra = source.read(1)
    if extra is None:
        message = "recording PCM EOF read made no progress"
        raise OSError(message)
    if extra:
        message = "recording float32 payload has extra bytes after its expected frames"
        raise ValueError(message)
    if checkpoint is not None:
        checkpoint()
    return RecordingPcmStats(
        frames=frames,
        channels=channels,
        peak_abs=measurements.peak,
        source_overrange_samples=measurements.outside_samples,
        source_overrange_frames=measurements.outside_frames,
        source_payload_sha256=source_hash.hexdigest(),
        output_payload_sha256=output_hash.hexdigest(),
    )
