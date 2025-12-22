from typing import TYPE_CHECKING
from unittest.mock import Mock, patch

import pytest

from flitzis_looper.app import FlitzisLooperApp

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper.state import AppState


@pytest.fixture
def audio_engine_mock() -> Iterator[Mock]:
    with patch("flitzis_looper.app.AudioEngine", autospec=True) as audio_engine:
        yield audio_engine


@pytest.fixture
def app(audio_engine_mock: Mock) -> FlitzisLooperApp:
    return FlitzisLooperApp()


@pytest.fixture
def state(app: FlitzisLooperApp) -> AppState:
    return app.state
