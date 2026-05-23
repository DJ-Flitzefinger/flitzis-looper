import math
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from flitzis_looper.models import SampleAnalysis


def timing_anchor_sec_from_analysis(analysis: SampleAnalysis | None) -> float:
    """Return the bounded pad timing anchor prepared for Rust playback timing."""
    if analysis is None:
        return 0.0

    downbeat = _finite_nonnegative_first(analysis.beat_grid.downbeats)
    if downbeat is not None:
        return downbeat

    beat = _finite_nonnegative_first(analysis.beat_grid.beats)
    if beat is not None:
        return beat

    return 0.0


def _finite_nonnegative_first(values: list[float]) -> float | None:
    if not values:
        return None

    value = float(values[0])
    if not math.isfinite(value) or value < 0.0:
        return None

    return value
