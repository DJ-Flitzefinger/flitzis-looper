"""Owned off-realtime recording codec work; Native/take permission stays with caller."""

import os
import shutil
import struct
import sys
from dataclasses import dataclass
from pathlib import Path
from threading import Event
from typing import Annotated

from pydantic import BaseModel, ConfigDict, Field, model_validator

from flitzis_looper.analysis.contracts import WorkerLimits
from flitzis_looper.analysis.process import run_owned_command
from flitzis_looper.encoder_provenance import supported_encoder_images
from flitzis_looper.recording_codec_commands import (
    recording_pcm_encoding,
    validate_recording_geometry,
)
from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.recording_pcm import convert_recording_pcm
from flitzis_looper_audio import SealedProcessingFiles

type _PositiveInt = Annotated[int, Field(strict=True, gt=0)]
type _FileNumber = Annotated[int, Field(strict=True, ge=0)]
type _Digest = Annotated[str, Field(pattern=r"^[0-9a-f]{64}$")]


class RecordingCodecRequest(BaseModel):
    """One exact raw-PCM encode attempt, with exclusive caller-owned output leaves."""

    model_config = ConfigDict(frozen=True, extra="forbid", strict=True)

    encoder: Path
    probe: Path
    source: Path
    output: Path
    decoded: Path
    source_identity: tuple[_FileNumber, _FileNumber]
    source_payload_sha256: _Digest
    options: RecordingOptions
    rate: _PositiveInt
    channels: _PositiveInt
    frames: _PositiveInt

    @model_validator(mode="after")
    def _geometry_and_paths(self) -> RecordingCodecRequest:
        validate_recording_geometry(self.rate, self.channels, self.frames)
        paths = (self.encoder, self.probe, self.source, self.output, self.decoded)
        if any(not path.is_absolute() or ".." in path.parts for path in paths):
            message = "recording codec paths must be absolute without parent traversal"
            raise ValueError(message)
        media = (self.source, self.output, self.decoded)
        if len({os.path.normcase(str(path)) for path in media}) != 3:
            message = "recording codec source and outputs must be distinct"
            raise ValueError(message)
        if len({path.parent for path in media}) != 1:
            message = "recording codec media must share the owned attempt directory"
            raise ValueError(message)
        return self


class RecordingCodecResult(BaseModel):
    """Closed verified codec evidence, without Native or publication permission."""

    model_config = ConfigDict(frozen=True, extra="forbid", strict=True)

    output_identity: tuple[_FileNumber, _FileNumber]
    output_bytes: _PositiveInt
    output_sha256: _Digest
    decoded_frames: _PositiveInt
    decoded_payload_sha256: _Digest
    mp3_skip_samples: Annotated[int, Field(strict=True, ge=0)] = 0
    mp3_discard_padding: Annotated[int, Field(strict=True, ge=0)] = 0


@dataclass(frozen=True, slots=True)
class RecordingCodecTools:
    """Retained encoder image witnesses and exact discovered executable paths."""

    encoder: Path
    probe: Path
    files: SealedProcessingFiles


@dataclass(frozen=True, slots=True)
class RecordingCodecReadiness:
    """Actual selected-codec roundtrip for immutable output geometry and options."""

    tools: RecordingCodecTools
    options: RecordingOptions
    rate: int
    channels: int
    proof: RecordingCodecResult


def discover_recording_codecs() -> RecordingCodecTools:
    """Reuse current discovery and bind executables/known shared encoder libraries."""
    # Discovery is parent-side. Codec children must load their request schema
    # without entering the App -> recording worker -> codec import cycle.
    from flitzis_looper.controller.stem_generation import (  # noqa: PLC0415
        _add_ffmpeg_dir_to_path,
    )

    environment = dict(os.environ)
    _add_ffmpeg_dir_to_path(environment)
    encoder = shutil.which("ffmpeg", path=environment.get("PATH"))
    probe = shutil.which("ffprobe", path=environment.get("PATH"))
    if encoder is None or probe is None:
        message = "FFmpeg/ffprobe unavailable for recording"
        raise FileNotFoundError(message)
    encoder_path, probe_path = Path(encoder).resolve(strict=True), Path(probe).resolve(strict=True)
    images = tuple(
        dict.fromkeys((encoder_path, probe_path, *supported_encoder_images(encoder_path)))
    )
    return RecordingCodecTools(
        encoder_path, probe_path, SealedProcessingFiles([str(path) for path in images])
    )


def run_recording_codec(
    request: RecordingCodecRequest,
    tools: RecordingCodecTools,
    *,
    cancel: Event,
    retired: Event,
    limits: WorkerLimits | None = None,
) -> RecordingCodecResult:
    """Run one child/encoder/probe tree under the existing supervisor until retirement.

    The caller keeps the namespace and original Native ticket, checks live terminal
    permission before calling, and retains source identity/contents throughout.
    This function never starts capture, publishes, deletes or retries a take.
    """
    retired.clear()
    tree_retired = Event()
    child_requested = False
    source_guard: SealedProcessingFiles | None = None
    try:
        if request.encoder != tools.encoder or request.probe != tools.probe:
            message = "recording codec request does not match its retained tools"
            raise ValueError(message)
        tools.files.verify()
        source_guard = SealedProcessingFiles([str(request.source)])
        source_guard.verify()
        state = request.source.stat()
        if (state.st_dev, state.st_ino) != request.source_identity:
            message = "recording PCM source identity changed"
            raise ValueError(message)
        environment = dict(os.environ)
        environment["PYTHONPATH"] = str(Path(__file__).resolve().parent.parent)
        selected_limits = limits or WorkerLimits(
            timeout_seconds=max(60.0, request.frames / request.rate * 4 + 30),
            max_response_bytes=16384,
        )
        child_requested = True
        result = run_owned_command(
            [
                sys.executable,
                "-m",
                "flitzis_looper.recording_codec_worker",
                request.model_dump_json(),
            ],
            cwd=request.source.parent,
            env=environment,
            cancel=cancel,
            retired=tree_retired,
            limits=selected_limits,
            abort_grace_seconds=min(0.2, selected_limits.reap_timeout_seconds),
        )
        tools.files.verify()
        source_guard.verify()
        if result.reason != "ok" or result.returncode != 0 or not tree_retired.is_set():
            detail = result.stderr[-4096:].decode("utf-8", errors="replace")
            message = f"recording codec {result.reason} (exit {result.returncode}): {detail}"
            raise RuntimeError(message)
        decoded = RecordingCodecResult.model_validate_json(result.stdout)
        if decoded.decoded_frames != request.frames:
            message = "recording codec response frame extent changed"
            raise ValueError(message)
        # Finalizer must retain/recheck the exact verified output before commit.
        return decoded
    finally:
        # Exceptions may retain this frame. Release this locally owned Native pin
        # before publishing wrapper retirement; tools remain caller-owned.
        source_guard = None
        if not child_requested or tree_retired.is_set():
            retired.set()


def preflight_recording_codec(
    tools: RecordingCodecTools,
    options: RecordingOptions,
    *,
    rate: int,
    channels: int,
    directory: Path,
    cancel: Event,
    retired: Event,
) -> RecordingCodecReadiness:
    """Exercise actual encode/verification at the requested geometry before launch.

    Call on the owning preparation worker, with an exclusive owned directory.
    Every leaf is created once; failure retains incomplete evidence. The finite
    proof grants codec readiness only, never capture/group/scheduler authority.
    """
    retired.clear()
    codec_retired = Event()
    codec_requested = False
    try:
        frames = 10001
        options = RecordingOptions.model_validate(options)
        validate_recording_geometry(rate, channels, frames)

        def checkpoint() -> None:
            if cancel.is_set():
                message = "recording codec preparation cancelled"
                raise InterruptedError(message)

        checkpoint()
        original = directory / "codec-ready-source.f32le"
        pcm = directory / "codec-ready-input.pcm"
        with original.open("xb") as output:
            block_frames = max(1, 4096 // channels)
            for first in range(0, frames, block_frames):
                checkpoint()
                block = bytearray()
                for frame in range(first, min(first + block_frames, frames)):
                    for channel in range(channels):
                        block.extend(
                            struct.pack("<f", ((frame * 17 + channel * 3) % 127 - 63) / 128)
                        )
                output.write(block)
            output.flush()
            os.fsync(output.fileno())
        with original.open("rb") as source, pcm.open("xb") as output:
            measurements = convert_recording_pcm(
                source,
                output,
                frames=frames,
                channels=channels,
                encoding=recording_pcm_encoding(options),
                checkpoint=checkpoint,
            )
            output.flush()
            os.fsync(output.fileno())
        state = pcm.stat()
        request = RecordingCodecRequest(
            encoder=tools.encoder,
            probe=tools.probe,
            source=pcm,
            output=directory / "codec-ready-encoded.partial",
            decoded=directory / "codec-ready-decoded.pcm",
            source_identity=(state.st_dev, state.st_ino),
            source_payload_sha256=measurements.output_payload_sha256,
            options=options,
            rate=rate,
            channels=channels,
            frames=frames,
        )
        codec_requested = True
        proof = run_recording_codec(request, tools, cancel=cancel, retired=codec_retired)
        return RecordingCodecReadiness(tools, options, rate, channels, proof)
    finally:
        if not codec_requested or codec_retired.is_set():
            retired.set()
