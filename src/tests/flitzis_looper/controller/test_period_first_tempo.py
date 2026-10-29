"""Finite period selection preserves physical evidence and existing performer owners."""

import json
import math
from copy import deepcopy
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.analysis.app_tempo_probe import FractionalTempoCheck
from flitzis_looper.copy_snapshot import CopySnapshotData, capture_copy_snapshot
from flitzis_looper.models import BeatGrid, PadContentIdentity, SampleAnalysis
from tests.flitzis_looper.controller.test_bpm_range import _range_data
from tests.flitzis_looper.controller.test_corroborated_tempo import _v6_data
from tests.flitzis_looper.controller.test_inferred_tempo_probe import (
    _alternating_parity_data,
    _candidate_data,
    _v5_data,
)
from tests.flitzis_looper.controller.test_nominal_tempo_prior import _prior_data

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController

_TRIAL_FIELDS = (
    "outcome",
    "evidence_tier",
    "candidate_limit_reached",
    "probe_windows",
    "fractional_check",
    "integer_check",
    "integer_candidate_bpm",
    "integer_preferred",
    "conflicting_sections",
)


def _perfect_checks(
    points: list[tuple[int, float]], period: float, windows: list[list[float]]
) -> dict:
    """Closed-form errors for an explicitly known perfect affine grid."""
    base = 60.0 / period
    integer = float(math.floor(base + 0.5))
    difference = period - 60.0 / integer
    mean = sum(k for k, _t in points) / len(points)
    rms = abs(difference) * math.sqrt(sum((k - mean) ** 2 for k, _t in points) / len(points))
    means = [
        sum(k for k, t in points if start <= t < end) / sum(start <= t < end for _k, t in points)
        for start, end in windows
    ]
    maximum = abs(difference) * max(abs(k - mean) for k, _t in points)
    phase = abs(difference * (means[1] - means[0]))
    integer_pass = (
        maximum <= min(0.025, 0.05 * (60.0 / integer)) and rms <= 0.00025 and phase <= 0.005
    )
    return {
        "probe_windows": windows,
        "fractional_check": {
            "status": "pass",
            "max_residual_seconds": 0.0,
            "rms_residual_seconds": 0.0,
            "rms_penalty_seconds": 0.0,
            "window_phase_difference_seconds": 0.0,
        },
        "integer_check": {
            "status": "pass" if integer_pass else "fail",
            "max_residual_seconds": maximum,
            "rms_residual_seconds": rms,
            "rms_penalty_seconds": rms,
            "window_phase_difference_seconds": phase,
        },
        "integer_candidate_bpm": integer,
        "integer_preferred": integer_pass and abs(integer - base) <= 0.005,
    }


def _upgrade(data: dict, trials: list[dict] | None = None) -> dict:
    fit = data["ordinal_fit"]
    declarations = [
        {
            "factor": h["factor"],
            "seed_period_seconds": h["seed_period_seconds"],
            "seed_anchor_seconds": h["seed_anchor_seconds"],
            "outcome": h["outcome"],
            "candidate_index": h["alternative_index"] + 1
            if h["alternative_index"] is not None
            else None,
        }
        for h in fit["harmonics"]
    ]
    if trials is None:
        trials = [
            {
                **{field: deepcopy(fit[field]) for field in _TRIAL_FIELDS},
                "candidate_index": 0,
                "critical_indices": list(range(1, len(fit["alternatives"]) + 1)),
            }
        ]
    fit.update(
        harmonics=[],
        period_selection={
            "canonical_index": 0,
            "harmonics": declarations,
            "trials": trials,
            "selected_trial_index": next(
                (i for i, t in enumerate(trials) if t["outcome"] == "supported"), None
            ),
        },
    )
    data.update(schema_version=9, policy="neural-inferred-periods-v9")
    return data


def _perfect_data(duration: float = 4.0, period: float = 0.5) -> dict:
    data = _v5_data(extra_peaks=())
    anchor = 0.0625
    points = [(k, anchor + period * k) for k in range(int(duration / period))]
    raw = [t for _k, t in points]
    fit = data["ordinal_fit"]
    windows = (
        [[0.0, duration / 2], [duration / 2, duration]]
        if duration < 30
        else [[0.0, 20.0], [duration - 20, duration]]
    )
    fit.update(
        winner=_candidate_data(raw, points, period, anchor, duration),
        assignments=[[i, k] for i, (k, _t) in enumerate(points)],
        alternatives=[
            _candidate_data(raw, [(2 * k, t) for k, t in points], period / 2, anchor, duration),
            _candidate_data(
                raw, [(k // 2, t) for k, t in points if k % 2 == 0], period * 2, anchor, duration
            ),
        ],
        evidence_tier="short_source" if duration < 30 else "two_ten_second",
        **_perfect_checks(points, period, windows),
    )
    fit["harmonics"] = [
        {
            "factor": factor,
            "seed_period_seconds": period * factor,
            "seed_anchor_seconds": anchor,
            "outcome": "fitted",
            "alternative_index": i,
        }
        for i, factor in enumerate((0.5, 2.0))
    ]
    data.update(
        frame_count=int(duration * 48000),
        raw_beats=raw,
        raw_downbeats=[],
        status="single_region_estimate" if duration < 30 else "estimated",
        bpm=60.0 / period,
    )
    return _upgrade(data)


def _promotion_data(*, limited: bool = False, fractional: bool = False) -> dict:
    data = _perfect_data()
    anchor, fine = 0.0625, 0.25001 if fractional else 0.25
    raw = [anchor + fine * k for k in range(16)]
    points = [
        [(k, raw[2 * k]) for k in range(8)],
        [(k, t) for k, t in enumerate(raw)],
        [(k, raw[4 * k]) for k in range(4)],
        [(k, raw[2 * k + 1]) for k in range(8)],
    ]
    summaries = [
        _candidate_data(raw, p, period, a, 4.0)
        for p, period, a in zip(
            points,
            (2 * fine, fine, 4 * fine, 2 * fine),
            (anchor, anchor, anchor, anchor + fine),
            strict=True,
        )
    ]
    checks = [
        _perfect_checks(p, period, [[0.0, 2.0], [2.0, 4.0]])
        for p, period in zip(points, (2 * fine, fine, 4 * fine, 2 * fine), strict=True)
    ]
    order = [0, 1, 2, 3] if limited else [1, 0, 2, 3]
    reverse = {original: virtual for virtual, original in enumerate(order)}
    trials = [
        {
            "candidate_index": reverse[index],
            "outcome": "ambiguous" if index == 0 else "search_limited" if limited else "supported",
            "evidence_tier": "short_source",
            "candidate_limit_reached": limited and index == 1,
            "conflicting_sections": [],
            "critical_indices": [i for i in range(4) if i != reverse[index]],
            **checks[index],
        }
        for index in (0, 1)
    ]
    chosen = 0 if limited else 1
    fit = data["ordinal_fit"]
    fit.update(
        winner=summaries[chosen],
        alternatives=[summaries[i] for i in order[1:]],
        assignments=[[raw.index(t), k] for k, t in points[chosen]],
        evaluated_candidate_count=4098 if limited else 4,
        outcome="search_limited" if limited else "supported",
        **checks[chosen],
    )
    fit["period_selection"] = {
        "canonical_index": reverse[0],
        "harmonics": [
            {
                "factor": factor,
                "seed_period_seconds": 2 * fine * factor,
                "seed_anchor_seconds": anchor,
                "outcome": "fitted",
                "candidate_index": reverse[index],
            }
            for factor, index in ((0.5, 1), (2.0, 2))
        ],
        "trials": trials,
        "selected_trial_index": None if limited else 1,
    }
    data.update(
        raw_beats=raw,
        status="uncorroborated" if limited else "single_region_estimate",
        bpm=None if limited else 60.0 / fine,
    )
    return data


def _joint_data() -> dict:
    data = _v6_data()
    fit = data["ordinal_fit"]
    trials = []
    for index, candidate in enumerate([fit["winner"], *fit["alternatives"]]):
        trials.append({
            **{field: deepcopy(fit[field]) for field in _TRIAL_FIELDS},
            "candidate_index": index,
            "critical_indices": [i for i in range(3) if i != index],
            "outcome": "insufficient" if index == 0 else "ambiguous",
            "integer_candidate_bpm": 60.0 / candidate["period_seconds"],
        })
    _upgrade(data, trials)
    data.update(schema_version=6, policy="neural-qm-same-period-v6")
    data["qm_corroboration"]["neural_result"].update(
        schema_version=9, policy="neural-inferred-periods-v9"
    )
    return data


def test_canonical_success_is_retained_despite_two_valid_unit_candidates() -> None:
    data = _perfect_data()
    method = AppTempoMethod.model_validate(data, strict=True)
    assert method.bpm == 120.0
    assert len(method.ordinal_fit.period_selection.trials) == 1
    assert method.model_dump_json().count('"raw_beats"') == 1
    assert AppTempoMethod.model_validate_json(method.model_dump_json(), strict=True) == method
    old = deepcopy(data)
    old.update(
        schema_version=5, policy="neural-inferred-ordinals-v5", status="uncorroborated", bpm=None
    )
    selection = old["ordinal_fit"].pop("period_selection")
    old["ordinal_fit"].update(
        outcome="ambiguous",
        harmonics=[
            {
                **{k: v for k, v in h.items() if k != "candidate_index"},
                "alternative_index": h["candidate_index"] - 1,
            }
            for h in selection["harmonics"]
        ],
    )
    assert AppTempoMethod.model_validate(old, strict=True).bpm is None


@pytest.mark.parametrize("fractional", [False, True])
def test_only_an_actual_declared_harmonic_can_replace_an_unsupported_canonical(
    *,
    fractional: bool,
) -> None:
    method = AppTempoMethod.model_validate(_promotion_data(fractional=fractional), strict=True)
    assert method.bpm == (60.0 / 0.25001 if fractional else 240.0)
    assert method.ordinal_fit.period_selection.selected_trial_index == 1
    assert method.ordinal_fit.integer_preferred is (not fractional)
    assert method.ordinal_fit.period_selection.trials[0].outcome == "ambiguous"


def test_alias_search_limit_does_not_forge_the_original_canonical_flag() -> None:
    method = AppTempoMethod.model_validate(_promotion_data(limited=True), strict=True)
    assert method.bpm is None
    assert method.ordinal_fit.outcome == "search_limited"
    assert method.ordinal_fit.candidate_limit_reached is False
    assert method.ordinal_fit.period_selection.trials[0].outcome == "ambiguous"
    assert method.ordinal_fit.period_selection.trials[1].candidate_limit_reached is True


def test_shifted_fast_parities_keep_global_and_local_vetoes_under_version_nine() -> None:
    data = _alternating_parity_data()
    fit = data["ordinal_fit"]
    trials = [
        {
            **{field: deepcopy(fit[field]) for field in _TRIAL_FIELDS},
            "candidate_index": index,
            "critical_indices": [i for i in range(4) if i != index],
            "outcome": "ambiguous" if index == 0 else "conflicting_sections",
            "conflicting_sections": [] if index == 0 else list(range(5)),
            "integer_candidate_bpm": bpm,
        }
        for index, bpm in enumerate((120.0, 240.0, 60.0))
    ]
    method = AppTempoMethod.model_validate(_upgrade(data, trials), strict=True)
    assert method.bpm is None
    assert method.ordinal_fit.outcome == "ambiguous"
    assert method.ordinal_fit.period_selection.trials[1].conflicting_sections == list(range(5))


def test_a_factor_four_rival_cannot_be_exempted_through_an_intermediate_unit() -> None:
    data = _perfect_data()
    anchor, duration = 0.0625, 16.0
    raw = [anchor + k for k in range(16)]
    fit = data["ordinal_fit"]
    points = [(4 * k, t) for k, t in enumerate(raw)]
    fit.update(
        winner=_candidate_data(raw, points, 0.25, anchor, duration),
        assignments=[[i, k] for i, (k, _t) in enumerate(points)],
        alternatives=[
            _candidate_data(raw, [(2 * k, t) for k, t in enumerate(raw)], 0.5, anchor, duration),
            _candidate_data(raw, list(enumerate(raw)), 1.0, anchor, duration),
        ],
        **_perfect_checks(points, 0.25, [[0.0, 8.0], [8.0, 16.0]]),
    )
    selection = fit["period_selection"]
    selection["harmonics"] = [
        {
            "factor": factor,
            "seed_period_seconds": 0.25 * factor,
            "seed_anchor_seconds": anchor,
            "outcome": "out_of_range" if factor == 0.5 else "fitted",
            "candidate_index": None if factor == 0.5 else 1,
        }
        for factor in (0.5, 2.0)
    ]
    selection["trials"][0].update({field: deepcopy(fit[field]) for field in _TRIAL_FIELDS})
    data.update(raw_beats=raw, frame_count=int(duration * 48000), bpm=240.0)
    with pytest.raises(ValidationError, match="trial outcome"):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize(
    "damage",
    [
        "seed",
        "canonical",
        "order",
        "unperformed",
        "selected",
        "mapping",
        "checks",
        "critical",
        "flag",
        "after_success",
    ],
)
def test_period_selection_rejects_forged_provenance_and_trial_priority(damage: str) -> None:
    data = _promotion_data()
    fit = data["ordinal_fit"]
    selection = fit["period_selection"]
    targets = {
        "seed": (
            selection["harmonics"][0],
            "seed_period_seconds",
            selection["harmonics"][0]["seed_period_seconds"] + 0.001,
        ),
        "canonical": (selection, "canonical_index", 0),
        "selected": (selection, "selected_trial_index", 0),
        "flag": (selection["trials"][0], "candidate_limit_reached", True),
    }
    if damage in targets:
        target, field, value = targets[damage]
        target[field] = value
    elif damage == "order":
        selection["trials"].reverse()
    elif damage == "unperformed":
        selection["trials"][1]["candidate_index"] = 3
    elif damage == "mapping":
        fit["assignments"].pop()
    elif damage == "checks":
        fit["fractional_check"]["rms_residual_seconds"] = 0.01
    elif damage == "critical":
        selection["trials"][0]["critical_indices"] = []
    else:
        selection["trials"].append(deepcopy(selection["trials"][1]))
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize("value", [None, {}])
def test_version_five_rejects_new_period_selection_even_when_null(value: object) -> None:
    data = _v5_data()
    data["ordinal_fit"]["period_selection"] = value
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize("value", [True, 1.0, -1, 32])
def test_virtual_references_keep_strict_integer_and_total_pool_bounds(value: object) -> None:
    data = _perfect_data()
    data["ordinal_fit"]["period_selection"]["canonical_index"] = value
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


def test_version_five_keeps_its_ten_summary_limit() -> None:
    data = _v5_data()
    data["ordinal_fit"]["alternatives"] += [deepcopy(data["ordinal_fit"]["alternatives"][0])] * 9
    with pytest.raises(ValidationError, match="original bounded ordinal"):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize("outcome", ["budget_unavailable", "no_winner"])
def test_unavailable_period_preparation_has_no_invented_selection_witness(outcome: str) -> None:
    data = _perfect_data()
    data.update(status="insufficient", bpm=None)
    if outcome == "budget_unavailable":
        data["ordinal_fit"] = {
            "outcome": outcome,
            "required_bytes": 1024 * len(data["raw_beats"]) + 8 * 1024 * 1024,
            "available_bytes": 0,
        }
    else:
        data["ordinal_fit"] = {
            "outcome": "insufficient",
            "evidence_tier": "inconclusive",
            "candidate_limit_reached": False,
            "evaluated_candidate_count": 0,
            "winner": None,
            "assignments": [],
            "alternatives": [],
            "harmonics": [],
            "probe_windows": [],
            "fractional_check": None,
            "integer_check": None,
            "integer_candidate_bpm": None,
            "integer_preferred": False,
            "conflicting_sections": [],
        }
    method = AppTempoMethod.model_validate(data, strict=True)
    assert method.bpm is None
    assert "period_selection" not in method.model_dump_json()


def test_joint_and_range_replay_the_actual_original_version_nine_header() -> None:
    joint = AppTempoMethod.model_validate(_joint_data(), strict=True)
    assert joint.qm_corroboration.neural_result.schema_version == 9
    assert joint.bpm == 120.0
    data = _range_data()
    primary = _perfect_data(duration=100.0)
    for key, value in primary.items():
        if key not in {"schema_version", "backend", "policy", "status", "bpm"}:
            data[key] = value
    data["bpm_range"]["primary"].update(schema_version=9, policy="neural-inferred-periods-v9")
    assert AppTempoMethod.model_validate(data, strict=True).bpm == 60.0
    rejected = _range_data(selected=False, version=6)
    rejected.update(
        ordinal_fit=_joint_data()["ordinal_fit"], qm_corroboration=_joint_data()["qm_corroboration"]
    )
    assert AppTempoMethod.model_validate(rejected, strict=True).bpm is None


def test_qm_kernel_nine_is_numeric_evidence_but_cannot_widen_saved_version_eight() -> None:
    data = _joint_data()
    qm = data["qm_corroboration"]["qm"]
    qm["estimate"] = {
        "kernel_version": 9,
        "status": "estimated",
        "bpm": 120.0,
        "selected": None,
        "regions": [],
        "ordinal_fit": _perfect_data(duration=100.0)["ordinal_fit"],
    }
    assert AppTempoMethod.model_validate(data, strict=True).bpm == 120.0
    saved = _prior_data()
    saved["nominal_selection"]["qm"]["estimate"] = {
        "kernel_version": 9,
        "status": "estimated",
        "bpm": 60.0,
        "selected": None,
        "regions": [],
        "ordinal_fit": _perfect_data(duration=100.0, period=1.0)["ordinal_fit"],
    }
    with pytest.raises(ValidationError, match="original QM kernel"):
        AppTempoMethod.model_validate(saved, strict=True)


@pytest.mark.parametrize("promoted", [False, True])
def test_period_choice_copy_restore_keeps_key_and_manual_performer_override(
    controller: AppController,
    *,
    promoted: bool,
) -> None:
    data = _promotion_data(fractional=True) if promoted else _perfect_data()
    method = AppTempoMethod.model_validate_json(json.dumps(data), strict=True)
    assert isinstance(method.ordinal_fit.fractional_check, FractionalTempoCheck)
    assert isinstance(method.ordinal_fit.integer_check, FractionalTempoCheck)
    assert all(
        isinstance(trial.fractional_check, FractionalTempoCheck)
        and isinstance(trial.integer_check, FractionalTempoCheck)
        for trial in method.ordinal_fit.period_selection.trials
    )
    analysis = SampleAnalysis(
        bpm=method.bpm,
        key="Am",
        beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]),
        bpm_method=method,
    )
    restored = SampleAnalysis.model_validate_json(analysis.model_dump_json(), strict=True)
    assert restored.key == "Am"
    assert restored.bpm_method == method
    controller.project.sample_analysis[0] = analysis
    controller.project.sample_paths[0] = "samples/period-fixture.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    controller.project.manual_bpm[0] = 90.0
    controller.project.pad_timing_intent[0] = "manual"
    snapshot = capture_copy_snapshot(controller.project, 0)
    copied = CopySnapshotData.model_validate_json(snapshot.payload_json, strict=True)
    assert copied.analysis is not None
    assert copied.analysis.key == "Am"
    assert copied.analysis.bpm_method == method
    assert isinstance(copied.analysis.bpm_method.ordinal_fit.fractional_check, FractionalTempoCheck)
    assert isinstance(copied.analysis.bpm_method.ordinal_fit.integer_check, FractionalTempoCheck)
    assert copied.manual_bpm == 90.0
