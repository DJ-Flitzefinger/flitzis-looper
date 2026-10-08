"""Numerical regressions for unaccepted full-sequence BPM metadata."""

import math
import struct
from dataclasses import asdict, replace
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatComponentResult,
    BeatModelIdentity,
    BeatPredictions,
    BeatWorkerRequest,
    MonoPcmInput,
)
from flitzis_looper.analysis.selected_bpm import assess_beat_sequence, summarize_beats

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.analysis.contracts import ComponentStatus


def _binary64(values: tuple[float, ...]) -> bytes:
    return struct.pack(f"<{len(values)}d", *values)


def _beats(period: float, count: int = 200, origin: float = 0.003) -> tuple[float, ...]:
    return tuple(origin + index * period for index in range(count))


@pytest.fixture
def beat_request(tmp_path: Path) -> BeatWorkerRequest:
    # Numerical metadata uses the retired request extent, never its deleted PCM file.
    return BeatWorkerRequest(
        AnalysisIdentity(0, 42, "immutable-complete-source", 7),
        MonoPcmInput(tmp_path / "retired-mono.f32le", 48_000, 48_000 * 150),
        BeatModelIdentity(
            sha256="a" * 64,
            frontend_id="beat-this-1.1.0-final0-minimal-fixture",
            environment_id="frozen-cpu-fp32-fixture",
        ),
    )


def _component(beat_request: BeatWorkerRequest) -> BeatComponentResult:
    return BeatComponentResult(
        beat_request.identity,
        beat_request.model,
        "ready",
        "",
        BeatPredictions(
            _beats(0.73),
            _beats(0.73)[::4],
            (-0.0, math.ulp(0.0), -math.ulp(0.0), 0.12345678901234568),
            (1.0000000000000002, -1.0000000000000002, 2.5, -3.75),
        ),
    )


@pytest.mark.parametrize("period", [0.73, 60.0 / 123.456789, 0.500000000123])
@pytest.mark.parametrize("rate", [44_100, 48_000, 96_000])
def test_complete_sequence_preserves_fractional_period_and_explicit_uncertainty(
    period: float, rate: int
) -> None:
    beats = _beats(period)
    frames = math.ceil((beats[-1] + period) * rate)

    result = assess_beat_sequence(beats, rate, frames)

    assert result.complete_fit is not None
    assert result.complete_fit.assigned_observations == len(beats)
    assert result.complete_fit.period_seconds_per_quarter == pytest.approx(period, abs=1e-13)
    assert result.complete_fit.bpm == pytest.approx(60.0 / period, abs=1e-10)
    assert result.complete_fit.bpm != round(result.complete_fit.bpm)
    assert result.global_status == "unverified"
    assert result.global_fit is not None
    assert result.global_fit.period_seconds_per_quarter == pytest.approx(period, abs=1e-13)
    assert result.global_policy_version == "constant-period-candidate-v1"
    assert result.policy_version == "selected-backend-bpm-v1"
    assert result.region_policy_version == "representative-middle-region-v1"
    assert result.sample_rate_hz == rate
    assert result.frame_count == frames
    assert _binary64(result.raw_beat_seconds) == _binary64(beats)
    assert len(result.local_interval_seconds) == len(beats) - 1
    assert result.local_interval_bpm == pytest.approx((60.0 / period,) * (len(beats) - 1))
    assert result.alternatives_bpm.half == pytest.approx(30.0 / period)
    assert result.alternatives_bpm.ordinal == pytest.approx(60.0 / period)
    assert result.alternatives_bpm.double == pytest.approx(120.0 / period)


def test_complete_ols_retains_an_outlier_while_g2_diagnostics_exclude_it() -> None:
    beats = list(_beats(0.5))
    beats[20] += 0.1

    result = assess_beat_sequence(tuple(beats), 48_000, 48_000 * 100)

    # Analytic OLS slope perturbation for one changed point, all 200 equal weights.
    expected_period = 0.5 + (20.0 - 99.5) * 0.1 / (200 * (200**2 - 1) / 12)
    assert result.complete_fit is not None
    assert result.complete_fit.assigned_observations == 200
    assert result.complete_fit.period_seconds_per_quarter == pytest.approx(expected_period)
    assert result.complete_fit.max_abs_residual_seconds > 0.09
    assert result.global_status == "unverified"
    assert result.global_fit is not None
    assert result.global_fit.period_seconds_per_quarter == pytest.approx(0.5)
    assert result.global_excluded_raw_indices == (20,)
    assert len(result.global_residual_seconds) == len(beats)
    assert result.raw_beat_seconds[20] == beats[20]


def test_sparse_edges_have_only_scoped_representative_metadata() -> None:
    beats = tuple(20.0 + index * 0.5 for index in range(120))

    result = assess_beat_sequence(beats, 48_000, 48_000 * 100)

    assert result.global_status == "unsupported"
    assert result.selected_region_id == "middle"
    assert result.representative_bpm == pytest.approx(120.0)
    assert result.complete_fit is not None
    assert result.complete_fit.assigned_observations == 120
    middle = next(region for region in result.regions if region.id == "middle")
    assert middle.start_seconds == 20.0
    assert middle.end_seconds == 80.0
    assert middle.status == "unverified"
    assert middle.inlier_raw_indices == tuple(range(120))
    assert len(result.global_windows) == 3
    assert _binary64(result.raw_beat_seconds) == _binary64(beats)


@pytest.mark.parametrize(
    ("count", "span", "supported"),
    [(23, 40.0, False), (24, 29.0, False), (24, 35.999, False), (24, 36.0, True)],
)
def test_middle_region_requires_long_span_count_and_frozen_window_coverage(
    count: int, span: float, *, supported: bool
) -> None:
    beats = tuple(32.0 + index * span / (count - 1) for index in range(count))

    result = assess_beat_sequence(beats, 48_000, 48_000 * 100)

    middle = next(region for region in result.regions if region.id == "middle")
    assert (middle.status == "unverified") is supported
    assert middle.assigned_positions == count
    assert middle.raw_positions == count
    assert middle.start_seconds == 20.0
    assert middle.end_seconds == 80.0
    if supported:
        assert result.selected_region_id == "middle"
    else:
        assert result.selected_region_id is None
        assert result.representative_bpm is None


def test_gradual_variable_tempo_remains_unsupported_with_full_local_information() -> None:
    beats = tuple(0.003 + 0.4 * index + 0.0005 * index**2 for index in range(220))

    result = assess_beat_sequence(beats, 48_000, 48_000 * 115)

    assert result.complete_fit is not None
    assert result.global_status == "unsupported"
    assert result.selected_region_id is None
    assert result.representative_bpm is None
    assert all(region.status == "unsupported" for region in result.regions)
    first_bpm, last_bpm = result.local_interval_bpm[0], result.local_interval_bpm[-1]
    assert first_bpm is not None
    assert last_bpm is not None
    assert first_bpm > last_bpm
    assert _binary64(result.raw_beat_seconds) == _binary64(beats)
    assert len(result.local_interval_seconds) == 219


@pytest.mark.parametrize(
    ("early_count", "early_period", "selected", "expected_bpm"),
    [(67, 0.5, "early", 120.0), (65, 0.5, "late", 120.0), (133, 0.25, "early", 240.0)],
)
def test_unsupported_middle_falls_back_to_longest_then_count_then_earliest_third(
    early_count: int, early_period: float, selected: str, expected_bpm: float
) -> None:
    beats = _beats(early_period, early_count) + _beats(0.5, 67, origin=66.67)

    result = assess_beat_sequence(beats, 48_000, 48_000 * 100)

    middle = next(region for region in result.regions if region.id == "middle")
    assert middle.status == "unsupported"
    assert result.global_status == "unsupported"
    assert result.selected_region_id == selected
    assert result.representative_bpm == pytest.approx(expected_bpm)
    assert _binary64(result.raw_beat_seconds) == _binary64(beats)
    chosen = next(region for region in result.regions if region.id == selected)
    assert chosen.status == "unverified"


@pytest.mark.parametrize("outlier_indices", [(40, 41), (40, 41, 42), tuple(range(0, 120, 9))])
def test_regional_exclusion_limits_remain_explicit(
    outlier_indices: tuple[int, ...],
) -> None:
    beats = [20.0 + index * 0.5 for index in range(120)]
    for index in outlier_indices:
        beats[index] += 0.1

    result = assess_beat_sequence(tuple(beats), 48_000, 48_000 * 100)

    middle = next(region for region in result.regions if region.id == "middle")
    if len(outlier_indices) == 2:
        assert middle.status == "unverified"
        assert middle.excluded_raw_indices == outlier_indices
        assert result.selected_region_id == "middle"
    else:
        assert middle.status == "unsupported"
        assert "consecutive_exclusions" in middle.reasons or "too_many_exclusions" in middle.reasons
    assert _binary64(result.raw_beat_seconds) == _binary64(tuple(beats))


def test_alternating_jitter_cannot_pass_only_the_wider_robust_seed_threshold() -> None:
    beats = tuple(0.03 + index * 0.5 + (-1.0) ** index * 0.015 for index in range(200))

    result = assess_beat_sequence(beats, 48_000, 48_000 * 100)

    assert result.global_status == "unsupported"
    assert "inconsistent_timing_bound" in result.global_reasons
    assert result.selected_region_id is None
    assert result.representative_bpm is None
    assert _binary64(result.raw_beat_seconds) == _binary64(beats)


@pytest.mark.parametrize("denominator", [1, 2, 4])
def test_explicit_comparable_snare_counts_preserve_beat_unit_without_verification(
    denominator: int,
) -> None:
    # Independent fixture assertion: these observed snares are two quarters apart.
    beats = _beats(1.46, 100)
    counts = tuple(index * 2 * denominator for index in range(len(beats)))

    result = assess_beat_sequence(
        beats,
        48_000,
        48_000 * 146,
        quarter_counts=counts,
        quarter_note_denominator=denominator,
        count_provenance="independently supplied fixture snare count mapping",
    )

    assert result.quarter_counts == counts
    assert result.complete_fit is not None
    assert result.complete_fit.quarter_note_denominator == denominator
    assert result.complete_fit.period_seconds_per_quarter == pytest.approx(0.73)
    assert result.complete_fit.bpm == pytest.approx(60.0 / 0.73)
    assert result.global_status == "unverified"
    assert result.count_provenance == "independently supplied fixture snare count mapping"
    assert result.local_interval_bpm == pytest.approx((60.0 / 0.73,) * 99)


def test_missing_count_jump_and_explicit_extra_exclusion_never_renumber_raw_events() -> None:
    indices = tuple(index for index in range(200) if index != 65)
    beats = [0.003 + index * 0.5 for index in indices]
    beats.insert(120, beats[119] + 0.1)
    counts: list[int | None] = list(indices)
    counts.insert(120, None)

    result = assess_beat_sequence(
        tuple(beats),
        48_000,
        48_000 * 100,
        quarter_counts=tuple(counts),
        count_provenance="independent fixture missing/extra mapping",
    )

    assert result.complete_fit is not None
    assert result.complete_fit.assigned_observations == 199
    assert result.complete_fit.bpm == pytest.approx(120.0)
    assert result.quarter_counts == tuple(counts)
    assert result.global_excluded_raw_indices == (120,)
    assert result.global_residual_seconds[120] is None
    assert result.raw_position_count == 200
    assert _binary64(result.raw_beat_seconds) == _binary64(tuple(beats))
    assert result.local_interval_bpm[119] is None
    assert result.local_interval_bpm[120] is None


@pytest.mark.parametrize("beats", [(), (0.0,), (0.0, 0.5)])
def test_short_complete_sequences_retain_data_and_report_unsupported(
    beats: tuple[float, ...],
) -> None:
    result = assess_beat_sequence(beats, 48_000, 48_000)

    assert result.global_status == "unsupported"
    assert result.selected_region_id is None
    assert result.representative_bpm is None
    assert result.raw_position_count == len(beats)
    assert _binary64(result.raw_beat_seconds) == _binary64(beats)


@pytest.mark.parametrize(
    "beats",
    [(-0.1,), (1.0,), (0.5, 0.25), (0.5, 0.5), (math.nan,), (math.inf,), (-math.inf,)],
)
def test_invalid_positions_do_not_produce_a_numerical_summary(beats: tuple[float, ...]) -> None:
    with pytest.raises(ValueError, match=r"invalid.*beat positions"):
        assess_beat_sequence(beats, 48_000, 48_000)


def test_unrepresentable_integer_position_fails_before_native_float_extraction() -> None:
    with pytest.raises(ValueError, match="exceeds binary64"):
        assess_beat_sequence((10**1000,), 48_000, 48_000)


def test_signed_zero_and_subnormal_raw_times_remain_bitexact_without_a_finite_bpm() -> None:
    beats = (-0.0, math.ulp(0.0))

    result = assess_beat_sequence(beats, 48_000, 48_000)

    assert _binary64(result.raw_beat_seconds) == _binary64(beats)
    assert result.complete_fit is None
    assert result.global_status == "unsupported"
    assert result.local_interval_seconds == (math.ulp(0.0),)
    assert result.local_interval_bpm == (None,)


@pytest.mark.parametrize("rate", [0, -1, True, 768_001])
def test_invalid_loaded_sample_rates_are_rejected(rate: int) -> None:
    with pytest.raises(ValueError, match="invalid selected BPM"):
        assess_beat_sequence((0.0, 0.5), rate, 48_000)


@pytest.mark.parametrize("frames", [0, -1, True, 2**64])
def test_invalid_loaded_frame_extents_are_rejected(frames: int) -> None:
    with pytest.raises(ValueError, match="invalid selected BPM"):
        assess_beat_sequence((0.0, 0.5), 48_000, frames)


@pytest.mark.parametrize("denominator", [0, 65, -1, True])
def test_invalid_beat_unit_denominators_are_rejected(denominator: int) -> None:
    with pytest.raises(ValueError, match="invalid selected BPM"):
        assess_beat_sequence((0.0, 0.5), 48_000, 48_000, quarter_note_denominator=denominator)


@pytest.mark.parametrize("counts", [(0,), (0, 0), (1, 0), (0, True), (0, 2**63)])
def test_incomplete_nonmonotone_or_noninteger_count_maps_are_rejected(
    counts: tuple[int | None, ...],
) -> None:
    with pytest.raises(ValueError, match=r"invalid.*BPM|invalid.*counts|count extent"):
        assess_beat_sequence((0.0, 0.5), 48_000, 48_000, quarter_counts=counts)


@pytest.mark.parametrize("provenance", ["", " ", "x" * 4097])
def test_invalid_count_provenance_is_rejected(provenance: str) -> None:
    with pytest.raises(ValueError, match="invalid selected BPM"):
        assess_beat_sequence((0.0, 0.5), 48_000, 48_000, count_provenance=provenance)


def test_complete_raw_position_bound_is_enforced_before_fitting() -> None:
    with pytest.raises(ValueError, match="invalid selected BPM"):
        assess_beat_sequence(tuple(index * 0.5 for index in range(250_001)), 48_000, 2**40)


def test_report_binds_retired_full_request_and_preserves_all_four_raw_arrays(
    beat_request: BeatWorkerRequest,
) -> None:
    component = _component(beat_request)

    report = summarize_beats(component, beat_request)

    assert report is not None
    assert report.identity == beat_request.identity
    assert report.model == beat_request.model
    assert report.sample_rate_hz == 48_000
    assert report.frame_count == 48_000 * 150
    assert report.origin_seconds == 0.0
    assert report.predictions is component.predictions
    assert report.predictions is not None
    assert component.predictions is not None
    for name in ("beat_seconds", "downbeat_seconds", "beat_logits", "downbeat_logits"):
        assert _binary64(getattr(report.predictions, name)) == _binary64(
            getattr(component.predictions, name)
        )
    assert report.assessment.global_status == "unverified"
    assert not beat_request.pcm.path.exists()


@pytest.mark.parametrize("status", ["unavailable", "failed", "cancelled"])
def test_unsuccessful_components_have_no_bpm_report(
    beat_request: BeatWorkerRequest, status: ComponentStatus
) -> None:
    component = replace(_component(beat_request), status=status, predictions=None)

    assert summarize_beats(component, beat_request) is None


@pytest.mark.parametrize("invalid", ["identity", "model", "retiring", "missing"])
def test_report_rejects_stale_unretired_or_incomplete_components(
    beat_request: BeatWorkerRequest, invalid: str
) -> None:
    component = _component(beat_request)
    if invalid == "identity":
        component = replace(component, identity=replace(component.identity, source_generation=8))
    elif invalid == "model":
        component = replace(component, model=replace(component.model, sha256="b" * 64))
    elif invalid == "retiring":
        component = replace(component, resources_released=False)
    else:
        component = replace(component, predictions=None)

    with pytest.raises(ValueError, match=r"identity_mismatch|retiring|predictions"):
        summarize_beats(component, beat_request)


@pytest.mark.parametrize(
    "invalid", ["positive_origin", "negative_origin", "schema", "channels", "dtype", "frames"]
)
def test_report_validates_retired_request_metadata_without_rebasing_source_zero(
    beat_request: BeatWorkerRequest, invalid: str
) -> None:
    component = _component(beat_request)
    pcm = beat_request.pcm
    invalid_fields = asdict(pcm)
    invalid_fields["dtype"] = "float64-le"
    invalid_dtype = MonoPcmInput(**invalid_fields)
    mutations = {
        "positive_origin": replace(pcm, origin_seconds=0.125),
        "negative_origin": replace(pcm, origin_seconds=-0.125),
        "channels": replace(pcm, channels=2),
        "dtype": invalid_dtype,
        "frames": replace(pcm, frame_count=True),
    }
    changed = (
        replace(beat_request, schema_version=2)
        if invalid == "schema"
        else replace(beat_request, pcm=mutations[invalid])
    )

    with pytest.raises(ValueError, match="validation error"):
        summarize_beats(component, changed)

    assert not beat_request.pcm.path.exists()
    assert component.predictions is not None
    assert _binary64(component.predictions.beat_seconds) == _binary64(_beats(0.73))
