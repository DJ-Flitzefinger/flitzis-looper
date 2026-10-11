"""Actual FFmpeg output parity with the pinned Demucs/TorchCodec encoder."""

import ctypes
import shutil
from importlib import import_module
from typing import TYPE_CHECKING

import pytest
import torch

from flitzis_looper import demucs_streaming
from flitzis_looper.demucs_streaming import write_demucs_pcm16
from flitzis_looper.pcm16_wav import PcmWavReader

if TYPE_CHECKING:
    from pathlib import Path

pytestmark = pytest.mark.skipif(
    not shutil.which("ffmpeg"), reason="Actual encoder oracle requires the explicit FFmpeg PATH"
)


def _samples() -> torch.Tensor:
    # Half-integer PCM thresholds, opposite signs, clamp endpoints and a final
    # non-divisible block distinguish an actual encoder from a self-derived rule.
    values = (
        -2.0,
        -1.0,
        -0.99,
        -32767.5 / 32768,
        -1.5 / 32768,
        -0.5 / 32768,
        0.0,
        0.5 / 32768,
        1.5 / 32768,
        32766.5 / 32768,
        0.99,
        1.0,
        2.0,
    )
    left = torch.tensor(values * 317 + values[:5], dtype=torch.float32)
    return torch.stack((left, -left.flip(0)))


@pytest.mark.parametrize("partition", [1, 7, 4096])
@pytest.mark.parametrize("clip", ["clamp", "rescale", "none"])
def test_actual_streaming_rf64_pcm_bits_match_original_demucs_torchcodec_writer(
    tmp_path: Path, partition: int, clip: str
) -> None:
    original = tmp_path / "original.wav"
    actual = tmp_path / "streamed.wav"
    source = _samples()
    import_module("demucs.audio").save_audio(
        source.clone(), original, 44100, clip=clip, bits_per_sample=16
    )
    sizes = []
    write_demucs_pcm16(source, actual, 44100, clip=clip, block_frames=partition, force_rf64=True)
    with PcmWavReader(original) as reference, PcmWavReader(actual) as streamed:
        assert streamed.getnframes() == reference.getnframes() == source.shape[1]
        assert streamed.getnchannels() == reference.getnchannels() == 2
        for begin in range(0, source.shape[1], 17):
            expected = reference.readframes(17)
            actual_bytes = streamed.readframes(17)
            sizes.append(len(actual_bytes))
            assert actual_bytes == expected, f"PCM16 mismatch at frame {begin}"
        assert streamed.readframes(1) == reference.readframes(1) == b""
        assert streamed.verify()
    assert max(sizes) <= 17 * 2 * 2
    with actual.open("rb") as handle:
        assert handle.read(4) == b"RF64"


def test_encoder_host_copies_are_bounded_at_arbitrary_block_partitions(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sizes = []
    original = ctypes.string_at

    def observe(pointer: int, count: int) -> bytes:
        sizes.append(count)
        return original(pointer, count)

    monkeypatch.setattr(ctypes, "string_at", observe)
    source = _samples()
    write_demucs_pcm16(source, tmp_path / "actual.wav", 48000, block_frames=7)
    assert sizes
    assert max(sizes) <= 7 * 2 * 4
    assert sum(sizes) == source.numel() * 4


def test_actual_encoder_failure_preserves_prior_leaf_and_removes_attempt_output(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = tmp_path / "prior.wav"
    path.write_bytes(b"prior job output")
    original = demucs_streaming._encoder_command

    def unsupported(path: Path, rate: int, channels: int, *, force_rf64: bool) -> list[str]:
        command = original(path, rate, channels, force_rf64=force_rf64)
        command[command.index("-c:a") + 1] = "flitzis_owned_invalid_encoder"
        return command

    monkeypatch.setattr(demucs_streaming, "_encoder_command", unsupported)
    with pytest.raises((RuntimeError, BrokenPipeError)):
        write_demucs_pcm16(torch.zeros((2, 2)), path, 48000)
    assert path.read_bytes() == b"prior job output"
    assert [leaf.name for leaf in tmp_path.iterdir()] == ["prior.wav"]


def test_nonfinite_output_failure_retires_attempt_after_encoder_returns(tmp_path: Path) -> None:
    path = tmp_path / "output.wav"
    source = torch.zeros((2, 4193))
    source[1, -1] = float("nan")
    with pytest.raises(ValueError, match="non-finite"):
        write_demucs_pcm16(source, path, 48000, block_frames=7)
    assert not list(tmp_path.iterdir())
