from __future__ import annotations

from flitzis_looper_rs import AudioEngine


class FlitzisLooperApp:
    """Application logic stub for Flitzis Looper.

    This is currently a minimal shell that only constructs the Rust-backed
    `AudioEngine`. It intentionally does not call `AudioEngine.run()`.
    """

    def __init__(self) -> None:
        self.audio_engine: AudioEngine = AudioEngine()
