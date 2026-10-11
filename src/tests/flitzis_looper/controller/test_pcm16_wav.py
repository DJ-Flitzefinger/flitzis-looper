"""Independent RIFF/RF64 geometry, productive I/O and complete integrity checks."""

import hashlib
import os
import struct
import wave
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.stem_artifacts import validate_target_shape
from flitzis_looper.controller.stem_generation import AudioShape
from flitzis_looper.pcm16_wav import (
    RIFF_MAX_DATA_BYTES,
    Pcm16WavWriter,
    PcmWavReader,
)

if TYPE_CHECKING:
    from pathlib import Path


def _independent_rf64(payload: bytes, *, channels: int = 2, rate: int = 48000) -> bytes:
    data_bytes = len(payload)
    frames = data_bytes // (channels * 2)
    return (
        b"RF64\xff\xff\xff\xffWAVE"
        b"ds64\x1c\x00\x00\x00"
        + struct.pack("<QQQI", data_bytes + 72, data_bytes, frames, 0)
        + b"fmt \x10\x00\x00\x00"
        + struct.pack("<HHIIHH", 1, channels, rate, rate * channels * 2, channels * 2, 16)
        + b"data\xff\xff\xff\xff"
        + payload
    )


@pytest.mark.parametrize("partition", [1, 3, 17])
def test_small_riff_writer_remains_byteexact_python_wave(tmp_path: Path, partition: int) -> None:
    values = (-32768, -32767, -1, 0, 1, 32767) * 19
    payload = struct.pack(f"<{len(values)}h", *values)
    legacy, actual = tmp_path / "legacy.wav", tmp_path / "actual.wav"
    with wave.open(str(legacy), "wb") as writer:
        writer.setnchannels(3)
        writer.setsampwidth(2)
        writer.setframerate(44100)
        writer.writeframes(payload)
    with Pcm16WavWriter(
        actual, sample_rate_hz=44100, channels=3, frames=len(values) // 3
    ) as writer:
        for begin in range(0, len(payload), partition * 6):
            writer.writeframesraw(payload[begin : begin + partition * 6])
    assert actual.read_bytes() == legacy.read_bytes()
    with PcmWavReader(actual) as reader:
        assert reader.verify(chunk_bytes=7) == hashlib.sha256(legacy.read_bytes()).hexdigest()


@pytest.mark.parametrize("partition", [1, 3, 17])
def test_rf64_writer_and_positioned_reader_use_independent_full_payload_oracle(
    tmp_path: Path, partition: int
) -> None:
    values = (-32768, 32767, -1, 0, 1, -32767) * 23
    payload = struct.pack(f"<{len(values)}h", *values)
    expected = _independent_rf64(payload)
    path = tmp_path / "output.wav"
    with Pcm16WavWriter(
        path, sample_rate_hz=48000, channels=2, frames=len(values) // 2, force_rf64=True
    ) as writer:
        for begin in range(0, len(payload), partition * 4):
            writer.writeframesraw(payload[begin : begin + partition * 4])
        digest = writer.file_digest
    assert path.read_bytes() == expected
    assert digest == hashlib.sha256(expected).hexdigest()
    with PcmWavReader(path) as reader:
        assert (reader.getnframes(), reader.getnchannels(), reader.getframerate()) == (69, 2, 48000)
        assert reader.verify(chunk_bytes=13) == digest
        reader.setpos(67)
        assert reader.readframes(5) == payload[67 * 4 :]
        assert reader.readframes(1) == b""
        reader.setpos(1)
        assert reader.readframes(3) == payload[4:16]


@pytest.mark.parametrize(
    "change", ["riff", "data", "frames", "table", "sentinel", "extra", "short"]
)
def test_rf64_rejects_inconsistent_declared_geometry_and_actual_eof(
    tmp_path: Path, change: str
) -> None:
    raw = bytearray(_independent_rf64(struct.pack("<hhhh", -32768, 32767, 1, -1)))
    if change == "riff":
        struct.pack_into("<Q", raw, 20, len(raw) - 7)
    elif change == "data":
        struct.pack_into("<Q", raw, 28, 7)
    elif change == "frames":
        struct.pack_into("<Q", raw, 36, 3)
    elif change == "table":
        struct.pack_into("<I", raw, 44, 1)
    elif change == "sentinel":
        struct.pack_into("<I", raw, 76, 8)
    elif change == "extra":
        raw.extend(b"x")
    else:
        del raw[-1]
    path = tmp_path / "invalid.wav"
    path.write_bytes(raw)
    with pytest.raises((wave.Error, EOFError)), PcmWavReader(path):
        pass


@pytest.mark.parametrize("location", [0, 1, 37])
def test_full_verification_detects_first_and_late_payload_changes(
    tmp_path: Path, location: int
) -> None:
    path = tmp_path / "source.wav"
    payload = struct.pack("<hhhh", -32768, 32767, -1, 1) * 38
    path.write_bytes(_independent_rf64(payload))
    with PcmWavReader(path) as reader:
        expected = reader.verify(chunk_bytes=11)
        with path.open("r+b") as handle:
            handle.seek(80 + location * 8)
            handle.write(b"\xff")
        assert reader.verify(chunk_bytes=11) != expected


def test_writer_rejects_incomplete_and_extra_frames(tmp_path: Path) -> None:
    with (
        pytest.raises(ValueError, match="incomplete"),
        Pcm16WavWriter(tmp_path / "short.wav", sample_rate_hz=1, channels=1, frames=2) as writer,
    ):
        writer.writeframesraw(b"\x00\x00")
    with (
        pytest.raises(ValueError, match="frame extent"),
        Pcm16WavWriter(tmp_path / "extra.wav", sample_rate_hz=1, channels=1, frames=1) as writer,
    ):
        writer.writeframesraw(b"\x00\x00\x00\x00")


def test_target_admission_accepts_disk_extent_above_riff_limit() -> None:
    validate_target_shape(AudioShape(48000, 2, RIFF_MAX_DATA_BYTES // 4 + 1))


def test_exclusive_writer_failure_preserves_existing_leaf(tmp_path: Path) -> None:
    path = tmp_path / "peer.wav"
    path.write_bytes(b"existing peer bytes")
    with (
        pytest.raises(FileExistsError),
        Pcm16WavWriter(path, sample_rate_hz=1, channels=1, frames=1),
    ):
        pass
    assert path.read_bytes() == b"existing peer bytes"


def test_owned_cleanup_preserves_replacement_identity(tmp_path: Path) -> None:
    path = tmp_path / "attempt.wav"
    retained = tmp_path / "retained-predecessor.wav"
    with Pcm16WavWriter(path, sample_rate_hz=1, channels=1, frames=1) as writer:
        writer.writeframesraw(b"\x00\x00")
    captured = writer.file_identity
    path.replace(retained)
    path.write_bytes(b"independent replacement bytes")
    writer.discard_owned_output()
    assert path.read_bytes() == b"independent replacement bytes"
    with PcmWavReader(retained) as reader:
        assert reader.file_identity == captured
        assert reader.readframes(1) == b"\x00\x00"


@pytest.mark.skipif(
    os.environ.get("FLITZIS_LONG_RF64_PROOF") != "1",
    reason="Explicit productive >4-GiB RF64 disk proof belongs to the serialized LONG runner",
)
def test_productive_large_rf64_writes_and_reopens_every_payload_byte(tmp_path: Path) -> None:
    # No sparse allocation or inflated manifest: write every sample beyond the
    # real RIFF boundary, hash it independently, then read every file byte.
    path = tmp_path / "productive-large.wav"
    frames = RIFF_MAX_DATA_BYTES // 4 + 8193
    pattern = struct.pack("<hhhhhhhh", -32768, 32767, 1, -1, 0, 12345, -23456, -32767)
    block = pattern * 4096
    header = bytearray(_independent_rf64(b"")[:80])
    struct.pack_into("<QQQ", header, 20, frames * 4 + 72, frames * 4, frames)
    expected = hashlib.sha256(header)
    with Pcm16WavWriter(path, sample_rate_hz=48000, channels=2, frames=frames) as writer:
        remaining = frames * 4
        while remaining:
            payload = block[: min(len(block), remaining)]
            writer.writeframesraw(payload)
            expected.update(payload)
            remaining -= len(payload)
        assert writer.file_digest == expected.hexdigest()
    assert path.stat().st_size > 0xFFFF_FFFF
    with PcmWavReader(path) as reader:
        assert reader.getnframes() == frames
        assert reader.verify(chunk_bytes=65521) == expected.hexdigest()
        reader.setpos(frames - 1)
        offset = ((frames - 1) * 4) % len(pattern)
        assert reader.readframes(2) == pattern[offset : offset + 4]
