"""UI gestures report native admission failures without losing the next gesture."""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.models import BeatGrid, SampleAnalysis
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.conftest import current_timing_metadata

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def _context(controller: AppController, audio: Mock) -> UiContext:
    """Configure synthetic loaded state without opening a source or device."""
    project = controller.project
    project.sample_paths[0] = "samples/synthetic-pad.wav"
    project.sample_paths[1] = "samples/synthetic-peer.wav"
    project.sample_durations[0] = 60.0
    project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
    )
    project.pad_loop_start_s[0] = 0.0
    project.pad_loop_end_s[0] = 2.0
    project.pad_loop_auto[0] = False
    project.multi_loop = False
    controller.session.waveform_editor_pad_id = 0
    controller.session.active_sample_ids.add(1)
    audio.output_sample_rate.return_value = 48_000
    audio.loaded_residency.return_value = None
    return UiContext(controller)


def _loop_intent(controller: AppController) -> tuple[float, float | None, bool, float]:
    project = controller.project
    return (
        project.pad_loop_start_s[0],
        project.pad_loop_end_s[0],
        project.pad_loop_auto[0],
        project.pad_loop_bars[0],
    )


def _assert_no_playback_mutation(controller: AppController, audio: Mock) -> None:
    """Admission failure must not start a voice, stop peers or invent a retry."""
    assert controller.session.active_sample_ids == {1}
    audio.publish_pad_start.assert_not_called()
    audio.prepare_captured_pad_start.assert_not_called()
    audio.play_sample.assert_not_called()
    audio.play_sample_exclusive.assert_not_called()
    audio.stop_sample.assert_not_called()
    audio.cancel_pad_launches.assert_not_called()
    audio.cancel_all_launches.assert_not_called()


def test_grid_offset_queue_failure_preserves_intent_and_next_gesture_succeeds(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ctx = _context(controller, audio_engine_mock)
    project = controller.project
    project.pad_grid_anchor_s[0] = 0.025
    project.pad_grid_offset_samples[0] = 12
    project.pad_timing_intent[0] = "automatic"
    audio_engine_mock.current_constant_timing.return_value = current_timing_metadata()
    audio_engine_mock.pad_timing_intent.return_value = "automatic"
    before_loop = _loop_intent(controller)
    failure = "Failed to send SetPadTimingMetadata - buffer may be full"
    audio_engine_mock.set_pad_timing_metadata.side_effect = RuntimeError(failure)

    ctx.audio.pads.set_pad_grid_offset_samples(0, -240)

    assert project.pad_grid_offset_samples[0] == 12
    assert project.pad_timing_intent[0] == "automatic"
    assert _loop_intent(controller) == before_loop
    assert ctx.audio.pads.residency_status(0)[1] == failure
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.capture_pad_start.assert_not_called()
    _assert_no_playback_mutation(controller, audio_engine_mock)
    controller.transport.residency.poll()
    assert audio_engine_mock.set_pad_timing_metadata.call_count == 1

    def publish_legacy_origin(_sample_id: int, _origin: float, **_options: object) -> None:
        audio_engine_mock.current_constant_timing.return_value = None
        audio_engine_mock.pad_timing_intent.return_value = "legacy"

    audio_engine_mock.set_pad_timing_metadata.side_effect = publish_legacy_origin

    ctx.audio.pads.set_pad_grid_offset_samples(0, -240)

    assert project.pad_grid_offset_samples[0] == -240
    assert project.pad_timing_intent[0] == "legacy"
    assert _loop_intent(controller) == before_loop
    assert ctx.audio.pads.residency_status(0) == (None, None)
    audio_engine_mock.set_pad_loop_region.assert_called_once()
    audio_engine_mock.capture_pad_start.assert_not_called()


@pytest.mark.parametrize("gesture", ["pad", "waveform_edit_and_play"])
def test_unavailable_start_reports_error_and_waits_for_a_new_gesture(
    controller: AppController, audio_engine_mock: Mock, gesture: str
) -> None:
    ctx = _context(controller, audio_engine_mock)
    capture = audio_engine_mock.capture_pad_start.side_effect
    failure = "current complete source/timing unavailable"
    audio_engine_mock.capture_pad_start.side_effect = ValueError(failure)
    controller.session.pad_playhead_s[0] = 1.25
    controller.session.paused_sample_ids.add(0)

    if gesture == "pad":
        ctx.audio.pads.trigger_pad(0, received_at_ns=17)
    else:
        ctx.ui.waveform.set_loop_start_and_play_selected_pad(0.25, received_at_ns=17)

    assert ctx.audio.pads.residency_status(0)[1] == failure
    assert controller.session.pad_playhead_s[0] == 1.25
    assert 0 in controller.session.paused_sample_ids
    _assert_no_playback_mutation(controller, audio_engine_mock)
    assert audio_engine_mock.capture_pad_start.call_count == 1
    controller.transport.residency.poll()
    controller.transport.residency.poll()
    assert audio_engine_mock.capture_pad_start.call_count == 1
    audio_engine_mock.publish_pad_start.assert_not_called()
    audio_engine_mock.capture_pad_start.side_effect = capture

    if gesture == "pad":
        ctx.audio.pads.trigger_pad(0, received_at_ns=29)
    else:
        ctx.ui.waveform.set_loop_start_and_play_selected_pad(0.5, received_at_ns=29)

    assert audio_engine_mock.capture_pad_start.call_count == 2
    assert audio_engine_mock.capture_pad_start.call_args.kwargs["received_at_ns"] == 29
    audio_engine_mock.publish_pad_start.assert_called_once()
    assert ctx.audio.pads.residency_status(0) == (None, None)
    audio_engine_mock.stop_sample.assert_not_called()


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
def test_failed_waveform_loop_publication_skips_dependent_restart(
    controller: AppController, audio_engine_mock: Mock, error_type: type[Exception]
) -> None:
    ctx = _context(controller, audio_engine_mock)
    before_loop = _loop_intent(controller)
    failure = "loop publication unavailable for the current source"
    audio_engine_mock.set_pad_loop_region.side_effect = error_type(failure)

    ctx.ui.waveform.set_loop_start_and_play_selected_pad(0.25, received_at_ns=17)

    assert ctx.audio.pads.residency_status(0)[1] == failure
    assert _loop_intent(controller) == before_loop
    audio_engine_mock.capture_pad_start.assert_not_called()
    _assert_no_playback_mutation(controller, audio_engine_mock)
    controller.transport.residency.poll()
    assert audio_engine_mock.set_pad_loop_region.call_count == 1
    audio_engine_mock.set_pad_loop_region.side_effect = None

    ctx.ui.waveform.set_loop_start_and_play_selected_pad(0.5, received_at_ns=29)

    assert controller.project.pad_loop_start_s[0] == 0.5
    audio_engine_mock.capture_pad_start.assert_called_once()
    audio_engine_mock.publish_pad_start.assert_called_once()
    assert ctx.audio.pads.residency_status(0) == (None, None)
