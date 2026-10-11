import math
from contextlib import nullcontext
from decimal import Decimal
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui, imgui_ctx, implot

from flitzis_looper.controller.current_timing import CurrentPadTiming, current_accepted_timing
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import waveform_editor
from flitzis_looper.ui.waveform_grid import (
    loop_beat_label,
    visible_grid_lines,
    waveform_view_start,
)
from tests.flitzis_looper.conftest import current_timing_metadata

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def test_new_loop_has_one_beat_of_virtual_space_and_hidden_zero() -> None:
    start_s = waveform_view_start(0.025, 120.0)
    lines = visible_grid_lines(
        start_s=start_s,
        end_s=2.025,
        anchor_s=0.025,
        loop_start_s=0.025,
        bpm=120.0,
        px_per_s=40.0,
    )

    assert start_s == pytest.approx(-0.475)
    assert [line.source_s for line in lines] == pytest.approx([0.025, 0.525, 1.025, 1.525, 2.025])
    assert [line.loop_beat for line in lines] == pytest.approx([1.0, 2.0, 3.0, 4.0, 5.0])
    assert lines[0].reference


def test_later_loop_renumbers_grid_without_moving_source_positions() -> None:
    original = visible_grid_lines(
        start_s=0.025,
        end_s=2.025,
        anchor_s=0.025,
        loop_start_s=0.025,
        bpm=120.0,
        px_per_s=40.0,
    )
    later = visible_grid_lines(
        start_s=0.025,
        end_s=2.025,
        anchor_s=0.025,
        loop_start_s=1.025,
        bpm=120.0,
        px_per_s=40.0,
    )

    by_source = {line.source_s: line.loop_beat for line in original}
    assert all(by_source[line.source_s] - line.loop_beat == pytest.approx(2.0) for line in later)
    assert next(line.loop_beat for line in later if line.source_s == 1.025) == pytest.approx(1.0)
    assert all(line.loop_beat != 0.0 for line in later)


def test_offgrid_loop_keeps_fractional_labels_and_has_no_fabricated_reference_line() -> None:
    lines = visible_grid_lines(
        start_s=-0.4,
        end_s=1.0,
        anchor_s=0.0,
        loop_start_s=0.1,
        bpm=120.0,
        px_per_s=40.0,
    )

    assert [line.source_s for line in lines] == [0.0, 0.5, 1.0]
    assert [loop_beat_label(line.loop_beat) for line in lines] == ["0.8", "1.8", "2.8"]
    assert not any(line.reference for line in lines)


def test_beat_labels_retain_their_coordinate_across_zoom_subdivisions() -> None:
    common = {"start_s": -0.5, "end_s": 2.0, "anchor_s": 0.0, "loop_start_s": 0.0, "bpm": 120.0}
    coarse = visible_grid_lines(**common, px_per_s=40.0)
    fine = visible_grid_lines(**common, px_per_s=1000.0)
    fine_by_source = {line.source_s: line.loop_beat for line in fine}

    assert len(fine) > len(coarse)
    assert all(fine_by_source[line.source_s] == line.loop_beat for line in coarse)
    assert all(line.loop_beat != 0.0 for line in fine)
    assert next(line.loop_beat for line in fine if line.source_s == 0.03125) == 1.0625


def test_coarse_zoom_retains_first_grid_line_when_loop_is_on_a_later_beat() -> None:
    lines = visible_grid_lines(
        start_s=0.0,
        end_s=100.0,
        anchor_s=0.0,
        loop_start_s=0.5,
        bpm=120.0,
        px_per_s=1.0,
    )

    assert next(line for line in lines if line.source_s == 0.5).loop_beat == 1.0
    assert all(line.source_s != 0.0 for line in lines)


@pytest.mark.parametrize("bpm", [None, 0.0, -120.0, math.nan, math.inf])
def test_missing_bpm_does_not_invent_virtual_beat_spacing(bpm: float | None) -> None:
    assert waveform_view_start(0.025, bpm) == 0.025


def test_extreme_valid_duration_and_bpm_have_bounded_grid_projection() -> None:
    lines = visible_grid_lines(
        start_s=-1.0,
        end_s=1e12,
        anchor_s=-0.05,
        loop_start_s=0.0,
        bpm=1e8,
        px_per_s=1e6,
    )

    assert 0 < len(lines) <= 2048
    assert all(math.isfinite(line.source_s) for line in lines)


@pytest.mark.parametrize("bpm", [0.0, -120.0, math.nan, math.inf, 1e308])
def test_invalid_or_overflowing_grid_coordinates_are_rejected(bpm: float) -> None:
    assert (
        visible_grid_lines(
            start_s=0.0,
            end_s=1e308,
            anchor_s=-1.0,
            loop_start_s=0.0,
            bpm=bpm,
            px_per_s=1.0,
        )
        == ()
    )


def test_tiny_free_offset_is_not_formatted_as_an_exact_integer_beat() -> None:
    assert loop_beat_label(0.99999) == "~1"
    assert loop_beat_label(1.0 + 1e-12) == "1"


def test_manual_120_grid_matches_every_independent_half_second_pulse() -> None:
    lines = visible_grid_lines(
        start_s=0.0,
        end_s=599.5,
        anchor_s=0.0,
        loop_start_s=0.0,
        bpm=120.0,
        px_per_s=40.0,
    )

    assert len(lines) == 1_200
    assert [line.source_s * 48_000 for line in lines] == [pulse * 24_000 for pulse in range(1_200)]


@pytest.mark.parametrize(
    ("sample_rate_hz", "bpm", "origin_frame"),
    [(44_100, 119.999, -217), (48_000, 123.45, 28_776_001), (96_000, 119.999, 96_000)],
)
def test_fractional_visible_subdivisions_keep_continuous_source_positions(
    sample_rate_hz: int, bpm: float, origin_frame: int
) -> None:
    period_frames = Decimal(60 * sample_rate_hz) / Decimal(str(bpm))
    first_frame = Decimal(origin_frame) + 1_198 * period_frames
    last_frame = first_frame + period_frames
    lines = visible_grid_lines(
        start_s=float((first_frame - period_frames / 32) / sample_rate_hz),
        end_s=float((last_frame + period_frames / 32) / sample_rate_hz),
        anchor_s=origin_frame / sample_rate_hz,
        loop_start_s=599.123,
        bpm=bpm,
        px_per_s=1_000.0,
    )

    assert len(lines) == 17
    for subdivision, line in enumerate(lines):
        exact_frame = first_frame + Decimal(subdivision) * period_frames / 16
        assert abs(Decimal(line.source_s) * sample_rate_hz - exact_frame) < Decimal("1e-6")
        assert round(line.source_s * sample_rate_hz) == round(exact_frame)
    assert any(
        line.source_s * sample_rate_hz != round(line.source_s * sample_rate_hz) for line in lines
    )


def test_first_frame_zoom_to_loop_wins_over_initial_source_view(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.sample_paths[0] = "samples/first.wav"
    controller.project.sample_durations[0] = 40.0
    controller.project.manual_bpm[0] = 120.0
    controller.project.pad_loop_end_s[0] = 16.0
    limits = Mock()
    monkeypatch.setattr(implot, "set_next_axis_limits", limits)
    monkeypatch.setattr(imgui_ctx, "begin_group", nullcontext)
    monkeypatch.setattr(imgui, "get_frame_height", lambda: 24.0)
    monkeypatch.setattr(imgui, "get_text_line_height", lambda: 14.0)
    monkeypatch.setattr(imgui, "get_cursor_pos_y", lambda: 0.0)
    monkeypatch.setattr(imgui, "same_line", Mock())
    monkeypatch.setattr(waveform_editor, "_toolbar_next_item", Mock())
    monkeypatch.setattr(waveform_editor, "_text_button_width", lambda *_args: 100.0)
    monkeypatch.setattr(
        waveform_editor, "_render_text_button", lambda label, _height: label == "Zoom to Loop"
    )
    for name in (
        "_render_playback_controls",
        "_separator",
        "_render_view_jump_buttons",
        "_render_loop_controls",
        "_render_close_button",
        "_render_plot",
    ):
        monkeypatch.setattr(waveform_editor, name, Mock())

    waveform_editor._render_editor_body(UiContext(controller), 0)

    assert [invocation.args[1:3] for invocation in limits.call_args_list] == [
        (-0.5, 40.0),
        (-0.5, 16.0),
    ]


def test_accepted_period_drives_visible_grid_and_virtual_margin_without_bpm_roundtrip() -> None:
    period = 0.2245048325556449
    assert 60.0 / (60.0 / period) != period
    origin = -0.025000001
    first_beat = 1_200
    start = origin + first_beat * period
    lines = visible_grid_lines(
        start_s=start - period / 32,
        end_s=start + period * (1 + 1 / 32),
        anchor_s=origin,
        loop_start_s=start,
        bpm=135.0,
        period_seconds=period,
        px_per_s=1000.0,
    )

    assert len(lines) == 17
    for subdivision, line in enumerate(lines):
        expected = origin + (first_beat + subdivision / 16) * period
        assert line.source_s == expected
        assert line.loop_beat == 1 + subdivision / 16
    assert waveform_view_start(start, 135.0, period_seconds=period) == start - period
    assert lines[0].source_s != origin + first_beat * (60.0 / 135.0)


def test_musical_grid_captures_one_current_period_origin_revision_snapshot(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    timing = CurrentPadTiming(
        period_seconds=0.500000013,
        origin_seconds=-0.025000001,
        sample_rate_hz=48_000,
        accepted_revision="a" * 64,
        origin_provenance="verified signed origin",
    )
    ctx = UiContext(controller)
    current = Mock(return_value=timing)
    region = Mock(return_value=(0.075, 10.0))
    monkeypatch.setattr(controller.transport.bpm, "current_timing", current)
    monkeypatch.setattr(controller.transport.bpm, "effective_bpm", Mock(side_effect=AssertionError))
    monkeypatch.setattr(
        controller.transport.loop, "grid_anchor_sec", Mock(side_effect=AssertionError)
    )
    monkeypatch.setattr(controller.transport.loop, "effective_region", region)
    monkeypatch.setattr(waveform_editor, "_plot_px_per_sec", lambda **_kwargs: 1000.0)
    drawn_lines = Mock()
    drawn_labels = Mock()
    monkeypatch.setattr(waveform_editor, "_draw_musical_grid_lines", drawn_lines)
    monkeypatch.setattr(waveform_editor, "_draw_musical_grid_labels", drawn_labels)
    draw_list = Mock()

    waveform_editor._plot_musical_grid(ctx, 0, draw_list, -0.5, 2.0)

    current.assert_called_once_with(0)
    region.assert_called_once_with(0, timing=timing)
    expected = visible_grid_lines(
        start_s=-0.5,
        end_s=2.0,
        anchor_s=timing.origin_seconds,
        loop_start_s=0.075,
        period_seconds=timing.period_seconds,
        px_per_s=1000.0,
    )
    drawn_lines.assert_called_once_with(draw_list, expected)
    drawn_labels.assert_called_once_with(
        draw_list, expected, loop_start_s=0.075, start_s=-0.5, end_s=2.0
    )


def test_unavailable_current_timing_does_not_render_a_fabricated_grid(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = UiContext(controller)
    monkeypatch.setattr(controller.transport.bpm, "current_timing", lambda _pad_id: None)
    drawn = Mock()
    monkeypatch.setattr(waveform_editor, "_draw_musical_grid_lines", drawn)

    waveform_editor._plot_musical_grid(ctx, 0, Mock(), -0.5, 2.0)

    drawn.assert_not_called()


def test_waveform_plot_uses_one_current_snapshot_and_actual_extent_for_clipping(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    timing = current_accepted_timing(current_timing_metadata(), sample_id=0)
    controller.project.sample_durations[0] = 3.0
    ctx = UiContext(controller)
    current = Mock(return_value=timing)
    monkeypatch.setattr(controller.transport.bpm, "current_timing", current)
    # This snapshot/unit test has no native frame; full-frame regressions own
    # readiness children and the ImPlot annotation style's actual geometry.
    readiness = Mock()
    monkeypatch.setattr(waveform_editor, "_render_context_readiness", readiness)
    monkeypatch.setattr(
        implot, "get_style", lambda: Mock(annotation_padding=imgui.ImVec2(4.0, 4.0))
    )
    monkeypatch.setattr(waveform_editor, "implot_style_var", lambda *_args: nullcontext())
    monkeypatch.setattr(implot, "begin_plot", lambda *_args: True)
    monkeypatch.setattr(implot, "end_plot", Mock())
    plot_limits = Mock()
    plot_limits.x.min = 0.0
    plot_limits.x.max = 800.0
    monkeypatch.setattr(implot, "get_plot_limits", lambda: plot_limits)
    monkeypatch.setattr(implot, "get_plot_size", lambda: Mock(x=320.0))
    draw_list = Mock()
    monkeypatch.setattr(implot, "get_plot_draw_list", lambda: draw_list)
    render_data = Mock(return_value=None)
    monkeypatch.setattr(ctx.ui.waveform, "get_render_data", render_data)
    setup = Mock()
    overlay = Mock()
    clicks = Mock()
    grid = Mock()
    monkeypatch.setattr(waveform_editor, "_setup_plot_axes", setup)
    monkeypatch.setattr(waveform_editor, "_plot_overlay_loop_region", overlay)
    monkeypatch.setattr(waveform_editor, "_handle_clicks", clicks)
    monkeypatch.setattr(waveform_editor, "_plot_musical_grid", grid)
    monkeypatch.setattr(waveform_editor, "_draw_zero_line", Mock())

    waveform_editor._render_plot(ctx, 0)

    current.assert_called_once_with(0)
    readiness.assert_called_once_with(ctx, 0)
    setup.assert_called_once_with(ctx, 0, timing=timing)
    render_data.assert_called_once_with(0, 320, 0.0, 800.0, timing=timing)
    overlay.assert_called_once_with(ctx, 0, 0.0, 800.0, draw_list, 600.0, timing=timing)
    clicks.assert_called_once_with(ctx, 0, 600.0)
    grid.assert_called_once_with(ctx, 0, draw_list, 0.0, 800.0, timing=timing)
    assert controller.project.sample_durations[0] == 3.0
