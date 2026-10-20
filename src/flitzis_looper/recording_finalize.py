"""Synchronous off-realtime finalization of one original owned Native capture.

The caller retains the sole higher-level Finalizing owner, its original ticket,
the writer-ready spool identity, and codec readiness for this entire call. This
module never prepares/starts/stops capture, changes playback, deletes evidence or
grants recovery metadata Native permission. No other owner may prepare a newer
engine capture between the last live ticket check and filesystem publication.
"""

import json
import os
import stat
from dataclasses import asdict, dataclass
from threading import Event
from typing import TYPE_CHECKING

from flitzis_looper.recording_capture_checks import validate_recording_capture
from flitzis_looper.recording_codec import (
    RecordingCodecRequest,
    run_recording_codec,
)
from flitzis_looper.recording_codec_commands import recording_pcm_encoding
from flitzis_looper.recording_paths import (
    RecordingCustodyReleaseError,
    RecordingPublicationCommittedError,
    hold_recording_namespace,
    publish_recording_no_replace,
)
from flitzis_looper.recording_pcm import convert_recording_pcm
from flitzis_looper_audio import SealedProcessingFiles, inspect_mix_capture_spool

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.recording_capture_checks import RecordingCaptureExtent
    from flitzis_looper.recording_codec import RecordingCodecReadiness, RecordingCodecResult
    from flitzis_looper.recording_paths import RecordingPaths
    from flitzis_looper.recording_pcm import RecordingPcmStats
    from flitzis_looper_audio import AudioEngine, MixCaptureTicket

_SPOOL_HEADER_BYTES = 256
_REPARSE_POINT = 0x400
_PCM_WIDTH = {"float32": 4, "pcm16": 2, "pcm24": 3}


@dataclass(frozen=True, kw_only=True)
class RecordingCompletion:
    """A verified committed file; actual local retirement is observed separately."""

    path: Path
    extent: RecordingCaptureExtent
    pcm: RecordingPcmStats
    codec: RecordingCodecResult
    warning: str | None


class RecordingFinalizationCommittedError(OSError):
    """Preserve a verified committed take when subsequent custody release fails."""

    def __init__(self, completion: RecordingCompletion, cause: OSError) -> None:
        self.completion = completion
        self.retirement_uncertain = True
        super().__init__(
            f"recording published at {completion.path}; custody release failed: {cause}"
        )


def _checkpoint(cancel: Event) -> None:
    if cancel.is_set():
        message = "recording finalization cancelled"
        raise InterruptedError(message)


def _strict_identity(identity: tuple[int, int]) -> None:
    if (
        type(identity) is not tuple
        or len(identity) != 2
        or any(type(value) is not int or value < 0 for value in identity)
        or identity[1] == 0
    ):
        message = "recording spool requires its original strict file identity"
        raise ValueError(message)


def _check_file_state(
    state: os.stat_result, identity: tuple[int, int], *, size: int | None = None
) -> None:
    if (
        not stat.S_ISREG(state.st_mode)
        or getattr(state, "st_file_attributes", 0) & _REPARSE_POINT
        or (state.st_dev, state.st_ino) != identity
        or (size is not None and state.st_size != size)
    ):
        message = "recording file identity, ordinary-file type or exact extent changed"
        raise ValueError(message)


def _require_closed_owner(status: dict[str, object]) -> None:
    # Do not attempt a deny-write Native pin while the actual writer or RT work
    # can still own the spool. Full terminal/identity checks follow while pinned.
    for key in ("rt_sealed", "rt_released", "worker_closed", "worker_joined"):
        if status.get(key) is not True:
            message = f"recording original owner is not quiescent: {key}"
            raise ValueError(message)


def _live_extent(
    engine: AudioEngine,
    ticket: MixCaptureTicket,
    paths: RecordingPaths,
    ready: RecordingCodecReadiness,
    session_id: int,
) -> RecordingCaptureExtent:
    checkpoint = inspect_mix_capture_spool(str(paths.spool_path))
    # The actual opaque-ticket lookup is intentionally after header inspection;
    # a newer prepare must not leave a stale earlier status as final permission.
    status = engine.mix_capture_status(ticket)
    return validate_recording_capture(
        status,
        checkpoint,
        session_id=session_id,
        spool_path=paths.spool_path,
        rate=ready.rate,
        channels=ready.channels,
    )


def _convert_spool(
    paths: RecordingPaths,
    extent: RecordingCaptureExtent,
    spool_identity: tuple[int, int],
    cancel: Event,
) -> tuple[RecordingPcmStats, tuple[int, int]]:
    expected_size = _SPOOL_HEADER_BYTES + extent.frames * extent.channels * 4
    encoding = recording_pcm_encoding(paths.options)

    def checkpoint() -> None:
        _checkpoint(cancel)

    with paths.spool_path.open("rb") as source, paths.pcm_path.open("xb") as output:
        _check_file_state(os.fstat(source.fileno()), spool_identity, size=expected_size)
        output_state = os.fstat(output.fileno())
        pcm_identity = output_state.st_dev, output_state.st_ino
        _strict_identity(pcm_identity)
        if pcm_identity == spool_identity:
            message = "recording PCM output aliases its original spool"
            raise ValueError(message)
        source.seek(_SPOOL_HEADER_BYTES)
        measurements = convert_recording_pcm(
            source,
            output,
            frames=extent.frames,
            channels=extent.channels,
            encoding=encoding,
            checkpoint=checkpoint,
        )
        if measurements.source_payload_sha256 != extent.payload_sha256:
            message = "recording payload digest differs from the original Native writer"
            raise ValueError(message)
        output.flush()
        os.fsync(output.fileno())
        _check_file_state(
            os.fstat(output.fileno()),
            pcm_identity,
            size=extent.frames * extent.channels * _PCM_WIDTH[encoding],
        )
    return measurements, pcm_identity


def _overrange_warning(paths: RecordingPaths, pcm: RecordingPcmStats) -> str | None:
    if pcm.source_overrange_samples == 0:
        return None
    extent = (
        f"{pcm.source_overrange_samples} samples in {pcm.source_overrange_frames} frames "
        f"exceeded full scale (peak {pcm.peak_abs:.9g})."
    )
    if recording_pcm_encoding(paths.options) == "float32":
        return f"{extent} Float32 WAV preserves these values without normalization."
    return f"{extent} Integer/lossy conversion saturated these values without normalization."


def _write_verified_receipt(
    paths: RecordingPaths,
    completion: RecordingCompletion,
    spool_identity: tuple[int, int],
    pcm_identity: tuple[int, int],
) -> None:
    content = {
        "schema": "flitzi-recording-verified-candidate-v1",
        "attempt_id": paths.attempt_id,
        "options": paths.options.model_dump(mode="json"),
        "extent": asdict(completion.extent),
        "pcm": asdict(completion.pcm),
        "codec": completion.codec.model_dump(mode="json"),
        "spool_identity": spool_identity,
        "pcm_identity": pcm_identity,
        "final": completion.path.name,
        "warning": completion.warning,
        "publication_ready": False,
        "requires_original_live_ticket": True,
    }
    encoded = (
        json.dumps(content, sort_keys=True, separators=(",", ":"), allow_nan=False) + "\n"
    ).encode("utf-8")
    with (paths.directory / "verified.json").open("xb") as receipt:
        receipt.write(encoded)
        receipt.flush()
        os.fsync(receipt.fileno())


def _check_encoded_evidence(
    paths: RecordingPaths,
    codec: RecordingCodecResult,
    witnesses: list[tuple[str, str, int]],
) -> None:
    _check_file_state(
        paths.encoded_path.stat(follow_symlinks=False),
        codec.output_identity,
        size=codec.output_bytes,
    )
    if len(witnesses) != 1 or witnesses[0][1:] != (
        codec.output_sha256,
        codec.output_bytes,
    ):
        message = "recording encoded file does not match the child's complete verified witness"
        raise ValueError(message)


@dataclass(slots=True)
class _FinalizationWork:
    codec_retired: Event
    codec_requested: bool = False
    published: RecordingCompletion | None = None


def _finalize_owned_take(
    engine: AudioEngine,
    ticket: MixCaptureTicket,
    paths: RecordingPaths,
    ready: RecordingCodecReadiness,
    spool_identity: tuple[int, int],
    cancel: Event,
    work: _FinalizationWork,
) -> RecordingCompletion:
    _checkpoint(cancel)
    _strict_identity(spool_identity)
    with hold_recording_namespace(paths):
        if paths.options != ready.options:
            message = "recording options differ from the original codec readiness"
            raise ValueError(message)
        ready.tools.files.verify()
        session_id = ticket.session_id()
        _require_closed_owner(engine.mix_capture_status(ticket))
        _check_file_state(paths.spool_path.stat(follow_symlinks=False), spool_identity)
        encoded_guard: SealedProcessingFiles | None = None
        spool_guard = SealedProcessingFiles([str(paths.spool_path)])
        try:
            extent = _live_extent(engine, ticket, paths, ready, session_id)
            pcm, pcm_identity = _convert_spool(paths, extent, spool_identity, cancel)
            request = RecordingCodecRequest(
                encoder=ready.tools.encoder,
                probe=ready.tools.probe,
                source=paths.pcm_path,
                output=paths.encoded_path,
                decoded=paths.decoded_path,
                source_identity=pcm_identity,
                source_payload_sha256=pcm.output_payload_sha256,
                options=paths.options,
                rate=extent.rate,
                channels=extent.channels,
                frames=extent.frames,
            )
            _checkpoint(cancel)
            work.codec_requested = True
            codec = run_recording_codec(
                request, ready.tools, cancel=cancel, retired=work.codec_retired
            )
            # The supervisor normally returns only after this is true. Keep
            # actual retirement as the lifetime boundary on every exit.
            work.codec_retired.wait()
            _checkpoint(cancel)
            encoded_guard = SealedProcessingFiles([str(paths.encoded_path)])
            _check_encoded_evidence(paths, codec, encoded_guard.witnesses())
            completion = RecordingCompletion(
                path=paths.final_path,
                extent=extent,
                pcm=pcm,
                codec=codec,
                warning=_overrange_warning(paths, pcm),
            )
            _write_verified_receipt(paths, completion, spool_identity, pcm_identity)
            ready.tools.files.verify()
            spool_guard.verify()
            encoded_guard.verify()
            if _live_extent(engine, ticket, paths, ready, session_id) != extent:
                message = "recording original live extent changed before publication"
                raise ValueError(message)
            _checkpoint(cancel)
            try:
                publish_recording_no_replace(paths, codec.output_identity)
            except RecordingPublicationCommittedError:
                work.published = completion
                raise
            work.published = completion
            return completion
        finally:
            # A retained exception/traceback must not retain a Native pin.
            # Nor may namespace/file custody end under a surviving child.
            try:
                if work.codec_requested:
                    work.codec_retired.wait()
            finally:
                del encoded_guard, spool_guard


def finalize_recording_take(
    engine: AudioEngine,
    ticket: MixCaptureTicket,
    paths: RecordingPaths,
    ready: RecordingCodecReadiness,
    *,
    spool_identity: tuple[int, int],
    cancel: Event,
    retired: Event,
) -> RecordingCompletion:
    """Finalize off realtime, preserving pre-commit evidence and post-commit outcome.

    The caller retains sole Finalizing ownership and readiness image pins.
    A RecordingFinalizationCommittedError preserves its verified completion while
    retirement remains uncertain; the file must not be treated as unpublished.
    Native hashing and owned child retirement retain their synchronous lifetimes.
    """
    retired.clear()
    work = _FinalizationWork(Event())
    retirement_uncertain = False
    try:
        return _finalize_owned_take(engine, ticket, paths, ready, spool_identity, cancel, work)
    except OSError as error:
        retirement_uncertain = isinstance(error, RecordingCustodyReleaseError)
        if work.published is not None:
            retirement_uncertain = True
            raise RecordingFinalizationCommittedError(work.published, error) from error
        raise
    finally:
        if not retirement_uncertain and (not work.codec_requested or work.codec_retired.is_set()):
            retired.set()
