"""Bounded, exact offline matching for the frozen B2 scoring protocol.

Times and tolerance denote their exact binary64 values after numeric validation.
An eligible pair has exact absolute distance at most that tolerance. The selected
monotone one-to-one sequence maximizes cardinality, minimizes exact summed
absolute distance, then chooses the lexicographically smallest index sequence.
This module reads no artifacts, imports no model and performs no realtime work.
"""

import math
from array import array
from dataclasses import dataclass
from fractions import Fraction
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Sequence

MAX_MATCHING_EVENTS = 250000
MAX_ELIGIBLE_EDGES = 1000000


class BeatMatchingLimitError(ValueError):
    """The complete input exceeds an explicit matching resource limit."""


@dataclass(frozen=True, slots=True)
class BeatMatchingResult:
    """Original index pairs and exact/rounded summed absolute timing error.

    The rounded total may be infinity if a valid exact sum exceeds binary64's
    range. It is never used to select a matching; the Fraction remains exact.
    """

    pairs: tuple[tuple[int, int], ...]
    total_absolute_error_seconds: float
    exact_absolute_error_seconds: Fraction


@dataclass(frozen=True, slots=True)
class _EligibleWindows:
    first: array[int]
    stop: array[int]
    offset: array[int]
    edge_count: int


class _FenwickScores:
    """Best count/cost for suffixes in reversed prediction-index order."""

    def __init__(self, prediction_count: int) -> None:
        self.counts = array("I", [0]) * (prediction_count + 1)
        self.costs = [0] * (prediction_count + 1)

    def best(self, position: int) -> tuple[int, int]:
        count = cost = 0
        while position:
            candidate_count = self.counts[position]
            candidate_cost = self.costs[position]
            if candidate_count > count or (candidate_count == count and candidate_cost < cost):
                count, cost = candidate_count, candidate_cost
            position -= position & -position
        return count, cost

    def include(self, position: int, count: int, cost: int) -> None:
        while position < len(self.counts):
            stored_count = self.counts[position]
            if count > stored_count or (count == stored_count and cost < self.costs[position]):
                self.counts[position] = count
                self.costs[position] = cost
            position += position & -position


def _finite_nonnegative(value: float, name: str) -> float:
    if isinstance(value, bool) or not isinstance(value, int | float):
        msg = f"{name} must be a finite nonnegative number"
        raise TypeError(msg)
    try:
        result = float(value)
    except OverflowError as error:
        msg = f"{name} must be a finite nonnegative number"
        raise ValueError(msg) from error
    if not math.isfinite(result) or result < 0:
        msg = f"{name} must be a finite nonnegative number"
        raise ValueError(msg)
    return result


def _validated_times(values: Sequence[float], name: str) -> tuple[float, ...]:
    count = len(values)
    if count > MAX_MATCHING_EVENTS:
        msg = f"{name} exceeds {MAX_MATCHING_EVENTS} events; complete matching rejected"
        raise BeatMatchingLimitError(msg)
    result: list[float] = []
    previous = -1.0
    # Index only the declared bounded sequence extent; never consume an iterator
    # with an unknown or potentially infinite extent during validation.
    for index in range(count):
        value = _finite_nonnegative(values[index], name)
        if value <= previous:
            msg = f"{name} must be strictly increasing"
            raise ValueError(msg)
        result.append(value)
        previous = value
    return tuple(result)


def _float_units(value: float, scale: int) -> int:
    numerator, denominator = value.as_integer_ratio()
    return numerator * (scale // denominator)


def _dyadic_units(
    reference: tuple[float, ...], prediction: tuple[float, ...], tolerance: float
) -> tuple[list[int], list[int], int, int]:
    scale_bits = tolerance.as_integer_ratio()[1].bit_length() - 1
    for values in (reference, prediction):
        for value in values:
            scale_bits = max(scale_bits, value.as_integer_ratio()[1].bit_length() - 1)
    scale = 1 << scale_bits
    return (
        [_float_units(value, scale) for value in reference],
        [_float_units(value, scale) for value in prediction],
        _float_units(tolerance, scale),
        scale,
    )


def _eligible_windows(
    reference: list[int], prediction: list[int], tolerance: int, max_edges: int
) -> _EligibleWindows:
    first = array("I")
    stop = array("I")
    offset = array("I", [0])
    lower = upper = edges = 0
    for value in reference:
        while lower < len(prediction) and prediction[lower] < value - tolerance:
            lower += 1
        upper = max(upper, lower)
        while upper < len(prediction) and prediction[upper] <= value + tolerance:
            upper += 1
        edges += upper - lower
        if edges > max_edges:
            msg = f"eligible pairs exceed {max_edges}; complete matching rejected"
            raise BeatMatchingLimitError(msg)
        first.append(lower)
        stop.append(upper)
        offset.append(edges)
    return _EligibleWindows(first, stop, offset, edges)


def _suffix_scores(
    reference: list[int], prediction: list[int], windows: _EligibleWindows
) -> tuple[array[int], list[int], int, int]:
    counts = array("I", [0]) * windows.edge_count
    costs = [0] * windows.edge_count
    tree = _FenwickScores(len(prediction))
    for ref_index in reversed(range(len(reference))):
        first, stop = windows.first[ref_index], windows.stop[ref_index]
        offset = windows.offset[ref_index]
        for pred_index in range(first, stop):
            edge_index = offset + pred_index - first
            count, cost = tree.best(len(prediction) - pred_index - 1)
            counts[edge_index] = count + 1
            costs[edge_index] = cost + abs(reference[ref_index] - prediction[pred_index])
        # Delay updates until the whole row is scored: one reference must never
        # extend another edge in its own row, even if their predictions differ.
        for pred_index in range(first, stop):
            edge_index = offset + pred_index - first
            tree.include(len(prediction) - pred_index, counts[edge_index], costs[edge_index])
    count, cost = tree.best(len(prediction))
    return counts, costs, count, cost


def _lexicographic_pairs(
    reference: list[int],
    prediction: list[int],
    windows: _EligibleWindows,
    counts: array[int],
    costs: list[int],
    remaining_count: int,
    remaining_cost: int,
) -> tuple[tuple[int, int], ...]:
    pairs: list[tuple[int, int]] = []
    previous_prediction = -1
    for ref_index, value in enumerate(reference):
        if remaining_count == 0:
            break
        first = windows.first[ref_index]
        for pred_index in range(max(first, previous_prediction + 1), windows.stop[ref_index]):
            edge_index = windows.offset[ref_index] + pred_index - first
            if counts[edge_index] == remaining_count and costs[edge_index] == remaining_cost:
                pairs.append((ref_index, pred_index))
                previous_prediction = pred_index
                remaining_count -= 1
                remaining_cost -= abs(value - prediction[pred_index])
                break
    return tuple(pairs)


def _solve_matching(
    reference: list[int], prediction: list[int], windows: _EligibleWindows, scale: int
) -> BeatMatchingResult:
    counts, costs, count, cost = _suffix_scores(reference, prediction, windows)
    pairs = _lexicographic_pairs(reference, prediction, windows, counts, costs, count, cost)
    exact_error = Fraction(cost, scale)
    try:
        rounded_error = float(exact_error)
    except OverflowError:
        rounded_error = math.inf
    return BeatMatchingResult(pairs, rounded_error, exact_error)


def match_beats(
    reference_seconds: Sequence[float],
    prediction_seconds: Sequence[float],
    tolerance_seconds: float,
    *,
    max_eligible_edges: int = MAX_ELIGIBLE_EDGES,
) -> BeatMatchingResult:
    """Select the complete frozen-protocol monotone one-to-one matching.

    Args:
        reference_seconds: Finite, nonnegative, strictly increasing times.
        prediction_seconds: Finite, nonnegative, strictly increasing times.
        tolerance_seconds: Finite nonnegative inclusive timing tolerance.
        max_eligible_edges: A lower optional work cap, from zero to the hard
            MAX_ELIGIBLE_EDGES bound. It never changes a returned matching.

    Returns:
        Original input index pairs, the exact absolute error sum and its nearest
        binary64 representation. Empty matching has zero count and error.

    Raises:
        TypeError: An event or tolerance is not an integer or float, or is bool.
        ValueError: Invalid input values/order/tolerance or invalid edge cap.
        BeatMatchingLimitError: More than MAX_MATCHING_EVENTS events in either
            input, or more than max_eligible_edges eligible pairs. Rejection
            provides no truncated or approximate result.

    Work is O(N + M + E log M), memory O(N + M + E), for E eligible pairs.
    Validation is bounded by 250000 values per input; at most 1000000 edges
    receive scores. Binary64 integer costs have a finite bit-width (at most
    2116 bits including a 250000-term sum), even at subnormal/extreme times.
    The limits bound memory independently of N*M; no quadratic table exists.
    """
    if (
        isinstance(max_eligible_edges, bool)
        or not isinstance(max_eligible_edges, int)
        or not 0 <= max_eligible_edges <= MAX_ELIGIBLE_EDGES
    ):
        msg = f"max_eligible_edges must be an integer from 0 to {MAX_ELIGIBLE_EDGES}"
        raise ValueError(msg)
    reference = _validated_times(reference_seconds, "reference_seconds")
    prediction = _validated_times(prediction_seconds, "prediction_seconds")
    tolerance = _finite_nonnegative(tolerance_seconds, "tolerance_seconds")
    ref_units, pred_units, tolerance_units, scale = _dyadic_units(reference, prediction, tolerance)
    windows = _eligible_windows(ref_units, pred_units, tolerance_units, max_eligible_edges)
    return _solve_matching(ref_units, pred_units, windows, scale)
