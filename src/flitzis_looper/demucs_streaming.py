"""Run the unchanged Demucs CLI with a bounded RIFF/RF64 PCM16 output adapter.

Demucs still owns its whole-track neural input/output and normalization. Only
offline output clipping, host copies and WAV encoding are streamed here. FFmpeg
performs the same float32-to-signed-PCM16 conversion as TorchCodec's WAV encoder.
The entry point imports no controller or Native extension.
"""

import ctypes
import os
import subprocess
import tempfile
from importlib import import_module
from pathlib import Path
from typing import TYPE_CHECKING, Literal, Protocol, cast
from uuid import uuid4

import torch

from flitzis_looper.pcm16_wav import PcmWavReader, discard_owned_file

if TYPE_CHECKING:
    from torch import Tensor

_BLOCK_FRAMES = 4096
_DIAGNOSTIC_BYTES = 64 * 1024
_MAX_BLOCK_BYTES = 1024 * 1024 * 1024


class _SaveAudio(Protocol):
    def __call__(
        self,
        wav: Tensor,
        path: str | Path,
        samplerate: int,
        bitrate: int = 320,
        clip: Literal["rescale", "clamp", "tanh", "none"] = "rescale",
        bits_per_sample: Literal[16, 24, 32] = 16,
        *,
        as_float: bool = False,
        preset: Literal[2, 3, 4, 5, 6, 7] = 2,
    ) -> None: ...


class _DemucsCLI(Protocol):
    save_audio: _SaveAudio

    def main(self) -> None: ...


_separate = cast("_DemucsCLI", import_module("demucs.separate"))
_original_save_audio = cast("_SaveAudio", import_module("demucs.audio").save_audio)


def _clip_scale(wav: Tensor, clip: str, block_frames: int) -> Tensor | int:
    if clip != "rescale":
        return 1
    maximum = wav.new_zeros(())
    for start in range(0, wav.shape[1], block_frames):
        maximum = torch.maximum(maximum, wav[:, start : start + block_frames].abs().max())
    scaled = 1.01 * maximum
    return 1 if bool(scaled < 1) else scaled


def _write_input(process: subprocess.Popen[bytes], wav: Tensor, clip: str, frames: int) -> None:
    if process.stdin is None:
        msg = "Demucs encoder has no PCM input pipe"
        raise RuntimeError(msg)
    scale = _clip_scale(wav, clip, frames)
    with process.stdin:
        for start in range(0, wav.shape[1], frames):
            block = wav[:, start : start + frames]
            if clip == "clamp":
                block = block.clamp(-0.99, 0.99)
            elif clip == "rescale":
                block = block.div(scale)
            elif clip == "tanh":
                block = block.tanh()
            block = block.to(device="cpu", dtype=torch.float32).transpose(0, 1).contiguous()
            if not bool(torch.isfinite(block).all()):
                msg = "Demucs output contains non-finite PCM"
                raise ValueError(msg)
            raw = ctypes.string_at(block.data_ptr(), block.numel() * 4)
            view = memoryview(raw)
            while view:
                count = process.stdin.write(view)
                if count is None or count <= 0:
                    msg = "short Demucs encoder PCM input write"
                    raise OSError(msg)
                view = view[count:]


def _encoder_command(path: Path, rate: int, channels: int, *, force_rf64: bool) -> list[str]:
    return [
        "ffmpeg",
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "f32le",
        "-ar",
        str(rate),
        "-ac",
        str(channels),
        "-i",
        "pipe:0",
        "-map_metadata",
        "-1",
        "-c:a",
        "pcm_s16le",
        "-rf64",
        "always" if force_rf64 else "auto",
        "-f",
        "wav",
        str(path),
    ]


def _verify_output(
    path: Path, rate: int, channels: int, frames: int, identity: tuple[int, int]
) -> None:
    with PcmWavReader(path) as reader:
        if reader.file_identity != identity:
            msg = "Demucs attempt output file identity changed"
            raise RuntimeError(msg)
        if (
            reader.getframerate() != rate
            or reader.getnchannels() != channels
            or reader.getnframes() != frames
            or reader.getsampwidth() != 2
        ):
            msg = "Demucs encoder produced incomplete PCM16 WAV geometry"
            raise RuntimeError(msg)
        reader.verify()


def _validate_output(wav: Tensor, rate: int, frames: int, clip: str) -> None:
    if wav.ndim != 2 or min(wav.shape) <= 0 or not wav.dtype.is_floating_point:
        msg = "unsupported Demucs PCM16 output shape"
        raise ValueError(msg)
    if (
        rate <= 0
        or not 0 < frames <= _BLOCK_FRAMES
        or wav.shape[0] * frames * 4 * 5 > _MAX_BLOCK_BYTES
        or clip not in {"clamp", "rescale", "tanh", "none"}
    ):
        msg = "unsupported Demucs PCM16 clip or scratch block"
        raise ValueError(msg)


def write_demucs_pcm16(
    wav: Tensor,
    path: Path,
    sample_rate_hz: int,
    *,
    clip: str = "clamp",
    block_frames: int = _BLOCK_FRAMES,
    force_rf64: bool = False,
) -> None:
    """Stream only bounded float32 copies/clipping into a complete PCM16 WAV.

    The actual FFmpeg encoder controls PCM16 rounding, matching the pinned
    TorchCodec WAV path. Its process/codec memory is separate from the PCM bound.
    A private output is closed and fully scanned before replacing this job's leaf.
    """
    _validate_output(wav, sample_rate_hz, block_frames, clip)
    temporary = path.with_name(f".{path.name}.{uuid4().hex}.part")
    identity = None
    try:
        with temporary.open("xb") as reservation:
            metadata = os.fstat(reservation.fileno())
            identity = (metadata.st_dev, metadata.st_ino)
        with tempfile.TemporaryFile(dir=path.parent) as diagnostics:
            with subprocess.Popen(
                _encoder_command(temporary, sample_rate_hz, wav.shape[0], force_rf64=force_rf64),
                stdin=subprocess.PIPE,
                stdout=subprocess.DEVNULL,
                stderr=diagnostics,
            ) as process:
                _write_input(process, wav, clip, block_frames)
            if process.returncode:
                diagnostics.seek(0)
                detail = diagnostics.read(_DIAGNOSTIC_BYTES).decode("utf-8", errors="replace")
                msg = f"Demucs PCM16 encoder failed: {detail.strip()}"
                raise RuntimeError(msg)
        _verify_output(temporary, sample_rate_hz, wav.shape[0], wav.shape[1], identity)
        current = temporary.stat(follow_symlinks=False)
        if (current.st_dev, current.st_ino) != identity:
            msg = "Demucs attempt output file identity changed"
            raise RuntimeError(msg)
        temporary.replace(path)
    finally:
        if identity is not None:
            discard_owned_file(temporary, identity)


def save_audio(
    wav: Tensor,
    path: str | Path,
    samplerate: int,
    bitrate: int = 320,
    clip: Literal["rescale", "clamp", "tanh", "none"] = "rescale",
    bits_per_sample: Literal[16, 24, 32] = 16,
    *,
    as_float: bool = False,
    preset: Literal[2, 3, 4, 5, 6, 7] = 2,
) -> None:
    """Adapt only Demucs PCM16 WAV output; retain its other explicit formats."""
    destination = Path(path)
    if destination.suffix.lower() == ".wav" and bits_per_sample == 16 and not as_float:
        write_demucs_pcm16(wav, destination, samplerate, clip=clip)
    else:
        _original_save_audio(
            wav,
            destination,
            samplerate,
            bitrate=bitrate,
            clip=clip,
            bits_per_sample=bits_per_sample,
            as_float=as_float,
            preset=preset,
        )


def main() -> None:
    """Execute Demucs' original CLI/model after installing the scoped output adapter."""
    _separate.save_audio = save_audio
    _separate.main()


if __name__ == "__main__":
    main()
