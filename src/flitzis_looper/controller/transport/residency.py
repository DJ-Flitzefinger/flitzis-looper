"""One controller path for source-bound loop, seek and processing readiness."""

import math
import time
from dataclasses import dataclass
from typing import TYPE_CHECKING

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
    ticket: ResidentWindowTicket | None = None
    waiting_ticket: ResidentWindowTicket | None = None
    waiting_intent: _Intent | None = None
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
        self._launch_errors: dict[int, str] = {}
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
            self._effective[sample_id] = self._intent(sample_id)

    def status(self, sample_id: int) -> tuple[str | None, str | None]:
        """Expose pending/error without presenting requested fields as effective."""
        pending = self._pending.get(sample_id)
        error = self._errors.get(sample_id) or self._launch_errors.get(sample_id)
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
        return isinstance(descriptor, dict) and isinstance(descriptor.get("source_identity"), int)

    def _owner(self, sample_id: int) -> tuple[object, ...]:
        descriptor = self._audio.loaded_residency(sample_id)
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
        if not self._cache_backed(sample_id):
            self._audio.set_pad_key_lock(sample_id, enabled)
            self._project.pad_key_lock[sample_id] = enabled
            self._effective[sample_id] = self._intent(sample_id)
            return
        self._project.pad_key_lock[sample_id] = enabled
        start, end = self._transport.loop.requested_region(sample_id)
        self._request(sample_id, start, end)

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
        self._request(sample_id, start, end, after_ack=prepared_play or (lambda _ticket: play()))

    def _request(
        self,
        sample_id: int,
        start: float,
        end: float | None,
        *,
        position: float | None = None,
        after_ack: Callable[[ResidentWindowTicket], bool | None] | None = None,
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
        if (
            waiting is not None
            and waiting.publication_status() == "accepted"
            and waiting.is_current()
            and waiting_intent is not None
        ):
            baseline = waiting_intent
        pending = _Pending(
            requested,
            baseline,
            start,
            end,
            position,
            after_ack,
            self._owner(sample_id),
            seek_only=position is not None and previous is None,
            waiting_ticket=waiting,
            waiting_intent=waiting_intent,
            deadline=time.monotonic() + 30.0,
        )
        self._pending[sample_id] = pending
        self._errors.pop(sample_id, None)
        if after_ack is not None:
            self._launch_errors.pop(sample_id, None)
        self._submit(sample_id, pending)

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
            pending.ticket = self._audio.prepare_resident_control(
                sample_id,
                start_s=None if pending.seek_only else pending.start,
                end_s=None if pending.seek_only else pending.end,
                position_s=pending.position,
                key_lock=None if pending.seek_only else pending.requested.key_lock,
            )
        except (RuntimeError, ValueError) as error:
            message = str(error)
            if isinstance(error, RuntimeError) and (
                "adoption is in progress" in message
                or "queue is full" in message
                or "cold source queue full" in message
                or "resident stem publication is pending" in message
            ):
                self._errors[sample_id] = message
                return
            self._fail(sample_id, pending, message)
        else:
            if (
                pending.waiting_ticket is not None
                and pending.waiting_ticket.publication_status() == "accepted"
                and pending.ticket.previous_window_revision
                == pending.waiting_ticket.window_revision
                and pending.waiting_intent is not None
            ):
                pending.previous = pending.waiting_intent
            pending.waiting_ticket = None
            pending.waiting_intent = None
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
            self._restore(sample_id, pending.previous)
        self._pending.pop(sample_id, None)
        self._errors[sample_id] = error

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
                self._audio.loaded_residency(sample_id)
                if self._owner(sample_id) != pending.owner:
                    self.cancel(sample_id)
                    continue
                self._poll_one(sample_id, pending)
            except (RuntimeError, ValueError) as error:
                self._fail(sample_id, pending, str(error))

    def _poll_one(self, sample_id: int, pending: _Pending) -> None:
        ticket = pending.ticket
        if ticket is None:
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
            self._errors[sample_id] = "resident native adoption is unconfirmed"

    def _retry(self, sample_id: int, pending: _Pending) -> None:
        waiting = pending.waiting_ticket
        if waiting is not None:
            status = waiting.publication_status()
            if status in {"preparing", "pending", "adopting"}:
                self._check_deadline(sample_id, pending, waiting, status)
                return
            if status == "accepted" and waiting.is_current() and pending.waiting_intent is not None:
                pending.previous = pending.waiting_intent
            pending.waiting_ticket = None
            pending.waiting_intent = None
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
        self._errors.pop(sample_id, None)
        self._launch_errors.pop(sample_id, None)

    def cancel_requested(self, sample_id: int) -> bool:
        """Cancel unclaimed preparation and restore only its still-current intent."""
        pending = self._pending.get(sample_id)
        if pending is None:
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
