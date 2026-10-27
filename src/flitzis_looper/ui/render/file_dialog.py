from dataclasses import dataclass
from subprocess import TimeoutExpired
from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.file_picker import AudioFilePicker

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext


@dataclass
class _PendingDialog:
    pad_id: int
    picker: AudioFilePicker | None
    error: str | None = None


@dataclass
class _DialogState:
    pending: _PendingDialog | None = None
    error: tuple[int, str] | None = None
    reported: tuple[int, str] | None = None


_state = _DialogState()


def open_file_dialog(sample_id: int) -> None:
    pending = _state.pending
    if pending is not None:
        if pending.pad_id == sample_id and (pending.picker is None or not pending.picker.cancelled):
            return
        if not cancel_file_dialog():
            return
    try:
        _state.reported = None
        _state.pending = _PendingDialog(sample_id, AudioFilePicker())
    except (OSError, RuntimeError, UnicodeError) as error:
        _state.pending = _PendingDialog(
            sample_id, None, f"Could not open audio file dialog: {error}"
        )


def cancel_file_dialog(*, wait: bool = False) -> bool:
    """Keep the original token until its exact helper is confirmed terminal."""
    pending = _state.pending
    if pending is None:
        return True
    if pending.picker is not None:
        try:
            if not pending.picker.cancel(wait=wait):
                return False
        except (OSError, TimeoutExpired) as error:
            _state.error = pending.pad_id, f"Could not cancel audio file dialog: {error}"
            return False
        if pending.picker.cleanup_error is not None:
            _state.error = pending.pad_id, pending.picker.cleanup_error
    if _state.pending is pending:
        _state.pending = None
    return True


def file_dialog_error() -> tuple[int, str] | None:
    return _state.error


def _report_error(ctx: UiContext) -> None:
    error = _state.error
    if error is not None and error != _state.reported:
        _state.reported = error
        ctx.audio.pads.report_load_error(error[0], error[1])


def _release_mouse_buttons_after_native_dialog() -> None:
    io = imgui.get_io()
    released = False
    io.add_mouse_button_event(imgui.MouseButton_.left, released)
    io.add_mouse_button_event(imgui.MouseButton_.right, released)
    io.add_mouse_button_event(imgui.MouseButton_.middle, released)


def _deliver_selection(ctx: UiContext, pending: _PendingDialog) -> None:
    sample_id, picker = pending.pad_id, pending.picker
    if pending.error is not None:
        ctx.audio.pads.report_load_error(sample_id, pending.error)
    elif picker is not None:
        try:
            path = picker.result()
        except (OSError, RuntimeError, ValueError, TypeError) as error:
            ctx.audio.pads.report_load_error(
                sample_id, f"Could not read audio file selection: {error}"
            )
            return
        if path is not None:
            ctx.audio.pads.load_sample_async(sample_id, path)


def check_file_dialog(ctx: UiContext, sample_id: int) -> None:
    _report_error(ctx)
    pending = _state.pending
    if pending is None:
        return
    if pending.pad_id != sample_id:
        cancel_file_dialog()
        _report_error(ctx)
        return
    picker = pending.picker
    if picker is not None and not picker.ready():
        return
    if _state.pending is not pending:
        return
    _state.pending = None  # Consume before any control callback can open another dialog.
    try:
        _deliver_selection(ctx, pending)
    finally:
        if picker is not None and picker.cleanup_error is not None:
            _state.error = sample_id, picker.cleanup_error
        _report_error(ctx)
        _release_mouse_buttons_after_native_dialog()
        if _state.pending is None:
            ctx.ui.close_file_dialog()
