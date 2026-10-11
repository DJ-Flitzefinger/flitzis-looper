"""Actual modal draw geometry and IO gestures with initialized runtime fonts."""

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

import pytest
from imgui_bundle import imgui

from flitzis_looper.input_mapping import KeyboardBinding, LooperAction
from flitzis_looper.ui.render import render
from flitzis_looper.ui.render.performance_confirmation import performance_confirmation
from tests.flitzis_looper.controller.test_performance_confirmation import _seed
from tests.flitzis_looper.ui import test_waveform_render_stability as waveform_frames

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.performance_confirmation import ConfirmationAction
    from flitzis_looper.ui.context import UiContext


headless_plot = waveform_frames.headless_plot
pytestmark = pytest.mark.usefixtures("headless_plot")


@dataclass
class _DrawObservation:
    buttons: dict[str, tuple[float, float, float, float]] = field(default_factory=dict)
    texts: list[str] = field(default_factory=list)
    clip: tuple[float, float, float, float] | None = None
    text_widths: dict[str, float] = field(default_factory=dict)


def _observe(monkeypatch: pytest.MonkeyPatch) -> _DrawObservation:
    observed = _DrawObservation()
    original_button = imgui.button
    original_text = imgui.text_wrapped

    def button(label: str, size: tuple[float, float] = (0, 0)) -> bool:
        clicked = original_button(label, size)
        a, b = imgui.get_item_rect_min(), imgui.get_item_rect_max()
        observed.buttons[label] = (a.x, a.y, b.x, b.y)
        observed.text_widths[label] = imgui.calc_text_size(label).x
        clip = imgui.internal.get_current_window().inner_clip_rect
        observed.clip = (clip.min.x, clip.min.y, clip.max.x, clip.max.y)
        return clicked

    def text(value: str) -> None:
        original_text(value)
        observed.texts.append(value)

    monkeypatch.setattr(imgui, "button", button)
    monkeypatch.setattr(imgui, "text_wrapped", text)
    return observed


def _frame(ctx: UiContext) -> None:
    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size(imgui.get_io().display_size)
    imgui.begin("independent warning surface", flags=imgui.WindowFlags_.no_saved_settings)
    render._poll_keyboard_input(ctx)
    performance_confirmation(ctx)
    imgui.end()
    imgui.render()
    data = imgui.get_draw_data()
    assert data.total_vtx_count > 0
    # Copy commands while draw pointers are valid. Real rendered popup commands
    # must have bounded clips and actual indexed triangles, independent of labels observed above.
    for draw_list in data.cmd_lists:
        for command in draw_list.cmd_buffer:
            assert command.elem_count >= 0
            if command.elem_count:
                assert command.idx_offset + command.elem_count <= len(draw_list.idx_buffer)
                assert command.clip_rect.z >= command.clip_rect.x
                assert command.clip_rect.w >= command.clip_rect.y


def _click(ctx: UiContext, observed: _DrawObservation, label: str) -> None:
    left, top, right, bottom = observed.buttons[label]
    io = imgui.get_io()
    io.add_mouse_pos_event((left + right) / 2, (top + bottom) / 2)
    _frame(ctx)
    pressed = True
    io.add_mouse_button_event(imgui.MouseButton_.left, pressed)
    _frame(ctx)
    released = False
    io.add_mouse_button_event(imgui.MouseButton_.left, released)
    _frame(ctx)


@pytest.mark.parametrize("scale", [1.0, 1.25, 1.5, 2.0])
@pytest.mark.parametrize("width", [600, 1200])
@pytest.mark.parametrize("action", ["unload", "analyze"])
def test_actual_warning_visible_and_cancel_preserves_bound_editor_and_audio(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    scale: float,
    width: int,
    action: ConfirmationAction,
) -> None:
    ctx = _seed(controller, audio_engine_mock)
    ctx.ui.waveform.open(0)
    io = imgui.get_io()
    io.display_size = imgui.ImVec2(width, 900)
    io.display_framebuffer_scale = imgui.ImVec2(scale, scale)
    imgui.get_style().font_scale_main = scale
    observed = _observe(monkeypatch)
    controller.performance_confirmation.request(action, 0)
    intent = ctx.ui.confirmation.pending
    assert intent is not None
    # Current sidebar selection and settings visibility cannot hide or retarget this popup.
    controller.project.selected_pad = 1
    controller.session.settings_open = True
    project = controller.project.model_dump()
    session = controller.session.model_dump()
    _frame(ctx)
    _frame(ctx)
    _frame(ctx)
    assert observed.texts[0] == "Pad 1: shared.wav"
    assert intent.path in observed.texts
    label = "UNLOAD AUDIO" if action == "unload" else "ANALYZE AUDIO"
    assert set(observed.buttons) == {label, "CANCEL"}
    assert observed.clip is not None
    clip_left, clip_top, clip_right, clip_bottom = observed.clip
    for name, (left, top, right, bottom) in observed.buttons.items():
        assert right - left >= observed.text_widths[name] + 2 * imgui.get_style().frame_padding.x
        assert bottom - top >= imgui.get_frame_height() - 0.01
        assert clip_left <= left < right <= clip_right
        assert clip_top <= top < bottom <= clip_bottom
        assert 0 <= left < right <= width
        assert 0 <= top < bottom <= 900
    _click(ctx, observed, "CANCEL")
    assert ctx.ui.confirmation.pending is None
    assert controller.project.model_dump() == project
    assert controller.session.model_dump() == session
    audio_engine_mock.unload_sample.assert_not_called()
    audio_engine_mock.analyze_sample_async.assert_not_called()
    audio_engine_mock.retry_waveform.assert_not_called()


@pytest.mark.parametrize("action", ["unload", "analyze"])
def test_actual_accept_click_executes_captured_target_once(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    action: ConfirmationAction,
) -> None:
    ctx = _seed(controller, audio_engine_mock)
    controller.performance_confirmation.request(action, 0)
    observed = _observe(monkeypatch)
    _frame(ctx)
    _frame(ctx)
    controller.project.selected_pad = 1
    _click(ctx, observed, "UNLOAD AUDIO" if action == "unload" else "ANALYZE AUDIO")
    assert ctx.ui.confirmation.pending is None
    target = (
        audio_engine_mock.unload_sample
        if action == "unload"
        else audio_engine_mock.analyze_sample_async
    )
    target.assert_called_once_with(0)
    assert controller.project.sample_paths[1] == "samples/other.wav"
    _frame(ctx)
    target.assert_called_once_with(0)


def test_actual_escape_dismisses_warning_and_owns_mapped_escape(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    ctx = _seed(controller, audio_engine_mock)
    controller.input_mapping.set_enabled(enabled=True)
    controller.input_mapping.save_mapping(
        "keyboard", KeyboardBinding(key_name="ESCAPE").key, LooperAction.stop_pad(0)
    )
    controller.performance_confirmation.request("unload", 0)
    _frame(ctx)
    _frame(ctx)
    audio_engine_mock.reset_mock()
    pressed = True
    imgui.get_io().add_key_event(imgui.Key.escape, pressed)
    _frame(ctx)
    assert ctx.ui.confirmation.pending is None
    audio_engine_mock.unload_sample.assert_not_called()
    audio_engine_mock.stop_sample.assert_not_called()
    released = False
    imgui.get_io().add_key_event(imgui.Key.escape, released)
    _frame(ctx)


def test_actual_failed_admission_shows_error_without_closing_editor_or_retrying(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = _seed(controller, audio_engine_mock)
    ctx.ui.waveform.open(0)
    controller.performance_confirmation.request("unload", 0)
    audio_engine_mock.unload_sample.side_effect = RuntimeError("native admission full")
    observed = _observe(monkeypatch)
    _frame(ctx)
    _frame(ctx)
    _click(ctx, observed, "UNLOAD AUDIO")
    _frame(ctx)
    assert "Action failed: native admission full" in observed.texts
    assert controller.session.waveform_editor_open
    assert controller.session.waveform_editor_pad_id == 0
    assert controller.project.sample_paths[0] == "samples/shared.wav"
    _click(ctx, observed, "UNLOAD AUDIO")
    audio_engine_mock.unload_sample.assert_called_once_with(0)
    audio_engine_mock.retry_waveform.assert_not_called()
    _click(ctx, observed, "CANCEL")
    assert ctx.ui.confirmation.pending is None
