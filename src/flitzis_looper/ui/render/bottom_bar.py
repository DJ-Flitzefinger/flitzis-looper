from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.models import (
    STEM_COMPONENT_MASK,
    STEM_INSTRUMENTAL_PRESET_MASK,
    STEM_MASK_BASS,
    STEM_MASK_DRUMS,
    STEM_MASK_MELODY,
    STEM_MASK_VOCALS,
    StemMaskDisplayMode,
)
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
STEM_COMPONENT_BUTTONS = (
    ("V", STEM_MASK_VOCALS),
    ("D", STEM_MASK_DRUMS),
    ("M", STEM_MASK_MELODY),
    ("B", STEM_MASK_BASS),
)
STEM_PRESET_BUTTONS: tuple[tuple[str, int, StemMaskDisplayMode], ...] = (
    ("I", STEM_INSTRUMENTAL_PRESET_MASK, "instrumental"),
    ("A", STEM_COMPONENT_MASK, "all"),
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


def _stem_preset_is_active(mask: int, display_mode: StemMaskDisplayMode) -> bool:
    return (display_mode == "instrumental" and mask == STEM_INSTRUMENTAL_PRESET_MASK) or (
        display_mode == "all" and mask == STEM_COMPONENT_MASK
    )


def stem_button_is_active(
    label: str,
    button_mask: int,
    current_mask: int,
    display_mode: StemMaskDisplayMode,
) -> bool:
    if label == "I":
        return display_mode == "instrumental" and current_mask == button_mask
    if label == "A":
        return display_mode == "all" and current_mask == button_mask
    if _stem_preset_is_active(current_mask, display_mode):
        return False
    return current_mask & button_mask != 0


def _stem_mask_button(
    ctx: UiContext,
    pad_id: int,
    label: str,
    button_mask: int,
    display_mode: StemMaskDisplayMode,
    current_mask: int,
    target_display_mode: StemMaskDisplayMode,
) -> None:
    style: ButtonStyleName = (
        "mode-on"
        if stem_button_is_active(label, button_mask, current_mask, display_mode)
        else "mode-off"
    )
    with button_style(style):
        if imgui.button(f"{label}##stem_mask_{label}", (32, 32)):
            target_mask = (
                current_mask ^ button_mask if target_display_mode == "custom" else button_mask
            )
            ctx.audio.stems.set_stem_enabled_mask(pad_id, target_mask, target_display_mode)


def _stem_mask_controls(ctx: UiContext) -> None:
    pad_id = ctx.state.project.selected_pad
    current_mask = ctx.state.stems.stem_enabled_mask(pad_id)
    display_mode = ctx.state.stems.stem_mask_display_mode(pad_id)
    enabled = ctx.state.stems.stem_mask_controls_enabled(pad_id)

    imgui.same_line(spacing=SPACING / 2)
    imgui.begin_disabled(disabled=not enabled)
    with (
        style_var(imgui.StyleVar_.item_spacing, (SPACING / 4, 0.0)),
        style_var(imgui.StyleVar_.frame_rounding, 16.0),
    ):
        for label, mask in STEM_COMPONENT_BUTTONS:
            _stem_mask_button(ctx, pad_id, label, mask, display_mode, current_mask, "custom")
            imgui.same_line(spacing=SPACING / 4)

        for idx, (label, mask, target_display_mode) in enumerate(STEM_PRESET_BUTTONS):
            _stem_mask_button(
                ctx,
                pad_id,
                label,
                mask,
                display_mode,
                current_mask,
                target_display_mode,
            )
            if idx < len(STEM_PRESET_BUTTONS) - 1:
                imgui.same_line(spacing=SPACING / 4)
    imgui.end_disabled()


def bottom_bar(ctx: UiContext) -> None:
    _master_volume(ctx)
    imgui.same_line(spacing=SPACING)
    style: ButtonStyleName = "mode-on" if ctx.state.project.multi_loop else "mode-off"

    with button_style(style):
        if imgui.button("MULTI LOOP"):
            ctx.audio.global_.toggle_multi_loop()

    imgui.same_line(spacing=SPACING)
    _trigger_quantization(ctx)
    _stem_mask_controls(ctx)
