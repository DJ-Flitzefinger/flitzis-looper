"""Actual ImGui/ImPlot frames, without an app, graphics backend or audio device."""

import gzip
import json
import os
from contextlib import contextmanager
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, NotRequired, TypedDict, Unpack
from unittest.mock import Mock

import imgui_bundle
import numpy as np
import pytest
from imgui_bundle import imgui, implot

from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import render, waveform_editor

if TYPE_CHECKING:
    from collections.abc import Iterator

    from numpy.typing import NDArray

    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.waveform_grid import WaveformGridLine
    from flitzis_looper_audio import WaveFormRenderData


@dataclass
class _PendingProjection:
    """Only the native worker boundary is replaced, with delayed view completion."""

    key: tuple[int, int, float, float] | None = None
    polls: int = 0
    requests: list[tuple[int, int, float, float]] = field(default_factory=list)
    status: str = "idle"
    fail: bool = False
    raw: bool = False

    def read(self, pad: int, width: int, start: float, end: float) -> WaveFormRenderData | None:
        key = (pad, width, start, end)
        if key != self.key:
            self.key = key
            self.polls = 0
            self.requests.append(key)
        self.polls += 1
        self.status = "error" if self.fail else "pending"
        if self.fail or self.polls < 3:
            return None
        self.status = "ready"
        self.raw = (end - start) * 48_000 < 2 * width
        count = 16 if self.raw else width
        xs = np.linspace(start, end, count, dtype=np.float64)
        ys = np.linspace(-0.5, -0.25, count, dtype=np.float32)
        return self.raw, xs, ys, None if self.raw else -ys

    def readiness(self, _pad: int) -> tuple[str, str | None]:
        return self.status, "injected worker failure" if self.status == "error" else None

    def retry(self, _pad: int) -> None:
        self.polls = 0
        self.status = "idle"


@dataclass(frozen=True)
class _Frame:
    scrollbar: bool
    plot_x: float
    plot_width: float
    vertices: tuple[tuple[float, float, float, float, int], ...]
    indices: tuple[int, ...]


@contextmanager
def _plot_context() -> Iterator[None]:
    """Create isolated native draw contexts, retaining any existing test contexts."""
    previous_imgui = imgui.get_current_context()
    previous_implot = implot.get_current_context()
    owned_imgui = imgui.create_context()
    imgui.set_current_context(owned_imgui)
    owned_implot = implot.create_context()
    implot.set_current_context(owned_implot)
    io = imgui.get_io()
    io.set_ini_filename(None)
    io.display_size = imgui.ImVec2(1200, 700)
    io.delta_time = 1 / 60
    # Dynamic fonts can produce draw data without uploading a graphics texture.
    io.backend_flags |= imgui.BackendFlags_.renderer_has_textures
    assets = Path(imgui_bundle.__file__).parent / "assets/fonts/Roboto"
    io.fonts.add_font_from_file_ttf(str(assets / "Roboto-Regular.ttf"), 16.0)
    io.fonts.add_font_from_file_ttf(str(assets / "Roboto-Bold.ttf"), 16.0)
    try:
        yield
    finally:
        implot.destroy_context(owned_implot)
        imgui.destroy_context(owned_imgui)
        if previous_implot is not None:
            implot.set_current_context(previous_implot)
        if previous_imgui is not None:
            imgui.set_current_context(previous_imgui)


@pytest.fixture(params=[False, True], ids=["current-context", "nested-context"])
def headless_plot(request: pytest.FixtureRequest) -> Iterator[None]:
    """Exercise frame isolation both directly and with pre-existing native contexts."""
    with _plot_context():
        if request.param:
            previous_imgui = imgui.get_current_context()
            previous_implot = implot.get_current_context()
            with _plot_context():
                yield
            assert imgui.get_current_context() is previous_imgui
            assert implot.get_current_context() is previous_implot
        else:
            yield


def _frame(
    ctx: UiContext, *, width: float = 900, limits: tuple[float, float] | None = None
) -> _Frame:
    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size((1000, 650))
    imgui.begin("renderer regression")
    # Reproduce the real center/editor child layout and its zero item spacing.
    imgui.push_style_var(imgui.StyleVar_.item_spacing, (0, 0))
    imgui.begin_child("center", (width, 550))
    imgui.begin_child("waveform_editor", (-1, -1))
    if limits is not None:
        implot.set_next_axis_limits(implot.ImAxis_.x1, *limits, imgui.Cond_.always)
    waveform_editor._render_plot(ctx, 0)
    window = imgui.internal.get_current_window()
    draw_list = window.draw_list
    # Native retained plot geometry remains inspectable after EndPlot.
    plot = implot.internal.get_plot("##waveform-0")
    assert plot is not None
    rectangle = plot.plot_rect
    result = _Frame(
        window.scrollbar_y,
        rectangle.min.x,
        rectangle.max.x - rectangle.min.x,
        tuple((v.pos.x, v.pos.y, v.uv.x, v.uv.y, v.col) for v in draw_list.vtx_buffer),
        tuple(draw_list.idx_buffer),
    )
    imgui.end_child()
    imgui.end_child()
    imgui.pop_style_var()
    imgui.end()
    imgui.render()
    return result


def _context(controller: AppController, audio: Mock) -> tuple[UiContext, _PendingProjection]:
    controller.project.sample_paths[0] = "samples/renderer.wav"
    controller.project.sample_durations[0] = 600.0
    controller.project.manual_bpm[0] = 120.0
    controller.project.pad_loop_end_s[0] = 16.0
    audio.waveform_source_identity.return_value = (7, "a" * 64, 28_800_000, 48_000)
    worker = _PendingProjection()
    audio.get_waveform_render_data.side_effect = worker.read
    audio.waveform_readiness.side_effect = worker.readiness
    audio.retry_waveform.side_effect = worker.retry
    return UiContext(controller), worker


@dataclass
class _DrawObservation:
    labels: list[tuple[int, int, dict[tuple[float, float, float, float], float]]] = field(
        default_factory=list
    )
    representations: list[str] = field(default_factory=list)
    buttons: dict[str, tuple[float, float, float, float]] = field(default_factory=dict)
    waveform_ranges: list[tuple[int, int]] = field(default_factory=list)
    window: imgui.internal.Window | None = None
    plot: implot.internal.Plot | None = None
    candidates: int = 0


def _observe_draw(monkeypatch: pytest.MonkeyPatch) -> _DrawObservation:
    """Observe the unchanged productive draws, including actual font glyphs."""
    result = _DrawObservation()
    original_labels = waveform_editor._draw_musical_grid_labels
    original_line = waveform_editor._plot_line
    original_shaded = waveform_editor._plot_shaded
    original_button = imgui.button
    original_body = waveform_editor._render_editor_body

    def body(ctx: UiContext, pad_id: int) -> None:
        original_body(ctx, pad_id)
        result.window = imgui.internal.get_current_window()
        result.plot = implot.internal.get_plot(f"##waveform-{pad_id}")

    def labels(
        draw_list: imgui.ImDrawList,
        lines: tuple[WaveformGridLine, ...],
        *,
        loop_start_s: float,
        start_s: float,
        end_s: float,
    ) -> None:
        first = len(draw_list.vtx_buffer)
        result.candidates += sum(line.major for line in lines)
        result.candidates += start_s <= loop_start_s <= end_s
        original_labels(draw_list, lines, loop_start_s=loop_start_s, start_s=start_s, end_s=end_s)
        baked = imgui.get_font().get_font_baked(imgui.get_font_size())
        glyphs = {}
        for character in "0123456789Loop .-/":
            glyph = baked.find_glyph(ord(character))
            glyphs[glyph.u0, glyph.v0, glyph.u1, glyph.v1] = glyph.y0
        result.labels.append((first, len(draw_list.vtx_buffer), glyphs))

    def line(
        xs: NDArray[np.float64], ys: NDArray[np.float32], *, show_sample_markers: bool
    ) -> None:
        result.representations.append("raw")
        draw_list = implot.get_plot_draw_list()
        first = len(draw_list.vtx_buffer)
        original_line(xs, ys, show_sample_markers=show_sample_markers)
        result.waveform_ranges.append((first, len(draw_list.vtx_buffer)))

    def shaded(
        xs: NDArray[np.float64], y_min: NDArray[np.float32], y_max: NDArray[np.float32]
    ) -> None:
        result.representations.append("envelope")
        draw_list = implot.get_plot_draw_list()
        first = len(draw_list.vtx_buffer)
        original_shaded(xs, y_min, y_max)
        result.waveform_ranges.append((first, len(draw_list.vtx_buffer)))

    def button(label: str, size: imgui.ImVec2Like | None = None) -> bool:
        clicked = original_button(label, size)
        lo, hi = imgui.get_item_rect_min(), imgui.get_item_rect_max()
        result.buttons[label] = (lo.x, lo.y, hi.x, hi.y)
        return clicked

    monkeypatch.setattr(waveform_editor, "_draw_musical_grid_labels", labels)
    monkeypatch.setattr(waveform_editor, "_plot_line", line)
    monkeypatch.setattr(waveform_editor, "_plot_shaded", shaded)
    monkeypatch.setattr(imgui, "button", button)
    monkeypatch.setattr(waveform_editor, "_render_editor_body", body)
    return result


def _visible_labels(draw_list: imgui.ImDrawList, observation: _DrawObservation) -> list[float]:
    """Read glyph origins only when actual indexed geometry intersects its command clip."""
    origins = []
    commands = [
        (
            c.clip_rect,
            {
                draw_list.idx_buffer[i] + c.vtx_offset
                for i in range(c.idx_offset, c.idx_offset + c.elem_count)
            },
        )
        for c in draw_list.cmd_buffer
    ]
    for first, last, glyphs in observation.labels:
        for index in range(first, last, 4):
            quad = [draw_list.vtx_buffer[i] for i in range(index, min(index + 4, last))]
            if len(quad) != 4:
                continue
            signature = (quad[0].uv.x, quad[0].uv.y, quad[2].uv.x, quad[2].uv.y)
            assert signature in glyphs, "Observed text vertices must match real baked glyph UVs"
            for clip, indices in commands:
                used = any(i in indices for i in range(index, index + 4))
                visible = (
                    min(v.pos.x for v in quad) < clip.z
                    and max(v.pos.x for v in quad) > clip.x
                    and min(v.pos.y for v in quad) < clip.w
                    and max(v.pos.y for v in quad) > clip.y
                )
                if used and visible:
                    origins.append(round(min(v.pos.y for v in quad) - glyphs[signature], 3))
                    break
    return sorted(set(origins))


def _waveform_clips(
    draw_list: imgui.ImDrawList, observation: _DrawObservation
) -> list[list[float]]:
    clips = []
    for command in draw_list.cmd_buffer:
        indices = {
            draw_list.idx_buffer[i] + command.vtx_offset
            for i in range(command.idx_offset, command.idx_offset + command.elem_count)
        }
        if any(
            any(first <= index < last for index in indices)
            for first, last in observation.waveform_ranges
        ):
            clip = command.clip_rect
            clips.append([clip.x, clip.y, clip.z, clip.w])
    return clips


class _EditorFrame(TypedDict):
    rect: list[float]
    limits: list[float]
    readiness: str
    worker: str
    query: list[int | float] | None
    cache_query: list[int | float | None]
    source: str
    scrollbar: bool
    bands: list[float]
    label_candidates: int
    representations: list[str]
    buttons: dict[str, tuple[float, float, float, float]]
    commands: list[list[int | float]]
    vertices: int
    indices: int
    waveform_ranges: list[tuple[int, int]]
    waveform_clips: list[list[float]]
    draw_vertices: NotRequired[list[list[int | float]]]
    draw_indices: NotRequired[list[int]]
    label_ranges: NotRequired[list[tuple[int, int]]]


class _FrameOptions(TypedDict):
    width: float
    scale: float


def _full_frame(
    ctx: UiContext,
    worker: _PendingProjection,
    observation: _DrawObservation,
    *,
    width: float = 1500,
    scale: float = 1.0,
    mouse: tuple[float, float] | None = None,
    wheel: float = 0,
    down: bool | None = None,
    limits: tuple[float, float] | None = None,
) -> _EditorFrame:
    """Render the complete productive editor in its actual center child/spacing layout."""
    io = imgui.get_io()
    io.display_size = imgui.ImVec2(width + 100, 1000 * scale)
    io.display_framebuffer_scale = imgui.ImVec2(scale, scale)
    imgui.get_style().font_scale_main = scale
    if mouse is not None:
        io.add_mouse_pos_event(*mouse)
    if wheel:
        io.add_mouse_wheel_event(0, wheel)
    if down is not None:
        io.add_mouse_button_event(imgui.MouseButton_.left, down)
    observation.labels.clear()
    observation.representations.clear()
    observation.buttons.clear()
    observation.waveform_ranges.clear()
    observation.candidates = 0
    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size((width + 50, 900 * scale))
    imgui.begin("full editor regression")
    imgui.begin_child("center_area", (width, 800 * scale))
    if limits is not None:
        implot.set_next_axis_limits(implot.ImAxis_.x1, *limits, imgui.Cond_.always)
    render._center_area(ctx)
    window = observation.window
    assert window is not None
    plot = observation.plot
    assert plot is not None
    rectangle = plot.plot_rect
    draw_list = window.draw_list
    result: _EditorFrame = {
        "rect": [rectangle.min.x, rectangle.min.y, rectangle.max.x, rectangle.max.y],
        "limits": list(ctx.ui.waveform._pad_view_ranges[0]),
        "readiness": ctx.ui.waveform.readiness(0)[0],
        "worker": worker.status,
        "query": list(worker.key) if worker.key else None,
        "cache_query": [
            ctx.ui.waveform._last_width_px,
            ctx.ui.waveform._last_start_s,
            ctx.ui.waveform._last_end_s,
        ],
        "source": repr(ctx.ui.waveform._last_source_identity),
        "scrollbar": window.scrollbar_y,
        "bands": _visible_labels(draw_list, observation),
        "label_candidates": observation.candidates,
        "representations": list(observation.representations),
        "buttons": dict(observation.buttons),
        "commands": [
            [
                c.clip_rect.x,
                c.clip_rect.y,
                c.clip_rect.z,
                c.clip_rect.w,
                c.idx_offset,
                c.elem_count,
                c.vtx_offset,
            ]
            for c in draw_list.cmd_buffer
        ],
        "vertices": len(draw_list.vtx_buffer),
        "indices": len(draw_list.idx_buffer),
        "waveform_ranges": list(observation.waveform_ranges),
        "waveform_clips": _waveform_clips(draw_list, observation),
    }
    if os.environ.get("LOOPER_UI_DRAW_DIR"):
        result["draw_vertices"] = [
            [v.pos.x, v.pos.y, v.uv.x, v.uv.y, v.col] for v in draw_list.vtx_buffer
        ]
        result["draw_indices"] = list(draw_list.idx_buffer)
        result["label_ranges"] = [(first, last) for first, last, _ in observation.labels]
    imgui.end_child()
    imgui.end()
    imgui.render()
    return result


def _write_frames(name: str, frames: list[_EditorFrame]) -> None:
    directory = os.environ.get("LOOPER_UI_DRAW_DIR")
    if directory:
        path = Path(directory) / f"{name}.json.gz"
        with gzip.open(path, "wt", encoding="utf-8") as target:
            json.dump(frames, target)


def _assert_fixed_geometry(frames: list[_EditorFrame], scale: float) -> None:
    rectangles = [frame["rect"] for frame in frames]
    assert len({(rect[1], rect[3]) for rect in rectangles}) == 1
    for frame in frames:
        assert len(frame["representations"]) <= 1
        assert len(frame["bands"]) == bool(frame["label_candidates"])
        assert not frame["scrollbar"]
        for band in frame["bands"]:
            assert abs(band - frame["rect"][1] - 4) <= 1.0
        if frame["readiness"] == "ready":
            assert len(frame["representations"]) == 1
        else:
            assert frame["representations"] == []
        if frame["waveform_ranges"]:
            first, last = frame["waveform_ranges"][0]
            assert last > first, "Valid projection must emit actual waveform geometry"
            assert frame["waveform_clips"]
            for clip in frame["waveform_clips"]:
                rect = frame["rect"]
                assert clip[0] >= rect[0]
                assert clip[1] >= rect[1]
                assert clip[2] <= rect[2]
                assert clip[3] <= rect[3]
    assert rectangles[0][3] - rectangles[0][1] > 100 * scale


def _click_toolbar(
    label: str,
    ctx: UiContext,
    worker: _PendingProjection,
    observer: _DrawObservation,
    frames: list[_EditorFrame],
    **options: Unpack[_FrameOptions],
) -> None:
    rect = frames[-1]["buttons"][label]
    mouse = ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
    frames.extend((
        _full_frame(ctx, worker, observer, mouse=mouse, **options),
        _full_frame(ctx, worker, observer, mouse=mouse, down=True, **options),
        _full_frame(ctx, worker, observer, mouse=mouse, down=False, **options),
    ))


def _navigate_toolbar_views(
    ctx: UiContext,
    worker: _PendingProjection,
    observer: _DrawObservation,
    frames: list[_EditorFrame],
    **options: Unpack[_FrameOptions],
) -> None:
    for label in ("Zoom to Loop", "Reset Zoom"):
        before = frames[-1]["limits"]
        _click_toolbar(label, ctx, worker, observer, frames, **options)
        frames.extend(_full_frame(ctx, worker, observer, **options) for _ in range(4))
        assert frames[-1]["limits"] != before
        assert frames[-1]["bands"]


def _replace_source_in_view(
    ctx: UiContext,
    audio: Mock,
    worker: _PendingProjection,
    observer: _DrawObservation,
    frames: list[_EditorFrame],
    **options: Unpack[_FrameOptions],
) -> None:
    audio.waveform_source_identity.return_value = (9, "b" * 64, 28_800_000, 48_000)
    worker.key = None
    frames.extend(_full_frame(ctx, worker, observer, **options) for _ in range(5))
    limits = ctx.ui.waveform.view_limits(0)
    assert limits is not None
    assert frames[-1]["limits"] == list(limits)
    assert frames[-1]["source"] != frames[0]["source"]


@pytest.mark.usefixtures("headless_plot")
def test_full_editor_wheel_transition_keeps_plot_and_upper_row_stationary(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    request: pytest.FixtureRequest,
) -> None:
    ctx, worker = _context(controller, audio_engine_mock)
    ctx.bold_font = imgui.get_io().fonts.fonts[1]
    ctx.ui.waveform.open(0)
    observer = _observe_draw(monkeypatch)
    initial = [_full_frame(ctx, worker, observer) for _ in range(8)]
    rect = initial[-1]["rect"]
    mouse = ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
    hovered = _full_frame(ctx, worker, observer, mouse=mouse)
    frames = [hovered, _full_frame(ctx, worker, observer, mouse=mouse, wheel=1)]
    frames.extend(_full_frame(ctx, worker, observer, mouse=mouse) for _ in range(8))
    receipt = os.environ.get("LOOPER_UI_FRAME_RECEIPT")
    if receipt:
        Path(receipt).write_text(
            json.dumps({"initial": initial, "zoom": frames}, indent=2), encoding="utf-8"
        )
    _write_frames(request.node.name, initial + frames)
    assert frames[1]["limits"] != hovered["limits"], "Real hovered wheel must change X limits"
    assert len({(f["rect"][1], f["rect"][3]) for f in frames}) == 1
    assert all(len(f["bands"]) <= 1 for f in initial + frames)
    assert all(len(f["representations"]) <= 1 for f in initial + frames)
    assert not any(f["scrollbar"] for f in initial + frames)


@pytest.mark.usefixtures("headless_plot")
@pytest.mark.parametrize("scale", [1.0, 1.25, 1.5, 2.0])
@pytest.mark.parametrize("width", [650, 1500], ids=["narrow", "ordinary"])
def test_full_editor_geometry_matrix(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    request: pytest.FixtureRequest,
    scale: float,
    width: float,
) -> None:
    """Check every actual request/transition frame, including genuine navigation gestures."""
    ctx, worker = _context(controller, audio_engine_mock)
    ctx.bold_font = imgui.get_io().fonts.fonts[1]
    ctx.ui.waveform.open(0)
    observer = _observe_draw(monkeypatch)
    options: _FrameOptions = {"scale": scale, "width": width * scale}
    frames = [_full_frame(ctx, worker, observer, **options) for _ in range(6)]
    assert frames[-1]["bands"], "Full view has visible major-number candidates"
    _navigate_toolbar_views(ctx, worker, observer, frames, **options)
    for wheel in (1, -1, 2, -2):
        rect = frames[-1]["rect"]
        mouse = ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
        frames.append(_full_frame(ctx, worker, observer, mouse=mouse, **options))
        before = frames[-1]["limits"]
        frames.append(_full_frame(ctx, worker, observer, mouse=mouse, wheel=wheel, **options))
        assert frames[-1]["limits"] != before
        frames.extend(_full_frame(ctx, worker, observer, **options) for _ in range(4))
    # Forced views supplement real wheel and toolbar navigation for the extreme raw/tail cases.
    for limits in ((0, 0.001), (599.1, 599.101), (-0.5, 600), (-0.5, 16)):
        frames.append(_full_frame(ctx, worker, observer, limits=limits, **options))
        frames.extend(_full_frame(ctx, worker, observer, **options) for _ in range(4))
        assert (frames[-1]["representations"] == ["raw"]) == (limits[1] - limits[0] < 0.01)
        if limits == (599.1, 599.101):
            assert frames[-1]["bands"] == [], "No invented numbers in a grid-free tail view"
    worker.fail = True
    ctx.ui.waveform.retry(0)
    frames.extend(_full_frame(ctx, worker, observer, **options) for _ in range(3))
    assert frames[-1]["readiness"] == "error"
    worker.fail = False
    _click_toolbar("Retry##waveform-0", ctx, worker, observer, frames, **options)
    frames.extend(_full_frame(ctx, worker, observer, **options) for _ in range(4))
    assert frames[-1]["readiness"] == "ready"
    controller.session.active_sample_ids.add(0)
    residency = Mock(return_value=("pending", None))
    monkeypatch.setattr(ctx.audio.pads, "residency_status", residency)
    cancel = Mock()
    monkeypatch.setattr(ctx.audio.pads, "cancel_residency", cancel)
    frames.append(_full_frame(ctx, worker, observer, **options))
    _click_toolbar("Cancel##resident-context-0", ctx, worker, observer, frames, **options)
    cancel.assert_called_once_with(0)
    for state, error, playhead in (
        ("adopting", None, 1.5),
        ("error", "x" * 500, 1.8),
        ("ready", None, None),
    ):
        residency.return_value = (state, error)
        controller.session.pad_playhead_s[0] = playhead
        frames.append(_full_frame(ctx, worker, observer, **options))
    _replace_source_in_view(ctx, audio_engine_mock, worker, observer, frames, **options)
    _write_frames(request.node.name, frames)
    _assert_fixed_geometry(frames, scale)
    # Cache polling stops at the native projection readiness boundary; all stable frames reuse it.
    calls = audio_engine_mock.get_waveform_render_data.call_count
    for _ in range(3):
        _full_frame(ctx, worker, observer, **options)
    assert audio_engine_mock.get_waveform_render_data.call_count == calls
    audio_engine_mock.seek_sample.assert_not_called()
    audio_engine_mock.prepare_resident_control.assert_not_called()


@pytest.mark.usefixtures("headless_plot")
@pytest.mark.parametrize("limits", [(-0.5, 600.0), (-0.5, 40.0), (-0.5, 16.0), (599.0, 599.001)])
def test_pending_waveform_settles_without_scrollbar_or_displaced_scales(
    controller: AppController, audio_engine_mock: Mock, limits: tuple[float, float]
) -> None:
    ctx, worker = _context(controller, audio_engine_mock)
    _frame(ctx)  # Initialize ImPlot's once-only source limits before navigation.
    frames = [_frame(ctx, limits=limits if index == 0 else None) for index in range(18)]

    assert worker.status == "ready"
    assert ctx.ui.waveform._pad_view_ranges[0] == limits
    assert len(worker.requests) == (1 if limits == (-0.5, 600.0) else 2)
    assert worker.polls == 3  # The real UI cache stops native polling once ready.
    assert not any(frame.scrollbar for frame in frames)
    assert len({(frame.plot_x, frame.plot_width) for frame in frames}) == 1
    # Full native axis, grid, marker and waveform draw geometry stops alternating.
    assert all(frame == frames[-1] for frame in frames[3:])
    assert frames[0].vertices != frames[-1].vertices  # Waveform genuinely appears.
    assert worker.raw == (limits[1] - limits[0] < 0.01)


@pytest.mark.usefixtures("headless_plot")
def test_waveform_readiness_playhead_zoom_resize_and_source_transitions(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx, worker = _context(controller, audio_engine_mock)
    _frame(ctx)
    frames = [_frame(ctx, limits=(-0.5, 40.0) if index == 0 else None) for index in range(6)]
    initial_width = frames[-1].plot_width
    controller.session.active_sample_ids.add(0)
    for playhead in (1.5, 41.0, None):
        controller.session.pad_playhead_s[0] = playhead
        frames.append(_frame(ctx))
    residency = Mock(return_value=("adopting", None))
    monkeypatch.setattr(ctx.audio.pads, "residency_status", residency)
    frames.append(_frame(ctx))
    residency.return_value = ("error", "injected residency failure")
    frames.append(_frame(ctx))
    residency.return_value = ("ready", None)
    worker.fail = True
    ctx.ui.waveform.retry(0)
    frames.extend(_frame(ctx) for _ in range(4))
    assert ctx.ui.waveform.readiness(0)[0] == "error"
    worker.fail = False
    ctx.ui.waveform.retry(0)
    frames.extend(_frame(ctx) for _ in range(6))
    assert worker.status == "ready"
    assert len(worker.requests) == 2
    assert all(frame.plot_width == initial_width for frame in frames)

    frames.extend(
        _frame(ctx, limits=(599.0, 599.001) if index == 0 else None) for index in range(6)
    )
    assert worker.raw
    frames.extend(_frame(ctx, limits=(-0.5, 40.0) if index == 0 else None) for index in range(6))
    assert not worker.raw
    frames.extend(_frame(ctx, width=760) for _ in range(6))
    assert frames[-1].plot_width < initial_width
    audio_engine_mock.waveform_source_identity.return_value = (9, "b" * 64, 28_800_000, 48_000)
    # The source identity fences the real cache even when the viewport is identical.
    previous_calls = audio_engine_mock.get_waveform_render_data.call_count
    frames.append(_frame(ctx, width=760))
    assert audio_engine_mock.get_waveform_render_data.call_count == previous_calls + 1
    assert len(worker.requests) == 5  # Initialization, view, zoom, reset and genuine resize.
    assert not any(frame.scrollbar for frame in frames)
    audio_engine_mock.seek_sample.assert_not_called()
    audio_engine_mock.prepare_resident_control.assert_not_called()
