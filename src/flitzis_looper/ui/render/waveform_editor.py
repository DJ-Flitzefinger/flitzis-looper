import math
from dataclasses import dataclass
from typing import TYPE_CHECKING, cast

from imgui_bundle import ImVec4, icons_fontawesome_6, imgui, imgui_ctx, implot

from flitzis_looper.ui.constants import (
    PLOT_FILL_RGBA,
    PLOT_MARKER_RGBA,
    PLOT_PLAYHEAD_RGBA,
    PLOT_REGION_FILL_RGBA,
    PLOT_REGION_RGBA,
    SPACING,
    TEXT_MUTED_RGBA,
    TEXT_RGBA,
)
from flitzis_looper.ui.contextmanager import implot_style_color, implot_style_var

if TYPE_CHECKING:
    import numpy as np
    from numpy.typing import NDArray

    from flitzis_looper.ui.context import UiContext

_LOOP_START_DRAG_LINE_ID = 0
_LOOP_END_DRAG_LINE_ID = 1

_GRID_MIN_MINOR_STEP_PX = 12.0


@dataclass(slots=True)
class _GridOffsetDragState:
    pad_id: int | None = None
    button: int | None = None
    accum_x: float = 0.0


_GRID_OFFSET_PX_PER_STEP = 2.0
_GRID_OFFSET_DRAG = _GridOffsetDragState()


def _separator(ctx: UiContext, label: str, height: float, text_pos_y: float) -> None:
    draw_list = imgui.get_window_draw_list()

    imgui.same_line(spacing=SPACING)
    pos = imgui.get_cursor_screen_pos()
    draw_list.add_line(pos, (pos[0], pos[1] + height), imgui.get_color_u32(imgui.Col_.separator))
    imgui.dummy((0, 0))

    with imgui_ctx.push_font(ctx.bold_font):
        imgui.same_line(spacing=SPACING)
        imgui.set_cursor_pos_y(text_pos_y)
        imgui.text_colored(TEXT_RGBA, label)


def _render_icon_button(label: str, size: float) -> bool:
    with imgui_ctx.push_style_var(imgui.StyleVar_.button_text_align, (0.5, 0.65)):
        if imgui.button(label, (size, size)):
            return True
    return False


def _render_text_button(label: str, height: float) -> bool:
    padding = imgui.get_style().frame_padding
    with imgui_ctx.push_style_var(imgui.StyleVar_.frame_padding, (padding.x + SPACING, padding.y)):
        if imgui.button(label, (0, height)):
            return True
    return False


def _render_playback_controls(ctx: UiContext, pad_id: int, height: float) -> None:
    is_active = ctx.state.pads.is_active(pad_id)

    if is_active:
        if _render_icon_button(f"{icons_fontawesome_6.ICON_FA_PAUSE}##wf_pause", height):
            ctx.ui.waveform.pause_selected_pad_on_press()
    elif _render_icon_button(f"{icons_fontawesome_6.ICON_FA_PLAY}##wf_play", height):
        ctx.ui.waveform.pause_selected_pad_on_press()
        ctx.ui.waveform.play_restart_selected_pad_on_press()

    imgui.begin_disabled(disabled=not is_active)
    imgui.same_line(spacing=SPACING)
    if _render_icon_button(f"{icons_fontawesome_6.ICON_FA_STOP}##wf_stop", height):
        ctx.ui.waveform.stop_and_reset_selected_pad_on_press()
    imgui.end_disabled()


def _render_zoom_buttons(ctx: UiContext, pad_id: int, height: float) -> None:
    imgui.same_line(spacing=SPACING)
    if _render_text_button("Reset Zoom", height):
        dur_s = ctx.state.project.sample_durations[pad_id]
        if dur_s is not None:
            implot.set_next_axis_limits(implot.ImAxis_.x1, 0.0, dur_s, imgui.Cond_.always)

    imgui.same_line(spacing=SPACING)
    if _render_text_button("Zoom to Loop", height):
        loop_start_s, loop_end_s = ctx.state.pads.effective_loop_region(pad_id)
        if loop_end_s:
            implot.set_next_axis_limits(
                implot.ImAxis_.x1, loop_start_s, loop_end_s, imgui.Cond_.always
            )


def _render_view_jump_buttons(ctx: UiContext, height: float) -> None:
    imgui.same_line(spacing=SPACING)
    btn_jump_start_label = f"{icons_fontawesome_6.ICON_FA_BACKWARD_STEP}##wf_view_jump_start"
    if _render_icon_button(btn_jump_start_label, height):
        limits = ctx.ui.waveform.view_jump_start_selected_pad_on_press()
        if limits is not None:
            implot.set_next_axis_limits(implot.ImAxis_.x1, limits[0], limits[1], imgui.Cond_.always)
    if imgui.is_item_hovered():
        with imgui_ctx.begin_tooltip():
            imgui.text("Jump to start")

    imgui.same_line(spacing=SPACING)
    btn_jump_end_label = f"{icons_fontawesome_6.ICON_FA_FORWARD_STEP}##wf_view_jump_end"
    if _render_icon_button(btn_jump_end_label, height):
        limits = ctx.ui.waveform.view_jump_end_selected_pad_on_press()
        if limits is not None:
            implot.set_next_axis_limits(implot.ImAxis_.x1, limits[0], limits[1], imgui.Cond_.always)
    if imgui.is_item_hovered():
        with imgui_ctx.begin_tooltip():
            imgui.text("Jump to end")


def _render_loop_controls(ctx: UiContext, pad_id: int, height: float, text_pos_y: float) -> None:
    auto_enabled = ctx.state.project.pad_loop_auto[pad_id]

    imgui.same_line(spacing=SPACING)
    if _render_text_button("Reset##wf_loop_reset", height):
        ctx.audio.pads.reset_pad_loop_region(pad_id)

    imgui.same_line(spacing=SPACING)
    check_h = imgui.get_frame_height()
    check_pos_y = imgui.get_cursor_pos_y() + (height - check_h) / 2
    imgui.set_cursor_pos_y(check_pos_y)
    changed, new_auto = imgui.checkbox("Auto-loop", auto_enabled)
    if changed:
        ctx.audio.pads.set_pad_loop_auto(pad_id, enabled=new_auto)

    bars = ctx.state.project.pad_loop_bars[pad_id]
    bpm = ctx.state.pads.effective_bpm(pad_id)

    imgui.same_line(spacing=SPACING)
    if bpm is None and new_auto:
        imgui.set_cursor_pos_y(text_pos_y)
        imgui.text_colored(TEXT_MUTED_RGBA, f"Bars: {bars} (BPM unavailable)")
    else:
        imgui.set_cursor_pos_y(text_pos_y)
        imgui.text_colored(TEXT_MUTED_RGBA, "Bars")
        imgui.same_line(spacing=SPACING / 2)
        imgui.set_cursor_pos_y(text_pos_y)
        imgui.text(str(bars))

        imgui.same_line(spacing=SPACING / 2)
        btn_minus_label = f"{icons_fontawesome_6.ICON_FA_CHEVRON_LEFT}##wf_loop_bars_minus"
        if _render_icon_button(btn_minus_label, height):
            ctx.audio.pads.set_pad_loop_bars(pad_id, bars=max(1, bars - 1))

        imgui.same_line(spacing=SPACING / 2)
        btn_plus_label = f"{icons_fontawesome_6.ICON_FA_CHEVRON_RIGHT}##wf_loop_bars_plus"
        if _render_icon_button(btn_plus_label, height):
            ctx.audio.pads.set_pad_loop_bars(pad_id, bars=bars + 1)

    _render_grid_offset_control(ctx, pad_id, height, text_pos_y)


def _render_grid_offset_control(
    ctx: UiContext, pad_id: int, height: float, text_pos_y: float
) -> None:
    offset = int(ctx.state.project.pad_grid_offset_samples[pad_id])
    offset_text = "0" if offset == 0 else f"{offset:+d}"

    imgui.same_line(spacing=SPACING)
    imgui.set_cursor_pos_y(text_pos_y)
    imgui.text_colored(TEXT_MUTED_RGBA, "Grid Offset")
    imgui.same_line(spacing=SPACING / 2)

    imgui.button(f"{offset_text} smp##grid_offset_samples", (90, height))
    hovered = imgui.is_item_hovered()

    if hovered and imgui.is_mouse_clicked(imgui.MouseButton_.left):
        _GRID_OFFSET_DRAG.pad_id = pad_id
        _GRID_OFFSET_DRAG.button = imgui.MouseButton_.left
        _GRID_OFFSET_DRAG.accum_x = 0.0

    if hovered and imgui.is_mouse_clicked(imgui.MouseButton_.right):
        _GRID_OFFSET_DRAG.pad_id = pad_id
        _GRID_OFFSET_DRAG.button = imgui.MouseButton_.right
        _GRID_OFFSET_DRAG.accum_x = 0.0

    if hovered and imgui.is_mouse_clicked(imgui.MouseButton_.middle):
        ctx.audio.pads.set_pad_grid_offset_samples(pad_id, 0)
        return

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
    end_s: float,
    draw_list: imgui.ImDrawList,
    sample_duration_s: float,
) -> None:
    loop_start_s, loop_end_s = ctx.state.pads.effective_loop_region(pad_id)
    playhead_s = ctx.state.session.pad_playhead_s[pad_id]

    # Loop region
    if loop_end_s is not None:
        # Start line
        start_dragging, start_x, _, _, _ = implot.drag_line_x(
            _LOOP_START_DRAG_LINE_ID, loop_start_s, PLOT_REGION_RGBA
        )
        if start_dragging:
            new_start = max(0.0, start_x)
            new_start = min(new_start, loop_end_s)
            ctx.audio.pads.set_pad_loop_start(pad_id, new_start)

        # End line
        end_dragging, end_x, _, _, _ = implot.drag_line_x(
            _LOOP_END_DRAG_LINE_ID, loop_end_s, PLOT_REGION_RGBA
        )
        if end_dragging:
            new_end = min(sample_duration_s, end_x)
            new_end = max(new_end, loop_start_s)
            ctx.audio.pads.set_pad_loop_end(pad_id, new_end)

        # Region background
        draw_loop_start_s = max(loop_start_s, start_s)
        draw_loop_end_s = min(loop_end_s, end_s)
        if draw_loop_end_s > draw_loop_start_s:
            p1 = implot.plot_to_pixels(draw_loop_start_s, 1.0)
            p2 = implot.plot_to_pixels(draw_loop_end_s, -1.0)
            draw_list.add_rect_filled(p1, p2, imgui.get_color_u32(PLOT_REGION_FILL_RGBA))

    # Playhead
    if (
        ctx.state.pads.is_active(pad_id)
        and playhead_s is not None
        and playhead_s >= start_s
        and playhead_s <= end_s
    ):
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
    start_s: float,
    end_s: float,
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
        if t < start_s or t > end_s:
            continue

        p1 = implot.plot_to_pixels(t, 1.0)
        p2 = implot.plot_to_pixels(t, -1.0)
        col = major_col if (n % major_every == 0) else minor_col
        draw_list.add_line(p1, p2, col)


def _plot_musical_grid(
    ctx: UiContext, pad_id: int, draw_list: imgui.ImDrawList, start_s: float, end_s: float
) -> None:
    bpm = ctx.state.pads.effective_bpm(pad_id)
    if bpm is None or not math.isfinite(bpm) or bpm <= 0.0:
        return

    px_per_sec = _plot_px_per_sec(start_s=start_s, end_s=end_s)
    if px_per_sec is None:
        return

    params = _grid_render_params(60.0 / bpm, px_per_sec=px_per_sec)
    if params is None:
        return

    minor_step_sec, major_every, minor_col, major_col = params

    _draw_musical_grid_lines(
        draw_list,
        start_s,
        end_s,
        _grid_anchor_sec(ctx, pad_id),
        minor_step_sec,
        major_every,
        minor_col,
        major_col,
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
            show_sample_markers = len(xs) < plot_width_px / 6  # On extreme zoom
            _plot_line(xs, y1, show_sample_markers=show_sample_markers)
        else:
            typed_y2 = cast("NDArray[np.float32]", y2)  # y2 is set in this branch
            _plot_shaded(xs, y1, typed_y2)
        _plot_overlay_loop_region(ctx, pad_id, start_s, end_s, draw_list, sample_duration_s)
        _handle_clicks(ctx, pad_id, sample_duration_s)

        _plot_musical_grid(ctx, pad_id, draw_list, start_s, end_s)

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
            # Toolbar
            with imgui_ctx.begin_group():
                toolbar_height = imgui.get_frame_height() * 1.3

                text_h = imgui.get_text_line_height()
                text_pos_y = imgui.get_cursor_pos_y() + (toolbar_height - text_h) / 2 - 3

                _render_playback_controls(ctx, pad_id, toolbar_height)
                _separator(ctx, "View", toolbar_height, text_pos_y)
                _render_zoom_buttons(ctx, pad_id, toolbar_height)
                _render_view_jump_buttons(ctx, toolbar_height)
                _separator(ctx, "Loop", toolbar_height, text_pos_y)
                _render_loop_controls(ctx, pad_id, toolbar_height, text_pos_y)

            _render_plot(ctx, pad_id)

        if not window.opened:
            ctx.ui.waveform.close()
