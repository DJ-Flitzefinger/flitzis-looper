from typing import TYPE_CHECKING
from unittest.mock import Mock, patch

import pytest

from flitzis_looper.controller import AppController

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path

    from flitzis_looper.models import ProjectState, SessionState


@pytest.fixture
def audio_engine_mock() -> Iterator[Mock]:
    with patch("flitzis_looper.controller.AudioEngine", autospec=True) as audio_engine:
        yield audio_engine


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
