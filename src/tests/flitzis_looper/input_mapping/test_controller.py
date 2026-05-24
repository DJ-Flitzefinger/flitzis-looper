from typing import TYPE_CHECKING

from flitzis_looper.input_mapping.actions import LooperAction
from flitzis_looper.input_mapping.bindings import KeyboardBinding
from flitzis_looper.input_mapping.storage import (
    KEYBOARD_MAPPING_PATH,
    MIDI_MAPPING_PATH,
    load_keyboard_mapping_file,
    load_midi_mapping_file,
)
from flitzis_looper.ui.context import UiContext

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def test_learn_saves_midi_mapping_and_refreshes_rust_snapshot(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)

    ctx.input.toggle_learn()
    audio_engine_mock.poll_input_events.side_effect = [
        {
            "source": "midi",
            "binding_key": "midi:note:1:60",
            "received_at_ns": 10,
            "direct": False,
            "dispatched": False,
        },
        None,
    ]
    controller.input_mapping.on_frame_render()

    ctx.audio.pads.trigger_pad(0)

    data = load_midi_mapping_file()
    assert len(data.mappings) == 1
    assert data.mappings[0].input.key == "midi:note:1:60"
    assert data.mappings[0].action.key == "pad.trigger:0"
    audio_engine_mock.set_input_mapping_snapshot.assert_called_with(
        [("midi:note:1:60", "pad.trigger:0")]
    )
    assert controller.session.input_learn_active is False


def test_learn_input_then_l_deletes_existing_midi_mapping(
    controller: AppController,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.input_mapping.save_mapping(
        "midi",
        "midi:cc:1:7",
        LooperAction.trigger_pad(0),
    )

    controller.input_mapping.toggle_learn()
    controller.input_mapping._handle_rust_input_event(
        {"source": "midi", "binding_key": "midi:cc:1:7"}
    )
    controller.input_mapping.toggle_learn()

    assert load_midi_mapping_file().mappings == []
    assert MIDI_MAPPING_PATH.is_file()


def test_keyboard_capture_is_suppressed_while_typing(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="A")
    controller.input_mapping.save_mapping("keyboard", binding.key, LooperAction.trigger_pad(0))
    controller.project.sample_paths[0] = "samples/foo.wav"

    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=True,
    )

    assert handled is False
    audio_engine_mock.play_sample_exclusive.assert_not_called()


def test_keyboard_learn_capture_does_not_execute_existing_mapping(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="A")
    controller.input_mapping.save_mapping("keyboard", binding.key, LooperAction.trigger_pad(0))
    controller.project.sample_paths[0] = "samples/foo.wav"

    controller.input_mapping.toggle_learn()
    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    assert controller.session.input_learn_pending_binding_key == binding.key
    audio_engine_mock.play_sample_exclusive.assert_not_called()


def test_keyboard_mapping_executes_action_when_not_typing(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="A", ctrl=True)
    controller.input_mapping.save_mapping("keyboard", binding.key, LooperAction.trigger_pad(0))
    controller.project.sample_paths[0] = "samples/foo.wav"

    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    audio_engine_mock.play_sample_exclusive.assert_called_once_with(0, 1.0)


def test_non_direct_rust_midi_event_executes_python_action(
    controller: AppController,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.selected_bank = 0

    controller.input_mapping._handle_rust_input_event(
        {
            "source": "midi",
            "binding_key": "midi:cc:1:7",
            "action_key": "ui.select_bank:2",
            "direct": False,
            "dispatched": True,
        }
    )

    assert controller.project.selected_bank == 2


def test_rust_midi_event_is_ignored_when_mapping_disabled(
    controller: AppController,
) -> None:
    controller.project.selected_bank = 0

    controller.input_mapping._handle_rust_input_event(
        {
            "source": "midi",
            "binding_key": "midi:cc:1:7",
            "action_key": "ui.select_bank:2",
            "direct": False,
            "dispatched": True,
        }
    )

    assert controller.project.selected_bank == 0


def test_direct_rust_midi_event_is_not_executed_twice(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.sample_paths[0] = "samples/foo.wav"

    controller.input_mapping._handle_rust_input_event(
        {
            "source": "midi",
            "binding_key": "midi:note:1:60",
            "action_key": "pad.trigger:0",
            "direct": True,
            "dispatched": True,
        }
    )

    audio_engine_mock.play_sample_exclusive.assert_not_called()


def test_settings_delete_all_mapping_actions(controller: AppController) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.save_mapping(
        "midi",
        "midi:note:1:60",
        LooperAction.trigger_pad(0),
    )
    controller.input_mapping.save_mapping(
        "keyboard",
        KeyboardBinding(key_name="B").key,
        LooperAction.trigger_pad(0),
    )

    ctx.ui.settings.delete_all_midi_mappings()
    ctx.ui.settings.delete_all_keyboard_mappings()

    assert load_midi_mapping_file().mappings == []
    assert load_keyboard_mapping_file().mappings == []
    assert MIDI_MAPPING_PATH.is_file()
    assert KEYBOARD_MAPPING_PATH.is_file()
