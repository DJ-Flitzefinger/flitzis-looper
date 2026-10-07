"""Demand-only observation API validation without creating an audio stream."""

import pytest

from flitzis_looper_audio import AudioEngine


def test_loop_observation_and_device_descriptor_do_not_initialize_audio() -> None:
    engine = AudioEngine()
    assert engine.output_device_descriptor() is None
    with pytest.raises(RuntimeError, match="not initialized"):
        engine.request_loop_acceptance_snapshot(0)
    with pytest.raises(RuntimeError, match="not initialized"):
        engine.loop_acceptance_snapshot(0, 256)
