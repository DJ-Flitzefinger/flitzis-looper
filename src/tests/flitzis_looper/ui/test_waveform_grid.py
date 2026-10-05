import math
from contextlib import nullcontext
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui, imgui_ctx, implot

from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import waveform_editor
from flitzis_looper.ui.waveform_grid import (
    loop_beat_label,
    visible_grid_lines,
    waveform_view_start,
)

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
