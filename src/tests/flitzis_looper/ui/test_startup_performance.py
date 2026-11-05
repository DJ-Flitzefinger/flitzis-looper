"""Actual inactive PAD frames preserve startup ownership and manual navigation."""

from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.audio_output_options import AudioOutputOptions
from flitzis_looper.constants import NUM_PADS
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper.ui.context import SettingsActions
from flitzis_looper.ui.render import render
from tests.flitzis_looper.ui import test_waveform_render_stability as frames

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.context import UiContext

headless_plot = frames.headless_plot
pytestmark = pytest.mark.usefixtures("headless_plot")


def _context() -> SimpleNamespace:
    session = SessionState()
    output = SimpleNamespace(
        phase="starting",
        can_perform=False,
        has_runtime=False,
        exclusive_native_access=True,
        busy=True,
        transaction_active=False,
        error=None,
        draft=AudioOutputOptions(),
        inventory=[],
        observed=None,
        previous_errors={},
    )
    settings = SettingsActions(
        cast(
            "AppController",
            SimpleNamespace(
                session=session,
                audio_output=output,
            ),
        )
    )
    return SimpleNamespace(
        on_frame_render=Mock(),
        state=SimpleNamespace(project=ProjectState(), session=session),
        ui=SimpleNamespace(settings=settings),
        persistence=SimpleNamespace(maybe_flush=Mock()),
    )


def _frame(ctx: SimpleNamespace) -> None:
    imgui.new_frame()
    imgui.set_next_window_size((1100, 700))
    imgui.begin("startup PADs", flags=imgui.WindowFlags_.no_saved_settings)
    render.render_ui(cast("UiContext", ctx))
    imgui.end()
    imgui.render()
    assert imgui.get_draw_data().total_vtx_count > 0


def test_startup_restore_ready_keeps_pads_and_never_queries_native_before_admission(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = _context()
    output = ctx.ui.settings.audio_output
    main, keyboard = Mock(), Mock()
    monkeypatch.setattr(render, "_main", main)
    monkeypatch.setattr(render, "_poll_keyboard_input", keyboard)
    monkeypatch.setattr(render, "_file_dialog", Mock())
    monkeypatch.setattr(render, "performance_confirmation", Mock())
    seen: list[str] = []
    original = imgui.button

    def button(label: str, size: imgui.ImVec2Like = (0, 0)) -> bool:
        clicked = original(label, size)
        if "##startup_pad_" in label:
            assert imgui.internal.get_item_flags() & imgui.internal.ItemFlagsPrivate_.disabled
            seen.append(label)
        return clicked

    monkeypatch.setattr(imgui, "button", button)
    for phase, runtime, exclusive in [
        ("starting", False, True),
        ("startup_restoring", True, False),
    ]:
        output.phase, output.has_runtime, output.exclusive_native_access = phase, runtime, exclusive
        _frame(ctx)
        assert not ctx.state.session.settings_open
        main.assert_not_called()
        keyboard.assert_not_called()
    assert len(seen) == 2 * NUM_PADS
    output.phase, output.can_perform, output.busy = "ready", True, False
    # Runtime and exclusive access remain independent rendering fences.
    output.exclusive_native_access = True
    output.phase = "startup_restoring"
    _frame(ctx)
    main.assert_not_called()
    output.exclusive_native_access, output.has_runtime = False, False
    _frame(ctx)
    main.assert_not_called()
    keyboard.assert_not_called()
    output.has_runtime = True
    output.exclusive_native_access, output.phase = False, "ready"
    _frame(ctx)
    main.assert_called_once_with(ctx)
    keyboard.assert_called_once_with(ctx)
    assert not ctx.state.session.settings_open


def test_manual_settings_navigation_survives_restore_without_auto_open(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = _context()
    original = imgui.button
    click = "Open settings"

    def button(label: str, size: imgui.ImVec2Like = (0, 0)) -> bool:
        actual = original(label, size)
        return label == click or actual

    monkeypatch.setattr(imgui, "button", button)
    _frame(ctx)
    assert ctx.state.session.settings_open
    click = "Back to PADs"
    _frame(ctx)
    assert not ctx.state.session.settings_open
    click = "Open settings"
    _frame(ctx)
    output = ctx.ui.settings.audio_output
    output.phase, output.has_runtime, output.can_perform = "ready", True, True
    output.exclusive_native_access, output.busy = False, False
    main = Mock()
    monkeypatch.setattr(render, "_main", main)
    monkeypatch.setattr(render, "_poll_keyboard_input", Mock())
    monkeypatch.setattr(render, "performance_confirmation", Mock())
    _frame(ctx)
    assert ctx.state.session.settings_open
    main.assert_called_once_with(ctx)


def test_ready_settings_center_uses_the_normal_surface(monkeypatch: pytest.MonkeyPatch) -> None:
    ctx = _context()
    ctx.ui.settings.open()
    settings = Mock()
    monkeypatch.setattr(render, "settings_overlay", settings)
    monkeypatch.setattr(render, "bottom_bar_layout", lambda _width: SimpleNamespace(height=36))
    monkeypatch.setattr(render, "bottom_bar", Mock())
    imgui.new_frame()
    imgui.begin("ready Settings", flags=imgui.WindowFlags_.no_saved_settings)
    render._center_area(cast("UiContext", ctx))
    imgui.end()
    imgui.render()
    settings.assert_called_once_with(ctx)


@pytest.mark.parametrize("phase", ["startup_failed", "unavailable", "device_failed"])
def test_real_startup_errors_show_corrective_settings_and_keep_input_fenced(
    monkeypatch: pytest.MonkeyPatch,
    phase: str,
) -> None:
    ctx = _context()
    output = ctx.ui.settings.audio_output
    output.phase, output.error = phase, "Original driver or source restoration error"
    output.exclusive_native_access, output.busy = False, False
    main, keyboard = Mock(), Mock()
    monkeypatch.setattr(render, "_main", main)
    monkeypatch.setattr(render, "_poll_keyboard_input", keyboard)
    observed: list[str] = []
    original = imgui.text_wrapped

    def text(message: str) -> None:
        observed.append(message)
        original(message)

    monkeypatch.setattr(imgui, "text_wrapped", text)
    _frame(ctx)
    assert ctx.state.session.settings_open
    assert output.error in observed
    main.assert_not_called()
    keyboard.assert_not_called()
