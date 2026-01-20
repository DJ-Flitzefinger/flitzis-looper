from typing import TYPE_CHECKING
from unittest.mock import patch

from flitzis_looper.controller.transport.state import ApplyProjectState

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper.models import ProjectState, SessionState


def test_transport_controller_initialization(
    project_state: ProjectState,
    session_state: SessionState,
    audio_engine_mock: Mock,
    transport_controller: TransportController,
) -> None:
    """Test TransportController initializes all sub-controllers."""
    assert transport_controller._project is project_state
    assert transport_controller._session is session_state
    assert transport_controller._audio is audio_engine_mock
    assert transport_controller.bpm is not None
    assert transport_controller.global_params is not None
    assert transport_controller.loop is not None
    assert transport_controller.playback is not None
    assert transport_controller.pad is not None
    assert transport_controller.waveform is not None


def test_transport_controller_passes_references(
    transport_controller: TransportController,
) -> None:
    """Test TransportController passes self to sub-controllers."""
    assert transport_controller.bpm._transport is transport_controller
    assert transport_controller.global_params._transport is transport_controller
    assert transport_controller.loop._transport is transport_controller
    assert transport_controller.playback._transport is transport_controller
    assert transport_controller.pad._transport is transport_controller
    assert transport_controller.waveform._transport is transport_controller


def test_apply_project_state_to_audio(
    transport_controller: TransportController,
) -> None:
    """Test apply_project_state_to_audio calls ApplyProjectState instance method."""
    with patch.object(ApplyProjectState, "apply_project_state_to_audio") as mock_apply:
        transport_controller.apply_project_state_to_audio()
        mock_apply.assert_called_once()
