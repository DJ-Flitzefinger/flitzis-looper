import json
import math
import struct
import sys
import time
from dataclasses import asdict
from threading import Event
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis.contracts import (
    BeatComponentResult,
    BeatModelIdentity,
    BeatPredictions,
    BeatWorkerRequest,
)
from flitzis_looper.analysis.jobs import OfflineAnalysisService
from flitzis_looper.analysis.publication import MAX_ENVELOPE_BYTES, decode_result
from flitzis_looper.analysis.worker import BeatWorkerAdapter
from tests.conftest import write_mono_pcm16_wav

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper_audio import AudioEngine, OfflineAnalysisJob


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


def test_native_offline_analysis_reuses_pcm_with_sealed_project_original(
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
    if sys.platform == "win32":
        with pytest.raises(PermissionError):
            cached_path.unlink()
        assert cached_path.is_file()
    else:
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


def _cancelled_native_envelope(native: OfflineAnalysisJob) -> str:
    metadata = native.metadata()
    return json.dumps({
        "schema_version": 1,
        "identity": {
            name: metadata[name]
            for name in ("pad_id", "request_id", "source_id", "source_generation")
        },
        "beat": {"status": "cancelled"},
        "key": {"status": "cancelled", "key": "unknown"},
    })


def test_native_staging_accounts_actual_source_and_complete_key_ownership(
    audio_engine: AudioEngine, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    source = tmp_path / "source.wav"
    write_mono_pcm16_wav(source, audio_engine.output_sample_rate())
    audio_engine.load_sample_async(0, str(source), run_analysis=False)
    _wait_event(audio_engine, "success")
    original_shape = audio_engine.loaded_sample_shape(0)
    loaded_rate, channels, frames = original_shape
    native = audio_engine.begin_offline_analysis(0)
    envelope = _cancelled_native_envelope(native)
    pcm_path = tmp_path / "mono.f32le"

    try:
        initial = native.staging_stats()
        source_bytes = channels * frames * 4
        assert initial["retained_source_bytes"] == source_bytes
        assert initial["observed_export_peak_bytes"] == 0
        assert initial["observed_key_peak_bytes"] == 0
        assert initial["limit_bytes"] == 512 * 1024 * 1024
        assert initial["admitted_peak_bytes"] == max(
            initial["admitted_export_peak_bytes"], initial["admitted_key_peak_bytes"]
        )
        assert initial["admitted_peak_bytes"] <= initial["limit_bytes"]

        native.prepare_export(str(pcm_path))

        prepared = native.staging_stats()
        assert prepared["retained_source_bytes"] == 0
        assert prepared["export_file_bytes"] == pcm_path.stat().st_size == frames * 4
        assert source_bytes < prepared["observed_export_peak_bytes"]
        assert prepared["observed_export_peak_bytes"] <= prepared["admitted_export_peak_bytes"]
        assert prepared["observed_key_peak_bytes"] == 0
        assert struct.unpack(f"<{frames}f", pcm_path.read_bytes()) == (0.25,) * frames
        assert audio_engine.loaded_sample_shape(0) == original_shape

        key = json.loads(native.analyze_key())
        assert key["status"] == "failed"
        assert key["key"] == "unknown"
        assert "InsufficientData" in key["detail"]
        converted = native.staging_stats()
        complete_key_bytes = math.ceil(frames * 44100 / loaded_rate) * 4
        assert complete_key_bytes <= converted["observed_key_peak_bytes"]
        assert converted["observed_key_peak_bytes"] <= converted["admitted_key_peak_bytes"]
        assert converted["retained_source_bytes"] == 0

        native.retire_pcm()
        pcm_path.unlink()
        assert audio_engine.loaded_sample_shape(0) == original_shape
        audio_engine.play_sample(0, 0.0)
        audio_engine.stop_all()
    finally:
        native.cancel()
        native.retire_pcm()
        pcm_path.unlink(missing_ok=True)
        assert not native.finish(envelope)

    replacement = audio_engine.begin_offline_analysis(0)
    replacement.abort_unstarted()


@pytest.mark.skipif(sys.platform != "win32", reason="Windows staged-file sharing contract")
def test_native_staged_file_blocks_writes_and_deletion_until_reader_retirement(
    audio_engine: AudioEngine, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    source = tmp_path / "source.wav"
    write_mono_pcm16_wav(source, audio_engine.output_sample_rate())
    audio_engine.load_sample_async(0, str(source), run_analysis=False)
    _wait_event(audio_engine, "success")
    native = audio_engine.begin_offline_analysis(0)
    envelope = _cancelled_native_envelope(native)
    pcm_path = tmp_path / "mono.f32le"

    try:
        native.prepare_export(str(pcm_path))
        original_pcm = pcm_path.read_bytes()
        with pytest.raises(PermissionError), pcm_path.open("r+b"):
            pytest.fail("another writer acquired staged PCM while native readers own it")
        with pytest.raises(PermissionError):
            pcm_path.unlink()
        assert pcm_path.read_bytes() == original_pcm

        # Retirement is also needed if cancellation arrives before key starts.
        native.cancel()
        native.retire_pcm()
        assert pcm_path.exists()
        with pytest.raises(RuntimeError, match="offline analysis busy"):
            audio_engine.begin_offline_analysis(0)
        with pcm_path.open("r+b") as output:
            output.write(b"test")
        pcm_path.unlink()
        with pytest.raises(RuntimeError, match="retired"):
            native.analyze_key()
    finally:
        native.cancel()
        native.retire_pcm()
        pcm_path.unlink(missing_ok=True)
        assert not native.finish(envelope)

    while (event := audio_engine.poll_loader_events()) is not None:
        assert event.get("type") != "offline_analysis_completed"
    replacement = audio_engine.begin_offline_analysis(0)
    replacement.abort_unstarted()


def test_native_prepare_failure_releases_source_without_ever_starting_key(
    audio_engine: AudioEngine, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    source = tmp_path / "source.wav"
    write_mono_pcm16_wav(source, audio_engine.output_sample_rate())
    audio_engine.load_sample_async(0, str(source), run_analysis=False)
    _wait_event(audio_engine, "success")
    original_shape = audio_engine.loaded_sample_shape(0)
    native = audio_engine.begin_offline_analysis(0)
    envelope = _cancelled_native_envelope(native)
    missing_parent = tmp_path / "nonexistent" / "mono.f32le"

    try:
        with pytest.raises(RuntimeError):
            native.prepare_export(str(missing_parent))
        assert not missing_parent.exists()
        assert native.staging_stats()["retained_source_bytes"] > 0
        native.retire_pcm()
        assert native.staging_stats()["retained_source_bytes"] == 0
        assert native.staging_stats()["observed_key_peak_bytes"] == 0
        with pytest.raises(RuntimeError, match="retired"):
            native.analyze_key()
        with pytest.raises(RuntimeError, match="offline analysis busy"):
            audio_engine.begin_offline_analysis(0)
        assert audio_engine.loaded_sample_shape(0) == original_shape
    finally:
        native.cancel()
        native.retire_pcm()
        assert not native.finish(envelope)

    replacement = audio_engine.begin_offline_analysis(0)
    replacement.abort_unstarted()


class _CompleteReadyAdapter:
    def __init__(self) -> None:
        self.retired = Event()
        self.retired.set()
        self.request: BeatWorkerRequest | None = None
        self.result: BeatComponentResult | None = None

    def run(self, request: BeatWorkerRequest, cancel: Event) -> BeatComponentResult:
        assert not cancel.is_set()
        assert request.pcm.path.stat().st_size == 128 * 4
        self.request = request
        # Ordinary nontrivial doubles make JSON larger than 1 MiB; complete binary64 fits.
        logits = tuple(math.sin(index + 0.12345678901234568) for index in range(30_000))
        rate = request.pcm.sample_rate_hz
        self.result = BeatComponentResult(
            identity=request.identity,
            model=request.model,
            status="ready",
            reason="",
            predictions=BeatPredictions(
                beat_seconds=(-0.0, 1 / rate, 127 / rate),
                downbeat_seconds=(0.0, 127 / rate),
                beat_logits=logits,
                downbeat_logits=logits[::-1],
            ),
        )
        return self.result


def _assert_exact_predictions(actual: BeatComponentResult, expected: BeatComponentResult) -> None:
    assert actual.identity == expected.identity
    assert actual.model == expected.model
    assert actual.status == "ready"
    assert actual.resources_released
    assert actual.predictions is not None
    assert expected.predictions is not None
    for name in ("beat_seconds", "downbeat_seconds", "beat_logits", "downbeat_logits"):
        expected_values = getattr(expected.predictions, name)
        actual_values = getattr(actual.predictions, name)
        assert struct.pack(f"<{len(actual_values)}d", *actual_values) == struct.pack(
            f"<{len(expected_values)}d", *expected_values
        )


def test_native_complete_v2_publication_retires_pcm_and_reopens_admission(
    audio_engine: AudioEngine, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    source = tmp_path / "source.wav"
    write_mono_pcm16_wav(source, audio_engine.output_sample_rate())
    audio_engine.load_sample_async(0, str(source), run_analysis=False)
    loaded = _wait_event(audio_engine, "success")
    original_shape = audio_engine.loaded_sample_shape(0)
    assert original_shape[2] == 128
    adapter = _CompleteReadyAdapter()
    model = BeatModelIdentity(
        sha256="a" * 64,
        frontend_id="fixture-reference-frontend",
        environment_id="fixture-locked-cpu-environment",
    )
    service = OfflineAnalysisService()

    job = service.start(audio_engine, 0, tmp_path / "analysis", model=model, adapter=adapter)

    assert job.done.wait(5), job.snapshot()
    completion = _wait_event(audio_engine, "offline_analysis_completed")
    assert adapter.request is not None
    assert adapter.result is not None
    assert adapter.result.predictions is not None
    assert not adapter.request.pcm.path.exists()
    assert list((tmp_path / "analysis").iterdir()) == []
    legacy = {
        "schema_version": 1,
        "identity": asdict(adapter.request.identity),
        "beat": asdict(adapter.result),
        "key": {"status": "failed", "key": "unknown"},
    }
    assert len(json.dumps(legacy).encode("utf-8")) > MAX_ENVELOPE_BYTES
    payload = completion["result_json"]
    assert isinstance(payload, str)
    assert len(payload.encode("utf-8")) <= MAX_ENVELOPE_BYTES
    snapshot = job.snapshot()
    assert snapshot.stage == "finished"
    assert snapshot.result_json is not None
    assert completion["request_id"] == adapter.request.identity.request_id
    decoded = decode_result(payload, adapter.request)
    snapshot_result = decode_result(snapshot.result_json, adapter.request)
    assert decoded.schema_version == 2
    assert decoded.identity.source_generation == adapter.request.identity.source_generation
    _assert_exact_predictions(decoded.beat, adapter.result)
    _assert_exact_predictions(snapshot_result.beat, adapter.result)
    assert snapshot_result.key == decoded.key
    assert decoded.key["status"] == "failed"
    assert decoded.key["key"] == "unknown"
    assert "InsufficientData" in str(decoded.key["detail"])
    assert audio_engine.loaded_sample_shape(0) == original_shape
    assert source.exists()
    assert "analysis" not in loaded
    while (event := audio_engine.poll_loader_events()) is not None:
        assert event.get("type") != "offline_analysis_completed"

    # The real native finish released admission; a subsequent service request can complete.
    replacement = service.start(audio_engine, 0, tmp_path / "analysis", model=BeatModelIdentity())
    assert replacement.done.wait(5), replacement.snapshot()
    subsequent = _wait_event(audio_engine, "offline_analysis_completed")
    assert subsequent["request_id"] != completion["request_id"]
    assert replacement.snapshot().identity.source_generation == snapshot.identity.source_generation
    assert audio_engine.loaded_sample_shape(0) == original_shape
    assert list((tmp_path / "analysis").iterdir()) == []
