import json
import shutil
from dataclasses import dataclass, field, replace
from pathlib import Path
from threading import Event, Thread, current_thread

import pytest

from flitzis_looper.analysis.contracts import (
    BeatComponentResult,
    BeatModelIdentity,
    BeatPredictions,
    BeatWorkerRequest,
)
from flitzis_looper.analysis.jobs import OfflineAnalysisService


@dataclass
class _NativeJob:
    key_release: Event = field(default_factory=Event)
    key_started: Event = field(default_factory=Event)
    key_finished: Event = field(default_factory=Event)
    cancelled: Event = field(default_factory=Event)
    finished: Event = field(default_factory=Event)
    pcm_retired: Event = field(default_factory=Event)
    retirement_attempted: Event = field(default_factory=Event)
    retiring: Event = field(default_factory=Event)
    beat_retired: Event | None = None
    pcm_path: Path | None = None
    result_json: str = ""
    key_running: bool = False
    export_thread: str = ""
    finish_thread: str = ""
    retirement_thread: str = ""
    progress_stages: list[str] = field(default_factory=list)
    prepare_error: bool = False
    retirement_error: bool = False
    key_result_json: str | None = None
    metadata_error: bool = False
    startup_abort_calls: int = 0
    cancellation_during_finish: str = ""

    def metadata(self) -> dict[str, object]:
        if self.metadata_error:
            msg = "native metadata unavailable"
            raise RuntimeError(msg)
        return {
            "pad_id": 0,
            "request_id": 7,
            "source_id": "loaded-source",
            "source_generation": 3,
            "sample_rate_hz": 48_000,
            "frame_count": 100,
            "channels": 2,
            "origin_seconds": 0.0,
        }

    def prepare_export(self, path: str) -> None:
        self.export_thread = current_thread().name
        self.pcm_path = Path(path)
        self.pcm_path.write_bytes(b"\0" * 400)
        if self.prepare_error:
            msg = "invalid shared PCM"
            raise RuntimeError(msg)

    def analyze_key(self) -> str:
        self.key_running = True
        self.key_started.set()
        try:
            assert self.key_release.wait(5), "test key release timed out"
            return self.key_result_json or json.dumps({
                "status": "ready",
                "key": "C#m",
                "detail": "",
                "provenance": "keynet-test",
            })
        finally:
            self.key_running = False
            self.key_finished.set()

    def is_cancelled(self) -> bool:
        return self.cancelled.is_set()

    def cancel(self) -> None:
        self.cancelled.set()

    def abort_unstarted(self) -> None:
        assert self.pcm_path is None
        assert not self.key_started.is_set()
        self.startup_abort_calls += 1
        self.finished.set()

    def progress(self, stage: str) -> None:
        self.progress_stages.append(stage)
        if stage == "retiring":
            self.retiring.set()

    def retire_pcm(self) -> None:
        assert not self.key_running
        assert self.beat_retired is None or self.beat_retired.is_set()
        assert self.pcm_path is None or self.pcm_path.exists()
        self.retirement_thread = current_thread().name
        self.retirement_attempted.set()
        if self.retirement_error:
            msg = "native PCM reader still active"
            raise RuntimeError(msg)
        self.pcm_retired.set()

    def finish(self, result_json: str) -> bool:
        assert not self.key_running
        assert self.pcm_retired.is_set()
        assert self.pcm_path is None or not self.pcm_path.exists()
        if self.cancellation_during_finish == "before_acceptance":
            self.cancelled.set()
        accepted = not self.cancelled.is_set()
        if self.cancellation_during_finish == "after_acceptance":
            self.cancelled.set()
        self.result_json = result_json
        self.finish_thread = current_thread().name
        self.finished.set()
        return accepted


@dataclass
class _Engine:
    native: _NativeJob
    admissions: int = 0

    def begin_offline_analysis(self, pad_id: int) -> _NativeJob:
        assert pad_id == 0
        if self.admissions and not self.native.finished.is_set():
            msg = "native analysis slot is occupied"
            raise RuntimeError(msg)
        self.admissions += 1
        return self.native


class _ControlledAdapter:
    def __init__(self, *, wait_for_cancel: bool = False, late_retirement: bool = False) -> None:
        self.retired = Event()
        self.started = Event()
        self.returned = Event()
        self.wait_for_cancel = wait_for_cancel
        self.late_retirement = late_retirement
        self.request: BeatWorkerRequest | None = None
        if not late_retirement:
            self.retired.set()

    def run(self, request: BeatWorkerRequest, cancel: Event) -> BeatComponentResult:
        self.request = request
        self.started.set()
        if self.wait_for_cancel:
            assert cancel.wait(5), "test cancellation timed out"
        self.returned.set()
        return BeatComponentResult(
            identity=request.identity,
            model=request.model,
            status="cancelled" if cancel.is_set() else "unavailable",
            reason="fixture without optional model",
            resources_released=not self.late_retirement,
        )


class _ReadyAdapter(_ControlledAdapter):
    def __init__(self, logit_count: int = 1) -> None:
        super().__init__()
        self.logit_count = logit_count

    def run(self, request: BeatWorkerRequest, cancel: Event) -> BeatComponentResult:
        return replace(
            super().run(request, cancel),
            status="ready",
            predictions=BeatPredictions(
                beat_seconds=(),
                downbeat_seconds=(),
                beat_logits=(0.5,) * self.logit_count,
                downbeat_logits=(0.5,) * self.logit_count,
            ),
        )


def test_missing_optional_worker_preserves_native_key_and_retires_offthread(tmp_path: Path) -> None:
    native = _NativeJob()
    native.key_release.set()
    service = OfflineAnalysisService()

    job = service.start(_Engine(native), 0, tmp_path, model=BeatModelIdentity())

    assert job.done.wait(5)
    result = json.loads(native.result_json)
    assert result["identity"] == {
        "pad_id": 0,
        "request_id": 7,
        "source_id": "loaded-source",
        "source_generation": 3,
    }
    assert result["beat"]["status"] == "unavailable"
    assert result["key"]["status"] == "ready"
    assert result["key"]["key"] == "C#m"
    assert native.export_thread == "offline-analysis-supervisor"
    assert native.finish_thread == "offline-analysis-supervisor"
    assert native.retirement_thread == "offline-analysis-supervisor"
    assert list(tmp_path.iterdir()) == []
    assert service.snapshot() is not None
    assert job.snapshot().stage == "finished"


def test_cancelled_stalled_key_retains_pcm_and_capacity_until_key_returns(tmp_path: Path) -> None:
    native = _NativeJob()
    engine = _Engine(native)
    adapter = _ControlledAdapter(wait_for_cancel=True)
    native.beat_retired = adapter.retired
    service = OfflineAnalysisService()
    job = service.start(engine, 0, tmp_path, model=BeatModelIdentity(), adapter=adapter)
    assert native.key_started.wait(5)
    assert adapter.started.wait(5)

    try:
        job.cancel()
        assert adapter.returned.wait(5)
        assert not job.done.is_set()
        assert not native.finished.is_set()
        assert native.pcm_path is not None
        assert native.pcm_path.exists()
        assert not native.retirement_attempted.is_set()
        with pytest.raises(RuntimeError, match="resources are still occupied"):
            service.start(engine, 0, tmp_path, model=BeatModelIdentity())
        assert engine.admissions == 1
        # Another coordinator cannot bypass the native global-to-engine reservation.
        with pytest.raises(RuntimeError, match="native analysis slot"):
            OfflineAnalysisService().start(engine, 0, tmp_path, model=BeatModelIdentity())
    finally:
        native.key_release.set()

    assert job.done.wait(5)
    assert not native.pcm_path.exists()
    assert native.pcm_retired.is_set()
    assert "retiring" in native.progress_stages
    result = json.loads(native.result_json)
    assert result["beat"]["status"] == "cancelled"
    assert result["key"]["status"] == "cancelled"


def test_native_source_replacement_cancels_beat_without_ui_polling(tmp_path: Path) -> None:
    native = _NativeJob()
    native.key_release.set()
    adapter = _ControlledAdapter(wait_for_cancel=True)
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=BeatModelIdentity(), adapter=adapter
    )
    assert adapter.started.wait(5)

    native.cancelled.set()

    assert adapter.returned.wait(5)
    assert job.done.wait(5)
    assert job.snapshot().cancellation_requested
    assert json.loads(native.result_json)["beat"]["status"] == "cancelled"


def test_shutdown_reports_pending_native_key_without_joining_it(tmp_path: Path) -> None:
    native = _NativeJob()
    adapter = _ControlledAdapter(wait_for_cancel=True)
    engine = _Engine(native)
    service = OfflineAnalysisService()
    job = service.start(engine, 0, tmp_path, model=BeatModelIdentity(), adapter=adapter)
    assert native.key_started.wait(5)

    try:
        snapshot = service.shutdown()
        assert snapshot is not None
        assert snapshot.cancellation_requested
        assert snapshot.stage != "finished"
        assert not native.finished.is_set()
        with pytest.raises(RuntimeError, match="shut down"):
            service.start(engine, 0, tmp_path, model=BeatModelIdentity())
    finally:
        native.key_release.set()

    assert job.done.wait(5)
    assert list(tmp_path.iterdir()) == []


def test_late_beat_process_retirement_holds_pcm_and_prevents_terminal_result(
    tmp_path: Path,
) -> None:
    native = _NativeJob()
    native.key_release.set()
    adapter = _ControlledAdapter(late_retirement=True)
    native.beat_retired = adapter.retired
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=BeatModelIdentity(), adapter=adapter
    )
    assert adapter.returned.wait(5)
    assert native.key_finished.wait(5)

    try:
        assert not job.done.is_set()
        assert not native.finished.is_set()
        assert native.pcm_path is not None
        assert native.pcm_path.exists()
        assert not native.retirement_attempted.is_set()
    finally:
        adapter.retired.set()

    assert job.done.wait(5)
    assert not native.pcm_path.exists()
    assert native.pcm_retired.is_set()
    assert json.loads(native.result_json)["beat"]["resources_released"]


def test_export_failure_does_not_launch_either_model_and_retires_partial_file(
    tmp_path: Path,
) -> None:
    native = _NativeJob(prepare_error=True)
    adapter = _ControlledAdapter()
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=BeatModelIdentity(), adapter=adapter
    )

    assert job.done.wait(5)
    assert not native.key_started.is_set()
    assert not adapter.started.is_set()
    assert native.pcm_retired.is_set()
    assert native.retirement_thread == "offline-analysis-supervisor"
    assert json.loads(native.result_json)["beat"]["status"] == "failed"
    assert list(tmp_path.iterdir()) == []


def test_worker_request_uses_native_loaded_origin_and_rate_not_key_rate(tmp_path: Path) -> None:
    native = _NativeJob()
    native.key_release.set()
    adapter = _ControlledAdapter()
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=BeatModelIdentity(), adapter=adapter
    )

    assert job.done.wait(5)
    assert adapter.request is not None
    assert adapter.request.pcm.sample_rate_hz == 48_000
    assert adapter.request.pcm.frame_count == 100
    assert adapter.request.pcm.origin_seconds == 0.0
    assert adapter.request.pcm.channels == 1


def test_invalid_key_result_does_not_discard_successful_beat_component(tmp_path: Path) -> None:
    native = _NativeJob(key_result_json="[]")
    native.key_release.set()
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=BeatModelIdentity(), adapter=_ReadyAdapter()
    )

    assert job.done.wait(5)
    result = json.loads(native.result_json)
    assert result["beat"]["status"] == "ready"
    assert result["key"]["status"] == "failed"
    assert result["key"]["key"] == "unknown"


def test_oversize_beat_output_is_rejected_without_losing_independent_key(tmp_path: Path) -> None:
    native = _NativeJob()
    native.key_release.set()
    job = OfflineAnalysisService().start(
        _Engine(native),
        0,
        tmp_path,
        model=BeatModelIdentity(),
        adapter=_ReadyAdapter(logit_count=150_000),
    )

    assert job.done.wait(5)
    result = json.loads(native.result_json)
    assert result["beat"]["status"] == "failed"
    assert "publication size limit" in result["beat"]["reason"]
    assert result["beat"]["predictions"] is None
    assert result["key"]["status"] == "ready"
    assert len(native.result_json.encode("utf-8")) < 1024 * 1024


def test_locked_pcm_cleanup_retains_reservation_and_retries_offthread(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    native = _NativeJob()
    native.key_release.set()
    attempted_cleanup = Event()
    release_cleanup = Event()
    original_rmtree = shutil.rmtree

    def retire(directory: Path) -> None:
        assert current_thread().name == "offline-analysis-supervisor"
        assert native.pcm_retired.is_set()
        attempted_cleanup.set()
        if not release_cleanup.is_set():
            msg = "PCM still mapped by operating system"
            raise PermissionError(msg)
        original_rmtree(directory)

    monkeypatch.setattr("flitzis_looper.analysis.jobs.shutil.rmtree", retire)
    service = OfflineAnalysisService()
    job = service.start(_Engine(native), 0, tmp_path, model=BeatModelIdentity())
    assert attempted_cleanup.wait(5)

    try:
        assert not job.done.is_set()
        assert not native.finished.is_set()
        assert native.pcm_path is not None
        assert native.pcm_path.exists()
        snapshot = service.shutdown()
        assert snapshot is not None
        assert snapshot.cancellation_requested
    finally:
        release_cleanup.set()

    assert job.done.wait(5)
    assert "retiring" in native.progress_stages
    assert list(tmp_path.iterdir()) == []


@pytest.mark.parametrize("prepare_error", [False, True])
def test_native_pcm_retirement_refusal_preserves_files_slot_and_unpublished_result(
    tmp_path: Path, *, prepare_error: bool
) -> None:
    native = _NativeJob(prepare_error=prepare_error, retirement_error=True)
    native.key_release.set()
    engine = _Engine(native)
    adapter = _ControlledAdapter()
    native.beat_retired = adapter.retired
    service = OfflineAnalysisService()

    job = service.start(engine, 0, tmp_path, model=BeatModelIdentity(), adapter=adapter)

    assert native.retirement_attempted.wait(5)
    assert native.retiring.wait(5)
    snapshot = job.snapshot()
    assert snapshot.stage == "retiring"
    assert "native PCM reader still active" in snapshot.detail
    assert snapshot.result_json is None
    assert not job.done.is_set()
    assert not native.finished.is_set()
    assert not native.pcm_retired.is_set()
    assert not native.result_json
    assert native.pcm_path is not None
    assert native.pcm_path.exists()
    assert native.pcm_path.parent.exists()
    assert native.key_started.is_set() is not prepare_error
    assert adapter.started.is_set() is not prepare_error
    with pytest.raises(RuntimeError, match="resources are still occupied"):
        service.start(engine, 0, tmp_path, model=BeatModelIdentity())
    with pytest.raises(RuntimeError, match="native analysis slot"):
        OfflineAnalysisService().start(engine, 0, tmp_path, model=BeatModelIdentity())
    assert engine.admissions == 1


def test_metadata_failure_aborts_native_admission(tmp_path: Path) -> None:
    native = _NativeJob(metadata_error=True)
    service = OfflineAnalysisService()

    with pytest.raises(RuntimeError, match="metadata unavailable"):
        service.start(_Engine(native), 0, tmp_path, model=BeatModelIdentity())

    assert native.startup_abort_calls == 1
    assert native.cancelled.is_set()
    assert native.finished.is_set()
    assert service.snapshot() is None


@pytest.mark.parametrize(
    ("cancellation_phase", "expected_status"),
    [("before_acceptance", "cancelled"), ("after_acceptance", "ready")],
)
def test_snapshot_reports_native_acceptance_when_cancellation_races_finish(
    tmp_path: Path, cancellation_phase: str, expected_status: str
) -> None:
    native = _NativeJob(cancellation_during_finish=cancellation_phase)
    native.key_release.set()
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=BeatModelIdentity(), adapter=_ReadyAdapter()
    )

    assert job.done.wait(5)
    result_json = job.snapshot().result_json
    assert result_json is not None
    result = json.loads(result_json)
    assert result["beat"]["status"] == expected_status
    assert result["key"]["status"] == expected_status
    assert native.finished.is_set()


def test_relative_workdir_is_rejected_before_native_admission() -> None:
    native = _NativeJob()
    engine = _Engine(native)

    with pytest.raises(ValueError, match="workdir must be absolute"):
        OfflineAnalysisService().start(engine, 0, Path("relative"), model=BeatModelIdentity())

    assert engine.admissions == 0
    assert not native.key_started.is_set()
    assert not native.finished.is_set()


@pytest.mark.parametrize(
    "failed_thread",
    ["offline-analysis-supervisor", "offline-analysis-cancellation", "offline-analysis-key"],
)
def test_thread_start_failure_never_orphans_native_admission(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failed_thread: str
) -> None:
    class FailingThread(Thread):
        def start(self) -> None:
            if self.name == failed_thread:
                msg = "OS thread start failed"
                raise RuntimeError(msg)
            super().start()

    monkeypatch.setattr("flitzis_looper.analysis.jobs.Thread", FailingThread)
    native = _NativeJob()
    service = OfflineAnalysisService()
    if failed_thread == "offline-analysis-supervisor":
        with pytest.raises(RuntimeError, match="OS thread start failed"):
            service.start(_Engine(native), 0, tmp_path, model=BeatModelIdentity())
        assert service.snapshot() is None
        assert native.startup_abort_calls == 1
    else:
        job = service.start(_Engine(native), 0, tmp_path, model=BeatModelIdentity())
        assert job.done.wait(5)
        assert json.loads(native.result_json)["beat"]["status"] == "failed"
    assert native.finished.is_set()
    assert not native.key_started.is_set()
    assert list(tmp_path.iterdir()) == []


@pytest.mark.parametrize(
    "invalid_part",
    [
        "request_id",
        "source_generation",
        "source_id",
        "pad_id",
        "model",
        "missing_predictions",
        "unsorted_times",
        "duplicate_times",
        "nonfinite_times",
        "outside_source",
        "mismatched_logits",
        "nonfinite_logits",
        "failed_identity",
        "failed_predictions",
    ],
)
def test_invalid_adapter_component_is_rejected_without_discarding_key(
    tmp_path: Path, invalid_part: str
) -> None:
    class InvalidAdapter(_ReadyAdapter):
        def run(self, request: BeatWorkerRequest, cancel: Event) -> BeatComponentResult:
            result = super().run(request, cancel)
            assert result.predictions is not None
            identity = result.identity
            predictions = result.predictions
            identities = {
                "request_id": replace(identity, request_id=99),
                "source_generation": replace(identity, source_generation=99),
                "pad_id": replace(identity, pad_id=99),
                "source_id": replace(identity, source_id="old-source"),
            }
            if invalid_part in identities:
                return replace(result, identity=identities[invalid_part])
            if invalid_part == "model":
                return replace(result, model=replace(result.model, sha256="0" * 64))
            if invalid_part == "missing_predictions":
                return replace(result, predictions=None)
            if invalid_part == "failed_identity":
                return replace(
                    result,
                    status="failed",
                    predictions=None,
                    identity=replace(identity, request_id=99),
                )
            if invalid_part == "failed_predictions":
                return replace(result, status="failed")
            return replace(result, predictions=_invalid_predictions(predictions, invalid_part))

    native = _NativeJob()
    native.key_release.set()
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=BeatModelIdentity(), adapter=InvalidAdapter()
    )

    assert job.done.wait(5)
    result = json.loads(native.result_json)
    assert result["beat"]["status"] == "failed"
    assert result["beat"]["predictions"] is None
    assert result["beat"]["identity"] == result["identity"]
    assert result["key"]["status"] == "ready"
    assert result["key"]["key"] == "C#m"


def _invalid_predictions(predictions: BeatPredictions, invalid_part: str) -> BeatPredictions:
    if invalid_part == "mismatched_logits":
        return replace(predictions, downbeat_logits=())
    if invalid_part == "nonfinite_logits":
        return replace(predictions, beat_logits=(float("inf"),))
    times = {
        "unsorted_times": (0.001, 0.0),
        "duplicate_times": (0.001, 0.001),
        "nonfinite_times": (float("nan"),),
        "outside_source": (1.0,),
    }
    return replace(predictions, beat_seconds=times[invalid_part])


def test_invalid_component_still_waits_for_its_process_retirement(tmp_path: Path) -> None:
    class InvalidRetiringAdapter(_ControlledAdapter):
        def run(self, request: BeatWorkerRequest, cancel: Event) -> BeatComponentResult:
            return replace(
                super().run(request, cancel), identity=replace(request.identity, request_id=99)
            )

    native = _NativeJob()
    native.key_release.set()
    adapter = InvalidRetiringAdapter(late_retirement=True)
    job = OfflineAnalysisService().start(
        _Engine(native), 0, tmp_path, model=BeatModelIdentity(), adapter=adapter
    )
    assert adapter.returned.wait(5)
    try:
        assert not job.done.is_set()
        assert not native.finished.is_set()
        assert native.pcm_path is not None
        assert native.pcm_path.exists()
    finally:
        adapter.retired.set()
    assert job.done.wait(5)
    result = json.loads(native.result_json)
    assert result["beat"]["status"] == "failed"
    assert result["key"]["status"] == "ready"
