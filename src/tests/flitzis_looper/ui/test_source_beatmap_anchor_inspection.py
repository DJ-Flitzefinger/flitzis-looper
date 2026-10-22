"""Inspect and position existing uncertain anchors without certifying musical coverage."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui, implot

from flitzis_looper.ui.render import source_beatmap_anchors, source_beatmap_editor, waveform_editor
from tests.flitzis_looper.ui.test_source_beatmap_editor import _evaluate, _expose, _map, _span
from tests.flitzis_looper.ui.test_waveform_render_stability import (
    _click_toolbar,
    _context,
    _full_frame,
    _observe_draw,
    _plot_context,
)

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import SourceBeatmapEditorAnchor, SourceBeatmapEditorSpan


def _uncertain_span(first: int, last: int) -> SourceBeatmapEditorSpan:
    span = _span(first, last)
    span.update({"coverage": "uncertain", "meter_status": "unknown", "downbeat_status": "unknown"})
    span["downbeat_label"] = None
    return span


def _uncertain_map() -> Mock:
    geometry = _map()
    position = _evaluate(0.25)
    position.update({
        "beat": None,
        "local_bpm": None,
        "coordinate_status": "uncertain",
        "interval_status": "uncertain",
        "meter_status": "unknown",
        "downbeat_label": None,
    })
    geometry.evaluate_source.side_effect = None
    geometry.evaluate_source.return_value = position
    geometry.editor_span.side_effect = _uncertain_span
    geometry.editor_view.return_value.update(
        status="unsupported_reference",
        step_beats=0.0,
        lines=[],
        anchors=[
            {"source_seconds": 0.5, "beat": 1.0, "sample_index": 24000},
            {"source_seconds": 600.0, "beat": 1100.0, "sample_index": None},
        ],
    )
    return geometry


@pytest.mark.parametrize(("endpoint", "beat", "sample"), [("first", 0.0, 0), ("last", 1.0, 24000)])
def test_actual_uncertain_endpoint_gesture_fills_position_then_moves_only_on_separate_action(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    endpoint: str,
    beat: float,
    sample: int,
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        geometry = _uncertain_map()
        edit, _identity = _expose(ctx, monkeypatch, geometry)
        before = controller.project.model_dump()
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        state = source_beatmap_editor._INPUTS[ctx.ui.waveform]

        _click_toolbar(
            f"Use {endpoint} anchor##source-map-use-{endpoint}",
            ctx,
            worker,
            observer,
            frames,
            width=1500,
            scale=1.0,
        )

        assert (float(state.beat), int(state.sample)) == (beat, sample)
        edit.assert_not_called()
        state.sample = str(sample + 1)
        _click_toolbar(
            "Move beat##source-map-move", ctx, worker, observer, frames, width=1500, scale=1.0
        )

        edit.assert_called_once_with(
            0,
            {
                "schema_version": 1,
                "operation": "move_beat",
                "beat": beat,
                "sample_index": sample + 1,
                "sample_rate_hz": 48000,
            },
            expected_revision="a" * 64,
        )
        assert state.coverage == "uncertain"
        assert controller.project.model_dump() == before
        geometry.record_json.assert_not_called()
        audio_engine_mock.seek_sample.assert_not_called()


def test_virtual_and_replaced_endpoint_selection_preserve_position_fields(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, audio_engine_mock)[0]
    geometry = _uncertain_map()
    edit, identity = _expose(ctx, monkeypatch, geometry)
    frame = source_beatmap_editor.capture_source_map(ctx, 0)
    state = source_beatmap_editor._inputs(ctx, frame)
    state.first_anchor, state.last_anchor = "1", "2"
    state.beat, state.sample = "-4", "7"

    with pytest.raises(ValueError, match="virtual anchor"):
        source_beatmap_editor._use_span_anchor(ctx, frame, state, first=False)
    assert (state.beat, state.sample) == ("-4", "7")
    replacement = _map()
    replacement.revision.return_value = "b" * 64
    monkeypatch.setattr(ctx.ui.source_map.selection, "effective", lambda _pad: replacement)
    with pytest.raises(ValueError, match="selected map changed"):
        source_beatmap_editor._use_span_anchor(ctx, frame, state, first=True)
    identity.return_value = ("replaced content", 7)
    source_beatmap_editor._run(
        ctx,
        frame,
        state,
        lambda: source_beatmap_editor._use_span_anchor(ctx, frame, state, first=True),
    )
    assert state.error is not None
    assert "source changed" in state.error
    assert (state.beat, state.sample) == ("-4", "7")
    edit.assert_not_called()


@pytest.mark.parametrize("changed", ["range", "source", "revision"])
@pytest.mark.parametrize("endpoint", ["first", "last"])
def test_endpoint_release_never_selects_a_replaced_range_source_or_revision(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    changed: str,
    endpoint: str,
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        geometry = _uncertain_map()
        edit, identity = _expose(ctx, monkeypatch, geometry)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        state = source_beatmap_editor._INPUTS[ctx.ui.waveform]
        state.beat, state.sample = "-4", "7"
        rect = frames[-1]["buttons"][f"Use {endpoint} anchor##source-map-use-{endpoint}"]
        mouse = ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
        _full_frame(ctx, worker, observer, mouse=mouse)
        _full_frame(ctx, worker, observer, mouse=mouse, down=True)
        if changed == "range":
            state.first_anchor, state.last_anchor = "1", "2"
        elif changed == "source":
            identity.return_value = ("replacement content", 9)
        else:
            geometry.revision.return_value = "b" * 64
        _full_frame(ctx, worker, observer, mouse=mouse, down=False)

        assert (state.beat, state.sample) == ("-4", "7")
        if changed == "range":
            assert source_beatmap_editor._INPUTS[ctx.ui.waveform] is state
        else:
            replacement = source_beatmap_editor._INPUTS[ctx.ui.waveform]
            assert replacement is not state
            assert (replacement.beat, replacement.sample) == ("0", "0")
        edit.assert_not_called()


def test_anchor_ticks_are_short_and_unlabelled_and_exclude_virtual_samples(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with _plot_context():
        draw = Mock()
        monkeypatch.setattr(implot, "plot_to_pixels", imgui.ImVec2)
        source_beatmap_anchors.draw_source_map_anchors(
            draw, _uncertain_map().editor_view.return_value["anchors"]
        )
        draw.add_line.assert_called_once()
        assert draw.add_line.call_args.args[:2] == ((0.5, 3.0), (0.5, 11.0))
        draw.add_text.assert_not_called()


def test_real_uncertain_waveform_draws_anchor_vertices_without_musical_grid(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        geometry = _uncertain_map()
        edit, _identity = _expose(ctx, monkeypatch, geometry)
        vertex_deltas: list[int] = []
        original = source_beatmap_anchors.draw_source_map_anchors

        def draw_anchors(
            draw_list: imgui.ImDrawList, anchors: list[SourceBeatmapEditorAnchor]
        ) -> None:
            before = len(draw_list.vtx_buffer)
            original(draw_list, anchors)
            vertex_deltas.append(len(draw_list.vtx_buffer) - before)

        monkeypatch.setattr(waveform_editor, "draw_source_map_anchors", draw_anchors)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        assert vertex_deltas
        assert all(delta > 0 for delta in vertex_deltas)
        assert all(not frame["bands"] for frame in frames)
        geometry.record_json.assert_not_called()
        edit.assert_not_called()
