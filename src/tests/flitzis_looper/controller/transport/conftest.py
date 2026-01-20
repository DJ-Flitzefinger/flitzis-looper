from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.transport import TransportController

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.models import ProjectState, SessionState


@pytest.fixture
def transport_controller(
    project_state: ProjectState, session_state: SessionState, audio_engine_mock: Mock
) -> TransportController:
    return TransportController(project_state, session_state, audio_engine_mock)
