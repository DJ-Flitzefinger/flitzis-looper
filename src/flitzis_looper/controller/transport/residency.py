"""One controller path for source-bound loop, seek and processing readiness."""

import math
import time
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING

from flitzis_looper.controller.transport.key_lock_status import KeyLockStatus

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper_audio import ResidentWindowTicket


@dataclass(frozen=True, slots=True)
class _Intent:
    path: str | None
    start: float
    end: float | None
    auto: bool
    bars: float
    key_lock: bool


@dataclass(slots=True)
class _Pending:
    requested: _Intent
    previous: _Intent
    start: float
    end: float | None
    position: float | None
    after_ack: Callable[[ResidentWindowTicket], bool | None] | None
    owner: tuple[object, ...]
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


class ResidencyController:
    """Keep requested project intent separate from native acknowledged playback.

    At most one latest request per pad is retained. Native preparation owns PCM,
    the fixed worker queue, cancellation and adoption. Polling observes ACK; it
    never moves a running playhead to compensate for timing.
    """

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._audio = transport._audio
        self._pending: dict[int, _Pending] = {}
        self._launched: dict[int, ResidentWindowTicket] = {}
        self._effective: dict[int, _Intent] = {}
        self._errors: dict[int, str] = {}
        self._error_sources: dict[int, tuple[object, ...]] = {}
        self._launch_errors: dict[int, str] = {}
        self._confirmed_modes: dict[int, tuple[tuple[object, ...], bool]] = {}
        self._closed = False
        transport._on_frame_render_callbacks.append(self.poll)

    def _intent(self, sample_id: int) -> _Intent:
        return _Intent(
            self._project.sample_paths[sample_id],
            self._project.pad_loop_start_s[sample_id],
            self._project.pad_loop_end_s[sample_id],
            self._project.pad_loop_auto[sample_id],
            self._project.pad_loop_bars[sample_id],
            self._project.pad_key_lock[sample_id],
        )

    def remember(self, sample_id: int) -> None:
        """Capture the effective intent before a performer changes requested fields."""
        if sample_id not in self._pending:
            intent = self._intent(sample_id)
            mode = self._observe_mode(sample_id)
            self._effective[sample_id] = intent if mode is None else replace(intent, key_lock=mode)

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
        return KeyLockStatus(
            self._project.pad_key_lock[sample_id],
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
        return None, error

    def report_launch_error(self, sample_id: int, error: str | None) -> None:
        """Retain a terminal shared launch error for the existing pad readiness UI."""
        if error is None:
            self._launch_errors.pop(sample_id, None)
        else:
            self._launch_errors[sample_id] = error

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

    def publish_loop(self, sample_id: int, start: float, end: float | None) -> None:
        """Publish loop and requested Key Lock together through one native transaction."""
        if not self._cache_backed(sample_id):
            self._audio.set_pad_loop_region(sample_id, start, end)
            self._effective[sample_id] = self._intent(sample_id)
            return
        self._request(sample_id, start, end)

    def set_key_lock(self, sample_id: int, *, enabled: bool) -> None:
        """Admit complete processing context before Key Lock becomes effective."""
        self.remember(sample_id)
        previous = self._pending.get(sample_id)
        if (
            self._reusable_mode_request(sample_id, previous)
            and previous is not None
            and previous.requested.key_lock is enabled
        ):
            return
        if (
            previous is None
            and self._project.pad_key_lock[sample_id] is enabled
            and self._observe_mode(sample_id) is enabled
            and self._current_error(sample_id) is None
        ):
            return
        self._project.pad_key_lock[sample_id] = enabled
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
            )
        else:
            start, end = self._transport.loop.requested_region(sample_id)
            self._request(
                sample_id,
                start,
                end,
                key_lock_only=True,
                scalar=not self._cache_backed(sample_id),
            )

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

    def seek(self, sample_id: int, position: float) -> None:
        """Update a seek projection only after matching adoption acknowledgement."""
        if not self._cache_backed(sample_id):
            duration = self._project.sample_durations[sample_id]
            if duration is not None and math.isfinite(duration) and duration >= 0:
                position = min(position, duration)
            self._audio.seek_sample(sample_id, position)
            self._transport._session.pad_playhead_s[sample_id] = position
            return
        self.remember(sample_id)
        start, end = self._transport.loop.requested_region(sample_id)
        self._request(sample_id, start, end, position=position)

    def start(
        self,
        sample_id: int,
        start: float,
        end: float | None,
        play: Callable[[], None],
        *,
        prepared_play: Callable[[ResidentWindowTicket], bool] | None = None,
    ) -> None:
        """Retain the input's launch intent until the required source context is ready."""
        if not self._cache_backed(sample_id):
            self._audio.set_pad_loop_region(sample_id, start, end)
            play()
            return
        self.remember(sample_id)
        launch: Callable[[ResidentWindowTicket], bool | None] = prepared_play or (
            lambda _ticket: play()
        )
        pending = self._pending.get(sample_id)
        if self._continue_start(sample_id, pending, start, end):
            assert pending is not None
            # An identical gesture changes launch time, not preparation ownership.
            # Keep the existing retry budget/deadline and actual native ACK gate.
            pending.after_ack = launch
            self._launch_errors.pop(sample_id, None)
            if pending.ticket is not None and pending.ticket.publication_status() == "accepted":
                self._poll_one(sample_id, pending)
            return
        self._request(sample_id, start, end, after_ack=launch)

    def _continue_start(
        self, sample_id: int, pending: _Pending | None, start: float, end: float | None
    ) -> bool:
        if (
            pending is None
            or pending.unconfirmed
            or pending.position is not None
            or pending.key_lock_only
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
    ) -> None:
        if self._closed:
            return
        previous = self._pending.get(sample_id)
        requested = self._intent(sample_id)
        baseline = previous.previous if previous else self._effective.get(sample_id, requested)
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

    def prepare_trigger(
        self, sample_id: int, start: float, end: float | None, play: Callable[[], None]
    ) -> bool:
        """Let guarded MIDI fallback use the same prepared launch transaction."""
        if not self._cache_backed(sample_id):
            return False
        self.start(sample_id, start, end, play)
        return True

    def _submit(self, sample_id: int, pending: _Pending) -> None:
        pending.attempts += 1
        try:
            if pending.scalar:
                request = self._audio.set_pad_key_lock(sample_id, pending.requested.key_lock)
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
        self._project.pad_key_lock[sample_id] = intent.key_lock
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
        for sample_id, pending in tuple(self._pending.items()):
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
        retained = sample_id in self._launched
        admitted = self._audio.cancel_pad_launches(sample_id)
        self._cancel_launch_ownership(sample_id)
        return retained or admitted

    def _cancel_launch_ownership(self, sample_id: int) -> None:
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
        return admitted

    def shut_down(self) -> None:
        """Stop admission before native ownership and source readers are drained."""
        self._closed = True
        self._audio.cancel_all_launches()
        for sample_id in self._pending.keys() | self._launched.keys():
            self.cancel(sample_id)
