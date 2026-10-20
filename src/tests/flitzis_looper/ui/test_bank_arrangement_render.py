"""Rendered bank gestures share normal selection and the existing pad pointer fence."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.input_mapping import KeyboardBinding, LooperAction
from flitzis_looper.ui.constants import BANK_PRESSED_RGBA
from flitzis_looper.ui.render import performance_view
from tests.flitzis_looper.ui import test_arrangement_render as pad_frames

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.context import UiContext

headless_plot = pad_frames.headless_plot
pytestmark = pytest.mark.usefixtures("headless_plot")


def _bank_dispatches(ctx: UiContext, monkeypatch: pytest.MonkeyPatch) -> tuple[Mock, Mock]:
    copy, clear = Mock(), Mock()
    monkeypatch.setattr(ctx.ui.confirmation, "request_bank_copy", copy)
    monkeypatch.setattr(ctx.ui.confirmation, "request_bank_clear", clear)
    return copy, clear


def _click(ctx: UiContext, observed: pad_frames._Observation, label: str) -> None:
    pad_frames._move(observed, label)
    pad_frames._frame(ctx)
    pad_frames._button_event(imgui.MouseButton_.left, down=True)
    pad_frames._frame(ctx)
    pad_frames._button_event(imgui.MouseButton_.left, down=False)
    pad_frames._frame(ctx)


@pytest.mark.parametrize("learn", [False, True])
def test_mode_off_bank_click_keeps_normal_selection_and_learn_path(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    learn: bool,
) -> None:
    ctx = pad_frames._seed(controller, audio_engine_mock, monkeypatch)
    copy, clear = _bank_dispatches(ctx, monkeypatch)
    observed = pad_frames._observe(monkeypatch)
    save = Mock(wraps=controller.input_mapping.save_mapping)
    monkeypatch.setattr(controller.input_mapping, "save_mapping", save)
    if learn:
        controller.input_mapping.set_enabled(enabled=True)
        ctx.input.toggle_learn()
        controller.input_mapping.capture_keyboard_input(
            KeyboardBinding(key_name="A"), text_input_focused=False
        )
    pad_frames._frame(ctx)
    _click(ctx, observed, "Bank 3")

    assert controller.project.selected_bank == (0 if learn else 2)
    if learn:
        assert not controller.session.input_learn_active
        save.assert_called_once_with(
            "keyboard", KeyboardBinding(key_name="A").key, LooperAction.select_bank(2)
        )
    else:
        save.assert_not_called()
    copy.assert_not_called()
    clear.assert_not_called()


@pytest.mark.parametrize(("source", "target"), [(0, 1), (3, 5)])
def test_mode_on_other_bank_copy_retains_source_and_ignores_self_copy(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    source: int,
    target: int,
) -> None:
    ctx = pad_frames._seed(controller, audio_engine_mock, monkeypatch)
    controller.project.selected_bank = source
    ctx.ui.arrangement.toggle()
    copy, clear = _bank_dispatches(ctx, monkeypatch)
    observed = pad_frames._observe(monkeypatch)
    pad_frames._frame(ctx)
    _click(ctx, observed, f"Bank {target + 1}")
    _click(ctx, observed, f"Bank {source + 1}")

    copy.assert_called_once_with(source, target)
    clear.assert_not_called()
    assert controller.project.selected_bank == source
    assert observed.colors[f"Bank {source + 1}"] == pytest.approx((
        BANK_PRESSED_RGBA.x,
        BANK_PRESSED_RGBA.y,
        BANK_PRESSED_RGBA.z,
        BANK_PRESSED_RGBA.w,
    ))
    assert any(f"copy Bank {source + 1} to Bank {target + 1}" in t for t in observed.tooltips)


@pytest.mark.parametrize("target", [0, 1, 5])
def test_mode_on_right_click_current_occupied_or_empty_bank_is_one_fresh_edge(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    target: int,
) -> None:
    ctx = pad_frames._seed(controller, audio_engine_mock, monkeypatch)
    ctx.ui.arrangement.toggle()
    copy, clear = _bank_dispatches(ctx, monkeypatch)
    observed = pad_frames._observe(monkeypatch)
    pad_frames._frame(ctx)
    pad_frames._move(observed, f"Bank {target + 1}")
    pad_frames._frame(ctx)
    pad_frames._button_event(imgui.MouseButton_.right, down=True)
    pad_frames._frame(ctx)
    pad_frames._frame(ctx)
    clear.assert_called_once_with(target)

    # Holding right while crossing another bank is neither another clear nor navigation.
    pad_frames._move(observed, f"Bank {(target + 1) % 6 + 1}")
    pad_frames._frame(ctx)
    pad_frames._button_event(imgui.MouseButton_.right, down=False)
    pad_frames._frame(ctx)
    clear.assert_called_once_with(target)
    pad_frames._button_event(imgui.MouseButton_.right, down=True)
    pad_frames._frame(ctx)
    assert clear.call_count == 2
    assert clear.call_args.args == ((target + 1) % 6,)
    pad_frames._button_event(imgui.MouseButton_.right, down=False)
    pad_frames._frame(ctx)
    assert controller.project.selected_bank == 0
    copy.assert_not_called()
    audio_engine_mock.stop_sample.assert_not_called()


@pytest.mark.parametrize("right", [False, True])
@pytest.mark.parametrize("mode_off", [False, True])
def test_pad_drag_hover_release_and_extra_bank_press_are_fenced(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    right: bool,
    mode_off: bool,
) -> None:
    ctx = pad_frames._seed(controller, audio_engine_mock, monkeypatch)
    ctx.ui.arrangement.toggle()
    copy, clear = _bank_dispatches(ctx, monkeypatch)
    select = Mock(wraps=ctx.ui.select_bank)
    monkeypatch.setattr(ctx.ui, "select_bank", select)
    observed = pad_frames._observe(monkeypatch)
    pad_frames._frame(ctx)
    pad_frames._move(observed, "pad_btn_0")
    pad_frames._frame(ctx)
    button = imgui.MouseButton_.right if right else imgui.MouseButton_.left
    other = imgui.MouseButton_.left if right else imgui.MouseButton_.right
    pad_frames._button_event(button, down=True)
    pad_frames._frame(ctx)
    assert ctx.ui.arrangement.owns_pointer
    pad_frames._move(observed, "Bank 2")
    if mode_off:
        ctx.ui.arrangement.toggle()
    pad_frames._frame(ctx)
    pad_frames._button_event(other, down=True)
    pad_frames._frame(ctx)
    pad_frames._button_event(other, down=False)
    pad_frames._button_event(button, down=False)
    pad_frames._frame(ctx)

    assert not ctx.ui.arrangement.owns_pointer
    assert controller.project.selected_bank == 0
    copy.assert_not_called()
    clear.assert_not_called()
    select.assert_not_called()
    audio_engine_mock.capture_pad_start.assert_not_called()
    audio_engine_mock.stop_sample.assert_not_called()


def test_other_banks_are_marked_with_existing_palette_slow_pulse_and_bank_help(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = pad_frames._seed(controller, audio_engine_mock, monkeypatch)
    observed = pad_frames._observe(monkeypatch)
    marker = Mock(wraps=performance_view._bank_copy_target_marker)
    monkeypatch.setattr(performance_view, "_bank_copy_target_marker", marker)
    pad_frames._frame(ctx)
    marker.assert_not_called()
    ctx.ui.arrangement.toggle()
    pad_frames._frame(ctx)
    assert marker.call_count == 5
    pad_frames._move(observed, "Re-Arrange")
    pad_frames._frame(ctx)
    assert any(
        "left-click another bank to copy; right-click any bank to clear" in text
        for text in observed.tooltips
    )


@pytest.mark.parametrize("mode_off", [False, True])
def test_consumed_pad_drag_cannot_arm_later_bank_click_after_staggered_button_release(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    mode_off: bool,
) -> None:
    ctx = pad_frames._seed(controller, audio_engine_mock, monkeypatch)
    ctx.ui.arrangement.toggle()
    copy, clear = _bank_dispatches(ctx, monkeypatch)
    select = Mock(wraps=ctx.ui.select_bank)
    monkeypatch.setattr(ctx.ui, "select_bank", select)
    observed = pad_frames._observe(monkeypatch)
    pad_frames._frame(ctx)
    pad_frames._move(observed, "pad_btn_0")
    pad_frames._frame(ctx)
    pad_frames._button_event(imgui.MouseButton_.right, down=True)
    pad_frames._frame(ctx)
    assert ctx.ui.arrangement.owns_pointer
    pad_frames._move(observed, "Bank 2")
    pad_frames._frame(ctx)
    if mode_off:
        ctx.ui.arrangement.toggle()
    pad_frames._button_event(imgui.MouseButton_.left, down=True)
    pad_frames._frame(ctx)
    pad_frames._button_event(imgui.MouseButton_.right, down=False)
    pad_frames._frame(ctx)
    assert not ctx.ui.arrangement.owns_pointer
    pad_frames._frame(ctx)
    pad_frames._button_event(imgui.MouseButton_.left, down=False)
    pad_frames._frame(ctx)

    copy.assert_not_called()
    clear.assert_not_called()
    select.assert_not_called()
    assert controller.project.selected_bank == 0
