import hashlib
import math
import shutil
import struct
import wave
from typing import TYPE_CHECKING

import pytest
import torch
from flitzis_bs_roformer.audio import decode_source, read_virtual_chunk
from flitzis_bs_roformer.inference import ChunkPlan, separate_pcm
from flitzis_bs_roformer.model import MAX_TRANSIENT_PCM_BYTES, SAMPLE_RATE, SOURCES, verify_file
from torch import Tensor, nn
from torch.nn import functional

if TYPE_CHECKING:
    from pathlib import Path


class CopySources(nn.Module):
    def __init__(self) -> None:
        super().__init__()
        self.batches: list[int] = []

    def forward(self, data: Tensor) -> Tensor:
        self.batches.append(data.shape[0])
        weights = torch.tensor((1.0, 0.5, -0.5, 0.0)).view(1, 4, 1, 1)
        return data.unsqueeze(1) * weights


class MalformedSources(nn.Module):
    def forward(self, data: Tensor) -> Tensor:
        return data.unsqueeze(1).repeat(1, 3, 1, 1)


class NonFiniteSources(nn.Module):
    def forward(self, data: Tensor) -> Tensor:
        return data.unsqueeze(1).repeat(1, 4, 1, 1) * float("nan")


class LateNonFiniteSources(nn.Module):
    def __init__(self) -> None:
        super().__init__()
        self.calls = 0

    def forward(self, data: Tensor) -> Tensor:
        self.calls += 1
        multiplier = 0.25 if self.calls == 1 else float("nan")
        return data.unsqueeze(1).repeat(1, 4, 1, 1) * multiplier


class ChunkDependentSources(nn.Module):
    """Vary predictions by chunk ordinal and local frame so windows cannot cancel."""

    def __init__(self) -> None:
        super().__init__()
        self.next_chunk = 0

    def forward(self, data: Tensor) -> Tensor:
        predictions = []
        local = torch.arange(data.shape[-1], dtype=torch.float32).view(1, -1)
        channels = torch.tensor((1.0, 2.0)).view(2, 1)
        stems = torch.arange(4, dtype=torch.float32).view(4, 1, 1)
        for chunk in data:
            ordinal = self.next_chunk
            prediction = chunk * (0.25 + ordinal * 0.03)
            prediction += 0.005 * (ordinal + 1) + local * channels * 0.0004
            predictions.append(prediction.unsqueeze(0) + stems * 0.003)
            self.next_chunk += 1
        return torch.stack(predictions)


def _scalar_window_oracle(original: Tensor, chunk_size: int) -> list[list[int]]:
    """Independently perform the intended per-chunk overlap-add in scalar binary64."""
    frames = original.shape[-1]
    step = chunk_size // 2
    border = step if frames > 2 * step else 0
    channels = original.tolist()
    virtual = (
        [row[border:0:-1] + row + row[-2 : -border - 2 : -1] for row in channels]
        if border
        else channels
    )
    length = len(virtual[0])
    result = [[0.0] * length for _ in range(8)]
    counter = [0.0] * length
    fade = chunk_size // 10
    for ordinal, start in enumerate(range(0, length, step)):
        count = min(chunk_size, length - start)
        for local in range(count):
            weight = 1.0
            if start > 0 and local < fade:
                weight = local / (fade - 1)
            if start + step < length and local >= chunk_size - fade:
                weight *= (chunk_size - 1 - local) / (fade - 1)
            counter[start + local] += weight
            for channel in range(2):
                value = virtual[channel][start + local] * (0.25 + ordinal * 0.03)
                value += 0.005 * (ordinal + 1) + local * (channel + 1) * 0.0004
                for stem in range(4):
                    result[stem * 2 + channel][start + local] += (value + stem * 0.003) * weight
    output = [[] for _ in SOURCES]
    for frame in range(border, border + frames):
        assert counter[frame] > 0
        for stem in range(4):
            for channel in range(2):
                value = result[stem * 2 + channel][frame] / counter[frame]
                value = max(-1.0, min(1.0, value))
                output[stem].append(-32768 if value <= -1 else round(value * 32767))
    return output


def _source(path: Path, frames: int) -> Tensor:
    values = torch.tensor(
        [(0.43 * math.sin(index / 7), -0.29 * math.cos(index / 9)) for index in range(frames)],
        dtype=torch.float32,
    )
    path.write_bytes(b"".join(struct.pack("<ff", *frame) for frame in values.tolist()))
    return values.transpose(0, 1)


@pytest.mark.parametrize("batch_size", [1, 2])
@pytest.mark.parametrize("frames", [1, 31, 32, 33, 63, 64, 65, 96, 127, 128, 129, 317])
def test_streamed_overlap_add_preserves_source_origin_and_complete_tail(
    tmp_path: Path,
    frames: int,
    batch_size: int,
) -> None:
    decoded = tmp_path / "source.f32"
    original = _source(decoded, frames)
    source_digest = hashlib.sha256(decoded.read_bytes()).hexdigest()
    model = CopySources()
    output = tmp_path / "stems"

    separate_pcm(model, decoded, output, frames, "cpu", ChunkPlan(64, batch_size))

    assert max(model.batches) <= batch_size
    assert {path.name for path in output.iterdir()} == {f"{source}.wav" for source in SOURCES}
    for source, multiplier in zip(SOURCES, (1.0, 0.5, -0.5, 0.0), strict=True):
        with wave.open(str(output / f"{source}.wav"), "rb") as handle:
            assert (handle.getframerate(), handle.getnchannels(), handle.getnframes()) == (
                SAMPLE_RATE,
                2,
                frames,
            )
            actual = torch.frombuffer(bytearray(handle.readframes(frames)), dtype=torch.int16)
        expected = (original.transpose(0, 1).reshape(-1) * multiplier * 32767).round()
        assert bool((actual.to(torch.float32) - expected).abs().le(1).all())
    assert hashlib.sha256(decoded.read_bytes()).hexdigest() == source_digest


@pytest.mark.parametrize(("start", "length"), [(0, 20), (0, 64), (17, 64), (90, 44), (115, 51)])
def test_virtual_reflection_matches_torch_source_padding(
    tmp_path: Path,
    start: int,
    length: int,
) -> None:
    decoded = tmp_path / "source.f32"
    original = _source(decoded, 102)
    reference = functional.pad(original, (32, 32), mode="reflect")
    with decoded.open("rb") as handle:
        actual = read_virtual_chunk(handle, start, length, 102, 32)
    assert torch.equal(actual, reference[:, start : start + length])


@pytest.mark.parametrize("batch_size", [1, 2])
@pytest.mark.parametrize("frames", [1, 31, 32, 33, 63, 64, 65, 96, 127, 128, 129, 317])
def test_chunk_dependent_predictions_match_independent_per_chunk_window_oracle(
    tmp_path: Path,
    frames: int,
    batch_size: int,
) -> None:
    decoded = tmp_path / "source.f32"
    original = _source(decoded, frames)
    expected = _scalar_window_oracle(original, 64)
    output = tmp_path / "stems"

    separate_pcm(ChunkDependentSources(), decoded, output, frames, "cpu", ChunkPlan(64, batch_size))

    for source, reference in zip(SOURCES, expected, strict=True):
        with wave.open(str(output / f"{source}.wav"), "rb") as handle:
            assert handle.getnframes() == frames
            samples = struct.unpack(f"<{frames * 2}h", handle.readframes(frames))
        # The production accumulator/model arithmetic is float32; the oracle
        # uses independent scalar binary64 and final PCM16 rounding.
        assert (
            max(abs(actual - target) for actual, target in zip(samples, reference, strict=True))
            <= 1
        )


@pytest.mark.parametrize("model", [MalformedSources(), NonFiniteSources(), LateNonFiniteSources()])
def test_invalid_prediction_never_replaces_existing_complete_outputs(
    tmp_path: Path,
    model: nn.Module,
) -> None:
    decoded = tmp_path / "source.f32"
    _source(decoded, 317)
    output = tmp_path / "stems"
    output.mkdir()
    previous = {source: source.encode("ascii") for source in SOURCES}
    for source, data in previous.items():
        (output / f"{source}.wav").write_bytes(data)

    with pytest.raises(ValueError, match=r"(incomplete four-source|non-finite)"):
        separate_pcm(model, decoded, output, 317, "cpu", ChunkPlan(64))

    assert {path.name for path in output.iterdir()} == {f"{source}.wav" for source in SOURCES}
    assert all((output / f"{source}.wav").read_bytes() == data for source, data in previous.items())


@pytest.mark.parametrize("invalid", ["truncated", "nonfinite"])
def test_invalid_decoded_source_leaves_no_outputs(tmp_path: Path, invalid: str) -> None:
    decoded = tmp_path / "source.f32"
    _source(decoded, 127)
    if invalid == "truncated":
        decoded.write_bytes(decoded.read_bytes()[:-1])
    else:
        data = bytearray(decoded.read_bytes())
        struct.pack_into("<f", data, 0, float("inf"))
        decoded.write_bytes(data)
    output = tmp_path / "stems"

    with pytest.raises(ValueError, match=r"(incomplete PCM|non-finite)"):
        separate_pcm(CopySources(), decoded, output, 127, "cpu", ChunkPlan(64))

    assert not list(output.iterdir())


@pytest.mark.parametrize("plan", [ChunkPlan(10_000_000), ChunkPlan(64, 3), ChunkPlan(64, 1, 3)])
def test_invalid_pcm_plan_is_rejected_before_opening_any_audio(
    tmp_path: Path, plan: ChunkPlan
) -> None:
    output = tmp_path / "stems"
    with pytest.raises(ValueError, match=r"(1 GiB|Unsupported)"):
        separate_pcm(CopySources(), tmp_path / "missing.f32", output, 100, "cpu", plan)
    assert not output.exists()
    assert ChunkPlan().transient_pcm_bytes < MAX_TRANSIENT_PCM_BYTES


def test_installed_artifact_rejects_same_size_modified_bytes(tmp_path: Path) -> None:
    artifact = tmp_path / "checkpoint"
    artifact.write_bytes(b"first identity")
    digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
    verify_file(artifact, artifact.stat().st_size, digest)
    artifact.write_bytes(b"other identity")
    with pytest.raises(ValueError, match="artifact identity"):
        verify_file(artifact, artifact.stat().st_size, digest)


@pytest.mark.skipif(
    not shutil.which("ffmpeg") or not shutil.which("ffprobe"),
    reason="Direct FFmpeg decoder regression requires the explicit tool PATH",
)
def test_real_ffmpeg_decode_resamples_and_duplicates_mono(tmp_path: Path) -> None:
    source = tmp_path / "mono.wav"
    with wave.open(str(source), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(22_050)
        handle.writeframes(struct.pack("<h", 8000) * 2205)
    decoded = tmp_path / "source.f32"

    frames = decode_source(source, decoded)

    assert frames == 4410
    with decoded.open("rb") as handle:
        samples = read_virtual_chunk(handle, 0, frames, frames, 0)
    assert torch.equal(samples[0], samples[1])
    assert bool(torch.isfinite(samples).all())
    assert torch.allclose(samples, torch.full_like(samples, 8000 / 32768), atol=1e-6)
