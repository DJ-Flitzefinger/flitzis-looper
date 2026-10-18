"""Close requests keep the UI loop alive until the controller can release ownership."""

from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import hello_imgui, imgui, immapp

from flitzis_looper.ui import run
from flitzis_looper.ui.context import UiContext

if TYPE_CHECKING:
    from imgui_bundle.hello_imgui import RunnerParams

    from flitzis_looper.controller import AppController


@pytest.fixture(autouse=True)
def _runtime_input_without_native_imgui(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(imgui, "get_io", lambda: SimpleNamespace(app_focus_lost=False))
    monkeypatch.setattr(imgui, "is_mouse_down", lambda _button: True)


@pytest.mark.parametrize("sticky_os_close", [False, True])
def test_close_waits_across_frames_and_notifies_exit_only_after_settlement(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, *, sticky_os_close: bool
) -> None:
    prepare = Mock(side_effect=[False, False, True])
    shutdown = Mock()
    exited = Mock()
    pending = Mock()
    monkeypatch.setattr(controller, "prepare_shutdown", prepare)
    monkeypatch.setattr(controller, "shut_down", shutdown)
    monkeypatch.setattr(run, "UiContext", Mock())
    monkeypatch.setattr(run, "render_ui", Mock())
    monkeypatch.setattr(run, "_show_close_pending", pending)

    def frames(params: RunnerParams, _addons: object) -> None:
        monkeypatch.setattr(hello_imgui, "get_runner_params", lambda: params)
        params.callbacks.after_swap()
        prepare.assert_not_called()
        params.app_shall_exit = True
        params.callbacks.after_swap()
        assert params.app_shall_exit is False
        shutdown.assert_not_called()
        exited.assert_not_called()
        controller.arrangement.error = "config write unavailable"
        params.callbacks.show_gui()
        pending.assert_called_once_with("config write unavailable")
        if sticky_os_close:
            params.app_shall_exit = True
        params.callbacks.after_swap()
        assert params.app_shall_exit is False
        assert prepare.call_count == 2
        shutdown.assert_not_called()
        if sticky_os_close:
            params.app_shall_exit = True
        params.callbacks.after_swap()
        assert params.app_shall_exit is True
        assert prepare.call_count == 3
        params.callbacks.before_exit()
        shutdown.assert_called_once()
        exited.assert_called_once()

    monkeypatch.setattr(immapp, "run", frames)

    run.run_ui(controller, on_exit=exited)


def test_close_without_copy_can_exit_in_the_requested_frame(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    prepare = Mock(return_value=True)
    monkeypatch.setattr(controller, "prepare_shutdown", prepare)
    monkeypatch.setattr(run, "UiContext", Mock())

    def frames(params: RunnerParams, _addons: object) -> None:
        monkeypatch.setattr(hello_imgui, "get_runner_params", lambda: params)
        params.app_shall_exit = True
        params.callbacks.after_swap()
        assert params.app_shall_exit is True
        prepare.assert_called_once()

    monkeypatch.setattr(immapp, "run", frames)

    run.run_ui(controller)


def test_pending_close_keeps_native_error_visible_and_balances_popup(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    opened = Mock()
    text = Mock()
    ended = Mock()
    monkeypatch.setattr(imgui, "open_popup", opened)
    monkeypatch.setattr(imgui, "begin_popup_modal", Mock(return_value=(True, None)))
    monkeypatch.setattr(imgui, "text_wrapped", text)
    monkeypatch.setattr(imgui, "end_popup", ended)

    run._show_close_pending("own Copy acknowledgement pending")

    opened.assert_called_once()
    assert text.call_count == 2
    assert "own Copy acknowledgement pending" in text.call_args.args[0]
    ended.assert_called_once()


@pytest.mark.parametrize("cancellation", ["button_up", "focus_cancel", "editor_hidden", "close"])
def test_global_runtime_tick_delivers_release_before_poll_even_without_toolbar(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, cancellation: str
) -> None:
    order: list[str] = []
    release = Mock(side_effect=order.append)
    poll = Mock(side_effect=lambda: order.append("poll"))
    context = UiContext(controller)
    monkeypatch.setattr(context.ui.waveform, "pause_selected_pad_hold_on_release", release)
    monkeypatch.setattr(context.audio.poll, "poll", poll)
    controller.session.settings_open = cancellation == "editor_hidden"
    controller.session.waveform_editor_open = True
    monkeypatch.setattr(
        imgui, "get_io", lambda: SimpleNamespace(app_focus_lost=cancellation == "focus_cancel")
    )
    monkeypatch.setattr(imgui, "is_mouse_down", lambda _button: cancellation != "button_up")
    run._poll_runtime_tick(context, closing=cancellation == "close")
    assert order == [cancellation, "poll"]
    poll.assert_called_once()


@pytest.mark.parametrize("boundary", ["button_up", "focus_cancel", "close"])
@pytest.mark.parametrize("learn_active", [False, True])
def test_global_tick_releases_output_mute_without_rendering_start_stop(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    boundary: str,
    *,
    learn_active: bool,
) -> None:
    """Release the actual global mute before polling, including hidden/Learn controls."""
    controller.project.volume = 0.45
    controller.session.settings_open = True
    controller.session.input_learn_active = learn_active
    controller.transport.global_params.set_momentary_output_mute(enabled=True)
    context = UiContext(controller)
    audio_engine_mock.reset_mock()
    monkeypatch.setattr(
        imgui, "get_io", lambda: SimpleNamespace(app_focus_lost=boundary == "focus_cancel")
    )
    monkeypatch.setattr(
        imgui,
        "is_mouse_down",
        lambda button: button != imgui.MouseButton_.middle or boundary != "button_up",
    )

    def poll_after_release() -> None:
        assert not controller.session.global_stop_momentary_mute_active
        audio_engine_mock.set_volume.assert_called_once_with(0.45)

    poll = Mock(side_effect=poll_after_release)
    monkeypatch.setattr(context.audio.poll, "poll", poll)
    run._poll_runtime_tick(context, closing=boundary == "close")
    run._poll_runtime_tick(context, closing=boundary == "close")

    assert poll.call_count == 2
    assert controller.project.volume == pytest.approx(0.45)
    audio_engine_mock.set_volume.assert_called_once_with(0.45)


def test_global_tick_keeps_output_muted_while_middle_remains_held(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A hidden control does not revoke a still-held, focused global mute."""
    controller.project.volume = 0.45
    controller.session.settings_open = True
    controller.transport.global_params.set_momentary_output_mute(enabled=True)
    context = UiContext(controller)
    poll = Mock()
    monkeypatch.setattr(context.audio.poll, "poll", poll)
    audio_engine_mock.reset_mock()

    run._poll_runtime_tick(context)

    assert controller.session.global_stop_momentary_mute_active
    assert controller.project.volume == pytest.approx(0.45)
    audio_engine_mock.set_volume.assert_not_called()
    poll.assert_called_once()
