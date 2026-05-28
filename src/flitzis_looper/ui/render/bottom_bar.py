from dataclasses import dataclass
from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.audio_gain import gain_meter_fraction_from_peak
from flitzis_looper.models import (
    STEM_COMPONENT_MASK,
    STEM_INSTRUMENTAL_PRESET_MASK,
    STEM_MASK_BASS,
    STEM_MASK_DRUMS,
    STEM_MASK_MELODY,
    STEM_MASK_VOCALS,
    StemMaskDisplayMode,
)
from flitzis_looper.ui.constants import (
    CONTROL_BORDER_RGBA,
    CONTROL_HOVERED_RGBA,
    CONTROL_PRESSED_RGBA,
    CONTROL_RGBA,
    SPACING,
    TEXT_MUTED_RGBA,
    TEXT_RGBA,
)
from flitzis_looper.ui.contextmanager import button_style, style_var
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
MASTER_VOLUME_FADER_HEIGHT = MODE_BUTTON_HEIGHT
MASTER_METER_CLIP_WIDTH = 38.0
STEM_BUTTON_SIZE = 32.0
START_STOP_BUTTON_WIDTH = 92.0
START_STOP_BUTTON_HEIGHT = SETTINGS_TOGGLE_BUTTON_SIZE
START_STOP_BUTTON_LABEL = "START/STOP##global_start_stop"
MODE_BUTTON_GAP = SPACING * 0.75
CONTROL_GROUP_GAP = SPACING * 1.75
STEM_BUTTON_GAP = SPACING * 0.5
STEM_PRESET_GAP = SPACING * 0.75
START_STOP_BUTTON_GAP = SPACING
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
_MASTER_METER_CLIP_GAP = 4.0
_MASTER_METER_GREEN_ZONE_FRACTION = 0.8
_MASTER_METER_BG_RGBA = (0.02, 0.02, 0.02, 0.72)
_MASTER_METER_FILL_RGBA = (1.0, 1.0, 1.0, 0.24)
_MASTER_METER_GREEN_RGBA = (0.18, 0.74, 0.38, 0.35)
_MASTER_METER_YELLOW_RGBA = (0.95, 0.75, 0.18, 0.45)
_MASTER_METER_CLIP_RGBA = (1.0, 0.08, 0.08, 0.95)


@dataclass(frozen=True)
class MasterMeterGeometry:
    """Horizontal geometry for the integrated Master Volume output meter."""

    meter_max_x: float
    meter_width: float
    clip_min_x: float
    clip_max_x: float


def master_volume_wheel_delta(wheel_steps: int) -> float:
    """Return volume delta for hovered Master Volume wheel movement."""
    return _MASTER_VOLUME_WHEEL_STEP * wheel_steps


def master_meter_geometry(*, pos_min_x: float, pos_max_x: float) -> MasterMeterGeometry:
    """Return Master Volume meter geometry with a right-end CLIP region."""
    width = max(0.0, pos_max_x - pos_min_x)
    clip_width = min(MASTER_METER_CLIP_WIDTH, width)
    clip_min_x = max(pos_min_x, pos_max_x - clip_width)
    meter_max_x = max(pos_min_x, clip_min_x - _MASTER_METER_CLIP_GAP)
    return MasterMeterGeometry(
        meter_max_x=meter_max_x,
        meter_width=max(0.0, meter_max_x - pos_min_x),
        clip_min_x=clip_min_x,
        clip_max_x=pos_max_x,
    )


def master_volume_fraction_from_mouse_x(
    *,
    mouse_x: float,
    pos_min_x: float,
    meter_max_x: float,
) -> float:
    """Return the Master Volume fader fraction selected by a mouse X position."""
    meter_width = max(0.0, meter_max_x - pos_min_x)
    if meter_width <= 0.0:
        return 0.0
    return min(max((mouse_x - pos_min_x) / meter_width, 0.0), 1.0)


def master_output_meter_fill_fraction(peak: float) -> float:
    """Return clamped visual meter fill for an unclamped master output peak."""
    return gain_meter_fraction_from_peak(peak)


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


def start_stop_button_local_pos(
    *,
    cursor_x: float,
    cursor_y: float,
    available_width: float,
    available_height: float,
) -> tuple[float, float]:
    """Return the local bottom-bar cursor position for START/STOP."""
    x = cursor_x + max(
        0.0,
        available_width
        - SETTINGS_TOGGLE_BUTTON_SIZE
        - START_STOP_BUTTON_GAP
        - START_STOP_BUTTON_WIDTH,
    )
    y = cursor_y + max(0.0, (available_height - START_STOP_BUTTON_HEIGHT) / 2.0)
    return (x, y)


def _set_cursor_y_for_button(*, center_y: float, height: float) -> None:
    imgui.set_cursor_pos_y(max(0.0, center_y - height / 2.0))


def _has_pending_learn_input(ctx: UiContext) -> bool:
    return (
        ctx.state.project.input_mapping_enabled
        and ctx.state.session.input_learn_pending_binding_key is not None
    )


def _master_volume_bg_rgba(*, active: bool, hovered: bool) -> imgui.ImVec4Like:
    if active:
        return CONTROL_PRESSED_RGBA
    if hovered:
        return CONTROL_HOVERED_RGBA
    return CONTROL_RGBA


def _draw_master_meter_zones(
    draw_list: imgui.ImDrawList,
    pos_min: imgui.ImVec2,
    pos_max: imgui.ImVec2,
    geometry: MasterMeterGeometry,
) -> None:
    meter_min = (pos_min.x, pos_min.y)
    meter_max = (geometry.meter_max_x, pos_max.y)
    green_end = pos_min.x + geometry.meter_width * _MASTER_METER_GREEN_ZONE_FRACTION

    draw_list.add_rect_filled(meter_min, meter_max, imgui.get_color_u32(_MASTER_METER_BG_RGBA))
    draw_list.add_rect_filled(
        meter_min,
        (green_end, pos_max.y),
        imgui.get_color_u32(_MASTER_METER_GREEN_RGBA),
    )
    draw_list.add_rect_filled(
        (green_end, pos_min.y),
        meter_max,
        imgui.get_color_u32(_MASTER_METER_YELLOW_RGBA),
    )


def _draw_master_meter_fill(
    draw_list: imgui.ImDrawList,
    pos_min: imgui.ImVec2,
    pos_max: imgui.ImVec2,
    geometry: MasterMeterGeometry,
    peak: float,
) -> None:
    fill_x = pos_min.x + geometry.meter_width * master_output_meter_fill_fraction(peak)
    if fill_x <= pos_min.x:
        return

    draw_list.add_rect_filled(
        (pos_min.x, pos_min.y),
        (fill_x, pos_max.y),
        imgui.get_color_u32(_MASTER_METER_FILL_RGBA),
    )


def _draw_master_clip_indicator(
    draw_list: imgui.ImDrawList,
    pos_min: imgui.ImVec2,
    pos_max: imgui.ImVec2,
    geometry: MasterMeterGeometry,
    *,
    active: bool,
) -> None:
    clip_min = (geometry.clip_min_x, pos_min.y)
    clip_max = (geometry.clip_max_x, pos_max.y)
    clip_bg = _MASTER_METER_CLIP_RGBA if active else _MASTER_METER_BG_RGBA
    clip_text = TEXT_RGBA if active else TEXT_MUTED_RGBA

    draw_list.add_rect_filled(clip_min, clip_max, imgui.get_color_u32(clip_bg))
    draw_list.add_rect(clip_min, clip_max, imgui.get_color_u32(CONTROL_BORDER_RGBA))

    label = "CLIP"
    label_size = imgui.calc_text_size(label)
    label_pos = (
        clip_min[0] + (clip_max[0] - clip_min[0] - label_size.x) * 0.5,
        clip_min[1] + (clip_max[1] - clip_min[1] - label_size.y) * 0.5,
    )
    draw_list.add_text(label_pos, imgui.get_color_u32(clip_text), label)


def _draw_master_volume_value(
    draw_list: imgui.ImDrawList,
    pos_min: imgui.ImVec2,
    pos_max: imgui.ImVec2,
    geometry: MasterMeterGeometry,
    value: int,
) -> None:
    value_text = f"{value} %"
    text_size = imgui.calc_text_size(value_text)
    text_pos = (
        pos_min.x + (geometry.meter_width - text_size.x) * 0.5,
        pos_min.y + (pos_max.y - pos_min.y - text_size.y) * 0.5,
    )
    draw_list.add_text(text_pos, imgui.get_color_u32(TEXT_RGBA), value_text)


def _draw_master_volume_handle(
    draw_list: imgui.ImDrawList,
    pos_min: imgui.ImVec2,
    pos_max: imgui.ImVec2,
    geometry: MasterMeterGeometry,
    value: int,
) -> None:
    raw_handle_x = pos_min.x + geometry.meter_width * (value / 100.0)
    handle_x = min(
        max(raw_handle_x, pos_min.x + 2.0),
        max(pos_min.x + 2.0, geometry.meter_max_x - 2.0),
    )
    draw_list.add_rect_filled(
        (handle_x - 2.0, pos_min.y + 2.0),
        (handle_x + 2.0, pos_max.y - 2.0),
        imgui.get_color_u32(TEXT_RGBA),
    )


def _draw_master_volume_fader(
    *,
    ctx: UiContext,
    value: int,
    pos_min: imgui.ImVec2,
    pos_max: imgui.ImVec2,
    active: bool,
    hovered: bool,
) -> None:
    geometry = master_meter_geometry(pos_min_x=pos_min.x, pos_max_x=pos_max.x)
    draw_list = imgui.get_window_draw_list()

    draw_list.add_rect_filled(
        (pos_min.x, pos_min.y),
        (pos_max.x, pos_max.y),
        imgui.get_color_u32(_master_volume_bg_rgba(active=active, hovered=hovered)),
    )
    _draw_master_meter_zones(draw_list, pos_min, pos_max, geometry)
    _draw_master_meter_fill(
        draw_list,
        pos_min,
        pos_max,
        geometry,
        ctx.state.global_.master_output_peak(),
    )
    _draw_master_clip_indicator(
        draw_list,
        pos_min,
        pos_max,
        geometry,
        active=ctx.state.global_.master_clip_active(),
    )
    draw_list.add_rect(
        (pos_min.x, pos_min.y),
        (geometry.meter_max_x, pos_max.y),
        imgui.get_color_u32(CONTROL_BORDER_RGBA),
    )
    _draw_master_volume_value(draw_list, pos_min, pos_max, geometry, value)
    _draw_master_volume_handle(draw_list, pos_min, pos_max, geometry, value)


def _set_volume_from_master_fader_drag(
    ctx: UiContext,
    pos_min: imgui.ImVec2,
    geometry: MasterMeterGeometry,
) -> None:
    mouse_x = float(imgui.get_io().mouse_pos.x)
    fraction = master_volume_fraction_from_mouse_x(
        mouse_x=mouse_x,
        pos_min_x=pos_min.x,
        meter_max_x=geometry.meter_max_x,
    )
    if abs(fraction - float(ctx.state.project.volume)) > 0.0005:
        ctx.audio.global_.set_volume(fraction)


def _master_volume_fader(ctx: UiContext, value: int) -> None:
    imgui.invisible_button(
        "##master_volume",
        (MASTER_VOLUME_WIDTH, MASTER_VOLUME_FADER_HEIGHT),
    )
    pos_min = imgui.get_item_rect_min()
    pos_max = imgui.get_item_rect_max()
    hovered = imgui.is_item_hovered()
    active = imgui.is_item_active()
    geometry = master_meter_geometry(pos_min_x=pos_min.x, pos_max_x=pos_max.x)
    learn_pending = _has_pending_learn_input(ctx)
    learn_clicked = learn_pending and hovered and imgui.is_mouse_clicked(imgui.MouseButton_.left)

    if learn_clicked:
        ctx.audio.global_.set_volume(value / 100.0)
    elif not learn_pending:
        if active and imgui.is_mouse_down(imgui.MouseButton_.left):
            _set_volume_from_master_fader_drag(ctx, pos_min, geometry)
        elif hovered and imgui.is_mouse_clicked(imgui.MouseButton_.right):
            ctx.audio.global_.set_volume(0.0)
        elif item_middle_clicked():
            ctx.audio.global_.set_volume(1.0)
        elif wheel_steps := hovered_wheel_steps():
            ctx.audio.global_.set_volume(
                float(ctx.state.project.volume) + master_volume_wheel_delta(wheel_steps)
            )

    draw_value = max(0, min(100, round(ctx.state.project.volume * 100)))
    _draw_master_volume_fader(
        ctx=ctx,
        value=draw_value,
        pos_min=pos_min,
        pos_max=pos_max,
        active=active,
        hovered=hovered,
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

        _master_volume_fader(ctx, val)


def trigger_quantization_button_style(*, enabled: bool) -> ButtonStyleName:
    """Return the bottom-bar Q button style for tests and rendering."""
    return "mode-on" if enabled else "mode-off"


def start_stop_button_style(*, stopped: bool) -> ButtonStyleName:
    """Return the bottom-bar START/STOP button style for tests and rendering."""
    return "mode-off" if stopped else "mode-on"


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


def _start_stop_button(ctx: UiContext) -> None:
    with button_style(start_stop_button_style(stopped=ctx.state.session.global_stop_engaged)):
        imgui.button(
            START_STOP_BUTTON_LABEL,
            (START_STOP_BUTTON_WIDTH, START_STOP_BUTTON_HEIGHT),
        )

    hovered = imgui.is_item_hovered()

    if imgui.is_mouse_down(imgui.MouseButton_.left):
        if hovered and not ctx.state.session.global_start_stop_left_pressed:
            ctx.audio.global_.start_or_restart_start_stop()
            ctx.ui.store_global_start_stop_pressed(pressed=True)
    else:
        ctx.ui.store_global_start_stop_pressed(pressed=False)

    if hovered and imgui.is_mouse_down(imgui.MouseButton_.right):
        ctx.audio.global_.stop_start_stop()

    if hovered and imgui.is_mouse_clicked(imgui.MouseButton_.middle):
        ctx.audio.global_.set_momentary_output_mute(enabled=True)

    if ctx.state.session.global_stop_momentary_mute_active and not imgui.is_mouse_down(
        imgui.MouseButton_.middle
    ):
        ctx.audio.global_.set_momentary_output_mute(enabled=False)

    if hovered:
        imgui.set_tooltip("Start/restart loops; right stop; hold mouse wheel for mute")


def bottom_bar(ctx: UiContext) -> None:
    avail = imgui.get_content_region_avail()
    start_pos = imgui.get_cursor_pos()
    center_y = start_pos.y + max(0.0, avail.y / 2.0)

    _bottom_bar_controls(ctx, center_y)

    start_stop_pos = start_stop_button_local_pos(
        cursor_x=start_pos.x,
        cursor_y=start_pos.y,
        available_width=avail.x,
        available_height=avail.y,
    )
    settings_pos = settings_button_local_pos(
        cursor_x=start_pos.x,
        cursor_y=start_pos.y,
        available_width=avail.x,
        available_height=avail.y,
    )

    imgui.set_cursor_pos(start_stop_pos)
    _start_stop_button(ctx)

    imgui.set_cursor_pos(settings_pos)
    settings_toggle_button(ctx)
