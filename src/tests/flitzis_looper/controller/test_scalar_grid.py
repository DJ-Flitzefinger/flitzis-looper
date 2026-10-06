"""Independent scalar timing truth and absolute physical marker rounding."""

import math
from decimal import Decimal

import pytest

from flitzis_looper.controller.scalar_grid import (
    beat_duration_s,
    nearest_grid_source_s,
    physical_source_marker_s,
    scalar_source_grid,
)
from flitzis_looper_audio import ScalarSourceGrid


@pytest.mark.parametrize("sample_rate_hz", [44_100, 48_000, 96_000])
@pytest.mark.parametrize("bpm", [119.999, 123.45])
@pytest.mark.parametrize("origin_frame", [-217, 0, 28_776_001])
def test_fractional_projection_rounds_each_absolute_subdivision_once(
    sample_rate_hz: int, bpm: float, origin_frame: int
) -> None:
    origin_s = origin_frame / sample_rate_hz
    grid = scalar_source_grid(origin_s=origin_s, bpm=bpm)
    assert grid is not None
    period_frames = Decimal(60 * sample_rate_hz) / Decimal(str(bpm))

    for tick in [0, 1, 15, 16, 320, 960, 19_184]:
        beat = tick / 16.0
        source_s = grid.source_at_beat(beat)
        assert source_s is not None
        exact_frame = Decimal(origin_frame) + Decimal(tick) * period_frames / 16
        marker_s = physical_source_marker_s(source_s, sample_rate_hz=sample_rate_hz)
        expected_frame = max(round(exact_frame), 0)
        assert round(marker_s * sample_rate_hz) == expected_frame
        if exact_frame >= 0:
            assert abs(Decimal(expected_frame) - exact_frame) <= Decimal("0.5")
        assert grid.beat_at_source(source_s) == pytest.approx(beat, abs=1e-10)
        assert nearest_grid_source_s(source_s, grid=grid, step_beats=1 / 16) == source_s


def test_projection_preserves_full_effective_precision() -> None:
    grid = scalar_source_grid(origin_s=0.0, bpm=119.999)
    assert grid is not None
    position_s = grid.source_at_beat(1_199.0)
    assert position_s is not None
    expected_s = Decimal(1_199 * 60) / Decimal("119.999")
    assert abs(Decimal(position_s) - expected_s) < Decimal("1e-12")


def test_explicit_period_overrides_bpm_without_a_roundtrip() -> None:
    period = 0.48602673147023145
    assert beat_duration_s(90.0, period_seconds=period) == period
    grid = scalar_source_grid(origin_s=0.0, bpm=90.0, period_seconds=period)
    assert grid is not None
    assert grid.source_at_beat(1.0) == period
    assert grid.source_at_beat(16.0) == period * 16.0


@pytest.mark.parametrize("period", [0.0, -0.5, math.nan, math.inf])
def test_invalid_explicit_period_cannot_revive_legacy_bpm(period: float) -> None:
    assert beat_duration_s(120.0, period_seconds=period) is None
    assert scalar_source_grid(origin_s=-0.1, bpm=120.0, period_seconds=period) is None


def test_signed_fractional_origin_and_explicit_period_use_absolute_boundary_rounding() -> None:
    origin = -0.010000000000002
    period = 0.48602673147023145
    grid = scalar_source_grid(origin_s=origin, period_seconds=period)
    assert grid is not None
    beat = 1049 / 16
    source_s = grid.source_at_beat(beat)
    assert source_s is not None
    exact_frame = (Decimal(origin) + Decimal(period) * Decimal(beat)) * 44_100
    marker_s = physical_source_marker_s(source_s, sample_rate_hz=44_100)
    assert round(marker_s * 44_100) == round(exact_frame)


def test_source_advancement_preserves_offgrid_phase_without_rounding_duration() -> None:
    grid = scalar_source_grid(origin_s=-0.05, bpm=123.45)
    assert grid is not None
    selected_s = 599.5 + 0.49 / 48_000
    end_s = grid.source_after_beats(selected_s, 2.0)
    assert end_s is not None
    expected_s = Decimal(selected_s) + Decimal(120) / Decimal("123.45")
    assert abs(Decimal(end_s) - expected_s) < Decimal("1e-12")


def test_nearest_even_grid_and_physical_ties_remain_unchanged() -> None:
    grid = ScalarSourceGrid(-1.0, 1.0)
    assert nearest_grid_source_s(0.5, grid=grid, step_beats=1.0) == 1.0
    assert nearest_grid_source_s(1.5, grid=grid, step_beats=1.0) == 1.0
    assert physical_source_marker_s(0.5, sample_rate_hz=1) == 0.0
    assert physical_source_marker_s(1.5, sample_rate_hz=1) == 2.0


def test_nearest_subdivision_preserves_the_shifted_source_anchor() -> None:
    grid = scalar_source_grid(origin_s=1.0 / 128.0, bpm=120.0)
    assert grid is not None
    # The requested position is three quarters of a finest-grid step past the origin.
    assert nearest_grid_source_s(1.0 / 32.0, grid=grid, step_beats=1 / 16) == 5.0 / 128.0


@pytest.mark.parametrize("bpm", [None, 0.0, -1.0, math.nan, math.inf])
def test_invalid_scalar_period_does_not_fabricate_a_grid(bpm: float | None) -> None:
    assert scalar_source_grid(origin_s=0.0, bpm=bpm) is None


def test_invalid_native_coordinates_return_no_projection() -> None:
    grid = ScalarSourceGrid(-0.05, 0.5)
    assert grid.source_at_beat(math.inf) is None
    assert grid.beat_at_source(math.nan) is None
    assert grid.source_after_beats(1.0, math.inf) is None
    with pytest.raises(ValueError, match="finite"):
        ScalarSourceGrid(math.nan, 0.5)
