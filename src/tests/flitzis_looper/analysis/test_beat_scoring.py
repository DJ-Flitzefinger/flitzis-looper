"""Frozen temporal metrics are engineering evidence, never musical acceptance."""

import json
import math
from dataclasses import asdict
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis.beat_scoring import TimingLabel, score_timing_events
from flitzis_looper.analysis.reference_inputs_models import Region

if TYPE_CHECKING:
    from flitzis_looper.analysis.beat_scoring import ToleranceResult


def _regions(duration: float = 5.0) -> tuple[Region, ...]:
    return (
        Region(
            start_seconds=0.0,
            end_seconds=duration,
            kind="metrical",
            beat_unit_quarters=1.0,
            provenance="independent synthetic fixture",
        ),
    )


def _labels(*seconds: float, width: float = 1.0) -> tuple[TimingLabel, ...]:
    return tuple(TimingLabel(value, width) for value in seconds)


def _score(labels: tuple[TimingLabel, ...], predictions: tuple[float, ...]) -> ToleranceResult:
    return score_timing_events(labels, predictions, _regions(), 5.0).tolerances[2]


def test_perfect_synthetic_metrics_retain_unchecked_pending_and_blocked() -> None:
    report = score_timing_events(_labels(0.0, 0.5, 1.0, 1.5), (0.0, 0.5, 1.0, 1.5), _regions(), 5.0)
    assert report.input_certification == "unchecked_by_metric_core"
    assert report.musical_acceptance == "pending"
    assert report.default_adoption == "blocked"
    assert all(result.f1 == 1.0 for result in report.tolerances)
    assert report.confident_temporal_fraction == report.trusted_metrical_fraction == 1.0
    # The report can be serialized without NaN, native imports or an artifact read.
    json.dumps(asdict(report), allow_nan=False)


def test_ineligible_reference_does_not_remove_nearby_prediction_from_denominator() -> None:
    result = _score(
        (TimingLabel(0.0, 1.0), TimingLabel(1.0, 11.0), TimingLabel(2.0, 1.0)), (0.0, 1.0, 2.0)
    )
    assert result.eligible_reference_indices == (0, 2)
    assert result.ineligible_reference_indices == (1,)
    assert result.evaluated_prediction_indices == (0, 1, 2)
    assert result.extra_prediction_indices == (1,)
    assert result.f1 == 0.8
    assert result.extra_fraction_of_eligible_reference == 0.5
    assert result.longest_matched_run is not None
    assert result.longest_matched_run.event_count == 1


@pytest.mark.parametrize(
    ("width", "eligible"),
    [
        (2.5, (True, True, True, True)),
        (2.500001, (False, True, True, True)),
        (5.0, (False, True, True, True)),
        (5.000001, (False, False, True, True)),
        (10.0, (False, False, True, True)),
        (10.000001, (False, False, False, False)),
    ],
)
def test_frozen_halfwidth_limits_have_separate_eligibility_denominators(
    width: float,
    eligible: tuple[bool, ...],
) -> None:
    report = score_timing_events(_labels(1.0, width=width), (1.0,), _regions(), 5.0)
    assert (
        tuple(bool(result.eligible_reference_indices) for result in report.tolerances) == eligible
    )
    assert tuple(result.eligible_uncertainty_halfwidth_ms for result in report.tolerances) == (
        2.5,
        5.0,
        10.0,
        10.0,
    )


def test_signed_bias_quantiles_and_interval_distances_describe_same_point_pairs() -> None:
    result = _score(_labels(1.0, 2.0, 3.0, width=2.0), (0.984375, 2.0, 3.03125))
    assert result.errors.signed_bias_ms == pytest.approx(15.625 / 3)
    assert result.errors.absolute_p50_ms == 15.625
    assert result.errors.absolute_p95_ms == pytest.approx(29.6875)
    assert result.errors.absolute_max_ms == 31.25
    assert tuple(match.interval_distance_ms for match in result.matches) == (13.625, 0.0, 29.25)
    assert result.interval_distance_errors_on_point_matches.max_ms == 29.25


def test_inclusive_binary64_tolerance_and_one_ulp_outside_are_distinct() -> None:
    exact = _score(_labels(0.0), (0.04,))
    outside = _score(_labels(0.0), (math.nextafter(0.04, math.inf),))
    assert exact.f1 == 1.0
    assert outside.f1 == 0.0
    assert outside.missing_reference_indices == outside.extra_prediction_indices == (0,)


def test_fixed_nonmetrical_regions_exclude_only_their_original_predictions() -> None:
    regions = (
        Region(
            start_seconds=0.0,
            end_seconds=2.0,
            kind="metrical",
            beat_unit_quarters=1.0,
            provenance="blind declaration",
        ),
        Region(
            start_seconds=2.0,
            end_seconds=3.0,
            kind="ambiguous",
            beat_unit_quarters=None,
            provenance="blind declaration",
        ),
        Region(
            start_seconds=3.0,
            end_seconds=5.0,
            kind="metrical",
            beat_unit_quarters=1.0,
            provenance="blind declaration",
        ),
    )
    report = score_timing_events(_labels(1.0, 3.0, 4.0), (1.0, 2.0, 2.5, 3.0, 4.0), regions, 5.0)
    result = report.tolerances[2]
    assert result.evaluated_prediction_indices == (0, 3, 4)
    assert result.excluded_nonmetrical_prediction_indices == (1, 2)
    assert result.f1 == 1.0
    assert report.confident_temporal_fraction == report.trusted_metrical_fraction == 0.8
    assert result.regions[1].predictions == 2
    assert result.regions[1].extra == 0
    assert result.longest_matched_run is not None
    assert (
        result.longest_matched_run.first_reference_index,
        result.longest_matched_run.last_reference_index,
    ) == (1, 2)


def test_global_matching_can_cross_short_predeclared_gap_and_reports_both_endpoints() -> None:
    regions = (
        Region(
            start_seconds=0.0,
            end_seconds=1.0,
            kind="metrical",
            beat_unit_quarters=1.0,
            provenance="blind declaration",
        ),
        Region(
            start_seconds=1.0,
            end_seconds=1.01,
            kind="nonrhythmic",
            beat_unit_quarters=None,
            provenance="blind declaration",
        ),
        Region(
            start_seconds=1.01,
            end_seconds=5.0,
            kind="metrical",
            beat_unit_quarters=1.0,
            provenance="blind declaration",
        ),
    )
    report = score_timing_events(_labels(0.99, 1.1), (1.02,), regions, 5.0)
    result = report.tolerances[2]
    assert tuple((match.reference_index, match.prediction_index) for match in result.matches) == (
        (0, 0),
    )
    assert result.regions[0].matched_reference_events == 1
    assert result.regions[2].matched_predictions == 1
    assert result.regions[0].cross_region_matches == result.regions[2].cross_region_matches == 1
    assert report.confident_temporal_fraction == 1.0


def test_missing_runs_stop_at_ineligible_labels_and_keep_original_indices() -> None:
    labels = tuple(TimingLabel(float(i), 11.0 if i == 2 else 1.0) for i in range(5))
    result = _score(labels, ())
    assert result.missing_reference_indices == (0, 1, 3, 4)
    assert tuple(
        (run.first_reference_index, run.last_reference_index, run.event_count)
        for run in result.unmatched_eligible_runs
    ) == ((0, 1, 2), (3, 4, 2))
    assert result.precision is None
    assert result.recall == result.f1 == 0.0


def test_extra_predictions_and_missing_reference_split_longest_correct_run() -> None:
    result = _score(_labels(0.0, 0.5, 1.0, 1.5, 2.0), (0.0, 0.25, 0.5, 1.5, 2.0))
    assert result.extra_prediction_indices == (1,)
    assert result.missing_reference_indices == (2,)
    assert result.longest_matched_run is not None
    assert result.longest_matched_run.first_reference_index == 3
    assert result.longest_matched_run.event_count == 2
    assert result.longest_matched_run.duration_seconds == 0.5


def test_longest_run_uses_endpoint_duration_then_count_then_earliest() -> None:
    result = _score(_labels(0.0, 0.125, 0.25, 1.0, 2.0, 3.0), (0.0, 0.125, 0.25, 2.0, 3.0))
    assert result.longest_matched_run is not None
    assert result.longest_matched_run.first_reference_index == 4
    assert result.longest_matched_run.event_count == 2
    assert result.longest_matched_run.duration_seconds == 1.0
    tied = _score(_labels(0.0, 1.0, 2.0, 3.0, 4.0), (0.0, 1.0, 3.0, 4.0))
    assert tied.longest_matched_run is not None
    assert tied.longest_matched_run.first_reference_index == 0


def test_empty_series_and_all_ineligible_reference_have_no_fabricated_ratios() -> None:
    empty = _score((), ())
    assert empty.precision is empty.recall is empty.f1 is None
    assert empty.errors.absolute_p50_ms is empty.errors.signed_bias_ms is None
    broad = _score(_labels(1.0, width=1000.0), (1.0,))
    assert broad.extra_prediction_indices == (0,)
    assert broad.recall is broad.missing_fraction_of_eligible_reference is None
    assert broad.extra_fraction_of_eligible_reference is None
    assert broad.precision == broad.f1 == 0.0


@pytest.mark.parametrize(
    ("labels", "predictions"),
    [
        (_labels(1.0, 1.0), ()),
        (_labels(-1.0), ()),
        (_labels(5.0), ()),
        (_labels(math.nan), ()),
        (_labels(1.0, width=-1.0), ()),
        (_labels(1.0, width=1001.0), ()),
        (_labels(1.0), (1.0, 1.0)),
        (_labels(1.0), (math.inf,)),
        (_labels(1.0), (5.0,)),
    ],
)
def test_invalid_times_widths_or_tail_do_not_produce_partial_scores(
    labels: tuple[TimingLabel, ...],
    predictions: tuple[float, ...],
) -> None:
    with pytest.raises(ValueError, match=r"strictly increasing|finite|half-width"):
        _score(labels, predictions)


@pytest.mark.parametrize("kind", ["ambiguous", "nonrhythmic"])
def test_reference_labels_in_nonmetrical_regions_are_rejected(kind: str) -> None:
    region = _regions()[0].model_copy(update={"kind": kind, "beat_unit_quarters": None})
    with pytest.raises(ValueError, match="metrical"):
        score_timing_events(_labels(1.0), (1.0,), (region,), 5.0)


def test_extent_gap_and_incomplete_tail_are_rejected_without_offset_fitting() -> None:
    for region in (
        _regions()[0].model_copy(update={"start_seconds": 0.001}),
        _regions()[0].model_copy(update={"end_seconds": 4.0}),
    ):
        with pytest.raises(ValueError, match=r"tile|tail"):
            score_timing_events(_labels(1.0), (1.0,), (region,), 5.0)


def test_whole_dense_input_refuses_scoring_instead_of_returning_truncated_pass() -> None:
    events = tuple(index / 100000.0 for index in range(1001))
    with pytest.raises(ValueError, match="eligible pairs"):
        score_timing_events(_labels(*events), events, _regions(), 5.0)


def test_event_limits_are_checked_before_allocating_or_reading_sequence() -> None:
    class OversizedEvents:
        def __len__(self) -> int:
            return 250001

        def __getitem__(self, _index: int) -> float:
            pytest.fail("oversized sequence must never be read")

    with pytest.raises(ValueError, match="count exceeds"):
        score_timing_events((), OversizedEvents(), _regions(), 5.0)  # type: ignore[arg-type]


def test_huge_integer_is_rejected_as_invalid_binary64_not_overflow() -> None:
    with pytest.raises(ValueError, match="finite binary64"):
        _score(_labels(10**1000), ())


def test_longest_run_splits_when_prediction_region_changes_across_gap() -> None:
    regions = (
        Region(
            start_seconds=0.0,
            end_seconds=1.0,
            kind="metrical",
            beat_unit_quarters=1.0,
            provenance="blind declaration",
        ),
        Region(
            start_seconds=1.0,
            end_seconds=1.01,
            kind="nonrhythmic",
            beat_unit_quarters=None,
            provenance="blind declaration",
        ),
        Region(
            start_seconds=1.01,
            end_seconds=5.0,
            kind="metrical",
            beat_unit_quarters=1.0,
            provenance="blind declaration",
        ),
    )
    result = score_timing_events(_labels(1.02, 1.5), (0.99, 1.5), regions, 5.0).tolerances[2]
    assert len(result.matches) == 2
    assert result.longest_matched_run is not None
    assert result.longest_matched_run.event_count == 1
