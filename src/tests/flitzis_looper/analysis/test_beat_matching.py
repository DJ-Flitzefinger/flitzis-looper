"""Independent exact exhaustive and bounded-resource B2 matching checks."""

import math
from fractions import Fraction
from itertools import combinations, product
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.analysis.beat_matching import (
    MAX_ELIGIBLE_EDGES,
    MAX_MATCHING_EVENTS,
    BeatMatchingLimitError,
    match_beats,
)

if TYPE_CHECKING:
    from collections.abc import Sequence


def _oracle(
    reference: tuple[float, ...], prediction: tuple[float, ...], tolerance: float
) -> tuple[tuple[tuple[int, int], ...], Fraction]:
    """Enumerate all monotone subsequence pairs, independently of sparse DP."""
    references = tuple(Fraction(value) for value in reference)
    predictions = tuple(Fraction(value) for value in prediction)
    tolerance_exact = Fraction(tolerance)
    candidates: list[tuple[int, Fraction, tuple[tuple[int, int], ...]]] = []
    for count in range(min(len(reference), len(prediction)) + 1):
        for ref_indices, pred_indices in product(
            combinations(range(len(reference)), count),
            combinations(range(len(prediction)), count),
        ):
            pairs = tuple(zip(ref_indices, pred_indices, strict=True))
            errors = tuple(abs(references[i] - predictions[j]) for i, j in pairs)
            if all(error <= tolerance_exact for error in errors):
                candidates.append((-count, sum(errors, Fraction()), pairs))
    _, cost, pairs = min(candidates)
    return pairs, cost


def test_all_small_grid_subsets_match_independent_exhaustive_oracle() -> None:
    grid = (0.0, 0.25, 0.5, 0.75, 1.0)
    sequences = tuple(subset for count in range(4) for subset in combinations(grid, count))
    for reference, prediction, tolerance in product(sequences, sequences, (0.0, 0.125, 0.25, 0.5)):
        expected_pairs, expected_cost = _oracle(reference, prediction, tolerance)
        actual = match_beats(reference, prediction, tolerance)
        assert actual.pairs == expected_pairs, (reference, prediction, tolerance)
        assert actual.exact_absolute_error_seconds == expected_cost
        assert actual.total_absolute_error_seconds == float(expected_cost)


@pytest.mark.parametrize(
    ("reference", "prediction", "tolerance", "expected"),
    [
        ((0.25, 0.375), (0.0, 0.3125), 0.25, ((0, 0), (1, 1))),
        ((0.125, 1.0), (0.0, 0.125, 1.0), 0.125, ((0, 1), (1, 2))),
        ((1.0,), (0.75, 1.25), 0.25, ((0, 0),)),
        ((0.75, 1.25), (1.0,), 0.25, ((0, 0),)),
        ((0.25, 0.5, 0.75), (0.375, 0.625), 0.125, ((0, 0), (1, 1))),
    ],
)
def test_cardinality_cost_and_global_lexicographic_priorities(
    reference: tuple[float, ...],
    prediction: tuple[float, ...],
    tolerance: float,
    expected: tuple[tuple[int, int], ...],
) -> None:
    actual = match_beats(reference, prediction, tolerance)
    expected_pairs, expected_cost = _oracle(reference, prediction, tolerance)
    assert actual.pairs == expected == expected_pairs
    assert actual.exact_absolute_error_seconds == expected_cost


def test_exact_cost_beats_lexicographic_tie_hidden_by_float_summation() -> None:
    reference = (10.0, float(2**60))
    prediction = (0.0, 9.0, float(2**59))
    assert float(Fraction(2**59 + 10)) == float(Fraction(2**59 + 1))
    actual = match_beats(reference, prediction, float(2**60))
    assert actual.pairs == ((0, 1), (1, 2))
    assert actual.exact_absolute_error_seconds == Fraction(2**59 + 1)
    assert (actual.pairs, actual.exact_absolute_error_seconds) == _oracle(
        reference, prediction, float(2**60)
    )


@pytest.mark.parametrize(
    ("prediction", "expected"),
    [
        (math.nextafter(0.25, 0.0), ((0, 0),)),
        (0.25, ((0, 0),)),
        (math.nextafter(0.25, math.inf), ()),
    ],
)
def test_inclusive_tolerance_respects_adjacent_binary64_values(
    prediction: float, expected: tuple[tuple[int, int], ...]
) -> None:
    actual = match_beats((0.0,), (prediction,), 0.25)
    assert actual.pairs == expected
    assert (actual.pairs, actual.exact_absolute_error_seconds) == _oracle(
        (0.0,), (prediction,), 0.25
    )


def test_exact_boundary_rejects_distance_that_rounded_subtraction_hides() -> None:
    reference = math.nextafter(0.25, 0.0)
    assert 1.0 - reference == 0.75
    assert Fraction(1.0) - Fraction(reference) > Fraction(3, 4)
    assert match_beats((reference,), (1.0,), 0.75).pairs == ()
    assert match_beats((0.25,), (1.0,), 0.75).pairs == ((0, 0),)


def test_subnormal_and_extreme_values_preserve_exact_sum() -> None:
    minimum = math.ulp(0.0)
    reference = (0.0, minimum)
    prediction = (1e308, math.nextafter(1e308, math.inf))
    tolerance = math.nextafter(1e308, math.inf)
    actual = match_beats(reference, prediction, tolerance)
    expected_pairs, expected_cost = _oracle(reference, prediction, tolerance)
    assert actual.pairs == expected_pairs == ((0, 0), (1, 1))
    assert actual.exact_absolute_error_seconds == expected_cost
    assert actual.total_absolute_error_seconds == math.inf


def test_exact_dyadic_translation_preserves_pairs_and_cost() -> None:
    reference = (0.0, 0.25, 0.75, 1.0)
    prediction = (0.125, 0.5, 0.75, 1.125)
    original = match_beats(reference, prediction, 0.25)
    translated = match_beats(
        tuple(value + 32.0 for value in reference),
        tuple(value + 32.0 for value in prediction),
        0.25,
    )
    assert translated == original


@pytest.mark.parametrize(
    "invalid", [-1.0, math.nan, math.inf, -math.inf, True, False, "0.0", None, 10**400]
)
def test_invalid_event_and_tolerance_values_rejected(invalid: object) -> None:
    with pytest.raises((ValueError, TypeError), match="finite nonnegative"):
        match_beats((cast("float", invalid),), (), 0.1)
    with pytest.raises((ValueError, TypeError), match="finite nonnegative"):
        match_beats((), (cast("float", invalid),), 0.1)
    with pytest.raises((ValueError, TypeError), match="finite nonnegative"):
        match_beats((), (), cast("float", invalid))


@pytest.mark.parametrize("invalid", [(1.0, 1.0), (1.0, 0.0), (-0.0, 0.0)])
def test_nonincreasing_times_rejected(invalid: tuple[float, ...]) -> None:
    with pytest.raises(ValueError, match="strictly increasing"):
        match_beats(invalid, (), 0.1)
    with pytest.raises(ValueError, match="strictly increasing"):
        match_beats((), invalid, 0.1)


@pytest.mark.parametrize("invalid", [-1, MAX_ELIGIBLE_EDGES + 1, 1.0, True, "10", None])
def test_invalid_edge_cap_rejected(invalid: object) -> None:
    with pytest.raises(ValueError, match="max_eligible_edges must be an integer"):
        match_beats((), (), 0.0, max_eligible_edges=cast("int", invalid))


def test_input_event_bound_rejects_before_accessing_oversize_sequence() -> None:
    class Oversize:
        def __len__(self) -> int:
            return MAX_MATCHING_EVENTS + 1

        def __getitem__(self, index: int) -> float:
            msg = "oversize input must never be read"
            raise AssertionError(msg)

    values = cast("Sequence[float]", Oversize())
    with pytest.raises(BeatMatchingLimitError, match="250000 events"):
        match_beats(values, (), 0.0)
    with pytest.raises(BeatMatchingLimitError, match="250000 events"):
        match_beats((), values, 0.0)


def test_dense_edge_failure_is_explicit_and_never_a_partial_matching() -> None:
    with pytest.raises(BeatMatchingLimitError, match="eligible pairs exceed 3"):
        match_beats((0.0, 0.25), (0.0, 0.25), 1.0, max_eligible_edges=3)
    complete = match_beats((0.0, 0.25), (0.0, 0.25), 1.0, max_eligible_edges=4)
    assert complete.pairs == ((0, 0), (1, 1))
    assert complete.exact_absolute_error_seconds == 0


def test_hard_dense_edge_limit_rejects_without_allocating_quadratic_scores() -> None:
    values = tuple(float(index) for index in range(1001))
    with pytest.raises(BeatMatchingLimitError, match="eligible pairs exceed 1000000"):
        match_beats(values, values, 1001.0)


@pytest.mark.parametrize(
    ("reference", "prediction"),
    [((), ()), ((), (0.0, 1.0)), ((0.0, 1.0), ()), ((0.0,), (1.0,))],
)
def test_empty_and_ineligible_inputs_return_exact_zero(
    reference: tuple[float, ...], prediction: tuple[float, ...]
) -> None:
    actual = match_beats(reference, prediction, 0.0, max_eligible_edges=0)
    assert actual.pairs == ()
    assert actual.total_absolute_error_seconds == 0.0
    assert actual.exact_absolute_error_seconds == Fraction()


def test_full_allowed_sparse_extent_has_no_quadratic_input_product() -> None:
    # A quadratic table would have 62.5 billion cells. Only one eligible edge
    # per row exists, and the exact original indices must survive at both ends.
    values = tuple(float(index) for index in range(MAX_MATCHING_EVENTS))
    actual = match_beats(values, values, 0.0)
    assert len(actual.pairs) == MAX_MATCHING_EVENTS
    assert all(pair == (index, index) for index, pair in enumerate(actual.pairs))
    assert actual.exact_absolute_error_seconds == 0


def test_large_disjoint_extent_allows_zero_edge_budget() -> None:
    reference = tuple(float(index) for index in range(MAX_MATCHING_EVENTS))
    prediction = tuple(float(index + MAX_MATCHING_EVENTS) for index in range(MAX_MATCHING_EVENTS))
    actual = match_beats(reference, prediction, 0.0, max_eligible_edges=0)
    assert actual.pairs == ()
    assert actual.exact_absolute_error_seconds == 0
