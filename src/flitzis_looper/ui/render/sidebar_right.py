import math
from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.constants import PITCH_BPM_COARSE_STEPS, PITCH_BPM_STEP, SPEED_MAX, SPEED_MIN
from flitzis_looper.ui.constants import (
    CONTROL_ACTIVE_BORDER_RGBA,
    CONTROL_BORDER_RGBA,
    DARK_RGBA,
    SPACING,
    TEXT_BPM_RGBA,
)
from flitzis_looper.ui.contextmanager import button_style, style_var
from flitzis_looper.ui.render.control_gestures import (
    active_left_button_repeat_count,
    hovered_button_repeat_count,
    hovered_wheel_steps,
    item_middle_clicked,
)

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper.ui.styles import ButtonStyleName


def _has_pending_learn_input(ctx: UiContext) -> bool:
    return (
        ctx.state.project.input_mapping_enabled
        and ctx.state.session.input_learn_pending_binding_key is not None
    )


def sanitize_bpm_entry_text(text: str) -> str:
    sanitized: list[str] = []
    has_decimal = False
    decimal_places = 0

    for char in text.replace(",", "."):
        if char.isdigit():
            if has_decimal:
                if decimal_places >= 2:
                    continue
                decimal_places += 1
            sanitized.append(char)
        elif char == "." and not has_decimal:
            has_decimal = True
            sanitized.append(char)

    return "".join(sanitized)


def filtered_bpm_entry_char(
    char_code: int,
    current_text: str,
    cursor_pos: int,
    *,
    has_selection: bool,
) -> int | None:
    """Return a replacement char code, or None when BPM entry must reject it."""
    if char_code == 0:
        return 0

    accepted: int | None = None
    char = chr(char_code)
    if char == ",":
        char = "."
        char_code = ord(".")

    if char.isdigit():
        if "." not in current_text or has_selection:
            accepted = char_code
        else:
            decimals = current_text.split(".", 1)[1]
            if cursor_pos <= current_text.index(".") or len(decimals) < 2:
                accepted = char_code
    elif char == "." and ("." not in current_text or has_selection):
        accepted = char_code

    return accepted


def bpm_entry_char_filter(data: imgui.InputTextCallbackData) -> int:
    """Reject BPM entry characters before they enter the input buffer."""
    replacement = filtered_bpm_entry_char(
        int(data.event_char),
        str(data.buf),
        int(data.cursor_pos),
        has_selection=data.has_selection(),
    )
    if replacement == 0:
        return 0
    if replacement is not None:
        data.event_char = replacement
        return 0
    return 1


def parse_bpm_entry_text(text: str) -> float | None:
    sanitized = sanitize_bpm_entry_text(text)
    if sanitized in {"", "."}:
        return None
    value = float(sanitized)
    if not math.isfinite(value) or value <= 0.0:
        return None
    return round(value, 2)


def snap_bpm_to_grid(bpm: float) -> float:
    return round(math.floor((float(bpm) / PITCH_BPM_STEP) + 0.5) * PITCH_BPM_STEP, 1)


def pitch_wheel_bpm_steps(wheel_steps: int) -> int:
    """Return 0.1-BPM controller steps for hovered Pitch wheel movement."""
    return wheel_steps * PITCH_BPM_COARSE_STEPS


def pitch_center_indicator_y(item_min_y: float, item_max_y: float) -> float:
    """Return the neutral marker y-position aligned to ImGui's slider grab center."""
    slider_height = item_max_y - item_min_y
    if slider_height <= 0.0:
        return item_min_y

    style = imgui.get_style()
    grab_padding = 2.0
    grab_half = float(style.grab_min_size) * 0.5
    usable_min_y = item_min_y + grab_padding + grab_half
    usable_max_y = item_max_y - grab_padding - grab_half
    neutral_fraction = (1.0 - SPEED_MIN) / (SPEED_MAX - SPEED_MIN)
    return usable_max_y - (usable_max_y - usable_min_y) * neutral_fraction


def pitch_center_indicator_color(*, speed: float) -> imgui.ImVec4Like:
    """Return the marker color for the current Pitch state."""
    if math.isclose(float(speed), 1.0, rel_tol=0.0, abs_tol=0.0005):
        return CONTROL_ACTIVE_BORDER_RGBA
    return CONTROL_BORDER_RGBA


def _draw_pitch_center_indicator(*, speed: float) -> None:
    item_min = imgui.get_item_rect_min()
    item_max = imgui.get_item_rect_max()
    neutral_y = pitch_center_indicator_y(item_min.y, item_max.y)
    line_start = (item_min.x - 22.0, neutral_y)
    line_end = (item_min.x - 7.0, neutral_y)
    color = pitch_center_indicator_color(speed=speed)

    draw_list = imgui.get_window_draw_list()
    draw_list.add_line(line_start, line_end, imgui.get_color_u32(DARK_RGBA), 4.0)
    draw_list.add_line(line_start, line_end, imgui.get_color_u32(color), 2.0)


def _render_bpm_entry(
    ctx: UiContext,
    pos: imgui.ImVec2,
    local_pos: imgui.ImVec2,
    avail: imgui.ImVec2,
    bpm_height: float,
) -> None:
    imgui.set_cursor_screen_pos((pos.x + 6.0, pos.y + 5.0))
    imgui.set_next_item_width(max(1.0, avail.x - 12.0))
    flags = (
        imgui.InputTextFlags_.enter_returns_true
        | imgui.InputTextFlags_.auto_select_all
        | imgui.InputTextFlags_.callback_char_filter
    )
    if ctx.state.session.global_bpm_edit_focus_requested:
        imgui.set_keyboard_focus_here()
        ctx.ui.clear_global_bpm_edit_focus_request()

    submitted, new_text = imgui.input_text(
        "##global_bpm_edit",
        ctx.state.session.global_bpm_edit_text,
        flags,
        bpm_entry_char_filter,
    )
    sanitized = sanitize_bpm_entry_text(new_text)
    if sanitized != ctx.state.session.global_bpm_edit_text:
        ctx.ui.set_global_bpm_edit_text(sanitized)

    commit = submitted or imgui.is_item_deactivated_after_edit()
    close = submitted or imgui.is_item_deactivated()
    if commit:
        target_bpm = parse_bpm_entry_text(sanitized)
        if target_bpm is not None:
            ctx.audio.global_.set_effective_bpm(target_bpm)
    if close:
        ctx.ui.finish_global_bpm_edit()

    imgui.set_cursor_pos((local_pos.x, local_pos.y + bpm_height))


def _bpm_display(ctx: UiContext) -> None:
    bpm = ctx.state.global_.effective_bpm()
    bpm_text = f"{bpm:.2f}" if bpm is not None else "--"
    pos = imgui.get_cursor_screen_pos()
    local_pos = imgui.get_cursor_pos()
    avail = imgui.get_content_region_avail()
    draw_list = imgui.get_window_draw_list()
    bpm_height = 40
    draw_list.add_rect_filled(
        pos, (pos.x + avail.x, pos.y + bpm_height), imgui.get_color_u32(DARK_RGBA)
    )

    if ctx.state.session.global_bpm_edit_active:
        _render_bpm_entry(ctx, pos, local_pos, avail, bpm_height)
        return

    imgui.invisible_button("##global_bpm_display", (avail.x, bpm_height))
    if (
        bpm is not None
        and imgui.is_item_hovered()
        and imgui.is_mouse_double_clicked(imgui.MouseButton_.left)
    ):
        ctx.ui.start_global_bpm_edit(bpm_text)

    imgui.push_font(None, 38)
    text_size = imgui.calc_text_size(bpm_text)
    x = pos.x + (avail.x - text_size.x) / 2
    y = pos.y + (bpm_height - text_size.y) / 2
    draw_list.add_text((x, y), imgui.color_convert_float4_to_u32(TEXT_BPM_RGBA), bpm_text)
    imgui.pop_font()


def _speed_controls(ctx: UiContext) -> None:
    avail = imgui.get_content_region_avail()

    _bpm_display(ctx)

    slider_width = 42
    slider_height = 400
    x = (avail.x - slider_width) / 2
    imgui.set_cursor_pos_x(x)
    reference_bpm = ctx.state.global_.speed_reference_bpm()
    current_bpm = ctx.state.global_.effective_bpm()
    learn_pending = _has_pending_learn_input(ctx)
    if reference_bpm is not None and current_bpm is not None:
        _speed_slider_with_bpm_reference(
            ctx,
            reference_bpm=float(reference_bpm),
            slider_width=slider_width,
            slider_height=slider_height,
            learn_pending=learn_pending,
        )
    else:
        _speed_slider_without_bpm_reference(
            ctx,
            slider_width=slider_width,
            slider_height=slider_height,
            learn_pending=learn_pending,
        )

    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
        plus_clicked, plus_steps = _pitch_step_button("+", avail.x, learn_pending=learn_pending)
        if plus_clicked:
            ctx.audio.global_.increase_speed()
        elif plus_steps:
            ctx.audio.global_.nudge_speed_by_bpm_steps(plus_steps)

        if imgui.button("Reset", (avail.x, 0)):
            ctx.audio.global_.reset_speed()

        minus_clicked, minus_steps = _pitch_step_button("-", avail.x, learn_pending=learn_pending)
        if minus_clicked:
            ctx.audio.global_.decrease_speed()
        elif minus_steps:
            ctx.audio.global_.nudge_speed_by_bpm_steps(-minus_steps)


def _speed_slider_with_bpm_reference(
    ctx: UiContext,
    *,
    reference_bpm: float,
    slider_width: float,
    slider_height: float,
    learn_pending: bool,
) -> None:
    changed, new_value = imgui.v_slider_float(
        "##speed_slider",
        (slider_width, slider_height),
        ctx.state.project.speed,
        SPEED_MIN,
        SPEED_MAX,
        format="%.2f",
    )
    _draw_pitch_center_indicator(speed=ctx.state.project.speed)

    learn_clicked = (
        learn_pending
        and imgui.is_item_hovered()
        and imgui.is_mouse_clicked(imgui.MouseButton_.left)
    )
    if changed or learn_clicked:
        speed = ctx.state.project.speed
        if changed:
            speed = snap_bpm_to_grid(float(new_value) * reference_bpm) / reference_bpm
        ctx.audio.global_.set_speed(float(speed))
    elif not learn_pending:
        _apply_pitch_hover_gestures(ctx)


def _speed_slider_without_bpm_reference(
    ctx: UiContext,
    *,
    slider_width: float,
    slider_height: float,
    learn_pending: bool,
) -> None:
    changed, new_value = imgui.v_slider_float(
        "##speed_slider",
        (slider_width, slider_height),
        ctx.state.project.speed,
        SPEED_MIN,
        SPEED_MAX,
        format="%.2f",
    )
    _draw_pitch_center_indicator(speed=ctx.state.project.speed)

    learn_clicked = (
        learn_pending
        and imgui.is_item_hovered()
        and imgui.is_mouse_clicked(imgui.MouseButton_.left)
    )
    if changed or learn_clicked:
        ctx.audio.global_.set_speed(float(new_value if changed else ctx.state.project.speed))
    elif not learn_pending:
        _apply_pitch_hover_gestures(ctx)


def _apply_pitch_hover_gestures(ctx: UiContext) -> None:
    if item_middle_clicked():
        ctx.audio.global_.reset_speed()
        return

    wheel_steps = hovered_wheel_steps()
    if wheel_steps:
        ctx.audio.global_.nudge_speed_by_bpm_steps(pitch_wheel_bpm_steps(wheel_steps))


def _pitch_step_button(label: str, width: float, *, learn_pending: bool) -> tuple[bool, int]:
    left_clicked = imgui.button(label, (width, 0))
    if left_clicked:
        return True, 0
    if not learn_pending:
        right_steps = 0
        if imgui.is_item_hovered() and imgui.is_mouse_clicked(imgui.MouseButton_.right):
            right_steps += PITCH_BPM_COARSE_STEPS
        right_repeats = hovered_button_repeat_count(imgui.MouseButton_.right)
        if right_repeats:
            right_steps += right_repeats * PITCH_BPM_COARSE_STEPS
        if right_steps:
            return False, right_steps

        repeat_count = active_left_button_repeat_count()
        if repeat_count:
            return False, repeat_count
    return False, 0


def sidebar_right(ctx: UiContext) -> None:
    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING)):
        key_lock = ctx.state.project.key_lock
        bpm_lock = ctx.state.project.bpm_lock

        _speed_controls(ctx)

        imgui.dummy(size=(-1, SPACING))

        with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
            key_lock_style: ButtonStyleName = "mode-on" if key_lock else "mode-off"
            with button_style(key_lock_style):
                if imgui.button("KEY LOCK##key_lock", (-1, 0)):
                    ctx.audio.global_.toggle_key_lock()

            bpm_lock_style: ButtonStyleName = "mode-on" if bpm_lock else "mode-off"
            with button_style(bpm_lock_style):
                if imgui.button("BPM LOCK##bpm_lock", (-1, 0)):
                    ctx.audio.global_.toggle_bpm_lock()
