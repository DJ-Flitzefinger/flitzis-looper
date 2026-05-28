import pytest

from flitzis_looper.ui.render import sidebar_left
from flitzis_looper.ui.render.sidebar_left import (
    filtered_eq_entry_char,
    parse_eq_entry_text,
    sanitize_eq_entry_text,
)


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
        button=sidebar_left.imgui.MouseButton_.left,
        mouse_pos=_Point(10.0, 0.0),
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
        sidebar_left._apply_gain_drag(ctx, 0)
    finally:
        sidebar_left._GAIN_DRAG.clear()

    assert ctx.audio.pads.calls == [(0, pytest.approx(6.0))]
