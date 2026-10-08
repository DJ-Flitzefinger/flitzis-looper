"""Complete engineering disagreement regressions preserve unverified musical gates."""

import copy
import hashlib
import json
import math
import struct
from dataclasses import replace
from fractions import Fraction
from pathlib import Path
from typing import cast

import pytest

from flitzis_looper.analysis.beat_candidate_models import NativeCandidate
from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatModelIdentity,
    BeatPredictions,
    BeatWorkerRequest,
    MonoPcmInput,
)
from flitzis_looper.analysis.corrected_legacy_evaluation import (
    array_identity,
    compare_event_arrays,
    engineering_compare,
    ordinal_fit,
)
from flitzis_looper.analysis.corrected_legacy_models import (
    CorrectedLegacyCandidate,
    decode_corrected_legacy,
)
from tests.flitzis_looper.analysis.test_corrected_legacy_models import (
    encoded,
    legacy_fixture,
    object_fields,
)


def comparison_fixture() -> tuple[NativeCandidate, CorrectedLegacyCandidate]:
    """Create independently synthetic same-source full arrays; no producer registration."""
    result = decode_corrected_legacy(encoded(legacy_fixture()))
    loaded = result.envelope.loaded
    prediction = BeatPredictions(
        result.beat_seconds,
        result.downbeat_seconds,
        (-0.0, 0.1234567890123456, 0.5, 1.0),
        (0.0, -0.25, 0.5, 0.75),
    )
    request = BeatWorkerRequest(
        AnalysisIdentity(0, 3, "loaded-0-1", 1),
        MonoPcmInput(Path("complete.f32le"), loaded.sample_rate_hz, loaded.frame_count),
        BeatModelIdentity(),
    )
    selected = NativeCandidate(
        "synthetic-selected",
        "T01",
        "fresh_native_v2",
        "synthetic-original",
        "c" * 64,
        123,
        loaded.mono_sha256,
        2,
        request,
        prediction,
        (),
        (),
    )
    legacy = CorrectedLegacyCandidate("synthetic-legacy", "T01", "c" * 64, 123, result, ())
    return selected, legacy


def walk_keys(value: object) -> set[str]:
    """Inspect complete nested report fields for accidental musical score claims."""
    if isinstance(value, dict):
        return set(value) | set().union(*(walk_keys(v) for v in value.values()))
    if isinstance(value, (tuple, list)):
        return set().union(*(walk_keys(v) for v in value))
    return set()


def object_rows(value: object) -> tuple[dict[str, object], ...]:
    """Assert the dynamic engineering report's list/row shapes before checking it."""
    assert isinstance(value, (list, tuple))
    return tuple(object_fields(item) for item in value)


def float_values(value: object) -> tuple[float, ...]:
    """Require unchanged numeric tuples rather than hiding missing array values."""
    assert isinstance(value, tuple)
    assert all(isinstance(v, float) for v in value)
    return cast("tuple[float, ...]", value)


def index_pairs(value: object) -> tuple[tuple[int, int], ...]:
    """Require all original integer pair identities in the dynamic matcher report."""
    assert isinstance(value, tuple)
    for pair in value:
        assert isinstance(pair, tuple)
        assert len(pair) == 2
        assert all(type(index) is int for index in pair)
    return cast("tuple[tuple[int, int], ...]", value)


def test_every_original_array_and_unverified_gate_is_retained_without_musical_scores() -> None:
    selected, legacy = comparison_fixture()
    originals = copy.deepcopy((selected.report(), legacy.report()))
    report = engineering_compare(selected, legacy)
    assert report["status"] == "complete_engineering_comparison"
    assert report["selected"] == originals[0]
    assert report["legacy"] == originals[1]
    assert report["musical_scores"] == "not_run"
    assert report["independent_reference"] == "not_supplied"
    assert report["paired_human_correction"] == "not_measured"
    assert report["resource_acceptance"] == "not_established"
    assert report["default_adoption"] == "blocked"
    assert not {"f1", "recall", "musical_acceptance_passed"} & walk_keys(report)
    assert all("precision" not in row for row in object_rows(report["beat_disagreement"]))
    assert (selected.report(), legacy.report()) == originals
    json.dumps(report, allow_nan=False)


@pytest.mark.parametrize(
    "name", ["beat_seconds", "downbeat_seconds", "beat_logits", "downbeat_logits"]
)
def test_selected_identity_binds_last_binary64_value_of_each_complete_array(name: str) -> None:
    selected, legacy = comparison_fixture()
    before = object_fields(
        object_fields(
            object_fields(engineering_compare(selected, legacy)["complete_array_identities"])[
                "selected"
            ]
        )[name]
    )
    values = getattr(selected.predictions, name)
    changed = (*values[:-1], math.nextafter(values[-1], math.inf))
    altered = replace(selected, predictions=replace(selected.predictions, **{name: changed}))
    after = object_fields(
        object_fields(
            object_fields(engineering_compare(altered, legacy)["complete_array_identities"])[
                "selected"
            ]
        )[name]
    )
    assert before["count"] == after["count"] == len(values)
    assert before["sha256"] != after["sha256"]
    assert getattr(selected.predictions, name) == values


@pytest.mark.parametrize(
    ("code", "values"),
    [("d", (-0.0, 0.0, 1.2345678901234567)), ("f", (-0.0, 0.0, 1.25)), ("Q", (0, 2, 2**64 - 1))],
)
def test_array_identity_uses_declared_complete_ieee_or_integer_bytes(
    code: str, values: tuple[float, ...] | tuple[int, ...]
) -> None:
    result = array_identity(values, code)
    assert result["count"] == len(values)
    assert (
        result["sha256"]
        == hashlib.sha256(struct.pack(f"<{len(values)}{code}", *values)).hexdigest()
    )


@pytest.mark.parametrize(
    "field",
    ["track_id", "source_sha256", "source_bytes", "pcm_sha256", "sample_rate_hz", "frame_count"],
)
def test_comparison_rejects_any_original_or_complete_pcm_identity_difference(field: str) -> None:
    selected, legacy = comparison_fixture()
    if field == "track_id":
        selected = replace(selected, track_id="T02")
    elif field == "source_sha256":
        selected = replace(selected, source_sha256="d" * 64)
    elif field == "source_bytes":
        selected = replace(selected, source_bytes=124)
    elif field == "pcm_sha256":
        selected = replace(selected, pcm_sha256="d" * 64)
    else:
        selected = replace(
            selected,
            request=replace(
                selected.request,
                pcm=replace(
                    selected.request.pcm, **{field: getattr(selected.request.pcm, field) + 1}
                ),
            ),
        )
    with pytest.raises(ValueError, match="identical complete original"):
        engineering_compare(selected, legacy)


def test_symmetric_disagreement_retains_indices_signs_and_all_unmatched_events() -> None:
    left = (0.0, 0.5, 1.0, 2.0)
    right = (0.015625, 0.5, 1.03125, 1.5, 2.0, 3.0)
    forward, reverse = compare_event_arrays(left, right), compare_event_arrays(right, left)
    assert tuple(row["tolerance_ms"] for row in forward) == (10, 20, 40, 70)
    for before, after in zip(forward, reverse, strict=True):
        assert before["matched_original_index_pairs"] == tuple(
            (b, a) for a, b in index_pairs(after["matched_original_index_pairs"])
        )
        assert before["left_unmatched_indices"] == after["right_unmatched_indices"]
        assert before["right_unmatched_indices"] == after["left_unmatched_indices"]
        assert before["signed_right_minus_left_errors_ms"] == tuple(
            -v for v in float_values(after["signed_right_minus_left_errors_ms"])
        )
        assert before["absolute_p50_ms"] == after["absolute_p50_ms"]
        assert before["absolute_max_ms"] == after["absolute_max_ms"]
    assert forward[2]["right_unmatched_indices"] == (3, 5)
    assert forward[2]["matched_original_index_pairs"] == ((0, 0), (1, 1), (2, 2), (3, 4))


def test_empty_backend_comparison_keeps_all_other_backend_events_unmatched() -> None:
    rows = compare_event_arrays((), (0.0, 1.0, 5.0))
    assert all(row["matched_original_index_pairs"] == () for row in rows)
    assert all(row["left_events"] == 0 and row["right_events"] == 3 for row in rows)
    assert all(row["right_unmatched_indices"] == (0, 1, 2) for row in rows)
    assert all(row["signed_bias_ms"] is None for row in rows)


def test_full_equal_weight_ordinal_fit_has_independently_known_fractional_solution() -> None:
    times = (0.125, 0.625, 1.125, 1.75)
    fit = ordinal_fit(times)
    assert fit["status"] == "unverified"
    assert fit["observations"] == 4
    assert fit["period_seconds_per_ordinal"] == float(Fraction(43, 80))
    assert fit["diagnostic_intercept_seconds"] == float(Fraction(1, 10))
    assert fit["residual_seconds"] == (0.025, -0.0125, -0.05, 0.0375)
    assert fit["ordinal_bpm"] == float(Fraction(4800, 43))
    assert fit["acoustic_timing_bound"] == "not_established"
    late_changed = ordinal_fit((*times[:-1], times[-1] + 0.125))
    assert late_changed["period_seconds_per_ordinal"] != fit["period_seconds_per_ordinal"]
    assert len(float_values(late_changed["residual_seconds"])) == len(times)


@pytest.mark.parametrize("times", [(), (0.0,)])
def test_ordinal_fit_never_fabricates_a_period_from_insufficient_events(
    times: tuple[float, ...],
) -> None:
    assert ordinal_fit(times) == {
        "status": "unsupported",
        "reason": "fewer_than_two_complete_events",
    }


def test_regions_preserve_indices_into_whole_arrays_and_detector_bounds_stay_conditional() -> None:
    selected, legacy = comparison_fixture()
    report = engineering_compare(selected, legacy)
    regions = object_rows(report["predeclared_regions"])
    assert tuple(region["id"] for region in regions) == ("middle", "early", "central", "late")
    assert regions[0]["start_seconds"] == 2.0
    assert regions[0]["end_seconds"] == 8.0
    for region in regions:
        start, end = region["start_seconds"], region["end_seconds"]
        assert isinstance(start, float)
        assert isinstance(end, float)
        expected = tuple(
            i for i, value in enumerate(legacy.result.beat_seconds) if start <= value < end
        )
        assert region["legacy_original_indices"] == expected
        assert region["selected_original_indices"] == expected
    assert report["legacy_detector_lattice_halfwidth_seconds"] == 512 / 88200
    assert report["selected_detector_lattice_halfwidth_seconds"] == 0.01
    assert report["detector_lattice_claim"] == "conditional only; no acoustic or musical bound"


@pytest.mark.parametrize(
    "times",
    [
        (-0.5, 0.0),
        (0.0, math.nan),
        (0.0, math.inf),
        (0.0, -math.inf),
        (0.5, 0.5),
        (1.0, 0.5),
        (False, 1.0),
        (0.0, True),
    ],
)
def test_ordinal_fit_rejects_invalid_complete_time_domains(times: tuple[float, ...]) -> None:
    with pytest.raises(ValueError, match="finite complete increasing nonnegative"):
        ordinal_fit(times)


def test_regional_beat_and_downbeat_reports_keep_full_index_maps_and_local_fits() -> None:
    selected, legacy = comparison_fixture()
    times = (0.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0)
    result = decode_corrected_legacy(
        encoded(
            legacy_fixture(frames=tuple(t * (44100 / 512) for t in times), indices=(0, 2, 4, 6, 8))
        )
    )
    legacy = replace(legacy, result=result)
    selected = replace(
        selected,
        predictions=replace(
            selected.predictions,
            beat_seconds=result.beat_seconds,
            downbeat_seconds=result.downbeat_seconds,
        ),
    )
    originals = (selected.report(), legacy.report())
    regions = object_rows(engineering_compare(selected, legacy)["predeclared_regions"])
    middle = regions[0]
    beats, downbeats = object_fields(middle["beats"]), object_fields(middle["downbeats"])
    assert (
        beats["legacy_original_indices"] == beats["selected_original_indices"] == (1, 2, 3, 4, 5, 6)
    )
    assert (
        downbeats["legacy_original_indices"] == downbeats["selected_original_indices"] == (1, 2, 3)
    )
    assert object_fields(beats["legacy_ordinal_fit"])["period_seconds_per_ordinal"] == 1.0
    assert object_fields(downbeats["legacy_ordinal_fit"])["period_seconds_per_ordinal"] == 2.0
    assert beats["legacy_local_interval_seconds"] == (1.0,) * 5
    for series, count in ((beats, 6), (downbeats, 3)):
        for row in object_rows(series["disagreement_with_regional_indices"]):
            assert row["matched_original_index_pairs"] == tuple((i, i) for i in range(count))
            assert row["left_unmatched_indices"] == row["right_unmatched_indices"] == ()
        assert (
            series["cross_boundary_matches"] == "reported separately by complete-track comparison"
        )
    assert (selected.report(), legacy.report()) == originals


def test_empty_predeclared_region_keeps_unsupported_fit_and_complete_arrays() -> None:
    selected, legacy = comparison_fixture()
    report = engineering_compare(selected, legacy)
    late = object_rows(report["predeclared_regions"])[3]
    for name in ("beats", "downbeats"):
        series = object_fields(late[name])
        assert series["legacy_original_indices"] == series["selected_original_indices"] == ()
        assert object_fields(series["legacy_ordinal_fit"])["status"] == "unsupported"
        assert series["legacy_local_interval_seconds"] == ()
        assert all(
            row["left_events"] == row["right_events"] == 0
            for row in object_rows(series["disagreement_with_regional_indices"])
        )
    assert report["legacy"] == legacy.report()


def test_complete_downbeat_fit_and_intervals_use_all_original_events_without_meter_claim() -> None:
    selected, legacy = comparison_fixture()
    result = decode_corrected_legacy(
        encoded(
            legacy_fixture(
                tuple(float(i * 43) for i in range(1200)),
                tuple(range(0, 1200, 4)),
                duration_seconds=1000,
            )
        )
    )
    legacy = replace(legacy, result=result)
    selected = replace(
        selected,
        request=replace(
            selected.request,
            pcm=replace(selected.request.pcm, frame_count=result.envelope.loaded.frame_count),
        ),
        predictions=replace(
            selected.predictions,
            beat_seconds=result.beat_seconds,
            downbeat_seconds=result.downbeat_seconds,
        ),
    )
    original = result.downbeat_seconds
    ys = tuple(Fraction(t) for t in original)
    n = len(ys)
    mean_ordinal = Fraction(n - 1, 2)
    expected_period = Fraction(12, n * (n * n - 1)) * sum(
        (i - mean_ordinal) * t for i, t in enumerate(ys)
    )
    report = engineering_compare(selected, legacy)
    expected_intervals = tuple(original[i + 1] - original[i] for i in range(n - 1))
    for prefix in ("legacy", "selected"):
        fit = object_fields(report[f"{prefix}_complete_downbeat_ordinal_fit"])
        assert fit["observations"] == n == 300
        assert fit["period_seconds_per_ordinal"] == float(expected_period)
        assert len(float_values(fit["residual_seconds"])) == n
        assert fit["count_interpretation"] == "original-event-ordinal; quarter units unverified"
        assert fit["status"] == "unverified"
        assert fit["acoustic_timing_bound"] == "not_established"
        assert report[f"{prefix}_downbeat_local_interval_seconds"] == expected_intervals
    altered_times = (*original[:-1], original[-1] + 0.0625)
    altered = replace(
        selected, predictions=replace(selected.predictions, downbeat_seconds=altered_times)
    )
    changed = engineering_compare(altered, legacy)
    changed_fit = object_fields(changed["selected_complete_downbeat_ordinal_fit"])
    assert changed_fit["period_seconds_per_ordinal"] != float(expected_period)
    assert (
        float_values(changed["selected_downbeat_local_interval_seconds"])[-1]
        != expected_intervals[-1]
    )
    assert (
        changed["legacy_complete_downbeat_ordinal_fit"]
        == report["legacy_complete_downbeat_ordinal_fit"]
    )
    assert result.downbeat_seconds == original
    assert changed["musical_scores"] == "not_run"
    assert changed["default_adoption"] == "blocked"
