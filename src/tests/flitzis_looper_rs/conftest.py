from collections.abc import Iterable

import pytest

from flitzis_looper_rs import AudioEngine


@pytest.fixture
def audio_engine() -> Iterable[AudioEngine]:
    engine = AudioEngine()

    try:
        engine.run()
    except RuntimeError as exc:
        pytest.skip(f"AudioEngine unavailable: {exc}")

    yield engine
    engine.shut_down()
