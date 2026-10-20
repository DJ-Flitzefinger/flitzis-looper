"""One off-realtime recording operation and its original resource lifetime.

The controller owns gestures and Native start/stop admission. This worker owns
preparation, the original live spool witness, selected-codec work and cleanup.
Its component Events are private; a returned codec/finalizer never retires this
outer owner. The controller must also observe actual thread join before reuse.
"""

import sys
from collections import deque
from contextlib import ExitStack, closing
from dataclasses import dataclass, replace
from threading import Event, Lock, Thread
from typing import TYPE_CHECKING, Literal, Protocol, TypedDict, Unpack

from flitzis_looper.recording_codec import discover_recording_codecs, preflight_recording_codec
from flitzis_looper.recording_finalize import (
    RecordingFinalizationCommittedError,
    finalize_recording_take,
)
from flitzis_looper.recording_paths import (
    RecordingCustodyReleaseError,
    create_recording_namespace,
    hold_recording_namespace,
)
from flitzis_looper.recording_recovery import iter_recording_recovery

if TYPE_CHECKING:
    from contextlib import AbstractContextManager
    from pathlib import Path

    from flitzis_looper.recording_codec import RecordingCodecReadiness, RecordingCodecTools
    from flitzis_looper.recording_finalize import RecordingCompletion
    from flitzis_looper.recording_options import RecordingOptions
    from flitzis_looper.recording_paths import FileIdentity, RecordingPaths
    from flitzis_looper.recording_recovery import RecordingRecoveryObservation
    from flitzis_looper_audio import AudioEngine, MixCaptureTicket

_POLL_SECONDS = 0.02
_U64_MAX = (1 << 64) - 1


class LiveSpoolWitness(Protocol):
    """Required external live-writer FileID custody, with no unsafe fallback.

    The returned context must hold the original ordinary file against deletion
    while allowing its existing Native writer to write (Windows READ|WRITE
    sharing, no DELETE). It yields the held fstat FileID. It must classify an
    uncertain release as RecordingCustodyReleaseError. A sealed read pin denies
    the original writer's writes and cannot implement this protocol.
    """

    def __call__(self, path: Path) -> AbstractContextManager[FileIdentity]: ...


def _integer(value: object, name: str, *, positive: bool = False) -> int:
    if type(value) is not int or not int(positive) <= value <= _U64_MAX:
        message = f"Native recording {name} is not a strict unsigned extent"
        raise ValueError(message)
    return value


def _flag(status: dict[str, object], name: str) -> bool:
    value = status[name]
    if type(value) is not bool:
        message = f"Native recording {name} is not a strict boolean"
        raise ValueError(message)
    return value


def _frame(status: dict[str, object], name: str) -> int | None:
    value = status[name]
    return None if value is None else _integer(value, name)


@dataclass(frozen=True, slots=True)
class RecordingNativeState:
    """An observation of the original ticket, without publication permission."""

    session_id: int
    rate: int
    channels: int
    writer_ready: bool
    started: bool
    rt_sealed: bool
    rt_released: bool
    worker_closed: bool
    worker_joined: bool
    first_frame: int | None
    end_frame: int | None
    submitted_frames: int
    written_frames: int
    empty: bool
    failure: str | None

    @property
    def quiescent(self) -> bool:
        """Require actual pool release, file close and writer join together."""
        return self.rt_sealed and self.rt_released and self.worker_closed and self.worker_joined


def recording_native_state(status: dict[str, object]) -> RecordingNativeState:
    """Parse projection fields strictly; finalization still checks the live owner."""
    failure = status["failure"]
    if failure is not None and type(failure) is not str:
        message = "Native recording failure is not an optional label"
        raise ValueError(message)
    return RecordingNativeState(
        session_id=_integer(status["session_id"], "session_id", positive=True),
        rate=_integer(status["sample_rate_hz"], "sample_rate_hz", positive=True),
        channels=_integer(status["channels"], "channels", positive=True),
        writer_ready=_flag(status, "writer_ready"),
        started=_flag(status, "started"),
        rt_sealed=_flag(status, "rt_sealed"),
        rt_released=_flag(status, "rt_released"),
        worker_closed=_flag(status, "worker_closed"),
        worker_joined=_flag(status, "worker_joined"),
        first_frame=_frame(status, "first_frame"),
        end_frame=_frame(status, "end_frame"),
        submitted_frames=_integer(status["submitted_frames"], "submitted_frames"),
        written_frames=_integer(status["written_frames"], "written_frames"),
        empty=_flag(status, "empty"),
        failure=failure,
    )


@dataclass(frozen=True, slots=True)
class RecordingReady:
    """Private control handoff of the exact original ticket and prepared geometry."""

    ticket: MixCaptureTicket
    paths: RecordingPaths
    session_id: int
    rate: int
    channels: int


@dataclass(frozen=True, slots=True)
class RecordingWorkerOutcome:
    """Released metadata only; never retain a live exception or traceback here."""

    error: str | None = None
    completion: RecordingCompletion | None = None
    empty: bool = False
    cancelled: bool = False
    retirement_uncertain: bool = False


@dataclass(frozen=True, slots=True)
class RecordingWorkSnapshot:
    """Private handoff; UI must use the controller's metadata projection."""

    stage: Literal["preparing", "waiting", "finalizing", "finished"]
    ready: RecordingReady | None
    paths: RecordingPaths | None
    native: RecordingNativeState | None
    outcome: RecordingWorkerOutcome | None
    cleanup_complete: bool
    thread_done: bool


class _RecordingWorkChanges(TypedDict, total=False):
    stage: Literal["preparing", "waiting", "finalizing", "finished"]
    ready: RecordingReady | None
    paths: RecordingPaths | None
    native: RecordingNativeState | None
    outcome: RecordingWorkerOutcome | None
    cleanup_complete: bool
    thread_done: bool


@dataclass(slots=True)
class _TakeResources:
    stack: ExitStack
    paths: RecordingPaths | None = None
    ticket: MixCaptureTicket | None = None
    tools: RecordingCodecTools | None = None
    readiness: RecordingCodecReadiness | None = None
    spool_identity: FileIdentity | None = None
    preflight_started: bool = False
    finalizer_started: bool = False

    def clear(self) -> None:
        self.ticket = self.readiness = self.tools = None
        self.paths = None
        self.spool_identity = None


def _failure(
    error: OSError | RuntimeError | ValueError | TypeError | KeyError,
) -> RecordingWorkerOutcome:
    completion = (
        error.completion if isinstance(error, RecordingFinalizationCommittedError) else None
    )
    uncertain = isinstance(
        error, RecordingCustodyReleaseError | RecordingFinalizationCommittedError
    )
    return RecordingWorkerOutcome(
        error=f"{type(error).__name__}: {error}",
        completion=completion,
        cancelled=isinstance(error, InterruptedError),
        retirement_uncertain=uncertain,
    )


class RecordingTakeWorker:
    """One take, one Thread and no start/stop gesture or alternate supervisor.

    Uncertain component/custody retirement retains its resource box and rejects
    reuse. This module exposes no flag setter that could invent its resolution.
    """

    def __init__(
        self,
        audio: AudioEngine,
        record_root: Path,
        options: RecordingOptions,
        *,
        live_spool_witness: LiveSpoolWitness,
    ) -> None:
        self._audio: AudioEngine | None = audio
        self._root = record_root
        self._options = options
        self._live_spool_witness = live_spool_witness
        self._lock = Lock()
        self._wake = Event()
        self._cancel = Event()
        self._done = Event()
        self._preflight_retired = Event()
        self._finalizer_retired = Event()
        self._resources: _TakeResources | None = None
        self._snapshot = RecordingWorkSnapshot(
            stage="preparing",
            ready=None,
            paths=None,
            native=None,
            outcome=None,
            cleanup_complete=False,
            thread_done=False,
        )
        self._thread = Thread(target=self._run, name="looper-recording-take")

    def start(self) -> None:
        """Start once, after the main controller has installed this owner."""
        if self._thread.ident is not None or self._done.is_set():
            message = "Recording worker has already been started"
            raise RuntimeError(message)
        try:
            self._thread.start()
        except RuntimeError as error:
            self._audio = None
            self._update(
                stage="finished",
                outcome=RecordingWorkerOutcome(error=str(error)),
                cleanup_complete=True,
                thread_done=True,
            )
            self._done.set()

    def cancel(self) -> None:
        """Cancel original work; no Event is promoted to Native closure."""
        self._cancel.set()
        self._wake.set()

    def snapshot(self) -> RecordingWorkSnapshot:
        """Read one bounded handoff without touching UI or filesystem state."""
        with self._lock:
            return self._snapshot

    def join_if_done(self) -> bool:
        """Join only a thread that has actually returned; never wait on live RT."""
        if not self._done.is_set() or self._thread.is_alive():
            return False
        if self._thread.ident is None:
            return True  # Thread creation failed before any Native operation.
        self._thread.join(timeout=0)
        return not self._thread.is_alive()

    def _update(self, **changes: Unpack[_RecordingWorkChanges]) -> None:
        with self._lock:
            self._snapshot = replace(self._snapshot, **changes)

    def _observe(self, resources: _TakeResources) -> RecordingNativeState:
        assert self._audio is not None
        assert resources.ticket is not None
        native = recording_native_state(self._audio.mix_capture_status(resources.ticket))
        if native.session_id != resources.ticket.session_id():
            message = "recording original Native session changed"
            raise ValueError(message)
        self._update(native=native)
        return native

    def _wait(self) -> None:
        self._wake.wait(_POLL_SECONDS)
        self._wake.clear()

    def _wait_ready(self, resources: _TakeResources) -> RecordingNativeState:
        while True:
            state = self._observe(resources)
            if self._cancel.is_set():
                message = "recording preparation cancelled"
                raise InterruptedError(message)
            if state.failure is not None:
                raise RuntimeError(state.failure)
            if state.writer_ready:
                return state
            self._wait()

    def _prepare(self, resources: _TakeResources) -> None:
        assert self._audio is not None
        if self._cancel.is_set():
            message = "recording preparation cancelled before namespace creation"
            raise InterruptedError(message)
        resources.paths = create_recording_namespace(self._root, self._options)
        self._update(paths=resources.paths)
        resources.stack.enter_context(hold_recording_namespace(resources.paths))
        if self._cancel.is_set():
            message = "recording preparation cancelled before Native admission"
            raise InterruptedError(message)
        resources.ticket = self._audio.prepare_mix_capture(str(resources.paths.spool_path))
        state = self._wait_ready(resources)
        resources.spool_identity = resources.stack.enter_context(
            self._live_spool_witness(resources.paths.spool_path)
        )
        device, inode = resources.spool_identity
        _integer(device, "live spool device")
        _integer(inode, "live spool inode", positive=True)
        resources.tools = discover_recording_codecs()
        ready_directory = resources.paths.directory / "codec-readiness"
        ready_directory.mkdir()
        resources.preflight_started = True
        resources.readiness = preflight_recording_codec(
            resources.tools,
            self._options,
            rate=state.rate,
            channels=state.channels,
            directory=ready_directory,
            cancel=self._cancel,
            retired=self._preflight_retired,
        )
        if not self._preflight_retired.is_set():
            message = "recording codec preflight retirement remains uncertain"
            raise RuntimeError(message)
        self._update(
            stage="waiting",
            ready=RecordingReady(
                resources.ticket, resources.paths, state.session_id, state.rate, state.channels
            ),
        )

    def _capture_and_finalize(self, resources: _TakeResources) -> RecordingWorkerOutcome:
        assert self._audio is not None
        assert resources.ticket is not None
        while True:
            state = self._observe(resources)
            if state.quiescent:
                break
            if self._cancel.is_set():
                self._audio.cancel_mix_capture(resources.ticket)
            self._wait()
        if state.failure is not None or self._cancel.is_set():
            return RecordingWorkerOutcome(
                error=state.failure or "recording cancelled",
                cancelled=self._cancel.is_set(),
            )
        if state.empty or not state.started:
            return RecordingWorkerOutcome(error="recording captured no frames", empty=True)
        assert resources.paths is not None
        assert resources.readiness is not None
        assert resources.spool_identity is not None
        self._update(stage="finalizing")
        resources.finalizer_started = True
        completion = finalize_recording_take(
            self._audio,
            resources.ticket,
            resources.paths,
            resources.readiness,
            spool_identity=resources.spool_identity,
            cancel=self._cancel,
            retired=self._finalizer_retired,
        )
        return RecordingWorkerOutcome(
            completion=completion,
            retirement_uncertain=not self._finalizer_retired.is_set(),
        )

    def _settle_native(self, resources: _TakeResources) -> bool:
        if resources.ticket is None:
            return True
        assert self._audio is not None
        while True:
            state = self._observe(resources)
            if state.quiescent:
                return True
            self._audio.cancel_mix_capture(resources.ticket)
            self._wait()

    def _release(
        self, resources: _TakeResources, outcome: RecordingWorkerOutcome
    ) -> RecordingWorkerOutcome:
        try:
            native_settled = self._settle_native(resources)
        except RuntimeError, ValueError, TypeError, KeyError:
            native_settled = False
        components_retired = (
            not resources.preflight_started or self._preflight_retired.is_set()
        ) and (not resources.finalizer_started or self._finalizer_retired.is_set())
        if outcome.retirement_uncertain or not native_settled or not components_retired:
            return replace(
                outcome,
                retirement_uncertain=True,
                error=outcome.error or "recording outer retirement remains unproven",
            )
        try:
            resources.stack.close()
        except OSError as error:
            return replace(
                outcome, error=f"{type(error).__name__}: {error}", retirement_uncertain=True
            )
        resources.clear()
        self._resources = None
        self._audio = None
        self._update(ready=None, cleanup_complete=True)
        return outcome

    def _run(self) -> None:
        resources = _TakeResources(ExitStack())
        self._resources = resources
        outcome: RecordingWorkerOutcome | None = None
        try:
            try:
                self._prepare(resources)
                outcome = self._capture_and_finalize(resources)
            except (OSError, RuntimeError, ValueError, TypeError, KeyError) as error:
                outcome = _failure(error)
            # The except binding/traceback has ended before outer Native custody
            # is released. Only pure metadata crosses to the controller.
            assert outcome is not None
            self._update(outcome=outcome)
            outcome = self._release(resources, outcome)
        finally:
            unexpected = sys.exception()
            if unexpected is not None:
                outcome = RecordingWorkerOutcome(
                    error=f"unexpected recording worker failure: {type(unexpected).__name__}",
                    retirement_uncertain=True,
                )
            self._update(stage="finished", outcome=outcome, thread_done=True)
            self._done.set()


@dataclass(frozen=True, slots=True)
class RecordingRecoverySnapshot:
    """Bounded released observations; none authenticate past Native permission."""

    observations: tuple[RecordingRecoveryObservation, ...]
    observed_count: int
    error: str | None
    cleanup_complete: bool


class RecordingRecoveryWorker:
    """Discovery uses the same controller slot and one bounded observation window."""

    def __init__(self, root: Path) -> None:
        self._root = root
        self._cancel = Event()
        self._done = Event()
        self._lock = Lock()
        self._snapshot = RecordingRecoverySnapshot((), 0, None, cleanup_complete=False)
        self._thread = Thread(target=self._run, name="looper-recording-discovery")

    def start(self) -> None:
        if self._thread.ident is not None or self._done.is_set():
            message = "Recording discovery has already been started"
            raise RuntimeError(message)
        try:
            self._thread.start()
        except RuntimeError as error:
            with self._lock:
                self._snapshot = RecordingRecoverySnapshot((), 0, str(error), cleanup_complete=True)
            self._done.set()

    def cancel(self) -> None:
        self._cancel.set()

    def snapshot(self) -> RecordingRecoverySnapshot:
        with self._lock:
            return self._snapshot

    def join_if_done(self) -> bool:
        if not self._done.is_set() or self._thread.is_alive():
            return False
        if self._thread.ident is None:
            return True
        self._thread.join(timeout=0)
        return not self._thread.is_alive()

    def _checkpoint(self) -> None:
        if self._cancel.is_set():
            message = "recording discovery cancelled"
            raise InterruptedError(message)

    def _run(self) -> None:
        observations: deque[RecordingRecoveryObservation] = deque(maxlen=32)
        count = 0
        error_message = None
        released = False
        try:
            with closing(
                iter_recording_recovery(self._root, checkpoint=self._checkpoint)
            ) as entries:
                for observation in entries:
                    observations.append(observation)
                    count += 1
                    with self._lock:
                        self._snapshot = RecordingRecoverySnapshot(
                            tuple(observations), count, None, cleanup_complete=False
                        )
            released = True
        except InterruptedError:
            released = True
        except (OSError, ValueError, TypeError) as error:
            error_message = f"{type(error).__name__}: {error}"
            released = not isinstance(error, RecordingCustodyReleaseError)
        finally:
            unexpected = sys.exception()
            if unexpected is not None:
                error_message = (
                    f"unexpected recording discovery failure: {type(unexpected).__name__}"
                )
                released = False
            with self._lock:
                self._snapshot = RecordingRecoverySnapshot(
                    tuple(observations), count, error_message, released
                )
            self._done.set()
