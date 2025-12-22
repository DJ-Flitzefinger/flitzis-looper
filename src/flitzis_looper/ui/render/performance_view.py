from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.constants import GRID_SIZE, NUM_BANKS, NUM_PADS
from flitzis_looper.ui.constants import (
    BANK_BUTTONS_HEIGHT,
    PAD_GRID_GAP,
    SPACING,
    TEXT_ACTIVE_RGBA,
    TEXT_MUTED_RGBA,
)
from flitzis_looper.ui.contextmanager import button_style, style_var

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper.ui.styles import ButtonStyleName


def _pad_context_menu(ctx: UiContext, pad_id: int) -> None:
    if ctx.state.is_pad_loaded(pad_id):
        if imgui.menu_item("Unload Audio", "", p_selected=False)[0]:
            ctx.audio.unload_sample(pad_id)
        imgui.separator()
        if imgui.menu_item("Re-detect BPM", "", p_selected=False)[0]:
            # TODO: redetect BPM
            pass
        if imgui.menu_item("Adjust Loop", "", p_selected=False)[0]:
            # TODO: adjust loop
            pass
        imgui.separator()
        if imgui.menu_item("Generate Stems", "", p_selected=False)[0]:
            # TODO: generate stems
            pass
    elif imgui.menu_item("Load Audio", "", p_selected=False)[0]:
        ctx.ui.open_file_dialog(pad_id)


def _pad_popover(ctx: UiContext, pad_id: int) -> None:
    """Open popup if button is hovered and middle-clicked."""
    popup_id = f"ctx_popup_pad_{pad_id}"
    if imgui.is_item_hovered() and imgui.is_mouse_clicked(imgui.MouseButton_.middle):
        imgui.open_popup(popup_id)
    if imgui.begin_popup(popup_id):
        _pad_context_menu(ctx, pad_id)
        imgui.end_popup()


def _pad_button(ctx: UiContext, pad_id: int, size: imgui.ImVec2Like) -> None:
    is_loaded = ctx.state.is_pad_loaded(pad_id)
    is_active = ctx.state.is_pad_active(pad_id)
    style_name: ButtonStyleName = "active" if is_active else "regular"
    label = ctx.state.pad_label(pad_id) if is_loaded else ""
    id_str = f"pad_btn_{pad_id}"

    with button_style(style_name):
        imgui.button(f"{label}##{id_str}", size)

        # Track pressed state
        if imgui.is_item_hovered():
            # Play on left click
            if imgui.is_mouse_down(imgui.MouseButton_.left):
                if not ctx.state.is_pad_pressed(pad_id):
                    if is_loaded:
                        ctx.audio.trigger_pad(pad_id)
                    ctx.ui.select_pad(pad_id)
                ctx.ui.store_pressed_pad_state(pad_id, pressed=True)
            else:
                ctx.ui.store_pressed_pad_state(pad_id, pressed=False)

            # Stop on right click
            if imgui.is_mouse_down(imgui.MouseButton_.right):
                ctx.audio.stop_pad(pad_id)

    # Draw pad number
    pos_min = imgui.get_item_rect_min()
    label_pos = (pos_min.x + 6, pos_min.y + 4)
    draw_list = imgui.get_window_draw_list()
    label = f"#{pad_id + 1}"
    color = imgui.get_color_u32(TEXT_ACTIVE_RGBA if is_active else TEXT_MUTED_RGBA)
    draw_list.add_text(label_pos, color, label)

    _pad_popover(ctx, pad_id)


def _render_pad_grid(ctx: UiContext) -> None:
    avail = imgui.get_content_region_avail()
    total_spacing = (GRID_SIZE - 1) * PAD_GRID_GAP
    pad_width = (avail.x - total_spacing) / GRID_SIZE
    pad_height = (avail.y - total_spacing) / GRID_SIZE

    with style_var(imgui.StyleVar_.item_spacing, (0, 0)):
        pad_id = ctx.state.project.selected_bank * NUM_PADS
        for row in range(GRID_SIZE):
            for col in range(GRID_SIZE):
                if col > 0:
                    imgui.same_line(spacing=PAD_GRID_GAP)
                _pad_button(ctx, pad_id, (pad_width, pad_height))
                pad_id += 1

            # vertical gap
            if row < GRID_SIZE - 1:
                imgui.dummy((0, PAD_GRID_GAP))


def _bank_button(ctx: UiContext, idx: int, width: float) -> None:
    is_selected = ctx.state.is_bank_selected(idx)
    style_name: ButtonStyleName = "bank-active" if is_selected else "bank"

    with button_style(style_name):
        if imgui.button(f"Bank {idx + 1}", size=(width, -1)):
            ctx.ui.select_bank(idx)


def _render_banks(ctx: UiContext) -> None:
    avail = imgui.get_content_region_avail()
    total_spacing = (NUM_BANKS - 1) * PAD_GRID_GAP
    btn_width = (avail.x - total_spacing) / GRID_SIZE

    with style_var(imgui.StyleVar_.item_spacing, (0, 0)):
        for idx in range(NUM_BANKS):
            if idx > 0:
                imgui.same_line(spacing=PAD_GRID_GAP)
            _bank_button(ctx, idx, btn_width)


def performance_view(ctx: UiContext) -> None:
    avail = imgui.get_content_region_avail()

    with style_var(imgui.StyleVar_.item_spacing, (0, SPACING)):
        imgui.begin_child("pad_grid", (avail.x, avail.y - BANK_BUTTONS_HEIGHT - SPACING))
        _render_pad_grid(ctx)
        imgui.end_child()

        imgui.begin_child("bank_buttons", (avail.x, BANK_BUTTONS_HEIGHT))
        _render_banks(ctx)
        imgui.end_child()
