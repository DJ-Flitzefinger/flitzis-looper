import pytest

from flitzis_looper.ui.render.bottom_bar import master_volume_wheel_delta
from flitzis_looper.ui.render.control_gestures import held_repeat_count, wheel_step_count
from flitzis_looper.ui.render.sidebar_left import eq_wheel_delta_db, gain_meter_zone_fractions


@pytest.mark.parametrize(
    ("wheel", "expected"),
    [
        (0.0, 0),
        (0.2, 1),
        (1.0, 1),
        (2.2, 2),
        (-0.2, -1),
        (-3.0, -3),
    ],
)
def test_wheel_step_count(wheel: float, expected: int) -> None:
    assert wheel_step_count(wheel) == expected


def test_control_specific_wheel_deltas() -> None:
    assert master_volume_wheel_delta(1) == pytest.approx(0.05)
    assert master_volume_wheel_delta(-2) == pytest.approx(-0.10)
    assert eq_wheel_delta_db(1) == pytest.approx(1.0)
    assert eq_wheel_delta_db(-2) == pytest.approx(-2.0)


def test_gain_meter_zone_fractions_are_green_yellow_only() -> None:
    green, yellow = gain_meter_zone_fractions()

    assert green == pytest.approx(0.8)
    assert yellow == pytest.approx(0.2)
    assert green + yellow == pytest.approx(1.0)


def test_held_repeat_count_waits_for_delay() -> None:
    assert held_repeat_count(0.34, 0.30) == 0


def test_held_repeat_count_emits_tick_at_delay_crossing() -> None:
    assert held_repeat_count(0.35, 0.34) == 1


def test_held_repeat_count_emits_crossed_interval_ticks() -> None:
    assert held_repeat_count(0.56, 0.34) == 3
    assert held_repeat_count(0.56, 0.45) == 1
