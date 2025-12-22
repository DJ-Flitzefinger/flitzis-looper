from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.ui.constants import SPACING
from flitzis_looper.ui.context import button_style, style_var

if TYPE_CHECKING:
    from flitzis_looper.app import FlitzisLooperApp
    from flitzis_looper.ui.styles import ButtonStyleName


def _master_volume(app: FlitzisLooperApp) -> None:
    with style_var(imgui.StyleVar_.item_spacing, (0, SPACING / 2)):
        val = max(0, min(100, round(app.state.volume * 100)))
        imgui.text_unformatted("Master Volume")
        imgui.push_item_width(240)
        changed, new_value = imgui.slider_int("##master_volume", val, 0, 100, "%d %")
        if changed:
            app.set_volume(new_value / 100.0)
        imgui.pop_item_width()


def bottom_bar(app: FlitzisLooperApp) -> None:
    _master_volume(app)
    imgui.same_line(spacing=SPACING)
    style: ButtonStyleName = "mode-on" if app.state.multi_loop else "mode-off"
    with button_style(style):
        if imgui.button("MULTI LOOP"):
            app.toggle_multi_loop()
