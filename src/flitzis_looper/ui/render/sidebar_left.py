from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA
from flitzis_looper.ui.contextmanager import style_var

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext


def sidebar_left(ctx: UiContext) -> None:
    avail = imgui.get_content_region_avail()
    pad_id = ctx.state.project.selected_pad
    is_loaded = ctx.state.is_pad_loaded(pad_id)
    is_loading = ctx.state.is_pad_loading(pad_id)
    load_error = ctx.state.pad_load_error(pad_id)

    # Filename
    filename = ctx.state.pad_label(pad_id) if is_loaded or is_loading else "- EMPTY -"
    text_width = imgui.calc_text_size(filename).x
    imgui.text_colored(TEXT_MUTED_RGBA, "Track")
    imgui.same_line(max(0, avail.x - text_width))
    if is_loaded or is_loading:
        imgui.text_unformatted(filename)
    else:
        imgui.text_colored(TEXT_MUTED_RGBA, filename)

    if load_error:
        imgui.text_wrapped(f"Load failed: {load_error}")

    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
        if is_loaded:
            if imgui.button("Unload Audio", (-1, 0)):
                ctx.audio.unload_sample(pad_id)
            if imgui.button("Re-detect BPM", (-1, 0)):
                # TODO: redetect BPM
                pass
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
