from typing import TYPE_CHECKING

from imgui_bundle import icons_fontawesome_6, imgui

from flitzis_looper.constants import (
    MAX_DEMUCS_OVERLAP,
    MAX_DEMUCS_SHIFTS,
    MIN_DEMUCS_OVERLAP,
    MIN_DEMUCS_SHIFTS,
)
from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA
from flitzis_looper.ui.contextmanager import button_style, item_width, style_var

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext

SETTINGS_TOGGLE_BUTTON_SIZE = 36.0


def settings_surface_child_id(*, settings_open: bool) -> str:
    """Return the main surface child ID for tests and rendering."""
    return "settings_overlay" if settings_open else "looper_main"


def settings_toggle_button_label(*, settings_open: bool) -> str:
    """Return the icon label for the bottom-right Settings toggle."""
    icon = (
        icons_fontawesome_6.ICON_FA_XMARK
        if settings_open
        else icons_fontawesome_6.ICON_FA_GEAR
    )
    return f"{icon}##settings_toggle"


def settings_toggle_tooltip(*, settings_open: bool) -> str:
    """Return the tooltip for the Settings toggle."""
    return "Close settings" if settings_open else "Open settings"


def settings_toggle_button(ctx: UiContext) -> None:
    """Render the Settings toggle at the caller-provided cursor position."""
    with button_style("regular"):
        if imgui.button(
            settings_toggle_button_label(settings_open=ctx.state.session.settings_open),
            (SETTINGS_TOGGLE_BUTTON_SIZE, SETTINGS_TOGGLE_BUTTON_SIZE),
        ):
            ctx.ui.settings.toggle()

    if imgui.is_item_hovered():
        imgui.set_tooltip(settings_toggle_tooltip(settings_open=ctx.state.session.settings_open))


def settings_overlay(ctx: UiContext) -> None:
    """Render the Settings page in place of the main Looper surface."""
    with style_var(imgui.StyleVar_.item_spacing, (SPACING, SPACING)):
        imgui.text_unformatted("Settings")
        imgui.separator()
        imgui.text_colored(TEXT_MUTED_RGBA, "Stem Quality")
        _demucs_quality_controls(ctx)


def _demucs_quality_controls(ctx: UiContext) -> None:
    shifts = int(ctx.state.project.demucs_shifts)
    overlap = float(ctx.state.project.demucs_overlap)

    with item_width(240):
        changed, new_shifts = imgui.slider_int(
            "Demucs shifts",
            shifts,
            MIN_DEMUCS_SHIFTS,
            MAX_DEMUCS_SHIFTS,
        )
    if changed:
        shifts = int(new_shifts)
        ctx.ui.settings.set_demucs_quality(shifts=shifts, overlap=overlap)
    _flush_settings_edit_if_completed(ctx)

    with item_width(240):
        changed, new_overlap = imgui.slider_float(
            "Demucs overlap",
            overlap,
            MIN_DEMUCS_OVERLAP,
            MAX_DEMUCS_OVERLAP,
            "%.2f",
        )
    if changed:
        ctx.ui.settings.set_demucs_quality(shifts=shifts, overlap=float(new_overlap))
    _flush_settings_edit_if_completed(ctx)


def _flush_settings_edit_if_completed(ctx: UiContext) -> None:
    if imgui.is_item_deactivated_after_edit():
        ctx.persistence.flush_if_dirty()
