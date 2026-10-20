"""Actual common modal gestures retain the original complete Bank request."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.constants import NUM_PADS
from flitzis_looper.controller.performance_confirmation import BankConfirmation
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.ui import test_arrangement_render as pad_frames
from tests.flitzis_looper.ui import test_bank_arrangement_render as bank_frames
from tests.flitzis_looper.ui import test_performance_confirmation_render as modal_frames

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.performance_confirmation import BankConfirmationAction
    from flitzis_looper.ui.context import UiContext

headless_plot = modal_frames.headless_plot
pytestmark = pytest.mark.usefixtures("headless_plot")


def _seed_bank(controller: AppController, bank: int) -> None:
    pad = bank * NUM_PADS
    controller.project.sample_paths[pad] = f"samples/bank-{bank}.wav"
    controller.project.sample_durations[pad] = 2.0
    controller.project.pad_content[pad] = PadContentIdentity(
        instance_id=f"{pad + 1:032x}", material_id="a" * 32
    )


def _request(
    ctx: UiContext, action: BankConfirmationAction, source: int, target: int
) -> BankConfirmation:
    if action == "copy_bank":
        ctx.ui.confirmation.request_bank_copy(source, target)
    else:
        ctx.ui.confirmation.request_bank_clear(target)
    intent = ctx.ui.confirmation.pending
    assert isinstance(intent, BankConfirmation)
    return intent


@pytest.mark.parametrize("occupied", [False, True])
def test_rendered_copy_gesture_reaches_common_capture_and_exact_bank_admission(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    occupied: bool,
) -> None:
    ctx = pad_frames._seed(controller, audio_engine_mock, monkeypatch)
    if occupied:
        _seed_bank(controller, 1)
    ctx.ui.arrangement.toggle()
    begin = Mock(return_value=True)
    monkeypatch.setattr(controller.banks, "begin", begin)
    banks = pad_frames._observe(monkeypatch)
    pad_frames._frame(ctx)
    bank_frames._click(ctx, banks, "Bank 2")
    assert controller.project.selected_bank == 0
    if occupied:
        intent = ctx.ui.confirmation.pending
        assert isinstance(intent, BankConfirmation)
        request = intent.request
        begin.assert_not_called()
        observed = modal_frames._observe(monkeypatch)
        modal_frames._frame(ctx)
        modal_frames._frame(ctx)
        modal_frames._click(ctx, observed, "OVERWRITE")
    else:
        begin.assert_called_once()
        request = begin.call_args.args[0]
    assert ctx.ui.confirmation.pending is None
    begin.assert_called_once_with(request)
    assert begin.call_args.args[0] is request
    assert request.bank.projected().source_bank == 0
    assert request.bank.projected().target_bank == 1
    audio_engine_mock.capture_bank_participants.assert_called_once_with(0, 1)
    assert controller.project.selected_bank == 0


@pytest.mark.parametrize("action", ["copy_bank", "clear_bank"])
@pytest.mark.parametrize("target", [0, 3])
@pytest.mark.parametrize(("width", "scale"), [(600, 1.0), (600, 1.5), (1200, 2.0)])
def test_common_modal_exact_bank_labels_and_cancel_preserve_everything(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    action: BankConfirmationAction,
    target: int,
    width: int,
    scale: float,
) -> None:
    ctx = pad_frames._seed(controller, audio_engine_mock, monkeypatch)
    source = 1 if target == 0 else 0
    _seed_bank(controller, source)
    _seed_bank(controller, target)
    controller.project.selected_bank = source
    begin = Mock(return_value=True)
    monkeypatch.setattr(controller.banks, "begin", begin)
    observed = modal_frames._observe(monkeypatch)
    intent = _request(ctx, action, source, target)
    controller.project.selected_bank = 5
    controller.session.settings_open = True
    project, session = controller.project.model_dump(), controller.session.model_dump()
    io = imgui.get_io()
    io.display_size = imgui.ImVec2(width, 900)
    imgui.get_style().font_scale_main = scale
    modal_frames._frame(ctx)
    modal_frames._frame(ctx)
    modal_frames._frame(ctx)

    verb = "Overwrite" if action == "copy_bank" else "Clear"
    label = "OVERWRITE" if action == "copy_bank" else "CLEAR BANK"
    assert observed.texts[0] == f"{verb} Bank {target + 1}?"
    assert set(observed.buttons) == {label, "CANCEL"}
    if action == "copy_bank":
        assert any(f"with Bank {source + 1}," in text for text in observed.texts)
    assert ctx.ui.confirmation.pending is intent
    assert observed.clip is not None
    clip_left, clip_top, clip_right, clip_bottom = observed.clip
    for name, (left, top, right, bottom) in observed.buttons.items():
        assert right - left >= observed.text_widths[name] + 2 * imgui.get_style().frame_padding.x
        assert bottom - top >= imgui.get_frame_height() - 0.01
        assert clip_left <= left < right <= clip_right
        assert clip_top <= top < bottom <= clip_bottom
        assert 0 <= left < right <= width
        assert 0 <= top < bottom <= 900
    modal_frames._click(ctx, observed, "CANCEL")
    assert ctx.ui.confirmation.pending is None
    assert controller.project.model_dump() == project
    assert controller.session.model_dump() == session
    begin.assert_not_called()
    audio_engine_mock.capture_bank_participants.assert_called_once_with(
        source if action == "copy_bank" else None, target
    )


@pytest.mark.parametrize("action", ["copy_bank", "clear_bank"])
def test_actual_accept_dispatches_the_original_bank_request_once(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    action: BankConfirmationAction,
) -> None:
    ctx = pad_frames._seed(controller, audio_engine_mock, monkeypatch)
    _seed_bank(controller, 3)
    begin = Mock(return_value=True)
    monkeypatch.setattr(controller.banks, "begin", begin)
    intent = _request(ctx, action, 0, 3)
    observed = modal_frames._observe(monkeypatch)
    modal_frames._frame(ctx)
    modal_frames._frame(ctx)
    controller.project.selected_bank = 5
    modal_frames._click(ctx, observed, "OVERWRITE" if action == "copy_bank" else "CLEAR BANK")
    modal_frames._frame(ctx)

    assert ctx.ui.confirmation.pending is None
    begin.assert_called_once_with(intent.request)
    assert begin.call_args.args[0] is intent.request
    assert controller.project.selected_bank == 5
    audio_engine_mock.capture_bank_participants.assert_called_once_with(
        0 if action == "copy_bank" else None, 3
    )


def test_actual_empty_bank_clear_asks_and_escape_dismisses_without_dispatch(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = pad_frames._seed(controller, audio_engine_mock, monkeypatch)
    begin = Mock(return_value=True)
    monkeypatch.setattr(controller.banks, "begin", begin)
    _request(ctx, "clear_bank", 0, 5)
    observed = modal_frames._observe(monkeypatch)
    modal_frames._frame(ctx)
    modal_frames._frame(ctx)
    assert observed.texts[0] == "Clear Bank 6?"
    assert set(observed.buttons) == {"CLEAR BANK", "CANCEL"}
    pressed = True
    imgui.get_io().add_key_event(imgui.Key.escape, pressed)
    modal_frames._frame(ctx)
    assert ctx.ui.confirmation.pending is None
    released = False
    imgui.get_io().add_key_event(imgui.Key.escape, released)
    modal_frames._frame(ctx)
    begin.assert_not_called()


def test_actual_failed_bank_admission_keeps_captured_request_and_disables_retry(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = pad_frames._seed(controller, audio_engine_mock, monkeypatch)
    _seed_bank(controller, 3)
    begin = Mock(side_effect=RuntimeError("bank native admission full"))
    monkeypatch.setattr(controller.banks, "begin", begin)
    intent = _request(ctx, "copy_bank", 0, 3)
    project = controller.project.model_dump()
    observed = modal_frames._observe(monkeypatch)
    modal_frames._frame(ctx)
    modal_frames._frame(ctx)
    modal_frames._click(ctx, observed, "OVERWRITE")
    modal_frames._frame(ctx)
    failed = ctx.ui.confirmation.pending
    assert isinstance(failed, BankConfirmation)
    assert failed.request is intent.request
    assert failed.error == "bank native admission full"
    assert any("Cancel and try again" in text for text in observed.texts)
    modal_frames._click(ctx, observed, "OVERWRITE")
    begin.assert_called_once_with(intent.request)
    assert controller.project.model_dump() == project
    audio_engine_mock.capture_bank_participants.assert_called_once_with(0, 3)
    modal_frames._click(ctx, observed, "CANCEL")
    assert ctx.ui.confirmation.pending is None
