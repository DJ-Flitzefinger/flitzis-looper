"""Small explicit manual-anchor intent; Native owns source geometry and validation."""

import json
import math
import struct
from typing import Literal

type ManualBeatMode = Literal["interior", "extend_start", "extend_end"]


def _revision(value: str) -> None:
    if (
        type(value) is not str
        or len(value) != 64
        or any(character not in "0123456789abcdef" for character in value)
    ):
        message = "Insertion requires exact current map and capture revisions."
        raise ValueError(message)


def _endpoints(mode: ManualBeatMode, first: int, last: int) -> None:
    if any(type(index) is not int or not 0 <= index < 250_000 for index in (first, last)):
        message = "Fixed anchor indices must be integers within 0 to 249999."
        raise ValueError(message)
    if type(mode) is not str or mode not in {"interior", "extend_start", "extend_end"}:
        message = "Choose Interior, Extend start or Extend end."
        raise ValueError(message)
    expected_last = first + 1 if mode == "interior" else first
    if last != expected_last or (mode == "extend_start" and first != 0):
        message = "Insertion requires adjacent interior endpoints or one fixed outer endpoint."
        raise ValueError(message)


def insert_manual_beat_operation(
    map_revision: str,
    capture_revision: str,
    mode: ManualBeatMode,
    first_anchor_index: int,
    last_anchor_index: int,
    sample_index: int,
    sample_rate_hz: int,
    quarter: float,
) -> dict[str, object]:
    """Encode one explicit sample/quarter pair without inferring source or beat positions."""
    _revision(map_revision)
    _revision(capture_revision)
    _endpoints(mode, first_anchor_index, last_anchor_index)
    if type(sample_index) is not int or not 0 <= sample_index < 1 << 64:
        message = "Source sample must be a non-negative unsigned 64-bit integer."
        raise ValueError(message)
    if type(sample_rate_hz) is not int or not 0 < sample_rate_hz < 1 << 32:
        message = "Source sample rate must be a positive unsigned 32-bit integer."
        raise ValueError(message)
    if type(quarter) is not float or not math.isfinite(quarter):
        message = "Enter an explicit finite quarter-note coordinate."
        raise ValueError(message)
    operation: dict[str, object] = {
        "schema_version": 1,
        "operation": "insert_manual_beat",
        "map_revision": map_revision,
        "capture_revision": capture_revision,
        "mode": mode,
        "first_anchor_index": first_anchor_index,
        "last_anchor_index": last_anchor_index,
        "sample_index": sample_index,
        "sample_rate_hz": sample_rate_hz,
        "quarter_bits": struct.pack(">d", quarter).hex(),
    }
    if len(json.dumps(operation, allow_nan=False, separators=(",", ":")).encode("utf-8")) > 8192:
        message = "Manual beat intent exceeds the 8 KiB limit."
        raise ValueError(message)
    return operation
