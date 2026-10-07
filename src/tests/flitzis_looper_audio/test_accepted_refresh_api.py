"""Opaque native acceptance/refresh boundaries without an app or device stream."""

import pytest

from flitzis_looper_audio import (
    AcceptedTimingRefreshTicket,
    AudioEngine,
    CapturedConstantTiming,
)


@pytest.mark.parametrize("ticket_type", [CapturedConstantTiming, AcceptedTimingRefreshTicket])
def test_accepted_control_owners_cannot_be_constructed_from_python(ticket_type: type) -> None:
    with pytest.raises(TypeError):
        ticket_type()


def test_preparation_capture_requires_opaque_actual_native_binding() -> None:
    engine = AudioEngine()
    with pytest.raises(TypeError):
        engine.capture_current_constant_timing(
            {"pad_id": 0, "intent": "automatic"},  # type: ignore[arg-type]
            0.01,
            "independent engineering assertion",
        )


def test_background_preparation_cannot_capture_a_later_source_from_metadata() -> None:
    engine = AudioEngine()
    with pytest.raises(TypeError):
        engine.prepare_captured_constant_timing({"pad_id": 0})  # type: ignore[arg-type]


def test_derived_refresh_cannot_use_historical_or_provisional_metadata_as_authority() -> None:
    engine = AudioEngine()
    with pytest.raises(TypeError):
        engine.refresh_current_constant_timing(
            {"pad_id": 0, "revision": "historical"},  # type: ignore[arg-type]
            0.0,
            2.0,
            0.5,
        )
