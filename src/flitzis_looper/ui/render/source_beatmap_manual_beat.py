"""Explicit detector-missed beat controls over the selected captured Native map."""

from dataclasses import dataclass
from typing import TYPE_CHECKING
from weakref import WeakKeyDictionary

from imgui_bundle import imgui

from flitzis_looper.source_beatmap_manual import insert_manual_beat_operation
from flitzis_looper.ui.constants import TEXT_MUTED_RGBA

if TYPE_CHECKING:
    from flitzis_looper.source_beatmap_manual import ManualBeatMode
    from flitzis_looper.ui.context import UiContext, WaveformEditorActions
    from flitzis_looper.ui.render.source_beatmap_editor import SourceMapEditorFrame
    from flitzis_looper_audio import (
        SourceBeatmapReviewInfo,
        SourceBeatmapReviewPage,
    )


@dataclass(slots=True)
class _ManualInputs:
    key: tuple[object, ...]
    frame_key: tuple[object, ...]
    last_frame: int
    mode: ManualBeatMode = "interior"
    window: tuple[object, ...] | None = None
    target: tuple[object, ...] | None = None
    first_anchor: str = "0"
    sample: str = ""
    quarter: str = ""


_INPUTS: WeakKeyDictionary[WaveformEditorActions, _ManualInputs] = WeakKeyDictionary()
_BUSY = frozenset({"pending", "restoring", "settling"})


def _frame_key(frame: SourceMapEditorFrame) -> tuple[object, ...]:
    return (
        frame.pad_id,
        frame.identity,
        frame.selected,
        frame.status,
        frame.revision,
        frame.rate,
    )


def discard_manual_beat(ctx: UiContext) -> None:
    """Discard inactive editor drafts; they grant no source or selection authority."""
    _INPUTS.pop(ctx.ui.waveform, None)


def observe_manual_beat(ctx: UiContext, frame: SourceMapEditorFrame) -> None:
    """Invalidate hidden selection/status roundtrips and gaps in rendered UI frames."""
    state = _INPUTS.get(ctx.ui.waveform)
    if state is None:
        return
    current_frame = imgui.get_frame_count()
    if (
        state.frame_key != _frame_key(frame)
        or current_frame - state.last_frame not in {0, 1}
        or frame.geometry is None
        or not frame.selected
        or frame.status in _BUSY
    ):
        discard_manual_beat(ctx)
    else:
        state.last_frame = current_frame


def _inputs(
    ctx: UiContext, frame: SourceMapEditorFrame, info: SourceBeatmapReviewInfo
) -> _ManualInputs:
    key = (
        frame.pad_id,
        frame.identity,
        frame.revision,
        info["source_identity"],
        info["map_revision"],
        info["capture_revision"],
        info["source_sample_rate_hz"],
        info["source_frame_count"],
    )
    state = _INPUTS.get(ctx.ui.waveform)
    if state is None or state.key != key:
        state = _ManualInputs(key, _frame_key(frame), imgui.get_frame_count())
        _INPUTS[ctx.ui.waveform] = state
    return state


def _mode(state: _ManualInputs) -> None:
    choices: tuple[tuple[ManualBeatMode, str], ...] = (
        ("interior", "Interior"),
        ("extend_start", "Extend start"),
        ("extend_end", "Extend end"),
    )
    imgui.set_next_item_width(180.0)
    if imgui.begin_combo("Placement##source-manual-mode", dict(choices)[state.mode]):
        for value, label in choices:
            if imgui.selectable(label, value == state.mode)[0]:
                state.mode = value
        imgui.end_combo()


def _indices(state: _ManualInputs, count: int) -> tuple[int, int]:
    if state.mode == "extend_start":
        return 0, 0
    if state.mode == "extend_end":
        return count - 1, count - 1
    first = int(state.first_anchor)
    if not 0 <= first < count - 1:
        message = "Choose an existing left anchor for one adjacent interior interval."
        raise ValueError(message)
    return first, first + 1


def _page(
    frame: SourceMapEditorFrame, info: SourceBeatmapReviewInfo, first: int, last: int
) -> SourceBeatmapReviewPage:
    assert frame.geometry is not None
    page = frame.geometry.editor_review_page(first, last, 0, 1)
    actual = (
        page["status"],
        page["map_revision"],
        page["source_identity"],
        page["capture_revision"],
        page["first_anchor_index"],
        page["last_anchor_index"],
        len(page["anchors"]),
        tuple(row["index"] for row in page["anchors"]),
    )
    expected = (
        "ready",
        info["map_revision"],
        info["source_identity"],
        info["capture_revision"],
        first,
        last,
        last - first + 1,
        tuple(range(first, last + 1)),
    )
    if actual != expected:
        message = "The fixed insertion endpoints are unavailable in the current captured map."
        raise ValueError(message)
    return page


def _endpoints(page: SourceBeatmapReviewPage) -> tuple[tuple[int, str, str], ...]:
    return tuple(
        (row["index"], row["source_seconds_bits"], row["beat_bits"]) for row in page["anchors"]
    )


def _page_identity(page: SourceBeatmapReviewPage) -> tuple[object, ...]:
    return (
        page["status"],
        page["map_revision"],
        page["source_identity"],
        page["capture_revision"],
        page["first_anchor_index"],
        page["last_anchor_index"],
        _endpoints(page),
    )


def _window(
    frame: SourceMapEditorFrame, state: _ManualInputs, page: SourceBeatmapReviewPage
) -> tuple[object, ...]:
    return (
        frame.status,
        frame.selected,
        state.mode,
        page["first_anchor_index"],
        page["last_anchor_index"],
        _endpoints(page),
    )


def _operation(
    info: SourceBeatmapReviewInfo, state: _ManualInputs, page: SourceBeatmapReviewPage
) -> dict[str, object]:
    assert info["capture_revision"] is not None
    sample = int(state.sample)
    operation = insert_manual_beat_operation(
        info["map_revision"],
        info["capture_revision"],
        state.mode,
        page["first_anchor_index"],
        page["last_anchor_index"],
        sample,
        info["source_sample_rate_hz"],
        float(state.quarter),
    )
    if sample >= info["source_frame_count"]:
        message = "Choose a physical source sample before the exclusive source end."
        raise ValueError(message)
    return operation


def _gesture(
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    state: _ManualInputs,
    page: SourceBeatmapReviewPage,
) -> tuple[object, ...]:
    try:
        draft = tuple(_operation(info, state, page).items())
    except ValueError:
        draft = (("invalid_sample", state.sample), ("invalid_quarter", state.quarter))
    return (
        state.key,
        _window(frame, state, page),
        info["source_sample_rate_hz"],
        info["source_frame_count"],
        draft,
    )


def _validate_current(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    page: SourceBeatmapReviewPage,
) -> None:
    current = ctx.ui.source_map.selection.effective(frame.pad_id)
    message = "The source, captured map or fixed endpoints changed. Reopen insertion."
    if frame.identity is None or current is None:
        raise ValueError(message)
    actual = (
        ctx.state.session.waveform_editor_pad_id,
        ctx.ui.source_map.identity(frame.pad_id),
        bool(ctx.ui.source_map.selection.is_selected(frame.pad_id)),
        current.revision(),
        current.source_identity(),
        current.editor_review_info(),
        info["map_revision"],
        info["source_sample_rate_hz"],
        info["anchor_count"],
        ctx.ui.source_map.selection.status(frame.pad_id),
        frame.status in _BUSY,
        _page_identity(
            current.editor_review_page(page["first_anchor_index"], page["last_anchor_index"], 0, 1)
        ),
    )
    expected = (
        frame.pad_id,
        frame.identity,
        True,
        info["map_revision"],
        info["source_identity"],
        info,
        frame.revision,
        frame.rate,
        frame.anchor_count,
        frame.status,
        False,
        _page_identity(page),
    )
    if actual != expected:
        raise ValueError(message)


def _apply(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    state: _ManualInputs,
    page: SourceBeatmapReviewPage,
) -> None:
    try:
        _validate_current(ctx, frame, info, page)
        ctx.ui.source_map.selection.edit(
            frame.pad_id, _operation(info, state, page), expected_revision=info["map_revision"]
        )
    except (OSError, RuntimeError, ValueError) as error:
        ctx.ui.source_map.report_error(frame.pad_id, str(error))
    else:
        ctx.ui.source_map.report_error(frame.pad_id, None)


def _controls(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    info: SourceBeatmapReviewInfo,
    state: _ManualInputs,
    page: SourceBeatmapReviewPage,
) -> None:
    for row in page["anchors"]:
        imgui.text_colored(
            TEXT_MUTED_RGBA,
            f"Fixed #{row['index']}: quarter {row['beat']:.17g}, "
            f"source {row['source_seconds']:.17g} s",
        )
    imgui.text_colored(TEXT_MUTED_RGBA, f"Physical source samples at {frame.rate} Hz")
    imgui.set_next_item_width(145.0)
    _, state.sample = imgui.input_text("Source sample##source-manual-sample", state.sample)
    imgui.set_next_item_width(145.0)
    _, state.quarter = imgui.input_text("Quarter coordinate##source-manual-quarter", state.quarter)
    imgui.text_wrapped(
        "Insert one beat missed by the detector at an explicit physical source sample. "
        "The new intervals remain uncertain with unknown meter and phase. "
        "If endpoint labels leave no room, use Set count separately first."
    )
    imgui.push_id(repr(_gesture(frame, info, state, page)))
    try:
        if imgui.button("Insert manual beat##source-manual-insert"):
            _apply(ctx, frame, info, state, page)
    finally:
        imgui.pop_id()


def _ready_info(ctx: UiContext, frame: SourceMapEditorFrame) -> SourceBeatmapReviewInfo | None:
    assert frame.geometry is not None
    try:
        info = frame.geometry.editor_review_info()
    except (OSError, RuntimeError, ValueError) as error:
        discard_manual_beat(ctx)
        imgui.text_wrapped(str(error))
        return None
    if info["status"] != "ready" or info["capture_revision"] is None:
        discard_manual_beat(ctx)
        return None
    return info


def render_manual_beat(ctx: UiContext, frame: SourceMapEditorFrame) -> None:
    """Expose insertion independently of viewport coverage or supported interpolation."""
    observe_manual_beat(ctx, frame)
    info = _ready_info(ctx, frame)
    if info is None:
        return
    if not imgui.collapsing_header("Insert detector-missed beat##source-manual-beat"):
        discard_manual_beat(ctx)
        return
    try:
        state = _inputs(ctx, frame, info)
        _mode(state)
        if state.mode == "interior":
            imgui.set_next_item_width(95.0)
            _, state.first_anchor = imgui.input_text(
                "Left anchor index##source-manual-left", state.first_anchor
            )
        target = frame.status, frame.selected, state.mode, state.first_anchor
        if target != state.target:
            state.target, state.window = target, None
            state.sample, state.quarter = "", ""
        first, last = _indices(state, info["anchor_count"])
        page = _page(frame, info, first, last)
    except (OSError, RuntimeError, ValueError) as error:
        discard_manual_beat(ctx)
        imgui.text_wrapped(str(error))
        return
    window = _window(frame, state, page)
    if window != state.window:
        state.window = window
        state.sample, state.quarter = "", ""
    imgui.begin_disabled(frame.status in _BUSY or frame.identity is None or not frame.selected)
    imgui.push_id(repr(state.key))
    try:
        _controls(ctx, frame, info, state, page)
    finally:
        imgui.pop_id()
        imgui.end_disabled()
