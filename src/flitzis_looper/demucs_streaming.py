"""Run the unchanged Demucs CLI with a bounded RIFF/RF64 PCM16 output adapter.

Demucs still owns its whole-track neural input/output and normalization. Only
offline output clipping, host copies and WAV encoding are streamed here. FFmpeg
performs the same float32-to-signed-PCM16 conversion as TorchCodec's WAV encoder.
The entry point imports no controller or Native extension.
"""

import ctypes
import os
import subprocess
import sys
import tempfile
from contextlib import contextmanager
from dataclasses import dataclass, field
from functools import partial
from importlib import import_module
from pathlib import Path
from typing import TYPE_CHECKING, Literal, Protocol, cast
from uuid import uuid4

import torch

from flitzis_looper.encoder_provenance import (
    encoder_completion_line,
    inspect_encoder,
    record_encoder,
)
from flitzis_looper.pcm16_wav import PcmWavReader, discard_owned_file
from flitzis_looper.stem_cancellation import (
    CANCEL_EXIT_CODE,
    StemCancellation,
    StemInferenceCancelledError,
    check_cancelled,
    communicate_cancellable,
    emit_cancellation_ack,
    tool_environment,
    wait_cancellable,
)
from flitzis_looper.stem_execution import completed_execution_line

if TYPE_CHECKING:
    from argparse import Namespace
    from collections.abc import Callable, Iterator

    from torch import Tensor
    from torch.nn import Module

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


class _ApplyModel(Protocol):
    def __call__(self, *args: object, **kwargs: object) -> Tensor: ...


class _DemucsCLI(Protocol):
    save_audio: _SaveAudio
    get_model_from_args: Callable[[Namespace], Module]
    load_track: Callable[[Path, int, int], Tensor]
    apply_model: _ApplyModel

    def main(self) -> None: ...


def _input_command(args: list[str]) -> list[str]:
    executable = os.environ.get("FLITZIS_DEMUCS_ENCODER")
    if executable is not None and args[0] in {"ffmpeg", "ffprobe"}:
        tool = (
            Path(executable) if args[0] == "ffmpeg" else Path(executable).with_name("ffprobe.exe")
        )
        return [str(tool), *args[1:]]
    return args


class _InputTools:
    """Pin only stock Demucs AudioFile launches, retaining decode arguments and behavior."""

    def __init__(self, checkpoint: Callable[[str], None] | None = None) -> None:
        self.checkpoint = checkpoint

    def check_output(self, args: list[str]) -> bytes:
        check_cancelled(self.checkpoint, "before_input_probe")
        actual = _input_command(args)
        with subprocess.Popen(actual, stdout=subprocess.PIPE, env=tool_environment()) as process:
            stdout, _ = communicate_cancellable(process, self.checkpoint)
            result = subprocess.CompletedProcess(actual, process.returncode, stdout)
        result.check_returncode()
        check_cancelled(self.checkpoint, "after_input_probe")
        assert stdout is not None
        return stdout

    def run(self, args: list[str], *, check: bool) -> subprocess.CompletedProcess[bytes]:
        check_cancelled(self.checkpoint, "before_input_decode")
        actual = _input_command(args)
        with subprocess.Popen(actual, env=tool_environment()) as process:
            communicate_cancellable(process, self.checkpoint)
            result = subprocess.CompletedProcess[bytes](actual, process.returncode)
        if check:
            result.check_returncode()
        check_cancelled(self.checkpoint, "after_input_decode")
        return result


class _DemucsAudio(Protocol):
    sp: _InputTools


_separate = cast("_DemucsCLI", import_module("demucs.separate"))
_original_save_audio = cast("_SaveAudio", import_module("demucs.audio").save_audio)


def _clip_scale(
    wav: Tensor, clip: str, block_frames: int, checkpoint: Callable[[str], None] | None = None
) -> Tensor | int:
    if clip != "rescale":
        return 1
    maximum = wav.new_zeros(())
    for start in range(0, wav.shape[1], block_frames):
        check_cancelled(checkpoint, "output_clip_block")
        maximum = torch.maximum(maximum, wav[:, start : start + block_frames].abs().max())
    scaled = 1.01 * maximum
    return 1 if bool(scaled < 1) else scaled


def _write_input(
    process: subprocess.Popen[bytes],
    wav: Tensor,
    clip: str,
    frames: int,
    checkpoint: Callable[[str], None] | None = None,
) -> None:
    if process.stdin is None:
        msg = "Demucs encoder has no PCM input pipe"
        raise RuntimeError(msg)
    scale = _clip_scale(wav, clip, frames, checkpoint)
    with process.stdin:
        for start in range(0, wav.shape[1], frames):
            check_cancelled(checkpoint, "output_pcm_block")
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
                check_cancelled(checkpoint, "output_encoder_write")
                count = process.stdin.write(view)
                if count is None or count <= 0:
                    msg = "short Demucs encoder PCM input write"
                    raise OSError(msg)
                view = view[count:]


def _encoder_command(path: Path, rate: int, channels: int, *, force_rf64: bool) -> list[str]:
    return [
        os.environ.get("FLITZIS_DEMUCS_ENCODER", "ffmpeg"),
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
    path: Path,
    rate: int,
    channels: int,
    frames: int,
    identity: tuple[int, int],
    checkpoint: Callable[[str], None] | None = None,
) -> None:
    check_cancelled(checkpoint, "before_output_verification")
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
        verify_checkpoint = partial(checkpoint, "output_verification") if checkpoint else None
        reader.verify(checkpoint=verify_checkpoint)
    check_cancelled(checkpoint, "after_output_verification")


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
    checkpoint: Callable[[str], None] | None = None,
) -> None:
    """Stream only bounded float32 copies/clipping into a complete PCM16 WAV.

    The actual FFmpeg encoder controls PCM16 rounding, matching the pinned
    TorchCodec WAV path. Its process/codec memory is separate from the PCM bound.
    A private output is closed and fully scanned before replacing this job's leaf.
    """
    check_cancelled(checkpoint, "before_output_conversion")
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
                env=tool_environment(),
            ) as process:
                encoder_proof = inspect_encoder(process)
                try:
                    _write_input(process, wav, clip, block_frames, checkpoint)
                    wait_cancellable(process, checkpoint)
                except StemInferenceCancelledError:
                    process.kill()
                    process.wait()
                    raise
            if process.returncode:
                diagnostics.seek(0)
                detail = diagnostics.read(_DIAGNOSTIC_BYTES).decode("utf-8", errors="replace")
                msg = f"Demucs PCM16 encoder failed: {detail.strip()}"
                raise RuntimeError(msg)
        _verify_output(temporary, sample_rate_hz, wav.shape[0], wav.shape[1], identity, checkpoint)
        current = temporary.stat(follow_symlinks=False)
        if (current.st_dev, current.st_ino) != identity:
            msg = "Demucs attempt output file identity changed"
            raise RuntimeError(msg)
        check_cancelled(checkpoint, "before_output_replace")
        temporary.replace(path)
        record_encoder(path, encoder_proof)
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
    checkpoint: Callable[[str], None] | None = None,
) -> None:
    """Adapt only Demucs PCM16 WAV output; retain its other explicit formats."""
    check_cancelled(checkpoint, "before_save_audio")
    destination = Path(path)
    if destination.suffix.lower() == ".wav" and bits_per_sample == 16 and not as_float:
        write_demucs_pcm16(wav, destination, samplerate, clip=clip, checkpoint=checkpoint)
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
    check_cancelled(checkpoint, "after_save_audio")


class _DemucsApply(Protocol):
    apply_model: _ApplyModel


@dataclass
class _ExecutionObservation:
    model_name: str = ""
    devices: set[str] = field(default_factory=set)


@contextmanager
def _cancellable_cli(cancellation: StemCancellation) -> Iterator[_ExecutionObservation]:
    apply = cast("_DemucsApply", import_module("demucs.apply"))
    audio = cast("_DemucsAudio", import_module("demucs.audio"))
    original_model = _separate.get_model_from_args
    original_track = _separate.load_track
    original_cli_apply = _separate.apply_model
    original_apply = apply.apply_model
    original_audio = _separate.save_audio
    original_tools = audio.sp
    execution = _ExecutionObservation()

    def load_model(args: Namespace) -> Module:
        cancellation.checkpoint("before_model_load")
        model = original_model(args)
        execution.model_name = str(getattr(args, "name", ""))
        cancellation.checkpoint("after_model_load")
        return model

    def load_track(track: Path, channels: int, rate: int) -> Tensor:
        cancellation.checkpoint("before_source_decode")
        wav = original_track(track, channels, rate)
        cancellation.checkpoint("after_source_decode")
        return wav

    def apply_model(*args: object, **kwargs: object) -> Tensor:
        cancellation.checkpoint("before_model_batch")
        prediction = original_apply(*args, **kwargs)
        # Stock Demucs moves computation to this explicit device, and may return
        # the completed PCM on CPU. Failed calls never confirm an execution.
        device = str(kwargs.get("device", "unknown")).split(":", 1)[0]
        execution.devices.add(device)
        cancellation.checkpoint("after_model_batch")
        return prediction

    _separate.get_model_from_args = load_model
    _separate.load_track = load_track
    _separate.apply_model = apply_model
    apply.apply_model = apply_model
    _separate.save_audio = partial(save_audio, checkpoint=cancellation.checkpoint)
    audio.sp = _InputTools(cancellation.checkpoint)
    try:
        yield execution
    finally:
        _separate.get_model_from_args = original_model
        _separate.load_track = original_track
        _separate.apply_model = original_cli_apply
        apply.apply_model = original_apply
        _separate.save_audio = original_audio
        audio.sp = original_tools


def main() -> None:
    """Execute Demucs' original CLI/model after installing the scoped output adapter."""
    cancellation = StemCancellation.from_environment()
    cancellation.checkpoint("before_model_stage")
    with _cancellable_cli(cancellation) as execution:
        _separate.main()
    cancellation.checkpoint("before_completion_receipt")
    if (
        len(execution.devices) == 1
        and execution.devices <= {"cpu", "cuda"}
        and execution.model_name
    ):
        sys.stdout.write(
            completed_execution_line("demucs", execution.model_name, next(iter(execution.devices)))
            + "\n"
        )
    sys.stdout.write(encoder_completion_line() + "\n")
    sys.stdout.flush()


if __name__ == "__main__":
    try:
        main()
    except StemInferenceCancelledError as error:
        emit_cancellation_ack(error)
        sys.exit(CANCEL_EXIT_CODE)
