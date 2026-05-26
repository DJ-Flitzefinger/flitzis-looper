from dataclasses import dataclass
from typing import TYPE_CHECKING

from imgui_bundle import imgui, imgui_knobs

from flitzis_looper.audio_gain import (
    clamp_gain_db,
    format_gain_db,
    gain_db_to_normalized,
    gain_drag_delta_db,
    gain_fine_step_direction,
    gain_meter_fraction_from_peak,
)
from flitzis_looper.constants import (
    PAD_EQ_DB_MAX,
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
_METER_BG_RGBA = (0.02, 0.02, 0.02, 0.55)
_METER_FILL_RGBA = (1.0, 1.0, 1.0, 0.22)
_METER_GREEN_RGBA = (0.18, 0.74, 0.38, 0.45)
_METER_YELLOW_RGBA = (0.95, 0.75, 0.18, 0.60)
_METER_CLIP_RGBA = (1.0, 0.08, 0.08, 0.95)
_METER_GREEN_ZONE_FRACTION = 0.8


def eq_wheel_delta_db(wheel_steps: int) -> float:
    """Return EQ dB delta for hovered EQ wheel movement."""
    return _EQ_WHEEL_STEP_DB * wheel_steps


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


def _has_pending_learn_input(ctx: UiContext) -> bool:
    return (
        ctx.state.project.input_mapping_enabled
        and ctx.state.session.input_learn_pending_binding_key is not None
    )


def _render_pad_header(ctx: UiContext, info: _SidebarPadInfo) -> None:
    _labeled_value_row("Pad", f"#{info.pad_id + 1}", avail_x=info.avail_x)

    if info.is_loaded or info.is_loading:
        filename = ctx.state.pads.label(info.pad_id) or "-"
        _labeled_value_row("Filename", filename, avail_x=info.avail_x)

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
        "%.1f",
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
            learn_pending
            and hovered
            and imgui.is_mouse_clicked(imgui.MouseButton_.left)
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


def _render_loaded_eq(ctx: UiContext, info: _SidebarPadInfo) -> None:
    knob_width = 68.0

    # Vert. space for the knob labels
    imgui.dummy((info.avail_x, SPACING / 2))

    learn_pending = _has_pending_learn_input(ctx)

    with style_var(imgui.StyleVar_.item_spacing, (SPACING / 2, SPACING / 4)):
        for idx, (label, knob_id, band, source_attr_name) in enumerate(_EQ_KNOBS):
            if idx > 0:
                imgui.same_line()

            source_attr = getattr(ctx.state.project, source_attr_name)
            knob_val = float(source_attr[info.pad_id])
            changed, new_val = imgui_knobs.knob(
                knob_id,
                p_value=knob_val,
                v_min=PAD_EQ_DB_MIN,
                v_max=PAD_EQ_DB_MAX,
                format="%.1f dB",
                size=knob_width,
            )

            draw_list = imgui.get_window_draw_list()
            item_min = imgui.get_item_rect_min()
            text_width = imgui.calc_text_size(label).x
            label_x = item_min.x + (knob_width - text_width) * 0.5
            label_y = item_min.y - imgui.get_text_line_height_with_spacing()
            draw_list.add_text(
                (label_x, label_y),
                imgui.get_color_u32(TEXT_MUTED_RGBA),
                label,
            )

            learn_clicked = (
                learn_pending
                and imgui.is_item_hovered()
                and imgui.is_mouse_clicked(imgui.MouseButton_.left)
            )
            if changed or learn_clicked:
                ctx.audio.pads.set_pad_eq_band(
                    info.pad_id,
                    band,
                    float(new_val if changed else knob_val),
                )
            elif not learn_pending:
                if item_middle_clicked():
                    ctx.audio.pads.set_pad_eq_band(info.pad_id, band, 0.0)
                elif wheel_steps := hovered_wheel_steps():
                    ctx.audio.pads.set_pad_eq_band(
                        info.pad_id,
                        band,
                        knob_val + eq_wheel_delta_db(wheel_steps),
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
        elif info.is_loading:
            _render_loading_status(ctx, info.pad_id)
        else:
            _render_unloaded_actions(ctx, info.pad_id)
