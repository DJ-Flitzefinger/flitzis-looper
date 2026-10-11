"""Render performer warnings outside the selected-pad and settings surfaces."""

from pathlib import Path, PureWindowsPath
from typing import TYPE_CHECKING

from imgui_bundle import imgui

if TYPE_CHECKING:
    from flitzis_looper.controller.performance_confirmation import PerformanceConfirmation
    from flitzis_looper.ui.context import UiContext

_POPUP = "Confirm Audio Action##performance-confirmation"


def _render_warning_body(ctx: UiContext, intent: PerformanceConfirmation) -> None:
    label = PureWindowsPath(intent.path).name if "\\" in intent.path else Path(intent.path).name
    imgui.text_wrapped(f"Pad {intent.pad_id + 1}: {label}")
    imgui.text_wrapped(intent.path)
    imgui.separator()
    if intent.action == "unload":
        imgui.text_wrapped(
            "Unload this audio? Playback will stop and this pad's audio, loop, and analysis "
            "settings will be cleared."
        )
        accept_label = "UNLOAD AUDIO"
    else:
        imgui.text_wrapped(
            "Analyze this audio again? Its detected BPM, key, and beat grid will be replaced "
            "when analysis completes. The previous key correction is cleared when admitted. "
            "Manual BPM, grid offset, and loop settings are preserved."
        )
        accept_label = "ANALYZE AUDIO"
    valid = ctx.ui.confirmation.is_current(intent)
    if intent.error is not None:
        imgui.text_wrapped(f"Action failed: {intent.error}")
    elif not valid:
        imgui.text_wrapped("This content changed or is no longer eligible. Cancel and try again.")
    imgui.spacing()
    control_height = imgui.get_frame_height()
    imgui.begin_disabled(not valid)
    accepted = imgui.button(accept_label, (-1, control_height))
    imgui.end_disabled()
    if accepted:
        ctx.ui.confirmation.accept(intent)
        if ctx.ui.confirmation.pending is None:
            imgui.close_current_popup()
    elif imgui.button("CANCEL", (-1, control_height)):
        ctx.ui.confirmation.dismiss(intent)
        imgui.close_current_popup()


def performance_confirmation(ctx: UiContext) -> None:
    """Display one captured pad/source warning with explicit accept/cancel controls."""
    intent = ctx.ui.confirmation.pending
    if intent is None:
        return
    if ctx.ui.confirmation.take_open_request(intent):
        imgui.open_popup(_POPUP)

    viewport = imgui.get_main_viewport()
    imgui.set_next_window_pos(viewport.get_center(), imgui.Cond_.appearing, (0.5, 0.5))
    font_size = imgui.get_font_size()
    width = min(34 * font_size, max(1, viewport.work_size.x - 2 * font_size))
    imgui.set_next_window_size((width, 0), imgui.Cond_.always)
    visible, opened = imgui.begin_popup_modal(
        _POPUP, p_open=True, flags=imgui.WindowFlags_.always_auto_resize
    )
    if not opened or not visible:
        ctx.ui.confirmation.dismiss(intent)
        if visible:
            imgui.end_popup()
        return
    try:
        if imgui.is_key_pressed(imgui.Key.escape, repeat=False):
            ctx.ui.confirmation.dismiss(intent)
            imgui.close_current_popup()
            return
        _render_warning_body(ctx, intent)
    finally:
        imgui.end_popup()
