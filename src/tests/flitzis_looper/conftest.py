import struct
import wave
from typing import TYPE_CHECKING
from unittest.mock import patch

import pytest

from flitzis_looper.controller import AppController
from flitzis_looper.controller.stem_generation import (
    StemGenerationRequest,
    StemGenerationResult,
)
from flitzis_looper.models import STEM_KINDS

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.models import ProjectState, SessionState


class FakeStemGenerationBackend:
    def __init__(self) -> None:
        self.requests: list[StemGenerationRequest] = []
        self.result = StemGenerationResult(
            backend_name="fake",
            model_name="fake-model",
            device="cpu",
            cpu_fallback=False,
            artifact_count=len(STEM_KINDS),
        )
        self.error: RuntimeError | None = None

    def generate(
        self,
        request: StemGenerationRequest,
        progress: Callable[[float, str], None],
    ) -> StemGenerationResult:
        self.requests.append(request)
        progress(0.5, "Generating fake stems")
        if self.error is not None:
            raise self.error

        request.cache_dir.mkdir(parents=True, exist_ok=True)
        for kind in STEM_KINDS:
            _write_test_wav(
                request.cache_dir / f"{kind}.wav",
                request.target_shape.sample_rate_hz,
                request.target_shape.channels,
                request.target_shape.frame_count,
            )
        return self.result


def _write_test_wav(path: Path, sample_rate_hz: int, channels: int, frames: int) -> None:
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(2)
        wav.setframerate(sample_rate_hz)
        frame = b"".join(struct.pack("<h", 1024) for _ in range(channels))
        wav.writeframes(frame * frames)


@pytest.fixture
def stem_backend() -> FakeStemGenerationBackend:
    return FakeStemGenerationBackend()


@pytest.fixture
def audio_engine_mock() -> Iterator[Mock]:
    with patch("flitzis_looper.controller.app.AudioEngine", autospec=True) as audio_engine:
        audio_engine.return_value.output_sample_rate.return_value = 44_100
        if hasattr(audio_engine.return_value, "loaded_sample_shape"):
            audio_engine.return_value.loaded_sample_shape.return_value = (44_100, 1, 128)
        yield audio_engine.return_value


@pytest.fixture
def controller(
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> AppController:
    monkeypatch.chdir(tmp_path)

    return AppController(stem_backend=stem_backend, stem_task_runner=lambda target: target())


@pytest.fixture
def project_state(controller: AppController) -> ProjectState:
    return controller.project


@pytest.fixture
def session_state(controller: AppController) -> SessionState:
    return controller.session
