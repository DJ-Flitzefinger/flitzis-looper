from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.constants import SPEED_MAX, SPEED_MIN
from flitzis_looper.ui.constants import DARK_RGBA, SPACING, TEXT_BPM_RGBA
from flitzis_looper.ui.contextmanager import button_style, style_var

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper.ui.styles import ButtonStyleName


def _bpm_display(ctx: UiContext) -> None:
    bpm_text = "104.54"  # TODO: use real value
    pos = imgui.get_cursor_screen_pos()
    avail = imgui.get_content_region_avail()
    draw_list = imgui.get_window_draw_list()
    bpm_height = 40
    draw_list.add_rect_filled(
        pos, (pos.x + avail.x, pos.y + bpm_height), imgui.get_color_u32(DARK_RGBA)
    )
    imgui.push_font(None, 38)
    text_size = imgui.calc_text_size(bpm_text)
    x = pos.x + (avail.x - text_size.x) / 2
    y = pos.y + (bpm_height - text_size.y) / 2
    draw_list.add_text((x, y), imgui.color_convert_float4_to_u32(TEXT_BPM_RGBA), bpm_text)
    imgui.pop_font()

    # Advance cursor to reserve space
    imgui.dummy((avail.x, bpm_height))


def _speed_controls(ctx: UiContext) -> None:
    avail = imgui.get_content_region_avail()

    _bpm_display(ctx)

    slider_width = 42
    slider_height = 400
    x = (avail.x - slider_width) / 2
    imgui.set_cursor_pos_x(x)
    changed, new_value = imgui.v_slider_float(
        "##speed_slider",
        (slider_width, slider_height),
        ctx.state.project.speed,
        SPEED_MIN,
        SPEED_MAX,
        format="%.2f",
    )
    if changed:
        ctx.audio.set_speed(new_value)

    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
        if imgui.button("+", (avail.x, 0)):
            ctx.audio.increase_speed()
        if imgui.button("Reset", (avail.x, 0)):
            ctx.audio.reset_speed()
        if imgui.button("-", (avail.x, 0)):
            ctx.audio.decrease_speed()


def sidebar_right(ctx: UiContext) -> None:
    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING)):
        key_lock = ctx.state.project.key_lock
        bpm_lock = ctx.state.project.bpm_lock

        _speed_controls(ctx)

        imgui.dummy(size=(-1, SPACING))

        with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
            key_lock_style: ButtonStyleName = "mode-on" if key_lock else "mode-off"
            with button_style(key_lock_style):
                if imgui.button("KEY LOCK", (-1, 0)):
                    ctx.audio.toggle_key_lock()

            bpm_lock_style: ButtonStyleName = "mode-on" if bpm_lock else "mode-off"
            with button_style(bpm_lock_style):
                if imgui.button("BPM LOCK", (-1, 0)):
                    ctx.audio.toggle_bpm_lock()
