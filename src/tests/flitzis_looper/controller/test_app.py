from typing import TYPE_CHECKING
from unittest.mock import patch

from flitzis_looper.constants import NUM_SAMPLES

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def test_controller_initializes_states(controller: AppController) -> None:
    """Test AppController creates all components during initialization."""
    assert controller.project is not None
    assert controller.session is not None
    assert controller.transport is not None
    assert controller.loader is not None
    assert controller.metering is not None
    assert len(controller.project.sample_paths) == NUM_SAMPLES


def test_controller_creates_audio_engine(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test AppController creates and runs audio engine during initialization."""
    assert controller._audio == audio_engine_mock
    audio_engine_mock.run.assert_called_once()


def test_controller_applies_project_state(controller: AppController) -> None:
    """Test AppController applies project state to audio during initialization."""
    assert controller.transport.apply_project_state_to_audio is not None


def test_controller_restores_samples(controller: AppController) -> None:
    """Test AppController restores samples from project during initialization."""
    assert controller.loader.restore_samples_from_project_state is not None


def test_controller_shut_down_flushes_persistence(controller: AppController) -> None:
    """Test AppController flushes persistence on shutdown."""
    with patch.object(controller._persistence, "flush") as mock_flush:
        controller.shut_down()
        mock_flush.assert_called_once()


def test_controller_shut_down_stops_audio(controller: AppController) -> None:
    """Test AppController stops all audio on shutdown."""
    assert hasattr(controller._audio, "stop_all")
    controller.shut_down()


def test_controller_shut_down_shuts_down_audio(controller: AppController) -> None:
    """Test AppController shuts down audio engine on shutdown."""
    assert hasattr(controller._audio, "shut_down")
    controller.shut_down()


def test_controller_shut_down_suppresses_os_error(controller: AppController) -> None:
    """Test AppController suppresses OSError during persistence flush on shutdown."""
    with patch.object(controller._persistence, "flush", side_effect=OSError("File not found")):
        controller.shut_down()


def test_controller_project_property(controller: AppController) -> None:
    """Test AppController.project property returns project state."""
    assert controller.project is controller._project


def test_controller_session_property(controller: AppController) -> None:
    """Test AppController.session property returns session state."""
    assert controller.session is controller._session


def test_controller_persistence_property(controller: AppController) -> None:
    """Test AppController.persistence property returns persistence instance."""
    assert controller.persistence is controller._persistence


def test_controller_on_frame_render_calls_all_controllers(
    controller: AppController,
) -> None:
    """Test AppController.on_frame_render calls on_frame_render on all controllers."""
    with (
        patch.object(controller.transport, "on_frame_render") as mock_transport,
        patch.object(controller.loader, "on_frame_render") as mock_loader,
        patch.object(controller.metering, "on_frame_render") as mock_metering,
    ):
        controller.on_frame_render()

        mock_transport.assert_called_once()
        mock_loader.assert_called_once()
        mock_metering.assert_called_once()


def test_controller_registers_controllers(controller: AppController) -> None:
    """Test AppController registers all controllers for on_frame_render."""
    assert len(controller._controllers) == 3
