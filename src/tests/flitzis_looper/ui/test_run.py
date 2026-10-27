"""Close requests keep the UI loop alive until the controller can release ownership."""

from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import Mock, call

import pytest
from imgui_bundle import hello_imgui, imgui, immapp

from flitzis_looper.ui import run
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import file_dialog
from tests.flitzis_looper.controller.test_stem_delete_backend import _loaded

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


def test_closing_continues_controller_settlement_while_owned_picker_retires(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    prepare = Mock(return_value=True)
    shutdown = Mock()
    picker = Mock(side_effect=[False, True, True, True])
    monkeypatch.setattr(controller, "prepare_shutdown", prepare)
    monkeypatch.setattr(controller, "shut_down", shutdown)
    monkeypatch.setattr(run, "cancel_file_dialog", picker)
    monkeypatch.setattr(run, "UiContext", Mock())

    def frames(params: RunnerParams, _addons: object) -> None:
        monkeypatch.setattr(hello_imgui, "get_runner_params", lambda: params)
        params.app_shall_exit = True
        params.callbacks.after_swap()
        assert not params.app_shall_exit
        assert prepare.call_count == 1
        shutdown.assert_not_called()
        params.callbacks.after_swap()
        assert params.app_shall_exit
        assert prepare.call_count == 2
        params.callbacks.before_exit()

    monkeypatch.setattr(immapp, "run", frames)
    run.run_ui(controller)
    assert picker.call_args_list == [call(), call(), call(wait=True), call(wait=True)]
    shutdown.assert_called_once()


def test_exit_hook_failure_still_shuts_down_controller(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    shutdown = Mock()
    monkeypatch.setattr(controller, "shut_down", shutdown)
    monkeypatch.setattr(run, "cancel_file_dialog", Mock(return_value=True))
    monkeypatch.setattr(run, "UiContext", Mock())
    monkeypatch.setattr(immapp, "run", lambda params, _addons: params.callbacks.before_exit())
    with pytest.raises(RuntimeError, match="exit hook"):
        run.run_ui(controller, on_exit=Mock(side_effect=RuntimeError("exit hook")))
    shutdown.assert_called_once()


def test_picker_cancel_error_cannot_skip_controller_settlement(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    picker = Mock(cleanup_error=None)
    picker.cancel.side_effect = [PermissionError("owned terminate denied"), True]
    monkeypatch.setattr(
        file_dialog, "_state", file_dialog._DialogState(file_dialog._PendingDialog(0, picker))
    )
    prepare, shutdown = Mock(return_value=True), Mock()
    monkeypatch.setattr(controller, "prepare_shutdown", prepare)
    monkeypatch.setattr(controller, "shut_down", shutdown)
    monkeypatch.setattr(run, "UiContext", Mock())

    def frames(params: RunnerParams, _addons: object) -> None:
        monkeypatch.setattr(hello_imgui, "get_runner_params", lambda: params)
        params.app_shall_exit = True
        params.callbacks.after_swap()
        assert prepare.call_count == 1
        assert not params.app_shall_exit
        assert file_dialog._state.pending is not None
        params.callbacks.after_swap()
        assert prepare.call_count == 2
        assert params.app_shall_exit
        params.callbacks.before_exit()

    monkeypatch.setattr(immapp, "run", frames)
    run.run_ui(controller)
    shutdown.assert_called_once()


def test_unexpected_runner_failure_attempts_guarded_controller_teardown(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    shutdown = Mock()
    monkeypatch.setattr(controller, "shut_down", shutdown)
    monkeypatch.setattr(run, "cancel_file_dialog", Mock(return_value=True))
    monkeypatch.setattr(run, "UiContext", Mock())
    monkeypatch.setattr(immapp, "run", Mock(side_effect=RuntimeError("runner failure")))
    with pytest.raises(RuntimeError, match="runner failure"):
        run.run_ui(controller)
    shutdown.assert_called_once()


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


def test_hidden_close_poll_logs_changed_errors_once_without_releasing_pending_owners(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    reasons = [
        "own Copy acknowledgement pending",
        "own Copy acknowledgement pending",
        "config writer unavailable",
        None,
        "own Copy acknowledgement pending",
    ]
    runtime = Mock()
    shutdown = Mock()
    monkeypatch.setattr(run, "UiContext", Mock())
    monkeypatch.setattr(run, "_poll_runtime_tick", runtime)
    monkeypatch.setattr(controller, "shut_down", shutdown)

    def prepare(*, holds_polled: bool, deletion_polled: bool) -> bool:
        assert holds_polled
        assert not deletion_polled  # ShowGui is omitted on every minimized frame.
        controller.arrangement.error = reasons.pop(0)
        return False

    preparing = Mock(side_effect=prepare)
    monkeypatch.setattr(controller, "prepare_shutdown", preparing)

    def frames(params: RunnerParams, _addons: object) -> None:
        monkeypatch.setattr(hello_imgui, "get_runner_params", lambda: params)
        params.app_shall_exit = True
        for expected in [
            "own Copy acknowledgement pending",
            None,
            "config writer unavailable",
            None,
            "own Copy acknowledgement pending",
        ]:
            params.callbacks.after_swap()
            output = capsys.readouterr().err
            if expected is None:
                assert "operation=shutdown" not in output
            else:
                assert output.count("operation=shutdown") == 1
                assert f"reason={expected}" in output
            assert not params.app_shall_exit
            shutdown.assert_not_called()

    monkeypatch.setattr(immapp, "run", frames)
    run.run_ui(controller)
    assert preparing.call_count == 5
    assert runtime.call_count == 5


def test_closing_frame_shares_delete_budget_and_minimized_frames_still_advance(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _loaded(controller, audio_engine_mock, tuple(range(17)))
    audio_engine_mock.poll_loader_events.return_value = None
    controller.performance_confirmation.request_all_loaded_stem_deletion()
    warning = controller.performance_confirmation.pending
    assert warning is not None
    controller.performance_confirmation.accept(warning)
    monkeypatch.setattr(run, "render_ui", lambda _context: controller.on_frame_render())
    monkeypatch.setattr(run, "_show_close_pending", Mock())

    def settled() -> int:
        summary = controller.stem_delete.summary
        assert summary is not None
        return summary.noop

    def frames(params: RunnerParams, _addons: object) -> None:
        monkeypatch.setattr(hello_imgui, "get_runner_params", lambda: params)
        params.callbacks.show_gui()
        assert settled() == 8
        params.app_shall_exit = True
        params.callbacks.after_swap()
        assert settled() == 8
        assert not params.app_shall_exit
        # A minimized frame can omit ShowGui; close preparation must still progress.
        params.callbacks.after_swap()
        assert settled() == 16
        assert not params.app_shall_exit
        params.callbacks.show_gui()
        params.callbacks.after_swap()
        assert settled() == 17
        assert params.app_shall_exit

    monkeypatch.setattr(immapp, "run", frames)
    run.run_ui(controller)


def test_pending_close_shows_generic_progress_without_an_error_body(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    opened = Mock()
    text = Mock()
    ended = Mock()
    monkeypatch.setattr(imgui, "open_popup", opened)
    monkeypatch.setattr(imgui, "begin_popup_modal", Mock(return_value=(True, None)))
    monkeypatch.setattr(imgui, "text_wrapped", text)
    monkeypatch.setattr(imgui, "end_popup", ended)
    monkeypatch.setattr(
        imgui,
        "get_main_viewport",
        lambda: SimpleNamespace(work_size=SimpleNamespace(x=1200), get_center=lambda: (600, 350)),
    )
    monkeypatch.setattr(imgui, "get_font_size", lambda: 16.0)
    monkeypatch.setattr(
        imgui,
        "get_style",
        lambda: SimpleNamespace(
            frame_padding=SimpleNamespace(x=4), window_padding=SimpleNamespace(x=8)
        ),
    )
    monkeypatch.setattr(imgui, "calc_text_size", lambda _text: SimpleNamespace(x=180))
    monkeypatch.setattr(imgui, "set_next_window_pos", Mock())
    monkeypatch.setattr(imgui, "set_next_window_size", Mock())

    run._show_close_pending("own Copy acknowledgement pending")

    opened.assert_called_once()
    assert text.call_args_list == [
        call("Please wait while pending work finishes."),
    ]
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
