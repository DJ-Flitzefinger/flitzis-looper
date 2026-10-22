"""Explicit bounded editing of the selected map's embedded captured observations."""

import math
from dataclasses import dataclass, field
from typing import TYPE_CHECKING
from weakref import WeakKeyDictionary

from imgui_bundle import imgui

from flitzis_looper.source_beatmap_review import (
    ReviewWindowRow,
    review_window_operation,
)
from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.source_beatmap_review import ReviewWindowMode
    from flitzis_looper.ui.context import UiContext, WaveformEditorActions
    from flitzis_looper.ui.render.source_beatmap_editor import SourceMapEditorFrame
    from flitzis_looper_audio import (
        SourceBeatmapEditorSpan,
        SourceBeatmapReviewAnchor,
        SourceBeatmapReviewInfo,
        SourceBeatmapReviewPage,
    )


@dataclass(slots=True)
class _WindowInputs:
    key: tuple[int, object, str, str]
    mode: ReviewWindowMode = "extend_end"
    window: tuple[object, ...] | None = None
    start_index: int = 0
    page_start: str = "0"
    raw_index: str = ""
    existing_index: str = ""
    quarter: str = ""
    rows: list[ReviewWindowRow] = field(default_factory=list)


_INPUTS: WeakKeyDictionary[WaveformEditorActions, _WindowInputs] = WeakKeyDictionary()
_BUSY = frozenset({"pending", "restoring", "settling"})


def _inputs(
    ctx: UiContext, frame: SourceMapEditorFrame, info: SourceBeatmapReviewInfo
) -> _WindowInputs:
    assert info["capture_revision"] is not None
    key = frame.pad_id, frame.identity, info["map_revision"], info["capture_revision"]
    state = _INPUTS.get(ctx.ui.waveform)
    if state is None or state.key != key:
        state = _WindowInputs(key)
        _INPUTS[ctx.ui.waveform] = state
    return state


def _current(ctx: UiContext, frame: SourceMapEditorFrame, info: SourceBeatmapReviewInfo) -> None:
    current = ctx.ui.source_map.selection.effective(frame.pad_id)
    if (
        frame.identity is None
        or ctx.ui.source_map.identity(frame.pad_id) != frame.identity
        or current is None
        or current.revision() != info["map_revision"]
        or current.source_identity() != info["source_identity"]
    ):
        message = "The selected source map changed. Reopen its review window."
        raise ValueError(message)
    current_info = current.editor_review_info()
    if (
        current_info["status"] != "ready"
        or current_info["capture_revision"] != info["capture_revision"]
        or current_info["map_revision"] != info["map_revision"]
    ):
        message = "The embedded capture changed. Reopen its review window."
        raise ValueError(message)
    if ctx.ui.source_map.selection.status(frame.pad_id) in _BUSY:
        message = "The source map is preparing or publishing. Wait for completion."
        raise ValueError(message)


def _run(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    action: Callable[[], object],
) -> None:
    try:
        _current(ctx, frame, info)
        action()
    except (OSError, RuntimeError, ValueError) as error:
        ctx.ui.source_map.report_error(frame.pad_id, str(error))
    else:
        ctx.ui.source_map.report_error(frame.pad_id, None)


def _mode(state: _WindowInputs) -> None:
    choices: tuple[tuple[ReviewWindowMode, str], ...] = (
        ("extend_end", "Extend end"),
        ("extend_start", "Extend start"),
        ("replace_interior", "Replace interior"),
    )
    imgui.set_next_item_width(180.0)
    if imgui.begin_combo("Window mode##source-review-mode", dict(choices)[state.mode]):
        for value, label in choices:
            if imgui.selectable(label, state.mode == value)[0]:
                state.mode = value
        imgui.end_combo()


def _indices(
    frame: SourceMapEditorFrame, span: SourceBeatmapEditorSpan, mode: ReviewWindowMode
) -> tuple[int, int]:
    if mode == "extend_end":
        return frame.anchor_count - 1, frame.anchor_count - 1
    if mode == "extend_start":
        return 0, 0
    first, last = span["first_anchor"], span["last_anchor"]
    if first is None or last is None or last["index"] - first["index"] > 63:
        message = "Choose at most 62 interior anchors (64 including the two fixed endpoints)."
        raise ValueError(message)
    return first["index"], last["index"]


def _read_page(
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    first: int,
    last: int,
    start: int,
    maximum: int = 128,
) -> SourceBeatmapReviewPage:
    assert frame.geometry is not None
    page = frame.geometry.editor_review_page(first, last, start, maximum)
    actual = (
        page["status"],
        page["map_revision"],
        page["source_identity"],
        page["capture_revision"],
        page["first_anchor_index"],
        page["last_anchor_index"],
        page["start_index"],
    )
    expected = (
        "ready",
        info["map_revision"],
        info["source_identity"],
        info["capture_revision"],
        first,
        last,
        start,
    )
    if actual != expected:
        message = "This bounded review page is unavailable in the current captured map."
        raise ValueError(message)
    return page


def _endpoint_identity(page: SourceBeatmapReviewPage) -> tuple[tuple[int, str, str], ...]:
    anchors = page["anchors"]
    if not anchors:
        message = "The fixed review endpoints are unavailable."
        raise ValueError(message)
    endpoints = anchors[:1] if len(anchors) == 1 else (anchors[0], anchors[-1])
    return tuple((row["index"], row["source_seconds_bits"], row["beat_bits"]) for row in endpoints)


def _window_identity(state: _WindowInputs, page: SourceBeatmapReviewPage) -> tuple[object, ...]:
    return (
        state.mode,
        page["first_anchor_index"],
        page["last_anchor_index"],
        _endpoint_identity(page),
    )


def _gesture_identity(
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    state: _WindowInputs,
    page: SourceBeatmapReviewPage,
) -> tuple[object, ...]:
    return (
        frame.pad_id,
        frame.identity,
        frame.revision,
        info["source_identity"],
        info["map_revision"],
        info["capture_revision"],
        _window_identity(state, page),
        (page["start_index"], page["returned_count"], page["total_event_count"]),
        tuple(row.identity() for row in state.rows),
    )


def _fixed_summary(page: SourceBeatmapReviewPage) -> None:
    anchors = page["anchors"]
    for row in anchors[:1] if len(anchors) == 1 else (anchors[0], anchors[-1]):
        sample = "virtual" if row["sample_index"] is None else str(row["sample_index"])
        imgui.text_colored(
            TEXT_MUTED_RGBA,
            f"Fixed #{row['index']}: quarter {row['beat']:.17g}, "
            f"source {row['source_seconds']:.17g} s, sample {sample}",
        )


def _page_start(state: _WindowInputs, count: int) -> None:
    start = int(state.page_start)
    if not 0 <= start <= count:
        message = f"Raw page start must satisfy 0 <= index <= {count}."
        raise ValueError(message)
    state.start_index = start


def _paging(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    state: _WindowInputs,
    page: SourceBeatmapReviewPage,
) -> None:
    imgui.set_next_item_width(95.0)
    _, state.page_start = imgui.input_text("Raw page start##source-review-page", state.page_start)
    imgui.same_line(spacing=SPACING)
    if imgui.button("Show page##source-review-show-page"):
        _run(ctx, frame, info, lambda: _page_start(state, page["total_event_count"]))
    imgui.begin_disabled(state.start_index == 0)
    if imgui.button("Previous raw page##source-review-previous"):
        state.start_index = max(0, state.start_index - 128)
        state.page_start = str(state.start_index)
    imgui.end_disabled()
    imgui.same_line(spacing=SPACING)
    imgui.begin_disabled(not page["has_more"])
    if imgui.button("Next raw page##source-review-next"):
        state.start_index = page["start_index"] + page["returned_count"]
        state.page_start = str(state.start_index)
    imgui.end_disabled()
    imgui.text_colored(
        TEXT_MUTED_RGBA,
        f"Raw detections {page['start_index']} to "
        f"{page['start_index'] + page['returned_count']} (exclusive) / "
        f"{page['total_event_count']}",
    )


def _raw_observation(
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    page: SourceBeatmapReviewPage,
    index: int,
) -> None:
    if type(index) is not int or not 0 <= index < info["event_count"]:
        message = "Choose one Raw detection inside this capture's event range."
        raise ValueError(message)
    current = _read_page(
        frame, info, page["first_anchor_index"], page["last_anchor_index"], index, 1
    )
    events = current["events"]
    if (
        len(events) != 1
        or events[0]["raw_index"] != index
        or events[0]["physical_domain"] != "inside"
    ):
        message = "Choose one Raw detection inside the physical original source."
        raise ValueError(message)


def _use_raw(state: _WindowInputs, index: int) -> None:
    state.raw_index, state.quarter = str(index), ""


def _raw_rows(state: _WindowInputs, page: SourceBeatmapReviewPage) -> None:
    imgui.begin_child("review-raw-page", (-1, imgui.get_text_line_height_with_spacing() * 5))
    try:
        for row in page["events"]:
            index = row["raw_index"]
            imgui.begin_disabled(row["physical_domain"] != "inside")
            if imgui.button(f"Use Raw #{index}##source-review-raw-{index}"):
                _use_raw(state, index)
            imgui.end_disabled()
            imgui.same_line(spacing=SPACING)
            downbeat = "yes" if row["detected_downbeat"] else "no"
            imgui.text_unformatted(
                f"source {row['source_seconds']:.17g} s | detected downbeat: {downbeat} | "
                f"{row['physical_domain']}"
            )
    finally:
        imgui.end_child()


def _existing_anchor(page: SourceBeatmapReviewPage, index: int) -> SourceBeatmapReviewAnchor:
    row = next((row for row in page["anchors"][1:-1] if row["index"] == index), None)
    if row is None:
        message = "Choose an Existing anchor strictly between the selected fixed endpoints."
        raise ValueError(message)
    return row


def _use_existing(state: _WindowInputs, row: SourceBeatmapReviewAnchor) -> None:
    state.existing_index, state.quarter = str(row["index"]), format(row["beat"], ".17g")


def _existing_rows(state: _WindowInputs, page: SourceBeatmapReviewPage) -> None:
    if state.mode != "replace_interior":
        return
    if imgui.button("Use current interior rows##source-review-keep-interior"):
        state.rows = [
            ReviewWindowRow("existing", row["index"], row["beat"]) for row in page["anchors"][1:-1]
        ]
    imgui.begin_child("review-existing-page", (-1, imgui.get_text_line_height_with_spacing() * 3))
    try:
        for row in page["anchors"][1:-1]:
            if imgui.button(f"Use Existing #{row['index']}##source-review-existing-{row['index']}"):
                _use_existing(state, row)
            imgui.same_line(spacing=SPACING)
            correction = " | sample corrected" if row["has_sample_edit"] else ""
            imgui.text_unformatted(
                f"quarter {row['beat']:.17g} | source {row['source_seconds']:.17g} s{correction}"
            )
    finally:
        imgui.end_child()


def _add_row(
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    state: _WindowInputs,
    page: SourceBeatmapReviewPage,
    *,
    existing: bool,
) -> None:
    quarter = float(state.quarter)
    if not math.isfinite(quarter):
        message = "Enter an explicit finite quarter-note coordinate."
        raise ValueError(message)
    index = int(state.existing_index if existing else state.raw_index)
    if existing:
        _existing_anchor(page, index)
    else:
        _raw_observation(frame, info, page, index)
    row = ReviewWindowRow("existing" if existing else "raw", index, quarter)
    position = next(
        (
            position
            for position, old in enumerate(state.rows)
            if (old.kind, old.index) == (row.kind, index)
        ),
        None,
    )
    if position is None:
        if len(state.rows) >= (62 if state.mode == "replace_interior" else 63):
            message = "This bounded review window has reached its row limit."
            raise ValueError(message)
        state.rows.append(row)
    else:
        state.rows[position] = row


def _draft_fields(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    state: _WindowInputs,
    page: SourceBeatmapReviewPage,
) -> None:
    imgui.set_next_item_width(95.0)
    _, state.raw_index = imgui.input_text("Raw index##source-review-raw-index", state.raw_index)
    imgui.same_line(spacing=SPACING)
    imgui.set_next_item_width(145.0)
    _, state.quarter = imgui.input_text("Quarter coordinate##source-review-quarter", state.quarter)
    if imgui.button("Add/update Raw row##source-review-add-raw"):
        _run(ctx, frame, info, lambda: _add_row(frame, info, state, page, existing=False))
    if state.mode == "replace_interior":
        imgui.set_next_item_width(95.0)
        _, state.existing_index = imgui.input_text(
            "Existing index##source-review-existing-index", state.existing_index
        )
        imgui.same_line(spacing=SPACING)
        if imgui.button("Add/update Existing row##source-review-add-existing"):
            _run(ctx, frame, info, lambda: _add_row(frame, info, state, page, existing=True))
    if state.rows:
        imgui.begin_child("review-draft-rows", (-1, imgui.get_text_line_height_with_spacing() * 4))
        try:
            for position, row in enumerate(tuple(state.rows)):
                if imgui.button(f"Remove row {position + 1}##source-review-remove-{position}"):
                    state.rows.pop(position)
                    break
                imgui.same_line(spacing=SPACING)
                imgui.text_unformatted(
                    f"{row.kind.title()} #{row.index}: explicit quarter {row.quarter:.17g}"
                )
        finally:
            imgui.end_child()


def _apply(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    state: _WindowInputs,
    page: SourceBeatmapReviewPage,
) -> None:
    assert info["capture_revision"] is not None
    operation = review_window_operation(
        info["map_revision"],
        info["capture_revision"],
        state.mode,
        page["first_anchor_index"],
        page["last_anchor_index"],
        tuple(state.rows),
    )
    ctx.ui.source_map.selection.edit(
        frame.pad_id, operation, expected_revision=info["map_revision"]
    )


def _controls(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    state: _WindowInputs,
    page: SourceBeatmapReviewPage,
) -> None:
    _fixed_summary(page)
    _paging(ctx, frame, info, state, page)
    _raw_rows(state, page)
    _existing_rows(state, page)
    _draft_fields(ctx, frame, info, state, page)
    imgui.text_wrapped(
        "Enter rows in source order with explicit quarter labels; skipped detections stay skipped. "
        "Existing references preserve sample corrections. Only Apply changes the map. "
        "New intervals remain uncertain, with unknown meter and phase."
    )
    if state.mode == "replace_interior":
        imgui.text_wrapped(
            "Applying replaces only the selected interior, removing omitted anchors. "
            "Endpoint totals stay fixed; use Set count separately to change the total."
        )
    imgui.begin_disabled(state.mode != "replace_interior" and not state.rows)
    # Earlier draft controls can change rows in this frame; bind Apply to their final exact bits.
    imgui.push_id(repr(_gesture_identity(frame, info, state, page)))
    try:
        if imgui.button("Apply window##source-review-apply"):
            _run(ctx, frame, info, lambda: _apply(ctx, frame, info, state, page))
    finally:
        imgui.pop_id()
        imgui.end_disabled()


def render_review_window(
    ctx: UiContext, frame: SourceMapEditorFrame, span: SourceBeatmapEditorSpan
) -> None:
    """Render bounded captured-map review inside the current Source Map editor."""
    if not imgui.collapsing_header("Review captured map##source-review-window"):
        return
    assert frame.geometry is not None
    try:
        info = frame.geometry.editor_review_info()
    except (OSError, RuntimeError, ValueError) as error:
        imgui.text_wrapped(str(error))
        return
    if info["status"] != "ready" or info["capture_revision"] is None:
        imgui.text_wrapped(info["reason"] or "This map has no complete embedded capture to review.")
        return
    state = _inputs(ctx, frame, info)
    _mode(state)
    try:
        first, last = _indices(frame, span, state.mode)
        page = _read_page(frame, info, first, last, state.start_index)
        window = _window_identity(state, page)
    except (OSError, RuntimeError, ValueError) as error:
        imgui.text_wrapped(str(error))
        return
    if state.window != window:
        state.window = window
        state.rows.clear()
        state.raw_index, state.existing_index, state.quarter = "", "", ""
    imgui.begin_disabled(frame.status in _BUSY or frame.identity is None)
    imgui.push_id(repr(_gesture_identity(frame, info, state, page)))
    try:
        _controls(ctx, frame, info, state, page)
    finally:
        imgui.pop_id()
        imgui.end_disabled()
