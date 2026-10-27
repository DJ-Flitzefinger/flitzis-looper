"""Exact Unicode routing and picker ownership without opening any real dialog."""

from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import file_dialog

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@dataclass
class _MouseIo:
    events: list[tuple[object, bool]] = field(default_factory=list)

    def add_mouse_button_event(self, button: object, down: object) -> None:
        self.events.append((button, bool(down)))


class _Picker:
    def __init__(self, path: str | None = "D:/samples/loop.wav", *, terminal: bool = True) -> None:
        self.path = path
        self.terminal = terminal
        self.cancelled = False
        self.error: Exception | None = None
        self.cleanup_error: str | None = None
        self.cancel_error: OSError | None = None
        self.consumed = 0

    def ready(self) -> bool:
        return self.terminal

    def cancel(self, *, wait: bool = False) -> bool:
        self.cancelled = True
        if self.cancel_error is not None:
            raise self.cancel_error
        return self.terminal

    def result(self) -> str | None:
        self.consumed += 1
        if self.error is not None:
            raise self.error
        return None if self.cancelled else self.path


@pytest.fixture
def io(monkeypatch: pytest.MonkeyPatch) -> _MouseIo:
    result = _MouseIo()
    monkeypatch.setattr(file_dialog, "_state", file_dialog._DialogState())
    monkeypatch.setattr(file_dialog.imgui, "get_io", lambda: result)
    return result


@pytest.mark.parametrize(
    "path",
    [
        "C:/SCRATCHBEATS/Veteran\u2019s Memorial.flac",
        "C:/音楽/東京 🥁.FLAC",
        "C:/samples/e\u0301 spaced track.wav",
    ],
)
def test_unicode_selection_is_forwarded_once_without_filename_conversion(
    monkeypatch: pytest.MonkeyPatch,
    io: _MouseIo,
    path: str,
) -> None:
    picker = _Picker(path)
    factory = Mock(return_value=picker)
    ctx = Mock()
    monkeypatch.setattr(file_dialog, "AudioFilePicker", factory)
    file_dialog.open_file_dialog(0)
    file_dialog.open_file_dialog(0)
    file_dialog.check_file_dialog(ctx, 0)
    file_dialog.check_file_dialog(ctx, 0)
    factory.assert_called_once_with()
    ctx.audio.pads.load_sample_async.assert_called_once_with(0, path)
    ctx.audio.pads.report_load_error.assert_not_called()
    ctx.ui.close_file_dialog.assert_called_once_with()
    assert picker.consumed == 1
    assert io.events == [
        (imgui.MouseButton_.left, False),
        (imgui.MouseButton_.right, False),
        (imgui.MouseButton_.middle, False),
    ]


def test_pending_and_cancel_do_not_admit_a_load(
    monkeypatch: pytest.MonkeyPatch,
    io: _MouseIo,
) -> None:
    picker = _Picker(None, terminal=False)
    monkeypatch.setattr(file_dialog, "AudioFilePicker", Mock(return_value=picker))
    ctx = Mock()
    file_dialog.open_file_dialog(3)
    file_dialog.check_file_dialog(ctx, 3)
    ctx.ui.close_file_dialog.assert_not_called()
    assert not io.events
    picker.terminal = True
    file_dialog.check_file_dialog(ctx, 3)
    ctx.audio.pads.load_sample_async.assert_not_called()
    ctx.audio.pads.report_load_error.assert_not_called()
    ctx.ui.close_file_dialog.assert_called_once()


def test_different_pad_waits_for_cancelled_helper_and_never_adopts_its_late_result(
    monkeypatch: pytest.MonkeyPatch,
    io: _MouseIo,
) -> None:
    old = _Picker("D:/samples/old.wav", terminal=False)
    new = _Picker("D:/samples/new.wav")
    factory = Mock(side_effect=[old, new])
    monkeypatch.setattr(file_dialog, "AudioFilePicker", factory)
    ctx = Mock()
    file_dialog.open_file_dialog(0)
    file_dialog.open_file_dialog(1)
    file_dialog.check_file_dialog(ctx, 1)
    assert old.cancelled
    assert factory.call_count == 1
    old.terminal = True
    file_dialog.open_file_dialog(1)
    file_dialog.check_file_dialog(ctx, 1)
    assert old.consumed == 0
    ctx.audio.pads.load_sample_async.assert_called_once_with(1, "D:/samples/new.wav")


def test_picker_error_closes_once_and_allows_a_new_open(
    monkeypatch: pytest.MonkeyPatch,
    io: _MouseIo,
) -> None:
    broken = _Picker()
    broken.error = UnicodeDecodeError("utf-8", b"\x92", 0, 1, "invalid start byte")
    fixed = _Picker("D:/samples/Veteran\u2019s Memorial.flac")
    monkeypatch.setattr(file_dialog, "AudioFilePicker", Mock(side_effect=[broken, fixed]))
    ctx = Mock()
    file_dialog.open_file_dialog(0)
    file_dialog.check_file_dialog(ctx, 0)
    ctx.audio.pads.report_load_error.assert_called_once()
    ctx.audio.pads.load_sample_async.assert_not_called()
    ctx.ui.close_file_dialog.assert_called_once()
    file_dialog.open_file_dialog(0)
    file_dialog.check_file_dialog(ctx, 0)
    ctx.audio.pads.load_sample_async.assert_called_once_with(0, fixed.path)
    assert len(io.events) == 6


def test_reopening_original_pad_during_cancellation_creates_a_fresh_picker_after_retirement(
    monkeypatch: pytest.MonkeyPatch,
    io: _MouseIo,
) -> None:
    old = _Picker("D:/samples/late.wav", terminal=False)
    fresh = _Picker("D:/samples/fresh.wav")
    factory = Mock(side_effect=[old, fresh])
    monkeypatch.setattr(file_dialog, "AudioFilePicker", factory)
    file_dialog.open_file_dialog(0)
    file_dialog.open_file_dialog(1)
    file_dialog.open_file_dialog(0)
    assert factory.call_count == 1
    old.terminal = True
    file_dialog.open_file_dialog(0)
    ctx = Mock()
    file_dialog.check_file_dialog(ctx, 0)
    assert old.consumed == 0
    ctx.audio.pads.load_sample_async.assert_called_once_with(0, fresh.path)


def test_cancellation_failure_retains_owned_token_and_reports_without_loading(
    monkeypatch: pytest.MonkeyPatch,
    io: _MouseIo,
) -> None:
    old = _Picker(terminal=False)
    old.cancel_error = PermissionError("owned terminate denied")
    fresh = _Picker()
    factory = Mock(side_effect=[old, fresh])
    monkeypatch.setattr(file_dialog, "AudioFilePicker", factory)
    file_dialog.open_file_dialog(0)
    file_dialog.open_file_dialog(1)
    ctx = Mock()
    file_dialog.check_file_dialog(ctx, 1)
    file_dialog.check_file_dialog(ctx, 1)
    assert file_dialog._state.pending is not None
    assert file_dialog._state.pending.picker is old
    assert factory.call_count == 1
    ctx.audio.pads.load_sample_async.assert_not_called()
    ctx.audio.pads.report_load_error.assert_called_once()
    old.cancel_error = None
    old.terminal = True
    file_dialog.open_file_dialog(1)
    file_dialog.check_file_dialog(ctx, 1)
    ctx.audio.pads.load_sample_async.assert_called_once_with(1, fresh.path)


def test_downstream_loader_failure_is_not_mislabelled_as_picker_failure(
    monkeypatch: pytest.MonkeyPatch,
    io: _MouseIo,
) -> None:
    monkeypatch.setattr(file_dialog, "AudioFilePicker", Mock(return_value=_Picker()))
    ctx = Mock()
    ctx.audio.pads.load_sample_async.side_effect = RuntimeError("unexpected loader failure")
    file_dialog.open_file_dialog(0)
    with pytest.raises(RuntimeError, match="unexpected loader failure"):
        file_dialog.check_file_dialog(ctx, 0)
    ctx.audio.pads.report_load_error.assert_not_called()
    ctx.ui.close_file_dialog.assert_called_once()
    assert len(io.events) == 3


def test_creation_error_preserves_assignment_and_uses_existing_load_diagnostics(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    io: _MouseIo,
) -> None:

    controller.project.sample_paths[0] = "samples/previous.wav"
    before = controller.project.model_dump()
    reserve = Mock()
    monkeypatch.setattr(controller.loader._assets, "reserve", reserve)
    monkeypatch.setattr(
        file_dialog, "AudioFilePicker", Mock(side_effect=OSError("picker unavailable"))
    )
    file_dialog.open_file_dialog(0)
    file_dialog.check_file_dialog(UiContext(controller), 0)
    assert controller.project.model_dump() == before
    assert "picker unavailable" in controller.session.sample_load_errors[0]
    reserve.assert_not_called()
    assert len(io.events) == 3


def test_result_poll_cannot_consume_or_clear_a_reentrant_new_dialog(
    monkeypatch: pytest.MonkeyPatch,
    io: _MouseIo,
) -> None:
    old, new = _Picker(), _Picker("D:/samples/new.wav")
    monkeypatch.setattr(file_dialog, "AudioFilePicker", Mock(side_effect=[old, new]))
    file_dialog.open_file_dialog(0)

    def reenter() -> bool:
        file_dialog.cancel_file_dialog()
        file_dialog.open_file_dialog(1)
        return True

    monkeypatch.setattr(old, "ready", reenter)
    ctx = Mock()
    file_dialog.check_file_dialog(cast("UiContext", ctx), 0)
    assert old.consumed == 0
    ctx.ui.close_file_dialog.assert_not_called()
    file_dialog.check_file_dialog(cast("UiContext", ctx), 1)
    ctx.audio.pads.load_sample_async.assert_called_once_with(1, new.path)
