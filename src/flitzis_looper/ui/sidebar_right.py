from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.ui.constants import (
    DARK_RGBA,
    SPACING,
    SPEED_MAX,
    SPEED_MIN,
    SPEED_STEP,
    TEXT_BPM_RGBA,
)
from flitzis_looper.ui.context import button_style, style_var

if TYPE_CHECKING:
    from flitzis_looper.app import FlitzisLooperApp
    from flitzis_looper.ui.styles import ButtonStyleName


def _bpm_display(app: FlitzisLooperApp) -> None:
    bpm_text = "104.54"  # dummy value
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


def _speed_controls(app: FlitzisLooperApp) -> None:
    avail = imgui.get_content_region_avail()

    _bpm_display(app)

    slider_width = 42
    slider_height = 400
    x = (avail.x - slider_width) / 2
    imgui.set_cursor_pos_x(x)
    changed, new_value = imgui.v_slider_float(
        "##speed_slider",
        (slider_width, slider_height),
        app.state.speed,
        SPEED_MIN,
        SPEED_MAX,
        format="%.2f",
    )
    if changed:
        app.set_speed(new_value)

    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
        if imgui.button("+", (avail.x, 0)):
            app.set_speed(app.state.speed + SPEED_STEP)
        if imgui.button("Reset", (avail.x, 0)):
            app.reset_speed()
        if imgui.button("-", (avail.x, 0)):
            app.set_speed(app.state.speed - SPEED_STEP)


def sidebar_right(app: FlitzisLooperApp) -> None:
    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING)):
        _speed_controls(app)

        imgui.dummy(size=(-1, SPACING))

        with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
            key_lock_style: ButtonStyleName = "mode-on" if app.state.key_lock else "mode-off"
            with button_style(key_lock_style):
                if imgui.button("KEY LOCK", (-1, 0)):
                    app.toggle_key_lock()

            bpm_lock_style: ButtonStyleName = "mode-on" if app.state.bpm_lock else "mode-off"
            with button_style(bpm_lock_style):
                if imgui.button("BPM LOCK", (-1, 0)):
                    app.toggle_bpm_lock()
