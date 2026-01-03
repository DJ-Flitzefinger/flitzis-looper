from dataclasses import dataclass
from typing import TYPE_CHECKING

from imgui_bundle import imgui, imgui_knobs

from flitzis_looper.constants import PAD_EQ_DB_MAX, PAD_EQ_DB_MIN
from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA, TEXT_RGBA
from flitzis_looper.ui.contextmanager import style_var

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext


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


@dataclass(frozen=True)
class _SidebarPadInfo:
    pad_id: int
    avail_x: float
    is_loaded: bool
    is_loading: bool


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


def _render_pad_header(ctx: UiContext, info: _SidebarPadInfo) -> None:
    _labeled_value_row("Pad", f"#{info.pad_id + 1}", avail_x=info.avail_x)

    if info.is_loaded or info.is_loading:
        filename = ctx.state.pads.label(info.pad_id) or "-"
        _labeled_value_row("Filename", filename, avail_x=info.avail_x)
    else:
        _labeled_value_row(
            "Filename",
            "- EMPTY -",
            avail_x=info.avail_x,
            value_rgba=TEXT_MUTED_RGBA,
        )

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


def _render_loaded_gain(ctx: UiContext, pad_id: int) -> None:
    with style_var(imgui.StyleVar_.item_spacing, (0, SPACING / 2)):
        imgui.text_colored(TEXT_MUTED_RGBA, "Gain")
        imgui.set_next_item_width(-1)
        gain_val = max(0, min(100, round(ctx.state.project.pad_gain[pad_id] * 100)))
        changed, new_gain = imgui.slider_int("##pad_gain", gain_val, 0, 100, "%d %")
        if changed:
            ctx.audio.pads.set_pad_gain(pad_id, new_gain / 100.0)


def _render_loaded_eq(ctx: UiContext, info: _SidebarPadInfo) -> None:
    knob_specs = (
        ("Low", "##pad_eq_low", ctx.state.project.pad_eq_low_db),
        ("Mid", "##pad_eq_mid", ctx.state.project.pad_eq_mid_db),
        ("High", "##pad_eq_high", ctx.state.project.pad_eq_high_db),
    )

    knob_width = 68.0

    # Vert. space for the knob labels
    imgui.dummy((info.avail_x, SPACING / 2))

    any_changed = False
    eq_values: list[float] = []

    with style_var(imgui.StyleVar_.item_spacing, (SPACING / 2, SPACING / 4)):
        for idx, (label, knob_id, source_attr) in enumerate(knob_specs):
            if idx > 0:
                imgui.same_line()

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

            any_changed = any_changed or changed
            eq_values.append(new_val if changed else knob_val)

    if any_changed:
        ctx.audio.pads.set_pad_eq(info.pad_id, eq_values[0], eq_values[1], eq_values[2])


def _render_loaded_actions(ctx: UiContext, pad_id: int) -> None:
    if imgui.button("Unload Audio", (-1, 0)):
        ctx.audio.pads.unload_sample(pad_id)

    if ctx.state.pads.is_analyzing(pad_id):
        imgui.text_disabled("Analyzing audio…")
        stage = ctx.state.pads.analysis_stage(pad_id) or "Analyzing"
        progress = ctx.state.pads.analysis_progress(pad_id)
        percent_text = "" if progress is None else f"{int(float(progress) * 100):d} %"
        status_line = " ".join([part for part in (stage, percent_text) if part])
        if status_line:
            imgui.text_colored(TEXT_MUTED_RGBA, status_line)
        return

    if imgui.button("Analyze audio", (-1, 0)):
        ctx.audio.pads.analyze_sample_async(pad_id)

    if imgui.button("Adjust Loop", (-1, 0)):
        # TODO: adjust loop
        pass

    if imgui.button("Generate Stems", (-1, 0)):
        # TODO: generate stems
        pass


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
    _render_bpm(ctx, info)
    _render_key(ctx, info)

    if info.is_loaded:
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
