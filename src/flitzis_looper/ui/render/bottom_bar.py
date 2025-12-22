from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.ui.constants import SPACING
from flitzis_looper.ui.contextmanager import button_style, item_width, style_var

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper.ui.styles import ButtonStyleName


def _master_volume(ctx: UiContext) -> None:
    with style_var(imgui.StyleVar_.item_spacing, (0, SPACING / 2)):
        val = max(0, min(100, round(ctx.state.project.volume * 100)))
        imgui.text_unformatted("Master Volume")

        with item_width(240):
            changed, new_value = imgui.slider_int("##master_volume", val, 0, 100, "%d %")
            if changed:
                ctx.audio.set_volume(new_value / 100.0)


def bottom_bar(ctx: UiContext) -> None:
    _master_volume(ctx)
    imgui.same_line(spacing=SPACING)
    style: ButtonStyleName = "mode-on" if ctx.state.project.multi_loop else "mode-off"

    with button_style(style):
        if imgui.button("MULTI LOOP"):
            ctx.audio.toggle_multi_loop()
