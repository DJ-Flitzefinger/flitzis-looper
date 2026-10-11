"""Productive native action layouts and gestures, without App or audio hardware."""

from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

import imgui_bundle
import pytest
from imgui_bundle import imgui

from flitzis_looper.models import PadContentIdentity
from flitzis_looper.ui.constants import SPACING
from flitzis_looper.ui.contextmanager import default_style
from flitzis_looper.ui.render import render, sidebar_left, sidebar_right
from flitzis_looper.ui.render.performance_confirmation import performance_confirmation
from tests.flitzis_looper.ui import test_waveform_render_stability as render_stability

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.context import UiContext

type _Rect = tuple[float, float, float, float]

headless_plot = render_stability.headless_plot


@dataclass(frozen=True)
class _Item:
    kind: str
    label: str
    rectangle: _Rect
    clip: _Rect
    frame_height: float
    glyphs: tuple[_Rect, ...]

    @property
    def center(self) -> tuple[float, float]:
        x0, y0, x1, y1 = self.rectangle
        return (x0 + x1) / 2, (y0 + y1) / 2


def _rect(rectangle: imgui.internal.ImRect) -> _Rect:
    return rectangle.min.x, rectangle.min.y, rectangle.max.x, rectangle.max.y


def _contained(inner: _Rect, outer: _Rect) -> bool:
    return (
        inner[0] >= outer[0] - 0.01
        and inner[1] >= outer[1] - 0.01
        and inner[2] <= outer[2] + 0.01
        and inner[3] <= outer[3] + 0.01
    )


def _emitted_glyphs(draw_list: imgui.ImDrawList, start: int, label: str) -> tuple[_Rect, ...]:
    """Identify actual font quads referenced by the native draw commands while valid."""
    baked = imgui.get_font_baked()
    uv_rectangles = set()
    for char in label:
        if char.isspace():
            continue
        glyph = baked.find_glyph_no_fallback(ord(char))
        assert glyph is not None
        uv_rectangles.add((glyph.u0, glyph.v0, glyph.u1, glyph.v1))

    references: dict[int, _Rect] = {}
    for command in draw_list.cmd_buffer:
        clip = command.clip_rect
        for offset in range(command.idx_offset, command.idx_offset + command.elem_count):
            vertex = draw_list.idx_buffer[offset] + command.vtx_offset
            if vertex >= start:
                references[vertex] = (clip.x, clip.y, clip.z, clip.w)

    glyphs = []
    for offset in range(start, len(draw_list.vtx_buffer) - 3):
        vertices = [draw_list.vtx_buffer[offset + index] for index in range(4)]
        a, b, c, d = vertices
        uv = (a.uv.x, a.uv.y, c.uv.x, c.uv.y)
        if (
            uv not in uv_rectangles
            or (b.uv.x, b.uv.y) != (c.uv.x, a.uv.y)
            or (d.uv.x, d.uv.y) != (a.uv.x, c.uv.y)
            or any(offset + index not in references for index in range(4))
        ):
            continue
        bounds = (a.pos.x, a.pos.y, c.pos.x, c.pos.y)
        assert all(_contained(bounds, references[offset + index]) for index in range(4))
        glyphs.append(bounds)
    return tuple(glyphs)


@dataclass
class _Observer:
    items: list[_Item] = field(default_factory=list)

    def install(self, monkeypatch: pytest.MonkeyPatch) -> None:
        original_button = imgui.button
        original_text = imgui.text
        original_colored = imgui.text_colored
        original_wrapped = imgui.text_wrapped
        original_disabled = imgui.text_disabled
        original_separator = imgui.separator

        def button(label: str, size: imgui.ImVec2Like | None = None) -> bool:
            draw_list = imgui.get_window_draw_list()
            start = len(draw_list.vtx_buffer)
            result = original_button(label) if size is None else original_button(label, size)
            visible_label = label.split("##", 1)[0]
            self._record(
                "button",
                visible_label,
                _emitted_glyphs(draw_list, start, visible_label)
                if visible_label in {"CLOSE LOOP EDITOR", "Adjust Loop"}
                else (),
            )
            return result

        def text(value: str) -> None:
            original_text(value)
            self._record("text", value)

        def colored(color: imgui.ImVec4Like, value: str) -> None:
            original_colored(color, value)
            self._record("text_colored", value)

        def wrapped(value: str) -> None:
            original_wrapped(value)
            self._record("text_wrapped", value)

        def disabled(value: str) -> None:
            original_disabled(value)
            self._record("text_disabled", value)

        def separator() -> None:
            original_separator()
            self._record("separator", "|")

        monkeypatch.setattr(imgui, "button", button)
        monkeypatch.setattr(imgui, "text", text)
        monkeypatch.setattr(imgui, "text_colored", colored)
        monkeypatch.setattr(imgui, "text_wrapped", wrapped)
        monkeypatch.setattr(imgui, "text_disabled", disabled)
        monkeypatch.setattr(imgui, "separator", separator)

    def _record(self, name: str, label: str, glyphs: tuple[_Rect, ...] = ()) -> None:
        minimum = imgui.get_item_rect_min()
        maximum = imgui.get_item_rect_max()
        self.items.append(
            _Item(
                name,
                label,
                (minimum.x, minimum.y, maximum.x, maximum.y),
                _rect(imgui.internal.get_current_window().clip_rect),
                imgui.get_frame_height(),
                glyphs,
            )
        )

    def one(self, label: str) -> _Item:
        matches = [item for item in self.items if item.label == label]
        assert len(matches) == 1, (label, [item.label for item in self.items])
        return matches[0]


def _fonts(ctx: UiContext, scale: float) -> None:
    """Use the same runtime Roboto and icon assets without a HelloImGui runner."""
    assets = Path(imgui_bundle.__file__).parent / "assets" / "fonts"
    io = imgui.get_io()
    io.font_default = io.fonts.add_font_from_file_ttf(str(assets / "Roboto/Roboto-Regular.ttf"), 16)
    merge = imgui.ImFontConfig()
    merge.merge_mode = True
    io.fonts.add_font_from_file_ttf(str(assets / "Font_Awesome_6_Free-Solid-900.otf"), 16, merge)
    ctx.bold_font = io.fonts.add_font_from_file_ttf(str(assets / "Roboto/Roboto-Bold.ttf"), 16)
    imgui.get_style().font_scale_main = scale
    io.display_framebuffer_scale = imgui.ImVec2(scale, scale)


def _editor_frame(ctx: UiContext, observer: _Observer, width: float) -> None:
    observer.items.clear()
    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size((width + 16, 850))
    imgui.begin("productive editor layout", flags=imgui.WindowFlags_.no_title_bar)
    with default_style():
        imgui.begin_child("center_area", (-1, -1))
        render._center_area(ctx)
        imgui.end_child()
    imgui.end()
    imgui.render()


@pytest.mark.usefixtures("headless_plot")
@pytest.mark.parametrize("scale", [1.0, 1.25, 1.5, 2.0])
@pytest.mark.parametrize("width", [540.0, 680.0, 1100.0], ids=["narrow", "compact", "ordinary"])
def test_full_close_label_follows_grid_offset_and_real_click_uses_shared_close(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    scale: float,
    width: float,
) -> None:
    ctx = render_stability._context(controller, audio_engine_mock)[0]
    _fonts(ctx, scale)
    imgui.get_io().display_size = imgui.ImVec2(1400, 900)
    ctx.ui.open_waveform_editor(0)
    controller.session.active_sample_ids.add(0)
    observer = _Observer()
    observer.install(monkeypatch)
    for _ in range(4):
        _editor_frame(ctx, observer, width)

    close = observer.one("CLOSE LOOP EDITOR")
    offset = observer.one("0 smp")
    grid_label = observer.one("Grid Offset")
    labels = [item.label for item in observer.items if item.kind == "button"]
    assert labels.index("CLOSE LOOP EDITOR") == labels.index("0 smp") + 1
    assert _contained(close.rectangle, close.clip)
    assert len(close.glyphs) == len("CLOSELOOPEDITOR")
    assert all(_contained(glyph, close.rectangle) for glyph in close.glyphs)
    x0, y0, x1, y1 = close.rectangle
    assert x1 - x0 >= max(32.0, 1.5 * close.frame_height)
    assert y1 - y0 >= max(32.0, 1.5 * close.frame_height)
    pair_width = offset.rectangle[2] - grid_label.rectangle[0] + SPACING + x1 - x0
    if pair_width <= close.clip[2] - close.clip[0]:
        assert y0 == offset.rectangle[1]
        assert x0 - offset.rectangle[2] == pytest.approx(SPACING)
    else:
        assert y0 > offset.rectangle[3]
    toolbar = observer.items[: observer.items.index(close) + 1]
    assert all(_contained(item.rectangle, item.clip) for item in toolbar if item.kind == "button")
    before = (
        controller.project.pad_loop_start_s[:],
        controller.project.pad_loop_end_s[:],
        controller.project.pad_grid_offset_samples[:],
        set(controller.session.active_sample_ids),
    )
    before_release = controller.transport.waveform.view_revision
    before_retries = audio_engine_mock.retry_waveform.call_count
    imgui.get_io().add_mouse_pos_event(*close.center)
    _editor_frame(ctx, observer, width)
    imgui.get_io().add_mouse_button_event(0, down=True)
    _editor_frame(ctx, observer, width)
    assert controller.session.waveform_editor_open
    imgui.get_io().add_mouse_button_event(0, down=False)
    _editor_frame(ctx, observer, width)
    assert not controller.session.waveform_editor_open
    assert controller.session.waveform_editor_pad_id is None
    assert controller.transport.waveform.view_revision == before_release + 1
    assert audio_engine_mock.retry_waveform.call_count == before_retries + 1
    assert before == (
        controller.project.pad_loop_start_s[:],
        controller.project.pad_loop_end_s[:],
        controller.project.pad_grid_offset_samples[:],
        set(controller.session.active_sample_ids),
    )
    audio_engine_mock.seek_sample.assert_not_called()


def _sidebar_frame(ctx: UiContext, observer: _Observer, width: float) -> None:
    observer.items.clear()
    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size((width + 170, 2500))
    imgui.begin("productive performer sidebars", flags=imgui.WindowFlags_.no_title_bar)
    with default_style():
        imgui.begin_child("left_sidebar", (width, -1))
        sidebar_left.sidebar_left(ctx)
        imgui.end_child()
        imgui.same_line()
        imgui.begin_child("right_sidebar", (-1, -1))
        sidebar_right.sidebar_right(ctx)
        imgui.end_child()
        performance_confirmation(ctx)
    imgui.end()
    imgui.render()


def _click_sidebar_item(ctx: UiContext, observer: _Observer, width: float, label: str) -> None:
    io = imgui.get_io()
    io.add_mouse_pos_event(*observer.one(label).center)
    _sidebar_frame(ctx, observer, width)
    io.add_mouse_button_event(0, down=True)
    _sidebar_frame(ctx, observer, width)
    io.add_mouse_button_event(0, down=False)
    _sidebar_frame(ctx, observer, width)


@pytest.mark.usefixtures("headless_plot")
@pytest.mark.parametrize("scale", [1.0, 1.25, 1.5, 2.0])
@pytest.mark.parametrize("width", [220.0, 280.0], ids=["ordinary", "wide"])
@pytest.mark.parametrize("status", ["pending", "error"])
def test_native_sidebar_adjust_section_during_analysis_preserves_key_lock_rows(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    scale: float,
    width: float,
    status: str,
) -> None:
    ctx = render_stability._context(controller, audio_engine_mock)[0]
    _fonts(ctx, scale)
    controller.project.pad_key_lock[0] = True
    controller.project.key_lock = True
    audio_engine_mock.pad_key_lock_status.return_value = {
        "source_id": "loaded-0-1",
        "source_generation": 1,
        "effective": False,
        "ready": status == "error",
        "state": "waiting" if status == "pending" else "failed",
        "error": "fixture KEYLOCK failure" if status == "error" else None,
    }
    controller.session.analyzing_sample_ids.add(0)
    observer = _Observer()
    observer.install(monkeypatch)
    imgui.get_io().display_size = imgui.ImVec2(1200, 2600)
    for _ in range(3):
        _sidebar_frame(ctx, observer, width)

    labels = [item.label for item in observer.items]
    pad = labels.index("Pad")
    adjust = labels.index("Adjust Loop")
    bpm = labels.index("BPM")
    assert pad < adjust < bpm
    assert labels[pad:adjust].count("|") == 1
    assert labels[adjust:bpm].count("|") == 1
    assert labels.count("Adjust Loop") == 1
    assert "Analyze audio" not in labels
    assert "Unload Audio" in labels
    pad_mode = ctx.state.pads.key_lock_status(0)
    global_mode = ctx.state.global_.key_lock_status()
    assert pad_mode.effective is False
    assert global_mode.effective is False
    expected_rows = (
        ["Preparing ON…", "Preparing ON…"]
        if status == "pending"
        else ["Key Lock: fixture KEYLOCK failure", "Key Lock: 1 pad(s): fixture KEYLOCK failure"]
    )
    rows = [item for item in observer.items if item.label in expected_rows]
    assert len(rows) == 2
    assert [item.label for item in rows] == expected_rows
    assert all(_contained(item.rectangle, item.clip) for item in rows), [
        (item.label, item.rectangle, item.clip) for item in rows
    ]
    assert pad_mode.pending == global_mode.pending == (status == "pending")
    assert _contained(observer.one("Adjust Loop").rectangle, observer.one("Adjust Loop").clip)
    assert len(observer.one("Adjust Loop").glyphs) == len("AdjustLoop")

    _click_sidebar_item(ctx, observer, width, "Adjust Loop")
    assert controller.session.waveform_editor_open
    assert controller.session.waveform_editor_pad_id == 0
    _click_sidebar_item(ctx, observer, width, "Adjust Loop")
    assert not controller.session.waveform_editor_open
    assert ctx.ui.confirmation.pending is None
    audio_engine_mock.unload_sample.assert_not_called()


@pytest.mark.usefixtures("headless_plot")
@pytest.mark.parametrize("scale", [1.0, 1.25, 1.5, 2.0])
@pytest.mark.parametrize("action", ["unload", "analyze"])
def test_actual_sidebar_click_opens_content_bound_warning_and_cancel_preserves_editor(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    scale: float,
    action: str,
) -> None:
    ctx = render_stability._context(controller, audio_engine_mock)[0]
    _fonts(ctx, scale)
    imgui.get_io().display_size = imgui.ImVec2(1200, 2600)
    controller.project.pad_content[0] = PadContentIdentity(
        instance_id="1" * 32, material_id="a" * 32
    )
    ctx.ui.open_waveform_editor(0)
    controller.session.active_sample_ids.add(0)
    observer = _Observer()
    observer.install(monkeypatch)
    for _ in range(3):
        _sidebar_frame(ctx, observer, 220.0)
    label = "Unload Audio" if action == "unload" else "Analyze audio"
    _click_sidebar_item(ctx, observer, 220.0, label)
    intent = ctx.ui.confirmation.pending
    assert intent is not None
    assert intent.action == action
    assert intent.pad_id == 0
    assert intent.path == "samples/renderer.wav"
    assert intent.content_instance == "1" * 32
    assert intent.source_identity == (7, "a" * 64, 28_800_000, 48_000)
    audio_engine_mock.unload_sample.assert_not_called()
    assert 0 not in controller.session.analyzing_sample_ids
    assert controller.session.waveform_editor_open
    controller.project.selected_pad = 1
    for _ in range(3):
        _sidebar_frame(ctx, observer, 220.0)
    confirm_label = "UNLOAD AUDIO" if action == "unload" else "ANALYZE AUDIO"
    assert _contained(observer.one(confirm_label).rectangle, observer.one(confirm_label).clip)
    _click_sidebar_item(ctx, observer, 220.0, "CANCEL")
    assert ctx.ui.confirmation.pending is None
    assert controller.session.waveform_editor_open
    assert controller.session.waveform_editor_pad_id == 0
    assert controller.session.active_sample_ids == {0}
    assert controller.project.sample_paths[0] == intent.path
    assert 0 not in controller.session.analyzing_sample_ids
    audio_engine_mock.unload_sample.assert_not_called()
