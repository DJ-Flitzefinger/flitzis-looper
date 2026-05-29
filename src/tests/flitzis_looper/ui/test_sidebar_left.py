from contextlib import contextmanager
from typing import TYPE_CHECKING, cast

import pytest
from imgui_bundle import imgui

from flitzis_looper.ui.render import sidebar_left
from flitzis_looper.ui.render.sidebar_left import (
    filtered_eq_entry_char,
    parse_eq_entry_text,
    sanitize_eq_entry_text,
)

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper.ui.context import UiContext


class _Point:
    __slots__ = ("x", "y")

    def __init__(self, x: float, y: float) -> None:
        self.x = x
        self.y = y


class _Io:
    __slots__ = ("mouse_delta", "mouse_pos")

    def __init__(self, mouse_x: float) -> None:
        self.mouse_pos = _Point(mouse_x, 0.0)
        self.mouse_delta = _Point(0.0, 0.0)


class _GainPads:
    __slots__ = ("calls",)

    def __init__(self) -> None:
        self.calls: list[tuple[int, float]] = []

    def set_pad_gain(self, pad_id: int, gain_db: float) -> None:
        self.calls.append((pad_id, gain_db))


class _GainAudio:
    __slots__ = ("pads",)

    def __init__(self) -> None:
        self.pads = _GainPads()


class _GainProject:
    __slots__ = ("pad_gain_db",)

    def __init__(self) -> None:
        self.pad_gain_db = [0.0]


class _GainState:
    __slots__ = ("project",)

    def __init__(self) -> None:
        self.project = _GainProject()


class _GainContext:
    __slots__ = ("audio", "state")

    def __init__(self) -> None:
        self.audio = _GainAudio()
        self.state = _GainState()


class _SidebarPads:
    __slots__ = ("_loaded", "_loading")

    def __init__(self, *, loaded: bool, loading: bool) -> None:
        self._loaded = loaded
        self._loading = loading

    def is_loaded(self, _pad_id: int) -> bool:
        return self._loaded

    def is_loading(self, _pad_id: int) -> bool:
        return self._loading


class _SidebarProject:
    __slots__ = ("selected_pad",)

    def __init__(self) -> None:
        self.selected_pad = 0


class _SidebarState:
    __slots__ = ("pads", "project")

    def __init__(self, *, loaded: bool, loading: bool) -> None:
        self.project = _SidebarProject()
        self.pads = _SidebarPads(loaded=loaded, loading=loading)


class _SidebarContext:
    __slots__ = ("state",)

    def __init__(self, *, loaded: bool, loading: bool) -> None:
        self.state = _SidebarState(loaded=loaded, loading=loading)


@contextmanager
def _noop_style_var(*_args: object, **_kwargs: object) -> Iterator[None]:
    yield None


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("123,456abc", "123.456"),
        ("12..3", "12.3"),
        ("a1b2c3", "123"),
        ("3,5", "3.5"),
        ("-6.0", "-6.0"),
        ("--60", "-60"),
        ("6-0", "60"),
        ("\u00dc1", "1"),
    ],
)
def test_sanitize_eq_entry_text(raw: str, expected: str) -> None:
    assert sanitize_eq_entry_text(raw) == expected


@pytest.mark.parametrize(
    ("char", "current", "cursor", "expected"),
    [
        ("1", "", 0, ord("1")),
        (",", "3", 1, ord(".")),
        (".", "3", 1, ord(".")),
        (".", "3.5", 3, None),
        ("-", "", 0, ord("-")),
        ("-", "3", 0, ord("-")),
        ("-", "3", 1, None),
        ("-", "-3", 0, None),
        ("\u00dc", "3", 1, None),
    ],
)
def test_filtered_eq_entry_char(
    char: str,
    current: str,
    cursor: int,
    expected: int | None,
) -> None:
    assert (
        filtered_eq_entry_char(
            ord(char),
            current,
            cursor,
            has_selection=False,
        )
        == expected
    )


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("3,5", 3.5),
        ("-6", -6.0),
        ("-6.0", -6.0),
        ("-60", -60.0),
        ("-999", -60.0),
        ("999", 6.0),
        ("001.2", 1.2),
    ],
)
def test_parse_eq_entry_text(raw: str, expected: float) -> None:
    assert parse_eq_entry_text(raw) == pytest.approx(expected)


@pytest.mark.parametrize("raw", ["", ".", "-", "-."])
def test_parse_eq_entry_text_ignores_empty_values(raw: str) -> None:
    assert parse_eq_entry_text(raw) is None


def test_filtered_eq_entry_char_accepts_replacing_selected_negative_sign() -> None:
    assert filtered_eq_entry_char(
        ord("-"),
        "-3",
        0,
        has_selection=True,
    ) == ord("-")


def test_gain_left_drag_tracks_absolute_pointer_position(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = _GainContext()
    sidebar_left._GAIN_DRAG.start(
        pad_id=0,
        button=imgui.MouseButton_.left,
        mouse_pos=imgui.ImVec2(10.0, 0.0),
    )

    monkeypatch.setattr("flitzis_looper.ui.render.sidebar_left.imgui.is_mouse_down", lambda _: True)
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left.imgui.get_item_rect_min",
        lambda: _Point(10.0, 0.0),
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left.imgui.get_item_rect_max",
        lambda: _Point(110.0, 0.0),
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left.imgui.get_io",
        lambda: _Io(85.0),
    )

    try:
        sidebar_left._apply_gain_drag(cast("UiContext", ctx), 0)
    finally:
        sidebar_left._GAIN_DRAG.clear()

    assert len(ctx.audio.pads.calls) == 1
    pad_id, gain_db = ctx.audio.pads.calls[0]
    assert pad_id == 0
    assert gain_db == pytest.approx(6.0)


@pytest.mark.parametrize(
    ("pad_state", "expected_key_lock_calls"),
    [
        ("loaded", [0]),
        ("loading", []),
        ("empty", []),
    ],
)
def test_sidebar_renders_pad_key_lock_only_for_loaded_pads(
    monkeypatch: pytest.MonkeyPatch,
    pad_state: str,
    expected_key_lock_calls: list[int],
) -> None:
    key_lock_calls: list[int] = []

    monkeypatch.setattr("flitzis_looper.ui.render.sidebar_left.style_var", _noop_style_var)
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left.imgui.get_content_region_avail",
        lambda: _Point(240.0, 0.0),
    )
    monkeypatch.setattr("flitzis_looper.ui.render.sidebar_left.imgui.separator", lambda: None)
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_pad_header",
        lambda _ctx, _info: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_bpm",
        lambda _ctx, _info: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_key",
        lambda _ctx, _info: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_loaded_gain",
        lambda _ctx, _pad_id: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_loaded_eq",
        lambda _ctx, _info: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_loaded_actions",
        lambda _ctx, _pad_id: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_loading_status",
        lambda _ctx, _pad_id: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_unloaded_actions",
        lambda _ctx, _pad_id: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_pad_key_lock",
        lambda _ctx, pad_id: key_lock_calls.append(pad_id),
    )

    sidebar_left.sidebar_left(
        cast(
            "UiContext",
            _SidebarContext(
                loaded=pad_state == "loaded",
                loading=pad_state == "loading",
            ),
        )
    )

    assert key_lock_calls == expected_key_lock_calls
