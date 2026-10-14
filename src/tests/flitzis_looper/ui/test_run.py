"""Close requests keep the UI loop alive until the controller can release ownership."""

from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.ui import run

if TYPE_CHECKING:
    from imgui_bundle.hello_imgui import RunnerParams

    from flitzis_looper.controller import AppController


@pytest.fixture(autouse=True)
def _runtime_input_without_native_imgui(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(run.imgui, "get_io", lambda: SimpleNamespace(app_focus_lost=False))
    monkeypatch.setattr(run.imgui, "is_mouse_down", lambda _button: True)


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
        monkeypatch.setattr(run.hello_imgui, "get_runner_params", lambda: params)
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

    monkeypatch.setattr(run.immapp, "run", frames)

    run.run_ui(controller, on_exit=exited)


def test_close_without_copy_can_exit_in_the_requested_frame(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    prepare = Mock(return_value=True)
    monkeypatch.setattr(controller, "prepare_shutdown", prepare)
    monkeypatch.setattr(run, "UiContext", Mock())

    def frames(params: RunnerParams, _addons: object) -> None:
        monkeypatch.setattr(run.hello_imgui, "get_runner_params", lambda: params)
        params.app_shall_exit = True
        params.callbacks.after_swap()
        assert params.app_shall_exit is True
        prepare.assert_called_once()

    monkeypatch.setattr(run.immapp, "run", frames)

    run.run_ui(controller)


def test_pending_close_keeps_native_error_visible_and_balances_popup(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    opened = Mock()
    text = Mock()
    ended = Mock()
    monkeypatch.setattr(run.imgui, "open_popup", opened)
    monkeypatch.setattr(run.imgui, "begin_popup_modal", Mock(return_value=(True, None)))
    monkeypatch.setattr(run.imgui, "text_wrapped", text)
    monkeypatch.setattr(run.imgui, "end_popup", ended)

    run._show_close_pending("own Copy acknowledgement pending")

    opened.assert_called_once()
    assert text.call_count == 2
    assert "own Copy acknowledgement pending" in text.call_args.args[0]
    ended.assert_called_once()


@pytest.mark.parametrize("cancellation", ["button_up", "focus_cancel", "editor_hidden", "close"])
def test_global_runtime_tick_delivers_release_before_poll_even_without_toolbar(
    monkeypatch: pytest.MonkeyPatch, cancellation: str
) -> None:
    order = []
    release = Mock(side_effect=order.append)
    poll = Mock(side_effect=lambda: order.append("poll"))
    context = SimpleNamespace(
        ui=SimpleNamespace(waveform=SimpleNamespace(pause_selected_pad_hold_on_release=release)),
        audio=SimpleNamespace(poll=SimpleNamespace(poll=poll)),
        state=SimpleNamespace(
            session=SimpleNamespace(
                settings_open=cancellation == "editor_hidden", waveform_editor_open=True
            )
        ),
    )
    monkeypatch.setattr(
        run.imgui, "get_io", lambda: SimpleNamespace(app_focus_lost=cancellation == "focus_cancel")
    )
    monkeypatch.setattr(run.imgui, "is_mouse_down", lambda _button: cancellation != "button_up")
    run._poll_runtime_tick(context, closing=cancellation == "close")
    assert order == [cancellation, "poll"]
    poll.assert_called_once()
