"""Main-control recording intent, original admission and outer retirement.

No App/UI integration is installed by this module. The live-spool witness and
original left-press gate are required external authority, never no-op stand-ins.
"""

import math
from dataclasses import dataclass, replace
from threading import get_ident
from time import monotonic
from typing import TYPE_CHECKING, Literal, Protocol

from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.recording_worker import (
    RecordingRecoveryWorker,
    RecordingTakeWorker,
    recording_native_state,
)

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

    from flitzis_looper.models import ProjectState
    from flitzis_looper.recording_finalize import RecordingCompletion
    from flitzis_looper.recording_recovery import RecordingRecoveryObservation
    from flitzis_looper.recording_worker import (
        LiveSpoolWitness,
        RecordingNativeState,
        RecordingReady,
        RecordingWorkSnapshot,
    )
    from flitzis_looper_audio import AudioEngine, MixCaptureTicket


class RecordingGroupGate(Protocol):
    """The existing GlobalPlayback owner retains this original press capture.

    ``publish`` uses publish_captured_global_start(original, capture=ticket),
    returning False only for genuine deferred readiness/pressure. True means
    that the same existing batch owner tracks its Native acknowledgement; it
    never means effective recording. ``cancel`` fences this original group,
    including a coupled request queued before its effective frame.
    """

    def is_current(self) -> bool: ...
    def publish(self, ticket: MixCaptureTicket) -> bool: ...
    def cancel(self) -> None: ...


@dataclass(frozen=True, slots=True)
class RecordingStatus:
    """Metadata for UI/diagnostics; no field grants admission or Native authority."""

    phase: Literal["Idle", "Preparing", "Recording", "Finalizing", "Failed"] = "Idle"
    pending: Literal["start", "stop", "drain", "encode", "retire", "recovery"] | None = None
    busy: bool = False
    retired: bool = True
    attempt_id: str | None = None
    incomplete_directory: Path | None = None
    session_id: int | None = None
    options: RecordingOptions | None = None
    rate: int | None = None
    channels: int | None = None
    effective_recording: bool = False
    first_frame: int | None = None
    end_frame: int | None = None
    submitted_frames: int = 0
    written_frames: int = 0
    error: str | None = None
    notice: str | None = None
    retirement_uncertain: bool = False
    last_completion: RecordingCompletion | None = None
    recovery_observed_count: int = 0


@dataclass(slots=True)
class _TakeOwner:
    worker: RecordingTakeWorker
    options: RecordingOptions
    gate: RecordingGroupGate | None
    ready: RecordingReady | None = None
    start_requested: bool = False
    stop_intent: bool = False
    stop_requested: bool = False
    gate_cancelled: bool = False
    gate_cancel_error: str | None = None
    stream_failure_observed: bool = False
    stream_check_pending: bool = False
    error: str | None = None
    retired: bool = False


@dataclass(slots=True)
class _RecoveryOwner:
    worker: RecordingRecoveryWorker


@dataclass(slots=True)
class _RejectedGateOwner:
    gate: RecordingGroupGate


def _pressure(error: RuntimeError) -> bool:
    return "queue is full" in str(error) or "buffer may be full" in str(error)


def _require_current_group(gate: RecordingGroupGate) -> None:
    if not gate.is_current():
        message = "Original recording group is no longer current"
        raise ValueError(message)


class RecordingController:
    """One serial operation slot and one original owner until actual retirement.

    The optional fatal-stream callback belongs to App shutdown coordination. It
    must return True only after checked Native callback quiescence. The independent
    failure getter also observes stream death after another sticky capture error.
    Without a checked receipt, a dead stream remains visibly busy. Normal shutdown
    keeps audio alive through recording finalization.
    """

    def __init__(
        self,
        project: ProjectState,
        audio: AudioEngine,
        record_root: Path,
        *,
        live_spool_witness: LiveSpoolWitness,
        quiesce_failed_stream: Callable[[], bool] | None = None,
        stream_failed: Callable[[], bool] | None = None,
    ) -> None:
        self._project = project
        self._audio = audio
        self._root = record_root
        self._live_spool_witness = live_spool_witness
        self._quiesce_failed_stream = quiesce_failed_stream
        self._stream_failed = stream_failed
        self._thread = get_ident()
        self._owner: _TakeOwner | _RecoveryOwner | _RejectedGateOwner | None = None
        self._status = RecordingStatus()
        self._closing = False
        self._fatal_quiescence_requested = False
        self._recovery: tuple[RecordingRecoveryObservation, ...] = ()

    def _require_thread(self) -> None:
        if get_ident() != self._thread:
            message = "Recording intent and projections belong to the control thread"
            raise RuntimeError(message)

    def status(self) -> RecordingStatus:
        self._require_thread()
        return self._status

    def record_mix(self) -> None:
        """Right gesture: prepare one take or stop the original recording."""
        self._gesture(None)

    def record_group(self, capture_original: Callable[[], RecordingGroupGate | None]) -> None:
        """Capture the original left press once, synchronously, before preparation.

        The factory must retain that press's original timestamp/source/loop/restore
        membership through the existing GlobalPlayback controller. Busy/repeat
        gestures never call it; readiness never calls it again.
        """
        self._gesture(capture_original)

    def _gesture(self, capture_original: Callable[[], RecordingGroupGate | None] | None) -> None:
        self._require_thread()
        self.poll()
        if self._closing:
            self._status = replace(self._status, notice="Recording admission is closed")
            return
        if self._status.phase == "Recording":
            self._request_stop()
            return
        if self._owner is not None or self._status.phase == "Failed":
            self._status = replace(
                self._status, notice="Recording is busy or requires failure acknowledgement"
            )
            return
        gate = None
        try:
            options = RecordingOptions.model_validate(self._project.recording_options, strict=True)
            if capture_original is not None:
                gate = capture_original()
                if gate is None:
                    self._status = replace(
                        self._status, notice="No active or remembered recording group"
                    )
                    return
                _require_current_group(gate)
            worker = RecordingTakeWorker(
                self._audio, self._root, options, live_spool_witness=self._live_spool_witness
            )
            self._owner = _TakeOwner(worker, options, gate)
            self._fatal_quiescence_requested = False
            self._status = RecordingStatus(
                phase="Preparing",
                busy=True,
                retired=False,
                options=options,
                last_completion=self._status.last_completion,
            )
            worker.start()
        except (OSError, RuntimeError, ValueError, TypeError) as error:
            self._reject_before_worker(gate, str(error))

    def _reject_before_worker(self, gate: RecordingGroupGate | None, message: str) -> None:
        if gate is not None:
            try:
                gate.cancel()
            except (OSError, RuntimeError, ValueError) as error:
                self._owner = _RejectedGateOwner(gate)
                self._status = replace(
                    self._status,
                    phase="Failed",
                    busy=True,
                    retired=False,
                    pending="retire",
                    error=f"{message}; original group cancellation remains pending: {error}",
                )
                return
        self._status = replace(self._status, phase="Failed", error=message)

    def _cancel_gate(self, owner: _TakeOwner) -> None:
        if owner.gate is not None and not owner.gate_cancelled:
            try:
                owner.gate.cancel()
            except (OSError, RuntimeError, ValueError) as error:
                owner.gate_cancel_error = str(error)
            else:
                owner.gate_cancelled = True
                owner.gate_cancel_error = None

    def _fail(self, owner: _TakeOwner, message: str) -> None:
        owner.error = message
        self._cancel_gate(owner)
        owner.worker.cancel()

    def _admit_start(self, owner: _TakeOwner) -> None:
        if owner.ready is None or owner.start_requested or self._closing or owner.error is not None:
            return
        try:
            if owner.gate is not None:
                if not owner.gate.is_current():
                    self._fail(owner, "Original recording group changed during preparation")
                    return
                owner.start_requested = owner.gate.publish(owner.ready.ticket)
            else:
                owner.start_requested = self._audio.start_mix_capture(owner.ready.ticket, None)
        except RuntimeError as error:
            if not _pressure(error):
                self._fail(owner, str(error))
        except ValueError as error:
            self._fail(owner, str(error))

    def _request_stop(self) -> None:
        owner = self._owner
        if isinstance(owner, _TakeOwner):
            owner.stop_intent = True
            self._admit_stop(owner)

    def _admit_stop(self, owner: _TakeOwner) -> None:
        if not owner.stop_intent or owner.stop_requested or owner.ready is None:
            return
        try:
            owner.stop_requested = self._audio.stop_mix_capture(owner.ready.ticket, None)
        except RuntimeError as error:
            if not _pressure(error):
                self._fail(owner, str(error))
        except ValueError as error:
            self._fail(owner, str(error))

    def _observe_failed_stream(
        self, owner: _TakeOwner, native: RecordingNativeState | None
    ) -> None:
        if self._fatal_quiescence_requested:
            return
        if native is not None and native.failure == "stream":
            owner.stream_failure_observed = True
        if self._stream_failed is not None:
            owner.stream_check_pending = True
            failed = self._stream_failed()
            if type(failed) is not bool:
                message = "Native stream failure observation did not return a bool"
                raise TypeError(message)
            owner.stream_check_pending = False
            owner.stream_failure_observed |= failed
        if not owner.stream_failure_observed:
            return
        if owner.error is None:
            self._fail(
                owner, native.failure if native and native.failure else "Audio stream failed"
            )
        if self._quiesce_failed_stream is None:
            return
        quiesced = self._quiesce_failed_stream()
        if type(quiesced) is not bool:
            message = "Native failed-stream quiescence did not return a bool"
            raise TypeError(message)
        if not quiesced:
            message = "Native failed-stream callback quiescence is not complete"
            raise RuntimeError(message)
        owner.stream_failure_observed = False
        self._fatal_quiescence_requested = True

    def _project_take(self, owner: _TakeOwner, work: RecordingWorkSnapshot) -> None:
        native = work.native
        outcome = work.outcome
        error = (
            owner.error
            or owner.gate_cancel_error
            or (outcome.error if outcome is not None else None)
        )
        if error is None and native is not None:
            error = native.failure
        phase: Literal["Idle", "Preparing", "Recording", "Finalizing", "Failed"] = "Preparing"
        pending: Literal["start", "stop", "drain", "encode", "retire", "recovery"] | None = "start"
        if native is not None and native.started:
            phase, pending = "Recording", "stop" if owner.stop_intent else None
        if native is not None and native.rt_sealed:
            phase, pending = "Finalizing", "drain"
        if work.stage == "finalizing":
            phase, pending = "Finalizing", "encode"
        if error is not None or (outcome is not None and outcome.retirement_uncertain):
            phase = "Failed"
        if (
            (outcome is not None and outcome.retirement_uncertain)
            or owner.gate_cancel_error is not None
            or owner.stream_failure_observed
            or owner.stream_check_pending
        ):
            pending = "retire"
        if owner.retired:
            pending = None
        ready = owner.ready
        paths = ready.paths if ready is not None else work.paths
        completion = outcome.completion if outcome is not None else None
        self._status = RecordingStatus(
            phase=phase,
            pending=pending,
            busy=not owner.retired,
            retired=owner.retired,
            attempt_id=paths.attempt_id if paths is not None else self._status.attempt_id,
            incomplete_directory=paths.directory if paths is not None else None,
            session_id=native.session_id if native is not None else None,
            options=owner.options,
            rate=native.rate if native is not None else None,
            channels=native.channels if native is not None else None,
            effective_recording=(
                native is not None
                and native.started
                and not native.rt_sealed
                and native.failure is None
                and not (owner.stream_failure_observed or self._fatal_quiescence_requested)
            ),
            first_frame=native.first_frame if native is not None else None,
            end_frame=native.end_frame if native is not None else None,
            submitted_frames=native.submitted_frames if native is not None else 0,
            written_frames=native.written_frames if native is not None else 0,
            error=error,
            retirement_uncertain=(
                owner.gate_cancel_error is not None
                or owner.stream_failure_observed
                or owner.stream_check_pending
                or (outcome is not None and outcome.retirement_uncertain)
            ),
            last_completion=completion or self._status.last_completion,
        )

    def _poll_take(self, owner: _TakeOwner) -> None:
        work = owner.worker.snapshot()
        if work.ready is not None:
            owner.ready = work.ready
        try:
            self._observe_failed_stream(owner, work.native)
        except (OSError, RuntimeError, ValueError, TypeError) as error:
            self._fail(owner, f"Failed-stream quiescence remains pending: {error}")
        if owner.gate_cancel_error is not None or (
            work.outcome is not None and work.outcome.error is not None
        ):
            self._cancel_gate(owner)
        if work.outcome is None and owner.gate is not None and not owner.start_requested:
            try:
                if not owner.gate.is_current():
                    self._fail(owner, "Original recording group is no longer current")
            except (OSError, RuntimeError, ValueError) as error:
                self._fail(owner, str(error))
        if work.outcome is None and (
            work.native is None or (work.native.failure is None and not work.native.rt_sealed)
        ):
            self._admit_start(owner)
            self._admit_stop(owner)
        self._retire_take_if_done(owner, work)
        self._project_take(owner, work)
        if (
            owner.retired
            and work.outcome is not None
            and self._status.error is None
            and not self._status.retirement_uncertain
        ):
            self._owner = None
            self._status = RecordingStatus(last_completion=self._status.last_completion)

    def _retire_take_if_done(self, owner: _TakeOwner, work: RecordingWorkSnapshot) -> None:
        joined = owner.worker.join_if_done()
        if not work.cleanup_complete or not joined:
            return
        # Worker cleanup has cleared all of its tool/pin/ticket/engine fields.
        # Clear both control handoffs before the public admission boundary.
        owner.ready = None
        owner.retired = (
            owner.gate_cancel_error is None
            and not owner.stream_failure_observed
            and not owner.stream_check_pending
        ) and (
            owner.gate is None
            or owner.gate_cancelled
            or (owner.start_requested and work.native is not None and work.native.started)
        )
        if owner.retired:
            owner.gate = None

    def poll(self) -> None:
        """Always reachable main-control polling, including while App closes."""
        self._require_thread()
        owner = self._owner
        if isinstance(owner, _TakeOwner):
            self._poll_take(owner)
        elif isinstance(owner, _RecoveryOwner):
            work = owner.worker.snapshot()
            self._recovery = work.observations
            self._status = replace(
                self._status,
                error=work.error,
                recovery_observed_count=work.observed_count,
                phase="Failed" if work.error else "Preparing",
                retirement_uncertain=work.error is not None and not work.cleanup_complete,
            )
            joined = owner.worker.join_if_done()
            if work.cleanup_complete and joined:
                self._owner = None
                self._status = replace(
                    self._status,
                    busy=False,
                    retired=True,
                    pending=None,
                    phase="Failed" if work.error else "Idle",
                )
        elif isinstance(owner, _RejectedGateOwner):
            try:
                owner.gate.cancel()
            except (OSError, RuntimeError, ValueError) as error:
                self._status = replace(self._status, error=str(error))
            else:
                self._owner = None
                self._status = replace(self._status, busy=False, retired=True, pending=None)

    def acknowledge_failure(self) -> bool:
        """Acknowledgement cannot discard a live or uncertain owner."""
        self._require_thread()
        self.poll()
        if (
            self._status.phase != "Failed"
            or not self._status.retired
            or self._status.retirement_uncertain
        ):
            return False
        self._owner = None
        self._status = RecordingStatus(last_completion=self._status.last_completion)
        return True

    def discover_recovery(self) -> None:
        """Read-only discovery occupies the same serial slot; no Native restore."""
        self._require_thread()
        self.poll()
        if self._closing or self._owner is not None or self._status.phase == "Failed":
            self._status = replace(self._status, notice="Recording discovery is busy")
            return
        worker = RecordingRecoveryWorker(self._root)
        self._owner = _RecoveryOwner(worker)
        self._status = replace(
            self._status, phase="Preparing", pending="recovery", busy=True, retired=False
        )
        worker.start()

    def recovery_observations(self) -> tuple[RecordingRecoveryObservation, ...]:
        """Latest bounded window; observed_count also reports earlier observations."""
        self._require_thread()
        return self._recovery

    def _shutdown_take(self, owner: _TakeOwner, deadline: float) -> None:
        native = owner.worker.snapshot().native
        if owner.ready is not None:
            try:
                native = recording_native_state(self._audio.mix_capture_status(owner.ready.ticket))
            except (OSError, RuntimeError, ValueError, TypeError, KeyError) as error:
                self._fail(owner, f"Original recording shutdown status unavailable: {error}")
                return
        if native is None or not native.rt_sealed:
            if (native is not None and native.started) or owner.start_requested:
                if native is not None and not native.started:
                    self._cancel_gate(owner)
                # Same original session/next-admissible stop also handles a
                # queued future start. A stale started=False projection must
                # not turn an already-applied take into cancellation.
                self._request_stop()
            else:
                self._cancel_gate(owner)
                owner.worker.cancel()
        if monotonic() >= deadline:
            self._fail(owner, "Recording shutdown deadline expired; incomplete evidence retained")

    def prepare_shutdown(self, *, deadline: float) -> bool:
        """Poll first; keep audio alive until real recording ownership retires.

        Deadline expiry requests cancellation and reports failure. It never makes
        an alive thread, retained pin or unsealed Native image count as retired.
        """
        self._require_thread()
        if not math.isfinite(deadline):
            message = "Recording shutdown deadline must be finite"
            raise ValueError(message)
        self._closing = True
        self.poll()
        owner = self._owner
        if isinstance(owner, _RecoveryOwner):
            owner.worker.cancel()
        elif isinstance(owner, _TakeOwner) and not owner.retired:
            self._shutdown_take(owner, deadline)
        self.poll()
        return self._owner is None or self._status.retired

    def shut_down(self) -> None:
        """Final control release is permitted only after actual outer retirement."""
        self._require_thread()
        self.poll()
        if self._owner is not None and not self._status.retired:
            message = "Recording close is waiting for actual Native, worker and custody retirement"
            raise RuntimeError(message)
        self._owner = None
