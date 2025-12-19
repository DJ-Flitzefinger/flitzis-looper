from __future__ import annotations

from flitzis_looper.app import FlitzisLooperApp
from flitzis_looper_rs import AudioEngine


def test_app_constructs_audio_engine() -> None:
    app = FlitzisLooperApp()
    assert isinstance(app.audio_engine, AudioEngine)
