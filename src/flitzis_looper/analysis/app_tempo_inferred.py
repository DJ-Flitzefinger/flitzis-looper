"""Replay bounded inferred-ordinal estimates without granting playback authority."""

import math
from bisect import bisect_left
from itertools import pairwise
from typing import TYPE_CHECKING, Annotated, Literal, NoReturn

from pydantic import BaseModel, ConfigDict, Field, field_validator

from flitzis_looper.analysis.app_tempo_probe import FractionalTempoCheck, _close, _sum

if TYPE_CHECKING:
    from flitzis_looper.analysis.app_tempo_probe import TempoMathView

_Count = Annotated[int, Field(strict=True, ge=0)]
_EXACT_INTEGER = 2**53 - 1
_Ordinal = Annotated[int, Field(strict=True, ge=0, le=_EXACT_INTEGER)]
_Time = Annotated[float, Field(allow_inf_nan=False, ge=0)]
_Finite = Annotated[float, Field(allow_inf_nan=False)]
_Bpm = Annotated[float, Field(allow_inf_nan=False, ge=30, le=300)]
_Bounds = Annotated[list[_Time], Field(min_length=2, max_length=2)]
_Assignment = Annotated[list[_Ordinal], Field(min_length=2, max_length=2)]


class SpanDiagnostics(BaseModel):
    """One fixed source-time span, including every raw event and expected slot."""

    model_config = ConfigDict(extra="forbid")

    window_seconds: _Bounds
    raw_count: _Count
    expected_slots: _Count
    matched: _Count
    observed_span_seconds: _Time | None
    fitted_bpm: _Bpm | None
    max_residual_seconds: _Time | None
    rms_residual_seconds: _Time | None
    max_gap_seconds: _Time


class CandidateSummary(BaseModel):
    """A bounded scalar rival summary, not a second retained assignment vector."""

    model_config = ConfigDict(extra="forbid", serialize_by_alias=True)

    period_seconds: Annotated[float, Field(allow_inf_nan=False, ge=0.2, le=2)]
    anchor_seconds: _Finite
    pivot_ordinal: _Time
    pivot_seconds: _Time
    global_span: SpanDiagnostics = Field(alias="global")
    sections: Annotated[list[SpanDiagnostics], Field(min_length=5, max_length=5)]


class BudgetOrdinalFit(BaseModel):
    """An unavailable optional fit must not claim observations or a candidate."""

    model_config = ConfigDict(extra="forbid")

    outcome: Literal["budget_unavailable", "allocation_unavailable"]
    required_bytes: _Count
    available_bytes: _Count


class HarmonicCheck(BaseModel):
    """One fixed seed, replayed separately from the unrestricted Native search."""

    model_config = ConfigDict(extra="forbid")

    factor: _Finite
    seed_period_seconds: Annotated[float, Field(allow_inf_nan=False, gt=0)]
    seed_anchor_seconds: _Finite
    outcome: Literal["fitted", "no_fixed_fit", "out_of_range"]
    alternative_index: Annotated[int, Field(strict=True, ge=0, le=9)] | None

    @field_validator("factor")
    @classmethod
    def _factor(cls, value: float) -> float:
        if value not in {0.5, 2.0}:
            message = "Harmonic factor must be one half or two"
            raise ValueError(message)
        return value


class EvaluatedOrdinalFit(BaseModel):
    """Declared winner and critical rivals after the bounded Native search."""

    model_config = ConfigDict(extra="forbid")

    outcome: Literal[
        "supported", "ambiguous", "conflicting_sections", "insufficient", "search_limited"
    ]
    evidence_tier: Literal["two_ten_second", "gapped_support", "short_source", "inconclusive"]
    candidate_limit_reached: bool = Field(strict=True)
    evaluated_candidate_count: Annotated[int, Field(strict=True, ge=0, le=4098)]
    winner: CandidateSummary | None
    assignments: Annotated[list[_Assignment], Field(max_length=250_000)]
    alternatives: Annotated[list[CandidateSummary], Field(max_length=10)]
    harmonics: Annotated[list[HarmonicCheck], Field(max_length=2)]
    probe_windows: Annotated[list[_Bounds], Field(max_length=2)]
    fractional_check: FractionalTempoCheck | None
    integer_check: FractionalTempoCheck | None
    integer_candidate_bpm: _Bpm | None
    integer_preferred: bool = Field(strict=True)
    conflicting_sections: Annotated[
        list[Annotated[int, Field(strict=True, ge=0, le=4)]], Field(max_length=5)
    ]


type OrdinalFit = Annotated[BudgetOrdinalFit | EvaluatedOrdinalFit, Field(discriminator="outcome")]

type _Point = tuple[int, int, float, float]


def _invalid(message: str) -> NoReturn:
    raise ValueError(message)


def _assign(beats: list[float], period: float, anchor: float, duration: float) -> list[_Point]:
    if any(
        not math.isfinite(q) or abs(q) > _EXACT_INTEGER
        for q in ((-anchor) / period, (duration - anchor) / period)
    ):
        _invalid("Ordinal source span exceeds its exact binary64 integer domain")
    tolerance = min(0.025, 0.05 * period)
    bins: dict[int, tuple[int, float, float, float]] = {}
    for raw_index, seconds in enumerate(beats):
        quotient = (seconds - anchor) / period
        if not math.isfinite(quotient) or abs(quotient) > _EXACT_INTEGER:
            _invalid("Ordinal assignment exceeds its exact binary64 integer domain")
        ordinal = round(quotient)
        center = anchor + period * ordinal
        residual = seconds - center
        if not 0.0 <= center < duration or abs(residual) > tolerance:
            continue
        previous = bins.get(ordinal)
        if previous is None or abs(residual) < abs(previous[2]):
            bins[ordinal] = raw_index, seconds, residual, center
    return [(i, k, t, center) for k, (i, t, _, center) in sorted(bins.items())]


def _pivot(points: list[_Point]) -> tuple[float, float, float | None]:
    if not points:
        return 0.0, 0.0, None
    mx = _sum(float(p[1]) for p in points) / len(points)
    my = _sum(p[2] for p in points) / len(points)
    xx = _sum((p[1] - mx) * (p[1] - mx) for p in points)
    if xx <= 0.0:
        return mx, my, None
    period = _sum((p[1] - mx) * (p[2] - my) for p in points) / xx
    return mx, my, period if 0.2 <= period <= 2.0 else None


def _span(
    beats: list[float],
    points: list[_Point],
    period: float,
    anchor: float,
    bounds: list[float],
) -> SpanDiagnostics:
    start, end = bounds
    local = [p for p in points if start <= p[2] < end and start <= p[3] < end]
    slots = math.ceil((end - anchor) / period) - math.ceil((start - anchor) / period)
    raw_count = bisect_left(beats, end) - bisect_left(beats, start)
    if not local:
        return SpanDiagnostics(
            window_seconds=bounds,
            raw_count=raw_count,
            expected_slots=slots,
            matched=0,
            observed_span_seconds=None,
            fitted_bpm=None,
            max_residual_seconds=None,
            rms_residual_seconds=None,
            max_gap_seconds=end - start,
        )
    _, _, fitted = _pivot(local)
    residuals = [p[2] - p[3] for p in local]
    gaps = [local[0][2] - start, end - local[-1][2]]
    gaps.extend(b[2] - a[2] for a, b in pairwise(local))
    return SpanDiagnostics(
        window_seconds=bounds,
        raw_count=raw_count,
        expected_slots=slots,
        matched=len(local),
        observed_span_seconds=local[-1][2] - local[0][2],
        fitted_bpm=None if fitted is None else 60.0 / fitted,
        max_residual_seconds=max(map(abs, residuals)),
        rms_residual_seconds=math.sqrt(_sum(r * r for r in residuals) / len(residuals)),
        max_gap_seconds=max(gaps),
    )


def _check_span(actual: SpanDiagnostics, expected: SpanDiagnostics) -> None:
    if actual.window_seconds != expected.window_seconds or (
        actual.raw_count,
        actual.expected_slots,
        actual.matched,
    ) != (expected.raw_count, expected.expected_slots, expected.matched):
        _invalid("Ordinal coverage differs from the fixed original source span")
    for key in (
        "observed_span_seconds",
        "fitted_bpm",
        "max_residual_seconds",
        "rms_residual_seconds",
        "max_gap_seconds",
    ):
        if not _close(getattr(actual, key), getattr(expected, key)):
            _invalid("Ordinal diagnostics differ from their original observations")


def _check_candidate(
    beats: list[float], candidate: CandidateSummary, duration: float
) -> list[_Point]:
    period, anchor = candidate.period_seconds, candidate.anchor_seconds
    points = _assign(beats, period, anchor, duration)
    if len(points) < 4 or points[0][1] != 0:
        _invalid("Ordinal candidate lacks its bounded full-fit observations")
    mx, my, fitted = _pivot(points)
    if not _close(candidate.pivot_ordinal, mx) or not _close(candidate.pivot_seconds, my):
        _invalid("Ordinal pivot differs from the complete declared assignments")
    if fitted is None or (
        not _close(candidate.period_seconds, fitted)
        or not _close(candidate.anchor_seconds, my - fitted * mx)
    ):
        _invalid("Ordinal assignment and free fit are not a consistent final model")
    reassigned = _assign(beats, fitted, my - fitted * mx, duration)
    if [(p[0], p[1]) for p in reassigned] != [(p[0], p[1]) for p in points]:
        _invalid("Recomputed ordinal fit changes its declared assignments")
    _check_span(candidate.global_span, _span(beats, points, period, anchor, [0.0, duration]))
    for index, section in enumerate(candidate.sections):
        bounds = [duration * index / 5.0, duration * (index + 1) / 5.0]
        _check_span(section, _span(beats, points, period, anchor, bounds))
    return points


def _band(span: SpanDiagnostics) -> tuple[float, float]:
    denominator = span.raw_count + span.expected_slots
    if denominator == 0:
        return 0.0, 0.0
    q = math.isqrt(span.matched)
    q += q * q != span.matched
    return (
        2.0 * max(0, span.matched - q) / denominator,
        2.0 * min(span.raw_count, span.expected_slots, span.matched + q) / denominator,
    )


def _sustained(span: SpanDiagnostics) -> bool:
    return (
        span.matched >= 4
        and span.observed_span_seconds is not None
        and span.observed_span_seconds
        >= min(10.0, 0.6 * (span.window_seconds[1] - span.window_seconds[0]))
    )


def _eligible(candidate: CandidateSummary, duration: float) -> bool:
    whole = candidate.global_span
    if duration < 30.0:
        return (
            whole.matched >= 4
            and whole.observed_span_seconds is not None
            and whole.observed_span_seconds >= max(3.0 * candidate.period_seconds, 0.6 * duration)
        )
    return whole.matched >= 8 and all(_sustained(s) for s in candidate.sections[1:4])


def _equivalent(a: CandidateSummary, b: CandidateSummary, duration: float) -> bool:
    if abs(a.period_seconds / b.period_seconds - 1.0) > 0.0001:
        return False
    tolerance = min(0.025, 0.05 * a.period_seconds, 0.05 * b.period_seconds)
    raw_winding = (a.anchor_seconds - b.anchor_seconds) / b.period_seconds
    drift = abs(a.period_seconds - b.period_seconds) * duration / a.period_seconds
    if (
        not math.isfinite(raw_winding)
        or abs(raw_winding) > _EXACT_INTEGER
        or not math.isfinite(drift)
        or drift > tolerance
    ):
        return False
    winding = round(raw_winding)
    phase = a.anchor_seconds - b.anchor_seconds - b.period_seconds * winding
    for t in (0.0, duration):
        k = (t - a.anchor_seconds) / a.period_seconds
        if abs(phase + (a.period_seconds - b.period_seconds) * k) > tolerance:
            return False
    return True


def _section_lattice_compatible(
    a: CandidateSummary, b: CandidateSummary, bounds: list[float]
) -> bool:
    """Compare fitted lattices with one winding over the complete fixed section."""
    fine, coarse = (a, b) if a.period_seconds <= b.period_seconds else (b, a)
    pf, pc = fine.period_seconds, coarse.period_seconds
    ratio = pc / pf
    if not math.isfinite(ratio):
        return False
    stride = round(ratio)
    if not 1 <= stride <= 10:
        return False
    step = pc - stride * pf
    phase = coarse.anchor_seconds - fine.anchor_seconds
    winding = (phase + (((bounds[0] + bounds[1]) * 0.5 - coarse.anchor_seconds) / pc) * step) / pf
    if not all(math.isfinite(v) for v in (step, phase, winding)) or abs(winding) > _EXACT_INTEGER:
        return False
    winding = round(winding)
    tolerance = min(0.025, 0.05 * pf, 0.05 * pc)
    for t in bounds:
        difference = phase - winding * pf + ((t - coarse.anchor_seconds) / pc) * step
        if not math.isfinite(difference) or abs(difference) > tolerance:
            return False
    return True


def _probes(
    beats: list[float], winner: CandidateSummary, points: list[_Point], duration: float
) -> list[list[float]]:
    if duration < 30.0:
        windows = [[0.0, duration / 2.0], [duration / 2.0, duration]]
        return (
            windows
            if all(
                _span(beats, points, winner.period_seconds, winner.anchor_seconds, window).matched
                >= 2
                for window in windows
            )
            else []
        )
    width = min(32.0, max(16.0, duration / 5.0))
    supported: list[list[float]] = []
    for section in range(5):
        center = (section + 0.5) * duration / 5.0
        window = [max(0.0, center - width / 2.0), min(duration, center + width / 2.0)]
        observed = _span(beats, points, winner.period_seconds, winner.anchor_seconds, window)
        if observed.matched >= 4 and (
            observed.observed_span_seconds is not None and observed.observed_span_seconds >= 10.0
        ):
            supported.append(window)
    pair: list[list[float]] = []
    gap = -1.0
    for side, left in enumerate(supported):
        for right in supported[side + 1 :]:
            if left[1] <= right[0] and right[0] - left[1] > gap:
                pair, gap = [left, right], right[0] - left[1]
    return pair


def _trial(
    points: list[_Point], winner: CandidateSummary, windows: list[list[float]], bpm: float
) -> FractionalTempoCheck | None:
    if len(points) < 2 or len(windows) != 2:
        return None
    mx, my, free = winner.pivot_ordinal, winner.pivot_seconds, winner.period_seconds
    period = 60.0 / bpm
    residuals = [p[2] - my - period * (p[1] - mx) for p in points]
    free_residuals = [p[2] - my - free * (p[1] - mx) for p in points]
    offsets = []
    for start, end in windows:
        local = [
            r
            for p, r in zip(points, residuals, strict=True)
            if start <= p[2] < end and start <= p[3] < end
        ]
        if not local:
            return None
        offsets.append(_sum(local) / len(local))
    maximum = max(map(abs, residuals))
    rms = math.sqrt(_sum(r * r for r in residuals) / len(points))
    free_rms = math.sqrt(_sum(r * r for r in free_residuals) / len(points))
    penalty, phase = max(0.0, rms - free_rms), abs(offsets[1] - offsets[0])
    passed = maximum <= min(0.025, 0.05 * period) and penalty <= 0.00025 and phase <= 0.005
    return FractionalTempoCheck(
        status="pass" if passed else "fail",
        max_residual_seconds=maximum,
        rms_residual_seconds=rms,
        rms_penalty_seconds=penalty,
        window_phase_difference_seconds=phase,
    )


def _check_trial(
    actual: FractionalTempoCheck | None, expected: FractionalTempoCheck | None
) -> None:
    if expected is None:
        if actual is not None:
            _invalid("Unsupported ordinal probe geometry cannot carry candidate metrics")
        return
    if actual is None or actual.status != expected.status:
        _invalid("Ordinal candidate status contradicts its shared-pivot check")
    for key in (
        "max_residual_seconds",
        "rms_residual_seconds",
        "rms_penalty_seconds",
        "window_phase_difference_seconds",
    ):
        if not _close(getattr(actual, key), getattr(expected, key)):
            _invalid("Ordinal candidate metrics differ from their one shared pivot")


def _fixed_summary(
    beats: list[float], points: list[_Point], period: float, anchor: float, duration: float
) -> CandidateSummary | None:
    mx, my, fitted = _pivot(points)
    if fitted != period:
        return None
    whole = _span(beats, points, period, anchor, [0.0, duration])
    sections = [
        _span(beats, points, period, anchor, [i * duration / 5.0, (i + 1) * duration / 5.0])
        for i in range(5)
    ]
    if any(s.matched > min(s.raw_count, s.expected_slots) for s in [whole, *sections]):
        return None
    return CandidateSummary.model_validate({
        "period_seconds": period,
        "anchor_seconds": anchor,
        "pivot_ordinal": mx,
        "pivot_seconds": my,
        "global": whole,
        "sections": sections,
    })


def _bounded_fit(
    beats: list[float], period: float, anchor: float, duration: float
) -> CandidateSummary | None:
    """Replay one declared harmonic start, never the Native discovery search."""
    try:
        points = _assign(beats, period, anchor, duration)
        for _ in range(6):
            if len(points) < 4:
                return None
            first = points[0][1]
            points = [(i, k - first, t, center) for i, k, t, center in points]
            mx, my, fitted = _pivot(points)
            if fitted is None:
                return None
            next_anchor = my - fitted * mx
            following = _assign(beats, fitted, next_anchor, duration)
            if [(p[0], p[1]) for p in following] == [(p[0], p[1]) for p in points]:
                return _fixed_summary(beats, following, fitted, next_anchor, duration)
            points = following
    except ValueError:
        # Invalid finite geometry or an unsupported line is a mathematical
        # refusal. MemoryError is not relabelled as a completed failed fit.
        return None
    return None


def _check_same_candidate(actual: CandidateSummary, expected: CandidateSummary) -> None:
    if not all(
        _close(getattr(actual, key), getattr(expected, key))
        for key in ("period_seconds", "anchor_seconds", "pivot_ordinal", "pivot_seconds")
    ):
        _invalid("Harmonic witness differs from its exact declared bounded start")
    _check_span(actual.global_span, expected.global_span)
    for a, b in zip(actual.sections, expected.sections, strict=True):
        _check_span(a, b)


def _validate_harmonics(
    method: TempoMathView, fit: EvaluatedOrdinalFit, winner: CandidateSummary
) -> None:
    if [h.factor for h in fit.harmonics] != [0.5, 2.0]:
        _invalid("Ordinal evidence must declare both fixed harmonic starts")
    attempts = 0
    for harmonic in fit.harmonics:
        period = winner.period_seconds * harmonic.factor
        if (
            harmonic.seed_period_seconds != period
            or harmonic.seed_anchor_seconds != winner.anchor_seconds
        ):
            _invalid("Harmonic seed differs from its one declared source reference")
        if not 0.2 <= period <= 2.0:
            if harmonic.outcome != "out_of_range" or harmonic.alternative_index is not None:
                _invalid("Out-of-range harmonic cannot carry a fitted witness")
            continue
        attempts += 1
        expected = _bounded_fit(
            method.raw_beats,
            period,
            winner.anchor_seconds,
            method.frame_count / method.sample_rate_hz,
        )
        if expected is None:
            if harmonic.outcome != "no_fixed_fit" or harmonic.alternative_index is not None:
                _invalid("Harmonic outcome differs from its bounded fixed-fit refusal")
        else:
            index = harmonic.alternative_index
            if harmonic.outcome != "fitted" or index is None or index >= len(fit.alternatives):
                _invalid("A completed harmonic fit requires its retained scalar witness")
            _check_same_candidate(fit.alternatives[index], expected)
    if fit.evaluated_candidate_count < attempts + 1:
        _invalid("Ordinal candidate count omits its mandatory bounded harmonic attempts")


def _validate_budget(method: TempoMathView, fit: BudgetOrdinalFit) -> None:
    required = 1024 * len(method.raw_beats) + 512 * len(method.raw_downbeats) + 8 * 1024 * 1024
    if fit.required_bytes != required or fit.available_bytes > 512 * 1024 * 1024:
        _invalid("Unavailable ordinal preparation differs from its bounded source reserve")
    over_budget = fit.required_bytes > fit.available_bytes
    if (fit.outcome == "budget_unavailable") != over_budget:
        _invalid("Ordinal preparation outcome contradicts its admitted reserve")
    if method.bpm is not None or method.status not in {"insufficient", "uncorroborated"}:
        _invalid("Unavailable ordinal preparation cannot carry a selected tempo")


def _validate_empty(fit: EvaluatedOrdinalFit) -> None:
    collections = (
        fit.assignments,
        fit.alternatives,
        fit.harmonics,
        fit.probe_windows,
        fit.conflicting_sections,
    )
    checks = (fit.fractional_check, fit.integer_check, fit.integer_candidate_bpm)
    if (
        any(collections)
        or any(check is not None for check in checks)
        or fit.integer_preferred
        or fit.evidence_tier != "inconclusive"
    ):
        _invalid("Missing ordinal winner cannot carry candidate evidence")
    expected = "search_limited" if fit.candidate_limit_reached else "insufficient"
    if fit.outcome != expected:
        _invalid("Missing ordinal winner has an incoherent search outcome")


def _validate_checks(
    method: TempoMathView,
    fit: EvaluatedOrdinalFit,
    winner: CandidateSummary,
    points: list[_Point],
    *,
    phase_only: bool = False,
) -> tuple[bool, float]:
    duration = method.frame_count / method.sample_rate_hz
    windows = _probes(method.raw_beats, winner, points, duration)
    if fit.probe_windows != windows:
        _invalid("Ordinal probes differ from the deterministic complete-source windows")
    base = 60.0 / winner.period_seconds
    integer = min(300.0, max(30.0, math.floor(base + 0.5)))
    if fit.integer_candidate_bpm != integer:
        _invalid("Ordinal integer hypothesis differs from the nearest positive integer BPM")
    fractional = _trial(points, winner, windows, base)
    integer_check = _trial(points, winner, windows, integer)
    _check_trial(fit.fractional_check, fractional)
    _check_trial(fit.integer_check, integer_check)
    if phase_only:
        _require_phase_only(fractional, base)
    passed = fractional is not None and fractional.status == "pass"
    preferred = (
        passed
        and integer_check is not None
        and integer_check.status == "pass"
        and abs(integer - base) <= 0.005
    )
    if fit.integer_preferred != preferred:
        _invalid("Ordinal integer preference contradicts the same-mask candidate checks")
    tier = "inconclusive"
    if len(windows) == 2:
        tier = (
            "short_source"
            if duration < 30.0
            else (
                "gapped_support"
                if winner.global_span.max_gap_seconds > max(10.0, 8.0 * winner.period_seconds)
                else "two_ten_second"
            )
        )
    if fit.evidence_tier != tier:
        _invalid("Ordinal evidence tier differs from its actual source coverage")
    return passed, integer if preferred else base


def _require_phase_only(check: FractionalTempoCheck | None, bpm: float) -> None:
    if check is None or check.status != "fail":
        _invalid("Acoustic corroboration requires an actual failed neural fractional check")
    maximum = check.max_residual_seconds
    penalty = check.rms_penalty_seconds
    phase = check.window_phase_difference_seconds
    if maximum is None or penalty is None or phase is None:
        _invalid("Acoustic corroboration requires complete neural candidate checks")
    if maximum > min(0.025, 0.05 * (60.0 / bpm)) or penalty > 0.00025 or phase <= 0.005:
        _invalid("Acoustic corroboration requires a phase-only neural refusal")


def _validate_rivals(fit: EvaluatedOrdinalFit, winner: CandidateSummary, duration: float) -> bool:
    incompatible: list[CandidateSummary] = []
    for index, candidate in enumerate(fit.alternatives):
        if candidate == winner or candidate in fit.alternatives[:index]:
            _invalid("Critical ordinal alternatives must be distinct scalar summaries")
        if not _equivalent(candidate, winner, duration):
            incompatible.append(candidate)
    conflicts = [
        section
        for section in range(5)
        if any(
            _sustained(c.sections[section])
            and not _section_lattice_compatible(c, winner, winner.sections[section].window_seconds)
            and _band(c.sections[section])[0] > _band(winner.sections[section])[1]
            for c in incompatible
        )
    ]
    if fit.conflicting_sections != conflicts:
        _invalid("Ordinal section outcome omitted or invented a declared sustained contradiction")
    return any(
        _sustained(c.global_span) and _band(c.global_span)[1] >= _band(winner.global_span)[0]
        for c in incompatible
    )


def _validate_evaluated(
    method: TempoMathView, fit: EvaluatedOrdinalFit, *, phase_only: bool = False
) -> None:
    if fit.winner is None:
        if phase_only:
            _invalid("Acoustic corroboration requires an eligible neural winner")
        _validate_empty(fit)
        _validate_result(method, None)
        return
    duration = method.frame_count / method.sample_rate_hz
    winner = fit.winner
    if phase_only and (
        duration < 30.0
        or fit.outcome != "insufficient"
        or fit.evidence_tier != "two_ten_second"
        or not _eligible(winner, duration)
    ):
        _invalid("Acoustic corroboration requires normal long-source neural support")
    if fit.evaluated_candidate_count < len(fit.alternatives) + 1:
        _invalid("Ordinal candidate count is smaller than its distinct declared summaries")
    points = _check_candidate(method.raw_beats, winner, duration)
    if fit.assignments != [[p[0], p[1]] for p in points]:
        _invalid("Ordinal assignments differ from the unchanged original detections")
    for candidate in fit.alternatives:
        _check_candidate(method.raw_beats, candidate, duration)
    _validate_harmonics(method, fit, winner)
    passed, selected_bpm = _validate_checks(method, fit, winner, points, phase_only=phase_only)
    ambiguous = _validate_rivals(fit, winner, duration)
    outcome = _outcome(fit, passed=passed and _eligible(winner, duration), ambiguous=ambiguous)
    if fit.outcome != outcome:
        _invalid("Ordinal outcome contradicts its declared coverage and critical alternatives")
    _validate_result(method, selected_bpm if outcome == "supported" else None)


def _outcome(fit: EvaluatedOrdinalFit, *, passed: bool, ambiguous: bool) -> str:
    if fit.candidate_limit_reached:
        return "search_limited"
    if fit.conflicting_sections:
        return "conflicting_sections"
    if ambiguous:
        return "ambiguous"
    return "supported" if passed else "insufficient"


def _validate_result(method: TempoMathView, expected_bpm: float | None) -> None:
    expected_status = (
        "single_region_estimate"
        if method.frame_count / method.sample_rate_hz < 30.0
        else "estimated"
    )
    if expected_bpm is not None:
        if method.bpm != expected_bpm or method.status != expected_status:
            _invalid("Selected ordinal BPM contradicts its exact candidate evidence")
    elif method.bpm is not None or method.status not in {"insufficient", "uncorroborated"}:
        _invalid("Unavailable ordinal evidence cannot carry a selected BPM")


def validate_ordinal_fit(method: TempoMathView, *, phase_only: bool = False) -> None:
    """Replay declared bounded evidence, not Native discovery or global uniqueness."""
    if method.selected is not None or method.integer_probe is not None:
        _invalid("Inferred ordinals cannot impersonate original-index regional evidence")
    fit = method.ordinal_fit
    if isinstance(fit, BudgetOrdinalFit):
        if phase_only:
            _invalid("Acoustic corroboration cannot bypass unavailable neural preparation")
        _validate_budget(method, fit)
    elif isinstance(fit, EvaluatedOrdinalFit):
        _validate_evaluated(method, fit, phase_only=phase_only)
    else:
        _invalid("Version five requires its explicit inferred-ordinal evidence")
