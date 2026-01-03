import math


def ensure_finite(value: float) -> None:
    """Ensure value is finite.

    Args:
        value: Value to check.

    Raises:
        ValueError: If value is not finite.
    """
    if not math.isfinite(value):
        msg = f"value must be finite, got {value!r}"
        raise ValueError(msg)


def normalize_bpm(bpm: float | None) -> float | None:
    if bpm is None:
        return None
    if not math.isfinite(bpm) or bpm <= 0:
        return None
    return float(bpm)
