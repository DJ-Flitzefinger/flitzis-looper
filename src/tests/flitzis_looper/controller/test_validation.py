import math

import pytest

from flitzis_looper.controller.validation import ensure_finite, normalize_bpm


@pytest.mark.parametrize("bpm", [0.0, 1.5, -100.0, math.pi])
def test_ensure_finite_valid_values(bpm: float) -> None:
    """Test ensure_finite passes for valid finite values."""
    ensure_finite(bpm)


def test_ensure_finite_nan_raises() -> None:
    """Test ensure_finite raises ValueError for NaN."""
    with pytest.raises(ValueError, match="value must be finite, got nan"):
        ensure_finite(float("nan"))


@pytest.mark.parametrize("bpm", ["inf", "-inf"])
def test_ensure_finite_inf_raises(bpm: str) -> None:
    """Test ensure_finite raises ValueError for infinity."""
    with pytest.raises(ValueError, match="value must be finite"):
        ensure_finite(float(bpm))


def test_normalize_bpm_none() -> None:
    """Test normalize_bpm returns None for None input."""
    assert normalize_bpm(None) is None


@pytest.mark.parametrize("bpm", [120.0, 90.5, 1.0])
def test_normalize_bpm_valid(bpm: float) -> None:
    """Test normalize_bpm returns float for valid positive values."""
    assert normalize_bpm(bpm) == bpm


@pytest.mark.parametrize("bpm", ["nan", "inf", "-inf"])
def test_normalize_bpm_non_finite(bpm: str) -> None:
    """Test normalize_bpm returns None for non-finite values."""
    assert normalize_bpm(float(bpm)) is None


@pytest.mark.parametrize("bpm", [0.0, -1.0, -120.5])
def test_normalize_bpm_non_positive(bpm: float) -> None:
    """Test normalize_bpm returns None for non-positive values."""
    assert normalize_bpm(bpm) is None
