"""Shared confirmed-mode button and performer-facing KEYLOCK status."""

from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.ui.constants import TEXT_MUTED_RGBA
from flitzis_looper.ui.contextmanager import button_style

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.transport.key_lock_status import KeyLockStatus
    from flitzis_looper.ui.styles import ButtonStyleName


def render_key_lock_button(status: KeyLockStatus, suffix: str, toggle: Callable[[], None]) -> None:
    """Draw the actual confirmed mode and retain toggles while a change is preparing."""
    style: ButtonStyleName = (
        "regular" if status.effective is None else "mode-on" if status.effective else "mode-off"
    )
    with button_style(style):
        if imgui.button(f"KEY LOCK##{suffix}", (-1, 0)):
            toggle()
    # The color continues to describe confirmed audio while these separate
    # descriptions make ongoing work, partial results and errors visible.
    if status.error:
        imgui.text_wrapped(f"Key Lock: {status.error}")
    if status.unconfirmed:
        imgui.text_colored(TEXT_MUTED_RGBA, "Change unconfirmed")
    elif status.pending:
        imgui.text_colored(TEXT_MUTED_RGBA, f"Preparing {'ON' if status.requested else 'OFF'}…")
    if status.mixed:
        imgui.text_colored(TEXT_MUTED_RGBA, "Mixed pads")
    elif status.effective is None and not status.pending and not status.error:
        imgui.text_colored(TEXT_MUTED_RGBA, "Mode unconfirmed")
