"""Controller-owned MIDI global actions reach the same current native batch."""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.input_mapping import KeyboardBinding, LooperAction
from flitzis_looper_audio import AudioMessage
from tests.flitzis_looper.conftest import (
    FakeGlobalPlaybackBatchTicket,
    FakeInputRuntimePadBinding,
    current_timing_metadata,
)

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def _prepare_midi_batch(
    controller: AppController, audio: Mock
) -> tuple[dict[int, dict[str, object]], dict[int, FakeInputRuntimePadBinding]]:
    controller.input_mapping.set_enabled(enabled=True)
    metadata = {
        sample_id: current_timing_metadata(
            sample_id=sample_id, period=0.2245048325556449, origin=-0.125
        )
        for sample_id in (0, 2)
    }
    bindings = {
        sample_id: FakeInputRuntimePadBinding(sample_id, accepted_timing=value, intent="automatic")
        for sample_id, value in metadata.items()
    }
    audio.current_constant_timing.side_effect = metadata.get
    audio.current_input_runtime_pad_binding.side_effect = bindings.get
    audio.pad_timing_intent.side_effect = lambda _sample_id: "automatic"
    controller.session.active_sample_ids = {0, 2}
    controller.project.pad_loop_start_s[0] = 1.0
    controller.project.pad_loop_end_s[0] = 2.0
    controller.project.pad_loop_start_s[2] = 3.0
    controller.project.pad_loop_end_s[2] = 4.0
    audio.reset_mock()
    return metadata, bindings


def _event(action: str, timestamp: object, *, dispatched: bool = True) -> dict[str, object]:
    return {
        "source": "midi",
        "binding_key": "midi:note:1:63",
        "action_key": action,
        "received_at_ns": timestamp,
        "direct": False,
        "dispatched": dispatched,
    }


def _assert_no_fallback(audio: Mock) -> None:
    audio.set_pad_loop_region.assert_not_called()
    audio.play_sample.assert_not_called()
    audio.play_sample_exclusive.assert_not_called()
    audio.trigger_input_runtime_pad.assert_not_called()
    audio.stop_all.assert_not_called()


def _dispatch_global_action(
    controller: AppController, action: str, *, route: str, timestamp: int
) -> None:
    if route == "midi":
        controller.input_mapping._handle_rust_input_event(_event(action, timestamp))
    else:
        binding = KeyboardBinding(key_name="B")
        controller.input_mapping.save_mapping(
            "keyboard", binding.key, LooperAction.from_key(action)
        )
        assert controller.input_mapping.capture_keyboard_input(
            binding, text_input_focused=False, received_at_ns=timestamp
        )


@pytest.mark.parametrize("route", ["midi", "keyboard"])
@pytest.mark.parametrize("remember_stop", [False, True])
def test_actual_app_global_input_drains_accepted_playback_feedback_before_next_targets(
    controller: AppController, audio_engine_mock: Mock, route: str, *, remember_stop: bool
) -> None:
    _, bindings = _prepare_midi_batch(controller, audio_engine_mock)
    controller.session.active_sample_ids.clear()
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 2}
    start_ticket = FakeGlobalPlaybackBatchTicket("pending")
    stop_ticket = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.start_global_playback_batch.return_value = start_ticket
    audio_engine_mock.stop_global_playback_batch.return_value = stop_ticket
    _dispatch_global_action(controller, "global.start_stop", route=route, timestamp=17)
    assert controller.session.active_sample_ids == set()
    assert controller.session.global_stop_restore_sample_ids == {0, 2}

    start_ticket.status = "accepted"
    audio_engine_mock.receive_msg.side_effect = [
        AudioMessage.SampleStarted(0),
        AudioMessage.SampleStarted(2),
        None,
    ]
    if remember_stop:
        controller.transport.playback.stop_global_start_stop()
    else:
        _dispatch_global_action(controller, "global.stop_all", route=route, timestamp=19)

    audio_engine_mock.stop_global_playback_batch.assert_called_once_with(
        [bindings[0], bindings[2]], received_at_ns=None if remember_stop else 19
    )
    assert controller.session.active_sample_ids == {0, 2}
    assert controller.session.global_stop_engaged is False
    assert controller.session.global_stop_restore_sample_ids == set()
    stop_ticket.status = "accepted"
    audio_engine_mock.receive_msg.side_effect = [
        AudioMessage.SampleStopped(0),
        AudioMessage.SampleStopped(2),
        None,
    ]

    _dispatch_global_action(controller, "global.start_stop", route=route, timestamp=23)

    assert controller.session.active_sample_ids == set()
    assert controller.session.paused_sample_ids == set()
    if remember_stop:
        assert audio_engine_mock.start_global_playback_batch.call_count == 2
        assert audio_engine_mock.start_global_playback_batch.call_args.args[0] == [
            (bindings[0], 1.0, 2.0),
            (bindings[2], 3.0, 4.0),
        ]
        assert audio_engine_mock.start_global_playback_batch.call_args.kwargs == {
            "received_at_ns": 23
        }
        assert controller.session.global_stop_restore_sample_ids == {0, 2}
    else:
        audio_engine_mock.start_global_playback_batch.assert_called_once()
        assert controller.session.global_stop_restore_sample_ids == set()
    _assert_no_fallback(audio_engine_mock)


@pytest.mark.parametrize("action", ["global.start_stop", "global.stop_all"])
@pytest.mark.parametrize("timestamp", [None, 0, 123_456_789, (1 << 64) - 1])
@pytest.mark.parametrize("dispatched", [False, True])
def test_productive_midi_global_action_captures_all_native_sources_with_original_time(
    controller: AppController,
    audio_engine_mock: Mock,
    action: str,
    timestamp: int | None,
    *,
    dispatched: bool,
) -> None:
    _, bindings = _prepare_midi_batch(controller, audio_engine_mock)
    before = controller.session.model_dump()

    controller.input_mapping._handle_rust_input_event(
        _event(action, timestamp, dispatched=dispatched)
    )

    if action == "global.start_stop":
        audio_engine_mock.start_global_playback_batch.assert_called_once_with(
            [(bindings[0], 1.0, 2.0), (bindings[2], 3.0, 4.0)], received_at_ns=timestamp
        )
        audio_engine_mock.stop_global_playback_batch.assert_not_called()
    else:
        audio_engine_mock.stop_global_playback_batch.assert_called_once_with(
            [bindings[0], bindings[2]], received_at_ns=timestamp
        )
        audio_engine_mock.start_global_playback_batch.assert_not_called()
    assert controller.session.model_dump() == before
    _assert_no_fallback(audio_engine_mock)


@pytest.mark.parametrize("action", ["global.start_stop", "global.stop_all"])
@pytest.mark.parametrize("failure", ["unavailable", "stale", "queue_full"])
def test_productive_midi_global_failure_preserves_restore_and_all_voice_state(
    controller: AppController, audio_engine_mock: Mock, action: str, failure: str
) -> None:
    metadata, bindings = _prepare_midi_batch(controller, audio_engine_mock)
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 2}
    controller.session.paused_sample_ids = {2}
    if failure == "unavailable":
        metadata.pop(2)
    elif failure == "stale":
        bindings[2] = FakeInputRuntimePadBinding(
            2,
            accepted_timing=dict(metadata[2], revision="same-period-new-revision"),
            intent="automatic",
        )
    else:
        audio_engine_mock.start_global_playback_batch.side_effect = RuntimeError(
            "full command ring"
        )
        audio_engine_mock.stop_global_playback_batch.side_effect = RuntimeError("full command ring")
    before = controller.session.model_dump()
    project_before = controller.project.model_dump()

    controller.input_mapping._handle_rust_input_event(_event(action, 41))

    after = controller.session.model_dump()
    if failure == "queue_full":
        assert after.pop("input_mapping_error") == "full command ring"
        before.pop("input_mapping_error")
    else:
        audio_engine_mock.start_global_playback_batch.assert_not_called()
        audio_engine_mock.stop_global_playback_batch.assert_not_called()
    assert after == before
    assert controller.project.model_dump() == project_before
    _assert_no_fallback(audio_engine_mock)


@pytest.mark.parametrize("action", ["global.start_stop", "global.stop_all"])
@pytest.mark.parametrize("timestamp", [-1, True, 1.5, "12", 1 << 64])
def test_invalid_midi_global_time_reports_error_without_unguarded_retry(
    controller: AppController, audio_engine_mock: Mock, action: str, timestamp: object
) -> None:
    _prepare_midi_batch(controller, audio_engine_mock)

    controller.input_mapping._handle_rust_input_event(_event(action, timestamp))

    assert "received_at_ns" in str(controller.session.input_mapping_error)
    audio_engine_mock.current_constant_timing.assert_not_called()
    audio_engine_mock.current_input_runtime_pad_binding.assert_not_called()
    audio_engine_mock.start_global_playback_batch.assert_not_called()
    audio_engine_mock.stop_global_playback_batch.assert_not_called()
    _assert_no_fallback(audio_engine_mock)


@pytest.mark.parametrize("action", ["global.start_stop", "global.stop_all"])
def test_midi_pending_then_rejected_global_batch_retains_previous_restore(
    controller: AppController, audio_engine_mock: Mock, action: str
) -> None:
    _prepare_midi_batch(controller, audio_engine_mock)
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 2}
    ticket = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.start_global_playback_batch.return_value = ticket
    audio_engine_mock.stop_global_playback_batch.return_value = ticket
    before = controller.session.model_dump()

    controller.input_mapping._handle_rust_input_event(_event(action, 0))
    controller.transport.on_frame_render()
    assert controller.session.model_dump() == before
    ticket.status = "rejected"
    controller.transport.on_frame_render()
    assert controller.session.model_dump() == before
    _assert_no_fallback(audio_engine_mock)


@pytest.mark.parametrize("action", ["global.start_stop", "global.stop_all"])
def test_successful_direct_global_event_is_never_executed_twice(
    controller: AppController, audio_engine_mock: Mock, action: str
) -> None:
    _prepare_midi_batch(controller, audio_engine_mock)
    event = _event(action, -1)
    event["direct"] = True

    controller.input_mapping._handle_rust_input_event(event)

    assert audio_engine_mock.method_calls == []
