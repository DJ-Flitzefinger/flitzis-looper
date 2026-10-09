"""Actual ImGui/ImPlot frames, without an app, graphics backend or audio device."""

from contextlib import contextmanager
from dataclasses import dataclass, field
from typing import TYPE_CHECKING
from unittest.mock import Mock

import numpy as np
import pytest
from imgui_bundle import imgui, implot

from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import waveform_editor

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper.controller import AppController
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
    assert frames[1].vertices != frames[-1].vertices  # Waveform genuinely appears.
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
