"""Inferred beat ordinals remain replayable scalar metadata through Copy and restore."""

import json
import math
from itertools import pairwise
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis.app_tempo_inferred import EvaluatedOrdinalFit
from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.copy_snapshot import CopySnapshotData, capture_copy_snapshot
from flitzis_looper.models import BeatGrid, PadContentIdentity, SampleAnalysis
from tests.flitzis_looper.controller.test_adaptive_tempo_probe import _v4_method

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController


def _span_data(
    raw: list[float],
    points: list[tuple[int, float]],
    period: float,
    bounds: list[float],
    anchor: float = 0.0,
) -> dict[str, object]:
    """Summarize explicitly known grid members without a production assignment fit."""
    start, end = bounds
    local = [(k, t) for k, t in points if start <= t < end and start <= anchor + period * k < end]
    times = [t for _, t in local]
    residuals = [t - (anchor + period * k) for k, t in local]
    fitted_bpm = None
    if len(local) >= 2:
        mx = sum(k for k, _ in local) / len(local)
        my = sum(times) / len(times)
        xx = sum((k - mx) ** 2 for k, _ in local)
        fitted = sum((k - mx) * (t - my) for k, t in local) / xx
        if 0.2 <= fitted <= 2.0:
            fitted_bpm = 60.0 / fitted
    return {
        "window_seconds": bounds,
        "raw_count": sum(start <= t < end for t in raw),
        "expected_slots": math.ceil((end - anchor) / period) - math.ceil((start - anchor) / period),
        "matched": len(times),
        "observed_span_seconds": times[-1] - times[0] if times else None,
        "fitted_bpm": fitted_bpm,
        "max_residual_seconds": max(map(abs, residuals)) if times else None,
        "rms_residual_seconds": (
            math.sqrt(sum(r * r for r in residuals) / len(times)) if times else None
        ),
        "max_gap_seconds": (
            max(times[0] - start, end - times[-1], *(b - a for a, b in pairwise(times)))
            if times
            else end - start
        ),
    }


def _candidate_data(
    raw: list[float],
    points: list[tuple[int, float]],
    period: float,
    anchor: float = 0.0,
    duration: float = 320.0,
) -> dict[str, object]:
    return {
        "period_seconds": period,
        "anchor_seconds": anchor,
        "pivot_ordinal": sum(k for k, _ in points) / len(points),
        "pivot_seconds": sum(t for _, t in points) / len(points),
        "global": _span_data(raw, points, period, [0.0, duration], anchor),
        "sections": [
            _span_data(raw, points, period, [duration * i / 5, duration * (i + 1) / 5], anchor)
            for i in range(5)
        ],
    }


def _v5_data(
    extra_peaks: tuple[float, ...] = (3.17, 20.17, 100.17),
) -> dict[str, object]:
    # Two missing beats and unrelated extra peaks make raw indices differ from
    # the explicitly known beat ordinals. All retained samples remain unchanged.
    points = [(k, 0.5 * k) for k in range(640) if k not in {51, 352}]
    raw = sorted([t for _, t in points] + list(extra_peaks))
    indices = {t: i for i, t in enumerate(raw)}
    data = _v4_method().model_dump(mode="python")
    data.update(
        schema_version=5,
        policy="neural-inferred-ordinals-v5",
        status="estimated",
        bpm=120.0,
        selected=None,
        regions=[],
        integer_probe=None,
        frame_count=320 * 48_000,
        raw_beats=raw,
        raw_downbeats=[2.0 * i for i in range(160)],
    )
    check = {
        "status": "pass",
        "max_residual_seconds": 0.0,
        "rms_residual_seconds": 0.0,
        "rms_penalty_seconds": 0.0,
        "window_phase_difference_seconds": 0.0,
    }
    data["ordinal_fit"] = {
        "outcome": "supported",
        "evidence_tier": "two_ten_second",
        "candidate_limit_reached": False,
        "evaluated_candidate_count": 3,
        "winner": _candidate_data(raw, points, 0.5),
        "assignments": [[indices[t], k] for k, t in points],
        "alternatives": [
            _candidate_data(raw, [(int(4 * t), t) for t in raw if (4 * t).is_integer()], 0.25),
            _candidate_data(raw, [(k // 2, t) for k, t in points if k % 2 == 0], 1.0),
        ],
        "harmonics": [
            {
                "factor": 0.5,
                "seed_period_seconds": 0.25,
                "seed_anchor_seconds": 0.0,
                "outcome": "fitted",
                "alternative_index": 0,
            },
            {
                "factor": 2.0,
                "seed_period_seconds": 1.0,
                "seed_anchor_seconds": 0.0,
                "outcome": "fitted",
                "alternative_index": 1,
            },
        ],
        "probe_windows": [[16.0, 48.0], [272.0, 304.0]],
        "fractional_check": dict(check),
        "integer_check": dict(check),
        "integer_candidate_bpm": 120.0,
        "integer_preferred": True,
        "conflicting_sections": [],
    }
    return data


def _v5_method() -> AppTempoMethod:
    return AppTempoMethod.model_validate(_v5_data(), strict=True)


def _known_grid_data(
    raw: list[float],
    points: list[tuple[int, float]],
    period: float,
    anchor: float,
    alternatives: list[tuple[float, float, list[tuple[int, float]]]],
    *,
    duration: float = 320.0,
    conflicts: tuple[int, ...] = (),
    ambiguous: bool = False,
) -> dict[str, object]:
    """Publish analytically specified grids, never infer memberships from a fit."""
    data = _v5_data(extra_peaks=())
    data.update(
        raw_beats=raw,
        raw_downbeats=[],
        frame_count=int(duration * 48_000),
        bpm=None if conflicts or ambiguous else 60.0 / period,
        status="uncorroborated" if conflicts or ambiguous else "estimated",
    )
    fit = data["ordinal_fit"]
    fit.update(
        outcome="conflicting_sections" if conflicts else "ambiguous" if ambiguous else "supported",
        evaluated_candidate_count=1 + len(alternatives),
        winner=_candidate_data(raw, points, period, anchor, duration),
        assignments=[[raw.index(t), k] for k, t in points],
        alternatives=[_candidate_data(raw, p, s, a, duration) for s, a, p in alternatives],
        probe_windows=[
            [duration / 10 - 16.0, duration / 10 + 16.0],
            [duration * 0.9 - 16.0, duration * 0.9 + 16.0],
        ],
        integer_candidate_bpm=60.0 / period,
        conflicting_sections=list(conflicts),
    )
    fit["harmonics"] = [
        {
            "factor": factor,
            "seed_period_seconds": period * factor,
            "seed_anchor_seconds": anchor,
            "outcome": "fitted" if 0.2 <= period * factor <= 2.0 else "out_of_range",
            "alternative_index": (
                next(
                    i
                    for i, (p, a, _) in enumerate(alternatives)
                    if p == period * factor and a == anchor
                )
                if 0.2 <= period * factor <= 2.0
                else None
            ),
        }
        for factor in (0.5, 2.0)
    ]
    return data


def _local_half_density_data(coarse_anchor: float = 0.0625) -> dict[str, object]:
    anchor = 0.0625
    outside = [(k, anchor + 0.25 * k) for k in range(1280) if not 64 <= anchor + 0.25 * k < 128]
    local = [
        (
            n,
            coarse_anchor + 0.5 * n,
            0.0 if ((n - 128) // 4) % 5 == 0 else 0.016 * (1, -1, -1, 1)[n % 4],
        )
        for n in range(128, 256)
    ]
    raw = sorted([t for _, t in outside] + [t + offset for _, t, offset in local])
    points = sorted(
        outside + [(int(4 * (t - anchor)), t) for _, t, offset in local if offset == 0.0]
    )
    aligned = [(k // 2, t) for k, t in outside if k % 2 == 0]
    if coarse_anchor == anchor:
        aligned += [(n, t + offset) for n, t, offset in local]
    alternatives = [(0.5, anchor, sorted(aligned))]
    if coarse_anchor != anchor:
        odd = [(k // 2, t) for k, t in outside if k % 2 == 1]
        alternatives.append((
            0.5,
            coarse_anchor,
            sorted(odd + [(n, t + offset) for n, t, offset in local]),
        ))
    return _known_grid_data(raw, points, 0.25, anchor, alternatives)


def _local_subdivision_data() -> dict[str, object]:
    anchor = 0.0625
    points = [(n, anchor + 0.5 * n) for n in range(640)]
    extra = [anchor + 0.25 * k for k in range(1280) if k % 2 and 64 <= anchor + 0.25 * k < 128]
    raw = sorted([t for _, t in points] + extra)
    return _known_grid_data(
        raw,
        points,
        0.5,
        anchor,
        [
            (0.25, anchor, [(int(4 * (t - anchor)), t) for t in raw]),
            (1.0, anchor, [(n // 2, t) for n, t in points if n % 2 == 0]),
        ],
    )


def _local_shift_data() -> dict[str, object]:
    anchor = 0.0625
    points = [(k, anchor + 0.25 * k) for k in range(1280) if not 64 <= anchor + 0.25 * k < 128]
    points += [(2 * n, anchor + 0.5 * n) for n in range(128, 256) if ((n - 128) // 4) % 5 == 0]
    points.sort()
    shifted = [(n, 64.1875 + 0.5 * n) for n in range(128)]
    raw = sorted([t for _, t in points] + [t for _, t in shifted])
    return _known_grid_data(
        raw,
        points,
        0.25,
        anchor,
        [
            (0.5, anchor, [(k // 2, t) for k, t in points if k % 2 == 0]),
            (0.5, 64.1875, shifted),
        ],
        conflicts=(1,),
    )


def _local_winding_data() -> dict[str, object]:
    anchor, duration = 0.03125, 300.0
    fine = [(k, anchor + 0.25 * k) for k in range(1200) if not 60 <= anchor + 0.25 * k < 120]
    coarse = [
        (n, anchor + 0.625 * n) for n in range(480) if n % 2 == 0 or 60 <= anchor + 0.625 * n < 120
    ]
    raw = sorted({t for _, t in fine} | {t for _, t in coarse})
    points = [(int(4 * (t - anchor)), t) for t in raw if (4 * (t - anchor)).is_integer()]
    return _known_grid_data(
        raw,
        points,
        0.25,
        anchor,
        [
            (
                0.5,
                anchor,
                [(int(2 * (t - anchor)), t) for t in raw if (2 * (t - anchor)).is_integer()],
            ),
            (0.625, anchor, coarse),
        ],
        duration=duration,
        conflicts=(1,),
    )


def _alternating_parity_data() -> dict[str, object]:
    even = [(n, 0.066 + 0.5 * n) for n in range(640)]
    odd = [(n, 0.284 + 0.5 * n) for n in range(640)]
    raw = sorted([t for _, t in even] + [t for _, t in odd])
    return _known_grid_data(
        raw,
        even,
        0.5,
        0.066,
        [
            (0.25, 0.066, [(2 * n, t) for n, t in even]),
            (1.0, 0.066, [(n // 2, t) for n, t in even if n % 2 == 0]),
            (0.5, 0.284, odd),
        ],
        ambiguous=True,
    )


def test_v5_preserves_raw_indices_and_explicit_missing_beat_ordinals() -> None:
    original = _v5_data()
    method = AppTempoMethod.model_validate(original, strict=True)
    reopened = AppTempoMethod.model_validate_json(method.model_dump_json())
    assert reopened == method
    assert method.bpm == 120.0
    assert method.selected is None
    assert method.raw_beats == original["raw_beats"]
    assert method.raw_downbeats == original["raw_downbeats"]
    fit = method.ordinal_fit
    assert isinstance(fit, EvaluatedOrdinalFit)
    assert fit.assignments[8][0] != fit.assignments[8][1]
    assert "global" in json.loads(method.model_dump_json())["ordinal_fit"]["winner"]


@pytest.mark.parametrize("count", [145, 4098])
def test_v5_drained_queue_count_remains_bounded_scalar_metadata(count: int) -> None:
    data = _v5_data()
    data["ordinal_fit"]["evaluated_candidate_count"] = count
    method = AppTempoMethod.model_validate(data, strict=True)
    assert AppTempoMethod.model_validate_json(method.model_dump_json(), strict=True) == method
    fit = method.ordinal_fit
    assert isinstance(fit, EvaluatedOrdinalFit)
    assert len(fit.alternatives) == 2


@pytest.mark.parametrize("count", [True, False, 4098.0, "4098", -1, 4099, 2**64])
def test_v5_candidate_count_rejects_nonintegers_and_admission_overflow(count: object) -> None:
    data = _v5_data()
    data["ordinal_fit"]["evaluated_candidate_count"] = count
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize("case", ["half_even", "half_odd", "subdivisions"])
def test_v5_local_lattice_density_does_not_invent_a_section_contradiction(case: str) -> None:
    # The fast model's 12.5ms limit excludes 16ms deviations that the coherent
    # half-density model can retain. The reverse case adds real subdivisions.
    data = (
        _local_subdivision_data()
        if case == "subdivisions"
        else _local_half_density_data(0.3125 if case == "half_odd" else 0.0625)
    )
    method = AppTempoMethod.model_validate(data, strict=True)
    fit = method.ordinal_fit
    assert isinstance(fit, EvaluatedOrdinalFit)
    assert fit.winner is not None
    assert fit.outcome == "supported"
    assert fit.conflicting_sections == []
    assert method.bpm == (120.0 if case == "subdivisions" else 240.0)
    assert any(
        candidate.sections[1].matched > fit.winner.sections[1].matched
        for candidate in fit.alternatives
    )
    data["ordinal_fit"]["conflicting_sections"] = [1]
    with pytest.raises(ValidationError, match="section outcome omitted or invented"):
        AppTempoMethod.model_validate(data, strict=True)


def test_v5_fast_alternating_parities_still_veto_a_strong_half_tempo() -> None:
    # These are the two exact parity grids of 240BPM observations alternating
    # by +16ms/-16ms. Local lattice filtering cannot remove global rivalry.
    data = _alternating_parity_data()
    method = AppTempoMethod.model_validate(data, strict=True)
    fit = method.ordinal_fit
    assert isinstance(fit, EvaluatedOrdinalFit)
    assert method.bpm is None
    assert fit.outcome == "ambiguous"
    data.update(bpm=120.0, status="estimated")
    data["ordinal_fit"]["outcome"] = "supported"
    with pytest.raises(ValidationError, match="outcome contradicts"):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize("case", ["shifted", "nonharmonic_winding"])
def test_v5_incompatible_local_grids_keep_their_actual_section_veto(case: str) -> None:
    data = _local_shift_data() if case == "shifted" else _local_winding_data()
    method = AppTempoMethod.model_validate(data, strict=True)
    fit = method.ordinal_fit
    assert isinstance(fit, EvaluatedOrdinalFit)
    assert method.bpm is None
    assert fit.outcome == "conflicting_sections"
    assert fit.conflicting_sections == [1]
    if case == "nonharmonic_winding":
        # Endpoint-wise modulo would hide many whole fine-grid windings. Both
        # endpoints individually lie inside the tolerance, with DIFFERENT k.
        assert round(0.625 / 0.25) == 2
        phases = [((t - 0.03125) / 0.625) * (0.625 - 2 * 0.25) for t in (60.0, 120.0)]
        windings = [round(phase / 0.25) for phase in phases]
        assert windings[0] != windings[1]
        assert all(abs(p - k * 0.25) < 0.0125 for p, k in zip(phases, windings, strict=True))
    data["ordinal_fit"]["conflicting_sections"] = []
    with pytest.raises(ValidationError, match="section outcome omitted or invented"):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize(
    "damage",
    ["raw_index", "ordinal", "bool_index", "float_ordinal", "missing", "count", "pivot", "metric"],
)
def test_v5_rejects_forged_assignments_coverage_and_shared_pivot(damage: str) -> None:
    data = json.loads(json.dumps(_v5_data()))
    fit = data["ordinal_fit"]
    if damage == "raw_index":
        fit["assignments"][8][0] += 1
    elif damage == "ordinal":
        fit["assignments"][8][1] += 1
    elif damage == "bool_index":
        fit["assignments"][0][0] = False
    elif damage == "float_ordinal":
        fit["assignments"][1][1] = 1.0
    elif damage == "missing":
        fit["assignments"].pop(8)
    elif damage == "count":
        fit["winner"]["global"]["raw_count"] -= 1
    elif damage == "pivot":
        fit["winner"]["pivot_ordinal"] += 1.0
        fit["winner"]["pivot_seconds"] += 0.5
    else:
        fit["fractional_check"]["rms_residual_seconds"] = 0.001
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data)


def test_v5_known_grid_keeps_midpoints_in_the_subdivision_model_only() -> None:
    data = _v5_data(extra_peaks=(0.25, 0.75, 3.17, 20.17, 100.17))
    method = AppTempoMethod.model_validate(data, strict=True)
    fit = method.ordinal_fit
    assert isinstance(fit, EvaluatedOrdinalFit)
    retained = {raw_index for raw_index, _ in fit.assignments}
    assert method.raw_beats.index(0.25) not in retained
    assert method.raw_beats.index(0.75) not in retained
    assert fit.alternatives[0].global_span.matched == len(fit.assignments) + 2


def test_v5_rejects_a_nonsettled_fit_even_when_its_assignment_is_unchanged() -> None:
    data = json.loads(json.dumps(_v5_data()))
    data["ordinal_fit"]["winner"]["period_seconds"] += 1e-7
    with pytest.raises(ValidationError, match="consistent final model"):
        AppTempoMethod.model_validate(data)


def test_v5_near_equal_fit_cannot_change_a_missing_beat_boundary_assignment() -> None:
    # The known grid has no event at ordinal51. This extra observation lies just
    # inside its left tolerance boundary at the exact independently known fit.
    peak = 25.5 - 0.025 + 1e-10
    shift = 5e-10
    assert abs(peak - 25.5) < 0.025
    assert abs(peak - (25.5 + shift)) > 0.025
    assert shift < 1e-9
    data = json.loads(json.dumps(_v5_data(extra_peaks=(3.17, 20.17, 100.17, peak))))
    fit = data["ordinal_fit"]
    fit["winner"]["anchor_seconds"] = shift
    for harmonic in fit["harmonics"]:
        harmonic["seed_anchor_seconds"] = shift
    peak_index = data["raw_beats"].index(peak)
    assert peak_index not in {i for i, _ in fit["assignments"]}
    with pytest.raises(
        ValidationError, match="Recomputed ordinal fit changes its declared assignments"
    ):
        AppTempoMethod.model_validate(data, strict=True)


def test_v5_probes_cannot_be_moved_into_convenient_neighboring_sections() -> None:
    data = json.loads(json.dumps(_v5_data()))
    data["ordinal_fit"]["probe_windows"][1] = [48.0, 80.0]
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data)


def test_v4_serialization_omits_inferred_fields_and_cannot_import_them() -> None:
    method = _v4_method(short=True)
    assert "ordinal_fit" not in method.model_dump(mode="python")
    assert "ordinal_fit" not in json.loads(method.model_dump_json())
    data = method.model_dump(mode="python")
    data.update(status="insufficient", bpm=None, selected=None, integer_probe=None)
    AppTempoMethod.model_validate(data)
    data["ordinal_fit"] = {
        "outcome": "budget_unavailable",
        "required_bytes": 1024 * len(data["raw_beats"])
        + 512 * len(data["raw_downbeats"])
        + 8 * 1024 * 1024,
        "available_bytes": 0,
    }
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data)


@pytest.mark.parametrize("damage", ["omitted", "wrong_reference", "false_failure"])
def test_v5_harmonic_declarations_replay_their_exact_bounded_starts(damage: str) -> None:
    data = json.loads(json.dumps(_v5_data()))
    fit = data["ordinal_fit"]
    if damage == "omitted":
        fit["harmonics"].pop()
    elif damage == "wrong_reference":
        fit["harmonics"][0]["alternative_index"] = 1
    else:
        fit["harmonics"][0].update(outcome="no_fixed_fit", alternative_index=None)
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data)


@pytest.mark.parametrize("outcome", ["budget_unavailable", "allocation_unavailable"])
def test_v5_unavailable_reserve_is_explicit_and_never_carries_a_bpm(outcome: str) -> None:
    data = json.loads(json.dumps(_v5_data()))
    required = 1024 * len(data["raw_beats"]) + 512 * len(data["raw_downbeats"]) + 8 * 1024 * 1024
    data.update(bpm=None, status="insufficient")
    data["ordinal_fit"] = {
        "outcome": outcome,
        "required_bytes": required,
        "available_bytes": 0 if outcome == "budget_unavailable" else required + 1,
    }
    method = AppTempoMethod.model_validate(data)
    assert method.bpm is None
    data["bpm"] = 120.0
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data)


def test_v5_copy_and_restore_keep_provenance_and_reject_contradictory_bpm(
    controller: AppController,
) -> None:
    data = _v5_data()
    data["ordinal_fit"]["evaluated_candidate_count"] = 4098
    method = AppTempoMethod.model_validate(data, strict=True)
    project = controller.project
    project.sample_paths[0] = "samples/source.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    analysis = SampleAnalysis(
        bpm=120.0,
        key="Am",
        beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]),
        bpm_method=method,
    )
    project.sample_analysis[0] = analysis
    saved = analysis.model_dump_json()
    assert SampleAnalysis.model_validate_json(saved).bpm_method == method
    assert not controller.loader._needs_tempo_refinement(0)
    snapshot = capture_copy_snapshot(project, 0)
    copied = CopySnapshotData.model_validate_json(snapshot.payload_json)
    assert copied.analysis is not None
    assert copied.analysis.bpm_method == method
    damaged = json.loads(snapshot.payload_json)
    damaged["analysis"]["bpm"] = 119.0
    with pytest.raises(ValidationError):
        CopySnapshotData.model_validate(damaged)
    for count in (True, 4099):
        damaged = json.loads(snapshot.payload_json)
        damaged["analysis"]["bpm_method"]["ordinal_fit"]["evaluated_candidate_count"] = count
        with pytest.raises(ValidationError):
            CopySnapshotData.model_validate(damaged, strict=True)
