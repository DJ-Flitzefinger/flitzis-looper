"""Frozen numerical BPM metadata; none of these records authorizes native timing."""

from dataclasses import asdict, dataclass
from typing import TYPE_CHECKING, Annotated, Literal

from pydantic import ConfigDict, Field, FiniteFloat

if TYPE_CHECKING:
    from flitzis_looper.analysis.contracts import (
        AnalysisIdentity,
        BeatModelIdentity,
        BeatPredictions,
    )

type PositionIndex = Annotated[int, Field(strict=True, ge=0, lt=250000)]
type PositionCount = Annotated[int, Field(strict=True, ge=0, le=250000)]
type PositiveFloat = Annotated[FiniteFloat, Field(gt=0)]
type NonnegativeFloat = Annotated[FiniteFloat, Field(ge=0)]
type NumericalStatus = Literal["unverified", "unsupported"]


class _StrictMetadata:
    __slots__ = ()
    __pydantic_config__ = ConfigDict(extra="forbid")


@dataclass(frozen=True, slots=True)
class BpmFit(_StrictMetadata):
    """Centered OLS estimate and conditional sensitivity, with a diagnostic intercept."""

    assigned_observations: PositionCount
    period_seconds_per_quarter: PositiveFloat
    bpm: PositiveFloat
    reference_count_numerator: int
    quarter_note_denominator: Annotated[int, Field(strict=True, ge=1, le=64)]
    fitted_seconds_at_reference: FiniteFloat
    diagnostic_intercept_seconds: FiniteFloat
    period_sensitivity_bound_seconds: NonnegativeFloat
    max_abs_residual_seconds: NonnegativeFloat
    residual_range_seconds: NonnegativeFloat
    numerical_tolerance_seconds: NonnegativeFloat


@dataclass(frozen=True, slots=True)
class GlobalBpmFit(_StrictMetadata):
    """Unchanged G2 robust fit fields; quantization is not an acoustic error bound."""

    period_seconds_per_quarter: PositiveFloat
    bpm: PositiveFloat
    reference_count_numerator: int
    quarter_note_denominator: Annotated[int, Field(strict=True, ge=1, le=64)]
    fitted_seconds_at_reference: FiniteFloat
    diagnostic_intercept_seconds: FiniteFloat
    period_sensitivity_bound_seconds: NonnegativeFloat
    max_abs_inlier_residual_seconds: NonnegativeFloat
    inlier_residual_range_seconds: NonnegativeFloat
    window_period_spread_seconds: NonnegativeFloat
    numerical_tolerance_seconds: NonnegativeFloat


@dataclass(frozen=True, slots=True)
class BpmWindow(_StrictMetadata):
    """A distant complete-source third, including its unavailable evidence."""

    start_seconds: NonnegativeFloat
    end_seconds: PositiveFloat
    assigned_positions: PositionCount
    global_inlier_positions: PositionCount
    inlier_positions: PositionCount
    period_seconds_per_quarter: PositiveFloat | None
    period_sensitivity_bound_seconds: NonnegativeFloat | None
    median_global_residual_seconds: FiniteFloat | None


@dataclass(frozen=True, slots=True)
class BpmRegion(_StrictMetadata):
    """One predeclared source window, preserving absolute raw indices and scope."""

    id: Literal["middle", "early", "central", "late"]
    start_seconds: NonnegativeFloat
    end_seconds: PositiveFloat
    assigned_positions: PositionCount
    raw_positions: PositionCount
    inlier_raw_indices: tuple[PositionIndex, ...]
    excluded_raw_indices: tuple[PositionIndex, ...]
    status: NumericalStatus
    reasons: tuple[str, ...]
    fit: BpmFit | None


@dataclass(frozen=True, slots=True)
class BpmAlternatives(_StrictMetadata):
    """Explicit complete-estimate half/double interpretations, never a model vote."""

    half: PositiveFloat | None
    ordinal: PositiveFloat | None
    double: PositiveFloat | None


@dataclass(frozen=True, slots=True)
class SelectedBpmAssessment(_StrictMetadata):
    """Full sequence and separate regional metadata under frozen numerical policies."""

    schema_version: Literal[1]
    policy_version: Literal["selected-backend-bpm-v1"]
    region_policy_version: Literal["representative-middle-region-v1"]
    global_policy_version: Literal["constant-period-candidate-v1"]
    beat_unit: Literal["quarter-note-assumption", "explicit-quarter-note-count-assertion"]
    count_provenance: Annotated[str, Field(min_length=1, max_length=4096)]
    timing_halfwidth_seconds: Annotated[FiniteFloat, Field(ge=0.01, le=0.01)]
    uncertainty: Literal["detector-lattice-only; musical timing/count unverified"]
    sample_rate_hz: Annotated[int, Field(strict=True, ge=8000, le=768000)]
    frame_count: Annotated[int, Field(strict=True, ge=1, le=2**53)]
    raw_position_count: PositionCount
    raw_beat_seconds: tuple[FiniteFloat, ...]
    quarter_counts: tuple[int | None, ...]
    quarter_note_denominator: Annotated[int, Field(strict=True, ge=1, le=64)]
    local_interval_seconds: tuple[PositiveFloat, ...]
    local_interval_bpm: tuple[PositiveFloat | None, ...]
    complete_fit: BpmFit | None
    complete_residual_seconds: tuple[FiniteFloat | None, ...]
    global_status: NumericalStatus
    global_reasons: tuple[str, ...]
    global_fit: GlobalBpmFit | None
    global_inlier_raw_indices: tuple[PositionIndex, ...]
    global_excluded_raw_indices: tuple[PositionIndex, ...]
    global_residual_seconds: tuple[FiniteFloat | None, ...]
    global_windows: tuple[BpmWindow, ...]
    regions: tuple[BpmRegion, ...]
    selected_region_id: Literal["middle", "early", "central", "late"] | None
    representative_bpm: PositiveFloat | None
    alternatives_bpm: BpmAlternatives


@dataclass(frozen=True, slots=True)
class SelectedBpmReport:
    """Request-bound metadata beside all four immutable arrays, without timing authority.

    Identity is the validated request token, not an independently sealed source hash.
    The existing native admission/publication owner supplies freshness separately.
    """

    identity: AnalysisIdentity
    model: BeatModelIdentity
    sample_rate_hz: int
    frame_count: int
    origin_seconds: float
    predictions: BeatPredictions
    assessment: SelectedBpmAssessment

    def report(self) -> dict[str, object]:
        """Return a serializable full report without modifying inference/publication bytes."""
        return asdict(self)
