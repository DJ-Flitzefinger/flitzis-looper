"""Actual Loop Editor gestures over the qualified map/controller boundary."""

import math
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.controller.current_timing import CurrentPadTiming
from flitzis_looper.ui.render import source_beatmap_editor, waveform_editor
from tests.flitzis_looper.ui.test_waveform_render_stability import (
    _click_toolbar,
    _context,
    _full_frame,
    _observe_draw,
    _plot_context,
)

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper_audio import SourceBeatmapEvaluation


def _evaluate(source: float) -> SourceBeatmapEvaluation:
    """Supply the complete Native DTO for a two-segment 120/110 BPM UI fixture."""
    beat = source * 2.0 if source < 0.5 else 1.0 + (source - 0.5) * 11.0 / 6.0
    return {
        "map_revision": "a" * 64,
        "source_seconds": source,
        "beat": beat,
        "local_bpm": 120.0 if source < 0.5 else 110.0,
        "coordinate_status": "anchor" if source in {0.0, 0.5, 600.0} else "supported",
        "interval_status": "trusted",
        "segment_index": 0 if source < 0.5 else 1,
        "physical_domain": "exclusive_source_end" if source == 600.0 else "within_source",
        "meter_status": "quarter_note_4_4_verified",
    }


def _source_at_beat(beat: float) -> float:
    return beat / 2.0 if beat < 1.0 else 0.5 + (beat - 1.0) * 6.0 / 11.0


def _source_after_beats(
    source: float, beats: float, *, require_four_four: bool = True
) -> SourceBeatmapEvaluation:
    current = _evaluate(source)["beat"]
    assert current is not None
    result = _evaluate(_source_at_beat(current + beats))
    if require_four_four:
        assert result["meter_status"] == "quarter_note_4_4_verified"
    return result


def _nearest_grid(source: float, step_beats: float = 0.0625) -> SourceBeatmapEvaluation:
    beat = _evaluate(source)["beat"]
    assert beat is not None
    return _evaluate(_source_at_beat(math.floor(beat / step_beats + 0.5) * step_beats))


def _map() -> Mock:
    geometry = Mock()
    geometry.revision.return_value = "a" * 64
    geometry.editor_info.return_value = {
        "map_revision": "a" * 64,
        "sample_rate_hz": 48_000,
        "source_duration_seconds": 600.0,
        "anchor_count": 3,
        "can_undo": True,
    }
    geometry.evaluate_source.side_effect = _evaluate
    geometry.local_tempo_at_source.side_effect = _evaluate
    geometry.source_after_beats.side_effect = _source_after_beats
    geometry.nearest_grid_source.side_effect = _nearest_grid
    geometry.editor_view.return_value = {
        "map_revision": "a" * 64,
        "status": "complete",
        "step_beats": 1.0,
        "lines": [
            {
                "source_seconds": 0.5,
                "loop_beat": 2.0,
                "major": True,
                "reference": False,
                "meter_status": "quarter_note_4_4_verified",
            }
        ],
        "anchors": [],
    }
    return geometry


def _expose(
    ctx: UiContext,
    monkeypatch: pytest.MonkeyPatch,
    geometry: Mock | None,
) -> tuple[Mock, Mock]:
    maps = ctx.ui.source_map.selection
    monkeypatch.setattr(maps, "effective", Mock(return_value=geometry))
    monkeypatch.setattr(maps, "is_selected", lambda _pad: geometry is not None)
    monkeypatch.setattr(maps, "status", lambda _pad: "effective" if geometry else "none")
    identity = Mock(return_value=("content", 3, (7, "a" * 64, 28_800_000, 48_000)))
    monkeypatch.setattr(ctx.ui.source_map, "identity", identity)
    edit = Mock()
    monkeypatch.setattr(maps, "edit", edit)
    original_header = imgui.collapsing_header

    def expanded(label: str) -> bool:
        imgui.set_next_item_open(is_open=True, cond=imgui.Cond_.always)
        return original_header(label)

    monkeypatch.setattr(imgui, "collapsing_header", expanded)
    return edit, identity


def test_full_editor_manual_creation_captures_source_scalar_intent(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        edit, _identity = _expose(ctx, monkeypatch, None)
        create = Mock()
        monkeypatch.setattr(ctx.ui.source_map.selection, "create_manual", create)
        before = controller.project.pad_loop_start_s[0], controller.project.pad_loop_end_s[0]
        timing = controller.transport.bpm.current_timing(0)
        assert timing is not None
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]

        _click_toolbar("Create manual 4/4 map##source-map-create", ctx, worker, observer, frames)

        create.assert_called_once_with(0, timing.origin_seconds, timing.period_seconds)
        edit.assert_not_called()
        assert before == (
            controller.project.pad_loop_start_s[0],
            controller.project.pad_loop_end_s[0],
        )
        audio_engine_mock.seek_sample.assert_not_called()


def test_full_editor_moves_one_sample_translates_and_undoes_with_same_revision(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        geometry = _map()
        edit, _identity = _expose(ctx, monkeypatch, geometry)
        observer = _observe_draw(monkeypatch)
        before = controller.project.model_dump()
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        state = source_beatmap_editor._INPUTS[ctx.ui.waveform]
        state.beat, state.sample, state.translation = "1", "24001", "-1"

        _click_toolbar("Move beat##source-map-move", ctx, worker, observer, frames)
        _click_toolbar("Translate map##source-map-translate", ctx, worker, observer, frames)
        _click_toolbar("Undo map edit##source-map-undo", ctx, worker, observer, frames)

        assert [call.args[1] for call in edit.call_args_list] == [
            {
                "schema_version": 1,
                "operation": "move_beat",
                "beat": 1.0,
                "sample_index": 24001,
                "sample_rate_hz": 48000,
            },
            {
                "schema_version": 1,
                "operation": "translate",
                "sample_offset": -1,
                "sample_rate_hz": 48000,
            },
            {"schema_version": 1, "operation": "undo"},
        ]
        assert all(call.kwargs == {"expected_revision": "a" * 64} for call in edit.call_args_list)
        assert all(call.args[0] == 0 for call in edit.call_args_list)
        assert controller.project.model_dump() == before
        assert frames[-1]["bands"], "Native variable coordinates reach the actual grid renderer"
        geometry.editor_view.assert_called()
        geometry.record_json.assert_not_called()
        audio_engine_mock.seek_sample.assert_not_called()


def test_source_replacement_between_press_and_release_cannot_edit_new_content(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        edit, identity = _expose(ctx, monkeypatch, _map())
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        old_inputs = source_beatmap_editor._INPUTS[ctx.ui.waveform]
        old_inputs.sample = "24001"
        rect = frames[-1]["buttons"]["Move beat##source-map-move"]
        mouse = ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
        _full_frame(ctx, worker, observer, mouse=mouse)
        _full_frame(ctx, worker, observer, mouse=mouse, down=True)
        identity.return_value = ("replacement", 5, (9, "b" * 64, 28_800_000, 48_000))

        _full_frame(ctx, worker, observer, mouse=mouse, down=False)

        edit.assert_not_called()
        assert source_beatmap_editor._INPUTS[ctx.ui.waveform] is not old_inputs
        assert source_beatmap_editor._INPUTS[ctx.ui.waveform].sample != "24001"


@pytest.mark.parametrize(
    ("label", "method"),
    [
        ("Apply bars##source-map-bars", "apply_source_map_bars"),
        ("Snap markers##source-map-snap", "snap_source_map_markers"),
    ],
)
@pytest.mark.parametrize("changed", ["source", "revision"])
def test_map_toolbar_release_cannot_transfer_to_replacement_target(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    label: str,
    method: str,
    changed: str,
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        geometry = _map()
        _edit, identity = _expose(ctx, monkeypatch, geometry)
        action = Mock()
        monkeypatch.setattr(ctx.audio.pads, method, action)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        rect = frames[-1]["buttons"][label]
        mouse = ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
        _full_frame(ctx, worker, observer, mouse=mouse)
        _full_frame(ctx, worker, observer, mouse=mouse, down=True)
        if changed == "source":
            identity.return_value = ("replacement", 6, (9, "b" * 64, 28_800_000, 48_000))
        else:
            geometry.revision.return_value = "b" * 64
        _full_frame(ctx, worker, observer, mouse=mouse, down=False)
        action.assert_not_called()


def test_selected_unavailable_map_never_draws_a_scalar_substitute(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, Mock())[0]
    _expose(ctx, monkeypatch, None)
    monkeypatch.setattr(ctx.ui.source_map.selection, "is_selected", lambda _pad: True)
    monkeypatch.setattr(ctx.ui.source_map.selection, "status", lambda _pad: "restoring")
    scalar = Mock(side_effect=AssertionError("selected map must not fall back to scalar"))
    monkeypatch.setattr(waveform_editor, "visible_grid_lines", scalar)

    waveform_editor._plot_musical_grid(
        ctx,
        0,
        Mock(),
        0.0,
        4.0,
        timing=CurrentPadTiming(period_seconds=0.5, origin_seconds=0.0, sample_rate_hz=48000),
    )

    scalar.assert_not_called()


@pytest.mark.parametrize(("position", "expected_bpm"), [(0.5, 110), (1.5, 110), (0.25, 120)])
def test_local_source_tempo_uses_retained_playhead_after_seek_or_wrap(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    position: float,
    expected_bpm: int,
) -> None:
    ctx = _context(controller, Mock())[0]
    geometry = _map()
    _expose(ctx, monkeypatch, geometry)
    controller.session.pad_playhead_s[0] = position
    frame = source_beatmap_editor.capture_source_map(ctx, 0)

    assert source_beatmap_editor._local_tempo_label(ctx, frame) == f"Source BPM {expected_bpm:.3f}"
    geometry.local_tempo_at_source.assert_called_once_with(position)
    assert controller.session.active_sample_ids == set()


def test_failed_or_stale_edit_reports_error_without_changing_selection(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, Mock())[0]
    _edit, identity = _expose(ctx, monkeypatch, _map())
    frame = source_beatmap_editor.capture_source_map(ctx, 0)
    state = source_beatmap_editor._inputs(ctx, frame)
    failure = Mock(side_effect=ValueError("monotone anchors required"))

    source_beatmap_editor._run(ctx, frame, state, failure)
    assert state.error == "monotone anchors required"
    identity.return_value = ("replacement", 6)
    action = Mock()
    source_beatmap_editor._run(ctx, frame, state, action)

    action.assert_not_called()
    assert state.error is not None
    assert "source changed" in state.error
