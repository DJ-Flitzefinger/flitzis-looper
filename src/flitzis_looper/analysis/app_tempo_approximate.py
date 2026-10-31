"""Replay scalar-only approximate tempo evidence; never certify beat phase."""

import math
from itertools import pairwise
from operator import itemgetter
from statistics import median
from typing import TYPE_CHECKING, Annotated, Literal, NoReturn

from pydantic import BaseModel, ConfigDict, Field

from flitzis_looper.analysis.app_tempo_fallback import FallbackRangeConstraint
from flitzis_looper.analysis.app_tempo_inferred import EvaluatedOrdinalFit

if TYPE_CHECKING:
    from collections.abc import Callable, Iterable

    from flitzis_looper.analysis.app_tempo_models import AppTempoMethod


class PreciseHeader(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    schema_version: Literal[4, 9]
    backend: Literal["beat_this"]
    policy: Literal["neural-stable-regions-v4", "neural-inferred-periods-v9"]
    status: Literal["insufficient", "uncorroborated", "invalid"]
    bpm: None


class EnergyWindow(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    start_frame: Annotated[int, Field(ge=0)]
    mean_level: Annotated[float, Field(allow_inf_nan=False, ge=0)]
    novelty: Annotated[
        list[Annotated[float, Field(allow_inf_nan=False, ge=0)]],
        Field(min_length=800, max_length=2000),
    ]


class ApproximateTempo(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    kind: Literal["neural_period", "neural_intervals", "source_energy"]
    original: PreciseHeader | None
    precise_rejection: Annotated[str, Field(min_length=1, max_length=512)]
    period_seconds: Annotated[float, Field(allow_inf_nan=False, ge=0.2, le=2)]
    range_constraint: FallbackRangeConstraint | None
    energy_windows: Annotated[list[EnergyWindow], Field(max_length=3)]


def _invalid(message: str) -> NoReturn:
    raise ValueError(message)


def interval_period(beats: list[float]) -> float | None:
    """Bounded median with repeated interval support, independent of request range."""
    intervals = [b - a for a, b in pairwise(beats) if 0.2 <= b - a <= 2.0]
    if len(intervals) < 8 or len(intervals) * 2 < len(beats) - 1:
        return None
    period = median(intervals)
    support = sum(abs(value - period) <= 0.15 * period for value in intervals)
    return period if support * 5 >= len(intervals) * 3 else None


def _ordered_sum(values: Iterable[float]) -> float:
    """Match Rust's sequential binary64 accumulation, including near ties."""
    total = 0.0
    for value in values:
        total += value
    return total


def _correlation(values: list[float], lag: int) -> float:
    cross = left = right = 0.0
    for i in range(lag, len(values)):
        cross += values[i] * values[i - lag]
        left += values[i] * values[i]
        right += values[i - lag] * values[i - lag]
    return cross / math.sqrt(left * right) if left * right > 0 else 0.0


def _energy_candidate(window: EnergyWindow) -> tuple[float, int] | None:
    raw = window.novelty
    peak = max(raw)
    if peak <= 1e-7 or sum(value > peak * 0.2 for value in raw) < 4:
        return None
    smooth = [
        _ordered_sum(
            raw[j] * weight
            for j, weight in zip(range(i - 4, i + 5), [1, 2, 3, 4, 5, 4, 3, 2, 1], strict=True)
            if 0 <= j < len(raw)
        )
        / 25.0
        for i in range(len(raw))
    ]
    if max(smooth) < 0.1 * window.mean_level:
        return None
    center = _ordered_sum(smooth) / len(smooth)
    values = [value - center for value in smooth]
    scores = [_correlation(values, lag) for lag in range(20, min(200, len(values) // 4) + 1)]
    best = max(scores)
    return next(
        (
            (score, index + 20)
            for index, score in enumerate(scores)
            if score >= max(0.35, best * 0.95)
            and (index == 0 or score >= scores[index - 1])
            and (index + 1 == len(scores) or score >= scores[index + 1])
        ),
        None,
    )


def energy_period(windows: list[EnergyWindow], rate: int) -> float | None:
    """Replay 100 Hz novelty autocorrelation, with no default-tempo prior."""
    candidates = [
        candidate for window in windows if (candidate := _energy_candidate(window)) is not None
    ]
    if not candidates:
        return None
    # Strongest actual window, deterministic first-window tie breaking.
    return max(candidates, key=itemgetter(0))[1] * (rate // 100) / rate


def _neural_choice(method: AppTempoMethod, version: int) -> tuple[str, float] | None:
    fit = method.ordinal_fit
    if (
        version == 9
        and isinstance(fit, EvaluatedOrdinalFit)
        and fit.winner is not None
        and _usable_period(fit)
    ):
        return "neural_period", fit.winner.period_seconds
    period = interval_period(method.raw_beats)
    return ("neural_intervals", period) if period is not None else None


def _usable_period(fit: EvaluatedOrdinalFit) -> bool:
    return (
        not fit.candidate_limit_reached
        and fit.evidence_tier != "inconclusive"
        and len(fit.assignments) >= 8
    )


def _source_energy_period(method: AppTempoMethod, evidence: ApproximateTempo) -> float | None:
    hop = method.sample_rate_hz // 100
    width = min(2000, method.frame_count // hop)
    available = method.frame_count - width * hop
    starts = list(dict.fromkeys([0, available // 2, available]))
    if [window.start_frame for window in evidence.energy_windows] != starts or any(
        len(window.novelty) != width for window in evidence.energy_windows
    ):
        _invalid("Energy evidence differs from its fixed bounded source windows")
    return energy_period(evidence.energy_windows, method.sample_rate_hz)


def validate_approximate(
    method: AppTempoMethod, validate_original: Callable[[AppTempoMethod], None]
) -> None:
    """Replay the unchanged refusal and the selected coarse scalar separately."""
    evidence = method.approximate_tempo
    if evidence is None or method.status != "approximate_estimate":
        _invalid("Approximate tempo requires its explicit scalar-only evidence")
    header = evidence.original
    if header is not None:
        original = method.model_copy(update=header.model_dump() | {"approximate_tempo": None})
        validate_original(original)
    elif evidence.kind != "source_energy" or any((
        method.execution,
        method.raw_beats,
        method.raw_downbeats,
        method.ordinal_fit,
        method.integer_probe,
        method.regions,
        method.selected,
    )):
        _invalid("Unavailable inference cannot invent neural observations or execution")
    period: float | None
    preferred = _neural_choice(method, header.schema_version) if header is not None else None
    if preferred is not None:
        if evidence.kind != preferred[0]:
            _invalid("Approximate evidence must retain the preferred existing neural source")
        period = preferred[1]
    elif evidence.kind == "source_energy":
        period = _source_energy_period(method, evidence)
    else:
        period = None
    if evidence.kind != "source_energy" and evidence.energy_windows:
        _invalid("Neural approximate evidence cannot carry source-energy windows")
    bpm = method.bpm
    if period is None or bpm is None or evidence.period_seconds != period or bpm != 60.0 / period:
        _invalid("Approximate scalar differs from its source evidence")
    bounds = evidence.range_constraint
    if bounds is not None and not bounds.minimum_bpm <= bpm <= bounds.maximum_bpm:
        _invalid("Approximate BPM lies outside its captured request range")


def from_saved(
    method: AppTempoMethod, bounds: FallbackRangeConstraint | None = None
) -> AppTempoMethod | None:
    """Reuse validated failed source evidence without scheduling another inference."""
    if method.schema_version not in {4, 9} or method.bpm is not None:
        return None
    if bounds is not None:
        bounds = FallbackRangeConstraint.model_validate(bounds, strict=True)
    choice = _neural_choice(method, method.schema_version)
    if choice is None:
        return None
    kind, period = choice
    if bounds is not None and not bounds.minimum_bpm <= 60.0 / period <= bounds.maximum_bpm:
        return None
    data = method.model_dump(mode="python")
    original = {
        name: data[name] for name in ("schema_version", "backend", "policy", "status", "bpm")
    }
    data.update(
        schema_version=11,
        backend="source_approximate",
        policy="source-approximate-tempo-v11",
        status="approximate_estimate",
        bpm=60.0 / period,
        approximate_tempo={
            "kind": kind,
            "original": original,
            "precise_rejection": f"Saved precise estimate {method.status}",
            "period_seconds": period,
            "range_constraint": bounds,
            "energy_windows": [],
        },
    )
    return type(method).model_validate(data, strict=True)
