import json
import struct
import time
from threading import Event
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis.contracts import (
    BeatComponentResult,
    BeatModelIdentity,
    BeatWorkerRequest,
)
from flitzis_looper.analysis.jobs import OfflineAnalysisService
from flitzis_looper.analysis.worker import BeatWorkerAdapter
from tests.conftest import write_mono_pcm16_wav

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper_audio import AudioEngine


def _wait_event(engine: AudioEngine, event_type: str) -> dict[str, object]:
    deadline = time.monotonic() + 3.0
    while time.monotonic() < deadline:
        event = engine.poll_loader_events()
        if event is not None and event.get("type") == event_type:
            return event
        if event is not None and event.get("type") == "error":
            pytest.fail(f"native loading failed: {event}")
        time.sleep(0.01)
    pytest.fail(f"timed out waiting for native event {event_type}")


class _InspectingMissingAdapter(BeatWorkerAdapter):
    def __init__(self) -> None:
        super().__init__()
        self.pcm: tuple[float, ...] = ()
        self.request: BeatWorkerRequest | None = None

    def run(self, request: BeatWorkerRequest, cancel: Event) -> BeatComponentResult:
        self.request = request
        pcm = request.pcm.path.read_bytes()
        self.pcm = struct.unpack(f"<{request.pcm.frame_count}f", pcm)
        return super().run(request, cancel)


class _HoldingAdapter:
    def __init__(self) -> None:
        self.started = Event()
        self.cancel_seen = Event()
        self.release = Event()
        self.retired = Event()

    def run(self, request: BeatWorkerRequest, cancel: Event) -> BeatComponentResult:
        self.started.set()
        assert cancel.wait(5), "native unload did not invalidate adapter request"
        self.cancel_seen.set()
        assert self.release.wait(5), "test did not release retiring adapter"
        self.retired.set()
        return BeatComponentResult(
            identity=request.identity,
            model=request.model,
            status="cancelled",
            reason="source replaced",
        )


def test_native_offline_analysis_reuses_pcm_after_source_files_are_removed(
    audio_engine: AudioEngine, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    source = tmp_path / "source.wav"
    rate = audio_engine.output_sample_rate()
    write_mono_pcm16_wav(source, rate)
    audio_engine.load_sample_async(0, str(source), run_analysis=False)
    loaded = _wait_event(audio_engine, "success")
    cached = loaded["cached_path"]
    assert isinstance(cached, str)
    cached_path = (tmp_path / cached).resolve()
    assert cached_path.is_relative_to(tmp_path.resolve())
    source.unlink()
    cached_path.unlink()
    original_shape = audio_engine.loaded_sample_shape(0)
    adapter = _InspectingMissingAdapter()

    job = OfflineAnalysisService().start(
        audio_engine,
        0,
        tmp_path / "analysis",
        model=BeatModelIdentity(),
        adapter=adapter,
    )

    assert job.done.wait(5)
    event = _wait_event(audio_engine, "offline_analysis_completed")
    payload = event["result_json"]
    assert isinstance(payload, str)
    result = json.loads(payload)
    assert result["beat"]["status"] == "unavailable"
    assert result["key"]["status"] == "failed"
    assert result["key"]["key"] == "unknown"
    assert "InsufficientData" in result["key"]["detail"]
    assert adapter.request is not None
    assert adapter.request.pcm.sample_rate_hz == rate
    assert adapter.request.pcm.frame_count == 128
    assert adapter.request.pcm.origin_seconds == 0.0
    assert adapter.pcm == (0.25,) * 128
    assert audio_engine.loaded_sample_shape(0) == original_shape
    audio_engine.play_sample(0, 1.0)
    audio_engine.stop_all()
    assert list((tmp_path / "analysis").iterdir()) == []


def test_native_source_replacement_retains_busy_slot_and_rejects_stale_result(
    audio_engine: AudioEngine, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    source = tmp_path / "source.wav"
    write_mono_pcm16_wav(source, audio_engine.output_sample_rate())
    audio_engine.load_sample_async(0, str(source), run_analysis=False)
    _wait_event(audio_engine, "success")
    adapter = _HoldingAdapter()
    service = OfflineAnalysisService()
    job = service.start(
        audio_engine, 0, tmp_path / "analysis", model=BeatModelIdentity(), adapter=adapter
    )
    assert adapter.started.wait(5)

    try:
        audio_engine.unload_sample(0)
        assert adapter.cancel_seen.wait(5)
        audio_engine.load_sample_async(0, str(source), run_analysis=False)
        _wait_event(audio_engine, "success")
        with pytest.raises(RuntimeError, match="offline analysis busy"):
            audio_engine.begin_offline_analysis(0)
        assert not job.done.is_set()
    finally:
        adapter.release.set()

    assert job.done.wait(5)
    while (event := audio_engine.poll_loader_events()) is not None:
        assert event.get("type") != "offline_analysis_completed"
    replacement = service.start(audio_engine, 0, tmp_path / "analysis", model=BeatModelIdentity())
    assert replacement.done.wait(5)
    assert (
        replacement.snapshot().identity.source_generation
        != job.snapshot().identity.source_generation
    )
    assert replacement.snapshot().identity.request_id != job.snapshot().identity.request_id
    completion = _wait_event(audio_engine, "offline_analysis_completed")
    assert completion["request_id"] == replacement.snapshot().identity.request_id
