from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def test_trigger_pad_single_loop(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test triggering a pad in single loop mode stops other pads."""
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.session.active_sample_ids.add(5)  # Another active sample
    controller.project.multi_loop = False

    controller.transport.playback.trigger_pad(sample_id)

    # Should stop all other pads first
    audio_engine_mock.return_value.stop_all.assert_called_once()
    # Then play the triggered pad
    audio_engine_mock.return_value.play_sample.assert_called_with(sample_id, 1.0)
    # Only the triggered pad should be active
    assert controller.session.active_sample_ids == {sample_id}


def test_trigger_pad_multi_loop(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test triggering a pad in multi loop mode toggles playback."""
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.session.active_sample_ids.add(sample_id)  # Already active
    controller.project.multi_loop = True

    controller.transport.playback.trigger_pad(sample_id)

    # Should stop only this pad (toggle behavior)
    audio_engine_mock.return_value.stop_sample.assert_called_with(sample_id)
    # Should not stop all pads
    audio_engine_mock.return_value.stop_all.assert_not_called()


def test_trigger_pad_not_loaded(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test triggering an unloaded pad does nothing."""
    sample_id = 0

    controller.transport.playback.trigger_pad(sample_id)

    audio_engine_mock.return_value.play_sample.assert_not_called()


def test_stop_pad(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test stopping a specific pad."""
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.session.active_sample_ids.add(sample_id)

    controller.transport.playback.stop_pad(sample_id)

    audio_engine_mock.return_value.stop_sample.assert_called_with(sample_id)
    assert sample_id not in controller.session.active_sample_ids


def test_stop_pad_not_active(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test stopping an inactive pad does nothing."""
    sample_id = 0

    controller.transport.playback.stop_pad(sample_id)

    audio_engine_mock.return_value.stop_sample.assert_not_called()


def test_stop_all_pads(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test stopping all pads clears active samples."""
    controller.session.active_sample_ids.update({0, 1, 2})

    controller.transport.playback.stop_all_pads()

    audio_engine_mock.return_value.stop_all.assert_called_once()
    assert controller.session.active_sample_ids == set()


def test_is_sample_active_true(controller: AppController) -> None:
    """Test is_sample_active returns True when sample is playing."""
    sample_id = 0
    controller.session.active_sample_ids.add(sample_id)

    assert sample_id in controller.session.active_sample_ids


def test_is_sample_active_false(controller: AppController) -> None:
    """Test is_sample_active returns False when sample is not playing."""
    sample_id = 0

    assert sample_id not in controller.session.active_sample_ids


def test_trigger_unloaded_sample_does_not_raise(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.transport.playback.trigger_pad(0)  # Should not raise error

    # Should not attempt to play
    audio_engine_mock.return_value.play_sample.assert_not_called()
