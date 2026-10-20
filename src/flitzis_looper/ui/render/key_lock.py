"""Shared KEY LOCK intent buttons and separate confirmed processing status."""

from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.ui.constants import TEXT_MUTED_RGBA
from flitzis_looper.ui.contextmanager import button_style

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.transport.key_lock_status import KeyLockStatus
    from flitzis_looper.ui.styles import ButtonStyleName


def render_key_lock_button(
    status: KeyLockStatus,
    suffix: str,
    toggle: Callable[[], None],
    *,
    compact_status: bool = False,
) -> None:
    """Draw the global default and retain toggles while a change is preparing."""
    style = _intent_style(value=status.requested)
    with button_style(style):
        if imgui.button(f"KEY LOCK##{suffix}", (-1, 0)):
            toggle()
    if compact_status:
        if imgui.is_item_hovered():
            effective = _effective_description(status)
            details = [f"Requested {'ON' if status.requested else 'OFF'}", f"Audio {effective}"]
            if status.unconfirmed:
                details.append("Change unconfirmed")
            elif status.pending:
                details.append(f"Preparing {'ON' if status.requested else 'OFF'}…")
            if status.mixed:
                details.append("Mixed pads")
            imgui.set_tooltip("\n".join(details))
        effective = "?" if status.effective is None else "ON" if status.effective else "OFF"
        imgui.text_colored(TEXT_MUTED_RGBA, f"Audio {effective}")
    else:
        render_key_lock_status(status)


def render_pad_key_lock_buttons(
    status: KeyLockStatus,
    *,
    override: bool | None,
    global_enabled: bool,
    set_override: Callable[[bool], None],
    clear_override: Callable[[], None],
) -> None:
    """Draw independent pad intent and the adjacent return-to-global gesture."""
    width = imgui.get_content_region_avail().x
    spacing = imgui.get_style().item_spacing.x
    global_width = imgui.calc_text_size("GLOBAL").x + 2 * imgui.get_style().frame_padding.x
    with button_style(_intent_style(value=override)):
        if imgui.button("KEY LOCK PAD##pad_key_lock", (width - global_width - spacing, 0)):
            set_override(True)
        if imgui.is_item_hovered() and imgui.is_mouse_clicked(imgui.MouseButton_.right):
            set_override(False)
    imgui.same_line()
    with button_style(_intent_style(value=global_enabled if override is None else None)):
        if imgui.button("GLOBAL##pad_key_lock_global", (global_width, 0)):
            clear_override()
    render_key_lock_status(status)


def _intent_style(*, value: bool | None) -> ButtonStyleName:
    return "regular" if value is None else "mode-on" if value else "mode-off"


def render_key_lock_status(status: KeyLockStatus) -> None:
    """Keep preparation and confirmed audio independent of intent colors."""
    imgui.text_colored(TEXT_MUTED_RGBA, f"Audio {_effective_description(status)}")
    imgui.push_text_wrap_pos(0.0)
    try:
        if status.unconfirmed:
            imgui.text_colored(TEXT_MUTED_RGBA, "Change unconfirmed")
        elif status.pending:
            imgui.text_colored(TEXT_MUTED_RGBA, f"Preparing {'ON' if status.requested else 'OFF'}…")
        if status.mixed:
            imgui.text_colored(TEXT_MUTED_RGBA, "Mixed pads")
        elif status.effective is None and not status.pending and not status.error:
            imgui.text_colored(TEXT_MUTED_RGBA, "Mode unconfirmed")
    finally:
        imgui.pop_text_wrap_pos()


def _effective_description(status: KeyLockStatus) -> str:
    return "unconfirmed" if status.effective is None else "ON" if status.effective else "OFF"
