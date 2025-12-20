from typing import cast

from flitzis_looper.app import FlitzisLooperApp, pad_label_from_sample_path
from flitzis_looper_rs import AudioEngine


class FakeAudioEngine:
    def __init__(self) -> None:
        self.calls: list[tuple[str, tuple[object, ...]]] = []

    def load_sample(self, sample_id: int, path: str) -> None:
        self.calls.append(("load_sample", (sample_id, path)))

    def play_sample(self, sample_id: int, velocity: float) -> None:
        self.calls.append(("play_sample", (sample_id, velocity)))

    def stop_sample(self, sample_id: int) -> None:
        self.calls.append(("stop_sample", (sample_id,)))

    def stop_all(self) -> None:
        self.calls.append(("stop_all", ()))

    def set_speed(self, speed: float) -> None:
        self.calls.append(("set_speed", (speed,)))

    def unload_sample(self, sample_id: int) -> None:
        self.calls.append(("unload_sample", (sample_id,)))


def test_app_constructs_audio_engine() -> None:
    app = FlitzisLooperApp()
    assert isinstance(app.audio_engine, AudioEngine)


def test_pad_label_from_sample_path_uses_basename_only() -> None:
    assert pad_label_from_sample_path("/music/loops/kick.wav", 1) == "kick.wav"
    assert pad_label_from_sample_path(r"C:\\music\\loops\\snare.wav", 2) == "snare.wav"


def test_multi_loop_defaults_disabled() -> None:
    app = FlitzisLooperApp()
    assert app.multi_loop_enabled is False
    assert not app.active_sample_ids


def test_multiloop_can_be_enabled_and_disabled() -> None:
    app = FlitzisLooperApp()

    app.set_multi_loop_enabled(enabled=True)
    assert app.multi_loop_enabled is True

    app.set_multi_loop_enabled(enabled=False)
    assert app.multi_loop_enabled is False


def test_speed_defaults_to_one() -> None:
    app = FlitzisLooperApp()
    assert app.speed == 1.0


def test_set_speed_updates_state_and_calls_engine() -> None:
    app = FlitzisLooperApp()
    fake = FakeAudioEngine()
    app.audio_engine = cast("AudioEngine", fake)

    app.set_speed(1.5)
    app.set_speed(3.0)
    app.set_speed(0.1)

    assert fake.calls == [
        ("set_speed", (1.5,)),
        ("set_speed", (2.0,)),
        ("set_speed", (0.5,)),
    ]
    assert app.speed == 0.5


def test_reset_speed_sets_one_and_calls_engine() -> None:
    app = FlitzisLooperApp()
    fake = FakeAudioEngine()
    app.audio_engine = cast("AudioEngine", fake)

    app.set_speed(1.5)
    app.reset_speed()

    assert fake.calls == [("set_speed", (1.5,)), ("set_speed", (1.0,))]
    assert app.speed == 1.0


def test_trigger_unloaded_pad_has_no_effect() -> None:
    app = FlitzisLooperApp()
    fake = FakeAudioEngine()
    app.audio_engine = cast("AudioEngine", fake)

    app.sample_paths[0] = "loaded"
    app.active_sample_ids.add(0)

    app.trigger_pad(1)

    assert fake.calls == []
    assert app.active_sample_ids == {0}


def test_trigger_stops_other_pads_when_multiloop_disabled() -> None:
    app = FlitzisLooperApp()
    fake = FakeAudioEngine()
    app.audio_engine = cast("AudioEngine", fake)

    app.sample_paths[0] = "loaded"
    app.sample_paths[2] = "loaded"
    app.active_sample_ids.add(0)

    app.trigger_pad(2)

    assert fake.calls == [("stop_all", ()), ("play_sample", (2, 1.0))]
    assert app.active_sample_ids == {2}


def test_trigger_does_not_stop_other_pads_when_multiloop_enabled() -> None:
    app = FlitzisLooperApp()
    fake = FakeAudioEngine()
    app.audio_engine = cast("AudioEngine", fake)

    app.set_multi_loop_enabled(enabled=True)
    app.sample_paths[0] = "loaded"
    app.sample_paths[1] = "loaded"
    app.active_sample_ids.add(0)

    app.trigger_pad(1)

    assert fake.calls == [("stop_sample", (1,)), ("play_sample", (1, 1.0))]
    assert app.active_sample_ids == {0, 1}


def test_retrigger_in_multiloop_does_not_clear_other_active_pads() -> None:
    app = FlitzisLooperApp()
    fake = FakeAudioEngine()
    app.audio_engine = cast("AudioEngine", fake)

    app.set_multi_loop_enabled(enabled=True)
    app.sample_paths[0] = "loaded"
    app.sample_paths[1] = "loaded"
    app.active_sample_ids.update({0, 1})

    app.trigger_pad(0)

    assert fake.calls == [("stop_sample", (0,)), ("play_sample", (0, 1.0))]
    assert app.active_sample_ids == {0, 1}


def test_stop_pad_is_noop_when_inactive() -> None:
    app = FlitzisLooperApp()
    fake = FakeAudioEngine()
    app.audio_engine = cast("AudioEngine", fake)

    app.stop_pad(0)

    assert fake.calls == []


def test_stop_pad_clears_active_state() -> None:
    app = FlitzisLooperApp()
    fake = FakeAudioEngine()
    app.audio_engine = cast("AudioEngine", fake)

    app.active_sample_ids.add(0)

    app.stop_pad(0)

    assert fake.calls == [("stop_sample", (0,))]
    assert 0 not in app.active_sample_ids


def test_unload_sample_clears_active_state() -> None:
    app = FlitzisLooperApp()
    fake = FakeAudioEngine()
    app.audio_engine = cast("AudioEngine", fake)

    app.sample_paths[0] = "loaded"
    app.active_sample_ids.add(0)

    app.unload_sample(0)

    assert fake.calls == [("stop_sample", (0,)), ("unload_sample", (0,))]
    assert app.sample_paths[0] is None
    assert 0 not in app.active_sample_ids
