"""Rejected publications keep saved performer intent eligible for a real retry."""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.models import BeatGrid, SampleAnalysis

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


@pytest.mark.parametrize("action", ["end", "auto", "bars", "full_track"])
@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
def test_synchronous_refused_loop_edit_retains_project_and_allows_explicit_retry(
    controller: AppController, audio_engine_mock: Mock, action: str, error_type: type[Exception]
) -> None:
    project = controller.project
    project.sample_paths[0] = "samples/synthetic-loop.wav"
    project.sample_durations[0] = 60.0
    project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0, key="C", beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0])
    )
    project.pad_loop_start_s[0] = 1.0
    project.pad_loop_end_s[0] = 5.0
    project.pad_loop_auto[0] = action != "auto"
    project.pad_loop_bars[0] = 2.0
    audio_engine_mock.output_sample_rate.return_value = 48_000
    audio_engine_mock.loaded_residency.return_value = None
    audio_engine_mock.set_pad_loop_region.side_effect = error_type("loop publication rejected")
    before_project = project.model_dump()
    before_session = controller.session.model_dump()
    revision = controller.persistence.revision

    def edit() -> None:
        loop = controller.transport.loop
        if action == "end":
            loop.set_end(0, 7.0)
        elif action == "auto":
            loop.set_auto(0, enabled=True)
        elif action == "bars":
            loop.set_bars(0, bars=4.0)
        else:
            loop.set_full_track_region(0)

    with pytest.raises(error_type, match="loop publication rejected"):
        edit()
    assert project.model_dump() == before_project
    assert controller.session.model_dump() == before_session
    assert controller.persistence.revision == revision
    audio_engine_mock.set_pad_loop_region.assert_called_once()

    audio_engine_mock.set_pad_loop_region.side_effect = None
    edit()
    assert audio_engine_mock.set_pad_loop_region.call_count == 2
    assert project.model_dump() != before_project
    assert controller.persistence.revision > revision


@pytest.mark.parametrize("action", ["enable", "disable", "step", "legacy_mode"])
@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
def test_refused_quantization_keeps_previous_intent_and_identical_retry_reaches_native(
    controller: AppController, audio_engine_mock: Mock, action: str, error_type: type[Exception]
) -> None:
    project = controller.project
    project.trigger_quantization_enabled = action in {"disable", "step"}
    project.trigger_quantization_step = "1_64"
    before = project.model_dump()
    revision = controller.persistence.revision
    audio_engine_mock.set_trigger_quantization.side_effect = error_type("quantization queue full")

    def edit() -> None:
        params = controller.transport.global_params
        if action == "enable":
            params.set_trigger_quantization_enabled(enabled=True)
        elif action == "disable":
            params.set_trigger_quantization_enabled(enabled=False)
        elif action == "step":
            params.set_trigger_quantization_step("1_32")
        else:
            params.set_trigger_quantization("1_32")

    with pytest.raises(error_type, match="quantization queue full"):
        edit()
    assert project.model_dump() == before
    assert controller.persistence.revision == revision
    audio_engine_mock.set_trigger_quantization.assert_called_once()

    audio_engine_mock.set_trigger_quantization.side_effect = None
    edit()
    assert audio_engine_mock.set_trigger_quantization.call_count == 2
    assert project.model_dump() != before
    assert controller.persistence.revision > revision
