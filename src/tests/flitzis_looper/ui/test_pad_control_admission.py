"""Real UI control boundaries with explicit Native refusal and feedback doubles."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import performance_view
from tests.flitzis_looper.controller.test_app_pair_feedback import _feedback
from tests.flitzis_looper.controller.transport.test_residency import resident
from tests.flitzis_looper.ui.test_context import _record_original_starts

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller import AppController


REFUSAL = "content placement publication is pending"
OTHER_ERROR = "independent pad feedback"


def _context(controller: AppController, audio: Mock, *, paused: bool = True) -> UiContext:
    for sample_id in (0, 2):
        controller.project.sample_paths[sample_id] = f"samples/synthetic-{sample_id}.wav"
        controller.project.sample_durations[sample_id] = 30.0
        controller.project.pad_loop_auto[sample_id] = False
        controller.project.pad_loop_start_s[sample_id] = 2.0
        controller.project.pad_loop_end_s[sample_id] = 5.0
    controller.session.active_sample_ids = {0, 2}
    controller.session.paused_sample_ids = {0, 2} if paused else {2}
    controller.session.pad_playhead_s[0] = 4.2
    controller.session.pad_playhead_s[2] = 9.0
    controller.session.loading_sample_ids.add(1)
    controller.session.waveform_editor_open = True
    controller.session.waveform_editor_pad_id = 0
    controller.session.global_stop_restore_sample_ids = {0, 2}
    controller.transport.residency.report_launch_error(2, OTHER_ERROR)
    audio.loaded_residency.return_value = None
    audio.poll_loader_events.return_value = None
    audio.reset_mock()
    return UiContext(controller)


def _actions(ctx: UiContext) -> dict[str, Callable[[], None]]:
    return {
        "pad_stop": lambda: ctx.audio.pads.stop_pad(0),
        "waveform_stop": ctx.ui.waveform.stop_selected_pad_on_press,
        "waveform_stop_reset": ctx.ui.waveform.stop_and_reset_selected_pad_on_press,
        "waveform_seek": lambda: ctx.ui.waveform.seek_selected_pad_to_position(3.5),
        "waveform_pause": ctx.ui.waveform.pause_selected_pad_on_press,
        "waveform_resume": ctx.ui.waveform.pause_selected_pad_on_press,
    }


def _assert_no_launch_fallback(audio: Mock) -> None:
    audio.stop_all.assert_not_called()
    audio.start_global_playback_batch.assert_not_called()
    audio.stop_global_playback_batch.assert_not_called()
    audio.capture_pad_start.assert_not_called()
    audio.publish_pad_start.assert_not_called()
    audio.play_sample.assert_not_called()
    audio.play_sample_exclusive.assert_not_called()


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
@pytest.mark.parametrize(
    ("action", "native_method"),
    [
        ("pad_stop", "cancel_pad_launches"),
        ("pad_stop", "stop_sample"),
        ("waveform_stop", "cancel_pad_launches"),
        ("waveform_stop", "stop_sample"),
        ("waveform_stop_reset", "cancel_pad_launches"),
        ("waveform_stop_reset", "stop_sample"),
        ("waveform_seek", "seek_sample"),
        ("waveform_pause", "pause_sample"),
        ("waveform_resume", "resume_sample"),
    ],
)
def test_pad_control_refusal_is_visible_without_state_mutation_or_background_replay(
    controller: AppController,
    audio_engine_mock: Mock,
    error_type: type[Exception],
    action: str,
    native_method: str,
) -> None:
    ctx = _context(controller, audio_engine_mock, paused=action != "waveform_pause")
    before_project = controller.project.model_dump()
    before_session = controller.session.model_dump()
    failure = getattr(audio_engine_mock, native_method)
    original_side_effect = failure.side_effect
    failure.side_effect = error_type(REFUSAL)
    execute = _actions(ctx)[action]

    execute()

    failure.assert_called_once()
    assert ctx.audio.pads.residency_status(0) == (None, REFUSAL)
    assert ctx.audio.pads.residency_status(2) == (None, OTHER_ERROR)
    assert controller.project.model_dump() == before_project
    assert controller.session.model_dump() == before_session
    _assert_no_launch_fallback(audio_engine_mock)

    ctx.audio.poll.poll()
    controller.transport.on_frame_render()

    failure.assert_called_once()
    assert ctx.audio.pads.residency_status(0) == (None, REFUSAL)
    assert controller.project.model_dump() == before_project
    assert controller.session.model_dump() == before_session
    failure.side_effect = original_side_effect

    execute()

    assert failure.call_count == 2
    assert ctx.audio.pads.residency_status(0) == (None, None)
    assert ctx.audio.pads.residency_status(2) == (None, OTHER_ERROR)
    assert controller.project.model_dump() == before_project
    if action in {"waveform_pause", "waveform_resume"}:
        assert controller.session.paused_sample_ids == before_session["paused_sample_ids"]
    assert 2 in controller.session.active_sample_ids
    assert 2 in controller.session.paused_sample_ids
    assert controller.session.pad_playhead_s[2] == 9.0
    _assert_no_launch_fallback(audio_engine_mock)


def test_pad_right_hold_retains_repeated_stop_gestures_after_one_refusal(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, audio_engine_mock)
    monkeypatch.setattr(imgui, "is_mouse_clicked", lambda _button: False)
    monkeypatch.setattr(imgui, "is_mouse_down", lambda button: button == imgui.MouseButton_.right)
    audio_engine_mock.cancel_pad_launches.side_effect = RuntimeError(REFUSAL)

    performance_view._pad_button_input(ctx, 0, is_loaded=True)

    audio_engine_mock.cancel_pad_launches.assert_called_once_with(0)
    audio_engine_mock.stop_sample.assert_not_called()
    assert ctx.audio.pads.residency_status(0) == (None, REFUSAL)
    audio_engine_mock.cancel_pad_launches.side_effect = None

    performance_view._pad_button_input(ctx, 0, is_loaded=True)

    assert audio_engine_mock.cancel_pad_launches.call_count == 2
    audio_engine_mock.stop_sample.assert_called_once_with(0)
    assert ctx.audio.pads.residency_status(0) == (None, None)
    assert ctx.audio.pads.residency_status(2) == (None, OTHER_ERROR)
    _assert_no_launch_fallback(audio_engine_mock)


@pytest.mark.parametrize("terminal", ["rejected", "accepted"])
def test_waveform_restart_preserves_pause_and_position_until_actual_audio_feedback(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    terminal: str,
) -> None:
    ctx = _context(controller, audio_engine_mock)
    ticket = resident(controller, audio_engine_mock)
    originals = _record_original_starts(audio_engine_mock)
    before_project = controller.project.model_dump()

    ctx.ui.waveform.play_restart_selected_pad_on_press(received_at_ns=17)

    assert controller.session.active_sample_ids == {0, 2}
    assert controller.session.paused_sample_ids == {0, 2}
    assert controller.session.pad_playhead_s[0] == 4.2
    assert controller.session.pad_playhead_s[2] == 9.0
    assert controller.project.model_dump() == before_project
    (original,) = originals
    assert original.received_at_ns == 17
    assert original.loop_region == (3.0, 4.0)
    audio_engine_mock.prepare_captured_pad_start.assert_called_once_with(original)
    audio_engine_mock.publish_pad_start.assert_not_called()
    controller.transport.residency.poll()
    assert controller.session.paused_sample_ids == {0, 2}
    assert controller.session.pad_playhead_s[0] == 4.2
    ticket.status = terminal
    ticket.message = REFUSAL if terminal == "rejected" else None

    controller.transport.residency.poll()

    assert controller.session.paused_sample_ids == {0, 2}
    assert controller.session.pad_playhead_s[0] == 4.2
    audio_engine_mock.capture_pad_start.assert_called_once()
    if terminal == "rejected":
        audio_engine_mock.publish_pad_start.assert_not_called()
        assert ctx.audio.pads.residency_status(0) == (None, REFUSAL)
    else:
        audio_engine_mock.publish_pad_start.assert_called_once_with(original, ticket)
        resolve = Mock(return_value=0)
        monkeypatch.setattr(audio_engine_mock, "_resolve_pad_feedback", resolve, raising=False)
        audio_engine_mock.receive_msg.side_effect = [_feedback("started"), None]

        controller._poll_audio_messages()

        assert controller.session.paused_sample_ids == {2}
        assert controller.session.pad_playhead_s[0] == 4.2
        audio_engine_mock.receive_msg.side_effect = [_feedback("playhead"), None]

        controller._poll_audio_messages()

        assert controller.session.pad_playhead_s[0] == 3.25
        assert controller.session.pad_playhead_s[2] == 9.0
        assert resolve.call_count == 2
    audio_engine_mock.play_sample.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    audio_engine_mock.play_resident_control.assert_not_called()
    audio_engine_mock.stop_all.assert_not_called()
    assert ctx.audio.pads.residency_status(2) == (None, OTHER_ERROR)
