from imgui_bundle import icons_fontawesome_6

from flitzis_looper.ui.render.bottom_bar import settings_button_local_pos
from flitzis_looper.ui.render.settings import (
    SETTINGS_TOGGLE_BUTTON_SIZE,
    settings_surface_child_id,
    settings_toggle_button_label,
    settings_toggle_tooltip,
)


def test_settings_toggle_uses_gear_when_closed() -> None:
    assert settings_toggle_button_label(settings_open=False) == (
        f"{icons_fontawesome_6.ICON_FA_GEAR}##settings_toggle"
    )
    assert settings_toggle_tooltip(settings_open=False) == "Open settings"


def test_settings_toggle_uses_x_when_open() -> None:
    assert settings_toggle_button_label(settings_open=True) == (
        f"{icons_fontawesome_6.ICON_FA_XMARK}##settings_toggle"
    )
    assert settings_toggle_tooltip(settings_open=True) == "Close settings"


def test_settings_overlay_replaces_main_surface_id() -> None:
    assert settings_surface_child_id(settings_open=False) == "looper_main"
    assert settings_surface_child_id(settings_open=True) == "settings_overlay"


def test_settings_button_position_right_aligns_inside_bottom_bar() -> None:
    x, y = settings_button_local_pos(
        cursor_x=0.0,
        cursor_y=0.0,
        available_width=1538.0,
        available_height=55.0,
    )

    assert x + SETTINGS_TOGGLE_BUTTON_SIZE == 1538.0
    assert y == 9.5


def test_settings_button_position_handles_tiny_bottom_bar() -> None:
    x, y = settings_button_local_pos(
        cursor_x=4.0,
        cursor_y=6.0,
        available_width=24.0,
        available_height=20.0,
    )

    assert x == 4.0
    assert y == 6.0
