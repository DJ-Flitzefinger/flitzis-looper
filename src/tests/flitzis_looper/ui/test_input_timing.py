from contextlib import nullcontext
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import ANY, Mock, call

from imgui_bundle import imgui, implot

from flitzis_looper.input_mapping import KeyboardBinding, LooperAction
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import bottom_bar, performance_view, render, waveform_editor
from tests.flitzis_looper.conftest import FakeCapturedGlobalStart, FakeCapturedPadStart

if TYPE_CHECKING:
    import pytest

    from flitzis_looper.controller import AppController


def _record_native_captures[T](capture_method: Mock, expected_type: type[T]) -> list[T]:
    """Record actual opaque receipt identities from the installed typed factory."""
    originals: list[T] = []
    factory = capture_method.side_effect
    assert callable(factory)

    def capture(*args: object, **kwargs: object) -> T:
        original = factory(*args, **kwargs)
        assert isinstance(original, expected_type)
        originals.append(original)
        return original

    capture_method.side_effect = capture
    return originals


def test_keyboard_ui_captures_rust_time_before_dispatch(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.sample_paths[0] = "samples/foo.wav"
    binding = KeyboardBinding(key_name="B")
    controller.input_mapping.save_mapping("keyboard", binding.key, LooperAction.trigger_pad(0))
    audio_engine_mock.reset_mock()
    audio_engine_mock.attach_mock(Mock(return_value=53), "capture_input_timestamp_ns")
    originals = _record_native_captures(audio_engine_mock.capture_pad_start, FakeCapturedPadStart)
    monkeypatch.setattr(render, "KEYBOARD_INPUT_KEYS", [("B", imgui.Key.b)])
    monkeypatch.setattr(imgui, "get_io", lambda: SimpleNamespace(want_text_input=False))
    monkeypatch.setattr(imgui, "is_any_item_active", lambda: False)
    monkeypatch.setattr(imgui, "is_key_down", lambda _key: False)
    monkeypatch.setattr(imgui, "is_key_pressed", lambda _key, *, repeat: not repeat)

    render._poll_keyboard_input(UiContext(controller))

    assert audio_engine_mock.method_calls[0] == call.capture_input_timestamp_ns()
    audio_engine_mock.capture_input_timestamp_ns.assert_called_once()
    capture_call = call.capture_pad_start(
        0, 0.0, None, 1.0, exclusive=True, received_at_ns=53, expected_source_generation=1
    )
    audio_engine_mock.capture_pad_start.assert_called_once_with(
        *capture_call.args, **capture_call.kwargs
    )
    (original,) = originals
    audio_engine_mock.publish_pad_start.assert_called_once_with(original)
    assert audio_engine_mock.publish_pad_start.call_args.args[0] is original
    assert original.received_at_ns == 53
    assert original.exclusive is True
    assert (
        audio_engine_mock.method_calls.index(capture_call)
        < audio_engine_mock.method_calls.index(call.loaded_residency(0))
        < audio_engine_mock.method_calls.index(call.publish_pad_start(original))
    )
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()


def test_mouse_pad_captures_time_before_loop_publication(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.sample_paths[0] = "samples/foo.wav"
    audio_engine_mock.reset_mock()
    audio_engine_mock.attach_mock(Mock(return_value=0), "capture_input_timestamp_ns")
    originals = _record_native_captures(audio_engine_mock.capture_pad_start, FakeCapturedPadStart)
    monkeypatch.setattr(imgui, "is_mouse_clicked", lambda button: button == imgui.MouseButton_.left)
    monkeypatch.setattr(imgui, "is_mouse_down", lambda button: button == imgui.MouseButton_.left)

    performance_view._pad_button_input(UiContext(controller), 0, is_loaded=True)

    assert audio_engine_mock.method_calls[0] == call.capture_input_timestamp_ns()
    audio_engine_mock.capture_input_timestamp_ns.assert_called_once()
    capture_call = call.capture_pad_start(
        0, 0.0, None, 1.0, exclusive=True, received_at_ns=0, expected_source_generation=1
    )
    audio_engine_mock.capture_pad_start.assert_called_once_with(
        *capture_call.args, **capture_call.kwargs
    )
    (original,) = originals
    audio_engine_mock.publish_pad_start.assert_called_once_with(original)
    assert audio_engine_mock.publish_pad_start.call_args.args[0] is original
    assert original.received_at_ns == 0
    assert original.exclusive is True
    assert (
        audio_engine_mock.method_calls.index(capture_call)
        < audio_engine_mock.method_calls.index(call.loaded_residency(0))
        < audio_engine_mock.method_calls.index(call.publish_pad_start(original))
    )
    assert audio_engine_mock.method_calls[-1] == call.select_input_pad(0)
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()


def _pad_mouse_state(monkeypatch: pytest.MonkeyPatch) -> SimpleNamespace:
    mouse = SimpleNamespace(clicked=None, down=None, hovered=True)
    monkeypatch.setattr(imgui, "is_mouse_clicked", lambda button: button == mouse.clicked)
    monkeypatch.setattr(imgui, "is_mouse_down", lambda button: button == mouse.down)
    monkeypatch.setattr(imgui, "is_item_hovered", lambda: mouse.hovered)
    monkeypatch.setattr(imgui, "button", lambda *_args: False)
    monkeypatch.setattr(performance_view, "button_style", lambda _style: nullcontext())
    monkeypatch.setattr(performance_view, "_pad_button_label", lambda *_args, **_kwargs: ("", None))
    monkeypatch.setattr(performance_view, "_pad_button_overlays", lambda *_args, **_kwargs: None)
    return mouse


def test_mouse_pad_release_outside_does_not_suppress_the_next_down_edge(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.sample_paths[0] = "samples/foo.wav"
    audio_engine_mock.reset_mock()
    audio_engine_mock.capture_input_timestamp_ns.side_effect = range(1, 21)
    originals = _record_native_captures(audio_engine_mock.capture_pad_start, FakeCapturedPadStart)
    mouse = _pad_mouse_state(monkeypatch)
    ctx = UiContext(controller)

    for index in range(1, 21):
        mouse.hovered = True
        mouse.clicked = mouse.down = imgui.MouseButton_.left
        performance_view._pad_button(ctx, 0, (100.0, 100.0))
        assert len(originals) == audio_engine_mock.publish_pad_start.call_count == index
        assert controller.session.pressed_pads[0]

        mouse.clicked = None
        performance_view._pad_button(ctx, 0, (100.0, 100.0))
        assert len(originals) == audio_engine_mock.publish_pad_start.call_count == index

        # The release occurs outside the hovered-pad input handler, as in the real grid.
        mouse.hovered, mouse.down = False, None
        performance_view._pad_button(ctx, 0, (100.0, 100.0))

    assert not controller.session.pressed_pads[0]
    assert audio_engine_mock.capture_input_timestamp_ns.call_count == 20
    assert len({id(original) for original in originals}) == 20
    assert audio_engine_mock.capture_pad_start.call_args_list == [
        call(
            0,
            0.0,
            None,
            1.0,
            exclusive=True,
            received_at_ns=timestamp,
            expected_source_generation=1,
        )
        for timestamp in range(1, 21)
    ]
    assert audio_engine_mock.publish_pad_start.call_args_list == [
        call(original) for original in originals
    ]
    assert [original.received_at_ns for original in originals] == list(range(1, 21))
    assert all(original.exclusive and original.loop_region == (0.0, None) for original in originals)
    audio_engine_mock.play_sample_exclusive.assert_not_called()


def test_mouse_held_drag_into_pad_requires_a_new_down_edge(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.sample_paths[0] = "samples/foo.wav"
    mouse = _pad_mouse_state(monkeypatch)
    audio_engine_mock.reset_mock()
    audio_engine_mock.capture_input_timestamp_ns.return_value = 1
    mouse.down = imgui.MouseButton_.left

    performance_view._pad_button(UiContext(controller), 0, (100.0, 100.0))

    audio_engine_mock.capture_input_timestamp_ns.assert_not_called()
    audio_engine_mock.capture_pad_start.assert_not_called()
    audio_engine_mock.publish_pad_start.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    assert controller.session.pressed_pads[0]


def test_mouse_right_hold_stops_on_each_hovered_frame(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.session.active_sample_ids.add(0)
    mouse = _pad_mouse_state(monkeypatch)
    mouse.down = imgui.MouseButton_.right
    ctx = UiContext(controller)
    audio_engine_mock.reset_mock()

    for _ in range(3):
        performance_view._pad_button(ctx, 0, (100.0, 100.0))

    assert audio_engine_mock.stop_sample.call_args_list == [call(0)] * 3
    audio_engine_mock.capture_input_timestamp_ns.assert_not_called()
    audio_engine_mock.capture_pad_start.assert_not_called()
    audio_engine_mock.publish_pad_start.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()


def test_start_stop_mouse_input_captures_once_for_whole_restart_batch(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    for pad_id in (0, 1):
        controller.project.sample_paths[pad_id] = f"samples/{pad_id}.wav"
    controller.session.active_sample_ids.update({0, 1})
    audio_engine_mock.reset_mock()
    audio_engine_mock.attach_mock(Mock(return_value=73), "capture_input_timestamp_ns")
    originals = _record_native_captures(
        audio_engine_mock.capture_global_start, FakeCapturedGlobalStart
    )
    monkeypatch.setattr(bottom_bar, "button_style", lambda _style: nullcontext())
    monkeypatch.setattr(imgui, "button", lambda *_args: False)
    monkeypatch.setattr(imgui, "is_item_hovered", lambda: True)
    monkeypatch.setattr(imgui, "is_mouse_down", lambda button: button == imgui.MouseButton_.left)
    monkeypatch.setattr(imgui, "is_mouse_clicked", lambda _button: False)
    monkeypatch.setattr(imgui, "set_tooltip", lambda _text: None)

    bottom_bar._start_stop_button(UiContext(controller))

    assert audio_engine_mock.method_calls[0] == call.capture_input_timestamp_ns()
    audio_engine_mock.capture_input_timestamp_ns.assert_called_once()
    audio_engine_mock.capture_global_start.assert_called_once_with(
        [(ANY, 0.0, None, None), (ANY, 0.0, None, None)], received_at_ns=73
    )
    (original,) = originals
    audio_engine_mock.publish_captured_global_start.assert_called_once_with(original)
    assert audio_engine_mock.publish_captured_global_start.call_args.args[0] is original
    entries = audio_engine_mock.capture_global_start.call_args.args[0]
    assert original.entries == tuple(entries)
    assert original.received_at_ns == 73
    assert [binding.metadata()["pad_id"] for binding, _, _, _ in entries] == [0, 1]
    assert audio_engine_mock.method_calls.index(
        call.capture_global_start(entries, received_at_ns=73)
    ) < audio_engine_mock.method_calls.index(call.publish_captured_global_start(original))
    audio_engine_mock.start_global_playback_batch.assert_not_called()
    audio_engine_mock.capture_pad_start.assert_not_called()
    audio_engine_mock.play_sample.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()


def test_waveform_loop_edit_retains_input_time_before_edit_and_restart(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.project.sample_durations[0] = 8.0
    controller.project.pad_loop_end_s[0] = 8.0
    controller.project.pad_loop_auto[0] = False
    controller.session.waveform_editor_pad_id = 0
    audio_engine_mock.reset_mock()
    audio_engine_mock.attach_mock(Mock(return_value=29), "capture_input_timestamp_ns")
    originals = _record_native_captures(audio_engine_mock.capture_pad_start, FakeCapturedPadStart)
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
    capture_call = call.capture_pad_start(
        0, 2.5, 8.0, 1.0, exclusive=False, received_at_ns=29, expected_source_generation=1
    )
    audio_engine_mock.capture_pad_start.assert_called_once_with(
        *capture_call.args, **capture_call.kwargs
    )
    (original,) = originals
    audio_engine_mock.publish_pad_start.assert_called_once_with(original)
    assert audio_engine_mock.publish_pad_start.call_args.args[0] is original
    assert original.received_at_ns == 29
    assert original.loop_region == (2.5, 8.0)
    assert original.exclusive is False
    audio_engine_mock.set_pad_loop_region.assert_called_once_with(
        0, 2.5, 8.0, expected_source_generation=1
    )
    assert audio_engine_mock.method_calls.index(
        call.set_pad_loop_region(0, 2.5, 8.0, expected_source_generation=1)
    ) < audio_engine_mock.method_calls.index(capture_call)
    assert controller.project.pad_loop_start_s[0] == 2.5
    audio_engine_mock.play_sample.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()
