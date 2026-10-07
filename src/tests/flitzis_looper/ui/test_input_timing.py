from contextlib import nullcontext
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import ANY, Mock, call

from imgui_bundle import imgui, implot

from flitzis_looper.input_mapping import KeyboardBinding, LooperAction
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import bottom_bar, performance_view, render, waveform_editor

if TYPE_CHECKING:
    import pytest

    from flitzis_looper.controller import AppController


def test_keyboard_ui_captures_rust_time_before_dispatch(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.sample_paths[0] = "samples/foo.wav"
    binding = KeyboardBinding(key_name="B")
    controller.input_mapping.save_mapping("keyboard", binding.key, LooperAction.trigger_pad(0))
    audio_engine_mock.reset_mock()
    audio_engine_mock.attach_mock(Mock(return_value=53), "capture_input_timestamp_ns")
    monkeypatch.setattr(render, "KEYBOARD_INPUT_KEYS", [("B", imgui.Key.b)])
    monkeypatch.setattr(imgui, "get_io", lambda: SimpleNamespace(want_text_input=False))
    monkeypatch.setattr(imgui, "is_any_item_active", lambda: False)
    monkeypatch.setattr(imgui, "is_key_down", lambda _key: False)
    monkeypatch.setattr(imgui, "is_key_pressed", lambda _key, *, repeat: not repeat)

    render._poll_keyboard_input(UiContext(controller))

    assert audio_engine_mock.method_calls[0] == call.capture_input_timestamp_ns()
    audio_engine_mock.capture_input_timestamp_ns.assert_called_once()
    assert audio_engine_mock.method_calls[-2:] == [
        call.set_pad_loop_region(0, 0.0, None),
        call.play_sample_exclusive(0, 1.0, received_at_ns=53),
    ]


def test_mouse_pad_captures_time_before_loop_publication(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.sample_paths[0] = "samples/foo.wav"
    audio_engine_mock.reset_mock()
    audio_engine_mock.attach_mock(Mock(return_value=0), "capture_input_timestamp_ns")
    monkeypatch.setattr(imgui, "is_mouse_clicked", lambda _button: False)
    monkeypatch.setattr(imgui, "is_mouse_down", lambda button: button == imgui.MouseButton_.left)

    performance_view._pad_button_input(UiContext(controller), 0, is_loaded=True)

    assert audio_engine_mock.method_calls[0] == call.capture_input_timestamp_ns()
    audio_engine_mock.capture_input_timestamp_ns.assert_called_once()
    assert audio_engine_mock.method_calls[-2:] == [
        call.set_pad_loop_region(0, 0.0, None),
        call.play_sample_exclusive(0, 1.0, received_at_ns=0),
    ]


def test_start_stop_mouse_input_captures_once_for_whole_restart_batch(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    for pad_id in (0, 1):
        controller.project.sample_paths[pad_id] = f"samples/{pad_id}.wav"
    controller.session.active_sample_ids.update({0, 1})
    audio_engine_mock.reset_mock()
    audio_engine_mock.attach_mock(Mock(return_value=73), "capture_input_timestamp_ns")
    monkeypatch.setattr(bottom_bar, "button_style", lambda _style: nullcontext())
    monkeypatch.setattr(imgui, "button", lambda *_args: False)
    monkeypatch.setattr(imgui, "is_item_hovered", lambda: True)
    monkeypatch.setattr(imgui, "is_mouse_down", lambda button: button == imgui.MouseButton_.left)
    monkeypatch.setattr(imgui, "is_mouse_clicked", lambda _button: False)
    monkeypatch.setattr(imgui, "set_tooltip", lambda _text: None)

    bottom_bar._start_stop_button(UiContext(controller))

    assert audio_engine_mock.method_calls[0] == call.capture_input_timestamp_ns()
    audio_engine_mock.capture_input_timestamp_ns.assert_called_once()
    audio_engine_mock.start_global_playback_batch.assert_called_once_with(
        [(ANY, 0.0, None), (ANY, 0.0, None)], received_at_ns=73
    )
    entries = audio_engine_mock.start_global_playback_batch.call_args.args[0]
    assert [binding.metadata()["pad_id"] for binding, _, _ in entries] == [0, 1]
    audio_engine_mock.play_sample.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()


def test_waveform_loop_edit_retains_input_time_before_edit_and_restart(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.project.sample_durations[0] = 8.0
    controller.project.pad_loop_end_s[0] = 8.0
    controller.session.waveform_editor_pad_id = 0
    audio_engine_mock.reset_mock()
    audio_engine_mock.attach_mock(Mock(return_value=29), "capture_input_timestamp_ns")
    monkeypatch.setattr(implot, "is_plot_hovered", lambda: True)
    monkeypatch.setattr(imgui, "is_mouse_clicked", lambda _button: False)
    monkeypatch.setattr(
        imgui, "is_mouse_released", lambda button: button == imgui.MouseButton_.left
    )
    monkeypatch.setattr(imgui, "get_mouse_pos", lambda: SimpleNamespace(x=10.0, y=5.0))
    monkeypatch.setattr(implot, "pixels_to_plot", lambda *_args: SimpleNamespace(x=2.5))
    monkeypatch.setattr(
        imgui, "get_mouse_drag_delta", lambda _button: SimpleNamespace(x=0.0, y=0.0)
    )
    monkeypatch.setattr(imgui, "reset_mouse_drag_delta", lambda _button: None)

    waveform_editor._handle_clicks(UiContext(controller), 0, 8.0)

    assert audio_engine_mock.method_calls[0] == call.capture_input_timestamp_ns()
    audio_engine_mock.play_sample.assert_called_once_with(0, 1.0, received_at_ns=29)
    assert controller.project.pad_loop_start_s[0] == 2.5
    audio_engine_mock.play_sample_exclusive.assert_not_called()
