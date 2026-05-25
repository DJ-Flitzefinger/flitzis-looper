import math

from imgui_bundle import imgui

BUTTON_HOLD_REPEAT_DELAY_S = 0.35
BUTTON_HOLD_REPEAT_INTERVAL_S = 0.10


def wheel_step_count(mouse_wheel: float) -> int:
    """Return signed whole control steps for a mouse-wheel delta."""
    if mouse_wheel == 0.0 or not math.isfinite(mouse_wheel):
        return 0

    direction = 1 if mouse_wheel > 0.0 else -1
    return direction * max(1, int(abs(mouse_wheel)))


def hovered_wheel_steps() -> int:
    """Return signed mouse-wheel steps when the last item is hovered."""
    if not imgui.is_item_hovered():
        return 0
    return wheel_step_count(float(imgui.get_io().mouse_wheel))


def item_middle_clicked() -> bool:
    """Return whether the hovered last item received a middle mouse click."""
    return imgui.is_item_hovered() and imgui.is_mouse_clicked(imgui.MouseButton_.middle)


def held_repeat_count(
    current_duration_s: float,
    previous_duration_s: float,
    *,
    delay_s: float = BUTTON_HOLD_REPEAT_DELAY_S,
    interval_s: float = BUTTON_HOLD_REPEAT_INTERVAL_S,
) -> int:
    """Return repeat ticks crossed by a held button since the previous frame."""
    if current_duration_s < delay_s or interval_s <= 0.0:
        return 0

    current_ticks = math.floor((current_duration_s - delay_s) / interval_s) + 1
    previous_ticks = 0
    if previous_duration_s >= delay_s:
        previous_ticks = math.floor((previous_duration_s - delay_s) / interval_s) + 1
    return max(0, current_ticks - previous_ticks)


def active_left_button_repeat_count() -> int:
    """Return repeat ticks for the active last item while the left mouse button is held."""
    if not imgui.is_item_active():
        return 0

    io = imgui.get_io()
    return held_repeat_count(
        float(io.mouse_down_duration[imgui.MouseButton_.left]),
        float(io.mouse_down_duration_prev[imgui.MouseButton_.left]),
    )
