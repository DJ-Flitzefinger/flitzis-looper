from __future__ import annotations

from array import array
import wave

import pytest

from flitzis_looper_rs import AudioEngine


def _write_mono_pcm16_wav(path, sample_rate_hz: int) -> None:
    samples = array("h", [8192] * 128)

    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(sample_rate_hz)
        wav.writeframes(samples.tobytes())


def test_load_and_play_sample_smoke(audio_engine: AudioEngine, tmp_path) -> None:
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
