import math

from flitzis_looper.constants import (
    PAD_GAIN_DB_DEFAULT,
    PAD_GAIN_DB_MAX,
    PAD_GAIN_DB_MIN,
    PAD_GAIN_NORMALIZED_CENTER,
    PAD_GAIN_NORMALIZED_MAX,
    PAD_GAIN_NORMALIZED_MIN,
)


def clamp_gain_db(gain_db: float) -> float:
    """Clamp a finite pad Gain/Trim value to the supported dB range."""
    if not math.isfinite(gain_db):
        msg = "gain_db must be finite"
        raise ValueError(msg)
    return min(max(float(gain_db), PAD_GAIN_DB_MIN), PAD_GAIN_DB_MAX)


def normalized_to_gain_db(normalized: float) -> float:
    """Map normalized UI position `0.0..1.0` to `-12..+12 dB` Gain/Trim."""
    if not math.isfinite(normalized):
        msg = "normalized gain must be finite"
        raise ValueError(msg)
    normalized = min(max(float(normalized), PAD_GAIN_NORMALIZED_MIN), PAD_GAIN_NORMALIZED_MAX)
    return PAD_GAIN_DB_MIN + normalized * (PAD_GAIN_DB_MAX - PAD_GAIN_DB_MIN)


def gain_db_to_normalized(gain_db: float) -> float:
    """Map dB Gain/Trim to normalized UI position."""
    gain_db = clamp_gain_db(gain_db)
    return (gain_db - PAD_GAIN_DB_MIN) / (PAD_GAIN_DB_MAX - PAD_GAIN_DB_MIN)


def gain_db_to_linear(gain_db: float) -> float:
    """Convert dB Gain/Trim to linear gain."""
    return math.pow(10.0, clamp_gain_db(gain_db) / 20.0)


def format_gain_db(gain_db: float) -> str:
    """Format Gain/Trim using professional signed dB display semantics."""
    gain_db = clamp_gain_db(gain_db)
    if abs(gain_db) < 0.05:
        gain_db = PAD_GAIN_DB_DEFAULT
    if gain_db > 0.0:
        return f"+{gain_db:.1f} dB"
    return f"{gain_db:.1f} dB"


def legacy_gain_value_to_db(value: object) -> float:
    """Convert legacy linear or percent gain values to dB Gain/Trim."""
    if not isinstance(value, int | float | str):
        return PAD_GAIN_DB_DEFAULT

    try:
        legacy = float(value)
    except (TypeError, ValueError):
        return PAD_GAIN_DB_DEFAULT

    if not math.isfinite(legacy):
        return PAD_GAIN_DB_DEFAULT
    if legacy <= 0.0:
        return PAD_GAIN_DB_MIN
    if legacy <= 1.0:
        linear = legacy
    elif legacy <= 100.0:
        linear = legacy / 100.0
    else:
        linear = 1.0

    return min(max(20.0 * math.log10(linear), PAD_GAIN_DB_MIN), PAD_GAIN_DB_MAX)


def gain_meter_fraction_from_peak(peak: float, *, floor_db: float = -60.0) -> float:
    """Return horizontal meter fill fraction from a linear sample peak."""
    if not math.isfinite(peak) or peak <= 0.0:
        return 0.0
    dbfs = 20.0 * math.log10(max(float(peak), 1e-12))
    dbfs = min(max(dbfs, floor_db), 0.0)
    return (dbfs - floor_db) / abs(floor_db)


def gain_fine_step_direction(click_x: float, axis_min_x: float, axis_max_x: float) -> int:
    """Return `-1` for negative-axis fine step and `1` for positive-axis fine step."""
    center_x = axis_min_x + (axis_max_x - axis_min_x) * PAD_GAIN_NORMALIZED_CENTER
    return -1 if click_x < center_x else 1


def gain_drag_delta_db(delta_x: float, delta_y: float, *, fine: bool) -> float:
    """Convert Gain/Trim drag movement to dB delta."""
    sensitivity = 0.02 if fine else 0.10
    return (float(delta_x) - float(delta_y)) * sensitivity
