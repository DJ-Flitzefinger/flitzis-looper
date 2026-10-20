"""Synthetic metadata agreement tests, never positive Native integration proof."""

from dataclasses import FrozenInstanceError
from pathlib import Path

import pytest

from flitzis_looper.recording_capture_checks import (
    RecordingCaptureExtent,
    RecordingCaptureValidationError,
    validate_recording_capture,
)

_SESSION = 17
_RATE = 48000
_CHANNELS = 2
_FIRST = 4096
_END = 8192
_SPOOL = Path(__file__).parent / "synthetic-metadata-only.float.capture"


@pytest.fixture
def records() -> tuple[dict[str, object], dict[str, object]]:
    status: dict[str, object] = {
        "session_id": _SESSION,
        "spool_path": str(_SPOOL),
        "sample_rate_hz": _RATE,
        "channels": _CHANNELS,
        "writer_ready": False,
        "started": True,
        "rt_sealed": True,
        "rt_released": True,
        "worker_closed": True,
        "worker_joined": True,
        "terminal_decided": True,
        "failure": None,
        "failure_code": None,
        "error_detail": None,
        "first_frame": _FIRST,
        "captured_end": _END,
        "buffered_frames": 0,
        "submitted_end": _END,
        "written_end": _END,
        "end_frame": _END,
        "submitted_frames": _END - _FIRST,
        "written_frames": _END - _FIRST,
        "submitted_blocks": 2,
        "written_blocks": 2,
        "empty": False,
        "successful_spool": True,
        "payload_sha256": "e" * 64,
    }
    checkpoint: dict[str, object] = {
        "session_id": _SESSION,
        "sample_rate_hz": _RATE,
        "channels": _CHANNELS,
        "state": "sealed_candidate",
        "failure": None,
        "failure_code": None,
        "first_frame": _FIRST,
        "declared_submitted_frames": _END - _FIRST,
        "declared_written_frames": _END - _FIRST,
        "declared_end_frame": _END,
        "physical_whole_frames": _END - _FIRST,
        "partial_frame_bytes": 0,
        "requires_verification": True,
        "publication_ready": False,
    }
    return status, checkpoint


def _validate(
    records: tuple[dict[str, object], dict[str, object]],
) -> RecordingCaptureExtent:
    return validate_recording_capture(
        *records,
        session_id=_SESSION,
        spool_path=_SPOOL,
        rate=_RATE,
        channels=_CHANNELS,
    )


def test_agreed_metadata_returns_only_a_frozen_extent_without_mutating_inputs(
    records: tuple[dict[str, object], dict[str, object]],
) -> None:
    status, checkpoint = records
    original_status = status.copy()
    original_checkpoint = checkpoint.copy()

    extent = _validate(records)

    assert extent == RecordingCaptureExtent(
        first_frame=_FIRST,
        end_frame=_END,
        frames=_END - _FIRST,
        rate=_RATE,
        channels=_CHANNELS,
        session_id=_SESSION,
        payload_sha256="e" * 64,
    )
    assert status == original_status
    assert checkpoint == original_checkpoint
    with pytest.raises(FrozenInstanceError, match="cannot assign"):
        extent.frames = 0  # type: ignore[misc]


def test_unknown_diagnostics_add_no_extra_prerequisites(
    records: tuple[dict[str, object], dict[str, object]],
) -> None:
    status, checkpoint = records
    status.update(start_queued=False, stop_target=None, future_diagnostic=object())
    checkpoint.update(revision=9, future_diagnostic=object())
    assert _validate(records).frames == _END - _FIRST


@pytest.mark.parametrize("value", [None, True, 0, "", "e" * 63, "E" * 64, "z" * 64])
def test_terminal_capture_requires_original_writer_payload_witness(
    records: tuple[dict[str, object], dict[str, object]], value: object
) -> None:
    records[0]["payload_sha256"] = value
    with pytest.raises(RecordingCaptureValidationError, match="payload_sha256"):
        _validate(records)


@pytest.mark.parametrize(
    "key",
    [
        "terminal_decided",
        "rt_sealed",
        "rt_released",
        "worker_closed",
        "worker_joined",
        "started",
        "successful_spool",
    ],
)
@pytest.mark.parametrize("value", [False, 1])
def test_each_terminal_condition_must_be_a_real_true_boolean(
    records: tuple[dict[str, object], dict[str, object]], key: str, value: object
) -> None:
    records[0][key] = value
    with pytest.raises(RecordingCaptureValidationError, match=rf"status\.{key}"):
        _validate(records)


def test_successful_spool_alone_cannot_authorize_a_transient_or_sparse_snapshot(
    records: tuple[dict[str, object], dict[str, object]],
) -> None:
    del records[0]["rt_released"]
    with pytest.raises(RecordingCaptureValidationError, match=r"status\.rt_released is missing"):
        _validate(records)


@pytest.mark.parametrize("value", [True, 0, None])
def test_terminal_writer_readiness_is_revoked_exactly_as_native_projects_it(
    records: tuple[dict[str, object], dict[str, object]], value: object
) -> None:
    records[0]["writer_ready"] = value
    with pytest.raises(RecordingCaptureValidationError, match=r"status\.writer_ready"):
        _validate(records)


@pytest.mark.parametrize(
    ("key", "value"),
    [
        ("failure", "stream"),
        ("failure", "writer_io"),
        ("failure_code", 9),
        ("error_detail", "last checkpoint I/O failed after a valid candidate was persisted"),
        ("empty", True),
    ],
)
def test_a_valid_candidate_never_overrides_live_failure_or_empty_state(
    records: tuple[dict[str, object], dict[str, object]], key: str, value: object
) -> None:
    records[0][key] = value
    assert records[1]["state"] == "sealed_candidate"
    with pytest.raises(RecordingCaptureValidationError, match=rf"status\.{key}"):
        _validate(records)


@pytest.mark.parametrize(
    ("key", "value"),
    [
        ("session_id", 17.0),
        ("sample_rate_hz", True),
        ("channels", 2.0),
        ("first_frame", True),
        ("end_frame", float(_END)),
        ("submitted_frames", "4096"),
        ("written_blocks", True),
        ("buffered_frames", False),
        ("written_blocks", -1),
        ("first_frame", -1),
        ("end_frame", 1 << 64),
    ],
)
def test_native_numeric_fields_do_not_coerce_bool_float_string_or_out_of_range_values(
    records: tuple[dict[str, object], dict[str, object]], key: str, value: object
) -> None:
    records[0][key] = value
    with pytest.raises(RecordingCaptureValidationError, match=rf"status\.{key}"):
        _validate(records)


@pytest.mark.parametrize(
    ("key", "value"),
    [
        ("captured_end", _END - 1),
        ("submitted_end", _END + 1),
        ("written_end", _END - 1),
        ("submitted_frames", _END - _FIRST - 1),
        ("written_frames", _END - _FIRST + 1),
        ("buffered_frames", 1),
        ("written_blocks", 1),
        ("end_frame", _FIRST),
    ],
)
def test_captured_submitted_written_and_closed_extents_must_all_agree(
    records: tuple[dict[str, object], dict[str, object]], key: str, value: object
) -> None:
    records[0][key] = value
    with pytest.raises(RecordingCaptureValidationError, match=rf"status\.{key}"):
        _validate(records)


@pytest.mark.parametrize("mismatch", ["session_id", "spool_path", "sample_rate_hz", "channels"])
def test_expected_original_identity_and_geometry_are_required(
    records: tuple[dict[str, object], dict[str, object]], mismatch: str
) -> None:
    with pytest.raises(RecordingCaptureValidationError, match=rf"status\.{mismatch}"):
        validate_recording_capture(
            *records,
            session_id=_SESSION + 1 if mismatch == "session_id" else _SESSION,
            spool_path=_SPOOL.with_name("other.float.capture")
            if mismatch == "spool_path"
            else _SPOOL,
            rate=44100 if mismatch == "sample_rate_hz" else _RATE,
            channels=1 if mismatch == "channels" else _CHANNELS,
        )


def test_path_object_in_status_is_not_the_native_exact_string_contract(
    records: tuple[dict[str, object], dict[str, object]],
) -> None:
    records[0]["spool_path"] = _SPOOL
    with pytest.raises(RecordingCaptureValidationError, match=r"status\.spool_path"):
        _validate(records)


@pytest.mark.parametrize(
    ("key", "value"),
    [
        ("state", "prepared_incomplete"),
        ("state", "recording_incomplete"),
        ("state", "failed_incomplete"),
        ("failure", "writer_io"),
        ("failure_code", 9),
        ("session_id", _SESSION + 1),
        ("sample_rate_hz", 44100),
        ("channels", 1),
        ("first_frame", _FIRST + 1),
        ("declared_end_frame", _END + 1),
        ("declared_submitted_frames", _END - _FIRST - 1),
        ("declared_written_frames", _END - _FIRST + 1),
        ("physical_whole_frames", _END - _FIRST - 1),
        ("partial_frame_bytes", 1),
        ("partial_frame_bytes", False),
        ("declared_written_frames", True),
        ("requires_verification", False),
        ("requires_verification", 1),
        ("publication_ready", True),
        ("publication_ready", 0),
    ],
)
def test_checkpoint_requires_exact_nonempty_candidate_agreement_without_publication_permission(
    records: tuple[dict[str, object], dict[str, object]], key: str, value: object
) -> None:
    records[1][key] = value
    with pytest.raises(RecordingCaptureValidationError, match=rf"checkpoint\.{key}"):
        _validate(records)


def test_missing_checkpoint_field_is_not_an_implicit_success_default(
    records: tuple[dict[str, object], dict[str, object]],
) -> None:
    del records[1]["failure_code"]
    with pytest.raises(
        RecordingCaptureValidationError, match=r"checkpoint\.failure_code is missing"
    ):
        _validate(records)


@pytest.mark.parametrize(("first", "end"), [(0, 4), ((1 << 64) - 5, (1 << 64) - 1)])
def test_nonempty_absolute_extent_accepts_real_zero_origin_and_u64_boundary(
    records: tuple[dict[str, object], dict[str, object]], first: int, end: int
) -> None:
    status, checkpoint = records
    status.update(
        first_frame=first,
        end_frame=end,
        captured_end=end,
        submitted_end=end,
        written_end=end,
        submitted_frames=end - first,
        written_frames=end - first,
    )
    checkpoint.update(
        first_frame=first,
        declared_end_frame=end,
        declared_submitted_frames=end - first,
        declared_written_frames=end - first,
        physical_whole_frames=end - first,
    )
    extent = _validate(records)
    assert extent.first_frame == first
    assert extent.end_frame == end
    assert extent.frames == 4
