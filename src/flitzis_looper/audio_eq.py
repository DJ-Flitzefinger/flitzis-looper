import math

from flitzis_looper.constants import PAD_EQ_DB_MAX, PAD_EQ_DB_MIN

EQ_KNOB_POSITION_MIN = 0.0
EQ_KNOB_POSITION_CENTER = 0.5
EQ_KNOB_POSITION_MAX = 1.0
EQ_VERTICAL_DRAG_SENSITIVITY = 1.0 / 200.0
EQ_HORIZONTAL_DRAG_RATIO = 0.5


def clamp_eq_db(db: float) -> float:
    """Clamp a finite pad EQ band value to the supported dB range."""
    if not math.isfinite(db):
        msg = "eq db must be finite"
        raise ValueError(msg)
    return min(max(float(db), PAD_EQ_DB_MIN), PAD_EQ_DB_MAX)


def eq_db_to_knob_position(db: float) -> float:
    """Map EQ dB to a neutral-centered `0.0..1.0` knob position."""
    db = clamp_eq_db(db)
    if db <= PAD_EQ_DB_MIN:
        return EQ_KNOB_POSITION_MIN
    if db <= 0.0:
        return EQ_KNOB_POSITION_CENTER * math.pow(10.0, db / 20.0)

    return EQ_KNOB_POSITION_CENTER + EQ_KNOB_POSITION_CENTER * (db / PAD_EQ_DB_MAX)


def eq_knob_position_to_db(position: float) -> float:
    """Map a neutral-centered `0.0..1.0` EQ knob position to dB."""
    if not math.isfinite(position):
        msg = "eq knob position must be finite"
        raise ValueError(msg)

    position = min(max(float(position), EQ_KNOB_POSITION_MIN), EQ_KNOB_POSITION_MAX)
    if position <= EQ_KNOB_POSITION_MIN:
        return PAD_EQ_DB_MIN
    if position <= EQ_KNOB_POSITION_CENTER:
        db = 20.0 * math.log10(position / EQ_KNOB_POSITION_CENTER)
        return max(db, PAD_EQ_DB_MIN)

    return min(
        PAD_EQ_DB_MAX,
        (position - EQ_KNOB_POSITION_CENTER) / EQ_KNOB_POSITION_CENTER * PAD_EQ_DB_MAX,
    )


def eq_drag_delta_position(delta_x: float, delta_y: float) -> float:
    """Convert EQ drag movement to neutral-centered knob-position delta."""
    return (
        float(delta_x) * EQ_HORIZONTAL_DRAG_RATIO - float(delta_y)
    ) * EQ_VERTICAL_DRAG_SENSITIVITY


def format_eq_db(db: float) -> str:
    """Format an EQ band value using one decimal place."""
    return f"{clamp_eq_db(db):.1f} dB"
