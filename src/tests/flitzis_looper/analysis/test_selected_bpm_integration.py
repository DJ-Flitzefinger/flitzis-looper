"""Diagnostic integration cannot adopt timing or publish stale BPM metadata."""

import json
import math
import struct
from dataclasses import asdict, replace
from threading import Event, current_thread
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatComponentResult,
    BeatModelIdentity,
    BeatPredictions,
    BeatWorkerRequest,
    MonoPcmInput,
)
from flitzis_looper.analysis.jobs import OfflineAnalysisService
from flitzis_looper.analysis.publication import decode_result, encode_result
from flitzis_looper.analysis.selected_bpm import summarize_published
from flitzis_looper.models import BeatGrid, SampleAnalysis
from tests.flitzis_looper.analysis.test_jobs import _ControlledAdapter, _Engine, _NativeJob
from tests.flitzis_looper.conftest import current_timing_metadata

if TYPE_CHECKING:
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.analysis.publication import PublishedAnalysisResult
    from flitzis_looper.analysis.selected_bpm_models import SelectedBpmReport
    from flitzis_looper.controller import AppController
    from flitzis_looper.models import TimingIntent


_KEY: dict[str, object] = {"status": "ready", "key": "C#m", "provenance": "keynet-test"}
_MODEL = BeatModelIdentity(
    sha256="a" * 64,
    frontend_id="beat-this-1.1.0-final0-minimal-fixture",
    environment_id="frozen-cpu-fp32-fixture",
)


def _binary64(values: tuple[float, ...]) -> bytes:
    return struct.pack(f"<{len(values)}d", *values)


def _predictions(logit_count: int = 4) -> BeatPredictions:
    beats = tuple(0.003 + index * 0.5 for index in range(200))
    logits: tuple[float, ...] = (-0.0, math.ulp(0.0), -math.ulp(0.0), 0.12345678901234568)
    if logit_count != 4:
        logits = (0.12345678901234568,) * logit_count
    return BeatPredictions(beats, beats[::4], logits, logits[::-1])


@pytest.fixture
def complete_request(tmp_path: Path) -> BeatWorkerRequest:
    return BeatWorkerRequest(
        AnalysisIdentity(0, 7, "loaded-source", 3),
        MonoPcmInput(tmp_path / "deleted-mono.f32le", 8000, 8000 * 120),
        _MODEL,
    )


def _ready(request: BeatWorkerRequest) -> BeatComponentResult:
    return BeatComponentResult(request.identity, request.model, "ready", "", _predictions())


class _CompleteNative(_NativeJob):
    def metadata(self) -> dict[str, object]:
        return dict(super().metadata(), sample_rate_hz=8000, frame_count=8000 * 120)

    def prepare_export(self, path: str) -> None:
        super().prepare_export(path)
        assert self.pcm_path is not None
        # Only source extent is numerical input. These lifecycle tests run no model/decoder.
        with self.pcm_path.open("r+b") as pcm:
            pcm.truncate(8000 * 120 * 4)


class _CompleteAdapter(_ControlledAdapter):
    def __init__(
        self, *, late_retirement: bool = False, wait_for_cancel: bool = False, logit_count: int = 4
    ) -> None:
        super().__init__(late_retirement=late_retirement, wait_for_cancel=wait_for_cancel)
        self.logit_count = logit_count

    def run(self, request: BeatWorkerRequest, cancel: Event) -> BeatComponentResult:
        return replace(
            super().run(request, cancel), status="ready", predictions=_predictions(self.logit_count)
        )


@pytest.mark.parametrize("version", [1, 2])
@pytest.mark.parametrize("key_status", ["ready", "unavailable", "failed", "cancelled"])
def test_model_free_opt_in_preserves_original_envelope_and_independent_key(
    complete_request: BeatWorkerRequest, version: int, key_status: str
) -> None:
    beat = _ready(complete_request)
    key = dict(_KEY, status=key_status)
    if version == 1:
        encoded = json.dumps({
            "schema_version": 1,
            "identity": asdict(complete_request.identity),
            "beat": asdict(beat),
            "key": key,
        })
    else:
        encoded = encode_result(beat, key)
    original_wire = encoded.encode("utf-8")
    original_decoded = decode_result(encoded, complete_request)

    decoded, report = summarize_published(encoded, complete_request)

    assert encoded.encode("utf-8") == original_wire
    assert decoded == original_decoded
    assert decoded.schema_version == version
    assert decoded.key == key
    assert report is not None
    assert report.predictions is decoded.beat.predictions
    assert report.assessment.global_status == "unverified"
    assert report.identity == complete_request.identity
    assert report.model == _MODEL
    assert decoded.beat.predictions is not None
    for name in ("beat_seconds", "downbeat_seconds", "beat_logits", "downbeat_logits"):
        assert _binary64(getattr(decoded.beat.predictions, name)) == _binary64(
            getattr(_predictions(), name)
        )
    assert not complete_request.pcm.path.exists()


def test_job_computes_only_after_retirement_offthread_then_exposes_after_finish(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    native = _CompleteNative()
    native.key_release.set()
    adapter = _CompleteAdapter(late_retirement=True)
    native.beat_retired = adapter.retired
    summary_calls: list[str] = []

    def check_retirement(
        encoded: str, request: BeatWorkerRequest
    ) -> tuple[PublishedAnalysisResult, SelectedBpmReport | None]:
        summary_calls.append(current_thread().name)
        assert native.pcm_retired.is_set()
        assert not native.finished.is_set()
        assert adapter.retired.is_set()
        assert native.pcm_path is not None
        assert not native.pcm_path.exists()
        return summarize_published(encoded, request)

    monkeypatch.setattr("flitzis_looper.analysis.jobs.summarize_published", check_retirement)
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=_MODEL, adapter=adapter
    )
    assert adapter.returned.wait(5)
    assert native.key_finished.wait(5)
    try:
        assert job.snapshot().bpm_summary is None
        assert job.snapshot().result_json is None
        assert not summary_calls
        assert not job.done.is_set()
        assert not native.pcm_retired.is_set()
    finally:
        adapter.retired.set()

    assert job.done.wait(5)
    snapshot = job.snapshot()
    assert snapshot.stage == "finished"
    assert snapshot.bpm_summary is not None
    assert snapshot.bpm_summary_error is None
    assert snapshot.bpm_summary.assessment.complete_fit is not None
    assert snapshot.bpm_summary.assessment.complete_fit.bpm == pytest.approx(120.0)
    assert summary_calls == ["offline-analysis-supervisor"]
    assert native.finished.is_set()
    assert list(tmp_path.iterdir()) == []


def test_metadata_computation_does_not_publish_snapshot_before_native_finish(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    native = _CompleteNative()
    native.key_release.set()
    summary_started = Event()
    release_summary = Event()

    def blocked_summary(
        encoded: str, request: BeatWorkerRequest
    ) -> tuple[PublishedAnalysisResult, SelectedBpmReport | None]:
        value = summarize_published(encoded, request)
        summary_started.set()
        assert release_summary.wait(5), "test metadata release timed out"
        return value

    monkeypatch.setattr("flitzis_looper.analysis.jobs.summarize_published", blocked_summary)
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=_MODEL, adapter=_CompleteAdapter()
    )
    try:
        assert summary_started.wait(5)
        assert native.pcm_retired.is_set()
        assert not native.finished.is_set()
        assert not job.done.is_set()
        assert job.snapshot().bpm_summary is None
        assert job.snapshot().result_json is None
    finally:
        release_summary.set()

    assert job.done.wait(5)
    assert job.snapshot().bpm_summary is not None


@pytest.mark.parametrize(
    ("phase", "summary_available"), [("before_acceptance", False), ("after_acceptance", True)]
)
def test_native_finish_freshness_is_authoritative_for_bpm_summary(
    tmp_path: Path, phase: str, *, summary_available: bool
) -> None:
    native = _CompleteNative(cancellation_during_finish=phase)
    native.key_release.set()
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=_MODEL, adapter=_CompleteAdapter()
    )

    assert job.done.wait(5)

    snapshot = job.snapshot()
    assert (snapshot.bpm_summary is not None) is summary_available
    assert snapshot.result_json is not None
    assert json.loads(snapshot.result_json)["beat"]["status"] == (
        "ready" if summary_available else "cancelled"
    )
    assert snapshot.bpm_summary_error is None
    assert list(tmp_path.iterdir()) == []


@pytest.mark.parametrize("cancel_source", [False, True])
def test_cancel_or_source_replacement_cannot_expose_bpm_summary(
    tmp_path: Path, *, cancel_source: bool
) -> None:
    native = _CompleteNative()
    adapter = _CompleteAdapter(wait_for_cancel=True)
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=_MODEL, adapter=adapter
    )
    assert native.key_started.wait(5)
    assert adapter.started.wait(5)
    try:
        if cancel_source:
            native.cancelled.set()
        else:
            job.cancel()
        assert adapter.returned.wait(5)
        assert job.snapshot().bpm_summary is None
        assert not native.pcm_retired.is_set()
    finally:
        native.key_release.set()

    assert job.done.wait(5)
    assert job.snapshot().bpm_summary is None
    assert job.snapshot().bpm_summary_error is None
    assert json.loads(native.result_json)["beat"]["status"] == "cancelled"


@pytest.mark.parametrize("key_result", ["[]", '{"status":"unavailable","key":"unknown"}'])
def test_independent_key_failure_cannot_discard_ready_beat_summary(
    tmp_path: Path, key_result: str
) -> None:
    native = _CompleteNative(key_result_json=key_result)
    native.key_release.set()
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=_MODEL, adapter=_CompleteAdapter()
    )

    assert job.done.wait(5)

    snapshot = job.snapshot()
    assert snapshot.bpm_summary is not None
    assert snapshot.result_json is not None
    result = json.loads(snapshot.result_json)
    assert result["beat"]["status"] == "ready"
    assert result["key"]["status"] in {"failed", "unavailable"}
    assert result["key"]["key"] == "unknown"


def test_oversize_publication_has_no_successful_summary_or_truncated_beat_result(
    tmp_path: Path,
) -> None:
    native = _CompleteNative()
    native.key_release.set()
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=_MODEL, adapter=_CompleteAdapter(logit_count=50_000)
    )

    assert job.done.wait(5)

    snapshot = job.snapshot()
    assert snapshot.bpm_summary is None
    assert snapshot.bpm_summary_error is None
    result = json.loads(native.result_json)
    assert result["beat"]["status"] == "failed"
    assert result["beat"]["predictions"] is None
    assert "publication size limit" in result["beat"]["reason"]
    assert result["key"] == _KEY | {"detail": ""}


@pytest.mark.parametrize("error_type", [RuntimeError, TypeError, ValueError])
def test_numerical_failure_retires_job_without_discarding_complete_beats_or_key(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, error_type: type[Exception]
) -> None:
    native = _CompleteNative()
    native.key_release.set()

    def fail_summary(
        _encoded: str, _request: BeatWorkerRequest
    ) -> tuple[PublishedAnalysisResult, SelectedBpmReport | None]:
        msg = "test numerical metadata unavailable"
        raise error_type(msg)

    monkeypatch.setattr("flitzis_looper.analysis.jobs.summarize_published", fail_summary)
    engine = _Engine(native)
    service = OfflineAnalysisService()
    job = service.start(engine, 0, tmp_path, model=_MODEL, adapter=_CompleteAdapter())

    assert job.done.wait(5)

    snapshot = job.snapshot()
    assert snapshot.stage == "finished"
    assert snapshot.bpm_summary is None
    assert snapshot.bpm_summary_error == "test numerical metadata unavailable"
    assert snapshot.result_json is not None
    result = json.loads(snapshot.result_json)
    assert result["beat"]["status"] == "ready"
    assert result["key"]["status"] == "ready"
    assert native.pcm_retired.is_set()
    assert native.finished.is_set()
    assert list(tmp_path.iterdir()) == []
    next_job = service.start(engine, 0, tmp_path, model=_MODEL, adapter=_CompleteAdapter())
    assert next_job.done.wait(5)
    assert engine.admissions == 2


@pytest.mark.parametrize("intent", ["legacy", "manual", "tap", "automatic"])
def test_headless_controller_diagnostic_completion_preserves_project_and_timing_authority(
    complete_request: BeatWorkerRequest,
    controller: AppController,
    audio_engine_mock: Mock,
    intent: TimingIntent,
) -> None:
    project = controller.project
    session = controller.session
    project.sample_paths[0] = "samples/source.wav"
    project.sample_durations[0] = 120.0
    project.sample_analysis[0] = SampleAnalysis(
        bpm=127.125,
        key="G#m",
        beat_grid=BeatGrid(beats=[0.125, 0.596], downbeats=[0.125], bars=[0.125]),
    )
    project.manual_bpm[0] = 94.123456789 if intent in {"manual", "tap"} else None
    project.pad_timing_intent[0] = intent
    project.pad_grid_anchor_s[0] = -0.125
    project.pad_grid_offset_samples[0] = 200_000
    project.pad_loop_auto[0] = False
    project.pad_loop_start_s[0] = 42.0
    project.pad_loop_end_s[0] = 42.5
    session.tap_bpm_pad_id = 0
    session.tap_bpm_timestamps = [100.0, 100.638]
    accepted = current_timing_metadata(period=0.500000000123, origin=-0.125)
    audio_engine_mock.current_constant_timing.return_value = accepted
    audio_engine_mock.pad_timing_intent.return_value = intent
    original_project = project.model_dump_json()
    original_session = session.model_dump_json()
    controller.persistence._dirty = False
    encoded = encode_result(_ready(complete_request), _KEY)
    _, report = summarize_published(encoded, complete_request)
    assert report is not None
    assert report.assessment.complete_fit is not None
    assert report.assessment.complete_fit.bpm == pytest.approx(120.0)
    audio_engine_mock.reset_mock()
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "offline_analysis_completed",
            "id": 0,
            "request_id": complete_request.identity.request_id,
            "result_json": encoded,
        },
        None,
    ]

    controller.poll_runtime_events()

    assert project.model_dump_json() == original_project
    assert session.model_dump_json() == original_session
    assert audio_engine_mock.current_constant_timing(0) == accepted
    assert not controller.persistence._dirty
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_intent.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
