"""Actual hardware-free codec trees, selected geometry and closed media evidence."""

import hashlib
import json
import os
import subprocess
import sys
from contextlib import contextmanager
from pathlib import Path
from threading import Event
from typing import TYPE_CHECKING

import pytest

from flitzis_looper import recording_codec, recording_codec_worker
from flitzis_looper.recording_codec import (
    RecordingCodecRequest,
    RecordingCodecTools,
    discover_recording_codecs,
    preflight_recording_codec,
    run_recording_codec,
)
from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.recording_pcm import convert_recording_pcm
from flitzis_looper_audio import SealedProcessingFiles

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator
    from typing import BinaryIO

    from flitzis_looper.recording_options import WavEncoding
    from flitzis_looper.recording_pcm import RecordingPcmStats


def test_codec_worker_imports_in_a_fresh_process_without_app_startup(tmp_path: Path) -> None:
    environment = dict(os.environ)
    environment["PYTHONPATH"] = str(Path(recording_codec.__file__).resolve().parent.parent)
    result = subprocess.run(
        [sys.executable, "-B", "-c", "import flitzis_looper.recording_codec_worker"],
        cwd=tmp_path,
        env=environment,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr


@pytest.fixture(scope="module")
def tools() -> RecordingCodecTools:
    """Keep actual executable/shared-library witnesses for this bounded suite."""
    return discover_recording_codecs()


@pytest.mark.parametrize(
    ("options", "rate"),
    [
        (RecordingOptions(wav_encoding="float32"), 48000),
        (RecordingOptions(wav_encoding="pcm16"), 48000),
        (RecordingOptions(wav_encoding="pcm24"), 48000),
        (RecordingOptions(format="flac", flac_bit_depth=16, flac_compression=0), 48000),
        (RecordingOptions(format="flac", flac_bit_depth=16, flac_compression=8), 48000),
        (RecordingOptions(format="flac", flac_bit_depth=24, flac_compression=0), 48000),
        (RecordingOptions(format="flac", flac_bit_depth=24, flac_compression=8), 48000),
        (RecordingOptions(format="mp3", mp3_bitrate_kbps=128), 48000),
        (RecordingOptions(format="mp3", mp3_bitrate_kbps=192), 48000),
        (RecordingOptions(format="mp3", mp3_bitrate_kbps=256), 48000),
        (RecordingOptions(format="mp3", mp3_bitrate_kbps=320), 48000),
        (RecordingOptions(format="mp3", mp3_bitrate_kbps=320), 44100),
    ],
)
def test_actual_selected_codec_roundtrip_is_closed_verified_and_retired(
    tmp_path: Path, tools: RecordingCodecTools, options: RecordingOptions, rate: int
) -> None:
    retired = Event()
    ready = preflight_recording_codec(
        tools,
        options,
        rate=rate,
        channels=2,
        directory=tmp_path,
        cancel=Event(),
        retired=retired,
    )
    assert retired.is_set()
    assert ready.options == options
    assert ready.rate == rate
    assert ready.channels == 2
    assert ready.proof.decoded_frames == 10001
    output = tmp_path / "codec-ready-encoded.partial"
    state = output.stat()
    assert ready.proof.output_identity == (state.st_dev, state.st_ino)
    assert ready.proof.output_bytes == state.st_size
    assert ready.proof.output_sha256 == hashlib.sha256(output.read_bytes()).hexdigest()
    # Kept actual command/stream/packet files are independent diagnostic evidence.
    metadata = json.loads((tmp_path / "codec-ready-encoded.partial.streams.json").read_bytes())
    assert int(metadata["streams"][0]["sample_rate"]) == rate
    assert metadata["streams"][0]["channels"] == 2
    if options.format == "mp3":
        assert ready.proof.mp3_skip_samples > 0
        assert ready.proof.mp3_discard_padding >= 0
        assert (tmp_path / "codec-ready-encoded.partial.packets").is_file()
    else:
        assert ready.proof.mp3_skip_samples == 0
        assert ready.proof.mp3_discard_padding == 0
    # No lingering child/output handle is needed for this evidence to remain valid.
    with output.open("r+b") as closed_output:
        assert closed_output.seek(0, 2) == ready.proof.output_bytes


@pytest.mark.parametrize(("rate", "channels"), [(96000, 2), (48000, 3)])
def test_actual_mp3_unsupported_geometry_never_returns_readiness(
    tmp_path: Path, tools: RecordingCodecTools, rate: int, channels: int
) -> None:
    retired = Event()
    with pytest.raises(RuntimeError, match="recording codec"):
        preflight_recording_codec(
            tools,
            RecordingOptions(format="mp3"),
            rate=rate,
            channels=channels,
            directory=tmp_path,
            cancel=Event(),
            retired=retired,
        )
    assert retired.is_set()
    assert not list(tmp_path.glob("*.mp3"))
    assert (tmp_path / "codec-ready-encoded.partial.stderr").is_file()


def test_preparation_cancellation_creates_no_media(
    tmp_path: Path, tools: RecordingCodecTools
) -> None:
    cancel = Event()
    retired = Event()
    cancel.set()
    with pytest.raises(InterruptedError, match="cancelled"):
        preflight_recording_codec(
            tools,
            RecordingOptions(),
            rate=48000,
            channels=2,
            directory=tmp_path,
            cancel=cancel,
            retired=retired,
        )
    assert list(tmp_path.iterdir()) == []
    assert retired.is_set()


def test_preflight_existing_local_file_fails_without_leaving_false_busy_state(
    tmp_path: Path, tools: RecordingCodecTools
) -> None:
    original = tmp_path / "codec-ready-source.f32le"
    original.write_bytes(b"unknown existing bytes")
    retired = Event()
    with pytest.raises(FileExistsError):
        preflight_recording_codec(
            tools,
            RecordingOptions(),
            rate=48000,
            channels=2,
            directory=tmp_path,
            cancel=Event(),
            retired=retired,
        )
    assert retired.is_set()
    assert original.read_bytes() == b"unknown existing bytes"


def test_preflight_clears_prior_retirement_before_local_file_work(
    tmp_path: Path, tools: RecordingCodecTools, monkeypatch: pytest.MonkeyPatch
) -> None:
    retired = Event()
    retired.set()
    original = convert_recording_pcm
    observed: list[bool] = []

    def conversion(
        source: BinaryIO,
        output: BinaryIO,
        *,
        frames: int,
        channels: int,
        encoding: WavEncoding,
        checkpoint: Callable[[], None] | None = None,
    ) -> RecordingPcmStats:
        observed.append(retired.is_set())
        assert not retired.is_set(), "preparation writes PCM after public retirement"
        return original(
            source,
            output,
            frames=frames,
            channels=channels,
            encoding=encoding,
            checkpoint=checkpoint,
        )

    monkeypatch.setattr(recording_codec, "convert_recording_pcm", conversion)
    ready = preflight_recording_codec(
        tools,
        RecordingOptions(),
        rate=48000,
        channels=2,
        directory=tmp_path,
        cancel=Event(),
        retired=retired,
    )
    assert observed == [False]
    assert ready.proof.decoded_frames == 10001
    assert retired.is_set()


def test_raw_input_digest_mismatch_is_not_encoded_or_silently_repaired(
    tmp_path: Path, tools: RecordingCodecTools
) -> None:
    source = tmp_path / "input.pcm"
    source.write_bytes(b"\0" * 80)
    state = source.stat()
    retired = Event()
    request = RecordingCodecRequest(
        encoder=tools.encoder,
        probe=tools.probe,
        source=source,
        output=tmp_path / "encoded.partial",
        decoded=tmp_path / "decoded.pcm",
        source_identity=(state.st_dev, state.st_ino),
        source_payload_sha256="0" * 64,
        options=RecordingOptions(),
        rate=48000,
        channels=2,
        frames=10,
    )
    with pytest.raises(RuntimeError, match="digest changed"):
        run_recording_codec(request, tools, cancel=Event(), retired=retired)
    assert retired.is_set()
    assert not request.output.exists()
    assert source.read_bytes() == b"\0" * 80


def test_source_file_identity_change_rejected_before_process(
    tmp_path: Path, tools: RecordingCodecTools
) -> None:
    source = tmp_path / "input.pcm"
    original = tmp_path / "original.pcm"
    source.write_bytes(b"\0" * 80)
    state = source.stat()
    source.rename(original)
    source.write_bytes(original.read_bytes())
    request = RecordingCodecRequest(
        encoder=tools.encoder,
        probe=tools.probe,
        source=source,
        output=tmp_path / "encoded.partial",
        decoded=tmp_path / "decoded.pcm",
        source_identity=(state.st_dev, state.st_ino),
        source_payload_sha256=hashlib.sha256(b"\0" * 80).hexdigest(),
        options=RecordingOptions(),
        rate=48000,
        channels=2,
        frames=10,
    )
    with pytest.raises(ValueError, match="identity changed"):
        run_recording_codec(request, tools, cancel=Event(), retired=Event())
    assert not request.output.exists()
    assert original.is_file()


def _short_request(tmp_path: Path, tools: RecordingCodecTools) -> RecordingCodecRequest:
    source = tmp_path / "input.pcm"
    source.write_bytes(b"\0" * 80)
    state = source.stat()
    return RecordingCodecRequest(
        encoder=tools.encoder,
        probe=tools.probe,
        source=source,
        output=tmp_path / "encoded.partial",
        decoded=tmp_path / "decoded.pcm",
        source_identity=(state.st_dev, state.st_ino),
        source_payload_sha256=hashlib.sha256(b"\0" * 80).hexdigest(),
        options=RecordingOptions(),
        rate=48000,
        channels=2,
        frames=10,
    )


@pytest.mark.skipif(os.name != "nt", reason="actual Windows file sharing custody")
@pytest.mark.parametrize("change", ["overwrite", "replace"])
def test_verified_encoded_file_cannot_change_before_response_evidence(
    tmp_path: Path, tools: RecordingCodecTools, monkeypatch: pytest.MonkeyPatch, change: str
) -> None:
    request = _short_request(tmp_path, tools)
    original = recording_codec_worker._verify_lossless_wav
    verified: list[bool] = []

    def attempt_change_after_success(value: RecordingCodecRequest) -> None:
        original(value)
        verified.append(True)
        if change == "replace":
            value.output.rename(value.output.with_suffix(".parked"))
        value.output.write_bytes(b"unverified replacement")

    monkeypatch.setattr(
        recording_codec_worker, "_verify_lossless_wav", attempt_change_after_success
    )
    with pytest.raises(PermissionError, match=r"\[(?:WinError (?:5|32)|Errno 13)\]") as raised:
        recording_codec_worker.encode_and_verify_recording(request)
    assert verified == [True]
    assert request.output.read_bytes() != b"unverified replacement"
    assert raised.value.__traceback__ is not None
    with request.output.open("r+b") as released:
        first = released.read(1)
        released.seek(0)
        assert released.write(first) == 1
    request.output.rename(tmp_path / "retired-encoded.partial")
    assert raised.value.__traceback__ is not None


@pytest.mark.skipif(os.name != "nt", reason="actual Windows file sharing retirement")
def test_retained_terminal_exception_does_not_retain_parent_owned_pcm_pin(
    tmp_path: Path, tools: RecordingCodecTools
) -> None:
    request = _short_request(tmp_path, tools).model_copy(update={"source_payload_sha256": "0" * 64})
    retired = Event()
    with pytest.raises(RuntimeError, match="digest changed") as raised:
        run_recording_codec(request, tools, cancel=Event(), retired=retired)
    assert retired.is_set()
    assert raised.value.__traceback__ is not None
    # Keep the real exception/traceback alive while exercising actual ownership.
    with request.source.open("r+b") as released_source:
        assert released_source.write(b"\0") == 1
    request.source.rename(tmp_path / "retired-input.pcm")
    assert raised.value.__traceback__ is not None


@pytest.mark.skipif(os.name != "nt", reason="actual Windows producing-handle custody")
@pytest.mark.parametrize("phase", ["before_encoder", "after_encoder"])
def test_producing_handle_denies_foreign_writes_before_birth_hash(
    tmp_path: Path, tools: RecordingCodecTools, monkeypatch: pytest.MonkeyPatch, phase: str
) -> None:
    request = _short_request(tmp_path, tools)
    original = subprocess.run
    exercised: list[bool] = []

    def guarded_run(
        arguments: list[str],
        *,
        stdin: int,
        stdout: BinaryIO,
        stderr: BinaryIO,
        check: bool,
        creationflags: int,
    ) -> subprocess.CompletedProcess[bytes]:
        target = getattr(stdout, "name", None) == str(request.output)

        def attempt_write() -> None:
            with pytest.raises(PermissionError, match=r"\[(?:WinError (?:5|32)|Errno 13)\]"):
                request.output.write_bytes(b"foreign unverified media")
            exercised.append(True)

        if target and phase == "before_encoder":
            attempt_write()
        result = original(
            arguments,
            stdin=stdin,
            stdout=stdout,
            stderr=stderr,
            check=check,
            creationflags=creationflags,
        )
        if target and phase == "after_encoder":
            attempt_write()
        return result

    monkeypatch.setattr(subprocess, "run", guarded_run)
    result = recording_codec_worker.encode_and_verify_recording(request)
    assert exercised == [True]
    assert result.decoded_frames == request.frames


def test_public_retirement_waits_for_actual_wrapper_input_reads(
    tmp_path: Path, tools: RecordingCodecTools, monkeypatch: pytest.MonkeyPatch
) -> None:
    request = _short_request(tmp_path, tools)
    native_type = SealedProcessingFiles
    retired = Event()
    observations: list[bool] = []

    class ObservedPin:
        def __init__(self, paths: list[str]) -> None:
            self.native = native_type(paths)

        def verify(self) -> None:
            observations.append(retired.is_set())
            assert not retired.is_set(), "wrapper still reads/pins input after public retirement"
            self.native.verify()

    monkeypatch.setattr(recording_codec, "SealedProcessingFiles", ObservedPin)
    result = run_recording_codec(request, tools, cancel=Event(), retired=retired)
    assert result.decoded_frames == request.frames
    assert observations == [False, False]
    assert retired.is_set()


@pytest.mark.skipif(os.name != "nt", reason="actual Windows verification sidecar custody")
@pytest.mark.parametrize(
    ("options", "stage"),
    [
        (RecordingOptions(), "streams.json"),
        (RecordingOptions(format="mp3"), "packets"),
        (RecordingOptions(format="flac", flac_bit_depth=16), "decoded.pcm"),
    ],
)
def test_verification_sidecars_retain_original_bytes_until_consumed(
    tmp_path: Path,
    tools: RecordingCodecTools,
    monkeypatch: pytest.MonkeyPatch,
    options: RecordingOptions,
    stage: str,
) -> None:
    request = _short_request(tmp_path, tools)
    width = 4 if options.format == "wav" else 2 if options.format == "flac" else 3
    payload = b"\0" * (10001 * 2 * width)
    request.source.write_bytes(payload)
    request = request.model_copy(
        update={
            "options": options,
            "frames": 10001,
            "source_payload_sha256": hashlib.sha256(payload).hexdigest(),
        }
    )
    original = recording_codec_worker._command_to_file
    protected: list[bool] = []
    retained_errors: list[pytest.ExceptionInfo[PermissionError]] = []
    sidecars: list[Path] = []

    @contextmanager
    def attempt_sidecar_change(
        arguments: list[str], destination: Path
    ) -> Iterator[recording_codec_worker._CommandFile]:
        with original(arguments, destination) as evidence:
            if destination.name.endswith(stage):
                with pytest.raises(
                    PermissionError, match=r"\[(?:WinError (?:5|32)|Errno 13)\]"
                ) as raised:
                    destination.write_bytes(b"unverified sidecar")
                retained_errors.append(raised)
                sidecars.append(destination)
                protected.append(True)
            yield evidence

    monkeypatch.setattr(recording_codec_worker, "_command_to_file", attempt_sidecar_change)
    proof = recording_codec_worker.encode_and_verify_recording(request)
    assert protected == [True]
    assert proof.decoded_frames == 10001
    assert retained_errors[0].value.__traceback__ is not None
    with sidecars[0].open("r+b") as released:
        first = released.read(1)
        released.seek(0)
        assert released.write(first) == 1
    sidecars[0].rename(tmp_path / "retired-sidecar")
    assert retained_errors[0].value.__traceback__ is not None
