"""A regional hull keeps original ordinals, exact base and version boundaries."""

import hashlib
import json
import math
import os
import struct
from copy import deepcopy
from pathlib import Path
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.copy_snapshot import (
    CopySnapshot,
    CopySnapshotData,
    capture_copy_snapshot,
    project_with_copy_snapshot,
)
from flitzis_looper.models import PadContentIdentity, SampleAnalysis
from tests.flitzis_looper.controller.test_adaptive_tempo_probe import _v4_method

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController


def _region(bounds: list[float], count: int) -> dict[str, object]:
    return {
        "window_seconds": bounds,
        "bpm": 60.0 / 0.875,
        "retained": count,
        "excluded_raw_indices": [],
        "max_residual_seconds": 0.0,
        "rms_residual_seconds": 0.0,
    }


def _envelope_data() -> dict[str, object]:
    # Binary-exact 7/8-second observations span the first two source thirds.
    # The noisy last third invalidates middle/whole/complete-run fits. The
    # selected first third itself is not a maximal original IOI run.
    data = _v4_method().model_dump(mode="python")
    beats = [i * 0.875 for i in range(343)]
    for i, value in enumerate(beats):
        if value >= 200.0:
            beats[i] += 0.125 if i % 2 else -0.125
    base = 60.0 / 0.875
    regions = [_region([0.0, 100.0], 115), _region([100.0, 200.0], 114)]
    common = _region([0.0, 200.0], 229)
    # The integer and selected-base metrics are independent analytic results
    # for indices 0..228, centered on index114/time99.75, not helper output.
    error = 0.875 - 60.0 / 69.0
    integer_rms = error * math.sqrt((229**2 - 1) / 12)
    data.update(
        schema_version=5,
        policy="neural-stable-regions-v5",
        bpm=base,
        selected=regions[0],
        regions=regions,
        frame_count=300 * 48000,
        raw_beats=beats,
        raw_downbeats=beats[::4],
        integer_probe={
            "status": "fractional_estimate",
            "reason": "long_span_mismatch",
            "candidate_bpm": 69.0,
            "base_bpm": base,
            "windows": [
                _region([0.0, math.nextafter(15.75, math.inf)], 19),
                _region([183.75, math.nextafter(199.5, math.inf)], 19),
            ],
            "common_region": common,
            "common_max_residual_seconds": error * 114,
            "common_rms_residual_seconds": integer_rms,
            "rms_penalty_seconds": integer_rms,
            "window_phase_difference_seconds": error * 210,
            "evidence_tier": "two_ten_second",
            "window_gap_seconds": 168.0,
            "pivot_raw_index": 114.0,
            "pivot_seconds": 99.75,
            "fractional_check": {
                "status": "pass",
                "max_residual_seconds": 0.0,
                "rms_residual_seconds": 0.0,
                "rms_penalty_seconds": 0.0,
                "window_phase_difference_seconds": 0.0,
            },
        },
    )
    return data


def _inconclusive(data: dict[str, object]) -> dict[str, object]:
    result = deepcopy(data)
    probe = result["integer_probe"]
    probe.update(
        status="inconclusive",
        reason="insufficient_probe_coverage",
        windows=[],
        common_region=None,
        common_max_residual_seconds=None,
        common_rms_residual_seconds=None,
        rms_penalty_seconds=None,
        window_phase_difference_seconds=None,
        evidence_tier="inconclusive",
        window_gap_seconds=None,
        pivot_raw_index=None,
        pivot_seconds=None,
        fractional_check={
            "status": "inconclusive",
            "max_residual_seconds": None,
            "rms_residual_seconds": None,
            "rms_penalty_seconds": None,
            "window_phase_difference_seconds": None,
        },
    )
    return result


def test_v5_uses_one_regional_hull_and_keeps_the_exact_selected_fractional_base() -> None:
    method = AppTempoMethod.model_validate(_envelope_data(), strict=True)
    assert AppTempoMethod.model_validate_json(method.model_dump_json(), strict=True) == method
    assert method.bpm == method.selected.bpm == 60.0 / 0.875
    assert method.integer_probe.status == "fractional_estimate"
    assert method.integer_probe.common_region.window_seconds == [0.0, 200.0]
    assert method.integer_probe.common_region not in method.regions
    assert method.frame_count / method.sample_rate_hz == 300.0
    assert method.integer_probe.common_region.excluded_raw_indices == []
    assert method.integer_probe.common_region.retained == 229


def test_v5_recomputes_a_genuine_isolated_error_mask_from_original_indices() -> None:
    data = _envelope_data()
    data["raw_beats"][150] += 0.125
    data["regions"][1].update(retained=113, excluded_raw_indices=[150])
    probe = data["integer_probe"]
    probe["common_region"].update(retained=228, excluded_raw_indices=[150])
    indices = [i for i in range(229) if i != 150]
    pivot = sum(indices) / len(indices)
    error = 0.875 - 60.0 / 69.0
    rms = error * math.sqrt(sum((i - pivot) ** 2 for i in indices) / len(indices))
    probe.update(
        pivot_raw_index=pivot,
        pivot_seconds=0.875 * pivot,
        common_max_residual_seconds=error * max(pivot, 228 - pivot),
        common_rms_residual_seconds=rms,
        rms_penalty_seconds=rms,
    )
    method = AppTempoMethod.model_validate(data, strict=True)
    assert method.integer_probe.common_region.excluded_raw_indices == [150]
    assert method.raw_beats[150] == 131.375
    assert method.integer_probe.pivot_raw_index == pivot
    assert method.bpm == 60.0 / 0.875


@pytest.mark.parametrize("version", [4, 5])
def test_v4_preferred_middle_and_short_paths_keep_their_previous_contract(version: int) -> None:
    for short in (False, True):
        data = _v4_method(short=short).model_dump(mode="python")
        data.update(schema_version=version, policy=f"neural-stable-regions-v{version}")
        assert AppTempoMethod.model_validate(data, strict=True).bpm == 120.0


def test_old_v4_inconclusive_remains_valid_and_cannot_import_a_v5_envelope() -> None:
    data = _inconclusive(_envelope_data())
    data.update(schema_version=4, policy="neural-stable-regions-v4")
    method = AppTempoMethod.model_validate(data, strict=True)
    assert method.integer_probe.status == "inconclusive"
    assert method.bpm == method.selected.bpm
    data = _envelope_data()
    data.update(schema_version=4, policy="neural-stable-regions-v4")
    with pytest.raises(ValidationError, match="genuine preferred common"):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize(
    "damage", ["missing", "smaller", "larger", "mask", "metric", "pivot", "base", "status"]
)
def test_v5_rejects_forged_or_missing_derived_hull_evidence(damage: str) -> None:
    data = _envelope_data()
    probe = data["integer_probe"]
    if damage == "missing":
        data = _inconclusive(data)
    elif damage in {"smaller", "larger"}:
        probe["common_region"]["window_seconds"][1] += -1.0 if damage == "smaller" else 1.0
    elif damage == "mask":
        # Removing one perfectly affine point leaves all fit residuals zero.
        # Recomputing the five-pass mask must still reject this convenient mask.
        probe["common_region"].update(retained=228, excluded_raw_indices=[5])
    elif damage == "metric":
        probe["common_region"]["max_residual_seconds"] = 0.001
    elif damage == "pivot":
        probe["pivot_raw_index"] = 113.0
    elif damage == "base":
        probe["base_bpm"] = 69.0
    else:
        probe.update(status="supported_integer_estimate", reason="two_windows_and_common_phase")
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize(
    "damage", ["non_third", "duplicate", "conflicting", "mask", "bpm", "metric", "extra_third"]
)
def test_v5_rejects_forged_supporting_thirds_without_filtering(damage: str) -> None:
    data = _envelope_data()
    other = data["regions"][1]
    if damage == "non_third":
        other["window_seconds"][0] += 0.001
    elif damage == "duplicate":
        data["regions"].append(deepcopy(other))
    elif damage == "conflicting":
        other["bpm"] = 69.0
    elif damage == "mask":
        other.update(retained=113, excluded_raw_indices=[120])
    elif damage == "bpm":
        other["bpm"] *= 1.0001
    elif damage == "metric":
        other["rms_residual_seconds"] = 0.001
    else:
        data["regions"].append(_region([200.0, 300.0], 114))
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


def test_envelope_cannot_override_an_existing_preferred_middle() -> None:
    data = _envelope_data()
    data["regions"].append(_region([60.0, 240.0], 206))
    with pytest.raises(ValidationError, match="genuine preferred common"):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize("omitted", ["conflicting_third", "compatible_third", "preferred_middle"])
def test_complete_raw_support_cannot_be_omitted_from_an_otherwise_valid_prefix_hull(
    controller: AppController,
    omitted: str,
) -> None:
    data = _envelope_data()
    if omitted == "conflicting_third":
        beats = [i * 0.875 for i in range(229)] + [200.375 + i for i in range(100)]
    elif omitted == "compatible_third":
        beats = [i * 0.875 + (0.25 if i >= 229 else 0.0) for i in range(343)]
    else:
        beats = [i * 0.875 for i in range(343)]
        for i, value in enumerate(beats):
            if value >= 240.0:
                beats[i] += 0.125 if i % 2 else -0.125
    # ALL first-two-thirds observations are unchanged. Their masks, selected
    # base, prefix-hull pivot and both candidate/window metrics remain valid.
    assert beats[:229] == data["raw_beats"][:229]
    data["raw_beats"] = beats
    data["raw_downbeats"] = beats[::4]
    reason = "preferred middle" if omitted == "preferred_middle" else "complete canonical"
    with pytest.raises(ValidationError, match=reason):
        AppTempoMethod.model_validate(data, strict=True)
    method = AppTempoMethod.model_validate(_envelope_data(), strict=True)
    project = controller.project
    project.sample_paths[0] = "samples/owned-envelope-fixture.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    project.sample_analysis[0] = SampleAnalysis(
        bpm=method.bpm,
        key="Am",
        beat_grid={"beats": [], "downbeats": [], "bars": []},
        bpm_method=method,
    )
    damaged = json.loads(capture_copy_snapshot(project, 0).payload_json)
    damaged["analysis"]["bpm_method"] = data
    with pytest.raises(ValidationError, match=reason):
        CopySnapshotData.model_validate(damaged)


def test_hull_cannot_skip_an_unreported_unstable_middle_third() -> None:
    data = _envelope_data()
    beats = [i * 0.875 for i in range(343)]
    for i, value in enumerate(beats):
        if 100.0 <= value < 200.0:
            beats[i] += 0.125 if i % 2 else -0.125
    data["raw_beats"] = beats
    data["raw_downbeats"] = beats[::4]
    data["regions"][1] = _region([200.0, 300.0], 114)
    data["integer_probe"]["common_region"] = _region([0.0, 300.0], 343)
    with pytest.raises(ValidationError, match="through all original observations"):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize("indices", [[130, 131], list(range(120, 229, 10))])
def test_support_masks_cannot_relax_adjacent_or_five_percent_exclusion_limits(
    indices: list[int],
) -> None:
    data = _envelope_data()
    for index in indices:
        data["raw_beats"][index] += 0.125
    data["regions"][1].update(retained=114 - len(indices), excluded_raw_indices=indices)
    data["integer_probe"]["common_region"].update(
        retained=229 - len(indices), excluded_raw_indices=indices
    )
    with pytest.raises(ValidationError, match="supporting third differs"):
        AppTempoMethod.model_validate(data, strict=True)


def test_v5_strict_copy_retains_observations_and_rejects_forged_support(
    controller: AppController,
) -> None:
    method = AppTempoMethod.model_validate(_envelope_data(), strict=True)
    project = controller.project
    project.sample_paths[0] = "samples/owned-envelope-fixture.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    project.sample_analysis[0] = SampleAnalysis(
        bpm=method.bpm,
        key="Am",
        beat_grid={"beats": [], "downbeats": [], "bars": []},
        bpm_method=method,
    )
    assert not controller.loader._needs_tempo_refinement(0)
    snapshot = capture_copy_snapshot(project, 0)
    copied = project_with_copy_snapshot(
        project,
        1,
        CopySnapshot.model_validate_json(snapshot.model_dump_json()),
        target_identity=PadContentIdentity(instance_id="b" * 32, copied_from="a" * 32),
    )
    assert copied.sample_analysis[1].bpm_method == method
    for damage in ("support", "hull_mask"):
        damaged = json.loads(snapshot.payload_json)
        method_data = damaged["analysis"]["bpm_method"]
        if damage == "support":
            method_data["regions"][1]["bpm"] *= 1.0001
        else:
            method_data["integer_probe"]["common_region"].update(
                retained=228, excluded_raw_indices=[5]
            )
        with pytest.raises(ValidationError):
            CopySnapshotData.model_validate(damaged)


def test_positive_v4_inconclusive_cache_does_not_schedule_automatic_refinement(
    controller: AppController,
) -> None:
    data = _inconclusive(_envelope_data())
    data.update(schema_version=4, policy="neural-stable-regions-v4")
    method = AppTempoMethod.model_validate(data, strict=True)
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=method.bpm,
        key="Am",
        beat_grid={"beats": [], "downbeats": [], "bars": []},
        bpm_method=method,
    )
    analysis = controller.project.sample_analysis[0]
    assert SampleAnalysis.model_validate_json(analysis.model_dump_json()) == analysis
    controller.project.sample_paths[0] = "samples/owned-old-v4-fixture.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    snapshot = capture_copy_snapshot(controller.project, 0)
    assert CopySnapshotData.model_validate_json(snapshot.payload_json).analysis == analysis
    assert not controller.loader._needs_tempo_refinement(0)


@pytest.mark.skipif(
    "FLITZI_CORROBORATED_HULL_REPLAY_ROOT" not in os.environ,
    reason="requires Root's retained source-bound Native v5 replay output",
)
def test_actual_native_v5_fractional_replay_keeps_exact_base_and_full_raw_arrays() -> None:
    path = Path(os.environ["FLITZI_CORROBORATED_HULL_REPLAY_ROOT"]) / "source-bound-flac-v5.json"
    original_path = Path(os.environ["FLITZI_CORROBORATED_HULL_SOURCE_REPORT"])
    assert path.is_absolute()
    assert original_path.is_absolute()
    original_bytes = original_path.read_bytes()
    original = json.loads(json.loads(original_bytes)["bpm_method"])
    report = json.loads(path.read_text(encoding="utf-8"))
    assert report["source_report_sha256"] == hashlib.sha256(original_bytes).hexdigest()
    method = AppTempoMethod.model_validate(report["method"], strict=True)
    assert method.schema_version == 5
    assert method.raw_beats == original["raw_beats"]
    assert method.raw_downbeats == original["raw_downbeats"]
    for name in ("raw_beats", "raw_downbeats"):
        actual = getattr(method, name)
        assert struct.pack(f"<{len(actual)}d", *actual) == struct.pack(
            f"<{len(original[name])}d", *original[name]
        )
    assert method.source_sha256 == original["source_sha256"]
    assert method.mono_sha256 == original["mono_sha256"]
    assert method.model_sha256 == original["model_sha256"]
    assert method.frontend_id == original["frontend_id"]
    assert method.sample_rate_hz == original["sample_rate_hz"]
    assert method.frame_count == original["frame_count"]
    assert method.bpm == method.selected.bpm == original["bpm"]
    assert method.integer_probe.status == "fractional_estimate"
    assert method.integer_probe.fractional_check.status == "pass"
    assert method.integer_probe.common_region.retained == 303
    assert method.integer_probe.common_region.window_seconds == [
        0.0,
        2.0 * (method.frame_count / method.sample_rate_hz) / 3.0,
    ]
    assert method.integer_probe.common_region.bpm != method.selected.bpm
