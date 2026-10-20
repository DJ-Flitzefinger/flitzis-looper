"""Independent sample and I/O oracles for bounded offline recording conversion."""

import hashlib
import io
import os
import struct
from dataclasses import FrozenInstanceError
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.recording_pcm import RecordingPcmStats, convert_recording_pcm

if TYPE_CHECKING:
    from pathlib import Path
    from typing import BinaryIO

    from flitzis_looper.recording_options import WavEncoding


def _float_payload(values: tuple[float, ...]) -> bytes:
    return struct.pack(f"<{len(values)}f", *values)


def test_pcm16_lsb_ties_endpoints_and_overrange_have_hand_authored_values() -> None:
    source_bytes = _float_payload((
        -2.0,
        -1.0,
        -32767 / 32768,
        -7 / 65536,
        -5 / 65536,
        -3 / 65536,
        -1 / 65536,
        -0.0,
        0.0,
        1 / 65536,
        3 / 65536,
        5 / 65536,
        7 / 65536,
        1 / 32768,
        32767 / 32768,
        1.0,
        2.0,
    ))
    # Half-LSB ties alternate between the adjacent even integers. Exact +1 is
    # the positive endpoint and saturates; only the original +/-2 are overrange.
    expected = struct.pack(
        "<17h",
        -32768,
        -32768,
        -32767,
        -4,
        -2,
        -2,
        0,
        0,
        0,
        0,
        2,
        2,
        4,
        1,
        32767,
        32767,
        32767,
    )
    source, output = io.BytesIO(source_bytes), io.BytesIO()
    stats = convert_recording_pcm(source, output, frames=17, channels=1, encoding="pcm16")
    assert output.getvalue() == expected
    assert stats.source_overrange_samples == stats.source_overrange_frames == 2
    assert stats.peak_abs == 2.0
    assert stats.source_payload_sha256 == hashlib.sha256(source_bytes).hexdigest()
    assert stats.output_payload_sha256 == hashlib.sha256(expected).hexdigest()


def test_pcm24_lsb_ties_endpoints_and_saturation_use_exact_little_endian_bytes() -> None:
    source = io.BytesIO(
        _float_payload((
            -2.0,
            -1.0,
            -8388607 / 8388608,
            -7 / 16777216,
            -5 / 16777216,
            -3 / 16777216,
            -1 / 16777216,
            -0.0,
            0.0,
            1 / 16777216,
            3 / 16777216,
            5 / 16777216,
            7 / 16777216,
            1 / 8388608,
            8388607 / 8388608,
            16777215 / 16777216,
            1.0,
            2.0,
        ))
    )
    expected = bytes.fromhex(
        "000080 000080 010080 fcffff feffff feffff 000000 "
        "000000 000000 000000 020000 020000 040000 010000 "
        "ffff7f ffff7f ffff7f ffff7f"
    )
    output = io.BytesIO()
    stats = convert_recording_pcm(source, output, frames=18, channels=1, encoding="pcm24")
    assert output.getvalue() == expected
    assert stats.source_overrange_samples == stats.source_overrange_frames == 2
    assert stats.output_payload_sha256 == hashlib.sha256(expected).hexdigest()


def test_float32_preserves_signed_zero_subnormal_and_overrange_payload_bits() -> None:
    words = (
        0x00000000,
        0x80000000,
        0x3F800000,
        0xBF800000,
        0x40000000,
        0xC0000000,
        0x00000001,
        0x80000001,
        0x7F7FFFFF,
    )
    payload = struct.pack("<9I", *words)
    source, output = io.BytesIO(payload), io.BytesIO()
    stats = convert_recording_pcm(source, output, frames=9, channels=1, encoding="float32")
    assert output.getvalue() == payload
    assert struct.unpack("<9I", output.getvalue()) == words
    assert stats.peak_abs == 340282346638528859811704183484516925440.0
    assert stats.source_overrange_samples == stats.source_overrange_frames == 3
    assert stats.source_payload_sha256 == stats.output_payload_sha256


@pytest.mark.parametrize("encoding", ["float32", "pcm16", "pcm24"])
def test_original_overrange_counts_samples_and_frames_before_any_quantization(
    encoding: WavEncoding,
) -> None:
    payload = _float_payload((1.0, -1.0, 1.25, 0.0, -1.5, 2.0, 0.5, -0.5, -0.0, 0.0))
    source, output = io.BytesIO(payload), io.BytesIO()
    stats = convert_recording_pcm(source, output, frames=5, channels=2, encoding=encoding)
    assert (stats.frames, stats.channels, stats.peak_abs) == (5, 2, 2.0)
    assert (stats.source_overrange_samples, stats.source_overrange_frames) == (3, 2)
    assert not source.closed
    assert not output.closed


def test_frame_overrange_accounting_remains_exact_across_three_channel_block_boundary() -> None:
    # 5461 three-channel frames fit in 64 KiB. Place independent overrange
    # samples in its last frame and the following frame, including two channels.
    payload = bytearray(5463 * 12)
    struct.pack_into("<f", payload, 5460 * 12 + 8, 1.5)
    struct.pack_into("<f", payload, 5461 * 12, -2.0)
    struct.pack_into("<f", payload, 5461 * 12 + 8, 3.0)
    source, output = io.BytesIO(payload), io.BytesIO()
    stats = convert_recording_pcm(source, output, frames=5463, channels=3, encoding="float32")
    assert output.getvalue() == payload
    assert (stats.source_overrange_samples, stats.source_overrange_frames) == (3, 2)
    assert stats.peak_abs == 3.0


def test_positions_advance_while_payload_hashes_exclude_caller_owned_prefixes() -> None:
    payload = _float_payload((-1.0, -0.5, 0.0, 0.5, 1.0, 2.0))
    expected = struct.pack("<6h", -32768, -16384, 0, 16384, 32767, 32767)
    source, output = io.BytesIO(b"source header" + payload), io.BytesIO(b"output header")
    source.seek(len(b"source header"))
    output.seek(0, os.SEEK_END)
    stats = convert_recording_pcm(source, output, frames=3, channels=2, encoding="pcm16")
    assert source.tell() == len(b"source header") + len(payload)
    assert output.getvalue() == b"output header" + expected
    assert stats.source_payload_sha256 == hashlib.sha256(payload).hexdigest()
    assert stats.output_payload_sha256 == hashlib.sha256(expected).hexdigest()
    assert isinstance(stats, RecordingPcmStats)
    field = "frames"
    with pytest.raises(FrozenInstanceError):
        setattr(stats, field, 7)


@pytest.mark.parametrize("encoding", ["float32", "pcm16", "pcm24"])
@pytest.mark.parametrize("word", [0x7F800000, 0xFF800000, 0x7FC00001, 0xFFC00001])
def test_nonfinite_samples_are_rejected_before_writing_their_block(
    encoding: WavEncoding, word: int
) -> None:
    source = io.BytesIO(_float_payload((0.5,)) + struct.pack("<I", word))
    output = io.BytesIO(b"preserved prefix")
    output.seek(0, os.SEEK_END)
    with pytest.raises(ValueError, match="nonfinite"):
        convert_recording_pcm(source, output, frames=1, channels=2, encoding=encoding)
    assert output.getvalue() == b"preserved prefix"
    assert not source.closed
    assert not output.closed


def test_late_nonfinite_block_preserves_only_preceding_owned_partial_output() -> None:
    source = io.BytesIO(bytes(65536) + struct.pack("<I", 0x7F800000))
    output = io.BytesIO()
    with pytest.raises(ValueError, match="nonfinite"):
        convert_recording_pcm(source, output, frames=16385, channels=1, encoding="pcm24")
    assert output.getvalue() == bytes(49152)
    assert not source.closed
    assert not output.closed


@pytest.mark.parametrize("size", [0, 1, 3, 4, 7])
def test_truncated_float_payload_rejects_partial_samples_and_frames(size: int) -> None:
    source, output = io.BytesIO(bytes(size)), io.BytesIO()
    with pytest.raises(EOFError, match="truncated"):
        convert_recording_pcm(source, output, frames=1, channels=2, encoding="pcm24")
    assert output.getvalue() == b""
    assert not source.closed
    assert not output.closed


@pytest.mark.parametrize("extra", [b"\0", b"\0\0\0", b"\0\0\0\0"])
def test_extra_partial_sample_or_complete_frame_is_not_silently_ignored(extra: bytes) -> None:
    source, output = io.BytesIO(_float_payload((0.5,)) + extra), io.BytesIO()
    with pytest.raises(ValueError, match="extra bytes"):
        convert_recording_pcm(source, output, frames=1, channels=1, encoding="pcm16")
    # Failure grants no output authority; the caller still owns completed bytes.
    assert output.getvalue() == bytes.fromhex("0040")
    assert not source.closed
    assert not output.closed


class _ReadTrace:
    def __init__(self, handle: BinaryIO, *, limit: int = 65536) -> None:
        self.handle = handle
        self.limit = limit
        self.requests: list[int] = []

    def read(self, size: int = -1) -> bytes:
        assert 0 < size <= 65536, "only bounded payload or EOF reads are permitted"
        self.requests.append(size)
        return self.handle.read(min(size, self.limit))


class _WriteTrace:
    def __init__(self, handle: BinaryIO, *, limit: int = 65536) -> None:
        self.handle = handle
        self.limit = limit
        self.requests: list[int] = []

    def write(self, payload: memoryview) -> int | None:
        assert 0 < len(payload) <= 65536
        self.requests.append(len(payload))
        return self.handle.write(payload[: self.limit])


@pytest.mark.parametrize(("read_limit", "write_limit"), [(1, 1), (3, 5), (7, 2)])
def test_short_io_reassembles_samples_without_losing_bytes_or_hashes(
    read_limit: int, write_limit: int
) -> None:
    payload = _float_payload((-1.0, -0.5, -0.0, 0.0, 0.5, 1.0))
    expected = bytes.fromhex("000080 0000c0 000000 000000 000040 ffff7f")
    source, output = io.BytesIO(payload), io.BytesIO()
    reader = _ReadTrace(source, limit=read_limit)
    writer = _WriteTrace(output, limit=write_limit)
    stats = convert_recording_pcm(
        cast("BinaryIO", reader),
        cast("BinaryIO", writer),
        frames=2,
        channels=3,
        encoding="pcm24",
    )
    assert output.getvalue() == expected
    assert stats.source_payload_sha256 == hashlib.sha256(payload).hexdigest()
    assert stats.output_payload_sha256 == hashlib.sha256(expected).hexdigest()
    assert len(reader.requests) > 2
    assert len(writer.requests) > 1
    assert not source.closed
    assert not output.closed


@pytest.mark.parametrize("channels", [1, 3, 16])
def test_real_file_io_uses_bounded_frame_aligned_blocks_at_more_than_one_block(
    tmp_path: Path, channels: int
) -> None:
    frames_per_block = 65536 // (channels * 4)
    frames = frames_per_block * 2 + 3
    payload = _float_payload((0.25, -0.25, 0.5, -0.5))
    payload = (payload * ((frames * channels * 4 + 15) // 16))[: frames * channels * 4]
    path, destination = tmp_path / "source.f32", tmp_path / "output.s24"
    path.write_bytes(payload)
    with path.open("rb") as source, destination.open("xb") as output:
        reader, writer = _ReadTrace(source), _WriteTrace(output)
        stats = convert_recording_pcm(
            cast("BinaryIO", reader),
            cast("BinaryIO", writer),
            frames=frames,
            channels=channels,
            encoding="pcm24",
        )
        assert reader.requests == [frames_per_block * channels * 4] * 2 + [3 * channels * 4, 1]
        assert all(size <= 65536 for size in reader.requests + writer.requests)
        assert not source.closed
        assert not output.closed
    expected_pattern = bytes.fromhex("000020 0000e0 000040 0000c0")
    expected = (expected_pattern * ((frames * channels + 3) // 4))[: frames * channels * 3]
    assert destination.read_bytes() == expected
    assert stats.source_payload_sha256 == hashlib.sha256(payload).hexdigest()
    assert stats.output_payload_sha256 == hashlib.sha256(expected).hexdigest()
    assert destination.stat().st_size == frames * channels * 3


class _InvalidRead:
    def __init__(self, result: bytes | None) -> None:
        self.result = result

    def read(self, size: int) -> bytes | None:
        return self.result


@pytest.mark.parametrize("result", [None, bytes(5)])
def test_reader_without_valid_forward_progress_is_rejected(result: bytes | None) -> None:
    source, output = _InvalidRead(result), io.BytesIO()
    with pytest.raises(OSError, match="invalid progress"):
        convert_recording_pcm(
            cast("BinaryIO", source), output, frames=1, channels=1, encoding="float32"
        )
    assert output.getvalue() == b""
    assert not output.closed


class _InvalidWrite:
    def __init__(self, result: int | None) -> None:
        self.result = result

    def write(self, payload: memoryview) -> int | None:
        return self.result


@pytest.mark.parametrize("result", [None, 0, -1, 5])
def test_writer_without_valid_forward_progress_is_rejected(result: int | None) -> None:
    source, output = io.BytesIO(bytes(4)), _InvalidWrite(result)
    with pytest.raises(OSError, match="invalid progress"):
        convert_recording_pcm(
            source, cast("BinaryIO", output), frames=1, channels=1, encoding="float32"
        )
    assert not source.closed


@pytest.mark.parametrize(("cancel_at", "completed_bytes"), [(1, 0), (2, 65536), (3, 65540)])
def test_cancellation_before_between_and_after_blocks_leaves_owned_partial_output(
    cancel_at: int, completed_bytes: int
) -> None:
    source, output = io.BytesIO(bytes(65540)), io.BytesIO()
    calls = 0

    def checkpoint() -> None:
        nonlocal calls
        calls += 1
        if calls == cancel_at:
            message = "cancel recording conversion"
            raise InterruptedError(message)

    with pytest.raises(InterruptedError, match="cancel recording"):
        convert_recording_pcm(
            source, output, frames=16385, channels=1, encoding="float32", checkpoint=checkpoint
        )
    assert calls == cancel_at
    assert source.tell() == completed_bytes
    assert output.getvalue() == bytes(completed_bytes)
    assert not source.closed
    assert not output.closed


@pytest.mark.parametrize(
    ("frames", "channels", "encoding"),
    [
        (0, 1, "float32"),
        (-1, 1, "float32"),
        (True, 1, "float32"),
        (1.0, 1, "float32"),
        ("1", 1, "float32"),
        (1, 0, "float32"),
        (1, 17, "float32"),
        (1, True, "float32"),
        (1, 1.0, "float32"),
        (1, "1", "float32"),
        ((1 << 64) // 4, 1, "float32"),
        (1, 1, "pcm32"),
        (1, 1, None),
        (1, 1, []),
    ],
)
def test_invalid_geometry_or_encoding_fails_before_touching_caller_io(
    frames: object, channels: object, encoding: object
) -> None:
    source, output = io.BytesIO(bytes(4)), io.BytesIO(b"untouched")
    with pytest.raises(ValueError, match="recording"):
        convert_recording_pcm(
            source,
            output,
            frames=cast("int", frames),
            channels=cast("int", channels),
            encoding=cast("WavEncoding", encoding),
        )
    assert source.tell() == output.tell() == 0
    assert output.getvalue() == b"untouched"
    assert not source.closed
    assert not output.closed


@pytest.mark.parametrize("channels", [1, 16])
def test_largest_aligned_u64_payload_is_admitted_without_allocating_the_take(
    channels: int,
) -> None:
    source, output = io.BytesIO(), io.BytesIO()

    def checkpoint() -> None:
        message = "cancel before reading a valid large payload"
        raise InterruptedError(message)

    # This checks extent arithmetic and pre-I/O cancellation only. The source
    # is not a real large recording and provides no actual >RIFF or RAM proof.
    with pytest.raises(InterruptedError, match="valid large payload"):
        convert_recording_pcm(
            source,
            output,
            frames=((1 << 64) - 1) // (channels * 4),
            channels=channels,
            encoding="float32",
            checkpoint=checkpoint,
        )
    assert source.tell() == output.tell() == 0
    assert not source.closed
    assert not output.closed


def test_same_handle_is_rejected_without_creating_or_truncating_anything() -> None:
    handle = io.BytesIO(_float_payload((0.5,)))
    with pytest.raises(ValueError, match="distinct handles"):
        convert_recording_pcm(handle, handle, frames=1, channels=1, encoding="float32")
    assert handle.tell() == 0
    assert handle.getvalue() == _float_payload((0.5,))
    assert not handle.closed
