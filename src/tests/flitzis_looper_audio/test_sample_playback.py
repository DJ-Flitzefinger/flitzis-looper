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

        try:
            audio_engine.load_sample(0, str(wav_path))
        except ValueError as exc:
            if "sample rate mismatch" in str(exc):
                continue
            raise
        else:
            break
    else:
        pytest.skip("No matching sample rate for output device")

    audio_engine.play_sample(0, 1.0)
    audio_engine.stop_all()
    audio_engine.unload_sample(0)


def test_sample_slot_id_range_is_0_to_215(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.load_sample(216, "does-not-matter.wav")

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.play_sample(216, 1.0)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.stop_sample(216)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.unload_sample(216)

    # Shouldn't crash
    with pytest.raises(FileNotFoundError):
        audio_engine.load_sample(215, "file-does-not-exist.wav")
    audio_engine.play_sample(215, 1.0)
    audio_engine.stop_sample(215)
    audio_engine.unload_sample(215)


def test_stop_all_requires_initialized_engine() -> None:
    engine = AudioEngine()

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.stop_all()


def test_stop_all_is_safe_when_nothing_playing(audio_engine: AudioEngine) -> None:
    audio_engine.stop_all()
