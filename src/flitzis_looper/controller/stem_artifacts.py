"""Bounded offline conversion of separator WAVs to the shared project stem contract."""

import math
import struct
import wave
from array import array
from contextlib import ExitStack, suppress
from dataclasses import dataclass
from typing import TYPE_CHECKING
from uuid import uuid4

from flitzis_looper.models import STEM_KINDS

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller.stem_generation import AudioShape, StemProgressCallback

MAX_TRANSIENT_PCM_BYTES = 1024 * 1024 * 1024
DEFAULT_BLOCK_FRAMES = 4096
_MAX_WAV_DATA_BYTES = 0xFFFF_FFFF - 36
_OUTPUT_MAP = (("vocals", "vocals"), ("drums", "drums"), ("bass", "bass"), ("melody", "other"))


class StemArtifactError(RuntimeError):
    """An offline separator did not supply a valid bounded complete artifact set."""


@dataclass(frozen=True, slots=True)
class AudioData:
    """Capped PCM16 reader result for nonproductive compatibility callers."""

    sample_rate_hz: int
    channels: int
    samples: array[float]

    @property
    def frame_count(self) -> int:
        """Return the number of complete interleaved frames."""
        return len(self.samples) // self.channels


@dataclass(frozen=True, slots=True)
class _Source:
    reader: wave.Wave_read
    path: Path
    sample_rate_hz: int
    channels: int
    frames: int


def _validate_limit(limit: int) -> None:
    if not isinstance(limit, int) or not 0 < limit <= MAX_TRANSIENT_PCM_BYTES:
        msg = "transient PCM limit must be a positive integer at most 1 GiB"
        raise StemArtifactError(msg)


def _source(reader: wave.Wave_read, path: Path) -> _Source:
    channels, rate, frames = reader.getnchannels(), reader.getframerate(), reader.getnframes()
    if channels <= 0 or rate <= 0 or frames <= 0:
        msg = f"{path.name} has an invalid audio shape"
        raise StemArtifactError(msg)
    if reader.getsampwidth() != 2:
        msg = f"{path.name} must be 16-bit PCM WAV"
        raise StemArtifactError(msg)
    return _Source(reader, path, rate, channels, frames)


def _read_span(source: _Source, first: int, count: int) -> array[float]:
    if count == 0:
        return array("d")
    source.reader.setpos(first)
    raw = source.reader.readframes(count)
    if len(raw) != count * source.channels * 2:
        msg = f"{source.path.name} has incomplete PCM data"
        raise StemArtifactError(msg)
    samples = array("d", [0.0]) * (count * source.channels)
    for index, (value,) in enumerate(struct.iter_unpack("<h", raw)):
        samples[index] = -1.0 if value == -32768 else value / 32767.0
    return samples


def read_pcm16_wav(
    path: Path, *, max_transient_pcm_bytes: int = MAX_TRANSIENT_PCM_BYTES
) -> AudioData:
    """Read PCM16 with an explicit raw-plus-binary64 PCM cap; generation streams instead."""
    _validate_limit(max_transient_pcm_bytes)
    with wave.open(str(path), "rb") as reader:
        source = _source(reader, path)
        if source.frames * source.channels * 10 > max_transient_pcm_bytes:
            msg = "PCM16 reader exceeds the transient PCM limit"
            raise StemArtifactError(msg)
        return AudioData(
            source.sample_rate_hz, source.channels, _read_span(source, 0, source.frames)
        )


def _find_output_file(output_root: Path, name: str) -> Path:
    matches = sorted(output_root.rglob(f"{name}.wav"))
    if not matches:
        msg = f"Demucs output is missing {name}.wav"
        raise StemArtifactError(msg)
    return matches[0]


def _span(source: _Source, start: int, frames: int, target_rate: int) -> tuple[int, int]:
    ratio = source.sample_rate_hz / target_rate
    first = min(math.floor(start * ratio), source.frames)
    end = min(math.floor((start + frames - 1) * ratio) + 2, source.frames)
    return first, max(0, end - first)


def _block_pcm_bytes(
    sources: dict[str, _Source], target: AudioShape, start: int, frames: int
) -> int:
    # Four retained binary64 component blocks plus one PCM16 payload and its
    # conservative write copy. Source raw+decoded spans are counted together,
    # although productive conversion reads only one source span at a time.
    output = frames * target.channels * 36
    source_spans = sum(
        _span(source, start, frames, target.sample_rate_hz)[1] * source.channels * 10
        for source in sources.values()
    )
    channel_slices = max(source.channels for source in sources.values()) * 8
    return output + source_spans + channel_slices


def _block_size(
    sources: dict[str, _Source], target: AudioShape, start: int, requested: int, limit: int
) -> int:
    low, high = 0, requested
    while low < high:
        middle = (low + high + 1) // 2
        if _block_pcm_bytes(sources, target, start, middle) <= limit:
            low = middle
        else:
            high = middle - 1
    if low == 0:
        msg = "aligned stem block exceeds the transient PCM limit"
        raise StemArtifactError(msg)
    return low


def _sample(
    samples: array[float], source: _Source, first: int, frame: int, channel: int, channels: int
) -> float:
    if frame < 0 or frame >= source.frames:
        return 0.0
    offset = (frame - first) * source.channels
    if source.channels == channels:
        return samples[offset + channel]
    if source.channels == 1:
        return samples[offset]
    if channels == 1:
        return sum(samples[offset : offset + source.channels]) / source.channels
    if channel < source.channels:
        return samples[offset + channel]
    return 0.0


def _align_block(source: _Source, target: AudioShape, start: int, frames: int) -> array[float]:
    first, count = _span(source, start, frames, target.sample_rate_hz)
    samples = _read_span(source, first, count)
    output = array("d", [0.0]) * (frames * target.channels)
    ratio = source.sample_rate_hz / target.sample_rate_hz
    for local in range(frames):
        position = (start + local) * ratio
        frame = math.floor(position)
        fraction = position - frame
        for channel in range(target.channels):
            before = _sample(samples, source, first, frame, channel, target.channels)
            after = _sample(samples, source, first, frame + 1, channel, target.channels)
            output[local * target.channels + channel] = before + (after - before) * fraction
    return output


def _pcm16(value: float) -> int:
    value = value if math.isfinite(value) else 0.0
    value = max(-1.0, min(1.0, value))
    if value >= 1.0:
        return 32767
    if value <= -1.0:
        return -32768
    return round(value * 32767.0)


def _payload(samples: array[float]) -> bytearray:
    payload = bytearray(len(samples) * 2)
    for index, value in enumerate(samples):
        struct.pack_into("<h", payload, index * 2, _pcm16(value))
    return payload


def _instrumental_payload(aligned: dict[str, array[float]]) -> bytearray:
    payload = bytearray(len(aligned["drums"]) * 2)
    for index in range(len(aligned["drums"])):
        value = 0.0
        for name in ("drums", "bass", "melody"):
            value = max(-1.0, min(1.0, value + aligned[name][index]))
        struct.pack_into("<h", payload, index * 2, _pcm16(value))
    return payload


def _validate_target(target: AudioShape, block_frames: int) -> None:
    if not (
        all(
            isinstance(value, int)
            for value in (target.channels, target.sample_rate_hz, target.frame_count)
        )
        and 0 < target.channels <= 0xFFFF // 2
        and 0 < target.sample_rate_hz <= 0xFFFF_FFFF
        and target.sample_rate_hz * target.channels * 2 <= 0xFFFF_FFFF
        and target.frame_count > 0
    ):
        msg = "target audio shape must be valid and non-empty"
        raise StemArtifactError(msg)
    if target.frame_count * target.channels * 2 > _MAX_WAV_DATA_BYTES:
        msg = "target stem exceeds the PCM WAV data-size limit"
        raise StemArtifactError(msg)
    if not isinstance(block_frames, int) or not 0 < block_frames <= DEFAULT_BLOCK_FRAMES:
        msg = "stem block frames must be between 1 and 4096"
        raise StemArtifactError(msg)


def validate_target_shape(target: AudioShape) -> None:
    """Reject unrepresentable target WAV shapes before separator inference starts."""
    _validate_target(target, DEFAULT_BLOCK_FRAMES)


def _write_block(
    sources: dict[str, _Source],
    writers: dict[str, wave.Wave_write],
    target: AudioShape,
    start: int,
    frames: int,
) -> None:
    aligned = {
        name: _align_block(source, target, start, frames) for name, source in sources.items()
    }
    for name, samples in aligned.items():
        writers[name].writeframesraw(_payload(samples))
    writers["instrumental"].writeframesraw(_instrumental_payload(aligned))


def _write_blocks(
    sources: dict[str, _Source],
    writers: dict[str, wave.Wave_write],
    target: AudioShape,
    block_frames: int,
    limit: int,
    progress: StemProgressCallback,
) -> None:
    start = 0
    reported_percent = 0
    while start < target.frame_count:
        frames = _block_size(
            sources, target, start, min(block_frames, target.frame_count - start), limit
        )
        _write_block(sources, writers, target, start, frames)
        start += frames
        percent = start * 100 // target.frame_count
        if percent > reported_percent:
            progress(0.85 + 0.0015 * percent, "Aligning stem cache")
            reported_percent = percent


def write_project_cache_artifacts(
    *,
    output_root: Path,
    cache_dir: Path,
    target_shape: AudioShape,
    progress: StemProgressCallback,
    block_frames: int = DEFAULT_BLOCK_FRAMES,
    max_transient_pcm_bytes: int = MAX_TRANSIENT_PCM_BYTES,
) -> None:
    """Write byte-compatible aligned stems in bounded blocks, without publishing a set.

    Only the controller promotes the private generation after all five outputs
    and its content marker are complete. Processing failures remove these private
    temporary files. File replacements remain inside the unpublished generation;
    a partial replacement failure cannot alter an accepted canonical stem set.
    """
    _validate_limit(max_transient_pcm_bytes)
    _validate_target(target_shape, block_frames)
    cache_dir.mkdir(parents=True, exist_ok=True)
    token = uuid4().hex
    temporary = {name: cache_dir / f"{name}.wav.{token}.tmp" for name in STEM_KINDS}
    try:
        with ExitStack() as stack:
            sources: dict[str, _Source] = {}
            for name, output_name in _OUTPUT_MAP:
                path = _find_output_file(output_root, output_name)
                reader = stack.enter_context(wave.open(str(path), "rb"))
                source = _source(reader, path)
                if source.channels * 10 > max_transient_pcm_bytes:
                    msg = "source PCM frame exceeds the transient PCM limit"
                    raise StemArtifactError(msg)
                # Reading the last declared complete frame detects a truncated
                # source even when the target would use only its beginning.
                _read_span(source, source.frames - 1, 1)
                sources[name] = source
            _block_size(
                sources,
                target_shape,
                0,
                min(block_frames, target_shape.frame_count),
                max_transient_pcm_bytes,
            )
            writers: dict[str, wave.Wave_write] = {}
            for name, path in temporary.items():
                writer = stack.enter_context(wave.open(str(path), "wb"))
                writer.setnchannels(target_shape.channels)
                writer.setsampwidth(2)
                writer.setframerate(target_shape.sample_rate_hz)
                writer.setnframes(target_shape.frame_count)
                writers[name] = writer
            _write_blocks(
                sources, writers, target_shape, block_frames, max_transient_pcm_bytes, progress
            )
        for name, path in temporary.items():
            path.replace(cache_dir / f"{name}.wav")
    finally:
        for path in temporary.values():
            with suppress(OSError):
                path.unlink(missing_ok=True)
    progress(1.0, "Stem cache ready")
