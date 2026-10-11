"""Actual ImGui buttons, draw colors, mouse actions and visible feedback without a backend."""

from contextlib import contextmanager
from typing import TYPE_CHECKING

import pytest
from imgui_bundle import imgui

from flitzis_looper.ui.constants import CONTROL_RGBA, MODE_OFF_RGBA, MODE_ON_RGBA
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import sidebar_left, sidebar_right
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding
from tests.flitzis_looper.controller.transport.test_key_lock_transactions import _feedback
from tests.flitzis_looper.controller.transport.test_residency import Ticket, resident

if TYPE_CHECKING:
    from collections.abc import Iterator
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


@contextmanager
def _imgui_context() -> Iterator[None]:
    previous = imgui.get_current_context()
    owned = imgui.create_context()
    imgui.set_current_context(owned)
    io = imgui.get_io()
    io.set_ini_filename(None)
    io.display_size = imgui.ImVec2(600, 450)
    io.delta_time = 1 / 60
    io.backend_flags |= imgui.BackendFlags_.renderer_has_textures
    io.add_mouse_pos_event(500, 400)
    try:
        yield
    finally:
        imgui.destroy_context(owned)
        if previous is not None:
            imgui.set_current_context(previous)


def _frame(ctx: UiContext, scope: str, *, pad: int = 0) -> tuple[int, ...]:
    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size((300, 400))
    imgui.begin("KEYLOCK render regression", flags=imgui.WindowFlags_.no_saved_settings)
    if scope == "global":
        sidebar_right._render_global_key_lock(ctx)
    else:
        sidebar_left._render_pad_key_lock(ctx, pad)
    colors = tuple(
        vertex.col for vertex in imgui.internal.get_current_window().draw_list.vtx_buffer
    )
    imgui.end()
    imgui.render()
    assert imgui.get_draw_data().total_vtx_count > 0
    return colors


@pytest.mark.parametrize("scope", ["global", "pad"])
@pytest.mark.parametrize("effective", [False, True, None])
def test_actual_buttons_use_confirmed_colors_and_draw_pending_description(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    scope: str,
    *,
    effective: bool | None,
) -> None:
    resident(controller, audio_engine_mock)
    feedback = _feedback(effective=bool(effective))
    audio_engine_mock.pad_key_lock_status.return_value = feedback if effective is not None else None
    # Requested and confirmed deliberately differ in both directions.
    controller.project.pad_key_lock[0] = bool(effective)
    controller.transport.pad.set_pad_key_lock(0, enabled=not bool(effective))
    controller.project.key_lock = not bool(effective)
    native_text = imgui.text_colored
    descriptions: list[str] = []

    def text(color: imgui.ImVec4Like, value: str) -> None:
        descriptions.append(value)
        native_text(color, value)

    monkeypatch.setattr(imgui, "text_colored", text)
    with _imgui_context():
        _frame(UiContext(controller), scope)
        descriptions.clear()
        colors = _frame(UiContext(controller), scope)
        expected = (
            CONTROL_RGBA if effective is None else MODE_ON_RGBA if effective else MODE_OFF_RGBA
        )
        assert imgui.color_convert_float4_to_u32(expected) in colors
        assert f"Preparing {'OFF' if effective else 'ON'}…" in descriptions
        assert not controller.transport.residency._pending[0].unconfirmed


@pytest.mark.parametrize("scope", ["global", "pad"])
def test_actual_draw_exposes_error_and_claimed_unconfirmed_state(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    scope: str,
) -> None:
    ticket = resident(controller, audio_engine_mock)
    audio_engine_mock.pad_key_lock_status.return_value = _feedback()
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    ticket.status = "adopting"
    controller.transport.residency._pending[0].deadline = 0.0
    controller.transport.residency.poll()
    colored_text = imgui.text_colored
    wrapped_text = imgui.text_wrapped
    descriptions: list[str] = []

    def colored(color: imgui.ImVec4Like, value: str) -> None:
        descriptions.append(value)
        colored_text(color, value)

    def wrapped(value: str) -> None:
        descriptions.append(value)
        wrapped_text(value)

    monkeypatch.setattr(imgui, "text_colored", colored)
    monkeypatch.setattr(imgui, "text_wrapped", wrapped)
    with _imgui_context():
        _frame(UiContext(controller), scope)
        colors = _frame(UiContext(controller), scope)
        assert imgui.color_convert_float4_to_u32(MODE_OFF_RGBA) in colors
        assert "Change unconfirmed" in descriptions
        assert any("resident native adoption is unconfirmed" in text for text in descriptions)


@pytest.mark.parametrize("scope", ["global", "pad"])
def test_actual_armed_start_draws_native_dry_readiness_without_controller_pending(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    scope: str,
) -> None:
    resident(controller, audio_engine_mock)
    feedback = _feedback(effective=True)
    feedback["state"] = "armed"
    audio_engine_mock.pad_key_lock_status.return_value = feedback
    controller.project.pad_key_lock[0] = True
    controller.project.key_lock = True
    controller.transport.residency.remember(0)
    feedback.update(effective=False, ready=False, state="waiting")
    descriptions: list[str] = []
    native_text = imgui.text_colored

    def text(color: imgui.ImVec4Like, value: str) -> None:
        descriptions.append(value)
        native_text(color, value)

    monkeypatch.setattr(imgui, "text_colored", text)
    with _imgui_context():
        _frame(UiContext(controller), scope)
        colors = _frame(UiContext(controller), scope)
        assert imgui.color_convert_float4_to_u32(MODE_OFF_RGBA) in colors
        assert "Preparing ON…" in descriptions
    assert not controller.transport.residency._pending
    assert controller.project.pad_key_lock[0]


def test_actual_global_button_draws_mixed_pads_in_neutral_color(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    resident(controller, audio_engine_mock)
    controller.project.sample_paths[37] = "samples/other-bank.wav"
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = FakeInputRuntimePadBinding
    audio_engine_mock.pad_key_lock_status.side_effect = lambda pad: _feedback(
        pad, effective=pad == 37
    )
    descriptions: list[str] = []
    native_text = imgui.text_colored

    def text(color: imgui.ImVec4Like, value: str) -> None:
        descriptions.append(value)
        native_text(color, value)

    monkeypatch.setattr(imgui, "text_colored", text)
    with _imgui_context():
        _frame(UiContext(controller), "global")
        colors = _frame(UiContext(controller), "global")
        assert imgui.color_convert_float4_to_u32(CONTROL_RGBA) in colors
        assert "Mixed pads" in descriptions


@pytest.mark.parametrize("scope", ["global", "pad"])
def test_real_mouse_click_toggles_requested_mode_during_pending(
    controller: AppController, audio_engine_mock: Mock, scope: str
) -> None:
    resident(controller, audio_engine_mock)
    audio_engine_mock.pad_key_lock_status.return_value = _feedback()
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    controller.project.key_lock = True
    audio_engine_mock.prepare_resident_control.return_value = Ticket()
    ctx = UiContext(controller)
    with _imgui_context():
        _frame(ctx, scope)
        _frame(ctx, scope)
        io = imgui.get_io()
        pressed, released = True, False
        io.add_mouse_pos_event(100, 35)
        io.add_mouse_button_event(0, pressed)
        _frame(ctx, scope)
        io.add_mouse_button_event(0, released)
        _frame(ctx, scope)
        assert controller.project.pad_key_lock[0] is False
        assert (
            controller.transport.residency._pending[0].ticket
            is audio_engine_mock.prepare_resident_control.return_value
        )
        assert audio_engine_mock.prepare_resident_control.call_count == 2
        assert controller.transport.residency.key_lock_status(0).effective is False
        assert controller.project.key_lock is (scope != "global")


def test_actual_global_button_learn_capture_saves_mapping_without_toggling(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    resident(controller, audio_engine_mock)
    audio_engine_mock.pad_key_lock_status.return_value = _feedback()
    controller.input_mapping.set_enabled(enabled=True)
    controller.input_mapping.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
    })
    ctx = UiContext(controller)
    with _imgui_context():
        _frame(ctx, "global")
        _frame(ctx, "global")
        io = imgui.get_io()
        pressed, released = True, False
        io.add_mouse_pos_event(100, 35)
        io.add_mouse_button_event(0, pressed)
        _frame(ctx, "global")
        io.add_mouse_button_event(0, released)
        _frame(ctx, "global")
    assert not controller.project.key_lock
    assert not controller.project.pad_key_lock[0]
    assert not controller.transport.residency._pending
    audio_engine_mock.prepare_resident_control.assert_not_called()
    audio_engine_mock.set_input_mapping_snapshot.assert_called_with([
        ("midi:note:1:60", "global.key_lock.toggle")
    ])
