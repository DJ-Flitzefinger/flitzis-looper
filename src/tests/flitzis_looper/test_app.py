from typing import TYPE_CHECKING, cast
from unittest.mock import call

from flitzis_looper_rs import AudioEngine

if TYPE_CHECKING:
    from unittest.mock import MagicMock

    from flitzis_looper.app import FlitzisLooperApp


def test_app_constructs_audio_engine(app: FlitzisLooperApp) -> None:
    assert isinstance(app.audio_engine, AudioEngine)


def test_pad_label(app: FlitzisLooperApp) -> None:
    app.state.sample_paths[0] = "/music/loops/kick.wav"
    app.state.sample_paths[1] = r"C:\\music\\loops\\snare.wav"
    assert app.pad_label(0) == "kick.wav"
    assert app.pad_label(1) == "snare.wav"


def test_multiloop_can_be_enabled_and_disabled(app: FlitzisLooperApp) -> None:
    app.set_multi_loop(enabled=True)
    assert app.state.multi_loop is True

    app.set_multi_loop(enabled=False)
    assert app.state.multi_loop is False


def test_set_speed_updates_state_and_calls_engine(app: FlitzisLooperApp) -> None:
    app.set_speed(1.5)
    app.set_speed(3.0)
    app.set_speed(0.1)

    set_speed = cast("MagicMock", app.audio_engine.set_speed)
    set_speed.assert_has_calls([call(1.5), call(2.0), call(0.5)])
    assert set_speed.call_count == 3


def test_reset_speed_sets_one_and_calls_engine(app: FlitzisLooperApp) -> None:
    app.set_speed(1.5)
    app.reset_speed()

    set_speed = cast("MagicMock", app.audio_engine.set_speed)
    set_speed.assert_has_calls([call(1.5), call(1.0)])
    assert app.state.speed == 1.0


def test_trigger_unloaded_pad_has_no_effect(app: FlitzisLooperApp) -> None:
    app.state.sample_paths[0] = "/path/kick.wav"
    app.state.active_sample_ids.add(0)

    app.trigger_pad(1)

    play_sample = cast("MagicMock", app.audio_engine.play_sample)
    assert play_sample.call_count == 0
    assert app.state.active_sample_ids == {0}


def test_trigger_stops_other_pads_when_multiloop_disabled(app: FlitzisLooperApp) -> None:
    app.state.sample_paths[0] = "loaded"
    app.state.sample_paths[2] = "loaded"
    app.state.active_sample_ids.add(0)

    app.trigger_pad(2)

    stop_all = cast("MagicMock", app.audio_engine.stop_all)
    play_sample = cast("MagicMock", app.audio_engine.play_sample)
    stop_all.assert_called_once()
    play_sample.assert_called_once_with(2, 1.0)
    assert app.state.active_sample_ids == {2}


def test_trigger_does_not_stop_other_pads_when_multiloop_enabled(app: FlitzisLooperApp) -> None:
    app.set_multi_loop(enabled=True)
    app.state.sample_paths[0] = "loaded"
    app.state.sample_paths[1] = "loaded"
    app.state.active_sample_ids.add(0)

    app.trigger_pad(1)

    stop_sample = cast("MagicMock", app.audio_engine.stop_sample)
    play_sample = cast("MagicMock", app.audio_engine.play_sample)
    stop_sample.assert_called_once_with(1)
    play_sample.assert_called_once_with(1, 1.0)
    assert app.state.active_sample_ids == {0, 1}


def test_retrigger_in_multiloop_does_not_clear_other_active_pads(app: FlitzisLooperApp) -> None:
    app.set_multi_loop(enabled=True)
    app.state.sample_paths[0] = "loaded"
    app.state.sample_paths[1] = "loaded"
    app.state.active_sample_ids.update({0, 1})

    app.trigger_pad(0)

    stop_sample = cast("MagicMock", app.audio_engine.stop_sample)
    play_sample = cast("MagicMock", app.audio_engine.play_sample)
    stop_sample.assert_called_once_with(0)
    play_sample.assert_called_once_with(0, 1.0)
    assert app.state.active_sample_ids == {0, 1}


def test_stop_pad_is_noop_when_inactive(app: FlitzisLooperApp) -> None:
    app.stop_pad(0)

    stop_sample = cast("MagicMock", app.audio_engine.stop_sample)
    stop_sample.assert_not_called()


def test_stop_pad_clears_active_state(app: FlitzisLooperApp) -> None:
    app.state.active_sample_ids.add(0)

    app.stop_pad(0)

    stop_sample = cast("MagicMock", app.audio_engine.stop_sample)
    stop_sample.assert_called_once_with(0)
    assert 0 not in app.state.active_sample_ids


def test_unload_sample_clears_active_state(app: FlitzisLooperApp) -> None:
    app.state.sample_paths[0] = "loaded"
    app.state.active_sample_ids.add(0)

    app.unload_sample(0)

    stop_sample = cast("MagicMock", app.audio_engine.stop_sample)
    unload_sample = cast("MagicMock", app.audio_engine.unload_sample)
    stop_sample.assert_called_once_with(0)
    unload_sample.assert_called_once_with(0)
    assert app.state.sample_paths[0] is None
    assert 0 not in app.state.active_sample_ids
