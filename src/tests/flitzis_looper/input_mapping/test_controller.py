from typing import TYPE_CHECKING
from unittest.mock import ANY, Mock

import pytest

from flitzis_looper.input_mapping.actions import (
    LooperAction,
    PadEqBand,
    global_speed_action,
    global_speed_delta_action,
    master_volume_action,
    master_volume_delta_action,
    pad_eq_action,
    pad_eq_delta_action,
    pad_gain_action,
    pad_gain_delta_action,
    selected_pad_eq_delta_action,
    selected_tap_bpm_action,
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


def test_toggle_learn_publishes_capture_state_to_rust(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    audio_engine_mock.reset_mock()

    controller.input_mapping.toggle_learn()

    active = True
    audio_engine_mock.set_input_learn_active.assert_called_once_with(active)

    audio_engine_mock.reset_mock()
    controller.input_mapping.toggle_learn()

    active = False
    audio_engine_mock.set_input_learn_active.assert_called_once_with(active)


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
    assert data.mappings[0].action.key == "pad.tap_bpm.selected"
    assert controller.session.input_learn_active is False


@pytest.mark.parametrize("source", ["keyboard", "midi"])
def test_mapped_global_key_lock_uses_existing_broadcast_and_learn_suppresses_it(
    controller: AppController, audio_engine_mock: Mock, source: str
) -> None:
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.sample_paths[37] = "samples/another-bank.wav"
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="A").key if source == "keyboard" else "midi:note:1:60"
    controller.input_mapping.save_mapping(source, binding, LooperAction.toggle_key_lock())

    def deliver() -> None:
        if source == "keyboard":
            controller.input_mapping.capture_keyboard_input(
                KeyboardBinding.from_key(binding), text_input_focused=False
            )
        else:
            controller.input_mapping._handle_rust_input_event({
                "source": "midi",
                "binding_key": binding,
                "action_key": "global.key_lock.toggle",
            })

    deliver()
    assert controller.project.key_lock
    assert controller.project.pad_key_lock[0]
    assert controller.project.pad_key_lock[37]
    assert audio_engine_mock.set_pad_key_lock.call_count == 2
    controller.input_mapping.toggle_learn()
    deliver()
    assert controller.project.key_lock
    assert audio_engine_mock.set_pad_key_lock.call_count == 2
    assert controller.session.input_learn_pending_binding_key == binding


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
    assert data.mappings[0].action.key == "pad.eq.selected.delta:mid"
    assert controller.project.pad_eq_mid_db[2] == 0.0


def test_midi_learn_keeps_first_pending_input_for_eq_cc_burst(
    controller: AppController,
) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:70",
        "value": 64,
    })
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:71",
        "value": 64,
    })

    ctx.audio.pads.set_pad_eq_band(2, "low", 0.0)

    data = load_midi_mapping_file()
    assert len(data.mappings) == 1
    assert data.mappings[0].input.key == "midi:cc:1:70"
    assert data.mappings[0].action.key == "pad.eq.selected.delta:low"


def test_learn_start_discards_queued_midi_tail_before_capturing_next_pot(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)
    audio_engine_mock.poll_input_events.side_effect = [
        {
            "source": "midi",
            "binding_key": "midi:cc:1:70",
            "value": 65,
        },
        None,
    ]

    ctx.input.toggle_learn()

    assert controller.session.input_learn_pending_binding_key is None
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:71",
        "value": 64,
    })
    ctx.audio.pads.set_pad_eq_band(2, "mid", 0.0)

    data = load_midi_mapping_file()
    assert len(data.mappings) == 1
    assert data.mappings[0].input.key == "midi:cc:1:71"
    assert data.mappings[0].action.key == "pad.eq.selected.delta:mid"


def test_learn_saves_three_pad_eq_band_mappings(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)

    eq_targets: tuple[tuple[int, PadEqBand], ...] = (
        (70, "low"),
        (71, "mid"),
        (72, "high"),
    )
    for cc_number, band in eq_targets:
        ctx.input.toggle_learn()
        controller.input_mapping._handle_rust_input_event({
            "source": "midi",
            "binding_key": f"midi:cc:1:{cc_number}",
            "value": 64,
        })
        ctx.audio.pads.set_pad_eq_band(2, band, 0.0)

    data = load_midi_mapping_file()
    assert [mapping.input.key for mapping in data.mappings] == [
        "midi:cc:1:70",
        "midi:cc:1:71",
        "midi:cc:1:72",
    ]
    assert [mapping.action.key for mapping in data.mappings] == [
        "pad.eq.selected.delta:low",
        "pad.eq.selected.delta:mid",
        "pad.eq.selected.delta:high",
    ]
    audio_engine_mock.set_input_mapping_snapshot.assert_called_with([
        ("midi:cc:1:70", "pad.eq.selected.delta:low"),
        ("midi:cc:1:71", "pad.eq.selected.delta:mid"),
        ("midi:cc:1:72", "pad.eq.selected.delta:high"),
    ])


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


def test_learn_saves_start_stop_mapping(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.session.active_sample_ids.add(0)

    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:63",
        "value": 100,
    })

    ctx.audio.global_.start_or_restart_start_stop()

    data = load_midi_mapping_file()
    assert data.mappings[0].input.key == "midi:note:1:63"
    assert data.mappings[0].action.key == "global.start_stop"
    audio_engine_mock.play_sample.assert_not_called()
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


def test_keyboard_mapping_executes_selected_tap_bpm(
    controller: AppController,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="T")
    controller.input_mapping.save_mapping(
        "keyboard",
        binding.key,
        selected_tap_bpm_action(),
    )

    controller.project.selected_pad = 3
    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    assert controller.session.tap_bpm_pad_id == 3
    assert len(controller.session.tap_bpm_timestamps) == 1

    controller.project.selected_pad = 5
    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    assert controller.session.tap_bpm_pad_id == 5
    assert len(controller.session.tap_bpm_timestamps) == 1


def test_midi_mapping_executes_selected_tap_bpm(controller: AppController) -> None:
    controller.input_mapping.set_enabled(enabled=True)

    controller.project.selected_pad = 3
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.tap_bpm.selected",
        "direct": False,
        "dispatched": True,
    })

    assert controller.session.tap_bpm_pad_id == 3
    assert len(controller.session.tap_bpm_timestamps) == 1

    controller.project.selected_pad = 5
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.tap_bpm.selected",
        "direct": False,
        "dispatched": True,
    })

    assert controller.session.tap_bpm_pad_id == 5
    assert len(controller.session.tap_bpm_timestamps) == 1


def test_keyboard_mapping_adjust_loop_toggles_and_switches_editor(
    controller: AppController,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="W")
    controller.input_mapping.save_mapping("keyboard", binding.key, LooperAction.adjust_loop(2))
    controller.project.sample_paths[1] = "samples/other.wav"
    controller.project.sample_paths[2] = "samples/foo.wav"

    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    assert controller.session.waveform_editor_open is True
    assert controller.session.waveform_editor_pad_id == 2

    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    assert controller.session.waveform_editor_open is False
    assert controller.session.waveform_editor_pad_id is None

    controller.session.waveform_editor_open = True
    controller.session.waveform_editor_pad_id = 1
    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    assert controller.session.waveform_editor_open is True
    assert controller.session.waveform_editor_pad_id == 2


def test_input_mapping_analyze_unloaded_pad_is_handled(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    handled = controller.input_mapping.execute_action(LooperAction.analyze_pad(0))

    assert handled is True
    audio_engine_mock.analyze_sample_async.assert_not_called()
    assert 0 not in controller.session.analyzing_sample_ids
    assert controller.session.sample_analysis_errors[0] == "sample is not loaded"


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


def test_keyboard_mapping_executes_pad_gain_negative_minimum(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    binding = KeyboardBinding(key_name="G")
    controller.input_mapping.save_mapping(
        "keyboard",
        binding.key,
        pad_gain_action(2, -60.0),
    )

    handled = controller.input_mapping.capture_keyboard_input(
        binding,
        text_input_focused=False,
    )

    assert handled is True
    audio_engine_mock.set_pad_gain.assert_called_once_with(2, -60.0)
    assert controller.project.pad_gain_db[2] == -60.0


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


def test_midi_cc_relative_selected_pad_eq_follows_current_selection(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.input_mapping.save_mapping(
        "midi",
        "midi:cc:1:70",
        selected_pad_eq_delta_action("high"),
    )

    controller.project.selected_pad = 2
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:70",
        "value": 64,
        "action_key": "pad.eq.selected.delta:high",
        "direct": False,
    })
    audio_engine_mock.set_pad_eq.assert_not_called()

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:70",
        "value": 65,
        "action_key": "pad.eq.selected.delta:high",
        "direct": False,
    })
    audio_engine_mock.set_pad_eq.assert_called_once_with(2, 0.0, 0.0, 0.5)
    assert controller.project.pad_eq_high_db[2] == pytest.approx(0.5)

    controller.project.selected_pad = 5
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:cc:1:70",
        "value": 66,
        "action_key": "pad.eq.selected.delta:high",
        "direct": False,
    })

    assert controller.project.pad_eq_high_db[2] == pytest.approx(0.5)
    assert controller.project.pad_eq_high_db[5] == pytest.approx(0.5)


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


def test_non_direct_rust_midi_event_executes_start_stop_action(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.session.active_sample_ids.add(0)

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:63",
        "action_key": "global.start_stop",
        "direct": False,
        "dispatched": True,
    })

    audio_engine_mock.start_global_playback_batch.assert_called_once_with(
        [(ANY, 0.0, None)], received_at_ns=None
    )
    audio_engine_mock.play_sample.assert_not_called()


def test_rust_midi_event_is_ignored_when_mapping_disabled(
    controller: AppController,
) -> None:
    controller.input_mapping.set_enabled(enabled=False)
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


def test_input_runtime_state_sync_skips_unchanged_frames(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.reset_mock()

    controller.input_mapping.on_frame_render()

    audio_engine_mock.set_input_runtime_state.assert_not_called()

    controller.project.multi_loop = True
    controller.input_mapping.on_frame_render()

    audio_engine_mock.set_input_runtime_state.assert_called_once()
    multi_loop, loaded, loop_starts, loop_ends, _bindings = (
        audio_engine_mock.set_input_runtime_state.call_args.args
    )
    assert multi_loop is True
    assert loaded[0] is False
    assert loop_starts[0] == 0.0
    assert loop_ends[0] is None

    audio_engine_mock.reset_mock()
    controller.input_mapping.on_frame_render()

    audio_engine_mock.set_input_runtime_state.assert_not_called()


def test_input_runtime_signature_uses_effective_bpm_and_republishes_precision_changes(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.project.sample_paths[0] = "samples/fractional.wav"
    controller.project.sample_durations[0] = 100.0
    controller.project.manual_bpm[0] = 119.999
    controller.project.pad_loop_auto[0] = True
    audio_engine_mock.reset_mock()

    controller.input_mapping.on_frame_render()
    signature = controller.input_mapping._last_input_runtime_state_signature
    assert signature is not None
    timing = signature[2][0].timing
    assert timing is not None
    assert timing.period_seconds == 60.0 / 119.999
    first_region = audio_engine_mock.set_input_runtime_state.call_args.args[3][0]
    audio_engine_mock.reset_mock()
    controller.input_mapping.on_frame_render()
    audio_engine_mock.set_input_runtime_state.assert_not_called()

    controller.project.manual_bpm[0] = 120.00128936767578
    controller.input_mapping.on_frame_render()
    signature = controller.input_mapping._last_input_runtime_state_signature
    assert signature is not None
    timing = signature[2][0].timing
    assert timing is not None
    assert timing.period_seconds == 60.0 / 120.00128936767578
    audio_engine_mock.set_input_runtime_state.assert_called_once()
    new_region = audio_engine_mock.set_input_runtime_state.call_args.args[3][0]
    assert new_region != first_region


@pytest.mark.parametrize("changed_input", ["source", "duration", "rate"])
def test_input_runtime_snapshot_republishes_source_and_loaded_domain_changes(
    controller: AppController,
    audio_engine_mock: Mock,
    changed_input: str,
) -> None:
    controller.project.sample_paths[0] = "samples/long.wav"
    controller.project.sample_durations[0] = 100_000.0
    controller.project.pad_loop_start_s[0] = (2**24 + 1) / 48_000
    controller.project.pad_loop_end_s[0] = (2**24 + 17) / 48_000
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.reset_mock()
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.set_input_runtime_state.assert_not_called()

    if changed_input == "source":
        controller.project.sample_paths[0] = "samples/replacement.wav"
    elif changed_input == "duration":
        controller.project.sample_durations[0] = 100_000.0 + 1 / 48_000
    else:
        audio_engine_mock.output_sample_rate.return_value = 44_100
    controller.input_mapping._sync_rust_runtime_state()

    audio_engine_mock.set_input_runtime_state.assert_called_once()
    _, loaded, starts, ends, _bindings = audio_engine_mock.set_input_runtime_state.call_args.args
    assert loaded[0]
    assert (starts[0], ends[0]) == controller.transport.loop.effective_region(0)
    audio_engine_mock.reset_mock()
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.set_input_runtime_state.assert_not_called()


def test_input_runtime_snapshot_publishes_exact_effective_endpoints_once(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    controller.project.sample_paths[0] = "samples/long.wav"
    start_s = (2**24 + 1) / 48_000
    end_s = (2**24 + 17) / 48_000
    region = Mock(return_value=(start_s, end_s))
    monkeypatch.setattr(controller.transport.loop, "effective_region", region)
    controller.input_mapping._sync_rust_runtime_state()
    region.assert_called_once_with(0, timing=None)
    audio_engine_mock.reset_mock()
    region.reset_mock()

    end_s = (2**24 + 18) / 48_000
    region.return_value = (start_s, end_s)
    controller.input_mapping._sync_rust_runtime_state()

    region.assert_called_once_with(0, timing=None)
    audio_engine_mock.set_input_runtime_state.assert_called_once()
    _, _, starts, ends, _bindings = audio_engine_mock.set_input_runtime_state.call_args.args
    assert starts[0] == start_s
    assert ends[0] == end_s


def test_input_runtime_state_sync_republishes_loaded_loop_changes(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 1.0
    controller.project.pad_loop_end_s[0] = 4.0
    audio_engine_mock.reset_mock()

    controller.input_mapping.on_frame_render()

    audio_engine_mock.set_input_runtime_state.assert_called_once()
    _, loaded, loop_starts, loop_ends, _bindings = (
        audio_engine_mock.set_input_runtime_state.call_args.args
    )
    assert loaded[0] is True
    assert loop_starts[0] == pytest.approx(1.0)
    assert loop_ends[0] == pytest.approx(4.0)

    audio_engine_mock.reset_mock()
    controller.input_mapping.on_frame_render()

    audio_engine_mock.set_input_runtime_state.assert_not_called()

    controller.transport.loop.set_start(0, 2.0)
    audio_engine_mock.reset_mock()

    controller.input_mapping.on_frame_render()

    audio_engine_mock.set_input_runtime_state.assert_called_once()
    _, _, loop_starts, loop_ends, _bindings = (
        audio_engine_mock.set_input_runtime_state.call_args.args
    )
    assert loop_starts[0] == pytest.approx(2.0)
    assert loop_ends[0] == pytest.approx(4.0)


def test_failed_direct_rust_midi_event_retries_guarded_native_trigger(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.sample_paths[0] = "samples/foo.wav"
    audio_engine_mock.reset_mock()

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.trigger:0",
        "direct": True,
        "dispatched": False,
    })

    audio_engine_mock.trigger_input_runtime_pad.assert_called_once_with(0, received_at_ns=None)
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()


@pytest.mark.parametrize("received_at_ns", [0, 12_345_678])
@pytest.mark.parametrize(("direct", "dispatched"), [(True, False), (False, False), (False, True)])
def test_midi_fallback_preserves_original_timestamp(
    controller: AppController,
    audio_engine_mock: Mock,
    received_at_ns: int,
    *,
    direct: bool,
    dispatched: bool,
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.sample_paths[0] = "samples/foo.wav"

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.trigger:0",
        "received_at_ns": received_at_ns,
        "direct": direct,
        "dispatched": dispatched,
    })

    audio_engine_mock.trigger_input_runtime_pad.assert_called_once_with(
        0, received_at_ns=received_at_ns
    )
    audio_engine_mock.play_sample_exclusive.assert_not_called()


@pytest.mark.parametrize("received_at_ns", [-1, True, 1.5, "12", 1 << 64])
def test_invalid_midi_timestamp_reports_error_and_refuses_guarded_retry(
    controller: AppController, audio_engine_mock: Mock, received_at_ns: object
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.sample_paths[0] = "samples/foo.wav"

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.trigger:0",
        "received_at_ns": received_at_ns,
        "direct": True,
        "dispatched": False,
    })

    audio_engine_mock.trigger_input_runtime_pad.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    assert "received_at_ns" in str(controller.session.input_mapping_error)


def test_successful_direct_midi_dispatch_is_not_replayed_even_with_invalid_timestamp(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.sample_paths[0] = "samples/foo.wav"
    audio_engine_mock.reset_mock()

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.trigger:0",
        "received_at_ns": -1,
        "direct": True,
        "dispatched": True,
    })

    assert audio_engine_mock.method_calls == []


def test_keyboard_mapping_preserves_captured_input_time(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.sample_paths[0] = "samples/foo.wav"
    binding = KeyboardBinding(key_name="B")
    controller.input_mapping.save_mapping("keyboard", binding.key, LooperAction.trigger_pad(0))

    handled = controller.input_mapping.capture_keyboard_input(
        binding, text_input_focused=False, received_at_ns=41
    )

    assert handled is True
    audio_engine_mock.play_sample_exclusive.assert_called_once_with(0, 1.0, received_at_ns=41)


def test_midi_controller_owned_global_restart_preserves_timestamp_for_batch(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    for pad_id in (0, 1):
        controller.project.sample_paths[pad_id] = f"samples/{pad_id}.wav"
    controller.session.active_sample_ids.update({0, 1})

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:63",
        "action_key": "global.start_stop",
        "received_at_ns": 19,
        "direct": False,
        "dispatched": True,
    })

    audio_engine_mock.start_global_playback_batch.assert_called_once_with(
        [(ANY, 0.0, None), (ANY, 0.0, None)], received_at_ns=19
    )
    entries = audio_engine_mock.start_global_playback_batch.call_args.args[0]
    assert [binding.metadata()["pad_id"] for binding, _, _ in entries] == [0, 1]
    audio_engine_mock.play_sample.assert_not_called()


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
