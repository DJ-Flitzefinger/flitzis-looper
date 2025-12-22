from typing import TYPE_CHECKING

import pytest

from flitzis_looper_rs import AudioEngine

if TYPE_CHECKING:
    from collections.abc import Iterable


@pytest.fixture
def audio_engine() -> Iterable[AudioEngine]:
    try:
        engine = AudioEngine()
        engine.run()
    except RuntimeError as exc:
        pytest.fail(f"AudioEngine unavailable: {exc}")
    else:
        yield engine
    finally:
        engine.shut_down()
