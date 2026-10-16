"""Real pitch widgets and mouse frames; only the Native admission boundary is fake."""

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

import pytest
from imgui_bundle import imgui

from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import sidebar_left
from tests.flitzis_looper.controller.transport.test_pitch_actions import (
    _as_native,
    _CapturedPitch,
    _pitch_fixture,
)
from tests.flitzis_looper.ui.test_key_lock_render import _imgui_context

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController

_BASE = "pitch_base_keyboard"
_EXTRA = "pitch_extra_keyboard"
_SELECTED_RGBA = (0.24, 0.65, 1.0, 1.0)


@dataclass(frozen=True)
class _Rect:
    left: float
    top: float
    right: float
    bottom: float

    @property
    def center(self) -> tuple[float, float]:
        return (self.left + self.right) / 2.0, (self.top + self.bottom) / 2.0

    def contains(self, x: float, y: float, *, inset: float = 0.0) -> bool:
        return (
            self.left + inset <= x <= self.right - inset
            and self.top + inset <= y <= self.bottom - inset
        )


def _item_rect() -> _Rect:
    minimum = imgui.get_item_rect_min()
    maximum = imgui.get_item_rect_max()
    return _Rect(minimum.x, minimum.y, maximum.x, maximum.y)


@dataclass(frozen=True)
class _Surface:
    keys: dict[int, _Rect]
    bounds: _Rect
    scroll_x: float
    maximum_scroll_x: float
    vertices: tuple[tuple[float, float, int], ...]

    def colors(self, value: int, *, inset: float = 0.0) -> set[int]:
        rectangle = self.keys[value]
        return {color for x, y, color in self.vertices if rectangle.contains(x, y, inset=inset)}

    def assert_selected_visible(self, value: int) -> None:
        selected = imgui.color_convert_float4_to_u32(_SELECTED_RGBA)
        # The thick border can extend into a neighboring cell. Its fringe is not
        # that neighbor's selection strip, which is drawn three pixels inside.
        assert {key for key in self.keys if selected in self.colors(key, inset=2.0)} == {value}
        assert self.bounds.contains(*self.keys[value].center)


@dataclass
class _Frame:
    buttons: dict[str, _Rect] = field(default_factory=dict)
    labels: dict[str, str] = field(default_factory=dict)
    text: list[str] = field(default_factory=list)
    surfaces: dict[str, _Surface] = field(default_factory=dict)


class _PitchRender:
    """Observe delegated native calls without replacing widget decisions or drawing."""

    def __init__(self, controller: AppController, monkeypatch: pytest.MonkeyPatch) -> None:
        self.controller = controller
        self.ctx = UiContext(controller)
        self.observed = _Frame()
        self._menu: str | None = None
        self._keys: dict[int, _Rect] = {}
        self._native_button = imgui.button
        self._native_invisible_button = imgui.invisible_button
        self._native_begin_child = imgui.begin_child
        self._native_end_child = imgui.end_child
        self._native_text_wrapped = imgui.text_wrapped
        self._native_text_colored = imgui.text_colored
        monkeypatch.setattr(imgui, "button", self._button)
        monkeypatch.setattr(imgui, "invisible_button", self._invisible_button)
        monkeypatch.setattr(imgui, "begin_child", self._begin_child)
        monkeypatch.setattr(imgui, "end_child", self._end_child)
        monkeypatch.setattr(imgui, "text_wrapped", self._text_wrapped)
        monkeypatch.setattr(imgui, "text_colored", self._text_colored)

    def _button(self, label: str, size: imgui.ImVec2Like = (0, 0)) -> bool:
        result = self._native_button(label, size)
        identifier = label.partition("##")[2] or label
        self.observed.buttons[identifier] = _item_rect()
        self.observed.labels[identifier] = label.partition("##")[0]
        return result

    def _invisible_button(self, label: str, size: imgui.ImVec2Like) -> bool:
        result = self._native_invisible_button(label, size)
        if self._menu is not None:
            self._keys[int(label.removeprefix("##pitch_"))] = _item_rect()
        return result

    def _begin_child(self, label: str, size: imgui.ImVec2Like, *, window_flags: int) -> bool:
        visible = self._native_begin_child(label, size, window_flags=window_flags)
        assert label in {_BASE, _EXTRA}
        self._menu = label
        self._keys = {}
        return visible

    def _end_child(self) -> None:
        assert self._menu is not None
        position = imgui.get_window_pos()
        size = imgui.get_window_size()
        draw_list = imgui.get_window_draw_list()
        self.observed.surfaces[self._menu] = _Surface(
            self._keys,
            _Rect(position.x, position.y, position.x + size.x, position.y + size.y),
            imgui.get_scroll_x(),
            imgui.get_scroll_max_x(),
            tuple((vertex.pos.x, vertex.pos.y, vertex.col) for vertex in draw_list.vtx_buffer),
        )
        self._menu = None
        self._native_end_child()

    def _text_wrapped(self, value: str) -> None:
        self.observed.text.append(value)
        self._native_text_wrapped(value)

    def _text_colored(self, color: imgui.ImVec4Like, value: str) -> None:
        self.observed.text.append(value)
        self._native_text_colored(color, value)

    def frame(self) -> _Frame:
        self.observed = _Frame()
        imgui.get_io().display_size = imgui.ImVec2(600, 1050)
        imgui.new_frame()
        imgui.set_next_window_pos((0, 0))
        imgui.set_next_window_size((300, 1000))
        imgui.begin("Pitch render regression", flags=imgui.WindowFlags_.no_saved_settings)
        info = sidebar_left._SidebarPadInfo(
            self.controller.project.selected_pad,
            imgui.get_content_region_avail().x,
            is_loaded=True,
            is_loading=False,
        )
        sidebar_left._render_key(self.ctx, info)
        imgui.end()
        imgui.render()
        assert imgui.get_draw_data().total_vtx_count > 0
        return self.observed

    def click(self, rectangle: _Rect) -> _Frame:
        io = imgui.get_io()
        io.add_mouse_pos_event(*rectangle.center)
        pressed, released = True, False
        io.add_mouse_button_event(0, pressed)
        self.frame()
        io.add_mouse_button_event(0, released)
        return self.frame()

    def open_both(self) -> _Frame:
        self.frame()
        current = self.frame()
        current = self.click(current.buttons["pitch_base_header"])
        self.click(current.buttons["pitch_extra_header"])
        # Scroll requests are applied by ImGui on the next child-window frame.
        self.frame()
        return self.frame()


@pytest.fixture
def pitch_render(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> _PitchRender:
    _pitch_fixture(controller, audio_engine_mock)
    controller.project.pad_key_intent[0] = PadKeyIntent(source=SourceKeyVersion(raw_key="C"))
    controller.project.pad_key_intent[1] = PadKeyIntent(
        source=SourceKeyVersion(raw_key="Dm"), base_shift=-2, extra_shift=-12
    )
    audio_engine_mock.capture_input_timestamp_ns.return_value = 123
    return _PitchRender(controller, monkeypatch)


@pytest.mark.parametrize("menu", [_BASE, _EXTRA], ids=["base", "extra"])
@pytest.mark.parametrize("value", [0, 1], ids=["light", "dark"])
def test_actual_repeated_pitch_key_clicks_keep_both_menus_and_contrasting_highlights(
    pitch_render: _PitchRender, audio_engine_mock: Mock, menu: str, value: int
) -> None:
    controller = pitch_render.controller
    controller.transport.pad.set_pitch_retrigger_intent(0, enabled=True)
    captures = [
        _CapturedPitch(
            sequence,
            base_shift=value if menu == _BASE else 0,
            extra_shift=value if menu == _EXTRA else 0,
            retrigger=True,
        )
        for sequence in (1, 2)
    ]
    audio_engine_mock.capture_pad_pitch_action.side_effect = [_as_native(item) for item in captures]
    with _imgui_context():
        frame = pitch_render.open_both()
        displayed_key = frame.surfaces[menu].keys[value]
        initial_scroll = frame.surfaces[menu].scroll_x
        for _ in captures:
            frame = pitch_render.click(frame.surfaces[menu].keys[value])
            assert frame.surfaces[menu].keys[value] == displayed_key
            assert frame.surfaces[menu].scroll_x == initial_scroll
        assert pitch_render.ctx.state.pitch_menus.base_open
        assert pitch_render.ctx.state.pitch_menus.extra_open
        surface = frame.surfaces[menu]
        surface.assert_selected_visible(value)
        fill = (0.12, 0.12, 0.14, 1.0) if value else (0.86, 0.86, 0.88, 1.0)
        assert imgui.color_convert_float4_to_u32(fill) in surface.colors(value)
    action = f"pitch.base:{value}:major" if menu == _BASE else f"pitch.extra:{value}"
    assert [call.args for call in audio_engine_mock.capture_pad_pitch_action.call_args_list] == [
        (0, f"{action}:set_retrigger"),
        (0, f"{action}:set_retrigger"),
    ]
    assert all(
        call.kwargs == {"received_at_ns": 123}
        for call in audio_engine_mock.capture_pad_pitch_action.call_args_list
    )
    assert [
        call.args[0] for call in audio_engine_mock.publish_pitch_action.call_args_list
    ] == captures
    assert controller.project.pad_key_intent[0].base_shift == captures[-1].base_shift
    assert controller.project.pad_key_intent[0].extra_shift == captures[-1].extra_shift


@pytest.mark.parametrize("value", [-18, 18])
def test_actual_scroll_reveals_extreme_and_changed_chosen_extra(
    pitch_render: _PitchRender, value: int
) -> None:
    with _imgui_context():
        pitch_render.open_both()
        pitch_render.controller.transport.pad.set_extra_shift_intent(0, value)
        pitch_render.frame()
        pitch_render.frame()
        extreme = pitch_render.frame().surfaces[_EXTRA]
        extreme.assert_selected_visible(value)
        assert extreme.maximum_scroll_x > 0
        assert extreme.scroll_x == pytest.approx(0 if value < 0 else extreme.maximum_scroll_x)
        pitch_render.controller.transport.pad.reset_extra_shift_intent(0)
        pitch_render.frame()
        pitch_render.frame()
        centered = pitch_render.frame().surfaces[_EXTRA]
        centered.assert_selected_visible(0)
        assert 0 < centered.scroll_x < centered.maximum_scroll_x
        assert pitch_render.ctx.state.pitch_menus.base_open
        assert pitch_render.ctx.state.pitch_menus.extra_open


def test_closed_midi_choice_release_correction_and_pad_changes_repaint_common_state(
    pitch_render: _PitchRender, audio_engine_mock: Mock
) -> None:
    controller = pitch_render.controller
    controller.input_mapping.set_enabled(enabled=True)
    original = _CapturedPitch(1, base_shift=2, extra_shift=1, received_at_ns=777)
    event: dict[str, object] = {
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pitch.base:2:major:set",
        "value": 100,
        "captured_pitch": _as_native(original),
        "received_at_ns": 777,
        "direct": True,
        "dispatched": False,
    }
    with _imgui_context():
        pitch_render.frame()
        controller.input_mapping._handle_rust_input_event(event)
        controller.input_mapping._handle_rust_input_event(event | {"value": 0})
        closed = pitch_render.frame()
        assert not closed.surfaces
        assert closed.labels["pitch_base_header"] == "> Key\nD / +2 st"
        assert closed.labels["pitch_extra_header"] == "> Transposition\n+1 st"
        assert "D# / +1 st from base" in closed.text
        controller.transport.pad.set_manual_key(0, "Dm")
        corrected = pitch_render.frame()
        assert "Dm" in corrected.text
        assert corrected.labels["pitch_base_header"] == "> Key\nEm / +2 st"
        assert "Fm / +1 st from base" in corrected.text
        controller.project.selected_pad = 1
        other_closed = pitch_render.frame()
        assert not other_closed.surfaces
        assert other_closed.labels["pitch_base_header"] == "> Key\nCm / -2 st"
        assert other_closed.labels["pitch_extra_header"] == "> Transposition\n-12 st"
        controller.project.selected_pad = 0
        reopened = pitch_render.open_both()
        reopened.surfaces[_BASE].assert_selected_visible(4)
        reopened.surfaces[_EXTRA].assert_selected_visible(1)
        controller.project.selected_pad = 1
        pitch_render.frame()
        pitch_render.frame()
        other = pitch_render.frame()
        assert other.labels["pitch_base_header"] == "v Key\nCm / -2 st"
        assert other.labels["pitch_extra_header"] == "v Transposition\n-12 st"
        other.surfaces[_BASE].assert_selected_visible(0)
        other.surfaces[_EXTRA].assert_selected_visible(-12)
        controller.project.selected_pad = 0
        pitch_render.frame()
        pitch_render.frame()
        restored = pitch_render.frame()
        restored.surfaces[_BASE].assert_selected_visible(4)
        restored.surfaces[_EXTRA].assert_selected_visible(1)
        assert pitch_render.ctx.state.pitch_menus.base_open
        assert pitch_render.ctx.state.pitch_menus.extra_open
    audio_engine_mock.publish_pitch_action.assert_called_once_with(original)
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    audio_engine_mock.capture_selected_pitch_action.assert_not_called()
    assert (original.received_at_ns, original.base_shift, original.extra_shift) == (777, 2, 1)
