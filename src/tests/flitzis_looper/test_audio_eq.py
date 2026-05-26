import pytest

from flitzis_looper.audio_eq import (
    EQ_HORIZONTAL_DRAG_RATIO,
    EQ_KNOB_POSITION_CENTER,
    EQ_KNOB_POSITION_MAX,
    EQ_KNOB_POSITION_MIN,
    EQ_VERTICAL_DRAG_SENSITIVITY,
    eq_db_to_knob_position,
    eq_drag_delta_position,
    eq_knob_position_to_db,
)
from flitzis_looper.constants import PAD_EQ_DB_MAX, PAD_EQ_DB_MIN


def test_eq_knob_position_places_neutral_at_center() -> None:
    assert eq_db_to_knob_position(PAD_EQ_DB_MIN) == pytest.approx(EQ_KNOB_POSITION_MIN)
    assert eq_db_to_knob_position(0.0) == pytest.approx(EQ_KNOB_POSITION_CENTER)
    assert eq_db_to_knob_position(PAD_EQ_DB_MAX) == pytest.approx(EQ_KNOB_POSITION_MAX)


def test_eq_knob_position_preserves_negative_kill_curve() -> None:
    assert eq_db_to_knob_position(-6.020599913279624) == pytest.approx(0.25)
    assert eq_db_to_knob_position(-20.0) == pytest.approx(0.05)


def test_eq_knob_position_positive_range_is_linear() -> None:
    assert eq_db_to_knob_position(PAD_EQ_DB_MAX * 0.5) == pytest.approx(0.75)


@pytest.mark.parametrize("db", [PAD_EQ_DB_MIN, -24.0, -6.0, 0.0, 3.0, PAD_EQ_DB_MAX])
def test_eq_knob_position_round_trips_to_db(db: float) -> None:
    position = eq_db_to_knob_position(db)

    assert eq_knob_position_to_db(position) == pytest.approx(db, abs=1e-6)


def test_eq_drag_delta_position_uses_horizontal_half_speed() -> None:
    vertical_delta = eq_drag_delta_position(0.0, -10.0)
    horizontal_delta = eq_drag_delta_position(10.0, 0.0)

    assert vertical_delta == pytest.approx(10.0 * EQ_VERTICAL_DRAG_SENSITIVITY)
    assert horizontal_delta == pytest.approx(vertical_delta * EQ_HORIZONTAL_DRAG_RATIO)
