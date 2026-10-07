import wave
from array import array
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from pathlib import Path


def pytest_addoption(parser: pytest.Parser) -> None:
    """Require deliberate human opt-in before a test opens an audio device."""
    parser.addoption("--audio-devices", action="store_true", help="Run real audio device tests")


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    """Keep full offline validation safe when no hardware session is authorized."""
    for item in items:
        if "audio_engine" in getattr(item, "fixturenames", ()):
            item.add_marker(pytest.mark.audio_device)
        if item.get_closest_marker("audio_device") is not None and not config.getoption(
            "--audio-devices"
        ):
            item.add_marker(pytest.mark.skip(reason="Real audio devices require --audio-devices"))


def write_mono_pcm16_wav(path: Path, sample_rate_hz: int) -> None:
    samples = array("h", [8192] * 128)

    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(sample_rate_hz)
        wav.writeframes(samples.tobytes())
