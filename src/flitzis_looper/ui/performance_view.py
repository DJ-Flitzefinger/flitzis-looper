from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.constants import GRID_SIZE, NUM_BANKS, PADS_PER_BANK
from flitzis_looper.ui.constants import (
    BANK_BUTTONS_HEIGHT,
    PAD_GRID_GAP,
    SPACING,
    TEXT_ACTIVE_RGBA,
    TEXT_MUTED_RGBA,
)
from flitzis_looper.ui.context import button_style, style_var

if TYPE_CHECKING:
    from flitzis_looper.app import FlitzisLooperApp
    from flitzis_looper.ui.styles import ButtonStyleName


def _pad_context_menu(app: FlitzisLooperApp, pad_id: int) -> None:
    if app.is_sample_loaded(pad_id):
        if imgui.menu_item("Unload Audio", "", p_selected=False)[0]:
            app.unload_sample(pad_id)
        imgui.separator()
        if imgui.menu_item("Re-detect BPM", "", p_selected=False)[0]:
            # TODO: redetect BPM
            pass
        if imgui.menu_item("Adjust Loop", "", p_selected=False)[0]:
            # TODO: adjust loop
            pass
        imgui.separator()
        if imgui.menu_item("Generate Stems", "", p_selected=False)[0]:
            print("Generate stems triggered")
    elif imgui.menu_item("Load Audio", "", p_selected=False)[0]:
        app.open_file_dialog(pad_id)


def _pad_popover(app: FlitzisLooperApp, pad_id: int) -> None:
    """Open popup if button is hovered and middle-clicked."""
    popup_id = f"ctx_popup_pad_{pad_id}"
    if imgui.is_item_hovered() and imgui.is_mouse_clicked(imgui.MouseButton_.middle):
        imgui.open_popup(popup_id)
    if imgui.begin_popup(popup_id):
        _pad_context_menu(app, pad_id)
        imgui.end_popup()


def _pad_button(app: FlitzisLooperApp, pad_id: int, size: imgui.ImVec2Like) -> None:
    is_loaded = app.is_sample_loaded(pad_id)
    is_active = pad_id in app.state.active_sample_ids
    style_name: ButtonStyleName = "active" if is_active else "regular"
    label = app.pad_label(pad_id) if is_loaded else ""
    id_str = f"pad_btn_{pad_id}"

    with button_style(style_name):
        imgui.button(f"{label}##{id_str}", size)

        # Track pressed state
        if imgui.is_item_hovered():
            if imgui.is_mouse_down(imgui.MouseButton_.left):
                if not app.state.pressed_pads[pad_id]:
                    if is_loaded:
                        app.trigger_pad(pad_id)
                    app.select_pad(pad_id)
                app.state.pressed_pads[pad_id] = True
            else:
                app.state.pressed_pads[pad_id] = False
            if imgui.is_mouse_down(imgui.MouseButton_.right):
                app.stop_pad(pad_id)

    # Draw pad number
    pos_min = imgui.get_item_rect_min()
    label_pos = (pos_min.x + 6, pos_min.y + 4)
    draw_list = imgui.get_window_draw_list()
    label = f"#{pad_id + 1}"
    color = imgui.get_color_u32(TEXT_ACTIVE_RGBA if is_active else TEXT_MUTED_RGBA)
    draw_list.add_text(label_pos, color, label)

    _pad_popover(app, pad_id)


def _render_pad_grid(app: FlitzisLooperApp) -> None:
    avail = imgui.get_content_region_avail()
    total_spacing = (GRID_SIZE - 1) * PAD_GRID_GAP
    pad_width = (avail.x - total_spacing) / GRID_SIZE
    pad_height = (avail.y - total_spacing) / GRID_SIZE

    with style_var(imgui.StyleVar_.item_spacing, (0, 0)):
        pad_id = app.state.selected_bank * PADS_PER_BANK
        for row in range(GRID_SIZE):
            for col in range(GRID_SIZE):
                if col > 0:
                    imgui.same_line(spacing=PAD_GRID_GAP)
                _pad_button(app, pad_id, (pad_width, pad_height))
                pad_id += 1

            # vertical gap
            if row < GRID_SIZE - 1:
                imgui.dummy((0, PAD_GRID_GAP))


def _bank_button(app: FlitzisLooperApp, idx: int, width: float) -> None:
    is_active = app.state.selected_bank == idx
    style_name: ButtonStyleName = "bank-active" if is_active else "bank"

    with button_style(style_name):
        if imgui.button(f"Bank {idx + 1}", size=(width, -1)):
            app.select_bank(idx)


def _render_banks(app: FlitzisLooperApp) -> None:
    avail = imgui.get_content_region_avail()
    total_spacing = (NUM_BANKS - 1) * PAD_GRID_GAP
    btn_width = (avail.x - total_spacing) / GRID_SIZE

    with style_var(imgui.StyleVar_.item_spacing, (0, 0)):
        for idx in range(NUM_BANKS):
            if idx > 0:
                imgui.same_line(spacing=PAD_GRID_GAP)
            _bank_button(app, idx, btn_width)


def performance_view(app: FlitzisLooperApp) -> None:
    avail = imgui.get_content_region_avail()

    with style_var(imgui.StyleVar_.item_spacing, (0, SPACING)):
        imgui.begin_child("pad_grid", (avail.x, avail.y - BANK_BUTTONS_HEIGHT - SPACING))
        _render_pad_grid(app)
        imgui.end_child()

        imgui.begin_child("bank_buttons", (avail.x, BANK_BUTTONS_HEIGHT))
        _render_banks(app)
        imgui.end_child()
