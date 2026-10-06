"""Public boundary checks without starting the app or a device stream.

Actual loaded-source races and mixer adoption use the in-memory native production
tests; these checks protect opaque-ticket and explicit-intent PyO3 contracts.
"""

import pytest

from flitzis_looper_audio import AudioEngine, ConstantTimingTicket


def test_constant_timing_ticket_cannot_be_synthesized_from_python() -> None:
    with pytest.raises(TypeError):
        ConstantTimingTicket()


def test_constant_timing_requires_an_actual_initialized_engine() -> None:
    engine = AudioEngine()
    with pytest.raises(RuntimeError, match="not initialized"):
        engine.prepare_constant_timing(
            sample_id=0,
            timing_error_halfwidth_seconds=0.01,
            timing_error_provenance="explicit engineering bound",
        )


def test_constant_timing_cannot_publish_a_matching_caller_snapshot() -> None:
    engine = AudioEngine()
    with pytest.raises(TypeError):
        engine.publish_constant_timing(
            {"pad_id": 0, "request_id": 1},  # type: ignore[arg-type]
            "[]",
            0.0,
            "independent origin",
            "explicit-test-policy",
            "independent acceptance",
        )
