from typing import TYPE_CHECKING

import pytest

from flitzis_looper_rs import AudioEngine

if TYPE_CHECKING:
    from collections.abc import Iterable


@pytest.fixture
def audio_engine() -> Iterable[AudioEngine]:
    engine = AudioEngine()

    try:
        engine.run()
    except RuntimeError as exc:
        pytest.skip(f"AudioEngine unavailable: {exc}")

    yield engine
    engine.shut_down()
