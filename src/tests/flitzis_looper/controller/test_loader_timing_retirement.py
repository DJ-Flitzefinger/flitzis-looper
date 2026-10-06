"""Retired native timing events settle bookkeeping without replaying legacy timing."""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.models import SampleAnalysis

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def _legacy_analysis() -> dict[str, object]:
    return {
        "bpm": 120.0,
        "key": "C#m",
        "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0], "bars": [0.0]},
    }


@pytest.mark.parametrize("restored", [False, True])
@pytest.mark.parametrize("include_analysis", [False, True])
def test_current_source_completion_preserves_newer_timing_and_analysis_request(
    controller: AppController,
    audio_engine_mock: Mock,
    *,
    restored: bool,
    include_analysis: bool,
) -> None:
    project = controller.project
    session = controller.session
    cached_path = "samples/current.wav"
    project.sample_paths[0] = cached_path if restored else None
    old_analysis = SampleAnalysis.model_validate(_legacy_analysis())
    project.sample_analysis[0] = old_analysis
    project.manual_bpm[0] = 123.456
    project.pad_grid_anchor_s[0] = -0.125
    project.pad_loop_auto[0] = False
    project.pad_loop_start_s[0] = 3.25
    project.pad_loop_end_s[0] = 5.75
    session.loading_sample_ids.add(0)
    session.pending_sample_paths[0] = cached_path
    controller.loader._load_request_ids[0] = 7
    controller.loader._analysis_request_ids[0] = 9
    session.analyzing_sample_ids.add(0)
    session.sample_analysis_progress[0] = 0.5
    audio_engine_mock.reset_mock()
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 7,
            "duration_s": 32.0,
            "cached_path": cached_path,
            "detected_loop_start_s": 1.0,
            "timing_stale": True,
            "analysis": _legacy_analysis() if include_analysis else None,
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert project.sample_paths[0] == cached_path
    assert project.sample_durations[0] == 32.0
    assert 0 not in session.loading_sample_ids
    assert 0 not in session.pending_sample_paths
    assert 0 not in controller.loader._load_request_ids
    assert project.sample_analysis[0] is old_analysis
    assert project.manual_bpm[0] == 123.456
    assert project.pad_grid_anchor_s[0] == -0.125
    assert project.pad_loop_auto[0] is False
    assert project.pad_loop_start_s[0] == 3.25
    assert project.pad_loop_end_s[0] == 5.75
    assert controller.loader._analysis_request_ids[0] == 9
    assert 0 in session.analyzing_sample_ids
    assert session.sample_analysis_progress[0] == 0.5
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()


def test_retired_analysis_completion_settles_its_task_without_legacy_publication(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old_analysis = SampleAnalysis.model_validate(_legacy_analysis())
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.sample_analysis[0] = old_analysis
    controller.loader._analysis_request_ids[0] = 7
    controller.session.analyzing_sample_ids.add(0)
    controller.session.sample_analysis_progress[0] = 0.5
    audio_engine_mock.reset_mock()
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "task_success",
            "id": 0,
            "request_id": 7,
            "task": "analysis",
            "timing_stale": True,
            "analysis": _legacy_analysis(),
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_analysis[0] is old_analysis
    assert 0 not in controller.session.analyzing_sample_ids
    assert 0 not in controller.session.sample_analysis_progress
    assert 0 not in controller.loader._analysis_request_ids
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
