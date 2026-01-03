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
    if ctx.state.pads.is_loading(pad_id):
        imgui.text_disabled("Loading audio…")
        return

    if ctx.state.pads.is_loaded(pad_id):
        if imgui.menu_item("Unload Audio", "", p_selected=False)[0]:
            ctx.audio.pads.unload_sample(pad_id)
        imgui.separator()
        if ctx.state.pads.is_analyzing(pad_id):
            imgui.text_disabled("Analyze audio")
        elif imgui.menu_item("Analyze audio", "", p_selected=False)[0]:
            ctx.audio.pads.analyze_sample_async(pad_id)
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


def _pad_button_label(
    ctx: UiContext, pad_id: int, *, is_loaded: bool, is_loading: bool
) -> tuple[str, float | None]:
    if not (is_loaded or is_loading):
        return "", None

    filename = ctx.state.pads.label(pad_id)

    if is_loading:
        stage = ctx.state.pads.load_stage(pad_id) or "Loading"
        progress = ctx.state.pads.load_progress(pad_id)
        loading_progress = float(progress) if isinstance(progress, (int, float)) else None
        percent_text = "" if loading_progress is None else f"{int(loading_progress * 100):d} %"
        status_line = " ".join([p for p in (stage, percent_text) if p])
        label = f"{filename}\n{status_line}" if filename else (status_line or "Loading…")
        return label, loading_progress

    if ctx.state.pads.is_analyzing(pad_id):
        stage = ctx.state.pads.analysis_stage(pad_id) or "Analyzing"
        progress = ctx.state.pads.analysis_progress(pad_id)
        percent_text = "" if progress is None else f"{int(float(progress) * 100):d} %"
        status_line = " ".join([p for p in (stage, percent_text) if p])
        label = f"{filename}\n{status_line}" if filename else (status_line or "Analyzing…")
        return label, None

    return filename, None


def _pad_button_progress_overlay(progress: float) -> None:
    pos_min = imgui.get_item_rect_min()
    pos_max = imgui.get_item_rect_max()
    width = pos_max.x - pos_min.x
    fill_x = pos_min.x + width * max(0.0, min(1.0, progress))

    base = imgui.get_style_color_vec4(imgui.Col_.button)
    progress_rgba = (
        base.x * 0.6,
        base.y * 0.6,
        base.z * 0.6,
        min(base.w, 1.0) * 0.5,
    )

    draw_list = imgui.get_window_draw_list()
    draw_list.add_rect_filled(pos_min, (fill_x, pos_max.y), imgui.get_color_u32(progress_rgba))


def _pad_button_peak_meter(peak: float) -> None:
    peak = max(0.0, min(1.0, float(peak)))

    pos_min = imgui.get_item_rect_min()
    pos_max = imgui.get_item_rect_max()
    meter_w = 6.0
    padding = 2.0
    x1 = pos_max.x - padding - meter_w
    x2 = pos_max.x - padding
    y2 = pos_max.y - padding
    y_min = pos_min.y + padding
    height = max(0.0, y2 - y_min)
    y1 = y2 - height * peak

    bg_rgba = (0.0, 0.0, 0.0, 0.25)
    fg_rgba = (1.0, 0.2, 0.2, 0.9) if peak >= 1.0 else (0.2, 0.8, 0.2, 0.7)

    draw_list = imgui.get_window_draw_list()
    draw_list.add_rect_filled((x1, y_min), (x2, y2), imgui.get_color_u32(bg_rgba))
    if peak > 0.0:
        draw_list.add_rect_filled((x1, y1), (x2, y2), imgui.get_color_u32(fg_rgba))


def _pad_button_input(ctx: UiContext, pad_id: int, *, is_loaded: bool) -> None:
    if imgui.is_mouse_down(imgui.MouseButton_.left):
        if not ctx.state.pads.is_pressed(pad_id):
            if is_loaded:
                ctx.audio.pads.trigger_pad(pad_id)
            ctx.ui.select_pad(pad_id)
        ctx.ui.store_pressed_pad_state(pad_id, pressed=True)
    else:
        ctx.ui.store_pressed_pad_state(pad_id, pressed=False)

    if imgui.is_mouse_down(imgui.MouseButton_.right):
        ctx.audio.pads.stop_pad(pad_id)


def _pad_button_overlays(ctx: UiContext, pad_id: int, *, is_active: bool, is_loaded: bool) -> None:
    pos_min = imgui.get_item_rect_min()
    draw_list = imgui.get_window_draw_list()

    # Pad number
    label_pos = (pos_min.x + 6, pos_min.y + 4)
    label = f"#{pad_id + 1}"
    color = imgui.get_color_u32(TEXT_ACTIVE_RGBA if is_active else TEXT_MUTED_RGBA)
    draw_list.add_text(label_pos, color, label)

    bpm = ctx.state.pads.effective_bpm(pad_id) if is_loaded else None
    key = ctx.state.pads.effective_key(pad_id) if is_loaded else None

    info = None
    if bpm is not None:
        info = f"{bpm:.1f}"
        if key is not None:
            info = f"{info} {key}"
    elif key is not None:
        info = key

    if info is None:
        return

    text_size = imgui.calc_text_size(info)
    pos_max = imgui.get_item_rect_max()
    info_pos = (pos_max.x - text_size.x - 6, pos_min.y + 4)
    info_color = TEXT_ACTIVE_RGBA if is_active else TEXT_MUTED_RGBA
    draw_list.add_text(info_pos, imgui.get_color_u32(info_color), info)


def _pad_button(ctx: UiContext, pad_id: int, size: imgui.ImVec2Like) -> None:
    is_loaded = ctx.state.pads.is_loaded(pad_id)
    is_loading = ctx.state.pads.is_loading(pad_id)
    is_active = ctx.state.pads.is_active(pad_id)
    style_name: ButtonStyleName = "active" if is_active else "regular"

    label, loading_progress = _pad_button_label(
        ctx, pad_id, is_loaded=is_loaded, is_loading=is_loading
    )
    id_str = f"pad_btn_{pad_id}"

    with button_style(style_name):
        imgui.button(f"{label}##{id_str}", size)

        if is_loading and loading_progress is not None:
            _pad_button_progress_overlay(loading_progress)

        if is_loaded:
            _pad_button_peak_meter(ctx.state.pads.peak(pad_id))

        if imgui.is_item_hovered():
            _pad_button_input(ctx, pad_id, is_loaded=is_loaded)

    _pad_button_overlays(ctx, pad_id, is_active=is_active, is_loaded=is_loaded)
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

            if row < GRID_SIZE - 1:
                imgui.dummy((0, PAD_GRID_GAP))


def _bank_button(ctx: UiContext, idx: int, width: float) -> None:
    is_selected = ctx.state.banks.is_selected(idx)
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
