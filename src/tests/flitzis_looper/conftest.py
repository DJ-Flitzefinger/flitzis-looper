from typing import TYPE_CHECKING
from unittest.mock import patch

import pytest

from flitzis_looper.controller import AppController

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.models import ProjectState, SessionState


@pytest.fixture
def audio_engine_mock() -> Iterator[Mock]:
    with patch("flitzis_looper.controller.app.AudioEngine", autospec=True) as audio_engine:
        yield audio_engine.return_value


@pytest.fixture
def controller(
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> AppController:
    monkeypatch.chdir(tmp_path)

    return AppController()


@pytest.fixture
def project_state(controller: AppController) -> ProjectState:
    return controller.project


@pytest.fixture
def session_state(controller: AppController) -> SessionState:
    return controller.session
