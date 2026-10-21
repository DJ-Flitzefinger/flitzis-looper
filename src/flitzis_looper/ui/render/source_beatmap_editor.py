"""Transient editor intent over the loader's source-qualified Native map."""

import math
from dataclasses import dataclass
from typing import TYPE_CHECKING
from weakref import WeakKeyDictionary

from imgui_bundle import imgui

from flitzis_looper.ui.constants import SPACING, TEXT_MUTED_RGBA

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.current_timing import CurrentPadTiming
    from flitzis_looper.ui.context import UiContext, WaveformEditorActions
    from flitzis_looper_audio import SourceBeatmap


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
    can_undo: bool
    error: str | None


@dataclass(slots=True)
class _EditorInputs:
    key: tuple[int, object, str | None]
    beat: str = "0"
    sample: str = "0"
    translation: str = "0"
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
        can_undo=False if info is None else info["can_undo"],
        error=maps.errors.get(pad_id),
    )


def _inputs(ctx: UiContext, frame: SourceMapEditorFrame) -> _EditorInputs:
    key = (frame.pad_id, frame.identity, frame.revision)
    state = _INPUTS.get(ctx.ui.waveform)
    if state is None or state.key != key:
        state = _EditorInputs(key)
        _INPUTS[ctx.ui.waveform] = state
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
        state.error = "The playhead has no supported musical beat coordinate."
        return
    state.beat = format(beat, ".17g")
    state.sample = str(round(source * frame.rate))
    state.error = None


def _run(
    ctx: UiContext,
    frame: SourceMapEditorFrame,
    state: _EditorInputs,
    action: Callable[[], None],
) -> None:
    """Refuse an old text target before the loader captures its own Native job."""
    if frame.identity is None or ctx.ui.source_map.identity(frame.pad_id) != frame.identity:
        state.error = "The source changed. Reopen the Source Map controls before editing."
        return
    try:
        action()
    except (OSError, RuntimeError, ValueError) as error:
        state.error = str(error)
    else:
        state.error = None


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
    """Keep pending/error text from changing the waveform's vertical layout."""
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
        _use_playhead(ctx, frame, state)
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
    error = state.error or frame.error
    _status_line(label if error is None else f"{label} | {error}")
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
        imgui.end_disabled()
    imgui.end_disabled()
