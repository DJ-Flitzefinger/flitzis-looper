"""Bounded explicit window intent; Native retains all map and source authority."""

import json
import math
import struct
from dataclasses import dataclass
from typing import Literal

type ReviewWindowMode = Literal["extend_end", "extend_start", "replace_interior"]
type ReviewRowKind = Literal["raw", "existing"]


@dataclass(frozen=True, slots=True)
class ReviewWindowRow:
    """One explicit quarter label referencing Native-owned source positions."""

    kind: ReviewRowKind
    index: int
    quarter: float

    def identity(self) -> tuple[str, int, str]:
        """Retain exact quarter bits, including signed zero, in gesture identity."""
        return self.kind, self.index, struct.pack(">d", self.quarter).hex()


def _revision(value: str) -> None:
    if (
        type(value) is not str
        or len(value) != 64
        or any(character not in "0123456789abcdef" for character in value)
    ):
        message = "Review requires an exact current map and capture revision."
        raise ValueError(message)


def _index(value: int) -> None:
    if type(value) is not int or not 0 <= value < 250_000:
        message = "Review indices must be integers within 0 to 249999."
        raise ValueError(message)


def _window_shape(
    mode: ReviewWindowMode,
    first_anchor_index: int,
    last_anchor_index: int,
    rows: tuple[ReviewWindowRow, ...],
) -> None:
    _index(first_anchor_index)
    _index(last_anchor_index)
    if type(mode) is not str or mode not in {"extend_end", "extend_start", "replace_interior"}:
        message = "Choose Extend end, Extend start or Replace interior."
        raise ValueError(message)
    if mode == "replace_interior":
        if not 0 <= last_anchor_index - first_anchor_index - 1 <= 62 or len(rows) > 62:
            message = "Replacement supports at most 62 old and 62 requested interior rows."
            raise ValueError(message)
    elif first_anchor_index != last_anchor_index or not 1 <= len(rows) <= 63:
        message = "Extension requires one fixed outer anchor and 1 to 63 new Raw labels."
        raise ValueError(message)


def _encode_rows(
    mode: ReviewWindowMode,
    first_anchor_index: int,
    last_anchor_index: int,
    rows: tuple[ReviewWindowRow, ...],
) -> list[dict[str, object]]:
    encoded_rows: list[dict[str, object]] = []
    used: set[tuple[str, int]] = set()
    previous_quarter = -math.inf
    for row in rows:
        _index(row.index)
        if row.kind not in {"raw", "existing"}:
            message = "Each row must explicitly reference Raw or Existing."
            raise ValueError(message)
        if row.kind == "existing" and (
            mode != "replace_interior" or not first_anchor_index < row.index < last_anchor_index
        ):
            message = "Existing rows must be selected interior anchors in replacement mode."
            raise ValueError(message)
        if type(row.quarter) is not float or not math.isfinite(row.quarter):
            message = "Enter an explicit finite quarter-note coordinate for every row."
            raise ValueError(message)
        if row.quarter <= previous_quarter or (row.kind, row.index) in used:
            message = "Rows require unique references and strictly increasing quarter labels."
            raise ValueError(message)
        encoded_rows.append({
            "kind": row.kind,
            "raw_index" if row.kind == "raw" else "anchor_index": row.index,
            "quarter_bits": row.identity()[2],
        })
        used.add((row.kind, row.index))
        previous_quarter = row.quarter
    return encoded_rows


def review_window_operation(
    map_revision: str,
    capture_revision: str,
    mode: ReviewWindowMode,
    first_anchor_index: int,
    last_anchor_index: int,
    rows: tuple[ReviewWindowRow, ...],
) -> dict[str, object]:
    """Build small explicit intent without resolving or inferring source geometry."""
    _revision(map_revision)
    _revision(capture_revision)
    _window_shape(mode, first_anchor_index, last_anchor_index, rows)
    operation: dict[str, object] = {
        "schema_version": 1,
        "operation": "review_raw_window",
        "map_revision": map_revision,
        "capture_revision": capture_revision,
        "mode": mode,
        "first_anchor_index": first_anchor_index,
        "last_anchor_index": last_anchor_index,
        "rows": _encode_rows(mode, first_anchor_index, last_anchor_index, rows),
    }
    encoded = json.dumps(operation, allow_nan=False, separators=(",", ":"))
    if len(encoded.encode("utf-8")) > 8192:
        message = "Review window intent exceeds the 8 KiB limit."
        raise ValueError(message)
    return operation
