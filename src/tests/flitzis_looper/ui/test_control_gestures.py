import pytest

from flitzis_looper.ui.render.control_gestures import held_repeat_count, wheel_step_count


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


def test_held_repeat_count_waits_for_delay() -> None:
    assert held_repeat_count(0.34, 0.30) == 0


def test_held_repeat_count_emits_tick_at_delay_crossing() -> None:
    assert held_repeat_count(0.35, 0.34) == 1


def test_held_repeat_count_emits_crossed_interval_ticks() -> None:
    assert held_repeat_count(0.56, 0.34) == 3
    assert held_repeat_count(0.56, 0.45) == 1
