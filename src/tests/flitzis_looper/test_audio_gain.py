import pytest

from flitzis_looper.audio_gain import (
    clamp_gain_db,
    format_gain_db,
    gain_db_to_linear,
    gain_drag_delta_db,
    gain_fine_step_direction,
    gain_meter_fraction_from_peak,
    normalized_to_gain_db,
)


def test_normalized_gain_maps_to_db_range() -> None:
    assert normalized_to_gain_db(0.0) == pytest.approx(-12.0)
    assert normalized_to_gain_db(0.5) == pytest.approx(0.0)
    assert normalized_to_gain_db(1.0) == pytest.approx(12.0)


def test_gain_db_to_linear_reference_values() -> None:
    assert gain_db_to_linear(0.0) == pytest.approx(1.0)
    assert gain_db_to_linear(6.0) == pytest.approx(1.995, abs=0.001)
    assert gain_db_to_linear(-6.0) == pytest.approx(0.501, abs=0.001)


def test_gain_db_clamps_to_range() -> None:
    assert clamp_gain_db(-24.0) == -12.0
    assert clamp_gain_db(24.0) == 12.0


def test_gain_db_display_formatting() -> None:
    assert format_gain_db(0.0) == "0.0 dB"
    assert format_gain_db(1.5) == "+1.5 dB"
    assert format_gain_db(-3.0) == "-3.0 dB"


def test_gain_fine_step_direction_uses_axis_sides() -> None:
    assert gain_fine_step_direction(25.0, 0.0, 100.0) == -1
    assert gain_fine_step_direction(75.0, 0.0, 100.0) == 1


def test_gain_drag_delta_direction_and_fine_speed() -> None:
    assert gain_drag_delta_db(10.0, 0.0, fine=False) == pytest.approx(1.0)
    assert gain_drag_delta_db(0.0, 10.0, fine=False) == pytest.approx(-1.0)
    assert gain_drag_delta_db(10.0, 0.0, fine=True) == pytest.approx(0.2)


def test_gain_meter_fraction_handles_silence_and_clip() -> None:
    assert gain_meter_fraction_from_peak(0.0) == 0.0
    assert gain_meter_fraction_from_peak(1.0) == 1.0
