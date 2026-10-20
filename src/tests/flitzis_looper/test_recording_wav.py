"""Independent WAV bytes exercise shared geometry, not codec or take completion."""

import os
import struct
import wave
from dataclasses import FrozenInstanceError
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.recording_wav import RecordingWavInfo, inspect_recording_wav

if TYPE_CHECKING:
    from pathlib import Path
    from typing import BinaryIO


def _chunk(name: bytes, payload: bytes) -> bytes:
    return name + struct.pack("<I", len(payload)) + payload + b"\0" * (len(payload) % 2)


def _fmt(tag: int, bits: int, channels: int, rate: int, *, extensible: bool = False) -> bytes:
    width = bits // 8
    raw = struct.pack(
        "<HHIIHH",
        0xFFFE if extensible else tag,
        channels,
        rate,
        rate * channels * width,
        channels * width,
        bits,
    )
    if extensible:
        raw += struct.pack("<HHI", 22, bits, 0)
        raw += struct.pack("<IHH", tag, 0, 0x0010) + bytes.fromhex("800000aa00389b71")
    return raw


def _riff(chunks: bytes) -> bytes:
    body = b"WAVE" + chunks
    return b"RIFF" + struct.pack("<I", len(body)) + body


def _container(
    fmt: bytes,
    pcm: bytes,
    frames: int,
    *,
    rf64: bool,
    before_data: bytes = b"",
) -> bytes:
    prefix = _chunk(b"fmt ", fmt) + before_data
    if not rf64:
        return _riff(prefix + _chunk(b"data", pcm))
    data = b"data\xff\xff\xff\xff" + pcm + b"\0" * (len(pcm) % 2)
    # Independent RF64 construction: ds64 counts full RIFF bytes minus 8,
    # data bytes without padding, and actual complete interleaved frames.
    riff_bytes = 4 + 36 + len(prefix) + len(data)
    sizes = _chunk(b"ds64", struct.pack("<QQQI", riff_bytes, len(pcm), frames, 0))
    return b"RF64\xff\xff\xff\xffWAVE" + sizes + prefix + data


@pytest.mark.parametrize(
    ("tag", "bits", "encoding"),
    [(3, 32, "float32"), (1, 16, "pcm16"), (1, 24, "pcm24")],
)
@pytest.mark.parametrize("rf64", [False, True])
@pytest.mark.parametrize("extensible", [False, True])
def test_selected_encodings_have_exact_riff_rf64_and_extensible_geometry(
    tmp_path: Path, tag: int, bits: int, encoding: str, *, rf64: bool, extensible: bool
) -> None:
    channels, rate, frames = 3, 44100, 7
    pcm = bytes(range(channels * (bits // 8))) * frames
    raw = _container(_fmt(tag, bits, channels, rate, extensible=extensible), pcm, frames, rf64=rf64)
    path = tmp_path / "recording.wav"
    path.write_bytes(raw)
    with path.open("rb") as handle:
        identity = os.fstat(handle.fileno())
        handle.seek(11)
        info = inspect_recording_wav(handle)
        assert handle.tell() == 11
        assert not handle.closed
        current = os.fstat(handle.fileno())
        assert (current.st_dev, current.st_ino, current.st_size) == (
            identity.st_dev,
            identity.st_ino,
            identity.st_size,
        )
        assert (info.encoding, info.channels, info.rate, info.frames) == (
            encoding,
            channels,
            rate,
            frames,
        )
        assert info.data_bytes == len(pcm)
        assert info.data_offset == (80 if rf64 else 44) + (24 if extensible else 0)
        assert info.file_bytes == len(raw)
        assert info.rf64 is rf64
        handle.seek(info.data_offset)
        assert handle.read(len(pcm)) == pcm


@pytest.mark.parametrize("rf64", [False, True])
def test_metadata_chunks_and_odd_pcm24_padding_keep_exact_data_extent(
    tmp_path: Path, *, rf64: bool
) -> None:
    pcm = bytes.fromhex("000080ffffff000000ffff7f000001")
    metadata = _chunk(b"JUNK", b"odd") + _chunk(b"LIST", b"INFO")
    raw = _container(_fmt(1, 24, 1, 48000), pcm, 5, rf64=rf64, before_data=metadata)
    path = tmp_path / "odd.wav"
    path.write_bytes(raw)
    with path.open("rb") as handle:
        info = inspect_recording_wav(handle)
    assert (info.encoding, info.frames, info.data_bytes) == ("pcm24", 5, 15)
    assert info.data_offset == (80 if rf64 else 44) + len(metadata)
    assert info.file_bytes == info.data_offset + 16


@pytest.mark.parametrize(("tag", "bits"), [(1, 8), (1, 32), (3, 64), (6, 8), (7, 8)])
def test_other_integer_float_and_compressed_encodings_are_rejected(
    tmp_path: Path, tag: int, bits: int
) -> None:
    path = tmp_path / "unsupported.wav"
    path.write_bytes(_container(_fmt(tag, bits, 1, 48000), bytes(bits // 8), 1, rf64=False))
    with path.open("rb") as handle:
        with pytest.raises(wave.Error):
            inspect_recording_wav(handle)
        assert not handle.closed


@pytest.mark.parametrize("rf64", [False, True])
@pytest.mark.parametrize("empty", [False, True])
def test_empty_and_partial_frames_are_rejected(tmp_path: Path, *, rf64: bool, empty: bool) -> None:
    pcm = b"" if empty else bytes(11)
    path = tmp_path / "incomplete.wav"
    path.write_bytes(_container(_fmt(1, 24, 2, 48000), pcm, 2, rf64=rf64))
    with path.open("rb") as handle:
        handle.seek(5)
        with pytest.raises(wave.Error):
            inspect_recording_wav(handle)
        assert handle.tell() == 5
        assert not handle.closed


@pytest.mark.parametrize("change", ["rate", "align", "valid_bits", "guid", "extension_size"])
def test_invalid_format_geometry_and_extensible_identity_are_rejected(
    tmp_path: Path, change: str
) -> None:
    fmt = bytearray(_fmt(3, 32, 2, 48000, extensible=True))
    if change == "rate":
        struct.pack_into("<I", fmt, 8, 1)
    elif change == "align":
        struct.pack_into("<H", fmt, 12, 7)
    elif change == "valid_bits":
        struct.pack_into("<H", fmt, 18, 24)
    elif change == "guid":
        fmt[39] ^= 1
    else:
        struct.pack_into("<H", fmt, 16, 20)
    path = tmp_path / "bad-fmt.wav"
    path.write_bytes(_container(bytes(fmt), bytes(8), 1, rf64=False))
    with path.open("rb") as handle, pytest.raises(wave.Error):
        inspect_recording_wav(handle)


@pytest.mark.parametrize(("channels", "rate"), [(0, 48000), (17, 48000), (2, 0), (2, 768001)])
def test_inherited_format_channel_and_rate_bounds_are_enforced(
    tmp_path: Path, channels: int, rate: int
) -> None:
    path = tmp_path / "bad-channel-rate.wav"
    path.write_bytes(_container(_fmt(3, 32, channels, rate), bytes(channels * 4), 1, rf64=False))
    with path.open("rb") as handle, pytest.raises(wave.Error):
        inspect_recording_wav(handle)


@pytest.mark.parametrize(
    "change",
    [
        "duplicate_fmt",
        "duplicate_data",
        "missing_fmt",
        "missing_data",
        "short_chunk",
        "short_fmt",
        "extra_eof",
        "short_eof",
    ],
)
def test_ambiguous_missing_truncated_and_mismatched_riff_is_rejected(
    tmp_path: Path, change: str
) -> None:
    fmt = _chunk(b"fmt ", _fmt(3, 32, 2, 48000))
    data = _chunk(b"data", bytes(8))
    chunks = fmt + data
    if change == "duplicate_fmt":
        chunks = fmt + fmt + data
    elif change == "duplicate_data":
        chunks += data
    elif change == "missing_fmt":
        chunks = data
    elif change == "missing_data":
        chunks = fmt
    elif change == "short_chunk":
        chunks = fmt + b"data\x08\0"
    elif change == "short_fmt":
        chunks = _chunk(b"fmt ", bytes(15)) + data
    raw = _riff(chunks)
    if change == "extra_eof":
        raw += b"x"
    elif change == "short_eof":
        raw = raw[:-1]
    path = tmp_path / "invalid-riff.wav"
    path.write_bytes(raw)
    with path.open("rb") as handle, pytest.raises((wave.Error, EOFError)):
        inspect_recording_wav(handle)


@pytest.mark.parametrize(
    "change",
    [
        "riff",
        "data",
        "frames",
        "table",
        "data_sentinel",
        "riff_sentinel",
        "missing_ds64",
        "huge_claim",
    ],
)
def test_rf64_requires_actual_ds64_extent_frames_and_sentinels(tmp_path: Path, change: str) -> None:
    raw = bytearray(_container(_fmt(3, 32, 2, 48000), bytes(16), 2, rf64=True))
    if change == "riff":
        struct.pack_into("<Q", raw, 20, len(raw) - 7)
    elif change == "data":
        struct.pack_into("<Q", raw, 28, 15)
    elif change == "frames":
        struct.pack_into("<Q", raw, 36, 3)
    elif change == "table":
        struct.pack_into("<I", raw, 44, 1)
    elif change == "data_sentinel":
        struct.pack_into("<I", raw, 76, 16)
    elif change == "riff_sentinel":
        struct.pack_into("<I", raw, 4, len(raw) - 8)
    elif change == "missing_ds64":
        raw[12:16] = b"JUNK"
    else:
        # A large u64 declaration cannot stand in for real >RIFF payload bytes.
        struct.pack_into("<Q", raw, 20, (1 << 32) + len(raw) - 8)
    path = tmp_path / "invalid-rf64.wav"
    path.write_bytes(raw)
    with path.open("rb") as handle, pytest.raises((wave.Error, EOFError)):
        inspect_recording_wav(handle)


@pytest.mark.parametrize("change", ["duplicate", "short", "misplaced"])
def test_rf64_ds64_is_one_complete_initial_chunk(tmp_path: Path, change: str) -> None:
    raw = bytearray(_container(_fmt(3, 32, 2, 48000), bytes(16), 2, rf64=True))
    if change == "duplicate":
        raw[48:48] = raw[12:48]
        struct.pack_into("<Q", raw, 20, len(raw) - 8)
    elif change == "short":
        struct.pack_into("<I", raw, 16, 27)
    else:
        # The same otherwise complete ds64 chunk follows fmt instead of WAVE.
        raw = raw[:12] + raw[48:72] + raw[12:48] + raw[72:]
    path = tmp_path / "bad-ds64.wav"
    path.write_bytes(raw)
    with path.open("rb") as handle, pytest.raises(wave.Error):
        inspect_recording_wav(handle)


class _ReadTrace:
    """Observe reads on a genuine held file without substituting its geometry."""

    def __init__(self, handle: BinaryIO) -> None:
        self.handle = handle
        self.reads: list[tuple[int, int]] = []

    def read(self, size: int = -1) -> bytes:
        assert 0 <= size <= 40, "inspection must request only bounded header bytes"
        self.reads.append((self.handle.tell(), size))
        return self.handle.read(size)

    def seek(self, offset: int, whence: int = os.SEEK_SET) -> int:
        return self.handle.seek(offset, whence)

    def tell(self) -> int:
        return self.handle.tell()


def test_real_file_trace_reads_only_bounded_headers_and_skips_large_chunks(
    tmp_path: Path,
) -> None:
    # Header/data/metadata all occupy real bytes. This is a small bounded read
    # proof, not the separate actual >RIFF disk/container acceptance gate.
    fmt = _fmt(3, 32, 2, 48000) + bytes((1 << 20) - 16)
    metadata = _chunk(b"JUNK", bytes(1 << 20))
    pcm = bytes(8192 * 8)
    path = tmp_path / "bounded-headers.wav"
    path.write_bytes(_container(fmt, pcm, 8192, rf64=True, before_data=metadata))
    with path.open("rb") as handle:
        trace = _ReadTrace(handle)
        info = inspect_recording_wav(cast("BinaryIO", trace))
        assert not handle.closed
        assert handle.tell() == 0
        assert info.frames == 8192
        assert sum(size for _, size in trace.reads) <= 256
        assert all(
            position + size <= info.data_offset or position >= info.data_offset + info.data_bytes
            for position, size in trace.reads
        )
        handle.seek(info.data_offset)
        assert handle.read(8) == pcm[:8]


def test_geometry_result_is_immutable_and_does_not_certify_float_payload(
    tmp_path: Path,
) -> None:
    path = tmp_path / "geometry-only.wav"
    path.write_bytes(
        _container(
            _fmt(3, 32, 2, 48000),
            struct.pack("<ff", float("nan"), float("inf")),
            1,
            rf64=False,
        )
    )
    with path.open("rb") as handle:
        info = inspect_recording_wav(handle)
    assert isinstance(info, RecordingWavInfo)
    assert (info.encoding, info.frames) == ("float32", 1)
    field = "frames"
    with pytest.raises(FrozenInstanceError):
        setattr(info, field, 2)
