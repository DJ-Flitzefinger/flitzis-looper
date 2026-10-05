"""Bounded supervision for the optional, diagnostic offline analysis boundary."""

import json
import math
import shutil
import tempfile
from dataclasses import dataclass, replace
from pathlib import Path
from threading import Event, Lock, Thread
from typing import Literal, Protocol

from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatComponentResult,
    BeatModelIdentity,
    BeatWorkerRequest,
    MonoPcmInput,
    WorkerLimits,
    validate_component_result,
)
from flitzis_looper.analysis.publication import encode_result
from flitzis_looper.analysis.worker import BeatWorkerAdapter

_POLL_SECONDS = 0.02
_MAX_KEY_JSON_BYTES = 16_384
type JobStage = Literal["preparing", "running", "waiting_key", "retiring", "finished"]


class NativeAnalysisJob(Protocol):
    """Native reservation owning staged PCM until both branch readers retire."""

    def metadata(self) -> dict[str, object]: ...

    def prepare_export(self, path: str) -> None: ...

    def analyze_key(self) -> str: ...

    def is_cancelled(self) -> bool: ...

    def cancel(self) -> None: ...

    def abort_unstarted(self) -> None: ...

    def retire_pcm(self) -> None: ...

    def progress(self, stage: str) -> None: ...

    def finish(self, result_json: str) -> bool: ...


class OfflineAnalysisEngine(Protocol):
    """Existing engine admission boundary; implementations must reject excess jobs."""

    def begin_offline_analysis(self, pad_id: int, /) -> NativeAnalysisJob: ...


class BeatAnalysisAdapter(Protocol):
    """Process adapter retaining ownership until its retirement event is set."""

    retired: Event

    def run(self, request: BeatWorkerRequest, cancel: Event) -> BeatComponentResult: ...


@dataclass(frozen=True, slots=True)
class JobSnapshot:
    """Nonblocking diagnostic state; only finished means resources were retired."""

    identity: AnalysisIdentity
    stage: JobStage
    cancellation_requested: bool
    detail: str = ""
    result_json: str | None = None


@dataclass(frozen=True, slots=True)
class _LoadedInput:
    identity: AnalysisIdentity
    sample_rate_hz: int
    frame_count: int
    origin_seconds: float


def _integer(metadata: dict[str, object], name: str, minimum: int) -> int:
    value = metadata[name]
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        msg = f"invalid native analysis {name}"
        raise ValueError(msg)
    return value


def _loaded_input(metadata: dict[str, object]) -> _LoadedInput:
    source_id = metadata["source_id"]
    origin = metadata["origin_seconds"]
    if not isinstance(source_id, str) or not source_id:
        msg = "invalid native analysis source identity"
        raise ValueError(msg)
    if (
        isinstance(origin, bool)
        or not isinstance(origin, (int, float))
        or not math.isfinite(origin)
    ):
        msg = "invalid native analysis source origin"
        raise ValueError(msg)
    return _LoadedInput(
        identity=AnalysisIdentity(
            pad_id=_integer(metadata, "pad_id", 0),
            request_id=_integer(metadata, "request_id", 1),
            source_id=source_id,
            source_generation=_integer(metadata, "source_generation", 1),
        ),
        sample_rate_hz=_integer(metadata, "sample_rate_hz", 1),
        frame_count=_integer(metadata, "frame_count", 1),
        origin_seconds=float(origin),
    )


def _key_failure(detail: str, *, cancelled: bool = False) -> dict[str, object]:
    return {
        "status": "cancelled" if cancelled else "failed",
        "key": "unknown",
        "detail": detail[:1024],
        "provenance": "KeyNet/native",
    }


def _key_result(native: NativeAnalysisJob) -> dict[str, object]:
    raw = native.analyze_key()
    if len(raw.encode("utf-8")) > _MAX_KEY_JSON_BYTES:
        msg = "native key result exceeds the component size limit"
        raise ValueError(msg)
    parsed: object = json.loads(raw)
    if not isinstance(parsed, dict):
        msg = "native key result must be an object"
        raise TypeError(msg)
    result: dict[str, object] = parsed
    if result.get("status") not in {"ready", "unavailable", "failed", "cancelled"}:
        msg = "invalid native key component status"
        raise ValueError(msg)
    if not isinstance(result.get("key"), str):
        msg = "native key result has no key string"
        raise TypeError(msg)
    return result


class AnalysisJob:
    """One supervised request; cancel and snapshot never join native inference."""

    def __init__(
        self,
        native: NativeAnalysisJob,
        loaded: _LoadedInput,
        workdir: Path,
        model: BeatModelIdentity,
        adapter: BeatAnalysisAdapter,
    ) -> None:
        self._native = native
        self._loaded = loaded
        self._workdir = workdir
        self._model = model
        self._adapter = adapter
        self._cancelled = Event()
        self._bridge_done = Event()
        self._lock = Lock()
        self._key_thread: Thread | None = None
        self._beat_started = False
        self._snapshot = JobSnapshot(loaded.identity, "preparing", cancellation_requested=False)
        self.done = Event()

    def cancel(self) -> None:
        """Invalidate this request without waiting for its resources to retire."""
        self._cancelled.set()
        self._native.cancel()

    def snapshot(self) -> JobSnapshot:
        """Read state without waiting for a worker or native model call."""
        with self._lock:
            return replace(self._snapshot, cancellation_requested=self._cancelled.is_set())

    def _set_stage(self, stage: JobStage, detail: str = "") -> None:
        detail = detail[:1024]
        with self._lock:
            if self._snapshot.stage == stage and self._snapshot.detail == detail:
                return
            self._snapshot = replace(self._snapshot, stage=stage, detail=detail)
        try:
            self._native.progress(stage)
        except RuntimeError:
            # An invalidated request can no longer publish progress.
            self._cancelled.set()

    def _bridge_cancellation(self) -> None:
        while not self._bridge_done.is_set():
            try:
                if self._native.is_cancelled():
                    self._cancelled.set()
            except RuntimeError:
                self._cancelled.set()
            self._bridge_done.wait(_POLL_SECONDS)

    def _run_key(self, results: list[dict[str, object]]) -> None:
        try:
            results.append(_key_result(self._native))
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            results.append(_key_failure(str(error), cancelled=self._cancelled.is_set()))

    def _wait_for_key(self) -> None:
        if self._key_thread is None:
            return
        while self._key_thread.is_alive():
            stage: JobStage = "retiring" if self._cancelled.is_set() else "waiting_key"
            if self.snapshot().stage != stage:
                self._set_stage(stage)
            self._key_thread.join(_POLL_SECONDS)

    def _cleanup(self, directory: Path | None) -> None:
        if directory is None:
            return
        while True:
            try:
                shutil.rmtree(directory)
            except FileNotFoundError:
                return
            except OSError as error:
                # Keep the admission reservation while an OS reader still owns the file.
                self._set_stage("retiring", f"Temporary PCM cleanup pending: {error}")
                self._bridge_done.wait(_POLL_SECONDS)
            else:
                return

    def _failed_beats(self, reason: str) -> BeatComponentResult:
        return BeatComponentResult(
            identity=self._loaded.identity,
            model=self._model,
            status="cancelled" if self._cancelled.is_set() else "failed",
            reason=reason[:1024],
        )

    def _run_branches(
        self, pcm_path: Path, key_results: list[dict[str, object]]
    ) -> BeatComponentResult:
        self._native.prepare_export(str(pcm_path))
        if self._native.is_cancelled():
            self._cancelled.set()
        if self._cancelled.is_set():
            return self._failed_beats("Analysis cancelled before inference")
        request = BeatWorkerRequest(
            identity=self._loaded.identity,
            pcm=MonoPcmInput(
                path=pcm_path,
                sample_rate_hz=self._loaded.sample_rate_hz,
                frame_count=self._loaded.frame_count,
                origin_seconds=self._loaded.origin_seconds,
            ),
            model=self._model,
        )
        self._key_thread = Thread(
            target=self._run_key,
            args=(key_results,),
            name="offline-analysis-key",
            daemon=True,
        )
        self._key_thread.start()
        self._set_stage("running")
        self._beat_started = True
        result = self._adapter.run(request, self._cancelled)
        validate_component_result(result, request, WorkerLimits())
        return result

    def _supervise(self) -> None:
        directory: Path | None = None
        key_results: list[dict[str, object]] = []
        bridge = Thread(
            target=self._bridge_cancellation,
            name="offline-analysis-cancellation",
            daemon=True,
        )
        bridge_started = False
        try:
            bridge.start()
            bridge_started = True
            self._workdir.mkdir(parents=True, exist_ok=True)
            directory = Path(tempfile.mkdtemp(prefix="offline-analysis-", dir=self._workdir))
            beat = self._run_branches(directory / "mono.f32le", key_results)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            beat = self._failed_beats(str(error))
        if self._beat_started and (
            not beat.resources_released or not self._adapter.retired.is_set()
        ):
            self._set_stage("retiring", "Waiting for beat worker resource release")
            self._adapter.retired.wait()
            beat = replace(beat, resources_released=True)
        self._wait_for_key()
        key = key_results[0] if key_results else _key_failure("Key analysis did not start")
        try:
            self._native.retire_pcm()
        except RuntimeError as error:
            # A native refusal is not retirement. Keep admission and the files;
            # closing readers is required before filesystem cleanup or publication.
            self._set_stage("retiring", f"Native PCM retirement failed: {error}")
            self._bridge_done.set()
            if bridge_started:
                bridge.join()
            return
        self._cleanup(directory)
        self._bridge_done.set()
        if bridge_started:
            bridge.join()
        if self._native.is_cancelled():
            self._cancelled.set()
        if self._cancelled.is_set():
            beat = replace(beat, status="cancelled", reason="Analysis cancelled", predictions=None)
            key = _key_failure("Analysis cancelled", cancelled=True)
        result_json = self._encode_result(beat, key)
        try:
            accepted = self._native.finish(result_json)
        except (RuntimeError, ValueError) as error:
            # Refusal must remain nonterminal: native ownership was not confirmed released.
            self._set_stage("retiring", f"Native analysis retirement failed: {error}")
            return
        if not accepted:
            # Native acceptance is linearized against source replacement/cancellation.
            # Rechecking source identity after acceptance would race the next request.
            self._cancelled.set()
            beat = replace(beat, status="cancelled", reason="Analysis cancelled", predictions=None)
            key = _key_failure("Analysis cancelled", cancelled=True)
            result_json = self._encode_result(beat, key)
        with self._lock:
            self._snapshot = replace(self._snapshot, stage="finished", result_json=result_json)
        self.done.set()

    def _encode_result(self, beat: BeatComponentResult, key: dict[str, object]) -> str:
        return encode_result(beat, key)


class OfflineAnalysisService:
    """Single admission facade for existing native jobs, with no pending queue."""

    def __init__(self) -> None:
        self._lock = Lock()
        self._job: AnalysisJob | None = None
        self._closed = False

    def start(
        self,
        engine: OfflineAnalysisEngine,
        pad_id: int,
        workdir: Path,
        *,
        model: BeatModelIdentity,
        adapter: BeatAnalysisAdapter | None = None,
    ) -> AnalysisJob:
        """Admit one diagnostic job without activating new default analysis routing."""
        if not workdir.is_absolute():
            msg = "offline analysis workdir must be absolute"
            raise ValueError(msg)
        with self._lock:
            if self._closed:
                msg = "offline analysis service is shut down"
                raise RuntimeError(msg)
            if self._job is not None and not self._job.done.is_set():
                msg = "offline analysis resources are still occupied"
                raise RuntimeError(msg)
            native = engine.begin_offline_analysis(pad_id)
            try:
                loaded = _loaded_input(native.metadata())
                job = AnalysisJob(native, loaded, workdir, model, adapter or BeatWorkerAdapter())
                self._job = job
                Thread(
                    target=job._supervise, name="offline-analysis-supervisor", daemon=True
                ).start()
            except KeyError, OSError, RuntimeError, TypeError, ValueError:
                native.cancel()
                native.abort_unstarted()
                self._job = None
                raise
            return job

    def snapshot(self) -> JobSnapshot | None:
        """Return the current or most recent request state immediately."""
        with self._lock:
            return self._job.snapshot() if self._job is not None else None

    def shutdown(self) -> JobSnapshot | None:
        """Cancel admission and work, truthfully retaining any live key resources."""
        with self._lock:
            self._closed = True
            if self._job is None:
                return None
            if not self._job.done.is_set():
                self._job.cancel()
            return self._job.snapshot()
