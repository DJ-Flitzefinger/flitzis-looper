from __future__ import annotations


def _unavailable_error() -> RuntimeError:
    return RuntimeError(
        "flitzis_looper_audio extension module is unavailable for this Python runtime"
    )


_ext_available = True

try:
    import sys as _sys

    from .flitzis_looper_audio import *  # type: ignore[import-not-found]  # noqa: F403

    _mod = _sys.modules.get(f"{__name__}.flitzis_looper_audio")
    if _mod is not None:
        __doc__ = _mod.__doc__
        if hasattr(_mod, "__all__"):
            __all__ = _mod.__all__
except ModuleNotFoundError:
    _ext_available = False
except ImportError:
    _ext_available = False

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

        def unload_sample(self, sample_id: int) -> None:
            raise _unavailable_error()

        def ping(self) -> None:
            raise _unavailable_error()

        def receive_msg(self) -> AudioMessage | None:
            raise _unavailable_error()

    __all__ = ["AudioEngine", "AudioMessage"]
