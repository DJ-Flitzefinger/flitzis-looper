import math
import string
from dataclasses import dataclass
from typing import TYPE_CHECKING

from imgui_bundle import imgui, imgui_knobs

from flitzis_looper.audio_eq import (
    EQ_KNOB_POSITION_MAX,
    EQ_KNOB_POSITION_MIN,
    clamp_eq_db,
    eq_db_to_knob_position,
    eq_drag_delta_position,
    eq_knob_position_to_db,
    format_eq_db,
)
from flitzis_looper.audio_gain import (
    clamp_gain_db,
    format_gain_db,
    gain_db_to_normalized,
    gain_drag_delta_db,
    gain_fine_step_direction,
    gain_meter_fraction_from_peak,
    gain_track_x_to_db,
)
from flitzis_looper.constants import (
    PAD_EQ_DB_MIN,
    PAD_GAIN_DB_DEFAULT,
    PAD_GAIN_FINE_STEP_DB,
)
from flitzis_looper.ui.constants import (
    CONTROL_ACTIVE_BORDER_RGBA,
    CONTROL_BORDER_RGBA,
    CONTROL_HOVERED_RGBA,
    CONTROL_RGBA,
    SPACING,
    TEXT_MUTED_RGBA,
    TEXT_RGBA,
)
from flitzis_looper.ui.contextmanager import button_style, style_var
from flitzis_looper.ui.render.control_gestures import hovered_wheel_steps, item_middle_clicked

if TYPE_CHECKING:
    from flitzis_looper.input_mapping import PadEqBand
    from flitzis_looper.models import StemMixMode
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper.ui.styles import ButtonStyleName


_KEYS = (
    "C",
    "C#",
    "D",
    "D#",
    "E",
    "F",
    "F#",
    "G",
    "G#",
    "A",
    "A#",
    "B",
)
_KEY_OPTIONS = [*_KEYS, *(f"{key}m" for key in _KEYS)]
_STEM_MIX_OPTIONS: tuple[tuple[StemMixMode, str], ...] = (
    ("full_mix", "FULL MIX"),
    ("all_stems", "ALL STEMS"),
)
_EQ_KNOBS: tuple[tuple[str, str, PadEqBand, str], ...] = (
    ("Low", "##pad_eq_low", "low", "pad_eq_low_db"),
    ("Mid", "##pad_eq_mid", "mid", "pad_eq_mid_db"),
    ("High", "##pad_eq_high", "high", "pad_eq_high_db"),
)
_GAIN_CONTROL_HEIGHT = 30.0
_GAIN_METER_HEIGHT = 22.0
_GAIN_HANDLE_WIDTH = 6.0
_GAIN_RIGHT_CLICK_DRAG_THRESHOLD_PX = 3.0
_GAIN_WHEEL_STEP_DB = 0.5
_EQ_WHEEL_STEP_DB = 1.0
_EQ_ENTRY_DIGITS = frozenset(string.digits)
_METER_BG_RGBA = (0.02, 0.02, 0.02, 0.55)
_METER_FILL_RGBA = (1.0, 1.0, 1.0, 0.22)
_METER_GREEN_RGBA = (0.18, 0.74, 0.38, 0.45)
_METER_YELLOW_RGBA = (0.95, 0.75, 0.18, 0.60)
_METER_CLIP_RGBA = (1.0, 0.08, 0.08, 0.95)
_METER_GREEN_ZONE_FRACTION = 0.8


def eq_wheel_delta_db(wheel_steps: int) -> float:
    """Return EQ dB delta for hovered EQ wheel movement."""
    return _EQ_WHEEL_STEP_DB * wheel_steps


def sanitize_eq_entry_text(text: str) -> str:
    """Return manual EQ text containing an optional leading minus and decimal point."""
    sanitized: list[str] = []
    has_decimal = False

    for char in text.replace(",", "."):
        if (char == "-" and not sanitized) or char in _EQ_ENTRY_DIGITS:
            sanitized.append(char)
        elif char == "." and not has_decimal:
            has_decimal = True
            sanitized.append(char)

    return "".join(sanitized)


def filtered_eq_entry_char(
    char_code: int,
    current_text: str,
    cursor_pos: int,
    *,
    has_selection: bool,
) -> int | None:
    """Return a replacement char code, or None when EQ entry must reject it."""
    if char_code == 0:
        return 0

    char = chr(char_code)
    if char == ",":
        char = "."
        char_code = ord(".")

    if char in _EQ_ENTRY_DIGITS:
        return char_code

    current_text = current_text.replace(",", ".")
    if char == "-" and cursor_pos == 0 and (has_selection or not current_text.startswith("-")):
        return char_code

    if char == "." and ("." not in current_text or has_selection):
        return char_code

    return None


def eq_entry_char_filter(data: imgui.InputTextCallbackData) -> int:
    """Reject manual EQ entry characters before they enter the input buffer."""
    replacement = filtered_eq_entry_char(
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


def parse_eq_entry_text(text: str) -> float | None:
    """Parse manual EQ entry text, returning a clamped one-decimal dB value."""
    sanitized = sanitize_eq_entry_text(text)
    if sanitized in {"", ".", "-", "-."}:
        return None
    try:
        value = float(sanitized)
    except ValueError:
        return None
    if not math.isfinite(value):
        return None
    return round(clamp_eq_db(value), 1)


def gain_wheel_delta_db(wheel_steps: int) -> float:
    """Return Gain/Trim dB delta for hovered Gain wheel movement."""
    return _GAIN_WHEEL_STEP_DB * wheel_steps


def gain_meter_zone_fractions() -> tuple[float, float]:
    """Return fixed green/yellow zone proportions for the Gain-area meter."""
    green = _METER_GREEN_ZONE_FRACTION
    return green, 1.0 - green


@dataclass(frozen=True)
class _SidebarPadInfo:
    pad_id: int
    avail_x: float
    is_loaded: bool
    is_loading: bool


@dataclass
class _GainDragState:
    pad_id: int | None = None
    button: int | None = None
    start_x: float = 0.0
    start_y: float = 0.0
    dragged: bool = False

    def start(self, *, pad_id: int, button: int, mouse_pos: imgui.ImVec2) -> None:
        self.pad_id = pad_id
        self.button = button
        self.start_x = float(mouse_pos.x)
        self.start_y = float(mouse_pos.y)
        self.dragged = False

    def clear(self) -> None:
        self.pad_id = None
        self.button = None
        self.start_x = 0.0
        self.start_y = 0.0
        self.dragged = False


_GAIN_DRAG = _GainDragState()


@dataclass
class _EqDragState:
    pad_id: int | None = None
    band: PadEqBand | None = None

    def start(self, *, pad_id: int, band: PadEqBand) -> None:
        self.pad_id = pad_id
        self.band = band

    def clear(self) -> None:
        self.pad_id = None
        self.band = None

    def matches(self, *, pad_id: int, band: PadEqBand) -> bool:
        return self.pad_id == pad_id and self.band == band


@dataclass
class _EqValueEditState:
    pad_id: int | None = None
    band: PadEqBand | None = None
    text: str = ""
    focus_requested: bool = False

    def start(self, *, pad_id: int, band: PadEqBand, db: float) -> None:
        self.pad_id = pad_id
        self.band = band
        self.text = f"{clamp_eq_db(db):.1f}"
        self.focus_requested = True

    def clear(self) -> None:
        self.pad_id = None
        self.band = None
        self.text = ""
        self.focus_requested = False

    def matches(self, *, pad_id: int, band: PadEqBand) -> bool:
        return self.pad_id == pad_id and self.band == band


_EQ_DRAG = _EqDragState()
_EQ_VALUE_EDIT = _EqValueEditState()


def _labeled_value_row(
    label: str,
    value: str,
    *,
    avail_x: float,
    value_rgba: imgui.ImVec4Like = TEXT_RGBA,
) -> None:
    imgui.text_colored(TEXT_MUTED_RGBA, label)
    text_width = imgui.calc_text_size(value).x
    imgui.same_line(max(0.0, avail_x - text_width))
    imgui.text_colored(value_rgba, value)


def _labeled_wrapped_value_row(
    label: str,
    value: str,
    *,
    avail_x: float,
    value_rgba: imgui.ImVec4Like = TEXT_RGBA,
) -> None:
    imgui.text_colored(TEXT_MUTED_RGBA, label)

    label_width = imgui.calc_text_size(label).x
    gap = imgui.get_style().item_inner_spacing.x
    value_x = min(avail_x, label_width + gap)
    imgui.same_line(value_x)

    value_pos = imgui.get_cursor_screen_pos()
    wrap_x = value_pos.x + max(1.0, avail_x - value_x)
    imgui.push_text_wrap_pos(wrap_x)
    try:
        imgui.text_colored(value_rgba, value)
    finally:
        imgui.pop_text_wrap_pos()


def _has_pending_learn_input(ctx: UiContext) -> bool:
    return (
        ctx.state.project.input_mapping_enabled
        and ctx.state.session.input_learn_pending_binding_key is not None
    )


def _render_pad_header(ctx: UiContext, info: _SidebarPadInfo) -> None:
    _labeled_value_row("Pad", f"#{info.pad_id + 1}", avail_x=info.avail_x)

    if info.is_loaded or info.is_loading:
        filename = ctx.state.pads.label(info.pad_id) or "-"
        _labeled_wrapped_value_row("Filename", filename, avail_x=info.avail_x)

    load_error = ctx.state.pads.load_error(info.pad_id)
    if load_error:
        imgui.text_wrapped(f"Load failed: {load_error}")


def _render_bpm(ctx: UiContext, info: _SidebarPadInfo) -> None:
    effective_bpm = ctx.state.pads.effective_bpm(info.pad_id) if info.is_loaded else None
    bpm_value = 0.0 if effective_bpm is None else float(effective_bpm)

    imgui.text_colored(TEXT_MUTED_RGBA, "BPM")
    input_width = 92.0
    imgui.same_line(max(0.0, info.avail_x - input_width))
    imgui.set_next_item_width(input_width)
    changed, new_value = imgui.input_float(
        "##sidebar_bpm",
        bpm_value,
        0.0,
        0.0,
        "%.2f",
    )
    if changed:
        if new_value <= 0:
            ctx.audio.pads.clear_manual_bpm(info.pad_id)
        else:
            ctx.audio.pads.set_manual_bpm(info.pad_id, float(new_value))

    imgui.button("Tap BPM", (-1, 0))
    if imgui.is_item_hovered() and imgui.is_mouse_clicked(imgui.MouseButton_.left):
        ctx.audio.pads.tap_bpm(info.pad_id)

    if imgui.button("Clear Manual BPM", (-1, 0)):
        ctx.audio.pads.clear_manual_bpm(info.pad_id)


def _render_key(ctx: UiContext, info: _SidebarPadInfo) -> None:
    effective_key = ctx.state.pads.effective_key(info.pad_id) if info.is_loaded else None

    imgui.text_colored(TEXT_MUTED_RGBA, "Key")
    input_width = 92.0
    imgui.same_line(max(0.0, info.avail_x - input_width))
    imgui.set_next_item_width(input_width)

    preview = effective_key or "-"
    if imgui.begin_combo("##sidebar_key", preview):
        for key in _KEY_OPTIONS:
            is_selected = key == effective_key
            if imgui.selectable(key, is_selected)[0]:
                ctx.audio.pads.set_manual_key(info.pad_id, key)
            if is_selected:
                imgui.set_item_default_focus()
        imgui.end_combo()

    if imgui.button("Clear Manual Key", (-1, 0)):
        ctx.audio.pads.clear_manual_key(info.pad_id)

    analysis_error = ctx.state.pads.analysis_error(info.pad_id) if info.is_loaded else None
    if analysis_error:
        imgui.text_wrapped(f"Analysis failed: {analysis_error}")


def _gain_rgba(*, active: bool, hovered: bool) -> imgui.ImVec4Like:
    if active:
        return CONTROL_ACTIVE_BORDER_RGBA
    if hovered:
        return CONTROL_HOVERED_RGBA
    return CONTROL_RGBA


def _render_gain_track(
    *,
    gain_db: float,
    hovered: bool,
    active: bool,
    pos_min: imgui.ImVec2,
    pos_max: imgui.ImVec2,
) -> None:
    draw_list = imgui.get_window_draw_list()
    y_mid = (pos_min.y + pos_max.y) * 0.5
    track_min = (pos_min.x, y_mid - 4.0)
    track_max = (pos_max.x, y_mid + 4.0)

    draw_list.add_rect_filled(
        track_min,
        track_max,
        imgui.get_color_u32(_gain_rgba(active=active, hovered=hovered)),
    )
    draw_list.add_rect(track_min, track_max, imgui.get_color_u32(CONTROL_BORDER_RGBA))

    center_x = pos_min.x + (pos_max.x - pos_min.x) * 0.5
    value_x = pos_min.x + (pos_max.x - pos_min.x) * gain_db_to_normalized(gain_db)
    fill_min = (min(center_x, value_x), y_mid - 4.0)
    fill_max = (max(center_x, value_x), y_mid + 4.0)
    if abs(value_x - center_x) > 0.5:
        draw_list.add_rect_filled(
            fill_min,
            fill_max,
            imgui.get_color_u32(CONTROL_ACTIVE_BORDER_RGBA),
        )

    draw_list.add_line(
        (center_x, pos_min.y + 3.0),
        (center_x, pos_max.y - 3.0),
        imgui.get_color_u32(TEXT_MUTED_RGBA),
    )
    draw_list.add_rect_filled(
        (value_x - _GAIN_HANDLE_WIDTH * 0.5, pos_min.y + 2.0),
        (value_x + _GAIN_HANDLE_WIDTH * 0.5, pos_max.y - 2.0),
        imgui.get_color_u32(TEXT_RGBA),
    )


def _gain_meter_geometry(pos_min: imgui.ImVec2, pos_max: imgui.ImVec2) -> tuple[float, float]:
    clip_width = 38.0
    meter_max_x = max(pos_min.x, pos_max.x - clip_width - 4.0)
    meter_width = max(0.0, meter_max_x - pos_min.x)
    return meter_max_x, meter_width


def _draw_gain_meter_zones(
    draw_list: imgui.ImDrawList,
    pos_min: imgui.ImVec2,
    pos_max: imgui.ImVec2,
    meter_max_x: float,
    meter_width: float,
) -> None:
    meter_min = (pos_min.x, pos_min.y)
    meter_max = (meter_max_x, pos_max.y)
    green_fraction, _ = gain_meter_zone_fractions()
    green_end = pos_min.x + meter_width * green_fraction

    draw_list.add_rect_filled(meter_min, meter_max, imgui.get_color_u32(_METER_BG_RGBA))
    draw_list.add_rect_filled(
        meter_min,
        (green_end, pos_max.y),
        imgui.get_color_u32(_METER_GREEN_RGBA),
    )
    draw_list.add_rect_filled(
        (green_end, pos_min.y),
        meter_max,
        imgui.get_color_u32(_METER_YELLOW_RGBA),
    )
    draw_list.add_rect(meter_min, meter_max, imgui.get_color_u32(CONTROL_BORDER_RGBA))


def _draw_gain_meter_fill(
    draw_list: imgui.ImDrawList,
    pos_min: imgui.ImVec2,
    pos_max: imgui.ImVec2,
    meter_width: float,
    peak: float,
) -> None:
    fill_x = pos_min.x + meter_width * gain_meter_fraction_from_peak(peak)
    if fill_x <= pos_min.x:
        return

    draw_list.add_rect_filled(
        (pos_min.x, pos_min.y),
        (fill_x, pos_max.y),
        imgui.get_color_u32(_METER_FILL_RGBA),
    )


def _draw_gain_meter_text(
    draw_list: imgui.ImDrawList,
    pos_min: imgui.ImVec2,
    pos_max: imgui.ImVec2,
    meter_width: float,
    gain_db: float,
) -> None:
    gain_text = format_gain_db(gain_db)
    text_size = imgui.calc_text_size(gain_text)
    text_pos = (
        pos_min.x + (meter_width - text_size.x) * 0.5,
        pos_min.y + (pos_max.y - pos_min.y - text_size.y) * 0.5,
    )
    draw_list.add_text(text_pos, imgui.get_color_u32(TEXT_RGBA), gain_text)


def _draw_gain_clip_indicator(
    draw_list: imgui.ImDrawList,
    pos_min: imgui.ImVec2,
    pos_max: imgui.ImVec2,
    meter_max_x: float,
    *,
    active: bool,
) -> None:
    clip_min = (meter_max_x + 4.0, pos_min.y)
    clip_max = (pos_max.x, pos_max.y)
    clip_bg = _METER_CLIP_RGBA if active else _METER_BG_RGBA
    clip_text = TEXT_RGBA if active else TEXT_MUTED_RGBA
    draw_list.add_rect_filled(clip_min, clip_max, imgui.get_color_u32(clip_bg))
    draw_list.add_rect(clip_min, clip_max, imgui.get_color_u32(CONTROL_BORDER_RGBA))

    clip_label = "CLIP"
    clip_text_size = imgui.calc_text_size(clip_label)
    clip_text_pos = (
        clip_min[0] + (clip_max[0] - clip_min[0] - clip_text_size.x) * 0.5,
        clip_min[1] + (clip_max[1] - clip_min[1] - clip_text_size.y) * 0.5,
    )
    draw_list.add_text(clip_text_pos, imgui.get_color_u32(clip_text), clip_label)


def _render_gain_meter(
    *,
    ctx: UiContext,
    pad_id: int,
    gain_db: float,
    width: float,
) -> None:
    imgui.invisible_button("##pad_gain_meter", (width, _GAIN_METER_HEIGHT))
    pos_min = imgui.get_item_rect_min()
    pos_max = imgui.get_item_rect_max()
    draw_list = imgui.get_window_draw_list()

    meter_max_x, meter_width = _gain_meter_geometry(pos_min, pos_max)
    _draw_gain_meter_zones(draw_list, pos_min, pos_max, meter_max_x, meter_width)
    _draw_gain_meter_fill(draw_list, pos_min, pos_max, meter_width, ctx.state.pads.peak(pad_id))
    _draw_gain_meter_text(draw_list, pos_min, pos_max, meter_width, gain_db)
    _draw_gain_clip_indicator(
        draw_list,
        pos_min,
        pos_max,
        meter_max_x,
        active=ctx.state.pads.clip_active(pad_id),
    )


def _apply_gain_drag(ctx: UiContext, pad_id: int) -> None:
    state = _GAIN_DRAG
    if state.pad_id != pad_id or state.button is None:
        return

    if not imgui.is_mouse_down(state.button):
        if state.button == imgui.MouseButton_.right and not state.dragged:
            axis_min_x = imgui.get_item_rect_min().x
            axis_max_x = imgui.get_item_rect_max().x
            direction = gain_fine_step_direction(state.start_x, axis_min_x, axis_max_x)
            current = float(ctx.state.project.pad_gain_db[pad_id])
            ctx.audio.pads.set_pad_gain(
                pad_id,
                clamp_gain_db(current + PAD_GAIN_FINE_STEP_DB * direction),
            )
        state.clear()
        return

    if state.button == imgui.MouseButton_.left:
        axis_min_x = imgui.get_item_rect_min().x
        axis_max_x = imgui.get_item_rect_max().x
        mouse_x = float(imgui.get_io().mouse_pos.x)
        ctx.audio.pads.set_pad_gain(pad_id, gain_track_x_to_db(mouse_x, axis_min_x, axis_max_x))
        return

    io = imgui.get_io()
    delta_x = float(io.mouse_delta.x)
    delta_y = float(io.mouse_delta.y)
    if abs(delta_x) + abs(delta_y) <= 0.0:
        return

    if abs(float(io.mouse_pos.x) - state.start_x) > _GAIN_RIGHT_CLICK_DRAG_THRESHOLD_PX:
        state.dragged = True
    if abs(float(io.mouse_pos.y) - state.start_y) > _GAIN_RIGHT_CLICK_DRAG_THRESHOLD_PX:
        state.dragged = True

    delta_db = gain_drag_delta_db(
        delta_x,
        delta_y,
        fine=state.button == imgui.MouseButton_.right,
    )
    if delta_db == 0.0:
        return

    current = float(ctx.state.project.pad_gain_db[pad_id])
    ctx.audio.pads.set_pad_gain(pad_id, clamp_gain_db(current + delta_db))


def _render_loaded_gain(ctx: UiContext, pad_id: int) -> None:
    width = imgui.get_content_region_avail().x
    gain_db = float(ctx.state.project.pad_gain_db[pad_id])
    gain_db = clamp_gain_db(gain_db)

    with style_var(imgui.StyleVar_.item_spacing, (0, SPACING / 3)):
        imgui.text_colored(TEXT_MUTED_RGBA, "Gain / Trim")
        imgui.invisible_button("##pad_gain_trim", (width, _GAIN_CONTROL_HEIGHT))

        hovered = imgui.is_item_hovered()
        active = _GAIN_DRAG.pad_id == pad_id
        pos_min = imgui.get_item_rect_min()
        pos_max = imgui.get_item_rect_max()
        _render_gain_track(
            gain_db=gain_db,
            hovered=hovered,
            active=active,
            pos_min=pos_min,
            pos_max=pos_max,
        )

        learn_pending = _has_pending_learn_input(ctx)
        learn_clicked = (
            learn_pending and hovered and imgui.is_mouse_clicked(imgui.MouseButton_.left)
        )
        if learn_clicked:
            ctx.audio.pads.set_pad_gain(pad_id, gain_db)
        elif not learn_pending:
            if hovered and imgui.is_mouse_clicked(imgui.MouseButton_.middle):
                ctx.audio.pads.set_pad_gain(pad_id, PAD_GAIN_DB_DEFAULT)
            elif hovered and imgui.is_mouse_clicked(imgui.MouseButton_.left):
                _GAIN_DRAG.start(
                    pad_id=pad_id,
                    button=imgui.MouseButton_.left,
                    mouse_pos=imgui.get_mouse_pos(),
                )
            elif hovered and imgui.is_mouse_clicked(imgui.MouseButton_.right):
                _GAIN_DRAG.start(
                    pad_id=pad_id,
                    button=imgui.MouseButton_.right,
                    mouse_pos=imgui.get_mouse_pos(),
                )

            _apply_gain_drag(ctx, pad_id)

            if wheel_steps := hovered_wheel_steps():
                current = float(ctx.state.project.pad_gain_db[pad_id])
                ctx.audio.pads.set_pad_gain(
                    pad_id,
                    clamp_gain_db(current + gain_wheel_delta_db(wheel_steps)),
                )

        _render_gain_meter(ctx=ctx, pad_id=pad_id, gain_db=gain_db, width=width)
        imgui.dummy((width, SPACING * 0.75))


def _pad_eq_band_db(ctx: UiContext, pad_id: int, band: PadEqBand) -> float:
    if band == "low":
        return float(ctx.state.project.pad_eq_low_db[pad_id])
    if band == "mid":
        return float(ctx.state.project.pad_eq_mid_db[pad_id])
    return float(ctx.state.project.pad_eq_high_db[pad_id])


def _apply_eq_drag(ctx: UiContext, pad_id: int, band: PadEqBand) -> None:
    if not _EQ_DRAG.matches(pad_id=pad_id, band=band):
        return

    if not imgui.is_mouse_down(imgui.MouseButton_.left):
        _EQ_DRAG.clear()
        return

    io = imgui.get_io()
    delta_position = eq_drag_delta_position(float(io.mouse_delta.x), float(io.mouse_delta.y))
    if delta_position == 0.0:
        return

    current_db = _pad_eq_band_db(ctx, pad_id, band)
    current_position = eq_db_to_knob_position(current_db)
    target_position = min(
        max(current_position + delta_position, EQ_KNOB_POSITION_MIN),
        EQ_KNOB_POSITION_MAX,
    )
    ctx.audio.pads.set_pad_eq_band(
        pad_id,
        band,
        eq_knob_position_to_db(target_position),
    )


def _render_eq_value_field(
    ctx: UiContext,
    *,
    pad_id: int,
    band: PadEqBand,
    db: float,
    width: float,
) -> None:
    if _EQ_VALUE_EDIT.matches(pad_id=pad_id, band=band):
        imgui.set_next_item_width(width)
        flags = (
            imgui.InputTextFlags_.enter_returns_true
            | imgui.InputTextFlags_.auto_select_all
            | imgui.InputTextFlags_.callback_char_filter
        )
        if _EQ_VALUE_EDIT.focus_requested:
            imgui.set_keyboard_focus_here()
            _EQ_VALUE_EDIT.focus_requested = False

        submitted, new_text = imgui.input_text(
            f"##pad_eq_entry_{band}",
            _EQ_VALUE_EDIT.text,
            flags,
            eq_entry_char_filter,
        )
        if new_text != _EQ_VALUE_EDIT.text:
            _EQ_VALUE_EDIT.text = new_text

        commit = submitted or imgui.is_item_deactivated_after_edit()
        close = submitted or imgui.is_item_deactivated()
        if commit:
            target_db = parse_eq_entry_text(_EQ_VALUE_EDIT.text)
            if target_db is not None:
                ctx.audio.pads.set_pad_eq_band(pad_id, band, target_db)
        if close:
            _EQ_VALUE_EDIT.clear()
        return

    value_text = format_eq_db(db)
    pos = imgui.get_cursor_screen_pos()
    height = imgui.get_text_line_height_with_spacing()
    imgui.invisible_button(f"##pad_eq_value_{band}", (width, height))
    hovered = imgui.is_item_hovered()

    text_size = imgui.calc_text_size(value_text)
    text_pos = (
        pos.x + (width - text_size.x) * 0.5,
        pos.y + (height - text_size.y) * 0.5,
    )
    color = CONTROL_HOVERED_RGBA if hovered else TEXT_RGBA
    imgui.get_window_draw_list().add_text(
        text_pos,
        imgui.get_color_u32(color),
        value_text,
    )

    if hovered and imgui.is_mouse_double_clicked(imgui.MouseButton_.left):
        _EQ_VALUE_EDIT.start(pad_id=pad_id, band=band, db=db)


def _draw_eq_knob_label(label: str, item_min: imgui.ImVec2, knob_width: float) -> None:
    text_width = imgui.calc_text_size(label).x
    label_x = item_min.x + (knob_width - text_width) * 0.5
    label_y = item_min.y - imgui.get_text_line_height_with_spacing()
    imgui.get_window_draw_list().add_text(
        (label_x, label_y),
        imgui.get_color_u32(TEXT_MUTED_RGBA),
        label,
    )


def _apply_eq_knob_gestures(
    ctx: UiContext,
    *,
    pad_id: int,
    band: PadEqBand,
    knob_val: float,
    hovered: bool,
    learn_pending: bool,
) -> float:
    learn_clicked = learn_pending and hovered and imgui.is_mouse_clicked(imgui.MouseButton_.left)
    if learn_clicked:
        ctx.audio.pads.set_pad_eq_band(pad_id, band, knob_val)
        return knob_val
    if learn_pending:
        return knob_val

    if hovered and imgui.is_mouse_clicked(imgui.MouseButton_.right):
        ctx.audio.pads.set_pad_eq_band(pad_id, band, PAD_EQ_DB_MIN)
    elif item_middle_clicked():
        ctx.audio.pads.set_pad_eq_band(pad_id, band, 0.0)
    elif hovered and imgui.is_mouse_clicked(imgui.MouseButton_.left):
        _EQ_DRAG.start(pad_id=pad_id, band=band)

    _apply_eq_drag(ctx, pad_id, band)
    knob_val = _pad_eq_band_db(ctx, pad_id, band)

    if not _EQ_DRAG.matches(pad_id=pad_id, band=band) and (wheel_steps := hovered_wheel_steps()):
        ctx.audio.pads.set_pad_eq_band(
            pad_id,
            band,
            knob_val + eq_wheel_delta_db(wheel_steps),
        )
        knob_val = _pad_eq_band_db(ctx, pad_id, band)

    return knob_val


def _render_eq_knob(
    ctx: UiContext,
    info: _SidebarPadInfo,
    *,
    label: str,
    knob_id: str,
    band: PadEqBand,
    source_attr_name: str,
    knob_width: float,
    learn_pending: bool,
) -> None:
    source_attr = getattr(ctx.state.project, source_attr_name)
    knob_val = float(source_attr[info.pad_id])
    knob_position = eq_db_to_knob_position(knob_val)
    imgui.begin_group()
    imgui_knobs.knob(
        knob_id,
        p_value=knob_position,
        v_min=EQ_KNOB_POSITION_MIN,
        v_max=EQ_KNOB_POSITION_MAX,
        format="",
        size=knob_width,
        flags=imgui_knobs.ImGuiKnobFlags_.no_input,
    )

    knob_item_min = imgui.get_item_rect_min()
    knob_hovered = imgui.is_item_hovered()
    _draw_eq_knob_label(label, knob_item_min, knob_width)
    knob_val = _apply_eq_knob_gestures(
        ctx,
        pad_id=info.pad_id,
        band=band,
        knob_val=knob_val,
        hovered=knob_hovered,
        learn_pending=learn_pending,
    )
    _render_eq_value_field(
        ctx,
        pad_id=info.pad_id,
        band=band,
        db=knob_val,
        width=knob_width,
    )
    imgui.end_group()


def _render_loaded_eq(ctx: UiContext, info: _SidebarPadInfo) -> None:
    knob_width = 68.0

    # Vert. space for the knob labels
    imgui.dummy((info.avail_x, SPACING / 2))

    learn_pending = _has_pending_learn_input(ctx)
    if _EQ_DRAG.pad_id is not None and not imgui.is_mouse_down(imgui.MouseButton_.left):
        _EQ_DRAG.clear()
    if _EQ_VALUE_EDIT.pad_id is not None and _EQ_VALUE_EDIT.pad_id != info.pad_id:
        _EQ_VALUE_EDIT.clear()

    with style_var(imgui.StyleVar_.item_spacing, (SPACING / 2, SPACING / 4)):
        for idx, (label, knob_id, band, source_attr_name) in enumerate(_EQ_KNOBS):
            if idx > 0:
                imgui.same_line()

            _render_eq_knob(
                ctx,
                info,
                label=label,
                knob_id=knob_id,
                band=band,
                source_attr_name=source_attr_name,
                knob_width=knob_width,
                learn_pending=learn_pending,
            )

    imgui.dummy((info.avail_x, 0.0))


def _stem_progress_text(ctx: UiContext, pad_id: int) -> str:
    stage, progress = ctx.state.stems.stem_generation_status(pad_id)
    stage = stage or "Generating stems"
    percent_text = "" if progress is None else f"{int(float(progress) * 100):d} %"
    return " ".join([part for part in (stage, percent_text) if part])


def _render_stem_status(ctx: UiContext, info: _SidebarPadInfo) -> None:
    if ctx.state.stems.is_stem_generation_running(info.pad_id):
        _labeled_value_row(
            "Stems",
            _stem_progress_text(ctx, info.pad_id),
            avail_x=info.avail_x,
        )
        return

    error = ctx.state.stems.stem_generation_error(info.pad_id)
    if error:
        _labeled_value_row("Stems", "Error", avail_x=info.avail_x)
        imgui.text_wrapped(error)
        return

    if ctx.state.stems.stems_available(info.pad_id):
        _labeled_value_row("Stems", "Available", avail_x=info.avail_x)
        return

    blocker = ctx.state.stems.stem_generation_block_reason(info.pad_id)
    if blocker is not None:
        _labeled_value_row("Stems", "Blocked", avail_x=info.avail_x)
        imgui.text_wrapped(blocker)
        return

    _labeled_value_row("Stems", "Unavailable", avail_x=info.avail_x)


def _render_stem_mix_mode(ctx: UiContext, info: _SidebarPadInfo) -> None:
    current_mode = ctx.state.stems.stem_mix_mode(info.pad_id)
    has_stems = ctx.state.stems.stems_available(info.pad_id)
    button_width = max(72.0, info.avail_x / len(_STEM_MIX_OPTIONS))

    imgui.text_colored(TEXT_MUTED_RGBA, "Stem Mix")
    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
        for idx, (mode, label) in enumerate(_STEM_MIX_OPTIONS):
            if idx > 0:
                imgui.same_line(spacing=0.0)

            style_name: ButtonStyleName = "mode-on" if current_mode == mode else "mode-off"
            with button_style(style_name):
                imgui.begin_disabled(disabled=not has_stems)
                if imgui.button(f"{label}##stem_mix_{mode}", (button_width, 0)):
                    ctx.audio.stems.set_stem_mix_mode(info.pad_id, mode)
                imgui.end_disabled()

    if current_mode == "all_stems" and not has_stems:
        imgui.text_colored(TEXT_MUTED_RGBA, "All stems pending current cache")


def _render_generate_stems(ctx: UiContext, pad_id: int) -> None:
    blocker = ctx.state.stems.stem_generation_block_reason(pad_id)
    is_running = ctx.state.stems.is_stem_generation_running(pad_id)
    disabled = blocker is not None

    imgui.begin_disabled(disabled=disabled)
    clicked = imgui.button("Generate Stems", (-1, 0))
    imgui.end_disabled()

    if clicked:
        ctx.audio.stems.generate_stems_async(pad_id)

    delete_disabled = is_running or not ctx.state.stems.has_stem_cache(pad_id)
    imgui.begin_disabled(disabled=delete_disabled)
    delete_clicked = imgui.button("Delete Stems", (-1, 0))
    imgui.end_disabled()

    if delete_clicked:
        ctx.audio.stems.delete_stems(pad_id)

    if blocker is not None and not is_running:
        imgui.text_colored(TEXT_MUTED_RGBA, blocker)


def _render_stem_controls(ctx: UiContext, info: _SidebarPadInfo) -> None:
    _render_stem_status(ctx, info)
    _render_generate_stems(ctx, info.pad_id)
    _render_stem_mix_mode(ctx, info)


def _render_pad_key_lock(ctx: UiContext, pad_id: int) -> None:
    style_name: ButtonStyleName = "mode-on" if ctx.state.pads.key_lock(pad_id) else "mode-off"
    with button_style(style_name):
        if imgui.button("KEY LOCK##pad_key_lock", (-1, 0)):
            ctx.audio.pads.toggle_pad_key_lock(pad_id)


def _render_loaded_actions(ctx: UiContext, pad_id: int) -> None:
    if imgui.button("Unload Audio", (-1, 0)):
        ctx.audio.pads.unload_sample(pad_id)

    if ctx.state.pads.is_analyzing(pad_id):
        imgui.text_disabled("Analyzing audio…")
        stage, progress = ctx.state.pads.analysis_status(pad_id)
        stage = stage or "Analyzing"
        percent_text = "" if progress is None else f"{int(float(progress) * 100):d} %"
        status_line = " ".join([part for part in (stage, percent_text) if part])
        if status_line:
            imgui.text_colored(TEXT_MUTED_RGBA, status_line)
        imgui.separator()
        _render_stem_controls(
            ctx,
            _SidebarPadInfo(
                pad_id=pad_id,
                avail_x=imgui.get_content_region_avail().x,
                is_loaded=True,
                is_loading=False,
            ),
        )
        return

    if imgui.button("Analyze audio", (-1, 0)):
        ctx.audio.pads.analyze_sample_async(pad_id)

    if imgui.button("Adjust Loop", (-1, 0)):
        ctx.ui.open_waveform_editor(pad_id)

    imgui.separator()
    _render_stem_controls(
        ctx,
        _SidebarPadInfo(
            pad_id=pad_id,
            avail_x=imgui.get_content_region_avail().x,
            is_loaded=True,
            is_loading=False,
        ),
    )


def _render_loading_status(ctx: UiContext, pad_id: int) -> None:
    stage = ctx.state.pads.load_stage(pad_id) or "Loading"
    progress = ctx.state.pads.load_progress(pad_id)
    percent_text = "" if progress is None else f"{int(float(progress) * 100):d} %"
    status_line = " ".join([part for part in (stage, percent_text) if part])
    imgui.text_colored(TEXT_MUTED_RGBA, status_line or "Loading…")


def _render_unloaded_actions(ctx: UiContext, pad_id: int) -> None:
    if imgui.button("Load Audio", (-1, 0)):
        ctx.ui.open_file_dialog(pad_id)


def sidebar_left(ctx: UiContext) -> None:
    avail_x = imgui.get_content_region_avail().x
    pad_id = ctx.state.project.selected_pad

    info = _SidebarPadInfo(
        pad_id=pad_id,
        avail_x=avail_x,
        is_loaded=ctx.state.pads.is_loaded(pad_id),
        is_loading=ctx.state.pads.is_loading(pad_id),
    )

    _render_pad_header(ctx, info)

    if info.is_loaded:
        imgui.separator()
        _render_bpm(ctx, info)
        imgui.separator()
        _render_key(ctx, info)
        imgui.separator()
        _render_loaded_gain(ctx, info.pad_id)
        _render_loaded_eq(ctx, info)

    imgui.separator()

    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
        if info.is_loaded:
            _render_loaded_actions(ctx, info.pad_id)
            _render_pad_key_lock(ctx, info.pad_id)
        elif info.is_loading:
            _render_loading_status(ctx, info.pad_id)
        else:
            _render_unloaded_actions(ctx, info.pad_id)
