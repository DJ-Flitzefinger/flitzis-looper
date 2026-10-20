"""One controller path for source-bound loop, seek and processing readiness."""

import math
import time
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING

from flitzis_looper.controller.transport.key_lock_status import KeyLockStatus

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.control_diagnostics import ConsoleDiagnostics
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper_audio import CapturedInputPadTrigger, CapturedPadStart, ResidentWindowTicket


@dataclass(frozen=True, slots=True)
class _Intent:
    path: str | None
    start: float
    end: float | None
    auto: bool
    bars: float
    key_lock: bool
    key_lock_override: bool | None


@dataclass(slots=True)
class _Pending:
    requested: _Intent
    previous: _Intent
    start: float
    end: float | None
    position: float | None
    after_ack: Callable[[ResidentWindowTicket], bool | None] | None
    owner: tuple[object, ...]
    expected_source_generation: int | None = None
    seek_only: bool = False
    key_lock_only: bool = False
    scalar: bool = False
    scalar_submitted: bool = False
    scalar_request: int | None = None
    ticket: ResidentWindowTicket | None = None
    waiting_ticket: ResidentWindowTicket | None = None
    waiting_intent: _Intent | None = None
    waiting_owner: tuple[object, ...] | None = None
    attempts: int = 0
    deadline: float = 0.0
    unconfirmed: bool = False


@dataclass(slots=True)
class _CapturedTrigger:
    trigger: CapturedInputPadTrigger
    ticket: ResidentWindowTicket | None = None
    attempts: int = 0
    deadline: float = 0.0
    cancel_requested: bool = False


@dataclass(slots=True)
class _CapturedStart:
    start: CapturedPadStart
    cache_backed: bool
    requested: _Intent | None
    ticket: ResidentWindowTicket | None = None
    attempts: int = 0
    deadline: float = 0.0
    published: bool = False
    geometry_acknowledged: bool = False
    dependency: ResidentWindowTicket | None = None


class ResidencyController:
    """Keep requested project intent separate from native acknowledged playback.

    Edit requests retain one latest intent per pad. Captured attacks remain
    independent original owners through preparation and publication. Native owns
    PCM, the fixed worker queue, cancellation and adoption. Polling observes ACK;
    it never moves a running playhead to compensate for timing.
    """

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._audio = transport._audio
        self._pending: dict[int, _Pending] = {}
        self._launched: dict[int, ResidentWindowTicket] = {}
        self._effective: dict[int, _Intent] = {}
        # Geometry can be acknowledged while processing-mode feedback is absent.
        # Values here provide geometry only; their requested mode is never effective.
        self._acknowledged_geometry: dict[int, _Intent] = {}
        self._errors: dict[int, str] = {}
        self._error_sources: dict[int, tuple[object, ...]] = {}
        self._launch_errors: dict[int, str] = {}
        self._confirmed_modes: dict[int, tuple[tuple[object, ...], bool]] = {}
        self._closed = False
        self._captured_triggers: dict[int, _CapturedTrigger] = {}
        self._captured_starts: dict[int, _CapturedStart] = {}
        self._diagnostics: ConsoleDiagnostics | None = None
        transport._on_frame_render_callbacks.append(self.poll)

    def _bind_diagnostics(self, diagnostics: ConsoleDiagnostics) -> None:
        """Bind optional reporting on this existing Python control owner."""
        self._diagnostics = diagnostics

    def _intent(self, sample_id: int) -> _Intent:
        return _Intent(
            self._project.sample_paths[sample_id],
            self._project.pad_loop_start_s[sample_id],
            self._project.pad_loop_end_s[sample_id],
            self._project.pad_loop_auto[sample_id],
            self._project.pad_loop_bars[sample_id],
            self._project.resolve_pad_key_lock(sample_id),
            self._project.pad_key_lock[sample_id],
        )

    def remember(self, sample_id: int) -> None:
        """Capture the effective intent before a performer changes requested fields."""
        if sample_id not in self._pending:
            intent = self._intent(sample_id)
            geometry = self._acknowledged_geometry.get(sample_id)
            if geometry is not None:
                intent = self._with_geometry(intent, geometry)
            mode = self._observe_mode(sample_id)
            if geometry is not None and mode is None:
                previous = self._effective.get(sample_id)
                if previous is None:
                    return
                mode = previous.key_lock
            self._effective[sample_id] = intent if mode is None else replace(intent, key_lock=mode)

    def _ensure_start_assignment_ready(self, sample_id: int) -> None:
        """An implicit Project loop cannot start until its incoming receipt is projected."""
        self._transport.ensure_current_source_assignment_ready(sample_id)

    def _mode_feedback(self, sample_id: int) -> dict[str, object] | None:
        """Read only the engine snapshot belonging to this actual current source."""
        feedback = self._audio.pad_key_lock_status(sample_id)
        binding = self._audio.current_input_runtime_pad_binding(sample_id)
        if not isinstance(feedback, dict) or binding is None:
            return None
        metadata = binding.metadata()
        if (
            feedback.get("source_id") != metadata.get("source_id")
            or feedback.get("source_generation") != metadata.get("source_generation")
            or not isinstance(feedback.get("effective"), bool)
            or not isinstance(feedback.get("ready"), bool)
        ):
            return None
        return feedback

    @staticmethod
    def _mode_identity(feedback: dict[str, object]) -> tuple[object, ...]:
        return (
            feedback.get("source_id"),
            feedback.get("source_generation"),
            feedback.get("source_identity"),
        )

    def _observe_mode(self, sample_id: int) -> bool | None:
        try:
            feedback = self._mode_feedback(sample_id)
        except RuntimeError, ValueError:
            # Feedback availability cannot manufacture confirmation or prevent
            # a fresh preparation from obtaining current source authority.
            return None
        if feedback is None:
            return None
        effective = feedback["effective"]
        if feedback.get("state") == "waiting" and isinstance(effective, bool):
            # An armed voice can return to dry readiness on start/resume without
            # creating a new controller transaction. Use its actual current feed.
            return effective
        # A terminal native error still identifies the actual audio baseline.
        # It can arrive after wet output but before the UI observed readiness.
        terminal_error = feedback.get("error")
        if (
            feedback["ready"] is True or (isinstance(terminal_error, str) and bool(terminal_error))
        ) and isinstance(effective, bool):
            self._confirmed_modes[sample_id] = (self._mode_identity(feedback), effective)
            return effective
        confirmed = self._confirmed_modes.get(sample_id)
        return confirmed[1] if confirmed and confirmed[0] == self._mode_identity(feedback) else None

    def key_lock_status(self, sample_id: int) -> KeyLockStatus:
        """Expose source-bound confirmed mode without treating a queue or Window ACK as ON."""
        if self._project.sample_paths[sample_id] is None:
            return KeyLockStatus(requested=False, effective=False)
        pending = self._pending.get(sample_id)
        native_waiting = False
        error = self._current_error(sample_id)
        effective = None
        try:
            feedback = self._mode_feedback(sample_id)
        except (RuntimeError, ValueError) as native_error:
            feedback = None
            error = error or str(native_error)
        if feedback is not None:
            mode = feedback["effective"]
            feedback_error = feedback.get("error")
            native_waiting = feedback.get("state") == "waiting" and feedback["ready"] is False
            if (
                feedback["ready"] is True
                or native_waiting
                or (isinstance(feedback_error, str) and bool(feedback_error))
            ) and isinstance(mode, bool):
                effective = mode
            else:
                confirmed = self._confirmed_modes.get(sample_id)
                if confirmed and confirmed[0] == self._mode_identity(feedback):
                    effective = confirmed[1]
            if isinstance(feedback_error, str) and feedback_error:
                error = error or feedback_error
        if self._diagnostics is not None:
            self._diagnostics.observe("key_lock", error, pad_id=sample_id)
        return KeyLockStatus(
            self._project.resolve_pad_key_lock(sample_id),
            effective,
            pending=pending is not None or native_waiting,
            unconfirmed=pending is not None and pending.unconfirmed,
            error=error,
        )

    def report_key_lock_error(self, sample_id: int, error: str) -> None:
        """Keep an individual synchronous broadcast failure visible without aborting peers."""
        self._record_error(sample_id, error)

    def _error_source(self, sample_id: int) -> tuple[object, ...]:
        try:
            binding = self._audio.current_input_runtime_pad_binding(sample_id)
            metadata = binding.metadata() if binding is not None else {}
        except RuntimeError, ValueError:
            metadata = {}
        return (
            self._project.sample_paths[sample_id],
            metadata.get("source_id"),
            metadata.get("source_generation"),
        )

    def _record_error(self, sample_id: int, error: str, pending: _Pending | None = None) -> None:
        self._errors[sample_id] = error
        self._error_sources[sample_id] = (
            (pending.requested.path, *pending.owner[1:3])
            if pending is not None
            else self._error_source(sample_id)
        )
        if self._diagnostics is not None:
            self._diagnostics.observe(
                "resident_preparation",
                error,
                pad_id=sample_id,
                request=pending.scalar_request if pending is not None else None,
            )

    def _current_error(self, sample_id: int) -> str | None:
        error = self._errors.get(sample_id)
        if error is None or self._error_sources.get(sample_id) != self._error_source(sample_id):
            return None
        return error

    def status(self, sample_id: int) -> tuple[str | None, str | None]:
        """Expose pending/error without presenting requested fields as effective."""
        pending = self._pending.get(sample_id)
        error = self._current_error(sample_id) or self._launch_errors.get(sample_id)
        if pending is not None:
            if pending.unconfirmed:
                return "unconfirmed", error
            status = pending.ticket.publication_status() if pending.ticket else "preparing"
            return status, error
        return self._captured_start_status(sample_id), error

    def _captured_start_status(self, sample_id: int) -> str | None:
        for pending in self._captured_starts.values():
            if pending.start.diagnostic_slot != sample_id or pending.start.state() in {
                "applied",
                "rejected",
            }:
                continue
            if pending.published:
                return "pending"
            status = pending.ticket.publication_status() if pending.ticket else "preparing"
            return "pending" if status == "accepted" else status
        return None

    def report_launch_error(
        self, sample_id: int, error: str | None, *, operation: str = "pad_start"
    ) -> None:
        """Retain control diagnostics without treating the old reason as admission authority."""
        if error is None:
            self._launch_errors.pop(sample_id, None)
        else:
            self._launch_errors[sample_id] = error
        if self._diagnostics is not None:
            self._diagnostics.observe(operation, error, pad_id=sample_id, channel="pad_control")

    def _global_start_dependency(
        self, sample_id: int
    ) -> tuple[float, float | None, ResidentWindowTicket] | None:
        """Retain an existing edit's real request and original requested geometry.

        A Python pending row without a native ticket has no durable ACK owner.
        Reject before GLOBAL capture instead of attaching a later matching request.
        """
        pending = self._pending.get(sample_id)
        if pending is None:
            return None
        if pending.unconfirmed or pending.ticket is None:
            message = "global start unavailable: resident edit has no native request owner"
            raise ValueError(message)
        status = pending.ticket.publication_status()
        if status in {"failed", "cancelled", "rejected"}:
            message = pending.ticket.error() or "original global resident dependency failed"
            raise ValueError(message)
        return pending.start, pending.end, pending.ticket

    def acknowledged_region(self, sample_id: int) -> tuple[float, float | None] | None:
        """Return previous native geometry while another region is preparing."""
        pending = self._pending.get(sample_id)
        if pending is None:
            return None
        previous = pending.previous
        if not previous.auto:
            return previous.start, previous.end
        return self._transport.loop.region_for_intent(
            sample_id,
            start=previous.start,
            end=previous.end,
            auto=previous.auto,
            bars=previous.bars,
        )

    def _cache_backed(self, sample_id: int) -> bool:
        try:
            descriptor = self._audio.loaded_residency(sample_id)
        except ValueError as error:
            if str(error) in {"source/window is pending native adoption", "sample is not loaded"}:
                # Initial saved settings still go through native cold-intent guards.
                return False
            raise
        return (
            isinstance(descriptor, dict)
            and descriptor.get("cache_backed") is not False
            and isinstance(descriptor.get("source_identity"), int)
        )

    def _owner(self, sample_id: int) -> tuple[object, ...]:
        try:
            descriptor = self._audio.loaded_residency(sample_id)
        except ValueError as error:
            if str(error) not in {
                "source/window is pending native adoption",
                "sample is not loaded",
            }:
                raise
            descriptor = {}
        binding = self._audio.current_input_runtime_pad_binding(sample_id)
        metadata = binding.metadata() if binding is not None else {}
        return (
            descriptor.get("source_identity"),
            metadata.get("source_id"),
            metadata.get("source_generation"),
            metadata.get("authority_revision"),
            self._project.pad_timing_intent[sample_id],
            self._project.manual_bpm[sample_id],
            self._project.pad_grid_anchor_s[sample_id],
            self._project.pad_grid_offset_samples[sample_id],
        )

    def publish_loop(
        self,
        sample_id: int,
        start: float,
        end: float | None,
        *,
        expected_source_generation: int | None = None,
    ) -> None:
        """Publish loop and requested Key Lock together through one native transaction."""
        if not self._cache_backed(sample_id):
            self._audio.set_pad_loop_region(
                sample_id,
                start,
                end,
                **self._transport.source_generation_options(expected_source_generation),
            )
            self._effective[sample_id] = self._intent(sample_id)
            self._acknowledged_geometry.pop(sample_id, None)
            return
        self._request(sample_id, start, end, expected_source_generation=expected_source_generation)

    def apply_key_lock(
        self, sample_id: int, *, force: bool = False, expected_source_generation: int | None = None
    ) -> None:
        """Admit resolved intent without changing its inherited or explicit origin."""
        enabled = self._project.resolve_pad_key_lock(sample_id)
        self.remember(sample_id)
        previous = self._pending.get(sample_id)
        if previous is not None:
            # Changing origin alone retains the same native work and bounded deadline.
            requested = self._intent(sample_id)
            if (
                replace(previous.requested, key_lock_override=requested.key_lock_override)
                == requested
            ):
                previous.requested = requested
        if (
            self._reusable_mode_request(sample_id, previous)
            and (
                previous is None
                or previous.expected_source_generation == expected_source_generation
            )
            and previous is not None
            and previous.requested.key_lock is enabled
        ):
            return
        if (
            not force
            and previous is None
            and self._settled_mode(sample_id) is enabled
            and self._current_error(sample_id) is None
        ):
            return
        if (
            previous is not None
            and not previous.key_lock_only
            and self._same_owner(sample_id, previous)
        ):
            # A genuine earlier loop/seek/launch remains one coupled transaction.
            self._request(
                sample_id,
                previous.start,
                previous.end,
                position=previous.position,
                after_ack=previous.after_ack,
                expected_source_generation=expected_source_generation,
            )
        else:
            start, end = self._transport.loop.requested_region(sample_id)
            self._request(
                sample_id,
                start,
                end,
                key_lock_only=True,
                scalar=not self._cache_backed(sample_id),
                expected_source_generation=expected_source_generation,
            )

    def _settled_mode(self, sample_id: int) -> bool | None:
        feedback = self._mode_feedback(sample_id)
        if (
            feedback is None
            or feedback["ready"] is not True
            or feedback.get("state") == "waiting"
            or feedback.get("error")
        ):
            return None
        mode = feedback["effective"]
        return mode if isinstance(mode, bool) else None

    def _reusable_mode_request(self, sample_id: int, pending: _Pending | None) -> bool:
        if (
            pending is None
            or pending.requested != self._intent(sample_id)
            or not self._same_owner(sample_id, pending)
        ):
            return False
        ticket = pending.ticket
        return ticket is None or (
            ticket.is_current()
            and ticket.publication_status() in {"preparing", "pending", "adopting", "accepted"}
        )

    def seek(
        self, sample_id: int, position: float, *, expected_source_generation: int | None = None
    ) -> None:
        """Update a seek projection only after matching adoption acknowledgement."""
        if not self._cache_backed(sample_id):
            duration = self._project.sample_durations[sample_id]
            if duration is not None and math.isfinite(duration) and duration >= 0:
                position = min(position, duration)
            self._audio.seek_sample(
                sample_id,
                position,
                **self._transport.source_generation_options(expected_source_generation),
            )
            self._transport._session.pad_playhead_s[sample_id] = position
            return
        self.remember(sample_id)
        start, end = self._transport.loop.requested_region(sample_id)
        self._request(
            sample_id,
            start,
            end,
            position=position,
            expected_source_generation=expected_source_generation,
        )

    def start(
        self,
        sample_id: int,
        start: float,
        end: float | None,
        play: Callable[[], None] | None = None,
        *,
        prepared_play: Callable[[ResidentWindowTicket], bool] | None = None,
        expected_source_generation: int | None = None,
        captured_start: CapturedPadStart | None = None,
    ) -> None:
        """Retain the input's launch intent until the required source context is ready."""
        self._ensure_start_assignment_ready(sample_id)
        if captured_start is not None:
            self._retain_captured_start(sample_id, captured_start)
            return
        if play is None:
            message = "Ordinary start requires its original capture or playback callback"
            raise ValueError(message)
        if not self._cache_backed(sample_id):
            self._audio.set_pad_loop_region(
                sample_id,
                start,
                end,
                **self._transport.source_generation_options(expected_source_generation),
            )
            play()
            return
        self.remember(sample_id)
        launch: Callable[[ResidentWindowTicket], bool | None] = prepared_play or (
            lambda _ticket: play()
        )
        pending = self._pending.get(sample_id)
        if self._continue_start(
            sample_id, pending, start, end, expected_source_generation=expected_source_generation
        ):
            assert pending is not None
            # An identical gesture changes launch time, not preparation ownership.
            # Keep the existing retry budget/deadline and actual native ACK gate.
            pending.after_ack = launch
            self._launch_errors.pop(sample_id, None)
            if pending.ticket is not None and pending.ticket.publication_status() == "accepted":
                self._poll_one(sample_id, pending)
            return
        self._request(
            sample_id,
            start,
            end,
            after_ack=launch,
            expected_source_generation=expected_source_generation,
        )

    def _retain_captured_start(self, sample_id: int, start: CapturedPadStart) -> None:
        identity = id(start)
        if identity in self._captured_starts:
            return
        if self._diagnostics is not None:
            self._diagnostics.begin_attempt("pad_control", sample_id)
        if len(self._captured_starts) >= 256:
            start.cancel()
            message = "Captured pad start capacity exhausted"
            raise RuntimeError(message)
        cache_backed = self._cache_backed(sample_id)
        try:
            dependency = self._captured_start_dependency(sample_id) if cache_backed else None
        except ValueError as error:
            start.cancel()
            self.report_launch_error(sample_id, str(error))
            return
        pending = _CapturedStart(
            start,
            cache_backed,
            self._intent(sample_id),
            deadline=time.monotonic() + 30.0,
            dependency=dependency,
        )
        self._captured_starts[identity] = pending
        self._launch_errors.pop(sample_id, None)
        if self._poll_captured_start(pending):
            self._captured_starts.pop(identity, None)

    def _captured_start_dependency(self, sample_id: int) -> ResidentWindowTicket | None:
        """Freeze an independent edit's genuine request before any readiness polling."""
        edit = self._pending.get(sample_id)
        if edit is None:
            return None
        if edit.unconfirmed or edit.ticket is None:
            message = "captured start unavailable: resident edit has no native request owner"
            raise ValueError(message)
        return edit.ticket

    def observe_captured_start(self, start: CapturedPadStart) -> None:
        """Observe an already-published original attack without preparing or publishing it."""
        if any(pending.start.same_owner(start) for pending in self._captured_starts.values()):
            return
        if self._diagnostics is not None:
            self._diagnostics.begin_attempt("pad_control", start.diagnostic_slot)
        if len(self._captured_starts) >= 256:
            start.cancel()
            message = "Captured pad start observation capacity exhausted"
            if start.diagnostic_slot is not None:
                self.report_launch_error(start.diagnostic_slot, message)
            raise RuntimeError(message)
        identity = id(start)
        pending = _CapturedStart(
            start,
            cache_backed=False,
            requested=None,
            deadline=time.monotonic() + 30.0,
            published=True,
        )
        self._captured_starts[identity] = pending
        if self._poll_captured_start(pending):
            self._captured_starts.pop(identity, None)

    def _poll_captured_starts(self) -> None:
        for identity, pending in tuple(self._captured_starts.items()):
            if self._poll_captured_start(pending):
                self._captured_starts.pop(identity, None)

    def _poll_captured_start(self, pending: _CapturedStart) -> bool:
        start = pending.start
        state = start.state()
        sample_id = start.diagnostic_slot
        owner_status = start.original_owner_status()
        if self._captured_start_terminal(pending, sample_id, state, owner_status):
            return True
        if pending.published or owner_status in {"pending", "admitted"} or sample_id is None:
            return False
        try:
            self._ensure_start_assignment_ready(sample_id)
            return self._advance_captured_start(pending, sample_id)
        except (RuntimeError, ValueError) as error:
            return self._captured_start_error(pending, sample_id, error)

    def _captured_start_terminal(
        self, pending: _CapturedStart, sample_id: int | None, state: str, owner_status: str
    ) -> bool:
        if state == "applied":
            if sample_id is not None:
                self._remember_captured_geometry(pending, sample_id)
                self.report_launch_error(sample_id, None)
            return True
        if state == "rejected":
            if sample_id is not None:
                self.report_launch_error(
                    sample_id, pending.start.reason() or "Captured pad start rejected"
                )
            return True
        native_join_pending = (
            pending.cache_backed and pending.ticket is None and not pending.published
        )
        if owner_status == "retired" or (owner_status == "superseded" and not native_join_pending):
            self._cancel_captured_start(pending)
            return True
        if self._closed or time.monotonic() >= pending.deadline:
            self._cancel_captured_start(pending)
            if sample_id is not None and not self._closed:
                self.report_launch_error(sample_id, "Captured pad start readiness deadline expired")
            return True
        return False

    def _captured_start_error(
        self, pending: _CapturedStart, sample_id: int, error: RuntimeError | ValueError
    ) -> bool:
        self.report_launch_error(sample_id, str(error))
        retryable = isinstance(error, RuntimeError) and any(
            marker in str(error)
            for marker in ("queue is full", "queue full", "buffer may be full", "lock")
        )
        if retryable and pending.attempts < 8:
            return False
        self._cancel_captured_start(pending)
        return True

    @staticmethod
    def _with_geometry(intent: _Intent, geometry: _Intent) -> _Intent:
        return replace(
            intent, start=geometry.start, end=geometry.end, auto=geometry.auto, bars=geometry.bars
        )

    def _remember_captured_geometry(self, pending: _CapturedStart, sample_id: int) -> None:
        if pending.geometry_acknowledged or pending.requested is None:
            return
        ticket = pending.ticket
        if ticket is not None:
            try:
                current_ack = ticket.publication_status() == "accepted" and ticket.is_current()
            except RuntimeError, ValueError:
                return
            if not current_ack:
                return
        pending.geometry_acknowledged = True
        self._acknowledged_geometry[sample_id] = pending.requested
        mode = self._observe_mode(sample_id)
        previous = self._effective.get(sample_id)
        if mode is not None:
            self._effective[sample_id] = replace(pending.requested, key_lock=mode)
        elif previous is not None:
            self._effective[sample_id] = self._with_geometry(previous, pending.requested)
        edit = self._pending.get(sample_id)
        if edit is not None:
            edit.previous = self._with_geometry(edit.previous, pending.requested)

    def _advance_captured_start(self, pending: _CapturedStart, sample_id: int) -> bool:
        if pending.attempts >= 8:
            self.report_launch_error(sample_id, "Captured pad start admission retry limit reached")
            self._cancel_captured_start(pending)
            return True
        if pending.cache_backed and pending.ticket is None:
            pending.attempts += 1
            pending.ticket = (
                self._audio.prepare_captured_pad_start(pending.start)
                if pending.dependency is None
                else self._audio.prepare_captured_pad_start(pending.start, pending.dependency)
            )
            # The exact ticket may already be accepted. Re-enter once with that
            # ticket installed, preserving terminal, deadline and attempt guards.
            return self._poll_captured_start(pending)
        ticket = pending.ticket
        if ticket is not None:
            status = ticket.publication_status()
            if status in {"failed", "cancelled", "rejected"}:
                self.report_launch_error(
                    sample_id, ticket.error() or "Captured pad start window rejected"
                )
                self._cancel_captured_start(pending)
                return True
            if status != "accepted":
                return False
            self._remember_captured_geometry(pending, sample_id)
        pending.attempts += 1
        published = (
            self._audio.publish_pad_start(pending.start)
            if ticket is None
            else self._audio.publish_pad_start(pending.start, ticket)
        )
        if published:
            pending.published = True
            self.report_launch_error(sample_id, None)
        else:
            self.report_launch_error(sample_id, "Captured pad start launch remains unavailable")
        return False

    @staticmethod
    def _cancel_captured_start(pending: _CapturedStart) -> None:
        pending.start.cancel()
        if pending.ticket is not None:
            pending.ticket.cancel_launch()
            pending.ticket.cancel()

    def _continue_start(
        self,
        sample_id: int,
        pending: _Pending | None,
        start: float,
        end: float | None,
        *,
        expected_source_generation: int | None = None,
    ) -> bool:
        if (
            pending is None
            or pending.unconfirmed
            or pending.position is not None
            or pending.key_lock_only
            or pending.expected_source_generation != expected_source_generation
        ):
            return False
        if (
            pending.start != start
            or pending.end != end
            or pending.requested != self._intent(sample_id)
            or not self._same_owner(sample_id, pending)
        ):
            return False
        ticket = pending.ticket
        return ticket is None or (
            ticket.is_current()
            and ticket.publication_status() in {"preparing", "pending", "adopting", "accepted"}
        )

    def _request(
        self,
        sample_id: int,
        start: float,
        end: float | None,
        *,
        position: float | None = None,
        after_ack: Callable[[ResidentWindowTicket], bool | None] | None = None,
        key_lock_only: bool = False,
        scalar: bool = False,
        expected_source_generation: int | None = None,
    ) -> None:
        if self._closed:
            return
        if self._diagnostics is not None:
            self._diagnostics.begin_attempt("resident_preparation", sample_id)
            self._diagnostics.begin_attempt("key_lock", sample_id)
        previous = self._pending.get(sample_id)
        requested = self._intent(sample_id)
        baseline = previous.previous if previous else self._effective.get(sample_id, requested)
        geometry = self._acknowledged_geometry.get(sample_id)
        if previous is None and geometry is not None and geometry.path == requested.path:
            baseline = self._with_geometry(baseline, geometry)
        waiting = (previous.ticket or previous.waiting_ticket) if previous else None
        waiting_intent = (
            previous.requested
            if previous and previous.ticket
            else previous.waiting_intent
            if previous
            else None
        )
        waiting_owner = (
            previous.owner
            if previous and previous.ticket
            else previous.waiting_owner
            if previous
            else None
        )
        if (
            waiting is not None
            and waiting_intent is not None
            and self._acknowledged_window(sample_id, waiting, waiting_owner)
        ):
            baseline = self._confirmed_baseline(sample_id, waiting_intent, baseline)
        pending = _Pending(
            requested,
            baseline,
            start,
            end,
            position,
            after_ack,
            self._owner(sample_id),
            expected_source_generation=expected_source_generation,
            seek_only=position is not None and previous is None,
            key_lock_only=key_lock_only,
            scalar=scalar,
            waiting_ticket=waiting,
            waiting_intent=waiting_intent,
            waiting_owner=waiting_owner,
            deadline=time.monotonic() + 30.0,
        )
        self._pending[sample_id] = pending
        self._errors.pop(sample_id, None)
        if after_ack is not None:
            self._launch_errors.pop(sample_id, None)
        self._submit(sample_id, pending)

    def _confirmed_baseline(self, sample_id: int, intent: _Intent, previous: _Intent) -> _Intent:
        # Window acknowledgement establishes geometry. Only ready mode feedback
        # establishes its processing mode; preserve the last actual mode otherwise.
        mode = self._observe_mode(sample_id)
        return replace(intent, key_lock=previous.key_lock if mode is None else mode)

    def _acknowledged_window(
        self, sample_id: int, ticket: ResidentWindowTicket, owner: tuple[object, ...] | None
    ) -> bool:
        """Keep actual ACKed geometry after a stem replacement retires launch authority."""
        if ticket.publication_status() != "accepted":
            return False
        if ticket.is_current():
            return True
        if owner is None:
            return False
        try:
            return (
                self._owner(sample_id) == owner
                and self._audio.loaded_residency(sample_id).get("window_revision")
                == ticket.window_revision
            )
        except RuntimeError, ValueError:
            return False

    def prepare_captured_trigger(self, trigger: CapturedInputPadTrigger) -> None:
        """Retain the exact original MIDI gesture through its own resident window ACK."""
        identity = id(trigger)
        if identity in self._captured_triggers:
            return
        if self._diagnostics is not None:
            self._diagnostics.begin_attempt("pad_control", trigger.diagnostic_slot)
        if len(self._captured_triggers) >= 256:
            message = "Captured MIDI trigger capacity exhausted"
            raise RuntimeError(message)
        self._captured_triggers[identity] = _CapturedTrigger(
            trigger, deadline=time.monotonic() + 30.0
        )

    def owns_captured_trigger(self, trigger: CapturedInputPadTrigger) -> bool:
        """Keep duplicate delivery under its original bounded readiness owner."""
        pending = self._captured_triggers.get(id(trigger))
        return pending is not None and pending.trigger is trigger

    def _poll_captured_triggers(self) -> None:
        for identity, pending in tuple(self._captured_triggers.items()):
            try:
                completed = self._poll_captured_trigger(pending)
            except (RuntimeError, ValueError) as error:
                self._report_captured_trigger_error(pending, str(error))
                continue
            if completed:
                self._captured_triggers.pop(identity, None)

    def _poll_captured_trigger(self, pending: _CapturedTrigger) -> bool:
        """Advance one exact gesture owner; false observations retain its original ticket."""
        owner_status = pending.trigger.original_owner_status()
        if owner_status in {"retired", "superseded", "admitted"}:
            return True
        if owner_status == "pending":
            return False
        if self._closed or pending.cancel_requested:
            return self._cancel_captured_trigger_window(pending)
        return self._advance_captured_trigger(pending)

    def _advance_captured_trigger(self, pending: _CapturedTrigger) -> bool:
        sample_id = pending.trigger.diagnostic_slot
        if sample_id is None:
            return False
        if time.monotonic() >= pending.deadline:
            self._report_captured_trigger_error(pending, "Captured MIDI readiness deadline expired")
            pending.cancel_requested = True
            return self._cancel_captured_trigger_window(pending)
        self._ensure_start_assignment_ready(sample_id)
        if pending.ticket is None:
            return self._prepare_captured_trigger_window(pending)
        return self._launch_captured_trigger_window(pending, sample_id)

    def _prepare_captured_trigger_window(self, pending: _CapturedTrigger) -> bool:
        if pending.attempts >= 8:
            message = "Captured MIDI admission retry limit reached"
            self._report_captured_trigger_error(pending, message)
            pending.cancel_requested = True
            return self._cancel_captured_trigger_window(pending)
        pending.attempts += 1
        pending.ticket = self._audio.prepare_captured_input_trigger(pending.trigger)
        return False

    def _launch_captured_trigger_window(self, pending: _CapturedTrigger, sample_id: int) -> bool:
        ticket = pending.ticket
        if ticket is None:
            return False
        status = ticket.publication_status()
        if status != "accepted":
            if status in {"failed", "cancelled", "rejected"}:
                self._report_captured_trigger_error(
                    pending, ticket.error() or "Captured MIDI window rejected"
                )
                return True
            return False
        try:
            launched = self._audio.trigger_prepared_input_pad(pending.trigger, ticket)
        except (RuntimeError, ValueError) as error:
            self._report_captured_trigger_error(pending, str(error))
            retryable = isinstance(error, RuntimeError) and any(
                marker in str(error)
                for marker in ("queue is full", "buffer may be full", "launch remains unavailable")
            )
            return self._retry_captured_trigger_launch(pending, retryable=retryable)
        if launched:
            self.report_launch_error(sample_id, None)
            return True
        self._report_captured_trigger_error(pending, "Captured MIDI launch remains unavailable")
        return self._retry_captured_trigger_launch(pending, retryable=True)

    def _retry_captured_trigger_launch(self, pending: _CapturedTrigger, *, retryable: bool) -> bool:
        if pending.trigger.original_owner_status() == "pending":
            return False
        if retryable and pending.attempts < 8 and time.monotonic() < pending.deadline:
            pending.attempts += 1
            return False
        pending.cancel_requested = True
        return self._cancel_captured_trigger_window(pending)

    def _report_captured_trigger_error(self, pending: _CapturedTrigger, message: str) -> None:
        sample_id = pending.trigger.diagnostic_slot
        if sample_id is not None:
            self.report_launch_error(sample_id, message)

    @staticmethod
    def _cancel_captured_trigger_window(pending: _CapturedTrigger) -> bool:
        if pending.ticket is None:
            # There is no window whose Native launch cancellation can supersede
            # this token. Retain this bounded exhausted owner until its original
            # Native source/mapping/gesture itself becomes terminal.
            return False
        pending.ticket.cancel_launch()
        pending.ticket.cancel()
        return pending.ticket.publication_status() in {"failed", "cancelled", "rejected"}

    def _submit(self, sample_id: int, pending: _Pending) -> None:
        pending.attempts += 1
        try:
            if pending.scalar:
                request = self._audio.set_pad_key_lock(
                    sample_id,
                    pending.requested.key_lock,
                    **self._transport.source_generation_options(pending.expected_source_generation),
                )
                pending.scalar_request = request if type(request) is int and request > 0 else None
                pending.scalar_submitted = True
                self._errors.pop(sample_id, None)
                return
            pending.ticket = self._audio.prepare_resident_control(
                sample_id,
                start_s=None if pending.seek_only or pending.key_lock_only else pending.start,
                end_s=None if pending.seek_only or pending.key_lock_only else pending.end,
                position_s=pending.position,
                key_lock=None if pending.seek_only else pending.requested.key_lock,
                **self._transport.source_generation_options(pending.expected_source_generation),
            )
        except (RuntimeError, ValueError) as error:
            message = str(error)
            if isinstance(error, RuntimeError) and any(
                marker in message
                for marker in (
                    "adoption is in progress",
                    "queue is full",
                    "cold source queue full",
                    "resident stem publication is pending",
                    "buffer may be full",
                )
            ):
                self._record_error(sample_id, message, pending)
                return
            self._fail(sample_id, pending, message)
        else:
            if (
                pending.waiting_ticket is not None
                and pending.waiting_ticket.publication_status() == "accepted"
                and pending.ticket.previous_window_revision
                == pending.waiting_ticket.window_revision
                and pending.waiting_intent is not None
                and pending.waiting_owner == pending.owner
            ):
                pending.previous = self._confirmed_baseline(
                    sample_id, pending.waiting_intent, pending.previous
                )
            pending.waiting_ticket = None
            pending.waiting_intent = None
            pending.waiting_owner = None
            self._errors.pop(sample_id, None)

    def _restore(self, sample_id: int, intent: _Intent) -> None:
        self._project.pad_loop_start_s[sample_id] = intent.start
        self._project.pad_loop_end_s[sample_id] = intent.end
        self._project.pad_loop_auto[sample_id] = intent.auto
        self._project.pad_loop_bars[sample_id] = intent.bars
        # Native rollback describes audio, never the performer's override origin.
        self._transport._mark_project_changed()

    def _fail(self, sample_id: int, pending: _Pending, error: str) -> None:
        if self._pending.get(sample_id) is not pending:
            return
        if self._intent(sample_id) == pending.requested and self._same_owner(sample_id, pending):
            pending.previous = self._confirmed_baseline(
                sample_id, pending.previous, pending.previous
            )
            self._restore(sample_id, pending.previous)
        self._pending.pop(sample_id, None)
        self._record_error(sample_id, error, pending)

    def _same_owner(self, sample_id: int, pending: _Pending) -> bool:
        try:
            return self._owner(sample_id) == pending.owner
        except RuntimeError, ValueError:
            return False

    def poll(self) -> None:
        """Observe bounded native completion, stale ownership and admission retries."""
        self._poll_captured_starts()
        self._poll_captured_triggers()
        for sample_id, pending in tuple(self._pending.items()):
            if sample_id in getattr(self._transport, "_pair_deferred_sample_ids", ()):
                continue
            if self._intent(sample_id) != pending.requested:
                self.cancel(sample_id)
                continue
            try:
                if self._owner(sample_id) != pending.owner:
                    self.cancel(sample_id)
                    continue
                self._poll_one(sample_id, pending)
            except (RuntimeError, ValueError) as error:
                self._fail(sample_id, pending, str(error))

    def _poll_one(self, sample_id: int, pending: _Pending) -> None:
        ticket = pending.ticket
        if ticket is None:
            if pending.scalar_submitted:
                self._poll_mode(sample_id, pending)
                return
            self._retry(sample_id, pending)
            return
        status = ticket.publication_status()
        if status in {"preparing", "pending", "adopting"}:
            self._check_deadline(sample_id, pending, ticket, status)
            return
        if status == "accepted" and not ticket.is_current():
            self.cancel(sample_id)
            return
        if status != "accepted":
            self._fail(sample_id, pending, ticket.error() or "resident control was rejected")
            return
        if (
            pending.key_lock_only
            or ticket.key_lock_request_id is not None
            or pending.requested.key_lock != pending.previous.key_lock
        ) and not self._poll_mode(sample_id, pending, ticket=ticket):
            return
        self._finish_ack(sample_id, pending, ticket)

    def _finish_ack(self, sample_id: int, pending: _Pending, ticket: ResidentWindowTicket) -> None:
        self._effective[sample_id] = pending.requested
        self._acknowledged_geometry.pop(sample_id, None)
        pending.previous = pending.requested
        self._errors.pop(sample_id, None)
        if pending.position is not None and ticket.effective_seek_seconds is not None:
            self._transport._session.pad_playhead_s[sample_id] = ticket.effective_seek_seconds
        if pending.after_ack is not None:
            try:
                self._invoke_launch(pending.after_ack, ticket)
            except (RuntimeError, ValueError) as error:
                self.report_launch_error(sample_id, str(error))
                retryable = isinstance(error, RuntimeError) and any(
                    marker in str(error)
                    for marker in (
                        "queue is full",
                        "buffer may be full",
                        "prepared MIDI launch remains unavailable",
                    )
                )
                if retryable and pending.attempts < 8 and time.monotonic() < pending.deadline:
                    pending.attempts += 1
                    return
            else:
                self._launched[sample_id] = ticket
                self.report_launch_error(sample_id, None)
        self._pending.pop(sample_id, None)

    def _poll_mode(
        self, sample_id: int, pending: _Pending, *, ticket: ResidentWindowTicket | None = None
    ) -> bool:
        feedback = self._mode_feedback(sample_id)
        own_feedback = feedback is not None and (
            feedback.get("request_id") == pending.scalar_request
            if pending.scalar and pending.scalar_request is not None
            else ticket is not None
            and feedback.get("window_revision") == ticket.window_revision
            and ticket.key_lock_request_id is not None
            and feedback.get("request_id") == ticket.key_lock_request_id
        )
        if own_feedback and feedback is not None:
            native_error = feedback.get("error")
            if isinstance(native_error, str) and native_error:
                effective = feedback["effective"]
                if isinstance(effective, bool):
                    self._confirmed_modes[sample_id] = (self._mode_identity(feedback), effective)
                    # This Window already adopted its geometry. A processing
                    # error cannot undo it by restoring project fields alone.
                    baseline = pending.requested if ticket is not None else pending.previous
                    pending.previous = replace(baseline, key_lock=effective)
                self._fail(sample_id, pending, native_error)
                return False
            if (
                feedback.get("effective") is pending.requested.key_lock
                and feedback["ready"] is True
            ):
                self._observe_mode(sample_id)
                if pending.scalar:
                    self._effective[sample_id] = pending.requested
                    self._pending.pop(sample_id, None)
                    self._errors.pop(sample_id, None)
                return True
        if time.monotonic() >= pending.deadline:
            pending.unconfirmed = True
            pending.after_ack = None
            self._record_error(sample_id, "Key Lock change is unconfirmed", pending)
        return False

    @staticmethod
    def _invoke_launch(
        launch: Callable[[ResidentWindowTicket], bool | None], ticket: ResidentWindowTicket
    ) -> None:
        if launch(ticket) is False:
            message = "resident launch ownership retired"
            raise ValueError(message)

    def _check_deadline(
        self, sample_id: int, pending: _Pending, ticket: ResidentWindowTicket, status: str
    ) -> None:
        if time.monotonic() < pending.deadline:
            return
        if status != "adopting" and ticket.cancel():
            self._fail(sample_id, pending, "resident preparation deadline expired")
        else:
            pending.unconfirmed = True
            pending.after_ack = None
            self._record_error(sample_id, "resident native adoption is unconfirmed", pending)

    def _retry(self, sample_id: int, pending: _Pending) -> None:
        waiting = pending.waiting_ticket
        if waiting is not None:
            status = waiting.publication_status()
            if status in {"preparing", "pending", "adopting"}:
                self._check_deadline(sample_id, pending, waiting, status)
                return
            if pending.waiting_intent is not None and self._acknowledged_window(
                sample_id, waiting, pending.waiting_owner
            ):
                pending.previous = self._confirmed_baseline(
                    sample_id, pending.waiting_intent, pending.previous
                )
            pending.waiting_ticket = None
            pending.waiting_intent = None
            pending.waiting_owner = None
            if pending.unconfirmed:
                self._fail(sample_id, pending, "latest intent retired after unconfirmed adoption")
                return
        if pending.attempts >= 8 or time.monotonic() >= pending.deadline:
            self._fail(sample_id, pending, "resident admission retry limit reached")
            return
        self._submit(sample_id, pending)

    def cancel(self, sample_id: int) -> None:
        """Retire exactly this pending intent without restoring an unloaded/new source."""
        launched = self._launched.pop(sample_id, None)
        if launched is not None:
            launched.cancel_launch()
        pending = self._pending.pop(sample_id, None)
        if pending is not None:
            for ticket in (pending.ticket, pending.waiting_ticket):
                if ticket is not None:
                    ticket.cancel()
        self._effective.pop(sample_id, None)
        self._acknowledged_geometry.pop(sample_id, None)
        self._confirmed_modes.pop(sample_id, None)
        self._errors.pop(sample_id, None)
        self._error_sources.pop(sample_id, None)
        self._launch_errors.pop(sample_id, None)

    def cancel_requested(self, sample_id: int) -> bool:
        """Cancel unclaimed preparation and restore only its still-current intent."""
        pending = self._pending.get(sample_id)
        if pending is None:
            return False
        if pending.scalar_submitted:
            # An enqueued scalar has no cancellable Window ticket. Retain its
            # exact source/request until callback feedback establishes the result.
            return False
        for ticket in (pending.ticket, pending.waiting_ticket):
            if ticket is not None and not ticket.cancel():
                return False
        if self._intent(sample_id) == pending.requested and self._same_owner(sample_id, pending):
            self._restore(sample_id, pending.previous)
        self.cancel(sample_id)
        return True

    def cancel_launch(self, sample_id: int) -> bool:
        """Capture and revoke admitted starts under the native admission fence."""
        retained = sample_id in self._launched or any(
            pending.start.diagnostic_slot == sample_id for pending in self._captured_starts.values()
        )
        admitted = self._audio.cancel_pad_launches(sample_id)
        self._cancel_launch_ownership(sample_id)
        return retained or admitted

    def _cancel_launch_ownership(self, sample_id: int) -> None:
        for identity, captured in tuple(self._captured_starts.items()):
            if captured.start.diagnostic_slot == sample_id:
                self._cancel_captured_start(captured)
                self._captured_starts.pop(identity, None)
        launched = self._launched.pop(sample_id, None)
        if launched is not None:
            launched.cancel_launch()
        pending = self._pending.get(sample_id)
        if pending is not None and pending.after_ack is not None:
            pending.after_ack = None
            self.cancel_requested(sample_id)

    def cancel_all_launches(self) -> frozenset[int]:
        """Return admitted STOP targets captured with native launch revocation."""
        admitted = frozenset(self._audio.cancel_all_launches()) | frozenset(self._launched)
        for sample_id in self._pending.keys() | self._launched.keys():
            self._cancel_launch_ownership(sample_id)
        for identity, captured in tuple(self._captured_starts.items()):
            self._cancel_captured_start(captured)
            self._captured_starts.pop(identity, None)
        return admitted

    def shut_down(self) -> None:
        """Stop admission before native ownership and source readers are drained."""
        self._closed = True
        self._audio.cancel_all_launches()
        for captured in tuple(self._captured_starts.values()):
            self._cancel_captured_start(captured)
        self._captured_starts.clear()
        for pending in tuple(self._captured_triggers.values()):
            try:
                self._cancel_captured_trigger_window(pending)
            except (RuntimeError, ValueError) as error:
                self._report_captured_trigger_error(pending, str(error))
        for sample_id in self._pending.keys() | self._launched.keys():
            self.cancel(sample_id)
