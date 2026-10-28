"""Terminal analysis feedback settles its owner when derived Native controls refuse."""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.models import BeatGrid, PadContentIdentity, SampleAnalysis

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


CONTROL_REFUSAL = "content placement publication is pending"
ANALYSIS = {
    "bpm": 120.0,
    "key": "C#m",
    "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0], "bars": [0.0]},
}


def _prepare_running_analyses(
    controller: AppController, audio_engine_mock: Mock, *, automatic: bool
) -> SampleAnalysis:
    project = controller.project
    previous = SampleAnalysis(
        bpm=97.0,
        key="Gm",
        beat_grid=BeatGrid(beats=[1.0, 1.5], downbeats=[1.0], bars=[1.0]),
    )
    for sample_id in (0, 2):
        project.sample_paths[sample_id] = f"samples/synthetic-{sample_id}.wav"
        project.sample_durations[sample_id] = 32.0
        project.pad_content[sample_id] = PadContentIdentity(instance_id=f"{sample_id + 1:032x}")
        project.sample_analysis[sample_id] = previous.model_copy(deep=True)
        project.pad_loop_auto[sample_id] = False
        project.pad_loop_start_s[sample_id] = 1.0
        project.pad_loop_end_s[sample_id] = 3.0
    project.pad_timing_intent[0] = "automatic" if automatic else "legacy"
    audio_engine_mock.pad_timing_intent.return_value = "automatic" if automatic else "legacy"
    audio_engine_mock.current_constant_timing.return_value = None
    audio_engine_mock.loaded_residency.return_value = None
    audio_engine_mock.waveform_source_identity.side_effect = lambda sample_id: (
        sample_id + 4,
        f"{sample_id + 1:064x}",
        48_000 * 32,
        48_000,
    )
    audio_engine_mock.analyze_sample_async.side_effect = [17, 23, 29]
    controller.loader.analyze_sample_async(0)
    controller.loader.analyze_sample_async(2)
    assert controller.loader._analysis_request_ids == {0: 17, 2: 23}
    return previous


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
@pytest.mark.parametrize(
    ("boundary", "key_completion_error"),
    [("automatic_authority", False), ("derived_loop", False), ("derived_loop", True)],
)
def test_analysis_terminal_control_refusal_settles_exact_request_without_replay(
    controller: AppController,
    audio_engine_mock: Mock,
    error_type: type[Exception],
    boundary: str,
    *,
    key_completion_error: bool,
) -> None:
    project = controller.project
    automatic = boundary == "automatic_authority"
    previous = _prepare_running_analyses(controller, audio_engine_mock, automatic=automatic)
    admitted_source_identity = audio_engine_mock.waveform_source_identity.side_effect
    if key_completion_error:
        audio_engine_mock.waveform_source_identity.side_effect = error_type(
            "source key metadata unavailable"
        )
    failure = audio_engine_mock.set_pad_bpm if automatic else audio_engine_mock.set_pad_loop_region
    failure.side_effect = error_type(CONTROL_REFUSAL)
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "task_progress",
            "id": 0,
            "task": "analysis",
            "request_id": 17,
            "percent": 0.75,
            "stage": "Finishing analysis",
        },
        {
            "type": "task_success",
            "id": 0,
            "task": "analysis",
            "request_id": 17,
            "analysis": ANALYSIS,
        },
        {
            "type": "task_progress",
            "id": 2,
            "task": "analysis",
            "request_id": 23,
            "percent": 0.5,
            "stage": "Independent analysis",
        },
        None,
    ]

    controller.loader.poll_loader_events()

    failure.assert_called_once()
    assert 0 not in controller.session.analyzing_sample_ids
    assert 0 not in controller.session.sample_analysis_progress
    assert 0 not in controller.session.sample_analysis_stage
    assert 0 not in controller.loader._analysis_request_ids
    assert not controller.loader._key_analysis.has_request(0)
    assert CONTROL_REFUSAL in controller.session.sample_analysis_errors[0]
    completed_key = project.pad_key_intent[0].source
    if key_completion_error:
        assert completed_key is None
        assert "source key metadata unavailable" in controller.session.sample_analysis_errors[0]
    else:
        assert completed_key is not None
        assert completed_key.raw_key == "C#m"
    if automatic:
        assert project.pad_timing_intent[0] == "automatic"
        assert project.sample_analysis[0] == previous
    else:
        assert project.pad_timing_intent[0] == "legacy"
        expected = ANALYSIS | {"key": previous.key if key_completion_error else ANALYSIS["key"]}
        assert project.sample_analysis[0] == SampleAnalysis.model_validate(expected)
    assert controller.loader._analysis_request_ids == {2: 23}
    assert controller.session.analyzing_sample_ids == {2}
    assert controller.session.sample_analysis_progress[2] == 0.5
    assert controller.session.sample_analysis_stage[2] == "Independent analysis"
    audio_engine_mock.poll_loader_events.side_effect = [None]

    controller.loader.poll_loader_events()

    failure.assert_called_once()
    assert audio_engine_mock.analyze_sample_async.call_count == 2
    assert CONTROL_REFUSAL in controller.session.sample_analysis_errors[0]
    failure.side_effect = None
    audio_engine_mock.waveform_source_identity.side_effect = admitted_source_identity

    controller.loader.analyze_sample_async(0)

    assert controller.loader._analysis_request_ids == {0: 29, 2: 23}
    assert 0 in controller.session.analyzing_sample_ids
    assert 0 not in controller.session.sample_analysis_errors
    failure.assert_called_once()
