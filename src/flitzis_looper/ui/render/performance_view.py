from typing import TYPE_CHECKING, cast

from imgui_bundle import imgui

from flitzis_looper.constants import GRID_SIZE, NUM_BANKS, NUM_PADS
from flitzis_looper.ui.constants import (
    BANK_BUTTONS_HEIGHT,
    BANK_PRESSED_RGBA,
    CONTROL_RGBA,
    MODE_OFF_RGBA,
    MODE_ON_RGBA,
    PAD_GRID_GAP,
    SPACING,
    TEXT_ACTIVE_RGBA,
    TEXT_MUTED_RGBA,
    TEXT_RGBA,
)
from flitzis_looper.ui.contextmanager import button_style, style_var

if TYPE_CHECKING:
    from flitzis_looper.models import StemGridIndicatorState
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper.ui.styles import ButtonStyleName

STEM_GRID_INDICATORS: dict[str, tuple[str, imgui.ImVec4Like, imgui.ImVec4Like]] = {
    "available": ("ST", MODE_ON_RGBA, TEXT_ACTIVE_RGBA),
    "generating": ("...", BANK_PRESSED_RGBA, TEXT_ACTIVE_RGBA),
    "blocked": ("BLK", CONTROL_RGBA, TEXT_RGBA),
    "error": ("!", MODE_OFF_RGBA, TEXT_RGBA),
}
PAD_TITLE_MAX_LINES = 3
PAD_TITLE_PADDING_SAMPLE = "M"
PAD_TITLE_FALLBACK_PADDING_PX = 8.0


def _text_width(text: str) -> float:
    return float(imgui.calc_text_size(text).x)


def _fit_text_to_width(text: str, max_width: float) -> str:
    if max_width <= 0.0 or _text_width(text) <= max_width:
        return text

    ellipsis = "..."
    ellipsis_width = _text_width(ellipsis)
    if ellipsis_width >= max_width:
        return ""

    fitted = ""
    target_width = max_width - ellipsis_width
    for char in text:
        candidate = f"{fitted}{char}"
        if _text_width(candidate) > target_width:
            break
        fitted = candidate

    return f"{fitted.rstrip()}{ellipsis}" if fitted else ellipsis


def _split_word_to_width(word: str, max_width: float) -> list[str]:
    if max_width <= 0.0 or _text_width(word) <= max_width:
        return [word]

    parts: list[str] = []
    current = ""
    for char in word:
        candidate = f"{current}{char}"
        if current and _text_width(candidate) > max_width:
            parts.append(current)
            current = char
        else:
            current = candidate

    if current:
        parts.append(current)
    return parts or [word]


def wrapped_text_lines(text: str, *, max_width: float, max_lines: int) -> list[str]:
    """Return pixel-width wrapped text lines for compact controls."""
    if max_lines <= 0:
        return []

    words = text.split()
    if not words:
        return [""]

    lines: list[str] = []
    current = ""
    for word in words:
        for segment in _split_word_to_width(word, max_width):
            candidate = segment if not current else f"{current} {segment}"
            if current and _text_width(candidate) > max_width:
                lines.append(current)
                current = segment
            else:
                current = candidate

    if current:
        lines.append(current)

    if len(lines) > max_lines:
        lines = lines[:max_lines]
        lines[-1] = _fit_text_to_width(f"{lines[-1]}...", max_width)

    return lines


def pad_title_horizontal_padding() -> float:
    """Return approximately one character of horizontal pad-title inset."""
    width = _text_width(PAD_TITLE_PADDING_SAMPLE)
    return width if width > 0.0 else PAD_TITLE_FALLBACK_PADDING_PX


def wrap_pad_title(title: str, *, pad_width: float, max_lines: int = PAD_TITLE_MAX_LINES) -> str:
    """Return a pad title label constrained to the padded pad width."""
    content_width = max(1.0, pad_width - pad_title_horizontal_padding() * 2.0)
    return "\n".join(wrapped_text_lines(title, max_width=content_width, max_lines=max_lines))


def _vec2_width(size: imgui.ImVec2Like) -> float:
    if isinstance(size, (tuple, list)):
        return float(size[0])
    return float(size.x)


def _pad_button_label(
    ctx: UiContext,
    pad_id: int,
    *,
    is_loaded: bool,
    is_loading: bool,
    pad_width: float,
) -> tuple[str, float | None]:
    if not (is_loaded or is_loading):
        return "", None

    filename = ctx.state.pads.label(pad_id)
    filename_label = wrap_pad_title(filename, pad_width=pad_width) if filename else ""
    stage: str | None

    if is_loading:
        stage = ctx.state.pads.load_stage(pad_id) or "Loading"
        progress = ctx.state.pads.load_progress(pad_id)
        loading_progress = float(progress) if isinstance(progress, (int, float)) else None
        percent_text = "" if loading_progress is None else f"{int(loading_progress * 100):d} %"
        status_line = " ".join([p for p in (stage, percent_text) if p])
        label = (
            f"{filename_label}\n{status_line}" if filename_label else (status_line or "Loading…")
        )
        return label, loading_progress

    if ctx.state.pads.is_analyzing(pad_id):
        stage, progress = ctx.state.pads.analysis_status(pad_id)
        stage = stage or "Analyzing"
        percent_text = "" if progress is None else f"{int(float(progress) * 100):d} %"
        status_line = " ".join([p for p in (stage, percent_text) if p])
        label = (
            f"{filename_label}\n{status_line}" if filename_label else (status_line or "Analyzing…")
        )
        return label, None

    return filename_label, None


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


def stem_grid_indicator_label(state: StemGridIndicatorState | None) -> str | None:
    """Return the compact pad-grid indicator label for tests and rendering."""
    if state is None:
        return None
    return STEM_GRID_INDICATORS[state][0]


def _pad_button_stem_indicator(ctx: UiContext, pad_id: int) -> None:
    state = ctx.state.stems.stem_grid_indicator_state(pad_id)
    if state is None:
        return

    label, bg_rgba, text_rgba = STEM_GRID_INDICATORS[state]
    pos_min = imgui.get_item_rect_min()
    pos_max = imgui.get_item_rect_max()
    text_size = imgui.calc_text_size(label)
    padding = (5.0, 2.0)
    margin = 5.0

    x1 = pos_min.x + margin
    y2 = pos_max.y - margin
    x2 = x1 + text_size.x + padding[0] * 2.0
    y1 = y2 - text_size.y - padding[1] * 2.0

    draw_list = imgui.get_window_draw_list()
    draw_list.add_rect_filled((x1, y1), (x2, y2), imgui.get_color_u32(bg_rgba))
    draw_list.add_text(
        (x1 + padding[0], y1 + padding[1]),
        imgui.get_color_u32(text_rgba),
        label,
    )


def _pad_button_input(ctx: UiContext, pad_id: int, *, is_loaded: bool) -> None:
    if imgui.is_mouse_clicked(imgui.MouseButton_.middle):
        ctx.ui.select_pad(pad_id)

    if imgui.is_mouse_down(imgui.MouseButton_.left):
        if not ctx.state.pads.is_pressed(pad_id):
            if is_loaded:
                ctx.audio.pads.trigger_pad(pad_id)
            ctx.ui.select_pad(pad_id)
        ctx.ui.store_pressed_pad_state(pad_id, pressed=True)
    else:
        ctx.ui.store_pressed_pad_state(pad_id, pressed=False)

    if imgui.is_mouse_clicked(imgui.MouseButton_.right):
        ctx.ui.select_pad(pad_id)

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

    _pad_button_stem_indicator(ctx, pad_id)

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
    is_selected = ctx.state.pads.is_selected(pad_id)

    style_name = "active" if is_active else "regular"
    if is_selected:
        style_name += "-selected"

    pad_width = _vec2_width(size)
    label, loading_progress = _pad_button_label(
        ctx,
        pad_id,
        is_loaded=is_loaded,
        is_loading=is_loading,
        pad_width=pad_width,
    )
    id_str = f"pad_btn_{pad_id}"

    with button_style(cast("ButtonStyleName", style_name)):
        imgui.button(f"{label}##{id_str}", size)

        if is_loading and loading_progress is not None:
            _pad_button_progress_overlay(loading_progress)

        if imgui.is_item_hovered():
            _pad_button_input(ctx, pad_id, is_loaded=is_loaded)

    _pad_button_overlays(ctx, pad_id, is_active=is_active, is_loaded=is_loaded)


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
