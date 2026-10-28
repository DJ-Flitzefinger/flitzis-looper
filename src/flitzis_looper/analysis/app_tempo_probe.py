"""Strict counterprobe evidence; estimates never grant playback authority."""

import math
from itertools import pairwise
from typing import TYPE_CHECKING, Annotated, Literal, NoReturn, Protocol

from pydantic import BaseModel, ConfigDict, Field

if TYPE_CHECKING:
    from collections.abc import Iterable

    from flitzis_looper.analysis.app_tempo_inferred import OrdinalFit

_Time = Annotated[float, Field(allow_inf_nan=False, ge=0)]


class TempoRegion(BaseModel):
    model_config = ConfigDict(extra="forbid")

    window_seconds: Annotated[list[_Time], Field(min_length=2, max_length=2)]
    bpm: Annotated[float, Field(allow_inf_nan=False, ge=30, le=300)]
    retained: Annotated[int, Field(ge=8, le=250_000)]
    excluded_raw_indices: Annotated[list[int], Field(max_length=250_000)]
    max_residual_seconds: Annotated[float, Field(allow_inf_nan=False, ge=0, le=0.025)]
    rms_residual_seconds: Annotated[float, Field(allow_inf_nan=False, ge=0, le=0.025)]


class IntegerTempoProbe(BaseModel):
    """Finite policy preference; never Native timing/map/phase authorization."""

    model_config = ConfigDict(extra="forbid")

    status: Literal["supported_integer_estimate", "fractional_estimate", "inconclusive"]
    reason: Literal[
        "two_windows_and_common_phase",
        "long_span_mismatch",
        "fractional_preferred",
        "insufficient_inner_coverage",
    ]
    candidate_bpm: Annotated[float, Field(allow_inf_nan=False, ge=30, le=300)]
    base_bpm: Annotated[float, Field(allow_inf_nan=False, ge=30, le=300)]
    windows: Annotated[list[TempoRegion], Field(max_length=2)]
    common_region: TempoRegion | None
    common_max_residual_seconds: _Time | None
    common_rms_residual_seconds: _Time | None
    rms_penalty_seconds: _Time | None
    window_phase_difference_seconds: _Time | None


class ProbeWindow(TempoRegion):
    """Four-point windows are exclusive to the declared v4 short-source tier."""

    retained: Annotated[int, Field(ge=4, le=250_000)]


class FractionalTempoCheck(BaseModel):
    model_config = ConfigDict(extra="forbid")

    status: Literal["pass", "fail", "inconclusive"]
    max_residual_seconds: _Time | None
    rms_residual_seconds: _Time | None
    rms_penalty_seconds: _Time | None
    window_phase_difference_seconds: _Time | None


class AdaptiveTempoProbe(BaseModel):
    model_config = ConfigDict(extra="forbid")

    status: Literal["supported_integer_estimate", "fractional_estimate", "inconclusive"]
    reason: Literal[
        "two_windows_and_common_phase",
        "long_span_mismatch",
        "fractional_preferred",
        "insufficient_probe_coverage",
        "no_supported_candidate",
    ]
    candidate_bpm: Annotated[float, Field(allow_inf_nan=False, ge=30, le=300)]
    base_bpm: Annotated[float, Field(allow_inf_nan=False, ge=30, le=300)]
    windows: Annotated[list[ProbeWindow], Field(max_length=2)]
    common_region: TempoRegion | None
    common_max_residual_seconds: _Time | None
    common_rms_residual_seconds: _Time | None
    rms_penalty_seconds: _Time | None
    window_phase_difference_seconds: _Time | None
    evidence_tier: Literal["two_ten_second", "short_source", "inconclusive"]
    window_gap_seconds: _Time | None
    pivot_raw_index: _Time | None
    pivot_seconds: _Time | None
    fractional_check: FractionalTempoCheck


class TempoMathView(Protocol):
    """Only scalar evidence and source geometry, without backend or execution claims."""

    @property
    def schema_version(self) -> int: ...

    @property
    def status(self) -> str: ...

    @property
    def bpm(self) -> float | None: ...

    @property
    def selected(self) -> TempoRegion | None: ...

    @property
    def regions(self) -> list[TempoRegion]: ...

    @property
    def integer_probe(self) -> IntegerTempoProbe | AdaptiveTempoProbe | None: ...

    @property
    def ordinal_fit(self) -> OrdinalFit | None: ...

    @property
    def sample_rate_hz(self) -> int: ...

    @property
    def frame_count(self) -> int: ...

    @property
    def raw_beats(self) -> list[float]: ...

    @property
    def raw_downbeats(self) -> list[float]: ...


def _invalid(message: str) -> NoReturn:
    raise ValueError(message)


def _sum(values: Iterable[float]) -> float:
    # Match the Native left-to-right binary64 pivot. Python's compensated
    # builtin sum is intentionally different on newer supported interpreters.
    total = 0.0
    for value in values:
        total += value
    return total


def _close(actual: float | None, expected: float | None) -> bool:
    return (
        actual is expected
        if expected is None
        else (actual is not None and math.isclose(actual, expected, rel_tol=1e-12, abs_tol=1e-9))
    )


def _points(method: TempoMathView, region: TempoRegion) -> list[tuple[int, float]]:
    excluded = region.excluded_raw_indices
    start, end = region.window_seconds
    if excluded != sorted(set(excluded)) or any(
        not 0 <= i < len(method.raw_beats) or not start <= method.raw_beats[i] < end
        for i in excluded
    ):
        _invalid("Counterprobe mask differs from original observations")
    mask = set(excluded)
    return [(i, t) for i, t in enumerate(method.raw_beats) if start <= t < end and i not in mask]


def _pivot(points: list[tuple[int, float]]) -> tuple[float, float, float]:
    if len(points) < 2:
        _invalid("Counterprobe common fit has insufficient observations")
    mx = _sum(float(i) for i, _ in points) / len(points)
    my = _sum(t for _, t in points) / len(points)
    xx = _sum((i - mx) * (i - mx) for i, _ in points)
    if xx <= 0:
        _invalid("Counterprobe common indices have no extent")
    period = _sum((i - mx) * (t - my) for i, t in points) / xx
    if not 0.2 <= period <= 2:
        _invalid("Counterprobe common line has unsupported period")
    return mx, my, period


def _is_run(method: TempoMathView, window: list[float]) -> bool:
    beats = method.raw_beats
    duration = method.frame_count / method.sample_rate_hz
    bounds = []
    start = 0
    for end in range(2, len(beats) + 1):
        if (
            end == len(beats)
            or not 2 / 3
            <= ((beats[end] - beats[end - 1]) / (beats[end - 1] - beats[end - 2]))
            <= 1.5
        ):
            if end - start >= 24 and beats[end - 1] - beats[start] >= 30:
                bounds.append([
                    beats[start],
                    min(math.nextafter(beats[end - 1], math.inf), duration),
                ])
                if len(bounds) > 64:
                    return False
            start = end
    return window in bounds


def _strict_common(
    method: TempoMathView,
    common: TempoRegion,
    points: list[tuple[int, float]],
    minimum: tuple[int, float] | None = None,
) -> None:
    duration = method.frame_count / method.sample_rate_hz
    whole = common.window_seconds == [0.0, duration]
    count, span = minimum if minimum is not None else (8, 4.0) if whole else (24, 30.0)
    lower, upper = common.window_seconds
    available = [(i, t) for i, t in enumerate(method.raw_beats) if lower <= t < upper]
    mask = set(common.excluded_raw_indices)
    if len(points) < count:
        _invalid("Counterprobe common region lacks the required observations")
    bad_exclusions = len(mask) * 20 > len(available) or any(
        a[0] in mask and b[0] in mask for a, b in pairwise(available)
    )
    if (
        bad_exclusions
        or points[-1][1] - points[0][1] < span
        or points[-1][1] - points[0][1] < 0.6 * (upper - lower)
        or points[0][1] > lower + 0.2 * (upper - lower)
        or points[-1][1] < upper - 0.2 * (upper - lower)
    ):
        _invalid("Counterprobe common region does not meet the strict original fit gates")
    mx, my, period = _pivot(points)
    residual = [t - my - period * (i - mx) for i, t in points]
    maximum = max(map(abs, residual))
    rms = math.sqrt(_sum(r * r for r in residual) / len(residual))
    if (
        maximum > min(0.025, 0.05 * period)
        or not _close(common.bpm, 60 / period)
        or not _close(common.max_residual_seconds, maximum)
        or not _close(common.rms_residual_seconds, rms)
    ):
        _invalid("Counterprobe common fit differs from its original retained observations")


def _metrics(
    points: list[tuple[int, float]],
    windows: list[ProbeWindow],
    pivot: tuple[float, float, float],
    bpm: float,
) -> tuple[float, float, float, float]:
    mx, my, free_period = pivot
    residual = [t - my - (60 / bpm) * (i - mx) for i, t in points]
    rms = math.sqrt(_sum(r * r for r in residual) / len(points))
    free = math.sqrt(_sum((t - my - free_period * (i - mx)) ** 2 for i, t in points) / len(points))
    offsets = []
    for window in windows:
        start, end = window.window_seconds
        local = [r for (_, t), r in zip(points, residual, strict=True) if start <= t < end]
        if not local:
            _invalid("Counterprobe window has no common observations")
        offsets.append(_sum(local) / len(local))
    return max(map(abs, residual)), rms, max(0.0, rms - free), abs(offsets[1] - offsets[0])


def _passes(metrics: tuple[float, float, float, float], bpm: float) -> bool:
    maximum, _, penalty, phase = metrics
    return maximum <= min(0.025, 0.05 * (60 / bpm)) and penalty <= 0.00025 and phase <= 0.005


def _check_metrics(actual: tuple[float | None, ...], expected: tuple[float, ...]) -> None:
    if len(actual) != len(expected) or not all(map(_close, actual, expected, strict=True)):
        _invalid("Counterprobe metrics differ from the shared original-index pivot")


def _validate_windows(
    method: TempoMathView,
    probe: AdaptiveTempoProbe,
    points: list[tuple[int, float]],
) -> None:
    if len(probe.windows) != 2:
        _invalid("A supported evidence tier requires two disjoint windows")
    count, span = (8, 10.0) if probe.evidence_tier == "two_ten_second" else (4, 1.0)
    endpoints = []
    common = probe.common_region
    assert common is not None
    for window in probe.windows:
        start, end = window.window_seconds
        if not common.window_seconds[0] <= start < end <= common.window_seconds[1]:
            _invalid("Counterprobe window is outside its common original region")
        local = _points(method, window)
        shared = [(i, t) for i, t in points if start <= t < end]
        # Local fits cannot remove additional events from shared candidate phase.
        for retained in (local, shared):
            if len(retained) < count or retained[-1][1] - retained[0][1] < span:
                _invalid("Counterprobe lacks its declared actual count or span")
        if window.retained != len(local):
            _invalid("Counterprobe window count differs from its original mask")
        _strict_common(method, window, local, (count, span))
        endpoints.append((shared[0], shared[-1]))
    a, b = probe.windows
    if a.window_seconds[1] > b.window_seconds[0] or endpoints[0][1][0] >= endpoints[1][0][0]:
        _invalid("Counterprobe windows overlap in source time or original index")
    gap = endpoints[1][0][1] - endpoints[0][1][1]
    if gap <= 0 or not _close(probe.window_gap_seconds, gap):
        _invalid("Counterprobe gap differs from retained original endpoints")
    _validate_window_geometry(method, probe, points, endpoints)
    if probe.evidence_tier == "short_source":
        _validate_short_tier(method, probe, points, endpoints)


def _validate_window_geometry(
    method: TempoMathView,
    probe: AdaptiveTempoProbe,
    points: list[tuple[int, float]],
    endpoints: list[tuple[tuple[int, float], tuple[int, float]]],
) -> None:
    common = probe.common_region
    assert common is not None
    duration = method.frame_count / method.sample_rate_hz
    lower, upper = 0.2 * duration, 0.8 * duration
    historical = (
        probe.evidence_tier == "two_ten_second"
        and upper - lower >= 42.0
        and common.window_seconds == [lower, upper]
        and [window.window_seconds for window in probe.windows]
        == [[lower, lower + 16.0], [upper - 16.0, upper]]
    )
    if historical:
        return
    left, right = endpoints
    split = len(points) // 2
    if (
        left[0] != points[0]
        or right[1] != points[-1]
        or left[1][0] >= points[split][0]
        or right[0][0] < points[split][0]
    ):
        _invalid("Adaptive probe endpoints must address opposite outer retained halves")
    # Native selects the right edge using last-16. Permit the single rounding
    # step from expressing that same bound as first+16, not a longer window.
    if any(last[1] > math.nextafter(first[1] + 16.0, math.inf) for first, last in endpoints):
        _invalid("Adaptive probe span exceeds its sixteen-second outer window")


def _validate_short_tier(
    method: TempoMathView,
    probe: AdaptiveTempoProbe,
    points: list[tuple[int, float]],
    endpoints: list[tuple[tuple[int, float], tuple[int, float]]],
) -> None:
    common = probe.common_region
    assert common is not None
    duration = method.frame_count / method.sample_rate_hz
    if (
        common.window_seconds != [0.0, duration]
        or len(points) < 8
        or points[-1][1] - points[0][1] < 4
    ):
        _invalid("Short-source evidence requires a genuine whole-original fit")
    if all(
        len(local := _points(method, w)) >= 8
        and local[-1][1] - local[0][1] >= 10
        and len([t for _, t in points if w.window_seconds[0] <= t < w.window_seconds[1]]) >= 8
        and ends[1][1] - ends[0][1] >= 10
        for w, ends in zip(probe.windows, endpoints, strict=True)
    ):
        _invalid("Ten-second evidence cannot be relabelled as short source")


def _common_points(method: TempoMathView, probe: AdaptiveTempoProbe) -> list[tuple[int, float]]:
    common = probe.common_region
    if common is None:
        if probe.pivot_raw_index is not None or probe.pivot_seconds is not None:
            _invalid("Missing common coverage cannot carry a pivot")
        return []
    duration = method.frame_count / method.sample_rate_hz
    middle = next(
        (r for r in method.regions if r.window_seconds == [0.2 * duration, 0.8 * duration]), None
    )
    expected = middle if middle is not None else method.selected
    if common not in method.regions or common != expected:
        _invalid("Counterprobe must use its one genuine preferred common region")
    if (
        middle is None
        and common.window_seconds != [0.0, duration]
        and not _is_run(method, common.window_seconds)
    ):
        _invalid("Fallback common coverage must be the selected original contiguous run")
    points = _points(method, common)
    if len(points) != common.retained:
        _invalid("Counterprobe common count differs from original observations")
    _strict_common(method, common, points)
    mx, my, _ = _pivot(points)
    if not _close(probe.pivot_raw_index, mx) or not _close(probe.pivot_seconds, my):
        _invalid("Counterprobe pivot differs from common retained observations")
    return points


def historical_probe(
    probe: IntegerTempoProbe | AdaptiveTempoProbe | None,
) -> IntegerTempoProbe | None:
    """Keep version-four evidence outside the historical validation branch."""
    if isinstance(probe, AdaptiveTempoProbe):
        _invalid("Adaptive evidence cannot weaken historical tempo metadata")
    return probe


def validate_adaptive_probe(
    method: TempoMathView,
    probe: IntegerTempoProbe | AdaptiveTempoProbe | None,
) -> None:
    """Recompute both exact candidate periods using the same immutable common mask."""
    if not isinstance(probe, AdaptiveTempoProbe):
        _invalid("Version four requires its adaptive dual candidate evidence")
    if method.selected is None or probe.base_bpm != method.selected.bpm:
        _invalid("Counterprobe base differs from the exact selected regional fit")
    if probe.candidate_bpm != min(300, max(30, math.floor(probe.base_bpm + 0.5))):
        _invalid("Counterprobe candidate is not the nearest integer")
    points = _common_points(method, probe)
    integer_fields = (
        probe.common_max_residual_seconds,
        probe.common_rms_residual_seconds,
        probe.rms_penalty_seconds,
        probe.window_phase_difference_seconds,
    )
    fractional = probe.fractional_check
    fractional_fields = (
        fractional.max_residual_seconds,
        fractional.rms_residual_seconds,
        fractional.rms_penalty_seconds,
        fractional.window_phase_difference_seconds,
    )
    if probe.evidence_tier == "inconclusive":
        has_metrics = any(x is not None for x in (*integer_fields, *fractional_fields))
        bad_status = (
            probe.status != "inconclusive"
            or probe.reason != "insufficient_probe_coverage"
            or fractional.status != "inconclusive"
        )
        if bad_status or probe.windows or probe.window_gap_seconds is not None or has_metrics:
            _invalid("Unavailable probe coverage cannot carry a successful candidate check")
        return
    if not points:
        _invalid("Candidate checks require genuine common observations")
    _validate_windows(method, probe, points)
    pivot = _pivot(points)
    integer_metrics = _metrics(points, probe.windows, pivot, probe.candidate_bpm)
    fractional_metrics = _metrics(points, probe.windows, pivot, probe.base_bpm)
    _check_metrics(integer_fields, integer_metrics)
    _check_metrics(fractional_fields, fractional_metrics)
    integer_pass = _passes(integer_metrics, probe.candidate_bpm)
    fractional_pass = _passes(fractional_metrics, probe.base_bpm)
    _validate_outcome(probe, integer_pass=integer_pass, fractional_pass=fractional_pass)


def _validate_outcome(
    probe: AdaptiveTempoProbe,
    *,
    integer_pass: bool,
    fractional_pass: bool,
) -> None:
    if probe.fractional_check.status != ("pass" if fractional_pass else "fail"):
        _invalid("Fractional check contradicts its exact selected base")
    if integer_pass and abs(probe.candidate_bpm - probe.base_bpm) <= 0.005:
        expected_status, reason = "supported_integer_estimate", "two_windows_and_common_phase"
    elif fractional_pass:
        expected_status = "fractional_estimate"
        reason = "fractional_preferred" if integer_pass else "long_span_mismatch"
    else:
        expected_status, reason = "inconclusive", "no_supported_candidate"
    if (probe.status, probe.reason) != (expected_status, reason):
        _invalid("Counterprobe status contradicts its independent candidate checks")
