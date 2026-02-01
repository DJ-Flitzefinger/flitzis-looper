import math
from dataclasses import dataclass
from typing import TYPE_CHECKING, cast

from imgui_bundle import ImVec4, imgui, imgui_ctx, implot

from flitzis_looper.ui.constants import (
    PLOT_FILL_RGBA,
    PLOT_MARKER_RGBA,
    PLOT_PLAYHEAD_RGBA,
    PLOT_REGION_FILL_RGBA,
    PLOT_REGION_RGBA,
    SPACING,
    TEXT_MUTED_RGBA,
)
from flitzis_looper.ui.contextmanager import implot_style_color, implot_style_var

if TYPE_CHECKING:
    from collections.abc import Callable

    import numpy as np
    from numpy.typing import NDArray

    from flitzis_looper.ui.context import UiContext

BTN_SIZE = (88, 0)

LOOP_START_DRAG_LINE_ID = 0
LOOP_END_DRAG_LINE_ID = 1


@dataclass(slots=True)
class _GridOffsetDragState:
    pad_id: int | None = None
    button: int | None = None
    accum_x: float = 0.0


_GRID_OFFSET_PX_PER_STEP = 2.0
_GRID_OFFSET_DRAG = _GridOffsetDragState()


def _icon_button(
    label: str,
    size: tuple[float, float],
    draw_icon: Callable[[imgui.ImDrawList, imgui.ImVec2, imgui.ImVec2, int], None],
) -> bool:
    """Render an icon-only button.

    Returns True on mouse-down (press), not release.
    """
    imgui.button(label, size)

    pressed = imgui.is_item_hovered() and imgui.is_mouse_clicked(imgui.MouseButton_.left)

    draw_list = imgui.get_window_draw_list()
    pos_min = imgui.get_item_rect_min()
    pos_max = imgui.get_item_rect_max()
    col = imgui.get_color_u32(imgui.get_style_color_vec4(imgui.Col_.text))
    draw_icon(draw_list, pos_min, pos_max, col)

    return pressed


def _draw_pause_icon(
    draw_list: imgui.ImDrawList, pos_min: imgui.ImVec2, pos_max: imgui.ImVec2, col: int
) -> None:
    w = pos_max.x - pos_min.x
    h = pos_max.y - pos_min.y
    pad = max(1.0, min(w, h) * 0.25)

    icon_w = max(0.0, w - 2 * pad)
    bar_w = max(1.0, icon_w * 0.28)
    gap = max(1.0, icon_w * 0.18)

    cx = (pos_min.x + pos_max.x) * 0.5
    y1 = pos_min.y + pad
    y2 = pos_max.y - pad

    x1 = cx - gap * 0.5 - bar_w
    x2 = cx - gap * 0.5
    x3 = cx + gap * 0.5
    x4 = cx + gap * 0.5 + bar_w

    draw_list.add_rect_filled((x1, y1), (x2, y2), col)
    draw_list.add_rect_filled((x3, y1), (x4, y2), col)


def _draw_stop_icon(
    draw_list: imgui.ImDrawList, pos_min: imgui.ImVec2, pos_max: imgui.ImVec2, col: int
) -> None:
    w = pos_max.x - pos_min.x
    h = pos_max.y - pos_min.y
    pad = max(1.0, min(w, h) * 0.28)

    x1 = pos_min.x + pad
    y1 = pos_min.y + pad
    x2 = pos_max.x - pad
    y2 = pos_max.y - pad

    draw_list.add_rect_filled((x1, y1), (x2, y2), col)


def _draw_triangle_icon(
    draw_list: imgui.ImDrawList,
    pos_min: imgui.ImVec2,
    pos_max: imgui.ImVec2,
    col: int,
    *,
    direction: int,
) -> None:
    w = pos_max.x - pos_min.x
    h = pos_max.y - pos_min.y
    pad = max(1.0, min(w, h) * 0.25)

    x1 = pos_min.x + pad
    x2 = pos_max.x - pad
    y1 = pos_min.y + pad
    y2 = pos_max.y - pad
    cy = (y1 + y2) * 0.5

    if direction < 0:
        p1 = (x2, y1)
        p2 = (x2, y2)
        p3 = (x1, cy)
    else:
        p1 = (x1, y1)
        p2 = (x1, y2)
        p3 = (x2, cy)

    draw_list.add_triangle_filled(p1, p2, p3, col)


def _render_view_jump_button(
    ctx: UiContext,
    label: str,
    direction: int,
    jump_func: Callable[[], tuple[float, float] | None],
) -> None:
    btn_wh = float(imgui.get_frame_height())
    btn_size = (btn_wh, btn_wh)

    if _icon_button(
        label,
        btn_size,
        lambda dl, mn, mx, c: _draw_triangle_icon(dl, mn, mx, c, direction=direction),
    ):
        limits = jump_func()
        if limits is not None:
            implot.set_next_axis_limits(implot.ImAxis_.x1, limits[0], limits[1], imgui.Cond_.always)


def _render_controls(ctx: UiContext, pad_id: int) -> None:
    btn_wh = float(imgui.get_frame_height())
    btn_size = (btn_wh, btn_wh)

    if _icon_button("##wf_pause", btn_size, _draw_pause_icon):
        ctx.ui.waveform.pause_selected_pad_on_press()

    imgui.same_line(spacing=SPACING)
    if _icon_button(
        "##wf_play", btn_size, lambda dl, mn, mx, c: _draw_triangle_icon(dl, mn, mx, c, direction=1)
    ):
        ctx.ui.waveform.play_restart_selected_pad_on_press()

    imgui.same_line(spacing=SPACING)
    if _icon_button("##wf_stop", btn_size, _draw_stop_icon):
        ctx.ui.waveform.stop_and_reset_selected_pad_on_press()

    imgui.same_line(spacing=SPACING)
    _render_view_jump_button(
        ctx,
        "##wf_view_jump_start",
        -1,
        ctx.ui.waveform.view_jump_start_selected_pad_on_press,
    )

    imgui.same_line(spacing=SPACING)
    _render_view_jump_button(
        ctx,
        "##wf_view_jump_end",
        1,
        ctx.ui.waveform.view_jump_end_selected_pad_on_press,
    )

    imgui.same_line(spacing=SPACING)
    if imgui.button("Reset Loop", BTN_SIZE):
        ctx.audio.pads.reset_pad_loop_region(pad_id)

    imgui.same_line(spacing=SPACING)
    if imgui.button("Reset Zoom", BTN_SIZE):
        dur_s = ctx.state.project.sample_durations[pad_id]
        if dur_s is not None:
            implot.set_next_axis_limits(implot.ImAxis_.x1, 0.0, dur_s, imgui.Cond_.always)


def _render_loop_controls(ctx: UiContext, pad_id: int) -> None:
    auto_enabled = ctx.state.project.pad_loop_auto[pad_id]

    imgui.same_line(spacing=SPACING)
    changed, new_auto = imgui.checkbox("Auto-loop", auto_enabled)
    if changed:
        ctx.audio.pads.set_pad_loop_auto(pad_id, enabled=new_auto)

    bars = ctx.state.project.pad_loop_bars[pad_id]
    bpm = ctx.state.pads.effective_bpm(pad_id)

    imgui.same_line(spacing=SPACING)
    if bpm is None and new_auto:
        imgui.text_colored(TEXT_MUTED_RGBA, f"Bars: {bars} (BPM unavailable)")
        _render_grid_offset_control(ctx, pad_id)
        return

    imgui.text_colored(TEXT_MUTED_RGBA, "Bars")
    imgui.same_line(spacing=SPACING / 2)
    imgui.text(str(bars))

    imgui.same_line(spacing=SPACING / 2)
    if imgui.button("-", (24, 0)):
        ctx.audio.pads.set_pad_loop_bars(pad_id, bars=max(1, bars - 1))

    imgui.same_line(spacing=SPACING / 2)
    if imgui.button("+", (24, 0)):
        ctx.audio.pads.set_pad_loop_bars(pad_id, bars=bars + 1)

    _render_grid_offset_control(ctx, pad_id)


def _render_grid_offset_control(ctx: UiContext, pad_id: int) -> None:
    offset = int(ctx.state.project.pad_grid_offset_samples[pad_id])
    offset_text = "0" if offset == 0 else f"{offset:+d}"

    imgui.same_line(spacing=SPACING)
    imgui.text_colored(TEXT_MUTED_RGBA, "Grid Offset")
    imgui.same_line(spacing=SPACING / 2)

    imgui.button(f"{offset_text} smp##grid_offset_samples", BTN_SIZE)
    hovered = imgui.is_item_hovered()

    if hovered and imgui.is_mouse_clicked(imgui.MouseButton_.left):
        _GRID_OFFSET_DRAG.pad_id = pad_id
        _GRID_OFFSET_DRAG.button = imgui.MouseButton_.left
        _GRID_OFFSET_DRAG.accum_x = 0.0

    if hovered and imgui.is_mouse_clicked(imgui.MouseButton_.right):
        _GRID_OFFSET_DRAG.pad_id = pad_id
        _GRID_OFFSET_DRAG.button = imgui.MouseButton_.right
        _GRID_OFFSET_DRAG.accum_x = 0.0

    if _GRID_OFFSET_DRAG.pad_id != pad_id or _GRID_OFFSET_DRAG.button is None:
        return

    button = _GRID_OFFSET_DRAG.button
    if not imgui.is_mouse_down(button):
        _GRID_OFFSET_DRAG.pad_id = None
        _GRID_OFFSET_DRAG.button = None
        _GRID_OFFSET_DRAG.accum_x = 0.0
        return

    step = 1 if button == imgui.MouseButton_.left else 10
    _GRID_OFFSET_DRAG.accum_x += imgui.get_io().mouse_delta.x

    steps = 0
    if _GRID_OFFSET_DRAG.accum_x >= _GRID_OFFSET_PX_PER_STEP:
        steps = int(_GRID_OFFSET_DRAG.accum_x / _GRID_OFFSET_PX_PER_STEP)
    elif _GRID_OFFSET_DRAG.accum_x <= -_GRID_OFFSET_PX_PER_STEP:
        steps = -int(abs(_GRID_OFFSET_DRAG.accum_x) / _GRID_OFFSET_PX_PER_STEP)

    if steps == 0:
        return

    ctx.audio.pads.set_pad_grid_offset_samples(pad_id, offset + steps * step)
    _GRID_OFFSET_DRAG.accum_x -= steps * _GRID_OFFSET_PX_PER_STEP


def _plot_overlay_loop_region(
    ctx: UiContext,
    pad_id: int,
    start_s: float,
    draw_list: imgui.ImDrawList,
    sample_duration_s: float,
) -> None:
    loop_start_s, loop_end_s = ctx.state.pads.effective_loop_region(pad_id)
    playhead_s = ctx.state.session.pad_playhead_s[pad_id]

    # Loop region
    if loop_end_s is not None:
        # Start line
        start_dragging, start_x, _, _, _ = implot.drag_line_x(
            LOOP_START_DRAG_LINE_ID, loop_start_s, PLOT_REGION_RGBA
        )
        if start_dragging:
            new_start = max(0.0, start_x)
            new_start = min(new_start, loop_end_s)
            ctx.audio.pads.set_pad_loop_start(pad_id, new_start)

        # End line
        end_dragging, end_x, _, _, _ = implot.drag_line_x(
            LOOP_END_DRAG_LINE_ID, loop_end_s, PLOT_REGION_RGBA
        )
        if end_dragging:
            new_end = min(sample_duration_s, end_x)
            new_end = max(new_end, loop_start_s)
            ctx.audio.pads.set_pad_loop_end(pad_id, new_end)

        # Region background
        p1 = implot.plot_to_pixels(loop_start_s, 1.0)
        p2 = implot.plot_to_pixels(loop_end_s, -1.0)
        draw_list.add_rect_filled(p1, p2, imgui.get_color_u32(PLOT_REGION_FILL_RGBA))

    # Playhead
    if ctx.state.pads.is_active(pad_id) and playhead_s is not None:
        implot.tag_x(playhead_s, PLOT_PLAYHEAD_RGBA, " ")
        p1 = implot.plot_to_pixels(playhead_s, 1.0)
        p2 = implot.plot_to_pixels(playhead_s, -1.0)
        draw_list.add_line(p1, p2, imgui.get_color_u32(PLOT_PLAYHEAD_RGBA))
    else:
        # Draw invisible tag to prevent layout shift
        implot.tag_x(start_s, (0, 0, 0, 0), " ")


def _plot_line(
    xs: NDArray[np.float32], ys: NDArray[np.float32], *, show_sample_markers: bool
) -> None:
    with implot_style_color(implot.Col_.line, PLOT_FILL_RGBA):
        if show_sample_markers:
            implot.set_next_marker_style(implot.Marker_.circle)
            with (
                implot_style_color(implot.Col_.marker_fill, PLOT_MARKER_RGBA),
                implot_style_var(implot.StyleVar_.marker_size, 3.0),
            ):
                implot.plot_line("wave", xs, ys)
        else:
            implot.plot_line("wave", xs, ys)


def _plot_shaded(
    xs: NDArray[np.float32], y_min: NDArray[np.float32], y_max: NDArray[np.float32]
) -> None:
    with (
        implot_style_color(implot.Col_.fill, PLOT_FILL_RGBA),
        implot_style_var(implot.StyleVar_.fill_alpha, 0.85),
    ):
        implot.plot_shaded("wave", xs, y_min, y_max)


_GRID_MIN_MINOR_STEP_PX = 12.0


def _grid_anchor_sec(ctx: UiContext, pad_id: int) -> float:
    """Match loop-region snapping's grid anchor (default onset + offset)."""
    return ctx._controller.transport.loop.grid_anchor_sec(pad_id)


def _select_minor_step_64ths(beat_sec: float, *, px_per_sec: float) -> int:
    """Choose the finest readable subdivision in 1/64-note units."""
    grid_64th_sec = beat_sec / 16.0

    # Finest -> coarsest candidates, all aligned to the 1/64-note snapping grid.
    candidates_64ths = [
        1,  # 1/16 beat (1/64-note)
        2,  # 1/8 beat (1/32-note)
        4,  # 1/4 beat (1/16-note)
        8,  # 1/2 beat (1/8-note)
        16,  # 1 beat
        64,  # 1 bar (4 beats)
        256,  # 4 bars (16 beats)
    ]

    for step_64ths in candidates_64ths:
        step_px = step_64ths * grid_64th_sec * px_per_sec
        if step_px >= _GRID_MIN_MINOR_STEP_PX:
            return step_64ths

    return 256


def _plot_px_per_sec(*, start_s: float, end_s: float) -> float | None:
    px_start = implot.plot_to_pixels(start_s, 0.0)
    px_end = implot.plot_to_pixels(end_s, 0.0)
    range_px = abs(px_end.x - px_start.x)
    range_sec = end_s - start_s
    if range_sec <= 0.0 or range_px <= 0.0:
        return None
    return range_px / range_sec


def _grid_render_params(
    beat_sec: float, *, px_per_sec: float
) -> tuple[float, int, int, int] | None:
    minor_step_64ths = _select_minor_step_64ths(beat_sec, px_per_sec=px_per_sec)
    minor_step_sec = (beat_sec / 16.0) * minor_step_64ths
    if minor_step_sec <= 0.0:
        return None

    # Major emphasis rules (see delta spec).
    if minor_step_64ths in {64, 16}:
        major_every = 4  # minor=1 bar or 1 beat; major emphasizes 4x
    elif minor_step_64ths < 16:
        major_every = 16 // minor_step_64ths  # minor<1 beat, major=1 beat
    else:
        major_every = 1  # minor>=4 bars, all visible lines are major

    base = TEXT_MUTED_RGBA
    minor_rgba = ImVec4(base.x, base.y, base.z, 0.08)
    major_rgba = ImVec4(base.x, base.y, base.z, 0.18)
    minor_col = imgui.get_color_u32(minor_rgba)
    major_col = imgui.get_color_u32(major_rgba)

    return (minor_step_sec, major_every, minor_col, major_col)


def _draw_musical_grid_lines(
    draw_list: imgui.ImDrawList,
    *,
    start_s: float,
    end_s: float,
    sample_duration_s: float,
    grid_anchor_sec: float,
    minor_step_sec: float,
    major_every: int,
    minor_col: int,
    major_col: int,
) -> None:
    n_start = math.floor((start_s - grid_anchor_sec) / minor_step_sec)
    n_end = math.ceil((end_s - grid_anchor_sec) / minor_step_sec)

    for n in range(int(n_start), int(n_end) + 1):
        t = grid_anchor_sec + n * minor_step_sec
        if t < 0.0 or t > sample_duration_s:
            continue

        p1 = implot.plot_to_pixels(t, 1.0)
        p2 = implot.plot_to_pixels(t, -1.0)
        col = major_col if (n % major_every == 0) else minor_col
        draw_list.add_line(p1, p2, col)


def _plot_musical_grid(
    ctx: UiContext,
    pad_id: int,
    draw_list: imgui.ImDrawList,
    *,
    start_s: float,
    end_s: float,
    sample_duration_s: float,
) -> None:
    bpm = ctx.state.pads.effective_bpm(pad_id)
    if bpm is None or not math.isfinite(bpm) or bpm <= 0.0:
        return

    px_per_sec = _plot_px_per_sec(start_s=start_s, end_s=end_s)
    if px_per_sec is None:
        return

    params = _grid_render_params(60.0 / float(bpm), px_per_sec=px_per_sec)
    if params is None:
        return

    minor_step_sec, major_every, minor_col, major_col = params

    _draw_musical_grid_lines(
        draw_list,
        start_s=start_s,
        end_s=end_s,
        sample_duration_s=sample_duration_s,
        grid_anchor_sec=_grid_anchor_sec(ctx, pad_id),
        minor_step_sec=minor_step_sec,
        major_every=major_every,
        minor_col=minor_col,
        major_col=major_col,
    )


def _handle_clicks(ctx: UiContext, pad_id: int, sample_duration_s: float) -> None:
    is_plot_hovered = implot.is_plot_hovered()

    if not is_plot_hovered:
        return

    left_released = imgui.is_mouse_released(imgui.MouseButton_.left)
    right_released = imgui.is_mouse_released(imgui.MouseButton_.right)

    if not (left_released or right_released):
        return

    auto_enabled = ctx.state.project.pad_loop_auto[pad_id]
    if auto_enabled and right_released:
        return

    mouse_pos = imgui.get_mouse_pos()
    mouse_plot_pos = implot.pixels_to_plot(mouse_pos.x, mouse_pos.y)
    click_x = mouse_plot_pos.x

    if left_released:
        drag_delta = imgui.get_mouse_drag_delta(imgui.MouseButton_.left)
        if abs(drag_delta.x) > 1 or abs(drag_delta.y) > 1:
            imgui.reset_mouse_drag_delta(imgui.MouseButton_.left)
            return

        loop_start_s, loop_end_s = ctx.state.pads.effective_loop_region(pad_id)
        new_start = max(0.0, click_x)
        if loop_end_s is not None:
            new_start = min(new_start, loop_end_s)
        ctx.audio.pads.set_pad_loop_start(pad_id, new_start)
        imgui.reset_mouse_drag_delta(imgui.MouseButton_.left)

    if right_released:
        drag_delta = imgui.get_mouse_drag_delta(imgui.MouseButton_.right)
        if abs(drag_delta.x) > 1 or abs(drag_delta.y) > 1:
            imgui.reset_mouse_drag_delta(imgui.MouseButton_.right)
            return

        loop_start_s, loop_end_s = ctx.state.pads.effective_loop_region(pad_id)
        new_end = min(sample_duration_s, click_x)
        new_end = max(new_end, loop_start_s)
        ctx.audio.pads.set_pad_loop_end(pad_id, new_end)
        imgui.reset_mouse_drag_delta(imgui.MouseButton_.right)


def _render_plot(ctx: UiContext, pad_id: int) -> None:
    sample_duration_s = ctx.state.project.sample_durations[pad_id]
    if sample_duration_s is None:
        return

    if not implot.begin_plot(
        "##waveform",
        (-1, -1),
        implot.Flags_.no_title
        | implot.Flags_.no_legend
        | implot.Flags_.no_menus
        | implot.Flags_.no_mouse_text,
    ):
        return

    axis_no_grid = getattr(implot.AxisFlags_, "no_grid_lines", 0)
    y_flags = implot.AxisFlags_.no_highlight | axis_no_grid
    implot.setup_axis(implot.ImAxis_.y1, None, y_flags)
    if axis_no_grid:
        implot.setup_axis(implot.ImAxis_.x1, None, axis_no_grid)

    implot.setup_axis_limits_constraints(implot.ImAxis_.x1, 0.0, sample_duration_s)
    implot.setup_axis_limits(implot.ImAxis_.x1, 0.0, sample_duration_s, imgui.Cond_.once)
    implot.setup_axis_limits(implot.ImAxis_.y1, -1.0, 1.0, imgui.Cond_.always)
    implot.setup_finish()

    # Get view limits
    plot_limits = implot.get_plot_limits()
    start_s = max(0.0, plot_limits.x.min)
    end_s = plot_limits.x.max
    ctx.ui.waveform.record_view_range(pad_id, start_s, end_s)

    # Get plot resolution
    plot_width_px = int(imgui.get_content_region_avail().x)
    plot_width_px = max(plot_width_px, 100)  # Safety fallback

    # Call Rust for data (fast aggregation, cached)
    data = ctx.ui.waveform.get_render_data(pad_id, plot_width_px, start_s, end_s)

    if data is not None:
        draw_list = implot.get_plot_draw_list()

        is_raw, xs, y1, y2 = data
        if is_raw:
            _plot_line(
                xs,
                y1,
                show_sample_markers=len(xs) < plot_width_px / 6,  # Show on extreme zoom
            )
        else:
            typed_y2 = cast("NDArray[np.float32]", y2)  # y2 is set in this branch
            _plot_shaded(xs, y1, typed_y2)
        _plot_overlay_loop_region(ctx, pad_id, start_s, draw_list, sample_duration_s)
        _handle_clicks(ctx, pad_id, sample_duration_s)

        _plot_musical_grid(
            ctx,
            pad_id,
            draw_list,
            start_s=start_s,
            end_s=end_s,
            sample_duration_s=sample_duration_s,
        )

    implot.end_plot()


def waveform_editor(ctx: UiContext) -> None:
    session = ctx.state.session
    pad_id = session.waveform_editor_pad_id

    if not session.waveform_editor_open or pad_id is None:
        return

    if not ctx.state.pads.is_loaded(pad_id):
        return

    with imgui_ctx.begin(
        "Waveform Editor", p_open=True, flags=imgui.WindowFlags_.no_collapse
    ) as window:
        if window:
            _render_controls(ctx, pad_id)
            _render_loop_controls(ctx, pad_id)
            _render_plot(ctx, pad_id)

        if not window.opened:
            ctx.ui.waveform.close()
