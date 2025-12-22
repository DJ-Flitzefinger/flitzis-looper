from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA
from flitzis_looper.ui.context import style_var

if TYPE_CHECKING:
    from flitzis_looper.app import FlitzisLooperApp


def sidebar_left(app: FlitzisLooperApp) -> None:
    avail = imgui.get_content_region_avail()
    pad_id = app.state.selected_pad
    is_loaded = app.is_sample_loaded(pad_id)

    # Filename
    filename = app.pad_label(pad_id) if is_loaded else "None"
    text_width = imgui.calc_text_size(filename).x
    imgui.text_colored(TEXT_MUTED_RGBA, "Track")
    imgui.same_line(max(0, avail.x - text_width))
    imgui.text_unformatted(filename)

    with style_var(imgui.StyleVar_.item_spacing, (0.0, SPACING / 4)):
        if is_loaded:
            if imgui.button("Unload Audio", (-1, 0)):
                app.unload_sample(pad_id)
            if imgui.button("Re-detect BPM", (-1, 0)):
                # TODO: redetect BPM
                pass
            if imgui.button("Adjust Loop", (-1, 0)):
                # TODO: adjust loop
                pass
            if imgui.button("Generate Stems", (-1, 0)):
                # TODO: adjust loop
                pass
        elif imgui.button("Load Audio", (-1, 0)):
            app.open_file_dialog(pad_id)
