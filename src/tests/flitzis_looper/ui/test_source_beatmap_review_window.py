"""Real ImGui review gestures; Native supplies the separate geometry/custody proof."""

import struct
from typing import TYPE_CHECKING, cast

import pytest
from imgui_bundle import imgui

from flitzis_looper.source_beatmap_review import ReviewWindowRow
from flitzis_looper.ui.render import source_beatmap_editor, source_beatmap_review_window
from tests.flitzis_looper.ui.test_source_beatmap_editor import _expose, _map
from tests.flitzis_looper.ui.test_waveform_render_stability import _context, _plot_context

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper_audio import (
        SourceBeatmapEditorSpan,
        SourceBeatmapReviewAnchor,
        SourceBeatmapReviewInfo,
        SourceBeatmapReviewPage,
    )

type _Buttons = dict[str, tuple[float, float, float, float]]


def _bits(value: float) -> str:
    return struct.pack(">d", value).hex()


def _anchor(index: int) -> SourceBeatmapReviewAnchor:
    source = 0.625 if index == 1 else index / 2.0
    return {
        "index": index,
        "source_seconds": source,
        "source_seconds_bits": _bits(source),
        "beat": float(index),
        "beat_bits": _bits(float(index)),
        "raw_beat_index": index * 2,
        "review": "reviewed" if index == 1 else "estimated",
        "sample_index": round(source * 48_000),
        "has_sample_edit": index == 1,
    }


def _captured_map(count: int = 4, revision: str = "a") -> Mock:
    geometry = _map()
    geometry.revision.return_value = revision * 64
    geometry.source_identity.return_value = "source-a"
    geometry.editor_info.return_value.update(map_revision=revision * 64, anchor_count=count)
    info: SourceBeatmapReviewInfo = {
        "status": "ready",
        "reason": None,
        "map_revision": revision * 64,
        "source_identity": "source-a",
        "capture_revision": "c" * 64,
        "capture_encoding": "source-beatmap-unreviewed-beat-this-raw-v2",
        "backend": "beat_this",
        "event_count": 1024,
        "anchor_count": count,
        "source_sample_rate_hz": 48_000,
        "source_frame_count": 28_800_000,
    }
    geometry.editor_review_info.return_value = info

    def span(first: int, last: int) -> SourceBeatmapEditorSpan:
        left, right = _anchor(first), _anchor(last)
        return {
            "map_revision": info["map_revision"],
            "status": "complete",
            "first_anchor": {
                "index": left["index"],
                "source_seconds": left["source_seconds"],
                "beat": left["beat"],
                "sample_index": left["sample_index"],
            },
            "last_anchor": {
                "index": right["index"],
                "source_seconds": right["source_seconds"],
                "beat": right["beat"],
                "sample_index": right["sample_index"],
            },
            "quarter_notes": right["beat"] - left["beat"],
            "interval_count": last - first,
            "coverage": "uncertain",
            "meter_status": "unknown",
            "downbeat_status": "unknown",
            "downbeat_label": None,
        }

    def page(first: int, last: int, start: int, maximum: int = 128) -> SourceBeatmapReviewPage:
        if not 0 <= first <= last < count or last - first > 63:
            message = "bounded accepted range"
            raise ValueError(message)
        end = min(start + maximum, info["event_count"])
        return {
            "status": "ready",
            "reason": None,
            "map_revision": info["map_revision"],
            "source_identity": info["source_identity"],
            "capture_revision": info["capture_revision"],
            "first_anchor_index": first,
            "last_anchor_index": last,
            "start_index": start,
            "returned_count": end - start,
            "total_event_count": info["event_count"],
            "has_more": end < info["event_count"],
            "anchors": [_anchor(index) for index in range(first, last + 1)],
            "events": [
                {
                    "raw_index": index,
                    "source_seconds": index / 4.0,
                    "source_seconds_bits": _bits(index / 4.0),
                    "detected_downbeat": index % 4 == 0,
                    "physical_domain": "inside",
                }
                for index in range(start, end)
            ],
        }

    geometry.editor_span.side_effect = span
    geometry.editor_review_page.side_effect = page
    return geometry


def _frame(
    ctx: UiContext,
    *,
    first: int = 0,
    last: int = 3,
    mouse: tuple[float, float] | None = None,
    down: bool | None = None,
) -> _Buttons:
    io = imgui.get_io()
    io.display_size = imgui.ImVec2(1600, 1600)
    if mouse is not None:
        io.add_mouse_pos_event(*mouse)
    if down is not None:
        io.add_mouse_button_event(imgui.MouseButton_.left, down)
    buttons: _Buttons = {}
    original = imgui.button

    def observe(label: str, size: imgui.ImVec2Like | None = None) -> bool:
        result = original(label, size)
        low, high = imgui.get_item_rect_min(), imgui.get_item_rect_max()
        buttons[label] = low.x, low.y, high.x, high.y
        return result

    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size((1500, 1500))
    imgui.begin("productive review-window gestures")
    frame = source_beatmap_editor.capture_source_map(ctx, 0)
    assert frame.geometry is not None
    span = frame.geometry.editor_span(first, last)
    imgui.button = observe
    try:
        source_beatmap_review_window.render_review_window(ctx, frame, span)
    finally:
        imgui.button = original
        imgui.end()
        imgui.render()
    return buttons


def _click(ctx: UiContext, label: str, *, first: int = 0, last: int = 3) -> None:
    buttons = _frame(ctx, first=first, last=last)
    low_x, low_y, high_x, high_y = buttons[label]
    mouse = (low_x + high_x) / 2, (low_y + high_y) / 2
    _frame(ctx, first=first, last=last, mouse=mouse)
    _frame(ctx, first=first, last=last, mouse=mouse, down=True)
    _frame(ctx, first=first, last=last, mouse=mouse, down=False)


def _ready(
    controller: AppController, audio: Mock, monkeypatch: pytest.MonkeyPatch, count: int = 4
) -> tuple[UiContext, Mock, Mock, Mock]:
    ctx, _worker = _context(controller, audio)
    ctx.ui.waveform.open(0)
    geometry = _captured_map(count)
    edit, identity = _expose(ctx, monkeypatch, geometry)
    for _ in range(3):
        _frame(ctx)
    return ctx, geometry, edit, identity


def test_actual_raw_selection_requires_explicit_nonordinal_quarters_before_apply(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch, 64)
        state = source_beatmap_review_window._INPUTS[ctx.ui.waveform]
        state.start_index, state.page_start, state.quarter = 128, "128", "999"
        _click(ctx, "Use Raw #128##source-review-raw-128")
        assert state.raw_index == "128"
        assert not state.quarter
        assert not state.rows
        state.quarter = "64.25"
        _click(ctx, "Add/update Raw row##source-review-add-raw")
        state.raw_index, state.quarter = "130", "66.5"
        _click(ctx, "Add/update Raw row##source-review-add-raw")
        assert state.rows == [ReviewWindowRow("raw", 128, 64.25), ReviewWindowRow("raw", 130, 66.5)]
        edit.assert_not_called()
        _click(ctx, "Apply window##source-review-apply")
        operation = edit.call_args.args[1]
        assert operation["first_anchor_index"] == operation["last_anchor_index"] == 63
        assert operation["rows"] == [
            {"kind": "raw", "raw_index": 128, "quarter_bits": _bits(64.25)},
            {"kind": "raw", "raw_index": 130, "quarter_bits": _bits(66.5)},
        ]
        assert edit.call_args.kwargs == {"expected_revision": "a" * 64}
        assert source_beatmap_editor.capture_source_map(ctx, 0).raw.status == "none"
        geometry.record_json.assert_not_called()
        audio_engine_mock.seek_sample.assert_not_called()


def test_successive_64_127_190_controls_use_new_outer_anchor_and_clear_completed_draft(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, _geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch, 64)
        state = source_beatmap_review_window._INPUTS[ctx.ui.waveform]
        state.rows = [ReviewWindowRow("raw", 128 + 2 * index, 64.0 + index) for index in range(63)]
        _click(ctx, "Apply window##source-review-apply")
        assert edit.call_args.args[1]["first_anchor_index"] == 63
        assert len(edit.call_args.args[1]["rows"]) == 63
        next_map = _captured_map(127, "b")
        cast("Mock", ctx.ui.source_map.selection.effective).return_value = next_map
        _frame(ctx)
        fresh = source_beatmap_review_window._INPUTS[ctx.ui.waveform]
        assert fresh is not state
        assert not fresh.rows
        fresh.rows = [ReviewWindowRow("raw", 254 + 2 * index, 127.0 + index) for index in range(63)]
        _click(ctx, "Apply window##source-review-apply")
        assert edit.call_args.args[1]["first_anchor_index"] == 126
        assert edit.call_args.kwargs == {"expected_revision": "b" * 64}
        cast("Mock", ctx.ui.source_map.selection.effective).return_value = _captured_map(190, "d")
        _frame(ctx)
        assert not source_beatmap_review_window._INPUTS[ctx.ui.waveform].rows
        assert edit.call_count == 2


def test_existing_selection_repairs_only_explicit_quarter_and_omission_is_separate_from_apply(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, _geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch)
        state = source_beatmap_review_window._INPUTS[ctx.ui.waveform]
        state.mode = "replace_interior"
        _frame(ctx)
        _click(ctx, "Use current interior rows##source-review-keep-interior")
        assert state.rows == [
            ReviewWindowRow("existing", 1, 1.0),
            ReviewWindowRow("existing", 2, 2.0),
        ]
        _click(ctx, "Use Existing #1##source-review-existing-1")
        state.quarter = "1.5"
        _click(ctx, "Add/update Existing row##source-review-add-existing")
        _click(ctx, "Remove row 2##source-review-remove-1")
        assert state.rows == [ReviewWindowRow("existing", 1, 1.5)]
        edit.assert_not_called()
        _click(ctx, "Apply window##source-review-apply")
        assert edit.call_args.args[1]["rows"] == [
            {"kind": "existing", "anchor_index": 1, "quarter_bits": _bits(1.5)}
        ]
        assert "source_seconds_bits" not in repr(edit.call_args.args[1])


@pytest.mark.parametrize(
    "changed", ["source", "revision", "capture", "range", "mode", "rows", "page", "endpoint"]
)
def test_actual_apply_release_refuses_every_changed_gesture_identity(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    changed: str,
) -> None:
    with _plot_context():
        ctx, geometry, edit, identity = _ready(controller, audio_engine_mock, monkeypatch)
        state = source_beatmap_review_window._INPUTS[ctx.ui.waveform]
        state.mode = "replace_interior"
        _frame(ctx)
        state.rows = [ReviewWindowRow("existing", 1, 1.5)]
        rectangle = _frame(ctx)["Apply window##source-review-apply"]
        mouse = (rectangle[0] + rectangle[2]) / 2, (rectangle[1] + rectangle[3]) / 2
        _frame(ctx, mouse=mouse)
        _frame(ctx, mouse=mouse, down=True)
        last = 3
        if changed == "source":
            identity.return_value = ("new source", 9)
        elif changed == "revision":
            cast("Mock", ctx.ui.source_map.selection.effective).return_value = _captured_map(4, "b")
        elif changed == "capture":
            geometry.editor_review_info.return_value["capture_revision"] = "e" * 64
        elif changed == "range":
            last = 2
        elif changed == "mode":
            state.mode = "extend_end"
        elif changed == "rows":
            state.rows = [ReviewWindowRow("existing", 1, 1.75)]
        elif changed == "page":
            state.start_index = 128
        else:
            original = geometry.editor_review_page.side_effect

            def moved_endpoint(*args: int) -> SourceBeatmapReviewPage:
                page: SourceBeatmapReviewPage = original(*args)
                page["anchors"][0]["beat_bits"] = _bits(-0.0)
                return page

            geometry.editor_review_page.side_effect = moved_endpoint
        _frame(ctx, last=last, mouse=mouse, down=False)
        edit.assert_not_called()
        audio_engine_mock.seek_sample.assert_not_called()


@pytest.mark.parametrize("quarter", ["", "nan", "inf"])
def test_invalid_labels_are_visible_refusals_without_draft_or_selection_mutation(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    quarter: str,
) -> None:
    with _plot_context():
        ctx, _geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch)
        state = source_beatmap_review_window._INPUTS[ctx.ui.waveform]
        state.raw_index, state.quarter = "9", quarter
        _click(ctx, "Add/update Raw row##source-review-add-raw")
        assert not state.rows
        assert cast("Mock", ctx.ui.source_map.report_error).call_args.args[1]
        edit.assert_not_called()


def test_legacy_unavailable_busy_and_oversized_replacement_never_submit(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch, 100)
        state = source_beatmap_review_window._INPUTS[ctx.ui.waveform]
        state.mode = "replace_interior"
        geometry.editor_review_page.reset_mock()
        assert "Apply window##source-review-apply" not in _frame(ctx, last=99)
        geometry.editor_review_page.assert_not_called()
        state.mode = "extend_start"
        _frame(ctx)
        state.rows = [ReviewWindowRow("raw", 0, -1.0)]
        monkeypatch.setattr(ctx.ui.source_map.selection, "status", lambda _pad: "pending")
        _click(ctx, "Apply window##source-review-apply")
        edit.assert_not_called()
        geometry.editor_review_info.return_value.update(status="unavailable", capture_revision=None)
        assert "Apply window##source-review-apply" not in _frame(ctx)
        edit.assert_not_called()


def test_actual_native_window_refusal_retains_draft_project_and_markers(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, _geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch)
        state = source_beatmap_review_window._INPUTS[ctx.ui.waveform]
        state.rows = [ReviewWindowRow("raw", 9, 4.25)]
        before = controller.project.model_dump()
        edit.side_effect = ValueError("new Raw position conflicts with a corrected anchor")
        _click(ctx, "Apply window##source-review-apply")
        assert state.rows == [ReviewWindowRow("raw", 9, 4.25)]
        assert controller.project.model_dump() == before
        assert "corrected anchor" in cast("Mock", ctx.ui.source_map.report_error).call_args.args[1]
        audio_engine_mock.seek_sample.assert_not_called()
