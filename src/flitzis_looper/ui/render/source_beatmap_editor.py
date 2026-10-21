"""Transient editor intent over the loader's source-qualified Native map."""

import math
from contextlib import suppress
from dataclasses import dataclass
from typing import TYPE_CHECKING
from weakref import WeakKeyDictionary

from imgui_bundle import imgui

from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.current_timing import CurrentPadTiming
    from flitzis_looper.ui.context import UiContext, WaveformEditorActions
    from flitzis_looper_audio import SourceBeatmap, SourceBeatmapEditorSpan


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


_INPUTS: WeakKeyDictionary[WaveformEditorActions, _EditorInputs] = WeakKeyDictionary()
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


def render_source_map_editor(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    timing: CurrentPadTiming | None,
) -> None:
    """Render explicit manual map controls without changing audio or scalar intent."""
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
    expanded = imgui.collapsing_header("Source Map##source-map-controls")
    revision = "" if frame.revision is None else f" | Revision {frame.revision[:12]}"
    label = (
        f"Editor only | Audio: scalar | {frame.status}{revision} | {_local_tempo_label(ctx, frame)}"
    )
    _status_line(label)
    if not expanded:
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
            _interval_fields(ctx, frame, state)
        imgui.end_disabled()
    imgui.end_disabled()
