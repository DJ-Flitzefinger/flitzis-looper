import time
from typing import TYPE_CHECKING, cast

import pytest

if TYPE_CHECKING:
    from flitzis_looper_audio import AudioEngine


def test_native_midi_capture_uses_exposed_engine_epoch(audio_engine: AudioEngine) -> None:
    audio_engine.set_input_mapping_enabled(True)
    before = audio_engine.capture_input_timestamp_ns()
    assert audio_engine.inject_midi_input_for_test([0x90, 60, 100])
    after = audio_engine.capture_input_timestamp_ns()
    deadline = time.monotonic() + 1.0
    while time.monotonic() < deadline:
        event = audio_engine.poll_input_events()
        if event is not None:
            captured = event["received_at_ns"]
            assert type(captured) is int
            assert before <= captured <= after
            return
        time.sleep(0.001)
    pytest.fail("native MIDI capture did not publish its event")


@pytest.mark.parametrize(
    ("timestamp", "error"),
    [(True, TypeError), (1.5, TypeError), (-1, ValueError), (1 << 64, ValueError)],
)
def test_native_play_timestamp_arguments_reject_invalid_values(
    uninitialized_audio_engine: AudioEngine, timestamp: object, error: type[Exception]
) -> None:
    for play in (
        uninitialized_audio_engine.play_sample,
        uninitialized_audio_engine.play_sample_exclusive,
    ):
        with pytest.raises(error, match="received_at_ns"):
            play(0, 1.0, received_at_ns=cast("int", timestamp))


def test_native_clock_snapshot_and_legacy_playback_api(audio_engine: AudioEngine) -> None:
    deadline = time.monotonic() + 1.0
    while time.monotonic() < deadline:
        snapshot = audio_engine.output_clock_snapshot()
        if snapshot is not None:
            assert snapshot["sample_rate_hz"] == audio_engine.output_sample_rate()
            assert type(snapshot["valid"]) is bool
            assert type(snapshot["fresh"]) is bool
            assert type(snapshot["observed_at_ns"]) is int
            assert type(snapshot["audible_at_ns"]) is int
            break
        time.sleep(0.001)
    else:
        pytest.fail("running stream did not publish an output-clock observation")

    assert audio_engine.input_clock_target_frame(None) is None
    assert audio_engine.input_clock_target_frame(1 << 63) is None
    audio_engine.play_sample(0, 1.0)
    audio_engine.play_sample_exclusive(0, 1.0, received_at_ns=0)
