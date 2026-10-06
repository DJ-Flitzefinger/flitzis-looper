"""Shared control-side scalar projection and final physical marker conversion."""

import math

from flitzis_looper_audio import ScalarSourceGrid


def beat_duration_s(bpm: float | None) -> float | None:
    """Return a finite positive quarter-note duration from the full effective BPM."""
    if bpm is None or not math.isfinite(bpm) or bpm <= 0.0:
        return None
    duration = 60.0 / bpm
    return duration if math.isfinite(duration) and duration > 0.0 else None


def scalar_source_grid(*, origin_s: float, bpm: float | None) -> ScalarSourceGrid | None:
    """Create a precise scalar evaluator, keeping the independent signed source origin."""
    duration_s = beat_duration_s(bpm)
    if duration_s is None or not math.isfinite(origin_s):
        return None
    return ScalarSourceGrid(origin_s, duration_s)


def nearest_grid_source_s(target_s: float, *, grid: ScalarSourceGrid, step_beats: float) -> float:
    """Project the nearest musical point, preserving the existing nearest-even ties."""
    beat = grid.beat_at_source(target_s)
    if beat is None or not math.isfinite(step_beats) or step_beats <= 0.0:
        return target_s
    steps = beat / step_beats
    if not math.isfinite(steps):
        return target_s
    source_s = grid.source_at_beat(round(steps) * step_beats)
    return target_s if source_s is None else source_s


def physical_source_marker_s(source_s: float, *, sample_rate_hz: int | None) -> float:
    """Round an absolute source boundary once to a nonnegative loaded-frame marker.

    Missing/invalid loaded rates leave source seconds unchanged. Valid rates use
    nearest-even sample ties and retain the existing source-zero clamp.
    """
    if sample_rate_hz is None or sample_rate_hz <= 0:
        return source_s
    frames = max(round(source_s * sample_rate_hz), 0)
    return frames / sample_rate_hz
