from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast

from flitzis_looper.ui.render import file_dialog

if TYPE_CHECKING:
    import pytest

    from flitzis_looper.ui.context import UiContext


@dataclass
class _MouseIo:
    events: list[tuple[object, bool]] = field(default_factory=list)

    def add_mouse_button_event(self, button: object, down: object) -> None:
        self.events.append((button, bool(down)))


def test_release_mouse_buttons_after_native_dialog(monkeypatch: pytest.MonkeyPatch) -> None:
    io = _MouseIo()
    monkeypatch.setattr(file_dialog.imgui, "get_io", lambda: io)

    file_dialog._release_mouse_buttons_after_native_dialog()

    assert io.events == [
        (file_dialog.imgui.MouseButton_.left, False),
        (file_dialog.imgui.MouseButton_.right, False),
        (file_dialog.imgui.MouseButton_.middle, False),
    ]


class _Result:
    def __init__(self, path: str) -> None:
        self._path = path

    def path(self) -> str:
        return self._path


class _Dialog:
    def __init__(self) -> None:
        self.checked_keys: list[str] = []
        self.closed = False

    def is_done(self, key: str) -> bool:
        self.checked_keys.append(key)
        return True

    def has_result(self) -> bool:
        return True

    def get_result(self) -> _Result:
        return _Result("D:/samples/loop.wav")

    def close(self) -> None:
        self.closed = True


class _PadActions:
    def __init__(self) -> None:
        self.load_calls: list[tuple[int, str]] = []

    def load_sample_async(self, pad_id: int, path: str) -> None:
        self.load_calls.append((pad_id, path))


class _AudioActions:
    def __init__(self) -> None:
        self.pads = _PadActions()


class _UiActions:
    def __init__(self) -> None:
        self.closed = False

    def close_file_dialog(self) -> None:
        self.closed = True


class _Context:
    def __init__(self) -> None:
        self.audio = _AudioActions()
        self.ui = _UiActions()


def test_check_file_dialog_loads_result_and_releases_mouse_buttons(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    dialog = _Dialog()
    io = _MouseIo()
    ctx = _Context()

    monkeypatch.setattr(file_dialog.ifd.FileDialog, "instance", staticmethod(lambda: dialog))
    monkeypatch.setattr(file_dialog.imgui, "get_io", lambda: io)

    file_dialog.check_file_dialog(cast("UiContext", ctx), 7)

    assert dialog.checked_keys == ["file_dialog_7"]
    assert ctx.audio.pads.load_calls == [(7, "D:/samples/loop.wav")]
    assert dialog.closed is True
    assert ctx.ui.closed is True
    assert io.events == [
        (file_dialog.imgui.MouseButton_.left, False),
        (file_dialog.imgui.MouseButton_.right, False),
        (file_dialog.imgui.MouseButton_.middle, False),
    ]
