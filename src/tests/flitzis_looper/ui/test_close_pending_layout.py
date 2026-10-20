"""Actual headless close/readiness layout keeps technical diagnostics out of the UI."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.ui import run
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import waveform_editor
from tests.flitzis_looper.ui.test_waveform_render_stability import _plot_context

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller import AppController


@pytest.mark.parametrize("width", [360, 600, 1200])
@pytest.mark.parametrize("scale", [1.0, 1.5, 2.0])
def test_close_popup_is_compact_and_inside_viewport_with_real_fonts(
    monkeypatch: pytest.MonkeyPatch, width: int, scale: float
) -> None:
    with _plot_context():
        io = imgui.get_io()
        io.display_size = imgui.ImVec2(width, 500)
        imgui.get_style().font_scale_main = scale
        observed: list[tuple[float, float, float, float, float]] = []
        texts: list[str] = []
        end_popup = imgui.end_popup
        text_wrapped = imgui.text_wrapped

        def finish_popup() -> None:
            position, size = imgui.get_window_pos(), imgui.get_window_size()
            observed.append((position.x, position.y, size.x, size.y, imgui.get_font_size()))
            end_popup()

        def draw_text(value: str) -> None:
            texts.append(value)
            text_wrapped(value)

        monkeypatch.setattr(imgui, "end_popup", finish_popup)
        monkeypatch.setattr(imgui, "text_wrapped", draw_text)
        technical_error = (
            "Native source owner epoch mismatch; immutable publication unconfirmed. " * 80
        )
        for error in [None] * 4 + [technical_error] * 4:
            imgui.new_frame()
            imgui.set_next_window_pos((0, 0))
            imgui.set_next_window_size(io.display_size)
            imgui.begin("close host", flags=imgui.WindowFlags_.no_saved_settings)
            run._show_close_pending(error)
            imgui.end()
            imgui.render()
        assert observed
        left, top, popup_width, popup_height, font_size = observed[-1]
        assert 0 <= left < left + popup_width <= width
        assert 0 <= top < top + popup_height <= 500
        assert popup_width >= imgui.calc_text_size("Finishing pending work").x
        assert popup_height <= 6 * font_size
        assert observed[3][:4] == observed[-1][:4]
        assert all("epoch mismatch" not in value for value in texts)
        assert all("publication unconfirmed" not in value for value in texts)


def test_waveform_error_rows_keep_retry_and_cancel_without_inline_errors(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    context = UiContext(controller)
    readiness = Mock(return_value=("error", "waveform native reader failure"))
    residency = Mock(return_value=("error", "resident source epoch failure"))
    retry, cancel = Mock(), Mock()
    monkeypatch.setattr(context.ui.waveform, "readiness", readiness)
    monkeypatch.setattr(context.audio.pads, "residency_status", residency)
    monkeypatch.setattr(context.ui.waveform, "retry", retry)
    monkeypatch.setattr(context.audio.pads, "cancel_residency", cancel)
    rows: list[tuple[str | None, str | None]] = []
    buttons: list[str] = []
    cursor_positions: list[float] = []
    original_row = waveform_editor._render_readiness_row
    click: str | None = None

    def render_row(
        row_id: str,
        label: str | None,
        *,
        action_label: str | None = None,
        action: Callable[[], None] | None = None,
    ) -> None:
        rows.append((label, action_label))
        original_row(row_id, label, action_label=action_label, action=action)

    monkeypatch.setattr(waveform_editor, "_render_readiness_row", render_row)
    with _plot_context():
        original_button = imgui.button

        def button(label: str, size: tuple[float, float]) -> bool:
            buttons.append(label)
            original_button(label, size)
            return label == click

        monkeypatch.setattr(imgui, "button", button)

        def frame() -> None:
            imgui.new_frame()
            imgui.set_next_window_pos((0, 0))
            imgui.set_next_window_size((600, 400))
            imgui.begin("readiness host", flags=imgui.WindowFlags_.no_saved_settings)
            waveform_editor._render_context_readiness(context, 0)
            cursor_positions.append(imgui.get_cursor_pos_y())
            imgui.end()
            imgui.render()

        frame()
        frame()
        assert rows[-2:] == [(None, "Retry##waveform-0"), (None, None)]
        assert "Retry##waveform-0" in buttons
        click = "Retry##waveform-0"
        frame()
        retry.assert_called_once_with(0)
        readiness.return_value = ("pending", None)
        residency.return_value = ("pending", None)
        click = "Cancel##resident-context-0"
        frame()
        cancel.assert_called_once_with(0)
        assert rows[-2:] == [
            ("Preparing source waveform...", None),
            ("Preparing playback context...", "Cancel##resident-context-0"),
        ]
        assert max(cursor_positions) == min(cursor_positions)
