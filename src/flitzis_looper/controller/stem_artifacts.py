"""Bounded offline conversion of separator WAVs to the shared project stem contract."""

import math
import struct
import wave
from array import array
from contextlib import ExitStack
from dataclasses import dataclass
from typing import TYPE_CHECKING
from uuid import uuid4

from flitzis_looper.controller.stem_process import (
    stem_command_checkpoint,
    stem_conversion_checkpoint,
)
from flitzis_looper.models import STEM_KINDS
from flitzis_looper.pcm16_wav import Pcm16WavWriter, PcmWavReader
from flitzis_looper_audio import StemConversionReservation

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller.stem_generation import AudioShape, StemProgressCallback

MAX_TRANSIENT_PCM_BYTES = 1024 * 1024 * 1024
DEFAULT_BLOCK_FRAMES = 4096
_MAX_WAV_DATA_BYTES = 0xFFFF_FFFF_FFFF_FFFF - 72
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
    reader: PcmWavReader
    path: Path
    sample_rate_hz: int
    channels: int
    frames: int


def _validate_limit(limit: int) -> None:
    if not isinstance(limit, int) or not 0 < limit <= MAX_TRANSIENT_PCM_BYTES:
        msg = "transient PCM limit must be a positive integer at most 1 GiB"
        raise StemArtifactError(msg)


def _source(reader: PcmWavReader, path: Path) -> _Source:
    channels, rate, frames = reader.getnchannels(), reader.getframerate(), reader.getnframes()
    if channels <= 0 or rate <= 0 or frames <= 0:
        msg = f"{path.name} has an invalid audio shape"
        raise StemArtifactError(msg)
    if reader.getsampwidth() != 2:
        msg = f"{path.name} must be 16-bit PCM WAV"
        raise StemArtifactError(msg)
    return _Source(reader, path, rate, channels, frames)


def _raw_span(source: _Source, first: int, count: int) -> bytes:
    if count == 0:
        return b""
    source.reader.setpos(first)
    raw = source.reader.readframes(count)
    if len(raw) != count * source.channels * 2:
        msg = f"{source.path.name} has incomplete PCM data"
        raise StemArtifactError(msg)
    return raw


def _read_span(source: _Source, first: int, count: int) -> array[float]:
    raw = _raw_span(source, first, count)
    samples = array("d", [0.0]) * (count * source.channels)
    for index, (value,) in enumerate(struct.iter_unpack("<h", raw)):
        samples[index] = -1.0 if value == -32768 else value / 32767.0
    return samples


def read_pcm16_wav(
    path: Path, *, max_transient_pcm_bytes: int = MAX_TRANSIENT_PCM_BYTES
) -> AudioData:
    """Read PCM16 with an explicit raw-plus-binary64 PCM cap; generation streams instead."""
    _validate_limit(max_transient_pcm_bytes)
    with PcmWavReader(path) as reader:
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
    # Retain the legacy conservative PCM envelope. Native actually retains four
    # raw spans and five native/Python payload pairs plus one writer copy (22
    # bytes/target sample), without decoded binary64 arrays. Fixed bridge/stack
    # metadata additionally receives a separate prospective host allowance.
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
        msg = "target stem exceeds the RF64 data-size limit"
        raise StemArtifactError(msg)
    if not isinstance(block_frames, int) or not 0 < block_frames <= DEFAULT_BLOCK_FRAMES:
        msg = "stem block frames must be between 1 and 4096"
        raise StemArtifactError(msg)


def validate_target_shape(target: AudioShape) -> None:
    """Reject unrepresentable target WAV shapes before separator inference starts."""
    _validate_target(target, DEFAULT_BLOCK_FRAMES)


def _write_block(
    sources: dict[str, _Source],
    writers: dict[str, Pcm16WavWriter],
    target: AudioShape,
    start: int,
    frames: int,
) -> None:
    stem_conversion_checkpoint()
    promised = _block_pcm_bytes(sources, target, start, frames)
    reservation = StemConversionReservation(promised, MAX_TRANSIENT_PCM_BYTES)
    try:
        _convert_block(sources, writers, target, start, frames, reservation)
    finally:
        # Synchronous native work and every prospective write have ended. A
        # retained exception traceback must not retain a prospective promise.
        reservation.close()


def _convert_block(
    sources: dict[str, _Source],
    writers: dict[str, Pcm16WavWriter],
    target: AudioShape,
    start: int,
    frames: int,
    reservation: StemConversionReservation,
) -> None:
    spans = []
    for source in sources.values():
        stem_command_checkpoint()
        first, count = _span(source, start, frames, target.sample_rate_hz)
        spans.append((
            source.sample_rate_hz,
            source.channels,
            source.frames,
            first,
            _raw_span(source, first, count),
        ))
    payloads = reservation.convert(
        (spans[0], spans[1], spans[2], spans[3]),
        (target.sample_rate_hz, target.channels, target.frame_count),
        start,
        frames,
    )
    # No write or completion can follow a cancellation during detached work.
    # The existing worker still owns its source/generation and host promise.
    stem_command_checkpoint()
    for name, payload in zip((*sources, "instrumental"), payloads, strict=True):
        stem_command_checkpoint()
        writers[name].writeframesraw(payload)
    # Block locals leave together before the next block's reservation/read.


def _write_blocks(
    sources: dict[str, _Source],
    writers: dict[str, Pcm16WavWriter],
    target: AudioShape,
    block_frames: int,
    limit: int,
    progress: StemProgressCallback,
) -> None:
    start = 0
    reported_percent = 0
    while start < target.frame_count:
        stem_command_checkpoint()
        frames = _block_size(
            sources, target, start, min(block_frames, target.frame_count - start), limit
        )
        _write_block(sources, writers, target, start, frames)
        start += frames
        percent = start * 100 // target.frame_count
        if percent > reported_percent:
            progress(0.85 + 0.0015 * percent, "Aligning stem cache")
            reported_percent = percent


def _open_sources(
    stack: ExitStack, output_root: Path, max_transient_pcm_bytes: int
) -> dict[str, _Source]:
    sources: dict[str, _Source] = {}
    for name, output_name in _OUTPUT_MAP:
        stem_command_checkpoint()
        path = _find_output_file(output_root, output_name)
        try:
            reader = stack.enter_context(PcmWavReader(path))
        except (EOFError, wave.Error) as error:
            msg = f"{path.name} has invalid or incomplete PCM data: {error}"
            raise StemArtifactError(msg) from error
        source = _source(reader, path)
        if source.channels * 10 > max_transient_pcm_bytes:
            msg = "source PCM frame exceeds the transient PCM limit"
            raise StemArtifactError(msg)
        probe = StemConversionReservation(source.channels * 10, max_transient_pcm_bytes)
        try:
            _raw_span(source, source.frames - 1, 1)
        finally:
            probe.close()
        sources[name] = source
    return sources


def _verify_outputs(
    temporary: dict[str, Path],
    target_shape: AudioShape,
    expected_digests: dict[str, str],
    writers: dict[str, Pcm16WavWriter],
    max_transient_pcm_bytes: int,
) -> None:
    for name, path in temporary.items():
        stem_command_checkpoint()
        with PcmWavReader(path) as reader:
            if reader.file_identity != writers[name].file_identity:
                msg = f"{name}.wav output file identity changed"
                raise StemArtifactError(msg)
            if (
                reader.getnframes() != target_shape.frame_count
                or reader.getnchannels() != target_shape.channels
                or reader.getframerate() != target_shape.sample_rate_hz
                or reader.getsampwidth() != 2
                or reader.verify(
                    chunk_bytes=min(64 * 1024, max_transient_pcm_bytes),
                    checkpoint=stem_command_checkpoint,
                )
                != expected_digests[name]
            ):
                msg = f"{name}.wav failed complete output verification"
                raise StemArtifactError(msg)


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
    stem_conversion_checkpoint()
    cache_dir.mkdir(parents=True, exist_ok=True)
    token = uuid4().hex
    temporary: dict[str, Path] = {
        name: cache_dir / f"{name}.wav.{token}.tmp" for name in STEM_KINDS
    }
    writers: dict[str, Pcm16WavWriter] = {}
    try:
        with ExitStack() as stack:
            sources = _open_sources(stack, output_root, max_transient_pcm_bytes)
            _block_size(
                sources,
                target_shape,
                0,
                min(block_frames, target_shape.frame_count),
                max_transient_pcm_bytes,
            )
            for name, path in temporary.items():
                stem_command_checkpoint()
                writers[name] = stack.enter_context(
                    Pcm16WavWriter(
                        path,
                        sample_rate_hz=target_shape.sample_rate_hz,
                        channels=target_shape.channels,
                        frames=target_shape.frame_count,
                    )
                )
            _write_blocks(
                sources, writers, target_shape, block_frames, max_transient_pcm_bytes, progress
            )
            expected_digests = {name: writer.file_digest for name, writer in writers.items()}
        _verify_outputs(temporary, target_shape, expected_digests, writers, max_transient_pcm_bytes)
        for name, path in temporary.items():
            stem_command_checkpoint()
            path.replace(cache_dir / f"{name}.wav")
    finally:
        for writer in writers.values():
            writer.discard_owned_output()
    progress(1.0, "Stem cache ready")
