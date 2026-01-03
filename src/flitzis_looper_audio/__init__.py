from __future__ import annotations

import warnings


def _unavailable_error() -> RuntimeError:
    return RuntimeError(
        "flitzis_looper_audio extension module is unavailable for this Python runtime"
    )


def _warn_missing_api(name: str) -> None:
    warnings.warn(
        f"AudioEngine is missing '{name}'. Rebuild the extension with 'uv run maturin develop'.",
        RuntimeWarning,
        stacklevel=3,
    )


_ext_available = True

AudioEngine = None  # type: ignore[assignment]
AudioMessage = None  # type: ignore[assignment]

try:
    from .flitzis_looper_audio import AudioEngine, AudioMessage  # type: ignore[import-not-found]
except ModuleNotFoundError:
    _ext_available = False
except ImportError:
    _ext_available = False

if _ext_available and AudioEngine is not None:
    if not hasattr(AudioEngine, "set_pad_gain"):

        def set_pad_gain(self: AudioEngine, sample_id: int, gain: float) -> None:
            _warn_missing_api("set_pad_gain")

        AudioEngine.set_pad_gain = set_pad_gain  # type: ignore[method-assign]

    if not hasattr(AudioEngine, "set_pad_eq"):

        def set_pad_eq(
            self: AudioEngine,
            sample_id: int,
            low_db: float,
            mid_db: float,
            high_db: float,
        ) -> None:
            _warn_missing_api("set_pad_eq")

        AudioEngine.set_pad_eq = set_pad_eq  # type: ignore[method-assign]

    __all__ = ["AudioEngine", "AudioMessage"]

if not _ext_available:

    class AudioMessage:
        pass

    class AudioEngine:
        def __init__(self) -> None:
            raise _unavailable_error()

        def run(self) -> None:
            raise _unavailable_error()

        def shut_down(self) -> None:
            raise _unavailable_error()

        def load_sample_async(self, sample_id: int, path: str) -> None:
            raise _unavailable_error()

        def analyze_sample_async(self, sample_id: int) -> None:
            raise _unavailable_error()

        def poll_loader_events(self) -> dict[str, object] | None:
            raise _unavailable_error()

        def play_sample(self, sample_id: int, volume: float) -> None:
            raise _unavailable_error()

        def stop_sample(self, sample_id: int) -> None:
            raise _unavailable_error()

        def stop_all(self) -> None:
            raise _unavailable_error()

        def set_volume(self, volume: float) -> None:
            raise _unavailable_error()

        def set_speed(self, speed: float) -> None:
            raise _unavailable_error()

        def set_pad_gain(self, sample_id: int, gain: float) -> None:
            raise _unavailable_error()

        def set_pad_eq(self, sample_id: int, low_db: float, mid_db: float, high_db: float) -> None:
            raise _unavailable_error()

        def unload_sample(self, sample_id: int) -> None:
            raise _unavailable_error()

        def ping(self) -> None:
            raise _unavailable_error()

        def receive_msg(self) -> AudioMessage | None:
            raise _unavailable_error()

    __all__ = ["AudioEngine", "AudioMessage"]
