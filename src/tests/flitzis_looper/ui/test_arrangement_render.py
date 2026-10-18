"""Actual ImGui pointer ownership and visible arrangement feedback, without a device."""

from dataclasses import dataclass, field
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.models import PadContentIdentity
from flitzis_looper.ui import run
from flitzis_looper.ui.constants import BANK_RGBA
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import performance_view
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding
from tests.flitzis_looper.ui import test_waveform_render_stability as waveform_frames

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController

headless_plot = waveform_frames.headless_plot
pytestmark = pytest.mark.usefixtures("headless_plot")


@dataclass
class _Observation:
    buttons: dict[str, tuple[float, float, float, float]] = field(default_factory=dict)
    colors: dict[str, tuple[float, float, float, float]] = field(default_factory=dict)
    tooltips: list[str] = field(default_factory=list)


def _seed(controller: AppController, audio: Mock, monkeypatch: pytest.MonkeyPatch) -> UiContext:
    for pad in (0, 1):
        controller.project.sample_paths[pad] = f"samples/pad-{pad}.wav"
        controller.project.sample_durations[pad] = 2.0
        controller.project.pad_content[pad] = PadContentIdentity(
            instance_id=f"{pad + 1:032x}", material_id="a" * 32
        )
    original = FakeInputRuntimePadBinding(0)
    audio.current_input_runtime_pad_binding.side_effect = lambda pad: (
        original if pad == 0 else FakeInputRuntimePadBinding(pad)
    )
    monkeypatch.setattr(controller.pairs, "begin", Mock(return_value=True))
    monkeypatch.setattr(controller.arrangement, "copy_to_empty", Mock(return_value=True))
    monkeypatch.setattr(controller.arrangement, "copy_overwrite", Mock(return_value=True))
    context = UiContext(controller)
    # Only final App polling and mutation admission are scripted; all rendered
    # pointer events, gesture ownership and global release remain actual code.
    monkeypatch.setattr(context.audio.poll, "poll", Mock())
    audio.reset_mock()
    return context


def _observe(monkeypatch: pytest.MonkeyPatch) -> _Observation:
    observation = _Observation()
    original_button = imgui.button
    original_tooltip = imgui.set_tooltip

    def button(label: str, size: imgui.ImVec2Like = (0, 0)) -> bool:
        color = imgui.get_style_color_vec4(imgui.Col_.button)
        clicked = original_button(label, size)
        a, b = imgui.get_item_rect_min(), imgui.get_item_rect_max()
        key = label.rsplit("##", maxsplit=1)[-1]
        observation.buttons[key] = (a.x, a.y, b.x, b.y)
        observation.colors[key] = (color.x, color.y, color.z, color.w)
        return clicked

    def tooltip(text: str) -> None:
        observation.tooltips.append(text)
        original_tooltip(text)

    monkeypatch.setattr(imgui, "button", button)
    monkeypatch.setattr(imgui, "set_tooltip", tooltip)
    return observation


def _frame(context: UiContext, *, width: float = 700, scale: float = 1.0) -> None:
    io = imgui.get_io()
    io.display_size = imgui.ImVec2(width, 650)
    imgui.get_style().font_scale_main = scale
    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size(io.display_size)
    imgui.begin("arrangement pointer proof", flags=imgui.WindowFlags_.no_saved_settings)
    performance_view.performance_view(context)
    imgui.end()
    imgui.render()
    assert imgui.get_draw_data().total_vtx_count > 0
    run._poll_runtime_tick(context)


def _move(observed: _Observation, label: str) -> None:
    left, top, right, bottom = observed.buttons[label]
    imgui.get_io().add_mouse_pos_event((left + right) / 2, (top + bottom) / 2)


def _button_event(button: int, *, down: bool) -> None:
    imgui.get_io().add_mouse_button_event(button, down)


@pytest.mark.parametrize("width", [560, 1000])
@pytest.mark.parametrize("scale", [1.0, 1.5])
def test_actual_toggle_has_warning_color_and_readable_help(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    width: float,
    scale: float,
) -> None:
    context = _seed(controller, audio_engine_mock, monkeypatch)
    observed = _observe(monkeypatch)
    _frame(context, width=width, scale=scale)
    _move(observed, "Re-Arrange")
    _frame(context, width=width, scale=scale)
    _button_event(imgui.MouseButton_.left, down=True)
    _frame(context, width=width, scale=scale)
    _button_event(imgui.MouseButton_.left, down=False)
    _frame(context, width=width, scale=scale)
    _frame(context, width=width, scale=scale)

    assert context.ui.arrangement.enabled
    assert observed.colors["Re-Arrange"] == pytest.approx((
        BANK_RGBA.x,
        BANK_RGBA.y,
        BANK_RGBA.z,
        BANK_RGBA.w,
    ))
    a, _top, b, _bottom = observed.buttons["Re-Arrange"]
    assert b - a >= imgui.calc_text_size("Re-Arrange").x
    assert any("left Move / Swap; right Copy / Overwrite" in text for text in observed.tooltips)
    assert controller.project.selected_bank == 0


@pytest.mark.parametrize("right", [False, True])
@pytest.mark.parametrize("target", [1, 2])
def test_rendered_drag_previews_target_and_dispatches_once_without_trigger_or_stop(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    target: int,
    *,
    right: bool,
) -> None:
    context = _seed(controller, audio_engine_mock, monkeypatch)
    observed = _observe(monkeypatch)
    context.ui.arrangement.toggle()
    _frame(context)
    _move(observed, "pad_btn_0")
    _frame(context)
    button = imgui.MouseButton_.right if right else imgui.MouseButton_.left
    _button_event(button, down=True)
    _frame(context)
    assert context.ui.arrangement.source_id == 0
    if not right:
        assert imgui.is_any_item_active(), "real source button must retain its active ImGui ID"
    original = audio_engine_mock.current_input_runtime_pad_binding(0)
    _move(observed, f"pad_btn_{target}")
    _frame(context)
    operation = (
        ("Copy overwrite" if target == 1 else "Copy")
        if right
        else ("Swap" if target == 1 else "Move")
    )
    assert f"{operation}: PAD 1 → PAD {target + 1}" in observed.tooltips
    _button_event(button, down=False)
    _frame(context)
    _frame(context)

    dispatch = (
        controller.arrangement.copy_overwrite
        if right and target == 1
        else controller.arrangement.copy_to_empty
        if right
        else controller.pairs.begin
    )
    assert isinstance(dispatch, Mock)
    dispatch.assert_called_once()
    assert dispatch.call_args.args == (0, target)
    binding = dispatch.call_args.kwargs["source_binding"]
    assert isinstance(binding, FakeInputRuntimePadBinding)
    assert binding.metadata()["pad_id"] == 0
    assert binding is original
    assert not context.ui.arrangement.owns_pointer
    audio_engine_mock.capture_pad_start.assert_not_called()
    audio_engine_mock.stop_sample.assert_not_called()


@pytest.mark.parametrize("boundary", ["outside", "bank", "escape", "mode_off"])
def test_rendered_cancel_and_bank_hover_release_do_not_dispatch_or_navigate(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    boundary: str,
) -> None:
    context = _seed(controller, audio_engine_mock, monkeypatch)
    observed = _observe(monkeypatch)
    context.ui.arrangement.toggle()
    _frame(context)
    _move(observed, "pad_btn_0")
    _frame(context)
    _button_event(imgui.MouseButton_.left, down=True)
    _frame(context)
    if boundary == "outside":
        imgui.get_io().add_mouse_pos_event(-100, -100)
    else:
        _move(observed, "Bank 2")
    if boundary == "mode_off":
        context.ui.arrangement.toggle()
    if boundary == "escape":
        pressed = True
        imgui.get_io().add_key_event(imgui.Key.escape, pressed)
    _frame(context)
    _button_event(imgui.MouseButton_.left, down=False)
    _frame(context)

    assert controller.project.selected_bank == 0
    assert not context.ui.arrangement.owns_pointer
    assert context.ui.arrangement.source_id is None
    assert isinstance(controller.pairs.begin, Mock)
    controller.pairs.begin.assert_not_called()
    assert isinstance(controller.arrangement.copy_to_empty, Mock)
    controller.arrangement.copy_to_empty.assert_not_called()
    audio_engine_mock.capture_pad_start.assert_not_called()
    audio_engine_mock.stop_sample.assert_not_called()
