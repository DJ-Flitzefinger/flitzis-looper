"""Adaptive probe evidence stays versioned and source-relative."""

import json
import math
import os
from pathlib import Path
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis.app_tempo_models import AppTempoMethod, TempoRegion
from flitzis_looper.copy_snapshot import CopySnapshotData, capture_copy_snapshot
from flitzis_looper.models import PadContentIdentity, SampleAnalysis
from tests.flitzis_looper.controller.test_neural_tempo_restore import _method, _v2_method

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController


def _v4_method(*, short: bool = False) -> AppTempoMethod:
    # Independently exact half-second observations have zero residuals at120BPM.
    data = _method().model_dump(mode="python")
    data.update(schema_version=4, policy="neural-stable-regions-v4")
    duration = 12 if short else 100
    data["frame_count"] = duration * 48000
    data["raw_beats"] = [i / 2 for i in range(duration * 2)]
    data["raw_downbeats"] = [i * 2.0 for i in range(duration // 2)]
    common = {
        "window_seconds": [0.0, 12.0] if short else [20.0, 80.0],
        "bpm": 120.0,
        "retained": 24 if short else 120,
        "excluded_raw_indices": [],
        "max_residual_seconds": 0.0,
        "rms_residual_seconds": 0.0,
    }
    windows = [
        {
            **common,
            "window_seconds": bounds,
            "retained": 12 if short else 32,
        }
        for bounds in (
            [[0.0, math.nextafter(5.5, math.inf)], [6.0, math.nextafter(11.5, math.inf)]]
            if short
            else [[20.0, 36.0], [64.0, 80.0]]
        )
    ]
    data.update(
        status="single_region_estimate" if short else "estimated",
        bpm=120.0,
        selected=common,
        regions=[common],
    )
    data["integer_probe"] = {
        "status": "supported_integer_estimate",
        "reason": "two_windows_and_common_phase",
        "candidate_bpm": 120.0,
        "base_bpm": 120.0,
        "windows": windows,
        "common_region": common,
        "common_max_residual_seconds": 0.0,
        "common_rms_residual_seconds": 0.0,
        "rms_penalty_seconds": 0.0,
        "window_phase_difference_seconds": 0.0,
        "evidence_tier": "short_source" if short else "two_ten_second",
        "window_gap_seconds": 0.5 if short else 28.5,
        "pivot_raw_index": 11.5 if short else 99.5,
        "pivot_seconds": 5.75 if short else 49.75,
        "fractional_check": {
            "status": "pass",
            "max_residual_seconds": 0.0,
            "rms_residual_seconds": 0.0,
            "rms_penalty_seconds": 0.0,
            "window_phase_difference_seconds": 0.0,
        },
    }
    return AppTempoMethod.model_validate(data)


@pytest.mark.parametrize("short", [False, True])
def test_v4_recomputes_shared_pivot_and_both_exact_candidate_periods(*, short: bool) -> None:
    method = _v4_method(short=short)
    assert AppTempoMethod.model_validate_json(method.model_dump_json()) == method
    assert method.bpm == method.selected.bpm == 120.0
    assert method.integer_probe.fractional_check.status == "pass"


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("pivot_raw_index", 11.0),
        ("pivot_seconds", 5.8),
        ("window_gap_seconds", 0.4),
        ("common_max_residual_seconds", 0.001),
        ("common_rms_residual_seconds", 0.001),
        ("rms_penalty_seconds", 0.0001),
        ("window_phase_difference_seconds", 0.001),
        ("evidence_tier", "two_ten_second"),
        ("status", "fractional_estimate"),
        ("reason", "fractional_preferred"),
    ],
)
def test_v4_rejects_forged_pivot_spans_metrics_and_status(field: str, value: object) -> None:
    data = _v4_method(short=True).model_dump(mode="python")
    data["integer_probe"][field] = value
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data)


def test_v4_checks_the_selected_fractional_period_instead_of_substituting_a_free_fit() -> None:
    data = _v4_method().model_dump(mode="python")
    data["integer_probe"]["fractional_check"]["max_residual_seconds"] = 0.001
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data)


def test_only_v4_short_source_can_retain_four_point_probe_windows() -> None:
    data = _v4_method(short=True).model_dump(mode="python")
    data.update(
        frame_count=8 * 48000,
        bpm=60.0,
        raw_beats=list(map(float, range(8))),
        raw_downbeats=[0.0, 4.0],
    )
    common = dict(data["selected"], window_seconds=[0.0, 8.0], bpm=60.0, retained=8)
    data.update(selected=common, regions=[common])
    probe = data["integer_probe"]
    probe.update(
        candidate_bpm=60.0,
        base_bpm=60.0,
        common_region=common,
        pivot_raw_index=3.5,
        pivot_seconds=3.5,
        window_gap_seconds=1.0,
    )
    probe["windows"] = [
        dict(common, window_seconds=bounds, retained=4)
        for bounds in ([0.0, math.nextafter(3.0, math.inf)], [4.0, math.nextafter(7.0, math.inf)])
    ]
    method = AppTempoMethod.model_validate(data)
    assert all(w.retained == 4 for w in method.integer_probe.windows)


def test_short_source_cannot_describe_a_small_island_in_a_long_original() -> None:
    data = _v4_method(short=True).model_dump(mode="python")
    data["frame_count"] = 100 * 48000
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data)


def test_recomputed_affine_metrics_cannot_move_both_probes_into_one_half() -> None:
    data = _v4_method().model_dump(mode="python")
    probe = data["integer_probe"]
    # Both local fits and all shared metrics remain exactly120BPM/zero. Only
    # the source-bound outer-half geometry exposes this candidate cherry-pick.
    probe["windows"][1].update(window_seconds=[36.0, math.nextafter(49.5, math.inf)], retained=28)
    probe["window_gap_seconds"] = 0.5
    with pytest.raises(ValidationError, match="opposite outer retained halves"):
        AppTempoMethod.model_validate(data)


@pytest.mark.parametrize("side", [0, 1])
def test_adaptive_probes_cannot_drop_a_common_outer_endpoint(side: int) -> None:
    data = _v4_method().model_dump(mode="python")
    window = data["integer_probe"]["windows"][side]
    if side == 0:
        window["window_seconds"][0] = 20.5
    else:
        window["window_seconds"][1] = 79.5
    window["retained"] = 31
    with pytest.raises(ValidationError, match="opposite outer retained halves"):
        AppTempoMethod.model_validate(data)


@pytest.mark.skipif(
    "FLITZI_TEMPO_NATIVE_METHOD_ROOT" not in os.environ,
    reason="requires Root's retained source-bound Native v4 replay output",
)
def test_actual_native_v4_replay_obeys_python_common_masks_and_window_geometry() -> None:
    root = Path(os.environ["FLITZI_TEMPO_NATIVE_METHOD_ROOT"])
    assert root.is_absolute()
    for pad in range(1, 4):
        data = json.loads((root / f"pad{pad}-v4-method.json").read_text(encoding="utf-8"))
        original = json.loads(
            (root / "input" / f"pad{pad}-method.json").read_text(encoding="utf-8")
        )
        method = AppTempoMethod.model_validate(data, strict=True)
        assert method.schema_version == 4
        assert method.source_sha256 == original["source_sha256"]
        assert method.raw_beats == original["raw_beats"]
        assert method.raw_downbeats == original["raw_downbeats"]
        assert method.bpm is not None
        assert method.bpm > 0
        if pad == 3:
            probe = method.integer_probe
            assert probe is not None
            assert probe.evidence_tier == "two_ten_second"
            assert probe.fractional_check.status == "pass"
            assert method.bpm < 90


@pytest.mark.parametrize("version", [2, 3])
def test_old_metadata_cannot_import_four_point_adaptive_windows(version: int) -> None:
    data = _v2_method(version).model_dump(mode="python")
    data["integer_probe"]["windows"][0]["retained"] = 4
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data)
    with pytest.raises(ValidationError):
        TempoRegion.model_validate(data["integer_probe"]["windows"][0])


def test_v4_copy_keeps_evidence_and_rejects_contradictory_numeric_or_metric_data(
    controller: AppController,
) -> None:
    method = _v4_method(short=True)
    project = controller.project
    project.sample_paths[0] = "samples/source.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0, key="Am", beat_grid={"beats": [], "downbeats": [], "bars": []}, bpm_method=method
    )
    assert not controller.loader._needs_tempo_refinement(0)
    snapshot = capture_copy_snapshot(project, 0)
    original = snapshot.payload_json
    for changed in ("bpm", "metric"):
        data = json.loads(original)
        if changed == "bpm":
            data["analysis"]["bpm"] = 117.0
        else:
            data["analysis"]["bpm_method"]["integer_probe"]["window_gap_seconds"] = 2.0
        with pytest.raises(ValidationError):
            CopySnapshotData.model_validate(data)
    assert project.sample_analysis[0].bpm_method == method
