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
from flitzis_looper.ui.render.control_gestures import hovered_wheel_steps, item_middle_clicked
from flitzis_looper.ui.render.settings import (
    SETTINGS_TOGGLE_BUTTON_SIZE,
    settings_toggle_button,
)

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper.ui.styles import ButtonStyleName

MODE_BUTTON_HEIGHT = 24.0
MODE_BUTTON_WIDTH = 36.0
MULTI_LOOP_BUTTON_WIDTH = 88.0
MASTER_VOLUME_WIDTH = 300.0
STEM_BUTTON_SIZE = 32.0
MODE_BUTTON_GAP = SPACING * 0.75
CONTROL_GROUP_GAP = SPACING * 1.75
STEM_BUTTON_GAP = SPACING * 0.5
STEM_PRESET_GAP = SPACING * 0.75
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
_MASTER_VOLUME_WHEEL_STEP = 0.05


def master_volume_wheel_delta(wheel_steps: int) -> float:
    """Return volume delta for hovered Master Volume wheel movement."""
    return _MASTER_VOLUME_WHEEL_STEP * wheel_steps


def settings_button_local_pos(
    *,
    cursor_x: float,
    cursor_y: float,
    available_width: float,
    available_height: float,
) -> tuple[float, float]:
    """Return the local bottom-bar cursor position for the Settings toggle."""
    x = cursor_x + max(0.0, available_width - SETTINGS_TOGGLE_BUTTON_SIZE)
    y = cursor_y + max(0.0, (available_height - SETTINGS_TOGGLE_BUTTON_SIZE) / 2.0)
    return (x, y)


def _set_cursor_y_for_button(*, center_y: float, height: float) -> None:
    imgui.set_cursor_pos_y(max(0.0, center_y - height / 2.0))


def _has_pending_learn_input(ctx: UiContext) -> bool:
    return (
        ctx.state.project.input_mapping_enabled
        and ctx.state.session.input_learn_pending_binding_key is not None
    )


def stem_controls_accept_input(*, enabled: bool, learn_pending: bool) -> bool:
    """Return whether stem mask buttons should accept a click this frame."""
    return enabled or learn_pending


def stem_button_learn_target_state(
    button_mask: int,
    target_display_mode: StemMaskDisplayMode,
) -> tuple[int, StemMaskDisplayMode]:
    """Return the stable action saved when Learn targets a stem button."""
    return button_mask, target_display_mode


def _master_volume(ctx: UiContext) -> None:
    with style_var(imgui.StyleVar_.item_spacing, (0, SPACING / 2)):
        val = max(0, min(100, round(ctx.state.project.volume * 100)))
        imgui.text_unformatted("Master Volume")

        with item_width(MASTER_VOLUME_WIDTH):
            changed, new_value = imgui.slider_int("##master_volume", val, 0, 100, "%d %")
            learn_pending = _has_pending_learn_input(ctx)
            learn_clicked = (
                learn_pending
                and imgui.is_item_hovered()
                and imgui.is_mouse_clicked(imgui.MouseButton_.left)
            )
            if changed or learn_clicked:
                volume_value = new_value if changed else val
                ctx.audio.global_.set_volume(volume_value / 100.0)
            elif not learn_pending:
                if item_middle_clicked():
                    ctx.audio.global_.set_volume(1.0)
                elif wheel_steps := hovered_wheel_steps():
                    ctx.audio.global_.set_volume(
                        float(ctx.state.project.volume) + master_volume_wheel_delta(wheel_steps)
                    )


def trigger_quantization_button_style(*, enabled: bool) -> ButtonStyleName:
    """Return the bottom-bar Q button style for tests and rendering."""
    return "mode-on" if enabled else "mode-off"


def _trigger_quantization_toggle(ctx: UiContext, center_y: float) -> None:
    style = trigger_quantization_button_style(
        enabled=ctx.state.project.trigger_quantization_enabled
    )
    _set_cursor_y_for_button(center_y=center_y, height=MODE_BUTTON_HEIGHT)
    with button_style(style):
        if imgui.button(
            "Q##trigger_quantization_toggle",
            (MODE_BUTTON_WIDTH, MODE_BUTTON_HEIGHT),
        ):
            ctx.audio.global_.toggle_trigger_quantization()

    if imgui.is_item_hovered():
        imgui.set_tooltip("Trigger Quantize")


def _learn_control(ctx: UiContext, center_y: float) -> None:
    enabled = ctx.state.project.input_mapping_enabled
    active = ctx.state.session.input_learn_active
    pending = ctx.state.session.input_learn_pending_binding_key is not None
    style: ButtonStyleName = "mode-on" if active or pending else "mode-off"

    _set_cursor_y_for_button(center_y=center_y, height=MODE_BUTTON_HEIGHT)
    imgui.begin_disabled(disabled=not enabled)
    with button_style(style):
        if imgui.button("L##input_mapping_learn", (MODE_BUTTON_WIDTH, MODE_BUTTON_HEIGHT)):
            ctx.input.toggle_learn()
    imgui.end_disabled()

    if imgui.is_item_hovered():
        if pending:
            imgui.set_tooltip("Delete mapping for captured input")
        elif active:
            imgui.set_tooltip("Cancel Learn")
        else:
            imgui.set_tooltip("Learn input mapping")


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


def stem_button_target_state(
    button_mask: int,
    current_mask: int,
    last_custom_mask: int,
    display_mode: StemMaskDisplayMode,
    target_display_mode: StemMaskDisplayMode,
) -> tuple[int, StemMaskDisplayMode]:
    """Return the mask/display state produced by a stem button click."""
    if target_display_mode != "custom":
        if display_mode == target_display_mode:
            return last_custom_mask, "custom"
        return button_mask, target_display_mode
    if display_mode != "custom":
        return button_mask, "custom"
    return current_mask ^ button_mask, "custom"


def stem_button_solo_state(button_mask: int) -> tuple[int, StemMaskDisplayMode]:
    """Return the mask/display state produced by a component stem right-click."""
    return button_mask, "custom"


def _stem_mask_button(
    ctx: UiContext,
    pad_id: int,
    label: str,
    button_mask: int,
    display_mode: StemMaskDisplayMode,
    current_mask: int,
    last_custom_mask: int,
    target_display_mode: StemMaskDisplayMode,
) -> None:
    learn_pending = _has_pending_learn_input(ctx)
    style: ButtonStyleName = (
        "mode-on"
        if stem_button_is_active(label, button_mask, current_mask, display_mode)
        else "mode-off"
    )
    with button_style(style):
        clicked = imgui.button(
            f"{label}##stem_mask_{label}",
            (STEM_BUTTON_SIZE, STEM_BUTTON_SIZE),
        )
        right_clicked = (
            target_display_mode == "custom"
            and imgui.is_item_hovered()
            and imgui.is_mouse_clicked(imgui.MouseButton_.right)
        )
        if clicked:
            if learn_pending:
                target_mask, next_display_mode = stem_button_learn_target_state(
                    button_mask,
                    target_display_mode,
                )
            else:
                target_mask, next_display_mode = stem_button_target_state(
                    button_mask,
                    current_mask,
                    last_custom_mask,
                    display_mode,
                    target_display_mode,
                )
            ctx.audio.stems.set_stem_enabled_mask(pad_id, target_mask, next_display_mode)
        elif right_clicked:
            target_mask, next_display_mode = stem_button_solo_state(button_mask)
            ctx.audio.stems.set_stem_enabled_mask(pad_id, target_mask, next_display_mode)


def _stem_mask_controls(ctx: UiContext, center_y: float) -> None:
    pad_id = ctx.state.project.selected_pad
    current_mask = ctx.state.stems.stem_enabled_mask(pad_id)
    last_custom_mask = ctx.state.stems.stem_last_custom_mask(pad_id)
    display_mode = ctx.state.stems.stem_mask_display_mode(pad_id)
    enabled = ctx.state.stems.stem_mask_controls_enabled(pad_id)
    learn_pending = _has_pending_learn_input(ctx)

    _set_cursor_y_for_button(center_y=center_y, height=STEM_BUTTON_SIZE)
    imgui.begin_disabled(
        disabled=not stem_controls_accept_input(
            enabled=enabled,
            learn_pending=learn_pending,
        )
    )
    row_y = imgui.get_cursor_pos_y()
    with (
        style_var(imgui.StyleVar_.item_spacing, (STEM_BUTTON_GAP, 0.0)),
        style_var(imgui.StyleVar_.frame_rounding, 16.0),
    ):
        for idx, (label, mask) in enumerate(STEM_COMPONENT_BUTTONS):
            imgui.set_cursor_pos_y(row_y)
            _stem_mask_button(
                ctx,
                pad_id,
                label,
                mask,
                display_mode,
                current_mask,
                last_custom_mask,
                "custom",
            )
            spacing = STEM_BUTTON_GAP if idx < len(STEM_COMPONENT_BUTTONS) - 1 else STEM_PRESET_GAP
            imgui.same_line(spacing=spacing)

        for idx, (label, mask, target_display_mode) in enumerate(STEM_PRESET_BUTTONS):
            imgui.set_cursor_pos_y(row_y)
            _stem_mask_button(
                ctx,
                pad_id,
                label,
                mask,
                display_mode,
                current_mask,
                last_custom_mask,
                target_display_mode,
            )
            if idx < len(STEM_PRESET_BUTTONS) - 1:
                imgui.same_line(spacing=STEM_BUTTON_GAP)
    imgui.end_disabled()


def _bottom_bar_controls(ctx: UiContext, center_y: float) -> None:
    _master_volume(ctx)
    imgui.same_line(spacing=CONTROL_GROUP_GAP)
    _set_cursor_y_for_button(center_y=center_y, height=MODE_BUTTON_HEIGHT)
    style: ButtonStyleName = "mode-on" if ctx.state.project.multi_loop else "mode-off"

    with button_style(style):
        if imgui.button("MULTI LOOP", (MULTI_LOOP_BUTTON_WIDTH, MODE_BUTTON_HEIGHT)):
            ctx.audio.global_.toggle_multi_loop()

    imgui.same_line(spacing=MODE_BUTTON_GAP)
    _learn_control(ctx, center_y)
    imgui.same_line(spacing=MODE_BUTTON_GAP)
    _trigger_quantization_toggle(ctx, center_y)
    imgui.same_line(spacing=CONTROL_GROUP_GAP)
    _stem_mask_controls(ctx, center_y)


def bottom_bar(ctx: UiContext) -> None:
    avail = imgui.get_content_region_avail()
    start_pos = imgui.get_cursor_pos()
    center_y = start_pos.y + max(0.0, avail.y / 2.0)

    _bottom_bar_controls(ctx, center_y)

    imgui.set_cursor_pos(
        settings_button_local_pos(
            cursor_x=start_pos.x,
            cursor_y=start_pos.y,
            available_width=avail.x,
            available_height=avail.y,
        )
    )
    settings_toggle_button(ctx)
