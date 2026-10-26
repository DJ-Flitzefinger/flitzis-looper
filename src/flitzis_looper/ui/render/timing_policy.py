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
        ("sync", "SYNC", ctx.audio.global_.toggle_source_sync),
        ("realtime_bpm", "REALTIME BPM", ctx.audio.global_.toggle_realtime_bpm),
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
        imgui.begin_disabled(not status.available)
        with button_style(style):
            if imgui.button(f"{label}##global-{control}", (-1, 0)):
                action()
        imgui.end_disabled()
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
        prefix = "Applied" if control == "realtime_bpm" else "Mode"
        if control == "sync" and status.effective is True and status.grid_phase_confirmed is None:
            suffix += " | grid phase unconfirmed"
        imgui.text_colored(TEXT_MUTED_RGBA, f"{prefix} {actual}{suffix}")
        if status.detail is not None:
            imgui.text_wrapped(status.detail)

    _render_timing_trace(ctx)


def _render_timing_trace(ctx: UiContext) -> None:
    trace = ctx.state.global_.timing_trace()
    imgui.begin_disabled(trace.finishing or not trace.supported)
    if imgui.button("Stop Timing Log" if trace.active else "Start Timing Log", (-1, 0)):
        if trace.active:
            ctx.ui.settings.stop_timing_trace()
        else:
            ctx.ui.settings.start_timing_trace()
    imgui.end_disabled()
    if trace.active:
        imgui.text(f"Timing log: {trace.elapsed}s / 600s")
        if imgui.button("Heard Drift##timing-log", (-1, 0)):
            ctx.ui.settings.mark_timing_drift()
    if not trace.supported:
        imgui.text_wrapped("Timing logging needs the updated Native engine after restart.")
    if trace.finishing:
        imgui.text("Finishing timing log...")
    if trace.path is not None:
        imgui.text_wrapped(str(trace.path))
    if trace.error is not None:
        imgui.text_wrapped(trace.error)
