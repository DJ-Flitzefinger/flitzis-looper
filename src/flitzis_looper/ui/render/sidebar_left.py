from typing import TYPE_CHECKING

from imgui_bundle import imgui, imgui_knobs

from flitzis_looper.constants import PAD_EQ_DB_MAX, PAD_EQ_DB_MIN
from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA, TEXT_RGBA
from flitzis_looper.ui.contextmanager import style_var

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext


def sidebar_left(ctx: UiContext) -> None:  # noqa: C901, PLR0912, PLR0914, PLR0915
    avail = imgui.get_content_region_avail()
    pad_id = ctx.state.project.selected_pad
    is_loaded = ctx.state.is_pad_loaded(pad_id)
    is_loading = ctx.state.is_pad_loading(pad_id)

    def labeled_value_row(
        label: str, value: str, *, value_rgba: imgui.ImVec4Like = TEXT_RGBA
    ) -> None:
        imgui.text_colored(TEXT_MUTED_RGBA, label)
        text_width = imgui.calc_text_size(value).x
        imgui.same_line(max(0, avail.x - text_width))
        imgui.text_colored(value_rgba, value)

    # Pad
    labeled_value_row("Pad", f"#{pad_id + 1}")

    # Filename
    if is_loaded or is_loading:
        filename = ctx.state.pad_label(pad_id) or "-"
        labeled_value_row("Filename", filename)
    else:
        labeled_value_row("Filename", "- EMPTY -", value_rgba=TEXT_MUTED_RGBA)

    load_error = ctx.state.pad_load_error(pad_id)
    if load_error:
        imgui.text_wrapped(f"Load failed: {load_error}")

    # BPM
    effective_bpm = ctx.state.pad_effective_bpm(pad_id) if is_loaded else None
    bpm_value = 0.0 if effective_bpm is None else float(effective_bpm)

    imgui.text_colored(TEXT_MUTED_RGBA, "BPM")
    input_width = 92
    imgui.same_line(max(0, avail.x - input_width))
    imgui.set_next_item_width(input_width)
    gain_changed, new_value = imgui.input_float("##sidebar_bpm", bpm_value, 0.0, 0.0, "%.1f")
    if gain_changed:
        if new_value <= 0:
            ctx.audio.clear_manual_bpm(pad_id)
        else:
            ctx.audio.set_manual_bpm(pad_id, float(new_value))

    imgui.button("Tap BPM", (-1, 0))
    if imgui.is_item_hovered() and imgui.is_mouse_clicked(imgui.MouseButton_.left):
        ctx.audio.tap_bpm(pad_id)

    if imgui.button("Clear Manual BPM", (-1, 0)):
        ctx.audio.clear_manual_bpm(pad_id)

    # Key
    effective_key = ctx.state.pad_effective_key(pad_id) if is_loaded else None

    imgui.text_colored(TEXT_MUTED_RGBA, "Key")
    keys = [
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
    ]
    key_options = [*keys, *(f"{k}m" for k in keys)]

    input_width = 92
    imgui.same_line(max(0, avail.x - input_width))
    imgui.set_next_item_width(input_width)
    preview = effective_key or "-"
    if imgui.begin_combo("##sidebar_key", preview):
        for key in key_options:
            is_selected = key == effective_key
            if imgui.selectable(key, is_selected)[0]:
                ctx.audio.set_manual_key(pad_id, key)
            if is_selected:
                imgui.set_item_default_focus()
        imgui.end_combo()

    if imgui.button("Clear Manual Key", (-1, 0)):
        ctx.audio.clear_manual_key(pad_id)

    analysis_error = ctx.state.pad_analysis_error(pad_id) if is_loaded else None
    if analysis_error:
        imgui.text_wrapped(f"Analysis failed: {analysis_error}")

    if is_loaded:
        imgui.separator()

        # Pad gain
        with style_var(imgui.StyleVar_.item_spacing, (0, SPACING / 2)):
            imgui.text_colored(TEXT_MUTED_RGBA, "Gain")
            imgui.set_next_item_width(-1)
            gain_val = max(0, min(100, round(ctx.state.project.pad_gain[pad_id] * 100)))
            gain_changed, new_gain = imgui.slider_int("##pad_gain", gain_val, 0, 100, "%d %")
            if gain_changed:
                ctx.audio.set_pad_gain(pad_id, new_gain / 100.0)

        # Pad EQ
        knob_data = [
            ("Low", "##pad_eq_low", ctx.state.project.pad_eq_low_db),
            ("Mid", "##pad_eq_mid", ctx.state.project.pad_eq_mid_db),
            ("High", "##pad_eq_high", ctx.state.project.pad_eq_high_db),
        ]

        any_knob_changed = False
        new_values = {}
        knob_width = 68

        # Vert. space for the knob labels
        imgui.dummy((avail.x, SPACING / 2))

        with style_var(imgui.StyleVar_.item_spacing, (SPACING / 2, SPACING / 4)):
            for i, (label, knob_id, source_attr) in enumerate(knob_data):
                if i > 0:
                    imgui.same_line()

                # Knob
                knob_val = float(source_attr[pad_id])
                gain_changed, new_val = imgui_knobs.knob(
                    knob_id,
                    p_value=knob_val,
                    v_min=PAD_EQ_DB_MIN,
                    v_max=PAD_EQ_DB_MAX,
                    format="%.1f dB",
                    size=knob_width,
                )

                # Draw knob label above
                draw_list = imgui.get_window_draw_list()
                text_width = imgui.calc_text_size(label).x
                label_x = imgui.get_item_rect_min().x + (knob_width - text_width) * 0.5
                label_y = imgui.get_item_rect_min().y - imgui.get_text_line_height_with_spacing()
                draw_list.add_text((label_x, label_y), imgui.get_color_u32(TEXT_MUTED_RGBA), label)

                if gain_changed:
                    any_knob_changed = True
                    new_values[i] = new_val
                else:
                    new_values[i] = knob_val

            if any_knob_changed:
                ctx.audio.set_pad_eq(pad_id, new_values[0], new_values[1], new_values[2])

    imgui.separator()

    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
        if is_loaded:
            if imgui.button("Unload Audio", (-1, 0)):
                ctx.audio.unload_sample(pad_id)
            if ctx.state.is_pad_analyzing(pad_id):
                imgui.text_disabled("Analyzing audio…")
                stage = ctx.state.pad_analysis_stage(pad_id) or "Analyzing"
                progress = ctx.state.pad_analysis_progress(pad_id)
                percent_text = "" if progress is None else f"{int(float(progress) * 100):d} %"
                status_line = " ".join([p for p in (stage, percent_text) if p])
                if status_line:
                    imgui.text_colored(TEXT_MUTED_RGBA, status_line)
            elif imgui.button("Analyze audio", (-1, 0)):
                ctx.audio.analyze_sample_async(pad_id)
            if imgui.button("Adjust Loop", (-1, 0)):
                # TODO: adjust loop
                pass
            if imgui.button("Generate Stems", (-1, 0)):
                # TODO: generate stems
                pass
        elif is_loading:
            stage = ctx.state.pad_load_stage(pad_id) or "Loading"
            progress = ctx.state.pad_load_progress(pad_id)
            percent_text = "" if progress is None else f"{int(float(progress) * 100):d} %"
            status_line = " ".join([p for p in (stage, percent_text) if p])
            imgui.text_colored(TEXT_MUTED_RGBA, status_line or "Loading…")
        elif imgui.button("Load Audio", (-1, 0)):
            ctx.ui.open_file_dialog(pad_id)
