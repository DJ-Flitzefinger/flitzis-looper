import wave
from array import array
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path


def write_mono_pcm16_wav(path: Path, sample_rate_hz: int) -> None:
    samples = array("h", [8192] * 128)

    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(sample_rate_hz)
        wav.writeframes(samples.tobytes())
