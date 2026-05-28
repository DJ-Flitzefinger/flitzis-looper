import pytest

from flitzis_looper.ui.render.sidebar_left import (
    filtered_eq_entry_char,
    parse_eq_entry_text,
    sanitize_eq_entry_text,
)


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
