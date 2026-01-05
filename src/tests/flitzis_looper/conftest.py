from typing import TYPE_CHECKING
from unittest.mock import Mock, patch

import pytest

from flitzis_looper.controller import LooperController

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path

    from flitzis_looper.models import ProjectState, SessionState


@pytest.fixture
def audio_engine_mock() -> Iterator[Mock]:
    with patch("flitzis_looper.controller.facade.AudioEngine", autospec=True) as audio_engine:
        yield audio_engine


@pytest.fixture
def controller(
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> LooperController:
    monkeypatch.chdir(tmp_path)
    return LooperController()


@pytest.fixture
def project_state(controller: LooperController) -> ProjectState:
    return controller.project


@pytest.fixture
def session_state(controller: LooperController) -> SessionState:
    return controller.session
