from typing import TYPE_CHECKING

import pytest

from flitzis_looper_audio import AudioEngine

if TYPE_CHECKING:
    from collections.abc import Iterable


@pytest.fixture
def uninitialized_audio_engine() -> Iterable[AudioEngine]:
    """Exercise real native validation and polling without opening a device."""
    engine = AudioEngine()
    try:
        yield engine
    finally:
        engine.shut_down()


@pytest.fixture
def audio_engine() -> Iterable[AudioEngine]:
    engine: AudioEngine | None = None
    try:
        engine = AudioEngine()
        engine.run()
    except RuntimeError as exc:
        pytest.skip(f"AudioEngine unavailable: {exc}")
    else:
        yield engine
    finally:
        if engine is not None:
            engine.shut_down()
