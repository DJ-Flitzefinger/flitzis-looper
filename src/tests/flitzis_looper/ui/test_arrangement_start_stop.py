"""A pad-owned pointer must not become a held START/STOP gesture on hover."""

from typing import TYPE_CHECKING
from unittest.mock import Mock, call

import pytest
from imgui_bundle import imgui

from flitzis_looper.ui import run
from flitzis_looper.ui.render import bottom_bar, performance_view
from tests.flitzis_looper.ui import test_arrangement_render as arrangement_frames

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.context import UiContext

headless_plot = arrangement_frames.headless_plot
pytestmark = pytest.mark.usefixtures("headless_plot")


def _frame(context: UiContext) -> None:
    io = imgui.get_io()
    io.display_size = imgui.ImVec2(800, 850)
    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size(io.display_size)
    imgui.begin("arrangement to START/STOP", flags=imgui.WindowFlags_.no_saved_settings)
    imgui.begin_child("performance", (0, 650))
    performance_view.performance_view(context)
    imgui.end_child()
    bottom_bar._start_stop_button(context)
    imgui.end()
    imgui.render()
    assert imgui.get_draw_data().total_vtx_count > 0
    run._poll_runtime_tick(context)


def _assert_no_arrangement_dispatch(controller: AppController) -> None:
    for dispatch in (
        controller.pairs.begin,
        controller.arrangement.copy_to_empty,
        controller.arrangement.copy_overwrite,
    ):
        assert isinstance(dispatch, Mock)
        dispatch.assert_not_called()


def _begin_copy_drag(context: UiContext, observed: arrangement_frames._Observation) -> None:
    owner = context.ui.arrangement
    owner.toggle()
    _frame(context)
    arrangement_frames._move(observed, "pad_btn_0")
    _frame(context)
    arrangement_frames._button_event(imgui.MouseButton_.right, down=True)
    _frame(context)
    assert owner.source_id == 0
    assert owner.owns_pointer
    preview = owner.preview(2)
    assert preview is not None
    assert preview.operation == "Copy"


def _independent_middle_mute(
    context: UiContext,
    controller: AppController,
    audio: Mock,
    observed: arrangement_frames._Observation,
) -> None:
    """A separate middle press still mutes and releases through the real global tick."""
    controller.project.volume = 0.45
    audio.reset_mock()
    arrangement_frames._move(observed, "global_start_stop")
    _frame(context)
    arrangement_frames._button_event(imgui.MouseButton_.middle, down=True)
    _frame(context)
    assert controller.session.global_stop_momentary_mute_active
    audio.set_volume.assert_called_once_with(0.0)
    imgui.get_io().add_mouse_pos_event(-100, -100)
    _frame(context)
    assert controller.session.global_stop_momentary_mute_active
    arrangement_frames._button_event(imgui.MouseButton_.middle, down=False)
    _frame(context)
    assert not controller.session.global_stop_momentary_mute_active
    assert controller.project.volume == pytest.approx(0.45)
    assert audio.set_volume.call_args_list == [call(0.0), call(0.45)]


@pytest.mark.parametrize("disable_while_held", [False, True], ids=["mode-on", "cancel-mode-off"])
def test_right_pad_drag_cannot_stop_global_playback_when_hovering_start_stop(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    disable_while_held: bool,
) -> None:
    context = arrangement_frames._seed(controller, audio_engine_mock, monkeypatch)
    observed = arrangement_frames._observe(monkeypatch)
    start = Mock()
    stop = Mock()
    monkeypatch.setattr(context.audio.global_, "start_or_restart_start_stop", start)
    monkeypatch.setattr(context.audio.global_, "stop_start_stop", stop)
    owner = context.ui.arrangement
    _begin_copy_drag(context, observed)
    if disable_while_held:
        owner.toggle()
        assert owner.source_id is None
        assert not owner.enabled
        assert owner.owns_pointer

    arrangement_frames._move(observed, "global_start_stop")
    _frame(context)
    _frame(context)
    assert owner.owns_pointer
    start.assert_not_called()
    stop.assert_not_called()
    _assert_no_arrangement_dispatch(controller)
    audio_engine_mock.capture_pad_start.assert_not_called()
    audio_engine_mock.stop_sample.assert_not_called()

    imgui.get_io().add_mouse_pos_event(-100, -100)
    arrangement_frames._button_event(imgui.MouseButton_.right, down=False)
    _frame(context)
    assert owner.source_id is None
    assert not owner.owns_pointer
    assert owner.enabled is not disable_while_held
    start.assert_not_called()
    stop.assert_not_called()
    _assert_no_arrangement_dispatch(controller)

    # A fresh independent down edge retains normal START/STOP behavior even
    # when Re-Arrange remains enabled after the cancelled pad drag.
    arrangement_frames._move(observed, "global_start_stop")
    _frame(context)
    arrangement_frames._button_event(imgui.MouseButton_.right, down=True)
    _frame(context)
    stop.assert_called_once_with()
    start.assert_not_called()
    assert not owner.owns_pointer
    imgui.get_io().add_mouse_pos_event(-100, -100)
    arrangement_frames._button_event(imgui.MouseButton_.right, down=False)
    _frame(context)
    _independent_middle_mute(context, controller, audio_engine_mock, observed)
    stop.assert_called_once_with()
    start.assert_not_called()
    _assert_no_arrangement_dispatch(controller)
