from typing import TYPE_CHECKING

from flitzis_looper.constants import NUM_SAMPLES

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def test_controller_initializes_states(controller: AppController) -> None:
    assert controller.project is not None
    assert controller.session is not None
    assert len(controller.project.sample_paths) == NUM_SAMPLES


def test_controller_initializes_audio_engine(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.assert_called_once()
    audio_engine_mock.return_value.run.assert_called_once()
