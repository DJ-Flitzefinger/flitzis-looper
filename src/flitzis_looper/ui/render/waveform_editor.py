from typing import TYPE_CHECKING, cast

from imgui_bundle import imgui, imgui_ctx, implot

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
    import numpy as np

    from flitzis_looper.ui.context import UiContext

BTN_SIZE = (88, 0)

LOOP_START_DRAG_LINE_ID = 0
LOOP_END_DRAG_LINE_ID = 1


def _render_controls(ctx: UiContext, pad_id: int) -> None:
    is_active = ctx.state.pads.is_active(pad_id)
    play_label = "Pause" if is_active else "Play"

    if imgui.button(play_label, BTN_SIZE):
        if is_active:
            ctx.audio.pads.stop_pad(pad_id)
        else:
            ctx.audio.pads.trigger_pad(pad_id)

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


def _plot_overlay_loop_region(
    ctx: UiContext, pad_id: int, start_s: float, draw_list: imgui.ImDrawList
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
            ctx.audio.pads.set_pad_loop_start(pad_id, start_x)

        # End line
        end_dragging, end_x, _, _, _ = implot.drag_line_x(
            LOOP_END_DRAG_LINE_ID, loop_end_s, PLOT_REGION_RGBA
        )
        if end_dragging:
            ctx.audio.pads.set_pad_loop_end(pad_id, end_x)

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


def _plot_line(xs: np.ndarray, ys: np.ndarray, *, show_sample_markers: bool) -> None:
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


def _plot_shaded(xs: np.ndarray, y_min: np.ndarray, y_max: np.ndarray) -> None:
    with (
        implot_style_color(implot.Col_.fill, PLOT_FILL_RGBA),
        implot_style_var(implot.StyleVar_.fill_alpha, 0.85),
    ):
        implot.plot_shaded("wave", xs, y_min, y_max)


def _beats(beats: list[float], draw_list: imgui.ImDrawList) -> None:
    for beat_x in beats:
        p1 = implot.plot_to_pixels(beat_x, 1.0)
        p2 = implot.plot_to_pixels(beat_x, -1.0)
        draw_list.add_line(p1, p2, imgui.get_color_u32(PLOT_PLAYHEAD_RGBA))


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

    implot.setup_axis(implot.ImAxis_.y1, None, implot.AxisFlags_.no_highlight)
    implot.setup_axis_limits_constraints(implot.ImAxis_.x1, 0.0, sample_duration_s)
    implot.setup_axis_limits(implot.ImAxis_.x1, 0.0, sample_duration_s, imgui.Cond_.once)
    implot.setup_axis_limits(implot.ImAxis_.y1, -1.0, 1.0, imgui.Cond_.always)
    implot.setup_finish()

    # Get view limits
    plot_limits = implot.get_plot_limits()
    start_s = max(0.0, plot_limits.x.min)
    end_s = plot_limits.x.max

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
            _plot_shaded(xs, y1, cast("np.ndarray", y2))
        _plot_overlay_loop_region(ctx, pad_id, start_s, draw_list)

        analysis = ctx.state.project.sample_analysis[pad_id]
        if analysis and len(analysis.beat_grid.beats) > 0:
            _beats(analysis.beat_grid.bars, draw_list)

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
