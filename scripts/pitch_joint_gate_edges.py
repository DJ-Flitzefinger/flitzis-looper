"""Complete conditional source-grid edge pruning for at most two finite gates.

The caller supplies a common plateau certificate whose real IQ box encloses
every feasible model under the declared global error and single-frame nuisance
budgets. Independent sample intervals deliberately relax all other gates to
``[0, 1]``. Surviving edges are necessary candidates, never admitted models.
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING

import numpy as np

if TYPE_CHECKING:
    from numpy.typing import NDArray
    from pitch_joint_gate_plateau import JointPlateauCertificate

type Real = NDArray[np.float64]
type Integer = NDArray[np.int64]
type EdgeCandidates = tuple[tuple[list[tuple[int, int]], list[tuple[int, int]]], ...]

MIN_RAMP_FRAMES = 3
MAX_RAMP_FRAMES = 512
MAX_EDGE_SURVIVORS = 4096
MAX_ANCHOR_SURVIVORS = 4096
MAX_EDGE_EVALUATIONS = 2_000_000
WIDTH_BATCH = 64
RAMP_PREFILTER_STRIDE = 16
PLATEAU_FRAMES = 2048
_EPS = np.finfo(np.float64).eps
_TINY = np.finfo(np.float64).tiny
_GATE_ABSOLUTE_ALLOWANCE = 64.0 * _EPS


@dataclass(slots=True)
class _Intervals:
    own_lower: Real
    own_upper: Real
    other_lower: Real
    other_upper: Real

    def sliced(self, start: int, end: int) -> _Intervals:
        return _Intervals(
            self.own_lower[start:end],
            self.own_upper[start:end],
            self.other_lower[start:end],
            self.other_upper[start:end],
        )

    def indexed(self, indices: Integer) -> _Intervals:
        return _Intervals(
            self.own_lower[indices],
            self.own_upper[indices],
            self.other_lower[indices],
            self.other_upper[indices],
        )


@dataclass(slots=True)
class _Cumulative:
    prefix_sum: Real
    prefix_max: Real
    suffix_sum: Real
    suffix_max: Real


@dataclass(slots=True)
class _EdgeProblem:
    source: Real
    intervals: _Intervals
    zeros: _Cumulative
    ones: _Cumulative
    offset: int
    release: bool
    budget_squared: float


@dataclass(slots=True)
class _RampObservation:
    source: Real
    intervals: _Intervals
    distances: Integer
    fixed_total: float
    fixed_maximum: float
    observation_frames: int


def _grid_width_count(anchors: int) -> int:
    increasing = min(anchors, MAX_RAMP_FRAMES - MIN_RAMP_FRAMES + 1)
    return increasing * (increasing + 1) // 2 + (anchors - increasing) * (
        MAX_RAMP_FRAMES - MIN_RAMP_FRAMES + 1
    )


def _edge_report(anchors: int) -> dict:
    return {
        "grid_anchor_count": anchors,
        "grid_width_count": _grid_width_count(anchors),
        "anchor_lower_bounds_evaluated": 0,
        "anchor_pruned_count": 0,
        "anchor_survivor_count": 0,
        "anchor_pruned_width_count": 0,
        "requested_width_evaluations": 0,
        "evaluated_width_count": 0,
        "partial_ramp_width_evaluations": 0,
        "partial_ramp_pruned_width_count": 0,
        "full_ramp_width_evaluations": 0,
        "full_ramp_pruned_width_count": 0,
        "width_pruned_count": 0,
        "edge_survivor_count": 0,
        "complete": False,
        "cap_exceeded": None,
    }


def _report(frames: int, start: int, end: int, carriers: int, error_budget: float) -> dict:
    return {
        "accepted": False,
        "status": "refused",
        "refusal_reason": None,
        "search_complete": False,
        "incomplete_search": False,
        "source_grid_search_complete": False,
        "full_source_grid_enumerated": False,
        "conditional_on_joint_plateau_certificate": True,
        "conditional_on_fixed_carrier_basis": True,
        "source_only_selection": True,
        "activity_oracle_bracketing_used": False,
        "instantaneous_pcm_division_used": False,
        "edge_survivors_are_necessary_candidates_only": True,
        "joint_global_confirmation_required": True,
        "global_error_budget_l2": error_budget,
        "nuisance_budget_stereo_frames": 1,
        "nuisance_may_replace_both_channels_of_one_arbitrary_frame": True,
        "nuisance_lower_bound": "sum_frame_distances_squared_minus_largest_frame",
        "each_edge_bound_relaxes_the_global_nuisance_location_independently": True,
        "other_gate_interval": [0.0, 1.0],
        "frame_prediction_intervals_from_full_real_iq_box": True,
        "observation_union_has_no_double_counted_frames": True,
        "source_frames": frames,
        "carriers": carriers,
        "plateau_interval_half_open": [start, end],
        "edge_observation_intervals_half_open": {
            "onset": [0, start],
            "release": [end, frames],
        },
        "source_grid_onset_anchor_interval_inclusive": [0, start - MIN_RAMP_FRAMES],
        "source_grid_release_anchor_interval_inclusive": [
            end - 1 + MIN_RAMP_FRAMES,
            frames - 1,
        ],
        "ramp_width_bounds": [MIN_RAMP_FRAMES, MAX_RAMP_FRAMES],
        "maximum_anchor_survivors_per_edge": MAX_ANCHOR_SURVIVORS,
        "maximum_edge_survivors_per_edge": MAX_EDGE_SURVIVORS,
        "maximum_edge_evaluations_per_edge": MAX_EDGE_EVALUATIONS,
        "maximum_width_batch": WIDTH_BATCH,
        "maximum_source_positions_per_width_batch": MAX_RAMP_FRAMES - 1,
        "ramp_prefilter_stride_source_frames": RAMP_PREFILTER_STRIDE,
        "ramp_prefilter_includes_first_and_last_observed_ramp_frame": True,
        "ramp_prefilter_is_necessary_subset_lower_bound": True,
        "ramp_prefilter_retains_full_observation_length_roundoff_allowance": True,
        "interval_endpoints_expanded_outward": True,
        "distance_and_sum_roundoff_allowances_applied": True,
        "cosine_absolute_error_allowance": _GATE_ABSOLUTE_ALLOWANCE,
        "cosine_error_within_reported_allowance_is_assumed": True,
        "universal_libm_error_bound_claim": False,
        "carrier_reports": [
            {
                "carrier_index": carrier,
                "onset": _edge_report(max(0, start - MIN_RAMP_FRAMES + 1)),
                "release": _edge_report(max(0, frames - end - MIN_RAMP_FRAMES + 1)),
            }
            for carrier in range(carriers)
        ],
    }


def _validate(
    source: Real,
    basis: Real,
    certificate: JointPlateauCertificate,
    error_budget: float,
) -> tuple[int, int, int]:
    if (
        not isinstance(source, np.ndarray)
        or source.dtype != np.float64
        or source.ndim != 2
        or source.shape[1] != 2
        or np.iscomplexobj(source)
    ):
        message = "Joint edge enumeration requires original real float64 stereo PCM"
        raise ValueError(message)
    message = "Joint edge basis must be real float64 N x 2 x (2*K), with K in 1..2"
    if (
        not isinstance(basis, np.ndarray)
        or basis.dtype != np.float64
        or basis.ndim != 3
        or np.iscomplexobj(basis)
    ):
        raise ValueError(message)
    if basis.shape[:2] != source.shape or basis.shape[2] not in {2, 4}:
        raise ValueError(message)
    start, end = certificate.start, certificate.end
    if any(
        isinstance(value, (bool, np.bool_)) or not isinstance(value, (int, np.integer))
        for value in (start, end)
    ):
        message = "Joint plateau endpoints must be integer source frames"
        raise TypeError(message)
    if not 0 <= start < end <= len(source) or end - start != PLATEAU_FRAMES:
        message = "Joint edge enumeration requires a certified 2048-frame source plateau"
        raise ValueError(message)
    for bound in (certificate.beta_lower, certificate.beta_upper):
        if (
            not isinstance(bound, np.ndarray)
            or bound.dtype != np.float64
            or bound.shape != (basis.shape[2],)
            or np.iscomplexobj(bound)
        ):
            message = "Joint plateau IQ bounds must be real float64 vectors of length 2*K"
            raise ValueError(message)
    if isinstance(error_budget, (bool, np.bool_)) or not np.isscalar(error_budget):
        message = "Joint edge error budget must be a nonnegative real scalar"
        raise TypeError(message)
    if np.iscomplexobj(error_budget) or error_budget < 0.0:
        message = "Joint edge error budget must be a nonnegative real scalar"
        raise ValueError(message)
    return int(start), int(end), basis.shape[2] // 2


def _tonal_intervals(basis: Real, lower: Real, upper: Real) -> list[tuple[Real, Real]]:
    intervals = []
    for carrier in range(basis.shape[2] // 2):
        columns = basis[:, :, 2 * carrier : 2 * carrier + 2]
        low_product = columns * lower[None, None, 2 * carrier : 2 * carrier + 2]
        high_product = columns * upper[None, None, 2 * carrier : 2 * carrier + 2]
        lo = np.sum(np.minimum(low_product, high_product), axis=2)
        hi = np.sum(np.maximum(low_product, high_product), axis=2)
        scale = np.sum(np.maximum(np.abs(low_product), np.abs(high_product)), axis=2)
        radius = 64.0 * _EPS * scale + _TINY
        intervals.append((np.nextafter(lo - radius, -np.inf), np.nextafter(hi + radius, np.inf)))
    return intervals


def _mode_intervals(tonal: list[tuple[Real, Real]], carrier: int) -> _Intervals:
    own_lower, own_upper = tonal[carrier]
    other_lower, other_upper = np.zeros_like(own_lower), np.zeros_like(own_upper)
    for index, (lower, upper) in enumerate(tonal):
        if index != carrier:
            other_lower += np.minimum(0.0, lower)
            other_upper += np.maximum(0.0, upper)
    return _Intervals(own_lower, own_upper, other_lower, other_upper)


def _frame_distances(
    source: Real,
    intervals: _Intervals,
    weights: float | Real,
    *,
    gate_allowance: float = 0.0,
) -> Real:
    scale = weights[..., None] if isinstance(weights, np.ndarray) else weights
    lo_product, hi_product = scale * intervals.own_lower, scale * intervals.own_upper
    lo, hi = lo_product + intervals.other_lower, hi_product + intervals.other_upper
    magnitude = np.maximum(np.abs(intervals.own_lower), np.abs(intervals.own_upper))
    radius = (
        32.0
        * _EPS
        * (
            np.abs(lo_product)
            + np.abs(hi_product)
            + np.abs(intervals.other_lower)
            + np.abs(intervals.other_upper)
        )
        + gate_allowance * magnitude
        + _TINY
    )
    lo, hi = np.nextafter(lo - radius, -np.inf), np.nextafter(hi + radius, np.inf)
    distance = np.maximum(np.maximum(lo - source, source - hi), 0.0)
    subtraction_allowance = 32.0 * _EPS * (np.abs(source) + np.abs(lo) + np.abs(hi)) + _TINY
    distance = np.maximum(distance - subtraction_allowance, 0.0)
    squared = np.sum(distance * distance, axis=-1)
    squared = np.maximum(squared - 32.0 * _EPS * squared - _TINY, 0.0)
    if not np.isfinite(squared).all():
        message = "Nonfinite joint edge interval distance"
        raise FloatingPointError(message)
    return squared


def _cumulative(values: Real) -> _Cumulative:
    prefix_sum, prefix_max = np.zeros(len(values) + 1), np.zeros(len(values) + 1)
    suffix_sum, suffix_max = np.zeros(len(values) + 1), np.zeros(len(values) + 1)
    np.cumsum(values, out=prefix_sum[1:])
    np.maximum.accumulate(values, out=prefix_max[1:])
    np.cumsum(values[::-1], out=suffix_sum[:-1][::-1])
    np.maximum.accumulate(values[::-1], out=suffix_max[:-1][::-1])
    return _Cumulative(prefix_sum, prefix_max, suffix_sum, suffix_max)


def _trimmed_lower_bound(total: Real, maximum: Real, frames: int) -> Real:
    # All summands are nonnegative. This allowance covers cumulative sums,
    # bounded ramp reductions, their combination and the final subtraction.
    gamma = 32.0 * _EPS * (frames + 32)
    if gamma >= 0.25:
        message = "Joint edge summation length exceeds the numerical allowance"
        raise FloatingPointError(message)
    allowance = gamma * (total + maximum) + _TINY * (frames + 1)
    bound = np.maximum(total - maximum - allowance, 0.0)
    if not np.isfinite(bound).all():
        message = "Nonfinite joint edge nuisance lower bound"
        raise FloatingPointError(message)
    return bound


def _anchor_candidates(problem: _EdgeProblem, report: dict) -> Integer:
    frames = len(problem.source)
    if problem.release:
        anchors = np.arange(MIN_RAMP_FRAMES - 1, frames, dtype=np.int64)
        one_stop = np.maximum(anchors - MAX_RAMP_FRAMES + 1, 0)
        total = problem.ones.prefix_sum[one_stop] + problem.zeros.suffix_sum[anchors]
        maximum = np.maximum(problem.ones.prefix_max[one_stop], problem.zeros.suffix_max[anchors])
    else:
        anchors = np.arange(max(0, frames - MIN_RAMP_FRAMES + 1), dtype=np.int64)
        one_start = np.minimum(anchors + MAX_RAMP_FRAMES, frames)
        total = problem.zeros.prefix_sum[anchors + 1] + problem.ones.suffix_sum[one_start]
        maximum = np.maximum(
            problem.zeros.prefix_max[anchors + 1], problem.ones.suffix_max[one_start]
        )
    bound = _trimmed_lower_bound(total, maximum, frames)
    survivors = anchors[bound <= problem.budget_squared]
    max_widths = (
        np.minimum(MAX_RAMP_FRAMES, survivors + 1)
        if problem.release
        else np.minimum(MAX_RAMP_FRAMES, frames - survivors)
    )
    widths = int(np.sum(max_widths - MIN_RAMP_FRAMES + 1, dtype=np.int64))
    report.update(
        anchor_lower_bounds_evaluated=len(anchors),
        anchor_pruned_count=len(anchors) - len(survivors),
        anchor_survivor_count=len(survivors),
        anchor_pruned_width_count=report["grid_width_count"] - widths,
        requested_width_evaluations=widths,
    )
    return survivors


def _ramp_span(problem: _EdgeProblem, anchor: int) -> tuple[int, int, int, float, float]:
    if problem.release:
        maximum_width = min(MAX_RAMP_FRAMES, anchor + 1)
        start, end = anchor - maximum_width + 1, anchor
        total = problem.ones.prefix_sum[start] + problem.zeros.suffix_sum[anchor]
        maximum = max(problem.ones.prefix_max[start], problem.zeros.suffix_max[anchor])
    else:
        maximum_width = min(MAX_RAMP_FRAMES, len(problem.source) - anchor)
        start, end = anchor + 1, anchor + maximum_width
        total = problem.zeros.prefix_sum[start] + problem.ones.suffix_sum[end]
        maximum = max(problem.zeros.prefix_max[start], problem.ones.suffix_max[end])
    return start, end, maximum_width, float(total), float(maximum)


def _ramp_weights(distances: Integer, widths: Integer) -> Real:
    progress = distances[None, :] / widths[:, None]
    np.clip(progress, 0.0, 1.0, out=progress)
    weights = 0.5 * (1.0 - np.cos(np.pi * progress))
    # Constants outside a trial's ramp are exactly zero or one.
    weights[progress >= 1.0] = 1.0
    weights[progress <= 0.0] = 0.0
    return weights


def _ramp_observation(problem: _EdgeProblem, anchor: int) -> tuple[_RampObservation, int]:
    start, end, maximum_width, fixed_total, fixed_maximum = _ramp_span(problem, anchor)
    distances = np.arange(start, end, dtype=np.int64)
    distances = anchor - distances if problem.release else distances - anchor
    return (
        _RampObservation(
            problem.source[start:end],
            problem.intervals.sliced(start, end),
            distances,
            fixed_total,
            fixed_maximum,
            len(problem.source),
        ),
        maximum_width,
    )


def _ramp_subset(observation: _RampObservation) -> _RampObservation:
    indices = np.unique(
        np.append(
            np.arange(0, len(observation.source), RAMP_PREFILTER_STRIDE, dtype=np.int64),
            len(observation.source) - 1,
        )
    )
    return _RampObservation(
        observation.source[indices],
        observation.intervals.indexed(indices),
        observation.distances[indices],
        observation.fixed_total,
        observation.fixed_maximum,
        observation.observation_frames,
    )


def _ramp_lower_bounds(observation: _RampObservation, widths: Integer) -> Real:
    contributions = _frame_distances(
        observation.source,
        observation.intervals,
        _ramp_weights(observation.distances, widths),
        gate_allowance=_GATE_ABSOLUTE_ALLOWANCE,
    )
    total = observation.fixed_total + np.sum(contributions, axis=1)
    maximum = np.maximum(observation.fixed_maximum, np.max(contributions, axis=1, initial=0.0))
    return _trimmed_lower_bound(total, maximum, observation.observation_frames)


def _width_candidates(problem: _EdgeProblem, anchor: int, report: dict) -> list[tuple[int, int]]:
    observation, maximum_width = _ramp_observation(problem, anchor)
    subset = _ramp_subset(observation)
    survivors = []
    for first in range(MIN_RAMP_FRAMES, maximum_width + 1, WIDTH_BATCH):
        widths = np.arange(first, min(first + WIDTH_BATCH, maximum_width + 1), dtype=np.int64)
        report["evaluated_width_count"] += len(widths)
        report["partial_ramp_width_evaluations"] += len(widths)
        # The fixed outside observations and this subset are disjoint. Adding
        # omitted nonnegative frame costs cannot decrease sum minus its largest
        # single frame, so this remains necessary under the same nuisance model.
        admitted = widths[_ramp_lower_bounds(subset, widths) <= problem.budget_squared]
        report["partial_ramp_pruned_width_count"] += len(widths) - len(admitted)
        report["width_pruned_count"] += len(widths) - len(admitted)
        if not len(admitted):
            continue
        report["full_ramp_width_evaluations"] += len(admitted)
        widths = admitted
        admitted = widths[_ramp_lower_bounds(observation, widths) <= problem.budget_squared]
        report["full_ramp_pruned_width_count"] += len(widths) - len(admitted)
        report["width_pruned_count"] += len(widths) - len(admitted)
        survivors.extend((problem.offset + anchor, int(width)) for width in admitted)
    return survivors


def _enumerate_edge(problem: _EdgeProblem, report: dict) -> list[tuple[int, int]] | None:
    anchors = _anchor_candidates(problem, report)
    if len(anchors) > MAX_ANCHOR_SURVIVORS:
        report["cap_exceeded"] = "anchor_survivor_cap"
        return None
    if report["requested_width_evaluations"] > MAX_EDGE_EVALUATIONS:
        report["cap_exceeded"] = "edge_evaluation_cap"
        return None
    survivors = []
    for anchor in anchors:
        survivors.extend(_width_candidates(problem, int(anchor), report))
        report["edge_survivor_count"] = len(survivors)
        if len(survivors) > MAX_EDGE_SURVIVORS:
            report["cap_exceeded"] = "edge_survivor_cap"
            return None
    report["complete"] = True
    return survivors


def _problem(
    source: Real,
    intervals: _Intervals,
    start: int,
    end: int,
    budget_squared: float,
    *,
    release: bool,
) -> _EdgeProblem:
    observations = source[start:end]
    local_intervals = intervals.sliced(start, end)
    zeros = _cumulative(_frame_distances(observations, local_intervals, 0.0))
    ones = _cumulative(_frame_distances(observations, local_intervals, 1.0))
    return _EdgeProblem(observations, local_intervals, zeros, ones, start, release, budget_squared)


def _refused(report: dict, reason: str, *, incomplete: bool) -> tuple[None, dict]:
    report.update(refusal_reason=reason, incomplete_search=incomplete)
    return None, report


def enumerate_joint_edges(
    source: Real,
    basis: Real,
    certificate: JointPlateauCertificate,
    error_budget: float,
) -> tuple[EdgeCandidates | None, dict]:
    """Retain every source-grid edge compatible with a certified joint IQ box.

    Args:
        source: Original finite float64 stereo PCM, without modification.
        basis: Fixed real N x 2 x (2*K) IQ design, interleaved by carrier.
        certificate: Common 2048-frame plateau and an enclosure of every
            feasible global IQ vector for the same error/nuisance hypothesis.
        error_budget: Absolute whole-source L2 residual limit after deleting
            at most one arbitrary complete stereo frame.

    Returns:
        For each carrier, onset ``(anchor, width)`` and release ``(end, width)``
        candidate lists, followed by a report. Empty lists are complete empty
        feasible edge sets. Nonfinite arithmetic or any fixed-cap overflow
        returns ``None`` with no partial candidate sets. Invalid shapes, types
        and plateau geometry raise ValueError or TypeError.
    """
    start, end, carriers = _validate(source, basis, certificate, error_budget)
    report = _report(len(source), start, end, carriers, float(error_budget))
    if not all(
        np.isfinite(value).all()
        for value in (source, basis, certificate.beta_lower, certificate.beta_upper)
    ) or not np.isfinite(error_budget):
        return _refused(report, "nonfinite_input", incomplete=True)
    if np.any(certificate.beta_lower > certificate.beta_upper):
        message = "Joint plateau IQ lower bounds must not exceed upper bounds"
        raise ValueError(message)
    edges = []
    try:
        with np.errstate(over="raise", invalid="raise", divide="raise"):
            budget_squared = float(np.nextafter(float(error_budget) ** 2, np.inf))
            if not np.isfinite(budget_squared):
                return _refused(report, "nonfinite_error_budget_square", incomplete=True)
            tonal = _tonal_intervals(basis, certificate.beta_lower, certificate.beta_upper)
            for carrier in range(carriers):
                intervals = _mode_intervals(tonal, carrier)
                mode_report = report["carrier_reports"][carrier]
                onset = _enumerate_edge(
                    _problem(source, intervals, 0, start, budget_squared, release=False),
                    mode_report["onset"],
                )
                if onset is None:
                    reason = f"carrier_{carrier}_onset_{mode_report['onset']['cap_exceeded']}"
                    return _refused(report, reason, incomplete=True)
                release = _enumerate_edge(
                    _problem(source, intervals, end, len(source), budget_squared, release=True),
                    mode_report["release"],
                )
                if release is None:
                    reason = f"carrier_{carrier}_release_{mode_report['release']['cap_exceeded']}"
                    return _refused(report, reason, incomplete=True)
                edges.append((onset, release))
    except FloatingPointError, OverflowError:
        return _refused(report, "nonfinite_or_unresolved_interval_arithmetic", incomplete=True)
    report.update(
        status="complete",
        search_complete=True,
        source_grid_search_complete=True,
        full_source_grid_enumerated=True,
    )
    return tuple(edges), report
