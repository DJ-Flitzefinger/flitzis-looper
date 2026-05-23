from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.ui.constants import SPACING
from flitzis_looper.ui.contextmanager import button_style, item_width, style_var

if TYPE_CHECKING:
    from flitzis_looper.models import TriggerQuantizationMode
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper.ui.styles import ButtonStyleName

TRIGGER_QUANTIZATION_OPTIONS: tuple[tuple[TriggerQuantizationMode, str], ...] = (
    ("immediate", "IMMEDIATE"),
    ("next_beat", "BEAT"),
    ("next_bar", "BAR"),
)


def _master_volume(ctx: UiContext) -> None:
    with style_var(imgui.StyleVar_.item_spacing, (0, SPACING / 2)):
        val = max(0, min(100, round(ctx.state.project.volume * 100)))
        imgui.text_unformatted("Master Volume")

        with item_width(240):
            changed, new_value = imgui.slider_int("##master_volume", val, 0, 100, "%d %")
            if changed:
                ctx.audio.global_.set_volume(new_value / 100.0)


def _trigger_quantization(ctx: UiContext) -> None:
    imgui.text_unformatted("Trigger Quantize")
    imgui.same_line(spacing=SPACING / 2)
    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
        for idx, (mode, label) in enumerate(TRIGGER_QUANTIZATION_OPTIONS):
            if idx > 0:
                imgui.same_line(spacing=0.0)

            style: ButtonStyleName = (
                "mode-on" if ctx.state.project.trigger_quantization == mode else "mode-off"
            )
            with button_style(style):
                if imgui.button(f"{label}##trigger_quantization_{mode}", (96, 0)):
                    ctx.audio.global_.set_trigger_quantization(mode)


def bottom_bar(ctx: UiContext) -> None:
    _master_volume(ctx)
    imgui.same_line(spacing=SPACING)
    style: ButtonStyleName = "mode-on" if ctx.state.project.multi_loop else "mode-off"

    with button_style(style):
        if imgui.button("MULTI LOOP"):
            ctx.audio.global_.toggle_multi_loop()

    imgui.same_line(spacing=SPACING)
    _trigger_quantization(ctx)
