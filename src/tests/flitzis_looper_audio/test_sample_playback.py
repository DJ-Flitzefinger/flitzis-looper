from __future__ import annotations

import time
import wave
from array import array
from typing import TYPE_CHECKING

import pytest

from flitzis_looper_audio import AudioEngine

if TYPE_CHECKING:
    from pathlib import Path


def _write_mono_pcm16_wav(path: Path, sample_rate_hz: int) -> None:
    samples = array("h", [8192] * 128)

    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(sample_rate_hz)
        wav.writeframes(samples.tobytes())


def test_load_and_play_sample_smoke(audio_engine: AudioEngine, tmp_path: Path) -> None:
    wav_path = tmp_path / "sample.wav"

    for sample_rate_hz in (48_000, 44_100):
        _write_mono_pcm16_wav(wav_path, sample_rate_hz)

        audio_engine.load_sample_async(0, str(wav_path))

        deadline = time.monotonic() + 2.0
        success = False
        error_msg: str | None = None

        while time.monotonic() < deadline:
            event = audio_engine.poll_loader_events()
            if event is None:
                time.sleep(0.01)
                continue

            if event.get("id") != 0:
                continue

            if event.get("type") == "success":
                success = True
                break

            if event.get("type") == "error":
                msg = event.get("msg")
                if isinstance(msg, str):
                    error_msg = msg
                break

        if success:
            break

        if error_msg is not None and "sample rate mismatch" in error_msg:
            continue

        pytest.fail(f"expected sample load success, got error: {error_msg!r}")
    else:
        pytest.skip("No matching sample rate for output device")

    audio_engine.play_sample(0, 1.0)
    audio_engine.stop_all()
    audio_engine.unload_sample(0)


def test_sample_slot_id_range_is_0_to_215(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.load_sample_async(216, "does-not-matter.wav")

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.play_sample(216, 1.0)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.stop_sample(216)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.unload_sample(216)

    audio_engine.load_sample_async(215, "file-does-not-exist.wav")

    deadline = time.monotonic() + 1.0
    saw_error = False
    while time.monotonic() < deadline:
        event = audio_engine.poll_loader_events()
        if event is None:
            time.sleep(0.01)
            continue

        if event.get("type") == "error" and event.get("id") == 215:
            saw_error = True
            break

    assert saw_error

    # Shouldn't crash
    audio_engine.play_sample(215, 1.0)
    audio_engine.stop_sample(215)
    audio_engine.unload_sample(215)


def test_stop_all_requires_initialized_engine() -> None:
    try:
        engine = AudioEngine()
    except RuntimeError as exc:
        pytest.skip(f"AudioEngine unavailable: {exc}")

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.stop_all()


def test_stop_all_is_safe_when_nothing_playing(audio_engine: AudioEngine) -> None:
    audio_engine.stop_all()


def test_load_sample_async_emits_started_and_error_for_missing_file(
    audio_engine: AudioEngine,
) -> None:
    audio_engine.load_sample_async(0, "file-does-not-exist.wav")

    deadline = time.monotonic() + 1.0
    seen: dict[str, dict[str, object]] = {}

    while time.monotonic() < deadline and ("started" not in seen or "error" not in seen):
        event = audio_engine.poll_loader_events()
        if event is None:
            time.sleep(0.01)
            continue

        event_type = event.get("type")
        if isinstance(event_type, str):
            seen[event_type] = event

    assert "started" in seen
    assert seen["started"].get("id") == 0

    assert "error" in seen
    assert seen["error"].get("id") == 0
    assert isinstance(seen["error"].get("msg"), str)


def test_poll_loader_events_returns_none_when_empty(audio_engine: AudioEngine) -> None:
    # Ensure the queue is drained.
    deadline = time.monotonic() + 0.2
    while time.monotonic() < deadline and audio_engine.poll_loader_events() is not None:
        pass

    assert audio_engine.poll_loader_events() is None
