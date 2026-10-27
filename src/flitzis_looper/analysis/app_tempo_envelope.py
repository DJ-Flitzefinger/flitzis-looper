"""Version-five corroborated thirds, never playback or whole-source authority."""

import math
from itertools import pairwise
from statistics import median
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterable

    from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
    from flitzis_looper.analysis.app_tempo_probe import TempoRegion

type _Point = tuple[int, float]
type _Fit = tuple[float, float, float]


def _sum(values: Iterable[float]) -> float:
    total = 0.0
    for value in values:
        total += value
    return total


def _line(points: list[_Point], keep: list[bool]) -> _Fit | None:
    chosen = [point for point, yes in zip(points, keep, strict=True) if yes]
    if len(chosen) < 2:
        return None
    mx = _sum(float(i) for i, _ in chosen) / len(chosen)
    my = _sum(t for _, t in chosen) / len(chosen)
    xx = _sum((i - mx) * (i - mx) for i, _ in chosen)
    if xx <= 0:
        return None
    period = _sum((i - mx) * (t - my) for i, t in chosen) / xx
    return (mx, my, period) if math.isfinite(period) and 0.2 <= period <= 2.0 else None


def _strict_fit(points: list[_Point], bounds: list[float]) -> tuple[list[int], _Fit] | None:
    if len(points) < 24:
        return None
    keep = [True] * len(points)
    for _ in range(5):
        fit = _line(points, keep)
        if fit is None:
            return None
        mx, my, period = fit
        residual = [t - my - period * (i - mx) for i, t in points]
        center = median(r for r, yes in zip(residual, keep, strict=True) if yes)
        threshold = min(
            0.025,
            0.05 * period,
            max(
                0.010,
                3.5
                * 1.4826
                * median(abs(r - center) for r, yes in zip(residual, keep, strict=True) if yes),
            ),
        )
        next_keep = [abs(r - center) <= threshold for r in residual]
        if next_keep == keep:
            break
        keep = next_keep
    excluded = [i for (i, _), yes in zip(points, keep, strict=True) if not yes]
    retained_points = [p for p, yes in zip(points, keep, strict=True) if yes]
    lower, upper = bounds
    if (
        len(excluded) * 20 > len(points)
        or any(not a and not b for a, b in pairwise(keep))
        or len(retained_points) < 24
    ):
        return None
    first, last = retained_points[0][1], retained_points[-1][1]
    if (
        last - first < 30.0
        or last - first < 0.6 * (upper - lower)
        or first > lower + 0.2 * (upper - lower)
        or last < upper - 0.2 * (upper - lower)
    ):
        return None
    fit = _line(points, keep)
    if fit is None:
        return None
    mx, my, period = fit
    return (
        (excluded, fit)
        if max(abs(t - my - period * (i - mx)) for i, t in retained_points)
        <= min(0.025, 0.05 * period)
        else None
    )


def _matches_fit(method: AppTempoMethod, region: TempoRegion) -> bool:
    lower, upper = region.window_seconds
    points = [(i, t) for i, t in enumerate(method.raw_beats) if lower <= t < upper]
    result = _strict_fit(points, region.window_seconds)
    if result is None:
        return False
    excluded, (mx, my, period) = result
    if region.excluded_raw_indices != excluded or region.retained != len(points) - len(excluded):
        return False
    mask = set(excluded)
    residual = [t - my - period * (i - mx) for i, t in points if i not in mask]
    maximum = max(map(abs, residual))
    rms = math.sqrt(_sum(r * r for r in residual) / len(residual))
    return all(
        math.isclose(actual, expected, rel_tol=1e-12, abs_tol=1e-9)
        for actual, expected in (
            (region.bpm, 60.0 / period),
            (region.max_residual_seconds, maximum),
            (region.rms_residual_seconds, rms),
        )
    )


def _qualifies(method: AppTempoMethod, bounds: list[float]) -> bool:
    points = [(i, t) for i, t in enumerate(method.raw_beats) if bounds[0] <= t < bounds[1]]
    return _strict_fit(points, bounds) is not None


def _validate_complete_support(method: AppTempoMethod, thirds: list[list[float]]) -> None:
    duration = method.frame_count / method.sample_rate_hz
    if _qualifies(method, [0.2 * duration, 0.8 * duration]):
        message = "Envelope cannot omit its strict preferred middle region"
        raise ValueError(message)
    qualifying = {tuple(bounds) for bounds in thirds if _qualifies(method, bounds)}
    if {tuple(region.window_seconds) for region in method.regions} != qualifying:
        message = "Envelope must retain the complete canonical strict-fit source thirds"
        raise ValueError(message)


def validate_envelope_common(
    method: AppTempoMethod, common: TempoRegion | None, *, selected_is_run: bool
) -> bool:
    """Validate the one new fallback; return false for unchanged preferred paths."""
    selected = method.selected
    assert selected is not None
    duration = method.frame_count / method.sample_rate_hz
    middle = next(
        (r for r in method.regions if r.window_seconds == [0.2 * duration, 0.8 * duration]), None
    )
    if middle is not None or selected.window_seconds == [0.0, duration] or selected_is_run:
        return False
    regions = method.regions
    thirds = [
        [0.0, duration / 3.0],
        [duration / 3.0, 2.0 * duration / 3.0],
        [2.0 * duration / 3.0, duration],
    ]
    windows = [r.window_seconds for r in regions]
    canonical = all(window in thirds for window in windows) and len({
        tuple(window) for window in windows
    }) == len(windows)
    eligible = (
        2 <= len(regions) <= 3
        and selected in regions
        and canonical
        and all(abs(r.bpm / selected.bpm - 1.0) <= 0.00025 for r in regions)
        and any(
            (
                r.window_seconds[0] < selected.window_seconds[0]
                and r.window_seconds[1] < selected.window_seconds[1]
            )
            or (
                r.window_seconds[0] > selected.window_seconds[0]
                and r.window_seconds[1] > selected.window_seconds[1]
            )
            for r in regions
        )
    )
    if not eligible:
        if common is not None:
            message = "Envelope requires all unique canonical corroborating source thirds"
            raise ValueError(message)
        return False
    if not all(_matches_fit(method, region) for region in regions):
        message = "Envelope supporting third differs from its strict original fit"
        raise ValueError(message)
    _validate_complete_support(method, thirds)
    bounds = [min(w[0] for w in windows), max(w[1] for w in windows)]
    points = [(i, t) for i, t in enumerate(method.raw_beats) if bounds[0] <= t < bounds[1]]
    fit = _strict_fit(points, bounds)
    if fit is None:
        if common is not None:
            message = "Envelope has no strict fit through all original observations"
            raise ValueError(message)
        return False
    if common is None or common.window_seconds != bounds or not _matches_fit(method, common):
        message = "Envelope common coverage differs from its one strict original hull fit"
        raise ValueError(message)
    return True
