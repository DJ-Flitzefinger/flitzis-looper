from imgui_bundle import icons_fontawesome_6

from flitzis_looper.constants import MAX_KEY_LOCK_SMOOTHING_STEP, MIN_KEY_LOCK_SMOOTHING_STEP
from flitzis_looper.models import (
    KEY_LOCK_INTERPOLATION_LABELS,
    KEY_LOCK_INTERPOLATIONS,
    KEY_LOCK_WINDOW_LABELS,
    KEY_LOCK_WINDOWS,
    TRIGGER_QUANTIZATION_STEP_LABELS,
    TRIGGER_QUANTIZATION_STEPS,
)
from flitzis_looper.ui.render.bottom_bar import settings_button_local_pos
from flitzis_looper.ui.render.settings import (
    KEY_LOCK_PARAMETER_HINTS,
    SETTINGS_TOGGLE_BUTTON_SIZE,
    clamp_key_lock_smoothing_step_for_settings,
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


def test_settings_quantize_grid_options_cover_loop_editor_grid() -> None:
    assert TRIGGER_QUANTIZATION_STEPS == ("1_64", "1_32", "1_16")
    assert TRIGGER_QUANTIZATION_STEP_LABELS["1_16"] == "1/16"
    assert TRIGGER_QUANTIZATION_STEP_LABELS["1_64"] == "1/64"


def test_settings_key_lock_parameter_options_cover_manual_dsp_choices() -> None:
    assert KEY_LOCK_INTERPOLATIONS == ("linear", "cubic")
    assert KEY_LOCK_INTERPOLATION_LABELS["linear"] == "Linear"
    assert KEY_LOCK_INTERPOLATION_LABELS["cubic"] == "Cubic"
    assert KEY_LOCK_WINDOWS == ("triangle", "hann")
    assert KEY_LOCK_WINDOW_LABELS["triangle"] == "Triangle"
    assert KEY_LOCK_WINDOW_LABELS["hann"] == "Hann"
    assert set(KEY_LOCK_PARAMETER_HINTS) == {
        "delay_min",
        "delay_range",
        "heads",
        "interpolation",
        "window",
        "smoothing",
        "output_gain",
    }


def test_settings_clamps_smoothing_step_float_edge_values() -> None:
    assert (
        clamp_key_lock_smoothing_step_for_settings(MIN_KEY_LOCK_SMOOTHING_STEP - 1.0e-9)
        == MIN_KEY_LOCK_SMOOTHING_STEP
    )
    assert (
        clamp_key_lock_smoothing_step_for_settings(MAX_KEY_LOCK_SMOOTHING_STEP + 1.0e-9)
        == MAX_KEY_LOCK_SMOOTHING_STEP
    )
    assert clamp_key_lock_smoothing_step_for_settings(0.05) == 0.05


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
