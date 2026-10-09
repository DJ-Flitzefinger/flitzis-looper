"""Compare productive streaming artifacts with the frozen full-buffer legacy oracle."""

import math
import struct
import wave
from dataclasses import dataclass
from itertools import pairwise
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller import stem_artifacts
from flitzis_looper.controller.stem_artifacts import (
    StemArtifactError,
    read_pcm16_wav,
    validate_target_shape,
    write_project_cache_artifacts,
)
from flitzis_looper.controller.stem_generation import AudioShape
from flitzis_looper.models import STEM_KINDS

if TYPE_CHECKING:
    from pathlib import Path


@dataclass
class _OracleAudio:
    rate: int
    channels: int
    samples: list[float]

    @property
    def frames(self) -> int:
        return len(self.samples) // self.channels


def _oracle_sample(audio: _OracleAudio, frame: int, channel: int, channels: int) -> float:
    if frame < 0 or frame >= audio.frames:
        return 0.0
    if audio.channels == channels:
        return audio.samples[frame * audio.channels + channel]
    if audio.channels == 1:
        return audio.samples[frame * audio.channels]
    if channels == 1:
        start = frame * audio.channels
        return sum(audio.samples[start : start + audio.channels]) / audio.channels
    if channel < audio.channels:
        return audio.samples[frame * audio.channels + channel]
    return 0.0


def _oracle_align(audio: _OracleAudio, target: AudioShape) -> list[float]:
    output = [0.0] * (target.frame_count * target.channels)
    ratio = audio.rate / target.sample_rate_hz
    for target_frame in range(target.frame_count):
        position = target_frame * ratio
        source_frame = math.floor(position)
        fraction = position - source_frame
        for channel in range(target.channels):
            before = _oracle_sample(audio, source_frame, channel, target.channels)
            after = _oracle_sample(audio, source_frame + 1, channel, target.channels)
            output[target_frame * target.channels + channel] = before + (after - before) * fraction
    return output


def _oracle_pcm16(sample: float) -> int:
    value = sample if math.isfinite(sample) else 0.0
    value = max(-1.0, min(1.0, value))
    if value >= 1.0:
        return 32767
    if value <= -1.0:
        return -32768
    return round(value * 32767.0)


def _legacy_oracle(output: Path, cache: Path, target: AudioShape) -> None:
    # Frozen a83de09 stem_generation.py algorithm: full lists, global f64
    # positions and sequential clamping precede component requantization.
    cache.mkdir()
    aligned = {}
    for name, raw_name in (
        ("vocals", "vocals"),
        ("drums", "drums"),
        ("bass", "bass"),
        ("melody", "other"),
    ):
        path = min(output.rglob(f"{raw_name}.wav"))
        with wave.open(str(path), "rb") as reader:
            raw = reader.readframes(reader.getnframes())
            samples = [
                -1.0 if value == -32768 else value / 32767.0
                for (value,) in struct.iter_unpack("<h", raw)
            ]
            audio = _OracleAudio(reader.getframerate(), reader.getnchannels(), samples)
        aligned[name] = _oracle_align(audio, target)
    summed = [0.0] * len(aligned["drums"])
    for name in ("drums", "bass", "melody"):
        for index, sample in enumerate(aligned[name]):
            summed[index] = max(-1.0, min(1.0, summed[index] + sample))
    aligned["instrumental"] = summed
    for name in STEM_KINDS:
        with wave.open(str(cache / f"{name}.wav"), "wb") as writer:
            writer.setnchannels(target.channels)
            writer.setsampwidth(2)
            writer.setframerate(target.sample_rate_hz)
            writer.writeframes(b"".join(struct.pack("<h", _oracle_pcm16(x)) for x in aligned[name]))


def _wav(path: Path, rate: int, channels: int, samples: list[int], *, width: int = 2) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as writer:
        writer.setnchannels(channels)
        writer.setsampwidth(width)
        writer.setframerate(rate)
        if width == 2:
            writer.writeframes(b"".join(struct.pack("<h", value) for value in samples))
        else:
            writer.writeframes(bytes(value % 256 for value in samples))


def _outputs(root: Path, *, rate: int = 48_000, channels: int = 2, frames: int = 37) -> None:
    for seed, name in enumerate(("vocals", "drums", "bass", "other")):
        samples = [
            ((index * 7919 + seed * 19013) % 65536) - 32768 for index in range(frames * channels)
        ]
        _wav(root / "model" / "source" / f"{name}.wav", rate, channels, samples)


@pytest.mark.parametrize(
    ("source_rate", "target_rate"), [(48_000, 44_100), (22_050, 48_000), (11, 7)]
)
@pytest.mark.parametrize(
    ("source_channels", "target_channels"), [(1, 2), (2, 1), (2, 2), (2, 3), (3, 2)]
)
@pytest.mark.parametrize("frames", [49, 4117])
def test_streaming_set_is_byteexact_legacy_across_rate_channel_and_block_boundaries(
    tmp_path: Path,
    source_rate: int,
    target_rate: int,
    source_channels: int,
    target_channels: int,
    frames: int,
) -> None:
    output = tmp_path / "output"
    _outputs(output, rate=source_rate, channels=source_channels, frames=frames - 13)
    target = AudioShape(target_rate, target_channels, frames)
    legacy, streaming = tmp_path / "legacy", tmp_path / "streaming"
    _legacy_oracle(output, legacy, target)

    write_project_cache_artifacts(
        output_root=output, cache_dir=streaming, target_shape=target, progress=lambda _p, _s: None
    )

    assert {path.name for path in streaming.iterdir()} == {f"{name}.wav" for name in STEM_KINDS}
    for name in STEM_KINDS:
        assert (streaming / f"{name}.wav").read_bytes() == (legacy / f"{name}.wav").read_bytes()


def test_instrumental_clamps_each_component_before_adding_the_next(tmp_path: Path) -> None:
    output = tmp_path / "output"
    for name, value in (("vocals", 32767), ("drums", 32767), ("bass", 32767), ("other", -32768)):
        _wav(output / f"{name}.wav", 48_000, 1, [value])
    cache = tmp_path / "cache"

    write_project_cache_artifacts(
        output_root=output,
        cache_dir=cache,
        target_shape=AudioShape(48_000, 1, 1),
        progress=lambda _p, _s: None,
    )

    with wave.open(str(cache / "instrumental.wav"), "rb") as reader:
        assert reader.readframes(1) == b"\x00\x00"


def test_reads_are_bounded_spans_and_small_budget_adapts_blocks(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    output = tmp_path / "output"
    _outputs(output, frames=1000)
    requested = []
    original = wave.Wave_read.readframes

    def observe(reader: wave.Wave_read, frames: int) -> bytes:
        requested.append(frames)
        return original(reader, frames)

    monkeypatch.setattr(wave.Wave_read, "readframes", observe)
    target = AudioShape(8000, 2, 175)
    cache = tmp_path / "cache"
    write_project_cache_artifacts(
        output_root=output,
        cache_dir=cache,
        target_shape=target,
        progress=lambda _p, _s: None,
        block_frames=7,
        max_transient_pcm_bytes=1200,
    )

    assert requested
    assert max(requested) < 7 * 6 + 2
    assert all(frames < 1000 for frames in requested)
    legacy = tmp_path / "legacy"
    _legacy_oracle(output, legacy, target)
    for name in STEM_KINDS:
        assert (cache / f"{name}.wav").read_bytes() == (legacy / f"{name}.wav").read_bytes()


@pytest.mark.parametrize("failure", ["missing", "truncated", "width"])
def test_incomplete_source_set_preserves_existing_outputs_and_never_marks_complete(
    tmp_path: Path, failure: str
) -> None:
    output, cache = tmp_path / "output", tmp_path / "cache"
    _outputs(output, frames=100)
    path = output / "model" / "source" / "other.wav"
    if failure == "missing":
        path.unlink()
    elif failure == "truncated":
        path.write_bytes(path.read_bytes()[:-2])
    else:
        _wav(path, 48_000, 2, [0] * 200, width=1)
    cache.mkdir()
    for name in STEM_KINDS:
        (cache / f"{name}.wav").write_bytes(b"previous-generation")
    (cache / "unowned.wav.tmp").write_bytes(b"unknown")

    with pytest.raises(StemArtifactError):
        write_project_cache_artifacts(
            output_root=output,
            cache_dir=cache,
            target_shape=AudioShape(48_000, 1, 2),
            progress=lambda _p, _s: None,
        )

    assert not (cache / ".complete.json").exists()
    assert (cache / "unowned.wav.tmp").read_bytes() == b"unknown"
    for name in STEM_KINDS:
        assert (cache / f"{name}.wav").read_bytes() == b"previous-generation"


@pytest.mark.parametrize(
    "shape",
    [
        AudioShape(0, 1, 1),
        AudioShape(1, 0, 1),
        AudioShape(1, 32768, 1),
        AudioShape(0xFFFF_FFFF, 2, 1),
        AudioShape(48_000, 1, 0),
        AudioShape(48_000, 1, 0xFFFF_FFFF),
    ],
)
def test_pathological_target_shape_rejects_before_output_creation(
    tmp_path: Path, shape: AudioShape
) -> None:
    cache = tmp_path / "cache"

    with pytest.raises(StemArtifactError):
        write_project_cache_artifacts(
            output_root=tmp_path / "absent",
            cache_dir=cache,
            target_shape=shape,
            progress=lambda _p, _s: None,
        )

    assert not cache.exists()
    with pytest.raises(StemArtifactError):
        validate_target_shape(shape)


def test_minimum_target_block_exceeding_pcm_budget_rejects_without_final_output(
    tmp_path: Path,
) -> None:
    output, cache = tmp_path / "output", tmp_path / "cache"
    _outputs(output)

    with pytest.raises(StemArtifactError, match="transient PCM limit"):
        write_project_cache_artifacts(
            output_root=output,
            cache_dir=cache,
            target_shape=AudioShape(48_000, 100, 1),
            progress=lambda _p, _s: None,
            max_transient_pcm_bytes=1024,
        )

    assert list(cache.iterdir()) == []


def test_pcm_reader_caps_before_full_read(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    path = tmp_path / "source.wav"
    _wav(path, 48_000, 2, [0] * 200)
    called = []
    original = wave.Wave_read.readframes

    def observe(reader: wave.Wave_read, frames: int) -> bytes:
        called.append(frames)
        return original(reader, frames)

    monkeypatch.setattr(wave.Wave_read, "readframes", observe)
    with pytest.raises(StemArtifactError, match="transient PCM limit"):
        read_pcm16_wav(path, max_transient_pcm_bytes=1000)
    assert called == []

    audio = read_pcm16_wav(path, max_transient_pcm_bytes=2000)
    assert audio.channels == 2
    assert audio.frame_count == 100
    assert called == [100]


@pytest.mark.parametrize(
    ("data", "expected"), [(b"", EOFError), (b"malformed WAV output", wave.Error)]
)
def test_compatibility_reader_preserves_wave_parser_exceptions(
    tmp_path: Path, data: bytes, expected: type[Exception]
) -> None:
    path = tmp_path / "invalid.wav"
    path.write_bytes(data)
    with pytest.raises(expected):
        read_pcm16_wav(path)


@pytest.mark.parametrize("limit", [0, -1, stem_artifacts.MAX_TRANSIENT_PCM_BYTES + 1])
def test_caller_cannot_disable_or_increase_pcm_cap(tmp_path: Path, limit: int) -> None:
    with pytest.raises(StemArtifactError, match="at most 1 GiB"):
        write_project_cache_artifacts(
            output_root=tmp_path,
            cache_dir=tmp_path / "cache",
            target_shape=AudioShape(48_000, 1, 1),
            progress=lambda _p, _s: None,
            max_transient_pcm_bytes=limit,
        )


def test_instrumental_sums_interpolated_components_before_pcm16_requantization(
    tmp_path: Path,
) -> None:
    output, cache = tmp_path / "output", tmp_path / "cache"
    for name in ("vocals", "drums", "bass", "other"):
        _wav(output / f"{name}.wav", 1, 1, [1, 2])

    write_project_cache_artifacts(
        output_root=output,
        cache_dir=cache,
        target_shape=AudioShape(2, 1, 2),
        progress=lambda _p, _s: None,
    )

    with wave.open(str(cache / "drums.wav"), "rb") as reader:
        assert struct.unpack("<hh", reader.readframes(2)) == (1, 2)
    with wave.open(str(cache / "instrumental.wav"), "rb") as reader:
        assert struct.unpack("<hh", reader.readframes(2)) == (3, 4)


def test_independent_source_shapes_match_frozen_legacy_sum(tmp_path: Path) -> None:
    output = tmp_path / "output"
    shapes = ((7, 1, 31), (11, 2, 49), (13, 3, 77), (17, 4, 93))
    for seed, (name, (rate, channels, frames)) in enumerate(
        zip(("vocals", "drums", "bass", "other"), shapes, strict=True)
    ):
        values = [
            ((index * 997 + seed * 7919) % 65536) - 32768 for index in range(frames * channels)
        ]
        _wav(output / f"{name}.wav", rate, channels, values)
    target = AudioShape(13, 3, 101)
    legacy, cache = tmp_path / "legacy", tmp_path / "cache"
    _legacy_oracle(output, legacy, target)

    write_project_cache_artifacts(
        output_root=output,
        cache_dir=cache,
        target_shape=target,
        progress=lambda _p, _s: None,
        block_frames=5,
    )

    for name in STEM_KINDS:
        assert (cache / f"{name}.wav").read_bytes() == (legacy / f"{name}.wav").read_bytes()


def test_progress_is_constant_bounded_even_for_one_frame_blocks(tmp_path: Path) -> None:
    output, cache = tmp_path / "output", tmp_path / "cache"
    _outputs(output, frames=501)
    updates = []

    write_project_cache_artifacts(
        output_root=output,
        cache_dir=cache,
        target_shape=AudioShape(48_000, 2, 501),
        progress=lambda percent, stage: updates.append((percent, stage)),
        block_frames=1,
    )

    assert len(updates) <= 101
    assert updates[-1] == (1.0, "Stem cache ready")
    assert all(first[0] <= second[0] for first, second in pairwise(updates))


def test_oversized_source_frame_rejects_before_pcm_read(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    output, cache = tmp_path / "output", tmp_path / "cache"
    _outputs(output, channels=1024, frames=1)
    called = []
    original = wave.Wave_read.readframes

    def observe(reader: wave.Wave_read, frames: int) -> bytes:
        called.append(frames)
        return original(reader, frames)

    monkeypatch.setattr(wave.Wave_read, "readframes", observe)
    with pytest.raises(StemArtifactError, match="source PCM frame"):
        write_project_cache_artifacts(
            output_root=output,
            cache_dir=cache,
            target_shape=AudioShape(48_000, 1, 1),
            progress=lambda _p, _s: None,
            max_transient_pcm_bytes=512,
        )

    assert called == []
    assert list(cache.iterdir()) == []


def test_failed_temporary_output_write_preserves_existing_final_files(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    output, cache = tmp_path / "output", tmp_path / "cache"
    _outputs(output)
    cache.mkdir()
    for name in STEM_KINDS:
        (cache / f"{name}.wav").write_bytes(b"previous-generation")
    original = wave.Wave_write.writeframesraw
    writes = 0

    def fail(reader: wave.Wave_write, data: bytes | bytearray) -> None:
        nonlocal writes
        writes += 1
        if writes == 3:
            msg = "simulated disk output failure"
            raise OSError(msg)
        original(reader, data)

    monkeypatch.setattr(wave.Wave_write, "writeframesraw", fail)
    with pytest.raises(OSError, match="simulated disk output failure"):
        write_project_cache_artifacts(
            output_root=output,
            cache_dir=cache,
            target_shape=AudioShape(48_000, 2, 17),
            progress=lambda _p, _s: None,
        )

    assert {path.name for path in cache.iterdir()} == {f"{name}.wav" for name in STEM_KINDS}
    for name in STEM_KINDS:
        assert (cache / f"{name}.wav").read_bytes() == b"previous-generation"
