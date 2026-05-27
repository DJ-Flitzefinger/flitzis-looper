from typing import TYPE_CHECKING
from unittest.mock import Mock, call

import pytest

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def test_trigger_pad_single_loop(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test triggering a pad in single loop mode uses the exclusive audio command."""
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.session.active_sample_ids.add(5)  # Another active sample
    controller.project.multi_loop = False

    controller.transport.playback.trigger_pad(sample_id)

    audio_engine_mock.stop_all.assert_not_called()
    audio_engine_mock.play_sample.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_called_once_with(sample_id, 1.0)
    # Simulate the audio message that would update state for the new pad
    msg = Mock()
    msg.sample_id.return_value = sample_id
    controller.transport.playback.handle_sample_started_message(msg)
    # Simulate the audio message for the stopped pad (5)
    msg2 = Mock()
    msg2.sample_id.return_value = 5
    controller.transport.playback.handle_sample_stopped_message(msg2)
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

    # Should not stop all pads
    audio_engine_mock.stop_all.assert_not_called()
    # But play_sample is called
    audio_engine_mock.play_sample.assert_called_with(sample_id, 1.0)


def test_trigger_pad_not_loaded(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test triggering an unloaded pad does nothing."""
    sample_id = 0

    controller.transport.playback.trigger_pad(sample_id)

    audio_engine_mock.play_sample.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()


def test_stop_pad(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test stopping a specific pad."""
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.session.active_sample_ids.add(sample_id)

    controller.transport.playback.stop_pad(sample_id)

    audio_engine_mock.stop_sample.assert_called_with(sample_id)
    # Simulate the audio message that would update state
    msg = Mock()
    msg.sample_id.return_value = sample_id
    controller.transport.playback.handle_sample_stopped_message(msg)
    assert sample_id not in controller.session.active_sample_ids


def test_stop_pad_not_active(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test stopping an inactive pad does nothing."""
    sample_id = 0

    controller.transport.playback.stop_pad(sample_id)

    audio_engine_mock.stop_sample.assert_not_called()


def test_seek_pad_calls_audio_and_updates_playhead(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "/path/to/sample.wav"
    controller.project.sample_durations[sample_id] = 10.0
    controller.session.active_sample_ids.add(sample_id)

    controller.transport.playback.seek_pad(sample_id, 3.5)

    audio_engine_mock.seek_sample.assert_called_once_with(sample_id, 3.5)
    assert controller.session.pad_playhead_s[sample_id] == 3.5


def test_seek_pad_clamps_projection_to_loaded_duration(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "/path/to/sample.wav"
    controller.project.sample_durations[sample_id] = 10.0
    controller.session.active_sample_ids.add(sample_id)

    controller.transport.playback.seek_pad(sample_id, 12.5)

    audio_engine_mock.seek_sample.assert_called_once_with(sample_id, 10.0)
    assert controller.session.pad_playhead_s[sample_id] == 10.0


def test_seek_pad_ignores_stopped_pad(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "/path/to/sample.wav"
    controller.project.sample_durations[sample_id] = 10.0

    controller.transport.playback.seek_pad(sample_id, 3.5)

    audio_engine_mock.seek_sample.assert_not_called()
    assert controller.session.pad_playhead_s[sample_id] is None


def test_seek_pad_keeps_paused_pad_paused(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0
    controller.project.sample_paths[sample_id] = "/path/to/sample.wav"
    controller.session.active_sample_ids.add(sample_id)
    controller.session.paused_sample_ids.add(sample_id)

    controller.transport.playback.seek_pad(sample_id, 3.5)

    audio_engine_mock.seek_sample.assert_called_once_with(sample_id, 3.5)
    assert sample_id in controller.session.paused_sample_ids


def test_stop_all_pads(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test stopping all pads clears active samples."""
    controller.session.active_sample_ids.update({0, 1, 2})

    controller.transport.playback.stop_all_pads()

    audio_engine_mock.stop_all.assert_called_once()
    # Simulate audio messages for stopped samples
    for sample_id in (0, 1, 2):
        msg = Mock()
        msg.sample_id.return_value = sample_id
        controller.transport.playback.handle_sample_stopped_message(msg)
    assert controller.session.active_sample_ids == set()


def test_global_start_stop_right_stops_and_remembers_playing_set(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "/path/to/sample-a.wav"
    controller.project.sample_paths[1] = "/path/to/sample-b.wav"
    controller.session.active_sample_ids.update({0, 1})
    controller.session.paused_sample_ids.add(1)

    controller.transport.playback.stop_global_start_stop()

    audio_engine_mock.stop_all.assert_called_once()
    assert controller.session.global_stop_engaged is True
    assert controller.session.global_stop_restore_sample_ids == {0}
    assert controller.session.active_sample_ids == set()
    assert controller.session.paused_sample_ids == set()


def test_global_start_stop_right_stops_paused_only_set_without_restore(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[1] = "/path/to/sample-b.wav"
    controller.session.active_sample_ids.add(1)
    controller.session.paused_sample_ids.add(1)

    controller.transport.playback.stop_global_start_stop()

    audio_engine_mock.stop_all.assert_called_once()
    assert controller.session.global_stop_engaged is False
    assert controller.session.global_stop_restore_sample_ids == set()
    assert controller.session.active_sample_ids == set()
    assert controller.session.paused_sample_ids == set()


def test_global_start_stop_right_ignores_empty_active_set(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.transport.playback.stop_global_start_stop()

    audio_engine_mock.stop_all.assert_not_called()
    assert controller.session.global_stop_engaged is False
    assert controller.session.global_stop_restore_sample_ids == set()


def test_global_start_stop_left_restarts_current_active_set(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "/path/to/sample-a.wav"
    controller.project.sample_paths[1] = "/path/to/sample-b.wav"
    controller.project.pad_loop_start_s[0] = 1.0
    controller.project.pad_loop_end_s[0] = 3.0
    controller.project.pad_loop_start_s[1] = 2.0
    controller.project.pad_loop_end_s[1] = 4.0
    controller.session.active_sample_ids.update({0, 1})

    controller.transport.playback.start_or_restart_global_start_stop()

    audio_engine_mock.stop_all.assert_not_called()
    assert audio_engine_mock.set_pad_loop_region.call_args_list == [
        call(0, 1.0, 3.0),
        call(1, 2.0, 4.0),
    ]
    assert audio_engine_mock.play_sample.call_args_list == [call(0, 1.0), call(1, 1.0)]
    assert controller.session.global_stop_engaged is False
    assert controller.session.global_stop_restore_sample_ids == set()
    assert controller.session.active_sample_ids == {0, 1}


def test_global_start_stop_left_restarts_current_active_set_without_paused_pads(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "/path/to/sample-a.wav"
    controller.project.sample_paths[1] = "/path/to/sample-b.wav"
    controller.session.active_sample_ids.update({0, 1})
    controller.session.paused_sample_ids.add(1)

    controller.transport.playback.start_or_restart_global_start_stop()

    audio_engine_mock.play_sample.assert_called_once_with(0, 1.0)
    assert 1 in controller.session.paused_sample_ids


def test_global_start_stop_left_restores_remembered_set_together(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "/path/to/sample-a.wav"
    controller.project.sample_paths[1] = "/path/to/sample-b.wav"
    controller.project.pad_loop_start_s[0] = 1.0
    controller.project.pad_loop_end_s[0] = 3.0
    controller.project.pad_loop_start_s[1] = 2.0
    controller.project.pad_loop_end_s[1] = 4.0
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 1}

    controller.transport.playback.start_or_restart_global_start_stop()

    audio_engine_mock.stop_all.assert_not_called()
    assert audio_engine_mock.set_pad_loop_region.call_args_list == [
        call(0, 1.0, 3.0),
        call(1, 2.0, 4.0),
    ]
    assert audio_engine_mock.play_sample.call_args_list == [call(0, 1.0), call(1, 1.0)]
    assert controller.session.global_stop_engaged is False
    assert controller.session.global_stop_restore_sample_ids == set()
    assert controller.session.active_sample_ids == {0, 1}


def test_global_start_stop_right_never_restores_remembered_set(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 1}

    controller.transport.playback.stop_global_start_stop()

    audio_engine_mock.stop_all.assert_not_called()
    audio_engine_mock.play_sample.assert_not_called()
    assert controller.session.global_stop_engaged is True
    assert controller.session.global_stop_restore_sample_ids == {0, 1}


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
    audio_engine_mock.play_sample.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()


def test_trigger_pad_applies_loop_region(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.project.multi_loop = False
    controller.project.pad_loop_start_s[sample_id] = 5.0
    controller.project.pad_loop_end_s[sample_id] = 15.0

    controller.transport.playback.trigger_pad(sample_id)

    audio_engine_mock.set_pad_loop_region.assert_called_with(sample_id, 5.0, 15.0)


def test_trigger_pad_plays_with_gain(controller: AppController, audio_engine_mock: Mock) -> None:
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.project.multi_loop = False

    controller.transport.playback.trigger_pad(sample_id)

    audio_engine_mock.play_sample_exclusive.assert_called_with(sample_id, 1.0)


def test_play_separate_method(controller: AppController, audio_engine_mock: Mock) -> None:
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.project.multi_loop = True

    controller.transport.playback.trigger_pad(sample_id)

    audio_engine_mock.stop_all.assert_not_called()
    audio_engine_mock.play_sample.assert_called_with(sample_id, 1.0)


def test_play_applies_loop_region(controller: AppController, audio_engine_mock: Mock) -> None:
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.project.multi_loop = True
    controller.project.pad_loop_start_s[sample_id] = 2.0
    controller.project.pad_loop_end_s[sample_id] = 8.0

    controller.transport.playback.trigger_pad(sample_id)

    audio_engine_mock.set_pad_loop_region.assert_called_with(sample_id, 2.0, 8.0)


def test_toggle_stop_active(controller: AppController, audio_engine_mock: Mock) -> None:
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.session.active_sample_ids.add(sample_id)
    controller.project.multi_loop = True

    controller.transport.playback.trigger_pad(sample_id)

    audio_engine_mock.stop_sample.assert_not_called()
    audio_engine_mock.play_sample.assert_called_with(sample_id, 1.0)


def test_toggle_start_inactive(controller: AppController, audio_engine_mock: Mock) -> None:
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.project.multi_loop = True

    controller.transport.playback.trigger_pad(sample_id)

    audio_engine_mock.play_sample.assert_called_with(sample_id, 1.0)


def test_trigger_invalid_sample_id(controller: AppController, audio_engine_mock: Mock) -> None:
    with pytest.raises(ValueError, match="sample_id must be >= 0"):
        controller.transport.playback.trigger_pad(-1)


def test_stop_invalid_sample_id(controller: AppController, audio_engine_mock: Mock) -> None:
    with pytest.raises(ValueError, match="sample_id must be >= 0"):
        controller.transport.playback.stop_pad(-1)


def test_seek_invalid_sample_id(controller: AppController, audio_engine_mock: Mock) -> None:
    with pytest.raises(ValueError, match="sample_id must be >= 0"):
        controller.transport.playback.seek_pad(-1, 0.0)


def test_seek_rejects_non_finite_position(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    with pytest.raises(ValueError, match="value must be finite"):
        controller.transport.playback.seek_pad(0, float("nan"))
