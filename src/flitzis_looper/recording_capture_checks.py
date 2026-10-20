"""Strict agreement checks for an already-owned live capture and its checkpoint.

This module opens no paths and grants no Native or publication authority. Its
caller must retain the original opaque engine ticket, pin the actual spool, and
verify the complete physical extent and finite PCM independently. A checksum-
valid sealed candidate alone cannot replace the live owner's terminal result.
"""

import re
from dataclasses import dataclass
from pathlib import Path

_U64_MAX = (1 << 64) - 1
_PAYLOAD_SHA256 = re.compile(r"[0-9a-f]{64}\Z")


class RecordingCaptureValidationError(ValueError):
    """The supplied live status and checkpoint do not describe one finished take."""


@dataclass(frozen=True, kw_only=True)
class RecordingCaptureExtent:
    """An agreed nonempty extent, without file, ticket or publication authority."""

    first_frame: int
    end_frame: int
    frames: int
    rate: int
    channels: int
    session_id: int
    payload_sha256: str


def _required(record: dict[str, object], name: str, key: str) -> object:
    if key not in record:
        message = f"{name}.{key} is missing"
        raise RecordingCaptureValidationError(message)
    return record[key]


def _unsigned(value: object, name: str, *, positive: bool = False) -> int:
    minimum = 1 if positive else 0
    if type(value) is not int or not minimum <= value <= _U64_MAX:
        message = f"{name} must be a strict {'positive' if positive else 'nonnegative'} u64"
        raise RecordingCaptureValidationError(message)
    return value


def _number(record: dict[str, object], name: str, key: str) -> int:
    return _unsigned(_required(record, name, key), f"{name}.{key}")


def _flag(record: dict[str, object], name: str, key: str, *, expected: bool) -> None:
    if _required(record, name, key) is not expected:
        message = f"{name}.{key} must be exactly {expected}"
        raise RecordingCaptureValidationError(message)


def _absent_failure(record: dict[str, object], name: str, key: str) -> None:
    if _required(record, name, key) is not None:
        message = f"{name}.{key} must be None"
        raise RecordingCaptureValidationError(message)


def _same_number(record: dict[str, object], name: str, key: str, expected: int) -> None:
    if _number(record, name, key) != expected:
        message = f"{name}.{key} does not match the original capture extent or identity"
        raise RecordingCaptureValidationError(message)


def _validate_terminal_status(status: dict[str, object]) -> None:
    for key in (
        "terminal_decided",
        "rt_sealed",
        "rt_released",
        "worker_closed",
        "worker_joined",
        "started",
        "successful_spool",
    ):
        _flag(status, "status", key, expected=True)
    _flag(status, "status", "empty", expected=False)
    # Native readiness is an admission state; terminal_decided/worker_closed
    # intentionally revoke it. Retain Native's actual terminal projection.
    _flag(status, "status", "writer_ready", expected=False)
    for key in ("failure", "failure_code", "error_detail"):
        _absent_failure(status, "status", key)
    _same_number(status, "status", "buffered_frames", 0)


def _payload_sha256(status: dict[str, object]) -> str:
    digest = _required(status, "status", "payload_sha256")
    if type(digest) is not str or _PAYLOAD_SHA256.fullmatch(digest) is None:
        message = "status.payload_sha256 must be the original writer's lowercase SHA256"
        raise RecordingCaptureValidationError(message)
    return digest


def _validate_checkpoint(
    checkpoint: dict[str, object], *, first_frame: int, end_frame: int, frames: int
) -> None:
    if _required(checkpoint, "checkpoint", "state") != "sealed_candidate":
        message = "checkpoint.state must be sealed_candidate"
        raise RecordingCaptureValidationError(message)
    for key in ("failure", "failure_code"):
        _absent_failure(checkpoint, "checkpoint", key)
    _same_number(checkpoint, "checkpoint", "first_frame", first_frame)
    _same_number(checkpoint, "checkpoint", "declared_end_frame", end_frame)
    for key in (
        "declared_submitted_frames",
        "declared_written_frames",
        "physical_whole_frames",
    ):
        _same_number(checkpoint, "checkpoint", key, frames)
    _same_number(checkpoint, "checkpoint", "partial_frame_bytes", 0)
    _flag(checkpoint, "checkpoint", "requires_verification", expected=True)
    _flag(checkpoint, "checkpoint", "publication_ready", expected=False)


def validate_recording_capture(
    status: dict[str, object],
    checkpoint: dict[str, object],
    *,
    session_id: int,
    spool_path: Path,
    rate: int,
    channels: int,
) -> RecordingCaptureExtent:
    """Require complete, strict agreement; ignore unrelated diagnostic fields.

    ``status`` must have been obtained with the caller's original live ticket.
    This pure function cannot establish that provenance or pin either dictionary
    to a file. It deliberately returns no reusable Native permission.
    """
    if not isinstance(status, dict) or not isinstance(checkpoint, dict):
        message = "capture status and checkpoint must be dictionaries"
        raise RecordingCaptureValidationError(message)
    session_id = _unsigned(session_id, "session_id", positive=True)
    rate = _unsigned(rate, "rate", positive=True)
    channels = _unsigned(channels, "channels", positive=True)
    if not isinstance(spool_path, Path):
        message = "spool_path must be the original Path"
        raise RecordingCaptureValidationError(message)

    for name, record in (("status", status), ("checkpoint", checkpoint)):
        _same_number(record, name, "session_id", session_id)
        _same_number(record, name, "sample_rate_hz", rate)
        _same_number(record, name, "channels", channels)
    path = _required(status, "status", "spool_path")
    if type(path) is not str or path != str(spool_path):
        message = "status.spool_path does not match the original capture path"
        raise RecordingCaptureValidationError(message)

    _validate_terminal_status(status)

    first_frame = _number(status, "status", "first_frame")
    end_frame = _number(status, "status", "end_frame")
    if end_frame <= first_frame:
        message = "status.end_frame must follow first_frame for a nonempty take"
        raise RecordingCaptureValidationError(message)
    frames = end_frame - first_frame
    for key in ("captured_end", "submitted_end", "written_end"):
        _same_number(status, "status", key, end_frame)
    for key in ("submitted_frames", "written_frames"):
        _same_number(status, "status", key, frames)
    blocks = _number(status, "status", "submitted_blocks")
    _same_number(status, "status", "written_blocks", blocks)

    _validate_checkpoint(checkpoint, first_frame=first_frame, end_frame=end_frame, frames=frames)
    return RecordingCaptureExtent(
        first_frame=first_frame,
        end_frame=end_frame,
        frames=frames,
        rate=rate,
        channels=channels,
        session_id=session_id,
        payload_sha256=_payload_sha256(status),
    )
