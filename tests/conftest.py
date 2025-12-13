"""Pytest configuration and shared fixtures."""

import pytest


@pytest.fixture
def sample_audio_file():
    """Provide a sample audio file path for testing."""
    # This fixture can be expanded later with actual test audio files
    return "tests/data/sample.wav"


@pytest.fixture
def mock_config():
    """Provide a mock configuration object for testing."""
    return {
        "sample_rate": 44100,
        "buffer_size": 512,
        "bpm": 120,
    }
