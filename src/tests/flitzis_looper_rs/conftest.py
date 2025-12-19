from collections.abc import Iterable

import pytest

from flitzis_looper_rs import AudioEngine


@pytest.fixture
def audio_engine() -> Iterable[AudioEngine]:
    engine = AudioEngine()
    engine.run()
    yield engine
