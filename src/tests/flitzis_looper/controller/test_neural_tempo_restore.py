"""Optional tempo metadata must not replace performer/key/current timing intent."""

import json
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.copy_snapshot import (
    CopySnapshot,
    CopySnapshotData,
    capture_copy_snapshot,
    project_with_copy_snapshot,
)
from flitzis_looper.models import BeatGrid, PadContentIdentity, ProjectState, SampleAnalysis

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController


def _method() -> AppTempoMethod:
    region = {
        "window_seconds": [20.0, 80.0],
        "bpm": 120.0,
        "retained": 120,
        "excluded_raw_indices": [],
        "max_residual_seconds": 0.01,
        "rms_residual_seconds": 0.005,
    }
    return AppTempoMethod.model_validate({
        "schema_version": 1,
        "backend": "beat_this",
        "policy": "neural-stable-regions-v1",
        "status": "estimated",
        "bpm": 120.0,
        "selected": region,
        "regions": [region],
        "model_sha256": "1" * 64,
        "frontend_id": "beat-this-1.1.0-soxr-hq-logmel-v1",
        "source_sha256": "2" * 64,
        "mono_sha256": "3" * 64,
        "sample_rate_hz": 48000,
        "frame_count": 4800000,
        "raw_beats": [i * 0.5 for i in range(200)],
        "raw_downbeats": [i * 2.0 for i in range(50)],
    })


def _previous() -> SampleAnalysis:
    return SampleAnalysis(
        bpm=117.0, key="D#m", beat_grid=BeatGrid(beats=[0.1], downbeats=[], bars=[])
    )


def _v2_method(version: int = 2) -> AppTempoMethod:
    data = _method().model_dump(mode="python")
    data.update(schema_version=version, policy=f"neural-stable-regions-v{version}")
    data["selected"]["bpm"] = 119.999
    data["regions"][0]["bpm"] = 119.999
    common = data["selected"]
    windows = [
        {**common, "window_seconds": bounds, "retained": 32}
        for bounds in ([20.0, 36.0], [64.0, 80.0])
    ]
    data["integer_probe"] = {
        "status": "supported_integer_estimate",
        "reason": "two_windows_and_common_phase",
        "candidate_bpm": 120.0,
        "base_bpm": 119.999,
        "windows": windows,
        "common_region": common,
        "common_max_residual_seconds": 0.01,
        "common_rms_residual_seconds": 0.005,
        "rms_penalty_seconds": 0.00001,
        "window_phase_difference_seconds": 0.001,
    }
    return AppTempoMethod.model_validate(data)


@pytest.mark.parametrize("version", [2, 3])
def test_v2_preserves_base_fit_and_validates_integer_preference(version: int) -> None:
    method = _v2_method(version)
    reopened = AppTempoMethod.model_validate_json(method.model_dump_json())
    assert reopened == method
    assert reopened.bpm == 120.0
    assert reopened.selected is not None
    assert reopened.selected.bpm == 119.999
    for key, value in (
        ("base_bpm", 119.99),
        ("candidate_bpm", 120.5),
        ("window_phase_difference_seconds", 0.006),
        ("rms_penalty_seconds", 0.0003),
        ("common_max_residual_seconds", 0.026),
        ("common_rms_residual_seconds", 1.0),
        ("windows", []),
        ("common_region", None),
    ):
        damaged = method.model_dump(mode="python")
        damaged["integer_probe"][key] = value
        with pytest.raises(ValidationError):
            AppTempoMethod.model_validate(damaged)
    damaged = method.model_dump(mode="python")
    damaged["bpm"] = 119.999
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(damaged)
    for key, value in (("retained", 31), ("excluded_raw_indices", [45, 45])):
        damaged = method.model_dump(mode="python")
        damaged["integer_probe"]["windows"][0][key] = value
        with pytest.raises(ValidationError):
            AppTempoMethod.model_validate(damaged)
    damaged = method.model_dump(mode="python")
    damaged["raw_beats"][40:72] = [20 + i * 0.05 for i in range(32)]
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(damaged)
    damaged = method.model_dump(mode="python")
    damaged["schema_version"] = 1
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(damaged)


def test_restore_upgrades_old_policy_only_without_performer_override(
    controller: AppController,
) -> None:
    project = controller.project
    project.sample_analysis[0] = _previous().model_copy(update={"bpm_method": _method()})
    assert controller.loader._needs_tempo_refinement(0)
    project.sample_analysis[0] = _previous().model_copy(
        update={"bpm": 120.0, "bpm_method": _v2_method()}
    )
    assert not controller.loader._needs_tempo_refinement(0)
    project.sample_analysis[0] = _previous().model_copy(update={"bpm_method": _method()})
    for intent in ("manual", "tap", "automatic"):
        project.pad_timing_intent[0] = intent
        assert not controller.loader._needs_tempo_refinement(0)
    project.pad_timing_intent[0] = "legacy"
    project.manual_bpm[0] = 119.5
    assert not controller.loader._needs_tempo_refinement(0)


@pytest.mark.parametrize("version", [2, 3])
def test_v2_copy_and_json_keep_the_finite_prior_and_original_observations(version: int) -> None:
    project = ProjectState()
    project.sample_paths[0] = "samples/legacy.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    project.sample_analysis[0] = _previous().model_copy(
        update={"bpm": 120.0, "bpm_method": _v2_method(version)}
    )
    snapshot = capture_copy_snapshot(project, 0)
    reopened = CopySnapshot.model_validate_json(snapshot.model_dump_json())
    copied = project_with_copy_snapshot(
        project,
        1,
        reopened,
        target_identity=PadContentIdentity(instance_id="b" * 32, copied_from="a" * 32),
    )
    assert copied.sample_analysis[1] == project.sample_analysis[0]
    damaged = json.loads(snapshot.payload_json)
    damaged["analysis"]["bpm"] = 117.0
    loaded = SampleAnalysis.model_validate(damaged["analysis"])
    assert loaded.bpm == 117.0
    assert loaded.bpm_method is None
    with pytest.raises(ValidationError):
        CopySnapshotData.model_validate(damaged)


def _unavailable_method(version: int) -> AppTempoMethod:
    data = _v2_method(version).model_dump(mode="python")
    data.update(status="insufficient", bpm=None, selected=None, regions=[], integer_probe=None)
    return AppTempoMethod.model_validate(data)


def test_unavailable_v2_upgrade_settles_once_without_changing_other_analysis(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    previous = _previous().model_copy(update={"bpm": 0.0, "bpm_method": _unavailable_method(2)})
    controller.project.sample_analysis[0] = previous
    assert controller.loader._needs_tempo_refinement(0)
    for intent in ("manual", "tap", "automatic"):
        controller.project.pad_timing_intent[0] = intent
        assert not controller.loader._needs_tempo_refinement(0)
    controller.project.pad_timing_intent[0] = "legacy"
    controller.project.manual_bpm[0] = 94.0
    assert not controller.loader._needs_tempo_refinement(0)
    controller.project.manual_bpm[0] = None
    audio_engine_mock.pad_timing_intent.return_value = "legacy"
    controller.loader._finish_loaded_timing(
        0,
        {
            "analysis_kind": "tempo_only",
            "analysis": {"bpm_method": _unavailable_method(3).model_dump_json()},
        },
        new_assignment=False,
        restored_assignment=False,
        timing_stale=False,
    )
    updated = controller.project.sample_analysis[0]
    assert updated is not None
    assert updated.bpm == previous.bpm
    assert updated.key == previous.key
    assert updated.beat_grid == previous.beat_grid
    assert updated.bpm_method == _unavailable_method(3)
    assert not controller.loader._needs_tempo_refinement(0)
    # An unavailable optional result never replaces a previously useful estimate.
    controller.project.sample_analysis[0] = previous.model_copy(update={"bpm": 94.0})
    assert not controller.loader._needs_tempo_refinement(0)
    controller.loader._apply_tempo_only_analysis(
        0, {"bpm_method": _unavailable_method(3).model_dump_json()}
    )
    assert controller.project.sample_analysis[0].bpm_method == previous.bpm_method


@pytest.mark.parametrize("auto", [False, True])
def test_tempo_restore_preserves_key_start_offset_and_newer_key_work(
    controller: AppController, audio_engine_mock: Mock, *, auto: bool
) -> None:
    project = controller.project
    project.sample_paths[0] = "samples/already-loaded.wav"
    project.sample_analysis[0] = _previous()
    project.pad_loop_auto[0] = auto
    project.pad_loop_bars[0] = 8.0
    project.pad_loop_start_s[0] = 5.0
    project.pad_loop_end_s[0] = 10.0
    project.pad_grid_anchor_s[0] = 0.13
    project.pad_grid_offset_samples[0] = -173
    before_key = project.pad_key_intent[0].model_copy(deep=True)
    controller.loader._analysis_request_ids[0] = 999
    audio_engine_mock.pad_timing_intent.return_value = "legacy"
    controller.loader._finish_loaded_timing(
        0,
        {"analysis_kind": "tempo_only", "analysis": {"bpm_method": _method().model_dump_json()}},
        new_assignment=False,
        restored_assignment=False,
        timing_stale=False,
    )
    updated = project.sample_analysis[0]
    assert updated is not None
    assert updated.bpm == 120.0
    assert updated.key == "D#m"
    assert project.pad_key_intent[0] == before_key
    assert controller.loader._analysis_request_ids[0] == 999
    assert project.pad_loop_start_s[0] == 5.0
    if not auto:
        assert project.pad_loop_end_s[0] == 10.0
    assert project.pad_grid_anchor_s[0] == 0.13
    assert project.pad_grid_offset_samples[0] == -173
    audio_engine_mock.set_pad_loop_region.assert_called_with(0, 5.0, 21.0 if auto else 10.0)


@pytest.mark.parametrize("reason", ["manual", "tap", "automatic", "native", "stale"])
def test_later_timing_wishes_win_over_optional_tempo(
    controller: AppController, audio_engine_mock: Mock, reason: str
) -> None:
    previous = _previous()
    controller.project.sample_analysis[0] = previous
    audio_engine_mock.pad_timing_intent.return_value = (
        "automatic" if reason == "native" else "legacy"
    )
    if reason in {"manual", "tap"}:
        controller.project.manual_bpm[0] = 119.25
        controller.project.pad_timing_intent[0] = reason
    elif reason == "automatic":
        controller.project.pad_timing_intent[0] = "automatic"
    controller.loader._finish_loaded_timing(
        0,
        {"analysis_kind": "tempo_only", "analysis": {"bpm_method": _method().model_dump_json()}},
        new_assignment=False,
        restored_assignment=False,
        timing_stale=reason == "stale",
    )
    assert controller.project.sample_analysis[0] is previous


def test_method_roundtrip_preserves_full_observations_and_old_serialization() -> None:
    original = _previous()
    assert "bpm_method" not in json.loads(original.model_dump_json())
    method = _method()
    saved = original.model_copy(update={"bpm_method": method})
    restored = SampleAnalysis.model_validate_json(saved.model_dump_json())
    assert restored.bpm_method == method
    assert restored.bpm_method.raw_beats == method.raw_beats
    invalid = method.model_dump(mode="python")
    invalid["raw_beats"] = [0.5, 0.1]
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(invalid)
    damaged = json.loads(saved.model_dump_json())
    damaged["bpm_method"] = invalid
    loaded = SampleAnalysis.model_validate(damaged)
    assert loaded.bpm == original.bpm
    assert loaded.key == original.key
    assert loaded.bpm_method is None


def test_copy_json_reopen_preserves_method_and_rejects_invalid_nested_metadata() -> None:
    project = ProjectState()
    project.sample_paths[0] = "samples/legacy.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    project.sample_analysis[0] = _previous().model_copy(update={"bpm_method": _method()})
    snapshot = capture_copy_snapshot(project, 0)
    reopened = CopySnapshot.model_validate_json(snapshot.model_dump_json())
    copied = project_with_copy_snapshot(
        project,
        1,
        reopened,
        target_identity=PadContentIdentity(instance_id="b" * 32, copied_from="a" * 32),
    )
    assert copied.sample_analysis[1] == project.sample_analysis[0]
    assert copied.sample_analysis[1] is not project.sample_analysis[0]
    malformed = json.loads(snapshot.payload_json)
    malformed["analysis"]["bpm_method"]["raw_beats"] = [0.5, 0.1]
    with pytest.raises(ValidationError):
        CopySnapshotData.model_validate(malformed)


def test_failed_same_source_beats_preserve_tempo_but_complete_key(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    previous = _previous().model_copy(update={"bpm_method": _method()})
    controller.project.sample_analysis[0] = previous
    controller.loader._analysis_request_ids[0] = 7
    completed = Mock(return_value=True)
    monkeypatch.setattr(controller.loader._key_analysis, "complete", completed)
    # This beat-failure fixture has no automatic-map backend. A mock-valued
    # version attribute otherwise records an equality probe as an audio call.
    audio_engine_mock.AUTOMATIC_SOURCE_BEATMAP_SELECTION_API_VERSION = 0
    audio_engine_mock.reset_mock()
    controller.loader._handle_task_success(
        0,
        {
            "task": "analysis",
            "request_id": 7,
            "analysis_error": "Beat This unavailable",
            "analysis": {
                "bpm": 0.0,
                "key": "Am",
                "beat_grid": {"beats": [], "downbeats": [], "bars": []},
            },
        },
    )
    updated = controller.project.sample_analysis[0]
    assert updated is not None
    assert updated.bpm == previous.bpm
    assert updated.beat_grid == previous.beat_grid
    assert updated.bpm_method == previous.bpm_method
    assert updated.key == "Am"
    completed.assert_called_once_with(0, 7, "Am")
    assert audio_engine_mock.mock_calls == []
    assert controller.session.sample_analysis_errors[0] == "Beat This unavailable"
