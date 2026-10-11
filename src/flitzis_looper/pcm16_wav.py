"""Positioned PCM WAV I/O with RIFF compatibility and 64-bit RF64 extents.

Small PCM16 outputs use the exact 44-byte header emitted by ``wave``. Larger
outputs use RF64's ds64 chunk; the extent is disk size, never a read allocation.
The reader retains one handle and validates all chunks and the actual final EOF.
This module has no controller or Native dependency for offline worker imports.
"""

import hashlib
import os
import struct
import wave
from contextlib import suppress
from dataclasses import dataclass
from typing import TYPE_CHECKING, BinaryIO, Self

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path
    from types import TracebackType

RIFF_MAX_DATA_BYTES = 0xFFFF_FFFF - 36
_U64_MAX = 0xFFFF_FFFF_FFFF_FFFF
_PCM_SUBTYPE = bytes.fromhex("0100000000001000800000aa00389b71")
_VERIFY_BYTES = 64 * 1024


def discard_owned_file(path: Path, identity: tuple[int, int]) -> None:
    """Retire an attempt leaf only if its captured file identity still matches."""
    with suppress(OSError):
        current = path.stat(follow_symlinks=False)
        if (current.st_dev, current.st_ino) == identity:
            path.unlink()


@dataclass(frozen=True, slots=True)
class _Geometry:
    channels: int
    rate: int
    width: int
    data_start: int
    data_bytes: int
    file_bytes: int


def _exact(handle: BinaryIO, count: int) -> bytes:
    value = handle.read(count)
    if len(value) != count:
        msg = "incomplete WAV header or PCM data"
        raise EOFError(msg)
    return value


def _format(payload: bytes) -> tuple[int, int, int]:
    if len(payload) < 16:
        msg = "incomplete WAV format"
        raise wave.Error(msg)
    tag, channels, rate, byte_rate, align, bits = struct.unpack_from("<HHIIHH", payload)
    if tag == 0xFFFE:
        if (
            len(payload) < 40
            or struct.unpack_from("<H", payload, 16)[0] < 22
            or struct.unpack_from("<H", payload, 18)[0] != bits
            or payload[24:40] != _PCM_SUBTYPE
        ):
            msg = "unsupported extensible WAV format"
            raise wave.Error(msg)
    elif tag != 1:
        msg = "WAV must contain integer PCM"
        raise wave.Error(msg)
    if not channels or not rate or not bits or bits % 8:
        msg = "invalid WAV PCM geometry"
        raise wave.Error(msg)
    if align != channels * (bits // 8) or byte_rate != rate * align:
        msg = "invalid WAV PCM block alignment or byte rate"
        raise wave.Error(msg)
    return channels, rate, bits // 8


def _file_extent(handle: BinaryIO) -> tuple[bool, int]:
    header = _exact(handle, 12)
    marker, declared, form = struct.unpack("<4sI4s", header)
    if marker not in {b"RIFF", b"RF64"} or form != b"WAVE":
        msg = "file does not start with RIFF or RF64 WAVE"
        raise wave.Error(msg)
    handle.seek(0, os.SEEK_END)
    actual = handle.tell()
    handle.seek(12)
    large = marker == b"RF64"
    if large and declared != 0xFFFF_FFFF:
        msg = "RF64 must use the RIFF size sentinel"
        raise wave.Error(msg)
    extent = actual if large else declared + 8
    if extent != actual:
        msg = "WAV declared extent does not match EOF"
        raise wave.Error(msg)
    return large, extent


@dataclass(slots=True)
class _Chunks:
    large: bool
    extent: int
    shape: tuple[int, int, int] | None = None
    data: tuple[int, int] | None = None
    sizes: tuple[int, int] | None = None

    def _ds64(self, handle: BinaryIO, begin: int, length: int) -> None:
        if not self.large or self.sizes is not None or begin != 20 or length != 28:
            msg = "unsupported RF64 ds64 chunk"
            raise wave.Error(msg)
        riff_bytes, data_bytes, frames, entries = struct.unpack("<QQQI", _exact(handle, 28))
        if entries or riff_bytes + 8 != self.extent:
            msg = "RF64 ds64 extent does not match EOF"
            raise wave.Error(msg)
        self.sizes = (data_bytes, frames)

    def _fmt(self, handle: BinaryIO, length: int) -> None:
        if self.shape is not None or length < 16:
            msg = "invalid or repeated WAV format"
            raise wave.Error(msg)
        self.shape = _format(_exact(handle, min(length, 40)))

    def _data(self, begin: int, length: int) -> int:
        if self.data is not None:
            msg = "repeated WAV data chunk"
            raise wave.Error(msg)
        if self.large:
            if self.sizes is None or length != 0xFFFF_FFFF:
                msg = "RF64 data must use its ds64 size"
                raise wave.Error(msg)
            length = self.sizes[0]
        self.data = (begin, length)
        return length

    def scan_chunk(self, handle: BinaryIO) -> None:
        chunk, length = struct.unpack("<4sI", _exact(handle, 8))
        begin = handle.tell()
        if chunk == b"ds64":
            self._ds64(handle, begin, length)
        elif chunk == b"fmt ":
            self._fmt(handle, length)
        elif chunk == b"data":
            length = self._data(begin, length)
        elif length == 0xFFFF_FFFF:
            msg = "unsupported sentinel WAV chunk"
            raise wave.Error(msg)
        end = begin + length + (length & 1)
        if end > self.extent:
            msg = "incomplete WAV chunk or PCM data"
            raise wave.Error(msg)
        handle.seek(end)

    def geometry(self) -> _Geometry:
        if self.shape is None or self.data is None or (self.large and self.sizes is None):
            msg = "incomplete WAV format or data"
            raise wave.Error(msg)
        channels, rate, width = self.shape
        data_start, data_bytes = self.data
        align = channels * width
        if data_bytes % align or (self.sizes is not None and self.sizes[1] != data_bytes // align):
            msg = "WAV data does not contain its declared complete frames"
            raise wave.Error(msg)
        return _Geometry(channels, rate, width, data_start, data_bytes, self.extent)


def _geometry(handle: BinaryIO) -> _Geometry:
    large, extent = _file_extent(handle)
    chunks = _Chunks(large, extent)
    while handle.tell() < extent:
        chunks.scan_chunk(handle)
    return chunks.geometry()


class PcmWavReader:
    """Read RIFF/RF64 PCM through one retained positioned file handle."""

    def __init__(self, path: Path) -> None:
        self._handle = path.open("rb")
        try:
            metadata = os.fstat(self._handle.fileno())
            self._identity = (metadata.st_dev, metadata.st_ino)
            self._shape = _geometry(self._handle)
        except EOFError, MemoryError, OSError, wave.Error:
            self._handle.close()
            raise
        self._position = 0

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self._handle.close()

    def getnchannels(self) -> int:
        """Return the interleaved channel count."""
        return self._shape.channels

    def getframerate(self) -> int:
        """Return the integer sample rate in Hz."""
        return self._shape.rate

    def getsampwidth(self) -> int:
        """Return the stored PCM sample width in bytes."""
        return self._shape.width

    def getnframes(self) -> int:
        """Return the complete 64-bit frame extent."""
        return self._shape.data_bytes // (self._shape.channels * self._shape.width)

    @property
    def file_identity(self) -> tuple[int, int]:
        """Return the actual identity captured from this retained reader handle."""
        return self._identity

    def setpos(self, position: int) -> None:
        """Position the next bounded read at a complete frame."""
        if not 0 <= position <= self.getnframes():
            msg = "position outside WAV PCM extent"
            raise wave.Error(msg)
        self._position = position

    def readframes(self, frames: int) -> bytes:
        """Read only the requested frames, capped at the declared final frame."""
        if frames < 0:
            msg = "bounded WAV reads require a nonnegative frame count"
            raise wave.Error(msg)
        count = min(frames, self.getnframes() - self._position)
        align = self._shape.channels * self._shape.width
        self._handle.seek(self._shape.data_start + self._position * align)
        raw = self._handle.read(count * align)
        self._position += len(raw) // align
        return raw

    def verify(
        self, *, chunk_bytes: int = _VERIFY_BYTES, checkpoint: Callable[[], None] | None = None
    ) -> str:
        """Hash the entire retained file and check current complete geometry/EOF."""
        if not 0 < chunk_bytes <= _VERIFY_BYTES:
            msg = "WAV verification block must be between 1 and 65536 bytes"
            raise ValueError(msg)
        if checkpoint is not None:
            checkpoint()
        self._handle.seek(0)
        if _geometry(self._handle) != self._shape:
            msg = "WAV geometry changed during processing"
            raise wave.Error(msg)
        self._handle.seek(0)
        digest = hashlib.sha256()
        remaining = self._shape.file_bytes
        view = memoryview(bytearray(min(chunk_bytes, remaining)))
        while remaining:
            if checkpoint is not None:
                checkpoint()
            count = min(len(view), remaining)
            offset = 0
            while offset < count:
                received = self._handle.readinto(view[offset:count])
                if received is None or received <= 0:
                    msg = "incomplete WAV header or PCM data"
                    raise EOFError(msg)
                offset += received
            digest.update(view[:count])
            remaining -= count
        if checkpoint is not None:
            checkpoint()
        if self._handle.read(1):
            msg = "WAV has extra data after the declared EOF"
            raise wave.Error(msg)
        return digest.hexdigest()


def _pcm16_header(rate: int, channels: int, frames: int, *, force_rf64: bool) -> bytes:
    if not (
        isinstance(rate, int)
        and isinstance(channels, int)
        and isinstance(frames, int)
        and 0 < channels <= 0xFFFF // 2
        and 0 < rate <= 0xFFFF_FFFF
        and rate * channels * 2 <= 0xFFFF_FFFF
        and frames > 0
    ):
        msg = "invalid PCM16 WAV geometry"
        raise ValueError(msg)
    data_bytes = frames * channels * 2
    if data_bytes > _U64_MAX - 72:
        msg = "PCM16 WAV extent exceeds the RF64 format"
        raise ValueError(msg)
    fmt = struct.pack(
        "<4sIHHIIHH", b"fmt ", 16, 1, channels, rate, rate * channels * 2, channels * 2, 16
    )
    if force_rf64 or data_bytes > RIFF_MAX_DATA_BYTES:
        ds64 = struct.pack("<4sIQQQI", b"ds64", 28, data_bytes + 72, data_bytes, frames, 0)
        return b"RF64\xff\xff\xff\xffWAVE" + ds64 + fmt + b"data\xff\xff\xff\xff"
    return (
        struct.pack("<4sI4s", b"RIFF", data_bytes + 36, b"WAVE")
        + fmt
        + struct.pack("<4sI", b"data", data_bytes)
    )


class Pcm16WavWriter:
    """Write a known complete PCM16 extent in bounded blocks without header patching."""

    def __init__(
        self,
        path: Path,
        *,
        sample_rate_hz: int,
        channels: int,
        frames: int,
        force_rf64: bool = False,
    ) -> None:
        header = _pcm16_header(sample_rate_hz, channels, frames, force_rf64=force_rf64)
        self._expected_bytes = frames * channels * 2
        self._align = channels * 2
        self._written = 0
        self._digest = hashlib.sha256(header)
        self._path = path
        self._handle = path.open("xb")
        self._identity: tuple[int, int] | None = None
        try:
            metadata = os.fstat(self._handle.fileno())
            self._identity = (metadata.st_dev, metadata.st_ino)
            self._write(header)
        except MemoryError, OSError:
            self._handle.close()
            self.discard_owned_output()
            raise

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        try:
            if exc_type is None:
                self.finish()
        finally:
            self._handle.close()

    def _write(self, payload: bytes | bytearray) -> None:
        view = memoryview(payload)
        while view:
            count = self._handle.write(view)
            if count is None or count <= 0:
                msg = "short PCM16 WAV write"
                raise OSError(msg)
            view = view[count:]

    def writeframesraw(self, payload: bytes | bytearray) -> None:
        """Append complete frames without exceeding the promised disk extent."""
        if len(payload) % self._align or self._written + len(payload) > self._expected_bytes:
            msg = "PCM16 WAV write exceeds its complete frame extent"
            raise ValueError(msg)
        self._write(payload)
        self._digest.update(payload)
        self._written += len(payload)

    @property
    def file_digest(self) -> str:
        """Return the complete header/payload digest once every frame was written."""
        if self._written != self._expected_bytes:
            msg = "PCM16 WAV output is incomplete"
            raise ValueError(msg)
        return self._digest.hexdigest()

    @property
    def file_identity(self) -> tuple[int, int]:
        """Return the exclusive output handle's captured file identity."""
        if self._identity is None:
            msg = "PCM16 WAV output has no captured file identity"
            raise ValueError(msg)
        return self._identity

    def finish(self) -> None:
        """Check completeness and flush durable bytes before independent reopening."""
        _ = self.file_digest
        self._handle.flush()
        os.fsync(self._handle.fileno())

    def discard_owned_output(self) -> None:
        """Remove only this writer's unpublished leaf after all handles return."""
        if self._identity is not None:
            discard_owned_file(self._path, self._identity)
