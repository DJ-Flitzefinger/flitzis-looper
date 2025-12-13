"""Tests for mathematical utility functions."""

import math

import pytest

from flitzis_looper.utils.math import db_to_amp, speed_to_semitones


class TestDBToAmp:
    """Tests for db_to_amp function."""

    def test_zero_db_returns_one(self):
        """Test that 0 dB converts to amplitude of 1.0."""
        assert db_to_amp(0.0) == 1.0

    def test_positive_db_values(self):
        """Test positive dB values produce amplitudes > 1.0."""
        assert db_to_amp(6.0) == pytest.approx(1.9953, rel=1e-4)
        assert db_to_amp(12.0) == pytest.approx(3.9811, rel=1e-4)

    def test_negative_db_values(self):
        """Test negative dB values produce amplitudes < 1.0."""
        assert db_to_amp(-6.0) == pytest.approx(0.5012, rel=1e-4)
        assert db_to_amp(-12.0) == pytest.approx(0.2512, rel=1e-4)

    def test_large_values(self):
        """Test large dB values."""
        assert db_to_amp(20.0) == pytest.approx(10.0, rel=1e-4)
        assert db_to_amp(-20.0) == pytest.approx(0.1, rel=1e-4)


class TestSpeedToSemitones:
    """Tests for speed_to_semitones function."""

    def test_normal_speed_returns_zero(self):
        """Test that speed of 1.0 returns 0 semitones (no pitch change)."""
        assert speed_to_semitones(1.0) == 0.0

    def test_double_speed(self):
        """Test that speed of 2.0 returns -12 semitones (one octave down)."""
        assert speed_to_semitones(2.0) == pytest.approx(-12.0)

    def test_half_speed(self):
        """Test that speed of 0.5 returns 12 semitones (one octave up)."""
        assert speed_to_semitones(0.5) == pytest.approx(12.0)

    def test_quarter_speed(self):
        """Test that speed of 0.25 returns 24 semitones (two octaves up)."""
        assert speed_to_semitones(0.25) == pytest.approx(24.0)

    def test_zero_or_negative_speed(self):
        """Test that zero or negative speeds return 0 semitones."""
        assert speed_to_semitones(0.0) == 0.0
        assert speed_to_semitones(-1.0) == 0.0

    def test_sqrt2_speed(self):
        """Test that speed of sqrt(2) returns -6 semitones (tritone down)."""
        assert speed_to_semitones(math.sqrt(2)) == pytest.approx(-6.0, rel=1e-10)
