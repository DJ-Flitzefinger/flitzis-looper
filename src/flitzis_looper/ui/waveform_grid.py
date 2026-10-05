"""Pure source-grid projections and virtual waveform view bounds."""

import math
from dataclasses import dataclass

_MIN_MINOR_STEP_PX = 12.0
_MAX_GRID_LINES = 2048
_BEAT_TOLERANCE = 1e-9


@dataclass(frozen=True, slots=True)
class WaveformGridLine:
    """One real scalar-grid position with a loop-relative beat coordinate."""

    source_s: float
    loop_beat: float
    major: bool
    reference: bool = False


def beat_duration_s(bpm: float | None) -> float | None:
    """Return a finite positive quarter-note duration, when available."""
    if bpm is None or not math.isfinite(bpm) or bpm <= 0.0:
        return None
    duration = 60.0 / bpm
    return duration if math.isfinite(duration) and duration > 0.0 else None


def waveform_view_start(loop_start_s: float, bpm: float | None) -> float:
    """Include a beat of visual space before a loop, without adding source audio."""
    beat_s = beat_duration_s(bpm)
    if beat_s is None:
        return loop_start_s
    return loop_start_s - beat_s


def loop_beat_label(beat: float) -> str:
    """Format continuous loop-relative beats without turning off-grid values into IDs."""
    integer = round(beat)
    if math.isclose(beat, integer, rel_tol=0.0, abs_tol=_BEAT_TOLERANCE):
        return str(integer)
    label = f"{beat:.4f}".rstrip("0").rstrip(".")
    # Sample-rounding or a tiny manual offset must not masquerade as an exact beat.
    return f"~{label}" if "." not in label else label


def _minor_step_beats(beat_s: float, px_per_s: float) -> float:
    for step in (1 / 16, 1 / 8, 1 / 4, 1 / 2, 1.0, 4.0, 16.0):
        if step * beat_s * px_per_s >= _MIN_MINOR_STEP_PX:
            return step
    return 16.0


def _major_every(step_beats: float) -> int:
    if step_beats in {1.0, 4.0}:
        return 4
    return round(1.0 / step_beats) if step_beats < 1.0 else 1


def _reference_grid_beats(loop_grid_beat: float) -> tuple[float, ...]:
    """Return actual finest-grid references near the loop, never invented grid lines."""
    if not math.isfinite(loop_grid_beat * 16.0):
        return ()
    nearest_tick = round(loop_grid_beat * 16.0) / 16.0
    if not math.isclose(loop_grid_beat, nearest_tick, rel_tol=0.0, abs_tol=_BEAT_TOLERANCE):
        return ()
    return (nearest_tick - 1.0, nearest_tick)


def visible_grid_lines(
    *,
    start_s: float,
    end_s: float,
    anchor_s: float,
    loop_start_s: float,
    bpm: float,
    px_per_s: float,
) -> tuple[WaveformGridLine, ...]:
    """Project bounded scalar grid lines, retaining real beat zero/one references.

    Grid positions always come from the source anchor. The selected loop changes
    only the displayed continuous coordinate ``1 + (source - loop_start) / beat``.
    """
    beat_s = beat_duration_s(bpm)
    if (
        beat_s is None
        or not all(math.isfinite(value) for value in (start_s, end_s, anchor_s, loop_start_s))
        or not math.isfinite(px_per_s)
        or px_per_s <= 0.0
        or end_s <= start_s
    ):
        return ()
    first_beat = (start_s - anchor_s) / beat_s
    last_beat = (end_s - anchor_s) / beat_s
    loop_grid_beat = (loop_start_s - anchor_s) / beat_s
    if not all(math.isfinite(value) for value in (first_beat, last_beat, loop_grid_beat)):
        return ()
    step_beats = _minor_step_beats(beat_s, px_per_s)
    # Keep even corrupt/extreme metadata from causing an unbounded UI loop.
    span_beats = last_beat - first_beat
    if not math.isfinite(span_beats):
        return ()
    if span_beats > (_MAX_GRID_LINES - 4) * step_beats:
        step_beats = max(16.0, math.ceil(span_beats / (_MAX_GRID_LINES - 4) / 16.0) * 16.0)
    if not math.isfinite(first_beat / step_beats) or not math.isfinite(last_beat / step_beats):
        return ()
    every = _major_every(step_beats)
    first = math.ceil(first_beat / step_beats)
    last = math.floor(last_beat / step_beats)
    projected: dict[float, WaveformGridLine] = {}
    for index in range(first, min(last + 1, first + _MAX_GRID_LINES - 2)):
        grid_beat = index * step_beats
        projected[grid_beat] = WaveformGridLine(
            anchor_s + grid_beat * beat_s,
            1.0 + grid_beat - loop_grid_beat,
            index % every == 0,
        )
    for grid_beat in _reference_grid_beats(loop_grid_beat):
        if first_beat <= grid_beat <= last_beat:
            projected[grid_beat] = WaveformGridLine(
                anchor_s + grid_beat * beat_s,
                1.0 + grid_beat - loop_grid_beat,
                major=True,
                reference=True,
            )
    return tuple(
        sorted(
            (
                line
                for line in projected.values()
                if math.isfinite(line.loop_beat)
                and math.isfinite(line.source_s)
                and not math.isclose(line.loop_beat, 0.0, rel_tol=0.0, abs_tol=_BEAT_TOLERANCE)
            ),
            key=lambda line: line.source_s,
        )
    )
