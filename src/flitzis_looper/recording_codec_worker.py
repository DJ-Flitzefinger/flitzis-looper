"""One supervised recording codec child; all media output is seekable and exclusive."""

import ctypes
import json
import os
import subprocess
import sys
from contextlib import ExitStack, contextmanager
from ctypes import wintypes
from dataclasses import dataclass
from typing import TYPE_CHECKING

from flitzis_looper.recording_codec import RecordingCodecRequest, RecordingCodecResult
from flitzis_looper.recording_codec_commands import (
    recording_decode_command,
    recording_encode_command,
    recording_packet_command,
    recording_pcm_encoding,
    recording_stream_command,
)
from flitzis_looper.recording_file_checks import hash_recording_extent
from flitzis_looper.recording_probe import validate_recording_streams, verify_mp3_packets
from flitzis_looper.recording_wav import inspect_recording_wav
from flitzis_looper_audio import SealedProcessingFiles

if os.name == "nt":
    import msvcrt

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path

_WIDTH = {"float32": 4, "pcm16": 2, "pcm24": 3}


def _output_opener(path: str, flags: int) -> int:
    """Create original Windows media handle denying foreign write/delete opens."""
    if os.name != "nt":
        return os.open(path, flags, 0o600)
    api = ctypes.WinDLL("kernel32", use_last_error=True)
    api.CreateFileW.argtypes = (
        wintypes.LPCWSTR,
        wintypes.DWORD,
        wintypes.DWORD,
        ctypes.c_void_p,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.HANDLE,
    )
    api.CreateFileW.restype = wintypes.HANDLE
    api.CloseHandle.argtypes = (wintypes.HANDLE,)
    api.CloseHandle.restype = wintypes.BOOL
    # The encoder inherits a duplicate of this exact writable handle. It never
    # needs another path open; independent writers/deleters cannot share it.
    handle = api.CreateFileW(str(path), 0xC0000000, 0x1, None, 1, 0x00200080, None)
    if handle == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        return msvcrt.open_osfhandle(int(handle), os.O_RDWR | os.O_BINARY)
    except OSError:
        api.CloseHandle(handle)
        raise


@dataclass(frozen=True, slots=True)
class _CommandFile:
    identity: tuple[int, int]
    size: int
    sha256: str


@contextmanager
def _command_to_file(arguments: list[str], destination: Path) -> Iterator[_CommandFile]:
    """Retain the producing FileID/content through every use of its evidence."""
    errors = destination.with_name(destination.name + ".stderr")
    with (
        open(destination, "x+b", opener=_output_opener) as output,
        errors.open("xb") as diagnostics,
    ):
        result = subprocess.run(
            arguments,
            stdin=subprocess.DEVNULL,
            stdout=output,
            stderr=diagnostics,
            check=False,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
        )
        output.flush()
        os.fsync(output.fileno())
        state = os.fstat(output.fileno())
        output.seek(0)
        produced = _CommandFile(
            (state.st_dev, state.st_ino),
            state.st_size,
            hash_recording_extent(output, state.st_size),
        )
    if result.returncode != 0:
        with errors.open("rb") as diagnostics:
            diagnostics.seek(max(0, os.fstat(diagnostics.fileno()).st_size - 4096))
            detail = diagnostics.read(4096).decode("utf-8", errors="replace")
        message = f"recording media command exited {result.returncode}: {detail}"
        raise RuntimeError(message)
    guard = SealedProcessingFiles([str(destination)])
    try:
        state = destination.stat()
        witnesses = guard.witnesses()
        if (
            (state.st_dev, state.st_ino) != produced.identity
            or state.st_size != produced.size
            or len(witnesses) != 1
            or witnesses[0][1:] != (produced.sha256, produced.size)
        ):
            message = "recording command output changed before its retained file pin"
            raise ValueError(message)
        yield produced
    finally:
        # Exceptions/tracebacks may retain this generator; terminal custody must
        # not depend on collecting it. The yielded DTO has no Native reference.
        del guard


def _packet_lines(path: Path) -> Iterator[bytes]:
    with path.open("rb") as source:
        while line := source.readline(8193):
            if len(line) > 8192:
                message = "recording packet evidence line exceeds its bound"
                raise ValueError(message)
            yield line


def _check_source(request: RecordingCodecRequest) -> None:
    encoding = recording_pcm_encoding(request.options)
    with request.source.open("rb") as source:
        state = os.fstat(source.fileno())
        count = request.frames * request.channels * _WIDTH[encoding]
        if (state.st_dev, state.st_ino) != request.source_identity or state.st_size != count:
            message = "recording raw PCM identity or extent changed"
            raise ValueError(message)
        digest = hash_recording_extent(source, count, finite_float=encoding == "float32")
    if digest != request.source_payload_sha256:
        message = "recording raw PCM digest changed"
        raise ValueError(message)


def _verify_lossless_wav(request: RecordingCodecRequest) -> None:
    with request.output.open("rb") as source:
        info = inspect_recording_wav(source)
        if (info.encoding, info.rate, info.channels, info.frames) != (
            request.options.wav_encoding,
            request.rate,
            request.channels,
            request.frames,
        ):
            message = "recording WAV container geometry changed"
            raise ValueError(message)
        source.seek(info.data_offset)
        digest = hash_recording_extent(
            source, info.data_bytes, finite_float=info.encoding == "float32"
        )
    if digest != request.source_payload_sha256:
        message = "recording WAV changed its selected PCM payload"
        raise ValueError(message)


def encode_and_verify_recording(request: RecordingCodecRequest) -> RecordingCodecResult:
    """Encode one raw input, independently inspect/decode it, and close every file.

    The parent supervisor owns this process and every encoder/probe descendant.
    It also owns cancellation/deadlines and file/take lifetimes. This worker
    grants no capture, original-spool success or final publication authority.
    """
    _check_source(request)
    with ExitStack() as pins:
        encoded = pins.enter_context(
            _command_to_file(
                recording_encode_command(
                    request.encoder,
                    request.source,
                    request.options,
                    rate=request.rate,
                    channels=request.channels,
                ),
                request.output,
            )
        )
        metadata = request.output.with_name(request.output.name + ".streams.json")
        pins.enter_context(
            _command_to_file(recording_stream_command(request.probe, request.output), metadata)
        )
        with metadata.open("rb") as source:
            content = source.read(65537)
        if len(content) > 65536:
            message = "recording stream metadata exceeds its bound"
            raise ValueError(message)
        info = validate_recording_streams(
            json.loads(content),
            request.options,
            rate=request.rate,
            channels=request.channels,
            frames=request.frames,
        )
        skip_samples, discard_padding = 0, 0
        if request.options.format == "mp3":
            packets = request.output.with_name(request.output.name + ".packets")
            pins.enter_context(
                _command_to_file(recording_packet_command(request.probe, request.output), packets)
            )
            trim = verify_mp3_packets(
                _packet_lines(packets),
                info,
                frames=request.frames,
                bitrate_kbps=request.options.mp3_bitrate_kbps,
            )
            skip_samples, discard_padding = trim.skip_samples, trim.discard_padding
        if request.options.format == "wav":
            _verify_lossless_wav(request)
            decoded_digest = request.source_payload_sha256
        else:
            pins.enter_context(
                _command_to_file(
                    recording_decode_command(request.encoder, request.output, request.options),
                    request.decoded,
                )
            )
            encoding = (
                "float32"
                if request.options.format == "mp3"
                else recording_pcm_encoding(request.options)
            )
            count = request.frames * request.channels * _WIDTH[encoding]
            with request.decoded.open("rb") as source:
                if os.fstat(source.fileno()).st_size != count:
                    message = "decoded recording frame extent changed"
                    raise ValueError(message)
                decoded_digest = hash_recording_extent(
                    source, count, finite_float=encoding == "float32"
                )
            if request.options.format == "flac" and decoded_digest != request.source_payload_sha256:
                message = "lossless recording changed its quantized PCM payload"
                raise ValueError(message)
        with request.output.open("rb") as source:
            state = os.fstat(source.fileno())
            digest = hash_recording_extent(source, state.st_size)
        if (state.st_dev, state.st_ino) != encoded.identity or digest != encoded.sha256:
            message = "recording encoded file changed during verification"
            raise ValueError(message)
        return RecordingCodecResult(
            output_identity=encoded.identity,
            output_bytes=encoded.size,
            output_sha256=digest,
            decoded_frames=request.frames,
            decoded_payload_sha256=decoded_digest,
            mp3_skip_samples=skip_samples,
            mp3_discard_padding=discard_padding,
        )


def main() -> None:
    """Accept a bounded metadata request; errors remain supervised diagnostics."""
    if len(sys.argv) != 2 or len(sys.argv[1]) > 16384:
        message = "recording worker expects one bounded request"
        raise ValueError(message)
    request = RecordingCodecRequest.model_validate_json(sys.argv[1])
    result = encode_and_verify_recording(request)
    sys.stdout.write(result.model_dump_json() + "\n")
    sys.stdout.flush()


if __name__ == "__main__":
    main()
