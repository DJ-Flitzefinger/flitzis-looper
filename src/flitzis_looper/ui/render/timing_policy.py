"""Global source timing choices with separate actual audio confirmation."""

from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.ui.constants import TEXT_MUTED_RGBA
from flitzis_looper.ui.contextmanager import button_style

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.transport.timing_policy import TimingControl
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper.ui.styles import ButtonStyleName


def render_global_timing_policy(ctx: UiContext) -> None:
    controls: tuple[tuple[TimingControl, str, Callable[[], None]], ...] = (
        ("realtime_bpm", "REALTIME BPM", ctx.audio.global_.toggle_realtime_bpm),
        ("sync", "SYNC", ctx.audio.global_.toggle_source_sync),
    )
    for control, label, action in controls:
        status = ctx.state.global_.timing_policy_status(control)
        style: ButtonStyleName = (
            "regular"
            if status.preference is None
            else "mode-on"
            if status.preference
            else "mode-off"
        )
        with button_style(style):
            if imgui.button(f"{label}##global-{control}", (-1, 0)):
                action()
        if imgui.is_item_hovered():
            choice = (
                "legacy preferences"
                if status.requested is None
                else "ON"
                if status.requested
                else "OFF"
            )
            master = (
                "\nUses the current master tempo; establish a BPM LOCK reference first."
                if control == "sync"
                else ""
            )
            guidance = f"\n{status.detail}" if status.detail is not None else ""
            imgui.set_tooltip(f"Requested {choice}.{guidance}{master}")
        actual = (
            "mixed/unconfirmed" if status.effective is None else "ON" if status.effective else "OFF"
        )
        suffix = " | pending" if status.pending else ""
        prefix = "Applied" if control == "realtime_bpm" else "Audio"
        imgui.text_colored(TEXT_MUTED_RGBA, f"{prefix} {actual}{suffix}")
        if status.detail is not None:
            imgui.text_wrapped(status.detail)
