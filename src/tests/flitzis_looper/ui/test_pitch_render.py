"""Actual central piano mouse frames; only Native admission is a control-flow fake."""

from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Protocol, cast

import pytest
from imgui_bundle import imgui

from flitzis_looper.input_mapping.actions import parse_pitch_action
from flitzis_looper.input_mapping.controller import InputMappingController
from flitzis_looper.input_mapping.storage import load_midi_mapping_file, save_midi_mapping_file
from flitzis_looper.key_intent import MusicalKey, PadKeyIntent, SourceKeyVersion
from flitzis_looper.ui.constants import SPACING
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import performance_view, sidebar_left
from flitzis_looper.ui.render.pitch_keyboard import piano_geometry
from tests.flitzis_looper.controller.transport.test_pitch_actions import (
    _as_native,
    _CapturedPitch,
    _pitch_fixture,
)
from tests.flitzis_looper.ui.test_key_lock_render import _imgui_context

if TYPE_CHECKING:
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.input_mapping.actions import PitchAction, PitchCcMode
    from flitzis_looper.input_mapping.storage import MidiMappingFile
    from flitzis_looper_audio import CapturedPitchAction

_PIANO = "performance_piano"
_SELECTED_RGBA = (0.24, 0.65, 1.0, 1.0)
_ORIGINAL_RGBA = (0.92, 0.57, 0.19, 1.0)


@dataclass(frozen=True)
class _Rect:
    left: float
    top: float
    right: float
    bottom: float

    @property
    def center(self) -> tuple[float, float]:
        return (self.left + self.right) / 2.0, (self.top + self.bottom) / 2.0

    @property
    def width(self) -> float:
        return self.right - self.left

    @property
    def height(self) -> float:
        return self.bottom - self.top

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
    vertices: tuple[tuple[float, float, int], ...]

    def note_point(self, value: int) -> tuple[float, float]:
        rectangle = self.keys[value]
        # The lower quarter of a white key is clear of all raised black keys.
        return rectangle.center[0], rectangle.bottom - rectangle.height / 4.0

    def marker_vertices(self, rgba: tuple[float, float, float, float]) -> set[tuple[float, float]]:
        marker = imgui.color_convert_float4_to_u32(rgba)
        return {(x, y) for x, y, color in self.vertices if color == marker}

    def assert_markers(self, selected_value: int, *, original_value: int | None = 0) -> None:
        selected = self.marker_vertices(_SELECTED_RGBA)
        assert selected
        assert all(self.keys[selected_value].contains(x, y, inset=-2.0) for x, y in selected)
        original = self.marker_vertices(_ORIGINAL_RGBA)
        if original_value is None:
            assert not original
        else:
            assert original
            rectangle = self.keys[original_value]
            assert all(
                rectangle.contains(x, y) and y > rectangle.bottom - 14.0 for x, y in original
            )


@dataclass
class _Frame:
    controls: dict[str, _Rect] = field(default_factory=dict)
    labels: dict[str, str] = field(default_factory=dict)
    text: list[str] = field(default_factory=list)
    children: dict[str, _Rect] = field(default_factory=dict)
    piano_bounds: _Rect | None = None
    piano: _Surface | None = None
    piano_items: int = 0
    horizontal_scroll: float = 0.0
    central_content_top: float = 0.0
    piano_feedback: list[tuple[str, _Rect, int]] = field(default_factory=list)


class _DrawText(Protocol):
    def __call__(self, *args: object, **kwargs: object) -> None: ...


class _DrawListObserver:
    """Forward every draw operation and retain actual range-feedback text bounds."""

    def __init__(self, native: imgui.ImDrawList, frame: _Frame) -> None:
        self.native = native
        self.frame = frame

    def __getattr__(self, name: str) -> object:
        return cast("object", getattr(self.native, name))

    def add_text(self, *args: object, **kwargs: object) -> None:
        before = len(self.native.vtx_buffer)
        cast("_DrawText", self.native.add_text)(*args, **kwargs)
        if len(args) != 5 or not isinstance(args[4], str) or not args[4].startswith("Selected:"):
            return
        value = args[4]
        position = cast("tuple[float, float]", args[2])
        scale = cast("float", args[1]) / imgui.get_font_size()
        size = imgui.calc_text_size(value)
        self.frame.piano_feedback.append((
            value,
            _Rect(
                position[0],
                position[1],
                position[0] + size.x * scale,
                position[1] + size.y * scale,
            ),
            len(self.native.vtx_buffer) - before,
        ))


def _resolve_capture(
    action: PitchAction, intent: PadKeyIntent, requested: tuple[int, int]
) -> tuple[int, int]:
    """Resolve fixture Native requested state once per captured semantic action."""
    base, extra = requested
    if action.kind in {"original", "reset_base"}:
        return 0, 0 if action.kind == "original" else extra
    if action.kind == "base":
        assert isinstance(action.target, MusicalKey)
        return intent.with_base_key(action.target).base_shift, extra
    assert isinstance(action.target, int)
    if action.kind == "note":
        return action.target, extra
    if action.kind == "note_step":
        return base + action.target, extra
    if action.kind == "extra":
        return base, action.target
    assert action.kind == "extra_step"
    return base, extra + action.target


@dataclass
class _PitchCapture:
    controller: AppController
    requested: dict[int, tuple[int, int]] = field(default_factory=dict)
    captures: list[_CapturedPitch] = field(default_factory=list)

    def __call__(self, slot: int, key: str, *, received_at_ns: int | None) -> CapturedPitchAction:
        action = parse_pitch_action(key)
        assert action is not None
        intent = self.controller.project.pad_key_intent[slot]
        requested = self.requested.get(slot, (intent.base_shift, intent.extra_shift))
        base, extra = _resolve_capture(action, intent, requested)
        self.requested[slot] = base, extra
        capture = _CapturedPitch(
            len(self.captures) + 1,
            content_lifetime=77 if slot == 0 else 88,
            current_control_slot=slot,
            base_shift=base,
            extra_shift=extra,
            received_at_ns=received_at_ns if received_at_ns is not None else 0,
            retrigger=action.retrigger,
            resets_key_correction=action.kind == "original",
        )
        self.captures.append(capture)
        return _as_native(capture)


class _PitchRender:
    """Observe real widget geometry and draw vertices without replacing decisions."""

    def __init__(self, controller: AppController, monkeypatch: pytest.MonkeyPatch) -> None:
        self.controller = controller
        self.ctx = UiContext(controller)
        self.observed = _Frame()
        self.central_width = 1000.0
        self._native_button = imgui.button
        self._native_small_button = imgui.small_button
        self._native_tree_node = imgui.tree_node
        self._native_checkbox = imgui.checkbox
        self._native_invisible_button = imgui.invisible_button
        self._native_begin_child = imgui.begin_child
        self._native_text_wrapped = imgui.text_wrapped
        self._native_text_colored = imgui.text_colored
        self._native_get_window_draw_list = imgui.get_window_draw_list
        monkeypatch.setattr(imgui, "button", self._button)
        monkeypatch.setattr(imgui, "small_button", self._small_button)
        monkeypatch.setattr(imgui, "tree_node", self._tree_node)
        monkeypatch.setattr(imgui, "checkbox", self._checkbox)
        monkeypatch.setattr(imgui, "invisible_button", self._invisible_button)
        monkeypatch.setattr(imgui, "begin_child", self._begin_child)
        monkeypatch.setattr(imgui, "text_wrapped", self._text_wrapped)
        monkeypatch.setattr(imgui, "text_colored", self._text_colored)
        monkeypatch.setattr(imgui, "get_window_draw_list", self._get_window_draw_list)

    def _get_window_draw_list(self) -> imgui.ImDrawList:
        return cast(
            "imgui.ImDrawList",
            _DrawListObserver(self._native_get_window_draw_list(), self.observed),
        )

    def _record_control(self, label: str) -> None:
        identifier = label.partition("##")[2] or label
        self.observed.controls[identifier] = _item_rect()
        self.observed.labels[identifier] = label.partition("##")[0]

    def _button(self, label: str, size: imgui.ImVec2Like = (0, 0)) -> bool:
        result = self._native_button(label, size)
        self._record_control(label)
        return result

    def _small_button(self, label: str) -> bool:
        result = self._native_small_button(label)
        self._record_control(label)
        return result

    def _tree_node(self, label: str) -> bool:
        result = self._native_tree_node(label)
        self._record_control(label)
        return result

    def _checkbox(self, label: str, *values: bool) -> tuple[bool, bool]:
        result = self._native_checkbox(label, *values)
        self._record_control(label)
        return result

    def _invisible_button(self, label: str, size: imgui.ImVec2Like) -> bool:
        result = self._native_invisible_button(label, size)
        assert label == _PIANO
        self.observed.piano_items += 1
        self.observed.piano_bounds = _item_rect()
        return result

    def _begin_child(self, label: str, size: imgui.ImVec2Like, *, window_flags: int = 0) -> bool:
        visible = self._native_begin_child(label, size, window_flags=window_flags)
        position = imgui.get_window_pos()
        actual_size = imgui.get_window_size()
        self.observed.children[label] = _Rect(
            position.x, position.y, position.x + actual_size.x, position.y + actual_size.y
        )
        return visible

    def _text_wrapped(self, value: str) -> None:
        self.observed.text.append(value)
        self._native_text_wrapped(value)

    def _text_colored(self, color: imgui.ImVec4Like, value: str) -> None:
        self.observed.text.append(value)
        self._native_text_colored(color, value)

    def _observe_piano(self) -> None:
        bounds = self.observed.piano_bounds
        if bounds is None:
            return
        intent = self.ctx.state.pads.key_intent(self.controller.project.selected_pad)
        original = intent.source.recognized if intent.source is not None else None
        source = intent.source_key
        geometry = piano_geometry(
            source.root if source is not None else None,
            width=bounds.width,
            height=bounds.height,
            original_root=original.root if original is not None else None,
        )
        self.observed.piano = _Surface(
            {
                item.key.value: _Rect(
                    bounds.left + item.x,
                    bounds.top,
                    bounds.left + item.x + item.width,
                    bounds.top + item.height,
                )
                for item in geometry
            },
            bounds,
            tuple(
                (vertex.pos.x, vertex.pos.y, vertex.col)
                for vertex in imgui.get_window_draw_list().vtx_buffer
            ),
        )

    def frame(self) -> _Frame:
        self.observed = _Frame()
        imgui.get_io().display_size = imgui.ImVec2(1600, 1050)
        imgui.new_frame()
        imgui.set_next_window_pos((0, 0))
        imgui.set_next_window_size((280, 900))
        imgui.begin("Pitch sidebar regression", flags=imgui.WindowFlags_.no_saved_settings)
        info = sidebar_left._SidebarPadInfo(
            self.controller.project.selected_pad,
            imgui.get_content_region_avail().x,
            is_loaded=True,
            is_loading=False,
        )
        sidebar_left._render_key(self.ctx, info)
        assert self.observed.piano_items == 0
        imgui.end()
        imgui.set_next_window_pos((300, 0))
        imgui.set_next_window_size((self.central_width, 900))
        imgui.begin("Pitch central regression", flags=imgui.WindowFlags_.no_saved_settings)
        self.observed.central_content_top = imgui.get_cursor_screen_pos().y
        performance_view.performance_view(self.ctx)
        self._observe_piano()
        self.observed.horizontal_scroll = imgui.get_scroll_max_x()
        imgui.end()
        imgui.render()
        assert imgui.get_draw_data().total_vtx_count > 0
        return self.observed

    def click_at(self, point: tuple[float, float]) -> _Frame:
        io = imgui.get_io()
        io.add_mouse_pos_event(*point)
        pressed, released = True, False
        io.add_mouse_button_event(0, pressed)
        self.frame()
        io.add_mouse_button_event(0, released)
        return self.frame()

    def click(self, rectangle: _Rect) -> _Frame:
        return self.click_at(rectangle.center)

    def open(self) -> _Frame:
        self.frame()
        current = self.frame()
        self.click(current.controls["pitch_piano_toggle"])
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
    audio_engine_mock.capture_pad_pitch_action.side_effect = _PitchCapture(controller)
    return _PitchRender(controller, monkeypatch)


@pytest.mark.parametrize(
    "value", [0, 1, -12, 12], ids=["original", "black", "octave-down", "octave-up"]
)
def test_actual_repeated_note_mouse_attacks_emit_distinct_captures_and_keep_selection(
    pitch_render: _PitchRender, audio_engine_mock: Mock, value: int
) -> None:
    controller = pitch_render.controller
    controller.transport.pad.set_pitch_retrigger_intent(0, enabled=True)
    with _imgui_context():
        frame = pitch_render.open()
        assert frame.piano is not None
        bounds = frame.piano.bounds
        for _ in range(2):
            assert frame.piano is not None
            frame = pitch_render.click_at(frame.piano.note_point(value))
            assert frame.piano is not None
            assert frame.piano.bounds == bounds
            frame.piano.assert_markers(value)
            assert frame.piano_items == 1
            assert frame.horizontal_scroll == 0.0
            assert pitch_render.ctx.state.pitch_menus.base_open
    requests = audio_engine_mock.capture_pad_pitch_action.call_args_list
    assert [call.args for call in requests] == [(0, f"pitch.note:{value}:set_retrigger")] * 2
    assert all(call.kwargs == {"received_at_ns": 123} for call in requests)
    published = [call.args[0] for call in audio_engine_mock.publish_pitch_action.call_args_list]
    assert [capture.accepted_sequence for capture in published] == [1, 2]
    assert all(capture.base_shift == value and capture.retrigger for capture in published)
    assert controller.project.pad_key_intent[0].base_shift == value
    assert controller.project.pad_key_intent[0].extra_shift == 0


def test_actual_mouse_black_overlap_takes_priority_and_lower_white_remains_playable(
    pitch_render: _PitchRender, audio_engine_mock: Mock
) -> None:
    with _imgui_context():
        frame = pitch_render.open()
        assert frame.piano is not None
        black = frame.piano.keys[1]
        x = black.left + black.width * 0.1
        frame = pitch_render.click_at((x, black.center[1]))
        assert pitch_render.controller.project.pad_key_intent[0].base_shift == 1
        assert frame.piano is not None
        frame.piano.assert_markers(1)
        frame = pitch_render.click_at((x, black.bottom + 2.0))
        assert pitch_render.controller.project.pad_key_intent[0].base_shift == 0
        assert frame.piano is not None
        frame.piano.assert_markers(0)
    assert [call.args[1] for call in audio_engine_mock.capture_pad_pitch_action.call_args_list] == [
        "pitch.note:1:set",
        "pitch.note:0:set",
    ]


def test_actual_offset_reset_preserves_note_octave_and_correction_original_clears_all(
    pitch_render: _PitchRender, audio_engine_mock: Mock
) -> None:
    controller = pitch_render.controller
    controller.project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(raw_key="C"),
        correction="Dm",
        base_shift=12,
        extra_shift=2,
        retrigger=True,
        extra_retrigger=False,
    )
    with _imgui_context():
        frame = pitch_render.open()
        assert frame.piano is not None
        frame.piano.assert_markers(12, original_value=-2)
        frame = pitch_render.click(frame.controls["pitch_offset_1"])
        intent = controller.project.pad_key_intent[0]
        assert (intent.base_shift, intent.extra_shift, intent.correction) == (12, 3, "Dm")
        assert frame.piano is not None
        frame.piano.assert_markers(12, original_value=-2)
        frame = pitch_render.click(frame.controls["pitch_offset_0"])
        intent = controller.project.pad_key_intent[0]
        assert (intent.base_shift, intent.extra_shift, intent.correction) == (12, 0, "Dm")
        assert frame.piano is not None
        frame.piano.assert_markers(12, original_value=-2)
        frame = pitch_render.click(frame.controls["pitch_original"])
        intent = controller.project.pad_key_intent[0]
        assert (intent.base_shift, intent.extra_shift, intent.correction) == (0, 0, None)
        assert intent.source == SourceKeyVersion(raw_key="C")
        assert intent.retrigger
        assert not intent.extra_retrigger
        assert frame.piano is not None
        frame.piano.assert_markers(0)
        frame = pitch_render.click(frame.controls["pitch_offset_1"])
        assert frame.piano is not None
        frame.piano.assert_markers(0)
        assert "C# / +1 st from base" in frame.text, (
            controller.project.pad_key_intent[0],
            audio_engine_mock.capture_pad_pitch_action.call_args_list,
        )
    assert [call.args[1] for call in audio_engine_mock.capture_pad_pitch_action.call_args_list] == [
        "pitch.extra.step:1:set",
        "pitch.extra:0:set",
        "pitch.original:set_retrigger",
        "pitch.extra.step:1:set",
    ]
    assert audio_engine_mock.publish_pitch_action.call_count == 4


def test_actual_retrigger_checkboxes_are_independent_and_fix_their_own_action_variant(
    pitch_render: _PitchRender, audio_engine_mock: Mock
) -> None:
    controller = pitch_render.controller
    with _imgui_context():
        frame = pitch_render.open()
        frame = pitch_render.click(frame.controls["pitch_piano_retrigger"])
        intent = controller.project.pad_key_intent[0]
        assert intent.retrigger
        assert not intent.extra_retrigger
        frame = pitch_render.click(frame.controls["pitch_offset_retrigger"])
        intent = controller.project.pad_key_intent[0]
        assert intent.retrigger
        assert intent.extra_retrigger
        frame = pitch_render.click(frame.controls["pitch_piano_retrigger"])
        intent = controller.project.pad_key_intent[0]
        assert not intent.retrigger
        assert intent.extra_retrigger
        audio_engine_mock.capture_pad_pitch_action.assert_not_called()
        assert frame.piano is not None
        frame = pitch_render.click_at(frame.piano.note_point(1))
        pitch_render.click(frame.controls["pitch_offset_1"])
    assert [call.args[1] for call in audio_engine_mock.capture_pad_pitch_action.call_args_list] == [
        "pitch.note:1:set",
        "pitch.extra.step:1:set_retrigger",
    ]
    assert controller.project.pad_key_intent[0].base_shift == 1
    assert controller.project.pad_key_intent[0].extra_shift == 1


@pytest.mark.parametrize("retrigger", [False, True])
def test_actual_fixed_offset_click_uses_its_own_variant_and_original_timestamp(
    pitch_render: _PitchRender, audio_engine_mock: Mock, *, retrigger: bool
) -> None:
    controller = pitch_render.controller
    intent = PadKeyIntent(
        source=SourceKeyVersion(raw_key="C"),
        correction="D",
        base_shift=-5,
        extra_shift=7,
        retrigger=not retrigger,
        extra_retrigger=retrigger,
    )
    controller.project.pad_key_intent[0] = intent
    with _imgui_context():
        frame = pitch_render.open()
        assert "pitch_offset_fixed" not in frame.controls
        frame = pitch_render.click(frame.controls["pitch_steps"])
        assert {"pitch_Piano_-12", "pitch_Offset_12"} <= frame.controls.keys()
        assert frame.labels["pitch_offset_fixed"] == "Fixed offset +7 st"
        frame = pitch_render.click(frame.controls["pitch_offset_fixed"])
        assert frame.piano is not None
        frame.piano.assert_markers(-5, original_value=-2)
        assert frame.piano_items == 1
        assert {"pitch_offset_1", "pitch_offset_0", "pitch_offset_-1"} <= frame.controls.keys()
    variant = "set_retrigger" if retrigger else "set"
    audio_engine_mock.capture_pad_pitch_action.assert_called_once_with(
        0, f"pitch.extra:7:{variant}", received_at_ns=123
    )
    audio_engine_mock.capture_selected_pitch_action.assert_not_called()
    audio_engine_mock.publish_pitch_action.assert_called_once()
    assert controller.project.pad_key_intent[0] == intent


def _replay_restored_fixed_offsets(
    controller: AppController, audio: Mock, restored: MidiMappingFile
) -> None:
    """Check stored semantics through the existing explicit Native admission fake."""
    original = controller.project.pad_key_intent[0]
    controller.project.selected_pad = 1
    captures = _PitchCapture(controller)

    def selected(key: str, *, received_at_ns: int | None) -> CapturedPitchAction:
        return captures(1, key, received_at_ns=received_at_ns)

    audio.capture_selected_pitch_action.side_effect = selected
    for index, entry in enumerate(restored.mappings):
        action = parse_pitch_action(entry.action.key)
        assert action is not None
        assert action.kind == "extra"
        controller.project.pad_key_intent[1] = PadKeyIntent(
            source=SourceKeyVersion(raw_key="Dm"),
            correction="E",
            base_shift=-5,
            extra_shift=1 if action.target != 1 else -1,
            retrigger=action.retrigger,
            extra_retrigger=not action.retrigger,
        )
        assert controller.input_mapping.execute_action(entry.action, received_at_ns=777 + index)
        capture = captures.captures[-1]
        assert (capture.base_shift, capture.extra_shift, capture.retrigger) == (
            -5,
            action.target,
            action.retrigger,
        )
        assert capture.received_at_ns == 777 + index
        intent = controller.project.pad_key_intent[1]
        assert (intent.base_shift, intent.extra_shift, intent.correction) == (
            -5,
            action.target,
            "E",
        )
        assert intent.extra_retrigger is not action.retrigger
        assert controller.project.pad_key_intent[0] == original
        capture.state = "applied"
        capture.effective = True
        capture.effective_output_frame = index + 1
        controller.transport.pitch.poll()
        assert not controller.transport.pitch._owners
    assert audio.capture_selected_pitch_action.call_count == len(restored.mappings)
    assert audio.publish_pitch_action.call_count == len(restored.mappings)
    audio.capture_pad_pitch_action.assert_not_called()


def test_actual_fixed_offset_learn_exposes_every_signed_target_and_restores_both_variants(
    pitch_render: _PitchRender,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    """Render and click real controls; Python MIDI events and Native admission are synthetic."""
    controller = pitch_render.controller
    path = tmp_path / "fixed-offset-midi.json"
    monkeypatch.setattr(
        "flitzis_looper.input_mapping.controller.load_midi_mapping_file",
        lambda: load_midi_mapping_file(path),
    )
    monkeypatch.setattr(
        "flitzis_looper.input_mapping.controller.save_midi_mapping_file",
        lambda data: save_midi_mapping_file(data, path),
    )
    controller.input_mapping = InputMappingController(controller)
    controller.input_mapping.set_enabled(enabled=True)
    expected: dict[str, str] = {}
    modes: tuple[PitchCcMode, ...] = ("button", "absolute", "twos_complement", "offset_binary")
    with _imgui_context():
        frame = pitch_render.open()
        assert "pitch_offset_fixed" not in frame.controls
        frame = pitch_render.click(frame.controls["pitch_steps"])
        for value in range(-18, 19):
            for retrigger in (False, True):
                controller.project.pad_key_intent[0] = PadKeyIntent(
                    source=SourceKeyVersion(raw_key="C"),
                    correction="D",
                    base_shift=-5,
                    extra_shift=value,
                    retrigger=not retrigger,
                    extra_retrigger=retrigger,
                )
                pitch_render.ctx.ui.set_pitch_cc_mode(modes[len(expected) % len(modes)])
                frame = pitch_render.frame()
                signed = "0" if value == 0 else f"{value:+d}"
                assert frame.labels["pitch_offset_fixed"].startswith(f"Fixed offset {signed} st")
                before = tuple(controller.project.pad_key_intent)
                binding = f"midi:cc:1:{20 + len(expected)}"
                controller.input_mapping.toggle_learn()
                controller.input_mapping._handle_rust_input_event({
                    "source": "midi",
                    "binding_key": binding,
                    "value": 127,
                })
                assert controller.session.input_learn_pending_binding_key == binding
                frame = pitch_render.click(frame.controls["pitch_offset_fixed"])
                assert not controller.session.input_learn_active
                assert controller.session.input_learn_pending_binding_key is None
                assert tuple(controller.project.pad_key_intent) == before
                assert not controller.transport.pitch._owners
                assert frame.piano is not None
                frame.piano.assert_markers(-5, original_value=-2)
                variant = "set_retrigger" if retrigger else "set"
                expected[binding] = f"pitch.extra:{value}:{variant}"
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    audio_engine_mock.capture_selected_pitch_action.assert_not_called()
    audio_engine_mock.publish_pitch_action.assert_not_called()
    saved = path.read_bytes()
    learned = controller.input_mapping._midi
    controller.input_mapping = InputMappingController(controller)
    restored = controller.input_mapping._midi
    assert restored is not learned
    assert {entry.input.key: entry.action.key for entry in restored.mappings} == expected
    assert all(entry.action.source == "direct" for entry in restored.mappings)
    pitch_render.ctx.ui.set_pitch_cc_mode("button")
    _replay_restored_fixed_offsets(controller, audio_engine_mock, restored)
    assert controller.project.selected_pad == 1
    assert path.read_bytes() == saved


@pytest.mark.parametrize("central_width", [620.0, 1000.0])
def test_shared_piano_reserves_one_stable_row_and_follows_selected_pad(
    pitch_render: _PitchRender, central_width: float
) -> None:
    pitch_render.central_width = central_width
    with _imgui_context():
        first = pitch_render.open()
        assert first.piano is not None
        first.piano.assert_markers(0)
        bounds = first.piano.bounds
        grid = first.children["pad_grid"]
        row_height = grid.top - first.central_content_top - SPACING
        assert bounds.left >= 300.0
        assert bounds.bottom < grid.top
        assert bounds.width == pytest.approx(grid.width)
        assert first.piano_items == 1
        assert set(first.children) == {"pad_grid", "bank_buttons"}
        assert abs(row_height - first.controls["pad_btn_0"].height) <= SPACING * 2.0
        pitch_render.ctx.ui.select_pad(1)
        other = pitch_render.frame()
        assert other.piano is not None
        assert other.piano.bounds == bounds
        assert other.children["pad_grid"] == grid
        other.piano.assert_markers(-2)
        assert pitch_render.ctx.state.pitch_menus.base_open
        pitch_render.ctx.ui.select_pad(0)
        restored = pitch_render.frame()
        assert restored.piano is not None
        assert restored.piano.bounds == bounds
        restored.piano.assert_markers(0)
        closed = pitch_render.click(restored.controls["pitch_piano_toggle"])
        assert closed.piano is None
        assert closed.piano_items == 0
        assert closed.children["pad_grid"].top == pytest.approx(first.central_content_top)
        assert closed.children["pad_grid"].height - grid.height == pytest.approx(
            row_height + SPACING, abs=1.0
        )
        assert closed.controls["pad_btn_0"].height > first.controls["pad_btn_0"].height


@pytest.mark.parametrize("source", [None, "unrecognized"])
def test_actual_unknown_source_keeps_relative_octave_choices_without_original_claim(
    pitch_render: _PitchRender, source: str | None
) -> None:
    pitch_render.controller.project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(raw_key=source) if source is not None else None
    )
    with _imgui_context():
        frame = pitch_render.open()
        assert frame.piano is not None
        frame.piano.assert_markers(0, original_value=None)
        frame = pitch_render.click_at(frame.piano.note_point(-12))
        assert frame.piano is not None
        frame.piano.assert_markers(-12, original_value=None)
        intent = pitch_render.controller.project.pad_key_intent[0]
        assert intent.base_shift == -12
        assert intent.source_key is None


def test_closed_midi_note_repaints_the_shared_piano_without_recapture(
    pitch_render: _PitchRender, audio_engine_mock: Mock
) -> None:
    controller = pitch_render.controller
    controller.input_mapping.set_enabled(enabled=True)
    original = _CapturedPitch(1, base_shift=2, extra_shift=1, received_at_ns=777)
    event: dict[str, object] = {
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pitch.note:2:set",
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
        assert closed.piano is None
        assert "D# / +1 st from base" in closed.text
        opened = pitch_render.open()
        assert opened.piano is not None
        opened.piano.assert_markers(2)
        assert controller.project.pad_key_intent[0].extra_shift == 1
    audio_engine_mock.publish_pitch_action.assert_called_once_with(original)
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    audio_engine_mock.capture_selected_pitch_action.assert_not_called()


def test_actual_note_refusal_keeps_previous_selection_and_displays_runtime_reason(
    pitch_render: _PitchRender, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.capture_pad_pitch_action.side_effect = ValueError("unsupported_pitch")
    with _imgui_context():
        frame = pitch_render.open()
        assert frame.piano is not None
        frame = pitch_render.click_at(frame.piano.note_point(23))
        assert frame.piano is not None
        frame.piano.assert_markers(0)
        assert pitch_render.controller.project.pad_key_intent[0].base_shift == 0
        assert any("not supported" in text for text in frame.text)
    audio_engine_mock.capture_pad_pitch_action.assert_called_once_with(
        0, "pitch.note:23:set", received_at_ns=123
    )
    audio_engine_mock.publish_pitch_action.assert_not_called()


def test_actual_corrected_reference_plays_labelled_notes_and_retains_detected_original(
    pitch_render: _PitchRender, audio_engine_mock: Mock
) -> None:
    controller = pitch_render.controller
    controller.project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(raw_key="C"), correction="D"
    )
    with _imgui_context():
        frame = pitch_render.open()
        assert frame.piano is not None
        frame.piano.assert_markers(0, original_value=-2)
        assert "D / 0 st from base" in frame.text
        frame = pitch_render.click_at(frame.piano.note_point(-2))
        assert frame.piano is not None
        frame.piano.assert_markers(-2, original_value=-2)
        assert "C / 0 st from base" in frame.text
        assert controller.project.pad_key_intent[0].base_shift == -2
    audio_engine_mock.capture_pad_pitch_action.assert_called_once_with(
        0, "pitch.note:-2:set", received_at_ns=123
    )


def _assert_range_feedback(frame: _Frame, description: str) -> None:
    assert frame.piano is not None
    assert len(frame.piano.keys) == 36
    assert not frame.piano.marker_vertices(_SELECTED_RGBA)
    assert len(frame.piano_feedback) == 1
    text, rectangle, vertex_count = frame.piano_feedback[0]
    assert text == description
    assert vertex_count > 0
    assert frame.piano.bounds.contains(rectangle.left, rectangle.top)
    assert frame.piano.bounds.contains(rectangle.right, rectangle.bottom)
    assert frame.piano_items == 1
    assert frame.horizontal_scroll == 0.0


@pytest.mark.parametrize(
    ("source", "shift", "description"),
    [
        ("C", -23, "Selected: C# | -23 st | -2 octave | below displayed range"),
        ("B", 23, "Selected: A# | +23 st | +2 octave | above displayed range"),
        (None, -23, "Selected: relative note | -23 st | -2 octave | below displayed range"),
        (
            "unrecognized",
            -13,
            "Selected: relative note | -13 st | -2 octave | below displayed range",
        ),
    ],
)
def test_restored_undisplayed_note_renders_exact_relative_octave_without_folded_marker(
    pitch_render: _PitchRender,
    audio_engine_mock: Mock,
    source: str | None,
    shift: int,
    description: str,
) -> None:
    pitch_render.controller.project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(raw_key=source) if source is not None else None,
        base_shift=shift,
    )
    with _imgui_context():
        frame = pitch_render.open()
        _assert_range_feedback(frame, description)
        assert frame.piano is not None
        assert shift not in frame.piano.keys
        if source in {None, "unrecognized"}:
            assert not frame.piano.marker_vertices(_ORIGINAL_RGBA)
    assert pitch_render.controller.project.pad_key_intent[0].base_shift == shift
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    audio_engine_mock.capture_selected_pitch_action.assert_not_called()
    audio_engine_mock.publish_pitch_action.assert_not_called()


@pytest.mark.parametrize("central_width", [620.0, 1000.0])
@pytest.mark.parametrize(
    ("source", "shift", "description"),
    [
        ("C", -13, "Selected: B | -13 st | -2 octave | below displayed range"),
        ("B", 13, "Selected: C | +13 st | +2 octave | above displayed range"),
        (None, -13, "Selected: relative note | -13 st | -2 octave | below displayed range"),
    ],
)
def test_midi_undisplayed_note_preserves_receipt_and_pad_return_then_restores_in_range_marker(
    pitch_render: _PitchRender,
    audio_engine_mock: Mock,
    central_width: float,
    source: str | None,
    shift: int,
    description: str,
) -> None:
    controller = pitch_render.controller
    controller.project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(raw_key=source) if source is not None else None
    )
    controller.input_mapping.set_enabled(enabled=True)
    pitch_render.central_width = central_width
    original = _CapturedPitch(1, base_shift=shift, extra_shift=0, received_at_ns=777)
    event: dict[str, object] = {
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": f"pitch.note:{shift}:set",
        "value": 100,
        "captured_pitch": _as_native(original),
        "received_at_ns": 777,
        "direct": True,
        "dispatched": False,
    }
    with _imgui_context():
        frame = pitch_render.open()
        assert frame.piano is not None
        frame.piano.assert_markers(0, original_value=0 if source is not None else None)
        bounds, grid = frame.piano.bounds, frame.children["pad_grid"]
        controller.input_mapping._handle_rust_input_event(event)
        controller.input_mapping._handle_rust_input_event(event | {"value": 0})
        frame = pitch_render.frame()
        _assert_range_feedback(frame, description)
        assert controller.project.pad_key_intent[0].base_shift == shift
        audio_engine_mock.publish_pitch_action.assert_called_once_with(original)
        assert original.received_at_ns == 777
        pitch_render.ctx.ui.select_pad(1)
        frame = pitch_render.frame()
        assert frame.piano is not None
        frame.piano.assert_markers(-2)
        assert not frame.piano_feedback
        assert frame.piano.bounds == bounds
        assert frame.children["pad_grid"] == grid
        pitch_render.ctx.ui.select_pad(0)
        frame = pitch_render.frame()
        _assert_range_feedback(frame, description)
        assert frame.piano is not None
        assert frame.piano.bounds == bounds
        assert frame.children["pad_grid"] == grid
        assert pitch_render.ctx.state.pitch_menus.base_open
        in_range = _CapturedPitch(2, base_shift=0, extra_shift=0, received_at_ns=888)
        controller.input_mapping._handle_rust_input_event(
            event
            | {
                "action_key": "pitch.note:0:set",
                "captured_pitch": _as_native(in_range),
                "received_at_ns": 888,
            }
        )
        frame = pitch_render.frame()
        assert frame.piano is not None
        frame.piano.assert_markers(0, original_value=0 if source is not None else None)
        assert not frame.piano_feedback
        assert frame.piano.bounds == bounds
        assert frame.children["pad_grid"] == grid
    assert [call.args[0] for call in audio_engine_mock.publish_pitch_action.call_args_list] == [
        original,
        in_range,
    ]
    assert controller.project.pad_key_intent[0].base_shift == 0
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    audio_engine_mock.capture_selected_pitch_action.assert_not_called()
