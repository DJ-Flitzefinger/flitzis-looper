"""Bounded complete-WAV threshold measurements without candidate timing information."""

import array
import math
import struct
import sys
from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from flitzis_looper.analysis.loop_capture_models import ChannelFeatures, DetectorPolicy, Feature
from flitzis_looper.analysis.reference_inputs_validation import fail

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path
    from typing import BinaryIO

    from flitzis_looper.analysis.loop_capture_models import DetectorChannel

_BLOCK_FRAMES = 8192
_MAX_FEATURES = 100000


@dataclass(frozen=True)
class WaveInfo:
    """Actual RIFF format/extent, not declared recorder or device clock identity."""

    rate: int
    channels: int
    frames: int
    sample_width: int
    format: Literal["pcm16", "pcm24", "pcm32", "float32"]
    data_offset: int
    data_bytes: int


def _format(raw: bytes) -> tuple[int, int, int, int]:
    if len(raw) < 16:
        fail("wav_fmt_truncated")
    encoding, channels, rate, byte_rate, alignment, bits = struct.unpack("<HHIIHH", raw[:16])
    if encoding == 0xFFFE:
        if len(raw) < 40 or struct.unpack("<H", raw[16:18])[0] < 22:
            fail("wav_extensible_fmt_truncated")
        valid_bits = struct.unpack("<H", raw[18:20])[0]
        guid_suffix = b"\x00\x00\x00\x00\x10\x00\x80\x00\x00\xaa\x00\x38\x9b\x71"
        if valid_bits != bits or raw[26:40] != guid_suffix:
            fail("wav_extensible_subformat_or_valid_bits_unsupported")
        encoding = struct.unpack("<H", raw[24:26])[0]
    if not 1 <= channels <= 16 or not 1 <= rate <= 768000:
        fail("wav_channels_or_rate_out_of_bounds")
    if (encoding, bits) not in {(1, 16), (1, 24), (1, 32), (3, 32)}:
        fail("wav_requires_pcm16_pcm24_pcm32_or_float32")
    if alignment != channels * (bits // 8) or byte_rate != rate * alignment:
        fail("wav_block_alignment_or_byte_rate_mismatch")
    return encoding, channels, rate, bits


def _wave_chunks(
    handle: BinaryIO, extent: int
) -> tuple[tuple[int, int, int, int], tuple[int, int]]:
    fmt: tuple[int, int, int, int] | None = None
    data: tuple[int, int] | None = None
    while handle.tell() < extent:
        chunk = handle.read(8)
        if len(chunk) != 8:
            fail("wav_chunk_header_truncated")
        name, size = struct.unpack("<4sI", chunk)
        offset = handle.tell()
        if offset + size + size % 2 > extent:
            fail("wav_chunk_extent_mismatch")
        if name == b"fmt ":
            if fmt is not None or size > 1024:
                fail("wav_duplicate_or_oversized_fmt")
            fmt = _format(handle.read(size))
        elif name == b"data":
            if data is not None:
                fail("wav_duplicate_data")
            data = offset, size
        handle.seek(offset + size + size % 2)
    if fmt is None or data is None:
        fail("wav_fmt_or_data_missing")
    assert fmt is not None
    assert data is not None
    return fmt, data


def inspect_wave(path: Path) -> WaveInfo:
    """Reject partial, compressed or ambiguous RIFF and derive complete data extent."""
    extent = path.stat().st_size
    with path.open("rb") as handle:
        header = handle.read(12)
        if len(header) != 12 or header[:4] != b"RIFF" or header[8:] != b"WAVE":
            fail("capture_requires_standard_riff_wave_not_rf64")
        if struct.unpack("<I", header[4:8])[0] + 8 != extent:
            fail("wav_riff_extent_mismatch_or_partial_capture")
        fmt, data = _wave_chunks(handle, extent)
    encoding, channels, rate, bits = fmt
    offset, size = data
    width = bits // 8
    if size == 0 or size % (channels * width):
        fail("wav_empty_or_incomplete_frames")
    names: dict[int, Literal["pcm16", "pcm24", "pcm32", "float32"]] = {
        16: "pcm16",
        24: "pcm24",
        32: "pcm32",
    }
    name = "float32" if encoding == 3 else names[bits]
    return WaveInfo(rate, channels, size // (channels * width), width, name, offset, size)


def _samples(raw: bytes, info: WaveInfo) -> array.array[float] | list[float]:
    if info.format == "pcm24":
        return [
            int.from_bytes(raw[index : index + 3], "little", signed=True) / 8388608.0
            for index in range(0, len(raw), 3)
        ]
    code = {"pcm16": "h", "pcm32": "i", "float32": "f"}[info.format]
    samples: array.array[float] = array.array(code)
    samples.frombytes(raw)
    if sys.byteorder != "little":
        samples.byteswap()
    if info.format == "float32":
        return samples
    scale = 32768.0 if info.format == "pcm16" else 2147483648.0
    return [value / scale for value in samples]


def _blocks(handle: BinaryIO, info: WaveInfo) -> Iterator[array.array[float] | list[float]]:
    remaining = info.data_bytes
    while remaining:
        raw = handle.read(min(remaining, _BLOCK_FRAMES * info.sample_width * info.channels))
        if not raw:
            fail("wav_changed_or_truncated_during_scan")
        remaining -= len(raw)
        if len(raw) % (info.sample_width * info.channels):
            fail("wav_incomplete_block_during_scan")
        yield _samples(raw, info)


@dataclass
class _Detector:
    policy: DetectorChannel
    armed: bool
    peak: float
    saturated: int
    rejected: int
    last_edge: int
    low_run: int
    full_scale: float
    frames: list[int]

    def observe(self, amplitude: float, frame: int) -> None:
        self.peak = max(self.peak, amplitude)
        self.saturated += amplitude >= self.full_scale
        if amplitude <= self.policy.low:
            self.low_run += 1
            if self.low_run >= self.policy.rearm_low_capture_frames:
                self.armed = True
        else:
            self.low_run = 0
        if self.armed and amplitude >= self.policy.high:
            self.armed = False
            if frame - self.last_edge >= self.policy.minimum_gap_capture_frames:
                self.frames.append(frame)
                self.last_edge = frame
            else:
                self.rejected += 1


def measure_wave(
    path: Path, policy: DetectorPolicy
) -> tuple[WaveInfo, tuple[ChannelFeatures, ...]]:
    """Scan all channels/samples, retaining features without any predicted windows."""
    info = inspect_wave(path)
    channels = [item.channel for item in policy.channels]
    if len(set(channels)) != len(channels) or any(channel >= info.channels for channel in channels):
        fail("detector_duplicate_or_missing_wave_channel")
    if any(item.low >= item.high for item in policy.channels):
        fail("detector_low_must_be_less_than_high")
    detectors = [
        _Detector(
            policy=item,
            armed=True,
            peak=0.0,
            saturated=0,
            rejected=0,
            last_edge=-item.minimum_gap_capture_frames,
            low_run=0,
            full_scale=1.0
            if info.format == "float32"
            else 1.0 - 2.0 ** (1 - info.sample_width * 8),
            frames=[],
        )
        for item in policy.channels
    ]
    frame_offset = 0
    with path.open("rb") as handle:
        handle.seek(info.data_offset)
        for samples in _blocks(handle, info):
            if any(not math.isfinite(value) for value in samples):
                fail("wav_nonfinite_sample")
            for detector in detectors:
                for index in range(detector.policy.channel, len(samples), info.channels):
                    detector.observe(abs(samples[index]), frame_offset + index // info.channels)
            frame_offset += len(samples) // info.channels
            if sum(len(item.frames) for item in detectors) > _MAX_FEATURES:
                fail("capture_feature_count_exceeds_100000_use_shorter_or_cleaner_recording")
    measured = tuple(
        ChannelFeatures(
            channel=item.policy.channel,
            peak_absolute=item.peak,
            samples_at_or_above_full_scale=item.saturated,
            rejected_by_minimum_gap=item.rejected,
            edges=tuple(
                Feature(event_id=index, capture_frame=frame)
                for index, frame in enumerate(item.frames)
            ),
        )
        for item in detectors
    )
    return info, measured
