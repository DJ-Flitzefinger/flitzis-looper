import pytest
from imgui_bundle import imgui

from flitzis_looper.constants import PITCH_BPM_COARSE_STEPS
from flitzis_looper.ui.constants import CONTROL_ACTIVE_BORDER_RGBA, CONTROL_BORDER_RGBA
from flitzis_looper.ui.render.sidebar_right import (
    filtered_bpm_entry_char,
    parse_bpm_entry_text,
    pitch_center_indicator_color,
    pitch_center_indicator_y,
    sanitize_bpm_entry_text,
    snap_bpm_to_grid,
)


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("123,456abc", "123.45"),
        ("12..3", "12.3"),
        ("a1b2c3", "123"),
        ("120,5", "120.5"),
        ("0\n1 2", "012"),
    ],
)
def test_sanitize_bpm_entry_text(raw: str, expected: str) -> None:
    assert sanitize_bpm_entry_text(raw) == expected


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("123,456abc", 123.45),
        ("120.1", 120.1),
        ("001.20", 1.2),
    ],
)
def test_parse_bpm_entry_text(raw: str, expected: float) -> None:
    assert parse_bpm_entry_text(raw) == pytest.approx(expected)


@pytest.mark.parametrize("raw", ["", ".", "0", "0.00"])
def test_parse_bpm_entry_text_ignores_invalid_or_non_positive(raw: str) -> None:
    assert parse_bpm_entry_text(raw) is None


def test_snap_bpm_to_grid_uses_one_tenth() -> None:
    assert snap_bpm_to_grid(123.14) == pytest.approx(123.1)
    assert snap_bpm_to_grid(123.15) == pytest.approx(123.2)


def test_right_click_pitch_step_uses_one_bpm() -> None:
    assert PITCH_BPM_COARSE_STEPS == 10


@pytest.mark.parametrize(
    ("char", "current", "cursor", "expected"),
    [
        ("1", "", 0, ord("1")),
        (",", "120", 3, ord(".")),
        (".", "120", 3, ord(".")),
        (".", "120.1", 5, None),
        ("5", "120.12", 6, None),
        ("5", "120.12", 2, ord("5")),
        ("\u00dc", "120", 3, None),
    ],
)
def test_filtered_bpm_entry_char(
    char: str,
    current: str,
    cursor: int,
    expected: int | None,
) -> None:
    assert (
        filtered_bpm_entry_char(
            ord(char),
            current,
            cursor,
            has_selection=False,
        )
        == expected
    )


def test_pitch_center_indicator_y_matches_imgui_slider_grab_center() -> None:
    imgui.create_context()
    style = imgui.get_style()
    y = pitch_center_indicator_y(10.0, 410.0)

    grab_padding = 2.0
    grab_half = style.grab_min_size * 0.5
    usable_min_y = 10.0 + grab_padding + grab_half
    usable_max_y = 410.0 - grab_padding - grab_half
    expected = usable_max_y - (usable_max_y - usable_min_y) * (0.5 / 1.5)

    assert y == pytest.approx(expected)


def test_pitch_center_indicator_color_is_green_only_at_neutral() -> None:
    assert pitch_center_indicator_color(speed=1.0) is CONTROL_ACTIVE_BORDER_RGBA
    assert pitch_center_indicator_color(speed=1.01) is CONTROL_BORDER_RGBA
