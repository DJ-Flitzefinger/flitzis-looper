"""Expected Native refusal consumes one mapped gesture without aborting later input."""

from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.input_mapping import KeyboardBinding, LooperAction
from flitzis_looper.models import BeatGrid, SampleAnalysis
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.conftest import (
    FakeCapturedGlobalStart,
    FakeGlobalPlaybackBatchTicket,
    FakeInputRuntimePadBinding,
    current_timing_metadata,
)

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


PENDING_PLACEMENT = "content placement publication is pending"


def _context(controller: AppController, audio: Mock, action: str) -> UiContext:
    """Provide explicit current source ownership, with a different pad still loading."""
    metadata = {sample_id: current_timing_metadata(sample_id=sample_id) for sample_id in (0, 2)}
    bindings = {
        sample_id: FakeInputRuntimePadBinding(sample_id, accepted_timing=value, intent="automatic")
        for sample_id, value in metadata.items()
    }
    for sample_id in bindings:
        controller.project.sample_paths[sample_id] = f"samples/synthetic-{sample_id}.wav"
        controller.project.sample_durations[sample_id] = 60.0
        controller.project.sample_analysis[sample_id] = SampleAnalysis(
            bpm=120.0,
            key="C",
            beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
        )
        controller.project.pad_loop_auto[sample_id] = False
        controller.project.pad_loop_start_s[sample_id] = 0.25
        controller.project.pad_loop_end_s[sample_id] = 2.25
    controller.session.loading_sample_ids.add(1)
    controller.session.active_sample_ids = {0, 2}
    controller.session.paused_sample_ids = {2}
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 2}
    controller.session.pad_playhead_s[0] = 1.5
    controller.input_mapping.save_mapping(
        "keyboard", KeyboardBinding(key_name="K").key, LooperAction.from_key(action)
    )
    audio.current_constant_timing.side_effect = metadata.get
    audio.current_input_runtime_pad_binding.side_effect = bindings.get
    audio.pad_timing_intent.return_value = "automatic"
    audio.loaded_residency.return_value = None
    audio.output_sample_rate.return_value = 48_000
    audio.reset_mock()
    return UiContext(controller)


def _session_without_mapping_error(controller: AppController) -> dict[str, object]:
    state = controller.session.model_dump()
    state.pop("input_mapping_error")
    return state


def _assert_no_fallback(audio: Mock) -> None:
    audio.start_global_playback_batch.assert_not_called()
    audio.stop_all.assert_not_called()
    audio.play_sample.assert_not_called()
    audio.play_sample_exclusive.assert_not_called()
    audio.trigger_input_runtime_pad.assert_not_called()
    audio.capture_pad_start.assert_not_called()
    audio.publish_pad_start.assert_not_called()
    audio.set_pad_loop_region.assert_not_called()


def _assert_no_admitted_stop_or_start(audio: Mock) -> None:
    audio.stop_global_playback_batch.assert_not_called()
    audio.publish_captured_global_start.assert_not_called()
    audio.stop_sample.assert_not_called()


@pytest.mark.parametrize("action", ["global.stop_all", "global.start_stop", "pad.stop:0"])
@pytest.mark.parametrize("error_type", [RuntimeError, ValueError, TypeError])
def test_keyboard_native_refusal_is_visible_without_state_mutation_or_background_replay(
    controller: AppController,
    audio_engine_mock: Mock,
    action: str,
    error_type: type[Exception],
) -> None:
    ctx = _context(controller, audio_engine_mock, action)
    before_session = _session_without_mapping_error(controller)
    before_project = controller.project.model_dump()
    operations = {
        "global.stop_all": audio_engine_mock.cancel_all_launches,
        "global.start_stop": audio_engine_mock.capture_global_start,
        "pad.stop:0": audio_engine_mock.cancel_pad_launches,
    }
    operation = operations[action]
    original_operation = operation.side_effect
    operation.side_effect = error_type(PENDING_PLACEMENT)

    ctx.input.capture_keyboard("K", received_at_ns=17)

    assert controller.session.input_mapping_error == PENDING_PLACEMENT
    assert _session_without_mapping_error(controller) == before_session
    assert controller.project.model_dump() == before_project
    operation.assert_called_once()
    _assert_no_admitted_stop_or_start(audio_engine_mock)
    _assert_no_fallback(audio_engine_mock)
    controller.transport.on_frame_render()
    controller.input_mapping.on_frame_render()
    controller.transport.on_frame_render()
    assert operation.call_count == 1
    assert _session_without_mapping_error(controller) == before_session
    assert controller.project.model_dump() == before_project
    assert controller.session.input_mapping_error == PENDING_PLACEMENT
    _assert_no_admitted_stop_or_start(audio_engine_mock)

    operation.side_effect = original_operation
    audio_engine_mock.stop_global_playback_batch.return_value = FakeGlobalPlaybackBatchTicket(
        "pending"
    )

    assert ctx.input.capture_keyboard("K", received_at_ns=29)

    assert controller.session.input_mapping_error is None
    assert operation.call_count == 2
    assert controller.session.active_sample_ids == {0, 2}
    assert controller.session.paused_sample_ids == {2}
    assert controller.project.model_dump() == before_project
    if action == "global.start_stop":
        assert audio_engine_mock.capture_global_start.call_args.kwargs == {"received_at_ns": 29}
        original = audio_engine_mock.publish_captured_global_start.call_args.args[0]
        assert isinstance(original, FakeCapturedGlobalStart)
        assert original.received_at_ns == 29
        assert original.entries == tuple(audio_engine_mock.capture_global_start.call_args.args[0])
        audio_engine_mock.publish_captured_global_start.assert_called_once_with(original)
        audio_engine_mock.cancel_all_launches.assert_not_called()
        audio_engine_mock.cancel_pad_launches.assert_not_called()
    elif action == "global.stop_all":
        assert audio_engine_mock.stop_global_playback_batch.call_args.kwargs == {
            "received_at_ns": 29
        }
        audio_engine_mock.stop_global_playback_batch.assert_called_once()
        audio_engine_mock.capture_global_start.assert_not_called()
        audio_engine_mock.cancel_pad_launches.assert_not_called()
    else:
        audio_engine_mock.cancel_pad_launches.assert_called_with(0)
        audio_engine_mock.stop_sample.assert_called_once_with(0)
        audio_engine_mock.cancel_all_launches.assert_not_called()
        audio_engine_mock.capture_global_start.assert_not_called()
    _assert_no_fallback(audio_engine_mock)


def _midi_event(action: str, timestamp: int) -> dict[str, object]:
    return {
        "source": "midi",
        "binding_key": "midi:note:1:63",
        "action_key": action,
        "received_at_ns": timestamp,
        "direct": False,
        "dispatched": False,
    }


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError, TypeError])
def test_failed_midi_fallback_stop_does_not_drop_later_independent_event(
    controller: AppController, audio_engine_mock: Mock, error_type: type[Exception]
) -> None:
    _context(controller, audio_engine_mock, "pad.stop:0")
    before_session = _session_without_mapping_error(controller)
    before_project = controller.project.model_dump()
    audio_engine_mock.cancel_pad_launches.side_effect = error_type(PENDING_PLACEMENT)
    audio_engine_mock.poll_input_events.side_effect = [
        _midi_event("pad.stop:0", 17),
        _midi_event("ui.select_bank:1", 23),
        None,
    ]

    controller.input_mapping.on_frame_render()

    assert controller.project.selected_bank == 1
    assert controller.session.input_mapping_error == PENDING_PLACEMENT
    assert _session_without_mapping_error(controller) == before_session
    before_project["selected_bank"] = 1
    assert controller.project.model_dump() == before_project
    audio_engine_mock.cancel_pad_launches.assert_called_once_with(0)
    audio_engine_mock.stop_sample.assert_not_called()
    audio_engine_mock.capture_global_start.assert_not_called()
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    _assert_no_fallback(audio_engine_mock)
    controller.transport.on_frame_render()
    assert audio_engine_mock.cancel_pad_launches.call_count == 1
    audio_engine_mock.cancel_pad_launches.side_effect = None
    audio_engine_mock.poll_input_events.side_effect = [
        _midi_event("pad.stop:0", 29),
        None,
    ]

    controller.input_mapping.on_frame_render()

    assert controller.session.input_mapping_error is None
    assert audio_engine_mock.cancel_pad_launches.call_count == 2
    audio_engine_mock.stop_sample.assert_called_once_with(0)
    assert controller.session.active_sample_ids == {0, 2}
    assert controller.session.paused_sample_ids == {2}
    assert controller.project.model_dump() == before_project
    _assert_no_fallback(audio_engine_mock)


@pytest.mark.parametrize("timestamp", [-1, True, 1.5, "17", 1 << 64])
def test_invalid_public_keyboard_timestamp_still_raises_before_admission(
    controller: AppController, audio_engine_mock: Mock, timestamp: object
) -> None:
    ctx = _context(controller, audio_engine_mock, "global.stop_all")

    with pytest.raises(ValueError, match="received_at_ns"):
        ctx.input.capture_keyboard("K", received_at_ns=cast("int", timestamp))

    assert controller.session.input_mapping_error is None
    audio_engine_mock.cancel_all_launches.assert_not_called()
    audio_engine_mock.stop_global_playback_batch.assert_not_called()
    _assert_no_fallback(audio_engine_mock)


def test_successful_keyboard_action_preserves_unrelated_mapping_feedback(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ctx = _context(controller, audio_engine_mock, "ui.select_bank:1")
    controller.session.input_mapping_error = "MIDI port unavailable"

    assert ctx.input.capture_keyboard("K", received_at_ns=17)

    assert controller.project.selected_bank == 1
    assert controller.session.input_mapping_error == "MIDI port unavailable"
    _assert_no_fallback(audio_engine_mock)


def test_successful_same_keyboard_action_preserves_feedback_replaced_by_another_owner(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ctx = _context(controller, audio_engine_mock, "pad.stop:0")
    audio_engine_mock.cancel_pad_launches.side_effect = RuntimeError(PENDING_PLACEMENT)
    assert ctx.input.capture_keyboard("K", received_at_ns=17)
    assert controller.session.input_mapping_error == PENDING_PLACEMENT
    controller.session.input_mapping_error = "MIDI port unavailable"
    audio_engine_mock.cancel_pad_launches.side_effect = None

    assert ctx.input.capture_keyboard("K", received_at_ns=29)

    assert controller.session.input_mapping_error == "MIDI port unavailable"
    audio_engine_mock.stop_sample.assert_called_once_with(0)
    _assert_no_fallback(audio_engine_mock)


def test_direct_controller_execution_keeps_native_exception_visible_to_caller(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _context(controller, audio_engine_mock, "global.stop_all")
    audio_engine_mock.cancel_all_launches.side_effect = RuntimeError(PENDING_PLACEMENT)

    with pytest.raises(RuntimeError, match=PENDING_PLACEMENT):
        controller.input_mapping.execute_action(
            LooperAction.from_key("global.stop_all"), received_at_ns=17
        )

    assert controller.session.input_mapping_error is None
    audio_engine_mock.stop_global_playback_batch.assert_not_called()
    _assert_no_fallback(audio_engine_mock)


def test_malformed_midi_fallback_timestamp_never_recaptures_as_a_fresh_gesture(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _context(controller, audio_engine_mock, "pad.stop:0")
    before_session = _session_without_mapping_error(controller)
    before_project = controller.project.model_dump()
    audio_engine_mock.poll_input_events.side_effect = [_midi_event("pad.stop:0", -1), None]

    controller.input_mapping.on_frame_render()

    assert "received_at_ns" in (controller.session.input_mapping_error or "")
    assert _session_without_mapping_error(controller) == before_session
    assert controller.project.model_dump() == before_project
    audio_engine_mock.cancel_pad_launches.assert_not_called()
    audio_engine_mock.stop_sample.assert_not_called()
    _assert_no_fallback(audio_engine_mock)


def test_relative_midi_feedback_survives_handled_events_until_a_controller_edit_succeeds(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _context(controller, audio_engine_mock, "global.volume.delta")
    controller.project.volume = 0.5
    before_project = controller.project.model_dump()

    def event(value: int | None, binding: str = "midi:cc:1:7") -> dict[str, object]:
        return {
            **_midi_event("global.volume.delta", 17),
            "binding_key": binding,
            "value": value,
        }

    audio_engine_mock.set_volume.side_effect = RuntimeError(PENDING_PLACEMENT)
    audio_engine_mock.poll_input_events.side_effect = [event(10), event(11), None]
    controller.input_mapping.on_frame_render()
    audio_engine_mock.set_volume.assert_called_once()
    assert controller.session.input_mapping_error == PENDING_PLACEMENT
    assert controller.project.model_dump() == before_project
    audio_engine_mock.set_volume.side_effect = None

    for handled_without_edit in (
        event(None),
        event(12, "midi:note:1:63"),
        event(64, "midi:cc:1:8"),
        event(64, "midi:cc:1:8"),
    ):
        audio_engine_mock.poll_input_events.side_effect = [handled_without_edit, None]

        controller.input_mapping.on_frame_render()

        assert audio_engine_mock.set_volume.call_count == 1
        assert controller.session.input_mapping_error == PENDING_PLACEMENT
        assert controller.project.model_dump() == before_project

    audio_engine_mock.poll_input_events.side_effect = [event(12), None]

    controller.input_mapping.on_frame_render()

    assert audio_engine_mock.set_volume.call_count == 2
    assert controller.session.input_mapping_error is None
    assert controller.project.volume > before_project["volume"]
    _assert_no_fallback(audio_engine_mock)
