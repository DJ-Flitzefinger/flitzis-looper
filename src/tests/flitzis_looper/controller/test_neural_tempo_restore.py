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
