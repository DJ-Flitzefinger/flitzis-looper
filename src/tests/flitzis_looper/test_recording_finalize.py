"""Finalizer failure guards and pure warnings; no simulated successful Native take."""

import os
import sys
from contextlib import contextmanager
from pathlib import Path
from threading import Event
from typing import TYPE_CHECKING, cast
from uuid import uuid4

import pytest

from flitzis_looper import recording_finalize, recording_paths
from flitzis_looper.recording_codec import (
    RecordingCodecReadiness,
    RecordingCodecResult,
    RecordingCodecTools,
)
from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.recording_paths import RecordingPaths, create_recording_namespace
from flitzis_looper.recording_pcm import RecordingPcmStats
from flitzis_looper_audio import SealedProcessingFiles

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper_audio import AudioEngine, MixCaptureTicket

_OWNER = (
    "# Owned finalizer failure tests\n"
    "Created only by src/tests/flitzis_looper/test_recording_finalize.py.\n"
    "Synthetic malformed media and metadata are retained for Root review/cleanup.\n"
    "No Human audio, settings, model assets or installations belong here.\n"
)


@pytest.fixture
def owned_case() -> Path:
    block = (
        Path(__file__).resolve().parents[4]
        / "test-artifacts"
        / "recording-format-finalize-20261020"
    )
    block.mkdir(parents=True, exist_ok=True)
    owner = block / "OWNER.md"
    try:
        with owner.open("x", encoding="utf-8", newline="\n") as handle:
            handle.write(_OWNER)
    except FileExistsError:
        assert owner.read_text(encoding="utf-8") == _OWNER
    case = block / uuid4().hex
    case.mkdir()
    return case


class _FailureTicket:
    """Only identifies negative guards; never accepted by a real Native engine."""

    def session_id(self) -> int:
        return 17


class _FailureEngine:
    def __init__(self, status: dict[str, object] | None = None) -> None:
        self.status = status
        self.calls = 0

    def mix_capture_status(self, _ticket: object) -> dict[str, object]:
        self.calls += 1
        if self.status is None:
            message = "foreign or retired mix capture ticket"
            raise ValueError(message)
        return self.status.copy()


def _negative_readiness(case: Path, options: RecordingOptions) -> RecordingCodecReadiness:
    """Real file witness, synthetic codec evidence used only before rejection.

    This is deliberately not a codec preflight and cannot support a successful
    take assertion. Actual successful Native/codec composition has its own test.
    """
    tool = case / "synthetic-never-executed-tool.bin"
    with tool.open("xb") as output:
        output.write(b"not executable; negative guard tests never spawn a process")
    files = SealedProcessingFiles([str(tool)])
    tools = RecordingCodecTools(tool, tool, files)
    proof = RecordingCodecResult(
        output_identity=(1, 1),
        output_bytes=1,
        output_sha256="0" * 64,
        decoded_frames=1,
        decoded_payload_sha256="0" * 64,
    )
    return RecordingCodecReadiness(tools, options, 48000, 2, proof)


def _invoke(
    engine: _FailureEngine,
    paths: RecordingPaths,
    ready: RecordingCodecReadiness,
    *,
    cancel: Event,
    retired: Event,
    identity: tuple[int, int] = (1, 1),
) -> None:
    recording_finalize.finalize_recording_take(
        cast("AudioEngine", engine),
        cast("MixCaptureTicket", _FailureTicket()),
        paths,
        ready,
        spool_identity=identity,
        cancel=cancel,
        retired=retired,
    )


def test_cancel_before_finalization_never_reads_native_or_creates_stages(owned_case: Path) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    ready = _negative_readiness(owned_case, paths.options)
    engine = _FailureEngine()
    cancel, retired = Event(), Event()
    cancel.set()

    with pytest.raises(InterruptedError, match="finalization cancelled"):
        _invoke(engine, paths, ready, cancel=cancel, retired=retired)

    assert retired.is_set()
    assert engine.calls == 0
    assert set(paths.directory.iterdir()) == {paths.metadata_path}
    assert not paths.final_path.exists()


def test_changed_options_cannot_reuse_previous_codec_readiness(owned_case: Path) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions(format="wav"))
    ready = _negative_readiness(owned_case, RecordingOptions(format="flac"))
    engine = _FailureEngine()
    retired = Event()

    with pytest.raises(ValueError, match="options differ"):
        _invoke(engine, paths, ready, cancel=Event(), retired=retired)

    assert retired.is_set()
    assert engine.calls == 0
    assert set(paths.directory.iterdir()) == {paths.metadata_path}


def test_original_ticket_rejection_remains_terminal_and_releases_namespace(
    owned_case: Path,
) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    ready = _negative_readiness(owned_case, paths.options)
    engine = _FailureEngine()
    retired = Event()

    with pytest.raises(ValueError, match="foreign or retired") as caught:
        _invoke(engine, paths, ready, cancel=Event(), retired=retired)

    # Keep the exception/traceback alive while exercising the actual released
    # owner file. Exception retention must not retain namespace custody.
    assert caught.value.__traceback__ is not None
    with paths.metadata_path.open("ab") as owner:
        owner.write(b"")
    assert retired.is_set()
    assert engine.calls == 1
    assert set(paths.directory.iterdir()) == {paths.metadata_path}
    assert not paths.final_path.exists()


def test_live_unreleased_rt_work_refuses_before_opening_a_spool(owned_case: Path) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    ready = _negative_readiness(owned_case, paths.options)
    engine = _FailureEngine({
        "rt_sealed": True,
        "rt_released": False,
        "worker_closed": True,
        "worker_joined": True,
    })
    retired = Event()

    with pytest.raises(ValueError, match="not quiescent: rt_released"):
        _invoke(engine, paths, ready, cancel=Event(), retired=retired)

    assert retired.is_set()
    assert not paths.spool_path.exists()
    assert set(paths.directory.iterdir()) == {paths.metadata_path}


@pytest.mark.skipif(sys.platform != "win32", reason="actual Windows namespace custody release")
def test_precommit_pin_release_error_never_fabricates_retirement(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    ready = _negative_readiness(owned_case, paths.options)
    engine = _FailureEngine()
    retired = Event()
    original_pin = recording_paths._windows_pin
    original_close = os.close
    targets: set[int] = set()

    @contextmanager
    def observe_owner_pin(path: Path, *, directory: bool) -> Iterator[int]:
        with original_pin(path, directory=directory) as descriptor:
            if path == paths.metadata_path:
                targets.add(descriptor)
            yield descriptor

    def report_close_error(descriptor: int) -> None:
        original_close(descriptor)
        if descriptor in targets:
            targets.remove(descriptor)
            message = "injected actual namespace close error before commit"
            raise OSError(message)

    monkeypatch.setattr(recording_paths, "_windows_pin", observe_owner_pin)
    monkeypatch.setattr(os, "close", report_close_error)
    with pytest.raises(
        recording_paths.RecordingCustodyReleaseError, match="close error before commit"
    ) as caught:
        _invoke(engine, paths, ready, cancel=Event(), retired=retired)
    assert caught.value.path == paths.metadata_path
    assert caught.value.retirement_uncertain
    assert isinstance(caught.value.__cause__, OSError)
    assert not retired.is_set()
    assert not paths.final_path.exists()
    assert not paths.pcm_path.exists()
    assert set(paths.directory.iterdir()) == {paths.metadata_path}


@pytest.mark.skipif(sys.platform != "win32", reason="actual Windows deny-write Native file pin")
def test_malformed_real_checkpoint_releases_local_native_pin_even_with_retained_traceback(
    owned_case: Path,
) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    ready = _negative_readiness(owned_case, paths.options)
    with paths.spool_path.open("xb") as source:
        source.write(bytes(512))
    state = paths.spool_path.stat()
    identity = state.st_dev, state.st_ino
    engine = _FailureEngine({
        "rt_sealed": True,
        "rt_released": True,
        "worker_closed": True,
        "worker_joined": True,
    })
    retired = Event()

    with pytest.raises(RuntimeError, match="spool inspection failed") as caught:
        _invoke(engine, paths, ready, cancel=Event(), retired=retired, identity=identity)

    # No complete Native status is fabricated. The actual inspector rejects the
    # actual corrupt file before full validation, conversion or any codec child.
    assert caught.value.__traceback__ is not None
    assert retired.is_set()
    with paths.spool_path.open("ab") as source:
        source.write(b"released-local-pin")
    retained = paths.directory / "malformed-spool.retained"
    paths.spool_path.rename(retained)
    assert retained.read_bytes().endswith(b"released-local-pin")
    assert not paths.pcm_path.exists()
    assert not paths.encoded_path.exists()
    assert not paths.final_path.exists()


@pytest.mark.parametrize(
    "options",
    [
        RecordingOptions(format="wav"),
        RecordingOptions(format="wav", wav_encoding="pcm16"),
        RecordingOptions(format="flac"),
        RecordingOptions(format="mp3"),
    ],
)
def test_overrange_warning_reports_original_counts_and_selected_conversion(
    owned_case: Path, options: RecordingOptions
) -> None:
    paths = create_recording_namespace(owned_case / "record", options)
    pcm = RecordingPcmStats(8, 2, 2.5, 3, 2, "0" * 64, "1" * 64)

    warning = recording_finalize._overrange_warning(paths, pcm)

    assert warning is not None
    assert "3 samples in 2 frames" in warning
    assert "peak 2.5" in warning
    assert "without normalization" in warning
    if options.format == "wav" and options.wav_encoding == "float32":
        assert "preserves these values" in warning
    else:
        assert "saturated these values" in warning


def test_exact_fullscale_endpoint_is_not_reported_as_original_overrange(owned_case: Path) -> None:
    paths = create_recording_namespace(
        owned_case / "record", RecordingOptions(format="wav", wav_encoding="pcm24")
    )
    pcm = RecordingPcmStats(8, 2, 1.0, 0, 0, "0" * 64, "1" * 64)
    assert recording_finalize._overrange_warning(paths, pcm) is None
