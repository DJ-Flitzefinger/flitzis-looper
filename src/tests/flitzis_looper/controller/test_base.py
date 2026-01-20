from typing import TYPE_CHECKING
from unittest.mock import Mock

from flitzis_looper.controller.base import BaseController

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState, SessionState


def test_base_controller_initialization(
    project_state: ProjectState, session_state: SessionState, audio_engine_mock: Mock
) -> None:
    """Test BaseController stores constructor parameters correctly."""
    on_project_changed_mock = Mock()

    controller = BaseController(
        project_state, session_state, audio_engine_mock, on_project_changed_mock
    )

    assert controller._project is project_state
    assert controller._session is session_state
    assert controller._audio is audio_engine_mock
    assert controller._on_project_changed is on_project_changed_mock
    assert len(controller._on_frame_render_callbacks) == 0


def test_output_sample_rate_hz_success(
    audio_engine_mock: Mock, project_state: ProjectState, session_state: SessionState
) -> None:
    """Test _output_sample_rate_hz returns int when audio engine provides it."""
    audio_engine_mock.output_sample_rate.return_value = 48000

    controller = BaseController(project_state, session_state, audio_engine_mock)

    result = controller._output_sample_rate_hz()

    assert result == 48000


def test_output_sample_rate_hz_none(
    audio_engine_mock: Mock, project_state: ProjectState, session_state: SessionState
) -> None:
    """Test _output_sample_rate_hz returns None when output_sample_rate method not available."""
    delattr(audio_engine_mock, "output_sample_rate")

    controller = BaseController(project_state, session_state, audio_engine_mock)

    result = controller._output_sample_rate_hz()

    assert result is None


def test_output_sample_rate_hz_handles_runtime_error(
    audio_engine_mock: Mock, project_state: ProjectState, session_state: SessionState
) -> None:
    """Test _output_sample_rate_hz returns None when RuntimeError is raised."""
    audio_engine_mock.output_sample_rate.side_effect = RuntimeError("Audio engine not running")

    controller = BaseController(project_state, session_state, audio_engine_mock)

    result = controller._output_sample_rate_hz()

    assert result is None


def test_output_sample_rate_hz_handles_type_error(
    audio_engine_mock: Mock, project_state: ProjectState, session_state: SessionState
) -> None:
    """Test _output_sample_rate_hz returns None when TypeError is raised."""
    audio_engine_mock.output_sample_rate.side_effect = TypeError("Invalid return type")

    controller = BaseController(project_state, session_state, audio_engine_mock)

    result = controller._output_sample_rate_hz()

    assert result is None


def test_output_sample_rate_hz_handles_value_error(
    audio_engine_mock: Mock, project_state: ProjectState, session_state: SessionState
) -> None:
    """Test _output_sample_rate_hz returns None when ValueError is raised."""
    audio_engine_mock.output_sample_rate.side_effect = ValueError("Invalid value")

    controller = BaseController(project_state, session_state, audio_engine_mock)

    result = controller._output_sample_rate_hz()

    assert result is None


def test_mark_project_changed_with_callback(
    audio_engine_mock: Mock, project_state: ProjectState, session_state: SessionState
) -> None:
    """Test _mark_project_changed calls the on_project_changed when it exists."""
    on_project_changed_mock = Mock()

    controller = BaseController(
        project_state, session_state, audio_engine_mock, on_project_changed_mock
    )

    controller._mark_project_changed()

    on_project_changed_mock.assert_called_once()


def test_mark_project_changed_without_callback(
    audio_engine_mock: Mock, project_state: ProjectState, session_state: SessionState
) -> None:
    """Test _mark_project_changed does nothing when on_project_changed is None."""
    controller = BaseController(
        project_state, session_state, audio_engine_mock, on_project_changed=None
    )

    controller._mark_project_changed()

    assert controller._on_project_changed is None
