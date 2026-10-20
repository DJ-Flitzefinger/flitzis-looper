"""A direct Native STOP receipt never authorizes another Python capture."""

from typing import TYPE_CHECKING

import pytest

from tests.flitzis_looper.input_mapping.test_admission_failures import (
    _assert_no_admitted_stop_or_start,
    _assert_no_fallback,
    _context,
    _midi_event,
    _session_without_mapping_error,
)

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


@pytest.mark.parametrize("replacement", [False, True])
def test_refused_direct_stop_is_visible_without_recapture_or_replay(
    controller: AppController, audio_engine_mock: Mock, *, replacement: bool
) -> None:
    ctx = _context(controller, audio_engine_mock, "pad.stop:0")
    if replacement:
        controller.project.sample_paths[0] = "samples/replacement.wav"
    before_session = _session_without_mapping_error(controller)
    before_project = controller.project.model_dump()
    event = {**_midi_event("pad.stop:0", 17), "direct": True}
    audio_engine_mock.poll_input_events.side_effect = [
        event,
        _midi_event("ui.select_bank:1", 23),
        None,
    ]

    controller.input_mapping.on_frame_render()

    assert "MIDI stop" in (controller.session.input_mapping_error or "")
    assert _session_without_mapping_error(controller) == before_session
    before_project["selected_bank"] = 1
    assert controller.project.model_dump() == before_project
    audio_engine_mock.cancel_pad_launches.assert_not_called()
    _assert_no_admitted_stop_or_start(audio_engine_mock)
    _assert_no_fallback(audio_engine_mock)
    audio_engine_mock.poll_input_events.side_effect = [None]
    controller.input_mapping.on_frame_render()
    controller.transport.on_frame_render()
    audio_engine_mock.cancel_pad_launches.assert_not_called()

    # A new explicit keyboard gesture may act normally and clear only this error.
    assert ctx.input.capture_keyboard("K", received_at_ns=29)
    assert controller.session.input_mapping_error is None
    audio_engine_mock.cancel_pad_launches.assert_called_once_with(0)
    audio_engine_mock.stop_sample.assert_called_once_with(0)


def test_admitted_direct_stop_receipt_never_sends_a_second_stop(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _context(controller, audio_engine_mock, "pad.stop:0")
    controller.session.input_mapping_error = "unrelated MIDI port error"
    before_session = _session_without_mapping_error(controller)
    audio_engine_mock.poll_input_events.side_effect = [
        {**_midi_event("pad.stop:0", 17), "direct": True, "dispatched": True},
        None,
    ]

    controller.input_mapping.on_frame_render()

    assert controller.session.input_mapping_error == "unrelated MIDI port error"
    assert _session_without_mapping_error(controller) == before_session
    audio_engine_mock.cancel_pad_launches.assert_not_called()
    _assert_no_admitted_stop_or_start(audio_engine_mock)
    _assert_no_fallback(audio_engine_mock)
