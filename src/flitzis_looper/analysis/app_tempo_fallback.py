"""Replay an approximate interior mean without claiming constant-tempo precision."""

import math
from statistics import median
from typing import TYPE_CHECKING, Annotated, Literal, NoReturn

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

from flitzis_looper.analysis.app_tempo_corroboration import (
    CorroborationSource,
    QmMathView,
    QmObservation,
    validate_qm_source_extent,
)
from flitzis_looper.analysis.app_tempo_inferred import EvaluatedOrdinalFit, OrdinalFit
from flitzis_looper.analysis.app_tempo_probe import (
    AdaptiveTempoProbe,
    IntegerTempoProbe,
    TempoRegion,
    _close,
    _pivot,
    _points,
    _strict_common,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
    from flitzis_looper.analysis.app_tempo_probe import TempoMathView

_Bpm = Annotated[float, Field(strict=True, allow_inf_nan=False, ge=30, le=300)]
_Time = Annotated[float, Field(strict=True, allow_inf_nan=False, ge=0)]


def _invalid(message: str) -> NoReturn:
    raise ValueError(message)


class FallbackNeuralResult(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    schema_version: Literal[9]
    backend: Literal["beat_this"]
    policy: Literal["neural-inferred-periods-v9"]
    status: Literal["uncorroborated", "insufficient"]
    bpm: None

    @field_validator("schema_version", mode="before")
    @classmethod
    def _strict_version(cls, value: object) -> object:
        if type(value) is not int:
            _invalid("Fallback original version must be an integer")
        return value


class CompletedQmEstimate(BaseModel):
    """Completed own mathematics, including an unchanged strict refusal."""

    model_config = ConfigDict(extra="forbid", strict=True)

    kernel_version: Literal[4, 9]
    status: Literal[
        "estimated", "single_region_estimate", "insufficient", "invalid", "uncorroborated"
    ]
    bpm: _Bpm | None
    selected: TempoRegion | None
    regions: Annotated[list[TempoRegion], Field(max_length=5)]
    integer_probe: IntegerTempoProbe | AdaptiveTempoProbe | None = Field(
        default=None, exclude_if=lambda value: value is None
    )
    ordinal_fit: OrdinalFit | None = Field(default=None, exclude_if=lambda value: value is None)

    @field_validator("kernel_version", mode="before")
    @classmethod
    def _strict_version(cls, value: object) -> object:
        if type(value) is not int:
            _invalid("Completed QM kernel version must be an integer")
        return value

    @model_validator(mode="after")
    def _kernel_shape(self) -> CompletedQmEstimate:
        if (self.kernel_version == 9) != (self.ordinal_fit is not None):
            _invalid("Completed QM evidence differs from its numeric kernel")
        return self


class CompletedQmEvidence(QmObservation):
    estimate: CompletedQmEstimate


class MeanRegion(TempoRegion):
    model_config = ConfigDict(extra="forbid", strict=True)

    retained: Annotated[int, Field(strict=True, ge=24, le=250_000)]
    excluded_raw_indices: Annotated[
        list[Annotated[int, Field(strict=True, ge=0, le=249_999)]], Field(max_length=250_000)
    ]


class InteriorMean(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    window_seconds: Annotated[list[_Time], Field(min_length=2, max_length=2)]
    neural_regions: Annotated[list[MeanRegion], Field(min_length=3, max_length=3)]
    qm_regions: Annotated[list[MeanRegion], Field(min_length=3, max_length=3)]
    neural_mean_bpm: _Bpm
    qm_mean_bpm: _Bpm
    qm_bpm_factor: Annotated[float, Field(strict=True, allow_inf_nan=False)]
    adjusted_qm_mean_bpm: _Bpm
    maximum_period_ratio: Annotated[float, Field(strict=True, allow_inf_nan=False, ge=1, le=1.01)]

    @field_validator("qm_bpm_factor")
    @classmethod
    def _global_factor(cls, value: float) -> float:
        if value not in {0.5, 1.0, 2.0}:
            _invalid("Interior mean requires one global half/double QM factor")
        return value


class FallbackRangeConstraint(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    minimum_bpm: _Bpm
    maximum_bpm: _Bpm

    @model_validator(mode="after")
    def _ordered(self) -> FallbackRangeConstraint:
        if self.minimum_bpm >= self.maximum_bpm:
            _invalid("Fallback request range bounds must be ordered")
        return self


class TempoFallback(BaseModel):
    model_config = ConfigDict(extra="forbid")

    kind: Literal["precise_qm", "mean_estimate"]
    neural_result: FallbackNeuralResult
    source: CorroborationSource
    qm: CompletedQmEvidence
    range_constraint: FallbackRangeConstraint | None = Field(
        default=None, exclude_if=lambda value: value is None
    )
    mean: InteriorMean | None = Field(default=None, exclude_if=lambda value: value is None)

    @field_validator("range_constraint", "mean", mode="before")
    @classmethod
    def _omit_inapplicable(cls, value: object) -> object:
        if value is None:
            _invalid("Inapplicable fallback fields must be omitted")
        return value

    @model_validator(mode="after")
    def _kind_shape(self) -> TempoFallback:
        if (self.kind == "mean_estimate") != (self.mean is not None):
            _invalid("Fallback kind differs from its interior mean evidence")
        return self


def _require_conflict(method: TempoMathView) -> None:
    fit = method.ordinal_fit
    if (
        method.schema_version != 9
        or method.bpm is not None
        or not isinstance(fit, EvaluatedOrdinalFit)
    ):
        _invalid("Last fallback requires a completed unchanged strict period conflict")
    if (
        fit.outcome != "conflicting_sections"
        or fit.candidate_limit_reached
        or fit.period_selection is None
    ):
        _invalid("Last fallback cannot bypass unavailable or incomplete period work")
    if any(trial.candidate_limit_reached for trial in fit.period_selection.trials):
        _invalid("Last fallback cannot bypass an incomplete performed trial")


def _qm_view(qm: CompletedQmEvidence) -> QmMathView:
    step = qm.odf_hop_samples / qm.analyzer.sample_rate_hz
    beats = [frame * step for frame in qm.raw_beat_frames]
    estimate = qm.estimate
    return QmMathView(
        estimate.kernel_version,
        estimate.status,
        estimate.bpm,
        estimate.selected,
        estimate.regions,
        estimate.integer_probe,
        estimate.ordinal_fit,
        qm.analyzer.sample_rate_hz,
        qm.analyzer.frame_count,
        beats,
        [beats[index] for index in qm.raw_downbeat_indices],
    )


def validate_tempo_fallback(
    method: AppTempoMethod, validate_math: Callable[[TempoMathView], None]
) -> None:
    evidence = method.tempo_fallback
    if evidence is None:
        _invalid("Version ten requires explicit last-fallback evidence")
    header = evidence.neural_result
    primary = method.model_copy(
        update={
            "schema_version": header.schema_version,
            "backend": header.backend,
            "policy": header.policy,
            "status": header.status,
            "bpm": None,
            "tempo_fallback": None,
        }
    )
    primary._validate_policy_version()
    validate_math(primary)
    _require_conflict(primary)
    validate_qm_source_extent(method, evidence.source, evidence.qm)
    qm = _qm_view(evidence.qm)
    validate_math(qm)
    _validate_selection(method, evidence, qm)


def _validate_selection(method: AppTempoMethod, evidence: TempoFallback, qm: QmMathView) -> None:
    if evidence.kind == "precise_qm":
        if (
            qm.bpm is None
            or qm.status not in {"estimated", "single_region_estimate"}
            or method.bpm != qm.bpm
            or method.status != qm.status
        ):
            _invalid("Precise fallback must retain its independently admitted QM scalar/status")
    else:
        _require_conflict(qm)
        mean = evidence.mean
        if mean is None:
            _invalid("Mean fallback requires all six fresh local fits")
        _validate_mean(method, qm, mean)
        if method.status != "mean_estimate" or method.bpm != mean.neural_mean_bpm:
            _invalid("Mean fallback must select its exact unrounded neural mean")
    bounds = evidence.range_constraint
    if bounds is not None and (
        method.bpm is None or not bounds.minimum_bpm <= method.bpm <= bounds.maximum_bpm
    ):
        _invalid("Fallback scalar lies outside its captured request range")


def _next_local_mask(points: list[tuple[int, float]], keep: list[bool]) -> list[bool]:
    retained = [point for point, yes in zip(points, keep, strict=True) if yes]
    mx, my, period = _pivot(retained)
    residuals = [t - my - period * (i - mx) for i, t in points]
    residual_keep = [r for r, yes in zip(residuals, keep, strict=True) if yes]
    center = median(residual_keep)
    deviations = [abs(r - center) for r in residual_keep]
    limit = min(0.025, 0.05 * period)
    threshold = min(limit, max(0.010, 3.5 * 1.4826 * median(deviations)))
    return [abs(r - center) <= threshold for r in residuals]


def _fresh_mask(method: TempoMathView, bounds: list[float]) -> list[int]:
    """Replay only the existing five local median/MAD iterations, never discovery."""
    lower, upper = bounds
    points = [(i, t) for i, t in enumerate(method.raw_beats) if lower <= t < upper]
    if len(points) < 24:
        _invalid("Interior mean window lacks local observations")
    keep = [True] * len(points)
    for _ in range(5):
        next_keep = _next_local_mask(points, keep)
        if next_keep == keep:
            break
        keep = next_keep
    return [point[0] for point, yes in zip(points, keep, strict=True) if not yes]


def _validate_regions(
    method: TempoMathView, regions: list[MeanRegion], duration: float
) -> tuple[float, list[float]]:
    numerator, denominator = 0.0, 0.0
    fitted_bpms = []
    for i, region in enumerate(regions, start=1):
        bounds = [(i * duration) / 5.0, ((i + 1) * duration) / 5.0]
        if region.window_seconds != bounds:
            _invalid("Interior mean regions must retain their fixed source fifths")
        if region.excluded_raw_indices != _fresh_mask(method, bounds):
            _invalid("Interior mean mask differs from its fresh original-index local fit")
        points = _points(method, region)
        if region.retained != len(points):
            _invalid("Interior mean retention differs from its original observations")
        _strict_common(method, region, points, (24, 30.0))
        fitted_bpms.append(60.0 / _pivot(points)[2])
        width = bounds[1] - bounds[0]
        numerator += width * region.bpm
        denominator += width
    return numerator / denominator, fitted_bpms


def mean_period_ratio(neural: list[float], qm: list[float], factor: float) -> float | None:
    """One global factor and the direct inclusive ratio boundary, without subtraction."""
    adjusted = [factor * bpm for bpm in qm]
    if any(not math.isfinite(bpm) or not 30.0 <= bpm <= 300.0 for bpm in [*neural, *adjusted]):
        return None
    periods = [60.0 / bpm for bpm in neural] + [60.0 / bpm for bpm in adjusted]
    ratio = max(periods) / min(periods)
    return ratio if math.isfinite(ratio) and ratio <= 1.01 else None


def _validate_mean(method: AppTempoMethod, qm: QmMathView, mean: InteriorMean) -> None:
    duration = method.frame_count / method.sample_rate_hz
    if mean.window_seconds != [duration / 5.0, (4.0 * duration) / 5.0]:
        _invalid("Mean evidence must describe the middle sixty percent of its source")
    neural_mean, neural_bpms = _validate_regions(method, mean.neural_regions, duration)
    qm_mean, qm_bpms = _validate_regions(qm, mean.qm_regions, duration)
    if mean.neural_mean_bpm != neural_mean or mean.qm_mean_bpm != qm_mean:
        _invalid("Mean scalar differs from its elapsed-window weighted local fits")
    accepted = [
        (factor, ratio)
        for factor in (0.5, 1.0, 2.0)
        if (
            ratio := mean_period_ratio(
                neural_bpms,
                qm_bpms,
                factor,
            )
        )
        is not None
    ]
    if len(accepted) != 1 or accepted[0][0] != mean.qm_bpm_factor:
        _invalid("Mean evidence lacks a unique range-independent global QM factor")
    if (
        not _close(mean.maximum_period_ratio, accepted[0][1])
        or mean.adjusted_qm_mean_bpm != mean.qm_bpm_factor * mean.qm_mean_bpm
    ):
        _invalid("Mean agreement differs from its six local periods and global factor")
