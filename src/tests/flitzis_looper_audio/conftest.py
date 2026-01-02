from __future__ import annotations

from typing import TYPE_CHECKING

import pytest

from flitzis_looper_audio import AudioEngine

if TYPE_CHECKING:
    from collections.abc import Iterable


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
