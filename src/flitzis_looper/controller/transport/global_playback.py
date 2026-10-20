"""Controller intent for transactional native GLOBAL START/STOP batches."""

import time
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING

from flitzis_looper.controller.current_binding import capture_current_pad_binding
from flitzis_looper.input_timing import validate_input_timestamp_ns

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.recording import RecordingGroupGate
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper_audio import (
        CapturedGlobalStart,
        GlobalPlaybackBatchTicket,
        InputRuntimePadBinding,
        MixCaptureTicket,
        ResidentWindowTicket,
    )


@dataclass(frozen=True)
class _PendingGlobalBatch:
    ticket: GlobalPlaybackBatchTicket
    restore_ids: frozenset[int]
    restore_revision: int
    start: bool = False
    start_ids: frozenset[int] = frozenset()
    capture: CapturedGlobalStart | None = None
    recording_gate: _RecordingStartGate | None = None


@dataclass(frozen=True)
class _DeferredStart:
    capture: CapturedGlobalStart
    sample_ids: frozenset[int]
    restore_revision: int
    deadline: float
    attempts: int = 0
    recording_gate: _RecordingStartGate | None = None


class _RecordingStartGate:
    """Control-only access to one retained original; Native keeps effect authority."""

    def __init__(self, owner: GlobalPlaybackController, capture: CapturedGlobalStart) -> None:
        self._owner = owner
        self._capture = capture
        self._capture_ticket: MixCaptureTicket | None = None
        self._cancelled = False

    def _deferred(self) -> _DeferredStart | None:
        deferred = self._owner._deferred
        if (
            deferred is not None
            and deferred.recording_gate is self
            and deferred.capture is self._capture
        ):
            return deferred
        return None

    def _pending(self) -> _PendingGlobalBatch | None:
        pending = self._owner._pending
        if (
            pending is not None
            and pending.recording_gate is self
            and pending.capture is self._capture
        ):
            return pending
        return None

    def is_current(self) -> bool:
        """Check this control owner; the original Native proof still guards publication."""
        if self._cancelled:
            return False
        if deferred := self._deferred():
            return self._owner._deferred_current(deferred)
        pending = self._pending()
        return (
            pending is not None
            and pending.restore_revision == self._owner._restore_revision
            and pending.ticket.publication_status() != "rejected"
            and self._capture.state() != "rejected"
        )

    def publish(self, ticket: MixCaptureTicket) -> bool:
        """Attach the same ready ticket; only a genuine FIFO receipt returns True."""
        if self._capture_ticket is not None and self._capture_ticket is not ticket:
            message = "Cannot replace the original recording capture ticket"
            raise ValueError(message)
        if not self.is_current():
            message = self._capture.reason() or "Original recording group is no longer current"
            raise ValueError(message)
        self._capture_ticket = ticket
        if self._pending() is not None:
            return True
        deferred = self._deferred()
        if deferred is None:
            return False
        if self._owner._pair_publication_pending():
            return False
        admitted = self._owner._admit_start(deferred, capture_ticket=ticket)
        if not admitted and not self.is_current():
            message = self._capture.reason() or "Original recording group is no longer current"
            raise ValueError(message)
        return admitted

    def cancel(self) -> None:
        """Fence only this original; its existing pending receipt still observes ACK."""
        if self._cancelled:
            return
        deferred = self._deferred()
        pending = self._pending()
        if deferred is not None or pending is not None:
            # A failed Native fence retains these exact owners for the caller's
            # existing cancellation retry. Accepted claims cannot be rolled back.
            self._capture.cancel()
            if deferred is not None:
                self._owner._deferred = None
        self._cancelled = True


class GlobalPlaybackController:
    """Admit a complete native batch, then observe its acknowledged restore intent.

    Active/paused state follows native playback telemetry. Deferred polling admits
    one prepared source-bound batch; ticket observation does not change timing.
    """

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._session = transport._session
        self._audio = transport._audio
        self._pending: _PendingGlobalBatch | None = None
        self._deferred: _DeferredStart | None = None
        self._restore_revision = 0
        self._poll_feedback: Callable[[], None] | None = None
        transport._on_frame_render_callbacks.append(self.poll)

    def set_feedback_poll(self, callback: Callable[[], None]) -> None:
        """Register the existing app-owned native playback event drain."""
        self._poll_feedback = callback

    def forget_restore(self) -> None:
        """Supersede remembered global intent after another playback gesture."""
        self._restore_revision += 1
        self._cancel_deferred()
        self._session.global_stop_engaged = False
        self._session.global_stop_restore_sample_ids = set()

    def discard_pad_restore(self, sample_id: int) -> None:
        """Prune an unloaded pad from an acknowledgement still awaiting observation."""
        self._session.global_stop_restore_sample_ids.discard(sample_id)
        self._session.global_stop_engaged = bool(self._session.global_stop_restore_sample_ids)
        if self._pending is not None:
            self._pending = replace(
                self._pending, restore_ids=self._pending.restore_ids - {sample_id}
            )

    def _pair_publication_pending(self) -> bool:
        deferred_ids = getattr(self._transport, "_pair_deferred_sample_ids", ())
        if self._pending is not None:
            starts = self._pending.start_ids
            if self._pending.capture is not None:
                starts = frozenset(self._pending.capture.diagnostic_slots())
            if (self._pending.restore_ids | starts).intersection(deferred_ids):
                return True
        if self._deferred is not None:
            slots = frozenset(self._deferred.capture.diagnostic_slots())
            if slots.intersection(deferred_ids):
                return True
        return False

    def poll(self) -> None:
        """Observe completion without projecting pending/rejected work as effective."""
        if self._pair_publication_pending():
            if self._deferred is not None and self._deferred.recording_gate is not None:
                # A writer/placement hold cannot extend this original press's
                # deadline. Cancellation fences its original members, not slots.
                self._deferred_current(self._deferred)
            return
        if self._pending is None:
            self._poll_deferred()
        pending = self._pending
        if pending is None:
            return
        status = pending.ticket.publication_status()
        if status == "pending":
            return
        self._pending = None
        if pending.start and status != "accepted":
            ids = (
                frozenset(pending.capture.diagnostic_slots())
                if pending.capture is not None
                else pending.start_ids
            )
            error = (
                pending.capture.reason() if pending.capture is not None else None
            ) or "global playback start rejected"
            for sample_id in ids:
                self._transport.residency.report_launch_error(sample_id, error)
        if status != "accepted" or pending.restore_revision != self._restore_revision:
            return
        self._session.global_stop_restore_sample_ids = set(pending.restore_ids)
        self._session.global_stop_engaged = bool(pending.restore_ids)

    def _poll_deferred(self) -> None:
        deferred = self._deferred
        if deferred is None:
            return
        if not self._deferred_current(deferred):
            return
        if deferred.recording_gate is None:
            self._admit_start(deferred, from_poll=True)

    def _deferred_current(self, deferred: _DeferredStart) -> bool:
        if deferred.restore_revision != self._restore_revision:
            self._cancel_deferred()
            return False
        if time.monotonic() >= deferred.deadline:
            self._retire_deferred(deferred, "global playback readiness deadline expired")
            return False
        if deferred.capture.state() == "rejected":
            self._retire_deferred(deferred, deferred.capture.reason() or "global start rejected")
            return False
        return True

    def _incoming_target_ready(self, sample_id: int) -> bool:
        try:
            self._transport.ensure_current_source_assignment_ready(sample_id)
        except ValueError as error:
            self._transport.residency.report_launch_error(sample_id, str(error))
            return False
        return True

    def _retire_deferred(self, deferred: _DeferredStart, error: str) -> None:
        deferred.capture.cancel()
        if self._deferred is not None and self._deferred.capture is deferred.capture:
            self._deferred = None
        for sample_id in set(deferred.capture.diagnostic_slots()):
            self._transport.residency.report_launch_error(sample_id, error)

    def _cancel_deferred(self) -> frozenset[int]:
        deferred = self._deferred
        if deferred is None:
            return frozenset()
        ids = frozenset(deferred.capture.diagnostic_slots())
        deferred.capture.cancel()
        self._deferred = None
        return ids

    def _admit_start(
        self,
        deferred: _DeferredStart,
        *,
        from_poll: bool = False,
        capture_ticket: MixCaptureTicket | None = None,
    ) -> bool:
        try:
            ticket = (
                self._audio.publish_captured_global_start(deferred.capture)
                if capture_ticket is None
                else self._audio.publish_captured_global_start(
                    deferred.capture, capture=capture_ticket
                )
            )
        except (RuntimeError, ValueError) as error:
            attempts = deferred.attempts + 1
            pressure = isinstance(error, RuntimeError) and (
                "queue is full" in str(error) or "buffer may be full" in str(error)
            )
            if not from_poll and not pressure:
                self._retire_deferred(deferred, str(error))
                raise
            if pressure and attempts < 8 and time.monotonic() < deferred.deadline:
                self._deferred = replace(deferred, attempts=attempts)
            else:
                self._retire_deferred(deferred, str(error))
            return False
        else:
            if ticket is None:
                self._deferred = deferred
                return False
            self._deferred = None
            ids = frozenset(deferred.capture.diagnostic_slots())
            for sample_id in ids:
                self._transport.residency.report_launch_error(sample_id, None)
            self._pending = _PendingGlobalBatch(
                ticket,
                frozenset(),
                deferred.restore_revision,
                start=True,
                start_ids=ids,
                capture=deferred.capture,
                recording_gate=deferred.recording_gate,
            )
            return True

    def start(self, *, received_at_ns: int | None = None) -> None:
        """Start/restart the entire remembered or playing set with one input time."""
        received_at_ns = validate_input_timestamp_ns(received_at_ns)
        deferred = self._capture_start(received_at_ns)
        if deferred is not None:
            self._deferred = deferred
            self._admit_start(deferred)

    def capture_recording_start(
        self, *, received_at_ns: int | None = None
    ) -> RecordingGroupGate | None:
        """Retain the original press on control until its writer can join publication.

        The existing frame poll validates deadlines and rejection while this gate
        holds publication. It cannot start this group without the original ticket.
        """
        received_at_ns = validate_input_timestamp_ns(received_at_ns)
        deferred = self._capture_start(received_at_ns)
        if deferred is None:
            return None
        gate = _RecordingStartGate(self, deferred.capture)
        self._deferred = replace(deferred, recording_gate=gate)
        return gate

    def _capture_start(self, received_at_ns: int | None) -> _DeferredStart | None:
        self._cancel_deferred()
        self.poll()
        if self._pending is not None:
            return None
        if self._poll_feedback is not None:
            self._poll_feedback()
        target_ids = (
            self._session.global_stop_restore_sample_ids
            if self._session.global_stop_engaged
            else self._session.active_sample_ids - self._session.paused_sample_ids
        )
        if not target_ids:
            return None
        if any(not self._incoming_target_ready(sample_id) for sample_id in sorted(target_ids)):
            return None
        entries: list[
            tuple[InputRuntimePadBinding, float, float | None, ResidentWindowTicket | None]
        ] = []
        for sample_id in sorted(target_ids):
            try:
                entry = self._transport.capture_global_start_entry(sample_id)
            except ValueError as error:
                for target in target_ids:
                    self._transport.residency.report_launch_error(target, str(error))
                return None
            if entry is None:
                return None
            entries.append(entry)
        capture = self._audio.capture_global_start(entries, received_at_ns=received_at_ns)
        return _DeferredStart(
            capture,
            frozenset(target_ids),
            self._restore_revision,
            time.monotonic() + 30.0,
        )

    def stop(self, *, remember: bool, received_at_ns: int | None = None) -> None:
        """Stop the complete active/paused set after all current permits agree."""
        received_at_ns = validate_input_timestamp_ns(received_at_ns)
        # Failed native revocation admits no STOP. Retain original deferred owners
        # until the whole cancellation fence succeeds; rendering can report failure.
        admitted_ids = self._transport.residency.cancel_all_launches()
        deferred_ids = self._cancel_deferred()
        prior_start_ids = (
            (
                self._pending.start_ids
                if self._pending is not None and self._pending.start
                else frozenset()
            )
            | admitted_ids
            | deferred_ids
        )
        self.poll()
        if self._pending is not None:
            if not self._pending.start:
                return
            # The common native stop revision now fences this prior queued start.
            self._pending = None
        if self._poll_feedback is not None:
            self._poll_feedback()
        # A claimed START can have effective voices before their telemetry arrives.
        # Queue STOP for those targets behind that tail, even with empty UI state.
        active_ids = frozenset(self._session.active_sample_ids) | prior_start_ids
        if not active_ids:
            return
        bindings: list[InputRuntimePadBinding] = []
        for sample_id in sorted(active_ids):
            self._transport.residency.cancel_requested(sample_id)
            timing = self._transport.bpm.current_timing(sample_id)
            snapshot = capture_current_pad_binding(self._audio, sample_id, timing=timing)
            if snapshot is None:
                return
            bindings.append(snapshot.binding)
        restore_ids = active_ids - self._session.paused_sample_ids if remember else frozenset()
        ticket = self._audio.stop_global_playback_batch(bindings, received_at_ns=received_at_ns)
        self._pending = _PendingGlobalBatch(ticket, frozenset(restore_ids), self._restore_revision)
