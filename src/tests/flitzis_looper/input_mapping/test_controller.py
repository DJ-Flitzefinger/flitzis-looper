from typing import TYPE_CHECKING

import pytest

from flitzis_looper.input_mapping.actions import (
    LooperAction,
    global_speed_action,
    global_speed_delta_action,
    master_volume_action,
    master_volume_delta_action,
    pad_eq_action,
    pad_eq_delta_action,
    pad_gain_action,
    pad_gain_delta_action,
    tap_bpm_action,
)
from flitzis_looper.input_mapping.bindings import KeyboardBinding
from flitzis_looper.input_mapping.storage import (
    KEYBOARD_MAPPING_PATH,
    MIDI_MAPPING_PATH,
    load_keyboard_mapping_file,
    load_midi_mapping_file,
)
from flitzis_looper.models import STEM_MASK_VOCALS
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
    audio_engine_mock.set_input_mapping_snapshot.assert_called_with([
        ("midi:note:1:60", "pad.trigger:0")
    ])
    assert controller.session.input_learn_active is False


def test_learn_saves_tap_bpm_mapping(controller: AppController) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
    })

    ctx.audio.pads.tap_bpm(4)

    data = load_midi_mapping_file()
    assert data.mappings[0].input.key == "midi:note:1:60"
    assert data.mappings[0].action.key == "pad.tap_bpm:4"
    assert controller.session.input_learn_active is False


def test_learn_saves_master_volume_mapping(controller: AppController) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:7",
        "value": 64,
    })

    ctx.audio.global_.set_volume(0.37)

    data = load_midi_mapping_file()
    assert data.mappings[0].input.key == "midi:cc:1:7"
    assert data.mappings[0].action.key == "global.volume.delta"
    assert controller.project.volume == 1.0


def test_learn_saves_nrpn_master_volume_mapping(controller: AppController) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:nrpn:1:0",
        "value": 65,
    })

    ctx.audio.global_.set_volume(0.37)

    data = load_midi_mapping_file()
    assert data.mappings[0].input.key == "midi:nrpn:1:0"
    assert data.mappings[0].action.key == "global.volume.delta"
    assert controller.project.volume == 1.0


def test_learn_saves_midi_note_master_volume_mapping_as_set_value(
    controller: AppController,
) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "value": 100,
    })

    ctx.audio.global_.set_volume(0.37)

    data = load_midi_mapping_file()
    assert data.mappings[0].input.key == "midi:note:1:60"
    assert data.mappings[0].action.key == "global.volume:37"
    assert controller.project.volume == 1.0


def test_learn_saves_pad_eq_band_mapping(controller: AppController) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:74",
        "value": 10,
    })

    ctx.audio.pads.set_pad_eq_band(2, "mid", -3.5)

    data = load_midi_mapping_file()
    assert data.mappings[0].input.key == "midi:cc:1:74"
    assert data.mappings[0].action.key == "pad.eq.delta:2:mid"
    assert controller.project.pad_eq_mid_db[2] == 0.0


def test_learn_saves_pad_gain_mapping(controller: AppController) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:73",
        "value": 10,
    })

    ctx.audio.pads.set_pad_gain(2, 0.4)

    data = load_midi_mapping_file()
    assert data.mappings[0].input.key == "midi:cc:1:73"
    assert data.mappings[0].action.key == "pad.gain.delta:2"
    assert controller.project.pad_gain_db[2] == 0.0


def test_learn_saves_midi_note_pad_gain_mapping_as_set_value(
    controller: AppController,
) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:61",
        "value": 100,
    })

    ctx.audio.pads.set_pad_gain(2, 3.7)

    data = load_midi_mapping_file()
    assert data.mappings[0].input.key == "midi:note:1:61"
    assert data.mappings[0].action.key == "pad.gain_db:2:37"
    assert controller.project.pad_gain_db[2] == 0.0


def test_learn_saves_global_speed_mapping(controller: AppController) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:72",
        "value": 10,
    })

    ctx.audio.global_.set_speed(1.23)

    data = load_midi_mapping_file()
    assert data.mappings[0].input.key == "midi:cc:1:72"
    assert data.mappings[0].action.key == "global.speed.delta"
    assert controller.project.speed == 1.0


def test_learn_saves_midi_note_global_speed_mapping_as_set_value(
    controller: AppController,
) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:62",
        "value": 100,
    })

    ctx.audio.global_.set_speed(1.23)

    data = load_midi_mapping_file()
    assert data.mappings[0].input.key == "midi:note:1:62"
    assert data.mappings[0].action.key == "global.speed:123"
    assert controller.project.speed == 1.0


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
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:7",
    })
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


def test_keyboard_mapping_executes_tap_bpm(controller: AppController) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="T")
    controller.input_mapping.save_mapping("keyboard", binding.key, tap_bpm_action(3))

    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    assert controller.session.tap_bpm_pad_id == 3
    assert len(controller.session.tap_bpm_timestamps) == 1


def test_keyboard_mapping_executes_master_volume(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="V")
    controller.input_mapping.save_mapping(
        "keyboard",
        binding.key,
        master_volume_action(0.42),
    )

    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    audio_engine_mock.set_volume.assert_called_once_with(0.42)
    assert controller.project.volume == 0.42


def test_keyboard_mapping_executes_pad_eq_band(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="E")
    controller.project.pad_eq_low_db[2] = 1.0
    controller.project.pad_eq_mid_db[2] = 2.0
    controller.project.pad_eq_high_db[2] = 3.0
    controller.input_mapping.save_mapping(
        "keyboard",
        binding.key,
        pad_eq_action(2, "mid", -3.5),
    )

    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    audio_engine_mock.set_pad_eq.assert_called_once_with(2, 1.0, -3.5, 3.0)
    assert controller.project.pad_eq_mid_db[2] == -3.5


def test_keyboard_mapping_executes_pad_gain(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="G")
    controller.input_mapping.save_mapping(
        "keyboard",
        binding.key,
        pad_gain_action(2, 4.2),
    )

    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    audio_engine_mock.set_pad_gain.assert_called_once_with(2, 4.2)
    assert controller.project.pad_gain_db[2] == 4.2


def test_keyboard_mapping_executes_legacy_pad_gain_as_db(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="G")
    controller.input_mapping.save_mapping(
        "keyboard",
        binding.key,
        LooperAction.from_key("pad.gain:2:50"),
    )

    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    audio_engine_mock.set_pad_gain.assert_called_once_with(2, pytest.approx(-6.0206))
    assert controller.project.pad_gain_db[2] == pytest.approx(-6.0206)


def test_keyboard_mapping_executes_global_speed(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="P")
    controller.input_mapping.save_mapping(
        "keyboard",
        binding.key,
        global_speed_action(1.23),
    )

    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    audio_engine_mock.set_speed.assert_called_once_with(1.23)
    assert controller.project.speed == 1.23


def test_midi_cc_relative_master_volume_uses_directional_steps(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.volume = 0.5
    controller.input_mapping.save_mapping(
        "midi",
        "midi:cc:1:7",
        master_volume_delta_action(),
    )

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:7",
        "value": 64,
        "action_key": "global.volume.delta",
        "direct": False,
    })
    audio_engine_mock.set_volume.assert_not_called()

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:7",
        "value": 65,
        "action_key": "global.volume.delta",
        "direct": False,
    })
    assert controller.project.volume == pytest.approx(0.51)

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:7",
        "value": 63,
        "action_key": "global.volume.delta",
        "direct": False,
    })
    assert controller.project.volume == pytest.approx(0.5)


def test_midi_cc_relative_master_volume_supports_endless_encoder_values(
    controller: AppController,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.volume = 0.5
    controller.input_mapping.save_mapping(
        "midi",
        "midi:cc:1:7",
        master_volume_delta_action(),
    )

    for value, expected in (
        (126, 0.5),
        (127, 0.51),
        (0, 0.52),
        (127, 0.51),
        (127, 0.5),
        (1, 0.51),
        (1, 0.52),
    ):
        controller.input_mapping._handle_rust_input_event({
            "source": "midi",
            "binding_key": "midi:cc:1:7",
            "value": value,
            "action_key": "global.volume.delta",
            "direct": False,
        })
        assert controller.project.volume == pytest.approx(expected)


def test_midi_cc_relative_master_volume_supports_inc_dec_encoder_values(
    controller: AppController,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.volume = 0.5
    controller.input_mapping.save_mapping(
        "midi",
        "midi:cc:1:7",
        master_volume_delta_action(),
    )

    for value, expected in (
        (64, 0.5),
        (65, 0.51),
        (65, 0.52),
        (63, 0.51),
        (63, 0.5),
    ):
        controller.input_mapping._handle_rust_input_event({
            "source": "midi",
            "binding_key": "midi:cc:1:7",
            "value": value,
            "action_key": "global.volume.delta",
            "direct": False,
        })
        assert controller.project.volume == pytest.approx(expected)


def test_midi_cc_relative_master_volume_uses_learned_inc_dec_code(
    controller: AppController,
) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.volume = 0.5

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:7",
        "value": 65,
        "direct": False,
    })
    ctx.audio.global_.set_volume(0.5)

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:7",
        "value": 65,
        "action_key": "global.volume.delta",
        "direct": False,
    })

    assert controller.project.volume == pytest.approx(0.51)


def test_midi_nrpn_relative_master_volume_uses_learned_inc_dec_code(
    controller: AppController,
) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.volume = 0.5

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:nrpn:1:0",
        "value": 65,
        "direct": False,
    })
    ctx.audio.global_.set_volume(0.5)

    for value, expected in ((65, 0.51), (65, 0.52), (63, 0.51)):
        controller.input_mapping._handle_rust_input_event({
            "source": "midi",
            "binding_key": "midi:nrpn:1:0",
            "value": value,
            "action_key": "global.volume.delta",
            "direct": False,
        })
        assert controller.project.volume == pytest.approx(expected)


def test_midi_cc_relative_pad_eq_uses_directional_steps(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.input_mapping.save_mapping(
        "midi",
        "midi:cc:1:74",
        pad_eq_delta_action(2, "mid"),
    )

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:74",
        "value": 10,
        "action_key": "pad.eq.delta:2:mid",
        "direct": False,
    })
    audio_engine_mock.set_pad_eq.assert_not_called()

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:74",
        "value": 11,
        "action_key": "pad.eq.delta:2:mid",
        "direct": False,
    })
    audio_engine_mock.set_pad_eq.assert_called_once_with(2, 0.0, 0.5, 0.0)
    assert controller.project.pad_eq_mid_db[2] == pytest.approx(0.5)

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:74",
        "value": 9,
        "action_key": "pad.eq.delta:2:mid",
        "direct": False,
    })
    assert controller.project.pad_eq_mid_db[2] == pytest.approx(0.0)


def test_midi_cc_relative_pad_gain_uses_directional_steps(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.pad_gain_db[2] = 0.0
    controller.input_mapping.save_mapping(
        "midi",
        "midi:cc:1:73",
        pad_gain_delta_action(2),
    )

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:73",
        "value": 64,
        "action_key": "pad.gain.delta:2",
        "direct": False,
    })
    audio_engine_mock.set_pad_gain.assert_not_called()

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:73",
        "value": 65,
        "action_key": "pad.gain.delta:2",
        "direct": False,
    })
    assert controller.project.pad_gain_db[2] == pytest.approx(0.1)

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:73",
        "value": 63,
        "action_key": "pad.gain.delta:2",
        "direct": False,
    })
    assert controller.project.pad_gain_db[2] == pytest.approx(0.0)


def test_midi_cc_relative_global_speed_uses_directional_steps(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.speed = 1.0
    controller.input_mapping.save_mapping(
        "midi",
        "midi:cc:1:72",
        global_speed_delta_action(),
    )

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:72",
        "value": 64,
        "action_key": "global.speed.delta",
        "direct": False,
    })
    audio_engine_mock.set_speed.assert_not_called()

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:72",
        "value": 65,
        "action_key": "global.speed.delta",
        "direct": False,
    })
    assert controller.project.speed == pytest.approx(1.01)

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:72",
        "value": 63,
        "action_key": "global.speed.delta",
        "direct": False,
    })
    assert controller.project.speed == pytest.approx(1.0)


def test_midi_cc_relative_global_speed_uses_bpm_steps_when_reference_exists(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.selected_pad = 1
    controller.transport.bpm.set_manual_bpm(1, 120.0)
    controller.input_mapping.save_mapping(
        "midi",
        "midi:cc:1:72",
        global_speed_delta_action(),
    )

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:72",
        "value": 64,
        "action_key": "global.speed.delta",
        "direct": False,
    })
    audio_engine_mock.set_speed.assert_not_called()

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:72",
        "value": 65,
        "action_key": "global.speed.delta",
        "direct": False,
    })
    assert controller.project.speed == pytest.approx(120.1 / 120.0)


def test_keyboard_mapping_executes_stem_mask_action_without_available_cache(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="S")
    controller.input_mapping.save_mapping(
        "keyboard",
        binding.key,
        LooperAction.stem_mask(0, STEM_MASK_VOCALS, "custom"),
    )

    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    assert controller.session.pad_stem_enabled_mask[0] == STEM_MASK_VOCALS
    assert controller.session.pad_stem_mask_display_mode[0] == "custom"
    audio_engine_mock.set_stem_enabled_mask.assert_not_called()


def test_non_direct_rust_midi_event_executes_python_action(
    controller: AppController,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.selected_bank = 0

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:7",
        "action_key": "ui.select_bank:2",
        "direct": False,
        "dispatched": True,
    })

    assert controller.project.selected_bank == 2


def test_rust_midi_event_is_ignored_when_mapping_disabled(
    controller: AppController,
) -> None:
    controller.project.selected_bank = 0

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:7",
        "action_key": "ui.select_bank:2",
        "direct": False,
        "dispatched": True,
    })

    assert controller.project.selected_bank == 0


def test_direct_rust_midi_event_is_not_executed_twice(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.sample_paths[0] = "samples/foo.wav"

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.trigger:0",
        "direct": True,
        "dispatched": True,
    })

    audio_engine_mock.play_sample_exclusive.assert_not_called()


def test_future_dsp_midi_event_does_not_call_audio_without_explicit_handler(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    audio_engine_mock.reset_mock()

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:74",
        "value": 65,
        "action_key": "dsp.pad.parameter.delta:0:filter.cutoff",
        "direct": False,
        "dispatched": True,
    })

    assert audio_engine_mock.method_calls == []


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
