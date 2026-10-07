"""Opaque native global admission boundaries without app or device control."""

from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper_audio import AudioEngine, GlobalPlaybackBatchTicket

if TYPE_CHECKING:
    from flitzis_looper_audio import InputRuntimePadBinding


def test_global_batch_ticket_cannot_be_constructed_from_python() -> None:
    with pytest.raises(TypeError):
        GlobalPlaybackBatchTicket()


@pytest.mark.parametrize("start", [False, True])
def test_global_batch_rejects_python_metadata_in_place_of_native_source_ownership(
    *, start: bool
) -> None:
    engine = AudioEngine()
    caller_binding = cast("InputRuntimePadBinding", {"pad_id": 0, "authority_revision": 1})
    if start:
        with pytest.raises(TypeError):
            engine.start_global_playback_batch([(caller_binding, 0.0, None)])
    else:
        with pytest.raises(TypeError):
            engine.stop_global_playback_batch([caller_binding])


@pytest.mark.parametrize("start", [False, True])
def test_global_batch_admission_requires_initialized_native_engine(*, start: bool) -> None:
    engine = AudioEngine()
    if start:
        with pytest.raises(RuntimeError, match="not initialized"):
            engine.start_global_playback_batch([])
    else:
        with pytest.raises(RuntimeError, match="not initialized"):
            engine.stop_global_playback_batch([])


@pytest.mark.parametrize("start", [False, True])
@pytest.mark.parametrize("timestamp", [-1, 1 << 64])
def test_global_batch_rejects_negative_or_overflowing_time_before_engine_admission(
    timestamp: int, *, start: bool
) -> None:
    engine = AudioEngine()
    if start:
        with pytest.raises(ValueError, match="received_at_ns"):
            engine.start_global_playback_batch([], received_at_ns=timestamp)
    else:
        with pytest.raises(ValueError, match="received_at_ns"):
            engine.stop_global_playback_batch([], received_at_ns=timestamp)


@pytest.mark.parametrize("start", [False, True])
@pytest.mark.parametrize("timestamp", [True, 1.5, "12"])
def test_global_batch_rejects_noninteger_input_time(timestamp: object, *, start: bool) -> None:
    engine = AudioEngine()
    invalid_time = cast("int", timestamp)
    if start:
        with pytest.raises(TypeError, match="received_at_ns"):
            engine.start_global_playback_batch([], received_at_ns=invalid_time)
    else:
        with pytest.raises(TypeError, match="received_at_ns"):
            engine.stop_global_playback_batch([], received_at_ns=invalid_time)
