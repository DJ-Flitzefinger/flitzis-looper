"""Transient editor intent over the loader's source-qualified Native map."""

import math
from contextlib import suppress
from dataclasses import dataclass, field
from typing import TYPE_CHECKING
from weakref import WeakKeyDictionary

from imgui_bundle import imgui

from flitzis_looper.timing_policy import variable_source_timing
from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA
from flitzis_looper.ui.render.source_beatmap_manual_beat import (
    discard_manual_beat,
    observe_manual_beat,
    render_manual_beat,
)
from flitzis_looper.ui.render.source_beatmap_review_window import render_review_window

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.current_timing import CurrentPadTiming
    from flitzis_looper.controller.source_beatmap_selection import RawSourceBeatmapState
    from flitzis_looper.ui.context import UiContext, WaveformEditorActions
    from flitzis_looper_audio import (
        SourceBeatmap,
        SourceBeatmapEditorSpan,
        SourceBeatmapRawInspection,
    )


@dataclass(frozen=True, slots=True)
class SourceMapEditorFrame:
    """One qualified map for this frame, separate from current scalar audio timing."""

    pad_id: int
    identity: object
    selected: bool
    status: str
    geometry: SourceBeatmap | None
    revision: str | None
    rate: int | None
    anchor_count: int
    can_undo: bool
    raw: RawSourceBeatmapState


@dataclass(slots=True)
class _EditorInputs:
    key: tuple[int, object, str | None]
    beat: str = "0"
    sample: str = "0"
    translation: str = "0"
    first_anchor: str = "0"
    last_anchor: str = "1"
    inspected_span: tuple[int, int] | None = None
    quarter_notes: str = ""
    meter_status: str = "unknown"
    downbeat_label: str = ""
    coverage: str = "gap_unknown_count"
    error: str | None = None


@dataclass(slots=True)
class _RawInputs:
    key: tuple[int, str | None]
    start_index: int = 0
    inspecting: bool = False
    raw_index: str = ""
    quarter_coordinate: str = ""
    labels: list[tuple[int, float]] = field(default_factory=list)


_INPUTS: WeakKeyDictionary[WaveformEditorActions, _EditorInputs] = WeakKeyDictionary()
_RAW_INPUTS: WeakKeyDictionary[WaveformEditorActions, _RawInputs] = WeakKeyDictionary()
_BUSY = frozenset({"pending", "restoring", "settling"})


def capture_source_map(ctx: UiContext, pad_id: int) -> SourceMapEditorFrame:
    """Read the existing selection owner once; receipt data alone is not geometry."""
    maps = ctx.ui.source_map.selection
    geometry = maps.effective(pad_id)
    info = geometry.editor_info() if geometry is not None else None
    return SourceMapEditorFrame(
        pad_id=pad_id,
        identity=ctx.ui.source_map.identity(pad_id),
        selected=maps.is_selected(pad_id),
        status=maps.status(pad_id),
        geometry=geometry,
        revision=None if geometry is None else geometry.revision(),
        rate=None if info is None else info["sample_rate_hz"],
        anchor_count=0 if info is None else info["anchor_count"],
        can_undo=False if info is None else info["can_undo"],
        raw=maps.raw_state(pad_id),
    )


def _inputs(ctx: UiContext, frame: SourceMapEditorFrame) -> _EditorInputs:
    key = (frame.pad_id, frame.identity, frame.revision)
    state = _INPUTS.get(ctx.ui.waveform)
    if state is None or state.key != key:
        state = _EditorInputs(key)
        _INPUTS[ctx.ui.waveform] = state
        # Unsupported coverage is ordinary initial readiness, not an edit failure.
        with suppress(ValueError):
            _use_playhead(ctx, frame, state)
    return state


def _use_playhead(ctx: UiContext, frame: SourceMapEditorFrame, state: _EditorInputs) -> None:
    if frame.geometry is None or frame.rate is None:
        return
    source = ctx.state.session.pad_playhead_s[frame.pad_id]
    if source is None:
        source = ctx.state.project.pad_loop_start_s[frame.pad_id]
    evaluated = frame.geometry.evaluate_source(source)
    beat = evaluated["beat"]
    if beat is None or not math.isfinite(beat):
        message = "The playhead has no supported musical beat coordinate."
        raise ValueError(message)
    state.beat = format(beat, ".17g")
    state.sample = str(round(source * frame.rate))


def _report(
    ctx: UiContext, frame: SourceMapEditorFrame, state: _EditorInputs, reason: str | None
) -> None:
    state.error = reason
    ctx.ui.source_map.report_error(frame.pad_id, reason)


def _run(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    state: _EditorInputs,
    action: Callable[[], object],
) -> None:
    """Refuse an old text target before the loader captures its own Native job."""
    if frame.identity is None or ctx.ui.source_map.identity(frame.pad_id) != frame.identity:
        _report(ctx, frame, state, "The source changed. Reopen Source Map before editing.")
        return
    try:
        action()
    except (OSError, RuntimeError, ValueError) as error:
        _report(ctx, frame, state, str(error))
    else:
        _report(ctx, frame, state, None)


def _move_beat(ctx: UiContext, frame: SourceMapEditorFrame, state: _EditorInputs) -> None:
    assert frame.revision is not None
    assert frame.rate is not None
    beat = float(state.beat)
    sample = int(state.sample)
    if not math.isfinite(beat) or sample < 0:
        message = "Enter a finite quarter-note beat and a non-negative source sample."
        raise ValueError(message)
    ctx.ui.source_map.selection.edit(
        frame.pad_id,
        {
            "schema_version": 1,
            "operation": "move_beat",
            "beat": beat,
            "sample_index": sample,
            "sample_rate_hz": frame.rate,
        },
        expected_revision=frame.revision,
    )


def _translate(ctx: UiContext, frame: SourceMapEditorFrame, state: _EditorInputs) -> None:
    assert frame.revision is not None
    assert frame.rate is not None
    ctx.ui.source_map.selection.edit(
        frame.pad_id,
        {
            "schema_version": 1,
            "operation": "translate",
            "sample_offset": int(state.translation),
            "sample_rate_hz": frame.rate,
        },
        expected_revision=frame.revision,
    )


def _status_line(label: str) -> None:
    """Keep ordinary readiness text from changing the waveform's vertical layout."""
    height = imgui.get_frame_height()
    imgui.begin_child(
        "source-map-status",
        (-1, height),
        window_flags=imgui.WindowFlags_.no_scrollbar | imgui.WindowFlags_.no_scroll_with_mouse,
    )
    position = imgui.get_cursor_screen_pos()
    available = imgui.get_content_region_avail().x
    draw_list = imgui.get_window_draw_list()
    draw_list.push_clip_rect(
        position,
        (position.x + available, position.y + height),
        intersect_with_current_clip_rect=True,
    )
    draw_list.add_text(
        (position.x, position.y + imgui.get_style().frame_padding.y),
        imgui.get_color_u32(TEXT_MUTED_RGBA),
        label,
    )
    draw_list.pop_clip_rect()
    imgui.dummy((available, height))
    # Only normal readiness/timing text reaches this line; errors use the console.
    if imgui.is_item_hovered():
        imgui.set_tooltip(label)
    imgui.end_child()


def _local_tempo_label(ctx: UiContext, frame: SourceMapEditorFrame) -> str:
    if not variable_source_timing(ctx.state.project.global_timing_policy):
        timing = ctx.state.pads.current_timing(frame.pad_id)
        return (
            "Fixed source BPM unavailable"
            if timing is None
            else f"Fixed source BPM {timing.bpm:.3f}"
        )
    if frame.geometry is None:
        return "Source BPM unavailable"
    position = ctx.state.session.pad_playhead_s[frame.pad_id]
    if position is None:
        position = ctx.state.project.pad_loop_start_s[frame.pad_id]
    value = frame.geometry.local_tempo_at_source(position)
    bpm = value["local_bpm"]
    if bpm is None:
        return f"Source BPM unavailable ({value['coordinate_status']}, {value['interval_status']})"
    return f"Source BPM {bpm:.3f}"


def _correction_fields(ctx: UiContext, frame: SourceMapEditorFrame, state: _EditorInputs) -> None:
    assert frame.revision is not None
    revision = frame.revision
    imgui.text_colored(
        TEXT_MUTED_RGBA, f"Source samples at {frame.rate} Hz; beats are quarter notes"
    )
    imgui.set_next_item_width(145.0)
    _, state.beat = imgui.input_text("Source beat (quarter notes)##source-map-beat", state.beat)
    imgui.set_next_item_width(145.0)
    _, state.sample = imgui.input_text("Source sample##source-map-sample", state.sample)
    if imgui.button("Use playhead##source-map-position"):
        _run(ctx, frame, state, lambda: _use_playhead(ctx, frame, state))
    imgui.same_line(spacing=SPACING)
    if imgui.button("Move beat##source-map-move"):
        _run(ctx, frame, state, lambda: _move_beat(ctx, frame, state))
    imgui.set_next_item_width(145.0)
    _, state.translation = imgui.input_text(
        "Signed alignment (samples)##source-map-alignment", state.translation
    )
    if imgui.button("Translate map##source-map-translate"):
        _run(ctx, frame, state, lambda: _translate(ctx, frame, state))
    imgui.same_line(spacing=SPACING)
    imgui.begin_disabled(not frame.can_undo)
    if imgui.button("Undo map edit##source-map-undo"):
        _run(
            ctx,
            frame,
            state,
            lambda: ctx.ui.source_map.selection.edit(
                frame.pad_id,
                {"schema_version": 1, "operation": "undo"},
                expected_revision=revision,
            ),
        )
    imgui.end_disabled()


def _span_indices(frame: SourceMapEditorFrame, state: _EditorInputs) -> tuple[int, int]:
    first, last = int(state.first_anchor), int(state.last_anchor)
    if not 0 <= first < last < frame.anchor_count:
        message = "Choose existing anchor indices with 0 <= first < last < anchor count."
        raise ValueError(message)
    return first, last


def _inspect_span(frame: SourceMapEditorFrame, state: _EditorInputs) -> SourceBeatmapEditorSpan:
    assert frame.geometry is not None
    first, last = _span_indices(frame, state)
    span = frame.geometry.editor_span(first, last)
    if span["map_revision"] != frame.revision or span["status"] != "complete":
        message = "The selected anchor span is unavailable in the current map revision."
        raise ValueError(message)
    return span


def _use_span_anchor(
    ctx: UiContext, frame: SourceMapEditorFrame, state: _EditorInputs, *, first: bool
) -> None:
    current = ctx.ui.source_map.selection.effective(frame.pad_id)
    if current is None or current.revision() != frame.revision:
        message = "The selected map changed. Reopen Source Map before selecting an anchor."
        raise ValueError(message)
    span = _inspect_span(frame, state)
    anchor = span["first_anchor" if first else "last_anchor"]
    if anchor is None or anchor["sample_index"] is None:
        message = "This virtual anchor has no physical source sample to position."
        raise ValueError(message)
    state.beat = format(anchor["beat"], ".17g")
    state.sample = str(anchor["sample_index"])


def _select_span(frame: SourceMapEditorFrame, state: _EditorInputs, first: int, last: int) -> None:
    if not 0 <= first < last < frame.anchor_count:
        message = "The requested interval is outside the existing accepted anchors."
        raise ValueError(message)
    state.first_anchor, state.last_anchor = str(first), str(last)


def _step_span(frame: SourceMapEditorFrame, state: _EditorInputs, direction: int) -> None:
    first, last = _span_indices(frame, state)
    _select_span(frame, state, first + direction, last + direction)


def _select_playhead_span(
    ctx: UiContext, frame: SourceMapEditorFrame, state: _EditorInputs
) -> None:
    assert frame.geometry is not None
    source = ctx.state.session.pad_playhead_s[frame.pad_id]
    if source is None:
        source = ctx.state.project.pad_loop_start_s[frame.pad_id]
    evaluated = frame.geometry.evaluate_source(source)
    index = evaluated["segment_index"]
    if evaluated["map_revision"] != frame.revision or index is None:
        message = "The playhead is outside the accepted anchor intervals."
        raise ValueError(message)
    # Gap queries still identify their actual interval without inventing its B coordinate.
    _select_span(frame, state, index, index + 1)


def _range_selection(ctx: UiContext, frame: SourceMapEditorFrame, state: _EditorInputs) -> None:
    imgui.text_colored(TEXT_MUTED_RGBA, f"Accepted anchor indices: 0 to {frame.anchor_count - 1}")
    imgui.set_next_item_width(70.0)
    first_changed, state.first_anchor = imgui.input_text(
        "First anchor##source-map-first", state.first_anchor
    )
    imgui.same_line(spacing=SPACING)
    imgui.set_next_item_width(70.0)
    last_changed, state.last_anchor = imgui.input_text(
        "Last anchor##source-map-last", state.last_anchor
    )
    if first_changed or last_changed:
        _run(ctx, frame, state, lambda: _inspect_span(frame, state))
    if imgui.button("Previous span##source-map-previous"):
        _run(ctx, frame, state, lambda: _step_span(frame, state, -1))
    imgui.same_line(spacing=SPACING)
    if imgui.button("Next span##source-map-next"):
        _run(ctx, frame, state, lambda: _step_span(frame, state, 1))
    imgui.same_line(spacing=SPACING)
    if imgui.button("Select interval at playhead##source-map-select-playhead"):
        _run(ctx, frame, state, lambda: _select_playhead_span(ctx, frame, state))


def _span_defaults(state: _EditorInputs, span: SourceBeatmapEditorSpan) -> None:
    first, last = span["first_anchor"], span["last_anchor"]
    assert first is not None
    assert last is not None
    key = first["index"], last["index"]
    if state.inspected_span == key:
        return
    state.inspected_span = key
    count = span["quarter_notes"]
    assert count is not None
    state.quarter_notes = format(count, ".17g")
    state.meter_status = span["meter_status"]
    state.downbeat_label = "" if span["downbeat_label"] is None else str(span["downbeat_label"])
    state.coverage = span["coverage"]


def _span_summary(span: SourceBeatmapEditorSpan) -> None:
    count = span["quarter_notes"]
    assert count is not None
    for label, anchor in (("First", span["first_anchor"]), ("Last", span["last_anchor"])):
        assert anchor is not None
        sample = "virtual" if anchor["sample_index"] is None else str(anchor["sample_index"])
        imgui.text_colored(
            TEXT_MUTED_RGBA,
            f"{label} #{anchor['index']}: beat {anchor['beat']:.17g}, "
            f"source {anchor['source_seconds']:.17g} s, sample {sample}",
        )
    phase = span["downbeat_status"]
    if span["downbeat_label"] is not None:
        phase = f"{phase} ({span['downbeat_label']} + 4Z)"
    imgui.text_colored(
        TEXT_MUTED_RGBA,
        f"{count:.17g} quarter notes / {span['interval_count']} intervals | "
        f"Coverage: {span['coverage']} | Meter: {span['meter_status']} | Downbeat: {phase}",
    )


def _edit_span(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    state: _EditorInputs,
    operation: str,
) -> None:
    assert frame.revision is not None
    first, last = _span_indices(frame, state)
    span = _inspect_span(frame, state)
    fields: dict[str, object]
    if operation == "set_count":
        count = float(state.quarter_notes)
        if not math.isfinite(count) or count <= 0:
            message = "Enter a finite positive quarter-note count."
            raise ValueError(message)
        fields = {"quarter_notes": count}
    elif operation == "set_meter":
        if state.meter_status not in {"unknown", "quarter_note_4_4_verified"}:
            message = "Select unknown meter or explicitly verified 4/4."
            raise ValueError(message)
        fields = {"meter_status": state.meter_status}
    elif operation == "set_downbeat":
        label = None if not state.downbeat_label.strip() else int(state.downbeat_label)
        if label is not None and not -(1 << 63) <= label < 1 << 63:
            message = "The downbeat label must fit an exact signed 64-bit integer."
            raise ValueError(message)
        if label is not None and span["meter_status"] != "quarter_note_4_4_verified":
            message = "Setting a downbeat requires explicitly verified 4/4 in the selected span."
            raise ValueError(message)
        fields = {"downbeat_label": label}
    elif operation == "set_coverage":
        if state.coverage not in {"trusted", "uncertain", "gap_unknown_count"}:
            message = "Select trusted, uncertain or unknown-count gap coverage."
            raise ValueError(message)
        fields = {
            "coverage": state.coverage,
            "reviewed_quarter_notes": (
                None if state.coverage == "gap_unknown_count" else span["quarter_notes"]
            ),
        }
    else:
        message = "Unknown source-map interval correction."
        raise ValueError(message)
    ctx.ui.source_map.selection.edit(
        frame.pad_id,
        {
            "schema_version": 1,
            "operation": operation,
            "first_anchor_index": first,
            "last_anchor_index": last,
            **fields,
        },
        expected_revision=frame.revision,
    )


def _choice(label: str, current: str, choices: tuple[tuple[str, str], ...]) -> str:
    labels = dict(choices)
    imgui.set_next_item_width(180.0)
    if imgui.begin_combo(label, labels.get(current, "Mixed; choose interpretation")):
        for value, text in choices:
            if imgui.selectable(text, value == current)[0]:
                current = value
        imgui.end_combo()
    return current


def _interpretation_actions(
    ctx: UiContext, frame: SourceMapEditorFrame, state: _EditorInputs
) -> None:
    imgui.set_next_item_width(180.0)
    _, state.quarter_notes = imgui.input_text(
        "Quarter-note count##source-map-count", state.quarter_notes
    )
    imgui.same_line(spacing=SPACING)
    if imgui.button("Set count##source-map-set-count"):
        _run(ctx, frame, state, lambda: _edit_span(ctx, frame, state, "set_count"))
    state.meter_status = _choice(
        "Meter##source-map-meter",
        state.meter_status,
        (("unknown", "Unknown"), ("quarter_note_4_4_verified", "Verified 4/4")),
    )
    imgui.same_line(spacing=SPACING)
    if imgui.button("Set meter##source-map-set-meter"):
        _run(ctx, frame, state, lambda: _edit_span(ctx, frame, state, "set_meter"))
    imgui.set_next_item_width(180.0)
    _, state.downbeat_label = imgui.input_text(
        "Downbeat label (blank: unknown)##source-map-downbeat", state.downbeat_label
    )
    imgui.same_line(spacing=SPACING)
    if imgui.button("Set downbeat##source-map-set-downbeat"):
        _run(ctx, frame, state, lambda: _edit_span(ctx, frame, state, "set_downbeat"))
    state.coverage = _choice(
        "Coverage##source-map-coverage",
        state.coverage,
        (
            ("trusted", "Trusted"),
            ("uncertain", "Uncertain"),
            ("gap_unknown_count", "Unknown-count gap"),
        ),
    )
    imgui.same_line(spacing=SPACING)
    if imgui.button("Set coverage##source-map-set-coverage"):
        _run(ctx, frame, state, lambda: _edit_span(ctx, frame, state, "set_coverage"))


def _interval_fields(ctx: UiContext, frame: SourceMapEditorFrame, state: _EditorInputs) -> None:
    _range_selection(ctx, frame, state)
    try:
        span = _inspect_span(frame, state)
    except ValueError:
        imgui.text_colored(TEXT_MUTED_RGBA, "Selected interval unavailable")
        return
    _span_defaults(state, span)
    imgui.push_id(repr(state.inspected_span))
    try:
        if imgui.button("Use first anchor##source-map-use-first"):
            _run(ctx, frame, state, lambda: _use_span_anchor(ctx, frame, state, first=True))
        imgui.same_line(spacing=SPACING)
        if imgui.button("Use last anchor##source-map-use-last"):
            _run(ctx, frame, state, lambda: _use_span_anchor(ctx, frame, state, first=False))
    finally:
        imgui.pop_id()
    imgui.text_colored(
        TEXT_MUTED_RGBA, "Anchor marks show positions; coverage is reviewed separately."
    )
    _span_summary(span)
    imgui.text_wrapped(
        "Count proportionally redistributes selected existing beat coordinates and shifts later "
        "beat labels; source samples stay fixed. Changed counts clear selected downbeat phase. "
        "Trusted/uncertain coverage accepts the displayed current count; gap asserts no count."
    )
    imgui.push_id(repr(state.inspected_span))
    try:
        _interpretation_actions(ctx, frame, state)
    finally:
        imgui.pop_id()
    render_review_window(ctx, frame, span)


def _raw_inputs(ctx: UiContext, frame: SourceMapEditorFrame) -> _RawInputs:
    key = frame.raw.attempt, frame.raw.revision
    state = _RAW_INPUTS.get(ctx.ui.waveform)
    if state is None or state.key != key:
        state = _RawInputs(key)
        _RAW_INPUTS[ctx.ui.waveform] = state
    return state


def _run_raw(ctx: UiContext, frame: SourceMapEditorFrame, action: Callable[[], object]) -> None:
    if frame.identity is None or ctx.ui.source_map.identity(frame.pad_id) != frame.identity:
        ctx.ui.source_map.report_raw_error(frame.pad_id, "The original source changed.")
        return
    try:
        action()
    except (OSError, RuntimeError, ValueError) as error:
        ctx.ui.source_map.report_raw_error(frame.pad_id, str(error))
    else:
        ctx.ui.source_map.report_raw_error(frame.pad_id, None)


def _read_raw_page(
    ctx: UiContext, frame: SourceMapEditorFrame, start_index: int
) -> SourceBeatmapRawInspection:
    assert frame.raw.revision is not None
    return ctx.ui.source_map.selection.inspect_raw(
        frame.pad_id,
        expected_attempt=frame.raw.attempt,
        expected_revision=frame.raw.revision,
        start_index=start_index,
        max_events=128,
    )


def _show_raw_page(ctx: UiContext, frame: SourceMapEditorFrame, state: _RawInputs) -> None:
    page = _read_raw_page(ctx, frame, state.start_index)
    if page["status"] != "ready" or page["raw_revision"] != frame.raw.revision:
        imgui.text_colored(
            TEXT_MUTED_RGBA, "Original source placement is pending; no current rows."
        )
        return
    if imgui.button("Previous detections##source-raw-previous") and state.start_index > 0:
        _read_raw_page(ctx, frame, max(0, state.start_index - 128))
        state.start_index = max(0, state.start_index - 128)
    imgui.same_line(spacing=SPACING)
    imgui.begin_disabled(not page["has_more"])
    if imgui.button("Next detections##source-raw-next"):
        next_index = page["start_index"] + page["returned_count"]
        _read_raw_page(ctx, frame, next_index)
        state.start_index = next_index
    imgui.end_disabled()
    imgui.text_colored(
        TEXT_MUTED_RGBA,
        f"Detections {page['start_index']} to {page['start_index'] + page['returned_count']} "
        f"(exclusive) / {page['total_event_count']}",
    )
    imgui.begin_child("raw-detection-page", (-1, imgui.get_text_line_height_with_spacing() * 7))
    try:
        for event in page["events"]:
            index = event["raw_index"]
            if imgui.button(f"Use detection #{index}##source-raw-use-{index}"):
                _use_raw_detection(ctx, frame, state, index)
            imgui.same_line(spacing=SPACING)
            downbeat = "yes" if event["detected_downbeat"] else "no"
            detector_position = (
                "" if event["odf_frame"] is None else f"ODF frame {event['odf_frame']:.17g} | "
            )
            imgui.text_unformatted(
                f"#{event['raw_index']} | {detector_position}"
                f"source {event['source_seconds']:.17g} s | "
                f"detected downbeat: {downbeat} | {event['physical_domain']}"
            )
    finally:
        imgui.end_child()


def _require_raw_observation(ctx: UiContext, frame: SourceMapEditorFrame, index: int) -> None:
    info = frame.raw.info
    if info is None:
        message = "Raw detection metadata is unavailable."
        raise ValueError(message)
    if type(index) is not int or not 0 <= index < info["event_count"]:
        message = f"Raw event index must satisfy 0 <= index < {info['event_count']}."
        raise ValueError(message)
    assert frame.raw.revision is not None
    page = ctx.ui.source_map.selection.inspect_raw(
        frame.pad_id,
        expected_attempt=frame.raw.attempt,
        expected_revision=frame.raw.revision,
        start_index=index,
        max_events=1,
    )
    if (
        page["status"] != "ready"
        or page["raw_revision"] != frame.raw.revision
        or page["returned_count"] != 1
        or page["events"][0]["raw_index"] != index
        or page["events"][0]["physical_domain"] != "inside"
    ):
        message = "Choose one current detection inside the physical original source."
        raise ValueError(message)


def _use_raw_detection(
    ctx: UiContext, frame: SourceMapEditorFrame, state: _RawInputs, index: int
) -> None:
    _require_raw_observation(ctx, frame, index)
    state.raw_index, state.quarter_coordinate = str(index), ""


def _add_raw_label(ctx: UiContext, frame: SourceMapEditorFrame, state: _RawInputs) -> None:
    index, quarter = int(state.raw_index), float(state.quarter_coordinate)
    if not math.isfinite(quarter):
        message = "Enter an explicit finite quarter-note coordinate."
        raise ValueError(message)
    _require_raw_observation(ctx, frame, index)
    existing = next((row for row, label in enumerate(state.labels) if label[0] == index), None)
    if existing is not None:
        state.labels[existing] = index, quarter
        return
    if len(state.labels) >= 64:
        message = "This review accepts at most 64 explicit labels."
        raise ValueError(message)
    state.labels.append((index, quarter))


def _remove_raw_label(ctx: UiContext, frame: SourceMapEditorFrame, state: _RawInputs) -> None:
    index = int(state.raw_index)
    _require_raw_observation(ctx, frame, index)
    existing = next((row for row, label in enumerate(state.labels) if label[0] == index), None)
    if existing is None:
        message = "No explicit label exists for this raw event index."
        raise ValueError(message)
    state.labels.pop(existing)
    state.quarter_coordinate = ""


def _raw_label_controls(ctx: UiContext, frame: SourceMapEditorFrame, state: _RawInputs) -> None:
    imgui.set_next_item_width(110.0)
    _, state.raw_index = imgui.input_text("Raw event index##source-raw-index", state.raw_index)
    imgui.same_line(spacing=SPACING)
    imgui.set_next_item_width(150.0)
    _, state.quarter_coordinate = imgui.input_text(
        "Quarter-note coordinate##source-raw-quarter", state.quarter_coordinate
    )
    if imgui.button("Add/update label##source-raw-label"):
        _run_raw(ctx, frame, lambda: _add_raw_label(ctx, frame, state))
    imgui.same_line(spacing=SPACING)
    if imgui.button("Remove label##source-raw-remove-label"):
        _run_raw(ctx, frame, lambda: _remove_raw_label(ctx, frame, state))
    if state.labels:
        imgui.begin_child(
            "raw-explicit-labels", (-1, imgui.get_text_line_height_with_spacing() * 3)
        )
        try:
            for index, quarter in state.labels:
                imgui.text_unformatted(f"Raw #{index}: quarter-note coordinate {quarter:.17g}")
        finally:
            imgui.end_child()
    imgui.text_wrapped(
        "Supply labels in chronological order without filling unlabelled detections. "
        "Intervals start uncertain. Meter and bar phase remain unknown."
    )
    imgui.begin_disabled(len(state.labels) < 2 or frame.status in _BUSY)
    if imgui.button("Build source map from labels##source-raw-build"):
        revision = frame.raw.revision
        assert revision is not None
        _run_raw(
            ctx,
            frame,
            lambda: ctx.ui.source_map.selection.review_raw(
                frame.pad_id,
                list(state.labels),
                expected_attempt=frame.raw.attempt,
                expected_revision=revision,
            ),
        )
    imgui.end_disabled()


def _raw_review(ctx: UiContext, frame: SourceMapEditorFrame) -> None:
    raw = frame.raw
    label = raw.status.replace("_", "-")
    if raw.status == "none" and raw.occupied:
        label = "review slot occupied by another original source"
    elif raw.info is not None and raw.info["status"] == "placement_pending":
        label = "original source placement pending"
    imgui.text_colored(TEXT_MUTED_RGBA, f"Raw analysis: {label}")
    _raw_actions(ctx, frame)
    if raw.status == "failed":
        imgui.text_colored(TEXT_MUTED_RGBA, "Raw analysis failed; see console diagnostics.")
        if raw.error:
            imgui.text_wrapped(raw.error)
    if raw.status != "unreviewed" or raw.info is None:
        return
    imgui.text_wrapped(
        "Unreviewed detector observations. Quarter-note count and meter are unverified; "
        "timing uncertainty is unknown. Detected downbeats are observations."
    )
    if raw.info["status"] != "ready":
        return
    _raw_provenance(frame)
    state = _raw_inputs(ctx, frame)
    _raw_label_controls(ctx, frame, state)
    if imgui.button("Inspect detections##source-raw-inspect"):
        state.inspecting = True
    if state.inspecting:
        _run_raw(ctx, frame, lambda: _show_raw_page(ctx, frame, state))


def _raw_actions(ctx: UiContext, frame: SourceMapEditorFrame) -> None:
    raw = frame.raw
    imgui.begin_disabled(raw.occupied or frame.status in _BUSY or frame.identity is None)
    action = "Analyze again" if raw.status == "failed" else "Analyze original full mix (Beat This)"
    if imgui.button(f"{action}##source-raw-analyze"):
        _run_raw(
            ctx,
            frame,
            lambda: ctx.ui.source_map.selection.analyze_original(
                frame.pad_id, expected_attempt=raw.attempt
            ),
        )
    imgui.end_disabled()
    if raw.status in {"queued", "running", "unreviewed"}:
        imgui.same_line(spacing=SPACING)
        if imgui.button("Discard candidate##source-raw-discard"):
            _run_raw(
                ctx,
                frame,
                lambda: ctx.ui.source_map.selection.discard_raw(
                    frame.pad_id, expected_attempt=raw.attempt, expected_revision=raw.revision
                ),
            )


def _raw_provenance(frame: SourceMapEditorFrame) -> None:
    raw = frame.raw
    assert raw.info is not None
    duration = raw.info["source_frame_count"] / raw.info["source_sample_rate_hz"]
    imgui.text_colored(
        TEXT_MUTED_RGBA,
        f"Original full mix: {raw.info['source_frame_count']} frames at "
        f"{raw.info['source_sample_rate_hz']} Hz, {duration:.17g} s, "
        f"frame zero {raw.info['source_zero_frame']} | {raw.info['event_count']} detections",
    )
    if imgui.tree_node("Input provenance##source-raw-provenance"):
        try:
            if raw.info.get("backend") == "beat_this":
                detector_input = (
                    f"Worker input: {raw.info['worker_input_frame_count']} frames at "
                    f"{raw.info['worker_input_sample_rate_hz']} Hz, "
                    f"frame zero {raw.info['worker_input_source_zero_frame']}\n"
                    f"Worker input SHA-256: {raw.info['worker_input_sha256']}\n"
                    f"Frontend identity: {raw.info['frontend_identity']}\n"
                    f"Model: {raw.info['model']}\n"
                    f"Complete logits: {raw.info['beat_logit_count']} beat / "
                    f"{raw.info['downbeat_logit_count']} downbeat\n"
                )
            else:
                detector_input = (
                    f"Analyzer: {raw.info['analyzer_frame_count']} frames at "
                    f"{raw.info['analyzer_sample_rate_hz']} Hz | "
                    f"Conversion: {raw.info['conversion_revision']}\n"
                    f"Analyzer SHA-256: {raw.info['analyzer_sha256']}\n"
                    f"Hop: {raw.info['odf_hop_samples']} | Settings: {raw.info['configuration']}\n"
                )
            imgui.text_wrapped(
                f"Decoder: {raw.info['decoder_identity']} | Mono rule: {raw.info['mono_rule']}\n"
                f"Mono SHA-256: {raw.info['mono_sha256']}\n"
                f"{detector_input}"
                f"Raw revision: {raw.revision} | "
                f"Detected downbeats: {raw.info['detected_downbeat_count']} | "
                f"Outside source: {raw.info['outside_source_event_count']}\n"
                f"Detector: {raw.info['detector_id']} | Code: {raw.info['code_revision']} | "
                f"Capture: {raw.info.get('capture_encoding', 'legacy QM')}"
            )
        finally:
            imgui.tree_pop()


def _render_raw_review(ctx: UiContext, frame: SourceMapEditorFrame) -> None:
    # Independent attempt identity prevents an equal-digest reanalysis accepting an old release.
    imgui.push_id(repr((frame.pad_id, frame.identity, frame.raw.attempt, frame.raw.revision)))
    try:
        _raw_review(ctx, frame)
    finally:
        imgui.pop_id()


def render_source_map_editor(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    timing: CurrentPadTiming | None,
) -> None:
    """Render explicit manual map controls without changing audio or scalar intent."""
    observe_manual_beat(ctx, frame)
    # A mouse press on the old source/revision must not release onto a new one.
    imgui.push_id(repr((frame.pad_id, frame.identity, frame.revision)))
    try:
        _render_source_map_controls(ctx, frame, timing)
    finally:
        imgui.pop_id()


def _render_source_map_controls(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    timing: CurrentPadTiming | None,
) -> None:
    state = _inputs(ctx, frame)
    sync = ctx.audio.pads.source_sync_status(frame.pad_id)
    preference = ctx.audio.pads.source_sync_preference(frame.pad_id)
    imgui.begin_disabled(frame.identity is None)
    if imgui.button(f"SYNC PAD {'ON' if preference else 'OFF'}##source-sync-toggle", (-1, 0)):
        ctx.audio.pads.toggle_source_sync(frame.pad_id)
    imgui.end_disabled()
    effective = "unconfirmed" if sync.effective is None else "ON" if sync.effective else "OFF"
    imgui.push_id("source-sync-status")
    try:
        _status_line(
            f"SYNC requested {'ON' if sync.requested else 'OFF'} | Audio {effective} | {sync.state}"
        )
    finally:
        imgui.pop_id()
    if sync.state in {"pending", "stopped"}:
        imgui.text_wrapped("Start/retrigger to apply this timing change.")
    expanded = imgui.collapsing_header("Source Map##source-map-controls")
    revision = "" if frame.revision is None else f" | Revision {frame.revision[:12]}"
    label = f"Source map {frame.status}{revision} | {_local_tempo_label(ctx, frame)}"
    _status_line(label)
    if not expanded:
        discard_manual_beat(ctx)
        return

    imgui.begin_disabled(frame.status in _BUSY or frame.identity is None)
    if not frame.selected:
        valid = (
            timing is not None
            and math.isfinite(timing.origin_seconds)
            and math.isfinite(timing.period_seconds)
            and timing.period_seconds > 0
        )
        imgui.begin_disabled(not valid)
        if imgui.button("Create manual 4/4 map##source-map-create") and timing is not None:
            _run(
                ctx,
                frame,
                state,
                lambda: ctx.ui.source_map.selection.create_manual(
                    frame.pad_id, timing.origin_seconds, timing.period_seconds
                ),
            )
        imgui.end_disabled()
        imgui.text_wrapped("Uses the current source grid as an explicit manual 4/4 map.")
    else:
        if imgui.button("Clear source map##source-map-clear"):
            _run(ctx, frame, state, lambda: ctx.ui.source_map.selection.clear(frame.pad_id))
        imgui.begin_disabled(frame.geometry is None)
        if frame.geometry is not None:
            _correction_fields(ctx, frame, state)
            render_manual_beat(ctx, frame)
            _interval_fields(ctx, frame, state)
        imgui.end_disabled()
    imgui.end_disabled()
    _render_raw_review(ctx, frame)
