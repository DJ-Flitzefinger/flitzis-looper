"""Pure frozen B2 temporal diagnostics; input certification and acceptance stay open.

This module never opens artifacts or verifies a seal. Callers must subsequently
bind independent references and complete raw candidates in the separate private
orchestration. Structurally valid input and perfect scores cannot certify music.
"""

import math
from bisect import bisect_right
from dataclasses import dataclass
from statistics import fmean
from typing import TYPE_CHECKING, Literal

from flitzis_looper.analysis.beat_matching import MAX_MATCHING_EVENTS, match_beats

if TYPE_CHECKING:
    from collections.abc import Sequence

    from flitzis_looper.analysis.contracts import BeatPredictions
    from flitzis_looper.analysis.reference_inputs_models import ReferenceTrack, Region

SCORING_POLICY = "b2a-private-pilot-v1-temporal-core-1"
TOLERANCES_MS = (10, 20, 40, 70)
UNCERTAINTY_LIMITS_MS = (2.5, 5.0, 10.0, 10.0)
MAX_REGIONS = 10000


@dataclass(frozen=True, slots=True)
class TimingLabel:
    """One reference event; uncertainty is an independently supplied half-width."""

    seconds: float
    uncertainty_ms: float


@dataclass(frozen=True, slots=True)
class ErrorDistribution:
    """Signed mean and linearly interpolated absolute p50/p95/max in milliseconds."""

    signed_bias_ms: float | None
    absolute_p50_ms: float | None
    absolute_p95_ms: float | None
    absolute_max_ms: float | None


@dataclass(frozen=True, slots=True)
class IntervalDistanceDistribution:
    """Nonnegative distance from the supplied uncertainty interval, in milliseconds."""

    mean_distance_ms: float | None
    p50_ms: float | None
    p95_ms: float | None
    max_ms: float | None


@dataclass(frozen=True, slots=True)
class MatchedEvent:
    """Original reference/prediction indices and immutable point-match diagnostics."""

    reference_index: int
    prediction_index: int
    signed_error_ms: float
    absolute_error_ms: float
    reference_uncertainty_halfwidth_ms: float
    interval_distance_ms: float


@dataclass(frozen=True, slots=True)
class ReferenceRun:
    """Consecutive reference endpoints; no duration is extrapolated past them."""

    first_reference_index: int
    last_reference_index: int
    event_count: int
    start_seconds: float
    end_seconds: float
    duration_seconds: float
    region_index: int


@dataclass(frozen=True, slots=True)
class RegionResult:
    """Fixed input region with endpoint counts from the same global matching."""

    region_index: int
    start_seconds: float
    end_seconds: float
    kind: str
    reference_events: int
    eligible_reference_events: int
    predictions: int
    matched_reference_events: int
    matched_predictions: int
    missing: int
    extra: int
    cross_region_matches: int


@dataclass(frozen=True, slots=True)
class ToleranceResult:
    """Frozen point-tolerance result with every denominator explicitly retained."""

    tolerance_ms: int
    eligible_uncertainty_halfwidth_ms: float
    eligible_reference_indices: tuple[int, ...]
    ineligible_reference_indices: tuple[int, ...]
    evaluated_prediction_indices: tuple[int, ...]
    excluded_nonmetrical_prediction_indices: tuple[int, ...]
    matches: tuple[MatchedEvent, ...]
    missing_reference_indices: tuple[int, ...]
    extra_prediction_indices: tuple[int, ...]
    precision: float | None
    recall: float | None
    f1: float | None
    missing_fraction_of_eligible_reference: float | None
    extra_fraction_of_eligible_reference: float | None
    errors: ErrorDistribution
    interval_distance_errors_on_point_matches: IntervalDistanceDistribution
    longest_matched_run: ReferenceRun | None
    unmatched_eligible_runs: tuple[ReferenceRun, ...]
    regions: tuple[RegionResult, ...]


@dataclass(frozen=True, slots=True)
class EventTimingReport:
    """One complete beat or downbeat event series, with no acceptance decision."""

    policy: str
    reference_event_count: int
    prediction_event_count: int
    confident_temporal_fraction: float
    trusted_metrical_fraction: float
    uncertainty_halfwidth_p50_ms: float | None
    uncertainty_halfwidth_p95_ms: float | None
    uncertainty_halfwidth_max_ms: float | None
    tolerances: tuple[ToleranceResult, ...]
    input_certification: Literal["unchecked_by_metric_core"] = "unchecked_by_metric_core"
    musical_acceptance: Literal["pending"] = "pending"
    default_adoption: Literal["blocked"] = "blocked"


@dataclass(frozen=True, slots=True)
class CriticalTimingResult:
    """Selected reference downbeat timing; candidate bar identity remains unchecked."""

    feature: str
    bar_id: int
    reference_index: int
    eligible_at_40_ms: bool
    prediction_index: int | None
    signed_error_ms: float | None
    candidate_bar_identity: Literal["unchecked"] = "unchecked"


@dataclass(frozen=True, slots=True)
class TrackTimingReport:
    """Complete temporal projection, separate from raw lineage and musical truth."""

    policy: str
    track_id: str
    beats: EventTimingReport
    downbeats: EventTimingReport
    critical_downbeat_timing: tuple[CriticalTimingResult, ...]
    regional_scope: Literal["supplied_regions_only_no_inferred_startup_break_tail"]
    input_certification: Literal["unchecked_by_metric_core"] = "unchecked_by_metric_core"
    quarter_count_and_bar_identity: Literal["pending"] = "pending"
    paired_correction_burden: Literal["pending"] = "pending"
    musical_acceptance: Literal["pending"] = "pending"
    default_adoption: Literal["blocked"] = "blocked"


def _quantile(sorted_values: Sequence[float], fraction: float) -> float | None:
    if not sorted_values:
        return None
    position = (len(sorted_values) - 1) * fraction
    lower = math.floor(position)
    upper = math.ceil(position)
    weight = position - lower
    return sorted_values[lower] * (1.0 - weight) + sorted_values[upper] * weight


def _distribution(values: Sequence[float]) -> ErrorDistribution:
    absolute = sorted(abs(value) for value in values)
    return ErrorDistribution(
        signed_bias_ms=fmean(values) if values else None,
        absolute_p50_ms=_quantile(absolute, 0.5),
        absolute_p95_ms=_quantile(absolute, 0.95),
        absolute_max_ms=absolute[-1] if absolute else None,
    )


def _interval_distribution(matches: tuple[MatchedEvent, ...]) -> IntervalDistanceDistribution:
    distances = sorted(match.interval_distance_ms for match in matches)
    return IntervalDistanceDistribution(
        fmean(distances) if distances else None,
        _quantile(distances, 0.5),
        _quantile(distances, 0.95),
        distances[-1] if distances else None,
    )


def _finite_number(value: float, name: str, *, positive: bool = False) -> float:
    if isinstance(value, bool) or not isinstance(value, int | float):
        msg = f"{name} must be a number"
        raise TypeError(msg)
    try:
        result = float(value)
    except OverflowError as error:
        msg = f"{name} must be a finite binary64 number"
        raise ValueError(msg) from error
    if not math.isfinite(result) or result < 0 or (positive and result == 0):
        msg = f"{name} must be finite and {'positive' if positive else 'nonnegative'}"
        raise ValueError(msg)
    return result


def _validate_regions(regions: Sequence[Region], duration: float) -> tuple[float, ...]:
    _finite_number(duration, "duration_seconds", positive=True)
    if not 0 < len(regions) <= MAX_REGIONS:
        msg = "complete region count must be from 1 to 10000"
        raise ValueError(msg)
    end = 0.0
    for region in regions:
        _finite_number(region.start_seconds, "region_start")
        _finite_number(region.end_seconds, "region_end")
        if region.start_seconds != end or not end < region.end_seconds <= duration:
            msg = "regions must tile the complete [0, duration) extent exactly"
            raise ValueError(msg)
        if region.kind not in {"metrical", "ambiguous", "nonrhythmic"}:
            msg = "unknown region kind"
            raise ValueError(msg)
        end = region.end_seconds
    if end != duration:
        msg = "regions must retain the complete tail"
        raise ValueError(msg)
    return tuple(region.start_seconds for region in regions)


def _event_regions(
    times: Sequence[float], starts: tuple[float, ...], duration: float
) -> tuple[int, ...]:
    if len(times) > MAX_MATCHING_EVENTS:
        msg = "complete event count exceeds 250000; no partial score"
        raise ValueError(msg)
    previous = -1.0
    result = []
    for index in range(len(times)):
        value = times[index]
        _finite_number(value, "event_seconds")
        if not previous < value < duration:
            msg = "events must be strictly increasing in the complete [0, duration) extent"
            raise ValueError(msg)
        result.append(bisect_right(starts, value) - 1)
        previous = value
    return tuple(result)


def _run(indices: Sequence[int], labels: Sequence[TimingLabel], region_index: int) -> ReferenceRun:
    first, last = indices[0], indices[-1]
    start, end = labels[first].seconds, labels[last].seconds
    return ReferenceRun(first, last, len(indices), start, end, end - start, region_index)


def _unmatched_runs(
    missing: tuple[int, ...], labels: Sequence[TimingLabel], ref_regions: tuple[int, ...]
) -> tuple[ReferenceRun, ...]:
    runs = []
    current: list[int] = []
    for index in missing:
        if current and (index != current[-1] + 1 or ref_regions[index] != ref_regions[current[-1]]):
            runs.append(_run(current, labels, ref_regions[current[0]]))
            current = []
        current.append(index)
    if current:
        runs.append(_run(current, labels, ref_regions[current[0]]))
    return tuple(runs)


def _longest_run(
    matches: tuple[MatchedEvent, ...],
    labels: Sequence[TimingLabel],
    ref_regions: tuple[int, ...],
    pred_regions: tuple[int, ...],
) -> ReferenceRun | None:
    best: ReferenceRun | None = None
    current: list[int] = []
    previous_prediction = -2
    for match in matches:
        index = match.reference_index
        if current and (
            index != current[-1] + 1
            or match.prediction_index != previous_prediction + 1
            or ref_regions[index] != ref_regions[current[-1]]
            or pred_regions[match.prediction_index] != pred_regions[previous_prediction]
        ):
            current = []
        current.append(index)
        candidate = _run(current, labels, ref_regions[index])
        # Rank by duration, then count, then earlier original reference endpoint.
        if best is None or (candidate.duration_seconds, candidate.event_count) > (
            best.duration_seconds,
            best.event_count,
        ):
            best = candidate
        previous_prediction = match.prediction_index
    return best


def _region_results(
    regions: Sequence[Region],
    ref_regions: tuple[int, ...],
    pred_regions: tuple[int, ...],
    eligible: tuple[int, ...],
    matches: tuple[MatchedEvent, ...],
) -> tuple[RegionResult, ...]:
    totals = [[0] * 6 for _ in regions]
    for region_index in ref_regions:
        totals[region_index][0] += 1
    for index in eligible:
        totals[ref_regions[index]][1] += 1
    for region_index in pred_regions:
        totals[region_index][2] += 1
    for match in matches:
        ref_region, pred_region = (
            ref_regions[match.reference_index],
            pred_regions[match.prediction_index],
        )
        totals[ref_region][3] += 1
        totals[pred_region][4] += 1
        if ref_region != pred_region:
            totals[ref_region][5] += 1
            totals[pred_region][5] += 1
    return tuple(
        RegionResult(
            region_index=index,
            start_seconds=region.start_seconds,
            end_seconds=region.end_seconds,
            kind=region.kind,
            reference_events=totals[index][0],
            eligible_reference_events=totals[index][1],
            predictions=totals[index][2],
            matched_reference_events=totals[index][3],
            matched_predictions=totals[index][4],
            missing=totals[index][1] - totals[index][3],
            extra=totals[index][2] - totals[index][4] if region.kind == "metrical" else 0,
            cross_region_matches=totals[index][5],
        )
        for index, region in enumerate(regions)
    )


@dataclass(frozen=True, slots=True)
class _TimingInput:
    labels: tuple[TimingLabel, ...]
    predictions: tuple[float, ...]
    regions: tuple[Region, ...]
    ref_regions: tuple[int, ...]
    pred_regions: tuple[int, ...]
    evaluated: tuple[int, ...]
    excluded: tuple[int, ...]


def _tolerance_result(data: _TimingInput, tolerance: int, uncertainty: float) -> ToleranceResult:
    eligible = tuple(
        i for i, label in enumerate(data.labels) if label.uncertainty_ms <= uncertainty
    )
    ineligible = tuple(
        i for i, label in enumerate(data.labels) if label.uncertainty_ms > uncertainty
    )
    matching = match_beats(
        tuple(data.labels[index].seconds for index in eligible),
        tuple(data.predictions[index] for index in data.evaluated),
        tolerance / 1000.0,
    )
    matches = tuple(
        _matched_event(
            data.labels[eligible[ref]],
            data.predictions[data.evaluated[pred]],
            eligible[ref],
            data.evaluated[pred],
        )
        for ref, pred in matching.pairs
    )
    matched_refs = {match.reference_index for match in matches}
    matched_preds = {match.prediction_index for match in matches}
    missing = tuple(index for index in eligible if index not in matched_refs)
    extra = tuple(index for index in data.evaluated if index not in matched_preds)
    reference_count, prediction_count, count = len(eligible), len(data.evaluated), len(matches)
    return ToleranceResult(
        tolerance,
        uncertainty,
        eligible,
        ineligible,
        data.evaluated,
        data.excluded,
        matches,
        missing,
        extra,
        count / prediction_count if prediction_count else None,
        count / reference_count if reference_count else None,
        2.0 * count / (reference_count + prediction_count)
        if reference_count + prediction_count
        else None,
        len(missing) / reference_count if reference_count else None,
        len(extra) / reference_count if reference_count else None,
        _distribution(tuple(match.signed_error_ms for match in matches)),
        _interval_distribution(matches),
        _longest_run(matches, data.labels, data.ref_regions, data.pred_regions),
        _unmatched_runs(missing, data.labels, data.ref_regions),
        _region_results(data.regions, data.ref_regions, data.pred_regions, eligible, matches),
    )


def _matched_event(label: TimingLabel, prediction: float, ref: int, pred: int) -> MatchedEvent:
    error = (prediction - label.seconds) * 1000.0
    return MatchedEvent(
        ref,
        pred,
        error,
        abs(error),
        label.uncertainty_ms,
        max(0.0, abs(error) - label.uncertainty_ms),
    )


def score_timing_events(
    labels: Sequence[TimingLabel],
    prediction_seconds: Sequence[float],
    regions: Sequence[Region],
    duration_seconds: float,
) -> EventTimingReport:
    """Score a complete temporal series without certifying its source or independence.

    Regions must already be independently declared and tile the complete extent.
    Half-open region boundaries classify a boundary event in the following region.
    Only predeclared ambiguous/nonrhythmic regions exclude predictions. Ineligible
    references never erase nearby predictions. All reference labels must be metrical.
    Global matching retains exact binary64-time objectives, including across a
    short declared gap; regional counts report cross-boundary matches explicitly.
    Quantiles use linear interpolation at (N-1)*p. A longest run is ranked by
    reference endpoint duration, then count, then earliest reference index.
    Zero denominators and empty error distributions return None, never a pass.
    Interval distances are companion errors on the same point matches only.

    Raises:
        ValueError: Invalid temporal structure, values or complete-input limits.
        TypeError: A temporal number is not an int/float or is a boolean.
        BeatMatchingLimitError: A tolerance needs more than 1000000 eligible edges.
    """
    duration_seconds = _finite_number(duration_seconds, "duration_seconds", positive=True)
    starts = _validate_regions(regions, duration_seconds)
    if len(labels) > MAX_MATCHING_EVENTS:
        msg = "complete reference count exceeds 250000; no partial score"
        raise ValueError(msg)
    if len(prediction_seconds) > MAX_MATCHING_EVENTS:
        msg = "complete prediction count exceeds 250000; no partial score"
        raise ValueError(msg)
    labels = tuple(
        TimingLabel(
            _finite_number(labels[i].seconds, "event_seconds"),
            _finite_number(labels[i].uncertainty_ms, "uncertainty_halfwidth_ms"),
        )
        for i in range(len(labels))
    )
    prediction_seconds = tuple(
        _finite_number(prediction_seconds[i], "event_seconds")
        for i in range(len(prediction_seconds))
    )
    ref_regions = _event_regions(tuple(label.seconds for label in labels), starts, duration_seconds)
    pred_regions = _event_regions(prediction_seconds, starts, duration_seconds)
    for label, region_index in zip(labels, ref_regions, strict=True):
        _finite_number(label.uncertainty_ms, "uncertainty_halfwidth_ms")
        if label.uncertainty_ms > 1000 or regions[region_index].kind != "metrical":
            msg = "reference labels require metrical regions and half-width at most 1000 ms"
            raise ValueError(msg)
    data = _TimingInput(
        tuple(labels),
        tuple(prediction_seconds),
        tuple(regions),
        ref_regions,
        pred_regions,
        tuple(i for i, region in enumerate(pred_regions) if regions[region].kind == "metrical"),
        tuple(i for i, region in enumerate(pred_regions) if regions[region].kind != "metrical"),
    )
    widths = sorted(label.uncertainty_ms for label in labels)
    return EventTimingReport(
        SCORING_POLICY,
        len(labels),
        len(prediction_seconds),
        math.fsum(
            region.end_seconds - region.start_seconds
            for region in regions
            if region.kind != "ambiguous"
        )
        / duration_seconds,
        math.fsum(
            region.end_seconds - region.start_seconds
            for region in regions
            if region.kind == "metrical"
        )
        / duration_seconds,
        _quantile(widths, 0.5),
        _quantile(widths, 0.95),
        widths[-1] if widths else None,
        tuple(
            _tolerance_result(data, tolerance, limit)
            for tolerance, limit in zip(TOLERANCES_MS, UNCERTAINTY_LIMITS_MS, strict=True)
        ),
    )


def _validate_logits(logits: tuple[float, ...]) -> None:
    if len(logits) > MAX_MATCHING_EVENTS:
        msg = "complete logits must be finite and bounded"
        raise ValueError(msg)
    for value in logits:
        if isinstance(value, bool) or not isinstance(value, int | float):
            msg = "complete logits require numeric values"
            raise TypeError(msg)
        try:
            finite = math.isfinite(value)
        except OverflowError as error:
            msg = "complete logits must be finite binary64 values"
            raise ValueError(msg) from error
        if not finite:
            msg = "complete logits must be finite and bounded"
            raise ValueError(msg)


def score_track_timing(
    reference: ReferenceTrack, predictions: BeatPredictions
) -> TrackTimingReport:
    """Project complete supplied beat/downbeat arrays into explicitly uncertified metrics.

    This pure API does not validate a ReferenceSeal or raw prediction lineage.
    It reads no PCM, protocol file, candidate artifact or model. A later private
    orchestrator must establish those bindings before any measured claim.
    Musical quarter counts/bar identity and human correction gates stay pending.
    """
    duration = reference.identity.pcm.frame_count / reference.identity.pcm.sample_rate_hz
    if reference.extent_start_seconds != 0 or reference.extent_end_seconds != duration:
        msg = "reference must retain the complete native loaded frame-zero extent"
        raise ValueError(msg)
    if len(predictions.beat_logits) != len(predictions.downbeat_logits):
        msg = "complete beat/downbeat logits must retain equal counts"
        raise ValueError(msg)
    for logits in (predictions.beat_logits, predictions.downbeat_logits):
        _validate_logits(logits)
    beats = score_timing_events(
        tuple(TimingLabel(label.seconds, label.uncertainty_ms) for label in reference.beats),
        predictions.beat_seconds,
        reference.regions,
        duration,
    )
    downbeats = score_timing_events(
        tuple(TimingLabel(label.seconds, label.uncertainty_ms) for label in reference.bars),
        predictions.downbeat_seconds,
        reference.regions,
        duration,
    )
    result_40 = downbeats.tolerances[2]
    matches = {match.reference_index: match for match in result_40.matches}
    bars = {bar.bar_id: index for index, bar in enumerate(reference.bars)}
    if len(bars) != len(reference.bars):
        msg = "reference bar identities must be unique"
        raise ValueError(msg)
    critical = []
    for feature in reference.critical_features:
        for bar_id in feature.bar_ids:
            if bar_id not in bars:
                msg = "critical feature must identify a supplied reference bar"
                raise ValueError(msg)
            index = bars[bar_id]
            match = matches.get(index)
            critical.append(
                CriticalTimingResult(
                    feature.feature,
                    bar_id,
                    index,
                    reference.bars[index].uncertainty_ms <= 10.0,
                    match.prediction_index if match else None,
                    match.signed_error_ms if match else None,
                )
            )
    return TrackTimingReport(
        SCORING_POLICY,
        reference.identity.track_id,
        beats,
        downbeats,
        tuple(critical),
        "supplied_regions_only_no_inferred_startup_break_tail",
    )
