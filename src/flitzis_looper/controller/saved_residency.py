"""Translate durable loop intent into a finite initial native resident request."""

import math
import struct
from dataclasses import dataclass
from typing import TYPE_CHECKING

from flitzis_looper.controller.scalar_grid import (
    beat_duration_s,
    physical_source_marker_s,
    scalar_source_grid,
)
from flitzis_looper.controller.timing_metadata import timing_anchor_sec_from_analysis

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState


@dataclass(frozen=True, slots=True)
class SavedResidentLoop:
    """Absolute physical source markers; native admission owns extent and context."""

    start_seconds: float
    end_seconds: float
    key_lock: bool


def saved_resident_loop(
    project: ProjectState, sample_id: int, *, sample_rate_hz: int
) -> SavedResidentLoop | None:
    """Return a finite saved loop hint without conferring timing acceptance.

    A historical Automatic period only bounds the initial storage request. Its
    complete evidence still has to pass fresh native verification and adoption.
    Missing or malformed historical geometry admits the complete-track path.
    Complete duration is deliberately resolved by the native full-source owner.
    """
    if sample_rate_hz <= 0:
        return None
    start = project.pad_loop_start_s[sample_id]
    end = project.pad_loop_end_s[sample_id]
    if project.pad_loop_auto[sample_id]:
        projection = _saved_period_origin(project, sample_id, sample_rate_hz=sample_rate_hz)
        if projection is None:
            return None
        period, origin = projection
        grid = scalar_source_grid(origin_s=origin, period_seconds=period)
        end = (
            grid.source_after_beats(start, project.pad_loop_bars[sample_id] * 4.0)
            if grid is not None
            else None
        )
    if end is None or not math.isfinite(start) or not math.isfinite(end) or start < 0.0:
        return None
    start = physical_source_marker_s(start, sample_rate_hz=sample_rate_hz)
    end = physical_source_marker_s(end, sample_rate_hz=sample_rate_hz)
    if end <= start:
        end = start + 1.0 / sample_rate_hz
    if not math.isfinite(end) or end <= start:
        return None
    return SavedResidentLoop(start, end, project.pad_key_lock[sample_id])


def _saved_period_origin(
    project: ProjectState, sample_id: int, *, sample_rate_hz: int
) -> tuple[float, float] | None:
    analysis = project.sample_analysis[sample_id]
    manual = project.manual_bpm[sample_id]
    if manual is None and project.pad_timing_intent[sample_id] == "automatic":
        saved = analysis.accepted_timing if analysis is not None else None
        if saved is None:
            return None
        period = _historical_float(saved.record.get("period_bits"))
        origin_record = saved.record.get("origin")
        origin = (
            _historical_float(origin_record.get("seconds_bits"))
            if isinstance(origin_record, dict)
            else None
        )
        if period is None or period <= 0.0 or origin is None:
            return None
        return period, origin
    period = beat_duration_s(manual if manual is not None else analysis.bpm if analysis else None)
    if period is None:
        return None
    anchor = project.pad_grid_anchor_s[sample_id]
    if anchor is None:
        anchor = timing_anchor_sec_from_analysis(analysis)
    anchor_frame = max(round(anchor * sample_rate_hz), 0)
    origin = (anchor_frame + project.pad_grid_offset_samples[sample_id]) / sample_rate_hz
    return period, origin


def _historical_float(value: object) -> float | None:
    if not isinstance(value, str) or len(value) != 16:
        return None
    try:
        decoded = bytes.fromhex(value)
    except ValueError:
        return None
    if len(decoded) != 8:
        return None
    result = struct.unpack("!d", decoded)[0]
    return result if math.isfinite(result) else None
